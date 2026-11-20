#!/usr/bin/env bash
# Tests docs-only.sh. Run with: bash .github/scripts/docs-only.test.sh
set -u

script="$(cd "$(dirname "$0")" && pwd)/docs-only.sh"
failures=0

fail() {
  echo "FAIL: $test_name: $1"
  failures=$((failures + 1))
}

# Hands docs-only.sh the paths given, one per line, and checks that it prints $expected and exits 0.
classify() {
  local output status
  output=$(printf '%s\n' "$@" | "$script" 2>&1)
  status=$?
  test "$status" -eq 0 || fail "exit status $status, printed $output"
  test "$output" = "$expected" || fail "printed $output, not $expected"
}

test_name="Markdown files at the repository's root are docs"
expected=true classify CLAUDE.md GLOSSARY.md README.md

test_name="files under docs/, at any depth, are docs"
expected=true classify docs/agents/issue-tracker.md docs/adr/0008-ci-in-two-tiers-linux-jobs-in-an-image.md docs/diagram.png

test_name="files under .out-of-scope/ are docs"
expected=true classify .out-of-scope/ui-only-ci.md

test_name="docs and code together are not docs-only"
expected=false classify README.md docs/adr/0008-ci-in-two-tiers-linux-jobs-in-an-image.md crates/gikli-core/src/lib.rs

test_name="a Markdown file under crates/ is code: it may be a fixture"
expected=false classify crates/gikli-core/tests/fixtures/README.md

test_name="a Markdown file under e2e/ is code"
expected=false classify e2e/README.md

test_name="a file under .github/ is code, Markdown or not"
expected=false classify .github/workflows/pr.yml
expected=false classify .github/pull_request_template.md

test_name="a file under .claude/ is code"
expected=false classify .claude/settings.json

test_name="a rename from a code path to a docs path is not docs-only: its old path is code"
expected=false classify crates/gikli-core/NOTES.md docs/notes.md

test_name="a last path without a newline after it still counts"
output=$(printf 'README.md
crates/gikli-core/src/lib.rs' | "$script" 2>&1)
status=$?
test "$status" -eq 0 || fail "exit status $status, printed $output"
test "$output" = "false" || fail "printed $output, not false"

test_name="a list of no files is not docs-only"
output=$("$script" < /dev/null 2>&1)
status=$?
test "$status" -eq 0 || fail "exit status $status, printed $output"
test "$output" = "false" || fail "printed $output, not false"

if [ "$failures" -eq 0 ]; then
  echo "All docs-only.sh tests passed."
fi
[ "$failures" -eq 0 ]
