//! The headings pass: each heading of a page gets an id derived from its text, unique within
//! the page, so that links can lead to it.
use std::ops::Range;

use pulldown_cmark::{CowStr, Event, HeadingLevel, Tag, TagEnd};

/// A heading of a note body.
pub(super) struct Heading {
    #[allow(dead_code, reason = "read by section embeds, to find where a section ends")]
    pub(super) level: HeadingLevel,
    /// Its text, without its markup.
    pub(super) text: String,
    /// Its id in the page: its text lowercased, words joined by `-`, unique within the body.
    pub(super) id: String,
    /// Where it is written in the body, from its `#` to the end of its line.
    #[allow(dead_code, reason = "read by section embeds, to cut a section out of the body")]
    pub(super) range: Range<usize>,
}

/// The headings of a note body, in order.
///
/// Ids only depend on the body, so a page and the links to it agree on them.
pub(super) fn headings(markdown: &str) -> Vec<Heading> {
    let mut headings: Vec<Heading> = Vec::new();
    // The heading being read: its level, its range, and its text so far.
    let mut open: Option<(HeadingLevel, Range<usize>, String)> = None;
    for (event, range) in super::parse(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => open = Some((level, range, String::new())),
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, _, heading_text)) = open.as_mut() {
                    heading_text.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                // Set by the matching Start event.
                let (level, range, text) = open.take().unwrap();
                let id = unique_id(slug(&text), &headings);
                headings.push(Heading { level, text, id, range });
            }
            _ => {}
        }
    }
    headings
}

/// The heading a link's `#text` leads to: the first one whose text gives the same id, so that
/// the link may differ from it in case or punctuation.
pub(super) fn find<'h>(headings: &'h [Heading], text: &str) -> Option<&'h Heading> {
    let wanted = slug(text);
    headings.iter().find(|heading| slug(&heading.text) == wanted)
}

/// The headings pass: puts the ids of `headings`, read from the same body, on the headings of
/// `events`, in order.
pub(super) fn add_ids<'a>(events: Vec<Event<'a>>, headings: &'a [Heading]) -> Vec<Event<'a>> {
    let mut ids = headings.iter().map(|heading| CowStr::from(heading.id.as_str()));
    events
        .into_iter()
        .map(|event| match event {
            Event::Start(Tag::Heading { level, id: _, classes, attrs }) => {
                Event::Start(Tag::Heading { level, id: ids.next(), classes, attrs })
            }
            other => other,
        })
        .collect()
}

/// The id a heading's text gives before it is made unique: lowercased, words joined by `-`,
/// signs left out. It starts with a letter, as XML ids must, or is prefixed with `h-`.
fn slug(text: &str) -> String {
    let words: Vec<String> = text
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c == '-')
        .map(|word| word.chars().filter(|&c| is_id_char(c)).collect())
        .filter(|word: &String| !word.is_empty())
        .collect();
    let slug = words.join("-");
    match slug.chars().next() {
        Some(first) if first.is_alphabetic() => slug,
        Some(_) => format!("h-{slug}"),
        None => "h".to_string(),
    }
}

/// Letters, digits and `_`, leaving out the Latin-1 signs XML names refuse (`²`, `º`).
fn is_id_char(c: char) -> bool {
    c == '_' || (c.is_alphanumeric() && (c.is_ascii() || c >= 'À'))
}

/// `slug`, or `slug` followed by the first `-N` that no heading before took.
fn unique_id(slug: String, before: &[Heading]) -> String {
    let taken = |id: &str| before.iter().any(|heading| heading.id == id);
    if !taken(&slug) {
        return slug;
    }
    // Only a finite number of ids are taken.
    (1..).map(|n| format!("{slug}-{n}")).find(|id| !taken(id)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;
    use crate::render::{BoundNotes, to_xhtml};
    use crate::vault::Vault;

    fn render(markdown: &str) -> String {
        to_xhtml(markdown, Lang::Fr, &BoundNotes::new(&Vault::default()))
    }

    #[test]
    fn a_heading_gets_an_id_from_its_text() {
        assert_eq!(
            render("# Méthode de préparation\n\n## Le *loup-garou* : `trois` pistes\n"),
            "<h1 id=\"méthode-de-préparation\">Méthode de préparation</h1>\n<h2 id=\"le-loup-garou-trois-pistes\">Le <em>loup-garou</em> : <code>trois</code> pistes</h2>\n"
        );
    }

    #[test]
    fn ids_are_unique_within_the_page() {
        assert_eq!(
            render("## Notes\n## Notes\n## Notes 1\n"),
            "<h2 id=\"notes\">Notes</h2>\n<h2 id=\"notes-1\">Notes</h2>\n<h2 id=\"notes-1-1\">Notes 1</h2>\n"
        );
    }

    #[test]
    fn an_id_starts_with_a_letter() {
        let ids: Vec<String> = headings("## 2024, l'année\n## ?!\n").into_iter().map(|heading| heading.id).collect();
        assert_eq!(ids, ["h-2024-lannée", "h"]);
    }

    #[test]
    fn a_heading_has_its_level_and_place_in_the_body() {
        let body = "Intro\n\n## Part\nText\n### Sub\n";
        let found: Vec<_> = headings(body).into_iter().map(|heading| (heading.level, &body[heading.range])).collect();
        assert_eq!(found, [(HeadingLevel::H2, "## Part\n"), (HeadingLevel::H3, "### Sub\n")]);
    }

    #[test]
    fn a_link_finds_a_heading_whatever_its_case() {
        let headings = headings("## Méthode de préparation\n");
        assert_eq!(find(&headings, "méthode de Préparation").map(|heading| heading.id.as_str()), Some("méthode-de-préparation"));
        assert!(find(&headings, "Méthode").is_none());
    }
}
