// viewer in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_viewer_*` symbols, the two strings the status
// bar shows, and the drop target — which is one sample's feature and has no
// business in a file every demo runs.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` leads with the mouse because that is what a visitor reaches for first
// and because this sample is a tool rather than a game: there is nothing to
// play, and the whole interaction is turning the document and looking at it.
// The native viewer opens a file the user names; this one opens the document it
// ships with and then takes whatever is dropped on the canvas, which is
// `docs/plan/sample/05-viewer.md`'s V-F5.
//
// `savedLabel` is "Nothing" and that is literal: the status bar says "Nothing
// saved." when the demo stops, which is the truth about a viewer with no score
// and no save file.

import init from './crcbl_viewer.js';
import { bootDemo } from '../../engine/demo.js';
import { readUtf8 } from '../../engine/wasm.js';

/**
 * How long the page waits for the engine's answer to a drop, in ms.
 *
 * A drop is opened by the next frame that draws, so the answer is normally one
 * `requestAnimationFrame` away. The deadline is for the case where no frame
 * follows at all — the demo stopped, or it never started — because the
 * alternative is a status line stuck on "Opening…" for as long as the tab is
 * open, which reads as a page that is still working on it.
 */
const VERDICT_MS = 10_000;

/**
 * What the frame around the canvas goes while a file is over it.
 *
 * The stage's border, not an outline on the canvas: `web/style.css` says a
 * focus ring drawn over a game viewport reads as a rendering artifact and
 * lights the frame instead, and this is the same signal for the same reason.
 * `--accent` rather than the `--accent-dim` focus uses, so "this will take the
 * file" is not the same colour as "this has the keyboard".
 */
const DRAG_BORDER = 'var(--accent)';

/**
 * Wires the canvas to open a `.glb` or `.gltf` dropped onto it.
 *
 * **Called from `bind`, which is the one place this page is handed its own
 * wasm instance.** `bootDemo` has no per-demo hook and should not grow one for
 * a single sample: every other demo's page is the shared file and nothing else,
 * and a hook that one caller passes is indirection for a second use that has
 * not arrived.
 *
 * The ABI is `apps/viewer/src/web.rs`'s `DropTarget`: one staging buffer
 * carrying the file's name and its bytes, and a verdict read back the way the
 * log queue is read.
 *
 * @param {Record<string, any>} ex the instance's raw exports
 */
function installDropTarget(ex) {
  const canvas = /** @type {HTMLCanvasElement} */ (
    document.getElementById('canvas')
  );
  // The frame the drag lights up. `.stage` is `web/templates/demo-window.html`'s
  // wrapper around the canvas and its status bar, and the element `web/style.css`
  // already lights on focus.
  const stage = /** @type {HTMLElement | null} */ (canvas.closest('.stage'));
  const detail = /** @type {HTMLElement} */ (document.getElementById('detail'));
  const memory = /** @type {WebAssembly.Memory} */ (ex.memory);

  /**
   * Puts one line under the canvas.
   *
   * The detail line rather than the status line above it: the demo is still
   * playing, and what a drop changes is the sentence about the document, not
   * the sentence about the run. `demo.js` owns both and rewrites them on the
   * next status change — a pause, a stop — which is correct: at that point what
   * it has to say outranks a line about a file.
   *
   * @param {string} text
   */
  function say(text) {
    detail.textContent = text;
  }

  /** Whether a file is currently being dragged over the canvas. */
  function lit(/** @type {boolean} */ on) {
    if (stage) stage.style.borderColor = on ? DRAG_BORDER : '';
  }

  /**
   * The engine's answer to the drop that was just committed, or `null` if none
   * arrived before the deadline.
   *
   * Polled on `requestAnimationFrame` because that is the clock the answer is
   * produced on: the frame that opens the document is a rAF tick of
   * `demo.js`'s own loop, so a poll on any other cadence is either early or
   * late for no reason.
   *
   * @returns {Promise<string | null>}
   */
  function verdict() {
    return new Promise((resolve) => {
      const deadline = performance.now() + VERDICT_MS;
      const poll = () => {
        const len = ex.__crcbl_viewer_drop_take();
        if (len !== 0) {
          resolve(readUtf8(memory, ex.__crcbl_viewer_drop_ptr(), len));
          return;
        }
        if (performance.now() >= deadline) {
          resolve(null);
          return;
        }
        requestAnimationFrame(poll);
      };
      requestAnimationFrame(poll);
    });
  }

  /**
   * Hands one dropped file to the engine and reports what it made of it.
   *
   * @param {File} file
   */
  async function open(file) {
    let bytes;
    try {
      bytes = new Uint8Array(await file.arrayBuffer());
    } catch (error) {
      // A file the browser could not read at all: it moved, or the permission
      // went away between the drop and the read. Nothing crossed into wasm.
      say(`${file.name} could not be read: ${String(error)}`);
      return;
    }
    const name = new TextEncoder().encode(file.name);
    const total = name.length + bytes.length;
    const ptr = ex.__crcbl_viewer_drop_buffer(total);
    if (ptr === 0) {
      say(
        `${file.name} is ${bytes.length} bytes, which is more than this page ` +
          'will copy into the engine in one go.'
      );
      return;
    }
    // THE VIEW IS BUILT AFTER THE CALL THAT HANDED BACK THE POINTER. That call
    // allocates and can grow wasm memory, which detaches any `Uint8Array` over
    // the old buffer — the same trap `crcbl_store::web::fetch` documents, and
    // nothing between here and the `set` can grow memory again.
    const view = new Uint8Array(memory.buffer, ptr, total);
    view.set(name, 0);
    view.set(bytes, name.length);
    if (ex.__crcbl_viewer_drop_commit(name.length, bytes.length) !== 1) {
      say(`${file.name} could not be handed to the engine.`);
      return;
    }
    say(`Opening ${file.name}…`);
    say(
      (await verdict()) ??
        `${file.name} was handed over and no frame has opened it. The demo is ` +
          'not drawing.'
    );
  }

  // Every one of these prevents the default, and that is not ceremony: a
  // browser handed a file it was not told to keep navigates the tab to it, and
  // the demo — canvas, device, log and all — is gone. `dragover` is the one
  // that actually decides whether a drop is allowed to happen at all.
  canvas.addEventListener('dragenter', (event) => {
    event.preventDefault();
    lit(true);
  });
  canvas.addEventListener('dragover', (event) => {
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy';
    lit(true);
  });
  canvas.addEventListener('dragleave', (event) => {
    event.preventDefault();
    lit(false);
  });
  canvas.addEventListener('drop', (event) => {
    event.preventDefault();
    lit(false);
    // The first file and no other. A viewer draws one document, and opening
    // the last of five would be a choice the visitor did not make; saying so is
    // better than picking.
    const files = event.dataTransfer?.files ?? [];
    if (files.length === 0) {
      say('That was not a file. Drop a .glb or .gltf document on the canvas.');
      return;
    }
    if (files.length > 1) {
      say(
        `${files.length} files were dropped and a viewer draws one document. ` +
          `Opening ${files[0].name}.`
      );
    }
    void open(files[0]);
  });
}

bootDemo({
  init,
  hint: 'Drag to orbit, scroll to zoom · drop a .glb or .gltf on the canvas to open your own · I lists what the document holds and what the conversion skipped · W wireframe · N world-space normals · -/= exposure · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => {
    installDropTarget(ex);
    return {
      prepare: () => ex.__crcbl_viewer_prepare(),
      boot: () => ex.__crcbl_viewer_boot(),
      frame: (/** @type {number} */ now) => ex.__crcbl_viewer_frame(now),
      status: () => ex.__crcbl_viewer_status(),
      shutdown: () => ex.__crcbl_viewer_shutdown(),
      logLevel: (/** @type {number} */ level) =>
        ex.__crcbl_viewer_log_level(level),
      logTake: ex.__crcbl_viewer_log_take,
      logPtr: ex.__crcbl_viewer_log_ptr,
      errorPtr: () => ex.__crcbl_viewer_error_ptr(),
      errorLen: () => ex.__crcbl_viewer_error_len(),
    };
  },
});
