#!/usr/bin/env bash
# Run the `crcbl` CLI's scaffold end-to-end suite.
#
#   crates/crcbl-cli/tests/run-cli-e2e.sh [extra nextest args…]
#
# The suite scaffolds a project with `crcbl new`, compiles it, lints it, and
# runs it headless. That means a full engine compile into a throwaway target
# directory, so — exactly like `crates/crcbl-shell/tests/run-wayland-e2e.sh` —
# the tests are feature-gated *and* `#[ignore]`d, this script is the only thing
# that turns them on, and CI runs this script.
#
# `docs/plan/12-testing.md` calls a silently-skipped e2e job a known trap, so
# the script fails when the suite reports zero tests run.
#
# It needs no display, no GPU and no compositor: the scaffolded game runs
# against `HeadlessShell` and, by default, the null GPU backend — which is the
# entire point of the CLI/headless pillar. Set `CRCBL_CLI_E2E_BACKEND=vk` (with
# a driver installed) to put the template's render graph in front of a real one
# instead; CI does exactly that against lavapipe.
#
# On `vk` the suite also puts the scaffolded run under the validation layer and
# makes what the layer says fatal, then proves that check can fail by running
# the scaffold once more with an injected validation error. That needs
# `VK_LAYER_KHRONOS_validation` installed (Arch: vulkan-validation-layers,
# Debian/Ubuntu: vulkan-validationlayers) — `crcbl-vk` refuses to open without
# it rather than running unvalidated.
#
# **When sway is installed it also runs the generated project windowed**, on a
# private headless compositor started by `crcbl-shell`'s `sway-session.sh`. That
# is the half `--headless` cannot reach — a real surface, joined to the device,
# with a swapchain at the size the window system configured — and it is the
# first thing anybody does with a scaffold, which until now had never been run.
# The pass is skipped, loudly, on a machine with no sway; in CI, where sway is
# installed on purpose, a skip is a failure.
#
# It does need `rustfmt` and `clippy`, which `rust-toolchain.toml` installs.

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${CRATE_DIR}/../.." && pwd)"

# The suite starts a nested `cargo` for the scaffolded project. Inheriting an
# outer `CARGO_TARGET_DIR` would point that build at the directory this very
# test run holds a lock on, which deadlocks rather than fails. The test sets the
# variable per child too; this is the belt to that pair of braces.
unset CARGO_TARGET_DIR

for tool in rustfmt cargo-clippy; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "crcbl e2e: $tool is not installed; the scaffold's lint checks need it" >&2
        exit 1
    fi
done

RUNTIME_DIR="$(mktemp -d -t crcbl-cli-e2e.XXXXXX)"

# Starting a compositor is `crcbl-shell`'s knowledge, and it is sourced rather
# than copied: two hand-rolled socket polls is where the two drift apart. It
# sets `SWAY_RUNTIME_DIR`, `WAYLAND_DISPLAY`, `SWAYSOCK`, `SWAY_LOG` and
# `SWAY_PID`, and defines `sway_log_tail` and `sway_session_stop` — the last of
# which does nothing when the session never started.
# shellcheck source=crates/crcbl-shell/tests/sway-session.sh
source "${REPO_ROOT}/crates/crcbl-shell/tests/sway-session.sh"

# And reading nextest's own summary is `tools/nextest-summary.sh`'s, for the
# same reason: eight harnesses carried that inline and five of them ended up
# reading a cancelled run's `2/15 tests run` as a healthy fifteen.
# shellcheck source=tools/nextest-summary.sh
source "${REPO_ROOT}/tools/nextest-summary.sh"

cleanup() {
    local status=$?
    sway_session_stop
    rm -rf "$RUNTIME_DIR"
    exit "$status"
}
trap cleanup EXIT INT TERM

# The windowed pass, when there is something to be windowed on. The suite reads
# `CRCBL_CLI_E2E_WINDOWED` and inherits `WAYLAND_DISPLAY` from here.
if command -v sway >/dev/null 2>&1; then
    sway_session_start "${CRATE_DIR}/tests/cli-e2e-sway.conf"
    export CRCBL_CLI_E2E_WINDOWED=1
else
    echo "crcbl e2e: no sway; the scaffold will only be run headless" >&2
    if [ -n "${CI:-}" ]; then
        echo "crcbl e2e: ...and this is CI, where sway is installed on purpose" >&2
        exit 1
    fi
fi

# **And in CI the backend has to be the one that can be validated.** The two
# validation passes in `cli_e2e.rs` return early on any other backend — there is
# no layer to ask — and an early return is a pass, so dropping
# `CRCBL_CLI_E2E_BACKEND` from the job would leave this suite green while
# checking nothing about the layer at all. The line below it already says which
# backend ran; this is what makes the wrong answer fail rather than print.
#
# A developer's machine keeps the null default, which is the whole point of the
# CLI/headless pillar: `crcbl new` has to work where there is no driver.
if [ -n "${CI:-}" ] && [ "${CRCBL_CLI_E2E_BACKEND:-null}" != "vk" ]; then
    echo "crcbl e2e: CRCBL_CLI_E2E_BACKEND is '${CRCBL_CLI_E2E_BACKEND:-null}' and this is" >&2
    echo "           CI, which sets it to vk on purpose — the scaffold has to meet a real" >&2
    echo "           driver and a real validation layer here, and on any other backend the" >&2
    echo "           suite's validation passes return early and count as passes." >&2
    exit 1
fi

cd "$REPO_ROOT"
OUTPUT="${RUNTIME_DIR}/nextest.log"
set +e
cargo nextest run \
    --locked \
    --package crcbl-cli \
    --features cli-e2e \
    --test cli_e2e \
    --run-ignored all \
    --test-threads 1 \
    "$@" 2>&1 | tee "$OUTPUT"
STATUS=${PIPESTATUS[0]}
set -e

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl e2e: the CLI suite failed" >&2
    exit "$STATUS"
fi

# The trap `docs/plan/12-testing.md` names by name: a job that skips everything
# and reports success is worse than no job — and so is one that stopped after two
# tests and printed a total that reads like fifteen.
PLAIN="${RUNTIME_DIR}/nextest.plain.log"
crcbl_nextest_plain "$OUTPUT" "$PLAIN"
if ! crcbl_nextest_summary "$PLAIN" "crcbl e2e" \
    "The cli-e2e feature or the ignore attribute stopped matching the tests."; then
    exit 1
fi
echo "crcbl e2e: $CRCBL_NEXTEST_TESTS_RUN CLI scaffold tests ran on the ${CRCBL_CLI_E2E_BACKEND:-null} GPU backend"

# Said out loud, because "the tests passed" does not distinguish a run that
# opened a window from one that skipped that step — which is the whole reason
# the suite reads an environment variable rather than probing for a compositor
# itself.
if [ -n "${CRCBL_CLI_E2E_WINDOWED:-}" ]; then
    echo "crcbl e2e: the scaffold was also run windowed against headless sway"
fi
