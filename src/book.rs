//! The plan of a book, read from its hub.
use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Result, bail};
use pulldown_cmark::{Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};

use crate::note::Note;
use crate::render::is_size;
use crate::vault::{Lookup, Vault, display_paths};

pub struct Book<'a> {
    pub title: String,
    /// The hub's path in the vault, whose text the intro and the chapters are.
    pub hub: PathBuf,
    /// The hub's text before its first `##` heading.
    pub intro: String,
    pub chapters: Vec<Chapter<'a>>,
}

/// A `##` section of the hub: its text opens the chapter, then come the notes it binds.
pub struct Chapter<'a> {
    pub title: String,
    pub text: String,
    pub notes: Vec<&'a Note>,
}

/// Reads the plan of a book from its hub, finding the notes it binds in `vault`.
///
/// A note enters the book when a list item of a `##` section opens with a link to it
/// (`- **[[Note]]** : summary`), under the first chapter that lists it. Links elsewhere in
/// the text, and links to notes missing from the vault, stay references.
///
/// Fails when the hub binds a link that several notes of the vault match.
pub fn plan<'a>(hub: &Note, vault: &'a Vault) -> Result<Book<'a>> {
    let (intro, sections) = split_sections(&hub.body);
    let mut bound = HashSet::from([hub.path.as_path()]);
    let mut chapters = Vec::new();
    for (title, text) in sections {
        let mut notes = Vec::new();
        for target in listed_notes(text) {
            match vault.get(&target) {
                Lookup::Found(note) => {
                    if bound.insert(&note.path) {
                        notes.push(note);
                    }
                }
                Lookup::Missing => {}
                // The hub listing itself binds nothing, even when another note has its name.
                Lookup::Ambiguous(namesakes)
                    if namesakes.iter().any(|note| note.path == hub.path) => {}
                Lookup::Ambiguous(namesakes) => bail!(
                    "book {} binds [[{target}]], which several notes match: {}",
                    hub.name,
                    display_paths(namesakes)
                ),
            }
        }
        chapters.push(Chapter {
            title,
            text: text.to_string(),
            notes,
        });
    }
    Ok(Book {
        title: hub.name.clone(),
        hub: hub.path.clone(),
        intro: intro.to_string(),
        chapters,
    })
}

/// The note a link target points to, as written: `Note#Heading` → `Note`,
/// `Folder/Note#Heading` → `Folder/Note`.
pub fn note_name(target: &str) -> &str {
    target.split_once('#').map_or(target, |(name, _)| name)
}

/// Splits a body at its `##` headings: the text before the first one, then each heading's
/// title with the text up to the next one.
///
/// A title is the heading's text, links showing their text, embeds the text after their `|`
/// unless it is a size, or the name of what they embed.
fn split_sections(body: &str) -> (&str, Vec<(String, &str)>) {
    let mut headings = Vec::new();
    let mut title: Option<String> = None;
    // Inside an embed of a title: the name of what it embeds, and its text after `|`, if any.
    let mut embed: Option<(String, Option<String>)> = None;
    let mut heading_start = 0;
    for (event, range) in Parser::new_ext(body, Options::ENABLE_WIKILINKS).into_offset_iter() {
        match event {
            Event::Start(Tag::Image {
                link_type: LinkType::WikiLink { has_pothole },
                dest_url,
                ..
            }) if title.is_some() => {
                let target = note_name(&dest_url);
                let name = target.rsplit_once('/').map_or(target, |(_, name)| name);
                embed = Some((name.to_string(), has_pothole.then(String::new)));
            }
            Event::End(TagEnd::Image) => {
                if let (Some((name, alias)), Some(title)) = (embed.take(), title.as_mut()) {
                    let alias = alias.as_deref().map(str::trim);
                    let shown = alias.filter(|alias| !alias.is_empty() && !is_size(alias));
                    title.push_str(shown.unwrap_or(&name));
                }
            }
            Event::Text(text) | Event::Code(text) if embed.is_some() => {
                if let Some((_, Some(alias))) = embed.as_mut() {
                    alias.push_str(&text);
                }
            }
            Event::Start(Tag::Heading {
                level: HeadingLevel::H2,
                ..
            }) => {
                heading_start = range.start;
                title = Some(String::new());
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2)) => {
                let title = title.take().expect("set by the matching Start event");
                headings.push((title, heading_start, range.end));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(title) = title.as_mut() {
                    title.push_str(&text);
                }
            }
            _ => {}
        }
    }

    let intro_end = headings.first().map_or(body.len(), |&(_, start, _)| start);
    let sections = headings
        .iter()
        .enumerate()
        .map(|(i, (title, _, text_start))| {
            let text_end = headings
                .get(i + 1)
                .map_or(body.len(), |&(_, start, _)| start);
            (title.clone(), &body[*text_start..text_end])
        })
        .collect();
    (&body[..intro_end], sections)
}

/// The notes linked at the start of a list item, in order, emphasis allowed around the link.
fn listed_notes(markdown: &str) -> Vec<String> {
    let mut at_item_start = false;
    let mut notes = Vec::new();
    for event in Parser::new_ext(markdown, Options::ENABLE_WIKILINKS) {
        match event {
            Event::Start(Tag::Item) => at_item_start = true,
            Event::Start(Tag::Paragraph | Tag::Strong | Tag::Emphasis) => {}
            Event::Start(Tag::Link {
                link_type: LinkType::WikiLink { .. },
                dest_url,
                ..
            }) if at_item_start => {
                notes.push(note_name(&dest_url).to_string());
                at_item_start = false;
            }
            _ => at_item_start = false,
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn hub(body: &str) -> Note {
        Note::at("Hub.md", body)
    }

    /// A vault holding an empty note at each of `paths`.
    fn vault(paths: &[&str]) -> Vault {
        Vault::from_notes(paths.iter().map(|path| Note::at(path, "")))
    }

    fn names<'a>(chapter: &Chapter<'a>) -> Vec<&'a str> {
        chapter
            .notes
            .iter()
            .map(|note| note.name.as_str())
            .collect()
    }

    fn paths<'a>(chapter: &Chapter<'a>) -> Vec<&'a Path> {
        chapter
            .notes
            .iter()
            .map(|note| note.path.as_path())
            .collect()
    }

    #[test]
    fn sections_become_chapters_listing_their_notes() {
        let hub =
            hub("Intro\n\n## Design\n- **[[A]]** : a\n- [[B|bee]]\n\n## Table\n- **[[C]]** : c\n");
        let vault = vault(&["A.md", "B.md", "C.md"]);
        let book = plan(&hub, &vault).unwrap();
        assert_eq!(book.intro, "Intro\n\n");
        let chapters: Vec<_> = book
            .chapters
            .iter()
            .map(|c| (c.title.as_str(), names(c)))
            .collect();
        assert_eq!(chapters, [("Design", vec!["A", "B"]), ("Table", vec!["C"])]);
        assert_eq!(book.chapters[1].text, "- **[[C]]** : c\n");
    }

    #[test]
    fn an_embed_in_a_chapter_heading_shows_its_alt_or_its_name() {
        let hub = hub(
            "## Gallery ![[maps/a_b.png]], ![[x.png|The map]], ![[y.png|300]] and ![[Folder/Note#Part]]\n",
        );
        let vault = vault(&[]);
        let book = plan(&hub, &vault).unwrap();
        assert_eq!(
            book.chapters[0].title,
            "Gallery a_b.png, The map, y.png and Note"
        );
    }

    #[test]
    fn a_note_listed_twice_is_bound_once() {
        let hub = hub("## One\n- [[A]]\n## Two\n- [[A]] (see One)\n- [[B]]\n");
        let vault = vault(&["A.md", "B.md"]);
        let book = plan(&hub, &vault).unwrap();
        assert_eq!(names(&book.chapters[1]), ["B"]);
    }

    #[test]
    fn links_inside_text_and_missing_notes_stay_references() {
        let hub =
            hub("## Words\n- **Clue**: leads to a [[Revelation]].\n- [[Missing]]\n- [[A#Part]]\n");
        let vault = vault(&["A.md", "Revelation.md"]);
        let book = plan(&hub, &vault).unwrap();
        assert_eq!(names(&book.chapters[0]), ["A"]);
    }

    #[test]
    fn a_book_binding_an_ambiguous_name_fails_naming_its_paths() {
        let hub = hub("## Words\n- [[A]]\n");
        let vault = vault(&["two/A.md", "one/A.md"]);
        let Err(error) = plan(&hub, &vault) else {
            panic!("A is ambiguous");
        };
        assert_eq!(
            error.to_string(),
            "book Hub binds [[A]], which several notes match: one/A.md, two/A.md"
        );
    }

    #[test]
    fn an_ambiguous_name_no_book_uses_is_ignored() {
        let hub = hub("## Words\n- [[A]]\n");
        let vault = vault(&["A.md", "one/README.md", "two/README.md"]);
        let book = plan(&hub, &vault).unwrap();
        assert_eq!(names(&book.chapters[0]), ["A"]);
    }

    #[test]
    fn an_ambiguous_name_used_as_a_reference_is_ignored() {
        let hub = hub("## Words\n- **Clue**: [[README]]\n- [[A]], see [[README]]\n");
        let vault = vault(&["A.md", "one/README.md", "two/README.md"]);
        let book = plan(&hub, &vault).unwrap();
        assert_eq!(names(&book.chapters[0]), ["A"]);
    }

    #[test]
    fn a_path_link_binds_the_note_at_that_path() {
        let hub = hub("## Words\n- [[two/A]]\n");
        let vault = vault(&["one/A.md", "two/A.md"]);
        let book = plan(&hub, &vault).unwrap();
        assert_eq!(paths(&book.chapters[0]), [Path::new("two/A.md")]);
    }

    #[test]
    fn a_path_link_matching_several_notes_fails() {
        let hub = hub("## Words\n- [[x/A]]\n");
        let vault = vault(&["one/x/A.md", "two/x/A.md"]);
        let Err(error) = plan(&hub, &vault) else {
            panic!("x/A is ambiguous");
        };
        assert_eq!(
            error.to_string(),
            "book Hub binds [[x/A]], which several notes match: one/x/A.md, two/x/A.md"
        );
    }

    #[test]
    fn a_note_reached_by_name_and_by_path_is_bound_once() {
        let hub = hub("## One\n- [[b]]\n## Two\n- [[dir/B]]\n");
        let vault = vault(&["dir/B.md"]);
        let book = plan(&hub, &vault).unwrap();
        assert_eq!(names(&book.chapters[0]), ["B"]);
        assert!(book.chapters[1].notes.is_empty());
    }

    #[test]
    fn a_hub_listing_itself_binds_nothing_even_when_its_name_is_shared() {
        let hub = hub("## Words\n- [[Hub]]\n- [[A]]\n");
        let vault = vault(&["Hub.md", "other/Hub.md", "A.md"]);
        let book = plan(&hub, &vault).unwrap();
        assert_eq!(names(&book.chapters[0]), ["A"]);
    }

    #[test]
    fn a_note_embedded_in_a_hub_is_not_bound() {
        let hub = hub("## Design\n- ![[A]]\n- **![[B]]**\n- [[C]]\n");
        let vault = vault(&["A.md", "B.md", "C.md"]);
        let book = plan(&hub, &vault).unwrap();
        assert_eq!(names(&book.chapters[0]), ["C"]);
    }

    #[test]
    fn a_hub_whose_list_item_is_commented_out_binds_nothing() {
        let hub = crate::note::parse(
            "Hub".into(),
            "Hub.md".into(),
            "## Design\n%%\n- [[A]]\n%%\n- %%[[B]]%%\n",
        )
        .unwrap();
        let vault = vault(&["A.md", "B.md"]);
        let book = plan(&hub, &vault).unwrap();
        assert!(book.chapters[0].notes.is_empty());
    }
}
