#!/usr/bin/env bash
# Run `crcbl-shell`'s Wayland end-to-end suite against a private headless sway.
#
#   crates/crcbl-shell/tests/run-wayland-e2e.sh [extra nextest args…]
#
# The tests are feature-gated *and* `#[ignore]`d, so a plain
# `cargo nextest run --workspace --all-features` on a machine with no compositor
# stays green. This script is the only thing that turns them on, and CI runs
# this script — `docs/plan/12-testing.md` calls a silently-skipped e2e job a
# known trap, so the script fails when the suite reports zero tests run.
#
# Exits non-zero if sway will not start, if the socket does not appear before
# the deadline, if no tests ran, or if any test fails. The compositor log tail is
# printed on every failure path, because "sway exited" with no reason attached
# costs a whole debugging session.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"
CONF="${CRATE_DIR}/tests/wayland-e2e-sway.conf"

# How long to wait for sway to publish its socket. Generous, because a cold CI
# runner starting a compositor for the first time is slow — and bounded, because
# `docs/plan/12-testing.md` requires a deadline rather than a sleep.
SOCKET_TIMEOUT_S="${CRCBL_E2E_SOCKET_TIMEOUT_S:-20}"
POLL_INTERVAL_S=0.1

if ! command -v sway >/dev/null 2>&1; then
    echo "crcbl e2e: sway is not installed; install it or run the harness elsewhere" >&2
    exit 1
fi

# A private XDG_RUNTIME_DIR so this never collides with a real session when a
# developer runs it on a live desktop, and so the socket poll below cannot
# accidentally find someone else's compositor.
RUNTIME_DIR="$(mktemp -d -t crcbl-e2e.XXXXXX)"
chmod 700 "$RUNTIME_DIR"
export XDG_RUNTIME_DIR="$RUNTIME_DIR"
SWAY_LOG="${RUNTIME_DIR}/sway.log"

# wlroots: no real outputs, no real input devices, no GPU required.
export WLR_BACKENDS=headless
export WLR_LIBINPUT_NO_DEVICES=1
export WLR_RENDERER_ALLOW_SOFTWARE=1
# Inherit nothing from an outer session.
unset WAYLAND_DISPLAY
unset DISPLAY

log_tail() {
    echo "--- sway log tail ---" >&2
    tail -n 40 "$SWAY_LOG" >&2 || true
    echo "--- end sway log ---" >&2
}

cleanup() {
    local status=$?
    if [ -n "${SWAY_PID:-}" ]; then
        kill "$SWAY_PID" 2>/dev/null || true
        wait "$SWAY_PID" 2>/dev/null || true
    fi
    rm -rf "$RUNTIME_DIR"
    exit "$status"
}
trap cleanup EXIT INT TERM

echo "crcbl e2e: starting headless sway (XDG_RUNTIME_DIR=$RUNTIME_DIR)"
sway --config "$CONF" >"$SWAY_LOG" 2>&1 &
SWAY_PID=$!

# Poll for the socket with a deadline and a liveness check on the child. Never a
# fixed sleep: a sleep long enough for the slowest runner wastes time on every
# other one, and a sleep short enough to be cheap is a flake.
SOCKET_NAME=""
DEADLINE=$(( $(date +%s) + SOCKET_TIMEOUT_S ))
while [ -z "$SOCKET_NAME" ]; do
    SOCKET_NAME="$(find "$RUNTIME_DIR" -maxdepth 1 -name 'wayland-[0-9]*' ! -name '*.lock' \
        -printf '%f\n' 2>/dev/null | sort | head -1 || true)"
    [ -n "$SOCKET_NAME" ] && break
    if ! kill -0 "$SWAY_PID" 2>/dev/null; then
        echo "crcbl e2e: sway exited before opening a socket" >&2
        log_tail
        exit 1
    fi
    if [ "$(date +%s)" -ge "$DEADLINE" ]; then
        echo "crcbl e2e: no wayland socket in $RUNTIME_DIR after ${SOCKET_TIMEOUT_S}s" >&2
        log_tail
        exit 1
    fi
    sleep "$POLL_INTERVAL_S"
done

export WAYLAND_DISPLAY="$SOCKET_NAME"
# The IPC socket can appear a moment after the display socket, and the close
# and resize tests are driven through it, so poll for it under the same
# deadline rather than racing.
SWAYSOCK=""
while [ -z "$SWAYSOCK" ]; do
    SWAYSOCK="$(find "$RUNTIME_DIR" -maxdepth 1 -name 'sway-ipc.*' -print -quit 2>/dev/null || true)"
    [ -n "$SWAYSOCK" ] && break
    if ! kill -0 "$SWAY_PID" 2>/dev/null; then
        echo "crcbl e2e: sway exited before opening its IPC socket" >&2
        log_tail
        exit 1
    fi
    if [ "$(date +%s)" -ge "$DEADLINE" ]; then
        echo "crcbl e2e: no sway IPC socket after ${SOCKET_TIMEOUT_S}s" >&2
        log_tail
        exit 1
    fi
    sleep "$POLL_INTERVAL_S"
done
export SWAYSOCK
echo "crcbl e2e: socket up at \$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY (sway pid $SWAY_PID)"
echo "crcbl e2e: sway IPC at $SWAYSOCK"

cd "$REPO_ROOT"
OUTPUT="${RUNTIME_DIR}/nextest.log"
set +e
cargo nextest run \
    --locked \
    --package crcbl-shell \
    --features wayland-e2e \
    --test wayland_e2e \
    --run-ignored all \
    --test-threads 1 \
    "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl e2e: the suite failed" >&2
    log_tail
    exit "$STATUS"
fi

# The trap `docs/plan/12-testing.md` names by name: a job that skips everything
# and reports success is worse than no job. `Summary [ 0.1s] 7 tests run: …`
RAN="$(grep -Eo '[0-9]+ tests? run' "$OUTPUT" | tail -1 | grep -Eo '^[0-9]+' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl e2e: the suite reported no tests run — the gate is not gating" >&2
    log_tail
    exit 1
fi
echo "crcbl e2e: $RAN tests ran against headless sway"
