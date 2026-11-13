//! The plan of a book, read from its hub.
use std::collections::HashSet;

use pulldown_cmark::{Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};

use crate::note::Note;

pub struct Book {
    pub title: String,
    /// The hub's text before its first `##` heading.
    pub intro: String,
    pub chapters: Vec<Chapter>,
}

/// A `##` section of the hub: its text opens the chapter, then come the notes it lists.
pub struct Chapter {
    pub title: String,
    pub text: String,
    pub notes: Vec<String>,
}

/// Reads the plan of a book from its hub.
///
/// A note enters the book when a list item of a `##` section opens with a link to it
/// (`- **[[Note]]** : summary`), under the first chapter that lists it. Links elsewhere in
/// the text, and links to notes missing from the vault (`in_vault` is false), stay references.
pub fn plan(hub: &Note, in_vault: impl Fn(&str) -> bool) -> Book {
    let (intro, sections) = split_sections(&hub.body);
    let mut bound = HashSet::from([hub.name.clone()]);
    let chapters = sections
        .into_iter()
        .map(|(title, text)| Chapter {
            title,
            notes: listed_notes(text)
                .into_iter()
                .filter(|name| in_vault(name) && bound.insert(name.clone()))
                .collect(),
            text: text.to_string(),
        })
        .collect();
    Book {
        title: hub.name.clone(),
        intro: intro.to_string(),
        chapters,
    }
}

/// The name of the note a link target points to: `Note#Heading` → `Note`.
pub fn note_name(target: &str) -> &str {
    target.split_once('#').map_or(target, |(name, _)| name)
}

/// Splits a body at its `##` headings: the text before the first one, then each heading's
/// title with the text up to the next one.
fn split_sections(body: &str) -> (&str, Vec<(String, &str)>) {
    let mut headings = Vec::new();
    let mut title: Option<String> = None;
    let mut heading_start = 0;
    for (event, range) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level: HeadingLevel::H2, .. }) => {
                heading_start = range.start;
                title = Some(String::new());
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2)) => {
                // Set by the matching Start event.
                headings.push((title.take().unwrap(), heading_start, range.end));
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
            let text_end = headings.get(i + 1).map_or(body.len(), |&(_, start, _)| start);
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
            Event::Start(Tag::Link { link_type: LinkType::WikiLink { .. }, dest_url, .. })
                if at_item_start =>
            {
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
    use super::*;
    use crate::note::Properties;

    fn hub(body: &str) -> Note {
        Note { name: "Hub".into(), properties: Properties::default(), body: body.into() }
    }

    #[test]
    fn sections_become_chapters_listing_their_notes() {
        let book = plan(
            &hub("Intro\n\n## Design\n- **[[A]]** : a\n- [[B|bee]]\n\n## Table\n- **[[C]]** : c\n"),
            |_| true,
        );
        assert_eq!(book.intro, "Intro\n\n");
        let chapters: Vec<_> = book.chapters.iter().map(|c| (c.title.as_str(), c.notes.clone())).collect();
        assert_eq!(chapters, [("Design", vec!["A".to_string(), "B".to_string()]), ("Table", vec!["C".to_string()])]);
        assert_eq!(book.chapters[1].text, "- **[[C]]** : c\n");
    }

    #[test]
    fn a_note_listed_twice_is_bound_once() {
        let book = plan(&hub("## One\n- [[A]]\n## Two\n- [[A]] (see One)\n- [[B]]\n"), |_| true);
        assert_eq!(book.chapters[1].notes, ["B"]);
    }

    #[test]
    fn links_inside_text_and_missing_notes_stay_references() {
        let book = plan(
            &hub("## Words\n- **Clue**: leads to a [[Revelation]].\n- [[Missing]]\n- [[A#Part]]\n"),
            |name| name != "Missing",
        );
        assert_eq!(book.chapters[0].notes, ["A"]);
    }
}
