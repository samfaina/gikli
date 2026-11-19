#!/usr/bin/env bash
# Given the version of the installed git package, as `dpkg-query -W -f='${Version}' git` prints
# it, prints the git version it holds, such as 2.51.1 for 1:2.51.1-0ppa1~ubuntu24.04.1. It fails
# with an ::error:: annotation when that package is not the latest stable git from
# ppa:git-core/ppa, such as Ubuntu's own 1:2.43.0-1ubuntu7.3, so CI never tests Ubuntu's git in
# its place.
#
# Usage: ppa-git-version.sh <package version>
#
# Tested by ppa-git-version.test.sh.
set -u

package_version=${1:-}
case "$package_version" in
  "")
    echo "::error::no git package is installed, so not the latest stable git from ppa:git-core/ppa" >&2
    exit 1
    ;;
  *ppa*) ;;
  *)
    echo "::error::git $package_version is installed, not the latest stable git from ppa:git-core/ppa" >&2
    exit 1
    ;;
esac
# Without the epoch (1:) and the revision (-0ppa1~ubuntu24.04.1).
sed -E 's/^[0-9]+://; s/-.*//' <<< "$package_version"
