//! The conflict engine: the files an operation left in conflict, told from the user's side, and
//! the ways to resolve them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::{GitRunner, path_from_bytes};
use crate::in_progress::{self, OperationKind};
use crate::{Failure, Repository, git_failed_to_start};

/// A file in conflict, with what each side did to it. "Yours" is the user's side, the branch they
/// are on, whatever git calls it: during a rebase git calls it "theirs".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictedFile {
    /// The file's path relative to the Repository root, with `/` between folders, as git writes
    /// it.
    pub path: PathBuf,
    pub yours: SideChange,
    pub theirs: SideChange,
}

/// What one side of a conflict did to the file since the two sides parted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SideChange {
    /// The side changed the file.
    Modified,
    /// The side added the file, which was not there when the sides parted.
    Added,
    /// The side deleted the file, or moved it away.
    Deleted,
}

/// One side of a conflict, from the user's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictSide {
    /// The user's side: the branch they are on.
    Yours,
    /// The changes the operation brings in.
    Theirs,
}

/// The versions of a file in conflict that the index holds: the one the sides parted from, ours
/// and theirs, as git names them.
struct Unmerged {
    path: PathBuf,
    base: bool,
    ours: bool,
    theirs: bool,
}

impl Unmerged {
    /// What the side whose version is there when `present` did.
    fn change(&self, present: bool) -> SideChange {
        match (present, self.base) {
            (false, _) => SideChange::Deleted,
            (true, false) => SideChange::Added,
            (true, true) => SideChange::Modified,
        }
    }
}

/// The files in conflict in `repository`, ordered by path, told from the user's side.
pub(crate) fn read(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Vec<ConflictedFile>, Failure> {
    let inverted = yours_is_theirs(runner, repository)?;
    Ok(unmerged(runner, repository)?
        .into_iter()
        .map(|file| {
            let (yours, theirs) = if inverted {
                (file.theirs, file.ours)
            } else {
                (file.ours, file.theirs)
            };
            ConflictedFile {
                yours: file.change(yours),
                theirs: file.change(theirs),
                path: file.path,
            }
        })
        .collect())
}

/// The commands, as the arguments after `git`, that resolve the files at `paths` in `repository`
/// with the version of `side`: they put it in the working tree and in the index, or delete the
/// file when that side deleted it. A path that is not in conflict returns
/// [`Failure::FileNotInConflict`], and nothing is resolved.
pub(crate) fn taking_side(
    runner: &GitRunner,
    repository: &Repository,
    paths: &[String],
    side: ConflictSide,
) -> Result<Vec<Vec<String>>, Failure> {
    let inverted = yours_is_theirs(runner, repository)?;
    let files = unmerged(runner, repository)?;
    let takes_ours = (side == ConflictSide::Yours) != inverted;
    let mut kept = Vec::new();
    let mut deleted = Vec::new();
    for path in paths {
        let file = files
            .iter()
            .find(|file| file.path == Path::new(path))
            .ok_or_else(|| Failure::FileNotInConflict { path: path.clone() })?;
        let present = if takes_ours { file.ours } else { file.theirs };
        if present { &mut kept } else { &mut deleted }.push(path.clone());
    }
    let mut commands = Vec::new();
    if !kept.is_empty() {
        let version = if takes_ours { "--ours" } else { "--theirs" };
        commands.push(pathspec_command(&["checkout", version], &kept));
        commands.push(pathspec_command(&["add"], &kept));
    }
    if !deleted.is_empty() {
        commands.push(pathspec_command(&["rm", "--quiet"], &deleted));
    }
    Ok(commands)
}

/// The command, as the arguments after `git`, that marks the files at `paths` in `repository`
/// resolved as the user left them: what is on disk goes to the index, and a file deleted by hand
/// leaves git. A path that is not in conflict returns [`Failure::FileNotInConflict`], and nothing
/// is marked.
pub(crate) fn marking_resolved(
    runner: &GitRunner,
    repository: &Repository,
    paths: &[String],
) -> Result<Vec<String>, Failure> {
    let files = unmerged(runner, repository)?;
    if let Some(path) = paths
        .iter()
        .find(|path| !files.iter().any(|file| file.path == Path::new(path)))
    {
        return Err(Failure::FileNotInConflict { path: path.clone() });
    }
    Ok(pathspec_command(&["add", "--all"], paths))
}

/// The arguments after `git` of `command` on `paths`, taken literally.
fn pathspec_command(command: &[&str], paths: &[String]) -> Vec<String> {
    let mut args = vec!["--literal-pathspecs".to_owned()];
    args.extend(command.iter().map(|&arg| arg.to_owned()));
    args.push("--".to_owned());
    args.extend(paths.iter().cloned());
    args
}

/// Whether the user's side is the one git calls "theirs", as it is during a rebase: git replays
/// the user's commits onto the other branch.
fn yours_is_theirs(runner: &GitRunner, repository: &Repository) -> Result<bool, Failure> {
    Ok(in_progress::read(runner, repository)?
        .is_some_and(|operation| operation.kind == OperationKind::Rebase))
}

/// The files the index of `repository` holds more than one version of, ordered by path.
fn unmerged(runner: &GitRunner, repository: &Repository) -> Result<Vec<Unmerged>, Failure> {
    let listed = runner
        .run(&repository.root, &["ls-files", "--unmerged", "-z"])
        .map_err(git_failed_to_start)?;
    if !listed.success {
        return Err(Failure::ChangesUnreadable {
            folder: repository.root.clone(),
        });
    }
    let mut files: Vec<Unmerged> = Vec::new();
    // `<mode> <object> <stage>\t<path>`, one record per version, the versions of a path together.
    for record in listed.stdout.split(|&byte| byte == 0) {
        let Some(tab) = record.iter().position(|&byte| byte == b'\t') else {
            continue;
        };
        let (fields, path) = (&record[..tab], &record[tab + 1..]);
        let path = path_from_bytes(path);
        if files.last().is_none_or(|file| file.path != path) {
            files.push(Unmerged {
                path,
                base: false,
                ours: false,
                theirs: false,
            });
        }
        let file = files.last_mut().expect("a file was just pushed");
        match fields.rsplit(|&byte| byte == b' ').next() {
            Some(b"1") => file.base = true,
            Some(b"2") => file.ours = true,
            Some(b"3") => file.theirs = true,
            _ => {}
        }
    }
    Ok(files)
}
