//! The git runner: the single way gikli runs git (ADR-0001).
//!
//! Every invocation runs the user's git executable with the user's configuration, except for the
//! settings that only change how output looks. Those are pinned, so a setting such as
//! `color.ui=always` or `core.quotePath=true` never changes what gikli parses. Commands that
//! gikli parses must still ask for machine-readable output themselves (porcelain formats, `-z`,
//! explicit `--format`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::events::{Event, EventBus, OperationLogEntry};
use crate::host::HostEnvironment;
use crate::version::GitVersion;
use crate::{GitCheck, MINIMUM_GIT_VERSION};

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
];

/// Runs the user's git executable and records each run in the operation log.
pub(crate) struct GitRunner {
    executable: PathBuf,
    host: HostEnvironment,
    events: Arc<EventBus>,
}

/// One finished git run.
pub(crate) struct Invocation {
    pub success: bool,
    pub stdout: Vec<u8>,
}

impl GitRunner {
    pub fn new(executable: PathBuf, mut host: HostEnvironment, events: Arc<EventBus>) -> Self {
        for name in REDIRECTING_VARIABLES {
            host.remove(*name);
        }
        // No terminal: a hook or credential prompt that needs one fails instead of hanging.
        host.set("GIT_TERMINAL_PROMPT", "0");
        host.set("GIT_OPTIONAL_LOCKS", "0");
        Self {
            executable,
            host,
            events,
        }
    }

    /// Runs git with `args` in `folder`.
    pub fn run(&self, folder: &Path, args: &[&str]) -> std::io::Result<Invocation> {
        let mut command_line = vec!["--no-pager"];
        for setting in PINNED_CONFIG {
            command_line.extend(["-c", setting]);
        }
        command_line.extend(args);

        let mut command = Command::new(&self.executable);
        command
            .args(&command_line)
            .current_dir(folder)
            .env_clear()
            .envs(self.host.vars())
            .stdin(Stdio::null());
        hide_console_window(&mut command);
        let output = command.output()?;
        self.events.send(Event::OperationLogged(OperationLogEntry {
            folder: folder.to_path_buf(),
            args: command_line.iter().map(|arg| (*arg).to_owned()).collect(),
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }));
        Ok(Invocation {
            success: output.status.success(),
            stdout: output.stdout,
        })
    }
}

impl Invocation {
    /// Standard output as a path, without the line ending git adds.
    pub fn stdout_path(&self) -> PathBuf {
        let bytes = self.stdout.strip_suffix(b"\n").unwrap_or(&self.stdout);
        path_from_bytes(bytes)
    }

    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

/// git for Windows writes paths as UTF-8.
#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
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

/// Finds git and checks its version.
pub(crate) fn check(host: &HostEnvironment, events: &Arc<EventBus>) -> GitCheck {
    let Some(executable) = find_on_path(host) else {
        return GitCheck::NotFound {
            minimum: MINIMUM_GIT_VERSION,
        };
    };
    let runner = GitRunner::new(executable.clone(), host.clone(), events.clone());
    let version = runner
        .run(&std::env::temp_dir(), &["--version"])
        .ok()
        .filter(|output| output.success)
        .and_then(|output| GitVersion::parse(&output.stdout_text()));
    match version {
        None => GitCheck::Unrecognized { executable },
        Some(version) if version < MINIMUM_GIT_VERSION => GitCheck::TooOld {
            executable,
            version,
            minimum: MINIMUM_GIT_VERSION,
        },
        Some(version) => GitCheck::Ready {
            executable,
            version,
        },
    }
}

/// Finds git in the folders of PATH.
fn find_on_path(host: &HostEnvironment) -> Option<PathBuf> {
    let name: OsString = format!("git{}", std::env::consts::EXE_SUFFIX).into();
    host.path_folders()
        .into_iter()
        .map(|folder| folder.join(&name))
        .find(|candidate| candidate.is_file())
}
