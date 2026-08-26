#!/usr/bin/env bash
# The WebGPU parity gate: drive crcbl's backend-agnostic golden `Scene` set
# through the `crcbl-webgpu` browser backend, offscreen, read every frame back,
# and compare it against the very golden image the native suites compare
# against. It is the only check that can say whether a browser backend draws the
# same picture the `vk`/`mtl`/`dx12` render-e2e suites do; those cannot answer
# it, because there is no browser in them.
#
#   ./web/run-render-harness-e2e.sh [--expect-fail ssr,ui]
#
# THREE PARTS, RUN BACK TO BACK.
#
#   1. `web/tools/render-harness-e2e.mjs` builds nothing and decides nothing: it
#      loads the harness page in headless Chromium under SwiftShader, drives
#      every scene, writes each readback to `$SITE/readback` as
#      `<scene>.<width>x<height>.<order>.bin`, and writes its per-scene outcome
#      to `$SITE/driver-result.json`.
#   2. `cargo run -p render-harness --example compare-readback` compares those
#      against `crates/crcbl/tests/golden/<scene>.png` with `crcbl-golden`, at
#      `Tolerance::RASTERISER` — the same comparator and the same numbers the
#      native golden tests use. Nothing in JS diffs a pixel.
#   3. `web/tools/render-harness-verdict.mjs` joins the two into one answer per
#      scene and is the only thing that decides the exit code.
#
# Each of the first two knows half of what a scene's outcome is — whether the
# browser backend got it through, and whether the pixels it produced are the
# golden's — and neither exit code names a scene. That is why the verdict is its
# own step: `--expect-fail` is per scene, and neither half can apply it.
#
# WHAT A RASTERISER CANNOT SERVE. `--expect-fail ssr,ui` says those scenes
# produce no verdict here. They still render, still compare and still print;
# only the verdict changes — and it changes in BOTH directions, because a named
# scene that *matches* fails the run as a stale list, and so does a name that is
# not a scene. The guards survive it untouched: a run where nothing was compared
# fails, and so does one where every passing scene was on the list, so the list
# cannot empty a run and call it a pass. `web/tools/render-harness-verdict.mjs`
# carries the rest.
#
# THIS IS NOT A TOLERANCE KNOB, and must not become one. The comparator runs at
# `Tolerance::RASTERISER` exactly as the native suites do; an excused scene is a
# scene whose *whole* comparison is set aside on this platform, named in the log
# every run, and failing the moment it starts passing. Widening the tolerance
# instead would quietly weaken every one of them.
#
# ENVIRONMENT
#   SITE_DIR                          Where the harness site is assembled.
#   PROFILE                           `release` (default) or `debug`.
#   CRCBL_RENDER_HARNESS_EXPECT_FAIL  Scene names that produce no verdict on
#                                     this platform, comma- or space-separated;
#                                     `--expect-fail ssr,ui` is the same switch.
#   CRCBL_CHROMIUM                    Path to the browser binary.
#
# It builds `apps/render-harness` to wasm plainly. There is no backend flag any
# more: `crcbl-wgpu` is a `cfg(not(target_arch = "wasm32"))` dependency of the
# umbrella, so `crcbl-webgpu` is the only GPU backend a wasm build links and
# `crcbl::backend` auto-selects it because the target says so.
#
# WHAT IT NEEDS
#   * Node 22+ (for the global WebSocket the DevTools client uses).
#   * A Chromium/Chrome with WebGPU. CRCBL_CHROMIUM pins one; otherwise the usual
#     four names are tried. No Xvfb is needed: the harness reads an offscreen
#     target back into wasm memory, so there is no canvas to snapshot.
#
# EXIT CODES
#   0  every scene rendered AND matched its golden, but for the excused ones.
#   1  the harness ran, but a scene did not render or did not match, or an
#      excused scene has started passing, or nothing was gated. The three tables
#      are on stdout and name every one.
#   2  it could not run at all — no browser, no adapter, the wasm failed to load,
#      the comparator could not be built, or its output no longer parses.
#
# WHERE IT RUNS IN CI: `.github/workflows/pages.yml`'s `render-harness` job, on
# Linux, with the expected-fail list the comment there records the measurement
# for.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SITE="${SITE_DIR:-$REPO/target/render-harness-site}"
PROFILE="${PROFILE:-release}"
TARGET=wasm32-unknown-unknown
CRATE=render-harness
LIB=crcbl_render_harness
EXPECT_FAIL="${CRCBL_RENDER_HARNESS_EXPECT_FAIL:-}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --expect-fail)
      [ "$#" -ge 2 ] || {
        echo "run-render-harness-e2e.sh: --expect-fail needs a scene list" >&2
        exit 2
      }
      EXPECT_FAIL="$2"
      shift 2
      ;;
    *)
      echo "run-render-harness-e2e.sh: unknown argument $1" >&2
      exit 2
      ;;
  esac
done

profile_flag=()
[ "$PROFILE" = "release" ] && profile_flag=(--release)

echo "==> cargo build --lib -p $CRATE --target $TARGET ($PROFILE, webgpu)"
# No feature flag: on wasm32 `crcbl-webgpu` is the umbrella's only GPU backend
# and therefore the one `crcbl::backend` opens — which is the backend this gate
# exists to exercise.
(cd "$REPO" && cargo build --locked --lib -p "$CRATE" --target "$TARGET" "${profile_flag[@]}")

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

# The two filenames `web/harness/main.js` imports, laid out exactly as
# `web/build.sh` lays a demo out: the artifact unmodified as `<lib>_bg.wasm`,
# and `web/tools/wasm-loader.js` as the `<lib>.js` whose default export
# instantiates it. There is no `wasm-bindgen` step here any more — see that
# file, and `web/build.sh`'s "no wasm-bindgen" note, for why the tool cannot
# run over an artifact that imports nothing.
echo "==> publishing $LIB.wasm"
cp "$REPO/target/$TARGET/$PROFILE/$LIB.wasm" "$SITE/harness/${LIB}_bg.wasm"
cp "$REPO/web/tools/wasm-loader.js" "$SITE/harness/$LIB.js"

# Inside $SITE, which was just removed and rebuilt, so it starts empty every run
# — a stale readback from a previous run is a comparison against the wrong frame,
# and the comparator refuses two files for one scene rather than picking.
READBACK="$SITE/readback"
mkdir -p "$READBACK"

# The two files the verdict step reads, in $SITE for the same reason: a result or
# a comparator log left over from an earlier run would be read as this one's.
RESULT_JSON="$SITE/driver-result.json"
COMPARE_LOG="$SITE/compare-readback.log"

# Said before anything runs as well as after it. A reader who stops at the top of
# a green log has to be able to see what this run's verdict does not cover.
if [ -n "$EXPECT_FAIL" ]; then
  echo "==> --expect-fail $EXPECT_FAIL — those scenes still render and still compare;"
  echo "    a match from any of them fails this run as a stale list"
fi

echo "==> driving the golden scenes in the browser"
driver=0
node "$REPO/web/tools/render-harness-e2e.mjs" "$SITE" \
  --readback-dir "$READBACK" --result-json "$RESULT_JSON" || driver=$?
# A driver that could not run at all leaves nothing to compare, so there is no
# point building the comparator to tell us the directory is empty.
if [ "$driver" -eq 2 ]; then
  exit 2
fi

echo "==> comparing each readback against crates/crcbl/tests/golden"
compare=0
# Redirected to a file and echoed back rather than piped, for two reasons. The
# verdict step reads the per-scene table out of it, and `cargo … | tee` reports
# the pipeline's status rather than cargo's, which is how a false green gets into
# a shell script. `cargo`'s own progress and the comparator's per-mismatch lines
# go to stderr and are still live on the terminal.
(cd "$REPO" && cargo run --locked -p "$CRATE" --example compare-readback -- "$READBACK") \
  > "$COMPARE_LOG" || compare=$?
cat "$COMPARE_LOG"
# 0 matched, 1 did not, 2 bad usage or unreadable input — anything else is cargo
# failing to build or run it at all, which is a gate that did not run rather than
# a gate that passed.
if [ "$compare" -gt 1 ]; then
  echo "run-render-harness-e2e.sh: the comparator exited $compare — nothing was compared" >&2
  exit 2
fi

# THE ONLY STEP THAT DECIDES. The two exit codes above are handed in so the
# verdict's read of the table can be held against what the tools actually
# returned; it exits 2 rather than reaching a verdict if they disagree.
echo "==> the verdict, scene by scene"
verdict_args=(
  --driver-json "$RESULT_JSON"
  --compare-log "$COMPARE_LOG"
  --driver-exit "$driver"
  --compare-exit "$compare"
)
if [ -n "$EXPECT_FAIL" ]; then
  verdict_args+=(--expect-fail "$EXPECT_FAIL")
fi
status=0
node "$REPO/web/tools/render-harness-verdict.mjs" "${verdict_args[@]}" || status=$?
if [ "$status" -ne 0 ]; then
  echo "run-render-harness-e2e.sh: FAIL — driver exit $driver, comparator exit $compare, verdict exit $status" >&2
  exit "$status"
fi
