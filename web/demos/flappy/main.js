// Flappy in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_flappy_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.

import init from './crcbl_flappy.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'SPACE or ↑ to flap · ESC pauses · F11 fullscreen',
  savedLabel: 'Best score',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_flappy_prepare(),
    boot: () => ex.__crcbl_flappy_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_flappy_frame(now),
    status: () => ex.__crcbl_flappy_status(),
    shutdown: () => ex.__crcbl_flappy_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_flappy_log_level(level),
    logTake: ex.__crcbl_flappy_log_take,
    logPtr: ex.__crcbl_flappy_log_ptr,
    errorPtr: () => ex.__crcbl_flappy_error_ptr(),
    errorLen: () => ex.__crcbl_flappy_error_len(),
  }),
});
