//! The events the core sends to whoever listens: the UI, and the tests.

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};

use serde::Serialize;

use crate::Repository;
use crate::notifications::Notification;
use crate::operation_log::OperationLogEntry;
use crate::operations::{LongOperation, Progress};

/// Something that happened in the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", content = "payload", rename_all = "camelCase")]
pub enum Event {
    /// gikli ran a git command. The operation log shows these entries.
    OperationLogged(OperationLogEntry),
    /// The user cleared the operation log: every view of it empties.
    OperationLogCleared,
    /// A feature raised a notification. It stays until the user acts on it or dismisses it.
    NotificationRaised(Notification),
    /// A notification went away: the user acted on it or dismissed it, or what it was about no
    /// longer holds.
    NotificationRemoved { id: u64 },
    /// A long operation started. Its progress follows, then [`Event::OperationFinished`].
    OperationStarted { id: u64, operation: LongOperation },
    /// The long operation `id` advanced.
    OperationProgress { id: u64, progress: Progress },
    /// The long operation `id` ended, whether it succeeded or not.
    OperationFinished { id: u64 },
    /// Files of a watched Repository changed, in its working tree or in its git directory,
    /// outside gikli: its changes should be read again. A burst of changes is announced once.
    FilesChanged { repository: Repository },
}

/// The listeners of the core's events.
#[derive(Default)]
pub(crate) struct EventBus {
    listeners: Mutex<Vec<Sender<Event>>>,
}

impl EventBus {
    pub fn subscribe(&self) -> Receiver<Event> {
        let (sender, receiver) = mpsc::channel();
        self.lock().push(sender);
        receiver
    }

    pub fn send(&self, event: Event) {
        self.lock()
            .retain(|listener| listener.send(event.clone()).is_ok());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Sender<Event>>> {
        self.listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
