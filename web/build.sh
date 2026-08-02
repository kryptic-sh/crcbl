#!/usr/bin/env bash
# Builds the demo site into `target/site/`.
#
# The same script the Pages workflow runs, so "it works in CI" and "it works on
# my machine" are the same claim. Nothing here is npm: the only tool is
# `wasm-bindgen`, pinned below to the version in `Cargo.lock`.
#
#   ./web/build.sh                 # build everything into target/site
#   ./web/build.sh --serve         # …and serve it on http://localhost:8000
#
# Serving matters: ES modules and `WebAssembly.instantiateStreaming` do not work
# from `file://`, and OPFS needs a secure context, which `localhost` is.
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
)

# The `wasm-bindgen` CLI must match the `wasm-bindgen` crate the build resolved,
# or it refuses with a version-mismatch error. Read it from the lockfile rather
# than pinning it twice.
bindgen_version() {
  awk '/^name = "wasm-bindgen"$/ { found = 1; next }
       found && /^version = / { gsub(/[",]/, "", $3); print $3; exit }' "$REPO/Cargo.lock"
}

BINDGEN_VERSION="$(bindgen_version)"
if [ -z "$BINDGEN_VERSION" ]; then
  echo "web/build.sh: no wasm-bindgen in Cargo.lock — is wgpu still a dependency?" >&2
  exit 1
fi

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "web/build.sh: wasm-bindgen not found." >&2
  echo "  cargo install wasm-bindgen-cli --version $BINDGEN_VERSION --locked" >&2
  exit 1
fi

HAVE_VERSION="$(wasm-bindgen --version | awk '{print $2}')"
if [ "$HAVE_VERSION" != "$BINDGEN_VERSION" ]; then
  # Not a warning. A mismatched CLI produces glue whose imports the module does
  # not have, and the failure surfaces as a `LinkError` in a browser rather than
  # here.
  echo "web/build.sh: wasm-bindgen $HAVE_VERSION, but Cargo.lock has $BINDGEN_VERSION." >&2
  echo "  cargo install wasm-bindgen-cli --version $BINDGEN_VERSION --locked --force" >&2
  exit 1
fi

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

for row in "${DEMOS[@]}"; do
  IFS=: read -r crate lib dest <<<"$row"
  echo "==> cargo build --lib -p $crate --target $TARGET ($PROFILE)"
  # `--lib`: the package also has a bin, and building both writes two files with
  # the same name. See `apps/breakout/Cargo.toml`.
  (cd "$REPO" && cargo build --locked --lib -p "$crate" --target "$TARGET" "${profile_flag[@]}")

  wasm="$REPO/target/$TARGET/$PROFILE/$lib.wasm"
  echo "==> wasm-bindgen $lib.wasm"
  # `--target web`: a plain ES module the page imports, no bundler, no npm
  # package layout. The generated glue exists for `wgpu`'s `web-sys` calls; the
  # engine's own ABI is the hand-written `extern "C"` exports, which the module
  # keeps and which `init()` returns as `wasm`.
  wasm-bindgen --target web --no-typescript --out-dir "$SITE/$dest" "$wasm"

  echo "==> checking the JS↔wasm export contract"
  node "$REPO/web/tools/check-exports.mjs" "$SITE/$dest/${lib}_bg.wasm" --sample "$crate" --quiet

  echo "==> smoke-testing the artifact under node"
  node "$REPO/web/tools/smoke.mjs" "$SITE/$dest/${lib}_bg.wasm" --sample "$crate"
done

echo "==> $SITE"
find "$SITE" -type f | sed "s|$SITE/|    |" | sort

if [ "${1:-}" = "--serve" ]; then
  echo "==> http://localhost:8000/"
  cd "$SITE" && exec python3 -m http.server 8000
fi
