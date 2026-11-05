//! gikli's application settings, kept as JSON in one file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// gikli's application settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The git executable to use. When unset, gikli looks for git itself. A folder under
    /// `\\wsl$` always uses the git of its WSL distribution.
    pub git_path: Option<PathBuf>,
}

impl Settings {
    /// Reads the settings in `file`. A missing or unreadable file gives the defaults, so gikli
    /// always starts.
    pub(crate) fn load(file: &Path) -> Self {
        fs::read(file)
            .ok()
            .and_then(|json| serde_json::from_slice(&json).ok())
            .unwrap_or_default()
    }

    /// Writes the settings to `file`, creating its folder.
    pub(crate) fn save(&self, file: &Path) -> io::Result<()> {
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(file, json)
    }
}
