// orbit in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_orbit_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` leads with the throttle because the rocket is already flying by the
// time anyone reads it: a script flies the ascent from the moment the page
// loads, and the first key a visitor presses takes it away from them for good.
//
// `savedLabel` is "Nothing" and that is literal: the status bar says "Nothing
// saved." when the demo stops, which is the truth about a flight with no score
// and no save file.

import init from './crcbl_orbit.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'W/S throttle · A/D turn · , and . step the timewarp — it drops to ×1 on a burn or in the air · SPACE launches, and restarts after a landing · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_orbit_prepare(),
    boot: () => ex.__crcbl_orbit_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_orbit_frame(now),
    status: () => ex.__crcbl_orbit_status(),
    shutdown: () => ex.__crcbl_orbit_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_orbit_log_level(level),
    logTake: ex.__crcbl_orbit_log_take,
    logPtr: ex.__crcbl_orbit_log_ptr,
    errorPtr: () => ex.__crcbl_orbit_error_ptr(),
    errorLen: () => ex.__crcbl_orbit_error_len(),
  }),
});
