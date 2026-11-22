//! Rejected push: a push of the current branch that its upstream rejects, as it has commits the
//! branch lacks, integrates them with the method the user chooses, or remembers, and pushes again.

use gikli_acceptance::{Sandbox, TestRepository, conflicts_left, opened};
use gikli_core::{Failure, OperationKind, PushTags, PushTarget, UpdateMethod};

/// A bare remote whose `main` holds First, a clone `mine` of it, and a clone `theirs` that pushes
/// to it what others publish.
struct Shared {
    remote: TestRepository,
    mine: TestRepository,
    theirs: TestRepository,
}

fn shared(sandbox: &Sandbox) -> Shared {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.repository("work");
    work.write("file.txt", "base\n");
    work.commit_all("First");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main"]);
    let mine = sandbox.clone_of(&remote, "mine");
    let theirs = sandbox.clone_of(&remote, "theirs");
    Shared {
        remote,
        mine,
        theirs,
    }
}

/// `mine` and the remote, through `theirs`, each with a commit of their own on `main`, which the
/// other lacks, in files of their own.
fn diverged(sandbox: &Sandbox) -> Shared {
    let shared = shared(sandbox);
    shared.theirs.write("theirs.txt", "theirs\n");
    shared.theirs.commit_all("Theirs");
    shared.theirs.git(&["push", "--quiet", "origin", "main"]);
    shared.mine.write("mine.txt", "mine\n");
    shared.mine.commit_all("Mine");
    shared
}

/// The upstream of `main`, as a push targets it: `main` of `origin`.
fn origin_main() -> PushTarget {
    PushTarget {
        remote: "origin".to_owned(),
        branch: "main".to_owned(),
    }
}

#[test]
fn a_rejected_push_offers_to_update_while_auto_update_is_off() {
    let sandbox = Sandbox::new();
    let Shared { remote, mine, .. } = diverged(&sandbox);
    let remote_before = remote.git(&["rev-parse", "main"]);
    let mine_before = mine.git(&["rev-parse", "main"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    assert!(
        !gikli.settings().auto_update_if_push_rejected,
        "auto-update is off by default"
    );

    let rejected = gikli.push(&root, &origin_main(), PushTags::None);

    assert_eq!(
        rejected,
        Err(Failure::PushRejected {
            folder: root.root.clone(),
            remote: "origin".to_owned(),
            branch: "main".to_owned(),
        }),
        "the remote has a commit the branch lacks"
    );
    assert_eq!(remote.git(&["rev-parse", "main"]), remote_before);
    assert_eq!(
        mine.git(&["rev-parse", "main"]),
        mine_before,
        "nothing was integrated before the user chose how"
    );
}

/// The subjects of the commits on `main` of `repository`, newest first, with the parents of each,
/// as git lists them.
fn history(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["log", "--format=%s%x09%p", "--topo-order", "main"])
        .lines()
        .map(|line| {
            let (subject, parents) = line.split_once('\t').unwrap_or((line, ""));
            let parents = parents.split_whitespace().count();
            format!("{subject} ({parents} parents)")
        })
        .collect()
}

#[test]
fn choosing_merge_merges_the_upstream_and_pushes_again() {
    let sandbox = Sandbox::new();
    let Shared { remote, mine, .. } = diverged(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let rejected = gikli.push(&root, &origin_main(), PushTags::None);
    assert!(matches!(rejected, Err(Failure::PushRejected { .. })));

    gikli
        .update_and_push(
            &root,
            &origin_main(),
            PushTags::None,
            UpdateMethod::Merge,
            false,
        )
        .expect("update and push");

    assert_eq!(
        remote.git(&["rev-parse", "main"]),
        mine.git(&["rev-parse", "main"]),
        "git agrees the remote has the branch"
    );
    let history = history(&mine);
    assert_eq!(
        history.len(),
        4,
        "a merge, Mine, Theirs and First: {history:?}"
    );
    assert!(
        history[0].ends_with("(2 parents)"),
        "the branch merged its upstream: {history:?}"
    );
    assert!(
        !gikli.settings().auto_update_if_push_rejected,
        "nothing asked to remember the choice"
    );
}

/// `theirs` publishes another commit on `main`, which `mine` lacks, and `mine` makes one more.
fn diverge_again(shared: &Shared, name: &str) {
    shared
        .theirs
        .git(&["pull", "--quiet", "--rebase", "origin", "main"]);
    shared
        .theirs
        .write(&format!("theirs-{name}.txt"), "theirs\n");
    shared.theirs.commit_all(&format!("Theirs {name}"));
    shared.theirs.git(&["push", "--quiet", "origin", "main"]);
    shared.mine.write(&format!("mine-{name}.txt"), "mine\n");
    shared.mine.commit_all(&format!("Mine {name}"));
}

#[test]
fn choosing_rebase_and_remembering_it_rebases_silently_from_then_on() {
    let sandbox = Sandbox::new();
    let shared = diverged(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &shared.mine);
    let rejected = gikli.push(&root, &origin_main(), PushTags::None);
    assert!(matches!(rejected, Err(Failure::PushRejected { .. })));

    gikli
        .update_and_push(
            &root,
            &origin_main(),
            PushTags::None,
            UpdateMethod::Rebase,
            true,
        )
        .expect("update and push");

    assert_eq!(
        history(&shared.mine),
        [
            "Mine (1 parents)",
            "Theirs (1 parents)",
            "First (0 parents)"
        ],
        "the branch was rebased onto its upstream"
    );
    assert_eq!(
        shared.remote.git(&["rev-parse", "main"]),
        shared.mine.git(&["rev-parse", "main"]),
        "git agrees the remote has the branch"
    );
    let settings = gikli.settings();
    assert!(
        settings.auto_update_if_push_rejected,
        "the choice turned it on"
    );
    assert_eq!(settings.push_rejected_update_method, UpdateMethod::Rebase);
    assert_eq!(
        sandbox.gikli().settings(),
        settings,
        "the settings were saved"
    );

    diverge_again(&shared, "again");
    gikli
        .push(&root, &origin_main(), PushTags::None)
        .expect("the push updates without asking");

    assert_eq!(
        history(&shared.mine)[..2],
        ["Mine again (1 parents)", "Theirs again (1 parents)"],
        "the branch was rebased again, as remembered"
    );
    assert_eq!(
        shared.remote.git(&["rev-parse", "main"]),
        shared.mine.git(&["rev-parse", "main"]),
        "git agrees the remote has the branch"
    );
}

#[test]
fn auto_update_turned_on_without_a_method_chosen_merges_silently() {
    let sandbox = Sandbox::new();
    let Shared { remote, mine, .. } = diverged(&sandbox);
    // The Repository has the incoming commits already, so git rejects the push as a
    // non-fast-forward rather than asking to fetch first.
    mine.git(&["fetch", "--quiet"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let settings = gikli
        .set_auto_update_if_push_rejected(true)
        .expect("turn auto-update on");
    assert!(settings.auto_update_if_push_rejected);

    gikli
        .push(&root, &origin_main(), PushTags::None)
        .expect("the push updates without asking");

    let history = history(&mine);
    assert!(
        history[0].ends_with("(2 parents)"),
        "the branch merged its upstream: {history:?}"
    );
    assert_eq!(
        remote.git(&["rev-parse", "main"]),
        mine.git(&["rev-parse", "main"]),
        "git agrees the remote has the branch"
    );
}

#[test]
fn an_update_that_stops_on_a_conflict_pushes_nothing_and_leads_to_the_conflict_list() {
    let sandbox = Sandbox::new();
    let Shared {
        remote,
        mine,
        theirs,
    } = shared(&sandbox);
    theirs.write("file.txt", "theirs\n");
    theirs.commit_all("Theirs");
    theirs.git(&["push", "--quiet", "origin", "main"]);
    mine.write("file.txt", "mine\n");
    mine.commit_all("Mine");
    let remote_before = remote.git(&["rev-parse", "main"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let events = gikli.subscribe();

    let stopped = gikli.update_and_push(
        &root,
        &origin_main(),
        PushTags::None,
        UpdateMethod::Merge,
        false,
    );

    assert_eq!(
        stopped,
        Err(Failure::UpdateStoppedOnConflicts {
            folder: root.root.clone()
        })
    );
    assert_eq!(conflicts_left(&events), std::slice::from_ref(&root));
    assert_eq!(
        gikli
            .operation_in_progress(mine.root(), &root)
            .expect("read the operation in progress")
            .map(|operation| operation.kind),
        Some(OperationKind::Merge),
        "the merge waits for the user to resolve it"
    );
    assert!(
        mine.git_succeeds(&["rev-parse", "--quiet", "--verify", "MERGE_HEAD"]),
        "git agrees a merge is in progress"
    );
    assert_eq!(
        mine.status_by_git(),
        ["UU file.txt"],
        "git agrees file.txt is in conflict"
    );
    assert_eq!(
        remote.git(&["rev-parse", "main"]),
        remote_before,
        "nothing was pushed"
    );
}

#[test]
fn a_rejected_push_to_a_branch_other_than_the_upstream_fails_without_updating() {
    let sandbox = Sandbox::new();
    let Shared { mine, theirs, .. } = shared(&sandbox);
    theirs.write("theirs.txt", "theirs\n");
    theirs.commit_all("Theirs");
    theirs.git(&["push", "--quiet", "origin", "main:release"]);
    mine.write("mine.txt", "mine\n");
    mine.commit_all("Mine");
    let mine_before = mine.git(&["rev-parse", "main"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    gikli
        .set_auto_update_if_push_rejected(true)
        .expect("turn auto-update on");
    let release = PushTarget {
        remote: "origin".to_owned(),
        branch: "release".to_owned(),
    };

    let rejected = gikli.push(&root, &release, PushTags::None);

    assert_eq!(
        rejected,
        Err(Failure::PushFailed {
            folder: root.root.clone()
        }),
        "release is not the upstream of main"
    );
    assert_eq!(mine.git(&["rev-parse", "main"]), mine_before);
}
