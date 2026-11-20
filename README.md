# gikli

A standalone desktop Git client for Windows, macOS and Linux. See `GLOSSARY.md` for the domain
terms, `docs/adr/` for the architecture and `docs/product-decisions.md` for settled behavior.

## Layout

- `crates/gikli-core`: the core. It runs the user's git (ADR-0001) and exposes the application
  interface, `Gikli`: the commands the UI sends and the events it listens to.
- `crates/gikli-acceptance`: the test harness and the tests. Tests drive `Gikli` and check every
  outcome a second way with plain git, in throwaway repositories and local bare remotes. The
  real git runs in every test except the installation and version checks and the few others
  [ADR-0006](docs/adr/0006-real-git-in-tests-except-installation-checks.md) lists, which use a
  stand-in (`fake-git`). Tests of WSL itself run on Windows with WSL and a git inside it,
  and skip elsewhere. Tests of a machine where gikli cannot read git's trace set `GIKLI_NO_TRACE_READER`,
  as nothing else makes that happen on demand. Tests of dubious ownership play
  another owner with git's own `GIT_TEST_ASSUME_DIFFERENT_OWNER`, and skip on a git that does
  not check ownership, such as 2.34.0. Tests of long paths run only on Windows. Tests of
  credentials use a local HTTP server that requires them, in front of git's own `git http-backend`.
  A test cannot count on an SSH server, so tests of ssh prompts and host keys use a stand-in for
  ssh that asks and reports in OpenSSH's words; the signing test runs the real `ssh-keygen`.
  One test lives in `crates/gikli-core/src/watch.rs` instead and runs no git: that a burst without
  a pause is announced once, since real files cannot make a burst whose timing does not depend on
  the machine.
  The test of the log's speed, `log_performance.rs`, generates a history of 300,000 commits with
  `git fast-import`, reports how long each page took and checks what the pages show.
  `go_to_performance.rs` does the same for going to a commit that the log, filtered by a branch
  or searched for a text, does not show. They check their budgets only when `GIKLI_CHECK_BUDGETS`
  is set, as `npm run test:performance` sets it, so a loaded machine does not fail the suite.
- `src-tauri`: the Tauri 2 shell. Each core command is a Tauri command that forwards to `Gikli`,
  and core events reach the UI as `gikli://event`. The shell adds only `startup_folder`, the
  folder to open at launch. The app is also its own askpass program: when git or ssh start it to
  ask the user something, it passes the question to the running app and exits.
- `ui`: the TypeScript UI. Every string it shows is in `ui/src/strings/en.ts`, and the theme
  follows the operating system's light or dark setting.
- `e2e`: the end-to-end smoke test, which launches the app through WebDriver.

## Requirements

- Rust (stable) and Node.js 20 or newer.
- git 2.34 or newer on the PATH. The tests need it there; the app also takes a path to git in
  its settings, or on Windows the git inside WSL.
- The Tauri prerequisites for your platform (<https://v2.tauri.app/start/prerequisites/>):
  WebView2 on Windows (preinstalled on Windows 11), Xcode Command Line Tools on macOS, and on
  Linux `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev` and `libayatana-appindicator3-dev`
  or their equivalents.

## Commands

```sh
npm install
npm run dev             # run the app with the UI hot-reloading
npm run build           # build the app for this platform
cargo test --workspace  # core tests, all but one through the application interface
npm run typecheck       # typecheck the UI
npm run test:e2e-helpers # tests of the smoke test's own helpers, such as how it attaches on Windows CI
npm run test:e2e        # smoke test: builds the app, launches it, opens a Project
npm test                # all of the above checks
npm run test:performance # the two tests of speed, checking their budgets
```

The smoke test needs `tauri-driver` (`cargo install tauri-driver --locked`). On Windows it
downloads the Microsoft Edge WebDriver that matches the installed WebView2 Runtime on first run. On Linux it
needs `webkit2gtk-driver` (WebKitWebDriver). tauri-driver does not support macOS, so there the
smoke test does not run.

CI builds the app and runs these checks in two tiers
([ADR-0008](docs/adr/0008-ci-in-two-tiers-linux-jobs-in-an-image.md)). A pull request against
`main` runs one job (`.github/workflows/pr.yml`), on Windows with the runner's preinstalled git.
Every push to `main` (`.github/workflows/ci.yml`) runs them
on Windows, macOS and Linux, and once more on Windows and on Linux against git 2.34, the oldest
supported version. The Linux jobs run in the CI image (`.github/ci-image/Dockerfile`, published to
GHCR by `.github/workflows/ci-image.yml` weekly, by hand, and when a push to `main` changes it),
which holds their prerequisites and both gits. The smoke test runs on every job but macOS's, which
reports it as skipped. On Windows it runs without tauri-driver, which cannot start a session on
the GitHub-hosted runner (#54): a CI step opens a WebView2 DevTools port with the
`AdditionalBrowserArguments` policy for `gikli.exe` and names it in
`GIKLI_E2E_WEBVIEW2_DEBUG_PORT`, and with that variable set the smoke test launches the app itself
and attaches the Edge WebDriver to that port. The runner can be slow to start WebView2 on a job's
first launch (#155), so when the port does not answer within 60 s the smoke test stops the app and
launches it again, three times at most. Without the variable it goes through tauri-driver.

With `GIKLI_E2E_OUTPUT` set to a folder, the smoke test writes its WebDriver logs there instead of
to the terminal, and a screenshot of each case that fails. CI sets it, and keeps the folder as an
artifact of a job whose smoke test failed. CI can also be run by hand (`gh workflow run ci.yml
--ref <branch>`), to measure how often the smoke test fails on Linux and Windows: each Linux and
Windows job then runs it the number of times its `smoke-runs` input asks for (20 by default), in a
row, and says how many of those runs failed.

To open a folder at startup, pass it as the first argument (`gikli <folder>`) or set
`GIKLI_PROJECT_FOLDER`.

gikli keeps its settings (`settings.json`) and its Projects (`projects.json`) in the app's config
folder (on Windows, `%APPDATA%\dev.gikli.app`). To keep them somewhere else, set
`GIKLI_CONFIG_FOLDER`; a relative path resolves against the current directory, and the folder is
created on the first save. The smoke test sets it to a folder inside its sandbox, so a run starts
from default settings and no Projects and leaves the developer's own untouched.
