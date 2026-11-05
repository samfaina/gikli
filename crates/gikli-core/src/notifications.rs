//! Notifications: long-lived messages that carry actions. The core keeps them until the user acts
//! on them, dismisses them, or gikli restarts, so the user can act on them later.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::events::{Event, EventBus};
use crate::{Failure, Project, Repository, lock};

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
}

/// What acting on a notification changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "payload", rename_all = "camelCase")]
pub enum NotificationOutcome {
    /// The Repositories of the Project changed; this is the Project now.
    ProjectChanged(Project),
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
        }
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
    /// raised or the user dismissed it.
    pub fn raise(&self, project: &Path, kind: NotificationKind, actions: Vec<NotificationAction>) {
        let mut state = self.lock();
        if state.is_raised_or_dismissed(project, &kind) {
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
        let Some(index) = state
            .raised
            .iter()
            .position(|notification| notification.id == id)
        else {
            return;
        };
        let notification = state.raised.remove(index);
        state
            .dismissed
            .push((notification.project, notification.kind));
        self.events.send(Event::NotificationRemoved { id });
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
