// The sparks demo's browser shim.
//
// Everything shared with the other demos — boot order, the log drain, the
// canvas, the status bar — is `web/engine/demo.js`. What is here is this
// sample's ten export names and the one line of hint text under its canvas.
//
// **The names are written out literally.** `web/tools/check-exports.mjs` scans
// this shim for `.__crcbl_…` to learn which exports the JS depends on, and then
// checks the built `.wasm` actually has them; a template literal would hide
// every one of them from it.
import init from './crcbl_sparks.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'It runs itself — sparks off the anvil, smoke coming and going at the vent, and a deliberately greedy effect held at its share · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_sparks_prepare(),
    boot: () => ex.__crcbl_sparks_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_sparks_frame(now),
    status: () => ex.__crcbl_sparks_status(),
    shutdown: () => ex.__crcbl_sparks_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_sparks_log_level(level),
    logTake: ex.__crcbl_sparks_log_take,
    logPtr: ex.__crcbl_sparks_log_ptr,
    errorPtr: () => ex.__crcbl_sparks_error_ptr(),
    errorLen: () => ex.__crcbl_sparks_error_len(),
  }),
});
