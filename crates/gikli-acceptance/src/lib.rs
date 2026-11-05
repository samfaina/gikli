//! Test harness for gikli.
//!
//! Every test drives gikli through its application interface ([`gikli_core::Gikli`]) and checks the
//! outcome a second way with plain git. The helpers here build that world: a [`Sandbox`] holds
//! throwaway repositories created with the real git executable in a temporary folder, local bare
//! repositories that act as remotes, and an isolated git configuration so the developer's own
//! settings never leak into a test.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

use gikli_core::{Gikli, HostEnvironment};
use tempfile::TempDir;

/// Makes git take every repository for someone else's.
const OWNED_BY_SOMEONE_ELSE: &str = "GIT_TEST_ASSUME_DIFFERENT_OWNER";

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

    /// Installs `script` as the hook `name` of every repository in the sandbox, as a user would
    /// with `core.hooksPath` in `~/.gitconfig`.
    pub fn hook(&self, name: &str, script: &str) {
        let folder = self.folder("hooks");
        let path = folder.join(name);
        // `wait_until_runnable` starts the script to find out whether it can run.
        fs::write(
            &path,
            format!("#!/bin/sh\nif [ \"$1\" = --version ]; then exit 0; fi\n{script}\n"),
        )
        .expect("write hook");
        make_executable(&path);
        wait_until_runnable(&path);
        self.configure(&format!(
            "[core]\n\thooksPath = {}\n",
            folder.to_string_lossy().replace('\\', "/")
        ));
    }

    /// gikli, running in the sandbox's environment.
    pub fn gikli(&self) -> Gikli {
        self.gikli_in(self.host_environment())
    }

    /// gikli, running in `host`. Every gikli of one sandbox shares the sandbox's configuration
    /// folder, as every start of the app shares the user's.
    pub fn gikli_in(&self, host: HostEnvironment) -> Gikli {
        Gikli::new(host, &self.config_folder())
    }

    /// Where gikli keeps its application settings and its Projects in this sandbox.
    pub fn config_folder(&self) -> PathBuf {
        self.root.join("gikli-settings")
    }

    /// A plain folder that is not inside any repository.
    pub fn folder(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::create_dir_all(&path).expect("create folder");
        path
    }

    /// Whether plain git considers `folder` part of a repository's working tree.
    pub fn is_inside_repository_by_git(&self, folder: &Path) -> bool {
        git_output(
            folder,
            &self.git_environment(),
            ["rev-parse", "--is-inside-work-tree"],
        )
        .status
        .success()
    }

    /// A new repository with no commits, created with `git init`.
    pub fn repository(&self, name: &str) -> TestRepository {
        let repository = self.repository_handle(self.folder(name));
        repository.git(&["init", "--quiet"]);
        repository
    }

    /// A handle to the repository at `root`, created by something other than the sandbox.
    pub fn existing_repository(&self, root: &Path) -> TestRepository {
        self.repository_handle(root.to_path_buf())
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
        let fake = self.fake_git_in("fake-bin", stand_in, version);
        (self.host_with_path(&fake.bin), fake)
    }

    /// An environment whose PATH holds no git, only a stand-in `wsl` whose git reports
    /// `version`, as on Windows with WSL and no native git. `stand_in` is the path of the
    /// `fake-git` binary: pass [`fake_git!`].
    pub fn host_with_fake_wsl(&self, stand_in: &str, version: &str) -> (HostEnvironment, FakeGit) {
        let fake = self.fake_program_in("fake-wsl", "wsl", stand_in, version);
        (self.host_with_path(&fake.bin), fake)
    }

    /// On Windows with WSL and a git inside it: an environment whose PATH holds the real
    /// `wsl.exe` and no native git. None elsewhere, so the test that asks can skip.
    pub fn host_with_real_wsl_only(&self) -> Option<HostEnvironment> {
        let wsl = real_wsl()?;
        Some(self.host_with_path(wsl.parent()?))
    }

    /// A stand-in git that reports `version`, installed in the folder `name` and on no PATH, as
    /// a git the user installed somewhere of their own. `stand_in` is the path of the `fake-git`
    /// binary: pass [`fake_git!`].
    pub fn fake_git_in(&self, name: &str, stand_in: &str, version: &str) -> FakeGit {
        self.fake_program_in(name, "git", stand_in, version)
    }

    /// Installs the `fake-git` binary as `program` in the folder `name`, answering `--version`
    /// with `git version {version}`.
    fn fake_program_in(&self, name: &str, program: &str, stand_in: &str, version: &str) -> FakeGit {
        let bin = self.folder(name);
        let executable = bin.join(format!("{program}{}", std::env::consts::EXE_SUFFIX));
        fs::copy(stand_in, &executable).expect("install fake git");
        fs::write(
            bin.join("fake-git-version"),
            format!("git version {version}\n"),
        )
        .expect("write fake git version");
        wait_until_runnable(&executable);
        // Start the invocation log empty: only gikli's runs belong in it.
        let _ = fs::remove_file(bin.join("fake-git-invocations"));
        FakeGit { bin, executable }
    }

    /// The real git executable at a path that is not on the PATH: the copy in git's own
    /// `libexec/git-core` folder, as a user would point gikli at a git of their choice.
    pub fn real_git_elsewhere(&self) -> PathBuf {
        let exec_path = git_output(&self.root, &self.git_environment(), ["--exec-path"]);
        assert!(exec_path.status.success(), "git --exec-path failed");
        let folder = String::from_utf8(exec_path.stdout).expect("git output is UTF-8");
        let executable =
            Path::new(folder.trim_end()).join(format!("git{}", std::env::consts::EXE_SUFFIX));
        assert!(executable.is_file(), "no git at {}", executable.display());
        dunce::canonicalize(&executable).expect("canonicalize git's path")
    }

    /// The sandbox's environment, where git speaks `language` as far as its translations are
    /// installed. `LANGUAGE` is what selects them, in any locale but the plain C one.
    pub fn host_speaking(&self, language: &str) -> HostEnvironment {
        let mut host = self.host_environment();
        host.set("LANGUAGE", language);
        host.set("LC_ALL", "C.UTF-8");
        host
    }

    /// The sandbox's environment, where the real git takes every repository for someone else's:
    /// the variable git's own tests use, as a test cannot own a folder as another user.
    pub fn host_where_repositories_belong_to_someone_else(&self) -> HostEnvironment {
        let mut host = self.host_environment();
        host.set(OWNED_BY_SOMEONE_ELSE, "1");
        host
    }

    /// An environment whose PATH holds no git at all.
    pub fn host_without_git(&self) -> HostEnvironment {
        self.host_with_path(&self.folder("empty-bin"))
    }

    /// The sandbox's environment with `folder` as the only folder on the PATH.
    fn host_with_path(&self, folder: &Path) -> HostEnvironment {
        let mut host = self.host_environment();
        host.set("PATH", folder);
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

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("make hook executable");
}

/// On Windows, git runs a hook through its own shell whatever the file's permissions.
#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

/// Waits until a freshly copied executable can run.
///
/// On Linux, starting a program fails with "text file busy" (`ETXTBSY`) while any process still
/// has the file open for writing. Tests run in parallel threads: one that starts a process while
/// another is copying the stand-in git hands the child a copy of the open file, which it holds
/// until its own program starts. gikli would then find a git it cannot run and report it
/// unrecognized.
#[cfg(unix)]
fn wait_until_runnable(executable: &Path) {
    const ETXTBSY: i32 = 26;
    for _ in 0..200 {
        match Command::new(executable).arg("--version").output() {
            Err(error) if error.raw_os_error() == Some(ETXTBSY) => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            _ => return,
        }
    }
    panic!("{} stayed busy", executable.display());
}

/// On Windows, processes started by the tests inherit no open files, so the copy is never busy.
#[cfg(not(unix))]
fn wait_until_runnable(_executable: &Path) {}

/// Path of the stand-in git binary built with these tests, for [`Sandbox::host_with_fake_git`],
/// [`Sandbox::host_with_fake_wsl`] and [`Sandbox::fake_git_in`].
#[macro_export]
macro_rules! fake_git {
    () => {
        env!("CARGO_BIN_EXE_fake-git")
    };
}

/// A `file://` URL for `folder`, so git clones through its transport, as from a server, instead of
/// copying the folder.
pub fn file_url(folder: &Path) -> String {
    let path = folder.to_string_lossy().replace('\\', "/");
    format!("file:///{}", path.trim_start_matches('/'))
}

/// A stand-in git, or `wsl.exe`, installed by the sandbox.
pub struct FakeGit {
    bin: PathBuf,
    executable: PathBuf,
}

impl FakeGit {
    /// The stand-in's executable file.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// The arguments of every run, in order.
    pub fn invocations(&self) -> Vec<Vec<String>> {
        fs::read_to_string(self.bin.join("fake-git-invocations"))
            .unwrap_or_default()
            .lines()
            .map(|line| line.split('\u{1f}').map(str::to_owned).collect())
            .collect()
    }
}

/// A plain git that runs until this is dropped.
pub struct RunningGit(Child);

impl Drop for RunningGit {
    fn drop(&mut self) {
        // Closing its input ends it, along with the git the wrapper on Windows starts.
        drop(self.0.stdin.take());
        let _ = self.0.wait();
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

    /// Runs plain git in the repository, for a command that may fail, and returns whether it
    /// succeeded.
    pub fn git_succeeds(&self, args: &[&str]) -> bool {
        git_output(&self.root, &self.env, args).status.success()
    }

    /// Whether plain git refuses this repository when it takes it for someone else's.
    pub fn refused_for_ownership_by_git(&self) -> bool {
        let mut env = self.env.clone();
        env.push((OWNED_BY_SOMEONE_ELSE.into(), "1".into()));
        !git_output(&self.root, &env, ["status"]).status.success()
    }

    /// A plain git running in the repository until the result is dropped, as a git the user
    /// started in a terminal. It waits for input it never gets.
    pub fn running_git(&self) -> RunningGit {
        let child = Command::new("git")
            .args(["cat-file", "--batch"])
            .current_dir(&self.root)
            .envs(self.env.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .expect("start git");
        RunningGit(child)
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

    /// Deletes a file from the working tree.
    pub fn delete(&self, path: &str) {
        fs::remove_file(self.root.join(path)).expect("delete file");
    }

    /// The uncommitted changes as plain git reports them, one `XY path` entry per file in
    /// `git status --porcelain` (v1) form, with every untracked file listed.
    pub fn status_by_git(&self) -> Vec<String> {
        self.git(&[
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ])
        .split_terminator('\0')
        .map(str::to_owned)
        .collect()
    }

    /// Adds a linked worktree of this repository in the sibling folder `name`, on a new branch of
    /// that name, with `git worktree add`. Its git directory lives inside this repository's.
    pub fn linked_worktree(&self, name: &str) -> TestRepository {
        let root = self
            .root
            .parent()
            .expect("repository has a parent")
            .join(name);
        let path = root.to_string_lossy().into_owned();
        self.git(&["worktree", "add", "--quiet", "-b", name, &path]);
        TestRepository {
            root: dunce::canonicalize(root).expect("canonicalize worktree folder"),
            env: self.env.clone(),
        }
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
    let output = git_output(cwd, env, args);
    assert!(
        output.status.success(),
        "git {:?} failed in {}:\n{}",
        args,
        cwd.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git output is UTF-8")
}

fn git_output(
    cwd: &Path,
    env: &[(OsString, OsString)],
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .envs(env.iter().map(|(name, value)| (name, value)))
        .output()
        .expect("run git")
}

/// The real `wsl.exe`, when WSL is installed and its default distribution has git.
pub fn real_wsl() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let system_root = std::env::var_os("SystemRoot")?;
    let wsl = Path::new(&system_root).join("System32").join("wsl.exe");
    let has_git = Command::new(&wsl)
        .args(["--exec", "git", "--version"])
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success());
    has_git.then_some(wsl)
}

/// A throwaway repository inside the default WSL distribution, created with the git there, and
/// removed when dropped.
pub struct WslRepository {
    wsl: PathBuf,
    distribution: String,
    linux_root: String,
}

impl WslRepository {
    /// A new repository with no commits in a temporary folder of the default WSL distribution,
    /// or None without WSL and a git inside it.
    pub fn create() -> Option<Self> {
        let wsl = real_wsl()?;
        let distribution = wsl_output(&wsl, None, &["printenv", "WSL_DISTRO_NAME"])
            .trim()
            .to_owned();
        let linux_root = wsl_output(&wsl, None, &["mktemp", "-d", "/tmp/gikli-test-XXXXXX"])
            .trim()
            .to_owned()
            + "/app";
        let repository = Self {
            wsl,
            distribution,
            linux_root,
        };
        wsl_output(
            &repository.wsl,
            None,
            &["mkdir", "-p", &repository.linux_root],
        );
        repository.git(&["init", "--quiet"]);
        Some(repository)
    }

    /// The repository root as Windows sees it, under `\\wsl$`.
    pub fn root(&self) -> PathBuf {
        self.root_under("wsl$")
    }

    /// The repository root through the share `server`: `wsl$` or `wsl.localhost`.
    pub fn root_under(&self, server: &str) -> PathBuf {
        self.windows_path(server, &self.linux_root)
    }

    /// The repository root as plain git inside WSL reports it, seen through `\\wsl$`.
    pub fn toplevel_by_git(&self) -> PathBuf {
        let toplevel = self.git(&["rev-parse", "--show-toplevel"]);
        self.windows_path("wsl$", toplevel.trim_end())
    }

    /// Creates a folder inside the working tree and returns its path under `\\wsl$`.
    pub fn folder(&self, path: &str) -> PathBuf {
        let folder = self.root().join(path);
        fs::create_dir_all(&folder).expect("create folder");
        folder
    }

    /// `linux_path` inside the distribution, through the share `server`.
    fn windows_path(&self, server: &str, linux_path: &str) -> PathBuf {
        let mut path = format!(r"\\{server}\{}", self.distribution);
        for name in linux_path.split('/').filter(|name| !name.is_empty()) {
            path.push('\\');
            path.push_str(name);
        }
        PathBuf::from(path)
    }

    /// Runs plain git inside WSL in the repository and returns its standard output.
    pub fn git(&self, args: &[&str]) -> String {
        let mut command = vec!["git"];
        command.extend(args);
        wsl_output(&self.wsl, Some(&self.linux_root), &command)
    }

    /// Writes a file in the working tree, through the `\\wsl$` share.
    pub fn write(&self, path: &str, contents: &str) {
        fs::write(self.root().join(path), contents).expect("write file");
    }

    /// The uncommitted changes as plain git inside WSL reports them, in the form of
    /// [`TestRepository::status_by_git`].
    pub fn status_by_git(&self) -> Vec<String> {
        self.git(&[
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ])
        .split_terminator('\0')
        .map(str::to_owned)
        .collect()
    }
}

impl Drop for WslRepository {
    fn drop(&mut self) {
        let parent = self.linux_root.trim_end_matches("/app");
        let _ = Command::new(&self.wsl)
            .args(["--exec", "rm", "-rf", parent])
            .output();
    }
}

/// Runs `command` inside the default WSL distribution, in `folder` if given, and returns its
/// standard output. Panics if it fails.
fn wsl_output(wsl: &Path, folder: Option<&str>, command: &[&str]) -> String {
    let mut wsl_command = Command::new(wsl);
    if let Some(folder) = folder {
        wsl_command.args(["--cd", folder]);
    }
    let output = wsl_command
        .arg("--exec")
        .args(command)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run wsl");
    assert!(
        output.status.success(),
        "wsl {command:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("WSL output is UTF-8")
}
