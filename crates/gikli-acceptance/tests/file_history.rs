//! File history: the user traces how a file evolved and restores one of its versions.

use std::path::PathBuf;

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    ChangeGrouping, ChangeNode, ChangeStatus, DiffContent, Difference, Failure, FileHistory,
    FileRevision, Gikli, LineRange, LocalReadOnly, RefKind, RefLabel, Repository, TextDiff,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// The full hash of `revision`, as git names it.
fn hash_of(repository: &TestRepository, revision: &str) -> String {
    repository
        .git(&["rev-parse", revision])
        .trim_end()
        .to_owned()
}

/// What a revision of the history says of the file: its commit's subject, the path the file has in
/// it, the path it came from and how the commit changed it.
fn described(revision: &FileRevision) -> (&str, PathBuf, Option<PathBuf>, ChangeStatus) {
    (
        revision.subject.as_str(),
        revision.path.clone(),
        revision.original_path.clone(),
        revision.status,
    )
}

#[test]
fn file_history_lists_the_revisions_of_the_file_newest_first_following_renames() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.write("other.txt", "unrelated\n");
    repository.git_at(1, &["add", "--all"]);
    repository.git_at(1, &["commit", "--quiet", "--message", "Add notes"]);
    repository.write("other.txt", "unrelated, changed\n");
    repository.git_at(
        2,
        &["commit", "--quiet", "--all", "--message", "Change other"],
    );
    repository.write("notes.txt", "one\ntwo\n");
    repository.git_at(
        3,
        &["commit", "--quiet", "--all", "--message", "Extend notes"],
    );
    repository.git(&["mv", "notes.txt", "docs.txt"]);
    repository.git_at(4, &["commit", "--quiet", "--message", "Rename notes"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let history = gikli
        .file_history(&root, "docs.txt", &[])
        .expect("read the file history");

    assert_eq!(history.repository, root);
    assert_eq!(history.path, PathBuf::from("docs.txt"));
    let revisions: Vec<_> = history.revisions.iter().map(described).collect();
    assert_eq!(
        revisions,
        vec![
            (
                "Rename notes",
                PathBuf::from("docs.txt"),
                Some(PathBuf::from("notes.txt")),
                ChangeStatus::Renamed,
            ),
            (
                "Extend notes",
                PathBuf::from("notes.txt"),
                None,
                ChangeStatus::Modified,
            ),
            (
                "Add notes",
                PathBuf::from("notes.txt"),
                None,
                ChangeStatus::Added,
            ),
        ]
    );
    let hashes: Vec<String> = history.revisions.iter().map(|r| r.hash.clone()).collect();
    let by_git: Vec<String> = repository
        .git(&["log", "--follow", "--format=%H", "--", "docs.txt"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        hashes, by_git,
        "git follows the file through the same commits"
    );
    assert_eq!(history.revisions[0].hash, hash_of(&repository, "HEAD"));
    assert!(!history.more, "the history holds every revision");
}

/// The subjects of the revisions in `history`, newest first.
fn subjects(history: &FileHistory) -> Vec<&str> {
    history
        .revisions
        .iter()
        .map(|revision| revision.subject.as_str())
        .collect()
}

#[test]
fn file_history_reads_the_current_branch_or_the_branches_it_is_filtered_by() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.git_at(1, &["add", "--all"]);
    repository.git_at(1, &["commit", "--quiet", "--message", "Add notes"]);
    repository.git(&["switch", "--quiet", "--create", "feature"]);
    repository.write("notes.txt", "one\nfeature\n");
    repository.git_at(
        2,
        &[
            "commit",
            "--quiet",
            "--all",
            "--message",
            "Notes on feature",
        ],
    );
    repository.git(&["switch", "--quiet", "main"]);
    repository.write("notes.txt", "main\none\n");
    repository.git_at(
        3,
        &["commit", "--quiet", "--all", "--message", "Notes on main"],
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let feature = RefLabel {
        name: "feature".to_owned(),
        kind: RefKind::Local,
    };
    let main = RefLabel {
        name: "main".to_owned(),
        kind: RefKind::Local,
    };

    let current = gikli.file_history(&root, "notes.txt", &[]).expect("read");
    let on_feature = gikli
        .file_history(&root, "notes.txt", std::slice::from_ref(&feature))
        .expect("read on feature");
    let on_both = gikli
        .file_history(&root, "notes.txt", &[feature, main])
        .expect("read on both");

    assert_eq!(subjects(&current), vec!["Notes on main", "Add notes"]);
    assert_eq!(subjects(&on_feature), vec!["Notes on feature", "Add notes"]);
    assert_eq!(
        subjects(&on_both),
        vec!["Notes on main", "Notes on feature", "Add notes"]
    );
    assert_eq!(
        repository.git(&["log", "--format=%s", "feature", "--", "notes.txt"]),
        "Notes on feature\nAdd notes\n",
        "git agrees on what the feature branch holds of the file"
    );
}

#[test]
fn file_history_filtered_by_a_branch_that_is_gone_shows_nothing() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.commit_all("Add notes");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let gone = RefLabel {
        name: "gone".to_owned(),
        kind: RefKind::Local,
    };

    let history = gikli
        .file_history(&root, "notes.txt", &[gone])
        .expect("read the file history");

    assert!(history.revisions.is_empty());
    assert!(!repository.git_succeeds(&["rev-parse", "--verify", "--quiet", "refs/heads/gone"]));
}

#[test]
fn file_history_in_a_repository_with_no_commits_is_empty() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let history = gikli
        .file_history(&root, "notes.txt", &[])
        .expect("read the file history");

    assert!(history.revisions.is_empty());
    assert!(!repository.git_succeeds(&["rev-parse", "--verify", "--quiet", "HEAD"]));
}

#[test]
fn file_history_lists_at_most_the_row_limit_the_user_set_newest_first() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    for (minute, text) in [(1, "one\n"), (2, "two\n"), (3, "three\n")] {
        repository.write("notes.txt", text);
        repository.git_at(minute, &["add", "--all"]);
        repository.git_at(minute, &["commit", "--quiet", "--message", text.trim_end()]);
    }
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    assert_eq!(gikli.settings().file_history_row_limit, 1000);

    let settings = gikli
        .set_file_history_row_limit(2)
        .expect("set the row limit");
    let history = gikli.file_history(&root, "notes.txt", &[]).expect("read");

    assert_eq!(settings.file_history_row_limit, 2);
    assert_eq!(subjects(&history), vec!["three", "two"]);
    assert!(history.more, "the history says it left revisions out");
    assert_eq!(
        repository
            .git(&["log", "--format=%s", "--", "notes.txt"])
            .lines()
            .count(),
        3,
        "git has a third revision"
    );
    assert_eq!(
        sandbox.gikli().settings().file_history_row_limit,
        2,
        "the row limit is kept from one start of gikli to the next"
    );
}

/// The text diff in `content`, which must be one.
fn text_of(content: &DiffContent) -> &TextDiff {
    match content {
        DiffContent::Text(text) => text,
        other => panic!("expected a text diff, got {other:?}"),
    }
}

#[test]
fn a_revision_of_the_history_previews_what_it_changed_in_the_file() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.git_at(1, &["add", "--all"]);
    repository.git_at(1, &["commit", "--quiet", "--message", "Add notes"]);
    repository.write("notes.txt", "one\ntwo\n");
    repository.git_at(
        2,
        &["commit", "--quiet", "--all", "--message", "Extend notes"],
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let history = gikli.file_history(&root, "notes.txt", &[]).expect("read");
    let extended = &history.revisions[0];

    let diff = gikli
        .revision_file_diff(&root, &extended.hash, "notes.txt")
        .expect("read the diff of the revision");

    assert_eq!(diff.repository, root);
    assert_eq!(diff.revision, extended.hash);
    assert_eq!(diff.parent, Some(hash_of(&repository, "HEAD~1")));
    assert_eq!(diff.path, PathBuf::from("notes.txt"));
    assert_eq!(diff.original_path, None);
    let text = text_of(&diff.content);
    assert_eq!(text.head.as_deref(), Some("one\n"));
    assert_eq!(text.local.as_deref(), Some("one\ntwo\n"));
    assert_eq!(
        text.differences,
        vec![Difference {
            head: LineRange { start: 1, end: 1 },
            local: LineRange { start: 1, end: 2 },
        }]
    );
    assert_eq!(text.local_read_only, Some(LocalReadOnly::Revision));
    assert_eq!(
        repository.git(&["show", "HEAD~1:notes.txt"]),
        "one\n",
        "git has the older side in the parent"
    );
}

#[test]
fn the_preview_of_a_rename_compares_with_the_file_at_its_old_path() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\ntwo\nthree\nfour\n");
    repository.commit_all("Add notes");
    repository.git(&["mv", "notes.txt", "docs.txt"]);
    repository.write("docs.txt", "one\ntwo\nthree\nfour\nfive\n");
    repository.commit_all("Rename notes");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let history = gikli.file_history(&root, "docs.txt", &[]).expect("read");
    let renamed = &history.revisions[0];
    assert_eq!(renamed.status, ChangeStatus::Renamed);

    let diff = gikli
        .revision_file_diff(&root, &renamed.hash, "docs.txt")
        .expect("read the diff of the rename");

    assert_eq!(diff.original_path, Some(PathBuf::from("notes.txt")));
    let text = text_of(&diff.content);
    assert_eq!(text.head.as_deref(), Some("one\ntwo\nthree\nfour\n"));
    assert_eq!(text.local.as_deref(), Some("one\ntwo\nthree\nfour\nfive\n"));
    assert_eq!(
        text.differences,
        vec![Difference {
            head: LineRange { start: 4, end: 4 },
            local: LineRange { start: 4, end: 5 },
        }]
    );
}

#[test]
fn the_preview_of_the_revision_that_added_the_file_has_no_older_side() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\ntwo\n");
    repository.commit_all("Add notes");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let root_commit = hash_of(&repository, "HEAD");

    let diff = gikli
        .revision_file_diff(&root, &root_commit, "notes.txt")
        .expect("read the diff of the first commit");

    assert_eq!(diff.parent, None);
    let text = text_of(&diff.content);
    assert_eq!(text.head, None);
    assert_eq!(text.local.as_deref(), Some("one\ntwo\n"));
    assert_eq!(
        text.differences,
        vec![Difference {
            head: LineRange { start: 0, end: 0 },
            local: LineRange { start: 0, end: 2 },
        }]
    );
}

#[test]
fn the_preview_of_the_revision_that_deleted_the_file_has_no_newer_side() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.write("other.txt", "other\n");
    repository.commit_all("Add notes");
    repository.delete("notes.txt");
    repository.commit_all("Delete notes");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let history = gikli.file_history(&root, "notes.txt", &[]).expect("read");
    assert_eq!(history.revisions[0].status, ChangeStatus::Deleted);

    let diff = gikli
        .revision_file_diff(&root, &history.revisions[0].hash, "notes.txt")
        .expect("read the diff of the deletion");

    let text = text_of(&diff.content);
    assert_eq!(text.head.as_deref(), Some("one\n"));
    assert_eq!(text.local, None);
    assert_eq!(
        text.differences,
        vec![Difference {
            head: LineRange { start: 0, end: 1 },
            local: LineRange { start: 0, end: 0 },
        }]
    );
}

#[test]
fn a_revision_of_the_history_shows_every_file_its_commit_changed() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.write("other.txt", "other\n");
    repository.commit_all("Add notes");
    repository.write("notes.txt", "one\ntwo\n");
    repository.write("lib/code.txt", "code\n");
    repository.commit_all("Extend notes with code");
    let gikli = sandbox.gikli();
    gikli
        .set_commit_changes_grouping(ChangeGrouping::Flat)
        .expect("list the files flat");
    let root = opened(&gikli, &repository);
    let history = gikli.file_history(&root, "notes.txt", &[]).expect("read");

    let changes = gikli
        .commit_changes(&root, &history.revisions[0].hash)
        .expect("read the files of the revision");

    let paths: Vec<PathBuf> = changes[0]
        .group
        .nodes
        .iter()
        .map(|node| match node {
            ChangeNode::File(change) => change.path.clone(),
            other => panic!("expected a file, got {other:?}"),
        })
        .collect();
    assert_eq!(
        paths,
        vec![PathBuf::from("lib/code.txt"), PathBuf::from("notes.txt")]
    );
    assert_eq!(
        repository.git(&["diff-tree", "-r", "--name-only", "--no-commit-id", "HEAD"]),
        "lib/code.txt\nnotes.txt\n",
        "git agrees on the files the commit changed"
    );
}

#[test]
fn get_a_file_version_restores_the_file_as_it_was_at_the_revision_and_nothing_else() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.write("other.txt", "other\n");
    repository.git_at(1, &["add", "--all"]);
    repository.git_at(1, &["commit", "--quiet", "--message", "Add notes"]);
    repository.write("notes.txt", "one\ntwo\n");
    repository.git_at(
        2,
        &["commit", "--quiet", "--all", "--message", "Extend notes"],
    );
    repository.write("other.txt", "other, edited\n");
    repository.write("staged.txt", "staged\n");
    repository.git(&["add", "staged.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let history = gikli.file_history(&root, "notes.txt", &[]).expect("read");
    let added = &history.revisions[1];
    assert_eq!(added.subject, "Add notes");

    gikli
        .get_file_version(&root, &added.hash, "notes.txt", "notes.txt")
        .expect("get the version");

    assert_eq!(
        std::fs::read_to_string(repository.root().join("notes.txt")).expect("read notes"),
        repository.git(&["show", &format!("{}:notes.txt", added.hash)])
    );
    assert_eq!(
        repository.status_by_git(),
        vec![" M notes.txt", " M other.txt", "A  staged.txt"],
        "only the file changed on disk, and the index is as it was"
    );
    assert_eq!(
        std::fs::read_to_string(repository.root().join("other.txt")).expect("read other"),
        "other, edited\n"
    );
}

#[test]
fn get_a_file_version_from_before_a_rename_restores_it_at_its_path_now() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\ntwo\nthree\n");
    repository.commit_all("Add notes");
    repository.git(&["mv", "notes.txt", "docs.txt"]);
    repository.write("docs.txt", "one\ntwo\nthree\nfour\n");
    repository.commit_all("Rename notes");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let history = gikli.file_history(&root, "docs.txt", &[]).expect("read");
    let before_rename = &history.revisions[1];
    assert_eq!(before_rename.path, PathBuf::from("notes.txt"));

    gikli
        .get_file_version(&root, &before_rename.hash, "notes.txt", "docs.txt")
        .expect("get the version");

    assert_eq!(
        std::fs::read_to_string(repository.root().join("docs.txt")).expect("read docs"),
        "one\ntwo\nthree\n"
    );
    assert!(!repository.root().join("notes.txt").exists());
    assert_eq!(repository.status_by_git(), vec![" M docs.txt"]);
}

#[test]
fn get_a_file_version_of_the_revision_that_deleted_the_file_deletes_it() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.write("other.txt", "other\n");
    repository.commit_all("Add notes");
    repository.delete("notes.txt");
    repository.commit_all("Delete notes");
    repository.write("notes.txt", "again\n");
    repository.commit_all("Add notes again");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let deleted = hash_of(&repository, "HEAD~1");

    gikli
        .get_file_version(&root, &deleted, "notes.txt", "notes.txt")
        .expect("get the version");

    assert!(!repository.root().join("notes.txt").exists());
    assert_eq!(repository.status_by_git(), vec![" D notes.txt"]);
}

#[test]
fn get_a_file_version_of_a_revision_that_names_no_commit_changes_nothing() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "one\n");
    repository.commit_all("Add notes");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.get_file_version(&root, "no-such-commit", "notes.txt", "notes.txt");

    assert_eq!(
        result,
        Err(Failure::RevisionNotFound {
            revision: "no-such-commit".to_owned()
        })
    );
    assert!(repository.status_by_git().is_empty());
}

#[test]
fn file_history_and_get_a_file_version_work_with_unicode_and_spaces_in_paths() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notas viejas/año 1.txt", "uno\n");
    repository.commit_all("Add notes");
    repository.delete("notas viejas/año 1.txt");
    repository.write("notas/[año] 2.txt", "uno\ndos\n");
    repository.commit_all("Move notes");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let history = gikli
        .file_history(&root, "notas/[año] 2.txt", &[])
        .expect("read the file history");

    let paths: Vec<PathBuf> = history.revisions.iter().map(|r| r.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("notas/[año] 2.txt"),
            PathBuf::from("notas viejas/año 1.txt"),
        ]
    );
    gikli
        .get_file_version(
            &root,
            &history.revisions[1].hash,
            "notas viejas/año 1.txt",
            "notas/[año] 2.txt",
        )
        .expect("get the version");
    assert_eq!(
        std::fs::read_to_string(repository.root().join("notas/[año] 2.txt")).expect("read"),
        "uno\n"
    );
    assert_eq!(repository.status_by_git(), vec![" M notas/[año] 2.txt"]);
}
