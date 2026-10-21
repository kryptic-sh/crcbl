#!/usr/bin/env bash
# The Wayland e2e suite against Hyprland, from inside a running Hyprland.
#
#   crates/crcbl-shell/tests/run-wayland-e2e-hyprland.sh [extra nextest args…]
#
# `run-wayland-e2e.sh` is the suite against headless sway, and the one CI
# runs. This runs the same `tests/wayland_e2e.rs` against a nested Hyprland
# that `hyprland-session.sh` starts on a hidden special workspace, so a
# developer on Hyprland can see what that compositor does with every case the
# suite asserts. The tests drive either compositor through their `wm_`
# functions, which `CRCBL_E2E_COMPOSITOR=hyprland` points at `hyprctl`.
#
# What it does not run, and why:
#
#   * The sandbox passes: they resize, fullscreen and power outputs through
#     `swaymsg` from the shell and are `run-wayland-e2e.sh`'s alone.
#   * `SWAY_ONLY` below: tests that assert sway's answer where Hyprland's
#     differs. Each is excluded by name, printed, and counted, so an exclusion
#     is never a pass.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# shellcheck source=crates/crcbl-shell/tests/hyprland-session.sh
source "${CRATE_DIR}/tests/hyprland-session.sh"
# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

# Each of these asserts sway's answer to a question Hyprland 0.56.2 answers
# differently, diagnosed from a `WAYLAND_DEBUG=1` run against this harness; in
# none is crcbl's side at fault.
#
#   * `server_side_decorations_are_negotiated_rather_than_assumed` reads sway's
#     per-window `border` field; Hyprland has no such field.
#   * `a_frame_request_waits_while_the_output_is_off`: Hyprland keeps answering
#     frame requests for surfaces it is not showing (`misc.render_unfocused_fps`),
#     so a client there keeps rendering while its output is dark.
#   * `a_touchpad_scroll_is_reported_as_pixels_not_as_detents`: the virtual
#     pointer sends a finger-source scroll, and Hyprland passes it on as a wheel
#     with an `axis_value120`, which crcbl rightly reads as detents.
#   * `the_pointer_outside_an_input_region_leaves_the_window`: crcbl sets and
#     commits the region, and Hyprland sends motion outside it on the
#     fullscreen test window rather than a leave.
#   * `a_seat_that_loses_its_devices_drops_focus_and_keeps_running`: Hyprland's
#     seat advertises a pointer and a keyboard from the start and keeps them
#     when the virtual devices go, so nothing is lost to report.
SWAY_ONLY=(
    server_side_decorations_are_negotiated_rather_than_assumed
    a_frame_request_waits_while_the_output_is_off
    a_touchpad_scroll_is_reported_as_pixels_not_as_detents
    the_pointer_outside_an_input_region_leaves_the_window
    a_seat_that_loses_its_devices_drops_focus_and_keeps_running
)

# What this script writes into the session's directory, removed by name before
# `hyprland_session_stop` removes the directory; anything else an application
# under test left there stays, and the directory with it.
cleanup() {
    local status=$?
    if [ -n "${HYPR_RUNTIME_DIR:-}" ]; then
        rm -f "${HYPR_RUNTIME_DIR}/nextest.log" "${HYPR_RUNTIME_DIR}/nextest.plain.log"
        rmdir "${HYPR_RUNTIME_DIR}/state" "${HYPR_RUNTIME_DIR}/cache" 2>/dev/null || true
    fi
    hyprland_session_stop
    exit "$status"
}
trap cleanup EXIT INT TERM

hyprland_session_start

filter="all()"
for test in "${SWAY_ONLY[@]}"; do
    echo "crcbl e2e: not run on Hyprland: $test (asserts sway's behaviour; see this script)"
    filter="${filter} - test(=${test})"
done

cd "$REPO_ROOT"
export XCOMPOSEFILE="${REPO_ROOT}/crates/crcbl-shell/tests/e2e.XCompose"
# The applications under test keep their state out of the developer's home.
export XDG_STATE_HOME="${HYPR_RUNTIME_DIR}/state"
export XDG_CACHE_HOME="${HYPR_RUNTIME_DIR}/cache"
OUTPUT="${HYPR_RUNTIME_DIR}/nextest.log"
set +e
cargo nextest run \
    --locked \
    --package crcbl-shell \
    --features wayland-e2e \
    --test wayland_e2e \
    --run-ignored all \
    --test-threads 1 \
    -E "$filter" \
    "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e
PLAIN="${HYPR_RUNTIME_DIR}/nextest.plain.log"
crcbl_nextest_plain "$OUTPUT" "$PLAIN"
if [ "$STATUS" -ne 0 ] || ! crcbl_nextest_summary "$PLAIN" "crcbl e2e"; then
    echo "crcbl e2e: the suite failed against Hyprland" >&2
    hyprland_log_tail
    exit 1
fi
echo "crcbl e2e: $CRCBL_NEXTEST_TESTS_RUN tests ran against a nested Hyprland; ${#SWAY_ONLY[@]} assert sway's behaviour and were not run"
echo "crcbl e2e: the sandbox passes are sway-only; run tests/run-wayland-e2e.sh for them"
