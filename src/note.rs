use std::ops::Range;
use std::path::PathBuf;

use anyhow::{Context, Result};
use pulldown_cmark::{Event, Parser, Tag};
use serde_yaml_ng::{Mapping, Value};

/// A note of the vault, its name being the file name without `.md`, as `[[links]]` write it.
pub struct Note {
    pub name: String,
    /// The note's file, relative to the vault's root (`Folder/Note.md`): several files may
    /// share a name.
    pub path: PathBuf,
    pub properties: Mapping,
    pub body: String,
}

impl Note {
    /// Whether the checkbox property `property` is checked.
    pub fn is_checked(&self, property: &str) -> bool {
        matches!(self.properties.get(property), Some(Value::Bool(true)))
    }

    /// A note without properties at `path` in the vault, named after its file.
    #[cfg(test)]
    pub fn at(path: &str, body: &str) -> Note {
        let path = PathBuf::from(path);
        Note {
            name: path.file_stem().unwrap().to_string_lossy().into_owned(),
            path,
            properties: Mapping::new(),
            body: body.into(),
        }
    }
}

/// Parses a note's text into its properties and its body.
///
/// Fails when the YAML frontmatter is invalid, naming the note's file.
pub fn parse(name: String, path: PathBuf, text: &str) -> Result<Note> {
    let (frontmatter, body) = split_frontmatter(text);
    let properties = match frontmatter {
        Some(yaml) if !yaml.trim().is_empty() => serde_yaml_ng::from_str(yaml)
            .with_context(|| format!("invalid properties in {}", path.display()))?,
        _ => Mapping::new(),
    };
    Ok(Note {
        name,
        path,
        properties,
        body: strip_comments(body),
    })
}

/// Removes the Obsidian comments, `%%…%%`, from a body.
///
/// `%%` inside inline code or a code block is text. An unclosed `%%` hides the rest of the
/// body, as in Obsidian.
fn strip_comments(body: &str) -> String {
    let code = code_ranges(body);
    let mut marks = body
        .match_indices("%%")
        .map(|(start, _)| start)
        .filter(|start| !code.iter().any(|range| range.contains(start)));
    let mut stripped = String::with_capacity(body.len());
    let mut kept_from = 0;
    while let Some(open) = marks.next() {
        stripped.push_str(&body[kept_from..open]);
        kept_from = marks.next().map_or(body.len(), |close| close + "%%".len());
    }
    stripped.push_str(&body[kept_from..]);
    stripped
}

/// The byte ranges of a body's inline code and code blocks, delimiters included.
fn code_ranges(body: &str) -> Vec<Range<usize>> {
    Parser::new(body)
        .into_offset_iter()
        .filter_map(|(event, range)| match event {
            Event::Code(_) | Event::Start(Tag::CodeBlock(_)) => Some(range),
            _ => None,
        })
        .collect()
}

/// Splits the YAML frontmatter, between two `---` lines at the top, from the body.
fn split_frontmatter(text: &str) -> (Option<&str>, &str) {
    let Some(rest) = text.strip_prefix("---\n") else {
        return (None, text);
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return (Some(&rest[..offset]), &rest[offset + line.len()..]);
        }
        offset += line.len();
    }
    (None, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_checkbox_and_strips_the_frontmatter() {
        let note = parse(
            "Hub".into(),
            "Hub.md".into(),
            "---\ntype: hub\nbook: true\ndraft: false\n---\nBody\n",
        )
        .unwrap();
        assert!(note.is_checked("book"));
        assert!(!note.is_checked("draft"));
        assert!(!note.is_checked("type"));
        assert_eq!(note.body, "Body\n");
    }

    #[test]
    fn a_note_without_frontmatter_keeps_its_whole_text() {
        let note = parse("Note".into(), "Note.md".into(), "# Title\n---\ntext\n").unwrap();
        assert!(!note.is_checked("book"));
        assert_eq!(note.body, "# Title\n---\ntext\n");
    }

    #[test]
    fn an_empty_frontmatter_is_no_properties() {
        let note = parse("Note".into(), "Note.md".into(), "---\n---\nBody").unwrap();
        assert!(note.properties.is_empty());
        assert_eq!(note.body, "Body");
    }

    #[test]
    fn an_inline_comment_is_removed() {
        let note = parse("Note".into(), "Note.md".into(), "a %%hidden%% b\n").unwrap();
        assert_eq!(note.body, "a  b\n");
    }

    #[test]
    fn a_comment_spanning_lines_is_removed() {
        let note = parse(
            "Note".into(),
            "Note.md".into(),
            "---\nbook: true\n---\nBefore\n%%\nhidden\n- [[A]]\n%%\nAfter\n",
        )
        .unwrap();
        assert!(note.is_checked("book"));
        assert_eq!(note.body, "Before\n\nAfter\n");
    }

    #[test]
    fn percent_signs_in_code_are_kept() {
        let text = "Type `%%x%%` here.\n\n```\n%% kept %%\n```\n";
        assert_eq!(
            parse("Note".into(), "Note.md".into(), text).unwrap().body,
            text
        );
    }

    #[test]
    fn an_unclosed_comment_hides_the_rest_of_the_note() {
        let note = parse(
            "Note".into(),
            "Note.md".into(),
            "Shown %% hidden\n\n`code`\nstill hidden\n",
        )
        .unwrap();
        assert_eq!(note.body, "Shown ");
    }
}
