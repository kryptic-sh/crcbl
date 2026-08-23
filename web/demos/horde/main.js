// Horde in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_horde_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// THE THREE SYMBOLS NO OTHER DEMO HAS. Horde is the sample that runs a pass on
// the job pool — `steer_enemies`, split across `crcbl_jobs::Pool::par_for` — so
// it is the only one with anything to say about threads, and the only one with a
// crowd worth staging:
//
//   __crcbl_horde_prefill      `--prefill`, from `?prefill=N`. Before boot.
//   __crcbl_horde_sim_threads  distinct threads that have run a steering chunk
//   __crcbl_horde_sim_workers  workers on the pool that last ran the pass
//
// The two counters are read through `globalThis.crcblHordeSim`, which is what
// `web/tools/horde-threads-e2e.mjs` asks. They are *here* rather than in the
// driver so that the names go through the export check above with everything
// else: a driver that spelled one wrong would fail in a browser, and only when
// somebody ran it.

import init from './crcbl_horde.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'WASD moves · the gun aims itself · 1/2/3 pick an upgrade · R restarts · ESC pauses · F11 fullscreen',
  savedLabel: 'Longest run',
  bind: (ex) => {
    // Before `bootDemo` calls `prepare` or `boot`, which is the window this
    // has: the count is read once, when the game takes its options. See
    // `apps/horde/src/web.rs`.
    const prefill = requestedPrefill();
    if (prefill > 0 && ex.__crcbl_horde_prefill(prefill) === 0) {
      console.warn(`crcbl: ?prefill=${prefill} arrived after the game opened`);
    }

    // A console readout rather than a HUD, in the shape `crcbl.audio()` and
    // `crcbl.saves()` already have in `web/engine/demo.js`.
    globalThis.crcblHordeSim = () => ({
      threads: ex.__crcbl_horde_sim_threads(),
      workers: ex.__crcbl_horde_sim_workers(),
    });

    return {
      prepare: () => ex.__crcbl_horde_prepare(),
      boot: () => ex.__crcbl_horde_boot(),
      frame: (/** @type {number} */ now) => ex.__crcbl_horde_frame(now),
      status: () => ex.__crcbl_horde_status(),
      shutdown: () => ex.__crcbl_horde_shutdown(),
      logLevel: (/** @type {number} */ level) =>
        ex.__crcbl_horde_log_level(level),
      logTake: ex.__crcbl_horde_log_take,
      logPtr: ex.__crcbl_horde_log_ptr,
      errorPtr: () => ex.__crcbl_horde_error_ptr(),
      errorLen: () => ex.__crcbl_horde_error_len(),
    };
  },
});

/**
 * How many enemies `?prefill=N` asked to stage, or none.
 *
 * The browser's half of the flag the scale measurement runs on. Zero — the
 * default, and what a visitor who did not ask gets — leaves the demo waiting at
 * its title screen with an empty arena, exactly as it does without this.
 *
 * @returns {number}
 */
function requestedPrefill() {
  const asked = new URLSearchParams(location.search).get('prefill');
  if (asked === null) return 0;
  const count = Number(asked);
  if (!Number.isInteger(count) || count < 0) {
    throw new Error(`crcbl: ?prefill=${asked} is not an enemy count`);
  }
  return count;
}
