#!/usr/bin/env bash
# Draw alcove's court on a named backend, check every occlusion claim, and
# compare the frames against their checked-in goldens.
#
#   CRCBL_GPU=vk apps/alcove/tests/run-alcove-golden.sh [extra nextest args…]
#
# # What this is for
#
# alcove's milestones 1 and 2. The suite is
# `apps/alcove/tests/golden.rs`, and what it is about is not whether a frame
# drew: an occlusion pass that never ran leaves a white channel, one whose
# intensity is stuck at zero leaves the same, and a technique selector wired to
# one pipeline draws two identical halves either side of the comparison seam.
# Every one of those produces a picture somebody would bless. So the goldens are
# the last thing this suite checks, and the assertions before them are about
# where the frame is dark, by how much, and which gather drew which column.
#
# # What every sample's golden harness shares
#
# `tools/run-sample-golden.sh`: why the backend must be named, how a driver and
# an adapter are pinned, the `CRCBL_GPU` / `CRCBL_ADAPTER` / `CRCBL_VK_ICD` /
# `CRCBL_BLESS` those are read from, and the three checks made after the run.

set -euo pipefail

# shellcheck source=tools/run-sample-golden.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/tools/run-sample-golden.sh"

crcbl_sample_golden alcove "$@"

echo "alcove golden: the court drew, made its occlusion claims and matched on $CRCBL_GPU"
