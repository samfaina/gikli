//! The log: the user reads the history of a Repository as a commit list with a graph.

use gikli_acceptance::{Sandbox, TestRepository, minute_as_seconds};
use gikli_core::{Failure, Gikli, GraphLine, LogPage, LogRow, Repository};

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

#[test]
fn the_log_lists_commits_newest_first_with_what_a_row_shows() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "First");
    repository.commit_at(2, "Second\n\nThe body stays out of the list.");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    let newest = &page.rows[0];
    assert_eq!(page.rows.len(), 2);
    assert_eq!(newest.hash, hash_of(&repository, "HEAD"));
    assert_eq!(newest.parents, vec![hash_of(&repository, "HEAD~1")]);
    assert_eq!(newest.subject, "Second");
    assert_eq!(newest.author, "Test User");
    assert_eq!(newest.committed_at, minute_as_seconds(2));
    assert_eq!(page.rows[1].subject, "First");
    assert!(page.rows[1].parents.is_empty());
    assert_eq!(page.next, None);
}

#[test]
fn a_repository_with_no_commits_shows_an_empty_log() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert!(page.rows.is_empty());
    assert_eq!(page.next, None);
    assert_eq!(page.repository, root);
    assert!(
        !repository.git_succeeds(&["rev-parse", "--verify", "--quiet", "HEAD"]),
        "git agrees there is no commit"
    );
}

/// The subjects of `rows`, in order.
fn subjects(rows: &[LogRow]) -> Vec<&str> {
    rows.iter().map(|row| row.subject.as_str()).collect()
}

/// Checks that `rows` are the commits plain git lists from `revisions`, in its order, each with the
/// parents git gives it.
fn assert_git_lists(repository: &TestRepository, revisions: &[&str], rows: &[LogRow]) {
    let mut args = vec!["log", "--date-order", "--format=%H %P"];
    args.extend(revisions);
    let by_git: Vec<String> = repository.git(&args).lines().map(str::to_owned).collect();
    let by_gikli: Vec<String> = rows
        .iter()
        .map(|row| format!("{} {}", row.hash, row.parents.join(" ")))
        .collect();
    assert_eq!(by_gikli, by_git);
}

/// What the log shows: every branch, remote branch and tag, and HEAD.
const WHOLE_GRAPH: &[&str] = &["--branches", "--remotes", "--tags", "HEAD"];

#[test]
fn every_branch_remote_branch_tag_and_head_show_in_order_of_commit_date() {
    let sandbox = Sandbox::new();
    let remote = sandbox.bare_remote("remote.git");
    let repository = sandbox.repository("app");
    repository.add_remote("origin", &remote);
    repository.commit_at(1, "Base");
    repository.git(&["push", "--quiet", "origin", "main"]);

    repository.git(&["switch", "--quiet", "--create", "only-remote"]);
    repository.commit_at(5, "On a remote branch");
    repository.git(&["push", "--quiet", "origin", "only-remote"]);
    repository.git(&["switch", "--quiet", "main"]);
    repository.git(&["branch", "--quiet", "--delete", "--force", "only-remote"]);

    repository.git(&["switch", "--quiet", "--create", "side", "main"]);
    repository.commit_at(3, "On a local branch");
    repository.git(&["switch", "--quiet", "--detach", "main"]);
    repository.commit_at(2, "Under a tag");
    repository.git(&["tag", "v1"]);
    repository.git(&["switch", "--quiet", "--detach", "main"]);
    repository.commit_at(4, "On detached HEAD");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert_eq!(
        subjects(&page.rows),
        vec![
            "On a remote branch",
            "On detached HEAD",
            "On a local branch",
            "Under a tag",
            "Base",
        ]
    );
    assert_git_lists(&repository, WHOLE_GRAPH, &page.rows);
}

#[test]
fn commits_only_a_stash_or_a_backup_holds_stay_out_of_the_log() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.commit_all("Base");
    repository.write("a.txt", "a changed\n");
    repository.git(&["stash", "push", "--quiet", "--message", "Saved work"]);
    repository.commit_at(9, "Rewritten away");
    let backup = hash_of(&repository, "HEAD");
    repository.git(&["update-ref", "refs/gikli/backups/one", &backup]);
    repository.git(&["reset", "--quiet", "--hard", "HEAD~1"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert_eq!(subjects(&page.rows), vec!["Base"]);
    assert_git_lists(&repository, WHOLE_GRAPH, &page.rows);
}

#[test]
fn a_branch_with_commits_shows_while_head_is_on_a_branch_with_none() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "On main");
    repository.git(&["switch", "--quiet", "--orphan", "fresh"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert_eq!(subjects(&page.rows), vec!["On main"]);
    assert_git_lists(&repository, &["--branches"], &page.rows);
}

/// A row of the graph as a test states it: the subject, the column of the commit, and the lines
/// above and below it as `(from column, to column)`.
type Drawn<'a> = (&'a str, usize, Vec<(usize, usize)>, Vec<(usize, usize)>);

fn drawn(rows: &[LogRow]) -> Vec<Drawn<'_>> {
    let pairs = |lines: &[GraphLine]| lines.iter().map(|line| (line.from, line.to)).collect();
    rows.iter()
        .map(|row| {
            (
                row.subject.as_str(),
                row.graph.column,
                pairs(&row.graph.above),
                pairs(&row.graph.below),
            )
        })
        .collect()
}

#[test]
fn a_straight_history_stays_in_one_column() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "First");
    repository.commit_at(2, "Second");
    repository.commit_at(3, "Third");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert_eq!(
        drawn(&page.rows),
        vec![
            ("Third", 0, vec![], vec![(0, 0)]),
            ("Second", 0, vec![(0, 0)], vec![(0, 0)]),
            ("First", 0, vec![(0, 0)], vec![]),
        ]
    );
    assert_git_lists(&repository, WHOLE_GRAPH, &page.rows);
}

/// A repository where `side` left `main` at Base and was merged back:
/// Merge (Main work, Side work), Side work, Main work, Base.
fn repository_with_a_merge(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.git(&["switch", "--quiet", "--create", "side"]);
    repository.commit_at(3, "Side work");
    repository.git(&["switch", "--quiet", "main"]);
    repository.commit_at(2, "Main work");
    repository.git_at(
        4,
        &["merge", "--quiet", "--no-ff", "--message", "Merge", "side"],
    );
    repository
}

#[test]
fn a_merge_opens_a_lane_for_the_branch_it_brings_in_and_the_lane_closes_at_the_fork() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_merge(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert_eq!(
        drawn(&page.rows),
        vec![
            ("Merge", 0, vec![], vec![(0, 0), (0, 1)]),
            ("Side work", 1, vec![(0, 0), (1, 1)], vec![(0, 0), (1, 1)]),
            ("Main work", 0, vec![(0, 0), (1, 1)], vec![(0, 0), (1, 0)]),
            ("Base", 0, vec![(0, 0)], vec![]),
        ]
    );
    assert_git_lists(&repository, WHOLE_GRAPH, &page.rows);
}

#[test]
fn branches_that_run_in_parallel_each_keep_a_lane_until_they_meet() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.git(&["switch", "--quiet", "--create", "side"]);
    repository.commit_at(3, "Side one");
    repository.commit_at(5, "Side two");
    repository.git(&["switch", "--quiet", "main"]);
    repository.commit_at(2, "Main one");
    repository.commit_at(4, "Main two");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert_eq!(
        drawn(&page.rows),
        vec![
            ("Side two", 0, vec![], vec![(0, 0)]),
            ("Main two", 1, vec![(0, 0)], vec![(0, 0), (1, 1)]),
            ("Side one", 0, vec![(0, 0), (1, 1)], vec![(0, 0), (1, 1)]),
            ("Main one", 1, vec![(0, 0), (1, 1)], vec![(0, 0), (1, 0)]),
            ("Base", 0, vec![(0, 0)], vec![]),
        ]
    );
    assert_git_lists(&repository, WHOLE_GRAPH, &page.rows);
}

#[test]
fn a_merge_of_three_branches_gives_each_one_its_own_lane() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    for (minute, branch) in [(2, "one"), (3, "two")] {
        repository.git(&["switch", "--quiet", "--create", branch, "main"]);
        repository.commit_at(minute, branch);
    }
    repository.git(&["switch", "--quiet", "main"]);
    repository.git_at(
        4,
        &[
            "merge",
            "--quiet",
            "--no-ff",
            "--message",
            "Octopus",
            "one",
            "two",
        ],
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert_eq!(
        drawn(&page.rows),
        vec![
            ("Octopus", 0, vec![], vec![(0, 0), (0, 1), (0, 2)]),
            (
                "two",
                2,
                vec![(0, 0), (1, 1), (2, 2)],
                vec![(0, 0), (1, 1), (2, 0)]
            ),
            ("one", 1, vec![(0, 0), (1, 1)], vec![(0, 0), (1, 0)]),
            ("Base", 0, vec![(0, 0)], vec![]),
        ]
    );
    assert_git_lists(&repository, WHOLE_GRAPH, &page.rows);
}

#[test]
fn a_commit_two_merges_bring_in_keeps_one_lane() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.git(&["switch", "--quiet", "--create", "side"]);
    repository.commit_at(2, "Side");
    repository.git(&["switch", "--quiet", "--create", "other", "main"]);
    let merge = |minute, message| {
        repository.git_at(
            minute,
            &["merge", "--quiet", "--no-ff", "--message", message, "side"],
        );
    };
    merge(4, "Merge two");
    repository.git(&["switch", "--quiet", "main"]);
    merge(3, "Merge one");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert_eq!(
        drawn(&page.rows),
        vec![
            ("Merge two", 0, vec![], vec![(0, 0), (0, 1)]),
            (
                "Merge one",
                2,
                vec![(0, 0), (1, 1)],
                vec![(0, 0), (1, 1), (2, 0), (2, 1)]
            ),
            ("Side", 1, vec![(0, 0), (1, 1)], vec![(0, 0), (1, 0)]),
            ("Base", 0, vec![(0, 0)], vec![]),
        ]
    );
    assert_git_lists(&repository, WHOLE_GRAPH, &page.rows);
}

#[test]
fn a_new_tip_takes_the_first_free_lane_and_unrelated_histories_each_get_one() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Root a");
    repository.git(&["switch", "--quiet", "--orphan", "unrelated"]);
    repository.commit_at(3, "Root b");
    repository.commit_at(5, "Tip b");
    repository.git(&["switch", "--quiet", "--orphan", "lone"]);
    repository.commit_at(2, "Lone");
    repository.git(&["switch", "--quiet", "main"]);
    repository.commit_at(4, "Tip a");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert_eq!(
        drawn(&page.rows),
        vec![
            ("Tip b", 0, vec![], vec![(0, 0)]),
            ("Tip a", 1, vec![(0, 0)], vec![(0, 0), (1, 1)]),
            ("Root b", 0, vec![(0, 0), (1, 1)], vec![(1, 1)]),
            ("Lone", 0, vec![(1, 1)], vec![(1, 1)]),
            ("Root a", 1, vec![(1, 1)], vec![]),
        ]
    );
    assert_git_lists(&repository, WHOLE_GRAPH, &page.rows);
}

/// Every page of the log of `root`, `count` commits at a time.
fn all_pages(gikli: &Gikli, root: &Repository, count: usize) -> Vec<LogPage> {
    let mut pages = vec![gikli.read_log(root, None, count).expect("read the log")];
    while let Some(next) = pages.last().and_then(|page| page.next.clone()) {
        pages.push(
            gikli
                .read_log(root, Some(&next), count)
                .expect("read a page"),
        );
    }
    pages
}

#[test]
fn the_log_comes_page_by_page_and_the_graph_goes_on_across_the_pages() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_merge(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let pages = all_pages(&gikli, &root, 1);

    let rows: Vec<LogRow> = pages.iter().flat_map(|page| page.rows.clone()).collect();
    assert_eq!(pages.len(), 4);
    assert!(pages.iter().all(|page| page.rows.len() == 1));
    assert_eq!(
        drawn(&rows),
        vec![
            ("Merge", 0, vec![], vec![(0, 0), (0, 1)]),
            ("Side work", 1, vec![(0, 0), (1, 1)], vec![(0, 0), (1, 1)]),
            ("Main work", 0, vec![(0, 0), (1, 1)], vec![(0, 0), (1, 0)]),
            ("Base", 0, vec![(0, 0)], vec![]),
        ]
    );
    assert_git_lists(&repository, WHOLE_GRAPH, &rows);
}

#[test]
fn the_page_that_reaches_the_oldest_commit_is_the_last() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_merge(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let first = gikli.read_log(&root, None, 4).expect("read the log");
    let short = gikli.read_log(&root, None, 3).expect("read the log");

    assert_eq!(first.rows.len(), 4);
    assert_eq!(first.next, None);
    assert_eq!(short.rows.len(), 3);
    assert!(short.next.is_some(), "one commit is left");
    assert_git_lists(&repository, WHOLE_GRAPH, &first.rows);
}

#[test]
fn a_page_asked_for_after_the_history_moved_says_so_instead_of_showing_the_wrong_commits() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_merge(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let first = gikli.read_log(&root, None, 2).expect("read the log");
    repository.commit_at(9, "Made after the first page");

    let next = gikli.read_log(&root, first.next.as_ref(), 2);

    assert_eq!(next, Err(Failure::LogChanged));
    assert_eq!(
        repository.git(&["log", "-1", "--format=%s"]),
        "Made after the first page\n"
    );
}

#[test]
fn a_page_asked_for_after_a_branch_was_replaced_says_so_even_when_the_last_commit_stayed_in_place()
{
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.git(&["switch", "--quiet", "--create", "side"]);
    repository.commit_at(2, "Below the page");
    repository.commit_at(8, "Replaced one");
    repository.commit_at(9, "Replaced two");
    repository.git(&["switch", "--quiet", "main"]);
    repository.commit_at(7, "Last of the page");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let first = gikli.read_log(&root, None, 3).expect("read the log");
    assert_eq!(
        subjects(&first.rows),
        vec!["Replaced two", "Replaced one", "Last of the page"]
    );

    repository.git(&["switch", "--quiet", "side"]);
    repository.git(&["reset", "--quiet", "--hard", "main~1"]);
    repository.commit_at(10, "New one");
    repository.commit_at(11, "New two");
    assert_eq!(
        repository.git(&[
            "log",
            "--branches",
            "--date-order",
            "--skip=2",
            "-1",
            "--format=%s"
        ]),
        "Last of the page\n",
        "the last commit of the page is where it was"
    );

    let next = gikli.read_log(&root, first.next.as_ref(), 3);

    assert_eq!(next, Err(Failure::LogChanged));
}

#[test]
fn the_users_log_settings_do_not_change_what_the_log_reads() {
    let sandbox = Sandbox::new();
    sandbox.configure(concat!(
        "[format]\n\tpretty = oneline\n",
        "[log]\n\tabbrevCommit = true\n\tdate = short\n\tdecorate = full\n",
        "\tshowSignature = true\n\tinitialDecorationSet = all\n",
        "[i18n]\n\tlogOutputEncoding = ISO-8859-1\n",
    ));
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Größe ändern ✓");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let page = gikli.read_log(&root, None, 50).expect("read the log");

    assert_eq!(subjects(&page.rows), vec!["Größe ändern ✓"]);
    assert_eq!(page.rows[0].hash, hash_of(&repository, "HEAD"));
    assert_eq!(page.rows[0].committed_at, minute_as_seconds(1));
}

#[test]
fn a_repository_git_cannot_read_says_so() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    repository.write(".git/HEAD", "not a reference");

    let read = gikli.read_log(&root, None, 50);

    assert_eq!(
        read,
        Err(Failure::LogUnreadable {
            folder: root.root.clone()
        })
    );
    assert!(
        !repository.git_succeeds(&["log", "-1"]),
        "git cannot read it either"
    );
}
