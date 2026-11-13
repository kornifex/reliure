//! The `[[links]]` pass: a link to a note of the book leads to its page, a link to a note
//! outside the book keeps its text and loses its link.
use pulldown_cmark::{CowStr, Event, LinkType, Tag, TagEnd};

use crate::book::note_name;

/// Rewrites the wikilinks of `events`, `href_of` giving a page from a note's name.
pub(super) fn rewrite<'a>(events: Vec<Event<'a>>, href_of: impl Fn(&str) -> Option<String>) -> Vec<Event<'a>> {
    // Links don't nest, so one flag pairs a dropped link's End with its Start.
    let mut dropping_link = false;
    events
        .into_iter()
        .filter_map(|event| match event {
            Event::Start(Tag::Link { link_type: LinkType::WikiLink { .. }, dest_url, .. }) => {
                match href_of(note_name(&dest_url)) {
                    Some(href) => Some(Event::Start(Tag::Link {
                        link_type: LinkType::Inline,
                        dest_url: CowStr::from(href),
                        title: CowStr::from(""),
                        id: CowStr::from(""),
                    })),
                    None => {
                        dropping_link = true;
                        None
                    }
                }
            }
            Event::End(TagEnd::Link) if dropping_link => {
                dropping_link = false;
                None
            }
            other => Some(other),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::lang::Lang;
    use crate::render::to_xhtml;

    fn href_of(name: &str) -> Option<String> {
        (name == "In book").then(|| "note-1.xhtml".to_string())
    }

    #[test]
    fn links_inside_the_book_point_to_their_page() {
        assert_eq!(
            to_xhtml("See [[In book|this note]] and [[In book#Part]].", Lang::En, href_of),
            "<p>See <a href=\"note-1.xhtml\">this note</a> and <a href=\"note-1.xhtml\">In book#Part</a>.</p>\n"
        );
    }

    #[test]
    fn links_outside_the_book_keep_their_text() {
        assert_eq!(to_xhtml("See [[Elsewhere]].", Lang::En, href_of), "<p>See Elsewhere.</p>\n");
    }
}
