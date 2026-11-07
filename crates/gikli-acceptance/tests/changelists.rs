//! Changelists: the changes view groups changes into Changelists, new changes go to the Active
//! changelist, and the Changelists follow what actually happens in git.

use gikli_acceptance::{Sandbox, TWO_CHANGELISTS, TestRepository};
use gikli_core::{ChangeSet, Changelist, Failure, Gikli};
use std::fs;

/// Opens the repository's folder as a Project and reads the changes of its Repository.
fn read_changes(gikli: &Gikli, repository: &TestRepository) -> ChangeSet {
    let project = gikli.open_project(repository.root()).expect("open project");
    gikli
        .read_changes(&project.repositories[0])
        .expect("read changes")
}

fn set_active(gikli: &Gikli, repository: &TestRepository, id: &str) -> ChangeSet {
    let project = gikli.open_project(repository.root()).expect("open project");
    gikli
        .set_active_changelist(&project.repositories[0], id)
        .expect("set the Active changelist")
}

fn list<'a>(change_set: &'a ChangeSet, name: &str) -> &'a Changelist {
    change_set
        .changelists
        .iter()
        .find(|list| list.name == name)
        .expect("the Changelist exists")
}

/// The paths of the changes in the Changelist called `name`.
fn paths_in(change_set: &ChangeSet, name: &str) -> Vec<String> {
    list(change_set, name)
        .group
        .changes
        .iter()
        .map(|change| change.path.to_string_lossy().into_owned())
        .collect()
}

/// The paths of the files in Unversioned Files.
fn unversioned_paths(change_set: &ChangeSet) -> Vec<String> {
    change_set
        .unversioned_files
        .changes
        .iter()
        .map(|change| change.path.to_string_lossy().into_owned())
        .collect()
}

fn committed_repository(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.write("notes.md", "notes\n");
    repository.commit_all("Initial commit");
    repository
}

fn repository_with_two_changelists(sandbox: &Sandbox) -> TestRepository {
    let repository = committed_repository(sandbox);
    repository.store_changelists(TWO_CHANGELISTS);
    repository
}

#[test]
fn a_repository_starts_with_one_changelist_named_changes_which_is_active() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(change_set.changelists.len(), 1);
    assert_eq!(change_set.changelists[0].name, "Changes");
    assert!(change_set.changelists[0].active);
    assert!(paths_in(&change_set, "Changes").is_empty());
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn new_changes_go_to_the_active_changelist_and_new_untracked_files_to_unversioned_files() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("README.md", "hello, world\n");
    repository.delete("notes.md");
    repository.write("staged.md", "staged\n");
    repository.git(&["add", "staged.md"]);
    repository.write("new.md", "new\n");

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        paths_in(&change_set, "Changes"),
        vec!["README.md", "notes.md", "staged.md"]
    );
    assert_eq!(unversioned_paths(&change_set), vec!["new.md"]);
    assert_eq!(
        repository.status_by_git(),
        vec![" M README.md", " D notes.md", "A  staged.md", "?? new.md"]
    );
}

#[test]
fn the_user_can_set_any_changelist_as_the_active_changelist() {
    let sandbox = Sandbox::new();
    let repository = repository_with_two_changelists(&sandbox);
    let gikli = sandbox.gikli();

    let change_set = set_active(&gikli, &repository, "feature");

    assert!(list(&change_set, "Feature").active);
    assert!(!list(&change_set, "Changes").active);
    repository.write("README.md", "hello, world\n");
    let change_set = read_changes(&gikli, &repository);
    assert_eq!(paths_in(&change_set, "Feature"), vec!["README.md"]);
    assert!(paths_in(&change_set, "Changes").is_empty());
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

/// The change happened while Changes was active, even when gikli had not read it yet.
#[test]
fn a_change_made_before_the_user_sets_another_changelist_active_stays_where_it_went() {
    let sandbox = Sandbox::new();
    let repository = repository_with_two_changelists(&sandbox);
    repository.write("README.md", "hello, world\n");

    let change_set = set_active(&sandbox.gikli(), &repository, "feature");

    assert_eq!(paths_in(&change_set, "Changes"), vec!["README.md"]);
    assert!(paths_in(&change_set, "Feature").is_empty());
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

#[test]
fn setting_active_a_changelist_that_does_not_exist_changes_nothing() {
    let sandbox = Sandbox::new();
    let repository = repository_with_two_changelists(&sandbox);
    let gikli = sandbox.gikli();
    let project = gikli.open_project(repository.root()).expect("open project");

    let failure = gikli
        .set_active_changelist(&project.repositories[0], "gone")
        .expect_err("there is no such Changelist");

    assert_eq!(
        failure,
        Failure::ChangelistGone {
            id: "gone".to_owned()
        }
    );
    assert!(list(&read_changes(&gikli, &repository), "Changes").active);
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn changelists_survive_restarting_gikli() {
    let sandbox = Sandbox::new();
    let repository = repository_with_two_changelists(&sandbox);
    repository.write("README.md", "hello, world\n");
    set_active(&sandbox.gikli(), &repository, "feature");
    repository.write("notes.md", "more notes\n");
    read_changes(&sandbox.gikli(), &repository);

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert!(list(&change_set, "Feature").active);
    assert_eq!(paths_in(&change_set, "Changes"), vec!["README.md"]);
    assert_eq!(paths_in(&change_set, "Feature"), vec!["notes.md"]);
    assert_eq!(
        repository.status_by_git(),
        vec![" M README.md", " M notes.md"]
    );
}

#[test]
fn changelists_survive_moving_the_project_folder() {
    let sandbox = Sandbox::new();
    let repository = repository_with_two_changelists(&sandbox);
    repository.write("README.md", "hello, world\n");
    set_active(&sandbox.gikli(), &repository, "feature");
    let moved = sandbox.root().join("moved");
    fs::rename(repository.root(), &moved).expect("move the Project folder");
    let repository = sandbox.existing_repository(&moved);

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert!(list(&change_set, "Feature").active);
    assert_eq!(paths_in(&change_set, "Changes"), vec!["README.md"]);
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

#[test]
fn changelists_are_kept_in_the_git_directory_and_never_appear_in_git_status() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("README.md", "hello, world\n");

    read_changes(&sandbox.gikli(), &repository);

    assert!(
        repository
            .root()
            .join(".git/gikli/changelists.json")
            .is_file()
    );
    assert_eq!(
        repository.git(&[
            "status",
            "--porcelain",
            "--ignored",
            "--untracked-files=all"
        ]),
        " M README.md\n"
    );
}

#[test]
fn each_linked_worktree_keeps_its_own_changelists() {
    let sandbox = Sandbox::new();
    let main = committed_repository(&sandbox);
    let worktree = main.linked_worktree("feature");
    main.store_changelists(TWO_CHANGELISTS);
    worktree.store_changelists(TWO_CHANGELISTS);
    main.write("README.md", "in main\n");
    worktree.write("notes.md", "in the worktree\n");
    let gikli = sandbox.gikli();

    set_active(&gikli, &worktree, "feature");
    worktree.write("README.md", "in the worktree\n");

    let in_main = read_changes(&gikli, &main);
    let in_worktree = read_changes(&gikli, &worktree);
    assert!(list(&in_main, "Changes").active);
    assert!(list(&in_worktree, "Feature").active);
    assert_eq!(paths_in(&in_main, "Changes"), vec!["README.md"]);
    assert_eq!(paths_in(&in_worktree, "Changes"), vec!["notes.md"]);
    assert_eq!(paths_in(&in_worktree, "Feature"), vec!["README.md"]);
    assert!(
        main.root()
            .join(".git/worktrees/feature/gikli/changelists.json")
            .is_file()
    );
    assert_eq!(main.status_by_git(), vec![" M README.md"]);
    assert_eq!(
        worktree.status_by_git(),
        vec![" M README.md", " M notes.md"]
    );
}

/// A newer gikli may keep what this one does not know about, so this one must not write over it.
#[test]
fn changelists_saved_by_a_newer_gikli_are_refused_and_left_as_they_are() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    let newer = r#"{ "version": 2, "somethingNew": true }"#;
    repository.store_changelists(newer);
    repository.write("README.md", "hello, world\n");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(repository.root()).expect("open project");

    let failure = gikli
        .read_changes(&project.repositories[0])
        .expect_err("gikli does not know this format");

    assert_eq!(
        failure,
        Failure::ChangelistsFromNewerGikli {
            file: repository.root().join(".git/gikli/changelists.json"),
            version: 2,
        }
    );
    assert_eq!(
        fs::read_to_string(repository.changelists_file()).expect("read Changelists"),
        newer
    );
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

/// The assignments may still be there for the user to rescue, so gikli does not write over them.
#[test]
fn changelists_gikli_cannot_read_are_reported_and_left_as_they_are() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.store_changelists("not what gikli wrote");
    repository.write("README.md", "hello, world\n");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(repository.root()).expect("open project");

    let failure = gikli
        .read_changes(&project.repositories[0])
        .expect_err("gikli cannot read this");

    assert!(
        matches!(&failure, Failure::ChangelistsUnreadable { file, .. }
            if *file == repository.root().join(".git/gikli/changelists.json")),
        "{failure:?}"
    );
    assert_eq!(
        fs::read_to_string(repository.changelists_file()).expect("read Changelists"),
        "not what gikli wrote"
    );
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

/// Changelists whose Active changelist is not one of them would leave new changes nowhere to go.
#[test]
fn changelists_without_their_active_changelist_are_reported() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.store_changelists(
        r#"{ "version": 1, "active": "gone", "head": null, "changelists": [] }"#,
    );
    let gikli = sandbox.gikli();
    let project = gikli.open_project(repository.root()).expect("open project");

    let failure = gikli
        .read_changes(&project.repositories[0])
        .expect_err("gikli cannot use this");

    assert!(
        matches!(failure, Failure::ActiveChangelistMissing { .. }),
        "{failure:?}"
    );
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

/// Something in the way of the Changelists does not stop the user seeing their changes.
#[test]
fn changelists_that_cannot_be_saved_do_not_stop_the_changes_being_read() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("README.md", "hello, world\n");
    let file = repository.changelists_file();
    fs::write(file.parent().expect("file has a folder"), "in the way").expect("write file");

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(paths_in(&change_set, "Changes"), vec!["README.md"]);
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

/// A folder where gikli writes the new version of the file stops it being saved, on every system.
#[test]
fn setting_active_a_changelist_when_it_cannot_be_saved_fails_and_changes_nothing() {
    let sandbox = Sandbox::new();
    let repository = repository_with_two_changelists(&sandbox);
    let file = repository.changelists_file();
    fs::create_dir(file.with_extension("json.partial")).expect("create folder");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(repository.root()).expect("open project");

    let failure = gikli
        .set_active_changelist(&project.repositories[0], "feature")
        .expect_err("the choice cannot be kept");

    assert!(
        matches!(&failure, Failure::ChangelistsNotSaved { file, .. }
            if *file == repository.root().join(".git/gikli/changelists.json")),
        "{failure:?}"
    );
    assert!(list(&read_changes(&gikli, &repository), "Changes").active);
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

/// `README.md` changed while Changes was active, and Feature active from then on.
fn repository_with_a_change_in_an_inactive_changelist(sandbox: &Sandbox) -> TestRepository {
    let repository = repository_with_two_changelists(sandbox);
    repository.write("README.md", "hello, world\n");
    set_active(&sandbox.gikli(), &repository, "feature");
    repository
}

#[test]
fn a_changed_file_stays_in_its_changelist_when_it_is_edited_again() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change_in_an_inactive_changelist(&sandbox);
    repository.write("README.md", "hello again\n");

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(paths_in(&change_set, "Changes"), vec!["README.md"]);
    assert!(paths_in(&change_set, "Feature").is_empty());
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

#[test]
fn a_changed_file_stays_in_its_changelist_when_it_is_staged() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change_in_an_inactive_changelist(&sandbox);
    repository.git(&["add", "README.md"]);

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(paths_in(&change_set, "Changes"), vec!["README.md"]);
    assert_eq!(repository.status_by_git(), vec!["M  README.md"]);
}

/// The reference shows a move as one change, so it keeps its place like any other edit.
#[test]
fn a_changed_file_stays_in_its_changelist_when_it_is_moved() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change_in_an_inactive_changelist(&sandbox);
    read_changes(&sandbox.gikli(), &repository);
    repository.git(&["mv", "README.md", "GUIDE.md"]);

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(paths_in(&change_set, "Changes"), vec!["GUIDE.md"]);
    assert!(paths_in(&change_set, "Feature").is_empty());
    assert_eq!(
        repository.status_by_git(),
        vec!["AM GUIDE.md", "D  README.md"]
    );
}

/// An unversioned file was never in a Changelist, so adding it is a new change.
#[test]
fn a_file_added_to_git_leaves_unversioned_files_for_the_active_changelist() {
    let sandbox = Sandbox::new();
    let repository = repository_with_two_changelists(&sandbox);
    repository.write("new.md", "new\n");
    let gikli = sandbox.gikli();
    set_active(&gikli, &repository, "feature");
    assert_eq!(
        unversioned_paths(&read_changes(&gikli, &repository)),
        vec!["new.md"]
    );

    repository.git(&["add", "new.md"]);

    let change_set = read_changes(&gikli, &repository);
    assert!(unversioned_paths(&change_set).is_empty());
    assert_eq!(paths_in(&change_set, "Feature"), vec!["new.md"]);
    assert_eq!(repository.status_by_git(), vec!["A  new.md"]);
}

/// Taken back to what HEAD has, the file no longer belongs anywhere. gikli does not remember
/// earlier assignments.
#[test]
fn a_file_that_is_no_longer_changed_leaves_its_changelist_for_good() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change_in_an_inactive_changelist(&sandbox);
    let gikli = sandbox.gikli();
    repository.write("README.md", "hello\n");
    assert!(paths_in(&read_changes(&gikli, &repository), "Changes").is_empty());

    repository.write("README.md", "hello, again\n");

    let change_set = read_changes(&gikli, &repository);
    assert!(paths_in(&change_set, "Changes").is_empty());
    assert_eq!(paths_in(&change_set, "Feature"), vec!["README.md"]);
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

#[test]
fn a_file_committed_outside_gikli_leaves_its_changelist_and_a_later_change_goes_to_the_active_one()
{
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change_in_an_inactive_changelist(&sandbox);
    let gikli = sandbox.gikli();
    repository.commit_all("Commit from a terminal");
    let after_the_commit = read_changes(&gikli, &repository);
    assert!(paths_in(&after_the_commit, "Changes").is_empty());
    assert!(paths_in(&after_the_commit, "Feature").is_empty());

    repository.write("README.md", "hello again\n");

    let change_set = read_changes(&gikli, &repository);
    assert!(paths_in(&change_set, "Changes").is_empty());
    assert_eq!(paths_in(&change_set, "Feature"), vec!["README.md"]);
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

/// The commit and the edit both happened while gikli was closed, so no read saw the file without
/// changes. The history says it was committed.
#[test]
fn a_file_committed_and_changed_again_while_gikli_was_closed_goes_to_the_active_changelist() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change_in_an_inactive_changelist(&sandbox);
    repository.write("notes.md", "more notes\n");
    read_changes(&sandbox.gikli(), &repository);
    repository.git(&[
        "commit",
        "--quiet",
        "--message",
        "From a terminal",
        "README.md",
    ]);
    repository.write("README.md", "hello again\n");

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert!(paths_in(&change_set, "Changes").is_empty());
    assert_eq!(
        paths_in(&change_set, "Feature"),
        vec!["README.md", "notes.md"]
    );
    assert_eq!(
        repository.status_by_git(),
        vec![" M README.md", " M notes.md"]
    );
}

/// The same in a Repository whose first commit that was, with no earlier commit to compare with.
#[test]
fn a_file_in_the_first_commit_and_changed_again_while_gikli_was_closed_goes_to_the_active_changelist()
 {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.store_changelists(TWO_CHANGELISTS);
    repository.write("README.md", "hello\n");
    repository.write("notes.md", "notes\n");
    repository.git(&["add", "README.md", "notes.md"]);
    set_active(&sandbox.gikli(), &repository, "feature");
    repository.git(&["commit", "--quiet", "--message", "First", "README.md"]);
    repository.write("README.md", "hello again\n");

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(paths_in(&change_set, "Changes"), vec!["notes.md"]);
    assert_eq!(paths_in(&change_set, "Feature"), vec!["README.md"]);
    assert_eq!(
        repository.status_by_git(),
        vec![" M README.md", "A  notes.md"]
    );
}

/// Going back in history commits nothing, so a file still changed stays where it is.
#[test]
fn a_changed_file_stays_in_its_changelist_when_head_moves_back() {
    let sandbox = Sandbox::new();
    let repository = repository_with_two_changelists(&sandbox);
    repository.write("README.md", "second\n");
    repository.commit_all("Second");
    repository.write("README.md", "second, edited\n");
    set_active(&sandbox.gikli(), &repository, "feature");

    repository.git(&["reset", "--quiet", "--soft", "HEAD~1"]);

    let change_set = read_changes(&sandbox.gikli(), &repository);
    assert_eq!(paths_in(&change_set, "Changes"), vec!["README.md"]);
    assert!(paths_in(&change_set, "Feature").is_empty());
    assert_eq!(repository.status_by_git(), vec!["MM README.md"]);
}

/// Rewording the last commit commits nothing, even when the file is in that commit.
#[test]
fn a_changed_file_stays_in_its_changelist_when_the_last_commit_is_reworded() {
    let sandbox = Sandbox::new();
    let repository = repository_with_two_changelists(&sandbox);
    repository.write("README.md", "second\n");
    repository.commit_all("Second");
    repository.write("README.md", "second, edited\n");
    set_active(&sandbox.gikli(), &repository, "feature");

    repository.git(&["commit", "--quiet", "--amend", "--message", "Reworded"]);

    let change_set = read_changes(&sandbox.gikli(), &repository);
    assert_eq!(paths_in(&change_set, "Changes"), vec!["README.md"]);
    assert!(paths_in(&change_set, "Feature").is_empty());
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

/// Added to the last commit, the file was committed like in any other commit.
#[test]
fn a_file_amended_into_the_last_commit_and_changed_again_goes_to_the_active_changelist() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change_in_an_inactive_changelist(&sandbox);
    repository.write("notes.md", "more notes\n");
    read_changes(&sandbox.gikli(), &repository);
    repository.git(&["add", "README.md"]);
    repository.git(&["commit", "--quiet", "--amend", "--message", "Amended"]);
    repository.write("README.md", "hello again\n");

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert!(paths_in(&change_set, "Changes").is_empty());
    assert_eq!(
        paths_in(&change_set, "Feature"),
        vec!["README.md", "notes.md"]
    );
    assert_eq!(
        repository.status_by_git(),
        vec![" M README.md", " M notes.md"]
    );
}

/// A commit that leaves `notes.md` out says nothing about it.
#[test]
fn a_commit_outside_gikli_leaves_the_other_files_of_the_changelist_in_it() {
    let sandbox = Sandbox::new();
    let repository = repository_with_two_changelists(&sandbox);
    repository.write("README.md", "hello, world\n");
    repository.write("notes.md", "more notes\n");
    set_active(&sandbox.gikli(), &repository, "feature");

    repository.git(&[
        "commit",
        "--quiet",
        "--message",
        "From a terminal",
        "README.md",
    ]);

    let change_set = read_changes(&sandbox.gikli(), &repository);
    assert_eq!(paths_in(&change_set, "Changes"), vec!["notes.md"]);
    assert_eq!(repository.status_by_git(), vec![" M notes.md"]);
}

/// A file with a path that is not valid UTF-8 can only exist on a system that allows such names.
#[cfg(target_os = "linux")]
#[test]
fn a_file_whose_path_is_not_utf8_stays_in_its_changelist() {
    use std::os::unix::ffi::OsStrExt;

    let sandbox = Sandbox::new();
    let repository = repository_with_two_changelists(&sandbox);
    let name = std::ffi::OsStr::from_bytes(b"caf\xe9.md");
    fs::write(repository.root().join(name), "one\n").expect("write file");
    repository.git(&["add", "--all"]);
    repository.git(&["commit", "--quiet", "--message", "Add"]);
    fs::write(repository.root().join(name), "two\n").expect("write file");
    set_active(&sandbox.gikli(), &repository, "feature");

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(list(&change_set, "Changes").group.changes.len(), 1);
    assert!(list(&change_set, "Feature").group.changes.is_empty());
    assert_eq!(repository.status_by_git().len(), 1);
}
