//! Long operations: git commands that take long enough for the user to want to see them advance,
//! such as a clone. Each one is announced when it starts and when it finishes, with the progress
//! git reports and the hook it waits on in between. The user can cancel one. gikli never does: it
//! cannot tell a hook waiting on the console from a slow one. Nothing else in the core waits for
//! them.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::Repository;
use crate::events::{Event, EventBus};
use crate::lock;
use crate::process_tree::Cancellation;

/// A long operation, as the views show it while it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LongOperation {
    /// Cloning the repository at `url`, its password hidden, into `folder`.
    Clone { url: String, folder: PathBuf },
    /// Adding files to the git index of `repository`, where a filter can take as long as a hook.
    AddFiles { repository: Repository },
    /// Rolling back files of `repository`, where a filter can take as long as a hook.
    RollbackFiles { repository: Repository },
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
    /// The hook the operation waits on, as git names it, while one runs.
    pub hook: Option<String>,
    /// Whether the user can cancel it. They cannot when git runs inside WSL, where ending the
    /// process on the Windows side may not reach the ones on the Linux side.
    pub cancellable: bool,
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
    running: Vec<Tracked>,
}

/// A long operation running, with what cancels it.
struct Tracked {
    shown: RunningOperation,
    cancellation: Arc<Cancellation>,
}

impl State {
    fn running(&mut self, id: u64) -> Option<&mut RunningOperation> {
        self.running
            .iter_mut()
            .map(|tracked| &mut tracked.shown)
            .find(|running| running.id == id)
    }
}

/// One long operation while it runs: what its work tells the views, and what cancels it.
pub(crate) struct OperationRun<'a> {
    id: u64,
    operations: &'a LongOperations,
    cancellation: Arc<Cancellation>,
}

impl OperationRun<'_> {
    pub fn report(&self, progress: Progress) {
        let mut state = lock(&self.operations.state);
        if let Some(running) = state.running(self.id) {
            running.progress = Some(progress.clone());
        }
        self.operations.events.send(Event::OperationProgress {
            id: self.id,
            progress,
        });
    }

    /// `hook` is the one the operation waits on from now on, or None when it waits on none.
    pub fn report_hook(&self, hook: Option<String>) {
        let mut state = lock(&self.operations.state);
        if let Some(running) = state.running(self.id) {
            running.hook = hook.clone();
        }
        self.operations
            .events
            .send(Event::OperationHook { id: self.id, hook });
    }

    pub fn cancellation(&self) -> &Cancellation {
        &self.cancellation
    }
}

impl LongOperations {
    pub fn new(events: Arc<EventBus>) -> Self {
        Self {
            state: Mutex::default(),
            events,
        }
    }

    /// Runs `work` as `operation`, which the user can cancel if it is `cancellable`. `work`
    /// receives what it tells the views through and what cancels it. [`Event::OperationStarted`]
    /// goes out before it, and [`Event::OperationFinished`] after it whatever its result. Nothing
    /// is locked while `work` runs.
    pub fn run<T>(
        &self,
        operation: LongOperation,
        cancellable: bool,
        work: impl FnOnce(&OperationRun) -> T,
    ) -> T {
        let cancellation = Arc::new(Cancellation::default());
        let id = {
            let mut state = lock(&self.state);
            let id = state.next_id;
            state.next_id += 1;
            state.running.push(Tracked {
                shown: RunningOperation {
                    id,
                    operation: operation.clone(),
                    progress: None,
                    hook: None,
                    cancellable,
                },
                cancellation: cancellation.clone(),
            });
            self.events.send(Event::OperationStarted {
                id,
                operation,
                cancellable,
            });
            id
        };
        let result = work(&OperationRun {
            id,
            operations: self,
            cancellation,
        });
        let mut state = lock(&self.state);
        state.running.retain(|tracked| tracked.shown.id != id);
        self.events.send(Event::OperationFinished { id });
        result
    }

    /// Cancels the operation `id`, when it runs and the user can cancel it. It ends as its work
    /// returns, without this waiting for that.
    pub fn cancel(&self, id: u64) {
        let cancellation = lock(&self.state)
            .running
            .iter()
            .find(|tracked| tracked.shown.id == id && tracked.shown.cancellable)
            .map(|tracked| tracked.cancellation.clone());
        if let Some(cancellation) = cancellation {
            cancellation.request();
        }
    }

    /// The long operations running, oldest first.
    pub fn running(&self) -> Vec<RunningOperation> {
        lock(&self.state)
            .running
            .iter()
            .map(|tracked| tracked.shown.clone())
            .collect()
    }
}
