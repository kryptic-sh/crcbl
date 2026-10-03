// The towers demo's browser shim.
//
// Everything shared with the other demos — boot order, the log drain, the
// canvas, the status bar — is `web/engine/demo.js`. What is here is this
// sample's ten export names, the one line of hint text under its canvas, and
// the one thing no other demo has: a row of buttons under the field that plays
// the game down the keyboard's own path.
//
// **The names are written out literally.** `web/tools/check-exports.mjs` scans
// this shim for `.__crcbl_…` to learn which exports the JS depends on, and then
// checks the built `.wasm` actually has them; a template literal would hide
// every one of them from it.
//
// `hint` leads with building because it is the only thing on this page a
// visitor has to do — a click or a tap on a plot opens the build menu, and the
// keys do the same — while the waves arrive on their own after the build phase,
// and a run that is won or lost plays itself again. `N` brings the next wave
// forward rather than being the only way to see one — see
// `apps/towers/src/wave.rs`.
//
// `savedLabel` is "Nothing" and that is literal: the status bar says "Nothing
// saved." when the demo stops, which is the truth about a field that keeps no
// score between visits. Save and resume are milestone 1's slice 5 —
// `docs/plan/sample/07-towers.md`.

import init from './crcbl_towers.js';
import { bootDemo } from '../../engine/demo.js';

/**
 * The buttons under the field, in the order they are drawn.
 *
 * **WHY THIS DEMO HAS THEM AND NO OTHER DOES.** Towers is the one sample on the
 * site whose whole subject is a command: a visitor who cannot build a tower is
 * watching a field play itself and cannot touch the thing the page exists to
 * show. A tap **inside the canvas** reaches the game now — it opens the build
 * menu on the plot it lands on, `apps/towers/src/build_menu.rs` — and these
 * buttons predate it. **They stay while the browser gate clicks them** rather
 * than the canvas: `web/tools/browser-e2e.mjs`'s `towers` row reads one of them
 * reach the game, and moving that check into the canvas is unwritten —
 * `docs/backlog.md` says what it would take. They are outside the canvas, so
 * they go with the hint text rather than with engine code.
 *
 * **THEY SYNTHESISE THE KEY RATHER THAN CALLING ANYTHING.** Each dispatches the
 * `keydown`/`keyup` pair `web/engine/shell.js` already listens for on the
 * canvas, so a button and a keyboard reach the game down **one** path and there
 * is nothing for the two to disagree about. A second route into the engine would
 * be a second thing to keep in step with `ACTION_*` in `apps/towers/src/app.rs`.
 *
 * `code` is what the engine binds to and `key` is what the key produced; both
 * are sent because `onKey` forwards both. The four rows that are not here — the
 * wave key, the restart key, the pause menu and the stats overlay — are left
 * out on purpose: the table sends its own waves, a finished run starts itself
 * again, and the other two are the engine's reserved keys rather than this
 * game's.
 *
 * @type {ReadonlyArray<{label: string, code: string, key: string}>}
 */
const BUTTONS = [
  { label: 'PREV', code: 'ArrowLeft', key: 'ArrowLeft' },
  { label: 'NEXT', code: 'ArrowRight', key: 'ArrowRight' },
  { label: 'BOLT', code: 'Digit1', key: '1' },
  { label: 'SPLASH', code: 'Digit2', key: '2' },
  { label: 'SLOW', code: 'Digit3', key: '3' },
  { label: 'BUILD', code: 'KeyB', key: 'b' },
  { label: 'UPGRADE', code: 'KeyU', key: 'u' },
];

bootDemo({
  init,
  hint: 'Click or tap a plot to build or upgrade there · LEFT/RIGHT pick a build plot · 1/2/3 pick a tower kind (bolt, splash, slow) · B builds it · U steps the tower on the plot up a tier · N sends the next wave now · R restarts the run · ESC opens the panel · F3 shows the stats · F11 fullscreen',
  savedLabel: 'Nothing',
  bind: (ex) => ({
    prepare: () => ex.__crcbl_towers_prepare(),
    boot: () => ex.__crcbl_towers_boot(),
    frame: (/** @type {number} */ now) => ex.__crcbl_towers_frame(now),
    status: () => ex.__crcbl_towers_status(),
    shutdown: () => ex.__crcbl_towers_shutdown(),
    logLevel: (/** @type {number} */ level) =>
      ex.__crcbl_towers_log_level(level),
    logTake: ex.__crcbl_towers_log_take,
    logPtr: ex.__crcbl_towers_log_ptr,
    errorPtr: () => ex.__crcbl_towers_error_ptr(),
    errorLen: () => ex.__crcbl_towers_error_len(),
  }),
});

addControlRow();

/**
 * Puts {@link BUTTONS} in a second status bar under the field.
 *
 * A `div.status` rather than a row of its own, so the buttons inherit the
 * `.status button` rules `web/style.css` already has and this demo needs no
 * stylesheet of its own. Appended to the `.stage` panel the canvas lives in —
 * `web/templates/demo-window.html` is where those ids come from — and if either
 * is missing the page is simply left as every other demo's, because a shim that
 * threw here would take the game down with the decoration.
 */
function addControlRow() {
  const canvas = /** @type {HTMLCanvasElement | null} */ (
    document.getElementById('canvas')
  );
  const stage = canvas?.parentElement;
  if (!canvas || !stage) return;

  const row = document.createElement('div');
  row.className = 'status';
  // The one id this demo adds to the shim's. `web/tools/browser-e2e.mjs`'s
  // `towers` row finds the buttons through it and through each button's
  // `data-key`, and clicks one to read the key arrive on the `[HUD]` line — so
  // the row is gated rather than merely present. Prefixed with the demo's own
  // name because the shared ids in `web/templates/demo-window.html` are not.
  row.id = 'towers-controls';
  const label = document.createElement('span');
  label.textContent = 'The keys, as buttons:';
  row.append(label);

  for (const { label: text, code, key } of BUTTONS) {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = text;
    button.dataset.key = code;
    button.addEventListener('click', () => sendKey(canvas, code, key));
    row.append(button);
  }
  stage.append(row);
}

/**
 * Sends one press of `code` to the engine, through the canvas.
 *
 * **The release waits two frames, and that is not politeness.** Every control in
 * this sample is a press *edge* — `apps/towers/src/app.rs` says why a held key
 * must not spend the purse sixty times a second — and the engine turns queued
 * events into edges once per tick inside the `requestAnimationFrame` callback.
 * A `keyup` dispatched in the same breath as its `keydown` lands in the same
 * tick and takes the edge with it, so the command never happens;
 * `web/tools/browser-e2e.mjs` spaces its own presses for exactly this reason.
 * Two frames rather than one because a frame is allowed to run no ticks at all.
 *
 * Focus goes back to the canvas afterwards, so a visitor who has both a finger
 * and a keyboard does not lose the keyboard by tapping a button.
 *
 * @param {HTMLCanvasElement} canvas
 * @param {string} code the physical key, which is what the engine binds to
 * @param {string} key what that key produced
 */
function sendKey(canvas, code, key) {
  const send = (/** @type {string} */ type) =>
    canvas.dispatchEvent(new KeyboardEvent(type, { code, key, bubbles: true }));
  send('keydown');
  requestAnimationFrame(() => requestAnimationFrame(() => send('keyup')));
  canvas.focus();
}
