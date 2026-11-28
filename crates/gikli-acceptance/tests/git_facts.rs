//! Facts of "Git behavior to account for" in `docs/reference/jetbrains-git.md` whose features are
//! not built yet. Each test runs plain git, with no gikli involved, so CI checks the fact on every
//! git and platform it runs on. Once its feature is built, a test through gikli replaces it, as for
//! the other facts.

use gikli_acceptance::{Sandbox, TestRepository};

/// The merge commits HEAD holds, as plain git counts them.
fn merges_by_git(repository: &TestRepository) -> String {
    repository.git(&["rev-list", "--merges", "--count", "HEAD"])
}

/// Pins "`git fetch <remote> <remote-branch>:<local-branch>` fails when the branch is checked out
/// in a worktree", which shapes Update a branch that is not checked out (Scope 2): gikli reports
/// such a branch and continues with the rest. A test of that feature through gikli replaces this
/// one once it is built.
///
/// The same fetch, once no worktree has the branch, fast-forwards it: the worktree is what makes
/// git refuse, and nothing else in the setup.
///
/// The fact holds from git 2.35 on. Before, git checked only the branch of the worktree it ran in,
/// and fetched into a branch a linked worktree has checked out, leaving that worktree out of step
/// with its branch: on the minimum git, the feature cannot count on git to refuse.
#[test]
fn a_fetch_into_a_branch_checked_out_in_a_linked_worktree_fails_until_the_worktree_goes() {
    let sandbox = Sandbox::new();
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main", "main:side"]);
    let mine = sandbox.clone_of(&remote, "mine");
    let linked = mine.linked_worktree("side");
    // As the feature finds a branch to update: it has an upstream.
    mine.git(&[
        "branch",
        "--quiet",
        "--set-upstream-to",
        "origin/side",
        "side",
    ]);
    work.commit_at(2, "On side");
    work.git(&["push", "--quiet", "origin", "main:side"]);
    let before = mine.commit_of("side");

    let while_checked_out = mine.git_succeeds(&["fetch", "--quiet", "origin", "side:side"]);

    if !sandbox.git_is_at_least(2, 35) {
        assert!(while_checked_out, "git before 2.35 fetches into side");
        assert_eq!(mine.commit_of("side"), remote.commit_of("side"));
        return;
    }
    assert!(!while_checked_out, "git refuses to fetch into side");
    assert_eq!(mine.commit_of("side"), before, "side stays where it was");

    let path = linked.root().to_string_lossy().into_owned();
    mine.git(&["worktree", "remove", &path]);

    let once_free = mine.git_succeeds(&["fetch", "--quiet", "origin", "side:side"]);

    assert!(once_free, "git fetches into side");
    assert_eq!(
        mine.commit_of("side"),
        remote.commit_of("side"),
        "side fast-forwarded to the remote's"
    );
}

/// A repository whose `main` holds Base, Dropped, Later and then a `--no-ff` merge of `topic`,
/// which holds Feature on top of Dropped. Returns the commit of Dropped.
fn history_with_a_later_merge(repository: &TestRepository) -> String {
    repository.write("base.txt", "base\n");
    repository.commit_all("Base");
    repository.write("dropped.txt", "dropped\n");
    repository.commit_all("Dropped");
    let dropped = repository.commit_of("HEAD");
    repository.git(&["switch", "--quiet", "--create", "topic"]);
    repository.write("feature.txt", "feature\n");
    repository.commit_all("Feature");
    repository.git(&["switch", "--quiet", "main"]);
    repository.write("later.txt", "later\n");
    repository.commit_all("Later");
    repository.git(&[
        "merge",
        "--quiet",
        "--no-ff",
        "--message",
        "Merge topic",
        "topic",
    ]);
    dropped
}

/// A new repository `name` holding [`history_with_a_later_merge`], from which plain git dropped
/// Dropped with `git rebase <options> --onto Dropped^ Dropped`.
fn dropped_with(sandbox: &Sandbox, name: &str, options: &[&str]) -> TestRepository {
    let repository = sandbox.repository(name);
    let dropped = history_with_a_later_merge(&repository);
    assert_eq!(
        merges_by_git(&repository),
        "1\n",
        "the history has its merge"
    );
    let parent = format!("{dropped}^");
    let mut rebase = vec!["rebase", "--quiet"];
    rebase.extend_from_slice(options);
    rebase.extend(["--onto", &parent, &dropped]);
    repository.git(&rebase);
    repository
}

/// The subjects of the commits HEAD holds, sorted, since the order git gives a flattened history is
/// not part of the fact.
fn sorted_subjects(repository: &TestRepository) -> Vec<String> {
    let mut subjects = repository.subjects();
    subjects.sort();
    subjects
}

/// Pins "`git rebase --onto <sha>^ <sha>` flattens later merges, and `--rebase-merges` keeps
/// them", which shapes Drop commits (Scope 3): gikli keeps later merges. A test of that feature
/// through gikli replaces this one once it is built.
#[test]
fn dropping_a_commit_with_rebase_onto_flattens_a_later_merge_and_rebase_merges_keeps_it() {
    let sandbox = Sandbox::new();

    let flattened = dropped_with(&sandbox, "flattened", &[]);
    let kept = dropped_with(&sandbox, "kept", &["--rebase-merges"]);

    assert_eq!(
        merges_by_git(&flattened),
        "0\n",
        "--onto flattens the merge"
    );
    assert_eq!(
        sorted_subjects(&flattened),
        ["Base", "Feature", "Later"],
        "Dropped is gone, and the merged commit stays"
    );
    assert_eq!(
        merges_by_git(&kept),
        "1\n",
        "--rebase-merges keeps the merge"
    );
    assert_eq!(
        sorted_subjects(&kept),
        ["Base", "Feature", "Later", "Merge topic"],
        "Dropped is gone, and nothing else"
    );
}
