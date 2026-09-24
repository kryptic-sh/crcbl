#!/usr/bin/env bash
# Run the horde binary on a named backend, ask it for its last frame, and
# compare that frame against its checked-in golden.
#
#   CRCBL_GPU=vk apps/horde/tests/run-horde-golden.sh [extra nextest args…]
#
# # What this is for
#
# `docs/plan/12-testing.md` asks every sample for a determinism check **and** a
# golden frame. The determinism half is the crate's own unit tests and the
# `Run horde headless against lavapipe` step in `.github/workflows/ci.yml`, and
# neither of them contains a pixel — they pass unchanged whether the frame is
# correct, black or wrongly tonemapped. This is the other half.
#
# The suite is `apps/horde/tests/golden.rs`, and what it drives is the
# **compiled binary** rather than a scene it built itself: `--screenshot <PATH>`
# is an engine flag on `crcbl::args::Common`, so the frame it writes is the frame
# a player would have seen, arena, field and HUD band included.
#
# # Why the run is prefilled
#
# A default headless run of horde never leaves its title screen, because nothing
# presses a key — `field: 0` in the summary's `SceneStats`, and a golden of that
# would go on passing after every enemy sprite stopped drawing. The suite passes
# `--prefill`, the fixture horde's scale measurement already uses, which
# stages the field and starts the run through the same action map a player's key
# goes through. `apps/horde/tests/golden.rs` says how much and why, and refuses
# the frame if the summary does not report the run as `Playing`.
#
# # What every sample's golden harness shares
#
# `tools/run-sample-golden.sh`: why the backend must be named, how a driver and
# an adapter are pinned, the `CRCBL_GPU` / `CRCBL_ADAPTER` / `CRCBL_VK_ICD` /
# `CRCBL_BLESS` those are read from, and the three checks made after the run.

set -euo pipefail

# shellcheck source=tools/run-sample-golden.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/tools/run-sample-golden.sh"

crcbl_sample_golden horde "$@"

echo "horde golden: the horde drew, made its claims and matched on $CRCBL_GPU"
