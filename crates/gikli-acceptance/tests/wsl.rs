//! WSL git on Windows: the fallback when there is no native git, and Repositories under
//! `\\wsl$`.
#![cfg(windows)]

use std::path::{Path, PathBuf};

use gikli_acceptance::{Sandbox, WslRepository, fake_git, real_wsl};
use gikli_core::{
    Change, ChangeStatus, GitCheck, GitSource, GitVersion, HostEnvironment, Repository,
};

#[test]
fn without_native_git_gikli_uses_wsl_git() {
    let sandbox = Sandbox::new();
    let (host, wsl) = sandbox.host_with_fake_wsl(fake_git!(), "2.43.0");

    let check = sandbox.gikli_in(host).check_git();

    assert_eq!(
        check,
        GitCheck::Ready {
            executable: wsl.executable().to_path_buf(),
            source: GitSource::Wsl,
            version: GitVersion::new(2, 43, 0),
        }
    );
    let asked_wsl_for_git = wsl.invocations().iter().any(|args| {
        let git = args.iter().position(|arg| arg == "git");
        git.is_some() && args.last().map(String::as_str) == Some("--version")
    });
    assert!(asked_wsl_for_git, "wsl ran {:?}", wsl.invocations());
}

#[test]
fn native_git_on_the_path_comes_before_wsl_git() {
    let sandbox = Sandbox::new();
    let (mut host, wsl) = sandbox.host_with_fake_wsl(fake_git!(), "2.43.0");
    let native = sandbox.fake_git_in("native-git", fake_git!(), "2.45.2");
    let path = std::env::join_paths([folder_of(native.executable()), folder_of(wsl.executable())])
        .expect("join PATH");
    host.set("PATH", path);

    let check = sandbox.gikli_in(host).check_git();

    assert_eq!(
        check,
        GitCheck::Ready {
            executable: native.executable().to_path_buf(),
            source: GitSource::Path,
            version: GitVersion::new(2, 45, 2),
        }
    );
}

fn folder_of(executable: &Path) -> &Path {
    executable.parent().expect("an executable has a folder")
}

#[test]
fn a_repository_on_a_windows_drive_opens_through_wsl_git() {
    let sandbox = Sandbox::new();
    let Some(host) = sandbox.host_with_real_wsl_only() else {
        eprintln!("skipped: no WSL with git on this machine");
        return;
    };
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    let gikli = sandbox.gikli_in(host);

    let project = gikli
        .open_project(&repository.folder("src"))
        .expect("open project");
    let change_set = gikli
        .read_changes(&project.repositories[0])
        .expect("read changes");

    assert!(
        matches!(
            gikli.check_git(),
            GitCheck::Ready {
                source: GitSource::Wsl,
                ..
            }
        ),
        "got {:?}",
        gikli.check_git()
    );
    assert_eq!(
        project.repositories,
        vec![Repository {
            root: repository.toplevel_by_git()
        }]
    );
    assert_eq!(
        change_set.changes,
        vec![Change {
            path: PathBuf::from("README.md"),
            status: ChangeStatus::Untracked,
            original_path: None,
            conflict: None,
            submodule: None,
        }]
    );
    assert_eq!(repository.status_by_git(), vec!["?? README.md"]);
}

/// The native git here is the stand-in, which fails every command but `--version`: the
/// Repository can only open and show its changes if gikli ran the git inside WSL.
#[test]
fn a_repository_under_wsl_share_opens_and_works_through_wsl_git() {
    let Some(repository) = WslRepository::create() else {
        eprintln!("skipped: no WSL with git on this machine");
        return;
    };
    repository.write("notes.txt", "from Windows\n");
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli_in(host_with_stand_in_git_and_real_wsl(&sandbox));
    let subfolder = repository.folder("src");

    let project = gikli.open_project(&subfolder).expect("open project");
    let change_set = gikli
        .read_changes(&project.repositories[0])
        .expect("read changes");

    assert_eq!(
        project.repositories,
        vec![Repository {
            root: repository.toplevel_by_git()
        }]
    );
    assert_eq!(
        project.repositories,
        vec![Repository {
            root: repository.root().to_path_buf()
        }]
    );
    assert_eq!(
        change_set.changes,
        vec![Change {
            path: PathBuf::from("notes.txt"),
            status: ChangeStatus::Untracked,
            original_path: None,
            conflict: None,
            submodule: None,
        }]
    );
    assert_eq!(repository.status_by_git(), vec!["?? notes.txt"]);
}

#[test]
fn a_repository_under_wsl_localhost_keeps_that_form() {
    let Some(repository) = WslRepository::create() else {
        eprintln!("skipped: no WSL with git on this machine");
        return;
    };
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli_in(host_with_stand_in_git_and_real_wsl(&sandbox));
    let localhost = repository.root_under("wsl.localhost");

    let project = gikli.open_project(&localhost).expect("open project");

    assert_eq!(project.repositories, vec![Repository { root: localhost }]);
}

/// An environment whose PATH holds the stand-in git, then the real `wsl.exe`.
fn host_with_stand_in_git_and_real_wsl(sandbox: &Sandbox) -> HostEnvironment {
    let (mut host, _native) = sandbox.host_with_fake_git(fake_git!(), "2.45.2");
    let wsl = real_wsl().expect("WSL is installed");
    let path = std::env::join_paths([
        PathBuf::from(host.get("PATH").expect("PATH is set")),
        folder_of(&wsl).to_path_buf(),
    ])
    .expect("join PATH");
    host.set("PATH", path);
    host
}
