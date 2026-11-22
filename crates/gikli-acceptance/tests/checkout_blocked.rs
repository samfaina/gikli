//! A checkout that local changes or files git does not track block: gikli lists the files, and
//! moves on over local changes by discarding them, once the user confirmed, or by saving them,
//! switching and restoring them.

use gikli_acceptance::{Sandbox, TestRepository, conflicts_left, opened, path_bufs, paths_in};
use gikli_core::{
    CheckoutTarget, CurrentBranch, Failure, Gikli, MoveTarget, NewBranchBase, OverLocalChanges,
    RefKind, RefLabel, Repository,
};

/// A repository whose `main` holds `notes.txt` and `todo.txt`, and whose branch `other` changes
/// `notes.txt` and adds `plan.txt`, opened as a Project on `main`.
fn with_other_branch(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository) {
    let work = sandbox.repository("work");
    work.write("notes.txt", "one\ntwo\n");
    work.write("todo.txt", "first\n");
    work.commit_all("Add notes and todo");
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.write("notes.txt", "zero\none\ntwo\n");
    work.write("plan.txt", "plan\n");
    work.commit_all("Start notes earlier and plan");
    work.git(&["switch", "--quiet", "main"]);
    let root = opened(gikli, &work);
    (work, root)
}

#[test]
fn a_checkout_local_changes_block_lists_the_files_that_block_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.write("todo.txt", "first\nsecond\n");

    let failure = gikli
        .check_out_branch(&root, "other")
        .expect_err("local changes to notes.txt block the checkout");

    assert_eq!(
        failure,
        Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: path_bufs(&["notes.txt"]),
            untracked_files: Vec::new(),
        },
        "todo.txt is the same on both branches, so it does not block"
    );
    assert_eq!(work.head_by_git(), "main");
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M todo.txt"]);
}

#[test]
fn a_checkout_untracked_files_block_lists_the_files_to_move_or_commit() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write("plan.txt", "my own plan\n");
    work.write("scratch.txt", "scratch\n");

    let failure = gikli
        .check_out_branch(&root, "other")
        .expect_err("the untracked plan.txt blocks the checkout");

    assert_eq!(
        failure,
        Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: Vec::new(),
            untracked_files: path_bufs(&["plan.txt"]),
        },
        "scratch.txt is in no branch, so it does not block"
    );
    assert_eq!(work.head_by_git(), "main");
    assert_eq!(
        std::fs::read_to_string(work.root().join("plan.txt")).expect("read plan"),
        "my own plan\n"
    );
}

/// The checkout of the branch `other`.
fn other_branch() -> CheckoutTarget {
    CheckoutTarget::Branch {
        name: "other".to_owned(),
    }
}

#[test]
fn discarding_the_confirmed_files_checks_out_and_keeps_the_other_changes() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write(
        "notes.txt",
        "one
two
three
",
    );
    work.write(
        "todo.txt",
        "first
second
",
    );

    let head = gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &other_branch(),
            &OverLocalChanges::Discard {
                files: path_bufs(&["notes.txt"]),
            },
        )
        .expect("discard notes.txt and check out");

    assert_eq!(
        head,
        CurrentBranch::Branch {
            name: "other".to_owned(),
            upstream: None,
        }
    );
    assert_eq!(
        work.read("notes.txt"),
        "zero
one
two
"
    );
    assert_eq!(
        work.read("todo.txt"),
        "first
second
",
        "not confirmed, so kept"
    );
    assert_eq!(work.status_by_git(), vec![" M todo.txt"]);
}

#[test]
fn discarding_files_other_than_those_that_block_now_discards_nothing_and_says_what_blocks() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.write("plan.txt", "my own plan\n");
    work.git(&["add", "plan.txt"]);

    let failure = gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &other_branch(),
            &OverLocalChanges::Discard {
                files: path_bufs(&["notes.txt"]),
            },
        )
        .expect_err("the staged plan.txt blocks too, and the user did not confirm it");

    assert_eq!(
        failure,
        Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: path_bufs(&["notes.txt", "plan.txt"]),
            untracked_files: Vec::new(),
        }
    );
    assert_eq!(work.head_by_git(), "main");
    assert_eq!(work.status_by_git(), vec![" M notes.txt", "A  plan.txt"]);
}

#[test]
fn discarding_a_new_file_that_blocks_removes_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write("plan.txt", "my own plan\n");
    work.git(&["add", "plan.txt"]);

    gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &other_branch(),
            &OverLocalChanges::Discard {
                files: path_bufs(&["plan.txt"]),
            },
        )
        .expect("discard plan.txt and check out");

    assert_eq!(work.head_by_git(), "other");
    assert_eq!(work.read("plan.txt"), "plan\n");
    assert_eq!(work.status_by_git(), Vec::<String>::new());
}

#[test]
fn saving_switching_and_restoring_brings_the_changes_onto_the_branch_checked_out() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.write("todo.txt", "first\nsecond\n");

    let head = gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &other_branch(),
            &OverLocalChanges::SaveAndRestore,
        )
        .expect("save, check out and restore");

    assert_eq!(
        head,
        CurrentBranch::Branch {
            name: "other".to_owned(),
            upstream: None,
        }
    );
    assert_eq!(work.read("notes.txt"), "zero\none\ntwo\nthree\n");
    assert_eq!(work.read("todo.txt"), "first\nsecond\n");
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M todo.txt"]);
    assert_eq!(work.stashes_by_git(), Vec::<String>::new());
}

#[test]
fn restoring_onto_a_conflict_keeps_the_saved_changes_and_opens_conflict_resolution() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write(
        "notes.txt",
        "uno
two
",
    );
    work.write(
        "todo.txt",
        "first
second
",
    );
    gikli
        .move_changes(
            &root,
            &["todo.txt".to_owned()],
            &MoveTarget::NewChangelist {
                name: "Feature".to_owned(),
            },
            false,
        )
        .expect("move todo.txt to Feature");
    let events = gikli.subscribe();

    gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &other_branch(),
            &OverLocalChanges::SaveAndRestore,
        )
        .expect("save, check out and restore onto a conflict");

    assert_eq!(work.head_by_git(), "other");
    assert_eq!(conflicts_left(&events), vec![root.clone()]);
    assert_eq!(
        work.stashes_by_git(),
        vec!["On main: Local changes saved by gikli"]
    );
    let changes = gikli.read_changes(&root).expect("read changes");
    let conflicted: Vec<_> = changes
        .conflicts
        .changes
        .iter()
        .map(|change| change.path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(conflicted, vec!["notes.txt"]);
    assert_eq!(paths_in(&changes, "Feature"), vec!["todo.txt"]);
}

#[test]
fn when_untracked_files_still_block_after_saving_the_changes_come_back_where_they_were() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write(
        "notes.txt",
        "one
two
three
",
    );
    work.write(
        "plan.txt",
        "my own plan
",
    );

    let failure = gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &other_branch(),
            &OverLocalChanges::SaveAndRestore,
        )
        .expect_err("plan.txt still blocks the checkout");

    assert_eq!(
        failure,
        Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: Vec::new(),
            untracked_files: path_bufs(&["plan.txt"]),
        }
    );
    assert_eq!(work.head_by_git(), "main");
    assert_eq!(work.status_by_git(), vec![" M notes.txt", "?? plan.txt"]);
    assert_eq!(
        work.read("notes.txt"),
        "one
two
three
"
    );
    assert_eq!(work.stashes_by_git(), Vec::<String>::new());
}

#[test]
fn a_tag_or_a_new_branch_that_local_changes_block_lists_them_and_checks_out_over_them() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.git(&["tag", "--annotate", "--message", "Release", "v1", "other"]);
    work.write(
        "notes.txt",
        "one
two
three
",
    );
    let blocked = Failure::CheckoutBlocked {
        folder: root.root.clone(),
        local_changes: path_bufs(&["notes.txt"]),
        untracked_files: Vec::new(),
    };
    let base = NewBranchBase::Branch {
        label: RefLabel {
            name: "other".to_owned(),
            kind: RefKind::Local,
        },
    };

    assert_eq!(gikli.check_out_tag(&root, "v1"), Err(blocked.clone()));
    assert_eq!(
        gikli.create_branch(&root, "topic", &base, true),
        Err(blocked)
    );
    assert!(
        !work.git_succeeds(&["rev-parse", "--verify", "--quiet", "refs/heads/topic"]),
        "the new branch is not created"
    );

    let head = gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &CheckoutTarget::NewBranch {
                name: "topic".to_owned(),
                base,
            },
            &OverLocalChanges::SaveAndRestore,
        )
        .expect("create topic over the saved changes");

    assert_eq!(
        head,
        CurrentBranch::Branch {
            name: "topic".to_owned(),
            upstream: None,
        }
    );
    assert_eq!(
        work.read("notes.txt"),
        "zero
one
two
three
"
    );
}

#[test]
fn untracked_files_in_a_folder_where_the_branch_has_a_file_block_too() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write("plan.txt/draft.txt", "draft\n");

    let failure = gikli
        .check_out_branch(&root, "other")
        .expect_err("plan.txt would replace the folder");

    assert_eq!(
        failure,
        Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: Vec::new(),
            untracked_files: path_bufs(&["plan.txt/draft.txt"]),
        }
    );
}

#[test]
fn discarding_while_untracked_files_block_discards_nothing() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.write("plan.txt", "my own plan\n");

    let failure = gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &other_branch(),
            &OverLocalChanges::Discard {
                files: path_bufs(&["notes.txt"]),
            },
        )
        .expect_err("plan.txt blocks whatever goes");

    assert_eq!(
        failure,
        Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: path_bufs(&["notes.txt"]),
            untracked_files: path_bufs(&["plan.txt"]),
        }
    );
    assert_eq!(work.read("notes.txt"), "one\ntwo\nthree\n", "not discarded");
}

#[test]
fn a_checkout_of_a_revision_that_local_changes_block_lists_them() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write("notes.txt", "one\ntwo\nthree\n");
    let hash = work.git(&["rev-parse", "other"]).trim().to_owned();

    assert_eq!(
        gikli.check_out_revision(&root, &hash),
        Err(Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: path_bufs(&["notes.txt"]),
            untracked_files: Vec::new(),
        })
    );
}

#[test]
fn a_file_taken_out_of_git_that_the_branch_deletes_blocks_as_untracked() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.git(&["switch", "--quiet", "other"]);
    work.git(&["rm", "--quiet", "todo.txt"]);
    work.commit_all("Drop todo");
    work.git(&["switch", "--quiet", "main"]);
    work.git(&["rm", "--quiet", "--cached", "todo.txt"]);

    assert_eq!(
        gikli.check_out_branch(&root, "other"),
        Err(Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: Vec::new(),
            untracked_files: path_bufs(&["todo.txt"]),
        })
    );
}

#[test]
fn untracked_files_block_a_checkout_in_a_repository_with_no_commits() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let work = sandbox.repository("work");
    work.git(&["switch", "--quiet", "--orphan", "other"]);
    work.write("plan.txt", "plan\n");
    work.commit_all("Plan");
    work.git(&["switch", "--quiet", "--orphan", "main"]);
    work.write("plan.txt", "my own plan\n");
    let root = opened(&gikli, &work);

    assert_eq!(
        gikli.check_out_branch(&root, "other"),
        Err(Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: Vec::new(),
            untracked_files: path_bufs(&["plan.txt"]),
        })
    );
}

/// Only where the file system ignores case, as git then does.
#[cfg(any(windows, target_os = "macos"))]
#[test]
fn an_untracked_file_whose_name_differs_only_in_case_blocks_where_case_is_ignored() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write("Plan.txt", "my own plan\n");

    assert_eq!(
        gikli.check_out_branch(&root, "other"),
        Err(Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: Vec::new(),
            untracked_files: path_bufs(&["Plan.txt"]),
        })
    );
}
