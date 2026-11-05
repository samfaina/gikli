//! The project registry: the Repositories registered in each Project and the ones the user chose
//! to ignore, kept as JSON in one file, and the search for Repositories inside a Project's folder.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What gikli remembers about every Project it opened, by folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ProjectRegistry {
    projects: BTreeMap<PathBuf, ProjectEntry>,
}

/// What gikli remembers about one Project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ProjectEntry {
    /// The roots of the Repositories registered in the Project.
    pub repositories: BTreeSet<PathBuf>,
    /// The roots of the Repositories found in the Project that the user chose not to add.
    pub ignored: BTreeSet<PathBuf>,
}

impl ProjectRegistry {
    /// Reads the registry in `file`. A missing or unreadable file gives an empty registry, so
    /// gikli always starts.
    pub fn load(file: &Path) -> Self {
        fs::read(file)
            .ok()
            .and_then(|json| serde_json::from_slice(&json).ok())
            .unwrap_or_default()
    }

    /// Writes the registry to `file`, creating its folder.
    pub fn save(&self, file: &Path) -> io::Result<()> {
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(file, json)
    }

    /// What gikli remembers about the Project at `folder`, if it opened it before.
    pub fn get(&self, folder: &Path) -> Option<&ProjectEntry> {
        self.projects.get(folder)
    }

    /// Remembers `entry` for the Project at `folder`.
    pub fn insert(&mut self, folder: PathBuf, entry: ProjectEntry) {
        self.projects.insert(folder, entry);
    }
}

/// The folders below `folder` that hold a `.git` entry, so may be the roots of Repositories,
/// ordered by path. Links are not followed, so a link to a folder above cannot loop.
pub(crate) fn nested_candidates(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let is_folder = entry.file_type().is_ok_and(|kind| kind.is_dir());
            if !is_folder || entry.file_name() == ".git" {
                continue;
            }
            let path = entry.path();
            if path.join(".git").symlink_metadata().is_ok() {
                found.push(path.clone());
            }
            pending.push(path);
        }
    }
    found.sort();
    found
}
