#!/usr/bin/env bash
# Runs a command that reaches an outside server, such as Ubuntu's package servers or Launchpad,
# until it succeeds, a few times at most. Each try is stopped once it has run for <seconds per try>,
# so a server that does not answer cannot hang the job. When every try fails, it prints an
# ::error:: annotation naming the command and the server, and exits with the last try's status.
#
# Usage: retry.sh <what it reaches> <seconds per try> <command> [arguments...]
#
# RETRY_TRIES (default 3) and RETRY_WAIT (seconds between tries, default 15) override the defaults.
# Tested by retry.test.sh.
set -u

what=$1
seconds=$2
shift 2
tries=${RETRY_TRIES:-3}
wait=${RETRY_WAIT:-15}

for ((try = 1; ; try++)); do
  # --kill-after: a command that ignores the TERM still stops.
  timeout --kill-after=10 "$seconds" "$@" && exit 0
  status=$?
  # timeout exits with 124 when it stopped the try, and with 137 when it had to kill it.
  if [ "$status" -eq 124 ] || [ "$status" -eq 137 ]; then
    reason="timed out after $seconds seconds"
  else
    reason="exited with status $status"
  fi
  if [ "$try" -ge "$tries" ]; then
    echo "::error::$* gave up after $tries tries: could not reach $what (the last try $reason)"
    exit "$status"
  fi
  echo "Try $try of $tries of $* $reason. Trying again in $wait seconds."
  sleep "$wait"
done
