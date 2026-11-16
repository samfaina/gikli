//! The searches of the log the user ran, kept as JSON in one file so they come back next time.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;

use crate::lock;
use crate::log::TextSearch;

/// The most searches kept.
const KEPT_SEARCHES: usize = 20;

/// The searches of the log the user ran, newest first, each once.
pub(crate) struct SearchHistory {
    file: PathBuf,
    searches: Mutex<Vec<TextSearch>>,
}

impl SearchHistory {
    /// The history kept in `file`. A missing or unreadable file gives an empty one.
    pub fn load(file: &Path) -> Self {
        let searches = fs::read(file)
            .ok()
            .and_then(|json| serde_json::from_slice::<Vec<Saved>>(&json).ok())
            .unwrap_or_default()
            .into_iter()
            .map(TextSearch::from)
            .collect();
        Self {
            file: file.to_path_buf(),
            searches: Mutex::new(searches),
        }
    }

    /// The searches, newest first.
    pub fn searches(&self) -> Vec<TextSearch> {
        lock(&self.searches).clone()
    }

    /// Puts `ran` first, as the search the user ran last. The history is a help, so when it
    /// cannot be written the search still goes on, and only the next start does not see it.
    pub fn remember(&self, ran: &TextSearch) {
        if ran.text.is_empty() {
            return;
        }
        let mut searches = lock(&self.searches);
        searches.retain(|search| search != ran);
        searches.insert(0, ran.clone());
        searches.truncate(KEPT_SEARCHES);
        let saved = self
            .file
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| {
                let json = serde_json::to_vec_pretty(&*searches).map_err(std::io::Error::other)?;
                fs::write(&self.file, json)
            });
        drop(saved);
    }
}

/// A search as the file keeps it.
#[derive(Deserialize)]
#[serde(untagged)]
enum Saved {
    /// A search with its options.
    Search(TextSearch),
    /// The text alone, as an earlier gikli kept it, from before the history kept the options.
    Text(String),
}

impl From<Saved> for TextSearch {
    fn from(saved: Saved) -> Self {
        match saved {
            Saved::Search(search) => search,
            Saved::Text(text) => Self {
                text,
                ..Self::default()
            },
        }
    }
}
