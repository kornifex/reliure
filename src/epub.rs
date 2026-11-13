use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};
use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ReferenceType, ZipLibrary};
use pulldown_cmark_escape::escape_html;

use crate::book::Book;
use crate::render::to_xhtml;
use crate::vault::Vault;

const STYLESHEET: &str = include_str!("stylesheet.css");

/// Writes `book` as an EPUB file at `path`, one page for the intro, then for each chapter
/// a page with its text followed by a page per note.
pub fn write(book: &Book, vault: &Vault, path: &Path) -> Result<()> {
    let note_pages: HashMap<&str, String> = book
        .chapters
        .iter()
        .flat_map(|chapter| &chapter.notes)
        .enumerate()
        .map(|(i, name)| (name.as_str(), format!("note-{i}.xhtml")))
        .collect();
    let href_of = |name: &str| note_pages.get(name).cloned();

    let mut epub = EpubBuilder::new(ZipLibrary::new()?)?;
    epub.epub_version(EpubVersion::V30);
    epub.set_title(&book.title);
    epub.set_languages(vec!["fr".into()]);
    epub.stylesheet(STYLESHEET.as_bytes())?;
    epub.add_content(
        EpubContent::new("intro.xhtml", page(&book.title, &to_xhtml(&book.intro, href_of)).as_bytes())
            .title(&book.title)
            .reftype(ReferenceType::Text),
    )?;
    for (i, chapter) in book.chapters.iter().enumerate() {
        epub.add_content(
            EpubContent::new(
                format!("chapter-{i}.xhtml"),
                page(&chapter.title, &to_xhtml(&chapter.text, href_of)).as_bytes(),
            )
            .title(&chapter.title),
        )?;
        for name in &chapter.notes {
            // The plan only lists notes found in the vault.
            let note = vault.get(name).unwrap();
            epub.add_content(
                EpubContent::new(&note_pages[name.as_str()], page(name, &to_xhtml(&note.body, href_of)).as_bytes())
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
fn page(title: &str, body: &str) -> String {
    let mut title_xhtml = String::new();
    // Writing into a String cannot fail.
    escape_html(&mut title_xhtml, title).unwrap();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="fr" lang="fr">
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
