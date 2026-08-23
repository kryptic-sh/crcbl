#!/usr/bin/env bash
# Reading what the Vulkan validation layer said, for every harness that runs a
# binary under it and has to fail on the answer.
#
# **Sourced, never run.** It defines one function in the caller's shell, which
# is what three harnesses did when they carried it inline:
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
# `docs/plan/02-vulkan-backend.md`'s P1 exit criterion says. The messenger only
# ever subscribes to those two severities, so there is no informational chatter
# to filter out. The pattern names the level, the module and the callback's own
# `vk <kind>:` prefix — the teardown leak warning comes from `crcbl_vk::device`
# and is a different question, asked separately by each harness.
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
    local log="$1" what="$2" complaints
    if ! grep -qF 'crcbl-vk: validation enabled (' "$log"; then
        echo "crcbl e2e: ${what} ran with CRCBL_VK_VALIDATION=1 and never loaded the" >&2
        echo "           layer, so a clean log here proves nothing. Install" >&2
        echo "           VK_LAYER_KHRONOS_validation (Arch: vulkan-validation-layers," >&2
        echo "           Debian/Ubuntu: vulkan-validationlayers) — crcbl-vk warns by name" >&2
        echo "           when it is missing, and the warning is in the log above." >&2
        cat "$log" >&2
        return 1
    fi
    if grep -qF 'a panic escaped the Vulkan debug messenger callback' "$log"; then
        echo "crcbl e2e: ${what} lost validation messages — a panic escaped the" >&2
        echo "           messenger callback, so the check below cannot see what the" >&2
        echo "           layer said." >&2
        cat "$log" >&2
        return 1
    fi
    complaints="$(grep -E '(ERROR|WARN) +crcbl_vk::debug] vk ' "$log" || true)"
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
