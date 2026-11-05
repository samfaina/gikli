//! The changes view: the uncommitted changes of a Repository.

use std::path::PathBuf;

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{Change, ChangeSet, ChangeStatus, ConflictKind, Failure, Gikli, SubmoduleState};

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
        original_path: None,
        conflict: None,
        submodule: None,
    }
}

fn renamed(path: &str, original_path: &str) -> Change {
    Change {
        original_path: Some(PathBuf::from(original_path)),
        ..change(path, ChangeStatus::Renamed)
    }
}

#[test]
fn an_edited_file_appears_as_modified() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");
    repository.write("README.md", "hello, world\n");

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
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

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
        vec![change("src/main.rs", ChangeStatus::Added)]
    );
    assert_eq!(repository.status_by_git(), vec!["A  src/main.rs"]);
}

/// As in JetBrains, a file added and then deleted is no change: it is in neither the last commit
/// nor the working tree.
#[test]
fn a_new_file_added_to_git_and_then_removed_from_disk_does_not_appear() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");
    repository.write("draft.md", "draft\n");
    repository.git(&["add", "draft.md"]);
    repository.delete("draft.md");

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(change_set.changes, vec![]);
    assert_eq!(repository.status_by_git(), vec!["AD draft.md"]);
}

#[test]
fn a_tracked_file_removed_from_disk_appears_as_deleted() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.write("NOTES.md", "notes\n");
    repository.commit_all("Initial commit");
    repository.delete("NOTES.md");

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
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

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
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

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
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

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
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
    for change in &change_set.changes {
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

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
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

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(change_set.changes, vec![]);
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

#[test]
fn a_file_moved_with_git_mv_appears_as_renamed_with_its_original_path() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(
        "old name.md",
        "hello
",
    );
    repository.commit_all("Initial commit");
    repository.git(&["mv", "old name.md", "new name.md"]);

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
        vec![renamed("new name.md", "old name.md")]
    );
    assert_eq!(
        repository.git(&["status", "--porcelain", "-z", "--find-renames"]),
        "R  new name.md old name.md "
    );
}

/// git looks for copies only when the user's `status.renames` asks for them, and gikli keeps
/// that setting.
#[test]
fn a_copied_file_appears_as_copied_with_its_original_path_when_git_looks_for_copies() {
    let sandbox = Sandbox::new();
    sandbox.configure(
        "[status]
	renames = copies
",
    );
    let repository = sandbox.repository("app");
    repository.write(
        "lines.txt",
        "one
two
three
four
five
",
    );
    repository.commit_all("Initial commit");
    repository.write(
        "copy.txt",
        "one
two
three
four
five
",
    );
    repository.write(
        "lines.txt",
        "one
two
three
four
five
six
",
    );
    repository.git(&["add", "--all"]);

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
        vec![
            Change {
                original_path: Some(PathBuf::from("lines.txt")),
                ..change("copy.txt", ChangeStatus::Copied)
            },
            change("lines.txt", ChangeStatus::Modified),
        ]
    );
    assert_eq!(
        repository.git(&["status", "--porcelain", "-z"]),
        "C  copy.txt lines.txt M  lines.txt "
    );
}

/// A repository that ignores `*.log` and the `target` folder, with a new file of each kind and
/// one untracked file.
fn repository_with_ignored_files(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write(
        ".gitignore",
        "*.log
target/
",
    );
    repository.commit_all("Initial commit");
    repository.write(
        "build.log",
        "log
",
    );
    repository.write(
        "target/debug/app",
        "binary
",
    );
    repository.write(
        "target/release/app",
        "binary
",
    );
    repository.write(
        "TODO.md", "todo
",
    );
    repository
}

#[test]
fn ignored_files_do_not_appear_by_default() {
    let sandbox = Sandbox::new();
    let repository = repository_with_ignored_files(&sandbox);

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
        vec![change("TODO.md", ChangeStatus::Untracked)]
    );
    assert!(!sandbox.gikli().settings().show_ignored_files);
}

/// An ignored folder appears once, as git reports it, not file by file.
#[test]
fn ignored_files_appear_when_the_user_turns_them_on_and_stay_on() {
    let sandbox = Sandbox::new();
    let repository = repository_with_ignored_files(&sandbox);

    let settings = sandbox
        .gikli()
        .set_show_ignored_files(true)
        .expect("turn on ignored files");
    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert!(settings.show_ignored_files);
    assert_eq!(
        change_set.changes,
        vec![
            change("TODO.md", ChangeStatus::Untracked),
            change("build.log", ChangeStatus::Ignored),
            change("target/", ChangeStatus::Ignored),
        ]
    );
    assert_eq!(
        repository.git(&["status", "--porcelain", "-z", "--ignored"]),
        "?? TODO.md !! build.log !! target/ "
    );
}

/// A repository on `main` with the repository `library` added as the submodule `lib`.
fn repository_with_a_submodule(sandbox: &Sandbox) -> TestRepository {
    // Local paths are not allowed as submodule URLs by default since git 2.38.1.
    sandbox.configure(
        "[protocol \"file\"]
	allow = always
",
    );
    let library = sandbox.repository("library");
    library.write(
        "lib.rs",
        "pub fn a() {}
",
    );
    library.commit_all("Library");
    let repository = sandbox.repository("app");
    repository.write(
        "README.md",
        "hello
",
    );
    repository.commit_all("Initial commit");
    let url = library.root().to_string_lossy().into_owned();
    repository.git(&["submodule", "--quiet", "add", &url, "lib"]);
    repository
}

fn submodule(path: &str, status: ChangeStatus, state: SubmoduleState) -> Change {
    Change {
        submodule: Some(state),
        ..change(path, status)
    }
}

#[test]
fn a_new_submodule_appears_as_added_with_its_state() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_submodule(&sandbox);

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
        vec![
            change(".gitmodules", ChangeStatus::Added),
            submodule(
                "lib",
                ChangeStatus::Added,
                SubmoduleState {
                    commit_changed: false,
                    has_tracked_changes: false,
                    has_untracked_files: false,
                }
            ),
        ]
    );
    assert_eq!(repository.status_by_git(), vec!["A  .gitmodules", "A  lib"]);
}

#[test]
fn a_submodule_with_a_new_commit_and_changes_inside_appears_as_modified_with_its_state() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_submodule(&sandbox);
    repository.commit_all("Add the library");
    repository.write(
        "lib/lib.rs",
        "pub fn b() {}
",
    );
    repository.git(&["-C", "lib", "commit", "--quiet", "--all", "--message", "B"]);
    repository.write(
        "lib/lib.rs",
        "pub fn c() {}
",
    );
    repository.write(
        "lib/notes.md",
        "notes
",
    );

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
        vec![submodule(
            "lib",
            ChangeStatus::Modified,
            SubmoduleState {
                commit_changed: true,
                has_tracked_changes: true,
                has_untracked_files: true,
            }
        )]
    );
    assert_eq!(repository.status_by_git(), vec![" M lib"]);
}

#[test]
fn a_submodule_with_only_new_files_inside_appears_with_that_state() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_submodule(&sandbox);
    repository.commit_all("Add the library");
    repository.write(
        "lib/notes.md",
        "notes
",
    );

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
        vec![submodule(
            "lib",
            ChangeStatus::Modified,
            SubmoduleState {
                commit_changed: false,
                has_tracked_changes: false,
                has_untracked_files: true,
            }
        )]
    );
    assert_eq!(repository.status_by_git(), vec![" M lib"]);
}

#[test]
fn with_sparse_checkout_files_outside_the_sparse_set_are_not_reported_as_deleted() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(
        "README.md",
        "hello
",
    );
    repository.write(
        "src/main.rs",
        "fn main() {}
",
    );
    repository.write(
        "docs/guide.md",
        "guide
",
    );
    repository.commit_all("Initial commit");
    repository.git(&["sparse-checkout", "init", "--cone"]);
    repository.git(&["sparse-checkout", "set", "src"]);
    repository.write(
        "src/main.rs",
        "fn main() { run() }
",
    );

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
        vec![change("src/main.rs", ChangeStatus::Modified)]
    );
    assert!(!repository.root().join("docs/guide.md").exists());
    assert_eq!(repository.status_by_git(), vec![" M src/main.rs"]);
}

/// With `core.autocrlf`, git converts line endings on the way in and out: a file whose only
/// difference from the last commit is its line endings is no change.
#[test]
fn with_autocrlf_files_that_differ_only_in_line_endings_do_not_appear() {
    for setting in ["true", "input"] {
        let sandbox = Sandbox::new();
        sandbox.configure(&format!(
            "[core]
	autocrlf = {setting}
"
        ));
        let repository = sandbox.repository("app");
        repository.write(
            "unix.txt", "one
two
",
        );
        repository.write(
            "windows.txt",
            "one
two
",
        );
        repository.commit_all("Initial commit");
        // An editor that writes the other line endings, as each platform's editors do.
        repository.write(
            "unix.txt", "one
two
",
        );
        repository.write(
            "windows.txt",
            "one
two
",
        );
        repository.write(
            "edited.txt",
            "new
",
        );

        let change_set = read_changes(&sandbox.gikli(), &repository);

        assert_eq!(
            change_set.changes,
            vec![change("edited.txt", ChangeStatus::Untracked)],
            "core.autocrlf = {setting}"
        );
        assert_eq!(repository.status_by_git(), vec!["?? edited.txt"]);
    }
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

    let change_set = gikli
        .read_changes(&project.repositories[0])
        .expect("read changes");

    assert_eq!(
        change_set.changes,
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

/// A repository on `main` whose merge of `feature` stops on a conflict over `README.md`: `main`
/// changes it to `ours`, and `feature` either changes it to `theirs` or, with None, deletes it.
fn repository_with_a_conflict(sandbox: &Sandbox, theirs: Option<&str>) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write(
        "README.md",
        "base
",
    );
    repository.commit_all("Initial commit");
    repository.git(&["switch", "--quiet", "--create", "feature"]);
    match theirs {
        Some(contents) => repository.write("README.md", contents),
        None => repository.delete("README.md"),
    }
    repository.commit_all("Feature");
    repository.git(&["switch", "--quiet", "main"]);
    repository.write(
        "README.md",
        "ours
",
    );
    repository.commit_all("Main");
    assert!(
        !repository.git_succeeds(&["merge", "--quiet", "feature"]),
        "the merge stops on the conflict"
    );
    repository
}

fn conflicted(path: &str, kind: ConflictKind) -> Change {
    Change {
        conflict: Some(kind),
        ..change(path, ChangeStatus::Conflicted)
    }
}

#[test]
fn a_file_both_sides_changed_appears_as_conflicted_by_both_modifying_it() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_conflict(
        &sandbox,
        Some(
            "theirs
",
        ),
    );

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
        vec![conflicted("README.md", ConflictKind::BothModified)]
    );
    assert_eq!(repository.status_by_git(), vec!["UU README.md"]);
}

#[test]
fn a_file_changed_here_and_deleted_on_the_merged_branch_appears_as_conflicted_by_their_deletion() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_conflict(&sandbox, None);

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        change_set.changes,
        vec![conflicted("README.md", ConflictKind::DeletedByThem)]
    );
    assert_eq!(repository.status_by_git(), vec!["UD README.md"]);
}

#[test]
fn a_repository_that_is_gone_is_reported_instead_of_showing_no_changes() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(repository.root()).expect("open project");
    std::fs::remove_dir_all(repository.root().join(".git")).expect("remove the git directory");

    let read = gikli.read_changes(&project.repositories[0]);

    assert_eq!(
        read,
        Err(Failure::ChangesUnreadable {
            folder: project.repositories[0].root.clone()
        })
    );
    assert!(!sandbox.is_inside_repository_by_git(repository.root()));
}
