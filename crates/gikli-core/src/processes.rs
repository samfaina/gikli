//! The git processes running on this machine, whoever started them: gikli, a terminal or an IDE.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sysinfo::{
    Pid, Process, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind,
};

/// The variables that send git to a repository other than the one its working folder is in.
const REDIRECTING_VARIABLES: &[&str] = &["GIT_DIR=", "GIT_WORK_TREE=", "GIT_INDEX_FILE="];

/// How long a git that does not say where it runs gets to say it, between two looks.
const SETTLE: Duration = Duration::from_millis(100);

/// How many times a git that does not say where it runs is looked at again, after the first.
const LOOKS_AGAIN: usize = 5;

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
    git_may_run(|processes| {
        if let ProcessesToUpdate::Some(_) = processes {
            std::thread::sleep(SETTLE);
        }
        look(processes, &folders)
    })
}

/// Whether git may run in some folders, from what `look` says of the processes: all of them at
/// first, and then only the gits that did not say where they run, each time afresh. `look` says
/// nothing when it cannot list the processes.
///
/// Windows may refuse to open a process that is starting or ending, which is common on a busy
/// machine, so a git that does not say where it runs is looked at again a few times before it
/// counts as one gikli will never read.
fn git_may_run(mut look: impl FnMut(ProcessesToUpdate) -> Option<Gits>) -> bool {
    let mut unreadable = None;
    for _ in 0..=LOOKS_AGAIN {
        let processes = unreadable
            .as_deref()
            .map_or(ProcessesToUpdate::All, ProcessesToUpdate::Some);
        match look(processes) {
            None | Some(Gits::InFolders) => return true,
            Some(Gits::NotSeenThere { unreadable: left }) if left.is_empty() => return false,
            Some(Gits::NotSeenThere { unreadable: left }) => unreadable = Some(left),
        }
    }
    true
}

/// What `processes` say about git running in `folders`, as a new list of the processes sees them.
/// sysinfo never opens again a process it could not open once, and keeps the name it had when its
/// pid is given to another process, so each look needs a new list.
fn look(processes: ProcessesToUpdate, folders: &[PathBuf]) -> Option<Gits> {
    let refresh = ProcessRefreshKind::nothing()
        .with_cwd(UpdateKind::Always)
        .with_cmd(UpdateKind::Always)
        .with_environ(UpdateKind::Always);
    let mut system = System::new().ok()?;
    system.refresh_processes_specifics(processes, true, refresh);
    Some(gits_in(&system, folders))
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

/// These play looks at the processes rather than run git, as Windows refuses to open a process
/// that is starting or ending only by chance.
#[cfg(test)]
mod tests {
    use super::*;

    /// What `looks` say in turn, and the processes each look after the first was asked about.
    fn decide(looks: Vec<Option<Gits>>) -> (bool, Vec<Vec<Pid>>) {
        let mut looks = looks.into_iter();
        let mut asked = Vec::new();
        let runs = git_may_run(|processes| {
            if let ProcessesToUpdate::Some(pids) = processes {
                asked.push(pids.to_vec());
            }
            looks.next().expect("no more looks than the test gives")
        });
        (runs, asked)
    }

    /// A look where no git says it runs in the folders, and the gits in `pids` do not say where
    /// they run.
    fn not_seen_there_but(pids: &[u32]) -> Option<Gits> {
        Some(Gits::NotSeenThere {
            unreadable: pids.iter().copied().map(Pid::from_u32).collect(),
        })
    }

    #[test]
    fn a_git_unreadable_at_first_and_gone_at_the_next_look_does_not_run_there() {
        let (runs, asked) = decide(vec![not_seen_there_but(&[7, 9]), not_seen_there_but(&[])]);

        assert!(!runs);
        assert_eq!(asked, vec![vec![Pid::from_u32(7), Pid::from_u32(9)]]);
    }

    #[test]
    fn a_git_that_never_says_where_it_runs_counts_as_running_there() {
        let looks = (0..=LOOKS_AGAIN)
            .map(|_| not_seen_there_but(&[7]))
            .collect();

        let (runs, asked) = decide(looks);

        assert!(runs);
        assert_eq!(asked.len(), LOOKS_AGAIN);
    }

    #[test]
    fn a_git_gets_every_look_but_the_last_to_say_it_runs_elsewhere() {
        let mut looks: Vec<_> = (0..LOOKS_AGAIN)
            .map(|_| not_seen_there_but(&[7, 9]))
            .collect();
        looks.push(not_seen_there_but(&[]));

        let (runs, asked) = decide(looks);

        assert!(!runs);
        assert_eq!(asked.len(), LOOKS_AGAIN);
    }

    #[test]
    fn only_the_gits_still_unreadable_are_looked_at_again() {
        let (runs, asked) = decide(vec![
            not_seen_there_but(&[7, 9]),
            not_seen_there_but(&[9]),
            not_seen_there_but(&[]),
        ]);

        assert!(!runs);
        assert_eq!(
            asked,
            vec![
                vec![Pid::from_u32(7), Pid::from_u32(9)],
                vec![Pid::from_u32(9)]
            ]
        );
    }

    #[test]
    fn a_git_unreadable_at_first_that_then_says_it_runs_there_runs_there() {
        let (runs, _) = decide(vec![not_seen_there_but(&[7]), Some(Gits::InFolders)]);

        assert!(runs);
    }

    #[test]
    fn a_first_look_that_cannot_list_the_processes_counts_as_git_running_there() {
        let (runs, _) = decide(vec![None]);

        assert!(runs);
    }

    #[test]
    fn a_later_look_that_cannot_list_the_processes_counts_as_git_running_there() {
        let (runs, _) = decide(vec![not_seen_there_but(&[7]), None]);

        assert!(runs);
    }
}
