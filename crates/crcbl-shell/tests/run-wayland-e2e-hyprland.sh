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
#   * `SWAY_ONLY` below: tests that read something only sway has. Each is
#     excluded by name, printed, and counted, so an exclusion is never a pass.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# shellcheck source=crates/crcbl-shell/tests/hyprland-session.sh
source "${CRATE_DIR}/tests/hyprland-session.sh"
# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

# `server_side_decorations_are_negotiated_rather_than_assumed` reads sway's
# per-window `border` field, which says whether the client draws its own
# decorations; Hyprland has no such field to read.
#
# `a_frame_request_waits_while_the_output_is_off` asserts that a powered-off
# output answers no frame request. Hyprland answers anyway, by design: it keeps
# sending frame callbacks to surfaces it is not showing (`misc.render_unfocused_fps`),
# so a client there keeps rendering, at a reduced rate, while its output is dark.
SWAY_ONLY=(
    server_side_decorations_are_negotiated_rather_than_assumed
    a_frame_request_waits_while_the_output_is_off
)

# Seen to differ on Hyprland 0.56.2 and **not yet diagnosed** — whether each is
# Hyprland's behaviour or crcbl's defect is open in `docs/backlog.md` ("Hyprland
# runs nested inside the live Hyprland"). Not run here until it is, and printed
# as such, so a run is not read as covering them.
UNDIAGNOSED=(
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
    echo "crcbl e2e: not run on Hyprland: $test (sway-only)"
    filter="${filter} - test(=${test})"
done
for test in "${UNDIAGNOSED[@]}"; do
    echo "crcbl e2e: not run on Hyprland: $test (differs there, undiagnosed; see docs/backlog.md)"
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
echo "crcbl e2e: $CRCBL_NEXTEST_TESTS_RUN tests ran against a nested Hyprland; not run: ${#SWAY_ONLY[@]} sway-only, ${#UNDIAGNOSED[@]} undiagnosed"
echo "crcbl e2e: the sandbox passes are sway-only; run tests/run-wayland-e2e.sh for them"
