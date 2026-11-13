use std::fs;
use std::iter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use walkdir::{DirEntry, WalkDir};

use crate::note::{self, Note};

/// Every note and attachment of an Obsidian vault, found as links find them.
///
/// A link names a file (`[[Note]]`, `![[photo.png]]`), or the end of its path when the name is
/// shared (`[[Folder/Note]]`), whatever the case. Nothing stops two folders from holding files
/// of the same name: the vault keeps them all, and a lookup tells when a link is ambiguous.
#[derive(Default)]
pub struct Vault {
    /// The vault's folder, which attachments are read from when a book needs them.
    root: PathBuf,
    notes: Index<Note>,
    attachments: Index<Attachment>,
}

/// A file of the vault that is not a note, such as an image, named after its file with its
/// extension, as `![[photo.png]]` writes it. It is read only when a book holds it.
pub struct Attachment {
    pub name: String,
    /// The attachment's file, relative to the vault's root (`Folder/photo.png`): several files
    /// may share a name.
    pub path: PathBuf,
}

impl Attachment {
    /// An attachment at `path` in the vault, named after its file.
    #[cfg(test)]
    pub fn at(path: &str) -> Attachment {
        let path = PathBuf::from(path);
        Attachment {
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            path,
        }
    }
}

/// A file of the vault, as links find it.
pub trait VaultFile {
    /// The name links write: a note's file name without `.md`, an attachment's file name.
    fn name(&self) -> &str;
    /// The file, relative to the vault's root.
    fn path(&self) -> &Path;
}

impl VaultFile for Note {
    fn name(&self) -> &str {
        &self.name
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl VaultFile for Attachment {
    fn name(&self) -> &str {
        &self.name
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

/// Files of one kind, notes or attachments, as links find them.
struct Index<T> {
    /// Sorted by key, so that the files a link matches sit side by side.
    files: Vec<T>,
    /// The key of each file, at the same index.
    keys: Vec<Key>,
}

impl<T> Default for Index<T> {
    fn default() -> Self {
        Index {
            files: Vec::new(),
            keys: Vec::new(),
        }
    }
}

/// A path read backwards, lowercased, as links match it: `Folder/Note.md` is `["note", "folder"]`,
/// `Folder/photo.png` is `["photo.png", "folder"]`. A link matches the files whose key starts
/// with its own.
type Key = Vec<String>;

/// What a link target designates among the notes, or among the attachments, of the vault.
///
/// Every caller looking a target up has to decide what an ambiguous one means for it.
pub enum Lookup<'a, T> {
    /// The one file it matches.
    Found(&'a T),
    /// No file matches it.
    Missing,
    /// Several files match it.
    Ambiguous(&'a [T]),
}

impl Vault {
    /// Reads every Markdown note under `root`, and lists the other files as attachments,
    /// skipping hidden folders (`.obsidian`, `.git`, `.trash`).
    pub fn load(root: &Path) -> Result<Vault> {
        let mut notes = Vec::new();
        let mut attachments = Vec::new();
        let entries = WalkDir::new(root)
            .into_iter()
            .filter_entry(|entry| entry.depth() == 0 || !is_hidden(entry));
        for entry in entries {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("the walk only yields paths under `root`")
                .to_path_buf();
            if path.extension().is_none_or(|ext| ext != "md") {
                attachments.push(Attachment {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: relative,
                });
                continue;
            }
            let text = fs::read_to_string(path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            let name = path
                .file_stem()
                .expect("a .md path has a file stem")
                .to_string_lossy()
                .into_owned();
            notes.push(note::parse(name, relative, &text)?);
        }
        Ok(Vault {
            root: root.to_path_buf(),
            notes: Index::new(notes),
            attachments: Index::new(attachments),
        })
    }

    /// Indexes `notes` as links find them, in a vault without attachments.
    #[cfg(test)]
    pub fn from_notes(notes: impl IntoIterator<Item = Note>) -> Vault {
        Vault {
            notes: Index::new(notes),
            ..Vault::default()
        }
    }

    /// The vault, holding `attachments` as well.
    #[cfg(test)]
    pub fn with_attachments(self, attachments: impl IntoIterator<Item = Attachment>) -> Vault {
        Vault {
            attachments: Index::new(attachments),
            ..self
        }
    }

    /// The notes a link target matches: `Note`, or `Folder/Note` for the notes whose path ends
    /// with these folders, whatever the case. Among several, the one at that very path from
    /// the vault's root wins, as Obsidian writes such paths when a name is shared.
    pub fn get(&self, target: &str) -> Lookup<'_, Note> {
        self.notes.get(target)
    }

    /// The attachments an embed's target matches, as `get` finds notes: `photo.png`, or
    /// `Folder/photo.png`, the extension included.
    pub fn attachment(&self, target: &str) -> Lookup<'_, Attachment> {
        self.attachments.get(target)
    }

    /// The content of the file of `attachment`, to copy it into a book.
    ///
    /// Fails when the file can't be read, naming it.
    pub fn read(&self, attachment: &Attachment) -> Result<Vec<u8>> {
        let path = self.root.join(&attachment.path);
        fs::read(&path).with_context(|| format!("cannot read {}", path.display()))
    }

    /// The hubs whose checkbox `property` is checked, sorted by name.
    ///
    /// Fails when two of them share a name, whatever the case, as their books would be
    /// written to the same file.
    pub fn books(&self, property: &str) -> Result<Vec<&Note>> {
        // Already sorted by name, as keys start with it.
        let hubs: Vec<&Note> = self
            .notes
            .files
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

impl<T: VaultFile> Index<T> {
    /// Indexes `files` as links find them.
    fn new(files: impl IntoIterator<Item = T>) -> Self {
        let mut keyed: Vec<(Key, T)> = files.into_iter().map(|file| (key(&file), file)).collect();
        keyed.sort_by(|(a, _), (b, _)| a.cmp(b));
        let (keys, files) = keyed.into_iter().unzip();
        Index { files, keys }
    }

    /// The files a link target matches, the one at that very path from the root winning.
    fn get(&self, target: &str) -> Lookup<'_, T> {
        let key = target_key(target);
        let start = self.keys.partition_point(|other| *other < key);
        let len = self.keys[start..].partition_point(|other| other.starts_with(&key));
        let (files, keys) = (
            &self.files[start..start + len],
            &self.keys[start..start + len],
        );
        match files {
            [] => Lookup::Missing,
            [file] => Lookup::Found(file),
            _ => {
                let mut from_root = files
                    .iter()
                    .zip(keys)
                    .filter(|(_, other)| other.len() == key.len());
                match (from_root.next(), from_root.next()) {
                    (Some((file, _)), None) => Lookup::Found(file),
                    _ => Lookup::Ambiguous(files),
                }
            }
        }
    }
}

fn key(file: &impl VaultFile) -> Key {
    let folders = file
        .path()
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .rev();
    iter::once(file.name().to_lowercase())
        .chain(folders.map(|folder| folder.as_os_str().to_string_lossy().to_lowercase()))
        .collect()
}

fn target_key(target: &str) -> Key {
    target.rsplit('/').map(str::to_lowercase).collect()
}

/// Lists the paths of `files` for an error message, sorted so that it doesn't depend on the
/// order folders are read in: `a/Note.md, b/Note.md`.
pub fn display_paths<'a, T: VaultFile + 'a>(files: impl IntoIterator<Item = &'a T>) -> String {
    let mut paths: Vec<String> = files
        .into_iter()
        .map(|file| file.path().display().to_string())
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

    /// The paths of the files a lookup designates, sorted: none when the target is missing,
    /// one when it is found, several when it is ambiguous.
    fn paths<T: VaultFile>(lookup: &Lookup<'_, T>) -> Vec<String> {
        let files = match *lookup {
            Lookup::Found(file) => std::slice::from_ref(file),
            Lookup::Missing => &[],
            Lookup::Ambiguous(files) => files,
        };
        let mut paths: Vec<String> = files
            .iter()
            .map(|file| file.path().display().to_string())
            .collect();
        paths.sort();
        paths
    }

    /// The paths of the notes `target` designates.
    fn lookup(vault: &Vault, target: &str) -> Vec<String> {
        paths(&vault.get(target))
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
    fn attachments_are_found_by_file_name_with_its_extension_or_by_path() {
        let (_dir, vault) = vault_with(&[
            ("Photo.md", ""),
            ("maps/Photo.PNG", ""),
            ("a/File.pdf", ""),
            ("b/File.pdf", ""),
            (".obsidian/app.json", ""),
        ]);
        let attachments = |target| paths(&vault.attachment(target));
        assert_eq!(attachments("photo.png"), ["maps/Photo.PNG"]);
        assert_eq!(attachments("MAPS/photo.png"), ["maps/Photo.PNG"]);
        assert_eq!(attachments("b/File.pdf"), ["b/File.pdf"]);
        assert_eq!(attachments("file.pdf"), ["a/File.pdf", "b/File.pdf"]);
        // The extension is part of the name, notes are not attachments, and hidden folders
        // are skipped.
        assert!(matches!(vault.attachment("Photo"), Lookup::Missing));
        assert!(matches!(vault.attachment("Photo.md"), Lookup::Missing));
        assert!(matches!(vault.attachment("app.json"), Lookup::Missing));
        // Attachments are not notes.
        assert_eq!(lookup(&vault, "Photo"), ["Photo.md"]);
        assert!(matches!(vault.get("File.pdf"), Lookup::Missing));
    }

    #[test]
    fn an_attachment_is_read_from_the_vault() {
        let (_dir, vault) = vault_with(&[("maps/Photo.png", "pixels")]);
        let Lookup::Found(photo) = vault.attachment("photo.png") else {
            panic!("photo.png is in the vault");
        };
        assert_eq!(vault.read(photo).unwrap(), b"pixels");
    }

    #[test]
    fn an_attachment_that_cannot_be_read_fails_naming_its_file() {
        let (dir, vault) = vault_with(&[("maps/Photo.png", "pixels")]);
        let Lookup::Found(photo) = vault.attachment("photo.png") else {
            panic!("photo.png is in the vault");
        };
        // A folder opens as a file does, but reading it fails.
        let file = dir.path().join("maps/Photo.png");
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        let error = vault.read(photo).unwrap_err();
        assert_eq!(error.to_string(), format!("cannot read {}", file.display()));
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
