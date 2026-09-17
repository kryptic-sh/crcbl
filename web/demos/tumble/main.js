// The tumble demo's browser shim.
//
// Everything shared with the other demos — boot order, the log drain, the
// canvas, the status bar — is `web/engine/demo.js`. What is here is this
// sample's ten export names, the one line of hint text under its canvas, and a
// clock around the frame call.
//
// **The names are written out literally.** `web/tools/check-exports.mjs` scans
// this shim for `.__crcbl_…` to learn which exports the JS depends on, and then
// checks the built `.wasm` actually has them; a template literal would hide
// every one of them from it.
//
// **Why the clock is out here.** The wasm module reads no clock — its step
// times say "no clock" — because `std::time::Instant` panics on wasm32 and a
// sample's module imports nothing of its own. So the browser's figure is taken
// around the whole frame call: every room's physics for the ticks that frame
// ran, and the frame's drawing, which is an upper bound on the physics alone.
// It is logged as a `[TIMING]` line every `TIMING_FRAMES` frames.
import init from './crcbl_tumble.js';
import { bootDemo } from '../../engine/demo.js';

/** How many frame calls each `[TIMING]` line summarises. */
const TIMING_FRAMES = 120;

let timed = 0;
let totalMs = 0;
let worstMs = 0;

/**
 * Times one frame call.
 * @param {() => number} call
 * @returns {number}
 */
function timeFrame(call) {
  const start = performance.now();
  const status = call();
  const spent = performance.now() - start;
  timed += 1;
  totalMs += spent;
  worstMs = Math.max(worstMs, spent);
  if (timed === TIMING_FRAMES) {
    console.log(
      `[TIMING] frame-call-ms mean: ${(totalMs / timed).toFixed(3)}  ` +
        `worst: ${worstMs.toFixed(3)}  frames: ${timed}`
    );
    timed = 0;
    totalMs = 0;
    worstMs = 0;
  }
  return status;
}

bootDemo({
  init,
  hint: 'It runs itself — three rooms: 1 a T-handle flipping in zero g and a box landing flat, 2 balls and pills bouncing down an obstacle wall, 3 a thousand balls poured into a pit · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_tumble_prepare(),
    boot: () => ex.__crcbl_tumble_boot(),
    frame: (/** @type {number} */ now) =>
      timeFrame(() => ex.__crcbl_tumble_frame(now)),
    status: () => ex.__crcbl_tumble_status(),
    shutdown: () => ex.__crcbl_tumble_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_tumble_log_level(level),
    logTake: ex.__crcbl_tumble_log_take,
    logPtr: ex.__crcbl_tumble_log_ptr,
    errorPtr: () => ex.__crcbl_tumble_error_ptr(),
    errorLen: () => ex.__crcbl_tumble_error_len(),
  }),
});
