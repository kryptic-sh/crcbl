#!/usr/bin/env bash
# Drive the Web Worker spawn backend from a real page in a real browser, and
# prove Rust ran on a second thread.
#
#   ./web/run-jobs-e2e.sh [--no-build]
#
# The same shape as `web/run-render-harness-e2e.sh`: it builds what it drives,
# assembles its own site, says what it checked, and fails when nothing was
# checked. It needs no Xvfb and no GPU — there is no canvas on the page — so
# headless Chromium is the whole environment.
#
# WHAT THIS IS THE ONLY GATE FOR
#   `web/tools/check-exports.mjs --threads` proves the *symbols* a worker brings
#   itself up on exist in the artifact. `web/tools/worker-gate.mjs` proves the
#   sequence built on them works under `node:worker_threads`. Neither is a
#   browser, and four of the claims the worker backend rests on are only
#   answerable in one:
#
#     * a browser `Worker` accepts a structured-cloned `WebAssembly.Module` and
#       a shared `WebAssembly.Memory` — node clones its own;
#     * that memory can be constructed at all, which is a property of the
#       *document* (`crossOriginIsolated`) rather than of the build, and is why
#       nothing threaded is publishable;
#     * a page's **main thread** survives driving a pool whose workers park on
#       `memory.atomic.wait32`. Node lets its main thread block and a browser
#       traps instead; `crates/crcbl-jobs/src/workers.rs` says in as many words
#       that no gate there can show it;
#     * the artifact a page can actually be given is refused workers.
#       `__crcbl_web_jobs_host_ready` is the whole of `Spawn::threaded`'s
#       answer, so announcing for an artifact no worker could attach to is the
#       one failure that makes the backend lie rather than degrade. The page
#       loads a *non-threaded* build of the same example — the shape every
#       published artifact has — and asserts the host refuses it.
#
# THE FOUR RED CHECKS ARE PART OF THE GATE, NOT A DEBUGGING AID.
#   Three of the page's assertions guard steps whose failure is *silent*, and
#   one guards a lie. Each is run again with that step deliberately left out,
#   and this script insists the right assertion went red — and that the others
#   did not, because "something failed" is not evidence that the assertion under
#   test is the thing that noticed.
#
#   The measurement that shaped this: skipping `__wasm_init_tls` does **not**
#   trap here. In the artifact this drives, `__tls_base` is left at zero, every
#   worker's thread-locals alias one address, and a `const`-initialised
#   `thread_local!` reads and writes it without complaint. So the assertion that
#   catches it is `gate_tls_shared` — a thread finding a frame address in its own
#   thread-local that its own stack could not have produced — and a gate that
#   waited for an exception would have passed the broken build.
#
# WHAT IT NEEDS
#   * **A Chromium or Chrome.** `CRCBL_CHROMIUM` pins one; otherwise the usual
#     four names are tried. No WebGPU is used.
#   * **Node 22 or newer**, for the global `WebSocket` the DevTools client uses.
#   * **The `--threads` toolchain**, unless `--no-build`: `web/build.sh` names
#     the nightly and the `rust-src` component, and fails saying which is
#     missing.
#
# ENVIRONMENT
#   SITE_DIR   Where the gate's site is assembled. Default `target/jobs-site`.
#   PROFILE    `release` (default) or `debug`.
#   CRCBL_CHROMIUM, CRCBL_CHROMIUM_FLAGS, CRCBL_CHROMIUM_NO_SANDBOX
#              As the other browser gates here; see `web/tools/browser-launch.mjs`.
#
# EXIT CODES
#   0  the green run passed every check, and all four red checks broke the
#      assertions they are supposed to break.
#   1  a check failed, or a red check did not go red where it should have.
#   2  it could not run at all — no browser, no node, nothing built.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SITE="${SITE_DIR:-$REPO/target/jobs-site}"
PROFILE="${PROFILE:-release}"
TARGET=wasm32-unknown-unknown
THREADED_DIR="${THREADED_TARGET_DIR:-$REPO/target/wasm-threaded}"
BUILD=1

while [ "$#" -gt 0 ]; do
    case "$1" in
        --no-build)
            BUILD=0
            shift
            ;;
        *)
            echo "run-jobs-e2e.sh: unknown argument $1" >&2
            echo "usage: ./web/run-jobs-e2e.sh [--no-build]" >&2
            exit 2
            ;;
    esac
done

if ! command -v node >/dev/null 2>&1; then
    echo "crcbl jobs e2e: node not found; this harness needs Node 22 or newer" >&2
    exit 2
fi
NODE_MAJOR="$(node --version | sed -E 's/^v([0-9]+).*/\1/')"
if [ "$NODE_MAJOR" -lt 22 ]; then
    echo "crcbl jobs e2e: node $(node --version) is too old; the DevTools client needs the global WebSocket from Node 22" >&2
    exit 2
fi

profile_flag=()
[ "$PROFILE" = "release" ] && profile_flag=(--release)

THREADED_WASM="$THREADED_DIR/$TARGET/$PROFILE/examples/web_worker_gate.wasm"
PLAIN_WASM="$REPO/target/$TARGET/$PROFILE/examples/web_worker_gate.wasm"

if [ "$BUILD" = "1" ]; then
    # `--gate-only` so a browser run does not pay for seven `-Z build-std` demo
    # builds it never loads. Every threaded link argument, and the toolchain
    # preflight that names the missing component, stay in `web/build.sh`: this
    # script has no copy of them to drift from.
    echo "==> building the worker-capable gate artifact"
    "$REPO/web/build.sh" --threads --gate-only

    # THE NEGATIVE CONTROL, AND IT IS A PLAIN BUILD ON PURPOSE. The same example
    # with no atomics, no shared memory and no imports — which is the shape every
    # artifact on the demo site has. There are no flags here worth sharing with
    # the threaded build; the absence of them is the whole point.
    echo "==> cargo build --example web_worker_gate -p crcbl-jobs ($PROFILE, plain)"
    (cd "$REPO" && cargo build --locked --example web_worker_gate -p crcbl-jobs \
        --target "$TARGET" "${profile_flag[@]}")
fi

for wasm in "$THREADED_WASM" "$PLAIN_WASM"; do
    if [ ! -f "$wasm" ]; then
        echo "crcbl jobs e2e: $wasm is missing; re-run without --no-build" >&2
        exit 2
    fi
done

# The site is assembled here rather than by `web/build.sh` because nothing in it
# may ever reach the demo site: the page loads an artifact that imports a shared
# `env.memory`, and GitHub Pages sends no COOP/COEP pair, so on the published
# origin that memory cannot exist and the page could only fail. `web/build.sh`
# prunes `web/jobs` from its copy for that reason.
echo "==> assembling $SITE"
rm -rf "$SITE"
mkdir -p "$SITE/jobs" "$SITE/engine"
for file in index.html main.js; do
    cp "$REPO/web/jobs/$file" "$SITE/jobs/$file"
done
# The host half of the spawn ABI, the worker bring-up it starts, and the
# import-section decoder it reads limits with — at the paths `web/jobs/main.js`
# imports them by. They live under `web/engine/` because a demo's threaded
# loader needs the same three, and one copy of an announce that must not be made
# on faith is the whole point; whether a memory import is *shared* is not in
# `WebAssembly.Module.imports()`, so that decoder is the only thing anywhere that
# can answer it, and `check-exports.mjs` and `worker-gate.mjs` share it too.
for file in jobs.js jobs-worker.js wasm-memory.js; do
    cp "$REPO/web/engine/$file" "$SITE/engine/$file"
done
# `_bg.wasm`, the same suffix `web/build.sh` publishes a demo artifact under, so
# the two layouts read the same way.
cp "$THREADED_WASM" "$SITE/jobs/web_worker_gate_bg.wasm"
cp "$PLAIN_WASM" "$SITE/jobs/web_worker_gate_plain_bg.wasm"

RUNTIME_DIR="$(mktemp -d -t crcbl-jobs-e2e.XXXXXX)"
chmod 700 "$RUNTIME_DIR"
cleanup() {
    status=$?
    rm -rf "$RUNTIME_DIR"
    exit "$status"
}
trap cleanup EXIT INT TERM

GREEN="$RUNTIME_DIR/green.log"

echo "==> driving the page in a browser"
set +e
node "$REPO/web/tools/jobs-e2e.mjs" "$SITE" 2>&1 | tee "$GREEN"
STATUS=${PIPESTATUS[0]}
set -e

# CI sets `CARGO_TERM_COLOR: always`, and a coloured pipeline has broken this
# repository's count guards before. This harness does not colour its own output,
# but a browser or a node warning might. `$'\033'` and not `\x1b`: `\x` is a GNU
# sed extension and BSD sed reads that pattern as a literal `x1b[…`, matching
# nothing, silently. Same line, for the same reason, as `web/run-browser-e2e.sh`.
sed -E $'s/\033\\[[0-9;]*[a-zA-Z]//g' "$GREEN" >"${GREEN}.plain"
GREEN="${GREEN}.plain"

if [ "$STATUS" -eq 2 ]; then
    echo "crcbl jobs e2e: the gate could not run" >&2
    exit 2
fi

# The guard every harness here carries: a run that checked nothing must not be
# able to report success. The driver exits non-zero on its own in that case; this
# is the second lock, because the failure being guarded against is precisely
# "the thing that was supposed to notice did not".
RAN="$(grep -Eo '[0-9]+/[0-9]+ checks passed' "$GREEN" | tail -1 | grep -Eo '/[0-9]+' | tr -d '/' || true)"
if [ -z "$RAN" ] || [ "$RAN" -eq 0 ]; then
    echo "crcbl jobs e2e: the driver reported no checks — the gate is not gating" >&2
    exit 1
fi

# Three assertions by name as well as by count, each because its absence would
# shrink the gate without failing anything. Renaming one in `web/jobs/main.js` is
# meant to fail here and be renamed here too.
#
#   isolation   every other check would still run, and most would still pass, on
#               an origin with no COOP/COEP — they would simply fail later and
#               for a reason that says nothing about the headers.
#   the thread  this is the exit criterion. Without it the run proves a `Worker`
#               was constructed, which any page can do.
#   the TLS     the one assertion that catches a missing `__wasm_init_tls` here,
#               because that failure does not trap. See the header.
for named in \
    'the document is cross-origin isolated' \
    'a chunk ran on a thread that is not the driver' \
    "no thread found another thread's value in its own thread-local"; do
    if ! grep -qF "$named" "$GREEN"; then
        echo "crcbl jobs e2e: the driver never checked '$named';" >&2
        echo "               the gate is smaller than it reports" >&2
        exit 1
    fi
done

if [ "$STATUS" -ne 0 ]; then
    echo "crcbl jobs e2e: $RAN checks ran and at least one failed" >&2
    exit "$STATUS"
fi

# ---------------------------------------------------------------------------
# The red checks
# ---------------------------------------------------------------------------

red_log=""
red_label=""

# Assert that a red run turned one named check red. The name is matched exactly:
# a check whose text moved on has to be updated here, which is the point.
expect_fail() {
    if ! grep -qF "FAIL $1" "$red_log"; then
        echo "crcbl jobs e2e: $red_label did not turn this check red:" >&2
        echo "                 $1" >&2
        echo "               so that assertion is not gating what it claims to" >&2
        cat "$red_log" >&2
        exit 1
    fi
    echo "    broke:      $1"
}

# And that it left another one alone. Without this pair a single check that
# failed for any reason at all would satisfy every red run, and the four of them
# would stop being four.
expect_pass() {
    if ! grep -qF "ok   $1" "$red_log"; then
        echo "crcbl jobs e2e: $red_label was expected to leave this check alone:" >&2
        echo "                 $1" >&2
        echo "               it did not, so the red runs are not distinguishable" >&2
        cat "$red_log" >&2
        exit 1
    fi
    echo "    left alone: $1"
}

# Assert that a red run turned **at least one** of several named checks red, and
# that every one of them ran.
#
# For a sabotage whose damage has more than one place to surface. Corrupting the
# main thread's stack is seen as the chunk's own array changing underneath it,
# as a checksum that does not reproduce, or as an outright trap — the same
# defect from three vantage points, and which one arrives first depends on where
# the corruption lands rather than on the sabotage. Demanding a particular one
# makes the gate depend on the module's layout, which is how CI met
# `memory access out of bounds` on 2026-08-23 and failed over a check that had
# stayed green only because the trap got there first.
#
# Requiring that all of them *ran* is what keeps this from being satisfied by
# nothing: a check that stopped being printed would otherwise slip through.
expect_any_fail() {
    local reached=0 failed=0 name
    for name in "$@"; do
        if grep -qF "ok   $name" "$red_log" || grep -qF "FAIL $name" "$red_log"; then
            reached=$((reached + 1))
        else
            echo "crcbl jobs e2e: $red_label never reached this check at all:" >&2
            echo "                 $name" >&2
            echo "               so nothing here is a verdict about it" >&2
            cat "$red_log" >&2
            exit 1
        fi
        if grep -qF "FAIL $name" "$red_log"; then
            failed=$((failed + 1))
            echo "    broke:      $name"
        fi
    done
    if [ "$failed" -eq 0 ]; then
        echo "crcbl jobs e2e: $red_label turned none of these $reached check(s) red:" >&2
        for name in "$@"; do
            echo "                 $name" >&2
        done
        echo "               so the sabotage was not observed anywhere and those" >&2
        echo "               assertions are not gating what they claim to" >&2
        cat "$red_log" >&2
        exit 1
    fi
}

# And that a check the red run does not decide **ran at all**.
#
# A trap is a consequence of the corruption rather than the observation, and
# whether one happens depends on where the corruption lands — which is a
# property of the module's layout, not of the sabotage. So neither direction is
# demanded, and what is demanded is that the check was reached: a check that
# stopped being printed would otherwise satisfy nothing and be noticed by
# nobody.
expect_either() {
    if ! grep -qF "ok   $1" "$red_log" && ! grep -qF "FAIL $1" "$red_log"; then
        echo "crcbl jobs e2e: $red_label never reached this check at all:" >&2
        echo "                 $1" >&2
        echo "               so nothing here is a verdict about it" >&2
        cat "$red_log" >&2
        exit 1
    fi
    if grep -qF "FAIL $1" "$red_log"; then
        echo "    also red:   $1 (a consequence, not the observation)"
    else
        echo "    left alone: $1"
    fi
}

red_run() {
    red_label="--query $2"
    red_log="$RUNTIME_DIR/red-$1.log"
    echo "==> red check: $red_label"
    local status=0
    node "$REPO/web/tools/jobs-e2e.mjs" "$SITE" --query "$2" >"$red_log" 2>&1 || status=$?
    if [ "$status" -eq 0 ]; then
        echo "crcbl jobs e2e: the run with $red_label PASSED." >&2
        echo "               A gate whose assertions cannot be made to fail is not a gate." >&2
        cat "$red_log" >&2
        exit 1
    fi
    if [ "$status" -ne 1 ]; then
        echo "crcbl jobs e2e: the run with $red_label exited $status rather than failing its checks;" >&2
        echo "               it did not reach the assertions this is testing" >&2
        cat "$red_log" >&2
        exit 1
    fi
}

# A worker that never writes `__stack_pointer` runs on the main thread's stack.
# The two threads then write over each other's frames, and the damage surfaces
# in whichever of three places reaches it first: the chunk's own stack array
# changing underneath it, a checksum that does not reproduce, or a trap — the
# driver's frames go with the rest, so the run often does not survive to make
# the finer observation at all.
#
# **Which one it is depends on the module's layout, not on the sabotage.** On
# 2026-08-23 CI trapped after one `par_for` call where this machine completes
# four, and the stack-array check passed — green because the run died before it
# could be wrong, over a build whose only change was in unrelated crates. So
# what is demanded is that the corruption was seen *somewhere*, and that the
# thread-local check is not where, which is what keeps this arm distinct from
# the one below.
red_run stack-pointer no-stack-pointer
expect_any_fail \
    'no chunk found its stack array changed underneath it' \
    'no run trapped'
expect_pass "no thread found another thread's value in its own thread-local"

# A worker that never calls `__wasm_init_tls` does not clobber a stack —
# `__tls_base` is left at zero and every worker's thread-locals alias one
# address — so the separation has to be observed directly, which is what the
# aliasing check does.
#
# **Whether it also traps is not this arm's business.** It used to be asserted
# as "does NOT trap here", and on 2026-08-23 CI met `memory access out of
# bounds` on that run and failed a gate about thread-locals over it. Writing
# through a `__tls_base` of zero lands wherever the module's low addresses
# happen to be, so the answer moves when anything linked into the artifact
# changes size — the same reason the arm above reports its trap rather than
# requiring it.
red_run init-tls no-init-tls
expect_fail "no thread found another thread's value in its own thread-local"
expect_pass 'no chunk found its stack array changed underneath it'
expect_either 'no run trapped'

# No announcement, no threads: the backend refuses every spawn, the queue stays
# empty, and no worker exists to run a chunk. The whole run degrades onto the
# inline path, which is the behaviour the published site actually gets.
red_run host-ready no-host-ready
expect_fail 'a chunk ran on a thread that is not the driver'
expect_fail 'host_ready answers the worker count it recorded'
expect_pass 'the single-threaded reference run does not clobber its own stack'

# And the lie, announced deliberately for an artifact no worker can attach to.
# `Spawn::threaded` answers true, `Pool::with_workers` hands out workers that can
# never arrive, and the spawn queue fills with requests nothing will drain.
red_run force-host-ready force-host-ready
expect_fail 'threaded() stays false for an artifact no worker could attach to'
expect_fail 'a pool on it gets zero workers rather than workers that never arrive'
expect_pass 'the document is cross-origin isolated'

echo "crcbl jobs e2e: $RAN checks ran in a real browser, and four red checks broke"
echo "crcbl jobs e2e: the assertions they are each supposed to break"
echo "crcbl jobs e2e: a Web Worker brought up through crcbl_jobs::workers' ABI ran"
echo "crcbl jobs e2e: Rust on a stack and a thread-local of its own"
