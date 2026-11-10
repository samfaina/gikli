//! Adding files to git and rolling back changes: what the user does on purpose to the files of a
//! Repository.

use std::fs;
use std::path::PathBuf;

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    Change, ChangeSet, ChangeStatus, Event, Failure, Gikli, LongOperation, Repository,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A repository with one commit that holds `README.md` and ignores `*.log` and `build/`.
fn committed_repository(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.write(".gitignore", "*.log\nbuild/\n");
    repository.commit_all("Initial commit");
    repository
}

fn paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

fn change(path: &str, status: ChangeStatus) -> Change {
    Change {
        path: PathBuf::from(path),
        status,
        original_path: None,
        conflict: None,
        submodule: None,
    }
}

#[test]
fn an_unversioned_file_can_be_added() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("new.txt", "new\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli.add_files(&root, &paths(&["new.txt"])).expect("add");

    assert_eq!(repository.status_by_git(), vec!["A  new.txt"]);
}

#[test]
fn an_ignored_file_is_not_added_without_being_forced() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("debug.log", "log\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.add_files(&root, &paths(&["debug.log"]));

    assert_eq!(
        result,
        Err(Failure::FilesNotAdded {
            folder: root.root.clone()
        })
    );
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn an_ignored_file_can_be_force_added() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("debug.log", "log\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .force_add_files(&root, &paths(&["debug.log"]))
        .expect("force add");

    assert_eq!(repository.status_by_git(), vec!["A  debug.log"]);
}

/// A folder git ignores as a whole is one row of the changes view.
#[test]
fn a_folder_git_ignores_as_a_whole_can_be_force_added() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("build/out.bin", "out\n");
    repository.write("build/nested/more.bin", "more\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .force_add_files(&root, &paths(&["build/"]))
        .expect("force add");

    assert_eq!(
        repository.status_by_git(),
        vec!["A  build/nested/more.bin", "A  build/out.bin"]
    );
}

#[test]
fn force_adding_takes_a_path_literally() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("a.log", "a\n");
    repository.write("b.log", "b\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.force_add_files(&root, &paths(&["*.log"]));

    assert_eq!(
        result,
        Err(Failure::FilesNotAdded {
            folder: root.root.clone()
        })
    );
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn the_list_to_confirm_holds_the_selected_changes_and_discards_nothing() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("other.txt", "other\n");
    repository.write("gone.txt", "gone\n");
    repository.commit_all("More files");
    repository.write("README.md", "changed\n");
    repository.delete("gone.txt");
    repository.write("other.txt", "changed too\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["gone.txt", "README.md"]))
        .expect("rollback plan");

    assert_eq!(
        plan.files,
        vec![
            change("README.md", ChangeStatus::Modified),
            change("gone.txt", ChangeStatus::Deleted),
        ]
    );
    assert!(!plan.has_added_files);
    assert_eq!(
        repository.status_by_git(),
        vec![" M README.md", " D gone.txt", " M other.txt"]
    );
}

#[test]
fn the_list_to_confirm_says_when_the_selection_holds_added_files() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("new.txt", "new\n");
    repository.git(&["add", "new.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["new.txt"]))
        .expect("rollback plan");

    assert_eq!(plan.files, vec![change("new.txt", ChangeStatus::Added)]);
    assert!(plan.has_added_files);
}

/// An Unversioned File has no change to discard: git has nothing to bring back.
#[test]
fn the_list_to_confirm_leaves_out_what_is_not_in_a_changelist() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("new.txt", "new\n");
    repository.write("debug.log", "log\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["new.txt", "debug.log", "no-such-file.txt"]))
        .expect("rollback plan");

    assert_eq!(plan.files, vec![]);
    assert!(!plan.has_added_files);
}

/// Lists what rolling back `selected` would discard and confirms that list straight away.
fn roll_back(
    gikli: &Gikli,
    root: &Repository,
    selected: &[&str],
    delete_added_files: bool,
) -> Result<ChangeSet, Failure> {
    let plan = gikli
        .rollback_plan(root, &paths(selected))
        .expect("rollback plan");
    gikli.rollback_files(root, &plan, delete_added_files)
}

/// What the file at `path` of `repository` holds on disk.
fn contents(repository: &TestRepository, path: &str) -> String {
    fs::read_to_string(repository.root().join(path)).expect("read file")
}

fn exists(repository: &TestRepository, path: &str) -> bool {
    repository.root().join(path).exists()
}

#[test]
fn rolling_back_a_modified_file_brings_back_the_last_commit() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("README.md", "staged\n");
    repository.git(&["add", "README.md"]);
    repository.write("README.md", "and edited\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let changes = roll_back(&gikli, &root, &["README.md"], false).expect("roll back");

    assert_eq!(changes.changes, vec![]);
    assert_eq!(contents(&repository, "README.md"), "hello\n");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn rolling_back_a_deleted_file_brings_it_back() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.delete("README.md");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["README.md"], false).expect("roll back");

    assert_eq!(contents(&repository, "README.md"), "hello\n");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn rolling_back_leaves_the_files_that_were_not_selected() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("other.txt", "other\n");
    repository.commit_all("More files");
    repository.write("README.md", "changed\n");
    repository.write("other.txt", "changed too\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["README.md"], false).expect("roll back");

    assert_eq!(contents(&repository, "other.txt"), "changed too\n");
    assert_eq!(repository.status_by_git(), vec![" M other.txt"]);
}

#[test]
fn rolling_back_takes_a_path_literally() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("[ab].txt", "");
    repository.write("a.txt", "one\n");
    repository.commit_all("More files");
    repository.write("[ab].txt", "changed\n");
    repository.write("a.txt", "changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["[ab].txt"], false).expect("roll back");

    assert_eq!(repository.status_by_git(), vec![" M a.txt"]);
}

#[test]
fn a_rolled_back_file_leaves_its_changelist() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("README.md", "changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    gikli.read_changes(&root).expect("read changes");

    let changes = roll_back(&gikli, &root, &["README.md"], false).expect("roll back");

    assert_eq!(changes.changelists[0].group.changes, vec![]);
}

/// An Unversioned File has no version to go back to, so nothing may remove it.
#[test]
fn rolling_back_never_touches_an_unversioned_file() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("new.txt", "new\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["new.txt"], true).expect("roll back");

    assert_eq!(contents(&repository, "new.txt"), "new\n");
    assert_eq!(repository.status_by_git(), vec!["?? new.txt"]);
}

#[test]
fn a_rollback_git_cannot_do_says_so_and_discards_nothing() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("README.md", "changed\n");
    repository.git(&["add", "README.md"]);
    repository.write("README.md", "changed again\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let lock = repository.root().join(".git").join("index.lock");
    fs::write(&lock, "").expect("write lock");

    let result = roll_back(&gikli, &root, &["README.md"], false);

    assert!(
        matches!(result, Err(Failure::Explained { .. })),
        "{result:?}"
    );
    assert_eq!(contents(&repository, "README.md"), "changed again\n");
}

#[test]
fn rolling_back_an_added_file_keeps_its_local_copy_unless_told_otherwise() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("new.txt", "new\n");
    repository.git(&["add", "new.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let changes = roll_back(&gikli, &root, &["new.txt"], false).expect("roll back");

    assert_eq!(contents(&repository, "new.txt"), "new\n");
    assert_eq!(repository.status_by_git(), vec!["?? new.txt"]);
    assert_eq!(
        changes.unversioned_files.changes,
        vec![change("new.txt", ChangeStatus::Untracked)]
    );
}

#[test]
fn rolling_back_an_added_file_can_delete_its_local_copy() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("src/nested/new.txt", "new\n");
    repository.git(&["add", "src/nested/new.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["src/nested/new.txt"], true).expect("roll back");

    assert!(!exists(&repository, "src"), "the empty folders go too");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

/// git refuses to remove a file whose index and working tree versions differ unless told to.
#[test]
fn an_added_file_edited_since_can_be_rolled_back() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("new.txt", "new\n");
    repository.git(&["add", "new.txt"]);
    repository.write("new.txt", "edited\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["new.txt"], false).expect("roll back");

    assert_eq!(contents(&repository, "new.txt"), "edited\n");
    assert_eq!(repository.status_by_git(), vec!["?? new.txt"]);
}

#[test]
fn the_option_to_delete_applies_only_to_added_files() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("README.md", "changed\n");
    repository.write("new.txt", "new\n");
    repository.git(&["add", "new.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["README.md", "new.txt"], true).expect("roll back");

    assert_eq!(contents(&repository, "README.md"), "hello\n");
    assert!(!exists(&repository, "new.txt"));
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn rollback_works_in_a_repository_with_no_commits() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("first.txt", "first\n");
    repository.write("second.txt", "second\n");
    repository.git(&["add", "first.txt", "second.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["first.txt", "second.txt"]))
        .expect("rollback plan");
    assert!(plan.has_added_files);
    roll_back(&gikli, &root, &["first.txt"], false).expect("roll back and keep");
    roll_back(&gikli, &root, &["second.txt"], true).expect("roll back and delete");

    assert_eq!(contents(&repository, "first.txt"), "first\n");
    assert!(!exists(&repository, "second.txt"));
    assert_eq!(repository.status_by_git(), vec!["?? first.txt"]);
}

/// `git add --intent-to-add` records that a file is coming without its content.
#[test]
fn a_file_added_with_intent_to_add_rolls_back_as_an_added_file() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("new.txt", "new\n");
    repository.git(&["add", "--intent-to-add", "new.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["new.txt"]))
        .expect("rollback plan");
    assert_eq!(plan.files, vec![change("new.txt", ChangeStatus::Added)]);
    roll_back(&gikli, &root, &["new.txt"], false).expect("roll back");

    assert_eq!(repository.status_by_git(), vec!["?? new.txt"]);
}

fn renamed(path: &str, original_path: &str) -> Change {
    Change {
        original_path: Some(PathBuf::from(original_path)),
        ..change(path, ChangeStatus::Renamed)
    }
}

/// Going back from a rename is one step: the file returns to where the last commit has it, and
/// nothing stays at the new path, whatever the user chose for added files.
#[test]
fn rolling_back_a_rename_puts_the_file_back_where_it_was() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "README.md", "moved.md"]);
    repository.write("moved.md", "hello\nand edited\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["moved.md"]))
        .expect("rollback plan");
    assert_eq!(plan.files, vec![renamed("moved.md", "README.md")]);
    assert!(!plan.has_added_files);
    roll_back(&gikli, &root, &["moved.md"], false).expect("roll back");

    assert_eq!(contents(&repository, "README.md"), "hello\n");
    assert!(!exists(&repository, "moved.md"));
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

/// A rename's rollback writes over a new file at the old path, so that file is on the list too.
#[test]
fn the_list_to_confirm_shows_the_new_file_a_rename_would_write_over() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "README.md", "moved.md"]);
    repository.write("README.md", "a new file\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["moved.md"]))
        .expect("rollback plan");

    assert_eq!(
        plan.files,
        vec![
            change("README.md", ChangeStatus::Untracked),
            renamed("moved.md", "README.md"),
        ]
    );
}

/// git still holds the new path in its index, though gikli shows only the deletion.
#[test]
fn rolling_back_a_file_moved_and_then_removed_leaves_nothing_behind() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "README.md", "moved.md"]);
    repository.delete("moved.md");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["README.md"], false).expect("roll back");

    assert_eq!(contents(&repository, "README.md"), "hello\n");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

/// A copy is a new file: the file it was copied from keeps its own changes.
#[test]
fn rolling_back_a_copy_takes_it_out_of_git_and_leaves_the_file_it_came_from() {
    let sandbox = Sandbox::new();
    sandbox.configure("[status]\n\trenames = copies\n");
    let repository = sandbox.repository("app");
    let lines = "one\ntwo\nthree\nfour\nfive\n";
    repository.write("lines.txt", lines);
    repository.commit_all("Initial commit");
    repository.write("copy.txt", lines);
    repository.write("lines.txt", &format!("{lines}six\n"));
    repository.git(&["add", "--all"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["copy.txt"]))
        .expect("rollback plan");
    assert!(plan.has_added_files);
    roll_back(&gikli, &root, &["copy.txt"], false).expect("roll back");

    assert_eq!(
        repository.status_by_git(),
        vec!["M  lines.txt", "?? copy.txt"]
    );
}

/// A repository on `main` whose merge of `feature` stops on a conflict over `README.md`.
fn repository_in_a_conflict(sandbox: &Sandbox, ours: Option<&str>) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("README.md", "base\n");
    repository.commit_all("Initial commit");
    repository.git(&["switch", "--quiet", "--create", "feature"]);
    repository.write("README.md", "theirs\n");
    repository.commit_all("Feature");
    repository.git(&["switch", "--quiet", "main"]);
    match ours {
        Some(contents) => repository.write("README.md", contents),
        None => repository.delete("README.md"),
    }
    repository.commit_all("Main");
    assert!(
        !repository.git_succeeds(&["merge", "--quiet", "feature"]),
        "the merge stops on the conflict"
    );
    repository
}

#[test]
fn rolling_back_a_conflicted_file_brings_back_our_side() {
    let sandbox = Sandbox::new();
    let repository = repository_in_a_conflict(&sandbox, Some("ours\n"));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["README.md"]))
        .expect("rollback plan");
    assert!(!plan.has_added_files);
    roll_back(&gikli, &root, &["README.md"], false).expect("roll back");

    assert_eq!(contents(&repository, "README.md"), "ours\n");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn a_conflicted_file_the_last_commit_does_not_hold_is_taken_out_of_git() {
    let sandbox = Sandbox::new();
    let repository = repository_in_a_conflict(&sandbox, None);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["README.md"]))
        .expect("rollback plan");
    assert!(plan.has_added_files);
    roll_back(&gikli, &root, &["README.md"], false).expect("roll back");

    assert_eq!(contents(&repository, "README.md"), "theirs\n");
    assert_eq!(repository.status_by_git(), vec!["?? README.md"]);
}

/// A repository with `library` as a submodule at `lib`, committed.
fn repository_with_a_submodule(sandbox: &Sandbox) -> TestRepository {
    sandbox.allow_local_submodules();
    let library = sandbox.repository("library");
    library.write("lib.rs", "pub fn a() {}\n");
    library.commit_all("Library");
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");
    let url = library.root().to_string_lossy().into_owned();
    repository.git(&["submodule", "--quiet", "add", &url, "lib"]);
    repository.commit_all("Add the library");
    repository
}

/// git only moves what the Repository records, so the submodule would stay as it is, changed.
#[test]
fn a_changed_submodule_is_not_something_to_roll_back() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_submodule(&sandbox);
    repository.write("lib/lib.rs", "pub fn b() {}\n");
    repository.git(&["-C", "lib", "commit", "--quiet", "--all", "--message", "B"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["lib"]))
        .expect("rollback plan");

    assert_eq!(plan.files, vec![]);
}

/// Taking the new path out of git would delete the submodule's files.
#[test]
fn a_moved_submodule_is_not_something_to_roll_back() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_submodule(&sandbox);
    repository.git(&["mv", "lib", "moved"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["moved"]))
        .expect("rollback plan");

    assert_eq!(plan.files, vec![]);
}

#[test]
fn a_rollback_shows_as_a_long_operation() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("README.md", "changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let events = gikli.subscribe();

    roll_back(&gikli, &root, &["README.md"], false).expect("roll back");

    assert!(events.try_iter().any(|event| matches!(
        event,
        Event::OperationStarted {
            operation: LongOperation::RollbackFiles { .. },
            ..
        }
    )));
}

#[test]
fn a_rollback_with_nothing_to_discard_runs_nothing() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let events = gikli.subscribe();

    roll_back(&gikli, &root, &["README.md"], false).expect("roll back");

    assert!(
        !events
            .try_iter()
            .any(|event| matches!(event, Event::OperationStarted { .. }))
    );
}

/// Taken out of git but still on disk, the file is a new one that the restore writes over.
#[test]
fn the_list_to_confirm_shows_the_new_file_at_the_path_of_a_deleted_file() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["rm", "--quiet", "--cached", "README.md"]);
    repository.write("README.md", "edited\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["README.md"]))
        .expect("rollback plan");

    assert_eq!(
        plan.files,
        vec![
            change("README.md", ChangeStatus::Deleted),
            change("README.md", ChangeStatus::Untracked),
        ]
    );
}

#[test]
fn the_list_to_confirm_shows_the_new_files_of_a_folder_in_the_way() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.delete("README.md");
    repository.write("README.md/inner.txt", "inner\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["README.md"]))
        .expect("rollback plan");

    assert_eq!(
        plan.files,
        vec![
            change("README.md", ChangeStatus::Deleted),
            change("README.md/inner.txt", ChangeStatus::Untracked),
        ]
    );
}

#[test]
fn the_list_to_confirm_shows_the_ignored_file_a_rename_would_write_over() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("old.log", "kept\n");
    repository.git(&["add", "--force", "old.log"]);
    repository.commit_all("A log worth keeping");
    repository.git(&["mv", "old.log", "new.txt"]);
    repository.write("old.log", "ignored\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let plan = gikli
        .rollback_plan(&root, &paths(&["new.txt"]))
        .expect("rollback plan");

    assert_eq!(
        plan.files,
        vec![
            renamed("new.txt", "old.log"),
            change("old.log", ChangeStatus::Ignored),
        ]
    );
}

/// On a disk that ignores case both names are one file, which must survive the rollback.
#[test]
fn rolling_back_a_rename_that_only_changes_case_keeps_the_file() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "README.md", "readme.md"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    roll_back(&gikli, &root, &["readme.md"], false).expect("roll back");

    assert_eq!(contents(&repository, "README.md"), "hello\n");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn a_list_that_no_longer_holds_discards_nothing() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "README.md", "moved.md"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let plan = gikli
        .rollback_plan(&root, &paths(&["moved.md"]))
        .expect("rollback plan");
    repository.write("README.md", "written after the list was shown\n");

    let result = gikli.rollback_files(&root, &plan, false);

    assert_eq!(result, Err(Failure::RollbackListOutdated));
    assert_eq!(
        contents(&repository, "README.md"),
        "written after the list was shown\n"
    );
    assert!(exists(&repository, "moved.md"));
}
