//! Rollback in Staging mode: discarding one side of the git index and keeping the other.

use std::fs;

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{ChangeSet, ChangeStatus, Failure, Gikli, Repository, RollbackPlan, StagingSide};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A repository with one commit that holds `file.txt`.
fn committed_repository(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("file.txt", "committed\n");
    repository.commit_all("Initial commit");
    repository
}

/// gikli in Staging mode.
fn in_staging_mode(sandbox: &Sandbox) -> Gikli {
    let gikli = sandbox.gikli();
    gikli.set_staging_mode(true).expect("turn on Staging mode");
    gikli
}

fn paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

/// Lists what rolling back `selected` on `side` would discard and confirms that list straight away.
fn roll_back(
    gikli: &Gikli,
    root: &Repository,
    selected: &[&str],
    side: StagingSide,
    delete_added_files: bool,
) -> Result<ChangeSet, Failure> {
    let plan = gikli
        .staging_rollback_plan(root, &paths(selected), side)
        .expect("rollback plan");
    gikli.rollback_files(root, &plan, delete_added_files)
}

/// The paths and statuses on `plan`, in the order it lists them.
fn listed(plan: &RollbackPlan) -> Vec<(String, ChangeStatus)> {
    plan.files
        .iter()
        .map(|file| (file.path.to_string_lossy().into_owned(), file.status))
        .collect()
}

/// What the file at `path` of `repository` holds on disk.
fn contents(repository: &TestRepository, path: &str) -> String {
    fs::read_to_string(repository.root().join(path)).expect("read file")
}

/// What the index of `repository` holds for the file at `path`.
fn staged_contents(repository: &TestRepository, path: &str) -> String {
    repository.git(&["show", &format!(":{path}")])
}

/// `file.txt` with one version staged and another on disk.
fn on_both_sides(repository: &TestRepository) {
    repository.write("file.txt", "staged\n");
    repository.git(&["add", "--", "file.txt"]);
    repository.write("file.txt", "on disk\n");
}

#[test]
fn rolling_back_an_unstaged_change_leaves_the_staged_change_of_the_file() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    on_both_sides(&repository);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["file.txt"], StagingSide::Unstaged, false).expect("roll back");

    assert_eq!(contents(&repository, "file.txt"), "staged\n");
    assert_eq!(staged_contents(&repository, "file.txt"), "staged\n");
    assert_eq!(repository.status_by_git(), vec!["M  file.txt"]);
}

#[test]
fn rolling_back_a_staged_change_leaves_the_unstaged_change_of_the_file() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    on_both_sides(&repository);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["file.txt"], StagingSide::Staged, false).expect("roll back");

    assert_eq!(contents(&repository, "file.txt"), "on disk\n");
    assert_eq!(staged_contents(&repository, "file.txt"), "committed\n");
    assert_eq!(repository.status_by_git(), vec![" M file.txt"]);
}

#[test]
fn a_file_only_staged_goes_back_to_the_last_commit() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("file.txt", "staged\n");
    repository.git(&["add", "--", "file.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["file.txt"], StagingSide::Staged, false).expect("roll back");

    assert_eq!(contents(&repository, "file.txt"), "committed\n");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn a_file_only_unstaged_goes_back_to_the_last_commit() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("file.txt", "on disk\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["file.txt"], StagingSide::Unstaged, false).expect("roll back");

    assert_eq!(contents(&repository, "file.txt"), "committed\n");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

/// A conflicted file is an Unstaged change, and the index holds no one version of it to go back to.
#[test]
fn a_conflicted_file_goes_back_to_our_side() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["switch", "--quiet", "--create", "feature"]);
    repository.write("file.txt", "theirs\n");
    repository.commit_all("Feature");
    repository.git(&["switch", "--quiet", "main"]);
    repository.write("file.txt", "ours\n");
    repository.commit_all("Main");
    assert!(
        !repository.git_succeeds(&["merge", "--quiet", "feature"]),
        "the merge stops on the conflict"
    );
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["file.txt"], StagingSide::Unstaged, false).expect("roll back");

    assert_eq!(contents(&repository, "file.txt"), "ours\n");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn an_added_file_only_staged_leaves_git_and_can_lose_its_local_copy() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("kept.txt", "kept\n");
    repository.write("deleted.txt", "deleted\n");
    repository.git(&["add", "--", "kept.txt", "deleted.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let plan = gikli
        .staging_rollback_plan(&root, &paths(&["kept.txt"]), StagingSide::Staged)
        .expect("rollback plan");
    assert!(plan.has_added_files);
    roll_back(&gikli, &root, &["kept.txt"], StagingSide::Staged, false).expect("keep");
    roll_back(&gikli, &root, &["deleted.txt"], StagingSide::Staged, true).expect("delete");

    assert_eq!(contents(&repository, "kept.txt"), "kept\n");
    assert!(!repository.root().join("deleted.txt").exists());
    assert_eq!(repository.status_by_git(), vec!["?? kept.txt"]);
}

/// With no commit there is no HEAD to restore from, and everything staged is an added file.
#[test]
fn a_repository_with_no_commits_can_roll_back_either_side() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("staged-side.txt", "staged\n");
    repository.write("unstaged-side.txt", "staged\n");
    repository.git(&["add", "--", "staged-side.txt", "unstaged-side.txt"]);
    repository.write("staged-side.txt", "on disk\n");
    repository.write("unstaged-side.txt", "on disk\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    roll_back(
        &gikli,
        &root,
        &["staged-side.txt"],
        StagingSide::Staged,
        false,
    )
    .expect("staged");
    roll_back(
        &gikli,
        &root,
        &["unstaged-side.txt"],
        StagingSide::Unstaged,
        false,
    )
    .expect("unstaged");

    assert_eq!(contents(&repository, "staged-side.txt"), "on disk\n");
    assert_eq!(contents(&repository, "unstaged-side.txt"), "staged\n");
    assert_eq!(
        repository.status_by_git(),
        vec!["A  unstaged-side.txt", "?? staged-side.txt"]
    );
}

/// The file on disk is the other side, so there is no local copy to offer to delete.
#[test]
fn the_staged_side_of_an_added_file_on_both_sides_never_deletes_its_local_copy() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("new.txt", "staged\n");
    repository.git(&["add", "--", "new.txt"]);
    repository.write("new.txt", "on disk\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let plan = gikli
        .staging_rollback_plan(&root, &paths(&["new.txt"]), StagingSide::Staged)
        .expect("rollback plan");
    assert!(!plan.has_added_files);
    gikli.rollback_files(&root, &plan, true).expect("roll back");

    assert_eq!(contents(&repository, "new.txt"), "on disk\n");
    assert_eq!(repository.status_by_git(), vec!["?? new.txt"]);
}

#[test]
fn the_staged_side_of_a_move_edited_since_leaves_the_file_on_disk() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "file.txt", "moved.txt"]);
    repository.write("moved.txt", "committed\nand edited\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["moved.txt"], StagingSide::Staged, false).expect("roll back");

    assert_eq!(
        contents(&repository, "moved.txt"),
        "committed\nand edited\n"
    );
    assert_eq!(
        repository.status_by_git(),
        vec![" D file.txt", "?? moved.txt"]
    );
}

#[test]
fn the_unstaged_side_of_a_move_edited_since_leaves_the_move_staged() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "file.txt", "moved.txt"]);
    repository.write("moved.txt", "committed\nand edited\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["moved.txt"], StagingSide::Unstaged, false).expect("roll back");

    assert_eq!(contents(&repository, "moved.txt"), "committed\n");
    assert_eq!(
        repository.status_by_git(),
        vec!["D  file.txt", "A  moved.txt"]
    );
}

#[test]
fn a_move_only_staged_goes_back_to_where_it_was() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "file.txt", "moved.txt"]);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["moved.txt"], StagingSide::Staged, false).expect("roll back");

    assert_eq!(contents(&repository, "file.txt"), "committed\n");
    assert!(!repository.root().join("moved.txt").exists());
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

/// On a disk that ignores case both names are one file, which must stay as it is.
#[test]
fn the_staged_side_of_a_move_that_only_changes_case_keeps_the_file_on_disk() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "file.txt", "FILE.txt"]);
    repository.write("FILE.txt", "committed\nand edited\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["FILE.txt"], StagingSide::Staged, false).expect("roll back");

    assert_eq!(contents(&repository, "FILE.txt"), "committed\nand edited\n");
    assert_eq!(staged_contents(&repository, "file.txt"), "committed\n");
}

/// The list holds every file that goes: the restore on disk removes a folder in its way.
#[test]
fn the_list_shows_the_new_files_in_the_way_of_an_unstaged_deletion_kept_staged() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("file.txt", "staged\n");
    repository.git(&["add", "--", "file.txt"]);
    repository.delete("file.txt");
    repository.write("file.txt/inner.txt", "inner\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let plan = gikli
        .staging_rollback_plan(&root, &paths(&["file.txt"]), StagingSide::Unstaged)
        .expect("rollback plan");

    assert_eq!(
        listed(&plan),
        vec![
            ("file.txt".to_owned(), ChangeStatus::Deleted),
            ("file.txt/inner.txt".to_owned(), ChangeStatus::Untracked),
        ]
    );
}

/// Taking a change out of the index writes nothing on disk, so nothing else goes.
#[test]
fn the_staged_side_of_a_file_on_both_sides_writes_over_nothing() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "file.txt", "moved.txt"]);
    repository.write("moved.txt", "committed\nand edited\n");
    repository.write("file.txt", "a new file\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let plan = gikli
        .staging_rollback_plan(&root, &paths(&["moved.txt"]), StagingSide::Staged)
        .expect("rollback plan");
    assert_eq!(
        listed(&plan),
        vec![("moved.txt".to_owned(), ChangeStatus::Renamed)]
    );
    gikli
        .rollback_files(&root, &plan, false)
        .expect("roll back");

    assert_eq!(contents(&repository, "file.txt"), "a new file\n");
}

#[test]
fn the_list_holds_only_the_changes_of_the_side_asked_for() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("file.txt", "on disk\n");
    repository.write("new.txt", "new\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let plan = gikli
        .staging_rollback_plan(&root, &paths(&["file.txt", "new.txt"]), StagingSide::Staged)
        .expect("rollback plan");

    assert_eq!(plan.files, vec![]);
    assert_eq!(
        repository.status_by_git(),
        vec![" M file.txt", "?? new.txt"]
    );
}

#[test]
fn a_changed_submodule_is_not_something_to_roll_back_on_the_unstaged_side() {
    let sandbox = Sandbox::new();
    sandbox.allow_local_submodules();
    let library = sandbox.repository("library");
    library.write("lib.rs", "pub fn a() {}\n");
    library.commit_all("Library");
    let repository = committed_repository(&sandbox);
    let url = library.root().to_string_lossy().into_owned();
    repository.git(&["submodule", "--quiet", "add", &url, "lib"]);
    repository.commit_all("Add the library");
    repository.write("lib/lib.rs", "pub fn b() {}\n");
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);

    let plan = gikli
        .staging_rollback_plan(&root, &paths(&["lib"]), StagingSide::Unstaged)
        .expect("rollback plan");

    assert_eq!(plan.files, vec![]);
}

#[test]
fn a_list_that_no_longer_holds_discards_nothing() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    on_both_sides(&repository);
    let gikli = in_staging_mode(&sandbox);
    let root = opened(&gikli, &repository);
    let plan = gikli
        .staging_rollback_plan(&root, &paths(&["file.txt"]), StagingSide::Staged)
        .expect("rollback plan");
    repository.git(&["restore", "--staged", "--", "file.txt"]);

    let result = gikli.rollback_files(&root, &plan, false);

    assert_eq!(result, Err(Failure::RollbackListOutdated));
    assert_eq!(contents(&repository, "file.txt"), "on disk\n");
}
