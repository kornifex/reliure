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
    String::from_utf8(read_bytes(epub, name)).unwrap()
}

/// The bytes of the file `name` inside the EPUB at `epub`.
fn read_bytes(epub: &Path, name: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(File::open(epub).unwrap()).unwrap();
    let mut bytes = Vec::new();
    archive
        .by_name(name)
        .unwrap_or_else(|_| panic!("no {name} in the book"))
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

/// A PNG image of one pixel.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x00, 0x00, 0x00, 0x00, 0x3a, 0x7e, 0x9b,
    0x55, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x01, 0x48, 0xaf, 0xa4, 0x71, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

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
fn creates_the_folder_the_books_go_to() {
    let vault = vault();
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("books/new");

    let output = reliure(vault.path(), &out, &[]);

    assert!(output.status.success(), "{output:?}");
    assert!(out.join("Hub.epub").is_file());
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
fn copies_the_images_the_pages_show_into_the_book() {
    let vault = vault_with(&[
        (
            "Hub.md",
            "---\nbook: true\n---\n## Design\n- [[Clues]]\n- [[Pacing]]\n",
        ),
        ("Clues.md", "![[assets/map.png]]\n"),
        (
            "Pacing.md",
            "Scenes ![[assets/Map.png|The map]] and ![[old/Map.png]].\n",
        ),
        ("old/Map.png", "an older map"),
    ]);
    fs::create_dir(vault.path().join("assets")).unwrap();
    fs::write(vault.path().join("assets/Map.png"), PNG).unwrap();
    let out = tempfile::tempdir().unwrap();

    let output = reliure(vault.path(), out.path(), &[]);

    assert!(output.status.success(), "{output:?}");
    let epub = out.path().join("Hub.epub");
    let clues = read(&epub, "OEBPS/note-0.xhtml");
    assert!(
        clues.contains("<p><img src=\"images/2d6a4b79-Map.png\" alt=\"Map.png\"/></p>"),
        "{clues}"
    );
    let pacing = read(&epub, "OEBPS/note-1.xhtml");
    assert!(
        pacing.contains(
            "<p>Scenes <img src=\"images/2d6a4b79-Map.png\" alt=\"The map\"/> and \
             <img src=\"images/fbe7717c-Map.png\" alt=\"Map.png\"/>.</p>"
        ),
        "{pacing}"
    );
    assert_eq!(read_bytes(&epub, "OEBPS/images/2d6a4b79-Map.png"), PNG);
    assert_eq!(read(&epub, "OEBPS/images/fbe7717c-Map.png"), "an older map");
    let package = read(&epub, "OEBPS/content.opf");
    assert_eq!(
        package.matches("href=\"images/2d6a4b79-Map.png\"").count(),
        1,
        "{package}"
    );
    assert!(package.contains("media-type=\"image/png\""), "{package}");
}

/// The values of the attribute `name` of the `element` tags of `xml`, in order.
fn attributes<'x>(xml: &'x str, element: &str, name: &str) -> Vec<&'x str> {
    let attribute = format!(" {name}=\"");
    xml.split(&format!("<{element}"))
        .skip(1)
        // Each tag from the space after its element's name, leaving out longer names.
        .filter(|tag| tag.starts_with(' '))
        .filter_map(|tag| {
            let tag = &tag[..tag.find('>').unwrap()];
            let value = &tag[tag.find(&attribute)? + attribute.len()..];
            Some(&value[..value.find('"').unwrap()])
        })
        .collect()
}

#[test]
fn images_whose_names_look_alike_stay_apart_in_the_book() {
    let vault = vault_with(&[
        ("Hub.md", "---\nbook: true\n---\n## Design\n- [[Shots]]\n"),
        (
            "Shots.md",
            "![[a b.png]] ![[a_b.png]] ![[x/y.png]] ![[x_y.png]]\n",
        ),
        ("a b.png", "a space"),
        ("a_b.png", "an underscore"),
        ("x/y.png", "a folder"),
        ("x_y.png", "no folder"),
    ]);
    let out = tempfile::tempdir().unwrap();

    let output = reliure(vault.path(), out.path(), &[]);

    assert!(output.status.success(), "{output:?}");
    let epub = out.path().join("Hub.epub");
    let package = read(&epub, "OEBPS/content.opf");
    let mut ids = attributes(&package, "item", "id");
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "{package}");
    let hrefs = attributes(&package, "item", "href");
    assert_eq!(count, hrefs.len(), "{package}");
    let archive = ZipArchive::new(File::open(&epub).unwrap()).unwrap();
    let entries: Vec<&str> = archive.file_names().collect();
    let shots = read(&epub, "OEBPS/note-0.xhtml");
    let sources = attributes(&shots, "img", "src");
    assert_eq!(sources.len(), 4, "{shots}");
    for (source, text) in sources
        .iter()
        .zip(["a space", "an underscore", "a folder", "no folder"])
    {
        assert!(hrefs.contains(source), "{source} in {package}");
        assert!(
            entries.contains(&format!("OEBPS/{source}").as_str()),
            "{source}"
        );
        assert_eq!(read(&epub, &format!("OEBPS/{source}")), text);
    }
    // No vault folder shows in the book.
    let images: Vec<&&str> = entries
        .iter()
        .filter(|entry| entry.starts_with("OEBPS/images/"))
        .collect();
    assert_eq!(images.len(), 4, "{entries:?}");
    assert!(
        images
            .iter()
            .all(|entry| !entry["OEBPS/images/".len()..].contains('/')),
        "{images:?}"
    );
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
