// The page's half of the adapter probe: the round trip, driven end to end.
//
// `crates/crcbl-webgpu/src/probe.rs` is the contract — four exports, the state
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
// THE ONE ORDERING RULE THAT MATTERS, and the reason `start` is one function
// rather than two calls a caller makes in order. The demo's frame loop calls
// `takeCommandStream` on every frame and drops what it decodes, because nothing
// encodes into the stream yet. The instant the probe installs a channel and
// writes a command, that loop becomes a competitor: whichever of the two drains
// the buffer first gets the command, and the other gets an empty frame. So the
// request and the drain happen in one synchronous run with no `await` between
// them — a `requestAnimationFrame` callback cannot interleave with synchronous
// JavaScript, so being uninterruptible is the whole guard.

import { putReplyStream, takeCommandStream } from './gpu-transport.js';
import { readUtf8 } from './wasm.js';

/** @typedef {import('./gpu-replay.js').Replayer} Replayer */

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
 * Asks wasm to enumerate adapters, and replays the frame that carries the ask.
 *
 * Synchronous from the request to the replay, for the reason in the header. The
 * WebGPU call the replay starts is not: it is a promise, and its answer is
 * queued into `replayer` whenever it settles, which is why this returns nothing
 * about the adapter and {@link pumpAdapterProbe} exists.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @param {Replayer} options.replayer
 * @returns {{ started: boolean, commands: string[] }} `started` is false when
 *   wasm would not take the request — a channel already installed by something
 *   else, or a full waiting set — and `commands` names what the frame actually
 *   carried, which is what says the command crossed rather than that it was
 *   written.
 */
export function startAdapterProbe({ exports, memory, replayer }) {
  if (exports.__crcbl_web_gpu_probe_adapters() !== 1) {
    return { started: false, commands: [] };
  }
  // NO `await` BETWEEN THESE TWO. See the header.
  const frame = takeCommandStream({ exports, memory });
  const commands = (frame?.commands ?? []).map((command) =>
    String(command.name)
  );
  replayer.replay(frame);
  return { started: true, commands };
}

/**
 * Asks wasm to open the adapter it was granted, and replays the frame that
 * carries the ask.
 *
 * The device half of {@link startAdapterProbe} in every respect, including the
 * one that matters: **no `await` between the request and the drain**, because
 * the demo's frame loop is draining the same buffer.
 *
 * The enumeration has to have been answered first — the descriptor names an
 * adapter id from one — so a `false` here right after
 * {@link startAdapterProbe} is that ordering rather than a failure.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @param {Replayer} options.replayer
 * @returns {{ started: boolean, commands: string[] }}
 */
export function startDeviceProbe({ exports, memory, replayer }) {
  if (exports.__crcbl_web_gpu_probe_device() !== 1) {
    return { started: false, commands: [] };
  }
  // NO `await` BETWEEN THESE TWO. See the header.
  const frame = takeCommandStream({ exports, memory });
  const commands = (frame?.commands ?? []).map((command) =>
    String(command.name)
  );
  replayer.replay(frame);
  return { started: true, commands };
}

/**
 * Hands wasm whatever the replayer has answered, and reads where the probe has
 * got to.
 *
 * Call it once a frame until the state settles. `WAITING` is the ordinary answer
 * on the frames before the browser has resolved its promise, and it is not a
 * failure — it is the whole shape of this seam.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @param {Replayer} options.replayer
 * @returns {{ state: number, name: string, text: string, delivered: boolean,
 *            caps: { featuresLo: number, featuresHi: number, maxImage2d: number } }}
 *   `text` is the adapter's name under `GRANTED`, the reason under `REFUSED`,
 *   and the decode error under `UNDECODABLE`. `caps` is the part of the granted
 *   adapter's `DeviceCaps` the probe exports — see below — and is all zeros
 *   under every state but `GRANTED`, where `0` is also a legal value, so read it
 *   only once the state says so.
 */
export function pumpAdapterProbe({ exports, memory, replayer }) {
  let delivered = false;
  if (replayer.hasReplies) {
    // Cleared only once wasm has actually taken them. A `false` is "not now",
    // and the same buffer is offered again next frame; dropping it would leave
    // a command waiting for ever.
    delivered = putReplyStream({ exports, memory, bytes: replayer.replies });
    if (delivered) replayer.clear();
  }

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
  return { state, name: probeStateName(state), text, delivered, caps };
}

/**
 * The same for the device request: deliver, then read where it has got to.
 *
 * **Either pump drains for both probes** — there is one channel and one
 * committed buffer, so whichever runs first decodes it and hands each probe its
 * own answer. Calling only this one is therefore enough to settle an
 * enumeration too, and a frame that called neither leaves both waiting.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @param {Replayer} options.replayer
 * @returns {{ state: number, name: string, reason: string, delivered: boolean,
 *            caps: { featuresLo: number, featuresHi: number, maxImage2d: number } }}
 *   `reason` says why no device opened and is empty when one did. `caps` is the
 *   **device's** — not the adapter's — and is all zeros under every state but
 *   `OPENED`, where `0` is also a legal value, so read it only once the state
 *   says so.
 */
export function pumpDeviceProbe({ exports, memory, replayer }) {
  let delivered = false;
  if (replayer.hasReplies) {
    delivered = putReplyStream({ exports, memory, bytes: replayer.replies });
    if (delivered) replayer.clear();
  }

  // `state` first, `ptr` second, for `pumpAdapterProbe`'s reason: this is the
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
  return { state, name: deviceStateName(state), reason, delivered, caps };
}
