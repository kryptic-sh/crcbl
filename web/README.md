# `web/` — the JS shim and the demo site

The browser half of P5. Everything here is hand-written ES modules loaded
directly by the page: **no framework, no bundler, no npm, no `node_modules`.**
That is the same policy `docs/plan/15-windowing.md` sets for the engine's
platform bindings — thin bindings to an ABI the platform forces on us, never a
framework that owns policy — and it applies here for the same reason.

## Layout

| Path                           | What it is                                                                 |
| ------------------------------ | -------------------------------------------------------------------------- |
| `pages/*.html`                 | one content file per page: its metadata, its prose, its includes           |
| `templates/layout.html`        | the chrome every page is rendered into                                     |
| `templates/demo-*.html`        | the blocks every demo shares — the window, the loop's keys, the log note   |
| `tools/build-pages.mjs`        | fills the layout from `pages/`, expands the includes                       |
| `style.css`                    | one stylesheet for the site                                                |
| `favicon.svg`                  | the site icon, declared by the layout                                      |
| `engine/demo.js`               | the boot sequence and the frame loop, shared by every demo                 |
| `engine/wasm.js`               | reading/writing wasm memory, and the detached-view rule                    |
| `engine/shell.js`              | canvas, DPI/resize, focus, fullscreen, keyboard, pointer → `__crcbl_web_*` |
| `engine/storage.js`            | asset pre-load over `fetch()`, OPFS restore and drain                      |
| `engine/audio.js`              | main-thread half of the AudioWorklet feed                                  |
| `engine/audio-worklet.js`      | the `AudioWorkletProcessor` itself                                         |
| `engine/log.js`                | drains the engine's log queue into the console                             |
| `demos/<name>/main.js`         | that sample's `__crcbl_<name>_*` symbols and its two status strings        |
| `engine/jobs.js`               | the host half of `crcbl_jobs::workers` — announce, drain, start workers    |
| `engine/jobs-worker.js`        | the worker half: instantiate, stack, TLS, enter                            |
| `engine/wasm-memory.js`        | decodes whether a module's `env.memory` import is **shared**               |
| `jobs/main.js`                 | the page that drives the spawn ABI, and the checks it publishes            |
| `tools/serve.mjs`              | the static server, and the COOP/COEP pair that buys `SharedArrayBuffer`    |
| `tools/check-exports.mjs`      | the JS↔wasm symbol contract check                                          |
| `tools/smoke.mjs`              | runs the artifact's boot sequence under node                               |
| `tools/wasm-loader.js`         | the `<lib>.js` every page imports as `init`, copied per demo               |
| `tools/wasm-loader-threads.js` | the same, over an artifact that imports a shared memory                    |
| `build.sh`                     | assembles `target/site/`, or `target/site-threaded/` with `--threads`      |

## Adding a demo

Six things, none of them an edit to an existing demo's page:

1. a row in `build.sh`'s `DEMOS` and a line in `tools/build-pages.mjs`'s
   `DEMOS`;
2. `pages/<name>.html` — metadata, the game's own prose, and the three
   `<!--include …-->` directives every demo page carries;
3. `demos/<name>/main.js` — roughly thirty lines binding this sample's
   `__crcbl_<name>_*` exports, plus what to press and what it saves;
4. `demos/<name>/assets/manifest.json`, even if its `keys` are empty;
5. an entry in `tools/browser-e2e.mjs`'s `EXPECTATIONS` — the two assertions
   that are about the _game_ rather than about the browser, which the gate
   refuses to run without;
6. a step in `.github/workflows/pages.yml`, because the gate reads one canvas
   and therefore runs once per demo.

The index page's own card and call-to-action are prose and are not on that list.

The demo window itself is not on it either, which is the point:
`templates/demo-window.html` is the only copy of it, and `tools/build-pages.mjs`
fails the build for a demo page that renders its own instead.

Each `engine/` module implements the JS side of an ABI a Rust module already
specified symbol by symbol. Those specifications are the source of truth:

- `crates/crcbl-shell/src/web/mod.rs` — input and frame
- `crates/crcbl-audio/src/web.rs` — the audio pull
- `crates/crcbl-store/src/web/fetch.rs` — assets
- `crates/crcbl-store/src/web/opfs.rs` — saves
- `apps/breakout/src/web.rs` — boot, frame, teardown, logs

## Build and run

```sh
./web/build.sh --serve      # http://localhost:8000/
```

No tool to install first. `cargo` and `node` are the whole list — see "Why there
is no `wasm-bindgen` here" below for what used to be here, and note that the
page renderer was Python until 2026-08-19, which is one runtime fewer to have
installed.

`file://` will not work: ES modules need an origin, and the Origin Private File
System needs a secure context. `localhost` is one.

`--serve` runs `web/tools/serve.mjs`, which sends
`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp` — the pair that makes the document
cross-origin isolated and therefore allows `SharedArrayBuffer`. The browser gate
imports the same server and asserts the isolation, which is why there is one
server rather than one per caller.

## Which GPU backend the browser renders through

`crcbl-webgpu`, our own wasm→JS→wasm command stream, and **there is nothing to
configure**. It is the only GPU backend a `wasm32` build links — the crate that
once sat beside it, `crcbl-wgpu`, was deleted on 2026-08-21 — so
`crcbl::backend` auto-selects it because the target says so.

```sh
./web/build.sh --serve
./web/run-browser-e2e.sh --build
```

This used to be a `CRCBL_WEB_BACKEND` variable choosing between the two, read by
`build.sh` (which turned it into `--features crcbl/webgpu`),
`run-browser-e2e.sh`, `tools/browser-e2e.mjs` and `run-probe-e2e.sh`. Both the
variable and the umbrella's `webgpu` feature are gone with the choice. An
invocation that still exports `CRCBL_WEB_BACKEND=webgpu` gets what it asked for;
the variable is simply ignored.

The driver still waits for `crcbl-webgpu`'s own `hal: webgpu adapter` line at
boot rather than any adapter line, because that is what makes it a check that
the _right_ backend opened the device. There is one right answer now: a
`hal: wgpu adapter` line from a browser would mean the manifest had regressed.

**Nothing in the Pages workflow builds a `wgpu` site any more, and nothing
could.** It used to, for one step and one reason: the seam gate's probe groups —
G through Z — install their own command-stream channel, a page has exactly one,
and on a WebGpu site the engine's own `WebGpuDevice` has already taken it. Those
groups are `tools/probe-groups.mjs` now and run on `probe/`, a page with **no
engine running** that loads a demo's wasm module without booting it and pumps
the channel itself — the same condition, met without a second backend. See
`run-probe-e2e.sh` below.

## Why there is no `wasm-bindgen` here

The engine's own ABI is **hand-written** `extern "C"` — no `#[wasm_bindgen]`
anywhere in the workspace, and no `wasm-bindgen` in any crate's dependency list.
The tool used to be required anyway, and not by choice: `wgpu` reaches WebGPU
through `web-sys`, `web-sys` _is_ `wasm-bindgen`, and `crcbl-wgpu` was an
unconditional dependency of the umbrella. Every artifact therefore imported 338
functions from `__wbindgen_placeholder__`, which nothing but the `wasm-bindgen`
CLI can resolve; `WebAssembly.instantiateStreaming` on that file was a
`LinkError`, every time.

Target-gating `crcbl-wgpu` off `wasm32` removed the last thing in a browser
build reaching `web-sys`. **The artifacts now import nothing at all** —
`tools/check-exports.mjs` prints the count per demo and fails on any import from
a module outside the loader's own, so a build that grew one back would be a red
step rather than a fact nobody measured. Measured on the site this repository
builds: 338 imports before, 0 after, and 1.57 MB off each `.wasm`.

That also made the tool unusable rather than merely unnecessary: with no
`wasm-bindgen` crate linked its runtime intrinsics are absent, and the CLI exits
with `failed to find intrinsics to enable 'clone_ref' function` instead of
passing the module through. So the one thing it still produced — the `<lib>.js`
whose default export instantiates the module — is `tools/wasm-loader.js` now,
copied beside each artifact under the filename the pages already import. It
preserves the contract exactly: `init()` resolves to the instance's raw exports
object, `memory` included, and everything in `engine/` calls the hand-written
`__crcbl_*` symbols off it directly.

The CLI version is not pinned in a second place — `build.sh` reads it out of
`Cargo.lock` and refuses to run on a mismatch, because a mismatched CLI produces
glue whose imports the module does not have and the failure surfaces in
somebody's browser rather than in the build.

## Checking it in a browser

`run-browser-e2e.sh` is the P5 gate. It builds nothing itself — point it at a
site `build.sh` already produced, or pass `--build`:

```sh
./web/run-browser-e2e.sh --build      # Xvfb + Chromium, then the check list
./web/run-browser-e2e.sh --headless --hardware   # the real GPU, no display
```

Anything past `--build`, `--headless` and `--hardware` goes to
`tools/browser-e2e.mjs`, which takes `--site`, `--demo`, `--adapter`,
`--timeout`, `--out` and `--no-isolation` and **refuses anything else** by name
— a flag borrowed from another gate used to be accepted and silently ignored.

It starts its own Xvfb, serves the site through `web/tools/serve.mjs` — the same
server `build.sh --serve` runs, so the origin the gate checks is the origin a
human loads — opens Chromium over the DevTools protocol, sends a **real** click
and a **real** Space key into the canvas, and reads the canvas back to assert
the frame is neither one flat colour nor identical from frame to frame while the
ball is in flight. It fails if zero checks ran, and separately if the
cross-origin isolation check is not among the ones that did.

**One demo is then driven a second time on the origin GitHub Pages actually
serves.** `serve.mjs` sends COOP and COEP, and Pages sends neither, so every
check above is made against an origin no visitor ever loads. The second run
passes `--no-isolation` — the server withholds the pair, and the driver's group
A asserts `crossOriginIsolated === false`, so a run handed the isolated origin
by mistake fails on its first check — and the demo has to boot, open a device
and draw there anyway. `hud` is the one, because it has no start key and no
per-demo guard block of its own and is still a demo whose clear colour reaches
the canvas; running every demo that way would double CI's matrix to make one
claim about the origin. What that run can prove is narrower than
`run-jobs-e2e.sh`'s second configuration: every published artifact is a plain
build, so no demo reaches the worker backend on either origin, and what is only
true here is the page, the shim, the device and the pixels under the published
headers.

**It needs no GPU**, which is what lets it run on a CI runner. Getting there
took measuring something undocumented: three of the four obvious ways to read a
WebGPU canvas back return transparent black regardless of what was drawn.

| display  | adapter     | `canvas.toDataURL()` |
| -------- | ----------- | -------------------- |
| headless | hardware    | the pixels           |
| headless | SwiftShader | transparent black    |
| Xvfb     | SwiftShader | the pixels           |
| Xvfb     | hardware    | transparent black    |

(`drawImage(canvas, …)` and `createImageBitmap(canvas)` return transparent black
in all four.) Nothing in the harness trusts that table: it clears a canvas to a
known colour in the same browser with the same flags first, tries both adapters,
and refuses to interpret the render checks unless the control comes back with
the colour it drew. The flags, and the failure each one prevents, are in the two
files' headers.

`run-probe-e2e.sh` is the gate beside it, and the only one that drives
`crcbl-webgpu`'s seam **one command at a time**: the wasm→JS→wasm round trip,
the device it opens, every resource kind created on a real `GPUDevice`, and from
group S onward the readbacks whose _bytes_ are compared against the colour or
pattern the command asked for. It needs a site with `probe/index.html` in it,
which `build.sh` puts there:

```sh
./web/run-probe-e2e.sh --build        # Xvfb + Chromium, then the probe groups
```

It prints the group letters it recorded a check under beside the count, and
fails when any group **the script itself enumerates** is missing from that line
— a page that silently drove only some of the probes is the failure mode it has
to be able to see. The list of letters lives in `run-probe-e2e.sh` and is not
repeated here: it was written out as "G-Z and AA" once and every group added
after that made it staler, unnoticed, because nothing checks prose. Xvfb is not
optional on a machine with no GPU: under `--headless=new` plus SwiftShader the
three groups that acquire or present a canvas frame never resolve their readback
map.

## Checking it without a browser

Cheaper, and they catch a different class of mistake. Both run on every PR:

```sh
./web/build.sh                                   # builds and runs both checks below
node web/tools/check-exports.mjs target/site/demos/breakout/crcbl_breakout_bg.wasm
node web/tools/smoke.mjs        target/site/demos/breakout/crcbl_breakout_bg.wasm
node --check web/engine/*.js web/demos/*/*.js    # every file parses

./web/build.sh --threads                         # the threaded artifacts, and the worker gate
node web/tools/worker-gate.mjs \
  target/wasm-threaded/wasm32-unknown-unknown/release/examples/web_worker_gate.wasm
```

The `--threads` half is a **local** gate: it needs the pinned
`nightly-2026-07-02` with `rust-src`, which no CI runner has.

`check-exports.mjs` compares three lists: what the Rust sources declare with
`#[unsafe(no_mangle)]`, what the shim calls off the exports object, and what the
built artifact actually exports. A symbol in either of the first two that is
missing from the third is a failure. It also asserts `memory` is exported and
that the module imports nothing outside the loader's own module — which today
means nothing at all, since no artifact imports anything.

`worker-gate.mjs` is the node half of the gate over the Web Worker spawn backend
— `./web/run-jobs-e2e.sh` is the browser half, below. `./web/build.sh --threads`
builds `crates/crcbl-jobs/examples/web_worker_gate.rs` as a threaded `cdylib`
and drives it: the host announces itself through `__crcbl_web_jobs_host_ready`,
a `Pool` spawns through the seam, each queued request is drained with
`__crcbl_web_jobs_take`, and a `node:worker_threads` worker instantiates against
the same shared memory, writes `__stack_pointer`, calls `__wasm_init_tls` and
enters `__crcbl_web_jobs_entry`. It asserts that a chunk ran on a thread that is
not the driver, on a **stack of its own** and with **thread-locals of its own**
— both of which fail silently rather than loudly, which is why the gate has
`--no-stack-pointer` and `--no-init-tls` switches that must turn it red.

## Checking the worker backend in a browser

`run-jobs-e2e.sh` is the only place `crcbl_jobs::workers` runs where it is meant
to run. It builds the gate artifact, assembles a site of its own, and drives
`web/jobs/index.html` in headless Chromium:

```sh
./web/run-jobs-e2e.sh              # build the artifacts, then the browser run
./web/run-jobs-e2e.sh --no-build   # drive what is already built
```

No Xvfb, no GPU, no canvas — the page opens no device and draws nothing. What it
asks that `worker-gate.mjs` cannot:

- a browser `Worker` takes a structured-cloned `WebAssembly.Module` and a shared
  `WebAssembly.Memory`;
- that memory can be constructed at all, which is a property of the **document**
  rather than of the build. It is the `crossOriginIsolated` half of
  `tools/serve.mjs`, and it is why nothing threaded is publishable;
- a page's **main thread** survives driving a pool whose workers park on
  `memory.atomic.wait32`. Node lets its main thread block and a browser traps
  instead, so no gate there can show it. Measured here: 3000 `par_for` calls
  from the main thread with eight workers up, no trap;
- a **non-threaded** artifact — the shape every published one has — is refused
  workers. Announcing one would make `Spawn::threaded` answer true where no
  worker could ever start, which is the one failure that makes the backend lie
  rather than degrade, so the page loads a plain build of the same example
  alongside the threaded one and asserts the refusal.

**It drives the page twice, because both configurations are supported.** The run
above is the cross-origin isolated one. GitHub Pages sends no COOP/COEP, so the
origin every visitor gets has no `SharedArrayBuffer` and the backend degrades
onto `Inline`'s behaviour — a supported configuration rather than a gap, and one
nothing ever ran until the script started serving a second run behind
`tools/serve.mjs --no-isolation`. That run asserts the document is not isolated,
that a shared memory cannot be handed to a `Worker`, that the threaded artifact
is refused outright rather than instantiated against an unshared memory, and
that what is left is `Inline` **by name** at parallelism one, still reaching the
checksum the threaded run reproduces. Both runs name their backend: the
isolation flag on its own is satisfied by a pool that silently fell back.

`--no-isolation` on the server and `?no-isolation` on the page are deliberately
separate — the first withholds the headers, the second says which list to run —
so one without the other fails loudly on its first line, which is what makes
each of them falsifiable.

Four red switches ride in the query string, and the script runs every one of
them and insists the right assertion went red — and that the others did not.
`?no-init-tls` is the reason that pairing exists: skipping `__wasm_init_tls`
does **not** trap in this artifact, so the assertion that catches it is the one
about thread-locals holding a frame address their own stack could not have
produced, and a gate that waited for an exception would pass the broken build.

`web/jobs/` is pruned from `build.sh`'s copy rather than published beside
`probe/` and `harness/`. That is a correctness requirement, not tidiness: the
page loads an artifact importing a shared `env.memory`, which cannot exist on an
origin that sends no COOP/COEP pair. `engine/jobs.js` and
`engine/jobs-worker.js` are **not** pruned, and that is the same judgement made
the other way: they are the host half of the spawn ABI, a demo's threaded loader
needs the same code, and they refuse an artifact that owns its memory — so on
the published site they load, decide no, and announce nothing.

## Checking that a sample's sim runs off the main thread

`run-horde-threads-e2e.sh` is the gate on the exit criterion the one above does
not reach. `run-jobs-e2e.sh` proves the backend works in a browser, against a
page with no engine on it; this one drives **the horde demo** — the page a
visitor loads, the shim a visitor runs — on a threaded site:

```sh
./web/run-horde-threads-e2e.sh              # build both sites, then run
./web/run-horde-threads-e2e.sh --no-build   # drive what is already built
```

`./web/build.sh --threads` assembles that site into `target/site-threaded/`: the
same pages, the same `engine/`, the same `demos/<name>/main.js`, with the
worker-capable artifact beside each demo and `tools/wasm-loader-threads.js` as
its `<lib>.js`. `--threads --serve` will serve it, cross-origin isolated. It is
never `target/site/`, which is the directory the Pages workflow uploads.

The assertion is `__crcbl_horde_sim_threads() >= 2`, and it exists because
nothing else can make the claim: `steer_enemies` is bit-identical at any worker
count **by construction**, so a threaded run and an inline run draw the same
frames and every other check in this repository passes either way. Two
`__crcbl_horde_*` exports carry the evidence — the distinct threads that have
run a steering chunk, and the pool's worker count — and a third,
`__crcbl_horde_prefill`, is `--prefill` reachable from a page: `par_for` runs a
single chunk inline whatever the pool holds, so a demo with a small field never
leaves the main thread and the gate stages a crowd through the flag the scale
measurement already uses.

Three red checks, each breaking the criterion for a different reason and each
leaving a different neighbour green: `?no-host-ready` (the page never announces,
so the demo plays and steers entirely on its own thread), `--prefill 0` (no
crowd, so horde waits at its title screen and steers nothing), and **the
published site** — the artifacts `build.sh` builds, which must fail this gate
and pass everything else. That last one is the red check and the standing proof
that the published demos still degrade onto the inline path.

It needs Xvfb and a WebGPU-capable Chromium, unlike `run-jobs-e2e.sh`: horde
draws, and `__crcbl_horde_frame` polls the device request forever on a browser
that has no adapter, so a run with no GPU never reaches a steering pass at all.

`smoke.mjs` goes further and _runs_ the module: it instantiates the deployed
artifact under node with every import stubbed to throw, and drives the
documented call order — `prepare`, the fetch pre-load round trip, the OPFS
restore, `boot`, a key event through the scratch, `shutdown`. It stops at the
first `frame` that would request a device, because a device is `navigator.gpu`.
Everything before that line is plain Rust behind a browser-shaped ABI, and it
all runs here. What it cannot see, a black canvas included, is what
`run-browser-e2e.sh` is for.

## What is not here

- **No `SharedArrayBuffer` on Pages.** GitHub Pages cannot set COOP/COEP, so the
  published demos are single-threaded and the audio feed is `postMessage`-based
  rather than a ring buffer. `docs/plan/10-wasm-webgpu.md`'s 2026-07-27
  correction settles it. **Locally is different**: `web/tools/serve.mjs` sends
  both headers, so a site served by `build.sh --serve` or by the browser gate
  _is_ cross-origin isolated, and the gate asserts that rather than assuming it.
  That is what makes a threaded wasm build testable at all. **The build is here
  now**: `./web/build.sh --threads` produces worker-capable artifacts under
  `target/wasm-threaded/`, and `tools/check-exports.mjs --threads` gates the
  surface a worker needs — a shared `env.memory` import, `__wasm_init_tls`, and
  the TLS and stack globals. **The backend behind `crcbl-jobs`'s `Spawn` seam
  exists now too** — `default_spawner` yields `Workers` on wasm — and
  `web/jobs/` is a page that drives it end to end in a real browser, gated by
  `./web/run-jobs-e2e.sh`. **The demos are wired to it now as well**, on the
  threaded site only: `./web/build.sh --threads` assembles
  `target/site-threaded/` and `./web/run-horde-threads-e2e.sh` drives horde on
  it and asserts its steering pass ran on a Web Worker. **The published site is
  unchanged in behaviour**: its artifacts import nothing, so `engine/jobs.js`
  refuses them, `Spawn::threaded()` answers `false`, and every demo runs exactly
  as it did — which that gate's third red check asserts rather than assumes.
  **The demo gate runs on that origin too now**: `run-browser-e2e.sh` drives
  `hud` a second time behind `serve.mjs --no-isolation`, so "a demo boots, opens
  a device and draws" is a claim about the published origin and not only about
  the isolated one.
- **No clipboard, no IME.** The Web shell backend clears those capability bits;
  there is nothing for a shim to wire. **Pointer lock is wired**, and
  `RAW_POINTER_MOTION` with it: the shim takes the lock from a gesture and asks
  for `unadjustedMovement: true`, which is the OS-level bypass that bit exists
  to promise, so a first-person camera in a browser gets what it gets natively.
  `crates/crcbl-shell/src/web/mod.rs` carries the caveat — a browser that
  declines the option is retried without it, and the deltas are then the
  adjusted stream.
- **No service worker, no offline cache.** The site is static files.
