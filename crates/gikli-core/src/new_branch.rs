//! The branch engine's new branch: a branch created from HEAD, a branch or a commit.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::git::GitRunner;
use crate::{
    Failure, RefKind, RefLabel, Repository, details, failure_of, git_failed_to_start, log, sync,
};

/// What a new branch starts from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NewBranchBase {
    /// The commit HEAD is on.
    Head,
    /// The latest commit of a local branch, a remote branch or a tag.
    Branch { label: RefLabel },
    /// A commit, by its hash.
    Commit { hash: String },
}

impl NewBranchBase {
    /// The revision git starts the branch from. A branch goes by its whole reference, so that a
    /// reference of another kind with the same name does not get in the way, and so that git sets
    /// up the upstream of a branch started from a remote branch as its configuration says.
    fn revision(&self) -> String {
        match self {
            Self::Head => "HEAD".to_owned(),
            Self::Branch { label } => label.full_name(),
            Self::Commit { hash } => hash.clone(),
        }
    }

    /// The name the user knows the base by.
    fn name(&self) -> &str {
        match self {
            Self::Head => "HEAD",
            Self::Branch { label } => &label.name,
            Self::Commit { hash } => hash,
        }
    }
}

/// `name`, as the user types it for a new branch, with each character git takes in no branch name
/// replaced by `-`: spaces and other control characters, `~`, `^`, `:`, `?`, `*`, `[` and `\`.
/// Each character stays one character, so the name keeps its length. What git rejects only in some
/// places, such as `..` or a name that ends with `/`, is left for [`check_name`] to reject.
pub(crate) fn clean_name(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            '~' | '^' | ':' | '?' | '*' | '[' | '\\' | ' ' => '-',
            character if character.is_ascii_control() => '-',
            character => character,
        })
        .collect()
}

/// The prefixes the name field of a new branch of `repository` suggests when the user has typed
/// `typed`: each directory of a local branch, and of a remote branch within its remote, with `/`
/// at its end, such as `feature/` and `feature/ui/` for `feature/ui/menu`. Those that start with
/// what was typed, ignoring case, are suggested, each once and by name, other than the one typed.
pub(crate) fn prefixes(
    runner: &GitRunner,
    repository: &Repository,
    typed: &str,
) -> Result<Vec<String>, Failure> {
    let remotes = sync::remotes(runner, repository)?;
    let typed_lowercase = typed.to_lowercase();
    let mut prefixes = BTreeSet::new();
    for label in log::references(runner, repository)? {
        let name = match label.kind {
            RefKind::Local => label.name.as_str(),
            RefKind::Remote => match within_remote(&label.name, &remotes) {
                Some(name) => name,
                None => continue,
            },
            RefKind::Head | RefKind::Tag => continue,
        };
        for (end, _) in name.match_indices('/') {
            let prefix = &name[..=end];
            if prefix != typed && prefix.to_lowercase().starts_with(&typed_lowercase) {
                prefixes.insert(prefix.to_owned());
            }
        }
    }
    Ok(prefixes.into_iter().collect())
}

/// The name of the remote branch `name`, such as `origin/feature/x`, within the one of `remotes`
/// it belongs to: the longest one it starts with, as a remote's own name can hold `/`.
fn within_remote<'a>(name: &'a str, remotes: &[String]) -> Option<&'a str> {
    remotes
        .iter()
        .filter_map(|remote| name.strip_prefix(remote.as_str())?.strip_prefix('/'))
        .min_by_key(|within| within.len())
}

/// Checks that git takes `name` for a new branch of `repository` as it is, and that no branch has
/// it yet. As it is means that `git check-ref-format --branch` accepts it and does not expand it to
/// another name, as it does `@{-1}` to the branch checked out before. `HEAD` is no name for a
/// branch either, which older git does not say. Any other name returns
/// [`Failure::BranchNameInvalid`], and the name of a branch there is [`Failure::BranchNameTaken`].
pub(crate) fn check_name(
    runner: &GitRunner,
    repository: &Repository,
    name: &str,
) -> Result<(), Failure> {
    let checked = runner
        .run(&repository.root, &["check-ref-format", "--branch", name])
        .map_err(git_failed_to_start)?;
    let taken_as_is = checked.success && checked.stdout_text().trim_end_matches('\n') == name;
    if !taken_as_is || name == "HEAD" {
        return Err(Failure::BranchNameInvalid {
            name: name.to_owned(),
        });
    }
    if has_local_branch(runner, repository, name)? {
        return Err(Failure::BranchNameTaken {
            name: name.to_owned(),
        });
    }
    Ok(())
}

/// Whether `repository` has the local branch `name`.
pub(crate) fn has_local_branch(
    runner: &GitRunner,
    repository: &Repository,
    name: &str,
) -> Result<bool, Failure> {
    details::exists(runner, repository, &format!("refs/heads/{name}"))
}

/// The revision a new branch of `repository` starts from at `base`, once it is known to name a
/// commit. One that does not returns [`Failure::RevisionNotFound`] with the name the user knows.
pub(crate) fn start_point(
    runner: &GitRunner,
    repository: &Repository,
    base: &NewBranchBase,
) -> Result<String, Failure> {
    let revision = base.revision();
    details::resolve_known_as(runner, repository, &revision, base.name())?;
    Ok(revision)
}

/// Creates the branch `name` of `repository` at `start`, without checking it out.
pub(crate) fn create(
    runner: &GitRunner,
    repository: &Repository,
    name: &str,
    start: &str,
) -> Result<(), Failure> {
    let root = &repository.root;
    let created = runner
        .run(root, &["branch", "--", name, start])
        .map_err(git_failed_to_start)?;
    if !created.success {
        return Err(failure_of(
            runner,
            root,
            &created,
            not_created(repository, name),
        ));
    }
    Ok(())
}

/// The failure of git that could not create the branch `name` of `repository`.
pub(crate) fn not_created(repository: &Repository, name: &str) -> Failure {
    Failure::BranchNotCreated {
        folder: repository.root.clone(),
        name: name.to_owned(),
    }
}
