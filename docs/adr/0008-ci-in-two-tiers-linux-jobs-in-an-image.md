# CI runs in two tiers, and its Linux jobs run in an image

A pull request against `main` runs one CI job, the PR gate (`.github/workflows/pr.yml`): on Windows, with the runner's preinstalled git, it builds the app and runs the core tests, the UI typecheck, the smoke test's helper tests and the smoke test. Every push to `main`, and a run by hand, runs the full matrix (`.github/workflows/ci.yml`): Windows and Windows against the minimum git, macOS, and Linux against the latest stable git and against the minimum git. The two Linux jobs run in a container image published to GHCR (`ghcr.io/samfaina/gikli-ci:latest`, defined in `.github/ci-image/Dockerfile`), which holds the Tauri prerequisites, WebKitWebDriver, xvfb, the latest stable git from `ppa:git-core/ppa`, git at `MINIMUM_GIT_VERSION` built from source into `/opt/git-<version>`, and `tauri-driver`. Those jobs make no request to Ubuntu's package servers, Launchpad or kernel.org. The push of a release tag runs the same matrix before it publishes anything: see ADR-0009 for that tier.

Before, every job ran on every pull request, and each Linux job installed all of that from those servers on every run, and built the minimum git from a kernel.org tarball whenever its cache missed, which it did on every branch after any change to the workflow. Those steps failed often, before any test ran, and turned pull requests and `main` red for reasons that had nothing to do with the change.

## Why Windows gates a pull request

Windows is gikli's main development platform (ADR-0002). Its job has the fewest network steps left: the runner's own git and WebView2, with nothing to install. A Linux gate would rest on the image, so a bad image would block every pull request; a macOS gate could not run the smoke test.

## Why the matrix runs on every push to `main`, not nightly

A push to `main` is a merged pull request. Running the matrix on it names the change that broke macOS, Linux or the minimum git, and the merge is still fresh in its author's mind. A nightly run would batch a day's merges into one red run, leave `main` broken for up to a day before anyone hears of it, and need a bisect to find the cause. Once per merge, the matrix still costs fewer runner minutes than when it ran on every push to every pull request.

## Why an image, not an apt cache

An apt cache would still run `apt-get update` against Ubuntu's servers and Launchpad on every run to find what to install, and `actions/cache` has the problems that made the minimum git rebuild: a branch cannot read a sibling's cache, a job saves only when it succeeds, and a key that covers the workflow misses after every change to it. An image moves every one of those downloads, and the build of git and `tauri-driver`, out of the test jobs into one build that runs weekly, by hand, and when its definition changes.

## Consequences

- The image workflow (`.github/workflows/ci-image.yml`) runs weekly and by hand, and pushes `:latest` only once the build succeeds, so a failed build leaves the previous image in place and blocks nothing.
- The PR gate is a workflow of its own: a job of the matrix's workflow that skipped a pull request would still show on it, as a skipped check. Both run the Windows and macOS jobs from one reusable workflow (`.github/workflows/windows-and-macos.yml`), so the gate checks a pull request exactly as `main`'s Windows job does. The Linux jobs repeat those checks in `ci.yml`, in the image.
- Each image is also tagged with the minimum git and a hash of the Dockerfile and the image workflow (`.github/scripts/ci-image-tag.sh`). A push to `main` looks for the tag of its own definition and builds the image first when it is missing, so the push that changes the definition or `MINIMUM_GIT_VERSION`, and the first push that runs the Linux jobs in the image, find their image. When that build fails, the Linux jobs run in the previous `:latest`. One case still turns `main` red: a push that raises `MINIMUM_GIT_VERSION` and whose build fails leaves the minimum git job without the git it needs, and it says so rather than test another git or fetch one from kernel.org.
- `MINIMUM_GIT_VERSION` in gikli-core stays the only place the minimum git is written down: the image build and the CI jobs read it with `.github/scripts/minimum-git-version.sh`.
- The latest git job still fails when the git package is not the PPA's (`.github/scripts/ppa-git-version.sh`), and every job still sets `GIKLI_EXPECTED_GIT_VERSION` for `the_suite_runs_against_the_git_ci_asked_for`.
- A pull request no longer tests macOS, Linux or the minimum git. A change that breaks only those shows up red on `main`, after the merge, and is fixed with a follow-up pull request.
- A change to the Dockerfile is not tested on its pull request: the image is built on the push to `main`.
- GHCR publishes a new package as private. The Linux jobs pull it with the workflow's token, which works once the package is tied to this repository (the image's `org.opencontainers.image.source` label does that); otherwise the maintainer makes the package public by hand.

## Considered Options

- **The full matrix on every pull request, as before**: every pull request waits on five jobs and on every network step they take.
- **A Linux PR gate in the image**: faster runners, but the gate would depend on the image, and gikli's main platform would be tested only after the merge.
- **The full matrix nightly**: see above.
- **An apt cache, or caching the minimum git under a key that leaves out the workflow**: see above; the Launchpad and package server requests stay in every run.
