//! Push: what the push dialog shows will be published, and publishing it, creating the target
//! branch when needed.

use gikli_acceptance::{Sandbox, TestRepository, opened, push_target};
use gikli_core::{
    Event, ExplainedMessage, Explanation, Failure, ListedCommit, LongOperation, NextStep, PushTags,
};

/// A bare remote whose `main` holds First, and a clone `mine` of it.
fn remote_and_clone(sandbox: &Sandbox) -> (TestRepository, TestRepository) {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main"]);
    let mine = sandbox.clone_of(&remote, "mine");
    (remote, mine)
}

/// The subjects of `commits`, in their order.
fn listed_subjects(commits: &[ListedCommit]) -> Vec<&str> {
    commits
        .iter()
        .map(|commit| commit.subject.as_str())
        .collect()
}

/// The hashes of `commits`, in their order.
fn hashes(commits: &[ListedCommit]) -> Vec<String> {
    commits.iter().map(|commit| commit.hash.clone()).collect()
}

/// The hashes of the commits git lists for `revisions` in `repository`, newest first.
fn hashes_by_git(repository: &TestRepository, revisions: &[&str]) -> Vec<String> {
    let mut args = vec!["rev-list"];
    args.extend(revisions);
    args.push("--");
    repository.git(&args).lines().map(str::to_owned).collect()
}

#[test]
fn the_push_dialog_targets_the_upstream_with_the_outgoing_commits() {
    let sandbox = Sandbox::new();
    let (_remote, mine) = remote_and_clone(&sandbox);
    mine.commit_at(2, "Second");
    mine.commit_at(3, "Third");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli.push_plan(&root, None).expect("plan the push");

    assert_eq!(plan.repository, root);
    assert_eq!(plan.branch, "main");
    assert_eq!(plan.remotes, ["origin"]);
    assert_eq!(plan.target, Some(push_target("origin", "main")));
    assert!(!plan.new_branch, "origin/main exists");
    assert_eq!(listed_subjects(&plan.commits), ["Third", "Second"]);
    assert_eq!(
        hashes(&plan.commits),
        hashes_by_git(&mine, &["origin/main..main"]),
        "git lists the same commits"
    );
}

#[test]
fn pushing_sends_the_commits_to_the_target() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    mine.commit_at(2, "Second");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .push(&root, &push_target("origin", "main"), PushTags::None)
        .expect("push");

    assert_eq!(
        remote.git(&["rev-parse", "main"]),
        mine.git(&["rev-parse", "main"]),
        "git agrees the remote has the commits"
    );
    let plan = gikli.push_plan(&root, None).expect("plan the push");
    assert_eq!(plan.commits, [], "nothing is left to push");
}

#[test]
fn a_branch_without_an_upstream_goes_to_a_new_branch_of_the_same_name_with_its_own_commits() {
    let sandbox = Sandbox::new();
    let (_remote, mine) = remote_and_clone(&sandbox);
    mine.commit_at(2, "On main");
    mine.git(&["switch", "--quiet", "--create", "feature"]);
    mine.commit_at(3, "On feature");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli.push_plan(&root, None).expect("plan the push");

    assert_eq!(plan.branch, "feature");
    assert_eq!(plan.target, Some(push_target("origin", "feature")));
    assert!(plan.new_branch, "origin has no feature yet");
    assert_eq!(
        listed_subjects(&plan.commits),
        ["On feature", "On main"],
        "every commit no branch of origin has"
    );
    assert_eq!(
        hashes(&plan.commits),
        hashes_by_git(&mine, &["feature", "--not", "--remotes=origin"]),
        "git lists the same commits"
    );
}

#[test]
fn pushing_to_a_new_branch_creates_it_and_sets_it_as_the_upstream() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    mine.git(&["switch", "--quiet", "--create", "feature"]);
    mine.commit_at(2, "On feature");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .push(&root, &push_target("origin", "feature"), PushTags::None)
        .expect("push");

    assert_eq!(
        remote.git(&["rev-parse", "feature"]),
        mine.git(&["rev-parse", "feature"]),
        "git agrees the remote has the new branch"
    );
    assert_eq!(
        mine.git(&["rev-parse", "--abbrev-ref", "feature@{upstream}"]),
        "origin/feature\n",
        "git agrees the new branch is the upstream"
    );
}

#[test]
fn a_changed_target_shows_the_commits_it_lacks_and_the_push_goes_there_alone() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    mine.commit_at(2, "Second");
    mine.git(&["push", "--quiet", "origin", "main:release"]);
    mine.git(&["reset", "--quiet", "--hard", "HEAD~1"]);
    mine.git(&["fetch", "--quiet"]);
    mine.commit_at(3, "Third");
    let release_before = remote.git(&["rev-parse", "release"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli
        .push_plan(&root, Some(&push_target("origin", "release")))
        .expect("plan the push");
    assert_eq!(plan.target, Some(push_target("origin", "release")));
    assert!(!plan.new_branch, "origin/release exists");
    assert_eq!(
        hashes(&plan.commits),
        hashes_by_git(&mine, &["origin/release..main"]),
        "git lists the same commits"
    );
    assert_eq!(listed_subjects(&plan.commits), ["Third"]);

    let rejected = gikli.push(&root, &push_target("origin", "release"), PushTags::None);
    assert_eq!(
        rejected,
        Err(Failure::PushFailed {
            folder: root.root.clone()
        }),
        "release has a commit main lacks"
    );
    assert_eq!(remote.git(&["rev-parse", "release"]), release_before);
}

#[test]
fn a_target_created_with_another_name_keeps_the_upstream_the_branch_has() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    mine.commit_at(2, "Second");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli
        .push_plan(&root, Some(&push_target("origin", "review/second")))
        .expect("plan the push");
    assert!(plan.new_branch, "origin has no review/second yet");
    assert_eq!(listed_subjects(&plan.commits), ["Second"]);
    gikli
        .push(
            &root,
            &push_target("origin", "review/second"),
            PushTags::None,
        )
        .expect("push");

    assert_eq!(
        remote.git(&["rev-parse", "review/second"]),
        mine.git(&["rev-parse", "main"]),
        "git agrees the remote has the new branch"
    );
    assert_ne!(
        remote.git(&["rev-parse", "main"]),
        mine.git(&["rev-parse", "main"]),
        "main of the remote did not move"
    );
    assert_eq!(
        mine.git(&["rev-parse", "--abbrev-ref", "main@{upstream}"]),
        "origin/main\n",
        "git agrees main still follows origin/main"
    );
}

#[test]
fn a_target_on_a_remote_the_repository_does_not_have_is_refused() {
    let sandbox = Sandbox::new();
    let (_remote, mine) = remote_and_clone(&sandbox);
    let elsewhere = sandbox.bare_remote("elsewhere.git");
    let path = elsewhere.root().to_string_lossy().into_owned();
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let unknown = push_target(&path, "main");

    assert_eq!(
        gikli.push_plan(&root, Some(&unknown)),
        Err(Failure::RemoteNotFound {
            remote: path.clone()
        })
    );
    assert_eq!(
        gikli.push(&root, &unknown, PushTags::None),
        Err(Failure::RemoteNotFound { remote: path })
    );
    assert!(
        !elsewhere.git_succeeds(&["rev-parse", "--verify", "--quiet", "main"]),
        "nothing went to the folder"
    );
}

#[test]
fn a_target_whose_name_is_no_branch_name_is_refused() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    for name in ["@{-1}", "two words", "ends.lock", ""] {
        let invalid = push_target("origin", name);
        let refused = Failure::BranchNameInvalid {
            name: name.to_owned(),
        };
        assert_eq!(
            gikli.push_plan(&root, Some(&invalid)),
            Err(refused.clone()),
            "{name:?}"
        );
        assert_eq!(
            gikli.push(&root, &invalid, PushTags::None),
            Err(refused),
            "{name:?}"
        );
    }
    assert_eq!(
        remote.git(&["for-each-ref", "--format=%(refname)", "refs/heads/"]),
        "refs/heads/main\n",
        "no branch was created"
    );
}

/// A clone with a commit to push, tagged with the annotated `v1` and the lightweight `light`, and
/// the annotated `aside` on a commit of another local branch.
fn tagged(sandbox: &Sandbox) -> (TestRepository, TestRepository) {
    let (remote, mine) = remote_and_clone(sandbox);
    mine.git(&["switch", "--quiet", "--create", "aside"]);
    mine.commit_at(2, "Aside");
    mine.git(&["tag", "--annotate", "--message", "Aside", "aside"]);
    mine.git(&["switch", "--quiet", "main"]);
    mine.commit_at(3, "Second");
    mine.git(&["tag", "--annotate", "--message", "Version 1", "v1"]);
    mine.git(&["tag", "light"]);
    (remote, mine)
}

/// The tags of `remote`, by name, as plain git lists them.
fn tags_by_git(remote: &TestRepository) -> Vec<String> {
    remote
        .git(&["tag", "--list"])
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn pushing_sends_no_tag_unless_asked() {
    let sandbox = Sandbox::new();
    let (remote, mine) = tagged(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    assert_eq!(PushTags::default(), PushTags::None, "off by default");
    gikli
        .push(&root, &push_target("origin", "main"), PushTags::default())
        .expect("push");

    assert_eq!(tags_by_git(&remote), Vec::<String>::new());
}

#[test]
fn pushing_every_tag_sends_them_all() {
    let sandbox = Sandbox::new();
    let (remote, mine) = tagged(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .push(&root, &push_target("origin", "main"), PushTags::All)
        .expect("push");

    assert_eq!(tags_by_git(&remote), ["aside", "light", "v1"]);
}

#[test]
fn pushing_the_tags_of_the_current_branch_sends_its_annotated_tags_only() {
    let sandbox = Sandbox::new();
    let (remote, mine) = tagged(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .push(
            &root,
            &push_target("origin", "main"),
            PushTags::CurrentBranch,
        )
        .expect("push");

    assert_eq!(
        tags_by_git(&remote),
        ["v1"],
        "as --follow-tags: not the lightweight tag, nor one off the branch"
    );
}

#[test]
fn commits_by_other_authors_are_marked() {
    let sandbox = Sandbox::new();
    let (_remote, mine) = remote_and_clone(&sandbox);
    mine.commit_at(2, "Mine");
    mine.git_at(
        3,
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "--author=Someone Else <someone@example.com>",
            "--message=Theirs",
        ],
    );
    mine.git_at(
        4,
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "--author=Test User <TEST@example.com>",
            "--message=Mine again",
        ],
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli.push_plan(&root, None).expect("plan the push");

    let marked: Vec<(&str, bool)> = plan
        .commits
        .iter()
        .map(|commit| (commit.subject.as_str(), commit.by_other_author))
        .collect();
    assert_eq!(
        marked,
        [("Mine again", false), ("Theirs", true), ("Mine", false)],
        "a commit is the user's when its author has their email, in any case"
    );
}

#[test]
fn a_repository_without_a_remote_has_no_target_until_the_user_defines_one() {
    let sandbox = Sandbox::new();
    let mine = sandbox.repository("mine");
    mine.commit_at(1, "First");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli.push_plan(&root, None).expect("plan the push");

    assert_eq!(plan.branch, "main");
    assert_eq!(plan.remotes, Vec::<String>::new());
    assert_eq!(plan.target, None, "there is nowhere to push to");
    assert_eq!(plan.commits, []);
}

#[test]
fn defining_a_remote_adds_it_and_fetches_it_at_once() {
    let sandbox = Sandbox::new();
    let remote = sandbox.bare_remote("remote.git");
    let others = sandbox.repository("others");
    others.commit_at(1, "Theirs");
    others.add_remote("origin", &remote);
    others.git(&["push", "--quiet", "origin", "main:shared"]);
    let mine = sandbox.repository("mine");
    mine.commit_at(2, "Mine");
    let url = remote.root().to_string_lossy().into_owned();
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    gikli
        .add_remote(&root, "upstream", &url)
        .expect("define the remote");

    assert_eq!(
        mine.git(&["remote", "get-url", "upstream"]).trim_end(),
        url,
        "git has the remote"
    );
    assert_eq!(
        mine.git(&["rev-parse", "upstream/shared"]),
        others.git(&["rev-parse", "main"]),
        "git agrees the remote was fetched"
    );
    let plan = gikli.push_plan(&root, None).expect("plan the push");
    assert_eq!(plan.remotes, ["upstream"]);
    assert_eq!(plan.target, Some(push_target("upstream", "main")));
    assert!(plan.new_branch, "the remote has no main");
    assert_eq!(listed_subjects(&plan.commits), ["Mine"]);
}

#[test]
fn a_remote_whose_url_holds_no_repository_is_explained_and_not_kept() {
    let sandbox = Sandbox::new();
    let mine = sandbox.repository("mine");
    mine.commit_at(1, "First");
    let missing = sandbox.folder("missing.git");
    let url = missing.to_string_lossy().into_owned();
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let defined = gikli.add_remote(&root, "origin", &url);

    assert_eq!(
        defined,
        Err(Failure::Explained {
            explanation: Explanation {
                message: ExplainedMessage::RemoteWithoutRepository {
                    remote: Some("origin".to_owned()),
                    url: url.clone(),
                },
                next_step: NextStep::CheckRemoteUrl,
                actions: vec![],
            },
        })
    );
    assert_eq!(mine.git(&["remote"]), "", "git has no remote left");
}

#[test]
fn a_remote_named_like_an_option_is_not_kept_either_when_it_cannot_be_fetched() {
    let sandbox = Sandbox::new();
    let mine = sandbox.repository("mine");
    mine.commit_at(1, "First");
    let missing = sandbox.folder("missing.git");
    let url = missing.to_string_lossy().into_owned();
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let defined = gikli.add_remote(&root, "-f", &url);

    assert!(
        matches!(defined, Err(Failure::Explained { .. })),
        "{defined:?}"
    );
    assert_eq!(mine.git(&["remote"]), "", "git has no remote left");
}

#[test]
fn a_remote_needs_a_free_valid_name_and_a_url() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    let url = remote.root().to_string_lossy().into_owned();
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    assert_eq!(
        gikli.add_remote(&root, "origin", &url),
        Err(Failure::RemoteNameTaken {
            name: "origin".to_owned()
        })
    );
    for name in ["", "two words", "a..b"] {
        assert_eq!(
            gikli.add_remote(&root, name, &url),
            Err(Failure::RemoteNameInvalid {
                name: name.to_owned()
            }),
            "{name:?}"
        );
    }
    assert_eq!(
        gikli.add_remote(&root, "other", "  "),
        Err(Failure::RemoteUrlEmpty)
    );
    assert_eq!(mine.git(&["remote"]), "origin\n", "git has no other remote");
}

#[test]
fn pushing_runs_as_a_long_operation_that_reports_its_progress() {
    let sandbox = Sandbox::new();
    let (_remote, mine) = remote_and_clone(&sandbox);
    for index in 0..3 {
        mine.write(&format!("file-{index}.txt"), &format!("content {index}\n"));
        mine.commit_all(&format!("Commit {index}"));
    }
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let events = gikli.subscribe();

    gikli
        .push(&root, &push_target("origin", "main"), PushTags::None)
        .expect("push");

    let received: Vec<Event> = events.try_iter().collect();
    let (id, operation) = received
        .iter()
        .find_map(|event| match event {
            Event::OperationStarted { id, operation, .. } => Some((*id, operation.clone())),
            _ => None,
        })
        .expect("the push announces it started");
    assert_eq!(
        operation,
        LongOperation::Push {
            repository: root.clone(),
            remote: "origin".to_owned(),
            branch: "main".to_owned(),
        }
    );
    assert!(
        received.iter().any(|event| matches!(
            event,
            Event::OperationProgress { id: reporter, progress } if *reporter == id && progress.percent == 100
        )),
        "the push reports progress up to the end: {received:?}"
    );
    assert!(received.contains(&Event::OperationFinished { id }));
}

#[test]
fn a_detached_head_has_no_branch_to_push() {
    let sandbox = Sandbox::new();
    let (_remote, mine) = remote_and_clone(&sandbox);
    mine.git(&["switch", "--quiet", "--detach"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    assert_eq!(
        gikli.push_plan(&root, None),
        Err(Failure::DetachedHeadNotPushed)
    );
    assert_eq!(
        gikli.push(&root, &push_target("origin", "main"), PushTags::None),
        Err(Failure::DetachedHeadNotPushed)
    );
}

#[test]
fn a_branch_without_commits_has_nothing_to_push() {
    let sandbox = Sandbox::new();
    let remote = sandbox.bare_remote("remote.git");
    let mine = sandbox.repository("mine");
    mine.add_remote("origin", &remote);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let refused = Failure::BranchWithoutCommits {
        branch: "main".to_owned(),
    };
    assert_eq!(gikli.push_plan(&root, None), Err(refused.clone()));
    assert_eq!(
        gikli.push(&root, &push_target("origin", "main"), PushTags::None),
        Err(refused)
    );
}

#[test]
fn a_new_target_on_a_remote_with_its_own_mapping_leaves_out_what_the_remote_holds() {
    let sandbox = Sandbox::new();
    let (_remote, mine) = remote_and_clone(&sandbox);
    mine.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/mirror/*",
    ]);
    mine.git(&["fetch", "--quiet", "origin"]);
    mine.git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    mine.git(&["switch", "--quiet", "--create", "feature"]);
    mine.commit_at(2, "On feature");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);

    let plan = gikli.push_plan(&root, None).expect("plan the push");

    assert!(plan.new_branch, "origin has no feature");
    assert_eq!(
        listed_subjects(&plan.commits),
        ["On feature"],
        "First is on refs/remotes/mirror/main, where origin's branches go"
    );
}
