#!/usr/bin/env bash
# Builds the demo site into `target/site/`.
#
# The same script the Pages workflow runs, so "it works in CI" and "it works on
# my machine" are the same claim. Nothing here is npm, and since `crcbl-wgpu`
# stopped being a wasm dependency there is no `wasm-bindgen` either — `cargo`
# and `node` are the whole tool list. See "no wasm-bindgen" below.
#
#   ./web/build.sh                 # build everything into target/site
#   ./web/build.sh --serve         # …and serve it on http://localhost:8000
#   ./web/build.sh --threads       # build the worker-capable site instead
#   ./web/build.sh --threads --serve       # …and serve it, cross-origin isolated
#   ./web/build.sh --threads --gate-only   # …only the worker-backend gate artifact
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

# `--threads` only. A second toolchain pinned by date, in the shape the
# `decoder-fuzz` job already uses: `-Z build-std` is nightly-only, and
# `rust-toolchain.toml` pins an exact stable on purpose — its own comment calls
# a floating channel a broken promise.
NIGHTLY=nightly-2026-07-02
THREADED_DIR="${THREADED_TARGET_DIR:-$REPO/target/wasm-threaded}"
# The site `--threads` assembles, and it is deliberately NOT `$SITE`: the Pages
# workflow builds `target/site` and uploads exactly that directory, so a
# threaded artifact reaching the deploy would have to be written there. Nothing
# below ever does.
THREADED_SITE="${THREADED_SITE_DIR:-$REPO/target/site-threaded}"
# The ceiling the shared memory declares. A shared memory must have one, and
# `web/tools/check-exports.mjs --threads` instantiates against the limits it
# reads out of the artifact rather than repeating this number.
THREADED_MAX_MEMORY_BYTES=1073741824

SERVE=0
THREADS=0
GATE_ONLY=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --serve)
      SERVE=1
      shift
      ;;
    --threads)
      THREADS=1
      shift
      ;;
    --gate-only)
      GATE_ONLY=1
      shift
      ;;
    *)
      echo "crcbl web build: unknown argument: $1" >&2
      echo "usage: ./web/build.sh [--serve] [--threads [--gate-only]]" >&2
      exit 2
      ;;
  esac
done

# THE PART BOTH SITES SHARE: the pages and every static file under `web/`.
#
# A function rather than a second copy, because the prune list below is the one
# thing here that decides what is published, and two copies of it is one copy
# that will be missing an entry. The two callers differ only in which artifact
# and which loader they lay beside each demo afterwards.
#
# $1 is the site directory, which is emptied first.
assemble_static() {
  local site="$1"
  echo "==> assembling $site"
  rm -rf "$site"
  mkdir -p "$site"
  # The rendered half: every page is `templates/layout.html` filled from a file
  # in `pages/`, so the header, the demo bar and the footer live in one place.
  echo "==> rendering pages"
  node "$REPO/web/tools/build-pages.mjs" "$site"

  # The static half: everything in `web/` except the build tooling and the
  # template sources, which are inputs rather than output.
  #
  # `-name '*.sh'` rather than naming each script: this list silently gained
  # `run-browser-e2e.sh` when the browser gate landed, and shipped it to the
  # demo site. A prune that has to be extended by hand every time a script is
  # added is one that will be wrong again.
  # `./jobs` is pruned with the build tooling rather than published with
  # `./probe` and `./harness`, and it is the one directory here whose absence is
  # a *correctness* requirement instead of tidiness: that page loads an artifact
  # that imports a shared `env.memory`, which cannot exist on an origin sending
  # no COOP/COEP pair. Published, it would be a page on the demo site that can
  # only fail. `web/run-jobs-e2e.sh` assembles its own site for it.
  #
  # `web/engine/jobs.js` and `web/engine/jobs-worker.js` are NOT pruned, and
  # that is the same judgement made the other way: they are the host half of the
  # spawn ABI, they refuse an artifact that owns its memory, and every published
  # artifact is one — so on the demo site they load, decide no, and announce
  # nothing.
  (cd "$REPO/web" && find . \
    -path ./tools -prune -o \
    -path ./jobs -prune -o \
    -path ./pages -prune -o \
    -path ./templates -prune -o \
    -name '*.sh' -prune -o \
    -name README.md -prune -o \
    -type f -print) | while read -r file; do
    mkdir -p "$site/$(dirname "$file")"
    cp "$REPO/web/$file" "$site/$file"
  done
}

# `--gate-only` narrows the threaded build to the one artifact
# `web/run-jobs-e2e.sh` drives, so a browser run does not pay for seven
# `-Z build-std` demo builds it never loads. It means nothing on its own: the
# default build has no gate artifact to be the only thing in it.
#
# It skips `check-exports.mjs --threads` with the demo loop, because that check
# has always been per demo and the gate example was never one of its subjects.
# What still runs is `worker-gate.mjs`, which refuses an artifact with no shared
# `env.memory` and then *uses* every symbol the link arguments below ask for —
# so `--gate-only` narrows the surface checked, not the artifact under it.
if [ "$GATE_ONLY" = "1" ] && [ "$THREADS" = "0" ]; then
  echo "crcbl web build: --gate-only only makes sense with --threads" >&2
  exit 2
fi

# Every wasm-ready sample: crate name, lib name, and where it goes in the site.
# One row per demo — a new sample is a line here and a directory under
# `web/demos/`.
DEMOS=(
  "breakout:crcbl_breakout:demos/breakout"
  "flappy:crcbl_flappy:demos/flappy"
  "asteroids:crcbl_asteroids:demos/asteroids"
  "horde:crcbl_horde:demos/horde"
  "hud:crcbl_hud:demos/hud"
  "lantern:crcbl_lantern:demos/lantern"
  "quarry:crcbl_quarry:demos/quarry"
  "viewer:crcbl_viewer:demos/viewer"
)

profile_flag=()
[ "$PROFILE" = "release" ] && profile_flag=(--release)

# THE THREADED ARTIFACT IS A SEPARATE BUILD, AND DELIBERATELY NOT ON THE SITE.
#
# `--threads` builds the same demo crates a second way: atomic instructions, a
# shared memory the *host* constructs, and the TLS and stack symbols a worker
# needs to bring itself up. It writes to `$THREADED_DIR` rather than
# `target/$TARGET/`, so the two artifacts coexist; one directory holding both
# would rebuild everything on every alternation, because the flags differ.
#
# NOTHING THIS MODE PRODUCES IS PUBLISHED. It writes a site of its own to
# `$THREADED_SITE`, and the Pages workflow builds `$SITE` and uploads exactly
# that directory — so the two never meet. The artifact imports `env.memory`, and
# neither of the two things that instantiate a *published* artifact can satisfy
# that: `web/tools/wasm-loader.js` passes an empty import object on purpose, and
# `web/tools/smoke.mjs` synthesises a stub memory of a single page. Each is a
# `LinkError` on a threaded module rather than a threaded demo, which is why
# neither runs below. What replaces them is `web/tools/wasm-loader-threads.js`,
# which constructs the shared memory the module wants, and
# `web/tools/check-exports.mjs --threads`, which gates the surface symbol by
# symbol.
#
# THE SITE IT ASSEMBLES IS THE DEMO SITE, not a second copy of it: the same
# pages, the same `web/engine/`, the same `web/demos/<name>/main.js`. Only the
# artifact beside each demo and the `<lib>.js` that instantiates it differ, which
# is what makes `web/run-horde-threads-e2e.sh` a gate on the demo path rather
# than on a page written to pass it.
#
# IT ALSO RUNS THE BACKEND. `crcbl-jobs`'s `Workers` spawner queues each spawn
# for a host to drain; `crates/crcbl-jobs/examples/web_worker_gate.rs` is a
# `cdylib` that exercises it, and `web/tools/worker-gate.mjs` brings real
# `node:worker_threads` workers up through the ABI and asserts they run Rust on
# stacks and thread-locals of their own.
#
# `web/run-jobs-e2e.sh` does the same thing in a real browser, off `--gate-only`
# below, and is the only gate for the parts node cannot reach: a browser
# `Worker` taking a structured-cloned module, a shared memory the *document*
# has to earn, and a page's main thread driving a pool whose workers park on
# `memory.atomic.wait32`.
#
# IT IS NOT PART OF THE PAGES BUILD AND CANNOT BECOME ONE. GitHub Pages sends
# no COOP/COEP pair, so a `SharedArrayBuffer` cannot exist on the published
# site and a threaded artifact could never be instantiated there.
# `web/tools/serve.mjs` does send both, which is what makes this testable
# locally at all. See `docs/backlog.md`.
if [ "$THREADS" = "1" ]; then
  if [ "$SERVE" = "1" ] && [ "$GATE_ONLY" = "1" ]; then
    echo "crcbl web build: --gate-only builds one artifact; there is no site to --serve" >&2
    exit 2
  fi

  # Without these, both failures arrive as an opaque cargo error: a toolchain
  # rustup does not have, or a `-Z build-std` that cannot find std's sources and
  # never names the component that would fix it.
  if ! command -v rustup >/dev/null 2>&1; then
    echo "crcbl web build: rustup not found; --threads needs the $NIGHTLY toolchain" >&2
    exit 1
  fi
  if ! rustup toolchain list | grep -q "^$NIGHTLY"; then
    echo "crcbl web build: toolchain $NIGHTLY is not installed" >&2
    echo "    rustup toolchain install $NIGHTLY --component rust-src" >&2
    exit 1
  fi
  if ! rustup component list --toolchain "$NIGHTLY" --installed | grep -qx rust-src; then
    echo "crcbl web build: $NIGHTLY has no rust-src component" >&2
    echo "    -Z build-std recompiles std with +atomics and needs its sources" >&2
    echo "    rustup component add rust-src --toolchain $NIGHTLY" >&2
    exit 1
  fi

  # Every flag here is asserted against the built artifact by
  # `check-exports.mjs --threads`, which names the one that is missing:
  #
  #   +atomics,+bulk-memory  the atomic instructions and `memory.init`
  #   +mutable-globals       a `__stack_pointer` JS is allowed to write
  #   --shared-memory        the memory is shared; without it a worker gets its
  #                          own copy rather than the module's heap
  #   --import-memory        the host constructs that memory and the module
  #                          imports it. A module that *owns* its memory cannot
  #                          hand it to a worker at all, and that is what a
  #                          build with only the target features produces.
  #   --max-memory           a shared memory must declare a maximum
  #   --export=__wasm_init_tls, __tls_base, __tls_size, __tls_align
  #                          a worker sets its own TLS block up before it runs
  #                          any Rust. Skipping the call is **silent** as often
  #                          as not: `__tls_base`'s initial value is a layout
  #                          accident of the module, and where it starts at zero
  #                          every worker's thread-locals simply alias one
  #                          address and are read and written without a trap
  #   --export=__stack_pointer  wasm globals are per-instance, so a worker that
  #                          does not set this one runs on the main thread's
  #                          stack region — and only code that *uses* its stack
  #                          can tell the difference
  threaded_rustflags=(
    "-C target-feature=+atomics,+bulk-memory,+mutable-globals"
    "-C link-arg=--shared-memory"
    "-C link-arg=--import-memory"
    "-C link-arg=--max-memory=$THREADED_MAX_MEMORY_BYTES"
    "-C link-arg=--export=__wasm_init_tls"
    "-C link-arg=--export=__tls_base"
    "-C link-arg=--export=__tls_size"
    "-C link-arg=--export=__tls_align"
    "-C link-arg=--export=__stack_pointer"
  )

  # Whatever the caller already set is kept in front of ours rather than
  # replaced: CI puts `-D warnings` there, and overwriting it took the warning
  # gate off the one build that compiles the atomics path — the half no other
  # job compiles at all. Joined into one string because that is what `RUSTFLAGS`
  # is; a `[target]` table cannot be used here for the reason below.
  THREADED_RUSTFLAGS="${RUSTFLAGS:+${RUSTFLAGS} }${threaded_rustflags[*]}"

  echo "==> threaded build: $NIGHTLY, -Z build-std=std,panic_abort, into $THREADED_DIR"
  for row in "${DEMOS[@]}"; do
    if [ "$GATE_ONLY" = "1" ]; then break; fi
    IFS=: read -r crate lib _ <<<"$row"
    echo "==> cargo +$NIGHTLY build --lib -p $crate --target $TARGET ($PROFILE, threaded)"
    # `RUSTFLAGS` rather than a `[target]` table: it has to apply to the std
    # units `-Z build-std` compiles too, which is the whole reason std is being
    # rebuilt.
    (cd "$REPO" && RUSTFLAGS="$THREADED_RUSTFLAGS" cargo "+$NIGHTLY" build \
      --locked --lib -p "$crate" --target "$TARGET" "${profile_flag[@]}" \
      --target-dir "$THREADED_DIR" -Z build-std=std,panic_abort)

    echo "==> checking the worker-capable surface"
    node "$REPO/web/tools/check-exports.mjs" \
      "$THREADED_DIR/$TARGET/$PROFILE/$lib.wasm" --sample "$crate" --threads --quiet
  done

  # The worker backend's own gate. An example rather than a demo crate: its
  # exports exist only to be observed, and an example cannot reach a site
  # artifact by any route. See the crate docs on `crcbl_jobs::workers`.
  echo "==> cargo +$NIGHTLY build --example web_worker_gate -p crcbl-jobs ($PROFILE, threaded)"
  (cd "$REPO" && RUSTFLAGS="$THREADED_RUSTFLAGS" cargo "+$NIGHTLY" build \
    --locked --example web_worker_gate -p crcbl-jobs --target "$TARGET" \
    "${profile_flag[@]}" --target-dir "$THREADED_DIR" -Z build-std=std,panic_abort)

  echo "==> bringing Web Workers up through the spawn ABI"
  node "$REPO/web/tools/worker-gate.mjs" \
    "$THREADED_DIR/$TARGET/$PROFILE/examples/web_worker_gate.wasm" --quiet

  echo "==> $THREADED_DIR/$TARGET/$PROFILE"
  for row in "${DEMOS[@]}"; do
    if [ "$GATE_ONLY" = "1" ]; then break; fi
    IFS=: read -r _ lib _ <<<"$row"
    echo "    $lib.wasm"
  done
  echo "    examples/web_worker_gate.wasm"

  if [ "$GATE_ONLY" = "0" ]; then
    assemble_static "$THREADED_SITE"
    for row in "${DEMOS[@]}"; do
      IFS=: read -r crate lib dest <<<"$row"
      echo "==> publishing $lib.wasm (threaded) to $dest"
      mkdir -p "$THREADED_SITE/$dest"
      cp "$THREADED_DIR/$TARGET/$PROFILE/$lib.wasm" \
        "$THREADED_SITE/$dest/${lib}_bg.wasm"
      # The threaded loader, under the name every page already imports. See its
      # header for why it is a second file and not a branch in the other one.
      cp "$REPO/web/tools/wasm-loader-threads.js" "$THREADED_SITE/$dest/$lib.js"
    done
    echo "==> $THREADED_SITE"
    find "$THREADED_SITE" -type f | sed "s|$THREADED_SITE/|    |" | sort

    if [ "$SERVE" = "1" ]; then
      exec node "$REPO/web/tools/serve.mjs" "$THREADED_SITE" --port "${PORT:-8000}"
    fi
  fi
  exit 0
fi

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

assemble_static "$SITE"

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

if [ "$SERVE" = "1" ]; then
  exec node "$REPO/web/tools/serve.mjs" "$SITE" --port "${PORT:-8000}"
fi
