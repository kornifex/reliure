use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use walkdir::{DirEntry, WalkDir};

use crate::note::{self, Note};

/// Every note of an Obsidian vault, by name.
///
/// Note names are unique across the vault, as Obsidian links rely on it.
pub struct Vault {
    notes: HashMap<String, Note>,
}

impl Vault {
    /// Reads every Markdown note under `root`, skipping hidden folders (`.obsidian`, `.git`, `.trash`).
    pub fn load(root: &Path) -> Result<Vault> {
        let mut notes = HashMap::new();
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
            // A .md path always has a file stem.
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            notes.insert(name.clone(), note::parse(name, &text)?);
        }
        Ok(Vault { notes })
    }

    pub fn get(&self, name: &str) -> Option<&Note> {
        self.notes.get(name)
    }

    /// The hubs marked `livre: true`, sorted by name.
    pub fn books(&self) -> Vec<&Note> {
        let mut hubs: Vec<&Note> = self.notes.values().filter(|note| note.properties.livre).collect();
        hubs.sort_by(|a, b| a.name.cmp(&b.name));
        hubs
    }
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry.file_name().to_string_lossy().starts_with('.')
}
