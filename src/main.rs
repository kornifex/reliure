mod book;
mod crosspoint;
mod epub;
mod lang;
mod note;
mod render;
mod vault;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::lang::Lang;
use crate::vault::Vault;

/// Binds Obsidian notes into EPUB books for e-readers: one book per hub note whose
/// checkbox property (`book` by default) is checked.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Root folder of the Obsidian vault.
    vault: PathBuf,
    /// Folder the EPUB files are written to.
    #[arg(long, default_value = ".")]
    out: PathBuf,
    /// Checkbox property marking a hub as a book.
    #[arg(long, default_value = "book")]
    property: String,
    /// Language of the books: EPUB metadata and callout headings.
    #[arg(long, value_enum, default_value_t = Lang::En)]
    lang: Lang,
    /// Also upload the books to a CrossPoint e-reader in File Transfer mode, at this host
    /// (`crosspoint.local`, or the IP address shown on its screen).
    #[arg(long, value_name = "HOST")]
    send: Option<String>,
    /// Folder of the e-reader the books are uploaded to.
    #[arg(long, default_value = "/", requires = "send")]
    folder: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let vault = Vault::load(&cli.vault)?;
    let hubs = vault.books(&cli.property)?;
    if hubs.is_empty() {
        bail!(
            "no note has `{}: true` in {}",
            cli.property,
            cli.vault.display()
        );
    }
    for hub in hubs {
        let book = book::plan(hub, &vault)?;
        let path = cli.out.join(format!("{}.epub", hub.name));
        epub::write(&book, &vault, cli.lang, &path)
            .with_context(|| format!("cannot write the book {}", hub.name))?;
        println!("{}", path.display());
        if let Some(host) = &cli.send {
            crosspoint::upload(host, &cli.folder, &path)?;
            println!("  sent to {host}:{}", cli.folder);
        }
    }
    Ok(())
}
