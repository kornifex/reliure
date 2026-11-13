//! Renders an Obsidian note body to an XHTML fragment.
//!
//! The body is parsed once into pulldown-cmark events, then goes through a pipeline of passes,
//! each a function from events to events in its own module, before being written as XHTML.
mod callouts;
mod links;
mod tables;

use pulldown_cmark::{Event, Options, Parser, html};

use crate::lang::Lang;

/// Renders an Obsidian note body to an XHTML fragment.
///
/// `href_of` gives the target of a `[[link]]` inside the book, from the linked note's name;
/// a link to a note outside the book keeps its text and loses its link. Callouts become
/// framed blocks headed by their title, or by their kind's name in `lang`; untitled quotes
/// stay plain blockquotes. Tables of 5 columns or more become one block per row.
pub fn to_xhtml(markdown: &str, lang: Lang, href_of: impl Fn(&str) -> Option<String>) -> String {
    let options = Options::ENABLE_WIKILINKS
        | Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let events: Vec<Event> = Parser::new_ext(markdown, options).collect();
    let events = links::rewrite(events, href_of);
    let events = callouts::rewrite(events, lang);
    let events = tables::rewrite(events);
    let mut xhtml = String::new();
    html::push_html(&mut xhtml, events.into_iter());
    xhtml
}
