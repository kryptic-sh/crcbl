#!/usr/bin/env bash
# Publishes every library crate to crates.io, in `check-publish-order.sh`'s
# order, at the workspace version. Run by `ci.yml`'s `publish-crates` on a `v*`
# tag, with `CARGO_REGISTRY_TOKEN` set.
#
# Idempotent: a crate already on crates.io at this version is skipped, so a
# re-run after a timeout or a failure resumes rather than failing on the first
# crate it already uploaded. The helper is tmxr's, which is hjkl's.
#
# `--no-verify`: the tag's `package` job has already built every crate from its
# own tarball against the others' tarballs, on the same commit, and this job
# runs only once that one passed. Verifying again here would spend the
# rate-limited job's hours compiling the engine a crate at a time.
set -euo pipefail

cd "$(dirname "$0")/.."
tools/check-publish-order.sh
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
# How long to wait after each upload, to space the uploads under the rate
# limit, once the index shows the crate.
PACING_SECS=25
# How long an uploaded version may take to reach the sparse index, polled
# every INDEX_POLL_SECS. `cargo publish` waits only a minute and then moves on,
# and on 2026-10-26 the index lagged the API by hours: v0.10.0 and v0.11.0
# both stopped at the first crate whose dependency was uploaded but not yet
# indexed, as `failed to select a version`. The next crate resolves its
# dependencies from the index, so nothing is uploaded until it is there.
INDEX_WAIT_SECS=7200
INDEX_POLL_SECS=30
# How many times a rate-limited upload is retried, each after the wait the
# server names.
RATE_LIMIT_RETRIES=6
# The wait when a 429 names no time this script can parse.
FALLBACK_WAIT_SECS=660
# The least a wait for a time already past is rounded up to.
MIN_WAIT_SECS=15
# How many times an upload crates.io answered with a server error (5xx, or
# its "Application Error" page) is retried, and the wait before each: 0.1.0's
# first publish stopped on one such error, which a re-run then got past.
SERVER_ERROR_RETRIES=3
SERVER_ERROR_WAIT_SECS=60

# Whether `crate` is on crates.io at this version.
# The sparse index's path for a crate, by cargo's rule: one, two and three
# characters long get their own directories, and longer names two levels of
# two characters each.
index_path() {
  local name
  name=$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')
  case "${#name}" in
    1) echo "1/${name}" ;;
    2) echo "2/${name}" ;;
    3) echo "3/${name:0:1}/${name}" ;;
    *) echo "${name:0:2}/${name:2:2}/${name}" ;;
  esac
}

# Waits until the sparse index lists `$1` at VERSION, or fails after
# INDEX_WAIT_SECS.
wait_for_index() {
  local crate="$1" waited=0 url
  url="https://index.crates.io/$(index_path "$crate")"
  until curl -fsS -A "${crate}-release-ci" "$url" | grep -q "\"vers\":\"${VERSION}\""; do
    if [ "$waited" -ge "$INDEX_WAIT_SECS" ]; then
      echo "::error::${crate}@${VERSION} is on crates.io but not in its index after ${INDEX_WAIT_SECS}s"
      return 1
    fi
    sleep "$INDEX_POLL_SECS"
    waited=$((waited + INDEX_POLL_SECS))
  done
  echo "${crate}@${VERSION} is in the index"
}

published() {
  # crates.io answers a request with no User-Agent with a 403.
  curl -fsS -A "${1}-release-ci" -o /dev/null "https://crates.io/api/v1/crates/${1}/${VERSION}"
}

publish_if_missing() {
  local crate="$1"
  local log="/tmp/publish_${crate}.log"
  if published "$crate"; then
    echo "::notice::${crate}@${VERSION} already on crates.io, skipping"
    wait_for_index "$crate"
    return
  fi
  local attempt=0 server_errors=0
  while [ "$attempt" -lt "$RATE_LIMIT_RETRIES" ]; do
    if cargo publish -p "$crate" --locked --no-verify 2>&1 | tee "$log"; then
      wait_for_index "$crate" || return 1
      sleep "$PACING_SECS"
      return 0
    fi
    if grep -qiE "status 5[0-9]{2}|Application Error" "$log"; then
      # The upload may have landed before the error page did; a retry would
      # then fail on a crate that already exists.
      if published "$crate"; then
        echo "::notice::${crate}@${VERSION} landed despite a server error"
        wait_for_index "$crate" || return 1
        sleep "$PACING_SECS"
        return 0
      fi
      server_errors=$((server_errors + 1))
      if [ "$server_errors" -gt "$SERVER_ERROR_RETRIES" ]; then
        echo "::error::publish failed for ${crate} after ${SERVER_ERROR_RETRIES} server-error retries"
        return 1
      fi
      echo "::warning::crates.io server error publishing ${crate}, retrying in ${SERVER_ERROR_WAIT_SECS}s"
      sleep "$SERVER_ERROR_WAIT_SECS"
      continue
    fi
    if ! grep -qiE "429|Too Many Requests" "$log"; then
      echo "::error::publish failed for ${crate} (not a rate limit or a server error)"
      return 1
    fi
    local wait_until wait_secs target
    # Match the RFC 1123 stamp exactly: a looser `try again after [^.]+`
    # runs on into the `crates.io` link that follows it, `date -d` fails, and
    # every wait collapses to the floor — tmxr's v0.8.1 retried before the
    # limit lifted and gave up with crates unpublished.
    wait_until=$(grep -oE 'try again after [A-Za-z]{3}, [0-9]{1,2} [A-Za-z]{3} [0-9]{4} [0-9]{2}:[0-9]{2}:[0-9]{2} GMT' "$log" |
      head -1 | sed 's/try again after //' || true)
    if [ -n "$wait_until" ] && target=$(date -d "$wait_until" +%s 2>/dev/null); then
      wait_secs=$((target - $(date +%s) + MIN_WAIT_SECS))
      # A floor for a stamp already past, never a cap: waiting less than the
      # server asked burns an attempt and pushes the next slot further out.
      [ "$wait_secs" -lt "$MIN_WAIT_SECS" ] && wait_secs=$MIN_WAIT_SECS
      echo "::warning::rate limited publishing ${crate}, sleeping ${wait_secs}s until ${wait_until}"
    else
      wait_secs=$FALLBACK_WAIT_SECS
      echo "::warning::rate limited publishing ${crate}, sleeping ${wait_secs}s (no parsable stamp)"
    fi
    sleep "$wait_secs"
    attempt=$((attempt + 1))
  done
  echo "::error::publish failed for ${crate} after ${RATE_LIMIT_RETRIES} rate-limit retries"
  return 1
}

for crate in $(tools/check-publish-order.sh --print); do
  publish_if_missing "$crate"
done
