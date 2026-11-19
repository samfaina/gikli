#!/usr/bin/env bash
# Tests minimum-git-version.sh. Run with: bash .github/scripts/minimum-git-version.test.sh
set -u

script="$(cd "$(dirname "$0")" && pwd)/minimum-git-version.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

fail() {
  echo "FAIL: $test_name: $1"
  failures=$((failures + 1))
}

test_name="it reads the version gikli-core declares"
cat > "$work/lib.rs" <<'EOF'
/// The oldest git gikli runs operations with.
pub const MINIMUM_GIT_VERSION: GitVersion = GitVersion::new(2, 41, 3);
EOF
output=$("$script" "$work/lib.rs" 2>&1)
status=$?
test "$status" -eq 0 || fail "exit status $status"
test "$output" = "2.41.3" || fail "printed $output"

test_name="without an argument it reads gikli-core's own lib.rs, from any folder"
output=$(cd "$work" && "$script" 2>&1)
status=$?
test "$status" -eq 0 || fail "exit status $status"
[[ $output =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "printed $output"

test_name="a file without the constant is an error that names the file"
echo "pub const OTHER: u32 = 1;" > "$work/lib.rs"
output=$("$script" "$work/lib.rs" 2>&1)
status=$?
test "$status" -ne 0 || fail "exit status 0"
[[ $output == "::error::MINIMUM_GIT_VERSION not found in $work/lib.rs" ]] || fail "printed $output"

if [ "$failures" -eq 0 ]; then
  echo "All minimum-git-version.sh tests passed."
fi
[ "$failures" -eq 0 ]
