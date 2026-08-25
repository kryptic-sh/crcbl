// puppet in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_puppet_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` leads with the walk keys because the character is already walking by
// the time anyone reads it: it paces a circuit on the spawn pad from the first
// tick, and the first movement key takes it over for good.
//
// `savedLabel` is "Nothing" and that is literal: the status bar says "Nothing
// saved." when the demo stops, which is the truth about a walk with no score
// and no save file.

import init from './crcbl_puppet.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'W/A/S/D or the arrows walk, relative to where the camera is looking · Q/E swing the camera, R/F raise and lower it · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_puppet_prepare(),
    boot: () => ex.__crcbl_puppet_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_puppet_frame(now),
    status: () => ex.__crcbl_puppet_status(),
    shutdown: () => ex.__crcbl_puppet_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_puppet_log_level(level),
    logTake: ex.__crcbl_puppet_log_take,
    logPtr: ex.__crcbl_puppet_log_ptr,
    errorPtr: () => ex.__crcbl_puppet_error_ptr(),
    errorLen: () => ex.__crcbl_puppet_error_len(),
  }),
});
