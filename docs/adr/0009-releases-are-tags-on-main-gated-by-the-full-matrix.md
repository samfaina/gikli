# Releases are tags on `main`, gated by the full matrix

A release of gikli is an annotated tag `vX.Y.Z` on a commit of `main`. The maintainer bumps the version in a pull request titled "Release vX.Y.Z", which changes it in the three places it is written down: `[workspace.package]` in the root `Cargo.toml` (which `src-tauri` inherits, and the Tauri config takes as the app's version), `package.json`, and both entries for the root package in `package-lock.json`. Once it is merged, they tag the merge commit and push the tag (`git tag -a vX.Y.Z -m "gikli X.Y.Z" <sha>`, `git push origin vX.Y.Z`).

The push of a `v*` tag runs the CI workflow (`.github/workflows/ci.yml`, ADR-0008) in three steps:

1. A first job checks the tag before any other job starts: it fails when the tagged commit is not in `main`'s history, or when `X.Y.Z` is not the version in each of those three files (`.github/scripts/release-version.sh`, which can be run by hand before tagging). Its error names the check that failed and the values it compared.
2. The full matrix runs, the same five jobs as on a push to `main`, with the smoke test once. The Linux jobs run in `gikli-ci:latest`: the tagged commit is on `main`, whose push already made sure of the image.
3. Only when the check and every job of the matrix succeeded, a last job publishes a GitHub Release for the tag, marked as a pre-release, with GitHub's generated notes and no assets (`gh release create <tag> --verify-tag --prerelease --generate-notes`). It is the only job of the workflow that can write to the repository.

A red run publishes nothing. The tag stays: the maintainer deletes it, or fixes the cause on `main` and pushes a new tag. Nothing deletes a tag on its own.

While gikli is in 0.x, the minor goes up when a release brings new features and the patch when it brings only fixes, with no fixed cadence, and every release is a pre-release. The first tag is `v0.1.0`.

## Why a tag on `main`, not a release branch

Every commit of `main` already went through the full matrix on its push (ADR-0008), so the commit a release names has been tested on every platform as merged, and the run of its tag tests it once more right before it is published. A `develop` or release branch (#157) would gather many pull requests before the matrix runs on them: the network failures that tier left out would come back at release time, and a regression would be mixed in with every other change since the last release.

## Why the release waits for the whole matrix

The matrix is what tells that a commit works on Windows, macOS and Linux, and against the oldest git gikli supports. Publishing when it is red would hand out a release that is known to fail on one of them; running it only by hand before tagging would leave the release to whether someone remembered, and to a run of another commit.

## Consequences

- The release runs in the CI workflow itself, not in a workflow of its own, so it is gated by the very jobs that test `main`. The two jobs a tag adds, the check and the release, therefore show as skipped on a push to `main` and on a run by hand: a job of a workflow that a run skips still shows on it. The PR gate (`pr.yml`) is a workflow of its own and does not show them.
- The concurrency group of the CI workflow is its ref, so a tag's run neither cancels nor is cancelled by a run of `main` or of another tag.
- The jobs of the matrix wait for the check on a tag; on any other run it is skipped, and they start as before.
- The image jobs are skipped on a tag, as on any ref other than `main`, so the release job names each job it needs in its condition rather than rely on GitHub's default, which would skip it along with them.
- The version is bumped by hand, in three files. The check catches a file left out of the bump before the matrix runs.
- There is no changelog in the repository: the generated notes on the Release list the pull requests since the previous tag.
- Bundles, installers, signing and the updater are not part of a release yet (#161).

## Considered Options

- **A `develop` or release branch**: see above.
- **release-please**: it derives the version and the notes from Conventional Commits, which this repository does not use.
- **Running the matrix only by hand before tagging**: nothing ties the run to the tag, nor makes sure it ran.
- **Publishing even when the matrix is red**: a release known to fail on a platform.
