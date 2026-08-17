#!/usr/bin/env bash
# Builds the demo site into `target/site/`.
#
# The same script the Pages workflow runs, so "it works in CI" and "it works on
# my machine" are the same claim. Nothing here is npm, and since `crcbl-wgpu`
# stopped being a wasm dependency there is no `wasm-bindgen` either — `cargo`,
# `python3` and `node` are the whole tool list. See "no wasm-bindgen" below.
#
#   ./web/build.sh                 # build everything into target/site
#   ./web/build.sh --serve         # …and serve it on http://localhost:8000
#
# Serving matters: ES modules and `WebAssembly.instantiateStreaming` do not work
# from `file://`, and OPFS needs a secure context, which `localhost` is.
#
# `--serve` runs `web/tools/serve.mjs`, which is the same server the browser
# e2e uses and therefore sends the same COOP/COEP pair — see that file for why
# cross-origin isolation is what a threaded wasm build stands on. Sharing it is
# deliberate: the gate asserts `crossOriginIsolated === true`, and a separate
# `python3 -m http.server` for humans would be an origin the gate never sees.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SITE="${SITE_DIR:-$REPO/target/site}"
PROFILE="${PROFILE:-release}"
TARGET=wasm32-unknown-unknown

# Every wasm-ready sample: crate name, lib name, and where it goes in the site.
# One row per demo — a new sample is a line here and a directory under
# `web/demos/`.
DEMOS=(
  "breakout:crcbl_breakout:demos/breakout"
  "flappy:crcbl_flappy:demos/flappy"
  "asteroids:crcbl_asteroids:demos/asteroids"
  "horde:crcbl_horde:demos/horde"
  "hud:crcbl_hud:demos/hud"
  "lumen:crcbl_lumen:demos/lumen"
)

# NO wasm-bindgen. This script used to run it over every artifact, and the pin
# it needed — the CLI version has to equal the `wasm-bindgen` crate the build
# resolved — was the one piece of tooling setup this repository asked of anyone
# building the site.
#
# It is gone because the reason for it is. `wgpu` reached WebGPU through
# `web-sys`, `web-sys` *is* `wasm-bindgen`, and `crcbl-wgpu` was an
# unconditional dependency of the umbrella; a raw artifact therefore imported
# ~340 functions from `__wbindgen_placeholder__` and would not instantiate
# without the tool resolving them. `crcbl-wgpu` is a
# `cfg(not(target_arch = "wasm32"))` dependency now, nothing else in a browser
# build reaches `web-sys`, and the artifacts import **nothing at all** —
# `web/tools/check-exports.mjs` is what asserts that, per demo, every build.
#
# Not merely unnecessary: impossible. With no `wasm-bindgen` crate linked, its
# runtime intrinsics are absent and the CLI exits with `failed to find
# intrinsics to enable ‘clone_ref’ function` rather than passing the module
# through. So the choice was to keep a browser GPU backend nothing renders
# through, or to replace the tool's one remaining product — the `<lib>.js` that
# pages `import init from` — with `web/tools/wasm-loader.js`, which is what the
# copy below does. That file documents the contract it preserves.

echo "==> assembling $SITE"
rm -rf "$SITE"
mkdir -p "$SITE"
# The rendered half: every page is `templates/layout.html` filled from a file
# in `pages/`, so the header, the demo bar and the footer live in one place.
echo "==> rendering pages"
python3 "$REPO/web/build-pages.py" "$SITE"

# The static half: everything in `web/` except the build tooling and the
# template sources, which are inputs rather than output.
#
# `-name '*.sh'` rather than naming each script: this list silently gained
# `run-browser-e2e.sh` when the browser gate landed, and shipped it to the
# demo site. A prune that has to be extended by hand every time a script is
# added is one that will be wrong again.
(cd "$REPO/web" && find . \
  -path ./tools -prune -o \
  -path ./pages -prune -o \
  -path ./templates -prune -o \
  -name '*.sh' -prune -o \
  -name '*.py' -prune -o \
  -name README.md -prune -o \
  -type f -print) | while read -r file; do
  mkdir -p "$SITE/$(dirname "$file")"
  cp "$REPO/web/$file" "$SITE/$file"
done

profile_flag=()
[ "$PROFILE" = "release" ] && profile_flag=(--release)

# THERE IS NO BACKEND CHOICE HERE ANY MORE, AND THAT IS THE POINT.
#
# This used to read `CRCBL_WEB_BACKEND` and pass `--features crcbl/webgpu` for
# one of its two values, because a browser build linked both `crcbl-wgpu` and
# `crcbl-webgpu` and something had to say which one `crcbl::backend` picked.
# `crcbl-wgpu` is a `cfg(not(target_arch = "wasm32"))` dependency of the umbrella
# now (see `crates/crcbl/Cargo.toml`), so a wasm build has exactly one GPU
# backend and the feature that chose between them is gone with the choice.
#
# Old invocations that still export `CRCBL_WEB_BACKEND=webgpu` get what they
# asked for: the variable is ignored and `crcbl-webgpu` is what builds.
echo "==> browser GPU backend: webgpu (crcbl-webgpu, the only one a wasm build links)"

for row in "${DEMOS[@]}"; do
  IFS=: read -r crate lib dest <<<"$row"
  echo "==> cargo build --lib -p $crate --target $TARGET ($PROFILE)"
  # `--lib`: the package also has a bin, and building both writes two files with
  # the same name. See `apps/breakout/Cargo.toml`.
  (cd "$REPO" && cargo build --locked --lib -p "$crate" --target "$TARGET" "${profile_flag[@]}")

  wasm="$REPO/target/$TARGET/$PROFILE/$lib.wasm"
  echo "==> publishing $lib.wasm"
  # The same two filenames `wasm-bindgen --target web` produced, because every
  # page imports them by name: `<lib>_bg.wasm` is the artifact, unmodified, and
  # `<lib>.js` is the ES module whose default export instantiates it. One
  # loader serves them all — it finds the module beside itself — so this is a
  # copy rather than a generated file per demo.
  mkdir -p "$SITE/$dest"
  cp "$wasm" "$SITE/$dest/${lib}_bg.wasm"
  cp "$REPO/web/tools/wasm-loader.js" "$SITE/$dest/$lib.js"

  echo "==> checking the JS↔wasm export contract"
  node "$REPO/web/tools/check-exports.mjs" "$SITE/$dest/${lib}_bg.wasm" --sample "$crate" --quiet

  echo "==> smoke-testing the artifact under node"
  node "$REPO/web/tools/smoke.mjs" "$SITE/$dest/${lib}_bg.wasm" --sample "$crate"
done

echo "==> $SITE"
find "$SITE" -type f | sed "s|$SITE/|    |" | sort

if [ "${1:-}" = "--serve" ]; then
  exec node "$REPO/web/tools/serve.mjs" "$SITE" --port "${PORT:-8000}"
fi
