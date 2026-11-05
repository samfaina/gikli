//! The operation log: every git command gikli ran, with its raw output. It is read-only: the user
//! can read it and clear it, nothing else.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

use crate::events::{Event, EventBus};
use crate::lock;

/// One git command gikli ran, with its raw output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogEntry {
    /// Grows with each command, so a listener that read the log and then receives the same entry
    /// as an event can tell it already has it.
    pub id: u64,
    /// The folder git ran in.
    pub folder: PathBuf,
    /// The full command line after `git`.
    pub args: Vec<String>,
    /// None when git was ended by a signal or could not start.
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Why git could not start, when it did not.
    pub error: Option<String>,
}

/// How many entries the log keeps. Older ones are dropped, so a session that runs git for days,
/// as the watcher makes it do, does not grow without end.
pub(crate) const KEPT_ENTRIES: usize = 1000;

/// The operation log, shared by every git runner of the core. It keeps the last
/// [`KEPT_ENTRIES`] entries.
pub(crate) struct OperationLog {
    entries: Mutex<Entries>,
    events: Arc<EventBus>,
}

#[derive(Default)]
struct Entries {
    next_id: u64,
    kept: VecDeque<OperationLogEntry>,
}

impl OperationLog {
    pub fn new(events: Arc<EventBus>) -> Self {
        Self {
            entries: Mutex::default(),
            events,
        }
    }

    /// Adds the command described by `entry` and announces it. The log gives it its id, whatever
    /// `entry.id` holds.
    pub fn record(&self, mut entry: OperationLogEntry) {
        let mut entries = self.lock();
        entry.id = entries.next_id;
        entries.next_id += 1;
        if entries.kept.len() == KEPT_ENTRIES {
            entries.kept.pop_front();
        }
        entries.kept.push_back(entry.clone());
        // Sent while the log is locked, so listeners receive entries in the order they are kept.
        self.events.send(Event::OperationLogged(entry));
    }

    /// Removes every entry and announces it. Later entries keep counting from the last id.
    pub fn clear(&self) {
        let mut entries = self.lock();
        entries.kept.clear();
        self.events.send(Event::OperationLogCleared {
            before: entries.next_id,
        });
    }

    /// The entries kept, oldest first.
    pub fn entries(&self) -> Vec<OperationLogEntry> {
        self.lock().kept.iter().cloned().collect()
    }

    fn lock(&self) -> MutexGuard<'_, Entries> {
        lock(&self.entries)
    }
}
