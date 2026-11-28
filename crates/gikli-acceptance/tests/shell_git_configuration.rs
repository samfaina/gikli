//! Configuration that the shell running the tests passes to git through the environment is the
//! developer's, so no plain git the harness runs sees it. Alone in its file, as the test sets that
//! configuration for its whole process.

use gikli_acceptance::Sandbox;

#[test]
fn configuration_the_shell_passes_to_git_does_not_reach_plain_git() {
    // SAFETY: the only test of this binary, so no other thread reads the environment meanwhile.
    unsafe {
        std::env::set_var("GIT_CONFIG_PARAMETERS", "'rebase.rebasemerges'='true'");
        std::env::set_var("GIT_CONFIG_COUNT", "1");
        std::env::set_var("GIT_CONFIG_KEY_0", "user.name");
        std::env::set_var("GIT_CONFIG_VALUE_0", "Someone Else");
    }
    let sandbox = Sandbox::new();

    let repository = sandbox.repository("repository");

    assert!(
        !repository.git_succeeds(&["config", "rebase.rebaseMerges"]),
        "git has no rebase.rebaseMerges"
    );
    assert_eq!(repository.git(&["config", "user.name"]), "Test User\n");
}
