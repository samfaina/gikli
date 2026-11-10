//! Committing: what git has to do for the changes the user selected to end up in one commit.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::git::GitRunner;
use crate::status::{Change, ChangeStatus, Status};
use crate::{Failure, git_failed_to_start, trace};

/// What a commit of a selection of changes works on.
pub(crate) struct Selection {
    /// The selected Unversioned Files, which git has to know before it can commit them.
    pub unversioned: Vec<String>,
    /// Every path the commit records.
    pub committed: Vec<String>,
}

impl Selection {
    /// The changes in `status` at `paths` that a commit can record: those in a Changelist and the
    /// Unversioned Files. Ignored files are no changes.
    pub fn of(status: &Status, paths: &[String]) -> Self {
        let chosen: Vec<&Change> = status
            .changes
            .iter()
            .filter(|change| change.status != ChangeStatus::Ignored)
            .filter(|change| paths.iter().any(|path| Path::new(path) == change.path))
            .collect();
        let mut committed = Vec::new();
        for change in &chosen {
            committed.push(text_of(&change.path));
            // A move is two changes to git: leaving out one would leave it staged.
            committed.extend(change.renamed_from().map(text_of));
            committed.extend(
                status
                    .moved_and_removed
                    .get(&change.path)
                    .map(|to| text_of(to)),
            );
        }
        Self {
            unversioned: chosen
                .iter()
                .filter(|change| change.status == ChangeStatus::Untracked)
                .map(|change| text_of(&change.path))
                .collect(),
            committed,
        }
    }
}

/// What git reads for one commit: the message and the paths, which do not fit on a command line
/// when a Changelist is large. The files are in gikli's folder of the git directory, the one place
/// git inside WSL and gikli on Windows both reach by a path of their own. They go when this is
/// dropped.
pub(crate) struct CommitInput {
    /// Where gikli wrote them.
    folder: PathBuf,
    /// The same folder, relative to the root of the Repository, as git gets it.
    folder_for_git: String,
    has_unversioned_files: bool,
}

const MESSAGE: &str = "message";
const PATHS: &str = "paths";
const UNVERSIONED_PATHS: &str = "unversioned-paths";

impl CommitInput {
    /// Writes what git reads to commit `selection` with `message`, under the git directory at
    /// `git_dir` relative to `root`.
    pub fn write(
        root: &Path,
        git_dir: &str,
        selection: &Selection,
        message: &str,
    ) -> io::Result<Self> {
        let name = trace::new_run_name();
        let folder = root.join(git_dir).join("gikli").join(&name);
        fs::create_dir_all(&folder)?;
        let input = Self {
            folder,
            folder_for_git: format!("{git_dir}/gikli/{name}"),
            has_unversioned_files: !selection.unversioned.is_empty(),
        };
        // git would take a last line without an end as it is, and other tools do not expect that.
        fs::write(
            input.folder.join(MESSAGE),
            format!("{}\n", message.trim_end()),
        )?;
        fs::write(
            input.folder.join(PATHS),
            nul_separated(&selection.committed),
        )?;
        fs::write(
            input.folder.join(UNVERSIONED_PATHS),
            nul_separated(&selection.unversioned),
        )?;
        Ok(input)
    }

    /// The arguments after `git` that add the selected Unversioned Files, when there are any.
    pub fn add_command(&self) -> Option<Vec<String>> {
        self.has_unversioned_files
            .then(|| self.command_on(UNVERSIONED_PATHS, &["add"]))
    }

    /// The arguments after `git` that take the selected Unversioned Files out of git again, when
    /// there are any. `--force` as well: git refuses otherwise when a file changed after it was added.
    pub fn forget_added_command(&self) -> Option<Vec<String>> {
        self.has_unversioned_files
            .then(|| self.command_on(UNVERSIONED_PATHS, &["rm", "--quiet", "--force", "--cached"]))
    }

    /// The arguments after `git` that commit the selection. The message goes in as written: the
    /// cleanup is not left to `commit.cleanup`, and `commit.verbose` would cut it at a scissors line.
    pub fn commit_command(&self) -> Vec<String> {
        let mut command = self.command_on(
            PATHS,
            &["commit", "--only", "--cleanup=verbatim", "--no-verbose"],
        );
        command.push(format!("--file={}/{MESSAGE}", self.folder_for_git));
        command
    }

    /// `git` with `command` on the paths in the file `paths`, taken literally.
    fn command_on(&self, paths: &str, command: &[&str]) -> Vec<String> {
        let mut args = vec!["--literal-pathspecs".to_owned()];
        args.extend(command.iter().map(|arg| (*arg).to_owned()));
        args.push(format!(
            "--pathspec-from-file={}/{paths}",
            self.folder_for_git
        ));
        args.push("--pathspec-file-nul".to_owned());
        args
    }
}

impl Drop for CommitInput {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}

fn nul_separated(paths: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for path in paths {
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(0);
    }
    bytes
}

/// Fails when git has no name or no email to write in a commit made in the Repository at `root`.
/// git could make some up from the name of the machine, as it does when it is not told otherwise,
/// and a commit would then carry them for good, so only what the user configured counts.
pub(crate) fn ensure_identity(runner: &GitRunner, root: &Path) -> Result<(), Failure> {
    let is_missing = |key: &str| -> Result<bool, Failure> {
        let invocation = runner
            .run(root, &["config", "--get", key])
            .map_err(git_failed_to_start)?;
        Ok(!invocation.success || invocation.stdout_text().trim().is_empty())
    };
    let name_missing = is_missing("user.name")?;
    let email_missing = is_missing("user.email")?;
    if name_missing || email_missing {
        return Err(Failure::CommitIdentityMissing {
            name_missing,
            email_missing,
        });
    }
    Ok(())
}

fn text_of(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
