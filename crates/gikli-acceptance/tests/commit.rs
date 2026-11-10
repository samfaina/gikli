//! Committing: what the user selected goes into a commit, as it is in the working tree.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gikli_acceptance::{Sandbox, TestRepository, as_git_writes_it};
use gikli_core::{
    ChangeSet, ExplainedMessage, Failure, Gikli, LongOperation, MoveTarget, Repository,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A repository with one commit that holds `a.txt` and `b.txt`.
fn committed_repository(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.write("b.txt", "b\n");
    repository.commit_all("Initial commit");
    repository
}

fn paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

/// The message of the last commit, as git stores it.
fn last_message(repository: &TestRepository) -> String {
    repository.git(&["log", "-1", "--format=%B"])
}

/// The paths the last commit touched, one `status path` entry each.
fn last_commit_files(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["show", "--format=", "--name-status", "--no-renames", "HEAD"])
        .lines()
        .map(|line| line.replace('\t', " "))
        .collect()
}

#[test]
fn a_selected_file_is_committed_and_the_others_stay_changed() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("a.txt", "a changed\n");
    repository.write("b.txt", "b changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let changes = gikli
        .commit_files(&root, &paths(&["a.txt"]), "Change a")
        .expect("commit");

    assert_eq!(last_message(&repository), "Change a\n\n");
    assert_eq!(last_commit_files(&repository), vec!["M a.txt"]);
    assert_eq!(repository.status_by_git(), vec![" M b.txt"]);
    let left: Vec<_> = changes.changes.iter().map(|change| &change.path).collect();
    assert_eq!(left, vec![std::path::Path::new("b.txt")]);
}

#[test]
fn a_commit_takes_the_working_tree_content_of_a_file_with_another_version_staged() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("a.txt", "staged\n");
    repository.git(&["add", "--", "a.txt"]);
    repository.write("a.txt", "in the working tree\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["a.txt"]), "Change a")
        .expect("commit");

    assert_eq!(
        repository.git(&["show", "HEAD:a.txt"]),
        "in the working tree\n"
    );
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn a_staged_file_that_is_not_selected_stays_staged_and_out_of_the_commit() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("a.txt", "a changed\n");
    repository.write("b.txt", "b staged\n");
    repository.git(&["add", "--", "b.txt"]);
    repository.write("b.txt", "b staged and changed again\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["a.txt"]), "Change a")
        .expect("commit");

    assert_eq!(last_commit_files(&repository), vec!["M a.txt"]);
    assert_eq!(repository.status_by_git(), vec!["MM b.txt"]);
    assert_eq!(repository.git(&["show", ":b.txt"]), "b staged\n");
}

#[test]
fn a_deletion_that_is_selected_is_recorded() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.delete("a.txt");
    repository.git(&["rm", "--quiet", "--", "b.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["a.txt", "b.txt"]), "Remove both")
        .expect("commit");

    assert_eq!(last_commit_files(&repository), vec!["D a.txt", "D b.txt"]);
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

/// The changes of the Changelist called `name`, by path.
fn changelist_paths(changes: &ChangeSet, name: &str) -> Vec<String> {
    changes
        .changelists
        .iter()
        .find(|list| list.name == name)
        .expect("the Changelist exists")
        .group
        .changes
        .iter()
        .map(|change| change.path.to_string_lossy().into_owned())
        .collect()
}

/// A repository where `a.txt` is a change in the Changelist Feature and `b.txt` one in Changes, the
/// Active changelist, with gikli and the Repository.
fn repository_with_two_changelists(
    sandbox: &Sandbox,
) -> (TestRepository, Gikli, Repository, ChangeSet) {
    let repository = committed_repository(sandbox);
    repository.write("a.txt", "a changed\n");
    repository.write("b.txt", "b changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let changes = gikli
        .move_changes(
            &root,
            &paths(&["a.txt"]),
            &MoveTarget::NewChangelist {
                name: "Feature".to_owned(),
            },
            false,
        )
        .expect("move");
    (repository, gikli, root, changes)
}

#[test]
fn a_whole_changelist_is_committed_on_its_own() {
    let sandbox = Sandbox::new();
    let (repository, gikli, root, changes) = repository_with_two_changelists(&sandbox);

    let after = gikli
        .commit_files(&root, &changelist_paths(&changes, "Feature"), "Feature")
        .expect("commit");

    assert_eq!(last_commit_files(&repository), vec!["M a.txt"]);
    assert_eq!(changelist_paths(&after, "Feature"), Vec::<String>::new());
    assert_eq!(changelist_paths(&after, "Changes"), vec!["b.txt"]);
}

/// git does not remember that a committed file was in a Changelist, and neither does gikli.
#[test]
fn a_committed_change_leaves_its_changelist_for_good() {
    let sandbox = Sandbox::new();
    let (repository, gikli, root, _) = repository_with_two_changelists(&sandbox);

    gikli
        .commit_files(&root, &paths(&["a.txt"]), "Feature")
        .expect("commit");
    repository.write("a.txt", "changed once more\n");
    let later = gikli.read_changes(&root).expect("read changes");

    assert_eq!(changelist_paths(&later, "Feature"), Vec::<String>::new());
    assert_eq!(changelist_paths(&later, "Changes"), vec!["a.txt", "b.txt"]);
}

#[test]
fn the_first_commit_of_a_repository_works() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.git(&["add", "--", "a.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let changes = gikli
        .commit_files(&root, &paths(&["a.txt"]), "First")
        .expect("commit");

    assert_eq!(last_commit_files(&repository), vec!["A a.txt"]);
    assert!(changes.changes.is_empty());
}

#[test]
fn a_path_is_taken_literally() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    if !cfg!(windows) {
        repository.write("*.txt", "star\n");
    }
    repository.write("a.txt", "a\n");
    repository.write("[a].txt", "brackets\n");
    repository.commit_all("Initial commit");
    repository.write("a.txt", "a changed\n");
    repository.write("[a].txt", "brackets changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["[a].txt"]), "Only the brackets")
        .expect("commit");

    assert_eq!(last_commit_files(&repository), vec!["M [a].txt"]);
}

#[test]
fn a_selected_unversioned_file_is_added_in_the_same_step() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write(
        "new.txt", "new
",
    );
    repository.write(
        "other new.txt",
        "other
",
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["new.txt"]), "Add new")
        .expect("commit");

    assert_eq!(last_commit_files(&repository), vec!["A new.txt"]);
    assert_eq!(
        repository.git(&["show", "HEAD:new.txt"]),
        "new
"
    );
    assert_eq!(repository.status_by_git(), vec!["?? other new.txt"]);
}

#[test]
fn a_selected_rename_records_both_of_its_paths() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "a.txt", "moved.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["moved.txt"]), "Move a")
        .expect("commit");

    assert_eq!(
        last_commit_files(&repository),
        vec!["D a.txt", "A moved.txt"]
    );
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

/// git shows such a file only as deleted, at the path the last commit has it.
#[test]
fn a_file_moved_and_then_removed_from_disk_leaves_nothing_in_the_index() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "a.txt", "moved.txt"]);
    repository.delete("moved.txt");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["a.txt"]), "Remove a")
        .expect("commit");

    assert_eq!(last_commit_files(&repository), vec!["D a.txt"]);
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn the_message_is_kept_as_written_whatever_the_configuration() {
    let sandbox = Sandbox::new();
    sandbox.configure(
        "[commit]\n\tcleanup = strip\n\tverbose = true\n[core]\n\tcommentChar = \";\"\n",
    );
    let repository = committed_repository(&sandbox);
    repository.write("a.txt", "a changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let message = "Subject\n\n# 123 is the ticket\n; not a comment either\n\n\n# ------------------------ >8 ------------------------\nThe end  ";

    gikli
        .commit_files(&root, &paths(&["a.txt"]), message)
        .expect("commit");

    assert_eq!(
        repository
            .git(&["cat-file", "-p", "HEAD"])
            .split_once("\n\n")
            .expect("a commit has a message")
            .1,
        format!("{message}\n")
    );
}

#[test]
fn a_selection_longer_than_a_command_line_is_committed() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let names: Vec<String> = (0..1500)
        .map(|number| format!("a folder with a rather long name/file number {number}.txt"))
        .collect();
    repository.write("committed.txt", "committed\n");
    repository.commit_all("Initial commit");
    for name in &names {
        repository.write(name, "new\n");
    }
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let changes = gikli
        .commit_files(&root, &names, "Add them all")
        .expect("commit");

    assert!(changes.changes.is_empty());
    assert_eq!(last_commit_files(&repository).len(), names.len());
}

/// How many commits HEAD holds, as plain git counts them.
fn commit_count(repository: &TestRepository) -> String {
    repository.git(&["rev-list", "--count", "HEAD"])
}

#[test]
fn a_missing_name_or_email_is_asked_for_before_anything_happens() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["config", "--global", "--unset", "user.name"]);
    repository.write("new.txt", "new\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.commit_files(&root, &paths(&["new.txt"]), "Add new");

    assert_eq!(
        result,
        Err(Failure::CommitIdentityMissing {
            name_missing: true,
            email_missing: false,
        })
    );
    assert_eq!(commit_count(&repository), "1\n");
    assert_eq!(repository.status_by_git(), vec!["?? new.txt"]);
}

#[test]
fn the_answer_about_the_identity_can_serve_every_repository() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["config", "--global", "--unset", "user.name"]);
    repository.git(&["config", "--global", "--unset", "user.email"]);
    repository.write("a.txt", "a changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .set_commit_identity(&root, Some("Ada"), Some("ada@example.com"), true)
        .expect("save the identity");
    gikli
        .commit_files(&root, &paths(&["a.txt"]), "Change a")
        .expect("commit");

    assert_eq!(
        repository.git(&["log", "-1", "--format=%an <%ae>"]),
        "Ada <ada@example.com>\n"
    );
    assert_eq!(
        repository.git(&["config", "--global", "user.name"]),
        "Ada\n"
    );
}

#[test]
fn the_answer_about_the_identity_can_serve_this_repository_alone() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["config", "--global", "--unset", "user.email"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .set_commit_identity(&root, None, Some("-ada@example.com"), false)
        .expect("save the identity");

    assert_eq!(
        repository.git(&["config", "--local", "user.email"]),
        "-ada@example.com\n"
    );
    assert!(!repository.git_succeeds(&["config", "--global", "user.email"]));
    assert!(!repository.git_succeeds(&["config", "--local", "user.name"]));
}

#[test]
fn a_commit_needs_a_message() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("new.txt", "new\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.commit_files(&root, &paths(&["new.txt"]), " \n\t\n");

    assert_eq!(result, Err(Failure::CommitMessageEmpty));
    assert_eq!(repository.status_by_git(), vec!["?? new.txt"]);
}

/// What was selected can stop being a change while the user writes the message.
#[test]
fn a_selection_where_nothing_is_a_change_any_more_commits_nothing() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("debug.log", "ignored\n");
    repository.write(".gitignore", "*.log\n");
    repository.commit_all("Ignore logs");
    repository.write("debug.log", "still ignored\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.commit_files(&root, &paths(&["a.txt", "debug.log"]), "Nothing");

    assert_eq!(result, Err(Failure::NothingToCommit));
    assert_eq!(commit_count(&repository), "2\n");
}

#[test]
fn hooks_run_as_configured() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    sandbox.hook("commit-msg", "echo 'Reviewed-by: a hook' >> \"$1\"");
    repository.write("a.txt", "a changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["a.txt"]), "Change a")
        .expect("commit");

    assert_eq!(
        last_message(&repository),
        "Change a\nReviewed-by: a hook\n\n"
    );
}

#[test]
fn a_hook_that_refuses_leaves_everything_as_it_was() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    sandbox.hook("pre-commit", "echo 'Lint found 2 problems' >&2\nexit 1");
    repository.write("a.txt", "a changed\n");
    repository.write("new.txt", "new\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.commit_files(&root, &paths(&["a.txt", "new.txt"]), "Both");

    let Err(Failure::Explained { explanation }) = result else {
        panic!("the hook's failure is explained: {result:?}");
    };
    assert_eq!(
        explanation.message,
        ExplainedMessage::HookFailed {
            hook: "pre-commit".to_owned(),
            output: "Lint found 2 problems\n".to_owned(),
        }
    );
    assert_eq!(commit_count(&repository), "1\n");
    assert_eq!(repository.status_by_git(), vec![" M a.txt", "?? new.txt"]);
}

#[test]
fn the_files_gikli_wrote_for_git_do_not_stay_behind() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("a.txt", "a changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["a.txt"]), "Change a")
        .expect("commit");

    let folder = repository
        .changelists_file()
        .parent()
        .expect("the file has a folder")
        .to_path_buf();
    let names: Vec<_> = std::fs::read_dir(folder)
        .expect("read the folder")
        .map(|entry| entry.expect("an entry").file_name())
        .collect();
    assert_eq!(names, vec!["changelists.json"]);
}

/// The git directory of a linked worktree is outside its working tree.
#[test]
fn a_commit_works_in_a_linked_worktree() {
    let sandbox = Sandbox::new();
    let main = committed_repository(&sandbox);
    let worktree = main.linked_worktree("linked");
    worktree.write("a.txt", "changed in the worktree\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &worktree);

    gikli
        .commit_files(&root, &paths(&["a.txt"]), "In the worktree")
        .expect("commit");

    assert_eq!(last_commit_files(&worktree), vec!["M a.txt"]);
    assert_eq!(commit_count(&main), "1\n");
}

/// A pre-commit hook that says it started and then runs until something ends it.
fn repository_with_a_hook_that_holds_the_commit(sandbox: &Sandbox) -> (TestRepository, PathBuf) {
    let repository = committed_repository(sandbox);
    let holding = sandbox.folder("markers").join("holding");
    sandbox.hook(
        "pre-commit",
        &format!(
            ": > '{}'\nwhile :; do sleep 1; done\n",
            as_git_writes_it(&holding)
        ),
    );
    repository.write("new.txt", "new\n");
    (repository, holding)
}

/// git holds the lock of the index while the hook runs, so the file it was given stays added until
/// the user has removed the lock.
#[test]
fn a_commit_is_a_long_operation_the_user_can_cancel_and_commits_nothing() {
    let sandbox = Sandbox::new();
    let (repository, holding) = repository_with_a_hook_that_holds_the_commit(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = std::thread::scope(|scope| {
        let commit = scope.spawn(|| gikli.commit_files(&root, &paths(&["new.txt"]), "Add new"));
        let give_up = Instant::now() + Duration::from_secs(60);
        while !holding.exists() {
            assert!(Instant::now() < give_up, "the hook did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        let running = gikli.running_operations();
        assert_eq!(
            running[0].operation,
            LongOperation::CommitFiles {
                repository: root.clone()
            }
        );
        gikli.cancel_operation(running[0].id);
        commit.join().expect("the commit ends")
    });

    let Err(Failure::OperationCancelled {
        left_behind: Some(explanation),
    }) = result
    else {
        panic!("the cancelled run left the lock of the index: {result:?}");
    };
    assert!(matches!(
        explanation.message,
        ExplainedMessage::IndexLocked { .. }
    ));
    assert_eq!(commit_count(&repository), "1\n");
    assert_eq!(repository.status_by_git(), vec!["A  new.txt"]);
}

/// `git commit --only` takes a path from the working tree, so committing the old path of the move
/// would commit a file nobody selected.
#[test]
fn a_selected_rename_leaves_out_a_new_file_at_its_old_path() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["mv", "a.txt", "moved.txt"]);
    repository.write("a.txt", "nobody selected this\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["moved.txt"]), "Move a")
        .expect("commit");

    assert_eq!(last_commit_files(&repository), vec!["A moved.txt"]);
    assert_eq!(repository.git(&["show", "HEAD:a.txt"]), "a\n");
    assert_eq!(repository.status_by_git(), vec!["D  a.txt", "?? a.txt"]);
}

#[test]
fn an_empty_answer_about_the_identity_saves_nothing() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["config", "--global", "--unset", "user.name"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.set_commit_identity(&root, Some("Ada"), Some("  "), true);

    assert_eq!(result, Err(Failure::CommitIdentityEmpty));
    assert!(!repository.git_succeeds(&["config", "--global", "user.name"]));
}

#[test]
fn the_answer_about_the_identity_is_saved_without_the_spaces_around_it() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .set_commit_identity(&root, Some("  Ada Lovelace "), None, false)
        .expect("save the identity");

    assert_eq!(
        repository.git(&["config", "--local", "user.name"]),
        "Ada Lovelace\n"
    );
}
