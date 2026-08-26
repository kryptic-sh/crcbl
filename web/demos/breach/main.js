// breach in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_breach_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` leads with the arrows rather than with the mouse, and that is not a
// preference. The web shell reports no `RAW_POINTER_MOTION`, because
// `movementX`/`movementY` under Pointer Lock are accelerated and clamped by the
// same OS layer the capability exists to bypass — so the engine declines the
// lock here and there is no mouselook on this page at all.
// `docs/plan/sample/11-breach.md` names that as one of the four reasons the
// competitive game is native only.
//
// `savedLabel` is "Nothing" and that is literal: the status bar says "Nothing
// saved." when the demo stops, which is the truth about a range that keeps no
// score between visits.

import init from './crcbl_breach.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'W/A/S/D walk, relative to where you are looking · the arrows look — a browser reports no raw mouse motion, so there is no mouselook here · SPACE fires · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_breach_prepare(),
    boot: () => ex.__crcbl_breach_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_breach_frame(now),
    status: () => ex.__crcbl_breach_status(),
    shutdown: () => ex.__crcbl_breach_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_breach_log_level(level),
    logTake: ex.__crcbl_breach_log_take,
    logPtr: ex.__crcbl_breach_log_ptr,
    errorPtr: () => ex.__crcbl_breach_error_ptr(),
    errorLen: () => ex.__crcbl_breach_error_len(),
  }),
});
