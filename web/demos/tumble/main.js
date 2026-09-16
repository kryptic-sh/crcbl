// The tumble demo's browser shim.
//
// Everything shared with the other demos — boot order, the log drain, the
// canvas, the status bar — is `web/engine/demo.js`. What is here is this
// sample's ten export names and the one line of hint text under its canvas.
//
// **The names are written out literally.** `web/tools/check-exports.mjs` scans
// this shim for `.__crcbl_…` to learn which exports the JS depends on, and then
// checks the built `.wasm` actually has them; a template literal would hide
// every one of them from it.
import init from './crcbl_tumble.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'It runs itself — a T-handle flipping in zero g, and a box dropped flat that falls through the floor because contacts are the next rung · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_tumble_prepare(),
    boot: () => ex.__crcbl_tumble_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_tumble_frame(now),
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
