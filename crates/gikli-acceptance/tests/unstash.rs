//! Unstash: the user brings stashed work back, by applying a stash, popping it or creating a branch
//! from it, and a conflict never loses the stash.

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    Event, Failure, Gikli, Notification, NotificationAction, NotificationKind, Repository,
    StashOptions, StashUnavailable, UnstashOptions,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// The stashes of `repository` as plain git lists them, newest first: the subject of each.
fn stashes_by_git(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["stash", "list", "--format=%gs"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A repository whose one commit holds `notes.txt`, with a stash `Half done` that changed it to
/// `one\ntwo\nthree\n`, and the hash of that stash.
fn with_a_stash(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository, String) {
    let work = sandbox.repository("work");
    work.write("notes.txt", "one\ntwo\n");
    work.commit_all("Add notes");
    let root = opened(gikli, &work);
    work.write("notes.txt", "one\ntwo\nthree\n");
    let stashes = gikli
        .stash(
            &root,
            &StashOptions {
                message: "Half done".to_owned(),
                keep_index: false,
                include_untracked: false,
            },
        )
        .expect("stash the change");
    let hash = stashes.entries[0].hash.clone();
    (work, root, hash)
}

/// Applying, neither dropping the stash nor restoring the index.
const APPLY: UnstashOptions = UnstashOptions {
    pop: false,
    restore_index: false,
};

#[test]
fn applying_a_stash_brings_its_changes_back_and_keeps_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_stash(&sandbox, &gikli);

    let unstashed = gikli
        .apply_stash(work.root(), &root, &hash, APPLY)
        .expect("apply the stash");

    assert!(!unstashed.conflicts_left);
    assert_eq!(unstashed.stashes.entries[0].hash, hash);
    assert_eq!(stashes_by_git(&work), vec!["On main: Half done"]);
    assert_eq!(work.status_by_git(), vec![" M notes.txt"]);
    assert_eq!(
        std::fs::read_to_string(work.root().join("notes.txt")).expect("read notes"),
        "one\ntwo\nthree\n"
    );
}

/// Popping, without restoring the index.
const POP: UnstashOptions = UnstashOptions {
    pop: true,
    restore_index: false,
};

#[test]
fn popping_a_stash_brings_its_changes_back_and_drops_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_stash(&sandbox, &gikli);

    let unstashed = gikli
        .apply_stash(work.root(), &root, &hash, POP)
        .expect("pop the stash");

    assert!(!unstashed.conflicts_left);
    assert_eq!(unstashed.stashes.entries, Vec::new());
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
    assert_eq!(work.status_by_git(), vec![" M notes.txt"]);
}

/// A repository whose one commit holds `notes.txt`, with a stash `Staged` of a staged change to it
/// and a new staged file `added.txt`, and the hash of that stash.
fn with_a_staged_stash(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository, String) {
    let work = sandbox.repository("work");
    work.write("notes.txt", "one\ntwo\n");
    work.commit_all("Add notes");
    let root = opened(gikli, &work);
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.write("added.txt", "added\n");
    work.git(&["add", "notes.txt", "added.txt"]);
    let stashes = gikli
        .stash(
            &root,
            &StashOptions {
                message: "Staged".to_owned(),
                keep_index: false,
                include_untracked: false,
            },
        )
        .expect("stash the staged changes");
    let hash = stashes.entries[0].hash.clone();
    (work, root, hash)
}

#[test]
fn applying_without_restoring_the_index_leaves_the_changes_unstaged() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_staged_stash(&sandbox, &gikli);

    gikli
        .apply_stash(work.root(), &root, &hash, APPLY)
        .expect("apply the stash");

    // git stages a file the stash added, as it would be untracked otherwise.
    assert_eq!(work.status_by_git(), vec!["A  added.txt", " M notes.txt"]);
}

#[test]
fn applying_and_popping_can_restore_what_was_staged() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_staged_stash(&sandbox, &gikli);

    gikli
        .apply_stash(
            work.root(),
            &root,
            &hash,
            UnstashOptions {
                restore_index: true,
                ..APPLY
            },
        )
        .expect("apply the stash with its index");
    assert_eq!(work.status_by_git(), vec!["A  added.txt", "M  notes.txt"]);
    assert_eq!(stashes_by_git(&work), vec!["On main: Staged"]);

    work.git(&["reset", "--hard", "--quiet"]);
    gikli
        .apply_stash(
            work.root(),
            &root,
            &hash,
            UnstashOptions {
                restore_index: true,
                ..POP
            },
        )
        .expect("pop the stash with its index");
    assert_eq!(work.status_by_git(), vec!["A  added.txt", "M  notes.txt"]);
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
}

/// [`with_a_stash`], where a commit since changed the line the stash adds, so applying it stops on
/// a conflict in `notes.txt`.
fn with_a_conflicting_stash(
    sandbox: &Sandbox,
    gikli: &Gikli,
) -> (TestRepository, Repository, String) {
    let (work, root, hash) = with_a_stash(sandbox, gikli);
    work.write(
        "notes.txt",
        "one
two
THREE
",
    );
    work.commit_all("Change notes");
    (work, root, hash)
}

/// The Repositories the events received so far said an operation left in conflict.
fn conflicts_left(events: &std::sync::mpsc::Receiver<Event>) -> Vec<Repository> {
    events
        .try_iter()
        .filter_map(|event| match event {
            Event::ConflictsLeft { repository } => Some(repository),
            _ => None,
        })
        .collect()
}

/// The files of `repository` in conflict, as plain git lists them.
fn conflicted_by_git(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["diff", "--name-only", "--diff-filter=U"])
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_pop_that_conflicts_opens_conflict_resolution_and_keeps_the_stash() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_conflicting_stash(&sandbox, &gikli);
    let events = gikli.subscribe();

    let unstashed = gikli
        .apply_stash(work.root(), &root, &hash, POP)
        .expect("pop the stash onto a conflict");

    assert!(unstashed.conflicts_left);
    assert_eq!(unstashed.stashes.entries[0].hash, hash);
    assert_eq!(stashes_by_git(&work), vec!["On main: Half done"]);
    assert_eq!(conflicts_left(&events), vec![root.clone()]);
    assert_eq!(conflicted_by_git(&work), vec!["notes.txt"]);
}

/// The notifications raised about a stash kept in `repository` after it conflicted.
fn kept_stash_notifications(gikli: &Gikli, repository: &Repository) -> Vec<Notification> {
    gikli
        .notifications()
        .into_iter()
        .filter(|notification| {
            matches!(&notification.kind, NotificationKind::StashKept { repository: kept, .. } if kept == repository)
        })
        .collect()
}

#[test]
fn a_stash_kept_after_a_conflict_is_dropped_once_the_user_confirms_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_conflicting_stash(&sandbox, &gikli);
    gikli
        .apply_stash(work.root(), &root, &hash, POP)
        .expect("pop the stash onto a conflict");
    let kept = kept_stash_notifications(&gikli, &root);
    assert_eq!(kept.len(), 1);
    let NotificationKind::StashKept { stash, .. } = &kept[0].kind else {
        unreachable!("only kept stashes were kept");
    };
    assert_eq!(stash.hash, hash);
    assert_eq!(stash.message, "On main: Half done");
    assert_eq!(kept[0].actions, vec![NotificationAction::DropStash]);
    work.write("notes.txt", "one\ntwo\nthree\n");
    gikli
        .mark_resolved(work.root(), &root, &["notes.txt".to_owned()])
        .expect("mark the file resolved");
    assert_eq!(stashes_by_git(&work), vec!["On main: Half done"]);

    gikli
        .act_on_notification(kept[0].id, &NotificationAction::DropStash)
        .expect("drop the kept stash");

    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
    assert_eq!(kept_stash_notifications(&gikli, &root), Vec::new());
}

#[test]
fn applying_a_stash_that_conflicts_keeps_it_without_asking_to_drop_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_conflicting_stash(&sandbox, &gikli);
    let events = gikli.subscribe();

    let unstashed = gikli
        .apply_stash(work.root(), &root, &hash, APPLY)
        .expect("apply the stash onto a conflict");

    assert!(unstashed.conflicts_left);
    assert_eq!(conflicts_left(&events), vec![root.clone()]);
    assert_eq!(stashes_by_git(&work), vec!["On main: Half done"]);
    assert_eq!(kept_stash_notifications(&gikli, &root), Vec::new());
}

#[test]
fn a_clean_tree_brings_a_stash_back_without_a_warning() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_stash(&sandbox, &gikli);
    work.write("draft.txt", "not tracked\n");

    let plan = gikli.unstash_plan(&root, &hash).expect("plan the unstash");

    assert_eq!(plan.stash.hash, hash);
    assert_eq!(plan.stash.message, "On main: Half done");
    assert!(!plan.local_changes);
}

#[test]
fn applying_onto_uncommitted_changes_warns_and_does_not_block() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_stash(&sandbox, &gikli);
    work.write("other.txt", "other\n");
    work.commit_all("Add other");
    work.write("other.txt", "other, changed\n");

    let plan = gikli.unstash_plan(&root, &hash).expect("plan the unstash");
    let unstashed = gikli
        .apply_stash(work.root(), &root, &hash, POP)
        .expect("pop the stash onto the changed tree");

    assert!(plan.local_changes);
    assert!(!unstashed.conflicts_left);
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M other.txt"]);
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
}

#[test]
fn a_stash_that_is_gone_is_not_brought_back() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_stash(&sandbox, &gikli);
    work.git(&["stash", "drop", "--quiet"]);

    let applied = gikli.apply_stash(work.root(), &root, &hash, APPLY);

    assert_eq!(applied, Err(Failure::StashNotFound { hash: hash.clone() }));
    assert_eq!(
        gikli.unstash_plan(&root, &hash),
        Err(Failure::StashNotFound { hash })
    );
    assert_eq!(work.status_by_git(), Vec::<String>::new());
}

#[test]
fn a_pop_git_refuses_over_local_changes_to_the_same_file_keeps_the_stash() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_stash(&sandbox, &gikli);
    work.write(
        "notes.txt",
        "one
changed here
",
    );
    let events = gikli.subscribe();

    let popped = gikli.apply_stash(work.root(), &root, &hash, POP);

    assert_eq!(
        popped,
        Err(Failure::StashNotApplied {
            folder: root.root.clone()
        })
    );
    assert_eq!(conflicts_left(&events), vec![]);
    assert_eq!(stashes_by_git(&work), vec!["On main: Half done"]);
    assert_eq!(work.status_by_git(), vec![" M notes.txt"]);
}

#[test]
fn a_branch_from_a_stash_starts_where_it_was_made_with_its_changes_and_drops_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_staged_stash(&sandbox, &gikli);
    let made_on = work.git(&["rev-parse", "HEAD"]);
    work.write("notes.txt", "one\ntwo\nTHREE\n");
    work.commit_all("Change notes");

    let unstashed = gikli
        .stash_branch(work.root(), &root, &hash, "from-stash")
        .expect("create a branch from the stash");

    assert!(!unstashed.conflicts_left);
    assert_eq!(unstashed.stashes.entries, Vec::new());
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
    assert_eq!(work.git(&["branch", "--show-current"]), "from-stash\n");
    assert_eq!(work.git(&["rev-parse", "HEAD"]), made_on);
    assert_eq!(work.status_by_git(), vec!["A  added.txt", "M  notes.txt"]);
}

#[test]
fn a_branch_from_a_stash_takes_a_name_as_a_new_branch_does() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_stash(&sandbox, &gikli);

    let taken = gikli.stash_branch(work.root(), &root, &hash, "main");
    let invalid = gikli.stash_branch(work.root(), &root, &hash, "@{-1}");

    assert_eq!(
        taken,
        Err(Failure::BranchNameTaken {
            name: "main".to_owned()
        })
    );
    assert_eq!(
        invalid,
        Err(Failure::BranchNameInvalid {
            name: "@{-1}".to_owned()
        })
    );
    assert_eq!(stashes_by_git(&work), vec!["On main: Half done"]);
    assert_eq!(work.git(&["branch", "--show-current"]), "main\n");
}

#[test]
fn no_stash_is_brought_back_while_files_are_in_conflict() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_conflicting_stash(&sandbox, &gikli);
    gikli
        .apply_stash(work.root(), &root, &hash, APPLY)
        .expect("apply the stash onto a conflict");

    let applied = gikli.apply_stash(work.root(), &root, &hash, POP);
    let branched = gikli.stash_branch(work.root(), &root, &hash, "from-stash");

    assert_eq!(applied, Err(Failure::ConflictsNotResolved));
    assert_eq!(branched, Err(Failure::ConflictsNotResolved));
    assert_eq!(stashes_by_git(&work), vec!["On main: Half done"]);
    assert_eq!(work.git(&["branch", "--list", "from-stash"]), "");
}

#[test]
fn a_kept_stash_dropped_elsewhere_lets_its_notification_go() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_a_conflicting_stash(&sandbox, &gikli);
    gikli
        .apply_stash(work.root(), &root, &hash, POP)
        .expect("pop the stash onto a conflict");
    let kept = kept_stash_notifications(&gikli, &root);
    gikli
        .drop_stash(&root, &hash)
        .expect("drop the stash from the stashes tab");

    gikli
        .act_on_notification(kept[0].id, &NotificationAction::DropStash)
        .expect("the stash is gone already");

    assert_eq!(kept_stash_notifications(&gikli, &root), Vec::new());
}

#[test]
fn a_repository_with_no_commits_brings_no_stash_back() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let unavailable = Some(Failure::StashUnavailable {
        folder: root.root.clone(),
        reason: StashUnavailable::NoCommits,
    });

    assert_eq!(
        gikli.unstash_plan(&root, "0123456789abcdef").err(),
        unavailable
    );
    assert_eq!(
        gikli
            .apply_stash(work.root(), &root, "0123456789abcdef", POP)
            .err(),
        unavailable
    );
    assert_eq!(
        gikli
            .stash_branch(work.root(), &root, "0123456789abcdef", "from-stash")
            .err(),
        unavailable
    );
}

/// A repository whose one commit holds `notes.txt`, with a stash `With draft` of a change to it and
/// of `draft.txt`, which git did not track, and the hash of that stash.
fn with_an_untracked_stash(
    sandbox: &Sandbox,
    gikli: &Gikli,
) -> (TestRepository, Repository, String) {
    let work = sandbox.repository("work");
    work.write("notes.txt", "one\ntwo\n");
    work.commit_all("Add notes");
    let root = opened(gikli, &work);
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.write("draft.txt", "draft\n");
    let stashes = gikli
        .stash(
            &root,
            &StashOptions {
                message: "With draft".to_owned(),
                keep_index: false,
                include_untracked: true,
            },
        )
        .expect("stash every change");
    let hash = stashes.entries[0].hash.clone();
    (work, root, hash)
}

#[test]
fn popping_a_stash_brings_back_the_files_git_did_not_track() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_an_untracked_stash(&sandbox, &gikli);

    let plan = gikli.unstash_plan(&root, &hash).expect("plan the unstash");
    gikli
        .apply_stash(work.root(), &root, &hash, POP)
        .expect("pop the stash");

    assert_eq!(plan.untracked_in_the_way, Vec::<std::path::PathBuf>::new());
    assert_eq!(work.status_by_git(), vec![" M notes.txt", "?? draft.txt"]);
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
}

#[test]
fn a_file_the_stash_did_not_track_already_on_disk_is_warned_of_and_keeps_the_stash() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_an_untracked_stash(&sandbox, &gikli);
    work.write("draft.txt", "another draft\n");

    let plan = gikli.unstash_plan(&root, &hash).expect("plan the unstash");
    let popped = gikli.apply_stash(work.root(), &root, &hash, POP);

    assert_eq!(
        plan.untracked_in_the_way,
        vec![std::path::PathBuf::from("draft.txt")]
    );
    assert!(!plan.local_changes);
    assert_eq!(
        popped,
        Err(Failure::StashNotApplied {
            folder: root.root.clone()
        })
    );
    assert_eq!(stashes_by_git(&work), vec!["On main: With draft"]);
    assert_eq!(
        std::fs::read_to_string(work.root().join("draft.txt")).expect("read the draft"),
        "another draft\n"
    );
}

#[test]
fn a_branch_from_a_stash_git_cannot_apply_whole_stays_and_keeps_the_stash() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root, hash) = with_an_untracked_stash(&sandbox, &gikli);
    work.write("draft.txt", "another draft\n");

    let branched = gikli.stash_branch(work.root(), &root, &hash, "from-stash");

    assert_eq!(
        branched,
        Err(Failure::StashNotApplied {
            folder: root.root.clone()
        })
    );
    assert_eq!(work.git(&["branch", "--show-current"]), "from-stash\n");
    assert_eq!(stashes_by_git(&work), vec!["On main: With draft"]);
}
