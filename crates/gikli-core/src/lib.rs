//! gikli's core.
//!
//! The core owns all behavior. [`Gikli`] is its application interface: the commands the UI sends
//! and the events it listens to. The UI and the tests use this same interface.

mod backups;
mod branches;
mod capability;
mod changelists;
mod checkout;
mod clone_url;
mod commit;
mod conflicts;
mod delete_branches;
mod details;
mod diff;
mod events;
mod explainer;
mod file_history;
mod git;
mod host;
mod in_progress;
mod listed_commit;
mod log;
mod merge;
mod new_branch;
mod notifications;
mod operation_log;
mod operations;
mod process_tree;
mod processes;
mod project;
mod prompts;
mod pull;
mod push;
mod rename_branch;
mod rollback;
mod saved_local_changes;
mod search_history;
mod settings;
mod shallow;
mod stash;
mod status;
mod sync;
mod trace;
mod undo_commit;
mod url_passwords;
mod version;
mod watch;
mod window;
mod wsl;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

pub use backups::{Backup, Rewrite};
pub use branches::{Branch, BranchNode, Branches};
pub use capability::{Capability, CapabilityState};
pub use changelists::MoveTarget;
pub use checkout::{CheckoutTarget, LocalCommits, OverLocalChanges};
pub use clone_url::CloneUrlForms;
pub use commit::CommitWarning;
pub use conflicts::{ConflictSide, ConflictedFile, SideChange};
pub use delete_branches::{DeletedBranch, RemoteBranch};
pub use details::{CommitChanges, CommitDetails, MessagePart};
pub use diff::{
    DiffContent, Difference, FileDiff, LineRange, LocalReadOnly, RevisionFileDiff, TextDiff,
    UnchangedRegion,
};
pub use events::Event;
pub use explainer::{ExplainedMessage, Explanation, FailureAction, NextStep};
pub use file_history::{FileHistory, FileRevision};
pub use host::HostEnvironment;
pub use in_progress::{OperationAction, OperationInProgress, OperationKind};
pub use listed_commit::ListedCommit;
pub use log::{
    GraphArrow, GraphLine, GraphRow, LOG_SEARCH_BATCH, LONG_EDGE, LogCursor, LogFilter, LogPage,
    LogRow, RefKind, RefLabel, TextSearch,
};
pub use new_branch::NewBranchBase;
pub use notifications::{Notification, NotificationAction, NotificationKind, NotificationOutcome};
pub use operation_log::OperationLogEntry;
pub use operations::{LongOperation, Progress, RunningOperation};
pub use prompts::{Prompt, PromptKind, answer_askpass_request};
pub use pull::{PullOptions, PullPlan, PullRemote, PullSource};
pub use push::{PushAfterCommit, PushPlan, PushTags, PushTarget, UpdateMethod};
pub use rollback::RollbackPlan;
use saved_local_changes::{AwaitingMerge, AwaitingMergeStore};
pub use saved_local_changes::{RestoredLocalChanges, SavedLocalChanges};
pub use settings::Settings;
pub use stash::{
    Stash, StashDiffBase, StashFileDiff, StashOptions, StashUnavailable, Stashes, UnstashOptions,
    UnstashPlan, Unstashed,
};
pub use status::{
    Change, ChangeGroup, ChangeGrouping, ChangeNode, ChangeSet, ChangeStatus, Changelist,
    ConflictKind, StagingArea, StagingSide, SubmoduleState,
};
pub use sync::{CurrentBranch, IncomingOutgoing, LocalBranch, Upstream};
pub use version::GitVersion;
pub use window::ProjectWindow;

use changelists::{ChangelistStore, Changelists};
use events::EventBus;
use git::{GitProgram, GitRunner, Invocation};
use notifications::{Deletion, Notifications, Rewritten};
use operation_log::OperationLog;
use operations::LongOperations;
use project::{ProjectRegistry, RememberedProject};
use prompts::PromptBridge;
use search_history::SearchHistory;
use url_passwords::hide_url_passwords;
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
    /// Held while the Changelists of a Repository are read, changed and saved, so one change does
    /// not write over another.
    changelists: Mutex<()>,
    /// The Project each open window shows.
    windows: Mutex<ProjectWindows>,
    /// The result of looking for git, once gikli has looked.
    git: Mutex<Option<GitCheck>>,
    /// The git of each WSL distribution that holds a folder gikli opened, once gikli has looked.
    wsl_git: Mutex<HashMap<WslShare, GitCheck>>,
    /// The watcher of each Repository whose changes gikli watches, by root.
    watchers: Mutex<HashMap<PathBuf, ChangesWatcher>>,
    /// The reads of the log gikli holds for the pages to come.
    log_reads: log::LogReads,
    /// The searches of the log the user ran.
    log_searches: SearchHistory,
    events: Arc<EventBus>,
    log: Arc<OperationLog>,
    notifications: Notifications,
    operations: LongOperations,
    /// What brings prompts to the user, once [`Self::bring_prompts_through`] has started it.
    prompts: Option<PromptBridge>,
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
    /// git failed for a reason gikli knows, and can say in plain words what to do about it.
    Explained {
        explanation: Explanation,
    },
    /// The user cancelled the operation, so git and every process it started ended. Not a failure
    /// of the run: nothing went wrong. `left_behind` is what the run left in the user's way, with
    /// what to do about it, when it left anything.
    #[serde(rename_all = "camelCase")]
    OperationCancelled {
        left_behind: Option<Explanation>,
    },
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
    /// `folder` is a bare repository or inside a git directory. gikli opens working trees.
    #[serde(rename_all = "camelCase")]
    NotAWorkingTree {
        folder: PathBuf,
    },
    /// git could not open `folder`, for a reason gikli does not know. The operation log holds git's
    /// output.
    #[serde(rename_all = "camelCase")]
    ProjectNotOpened {
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
    /// git could not clone `url`, its password hidden, into `folder`. The operation log holds git's
    /// output.
    #[serde(rename_all = "camelCase")]
    CloneFailed {
        url: String,
        folder: PathBuf,
    },
    /// The user cancelled the clone into `folder`, and gikli could not remove what it had created
    /// there.
    #[serde(rename_all = "camelCase")]
    CancelledCloneNotRemoved {
        folder: PathBuf,
        message: String,
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
    /// The prompt is gone: the user already answered or cancelled it, or what asked went away.
    PromptGone {
        id: u64,
    },
    /// `folder` is not the root of a repository, so it cannot be a Repository of a Project.
    #[serde(rename_all = "camelCase")]
    NotARepositoryRoot {
        folder: PathBuf,
    },
    /// git could not add the files to the Repository at `folder`. The operation log holds git's
    /// output.
    #[serde(rename_all = "camelCase")]
    FilesNotAdded {
        folder: PathBuf,
    },
    /// git could not unstage the files in the Repository at `folder`. The operation log holds
    /// git's output.
    #[serde(rename_all = "camelCase")]
    FilesNotUnstaged {
        folder: PathBuf,
    },
    /// git could not roll back the files in the Repository at `folder`. The operation log holds
    /// git's output.
    #[serde(rename_all = "camelCase")]
    FilesNotRolledBack {
        folder: PathBuf,
    },
    /// The files changed after gikli listed what a rollback would discard, so nothing was rolled
    /// back. The user has to confirm the list as it is now.
    RollbackListOutdated,
    /// A commit needs a message, so nothing was committed.
    CommitMessageEmpty,
    /// Nothing the user selected is a change any more, so nothing was committed.
    NothingToCommit,
    /// The Repository has no commit yet, so there is nothing to amend.
    NothingToAmend,
    /// git could not commit in the Repository at `folder`. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    CommitFailed {
        folder: PathBuf,
    },
    /// Undo commit takes only the last commit of the current branch, and `commit`, by its full
    /// hash, is not that one, or HEAD is on no branch. Nothing changed.
    NotLastCommit {
        commit: String,
    },
    /// Undo commit does not rewrite shared history, and `commit` is on `branch`, the remote branch
    /// of a protected branch, such as `origin/main`. Nothing changed.
    CommitPushedToProtectedBranch {
        commit: String,
        branch: String,
    },
    /// git could not save a Backup in the Repository at `folder`, so the history rewrite it was
    /// for did not happen. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    BackupNotSaved {
        folder: PathBuf,
    },
    /// Undo of a history rewrite brings back only the branch the rewrite left, and `branch`, or
    /// the detached HEAD when there is none, has moved since. Nothing changed.
    BranchMovedSinceRewrite {
        branch: Option<String>,
    },
    /// git could not bring the branch back to a Backup in the Repository at `folder`. The
    /// operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    BackupNotRestored {
        folder: PathBuf,
    },
    /// git could not undo the last commit in the Repository at `folder`. The operation log holds
    /// git's output.
    #[serde(rename_all = "camelCase")]
    CommitNotUndone {
        folder: PathBuf,
    },
    /// gikli could not write what git reads for a commit in the Repository at `folder`, so nothing
    /// was committed.
    #[serde(rename_all = "camelCase")]
    CommitInputNotWritten {
        folder: PathBuf,
        message: String,
    },
    /// git has no `user.name` or no `user.email` to write in a commit, so nothing was committed. The
    /// user has to say what they are: see [`Gikli::set_commit_identity`].
    #[serde(rename_all = "camelCase")]
    CommitIdentityMissing {
        name_missing: bool,
        email_missing: bool,
    },
    /// A name or email for commits has to have some text, so nothing was saved.
    CommitIdentityEmpty,
    /// git could not save the name or email for commits in the Repository at `folder`. The
    /// operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    CommitIdentityNotSaved {
        folder: PathBuf,
    },
    /// git could not read the previous commit messages of the Repository at `folder`. The operation
    /// log holds git's output.
    #[serde(rename_all = "camelCase")]
    CommitMessagesUnreadable {
        folder: PathBuf,
    },
    /// git could not read the log of the Repository at `folder`. The operation log holds git's
    /// output.
    #[serde(rename_all = "camelCase")]
    LogUnreadable {
        folder: PathBuf,
    },
    /// The history changed since the page of the log before this one, so this one would not follow
    /// from it. The log starts over from the first page.
    LogChanged,
    /// The Repository at `folder` has no remote to fetch from.
    #[serde(rename_all = "camelCase")]
    NoRemoteToFetchFrom {
        folder: PathBuf,
    },
    /// git could not fetch the rest of the history of the Repository at `folder` from `remote`.
    /// The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    HistoryNotFetched {
        folder: PathBuf,
        remote: String,
    },
    /// The local `branch` has no upstream on a remote, so there is nothing to fetch for it alone.
    NoUpstream {
        branch: String,
    },
    /// git could not fetch into the Repository at `folder`. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    FetchFailed {
        folder: PathBuf,
    },
    /// The Repository has no remote called `remote`.
    RemoteNotFound {
        remote: String,
    },
    /// `name` is no name git takes for a branch.
    BranchNameInvalid {
        name: String,
    },
    /// Another branch of the Repository is already called `name`, so no branch was created.
    BranchNameTaken {
        name: String,
    },
    /// Another remote of the Repository is already called `name`, so no remote was defined.
    RemoteNameTaken {
        name: String,
    },
    /// `name` is no name git takes for a remote, so no remote was defined.
    RemoteNameInvalid {
        name: String,
    },
    /// A remote needs a URL, so no remote was defined.
    RemoteUrlEmpty,
    /// git could not define the remote `name` of the Repository at `folder`. The operation log
    /// holds git's output.
    #[serde(rename_all = "camelCase")]
    RemoteNotAdded {
        folder: PathBuf,
        name: String,
    },
    /// HEAD is detached, so there is no current branch to push.
    DetachedHeadNotPushed,
    /// The current `branch` has no commit yet, so there is nothing to push.
    BranchWithoutCommits {
        branch: String,
    },
    /// git could not push from the Repository at `folder`. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    PushFailed {
        folder: PathBuf,
    },
    /// The upstream of the current branch, the branch `branch` of `remote`, rejected the push from
    /// the Repository at `folder`, as it has incoming commits, and the settings say to ask
    /// the user how to integrate them before pushing again.
    #[serde(rename_all = "camelCase")]
    PushRejected {
        folder: PathBuf,
        remote: String,
        branch: String,
    },
    /// Integrating the upstream that rejected a push from the Repository at `folder` stopped on a
    /// conflict, so nothing was pushed. The user resolves it and pushes again.
    #[serde(rename_all = "camelCase")]
    UpdateStoppedOnConflicts {
        folder: PathBuf,
    },
    /// HEAD is detached, so there is no current branch to pull into.
    DetachedHeadNotPulled,
    /// git could not pull into the Repository at `folder`. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    PullFailed {
        folder: PathBuf,
    },
    /// Files of the Repository at `folder` keep git from merging a branch into the current branch,
    /// and nothing changed. `local_changes` are files with uncommitted changes the merge would
    /// overwrite, which the user can save and restore around the merge with
    /// [`Gikli::merge_over_local_changes`]. `untracked_files` are files git does not track that
    /// the merge would overwrite, which the user moves or commits. Both by path, relative to the
    /// Repository root.
    #[serde(rename_all = "camelCase")]
    MergeBlocked {
        folder: PathBuf,
        local_changes: Vec<PathBuf>,
        untracked_files: Vec<PathBuf>,
    },
    /// git could not merge a branch into the current branch of the Repository at `folder`. The
    /// operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    MergeFailed {
        folder: PathBuf,
    },
    /// The pull options `first` and `second`, as git names them, contradict each other, so the
    /// pull did not start.
    PullOptionsIncompatible {
        first: String,
        second: String,
    },
    /// git could not create the branch `name` in the Repository at `folder`. The operation log holds
    /// git's output.
    #[serde(rename_all = "camelCase")]
    BranchNotCreated {
        folder: PathBuf,
        name: String,
    },
    /// git could not rename the branch `name` in the Repository at `folder`. The operation log holds
    /// git's output.
    #[serde(rename_all = "camelCase")]
    BranchNotRenamed {
        folder: PathBuf,
        name: String,
    },
    /// git renamed the branch to `name` in the Repository at `folder`, and could not unset its
    /// upstream. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    UpstreamNotUnset {
        folder: PathBuf,
        name: String,
    },
    /// HEAD is on the branch `name`, which git does not delete, so no branch was deleted.
    CurrentBranchNotDeleted {
        name: String,
    },
    /// git could not delete the branch `name` in the Repository at `folder`. The operation log holds
    /// git's output.
    #[serde(rename_all = "camelCase")]
    BranchNotDeleted {
        folder: PathBuf,
        name: String,
    },
    /// git could not delete the branch `branch` of `remote` from the Repository at `folder`. The
    /// operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    RemoteBranchNotDeleted {
        folder: PathBuf,
        remote: String,
        branch: String,
    },
    /// git could not check out a branch, a tag or a commit in the Repository at `folder`. The
    /// operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    CheckoutFailed {
        folder: PathBuf,
    },
    /// Files of the Repository at `folder` keep git from checking out a branch, a tag or a commit,
    /// and nothing changed. `local_changes` are files with uncommitted changes the checkout would
    /// overwrite, which the user can discard or save and restore around the checkout with
    /// [`Gikli::check_out_over_local_changes`]. `untracked_files` are files git does not track
    /// that the checkout would overwrite or remove, which the user moves or commits. Both by path,
    /// relative to the Repository root.
    #[serde(rename_all = "camelCase")]
    CheckoutBlocked {
        folder: PathBuf,
        local_changes: Vec<PathBuf>,
        untracked_files: Vec<PathBuf>,
    },
    /// Checking out the remote branch `remote_branch` would move the local branch `branch` of the
    /// same name to it, and lose `commits`, the number of its commits the remote branch does not
    /// have. Nothing changed. The user drops them, with a Backup, or rebases them onto the remote
    /// branch, with [`Gikli::check_out_remote_branch`].
    #[serde(rename_all = "camelCase")]
    LocalCommitsWouldBeLost {
        branch: String,
        remote_branch: String,
        commits: usize,
    },
    /// git could not read the branches of the Repository at `folder`, or how far they are from
    /// their upstreams. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    BranchesUnreadable {
        folder: PathBuf,
    },
    /// No commit of the Repository goes by `revision`, as a hash, a branch, a tag or any other
    /// name git knows.
    #[serde(rename_all = "camelCase")]
    RevisionNotFound {
        revision: String,
    },
    /// git could not bring the file at `path` back to its version in a commit. The operation log
    /// holds git's output.
    #[serde(rename_all = "camelCase")]
    FileVersionNotRestored {
        path: PathBuf,
    },
    /// The text of a search of the log is no regular expression gikli can read, as `message`
    /// explains.
    #[serde(rename_all = "camelCase")]
    SearchPatternInvalid {
        message: String,
    },
    /// The lock of the index could not be removed.
    #[serde(rename_all = "camelCase")]
    IndexLockNotRemoved {
        lock: PathBuf,
        message: String,
    },
    /// git could not add `folder` to the folders the user trusts. The operation log holds git's
    /// output.
    FolderNotTrusted {
        folder: String,
    },
    /// git could not read the changes of the Repository at `folder`, for example because it is
    /// no longer a repository. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    ChangesUnreadable {
        folder: PathBuf,
    },
    /// git could not compare the file at `path` with HEAD. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    DiffUnreadable {
        path: PathBuf,
    },
    /// gikli could not read the file at `path` from disk.
    #[serde(rename_all = "camelCase")]
    LocalFileUnreadable {
        path: PathBuf,
        message: String,
    },
    /// The user cannot edit the file at `path`, as its [`TextDiff::local_read_only`] says.
    #[serde(rename_all = "camelCase")]
    LocalSideNotEditable {
        path: PathBuf,
    },
    /// The file at `path` is no longer what the user saw when they started editing it, so gikli did
    /// not write over it.
    #[serde(rename_all = "camelCase")]
    LocalFileChanged {
        path: PathBuf,
    },
    /// gikli could not write the file at `path`.
    #[serde(rename_all = "camelCase")]
    LocalFileNotSaved {
        path: PathBuf,
        message: String,
    },
    /// The Changelist is gone, for example because another window deleted it.
    ChangelistGone {
        id: String,
    },
    /// A Changelist needs a name, so nothing changed.
    ChangelistNameEmpty,
    /// Another Changelist of the Repository is already called `name`, so nothing changed.
    ChangelistNameTaken {
        name: String,
    },
    /// The Active changelist cannot be deleted: another one has to become active first.
    ActiveChangelistNotDeletable,
    /// gikli could not read the Changelists in `file`, and left it as it is.
    #[serde(rename_all = "camelCase")]
    ChangelistsUnreadable {
        file: PathBuf,
        message: String,
    },
    /// The Changelists in `file` have no Active changelist, so new changes would have nowhere to go.
    /// gikli left the file as it is.
    #[serde(rename_all = "camelCase")]
    ActiveChangelistMissing {
        file: PathBuf,
    },
    /// A newer gikli saved the Changelists in `file`, in a `version` of the format this one does not
    /// know, so it left it as it is.
    #[serde(rename_all = "camelCase")]
    ChangelistsFromNewerGikli {
        file: PathBuf,
        version: u32,
    },
    /// The Changelists could not be written to `file`, so they were not changed.
    #[serde(rename_all = "camelCase")]
    ChangelistsNotSaved {
        file: PathBuf,
        message: String,
    },
    /// No merge, rebase, revert or cherry-pick is half done in the Repository, so there is nothing
    /// to continue or abort.
    NoOperationInProgress,
    /// The operation half done in the Repository does not offer `action`.
    OperationActionNotOffered {
        action: OperationAction,
    },
    /// Files still have conflicts, so the operation in progress cannot go on. The user resolves
    /// them first.
    ConflictsNotResolved,
    /// The file at `path` is not in conflict, so there is nothing to resolve.
    FileNotInConflict {
        path: PathBuf,
    },
    /// git could not resolve the conflicts of the files in the Repository at `folder`. The
    /// operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    FilesNotResolved {
        folder: PathBuf,
    },
    /// git could not take `action` on the `operation` half done in the Repository at `folder`. The
    /// operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    OperationActionFailed {
        folder: PathBuf,
        operation: OperationKind,
        action: OperationAction,
    },
    /// The Repository at `folder` cannot stash, for `reason`, so its stash actions are
    /// unavailable.
    #[serde(rename_all = "camelCase")]
    StashUnavailable {
        folder: PathBuf,
        reason: StashUnavailable,
    },
    /// The local changes of the Repository at `folder` cannot be saved for `reason`, as they are
    /// saved in a stash, so an operation that saves them first is unavailable.
    #[serde(rename_all = "camelCase")]
    LocalChangesNotSavable {
        folder: PathBuf,
        reason: StashUnavailable,
    },
    /// gikli could not write `file`, where it keeps the local changes it saved around a merge until
    /// the merge is over, for `message`, the reason the system gives in the user's language. The
    /// changes are back and nothing was merged.
    #[serde(rename_all = "camelCase")]
    LocalChangesNotKept {
        file: PathBuf,
        message: String,
    },
    /// The Repository has no stash whose commit is `hash`: it was dropped, or `hash` is no stash.
    StashNotFound {
        hash: String,
    },
    /// The Repository at `folder` has no uncommitted change a stash would take, so git made no
    /// stash. Files git does not track count only when the stash includes them.
    #[serde(rename_all = "camelCase")]
    NothingToStash {
        folder: PathBuf,
    },
    /// git could not stash the changes of the Repository at `folder`. The operation log holds
    /// git's output.
    #[serde(rename_all = "camelCase")]
    NotStashed {
        folder: PathBuf,
    },
    /// git could not drop the stash of the Repository at `folder`. The operation log holds git's
    /// output.
    #[serde(rename_all = "camelCase")]
    StashNotDropped {
        folder: PathBuf,
    },
    /// The stashes of the Repository at `folder` are no longer the ones the user saw, so none was
    /// cleared. The user looks at them again first.
    #[serde(rename_all = "camelCase")]
    StashesChanged {
        folder: PathBuf,
    },
    /// git could not apply all of the stash to the Repository at `folder`, and left no file in
    /// conflict: it would have overwritten local changes, or files the stash holds that git did
    /// not track are on disk already. What git applied stays, and so does a branch it created
    /// from the stash. The stash is kept. The operation log holds git's output.
    #[serde(rename_all = "camelCase")]
    StashNotApplied {
        folder: PathBuf,
    },
    /// git could not clear the stashes of the Repository at `folder`. The operation log holds
    /// git's output.
    #[serde(rename_all = "camelCase")]
    StashesNotCleared {
        folder: PathBuf,
    },
    /// git could not list the stashes of the Repository at `folder`. The operation log holds git's
    /// output.
    #[serde(rename_all = "camelCase")]
    StashesUnreadable {
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
            changelists: Mutex::default(),
            windows: Mutex::default(),
            git: Mutex::default(),
            wsl_git: Mutex::default(),
            watchers: Mutex::default(),
            log_reads: log::LogReads::default(),
            log_searches: SearchHistory::load(&config_folder.join("log-searches.json")),
            log: Arc::new(OperationLog::new(events.clone())),
            notifications: Notifications::new(events.clone()),
            operations: LongOperations::new(events.clone()),
            prompts: None,
            events,
        }
    }

    /// Brings what git and ssh ask the user to the user, as [`Event::PromptRaised`]: credentials,
    /// passphrases of keys and host keys. They ask through the program `askpass`, which must call
    /// [`answer_askpass_request`] first thing. Without this, what needs an answer fails.
    pub fn bring_prompts_through(&mut self, askpass: PathBuf) -> std::io::Result<()> {
        self.prompts = Some(PromptBridge::start(askpass, self.events.clone())?);
        Ok(())
    }

    /// The prompts waiting for the user, oldest first, for a window that opens while they wait.
    /// Each new one arrives as [`Event::PromptRaised`].
    pub fn pending_prompts(&self) -> Vec<Prompt> {
        self.prompts
            .as_ref()
            .map(PromptBridge::pending)
            .unwrap_or_default()
    }

    /// Gives what asked the prompt `id` the user's `answer`, which gikli does not keep. The prompt
    /// closes with [`Event::PromptClosed`].
    pub fn answer_prompt(&self, id: u64, answer: &str) -> Result<(), Failure> {
        self.respond_to_prompt(id, Some(answer))
    }

    /// Tells what asked the prompt `id` that the user declined to answer, which usually fails the
    /// operation. The prompt closes with [`Event::PromptClosed`].
    pub fn cancel_prompt(&self, id: u64) -> Result<(), Failure> {
        self.respond_to_prompt(id, None)
    }

    fn respond_to_prompt(&self, id: u64, answer: Option<&str>) -> Result<(), Failure> {
        self.prompts
            .as_ref()
            .ok_or(Failure::PromptGone { id })?
            .respond(id, answer)
    }

    /// Starts listening to the core's events. Each listener receives every event sent after it
    /// subscribed.
    pub fn subscribe(&self) -> Receiver<Event> {
        self.events.subscribe()
    }

    /// The git commands gikli ran, oldest first, with their raw output, passwords in URLs hidden:
    /// the last thousand of them. Each new one also arrives as [`Event::OperationLogged`].
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

    /// Turns on or off drawing long edges of the log whole. It applies from the next first page of
    /// the log.
    pub fn set_show_long_edges(&self, show: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.show_long_edges = show)
    }

    /// Arranges the files a commit changed, in the log, as a tree of folders or as a flat list.
    pub fn set_commit_changes_grouping(
        &self,
        grouping: ChangeGrouping,
    ) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.commit_changes_grouping = grouping)
    }

    /// Sets the most revisions a file history lists, newest first. A limit below 1 is 1.
    pub fn set_file_history_row_limit(&self, limit: usize) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.file_history_row_limit = limit.max(1))
    }

    /// Groups the branches the branch selector and the branches pane list, such as
    /// `feature/login`, under their directory, or lists them by their whole name.
    pub fn set_group_branches_by_directory(&self, group: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.group_branches_by_directory = group)
    }

    /// Shows or hides Recent, the local branches checked out last, in the branch selector.
    pub fn set_show_recent_branches(&self, show: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.show_recent_branches = show)
    }

    /// Shows or hides the tags in the branch selector.
    pub fn set_show_tags_in_branch_selector(&self, show: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.show_tags_in_branch_selector = show)
    }

    /// Turns soft wrap of the operation log on or off.
    pub fn set_operation_log_soft_wrap(&self, soft_wrap: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.operation_log_soft_wrap = soft_wrap)
    }

    /// Leaves credentials to the user's credential helpers, or turns them off so git asks the user
    /// every time and nothing saves the answers.
    pub fn set_use_credential_helper(&self, use_helper: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.use_credential_helper = use_helper)
    }

    /// Makes a clone also clone the submodules of the repository, and theirs, or leave them out.
    pub fn set_clone_submodules_recursively(&self, recursively: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.clone_submodules_recursively = recursively)
    }

    /// Lets the user edit and save the local side of a diff, or turns that off.
    pub fn set_diff_local_side_editable(&self, editable: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.diff_local_side_editable = editable)
    }

    /// Makes moving on from the last difference of a file go to the next file, or stay.
    pub fn set_go_to_next_file_after_last_change(
        &self,
        go_to_next_file: bool,
    ) -> Result<Settings, Failure> {
        self.change_settings(|settings| {
            settings.go_to_next_file_after_last_change = go_to_next_file;
        })
    }

    /// Makes a commit in detached HEAD or during a rebase warn the user, or stops that.
    pub fn set_warn_on_unusual_commit_state(&self, warn: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.warn_on_unusual_commit_state = warn)
    }

    /// Makes the commit message empty once a commit went through, or keeps it.
    pub fn set_clear_commit_message_after_commit(&self, clear: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.clear_commit_message_after_commit = clear)
    }

    /// Makes Commit and push open the push dialog once the commit went through, or push at once.
    pub fn set_show_push_dialog_for_commit_and_push(
        &self,
        show: bool,
    ) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.show_push_dialog_for_commit_and_push = show)
    }

    /// Makes Commit and push open the push dialog only when the push goes to a protected branch,
    /// or whatever branch it goes to.
    pub fn set_show_push_dialog_only_for_protected_branches(
        &self,
        only_protected: bool,
    ) -> Result<Settings, Failure> {
        self.change_settings(|settings| {
            settings.show_push_dialog_only_for_protected_branches = only_protected;
        })
    }

    /// Makes a push of the current branch that its upstream rejects integrate the commits the
    /// branch lacks and push again without asking, with the method the user last chose to
    /// remember, or merge, or ask the user how first.
    pub fn set_auto_update_if_push_rejected(&self, auto_update: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.auto_update_if_push_rejected = auto_update)
    }

    /// Makes the diff of a file in a stash compare it with the file on disk or with the file in
    /// the commit the stash was made on.
    pub fn set_stash_diff_against(&self, against: StashDiffBase) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.stash_diff_against = against)
    }

    /// Switches between Changelist mode and Staging mode. The Changelists stay as they are.
    pub fn set_staging_mode(&self, enabled: bool) -> Result<Settings, Failure> {
        self.change_settings(|settings| settings.staging_mode = enabled)
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
    /// neither registered nor ignored, and deletes the Backups of its Repositories saved more than
    /// 30 days ago.
    pub fn open_project(&self, folder: &Path) -> Result<Project, Failure> {
        let place = self.project_folder(folder)?;
        place.ensure_can_open()?;
        let (remembered, opened_before) = {
            let mut projects = lock(&self.projects);
            match projects.remembered(&place.folder) {
                Some(remembered) => (remembered.clone(), true),
                None => {
                    let remembered = RememberedProject::first_open(place.toplevel.as_ref().ok());
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
            return Err(place.nothing_to_open());
        }
        self.notifications.withdraw_settled(&project);
        self.raise_project_notifications(&project);
        for repository in &project.repositories {
            // Best-effort: a Repository whose git cannot run keeps its Backups until next time.
            if let Ok(runner) = self.runner(&repository.root) {
                backups::delete_expired(&runner, repository);
            }
        }
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
            NotificationAction::RestoreBranches => return self.restore_branches(&notification),
            NotificationAction::ShowUnmergedCommits { branch } => {
                return self.unmerged_commits(&notification, branch);
            }
            NotificationAction::DeleteRemoteBranch { upstream } => {
                return self.delete_remote_branch(&notification, action, upstream);
            }
            NotificationAction::ContinueRebase => {
                let repository = notification.stopped_rebase()?;
                return self.act_on_operation(&notification, repository, OperationAction::Continue);
            }
            NotificationAction::AbortRebase => {
                let repository = notification.stopped_rebase()?;
                return self.act_on_operation(&notification, repository, OperationAction::Abort);
            }
            NotificationAction::ResolveConflicts => {
                let repository = notification.unresolved_conflicts()?;
                self.notifications.withdraw(notification.id);
                return Ok(NotificationOutcome::ResolveConflicts(repository.clone()));
            }
            NotificationAction::AbortOperation => {
                let repository = notification.unresolved_conflicts()?;
                return self.act_on_operation(&notification, repository, OperationAction::Abort);
            }
            NotificationAction::RestoreBackup => return self.restore_backup(&notification),
            NotificationAction::DropStash => return self.drop_kept_stash(&notification),
        };
        Ok(NotificationOutcome::ProjectChanged(project))
    }

    /// Drops the stash `notification` tells was kept after it conflicted, and removes the
    /// notification. One dropped since, as from the stashes tab, is gone already.
    fn drop_kept_stash(&self, notification: &Notification) -> Result<NotificationOutcome, Failure> {
        let (repository, stash) = notification.kept_stash()?;
        stash::drop_if_there(&self.runner(&repository.root)?, repository, &stash.hash)?;
        self.notifications.withdraw(notification.id);
        Ok(NotificationOutcome::StashesChanged(repository.clone()))
    }

    /// Creates the branches `notification` tells were deleted again, each on the commit it was on,
    /// and removes the notification. When a branch has one of their names by now, none is created
    /// and this returns [`Failure::BranchNameTaken`].
    fn restore_branches(
        &self,
        notification: &Notification,
    ) -> Result<NotificationOutcome, Failure> {
        let Deletion {
            repository,
            branches,
            ..
        } = notification.deletion()?;
        delete_branches::restore(&self.runner(&repository.root)?, repository, branches)?;
        self.notifications.dismiss(notification.id);
        Ok(NotificationOutcome::BranchesChanged(repository.clone()))
    }

    /// The commits of the branch `name` that `notification` tells was deleted, which HEAD did not
    /// have when it went, newest first. The notification stays.
    fn unmerged_commits(
        &self,
        notification: &Notification,
        name: &str,
    ) -> Result<NotificationOutcome, Failure> {
        let Deletion {
            repository,
            branches,
            head,
        } = notification.deletion()?;
        let branch = branches.iter().find(|branch| branch.name == name).ok_or(
            Failure::NotificationActionNotOffered {
                id: notification.id,
            },
        )?;
        let runner = self.runner(&repository.root)?;
        let commits =
            delete_branches::unmerged_commits(&runner, repository, &branch.commit, head, None)?;
        Ok(NotificationOutcome::UnmergedCommits(commits))
    }

    /// Deletes `upstream` on its remote, the remote branch a branch `notification` tells was
    /// deleted followed, as `action` of the notification asks. The push runs as a
    /// [`LongOperation`] that reports its progress and that the user can cancel. The notification
    /// stays, without the action.
    fn delete_remote_branch(
        &self,
        notification: &Notification,
        action: &NotificationAction,
        upstream: &RemoteBranch,
    ) -> Result<NotificationOutcome, Failure> {
        let repository = notification.deletion()?.repository;
        let runner = self.runner(&repository.root)?;
        let root = &repository.root;
        let operation = LongOperation::DeleteRemoteBranch {
            repository: repository.clone(),
            remote: upstream.remote.clone(),
            branch: upstream.branch.clone(),
        };
        let args = delete_branches::delete_remote_branch_args(upstream);
        let invocation = self.run_operation(&runner, operation, root, &as_args(&args))?;
        if !invocation.success {
            return Err(failure_of_contacting_remotes(
                &runner,
                root,
                &invocation,
                std::slice::from_ref(&upstream.remote),
                Failure::RemoteBranchNotDeleted {
                    folder: root.clone(),
                    remote: upstream.remote.clone(),
                    branch: upstream.branch.clone(),
                },
            ));
        }
        self.notifications.withdraw_action(notification.id, action);
        Ok(NotificationOutcome::BranchesChanged(repository.clone()))
    }

    /// Brings the branch the history rewrite `notification` tells of back to its Backup, and
    /// removes the notification. When the branch moved since the rewrite, nothing changes, the
    /// notification stays, and this returns [`Failure::BranchMovedSinceRewrite`].
    fn restore_backup(&self, notification: &Notification) -> Result<NotificationOutcome, Failure> {
        let Rewritten {
            repository,
            rewrite,
            backup,
            head,
        } = notification.history_rewrite()?;
        let runner = self.runner(&repository.root)?;
        backups::restore(&runner, repository, rewrite, backup, head)?;
        self.notifications.dismiss(notification.id);
        Ok(NotificationOutcome::HeadMoved(repository.clone()))
    }

    /// Takes `action` on the operation half done in `repository`, which `notification` tells of,
    /// and removes the notification. A rebase that goes on and stops again raises its notification
    /// anew.
    fn act_on_operation(
        &self,
        notification: &Notification,
        repository: &Repository,
        action: OperationAction,
    ) -> Result<NotificationOutcome, Failure> {
        self.run_operation_action(&notification.project, repository, action)?;
        self.notifications.withdraw(notification.id);
        self.operation_in_progress(&notification.project, repository)?;
        Ok(NotificationOutcome::HeadMoved(repository.clone()))
    }

    /// Takes `action`, one of the actions of an [`Explanation`]. The UI asks the user to confirm
    /// first: explaining a failure never changes anything by itself.
    pub fn take_failure_action(&self, action: &FailureAction) -> Result<(), Failure> {
        match action {
            FailureAction::TrustFolder { repository, folder } => {
                let runner = self.runner(&repository.root)?;
                let invocation = runner
                    .run(
                        &repository.root,
                        &["config", "--global", "--add", "safe.directory", folder],
                    )
                    .map_err(git_failed_to_start)?;
                if !invocation.success {
                    return Err(failure_of(
                        &runner,
                        &repository.root,
                        &invocation,
                        Failure::FolderNotTrusted {
                            folder: folder.clone(),
                        },
                    ));
                }
                Ok(())
            }
            FailureAction::RemoveIndexLock { repository } => {
                explainer::remove_stale_index_lock(&self.runner(&repository.root)?, repository)
            }
        }
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
                .unwrap_or_else(|| RememberedProject::first_open(place.toplevel.as_ref().ok()));
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
            let fallback = Failure::RepositoryNotCreated {
                folder: folder.clone(),
            };
            return Err(failure_of(&runner, &folder, &invocation, fallback));
        }
        Ok(Repository { root: folder })
    }

    /// The long operations running, with the last progress of each, for a view that opens while
    /// they run. Their progress then arrives as events.
    pub fn running_operations(&self) -> Vec<RunningOperation> {
        self.operations.running()
    }

    /// Cancels the long operation `id`: git and every process it started end, and the command that
    /// started the operation returns [`Failure::OperationCancelled`], or for a clone that is
    /// complete the Project, with [`NotificationKind::HookNotFinished`] raised. This does not wait
    /// for that. Nothing happens when the operation already ended, or does not offer it, as its
    /// [`RunningOperation::cancellable`] says.
    pub fn cancel_operation(&self, id: u64) {
        self.operations.cancel(id);
    }

    /// The HTTPS and SSH forms of `url`, for the user to choose between before they clone, when it
    /// has them. Nothing reaches git or the operation log.
    pub fn clone_url_forms(&self, url: &str) -> Option<CloneUrlForms> {
        clone_url::forms_of(url)
    }

    /// Clones the repository at `url` into `folder`, which must not exist or be empty, and opens
    /// it as a Project. The clone runs as a [`LongOperation`] that reports its progress; the
    /// rest of the core keeps answering meanwhile. A password in `url` goes to git and shows
    /// nowhere else.
    ///
    /// Its submodules, and theirs, are cloned too, unless
    /// [`Settings::clone_submodules_recursively`] says otherwise.
    ///
    /// When the user cancels it during a hook that runs after the checkout, with no submodules left
    /// to clone, the clone is complete: it opens as a Project, with
    /// [`NotificationKind::HookNotFinished`] raised. When they cancel it earlier, or while there are
    /// submodules left, it returns [`Failure::OperationCancelled`] and leaves `folder` as it found
    /// it.
    pub fn clone_repository(&self, url: &str, folder: &Path) -> Result<Project, Failure> {
        let runner = self.runner(folder)?;
        let target = std::path::absolute(folder).map_err(|_| Failure::FolderNotFound {
            folder: folder.to_path_buf(),
        })?;
        // Only git gets the URL as the user gave it.
        let shown_url = hide_url_passwords(url);
        let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
            return Err(Failure::CloneFailed {
                url: shown_url,
                folder: target,
            });
        };
        std::fs::create_dir_all(parent).map_err(|error| Failure::FolderNotCreated {
            folder: parent.to_path_buf(),
            message: error.to_string(),
        })?;
        let destination = CloneTarget {
            folder: target.clone(),
            found: FoundFolder::at(&target),
            with_submodules: self.settings().clone_submodules_recursively,
        };
        // The folder goes by its name inside its parent, a path git inside WSL reads too.
        let name = name.to_string_lossy();
        let operation = LongOperation::Clone {
            url: shown_url.clone(),
            folder: target.clone(),
        };
        let mut args = vec!["clone", "--progress"];
        if destination.with_submodules {
            args.push("--recurse-submodules");
        }
        args.extend(["--", url, &name]);
        let invocation = self.run_operation(&runner, operation, parent, &args)?;
        if invocation.cancelled {
            return self.end_cancelled_clone(&destination, invocation);
        }
        if !invocation.success {
            let explanation =
                explainer::explain_failed_clone(&runner, parent, &destination, url, &invocation);
            return Err(match explanation {
                Some(explanation) => Failure::Explained { explanation },
                None => Failure::CloneFailed {
                    url: shown_url,
                    folder: target,
                },
            });
        }
        self.open_project(&target)
    }

    /// Runs git with `args` in `folder` as `operation`, which the user can cancel unless git runs
    /// inside WSL.
    fn run_operation(
        &self,
        runner: &GitRunner,
        operation: LongOperation,
        folder: &Path,
        args: &[&str],
    ) -> Result<Invocation, Failure> {
        self.operations
            .run(operation, runner.runs_natively(), |run| {
                runner.run_operation(folder, args, run)
            })
            .map_err(git_failed_to_start)
    }

    /// What is left to do about `clone`, a clone into `target`, that the user cancelled.
    fn end_cancelled_clone(
        &self,
        target: &CloneTarget,
        clone: Invocation,
    ) -> Result<Project, Failure> {
        let checked_out = clone
            .last_hook
            .is_some_and(|hook| HOOKS_AFTER_CHECKOUT.contains(&hook.as_str()));
        // git goes on to the submodules after those hooks, so these did not get to all it had to do.
        let complete = checked_out && !target.has_submodules_to_clone();
        if !complete {
            return match target.found.restore(&target.folder) {
                Ok(()) => Err(Failure::OperationCancelled { left_behind: None }),
                Err(error) => Err(Failure::CancelledCloneNotRemoved {
                    folder: target.folder.clone(),
                    message: error.to_string(),
                }),
            };
        };
        let project = self.open_project(&target.folder)?;
        // Unless the cancel came just as the hook ended, which leaves nothing to tell.
        if let Some(hook) = clone.interrupted_hook {
            // A clone is the Repository at the root of the Project it opens as.
            let repository = Repository {
                root: project.folder.clone(),
            };
            self.notifications.raise(
                &project.folder,
                NotificationKind::HookNotFinished { repository, hook },
                vec![],
            );
        }
        Ok(project)
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

    /// Adds `paths`, relative to the root of `repository` and taken literally, to the git index.
    pub fn add_files(&self, repository: &Repository, paths: &[String]) -> Result<(), Failure> {
        self.add(repository, &["add"], paths)
    }

    /// Adds `paths`, relative to the root of `repository` and taken literally, to the git index,
    /// even when git ignores them. A folder git ignores as a whole is added with what it holds.
    pub fn force_add_files(
        &self,
        repository: &Repository,
        paths: &[String],
    ) -> Result<(), Failure> {
        self.add(repository, &["add", "--force"], paths)
    }

    /// Runs `command`, a form of `git add`, on `paths`.
    fn add(
        &self,
        repository: &Repository,
        command: &[&str],
        paths: &[String],
    ) -> Result<(), Failure> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut args = vec!["--literal-pathspecs"];
        args.extend(command);
        args.push("--");
        args.extend(paths.iter().map(String::as_str));
        self.run_add(repository, &args)
    }

    /// Stages every change of `repository`, new files included.
    pub fn stage_all(&self, repository: &Repository) -> Result<(), Failure> {
        self.run_add(repository, &["add", "--all"])
    }

    /// Stages every change to a file git tracks in `repository`, leaving new files out.
    pub fn stage_tracked(&self, repository: &Repository) -> Result<(), Failure> {
        self.run_add(repository, &["add", "--update"])
    }

    /// Runs `args`, the arguments after `git` of a command that adds to the index of `repository`.
    fn run_add(&self, repository: &Repository, args: &[&str]) -> Result<(), Failure> {
        let runner = self.runner(&repository.root)?;
        let operation = LongOperation::AddFiles {
            repository: repository.clone(),
        };
        let invocation = self.run_operation(&runner, operation, &repository.root, args)?;
        if !invocation.success {
            return Err(failure_of(
                &runner,
                &repository.root,
                &invocation,
                Failure::FilesNotAdded {
                    folder: repository.root.clone(),
                },
            ));
        }
        Ok(())
    }

    /// Takes what is staged at `paths`, relative to the root of `repository`, out of the index. The
    /// working tree stays as it is. A path with nothing staged is left alone, and unstaging a move
    /// unstages both of its paths.
    pub fn unstage_files(&self, repository: &Repository, paths: &[String]) -> Result<(), Failure> {
        let runner = self.runner(&repository.root)?;
        let status = read_status(&runner, repository, false)?;
        let staged = status.staged_paths(paths);
        if staged.is_empty() {
            return Ok(());
        }
        let root = &repository.root;
        let not_unstaged = || Failure::FilesNotUnstaged {
            folder: root.clone(),
        };
        let has_commits = commit::has_commits(&runner, root)?;
        let git_dir = runner
            .relative_git_dir(root)
            .map_err(git_failed_to_start)?
            .ok_or_else(not_unstaged)?;
        let selection = commit::Selection {
            unversioned: Vec::new(),
            committed: staged,
        };
        let input = commit::CommitInput::write(root, &git_dir, &selection, "")
            .map_err(|_| not_unstaged())?;
        let operation = LongOperation::UnstageFiles {
            repository: repository.clone(),
        };
        let command = input.unstage_command(has_commits);
        let invocation = self.run_operation(&runner, operation, root, &as_args(&command))?;
        if !invocation.success {
            return Err(failure_of(&runner, root, &invocation, not_unstaged()));
        }
        Ok(())
    }

    /// Reads the uncommitted changes of `repository`, sorted into its Changelists, which follow
    /// what the read finds. Reading again refreshes them.
    pub fn read_changes(&self, repository: &Repository) -> Result<ChangeSet, Failure> {
        self.read_changes_with_choice(repository, |_| Ok(()))
    }

    /// Makes the Changelist `id` of `repository` the Active changelist, the one new changes go into
    /// from here on, and returns the changes after that. A change made before this stays in the
    /// Active changelist of its time, even when gikli had not read it yet.
    pub fn set_active_changelist(
        &self,
        repository: &Repository,
        id: &str,
    ) -> Result<ChangeSet, Failure> {
        self.read_changes_with_choice(repository, |lists| lists.set_active(id))
    }

    /// Adds a Changelist called `name`, with an optional `description`, to `repository`, and returns
    /// the changes after that.
    pub fn create_changelist(
        &self,
        repository: &Repository,
        name: &str,
        description: Option<&str>,
    ) -> Result<ChangeSet, Failure> {
        self.read_changes_with_choice(repository, |lists| lists.create(name, description))
    }

    /// Calls the Changelist `id` of `repository` `name` from here on, and returns the changes after
    /// that.
    pub fn rename_changelist(
        &self,
        repository: &Repository,
        id: &str,
        name: &str,
    ) -> Result<ChangeSet, Failure> {
        self.read_changes_with_choice(repository, |lists| lists.rename(id, name))
    }

    /// Deletes the Changelist `id` of `repository`, whose changes go to the Active changelist, and
    /// returns the changes after that. The Active changelist cannot be deleted, so at least one
    /// Changelist always exists.
    pub fn delete_changelist(
        &self,
        repository: &Repository,
        id: &str,
    ) -> Result<ChangeSet, Failure> {
        self.read_changes_with_choice(repository, |lists| lists.delete(id))
    }

    /// Gives the Changelist `id` of `repository` the comment `comment`, stored with it, or takes it
    /// away when there is no text, and returns the changes after that.
    pub fn set_changelist_comment(
        &self,
        repository: &Repository,
        id: &str,
        comment: Option<&str>,
    ) -> Result<ChangeSet, Failure> {
        self.read_changes_with_choice(repository, |lists| lists.set_comment(id, comment))
    }

    /// Moves the changes at `paths`, relative to the root of `repository`, to `target`, which becomes
    /// the Active changelist if `make_active`, and returns the changes after that. It is one step:
    /// when any of it cannot be done, nothing changes.
    pub fn move_changes(
        &self,
        repository: &Repository,
        paths: &[String],
        target: &MoveTarget,
        make_active: bool,
    ) -> Result<ChangeSet, Failure> {
        self.read_changes_with_choice(repository, |lists| {
            lists.move_changes(paths, target, make_active)
        })
    }

    /// What rolling back the changes at `paths`, relative to the root of `repository`, would
    /// discard, for the user to confirm before anything goes. It changes nothing.
    pub fn rollback_plan(
        &self,
        repository: &Repository,
        paths: &[String],
    ) -> Result<RollbackPlan, Failure> {
        let runner = self.runner(&repository.root)?;
        let status = read_status(&runner, repository, true)?;
        Ok(rollback::plan(&status, paths, None))
    }

    /// What rolling back the changes at `paths`, relative to the root of `repository`, on `side` of
    /// the index would discard, for the user to confirm before anything goes. It changes nothing.
    pub fn staging_rollback_plan(
        &self,
        repository: &Repository,
        paths: &[String],
        side: StagingSide,
    ) -> Result<RollbackPlan, Failure> {
        let runner = self.runner(&repository.root)?;
        let status = read_status(&runner, repository, true)?;
        Ok(rollback::plan(&status, paths, Some(side)))
    }

    /// Discards the changes on `plan`, which the user confirmed, and returns the changes after that.
    /// An added file leaves git, and its local copy goes too if `delete_added_files`. When the plan
    /// no longer says what a rollback would discard, nothing goes and this returns
    /// [`Failure::RollbackListOutdated`].
    pub fn rollback_files(
        &self,
        repository: &Repository,
        plan: &RollbackPlan,
        delete_added_files: bool,
    ) -> Result<ChangeSet, Failure> {
        let runner = self.runner(&repository.root)?;
        let status = read_status(&runner, repository, true)?;
        let paths = plan.paths();
        if rollback::plan(&status, &paths, plan.side) != *plan {
            return Err(Failure::RollbackListOutdated);
        }
        let commands = rollback::commands(&status, &paths, plan.side, delete_added_files);
        if commands.is_empty() {
            return self.read_changes(repository);
        }
        let operation = LongOperation::RollbackFiles {
            repository: repository.clone(),
        };
        let failed = self
            .operations
            .run(operation, runner.runs_natively(), |run| {
                for command in &commands {
                    let invocation =
                        runner.run_operation(&repository.root, &as_args(command), run)?;
                    if !invocation.success {
                        return Ok(Some(invocation));
                    }
                }
                Ok(None)
            })
            .map_err(git_failed_to_start)?;
        if let Some(invocation) = failed {
            return Err(failure_of(
                &runner,
                &repository.root,
                &invocation,
                Failure::FilesNotRolledBack {
                    folder: repository.root.clone(),
                },
            ));
        }
        self.read_changes(repository)
    }

    /// Commits the changes at `paths`, relative to the root of `repository`, as they are in the
    /// working tree, with `message` as written, and returns the changes after that. Unversioned
    /// Files among them are added in the same step, and go back to being unversioned when the commit
    /// fails, unless a cancelled run left the lock of the index in the way. What is staged at any
    /// other path stays staged, and out of the commit. Hooks run as the user configured them.
    ///
    /// Without a `user.name` and a `user.email` this returns [`Failure::CommitIdentityMissing`], and
    /// nothing happens until [`Self::set_commit_identity`] has the answer.
    pub fn commit_files(
        &self,
        repository: &Repository,
        paths: &[String],
        message: &str,
    ) -> Result<ChangeSet, Failure> {
        self.commit_selection(None, repository, paths, message, commit::CommitKind::New)
    }

    /// Replaces the last commit of `repository` with one that also holds the changes at `paths`, as
    /// they are in the working tree, and has `message` as written, and returns the changes after
    /// that. It saves a Backup of the commit it replaces first, and raises a notification about the
    /// Project at `project` that offers to undo the amend. Everything else works as in
    /// [`Self::commit_files`].
    pub fn amend_last_commit(
        &self,
        project: &Path,
        repository: &Repository,
        paths: &[String],
        message: &str,
    ) -> Result<ChangeSet, Failure> {
        let kind = commit::CommitKind::Amend;
        self.commit_selection(Some(project), repository, paths, message, kind)
    }

    /// Commits what is staged in `repository`, with `message` as written, and returns the changes
    /// after that. What is not staged stays as it is, even in a file with another version staged.
    /// Hooks run as the user configured them. With nothing staged and no merge to conclude this
    /// returns [`Failure::NothingToCommit`], and without a `user.name` and a `user.email`
    /// [`Failure::CommitIdentityMissing`].
    pub fn commit_staged(
        &self,
        repository: &Repository,
        message: &str,
    ) -> Result<ChangeSet, Failure> {
        self.commit_index(None, repository, message, commit::CommitKind::New)
    }

    /// Replaces the last commit of `repository` with one that also holds what is staged, and has
    /// `message` as written, and returns the changes after that. It saves a Backup and raises a
    /// notification as [`Self::amend_last_commit`] does. Everything else works as in
    /// [`Self::commit_staged`].
    pub fn amend_with_staged(
        &self,
        project: &Path,
        repository: &Repository,
        message: &str,
    ) -> Result<ChangeSet, Failure> {
        self.commit_index(
            Some(project),
            repository,
            message,
            commit::CommitKind::Amend,
        )
    }

    /// Commits the changes at `paths`. An amend raises its notification about the Project at
    /// `project`.
    fn commit_selection(
        &self,
        project: Option<&Path>,
        repository: &Repository,
        paths: &[String],
        message: &str,
        kind: commit::CommitKind,
    ) -> Result<ChangeSet, Failure> {
        let runner = self.commit_runner(repository, message, kind)?;
        let status = read_status(&runner, repository, true)?;
        let selection = commit::Selection::of(&status, paths);
        if selection.committed.is_empty() {
            return Err(Failure::NothingToCommit);
        }
        let commit = commit::Commit {
            selection: &selection,
            message,
            scope: commit::CommitScope::Selection,
            kind,
        };
        self.run_commit(&runner, project, repository, commit)
    }

    /// Commits what is staged. An amend raises its notification about the Project at `project`.
    fn commit_index(
        &self,
        project: Option<&Path>,
        repository: &Repository,
        message: &str,
        kind: commit::CommitKind,
    ) -> Result<ChangeSet, Failure> {
        let runner = self.commit_runner(repository, message, kind)?;
        let git_dir = runner
            .git_dir(&repository.root)
            .map_err(git_failed_to_start)?
            .ok_or_else(|| Failure::ChangesUnreadable {
                folder: repository.root.clone(),
            })?;
        let status = read_status(&runner, repository, false)?;
        // A merge resolved to what HEAD already has stages nothing, and still has to be concluded.
        if status.staged.is_empty() && !in_progress::is_merging(&git_dir) {
            return Err(Failure::NothingToCommit);
        }
        let selection = commit::Selection::default();
        let commit = commit::Commit {
            selection: &selection,
            message,
            scope: commit::CommitScope::Index,
            kind,
        };
        self.run_commit(&runner, project, repository, commit)
    }

    /// The runner for a commit in `repository` with `message`, once what can be told before reading
    /// the changes says it can go ahead.
    fn commit_runner(
        &self,
        repository: &Repository,
        message: &str,
        kind: commit::CommitKind,
    ) -> Result<GitRunner, Failure> {
        if message.trim().is_empty() {
            return Err(Failure::CommitMessageEmpty);
        }
        let runner = self.runner(&repository.root)?;
        if kind == commit::CommitKind::Amend && !commit::has_commits(&runner, &repository.root)? {
            return Err(Failure::NothingToAmend);
        }
        Ok(runner)
    }

    /// Makes `commit`, once it is known to have something to record. An amend saves a Backup of the
    /// commit it replaces first, and once it went through, raises a notification about the Project
    /// at `project` that offers to undo it.
    fn run_commit(
        &self,
        runner: &GitRunner,
        project: Option<&Path>,
        repository: &Repository,
        commit: commit::Commit<'_>,
    ) -> Result<ChangeSet, Failure> {
        let commit::Commit {
            selection,
            message,
            scope,
            kind,
        } = commit;
        commit::ensure_identity(runner, &repository.root)?;
        let operation = LongOperation::CommitFiles {
            repository: repository.clone(),
        };
        let root = &repository.root;
        let git_dir = runner
            .relative_git_dir(root)
            .map_err(git_failed_to_start)?
            .ok_or_else(|| Failure::ChangesUnreadable {
                folder: root.clone(),
            })?;
        let input =
            commit::CommitInput::write(root, &git_dir, selection, message).map_err(|error| {
                Failure::CommitInputNotWritten {
                    folder: root.clone(),
                    message: error.to_string(),
                }
            })?;
        let backup = match kind {
            commit::CommitKind::Amend => Some(self.back_up_head(runner, repository)?),
            commit::CommitKind::New => None,
        };
        let failed = self
            .operations
            .run(operation, runner.runs_natively(), |run| {
                let git = |command: &[String]| runner.run_operation(root, &as_args(command), run);
                if let Some(add) = input.add_command() {
                    let added = git(&add)?;
                    if !added.success {
                        return Ok(Some(added));
                    }
                }
                let committed = git(&input.commit_command(scope, kind))?;
                if committed.success {
                    return Ok(None);
                }
                if let Some(forget) = input.forget_added_command() {
                    // Not as part of the operation: a cancelled one would end this at once. When
                    // it fails, the files stay added, and the operation log says why.
                    let _ = runner.run(root, &as_args(&forget));
                }
                Ok(Some(committed))
            })
            .map_err(git_failed_to_start);
        let failure = match failed {
            Ok(None) => None,
            Ok(Some(invocation)) => Some(failure_of(
                runner,
                root,
                &invocation,
                Failure::CommitFailed {
                    folder: root.clone(),
                },
            )),
            Err(failure) => Some(failure),
        };
        if let Some(failure) = failure {
            if let Some(backup) = &backup {
                // Nothing was rewritten, so there is nothing to undo.
                backups::forget(runner, repository, backup);
            }
            return Err(failure);
        }
        if let (Some(backup), Some(project)) = (backup, project) {
            self.raise_history_rewritten(runner, project, repository, Rewrite::Amend, backup)?;
        }
        self.read_changes(repository)
    }

    /// Saves a Backup of the commit HEAD of `repository` is on, with the branch it is on, if any.
    fn back_up_head(&self, runner: &GitRunner, repository: &Repository) -> Result<Backup, Failure> {
        let branch = sync::head_branch(runner, repository)?;
        let head = details::resolve(runner, repository, "HEAD")?;
        backups::save(runner, repository, branch.as_deref(), &head)
    }

    /// Raises a notification about the Project at `project` that tells of `rewrite`, the history
    /// rewrite in `repository` that `backup` was saved for, and offers to undo it.
    fn raise_history_rewritten(
        &self,
        runner: &GitRunner,
        project: &Path,
        repository: &Repository,
        rewrite: Rewrite,
        backup: Backup,
    ) -> Result<(), Failure> {
        let head = details::commit_if_any(runner, repository, "HEAD")?;
        self.notifications.raise(
            project,
            NotificationKind::HistoryRewritten {
                repository: repository.clone(),
                rewrite,
                backup,
                head,
            },
            vec![NotificationAction::RestoreBackup],
        );
        Ok(())
    }

    /// Undoes `commit`, the last commit of the current branch of `repository`: the branch goes
    /// back to the commit before it, and its changes stay, uncommitted, in the Active changelist.
    /// Returns the changes after that.
    pub fn undo_commit(
        &self,
        project: &Path,
        repository: &Repository,
        commit: &str,
    ) -> Result<ChangeSet, Failure> {
        let runner = self.runner(&repository.root)?;
        let commit = details::resolve(&runner, repository, commit)?;
        let branch = sync::head_branch(&runner, repository)?;
        let head = details::commit_if_any(&runner, repository, "HEAD")?;
        let Some(branch) = branch.filter(|_| head.as_ref() == Some(&commit)) else {
            return Err(Failure::NotLastCommit { commit });
        };
        if let Some(branch) = undo_commit::protected_branch_holding(&runner, repository, &commit)? {
            return Err(Failure::CommitPushedToProtectedBranch { commit, branch });
        }
        let backup = backups::save(&runner, repository, Some(&branch), &commit)?;
        if let Err(failure) = undo_commit::undo(&runner, repository, &branch, &commit) {
            backups::forget(&runner, repository, &backup);
            return Err(failure);
        }
        self.raise_history_rewritten(&runner, project, repository, Rewrite::UndoCommit, backup)?;
        self.read_changes(repository)
    }

    /// The messages the user can reuse for a commit in `repository`, newest first.
    pub fn previous_commit_messages(
        &self,
        repository: &Repository,
    ) -> Result<Vec<String>, Failure> {
        commit::previous_messages(&self.runner(&repository.root)?, &repository.root)
    }

    /// The message of the last commit of `repository`, which an amend starts from. None when it has
    /// no commit yet.
    pub fn last_commit_message(&self, repository: &Repository) -> Result<Option<String>, Failure> {
        commit::last_message(&self.runner(&repository.root)?, &repository.root)
    }

    /// What the message of a commit of the changes at `paths`, relative to the root of `repository`,
    /// starts as: the comment of the Changelist they are in, when they are all in one that has a
    /// comment, or else the text of `commit.template`, as it is in its file.
    pub fn prefilled_commit_message(
        &self,
        repository: &Repository,
        paths: &[String],
    ) -> Result<Option<String>, Failure> {
        let changes = self.read_changes(repository)?;
        if let Some(comment) = commit::comment_of_selection(&changes, paths) {
            return Ok(Some(comment));
        }
        commit::template(&self.runner(&repository.root)?, &repository.root)
    }

    /// What the user should hear before committing in `repository`, when it is somewhere a commit
    /// may not be meant to go and the setting for this warning is on.
    pub fn commit_warning(
        &self,
        repository: &Repository,
    ) -> Result<Option<CommitWarning>, Failure> {
        if !self.settings().warn_on_unusual_commit_state {
            return Ok(None);
        }
        commit::warning(&self.runner(&repository.root)?, &repository.root)
    }

    /// Saves the answer to [`Failure::CommitIdentityMissing`] in git's configuration: for every
    /// Repository of the user if `everywhere`, and for `repository` alone otherwise, without the
    /// spaces around it. A value that is None stays as it is. A value without text saves nothing.
    pub fn set_commit_identity(
        &self,
        repository: &Repository,
        name: Option<&str>,
        email: Option<&str>,
        everywhere: bool,
    ) -> Result<(), Failure> {
        let answers = [("user.name", name), ("user.email", email)];
        let answers = answers.map(|(key, value)| (key, value.map(str::trim)));
        if answers.iter().any(|(_, value)| *value == Some("")) {
            return Err(Failure::CommitIdentityEmpty);
        }
        let runner = self.runner(&repository.root)?;
        let scope = if everywhere { "--global" } else { "--local" };
        for (key, value) in answers {
            let Some(value) = value else { continue };
            let invocation = runner
                .run(&repository.root, &["config", scope, "--", key, value])
                .map_err(git_failed_to_start)?;
            if !invocation.success {
                return Err(failure_of(
                    &runner,
                    &repository.root,
                    &invocation,
                    Failure::CommitIdentityNotSaved {
                        folder: repository.root.clone(),
                    },
                ));
            }
        }
        Ok(())
    }

    /// Reads the changes of `repository`, makes its Changelists follow them, and applies `choice`,
    /// something the user chose. When a choice cannot be saved, this fails and nothing changes.
    /// What the read found alone is worked out again on the next read, so it does not stop the user
    /// seeing their changes when it cannot be saved.
    fn read_changes_with_choice(
        &self,
        repository: &Repository,
        choice: impl FnOnce(&mut Changelists) -> Result<(), Failure>,
    ) -> Result<ChangeSet, Failure> {
        let runner = self.runner(&repository.root)?;
        let settings = self.settings();
        let status = read_status(&runner, repository, settings.show_ignored_files)?;
        let git_dir = runner
            .git_dir(&repository.root)
            .map_err(git_failed_to_start)?
            .ok_or_else(|| Failure::ChangesUnreadable {
                folder: repository.root.clone(),
            })?;
        let store = ChangelistStore::in_git_dir(&git_dir);
        let lists = {
            let _only_one_at_a_time = lock(&self.changelists);
            let found = store.load()?;
            let mut followed = found.clone();
            followed.follow(&status, &runner, &repository.root);
            let mut chosen = followed.clone();
            choice(&mut chosen)?;
            if chosen != found {
                let saved = store.save(&chosen);
                if chosen != followed {
                    saved.map_err(|error| store.not_saved(&error))?;
                }
            }
            chosen
        };
        let staging = settings.staging_mode.then(|| {
            StagingArea::new(
                status.staged,
                status.unstaged,
                in_progress::is_merging(&git_dir),
                settings.change_grouping,
            )
        });
        Ok(ChangeSet::new(
            repository.clone(),
            status.changes,
            &lists,
            staging,
            settings.change_grouping,
        ))
    }

    /// The diff of the file at `path`, relative to the root of `repository`, against HEAD.
    pub fn read_diff(&self, repository: &Repository, path: &str) -> Result<FileDiff, Failure> {
        let changes = self.read_changes(repository)?;
        diff::read(
            &self.runner(&repository.root)?,
            &changes,
            path,
            &self.settings(),
        )
    }

    /// The diff of the file at `path`, relative to the root of `repository`, on `side` of the index:
    /// the index against HEAD for [`StagingSide::Staged`], and the file on disk against the index
    /// for [`StagingSide::Unstaged`].
    pub fn read_staging_diff(
        &self,
        repository: &Repository,
        path: &str,
        side: StagingSide,
    ) -> Result<FileDiff, Failure> {
        let changes = self.read_changes(repository)?;
        diff::read_side(
            &self.runner(&repository.root)?,
            &changes,
            path,
            side,
            &self.settings(),
        )
    }

    /// Saves `text` as the file at `path`, as [`Self::save_local_file`] does, and returns its diff
    /// against the index after that. The index stays as it is.
    pub fn save_unstaged_file(
        &self,
        repository: &Repository,
        path: &str,
        text: &str,
        version: &str,
    ) -> Result<FileDiff, Failure> {
        let editable = self.settings().diff_local_side_editable;
        diff::save(&repository.root, path, text, version, editable)?;
        self.read_staging_diff(repository, path, StagingSide::Unstaged)
    }

    /// Saves `text`, with `\n` between lines, as the local side of the diff of the file at `path`,
    /// and returns the diff after that. The lines keep the line endings they have in the file.
    ///
    /// `version` is the [`TextDiff::local_version`] of the diff the user edited. When the file no
    /// longer is what they saw, nothing is written and this returns [`Failure::LocalFileChanged`]:
    /// neither version gets lost without them knowing.
    pub fn save_local_file(
        &self,
        repository: &Repository,
        path: &str,
        text: &str,
        version: &str,
    ) -> Result<FileDiff, Failure> {
        let editable = self.settings().diff_local_side_editable;
        diff::save(&repository.root, path, text, version, editable)?;
        self.read_diff(repository, path)
    }

    /// A page of the log of `repository`: its commits on every branch, remote branch and tag and
    /// on HEAD, newest commit date first, at most `count` to a page, each with its row of the graph.
    /// The first page comes with no `after`, and each later one with the [`LogPage::next`] of the
    /// page before it. A Repository with no commits has an empty page. Unless the user shows long
    /// edges, an edge that spans more than [`LONG_EDGE`] rows is drawn as two arrows that lead to
    /// each other; later pages keep to what the first one did.
    ///
    /// When the history changed since the page before, this returns [`Failure::LogChanged`], and
    /// the log has to start over from the first page.
    pub fn read_log(
        &self,
        repository: &Repository,
        after: Option<&LogCursor>,
        count: usize,
    ) -> Result<LogPage, Failure> {
        self.read_filtered_log(repository, &LogFilter::default(), after, count)
    }

    /// A page of the log of `repository` as [`Self::read_log`] reads it, with only the commits
    /// `filter` shows. A later page keeps to the filter of the first. Filtered by branch, the log
    /// shows the commits those branches hold, with their graph, and a branch that no longer exists
    /// shows nothing. A search shows the commits whose message, author or hash match its text,
    /// without their graph, and the first page of one puts it first in
    /// [`Self::log_search_history`].
    pub fn read_filtered_log(
        &self,
        repository: &Repository,
        filter: &LogFilter,
        after: Option<&LogCursor>,
        count: usize,
    ) -> Result<LogPage, Failure> {
        let show_long_edges = self.settings().show_long_edges;
        let runner = self.runner(&repository.root)?;
        let page = log::read(
            &runner,
            &self.log_reads,
            repository,
            filter,
            after,
            count,
            show_long_edges,
        )?;
        if after.is_none()
            && let Some(search) = &filter.text
        {
            self.log_searches.remember(search);
        }
        Ok(page)
    }

    /// The searches of the log the user ran, newest first, each once, the last 20 of them, each
    /// with its regex and match-case options. The same text with other options is another search.
    /// They are kept from one start of gikli to the next.
    pub fn log_search_history(&self) -> Vec<TextSearch> {
        self.log_searches.searches()
    }

    /// Fetches the rest of the history of `repository`, whose history is cut, as in a shallow clone,
    /// so that the log shows it whole, as [`LogPage::history_cut`] offers. It fetches from the
    /// remote the current branch fetches from, or `origin`, or the first remote git lists. A remote
    /// that tracks single branches, as `git clone --depth` leaves it, first tracks every branch, as
    /// `git remote set-branches <remote> '*'` does, so every remote branch comes and later fetches
    /// keep them up to date; one that names its branches with a pattern keeps its configuration.
    /// The fetch runs as a [`LongOperation`] that reports its progress and that the user can
    /// cancel.
    pub fn fetch_rest_of_history(&self, repository: &Repository) -> Result<(), Failure> {
        let runner = self.runner(&repository.root)?;
        let root = &repository.root;
        let remote = shallow::remote_to_fetch_from(&runner, repository)?.ok_or_else(|| {
            Failure::NoRemoteToFetchFrom {
                folder: root.clone(),
            }
        })?;
        let operation = LongOperation::FetchRestOfHistory {
            repository: repository.clone(),
            remote: remote.clone(),
        };
        let fallback = Failure::HistoryNotFetched {
            folder: root.clone(),
            remote: remote.clone(),
        };
        if let Some(failed) = shallow::track_every_branch(&runner, repository, &remote)? {
            return Err(failure_of(&runner, root, &failed, fallback));
        }
        let args = shallow::fetch_rest_args(&remote);
        let invocation = self.run_operation(&runner, operation, root, &args)?;
        if invocation.success {
            return Ok(());
        }
        Err(failure_of_contacting_remotes(
            &runner,
            root,
            &invocation,
            &[remote],
            fallback,
        ))
    }

    /// Fetches every remote of `repository`, and prunes the remote branches that are gone from
    /// them. The fetch runs as a [`LongOperation`] that reports its progress and that the user can
    /// cancel. A Repository without a remote returns [`Failure::NoRemoteToFetchFrom`].
    pub fn fetch_all(&self, repository: &Repository) -> Result<(), Failure> {
        let runner = self.runner(&repository.root)?;
        let remotes = sync::remotes(&runner, repository)?;
        if remotes.is_empty() {
            return Err(Failure::NoRemoteToFetchFrom {
                folder: repository.root.clone(),
            });
        }
        let operation = LongOperation::FetchAll {
            repository: repository.clone(),
        };
        self.fetch(
            &runner,
            repository,
            operation,
            &sync::fetch_all_args(&remotes),
            &remotes,
        )
    }

    /// Fetches `branch` of `repository` alone: a local branch from the remote branch its upstream
    /// is, a remote branch from its remote. Only the remote branch it updates changes. The fetch
    /// runs as a [`LongOperation`] that reports its progress and that the user can cancel. A local
    /// branch without an upstream on a remote returns [`Failure::NoUpstream`], and a remote branch
    /// no remote fetches into [`Failure::NoRemoteToFetchFrom`].
    pub fn fetch_branch(&self, repository: &Repository, branch: &RefLabel) -> Result<(), Failure> {
        let runner = self.runner(&repository.root)?;
        let source = sync::branch_source(&runner, repository, branch)?;
        let operation = LongOperation::FetchBranch {
            repository: repository.clone(),
            remote: source.remote.clone(),
            branch: branch.name.clone(),
        };
        let remotes = [source.remote.clone()];
        self.fetch(
            &runner,
            repository,
            operation,
            &source.fetch_args(),
            &remotes,
        )
    }

    /// Runs git with `args`, a fetch from `remotes`, in `repository` as `operation`, and explains
    /// its failure.
    fn fetch(
        &self,
        runner: &GitRunner,
        repository: &Repository,
        operation: LongOperation,
        args: &[&str],
        remotes: &[String],
    ) -> Result<(), Failure> {
        let root = &repository.root;
        let invocation = self.run_operation(runner, operation, root, args)?;
        if invocation.success {
            return Ok(());
        }
        let fallback = Failure::FetchFailed {
            folder: root.clone(),
        };
        Err(failure_of_contacting_remotes(
            runner,
            root,
            &invocation,
            remotes,
            fallback,
        ))
    }

    /// What the push dialog shows for `repository`: its current branch, which is the one pushed,
    /// where it goes and the commits it would send there. The target is `target` when the user
    /// chose one, and otherwise the upstream of the branch when that is on a remote.
    pub fn push_plan(
        &self,
        repository: &Repository,
        target: Option<&PushTarget>,
    ) -> Result<PushPlan, Failure> {
        push::plan(&self.runner(&repository.root)?, repository, target)
    }

    /// Pushes the current branch of `repository` to `target`, with the tags `tags` asks for. The
    /// push runs as a [`LongOperation`] that reports its progress and that the user can cancel.
    /// When `target` is the upstream of the branch and rejects the push, as it has incoming
    /// commits, the push returns [`Failure::PushRejected`], so the user chooses how to
    /// integrate them, unless [`Settings::auto_update_if_push_rejected`] is on: then it integrates
    /// them with [`Settings::push_rejected_update_method`] and pushes again, as
    /// [`Gikli::update_and_push`] does.
    pub fn push(
        &self,
        repository: &Repository,
        target: &PushTarget,
        tags: PushTags,
    ) -> Result<(), Failure> {
        match self.push_once(repository, target, tags) {
            Err(rejected @ Failure::PushRejected { .. }) => {
                let settings = self.settings();
                if !settings.auto_update_if_push_rejected {
                    return Err(rejected);
                }
                let method = settings.push_rejected_update_method;
                self.integrate_upstream_then_push(repository, target, tags, method)
            }
            pushed => pushed,
        }
    }

    /// Integrates `target`, the upstream of the current branch of `repository` that rejected its
    /// push, into the branch with `method`, and pushes the branch there again with the tags `tags`
    /// asks for, as the user chose after [`Failure::PushRejected`]. With `remember`, a rejected
    /// push integrates with `method` without asking from then on, as
    /// [`Settings::auto_update_if_push_rejected`] says. The update runs as a pull, a
    /// [`LongOperation`] of its own. An update that stops on a conflict says so with
    /// [`Event::ConflictsLeft`] and returns [`Failure::UpdateStoppedOnConflicts`], with nothing
    /// pushed.
    pub fn update_and_push(
        &self,
        repository: &Repository,
        target: &PushTarget,
        tags: PushTags,
        method: UpdateMethod,
        remember: bool,
    ) -> Result<(), Failure> {
        if remember {
            self.change_settings(|settings| {
                settings.auto_update_if_push_rejected = true;
                settings.push_rejected_update_method = method;
            })?;
        }
        self.integrate_upstream_then_push(repository, target, tags, method)
    }

    /// Integrates `target` into the current branch of `repository` with `method` and pushes the
    /// branch there again, once.
    fn integrate_upstream_then_push(
        &self,
        repository: &Repository,
        target: &PushTarget,
        tags: PushTags,
        method: UpdateMethod,
    ) -> Result<(), Failure> {
        let runner = self.runner(&repository.root)?.accepting_prepared_messages();
        push::current_branch(&runner, repository)?;
        push::check(
            &runner,
            repository,
            target,
            &sync::remotes(&runner, repository)?,
        )?;
        let source = PullSource {
            remote: target.remote.clone(),
            branch: target.branch.clone(),
        };
        let options = PullOptions {
            rebase: method == UpdateMethod::Rebase,
            ..PullOptions::default()
        };
        if self.pull_into(&runner, repository, &source, options)? {
            return Err(Failure::UpdateStoppedOnConflicts {
                folder: repository.root.clone(),
            });
        }
        self.push_once(repository, target, tags)
    }

    /// Pushes the current branch of `repository` to `target` once, as [`Gikli::push`] does
    /// without updating.
    fn push_once(
        &self,
        repository: &Repository,
        target: &PushTarget,
        tags: PushTags,
    ) -> Result<(), Failure> {
        let runner = self.runner(&repository.root)?;
        let root = &repository.root;
        let branch = push::current_branch(&runner, repository)?;
        push::check(
            &runner,
            repository,
            target,
            &sync::remotes(&runner, repository)?,
        )?;
        let operation = LongOperation::Push {
            repository: repository.clone(),
            remote: target.remote.clone(),
            branch: target.branch.clone(),
        };
        let set_upstream = push::sets_upstream(&runner, repository, &branch, target)?;
        let args = push::push_args(&branch, target, tags, set_upstream);
        let invocation = self.run_operation(&runner, operation, root, &as_args(&args))?;
        // What git says of the branch, not only how it ended, tells whether it went. A cancelled
        // push is cancelled, whatever git wrote before it ended.
        let rejected = !invocation.cancelled
            && push::rejected_for_commits_it_lacks(&invocation.stdout, &branch, target);
        if invocation.success && !rejected {
            return Ok(());
        }
        if rejected && push::upstream(&runner, repository, &branch)?.as_ref() == Some(target) {
            return Err(Failure::PushRejected {
                folder: root.clone(),
                remote: target.remote.clone(),
                branch: target.branch.clone(),
            });
        }
        Err(failure_of_contacting_remotes(
            &runner,
            root,
            &invocation,
            std::slice::from_ref(&target.remote),
            Failure::PushFailed {
                folder: root.clone(),
            },
        ))
    }

    /// What follows a commit the user made with Commit and push in `repository`, once it went
    /// through. The push dialog opens, with what a push of the current branch would send, unless
    /// the settings suppress it: then the branch goes at once, without tags, to the target the
    /// dialog would start from. The dialog is suppressed when it is off for Commit and push, or
    /// when it shows only for protected branches and the target is not one. A Repository without
    /// a remote has no target, so the dialog opens to offer to define one.
    pub fn push_after_commit(&self, repository: &Repository) -> Result<PushAfterCommit, Failure> {
        let plan = self.push_plan(repository, None)?;
        let settings = self.settings();
        match plan.target {
            Some(target) if !settings.shows_push_dialog_after_commit_for(&target) => {
                self.push(repository, &target, PushTags::None)?;
                Ok(PushAfterCommit::Pushed { target })
            }
            _ => Ok(PushAfterCommit::Dialog { plan }),
        }
    }

    /// What the pull dialog shows for `repository`: its current branch, which the pull integrates
    /// into, its remotes with the branches each had when it was last fetched, and the remote and
    /// branch the dialog starts from.
    pub fn pull_plan(&self, repository: &Repository) -> Result<PullPlan, Failure> {
        pull::plan(&self.runner(&repository.root)?, repository)
    }

    /// Pulls `source` into the current branch of `repository`, a Repository of the Project at
    /// `project`, integrating it as `options` say, and returns the operation in progress after
    /// that. A pull that stops on a conflict says so with [`Event::ConflictsLeft`], so the
    /// conflicts dialog opens, and a rebase or a merge that stops is in progress. The pull runs as
    /// a [`LongOperation`] that reports its progress and that the user can cancel. A detached HEAD
    /// returns [`Failure::DetachedHeadNotPulled`].
    pub fn pull(
        &self,
        project: &Path,
        repository: &Repository,
        source: &PullSource,
        options: PullOptions,
    ) -> Result<Option<OperationInProgress>, Failure> {
        let runner = self.runner(&repository.root)?.accepting_prepared_messages();
        let branch = pull::current_branch(&runner, repository)?;
        options.check()?;
        push::check_remote_branch(
            &runner,
            repository,
            &source.remote,
            &source.branch,
            &sync::remotes(&runner, repository)?,
        )?;
        self.pull_into(&runner, repository, source, options)?;
        // Only a source the pull got to integrate is remembered, so a mistyped branch is not.
        pull::remember(&runner, repository, &branch, source)?;
        self.operation_in_progress(project, repository)
    }

    /// Merges `branch` into the current branch of `repository`, a Repository of the Project at
    /// `project`, as Merge into current does from the branch selector, and returns the operation in
    /// progress after that. git writes the message of the merge commit as it would on its own. A
    /// merge that stops on a conflict says so with [`Event::ConflictsLeft`], so the conflicts
    /// dialog opens, and is in progress. The merge runs as a [`LongOperation`] that reports the
    /// hook it waits on and that the user can cancel. A branch that no longer exists returns
    /// [`Failure::RevisionNotFound`] with its name.
    pub fn merge_into_current(
        &self,
        project: &Path,
        repository: &Repository,
        branch: &RefLabel,
    ) -> Result<Option<OperationInProgress>, Failure> {
        let runner = self.runner(&repository.root)?.accepting_prepared_messages();
        let planned = merge::plan(&runner, repository, branch)?;
        self.run_merge(&runner, repository, branch, &planned)?;
        self.operation_in_progress(project, repository)
    }

    /// Merges `branch` into the current branch of `repository`, a Repository of the Project at
    /// `project`, once the local changes that blocked it, as [`Failure::MergeBlocked`] told, are put
    /// aside as Saved local changes, and returns the operation in progress after that, as
    /// [`Self::merge_into_current`] does.
    ///
    /// The changes come back in their Changelists, as [`Self::save_local_changes`] and
    /// [`Self::restore_local_changes`] do, once the merge is over, also when it fails. A merge
    /// that stops on a conflict keeps them until the user aborts it or commits it with
    /// [`Self::take_operation_action`]. Files git does not track are not saved, so those that
    /// block the merge still do.
    pub fn merge_over_local_changes(
        &self,
        project: &Path,
        repository: &Repository,
        branch: &RefLabel,
    ) -> Result<Option<OperationInProgress>, Failure> {
        let runner = self.runner(&repository.root)?.accepting_prepared_messages();
        let planned = merge::plan(&runner, repository, branch)?;
        let store = AwaitingMergeStore::in_git_dir(&in_progress::git_dir(&runner, repository)?);
        let saved = self.save_local_changes(repository)?;
        let awaiting = AwaitingMerge {
            merging: planned.commit.clone(),
            saved,
        };
        // What waits is kept before the merge runs, so a merge that stops finds it there, and
        // nothing merges when it cannot be kept.
        if awaiting.saved.stash.is_some()
            && let Err(failure) = store.keep(&awaiting)
        {
            self.restore_local_changes(project, &awaiting.saved)?;
            return Err(failure);
        }
        let merged = self.run_merge(&runner, repository, branch, &planned);
        if !matches!(merged, Ok(true)) {
            store.forget();
            self.restore_local_changes(project, &awaiting.saved)?;
        }
        merged?;
        self.operation_in_progress(project, repository)
    }

    /// Runs `planned`, the merge of `branch` into the current branch of `repository`, through
    /// `runner`, which takes the messages git prepared as they are, and returns whether git stopped
    /// on a conflict, which it says with [`Event::ConflictsLeft`]. Any other failure of git's is
    /// the one git's output explains, or [`Failure::MergeFailed`].
    fn run_merge(
        &self,
        runner: &GitRunner,
        repository: &Repository,
        branch: &RefLabel,
        planned: &merge::Planned,
    ) -> Result<bool, Failure> {
        let root = &repository.root;
        let operation = LongOperation::Merge {
            repository: repository.clone(),
            branch: branch.name.clone(),
        };
        let (invocation, stopped_on_conflict) =
            self.run_stopping_on_conflicts(runner, repository, operation, &as_args(&planned.args))?;
        if !invocation.success && !stopped_on_conflict {
            // What blocks it cannot be told when the Repository cannot be read, which says why in
            // the operation log: the merge failure stays the one to tell.
            let otherwise = merge::blocked(runner, repository, planned)
                .ok()
                .flatten()
                .unwrap_or(Failure::MergeFailed {
                    folder: root.clone(),
                });
            return Err(failure_of(runner, root, &invocation, otherwise));
        }
        Ok(stopped_on_conflict)
    }

    /// Pulls `source` into the current branch of `repository` with `options`, through `runner`,
    /// which takes the messages git prepared as they are, and returns whether git stopped on a
    /// conflict, which it says with [`Event::ConflictsLeft`]. Any other failure of git's is
    /// [`Failure::PullFailed`], unless it is one of contacting the remote.
    fn pull_into(
        &self,
        runner: &GitRunner,
        repository: &Repository,
        source: &PullSource,
        options: PullOptions,
    ) -> Result<bool, Failure> {
        let root = &repository.root;
        let operation = LongOperation::Pull {
            repository: repository.clone(),
            remote: source.remote.clone(),
            branch: source.branch.clone(),
        };
        let args = pull::pull_args(source, options);
        // A merge or a rebase that stops on a conflict waits for the user to resolve it, and so
        // does a squash, with no operation to go on with.
        let (invocation, stopped_on_conflict) =
            self.run_stopping_on_conflicts(runner, repository, operation, &as_args(&args))?;
        if !invocation.success && !stopped_on_conflict {
            return Err(failure_of_contacting_remotes(
                runner,
                root,
                &invocation,
                std::slice::from_ref(&source.remote),
                Failure::PullFailed {
                    folder: root.clone(),
                },
            ));
        }
        Ok(stopped_on_conflict)
    }

    /// Defines the remote `name` of `repository` at `url`, as the push dialog offers when there is
    /// none, and fetches it at once, as a [`LongOperation`] that reports its progress and that the
    /// user can cancel. A remote that cannot be fetched is not kept, so the user can correct its
    /// URL and define it again. A name another remote has returns [`Failure::RemoteNameTaken`],
    /// one git does not take for a remote [`Failure::RemoteNameInvalid`], and an empty URL
    /// [`Failure::RemoteUrlEmpty`].
    pub fn add_remote(
        &self,
        repository: &Repository,
        name: &str,
        url: &str,
    ) -> Result<(), Failure> {
        let runner = self.runner(&repository.root)?;
        let root = &repository.root;
        let url = url.trim();
        if sync::remotes(&runner, repository)?
            .iter()
            .any(|remote| remote == name)
        {
            return Err(Failure::RemoteNameTaken {
                name: name.to_owned(),
            });
        }
        // git takes a name for a remote when its remote branches would have valid names.
        let remote_branches = format!("refs/remotes/{name}/branch");
        if !push::is_reference_name(&runner, repository, &remote_branches)? {
            return Err(Failure::RemoteNameInvalid {
                name: name.to_owned(),
            });
        }
        if url.is_empty() {
            return Err(Failure::RemoteUrlEmpty);
        }
        let added = runner
            .run(root, &["remote", "add", "--", name, url])
            .map_err(git_failed_to_start)?;
        if !added.success {
            return Err(failure_of(
                &runner,
                root,
                &added,
                Failure::RemoteNotAdded {
                    folder: root.clone(),
                    name: name.to_owned(),
                },
            ));
        }
        let operation = LongOperation::FetchRemote {
            repository: repository.clone(),
            remote: name.to_owned(),
        };
        let remotes = [name.to_owned()];
        let fetched = self.fetch(
            &runner,
            repository,
            operation,
            &["fetch", "--progress", "--end-of-options", name],
            &remotes,
        );
        if fetched.is_err() {
            // Once the failure is explained, which reads the remote's URL. When this fails, the
            // remote stays, and the operation log says why. git before 2.38 takes the name of
            // `remote remove` as it is, so `--` would be the name, and later git takes a name that
            // starts with `-` for an option unless `--` comes first.
            let removed = runner.run(root, &["remote", "remove", name]);
            if name.starts_with('-') && !removed.is_ok_and(|removed| removed.success) {
                let _ = runner.run(root, &["remote", "remove", "--", name]);
            }
        }
        fetched
    }

    /// The local branches of `repository`, by name, each with how far it is from its upstream.
    pub fn local_branches(&self, repository: &Repository) -> Result<Vec<LocalBranch>, Failure> {
        sync::local_branches(&self.runner(&repository.root)?, repository)
    }

    /// What HEAD of `repository` is on, as the status area shows it at all times: the current
    /// branch with how far it is from its upstream, or the commit of a detached HEAD.
    pub fn current_branch(&self, repository: &Repository) -> Result<CurrentBranch, Failure> {
        sync::current_branch(&self.runner(&repository.root)?, repository)
    }

    /// The branches of `repository` as the branch selector lists them: what HEAD is on, then the
    /// local branches, the remote branches and the tags.
    pub fn branches(&self, repository: &Repository, search: &str) -> Result<Branches, Failure> {
        let settings = self.settings();
        branches::list(
            &self.runner(&repository.root)?,
            repository,
            &settings,
            search,
        )
    }

    /// `name`, as the user types it for a new branch, with each character git takes in no branch
    /// name replaced by `-`, so that it keeps its length. What else git rejects is left for
    /// [`Self::create_branch`] to reject.
    pub fn clean_branch_name(&self, name: &str) -> String {
        new_branch::clean_name(name)
    }

    /// The prefixes the name field of a new branch in `repository` suggests once the user has typed
    /// `typed`: the directories of its local and remote branches, such as `feature/`, that start
    /// with it, ignoring case, by name.
    pub fn new_branch_prefixes(
        &self,
        repository: &Repository,
        typed: &str,
    ) -> Result<Vec<String>, Failure> {
        new_branch::prefixes(&self.runner(&repository.root)?, repository, typed)
    }

    /// Creates the branch `name` of `repository` at the commit `base` names, and checks it out when
    /// `check_out` says so. Returns what HEAD is on after that.
    pub fn create_branch(
        &self,
        repository: &Repository,
        name: &str,
        base: &NewBranchBase,
        check_out: bool,
    ) -> Result<CurrentBranch, Failure> {
        let runner = self.runner(&repository.root)?;
        if check_out {
            let target = CheckoutTarget::NewBranch {
                name: name.to_owned(),
                base: base.clone(),
            };
            return self.check_out(&runner, None, repository, &target);
        }
        new_branch::check_name(&runner, repository, name)?;
        let start = new_branch::start_point(&runner, repository, base)?;
        new_branch::create(&runner, repository, name, &start)?;
        sync::current_branch(&runner, repository)
    }

    /// Renames the local branch `name` of `repository` to `new_name`, which takes it as a new
    /// branch's name would, and with `unset_upstream` leaves it following nothing. HEAD moves along
    /// when it is on the branch. Returns what HEAD is on after that.
    pub fn rename_branch(
        &self,
        repository: &Repository,
        name: &str,
        new_name: &str,
        unset_upstream: bool,
    ) -> Result<CurrentBranch, Failure> {
        let runner = self.runner(&repository.root)?;
        rename_branch::rename(&runner, repository, name, new_name, unset_upstream)?;
        sync::current_branch(&runner, repository)
    }

    /// Deletes the local branches `names` of `repository`, merged or not, as `git branch -D`, and
    /// raises a notification about the Project at `project` that tells of them. It offers to
    /// restore them, to show the commits of each that HEAD did not have, and to delete the remote
    /// branch each followed that the Repository knows of. When one of them does not exist, this
    /// returns [`Failure::RevisionNotFound`], and when HEAD is on one,
    /// [`Failure::CurrentBranchNotDeleted`], and none goes. When git cannot delete one, the ones
    /// before it are gone, the notification tells of those, and this returns why.
    pub fn delete_branches(
        &self,
        project: &Path,
        repository: &Repository,
        names: &[String],
    ) -> Result<(), Failure> {
        let runner = self.runner(&repository.root)?;
        delete_branches::check_deletable(&runner, repository, names)?;
        let head = delete_branches::head_commit(&runner, repository)?;
        let mut deleted = Vec::new();
        let mut outcome = Ok(());
        for name in names {
            match delete_branches::delete(&runner, repository, name, head.as_deref()) {
                Ok(branch) => deleted.push(branch),
                Err(failure) => {
                    outcome = Err(failure);
                    break;
                }
            }
        }
        if !deleted.is_empty() {
            let mut actions = vec![NotificationAction::RestoreBranches];
            actions.extend(
                deleted
                    .iter()
                    .filter(|branch| branch.unmerged)
                    .map(|branch| NotificationAction::ShowUnmergedCommits {
                        branch: branch.name.clone(),
                    }),
            );
            for upstream in deleted.iter().filter_map(|branch| branch.upstream.as_ref()) {
                let action = NotificationAction::DeleteRemoteBranch {
                    upstream: upstream.clone(),
                };
                // Several branches can follow the same remote branch, which goes once.
                if !actions.contains(&action)
                    && delete_branches::has_remote_branch(&runner, repository, upstream)?
                {
                    actions.push(action);
                }
            }
            self.notifications.raise(
                project,
                NotificationKind::BranchesDeleted {
                    repository: repository.clone(),
                    branches: deleted,
                    head,
                },
                actions,
            );
        }
        outcome
    }

    /// Checks out the local branch `name` of `repository`, which HEAD then is on, and returns what
    /// HEAD is on after that. The checkout runs as a [`LongOperation`] that reports the hook it
    /// waits on and that the user can cancel. A branch that no longer exists returns
    /// [`Failure::RevisionNotFound`] with its name. Files that block it return
    /// [`Failure::CheckoutBlocked`], with nothing changed.
    pub fn check_out_branch(
        &self,
        repository: &Repository,
        name: &str,
    ) -> Result<CurrentBranch, Failure> {
        let target = CheckoutTarget::Branch {
            name: name.to_owned(),
        };
        self.check_out(&self.runner(&repository.root)?, None, repository, &target)
    }

    /// Checks out the tag `name` of `repository`, which leaves a detached HEAD on the commit it
    /// points at, and returns what HEAD is on after that. The tag goes by its whole reference, so a
    /// branch with the same name does not get in the way. The checkout runs as in
    /// [`Self::check_out_branch`]. A tag that no longer exists returns
    /// [`Failure::RevisionNotFound`] with its name.
    pub fn check_out_tag(
        &self,
        repository: &Repository,
        name: &str,
    ) -> Result<CurrentBranch, Failure> {
        let target = CheckoutTarget::Tag {
            name: name.to_owned(),
        };
        self.check_out(&self.runner(&repository.root)?, None, repository, &target)
    }

    /// Checks out the commit `revision` names in `repository`, as the commit menu of the log does,
    /// which leaves a detached HEAD on it, and returns what HEAD is on after that. The checkout
    /// runs as in [`Self::check_out_branch`]. A revision that names no commit returns
    /// [`Failure::RevisionNotFound`].
    pub fn check_out_revision(
        &self,
        repository: &Repository,
        revision: &str,
    ) -> Result<CurrentBranch, Failure> {
        let target = CheckoutTarget::Revision {
            revision: revision.to_owned(),
        };
        self.check_out(&self.runner(&repository.root)?, None, repository, &target)
    }

    /// Checks out the remote branch `name` of `repository`, a Repository of the Project at
    /// `project`, such as `origin/main`, as a local branch that follows it, and returns what HEAD is
    /// on after that. The local branch is the branch of the remote, as its fetch configuration
    /// maps it. The checkout runs as in [`Self::check_out_branch`]. A remote branch that no longer
    /// exists returns [`Failure::RevisionNotFound`] with its name.
    ///
    /// A local branch of that name moves to the remote branch when none of its commits would be
    /// lost. Otherwise, with no `local_commits` chosen, nothing changes and this returns
    /// [`Failure::LocalCommitsWouldBeLost`]. [`LocalCommits::Drop`] saves a Backup of the local
    /// branch first and raises a notification about the Project that offers to undo it.
    /// [`LocalCommits::Rebase`] rebases its commits onto the remote branch, which every local change
    /// blocks, as [`Failure::CheckoutBlocked`] tells. A rebase that stops on a conflict says so with
    /// [`Event::ConflictsLeft`] and is in progress.
    pub fn check_out_remote_branch(
        &self,
        project: &Path,
        repository: &Repository,
        name: &str,
        local_commits: Option<LocalCommits>,
    ) -> Result<CurrentBranch, Failure> {
        let target = CheckoutTarget::RemoteBranch {
            name: name.to_owned(),
            local_commits,
        };
        let runner = self.runner(&repository.root)?;
        self.check_out(&runner, Some(project), repository, &target)
    }

    /// Checks out `target` of `repository`, a Repository of the Project at `project`, once the
    /// local changes that blocked it, as [`Failure::CheckoutBlocked`] told, have gone as `over`
    /// says, and returns what HEAD is on after that.
    ///
    /// Discarding goes only when the files the user confirmed are still the ones whose local
    /// changes block the checkout: otherwise nothing goes and this returns
    /// [`Failure::CheckoutBlocked`] with the files that block it now.
    ///
    /// Saving puts the local changes aside as Saved local changes, checks out, and restores them in
    /// their Changelists, as [`Self::save_local_changes`] and [`Self::restore_local_changes`] do,
    /// also when the checkout fails. A restore that conflicts keeps the saved changes and says so
    /// with [`Event::ConflictsLeft`], so conflict resolution opens. Files git does not track are
    /// not saved, so those that block the checkout still do.
    pub fn check_out_over_local_changes(
        &self,
        project: &Path,
        repository: &Repository,
        target: &CheckoutTarget,
        over: &OverLocalChanges,
    ) -> Result<CurrentBranch, Failure> {
        let runner = self.runner(&repository.root)?;
        let planned = checkout::plan(&runner, repository, target)?;
        match over {
            OverLocalChanges::Discard { files } => {
                let files = checkout::to_discard(&runner, repository, &planned, files)?;
                self.discard_local_changes(&runner, repository, &files)?;
                self.run_checkout(&runner, Some(project), repository, planned)
            }
            OverLocalChanges::SaveAndRestore => {
                let rebases = matches!(planned.how, checkout::How::Rebase { .. });
                let saved = self.save_local_changes(repository)?;
                let checked_out = self.run_checkout(&runner, Some(project), repository, planned);
                // A rebase that stopped holds files in conflict, which the changes cannot come
                // back onto: they wait in the stash list for the user to restore once it is over.
                let stopped = rebases
                    && in_progress::read(&runner, repository)?
                        .is_some_and(|operation| operation.kind == OperationKind::Rebase);
                // Otherwise the changes come back whether the checkout went or not, onto whatever
                // HEAD is on by then. Changes that cannot come back are what the user needs to
                // hear of.
                if !stopped {
                    self.restore_local_changes(project, &saved)?;
                }
                checked_out
            }
        }
    }

    /// Discards the uncommitted changes to `files` of `repository`, staged or not, the Rollback
    /// of files the user confirmed, as a [`LongOperation`], bringing them back to HEAD. A file HEAD
    /// does not hold goes.
    fn discard_local_changes(
        &self,
        runner: &GitRunner,
        repository: &Repository,
        files: &[PathBuf],
    ) -> Result<(), Failure> {
        if files.is_empty() {
            return Ok(());
        }
        let mut args: Vec<String> = [
            "--literal-pathspecs",
            "restore",
            "--source=HEAD",
            "--staged",
            "--worktree",
            "--",
        ]
        .map(str::to_owned)
        .into();
        args.extend(files.iter().map(|file| file.to_string_lossy().into_owned()));
        let operation = LongOperation::RollbackFiles {
            repository: repository.clone(),
        };
        let root = &repository.root;
        let invocation = self.run_operation(runner, operation, root, &as_args(&args))?;
        if !invocation.success {
            let failed = Failure::FilesNotRolledBack {
                folder: root.clone(),
            };
            return Err(failure_of(runner, root, &invocation, failed));
        }
        Ok(())
    }

    /// Checks out `target` of `repository`, as [`checkout::plan`] plans it.
    fn check_out(
        &self,
        runner: &GitRunner,
        project: Option<&Path>,
        repository: &Repository,
        target: &CheckoutTarget,
    ) -> Result<CurrentBranch, Failure> {
        let planned = checkout::plan(runner, repository, target)?;
        self.run_checkout(runner, project, repository, planned)
    }

    /// Runs `planned`, a checkout in `repository`, as a [`LongOperation`], and returns what HEAD
    /// is on after it. When it fails, this returns the failure that git's output explains, then
    /// [`Failure::CheckoutBlocked`] when files block it, or the planned fallback when nothing does.
    fn run_checkout(
        &self,
        runner: &GitRunner,
        project: Option<&Path>,
        repository: &Repository,
        planned: checkout::Planned,
    ) -> Result<CurrentBranch, Failure> {
        let backup = match &planned.how {
            checkout::How::Rebase { branch } => {
                let branch = branch.clone();
                return self.run_rebasing_checkout(runner, repository, &branch, planned);
            }
            checkout::How::SwitchDropping { branch, commit } => {
                Some(backups::save(runner, repository, Some(branch), commit)?)
            }
            checkout::How::Switch => None,
        };
        let operation = LongOperation::CheckOut {
            repository: repository.clone(),
            target: planned.label.clone(),
        };
        let root = &repository.root;
        let invocation = self.run_operation(runner, operation, root, &as_args(&planned.args))?;
        if !invocation.success {
            if let Some(backup) = &backup {
                // Nothing was dropped, so there is nothing to undo.
                backups::forget(runner, repository, backup);
            }
            // What blocks it cannot be told when the Repository cannot be read, which says why
            // in the operation log: the checkout failure stays the one to tell.
            let otherwise = checkout::blocked(runner, repository, &planned)
                .ok()
                .flatten()
                .unwrap_or(planned.fallback);
            return Err(failure_of(runner, root, &invocation, otherwise));
        }
        if let (Some(backup), Some(project)) = (backup, project) {
            let rewrite = Rewrite::DropLocalCommits;
            self.raise_history_rewritten(runner, project, repository, rewrite, backup)?;
        }
        sync::current_branch(runner, repository)
    }

    /// Runs `planned`, a checkout in `repository` that rebases the local branch `branch` onto the
    /// remote branch it then follows, as a [`LongOperation`], and returns what HEAD is on after it. Files
    /// that block it return [`Failure::CheckoutBlocked`] before git runs, as git rebases nothing
    /// while a tracked file has uncommitted changes. A rebase that stops on a conflict says so with
    /// [`Event::ConflictsLeft`], so the conflicts dialog opens, and is in progress, with the branch
    /// following the remote branch already.
    fn run_rebasing_checkout(
        &self,
        runner: &GitRunner,
        repository: &Repository,
        branch: &str,
        planned: checkout::Planned,
    ) -> Result<CurrentBranch, Failure> {
        if let Some(blocked) = checkout::blocked(runner, repository, &planned)? {
            return Err(blocked);
        }
        let runner = &runner.accepting_prepared_messages();
        let operation = LongOperation::CheckOut {
            repository: repository.clone(),
            target: planned.label.clone(),
        };
        let args = as_args(&planned.args);
        let (invocation, stopped_on_conflict) =
            self.run_stopping_on_conflicts(runner, repository, operation, &args)?;
        if !invocation.success && !stopped_on_conflict {
            return Err(failure_of(
                runner,
                &repository.root,
                &invocation,
                planned.fallback,
            ));
        }
        checkout::follow(runner, repository, branch, &planned.revision)?;
        sync::current_branch(runner, repository)
    }

    /// The merge, rebase, revert or cherry-pick half done in `repository`, a Repository of the
    /// Project at `project`, whoever started it, as the status area and the branch selector show
    /// it. None when there is none. It is the one of the worktree the Repository is, not of
    /// another worktree of the same repository.
    ///
    /// A stopped rebase also raises a notification about the Project that offers to continue or
    /// abort it, once while it stays stopped. It goes once the rebase is over. The notification
    /// about files left in conflict goes once none is, however they were resolved.
    pub fn operation_in_progress(
        &self,
        project: &Path,
        repository: &Repository,
    ) -> Result<Option<OperationInProgress>, Failure> {
        let runner = self.runner(&repository.root)?;
        if !conflicts::any(&runner, repository)? {
            self.settle_conflicts_unresolved(project, repository);
        }
        let operation = in_progress::read(&runner, repository)?;
        let stopped = NotificationKind::RebaseStopped {
            repository: repository.clone(),
        };
        if operation
            .as_ref()
            .is_some_and(|operation| operation.kind == OperationKind::Rebase)
        {
            self.notifications.raise(
                project,
                stopped,
                vec![
                    NotificationAction::ContinueRebase,
                    NotificationAction::AbortRebase,
                ],
            );
        } else {
            self.notifications.settle(project, &stopped);
        }
        Ok(operation)
    }

    /// The files in conflict in `repository`, a Repository of the Project at `project`, ordered by
    /// path, as the conflicts dialog lists them. Each one tells what each side did to the file from
    /// the user's side: during a rebase, "yours" is what git calls `--theirs`.
    pub fn conflicts(
        &self,
        project: &Path,
        repository: &Repository,
    ) -> Result<Vec<ConflictedFile>, Failure> {
        let files = conflicts::read(&self.runner(&repository.root)?, repository)?;
        if files.is_empty() {
            self.settle_conflicts_unresolved(project, repository);
        }
        Ok(files)
    }

    /// Removes the notification about the files of `repository` left in conflict, as none is.
    fn settle_conflicts_unresolved(&self, project: &Path, repository: &Repository) {
        self.notifications
            .settle(project, &conflicts_unresolved(repository));
    }

    /// Closes the conflicts dialog on `repository`, a Repository of the Project at `project`. The
    /// files still in conflict stay so, as a group in the changes view, and a notification about
    /// the Project offers to resolve them, and to abort the operation in progress that left them
    /// when there is one. It is raised once while they stay in conflict, and goes once none is.
    pub fn close_conflicts(&self, project: &Path, repository: &Repository) -> Result<(), Failure> {
        let runner = self.runner(&repository.root)?;
        if !conflicts::any(&runner, repository)? {
            self.settle_conflicts_unresolved(project, repository);
            return Ok(());
        }
        let mut actions = vec![NotificationAction::ResolveConflicts];
        if in_progress::read(&runner, repository)?.is_some() {
            actions.push(NotificationAction::AbortOperation);
        }
        self.notifications
            .raise(project, conflicts_unresolved(repository), actions);
        Ok(())
    }

    /// Resolves the files at `paths`, relative to the root of `repository` and in conflict there,
    /// with the version of `side`, and returns the files still in conflict. A side that deleted
    /// the file deletes it. "Yours" stays the user's side during a rebase too, where git calls it
    /// `--theirs`. A path that is not in conflict returns [`Failure::FileNotInConflict`], and
    /// nothing is resolved.
    pub fn accept_side(
        &self,
        project: &Path,
        repository: &Repository,
        paths: &[String],
        side: ConflictSide,
    ) -> Result<Vec<ConflictedFile>, Failure> {
        let runner = self.runner(&repository.root)?;
        let commands = conflicts::taking_side(&runner, repository, paths, side)?;
        self.resolve_conflicts(&runner, repository, &commands)?;
        self.conflicts(project, repository)
    }

    /// Marks the files at `paths`, relative to the root of `repository` and in conflict there,
    /// resolved as the user edited them by hand, and returns the files still in conflict. A file
    /// the user deleted leaves git. A path that is not in conflict returns
    /// [`Failure::FileNotInConflict`], and nothing is marked.
    pub fn mark_resolved(
        &self,
        project: &Path,
        repository: &Repository,
        paths: &[String],
    ) -> Result<Vec<ConflictedFile>, Failure> {
        let runner = self.runner(&repository.root)?;
        let command = conflicts::marking_resolved(&runner, repository, paths)?;
        self.resolve_conflicts(&runner, repository, &[command])?;
        self.conflicts(project, repository)
    }

    /// Runs `commands`, the arguments after `git` that resolve conflicts in `repository`, in
    /// order, as a [`LongOperation`]: a filter can take as long as a hook.
    fn resolve_conflicts(
        &self,
        runner: &GitRunner,
        repository: &Repository,
        commands: &[Vec<String>],
    ) -> Result<(), Failure> {
        let root = &repository.root;
        for command in commands {
            let operation = LongOperation::ResolveConflicts {
                repository: repository.clone(),
            };
            let invocation = self.run_operation(runner, operation, root, &as_args(command))?;
            if !invocation.success {
                return Err(failure_of(
                    runner,
                    root,
                    &invocation,
                    Failure::FilesNotResolved {
                        folder: root.clone(),
                    },
                ));
            }
        }
        Ok(())
    }

    /// Takes `action` on the operation half done in `repository`, a Repository of the Project at
    /// `project`, and returns the operation in progress after that. A rebase, revert or
    /// cherry-pick that goes on and stops at a later commit, on a conflict or on one the
    /// resolution left empty, is still in progress.
    /// git runs as a [`LongOperation`] that reports the hook it waits on and that the user can
    /// cancel, and takes the messages it prepared for the commits it makes as they are. With
    /// nothing half done this returns [`Failure::NoOperationInProgress`], with an action the
    /// operation does not offer [`Failure::OperationActionNotOffered`], and when going on while
    /// files still have conflicts [`Failure::ConflictsNotResolved`].
    pub fn take_operation_action(
        &self,
        project: &Path,
        repository: &Repository,
        action: OperationAction,
    ) -> Result<Option<OperationInProgress>, Failure> {
        self.run_operation_action(project, repository, action)?;
        self.operation_in_progress(project, repository)
    }

    /// Takes `action` on the operation half done in `repository`, a Repository of the Project at
    /// `project`, as [`Self::take_operation_action`] tells. A merge that is over, aborted or
    /// committed, brings back the local changes saved around it, as
    /// [`Self::merge_over_local_changes`] tells.
    fn run_operation_action(
        &self,
        project: &Path,
        repository: &Repository,
        action: OperationAction,
    ) -> Result<(), Failure> {
        let runner = self.runner(&repository.root)?;
        let operation =
            in_progress::read(&runner, repository)?.ok_or(Failure::NoOperationInProgress)?;
        if !operation.actions.contains(&action) {
            return Err(Failure::OperationActionNotOffered { action });
        }
        // Skipping leaves the conflicts of the commit out with it.
        let resolved_first = matches!(
            action,
            OperationAction::Continue | OperationAction::CommitMerge
        );
        if resolved_first && conflicts::any(&runner, repository)? {
            return Err(Failure::ConflictsNotResolved);
        }
        let runner = runner.accepting_prepared_messages();
        let root = &repository.root;
        let head_before = in_progress::head(&runner, repository)?;
        let long = LongOperation::TakeOperationAction {
            repository: repository.clone(),
            operation: operation.kind,
            action,
        };
        let args = in_progress::action_args(operation.kind, action);
        let awaiting = match operation.kind {
            OperationKind::Merge => self.awaiting_merge(&runner, repository)?,
            _ => None,
        };
        let invocation = self.run_operation(&runner, long, root, &args)?;
        if invocation.success {
            if let Some((store, awaiting)) = awaiting {
                store.forget();
                self.restore_local_changes(project, &awaiting.saved)?;
            }
            return Ok(());
        }
        // git went on and stopped at a later commit, which the user resolves or skips next: on a
        // conflict, or after committing what it could, on one the resolution left empty.
        if action != OperationAction::Abort
            && !invocation.cancelled
            && in_progress::read(&runner, repository)?.is_some()
        {
            if conflicts::any(&runner, repository)? {
                self.events.send(Event::ConflictsLeft {
                    repository: repository.clone(),
                });
                return Ok(());
            }
            if in_progress::head(&runner, repository)? != head_before {
                return Ok(());
            }
        }
        Err(failure_of(
            &runner,
            root,
            &invocation,
            Failure::OperationActionFailed {
                folder: root.clone(),
                operation: operation.kind,
                action,
            },
        ))
    }

    /// The local changes saved around the merge half done in `repository`, which wait for it to be
    /// over, with where they wait, if any do.
    fn awaiting_merge(
        &self,
        runner: &GitRunner,
        repository: &Repository,
    ) -> Result<Option<(AwaitingMergeStore, AwaitingMerge)>, Failure> {
        let Some(merging) = details::commit_if_any(runner, repository, "MERGE_HEAD")? else {
            return Ok(None);
        };
        let store = AwaitingMergeStore::in_git_dir(&in_progress::git_dir(runner, repository)?);
        Ok(store.awaiting(&merging).map(|awaiting| (store, awaiting)))
    }

    /// Lets go of what gikli holds to read the page after `cursor` quickly, once the log no longer
    /// needs it, as when the user closes it or it starts over. That page can still be read, only
    /// more slowly.
    pub fn end_log_read(&self, cursor: &LogCursor) {
        self.log_reads.end(cursor);
    }

    /// The details of the commit `revision` names in `repository`: its whole message, hash, author
    /// with email and dates. A revision that names no commit returns [`Failure::RevisionNotFound`].
    pub fn commit_details(
        &self,
        repository: &Repository,
        revision: &str,
    ) -> Result<CommitDetails, Failure> {
        details::read(&self.runner(&repository.root)?, repository, revision)
    }

    /// The full hash of the commit the user goes to in the log of `repository`, which `revision`
    /// names: a hash, whole or abbreviated, of 40 or 64 characters, a branch, a remote branch, a
    /// tag or any other revision git knows. When it names no commit, this returns
    /// [`Failure::RevisionNotFound`].
    pub fn go_to(&self, repository: &Repository, revision: &str) -> Result<String, Failure> {
        details::resolve(&self.runner(&repository.root)?, repository, revision)
    }

    /// The full hash of the latest commit of `branch` in `repository`, as the branches pane goes
    /// to it: the commit a local branch, a remote branch or a tag points at, even when a reference
    /// of another kind has the same name. A branch that no longer exists returns
    /// [`Failure::RevisionNotFound`] with its name.
    pub fn go_to_branch(
        &self,
        repository: &Repository,
        branch: &RefLabel,
    ) -> Result<String, Failure> {
        details::resolve_known_as(
            &self.runner(&repository.root)?,
            repository,
            &branch.full_name(),
            &branch.name,
        )
    }

    /// Whether the log of `repository` filtered by `filter` reaches the commit `hash`, which
    /// [`Self::go_to`] returned: whether its search, if any, matches the commit as the pages of the
    /// log would, and any of the branches the log is filtered by holds it, or the log is filtered
    /// by none. When it does not, the log does not show the commit, and going to it can say so
    /// without reading the pages of the log. A regular expression gikli cannot read returns
    /// [`Failure::SearchPatternInvalid`], as [`Self::read_filtered_log`] does, and the search is
    /// not kept in [`Self::log_search_history`].
    pub fn filtered_log_reaches(
        &self,
        repository: &Repository,
        filter: &LogFilter,
        hash: &str,
    ) -> Result<bool, Failure> {
        log::filtered_log_reaches(&self.runner(&repository.root)?, repository, filter, hash)
    }

    /// The local branches, remote branches and tags of `repository` that the user can go to when
    /// they have typed `text`: those whose name holds it, ignoring case. The names that start with
    /// it come first, then the others, each local branches first, then remote branches and tags,
    /// each kind by name.
    pub fn go_to_suggestions(
        &self,
        repository: &Repository,
        text: &str,
    ) -> Result<Vec<RefLabel>, Failure> {
        log::suggestions(&self.runner(&repository.root)?, repository, text)
    }

    /// The local and remote branches of `repository`, local ones first, each kind by name: those
    /// the user can filter the log by.
    pub fn log_branches(&self, repository: &Repository) -> Result<Vec<RefLabel>, Failure> {
        log::branches(&self.runner(&repository.root)?, repository)
    }

    /// The files the commit `revision` names in `repository` changed, against each of its parents,
    /// the first parent first, and against nothing for a commit with none. They are arranged as the
    /// user chose with [`Self::set_commit_changes_grouping`]. A revision that names no commit
    /// returns [`Failure::RevisionNotFound`].
    pub fn commit_changes(
        &self,
        repository: &Repository,
        revision: &str,
    ) -> Result<Vec<CommitChanges>, Failure> {
        let grouping = self.settings().commit_changes_grouping;
        details::changes(
            &self.runner(&repository.root)?,
            repository,
            revision,
            grouping,
        )
    }

    /// The history of the file at `path`, relative to the root of `repository`: the commits that
    /// changed it, newest first, followed through its renames, each with the path the file has in
    /// it. It reads the history of HEAD, or of `branches` when the user filters it by branch, and
    /// lists at most the row limit set with [`Self::set_file_history_row_limit`].
    pub fn file_history(
        &self,
        repository: &Repository,
        path: &str,
        branches: &[RefLabel],
    ) -> Result<FileHistory, Failure> {
        let limit = self.settings().file_history_row_limit;
        file_history::read(
            &self.runner(&repository.root)?,
            repository,
            path,
            branches,
            limit,
        )
    }

    /// Get a file version: brings the file at `path`, relative to the root of `repository`, back to
    /// what it was in the commit `revision` names, where it was at `revision_path`, which differs
    /// from `path` before a rename. Only that file changes, on disk alone: the index and every
    /// other file stay as they are. When the commit has no such file, the file is deleted. A
    /// revision that names no commit returns [`Failure::RevisionNotFound`].
    pub fn get_file_version(
        &self,
        repository: &Repository,
        revision: &str,
        revision_path: &str,
        path: &str,
    ) -> Result<(), Failure> {
        file_history::get_version(
            &self.runner(&repository.root)?,
            repository,
            revision,
            revision_path,
            path,
        )
    }

    /// What the commit `revision` names in `repository` changed in the file at `path`, its path in
    /// that commit: the file in its first parent, at the path it had there, against the file in the
    /// commit, as the preview of a file history shows it. A revision that names no commit returns
    /// [`Failure::RevisionNotFound`].
    pub fn revision_file_diff(
        &self,
        repository: &Repository,
        revision: &str,
        path: &str,
    ) -> Result<RevisionFileDiff, Failure> {
        diff::read_revision(&self.runner(&repository.root)?, repository, revision, path)
    }

    /// Puts the uncommitted changes of `repository` aside in a stash, with the message, keeping
    /// the index and including the files git does not track as `options` say. Returns the stashes
    /// after that, the new one first.
    pub fn stash(
        &self,
        repository: &Repository,
        options: &StashOptions,
    ) -> Result<Stashes, Failure> {
        let runner = self.runner(&repository.root)?;
        stash::push(&runner, repository, options)?;
        stash::list(&runner, repository)
    }

    /// The stashes of `repository`, newest first, with why it cannot stash, when it cannot.
    pub fn stashes(&self, repository: &Repository) -> Result<Stashes, Failure> {
        stash::list(&self.runner(&repository.root)?, repository)
    }

    /// Drops the stash whose commit is `hash` from `repository`, wherever it is in the list now,
    /// once the user confirmed it. Returns the stashes after that.
    pub fn drop_stash(&self, repository: &Repository, hash: &str) -> Result<Stashes, Failure> {
        let runner = self.runner(&repository.root)?;
        stash::drop(&runner, repository, hash)?;
        stash::list(&runner, repository)
    }

    /// Drops every stash of `repository`, once the user confirmed it, when they are still `seen`,
    /// the hashes of the stashes the user was shown, newest first. When they are not,
    /// [`Failure::StashesChanged`] says so and none goes. Returns the stashes after that.
    pub fn clear_stashes(
        &self,
        repository: &Repository,
        seen: &[String],
    ) -> Result<Stashes, Failure> {
        let runner = self.runner(&repository.root)?;
        stash::clear(&runner, repository, seen)?;
        stash::list(&runner, repository)
    }

    /// What the unstash dialog shows for the stash whose commit is `hash` in `repository`: the
    /// stash, whether tracked files have uncommitted changes, and the files the stash holds that
    /// git did not track and that are on disk already. It warns of them without blocking.
    pub fn unstash_plan(
        &self,
        repository: &Repository,
        hash: &str,
    ) -> Result<UnstashPlan, Failure> {
        stash::unstash_plan(&self.runner(&repository.root)?, repository, hash)
    }

    /// Applies the stash whose commit is `hash` to `repository`, a Repository of the Project at
    /// `project`, restoring what was staged when `options` say so, and drops it after when they
    /// say to pop it. Returns the stashes after that, and whether applying left files in conflict.
    /// A stash that left files in conflict says so with [`Event::ConflictsLeft`], so conflict
    /// resolution opens, and is kept even when popped. Uncommitted changes do not stop it: git
    /// fails, with [`Failure::StashNotApplied`], only on what it would overwrite. A stash that is
    /// gone returns [`Failure::StashNotFound`].
    pub fn apply_stash(
        &self,
        project: &Path,
        repository: &Repository,
        hash: &str,
        options: UnstashOptions,
    ) -> Result<Unstashed, Failure> {
        let runner = self.runner(&repository.root)?;
        let mut args = vec!["stash", "apply", "--quiet"];
        if options.restore_index {
            args.push("--index");
        }
        args.push(hash);
        self.bring_back(&runner, project, repository, hash, &args, options.pop)
    }

    /// Creates the branch `name` of `repository`, a Repository of the Project at `project`, at the
    /// commit the stash whose commit is `hash` was made on, checks it out, applies the stash there
    /// with what it staged staged again, and drops it, as `git stash branch`. The name is taken as
    /// a new branch's name is. A stash that left files in conflict says so with
    /// [`Event::ConflictsLeft`], and is kept, as a pop that conflicts is.
    pub fn stash_branch(
        &self,
        project: &Path,
        repository: &Repository,
        hash: &str,
        name: &str,
    ) -> Result<Unstashed, Failure> {
        let runner = self.runner(&repository.root)?;
        new_branch::check_name(&runner, repository, name)?;
        // Given the stash's commit and not its place in the list, git drops nothing itself.
        let args = ["stash", "branch", name, hash];
        self.bring_back(&runner, project, repository, hash, &args, true)
    }

    /// Brings the stash `hash` back into `repository` with `args`, the arguments after `git` that
    /// apply it, and drops it after when `drops` says so. A stash that left files in conflict is
    /// kept, and a notification about the Project at `project` offers to drop it, once the user
    /// confirms it. One dropped elsewhere since it applied is gone already.
    fn bring_back(
        &self,
        runner: &GitRunner,
        project: &Path,
        repository: &Repository,
        hash: &str,
        args: &[&str],
        drops: bool,
    ) -> Result<Unstashed, Failure> {
        let stash = stash::available_stash(runner, repository, hash)?;
        let operation = LongOperation::ApplyStash {
            repository: repository.clone(),
        };
        let root = &repository.root;
        let (invocation, conflicts_left) =
            self.run_stopping_on_conflicts(runner, repository, operation, args)?;
        if !invocation.success && !conflicts_left {
            return Err(failure_of(
                runner,
                root,
                &invocation,
                Failure::StashNotApplied {
                    folder: root.clone(),
                },
            ));
        }
        if drops && conflicts_left {
            self.notifications.raise(
                project,
                NotificationKind::StashKept {
                    repository: repository.clone(),
                    stash,
                },
                vec![NotificationAction::DropStash],
            );
        } else if drops {
            stash::drop_if_there(runner, repository, hash)?;
        }
        Ok(Unstashed {
            stashes: stash::list(runner, repository)?,
            conflicts_left,
        })
    }

    /// Runs `operation`, git with `args` in `repository`, and returns how it ran and whether it
    /// stopped on a conflict, which it says with [`Event::ConflictsLeft`], so the conflicts dialog
    /// opens. Files already in conflict return [`Failure::ConflictsNotResolved`] before git runs,
    /// as git goes on with none of these over them, so a conflict after it is one it left.
    fn run_stopping_on_conflicts(
        &self,
        runner: &GitRunner,
        repository: &Repository,
        operation: LongOperation,
        args: &[&str],
    ) -> Result<(Invocation, bool), Failure> {
        if conflicts::any(runner, repository)? {
            return Err(Failure::ConflictsNotResolved);
        }
        let invocation = self.run_operation(runner, operation, &repository.root, args)?;
        let stopped_on_conflict =
            !invocation.success && !invocation.cancelled && conflicts::any(runner, repository)?;
        if stopped_on_conflict {
            self.events.send(Event::ConflictsLeft {
                repository: repository.clone(),
            });
        }
        Ok((invocation, stopped_on_conflict))
    }

    /// Puts the uncommitted changes to tracked files of `repository` aside in a stash, before an
    /// operation that needs a clean working tree, as Saved local changes. Files git does not track
    /// stay, as git's `--autostash` leaves them. When there is nothing to save, nothing is put
    /// aside, and restoring what this returns restores nothing.
    pub fn save_local_changes(
        &self,
        repository: &Repository,
    ) -> Result<SavedLocalChanges, Failure> {
        let runner = self.runner(&repository.root)?;
        let mut followed = None;
        let change_set = self.read_changes_with_choice(repository, |lists| {
            followed = Some(lists.clone());
            Ok(())
        })?;
        let options = StashOptions {
            message: saved_local_changes::STASH_MESSAGE.to_owned(),
            keep_index: false,
            include_untracked: false,
        };
        let stash = match stash::push(&runner, repository, &options) {
            Ok(hash) => hash,
            Err(Failure::NothingToStash { .. }) => {
                return Ok(SavedLocalChanges {
                    repository: repository.clone(),
                    stash: None,
                    changelist_of_path: BTreeMap::new(),
                });
            }
            Err(Failure::StashUnavailable { folder, reason }) => {
                return Err(Failure::LocalChangesNotSavable { folder, reason });
            }
            Err(failure) => return Err(failure),
        };
        Ok(SavedLocalChanges {
            repository: repository.clone(),
            stash: Some(stash),
            changelist_of_path: followed
                .map(|lists| lists.changelist_of_path(&change_set.changes))
                .unwrap_or_default(),
        })
    }

    /// Restores `saved`, the local changes [`Self::save_local_changes`] put aside, in its
    /// Repository of the Project at `project`, and drops the stash that held them. Only that stash
    /// is restored, and nothing when nothing was saved. When restoring leaves files in conflict,
    /// it says so with [`Event::ConflictsLeft`], so conflict resolution opens, and the stash is
    /// kept, as a pop that conflicts keeps it.
    pub fn restore_local_changes(
        &self,
        project: &Path,
        saved: &SavedLocalChanges,
    ) -> Result<RestoredLocalChanges, Failure> {
        let Some(hash) = &saved.stash else {
            return Ok(RestoredLocalChanges {
                conflicts_left: false,
            });
        };
        let repository = &saved.repository;
        let runner = self.runner(&repository.root)?;
        let args = ["stash", "apply", "--quiet", hash.as_str()];
        let unstashed = self.bring_back(&runner, project, repository, hash, &args, true)?;
        // The Changelists follow what git holds now first, so a file committed since leaves its
        // Changelist before the restored changes go back to theirs.
        self.read_changes_with_choice(repository, |lists| {
            lists.put_back(&saved.changelist_of_path);
            Ok(())
        })?;
        Ok(RestoredLocalChanges {
            conflicts_left: unstashed.conflicts_left,
        })
    }

    /// The files the stash whose commit is `hash` holds in `repository`, by path: each change to a
    /// tracked file against the commit it was made on, and the files git did not track when the
    /// stash includes them, as [`ChangeStatus::Untracked`]. They are arranged as the log arranges
    /// the files of a commit, set with [`Self::set_commit_changes_grouping`].
    pub fn stash_files(&self, repository: &Repository, hash: &str) -> Result<ChangeGroup, Failure> {
        let runner = self.runner(&repository.root)?;
        let files =
            stash::StashCommits::of(&runner, repository, hash)?.files(&runner, repository)?;
        Ok(ChangeGroup::new(
            files,
            self.settings().commit_changes_grouping,
        ))
    }

    /// The diff of the file at `path` in the stash whose commit is `hash` in `repository`: against
    /// the file on disk, or against the file in the commit the stash was made on, as the user chose
    /// with [`Self::set_stash_diff_against`]. Neither side can be edited.
    pub fn stash_file_diff(
        &self,
        repository: &Repository,
        hash: &str,
        path: &str,
    ) -> Result<StashFileDiff, Failure> {
        let against = self.settings().stash_diff_against;
        stash::file_diff(
            &self.runner(&repository.root)?,
            repository,
            hash,
            path,
            against,
        )
    }

    /// Starts watching `repository` for changes made outside gikli: after each burst of changes
    /// to its working tree, its git directory, or the common git directory that holds its refs,
    /// gikli sends [`Event::FilesChanged`]. Watching it again changes nothing.
    pub fn watch_changes(&self, repository: &Repository) -> Result<(), Failure> {
        if lock(&self.watchers).contains_key(&repository.root) {
            return Ok(());
        }
        let runner = self.runner(&repository.root)?;
        let canonical = |path: PathBuf| canonicalize(&path).unwrap_or(path);
        let git_dir = runner
            .git_dir(&repository.root)
            .map_err(git_failed_to_start)?
            .map(canonical);
        let common_git_dir = runner
            .common_git_dir(&repository.root)
            .map_err(git_failed_to_start)?
            .map(canonical);
        let watcher = ChangesWatcher::start(
            repository.clone(),
            git_dir,
            common_git_dir,
            self.events.clone(),
        )
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
        let mut host = self.host.clone();
        if let Some(prompts) = &self.prompts {
            prompts.expose_to(&mut host);
        }
        let runner = GitRunner::new(program, host, self.log.clone());
        Ok(if lock(&self.settings).use_credential_helper {
            runner
        } else {
            runner.without_credential_helpers()
        })
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

/// The hooks git runs on a clone once it has checked out the files, when all that is left of the
/// clone is running them.
const HOOKS_AFTER_CHECKOUT: &[&str] = &["post-index-change", "post-checkout"];

/// Where a clone goes, and what it has to do there.
struct CloneTarget {
    folder: PathBuf,
    /// What the folder held before the clone started.
    found: FoundFolder,
    /// Whether the clone also clones submodules.
    with_submodules: bool,
}

impl CloneTarget {
    /// Whether the folder held something git refuses to clone into.
    fn was_in_use(&self) -> bool {
        matches!(self.found, FoundFolder::Contents)
    }

    /// Whether the files of the clone name submodules that git goes on to clone.
    fn has_submodules_to_clone(&self) -> bool {
        self.with_submodules && self.folder.join(".gitmodules").is_file()
    }
}

/// What the target of a clone held before the clone started.
#[derive(Clone, Copy)]
enum FoundFolder {
    /// There was no folder.
    Nothing,
    /// There was a folder with nothing in it.
    Empty,
    /// There was something gikli must leave alone: a folder with something in it, which git refuses
    /// to clone into, or anything it cannot tell is empty.
    Contents,
}

impl FoundFolder {
    fn at(target: &Path) -> Self {
        match std::fs::read_dir(target).map(|mut entries| entries.next().is_none()) {
            Ok(true) => Self::Empty,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::Nothing,
            Ok(false) | Err(_) => Self::Contents,
        }
    }

    /// Leaves `target`, where a cancelled clone worked, as it was before. The processes of the clone
    /// have just ended, and on Windows what they held open can take a moment to close, or to be
    /// let go by whatever scans new files.
    fn restore(self, target: &Path) -> std::io::Result<()> {
        const ATTEMPTS: u32 = 20;
        for _ in 1..ATTEMPTS {
            if self.remove_what_the_clone_created(target).is_ok() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        self.remove_what_the_clone_created(target)
    }

    fn remove_what_the_clone_created(self, target: &Path) -> std::io::Result<()> {
        match self {
            Self::Contents => Ok(()),
            Self::Nothing if !target.exists() => Ok(()),
            Self::Nothing => std::fs::remove_dir_all(target),
            Self::Empty => {
                for entry in std::fs::read_dir(target)? {
                    let entry = entry?;
                    if entry.file_type()?.is_dir() {
                        std::fs::remove_dir_all(entry.path())?;
                    } else {
                        std::fs::remove_file(entry.path())?;
                    }
                }
                Ok(())
            }
        }
    }
}

/// The folder of a Project, with what git says about it.
struct ProjectFolder {
    /// The runner for operations in the folder.
    runner: GitRunner,
    /// The folder, in its absolute form.
    folder: PathBuf,
    /// The root of the Repository the folder is in, or the run of git that could not give one.
    toplevel: Result<PathBuf, Invocation>,
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
        root.starts_with(&self.folder)
            || self
                .toplevel
                .as_ref()
                .is_ok_and(|toplevel| toplevel == root)
    }

    /// Fails when this folder cannot open as a Project, whatever it holds: when the failure explainer
    /// knows why git could not look at it, as a Repository git refuses is not a broken one and the
    /// user can do something about it, and in a git directory that knows its working tree, where git
    /// gives a root that does not hold the folder.
    fn ensure_can_open(&self) -> Result<(), Failure> {
        match &self.toplevel {
            Err(failed) => match explainer::explain(&self.runner, &self.folder, failed) {
                Some(explanation) => Err(Failure::Explained { explanation }),
                None => Ok(()),
            },
            Ok(root) if !self.folder.starts_with(root) && self.is_outside_work_tree()? => {
                Err(Failure::NotAWorkingTree {
                    folder: self.folder.clone(),
                })
            }
            Ok(_) => Ok(()),
        }
    }

    /// Why this folder, which shows no Repository at all, cannot open as a Project.
    fn nothing_to_open(&self) -> Failure {
        let folder = self.folder.clone();
        if self
            .toplevel
            .as_ref()
            .is_err_and(explainer::found_no_repository)
        {
            return Failure::NotInRepository { folder };
        }
        if self
            .toplevel
            .as_ref()
            .is_err_and(explainer::refused_bare_repository)
        {
            return Failure::NotAWorkingTree { folder };
        }
        match self.is_outside_work_tree() {
            Ok(true) => Failure::NotAWorkingTree { folder },
            Ok(false) => Failure::ProjectNotOpened { folder },
            Err(failure) => failure,
        }
    }

    fn is_outside_work_tree(&self) -> Result<bool, Failure> {
        self.runner
            .is_outside_work_tree(&self.folder)
            .map_err(git_failed_to_start)
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
    Ok(toplevel.is_ok_and(|root| canonicalize(&root).unwrap_or(root) == folder))
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

/// What git reports about the working tree of `repository` now, with the ignored files when
/// `show_ignored` is set.
fn read_status(
    runner: &GitRunner,
    repository: &Repository,
    show_ignored: bool,
) -> Result<status::Status, Failure> {
    let invocation = runner
        .run(&repository.root, &status::status_args(show_ignored))
        .map_err(git_failed_to_start)?;
    if !invocation.success {
        return Err(failure_of(
            runner,
            &repository.root,
            &invocation,
            Failure::ChangesUnreadable {
                folder: repository.root.clone(),
            },
        ));
    }
    Ok(status::Status::parse(&invocation.stdout))
}

/// The failure of `invocation`, which failed in `folder`: the user's cancelling it, when they did,
/// or else what the failure explainer says about it, or `otherwise` when it does not know this
/// failure.
fn failure_of(
    runner: &GitRunner,
    folder: &Path,
    invocation: &Invocation,
    otherwise: Failure,
) -> Failure {
    failure_of_contacting(
        runner,
        folder,
        invocation,
        &explainer::Contacted::Nothing,
        otherwise,
    )
}

/// The failure of `invocation`, which failed in `folder` and reached out to what `contacted` names,
/// as [`failure_of`] tells it.
fn failure_of_contacting(
    runner: &GitRunner,
    folder: &Path,
    invocation: &Invocation,
    contacted: &explainer::Contacted,
    otherwise: Failure,
) -> Failure {
    if invocation.cancelled {
        return Failure::OperationCancelled {
            left_behind: explainer::left_behind_by_cancelled_run(runner, folder),
        };
    }
    match explainer::explain_contacting(runner, folder, invocation, contacted) {
        Some(explanation) => Failure::Explained { explanation },
        None => otherwise,
    }
}

/// The failure of `invocation`, which failed in `folder` and reached out to `remotes`, as
/// [`failure_of`] tells it.
fn failure_of_contacting_remotes(
    runner: &GitRunner,
    folder: &Path,
    invocation: &Invocation,
    remotes: &[String],
    otherwise: Failure,
) -> Failure {
    let contacted = explainer::Contacted::Remotes(remotes);
    failure_of_contacting(runner, folder, invocation, &contacted, otherwise)
}

/// What the notification about the files of `repository` left in conflict says.
fn conflicts_unresolved(repository: &Repository) -> NotificationKind {
    NotificationKind::ConflictsUnresolved {
        repository: repository.clone(),
    }
}

/// `command`, a list of arguments gikli built, as the runner takes it.
fn as_args(command: &[String]) -> Vec<&str> {
    command.iter().map(String::as_str).collect()
}

fn git_failed_to_start(error: std::io::Error) -> Failure {
    Failure::GitFailedToStart {
        message: error.to_string(),
    }
}
