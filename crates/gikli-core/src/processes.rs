//! The git processes running on this machine, whoever started them: gikli, a terminal or an IDE.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

/// How long a process gets to show where it runs, when it has only just started.
const SETTLE: Duration = Duration::from_millis(100);

/// Whether a git process may be running at or inside one of `folders`: its working folder is
/// there, or one of its arguments names a place there, as `git -C` does.
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
        .with_cmd(UpdateKind::Always);
    let Ok(mut system) = System::new() else {
        return true;
    };
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
    let unreadable = match gits_in(&system, &folders) {
        Gits::InFolders => return true,
        Gits::Unreadable(unreadable) if unreadable.is_empty() => return false,
        Gits::Unreadable(unreadable) => unreadable,
    };
    // A process shows its working folder only once it has started, and most of these belong to
    // another run of git that is about to end. The ones still unreadable later really are.
    std::thread::sleep(SETTLE);
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&unreadable), true, refresh);
    match gits_in(&system, &folders) {
        Gits::InFolders => true,
        Gits::Unreadable(unreadable) => !unreadable.is_empty(),
    }
}

/// What the processes of `system` say about git running in some folders.
enum Gits {
    InFolders,
    /// None is known to run there; these git processes do not say where they run.
    Unreadable(Vec<Pid>),
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
        if process
            .cmd()
            .iter()
            .any(|argument| names_a_place_in(Path::new(argument), folders))
        {
            return Gits::InFolders;
        }
        match process.cwd() {
            Some(cwd) if names_a_place_in(cwd, folders) => return Gits::InFolders,
            Some(_) => {}
            None => unreadable.push(*pid),
        }
    }
    Gits::Unreadable(unreadable)
}

fn is_git(name: &OsStr) -> bool {
    name.eq_ignore_ascii_case("git") || name.eq_ignore_ascii_case("git.exe")
}

/// Whether the arguments are those of git's file system monitor, which runs for as long as the
/// user wants it to and never takes the lock of the index.
fn is_daemon(arguments: &[std::ffi::OsString]) -> bool {
    arguments
        .iter()
        .any(|argument| argument == "fsmonitor--daemon")
}

fn names_a_place_in(path: &Path, folders: &[PathBuf]) -> bool {
    let path = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    folders.iter().any(|folder| path.starts_with(folder))
}
