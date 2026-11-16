//! The branch selector and the branches pane of the log: every branch of a Repository, grouped
//! and searched.

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    Branch, BranchNode, Branches, CurrentBranch, Failure, Gikli, IncomingOutgoing, LogFilter,
    RefKind, RefLabel, Repository, Upstream,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

fn label(name: &str, kind: RefKind) -> RefLabel {
    RefLabel {
        name: name.to_owned(),
        kind,
    }
}

/// The branch `name` of `kind`, without an upstream.
fn branch(name: &str, kind: RefKind) -> BranchNode {
    BranchNode::Branch(Branch {
        label: label(name, kind),
        upstream: None,
    })
}

fn on_branch(name: &str) -> CurrentBranch {
    CurrentBranch::Branch {
        name: name.to_owned(),
        upstream: None,
    }
}

/// A bare remote `origin` of `work`, with `main` pushed to it.
fn with_origin(sandbox: &Sandbox, work: &TestRepository) {
    let remote = sandbox.bare_remote("remote.git");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main"]);
}

#[test]
fn the_selector_shows_the_current_branch_then_local_and_remote_branches_and_tags() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    with_origin(&sandbox, &work);
    work.git(&["branch", "release"]);
    work.git(&["branch", "fix"]);
    work.git(&["tag", "v1"]);
    work.git(&["tag", "--annotate", "--message", "Second", "v2"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let branches = gikli.branches(&root, "").expect("list the branches");

    assert_eq!(
        branches,
        Branches {
            repository: root.clone(),
            current: on_branch("main"),
            recent: vec![],
            local: vec![
                branch("fix", RefKind::Local),
                branch("main", RefKind::Local),
                branch("release", RefKind::Local),
            ],
            remote: vec![directory(
                "origin",
                vec![branch("origin/main", RefKind::Remote)]
            )],
            tags: vec![branch("v1", RefKind::Tag), branch("v2", RefKind::Tag)],
        }
    );
}

/// The upstream `name` of a branch, with `incoming` commits the branch does not have and
/// `outgoing` ones it does not have.
fn upstream(name: &str, incoming: usize, outgoing: usize) -> Option<Upstream> {
    Some(Upstream {
        name: name.to_owned(),
        commits: Some(IncomingOutgoing { incoming, outgoing }),
    })
}

fn local_branch(name: &str, upstream: Option<Upstream>) -> BranchNode {
    BranchNode::Branch(Branch {
        label: label(name, RefKind::Local),
        upstream,
    })
}

#[test]
fn local_branches_show_their_incoming_and_outgoing_commits() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    with_origin(&sandbox, &work);
    work.git(&["branch", "--set-upstream-to", "origin/main"]);
    work.git(&["branch", "behind", "--track", "origin/main"]);
    work.git(&["branch", "gone", "--track", "origin/main"]);
    work.git(&["config", "branch.gone.merge", "refs/heads/deleted"]);
    work.git(&["branch", "alone"]);
    work.commit_at(2, "Second");
    work.git(&["push", "--quiet", "origin", "main"]);
    work.commit_at(3, "Third");
    work.git(&["branch", "--force", "behind", "HEAD~2"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let branches = gikli.branches(&root, "").expect("list the branches");

    assert_eq!(
        branches.local,
        vec![
            local_branch("alone", None),
            local_branch("behind", upstream("origin/main", 1, 0)),
            local_branch(
                "gone",
                Some(Upstream {
                    name: "origin/deleted".to_owned(),
                    commits: None,
                })
            ),
            local_branch("main", upstream("origin/main", 0, 1)),
        ]
    );
    assert_eq!(
        branches.current,
        CurrentBranch::Branch {
            name: "main".to_owned(),
            upstream: upstream("origin/main", 0, 1),
        }
    );
    assert_eq!(
        branches.remote,
        vec![directory(
            "origin",
            vec![branch("origin/main", RefKind::Remote)]
        )]
    );
}

fn recent(names: &[&str]) -> Vec<Branch> {
    names
        .iter()
        .map(|name| Branch {
            label: label(name, RefKind::Local),
            upstream: None,
        })
        .collect()
}

/// Recent holds the local branches the reflog of HEAD last checked out, other than the current
/// one, each once, newest first: neither a branch since deleted nor a detached HEAD.
#[test]
fn recent_holds_the_last_five_branches_checked_out() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    for name in ["one", "two", "three", "four", "five", "six", "seven"] {
        work.git(&["branch", name]);
    }
    for name in ["seven", "one", "two", "three", "four", "five", "six"] {
        work.git(&["checkout", "--quiet", name]);
    }
    work.git(&["checkout", "--quiet", "--detach"]);
    for name in ["main", "two", "main"] {
        work.git(&["checkout", "--quiet", name]);
    }
    work.git(&["branch", "--quiet", "--delete", "five"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let branches = gikli.branches(&root, "").expect("list the branches");

    assert_eq!(
        branches.recent,
        recent(&["two", "six", "four", "three", "one"])
    );
}

#[test]
fn a_repository_without_a_reflog_has_no_recent_branches() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.git(&["config", "core.logAllRefUpdates", "false"]);
    work.commit_at(1, "First");
    work.git(&["branch", "other"]);
    work.git(&["checkout", "--quiet", "other"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let branches = gikli.branches(&root, "").expect("list the branches");

    assert_eq!(branches.recent, vec![]);
    assert_eq!(branches.current, on_branch("other"));
}

/// HEAD on a branch with no commit yet has no reflog git can read, so Recent is empty.
#[test]
fn a_repository_without_commits_lists_its_current_branch_only() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let branches = gikli.branches(&root, "").expect("list the branches");

    assert_eq!(
        branches,
        Branches {
            repository: root.clone(),
            current: on_branch("main"),
            recent: vec![],
            local: vec![],
            remote: vec![],
            tags: vec![],
        }
    );
}

fn directory(path: &str, children: Vec<BranchNode>) -> BranchNode {
    BranchNode::Directory {
        path: path.to_owned(),
        children,
    }
}

/// A repository on `main` with branches and tags in directories, and remote branches of `origin`.
fn repository_with_directories(sandbox: &Sandbox) -> (TestRepository, Gikli, Repository) {
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    with_origin(sandbox, &work);
    for name in ["feature/search", "feature/login", "feature/ui/menu", "fix"] {
        work.git(&["branch", name]);
    }
    work.git(&["push", "--quiet", "origin", "feature/login"]);
    work.git(&["tag", "release/v1"]);
    work.git(&["tag", "v2"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    (work, gikli, root)
}

/// Each directory comes before the branches beside it, each in order of name, as in a file
/// browser. Remote branches are in the directory of their remote.
#[test]
fn branches_are_grouped_by_directory_by_default() {
    let sandbox = Sandbox::new();
    let (_work, gikli, root) = repository_with_directories(&sandbox);

    let branches = gikli.branches(&root, "").expect("list the branches");

    assert!(gikli.settings().group_branches_by_directory);
    assert_eq!(
        branches.local,
        vec![
            directory(
                "feature",
                vec![
                    directory(
                        "feature/ui",
                        vec![branch("feature/ui/menu", RefKind::Local)]
                    ),
                    branch("feature/login", RefKind::Local),
                    branch("feature/search", RefKind::Local),
                ]
            ),
            branch("fix", RefKind::Local),
            branch("main", RefKind::Local),
        ]
    );
    assert_eq!(
        branches.remote,
        vec![directory(
            "origin",
            vec![
                directory(
                    "origin/feature",
                    vec![branch("origin/feature/login", RefKind::Remote)]
                ),
                branch("origin/main", RefKind::Remote),
            ]
        )]
    );
    assert_eq!(
        branches.tags,
        vec![
            directory("release", vec![branch("release/v1", RefKind::Tag)]),
            branch("v2", RefKind::Tag),
        ]
    );
}

/// The local branches, remote branches or tags under `prefix`, such as `refs/heads/`, by whole
/// name, as plain git lists them.
fn listed_by_git(repository: &TestRepository, prefix: &str) -> Vec<String> {
    repository
        .git(&["for-each-ref", "--format=%(refname)", prefix])
        .lines()
        .filter(|name| !name.ends_with("/HEAD"))
        .map(|name| name[prefix.len()..].to_owned())
        .collect()
}

/// The whole names of `nodes`, as a flat list holds them.
fn names(nodes: &[BranchNode]) -> Vec<String> {
    listed(nodes).into_iter().map(|label| label.name).collect()
}

#[test]
fn branches_can_be_listed_without_their_directories() {
    let sandbox = Sandbox::new();
    let (work, gikli, root) = repository_with_directories(&sandbox);

    let settings = gikli
        .set_group_branches_by_directory(false)
        .expect("save the setting");
    let branches = gikli.branches(&root, "").expect("list the branches");

    assert!(!settings.group_branches_by_directory);
    assert_eq!(
        branches.local,
        vec![
            branch("feature/login", RefKind::Local),
            branch("feature/search", RefKind::Local),
            branch("feature/ui/menu", RefKind::Local),
            branch("fix", RefKind::Local),
            branch("main", RefKind::Local),
        ]
    );
    assert_eq!(
        branches.remote,
        vec![
            branch("origin/feature/login", RefKind::Remote),
            branch("origin/main", RefKind::Remote),
        ]
    );
    assert_eq!(names(&branches.local), listed_by_git(&work, "refs/heads/"));
    assert_eq!(
        names(&branches.remote),
        listed_by_git(&work, "refs/remotes/")
    );
    assert_eq!(names(&branches.tags), listed_by_git(&work, "refs/tags/"));
    assert!(
        !sandbox.gikli().settings().group_branches_by_directory,
        "kept for the next start"
    );
}

#[test]
fn the_selector_shows_recent_branches_and_tags_unless_the_user_hides_them() {
    let sandbox = Sandbox::new();
    let (work, gikli, root) = repository_with_directories(&sandbox);
    work.git(&["checkout", "--quiet", "fix"]);
    work.git(&["checkout", "--quiet", "main"]);
    let settings = gikli.settings();
    assert!(settings.show_recent_branches);
    assert!(settings.show_tags_in_branch_selector);
    let shown = gikli.branches(&root, "").expect("list the branches");
    assert_eq!(shown.recent, recent(&["fix"]));
    assert_eq!(shown.tags.len(), 2);

    gikli
        .set_show_recent_branches(false)
        .expect("save the setting");
    let settings = gikli
        .set_show_tags_in_branch_selector(false)
        .expect("save the setting");
    let hidden = gikli.branches(&root, "").expect("list the branches");

    assert!(!settings.show_recent_branches);
    assert!(!settings.show_tags_in_branch_selector);
    assert_eq!(hidden.recent, vec![]);
    assert_eq!(hidden.tags, vec![]);
    assert_eq!(hidden.local, shown.local, "the other groups stay");
    let restarted = sandbox.gikli().settings();
    assert!(!restarted.show_recent_branches, "kept for the next start");
    assert!(
        !restarted.show_tags_in_branch_selector,
        "kept for the next start"
    );
}

/// A search keeps, in every group, the branches and tags whose whole name holds what the user
/// typed, ignoring case, and the directories that still hold one.
#[test]
fn a_search_finds_branches_and_tags_in_every_group() {
    let sandbox = Sandbox::new();
    let (work, gikli, root) = repository_with_directories(&sandbox);
    work.git(&["tag", "login-done"]);
    work.git(&["checkout", "--quiet", "feature/login"]);
    work.git(&["checkout", "--quiet", "fix"]);
    work.git(&["checkout", "--quiet", "main"]);

    let found = gikli
        .branches(&root, " LOGIN ")
        .expect("search the branches");

    assert_eq!(found.current, on_branch("main"), "the current branch stays");
    assert_eq!(found.recent, recent(&["feature/login"]));
    assert_eq!(
        found.local,
        vec![directory(
            "feature",
            vec![branch("feature/login", RefKind::Local)]
        )]
    );
    assert_eq!(
        found.remote,
        vec![directory(
            "origin",
            vec![directory(
                "origin/feature",
                vec![branch("origin/feature/login", RefKind::Remote)]
            )]
        )]
    );
    assert_eq!(found.tags, vec![branch("login-done", RefKind::Tag)]);

    let across = gikli
        .branches(&root, "re/l")
        .expect("search across a directory");
    assert_eq!(
        across.local,
        vec![directory(
            "feature",
            vec![branch("feature/login", RefKind::Local)]
        )]
    );
    assert_eq!(across.tags, vec![], "release/v1 holds no re/l");

    let nothing = gikli
        .branches(&root, "nothing")
        .expect("search the branches");
    assert_eq!(
        (nothing.recent, nothing.local, nothing.remote, nothing.tags),
        (vec![], vec![], vec![], vec![])
    );
}

/// Every branch and tag the lists hold, depth first.
fn listed(nodes: &[BranchNode]) -> Vec<RefLabel> {
    nodes
        .iter()
        .flat_map(|node| match node {
            BranchNode::Directory { children, .. } => listed(children),
            BranchNode::Branch(branch) => vec![branch.label.clone()],
        })
        .collect()
}

/// Going to a branch of the branches pane goes to its latest commit, even when a tag or a branch
/// of another kind has the same name, and to the commit an annotated tag points at.
#[test]
fn the_branches_pane_goes_to_the_latest_commit_of_a_branch() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    with_origin(&sandbox, &work);
    work.git(&["tag", "--annotate", "--message", "Annotated", "feature"]);
    work.git(&["checkout", "--quiet", "-b", "feature"]);
    work.commit_at(2, "On feature");
    work.git(&["branch", "origin/main", "refs/heads/feature"]);
    work.git(&["checkout", "--quiet", "main"]);
    work.commit_at(3, "On main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let branches = gikli.branches(&root, "").expect("list the branches");
    let all: Vec<RefLabel> = [branches.local, branches.remote, branches.tags]
        .iter()
        .flat_map(|group| listed(group))
        .collect();

    let latest: Vec<(String, String)> = all
        .iter()
        .map(|label| {
            let hash = gikli.go_to_branch(&root, label).expect("go to the branch");
            (label.name.clone(), hash)
        })
        .collect();

    let commit_of = |revision: &str| work.git(&["rev-parse", revision]).trim().to_owned();
    assert_eq!(
        latest,
        vec![
            (
                "origin/main".to_owned(),
                commit_of("refs/heads/origin/main")
            ),
            ("feature".to_owned(), commit_of("refs/heads/feature")),
            ("main".to_owned(), commit_of("refs/heads/main")),
            (
                "origin/main".to_owned(),
                commit_of("refs/remotes/origin/main")
            ),
            (
                "feature".to_owned(),
                commit_of("refs/tags/feature^{commit}")
            ),
        ]
    );
}

#[test]
fn going_to_a_branch_deleted_since_says_so() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "old"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    work.git(&["branch", "--quiet", "--delete", "old"]);

    assert_eq!(
        gikli.go_to_branch(&root, &label("old", RefKind::Local)),
        Err(Failure::RevisionNotFound {
            revision: "old".to_owned()
        })
    );
}

/// The branches pane filters the log by the branch the user picks.
#[test]
fn the_branches_pane_filters_the_log_by_a_branch() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "Base");
    work.git(&["checkout", "--quiet", "-b", "feature/login"]);
    work.commit_at(2, "On feature");
    work.git(&["checkout", "--quiet", "main"]);
    work.commit_at(3, "On main");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let branches = gikli.branches(&root, "").expect("list the branches");
    let picked = listed(&branches.local)
        .into_iter()
        .find(|label| label.name == "feature/login")
        .expect("feature/login is listed");

    let filter = LogFilter {
        branches: vec![picked],
        text: None,
    };
    let page = gikli
        .read_filtered_log(&root, &filter, None, 10)
        .expect("read the log");

    let subjects: Vec<&str> = page.rows.iter().map(|row| row.subject.as_str()).collect();
    assert_eq!(subjects, vec!["On feature", "Base"]);
}
