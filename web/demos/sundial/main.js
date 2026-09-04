// sundial in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_sundial_*` symbols, the two strings the status
// bar shows, and the shadow knobs — which are one sample's controls and have no
// business in a file every demo runs.
//
// The knobs reach three different kinds of state and `apps/sundial/src/web.rs`
// is where that table is: the filter and the seam are `r_shadow_*` console
// cells, the sun is the fixture's own clock, and the atlas viewer and the
// cascade tint are the engine's `r_debug_view` — one cell for both of them, so
// each of those two buttons takes the other's picture down.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` names the page's own knobs first, because they are what this fixture
// is for and what a visitor with no keyboard can reach; the free camera comes
// second, and it says to swap camera first because that is the truth — the page
// opens on the fixed pose the goldens are taken from, and `apps/sundial`
// integrates the free camera whether or not it is the one being drawn from.
// `savedLabel` is "Nothing" and that is literal: the status bar says "Nothing
// saved." when the demo stops, which is the truth about a shadow fixture with
// no score and no save file.

import init from './crcbl_sundial.js';
import { bootDemo, STATUS } from '../../engine/demo.js';
import { readUtf8 } from '../../engine/wasm.js';

/**
 * Wires the page's shadow controls to the sample's own exports.
 *
 * **Called from `bind`, which is the one place this page is handed its own wasm
 * instance.** `bootDemo` has no per-demo hook and should not grow one for a
 * single sample — `web/demos/alcove/main.js` and `web/demos/viewer/main.js` say
 * the same thing about their own, for the same reason.
 *
 * Nothing here keeps a copy of a knob. Every control is written *and then read
 * back* through `apps/sundial/src/web.rs`, whose answer is what the engine holds
 * after the write — so a slider the engine clamped shows where it landed, and a
 * value moved by a key, by the pause panel or by a typed console line is picked
 * up the next time anything on this page refreshes.
 *
 * **The sun's two controls are the ones that are not instant.** The filter and
 * the seam are console cells and move at once; a tick and a run flag live on the
 * fixture's own clock and are adopted on its next fixed step, so what the sun's
 * exports answer with is the request — which is where the clock is about to be,
 * and what a slider has to show while it is being dragged.
 *
 * @param {Record<string, any>} ex the instance's raw exports
 */
function installKnobs(ex) {
  const memory = /** @type {WebAssembly.Memory} */ (ex.memory);

  /** @param {string} id */
  const el = (id) => /** @type {HTMLElement} */ (document.getElementById(id));
  /** @param {string} id */
  const button = (id) => /** @type {HTMLButtonElement} */ (el(id));
  /** @param {string} id */
  const slider = (id) => /** @type {HTMLInputElement} */ (el(id));

  const filter = button('knob-filter');
  const seam = button('knob-seam');
  const atlas = button('knob-atlas');
  const cascades = button('knob-cascades');
  const sun = button('knob-sun');
  const reset = button('knob-reset');
  const seamAt = slider('knob-seam-at');
  const sunTick = slider('knob-sun-tick');
  const seamValue = el('knob-seam-value');
  const sunValue = el('knob-sun-value');

  const controls = [filter, seam, atlas, cascades, sun, reset, seamAt, sunTick];

  // The arc the tick slider spans, off the engine's own `sun::SWEEP_TICKS`
  // rather than a number written into the markup — `web/pages/sundial.html`
  // gives the slider no `max` at all, and this is what sets it. A sweep read
  // once: it is a constant of the fixture, not a knob.
  const sweep = ex.__crcbl_sundial_sun_sweep();
  sunTick.max = String(sweep - 1);

  /**
   * The filter's own name, out of the engine's variable.
   *
   * The length and the address are one read — the call below moves nothing when
   * it is passed zero — and the name is the `&'static str` the console holds, so
   * this page never spells the set of filters itself.
   *
   * @param {number} cycle non-zero moves the shadow on to the next filter
   * @returns {string}
   */
  function filterName(cycle) {
    const len = ex.__crcbl_sundial_filter(cycle);
    return readUtf8(memory, ex.__crcbl_sundial_filter_ptr(), len);
  }

  /** Puts every control where the engine now is. */
  function refresh() {
    filter.textContent = `${filterName(0)} →`;

    const at = ex.__crcbl_sundial_seam_at(-1);
    seam.textContent = at > 0 ? 'lower it' : 'raise it';
    seamAt.value = String(at);
    seamValue.textContent =
      at > 0 ? `${at.toFixed(2)} of the width` : 'no seam';

    // The engine holds one debug view, so each of these is "is *this* the
    // picture" rather than a flag of its own control — a view some other route
    // put up leaves both readings off, and putting one of the two up takes the
    // other down. That is the truth about the frame rather than about the page.
    atlas.textContent = ex.__crcbl_sundial_atlas_view(0)
      ? 'hide it'
      : 'show it';
    cascades.textContent = ex.__crcbl_sundial_cascades(0)
      ? 'hide it'
      : 'show it';

    const running = ex.__crcbl_sundial_sun_running(-1);
    sun.textContent = running ? 'stop it' : 'start it';
    const tick = ex.__crcbl_sundial_sun_tick(-1);
    // The slider spans one sweep and the clock counts past it — `Sky::at` takes
    // the remainder, so this is the same pose the frame is drawn at rather than
    // a second opinion about where the sun is.
    sunTick.value = String(tick % sweep);
    sunValue.textContent = `tick ${tick} of ${sweep}`;
  }

  /**
   * Runs `act`, then puts every control back where the engine is.
   *
   * @param {() => void} act
   */
  function drive(act) {
    act();
    refresh();
  }

  /**
   * Wires a button so that pressing it does not take the keyboard off the
   * canvas.
   *
   * **A canvas that loses focus is a demo the engine pauses**, and focus coming
   * back does not resume it — so a button that took the focus the way a button
   * normally does would pause the fixture on every press. `preventDefault` on
   * `mousedown` is what stops the focus moving at all; the `click` still fires,
   * because a click is raised on release over the same element whatever the
   * press did.
   *
   * A slider is deliberately *not* wired this way: cancelling its `mousedown`
   * would cancel the drag with it, and the drag is the whole control. Moving one
   * does pause the fixture — and here that costs something the court in alcove
   * did not, because a paused loop runs no fixed step and a sun request is
   * adopted on the next one. `web/pages/sundial.html` says how to set it ticking
   * again, and the control's own label is the request either way.
   *
   * @param {HTMLButtonElement} control
   * @param {() => void} act
   */
  function press(control, act) {
    control.addEventListener('mousedown', (event) => event.preventDefault());
    control.addEventListener('click', () => drive(act));
  }

  press(filter, () => ex.__crcbl_sundial_filter(1));
  press(seam, () => ex.__crcbl_sundial_seam(1));
  press(atlas, () => ex.__crcbl_sundial_atlas_view(1));
  press(cascades, () => ex.__crcbl_sundial_cascades(1));
  press(sun, () =>
    ex.__crcbl_sundial_sun_running(ex.__crcbl_sundial_sun_running(-1) ? 0 : 1)
  );
  press(reset, () => ex.__crcbl_sundial_reset());

  seamAt.addEventListener('input', () =>
    drive(() => ex.__crcbl_sundial_seam_at(Number(seamAt.value)))
  );
  sunTick.addEventListener('input', () =>
    drive(() => ex.__crcbl_sundial_sun_tick(Number(sunTick.value)))
  );

  /**
   * Opens the controls once the demo is actually running.
   *
   * They start disabled in the markup and stay that way until there is a loop
   * behind them: a page that let a visitor move the seam while start-up was
   * still polling would write a value `Options::apply` then overwrites on the
   * first frame, which reads as a control that did nothing. The status export is
   * the same one `demo.js` drives its own loop on, and it answers without
   * advancing anything.
   */
  function open() {
    const status = ex.__crcbl_sundial_status();
    if (status === STATUS.RUNNING || status === STATUS.PAUSED) {
      refresh();
      for (const control of controls) control.disabled = false;
      return;
    }
    if (status === STATUS.FAILED || status === STATUS.STOPPED) return;
    requestAnimationFrame(open);
  }
  requestAnimationFrame(open);
}

bootDemo({
  init,
  hint: 'the knobs under the canvas drive the filter, the seam, the shadow atlas, the cascade tint and the sun · ESC opens the panel — CAMERA swaps to the free one · then WASD, Space/Shift and the arrows fly it · F3 shows the panel · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => {
    installKnobs(ex);
    return {
      prepare: () => ex.__crcbl_sundial_prepare(),
      boot: () => ex.__crcbl_sundial_boot(),
      frame: (/** @type {number} */ now) => ex.__crcbl_sundial_frame(now),
      status: () => ex.__crcbl_sundial_status(),
      shutdown: () => ex.__crcbl_sundial_shutdown(),
      logLevel: (/** @type {number} */ level) =>
        ex.__crcbl_sundial_log_level(level),
      logTake: ex.__crcbl_sundial_log_take,
      logPtr: ex.__crcbl_sundial_log_ptr,
      errorPtr: () => ex.__crcbl_sundial_error_ptr(),
      errorLen: () => ex.__crcbl_sundial_error_len(),
    };
  },
});
