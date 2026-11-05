//! gikli's core.
//!
//! The core owns all behavior. [`Gikli`] is its application interface: the commands the UI sends
//! and the events it listens to. The UI and the tests use this same interface.

mod capability;
mod events;
mod git;
mod host;
mod notifications;
mod operation_log;
mod operations;
mod project;
mod settings;
mod status;
mod version;
mod watch;
mod window;
mod wsl;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

pub use capability::{Capability, CapabilityState};
pub use events::Event;
pub use host::HostEnvironment;
pub use notifications::{Notification, NotificationAction, NotificationKind, NotificationOutcome};
pub use operation_log::OperationLogEntry;
pub use operations::{LongOperation, Progress, RunningOperation};
pub use settings::Settings;
pub use status::{
    Change, ChangeGrouping, ChangeNode, ChangeSet, ChangeStatus, ConflictKind, SubmoduleState,
};
pub use version::GitVersion;
pub use window::ProjectWindow;

use events::EventBus;
use git::{GitProgram, GitRunner};
use notifications::Notifications;
use operation_log::OperationLog;
use operations::LongOperations;
use project::{ProjectRegistry, RememberedProject};
use watch::ChangesWatcher;
use window::ProjectWindows;
use wsl::WslShare;

/// The application interface of gikli's core.
pub struct Gikli {
    host: HostEnvironment,
    settings_file: PathBuf,
    settings: Mutex<Settings>,
    projects_file: PathBuf,
    /// The Repositories registered in each Project gikli opened.
    projects: Mutex<ProjectRegistry>,
    /// The Project each open window shows.
    windows: Mutex<ProjectWindows>,
    /// The result of looking for git, once gikli has looked.
    git: Mutex<Option<GitCheck>>,
    /// The git of each WSL distribution that holds a folder gikli opened, once gikli has looked.
    wsl_git: Mutex<HashMap<WslShare, GitCheck>>,
    /// The watcher of each Repository whose changes gikli watches, by root.
    watchers: Mutex<HashMap<PathBuf, ChangesWatcher>>,
    events: Arc<EventBus>,
    log: Arc<OperationLog>,
    notifications: Notifications,
    operations: LongOperations,
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
    /// The Repositories registered in the Project that are valid repositories, ordered by root.
    pub repositories: Vec<Repository>,
    /// The Repositories registered in the Project whose root is no longer a valid repository,
    /// because it was moved, deleted or broken, ordered by root.
    pub invalid_repositories: Vec<Repository>,
    /// The Repositories gikli found at or inside the folder that are neither registered nor
    /// ignored, ordered by root. gikli offers to add or ignore each one.
    pub unregistered_repositories: Vec<Repository>,
    /// The Repositories gikli found at or inside the folder that the user chose to ignore,
    /// ordered by root. They can still be added.
    pub ignored_repositories: Vec<Repository>,
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
    /// The Repositories of a Project could not be written to disk, so they were not changed.
    ProjectNotSaved {
        message: String,
    },
    /// The folder could not be created.
    #[serde(rename_all = "camelCase")]
    FolderNotCreated {
        folder: PathBuf,
        message: String,
    },
    /// `folder` is already the root of a repository, so no new one is created there.
    #[serde(rename_all = "camelCase")]
    RepositoryExists {
        folder: PathBuf,
    },
    /// git could not create a repository in `folder`. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    RepositoryNotCreated {
        folder: PathBuf,
    },
    /// The repository at `folder` is neither inside the Project's folder nor the Repository the
    /// folder is in, so it cannot be a Repository of the Project.
    #[serde(rename_all = "camelCase")]
    RepositoryOutsideProject {
        folder: PathBuf,
    },
    /// git could not clone `url` into `folder`. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    CloneFailed {
        url: String,
        folder: PathBuf,
    },
    /// The notification is gone: the user already acted on it or dismissed it, or what it said
    /// no longer holds.
    NotificationGone {
        id: u64,
    },
    /// The notification `id` does not offer the action asked for.
    NotificationActionNotOffered {
        id: u64,
    },
    /// `folder` is not the root of a repository, so it cannot be a Repository of a Project.
    #[serde(rename_all = "camelCase")]
    NotARepositoryRoot {
        folder: PathBuf,
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
    /// The core, running in `host`, keeping its application settings and the Repositories of
    /// each Project in `config_folder`.
    pub fn new(host: HostEnvironment, config_folder: &Path) -> Self {
        let settings_file = config_folder.join("settings.json");
        let projects_file = config_folder.join("projects.json");
        let events = Arc::<EventBus>::default();
        Self {
            host,
            settings: Mutex::new(Settings::load(&settings_file)),
            settings_file,
            projects: Mutex::new(ProjectRegistry::load(&projects_file)),
            projects_file,
            windows: Mutex::default(),
            git: Mutex::default(),
            wsl_git: Mutex::default(),
            watchers: Mutex::default(),
            log: Arc::new(OperationLog::new(events.clone())),
            notifications: Notifications::new(events.clone()),
            operations: LongOperations::new(events.clone()),
            events,
        }
    }

    /// Starts listening to the core's events. Each listener receives every event sent after it
    /// subscribed.
    pub fn subscribe(&self) -> Receiver<Event> {
        self.events.subscribe()
    }

    /// The git commands gikli ran, oldest first, with their raw output: the last
    /// thousand of them. Each new one also arrives as [`Event::OperationLogged`].
    pub fn operation_log(&self) -> Vec<OperationLogEntry> {
        self.log.entries()
    }

    /// Empties the operation log. Every listener receives [`Event::OperationLogCleared`].
    pub fn clear_operation_log(&self) {
        self.log.clear();
    }

    /// Looks for git and checks that gikli can use it.
    pub fn check_git(&self) -> GitCheck {
        let mut git = lock(&self.git);
        git.get_or_insert_with(|| {
            let configured = lock(&self.settings).git_path.clone();
            git::check(configured, &self.host, &self.log)
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

    /// Turns soft wrap of the operation log on or off.
    pub fn set_operation_log_soft_wrap(&self, soft_wrap: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.operation_log_soft_wrap = soft_wrap)
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
            &self.log,
        )
    }

    /// Opens `folder` as a Project. The first time, gikli registers the Repository the folder is
    /// in. Every time, it offers the Repositories it finds at or inside the folder that are
    /// neither registered nor ignored.
    pub fn open_project(&self, folder: &Path) -> Result<Project, Failure> {
        let place = self.project_folder(folder)?;
        let (remembered, opened_before) = {
            let mut projects = lock(&self.projects);
            match projects.remembered(&place.folder) {
                Some(remembered) => (remembered.clone(), true),
                None => {
                    let remembered = RememberedProject::first_open(place.toplevel.as_ref());
                    if !remembered.repositories.is_empty() {
                        projects.remember(place.folder.clone(), remembered.clone());
                        // Remembered on a best-effort basis: when the registry cannot be saved,
                        // the next start registers the same Repository again.
                        let _ = projects.save(&self.projects_file);
                    }
                    (remembered, false)
                }
            }
        };
        let project = place.project(&remembered)?;
        // A folder opened before keeps opening, even when the user ignored all it holds.
        if !opened_before && project.holds_nothing() {
            return Err(Failure::NotInRepository {
                folder: project.folder,
            });
        }
        self.notifications.withdraw_settled(&project);
        self.raise_project_notifications(&project);
        Ok(project)
    }

    /// The notifications raised and not yet acted on or dismissed, oldest first. Each new one also
    /// arrives as [`Event::NotificationRaised`].
    pub fn notifications(&self) -> Vec<Notification> {
        self.notifications.raised()
    }

    /// Takes `action`, one of the actions of the notification `id`. Once taken, the notification
    /// goes away with [`Event::NotificationRemoved`]; when the action fails, it stays.
    pub fn act_on_notification(
        &self,
        id: u64,
        action: &NotificationAction,
    ) -> Result<NotificationOutcome, Failure> {
        let notification = self.notifications.offering(id, action)?;
        let folder = &notification.project;
        let project = match action {
            NotificationAction::AddRepository { repository } => {
                self.add_repository(folder, repository)?
            }
            NotificationAction::IgnoreRepository { repository } => {
                self.ignore_repository(folder, repository)?
            }
            NotificationAction::RemoveRepository { repository } => {
                self.remove_repository(folder, repository)?
            }
        };
        Ok(NotificationOutcome::ProjectChanged(project))
    }

    /// Removes the notification `id` without acting on it. Nothing happens when it is already
    /// gone.
    pub fn dismiss_notification(&self, id: u64) {
        self.notifications.dismiss(id);
    }

    /// Raises a notification for each Repository of `project` that needs the user: one found and
    /// not registered, to add or ignore it, and one registered and no longer valid, to remove it.
    fn raise_project_notifications(&self, project: &Project) {
        for repository in &project.unregistered_repositories {
            self.notifications.raise(
                &project.folder,
                NotificationKind::UnregisteredRepository {
                    repository: repository.clone(),
                },
                vec![
                    NotificationAction::AddRepository {
                        repository: repository.clone(),
                    },
                    NotificationAction::IgnoreRepository {
                        repository: repository.clone(),
                    },
                ],
            );
        }
        for repository in &project.invalid_repositories {
            self.notifications.raise(
                &project.folder,
                NotificationKind::InvalidRepository {
                    repository: repository.clone(),
                },
                vec![NotificationAction::RemoveRepository {
                    repository: repository.clone(),
                }],
            );
        }
    }

    /// Registers `repository`, found at or inside the Project's folder, in the Project at
    /// `folder`, and returns the Project. An ignored Repository is no longer ignored.
    pub fn add_repository(
        &self,
        folder: &Path,
        repository: &Repository,
    ) -> Result<Project, Failure> {
        let place = self.project_folder(folder)?;
        let root = place.repository_root(repository)?;
        if !place.holds(&root) {
            return Err(Failure::RepositoryOutsideProject { folder: root });
        }
        self.change_project(place, |remembered| {
            remembered.ignored.remove(&root);
            remembered.repositories.insert(root);
        })
    }

    /// Stops offering `repository` to the Project at `folder`, and returns the Project.
    pub fn ignore_repository(
        &self,
        folder: &Path,
        repository: &Repository,
    ) -> Result<Project, Failure> {
        let place = self.project_folder(folder)?;
        let root = place.repository_root(repository)?;
        self.change_project(place, |remembered| {
            remembered.repositories.remove(&root);
            remembered.ignored.insert(root);
        })
    }

    /// Unregisters `repository` from the Project at `folder`, valid or not, and returns the
    /// Project. A valid one is offered again.
    pub fn remove_repository(
        &self,
        folder: &Path,
        repository: &Repository,
    ) -> Result<Project, Failure> {
        let place = self.project_folder(folder)?;
        self.change_project(place, |remembered| {
            remembered.repositories.remove(&repository.root);
        })
    }

    /// Applies `change` to what gikli remembers about the Project at `place`, and saves it.
    /// When it cannot be saved, nothing changes.
    fn change_project(
        &self,
        place: ProjectFolder,
        change: impl FnOnce(&mut RememberedProject),
    ) -> Result<Project, Failure> {
        let remembered = {
            let mut projects = lock(&self.projects);
            let mut remembered = projects
                .remembered(&place.folder)
                .cloned()
                .unwrap_or_else(|| RememberedProject::first_open(place.toplevel.as_ref()));
            change(&mut remembered);
            let mut changed = projects.clone();
            changed.remember(place.folder.clone(), remembered.clone());
            changed
                .save(&self.projects_file)
                .map_err(|error| Failure::ProjectNotSaved {
                    message: error.to_string(),
                })?;
            *projects = changed;
            remembered
        };
        let project = place.project(&remembered)?;
        self.notifications.withdraw_settled(&project);
        Ok(project)
    }

    /// Where the Project at `folder`, as [`Project::folder`] gives it, shows when the window
    /// labelled `window` asks to open it: in that window when it shows no Project yet, in the
    /// window that already shows it, or else in a new window.
    pub fn window_for_project(&self, window: &str, folder: &Path) -> ProjectWindow {
        lock(&self.windows).window_for(window, folder)
    }

    /// The folder of the Project the window labelled `window` shows, if any. A new window asks
    /// this to know which Project to open.
    pub fn project_in_window(&self, window: &str) -> Option<PathBuf> {
        lock(&self.windows).project_in(window)
    }

    /// Forgets the window labelled `window`, which closed, so its Project opens in a new window
    /// next time.
    pub fn window_closed(&self, window: &str) {
        lock(&self.windows).closed(window);
    }

    /// Creates a new Repository in `folder` with `git init`, creating the folder when it does not
    /// exist. A folder inside another Repository gets a Repository of its own.
    pub fn create_repository(&self, folder: &Path) -> Result<Repository, Failure> {
        let runner = self.runner(folder)?;
        std::fs::create_dir_all(folder).map_err(|error| Failure::FolderNotCreated {
            folder: folder.to_path_buf(),
            message: error.to_string(),
        })?;
        let folder = absolute_folder(folder)?;
        if is_repository_root(&runner, &folder)? {
            return Err(Failure::RepositoryExists { folder });
        }
        let invocation = runner
            .run(&folder, &["init"])
            .map_err(git_failed_to_start)?;
        if !invocation.success {
            return Err(Failure::RepositoryNotCreated { folder });
        }
        Ok(Repository { root: folder })
    }

    /// The long operations running, with the last progress of each, for a view that opens while
    /// they run. Their progress then arrives as events.
    pub fn running_operations(&self) -> Vec<RunningOperation> {
        self.operations.running()
    }

    /// Clones the repository at `url` into `folder`, which must not exist or be empty, and opens
    /// it as a Project. The clone runs as a [`LongOperation`] that reports its progress; the
    /// rest of the core keeps answering meanwhile.
    pub fn clone_repository(&self, url: &str, folder: &Path) -> Result<Project, Failure> {
        let runner = self.runner(folder)?;
        let target = std::path::absolute(folder).map_err(|_| Failure::FolderNotFound {
            folder: folder.to_path_buf(),
        })?;
        let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
            return Err(Failure::CloneFailed {
                url: url.to_owned(),
                folder: target,
            });
        };
        std::fs::create_dir_all(parent).map_err(|error| Failure::FolderNotCreated {
            folder: parent.to_path_buf(),
            message: error.to_string(),
        })?;
        // The folder goes by its name inside its parent, a path git inside WSL reads too.
        let name = name.to_string_lossy();
        let operation = LongOperation::Clone {
            url: url.to_owned(),
            folder: target.clone(),
        };
        let invocation = self
            .operations
            .run(operation, |report| {
                runner.run_reporting(parent, &["clone", "--progress", "--", url, &name], report)
            })
            .map_err(git_failed_to_start)?;
        if !invocation.success {
            return Err(Failure::CloneFailed {
                url: url.to_owned(),
                folder: target,
            });
        }
        self.open_project(&target)
    }

    /// The Project folder `folder`, in its absolute form, with the runner for it.
    fn project_folder(&self, folder: &Path) -> Result<ProjectFolder, Failure> {
        let runner = self.runner(folder)?;
        let folder = absolute_folder(folder)?;
        if !folder.is_dir() {
            return Err(Failure::FolderNotFound { folder });
        }
        let toplevel = runner
            .toplevel(&folder)
            .map_err(git_failed_to_start)?
            .map(|root| canonicalize(&root).unwrap_or(root));
        Ok(ProjectFolder {
            runner,
            folder,
            toplevel,
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
    /// to its working tree or its git directory, gikli sends [`Event::FilesChanged`]. Watching
    /// it again changes nothing.
    pub fn watch_changes(&self, repository: &Repository) -> Result<(), Failure> {
        if lock(&self.watchers).contains_key(&repository.root) {
            return Ok(());
        }
        let git_dir = self
            .runner(&repository.root)?
            .git_dir(&repository.root)
            .map_err(git_failed_to_start)?
            .map(|git_dir| canonicalize(&git_dir).unwrap_or(git_dir));
        let watcher = ChangesWatcher::start(repository.clone(), git_dir, self.events.clone())
            .map_err(|error| Failure::ChangesNotWatched {
                folder: repository.root.clone(),
                message: error.to_string(),
            })?;
        // Two calls at once may both start a watcher; the first one kept wins.
        lock(&self.watchers)
            .entry(repository.root.clone())
            .or_insert(watcher);
        Ok(())
    }

    /// Stops watching `repository` for changes made outside gikli, when gikli watches it.
    pub fn unwatch_changes(&self, repository: &Repository) {
        lock(&self.watchers).remove(&repository.root);
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
        Ok(GitRunner::new(program, self.host.clone(), self.log.clone()))
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
            .or_insert_with(|| git::check_wsl(share, &self.host, &self.log))
            .clone()
    }
}

/// The folder of a Project, with what git says about it.
struct ProjectFolder {
    /// The runner for operations in the folder.
    runner: GitRunner,
    /// The folder, in its absolute form.
    folder: PathBuf,
    /// The root of the Repository the folder is in, if any.
    toplevel: Option<PathBuf>,
}

impl ProjectFolder {
    /// The Project in this folder, whose remembered Repositories are `remembered`.
    fn project(&self, remembered: &RememberedProject) -> Result<Project, Failure> {
        let mut repositories = Vec::new();
        let mut invalid_repositories = Vec::new();
        for root in &remembered.repositories {
            let repository = Repository { root: root.clone() };
            if is_repository_root(&self.runner, root)? {
                repositories.push(repository);
            } else {
                invalid_repositories.push(repository);
            }
        }
        let mut found: Vec<PathBuf> = self.toplevel.iter().cloned().collect();
        for candidate in project::nested_candidates(&self.folder) {
            if is_repository_root(&self.runner, &candidate)? {
                found.push(candidate);
            }
        }
        found.sort();
        found.dedup();
        let (ignored, unregistered): (Vec<_>, Vec<_>) = found
            .into_iter()
            .filter(|root| !remembered.repositories.contains(root))
            .map(|root| Repository { root })
            .partition(|repository| remembered.ignored.contains(&repository.root));
        Ok(Project {
            folder: self.folder.clone(),
            repositories,
            invalid_repositories,
            unregistered_repositories: unregistered,
            ignored_repositories: ignored,
        })
    }

    /// The root of `repository` in its absolute form, when it is the root of a repository.
    fn repository_root(&self, repository: &Repository) -> Result<PathBuf, Failure> {
        let root = canonicalize(&repository.root).unwrap_or_else(|_| repository.root.clone());
        if !is_repository_root(&self.runner, &root)? {
            return Err(Failure::NotARepositoryRoot { folder: root });
        }
        Ok(root)
    }

    /// Whether the repository at `root` belongs in this Project: it is at or inside the folder,
    /// or it is the Repository the folder is in.
    fn holds(&self, root: &Path) -> bool {
        root.starts_with(&self.folder) || self.toplevel.as_deref() == Some(root)
    }
}

impl Project {
    /// Whether the Project shows no Repository at all.
    fn holds_nothing(&self) -> bool {
        self.repositories.is_empty()
            && self.invalid_repositories.is_empty()
            && self.unregistered_repositories.is_empty()
            && self.ignored_repositories.is_empty()
    }
}

/// Whether `folder` is the root of a repository, as git reports it.
fn is_repository_root(runner: &GitRunner, folder: &Path) -> Result<bool, Failure> {
    if !folder.is_dir() {
        return Ok(false);
    }
    let toplevel = runner.toplevel(folder).map_err(git_failed_to_start)?;
    Ok(toplevel.is_some_and(|root| canonicalize(&root).unwrap_or(root) == folder))
}

/// The absolute form of `folder`, which must exist.
fn absolute_folder(folder: &Path) -> Result<PathBuf, Failure> {
    canonicalize(folder).map_err(|_| Failure::FolderNotFound {
        folder: folder.to_path_buf(),
    })
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

/// Locks `mutex`, even when a thread panicked while holding it, so one failed command does not
/// stop every later one.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn git_failed_to_start(error: std::io::Error) -> Failure {
    Failure::GitFailedToStart {
        message: error.to_string(),
    }
}
