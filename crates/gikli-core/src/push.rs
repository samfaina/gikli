//! Push: what publishing the current branch would send, and to which branch of which remote.

use serde::{Deserialize, Serialize};

use crate::git::GitRunner;
use crate::log::run_git;
use crate::sync::{configured, fetch_destinations, fetched_from, remotes};
use crate::{Failure, Repository, commit, git_failed_to_start};

/// Where a push sends the current branch: the branch `branch` of the remote `remote`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushTarget {
    pub remote: String,
    /// The name of the branch on the remote, such as `main`.
    pub branch: String,
}

/// What the push dialog shows: the Repository, its current branch, where it goes and the commits
/// it would send there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushPlan {
    pub repository: Repository,
    /// The current branch, the one that is pushed. The user cannot pick another.
    pub branch: String,
    /// The remotes of the Repository, as git lists them, which the target can be on.
    pub remotes: Vec<String>,
    /// Where the branch goes. None when the Repository has no remote, so the user has to define
    /// one first.
    pub target: Option<PushTarget>,
    /// Whether the target does not exist yet, as far as the remote branches the Repository holds
    /// say, so the push creates it.
    pub new_branch: bool,
    /// The commits the push would send, newest first.
    pub commits: Vec<PushCommit>,
}

/// What follows a commit made with Commit and push.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PushAfterCommit {
    /// The push dialog opens with `plan`, and the user pushes from there.
    Dialog { plan: PushPlan },
    /// The push dialog was suppressed, so the current branch went to `target` at once.
    Pushed { target: PushTarget },
}

/// A commit a push would send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushCommit {
    /// The full hash of the commit.
    pub hash: String,
    pub subject: String,
    pub author: String,
    /// When it was committed, in seconds since the epoch.
    pub committed_at: i64,
    /// Whether someone other than the user wrote it: its author does not have the email of their
    /// Commit identity. The dialog marks it with `*`. Without an email, no commit is marked.
    pub by_other_author: bool,
}

/// Which tags a push sends besides the branch. None, unless the user asks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PushTags {
    /// No tag.
    #[default]
    None,
    /// Every tag of the Repository, as `--tags` sends them.
    All,
    /// The annotated tags on the commits the push sends, or that the remote has, as
    /// `--follow-tags` sends them. Lightweight tags stay behind.
    CurrentBranch,
}

impl PushTags {
    /// The option of `git push` that sends these tags.
    fn option(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::All => Some("--tags"),
            Self::CurrentBranch => Some("--follow-tags"),
        }
    }
}

/// The protected branches: those where shared history lives. In this scope they are the default
/// list of the reference, whose expressions match a whole branch name.
const PROTECTED_BRANCHES: [&str; 2] = ["master", "main"];

/// Whether `branch`, a name such as `main`, is a protected branch.
pub(crate) fn is_protected(branch: &str) -> bool {
    PROTECTED_BRANCHES.contains(&branch)
}

/// The arguments of the git that pushes `branch`, the current branch, to `target`, with the tags
/// `tags` asks for, setting `target` as the upstream of the branch when `set_upstream` says so.
pub(crate) fn push_args(
    branch: &str,
    target: &PushTarget,
    tags: PushTags,
    set_upstream: bool,
) -> Vec<String> {
    let mut args = vec!["push".to_owned(), "--progress".to_owned()];
    if set_upstream {
        args.push("--set-upstream".to_owned());
    }
    args.extend(tags.option().map(str::to_owned));
    args.extend([
        "--end-of-options".to_owned(),
        target.remote.clone(),
        format!("refs/heads/{branch}:refs/heads/{}", target.branch),
    ]);
    args
}

/// What a push of the current branch of `repository` would send to `target`, or to the branch it
/// goes to by default when the user has not chosen one.
pub(crate) fn plan(
    runner: &GitRunner,
    repository: &Repository,
    target: Option<&PushTarget>,
) -> Result<PushPlan, Failure> {
    let branch = current_branch(runner, repository)?;
    let remotes = remotes(runner, repository)?;
    let target = match target {
        Some(target) => {
            check(runner, repository, target, &remotes)?;
            Some(target.clone())
        }
        None => default_target(runner, repository, &branch, &remotes)?,
    };
    let Some(target) = target else {
        return Ok(PushPlan {
            repository: repository.clone(),
            branch,
            remotes,
            target: None,
            new_branch: false,
            commits: Vec::new(),
        });
    };
    let local = format!("refs/heads/{branch}");
    let (new_branch, commits) = match remote_branch(runner, repository, &target)? {
        Some(remote_branch) => (
            false,
            commits(runner, repository, &[&local, "--not", &remote_branch])?,
        ),
        // A new branch starts from commits the remote may already have on other branches: those
        // of every remote branch its fetch configuration writes into.
        None => {
            let mut revisions = vec![local.clone(), "--not".to_owned()];
            for destination in fetch_destinations(runner, repository, &target.remote)? {
                if destination.contains('*') {
                    revisions.push(format!("--glob={destination}"));
                } else if exists(runner, repository, &destination)? {
                    revisions.push(destination);
                }
            }
            let revisions: Vec<&str> = revisions.iter().map(String::as_str).collect();
            (true, commits(runner, repository, &revisions)?)
        }
    };
    Ok(PushPlan {
        repository: repository.clone(),
        branch,
        remotes,
        target: Some(target),
        new_branch,
        commits,
    })
}

/// Fails unless `target` is a branch of one of `remotes`, those of `repository`, by a name git
/// takes for a branch. git would take a remote it does not have for a path or a URL, and push there.
pub(crate) fn check(
    runner: &GitRunner,
    repository: &Repository,
    target: &PushTarget,
    remotes: &[String],
) -> Result<(), Failure> {
    if !remotes.contains(&target.remote) {
        return Err(Failure::RemoteNotFound {
            remote: target.remote.clone(),
        });
    }
    if !is_reference_name(runner, repository, &format!("refs/heads/{}", target.branch))? {
        return Err(Failure::BranchNameInvalid {
            name: target.branch.clone(),
        });
    }
    Ok(())
}

/// Whether git takes `reference`, a full name such as `refs/heads/main`, for the name of a
/// reference.
pub(crate) fn is_reference_name(
    runner: &GitRunner,
    repository: &Repository,
    reference: &str,
) -> Result<bool, Failure> {
    Ok(runner
        .run(&repository.root, &["check-ref-format", reference])
        .map_err(git_failed_to_start)?
        .success)
}

/// Whether a push of `branch` of `repository` to `target` sets `target` as its upstream: when the
/// push creates it, and the branch has no upstream on a remote yet.
pub(crate) fn sets_upstream(
    runner: &GitRunner,
    repository: &Repository,
    branch: &str,
    target: &PushTarget,
) -> Result<bool, Failure> {
    Ok(remote_branch(runner, repository, target)?.is_none()
        && upstream(runner, repository, branch)?.is_none())
}

/// The name of the branch HEAD of `repository` is on, the one a push sends. A detached HEAD
/// returns [`Failure::DetachedHeadNotPushed`], and a branch with no commit yet
/// [`Failure::BranchWithoutCommits`].
pub(crate) fn current_branch(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<String, Failure> {
    let head = runner
        .run(&repository.root, &["symbolic-ref", "--quiet", "HEAD"])
        .map_err(git_failed_to_start)?;
    let head_ref = head.stdout_text();
    let Some(branch) = head_ref
        .trim_end()
        .strip_prefix("refs/heads/")
        .filter(|_| head.success)
    else {
        return Err(Failure::DetachedHeadNotPushed);
    };
    if !exists(runner, repository, &format!("refs/heads/{branch}"))? {
        return Err(Failure::BranchWithoutCommits {
            branch: branch.to_owned(),
        });
    }
    Ok(branch.to_owned())
}

/// Where `branch` of `repository`, with `remotes`, goes when the user does not choose: its
/// upstream, when that is a branch of a remote, and otherwise the branch of the same name of
/// `origin`, or of the first remote when there is no `origin`.
fn default_target(
    runner: &GitRunner,
    repository: &Repository,
    branch: &str,
    remotes: &[String],
) -> Result<Option<PushTarget>, Failure> {
    if let Some(upstream) = upstream(runner, repository, branch)? {
        return Ok(Some(upstream));
    }
    let remote = remotes
        .iter()
        .find(|remote| *remote == "origin")
        .or(remotes.first());
    Ok(remote.map(|remote| PushTarget {
        remote: remote.clone(),
        branch: branch.to_owned(),
    }))
}

/// The upstream of `branch` of `repository`, when it is a branch of a remote.
fn upstream(
    runner: &GitRunner,
    repository: &Repository,
    branch: &str,
) -> Result<Option<PushTarget>, Failure> {
    let remote = configured(runner, repository, &format!("branch.{branch}.remote"))?;
    let merge = configured(runner, repository, &format!("branch.{branch}.merge"))?;
    Ok(match (remote, merge) {
        // `.` is the Repository itself: the upstream is another local branch.
        (Some(remote), Some(merge)) if remote != "." => {
            merge.strip_prefix("refs/heads/").map(|name| PushTarget {
                remote,
                branch: name.to_owned(),
            })
        }
        _ => None,
    })
}

/// The remote branch of `repository` that holds `target` as the remote had it when it was last
/// fetched, if it exists.
fn remote_branch(
    runner: &GitRunner,
    repository: &Repository,
    target: &PushTarget,
) -> Result<Option<String>, Failure> {
    let reference = format!("refs/heads/{}", target.branch);
    let fetched = fetched_from(runner, repository, &target.remote, &reference)?;
    match fetched {
        Some(fetched) if exists(runner, repository, &fetched)? => Ok(Some(fetched)),
        _ => Ok(None),
    }
}

/// Whether the reference `reference` exists in `repository`.
fn exists(runner: &GitRunner, repository: &Repository, reference: &str) -> Result<bool, Failure> {
    Ok(runner
        .run(
            &repository.root,
            &["rev-parse", "--verify", "--quiet", reference],
        )
        .map_err(git_failed_to_start)?
        .success)
}

/// What git prints of each commit, a field per NUL, with `-z` putting a NUL between commits too.
const FORMAT: &str = "--format=%H%x00%an%x00%ae%x00%ct%x00%s";

/// The commits `revisions` names in `repository`, newest first.
fn commits(
    runner: &GitRunner,
    repository: &Repository,
    revisions: &[&str],
) -> Result<Vec<PushCommit>, Failure> {
    // `log.showSignature` would add the signatures to what git prints.
    let mut args = vec![
        "log",
        "-z",
        "--no-show-signature",
        "--encoding=UTF-8",
        FORMAT,
    ];
    args.extend(revisions);
    args.push("--");
    let listed = run_git(runner, repository, &args)?.stdout_text();
    let user_email = commit::user_email(runner, &repository.root)?;
    let fields: Vec<&str> = listed.split('\0').collect();
    Ok(fields
        .as_chunks::<5>()
        .0
        .iter()
        .map(|[hash, author, email, committed_at, subject]| PushCommit {
            hash: (*hash).to_owned(),
            author: (*author).to_owned(),
            by_other_author: user_email
                .as_deref()
                .is_some_and(|user_email| !user_email.eq_ignore_ascii_case(email)),
            committed_at: committed_at.parse().unwrap_or_default(),
            subject: (*subject).to_owned(),
        })
        .collect())
}
