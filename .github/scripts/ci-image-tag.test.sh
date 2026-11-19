#!/usr/bin/env bash
# Tests ci-image-tag.sh. Run with: bash .github/scripts/ci-image-tag.test.sh
set -u

script="$(cd "$(dirname "$0")" && pwd)/ci-image-tag.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

fail() {
  echo "FAIL: $test_name: $1"
  failures=$((failures + 1))
}

# A checkout at $work/$1 whose image definition is $2 and whose minimum git is 2.$3.0.
checkout() {
  mkdir -p "$work/$1/.github/ci-image" "$work/$1/crates/gikli-core/src"
  printf '%s\n' "$2" > "$work/$1/.github/ci-image/Dockerfile"
  printf 'pub const MINIMUM_GIT_VERSION: GitVersion = GitVersion::new(2, %s, 0);\n' "$3" \
    > "$work/$1/crates/gikli-core/src/lib.rs"
}

tag() { "$script" "$work/$1" 2>&1; }

checkout one "FROM ubuntu:24.04" 34
checkout same "FROM ubuntu:24.04" 34
checkout other-definition "FROM ubuntu:26.04" 34
checkout other-minimum "FROM ubuntu:24.04" 35

test_name="the tag names the minimum git and is a valid image tag"
output=$(tag one)
status=$?
test "$status" -eq 0 || fail "exit status $status"
[[ $output =~ ^git-2\.34\.0-[0-9a-f]{16}$ ]] || fail "printed $output"

test_name="the same definition and minimum git give the same tag"
test "$(tag same)" = "$(tag one)" || fail "$(tag same) and $(tag one)"

test_name="another image definition gives another tag"
test "$(tag other-definition)" != "$(tag one)" || fail "both are $(tag one)"

test_name="another minimum git gives another tag"
test "$(tag other-minimum)" != "$(tag one)" || fail "both are $(tag one)"

test_name="without an argument it names the image of the checkout it belongs to"
output=$(cd "$work" && "$script" 2>&1)
status=$?
test "$status" -eq 0 || fail "exit status $status"
[[ $output =~ ^git-[0-9.]+-[0-9a-f]{16}$ ]] || fail "printed $output"

test_name="a checkout without a minimum git is an error"
rm "$work/one/crates/gikli-core/src/lib.rs"
output=$(tag one)
status=$?
test "$status" -ne 0 || fail "exit status 0, printed $output"
[[ $output == "::error::MINIMUM_GIT_VERSION not found in"* ]] || fail "printed $output"

if [ "$failures" -eq 0 ]; then
  echo "All ci-image-tag.sh tests passed."
fi
[ "$failures" -eq 0 ]
