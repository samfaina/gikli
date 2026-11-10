//! Staging mode: the user works with the git index directly, at file level, without losing their
//! Changelists.

use std::path::Path;

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    Change, ChangeStatus, DiffContent, Gikli, LocalReadOnly, Repository, StagingArea, StagingSide,
    TextDiff,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A repository with one commit holding `a.txt` and `b.txt`.
fn repository_with_a_commit(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.write("b.txt", "b\n");
    repository.commit_all("Initial commit");
    repository
}

/// gikli in Staging mode.
fn in_staging_mode(sandbox: &Sandbox) -> Gikli {
    let gikli = sandbox.gikli();
    gikli.set_staging_mode(true).expect("turn on Staging mode");
    gikli
}

/// The Staged and Unstaged changes of `root` as gikli reads them.
fn staging_area(gikli: &Gikli, root: &Repository) -> StagingArea {
    gikli
        .read_changes(root)
        .expect("read changes")
        .staging
        .expect("Staging mode is on")
}

fn entries(changes: &[Change]) -> Vec<(&Path, ChangeStatus)> {
    changes
        .iter()
        .map(|change| (change.path.as_path(), change.status))
        .collect()
}

#[test]
fn staging_mode_is_off_by_default_and_the_setting_turns_it_on_and_off() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    assert!(!gikli.settings().staging_mode);
    assert_eq!(gikli.read_changes(&root).expect("read").staging, None);

    assert!(gikli.set_staging_mode(true).expect("turn on").staging_mode);
    assert!(gikli.read_changes(&root).expect("read").staging.is_some());
    assert!(
        sandbox.gikli().settings().staging_mode,
        "the setting lasts after gikli closes"
    );

    assert!(
        !gikli
            .set_staging_mode(false)
            .expect("turn off")
            .staging_mode
    );
    assert_eq!(gikli.read_changes(&root).expect("read").staging, None);
}

#[test]
fn a_file_shows_as_staged_unstaged_or_both_by_what_differs() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a staged\n");
    repository.git(&["add", "--", "a.txt"]);
    repository.write("a.txt", "a staged and changed again\n");
    repository.write("b.txt", "b changed\n");
    repository.write("c.txt", "c\n");
    repository.git(&["add", "--", "c.txt"]);
    repository.write("new.txt", "new\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let changes = gikli.read_changes(&root).expect("read");
    let area = changes.staging.as_ref().expect("Staging mode is on");

    assert_eq!(
        entries(&area.staged.changes),
        vec![
            (Path::new("a.txt"), ChangeStatus::Modified),
            (Path::new("c.txt"), ChangeStatus::Added),
        ]
    );
    assert_eq!(
        entries(&area.unstaged.changes),
        vec![
            (Path::new("a.txt"), ChangeStatus::Modified),
            (Path::new("b.txt"), ChangeStatus::Modified),
        ]
    );
    assert_eq!(
        entries(&changes.unversioned_files.changes),
        vec![(Path::new("new.txt"), ChangeStatus::Untracked)],
        "a new file stays in Unversioned Files"
    );
}

#[test]
fn a_move_shows_with_its_original_path_on_the_staged_side() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.git(&["mv", "--", "a.txt", "moved.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let area = staging_area(&gikli, &root);

    assert_eq!(
        entries(&area.staged.changes),
        vec![(Path::new("moved.txt"), ChangeStatus::Renamed)]
    );
    assert_eq!(
        area.staged.changes[0].original_path.as_deref(),
        Some(Path::new("a.txt"))
    );
    assert_eq!(area.unstaged.changes, vec![]);
}

#[test]
fn a_file_staged_and_then_removed_from_disk_shows_on_both_sides() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("c.txt", "c\n");
    repository.git(&["add", "--", "c.txt"]);
    repository.delete("c.txt");
    repository.delete("b.txt");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let area = staging_area(&gikli, &root);

    assert_eq!(
        entries(&area.staged.changes),
        vec![(Path::new("c.txt"), ChangeStatus::Added)]
    );
    assert_eq!(
        entries(&area.unstaged.changes),
        vec![
            (Path::new("b.txt"), ChangeStatus::Deleted),
            (Path::new("c.txt"), ChangeStatus::Deleted),
        ]
    );
}

#[test]
fn staged_and_unstaged_changes_follow_the_grouping_the_user_chose() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("src/x.txt", "x\n");
    repository.git(&["add", "--", "src/x.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let area = staging_area(&gikli, &root);

    assert!(matches!(
        &area.staged.nodes[0],
        gikli_core::ChangeNode::Folder { path, .. } if path == Path::new("src")
    ));
}

fn paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

#[test]
fn staging_a_file_moves_it_to_the_staged_side() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a changed\n");
    repository.delete("b.txt");
    repository.write("new.txt", "new\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli
        .add_files(&root, &paths(&["a.txt", "b.txt", "new.txt"]))
        .expect("stage");

    assert_eq!(
        repository.status_by_git(),
        vec!["M  a.txt", "D  b.txt", "A  new.txt"]
    );
}

#[test]
fn unstaging_a_file_leaves_its_working_tree_content() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a changed\n");
    repository.write("b.txt", "b changed\n");
    repository.write("c.txt", "c\n");
    repository.git(&["add", "--all"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli
        .unstage_files(&root, &paths(&["a.txt", "c.txt"]))
        .expect("unstage");

    assert_eq!(
        repository.status_by_git(),
        vec![" M a.txt", "M  b.txt", "?? c.txt"]
    );
    assert_eq!(
        std::fs::read_to_string(repository.root().join("a.txt")).expect("read"),
        "a changed\n"
    );
}

#[test]
fn unstaging_a_move_puts_both_of_its_paths_back() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.git(&["mv", "--", "a.txt", "moved.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli
        .unstage_files(&root, &paths(&["moved.txt"]))
        .expect("unstage");

    assert_eq!(repository.status_by_git(), vec![" D a.txt", "?? moved.txt"]);
}

#[test]
fn files_can_be_unstaged_before_the_first_commit() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.write("b.txt", "b\n");
    repository.git(&["add", "--all"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli
        .unstage_files(&root, &paths(&["a.txt"]))
        .expect("unstage");

    assert_eq!(repository.status_by_git(), vec!["A  b.txt", "?? a.txt"]);
}

#[test]
fn unstaging_what_is_not_staged_changes_nothing() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a changed\n");
    repository.write("new.txt", "new\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli
        .unstage_files(&root, &paths(&["a.txt", "new.txt", "gone.txt"]))
        .expect("unstage");

    assert_eq!(repository.status_by_git(), vec![" M a.txt", "?? new.txt"]);
}

#[test]
fn stage_all_takes_every_change_and_new_file() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a changed\n");
    repository.delete("b.txt");
    repository.write("new.txt", "new\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli.stage_all(&root).expect("stage all");

    assert_eq!(
        repository.status_by_git(),
        vec!["M  a.txt", "D  b.txt", "A  new.txt"]
    );
}

#[test]
fn stage_tracked_leaves_new_files_out() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a changed\n");
    repository.delete("b.txt");
    repository.write("new.txt", "new\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli.stage_tracked(&root).expect("stage tracked");

    assert_eq!(
        repository.status_by_git(),
        vec!["M  a.txt", "D  b.txt", "?? new.txt"]
    );
}

#[test]
fn a_failed_unstage_says_so() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a changed\n");
    repository.git(&["add", "--", "a.txt"]);
    let lock = repository.root().join(".git/index.lock");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);
    std::fs::write(&lock, "").expect("lock the index");

    let failure = gikli
        .unstage_files(&root, &paths(&["a.txt"]))
        .expect_err("the index is locked");

    assert!(
        matches!(failure, gikli_core::Failure::Explained { .. }),
        "got {failure:?}"
    );
    assert_eq!(repository.status_by_git(), vec!["M  a.txt"]);
}

fn commit_count(repository: &TestRepository) -> String {
    repository.git(&["rev-list", "--count", "HEAD"])
}

#[test]
fn a_commit_takes_what_is_staged_and_leaves_the_rest() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a staged\n");
    repository.git(&["add", "--", "a.txt"]);
    repository.write("a.txt", "a staged and changed again\n");
    repository.write("b.txt", "b changed\n");
    repository.write("new.txt", "new\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli
        .commit_staged(&root, "Commit the index")
        .expect("commit");

    assert_eq!(commit_count(&repository), "2\n");
    assert_eq!(repository.git(&["show", "HEAD:a.txt"]), "a staged\n");
    assert_eq!(repository.git(&["show", "HEAD:b.txt"]), "b\n");
    assert_eq!(
        repository.git(&["log", "-1", "--format=%B"]),
        "Commit the index\n\n"
    );
    assert_eq!(
        repository.status_by_git(),
        vec![" M a.txt", " M b.txt", "?? new.txt"]
    );
}

#[test]
fn a_commit_records_a_staged_move_and_a_staged_deletion() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.git(&["mv", "--", "a.txt", "moved.txt"]);
    repository.git(&["rm", "--quiet", "--", "b.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli
        .commit_staged(&root, "Move and remove")
        .expect("commit");

    assert_eq!(
        repository.git(&["ls-tree", "--name-only", "HEAD"]),
        "moved.txt\n"
    );
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn the_first_commit_of_a_repository_takes_what_is_staged() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.write("b.txt", "b\n");
    repository.git(&["add", "--", "a.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli.commit_staged(&root, "First").expect("commit");

    assert_eq!(
        repository.git(&["ls-tree", "--name-only", "HEAD"]),
        "a.txt\n"
    );
    assert_eq!(repository.status_by_git(), vec!["?? b.txt"]);
}

#[test]
fn a_message_reaches_the_commit_as_written() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a changed\n");
    repository.git(&["add", "--", "a.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli
        .commit_staged(&root, "# Not a comment\n\n  spaces stay")
        .expect("commit");

    assert_eq!(
        repository.git(&["log", "-1", "--format=%B"]),
        "# Not a comment\n\n  spaces stay\n\n"
    );
}

#[test]
fn with_nothing_staged_there_is_nothing_to_commit() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a changed\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let failure = gikli
        .commit_staged(&root, "Nothing")
        .expect_err("nothing is staged");

    assert_eq!(failure, gikli_core::Failure::NothingToCommit);
    assert_eq!(commit_count(&repository), "1\n");
}

#[test]
fn a_commit_needs_a_message() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a changed\n");
    repository.git(&["add", "--", "a.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let failure = gikli.commit_staged(&root, " \n").expect_err("no message");

    assert_eq!(failure, gikli_core::Failure::CommitMessageEmpty);
    assert_eq!(repository.status_by_git(), vec!["M  a.txt"]);
}

#[test]
fn a_commit_a_hook_stops_leaves_the_index_as_it_was() {
    let sandbox = Sandbox::new();
    sandbox.hook("pre-commit", "#!/bin/sh\necho no >&2\nexit 1\n");
    let repository = repository_with_a_commit_past_hooks(&sandbox);
    repository.write("a.txt", "a changed\n");
    repository.git(&["add", "--", "a.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let failure = gikli
        .commit_staged(&root, "Stopped")
        .expect_err("the hook refuses");

    assert!(
        matches!(failure, gikli_core::Failure::Explained { .. }),
        "got {failure:?}"
    );
    assert_eq!(repository.status_by_git(), vec!["M  a.txt"]);
    assert_eq!(commit_count(&repository), "1\n");
}

/// A repository like [`repository_with_a_commit`], for a sandbox with a hook that would stop the
/// commit it starts with.
fn repository_with_a_commit_past_hooks(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.write("b.txt", "b\n");
    repository.git(&["add", "--all"]);
    repository.git(&[
        "commit",
        "--no-verify",
        "--quiet",
        "--message=Initial commit",
    ]);
    repository
}

#[test]
fn an_amend_puts_what_is_staged_into_the_last_commit() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a staged\n");
    repository.git(&["add", "--", "a.txt"]);
    repository.write("b.txt", "b changed\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli.amend_with_staged(&root, "Amended").expect("amend");

    assert_eq!(commit_count(&repository), "1\n");
    assert_eq!(repository.git(&["show", "HEAD:a.txt"]), "a staged\n");
    assert_eq!(repository.git(&["log", "-1", "--format=%B"]), "Amended\n\n");
    assert_eq!(repository.status_by_git(), vec![" M b.txt"]);
}

#[test]
fn an_amend_needs_something_staged() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let failure = gikli
        .amend_with_staged(&root, "Only a new message")
        .expect_err("nothing is staged");

    assert_eq!(failure, gikli_core::Failure::NothingToCommit);
    assert_eq!(
        repository.git(&["log", "-1", "--format=%s"]),
        "Initial commit\n"
    );
}

#[test]
fn there_is_nothing_to_amend_before_the_first_commit() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.git(&["add", "--", "a.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let failure = gikli
        .amend_with_staged(&root, "No commit yet")
        .expect_err("no commit");

    assert_eq!(failure, gikli_core::Failure::NothingToAmend);
}

/// The text diff of the file at `path` on `side`, with the side the core says it compared.
fn side_diff(
    gikli: &Gikli,
    root: &Repository,
    path: &str,
    side: StagingSide,
) -> (TextDiff, Option<StagingSide>) {
    let diff = gikli
        .read_staging_diff(root, path, side)
        .expect("read diff");
    match diff.content {
        DiffContent::Text(text) => (text, diff.side),
        other => panic!("expected a text diff, got {other:?}"),
    }
}

/// A repository whose `a.txt` has one version in HEAD, another staged and a third on disk.
fn repository_with_three_versions(sandbox: &Sandbox) -> TestRepository {
    let repository = repository_with_a_commit(sandbox);
    repository.write("a.txt", "staged\n");
    repository.git(&["add", "--", "a.txt"]);
    repository.write("a.txt", "on disk\n");
    repository
}

#[test]
fn the_diff_of_a_staged_change_compares_head_with_the_index() {
    let sandbox = Sandbox::new();
    let repository = repository_with_three_versions(&sandbox);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let (text, side) = side_diff(&gikli, &root, "a.txt", StagingSide::Staged);

    assert_eq!(side, Some(StagingSide::Staged));
    assert_eq!(text.head.as_deref(), Some("a\n"));
    assert_eq!(text.local.as_deref(), Some("staged\n"));
    assert_eq!(text.differences.len(), 1);
    assert_eq!(text.local_read_only, Some(LocalReadOnly::Index));
}

#[test]
fn the_diff_of_an_unstaged_change_compares_the_index_with_the_working_tree() {
    let sandbox = Sandbox::new();
    let repository = repository_with_three_versions(&sandbox);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let (text, side) = side_diff(&gikli, &root, "a.txt", StagingSide::Unstaged);

    assert_eq!(side, Some(StagingSide::Unstaged));
    assert_eq!(text.head.as_deref(), Some("staged\n"));
    assert_eq!(text.local.as_deref(), Some("on disk\n"));
    assert_eq!(text.differences.len(), 1);
    assert_eq!(text.local_read_only, None);
}

#[test]
fn a_staged_new_file_has_nothing_on_the_head_side() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("c.txt", "c\n");
    repository.git(&["add", "--", "c.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let (text, _) = side_diff(&gikli, &root, "c.txt", StagingSide::Staged);

    assert_eq!(text.head, None);
    assert_eq!(text.local.as_deref(), Some("c\n"));
    assert_eq!(text.differences.len(), 1);
}

#[test]
fn a_staged_file_diffs_before_the_first_commit() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.git(&["add", "--", "a.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let (text, _) = side_diff(&gikli, &root, "a.txt", StagingSide::Staged);

    assert_eq!(text.head, None);
    assert_eq!(text.local.as_deref(), Some("a\n"));
    assert_eq!(text.differences.len(), 1);
}

#[test]
fn a_staged_deletion_has_nothing_on_the_index_side() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.git(&["rm", "--quiet", "--", "b.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let (text, _) = side_diff(&gikli, &root, "b.txt", StagingSide::Staged);

    assert_eq!(text.head.as_deref(), Some("b\n"));
    assert_eq!(text.local, None);
    assert_eq!(text.differences.len(), 1);
}

#[test]
fn a_staged_move_compares_the_file_where_head_has_it() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "one\ntwo\nthree\nfour\nfive\n");
    repository.commit_all("Longer");
    repository.git(&["mv", "--", "a.txt", "moved.txt"]);
    repository.write("moved.txt", "one\ntwo\nthree\nfour\nfive changed\n");
    repository.git(&["add", "--", "moved.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let diff = gikli
        .read_staging_diff(&root, "moved.txt", StagingSide::Staged)
        .expect("read diff");

    assert_eq!(diff.original_path.as_deref(), Some(Path::new("a.txt")));
    let DiffContent::Text(text) = diff.content else {
        panic!("expected a text diff");
    };
    assert_eq!(text.head.as_deref(), Some("one\ntwo\nthree\nfour\nfive\n"));
    assert_eq!(
        text.local.as_deref(),
        Some("one\ntwo\nthree\nfour\nfive changed\n")
    );
    assert_eq!(text.differences.len(), 1);
}

#[test]
fn a_file_removed_from_disk_compares_the_index_with_nothing() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.delete("b.txt");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let (text, _) = side_diff(&gikli, &root, "b.txt", StagingSide::Unstaged);

    assert_eq!(text.head.as_deref(), Some("b\n"));
    assert_eq!(text.local, None);
    assert_eq!(text.local_read_only, Some(LocalReadOnly::NoLocalFile));
    assert_eq!(text.differences.len(), 1);
}

#[test]
fn a_new_file_has_nothing_on_the_index_side() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("new.txt", "new\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let (text, _) = side_diff(&gikli, &root, "new.txt", StagingSide::Unstaged);

    assert_eq!(text.head, None);
    assert_eq!(text.local.as_deref(), Some("new\n"));
    assert_eq!(text.differences.len(), 1);
}

#[test]
fn a_line_ending_git_converts_is_no_unstaged_difference() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.git(&["config", "core.autocrlf", "true"]);
    repository.write("a.txt", "a\r\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let (text, _) = side_diff(&gikli, &root, "a.txt", StagingSide::Unstaged);

    assert_eq!(text.differences, vec![]);
}

#[test]
fn a_staged_binary_file_shows_no_text_diff() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    std::fs::write(repository.root().join("data.bin"), [0u8, 1, 2, 0]).expect("write");
    repository.git(&["add", "--", "data.bin"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let diff = gikli
        .read_staging_diff(&root, "data.bin", StagingSide::Staged)
        .expect("read diff");

    assert_eq!(diff.content, DiffContent::Binary);
}

#[test]
fn a_conflicted_file_is_unstaged_and_compares_with_head() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.git(&["checkout", "--quiet", "-b", "other"]);
    repository.write("a.txt", "theirs\n");
    repository.commit_all("Theirs");
    repository.git(&["checkout", "--quiet", "-"]);
    repository.write("a.txt", "ours\n");
    repository.commit_all("Ours");
    assert!(!repository.git_succeeds(&["merge", "--quiet", "other"]));
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let area = staging_area(&gikli, &root);
    let (text, side) = side_diff(&gikli, &root, "a.txt", StagingSide::Unstaged);

    assert_eq!(area.staged.changes, vec![]);
    assert_eq!(
        entries(&area.unstaged.changes),
        vec![(Path::new("a.txt"), ChangeStatus::Conflicted)]
    );
    assert_eq!(side, None, "the index holds no one version to compare with");
    assert_eq!(text.head.as_deref(), Some("ours\n"));
}

#[test]
fn next_and_previous_file_stay_on_the_side_of_the_diff() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "a staged\n");
    repository.write("c.txt", "c\n");
    repository.git(&["add", "--", "a.txt", "c.txt"]);
    repository.write("a.txt", "a on disk\n");
    repository.write("b.txt", "b changed\n");
    repository.write("new.txt", "new\n");
    let gikli = in_staging_mode(&sandbox);
    gikli
        .set_change_grouping(gikli_core::ChangeGrouping::Flat)
        .expect("flat");
    let root = opened(&gikli, &repository);
    let neighbors = |path: &str, side: StagingSide| {
        let diff = gikli
            .read_staging_diff(&root, path, side)
            .expect("read diff");
        (diff.previous_file, diff.next_file)
    };

    assert_eq!(
        neighbors("a.txt", StagingSide::Staged),
        (None, Some("c.txt".into()))
    );
    assert_eq!(
        neighbors("c.txt", StagingSide::Staged),
        (Some("a.txt".into()), None)
    );
    assert_eq!(
        neighbors("a.txt", StagingSide::Unstaged),
        (None, Some("b.txt".into()))
    );
    assert_eq!(
        neighbors("b.txt", StagingSide::Unstaged),
        (Some("a.txt".into()), Some("new.txt".into())),
        "Unversioned Files come after the Unstaged changes"
    );
    assert_eq!(
        neighbors("new.txt", StagingSide::Unstaged),
        (Some("b.txt".into()), None)
    );
}

#[test]
fn saving_the_local_side_of_an_unstaged_diff_writes_the_file_and_leaves_the_index() {
    let sandbox = Sandbox::new();
    let repository = repository_with_three_versions(&sandbox);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);
    let (text, _) = side_diff(&gikli, &root, "a.txt", StagingSide::Unstaged);

    let saved = gikli
        .save_unstaged_file(&root, "a.txt", "edited\n", &text.local_version)
        .expect("save");

    assert_eq!(saved.side, Some(StagingSide::Unstaged));
    let DiffContent::Text(after) = saved.content else {
        panic!("expected a text diff");
    };
    assert_eq!(after.head.as_deref(), Some("staged\n"));
    assert_eq!(after.local.as_deref(), Some("edited\n"));
    assert_eq!(
        std::fs::read_to_string(repository.root().join("a.txt")).expect("read"),
        "edited\n"
    );
    assert_eq!(repository.git(&["show", ":a.txt"]), "staged\n");
}

#[test]
fn the_diff_against_head_says_it_is_on_no_side() {
    let sandbox = Sandbox::new();
    let repository = repository_with_three_versions(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let diff = gikli.read_diff(&root, "a.txt").expect("read diff");

    assert_eq!(diff.side, None);
}

/// The paths in the Changelist called `name`.
fn paths_in_changelist(changes: &gikli_core::ChangeSet, name: &str) -> Vec<std::path::PathBuf> {
    changes
        .changelists
        .iter()
        .find(|list| list.name == name)
        .unwrap_or_else(|| panic!("no Changelist called {name}"))
        .group
        .changes
        .iter()
        .map(|change| change.path.clone())
        .collect()
}

/// gikli on a repository where `a.txt` is in a Changelist called Feature, and `b.txt` in Changes.
fn with_a_file_in_another_changelist(sandbox: &Sandbox) -> (Gikli, Repository, TestRepository) {
    let repository = repository_with_a_commit(sandbox);
    repository.write("a.txt", "a changed\n");
    repository.write("b.txt", "b changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    gikli
        .read_changes(&root)
        .expect("the files go to the Active changelist");
    gikli
        .move_changes(
            &root,
            &paths(&["a.txt"]),
            &gikli_core::MoveTarget::NewChangelist {
                name: "Feature".to_owned(),
            },
            false,
        )
        .expect("move");
    (gikli, root, repository)
}

#[test]
fn changelists_come_back_after_staging_and_unstaging() {
    let sandbox = Sandbox::new();
    let (gikli, root, _repository) = with_a_file_in_another_changelist(&sandbox);

    gikli.set_staging_mode(true).expect("turn on");
    gikli.stage_all(&root).expect("stage all");
    gikli.read_changes(&root).expect("read in Staging mode");
    gikli
        .unstage_files(&root, &paths(&["a.txt", "b.txt"]))
        .expect("unstage");
    gikli.read_changes(&root).expect("read in Staging mode");
    gikli.set_staging_mode(false).expect("turn off");

    let changes = gikli.read_changes(&root).expect("read");
    assert_eq!(changes.staging, None);
    assert_eq!(
        paths_in_changelist(&changes, "Feature"),
        vec![Path::new("a.txt")]
    );
    assert_eq!(
        paths_in_changelist(&changes, "Changes"),
        vec![Path::new("b.txt")]
    );
}

#[test]
fn a_file_left_staged_stays_in_its_changelist() {
    let sandbox = Sandbox::new();
    let (gikli, root, repository) = with_a_file_in_another_changelist(&sandbox);

    gikli.set_staging_mode(true).expect("turn on");
    gikli.stage_all(&root).expect("stage all");
    gikli.read_changes(&root).expect("read in Staging mode");
    gikli.set_staging_mode(false).expect("turn off");

    let changes = gikli.read_changes(&root).expect("read");
    assert_eq!(
        paths_in_changelist(&changes, "Feature"),
        vec![Path::new("a.txt")]
    );
    assert_eq!(
        repository.status_by_git(),
        vec!["M  a.txt", "M  b.txt"],
        "switching leaves the index as it is"
    );
}

#[test]
fn a_file_committed_in_staging_mode_leaves_its_changelist_and_the_others_stay() {
    let sandbox = Sandbox::new();
    let (gikli, root, _repository) = with_a_file_in_another_changelist(&sandbox);

    gikli.set_staging_mode(true).expect("turn on");
    gikli
        .add_files(&root, &paths(&["a.txt"]))
        .expect("stage a.txt");
    gikli.commit_staged(&root, "Commit a").expect("commit");
    gikli.set_staging_mode(false).expect("turn off");

    let changes = gikli.read_changes(&root).expect("read");
    assert_eq!(
        paths_in_changelist(&changes, "Feature"),
        Vec::<&Path>::new()
    );
    assert_eq!(
        paths_in_changelist(&changes, "Changes"),
        vec![Path::new("b.txt")]
    );
}

#[test]
fn a_change_made_in_staging_mode_goes_to_the_active_changelist() {
    let sandbox = Sandbox::new();
    let (gikli, root, repository) = with_a_file_in_another_changelist(&sandbox);
    gikli.set_staging_mode(true).expect("turn on");
    repository.write("c.txt", "c\n");
    repository.git(&["add", "--", "c.txt"]);
    gikli.read_changes(&root).expect("read in Staging mode");
    gikli.set_staging_mode(false).expect("turn off");

    let changes = gikli.read_changes(&root).expect("read");

    assert_eq!(
        paths_in_changelist(&changes, "Changes"),
        vec![Path::new("b.txt"), Path::new("c.txt")]
    );
}

#[test]
fn a_commit_in_changelist_mode_still_takes_the_working_tree() {
    let sandbox = Sandbox::new();
    let repository = repository_with_three_versions(&sandbox);
    let gikli = in_staging_mode(&sandbox);
    gikli.set_staging_mode(false).expect("turn off");
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["a.txt"]), "From the working tree")
        .expect("commit");

    assert_eq!(repository.git(&["show", "HEAD:a.txt"]), "on disk\n");
}

#[test]
fn a_staged_deletion_is_not_mistaken_for_a_new_file_in_a_folder_of_the_same_name() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("x", "the file\n");
    repository.commit_all("A file");
    repository.delete("x");
    repository.write("x/y", "in the folder\n");
    repository.git(&["add", "--all"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let (text, _) = side_diff(&gikli, &root, "x", StagingSide::Staged);

    assert_eq!(text.head.as_deref(), Some("the file\n"));
    assert_eq!(text.local, None);
    assert_eq!(text.differences.len(), 1);
}

#[test]
fn a_merge_with_nothing_to_stage_can_be_concluded() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.git(&["checkout", "--quiet", "-b", "other"]);
    repository.write("a.txt", "theirs\n");
    repository.commit_all("Theirs");
    repository.git(&["checkout", "--quiet", "-"]);
    repository.git(&[
        "merge",
        "--quiet",
        "--strategy=ours",
        "--no-commit",
        "other",
    ]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    assert!(staging_area(&gikli, &root).merging);
    gikli
        .commit_staged(&root, "Merge other")
        .expect("conclude the merge");

    assert_eq!(
        repository.git(&["rev-list", "--count", "--merges", "HEAD"]),
        "1\n"
    );
}

#[test]
fn outside_a_merge_the_staging_area_says_so() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    assert!(!staging_area(&gikli, &root).merging);
}

#[test]
fn a_file_added_with_intent_to_add_is_unstaged_and_stays_out_of_a_commit() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("planned.txt", "planned\n");
    repository.git(&["add", "--intent-to-add", "--", "planned.txt"]);
    repository.write("a.txt", "a changed\n");
    repository.git(&["add", "--", "a.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let area = staging_area(&gikli, &root);
    gikli.commit_staged(&root, "Only a").expect("commit");

    assert_eq!(
        entries(&area.staged.changes),
        vec![(Path::new("a.txt"), ChangeStatus::Modified)]
    );
    assert_eq!(
        entries(&area.unstaged.changes),
        vec![(Path::new("planned.txt"), ChangeStatus::Added)]
    );
    assert_eq!(
        repository.git(&["ls-tree", "--name-only", "HEAD"]),
        "a.txt\nb.txt\n"
    );
}

#[test]
fn a_file_added_with_intent_to_add_that_git_finds_moved_says_where_from() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("a.txt", "one\ntwo\nthree\nfour\nfive\n");
    repository.commit_all("Longer");
    std::fs::rename(
        repository.root().join("a.txt"),
        repository.root().join("moved.txt"),
    )
    .expect("move");
    repository.git(&["add", "--intent-to-add", "--", "moved.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let area = staging_area(&gikli, &root);

    let moved = area
        .unstaged
        .changes
        .iter()
        .find(|change| change.path == Path::new("moved.txt"))
        .expect("moved.txt is unstaged");
    assert_eq!(moved.status, ChangeStatus::Renamed);
    assert_eq!(moved.original_path.as_deref(), Some(Path::new("a.txt")));
}

#[test]
fn paths_git_could_read_as_patterns_or_options_are_taken_literally() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    repository.write("[a] b.txt", "literal\n");
    repository.write("-dash.txt", "dash\n");
    repository.write("a b.txt", "a file the pattern would match\n");
    let selected = paths(&["[a] b.txt", "-dash.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli.add_files(&root, &selected).expect("stage");
    assert_eq!(
        entries(&staging_area(&gikli, &root).staged.changes).len(),
        2,
        "only the two selected files are staged"
    );

    let (text, _) = side_diff(&gikli, &root, "[a] b.txt", StagingSide::Staged);
    assert_eq!(text.local.as_deref(), Some("literal\n"));

    gikli.unstage_files(&root, &selected).expect("unstage");
    assert_eq!(staging_area(&gikli, &root).staged.changes, vec![]);
}

#[test]
fn more_files_than_fit_on_a_command_line_can_be_unstaged() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_commit(&sandbox);
    let padding = "a-rather-long-name-for-a-file-so-that-few-of-them-fill-a-command-line";
    let names: Vec<String> = (0..700).map(|n| format!("{padding}-{n}.txt")).collect();
    for name in &names {
        std::fs::write(repository.root().join(name), "new\n").expect("write");
    }
    repository.git(&["add", "--all"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    gikli.unstage_files(&root, &names).expect("unstage");

    assert_eq!(staging_area(&gikli, &root).staged.changes, vec![]);
}

#[test]
fn a_submodule_shows_no_text_diff_on_the_staged_side() {
    let sandbox = Sandbox::new();
    sandbox.allow_local_submodules();
    let library = sandbox.repository("library");
    library.write("lib.txt", "lib\n");
    library.commit_all("Library");
    let repository = repository_with_a_commit(&sandbox);
    let url = gikli_acceptance::file_url(library.root());
    repository.git(&["submodule", "--quiet", "add", "--", &url, "lib"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let diff = gikli
        .read_staging_diff(&root, "lib", StagingSide::Staged)
        .expect("read diff");

    assert_eq!(diff.content, DiffContent::Folder);
}
