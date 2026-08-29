// options in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_options_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` names the arrows and the commit key because the panel is the whole
// demo — it is never dismissed, so there is nothing else to reach. `savedLabel`
// is "Settings", and unlike most demos here it is not a formality: this is the
// sample whose entire point is that the file outlives the tab, and the line the
// status bar prints when it stops is the last thing a visitor sees about it.
import init from './crcbl_options.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'up/down picks a row, left/right moves a fader or RENDER SCALE · ENTER steps FRAME CAP or ANISOTROPY and presses SAVE or RESET · F3 shows the panel · F11 fullscreen',
  savedLabel: 'Settings',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_options_prepare(),
    boot: () => ex.__crcbl_options_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_options_frame(now),
    status: () => ex.__crcbl_options_status(),
    shutdown: () => ex.__crcbl_options_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_options_log_level(level),
    logTake: ex.__crcbl_options_log_take,
    logPtr: ex.__crcbl_options_log_ptr,
    errorPtr: () => ex.__crcbl_options_error_ptr(),
    errorLen: () => ex.__crcbl_options_error_len(),
  }),
});
