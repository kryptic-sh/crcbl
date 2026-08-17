#!/usr/bin/env bash
# The WebGPU parity gate: drive crcbl's backend-agnostic golden `Scene` set
# through the `crcbl-webgpu` browser backend, offscreen, read every frame back,
# and compare it against the very golden image the native suites compare
# against. It is the only check that can say whether a browser backend draws the
# same picture the `vk`/`mtl`/`dx12` render-e2e suites do; those cannot answer
# it, because there is no browser in them.
#
#   ./web/run-render-harness-e2e.sh
#
# TWO HALVES, RUN BACK TO BACK.
#
#   1. `web/tools/render-harness-e2e.mjs` builds nothing and decides nothing: it
#      loads the harness page in headless Chromium under SwiftShader, drives
#      every scene, and writes each readback to `$SITE/readback` as
#      `<scene>.<width>x<height>.<order>.bin`.
#   2. `cargo run -p render-harness --example compare-readback` compares those
#      against `crates/crcbl/tests/golden/<scene>.png` with `crcbl-golden`, at
#      `Tolerance::RASTERISER` — the same comparator and the same numbers the
#      native golden tests use. Nothing in JS diffs a pixel.
#
# Both halves report per scene, and BOTH decide the exit code: a scene that never
# rendered is the browser half's crack, and a scene that rendered the wrong
# picture is the comparator's. Either one is a red gate.
#
# It builds `apps/render-harness` to wasm with `--features crcbl/webgpu`, which
# is what flips the auto-selected browser backend to `crcbl-webgpu`.
#
# WHAT IT NEEDS
#   * wasm-bindgen, pinned to the Cargo.lock version, exactly as web/build.sh.
#   * Node 22+ (for the global WebSocket the DevTools client uses).
#   * A Chromium/Chrome with WebGPU. CRCBL_CHROMIUM pins one; otherwise the usual
#     four names are tried. No Xvfb is needed: the harness reads an offscreen
#     target back into wasm memory, so there is no canvas to snapshot.
#
# EXIT CODES
#   0  every scene rendered AND matched its golden.
#   1  the harness ran, but a scene did not render or did not match. The two
#      tables are on stdout and name every one.
#   2  it could not run at all — no browser, no adapter, the wasm failed to load,
#      or the comparator could not be built.
#
# NOT YET A CI STEP: the parent wires that once the crack list is short enough to
# hold to, see docs/backlog.md.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SITE="${SITE_DIR:-$REPO/target/render-harness-site}"
PROFILE="${PROFILE:-release}"
TARGET=wasm32-unknown-unknown
CRATE=render-harness
LIB=crcbl_render_harness

bindgen_version() {
  awk '/^name = "wasm-bindgen"$/ { found = 1; next }
       found && /^version = / { gsub(/[",]/, "", $3); print $3; exit }' "$REPO/Cargo.lock"
}

BINDGEN_VERSION="$(bindgen_version)"
if [ -z "$BINDGEN_VERSION" ]; then
  echo "run-render-harness-e2e.sh: no wasm-bindgen in Cargo.lock" >&2
  exit 1
fi
if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "run-render-harness-e2e.sh: wasm-bindgen not found." >&2
  echo "  cargo install wasm-bindgen-cli --version $BINDGEN_VERSION --locked" >&2
  exit 1
fi
HAVE_VERSION="$(wasm-bindgen --version | awk '{print $2}')"
if [ "$HAVE_VERSION" != "$BINDGEN_VERSION" ]; then
  echo "run-render-harness-e2e.sh: wasm-bindgen $HAVE_VERSION, but Cargo.lock has $BINDGEN_VERSION." >&2
  echo "  cargo install wasm-bindgen-cli --version $BINDGEN_VERSION --locked --force" >&2
  exit 1
fi

profile_flag=()
[ "$PROFILE" = "release" ] && profile_flag=(--release)

echo "==> cargo build --lib -p $CRATE --target $TARGET ($PROFILE, webgpu)"
# `--features crcbl/webgpu` flips the auto-selected browser backend from
# crcbl-wgpu to crcbl-webgpu — the backend this gate exists to exercise.
(cd "$REPO" && cargo build --locked --lib -p "$CRATE" --target "$TARGET" "${profile_flag[@]}" --features crcbl/webgpu)

echo "==> assembling $SITE"
rm -rf "$SITE"
mkdir -p "$SITE/engine" "$SITE/harness"

# The engine's GPU transport and replayer, and the two modules they import. The
# harness page reuses these unchanged — the same drain→replay→deliver loop the
# demos run.
for js in gpu-transport.js gpu-replay.js gpu-stream.js gpu-reply.js; do
  cp "$REPO/web/engine/$js" "$SITE/engine/$js"
done
cp "$REPO/web/harness/index.html" "$SITE/harness/index.html"
cp "$REPO/web/harness/main.js" "$SITE/harness/main.js"

echo "==> wasm-bindgen $LIB.wasm"
wasm-bindgen --target web --no-typescript --out-dir "$SITE/harness" \
  "$REPO/target/$TARGET/$PROFILE/$LIB.wasm"

# Inside $SITE, which was just removed and rebuilt, so it starts empty every run
# — a stale readback from a previous run is a comparison against the wrong frame,
# and the comparator refuses two files for one scene rather than picking.
READBACK="$SITE/readback"
mkdir -p "$READBACK"

echo "==> driving the golden scenes in the browser"
driver=0
node "$REPO/web/tools/render-harness-e2e.mjs" "$SITE" --readback-dir "$READBACK" || driver=$?
# A driver that could not run at all leaves nothing to compare, so there is no
# point building the comparator to tell us the directory is empty.
if [ "$driver" -eq 2 ]; then
  exit 2
fi

echo "==> comparing each readback against crates/crcbl/tests/golden"
compare=0
(cd "$REPO" && cargo run --locked -p "$CRATE" --example compare-readback -- "$READBACK") || compare=$?
# 0 matched, 1 did not, 2 bad usage or unreadable input — anything else is cargo
# failing to build or run it at all, which is a gate that did not run rather than
# a gate that passed.
if [ "$compare" -gt 1 ]; then
  echo "run-render-harness-e2e.sh: the comparator exited $compare — nothing was compared" >&2
  exit 2
fi

if [ "$driver" -ne 0 ] || [ "$compare" -ne 0 ]; then
  echo "run-render-harness-e2e.sh: FAIL — driver exit $driver, comparator exit $compare" >&2
  exit 1
fi
echo "run-render-harness-e2e.sh: every scene rendered in the browser and matched its golden"
