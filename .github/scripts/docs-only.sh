#!/usr/bin/env bash
# Says whether a change is docs-only: reads the paths it changes on stdin, one per line, and prints
# true when every one of them counts as docs, false otherwise. A change with no paths is not
# docs-only. For a renamed file, give both its old path and its new one.
#
# A path counts as docs when it matches one of these globs, and nothing else does (ADR-0008):
#
#   *.md              at the repository's root only, such as README.md
#   docs/**           the ADRs and docs/agents/ among them
#   .out-of-scope/**
#
# No build or test reads them. A Markdown file anywhere else, such as under crates/ or e2e/, is
# code: it may be a fixture. So are .github/ and .claude/.
#
# The PR gate (pr.yml) skips its Windows job on a docs-only pull request. ci.yml's push trigger
# repeats the three globs as its paths-ignore, since paths-ignore cannot call a script: keep the two
# in step.
#
# Usage: docs-only.sh < paths
# Tested by docs-only.test.sh.
set -u

paths=0
while IFS= read -r path || [ -n "$path" ]; do
  paths=$((paths + 1))
  case $path in
    docs/* | .out-of-scope/*) ;;
    */*) echo false; exit 0 ;;
    *.md) ;;
    *) echo false; exit 0 ;;
  esac
done
if [ "$paths" -eq 0 ]; then
  echo false
else
  echo true
fi
