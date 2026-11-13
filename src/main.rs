mod book;
mod epub;
mod note;
mod render;
mod vault;

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;

use crate::vault::Vault;

/// Binds Obsidian notes into EPUB books: one book per hub marked `livre: true`.
#[derive(Parser)]
struct Cli {
    /// Root folder of the Obsidian vault.
    vault: PathBuf,
    /// Folder the EPUB files are written to.
    #[arg(long, default_value = ".")]
    out: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let vault = Vault::load(&cli.vault)?;
    let hubs = vault.books();
    if hubs.is_empty() {
        bail!("no note has `livre: true` in {}", cli.vault.display());
    }
    for hub in hubs {
        let book = book::plan(hub, |name| vault.get(name).is_some());
        let path = cli.out.join(format!("{}.epub", hub.name));
        epub::write(&book, &vault, &path)?;
        println!("{}", path.display());
    }
    Ok(())
}
