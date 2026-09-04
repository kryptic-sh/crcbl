// alcove in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_alcove_*` symbols, the two strings the status
// bar shows, and the occlusion knobs — which are one sample's controls and have
// no business in a file every demo runs.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` names the page's own knobs first, because they are what this fixture
// is for and what a visitor with no keyboard can reach; the free camera comes
// second, and it says to swap camera first because that is the truth — the page
// opens on the fixed pose the goldens are taken from, and `apps/alcove`
// integrates the free camera whether or not it is the one being drawn from.
// `savedLabel` is "Nothing" and that is literal: the status bar says "Nothing
// saved." when the demo stops, which is the truth about an occlusion fixture
// with no score and no save file.

import init from './crcbl_alcove.js';
import { bootDemo, STATUS } from '../../engine/demo.js';
import { readUtf8 } from '../../engine/wasm.js';

/**
 * Wires the page's occlusion controls to the sample's own exports.
 *
 * **Called from `bind`, which is the one place this page is handed its own wasm
 * instance.** `bootDemo` has no per-demo hook and should not grow one for a
 * single sample — `web/demos/viewer/main.js` says the same thing about its drop
 * target, for the same reason.
 *
 * Nothing here keeps a copy of a knob. Every control is written *and then read
 * back* through `apps/alcove/src/web.rs`, whose answer is what the console holds
 * after the write — so a slider the engine clamped shows where it landed, and a
 * value moved by a key, by the pause panel or by a typed console line is picked
 * up the next time anything on this page refreshes.
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

  const view = button('knob-view');
  const technique = button('knob-technique');
  const bent = button('knob-bent');
  const seam = button('knob-seam');
  const reset = button('knob-reset');
  const seamAt = slider('knob-seam-at');
  const radius = slider('knob-radius');
  const intensity = slider('knob-intensity');
  const seamValue = el('knob-seam-value');
  const radiusValue = el('knob-radius-value');
  const intensityValue = el('knob-intensity-value');

  const controls = [
    view,
    technique,
    bent,
    seam,
    reset,
    seamAt,
    radius,
    intensity,
  ];

  /** Where the seam stands when the page raises it: the middle of the frame. */
  const SEAM_CENTRE = 0.5;

  /**
   * The technique's own name, out of the engine's variable.
   *
   * The length and the address are one read — the call below moves nothing when
   * it is passed zero — and the name is the `&'static str` the console holds, so
   * this page never spells the set of techniques itself.
   *
   * @param {number} cycle non-zero moves the gather on to the next one
   * @returns {string}
   */
  function techniqueName(cycle) {
    const len = ex.__crcbl_alcove_technique(cycle);
    return readUtf8(memory, ex.__crcbl_alcove_technique_ptr(), len);
  }

  /** Puts every control where the console now is. */
  function refresh() {
    view.textContent = ex.__crcbl_alcove_view(-1) ? 'AO only' : 'shaded';
    technique.textContent = `${techniqueName(0)} →`;
    bent.textContent = ex.__crcbl_alcove_bent_normals(-1) ? 'on' : 'off';

    const at = ex.__crcbl_alcove_seam(-1);
    seam.textContent = at > 0 ? 'lower it' : 'raise it';
    seamAt.value = String(at);
    seamValue.textContent =
      at > 0 ? `${at.toFixed(2)} of the width` : 'no seam';

    radius.value = String(ex.__crcbl_alcove_radius_dial());
    radiusValue.textContent = `${ex.__crcbl_alcove_radius(-1).toFixed(2)} m`;
    intensity.value = String(ex.__crcbl_alcove_intensity_dial());
    intensityValue.textContent = ex.__crcbl_alcove_intensity(-1).toFixed(2);
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
   * does pause the fixture, which costs nothing to look at — a court with
   * nothing in it that moves draws the same picture paused, and the knobs go on
   * changing that picture — and `web/pages/alcove.html` says how to set it
   * ticking again.
   *
   * @param {HTMLButtonElement} control
   * @param {() => void} act
   */
  function press(control, act) {
    control.addEventListener('mousedown', (event) => event.preventDefault());
    control.addEventListener('click', () => drive(act));
  }

  press(view, () => ex.__crcbl_alcove_view(ex.__crcbl_alcove_view(-1) ? 0 : 1));
  press(technique, () => ex.__crcbl_alcove_technique(1));
  press(bent, () =>
    ex.__crcbl_alcove_bent_normals(ex.__crcbl_alcove_bent_normals(-1) ? 0 : 1)
  );
  press(seam, () =>
    ex.__crcbl_alcove_seam(ex.__crcbl_alcove_seam(-1) > 0 ? 0 : SEAM_CENTRE)
  );
  press(reset, () => ex.__crcbl_alcove_reset());

  seamAt.addEventListener('input', () =>
    drive(() => ex.__crcbl_alcove_seam(Number(seamAt.value)))
  );
  radius.addEventListener('input', () =>
    drive(() => ex.__crcbl_alcove_radius(Number(radius.value)))
  );
  intensity.addEventListener('input', () =>
    drive(() => ex.__crcbl_alcove_intensity(Number(intensity.value)))
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
    const status = ex.__crcbl_alcove_status();
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
  hint: 'the knobs under the canvas drive the occlusion · ESC opens the panel — CAMERA swaps to the free one · then WASD, Space/Shift and the arrows fly it · F3 shows the panel · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => {
    installKnobs(ex);
    return {
      prepare: () => ex.__crcbl_alcove_prepare(),
      boot: () => ex.__crcbl_alcove_boot(),
      frame: (/** @type {number} */ now) => ex.__crcbl_alcove_frame(now),
      status: () => ex.__crcbl_alcove_status(),
      shutdown: () => ex.__crcbl_alcove_shutdown(),
      logLevel: (/** @type {number} */ level) =>
        ex.__crcbl_alcove_log_level(level),
      logTake: ex.__crcbl_alcove_log_take,
      logPtr: ex.__crcbl_alcove_log_ptr,
      errorPtr: () => ex.__crcbl_alcove_error_ptr(),
      errorLen: () => ex.__crcbl_alcove_error_len(),
    };
  },
});
