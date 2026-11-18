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
    /// The side does not have the file, which was not there when the sides parted either: only the
    /// other side added it, or moved it there.
    Absent,
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

/// How the user's sides map onto git's in a Repository now.
#[derive(Clone, Copy)]
struct Sides {
    /// Whether yours is the side git calls "theirs", as it is during a rebase: git replays the
    /// user's commits onto the other branch.
    yours_is_theirs: bool,
}

impl Sides {
    fn read(runner: &GitRunner, repository: &Repository) -> Result<Self, Failure> {
        let yours_is_theirs = in_progress::read(runner, repository)?
            .is_some_and(|operation| operation.kind == OperationKind::Rebase);
        Ok(Self { yours_is_theirs })
    }

    /// Whether `side` is the one git calls "ours".
    fn is_ours(self, side: ConflictSide) -> bool {
        (side == ConflictSide::Yours) != self.yours_is_theirs
    }
}

/// The mode git gives a submodule, whose version is a commit of its own repository.
const GITLINK_MODE: &str = "160000";

/// One version of a file the index holds.
struct Version {
    /// As git writes it, such as `100644`, or `160000` for a submodule.
    mode: String,
    object: String,
}

/// The versions of a file in conflict that the index holds: the one the sides parted from, ours
/// and theirs, as git names them. A side that does not have the file has no version.
struct VersionsInConflict {
    path: PathBuf,
    base: Option<Version>,
    ours: Option<Version>,
    theirs: Option<Version>,
}

impl VersionsInConflict {
    /// The version of `side`, if that side has the file.
    fn of(&self, side: ConflictSide, sides: Sides) -> Option<&Version> {
        if sides.is_ours(side) {
            self.ours.as_ref()
        } else {
            self.theirs.as_ref()
        }
    }

    /// What `side` did to the file since the sides parted.
    fn change(&self, side: ConflictSide, sides: Sides) -> SideChange {
        match (self.of(side, sides).is_some(), self.base.is_some()) {
            (true, true) => SideChange::Modified,
            (true, false) => SideChange::Added,
            (false, true) => SideChange::Deleted,
            (false, false) => SideChange::Absent,
        }
    }
}

/// The files in conflict in `repository`, ordered by path, told from the user's side.
pub(crate) fn read(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Vec<ConflictedFile>, Failure> {
    let sides = Sides::read(runner, repository)?;
    Ok(versions_in_conflict(runner, repository)?
        .into_iter()
        .map(|file| ConflictedFile {
            yours: file.change(ConflictSide::Yours, sides),
            theirs: file.change(ConflictSide::Theirs, sides),
            path: file.path,
        })
        .collect())
}

/// Whether files of `repository` are in conflict: the index holds more than one version of them.
pub(crate) fn any(runner: &GitRunner, repository: &Repository) -> Result<bool, Failure> {
    Ok(!versions_in_conflict(runner, repository)?.is_empty())
}

/// The commands, as the arguments after `git`, that resolve the files at `paths` in `repository`
/// with the version of `side`: they put it in the working tree and in the index, or delete the
/// file when that side does not have it. A path that is not in conflict returns
/// [`Failure::FileNotInConflict`], and nothing is resolved.
pub(crate) fn taking_side(
    runner: &GitRunner,
    repository: &Repository,
    paths: &[String],
    side: ConflictSide,
) -> Result<Vec<Vec<String>>, Failure> {
    let sides = Sides::read(runner, repository)?;
    let files = versions_in_conflict(runner, repository)?;
    let mut kept = Vec::new();
    let mut deleted = Vec::new();
    let mut commands = Vec::new();
    for path in paths {
        match in_conflict(&files, path)?.of(side, sides) {
            // git checks out no version of a submodule: the index records the commit of that side,
            // and the submodule stays on the commit it has checked out, as after any merge.
            Some(version) if version.mode == GITLINK_MODE => commands.push(vec![
                "update-index".to_owned(),
                "--cacheinfo".to_owned(),
                format!("{},{},{path}", version.mode, version.object),
            ]),
            Some(_) => kept.push(path.clone()),
            None => deleted.push(path.clone()),
        }
    }
    if !kept.is_empty() {
        let version = if sides.is_ours(side) {
            "--ours"
        } else {
            "--theirs"
        };
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
    let files = versions_in_conflict(runner, repository)?;
    for path in paths {
        in_conflict(&files, path)?;
    }
    Ok(pathspec_command(&["add", "--all"], paths))
}

/// The versions of the file at `path` among `files`, or [`Failure::FileNotInConflict`] when it is
/// not one of them.
fn in_conflict<'a>(
    files: &'a [VersionsInConflict],
    path: &str,
) -> Result<&'a VersionsInConflict, Failure> {
    files
        .iter()
        .find(|file| file.path == Path::new(path))
        .ok_or_else(|| Failure::FileNotInConflict {
            path: PathBuf::from(path),
        })
}

/// The arguments after `git` of `command` on `paths`, taken literally.
fn pathspec_command(command: &[&str], paths: &[String]) -> Vec<String> {
    let mut args = vec!["--literal-pathspecs".to_owned()];
    args.extend(command.iter().map(|&arg| arg.to_owned()));
    args.push("--".to_owned());
    args.extend(paths.iter().cloned());
    args
}

/// The files the index of `repository` holds more than one version of, ordered by path.
fn versions_in_conflict(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Vec<VersionsInConflict>, Failure> {
    let listed = runner
        .run(&repository.root, &["ls-files", "--unmerged", "-z"])
        .map_err(git_failed_to_start)?;
    if !listed.success {
        return Err(Failure::ChangesUnreadable {
            folder: repository.root.clone(),
        });
    }
    let mut files: Vec<VersionsInConflict> = Vec::new();
    // `<mode> <object> <stage>\t<path>`, one record per version, the versions of a path together.
    for record in listed.stdout.split(|&byte| byte == 0) {
        let Some(tab) = record.iter().position(|&byte| byte == b'\t') else {
            continue;
        };
        let fields = String::from_utf8_lossy(&record[..tab]);
        let mut fields = fields.split(' ');
        let (Some(mode), Some(object), Some(stage)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let path = path_from_bytes(&record[tab + 1..]);
        if files.last().is_none_or(|file| file.path != path) {
            files.push(VersionsInConflict {
                path,
                base: None,
                ours: None,
                theirs: None,
            });
        }
        let file = files.last_mut().expect("a file was just pushed");
        let version = Some(Version {
            mode: mode.to_owned(),
            object: object.to_owned(),
        });
        match stage {
            "1" => file.base = version,
            "2" => file.ours = version,
            "3" => file.theirs = version,
            _ => {}
        }
    }
    Ok(files)
}
