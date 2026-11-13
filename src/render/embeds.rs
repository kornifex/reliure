//! The embeds pass: `![[Note]]` puts the note's body in place, and `![[Note#Heading]]` the
//! section under that heading. The embedded events go through this pass too, so embeds nest.
//!
//! An embed stays a reference, a wikilink the links pass reads, when its note is missing or
//! already being embedded higher up, or when it is written where blocks can't go: in a
//! heading, a table cell, or emphasis.
//!
//! `![[photo.png]]` shows an image, inline wherever it is written. Any other attachment, or a
//! missing one, shows its name.
use std::mem;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};

use super::headings;
use super::images::{self, Image};
use crate::note::Note;
use crate::vault::{Lookup, Vault, display_paths};

/// The embeds pass: replaces the embeds of `events`, the page of the note at `page`, with the
/// content of the notes of `vault` they designate, or with the images they show. Gives the
/// images too, each once, in the order they first show.
///
/// Fails when an embed matches several notes or attachments, naming the embedded notes it is
/// in.
pub(super) fn rewrite<'a, 'v: 'a>(
    events: Vec<Event<'a>>,
    page: &'a Path,
    vault: &'v Vault,
) -> Result<(Vec<Event<'a>>, Vec<Image<'v>>)> {
    let mut embedder = Embedder {
        vault,
        embedding: vec![page],
        images: Vec::new(),
    };
    let events = embedder.rewrite(events)?;
    Ok((events, embedder.images))
}

/// A `![[…]]` as written, which pulldown-cmark reads as an image.
struct Embed<'a> {
    has_pothole: bool,
    dest_url: CowStr<'a>,
    title: CowStr<'a>,
    id: CowStr<'a>,
    /// The events between its start and its end: its target, or its alias, as text.
    text: Vec<Event<'a>>,
}

impl<'a> Embed<'a> {
    /// The embed opened by `start`, reading its text from `events` up to its end. `None` when
    /// `start` opens anything else.
    fn read(start: &Event<'a>, events: &mut impl Iterator<Item = Event<'a>>) -> Option<Self> {
        let Event::Start(Tag::Image {
            link_type: LinkType::WikiLink { has_pothole },
            dest_url,
            title,
            id,
        }) = start
        else {
            return None;
        };
        Some(Embed {
            has_pothole: *has_pothole,
            dest_url: dest_url.clone(),
            title: title.clone(),
            id: id.clone(),
            text: events
                .take_while(|event| !matches!(event, Event::End(TagEnd::Image)))
                .collect(),
        })
    }

    /// The embed as a reference: the same wikilink without its `!`, which the links pass
    /// turns into a link to the note's page, or into text.
    fn into_reference(self) -> Vec<Event<'a>> {
        let start = Event::Start(Tag::Link {
            link_type: LinkType::WikiLink {
                has_pothole: self.has_pothole,
            },
            dest_url: self.dest_url,
            title: self.title,
            id: self.id,
        });
        let mut events = Vec::with_capacity(self.text.len() + 2);
        events.push(start);
        events.extend(self.text);
        events.push(Event::End(TagEnd::Link));
        events
    }

    /// The text alternative of an image written after `|`, unless it is a size.
    fn alt(&self) -> Option<String> {
        if !self.has_pothole {
            return None;
        }
        let text: String = self
            .text
            .iter()
            .filter_map(|event| match event {
                Event::Text(text) | Event::Code(text) => Some(text.as_ref()),
                _ => None,
            })
            .collect();
        let text = text.trim();
        (!text.is_empty() && !images::is_size(text)).then(|| text.to_string())
    }
}

/// What an embed becomes.
enum Placed<'a> {
    /// The blocks of a note or of a section, which no paragraph may hold.
    Blocks(Vec<Event<'a>>),
    /// Inline events: a reference, an image, or an attachment's name.
    Inline(Vec<Event<'a>>),
}

/// Replaces embeds with the content of their notes, recursively, and with their images.
struct Embedder<'a, 'v> {
    vault: &'v Vault,
    /// The paths of the notes being embedded, starting with the page's own: embedding one of
    /// them again would never end.
    embedding: Vec<&'a Path>,
    /// The images shown so far, each once.
    images: Vec<Image<'v>>,
}

impl<'a, 'v: 'a> Embedder<'a, 'v> {
    /// Replaces the embeds of `events`.
    fn rewrite(&mut self, events: Vec<Event<'a>>) -> Result<Vec<Event<'a>>> {
        let mut out = Vec::with_capacity(events.len());
        // One entry per open tag: whether blocks may go inside it. Only a tight list item
        // holds inline content outside a paragraph and may take blocks; paragraphs are read
        // whole, so never open here.
        let mut open: Vec<bool> = Vec::new();
        let mut events = events.into_iter();
        while let Some(event) = events.next() {
            if let Some(embed) = Embed::read(&event, &mut events) {
                let blocks_allowed = open.last().is_none_or(|&allowed| allowed);
                match self.place(embed, blocks_allowed)? {
                    Placed::Blocks(placed) | Placed::Inline(placed) => out.extend(placed),
                }
                continue;
            }
            match event {
                Event::Start(Tag::Paragraph) => {
                    let content = events
                        .by_ref()
                        .take_while(|event| !matches!(event, Event::End(TagEnd::Paragraph)))
                        .collect();
                    self.paragraph(content, &mut out)?;
                }
                Event::Start(tag) => {
                    open.push(matches!(tag, Tag::Item));
                    out.push(Event::Start(tag));
                }
                Event::End(tag) => {
                    open.pop();
                    out.push(Event::End(tag));
                }
                other => out.push(other),
            }
        }
        Ok(out)
    }

    /// Replaces the embeds of a paragraph's `content`, pushing it to `out`. An embed of blocks
    /// closes the paragraph before it and opens it again after, as a paragraph can't hold
    /// blocks; the parts of the paragraph left blank are dropped. An embed of nothing, such as
    /// an empty note, leaves the paragraph whole.
    fn paragraph(&mut self, content: Vec<Event<'a>>, out: &mut Vec<Event<'a>>) -> Result<()> {
        let mut part = Vec::with_capacity(content.len());
        // The inline tags open around an event, such as emphasis: blocks can't go inside them.
        let mut depth = 0usize;
        // Inline HTML came before in the paragraph: it may open a tag that blocks can't go in,
        // such as `<mark>`, and closing the paragraph would leave it unclosed.
        let mut after_html = false;
        let mut events = content.into_iter();
        while let Some(event) = events.next() {
            if let Some(embed) = Embed::read(&event, &mut events) {
                match self.place(embed, depth == 0 && !after_html)? {
                    Placed::Blocks(blocks) if blocks.is_empty() => {}
                    Placed::Blocks(blocks) => {
                        push_paragraph(out, mem::take(&mut part));
                        out.extend(blocks);
                    }
                    Placed::Inline(inline) => part.extend(inline),
                }
                continue;
            }
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) => depth -= 1,
                Event::InlineHtml(_) => after_html = true,
                _ => {}
            }
            part.push(event);
        }
        push_paragraph(out, part);
        Ok(())
    }

    /// What `embed` becomes, `blocks_allowed` telling whether blocks may go where it is
    /// written.
    fn place(&mut self, embed: Embed<'a>, blocks_allowed: bool) -> Result<Placed<'a>> {
        let (target, subpath) = match embed.dest_url.split_once('#') {
            Some((target, subpath)) => (target.to_string(), Some(subpath.to_string())),
            None => (embed.dest_url.to_string(), None),
        };
        let note = match self.vault.get(&target) {
            Lookup::Found(note) => note,
            Lookup::Missing if is_attachment(&target) => return self.attachment(&embed, target),
            Lookup::Missing => return Ok(Placed::Inline(embed.into_reference())),
            Lookup::Ambiguous(notes) => bail!(
                "cannot embed ![[{}]], which several notes match: {}",
                embed.dest_url,
                display_paths(notes)
            ),
        };
        if !blocks_allowed || self.embedding.contains(&note.path.as_path()) {
            return Ok(Placed::Inline(embed.into_reference()));
        }
        let body = match subpath.filter(|subpath| !subpath.starts_with('^')) {
            None => note.body.as_str(),
            Some(heading) => match section(&note.body, &heading) {
                Some(section) => section,
                None => return Ok(Placed::Inline(embed.into_reference())),
            },
        };
        let events = point_heading_links_at(super::parse(body).collect(), note);
        self.embedding.push(&note.path);
        let blocks = self.rewrite(events);
        self.embedding.pop();
        let blocks =
            blocks.with_context(|| format!("in the embedded note {}", note.path.display()))?;
        Ok(Placed::Blocks(blocks))
    }

    /// What `embed` of the attachment `target` becomes: an `<img>` when it is an image of the
    /// vault, its name otherwise.
    fn attachment(&mut self, embed: &Embed<'a>, target: String) -> Result<Placed<'a>> {
        let image = match self.vault.attachment(&target) {
            Lookup::Found(attachment) => Image::new(attachment),
            Lookup::Missing => None,
            Lookup::Ambiguous(attachments) => bail!(
                "cannot embed ![[{}]], which several attachments match: {}",
                embed.dest_url,
                display_paths(attachments)
            ),
        };
        let Some(image) = image else {
            return Ok(Placed::Inline(vec![Event::Text(target.into())]));
        };
        let alt = embed.alt().unwrap_or_else(|| image.attachment.name.clone());
        let tag = image.tag(&alt);
        if !self
            .images
            .iter()
            .any(|shown| shown.attachment.path == image.attachment.path)
        {
            self.images.push(image);
        }
        Ok(Placed::Inline(vec![Event::InlineHtml(tag.into())]))
    }
}

/// Pushes `part` of a paragraph's content to `out` as a paragraph, without the line breaks at
/// its ends, unless nothing but spaces is left of it.
fn push_paragraph<'a>(out: &mut Vec<Event<'a>>, part: Vec<Event<'a>>) {
    let is_break = |event: &Event| matches!(event, Event::SoftBreak | Event::HardBreak);
    let is_blank = |event: &Event| {
        is_break(event) || matches!(event, Event::Text(text) if text.trim().is_empty())
    };
    if part.iter().all(is_blank) {
        return;
    }
    let start = part
        .iter()
        .position(|event| !is_break(event))
        .expect("a part that isn't blank has an event other than a line break");
    let end = part
        .iter()
        .rposition(|event| !is_break(event))
        .expect("a part that isn't blank has an event other than a line break");
    out.push(Event::Start(Tag::Paragraph));
    out.extend(part.into_iter().take(end + 1).skip(start));
    out.push(Event::End(TagEnd::Paragraph));
}

/// Whether an embed's target, missing from the notes, names an attachment: a file with an
/// extension other than `.md`, such as `photo.png`.
fn is_attachment(target: &str) -> bool {
    Path::new(target)
        .extension()
        .is_some_and(|extension| !extension.eq_ignore_ascii_case("md"))
}

/// The section of `body` under the heading whose text gives the same id as `text`: from that
/// heading's line, up to the next heading of the same or a higher level.
fn section<'b>(body: &'b str, text: &str) -> Option<&'b str> {
    let headings = headings::headings(body);
    let heading = headings::find(&headings, text)?;
    let end = headings
        .iter()
        .find(|next| next.range.start > heading.range.start && next.level <= heading.level)
        .map_or(body.len(), |next| next.range.start);
    Some(&body[heading.range.start..end])
}

/// Points the `[[#Heading]]` links and `![[#Heading]]` embeds of the events of `note`,
/// embedded in a page, at that note: they would lead to the page's own headings otherwise.
/// They keep showing `Heading`.
fn point_heading_links_at<'a>(events: Vec<Event<'a>>, note: &Note) -> Vec<Event<'a>> {
    let path = link_path(&note.path);
    let mut out = Vec::with_capacity(events.len());
    // Inside such a link or embed without an alias, whose text is replaced.
    let mut text_replaced = false;
    for event in events {
        let is_embed = matches!(event, Event::Start(Tag::Image { .. }));
        match event {
            Event::Start(
                Tag::Link {
                    link_type: LinkType::WikiLink { has_pothole },
                    dest_url,
                    title,
                    id,
                }
                | Tag::Image {
                    link_type: LinkType::WikiLink { has_pothole },
                    dest_url,
                    title,
                    id,
                },
            ) if dest_url.starts_with('#') => {
                let link_type = LinkType::WikiLink { has_pothole: true };
                let heading = dest_url[1..].to_string();
                let dest_url = format!("{path}{dest_url}").into();
                out.push(Event::Start(if is_embed {
                    Tag::Image {
                        link_type,
                        dest_url,
                        title,
                        id,
                    }
                } else {
                    Tag::Link {
                        link_type,
                        dest_url,
                        title,
                        id,
                    }
                }));
                if !has_pothole {
                    out.push(Event::Text(heading.into()));
                    text_replaced = true;
                }
            }
            Event::End(end @ (TagEnd::Link | TagEnd::Image)) => {
                text_replaced = false;
                out.push(Event::End(end));
            }
            _ if text_replaced => {}
            other => out.push(other),
        }
    }
    out
}

/// The link target designating the note at `path` from the vault's root, which no other note
/// matches: `Folder/Note.md` → `Folder/Note`.
fn link_path(path: &Path) -> String {
    let path: PathBuf = path.with_extension("");
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::lang::Lang;
    use crate::note::Note;
    use crate::render::{BoundNotes, to_xhtml};
    use crate::vault::{Attachment, Lookup, Vault};

    const LOOP_A: &str = "A text.\n\n![[Loop B]]\n";
    const SELFIE: &str = "Me.\n\n![[Selfie]]\n";
    const PHOTO: &str = "<img src=\"images/0ec41df3-Photo.PNG\" alt=\"Photo.PNG\"/>";

    /// Renders `markdown` as the page of the note at `page`, in a book binding "Loop A" and
    /// "folder/Guide"; the other notes are only in the vault.
    fn try_render_in(page: &str, markdown: &str) -> anyhow::Result<String> {
        render_with_images(page, markdown).map(|(xhtml, _)| xhtml)
    }

    /// Renders `markdown` as `try_render_in` does, with the files in the book of the images the
    /// page shows.
    fn render_with_images(page: &str, markdown: &str) -> anyhow::Result<(String, Vec<String>)> {
        let vault = Vault::from_notes([
            Note::at("Aside.md", "Aside text.\n"),
            Note::at("Empty.md", ""),
            Note::at("Outer.md", "Outer text.\n\n![[Aside]]\n"),
            Note::at("Loop A.md", LOOP_A),
            Note::at("Loop B.md", "B text.\n\n![[Loop A]]\n"),
            Note::at("Selfie.md", SELFIE),
            Note::at(
                "Rules.md",
                "Intro.\n\n## Combat\nFight.\n### Initiative\nRoll.\n# Magic\nSpells.\n",
            ),
            Note::at(
                "folder/Guide.md",
                "See [[#Steps]] and [[#Steps|the steps]].\n\n![[#Steps]]\n\n## Steps\nOne.\n",
            ),
            Note::at("Uses twin.md", "![[Twin]]\n"),
            Note::at("one/Twin.md", ""),
            Note::at("two/Twin.md", ""),
            Note::at("Gallery.md", "![[photo.png]]\n\n![[one/Map.png]]\n"),
        ])
        .with_attachments([
            Attachment::at("maps/Photo.PNG"),
            Attachment::at("one/Map.png"),
            Attachment::at("two/Map.png"),
            Attachment::at("Été/Plan 1.png"),
            Attachment::at("docs/File.pdf"),
            Attachment::at("sounds/Theme.mp3"),
            Attachment::at("Tasks.base"),
        ]);
        let mut notes = BoundNotes::new(&vault);
        for (target, file) in [("Loop A", "note-0.xhtml"), ("Guide", "note-1.xhtml")] {
            let Lookup::Found(note) = vault.get(target) else {
                unreachable!("{target} is in the vault");
            };
            notes.insert(note, file.into());
        }
        let rendered = to_xhtml(markdown, Path::new(page), Lang::En, &notes)?;
        let images = rendered.images.iter().map(|image| image.file.clone());
        Ok((rendered.xhtml, images.collect()))
    }

    /// Renders `markdown` as the page of a note that is not in the vault.
    fn try_render(markdown: &str) -> anyhow::Result<String> {
        try_render_in("Page.md", markdown)
    }

    fn render(markdown: &str) -> String {
        try_render(markdown).unwrap()
    }

    #[test]
    fn an_embed_alone_in_its_paragraph_puts_the_note_in_place() {
        assert_eq!(
            render("Before.\n\n![[Aside]]\n\nAfter.\n"),
            "<p>Before.</p>\n<p>Aside text.</p>\n<p>After.</p>\n"
        );
    }

    #[test]
    fn an_embed_inside_a_sentence_splits_its_paragraph() {
        assert_eq!(
            render("See ![[Aside]] here,\n![[Aside]]\nand there.\n"),
            "<p>See </p>\n<p>Aside text.</p>\n<p> here,</p>\n<p>Aside text.</p>\n<p>and there.</p>\n"
        );
    }

    #[test]
    fn an_embed_in_a_tight_list_item_goes_inside_it() {
        assert_eq!(
            render("- ![[Aside]]\n- Next\n"),
            "<ul>\n<li>\n<p>Aside text.</p>\n</li>\n<li>Next</li>\n</ul>\n"
        );
    }

    #[test]
    fn embeds_nest() {
        assert_eq!(
            render("![[Outer]]\n"),
            "<p>Outer text.</p>\n<p>Aside text.</p>\n"
        );
    }

    #[test]
    fn a_note_already_being_embedded_is_a_reference() {
        assert_eq!(
            render("![[Loop B]]\n"),
            "<p>B text.</p>\n<p>A text.</p>\n<p>Loop B</p>\n"
        );
    }

    #[test]
    fn an_embed_of_the_page_note_further_down_is_a_reference() {
        assert_eq!(
            try_render_in("Loop A.md", LOOP_A).unwrap(),
            "<p>A text.</p>\n<p>B text.</p>\n<p><a href=\"note-0.xhtml\">Loop A</a></p>\n"
        );
    }

    #[test]
    fn a_note_embedding_itself_shows_a_reference() {
        assert_eq!(
            try_render_in("Selfie.md", SELFIE).unwrap(),
            "<p>Me.</p>\n<p>Selfie</p>\n"
        );
    }

    #[test]
    fn an_empty_note_embedded_in_a_sentence_leaves_its_paragraph_whole() {
        assert_eq!(
            render("Before ![[Empty]] after.\n"),
            "<p>Before  after.</p>\n"
        );
    }

    #[test]
    fn an_embed_after_inline_html_in_its_paragraph_is_a_reference() {
        assert_eq!(
            render("<mark>See ![[Aside]] here</mark>\n"),
            "<p><mark>See Aside here</mark></p>\n"
        );
    }

    #[test]
    fn a_section_embed_stops_at_a_heading_of_the_same_or_a_higher_level() {
        assert_eq!(
            render("![[Rules#combat]]\n"),
            "<h2>Combat</h2>\n<p>Fight.</p>\n<h3>Initiative</h3>\n<p>Roll.</p>\n"
        );
    }

    #[test]
    fn an_embed_of_a_missing_heading_is_a_reference() {
        assert_eq!(
            render("![[Rules#Nowhere]]\n"),
            "<p>Rules &gt; Nowhere</p>\n"
        );
    }

    #[test]
    fn an_embed_of_a_block_puts_the_whole_note_in_place() {
        assert_eq!(render("![[Aside#^b1]]\n"), "<p>Aside text.</p>\n");
    }

    #[test]
    fn an_embed_of_a_missing_note_is_a_reference() {
        assert_eq!(
            render("![[Missing]] and ![[Missing|alias]]\n"),
            "<p>Missing and alias</p>\n"
        );
    }

    #[test]
    fn an_embed_several_notes_match_fails_naming_their_paths() {
        let error = try_render("![[Twin]]\n").unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot embed ![[Twin]], which several notes match: one/Twin.md, two/Twin.md"
        );
    }

    #[test]
    fn an_ambiguous_embed_in_an_embedded_note_names_that_note() {
        let error = try_render("![[Uses twin]]\n").unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "in the embedded note Uses twin.md: cannot embed ![[Twin]], which several notes match: one/Twin.md, two/Twin.md"
        );
    }

    #[test]
    fn heading_links_of_an_embedded_note_lead_to_its_headings() {
        assert_eq!(
            render("## Steps\n\n![[Guide]]\n"),
            "<h2 id=\"steps\">Steps</h2>\n\
             <p>See <a href=\"note-1.xhtml#steps\">Steps</a> and <a href=\"note-1.xhtml#steps\">the steps</a>.</p>\n\
             <p><a href=\"note-1.xhtml#steps\">Steps</a></p>\n\
             <h2>Steps</h2>\n<p>One.</p>\n"
        );
    }

    #[test]
    fn only_the_page_own_headings_get_an_id() {
        assert_eq!(
            render("## Combat\n\n![[Rules#Combat]]\n\nSee [[#Combat]].\n"),
            "<h2 id=\"combat\">Combat</h2>\n<h2>Combat</h2>\n<p>Fight.</p>\n<h3>Initiative</h3>\n<p>Roll.</p>\n<p>See <a href=\"#combat\">Combat</a>.</p>\n"
        );
    }

    #[test]
    fn an_embed_in_a_heading_or_a_table_cell_is_a_reference() {
        assert_eq!(
            render("## ![[Loop A]]\n\n| A | B |\n|---|---|\n| ![[Aside]] | x |\n"),
            "<h2 id=\"loop-a\"><a href=\"note-0.xhtml\">Loop A</a></h2>\n\
             <table><thead><tr><th>A</th><th>B</th></tr></thead><tbody>\n\
             <tr><td>Aside</td><td>x</td></tr>\n</tbody></table>\n"
        );
    }

    #[test]
    fn an_embed_inside_emphasis_is_a_reference() {
        assert_eq!(render("*![[Aside]]*\n"), "<p><em>Aside</em></p>\n");
    }

    #[test]
    fn an_embed_in_a_callout_goes_inside_it() {
        assert_eq!(
            render("> [!tip]\n> ![[Aside]]\n"),
            "<div class=\"callout callout-tip\">\n<p class=\"callout-title\"><strong>Tip</strong></p>\n<p>Aside text.</p>\n</div>\n"
        );
    }

    #[test]
    fn an_image_shows_in_its_sentence_under_the_exact_name_of_its_file() {
        assert_eq!(
            render("See ![[photo.png]] here.\n"),
            format!("<p>See {PHOTO} here.</p>\n")
        );
    }

    #[test]
    fn a_size_is_ignored_and_other_text_is_the_alt() {
        assert_eq!(
            render("![[photo.png|300]] ![[photo.png|300x200]] ![[photo.png|The \"old\" map]]\n"),
            format!(
                "<p>{PHOTO} {PHOTO} \
                 <img src=\"images/0ec41df3-Photo.PNG\" alt=\"The &quot;old&quot; map\"/></p>\n"
            )
        );
    }

    #[test]
    fn an_image_goes_in_a_heading_a_table_cell_emphasis_or_a_list_item() {
        assert_eq!(
            render(
                "## ![[photo.png]] Map\n\n| A | B |\n|---|---|\n| ![[photo.png]] | x |\n\n\
                 *![[photo.png]]*\n\n- ![[photo.png]]\n"
            ),
            format!(
                "<h2 id=\"photopng-map\">{PHOTO} Map</h2>\n\
                 <table><thead><tr><th>A</th><th>B</th></tr></thead><tbody>\n\
                 <tr><td>{PHOTO}</td><td>x</td></tr>\n</tbody></table>\n\
                 <p><em>{PHOTO}</em></p>\n<ul>\n<li>{PHOTO}</li>\n</ul>\n"
            )
        );
    }

    #[test]
    fn images_sharing_a_name_in_different_folders_stay_apart() {
        assert_eq!(
            render("![[one/Map.png]] ![[two/Map.png]]\n"),
            "<p><img src=\"images/a10d2e75-Map.png\" alt=\"Map.png\"/> \
             <img src=\"images/91a17244-Map.png\" alt=\"Map.png\"/></p>\n"
        );
    }

    #[test]
    fn an_image_file_in_the_book_has_a_name_safe_for_a_url() {
        assert_eq!(
            render("![[Plan 1.png]]\n"),
            "<p><img src=\"images/44414ae0-Plan_1.png\" alt=\"Plan 1.png\"/></p>\n"
        );
    }

    #[test]
    fn an_embed_several_attachments_match_fails_naming_their_paths() {
        let error = try_render("![[Map.png]]\n").unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot embed ![[Map.png]], which several attachments match: one/Map.png, two/Map.png"
        );
    }

    #[test]
    fn a_missing_image_or_another_attachment_shows_its_name() {
        assert_eq!(
            render("![[missing.png|300]] and ![[docs/File.pdf]] and ![[File.pdf|doc]]\n"),
            "<p>missing.png and docs/File.pdf and File.pdf</p>\n"
        );
    }

    #[test]
    fn an_audio_file_or_a_base_shows_its_name_and_is_no_image() {
        let (xhtml, images) =
            render_with_images("Page.md", "![[Theme.mp3]] and ![[tasks.base|Tasks]]\n").unwrap();
        assert_eq!(xhtml, "<p>Theme.mp3 and tasks.base</p>\n");
        assert!(images.is_empty(), "{images:?}");
    }

    #[test]
    fn a_page_tells_each_image_it_shows_once() {
        let (_, images) = render_with_images(
            "Page.md",
            "![[photo.png]] ![[missing.png]] ![[File.pdf]]\n\n![[Gallery]]\n\n![[PHOTO.png|alt]]\n",
        )
        .unwrap();
        assert_eq!(
            images,
            ["images/0ec41df3-Photo.PNG", "images/a10d2e75-Map.png"]
        );
    }
}
