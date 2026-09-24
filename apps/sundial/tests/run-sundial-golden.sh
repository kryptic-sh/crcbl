#!/usr/bin/env bash
# Draw sundial's plaza on a named backend, check every shadow claim, and compare
# the frames against their checked-in goldens.
#
#   CRCBL_GPU=vk apps/sundial/tests/run-sundial-golden.sh [extra nextest args…]
#
# # What this is for
#
# sundial's milestone 1. The suite is
# `apps/sundial/tests/golden.rs`, and what it is about is not whether a frame
# drew: a PCSS blocker search that never ran draws the same picture the
# fixed-width disc does, a filter selector wired to one branch draws two
# identical halves either side of the comparison seam, and a bias that detached
# every shadow from its caster still fills the frame with shadows. Every one of
# those produces a picture somebody would bless. So the goldens are the last
# thing this suite checks, and the assertions before them are about how wide the
# penumbrae are, which column ran which filter, where the frame is dark, and
# whether one tick of the scripted clock draws one frame.
#
# # What every sample's golden harness shares
#
# `tools/run-sample-golden.sh`: why the backend must be named, how a driver and
# an adapter are pinned, the `CRCBL_GPU` / `CRCBL_ADAPTER` / `CRCBL_VK_ICD` /
# `CRCBL_BLESS` those are read from, and the three checks made after the run.

set -euo pipefail

# shellcheck source=tools/run-sample-golden.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/tools/run-sample-golden.sh"

crcbl_sample_golden sundial "$@"

echo "sundial golden: the plaza drew, made its shadow claims and matched on $CRCBL_GPU"
