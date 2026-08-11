// hud in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_hud_*` symbols, and the two strings the status
// bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` names no key that does anything to the page, because none does: hud
// takes no input at all and the three keys listed are the engine loop's, the
// same three every sample answers to. `savedLabel` is "Nothing" and that is
// literal — the status bar says "Nothing saved." when the demo stops, which is
// the truth about a sample with no score and no save file.

import init from './crcbl_hud.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'nothing to press — the ticker drives it · ESC pauses · F3 shows the panel · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_hud_prepare(),
    boot: () => ex.__crcbl_hud_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_hud_frame(now),
    status: () => ex.__crcbl_hud_status(),
    shutdown: () => ex.__crcbl_hud_shutdown(),
    logLevel: (/** @type {number} */ level) => ex.__crcbl_hud_log_level(level),
    logTake: ex.__crcbl_hud_log_take,
    logPtr: ex.__crcbl_hud_log_ptr,
    errorPtr: () => ex.__crcbl_hud_error_ptr(),
    errorLen: () => ex.__crcbl_hud_error_len(),
  }),
});
