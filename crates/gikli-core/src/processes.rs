//! The git processes running on this machine, whoever started them: gikli, a terminal or an IDE.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sysinfo::{
    Pid, Process, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind,
};

/// The variables that send git to a repository other than the one its working folder is in.
const REDIRECTING_VARIABLES: &[&str] = &["GIT_DIR=", "GIT_WORK_TREE=", "GIT_INDEX_FILE="];

/// How long a process gets to show where it runs, when it has only just started.
const SETTLE: Duration = Duration::from_millis(100);

/// Whether a git process may be running at or inside one of `folders`: its working folder is
/// there, or one of its arguments or variables names a place there, as `git -C`,
/// `--git-dir=<path>` and `GIT_DIR` do.
///
/// When in doubt the answer is yes. A git whose working folder gikli cannot read, because it
/// belongs to another user or an administrator, counts as running there, and so does any git when
/// gikli cannot list the processes at all.
pub(crate) fn git_runs_in(folders: &[&Path]) -> bool {
    let folders: Vec<PathBuf> = folders
        .iter()
        .map(|folder| dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf()))
        .collect();
    let refresh = ProcessRefreshKind::nothing()
        .with_cwd(UpdateKind::Always)
        .with_cmd(UpdateKind::Always)
        .with_environ(UpdateKind::Always);
    let Ok(mut system) = System::new() else {
        return true;
    };
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
    let unreadable = match gits_in(&system, &folders) {
        Gits::InFolders => return true,
        Gits::NotSeenThere { unreadable } if unreadable.is_empty() => return false,
        Gits::NotSeenThere { unreadable } => unreadable,
    };
    // A process shows its working folder only once it has started, and most of these belong to
    // another run of git that is about to end. The ones still unreadable later really are.
    std::thread::sleep(SETTLE);
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&unreadable), true, refresh);
    match gits_in(&system, &folders) {
        Gits::InFolders => true,
        Gits::NotSeenThere { unreadable } => !unreadable.is_empty(),
    }
}

/// What the processes of a system say about git running in some folders.
enum Gits {
    InFolders,
    /// No git says it runs there, and the ones in `unreadable` do not say where they run.
    NotSeenThere {
        unreadable: Vec<Pid>,
    },
}

fn gits_in(system: &System, folders: &[PathBuf]) -> Gits {
    let mut unreadable = Vec::new();
    for (pid, process) in system.processes() {
        if !is_git(process.name())
            || process.status() == ProcessStatus::Zombie
            || is_daemon(process.cmd())
        {
            continue;
        }
        if places_named_by(process).any(|place| is_in(&place, process.cwd(), folders)) {
            return Gits::InFolders;
        }
        match process.cwd() {
            Some(cwd) if is_in(cwd, None, folders) => return Gits::InFolders,
            Some(_) => {}
            None => unreadable.push(*pid),
        }
    }
    Gits::NotSeenThere { unreadable }
}

/// What the arguments and the variables of `process` may name as a place: each argument, with what
/// follows the `=` of an option such as `--git-dir=<path>`, and the variables that redirect git.
fn places_named_by(process: &Process) -> impl Iterator<Item = PathBuf> {
    let arguments = process.cmd().iter().map(|argument| {
        let argument = argument.to_string_lossy();
        let value = argument
            .split_once('=')
            .map_or(&*argument, |(_, value)| value);
        PathBuf::from(value)
    });
    let variables = process.environ().iter().filter_map(|variable| {
        let variable = variable.to_string_lossy();
        REDIRECTING_VARIABLES
            .iter()
            .find_map(|name| variable.strip_prefix(name))
            .map(PathBuf::from)
    });
    arguments.chain(variables)
}

fn is_git(name: &OsStr) -> bool {
    name.eq_ignore_ascii_case("git") || name.eq_ignore_ascii_case("git.exe")
}

/// Whether the arguments are those of git's file system monitor, which runs for as long as the
/// user wants it to and never takes the lock of the index.
fn is_daemon(arguments: &[OsString]) -> bool {
    arguments
        .iter()
        .any(|argument| argument == "fsmonitor--daemon")
}

/// Whether `path`, relative to `base` when it is relative, is at or inside one of `folders`.
fn is_in(path: &Path, base: Option<&Path>, folders: &[PathBuf]) -> bool {
    let path = base.map_or_else(|| path.to_path_buf(), |base| base.join(path));
    let path = dunce::canonicalize(&path).unwrap_or(path);
    folders.iter().any(|folder| path.starts_with(folder))
}
