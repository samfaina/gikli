//! Going to a commit stays quick on a Repository with hundreds of thousands of commits, when the
//! log is filtered by a branch that does not reach it, or searched for a text it does not match.
//!
//! The test reports its timings and checks the answer. It checks the budget only when
//! `GIKLI_CHECK_BUDGETS` is set, as `npm run test:performance` sets it, since a loaded machine or a
//! shared CI runner misses it for changes that never touched the log. This is the only test in its
//! file, so no other test of the suite runs beside it and slows what it measures.

use std::time::{Duration, Instant};

use gikli_acceptance::{ImportedHistory, Sandbox, checks_budgets};
use gikli_core::{Gikli, LogFilter, RefKind, RefLabel, Repository, TextSearch};

/// The commits of the generated history.
const COMMITS: usize = 300_000;
/// The budget of going to a commit the filtered log does not show, from the revision the user
/// typed to the answer that the log does not show it.
const NOT_SHOWN_BUDGET: Duration = Duration::from_secs(1);

/// Goes to `main` in the log of `root` filtered by `filter`, which does not show it, as `what`
/// tells, and checks the answer and, when the budgets are checked, its budget.
fn goes_to_main_not_shown(gikli: &Gikli, root: &Repository, filter: &LogFilter, what: &str) {
    let started = Instant::now();
    let hash = gikli.go_to(root, "main").expect("go to main");
    let reaches = gikli
        .filtered_log_reaches(root, filter, &hash)
        .expect("ask whether the log reaches it");
    let took = started.elapsed();

    assert!(!reaches, "the log {what} does not show main");
    eprintln!(
        "going to a commit the log of {COMMITS} commits {what} does not show: answered in \
         {took:?} (budget {NOT_SHOWN_BUDGET:?})"
    );
    if checks_budgets() {
        assert!(took < NOT_SHOWN_BUDGET, "the answer took {took:?}");
    } else {
        eprintln!("skipped: the budget, as GIKLI_CHECK_BUDGETS is not set");
    }
}

#[test]
fn going_to_a_commit_the_filtered_log_does_not_show_answers_within_its_budget() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("large");
    repository.fast_import(&ImportedHistory::generated(COMMITS));
    repository.git(&["checkout", "--quiet", "main"]);
    let counted = repository.git(&["rev-list", "--count", "--all"]);
    assert_eq!(
        counted.trim_end(),
        COMMITS.to_string(),
        "git counts the history"
    );
    let gikli = sandbox.gikli();
    let root = gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone();
    let by_branch = LogFilter {
        branches: vec![RefLabel {
            name: "release".to_owned(),
            kind: RefKind::Local,
        }],
        ..LogFilter::default()
    };
    let by_search = LogFilter {
        text: Some(TextSearch {
            text: "on release".to_owned(),
            ..TextSearch::default()
        }),
        ..LogFilter::default()
    };
    assert!(
        !repository.git_succeeds(&["merge-base", "--is-ancestor", "main", "release"]),
        "git agrees the release line does not reach the tip of main"
    );
    assert!(
        !repository
            .git(&["log", "-1", "--format=%an%n%B", "main"])
            .to_lowercase()
            .contains("on release"),
        "git agrees the tip of main does not hold the text"
    );

    goes_to_main_not_shown(&gikli, &root, &by_branch, "filtered by a branch");
    goes_to_main_not_shown(&gikli, &root, &by_search, "searched for a text");
}
