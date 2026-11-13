use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};
use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ReferenceType, ZipLibrary};
use pulldown_cmark_escape::escape_html;

use crate::book::Book;
use crate::lang::Lang;
use crate::render::{BoundNotes, to_xhtml};
use crate::vault::Vault;

const STYLESHEET: &str = include_str!("stylesheet.css");

/// Writes `book` as an EPUB file at `path`, one page for the intro, then for each chapter
/// a page with its text followed by a page per note.
pub fn write(book: &Book, vault: &Vault, lang: Lang, path: &Path) -> Result<()> {
    let mut notes = BoundNotes::default();
    for (i, name) in book.chapters.iter().flat_map(|chapter| &chapter.notes).enumerate() {
        // The plan only lists notes found in the vault.
        notes.insert(name, format!("note-{i}.xhtml"), &vault.get(name).unwrap().body);
    }
    let render_page = |title: &str, markdown: &str| page(title, &to_xhtml(markdown, lang, &notes), lang);

    let mut epub = EpubBuilder::new(ZipLibrary::new()?)?;
    epub.epub_version(EpubVersion::V30);
    epub.set_title(&book.title);
    epub.set_languages(vec![lang.tag().into()]);
    epub.set_toc_name(lang.table_of_contents());
    epub.stylesheet(STYLESHEET.as_bytes())?;
    epub.add_content(
        EpubContent::new("intro.xhtml", render_page(&book.title, &book.intro).as_bytes())
            .title(&book.title)
            .reftype(ReferenceType::Text),
    )?;
    for (i, chapter) in book.chapters.iter().enumerate() {
        epub.add_content(
            EpubContent::new(format!("chapter-{i}.xhtml"), render_page(&chapter.title, &chapter.text).as_bytes())
                .title(&chapter.title),
        )?;
        for name in &chapter.notes {
            // Both found above: the note in the vault, its page among the bound notes.
            let note = vault.get(name).unwrap();
            let file = notes.file(name).unwrap();
            epub.add_content(
                EpubContent::new(file, render_page(name, &note.body).as_bytes())
                    .title(name)
                    .level(2),
            )?;
        }
    }

    let file = File::create(path).with_context(|| format!("cannot create {}", path.display()))?;
    epub.generate(file)?;
    Ok(())
}

/// An XHTML page titled `title`, around a body fragment.
fn page(title: &str, body: &str, lang: Lang) -> String {
    let mut title_xhtml = String::new();
    // Writing into a String cannot fail.
    escape_html(&mut title_xhtml, title).unwrap();
    let tag = lang.tag();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="{tag}" lang="{tag}">
<head>
<meta charset="UTF-8"/>
<title>{title_xhtml}</title>
<link rel="stylesheet" type="text/css" href="stylesheet.css"/>
</head>
<body>
<h1>{title_xhtml}</h1>
{body}
</body>
</html>
"#
    )
}
