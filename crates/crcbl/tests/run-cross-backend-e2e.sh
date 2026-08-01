#!/usr/bin/env bash
# Render one frame through *both* GPU backends and compare the two images.
#
#   crates/crcbl/tests/run-cross-backend-e2e.sh [extra compare-png args…]
#
# `docs/plan/12-testing.md` schedules "cross-backend image compare (vk↔wgpu)"
# for P5 and calls it "the tier system's regression net"; the roadmap makes P5
# exit the point the HAL freezes, "because that is when a *second* backend
# implements the seam". This script is that gate.
#
# WHY IT LIVES HERE
#   `crates/crcbl` owns `src/screenshot.rs` — the offscreen render → readback
#   path both backends serve — and it is the only crate that depends on Vulkan,
#   on wgpu and (as a dev-dependency) on the comparator. Anywhere else would
#   have to reach across the seam it is testing: `crcbl-wgpu` cannot name
#   `crcbl-vk`, `crcbl-vk` cannot name `crcbl-wgpu`, and `crcbl-golden` must not
#   depend on a GPU at all.
#
# WHY A SCRIPT AND TWO PROCESSES, NOT ONE #[test]
#   Which Vulkan ICD a process ends up on is decided by `VK_DRIVER_FILES` /
#   `VK_ICD_FILENAMES` when the instance is created, so "Vulkan on radv versus
#   wgpu on lavapipe" — the case a developer's machine and a CI runner disagree
#   about — is not expressible inside one test binary. Two processes,
#   two pins, one comparison.
#
# WHAT IT COMPARES, AND AGAINST WHAT BOUND
#   The comparison is `crcbl-golden`'s `Tolerance::RASTERISER` (max per-channel
#   delta 2, at most 2% of pixels over that, block SSIM ≥ 0.99), **not** byte
#   equality. Byte equality holds today only because both backends went through
#   the same ICD; measured on this machine with the ICDs crossed — Vulkan on
#   radv against wgpu on lavapipe, 256x192 —
#
#       84.23% of pixels differ, max channel delta 1, 0 pixels over tolerance,
#       mean abs error 0.2127, rmse 0.4612, ssim 0.999898
#
#   which is the same shape as the radv-vs-lavapipe numbers `crcbl-golden`'s
#   docs already record for the HDR path: everything differs, everything differs
#   by exactly one level. See that crate's docs for the full table.
#
# WHY A LOOSE TOLERANCE IS STILL A GATE
#   Two blank frames match perfectly, so `compare-png` refuses to pass a frame
#   with fewer than `CRCBL_CROSS_MIN_COLORS` distinct colours. The lit cube has
#   41 at 256x192 and 36 at 97x61; a cleared frame has one. A backend that
#   rendered nothing therefore fails this gate even though its output "matches".
#
# ENVIRONMENT
#   CRCBL_VK_ICD           ICD manifest pinned for the `CRCBL_GPU=vk` run.
#   CRCBL_WGPU_ICD         ICD manifest pinned for the `CRCBL_GPU=wgpu` run.
#                          Setting the two to different drivers is the
#                          interesting configuration; CI pins both to lavapipe,
#                          which is the only driver a runner has.
#   CRCBL_CROSS_SIZES      Frame sizes, space separated. Default "256x192 97x61".
#                          The second is deliberately not a multiple of 64: a
#                          256-byte row-pitch rule wgpu enforces and Vulkan does
#                          not made every other width fail, and only a size like
#                          this catches it.
#   CRCBL_CROSS_MIN_COLORS Anti-vacuity floor. Default 16.
#   CRCBL_CROSS_OUT        Where the PNGs go. Default: a temporary directory.
#
# Exits non-zero if either backend fails to render, if a rendered frame is
# missing or empty, if any comparison exceeds the tolerance, if a frame is too
# nearly blank to be evidence, or if **zero comparisons ran** — the trap
# `docs/plan/12-testing.md` names by name and this repo has already paid for
# once ("a test-count guard silently matched nothing because CI colours its
# output"). This guard counts in the shell rather than by grepping the tool's
# output, so no amount of colouring can make it match nothing.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"
SIZES="${CRCBL_CROSS_SIZES:-256x192 97x61}"
MIN_COLORS="${CRCBL_CROSS_MIN_COLORS:-16}"

cd "$REPO_ROOT"

OUT_DIR="${CRCBL_CROSS_OUT:-}"
OWN_OUT_DIR=0
if [ -z "$OUT_DIR" ]; then
    OUT_DIR="$(mktemp -d -t crcbl-cross-backend.XXXXXX)"
    OWN_OUT_DIR=1
fi
mkdir -p "$OUT_DIR"
cleanup() {
    local status=$?
    if [ "$OWN_OUT_DIR" -eq 1 ] && [ "$status" -eq 0 ]; then
        rm -rf "$OUT_DIR"
    elif [ "$OWN_OUT_DIR" -eq 1 ]; then
        echo "crcbl cross-backend: rendered frames kept in $OUT_DIR" >&2
    fi
    exit "$status"
}
trap cleanup EXIT INT TERM

# Resolve an ICD manifest, tolerating the packaging difference between
# distributions: Debian ships `lvp_icd.x86_64.json`, Arch ships `lvp_icd.json`.
# A miss is a hard failure — a pinned ICD that silently fell back to whatever
# the loader found would defeat the point of pinning it. Same code as
# `run-vk-e2e.sh` and `run-wgpu-e2e.sh`, and deliberately as strict.
resolve_icd() {
    local wanted="$1"
    local label="$2"
    if [ -f "$wanted" ]; then
        echo "$wanted"
        return 0
    fi
    local dir stem candidate
    dir="$(dirname "$wanted")"
    stem="$(basename "$wanted")"
    stem="${stem%%.*}"
    for candidate in "${dir}/${stem}".json "${dir}/${stem}".*.json; do
        if [ -f "$candidate" ]; then
            echo "crcbl cross-backend: $label $wanted is absent; using $candidate" >&2
            echo "$candidate"
            return 0
        fi
    done
    echo "crcbl cross-backend: $label=$wanted does not exist, and no sibling matched" >&2
    ls -la "$dir" >&2 || true
    return 1
}

VK_PIN=""
WGPU_PIN=""
if [ -n "${CRCBL_VK_ICD:-}" ]; then
    VK_PIN="$(resolve_icd "$CRCBL_VK_ICD" CRCBL_VK_ICD)"
    echo "crcbl cross-backend: vk pinned to $VK_PIN"
fi
if [ -n "${CRCBL_WGPU_ICD:-}" ]; then
    WGPU_PIN="$(resolve_icd "$CRCBL_WGPU_ICD" CRCBL_WGPU_ICD)"
    echo "crcbl cross-backend: wgpu pinned to $WGPU_PIN"
fi
if [ -n "$VK_PIN" ] && [ -n "$WGPU_PIN" ] && [ "$VK_PIN" = "$WGPU_PIN" ]; then
    echo "crcbl cross-backend: both backends are pinned to the same ICD, so this run"
    echo "                     compares two backends and not two drivers. That is what"
    echo "                     a CI runner has; a developer with two ICDs should cross"
    echo "                     them."
fi

# NOTE: neither `crcbl screenshot` nor its `--json` output names the adapter it
# opened — the CLI installs no logger, so `crcbl-vk`'s and `crcbl-wgpu`'s
# adapter lines go nowhere. `run-vk-e2e.sh` and `run-wgpu-e2e.sh` both read that
# line out of their suites and would catch a pinned ICD the loader ignored; this
# harness cannot, and pins by manifest path alone. Recorded in
# `crates/crcbl/src/screenshot.rs`'s docs rather than worked around silently.

# Rendered before the loop so a compile error is one message rather than one per
# size, and so the timings below are the render's rather than rustc's.
echo "crcbl cross-backend: building the CLI and the comparator"
cargo build --locked --quiet --package crcbl-cli --bin crcbl
cargo build --locked --quiet --package crcbl-golden --example compare-png

render() {
    local backend="$1" size="$2" output="$3" pin="$4"
    rm -f "$output"
    local log
    log="$(mktemp -t "crcbl-cross-${backend}.XXXXXX.log")"
    set +e
    (
        if [ -n "$pin" ]; then
            export VK_DRIVER_FILES="$pin"
            export VK_ICD_FILENAMES="$pin"
        fi
        export CRCBL_GPU="$backend"
        cargo run --locked --quiet --package crcbl-cli --bin crcbl -- \
            screenshot --size "$size" --output "$output"
    ) >"$log" 2>&1
    local status=$?
    set -e
    if [ "$status" -ne 0 ]; then
        echo "crcbl cross-backend: $backend failed to render $size (exit $status)" >&2
        cat "$log" >&2
        rm -f "$log"
        exit "$status"
    fi
    rm -f "$log"
    # The file is checked rather than the exit code alone: it was deleted above,
    # so a run that wrote nothing cannot be compared against a stale frame from
    # a previous invocation. That is the same class of bug as a gate that runs
    # no tests.
    if [ ! -s "$output" ]; then
        echo "crcbl cross-backend: $backend reported success but wrote no frame at $output" >&2
        exit 1
    fi
}

EXPECTED=0
for _ in $SIZES; do
    EXPECTED=$((EXPECTED + 1))
done
if [ "$EXPECTED" -eq 0 ]; then
    echo "crcbl cross-backend: CRCBL_CROSS_SIZES is empty — there is nothing to compare" >&2
    exit 1
fi

COMPARISONS=0
for SIZE in $SIZES; do
    VK_PNG="${OUT_DIR}/vk-${SIZE}.png"
    WGPU_PNG="${OUT_DIR}/wgpu-${SIZE}.png"

    echo "crcbl cross-backend: rendering ${SIZE} through vk"
    render vk "$SIZE" "$VK_PNG" "$VK_PIN"
    echo "crcbl cross-backend: rendering ${SIZE} through wgpu"
    render wgpu "$SIZE" "$WGPU_PNG" "$WGPU_PIN"

    # Informative, never asserted: byte equality is what happens when both
    # backends reach the same driver, and demanding it would make this gate fail
    # on any machine where they do not.
    if cmp -s "$VK_PNG" "$WGPU_PNG"; then
        echo "crcbl cross-backend: ${SIZE} is byte-identical between the backends ($(sha256sum "$VK_PNG" | cut -c1-16)…)"
    else
        echo "crcbl cross-backend: ${SIZE} differs byte-wise; the tolerance is what decides"
    fi

    cargo run --locked --quiet --package crcbl-golden --example compare-png -- \
        "$VK_PNG" "$WGPU_PNG" \
        --label "cross-backend-${SIZE}" \
        --min-colors "$MIN_COLORS" \
        "$@"
    COMPARISONS=$((COMPARISONS + 1))
done

# The guard `docs/plan/12-testing.md` names: a job that compares nothing and
# reports success is worse than no job. Both halves are checked — none ran at
# all, and fewer ran than were asked for.
if [ "$COMPARISONS" -eq 0 ]; then
    echo "crcbl cross-backend: no comparisons ran — the gate is not gating" >&2
    exit 1
fi
if [ "$COMPARISONS" -ne "$EXPECTED" ]; then
    echo "crcbl cross-backend: $COMPARISONS of $EXPECTED comparisons ran" >&2
    exit 1
fi

echo "crcbl cross-backend: $COMPARISONS/$EXPECTED size(s) agree across vk and wgpu"
