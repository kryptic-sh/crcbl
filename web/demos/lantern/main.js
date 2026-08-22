// lantern in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_lantern_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` names the free camera's keys, and it says to swap camera first because
// that is the truth: the page opens on the fixed pose the goldens are taken
// from, and `apps/lantern` integrates the free camera whether or not it is the one
// being drawn from — so a visitor pressing W before the swap moves a camera they
// are not looking through. `savedLabel` is "Nothing" and that is literal: the
// status bar says "Nothing saved." when the demo stops, which is the truth about
// a lighting fixture with no score and no save file.

import init from './crcbl_lantern.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'ESC opens the panel — CAMERA swaps to the free one · then WASD, Space/Shift and the arrows fly it · F3 shows the panel · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_lantern_prepare(),
    boot: () => ex.__crcbl_lantern_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_lantern_frame(now),
    status: () => ex.__crcbl_lantern_status(),
    shutdown: () => ex.__crcbl_lantern_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_lantern_log_level(level),
    logTake: ex.__crcbl_lantern_log_take,
    logPtr: ex.__crcbl_lantern_log_ptr,
    errorPtr: () => ex.__crcbl_lantern_error_ptr(),
    errorLen: () => ex.__crcbl_lantern_error_len(),
  }),
});
