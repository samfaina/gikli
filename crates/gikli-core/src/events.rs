//! The events the core sends to whoever listens: the UI, and the tests.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};

use serde::Serialize;

/// Something that happened in the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", content = "payload", rename_all = "camelCase")]
pub enum Event {
    /// gikli ran a git command. The operation log shows these entries.
    OperationLogged(OperationLogEntry),
}

/// One git command gikli ran, with its raw output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogEntry {
    /// The folder git ran in.
    pub folder: PathBuf,
    /// The full command line after `git`.
    pub args: Vec<String>,
    /// None when git was ended by a signal.
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
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
