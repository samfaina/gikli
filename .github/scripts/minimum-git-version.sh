#!/usr/bin/env bash
# Prints MINIMUM_GIT_VERSION, the oldest git gikli supports, as gikli-core declares it, such as
# 2.34.0. gikli-core is the only place that version is written down: the CI jobs that test against
# that git and the CI image that holds it read it here.
#
# Usage: minimum-git-version.sh [path to lib.rs]
#
# Without an argument it reads crates/gikli-core/src/lib.rs of the checkout it belongs to. When the
# file declares no such constant, it prints an ::error:: annotation naming the file and fails.
# Tested by minimum-git-version.test.sh.
set -u

file=${1:-"$(dirname "$0")/../../crates/gikli-core/src/lib.rs"}
version=$(sed -nE 's/.*MINIMUM_GIT_VERSION: GitVersion = GitVersion::new\(([0-9]+), ([0-9]+), ([0-9]+)\);.*/\1.\2.\3/p' \
  "$file" 2>/dev/null)
if [ -z "$version" ]; then
  echo "::error::MINIMUM_GIT_VERSION not found in $file" >&2
  exit 1
fi
echo "$version"
