use anyhow::{Context, Result};
use serde::Deserialize;

/// A note of the vault, its name being the file name without `.md`, as `[[links]]` write it.
pub struct Note {
    pub name: String,
    pub properties: Properties,
    pub body: String,
}

/// The note properties reliure reads; the others are ignored.
#[derive(Deserialize, Default)]
pub struct Properties {
    /// Marks a hub to be bound into a book.
    #[serde(default)]
    pub livre: bool,
}

/// Parses a note's text into its properties and its body.
///
/// Fails when the YAML frontmatter is invalid, naming the note.
pub fn parse(name: String, text: &str) -> Result<Note> {
    let (frontmatter, body) = split_frontmatter(text);
    let properties = match frontmatter {
        Some(yaml) if !yaml.trim().is_empty() => serde_yaml_ng::from_str(yaml)
            .with_context(|| format!("invalid properties in note {name}"))?,
        _ => Properties::default(),
    };
    Ok(Note {
        name,
        properties,
        body: body.to_string(),
    })
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
    fn reads_livre_and_strips_the_frontmatter() {
        let note = parse("Hub".into(), "---\ntype: hub\nlivre: true\n---\nBody\n").unwrap();
        assert!(note.properties.livre);
        assert_eq!(note.body, "Body\n");
    }

    #[test]
    fn a_note_without_frontmatter_keeps_its_whole_text() {
        let note = parse("Note".into(), "# Title\n---\ntext\n").unwrap();
        assert!(!note.properties.livre);
        assert_eq!(note.body, "# Title\n---\ntext\n");
    }

    #[test]
    fn an_empty_frontmatter_is_no_properties() {
        let note = parse("Note".into(), "---\n---\nBody").unwrap();
        assert!(!note.properties.livre);
        assert_eq!(note.body, "Body");
    }
}
