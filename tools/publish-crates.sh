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
# How long to wait after each upload: `cargo publish` already waits for the
# index to show the crate, and this spaces the uploads under the rate limit.
PACING_SECS=25
# How many times a rate-limited upload is retried, each after the wait the
# server names.
RATE_LIMIT_RETRIES=6
# The wait when a 429 names no time this script can parse.
FALLBACK_WAIT_SECS=660
# The least a wait for a time already past is rounded up to.
MIN_WAIT_SECS=15

publish_if_missing() {
  local crate="$1"
  local url="https://crates.io/api/v1/crates/${crate}/${VERSION}"
  local log="/tmp/publish_${crate}.log"
  # crates.io answers a request with no User-Agent with a 403.
  if curl -fsS -A "${crate}-release-ci" -o /dev/null "$url"; then
    echo "::notice::${crate}@${VERSION} already on crates.io, skipping"
    return 0
  fi
  local attempt=0
  while [ "$attempt" -lt "$RATE_LIMIT_RETRIES" ]; do
    if cargo publish -p "$crate" --locked --no-verify 2>&1 | tee "$log"; then
      sleep "$PACING_SECS"
      return 0
    fi
    if ! grep -qiE "429|Too Many Requests" "$log"; then
      echo "::error::publish failed for ${crate} (not a rate limit)"
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
