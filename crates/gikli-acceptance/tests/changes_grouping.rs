//! The changes view grouped by folder or shown as a flat list.

use std::path::PathBuf;

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{Change, ChangeGrouping, ChangeNode, ChangeSet, ChangeStatus, Gikli};

/// Opens the repository's folder as a Project and reads the changes of its Repository.
fn read_changes(gikli: &Gikli, repository: &TestRepository) -> ChangeSet {
    let project = gikli.open_project(repository.root()).expect("open project");
    gikli
        .read_changes(&project.repositories[0])
        .expect("read changes")
}

fn file(path: &str, status: ChangeStatus) -> ChangeNode {
    ChangeNode::File(Change {
        path: PathBuf::from(path),
        status,
        original_path: None,
        conflict: None,
        submodule: None,
    })
}

fn folder(path: &str, has_changes: bool, children: Vec<ChangeNode>) -> ChangeNode {
    ChangeNode::Folder {
        path: PathBuf::from(path),
        has_changes,
        children,
    }
}

/// A repository with a change at its root, one in a folder and one in a nested folder.
fn repository_with_changes_in_folders(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.write("docs/guide.md", "guide\n");
    repository.write("src/main.rs", "fn main() {}\n");
    repository.commit_all("Initial commit");
    repository.write("README.md", "hello, world\n");
    repository.delete("docs/guide.md");
    repository.write("src/main.rs", "fn main() { run() }\n");
    repository.write("src/ui/added.rs", "pub fn added() {}\n");
    repository.git(&["add", "src/ui/added.rs"]);
    repository.write("src/ui/view.rs", "pub fn view() {}\n");
    repository
}

/// Folders come before files, each in order of name, as in a file browser.
#[test]
fn changes_are_grouped_by_folder_by_default() {
    let sandbox = Sandbox::new();
    let repository = repository_with_changes_in_folders(&sandbox);

    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(
        sandbox.gikli().settings().change_grouping,
        ChangeGrouping::Folder
    );
    assert_eq!(
        change_set.changelists[0].group.nodes,
        vec![
            folder(
                "docs",
                true,
                vec![file("docs/guide.md", ChangeStatus::Deleted)]
            ),
            folder(
                "src",
                true,
                vec![
                    folder(
                        "src/ui",
                        true,
                        vec![file("src/ui/added.rs", ChangeStatus::Added)]
                    ),
                    file("src/main.rs", ChangeStatus::Modified),
                ]
            ),
            file("README.md", ChangeStatus::Modified),
        ]
    );
    assert_eq!(
        change_set.unversioned_files.nodes,
        vec![folder(
            "src",
            true,
            vec![folder(
                "src/ui",
                true,
                vec![file("src/ui/view.rs", ChangeStatus::Untracked)]
            )]
        )]
    );
    assert_eq!(
        repository.status_by_git(),
        vec![
            " M README.md",
            " D docs/guide.md",
            " M src/main.rs",
            "A  src/ui/added.rs",
            "?? src/ui/view.rs"
        ]
    );
}

#[test]
fn changes_are_shown_as_a_flat_list_when_the_user_chooses_it_and_stay_so() {
    let sandbox = Sandbox::new();
    let repository = repository_with_changes_in_folders(&sandbox);

    let settings = sandbox
        .gikli()
        .set_change_grouping(ChangeGrouping::Flat)
        .expect("show a flat list");
    let change_set = read_changes(&sandbox.gikli(), &repository);

    assert_eq!(settings.change_grouping, ChangeGrouping::Flat);
    assert_eq!(
        change_set.changelists[0].group.nodes,
        vec![
            file("README.md", ChangeStatus::Modified),
            file("docs/guide.md", ChangeStatus::Deleted),
            file("src/main.rs", ChangeStatus::Modified),
            file("src/ui/added.rs", ChangeStatus::Added),
        ]
    );
    assert_eq!(
        change_set.unversioned_files.nodes,
        vec![file("src/ui/view.rs", ChangeStatus::Untracked)]
    );
    assert_eq!(repository.status_by_git().len(), 5);
}

/// Ignored files are not changes, so they are in a group of their own, whose folders are not
/// highlighted.
#[test]
fn ignored_files_are_shown_on_their_own_in_folders_with_no_changes() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write(".gitignore", "*.log\n");
    repository.commit_all("Initial commit");
    repository.write("logs/app.log", "log\n");
    repository.write("logs/today/app.log", "log\n");
    repository.write("logs/today/summary.md", "summary\n");
    let gikli = sandbox.gikli();
    gikli
        .set_show_ignored_files(true)
        .expect("turn on ignored files");

    let change_set = read_changes(&gikli, &repository);

    assert_eq!(
        change_set.ignored_files.nodes,
        vec![folder(
            "logs",
            false,
            vec![
                folder(
                    "logs/today",
                    false,
                    vec![file("logs/today/app.log", ChangeStatus::Ignored)]
                ),
                file("logs/app.log", ChangeStatus::Ignored),
            ]
        )]
    );
    assert_eq!(
        change_set.unversioned_files.nodes,
        vec![folder(
            "logs",
            true,
            vec![folder(
                "logs/today",
                true,
                vec![file("logs/today/summary.md", ChangeStatus::Untracked)]
            )]
        )]
    );
    assert!(change_set.changelists[0].group.nodes.is_empty());
    assert_eq!(
        repository.git(&[
            "status",
            "--porcelain",
            "-z",
            "--ignored",
            "--untracked-files=all"
        ]),
        "?? logs/today/summary.md\0!! logs/app.log\0!! logs/today/app.log\0"
    );
}
