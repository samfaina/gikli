#!/usr/bin/env bash
# Tests release-version.sh. Run with: bash .github/scripts/release-version.test.sh
set -u

script="$(cd "$(dirname "$0")" && pwd)/release-version.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

fail() {
  echo "FAIL: $test_name: $1"
  failures=$((failures + 1))
}

# A checkout at $work/$1 whose Cargo.toml's [workspace.package] has version $2, whose package.json
# has $3, and whose package-lock.json has $4 at its top and $5 for its root package.
checkout() {
  mkdir -p "$work/$1"
  cat > "$work/$1/Cargo.toml" <<EOF
[workspace]
resolver = "3"
members = ["crates/gikli-core"]

[workspace.package]
edition = "2024"
version = "$2"
license = "Apache-2.0"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
EOF
  cat > "$work/$1/package.json" <<EOF
{
  "name": "gikli",
  "private": true,
  "version": "$3",
  "dependencies": { "vite": "7.3.7" }
}
EOF
  cat > "$work/$1/package-lock.json" <<EOF
{
  "name": "gikli",
  "version": "$4",
  "lockfileVersion": 3,
  "packages": {
    "": { "name": "gikli", "version": "$5" },
    "node_modules/vite": { "version": "7.3.7" }
  }
}
EOF
}

check() { "$script" "$1" "$work/$2" 2>&1; }

test_name="a tag whose version every file carries gives that version"
checkout matching 0.2.0 0.2.0 0.2.0 0.2.0
output=$(check v0.2.0 matching)
status=$?
test "$status" -eq 0 || fail "exit status $status, printed $output"
test "$output" = "0.2.0" || fail "printed $output"

test_name="another version in Cargo.toml's [workspace.package] is an error that names the file and both versions"
checkout cargo 0.1.0 0.2.0 0.2.0 0.2.0
output=$(check v0.2.0 cargo)
status=$?
test "$status" -ne 0 || fail "exit status 0, printed $output"
[[ $output == "::error::The tag v0.2.0 names version 0.2.0, but Cargo.toml ([workspace.package]) has 0.1.0" ]] ||
  fail "printed $output"

test_name="another version in package.json is an error that names the file and both versions"
checkout package 0.2.0 0.1.0 0.2.0 0.2.0
output=$(check v0.2.0 package)
status=$?
test "$status" -ne 0 || fail "exit status 0, printed $output"
[[ $output == "::error::The tag v0.2.0 names version 0.2.0, but package.json has 0.1.0" ]] ||
  fail "printed $output"

test_name="another version at the top of package-lock.json is an error that names the entry and both versions"
checkout lock-top 0.2.0 0.2.0 0.1.0 0.2.0
output=$(check v0.2.0 lock-top)
status=$?
test "$status" -ne 0 || fail "exit status 0, printed $output"
[[ $output == "::error::The tag v0.2.0 names version 0.2.0, but package-lock.json (version) has 0.1.0" ]] ||
  fail "printed $output"

test_name="another version for package-lock.json's root package is an error that names the entry and both versions"
checkout lock-root 0.2.0 0.2.0 0.2.0 0.1.0
output=$(check v0.2.0 lock-root)
status=$?
test "$status" -ne 0 || fail "exit status 0, printed $output"
[[ $output == '::error::The tag v0.2.0 names version 0.2.0, but package-lock.json (packages[""].version) has 0.1.0' ]] ||
  fail "printed $output"

test_name="a file without a version is an error that names the file"
checkout no-version 0.2.0 0.2.0 0.2.0 0.2.0
sed -i '/^version = /d' "$work/no-version/Cargo.toml"
rm "$work/no-version/package.json"
output=$(check v0.2.0 no-version)
status=$?
test "$status" -ne 0 || fail "exit status 0, printed $output"
[[ $output == *"::error::The tag v0.2.0 names version 0.2.0, but Cargo.toml ([workspace.package]) has no version"* ]] ||
  fail "printed $output"
[[ $output == *"::error::The tag v0.2.0 names version 0.2.0, but package.json has no version"* ]] ||
  fail "printed $output"

test_name="without a checkout root it reads the checkout it belongs to, from any folder"
output=$(cd "$work" && "$script" v999.0.0 2>&1)
status=$?
test "$status" -ne 0 || fail "exit status 0, printed $output"
for place in 'Cargo.toml ([workspace.package])' package.json 'package-lock.json (version)' \
  'package-lock.json (packages[""].version)'; do
  [[ $output == *"::error::The tag v999.0.0 names version 999.0.0, but $place has "[0-9]* ]] ||
    fail "printed $output"
done

for tag in v0.2 0.2.0 v0.2.0-rc.1 ""; do
  test_name="the tag '$tag', not vX.Y.Z, is an error that names it"
  output=$(check "$tag" matching)
  status=$?
  test "$status" -ne 0 || fail "exit status 0, printed $output"
  [[ $output == "::error::The tag '$tag' is not of the form vX.Y.Z" ]] || fail "printed $output"
done

if [ "$failures" -eq 0 ]; then
  echo "All release-version.sh tests passed."
fi
[ "$failures" -eq 0 ]
