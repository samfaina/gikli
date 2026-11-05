//! Finding git on the PATH and checking its version.

use gikli_acceptance::{Sandbox, fake_git};
use gikli_core::{Failure, Gikli, GitCheck, GitVersion};

#[test]
fn gikli_finds_git_on_the_path_and_shows_its_version() {
    let sandbox = Sandbox::new();

    let check = sandbox.gikli().check_git();

    let GitCheck::Ready { version, .. } = check else {
        panic!("expected a usable git, got {check:?}");
    };
    let reported_by_git = sandbox.repository("probe").git(&["--version"]);
    assert!(
        reported_by_git.starts_with(&format!("git version {version}")),
        "gikli shows {version}, git says {reported_by_git:?}"
    );
}

#[test]
fn git_older_than_2_34_gets_a_warning_with_its_version() {
    let sandbox = Sandbox::new();
    let (host, _) = sandbox.host_with_fake_git(fake_git!(), "2.33.8");

    let check = Gikli::new(host).check_git();

    assert!(
        matches!(
            check,
            GitCheck::TooOld { version, minimum, .. }
                if version == GitVersion::new(2, 33, 8) && minimum == GitVersion::new(2, 34, 0)
        ),
        "got {check:?}"
    );
}

#[test]
fn git_older_than_2_34_runs_no_operations() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let (host, fake) = sandbox.host_with_fake_git(fake_git!(), "2.33.8");

    let opened = Gikli::new(host).open_project(repository.root());

    assert_eq!(
        opened,
        Err(Failure::GitTooOld {
            version: GitVersion::new(2, 33, 8),
            minimum: GitVersion::new(2, 34, 0),
        })
    );
    let only_version_checks = fake
        .invocations()
        .iter()
        .all(|args| args.last().map(String::as_str) == Some("--version"));
    assert!(only_version_checks, "git ran {:?}", fake.invocations());
}

#[test]
fn git_2_34_is_enough() {
    let sandbox = Sandbox::new();
    let (host, _) = sandbox.host_with_fake_git(fake_git!(), "2.34.0");

    let check = Gikli::new(host).check_git();

    assert!(
        matches!(check, GitCheck::Ready { version, .. } if version == GitVersion::new(2, 34, 0)),
        "got {check:?}"
    );
}

#[test]
fn a_platform_suffix_in_the_version_is_ignored() {
    let sandbox = Sandbox::new();
    let (host, _) = sandbox.host_with_fake_git(fake_git!(), "2.39.3 (Apple Git-145)");

    let check = Gikli::new(host).check_git();

    assert!(
        matches!(check, GitCheck::Ready { version, .. } if version == GitVersion::new(2, 39, 3)),
        "got {check:?}"
    );
}

#[test]
fn no_git_on_the_path_is_reported() {
    let sandbox = Sandbox::new();
    let gikli = Gikli::new(sandbox.host_without_git());

    assert_eq!(
        gikli.check_git(),
        GitCheck::NotFound {
            minimum: GitVersion::new(2, 34, 0)
        }
    );
    assert_eq!(
        gikli.open_project(&sandbox.folder("app")),
        Err(Failure::GitNotFound)
    );
}

#[test]
fn a_git_whose_version_cannot_be_read_is_not_used() {
    let sandbox = Sandbox::new();
    let (host, _) = sandbox.host_with_fake_git(fake_git!(), "unknown");

    let check = Gikli::new(host).check_git();

    assert!(
        matches!(check, GitCheck::Unrecognized { .. }),
        "got {check:?}"
    );
}

/// On Linux, CI runs the suite once against the latest stable git and once against the oldest
/// supported git. Each of those jobs sets `GIKLI_EXPECTED_GIT_VERSION` to the version it installed
/// (`2.34`, or a full `2.34.0`). This test proves the job ran against that git: both gikli and
/// the harness's plain git must find it. It passes without checking when the variable is unset,
/// as in a local run.
#[test]
fn the_suite_runs_against_the_git_ci_asked_for() {
    let Some(expected) = std::env::var_os("GIKLI_EXPECTED_GIT_VERSION") else {
        eprintln!("skipped: GIKLI_EXPECTED_GIT_VERSION is not set");
        return;
    };
    let expected = expected
        .into_string()
        .expect("GIKLI_EXPECTED_GIT_VERSION is UTF-8");
    let is_expected = |version: &str| {
        version == expected
            || version
                .strip_prefix(expected.as_str())
                .is_some_and(|rest| rest.starts_with('.'))
    };
    let sandbox = Sandbox::new();

    let check = sandbox.gikli().check_git();

    let GitCheck::Ready { version, .. } = check else {
        panic!("expected git {expected}, got {check:?}");
    };
    assert!(
        is_expected(&version.to_string()),
        "gikli found git {version}, CI installed {expected}"
    );
    let reported_by_git = sandbox.repository("probe").git(&["--version"]);
    let plain_version = reported_by_git
        .trim()
        .strip_prefix("git version ")
        .unwrap_or_default();
    assert!(
        is_expected(plain_version),
        "plain git is {reported_by_git:?}, CI installed {expected}"
    );
}
