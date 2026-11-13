//! The `[[links]]` pass: a link to a note of the book leads to its page, or to a heading of
//! it; a link to a note outside the book keeps its text and loses its link.
use std::collections::HashMap;

use pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};
use pulldown_cmark_escape::escape_html;

use super::headings::{self, Heading};

/// The notes bound in a book, as links reach them: each one's page and headings.
#[derive(Default)]
pub struct BoundNotes<'a> {
    pages: HashMap<&'a str, NotePage>,
}

/// The page of a bound note, and the headings a link to it can lead to.
struct NotePage {
    file: String,
    headings: Vec<Heading>,
}

impl<'a> BoundNotes<'a> {
    /// Binds the note `name`, whose body is `body`, to the page `file`.
    pub fn insert(&mut self, name: &'a str, file: String, body: &str) {
        self.pages.insert(name, NotePage { file, headings: headings::headings(body) });
    }

    /// The page of the note `name`, when it is bound.
    pub fn file(&self, name: &str) -> Option<&str> {
        self.pages.get(name).map(|page| page.file.as_str())
    }

    /// Where a link to the note `name` leads: its page, at `heading` when the note has it.
    fn href(&self, name: &str, heading: Option<&str>) -> Option<String> {
        let page = self.pages.get(name)?;
        Some(match heading.and_then(|text| headings::find(&page.headings, text)) {
            Some(heading) => format!("{}#{}", page.file, heading.id),
            None => page.file.clone(),
        })
    }
}

/// What a wikilink points to: `Note`, `Note#Heading`, `Note#^block`, or `#Heading` in the
/// same note.
struct Target<'u> {
    /// Empty for a link inside the note.
    note: &'u str,
    /// A heading's text, or a block id starting with `^`.
    subpath: Option<&'u str>,
}

impl<'u> Target<'u> {
    fn parse(dest: &'u str) -> Self {
        match dest.split_once('#') {
            Some((note, subpath)) => Target { note, subpath: Some(subpath) },
            None => Target { note: dest, subpath: None },
        }
    }

    /// Where the link leads in the book, `headings` being those of the page it is in.
    fn href(&self, notes: &BoundNotes, headings: &[Heading]) -> Option<String> {
        let heading = self.subpath.filter(|subpath| !subpath.starts_with('^'));
        if self.note.is_empty() {
            headings::find(headings, heading?).map(|heading| format!("#{}", heading.id))
        } else {
            notes.href(self.note, heading)
        }
    }

    /// The text of the link without an alias, as Obsidian's reading view shows it, when it
    /// differs from the target as written: `Note > Heading`, `Heading`, or `Note` for a block.
    fn text(&self) -> Option<String> {
        let subpath = self.subpath?;
        Some(if self.note.is_empty() {
            subpath.to_string()
        } else if subpath.starts_with('^') {
            self.note.to_string()
        } else {
            format!("{} > {subpath}", self.note)
        })
    }
}

/// What becomes of the events of a wikilink, up to its end.
struct OpenLink {
    /// It leads somewhere in the book, so keeps its end.
    kept: bool,
    /// Its text was replaced, so its own is dropped.
    text_replaced: bool,
}

/// Rewrites the wikilinks of `events`: `notes` gives where a link to a note leads, `headings`
/// are those of the page, which `[[#Heading]]` leads to.
pub(super) fn rewrite<'a>(events: Vec<Event<'a>>, notes: &BoundNotes, headings: &[Heading]) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(events.len());
    // Links don't nest, so only one is open at a time.
    let mut open: Option<OpenLink> = None;
    for event in events {
        match event {
            Event::Start(Tag::Link { link_type: LinkType::WikiLink { has_pothole }, dest_url, .. }) => {
                let target = Target::parse(&dest_url);
                let href = target.href(notes, headings);
                let text = if has_pothole { None } else { target.text() };
                open = Some(OpenLink { kept: href.is_some(), text_replaced: text.is_some() });
                out.extend(href.map(|href| Event::InlineHtml(CowStr::from(link_opening(&href)))));
                out.extend(text.map(|text| Event::Text(CowStr::from(text))));
            }
            Event::End(TagEnd::Link) if open.is_some() => {
                if open.take().is_some_and(|link| link.kept) {
                    out.push(Event::InlineHtml(CowStr::from("</a>")));
                }
            }
            _ if open.as_ref().is_some_and(|link| link.text_replaced) => {}
            other => out.push(other),
        }
    }
    out
}

/// The `<a>` tag of a link to `href`, written as HTML: pulldown-cmark would percent-encode the
/// accents of a heading's id in the href, and CrossPoint compares it to the ids as written.
fn link_opening(href: &str) -> String {
    let mut href_xhtml = String::new();
    // Writing into a String cannot fail.
    escape_html(&mut href_xhtml, href).unwrap();
    format!("<a href=\"{href_xhtml}\">")
}

#[cfg(test)]
mod tests {
    use crate::lang::Lang;
    use crate::render::{BoundNotes, to_xhtml};

    /// Renders `markdown` in a book binding the note "In book", which has a French heading.
    fn render(markdown: &str) -> String {
        let mut notes = BoundNotes::default();
        notes.insert("In book", "note-1.xhtml".into(), "Text\n\n## Méthode de préparation\n");
        to_xhtml(markdown, Lang::En, &notes)
    }

    #[test]
    fn links_inside_the_book_point_to_their_page() {
        assert_eq!(
            render("See [[In book|this note]] and [[In book]]."),
            "<p>See <a href=\"note-1.xhtml\">this note</a> and <a href=\"note-1.xhtml\">In book</a>.</p>\n"
        );
    }

    #[test]
    fn a_link_to_a_heading_leads_to_it() {
        assert_eq!(
            render("See [[In book#Méthode de préparation]]."),
            "<p>See <a href=\"note-1.xhtml#méthode-de-préparation\">In book &gt; Méthode de préparation</a>.</p>\n"
        );
    }

    #[test]
    fn an_aliased_link_to_a_heading_keeps_its_alias() {
        assert_eq!(
            render("See [[In book#Méthode de préparation|the method]]."),
            "<p>See <a href=\"note-1.xhtml#méthode-de-préparation\">the method</a>.</p>\n"
        );
    }

    #[test]
    fn a_link_to_a_heading_of_the_page_leads_to_it() {
        assert_eq!(
            render("See [[#Pièges]].\n\n## Pièges\n"),
            "<p>See <a href=\"#pièges\">Pièges</a>.</p>\n<h2 id=\"pièges\">Pièges</h2>\n"
        );
    }

    #[test]
    fn a_link_to_a_missing_heading_leads_to_the_page() {
        assert_eq!(
            render("See [[In book#Nowhere]]."),
            "<p>See <a href=\"note-1.xhtml\">In book &gt; Nowhere</a>.</p>\n"
        );
    }

    #[test]
    fn a_link_to_a_block_leads_to_the_page() {
        assert_eq!(render("See [[In book#^a1b2c3]]."), "<p>See <a href=\"note-1.xhtml\">In book</a>.</p>\n");
    }

    #[test]
    fn links_outside_the_book_keep_their_text() {
        assert_eq!(
            render("See [[Elsewhere]], [[Elsewhere#Part]] and [[#Nowhere]]."),
            "<p>See Elsewhere, Elsewhere &gt; Part and Nowhere.</p>\n"
        );
    }
}
