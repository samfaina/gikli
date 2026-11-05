//! gikli's core.
//!
//! The core owns all behavior. [`Gikli`] is its application interface: the commands the UI sends
//! and the events it listens to. The UI and the tests use this same interface.

mod events;
mod git;
mod host;
mod version;

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, OnceLock};

use serde::Serialize;

pub use events::{Event, OperationLogEntry};
pub use host::HostEnvironment;
pub use version::GitVersion;

use events::EventBus;
use git::GitRunner;

/// The application interface of gikli's core.
pub struct Gikli {
    host: HostEnvironment,
    git: OnceLock<GitCheck>,
    events: Arc<EventBus>,
}

/// The oldest git gikli works with.
pub const MINIMUM_GIT_VERSION: GitVersion = GitVersion::new(2, 34, 0);

/// What gikli found when it looked for git.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum GitCheck {
    /// A git gikli can use.
    #[serde(rename_all = "camelCase")]
    Ready {
        executable: PathBuf,
        version: GitVersion,
    },
    /// A git older than [`MINIMUM_GIT_VERSION`]. gikli warns and runs no operations.
    #[serde(rename_all = "camelCase")]
    TooOld {
        executable: PathBuf,
        version: GitVersion,
        minimum: GitVersion,
    },
    /// A git that does not report a version gikli can read.
    #[serde(rename_all = "camelCase")]
    Unrecognized { executable: PathBuf },
    /// No git on the PATH.
    NotFound { minimum: GitVersion },
}

/// A folder opened in gikli, holding one or more Repositories (ADR-0004).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub folder: PathBuf,
    pub repositories: Vec<Repository>,
}

/// One git repository inside a Project, identified by its root folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub root: PathBuf,
}

/// Why a command could not be carried out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Failure {
    GitNotFound,
    GitTooOld {
        version: GitVersion,
        minimum: GitVersion,
    },
    GitUnrecognized,
    /// git could not be started.
    GitFailedToStart {
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    FolderNotFound {
        folder: PathBuf,
    },
}

impl Gikli {
    pub fn new(host: HostEnvironment) -> Self {
        Self {
            host,
            git: OnceLock::new(),
            events: Arc::default(),
        }
    }

    /// Starts listening to the core's events. Each listener receives every event sent after it
    /// subscribed.
    pub fn subscribe(&self) -> Receiver<Event> {
        self.events.subscribe()
    }

    /// Looks for git and checks that gikli can use it.
    pub fn check_git(&self) -> GitCheck {
        self.git
            .get_or_init(|| git::check(&self.host, &self.events))
            .clone()
    }

    /// Opens `folder` as a Project and detects the Repository it is in. A folder outside any
    /// repository opens as a Project with no Repositories.
    pub fn open_project(&self, folder: &Path) -> Result<Project, Failure> {
        let runner = self.runner()?;
        let folder = dunce::canonicalize(folder).map_err(|_| Failure::FolderNotFound {
            folder: folder.to_path_buf(),
        })?;
        if !folder.is_dir() {
            return Err(Failure::FolderNotFound { folder });
        }
        let toplevel = runner.run(
            &folder,
            &["rev-parse", "--path-format=absolute", "--show-toplevel"],
        )?;
        let mut repositories = Vec::new();
        if toplevel.success {
            let root = toplevel.stdout_path();
            let root = dunce::canonicalize(&root).unwrap_or(root);
            repositories.push(Repository { root });
        }
        Ok(Project {
            folder,
            repositories,
        })
    }

    /// The runner for every operation, available only with a usable git.
    fn runner(&self) -> Result<GitRunner, Failure> {
        match self.check_git() {
            GitCheck::Ready { executable, .. } => Ok(GitRunner::new(
                executable,
                self.host.clone(),
                self.events.clone(),
            )),
            GitCheck::TooOld {
                version, minimum, ..
            } => Err(Failure::GitTooOld { version, minimum }),
            GitCheck::Unrecognized { .. } => Err(Failure::GitUnrecognized),
            GitCheck::NotFound { .. } => Err(Failure::GitNotFound),
        }
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Failure::GitFailedToStart {
            message: error.to_string(),
        }
    }
}
