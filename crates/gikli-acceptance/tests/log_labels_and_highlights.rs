//! The log is quick to read: references are labeled, the current branch stands out, and gikli's
//! own refs stay out of sight.

use gikli_acceptance::{Sandbox, TestRepository, opened};
use gikli_core::{Gikli, LogRow, RefKind, Repository};

/// Every row of the log of `root`, read in one page.
fn rows_of(gikli: &Gikli, root: &Repository) -> Vec<LogRow> {
    gikli.read_log(root, None, 500).expect("read the log").rows
}

/// Checks that `rows` are the commits plain git lists for the whole graph, in its order, each
/// with the parents git gives it.
fn assert_git_lists(repository: &TestRepository, rows: &[LogRow]) {
    let by_git = repository.git(&[
        "log",
        "--date-order",
        "--format=%H %P",
        "--branches",
        "--remotes",
        "--tags",
        "HEAD",
    ]);
    let by_gikli: Vec<String> = rows
        .iter()
        .map(|row| format!("{} {}", row.hash, row.parents.join(" ")))
        .collect();
    assert_eq!(by_gikli, by_git.lines().collect::<Vec<_>>());
}

/// The subjects plain git lists from HEAD, newest commit date first.
fn subjects_head_reaches_by_git(repository: &TestRepository) -> Vec<String> {
    let by_git = repository.git(&["log", "--date-order", "--format=%s", "HEAD"]);
    by_git.lines().map(str::to_owned).collect()
}

/// The row of the commit with `subject`.
fn row<'a>(rows: &'a [LogRow], subject: &str) -> &'a LogRow {
    rows.iter()
        .find(|row| row.subject == subject)
        .unwrap_or_else(|| panic!("no row {subject}"))
}

/// The labels of `row` as `(kind, name)`, in the order they show.
fn labels(row: &LogRow) -> Vec<(RefKind, &str)> {
    row.labels
        .iter()
        .map(|label| (label.kind, label.name.as_str()))
        .collect()
}

#[test]
fn head_local_branches_remote_branches_and_tags_label_their_commits_each_by_kind() {
    let sandbox = Sandbox::new();
    let remote = sandbox.bare_remote("remote.git");
    let repository = sandbox.repository("app");
    repository.add_remote("origin", &remote);
    repository.commit_at(1, "Base");
    repository.git(&["tag", "v1"]);
    repository.git(&["tag", "--annotate", "--message", "Release", "v1-annotated"]);
    repository.git(&["push", "--quiet", "--set-upstream", "origin", "main"]);
    repository.git(&["remote", "set-head", "origin", "main"]);
    repository.git(&["branch", "feature"]);
    repository.commit_at(2, "Ahead");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_of(&gikli, &root);

    assert_eq!(
        labels(row(&rows, "Ahead")),
        vec![(RefKind::Head, "HEAD"), (RefKind::Local, "main")]
    );
    assert_eq!(
        labels(row(&rows, "Base")),
        vec![
            (RefKind::Local, "feature"),
            (RefKind::Remote, "origin/main"),
            (RefKind::Tag, "v1"),
            (RefKind::Tag, "v1-annotated"),
        ]
    );
    assert_eq!(
        decorations_by_git(&repository, "main~1"),
        vec![
            "feature",
            "origin/HEAD",
            "origin/main",
            "tag: v1",
            "tag: v1-annotated"
        ],
        "git puts the same references there, and its own pointer to the remote's default branch"
    );
}

/// The references git decorates `revision` with, sorted.
fn decorations_by_git(repository: &TestRepository, revision: &str) -> Vec<String> {
    let text = repository.git(&["log", "-1", "--decorate=short", "--format=%D", revision]);
    let mut decorations: Vec<String> = text
        .trim_end()
        .split(", ")
        .filter(|decoration| !decoration.is_empty())
        .map(str::to_owned)
        .collect();
    decorations.sort();
    decorations
}

#[test]
fn stashes_and_gikli_refs_label_no_commit() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.commit_all("Base");
    repository.write("a.txt", "a changed\n");
    repository.git(&["stash", "push", "--quiet", "--message", "Saved work"]);
    let base = repository.commit_of("HEAD");
    repository.git(&["update-ref", "refs/gikli/backups/one", &base]);
    repository.git(&["update-ref", "refs/gikli/other", &base]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_of(&gikli, &root);

    assert_eq!(rows.len(), 1);
    assert_eq!(
        labels(&rows[0]),
        vec![(RefKind::Head, "HEAD"), (RefKind::Local, "main")]
    );
    assert_eq!(
        repository.git(&["for-each-ref", "--format=%(refname)", "--points-at", &base]),
        "refs/gikli/backups/one\nrefs/gikli/other\nrefs/heads/main\n",
        "git keeps gikli's refs on the commit"
    );
}

/// The subjects of the rows on the current branch, in order.
fn on_current_branch(rows: &[LogRow]) -> Vec<&str> {
    rows.iter()
        .filter(|row| row.on_current_branch)
        .map(|row| row.subject.as_str())
        .collect()
}

/// A repository where `merged` and `other` left `main` at Base, `main` merged `merged` back in,
/// and `other` went on on its own. HEAD is on `main`.
fn repository_with_branches(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.git(&["switch", "--quiet", "--create", "merged"]);
    repository.commit_at(3, "Merged work");
    repository.git(&["switch", "--quiet", "--create", "other", "main"]);
    repository.commit_at(4, "Other work");
    repository.commit_at(6, "More other work");
    repository.git(&["switch", "--quiet", "main"]);
    repository.commit_at(2, "Main work");
    repository.git_at(
        5,
        &[
            "merge",
            "--quiet",
            "--no-ff",
            "--message",
            "Merge",
            "merged",
        ],
    );
    repository
}

#[test]
fn the_commits_head_reaches_are_on_the_current_branch() {
    let sandbox = Sandbox::new();
    let repository = repository_with_branches(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_of(&gikli, &root);

    assert_eq!(
        on_current_branch(&rows),
        vec!["Merge", "Merged work", "Main work", "Base"]
    );
    assert_eq!(
        repository.git(&["log", "--date-order", "--format=%s", "HEAD"]),
        "Merge\nMerged work\nMain work\nBase\n"
    );
}

#[test]
fn the_current_branch_stands_out_across_pages_and_in_detached_head() {
    let sandbox = Sandbox::new();
    let repository = repository_with_branches(&sandbox);
    repository.git(&["switch", "--quiet", "--detach", "other~1"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_by_pages(&gikli, &root, 1);

    assert_eq!(on_current_branch(&rows), vec!["Other work", "Base"]);
    assert_eq!(
        subjects_head_reaches_by_git(&repository),
        vec!["Other work", "Base"]
    );
    assert!(
        !repository.git_succeeds(&["symbolic-ref", "--quiet", "HEAD"]),
        "HEAD is detached"
    );
}

#[test]
fn nothing_is_on_the_current_branch_while_head_has_no_commit() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "On main");
    repository.git(&["switch", "--quiet", "--orphan", "fresh"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_of(&gikli, &root);

    assert_eq!(on_current_branch(&rows), Vec::<&str>::new());
    assert_eq!(labels(&rows[0]), vec![(RefKind::Local, "main")]);
    assert!(
        !repository.git_succeeds(&["rev-parse", "--verify", "--quiet", "HEAD"]),
        "git has no commit for HEAD"
    );
}

#[test]
fn the_users_own_commits_are_the_ones_with_their_email() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Mine");
    repository.git(&[
        "commit",
        "--quiet",
        "--allow-empty",
        "--author",
        "Someone Else <someone@example.com>",
        "--message",
        "Theirs",
    ]);
    repository.git(&[
        "commit",
        "--quiet",
        "--allow-empty",
        "--author",
        "Another Name <TEST@example.com>",
        "--message",
        "Mine under another name",
    ]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_of(&gikli, &root);

    let own: Vec<(&str, bool)> = rows
        .iter()
        .map(|row| (row.subject.as_str(), row.own))
        .collect();
    assert_eq!(
        own,
        vec![
            ("Mine under another name", true),
            ("Theirs", false),
            ("Mine", true),
        ]
    );
    assert_eq!(
        repository.git(&["config", "user.email"]),
        "test@example.com\n"
    );
}

#[test]
fn without_an_email_no_commit_is_the_users_own() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.git(&["config", "user.email", ""]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_of(&gikli, &root);

    assert!(!rows[0].own);
    assert_eq!(
        repository.git(&["log", "-1", "--format=%ae"]),
        "test@example.com\n",
        "the commit still has the email the user had"
    );
    assert_eq!(repository.git(&["config", "user.email"]), "\n");
}

#[test]
fn a_commit_whose_author_is_not_its_committer_is_marked() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Same person");
    repository.git(&[
        "commit",
        "--quiet",
        "--allow-empty",
        "--author",
        "Someone Else <someone@example.com>",
        "--message",
        "Other author",
    ]);
    repository.git(&[
        "commit",
        "--quiet",
        "--allow-empty",
        "--author",
        "Test User <elsewhere@example.com>",
        "--message",
        "Same name, other email",
    ]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_of(&gikli, &root);

    let marked: Vec<(&str, bool)> = rows
        .iter()
        .map(|row| (row.subject.as_str(), row.committer_differs))
        .collect();
    assert_eq!(
        marked,
        vec![
            ("Same name, other email", true),
            ("Other author", true),
            ("Same person", false),
        ]
    );
    assert_eq!(
        repository.git(&["log", "-1", "--format=%an <%ae> / %cn <%ce>", "HEAD~1"]),
        "Someone Else <someone@example.com> / Test User <test@example.com>\n"
    );
}

/// The subjects of the commits `hashes` name, as `rows` show them.
fn subjects_of<'a>(rows: &'a [LogRow], hashes: &[String]) -> Vec<&'a str> {
    hashes
        .iter()
        .map(|hash| {
            rows.iter()
                .find(|row| &row.hash == hash)
                .map_or("not in the log", |row| row.subject.as_str())
        })
        .collect()
}

#[test]
fn each_commit_leads_to_its_parents_and_children_with_the_child_on_its_line_first() {
    let sandbox = Sandbox::new();
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
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_of(&gikli, &root);

    let walked: Vec<(&str, Vec<&str>, Vec<&str>)> = rows
        .iter()
        .map(|row| {
            (
                row.subject.as_str(),
                subjects_of(&rows, &row.parents),
                subjects_of(&rows, &row.children),
            )
        })
        .collect();
    assert_eq!(
        walked,
        vec![
            ("Merge", vec!["Main work", "Side work"], vec![]),
            ("Side work", vec!["Base"], vec!["Merge"]),
            ("Main work", vec!["Base"], vec!["Merge"]),
            ("Base", vec![], vec!["Main work", "Side work"]),
        ]
    );
    let base = repository.commit_of("main~2");
    let by_git = repository.git(&["rev-list", "--children", "--date-order", "main"]);
    let mut children_by_git: Vec<&str> = by_git
        .lines()
        .find_map(|line| line.strip_prefix(&base))
        .expect("git lists Base")
        .split_whitespace()
        .collect();
    children_by_git.sort_unstable();
    let mut children_by_gikli: Vec<&str> = rows[3].children.iter().map(String::as_str).collect();
    children_by_gikli.sort_unstable();
    assert_eq!(
        children_by_gikli, children_by_git,
        "git knows the same children"
    );
}

#[test]
fn a_commit_knows_its_children_on_pages_before_its_own() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.commit_at(2, "Child");
    repository.commit_at(3, "Grandchild");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let first = gikli.read_log(&root, None, 2).expect("read the log");
    let last = gikli
        .read_log(&root, first.next.as_ref(), 2)
        .expect("read a page");

    assert_eq!(last.rows[0].subject, "Base");
    assert_eq!(last.rows[0].children, vec![repository.commit_of("HEAD~1")]);
    assert_eq!(
        repository.git(&["rev-parse", "HEAD~1^"]),
        format!("{}\n", last.rows[0].hash),
        "git has Base as the parent of Child"
    );
}

/// Every row of the log of `root`, read `count` commits at a time.
fn rows_by_pages(gikli: &Gikli, root: &Repository, count: usize) -> Vec<LogRow> {
    let mut page = gikli.read_log(root, None, count).expect("read the log");
    let mut rows = page.rows.clone();
    while let Some(next) = page.next {
        page = gikli
            .read_log(root, Some(&next), count)
            .expect("read a page");
        rows.extend(page.rows.clone());
    }
    rows
}

/// A row of the graph as a test states it: the subject, the column of the commit, the lines above
/// and below it as `(from column, to column)`, and its arrows up and down as
/// `(column, subject of the commit they lead to)`.
type Drawn<'a> = (
    &'a str,
    usize,
    Vec<(usize, usize)>,
    Vec<(usize, usize)>,
    Vec<(usize, &'a str)>,
    Vec<(usize, &'a str)>,
);

fn drawn(rows: &[LogRow]) -> Vec<Drawn<'_>> {
    let subject_of = |hash: &String| subjects_of(rows, std::slice::from_ref(hash))[0];
    rows.iter()
        .map(|row| {
            let graph = &row.graph;
            (
                row.subject.as_str(),
                graph.column,
                graph
                    .above
                    .iter()
                    .map(|line| (line.from, line.to))
                    .collect(),
                graph
                    .below
                    .iter()
                    .map(|line| (line.from, line.to))
                    .collect(),
                graph
                    .arrows_up
                    .iter()
                    .map(|arrow| (arrow.column, subject_of(&arrow.target)))
                    .collect(),
                graph
                    .arrows_down
                    .iter()
                    .map(|arrow| (arrow.column, subject_of(&arrow.target)))
                    .collect(),
            )
        })
        .collect()
}

/// A repository where `side` left `main` at Base and got one commit, newer than the `count`
/// commits `main` got since. So the edge from Side to Base spans them all.
fn repository_with_a_long_edge(sandbox: &Sandbox, count: i64) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.git(&["switch", "--quiet", "--create", "side"]);
    repository.commit_at(1000, "Side");
    repository.git(&["switch", "--quiet", "main"]);
    for minute in 2..2 + count {
        repository.commit_at(minute, &format!("Main {minute}"));
    }
    repository
}

#[test]
fn a_long_edge_is_cut_into_two_arrows_that_lead_to_each_other_and_frees_its_lane() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_long_edge(&sandbox, 40);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_by_pages(&gikli, &root, 7);

    let drawn = drawn(&rows);
    assert_eq!(rows.len(), 42);
    assert_eq!(
        drawn[0],
        ("Side", 0, vec![], vec![], vec![], vec![(0, "Base")])
    );
    assert_eq!(
        drawn[1],
        ("Main 41", 0, vec![], vec![(0, 0)], vec![], vec![])
    );
    assert_eq!(
        drawn[41],
        ("Base", 0, vec![(0, 0)], vec![], vec![(1, "Side")], vec![])
    );
    assert!(
        drawn[1..41]
            .iter()
            .all(|row| row.1 == 0 && row.2.len() <= 1 && row.3.len() == 1),
        "main keeps one lane to itself"
    );
    assert_eq!(rows[0].parents, vec![repository.commit_of("main~40")]);
    assert_git_lists(&repository, &rows);
}

#[test]
fn an_edge_of_thirty_rows_is_drawn_whole() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_long_edge(&sandbox, 29);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_by_pages(&gikli, &root, 7);

    let drawn = drawn(&rows);
    assert_eq!(rows.len(), 31);
    assert_eq!(drawn[0], ("Side", 0, vec![], vec![(0, 0)], vec![], vec![]));
    assert_eq!(
        drawn[1],
        (
            "Main 30",
            1,
            vec![(0, 0)],
            vec![(0, 0), (1, 1)],
            vec![],
            vec![]
        )
    );
    assert_eq!(drawn[30], ("Base", 0, vec![(0, 0)], vec![], vec![], vec![]));
    assert_eq!(
        repository.git(&["rev-list", "--count", "side^..main"]),
        "29\n",
        "the edge from Side to Base spans the 29 commits main got since"
    );
    assert_git_lists(&repository, &rows);
}

#[test]
fn long_edges_are_drawn_whole_once_the_user_turns_them_on() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_long_edge(&sandbox, 40);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    assert!(!gikli.settings().show_long_edges, "off by default");

    gikli.set_show_long_edges(true).expect("turn long edges on");
    let rows = rows_by_pages(&gikli, &root, 7);

    let drawn = drawn(&rows);
    assert_eq!(drawn[0], ("Side", 0, vec![], vec![(0, 0)], vec![], vec![]));
    assert_eq!(
        drawn[1],
        (
            "Main 41",
            1,
            vec![(0, 0)],
            vec![(0, 0), (1, 1)],
            vec![],
            vec![]
        )
    );
    assert_eq!(drawn[41], ("Base", 0, vec![(0, 0)], vec![], vec![], vec![]));
    assert!(
        sandbox.gikli().settings().show_long_edges,
        "the setting stays"
    );
    assert_git_lists(&repository, &rows);
}

#[test]
fn a_sha256_repository_shows_its_64_character_hashes() {
    let sandbox = Sandbox::new();
    let repository = sandbox.existing_repository(&sandbox.folder("app"));
    repository.git(&["init", "--quiet", "--object-format=sha256"]);
    repository.commit_at(1, "Base");
    repository.git(&["tag", "--annotate", "--message", "Release", "v1"]);
    repository.git(&["switch", "--quiet", "--create", "side"]);
    repository.commit_at(3, "Side work");
    repository.git(&["switch", "--quiet", "main"]);
    repository.commit_at(2, "Main work");
    repository.git_at(
        4,
        &["merge", "--quiet", "--no-ff", "--message", "Merge", "side"],
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_by_pages(&gikli, &root, 1);

    let merge = row(&rows, "Merge");
    let base = row(&rows, "Base");
    assert_eq!(merge.hash, repository.commit_of("HEAD"));
    assert_eq!(merge.hash.len(), 64);
    assert_eq!(
        merge.parents,
        vec![
            repository.commit_of("HEAD^1"),
            repository.commit_of("HEAD^2")
        ]
    );
    assert_eq!(base.children.len(), 2);
    assert!(base.children.iter().all(|child| child.len() == 64));
    assert_eq!(labels(base), vec![(RefKind::Tag, "v1")]);
    assert_eq!(
        on_current_branch(&rows),
        vec!["Merge", "Side work", "Main work", "Base"]
    );
    assert_eq!(
        repository.git(&["rev-parse", "--show-object-format"]),
        "sha256\n"
    );
}

#[test]
fn the_right_arrow_follows_the_drawn_line_and_not_a_cut_edge() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_long_edge(&sandbox, 40);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_by_pages(&gikli, &root, 7);

    let base = row(&rows, "Base");
    assert_eq!(subjects_of(&rows, &base.children), vec!["Main 2", "Side"]);
    assert_eq!(
        repository.git(&["rev-parse", "main~39^", "side^"]),
        format!("{0}\n{0}\n", base.hash),
        "both are children of Base for git"
    );
}

#[test]
fn a_merge_whose_second_parent_is_far_below_cuts_only_that_edge() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.git(&["switch", "--quiet", "--create", "side"]);
    repository.commit_at(2, "Old side work");
    repository.git(&["switch", "--quiet", "main"]);
    for minute in 3..43 {
        repository.commit_at(minute, &format!("Main {minute}"));
    }
    repository.git_at(
        50,
        &["merge", "--quiet", "--no-ff", "--message", "Merge", "side"],
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let rows = rows_by_pages(&gikli, &root, 7);

    let drawn = drawn(&rows);
    assert_eq!(
        drawn[0],
        (
            "Merge",
            0,
            vec![],
            vec![(0, 0)],
            vec![],
            vec![(1, "Old side work")]
        )
    );
    assert_eq!(
        drawn[41],
        (
            "Old side work",
            1,
            vec![(0, 0)],
            vec![(0, 0), (1, 0)],
            vec![(1, "Merge")],
            vec![]
        )
    );
    assert_eq!(drawn[42], ("Base", 0, vec![(0, 0)], vec![], vec![], vec![]));
    assert_eq!(
        repository.git(&["rev-parse", "HEAD^2"]),
        format!("{}\n", rows[41].hash),
        "git has Old side work as the second parent of Merge"
    );
}
