#!/usr/bin/env bash
# A nested Hyprland for the Wayland suite, hidden inside a running Hyprland.
#
# **Sourced, never run**, like `sway-session.sh`, whose shape this follows:
#
#   source "…/crates/crcbl-shell/tests/hyprland-session.sh"
#   hyprland_session_start
#   # $HYPR_RUNTIME_DIR, $WAYLAND_DISPLAY and $HYPRLAND_INSTANCE_SIGNATURE now
#   # name the nested compositor, and hyprland_log_tail prints its log.
#   hyprland_session_stop
#
# **Why nested in a live Hyprland.** Hyprland 0.56 has no headless-only start,
# and it cannot nest inside headless sway: it binds `xdg_wm_base` at version 6
# and sway 1.12 offers 5. So it runs as a client of the Hyprland the developer
# is already in, started straight onto a hidden special workspace by the host's
# own `hl.exec_cmd`, so nothing flashes on the desktop and no window rule is
# added to the live session. No CI runner has a Hyprland to host it, so this is
# a developer-machine harness.
#
# **Its outputs are sway's.** Two headless outputs, named, sized and placed as
# `wayland-e2e-sway.conf`'s, so the suite's output names and layout constants
# mean the same thing on either compositor; the nested window's own output is
# disabled, because the host never draws a hidden workspace, so that output
# would get no frame callbacks and every FIFO present to it would block.

HYPR_SESSION_TIMEOUT_S="${CRCBL_E2E_SOCKET_TIMEOUT_S:-30}"
HYPR_SESSION_POLL_S=0.2

# The host's signature, kept so teardown and the start poll ask the host and
# not the nested compositor once the variable has been repointed.
HYPR_HOST_SIGNATURE="${HYPRLAND_INSTANCE_SIGNATURE:-}"

hyprland_log_tail() {
    echo "--- nested Hyprland log tail ---" >&2
    tail -n 40 "${HYPR_RUNTIME_DIR}/hyprland.log" >&2 || true
    echo "--- end nested Hyprland log ---" >&2
}

# The instances the host can see, as `signature socket pid` lines.
hyprland_instances() {
    HYPRLAND_INSTANCE_SIGNATURE="$HYPR_HOST_SIGNATURE" hyprctl instances -j \
        | python3 -I -c 'import json, sys
for i in json.load(sys.stdin):
    print(i["instance"], i["wl_socket"], i["pid"])'
}

# Runs a Lua chunk in the nested compositor and fails the harness if it does
# not answer `ok`.
hyprland_eval() {
    local reply
    reply="$(HYPRLAND_INSTANCE_SIGNATURE="$HYPR_SIGNATURE" hyprctl eval "$1")"
    if [ "$reply" != "ok" ]; then
        echo "crcbl e2e: the nested Hyprland refused '$1': $reply" >&2
        hyprland_log_tail
        exit 1
    fi
}

hyprland_session_start() {
    if [ -z "$HYPR_HOST_SIGNATURE" ] || ! command -v hyprctl >/dev/null 2>&1; then
        echo "crcbl e2e: the Hyprland harness runs inside a Hyprland session; none is running" >&2
        exit 1
    fi
    HYPR_RUNTIME_DIR="$(mktemp -d -t crcbl-e2e-hypr.XXXXXX)"
    chmod 700 "$HYPR_RUNTIME_DIR"
    local config="${HYPR_RUNTIME_DIR}/nested.lua"
    # `wayland-e2e-sway.conf`'s load-bearing lines, in Hyprland's terms: the
    # windows under test float, so a window's size is the one it asked for;
    # no gaps or borders, so the size configured is the size of the tile; and
    # the pointer does not move the keyboard. `follow_mouse = 2` is sway's
    # `focus_follows_mouse no`: pointer focus follows the cursor and only a
    # click moves the keyboard. `0` detaches pointer focus too, and then no
    # surface hears the pointer enter at all.
    # `mouse_move_focuses_monitor = false` keeps the suite's windows on the
    # first output, where it opens them, whatever output the cursor is on.
    cat >"$config" <<'LUA'
hl.monitor({ output = "", mode = "1920x1080", position = "auto", scale = 1 })
hl.config({
  misc = { disable_hyprland_logo = true, force_default_wallpaper = 0, disable_splash_rendering = true, mouse_move_focuses_monitor = false },
  animations = { enabled = false },
  general = { gaps_in = 0, gaps_out = 0, border_size = 0 },
  input = { follow_mouse = 2 },
})
hl.window_rule({ match = { class = "^(sh\\.kryptic\\.crcbl\\.e2e)$" }, float = true })
hl.window_rule({ match = { class = "^(sh\\.kryptic\\.crcbl\\.sandbox)$" }, float = true })
LUA

    local before
    before="$(hyprland_instances | cut -d' ' -f1)"
    echo "crcbl e2e: starting a nested Hyprland on a hidden special workspace"
    if [ "$(HYPRLAND_INSTANCE_SIGNATURE="$HYPR_HOST_SIGNATURE" hyprctl eval \
        "hl.exec_cmd('Hyprland -c ${config} > ${HYPR_RUNTIME_DIR}/hyprland.log 2>&1', { workspace = 'special:crcbl-e2e silent', no_initial_focus = true })")" != "ok" ]; then
        echo "crcbl e2e: the host Hyprland refused to start the nested one" >&2
        exit 1
    fi

    # Polled, never slept: a busy machine has taken more than four seconds.
    local deadline=$(( $(date +%s) + HYPR_SESSION_TIMEOUT_S )) line=""
    while [ -z "$line" ]; do
        line="$(hyprland_instances | grep -vF "$before" | head -1 || true)"
        if [ -z "$line" ] && [ "$(date +%s)" -ge "$deadline" ]; then
            echo "crcbl e2e: no nested Hyprland appeared after ${HYPR_SESSION_TIMEOUT_S}s" >&2
            hyprland_log_tail
            exit 1
        fi
        [ -n "$line" ] || sleep "$HYPR_SESSION_POLL_S"
    done
    read -r HYPR_SIGNATURE HYPR_SOCKET HYPR_PID <<<"$line"

    HYPRLAND_INSTANCE_SIGNATURE="$HYPR_SIGNATURE" hyprctl output create headless HEADLESS-1 >/dev/null
    HYPRLAND_INSTANCE_SIGNATURE="$HYPR_SIGNATURE" hyprctl output create headless HEADLESS-2 >/dev/null
    hyprland_eval 'hl.monitor({ output = "HEADLESS-1", mode = "1920x1080@60", position = "0x0", scale = 1 })'
    hyprland_eval 'hl.monitor({ output = "HEADLESS-2", mode = "1280x720@60", position = "1920x0", scale = 1 })'
    hyprland_eval 'hl.monitor({ output = "WAYLAND-1", disabled = true })'
    hyprland_eval 'hl.dispatch(hl.dsp.focus({ monitor = "HEADLESS-1" }))'

    export WAYLAND_DISPLAY="$HYPR_SOCKET"
    export HYPRLAND_INSTANCE_SIGNATURE="$HYPR_SIGNATURE"
    export CRCBL_E2E_COMPOSITOR=hyprland
    echo "crcbl e2e: nested Hyprland up at \$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY (pid $HYPR_PID)"
}

# Kills the nested compositor and removes its directory. Safe to call twice,
# and when `hyprland_session_start` never ran.
hyprland_session_stop() {
    if [ -n "${HYPR_PID:-}" ]; then
        kill "$HYPR_PID" 2>/dev/null || true
        HYPR_PID=""
    fi
    # By name, and `rmdir` last: the directory holds exactly the two files
    # `hyprland_session_start` wrote, and anything else in it stays.
    if [ -n "${HYPR_RUNTIME_DIR:-}" ]; then
        rm -f "${HYPR_RUNTIME_DIR}/nested.lua" "${HYPR_RUNTIME_DIR}/hyprland.log"
        rmdir "$HYPR_RUNTIME_DIR" 2>/dev/null || true
        HYPR_RUNTIME_DIR=""
    fi
}
