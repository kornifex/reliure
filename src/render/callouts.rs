//! The callouts pass: Obsidian callouts become framed blocks headed by their title, or by their
//! kind's name in the book's language; untitled quote callouts stay plain blockquotes.
use pulldown_cmark::{Event, Tag, TagEnd};
use pulldown_cmark_escape::escape_html;

use crate::lang::Lang;

/// The header of an Obsidian callout, the first line of its blockquote: `[!warning] Pièges`.
#[derive(Debug, PartialEq)]
struct Callout {
    /// Lowercased: Obsidian ignores the case of `[!TIP]`.
    kind: String,
    title: Option<String>,
}

/// Reads a callout header: `[!kind]`, an optional fold marker (`+` or `-`), then an optional title.
fn parse_callout(line: &str) -> Option<Callout> {
    let (kind, after) = line.trim_start().strip_prefix("[!")?.split_once(']')?;
    if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let title = after.trim_start_matches(['+', '-']).trim();
    Some(Callout {
        kind: kind.to_ascii_lowercase(),
        title: (!title.is_empty()).then(|| title.to_string()),
    })
}

/// The heading of a callout without a title: like Obsidian, its kind capitalized
/// (`[!tip]` → "Tip"), translated when `lang` has a name for it.
fn default_title(kind: &str, lang: Lang) -> String {
    let translated = match lang {
        Lang::En => None,
        Lang::Fr => french_title(kind),
    };
    translated.map_or_else(|| capitalize(kind), str::to_string)
}

fn french_title(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "abstract" | "summary" | "tldr" => "En bref",
        "note" => "Note",
        "info" => "Info",
        "tip" | "hint" => "Astuce",
        "important" => "Important",
        "warning" | "caution" | "attention" => "Attention",
        "danger" | "error" => "Danger",
        "example" => "Exemple",
        "question" | "help" | "faq" => "Question",
        "success" | "check" | "done" => "Réussi",
        "failure" | "fail" | "missing" => "Échec",
        "todo" => "À faire",
        "bug" => "Bug",
        _ => return None,
    })
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// Turns the blockquotes that open with a callout header into `div.callout` blocks with their
/// title, and strips the header off untitled quote callouts, which stay blockquotes.
pub(super) fn rewrite(events: Vec<Event<'_>>, lang: Lang) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(events.len());
    // One entry per open blockquote: whether it closes as a callout div.
    let mut open_quotes: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        match &events[i] {
            Event::Start(Tag::BlockQuote(_)) => {
                let Some(header) = read_header(&events[i + 1..]) else {
                    open_quotes.push(false);
                    out.push(events[i].clone());
                    i += 1;
                    continue;
                };
                let Callout { kind, title } = header.callout;
                let stays_quote = matches!(kind.as_str(), "quote" | "cite") && title.is_none();
                if stays_quote {
                    out.push(events[i].clone());
                } else {
                    let title = title.unwrap_or_else(|| default_title(&kind, lang));
                    out.push(Event::Html(callout_opening(&kind, &title).into()));
                }
                open_quotes.push(!stays_quote);
                if header.paragraph_continues {
                    out.push(Event::Start(Tag::Paragraph));
                }
                i += 1 + header.consumed;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                if open_quotes.pop() == Some(true) {
                    out.push(Event::Html("</div>\n".into()));
                } else {
                    out.push(events[i].clone());
                }
                i += 1;
            }
            _ => {
                out.push(events[i].clone());
                i += 1;
            }
        }
    }
    out
}

struct Header {
    callout: Callout,
    /// Events making up the header, from the paragraph's start to the line break.
    consumed: usize,
    /// The header line is followed by more text in the same paragraph.
    paragraph_continues: bool,
}

/// Reads a callout header from the events following a blockquote's start.
///
/// pulldown-cmark splits a line into several Text events (at `[`, `!`, `]`), so the header is
/// put back together up to the first line break.
fn read_header(events: &[Event<'_>]) -> Option<Header> {
    let [Event::Start(Tag::Paragraph), rest @ ..] = events else {
        return None;
    };
    let mut line = String::new();
    for (n, event) in rest.iter().enumerate() {
        match event {
            Event::Text(text) => line.push_str(text),
            Event::SoftBreak | Event::HardBreak | Event::End(TagEnd::Paragraph) => {
                return parse_callout(&line).map(|callout| Header {
                    callout,
                    consumed: n + 2,
                    paragraph_continues: !matches!(event, Event::End(TagEnd::Paragraph)),
                });
            }
            _ => return None,
        }
    }
    None
}

fn callout_opening(kind: &str, title: &str) -> String {
    let mut title_xhtml = String::new();
    // Writing into a String cannot fail.
    escape_html(&mut title_xhtml, title).unwrap();
    format!(
        "<div class=\"callout callout-{kind}\">\n<p class=\"callout-title\"><strong>{title_xhtml}</strong></p>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{BoundNotes, to_xhtml};
    use crate::vault::Vault;

    #[test]
    fn reads_a_callout_header() {
        assert_eq!(
            parse_callout("[!WARNING]- Pièges "),
            Some(Callout {
                kind: "warning".into(),
                title: Some("Pièges".into())
            })
        );
        assert_eq!(
            parse_callout("[!quote]"),
            Some(Callout {
                kind: "quote".into(),
                title: None
            })
        );
        assert_eq!(parse_callout("[link] text"), None);
        assert_eq!(parse_callout("[!two words]"), None);
    }

    #[test]
    fn a_callout_becomes_a_titled_block() {
        assert_eq!(
            to_xhtml(
                "> [!example] Le loup-garou\n> Trois façons.\n",
                Lang::Fr,
                &BoundNotes::new(&Vault::default())
            ),
            "<div class=\"callout callout-example\">\n<p class=\"callout-title\"><strong>Le loup-garou</strong></p>\n<p>Trois façons.</p>\n</div>\n"
        );
    }

    #[test]
    fn a_callout_without_title_takes_its_kind_heading() {
        assert_eq!(
            to_xhtml(
                "> [!WARNING]\n> - point\n",
                Lang::En,
                &BoundNotes::new(&Vault::default())
            ),
            "<div class=\"callout callout-warning\">\n<p class=\"callout-title\"><strong>Warning</strong></p>\n<ul>\n<li>point</li>\n</ul>\n</div>\n"
        );
    }

    #[test]
    fn the_kind_heading_is_translated_when_known() {
        assert_eq!(default_title("warning", Lang::Fr), "Attention");
        assert_eq!(default_title("custom-kind", Lang::Fr), "Custom-kind");
    }

    #[test]
    fn a_quote_callout_stays_a_blockquote() {
        assert_eq!(
            to_xhtml(
                "> [!quote]\n> \"Three clues.\"\n",
                Lang::En,
                &BoundNotes::new(&Vault::default())
            ),
            "<blockquote>\n<p>\"Three clues.\"</p>\n</blockquote>\n"
        );
    }

    #[test]
    fn a_titled_quote_callout_closes_as_a_block() {
        assert_eq!(
            to_xhtml(
                "> [!quote] Alexander\n> Text.\n",
                Lang::En,
                &BoundNotes::new(&Vault::default())
            ),
            "<div class=\"callout callout-quote\">\n<p class=\"callout-title\"><strong>Alexander</strong></p>\n<p>Text.</p>\n</div>\n"
        );
    }

    #[test]
    fn a_plain_blockquote_is_left_alone() {
        assert_eq!(
            to_xhtml(
                "> Just a quote.\n",
                Lang::En,
                &BoundNotes::new(&Vault::default())
            ),
            "<blockquote>\n<p>Just a quote.</p>\n</blockquote>\n"
        );
    }
}
