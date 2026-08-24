// bracket in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_bracket_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` says what to watch rather than what to press, because there is very
// little to press: the population queues, matches and re-rates itself from the
// moment the page loads, and the interesting thing on screen is a curve coming
// down rather than anything a visitor does.
//
// `savedLabel` is "Nothing" and that is literal: the status bar says "Nothing
// saved." when the demo stops, which is the truth about a ladder that starts
// from its seed every time.

import init from './crcbl_bracket.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'It runs itself · watch CONVERGENCE fall as the ladder learns who is actually good · the mark on each bar is that player’s true skill, which the matchmaker never sees · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_bracket_prepare(),
    boot: () => ex.__crcbl_bracket_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_bracket_frame(now),
    status: () => ex.__crcbl_bracket_status(),
    shutdown: () => ex.__crcbl_bracket_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_bracket_log_level(level),
    logTake: ex.__crcbl_bracket_log_take,
    logPtr: ex.__crcbl_bracket_log_ptr,
    errorPtr: () => ex.__crcbl_bracket_error_ptr(),
    errorLen: () => ex.__crcbl_bracket_error_len(),
  }),
});
