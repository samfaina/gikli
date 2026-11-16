//! Fetch from the remotes, and how far each branch is from its upstream, which the status area
//! shows for the current branch.

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    CurrentBranch, Event, ExplainedMessage, Explanation, Failure, Gikli, IncomingOutgoing,
    LocalBranch, LongOperation, NextStep, Progress, RefKind, RefLabel, Repository, Upstream,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A bare remote whose `main` holds First, pushed from the returned `work`, where others commit.
fn remote_with_work(sandbox: &Sandbox) -> (TestRepository, TestRepository) {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "--set-upstream", "origin", "main"]);
    (remote, work)
}

/// The upstream `name` of a branch, with `incoming` commits the branch does not have and
/// `outgoing` ones it does not have.
fn upstream(name: &str, incoming: usize, outgoing: usize) -> Option<Upstream> {
    Some(Upstream {
        name: name.to_owned(),
        commits: Some(IncomingOutgoing { incoming, outgoing }),
    })
}

/// The incoming and outgoing commits of `branch` against `upstream`, as plain git counts them.
fn counted_by_git(repository: &TestRepository, branch: &str, upstream: &str) -> (usize, usize) {
    let range = format!("{branch}...{upstream}");
    let counted = repository.git(&["rev-list", "--left-right", "--count", &range]);
    let mut numbers = counted
        .split_whitespace()
        .map(|number| number.parse().expect("a count"));
    let outgoing = numbers.next().expect("outgoing");
    let incoming = numbers.next().expect("incoming");
    (incoming, outgoing)
}

/// The remote branches of `repository` and where they point, as plain git lists them.
fn remote_branches_by_git(repository: &TestRepository) -> String {
    repository.git(&[
        "for-each-ref",
        "--format=%(refname) %(objectname)",
        "refs/remotes/",
    ])
}

#[test]
fn fetching_every_remote_brings_what_others_pushed_as_incoming_commits() {
    let sandbox = Sandbox::new();
    let (remote, work) = remote_with_work(&sandbox);
    let mine = sandbox.clone_of(&remote, "mine");
    work.commit_at(2, "Second");
    work.commit_at(3, "Third");
    work.git(&["push", "--quiet"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli.fetch_all(&root).expect("fetch every remote");

    assert_eq!(
        gikli.current_branch(&root),
        Ok(CurrentBranch::Branch {
            name: "main".to_owned(),
            upstream: upstream("origin/main", 2, 0),
        })
    );
    assert_eq!(
        mine.git(&["rev-parse", "origin/main"]),
        work.git(&["rev-parse", "HEAD"]),
        "git agrees the remote branch is where the remote has it"
    );
}

#[test]
fn fetching_every_remote_prunes_a_remote_branch_deleted_there_and_its_local_branch_sees_it_gone() {
    let sandbox = Sandbox::new();
    let (remote, work) = remote_with_work(&sandbox);
    work.git(&["push", "--quiet", "origin", "main:side"]);
    let mine = sandbox.clone_of(&remote, "mine");
    mine.git(&["branch", "--quiet", "--track", "side", "origin/side"]);
    work.git(&["push", "--quiet", "origin", "--delete", "side"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli.fetch_all(&root).expect("fetch every remote");

    let branches = gikli.local_branches(&root).expect("read the branches");
    assert_eq!(
        branches,
        [
            LocalBranch {
                name: "main".to_owned(),
                upstream: upstream("origin/main", 0, 0),
            },
            LocalBranch {
                name: "side".to_owned(),
                upstream: Some(Upstream {
                    name: "origin/side".to_owned(),
                    commits: None,
                }),
            },
        ]
    );
    assert!(
        !mine.git_succeeds(&[
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/remotes/origin/side"
        ]),
        "git agrees the remote branch is gone"
    );
}

#[test]
fn fetching_every_remote_fetches_each_of_them() {
    let sandbox = Sandbox::new();
    let (remote, work) = remote_with_work(&sandbox);
    let other = sandbox.bare_remote("other.git");
    work.add_remote("other", &other);
    work.git(&["push", "--quiet", "other", "main"]);
    let mine = sandbox.clone_of(&remote, "mine");
    mine.add_remote("other", &other);
    work.commit_at(2, "Second");
    work.git(&["push", "--quiet", "other", "main"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli.fetch_all(&root).expect("fetch every remote");

    assert_eq!(
        mine.git(&["rev-parse", "other/main"]),
        work.git(&["rev-parse", "HEAD"]),
        "git agrees the other remote was fetched too"
    );
    let labels = gikli.log_branches(&root).expect("read the branches");
    assert!(
        labels.iter().any(|label| label.name == "other/main"),
        "{labels:?}"
    );
}

#[test]
fn a_branch_with_commits_of_its_own_and_of_its_upstream_counts_both() {
    let sandbox = Sandbox::new();
    let (remote, work) = remote_with_work(&sandbox);
    let mine = sandbox.clone_of(&remote, "mine");
    work.commit_at(2, "Theirs");
    work.git(&["push", "--quiet"]);
    mine.commit_at(3, "Mine");
    mine.commit_at(4, "Mine again");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let before = gikli
        .current_branch(&root)
        .expect("read the current branch");
    gikli.fetch_all(&root).expect("fetch every remote");
    let after = gikli
        .current_branch(&root)
        .expect("read the current branch");

    let on_main = |upstream| CurrentBranch::Branch {
        name: "main".to_owned(),
        upstream,
    };
    assert_eq!(before, on_main(upstream("origin/main", 0, 2)));
    assert_eq!(after, on_main(upstream("origin/main", 1, 2)));
    assert_eq!(
        mine.git(&["rev-list", "--left-right", "--count", "main...origin/main"]),
        "2\t1\n",
        "git counts the same"
    );
}

/// A clone `mine` of a remote with the branches `main` and `side`, each of which got a commit on
/// the remote after the clone, pushed from the returned `work`.
fn both_branches_moved_on(sandbox: &Sandbox) -> (TestRepository, TestRepository) {
    let (remote, work) = remote_with_work(sandbox);
    work.git(&["push", "--quiet", "origin", "main:side"]);
    let mine = sandbox.clone_of(&remote, "mine");
    mine.git(&["branch", "--quiet", "--track", "side", "origin/side"]);
    work.commit_at(2, "On main");
    work.git(&["push", "--quiet", "origin", "main"]);
    work.git(&[
        "switch",
        "--quiet",
        "--create",
        "side",
        "--track",
        "origin/side",
    ]);
    work.commit_at(3, "On side");
    work.git(&["push", "--quiet", "origin", "side"]);
    (mine, work)
}

fn label(name: &str, kind: RefKind) -> RefLabel {
    RefLabel {
        name: name.to_owned(),
        kind,
    }
}

#[test]
fn fetching_a_local_branch_fetches_only_its_upstream() {
    let sandbox = Sandbox::new();
    let (mine, work) = both_branches_moved_on(&sandbox);
    let before_side = mine.git(&["rev-parse", "origin/side"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .fetch_branch(&root, &label("main", RefKind::Local))
        .expect("fetch the branch");

    assert_eq!(
        mine.git(&["rev-parse", "origin/main"]),
        work.git(&["rev-parse", "main"]),
        "git agrees the upstream was fetched"
    );
    assert_eq!(
        mine.git(&["rev-parse", "origin/side"]),
        before_side,
        "git agrees no other branch was fetched"
    );
    let branches = gikli.local_branches(&root).expect("read the branches");
    assert_eq!(branches[0].upstream, upstream("origin/main", 1, 0));
    assert_eq!(branches[1].upstream, upstream("origin/side", 0, 0));
}

#[test]
fn fetching_a_remote_branch_fetches_only_it() {
    let sandbox = Sandbox::new();
    let (mine, work) = both_branches_moved_on(&sandbox);
    let before_main = mine.git(&["rev-parse", "origin/main"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .fetch_branch(&root, &label("origin/side", RefKind::Remote))
        .expect("fetch the branch");

    assert_eq!(
        mine.git(&["rev-parse", "origin/side"]),
        work.git(&["rev-parse", "side"]),
        "git agrees the remote branch was fetched"
    );
    assert_eq!(mine.git(&["rev-parse", "origin/main"]), before_main);
    let branches = gikli.local_branches(&root).expect("read the branches");
    assert_eq!(branches[1].upstream, upstream("origin/side", 1, 0));
}

#[test]
fn fetching_a_branch_runs_as_a_long_operation_from_its_remote() {
    let sandbox = Sandbox::new();
    let (mine, work) = both_branches_moved_on(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let events = gikli.subscribe();

    gikli
        .fetch_branch(&root, &label("side", RefKind::Local))
        .expect("fetch the branch");

    let received: Vec<Event> = events.try_iter().collect();
    let (id, operation) = started(&received).expect("the fetch announces it started");
    assert_eq!(
        operation,
        LongOperation::FetchBranch {
            repository: root.clone(),
            remote: "origin".to_owned(),
            branch: "side".to_owned(),
        }
    );
    assert!(received.contains(&Event::OperationFinished { id }));
    assert_eq!(
        mine.git(&["rev-parse", "origin/side"]),
        work.git(&["rev-parse", "side"]),
        "git agrees the branch was fetched"
    );
}

/// The first long operation `received` announces as started, with its id.
fn started(received: &[Event]) -> Option<(u64, LongOperation)> {
    received.iter().find_map(|event| match event {
        Event::OperationStarted { id, operation, .. } => Some((*id, operation.clone())),
        _ => None,
    })
}

#[test]
fn a_local_branch_without_an_upstream_has_nothing_to_fetch() {
    let sandbox = Sandbox::new();
    let (mine, _work) = both_branches_moved_on(&sandbox);
    mine.git(&["branch", "--quiet", "alone"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let before = remote_branches_by_git(&mine);

    let fetched = gikli.fetch_branch(&root, &label("alone", RefKind::Local));

    assert_eq!(
        fetched,
        Err(Failure::NoUpstream {
            branch: "alone".to_owned()
        })
    );
    let branches = gikli.local_branches(&root).expect("read the branches");
    assert_eq!(branches[0].name, "alone");
    assert_eq!(branches[0].upstream, None);
    assert_eq!(
        remote_branches_by_git(&mine),
        before,
        "git agrees nothing was fetched"
    );
    assert!(!mine.git_succeeds(&["rev-parse", "--verify", "--quiet", "alone@{upstream}"]));
}

#[test]
fn fetching_every_remote_runs_as_a_long_operation_that_reports_its_progress() {
    let sandbox = Sandbox::new();
    let (remote, work) = remote_with_work(&sandbox);
    let mine = sandbox.clone_of(&remote, "mine");
    for index in 0..3 {
        work.write(&format!("file-{index}.txt"), &format!("content {index}\n"));
        work.commit_all(&format!("Commit {index}"));
    }
    work.git(&["push", "--quiet"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let events = gikli.subscribe();

    gikli.fetch_all(&root).expect("fetch every remote");

    let received: Vec<Event> = events.try_iter().collect();
    let (id, operation) = started(&received).expect("the fetch announces it started");
    assert_eq!(
        operation,
        LongOperation::FetchAll {
            repository: root.clone()
        }
    );
    let reported: Vec<&Progress> = received
        .iter()
        .filter_map(|event| match event {
            Event::OperationProgress {
                id: reporter,
                progress,
            } if *reporter == id => Some(progress),
            _ => None,
        })
        .collect();
    assert!(
        reported.iter().any(|progress| progress.percent == 100),
        "the fetch reports progress up to the end: {reported:?}"
    );
    assert!(received.contains(&Event::OperationFinished { id }));
    assert_eq!(
        mine.git(&["rev-parse", "origin/main"]),
        work.git(&["rev-parse", "HEAD"]),
        "git agrees the remote was fetched"
    );
}

#[test]
fn a_repository_without_a_remote_has_nowhere_to_fetch_from() {
    let sandbox = Sandbox::new();
    let alone = sandbox.repository("alone");
    alone.commit_at(1, "First");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &alone);

    assert_eq!(
        gikli.fetch_all(&root),
        Err(Failure::NoRemoteToFetchFrom {
            folder: root.root.clone()
        })
    );
    assert_eq!(
        gikli.current_branch(&root),
        Ok(CurrentBranch::Branch {
            name: "main".to_owned(),
            upstream: None,
        })
    );
    assert_eq!(alone.git(&["remote"]), "", "git agrees there is no remote");
}

/// What the failure explainer says of a server git could not reach.
fn unreachable_server() -> Explanation {
    Explanation {
        message: ExplainedMessage::ServerUnreachable,
        next_step: NextStep::CheckConnection,
        actions: vec![],
    }
}

#[test]
fn a_fetch_that_cannot_reach_the_server_is_explained() {
    let sandbox = Sandbox::new();
    let (remote, _work) = remote_with_work(&sandbox);
    let mine = sandbox.clone_of(&remote, "mine");
    mine.git(&["remote", "set-url", "origin", "git@gikli.test:remote.git"]);
    sandbox.ssh_stand_in(
        "echo \"ssh: Could not resolve hostname gikli.test: Name or service not known\" >&2\nexit 255",
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let before = remote_branches_by_git(&mine);

    let every_remote = gikli.fetch_all(&root);
    let one_branch = gikli.fetch_branch(&root, &label("main", RefKind::Local));

    for fetched in [every_remote, one_branch] {
        assert_eq!(
            fetched,
            Err(Failure::Explained {
                explanation: unreachable_server()
            })
        );
    }
    let log = gikli.operation_log();
    assert!(
        log.iter()
            .any(|entry| format!("{entry:?}").contains("Could not resolve hostname")),
        "the raw output goes to the operation log"
    );
    assert_eq!(
        remote_branches_by_git(&mine),
        before,
        "git agrees nothing was fetched"
    );
}

#[test]
fn a_fetch_git_cannot_explain_says_it_failed() {
    let sandbox = Sandbox::new();
    let (remote, _work) = remote_with_work(&sandbox);
    let mine = sandbox.clone_of(&remote, "mine");
    let server = sandbox.http_remote_answering(&remote, "500 Internal Server Error");
    mine.git(&["remote", "set-url", "origin", &server.url()]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let before = remote_branches_by_git(&mine);

    assert_eq!(
        gikli.fetch_all(&root),
        Err(Failure::FetchFailed {
            folder: root.root.clone()
        })
    );
    assert_eq!(
        gikli.current_branch(&root),
        Ok(CurrentBranch::Branch {
            name: "main".to_owned(),
            upstream: upstream("origin/main", 0, 0),
        })
    );
    assert_eq!(
        remote_branches_by_git(&mine),
        before,
        "git agrees nothing was fetched"
    );
}

#[test]
fn on_a_detached_head_the_status_area_shows_the_commit() {
    let sandbox = Sandbox::new();
    let (remote, _work) = remote_with_work(&sandbox);
    let mine = sandbox.clone_of(&remote, "mine");
    mine.commit_at(2, "Second");
    mine.git(&["switch", "--quiet", "--detach", "HEAD~1"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    assert_eq!(
        gikli.current_branch(&root),
        Ok(CurrentBranch::Detached {
            hash: mine.git(&["rev-parse", "HEAD"]).trim_end().to_owned(),
        })
    );
    gikli.fetch_all(&root).expect("fetch on a detached HEAD");
}

#[test]
fn a_branch_without_commits_shows_as_the_current_branch() {
    let sandbox = Sandbox::new();
    let empty = sandbox.repository("empty");
    empty.git(&["switch", "--quiet", "--orphan", "fresh"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &empty);

    assert_eq!(
        gikli.current_branch(&root),
        Ok(CurrentBranch::Branch {
            name: "fresh".to_owned(),
            upstream: None,
        })
    );
    assert_eq!(gikli.local_branches(&root), Ok(vec![]));
    assert_eq!(empty.git(&["symbolic-ref", "HEAD"]), "refs/heads/fresh\n");
    assert_eq!(
        empty.git(&["for-each-ref", "refs/heads/"]),
        "",
        "git lists no branch either"
    );
}

#[test]
fn a_linked_worktree_shows_its_own_current_branch_and_counts() {
    let sandbox = Sandbox::new();
    let (remote, work) = remote_with_work(&sandbox);
    let mine = sandbox.clone_of(&remote, "mine");
    let worktree = mine.linked_worktree("feature");
    worktree.git(&["branch", "--quiet", "--set-upstream-to", "origin/main"]);
    work.commit_at(2, "Second");
    work.git(&["push", "--quiet"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &worktree);

    gikli.fetch_all(&root).expect("fetch from the worktree");

    assert_eq!(
        gikli.current_branch(&root),
        Ok(CurrentBranch::Branch {
            name: "feature".to_owned(),
            upstream: upstream("origin/main", 1, 0),
        })
    );
    assert_eq!(counted_by_git(&worktree, "feature", "origin/main"), (1, 0));
}

#[test]
fn a_branch_that_follows_another_local_branch_counts_against_it() {
    let sandbox = Sandbox::new();
    let (remote, _work) = remote_with_work(&sandbox);
    let mine = sandbox.clone_of(&remote, "mine");
    mine.git(&["switch", "--quiet", "--create", "topic", "--track", "main"]);
    mine.commit_at(2, "On topic");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    assert_eq!(
        gikli.current_branch(&root),
        Ok(CurrentBranch::Branch {
            name: "topic".to_owned(),
            upstream: upstream("main", 0, 1),
        })
    );
    assert_eq!(counted_by_git(&mine, "topic", "main"), (0, 1));
    assert_eq!(
        gikli.fetch_branch(&root, &label("topic", RefKind::Local)),
        Err(Failure::NoUpstream {
            branch: "topic".to_owned()
        })
    );
}

#[test]
fn a_remote_branch_of_a_remote_with_its_own_mapping_fetches_from_that_remote() {
    let sandbox = Sandbox::new();
    let (mine, work) = both_branches_moved_on(&sandbox);
    mine.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/mirror/*",
    ]);
    mine.git(&["fetch", "--quiet", "origin", "main"]);
    mine.git(&["update-ref", "refs/remotes/mirror/side", "origin/side"]);
    mine.git(&[
        "branch",
        "--quiet",
        "--set-upstream-to",
        "mirror/side",
        "side",
    ]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .fetch_branch(&root, &label("mirror/side", RefKind::Remote))
        .expect("fetch the branch");

    assert_eq!(
        mine.git(&["rev-parse", "mirror/side"]),
        work.git(&["rev-parse", "side"]),
        "git agrees the remote branch the mapping names was fetched"
    );
    let branches = gikli.local_branches(&root).expect("read the branches");
    assert_eq!(branches[1].upstream, upstream("mirror/side", 1, 0));
    assert_eq!(counted_by_git(&mine, "side", "mirror/side"), (1, 0));
}

#[test]
fn an_upstream_outside_the_remote_branches_is_counted_and_not_gone() {
    let sandbox = Sandbox::new();
    let (remote, work) = remote_with_work(&sandbox);
    let mine = sandbox.clone_of(&remote, "mine");
    mine.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/mirrored/origin/*",
    ]);
    work.commit_at(2, "Second");
    work.git(&["push", "--quiet"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli.fetch_all(&root).expect("fetch every remote");

    assert_eq!(
        gikli.current_branch(&root),
        Ok(CurrentBranch::Branch {
            name: "main".to_owned(),
            upstream: upstream("refs/mirrored/origin/main", 1, 0),
        })
    );
    assert_eq!(
        mine.git(&["rev-parse", "--symbolic-full-name", "main@{upstream}"]),
        "refs/mirrored/origin/main\n",
        "git agrees where the upstream is"
    );
    assert_eq!(
        counted_by_git(&mine, "main", "refs/mirrored/origin/main"),
        (1, 0)
    );
}
