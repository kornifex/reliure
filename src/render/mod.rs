//! Renders an Obsidian note body to an XHTML fragment.
//!
//! The body is parsed once into pulldown-cmark events, then goes through a pipeline of passes,
//! each a function from events to events in its own module, before being written as XHTML.
mod callouts;
mod headings;
mod links;
mod tables;

use pulldown_cmark::{Event, Options, Parser, html};

use crate::lang::Lang;

pub use links::BoundNotes;

/// Renders an Obsidian note body to an XHTML fragment.
///
/// Each heading gets an id derived from its text. `notes` gives where a `[[link]]` to a note
/// of the book leads, and `[[#Heading]]` leads to a heading of the body itself; a link to a
/// note outside the book keeps its text and loses its link. Callouts become framed blocks
/// headed by their title, or by their kind's name in `lang`; untitled quotes stay plain
/// blockquotes. Tables of 5 columns or more become one block per row.
pub fn to_xhtml(markdown: &str, lang: Lang, notes: &BoundNotes) -> String {
    let headings = headings::headings(markdown);
    let events: Vec<Event> = parse(markdown).collect();
    // First, so that only the body's own headings get an id: the headings of the notes the
    // embeds pass will put in place right after it come from other bodies.
    let events = headings::add_ids(events, &headings);
    let events = links::rewrite(events, notes, &headings);
    let events = callouts::rewrite(events, lang);
    let events = tables::rewrite(events);
    let mut xhtml = String::new();
    html::push_html(&mut xhtml, events.into_iter());
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
