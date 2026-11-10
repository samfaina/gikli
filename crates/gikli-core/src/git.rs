//! The git runner: the single way gikli runs git (ADR-0001).
//!
//! Every invocation runs the user's git executable with the user's configuration, except for the
//! settings that only change how output looks. Those are pinned, so a setting such as
//! `color.ui=always` or `core.quotePath=true` never changes what gikli parses. Commands that
//! gikli parses must still ask for machine-readable output themselves (porcelain formats, `-z`,
//! explicit `--format`).
//!
//! Credential helpers are the other exception. gikli adds one of its own after the user's, which
//! only notes that a remote refused the credentials, and the "Use credential helper" setting can
//! turn the user's off.
//!
//! The run that looks for the root of a repository is told not to translate its messages
//! (`LC_ALL=C`), as nothing but one of git's sentences says that there is no repository, or that
//! the user's `safe.bareRepository` refuses the one it found.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::host::HostEnvironment;
use crate::lock;
use crate::operation_log::{OperationLog, OperationLogEntry};
use crate::operations::{OperationRun, Progress};
use crate::process_tree::{self, Cancellation};
use crate::prompts;
use crate::trace::{self, ForwardedEvents, HookRuns, TraceReader};
use crate::url_passwords::hide_url_passwords;
use crate::version::GitVersion;
use crate::wsl::WslShare;
use crate::{GitCheck, GitSource, MINIMUM_GIT_VERSION};

/// Configuration that only changes presentation, pinned on every invocation.
/// Per-command color settings override `color.ui`, so each one is pinned too.
const PINNED_CONFIG: &[&str] = &[
    "color.ui=false",
    "color.advice=false",
    "color.branch=false",
    "color.diff=false",
    "color.grep=false",
    "color.interactive=false",
    "color.push=false",
    "color.remote=false",
    "color.showBranch=false",
    "color.status=false",
    "color.transport=false",
    "core.quotePath=false",
    "diff.noprefix=false",
    "diff.mnemonicPrefix=false",
    "diff.relative=false",
    "log.showSignature=false",
    "status.relativePaths=false",
];

/// Variables that would point git at another repository or index than the one gikli opened.
/// They leak in when gikli is started from a git hook or a shell that set them. Variables that
/// carry the user's configuration (`GIT_CONFIG_COUNT` and the like) are kept.
const REDIRECTING_VARIABLES: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
    // A git started from a hook nests its trace under that hook's, and gikli reads only the
    // trace of the git it started.
    "GIT_TRACE2_PARENT_SID",
];

/// The git a runner starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitProgram {
    /// A git executable, started directly.
    Native(PathBuf),
    /// The git inside WSL, started through `wsl.exe`: in the distribution of `share`, or in the
    /// default distribution when there is none.
    Wsl {
        wsl: PathBuf,
        share: Option<WslShare>,
    },
}

impl GitProgram {
    /// The program gikli found as `executable` from `source`.
    pub fn found(executable: PathBuf, source: GitSource) -> Self {
        match source {
            GitSource::Configured | GitSource::Path => Self::Native(executable),
            GitSource::Wsl => Self::Wsl {
                wsl: executable,
                share: None,
            },
        }
    }

    /// The command that starts git in `folder`, before its arguments. Inside WSL, a run whose trace
    /// is forwarded with the marker `forwarding_with` goes through [`trace::WSL_FORWARDER`].
    fn command(&self, folder: &Path, forwarding_with: Option<&str>) -> Command {
        match self {
            Self::Native(executable) => {
                let mut command = Command::new(executable);
                command.current_dir(folder);
                command
            }
            Self::Wsl { wsl, share } => {
                let mut command = Command::new(wsl);
                if let Some(share) = share {
                    command.args(["--distribution", &share.distribution]);
                }
                // A folder inside the distribution goes as its Linux path; wsl.exe translates
                // any other Windows path itself.
                match WslShare::of(folder) {
                    Some((_, linux_path)) => command.args(["--cd", &linux_path]),
                    None => command.arg("--cd").arg(folder),
                };
                command.arg("--exec");
                if let Some(marker) = forwarding_with {
                    command.args(["sh", "-c", trace::WSL_FORWARDER, "sh", marker]);
                }
                command.arg("git");
                command
            }
        }
    }

    /// The arguments that ask git for the root of the repository a folder is in.
    fn toplevel_args(&self) -> &'static [&'static str] {
        match self {
            Self::Native(_) => &["rev-parse", "--path-format=absolute", "--show-toplevel"],
            // git inside WSL prints Linux paths, and where a Windows drive appears inside WSL
            // depends on the distribution's settings. The way up from the folder, as `../`
            // steps, needs no translation. Outside a working tree that way up is empty and git
            // does not fail, as it does when it also has to give the root.
            Self::Wsl { .. } => &["rev-parse", "--show-toplevel", "--show-cdup"],
        }
    }

    /// The root of the repository `folder` is in, from the output of [`Self::toplevel_args`].
    fn toplevel(&self, folder: &Path, output: &[u8]) -> PathBuf {
        let output = output.strip_suffix(b"\n").unwrap_or(output);
        match self {
            Self::Native(_) => path_from_bytes(output),
            Self::Wsl { .. } => {
                let output = String::from_utf8_lossy(output);
                folder.join(output.rsplit('\n').next().unwrap_or_default())
            }
        }
    }
}

/// Runs the user's git and records each run in the operation log.
pub(crate) struct GitRunner {
    program: GitProgram,
    host: HostEnvironment,
    log: Arc<OperationLog>,
    use_credential_helpers: bool,
    can_read_trace: bool,
}

/// One finished git run.
pub(crate) struct Invocation {
    pub success: bool,
    /// What git wrote, as it wrote it, for gikli to parse. It goes nowhere else, as it can hold a
    /// password in a URL.
    pub stdout: Vec<u8>,
    /// What git wrote, with passwords in URLs hidden. Part of it can reach the user.
    pub stderr: String,
    /// The hook that failed in this run, as git's trace of the run names it.
    pub failed_hook: Option<String>,
    /// Whether a remote refused the credentials git gave it in this run.
    pub credentials_refused: bool,
    /// Whether the user cancelled this run, which ended it. It is not a failure of git's, whatever
    /// its output says.
    pub cancelled: bool,
    /// The last hook this run started, when it was cancelled.
    pub last_hook: Option<String>,
    /// The hook this run was waiting on, when it was cancelled.
    pub interrupted_hook: Option<String>,
}

/// Where gikli's credential helper notes that a remote refused the credentials.
const REFUSED_CREDENTIALS_VARIABLE: &str = "GIKLI_CREDENTIALS_REFUSED";

/// gikli's credential helper, the last of them. git tells every helper to erase credentials a
/// remote refused, and this one creates an empty file instead. git's own sentence about it is in
/// the user's language, and tracing the HTTP headers is no option: git before 2.43 can write the
/// credentials to the trace. This helper reads nothing and keeps nothing.
const NOTE_REFUSED_CREDENTIALS: &str =
    "credential.helper=!f() { [ \"$1\" != erase ] || : > \"$GIKLI_CREDENTIALS_REFUSED\"; }; f";

/// In the environment of a test, this plays a machine where gikli cannot set up the reader of git's
/// trace. Nothing else sets it, and git does not get it.
const NO_TRACE_READER_VARIABLE: &str = "GIKLI_NO_TRACE_READER";

/// Variables that cross into WSL, so git there runs without a trace unless the forwarder gives it
/// one, runs without a terminal as it does natively, and leaves what it reports about each run in
/// files gikli can read: `/p` translates a path.
const WSL_SHARED_VARIABLES: &[&str] = &[
    "GIT_TERMINAL_PROMPT",
    "GIT_OPTIONAL_LOCKS",
    "GIT_TRACE2_EVENT",
    "GIKLI_CREDENTIALS_REFUSED/p",
];

impl GitRunner {
    pub fn new(program: GitProgram, mut host: HostEnvironment, log: Arc<OperationLog>) -> Self {
        for name in REDIRECTING_VARIABLES {
            host.remove(*name);
        }
        let can_read_trace = host.get(NO_TRACE_READER_VARIABLE).is_none();
        host.remove(NO_TRACE_READER_VARIABLE);
        // No terminal: a hook or credential prompt that needs one fails instead of hanging.
        host.set("GIT_TERMINAL_PROMPT", "0");
        host.set("GIT_OPTIONAL_LOCKS", "0");
        if matches!(program, GitProgram::Wsl { .. }) {
            let mut shared: OsString = host.get("WSLENV").unwrap_or_default().to_owned();
            for name in WSL_SHARED_VARIABLES
                .iter()
                .chain(prompts::WSL_SHARED_VARIABLES)
            {
                if !shared.is_empty() {
                    shared.push(":");
                }
                shared.push(name);
            }
            host.set("WSLENV", shared);
        }
        Self {
            program,
            host,
            log,
            use_credential_helpers: true,
            can_read_trace,
        }
    }

    /// This runner, where git uses none of the credential helpers the user configured.
    pub fn without_credential_helpers(mut self) -> Self {
        self.use_credential_helpers = false;
        self
    }

    /// Whether git runs as a process of this machine, one gikli can see. The git inside WSL does
    /// not.
    pub fn runs_natively(&self) -> bool {
        matches!(self.program, GitProgram::Native(_))
    }

    /// Runs git with `args` in `folder`, for a command nobody needs to see or stop.
    pub fn run(&self, folder: &Path, args: &[&str]) -> std::io::Result<Invocation> {
        let (command, entry, trace) = self.command(folder, args);
        self.run_to_the_end(command, entry, trace)
    }

    /// Runs git with `args` in `folder` as [`Self::run`] does, with git writing its messages in its
    /// own language whatever the user's is, for a failure no fixed part of the output tells apart.
    fn run_untranslated(&self, folder: &Path, args: &[&str]) -> std::io::Result<Invocation> {
        let (mut command, entry, trace) = self.command(folder, args);
        command.env("LC_ALL", "C");
        if matches!(self.program, GitProgram::Wsl { .. }) {
            let mut shared = self.host.get("WSLENV").unwrap_or_default().to_owned();
            shared.push(":LC_ALL");
            command.env("WSLENV", shared);
        }
        self.run_to_the_end(command, entry, trace)
    }

    /// Runs `command`, which nobody needs to see or stop.
    fn run_to_the_end(
        &self,
        mut command: Command,
        entry: OperationLogEntry,
        mut trace: RunTrace,
    ) -> std::io::Result<Invocation> {
        let output = command.output().map(|mut output| {
            let mut stderr = trace.take_events_from(&output.stderr);
            stderr.extend(trace.stderr_ended());
            output.stderr = stderr;
            output
        });
        self.finish(entry, output, trace, &Cancellation::default())
    }

    /// Runs git with `args` in `folder` as the long operation `operation`, which it tells of each
    /// line of progress and of the hook it waits on as they come, and which can end it. `args` must
    /// ask git for progress (`--progress`) for any to come, as git reports none without a terminal
    /// otherwise.
    ///
    /// The git inside WSL cannot be ended this way, so it runs as a plain process, and `operation`
    /// must not offer to cancel it.
    pub fn run_operation(
        &self,
        folder: &Path,
        args: &[&str],
        operation: &OperationRun,
    ) -> std::io::Result<Invocation> {
        let (mut command, entry, mut trace) = self.command(folder, args);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let cancellation = operation.cancellation();
        let started = if self.runs_natively() {
            process_tree::spawn_contained(&mut command, cancellation)
        } else {
            command.spawn()
        };
        let output = started.and_then(|child| watch_run(child, &mut trace, operation));
        self.finish(entry, output, trace, cancellation)
    }

    /// The command that runs git with `args` in `folder`, its operation log entry so far, and
    /// where git traces the run.
    fn command(&self, folder: &Path, args: &[&str]) -> (Command, OperationLogEntry, RunTrace) {
        let mut command_line = vec!["--no-pager"];
        for setting in PINNED_CONFIG {
            command_line.extend(["-c", setting]);
        }
        if !self.use_credential_helpers {
            // An empty value empties the list of helpers so far, and this one comes last.
            command_line.extend(["-c", "credential.helper="]);
        }
        command_line.extend(["-c", NOTE_REFUSED_CREDENTIALS]);
        command_line.extend(args);

        let trace = RunTrace::new(&self.program, self.can_read_trace);
        let mut command = self.program.command(folder, trace.forwarding_marker());
        command
            .args(&command_line)
            .env_clear()
            .envs(self.host.vars())
            .stdin(Stdio::null());
        hide_console_window(&mut command);
        detach_from_terminal(&mut command);
        command.env("GIT_TRACE2_EVENT", &trace.git_target);
        command.env(
            REFUSED_CREDENTIALS_VARIABLE,
            &trace.refused_credentials_file,
        );
        let entry = OperationLogEntry {
            id: 0,
            folder: folder.to_path_buf(),
            args: command_line
                .iter()
                .map(|arg| hide_url_passwords(arg))
                .collect(),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: None,
            cancelled: false,
        };
        (command, entry, trace)
    }

    /// Records the run described by `entry`, which ended with `output`, or could not start.
    fn finish(
        &self,
        mut entry: OperationLogEntry,
        output: std::io::Result<Output>,
        mut trace: RunTrace,
        cancellation: &Cancellation,
    ) -> std::io::Result<Invocation> {
        let Output {
            status,
            stdout,
            stderr,
        } = match output {
            Ok(output) => output,
            Err(error) => {
                entry.error = Some(error.to_string());
                self.log.record(entry);
                return Err(error);
            }
        };
        trace.run_ended();
        let stderr = hide_url_passwords(&String::from_utf8_lossy(&stderr));
        let cancelled = cancellation.ended_run_with(status);
        entry.cancelled = cancelled;
        entry.exit_code = status.code();
        entry.stdout = hide_url_passwords(&String::from_utf8_lossy(&stdout));
        entry.stderr = stderr.clone();
        self.log.record(entry);
        Ok(Invocation {
            success: status.success(),
            stdout,
            stderr,
            failed_hook: trace.failed_hook(),
            credentials_refused: trace.refused_credentials_file.exists(),
            cancelled,
            last_hook: trace.last_hook().filter(|_| cancelled),
            interrupted_hook: trace.running_hook().filter(|_| cancelled),
        })
    }

    /// The git directory of the repository `folder` is in, or None when git cannot say. Inside
    /// the working tree for most repositories; elsewhere for a linked worktree or a submodule.
    pub fn git_dir(&self, folder: &Path) -> std::io::Result<Option<PathBuf>> {
        Ok(self
            .relative_git_dir(folder)?
            .map(|git_dir| folder.join(git_dir)))
    }

    /// The git directory of the repository `folder` is in, relative to `folder` as git writes it, or
    /// None when git cannot say. Such a path needs no translation from WSL's Linux paths, so git
    /// inside WSL can also be given one.
    pub fn relative_git_dir(&self, folder: &Path) -> std::io::Result<Option<String>> {
        let invocation = self.run(
            folder,
            &["rev-parse", "--path-format=relative", "--git-dir"],
        )?;
        let output = invocation
            .stdout
            .strip_suffix(b"\n")
            .unwrap_or(&invocation.stdout);
        Ok(invocation
            .success
            .then(|| path_from_bytes(output).to_string_lossy().into_owned()))
    }

    /// The root of the repository `folder` is in, as Windows sees it, or the run of git that could
    /// not give one, which says why in words [`crate::explainer`] can read.
    pub fn toplevel(&self, folder: &Path) -> std::io::Result<Result<PathBuf, Invocation>> {
        let invocation = self.run_untranslated(folder, self.program.toplevel_args())?;
        Ok(if invocation.success {
            Ok(self.program.toplevel(folder, &invocation.stdout))
        } else {
            Err(invocation)
        })
    }

    /// Whether git says that `folder` is in a repository and outside any working tree, as a bare
    /// repository and the inside of a git directory are.
    pub fn is_outside_work_tree(&self, folder: &Path) -> std::io::Result<bool> {
        let invocation = self.run(folder, &["rev-parse", "--is-inside-work-tree"])?;
        Ok(invocation.success && invocation.stdout == b"false\n")
    }
}

/// How often gikli looks at the trace of a running git for the hook it waits on.
const TRACE_READ_INTERVAL: Duration = Duration::from_millis(100);

/// Follows the run of `child`, traced to `trace`, until it ends, and returns what it wrote. A
/// program that holds git's output open after the run was cancelled, having escaped the cancel,
/// no longer holds this back.
fn watch_run(
    mut child: Child,
    trace: &mut RunTrace,
    operation: &OperationRun,
) -> std::io::Result<Output> {
    let (sender, pipes) = mpsc::channel();
    read_on_its_own_thread(
        child.stdout.take().expect("stdout is piped"),
        Piped::Stdout,
        sender.clone(),
    );
    read_on_its_own_thread(
        child.stderr.take().expect("stderr is piped"),
        Piped::Stderr,
        sender,
    );
    let cancellation = operation.cancellation();
    let mut stdout = Vec::new();
    let mut stderr = ProgressLines::default();
    let mut open_pipes = 2;
    let mut hook = None;
    let mut next_trace_read = Instant::now();
    let mut give_up = None;
    while open_pipes > 0 {
        match pipes.recv_timeout(TRACE_READ_INTERVAL) {
            Ok(Piped::Stdout(bytes)) => stdout.extend_from_slice(&bytes),
            Ok(Piped::Stderr(bytes)) => stderr.add(&trace.take_events_from(&bytes), operation),
            Ok(Piped::Closed) => open_pipes -= 1,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        if now >= next_trace_read {
            tell_hook(trace, &mut hook, operation);
            next_trace_read = now + TRACE_READ_INTERVAL;
        }
        if cancellation.is_requested() && now >= *give_up.get_or_insert(now + process_tree::GRACE) {
            break;
        }
    }
    stderr.add(&trace.stderr_ended(), operation);
    let status = cancellation.wait_for(&mut child)?;
    trace.run_ended();
    tell_hook(trace, &mut hook, operation);
    Ok(Output {
        status,
        stdout,
        stderr: stderr.finish(operation),
    })
}

/// Tells `operation` of the hook `trace` says runs now, when it is not `hook`, the last one told.
fn tell_hook(trace: &RunTrace, hook: &mut Option<String>, operation: &OperationRun) {
    let running = trace.running_hook();
    if running != *hook {
        operation.report_hook(running.clone());
        *hook = running;
    }
}

/// Something that came out of the pipes of a running git.
enum Piped {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Closed,
}

/// Reads `pipe` on a thread of its own, so git never blocks on one pipe while gikli reads another,
/// and so gikli can stop waiting for a pipe that stays open. `piped` makes what it sends of what
/// it read, and the thread ends when the pipe closes or nobody listens any more.
fn read_on_its_own_thread(
    mut pipe: impl Read + Send + 'static,
    piped: fn(Vec<u8>) -> Piped,
    sender: Sender<Piped>,
) {
    std::thread::spawn(move || {
        let mut chunk = [0; 4096];
        while let Ok(read) = pipe.read(&mut chunk)
            && read > 0
        {
            if sender.send(piped(chunk[..read].to_vec())).is_err() {
                return;
            }
        }
        let _ = sender.send(Piped::Closed);
    });
}

/// git's error output so far, whose lines of progress go on to a long operation as they end. git
/// redraws a progress line after a carriage return.
#[derive(Default)]
struct ProgressLines {
    captured: Vec<u8>,
    line_start: usize,
}

impl ProgressLines {
    fn add(&mut self, bytes: &[u8], operation: &OperationRun) {
        self.captured.extend_from_slice(bytes);
        while let Some(end) = self.captured[self.line_start..]
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n'))
        {
            let line = &self.captured[self.line_start..self.line_start + end];
            report_progress_in(line, operation);
            self.line_start += end + 1;
        }
    }

    /// The whole output, once nothing more comes.
    fn finish(self, operation: &OperationRun) -> Vec<u8> {
        report_progress_in(&self.captured[self.line_start..], operation);
        self.captured
    }
}

/// Tells `operation` of the progress in this line of git's error output, when there is any.
fn report_progress_in(line: &[u8], operation: &OperationRun) {
    if let Some(progress) = Progress::parse(&hide_url_passwords(&String::from_utf8_lossy(line))) {
        operation.report(progress);
    }
}

impl Invocation {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// This run of `git clone`, with only what a hook wrote in its error output, as far as that
    /// can be told: without the line that names the folder, the first one, in the user's language,
    /// and without the progress of git and of the remote.
    pub fn without_clone_reports(&self) -> Self {
        let mut kept = String::new();
        for line in self.stderr.split_inclusive('\n').skip(1) {
            let text = line.trim_end_matches(['\r', '\n']);
            // git redraws a progress line after a carriage return.
            let last_drawn = text.rsplit('\r').next().unwrap_or(text);
            if !last_drawn.starts_with("remote: ") && Progress::parse(last_drawn).is_none() {
                kept.push_str(last_drawn);
                kept.push('\n');
            }
        }
        Self {
            success: self.success,
            stdout: self.stdout.clone(),
            stderr: kept,
            failed_hook: self.failed_hook.clone(),
            credentials_refused: self.credentials_refused,
            cancelled: self.cancelled,
            last_hook: self.last_hook.clone(),
            interrupted_hook: self.interrupted_hook.clone(),
        }
    }
}

/// What git reports about one run outside its messages, which is the same in every language. It
/// is the only place that tells which hook runs or failed, as git itself says nothing about it.
struct RunTrace {
    /// What git's trace of the run has said so far, without its command lines. Empty for a run
    /// without a trace, which then reports no hook.
    hooks: Arc<Mutex<HookRuns>>,
    source: TraceSource,
    /// What git gets as its target for the trace.
    git_target: OsString,
    /// The empty file gikli's credential helper creates when a remote refuses the credentials.
    refused_credentials_file: PathBuf,
}

/// Where the trace of a run reaches gikli.
enum TraceSource {
    /// The run has no trace.
    None,
    /// A git on this machine writes to a reader.
    Reader(TraceReader),
    /// The git inside WSL has its events forwarded on its error output.
    Forwarded(ForwardedEvents),
}

impl RunTrace {
    /// The trace of a run of `program`. When it cannot be read, git runs without one, as it can do
    /// its work all the same, and gikli does not fall back to a file.
    fn new(program: &GitProgram, can_read: bool) -> Self {
        let hooks = Arc::new(Mutex::new(HookRuns::default()));
        let mut source = TraceSource::None;
        let mut git_target = OsString::from(trace::NO_TRACE);
        if can_read {
            match program {
                GitProgram::Native(_) => {
                    if let Ok((reader, target)) = TraceReader::start(hooks.clone()) {
                        source = TraceSource::Reader(reader);
                        git_target = target;
                    }
                }
                // The forwarder gives git its target, and git runs with this one without it.
                GitProgram::Wsl { .. } => {
                    if let Ok(forwarded) = ForwardedEvents::new(hooks.clone()) {
                        source = TraceSource::Forwarded(forwarded);
                    }
                }
            }
        }
        let refused_credentials_file =
            std::env::temp_dir().join(format!("{}.refused", trace::new_run_name()));
        Self {
            hooks,
            source,
            git_target,
            refused_credentials_file,
        }
    }

    /// The marker of the events the git inside WSL forwards in its error output.
    fn forwarding_marker(&self) -> Option<&str> {
        match &self.source {
            TraceSource::Forwarded(forwarded) => Some(forwarded.marker()),
            _ => None,
        }
    }

    /// `stderr`, a piece of git's error output, without the events in it.
    fn take_events_from(&mut self, stderr: &[u8]) -> Vec<u8> {
        match &mut self.source {
            TraceSource::Forwarded(forwarded) => forwarded.take_from(stderr),
            _ => stderr.to_vec(),
        }
    }

    /// What is left of git's error output now that it has ended.
    fn stderr_ended(&mut self) -> Vec<u8> {
        match &mut self.source {
            TraceSource::Forwarded(forwarded) => forwarded.finish(),
            _ => Vec::new(),
        }
    }

    /// Takes what is left of the trace, once git has ended. A process a hook started that is still
    /// running does not hold this back.
    fn run_ended(&mut self) {
        if let TraceSource::Reader(reader) = &mut self.source {
            reader.finish();
        }
    }

    /// The hook that ended with an error in the run, as the git gikli started saw it.
    fn failed_hook(&self) -> Option<String> {
        lock(&self.hooks).failed()
    }

    /// The hook running now, as the git gikli started sees it.
    fn running_hook(&self) -> Option<String> {
        lock(&self.hooks).running()
    }

    /// The last hook the git gikli started has started, running or not.
    fn last_hook(&self) -> Option<String> {
        lock(&self.hooks).last()
    }
}

impl Drop for RunTrace {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.refused_credentials_file);
    }
}

#[cfg(unix)]
pub(crate) fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

/// git for Windows writes paths as UTF-8.
#[cfg(not(unix))]
pub(crate) fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// gikli is a windowed app: on Windows, git would otherwise flash a console window.
#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

/// Runs git in a session of its own, with no controlling terminal. A hook that opens `/dev/tty`
/// then fails at once, even when gikli was started from a terminal, where it would otherwise wait
/// for an answer nobody knows it asked for.
#[cfg(unix)]
fn detach_from_terminal(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: `setsid` is async-signal-safe and touches no memory of the parent.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// On Windows, `/dev/tty` fails by itself in a hook, as no terminal is attached. A native program
/// that reads the console directly (`CONIN$`) is the exception: the shell that runs the hook gives
/// it a hidden console, whatever flags git starts with, and it waits there until it is killed.
/// gikli keeps answering meanwhile and cannot tell it from a hook that takes long, so it leaves it
/// to the user to cancel the operation.
#[cfg(not(unix))]
fn detach_from_terminal(_command: &mut Command) {}

/// Finds git and checks its version. The order is the `configured` path when there is one, then
/// git on PATH, then on Windows the git inside WSL.
pub(crate) fn check(
    configured: Option<PathBuf>,
    host: &HostEnvironment,
    log: &Arc<OperationLog>,
) -> GitCheck {
    if let Some(executable) = configured {
        return check_executable(executable, GitSource::Configured, host, log);
    }
    if let Some(executable) = find_on_path(host, "git") {
        return check_executable(executable, GitSource::Path, host, log);
    }
    if cfg!(windows)
        && let Some(wsl) = find_on_path(host, "wsl")
    {
        return check_executable(wsl, GitSource::Wsl, host, log);
    }
    GitCheck::NotFound {
        minimum: MINIMUM_GIT_VERSION,
    }
}

/// Finds `wsl.exe` and checks the version of the git inside the distribution of `share`.
pub(crate) fn check_wsl(
    share: &WslShare,
    host: &HostEnvironment,
    log: &Arc<OperationLog>,
) -> GitCheck {
    let Some(wsl) = find_on_path(host, "wsl") else {
        return GitCheck::NotFound {
            minimum: MINIMUM_GIT_VERSION,
        };
    };
    let program = GitProgram::Wsl {
        wsl: wsl.clone(),
        share: Some(share.clone()),
    };
    check_program(program, wsl, GitSource::Wsl, host, log)
}

/// Checks the version of the git at `executable`, found from `source`.
pub(crate) fn check_executable(
    executable: PathBuf,
    source: GitSource,
    host: &HostEnvironment,
    log: &Arc<OperationLog>,
) -> GitCheck {
    let Some(executable) = executable_file(&executable) else {
        return GitCheck::Missing { executable };
    };
    let program = GitProgram::found(executable.clone(), source);
    check_program(program, executable, source, host, log)
}

/// Runs `git --version` with `program` and checks the version. `executable` and `source` are
/// what the result reports.
fn check_program(
    program: GitProgram,
    executable: PathBuf,
    source: GitSource,
    host: &HostEnvironment,
    log: &Arc<OperationLog>,
) -> GitCheck {
    let runner = GitRunner::new(program, host.clone(), log.clone());
    let version = runner
        .run(&std::env::temp_dir(), &["--version"])
        .ok()
        .filter(|output| output.success)
        .and_then(|output| GitVersion::parse(&output.stdout_text()));
    match version {
        None => GitCheck::Unrecognized { executable, source },
        Some(version) if version < MINIMUM_GIT_VERSION => GitCheck::TooOld {
            executable,
            source,
            version,
            minimum: MINIMUM_GIT_VERSION,
        },
        Some(version) => GitCheck::Ready {
            executable,
            source,
            version,
        },
    }
}

/// The file `path` names: `path` itself, or on Windows `path` with `.exe` when the user left it
/// out, as a terminal allows.
fn executable_file(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let suffix = std::env::consts::EXE_SUFFIX;
    if suffix.is_empty() || path.extension().is_some() {
        return None;
    }
    let mut with_suffix = path.as_os_str().to_owned();
    with_suffix.push(suffix);
    let with_suffix = PathBuf::from(with_suffix);
    with_suffix.is_file().then_some(with_suffix)
}

/// Finds the executable `program` in the folders of PATH.
fn find_on_path(host: &HostEnvironment, program: &str) -> Option<PathBuf> {
    let name: OsString = format!("{program}{}", std::env::consts::EXE_SUFFIX).into();
    host.path_folders()
        .into_iter()
        .map(|folder| folder.join(&name))
        .find(|candidate| candidate.is_file())
}
