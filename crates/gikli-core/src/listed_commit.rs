//! Listed commits: the commits a list shows, as the push dialog shows those a push would send and
//! the unmerged commits view those a deleted branch took away.

use serde::Serialize;

use crate::git::GitRunner;
use crate::log::run_git;
use crate::{Failure, Repository, commit};

/// A commit in a list of commits: one a push would send, or one a deleted branch took away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedCommit {
    /// The full hash of the commit.
    pub hash: String,
    pub subject: String,
    pub author: String,
    /// When it was committed, in seconds since the epoch.
    pub committed_at: i64,
    /// Whether someone other than the user wrote it: its author does not have the email of their
    /// Commit identity. The push dialog marks it with `*`. Without an email, no commit is marked.
    pub by_other_author: bool,
}

/// What git prints of each commit, a field per NUL, with `-z` putting a NUL between commits too.
const FORMAT: &str = "--format=%H%x00%an%x00%ae%x00%ct%x00%s";

/// The commits `revisions` names in `repository`, newest first.
pub(crate) fn list(
    runner: &GitRunner,
    repository: &Repository,
    revisions: &[&str],
) -> Result<Vec<ListedCommit>, Failure> {
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
        .map(
            |[hash, author, email, committed_at, subject]| ListedCommit {
                hash: (*hash).to_owned(),
                author: (*author).to_owned(),
                by_other_author: user_email
                    .as_deref()
                    .is_some_and(|user_email| !user_email.eq_ignore_ascii_case(email)),
                committed_at: committed_at.parse().unwrap_or_default(),
                subject: (*subject).to_owned(),
            },
        )
        .collect())
}
