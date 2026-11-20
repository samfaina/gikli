//! Pull: integrating the work of a remote branch into the current branch, with the method the
//! user chooses, and remembering a source branch other than the default.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    Event, ExplainedMessage, Explanation, Failure, Gikli, NextStep, OperationAction,
    OperationInProgress, OperationKind, PullOptions, PullPlan, PullRemote, PullSource, Repository,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

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

#[test]
fn the_pull_dialog_starts_from_origin_and_the_upstream() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = shared(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli.pull_plan(&root).expect("plan the pull");

    assert_eq!(
        plan,
        PullPlan {
            repository: root.clone(),
            branch: "main".to_owned(),
            remotes: vec![PullRemote {
                name: "origin".to_owned(),
                branches: vec!["main".to_owned()],
            }],
            remote: Some("origin".to_owned()),
            source_branch: Some("main".to_owned()),
        }
    );
}

fn source(remote: &str, branch: &str) -> PullSource {
    PullSource {
        remote: remote.to_owned(),
        branch: branch.to_owned(),
    }
}

/// The subjects of the commits on HEAD of `repository`, newest first, as git lists them.
fn subjects(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["log", "--format=%s"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The parents of HEAD in `repository`, as git lists them.
fn parents_of_head(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["rev-list", "--parents", "--max-count=1", "HEAD"])
        .split_whitespace()
        .skip(1)
        .map(str::to_owned)
        .collect()
}

/// The commit `revision` names in `repository`.
fn commit_of(repository: &TestRepository, revision: &str) -> String {
    repository
        .git(&["rev-parse", revision])
        .trim_end()
        .to_owned()
}

/// Publishes a commit of `theirs` that changes `path` to `contents`, as someone else would.
fn publish(theirs: &TestRepository, path: &str, contents: &str, message: &str) {
    theirs.write(path, contents);
    theirs.commit_all(message);
    theirs.git(&["push", "--quiet", "origin", "main"]);
}

#[test]
fn pulling_brings_the_commits_others_published() {
    let sandbox = Sandbox::new();
    let Shared { mine, theirs, .. } = shared(&sandbox);
    publish(&theirs, "theirs.txt", "theirs\n", "Theirs");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let operation = gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions::default(),
        )
        .expect("pull");

    assert_eq!(operation, None, "nothing is left half done");
    assert_eq!(
        commit_of(&mine, "main"),
        commit_of(&theirs, "main"),
        "git agrees the branch fast-forwarded to what they published"
    );
}

#[test]
fn pulling_merges_a_branch_that_parted_from_the_remote_one() {
    let sandbox = Sandbox::new();
    let Shared { mine, theirs, .. } = shared(&sandbox);
    publish(&theirs, "theirs.txt", "theirs\n", "Theirs");
    mine.write("mine.txt", "mine\n");
    mine.commit_all("Mine");
    let mine_before = commit_of(&mine, "main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions::default(),
        )
        .expect("pull");

    assert_eq!(
        parents_of_head(&mine),
        [mine_before, commit_of(&theirs, "main")],
        "git made a merge of both"
    );
}

/// `mine` and the remote each have a commit of their own since they parted, on files of their own.
fn parted(sandbox: &Sandbox) -> Shared {
    let shared = shared(sandbox);
    publish(&shared.theirs, "theirs.txt", "theirs\n", "Theirs");
    shared.mine.write("mine.txt", "mine\n");
    shared.mine.commit_all("Mine");
    shared
}

#[test]
fn pulling_with_rebase_puts_the_branch_s_own_commits_on_top_of_the_remote_ones() {
    let sandbox = Sandbox::new();
    let Shared { mine, theirs, .. } = parted(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions {
                rebase: true,
                ..PullOptions::default()
            },
        )
        .expect("pull with rebase");

    assert_eq!(subjects(&mine), ["Mine", "Theirs", "First"]);
    assert_eq!(parents_of_head(&mine), [commit_of(&theirs, "main")]);
}

#[test]
fn pulling_merges_when_the_user_chose_to_even_if_git_is_configured_to_rebase() {
    let sandbox = Sandbox::new();
    let Shared { mine, theirs, .. } = parted(&sandbox);
    sandbox.configure("[pull]\n\trebase = true\n");
    let mine_before = commit_of(&mine, "main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions::default(),
        )
        .expect("pull");

    assert_eq!(
        parents_of_head(&mine),
        [mine_before, commit_of(&theirs, "main")]
    );
}

#[test]
fn pulling_fast_forward_only_leaves_a_branch_that_parted_as_it_was() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = parted(&sandbox);
    let mine_before = commit_of(&mine, "main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let pulled = gikli.pull(
        mine.root(),
        &root,
        &source("origin", "main"),
        PullOptions {
            ff_only: true,
            ..PullOptions::default()
        },
    );

    assert!(pulled.is_err(), "the branch cannot fast-forward");
    assert_eq!(commit_of(&mine, "main"), mine_before);
}

#[test]
fn pulling_without_fast_forward_makes_a_merge_commit_anyway() {
    let sandbox = Sandbox::new();
    let Shared { mine, theirs, .. } = shared(&sandbox);
    publish(&theirs, "theirs.txt", "theirs\n", "Theirs");
    let mine_before = commit_of(&mine, "main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions {
                no_ff: true,
                ..PullOptions::default()
            },
        )
        .expect("pull without fast-forward");

    assert_eq!(
        parents_of_head(&mine),
        [mine_before, commit_of(&theirs, "main")]
    );
}

#[test]
fn pulling_squashed_stages_what_the_remote_brings_without_a_commit() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = parted(&sandbox);
    let mine_before = commit_of(&mine, "main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let operation = gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions {
                squash: true,
                ..PullOptions::default()
            },
        )
        .expect("pull squashed");

    assert_eq!(operation, None, "a squash leaves no merge to finish");
    assert_eq!(commit_of(&mine, "main"), mine_before);
    assert_eq!(mine.status_by_git(), ["A  theirs.txt"]);
}

#[test]
fn pulling_without_a_commit_leaves_the_merge_in_progress() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = parted(&sandbox);
    let mine_before = commit_of(&mine, "main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let operation = gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions {
                no_commit: true,
                ..PullOptions::default()
            },
        )
        .expect("pull without a commit");

    assert_eq!(
        operation,
        Some(OperationInProgress {
            kind: OperationKind::Merge,
            actions: vec![OperationAction::CommitMerge, OperationAction::Abort],
        })
    );
    assert_eq!(commit_of(&mine, "main"), mine_before);
}

#[test]
fn pulling_without_verify_skips_the_hooks_of_the_merge() {
    let sandbox = Sandbox::new();
    let Shared { mine, theirs, .. } = parted(&sandbox);
    sandbox.hook("pre-merge-commit", "echo refused by the hook >&2\nexit 1");
    let mine_before = commit_of(&mine, "main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let verified = gikli.pull(
        mine.root(),
        &root,
        &source("origin", "main"),
        PullOptions::default(),
    );
    assert!(verified.is_err(), "the hook refuses the merge");
    mine.git(&["merge", "--abort"]);

    gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions {
                no_verify: true,
                ..PullOptions::default()
            },
        )
        .expect("pull without verify");

    assert_eq!(
        parents_of_head(&mine),
        [mine_before, commit_of(&theirs, "main")]
    );
}

#[test]
fn options_that_contradict_each_other_are_refused_before_git_runs() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = parted(&sandbox);
    let mine_before = commit_of(&mine, "main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let contradictions = [
        (
            PullOptions {
                rebase: true,
                squash: true,
                ..PullOptions::default()
            },
            ("--rebase", "--squash"),
        ),
        (
            PullOptions {
                rebase: true,
                no_commit: true,
                ..PullOptions::default()
            },
            ("--rebase", "--no-commit"),
        ),
        (
            PullOptions {
                ff_only: true,
                no_ff: true,
                ..PullOptions::default()
            },
            ("--ff-only", "--no-ff"),
        ),
        (
            PullOptions {
                no_ff: true,
                squash: true,
                ..PullOptions::default()
            },
            ("--no-ff", "--squash"),
        ),
    ];

    for (options, (first, second)) in contradictions {
        let pulled = gikli.pull(mine.root(), &root, &source("origin", "main"), options);

        assert_eq!(
            pulled,
            Err(Failure::PullOptionsIncompatible {
                first: first.to_owned(),
                second: second.to_owned(),
            })
        );
    }
    assert_eq!(commit_of(&mine, "main"), mine_before);
    assert_eq!(
        commit_of(&mine, "origin/main"),
        commit_of(&mine, "main~1"),
        "nothing was fetched either"
    );
}

/// `mine` and the remote each changed the one line of `file.txt` differently since they parted,
/// so integrating them stops on a conflict.
fn in_conflict(sandbox: &Sandbox) -> Shared {
    let shared = shared(sandbox);
    publish(&shared.theirs, "file.txt", "theirs\n", "Theirs");
    shared.mine.write("file.txt", "mine\n");
    shared.mine.commit_all("Mine");
    shared
}

/// The Repositories the `events` received say were left with files in conflict.
fn conflicts_left(events: &Receiver<Event>) -> Vec<Repository> {
    events
        .try_iter()
        .filter_map(|event| match event {
            Event::ConflictsLeft { repository } => Some(repository),
            _ => None,
        })
        .collect()
}

#[test]
fn a_merge_that_stops_on_a_conflict_leads_to_the_conflict_list() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = in_conflict(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let events = gikli.subscribe();

    let operation = gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions::default(),
        )
        .expect("a conflict is no failure");

    assert_eq!(conflicts_left(&events), std::slice::from_ref(&root));
    assert_eq!(
        operation.map(|operation| operation.kind),
        Some(OperationKind::Merge)
    );
    let conflicts = gikli
        .conflicts(mine.root(), &root)
        .expect("read the conflicts");
    assert_eq!(
        conflicts
            .iter()
            .map(|conflict| conflict.path.clone())
            .collect::<Vec<_>>(),
        [PathBuf::from("file.txt")]
    );
}

#[test]
fn a_rebase_that_stops_on_a_conflict_is_an_operation_in_progress() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = in_conflict(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let events = gikli.subscribe();

    let operation = gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions {
                rebase: true,
                ..PullOptions::default()
            },
        )
        .expect("a conflict is no failure");

    assert_eq!(
        operation,
        Some(OperationInProgress {
            kind: OperationKind::Rebase,
            actions: vec![
                OperationAction::Continue,
                OperationAction::Skip,
                OperationAction::Abort,
            ],
        })
    );
    assert_eq!(conflicts_left(&events), std::slice::from_ref(&root));
    assert_eq!(
        gikli
            .operation_in_progress(mine.root(), &root)
            .expect("read the operation in progress")
            .map(|operation| operation.kind),
        Some(OperationKind::Rebase),
        "the status area shows it too"
    );
}

#[test]
fn a_squash_that_stops_on_a_conflict_leads_to_the_conflict_list_with_nothing_in_progress() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = in_conflict(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let events = gikli.subscribe();

    let operation = gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions {
                squash: true,
                ..PullOptions::default()
            },
        )
        .expect("a conflict is no failure");

    assert_eq!(operation, None);
    assert_eq!(conflicts_left(&events), std::slice::from_ref(&root));
}

/// Publishes the branch `branch` of `theirs`, with a commit of its own.
fn publish_branch(theirs: &TestRepository, branch: &str) {
    theirs.git(&["switch", "--quiet", "--create", branch, "origin/main"]);
    theirs.write(&format!("{branch}.txt"), "theirs\n");
    theirs.commit_all(&format!("On {branch}"));
    theirs.git(&["push", "--quiet", "origin", branch]);
}

#[test]
fn a_source_branch_other_than_the_default_is_remembered_and_shown_next_time() {
    let sandbox = Sandbox::new();
    let Shared { mine, theirs, .. } = shared(&sandbox);
    publish_branch(&theirs, "feature");
    {
        let gikli = sandbox.gikli();
        let root = opened(&gikli, &mine);
        gikli
            .pull(
                mine.root(),
                &root,
                &source("origin", "feature"),
                PullOptions::default(),
            )
            .expect("pull feature");
    }
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli.pull_plan(&root).expect("plan the pull");

    assert_eq!(plan.remote.as_deref(), Some("origin"));
    assert_eq!(plan.source_branch.as_deref(), Some("feature"));
    assert_eq!(
        plan.remotes[0].branches,
        ["feature", "main"],
        "the pull fetched feature"
    );
}

#[test]
fn the_source_branch_is_remembered_for_the_branch_it_was_pulled_into() {
    let sandbox = Sandbox::new();
    let Shared { mine, theirs, .. } = shared(&sandbox);
    publish_branch(&theirs, "feature");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    gikli
        .pull(
            mine.root(),
            &root,
            &source("origin", "feature"),
            PullOptions::default(),
        )
        .expect("pull feature");
    mine.git(&[
        "switch",
        "--quiet",
        "--create",
        "other",
        "--track",
        "origin/main",
    ]);

    let plan = gikli.pull_plan(&root).expect("plan the pull");

    assert_eq!(plan.branch, "other");
    assert_eq!(plan.source_branch.as_deref(), Some("main"), "its upstream");
}

#[test]
fn pulling_from_the_default_again_lets_the_dialog_follow_the_upstream() {
    let sandbox = Sandbox::new();
    let Shared { mine, theirs, .. } = shared(&sandbox);
    publish_branch(&theirs, "feature");
    publish_branch(&theirs, "release");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    for branch in ["feature", "main"] {
        gikli
            .pull(
                mine.root(),
                &root,
                &source("origin", branch),
                PullOptions::default(),
            )
            .expect("pull");
    }
    mine.git(&["fetch", "--quiet", "origin"]);
    mine.git(&["branch", "--quiet", "--set-upstream-to", "origin/release"]);

    let plan = gikli.pull_plan(&root).expect("plan the pull");

    assert_eq!(plan.source_branch.as_deref(), Some("release"));
}

#[test]
fn a_branch_without_an_upstream_starts_from_its_namesake_on_origin() {
    let sandbox = Sandbox::new();
    let Shared { mine, theirs, .. } = shared(&sandbox);
    publish_branch(&theirs, "feature");
    mine.git(&["fetch", "--quiet", "origin"]);
    mine.git(&[
        "switch",
        "--quiet",
        "--no-track",
        "--create",
        "feature",
        "main",
    ]);
    let elsewhere = sandbox.bare_remote("elsewhere.git");
    mine.add_remote("another", &elsewhere);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli.pull_plan(&root).expect("plan the pull");

    assert_eq!(plan.branch, "feature");
    assert_eq!(
        plan.remotes
            .iter()
            .map(|remote| remote.name.as_str())
            .collect::<Vec<_>>(),
        ["another", "origin"]
    );
    assert_eq!(plan.remote.as_deref(), Some("origin"));
    assert_eq!(plan.source_branch.as_deref(), Some("feature"));
}

#[test]
fn a_branch_without_an_upstream_or_a_namesake_starts_from_origin_with_no_branch() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = shared(&sandbox);
    mine.git(&[
        "switch",
        "--quiet",
        "--no-track",
        "--create",
        "topic",
        "main",
    ]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli.pull_plan(&root).expect("plan the pull");

    assert_eq!(plan.remote.as_deref(), Some("origin"));
    assert_eq!(plan.source_branch, None);
}

#[test]
fn a_repository_without_a_remote_has_nothing_to_pull_from() {
    let sandbox = Sandbox::new();
    let mine = sandbox.repository("mine");
    mine.commit_at(1, "First");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli.pull_plan(&root).expect("plan the pull");

    assert_eq!(plan.remotes, []);
    assert_eq!(plan.remote, None);
    assert_eq!(plan.source_branch, None);
}

#[test]
fn a_detached_head_has_no_branch_to_pull_into() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = shared(&sandbox);
    mine.git(&["switch", "--quiet", "--detach"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    assert_eq!(gikli.pull_plan(&root), Err(Failure::DetachedHeadNotPulled));
    assert_eq!(
        gikli.pull(
            mine.root(),
            &root,
            &source("origin", "main"),
            PullOptions::default()
        ),
        Err(Failure::DetachedHeadNotPulled)
    );
}

#[test]
fn a_source_on_a_remote_the_repository_does_not_have_is_refused() {
    let sandbox = Sandbox::new();
    let Shared { mine, remote, .. } = shared(&sandbox);
    let path = remote.root().to_string_lossy().into_owned();
    let mine_before = commit_of(&mine, "main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let pulled = gikli.pull(
        mine.root(),
        &root,
        &source(&path, "main"),
        PullOptions::default(),
    );

    assert_eq!(pulled, Err(Failure::RemoteNotFound { remote: path }));
    assert_eq!(commit_of(&mine, "main"), mine_before);
}

#[test]
fn a_source_whose_name_is_no_branch_name_is_refused() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = shared(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    for name in ["@{-1}", "two words", ""] {
        assert_eq!(
            gikli.pull(
                mine.root(),
                &root,
                &source("origin", name),
                PullOptions::default()
            ),
            Err(Failure::BranchNameInvalid {
                name: name.to_owned()
            }),
            "{name:?}"
        );
    }
}

#[test]
fn pulling_while_files_are_in_conflict_is_refused() {
    let sandbox = Sandbox::new();
    let Shared { mine, .. } = in_conflict(&sandbox);
    assert!(!mine.git_succeeds(&["pull", "--no-rebase", "--quiet", "origin", "main"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let pulled = gikli.pull(
        mine.root(),
        &root,
        &source("origin", "main"),
        PullOptions::default(),
    );

    assert_eq!(pulled, Err(Failure::ConflictsNotResolved));
}

#[test]
fn a_pull_from_a_remote_that_holds_no_repository_any_more_is_explained() {
    let sandbox = Sandbox::new();
    let Shared { mine, remote, .. } = shared(&sandbox);
    let url = mine
        .git(&["remote", "get-url", "origin"])
        .trim_end()
        .to_owned();
    std::fs::remove_dir_all(remote.root()).expect("delete the remote");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let pulled = gikli.pull(
        mine.root(),
        &root,
        &source("origin", "main"),
        PullOptions::default(),
    );

    assert_eq!(
        pulled,
        Err(Failure::Explained {
            explanation: Explanation {
                message: ExplainedMessage::RemoteWithoutRepository {
                    remote: Some("origin".to_owned()),
                    url,
                },
                next_step: NextStep::CheckRemoteUrl,
                actions: vec![],
            },
        })
    );
}

#[test]
fn a_remembered_source_whose_remote_is_gone_gives_way_to_the_default() {
    let sandbox = Sandbox::new();
    let Shared { mine, remote, .. } = shared(&sandbox);
    mine.add_remote("another", &remote);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    gikli
        .pull(
            mine.root(),
            &root,
            &source("another", "main"),
            PullOptions::default(),
        )
        .expect("pull from another");
    mine.git(&["remote", "remove", "another"]);

    let plan = gikli.pull_plan(&root).expect("plan the pull");

    assert_eq!(plan.remote.as_deref(), Some("origin"));
    assert_eq!(plan.source_branch.as_deref(), Some("main"));
}
