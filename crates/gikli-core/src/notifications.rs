//! Notifications: long-lived messages that carry actions. The core keeps them until the user acts
//! on them, dismisses them, or gikli restarts, so the user can act on them later.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::events::{Event, EventBus};
use crate::{DeletedBranch, Failure, ListedCommit, Project, RemoteBranch, Repository, lock};

/// A long-lived message about a Project, with the actions the user can take on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: u64,
    /// The folder of the Project the notification is about, as [`crate::Project::folder`] gives
    /// it. Its window shows it.
    pub project: PathBuf,
    pub kind: NotificationKind,
    /// The actions the user can take, in the order to offer them.
    pub actions: Vec<NotificationAction>,
}

/// What a notification says. The UI words each kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NotificationKind {
    /// gikli found a Repository in the Project's folder that is neither registered nor ignored.
    UnregisteredRepository { repository: Repository },
    /// A Repository registered in the Project is no longer a valid repository: it was moved,
    /// deleted or broken.
    InvalidRepository { repository: Repository },
    /// The user cancelled the clone that created `repository` while the hook `hook` ran. The clone
    /// is complete, and the hook did not finish.
    HookNotFinished {
        repository: Repository,
        hook: String,
    },
    /// gikli deleted the local branches `branches` of `repository`, as the user asked, while HEAD
    /// was on the commit `head`, which their commits were merged into or not. A HEAD on a branch
    /// with no commit yet has none.
    BranchesDeleted {
        repository: Repository,
        branches: Vec<DeletedBranch>,
        head: Option<String>,
    },
    /// A rebase in `repository` stopped, on a conflict or at a commit to edit, and waits for the
    /// user to continue or abort it.
    RebaseStopped { repository: Repository },
    /// The user closed the conflicts dialog while files of `repository` were still in conflict.
    /// They stay in conflict, as a group in the changes view, until the user resolves them or
    /// aborts the operation that left them.
    ConflictsUnresolved { repository: Repository },
}

/// An action a notification offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum NotificationAction {
    /// Registers the Repository in the Project.
    AddRepository { repository: Repository },
    /// Stops offering the Repository to the Project.
    IgnoreRepository { repository: Repository },
    /// Unregisters the Repository from the Project.
    RemoveRepository { repository: Repository },
    /// Creates the deleted branches again, each on the commit it was on.
    RestoreBranches,
    /// Shows the commits of the deleted branch `branch` that HEAD did not have when it went.
    ShowUnmergedCommits { branch: String },
    /// Deletes `upstream`, which a deleted branch followed, on its remote too.
    DeleteRemoteBranch { upstream: RemoteBranch },
    /// Goes on with the stopped rebase, once the user resolved the conflicts it stopped on.
    ContinueRebase,
    /// Gives up the stopped rebase, and brings the branch back to where it was.
    AbortRebase,
    /// Opens the conflicts dialog again, to resolve the files still in conflict.
    ResolveConflicts,
    /// Gives up the operation in progress that left the conflicts, and brings back what HEAD and
    /// the files were before it started.
    AbortOperation,
}

/// What acting on a notification changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "payload", rename_all = "camelCase")]
pub enum NotificationOutcome {
    /// The Repositories of the Project changed; this is the Project now.
    ProjectChanged(Project),
    /// The branches of the Repository changed.
    BranchesChanged(Repository),
    /// The commits a deleted branch took away, newest first.
    UnmergedCommits(Vec<ListedCommit>),
    /// HEAD of the Repository moved, as continuing or aborting a rebase moves it, and the operation
    /// in progress there may have changed or ended.
    HeadMoved(Repository),
    /// The conflicts dialog opens on the files of the Repository still in conflict.
    ResolveConflicts(Repository),
}

/// The deletion of branches a notification tells of.
pub(crate) struct Deletion<'a> {
    pub repository: &'a Repository,
    pub branches: &'a [DeletedBranch],
    /// The commit HEAD was on when they went, if any.
    pub head: Option<&'a str>,
}

impl Notification {
    /// The deletion of branches this notification tells of. One about anything else offers no
    /// action on deleted branches, and returns [`Failure::NotificationActionNotOffered`].
    pub(crate) fn deletion(&self) -> Result<Deletion<'_>, Failure> {
        match &self.kind {
            NotificationKind::BranchesDeleted {
                repository,
                branches,
                head,
            } => Ok(Deletion {
                repository,
                branches,
                head: head.as_deref(),
            }),
            _ => Err(Failure::NotificationActionNotOffered { id: self.id }),
        }
    }

    /// The Repository whose stopped rebase this notification tells of. One about anything else
    /// offers no action on a rebase, and returns [`Failure::NotificationActionNotOffered`].
    pub(crate) fn stopped_rebase(&self) -> Result<&Repository, Failure> {
        match &self.kind {
            NotificationKind::RebaseStopped { repository } => Ok(repository),
            _ => Err(Failure::NotificationActionNotOffered { id: self.id }),
        }
    }

    /// The Repository whose unresolved conflicts this notification tells of. One about anything
    /// else offers no action on conflicts, and returns [`Failure::NotificationActionNotOffered`].
    pub(crate) fn unresolved_conflicts(&self) -> Result<&Repository, Failure> {
        match &self.kind {
            NotificationKind::ConflictsUnresolved { repository } => Ok(repository),
            _ => Err(Failure::NotificationActionNotOffered { id: self.id }),
        }
    }
}

impl NotificationKind {
    /// Whether what the notification says still holds for `project`, the Project it is about.
    fn holds_for(&self, project: &Project) -> bool {
        match self {
            Self::UnregisteredRepository { repository } => {
                project.unregistered_repositories.contains(repository)
            }
            Self::InvalidRepository { repository } => {
                project.invalid_repositories.contains(repository)
            }
            // Whether a rebase is still stopped, or files still in conflict, takes git to tell: see
            // [`Notifications::settle`].
            Self::HookNotFinished { .. }
            | Self::BranchesDeleted { .. }
            | Self::RebaseStopped { .. }
            | Self::ConflictsUnresolved { .. } => true,
        }
    }

    /// Whether the notification tells of something that happened once. It may happen again, and it
    /// is news then, so dismissing it does not keep it from being raised again.
    fn happened_once(&self) -> bool {
        matches!(
            self,
            Self::HookNotFinished { .. } | Self::BranchesDeleted { .. }
        )
    }
}

/// The notifications raised and not yet acted on or dismissed.
pub(crate) struct Notifications {
    state: Mutex<State>,
    events: Arc<EventBus>,
}

#[derive(Default)]
struct State {
    next_id: u64,
    raised: Vec<Notification>,
    /// What the user dismissed, by Project and kind, while it still holds: it is not raised
    /// again until gikli restarts.
    dismissed: Vec<(PathBuf, NotificationKind)>,
}

impl State {
    fn is_raised_or_dismissed(&self, project: &Path, kind: &NotificationKind) -> bool {
        self.raised
            .iter()
            .any(|raised| raised.project == project && &raised.kind == kind)
            || self
                .dismissed
                .iter()
                .any(|(folder, dismissed)| folder == project && dismissed == kind)
    }
}

impl Notifications {
    pub fn new(events: Arc<EventBus>) -> Self {
        Self {
            state: Mutex::default(),
            events,
        }
    }

    /// Raises a notification about the Project at `project`, unless the same one is already
    /// raised or the user dismissed it. One that tells of something that happened once is news
    /// each time.
    pub fn raise(&self, project: &Path, kind: NotificationKind, actions: Vec<NotificationAction>) {
        let mut state = self.lock();
        if !kind.happened_once() && state.is_raised_or_dismissed(project, &kind) {
            return;
        }
        let notification = Notification {
            id: state.next_id,
            project: project.to_path_buf(),
            kind,
            actions,
        };
        state.next_id += 1;
        state.raised.push(notification.clone());
        self.events.send(Event::NotificationRaised(notification));
    }

    /// The notification `id`, when it is still raised and offers `action`.
    pub fn offering(&self, id: u64, action: &NotificationAction) -> Result<Notification, Failure> {
        let state = self.lock();
        let notification = state
            .raised
            .iter()
            .find(|notification| notification.id == id)
            .ok_or(Failure::NotificationGone { id })?;
        if !notification.actions.contains(action) {
            return Err(Failure::NotificationActionNotOffered { id });
        }
        Ok(notification.clone())
    }

    /// Removes the notification `id`, when it is still raised, and announces it. It is not raised
    /// again while what it says holds.
    pub fn dismiss(&self, id: u64) {
        let mut state = self.lock();
        let Some(notification) = self.remove(&mut state, id) else {
            return;
        };
        if !notification.kind.happened_once() {
            state
                .dismissed
                .push((notification.project, notification.kind));
        }
    }

    /// Removes the notification `id`, when it is still raised, and announces it, as what it said
    /// was dealt with. Unlike [`Self::dismiss`], it may be raised again whenever it holds again.
    pub fn withdraw(&self, id: u64) {
        let mut state = self.lock();
        self.remove(&mut state, id);
    }

    /// Takes the notification `id` out of `state`, when it is still raised, announces it and
    /// returns it.
    fn remove(&self, state: &mut State, id: u64) -> Option<Notification> {
        let index = state
            .raised
            .iter()
            .position(|notification| notification.id == id)?;
        let notification = state.raised.remove(index);
        self.events.send(Event::NotificationRemoved { id });
        Some(notification)
    }

    /// Removes the notifications about `project` that say `kind`, which no longer holds, and
    /// announces each removal. Once the user dismissed it, it may be raised again if it comes back.
    pub fn settle(&self, project: &Path, kind: &NotificationKind) {
        let mut state = self.lock();
        let (settled, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut state.raised)
            .into_iter()
            .partition(|notification| {
                notification.project == project && &notification.kind == kind
            });
        state.raised = kept;
        state
            .dismissed
            .retain(|(folder, dismissed)| folder != project || dismissed != kind);
        for notification in settled {
            self.events.send(Event::NotificationRemoved {
                id: notification.id,
            });
        }
    }

    /// Takes `action` away from the notification `id`, when it is still raised, as the user took
    /// it and it cannot be taken again. The notification stays where it was with the other
    /// actions, and the change is announced.
    pub fn withdraw_action(&self, id: u64, action: &NotificationAction) {
        let mut state = self.lock();
        let Some(notification) = state
            .raised
            .iter_mut()
            .find(|notification| notification.id == id)
        else {
            return;
        };
        notification.actions.retain(|offered| offered != action);
        let changed = notification.clone();
        self.events.send(Event::NotificationChanged(changed));
    }

    /// Removes the notifications about `project` whose message no longer holds, as after the
    /// user acted on one of them or changed the Project outside gikli, and announces each
    /// removal.
    pub fn withdraw_settled(&self, project: &Project) {
        let mut state = self.lock();
        let (kept, settled): (Vec<_>, Vec<_>) = std::mem::take(&mut state.raised)
            .into_iter()
            .partition(|notification| {
                notification.project != project.folder || notification.kind.holds_for(project)
            });
        state.raised = kept;
        // Once a dismissed message no longer holds, it may be raised again if it comes back.
        state
            .dismissed
            .retain(|(folder, kind)| folder != &project.folder || kind.holds_for(project));
        for notification in settled {
            self.events.send(Event::NotificationRemoved {
                id: notification.id,
            });
        }
    }

    /// The notifications raised, oldest first.
    pub fn raised(&self) -> Vec<Notification> {
        self.lock().raised.clone()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }
}
