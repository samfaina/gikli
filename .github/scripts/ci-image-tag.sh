#!/usr/bin/env bash
# Prints the tag that names one definition of the CI image, such as git-2.34.0-0123456789abcdef:
# the minimum git it holds and a hash of what builds it, .github/ci-image/Dockerfile and
# .github/workflows/ci-image.yml. A change to any of them gives another tag. The image is
# published under this tag beside :latest, so the CI workflow on main can tell whether the image
# for its own definition exists yet, and builds it when it does not.
#
# Usage: ci-image-tag.sh [checkout root]
#
# Without an argument it reads the checkout it belongs to. Fails, with the ::error:: annotation of
# minimum-git-version.sh, when the checkout declares no MINIMUM_GIT_VERSION.
# Tested by ci-image-tag.test.sh.
set -u -o pipefail

root=${1:-"$(dirname "$0")/../.."}
version=$("$(dirname "$0")/minimum-git-version.sh" "$root/crates/gikli-core/src/lib.rs") || exit 1
definition=$(cat "$root/.github/ci-image/Dockerfile" "$root/.github/workflows/ci-image.yml" | sha256sum) ||
  exit 1
echo "git-$version-${definition:0:16}"
