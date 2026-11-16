//! The log's details and navigation: the user finds a commit, sees what it did and where it lives,
//! and acts on it from the log.

use gikli_acceptance::{ImportedHistory, Sandbox, TestRepository, minute_as_seconds};
use gikli_core::{
    ChangeGrouping, ChangeNode, Failure, Gikli, LOG_SEARCH_BATCH, LogFilter, LogRow, RefKind,
    RefLabel, Repository, TextSearch,
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

#[test]
fn commit_details_show_the_message_hash_author_with_email_and_date() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.git_at(
        2,
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "--author",
            "Ada Lovelace <ada@example.com>",
            "--message",
            "Add the engine\n\nIt computes numbers.",
        ],
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let hash = hash_of(&repository, "HEAD");

    let details = gikli
        .commit_details(&root, &hash)
        .expect("read the details");

    assert_eq!(details.hash, hash);
    assert_eq!(details.parents, vec![hash_of(&repository, "HEAD~1")]);
    assert_eq!(details.author, "Ada Lovelace");
    assert_eq!(details.author_email, "ada@example.com");
    assert_eq!(details.authored_at, minute_as_seconds(2));
    assert_eq!(details.committer, "Test User");
    assert_eq!(details.committed_at, minute_as_seconds(2));
    assert!(details.committer_differs);
    assert_eq!(
        repository.git(&["log", "-1", "--format=%an <%ae> %at|%cn"]),
        format!(
            "Ada Lovelace <ada@example.com> {}|Test User\n",
            minute_as_seconds(2)
        ),
        "git agrees on the author and that someone else committed it"
    );
    assert_eq!(
        details.message_text(),
        repository.git(&["log", "-1", "--format=%B"]).trim_end()
    );
}

#[test]
fn hashes_of_commits_in_a_message_link_to_those_commits() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    let base = hash_of(&repository, "HEAD");
    repository.commit_at(2, "Middle");
    let middle = hash_of(&repository, "HEAD");
    let short_base = &base[..10];
    let message = format!("Revert {middle}.\n\nIt broke {short_base}, unlike deadbeef0 or 12-34.");
    repository.commit_at(3, &message);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let details = gikli
        .commit_details(&root, &hash_of(&repository, "HEAD"))
        .expect("read the details");

    let parts: Vec<(&str, Option<&str>)> = details
        .message
        .iter()
        .map(|part| (part.text.as_str(), part.commit.as_deref()))
        .collect();
    assert_eq!(
        parts,
        vec![
            ("Revert ", None),
            (middle.as_str(), Some(middle.as_str())),
            (".\n\nIt broke ", None),
            (short_base, Some(base.as_str())),
            (", unlike deadbeef0 or 12-34.", None),
        ]
    );
    assert_eq!(details.message_text(), message);
}

#[test]
fn commit_details_list_every_branch_that_contains_the_commit() {
    let sandbox = Sandbox::new();
    let remote = sandbox.bare_remote("remote.git");
    let repository = sandbox.repository("app");
    repository.add_remote("origin", &remote);
    repository.commit_at(1, "Base");
    repository.commit_at(2, "Shared");
    let shared = hash_of(&repository, "HEAD");
    repository.git(&["push", "--quiet", "origin", "main"]);
    repository.git(&["remote", "set-head", "origin", "main"]);
    repository.git(&["branch", "feature"]);
    repository.git(&["branch", "old", "HEAD~1"]);
    repository.git(&["tag", "v1"]);
    repository.commit_at(3, "Later");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let details = gikli
        .commit_details(&root, &shared)
        .expect("read the details");

    let branches: Vec<(&str, RefKind)> = details
        .branches
        .iter()
        .map(|branch| (branch.name.as_str(), branch.kind))
        .collect();
    assert_eq!(
        branches,
        vec![
            ("feature", RefKind::Local),
            ("main", RefKind::Local),
            ("origin/main", RefKind::Remote),
        ]
    );
    let by_git = repository.git(&[
        "branch",
        "--all",
        "--contains",
        &shared,
        "--format=%(refname)",
    ]);
    assert_eq!(
        by_git
            .lines()
            .filter(|name| *name != "refs/remotes/origin/HEAD")
            .collect::<Vec<_>>(),
        vec![
            "refs/heads/feature",
            "refs/heads/main",
            "refs/remotes/origin/main"
        ],
        "git agrees, besides origin/HEAD, which only repeats origin/main"
    );
}

/// The files of `nodes` with their status, in the order the view shows them, each folder as its
/// path followed by `/`.
fn shown(nodes: &[ChangeNode]) -> Vec<String> {
    let mut rows = Vec::new();
    for node in nodes {
        match node {
            ChangeNode::Folder { path, children, .. } => {
                rows.push(format!("{}/", path.display()));
                rows.extend(shown(children).into_iter().map(|row| format!("  {row}")));
            }
            ChangeNode::File(change) => rows.push(format!(
                "{:?} {}{}",
                change.status,
                change.path.display(),
                change
                    .original_path
                    .as_ref()
                    .map(|from| format!(" from {}", from.display()))
                    .unwrap_or_default()
            )),
        }
    }
    rows
}

#[test]
fn a_commits_changed_files_show_as_a_tree_or_a_list() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("src/kept.txt", "kept\n");
    repository.write(
        "src/old name.txt",
        "a file long enough to be found as renamed\n",
    );
    repository.write("gone.txt", "gone\n");
    repository.commit_all("Base");
    repository.write("src/kept.txt", "kept, changed\n");
    repository.delete("src/old name.txt");
    repository.write(
        "src/ñew name.txt",
        "a file long enough to be found as renamed\n",
    );
    repository.delete("gone.txt");
    repository.write("docs/new.md", "new\n");
    repository.commit_all("Change things");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let hash = hash_of(&repository, "HEAD");

    let changes = gikli
        .commit_changes(&root, &hash)
        .expect("read the changes");

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].parent, Some(hash_of(&repository, "HEAD~1")));
    assert_eq!(
        shown(&changes[0].group.nodes),
        vec![
            "docs/",
            "  Added docs/new.md",
            "src/",
            "  Modified src/kept.txt",
            "  Renamed src/ñew name.txt from src/old name.txt",
            "Deleted gone.txt",
        ]
    );
    let by_git = repository.git(&[
        "diff-tree",
        "-r",
        "-M",
        "--name-status",
        "--no-commit-id",
        &hash,
    ]);
    assert_eq!(
        by_git.lines().map(|line| &line[..1]).collect::<Vec<_>>(),
        vec!["A", "D", "M", "R"],
        "git agrees on what changed"
    );

    gikli
        .set_commit_changes_grouping(ChangeGrouping::Flat)
        .expect("show a list");
    let changes = gikli
        .commit_changes(&root, &hash)
        .expect("read the changes");

    assert_eq!(
        shown(&changes[0].group.nodes),
        vec![
            "Added docs/new.md",
            "Deleted gone.txt",
            "Modified src/kept.txt",
            "Renamed src/ñew name.txt from src/old name.txt",
        ]
    );
}

#[test]
fn a_merge_commit_shows_its_changed_files_against_each_parent() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.write("b.txt", "b\n");
    repository.commit_all("Base");
    repository.git(&["switch", "--quiet", "--create", "side"]);
    repository.write("b.txt", "b on side\n");
    repository.commit_all("Change b");
    repository.git(&["switch", "--quiet", "main"]);
    repository.write("a.txt", "a on main\n");
    repository.commit_all("Change a");
    repository.git(&["merge", "--quiet", "--no-edit", "side"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    gikli
        .set_commit_changes_grouping(ChangeGrouping::Flat)
        .expect("show a list");

    let changes = gikli
        .commit_changes(&root, &hash_of(&repository, "HEAD"))
        .expect("read the changes");

    let per_parent: Vec<(Option<String>, Vec<String>)> = changes
        .iter()
        .map(|against| (against.parent.clone(), shown(&against.group.nodes)))
        .collect();
    assert_eq!(
        per_parent,
        vec![
            (
                Some(hash_of(&repository, "HEAD^1")),
                vec!["Modified b.txt".to_owned()]
            ),
            (
                Some(hash_of(&repository, "HEAD^2")),
                vec!["Modified a.txt".to_owned()]
            ),
        ]
    );
    assert_eq!(
        repository.git(&["diff", "--name-only", "HEAD^1", "HEAD"]),
        "b.txt\n"
    );
    assert_eq!(
        repository.git(&["diff", "--name-only", "HEAD^2", "HEAD"]),
        "a.txt\n"
    );
}

#[test]
fn the_first_commit_shows_every_file_as_added() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.write("src/b.txt", "b\n");
    repository.commit_all("First");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    gikli
        .set_commit_changes_grouping(ChangeGrouping::Flat)
        .expect("show a list");

    let changes = gikli
        .commit_changes(&root, "main")
        .expect("read the changes");

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].parent, None);
    assert_eq!(
        shown(&changes[0].group.nodes),
        vec!["Added a.txt", "Added src/b.txt"]
    );
    assert_eq!(
        repository.git(&["ls-tree", "-r", "--name-only", "main"]),
        "a.txt\nsrc/b.txt\n"
    );
}

#[test]
fn the_user_goes_to_a_hash_a_branch_or_a_tag() {
    let sandbox = Sandbox::new();
    let remote = sandbox.bare_remote("remote.git");
    let repository = sandbox.repository("app");
    repository.add_remote("origin", &remote);
    repository.commit_at(1, "Base");
    repository.git(&["tag", "--annotate", "--message", "Version 1", "v1"]);
    repository.git(&["push", "--quiet", "origin", "main"]);
    repository.git(&["switch", "--quiet", "--create", "feature"]);
    repository.commit_at(2, "Feature");
    repository.git(&["switch", "--quiet", "main"]);
    repository.commit_at(3, "Main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let head = hash_of(&repository, "HEAD");

    assert_eq!(
        gikli.go_to(&root, "feature"),
        Ok(hash_of(&repository, "feature"))
    );
    assert_eq!(
        gikli.go_to(&root, "origin/main"),
        Ok(hash_of(&repository, "origin/main"))
    );
    assert_eq!(
        gikli.go_to(&root, "v1"),
        Ok(hash_of(&repository, "v1^{commit}"))
    );
    assert_eq!(gikli.go_to(&root, &head), Ok(head.clone()));
    assert_eq!(
        gikli.go_to(&root, &format!(" {} ", &head[..8])),
        Ok(head.clone())
    );
    assert_ne!(
        hash_of(&repository, "v1"),
        hash_of(&repository, "v1^{commit}"),
        "git agrees the annotated tag is an object of its own"
    );
}

#[test]
fn going_to_what_names_no_commit_says_so() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let unknown = "0123456789abcdef0123456789abcdef01234567";

    for revision in [unknown, "no-such-branch", "--all", "HEAD^{tree}"] {
        assert_eq!(
            gikli.go_to(&root, revision),
            Err(Failure::RevisionNotFound {
                revision: revision.to_owned()
            })
        );
    }
    assert!(
        !repository.git_succeeds(&["cat-file", "-e", unknown]),
        "git agrees there is no such object"
    );
}

#[test]
fn a_sha256_repository_takes_its_64_character_hashes() {
    let sandbox = Sandbox::new();
    let repository = sandbox.existing_repository(&sandbox.folder("app"));
    repository.git(&["init", "--quiet", "--object-format=sha256"]);
    repository.write("a.txt", "a\n");
    repository.commit_all("First");
    repository.commit_at(2, &format!("After {}", hash_of(&repository, "HEAD")));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let first = hash_of(&repository, "HEAD~1");
    assert_eq!(first.len(), 64);

    assert_eq!(gikli.go_to(&root, &first), Ok(first.clone()));
    let details = gikli
        .commit_details(&root, &first)
        .expect("read the details");
    assert_eq!(details.hash, first);
    let changes = gikli
        .commit_changes(&root, &first)
        .expect("read the changes");
    assert_eq!(changes[0].group.changes[0].path.to_str(), Some("a.txt"));
    let later = gikli
        .commit_details(&root, "HEAD")
        .expect("read the details");
    assert_eq!(later.message[1].commit.as_deref(), Some(first.as_str()));
    assert_eq!(
        repository.git(&[
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-only",
            "-r",
            &first
        ]),
        "a.txt\n",
        "git agrees on the changed file"
    );
    assert_eq!(
        repository.git(&["log", "-1", "--format=%s"]),
        format!("After {first}\n"),
        "git agrees the message names the first commit"
    );
}

#[test]
fn going_to_a_branch_or_tag_suggests_the_names_that_match() {
    let sandbox = Sandbox::new();
    let remote = sandbox.bare_remote("remote.git");
    let repository = sandbox.repository("app");
    repository.add_remote("origin", &remote);
    repository.commit_at(1, "Base");
    repository.git(&["branch", "feature/Login"]);
    repository.git(&["branch", "fix-login"]);
    repository.git(&["branch", "other"]);
    repository.git(&["tag", "login-v1"]);
    repository.git(&["push", "--quiet", "origin", "feature/Login"]);
    repository.git(&["remote", "set-head", "origin", "feature/Login"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let suggestions = gikli
        .go_to_suggestions(&root, "LOGIN")
        .expect("suggest names");

    let names: Vec<(&str, RefKind)> = suggestions
        .iter()
        .map(|label| (label.name.as_str(), label.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("login-v1", RefKind::Tag),
            ("feature/Login", RefKind::Local),
            ("fix-login", RefKind::Local),
            ("origin/feature/Login", RefKind::Remote),
        ],
        "the names that start with the text come first"
    );
    let every_name = gikli.go_to_suggestions(&root, "").expect("suggest names");
    assert_eq!(every_name.len(), 6, "{every_name:?}");
    assert!(
        repository
            .git(&["for-each-ref", "--format=%(refname)"])
            .lines()
            .any(|name| name == "refs/remotes/origin/HEAD"),
        "git has origin/HEAD, which only repeats origin/feature/Login"
    );
}

/// The subjects of `rows`, in order.
fn subjects(rows: &[LogRow]) -> Vec<&str> {
    rows.iter().map(|row| row.subject.as_str()).collect()
}

/// The hashes and parents of the commits plain git lists from `revisions`, in the log's order.
fn listed_by_git(repository: &TestRepository, revisions: &[&str]) -> Vec<String> {
    let mut args = vec!["log", "--date-order", "--format=%H %P"];
    args.extend(revisions);
    repository.git(&args).lines().map(str::to_owned).collect()
}

/// The hashes and parents of `rows`, as [`listed_by_git`] has them.
fn listed_by_gikli(rows: &[LogRow]) -> Vec<String> {
    rows.iter()
        .map(|row| format!("{} {}", row.hash, row.parents.join(" ")))
        .collect()
}

/// The log of `repository` with `filter`, read in pages of `count` commits.
fn filtered_rows(
    gikli: &Gikli,
    root: &Repository,
    filter: &LogFilter,
    count: usize,
) -> Vec<LogRow> {
    let mut rows = Vec::new();
    let mut after = None;
    loop {
        let page = gikli
            .read_filtered_log(root, filter, after.as_ref(), count)
            .expect("read the log");
        rows.extend(page.rows);
        match page.next {
            Some(next) => after = Some(next),
            None => return rows,
        }
    }
}

fn branch(name: &str, kind: RefKind) -> RefLabel {
    RefLabel {
        name: name.to_owned(),
        kind,
    }
}

#[test]
fn the_log_filtered_by_branch_shows_only_the_commits_of_those_branches() {
    let sandbox = Sandbox::new();
    let remote = sandbox.bare_remote("remote.git");
    let repository = sandbox.repository("app");
    repository.add_remote("origin", &remote);
    repository.commit_at(1, "Base");
    repository.git(&["switch", "--quiet", "--create", "feature"]);
    repository.commit_at(2, "Feature one");
    repository.commit_at(4, "Feature two");
    repository.git(&["switch", "--quiet", "--create", "published", "main"]);
    repository.commit_at(5, "Published");
    repository.git(&["push", "--quiet", "origin", "published"]);
    repository.git(&["switch", "--quiet", "main"]);
    repository.git(&["branch", "--quiet", "--delete", "--force", "published"]);
    repository.commit_at(3, "Main");
    repository.commit_at(6, "Main again");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    assert_eq!(
        gikli.log_branches(&root).expect("list the branches"),
        vec![
            branch("feature", RefKind::Local),
            branch("main", RefKind::Local),
            branch("origin/published", RefKind::Remote),
        ]
    );
    let filter = LogFilter {
        branches: vec![
            branch("feature", RefKind::Local),
            branch("origin/published", RefKind::Remote),
        ],
        ..LogFilter::default()
    };

    let rows = filtered_rows(&gikli, &root, &filter, 2);

    assert_eq!(
        subjects(&rows),
        vec!["Published", "Feature two", "Feature one", "Base"]
    );
    assert_eq!(
        listed_by_gikli(&rows),
        listed_by_git(
            &repository,
            &["refs/heads/feature", "refs/remotes/origin/published"]
        )
    );
    let base = &rows[3];
    assert!(
        base.on_current_branch,
        "main, the current branch, holds Base"
    );
    assert!(!rows[1].on_current_branch);
    assert_eq!(
        base.children.len(),
        2,
        "its children on the branches shown, not the ones on main"
    );
}

#[test]
fn going_to_a_commit_no_filtered_branch_reaches_says_it_is_not_shown() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.git(&["switch", "--quiet", "--create", "feature"]);
    repository.commit_at(2, "Feature");
    repository.git(&["switch", "--quiet", "main"]);
    repository.commit_at(3, "Main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let filter = LogFilter {
        branches: vec![branch("main", RefKind::Local)],
        ..LogFilter::default()
    };
    let only_gone = LogFilter {
        branches: vec![branch("gone", RefKind::Local)],
        ..LogFilter::default()
    };
    let feature = gikli.go_to(&root, "feature").expect("go to the feature");

    assert_eq!(
        gikli.filtered_log_reaches(&root, &filter, &feature),
        Ok(false)
    );
    assert_eq!(
        gikli.filtered_log_reaches(&root, &only_gone, &feature),
        Ok(false)
    );
    assert!(
        !repository.git_succeeds(&["merge-base", "--is-ancestor", &feature, "main"]),
        "git agrees main does not reach the feature"
    );
    assert!(
        !listed_by_git(&repository, &["refs/heads/main"])
            .iter()
            .any(|listed| listed.starts_with(&feature)),
        "git agrees the log of main does not list the feature"
    );
}

#[test]
fn going_to_a_commit_the_filtered_log_shows_still_goes_to_it() {
    let sandbox = Sandbox::new();
    let remote = sandbox.bare_remote("remote.git");
    let repository = sandbox.repository("app");
    repository.add_remote("origin", &remote);
    repository.commit_at(1, "Base");
    repository.git(&["switch", "--quiet", "--create", "feature"]);
    repository.commit_at(2, "Feature");
    repository.git(&["push", "--quiet", "origin", "feature"]);
    repository.git(&["switch", "--quiet", "main"]);
    repository.commit_at(3, "Main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    // A branch that no longer exists reaches nothing, and the others still count.
    let filter = LogFilter {
        branches: vec![
            branch("gone", RefKind::Local),
            branch("main", RefKind::Local),
            branch("origin/feature", RefKind::Remote),
        ],
        ..LogFilter::default()
    };
    let feature = gikli.go_to(&root, "feature").expect("go to the feature");
    let base = gikli.go_to(&root, "main~1").expect("go to the base");

    assert_eq!(
        gikli.filtered_log_reaches(&root, &filter, &feature),
        Ok(true)
    );
    assert_eq!(gikli.filtered_log_reaches(&root, &filter, &base), Ok(true));
    assert_eq!(
        gikli.filtered_log_reaches(&root, &LogFilter::default(), &feature),
        Ok(true)
    );
    let rows = filtered_rows(&gikli, &root, &filter, 1);
    assert!(rows.iter().any(|row| row.hash == feature));
    assert!(rows.iter().any(|row| row.hash == base));
    let by_git = listed_by_git(
        &repository,
        &["refs/heads/main", "refs/remotes/origin/feature"],
    );
    assert!(
        by_git.iter().any(|listed| listed.starts_with(&feature))
            && by_git.iter().any(|listed| listed.starts_with(&base)),
        "git agrees the log of main and origin/feature lists both"
    );
}

#[test]
fn going_to_a_commit_the_search_does_not_match_says_it_is_not_shown() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base fix");
    repository.git(&["switch", "--quiet", "--create", "feature"]);
    repository.commit_at(2, "Feature work");
    repository.git(&["switch", "--quiet", "main"]);
    repository.commit_at(3, "Main fix");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let whole_log = search("fix");
    let mut of_feature = search("fix");
    of_feature.branches = vec![branch("feature", RefKind::Local)];
    let feature = gikli.go_to(&root, "feature").expect("go to the feature");

    assert_eq!(
        gikli.filtered_log_reaches(&root, &whole_log, &feature),
        Ok(false)
    );
    assert_eq!(
        gikli.filtered_log_reaches(&root, &of_feature, &feature),
        Ok(false)
    );
    assert!(
        gikli.log_search_history().is_empty(),
        "going to a commit runs no search the user keeps"
    );
    assert!(
        !repository
            .git(&["log", "--format=%H", "-i", "--grep=fix", "--all"])
            .contains(&feature),
        "git agrees the search does not find the feature"
    );
    assert!(
        !filtered_rows(&gikli, &root, &of_feature, 1)
            .iter()
            .any(|row| row.hash == feature),
        "the pages of the log do not show it"
    );
}

#[test]
fn going_to_a_commit_tells_a_match_by_the_rules_of_the_search() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.git_at(
        2,
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "--author",
            "Ada Lovelace <ada@example.com>",
            "--message",
            "Fix the parser\n\nThe LEXER was wrong.",
        ],
    );
    repository.commit_at(3, "Later");
    let target = hash_of(&repository, "HEAD~1");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let with = |text: &str, regex: bool, match_case: bool| TextSearch {
        text: text.to_owned(),
        regex,
        match_case,
    };
    let inside_the_hash = &target[5..15];
    let cases = [
        (
            with("lexer", false, false),
            true,
            "the whole message, any case",
        ),
        (
            with("lexer", false, true),
            false,
            "the message in another case",
        ),
        (with("LEXER", false, true), true, "the message in its case"),
        (with("lovelace", false, false), true, "the author's name"),
        (
            with("ada@example.com", false, false),
            false,
            "the author's email",
        ),
        (
            with(&target[..9].to_uppercase(), false, false),
            true,
            "the start of the hash",
        ),
        (
            with(inside_the_hash, false, false),
            false,
            "inside the hash, as text",
        ),
        (
            with(inside_the_hash, true, false),
            true,
            "inside the hash, as a regex",
        ),
        (
            with("par.er", false, false),
            false,
            "a regex that is not one",
        ),
        (
            with(r"^The lex\w+", true, false),
            true,
            "a regex at the start of a line",
        ),
        (
            with("^Fix.*wrong", true, false),
            false,
            "a regex across lines",
        ),
        (with("", false, false), true, "no text"),
    ];

    for (search, expected, what) in cases {
        let main_only = LogFilter {
            branches: vec![branch("main", RefKind::Local)],
            text: Some(search.clone()),
        };
        let reaches = gikli.filtered_log_reaches(&root, &main_only, &target);
        let shown = filtered_rows(&gikli, &root, &main_only, 1)
            .iter()
            .any(|row| row.hash == target);

        assert_eq!(reaches, Ok(expected), "{what}: {search:?}");
        assert_eq!(shown, expected, "the pages agree on {what}: {search:?}");
    }
}

#[test]
fn going_to_a_commit_with_a_search_that_is_no_regular_expression_fails_as_the_search_does() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let mut filter = searching(TextSearch {
        text: "fix(".to_owned(),
        regex: true,
        match_case: false,
    });
    let base = gikli.go_to(&root, "main").expect("go to the base");

    let read = gikli.read_filtered_log(&root, &filter, None, 50);
    let reaches = gikli.filtered_log_reaches(&root, &filter, &base);
    filter.branches = vec![branch("main", RefKind::Local)];
    let reaches_on_main = gikli.filtered_log_reaches(&root, &filter, &base);

    assert!(
        matches!(read, Err(Failure::SearchPatternInvalid { .. })),
        "{read:?}"
    );
    assert_eq!(reaches, read.clone().map(|_| true));
    assert_eq!(reaches_on_main, read.map(|_| true));
}

/// A search of `text` with the regex and match-case options off.
fn plain(text: &str) -> TextSearch {
    TextSearch {
        text: text.to_owned(),
        ..TextSearch::default()
    }
}

/// The log filtered by `search` alone.
fn searching(search: TextSearch) -> LogFilter {
    LogFilter {
        text: Some(search),
        ..LogFilter::default()
    }
}

/// The log filtered by a plain search of `text` alone.
fn search(text: &str) -> LogFilter {
    searching(plain(text))
}

#[test]
fn text_search_matches_the_message_the_author_or_the_hash() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.commit_at(2, "Fix the parser\n\nThe LEXER was wrong.");
    repository.git_at(
        3,
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "--author",
            "Lex Luthor <lex@example.com>",
            "--message",
            "Unrelated",
        ],
    );
    repository.commit_at(4, "Not this one");
    repository.commit_at(5, "Nor this");
    let target = hash_of(&repository, "HEAD~1");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let by_text = filtered_rows(&gikli, &root, &search("lex"), 1);
    let by_hash = filtered_rows(&gikli, &root, &search(&target[..9].to_uppercase()), 50);

    assert_eq!(subjects(&by_text), vec!["Unrelated", "Fix the parser"]);
    assert!(by_text.iter().all(|row| row.on_current_branch));
    assert!(
        by_text.iter().all(|row| row.graph.column == 0
            && row.graph.above.is_empty()
            && row.graph.below.is_empty()),
        "a search shows the commits without their graph"
    );
    assert_eq!(subjects(&by_hash), vec!["Not this one"]);
    assert_eq!(
        repository.git(&["log", "--format=%s", "-i", "--grep=lex"]),
        "Fix the parser\n",
        "git finds the message"
    );
    assert_eq!(
        repository.git(&["log", "--format=%s", "-i", "--author=lex"]),
        "Unrelated\n",
        "git finds the author"
    );
}

#[test]
fn text_search_can_match_case_and_take_a_regular_expression() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Fix issue 12");
    repository.commit_at(2, "fix the build");
    repository.commit_at(3, "Prefix things");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let with = |text: &str, regex: bool, match_case: bool| LogFilter {
        text: Some(TextSearch {
            text: text.to_owned(),
            regex,
            match_case,
        }),
        ..LogFilter::default()
    };

    let any_case = filtered_rows(&gikli, &root, &with("Fix", false, false), 50);
    let same_case = filtered_rows(&gikli, &root, &with("Fix", false, true), 50);
    let regex = filtered_rows(&gikli, &root, &with(r"^fix\b", true, false), 50);
    let regex_same_case = filtered_rows(&gikli, &root, &with(r"issue \d+$", true, true), 50);
    let not_regex = filtered_rows(&gikli, &root, &with(r"issue \d+", false, false), 50);

    assert_eq!(
        subjects(&any_case),
        vec!["Prefix things", "fix the build", "Fix issue 12"]
    );
    assert_eq!(subjects(&same_case), vec!["Fix issue 12"]);
    assert_eq!(subjects(&regex), vec!["fix the build", "Fix issue 12"]);
    assert_eq!(subjects(&regex_same_case), vec!["Fix issue 12"]);
    assert!(not_regex.is_empty());
    // `\b` is a GNU extension that the regular expressions of macOS lack, so git is asked the
    // same in plain POSIX.
    assert_eq!(
        repository.git(&[
            "log",
            "--format=%s",
            "-E",
            "-i",
            "--grep=^fix([^[:alnum:]_]|$)"
        ]),
        "fix the build\nFix issue 12\n",
        "git finds the same"
    );
}

#[test]
fn a_search_that_is_no_regular_expression_says_why() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let filter = LogFilter {
        text: Some(TextSearch {
            text: "fix(".to_owned(),
            regex: true,
            match_case: false,
        }),
        ..LogFilter::default()
    };

    let read = gikli.read_filtered_log(&root, &filter, None, 50);

    assert!(
        matches!(read, Err(Failure::SearchPatternInvalid { ref message }) if !message.is_empty()),
        "{read:?}"
    );
    assert!(
        gikli.log_search_history().is_empty(),
        "a search that failed is not kept"
    );
    assert!(
        !repository.git_succeeds(&["log", "-E", "--grep=fix("]),
        "git does not take it either"
    );
}

#[test]
fn the_searches_the_user_ran_are_kept_newest_first() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base");
    repository.commit_at(2, "Parser");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    for text in ["parser", "lex", "", "parser"] {
        filtered_rows(&gikli, &root, &search(text), 1);
    }
    for number in 3..25 {
        filtered_rows(&gikli, &root, &search(&format!("search {number}")), 1);
    }

    let history = gikli.log_search_history();
    assert_eq!(history.len(), 20);
    assert_eq!(history[0], plain("search 24"));
    assert_eq!(history[19], plain("search 5"));
    let again = sandbox.gikli();
    assert_eq!(
        again.log_search_history(),
        history,
        "kept for the next start"
    );

    filtered_rows(&again, &root, &search("lex"), 1);
    assert_eq!(again.log_search_history()[0], plain("lex"));
    assert_eq!(again.log_search_history().len(), 20);
}

#[test]
fn a_search_in_the_history_keeps_its_regex_and_match_case_options() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Fix the parser");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let ran = TextSearch {
        regex: true,
        match_case: true,
        ..plain("^Fix")
    };

    filtered_rows(&gikli, &root, &searching(ran.clone()), 1);

    assert_eq!(gikli.log_search_history(), vec![ran.clone()]);
    assert_eq!(
        sandbox.gikli().log_search_history(),
        vec![ran],
        "kept with its options for the next start"
    );
}

#[test]
fn the_same_text_with_other_options_is_another_search_in_the_history() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Fix the parser");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let regex = TextSearch {
        regex: true,
        ..plain("fix")
    };
    let match_case = TextSearch {
        match_case: true,
        ..plain("fix")
    };

    for ran in [
        plain("fix"),
        regex.clone(),
        match_case.clone(),
        plain("fix"),
    ] {
        filtered_rows(&gikli, &root, &searching(ran), 1);
    }

    assert_eq!(
        gikli.log_search_history(),
        vec![plain("fix"), match_case, regex]
    );
}

#[test]
fn a_history_of_search_texts_only_loads_with_both_options_off() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Fix the parser");
    let config = sandbox.config_folder();
    std::fs::create_dir_all(&config).expect("create the config folder");
    std::fs::write(config.join("log-searches.json"), r#"["parser", "lex"]"#)
        .expect("write the history an earlier gikli saved");

    let gikli = sandbox.gikli();

    assert_eq!(
        gikli.log_search_history(),
        vec![plain("parser"), plain("lex")]
    );

    let regex = TextSearch {
        regex: true,
        ..plain("fix")
    };
    filtered_rows(
        &gikli,
        &opened(&gikli, &repository),
        &searching(regex.clone()),
        1,
    );
    assert_eq!(
        sandbox.gikli().log_search_history(),
        vec![regex, plain("parser"), plain("lex")],
        "kept again with the options for the next start"
    );
}

#[test]
fn a_search_keeps_to_the_branches_the_log_is_filtered_by() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.commit_at(1, "Base work");
    repository.git(&["switch", "--quiet", "--create", "feature"]);
    repository.commit_at(2, "Feature work");
    repository.git(&["switch", "--quiet", "main"]);
    repository.commit_at(3, "Main work");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let mut filter = search("work");
    filter.branches = vec![branch("feature", RefKind::Local)];

    let rows = filtered_rows(&gikli, &root, &filter, 50);

    assert_eq!(subjects(&rows), vec!["Feature work", "Base work"]);
    assert_eq!(
        repository.git(&["log", "--format=%s", "--grep=work", "feature"]),
        "Feature work\nBase work\n"
    );

    repository.git(&["branch", "--quiet", "--delete", "--force", "feature"]);
    let gone = gikli
        .read_filtered_log(&root, &filter, None, 50)
        .expect("read the log");
    assert!(gone.rows.is_empty(), "a branch that is gone shows nothing");
    assert_eq!(gone.next, None);
}

#[test]
fn a_search_that_finds_little_comes_in_short_pages_so_the_log_stays_responsive() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let mut history = ImportedHistory::default();
    let mut previous = history.commit("main", &[]);
    while history.commits() < LOG_SEARCH_BATCH + 5 {
        previous = history.commit("main", &[previous]);
    }
    repository.fast_import(&history);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let filter = LogFilter {
        text: Some(TextSearch {
            text: "^Commit 1 on main$".to_owned(),
            regex: true,
            match_case: true,
        }),
        ..LogFilter::default()
    };

    let first = gikli
        .read_filtered_log(&root, &filter, None, 50)
        .expect("read the log");
    let rows = filtered_rows(&gikli, &root, &filter, 50);

    assert!(
        first.rows.is_empty(),
        "the first page went through no match"
    );
    assert!(first.next.is_some(), "the search goes on");
    assert_eq!(subjects(&rows), vec!["Commit 1 on main"]);
    assert_eq!(
        repository.git(&[
            "log",
            "--format=%s",
            "-E",
            "--grep=^Commit 1 on main$",
            "main"
        ]),
        "Commit 1 on main\n"
    );
}
