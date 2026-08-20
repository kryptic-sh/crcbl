// quarry in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_quarry_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` names the camera row first because the page is already moving when it
// opens: `apps/quarry/src/web.rs` boots on the animated dolly, so a visitor who
// wants the goldens' pose or the free camera reaches them by cycling that row
// rather than by pressing a key. The free camera's own keys are listed after it
// for the same reason lumen's are — `apps/quarry` integrates the flyer whether
// or not it is being drawn from, so W before the swap moves a camera nobody is
// looking through. `savedLabel` is "Nothing" and that is literal: the status bar
// says "Nothing saved." when the demo stops, which is the truth about a geometry
// fixture with no score and no save file.

import init from './crcbl_quarry.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'ESC opens the panel — CAMERA cycles the dolly, the free camera and the fixed golden pose · LOD VIEW tints each cluster by its level · then WASD, Space/Shift and the arrows fly it · F3 shows the panel · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_quarry_prepare(),
    boot: () => ex.__crcbl_quarry_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_quarry_frame(now),
    status: () => ex.__crcbl_quarry_status(),
    shutdown: () => ex.__crcbl_quarry_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_quarry_log_level(level),
    logTake: ex.__crcbl_quarry_log_take,
    logPtr: ex.__crcbl_quarry_log_ptr,
    errorPtr: () => ex.__crcbl_quarry_error_ptr(),
    errorLen: () => ex.__crcbl_quarry_error_len(),
  }),
});
