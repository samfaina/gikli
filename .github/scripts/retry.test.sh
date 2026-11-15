#!/usr/bin/env bash
# Tests retry.sh. Run with: bash .github/scripts/retry.test.sh
set -u

retry="$(dirname "$0")/retry.sh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0

fail() {
  echo "FAIL: $test_name: $1"
  failures=$((failures + 1))
}

# A command that fails the first $1 times it runs, then succeeds. It counts its runs in $work/runs.
flaky() {
  cat > "$work/flaky" <<EOF
#!/usr/bin/env bash
runs=\$(( \$(cat "$work/runs" 2>/dev/null || echo 0) + 1 ))
echo "\$runs" > "$work/runs"
echo "run \$runs"
test "\$runs" -gt $1
EOF
  chmod +x "$work/flaky"
  rm -f "$work/runs"
}

runs() { cat "$work/runs" 2>/dev/null || echo 0; }

export RETRY_WAIT=0

test_name="a command that succeeds runs once"
flaky 0
output=$("$retry" "the test server" 5 "$work/flaky" 2>&1)
status=$?
test "$status" -eq 0 || fail "exit status $status"
test "$(runs)" -eq 1 || fail "ran $(runs) times"
[[ $output == *"run 1"* ]] || fail "the command's output is missing: $output"

test_name="a command that fails is tried again until it succeeds"
flaky 2
output=$(RETRY_TRIES=3 "$retry" "the test server" 5 "$work/flaky" 2>&1)
status=$?
test "$status" -eq 0 || fail "exit status $status"
test "$(runs)" -eq 3 || fail "ran $(runs) times"
[[ $output != *"::error::"* ]] || fail "reported an error: $output"

test_name="a command that keeps failing fails with an error naming the command and what it reaches"
flaky 99
output=$(RETRY_TRIES=3 "$retry" "the test server" 5 "$work/flaky" --some-option 2>&1)
status=$?
test "$status" -ne 0 || fail "exit status $status"
test "$(runs)" -eq 3 || fail "ran $(runs) times"
error=$(grep '^::error::' <<< "$output")
[[ $error == *"$work/flaky --some-option"* ]] || fail "the error does not name the command: $output"
[[ $error == *"the test server"* ]] || fail "the error does not name what the command reaches: $output"
[[ $error == *"3 tries"* ]] || fail "the error does not say how many tries were made: $output"

test_name="a try that hangs is stopped at its time bound and tried again"
started=$SECONDS
output=$(RETRY_TRIES=2 "$retry" "the test server" 1 sleep 30 2>&1)
status=$?
elapsed=$((SECONDS - started))
test "$status" -ne 0 || fail "exit status $status"
test "$elapsed" -lt 10 || fail "took $elapsed seconds"
error=$(grep '^::error::' <<< "$output")
[[ $error == *"sleep 30"* ]] || fail "the error does not name the command: $output"
[[ $error == *"timed out"* ]] || fail "the error does not say the last try timed out: $output"

if [ "$failures" -gt 0 ]; then
  echo "$failures failure(s)"
  exit 1
fi
echo "All tests passed"
