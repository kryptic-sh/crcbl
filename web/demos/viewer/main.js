// viewer in the browser.
//
// The boot sequence and the frame loop are `web/engine/demo.js`, shared with
// every other demo. What is left here is the part that genuinely cannot be
// shared: this sample's `__crcbl_viewer_*` symbols, and the two strings the
// status bar shows.
//
// The symbols are written out literally rather than built from the sample's
// name. `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` to learn
// which exports the JS depends on, and fails when one is missing from the
// artifact; a template literal would hide every one of them from it.
//
// `hint` leads with the mouse because that is what a visitor reaches for first
// and because this sample is a tool rather than a game: there is nothing to
// play, and the whole interaction is turning the document and looking at it.
// The native viewer opens a file the user names; this one shows the document it
// ships with, so no key here opens anything — `docs/plan/sample/05-viewer.md`'s
// drop target is V-F5 and is not built yet.
//
// `savedLabel` is "Nothing" and that is literal: the status bar says "Nothing
// saved." when the demo stops, which is the truth about a viewer with no score
// and no save file.

import init from './crcbl_viewer.js';
import { bootDemo } from '../../engine/demo.js';

bootDemo({
  init,
  hint: 'Drag to orbit, scroll to zoom · I lists what the document holds and what the conversion skipped · W wireframe · N world-space normals · -/= exposure · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
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
  }),
});
