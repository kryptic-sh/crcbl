#!/usr/bin/env bash
# Run the hud binary on a named backend, ask it for its last frame, and
# compare that frame against its checked-in golden.
#
#   CRCBL_GPU=vk apps/hud/tests/run-hud-golden.sh [extra nextest args…]
#
# # What this is for
#
# `docs/notes/process.md` asks every sample for a determinism check **and** a
# golden frame. The determinism half is the crate's own unit tests and the
# `Run hud headless against lavapipe` step in `.github/workflows/ci.yml`, and
# neither of them contains a pixel — they pass unchanged whether the frame is
# correct, black or wrongly tonemapped. This is the other half.
#
# The suite is `apps/hud/tests/golden.rs`, and what it drives is the
# **compiled binary** rather than a scene it built itself: `--screenshot <PATH>`
# is an engine flag on `crcbl::args::Common`, so the frame it writes is the frame
# a player would have seen, every widget on the page included.
#
# # What every sample's golden harness shares
#
# `tools/run-sample-golden.sh`: why the backend must be named, how a driver and
# an adapter are pinned, the `CRCBL_GPU` / `CRCBL_ADAPTER` / `CRCBL_VK_ICD` /
# `CRCBL_BLESS` those are read from, and the three checks made after the run.

set -euo pipefail

# shellcheck source=tools/run-sample-golden.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/tools/run-sample-golden.sh"

crcbl_sample_golden hud "$@"

echo "hud golden: the page drew, made its claims and matched on $CRCBL_GPU"
