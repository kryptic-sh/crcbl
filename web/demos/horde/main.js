// Horde in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_horde_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.

import init from './crcbl_horde.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'WASD moves · the gun aims itself · 1/2/3 pick an upgrade · R restarts · ESC pauses · F11 fullscreen',
  savedLabel: 'Longest run',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_horde_prepare(),
    boot: () => ex.__crcbl_horde_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_horde_frame(now),
    status: () => ex.__crcbl_horde_status(),
    shutdown: () => ex.__crcbl_horde_shutdown(),
    logLevel: (/** @type {number} */ level) => ex.__crcbl_horde_log_level(level),
    logTake: ex.__crcbl_horde_log_take,
    logPtr: ex.__crcbl_horde_log_ptr,
    errorPtr: () => ex.__crcbl_horde_error_ptr(),
    errorLen: () => ex.__crcbl_horde_error_len(),
  }),
});
