//! The changes view: the uncommitted changes of a Repository.

use std::path::PathBuf;

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{Change, ChangeSet, ChangeStatus, Failure, Gikli};

/// Opens the repository's folder as a Project and reads the changes of its Repository.
fn read_changes(gikli: &Gikli, repository: &TestRepository) -> ChangeSet {
    let project = gikli.open_project(repository.root()).expect("open project");
    gikli
        .read_changes(&project.repositories[0])
        .expect("read changes")
}

fn change(path: &str, status: ChangeStatus) -> Change {
    Change {
        path: PathBuf::from(path),
        status,
    }
}

#[test]
fn an_edited_file_appears_as_modified() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");
    repository.write("README.md", "hello, world\n");

    let changes = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        changes.changes,
        vec![change("README.md", ChangeStatus::Modified)]
    );
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

#[test]
fn a_new_file_added_to_git_appears_as_added() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");
    repository.write("src/main.rs", "fn main() {}\n");
    repository.git(&["add", "src/main.rs"]);

    let changes = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        changes.changes,
        vec![change("src/main.rs", ChangeStatus::Added)]
    );
    assert_eq!(repository.status_by_git(), vec!["A  src/main.rs"]);
}

#[test]
fn a_tracked_file_removed_from_disk_appears_as_deleted() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.write("NOTES.md", "notes\n");
    repository.commit_all("Initial commit");
    repository.delete("NOTES.md");

    let changes = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        changes.changes,
        vec![change("NOTES.md", ChangeStatus::Deleted)]
    );
    assert_eq!(repository.status_by_git(), vec![" D NOTES.md"]);
}

#[test]
fn a_file_removed_with_git_rm_appears_as_deleted() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.write("NOTES.md", "notes\n");
    repository.commit_all("Initial commit");
    repository.git(&["rm", "--quiet", "NOTES.md"]);

    let changes = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        changes.changes,
        vec![change("NOTES.md", ChangeStatus::Deleted)]
    );
    assert_eq!(repository.status_by_git(), vec!["D  NOTES.md"]);
}

#[test]
fn new_files_git_does_not_track_appear_one_by_one_as_untracked() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");
    repository.write("TODO.md", "todo\n");
    repository.write("docs/guide/intro.md", "intro\n");

    let changes = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        changes.changes,
        vec![
            change("TODO.md", ChangeStatus::Untracked),
            change("docs/guide/intro.md", ChangeStatus::Untracked),
        ]
    );
    assert_eq!(
        repository.status_by_git(),
        vec!["?? TODO.md", "?? docs/guide/intro.md"]
    );
}

#[test]
fn paths_with_unicode_and_spaces_appear_exactly_as_on_disk() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("año único ✓");
    repository.write("café ☕.txt", "black\n");
    repository.commit_all("Initial commit");
    repository.write("café ☕.txt", "with milk\n");
    repository.write("notas de reunión/日本語 ファイル.md", "メモ\n");

    let changes = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        changes.changes,
        vec![
            change("café ☕.txt", ChangeStatus::Modified),
            change(
                "notas de reunión/日本語 ファイル.md",
                ChangeStatus::Untracked
            ),
        ]
    );
    assert_eq!(
        repository.status_by_git(),
        vec![" M café ☕.txt", "?? notas de reunión/日本語 ファイル.md"]
    );
    for change in &changes.changes {
        assert!(repository.root().join(&change.path).is_file());
    }
}

#[test]
fn a_repository_with_no_commits_shows_its_changes() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.git(&["add", "README.md"]);
    repository.write("src/main.rs", "fn main() {}\n");

    let changes = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        changes.changes,
        vec![
            change("README.md", ChangeStatus::Added),
            change("src/main.rs", ChangeStatus::Untracked),
        ]
    );
    assert_eq!(
        repository.status_by_git(),
        vec!["A  README.md", "?? src/main.rs"]
    );
    assert!(repository.git(&["rev-list", "--all"]).is_empty());
}

#[test]
fn a_clean_repository_has_no_changes() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");

    let changes = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(changes.changes, vec![]);
    assert!(repository.status_by_git().is_empty());
}

#[test]
fn reading_the_changes_again_refreshes_them() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(repository.root()).expect("open project");
    let first = gikli
        .read_changes(&project.repositories[0])
        .expect("read changes");
    repository.write("README.md", "hello, world\n");
    repository.write("TODO.md", "todo\n");

    let refreshed = gikli
        .read_changes(&project.repositories[0])
        .expect("refresh changes");

    assert_eq!(first.changes, vec![]);
    assert_eq!(refreshed.repository, project.repositories[0]);
    assert_eq!(
        refreshed.changes,
        vec![
            change("README.md", ChangeStatus::Modified),
            change("TODO.md", ChangeStatus::Untracked),
        ]
    );
    assert_eq!(
        repository.status_by_git(),
        vec![" M README.md", "?? TODO.md"]
    );
}

/// Renames get their own status with #7. Until then, the move shows as its two halves, so no
/// file goes missing from the list.
#[test]
fn a_file_moved_with_git_mv_appears_deleted_at_its_old_path_and_added_at_its_new_one() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("old name.md", "hello\n");
    repository.commit_all("Initial commit");
    repository.git(&["mv", "old name.md", "new name.md"]);

    let changes = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        changes.changes,
        vec![
            change("new name.md", ChangeStatus::Added),
            change("old name.md", ChangeStatus::Deleted),
        ]
    );
    assert_eq!(
        repository.status_by_git(),
        vec!["A  new name.md", "D  old name.md"]
    );
}

#[test]
fn the_users_status_settings_do_not_change_the_changes() {
    let sandbox = Sandbox::new();
    sandbox.configure(
        "[status]\n\tshowUntrackedFiles = no\n\trenames = copies\n\trelativePaths = true\n\
         [diff]\n\trenames = copies\n\
         [color]\n\tui = always\n\tstatus = always\n\
         [core]\n\tquotePath = true\n",
    );
    let repository = sandbox.repository("app");
    repository.write("src/lib.rs", "pub fn a() {}\n");
    repository.commit_all("Initial commit");
    repository.write("src/lib.rs", "pub fn b() {}\n");
    repository.write("src/año nuevo.rs", "\n");
    let gikli = sandbox.gikli();
    let project = gikli
        .open_project(&repository.folder("src"))
        .expect("open project");

    let changes = gikli
        .read_changes(&project.repositories[0])
        .expect("read changes");

    assert_eq!(
        changes.changes,
        vec![
            change("src/año nuevo.rs", ChangeStatus::Untracked),
            change("src/lib.rs", ChangeStatus::Modified),
        ]
    );
    assert_eq!(
        repository.status_by_git(),
        vec![" M src/lib.rs", "?? src/año nuevo.rs"]
    );
}

/// Conflicted files get their own status with #7. Until then they stay in the list as modified.
#[test]
fn a_file_with_a_merge_conflict_still_appears() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "base\n");
    repository.commit_all("Initial commit");
    repository.git(&["switch", "--quiet", "--create", "feature"]);
    repository.write("README.md", "feature\n");
    repository.commit_all("Feature");
    repository.git(&["switch", "--quiet", "main"]);
    repository.write("README.md", "main\n");
    repository.commit_all("Main");
    let merge = repository.git_may_fail(&["merge", "--quiet", "feature"]);

    let changes = read_changes(&sandbox.gikli(), &repository);

    assert!(!merge, "the merge stops on the conflict");
    assert_eq!(
        changes.changes,
        vec![change("README.md", ChangeStatus::Modified)]
    );
    assert_eq!(repository.status_by_git(), vec!["UU README.md"]);
}

#[test]
fn a_repository_that_is_gone_is_reported_instead_of_showing_no_changes() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(
        "README.md",
        "hello
",
    );
    let gikli = sandbox.gikli();
    let project = gikli.open_project(repository.root()).expect("open project");
    std::fs::remove_dir_all(repository.root().join(".git")).expect("remove the git directory");

    let changes = gikli.read_changes(&project.repositories[0]);

    assert_eq!(
        changes,
        Err(Failure::NotInRepository {
            folder: project.repositories[0].root.clone()
        })
    );
    assert!(!sandbox.is_inside_repository_by_git(repository.root()));
}
