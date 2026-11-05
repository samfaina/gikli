//! Which window shows each Project: one window per Project, with several open at once. The
//! shell creates, focuses and closes the windows; this decides which one shows a Project.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Where a Project shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProjectWindow {
    /// In the window that asked, which shows no other Project.
    Here,
    /// In `window`, which already shows it: bring that window to the front.
    Open { window: String },
    /// In a new window, labelled `window`, for the shell to create.
    New { window: String },
}

/// The Project each open window shows, by window label.
#[derive(Debug, Default)]
pub(crate) struct ProjectWindows {
    projects: BTreeMap<String, PathBuf>,
    /// How many new windows were handed out, so each label is new.
    created: u64,
}

impl ProjectWindows {
    /// Where the Project at `folder` shows, when the window `asking` asks to open it.
    pub fn window_for(&mut self, asking: &str, folder: &Path) -> ProjectWindow {
        if let Some(window) = self.window_showing(folder) {
            return if window == asking {
                ProjectWindow::Here
            } else {
                ProjectWindow::Open { window }
            };
        }
        if !self.projects.contains_key(asking) {
            self.projects
                .insert(asking.to_owned(), folder.to_path_buf());
            return ProjectWindow::Here;
        }
        self.created += 1;
        let window = format!("project-{}", self.created);
        self.projects.insert(window.clone(), folder.to_path_buf());
        ProjectWindow::New { window }
    }

    /// The folder of the Project `window` shows, if any.
    pub fn project_in(&self, window: &str) -> Option<PathBuf> {
        self.projects.get(window).cloned()
    }

    /// Forgets `window`, which closed.
    pub fn closed(&mut self, window: &str) {
        self.projects.remove(window);
    }

    fn window_showing(&self, folder: &Path) -> Option<String> {
        self.projects
            .iter()
            .find(|(_, shown)| shown.as_path() == folder)
            .map(|(window, _)| window.clone())
    }
}
