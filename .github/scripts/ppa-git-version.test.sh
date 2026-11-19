#!/usr/bin/env bash
# Tests ppa-git-version.sh. Run with: bash .github/scripts/ppa-git-version.test.sh
set -u

script="$(cd "$(dirname "$0")" && pwd)/ppa-git-version.sh"
failures=0

fail() {
  echo "FAIL: $test_name: $1"
  failures=$((failures + 1))
}

test_name="a git from ppa:git-core/ppa gives its version without epoch and revision"
output=$("$script" "1:2.51.1-0ppa1~ubuntu24.04.1" 2>&1)
status=$?
test "$status" -eq 0 || fail "exit status $status"
test "$output" = "2.51.1" || fail "printed $output"

test_name="Ubuntu's own git is refused"
output=$("$script" "1:2.43.0-1ubuntu7.3" 2>&1)
status=$?
test "$status" -ne 0 || fail "exit status 0"
[[ $output == "::error::git 1:2.43.0-1ubuntu7.3 is installed, not the latest stable git from ppa:git-core/ppa" ]] ||
  fail "printed $output"

test_name="no git package at all is refused"
output=$("$script" "" 2>&1)
status=$?
test "$status" -ne 0 || fail "exit status 0"
[[ $output == "::error::no git package is installed, so not the latest stable git from ppa:git-core/ppa" ]] ||
  fail "printed $output"

if [ "$failures" -eq 0 ]; then
  echo "All ppa-git-version.sh tests passed."
fi
[ "$failures" -eq 0 ]
