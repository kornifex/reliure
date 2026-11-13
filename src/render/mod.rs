//! Renders an Obsidian note body to an XHTML fragment.
//!
//! The body is parsed once into pulldown-cmark events, then goes through a pipeline of passes,
//! each a function from events to events in its own module, before being written as XHTML.
mod callouts;
mod embeds;
mod headings;
mod links;
mod tables;

use std::path::Path;

use anyhow::Result;
use pulldown_cmark::{Event, Options, Parser, html};
use pulldown_cmark_escape::escape_html;

use crate::lang::Lang;

pub use links::BoundNotes;

/// Renders an Obsidian note body, the text of the note at `page` in the vault, to an XHTML
/// fragment.
///
/// Each heading gets an id derived from its text. `![[Note]]` puts the body of a note of the
/// vault in place, `![[Note#Heading]]` the section under that heading; the note at `page`
/// stays a reference, as do the notes embedding the embed. `notes` gives where a
/// `[[link]]` to a note of the book leads, and `[[#Heading]]` leads to a heading of the body
/// itself; a link to a note outside the book keeps its text and loses its link. Callouts
/// become framed blocks headed by their title, or by their kind's name in `lang`; untitled
/// quotes stay plain blockquotes. Tables of 5 columns or more become one block per row.
///
/// Fails when an embed matches several notes of the vault.
pub fn to_xhtml(markdown: &str, page: &Path, lang: Lang, notes: &BoundNotes) -> Result<String> {
    let headings = headings::headings(markdown);
    let events: Vec<Event> = parse(markdown).collect();
    // First, so that only the body's own headings get an id: the headings of the notes the
    // embeds pass puts in place right after it come from other bodies.
    let events = headings::add_ids(events, &headings);
    let events = embeds::rewrite(events, page, notes.vault())?;
    let events = links::rewrite(events, notes, &headings);
    let events = callouts::rewrite(events, lang);
    let events = tables::rewrite(events);
    let mut xhtml = String::new();
    html::push_html(&mut xhtml, events.into_iter());
    Ok(xhtml)
}

/// `text` escaped for XHTML, as text or as an attribute value.
pub fn escaped(text: &str) -> String {
    let mut xhtml = String::new();
    escape_html(&mut xhtml, text).expect("writing into a String cannot fail");
    xhtml
}

/// The parser of a note body, with the Markdown extensions Obsidian notes use.
fn parse(markdown: &str) -> Parser<'_> {
    let options = Options::ENABLE_WIKILINKS
        | Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    Parser::new_ext(markdown, options)
}
