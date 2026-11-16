//! gikli's application settings, kept as JSON in one file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ChangeGrouping;

/// gikli's application settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The git executable to use. When unset, gikli looks for git itself. A folder under
    /// `\\wsl$` always uses the git of its WSL distribution.
    pub git_path: Option<PathBuf>,
    /// Whether the changes view lists the files git ignores.
    pub show_ignored_files: bool,
    /// How the changes view arranges the changes.
    pub change_grouping: ChangeGrouping,
    /// Whether the operation log wraps long lines to its width, instead of scrolling sideways.
    pub operation_log_soft_wrap: bool,
    /// Whether the user's credential helpers answer for credentials and keep them. When off, git
    /// asks the user every time and nothing saves the answers.
    pub use_credential_helper: bool,
    /// Whether a clone also clones the submodules of the repository, and theirs.
    pub clone_submodules_recursively: bool,
    /// Whether the user can edit and save the local side of a diff.
    pub diff_local_side_editable: bool,
    /// Whether moving on from the last difference of a file goes to the next file.
    pub go_to_next_file_after_last_change: bool,
    /// Whether the user is warned before committing in detached HEAD or during a rebase.
    pub warn_on_unusual_commit_state: bool,
    /// Whether the commit message is emptied once the commit went through.
    pub clear_commit_message_after_commit: bool,
    /// Whether Commit and push opens the push dialog once the commit went through. When off, the
    /// push runs at once.
    pub show_push_dialog_for_commit_and_push: bool,
    /// Whether Commit and push opens the push dialog only when the push goes to a protected
    /// branch, and pushes at once otherwise. It applies while the dialog shows for Commit and push.
    pub show_push_dialog_only_for_protected_branches: bool,
    /// Whether the changes view shows the git index as Staged and Unstaged changes instead of
    /// Changelists.
    pub staging_mode: bool,
    /// Whether the log draws an edge that spans many rows whole, instead of as two arrows that
    /// lead to each other.
    pub show_long_edges: bool,
    /// How the log arranges the files a commit changed.
    pub commit_changes_grouping: ChangeGrouping,
    /// The most revisions a file history lists, newest first. It is at least 1.
    pub file_history_row_limit: usize,
}

/// How many revisions a file history lists until the user sets another limit.
const DEFAULT_FILE_HISTORY_ROW_LIMIT: usize = 1000;

impl Default for Settings {
    fn default() -> Self {
        Self {
            git_path: None,
            show_ignored_files: false,
            change_grouping: ChangeGrouping::default(),
            operation_log_soft_wrap: false,
            use_credential_helper: true,
            clone_submodules_recursively: true,
            diff_local_side_editable: true,
            go_to_next_file_after_last_change: true,
            warn_on_unusual_commit_state: true,
            clear_commit_message_after_commit: true,
            show_push_dialog_for_commit_and_push: true,
            show_push_dialog_only_for_protected_branches: false,
            staging_mode: false,
            show_long_edges: false,
            commit_changes_grouping: ChangeGrouping::default(),
            file_history_row_limit: DEFAULT_FILE_HISTORY_ROW_LIMIT,
        }
    }
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
