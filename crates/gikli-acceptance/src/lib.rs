//! Test harness for gikli.
//!
//! Every test drives gikli through its application interface ([`gikli_core::Gikli`]) and checks the
//! outcome a second way with plain git. The helpers here build that world: a [`Sandbox`] holds
//! throwaway repositories created with the real git executable in a temporary folder, local bare
//! repositories that act as remotes, and an isolated git configuration so the developer's own
//! settings never leak into a test.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use gikli_core::{Gikli, HostEnvironment};
use tempfile::TempDir;

/// A temporary folder with its own git configuration, holding the repositories of one test.
pub struct Sandbox {
    dir: Option<TempDir>,
    root: PathBuf,
}

impl Sandbox {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("gikli-test-")
            .tempdir()
            .expect("create sandbox folder");
        let root = dunce::canonicalize(dir.path()).expect("canonicalize sandbox folder");
        fs::write(
            root.join(".gitconfig"),
            "[user]\n\tname = Test User\n\temail = test@example.com\n[init]\n\tdefaultBranch = main\n",
        )
        .expect("write sandbox git configuration");
        Self {
            dir: Some(dir),
            root,
        }
    }

    /// The sandbox folder. Nothing above it is ever treated as a repository.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The environment gikli runs in: the real one, with git's global configuration replaced by
    /// the sandbox's and repository discovery stopped at the sandbox folder.
    pub fn host_environment(&self) -> HostEnvironment {
        let mut host = HostEnvironment::current();
        for (name, value) in self.git_environment() {
            host.set(name, value);
        }
        host
    }

    fn git_environment(&self) -> Vec<(OsString, OsString)> {
        vec![
            (
                "GIT_CONFIG_GLOBAL".into(),
                self.root.join(".gitconfig").into(),
            ),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("GIT_CEILING_DIRECTORIES".into(), self.root.clone().into()),
        ]
    }

    /// Appends to the sandbox's global git configuration, as a user would edit `~/.gitconfig`.
    pub fn configure(&self, text: &str) {
        let path = self.root.join(".gitconfig");
        let mut config = fs::read_to_string(&path).expect("read sandbox git configuration");
        config.push_str(text);
        fs::write(path, config).expect("write sandbox git configuration");
    }

    /// gikli, running in the sandbox's environment.
    pub fn gikli(&self) -> Gikli {
        Gikli::new(self.host_environment())
    }

    /// A plain folder that is not inside any repository.
    pub fn folder(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::create_dir_all(&path).expect("create folder");
        path
    }

    /// Whether plain git considers `folder` part of a repository's working tree.
    pub fn is_inside_repository_by_git(&self, folder: &Path) -> bool {
        Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(folder)
            .envs(self.git_environment())
            .output()
            .expect("run git")
            .status
            .success()
    }

    /// A new repository with no commits, created with `git init`.
    pub fn repository(&self, name: &str) -> TestRepository {
        let repository = self.repository_handle(self.folder(name));
        repository.git(&["init", "--quiet"]);
        repository
    }

    /// A new bare repository that acts as a remote, created with `git init --bare`.
    pub fn bare_remote(&self, name: &str) -> TestRepository {
        let repository = self.repository_handle(self.folder(name));
        repository.git(&["init", "--quiet", "--bare"]);
        repository
    }

    /// Clones `remote` into a new folder with plain git.
    pub fn clone_of(&self, remote: &TestRepository, name: &str) -> TestRepository {
        let root = self.root.join(name);
        run_git(
            &self.root,
            &self.git_environment(),
            &[
                "clone".into(),
                "--quiet".into(),
                remote.root.clone().into(),
                root.clone().into(),
            ],
        );
        self.repository_handle(root)
    }

    fn repository_handle(&self, root: PathBuf) -> TestRepository {
        TestRepository {
            root,
            env: self.git_environment(),
        }
    }

    /// An environment whose PATH holds only a stand-in git that reports `version` and records
    /// every invocation. `stand_in` is the path of the `fake-git` binary: pass [`fake_git!`].
    pub fn host_with_fake_git(&self, stand_in: &str, version: &str) -> (HostEnvironment, FakeGit) {
        let bin = self.folder("fake-bin");
        let executable = bin.join(format!("git{}", std::env::consts::EXE_SUFFIX));
        fs::copy(stand_in, &executable).expect("install fake git");
        fs::write(
            bin.join("fake-git-version"),
            format!("git version {version}\n"),
        )
        .expect("write fake git version");
        let mut host = self.host_environment();
        host.set("PATH", bin.clone());
        (host, FakeGit { bin })
    }

    /// An environment whose PATH holds no git at all.
    pub fn host_without_git(&self) -> HostEnvironment {
        let mut host = self.host_environment();
        host.set("PATH", self.folder("empty-bin"));
        host
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // Keep the folder when a test fails, so its repositories can be inspected.
        if std::thread::panicking()
            && let Some(dir) = self.dir.take()
        {
            eprintln!("sandbox kept at {}", dir.keep().display());
        }
    }
}

/// Path of the stand-in git binary built with these tests, for [`Sandbox::host_with_fake_git`].
#[macro_export]
macro_rules! fake_git {
    () => {
        env!("CARGO_BIN_EXE_fake-git")
    };
}

/// A stand-in git installed by [`Sandbox::host_with_fake_git`].
pub struct FakeGit {
    bin: PathBuf,
}

impl FakeGit {
    /// The arguments of every run, in order.
    pub fn invocations(&self) -> Vec<Vec<String>> {
        fs::read_to_string(self.bin.join("fake-git-invocations"))
            .unwrap_or_default()
            .lines()
            .map(|line| line.split('\u{1f}').map(str::to_owned).collect())
            .collect()
    }
}

/// A throwaway repository, driven with plain git.
pub struct TestRepository {
    root: PathBuf,
    env: Vec<(OsString, OsString)>,
}

impl TestRepository {
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Runs plain git in the repository and returns its standard output. Panics if git fails.
    pub fn git(&self, args: &[&str]) -> String {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        run_git(&self.root, &self.env, &args)
    }

    /// Writes a file in the working tree, creating its folders.
    pub fn write(&self, path: &str, contents: &str) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().expect("file has a parent folder"))
            .expect("create folders");
        fs::write(path, contents).expect("write file");
    }

    /// Creates a folder inside the working tree and returns its path.
    pub fn folder(&self, path: &str) -> PathBuf {
        let path = self.root.join(path);
        fs::create_dir_all(&path).expect("create folder");
        path
    }

    /// Stages everything and commits it with plain git.
    pub fn commit_all(&self, message: &str) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--message", message]);
    }

    /// Adds `remote` under `name`.
    pub fn add_remote(&self, name: &str, remote: &TestRepository) {
        let url = remote.root.to_string_lossy().into_owned();
        self.git(&["remote", "add", name, &url]);
    }

    /// The repository root as plain git reports it.
    pub fn toplevel_by_git(&self) -> PathBuf {
        let output = self.git(&["rev-parse", "--show-toplevel"]);
        dunce::canonicalize(output.trim_end()).expect("canonicalize git's toplevel")
    }
}

fn run_git(cwd: &Path, env: &[(OsString, OsString)], args: &[OsString]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .envs(env.iter().map(|(name, value)| (name, value)))
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed in {}:\n{}",
        args,
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git output is UTF-8")
}
