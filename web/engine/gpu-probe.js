// The page's half of the probes: the round trip, driven end to end.
//
// `crates/crcbl-webgpu/src/probe.rs` is the contract — the exports, the state
// codes, and why the module exists at all. The short version: everything else
// about this format is checked without a browser, against a committed fixture
// and a synthetic `WebAssembly.Memory`, and **none of that can call
// `navigator.gpu`**. Only a real browser can, so there has to be something a
// real browser can be told to run. This is it, and `web/tools/browser-e2e.mjs`
// is what tells it to.
//
// NOTHING IMPORTS THIS AT RUNTIME, and that is not an oversight. No demo drives
// the probe: the engine has no WebGPU backend yet, so the channel it installs is
// the only one in the page, and the moment a backend installs its own this file
// and the Rust module behind it both go. It ships with the site because the gate
// loads it from the demo's own origin, in the demo's own page, next to the demo's
// own wasm instance — a copy pasted into the driver would be testing the driver.
//
// NOTHING HERE MOVES A BYTE ACROSS THE SEAM, and that is the whole shape of
// this file. `web/engine/demo.js`'s frame loop drains the command stream,
// replays it and hands the replies back — one drain and one delivery for the
// page, in `pumpGpu`. There is one channel and one buffer behind it, so a
// second drain here would take commands that loop never sees and a second
// delivery would offer bytes its replayer has already cleared.
//
// So these functions ask wasm to *encode* something and read where it has got
// to, and everything in between belongs to the loop: what a frame carried, when
// it was replayed, what the replayer holds. `globalThis.crcbl.gpu` on a demo
// page is where the loop reports all three.
//
// This is also why there is no ordering rule left to obey. The version of this
// file that drained had to do its request and its drain in one uninterruptible
// run, because the demo's loop was a competitor for the same buffer; with one
// drainer there is no race to lose, and an `await` between any two calls below
// costs nothing but time.

import { readUtf8 } from './wasm.js';

/**
 * The `PROBE_*` codes `__crcbl_web_gpu_probe_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * `UNDECODABLE` is deliberately not folded into `REFUSED`: a refusal is a
 * browser with no GPU behind its WebGPU, and this is the two hand-written halves
 * of the format having drifted. They blame opposite ends of the build.
 */
export const PROBE = Object.freeze({
  UNASKED: 0,
  WAITING: 1,
  GRANTED: 2,
  REFUSED: 3,
  UNDECODABLE: 4,
});

/**
 * The `DEVICE_*` codes `__crcbl_web_gpu_probe_device_state` answers, from the
 * same file.
 *
 * `WAITING` is the ordinary answer on every frame between the ask and the
 * answer — `requestDevice` is a promise, and this is
 * `DeviceRequestState::Pending` seen through the ABI. `FAILED` is the browser
 * refusing, or wasm refusing to ask; `UNDECODABLE` is the format's two halves
 * having drifted, which blames the other end of the build entirely.
 */
export const DEVICE = Object.freeze({
  UNASKED: 0,
  WAITING: 1,
  OPENED: 2,
  FAILED: 3,
  UNDECODABLE: 4,
});

/**
 * The `CAPS_*` codes `__crcbl_web_gpu_probe_surface_caps_state` answers, from
 * the same file.
 *
 * `REFUSED` is **not** an error on this seam: `Instance::surface_caps` is how
 * adapter selection is done, so a query that answers nothing is an ordinary step
 * of it and comes back through the reply channel rather than as a thrown frame.
 * `UNDECODABLE` is the format's two halves having drifted, which blames the
 * other end of the build entirely.
 */
export const CAPS = Object.freeze({
  UNASKED: 0,
  WAITING: 1,
  ANSWERED: 2,
  REFUSED: 3,
  UNDECODABLE: 4,
});

/**
 * The `SurfaceCapsFailure` codes `crates/crcbl-webgpu/src/tag.rs` assigns, which
 * `__crcbl_web_gpu_probe_surface_caps_cause` answers with.
 *
 * One code, because `Command::SurfaceCaps` carries no arguments and so has
 * nothing to refuse: what is left is the query itself failing. `BACKEND` is `0`,
 * so it is also, unavoidably, what the export answers when nothing was refused.
 * That is why the cause is read only once the state says `REFUSED`.
 */
export const CAPS_FAILURE = Object.freeze({
  BACKEND: 0,
});

/**
 * The `PresentMode` codes the mode word's bits stand at, from the same file.
 *
 * `SurfaceCaps` promises `FIFO` is always offered, and a browser offers nothing
 * else: WebGPU has no present mode at all and a canvas presents at the
 * `requestAnimationFrame` boundary, which is what `Fifo` describes.
 */
export const PRESENT_MODE = Object.freeze({
  FIFO: 0,
  FIFO_RELAXED: 1,
  MAILBOX: 2,
  IMMEDIATE: 3,
});

/**
 * Names for those codes, for a log line that reads.
 *
 * @param {number} state
 * @returns {string}
 */
export function probeStateName(state) {
  const found = Object.entries(PROBE).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * The same, for the device's codes. Its own function rather than a second
 * argument, because the two tables happen to share their numbers and a call
 * that passed the wrong one would print a plausible name for the wrong state.
 *
 * @param {number} state
 * @returns {string}
 */
export function deviceStateName(state) {
  const found = Object.entries(DEVICE).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * The same again, for the capability query's codes. Its own function for
 * {@link deviceStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function capsStateName(state) {
  const found = Object.entries(CAPS).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * The name of the `SurfaceCapsFailure` a `cause` code stands for.
 *
 * @param {number} cause
 * @returns {string}
 */
export function capsFailureName(cause) {
  const found = Object.entries(CAPS_FAILURE).find(([, code]) => code === cause);
  return found ? found[0] : `unknown(${cause})`;
}

/**
 * Asks wasm to encode an adapter enumeration.
 *
 * **It is not replayed here, and this call cannot say what the frame carried.**
 * The command sits in the channel's buffer until the demo's next frame, where
 * that loop drains it, replays it, and — a frame or more later, because WebGPU's
 * adapter API is a promise — hands the answer back. What the frame carried is
 * therefore the loop's to report: `globalThis.crcbl.gpu.stats()` names the
 * commands of the last frame it replayed, and {@link readAdapterProbe} is where
 * the answer surfaces.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm took the request. `false` is a channel
 *   already installed by something else, or a full waiting set.
 */
export function startAdapterProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_adapters() === 1;
}

/**
 * Asks wasm to encode a device request against the adapter it was granted.
 *
 * The device half of {@link startAdapterProbe} in every respect, including the
 * one that matters: it encodes and returns, and the demo's loop does the rest.
 *
 * The enumeration has to have been answered first — the descriptor names an
 * adapter id from one — so a `false` here right after
 * {@link startAdapterProbe} is that ordering rather than a failure.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean}
 */
export function startDeviceProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_device() === 1;
}

/**
 * Asks wasm to encode a surface creation naming one of the page's canvases.
 *
 * **There is no `readSurfaceProbe`, and that is the command rather than a gap.**
 * `CreateSurface` has no entry on the reply channel — wasm names the handle
 * itself and moves on — so there is nothing to poll wasm for. Everything there
 * is to see is on this side, in the replayer the demo's loop drives:
 * `globalThis.crcbl.gpu.replayer.surfaces` is the table the context lands in,
 * keyed by the handle index the command carried, once that loop has replayed
 * the frame.
 *
 * `canvasId` must be a key the demo's registry holds — `crcbl.gpu.canvasId` is
 * the one it registers its own canvas under. One that is not makes the replay
 * **throw a `SurfaceError` in the demo's frame loop**, because there is no reply
 * channel to report it on: the near side is told loudly instead of the far side
 * being told wrongly. `gpu-replay.js` argues that choice where it is made, and
 * `demo.js` argues what its loop does about it — the throw is latched, logged,
 * and readable as `crcbl.gpu.stats().failure`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {number} options.canvasId The registry key of the canvas to present
 *   to. The page's own number: nothing in wasm knows what the shell registered.
 * @returns {boolean} Whether wasm encoded it. `false` is the probe being
 *   re-entered, or another channel being installed.
 */
export function startSurfaceProbe({ exports, canvasId }) {
  return exports.__crcbl_web_gpu_probe_surface(canvasId) === 1;
}

/**
 * Asks wasm to encode a query for what a canvas surface will accept.
 *
 * The third command that makes a round trip, and the encode-and-return half of
 * it: {@link readSurfaceCapsProbe} is where the answer surfaces, and everything
 * between belongs to the demo's loop.
 *
 * **There is nothing to choose, because the command carries nothing.** The
 * surface and the adapter `Instance::surface_caps` takes are validated against
 * an impl's own handle tables and never reach the wire — the record depends on
 * neither, since `getPreferredCanvasFormat()` is a method on `GPU`.
 *
 * Unlike {@link startDeviceProbe} this needs no adapter to have been granted
 * first, so it is legal on any frame.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm took the query. `false` is a channel already
 *   installed by something else, or a full waiting set.
 */
export function startSurfaceCapsProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_surface_caps() === 1;
}

/**
 * Reads where the adapter probe has got to.
 *
 * A reader and nothing more: the replies it is reading reached wasm through the
 * demo's frame loop, which is the page's one delivery. Call it once a frame
 * until the state settles. `WAITING` is the ordinary answer on the frames before
 * the browser has resolved its promise, and it is not a failure — it is the
 * whole shape of this seam.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, text: string,
 *            caps: { featuresLo: number, featuresHi: number, maxImage2d: number } }}
 *   `text` is the adapter's name under `GRANTED`, the reason under `REFUSED`,
 *   and the decode error under `UNDECODABLE`. `caps` is the part of the granted
 *   adapter's `DeviceCaps` the probe exports — see below — and is all zeros
 *   under every state but `GRANTED`, where `0` is also a legal value, so read it
 *   only once the state says so.
 */
export function readAdapterProbe({ exports, memory }) {
  // `state` first, `ptr` second: `state` decodes a buffer and clones a string,
  // so it allocates, and an allocation may grow wasm memory and detach any view
  // built before it. `ptr`, `len` and the three numbers below allocate nothing.
  const state = exports.__crcbl_web_gpu_probe_state();
  const text = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_text_ptr(),
    exports.__crcbl_web_gpu_probe_text_len()
  );
  // THE PART OF THE ANSWER A BROWSER CAN CORROBORATE. The whole of
  // `AdapterInfo` crosses the wire, but five of its seven wire fields are the
  // absences WebGPU forces — a browser has nothing to disagree with about a
  // vendor id it does not have. The feature word and a limit are the two that
  // vary per machine, so they are what `crates/crcbl-webgpu/src/probe.rs`
  // exports and what `browser-e2e.mjs` checks against `navigator.gpu`.
  //
  // Two halves rather than one `i64`, because the whole of this ABI is
  // `(i32, …) -> i32`. `>>> 0` because a wasm `i32` arrives signed.
  const caps = {
    featuresLo: exports.__crcbl_web_gpu_probe_features_lo() >>> 0,
    featuresHi: exports.__crcbl_web_gpu_probe_features_hi() >>> 0,
    maxImage2d: exports.__crcbl_web_gpu_probe_max_image_2d() >>> 0,
  };
  return { state, name: probeStateName(state), text, caps };
}

/**
 * The same for the device request.
 *
 * **Either read drains wasm's reply buffer for both probes** — there is one
 * channel and one committed buffer, so whichever of the two `state` calls runs
 * first decodes it and hands each probe its own answer. Calling only this one is
 * therefore enough to settle an enumeration too, and a frame that called neither
 * leaves both waiting on a buffer the demo's loop has already delivered.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string,
 *            caps: { featuresLo: number, featuresHi: number, maxImage2d: number } }}
 *   `reason` says why no device opened and is empty when one did. `caps` is the
 *   **device's** — not the adapter's — and is all zeros under every state but
 *   `OPENED`, where `0` is also a legal value, so read it only once the state
 *   says so.
 */
export function readDeviceProbe({ exports, memory }) {
  // `state` first, `ptr` second, for `readAdapterProbe`'s reason: this is the
  // call that allocates.
  const state = exports.__crcbl_web_gpu_probe_device_state();
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_device_reason_ptr(),
    exports.__crcbl_web_gpu_probe_device_reason_len()
  );
  // THE PART A BROWSER CAN CORROBORATE, and here it corroborates something the
  // adapter's numbers cannot: a page can open its own default device and read
  // `device.features` and `device.limits` off it, and both differ from the
  // adapter's on any machine whose adapter reports more than the floor.
  const caps = {
    featuresLo: exports.__crcbl_web_gpu_probe_device_features_lo() >>> 0,
    featuresHi: exports.__crcbl_web_gpu_probe_device_features_hi() >>> 0,
    maxImage2d: exports.__crcbl_web_gpu_probe_device_max_image_2d() >>> 0,
  };
  return { state, name: deviceStateName(state), reason, caps };
}

/**
 * The same for the capability query.
 *
 * **This read drains wasm's reply buffer for every probe**, on
 * {@link readDeviceProbe}'s terms and for its reason: there is one channel and
 * one committed buffer, so whichever `state` call runs first in a frame decodes
 * it and hands each probe its own answer.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, cause: number,
 *            causeName: string, format: number, presentModes: number,
 *            hasExtent: boolean }}
 *   `reason` says why the query answered nothing and is empty when it answered.
 *   `cause` is meaningful only under `REFUSED` — `0` is a real cause as well as
 *   "nothing was refused". `format`, `presentModes` and `hasExtent` are
 *   meaningful only under `ANSWERED`, where `0` is legal for each, so read them
 *   only once the state says so.
 */
export function readSurfaceCapsProbe({ exports, memory }) {
  // `state` first, `ptr` second, for `readAdapterProbe`'s reason: this is the
  // call that allocates.
  const state = exports.__crcbl_web_gpu_probe_surface_caps_state();
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_surface_caps_reason_ptr(),
    exports.__crcbl_web_gpu_probe_surface_caps_reason_len()
  );
  const cause = exports.__crcbl_web_gpu_probe_surface_caps_cause() >>> 0;
  // THE PART A BROWSER CAN CORROBORATE IS `format`, AND ONLY IT. The whole of
  // `SurfaceCaps` crosses the wire, but five of its six fields are decisions
  // `gpu-replay.js` made rather than answers the browser gave — a browser has no
  // present mode, no image count and no `currentExtent` to disagree about. The
  // preferred canvas format is the one field it fills, and the page can ask
  // `navigator.gpu.getPreferredCanvasFormat()` for itself, which is what makes
  // `browser-e2e.mjs`'s check on it evidence rather than a constant.
  //
  // The other two below are here for a different job: `presentModes` and the
  // extent flag are the cheapest facts that say the rest of the record survived
  // the crossing, because a format decodes correctly whatever happened to the
  // lists behind it. See `crates/crcbl-webgpu/src/probe.rs`.
  return {
    state,
    name: capsStateName(state),
    reason,
    cause,
    causeName: capsFailureName(cause),
    format: exports.__crcbl_web_gpu_probe_surface_caps_format() >>> 0,
    presentModes:
      exports.__crcbl_web_gpu_probe_surface_caps_present_modes() >>> 0,
    hasExtent: exports.__crcbl_web_gpu_probe_surface_caps_has_extent() === 1,
  };
}
