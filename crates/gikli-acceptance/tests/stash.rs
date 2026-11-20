//! Stash: the user puts work aside in a stash, sees what each stash holds, and drops one or clears
//! them all.

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    ChangeStatus, DiffContent, Failure, Gikli, Repository, StashDiffBase, StashOptions,
    StashUnavailable,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A repository whose one commit holds `notes.txt`.
fn with_notes(sandbox: &Sandbox) -> TestRepository {
    let work = sandbox.repository("work");
    work.write("notes.txt", "one\ntwo\n");
    work.commit_all("Add notes");
    work
}

/// The stashes of `repository` as plain git lists them, newest first: the subject of each.
fn stashes_by_git(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["stash", "list", "--format=%gs"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The options of a stash with `message` that neither keeps the index nor includes the files git
/// does not track.
fn options(message: &str) -> StashOptions {
    StashOptions {
        message: message.to_owned(),
        keep_index: false,
        include_untracked: false,
    }
}

#[test]
fn a_stash_with_a_message_puts_the_changes_aside() {
    let sandbox = Sandbox::new();
    let work = with_notes(&sandbox);
    work.write("notes.txt", "one\ntwo\nthree\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let stashes = gikli
        .stash(&root, &options("Half done"))
        .expect("stash the changes");

    assert_eq!(stashes.unavailable, None);
    assert_eq!(stashes.entries.len(), 1);
    assert_eq!(stashes.entries[0].name, "stash@{0}");
    assert_eq!(stashes.entries[0].message, "On main: Half done");
    assert_eq!(
        stashes.entries[0].hash,
        work.git(&["rev-parse", "stash@{0}"]).trim()
    );
    assert_eq!(stashes_by_git(&work), vec!["On main: Half done"]);
    assert_eq!(work.status_by_git(), Vec::<String>::new());
}

#[test]
fn keeping_the_index_leaves_what_is_staged_in_place() {
    let sandbox = Sandbox::new();
    let work = with_notes(&sandbox);
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.git(&["add", "notes.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .stash(
            &root,
            &StashOptions {
                keep_index: true,
                ..options("Staged too")
            },
        )
        .expect("stash the changes");

    assert_eq!(work.status_by_git(), vec!["M  notes.txt"]);
    assert_eq!(
        work.git(&["show", "stash@{0}:notes.txt"]),
        "one\ntwo\nthree\n"
    );
}

#[test]
fn files_git_does_not_track_go_into_the_stash_only_when_included() {
    let sandbox = Sandbox::new();
    let work = with_notes(&sandbox);
    work.write("notes.txt", "one\n");
    work.write("draft.txt", "new\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .stash(&root, &options("Tracked only"))
        .expect("stash the tracked changes");
    assert_eq!(work.status_by_git(), vec!["?? draft.txt"]);

    work.write("notes.txt", "one\n");
    gikli
        .stash(
            &root,
            &StashOptions {
                include_untracked: true,
                ..options("Everything")
            },
        )
        .expect("stash every change");
    assert_eq!(work.status_by_git(), Vec::<String>::new());
    assert_eq!(work.git(&["show", "stash@{0}^3:draft.txt"]), "new\n");
}

#[test]
fn a_stash_of_a_clean_tree_says_there_is_nothing_to_stash() {
    let sandbox = Sandbox::new();
    let work = with_notes(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let stashed = gikli.stash(&root, &options("Nothing"));

    assert_eq!(
        stashed,
        Err(Failure::NothingToStash {
            folder: root.root.clone()
        })
    );
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
}

#[test]
fn files_git_does_not_track_alone_are_nothing_to_stash_unless_included() {
    let sandbox = Sandbox::new();
    let work = with_notes(&sandbox);
    work.write("draft.txt", "new\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let stashed = gikli.stash(&root, &options("Untracked"));

    assert!(
        matches!(stashed, Err(Failure::NothingToStash { .. })),
        "{stashed:?}"
    );
    assert_eq!(work.status_by_git(), vec!["?? draft.txt"]);
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
}

#[test]
fn a_repository_with_no_commits_cannot_stash_and_says_why() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.write("notes.txt", "one\n");
    work.git(&["add", "notes.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let stashes = gikli.stashes(&root).expect("read the stashes");
    let stashed = gikli.stash(&root, &options("First"));

    assert_eq!(stashes.unavailable, Some(StashUnavailable::NoCommits));
    assert_eq!(stashes.entries, Vec::new());
    assert_eq!(
        stashed,
        Err(Failure::StashUnavailable {
            folder: root.root.clone(),
            reason: StashUnavailable::NoCommits,
        })
    );
    assert_eq!(work.status_by_git(), vec!["A  notes.txt"]);
}

/// The files of the stash `hash` in `repository` as gikli lists them, each with its status.
fn files_of(gikli: &Gikli, repository: &Repository, hash: &str) -> Vec<(String, ChangeStatus)> {
    gikli
        .stash_files(repository, hash)
        .expect("list the files of the stash")
        .changes
        .into_iter()
        .map(|change| (change.path.to_string_lossy().into_owned(), change.status))
        .collect()
}

#[test]
fn each_stash_lists_the_files_it_holds() {
    let sandbox = Sandbox::new();
    let work = with_notes(&sandbox);
    work.write("old.txt", "old\n");
    work.commit_all("Add old");
    work.write("notes.txt", "changed\n");
    work.delete("old.txt");
    work.write("staged.txt", "staged\n");
    work.git(&["add", "staged.txt"]);
    work.write("draft folder/draft é.txt", "draft\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let stashes = gikli
        .stash(
            &root,
            &StashOptions {
                include_untracked: true,
                ..options("Everything")
            },
        )
        .expect("stash every change");

    let files = files_of(&gikli, &root, &stashes.entries[0].hash);

    assert_eq!(
        files,
        vec![
            (
                "draft folder/draft é.txt".to_owned(),
                ChangeStatus::Untracked
            ),
            ("notes.txt".to_owned(), ChangeStatus::Modified),
            ("old.txt".to_owned(), ChangeStatus::Deleted),
            ("staged.txt".to_owned(), ChangeStatus::Added),
        ]
    );
    let mut by_git: Vec<String> = work
        .git(&[
            "stash",
            "show",
            "--include-untracked",
            "--name-only",
            "-z",
            "stash@{0}",
        ])
        .split_terminator('\0')
        .map(str::to_owned)
        .collect();
    by_git.sort();
    let listed: Vec<String> = files.into_iter().map(|(path, _)| path).collect();
    assert_eq!(listed, by_git);
}

/// The two sides of the diff of the file at `path` in the stash `hash`, older first, and what the
/// diff compared the stash with.
fn diff_of(
    gikli: &Gikli,
    repository: &Repository,
    hash: &str,
    path: &str,
) -> (Option<String>, Option<String>, StashDiffBase) {
    let diff = gikli
        .stash_file_diff(repository, hash, path)
        .expect("read the diff of the stashed file");
    let DiffContent::Text(text) = diff.content else {
        panic!("a text diff: {:?}", diff.content);
    };
    (text.head, text.local, diff.against)
}

/// A repository with a stash of a change to `notes.txt`, whose file on disk then changed again.
fn with_stashed_notes(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository, String) {
    let work = with_notes(sandbox);
    work.write("notes.txt", "one\ntwo\nthree\n");
    let root = opened(gikli, &work);
    let stashes = gikli
        .stash(&root, &options("Three"))
        .expect("stash the change");
    work.write("notes.txt", "one\nTWO\n");
    (work, root, stashes.entries[0].hash.clone())
}

#[test]
fn a_stashed_file_compares_with_the_local_version_by_default() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_stashed_notes(&sandbox, &gikli);

    let diff = diff_of(&gikli, &root, &hash, "notes.txt");

    assert_eq!(
        diff,
        (
            Some("one\ntwo\nthree\n".to_owned()),
            Some("one\nTWO\n".to_owned()),
            StashDiffBase::Local
        )
    );
    assert_eq!(
        work.git(&["show", "stash@{0}:notes.txt"]),
        "one\ntwo\nthree\n",
        "the older side is the stashed file, as git has it"
    );
}

#[test]
fn a_stashed_file_compares_with_the_parent_commit_when_the_user_chooses_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_stashed_notes(&sandbox, &gikli);

    let settings = gikli
        .set_stash_diff_against(StashDiffBase::Parent)
        .expect("compare stashes with their parent");
    let diff = diff_of(&gikli, &root, &hash, "notes.txt");

    assert_eq!(settings.stash_diff_against, StashDiffBase::Parent);
    assert_eq!(
        diff,
        (
            Some("one\ntwo\n".to_owned()),
            Some("one\ntwo\nthree\n".to_owned()),
            StashDiffBase::Parent
        )
    );
    assert_eq!(
        work.git(&["show", "stash@{0}^1:notes.txt"]),
        "one\ntwo\n",
        "the older side is the file in the commit the stash was made on, as git has it"
    );
}

#[test]
fn a_file_the_stash_moved_compares_with_its_old_path_in_the_parent_commit() {
    let sandbox = Sandbox::new();
    let work = with_notes(&sandbox);
    work.git(&["mv", "notes.txt", "kept notes.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let stashes = gikli
        .stash(&root, &options("Moved"))
        .expect("stash the move");
    let hash = &stashes.entries[0].hash;
    gikli
        .set_stash_diff_against(StashDiffBase::Parent)
        .expect("compare stashes with their parent");

    let files = gikli
        .stash_files(&root, hash)
        .expect("list the files of the stash")
        .changes;
    let diff = gikli
        .stash_file_diff(&root, hash, "kept notes.txt")
        .expect("read the diff of the moved file");

    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].status, ChangeStatus::Renamed);
    assert_eq!(
        files[0].original_path.as_deref(),
        Some("notes.txt".as_ref())
    );
    assert_eq!(diff.original_path.as_deref(), Some("notes.txt".as_ref()));
    let DiffContent::Text(text) = diff.content else {
        panic!("a text diff: {:?}", diff.content);
    };
    assert_eq!(text.head.as_deref(), Some("one\ntwo\n"));
    assert_eq!(text.local.as_deref(), Some("one\ntwo\n"));
    assert_eq!(text.differences, Vec::new());
    assert_eq!(
        work.git(&["show", "stash@{0}:kept notes.txt"]),
        "one\ntwo\n"
    );
}

#[test]
fn a_file_the_stash_deleted_is_missing_on_the_stash_side() {
    let sandbox = Sandbox::new();
    let work = with_notes(&sandbox);
    work.delete("notes.txt");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let stashes = gikli
        .stash(&root, &options("Deleted"))
        .expect("stash the deletion");
    let hash = &stashes.entries[0].hash;

    let against_local = diff_of(&gikli, &root, hash, "notes.txt");
    gikli
        .set_stash_diff_against(StashDiffBase::Parent)
        .expect("compare stashes with their parent");
    let against_parent = diff_of(&gikli, &root, hash, "notes.txt");

    assert_eq!(
        against_local,
        (None, Some("one\ntwo\n".to_owned()), StashDiffBase::Local),
        "the stash put the file back on disk"
    );
    assert_eq!(
        against_parent,
        (Some("one\ntwo\n".to_owned()), None, StashDiffBase::Parent)
    );
    assert!(!work.git_succeeds(&["cat-file", "-e", "stash@{0}:notes.txt"]));
}

#[test]
fn a_stashed_file_git_did_not_track_compares_with_nothing_in_the_parent_commit() {
    let sandbox = Sandbox::new();
    let work = with_notes(&sandbox);
    work.write("draft.txt", "new\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let stashes = gikli
        .stash(
            &root,
            &StashOptions {
                include_untracked: true,
                ..options("Draft")
            },
        )
        .expect("stash the new file");
    let hash = &stashes.entries[0].hash;

    let against_local = diff_of(&gikli, &root, hash, "draft.txt");
    gikli
        .set_stash_diff_against(StashDiffBase::Parent)
        .expect("compare stashes with their parent");
    let against_parent = diff_of(&gikli, &root, hash, "draft.txt");

    assert_eq!(
        against_local,
        (Some("new\n".to_owned()), None, StashDiffBase::Local),
        "the file left the working tree with the stash"
    );
    assert_eq!(
        against_parent,
        (None, Some("new\n".to_owned()), StashDiffBase::Parent)
    );
}

#[test]
fn a_stashed_file_the_same_as_on_disk_shows_no_difference() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_stashed_notes(&sandbox, &gikli);
    work.write("notes.txt", "one\ntwo\nthree\n");

    let diff = gikli
        .stash_file_diff(&root, &hash, "notes.txt")
        .expect("read the diff of the stashed file");

    let DiffContent::Text(text) = diff.content else {
        panic!("a text diff: {:?}", diff.content);
    };
    assert_eq!(text.differences, Vec::new());
}

#[test]
fn a_stash_that_is_gone_has_no_files_to_show() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_stashed_notes(&sandbox, &gikli);
    work.git(&["stash", "drop", "--quiet"]);

    let files = gikli.stash_files(&root, &hash);
    let diff = gikli.stash_file_diff(&root, &hash, "notes.txt");

    assert_eq!(files, Err(Failure::StashNotFound { hash: hash.clone() }));
    assert_eq!(diff, Err(Failure::StashNotFound { hash }));
}

/// A repository with two stashes, `Older` and then `Newer`, and the hashes of their commits,
/// newest first.
fn with_two_stashes(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository, Vec<String>) {
    let work = with_notes(sandbox);
    let root = opened(gikli, &work);
    work.write("notes.txt", "older\n");
    gikli
        .stash(&root, &options("Older"))
        .expect("stash the older change");
    work.write("notes.txt", "newer\n");
    let stashes = gikli
        .stash(&root, &options("Newer"))
        .expect("stash the newer change");
    let hashes = stashes
        .entries
        .iter()
        .map(|stash| stash.hash.clone())
        .collect();
    (work, root, hashes)
}

#[test]
fn dropping_a_stash_removes_that_one_alone() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hashes) = with_two_stashes(&sandbox, &gikli);

    let stashes = gikli
        .drop_stash(&root, &hashes[1])
        .expect("drop the older stash");

    let left: Vec<(&str, &str)> = stashes
        .entries
        .iter()
        .map(|stash| (stash.name.as_str(), stash.hash.as_str()))
        .collect();
    assert_eq!(left, vec![("stash@{0}", hashes[0].as_str())]);
    assert_eq!(stashes_by_git(&work), vec!["On main: Newer"]);
}

#[test]
fn a_stash_that_is_gone_is_not_dropped_again() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hashes) = with_two_stashes(&sandbox, &gikli);
    work.git(&["stash", "drop", "--quiet", "stash@{1}"]);

    let dropped = gikli.drop_stash(&root, &hashes[1]);

    assert_eq!(
        dropped,
        Err(Failure::StashNotFound {
            hash: hashes[1].clone()
        })
    );
    assert_eq!(stashes_by_git(&work), vec!["On main: Newer"]);
}

#[test]
fn clearing_the_stashes_removes_them_all() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hashes) = with_two_stashes(&sandbox, &gikli);

    let stashes = gikli
        .clear_stashes(&root, &hashes)
        .expect("clear the stashes");

    assert_eq!(stashes.entries, Vec::new());
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
}

#[test]
fn stashes_the_user_did_not_see_are_not_cleared() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hashes) = with_two_stashes(&sandbox, &gikli);
    work.write("notes.txt", "elsewhere\n");
    work.git(&["stash", "push", "--quiet", "--message", "Elsewhere"]);

    let cleared = gikli.clear_stashes(&root, &hashes);

    assert_eq!(
        cleared,
        Err(Failure::StashesChanged {
            folder: root.root.clone()
        })
    );
    assert_eq!(
        stashes_by_git(&work),
        vec!["On main: Elsewhere", "On main: Newer", "On main: Older"]
    );
}

#[test]
fn a_repository_with_no_commits_drops_and_clears_nothing() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let unavailable = Err(Failure::StashUnavailable {
        folder: root.root.clone(),
        reason: StashUnavailable::NoCommits,
    });

    assert_eq!(gikli.drop_stash(&root, "0123456789abcdef"), unavailable);
    assert_eq!(gikli.clear_stashes(&root, &[]), unavailable);
}
