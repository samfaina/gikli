//! A shallow clone: the log shows where its history is cut, and gikli fetches the rest.

use gikli_acceptance::{Sandbox, TestRepository, file_url, minute_as_seconds};
use gikli_core::{Event, Failure, Gikli, LogRow, LongOperation, RefKind, Repository};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A bare remote whose `main` holds the commits First, Second and Third, one a minute, and whose
/// `side` branches off Second with On side.
fn remote_with_history(sandbox: &Sandbox) -> TestRepository {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.commit_at(2, "Second");
    work.git(&["switch", "--quiet", "--create", "side"]);
    work.commit_at(4, "On side");
    work.git(&["switch", "--quiet", "main"]);
    work.commit_at(3, "Third");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main", "side"]);
    remote
}

/// A clone of `remote` with plain git, cut `depth` commits down, with the extra options `options`.
fn shallow_clone(
    sandbox: &Sandbox,
    remote: &TestRepository,
    depth: &str,
    options: &[&str],
) -> TestRepository {
    let target = sandbox.root().join("shallow");
    let target = target.to_string_lossy().into_owned();
    // A local path clones every object whatever the depth; a file URL is cut as a network one.
    let url = file_url(remote.root());
    let mut args = vec!["clone", "--quiet", "--depth", depth];
    args.extend(options);
    args.extend(["--", &url, &target]);
    sandbox.existing_repository(sandbox.root()).git(&args);
    sandbox.existing_repository(std::path::Path::new(&target))
}

/// The subjects of `rows` whose history is cut, in order.
fn cut(rows: &[LogRow]) -> Vec<&str> {
    rows.iter()
        .filter(|row| row.history_cut)
        .map(|row| row.subject.as_str())
        .collect()
}

#[test]
fn the_log_marks_the_commits_where_a_shallow_history_is_cut() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "2", &["--no-single-branch"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    let subjects: Vec<&str> = page.rows.iter().map(|row| row.subject.as_str()).collect();
    assert_eq!(subjects, ["On side", "Third", "Second"]);
    assert_eq!(cut(&page.rows), ["Second"]);
    assert_eq!(
        clone.git(&["rev-parse", "--is-shallow-repository"]),
        "true\n",
        "git agrees the history is cut"
    );
    assert_eq!(page.rows[2].committed_at, minute_as_seconds(2));
}

#[test]
fn the_first_commit_of_a_whole_history_is_not_marked_as_cut() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = sandbox.clone_of(&remote, "whole");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert_eq!(page.rows.last().expect("a commit").subject, "First");
    assert!(page.rows.last().expect("a commit").parents.is_empty());
    assert_eq!(cut(&page.rows), Vec::<&str>::new());
}

#[test]
fn every_page_of_a_shallow_log_says_the_history_is_cut_so_gikli_offers_the_rest() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "2", &["--no-single-branch"]);
    let whole = sandbox.clone_of(&remote, "whole");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);

    let first = gikli.read_log(&root, None, 1).expect("read the log");
    let next = first.next.as_ref().expect("a page after the first");
    let second = gikli.read_log(&root, Some(next), 1).expect("read on");

    assert_eq!(cut(&first.rows), Vec::<&str>::new());
    assert!(first.history_cut);
    assert!(second.history_cut);
    let whole_page = gikli
        .read_log(&opened(&gikli, &whole), None, 50)
        .expect("read the whole log");
    assert!(!whole_page.history_cut);
}

/// The remote branches the log labels its commits with, by name.
fn remote_branches(rows: &[LogRow]) -> Vec<&str> {
    let mut names: Vec<&str> = rows
        .iter()
        .flat_map(|row| &row.labels)
        .filter(|label| label.kind == RefKind::Remote)
        .map(|label| label.name.as_str())
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn fetching_the_rest_of_a_single_branch_shallow_clone_brings_the_whole_history_of_every_remote_branch()
 {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "1", &[]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);
    let before = gikli.read_log(&root, None, 50).expect("read the log");
    assert_eq!(remote_branches(&before.rows), ["origin/main"]);

    gikli
        .fetch_rest_of_history(&root)
        .expect("fetch the rest of the history");

    let after = gikli.read_log(&root, None, 50).expect("read the log again");
    let subjects: Vec<&str> = after.rows.iter().map(|row| row.subject.as_str()).collect();
    assert_eq!(subjects, ["On side", "Third", "Second", "First"]);
    assert_eq!(cut(&after.rows), Vec::<&str>::new());
    assert!(!after.history_cut);
    assert_eq!(remote_branches(&after.rows), ["origin/main", "origin/side"]);
    assert_eq!(
        clone.git(&["rev-parse", "--is-shallow-repository"]),
        "false\n",
        "git agrees the history is whole"
    );
}

#[test]
fn fetching_the_rest_runs_as_a_long_operation_from_the_remote_it_names() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "1", &[]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);
    let events = gikli.subscribe();

    gikli
        .fetch_rest_of_history(&root)
        .expect("fetch the rest of the history");

    let received: Vec<Event> = events.try_iter().collect();
    let (id, operation) = received
        .iter()
        .find_map(|event| match event {
            Event::OperationStarted { id, operation, .. } => Some((*id, operation.clone())),
            _ => None,
        })
        .expect("the fetch announces it started");
    assert_eq!(
        operation,
        LongOperation::FetchRestOfHistory {
            repository: root.clone(),
            remote: "origin".to_owned(),
        }
    );
    assert!(received.contains(&Event::OperationFinished { id }));
}

#[test]
fn the_rest_comes_from_the_remote_the_current_branch_fetches_from_before_origin() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "1", &[]);
    clone.git(&["remote", "rename", "origin", "upstream"]);
    let elsewhere = sandbox.bare_remote("elsewhere.git");
    clone.add_remote("origin", &elsewhere);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);

    gikli
        .fetch_rest_of_history(&root)
        .expect("fetch the rest of the history");

    let after = gikli.read_log(&root, None, 50).expect("read the log");
    assert!(!after.history_cut);
    assert_eq!(
        remote_branches(&after.rows),
        ["upstream/main", "upstream/side"]
    );
}

#[test]
fn a_shallow_clone_without_a_remote_has_nowhere_to_fetch_the_rest_from() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "1", &[]);
    clone.git(&["remote", "remove", "origin"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);

    let fetched = gikli.fetch_rest_of_history(&root);

    assert_eq!(
        fetched,
        Err(Failure::NoRemoteToFetchFrom {
            folder: root.root.clone()
        })
    );
    assert!(gikli.read_log(&root, None, 50).expect("read").history_cut);
}

#[test]
fn when_git_cannot_reach_the_remote_the_history_stays_cut() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "1", &[]);
    std::fs::remove_dir_all(remote.root()).expect("remove the remote");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);

    let fetched = gikli.fetch_rest_of_history(&root);

    assert_eq!(
        fetched,
        Err(Failure::HistoryNotFetched {
            folder: root.root.clone(),
            remote: "origin".to_owned(),
        })
    );
    assert!(gikli.read_log(&root, None, 50).expect("read").history_cut);
}

#[test]
fn a_page_read_before_the_rest_was_fetched_finds_the_history_changed() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "2", &["--no-single-branch"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);
    let first = gikli.read_log(&root, None, 1).expect("read the log");

    gikli
        .fetch_rest_of_history(&root)
        .expect("fetch the rest of the history");

    let next = first.next.as_ref().expect("a page after the first");
    assert_eq!(
        gikli.read_log(&root, Some(next), 1),
        Err(Failure::LogChanged)
    );
}

#[test]
fn after_the_rest_is_fetched_a_later_fetch_keeps_every_remote_branch_up_to_date() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "1", &[]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);
    gikli
        .fetch_rest_of_history(&root)
        .expect("fetch the rest of the history");

    let work = sandbox.existing_repository(&sandbox.root().join("work"));
    work.git(&["switch", "--quiet", "side"]);
    work.commit_at(5, "Later on side");
    work.git(&["push", "--quiet", "origin", "side"]);
    clone.git(&["fetch", "--quiet"]);

    assert_eq!(
        clone.git(&["log", "-1", "--format=%s", "origin/side"]),
        "Later on side\n"
    );
}

#[test]
fn a_remote_that_already_fetches_every_branch_elsewhere_keeps_its_own_remote_branches() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "1", &["--no-single-branch"]);
    clone.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/mirror/*",
    ]);
    clone.git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    clone.git(&["update-ref", "-d", "refs/remotes/origin/side"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);

    gikli
        .fetch_rest_of_history(&root)
        .expect("fetch the rest of the history");

    let after = gikli.read_log(&root, None, 50).expect("read the log");
    assert!(!after.history_cut);
    assert_eq!(remote_branches(&after.rows), ["mirror/main", "mirror/side"]);
    assert_eq!(
        clone.git(&["config", "--get-all", "remote.origin.fetch"]),
        "+refs/heads/*:refs/remotes/mirror/*\n"
    );
}

#[test]
fn a_linked_worktree_of_a_shallow_clone_shows_the_cut_and_fetches_the_rest() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "2", &["--no-single-branch"]);
    let worktree = clone.linked_worktree("feature");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &worktree);

    let before = gikli.read_log(&root, None, 50).expect("read the log");
    gikli
        .fetch_rest_of_history(&root)
        .expect("fetch the rest of the history");
    let after = gikli.read_log(&root, None, 50).expect("read the log again");

    assert_eq!(cut(&before.rows), ["Second"]);
    assert!(before.history_cut);
    assert!(!after.history_cut);
    assert_eq!(after.rows.last().expect("a commit").subject, "First");
}

#[test]
fn on_a_detached_head_the_rest_comes_from_origin() {
    let sandbox = Sandbox::new();
    let remote = remote_with_history(&sandbox);
    let clone = shallow_clone(&sandbox, &remote, "1", &[]);
    clone.git(&["switch", "--quiet", "--detach"]);
    let elsewhere = sandbox.bare_remote("elsewhere.git");
    clone.add_remote("another", &elsewhere);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &clone);
    let events = gikli.subscribe();

    gikli
        .fetch_rest_of_history(&root)
        .expect("fetch the rest of the history");

    let started = events.try_iter().find_map(|event| match event {
        Event::OperationStarted { operation, .. } => Some(operation),
        _ => None,
    });
    assert_eq!(
        started,
        Some(LongOperation::FetchRestOfHistory {
            repository: root.clone(),
            remote: "origin".to_owned(),
        })
    );
    assert!(!gikli.read_log(&root, None, 50).expect("read").history_cut);
}
