#!/usr/bin/env bash
# Given a release tag, such as v0.2.0, prints the version it names, 0.2.0, when every place
# gikli's version is written down carries that version: the [workspace.package] section of
# Cargo.toml (which src-tauri inherits, and the Tauri config takes as the app's version), the
# version of package.json, and both entries for the root package in package-lock.json. Otherwise
# it prints an ::error:: annotation for each place that differs, naming it and both versions, and
# fails. It also fails when the tag is not of the form vX.Y.Z. The CI workflow runs it on the push
# of a v* tag, before any other job (ADR-0009); run it before tagging to check a release by hand.
#
# Usage: release-version.sh <tag> [checkout root]
#
# Without a checkout root it reads the checkout it belongs to. Needs node, to read the JSON files.
# Tested by release-version.test.sh.
set -u

tag=${1:-}
root=${2:-"$(dirname "$0")/../.."}
if ! [[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "::error::The tag '$tag' is not of the form vX.Y.Z" >&2
  exit 1
fi
version=${tag#v}
failed=false

# Fails, naming the place, when the version found there is not the tag's.
compare() {
  if [ "$2" != "$version" ]; then
    echo "::error::The tag $tag names version $version, but $1 has ${2:-no version}" >&2
    failed=true
  fi
}

# The value of $2, a JavaScript expression of the parsed file, in the JSON file $1.
json() {
  node -e "
    const file = JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'));
    console.log(($2) ?? '');
  " "$1" 2>/dev/null
}

# The version line of the [workspace.package] section, which src-tauri inherits.
compare "Cargo.toml ([workspace.package])" "$(awk '
  /^\[/ { section = $0 }
  section == "[workspace.package]" && match($0, /^version *= *"[^"]*"/) {
    value = substr($0, RSTART, RLENGTH); sub(/^version *= *"/, "", value); sub(/"$/, "", value)
    print value; exit
  }
' "$root/Cargo.toml" 2>/dev/null)"
compare "package.json" "$(json "$root/package.json" "file.version")"
compare "package-lock.json (version)" "$(json "$root/package-lock.json" "file.version")"
compare 'package-lock.json (packages[""].version)' \
  "$(json "$root/package-lock.json" "file.packages?.['']?.version")"

if [ "$failed" = true ]; then
  exit 1
fi
echo "$version"
