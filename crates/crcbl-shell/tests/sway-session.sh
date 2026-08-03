#!/usr/bin/env bash
# A private headless sway, for any harness that needs a real compositor.
#
# **Sourced, never run.** Every function here sets variables in the caller's
# shell and exits it on failure, which is the behaviour the two callers had when
# each carried its own copy:
#
#   source "…/crates/crcbl-shell/tests/sway-session.sh"
#   sway_session_start "…/my-sway.conf"
#   # $SWAY_RUNTIME_DIR, $WAYLAND_DISPLAY, $SWAYSOCK, $SWAY_LOG are now set,
#   # and sway_log_tail prints the compositor's log.
#   sway_session_stop
#
# `crcbl-shell`'s Wayland suite owns this because starting a compositor is its
# knowledge, but `crcbl-cli`'s scaffold suite sources it too: a generated
# project has a windowed path, and the only way to find out whether it opens a
# window is to give it a window system to open one on.
#
# Each caller brings its own config. What a harness needs from sway is mostly
# `for_window` rules for the `app_id`s it is about to assert geometry on, and
# those are the caller's business — a shared config would collect every
# harness's rules and nobody would know which line was load-bearing for which.

# How long to wait for sway to publish its sockets. Generous, because a cold CI
# runner starting a compositor for the first time is slow — and bounded, because
# `docs/plan/12-testing.md` requires a deadline rather than a sleep.
SWAY_SESSION_TIMEOUT_S="${CRCBL_E2E_SOCKET_TIMEOUT_S:-20}"
SWAY_SESSION_POLL_S=0.1

# Prints the compositor's log tail, which every failure path wants: "sway
# exited" with no reason attached costs a whole debugging session.
sway_log_tail() {
    echo "--- sway log tail ---" >&2
    tail -n 40 "$SWAY_LOG" >&2 || true
    echo "--- end sway log ---" >&2
}

# `sway_session_start <config path>`
sway_session_start() {
    local config="$1"

    if ! command -v sway >/dev/null 2>&1; then
        echo "crcbl e2e: sway is not installed; install it or run the harness elsewhere" >&2
        exit 1
    fi

    # A private XDG_RUNTIME_DIR so this never collides with a real session when
    # a developer runs it on a live desktop, and so the socket poll below cannot
    # accidentally find someone else's compositor.
    SWAY_RUNTIME_DIR="$(mktemp -d -t crcbl-e2e.XXXXXX)"
    chmod 700 "$SWAY_RUNTIME_DIR"
    export XDG_RUNTIME_DIR="$SWAY_RUNTIME_DIR"
    SWAY_LOG="${SWAY_RUNTIME_DIR}/sway.log"

    # wlroots: no real outputs, no real input devices, no GPU required.
    export WLR_BACKENDS=headless
    export WLR_LIBINPUT_NO_DEVICES=1
    export WLR_RENDERER_ALLOW_SOFTWARE=1
    # Inherit nothing from an outer session.
    unset WAYLAND_DISPLAY
    unset DISPLAY

    echo "crcbl e2e: starting headless sway (XDG_RUNTIME_DIR=$SWAY_RUNTIME_DIR)"
    sway --config "$config" >"$SWAY_LOG" 2>&1 &
    SWAY_PID=$!

    # Poll for the socket with a deadline and a liveness check on the child.
    # Never a fixed sleep: a sleep long enough for the slowest runner wastes
    # time on every other one, and a sleep short enough to be cheap is a flake.
    local socket_name=""
    local deadline=$(( $(date +%s) + SWAY_SESSION_TIMEOUT_S ))
    while [ -z "$socket_name" ]; do
        socket_name="$(find "$SWAY_RUNTIME_DIR" -maxdepth 1 -name 'wayland-[0-9]*' ! -name '*.lock' \
            -printf '%f\n' 2>/dev/null | sort | head -1 || true)"
        [ -n "$socket_name" ] && break
        if ! kill -0 "$SWAY_PID" 2>/dev/null; then
            echo "crcbl e2e: sway exited before opening a socket" >&2
            sway_log_tail
            exit 1
        fi
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "crcbl e2e: no wayland socket in $SWAY_RUNTIME_DIR after ${SWAY_SESSION_TIMEOUT_S}s" >&2
            sway_log_tail
            exit 1
        fi
        sleep "$SWAY_SESSION_POLL_S"
    done
    export WAYLAND_DISPLAY="$socket_name"

    # The IPC socket can appear a moment after the display socket, and the close
    # and resize tests are driven through it, so poll for it under the same
    # deadline rather than racing.
    SWAYSOCK=""
    while [ -z "$SWAYSOCK" ]; do
        SWAYSOCK="$(find "$SWAY_RUNTIME_DIR" -maxdepth 1 -name 'sway-ipc.*' -print -quit 2>/dev/null || true)"
        [ -n "$SWAYSOCK" ] && break
        if ! kill -0 "$SWAY_PID" 2>/dev/null; then
            echo "crcbl e2e: sway exited before opening its IPC socket" >&2
            sway_log_tail
            exit 1
        fi
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "crcbl e2e: no sway IPC socket after ${SWAY_SESSION_TIMEOUT_S}s" >&2
            sway_log_tail
            exit 1
        fi
        sleep "$SWAY_SESSION_POLL_S"
    done
    export SWAYSOCK

    echo "crcbl e2e: socket up at \$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY (sway pid $SWAY_PID)"
    echo "crcbl e2e: sway IPC at $SWAYSOCK"
}

# Kills the compositor and removes its runtime directory. Safe to call twice,
# and safe to call when `sway_session_start` never ran — an `EXIT` trap fires
# on the failure paths above too.
sway_session_stop() {
    if [ -n "${SWAY_PID:-}" ]; then
        kill "$SWAY_PID" 2>/dev/null || true
        wait "$SWAY_PID" 2>/dev/null || true
        SWAY_PID=""
    fi
    if [ -n "${SWAY_RUNTIME_DIR:-}" ]; then
        rm -rf "$SWAY_RUNTIME_DIR"
        SWAY_RUNTIME_DIR=""
    fi
}
