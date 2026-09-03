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
//
// THE ONE SYMBOL NO OTHER DEMO HAS. breach is the sample with two maps —
// `docs/plan/sample/11-breach.md`'s milestone 0 is a firing range *and* a bot
// practice map — so it is the only one with anything for a query string to
// choose:
//
//   __crcbl_breach_map   `--map`, from `?map=NAME`. Before boot.

import init from './crcbl_breach.js';
import { bootDemo } from '../../engine/demo.js';

/**
 * The maps `?map=` answers to, in the order `MapChoice::ALL` declares them.
 *
 * The export takes an index rather than a string, because a wasm export takes
 * numbers; this array is the JS half of that table and the order is a
 * deliberate coupling to `apps/breach/src/map.rs`. A name that is not here is
 * refused below rather than silently opening the range, and so is an index the
 * Rust side does not recognise — see `apps/breach/src/web.rs`.
 */
const MAPS = ['range', 'practice'];

bootDemo({
  init,
  hint: 'W/A/S/D walk, relative to where you are looking · click to take the pointer, then the mouse looks · the arrows look too · SPACE fires · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => {
    // Before `bootDemo` calls `prepare` or `boot`, which is the window this
    // has: the choice is read once, when the game takes its options. See
    // `apps/breach/src/web.rs`.
    const map = requestedMap();
    if (map > 0 && ex.__crcbl_breach_map(map) === 0) {
      console.warn(`crcbl: ?map=${MAPS[map]} arrived after the game opened`);
    }

    return {
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
    };
  },
});

/**
 * Which map `?map=NAME` asked for, as an index into `MAPS`.
 *
 * Zero — the default, and what a visitor who did not ask gets — is the firing
 * range, which is what `/demos/breach/` has always opened on.
 *
 * @returns {number}
 */
function requestedMap() {
  const asked = new URLSearchParams(location.search).get('map');
  if (asked === null) return 0;
  const index = MAPS.indexOf(asked);
  if (index < 0) {
    throw new Error(`crcbl: ?map=${asked} is not one of ${MAPS.join(', ')}`);
  }
  return index;
}
