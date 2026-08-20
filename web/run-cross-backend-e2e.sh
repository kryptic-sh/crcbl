#!/usr/bin/env bash
# Hold the browser's pixels against a native backend's, directly.
#
#   ./web/run-cross-backend-e2e.sh [--reference vk] [--expect-fail ssr,ui]
#
# # What this is and why it is separate
#
# `web/run-render-harness-e2e.sh` compares every scene the browser backend draws
# against the committed golden. This compares the same readbacks against a frame
# a **native backend rendered in this run** — no committed image between them, so
# the two backends are held against each other rather than each against a
# reference that could have drifted with them.
#
# That is what `the native vk-against-wgpu gate` does for vk against
# wgpu, and it is the one thing `crcbl-wgpu` is still kept for. This is its
# replacement, and a wider one: eleven scenes rather than three, and against a
# backend that is a genuinely separate implementation instead of a second
# abstraction over the same Vulkan driver.
#
# # It reuses the harness's readbacks rather than driving the browser again
#
# Building the wasm module and driving headless Chromium is the expensive half
# and it is identical for both comparisons, so this takes a site directory that
# `run-render-harness-e2e.sh` has already filled. Run them back to back with the
# same `SITE_DIR` and the browser runs once.
#
# # Measured, so the numbers here are not a guess
#
# On 2026-08-19, radv against Chromium-on-SwiftShader: nine of eleven scenes
# match, `sprite` is **byte-identical**, and the two that do not are `ssr` and
# `ui` — the same two `run-render-harness-e2e.sh` already carries. The direct
# comparison is *tighter* than the golden one rather than looser: `ssr` differs
# in 1355 pixels here against 25611 against the golden.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# The same pin every native harness uses, sourced rather than reimplemented:
# `CRCBL_VK_ICD` names a manifest whose basename differs between distributions
# (`lvp_icd.json` on Arch, `lvp_icd.x86_64.json` on Debian), and resolving it
# here by hand would be a second copy that is wrong on one of them.
# shellcheck source=crates/crcbl-vk/tests/vulkan-icd.sh
source "$REPO/crates/crcbl-vk/tests/vulkan-icd.sh"
crcbl_pin_vk_icd "crcbl cross-backend"
SITE="${SITE_DIR:-$REPO/target/render-harness-site}"
REFERENCE="${CRCBL_CROSS_REFERENCE:-vk}"
EXPECT_FAIL="${CRCBL_CROSS_EXPECT_FAIL:-}"
# The size the harness renders at. Not configurable: the readback file names
# carry the extent the browser used, and a reference rendered at another size
# would be rejected by the comparator rather than compared.
SIZE=256x192

while [ "$#" -gt 0 ]; do
  case "$1" in
    --reference)
      [ "$#" -ge 2 ] || { echo "run-cross-backend-e2e.sh: --reference needs a backend" >&2; exit 2; }
      REFERENCE="$2"
      shift 2
      ;;
    --expect-fail)
      [ "$#" -ge 2 ] || { echo "run-cross-backend-e2e.sh: --expect-fail needs a scene list" >&2; exit 2; }
      EXPECT_FAIL="$2"
      shift 2
      ;;
    *)
      echo "run-cross-backend-e2e.sh: unknown argument $1" >&2
      exit 2
      ;;
  esac
done

READBACK="$SITE/readback"
if [ ! -d "$READBACK" ]; then
  echo "run-cross-backend-e2e.sh: $READBACK does not exist." >&2
  echo "  This compares readbacks web/run-render-harness-e2e.sh produced; run" >&2
  echo "  that first with the same SITE_DIR." >&2
  exit 2
fi
# A directory that exists and is empty is the shape that would otherwise render
# eleven reference frames, compare nothing, and report success.
if [ -z "$(ls -A "$READBACK" 2>/dev/null)" ]; then
  echo "run-cross-backend-e2e.sh: $READBACK is empty, so there is nothing to compare." >&2
  exit 2
fi

# Checked here rather than beside its use, so a site that cannot produce a
# verdict costs a message instead of eleven reference renders first.
DRIVER_JSON="$SITE/driver-result.json"
if [ ! -f "$DRIVER_JSON" ]; then
  echo "run-cross-backend-e2e.sh: $DRIVER_JSON is missing, so the browser half of" >&2
  echo "  each scene's outcome is unknown. Run web/run-render-harness-e2e.sh with" >&2
  echo "  the same SITE_DIR first." >&2
  exit 2
fi

REFERENCE_DIR="$SITE/reference-$REFERENCE"
rm -rf "$REFERENCE_DIR"
mkdir -p "$REFERENCE_DIR"

cd "$REPO"

# The scene list comes from the readbacks themselves — `<scene>.<w>x<h>.<order>.bin`
# — rather than from a list written out here. Two reasons, and the second is the
# load-bearing one: a copy of the list goes stale the day a scene is added, and
# more importantly this is the set the browser *actually produced* in this run,
# so a reference is rendered for exactly what there is something to compare it
# against.
SCENES="$(
  for file in "$READBACK"/*.bin; do
    name="$(basename "$file")"
    echo "${name%%.*}"
  done | sort -u
)"
if [ -z "$SCENES" ]; then
  echo "run-cross-backend-e2e.sh: no readback in $READBACK names a scene." >&2
  exit 2
fi

echo "==> rendering each scene through $REFERENCE at $SIZE"
COUNT=0
for SCENE in $SCENES; do
  if ! CRCBL_GPU="$REFERENCE" cargo run --locked --quiet -p crcbl-cli --bin crcbl -- \
      screenshot --scene "$SCENE" --size "$SIZE" --output "$REFERENCE_DIR/$SCENE.png" >/dev/null; then
    echo "run-cross-backend-e2e.sh: $REFERENCE could not render $SCENE" >&2
    exit 1
  fi
  COUNT=$((COUNT + 1))
done
echo "==> $COUNT scene(s) rendered through $REFERENCE"

echo "==> comparing the browser's readbacks against $REFERENCE's frames"
compare=0
cargo run --locked -p render-harness --example compare-readback -- \
  "$READBACK" --golden-dir "$REFERENCE_DIR" > "$SITE/cross-backend.log" || compare=$?
cat "$SITE/cross-backend.log"
# 0 matched, 1 did not, 2 bad usage or unreadable input — anything else is the
# comparator failing to build or run, which is a gate that did not run.
if [ "$compare" -gt 1 ]; then
  echo "run-cross-backend-e2e.sh: the comparator exited $compare, so nothing was compared" >&2
  exit "$compare"
fi

# The same verdict step the golden gate uses, and deliberately the same one: the
# expected-failure list has to be exact in both directions — a listed scene that
# *matched* fails the run as a stale list — and re-implementing that here would
# be a second copy of the only rule that keeps the list from rotting into a
# blanket suppression.
#
# The driver JSON is the harness run's, because the driver half of each scene's
# outcome is the same in both comparisons: the browser either got the scene
# through or it did not, and only what its pixels are held against differs. The
# driver exit is 0 for the same reason — this script refused to start unless that
# run left readbacks behind.
echo "==> the verdict, $REFERENCE against the browser"
node "$REPO/web/tools/render-harness-verdict.mjs" \
  --driver-json "$DRIVER_JSON" \
  --compare-log "$SITE/cross-backend.log" \
  --driver-exit 0 \
  --compare-exit "$compare" \
  ${EXPECT_FAIL:+--expect-fail "$EXPECT_FAIL"}
