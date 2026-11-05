//! Long operations: git commands that take long enough for the user to want to see them advance,
//! such as a clone. Each one is announced when it starts and when it finishes, with the progress
//! git reports in between. Nothing else in the core waits for them.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::events::{Event, EventBus};
use crate::lock;

/// A long operation, as the views show it while it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LongOperation {
    /// Cloning the repository at `url`, its password hidden, into `folder`.
    Clone { url: String, folder: PathBuf },
}

/// How far one phase of a long operation has come, as git reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// The phase as git names it, in the user's language, such as `Receiving objects`.
    pub phase: String,
    /// From 0 to 100.
    pub percent: u8,
}

impl Progress {
    /// The progress in one line of git's progress output, such as
    /// `Receiving objects:  45% (9/20), 1.2 MiB | 3.0 MiB/s`. git translates the phase but not
    /// the `NN% (a/b)` counter, so this reads any language.
    pub(crate) fn parse(line: &str) -> Option<Self> {
        let counter = line.find("% (")?;
        let digits_start = line[..counter]
            .rfind(|character: char| !character.is_ascii_digit())
            .map_or(0, |index| index + 1);
        let percent: u8 = line[digits_start..counter].parse().ok()?;
        let phase = line[..digits_start].trim_end().strip_suffix(':')?.trim();
        (percent <= 100 && !phase.is_empty()).then(|| Self {
            phase: phase.to_owned(),
            percent,
        })
    }
}

/// A long operation that has started and not finished, with the last progress it reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningOperation {
    pub id: u64,
    pub operation: LongOperation,
    /// None until git reports any.
    pub progress: Option<Progress>,
}

/// The long operations of the core: the ones running, announced as they start, advance and
/// finish.
pub(crate) struct LongOperations {
    state: Mutex<State>,
    events: Arc<EventBus>,
}

#[derive(Default)]
struct State {
    next_id: u64,
    running: Vec<RunningOperation>,
}

impl LongOperations {
    pub fn new(events: Arc<EventBus>) -> Self {
        Self {
            state: Mutex::default(),
            events,
        }
    }

    /// Runs `work` as `operation`. `work` receives the function that reports its progress.
    /// [`Event::OperationStarted`] goes out before it, and [`Event::OperationFinished`] after it
    /// whatever its result. Nothing is locked while `work` runs.
    pub fn run<T>(&self, operation: LongOperation, work: impl FnOnce(&dyn Fn(Progress)) -> T) -> T {
        let id = {
            let mut state = lock(&self.state);
            let id = state.next_id;
            state.next_id += 1;
            state.running.push(RunningOperation {
                id,
                operation: operation.clone(),
                progress: None,
            });
            self.events.send(Event::OperationStarted { id, operation });
            id
        };
        let result = work(&|progress| {
            let mut state = lock(&self.state);
            if let Some(running) = state.running.iter_mut().find(|running| running.id == id) {
                running.progress = Some(progress.clone());
            }
            self.events.send(Event::OperationProgress { id, progress });
        });
        let mut state = lock(&self.state);
        state.running.retain(|running| running.id != id);
        self.events.send(Event::OperationFinished { id });
        result
    }

    /// The long operations running, oldest first.
    pub fn running(&self) -> Vec<RunningOperation> {
        lock(&self.state).running.clone()
    }
}
