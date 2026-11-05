//! The setting that gives the path to git, and the button that tests a path.

use gikli_acceptance::{Sandbox, fake_git};
use gikli_core::{Failure, GitCheck, GitSource, GitVersion, Repository};

#[test]
fn testing_a_path_shows_the_version_of_the_git_there() {
    let sandbox = Sandbox::new();
    let custom = sandbox.fake_git_in("custom-git", fake_git!(), "2.40.1");

    let check = sandbox.gikli().test_git(custom.executable());

    assert_eq!(
        check,
        GitCheck::Ready {
            executable: custom.executable().to_path_buf(),
            source: GitSource::Configured,
            version: GitVersion::new(2, 40, 1),
        }
    );
}

#[test]
fn testing_a_path_with_no_git_says_so() {
    let sandbox = Sandbox::new();
    let nowhere = sandbox.folder("empty").join("git");

    let check = sandbox.gikli().test_git(&nowhere);

    assert_eq!(
        check,
        GitCheck::Missing {
            executable: nowhere
        }
    );
}

#[test]
fn testing_a_file_that_is_not_git_says_its_version_cannot_be_read() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "not a program\n");
    let not_git = repository.root().join("notes.txt");

    let check = sandbox.gikli().test_git(&not_git);

    assert_eq!(
        check,
        GitCheck::Unrecognized {
            executable: not_git,
            source: GitSource::Configured,
        }
    );
}

#[test]
fn the_configured_git_comes_before_the_one_on_the_path() {
    let sandbox = Sandbox::new();
    let custom = sandbox.fake_git_in("custom-git", fake_git!(), "2.40.1");
    let gikli = sandbox.gikli();

    let applied = gikli
        .set_git_path(Some(custom.executable().to_path_buf()))
        .expect("save the setting");

    let expected = GitCheck::Ready {
        executable: custom.executable().to_path_buf(),
        source: GitSource::Configured,
        version: GitVersion::new(2, 40, 1),
    };
    assert_eq!(applied, expected);
    assert_eq!(gikli.check_git(), expected);
}

#[test]
fn operations_run_the_configured_git() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let gikli = sandbox.gikli_in(sandbox.host_without_git());
    gikli
        .set_git_path(Some(sandbox.real_git_elsewhere()))
        .expect("save the setting");

    let project = gikli.open_project(repository.root()).expect("open project");

    assert_eq!(
        project.repositories,
        vec![Repository {
            root: repository.toplevel_by_git()
        }]
    );
}

#[test]
fn the_path_to_git_is_kept_for_the_next_start() {
    let sandbox = Sandbox::new();
    let custom = sandbox.fake_git_in("custom-git", fake_git!(), "2.40.1");
    sandbox
        .gikli()
        .set_git_path(Some(custom.executable().to_path_buf()))
        .expect("save the setting");

    let next_start = sandbox.gikli();

    assert_eq!(
        next_start.settings().git_path.as_deref(),
        Some(custom.executable())
    );
    assert!(
        matches!(
            next_start.check_git(),
            GitCheck::Ready { source: GitSource::Configured, version, .. }
                if version == GitVersion::new(2, 40, 1)
        ),
        "got {:?}",
        next_start.check_git()
    );
}

/// The user chose that git: gikli does not quietly run another one in its place.
#[test]
fn a_configured_git_that_is_gone_is_reported_instead_of_using_the_path() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let gone = sandbox.folder("uninstalled").join("git");
    sandbox
        .gikli()
        .set_git_path(Some(gone.clone()))
        .expect("save the setting");

    let gikli = sandbox.gikli();

    assert_eq!(
        gikli.check_git(),
        GitCheck::Missing {
            executable: gone.clone()
        }
    );
    assert_eq!(
        gikli.open_project(repository.root()),
        Err(Failure::GitNotFound)
    );
}

#[test]
fn clearing_the_path_to_git_goes_back_to_the_path() {
    let sandbox = Sandbox::new();
    let custom = sandbox.fake_git_in("custom-git", fake_git!(), "2.40.1");
    let gikli = sandbox.gikli();
    gikli
        .set_git_path(Some(custom.executable().to_path_buf()))
        .expect("save the setting");

    let applied = gikli.set_git_path(None).expect("save the setting");

    let GitCheck::Ready {
        source, version, ..
    } = applied
    else {
        panic!("expected the git on the PATH, got {applied:?}");
    };
    assert_eq!(source, GitSource::Path);
    let reported_by_git = sandbox.repository("probe").git(&["--version"]);
    assert!(reported_by_git.starts_with(&format!("git version {version}")));
    assert_eq!(sandbox.gikli().settings().git_path, None);
}
