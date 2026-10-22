#!/usr/bin/env bash
# Reading what the Vulkan validation layer said, for every harness that runs a
# binary under it and has to fail on the answer — and what the D3D12 debug
# layer said, for the one harness that runs samples on dx12.
#
# **Sourced, never run.** It defines its functions in the caller's shell, which
# is what three harnesses did when they carried the first one inline:
#
#   source "${REPO_ROOT}/tools/vk-validation-log.sh"
#   if ! crcbl_validation_saw_nothing "$log" "$what"; then
#       log_tail
#       exit 1
#   fi
#
# It lives at the top level rather than under a crate for the reason
# `tools/nextest-summary.sh` gives: how `crcbl-vk` spells a validation record in
# a log is `crcbl-vk`'s subject, but the callers span the apps and crates that
# have a harness, and none of them owns the others.
#
# # What it asks, and why both halves
#
# This is the shell's copy of `ValidationReport::assert_clean`, which the vk and
# windowed e2e suites reach from Rust and nothing above the seam can. A
# validation error reaches `crcbl_core::log::error!` in `crcbl-vk`'s `debug`
# module and the process still exits 0, so without this a run advertises that it
# is validating and cannot fail because of it.
#
# The **first** half is not pedantry and the second is worthless without it: a
# log with no validation errors in it is exactly what a run with no messenger
# produces. `crcbl-vk` prints the "validation enabled" line only once the debug
# messenger really exists, so its absence means the layer was missing,
# `VK_EXT_debug_utils` was, or the messenger failed to be created — every one of
# which turns the complaint scan into a green light wired to nothing.
#
# Errors **and** warnings, which is where `assert_clean` draws the line and what
# stage 2's P1 exit criterion said. The messenger only
# ever subscribes to those two severities, so there is no informational chatter
# to filter out. The pattern names the level, the module and the callback's own
# `vk <kind>:` prefix — the teardown leak warning comes from `crcbl_vk::device`
# and is a different question, asked separately by each harness.
#
# # And the same two questions for dx12
#
# `crcbl-dx12` has no messenger: the debug layer stores its messages in an
# `ID3D12InfoQueue`, and `crcbl_dx12::debug::TeardownLog` writes them to the
# log when the device is dropped, at the same levels and in the same shape —
# `(ERROR|WARN) +crcbl_dx12::debug] dx12 validation: …`. Its "validation
# enabled" line is printed once the device's info queue is reachable, which is
# the same claim `crcbl-vk`'s makes about its messenger. So
# `crcbl_dx12_validation_saw_nothing` asks both questions through the one body
# below, with the backend's name swapped.
#
# **And one more, which only dx12 needs.** A vk message is logged the moment
# the layer reports it; a dx12 message is logged only if the device is
# dropped. A process that exits past its device — or leaks the last reference
# to it — logs nothing at all, which reads exactly like a clean run. So a dx12
# log must also carry the line `TeardownLog` always ends on,
# `crcbl-dx12: validation messages read at device teardown`, or it is refused.
#
# There is no dx12 counterpart of `crcbl_validation_layer_checked` yet:
# `crcbl-dx12` has no switch that commits a violation in a running binary, so
# nothing has shown this check go red on a real dx12 log. Its own device tests
# do provoke the layer, through `debug::Validated`.
#
# # What it does not do
#
# It never exits and it never calls the caller's `log_tail`: a harness knows
# what else belongs on the way out — a compositor's log, an X server's — and
# this does not. It prints the diagnosis and returns 1.

# `crcbl_validation_saw_nothing <log> <what>` — vk runs only.
#
# Returns 0 when the layer was loaded and said nothing, 1 otherwise, having
# written to stderr what it found.
crcbl_validation_saw_nothing() {
    crcbl_backend_validation_saw_nothing vk "$1" "$2"
}

# `crcbl_dx12_validation_saw_nothing <log> <what>` — dx12 runs only.
#
# The same answer about the D3D12 debug layer, plus the teardown line the
# header argues for.
crcbl_dx12_validation_saw_nothing() {
    crcbl_backend_validation_saw_nothing dx12 "$1" "$2"
}

# `crcbl_backend_validation_saw_nothing <vk|dx12> <log> <what>` — the body
# both of the above share. What differs by backend is the advice for a missing
# layer and the one liveness question each backend has of its own; the two
# lines asked about and the complaint pattern are one shape with the name
# swapped.
crcbl_backend_validation_saw_nothing() {
    local backend="$1" log="$2" what="$3" complaints
    if ! grep -qF "crcbl-${backend}: validation enabled (" "$log"; then
        case "$backend" in
            vk)
                echo "crcbl e2e: ${what} ran with CRCBL_VK_VALIDATION=1 and never loaded the" >&2
                echo "           layer, so a clean log here proves nothing. Install" >&2
                echo "           VK_LAYER_KHRONOS_validation (Arch: vulkan-validation-layers," >&2
                echo "           Debian/Ubuntu: vulkan-validationlayers) — crcbl-vk warns by name" >&2
                echo "           when it is missing, and the warning is in the log above." >&2
                ;;
            dx12)
                echo "crcbl e2e: ${what} ran with CRCBL_DX12_VALIDATION=1 and never enabled the" >&2
                echo "           D3D12 debug layer, so a clean log here proves nothing. Install" >&2
                echo "           Windows' Graphics Tools optional feature — crcbl-dx12 warns by" >&2
                echo "           name when the layer is missing, and the warning is in the log" >&2
                echo "           above." >&2
                ;;
            *)
                echo "crcbl e2e: no validation log shape is known for backend '${backend}'" >&2
                return 1
                ;;
        esac
        cat "$log" >&2
        return 1
    fi
    if [ "$backend" = vk ] &&
        grep -qF 'a panic escaped the Vulkan debug messenger callback' "$log"; then
        echo "crcbl e2e: ${what} lost validation messages — a panic escaped the" >&2
        echo "           messenger callback, so the check below cannot see what the" >&2
        echo "           layer said." >&2
        cat "$log" >&2
        return 1
    fi
    if [ "$backend" = dx12 ] &&
        ! grep -qF 'crcbl-dx12: validation messages read at device teardown' "$log"; then
        echo "crcbl e2e: ${what} enabled the D3D12 debug layer and never read its" >&2
        echo "           messages back — crcbl-dx12 writes them when the device is" >&2
        echo "           dropped, and this run's device never was, so a log with no" >&2
        echo "           complaint in it here proves nothing. Drop the device before" >&2
        echo "           the process exits." >&2
        cat "$log" >&2
        return 1
    fi
    complaints="$(grep -E "(ERROR|WARN) +crcbl_${backend}::debug] ${backend} " "$log" || true)"
    [ -z "$complaints" ] && return 0
    echo "crcbl e2e: the validation layer complained about ${what}:" >&2
    while IFS= read -r line; do
        echo "               $line" >&2
    done <<<"$complaints"
    echo "           Those are specification violations this run committed. Fix" >&2
    echo "           them where they were recorded rather than leaving the line" >&2
    echo "           in a log." >&2
    return 1
}

# `crcbl_validation_layer_checked <log> <what>` — runs with
# `CRCBL_VK_VALIDATION_PROVOKE=1` only.
#
# **The question `crcbl_validation_saw_nothing` cannot ask.** That one is
# satisfied by a layer which loads, announces itself and checks nothing: a
# message submitted through `vkSubmitDebugUtilsMessageEXT` is delivered whatever
# the layer's checks are set to, so `CRCBL_VK_VALIDATION_SELF_TEST` proves the
# report path and not the checking. Measured on layer 1.4.357: with
# `VK_KHRONOS_VALIDATION_VALIDATE_CORE=false` the self-test message still
# arrives and a real specification violation produces nothing at all.
#
# So a run that sets `CRCBL_VK_VALIDATION_PROVOKE=1` has `crcbl-vk` record one
# out-of-bounds `vkCmdCopyBuffer` at its first present — never submitted — and
# this reads the log for what only a **core check** emits.
#
# It greps for the layer's own complaint and never for `crcbl-vk`'s
# `CRCBL_VK_VALIDATION_PROVOKE records …` line, which deliberately names neither
# the entry point nor the VUIDs: a grep a harness's own log line can answer is
# the green light wired to nothing this exists to remove. The digits of the
# `VUID-vkCmdCopyBuffer-size-*` pair belong to the layer build and are not named
# here for the same reason they are not named in `crcbl-vk` — 1.4.357 reports
# `-00115` and `-00116` where an older comment expected `-00225`.
#
# Prints the diagnosis and returns 1; the caller decides what else belongs on
# the way out, and usually has the failing run's whole output to show.
crcbl_validation_layer_checked() {
    local log="$1" what="$2"
    grep -qE 'crcbl_vk::debug\] vk validation: VUID-vkCmdCopyBuffer-size-' "$log" && return 0
    echo "crcbl e2e: ${what} ran with CRCBL_VK_VALIDATION_PROVOKE=1 and the layer" >&2
    echo "           reported nothing about the deliberate out-of-bounds copy" >&2
    echo "           crcbl-vk recorded at its first present. Only a core check" >&2
    echo "           emits that, so this layer is loaded and checking nothing —" >&2
    echo "           which is the state every other validation pass here reads as" >&2
    echo "           success. Check VK_KHRONOS_VALIDATION_VALIDATE_CORE and the" >&2
    echo "           layer's settings file; a debug build is also required, and" >&2
    echo "           a release one says so in the log." >&2
    return 1
}
