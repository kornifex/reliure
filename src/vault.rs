use std::fs;
use std::iter;
use std::path::Path;

use anyhow::{Context, Result, bail};
use walkdir::{DirEntry, WalkDir};

use crate::note::{self, Note};

/// Every note of an Obsidian vault, found as links find them.
///
/// A link names a note (`[[Note]]`), or the end of its path when the name is shared
/// (`[[Folder/Note]]`), whatever the case. Nothing stops two folders from holding files of
/// the same name: the vault keeps them all, and a lookup tells when a link is ambiguous.
#[derive(Default)]
pub struct Vault {
    /// Sorted by key, so that the notes a link matches sit side by side.
    notes: Vec<Note>,
    /// The key of each note, at the same index.
    keys: Vec<Key>,
}

/// A path read backwards, lowercased, as links match it: `Folder/Note.md` is `["note", "folder"]`.
/// A link matches the notes whose key starts with its own.
type Key = Vec<String>;

/// What a link target designates in the vault.
///
/// Every caller looking a target up has to decide what an ambiguous one means for it.
pub enum Lookup<'a> {
    /// The one note it matches.
    Found(&'a Note),
    /// No note matches it.
    Missing,
    /// Several notes match it.
    Ambiguous(&'a [Note]),
}

impl Vault {
    /// Reads every Markdown note under `root`, skipping hidden folders (`.obsidian`, `.git`, `.trash`).
    pub fn load(root: &Path) -> Result<Vault> {
        let mut notes = Vec::new();
        let entries = WalkDir::new(root)
            .into_iter()
            .filter_entry(|entry| entry.depth() == 0 || !is_hidden(entry));
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if !entry.file_type().is_file() || path.extension().is_none_or(|ext| ext != "md") {
                continue;
            }
            let text = fs::read_to_string(path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            // A .md path always has a file stem, and the walk only yields paths under `root`.
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            notes.push(note::parse(name, relative, &text)?);
        }
        Ok(Vault::from_notes(notes))
    }

    /// Indexes `notes` as links find them.
    pub fn from_notes(notes: impl IntoIterator<Item = Note>) -> Vault {
        let mut keyed: Vec<(Key, Note)> = notes
            .into_iter()
            .map(|note| (note_key(&note), note))
            .collect();
        keyed.sort_by(|(a, _), (b, _)| a.cmp(b));
        let (keys, notes) = keyed.into_iter().unzip();
        Vault { notes, keys }
    }

    /// The notes a link target matches: `Note`, or `Folder/Note` for the notes whose path ends
    /// with these folders, whatever the case. Among several, the one at that very path from
    /// the vault's root wins, as Obsidian writes such paths when a name is shared.
    pub fn get(&self, target: &str) -> Lookup<'_> {
        let key = target_key(target);
        let start = self.keys.partition_point(|other| *other < key);
        let len = self.keys[start..].partition_point(|other| other.starts_with(&key));
        let (notes, keys) = (
            &self.notes[start..start + len],
            &self.keys[start..start + len],
        );
        match notes {
            [] => Lookup::Missing,
            [note] => Lookup::Found(note),
            _ => {
                let mut from_root = notes
                    .iter()
                    .zip(keys)
                    .filter(|(_, other)| other.len() == key.len());
                match (from_root.next(), from_root.next()) {
                    (Some((note, _)), None) => Lookup::Found(note),
                    _ => Lookup::Ambiguous(notes),
                }
            }
        }
    }

    /// The hubs whose checkbox `property` is checked, sorted by name.
    ///
    /// Fails when two of them share a name, whatever the case, as their books would be
    /// written to the same file.
    pub fn books(&self, property: &str) -> Result<Vec<&Note>> {
        // Already sorted by name, as keys start with it.
        let hubs: Vec<&Note> = self
            .notes
            .iter()
            .filter(|note| note.is_checked(property))
            .collect();
        let same_name = |a: &&Note, b: &&Note| a.name.to_lowercase() == b.name.to_lowercase();
        if let Some(namesakes) = hubs.chunk_by(same_name).find(|hubs| hubs.len() > 1) {
            bail!(
                "several books are named {}: {}",
                namesakes[0].name,
                display_paths(namesakes.iter().copied())
            );
        }
        Ok(hubs)
    }
}

fn note_key(note: &Note) -> Key {
    let folders = note
        .path
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .rev();
    iter::once(note.name.to_lowercase())
        .chain(folders.map(|folder| folder.as_os_str().to_string_lossy().to_lowercase()))
        .collect()
}

fn target_key(target: &str) -> Key {
    target.rsplit('/').map(str::to_lowercase).collect()
}

/// Lists the paths of `notes` for an error message, sorted so that it doesn't depend on the
/// order folders are read in: `a/Note.md, b/Note.md`.
pub fn display_paths<'a>(notes: impl IntoIterator<Item = &'a Note>) -> String {
    let mut paths: Vec<String> = notes
        .into_iter()
        .map(|note| note.path.display().to_string())
        .collect();
    paths.sort();
    paths.join(", ")
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry.file_name().to_string_lossy().starts_with('.')
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    const BOOK: &str = "---\nbook: true\n---\n";

    /// A vault on disk holding `files`, as (path under the vault, text) pairs.
    fn vault_with(files: &[(&str, &str)]) -> (TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let vault = Vault::load(dir.path()).unwrap();
        (dir, vault)
    }

    /// The paths of the notes `target` designates, sorted: none when it is missing, one when
    /// it is found, several when it is ambiguous.
    fn lookup(vault: &Vault, target: &str) -> Vec<String> {
        let notes = match vault.get(target) {
            Lookup::Found(note) => std::slice::from_ref(note),
            Lookup::Missing => &[],
            Lookup::Ambiguous(notes) => notes,
        };
        let mut paths: Vec<String> = notes
            .iter()
            .map(|note| note.path.display().to_string())
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn a_name_held_by_several_files_is_ambiguous() {
        let (_dir, vault) = vault_with(&[("b/Note.md", ""), ("a/Note.md", ""), ("Other.md", "")]);
        assert_eq!(lookup(&vault, "Note"), ["a/Note.md", "b/Note.md"]);
        assert_eq!(lookup(&vault, "Other"), ["Other.md"]);
        assert!(matches!(vault.get("Other"), Lookup::Found(_)));
        assert!(matches!(vault.get("Missing"), Lookup::Missing));
    }

    #[test]
    fn names_are_found_whatever_their_case() {
        let (_dir, vault) = vault_with(&[("Note.md", ""), ("a/Twin.md", ""), ("b/twin.md", "")]);
        assert_eq!(lookup(&vault, "nOTE"), ["Note.md"]);
        assert_eq!(lookup(&vault, "TWIN"), ["a/Twin.md", "b/twin.md"]);
    }

    #[test]
    fn a_path_link_finds_the_notes_whose_path_ends_with_it() {
        let (_dir, vault) = vault_with(&[
            ("a/Note.md", ""),
            ("b/Note.md", ""),
            ("x/a/Note.md", ""),
            ("folder/Note.md", ""),
        ]);
        assert_eq!(lookup(&vault, "b/Note"), ["b/Note.md"]);
        assert_eq!(lookup(&vault, "X/A/note"), ["x/a/Note.md"]);
        assert_eq!(lookup(&vault, "a/Note"), ["a/Note.md"]);
        assert_eq!(
            lookup(&vault, "Note"),
            ["a/Note.md", "b/Note.md", "folder/Note.md", "x/a/Note.md"]
        );
        // Paths match on whole folder names.
        assert_eq!(lookup(&vault, "older/Note"), [] as [&str; 0]);
        assert_eq!(lookup(&vault, "c/Note"), [] as [&str; 0]);
    }

    #[test]
    fn the_note_at_a_path_from_the_root_wins_over_deeper_ones() {
        let (_dir, vault) = vault_with(&[("Note.md", ""), ("a/Note.md", ""), ("x/a/Note.md", "")]);
        assert_eq!(lookup(&vault, "Note"), ["Note.md"]);
        assert_eq!(lookup(&vault, "a/Note"), ["a/Note.md"]);
        assert_eq!(lookup(&vault, "x/a/Note"), ["x/a/Note.md"]);
    }

    #[test]
    fn two_books_sharing_a_name_fail_naming_their_paths() {
        let (_dir, vault) =
            vault_with(&[("b/Hub.md", BOOK), ("a/Hub.md", BOOK), ("Other.md", BOOK)]);
        let Err(error) = vault.books("book") else {
            panic!("two books are named Hub");
        };
        assert_eq!(
            error.to_string(),
            "several books are named Hub: a/Hub.md, b/Hub.md"
        );
    }

    #[test]
    fn two_books_whose_names_differ_in_case_fail() {
        let (_dir, vault) = vault_with(&[("Hub.md", BOOK), ("a/hub.md", BOOK)]);
        assert!(vault.books("book").is_err());
    }

    #[test]
    fn a_book_may_share_its_name_with_a_note() {
        let (_dir, vault) = vault_with(&[("a/Hub.md", BOOK), ("b/Hub.md", "")]);
        let hubs = vault.books("book").unwrap();
        assert_eq!(hubs.len(), 1);
    }
}
