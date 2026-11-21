//! The integration engine's merge into current: the merge of a branch into the one HEAD is on, as
//! the branch selector offers it, and what blocks one: the files that keep git from merging, told
//! from what the Repository holds rather than from git's translated sentences, as the failure
//! explainer does.

use crate::checkout::{
    changed_paths, local_changes_in_the_way, moved_and_staged, untracked_in_the_way,
};
use crate::git::GitRunner;
use crate::{Failure, RefLabel, Repository, details, git_failed_to_start};

/// A merge, ready to run.
pub(crate) struct Planned {
    /// The commit the merge brings in, by its full hash.
    pub commit: String,
    /// The arguments after `git` of the merge.
    pub args: Vec<String>,
}

/// The merge of `branch` into the current branch of `repository`, once the branch is known to be
/// there. One that is not returns [`Failure::RevisionNotFound`] with its name.
pub(crate) fn plan(
    runner: &GitRunner,
    repository: &Repository,
    branch: &RefLabel,
) -> Result<Planned, Failure> {
    let full_name = branch.full_name();
    let commit = details::resolve_known_as(runner, repository, &full_name, &branch.name)?;
    let named = given_name(runner, repository, branch, &full_name)?;
    Ok(Planned {
        commit,
        args: ["merge", "--no-edit", "--end-of-options", &named]
            .map(str::to_owned)
            .into(),
    })
}

/// The name to give git for `branch` of `repository`, whose whole reference is `full_name`: the
/// name the user knows when git takes it for that reference, so the message git writes names the
/// branch as the user knows it, as `Merge branch 'feature'`, and the whole reference when another
/// one of that name, such as a tag, would get in the way.
fn given_name(
    runner: &GitRunner,
    repository: &Repository,
    branch: &RefLabel,
    full_name: &str,
) -> Result<String, Failure> {
    // git would take a name that starts with `-` for an option.
    if branch.name.starts_with('-') {
        return Ok(full_name.to_owned());
    }
    let taken = runner
        .run(
            &repository.root,
            &["rev-parse", "--symbolic-full-name", &branch.name],
        )
        .map_err(git_failed_to_start)?;
    let unambiguous = taken.success && taken.stdout_text().trim() == full_name;
    Ok(if unambiguous {
        branch.name.clone()
    } else {
        full_name.to_owned()
    })
}

/// The failure of `planned`, a merge in `repository`, when files block it. None when none does,
/// and git, when it refused, failed for another reason.
///
/// A merge that fast-forwards, as `merge.ff` lets it, checks the commit out, which the same files block as a checkout. Any
/// other merge combines what both sides changed since their merge base, and git starts none while
/// a change is staged, or while a file it would change has changes on disk. A file deleted on disk
/// only does not count, as nothing on disk would be lost. A file git does not track blocks either
/// way where the commit brings a file in at its path, a folder above it or under it.
pub(crate) fn blocked(
    runner: &GitRunner,
    repository: &Repository,
    planned: &Planned,
) -> Result<Option<Failure>, Failure> {
    let commit = planned.commit.as_str();
    let fast_forward = fast_forward(runner, repository)?;
    // The merge base a merge that does not fast-forward combines both sides from.
    let combined_from = match merge_base(runner, repository, commit)? {
        // With no commit yet, the merge checks the commit out.
        None if !details::exists(runner, repository, "HEAD")? => None,
        // Histories that share no commit git merges with none of these.
        None => return Ok(None),
        Some(base) if base == commit => return Ok(None),
        Some(base)
            if fast_forward != FastForward::Never
                && base == details::resolve(runner, repository, "HEAD")? =>
        {
            None
        }
        Some(base) => Some(base),
    };
    // A merge that may only fast-forward and cannot fails for that, whatever the files hold.
    if combined_from.is_some() && fast_forward == FastForward::Only {
        return Ok(None);
    }
    let (moved, local_changes) = match combined_from {
        None => {
            let (moved, staged) = moved_and_staged(runner, repository, commit)?;
            let local = local_changes_in_the_way(runner, repository, commit, &moved, &staged)?;
            (moved, local)
        }
        Some(base) => {
            let moved = changed_paths(runner, repository, &["diff-tree", "-r"], &[&base, commit])?;
            let mut local =
                changed_paths(runner, repository, &["diff-index", "--cached"], &["HEAD"])?;
            let unstaged = changed_paths(runner, repository, &["diff", "--diff-filter=d"], &[])?;
            local.extend(unstaged.intersection(&moved).cloned());
            (moved, local.into_iter().collect())
        }
    };
    let untracked_files = untracked_in_the_way(runner, repository, &moved, &local_changes)?;
    let blocks = !local_changes.is_empty() || !untracked_files.is_empty();
    Ok(blocks.then(|| Failure::MergeBlocked {
        folder: repository.root.clone(),
        local_changes,
        untracked_files,
    }))
}

/// The best common ancestor of HEAD of `repository` and `commit`, by its full hash, or None when
/// they have none, as when HEAD has no commit yet.
fn merge_base(
    runner: &GitRunner,
    repository: &Repository,
    commit: &str,
) -> Result<Option<String>, Failure> {
    let base = runner
        .run(&repository.root, &["merge-base", "HEAD", commit])
        .map_err(git_failed_to_start)?;
    Ok(base.success.then(|| base.stdout_text().trim().to_owned()))
}

/// Whether git fast-forwards a merge, as `merge.ff` says.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FastForward {
    /// When it can, as by default.
    Allowed,
    /// Never: every merge makes a merge commit.
    Never,
    /// Only: a merge that cannot fast-forward fails.
    Only,
}

/// How git fast-forwards a merge in `repository`, as `merge.ff` says.
fn fast_forward(runner: &GitRunner, repository: &Repository) -> Result<FastForward, Failure> {
    let config = runner
        .run(&repository.root, &["config", "--get", "merge.ff"])
        .map_err(git_failed_to_start)?;
    let value = config.stdout_text().trim().to_lowercase();
    Ok(match value.as_str() {
        "only" => FastForward::Only,
        "false" | "no" | "off" | "0" => FastForward::Never,
        _ => FastForward::Allowed,
    })
}
