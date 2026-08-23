#!/usr/bin/env bash
# Run `crcbl-vk`'s end-to-end suite against a real Vulkan implementation.
#
#   crates/crcbl-vk/tests/run-vk-e2e.sh [--bless] [extra nextest args…]
#
# The tests are feature-gated *and* `#[ignore]`d, so a plain
# `cargo nextest run --workspace --all-features` on a machine with no Vulkan
# loader stays green. This script is the only thing that turns them on, and CI
# runs this script — `docs/plan/12-testing.md` calls a silently-skipped e2e job a
# known trap, so the script fails when the suite reports zero tests run.
#
# Everything here is headless: the suite renders into an offscreen image ring
# (`SurfaceTarget::Offscreen`), so no compositor and no display are needed. The
# *windowed* Vulkan paths are covered by the sandbox runs inside
# `crates/crcbl-shell/tests/run-wayland-e2e.sh` and `run-x11-e2e.sh`.
#
# Exits non-zero if there is no Vulkan loader, if no ICD is visible, if no tests
# ran, or if any test fails.
#
# WHAT A GREEN RUN HERE IS AND IS NOT EVIDENCE OF
#   This script is what a developer runs to see what CI sees, so it has to say
#   what it actually checked rather than only how many tests passed. Two things
#   were silently inherited before and are now reported by name:
#
#   * **which driver ran**, taken from the suite's own adapter line rather than
#     from the manifest path that was asked for; and
#   * **how far this machine's validation layer can see**, because sync
#     validation is not one switch. A hazard inside one command buffer is caught
#     while it is recorded; a hazard that spans two *submissions* can only be
#     caught when the queue is submitted, and layer builds differ in whether
#     they model that. Every cross-frame hazard is of the second kind.
#
#   That difference is not hypothetical. A missing cross-frame barrier on the
#   render graph's depth transient was reported by the CI leg's layer and by
#   nothing at all on an Arch box running VK_LAYER_KHRONOS_validation 1.4.350 —
#   same driver, same flags, same tests, 26/26 green. A harness that reports
#   26/26 without saying that is worse than no harness, so it now prints the
#   layer's reach and shouts when the reach is short.
#
#   The gate for that *class* of bug is therefore deliberately not here: it is
#   `crcbl-render`'s graph-compile suite, which compiles two frames against one
#   `TransientPool` and asserts the second one's barriers name what the first
#   left behind. That covers transients, whose state the pool knows, and — since
#   `ImportedImage` gained `InitialClaim` — imports too: the pool records what
#   each executed graph left every tracked import in, and a second frame whose
#   `initial` contradicts it is refused at compile time rather than barriered
#   against. No layer, no ICD, no GPU, no packaging opinions.
#
# ENVIRONMENT
#   CRCBL_VK_ICD              Pin an ICD manifest, e.g. lavapipe's `lvp_icd.json`.
#                             CI sets this so a runner that grows a GPU does not
#                             silently stop testing the software path.
#   CRCBL_VK_EXPECT_ADAPTER   A substring the adapter the suite actually opened
#                             must contain, e.g. `llvmpipe`. Unset means "do not
#                             check", which is what a developer running against
#                             their own hardware wants. It exists because
#                             `CRCBL_VK_ICD` pins what the loader is *offered*
#                             and never what it chose: a manifest the loader
#                             declined and a manifest it never read both leave a
#                             green run behind. `run-dx12-e2e.sh` makes the same
#                             distinction, and hardcodes its answer because its
#                             pin accepts exactly one value.
#   CRCBL_VK_SYNC_VALIDATION  `1` adds synchronisation validation. CI sets it;
#                             `docs/plan/02-vulkan-backend.md` names sync bugs
#                             as this stage's headline risk and this as the
#                             mitigation.
#   CRCBL_BLESS               `1` regenerates golden images instead of comparing
#                             against them. `--bless` sets it; see below.
#
# GOLDEN IMAGES
#   `--bless` regenerates `tests/golden/*.png` rather than comparing against
#   them, which is the spelling `docs/plan/12-testing.md` asks for. A blessed run
#   deliberately **fails**: it has not checked anything, and a gate that any
#   missing reference switches off is not a gate. Review the regenerated image,
#   commit it, and re-run without the flag.
#
#   Bless on the driver the reference is meant to represent. The tolerance in
#   `crcbl-golden` is calibrated for the radv/lavapipe difference (see that
#   crate's docs for the measurements), so either works — but re-blessing on a
#   third driver and committing it silently moves the reference.

set -euo pipefail

if [ "${1:-}" = "--bless" ]; then
    export CRCBL_BLESS=1
    shift
    echo "crcbl vk e2e: CRCBL_BLESS=1 — golden images will be regenerated, and the"
    echo "              golden tests will fail on purpose because a blessed run checks nothing."
fi

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# Validation is the point of this suite: `ValidationReport::assert_clean` fails
# when the layer was never loaded, so a run without it fails loudly rather than
# passing vacuously. Setting it explicitly means a release-profile run works too.
export CRCBL_VK_VALIDATION="${CRCBL_VK_VALIDATION:-1}"
export CRCBL_VK_SYNC_VALIDATION="${CRCBL_VK_SYNC_VALIDATION:-1}"

# shellcheck source=crates/crcbl-vk/tests/vulkan-icd.sh
source "${CRATE_DIR}/tests/vulkan-icd.sh"
crcbl_pin_vk_icd "crcbl vk e2e"

# Reading nextest's summary is `tools/nextest-summary.sh`'s job, for the same
# reason this file does not carry its own ICD resolution: eight harnesses had a
# copy, and five of them read a cancelled run's `2/15 tests run` as a healthy
# fifteen. `run-vk-e2e.ps1` still carries its own — it cannot source a bash file
# — which `docs/backlog.md` records.
# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

if [ -z "${CRCBL_VK_ICD:-}" ]; then
    # Say so. The header above claims this script is what a developer runs to
    # see what CI sees, and with no ICD pinned that claim is false: the loader
    # picks whatever is installed, which on a workstation is the discrete GPU
    # and never the software rasteriser CI runs. The suite prints the adapter it
    # got, but nobody reads an adapter line looking for an absence — this is the
    # line that names it. Not a hard failure: running against real hardware is a
    # thing worth doing deliberately, and this is exactly how.
    cat >&2 <<'NOICD'
crcbl vk e2e: CRCBL_VK_ICD is not set, so the loader will choose the driver.
              CI pins lavapipe and this run does not, so a green run here is
              NOT the run CI makes. To reproduce CI:
                CRCBL_VK_ICD=/usr/share/vulkan/icd.d/lvp_icd.json $0
NOICD
fi

# Fail early and legibly rather than letting every test panic with the same
# message. Best-effort on both platforms: the suite's own `NoLoader` panic is the
# real gate, and this is the line that names the fix.
#
# The two arms are not variants of one another. `ash::Entry::load` asks the
# platform's own loader — `dlopen("libvulkan.so.1")` on one, `LoadLibrary(
# "vulkan-1.dll")` on the other — and the two search different places, so a probe
# written for one says nothing at all about the other. There is no Vulkan loader
# in a stock `windows-latest` image (`actions/runner-images`' Windows README
# lists none), which is why the CI job there installs one and why an absence has
# to be a *loud* failure here rather than a suite that quietly fails every test.
case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        # Windows resolves a bare DLL name against the process directory, the
        # system directory and then `PATH`. Git Bash hands `PATH` over in POSIX
        # form with the system directory already on it, so walking it is the
        # whole search — and naming the file that was found is what tells a
        # loader installed by this job from one that was already there.
        IFS=: read -r -a CRCBL_PATH_DIRS <<<"$PATH"
        LOADER=""
        for dir in "${CRCBL_PATH_DIRS[@]}"; do
            if [ -f "${dir}/vulkan-1.dll" ]; then
                LOADER="${dir}/vulkan-1.dll"
                break
            fi
        done
        if [ -z "$LOADER" ]; then
            echo "crcbl vk e2e: no vulkan-1.dll anywhere on PATH; install a Vulkan loader" >&2
            echo "  The LunarG SDK carries one, and so does the smaller Vulkan Runtime:" >&2
            echo "    https://sdk.lunarg.com/sdk/download/<version>/windows/vulkan-runtime-components.zip" >&2
            echo "  A stock GitHub windows runner has neither." >&2
            exit 1
        fi
        echo "crcbl vk e2e: loader $LOADER"
        ;;
    *)
        # `ldconfig -p` is not universal, hence the two well-known paths beside
        # it.
        if ! ldconfig -p 2>/dev/null | grep -q 'libvulkan\.so\.1' \
            && [ ! -e /usr/lib/libvulkan.so.1 ] \
            && [ ! -e /usr/lib/x86_64-linux-gnu/libvulkan.so.1 ]; then
            echo "crcbl vk e2e: no libvulkan.so.1 found; install a Vulkan loader" >&2
            echo "  Debian/Ubuntu: libvulkan1 mesa-vulkan-drivers vulkan-validationlayers" >&2
            echo "  Arch:          vulkan-icd-loader vulkan-swrast vulkan-validation-layers" >&2
            exit 1
        fi
        ;;
esac

if command -v vulkaninfo >/dev/null 2>&1; then
    echo "crcbl vk e2e: --- vulkaninfo --summary ---"
    vulkaninfo --summary 2>&1 | sed -n '1,80p' || true
    echo "crcbl vk e2e: --- end vulkaninfo ---"
    # Named, rather than left in eighty lines of dump: a run gated by a
    # validation layer is only evidence about the layer that gated it, and
    # `vulkaninfo --summary`'s layer table is the one place that says which.
    LAYER_LINE="$(vulkaninfo --summary 2>/dev/null | grep -E '^VK_LAYER_KHRONOS_validation' || true)"
    if [ -n "$LAYER_LINE" ]; then
        # `NAME  description…  <spec version>  version <impl>`, so the three
        # trailing fields are the two numbers that identify a build.
        echo "crcbl vk e2e: validation layer $(echo "$LAYER_LINE" | awk '{print $1, "spec", $(NF-2), $(NF-1), $NF}')"
    else
        echo "crcbl vk e2e: vulkaninfo does not list VK_LAYER_KHRONOS_validation" >&2
    fi
fi

cd "$REPO_ROOT"
OUTPUT="$(mktemp -t crcbl-vk-e2e.XXXXXX.log)"
cleanup() {
    local status=$?
    rm -f "$OUTPUT" "${OUTPUT}.plain"
    exit "$status"
}
trap cleanup EXIT INT TERM

# `--success-output immediate` rather than `--no-capture`, which is what this
# passed for most of its life. `--no-capture` hands the test binary the real
# stdio, and nextest cannot then interleave two tests' output — so it silently
# forces one thread and prints `warning: ignoring --test-threads because
# --no-capture is specified` on every run. The `--test-threads 1` that used to
# sit beside it was therefore dead, and the suite ran serially on both legs
# whatever either flag said. Capturing instead lets nextest use the runner's
# cores, and `immediate` keeps every line the greps below need — the adapter
# line, the sync-validation reach — in the log, ahead of the summary, which is
# the ordering both this file and `run-vk-e2e.ps1` read. `run-dx12-e2e.sh`,
# `run-mtl-e2e.sh` and `run-render-e2e.sh` were already spelled this way.
set +e
cargo nextest run \
    --locked \
    --package crcbl-vk \
    --features vk-e2e \
    --test vk_e2e \
    --run-ignored all \
    --success-output immediate \
    "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

# The colour-stripped copy is load-bearing for every match below — CI sets
# `CARGO_TERM_COLOR: always`, so nextest wraps its counts in escapes and a
# plain-text match sees no digits next to "tests run".
crcbl_nextest_plain "$OUTPUT" "${OUTPUT}.plain"

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl vk e2e: the suite failed" >&2
    # `docs/plan/12-testing.md`: "diffs uploaded as CI artifacts on failure".
    # Naming the directory here is what makes the CI step's `if: failure()`
    # upload obvious rather than folklore.
    if [ -d "${REPO_ROOT}/target/golden-diff" ]; then
        echo "crcbl vk e2e: golden-image diffs are in target/golden-diff:" >&2
        ls -la "${REPO_ROOT}/target/golden-diff" >&2 || true
    fi
    exit "$STATUS"
fi

# The trap `docs/plan/12-testing.md` names by name: a job that skips everything
# and reports success is worse than no job — and so is one nextest cancelled
# after two tests, whose summary still ends in the total it never reached.
if ! crcbl_nextest_summary "${OUTPUT}.plain" "crcbl vk e2e" \
    "The vk-e2e feature or the ignore attribute stopped matching the tests."; then
    exit 1
fi
RAN="$CRCBL_NEXTEST_TESTS_RUN"

# Which driver actually ran, from the suite rather than from the manifest that
# was asked for. A pinned ICD the loader quietly ignored, or a sibling manifest
# that turned out to be a different driver, both show up here and nowhere else.
DRIVER="$(grep -Eo 'vk e2e: adapter .*' "${OUTPUT}.plain" | head -1 || true)"
if [ -n "$DRIVER" ]; then
    echo "crcbl vk e2e: ${DRIVER#vk e2e: }"
else
    echo "crcbl vk e2e: the suite never named an adapter — it did not open a device" >&2
    exit 1
fi

# And whether that is the implementation the caller came here for. `CRCBL_VK_ICD`
# offers the loader a manifest; only this line says the loader took it. The two
# come apart quietly — a manifest for an incompatible driver is refused with the
# same `ERROR_INCOMPATIBLE_DRIVER` a missing one gets, and a `VK_DRIVER_FILES`
# that never reached the test process leaves the loader free to pick anything
# installed. Either way the tests below still run, still pass, and are evidence
# about a driver nobody chose.
if [ -n "${CRCBL_VK_EXPECT_ADAPTER:-}" ]; then
    case "$DRIVER" in
        *"$CRCBL_VK_EXPECT_ADAPTER"*)
            echo "crcbl vk e2e: the adapter contains '${CRCBL_VK_EXPECT_ADAPTER}', as expected"
            ;;
        *)
            echo "crcbl vk e2e: ############################################################" >&2
            echo "crcbl vk e2e: # THE PIN MISSED. THIS RUN IS NOT THE RUN IT SAYS IT IS.   #" >&2
            echo "crcbl vk e2e: ############################################################" >&2
            echo "crcbl vk e2e: expected an adapter containing '${CRCBL_VK_EXPECT_ADAPTER}'," >&2
            echo "              and the suite opened its device on:" >&2
            echo "                ${DRIVER#vk e2e: }" >&2
            echo "              CRCBL_VK_ICD=${CRCBL_VK_ICD:-<unset>} was what the loader was" >&2
            echo "              offered, so either it declined that manifest or the pin never" >&2
            echo "              reached the test process." >&2
            exit 1
            ;;
    esac
fi

echo "crcbl vk e2e: $RAN tests ran against a real Vulkan implementation"

# **The teardown leak report, read rather than left in the log.** `crcbl-vk`
# warns when a device is destroyed with objects still alive, naming their kinds
# and formats — and a warning fails nothing, so the four real leaks it found the
# afternoon it learned to name them were found by a person reading a job log.
# Nothing re-read one since, which means a leak introduced tomorrow warns and
# passes.
#
# The expectation is zero lines, not a judgement call: every test in this suite
# destroys what it creates, so a line here names a test that stopped doing so. A
# test that must leave an object alive — one asserting a refusal has nothing to
# destroy — has to be given its own expectation deliberately rather than hiding
# inside a warning nobody reads.
LEAKS="$(grep -F 'object(s) still alive at device teardown' "${OUTPUT}.plain" || true)"
if [ -n "$LEAKS" ]; then
    echo "crcbl vk e2e: a device was destroyed with objects still alive:" >&2
    while IFS= read -r line; do
        echo "                $line" >&2
    done <<<"$LEAKS"
    echo "              The suite's own teardown reporter wrote that. Destroy the" >&2
    echo "              objects in the test that made them — the kinds and formats" >&2
    echo "              above are what it saw — rather than leaving the line in the" >&2
    echo "              log for somebody to notice." >&2
    exit 1
fi
echo "crcbl vk e2e: every device was destroyed with nothing left alive"

# **A test that returned early is not a test that passed.** Seven tests in this
# suite open a device asking for `TASK_SHADER` and return early when it is
# absent, printing why — the amplification stage is what they exercise and there
# is nothing to exercise without it. Each of those early returns is counted by
# nextest as a pass, so an adapter that stopped reporting the feature would take
# the whole mesh path out of this run and leave the summary unchanged.
#
# radv and lavapipe both report it, which are the two implementations this suite
# runs against, so a line here is a finding rather than a fact of life. It is a
# banner on a developer's machine — somebody on other hardware is not wrong to
# run this — and a failure under `CI`, exactly as the loader probe below is, for
# `docs/plan/12-testing.md`'s reason: a silently-skipped e2e job is worse than no
# job.
SKIPS="$(grep -c 'no TASK_SHADER on this device' "${OUTPUT}.plain" || true)"
if [ "$SKIPS" -gt 0 ]; then
    echo "crcbl vk e2e: ############################################################" >&2
    echo "crcbl vk e2e: # $SKIPS MESH-PATH TEST(S) RETURNED EARLY AND COUNTED AS PASSES.  #" >&2
    echo "crcbl vk e2e: ############################################################" >&2
    echo "crcbl vk e2e: This device reports no TASK_SHADER, so every test whose" >&2
    echo "              subject is the amplification stage exercised nothing. The" >&2
    echo "              count above is how many, and the suite's own lines say" >&2
    echo "              which." >&2
    if [ -n "${CI:-}" ]; then
        echo "crcbl vk e2e: ...and this is CI, where both implementations this job" >&2
        echo "              runs against report the feature." >&2
        exit 1
    fi
else
    echo "crcbl vk e2e: every mesh-path test had an amplification stage to run on"
fi

# How far this machine's validation layer can see. The suite measures it; this
# is what turns the measurement into something a reader cannot miss.
REACH="$(grep -Eo 'sync-validation reach: .*' "${OUTPUT}.plain" | tail -1 || true)"
if [ -z "$REACH" ]; then
    # Not a soft warning: the suite is supposed to publish this, and a harness
    # that silently stopped measuring its own blind spot is the failure this
    # whole section exists to prevent.
    echo "crcbl vk e2e: the suite did not report its sync-validation reach." >&2
    echo "              crates/crcbl-vk/tests/vk_e2e/main.rs must print it, and this" >&2
    echo "              script must be able to find it, or a green run here" >&2
    echo "              claims evidence it does not have." >&2
    exit 1
fi
echo "crcbl vk e2e: $REACH"
case "$REACH" in
    *cross-submission=no*)
        echo "crcbl vk e2e: ############################################################" >&2
        echo "crcbl vk e2e: # THIS RUN IS WEAKER THAN THE CI JOB IT STANDS IN FOR.     #" >&2
        echo "crcbl vk e2e: ############################################################" >&2
        echo "crcbl vk e2e: This machine's validation layer reports hazards inside a" >&2
        echo "              submission and not hazards *between* submissions. Every" >&2
        echo "              missing cross-frame barrier is of the second kind, so the" >&2
        echo "              green result above says nothing about them — CI's layer" >&2
        echo "              has caught one that this configuration cannot see." >&2
        echo "              Rely on 'cargo nextest run -p crcbl-render' for that" >&2
        echo "              class: it compiles consecutive frames against one pool" >&2
        echo "              and needs no layer at all." >&2
        ;;
    *)
        echo "crcbl vk e2e: the layer sees across submissions, so cross-frame hazards \
were in scope"
        ;;
esac

# The sandbox's own frame, headless, against the same implementation. This is
# the thing `docs/plan/02-vulkan-backend.md`'s milestone 1 is measured by — a
# clear reaching the screen through the whole shell→HAL→swapchain join — and it
# is the only part of that path a windowless CI runner can exercise.
echo "crcbl vk e2e: running the sandbox headless against Vulkan"
SANDBOX_LOG="$(mktemp -t crcbl-vk-sandbox.XXXXXX.log)"
set +e
cargo run --locked --quiet --package sandbox -- \
    --headless --backend vk --frames 30 2>&1 | tee "$SANDBOX_LOG"
SANDBOX_STATUS=${PIPESTATUS[0]}
set -e
if [ "$SANDBOX_STATUS" -ne 0 ]; then
    echo "crcbl vk e2e: the sandbox failed against Vulkan (exit $SANDBOX_STATUS)" >&2
    rm -f "$SANDBOX_LOG"
    exit "$SANDBOX_STATUS"
fi
if ! grep -q "30 frames" "$SANDBOX_LOG"; then
    echo "crcbl vk e2e: the sandbox did not report 30 frames" >&2
    cat "$SANDBOX_LOG" >&2
    rm -f "$SANDBOX_LOG"
    exit 1
fi
rm -f "$SANDBOX_LOG"
echo "crcbl vk e2e: the sandbox presented 30 frames through the Vulkan backend"

# And the null backend still works, on the same binary, selected at runtime.
# `docs/plan/10-wasm-webgpu.md`'s "does it repro on the other backend?" triage
# only exists if both are reachable without a rebuild.
echo "crcbl vk e2e: running the sandbox headless against the null backend"
if ! cargo run --locked --quiet --package sandbox -- \
    --headless --backend null --frames 10 | grep -q "10 frames"; then
    echo "crcbl vk e2e: the null backend is no longer reachable at runtime" >&2
    exit 1
fi
echo "crcbl vk e2e: both backends are selectable from one binary"

# And a run that asked for validation to be able to fail it, on a machine where
# it cannot, **refuses to start**.
#
# `CRCBL_VK_VALIDATION_FATAL=1` is what the seven `ci.yml` steps set, and
# without the layer it would be satisfied by nothing at all: no messenger, no
# errors, exit 0, and a step reporting that a sample drew cleanly under a
# validation gate that was never there. The refusal is the only thing standing
# between that variable and a green light wired to nothing, so it is asserted
# here rather than trusted.
#
# `VK_LAYER_PATH` at an empty directory is how the layer is hidden — the
# loader's own mechanism, so this proves the real path rather than a test hook.
# The layer is genuinely installed on any machine that got this far, which is
# what makes the assertion meaningful: the run below fails *because of the
# variable*, not because the box is bare.
echo "crcbl vk e2e: running the sandbox with a fatal validation gate it cannot honour"
NO_LAYERS="$(mktemp -d -t crcbl-vk-no-layers.XXXXXX)"
REFUSAL_LOG="$(mktemp -t crcbl-vk-refusal.XXXXXX.log)"
set +e
VK_LAYER_PATH="$NO_LAYERS" CRCBL_VK_VALIDATION=1 CRCBL_VK_VALIDATION_FATAL=1 \
    cargo run --locked --quiet --package sandbox -- \
    --headless --backend vk --frames 5 >"$REFUSAL_LOG" 2>&1
REFUSAL_STATUS=$?
set -e
rmdir "$NO_LAYERS"
if [ "$REFUSAL_STATUS" -eq 0 ]; then
    echo "crcbl vk e2e: the sandbox ran to completion with CRCBL_VK_VALIDATION_FATAL=1" >&2
    echo "              and no validation layer to honour it, so that variable can be" >&2
    echo "              set on a machine without the layer and change nothing — which" >&2
    echo "              is what it exists to stop" >&2
    cat "$REFUSAL_LOG" >&2
    rm -f "$REFUSAL_LOG"
    exit 1
fi
if ! grep -qF "CRCBL_VK_VALIDATION_FATAL=1 cannot be honoured" "$REFUSAL_LOG"; then
    echo "crcbl vk e2e: the sandbox failed with the fatal gate unavailable, but not" >&2
    echo "              for that reason — so this proves nothing about the refusal" >&2
    cat "$REFUSAL_LOG" >&2
    rm -f "$REFUSAL_LOG"
    exit 1
fi
rm -f "$REFUSAL_LOG"
echo "crcbl vk e2e: a fatal validation gate that cannot be honoured refuses to open"
