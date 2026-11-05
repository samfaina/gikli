//! gikli's core.
//!
//! The core owns all behavior. [`Gikli`] is its application interface: the commands the UI sends
//! and the events it listens to. The UI and the tests use this same interface.

mod capability;
mod events;
mod git;
mod host;
mod settings;
mod status;
mod version;
mod watch;
mod wsl;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

pub use capability::{Capability, CapabilityState};
pub use events::{Event, OperationLogEntry};
pub use host::HostEnvironment;
pub use settings::Settings;
pub use status::{
    Change, ChangeGrouping, ChangeNode, ChangeSet, ChangeStatus, ConflictKind, SubmoduleState,
};
pub use version::GitVersion;

use events::EventBus;
use git::{GitProgram, GitRunner};
use watch::ChangesWatcher;
use wsl::WslShare;

/// The application interface of gikli's core.
pub struct Gikli {
    host: HostEnvironment,
    settings_file: PathBuf,
    settings: Mutex<Settings>,
    /// The result of looking for git, once gikli has looked.
    git: Mutex<Option<GitCheck>>,
    /// The git of each WSL distribution that holds a folder gikli opened, once gikli has looked.
    wsl_git: Mutex<HashMap<WslShare, GitCheck>>,
    /// The watcher of each Repository whose changes gikli watches, by root.
    watchers: Mutex<HashMap<PathBuf, ChangesWatcher>>,
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
        source: GitSource,
        version: GitVersion,
    },
    /// A git older than [`MINIMUM_GIT_VERSION`]. gikli warns and runs no operations.
    #[serde(rename_all = "camelCase")]
    TooOld {
        executable: PathBuf,
        source: GitSource,
        version: GitVersion,
        minimum: GitVersion,
    },
    /// A git that does not report a version gikli can read.
    #[serde(rename_all = "camelCase")]
    Unrecognized {
        executable: PathBuf,
        source: GitSource,
    },
    /// No file at the path to git in the settings.
    #[serde(rename_all = "camelCase")]
    Missing { executable: PathBuf },
    /// No git anywhere gikli looks.
    NotFound { minimum: GitVersion },
}

/// Where gikli found the git it checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GitSource {
    /// The path to git in the settings.
    Configured,
    /// The folders of PATH.
    Path,
    /// The git inside WSL, on Windows when neither of the others has one. The executable is
    /// `wsl.exe`.
    Wsl,
}

/// A folder opened in gikli, holding one or more Repositories (ADR-0004).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub folder: PathBuf,
    pub repositories: Vec<Repository>,
}

/// One git repository inside a Project, identified by its root folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// There is no folder at that path.
    #[serde(rename_all = "camelCase")]
    FolderNotFound {
        folder: PathBuf,
    },
    /// git found no repository at or above the folder, so there is no Project to open.
    #[serde(rename_all = "camelCase")]
    NotInRepository {
        folder: PathBuf,
    },
    /// The settings could not be written to disk, so they were not changed.
    SettingsNotSaved {
        message: String,
    },
    /// git could not read the changes of the Repository at `folder`, for example because it is
    /// no longer a repository. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    ChangesUnreadable {
        folder: PathBuf,
    },
    /// gikli could not watch the Repository at `folder` for changes made outside it. Its changes
    /// still refresh on request.
    #[serde(rename_all = "camelCase")]
    ChangesNotWatched {
        folder: PathBuf,
        message: String,
    },
}

impl Gikli {
    /// The core, running in `host`, with its application settings kept in `settings_file`.
    pub fn new(host: HostEnvironment, settings_file: PathBuf) -> Self {
        Self {
            host,
            settings: Mutex::new(Settings::load(&settings_file)),
            settings_file,
            git: Mutex::default(),
            wsl_git: Mutex::default(),
            watchers: Mutex::default(),
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
        let mut git = lock(&self.git);
        git.get_or_insert_with(|| {
            let configured = lock(&self.settings).git_path.clone();
            git::check(configured, &self.host, &self.events)
        })
        .clone()
    }

    /// Sets the path to git, or clears it so gikli looks for git itself, and looks for git again.
    pub fn set_git_path(&self, path: Option<PathBuf>) -> Result<GitCheck, Failure> {
        self.change_settings(|settings| settings.git_path = path)?;
        lock(&self.git).take();
        Ok(self.check_git())
    }

    /// Turns the listing of ignored files in the changes view on or off.
    pub fn set_show_ignored_files(&self, show: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.show_ignored_files = show)
    }

    /// Groups the changes view by folder or shows it as a flat list.
    pub fn set_change_grouping(&self, grouping: ChangeGrouping) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.change_grouping = grouping)
    }

    /// The application settings.
    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    /// Applies `change` to the settings and saves them. When they cannot be saved, nothing
    /// changes.
    fn change_settings(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings, Failure> {
        let mut settings = lock(&self.settings);
        let mut changed = settings.clone();
        change(&mut changed);
        changed
            .save(&self.settings_file)
            .map_err(|error| Failure::SettingsNotSaved {
                message: error.to_string(),
            })?;
        *settings = changed.clone();
        Ok(changed)
    }

    /// Checks the git at `executable` without using it: the test button of the git path setting.
    pub fn test_git(&self, executable: &Path) -> GitCheck {
        git::check_executable(
            executable.to_path_buf(),
            GitSource::Configured,
            &self.host,
            &self.events,
        )
    }

    /// Opens `folder` as a Project and detects the Repository it is in.
    pub fn open_project(&self, folder: &Path) -> Result<Project, Failure> {
        let runner = self.runner(folder)?;
        let folder = canonicalize(folder).map_err(|_| Failure::FolderNotFound {
            folder: folder.to_path_buf(),
        })?;
        if !folder.is_dir() {
            return Err(Failure::FolderNotFound { folder });
        }
        let Some(root) = runner.toplevel(&folder).map_err(git_failed_to_start)? else {
            return Err(Failure::NotInRepository { folder });
        };
        let root = canonicalize(&root).unwrap_or(root);
        Ok(Project {
            folder,
            repositories: vec![Repository { root }],
        })
    }

    /// Reads the uncommitted changes of `repository`. Reading again refreshes them.
    pub fn read_changes(&self, repository: &Repository) -> Result<ChangeSet, Failure> {
        let runner = self.runner(&repository.root)?;
        let settings = self.settings();
        let invocation = runner
            .run(
                &repository.root,
                &status::status_args(settings.show_ignored_files),
            )
            .map_err(git_failed_to_start)?;
        if !invocation.success {
            return Err(Failure::ChangesUnreadable {
                folder: repository.root.clone(),
            });
        }
        Ok(ChangeSet::new(
            repository.clone(),
            status::parse(&invocation.stdout),
            settings.change_grouping,
        ))
    }

    /// Starts watching `repository` for changes made outside gikli: after each burst of changes
    /// to its files, gikli sends [`Event::FilesChanged`]. Watching it again changes nothing.
    pub fn watch_changes(&self, repository: &Repository) -> Result<(), Failure> {
        let mut watchers = lock(&self.watchers);
        if !watchers.contains_key(&repository.root) {
            let watcher = ChangesWatcher::start(repository.clone(), self.events.clone()).map_err(
                |error| Failure::ChangesNotWatched {
                    folder: repository.root.clone(),
                    message: error.to_string(),
                },
            )?;
            watchers.insert(repository.root.clone(), watcher);
        }
        Ok(())
    }

    /// What the git that runs in `repository` can do beyond the minimum gikli needs. A feature
    /// whose capability is unavailable is shown disabled, with the version it needs.
    pub fn capabilities(&self, repository: &Repository) -> Result<Vec<CapabilityState>, Failure> {
        let (_, version) = self.git_for(&repository.root)?;
        Ok(capability::of(version))
    }

    /// The runner for an operation in `folder`, available only with a usable git.
    fn runner(&self, folder: &Path) -> Result<GitRunner, Failure> {
        let (program, _) = self.git_for(folder)?;
        Ok(GitRunner::new(
            program,
            self.host.clone(),
            self.events.clone(),
        ))
    }

    /// The git that runs in `folder`, with its version: the git of the WSL distribution for a
    /// folder under `\\wsl$`, and the git gikli found otherwise.
    fn git_for(&self, folder: &Path) -> Result<(GitProgram, GitVersion), Failure> {
        Ok(match WslShare::of(folder) {
            Some((share, _)) => {
                let (wsl, _, version) = usable(self.check_wsl_git(&share))?;
                let program = GitProgram::Wsl {
                    wsl,
                    share: Some(share),
                };
                (program, version)
            }
            None => {
                let (executable, source, version) = usable(self.check_git())?;
                (GitProgram::found(executable, source), version)
            }
        })
    }

    /// Looks for the git inside the WSL distribution of `share`.
    fn check_wsl_git(&self, share: &WslShare) -> GitCheck {
        lock(&self.wsl_git)
            .entry(share.clone())
            .or_insert_with(|| git::check_wsl(share, &self.host, &self.events))
            .clone()
    }
}

/// The executable, source and version of a git gikli can run operations with.
fn usable(check: GitCheck) -> Result<(PathBuf, GitSource, GitVersion), Failure> {
    match check {
        GitCheck::Ready {
            executable,
            source,
            version,
        } => Ok((executable, source, version)),
        GitCheck::TooOld {
            version, minimum, ..
        } => Err(Failure::GitTooOld { version, minimum }),
        GitCheck::Unrecognized { .. } => Err(Failure::GitUnrecognized),
        GitCheck::Missing { .. } | GitCheck::NotFound { .. } => Err(Failure::GitNotFound),
    }
}

/// The absolute form of `path`, with links resolved. A folder under `\\wsl$` keeps that form
/// instead of becoming `\\?\UNC\wsl$`.
fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = dunce::canonicalize(path)?;
    Ok(match WslShare::of(&canonical) {
        Some((share, linux_path)) => share.windows_path(&linux_path),
        None => canonical,
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn git_failed_to_start(error: std::io::Error) -> Failure {
    Failure::GitFailedToStart {
        message: error.to_string(),
    }
}
