#!/usr/bin/env bash
# Draw tide's courtyard on a named backend, check every water claim, and compare
# the frames against their checked-in goldens.
#
#   CRCBL_GPU=vk apps/tide/tests/run-tide-golden.sh [extra nextest args…]
#
# # What this is for
#
# `docs/plan/sample/21-tide.md`'s milestone 1. The suite is
# `apps/tide/tests/golden.rs`, and what it is about is not whether a frame drew:
# a body never handed to the renderer, a medium preset that never reached the
# shader and a surface that absorbed nothing with depth each draw a pool somebody
# would bless. So the goldens are the last thing this suite checks, and the
# assertions before them are relations between bands of the frame — where the
# water is and is not, the deep end against the shallow end, and each medium
# against the others.
#
# # What every sample's golden harness shares
#
# `tools/run-sample-golden.sh`: why the backend must be named, how a driver and
# an adapter are pinned, the `CRCBL_GPU` / `CRCBL_ADAPTER` / `CRCBL_VK_ICD` /
# `CRCBL_BLESS` those are read from, and the three checks made after the run.

set -euo pipefail

# shellcheck source=tools/run-sample-golden.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/tools/run-sample-golden.sh"

crcbl_sample_golden tide "$@"

echo "tide golden: the courtyard drew, made its water claims and matched on $CRCBL_GPU"
