//! The log stays responsive on a Repository with hundreds of thousands of commits.
//!
//! The budgets hold on the development machine, where the test checks them, along with the other
//! timings. A CI runner is slower and shared, so there the test only reports what it measured, and
//! still checks what the pages show. This is the only test in its file, so no other test of the suite runs beside it and slows
//! what it measures.

use std::time::{Duration, Instant};

use gikli_acceptance::{ImportedHistory, Sandbox, TestRepository, on_ci};
use gikli_core::{Gikli, LogPage, Repository};

/// The commits of the generated history.
const COMMITS: usize = 300_000;
/// The budget of the first page, from the read to its arrival through the application interface.
const FIRST_PAGE_BUDGET: Duration = Duration::from_secs(1);
/// The budget of each later page.
const LATER_PAGE_BUDGET: Duration = Duration::from_millis(200);
/// The commits of a page, as many as the log view asks for.
const PAGE_SIZE: usize = 200;
/// The later pages measured.
const LATER_PAGES: usize = 25;

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// Reads a page of the log through the application interface, and how long it took to arrive.
fn timed_read(
    gikli: &Gikli,
    root: &Repository,
    after: Option<&gikli_core::LogCursor>,
) -> (LogPage, Duration) {
    let started = Instant::now();
    let page = gikli
        .read_log(root, after, PAGE_SIZE)
        .expect("read the log");
    (page, started.elapsed())
}

#[test]
fn the_log_of_several_hundred_thousand_commits_reads_each_page_within_its_budget() {
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
    let root = opened(&gikli, &repository);

    let (first, first_took) = timed_read(&gikli, &root, None);
    let mut rows = first.rows;
    let mut next = first.next;
    let mut later_took = Vec::new();
    for _ in 0..LATER_PAGES {
        let (page, took) = timed_read(&gikli, &root, next.as_ref());
        rows.extend(page.rows);
        next = page.next;
        later_took.push(took);
    }

    let wanted = PAGE_SIZE * (1 + LATER_PAGES);
    let max_count = format!("--max-count={wanted}");
    let by_git = repository.git(&[
        "log",
        "--date-order",
        "--format=%H",
        &max_count,
        "--branches",
        "--remotes",
        "--tags",
        "HEAD",
    ]);
    let by_gikli: Vec<&str> = rows.iter().map(|row| row.hash.as_str()).collect();
    assert_eq!(by_gikli, by_git.lines().collect::<Vec<_>>());
    assert!(next.is_some(), "more pages come after those read");

    let slowest_later = later_took.iter().max().copied().unwrap_or_default();
    eprintln!(
        "log of {COMMITS} commits: first page in {first_took:?} (budget {FIRST_PAGE_BUDGET:?}), \
         slowest of {LATER_PAGES} later pages in {slowest_later:?} (budget {LATER_PAGE_BUDGET:?})"
    );
    if !on_ci() {
        assert!(
            first_took < FIRST_PAGE_BUDGET,
            "the first page took {first_took:?}"
        );
        assert!(
            slowest_later < LATER_PAGE_BUDGET,
            "a later page took {slowest_later:?}: {later_took:?}"
        );
    }

    // The rest of the application interface goes on answering while the log reads.
    let (changes_came_while_reading, page) = std::thread::scope(|scope| {
        let reading = scope.spawn(|| gikli.read_log(&root, None, PAGE_SIZE));
        let changes = gikli.read_changes(&root).expect("read the changes");
        let came_while_reading = !reading.is_finished();
        assert!(
            changes.changes.is_empty(),
            "nothing changed in the working tree"
        );
        (came_while_reading, reading.join().expect("the read ended"))
    });
    assert_eq!(page.expect("read the log").rows.len(), PAGE_SIZE);
    // A read of the changes takes a fraction of the first page here, a margin a runner shared with
    // other jobs need not keep.
    if !on_ci() {
        assert!(
            changes_came_while_reading,
            "the changes came only once the log was read"
        );
    }
}
