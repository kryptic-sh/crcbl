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
 * The `READBACK_*` codes `__crcbl_web_gpu_probe_readback_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * `PENDING` and `WAITING` both mean "poll again": `WAITING` is a poll out and
 * unanswered, `PENDING` is a poll answered "not yet". `READY` is the one that
 * carries bytes.
 *
 * `FAILED` is the browser refusing the map and saying why — a settled state,
 * not a step, because a readback is answered exactly once. Read it before
 * polling again: the reason exports carry the browser's own words, and
 * treating it as "not yet" is what turns a named refusal into a timeout.
 */
export const READBACK = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `DRAW_*` codes `__crcbl_web_gpu_probe_draw_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The draw probe is a readback at heart — its setup frame ends in the same
 * `request_readback` — so its codes mirror {@link READBACK} exactly. `READY`
 * carries the drawn pixels, which the gate checks are the draw colour and not
 * the clear.
 */
export const DRAW = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `PRESENT_*` codes `__crcbl_web_gpu_probe_present_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The present probe is a readback at heart — its setup frame ends in the same
 * `request_readback` — so its codes mirror {@link READBACK} exactly. `READY`
 * carries the presented pixels, which the gate checks are the present colour and
 * so proves the real canvas-context path (configure, getCurrentTexture, render,
 * copy) ran end to end.
 */
export const PRESENT = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `RECONFIG_*` codes `__crcbl_web_gpu_probe_reconfigure_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The reconfigure probe is the present probe with one command more, so it too is
 * a readback at heart and its codes mirror {@link READBACK} exactly. `READY`
 * carries the reconfigured pixels, which the gate checks are the BGRA present
 * colour — the proof that the reconfigure re-ran `configure` with the new format
 * rather than leaving the swapchain in its created one.
 */
export const RECONFIG = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `INDIRECT_*` codes `__crcbl_web_gpu_probe_indirect_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The indirect-draw probe is a readback at heart — its setup frame ends in the
 * same `request_readback` — so its codes mirror {@link READBACK} exactly. `READY`
 * carries the drawn pixels, which the gate checks are the draw colour and not the
 * clear, proving an indirect `drawIndexedIndirect` put exactly what a direct draw
 * would.
 */
export const INDIRECT = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `DEPTH_*` codes `__crcbl_web_gpu_probe_depth_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The depth probe is a readback at heart — its setup frame ends in the same
 * `request_readback` — so its codes mirror {@link READBACK} exactly. `READY`
 * carries the 64×64 `depth32float` texels of a cleared depth atlas, which the
 * gate checks are the clear value: the one claim no native suite can make about
 * this backend, because a depth plane only crosses `copyTextureToBuffer` in a
 * browser.
 */
export const DEPTH = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `STENCIL_*` codes `__crcbl_web_gpu_probe_stencil_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The stencil probe is a readback at heart — its setup frame ends in the same
 * `request_readback` — so its codes mirror {@link READBACK} exactly. `READY`
 * carries the 64×64 `Rgba8Unorm` texels of a target two draws competed for, and
 * which colour they hold is the whole evidence that a `setStencilReference`
 * decided which draw survived: the one claim no native suite can make about this
 * backend.
 */
export const STENCIL = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `MSAA_*` codes `__crcbl_web_gpu_probe_msaa_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The MSAA probe is a readback at heart — its setup frame ends in the same
 * `request_readback` — so its codes mirror {@link READBACK} with one addition,
 * `UNSUPPORTED`, which pushes its `FAILED` to `7` rather than `6`.
 * `READY` carries the `Rgba8Unorm` texels of a single-sampled target a
 * multisampled pass resolved into, and whether they are the clear or the poison
 * the target was primed with is the whole evidence that the resolve ran: the one
 * claim no native suite can make about this backend.
 *
 * `UNSUPPORTED` is the addition, and the reason this probe has a sample-count
 * export beside it: the device reported a `max_sample_count` below the one the
 * probe resolves, so there was no multisampled target to resolve from and
 * nothing was encoded. Read `__crcbl_web_gpu_probe_msaa_samples` to say what it
 * reported.
 */
export const MSAA = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  UNSUPPORTED: 6,
  FAILED: 7,
});

/**
 * The `CLAMP_*` codes `__crcbl_web_gpu_probe_clamp_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The depth-clamp probe is a readback at heart — its setup frame ends in the
 * same `request_readback` — so its codes are {@link MSAA}'s, `UNSUPPORTED` and
 * the `FAILED` of `7` included. `READY` carries two blocks of `Rgba8Unorm`
 * texels: the target a pipeline with `depth_clamp` drew into, then the target
 * the pipeline beside it drew into with the flag off and nothing else changed.
 * Which colour each block holds is the whole evidence that depth clamping
 * happened, because the same triangle was drawn into both, past the far plane.
 *
 * `UNSUPPORTED` is the device opening without `depth-clip-control`, which is
 * also why this probe has a supported flag beside it: WebGPU refuses
 * `primitive.unclippedDepth` on such a device, so nothing was encoded rather
 * than two identical blocks being read back and called a pass. Read
 * `__crcbl_web_gpu_probe_clamp_supported` to say which happened.
 */
export const CLAMP = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  UNSUPPORTED: 6,
  FAILED: 7,
});

/**
 * The `FIRST_INSTANCE_*` codes `__crcbl_web_gpu_probe_first_instance_state`
 * answers, from `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The first-instance probe is a readback at heart — its setup frame ends in the
 * same `request_readback` — so its codes are {@link CLAMP}'s, `UNSUPPORTED` and
 * the `FAILED` of `7` included. `READY` carries two blocks of `Rgba8Unorm`
 * texels: the target an indirect draw whose `firstInstance` is zero painted,
 * then the target an otherwise identical draw whose `firstInstance` is one
 * painted. The shader shifts its quad right by `@builtin(instance_index)`, so
 * *which half of each block* holds the draw colour is the whole evidence that
 * the number in the argument structure reached the GPU.
 *
 * `UNSUPPORTED` is the device opening without `indirect-first-instance`, which
 * is also why this probe has a supported flag beside it: core WebGPU accepts
 * only a zero `firstInstance` in an indirect draw, so nothing was encoded
 * rather than two identical blocks being read back and called a pass. Read
 * `__crcbl_web_gpu_probe_first_instance_supported` to say which happened.
 */
export const FIRST_INSTANCE = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  UNSUPPORTED: 6,
  FAILED: 7,
});

/**
 * The `TEXTURE_SAMPLE_*` codes `__crcbl_web_gpu_probe_texture_sample_state`
 * answers, from `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The texture-sampling probe is a readback at heart — its setup frame ends in
 * the same `request_readback` — so its codes are {@link DRAW}'s **seven**, and
 * that is the difference worth reading: {@link CLAMP} and
 * {@link FIRST_INSTANCE} each spend `6` on an `UNSUPPORTED` because a device can
 * withhold the feature their fixtures need, and sampling a `rgba8unorm` texture
 * is core WebGPU. There is no device that can refuse this, so there is no
 * `UNSUPPORTED` here and `FAILED` is `6`.
 *
 * `READY` carries one block of `Rgba8Unorm` texels: the target a fullscreen quad
 * painted by sampling a two-by-two source whose four texels are four different
 * colours. Which colour each *quadrant* of the block came back as is the whole
 * evidence — that a texture reached the fragment shader at all, and that the
 * texel it delivered was the one under that quadrant rather than a flipped,
 * transposed or channel-swapped neighbour.
 */
export const TEXTURE_SAMPLE = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `OCCLUSION_*` codes `__crcbl_web_gpu_probe_occlusion_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The occlusion probe is a readback at heart — its setup frame ends in the same
 * `request_readback` — so its codes mirror {@link READBACK} exactly. `READY`
 * carries one little-endian `u64` per query, resolved out of a `GPUQuerySet`
 * over a destination that was filled with a sentinel first: whether those bytes
 * are the sentinel or zero is the whole evidence that the resolve ran against a
 * set the browser really created, which is the one claim no native suite can
 * make about this backend.
 */
export const OCCLUSION = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `OCCLUSION_VALUES_*` codes
 * `__crcbl_web_gpu_probe_occlusion_values_state` answers.
 *
 * The *other* half of the same exercise: `Device::query_results` reading the
 * same queries through a resolve, a copy and a map the **replayer** performs,
 * because a `GPUQuerySet` has no accessor. Three codes rather than six, because
 * there is nothing to poll — the replayer answers when its own map settles, so
 * there is no `PENDING` between `WAITING` and `READY` and no poll to issue.
 *
 * **`READY` with no values is a failed read**, not an empty success: the seam
 * never asks for zero values, so an empty list is the only way a `QueryResults`
 * reply can say it could not be served.
 */
export const OCCLUSION_VALUES = Object.freeze({
  UNASKED: 0,
  WAITING: 1,
  READY: 2,
});

/**
 * The `TIMESTAMP_*` codes `__crcbl_web_gpu_probe_timestamp_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * {@link OCCLUSION_VALUES}' shape — one ask, no poll — on a `'timestamp'` query
 * set, plus an `UNSUPPORTED` for a browser that opened without
 * `timestamp-query`, which is a device that could not create such a set at all.
 *
 * **`READY` with no values is a failed read**, and **`READY` with two zeros is a
 * pass nothing timed**: an unwritten query resolves to zero by specification, so
 * a browser that took the pass descriptor and wrote neither query reads back as
 * zeros. That is exactly the outcome this backend refused timestamp sets over
 * until the seam's timestamps moved into the pass descriptor.
 */
export const TIMESTAMP = Object.freeze({
  UNASKED: 0,
  WAITING: 1,
  READY: 2,
  UNSUPPORTED: 3,
});

/**
 * The `COMPUTE_*` codes `__crcbl_web_gpu_probe_compute_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The dispatch probe is a readback at heart — its setup frame ends in the same
 * `request_readback` — so its codes mirror {@link READBACK} exactly. `READY`
 * carries the 64 `u32`s the dispatch wrote, which the gate checks are all the
 * known pattern and so proves the dispatch ran.
 */
export const COMPUTE = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `DISPATCH_INDIRECT_*` codes
 * `__crcbl_web_gpu_probe_dispatch_indirect_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The indirect-dispatch probe is a readback at heart — its setup frame ends in
 * the same `request_readback` — so its codes mirror {@link READBACK} exactly.
 * `READY` carries the tally the dispatched workgroups wrote: a counter of how
 * many ran, then one region per axis in which the slots up to that axis's count
 * are marked and the rest are still zero. The gate reads the three workgroup
 * counts back out of it, so a dispatch that used the wrong counts — `1x1x1`
 * above all — is a different readback rather than a missing one.
 */
export const DISPATCH_INDIRECT = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `COPYCHAIN_*` codes `__crcbl_web_gpu_probe_copychain_state` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * The copy-chain probe is a readback at heart — its setup frame ends in the same
 * `request_readback` — so its codes mirror {@link READBACK} exactly. `READY`
 * carries the 64×64 `rgba8unorm` texels the chain moved, which the gate checks
 * are all red and so proves `copyBufferToTexture` and `copyTextureToTexture` ran.
 */
export const COPYCHAIN = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
});

/**
 * The `FILL_*` codes `__crcbl_web_gpu_probe_fill_state` answers, from the same
 * file.
 *
 * The fill probe is a readback at heart — its setup frame ends in the same
 * `request_readback` — so its codes mirror {@link READBACK} exactly. `READY`
 * carries the 64 `u32`s, the first half zeroed by `clearBuffer` and the second
 * half still the pattern, which is what the gate checks.
 */
export const FILL = Object.freeze({
  UNASKED: 0,
  REQUESTED: 1,
  WAITING: 2,
  PENDING: 3,
  READY: 4,
  UNDECODABLE: 5,
  FAILED: 6,
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
 * Asks wasm to encode a buffer creation on the device it opened.
 *
 * **There is no `readBufferProbe`, for {@link startSurfaceProbe}'s reason:**
 * `CreateBuffer` has no entry on the reply channel — wasm names the handle
 * itself and moves on — so there is nothing to poll wasm for. What there is to
 * see is on this side: `globalThis.crcbl.gpu.replayer.buffers` is the table the
 * `GPUBuffer` lands in, once the demo's loop has replayed the frame.
 *
 * **And what could not be done is on this side too.** A creation that fails —
 * a size the device will not allocate, a usage combination WebGPU refuses, a
 * flag it has no bit for — has no reply to arrive in either, so the replayer
 * queues the reason where `crcbl_hal::Device::take_error` will drain it:
 * `crcbl.gpu.replayer.takeError()`. `gpu-replay.js` argues that choice where it
 * is made.
 *
 * Unlike every other probe here this one needs a **device**, not just an
 * adapter: `create_buffer` is a device method, so a `false` right after
 * {@link startDeviceProbe} is that ordering rather than a failure — wait for
 * {@link readDeviceProbe} to say `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {number} options.size How many bytes to ask for. The page's own
 *   number, so that what it reads back off `GPUBuffer.size` is something it
 *   chose rather than a constant wasm and the check share.
 * @returns {boolean} Whether wasm encoded it. `false` is no device open yet,
 *   the probe being re-entered, or another channel being installed.
 */
export function startBufferProbe({ exports, size }) {
  return exports.__crcbl_web_gpu_probe_buffer(size) === 1;
}

/**
 * Asks wasm to encode an image creation on the device it opened.
 *
 * **There is no `readImageProbe`, for {@link startBufferProbe}'s reason:**
 * `CreateImage` has no entry on the reply channel either. What there is to see
 * is on this side — `globalThis.crcbl.gpu.replayer.images` is the table the
 * `GPUTexture` lands in — and what could not be done is on this side too, in
 * `crcbl.gpu.replayer.takeError()`.
 *
 * It needs a **device**, as {@link startBufferProbe} does and for the same
 * reason: `create_image` is a device method, so a `false` right after
 * {@link startDeviceProbe} is that ordering rather than a failure.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {number} options.width Texels across. The page's own number, so what it
 *   reads back off `GPUTexture.width` is something it chose.
 * @param {number} options.height Texels down.
 * @param {number} options.mipLevels How many mip levels to ask for. Must fit the
 *   extent — a browser refuses a longer chain than the size can hold, and says
 *   so through `takeError()`.
 * @returns {boolean} Whether wasm encoded it. `false` is no device open yet, the
 *   probe being re-entered, or another channel being installed.
 */
export function startImageProbe({ exports, width, height, mipLevels }) {
  return exports.__crcbl_web_gpu_probe_image(width, height, mipLevels) === 1;
}

/**
 * Asks wasm to encode a view of the image {@link startImageProbe} created.
 *
 * **THE IMAGE HAS TO BE THERE, AND NEITHER SIDE OF THIS CALL CHECKS IT.** The
 * image lives in the page's replayer and nothing in wasm holds one, so a view
 * naming an image that was never created — or one already destroyed, or one at a
 * generation the slot has moved past — is encoded happily and reported by the
 * replayer through `crcbl.gpu.replayer.takeError()`. It does not throw, because
 * a view arriving before its image is a far side that got its ordering wrong
 * mid-frame and taking the frame down would abandon every command after it;
 * `gpu-replay.js` argues that where it is made.
 *
 * **The descriptor takes nothing, and its range is the whole image.** Both
 * counts are `ImageSubresourceRange::ALL` — `u32::MAX` — which crosses the wire
 * verbatim, and WebGPU spells "the rest" as an *absent* descriptor member rather
 * than as a number. So this is the call that puts that resolution in front of a
 * real browser: `crcbl.gpu.replayer.imageViews` is the table the
 * `GPUTextureView` lands in, and a replayer that passed `4294967295` on gets a
 * view the browser refuses.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded it, on {@link startImageProbe}'s
 *   terms.
 */
export function startImageViewProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_image_view() === 1;
}

/**
 * Asks wasm to encode a sampler creation on the device it opened.
 *
 * **There is no `readSamplerProbe`, for {@link startBufferProbe}'s reason:**
 * `CreateSampler` has no entry on the reply channel either.
 * `globalThis.crcbl.gpu.replayer.samplers` is the table the `GPUSampler` lands
 * in, and a descriptor the browser would not have arrives in
 * `crcbl.gpu.replayer.takeError()`.
 *
 * **AND THERE IS NOTHING TO PASS IN**, which is where it differs from
 * {@link startImageProbe} rather than from {@link startImageViewProbe}: a
 * `GPUSampler` reports its `label` and nothing else — no filters, no address
 * modes, no clamps — so a number chosen by the page could not be read back off
 * the object anyway. The descriptor is fixed in
 * `crates/crcbl-webgpu/src/probe.rs`, and it is chosen for what a browser can
 * *refuse*: its `lod_max` is `f32::MAX`, the "no limit" sentinel, which crosses
 * the wire verbatim and which the replayer has to hand WebGPU as an explicit
 * `lodMaxClamp`. Omitting the member would substitute WebGPU's own default,
 * which is a number rather than "the rest", and only a real `createSampler` can
 * say the value this seam sends is one it accepts — it says so by reporting
 * nothing on the device's error channel.
 *
 * It needs a **device**, as {@link startBufferProbe} does and for the same
 * reason: `create_sampler` is a device method, so a `false` right after
 * {@link startDeviceProbe} is that ordering rather than a failure.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded it, on {@link startImageProbe}'s
 *   terms.
 */
export function startSamplerProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_sampler() === 1;
}

/**
 * Asks wasm to encode a bind-group layout creation on the device it opened.
 *
 * **There is no `readBindGroupLayoutProbe`, for {@link startBufferProbe}'s
 * reason:** `CreateBindGroupLayout` has no entry on the reply channel either.
 * `globalThis.crcbl.gpu.replayer.bindGroupLayouts` is the table the
 * `GPUBindGroupLayout` lands in, and a layout the browser would not have arrives
 * in `crcbl.gpu.replayer.takeError()`.
 *
 * **AND NOTHING TO PASS IN**, on {@link startSamplerProbe}'s terms exactly: a
 * `GPUBindGroupLayout` reports its `label` and nothing else — not its entries,
 * not their bindings, not their visibility — so a number chosen by the page
 * could not be read back off the object. The descriptor is fixed in
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * **What is new is that the descriptor is a LIST.** Every command before this
 * one is a fixed set of fields; this one carries four entries, each five fields
 * deep, each holding an enum whose variants have different-length payloads. A
 * stride out by a byte therefore does not truncate — it decodes the next entry
 * out of the middle of this one and produces a layout that is well-formed and
 * describes different resources. Four entries, and every one of them a kind
 * WebGPU can express, is what makes a real `createBindGroupLayout` able to
 * disagree.
 *
 * It needs a **device**, as {@link startBufferProbe} does and for the same
 * reason: `create_bind_group_layout` is a device method, so a `false` right
 * after {@link startDeviceProbe} is that ordering rather than a failure.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded it, on {@link startImageProbe}'s
 *   terms.
 */
export function startBindGroupLayoutProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_bind_group_layout() === 1;
}

/**
 * Asks wasm to encode a bind-group creation on the device it opened.
 *
 * **There is no `readBindGroupProbe`, for {@link startBufferProbe}'s reason:**
 * `CreateBindGroup` has no entry on the reply channel either.
 * `globalThis.crcbl.gpu.replayer.bindGroups` is the table the `GPUBindGroup`
 * lands in, and a group the browser would not have arrives in
 * `crcbl.gpu.replayer.takeError()`.
 *
 * **AND NOTHING TO PASS IN**, on {@link startBindGroupLayoutProbe}'s terms
 * exactly: a `GPUBindGroup` reports its `label` and nothing else — not its
 * layout, not its entries — so a number chosen by the page could not be read back
 * off the object. The descriptor is fixed in
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * **What is new is that this one export encodes a whole FRAME.** A bind group
 * names a live layout and live resources, so wasm records the layout, a buffer,
 * an image, its view and a sampler before the group — and the group itself binds
 * one handle into each of three resource tables, which is what puts the
 * `BindingResource` discriminant and the `WHOLE_BUFFER` sentinel in front of a
 * real `createBindGroup`.
 *
 * It needs a **device**, as {@link startBufferProbe} does and for the same
 * reason: `create_bind_group` is a device method, so a `false` right after
 * {@link startDeviceProbe} is that ordering rather than a failure.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded it, on {@link startImageProbe}'s
 *   terms.
 */
export function startBindGroupProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_bind_group() === 1;
}

/**
 * Asks wasm to encode a shader-module creation on the device it opened.
 *
 * **There is no `readShaderModuleProbe`, for {@link startBufferProbe}'s reason:**
 * `CreateShaderModule` has no entry on the reply channel either.
 * `globalThis.crcbl.gpu.replayer.shaderModules` is the table the
 * `GPUShaderModule` lands in, and a module the browser would not have — one
 * carrying no WGSL for this backend to compile — arrives in
 * `crcbl.gpu.replayer.takeError()`.
 *
 * **What differs from every probe before it is what a browser can be asked about
 * the result.** A `GPUShaderModule` reports its `label`, like a sampler, but it
 * is also **where compilation happens** — so beyond `instanceof GPUShaderModule`
 * the gate reads `getCompilationInfo()` off the object and holds it to no errors
 * for the known-good WGSL the descriptor carries. That is stronger than mere
 * existence and is the one thing a stub cannot fake: node has no
 * `GPUShaderModule` binding and no compiler behind it. The descriptor is fixed in
 * `crates/crcbl-webgpu/src/probe.rs` — WGSL alone, the other three artifacts
 * absent — because a browser consumes only WGSL.
 *
 * It needs a **device**, as {@link startBufferProbe} does and for the same
 * reason: `create_shader_module` is a device method, so a `false` right after
 * {@link startDeviceProbe} is that ordering rather than a failure.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded it, on {@link startImageProbe}'s
 *   terms.
 */
export function startShaderModuleProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_shader_module() === 1;
}

/**
 * Asks wasm to encode a pipeline-layout creation on the device it opened.
 *
 * **There is no `readPipelineLayoutProbe`, for {@link startBufferProbe}'s
 * reason:** `CreatePipelineLayout` has no entry on the reply channel either.
 * `globalThis.crcbl.gpu.replayer.pipelineLayouts` is the table the
 * `GPUPipelineLayout` lands in, and a layout the browser would not have — one
 * carrying push constants, or naming a bind-group layout it cannot resolve —
 * arrives in `crcbl.gpu.replayer.takeError()`.
 *
 * **AND NOTHING TO PASS IN**, on {@link startBindGroupLayoutProbe}'s terms: a
 * `GPUPipelineLayout` reports its `label` and nothing else — not its bind-group
 * layouts, not its push-constant ranges — so a number chosen by the page could
 * not be read back off the object. The descriptor is fixed in
 * `crates/crcbl-webgpu/src/probe.rs`, with `push_constants: None` so it *builds*
 * rather than being refused.
 *
 * **What is new is that this one export encodes a whole FRAME.** A pipeline
 * layout names a live bind-group layout, so wasm records the layout before the
 * pipeline layout — and the pipeline layout resolves that layout out of the
 * bind-group-layout table, which is what puts the set-index resolution in front
 * of a real `createPipelineLayout`.
 *
 * It needs a **device**, as {@link startBufferProbe} does and for the same
 * reason: `create_pipeline_layout` is a device method, so a `false` right after
 * {@link startDeviceProbe} is that ordering rather than a failure.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded it, on {@link startImageProbe}'s
 *   terms.
 */
export function startPipelineLayoutProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_pipeline_layout() === 1;
}

/**
 * Asks wasm to encode a compute-pipeline creation on the device it opened.
 *
 * **There is no `readComputePipelineProbe`, for {@link startBufferProbe}'s
 * reason:** `CreateComputePipeline` has no entry on the reply channel either.
 * `globalThis.crcbl.gpu.replayer.computePipelines` is the table the
 * `GPUComputePipeline` lands in, and a pipeline the browser would not have — one
 * naming a shader module or a pipeline layout the replayer cannot resolve —
 * arrives in `crcbl.gpu.replayer.takeError()`.
 *
 * **What differs from every probe before it is what a browser can be asked about
 * the result.** A `GPUComputePipeline` reports its `label`, like a pipeline
 * layout, but it also answers `getBindGroupLayout(n)` — the derived layout only a
 * genuinely-built pipeline can hand back, because a pipeline is where the shader
 * and its layout are validated against each other. That is stronger than mere
 * existence and is a second thing a stub cannot fake: node has no
 * `GPUComputePipeline` binding at all. The descriptor is fixed in
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * **What is new is that the pipeline resolves handles into two *different*
 * tables.** Its export encodes a whole frame — a compute shader module, an empty
 * pipeline layout, and the pipeline built from both — and the pipeline then
 * resolves one id out of the shader-module table and one out of the
 * pipeline-layout table, which is the first command anywhere to do that.
 *
 * It needs a **device**, as {@link startBufferProbe} does and for the same
 * reason: `create_compute_pipeline` is a device method, so a `false` right after
 * {@link startDeviceProbe} is that ordering rather than a failure.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded it, on {@link startImageProbe}'s
 *   terms.
 */
export function startComputePipelineProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_compute_pipeline() === 1;
}

/**
 * Asks wasm to encode a graphics (render) pipeline creation on the device it
 * opened.
 *
 * **There is no `readGraphicsPipelineProbe`, for {@link startBufferProbe}'s
 * reason:** `CreateGraphicsPipeline` has no entry on the reply channel either.
 * `globalThis.crcbl.gpu.replayer.graphicsPipelines` is the table the
 * `GPURenderPipeline` lands in, and a pipeline the browser would not have — one
 * naming a shader module or a pipeline layout the replayer cannot resolve, or a
 * descriptor field WebGPU cannot express — arrives in
 * `crcbl.gpu.replayer.takeError()`.
 *
 * **What a browser can be asked about the result is the compute pipeline's two
 * things**: a `GPURenderPipeline` reports its `label` and answers
 * `getBindGroupLayout(n)`, the derived layout only a genuinely-built pipeline can
 * hand back — a second thing a stub cannot fake, since node has no
 * `GPURenderPipeline` binding at all.
 *
 * **What is new is that this is the largest descriptor on the seam.** Its export
 * encodes a whole frame — a vertex-plus-fragment shader module, an empty pipeline
 * layout, and the pipeline built from both — and the pipeline resolves the module
 * for both stages and the layout, then carries the whole nested tree (the
 * primitive state, the reversed-Z depth-stencil, the multisample state, and a
 * blended colour target) that a real `createRenderPipeline` has to accept. The
 * descriptor is fixed in `crates/crcbl-webgpu/src/probe.rs`.
 *
 * It needs a **device**, as {@link startBufferProbe} does and for the same
 * reason: `create_graphics_pipeline` is a device method, so a `false` right after
 * {@link startDeviceProbe} is that ordering rather than a failure.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded it, on {@link startImageProbe}'s
 *   terms.
 */
export function startGraphicsPipelineProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_graphics_pipeline() === 1;
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
 * How a limit row's two halves become one JavaScript number.
 *
 * `>>> 0` on each because a wasm `i32` arrives signed. The 64-bit join
 * multiplies rather than shifts: `<<` in JavaScript is a 32-bit operator, so
 * `hi << 32` is `hi`. Every value the seam carries here is an image dimension,
 * a buffer size or an alignment, all far below `2 ** 53`, so a `number` holds
 * them exactly and nothing has to become a `BigInt` — which would not survive
 * the wire back to the browser-gate driver anyway.
 */
const joinU32 = (lo) => lo >>> 0;
const joinU64 = (lo, hi) => (hi >>> 0) * 2 ** 32 + (lo >>> 0);
/**
 * The one row that is not an integer: `max_sampler_anisotropy` is an `f32` and
 * this ABI carries no floats, so it travels as `f32::to_bits` and the bit
 * pattern is reinterpreted here.
 */
const joinF32 = (lo) => new Float32Array(new Uint32Array([lo >>> 0]).buffer)[0];

/**
 * `crcbl_hal::Limits` in the index order `__crcbl_web_gpu_probe_limit_lo` takes,
 * which is the order `crates/crcbl-webgpu/src/probe.rs` writes out under its
 * `# Exports` heading. Each row names the seam field in the lowerCamel spelling
 * `halLimitsFor` in `gpu-replay.js` uses, says how its two halves join, and — for the one
 * array field — how many consecutive indices it spends.
 *
 * **What keeps the two lists in step is not this file.** The Rust side
 * destructures `Limits` with no rest pattern, so a field added to the seam
 * cannot compile without a row there; and `__crcbl_web_gpu_probe_limit_count`
 * is what says whether the table below reaches as far as that one does, which
 * is why {@link readLimitTable} reports the count it read rather than assuming
 * this list is the whole of it.
 *
 * One table for both replies, because wasm flattens one `crcbl_hal::Limits` the
 * one way whichever reply carried it — see `granted_limits` in
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * @type {ReadonlyArray<{ field: string,
 *                        join: (lo: number, hi: number) => number,
 *                        arity?: number }>}
 */
const LIMIT_ROWS = [
  { field: 'maxImage2d', join: joinU32 },
  { field: 'maxImage3d', join: joinU32 },
  { field: 'maxImageArrayLayers', join: joinU32 },
  { field: 'maxStorageBufferRange', join: joinU64 },
  { field: 'maxUniformBufferRange', join: joinU64 },
  { field: 'maxBindGroups', join: joinU32 },
  { field: 'maxBindlessDescriptors', join: joinU32 },
  { field: 'maxPushConstantSize', join: joinU32 },
  { field: 'maxColorAttachments', join: joinU32 },
  { field: 'maxSampleCount', join: joinU32 },
  { field: 'maxDrawIndirectCount', join: joinU32 },
  { field: 'maxComputeWorkgroupSize', join: joinU32, arity: 3 },
  { field: 'maxComputeInvocationsPerWorkgroup', join: joinU32 },
  { field: 'maxComputeWorkgroupsPerDimension', join: joinU32 },
  { field: 'minUniformBufferOffsetAlignment', join: joinU64 },
  { field: 'minStorageBufferOffsetAlignment', join: joinU64 },
  { field: 'optimalBufferCopyOffsetAlignment', join: joinU64 },
  { field: 'maxSamplerAnisotropy', join: joinF32 },
];

/**
 * `crcbl_hal::Limits` as an indexed pair of readers answers it, keyed by
 * {@link LIMIT_ROWS}.
 *
 * **Two callers, one loop.** {@link readAdapterProbe} and
 * {@link readDeviceProbe} ask different exports for numbers that are genuinely
 * different — an adapter's limits are its ceilings, a device's are the ones it
 * was created with — but the flattening they are decoding is the same one, so a
 * second copy of this walk would be the place the two orders drift apart.
 *
 * Driven by `count`, which is wasm's own, rather than by {@link LIMIT_ROWS}'
 * length: a seam that grew a limit then shows up as a table this file is short
 * of, rather than as a loop that stopped early and looked complete.
 *
 * **Allocates nothing in wasm**, which is what lets it run after the `state`
 * call that may have grown memory: every export it reaches for is a numeric
 * getter.
 *
 * @param {number} count How many scalars wasm says it flattened.
 * @param {(index: number) => number} lo The low half of the scalar at `index`.
 * @param {(index: number) => number} hi The high half of the same scalar.
 * @returns {Record<string, number | number[] | undefined>} Keyed by
 *   {@link LIMIT_ROWS}, with `undefined` in any field wasm reported fewer
 *   scalars than this file has rows for.
 */
function readLimitTable(count, lo, hi) {
  /** @type {{ lo: number, hi: number }[]} */
  const scalars = [];
  for (let index = 0; index < count; index += 1) {
    scalars.push({ lo: lo(index) >>> 0, hi: hi(index) >>> 0 });
  }
  let cursor = 0;
  /** The next scalar, joined, or `undefined` if wasm reported fewer than that. */
  const take = (join) => {
    const scalar = scalars[cursor];
    cursor += 1;
    return scalar === undefined ? undefined : join(scalar.lo, scalar.hi);
  };
  /** @type {Record<string, number | number[] | undefined>} */
  const limits = {};
  for (const { field, join, arity } of LIMIT_ROWS) {
    limits[field] =
      arity === undefined
        ? take(join)
        : Array.from({ length: arity }, () => take(join));
  }
  return limits;
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
 *            caps: { featuresLo: number, featuresHi: number, maxImage2d: number,
 *                    limitCount: number,
 *                    limits: Record<string, number | number[] | undefined> } }}
 *   `text` is the adapter's name under `GRANTED`, the reason under `REFUSED`,
 *   and the decode error under `UNDECODABLE`. `caps` is the part of the granted
 *   adapter's `DeviceCaps` the probe exports — see below — and is all zeros
 *   under every state but `GRANTED`, where `0` is also a legal value, so read it
 *   only once the state says so. `caps.limits` is the whole of the adapter's
 *   `crcbl_hal::Limits`, keyed by {@link LIMIT_ROWS}; `caps.limitCount` is how
 *   many scalars wasm said it had, so a caller can tell a full read from a
 *   table this file has fallen behind on.
 */
export function readAdapterProbe({ exports, memory }) {
  // `state` first, `ptr` second: `state` decodes a buffer and clones a string,
  // so it allocates, and an allocation may grow wasm memory and detach any view
  // built before it. `ptr`, `len` and every number read below allocate nothing,
  // which is what lets the limit table be read after the string.
  const state = exports.__crcbl_web_gpu_probe_state();
  const text = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_text_ptr(),
    exports.__crcbl_web_gpu_probe_text_len()
  );
  // THE PART OF THE ANSWER A BROWSER CAN CORROBORATE. The whole of
  // `AdapterInfo` crosses the wire, but five of its seven wire fields are the
  // absences WebGPU forces — a browser has nothing to disagree with about a
  // vendor id it does not have. The feature word and the limits are what vary
  // per machine, so they are what `crates/crcbl-webgpu/src/probe.rs` exports
  // and what `browser-e2e.mjs` checks against `navigator.gpu`.
  //
  // Two halves rather than one `i64`, because the whole of this ABI is
  // `(i32, …) -> i32`. `>>> 0` because a wasm `i32` arrives signed.
  //
  // `maxImage2d` stays a member of its own beside the indexed table that also
  // carries it: it is the number every existing caller reads, and the table
  // arrived later.
  const limitCount = exports.__crcbl_web_gpu_probe_limit_count() >>> 0;
  const caps = {
    featuresLo: exports.__crcbl_web_gpu_probe_features_lo() >>> 0,
    featuresHi: exports.__crcbl_web_gpu_probe_features_hi() >>> 0,
    maxImage2d: exports.__crcbl_web_gpu_probe_max_image_2d() >>> 0,
    limitCount,
    limits: readLimitTable(
      limitCount,
      (index) => exports.__crcbl_web_gpu_probe_limit_lo(index),
      (index) => exports.__crcbl_web_gpu_probe_limit_hi(index)
    ),
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
 *            caps: { featuresLo: number, featuresHi: number, maxImage2d: number,
 *                    limitCount: number,
 *                    limits: Record<string, number | number[] | undefined> } }}
 *   `reason` says why no device opened and is empty when one did. `caps` is the
 *   **device's** — not the adapter's — and is all zeros under every state but
 *   `OPENED`, where `0` is also a legal value, so read it only once the state
 *   says so. `caps.limits` is the whole of the device's `crcbl_hal::Limits`,
 *   keyed by {@link LIMIT_ROWS}; `caps.limitCount` is how many scalars wasm said
 *   it had, and is the same count the adapter's read reports because the
 *   flattening is the same one.
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
  const limitCount = exports.__crcbl_web_gpu_probe_limit_count() >>> 0;
  const caps = {
    featuresLo: exports.__crcbl_web_gpu_probe_device_features_lo() >>> 0,
    featuresHi: exports.__crcbl_web_gpu_probe_device_features_hi() >>> 0,
    maxImage2d: exports.__crcbl_web_gpu_probe_device_max_image_2d() >>> 0,
    limitCount,
    limits: readLimitTable(
      limitCount,
      (index) => exports.__crcbl_web_gpu_probe_device_limit_lo(index),
      (index) => exports.__crcbl_web_gpu_probe_device_limit_hi(index)
    ),
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

/**
 * The same state-name helper again, for the readback codes. Its own function
 * for {@link deviceStateName}'s reason: a shared one passed the wrong table
 * would print a plausible name for the wrong state.
 *
 * @param {number} state
 * @returns {string}
 */
export function readbackStateName(state) {
  const found = Object.entries(READBACK).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to clear a texture and start reading it back on the device it opened.
 *
 * The setup half of the readback probe: one frame that clears, copies, submits
 * and requests. {@link pollReadbackProbe} drives the poll, and
 * {@link readReadbackProbe} reads where it has got to and the bytes when they
 * land.
 *
 * Unlike every creation probe here this one's answer is *data*, and it needs a
 * **device**, not just an adapter: `false` before one has opened is that
 * ordering rather than a failure — wait for {@link readDeviceProbe} to say
 * `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startReadbackProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_readback() === 1;
}

/**
 * Poll the in-flight readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame without tracking whether a
 * poll is outstanding. See `__crcbl_web_gpu_probe_readback_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollReadbackProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_readback_poll() === 1;
}

/**
 * Read where the readback has got to, and its bytes once it is `READY`.
 *
 * `state` first, because it drains the reply buffer and decodes a reply — which
 * allocates, and an allocation may grow wasm memory and detach a view built
 * before it, exactly the hazard {@link readAdapterProbe} guards. The pointer and
 * length allocate nothing, so the `Uint8Array` is built from `memory.buffer`
 * *after* the state call, and copied out with `slice` so a later drain cannot
 * move the bytes under a caller holding the view.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readReadbackProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_readback_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_readback_reason_ptr(),
    exports.__crcbl_web_gpu_probe_readback_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_readback_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_readback_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: readbackStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the draw codes. Its own function for
 * {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function drawStateName(state) {
  const found = Object.entries(DRAW).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to draw a red triangle over a clear and start reading it back on the
 * device it opened.
 *
 * {@link startReadbackProbe}'s draw sibling: one frame that builds a pipeline,
 * clears, binds and draws it, copies, submits and requests. {@link pollDrawProbe}
 * drives the poll and {@link readDrawProbe} reads the bytes when they land. Its
 * answer is *data* and it needs a **device**, so `false` before one has opened
 * is ordering rather than failure — wait for {@link readDeviceProbe} to say
 * `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startDrawProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_draw() === 1;
}

/**
 * Poll the draw's in-flight readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. See
 * `__crcbl_web_gpu_probe_draw_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollDrawProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_draw_poll() === 1;
}

/**
 * Read where the draw readback has got to, and its bytes once it is `READY`.
 *
 * {@link readReadbackProbe}'s draw sibling, and `state` first for its reason —
 * draining allocates and may detach a view built before it, so the `Uint8Array`
 * is built after the state call and copied out with `slice`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readDrawProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_draw_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_draw_reason_ptr(),
    exports.__crcbl_web_gpu_probe_draw_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_draw_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_draw_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: drawStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the present codes. Its own function for
 * {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function presentStateName(state) {
  const found = Object.entries(PRESENT).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to present a frame to a canvas and start reading it back on the
 * device it opened.
 *
 * {@link startDrawProbe}'s present sibling, and the first probe to drive a *real
 * canvas context*: one frame that creates a surface on the canvas `canvasId`
 * names, configures a swapchain on it, acquires the frame, clears the acquired
 * view red, copies it out, submits, presents (a no-op) and requests.
 * {@link pollPresentProbe} drives the poll and {@link readPresentProbe} reads the
 * bytes when they land. Its answer is *data* and it needs a **device**, so
 * `false` before one has opened is ordering rather than failure — wait for
 * {@link readDeviceProbe} to say `OPENED`.
 *
 * `canvasId` must be a key the demo's registry holds, exactly as
 * {@link startSurfaceProbe}'s must — `crcbl.gpu.canvasId` is the one the page's
 * canvas is registered under.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {number} options.canvasId The registry key of the canvas to present to.
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startPresentProbe({ exports, canvasId }) {
  return exports.__crcbl_web_gpu_probe_present(canvasId) === 1;
}

/**
 * Poll the present's in-flight readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. See
 * `__crcbl_web_gpu_probe_present_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollPresentProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_present_poll() === 1;
}

/**
 * Read where the present readback has got to, and its bytes once it is `READY`.
 *
 * {@link readDrawProbe}'s present sibling, and `state` first for its reason —
 * draining allocates and may detach a view built before it, so the `Uint8Array`
 * is built after the state call and copied out with `slice`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readPresentProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_present_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_present_reason_ptr(),
    exports.__crcbl_web_gpu_probe_present_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_present_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_present_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: presentStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the reconfigure codes. Its own function
 * for {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function reconfigureStateName(state) {
  const found = Object.entries(RECONFIG).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to reconfigure a swapchain and present a frame in the new format, and
 * start reading it back on the device it opened.
 *
 * {@link startPresentProbe}'s sibling with one command more: the swapchain is
 * created `Rgba8Unorm` and then reconfigured `Bgra8Unorm` before the acquire, so
 * the frame that comes back is in BGRA byte order. {@link pollReconfigureProbe}
 * drives the poll and {@link readReconfigureProbe} reads the bytes when they land.
 * Its answer is *data* and it needs a **device**, so `false` before one has opened
 * is ordering rather than failure.
 *
 * `canvasId` must be a key the registry holds, exactly as
 * {@link startPresentProbe}'s must.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {number} options.canvasId The registry key of the canvas to present to.
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startReconfigureProbe({ exports, canvasId }) {
  return exports.__crcbl_web_gpu_probe_reconfigure(canvasId) === 1;
}

/**
 * Poll the reconfigure's in-flight readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. See
 * `__crcbl_web_gpu_probe_reconfigure_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollReconfigureProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_reconfigure_poll() === 1;
}

/**
 * Read where the reconfigure readback has got to, and its bytes once it is
 * `READY`.
 *
 * {@link readPresentProbe}'s sibling, and `state` first for its reason — draining
 * allocates and may detach a view built before it, so the `Uint8Array` is built
 * after the state call and copied out with `slice`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readReconfigureProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_reconfigure_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_reconfigure_reason_ptr(),
    exports.__crcbl_web_gpu_probe_reconfigure_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_reconfigure_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_reconfigure_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: reconfigureStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the indirect codes. Its own function for
 * {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function indirectStateName(state) {
  const found = Object.entries(INDIRECT).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to render a frame with an INDIRECT draw and start reading it back on
 * the device it opened.
 *
 * {@link startDrawProbe}'s indirect sibling: the same fullscreen-triangle
 * pipeline, but the draw reads its counts from an args buffer
 * (`drawIndexedIndirect`) that a `write_buffer` filled, over an index buffer a
 * second `write_buffer` filled. {@link pollIndirectProbe} drives the poll and
 * {@link readIndirectProbe} reads the bytes when they land. Its answer is *data*
 * and it needs a **device**, so `false` before one has opened is ordering rather
 * than failure — wait for {@link readDeviceProbe} to say `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startIndirectProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_indirect() === 1;
}

/**
 * Poll the indirect draw's in-flight readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. See
 * `__crcbl_web_gpu_probe_indirect_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollIndirectProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_indirect_poll() === 1;
}

/**
 * Read where the indirect-draw readback has got to, and its bytes once it is
 * `READY`.
 *
 * {@link readDrawProbe}'s indirect sibling, and `state` first for its reason —
 * draining allocates and may detach a view built before it, so the `Uint8Array`
 * is built after the state call and copied out with `slice`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readIndirectProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_indirect_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_indirect_reason_ptr(),
    exports.__crcbl_web_gpu_probe_indirect_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_indirect_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_indirect_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: indirectStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the depth codes. Its own function for
 * {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function depthStateName(state) {
  const found = Object.entries(DEPTH).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to clear a `depth32float` atlas and copy its DEPTH PLANE out to a
 * buffer, and start reading that buffer back on the device it opened.
 *
 * {@link startDrawProbe}'s depth sibling, and the only gate that moves a depth
 * plane across `copyTextureToBuffer` at all. It needs no pipeline: the pass has
 * one depth attachment, no colour attachment, and the clear is the write.
 * {@link pollDepthProbe} drives the poll and {@link readDepthProbe} reads the
 * bytes when they land. Its answer is *data* and it needs a **device**, so
 * `false` before one has opened is ordering rather than failure — wait for
 * {@link readDeviceProbe} to say `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startDepthProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_depth() === 1;
}

/**
 * Poll the depth readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. See
 * `__crcbl_web_gpu_probe_depth_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollDepthProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_depth_poll() === 1;
}

/**
 * Read where the depth readback has got to, and its bytes once it is `READY`.
 *
 * {@link readDrawProbe}'s depth sibling, and `state` first for its reason.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readDepthProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_depth_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_depth_reason_ptr(),
    exports.__crcbl_web_gpu_probe_depth_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_depth_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_depth_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: depthStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the stencil codes. Its own function for
 * {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function stencilStateName(state) {
  const found = Object.entries(STENCIL).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to draw twice into one target with a different stencil reference
 * before each, and start reading the target back on the device it opened.
 *
 * {@link startDrawProbe}'s masked sibling, and the only gate anywhere that shows
 * a `setStencilReference` deciding which fragments survive. The pass clears a
 * `depth24plus-stencil8` plane to a known value and the pipeline compares
 * `Equal` against whatever the pass last set — there is no pipeline-side
 * reference on this seam — so the colour that comes back says which of the two
 * per-pass references took effect. The first of them is set *before* the
 * pipeline is bound, so a bind that reset it would show up too.
 * {@link pollStencilProbe} drives the poll and {@link readStencilProbe} reads the
 * bytes when they land. Its answer is *data* and it needs a **device**, so
 * `false` before one has opened is ordering rather than failure — wait for
 * {@link readDeviceProbe} to say `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startStencilProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_stencil() === 1;
}

/**
 * Poll the stencil readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. See
 * `__crcbl_web_gpu_probe_stencil_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollStencilProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_stencil_poll() === 1;
}

/**
 * Read where the stencil readback has got to, and its bytes once it is `READY`.
 *
 * {@link readDrawProbe}'s masked sibling, and `state` first for its reason.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readStencilProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_stencil_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_stencil_reason_ptr(),
    exports.__crcbl_web_gpu_probe_stencil_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_stencil_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_stencil_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: stencilStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the MSAA codes. Its own function for
 * {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function msaaStateName(state) {
  const found = Object.entries(MSAA).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * What the opened device reported as its `max_sample_count`.
 *
 * Read this *before* {@link startMsaaProbe}, and read it again when the state is
 * `UNSUPPORTED`: it is what tells "this device cannot make a multisampled
 * target" from "nothing asked it to". `0` before a device opens.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {number}
 */
export function msaaSampleCount({ exports }) {
  return exports.__crcbl_web_gpu_probe_msaa_samples() >>> 0;
}

/**
 * Ask wasm to clear a multisampled target and resolve it into a single-sampled
 * one, and start reading that one back on the device it opened.
 *
 * The only gate anywhere that puts a resolve view on this backend's wire. The
 * pass has **no draws** — a resolve is an end-of-pass operation over whatever the
 * samples hold, and a clear puts a known value in all of them — and the resolve
 * target is primed with a poison colour first, so a resolve that was accepted and
 * dropped comes back distinguishable from one that ran. {@link pollMsaaProbe}
 * drives the poll and {@link readMsaaProbe} reads the bytes when they land.
 *
 * Its answer is *data* and it needs a **device**, so `false` before one has
 * opened is ordering rather than failure — wait for {@link readDeviceProbe} to
 * say `OPENED`. `false` also means the device reported too few samples, which
 * {@link msaaSampleCount} and a state of `UNSUPPORTED` are what distinguish.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame.
 */
export function startMsaaProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_msaa() === 1;
}

/**
 * Poll the MSAA readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered, the bytes are already
 * in, or the device could not serve the sample count, so the gate can call it
 * every frame. See `__crcbl_web_gpu_probe_msaa_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollMsaaProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_msaa_poll() === 1;
}

/**
 * Read where the MSAA readback has got to, and its bytes once it is `READY`.
 *
 * {@link readDrawProbe}'s resolving sibling, and `state` first for its reason.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readMsaaProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_msaa_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_msaa_reason_ptr(),
    exports.__crcbl_web_gpu_probe_msaa_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_msaa_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_msaa_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: msaaStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the depth-clamp codes. Its own function
 * for {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function clampStateName(state) {
  const found = Object.entries(CLAMP).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Whether the device this page opened has the browser's `depth-clip-control`.
 *
 * Read this *before* {@link startClampProbe}, and read it again when the state
 * is `UNSUPPORTED`: it is what tells "this device withheld the feature the
 * clamped pipeline needs" from "nothing asked it to". `false` before a device
 * opens.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean}
 */
export function clampSupported({ exports }) {
  return exports.__crcbl_web_gpu_probe_clamp_supported() === 1;
}

/**
 * Ask wasm to draw one triangle past the far plane twice — through a pipeline
 * whose `depth_clamp` is set and through one whose is not — and start reading
 * both targets back on the device it opened.
 *
 * The only gate anywhere that carries `depth_clamp` to a browser at all. What
 * comes back is a *difference*: with depth clipping disabled the triangle
 * rasterises, with it enabled the triangle is discarded, and the two blocks the
 * readback holds are those two outcomes side by side. The unclamped half is the
 * control and is not optional — without it, geometry that turned out to be
 * inside the depth range after all would paint both blocks and prove nothing.
 * {@link pollClampProbe} drives the poll and {@link readClampProbe} reads the
 * bytes when they land.
 *
 * Its answer is *data* and it needs a **device**, so `false` before one has
 * opened is ordering rather than failure — wait for {@link readDeviceProbe} to
 * say `OPENED`. `false` also means the device opened without the feature, which
 * {@link clampSupported} and a state of `UNSUPPORTED` are what distinguish.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame.
 */
export function startClampProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_clamp() === 1;
}

/**
 * Poll the depth-clamp readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered, the bytes are already
 * in, or the device withheld the feature, so the gate can call it every frame.
 * See `__crcbl_web_gpu_probe_clamp_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollClampProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_clamp_poll() === 1;
}

/**
 * Read where the depth-clamp readback has got to, and its bytes once it is
 * `READY` — the clamped target's block first, the clipped one's second.
 *
 * {@link readMsaaProbe}'s sibling, and `state` first for its reason.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readClampProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_clamp_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_clamp_reason_ptr(),
    exports.__crcbl_web_gpu_probe_clamp_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_clamp_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_clamp_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: clampStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the first-instance codes. Its own
 * function for {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function firstInstanceStateName(state) {
  const found = Object.entries(FIRST_INSTANCE).find(
    ([, code]) => code === state
  );
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Whether the device this page opened has the browser's
 * `indirect-first-instance`.
 *
 * Read this *before* {@link startFirstInstanceProbe}, and read it again when the
 * state is `UNSUPPORTED`: it is what tells "this device withheld the feature the
 * non-zero `firstInstance` needs" from "nothing asked it to". `false` before a
 * device opens.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean}
 */
export function firstInstanceSupported({ exports }) {
  return exports.__crcbl_web_gpu_probe_first_instance_supported() === 1;
}

/**
 * Ask wasm to draw one half-width quad twice through one pipeline — off an
 * indirect argument structure whose `firstInstance` is zero and off one whose is
 * one — and start reading both targets back on the device it opened.
 *
 * The only gate anywhere that carries a non-zero `firstInstance` to a browser at
 * all. What comes back is a *difference*: the shader shifts its quad right by
 * `@builtin(instance_index)`, which WebGPU defines as the draw's `firstInstance`
 * plus the instance number, so the zero draw paints the left half of its target
 * and the one draw the right half of its. The zero half is the control and is
 * not optional — without it, a shader that shifted every instance would paint the
 * right half whatever the argument structure said, and prove nothing.
 * {@link pollFirstInstanceProbe} drives the poll and
 * {@link readFirstInstanceProbe} reads the bytes when they land.
 *
 * Its answer is *data* and it needs a **device**, so `false` before one has
 * opened is ordering rather than failure — wait for {@link readDeviceProbe} to
 * say `OPENED`. `false` also means the device opened without the feature, which
 * {@link firstInstanceSupported} and a state of `UNSUPPORTED` are what
 * distinguish.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame.
 */
export function startFirstInstanceProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_first_instance() === 1;
}

/**
 * Poll the first-instance readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered, the bytes are already
 * in, or the device withheld the feature, so the gate can call it every frame.
 * See `__crcbl_web_gpu_probe_first_instance_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollFirstInstanceProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_first_instance_poll() === 1;
}

/**
 * Read where the first-instance readback has got to, and its bytes once it is
 * `READY` — the zero draw's target first, the one draw's second.
 *
 * {@link readClampProbe}'s sibling, and `state` first for its reason.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readFirstInstanceProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_first_instance_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_first_instance_reason_ptr(),
    exports.__crcbl_web_gpu_probe_first_instance_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_first_instance_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_first_instance_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: firstInstanceStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the texture-sampling codes. Its own
 * function for {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function textureSampleStateName(state) {
  const found = Object.entries(TEXTURE_SAMPLE).find(
    ([, code]) => code === state
  );
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to upload a two-by-two texture, sample it across a fullscreen quad
 * through a nearest sampler, and start reading the target back on the device it
 * opened.
 *
 * **The only gate anywhere that puts a `textureSample` in front of a GPU.**
 * `BindingKind::SampledImage` and `BindingKind::Sampler` are declared, built and
 * accepted by a browser elsewhere — a `GPUBindGroupLayout` reports its `label`
 * and nothing else, so that is as far as those checks reach. This one binds a
 * real view and a real sampler to a fragment shader and asserts the texels that
 * come out.
 *
 * What comes back is one block, and the claim is *which colour each quadrant
 * holds*: the four source texels are four different colours, so a shader that
 * returned a constant, a flipped V axis, a transposed UV and a swapped channel
 * each produce a different block from the correct one.
 *
 * There is no `supported` flag beside this, and no `UNSUPPORTED` state, because
 * sampling is core WebGPU — no device can withhold it.
 * {@link pollTextureSampleProbe} drives the poll and
 * {@link readTextureSampleProbe} reads the bytes when they land.
 *
 * Its answer is *data* and it needs a **device**, so `false` before one has
 * opened is ordering rather than failure — wait for {@link readDeviceProbe} to
 * say `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame.
 */
export function startTextureSampleProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_texture_sample() === 1;
}

/**
 * Poll the texture-sampling readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. See
 * `__crcbl_web_gpu_probe_texture_sample_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollTextureSampleProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_texture_sample_poll() === 1;
}

/**
 * Read where the texture-sampling readback has got to, and its bytes once it is
 * `READY` — one block of the sampled target's texels.
 *
 * {@link readFirstInstanceProbe}'s sibling, and `state` first for its reason.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readTextureSampleProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_texture_sample_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_texture_sample_reason_ptr(),
    exports.__crcbl_web_gpu_probe_texture_sample_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_texture_sample_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_texture_sample_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: textureSampleStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the occlusion codes. Its own function
 * for {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function occlusionStateName(state) {
  const found = Object.entries(OCCLUSION).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * The same helper again, for the direct read's own three codes.
 *
 * @param {number} state
 * @returns {string}
 */
export function occlusionValuesStateName(state) {
  const found = Object.entries(OCCLUSION_VALUES).find(
    ([, code]) => code === state
  );
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to build an occlusion query set, resolve it over a sentinel, and
 * start reading the result back — **both ways at once**.
 *
 * The only gate anywhere that puts a `GPUQuerySet` on this backend's wire. One
 * frame carries the set, a `QUERY_RESOLVE` destination filled with a sentinel,
 * the seam's reset (which WebGPU has no call for and the replayer records
 * nothing for), the resolve over that sentinel, the copy, the readback request
 * — and a `query_results` ask that reads the same queries the other way, through
 * a resolve the *replayer* performs. Two mechanisms, one set, one expected
 * answer; {@link pollOcclusionProbe} drives the readback's poll,
 * {@link readOcclusionProbe} reads its bytes, and
 * {@link readOcclusionValues} reads the direct read's.
 *
 * Its answer is *data* and it needs a **device**, so `false` before one has
 * opened is ordering rather than failure — wait for {@link readDeviceProbe} to
 * say `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startOcclusionProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_occlusion() === 1;
}

/**
 * Poll the occlusion readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. The direct read is never
 * polled: the replayer answers it when its own map settles. See
 * `__crcbl_web_gpu_probe_occlusion_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollOcclusionProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_occlusion_poll() === 1;
}

/**
 * Read where the occlusion readback has got to, and its bytes once it is
 * `READY`.
 *
 * {@link readDrawProbe}'s query sibling, and `state` first for its reason.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readOcclusionProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_occlusion_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_occlusion_reason_ptr(),
    exports.__crcbl_web_gpu_probe_occlusion_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_occlusion_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_occlusion_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: occlusionStateName(state), reason, bytes };
}

/**
 * Read where the **direct** query read has got to, and its values once it is
 * `READY`.
 *
 * The values are little-endian `u64`s and are handed over as bytes rather than
 * as a `BigUint64Array` view, because a typed array of that width needs its
 * offset aligned and nothing about a wasm allocation promises one. The caller
 * reads them eight at a time, which is what the gate does.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, bytes: Uint8Array }}
 */
export function readOcclusionValues({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_occlusion_values_state() >>> 0;
  const ptr = exports.__crcbl_web_gpu_probe_occlusion_values_ptr();
  const len = exports.__crcbl_web_gpu_probe_occlusion_values_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: occlusionValuesStateName(state), bytes };
}

/**
 * The same helper again, for the timed pass's four codes.
 *
 * @param {number} state
 * @returns {string}
 */
export function timestampStateName(state) {
  const found = Object.entries(TIMESTAMP).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Whether the device this page opened has the browser's `timestamp-query`.
 *
 * **Read before {@link startTimestampProbe}**, and the reason the probe has a
 * flag of its own: it is what tells a browser that cannot serve timestamps from
 * a request that never happened, so an `UNSUPPORTED` is a stated fact about this
 * browser rather than a silent skip.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean}
 */
export function timestampSupported({ exports }) {
  return exports.__crcbl_web_gpu_probe_timestamp_supported() === 1;
}

/**
 * Ask wasm to submit a compute pass whose descriptor names two timestamp
 * queries, and to read both of them back.
 *
 * **The only gate anywhere that puts a `'timestamp'` `GPUQuerySet` on this
 * backend's wire, and the only one that reads a `timestampWrites` back as
 * values.** WebGPU takes a timestamp nowhere but a pass descriptor, which is why
 * the seam has no free-standing write left; the pass is empty because what is
 * being observed is whether the browser writes the two queries at all.
 *
 * Its answer is *data* and it needs a **device**, so `false` before one has
 * opened is ordering rather than failure — and `false` on a browser without
 * `timestamp-query` is that browser's answer, which {@link timestampSupported}
 * is how the caller tells apart.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the timed frame.
 */
export function startTimestampProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_timestamp() === 1;
}

/**
 * Read where the timed pass's read has got to, and its two ticks once it is
 * `READY`.
 *
 * {@link readOcclusionValues}' shape, and the values are handed over as bytes
 * for its reason: a `BigUint64Array` view needs its offset aligned and nothing
 * about a wasm allocation promises one.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, bytes: Uint8Array }}
 */
export function readTimestampProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_timestamp_state() >>> 0;
  const ptr = exports.__crcbl_web_gpu_probe_timestamp_ptr();
  const len = exports.__crcbl_web_gpu_probe_timestamp_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: timestampStateName(state), bytes };
}

/**
 * The same state-name helper again, for the compute codes. Its own function for
 * {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function computeStateName(state) {
  const found = Object.entries(COMPUTE).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to run a compute dispatch that writes a known pattern into a storage
 * buffer and start reading it back on the device it opened.
 *
 * {@link startDrawProbe}'s dispatch sibling: one frame that builds a compute
 * pipeline, binds and dispatches it, copies its storage buffer to a host buffer,
 * submits and requests. {@link pollComputeProbe} drives the poll and
 * {@link readComputeProbe} reads the bytes when they land. Its answer is *data*
 * and it needs a **device**, so `false` before one has opened is ordering rather
 * than failure — wait for {@link readDeviceProbe} to say `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startComputeProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_compute() === 1;
}

/**
 * Poll the dispatch's in-flight readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. See
 * `__crcbl_web_gpu_probe_compute_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollComputeProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_compute_poll() === 1;
}

/**
 * Read where the dispatch readback has got to, and its bytes once it is `READY`.
 *
 * {@link readDrawProbe}'s dispatch sibling, and `state` first for its reason —
 * draining allocates and may detach a view built before it, so the `Uint8Array`
 * is built after the state call and copied out with `slice`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readComputeProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_compute_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_compute_reason_ptr(),
    exports.__crcbl_web_gpu_probe_compute_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_compute_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_compute_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: computeStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the indirect-dispatch codes. Its own
 * function for {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function dispatchIndirectStateName(state) {
  const found = Object.entries(DISPATCH_INDIRECT).find(
    ([, code]) => code === state
  );
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to run a compute dispatch whose **workgroup counts come out of a
 * buffer**, and start reading its tally back on the device it opened.
 *
 * {@link startComputeProbe}'s indirect sibling: the same frame with
 * `dispatchWorkgroups(x, y, z)` replaced by a `queue.writeBuffer` that fills an
 * indirect-args buffer and a `dispatchWorkgroupsIndirect` reading it at a
 * non-zero offset. {@link pollDispatchIndirectProbe} drives the poll and
 * {@link readDispatchIndirectProbe} reads the tally when it lands. Its answer is
 * *data* and it needs a **device**, so `false` before one has opened is ordering
 * rather than failure — wait for {@link readDeviceProbe} to say `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startDispatchIndirectProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_dispatch_indirect() === 1;
}

/**
 * Poll the indirect dispatch's in-flight readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. See
 * `__crcbl_web_gpu_probe_dispatch_indirect_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollDispatchIndirectProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_dispatch_indirect_poll() === 1;
}

/**
 * Read where the indirect dispatch's readback has got to, and its tally once it
 * is `READY`.
 *
 * {@link readComputeProbe}'s indirect sibling, and `state` first for its reason
 * — draining allocates and may detach a view built before it, so the
 * `Uint8Array` is built after the state call and copied out with `slice`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readDispatchIndirectProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_dispatch_indirect_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_dispatch_indirect_reason_ptr(),
    exports.__crcbl_web_gpu_probe_dispatch_indirect_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_dispatch_indirect_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_dispatch_indirect_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: dispatchIndirectStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the copy-chain codes. Its own function
 * for {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function copyChainStateName(state) {
  const found = Object.entries(COPYCHAIN).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to run the copy chain — a dispatch that fills a storage buffer red, a
 * buffer→image copy into a texture, an image→image copy to a second texture, and
 * an image→buffer copy out to a host buffer — and start reading it back on the
 * device it opened.
 *
 * {@link startComputeProbe}'s copy sibling. {@link pollCopyChainProbe} drives the
 * poll and {@link readCopyChainProbe} reads the bytes when they land. Its answer
 * is *data* and it needs a **device**, so `false` before one has opened is
 * ordering rather than failure — wait for {@link readDeviceProbe} to say
 * `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startCopyChainProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_copychain() === 1;
}

/**
 * Poll the copy chain's in-flight readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. See
 * `__crcbl_web_gpu_probe_copychain_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollCopyChainProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_copychain_poll() === 1;
}

/**
 * Read where the copy chain's readback has got to, and its bytes once it is
 * `READY`.
 *
 * {@link readComputeProbe}'s copy sibling, and `state` first for its reason.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readCopyChainProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_copychain_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_copychain_reason_ptr(),
    exports.__crcbl_web_gpu_probe_copychain_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_copychain_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_copychain_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: copyChainStateName(state), reason, bytes };
}

/**
 * The same state-name helper again, for the fill codes. Its own function for
 * {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function fillStateName(state) {
  const found = Object.entries(FILL).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Ask wasm to run the fill probe — a dispatch that fills a storage buffer with a
 * pattern, a zero `fill_buffer` over its first half, and a copy to a host buffer
 * — and start reading it back on the device it opened.
 *
 * {@link startComputeProbe}'s fill sibling. {@link pollFillProbe} drives the poll
 * and {@link readFillProbe} reads the bytes when they land. Its answer is *data*
 * and it needs a **device**, so `false` before one has opened is ordering rather
 * than failure — wait for {@link readDeviceProbe} to say `OPENED`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether wasm encoded the setup frame. `false` is no device
 *   yet, the probe being re-entered, or another channel being installed.
 */
export function startFillProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_fill() === 1;
}

/**
 * Poll the fill probe's in-flight readback once.
 *
 * A no-op — `false` — while a previous poll is unanswered or the bytes are
 * already in, so the gate can call it every frame. See
 * `__crcbl_web_gpu_probe_fill_poll`.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean} Whether a poll was encoded this frame.
 */
export function pollFillProbe({ exports }) {
  return exports.__crcbl_web_gpu_probe_fill_poll() === 1;
}

/**
 * Read where the fill probe's readback has got to, and its bytes once it is
 * `READY`.
 *
 * {@link readComputeProbe}'s fill sibling, and `state` first for its reason.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, reason: string, bytes: Uint8Array }}
 *   `reason` is the browser's own words under `FAILED`, the decode error
 *   under `UNDECODABLE`, and empty otherwise.
 */
export function readFillProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_fill_state() >>> 0;
  const reason = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_fill_reason_ptr(),
    exports.__crcbl_web_gpu_probe_fill_reason_len()
  );
  const ptr = exports.__crcbl_web_gpu_probe_fill_bytes_ptr();
  const len = exports.__crcbl_web_gpu_probe_fill_bytes_len() >>> 0;
  const bytes =
    len === 0
      ? new Uint8Array(0)
      : new Uint8Array(memory.buffer, ptr, len).slice();
  return { state, name: fillStateName(state), reason, bytes };
}

/**
 * The `PARITY_*` codes `__crcbl_web_gpu_probe_parity` answers, from
 * `crates/crcbl-webgpu/src/probe.rs`.
 *
 * `NO_DEVICE` is ordering rather than failure — there is no `DeviceCaps` to
 * build a `WebGpuDevice` around until the device request has come back, so wait
 * for {@link readDeviceProbe} to say `OPENED`. `MATCHED` and `MISMATCHED` are
 * the verdict, and `MISMATCHED` always comes with `failures`.
 */
export const PARITY = Object.freeze({
  UNASKED: 0,
  NO_DEVICE: 1,
  MATCHED: 2,
  MISMATCHED: 3,
});

/**
 * The same state-name helper again, for the parity codes. Its own function for
 * {@link readbackStateName}'s reason.
 *
 * @param {number} state
 * @returns {string}
 */
export function parityStateName(state) {
  const found = Object.entries(PARITY).find(([, code]) => code === state);
  return found ? found[0] : `unknown(${state})`;
}

/**
 * Run the parity report: walk a real `WebGpuDevice`'s whole `supports()` matrix
 * on the device the browser opened, and hold every answer against
 * `crcbl_hal::DIVERGENCES`.
 *
 * THE ONE PROBE THAT ASKS THE BROWSER NOTHING, and the reason it needs no
 * `start`/`poll`/`read` trio. Every other probe here puts a command on the
 * stream and waits for the page's loop to replay it; this one compares two
 * things wasm already holds — the caps that came back with the device, and a
 * `const` list — so it is one call, and its answer is ready when it returns. It
 * neither drains nor encodes, so calling it cannot take a reply another probe
 * was waiting on, and nothing queued ahead of it can strand it.
 *
 * `checked` and `held` are the vacuity guard: a report that walked no
 * capabilities agrees with every list there is, and one where every capability
 * was left unprovable by a device that withheld its gating feature settled
 * nothing. The gate asserts both against the matrix rather than trusting
 * `MATCHED`.
 *
 * The call allocates, so the four views over wasm memory are built after it.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ state: number, name: string, checked: number, held: number,
 *   report: string, failures: string }} `report` is one `Capability=verdict`
 *   token per capability, space separated; `failures` is one disagreement per
 *   line and empty when `state` is `MATCHED`.
 */
export function runParityProbe({ exports, memory }) {
  const state = exports.__crcbl_web_gpu_probe_parity() >>> 0;
  const checked = exports.__crcbl_web_gpu_probe_parity_checked() >>> 0;
  const held = exports.__crcbl_web_gpu_probe_parity_held() >>> 0;
  const report = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_parity_report_ptr(),
    exports.__crcbl_web_gpu_probe_parity_report_len() >>> 0
  );
  const failures = readUtf8(
    memory,
    exports.__crcbl_web_gpu_probe_parity_failures_ptr(),
    exports.__crcbl_web_gpu_probe_parity_failures_len() >>> 0
  );
  return {
    state,
    name: parityStateName(state),
    checked,
    held,
    report,
    failures,
  };
}
