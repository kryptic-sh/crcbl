#!/usr/bin/env bash
# Draw lantern's room on a named backend, check every lighting claim, and
# compare the frame against its checked-in golden.
#
#   CRCBL_GPU=vk apps/lantern/tests/run-lantern-golden.sh [extra nextest args…]
#
# # What this is for
#
# Lantern's milestone 1a, the raster room. The suite is
# `apps/lantern/tests/golden.rs`, and it is the only thing in the tree that renders
# an **application's** `SceneDesc` — every other frame comes from
# `crcbl_render::scene::demo`. A description that reached the device short by a
# mesh, a material row or a page layer draws a perfectly plausible room, so the
# claims in front of the golden are about where the frame is bright and dark
# rather than about whether it drew.
#
# # Two goldens, and only one of them comes out of this process
#
# `tests/golden/room.png` is the room from the fixed camera through
# `crcbl::screenshot::OffscreenSetup`, which renders **one** view — so the
# monitor hanging on the back wall is a black screen in it, and that is honest
# rather than a defect: nothing in that harness feeds the screen.
#
# `tests/golden/live.png` is the frame the **binary** presented, on the shape
# every other sample's golden suite already uses: the second view and the copy
# that puts a picture on that screen are recorded by the sample's own frame, so a
# live screen only exists in a run the binary made.
#
# # What every sample's golden harness shares
#
# `tools/run-sample-golden.sh`: why the backend must be named, how a driver and
# an adapter are pinned, the `CRCBL_GPU` / `CRCBL_ADAPTER` / `CRCBL_VK_ICD` /
# `CRCBL_BLESS` those are read from, and the three checks made after the run.

set -euo pipefail

# shellcheck source=tools/run-sample-golden.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)/tools/run-sample-golden.sh"

crcbl_sample_golden lantern "$@"

echo "lantern golden: the room drew, made its lighting claims and matched on $CRCBL_GPU"
