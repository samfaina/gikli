#!/usr/bin/env bash
# Tests docs-only.sh. Run with: bash .github/scripts/docs-only.test.sh
set -u

script="$(cd "$(dirname "$0")" && pwd)/docs-only.sh"
failures=0

fail() {
  echo "FAIL: $test_name: $1"
  failures=$((failures + 1))
}

# Hands docs-only.sh its stdin, and checks that it prints $1 and exits 0.
check() {
  local output status
  output=$("$script" 2>&1)
  status=$?
  test "$status" -eq 0 || fail "exit status $status, printed $output"
  test "$output" = "$1" || fail "printed $output, not $1"
}

# The paths given, one per line.
paths() { printf '%s\n' "$@"; }

test_name="Markdown files at the repository's root are docs"
paths CLAUDE.md GLOSSARY.md README.md | check true

test_name="files under docs/, at any depth, are docs"
paths docs/agents/issue-tracker.md docs/adr/0008-ci-in-two-tiers-linux-jobs-in-an-image.md docs/diagram.png | check true

test_name="files under .out-of-scope/ are docs"
paths .out-of-scope/ui-only-ci.md | check true

test_name="docs and code together are not docs-only"
paths README.md docs/adr/0008-ci-in-two-tiers-linux-jobs-in-an-image.md crates/gikli-core/src/lib.rs | check false

test_name="a Markdown file under crates/ is code: it may be a fixture"
paths crates/gikli-core/tests/fixtures/README.md | check false

test_name="a Markdown file under e2e/ is code"
paths e2e/README.md | check false

test_name="a file under .github/ is code, Markdown or not"
paths .github/workflows/pr.yml | check false
paths .github/pull_request_template.md | check false

test_name="a file under .claude/ is code"
paths .claude/settings.json | check false

test_name="a rename from a code path to a docs path is not docs-only: its old path is code"
paths crates/gikli-core/NOTES.md docs/notes.md | check false

test_name="a last path without a newline after it still counts"
printf 'README.md\ncrates/gikli-core/src/lib.rs' | check false

test_name="a list of no files is not docs-only"
check false < /dev/null

if [ "$failures" -eq 0 ]; then
  echo "All docs-only.sh tests passed."
fi
[ "$failures" -eq 0 ]
