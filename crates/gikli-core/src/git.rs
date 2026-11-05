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

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::host::HostEnvironment;
use crate::operation_log::{OperationLog, OperationLogEntry};
use crate::operations::Progress;
use crate::prompts;
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

    /// The command that starts git in `folder`, before its arguments.
    fn command(&self, folder: &Path) -> Command {
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
                command.args(["--exec", "git"]);
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
            // steps, needs no translation.
            Self::Wsl { .. } => &["rev-parse", "--show-cdup"],
        }
    }

    /// The root of the repository `folder` is in, from the output of [`Self::toplevel_args`].
    fn toplevel(&self, folder: &Path, output: &[u8]) -> PathBuf {
        let output = output.strip_suffix(b"\n").unwrap_or(output);
        match self {
            Self::Native(_) => path_from_bytes(output),
            Self::Wsl { .. } => folder.join(String::from_utf8_lossy(output).as_ref()),
        }
    }
}

/// Runs the user's git and records each run in the operation log.
pub(crate) struct GitRunner {
    program: GitProgram,
    host: HostEnvironment,
    log: Arc<OperationLog>,
    use_credential_helpers: bool,
}

/// One finished git run.
pub(crate) struct Invocation {
    pub success: bool,
    /// What git wrote, as it wrote it, for gikli to parse. It goes nowhere else, as it can hold a
    /// password in a URL.
    pub stdout: Vec<u8>,
    /// What git wrote, with passwords in URLs hidden. Part of it can reach the user.
    pub stderr: Vec<u8>,
    /// The hook that failed in this run, as git's trace of the run names it.
    pub failed_hook: Option<String>,
    /// Whether a remote refused the credentials git gave it in this run.
    pub credentials_refused: bool,
}

/// Where gikli's credential helper notes that a remote refused the credentials.
const REFUSED_CREDENTIALS_VARIABLE: &str = "GIKLI_CREDENTIALS_REFUSED";

/// gikli's credential helper, the last of them. git tells every helper to erase credentials a
/// remote refused, and this one creates an empty file instead. git's own sentence about it is in
/// the user's language, and tracing the HTTP headers is no option: git before 2.43 can write the
/// credentials to the trace. This helper reads nothing and keeps nothing.
const NOTE_REFUSED_CREDENTIALS: &str =
    "credential.helper=!f() { [ \"$1\" != erase ] || : > \"$GIKLI_CREDENTIALS_REFUSED\"; }; f";

/// Variables that cross into WSL, so git there runs without a terminal as it does natively, and
/// leaves what it reports about each run in files gikli can read: `/p` translates a path.
const WSL_SHARED_VARIABLES: &[&str] = &[
    "GIT_TERMINAL_PROMPT",
    "GIT_OPTIONAL_LOCKS",
    "GIT_TRACE2_EVENT/p",
    "GIKLI_CREDENTIALS_REFUSED/p",
];

impl GitRunner {
    pub fn new(program: GitProgram, mut host: HostEnvironment, log: Arc<OperationLog>) -> Self {
        for name in REDIRECTING_VARIABLES {
            host.remove(*name);
        }
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

    /// Runs git with `args` in `folder`.
    pub fn run(&self, folder: &Path, args: &[&str]) -> std::io::Result<Invocation> {
        let (mut command, entry, trace) = self.command(folder, args);
        let output = command.output();
        self.finish(entry, output, trace)
    }

    /// Runs git with `args` in `folder`, as [`Self::run`] does, and passes each line of progress
    /// git writes to `report` as it comes. `args` must ask git for progress (`--progress`), as
    /// git reports none without a terminal otherwise.
    pub fn run_reporting(
        &self,
        folder: &Path,
        args: &[&str],
        report: &dyn Fn(Progress),
    ) -> std::io::Result<Invocation> {
        let (mut command, entry, trace) = self.command(folder, args);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let output = command.spawn().and_then(|mut child| {
            let mut stdout = child.stdout.take().expect("stdout is piped");
            // Read apart, so git never blocks on a full stdout while gikli reads its stderr.
            let stdout = std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = stdout.read_to_end(&mut bytes);
                bytes
            });
            let stderr = read_progress(child.stderr.take().expect("stderr is piped"), report);
            let status = child.wait()?;
            Ok(Output {
                status,
                stdout: stdout.join().unwrap_or_default(),
                stderr,
            })
        });
        self.finish(entry, output, trace)
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

        let mut command = self.program.command(folder);
        command
            .args(&command_line)
            .env_clear()
            .envs(self.host.vars())
            .stdin(Stdio::null());
        hide_console_window(&mut command);
        detach_from_terminal(&mut command);
        let trace = RunTrace::new();
        command.env("GIT_TRACE2_EVENT", &trace.file);
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
        };
        (command, entry, trace)
    }

    /// Records the run described by `entry`, which ended with `output`, or could not start.
    fn finish(
        &self,
        mut entry: OperationLogEntry,
        output: std::io::Result<Output>,
        trace: RunTrace,
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
        let stderr = hide_url_passwords(&String::from_utf8_lossy(&stderr));
        entry.exit_code = status.code();
        entry.stdout = hide_url_passwords(&String::from_utf8_lossy(&stdout));
        entry.stderr = stderr.clone();
        self.log.record(entry);
        Ok(Invocation {
            success: status.success(),
            stdout,
            stderr: stderr.into_bytes(),
            failed_hook: trace.failed_hook(),
            credentials_refused: trace.refused_credentials_file.exists(),
        })
    }

    /// The git directory of the repository `folder` is in, or None when git cannot say. Inside
    /// the working tree for most repositories; elsewhere for a linked worktree or a submodule.
    pub fn git_dir(&self, folder: &Path) -> std::io::Result<Option<PathBuf>> {
        // A path relative to `folder` needs no translation from WSL's Linux paths.
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
            .then(|| folder.join(path_from_bytes(output))))
    }

    /// The root of the repository `folder` is in, as Windows sees it, or None when `folder` is
    /// not in a repository.
    pub fn toplevel(&self, folder: &Path) -> std::io::Result<Option<PathBuf>> {
        let invocation = self.run(folder, self.program.toplevel_args())?;
        Ok(invocation
            .success
            .then(|| self.program.toplevel(folder, &invocation.stdout)))
    }
}

/// Reads git's error output to its end and returns it, passing each line of progress in it to
/// `report`. git redraws a progress line after a carriage return.
fn read_progress(mut stderr: impl Read, report: &dyn Fn(Progress)) -> Vec<u8> {
    let mut captured = Vec::new();
    let mut line_start = 0;
    let mut chunk = [0; 4096];
    while let Ok(read) = stderr.read(&mut chunk)
        && read > 0
    {
        captured.extend_from_slice(&chunk[..read]);
        while let Some(end) = captured[line_start..]
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n'))
        {
            let line = String::from_utf8_lossy(&captured[line_start..line_start + end]);
            if let Some(progress) = Progress::parse(&line) {
                report(progress);
            }
            line_start += end + 1;
        }
    }
    if let Some(progress) = Progress::parse(&String::from_utf8_lossy(&captured[line_start..])) {
        report(progress);
    }
    captured
}

impl Invocation {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// This run of `git clone`, with only what a hook wrote in its error output, as far as that
    /// can be told: without the line that names the folder, the first one, in the user's language,
    /// and without the progress of git and of the remote.
    pub fn without_clone_reports(&self) -> Self {
        let stderr = String::from_utf8_lossy(&self.stderr);
        let mut kept = String::new();
        for line in stderr.split_inclusive('\n').skip(1) {
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
            stderr: kept.into_bytes(),
            failed_hook: self.failed_hook.clone(),
            credentials_refused: self.credentials_refused,
        }
    }
}

/// What git reports about one run outside its messages, which is the same in every language, in
/// files of its own. It is the only place that tells a hook failed, as git itself says nothing
/// about it.
struct RunTrace {
    /// The events of the run, in the event format of trace2.
    file: PathBuf,
    /// The empty file gikli's credential helper creates when a remote refuses the credentials.
    refused_credentials_file: PathBuf,
}

/// Tells apart the trace files of the runs of this process.
static NEXT_TRACE: AtomicU64 = AtomicU64::new(0);

impl RunTrace {
    fn new() -> Self {
        let number = NEXT_TRACE.fetch_add(1, Ordering::Relaxed);
        let name = format!("gikli-trace-{}-{number}", std::process::id());
        let folder = std::env::temp_dir();
        Self {
            file: folder.join(format!("{name}.json")),
            refused_credentials_file: folder.join(format!("{name}.refused")),
        }
    }

    /// The hook that ended with an error in the run, as the git gikli started saw it. A git the
    /// hook started traces to the same file, under a session nested with `/`.
    fn failed_hook(&self) -> Option<String> {
        let trace = std::fs::read_to_string(&self.file).ok()?;
        let events: Vec<serde_json::Value> = trace
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .filter(|event: &serde_json::Value| {
                event["sid"].as_str().is_some_and(|sid| !sid.contains('/'))
            })
            .collect();
        events.iter().find_map(|started| {
            let hook = started["hook_name"].as_str()?;
            let failed = events.iter().any(|ended| {
                ended["event"] == "child_exit"
                    && ended["child_id"] == started["child_id"]
                    && ended["code"] != 0
            });
            failed.then(|| hook.to_owned())
        })
    }
}

impl Drop for RunTrace {
    fn drop(&mut self) {
        // Absent when git traced nothing; left behind when a process the hook started still
        // holds it. The temporary folder takes care of it then.
        let _ = std::fs::remove_file(&self.file);
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
/// gikli keeps answering meanwhile, and cannot tell it from a hook that takes long.
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
