// shard in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_shard_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// There is no extra symbol here beyond the ten every demo has. breach has an
// eleventh because it ships two maps for `?map=` to choose between; shard's
// milestone 1 is one zone, so a query string has nothing to pick and the shim
// has nothing to validate.
//
// `hint` leads with the walk keys because the page opens standing in the zone
// with the torches already lit, so the first useful thing is to move. The blow
// comes next, because the zone has three things in it that will not wait
// forever. L is listed after them and named for what it does rather than for the
// key, because dousing the torches is the one control that changes the *picture*
// rather than the position — it is what the browser gate presses to prove the
// lighting is being computed rather than declared. `savedLabel` is "Nothing" and
// that is literal: the status bar says "Nothing saved." when the demo stops,
// which is the truth about a slice with no persistence —
// `docs/plan/sample/15-shard.md` puts saves in a later slice, and
// `docs/backlog.md` carries what they need.

import init from './crcbl_shard.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'W/A/S/D walk, relative to where the camera is looking · Q/E swing it a quarter turn · SPACE strikes everything in reach · L douses the torches and lights them again · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_shard_prepare(),
    boot: () => ex.__crcbl_shard_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_shard_frame(now),
    status: () => ex.__crcbl_shard_status(),
    shutdown: () => ex.__crcbl_shard_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_shard_log_level(level),
    logTake: ex.__crcbl_shard_log_take,
    logPtr: ex.__crcbl_shard_log_ptr,
    errorPtr: () => ex.__crcbl_shard_error_ptr(),
    errorLen: () => ex.__crcbl_shard_error_len(),
  }),
});
