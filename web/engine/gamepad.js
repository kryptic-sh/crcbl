// The page's half of the Web Gamepad API backend: read `navigator.getGamepads()`
// once a frame and write every pad into the buffers wasm owns.
//
// The API is poll-based — nothing fires when a stick moves — and the engine's
// wasm imports nothing, so it cannot read the pads itself. The report layout,
// the flags and what the engine does with a frame are specified in
// `crates/crcbl-input/src/web_gamepad/shim.rs`; the mapping is that module's
// parent's. This file only copies.
//
// A browser hides every pad until one of its buttons is pressed on the page, so
// an empty list here on the first frames is the normal case, not a failure.

import { writeUtf8 } from './wasm.js';

/** Mirrors `PAD_CONNECTED` in `web_gamepad/shim.rs`. */
const PAD_CONNECTED = 1 << 0;
/** Mirrors `PAD_STANDARD`. */
const PAD_STANDARD = 1 << 1;
/** The standard mapping's four axes, which follow its buttons in a report —
 * `STANDARD_AXES` in `web_gamepad/map.rs`. */
const STANDARD_AXES = 4;

/** Whether a `getGamepads()` refusal has been said, so it is said once. */
let refused = false;

/**
 * Reports every pad the browser exposes, as one frame. Call once per
 * `requestAnimationFrame`, before the demo's frame.
 *
 * Reports nothing at all — not an empty frame — where the API is missing or
 * refuses (an insecure context, a `gamepad` permissions policy): the engine
 * then changes nothing, which is what "this page cannot see pads" should do.
 *
 * @param {object} options
 * @param {Record<string, any>} options.exports
 * @param {WebAssembly.Memory} options.memory
 */
export function pumpGamepads({ exports, memory }) {
  if (refused || typeof navigator.getGamepads !== 'function') return;
  let pads;
  try {
    pads = navigator.getGamepads();
  } catch (error) {
    refused = true;
    console.warn(
      `crcbl: navigator.getGamepads() refused, so no pads: ${error}`
    );
    return;
  }

  exports.__crcbl_web_pad_begin();
  const valueCount = exports.__crcbl_web_pad_values_capacity();
  const buttonCount = valueCount - STANDARD_AXES;
  const idCapacity = exports.__crcbl_web_pad_id_capacity();
  for (const pad of pads) {
    if (!pad) continue;
    // Fresh views for every pad: a `memory.grow()` between two frames — or
    // inside the report call — detaches the old ones.
    const values = new Float32Array(
      memory.buffer,
      exports.__crcbl_web_pad_values_ptr(),
      valueCount
    );
    let pressed = 0;
    for (let i = 0; i < buttonCount; i += 1) {
      const button = pad.buttons[i];
      values[i] = button ? button.value : 0;
      if (button?.pressed) pressed |= 1 << i;
    }
    for (let i = 0; i < STANDARD_AXES; i += 1) {
      values[buttonCount + i] = pad.axes[i] ?? 0;
    }
    // An id too long for the buffer goes across empty rather than cut: it only
    // names the pad's family, and a pad of no known family is still a pad.
    const written =
      writeUtf8(
        memory,
        exports.__crcbl_web_pad_id_ptr(),
        idCapacity,
        pad.id ?? ''
      ) ?? 0;
    const flags =
      (pad.connected ? PAD_CONNECTED : 0) |
      (pad.mapping === 'standard' ? PAD_STANDARD : 0);
    exports.__crcbl_web_pad(pad.index, flags, pressed >>> 0, written);
  }
}
