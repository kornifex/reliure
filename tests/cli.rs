//! Runs the reliure binary on a vault and reads the book it writes.
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;
use zip::ZipArchive;

/// A vault holding `files`, as (path under the vault, text) pairs.
fn vault_with(files: &[(&str, &str)]) -> TempDir {
    let vault = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = vault.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    vault
}

/// A vault with a hub binding two notes, the first one linking to the second.
fn vault() -> TempDir {
    vault_with(&[
        (
            "Hub.md",
            "---\nbook: true\n---\nOpening.\n\n## Design\n- **[[Clues]]**: three of them.\n- [[Pacing]]\n",
        ),
        (
            "notes/Clues.md",
            "See [[Pacing#Scenes]].\n\n> [!tip]\n> Plan three.\n",
        ),
        ("notes/Pacing.md", "## Scenes\nShort ones.\n"),
        // Hidden folders are skipped: this hub would make a second book.
        (".trash/Old hub.md", "---\nbook: true\n---\n"),
    ])
}

/// Runs reliure on `vault`, writing the books into `out`, with more options in `args`.
fn reliure(vault: &Path, out: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reliure"))
        .arg(vault)
        .arg("--out")
        .arg(out)
        .args(args)
        .output()
        .unwrap()
}

/// The text of the file `name` inside the EPUB at `epub`.
fn read(epub: &Path, name: &str) -> String {
    let mut archive = ZipArchive::new(File::open(epub).unwrap()).unwrap();
    let mut text = String::new();
    archive
        .by_name(name)
        .unwrap_or_else(|_| panic!("no {name} in the book"))
        .read_to_string(&mut text)
        .unwrap();
    text
}

#[test]
fn binds_a_hub_into_a_book() {
    let vault = vault();
    let out = tempfile::tempdir().unwrap();

    let output = reliure(vault.path(), out.path(), &["--lang", "fr"]);

    assert!(output.status.success(), "{output:?}");
    let epub = out.path().join("Hub.epub");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n", epub.display())
    );
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 1);
    assert_eq!(read(&epub, "mimetype"), "application/epub+zip");
    assert!(read(&epub, "OEBPS/intro.xhtml").contains("<p>Opening.</p>"));
    assert!(read(&epub, "OEBPS/chapter-0.xhtml").contains("<h1>Design</h1>"));
    let clues = read(&epub, "OEBPS/note-0.xhtml");
    assert!(
        clues.contains("<a href=\"note-1.xhtml#scenes\">Pacing &gt; Scenes</a>"),
        "{clues}"
    );
    assert!(clues.contains("<strong>Astuce</strong>"), "{clues}");
    assert!(read(&epub, "OEBPS/note-1.xhtml").contains("<h2 id=\"scenes\">Scenes</h2>"));
}

#[test]
fn embeds_a_note_without_binding_it() {
    let vault = vault_with(&[
        (
            "Hub.md",
            "---\nbook: true\n---\n## Design\n![[Aside]]\n\n- [[Clues]]\n- ![[Aside]]\n- [[Pacing]]\n",
        ),
        ("Clues.md", "Three of them.\n\n![[Aside]]\n"),
        ("Aside.md", "---\ntags: [aside]\n---\nRead [[Pacing]].\n"),
        ("Pacing.md", "Short scenes.\n"),
    ]);
    let out = tempfile::tempdir().unwrap();

    let output = reliure(vault.path(), out.path(), &[]);

    assert!(output.status.success(), "{output:?}");
    let epub = out.path().join("Hub.epub");
    let clues = read(&epub, "OEBPS/note-0.xhtml");
    assert!(
        clues.contains("<p>Three of them.</p>\n<p>Read <a href=\"note-1.xhtml\">Pacing</a>.</p>"),
        "{clues}"
    );
    assert!(!clues.contains("tags"), "{clues}");
    let chapter = read(&epub, "OEBPS/chapter-0.xhtml");
    assert!(
        chapter.contains("<h1>Design</h1>\n<p>Read <a href=\"note-1.xhtml\">Pacing</a>.</p>"),
        "{chapter}"
    );
    let mut archive = ZipArchive::new(File::open(&epub).unwrap()).unwrap();
    assert!(archive.by_name("OEBPS/note-2.xhtml").is_err());
}

#[test]
fn an_ambiguous_embed_fails_naming_the_book_and_the_page() {
    let vault = vault_with(&[
        ("Hub.md", "---\nbook: true\n---\n## Design\n- [[Pacing]]\n"),
        ("Pacing.md", "Short scenes.\n\n![[Twin]]\n"),
        ("one/Twin.md", ""),
        ("two/Twin.md", ""),
    ]);
    let out = tempfile::tempdir().unwrap();

    let output = reliure(vault.path(), out.path(), &[]);

    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(
        error.contains(
            "cannot write the book Hub\n\nCaused by:\n    0: in the page Pacing\n    1: cannot embed ![[Twin]], which several notes match: one/Twin.md, two/Twin.md"
        ),
        "{error}"
    );
}

#[test]
fn a_vault_without_a_book_fails() {
    let vault = tempfile::tempdir().unwrap();
    fs::write(vault.path().join("Note.md"), "Text\n").unwrap();
    let out = tempfile::tempdir().unwrap();

    let output = reliure(vault.path(), out.path(), &[]);

    assert!(!output.status.success());
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("no note has `book: true`"), "{error}");
}
