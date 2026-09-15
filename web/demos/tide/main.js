// tide in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_tide_*` symbols, the two strings the status bar
// shows, and the gallery's three knobs — which scene, which medium preset and
// which camera — which are one sample's controls and have no business in a file
// every demo runs.
//
// All three reach one cell, `apps/tide/src/knobs.rs`, the same one `N`, `M`, `C`
// and the pause panel's rows write; `apps/tide/src/web.rs` is where that is
// argued. Each export answers with the name the cell holds after the write, so
// this page never spells the set of scenes or presets itself.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `savedLabel` is "Nothing" and that is literal: the status bar says "Nothing
// saved." when the demo stops, which is the truth about a water fixture with no
// score and no save file.

import init from './crcbl_tide.js';
import { bootDemo } from '../../engine/demo.js';
import { button, enumName, installKnobs } from '../../engine/knobs.js';

/**
 * Wires the page's gallery controls to the sample's own exports.
 *
 * Called from `bind`, the one place this page is handed its own wasm instance —
 * `web/demos/sundial/main.js` says why `bootDemo` grows no per-demo hook. The
 * wiring itself is `web/engine/knobs.js`; what is this sample's is the element
 * ids, what each control writes, and `refresh`.
 *
 * @param {Record<string, any>} ex the instance's raw exports
 */
function galleryKnobs(ex) {
  const memory = /** @type {WebAssembly.Memory} */ (ex.memory);

  const scene = button('knob-scene');
  const medium = button('knob-medium');
  const camera = button('knob-camera');
  const reset = button('knob-reset');

  /** Puts every control where the engine's cell now is. */
  function refresh() {
    scene.textContent = `${enumName(memory, ex.__crcbl_tide_scene, ex.__crcbl_tide_scene_ptr, 0)} →`;
    medium.textContent = `${enumName(memory, ex.__crcbl_tide_medium, ex.__crcbl_tide_medium_ptr, 0)} →`;
    camera.textContent = `${enumName(memory, ex.__crcbl_tide_camera, ex.__crcbl_tide_camera_ptr, 0).toLowerCase()} →`;
  }

  installKnobs({
    status: () => ex.__crcbl_tide_status(),
    refresh,
    buttons: [
      [scene, () => ex.__crcbl_tide_scene(1)],
      [medium, () => ex.__crcbl_tide_medium(1)],
      [camera, () => ex.__crcbl_tide_camera(1)],
      [reset, () => ex.__crcbl_tide_reset()],
    ],
    sliders: [],
  });
}

bootDemo({
  init,
  hint: 'the knobs under the canvas switch the scene, the medium and the camera · ESC opens the panel · on the free camera, WASD, Space/Shift and the arrows fly it · F3 shows the panel and the water passes’ cost · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => {
    galleryKnobs(ex);
    return {
      prepare: () => ex.__crcbl_tide_prepare(),
      boot: () => ex.__crcbl_tide_boot(),
      frame: (/** @type {number} */ now) => ex.__crcbl_tide_frame(now),
      status: () => ex.__crcbl_tide_status(),
      shutdown: () => ex.__crcbl_tide_shutdown(),
      logLevel: (/** @type {number} */ level) =>
        ex.__crcbl_tide_log_level(level),
      logTake: ex.__crcbl_tide_log_take,
      logPtr: ex.__crcbl_tide_log_ptr,
      errorPtr: () => ex.__crcbl_tide_error_ptr(),
      errorLen: () => ex.__crcbl_tide_error_len(),
    };
  },
});
