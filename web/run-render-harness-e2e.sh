#!/usr/bin/env bash
# The WebGPU parity gate: drive crcbl's backend-agnostic golden `Scene` set
# through the `crcbl-webgpu` browser backend, offscreen, and report — per scene —
# how far the open got. It is the only check that can say whether
# `crcbl::screenshot`'s offscreen path can be driven through `crcbl-webgpu` at
# all; the native `vk`/`mtl`/`dx12` render-e2e suites cannot, because there is no
# browser in them.
#
#   ./web/run-render-harness-e2e.sh
#
# It builds `apps/render-harness` to wasm with `--features crcbl/webgpu` (so the
# auto-selected backend is `crcbl-webgpu`), assembles a tiny site next to the
# engine's GPU transport, and runs `web/tools/render-harness-e2e.mjs`, which
# loads it in headless Chromium under SwiftShader and reads the per-scene verdict
# back out of wasm memory over the DevTools protocol.
#
# WHAT IT NEEDS
#   * wasm-bindgen, pinned to the Cargo.lock version, exactly as web/build.sh.
#   * Node 22+ (for the global WebSocket the DevTools client uses).
#   * A Chromium/Chrome with WebGPU. CRCBL_CHROMIUM pins one; otherwise the usual
#     four names are tried. No Xvfb is needed: the harness reads an offscreen
#     target back into wasm memory, so there is no canvas to snapshot.
#
# EXIT CODES are the driver's: 0 every scene rendered, 1 the harness ran but at
# least one scene did not (the crack list is on stdout), 2 it could not run.
#
# NOT YET A CI STEP. Until the offscreen surface command lands in `crcbl-webgpu`,
# every scene refuses at the same wall and this exits 1 by design — see
# docs/backlog.md. The parent wires CI once the wall is closed.
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

echo "==> driving the golden scenes in the browser"
exec node "$REPO/web/tools/render-harness-e2e.mjs" "$SITE"
