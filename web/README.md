# `web/` — the JS shim and the demo site

The browser half of P5. Everything here is hand-written ES modules loaded
directly by the page: **no framework, no bundler, no npm, no `node_modules`.**
That is the same policy `docs/plan/15-windowing.md` sets for the engine's
platform bindings — thin bindings to an ABI the platform forces on us, never a
framework that owns policy — and it applies here for the same reason.

## Layout

| Path                      | What it is                                              |
| ------------------------- | ------------------------------------------------------- |
| `index.html`              | the demo site index                                     |
| `style.css`               | one stylesheet for the site                             |
| `engine/wasm.js`          | reading/writing wasm memory, and the detached-view rule |
| `engine/shell.js`         | canvas, DPI/resize, keyboard, pointer → `__crcbl_web_*` |
| `engine/storage.js`       | asset pre-load over `fetch()`, OPFS restore and drain   |
| `engine/audio.js`         | main-thread half of the AudioWorklet feed               |
| `engine/audio-worklet.js` | the `AudioWorkletProcessor` itself                      |
| `engine/log.js`           | drains the engine's log queue into the console          |
| `demos/breakout/`         | the breakout demo page and its boot sequence            |
| `tools/check-exports.mjs` | the JS↔wasm symbol contract check                       |
| `tools/smoke.mjs`         | runs the artifact's boot sequence under node            |
| `build.sh`                | assembles `target/site/`                                |

Each `engine/` module implements the JS side of an ABI a Rust module already
specified symbol by symbol. Those specifications are the source of truth:

- `crates/crcbl-shell/src/web/mod.rs` — input and frame
- `crates/crcbl-audio/src/web.rs` — the audio pull
- `crates/crcbl-store/src/web/fetch.rs` — assets
- `crates/crcbl-store/src/web/opfs.rs` — saves
- `apps/breakout/src/web.rs` — boot, frame, teardown, logs

## Build and run

```sh
cargo install wasm-bindgen-cli --version "$(awk '/^name = "wasm-bindgen"$/{f=1;next} f&&/^version/{gsub(/[",]/,"",$3);print $3;exit}' Cargo.lock)" --locked
./web/build.sh --serve      # http://localhost:8000/
```

`file://` will not work: ES modules need an origin, and the Origin Private File
System needs a secure context. `localhost` is one.

## Why `wasm-bindgen` is here at all

The engine's own ABI is **hand-written** `extern "C"` — no `#[wasm_bindgen]`
anywhere in the workspace, and no `wasm-bindgen` in any crate's dependency list.
The tool is still required, and not by choice:

`crcbl-wgpu` is the browser's graphics backend, `wgpu` reaches WebGPU through
`web-sys`, and `web-sys` is `wasm-bindgen`. A raw `cargo build` artifact
therefore imports ~320 functions from `__wbindgen_placeholder__`, which nothing
but the `wasm-bindgen` CLI can resolve. `WebAssembly.instantiateStreaming` on
that file is a `LinkError`, every time. Dropping the tool means dropping `wgpu`,
which means there is no browser backend at all.

So the split is: **`wasm-bindgen` links `wgpu` to the browser; it does not
define our ABI.** The generated `init()` returns the instance's raw exports
object, and everything in `engine/` calls the hand-written `__crcbl_*` symbols
off it directly. `tools/check-exports.mjs` asserts that all 60 of them survive
the tool, and that no import outside its glue ever appears.

The CLI version is not pinned in a second place — `build.sh` reads it out of
`Cargo.lock` and refuses to run on a mismatch, because a mismatched CLI produces
glue whose imports the module does not have and the failure surfaces in
somebody's browser rather than in the build.

## Checking it in a browser

`run-browser-e2e.sh` is the P5 gate. It builds nothing itself — point it at a
site `build.sh` already produced, or pass `--build`:

```sh
./web/run-browser-e2e.sh --build      # Xvfb + Chromium, then 18 checks
./web/run-browser-e2e.sh --headless --hardware   # the real GPU, no display
```

It starts its own Xvfb and its own static server, opens Chromium over the
DevTools protocol, sends a **real** click and a **real** Space key into the
canvas, and reads the canvas back to assert the frame is neither one flat colour
nor identical from frame to frame while the ball is in flight. It fails if zero
checks ran.

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

## Checking it without a browser

Cheaper, and they catch a different class of mistake. Both run on every PR:

```sh
./web/build.sh                                   # builds and runs both checks below
node web/tools/check-exports.mjs target/site/demos/breakout/crcbl_breakout_bg.wasm
node web/tools/smoke.mjs        target/site/demos/breakout/crcbl_breakout_bg.wasm
node --check web/engine/*.js web/demos/*/*.js    # every file parses
```

`check-exports.mjs` compares three lists: what the Rust sources declare with
`#[unsafe(no_mangle)]`, what the shim calls off the exports object, and what the
built artifact actually exports. A symbol in either of the first two that is
missing from the third is a failure. It also asserts `memory` is exported and
that the module imports nothing outside the bindgen glue.

`smoke.mjs` goes further and _runs_ the module: it instantiates the deployed
artifact under node with every `wasm-bindgen` import stubbed to throw, and
drives the documented call order — `prepare`, the fetch pre-load round trip, the
OPFS restore, `boot`, a key event through the scratch, `shutdown`. It stops at
the first `frame` that would request a device, because a device is
`navigator.gpu`. Everything before that line is plain Rust behind a
browser-shaped ABI, and it all runs here. What it cannot see, a black canvas
included, is what `run-browser-e2e.sh` is for.

## What is not here

- **No `SharedArrayBuffer`.** GitHub Pages cannot set COOP/COEP, so the demos
  are single-threaded and the audio feed is `postMessage`-based rather than a
  ring buffer. `docs/plan/10-wasm-webgpu.md`'s 2026-07-27 correction settles it.
- **No pointer lock, no clipboard, no IME.** The Web shell backend clears those
  capability bits; there is nothing for a shim to wire.
- **No service worker, no offline cache.** The site is static files.
