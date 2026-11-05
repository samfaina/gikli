//! Features that need a newer git than the minimum: gikli records what the git in use can do,
//! and a feature it cannot run is disabled with the version it needs.

use gikli_acceptance::{Sandbox, fake_git};
use gikli_core::{Capability, CapabilityState, GitVersion, Repository};

fn update_refs(states: &[CapabilityState]) -> &CapabilityState {
    states
        .iter()
        .find(|state| state.capability == Capability::RebaseUpdateRefs)
        .expect("gikli reports rebase --update-refs")
}

#[test]
fn a_feature_that_needs_a_newer_git_is_disabled_with_the_version_it_needs() {
    let sandbox = Sandbox::new();
    let (host, _) = sandbox.host_with_fake_git(fake_git!(), "2.37.9");
    let repository = Repository {
        root: sandbox.folder("app"),
    };

    let states = sandbox
        .gikli_in(host)
        .capabilities(&repository)
        .expect("a usable git");

    assert_eq!(
        *update_refs(&states),
        CapabilityState {
            capability: Capability::RebaseUpdateRefs,
            required: GitVersion::new(2, 38, 0),
            available: false,
        }
    );
}

#[test]
fn a_feature_is_enabled_from_the_version_it_needs() {
    let sandbox = Sandbox::new();
    let (host, _) = sandbox.host_with_fake_git(fake_git!(), "2.38.0");
    let repository = Repository {
        root: sandbox.folder("app"),
    };

    let states = sandbox
        .gikli_in(host)
        .capabilities(&repository)
        .expect("a usable git");

    assert!(update_refs(&states).available);
}

/// Against the git under test, whichever version it is: gikli enables `rebase --update-refs`
/// exactly when that git accepts it.
#[test]
fn gikli_agrees_with_git_about_what_git_can_do() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(repository.root()).expect("open project");

    let states = gikli
        .capabilities(&project.repositories[0])
        .expect("a usable git");

    let accepted_by_git = repository.git_succeeds(&["rebase", "--update-refs", "HEAD"]);
    assert_eq!(update_refs(&states).available, accepted_by_git);
}
