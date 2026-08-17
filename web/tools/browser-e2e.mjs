#!/usr/bin/env node
// Drives the built demo site in a real headless Chromium and asserts that
// the demo under test *renders*.
//
//   node web/tools/browser-e2e.mjs [--site target/site] [--demo demos/breakout/]
//
// `web/run-browser-e2e.sh` is the entry point a human or CI runs; this file is
// the half that needs a JS engine. Everything here is plain Node with no
// dependencies — the shim has a no-npm policy, and a test harness that needed a
// package manager would be the one thing in `web/` that did.
//
// WHY THIS EXISTS. `check-exports.mjs` proves the artifact's symbols line up
// and `smoke.mjs` proves the boot sequence runs under Node with every import
// stubbed. Neither can see a GPU, so between them they stop exactly one call
// short of the thing the P5 gate is about: a browser that loads the page, opens
// a WebGPU device and puts pixels on the canvas. A black canvas passes every
// check those two can make.
//
// WHAT IT ASSERTS. Five groups, printed in order:
//
//   A  the platform — `navigator.gpu`, an adapter, **that this browser can
//      report canvas pixels at all** (see below; this one is not a formality),
//      and that the origin is cross-origin isolated, which is what decides
//      whether a threaded wasm build could run here at all
//   B  the engine boots — canvas size, wgpu backend, swapchain, STATUS_RUNNING
//   C  input drives the simulation — a real click focuses the canvas, a real
//      Space key launches the ball, and the game's own HUD log line changes.
//      A demo with no input at all (`key: null` in EXPECTATIONS) skips the key
//      and keeps the log line, which is the half it can still make good on
//   D  it renders — no WebGPU device errors, the canvas is not one flat colour,
//      and the canvas changes from frame to frame while the ball is in flight
//   E  focus and pause — a blurred canvas pauses and runs no ticks, focus
//      coming back does not resume on its own, and Escape does
//
// THE READBACK IS THE PART THAT NEEDED PROVING. Three ways to get a WebGPU
// canvas's pixels into JS look equivalent and are not, measured on Chromium 150
// and again on 151, against a page that does nothing but clear a canvas to a
// known colour:
//
//   drawImage(canvas, …) + getImageData   transparent black, always
//   createImageBitmap(canvas)             transparent black, always
//   canvas.toDataURL()                    the actual pixels
//
// The first two are the obvious spellings and both of them report a perfectly
// rendered frame as blank. That is a harness that fails a working engine, which
// is worse than no harness — so group A runs that known-colour clear first, in
// this same browser with these same flags, and refuses to interpret group D
// unless the control comes back with the colour it drew. `docs/plan/ROADMAP.md`
// puts it as "verify the checker, not just the code".
//
// That control has since earned its keep. On Chromium 151 it went red under
// every adapter mode, and what it had caught was not a readback quirk: the
// browser could not get the canvas out of the WebGPU device and into the
// compositor, so the snapshot was uninitialised memory — for the demo's canvas
// exactly as for the control's. The engine was fine and the reader was fine;
// the two devices did not match. `browserFlags` carries the fix.
//
// WHAT GROUP D DOES NOT PROVE: that the frame is the *right* image. There is no
// reference comparison here — `crcbl screenshot` renders a different scene at a
// different size through a different backend, and calling those two comparable
// would be a lie a tolerance could hide. "Not blank, changing with the ball,
// with no device errors behind it" is the honest ceiling for this harness.
//
// EXIT STATUS. Non-zero if any check fails *or* if zero checks ran. The second
// half is not decoration: `docs/plan/12-testing.md` names a silently-skipped
// e2e job as a known trap, and a harness whose browser never started would
// otherwise print nothing and succeed.

import { spawn } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { ISOLATION_HEADERS, MIME, serve } from './serve.mjs';

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

// ---------------------------------------------------------------------------
// Arguments and environment
// ---------------------------------------------------------------------------

/**
 * Every browser this process started and has not stopped.
 *
 * A leaked Chromium is not a tidiness problem: it holds a GPU context and a
 * profile directory, and a developer who runs the harness a few times ends up
 * with several of them. [`fail`] and the exit hook below close over this so
 * that no exit path can skip the kill.
 *
 * **Declared here, above the argument parsing, and that placement is the fix
 * for a real bug.** `fail` calls `stopEverything`, which reads this; `fail` and
 * `stopEverything` are function declarations and hoist, but a `const` does not.
 * With this further down the file, every argument-parsing failure printed its
 * real message and then died in the temporal dead zone with
 * `ReferenceError: Cannot access 'running' before initialization`, burying the
 * diagnosis under a stack trace pointing at the cleanup path.
 *
 * @type {Set<{ stop: () => void }>}
 */
const running = new Set();

/** @type {Record<string, string>} */
const args = {};
for (let i = 2; i < process.argv.length; i += 1) {
  const arg = process.argv[i];
  if (!arg.startsWith('--')) fail(`unexpected argument ${arg}`);
  const [name, inline] = arg.slice(2).split('=', 2);
  args[name] = inline ?? process.argv[++i] ?? '';
}

const SITE = resolve(REPO, args.site ?? process.env.SITE_DIR ?? 'target/site');
const DEMO = args.demo ?? 'demos/breakout/';

/** The demo's own name, for filenames and messages. */
const SLUG = DEMO.replace(/^demos\//, '').replace(/\/$/, '');

/**
 * The two assertions that are about the *game* rather than about the browser.
 *
 * Everything else this file checks — a device opened, a canvas that is neither
 * blank nor still, no page errors — is true of any demo. These two are not:
 * "the run started" and "it is advancing on its own" have to be read out of
 * whatever HUD line the game happens to log. Keeping them in one table is what
 * lets the same gate cover a second game; before flappy they were breakout's
 * strings inline, which is the shape that only ever works once.
 *
 * `key` is **nullable**, and a row that sets it to `null` is saying the demo has
 * no key that *starts* it rather than that nobody got round to writing the
 * checks. Group C skips its start-key half for such a demo — there is no waiting
 * state to leave, so a dispatched key and an assertion about what it did would be
 * a check wired to nothing — and keeps the half that is about the simulation
 * advancing, which is the claim every demo can make. `started`, `startedLabel`
 * and `startedFailure` are then unused and left out. `waiting` and `moving` are
 * required of every row.
 *
 * Two rows use it and they are `null` for different reasons: hud takes no input
 * at all, and lumen takes plenty but has no run to begin — which is why this
 * says "no start key" rather than "no input".
 */
const EXPECTATIONS = {
  breakout: {
    key: 'Space',
    waiting: (line) => line.includes('WAITING'),
    started: (line) => line.includes('PLAYING'),
    startedLabel: 'Space launches the ball',
    startedFailure: 'the state never left WAITING',
    moving: /Ball x: (-?[\d.]+)/,
    movingLabel: 'the ball moves after the launch',
    // **What a finger can do here**, read by group F the way the rest of this
    // table is read. A row with no `touch` key is a demo whose bindings take no
    // pointer at all, and group F makes only the page-level claims for it.
    //
    // `paddle` says this game binds the pointer's *position*, so a drag has a
    // visible result and [`SAMPLE_PADDLE`] can read it. `pause` says the demo
    // draws `crcbl::engine::PauseControl`, which is the only route to the pause
    // menu — and so to fullscreen and the debug panel — on a device with no
    // keyboard. `lives` is how the gate knows the start menu has gone:
    // `menu::MenuKind::of` shows it while the run is untouched, a menu on
    // screen owns the button, and a lost life is what returns the game to
    // WAITING with nothing over it — the state a tap serves from, and the
    // reason the tap binding exists at all.
    touch: { paddle: true, lives: /Lives: (\d+)/, pause: true },
  },
  flappy: {
    key: 'Space',
    waiting: (line) => line.includes('WaitingToStart'),
    started: (line) => line.includes('Playing'),
    startedLabel: 'Space starts the run',
    startedFailure: 'the state never left WaitingToStart',
    moving: /\bx: (-?[\d.]+)/,
    movingLabel: 'the bird advances after the flap',
    // No pointer *position* binding in this game — a tap is a flap and where it
    // landed says nothing — so a drag has nothing to show and there is no
    // `paddle`. `height` is the tap's observable instead: gravity is the only
    // other thing that touches the bird's `y`, and it can only ever lower it, so
    // a value above the one from before the taps is a flap and cannot be
    // anything else. `\by:` and not `y:`, or the line's own `vy:` matches first.
    touch: { height: /\by: (-?[\d.]+)/, pause: true },
  },
  // `rock x` and not the ship's: this game's ship is stationary until the
  // player thrusts, and Space only fires. The rocks drift on their own from the
  // first tick, so a value that changes is the simulation advancing and nothing
  // else. `apps/asteroids/src/game.rs` logs it for exactly this reason.
  asteroids: {
    key: 'Space',
    waiting: (line) => line.includes('WaitingToStart'),
    started: (line) => line.includes('Playing'),
    startedLabel: 'Space starts the game',
    startedFailure: 'the state never left WaitingToStart',
    moving: /rock x: (-?[\d.]+)/,
    movingLabel: 'the rocks drift after the first shot',
  },
  // This row used to be the odd one out: horde shipped with no waiting state,
  // so "the input reached the simulation" was read off the *run counter* —
  // `run: 1` before and `run: 2` after, because Space was bound to restart. It
  // has a start screen now, so it makes the same claim the other three do.
  //
  // The clock is the right `moving` value and it is stricter than it looks:
  // this game's clock is stopped on `WaitingToStart`, so two distinct `time:`
  // values cannot appear unless the key really started the run. The run counter
  // stays in the HUD line for a bug report, and `run: 1` in `waiting` is what
  // says the demo booted a fresh run rather than a restarted one.
  horde: {
    key: 'Space',
    waiting: (line) =>
      line.includes('WaitingToStart') && line.includes('run: 1'),
    started: (line) => line.includes('Playing'),
    startedLabel: 'Space starts the run',
    startedFailure: 'the state never left WaitingToStart',
    moving: /time: ([\d.]+)/,
    movingLabel: 'the clock advances under its own steam',
    // **The demo with on-screen controls**, and the only one that can make the
    // claim below: `walk` is where the wizard is standing, and nothing but the
    // player's own movement input ever changes it — enemies do not push him and
    // the arena does not drift. `pause` says this game draws a second control
    // beside the stick — the pause button every touch demo draws — which is
    // what lets one finger walk while another does something else entirely.
    touch: { walk: /\bx: (-?[\d.]+)/, pause: true },
  },
  // **The demo with no input.** `apps/hud` is the UI system's fixture rather
  // than a game: its `HostedGame::key_event` is empty by design and its page is
  // driven end to end by a scripted ticker on the server, so there is no key to
  // press and no waiting state to leave. `key: null` is what says that, and it
  // is the difference between "this demo takes no input" and "this row is
  // half-written".
  //
  // `waiting` is therefore not about waiting: it is the shape of hud's own HUD
  // line at the start of a run, and it is what separates a hud page that got as
  // far as ticking from every other demo's log and from a page that logged
  // nothing at all. The wave is `1` because the first line is emitted on tick
  // one — `Game::log_heartbeat` logs the tick a wave turns over on — and the
  // second wave is ten seconds of simulated time away.
  //
  // `rolls` is the rng cursor and it is the strictest `moving` value any row
  // here uses. It advances only when the script actually rolls a number — an
  // incoming hit, or an ability being cast — so a ticker that was being stepped
  // but doing nothing would leave it standing still, and a frame counter,
  // a wall clock or the heartbeat's own cadence cannot move it.
  hud: {
    key: null,
    waiting: (line) => line.includes('[HUD] tick: 1  wave: 1'),
    moving: /rolls: (\d+)/,
    movingLabel: 'the ticker rolls new numbers under its own steam',
  },
  // **The other demo with no start key.** `apps/lumen` is a lighting fixture
  // rather than a game: there is no run to begin, so there is no waiting state
  // and nothing for a `Space` to leave. It does take input — the arrows and WASD
  // fly the free camera — but the page opens on the fixed pose the goldens are
  // taken from, and swapping to the other camera is a pause-menu row rather than
  // a key. `key: null` says all of that; what it never says is that a row was
  // left half-written.
  //
  // `waiting` is this sample's own claim and it is the reason the page exists.
  // A browser has **no ray query**, so `LightingPath::Rasterised` is the arm the
  // selector resolves to by construction, and the first heartbeat naming it is
  // what separates a rasterised room from a page that opened some other device
  // or fell over before the first tick. `Lumen::log_heartbeat` prints the
  // selector's own `Debug`, which is a deliberate coupling to
  // `crates/crcbl-hal/src/caps.rs`: a renamed variant fails here loudly.
  //
  // `moving` is the orbiting lamp's x. It is the only thing in the room that
  // moves, `room::lamp` is a pure function of the seconds `Gpu::advance`
  // accumulates, and those seconds accumulate in the tick — so a page that was
  // presenting frames without ticking, or one stuck on the first tick, leaves it
  // standing still. `room::LAMP_PERIOD` is slow enough to read as a moving light
  // and still quick enough that consecutive heartbeats differ far above the two
  // decimal places the line prints them to.
  lumen: {
    key: null,
    waiting: (line) =>
      line.includes('[HUD] tick: 60') && line.includes('lighting: Rasterised'),
    moving: /lamp x: (-?[\d.]+)/,
    movingLabel: 'the lamp keeps orbiting under its own steam',
  },
};

const EXPECTED = EXPECTATIONS[SLUG];
if (!EXPECTED) {
  console.error(
    `browser-e2e: no expectations for demo "${SLUG}". Add it to EXPECTATIONS ` +
      `in this file — a gate that does not know what the game logs would pass ` +
      `on a game that never started.`
  );
  process.exit(2);
}
const OUT = resolve(REPO, args.out ?? 'target/web-e2e');
const TIMEOUT_MS = Number(
  args.timeout ?? process.env.CRCBL_WEB_E2E_TIMEOUT_MS ?? 90_000
);

/**
 * Which WebGPU adapter Chromium is told to use.
 *
 * `auto` (the default) tries `hardware` and falls back to `swiftshader`, taking
 * the first mode whose *readback control* passes — an adapter that renders but
 * whose pixels cannot be read is no use to this harness, and each Chromium
 * release so far has had at least one mode that is exactly that.
 */
const ADAPTER = args.adapter ?? process.env.CRCBL_WEB_E2E_ADAPTER ?? 'auto';

/**
 * Whether the site under test renders through `crcbl-webgpu` rather than
 * `crcbl-wgpu`.
 *
 * Set by `CRCBL_WEB_BACKEND=webgpu`, the same variable `web/build.sh` reads to
 * turn on the umbrella's `webgpu` feature. In this mode the engine's own
 * `WebGpuDevice` installs the one `StreamChannel`, so the crcbl-webgpu PROBE
 * groups (G onward) cannot install a second one and are skipped — the demo does
 * through the backend exactly what those groups do standalone. The engine boot
 * check also expects the *webgpu* backend's adapter line instead of wgpu's. The
 * default (`wgpu`, or unset) runs everything as before.
 */
const WEBGPU_MODE = (process.env.CRCBL_WEB_BACKEND ?? 'wgpu') === 'webgpu';

/** How many times the canvas is read back once the ball is in flight. */
const SAMPLE_COUNT = 16;

/**
 * How long the focus/pause group watches for a HUD heartbeat.
 *
 * Both samples log one every sixty ticks — a second of *simulated* time. Under
 * SwiftShader a frame is slow enough that the accumulator's 64 ms clamp makes
 * simulated time run behind wall time, so the window is several times that
 * second rather than a hair over it. The group's first check is the control
 * that keeps this number honest: a window too short to hold a heartbeat fails
 * there rather than making "a paused demo runs no ticks" pass for free.
 */
const TICK_WINDOW_MS = 4_000;

/**
 * How far inside the canvas group E clicks to hand the keyboard back.
 *
 * CSS pixels from the canvas's top-left corner. Far enough in that a rounded
 * `getBoundingClientRect` cannot put the point on the neighbouring element, and
 * nowhere near the centred pause menu — which is the point, and is why this is a
 * named constant rather than a `+ 8` in the middle of a check. Each sample's
 * `a_focusing_click_off_every_button_leaves_the_game_paused` asserts the same
 * inset is outside every button of every menu, so a menu that grew until it
 * reached the corner fails a fast Rust test rather than this slow one.
 */
const FOCUS_CLICK_INSET = 8;

/**
 * How many contacts the emulated touchscreen reports.
 *
 * The checks need two — one to hold and one to move — and a phone reports about
 * this many.
 */
const MAX_TOUCH_POINTS = 5;

/**
 * One `ShellEvent::Touch` as the engine logs it at debug level.
 *
 * `Pending::observe` logs every shell event it folds, so this is the seam's own
 * account of what reached the engine: past the browser, past the JS shim, past
 * the wasm ABI. Nothing else in the page can see a contact — the demos here bind
 * the pointer and none of them draws an on-screen control yet — and a check that
 * watched the *game* instead could only ever assert what the emulated pointer
 * did, which is the thing that has not changed.
 *
 * Matching on `Debug` output is a deliberate coupling to
 * `crates/crcbl-shell/src/event.rs`: it fails loudly and immediately if the
 * variant is reshaped, which is the failure mode to want.
 */
const TOUCH_LINE =
  /Touch \{.*?contact: ContactId\((\d+)\).*?phase: (\w+).*?position: PhysicalPoint \{ x: (-?[\d.]+), y: (-?[\d.]+)/;

/** The engine's `LevelFilter::Debug`, the level `TOUCH_LINE` needs. */
const LOG_DEBUG = 4;

/** …and the level the demos boot at, restored once the contacts are read. */
const LOG_INFO = 3;

/**
 * How far a logged contact may sit from where it was dispatched, in device
 * pixels.
 *
 * A dispatched point is rounded to whole CSS pixels and the canvas's own box is
 * fractional, so a couple of pixels of slack is the coordinate arithmetic and
 * not the engine. It stays far tighter than the gap between the two contacts
 * below — hundreds of pixels — which is what the tolerance must not swallow.
 */
const CONTACT_TOLERANCE = 4;

/**
 * Where the two contacts of the multi-touch check land, as fractions of the
 * canvas box.
 *
 * Low on the canvas, so the press that starts each one lands below a centred
 * menu and cannot fire a widget; far apart across it, so a check that confused
 * one contact with the other cannot pass inside `CONTACT_TOLERANCE`.
 */
const CONTACT_BAND = 0.85;
const CONTACT_A = 0.25;
const CONTACT_B = 0.6;
const CONTACT_B_MOVED = 0.75;

/**
 * How many `touchMove`s a drag is dispatched as.
 *
 * Enough that "the moves stopped arriving part way" is a different number from
 * "every move arrived", which is the whole of the `touch-action` check: a
 * browser that claims the gesture delivers the first one or two and then a
 * `pointercancel`.
 */
const DRAG_STEPS = 8;

/**
 * The band of the canvas [`SAMPLE_PADDLE`] looks in, as a fraction of its
 * height, and how near the finger the paddle has to land, as a fraction of its
 * width.
 *
 * The paddle sits at the bottom of breakout's field and nothing else blue is
 * down there — the ball rests well above the band, the walls are grey, and a
 * menu is laid out centred.
 */
const PADDLE_BAND = 0.85;
const PADDLE_TOLERANCE = 0.015;

/**
 * How long the paddle is given to arrive where a finger put it.
 *
 * A deadline on a poll rather than a sleep, everywhere but one: it is also the
 * window "a second contact leaves the emulated pointer alone" watches nothing
 * happen for, and a negative claim has no observable to poll. The control inside
 * that check — the first contact moving, inside this same window — is what stops
 * a window too short to show a move from making it pass for free.
 */
const PADDLE_SETTLE_MS = 1_500;

/**
 * Where the drags in group F put the paddle, as fractions of the canvas width.
 *
 * The second drag starts somewhere else and moves a *short* way, which is what
 * tells an absolute placement from a delta-composed one: composed on top of the
 * first drag it would land near 0.36 instead, which is nowhere near the
 * tolerance above. Both targets stay inside the walls, because
 * `game::clamp_paddle` stops the paddle short of them and a target beyond one
 * would be asserting on the clamp rather than on the drag.
 */
const FIRST_DRAG = { from: 0.5, to: 0.3 };
const SECOND_DRAG = { from: 0.6, to: 0.66 };

/**
 * Where every later touch in breakout lands, as a fraction of the canvas width.
 *
 * A contact states a *place*, so a tap is also a move: served from the middle of
 * the canvas, each tap would park the paddle under the ball it just launched and
 * the rally would go on until the run's timeout. The far edge is the one place a
 * tap cannot catch the ball, which is what makes "wait for the life to be lost"
 * a wait of seconds rather than of minutes.
 */
const PARK_X = 0.02;

/**
 * How long group F waits for the ball to come down, and how long it taps a
 * flappy bird for while waiting to see it climb.
 *
 * Neither is a guess about how fast the demo runs: both are deadlines on a poll
 * for an observable, sized so a check that is *going* to fail says so in a
 * reasonable time rather than sitting on the run's whole timeout.
 */
const LIFE_MS = 25_000;
const CLIMB_MS = 10_000;

/** How long between the taps that keep a bird in the air. */
const TAP_INTERVAL_MS = 120;

/**
 * Where the on-screen stick check puts its thumb, as fractions of the canvas
 * box.
 *
 * Low enough to clear a centred menu and the HUD strip along the top, and a long
 * way across: the throw is a fraction of the surface's shorter side, so a drag
 * of a quarter of the width is full deflection at any size this runs at.
 */
const STICK_BAND = 0.62;
const STICK_FROM = 0.35;
const STICK_TO = 0.62;

/**
 * How far the wizard has to walk before the thumb is the only thing that can
 * have moved him, in the world units his HUD line prints.
 *
 * `apps/horde` walks him at `PLAYER_SPEED` and nothing else in the game touches
 * his position — no knockback, no drift — so anything above zero is already the
 * claim. This is a margin over the two decimal places the line is printed to.
 */
const WALK_MARGIN = 1.0;

/**
 * How long the wizard is given to walk, and to stop again.
 *
 * A deadline on a poll, not a sleep. It is generous because the observable is
 * the every-sixtieth-tick HUD line and a SwiftShader frame is slow enough that a
 * simulated second is several wall ones.
 */
const WALK_MS = 25_000;

/**
 * How far inside the canvas's top-right corner the pause button is tapped, in
 * the canvas's own device pixels.
 *
 * A deliberate coupling to `crcbl::engine::pause`, which insets a 112 × 56
 * button by 12 — the same button in every demo that draws one: any point at
 * least this far in from the corner and no further than the button's own size
 * lands on it. Written as one number rather than as the rectangle so that a
 * button that is resized but stays in its corner does not move this file.
 */
const PAUSE_INSET = 40;

/**
 * A menu action the engine logs, for the checks that assert **no** button was
 * fired.
 *
 * Resume, fullscreen and the debug panel are the three the loop owns, and two of
 * them say so in the log. A deliberate coupling to `crcbl::engine`'s own lines,
 * like `TOUCH_LINE`: a rename fails this loudly rather than quietly asserting
 * nothing.
 */
const STRAY_MENU_ACTION = /game resumed|asked for borderless/;

/** The engine's status while a demo is running, and while it is paused. */
const STATUS_RUNNING = 3;
const STATUS_PAUSED = 6;

/**
 * The key group G adds a second, wrong canvas to the demo's registry under.
 *
 * `SurfaceTarget::Web` is an integer into the shell's JS-side canvas registry
 * and nothing else, so this number is the page's to choose and means nothing to
 * wasm. The *right* key is not here: `web/engine/demo.js` owns the registry now
 * and registers its own canvas under its own `CANVAS_ID`, which the driver
 * reads back as `crcbl.gpu.canvasId` rather than restating.
 *
 * **The decoy is why a second one is added at all.** With one canvas in the
 * registry, a replayer that ignored the `canvasId` on the wire and took whatever
 * it found would hand back the right context by accident, and no identity check
 * could tell. It is inserted *ahead of* the demo's canvas — group G re-inserts
 * that one to make it so — because a `Map` keeps insertion order, and the decoy
 * is only a decoy while "the first entry" is also the wrong answer.
 * `web/tools/gpu-replay.mjs` runs the same shape against a stub.
 */
const DECOY_CANVAS_ID = 6;

/**
 * The `crcbl_hal::Format` code each canvas format the specification defines has,
 * from `crates/crcbl-webgpu/src/tag.rs`.
 *
 * Spelled out here rather than imported from `gpu-replay.js` for the reason the
 * feature bits in group G are: a table taken from the thing under test agrees
 * with it by construction, and what the check below is for is that the table
 * itself is right about what this browser prefers.
 */
const SEAM_FORMAT_CODE = Object.freeze({
  rgba8unorm: 0x02,
  bgra8unorm: 0x04,
});

/** The bit `SurfaceCaps::present_modes` sets for `PresentMode::Fifo`. */
const FIFO_BIT = 1 << 0;

/**
 * How many bytes group J asks wasm to make a buffer of.
 *
 * **The driver's number rather than wasm's**, which is what makes the size check
 * evidence: `__crcbl_web_gpu_probe_buffer` takes it as an argument, a browser
 * reports `GPUBuffer.size` off the object it created, and the two are compared.
 * A size fixed in `probe.rs` would be a constant checked against itself.
 *
 * Nothing about the value matters beyond being a size no default produces —
 * `4096` is the fixture's, so this is deliberately not that either.
 */
const PROBE_BUFFER_BYTES = 12_288;

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `probe_buffer_desc` sets.
 *
 * Restated rather than exported through the ABI: it is a `&'static str` on the
 * Rust side and the point of checking it is that the string *crossed* — a label
 * the driver read out of wasm would agree with wasm whatever the replayer did
 * with it.
 */
const PROBE_BUFFER_LABEL = 'crcbl-webgpu probe buffer';

/**
 * The extent and mip count group K asks wasm to make a texture with.
 *
 * The driver's numbers rather than wasm's, for {@link PROBE_BUFFER_BYTES}'s
 * reason: `__crcbl_web_gpu_probe_image` takes all three as arguments and a
 * browser reports `GPUTexture.width`, `.height` and `.mipLevelCount` off the
 * object it created, so the two are compared rather than restated.
 *
 * A power of two in each axis with a chain shorter than the full one, so the mip
 * count is a number the extent permits and is visibly not the default of `1`.
 */
const PROBE_IMAGE_WIDTH = 256;
const PROBE_IMAGE_HEIGHT = 128;
const PROBE_IMAGE_MIPS = 4;

/**
 * The labels `crates/crcbl-webgpu/src/probe.rs` sets on the image and its view.
 *
 * Restated rather than exported through the ABI, for {@link PROBE_BUFFER_LABEL}'s
 * reason — the point of checking one is that the string *crossed*.
 */
const PROBE_IMAGE_LABEL = 'crcbl-webgpu probe image';
const PROBE_VIEW_LABEL = 'crcbl-webgpu probe view';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_SAMPLER_DESC` sets, and
 * **the only member of the sampler a browser reports at all**.
 *
 * There is no `PROBE_SAMPLER_*` number beside it, unlike the buffer's size and
 * the image's extent, and that is the object rather than an omission: a
 * `GPUSampler` has no readable filters, address modes or clamps, so there is
 * nothing a page could pass in and read back. What group L has instead is the
 * class of what came back and the silence of the device's error queue — see the
 * group's own comment.
 */
const PROBE_SAMPLER_LABEL = 'crcbl-webgpu probe sampler';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_BIND_GROUP_LAYOUT_DESC`
 * sets, and — as with a sampler — **the only member of the layout a browser
 * reports at all**.
 *
 * A `GPUBindGroupLayout` exposes no entries, no bindings and no visibility, so
 * group M's evidence is the class of what came back and the silence of the
 * device's error queue afterwards. What the descriptor was is checkable only
 * before it is handed over, which is `web/tools/gpu-replay.mjs`'s job.
 */
const PROBE_LAYOUT_LABEL = 'crcbl-webgpu probe layout';

/**
 * How many entries `PROBE_BIND_GROUP_LAYOUT_ENTRIES` declares.
 *
 * Not readable off the `GPUBindGroupLayout` — nothing about a layout is — so
 * this is not compared against the object. It is what the group's message
 * *says*, so a person reading a failure knows how much of a list the browser was
 * asked to take: four entries, not one, because this is the first command whose
 * body is a counted list of structs and a single-entry layout would decode the
 * same whatever the stride.
 */
const PROBE_LAYOUT_ENTRIES = 4;

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_BIND_GROUP_DESC` sets,
 * and — as with a sampler and a layout — **the only member of the group a browser
 * reports at all**.
 *
 * A `GPUBindGroup` exposes no layout and no entries, so group N's evidence is the
 * class of what came back and the silence of the device's error queue afterwards.
 * What the group *bound* is checkable only before it is handed over, which is
 * `web/tools/gpu-replay.mjs`'s job.
 */
const PROBE_BIND_GROUP_LABEL = 'crcbl-webgpu probe bind group';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_SHADER_MODULE_DESC` sets.
 *
 * A `GPUShaderModule` reports its `label` like every object above — but, unlike
 * them, it is **where compilation happens**, so group O has a second and stronger
 * piece of evidence than the label and the silent error queue: `getCompilationInfo()`.
 * The descriptor carries a known-good WGSL vertex entry, so a clean compile is a
 * fact a real browser can state and a stub cannot fake.
 */
const PROBE_SHADER_MODULE_LABEL = 'crcbl-webgpu probe shader';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_PIPELINE_LAYOUT_DESC`
 * sets.
 *
 * A `GPUPipelineLayout` reports its `label` and nothing else — not its
 * bind-group layouts, not its push-constant ranges (WebGPU has none) — so group
 * P's evidence is group N's two: the class of what came back, which
 * `instanceof GPUPipelineLayout` settles and no stub can satisfy, and the
 * device's error queue being empty afterwards, which is the only thing that can
 * say `createPipelineLayout` *accepted* the set list it was handed.
 */
const PROBE_PIPELINE_LAYOUT_LABEL = 'crcbl-webgpu probe pipeline layout';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_COMPUTE_PIPELINE_DESC`
 * sets.
 *
 * A `GPUComputePipeline` reports its `label` like a pipeline layout — but, unlike
 * it, it also answers `getBindGroupLayout(n)`, the derived layout only a
 * genuinely-built pipeline can hand back, because a pipeline is where the shader
 * and its layout are validated against each other. So group Q has that call as a
 * second piece of evidence beyond `instanceof GPUComputePipeline` and the silent
 * error queue.
 */
const PROBE_COMPUTE_PIPELINE_LABEL = 'crcbl-webgpu probe compute pipeline';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_GRAPHICS_PIPELINE_DESC`
 * sets.
 *
 * A `GPURenderPipeline` reports its `label` and answers `getBindGroupLayout(n)`
 * exactly as a compute pipeline does, so group R has the same two pieces of
 * evidence — but it is the *largest* descriptor on the seam, so what group R
 * really puts in front of the device is a whole nested tree: the primitive
 * state, the reversed-Z depth-stencil, the multisample state, and a blended
 * colour target, all of which a real `createRenderPipeline` must accept.
 */
const PROBE_GRAPHICS_PIPELINE_LABEL = 'crcbl-webgpu probe raster pipeline';

/**
 * How far above its starting height a flappy bird has to climb before the taps
 * are the only thing that can have put it there, in the units its HUD prints.
 *
 * A run *starts* with a flap: the start menu's button is bound to the same
 * action a tap is, so the bird is above where it began for the third of a second
 * that one flap's arc lasts, whether or not another tap ever lands.
 * `game::FLAP_SPEED` documents that arc as a little under half a pipe gap, and
 * `game::WORLD_CEILING` is where a bird tapped several times a second is pinned
 * within a second. This sits between them, nearer the ceiling, so that a sample
 * taken part way up the climb still counts.
 */
const CLIMB_ABOVE = 4;

/** The control page's clear colour, and what it must read back as. */
const CONTROL_RGB = [0, 51, 204];

/** Where the harness's own control page lives on its server. */
const CONTROL_PATH = '/__crcbl-readback-control__';

function stopEverything() {
  for (const browser of running) browser.stop();
  running.clear();
}

// `process.exit` does not unwind, so a `finally` is not enough on its own.
process.on('exit', stopEverything);
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => {
    stopEverything();
    process.exit(130);
  });
}

function fail(message) {
  console.error(`web e2e: ${message}`);
  stopEverything();
  process.exit(2);
}

if (!existsSync(SITE)) fail(`no site at ${SITE} — run web/build.sh first`);
if (!Number.isFinite(TIMEOUT_MS) || TIMEOUT_MS <= 0) {
  fail(
    `--timeout must be a positive number of milliseconds, got ${TIMEOUT_MS}`
  );
}
if (!['auto', 'hardware', 'swiftshader'].includes(ADAPTER)) {
  fail(`--adapter must be auto, hardware or swiftshader, got "${ADAPTER}"`);
}

const pause = (ms) => new Promise((ok) => setTimeout(ok, ms));

// ---------------------------------------------------------------------------
// The static server
// ---------------------------------------------------------------------------
//
// `file://` is not an option and neither is "assume python3": ES modules,
// `WebAssembly.instantiateStreaming`, OPFS and WebGPU all want a real origin,
// and `localhost` is the one origin a browser treats as secure without a
// certificate.
//
// It lives in `serve.mjs` because `web/build.sh --serve` runs the same code:
// that server sends the COOP/COEP pair, group A asserts the isolation those
// headers buy, and sharing the file is what stops the gate proving something
// about an origin no human ever loads.

/**
 * The control page group A uses to prove this browser can report canvas pixels.
 *
 * Deliberately the smallest WebGPU program there is — configure a context and
 * clear — so that a failure here cannot be anything the engine did. It is
 * served from the harness's own origin rather than a `data:` URL because a
 * `data:` URL is an opaque origin and opaque origins are not secure contexts,
 * which is the one thing WebGPU insists on.
 */
const CONTROL_PAGE = `<!doctype html>
<meta charset="utf-8">
<title>crcbl readback control</title>
<canvas id="canvas" width="64" height="64"></canvas>
<script type="module">
globalThis.controlError = null;
globalThis.controlFrames = 0;
try {
  const canvas = document.getElementById('canvas');
  const adapter = await navigator.gpu.requestAdapter();
  const device = await adapter.requestDevice();
  const context = canvas.getContext('webgpu');
  context.configure({
    device,
    format: navigator.gpu.getPreferredCanvasFormat(),
    alphaMode: 'opaque',
  });
  const draw = () => {
    const encoder = device.createCommandEncoder();
    encoder
      .beginRenderPass({
        colorAttachments: [
          {
            view: context.getCurrentTexture().createView(),
            loadOp: 'clear',
            storeOp: 'store',
            // sRGB-encoded ${CONTROL_RGB.join(', ')} once the canvas is read back.
            clearValue: { r: 0, g: 0.2, b: 0.8, a: 1 },
          },
        ],
      })
      .end();
    device.queue.submit([encoder.finish()]);
    globalThis.controlFrames += 1;
    requestAnimationFrame(draw);
  };
  requestAnimationFrame(draw);
} catch (error) {
  globalThis.controlError = String(error);
}
</script>`;

/** The routes `serve` answers itself, rather than from the site directory. */
const CONTROL_ROUTES = {
  [CONTROL_PATH]: { contentType: MIME['.html'], body: CONTROL_PAGE },
};

// ---------------------------------------------------------------------------
// The browser
// ---------------------------------------------------------------------------

/**
 * The first browser binary on this machine that could drive WebGPU.
 *
 * Named rather than guessed at call time, so a miss says what was looked for.
 * `google-chrome` comes first because that is what GitHub's Ubuntu images ship
 * as a real binary; a `chromium` that is a snap wrapper cannot see a
 * `--user-data-dir` under `/tmp`, which is a miserable failure to debug from a
 * CI log.
 */
function findBrowser() {
  const explicit = process.env.CRCBL_CHROMIUM;
  if (explicit) {
    if (!existsSync(explicit))
      fail(`CRCBL_CHROMIUM=${explicit} does not exist`);
    return explicit;
  }
  const candidates = [
    'google-chrome',
    'google-chrome-stable',
    'chromium',
    'chromium-browser',
  ];
  for (const name of candidates) {
    for (const dir of (process.env.PATH ?? '').split(':')) {
      if (dir && existsSync(join(dir, name))) return join(dir, name);
    }
  }
  return fail(
    `no browser found. Tried ${candidates.join(', ')} on PATH.\n` +
      '  Set CRCBL_CHROMIUM to a Chromium or Chrome binary with WebGPU support.'
  );
}

/**
 * The flags, and why each one is here.
 *
 * Every one of these was measured rather than copied — on Chromium 150 first,
 * and the SwiftShader set again on 151. Without the WebGPU pair for the chosen
 * mode, `navigator.gpu.requestAdapter()` resolves to `null` in headless and the
 * demo stops at its own "this browser has no WebGPU" banner.
 */
function browserFlags(profile, mode) {
  const flags = [
    // Modern headless. The old one is a separate browser with no GPU stack at
    // all, so WebGPU is simply absent there. `CRCBL_WEB_E2E_HEADED=1` drops it
    // for a run inside Xvfb, which is worth trying when a machine's headless
    // compositor refuses to hand canvas pixels back.
    ...(process.env.CRCBL_WEB_E2E_HEADED === '1' ? [] : ['--headless=new']),
    // Port 0 and read it back from the profile, rather than picking a number
    // and hoping. Two runs on one machine must not collide.
    '--remote-debugging-port=0',
    `--user-data-dir=${profile}`,
    '--no-first-run',
    '--no-default-browser-check',
    // Chrome's default /dev/shm is small in containers and the renderer dies
    // with an unhelpful crash when it fills.
    '--disable-dev-shm-usage',
    // Nothing here needs the network beyond localhost, and the component
    // updater's failures are noise in the log this harness prints on failure.
    '--disable-background-networking',
    '--disable-component-update',
    '--disable-extensions',
    // The canvas is sized by CSS against the viewport, so a fixed window makes
    // the pixel counts below mean the same thing on every machine.
    '--window-size=1024,768',
    // THIS GATE BOOTS REAL GAMES AND PRESSES REAL KEYS, so it plays their cues
    // out of the machine's speakers — a launched ball, a broken brick, once per
    // demo per run. Nothing in this driver asserts anything about audio, so
    // muting the output costs no coverage; the `AudioContext` and the worklet
    // still run, which is what `smoke.mjs` and the shim's own checks care
    // about. It is muted here rather than in the engine because the noise is a
    // property of running the harness, not of the build under test.
    '--mute-audio',
  ];

  if (mode === 'hardware') {
    // Without these two the GPU process falls back to ANGLE's SwiftShader GL
    // and `chrome://gpu` reports `webgpu: unavailable_software`; with them it
    // reports `webgpu: enabled` and the adapter is the real device.
    flags.push('--enable-features=Vulkan', '--use-angle=vulkan');
  } else {
    // `--use-webgpu-adapter=swiftshader` alone is not enough: Chrome refuses
    // WebGPU when the GPU feature status is `unavailable_software`, which is
    // exactly what a headless run without a display reports.
    // `--enable-unsafe-webgpu` is what lifts that refusal.
    //
    // The other two point the *shared image* device at SwiftShader too, and on
    // Chrome 151 they are what makes this mode work at all. A canvas is handed
    // between two devices — Dawn renders into it, and the compositor reads it
    // back out for `toDataURL` — and those two have to be the same Vulkan
    // implementation. `--use-webgpu-adapter=swiftshader` moves only Dawn; the
    // shared-image device stays on whatever the machine has, and Chrome then
    // fails to hand the texture across:
    //
    //   AssociateMailbox: Accessing an uncleared texture requires passing a
    //   usage that supports lazy clearing
    //   GPUDevice: [Invalid Texture] is invalid … While validating
    //   CopyTextureForBrowser
    //
    // The canvas snapshot is uninitialised memory after that — largely
    // zero-alpha, which is what makes it read as transparent black. Measured on
    // Chromium 151, and neither flag is enough on its own: with only the
    // feature, `chrome://gpu` still names the machine's own GL driver.
    flags.push(
      '--enable-unsafe-webgpu',
      '--use-webgpu-adapter=swiftshader',
      '--enable-features=Vulkan',
      '--use-vulkan=swiftshader'
    );
  }

  // Chrome's sandbox needs user namespaces, which a root-in-container CI job
  // usually cannot have. Opt in on the condition rather than always: a
  // sandboxed browser is the configuration a visitor runs.
  if (
    process.env.CRCBL_CHROMIUM_NO_SANDBOX === '1' ||
    process.getuid?.() === 0
  ) {
    flags.push('--no-sandbox');
  }

  // An escape hatch for the machine this was not written on. Chromium's GPU
  // flags are the part of this harness most likely to need one more switch on a
  // runner nobody here has, and the alternative to an escape hatch is a patched
  // copy of this file. Printed with the rest of the command line, so a run that
  // used one says so.
  const extra = (process.env.CRCBL_CHROMIUM_FLAGS ?? '')
    .split(' ')
    .filter(Boolean);
  return [...flags, ...extra];
}

/**
 * Starts the browser and returns its DevTools endpoint.
 *
 * The endpoint comes from `DevToolsActivePort`, which Chrome writes into the
 * profile once it is listening. Polling that file is how the launch is
 * synchronised: a sleep would be a flake on a slow machine and wasted time on a
 * fast one.
 */
async function launch(binary, mode) {
  const profile = mkdtempSync(join(tmpdir(), 'crcbl-web-e2e-'));
  const flags = browserFlags(profile, mode);
  const child = spawn(binary, [...flags, 'about:blank'], {
    stdio: ['ignore', 'ignore', 'pipe'],
    // Its own process group, so `stop` can kill the whole tree. Chromium is
    // half a dozen processes and a `kill` aimed at the parent leaves the GPU
    // process and the zygotes behind when the parent is wedged — which is how
    // this harness left three Chromiums on the machine that wrote it.
    detached: true,
    env: {
      ...process.env,
      // A developer's `~/.config/chromium-flags.conf` is read by the launcher
      // on some distributions and appended to the command line. On the machine
      // this was written on it set `--ozone-platform=wayland`, which in a
      // headless run takes the GPU process down with it and hides WebGPU
      // entirely — indistinguishable from a browser that has no WebGPU.
      // Pointing XDG_CONFIG_HOME at the throwaway profile makes the run depend
      // on the flags above and nothing else.
      XDG_CONFIG_HOME: profile,
    },
  });

  /** Chrome's own diagnostics. Printed only when something fails. */
  const stderr = [];
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', (chunk) => {
    for (const line of chunk.split('\n'))
      if (line.trim()) stderr.push(line.trimEnd());
  });

  let exited = null;
  child.on('exit', (code, signal) => {
    exited = signal ? `signal ${signal}` : `exit ${code}`;
  });

  const browser = {
    child,
    stderr,
    flags,
    mode,
    endpoint: '',
    stop() {
      running.delete(browser);
      try {
        // Negative pid: the process *group* created by `detached`. `SIGKILL`
        // rather than `SIGTERM` because there is nothing to save — the profile
        // is thrown away on the next line — and a Chromium that ignores the
        // polite signal is exactly the one worth being rude to.
        process.kill(-child.pid, 'SIGKILL');
      } catch {
        // Already gone, which is the outcome this wanted.
      }
      rmSync(profile, { recursive: true, force: true });
    },
  };
  running.add(browser);

  const portFile = join(profile, 'DevToolsActivePort');
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (exited) {
      console.error(stderr.join('\n'));
      fail(`the browser stopped before it listened (${exited})`);
    }
    if (existsSync(portFile)) {
      const [port, path] = readFileSync(portFile, 'utf8').split('\n');
      if (port && path) {
        browser.endpoint = `ws://127.0.0.1:${port}${path}`;
        return browser;
      }
    }
    await pause(50);
  }
  console.error(stderr.join('\n'));
  return fail('the browser never wrote DevToolsActivePort');
}

// ---------------------------------------------------------------------------
// A Chrome DevTools Protocol client, in about forty lines
// ---------------------------------------------------------------------------
//
// Node 22 shipped a global `WebSocket`, so a CDP session needs no library. The
// protocol is JSON both ways: `{ id, method, params }` out, `{ id, result }` or
// `{ method, params }` back.

class Cdp {
  #socket;
  #next = 0;
  #pending = new Map();
  /** @type {Map<string, Array<(params: any) => void>>} */
  #listeners = new Map();

  static async connect(url) {
    const client = new Cdp();
    client.#socket = new WebSocket(url);
    await new Promise((ok, no) => {
      client.#socket.onopen = ok;
      client.#socket.onerror = () => no(new Error(`cannot reach ${url}`));
    });
    client.#socket.onmessage = (event) =>
      client.#dispatch(JSON.parse(event.data));
    return client;
  }

  #dispatch(message) {
    if (message.id !== undefined) {
      const slot = this.#pending.get(message.id);
      if (!slot) return;
      this.#pending.delete(message.id);
      if (message.error)
        slot.reject(
          new Error(`${message.error.message} (${message.error.code})`)
        );
      else slot.resolve(message.result);
      return;
    }
    for (const handler of this.#listeners.get(message.method) ?? [])
      handler(message.params);
  }

  on(method, handler) {
    if (!this.#listeners.has(method)) this.#listeners.set(method, []);
    this.#listeners.get(method).push(handler);
  }

  send(method, params = {}) {
    this.#next += 1;
    const id = this.#next;
    return new Promise((resolve, reject) => {
      this.#pending.set(id, { resolve, reject });
      this.#socket.send(JSON.stringify({ id, method, params }));
    });
  }

  close() {
    this.#socket.close();
  }
}

/**
 * Opens a fresh tab and attaches to it.
 *
 * Fresh, rather than the one the command line opened: the browser's *first*
 * renderer is created before the GPU process has finished reporting what it can
 * do, and a page loaded into it can miss `navigator.gpu` entirely even when the
 * browser has it. An hour went into chasing flags for that symptom.
 */
async function openPage(browser) {
  const control = await Cdp.connect(browser.endpoint);
  const created = await control.send('Target.createTarget', {
    url: 'about:blank',
  });
  control.close();
  return Cdp.connect(
    browser.endpoint.replace(
      /\/devtools\/browser\/.*$/,
      `/devtools/page/${created.targetId}`
    )
  );
}

/**
 * Evaluates `expression` in the page and returns its value.
 *
 * `awaitPromise` is on for everything, so an `async` IIFE works; anything that
 * throws comes back as a rejection here rather than as `undefined`, because a
 * check that silently reads `undefined` is a check that passes for the wrong
 * reason.
 */
async function evaluate(page, expression) {
  const result = await page.send('Runtime.evaluate', {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (result.exceptionDetails) {
    const details = result.exceptionDetails;
    throw new Error(
      details.exception?.description ?? details.text ?? 'evaluation threw'
    );
  }
  return result.result.value;
}

/**
 * Polls `probe` until it returns something truthy, or the deadline passes.
 *
 * `docs/plan/ROADMAP.md`: "Poll for the condition, never sleep." The interval is
 * one frame at 60 Hz, so a condition that becomes true on a rAF tick is seen on
 * the next one.
 */
async function until(probe, timeout = TIMEOUT_MS) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    // A probe that throws is a condition not met yet — `crcbl` does not exist
    // until the module has loaded — rather than a reason to abandon the run.
    let value;
    try {
      value = await probe();
    } catch {
      value = null;
    }
    if (value) return value;
    await pause(16);
  }
  return null;
}

// ---------------------------------------------------------------------------
// Reading the canvas back
// ---------------------------------------------------------------------------

/**
 * The expression that samples a canvas, evaluated in the page.
 *
 * `toDataURL()` and not `drawImage(canvas, …)`: see the header. The PNG is
 * decoded back through an `Image` so the pixels can be counted, which is a
 * round trip through the browser's own encoder and decoder and therefore
 * lossless for the 8-bit RGBA a canvas holds.
 *
 * The summary is deliberately small — a full frame is two megabytes and the
 * checks only need "is it one colour", "which colours" and "did it change".
 * Colours are quantised to 5 bits per channel so two rasterisers disagreeing in
 * the last bit does not read as a different frame.
 */
const SAMPLE_CANVAS = (selector) => `(async () => {
  const canvas = document.querySelector(${JSON.stringify(selector)});
  if (!canvas || !canvas.width || !canvas.height) return null;
  const image = new Image();
  image.src = canvas.toDataURL();
  await image.decode();
  const scratch = document.createElement('canvas');
  scratch.width = canvas.width;
  scratch.height = canvas.height;
  const context = scratch.getContext('2d', { willReadFrequently: true });
  context.drawImage(image, 0, 0);
  const pixels = context.getImageData(0, 0, scratch.width, scratch.height).data;
  const histogram = new Map();
  let hash = 2166136261;
  for (let i = 0; i < pixels.length; i += 4) {
    const key = ((pixels[i] >> 3) << 10) | ((pixels[i + 1] >> 3) << 5) | (pixels[i + 2] >> 3);
    histogram.set(key, (histogram.get(key) ?? 0) + 1);
    hash = Math.imul(hash ^ pixels[i], 16777619);
    hash = Math.imul(hash ^ pixels[i + 1], 16777619);
    hash = Math.imul(hash ^ pixels[i + 2], 16777619);
  }
  const ranked = [...histogram.entries()].sort((a, b) => b[1] - a[1]);
  const total = pixels.length / 4;
  return {
    width: scratch.width,
    height: scratch.height,
    distinct: histogram.size,
    hash: hash >>> 0,
    top: ranked.slice(0, 5).map(([key, count]) => ({
      rgb: [((key >> 10) & 31) << 3, ((key >> 5) & 31) << 3, (key & 31) << 3],
      share: count / total,
    })),
  };
})()`;

const describe = (sample) =>
  sample.top
    .map(
      ({ rgb, share }) => `rgb(${rgb.join(',')}) ${(share * 100).toFixed(1)}%`
    )
    .join(', ');

/**
 * Where breakout's paddle is, as a fraction of the canvas's width.
 *
 * **The frame is the observable**, because the HUD line is not: it carries the
 * *ball's* x and the ball is pinned at its start position while the paddle is
 * being dragged, so nothing the game logs moves when the finger does. What the
 * drag is supposed to produce is a paddle somewhere else on screen, and that is
 * what this reads.
 *
 * The classifier is "strongly blue in the bottom band": the paddle is the only
 * thing down there that is, with the background near-black, the walls grey and
 * the ball white — all three of which fail `blue − red`. `count` comes back with
 * the answer so a check can insist the paddle was actually found rather than
 * pass on an empty band.
 */
const SAMPLE_PADDLE = (selector) => `(async () => {
  const canvas = document.querySelector(${JSON.stringify(selector)});
  if (!canvas || !canvas.width || !canvas.height) return null;
  const image = new Image();
  image.src = canvas.toDataURL();
  await image.decode();
  const scratch = document.createElement('canvas');
  scratch.width = canvas.width;
  scratch.height = canvas.height;
  const context = scratch.getContext('2d', { willReadFrequently: true });
  context.drawImage(image, 0, 0);
  const top = Math.floor(scratch.height * ${PADDLE_BAND});
  const pixels = context
    .getImageData(0, top, scratch.width, scratch.height - top)
    .data;
  let sum = 0;
  let count = 0;
  for (let i = 0; i < pixels.length; i += 4) {
    if (pixels[i + 2] - pixels[i] >= 50 && pixels[i + 1] - pixels[i] >= 30) {
      sum += (i / 4) % scratch.width;
      count += 1;
    }
  }
  return { count, at: count ? sum / count / scratch.width : null };
})()`;

// ---------------------------------------------------------------------------
// The pre-flight: can this browser render *and* report pixels?
// ---------------------------------------------------------------------------

/**
 * Runs the control page under one adapter mode.
 *
 * Returns what happened rather than asserting, because `--adapter auto` uses
 * the answer to choose a mode and the checks below report whichever one won.
 */
async function preflight(binary, mode, origin) {
  const browser = await launch(binary, mode);
  try {
    const page = await openPage(browser);
    await page.send('Runtime.enable');
    await page.send('Page.enable');
    await page.send('Page.navigate', { url: `${origin}${CONTROL_PATH}` });

    const platform = await until(async () =>
      evaluate(
        page,
        `(async () => {
           if (document.readyState !== 'complete') return null;
           const out = {
             gpu: 'gpu' in navigator,
             secure: isSecureContext,
             // Cross-origin isolation, asked of the document rather than
             // inferred from the fact that serve.mjs was told to send the
             // headers. Two spellings because they can fail apart: the flag is
             // what the headers buy, and a shared WebAssembly.Memory is the
             // capability a +atomics build actually reaches for.
             isolated: globalThis.crossOriginIsolated === true,
             sharedMemory: (() => {
               try {
                 return new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true })
                   .buffer instanceof SharedArrayBuffer;
               } catch (error) {
                 return String(error);
               }
             })(),
           };
           if (!out.gpu) return out;
           const adapter = await navigator.gpu.requestAdapter();
           if (!adapter) return { ...out, adapter: null };
           const info = adapter.info ?? {};
           out.adapter = [info.vendor, info.architecture, info.device, info.description]
             .filter(Boolean).join(' ') || 'unnamed';
           return out;
         })()`
      )
    );
    if (!platform?.gpu || !platform?.adapter) {
      return {
        mode,
        ...platform,
        readback: null,
        error: platform?.gpu ? 'no adapter' : 'no navigator.gpu',
      };
    }

    // The control has to have drawn before its pixels mean anything.
    const drew = await until(
      async () => (await evaluate(page, `globalThis.controlFrames ?? 0`)) > 2
    );
    const error = await evaluate(page, `globalThis.controlError`);
    if (error) return { mode, ...platform, readback: null, error };
    if (!drew)
      return {
        mode,
        ...platform,
        readback: null,
        error: 'the control never drew a frame',
      };

    const sample = await evaluate(page, SAMPLE_CANVAS('#canvas'));
    // One clear, so one colour, and it must be the one the control asked for.
    // Within 4 per channel: the canvas format may be 8-bit sRGB either way
    // round and the PNG round trip is exact, but a rasteriser is allowed the
    // last bit.
    const seen = sample?.top?.[0]?.rgb ?? [-1, -1, -1];
    const matches =
      sample &&
      sample.top[0].share > 0.99 &&
      seen.every((value, i) => Math.abs(value - CONTROL_RGB[i]) <= 8);
    return {
      mode,
      ...platform,
      readback: { matches, seen, sample },
      error: null,
    };
  } finally {
    browser.stop();
  }
}

// ---------------------------------------------------------------------------
// The checks
// ---------------------------------------------------------------------------

/** @type {{ group: string, name: string, ok: boolean, detail: string }[]} */
const checks = [];

function check(group, name, ok, detail = '') {
  checks.push({ group, name, ok: Boolean(ok), detail });
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? ` — ${detail}` : ''}`
  );
  return Boolean(ok);
}

function group(name) {
  console.log(`\nweb e2e: ${name}`);
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

mkdirSync(OUT, { recursive: true });

const site = await serve(SITE, { routes: CONTROL_ROUTES });
const binary = findBrowser();

/** Everything the page logged, in order, so a failure can print it. */
const consoleLines = [];
/** WebGPU device errors, which Chrome reports through the Log domain. */
const deviceErrors = [];
/** Uncaught exceptions in page JS. */
const pageErrors = [];

let browser = null;
let exitCode = 1;

try {
  console.log(`web e2e: browser ${binary}`);
  console.log(`web e2e: serving ${SITE} at ${site.origin}`);
  console.log(`web e2e: adapter mode "${ADAPTER}"`);

  group('A — the platform');

  // Every mode that will be tried, in order of preference. `hardware` first:
  // it is what a visitor to the Pages URL gets. A CI runner has no GPU, so it
  // gets no adapter there and falls through to `swiftshader` on the next line.
  const modes = ADAPTER === 'auto' ? ['hardware', 'swiftshader'] : [ADAPTER];
  /** @type {Awaited<ReturnType<typeof preflight>> | null} */
  let chosen = null;
  const attempts = [];
  for (const mode of modes) {
    const result = await preflight(binary, mode, site.origin);
    attempts.push(result);
    console.log(
      `  ..   control page under "${mode}": ` +
        `adapter ${result.adapter ?? 'none'}, ` +
        `readback ${result.readback ? (result.readback.matches ? 'ok' : `rgb(${result.readback.seen.join(',')})`) : (result.error ?? 'n/a')}`
    );
    if (result.readback?.matches) {
      chosen = result;
      break;
    }
  }

  const best = chosen ?? attempts.at(-1);

  // Isolation first, and above the readback gate on purpose: it is a property
  // of the *origin* rather than of the GPU, it is the one thing here that
  // `web/tools/serve.mjs` alone decides, and a browser that cannot report
  // canvas pixels can still answer it. Below the gate it would be skipped by
  // the `throw` on exactly the machines where nobody would notice.
  check(
    'A',
    'the document is cross-origin isolated',
    best?.isolated,
    best?.isolated
      ? Object.entries(ISOLATION_HEADERS)
          .map(([name, value]) => `${name}: ${value}`)
          .join('; ')
      : 'crossOriginIsolated is false — no SharedArrayBuffer, so a wasm build ' +
          'with +atomics cannot start a worker on this origin'
  );
  check(
    'A',
    'a shared WebAssembly.Memory can be constructed',
    best?.sharedMemory === true,
    best?.sharedMemory === true
      ? 'its buffer is a SharedArrayBuffer'
      : String(best?.sharedMemory)
  );

  check(
    'A',
    'the browser exposes navigator.gpu',
    best?.gpu,
    best?.gpu ? '' : 'the demo would show its no-WebGPU banner'
  );
  check(
    'A',
    'a WebGPU adapter is granted',
    best?.adapter,
    best?.adapter ?? best?.error ?? 'requestAdapter() resolved null'
  );
  const readable = check(
    'A',
    'a known-colour clear reads back as that colour',
    chosen,
    chosen
      ? `${chosen.mode} adapter, rgb(${chosen.readback.seen.join(',')})`
      : `tried ${modes.join(', ')}; none returned pixels — group D could not tell a rendered frame from a blank one`
  );

  if (!readable) {
    throw new Error(
      'this browser cannot report canvas pixels with any adapter mode; refusing to run the render checks, ' +
        'because passing them would mean nothing and failing them would blame the engine'
    );
  }

  console.log(
    `\nweb e2e: running against the "${chosen.mode}" adapter — ${chosen.adapter}`
  );

  browser = await launch(binary, chosen.mode);
  console.log(
    `web e2e: flags ${browser.flags.filter((f) => !f.startsWith('--user-data-dir')).join(' ')}`
  );

  const page = await openPage(browser);
  page.on('Runtime.consoleAPICalled', ({ type, args: values }) => {
    consoleLines.push(
      `[${type}] ${values.map((v) => v.value ?? v.description ?? '').join(' ')}`
    );
  });
  page.on('Runtime.exceptionThrown', ({ exceptionDetails }) => {
    pageErrors.push(
      exceptionDetails.exception?.description ?? exceptionDetails.text
    );
  });
  page.on('Log.entryAdded', ({ entry }) => {
    consoleLines.push(`[${entry.source}.${entry.level}] ${entry.text}`);
    // Dawn reports a rejected shader, an invalid pipeline or a refused submit
    // through the device's error callback, and Chrome surfaces those here.
    // `wgpu` does not turn any of them into a `Result`, so this is the only
    // place a harness can see them at all.
    if (entry.source === 'rendering' && entry.level !== 'info')
      deviceErrors.push(entry.text);
  });
  await page.send('Runtime.enable');
  await page.send('Log.enable');
  await page.send('Page.enable');

  const url = `${site.origin}/${DEMO}`;
  await page.send('Page.navigate', { url });
  await until(async () => evaluate(page, `document.readyState === 'complete'`));

  const ready = await until(async () =>
    evaluate(page, `Boolean(globalThis.crcbl)`)
  );
  check(
    'A',
    'the shim loads and publishes its debug handle',
    ready,
    ready ? url : 'globalThis.crcbl never appeared'
  );

  const banner = await evaluate(
    page,
    `document.getElementById('status')?.textContent ?? ''`
  );
  check(
    'A',
    'the page raised no uncaught exception',
    pageErrors.length === 0,
    pageErrors[0] ?? `page says "${banner}"`
  );

  group('B — the engine boots');

  const said = (needle) => consoleLines.find((line) => line.includes(needle));

  const settled = await until(async () => {
    const status = await evaluate(page, `crcbl.status()`);
    // 3 is STATUS_RUNNING, 6 STATUS_PAUSED; 4 STOPPED and 5 FAILED are
    // terminal, so stop asking. 6 is here because a headless Chrome that never
    // gave the canvas focus is a legitimate way to arrive, and it settles the
    // wait rather than spinning it out — the check below still insists on 3,
    // and reports the 6 instead of "never settled" when it is not.
    return [3, 4, 5, 6].includes(status) ? { status } : null;
  });

  check(
    'B',
    'the shell reported the canvas size',
    said('shell: first configure'),
    said('shell: first configure')?.trim() ?? 'no configure line'
  );
  // The engine logs `hal: <backend> adapter …` at open, where `<backend>` is
  // `Instance::backend()` — `wgpu` normally, `webgpu` when the site was built
  // with the `webgpu` feature. Reading the backend's own line is what makes this
  // a check that the *right* backend opened the device rather than that some
  // backend did.
  const backendAdapterLine = WEBGPU_MODE
    ? 'hal: webgpu adapter'
    : 'hal: wgpu adapter';
  check(
    'B',
    `the ${WEBGPU_MODE ? 'webgpu' : 'wgpu'} backend opened a device`,
    said(backendAdapterLine),
    said(backendAdapterLine)?.trim() ?? 'no adapter line'
  );
  check(
    'B',
    'a swapchain was created',
    said('hal: swapchain'),
    said('hal: swapchain')?.trim() ?? 'no swapchain line'
  );
  const isRunning = check(
    'B',
    'the demo reached STATUS_RUNNING',
    settled?.status === 3,
    settled?.status === 6
      ? 'status 6 (PAUSED) — the canvas never had focus'
      : `status ${settled?.status ?? 'never settled'}`
  );
  // The pages declare `/favicon.svg`, which exists, so this filter is now the
  // belt to that braces: a browser that ignores the declaration and asks for
  // `/favicon.ico` anyway is not the page wanting an asset. Every other 404 is.
  const missing = site.misses.filter((m) => !m.endsWith('favicon.ico'));
  check(
    'B',
    'every asset the page asked for exists',
    missing.length === 0,
    missing.join(', ')
  );

  if (!isRunning) {
    const detail = await evaluate(
      page,
      `document.getElementById('status')?.textContent + ' | ' + document.getElementById('detail')?.textContent`
    );
    throw new Error(`the demo never started running: ${detail}`);
  }

  // The focus half runs for every demo — a canvas that cannot take the keyboard
  // is a paused demo whatever it is — and the key half only where there is a key.
  group(
    EXPECTED.key
      ? 'C — input drives the simulation'
      : 'C — the simulation drives itself'
  );

  const hud = () => consoleLines.filter((line) => line.includes('[HUD]'));
  await until(async () => hud().length > 0);
  const beforeLaunch = hud().length;
  check(
    'C',
    'the game reports its state before any input',
    beforeLaunch > 0 && EXPECTED.waiting(hud()[0]),
    hud()[0]?.trim() ?? 'no HUD line'
  );

  // A real click, dispatched through the browser's own input pipeline rather
  // than by calling `canvas.focus()` from script. That is the point: it
  // exercises the shim's `pointerdown` listener, which is what a player's click
  // does and what hands the canvas the keyboard.
  //
  // Scroll it into view first, and read the rect *after*. `Input.dispatch-
  // MouseEvent` takes viewport coordinates and `getBoundingClientRect` returns
  // them, so a canvas sitting below the fold yields a y outside the viewport
  // and the click lands on nothing — which presents identically to the shim
  // having no pointer listener at all. A page redesign that moves the canvas
  // down the document is not a regression in the thing this checks.
  //
  // **The corner, not the centre**, for the reason group E already clicks a
  // corner: a start screen is laid out centred, so the middle of the canvas is
  // its first button. Here that button is horde's `PLAY`, which is bound to the
  // same edge as `Space` — so a centred click started the run and the `Space`
  // below then *restarted* it, leaving the demo on the title screen of run 2
  // with a clock frozen at 0.0 for the rest of the session. That is what check
  // C and check D were reporting, correctly, about two Pages runs in three.
  //
  // Group E learned this against the pause menu and `RESUME`; the same fix
  // never reached here, and only horde made it visible, because only horde's
  // centre button destroys the run rather than being idempotent.
  const rect = await evaluate(
    page,
    `(() => { const c = document.getElementById('canvas');
              c.scrollIntoView({ block: 'center', behavior: 'instant' });
              const r = c.getBoundingClientRect();
              return { x: Math.round(r.x + ${FOCUS_CLICK_INSET}), y: Math.round(r.y + ${FOCUS_CLICK_INSET}),
                       left: r.x, top: r.y }; })()`
  );
  const inViewport = await evaluate(
    page,
    `(() => { const r = document.getElementById('canvas').getBoundingClientRect();
              const cy = r.y + ${FOCUS_CLICK_INSET}, cx = r.x + ${FOCUS_CLICK_INSET};
              return cy >= 0 && cy <= innerHeight && cx >= 0 && cx <= innerWidth; })()`
  );
  check(
    'C',
    'the point that will be clicked for focus is inside the viewport',
    inViewport === true,
    `(${rect.x}, ${rect.y}) is ${inViewport === true ? 'inside' : 'outside'} the window`
  );
  for (const type of ['mousePressed', 'mouseReleased']) {
    await page.send('Input.dispatchMouseEvent', {
      type,
      x: rect.x,
      y: rect.y,
      button: 'left',
      clickCount: 1,
      buttons: type === 'mousePressed' ? 1 : 0,
    });
  }
  const focused = await evaluate(page, `document.activeElement?.id ?? ''`);
  check(
    'C',
    'a click gives the canvas keyboard focus',
    focused === 'canvas',
    `activeElement is "${focused}"`
  );

  // Everything from here to the `moving` check is about the start key, so a
  // demo that has none skips it rather than dispatching a key that means
  // nothing and asserting it did something. `EXPECTATIONS` says which.
  if (EXPECTED.key) {
    // **The focusing click must not also press a button**, which is the check
    // whose absence let the centred click above survive. Without it, a click
    // that starts the game reads as a pass here and then poisons the `Space`
    // below — the key starts a run that is already running, and in horde that
    // is a restart. Read after the click and before any key, so the only thing
    // it can be reporting on is the click.
    await until(async () => hud().length > beforeLaunch || null);
    const afterFocusClick = hud().at(-1) ?? '';
    check(
      'C',
      'the focusing click pressed no button',
      EXPECTED.waiting(afterFocusClick),
      afterFocusClick.trim() || 'no HUD line after the click'
    );

    // The key the demo's own instructions name. `code` is what the engine binds
    // to; `key` and the virtual key codes are what a real keyboard sends — and
    // they are spelled for Space, which is what every keyed row asks for. A row
    // naming a different key has to bring its own three values with it.
    for (const type of ['keyDown', 'keyUp']) {
      await page.send('Input.dispatchKeyEvent', {
        type,
        code: EXPECTED.key,
        key: ' ',
        windowsVirtualKeyCode: 32,
        nativeVirtualKeyCode: 32,
        ...(type === 'keyDown' ? { text: ' ' } : {}),
      });
    }

    const launched = await until(async () =>
      hud()
        .slice(beforeLaunch)
        .find((line) => EXPECTED.started(line))
    );
    check(
      'C',
      EXPECTED.startedLabel,
      Boolean(launched),
      (launched ?? EXPECTED.startedFailure).trim()
    );
  }

  // The ball's x is in every HUD line, and two different values is the
  // simulation advancing under its own steam — which nothing on the JS side
  // could fake. The one check in this group every demo makes, including the one
  // that took no key to get here.
  const positions = await until(async () => {
    const seen = new Set(
      hud()
        .slice(beforeLaunch)
        .map((line) => line.match(EXPECTED.moving)?.[1])
        .filter(Boolean)
    );
    return seen.size > 1 ? seen : null;
  });
  check(
    'C',
    EXPECTED.movingLabel,
    Boolean(positions),
    positions
      ? `it took ${positions.size} values: ${[...positions].join(', ')}`
      : 'it never changed'
  );

  group('D — it renders');

  // Sampled repeatedly rather than twice: a single pair could catch two frames
  // that happen to match, and the count of distinct frames is a more useful
  // number in a failure report than a boolean.
  const samples = [];
  for (let i = 0; i < SAMPLE_COUNT; i += 1) {
    const sample = await evaluate(page, SAMPLE_CANVAS('#canvas'));
    if (sample) samples.push(sample);
    await pause(50);
  }

  const last = samples.at(-1);
  check(
    'D',
    'the canvas has a backing store',
    last && last.width > 0 && last.height > 0,
    last ? `${last.width}x${last.height}` : 'no canvas'
  );

  // NOTHING IS FILTERED OUT OF THIS. It used to drop Chrome's "configured with
  // a different format than is preferred by this device" warning, which
  // `crcbl-webgpu` earned on every frame by offering `Bgra8Unorm` first while
  // the browser preferred `rgba8unorm`. That backend now fetches
  // `getPreferredCanvasFormat()` during its instance-open and reports it first,
  // so the warning is not produced — and the filter that made it invisible is
  // gone with it, because the whole cost of that warning is a full-canvas copy
  // per frame that nothing else here would notice.
  check(
    'D',
    'the browser reported no WebGPU device errors',
    deviceErrors.length === 0,
    deviceErrors.length
      ? `${deviceErrors.length} error(s); first: ${deviceErrors[0].split('\n')[0]}`
      : ''
  );

  check(
    'D',
    'the canvas is not one flat colour',
    last && last.distinct > 1,
    last
      ? `${last.distinct} distinct colour(s): ${describe(last)}`
      : 'nothing sampled'
  );

  const frames = new Set(samples.map((s) => s.hash));
  check(
    'D',
    'the canvas changes between frames while the simulation runs',
    frames.size > 1,
    `${frames.size} distinct frame(s) across ${samples.length} samples`
  );

  group('E — focus and pause');

  // **The reported bug, in a real browser.** A canvas that loses keyboard focus
  // used to keep simulating and keep saying "Playing." — for a game sitting
  // behind another window, that is a life lost while nobody is looking.
  //
  // The blur is real, not synthesized: moving focus to another element in the
  // document fires a `FocusEvent` at the canvas exactly as clicking outside it
  // does, which is the shim listener under test. There is no way to make the
  // browser blur an element through `Input.dispatch*` — Tab is swallowed by the
  // shim precisely so the page does not steal the game's keys.
  //
  // **What is measured is the HUD heartbeat**, which both samples log every
  // sixty ticks, whatever state the game is in. The canvas would be the more
  // direct observable and was tried first: breakout's ball dies within a second
  // of the last paddle input, so a still board is the *normal* state by this
  // point in the run and "the picture stopped changing" passes whether or not
  // anything paused. A line that only the tick loop can emit does not.
  const heartbeats = async () => {
    const before = hud().length;
    await pause(TICK_WINDOW_MS);
    return hud().length - before;
  };

  // The control, and it is what makes the next check mean something: if the
  // window below is ever too short to contain a heartbeat, this fails loudly
  // instead of the pause check passing for free.
  const running = await heartbeats();
  check(
    'E',
    'a running demo logs its HUD from inside the tick',
    running > 0,
    `${running} HUD line(s) in ${TICK_WINDOW_MS} ms`
  );

  await evaluate(page, `document.getElementById('stop').focus()`);
  const paused = await until(async () => {
    const status = await evaluate(page, `crcbl.status()`);
    return status === 6 ? status : null;
  });
  check(
    'E',
    'blurring the canvas pauses the demo',
    paused === 6,
    `status ${await evaluate(page, `crcbl.status()`)}`
  );

  const whilePaused = await heartbeats();
  check(
    'E',
    'a paused demo runs no ticks at all',
    whilePaused === 0,
    `${whilePaused} HUD line(s) in ${TICK_WINDOW_MS} ms`
  );

  // Clicking back in deliberately does not resume — the pause menu is dismissed
  // on purpose — so this is two steps and the first one must not be enough.
  //
  // **Not the canvas centre**, which is where this check used to click and is
  // the whole of why this group read 23/25. The pause menu is laid out centred
  // in the framebuffer and `RESUME` is the item the centre lands in, so a click
  // there is a click on `RESUME` — the demo resumed, correctly, and the check
  // read it as focus handling having regressed. It had not: `a_click_on_resume_-
  // resumes_the_game` in each sample's `app.rs` requires exactly that. A corner
  // is the part of the canvas least likely to be a widget in any sample, which
  // is what a check about *focus* rather than about a button needs.
  // `a_focusing_click_off_every_button_leaves_the_game_paused` pins the inset
  // used here as being outside every button, in a place a layout change will
  // trip over.
  const corner = {
    x: Math.round(rect.left + FOCUS_CLICK_INSET),
    y: Math.round(rect.top + FOCUS_CLICK_INSET),
  };
  for (const type of ['mousePressed', 'mouseReleased']) {
    await page.send('Input.dispatchMouseEvent', {
      type,
      x: corner.x,
      y: corner.y,
      button: 'left',
      clickCount: 1,
      buttons: type === 'mousePressed' ? 1 : 0,
    });
  }

  // The click has to have *reached the engine* before "it is still paused"
  // means anything: read the status too early and a demo that would have
  // resumed still reports 6, and the check passes for the reason it exists to
  // rule out. A sleep cannot establish that — it only makes the wrong answer
  // less likely — so wait on the observable instead. The shim's `pointerdown`
  // listener runs synchronously while `Input.dispatchMouseEvent` is in flight,
  // so the event is already queued in the wasm backend by the time the command
  // returns; `frame()` in the demo's `main.js` drains that queue and applies
  // any menu action, so two `requestAnimationFrame` ticks is the engine having
  // had, and taken, its chance to resume.
  await evaluate(
    page,
    `new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(ok)))`
  );

  // The other half of "the click reached the engine": it also has to have done
  // the job it was dispatched for, or this is a check about a click that missed
  // the canvas entirely.
  const refocused = await evaluate(page, `document.activeElement?.id ?? ''`);
  check(
    'E',
    'a click in the corner hands the canvas its keyboard back',
    refocused === 'canvas',
    `activeElement is "${refocused}"`
  );

  check(
    'E',
    'focus coming back does not resume on its own',
    (await evaluate(page, `crcbl.status()`)) === 6,
    `status ${await evaluate(page, `crcbl.status()`)}`
  );

  // **Read before the key, and required to be 6.** `until` returns the moment
  // it sees a 3, including the 3 that was already there — so if anything above
  // left the demo running, this check passes without Escape doing a thing, and
  // the two checks after it fail instead with no hint of why. That is exactly
  // how the centred click hid: "focus coming back does not resume" went red,
  // "Escape resumes the demo" went green off the stale status, and Escape then
  // *paused* a running demo and took the heartbeat check down with it. A check
  // that cannot fail is not a check, and neither is one that cannot be reached.
  const beforeEscape = await evaluate(page, `crcbl.status()`);
  for (const type of ['keyDown', 'keyUp']) {
    await page.send('Input.dispatchKeyEvent', {
      type,
      code: 'Escape',
      key: 'Escape',
      windowsVirtualKeyCode: 27,
      nativeVirtualKeyCode: 27,
    });
  }
  const resumed =
    beforeEscape === 6 &&
    (await until(async () => {
      const status = await evaluate(page, `crcbl.status()`);
      return status === 3 ? status : null;
    }));
  check(
    'E',
    'Escape resumes the demo',
    resumed === 3,
    beforeEscape === 6
      ? `status ${await evaluate(page, `crcbl.status()`)}`
      : `the demo was not paused going in (status ${beforeEscape}), so Escape ` +
          `pausing and Escape resuming are indistinguishable here`
  );
  const afterResume = await heartbeats();
  check(
    'E',
    'the simulation runs again after resuming',
    afterResume > 0,
    `${afterResume} HUD line(s) in ${TICK_WINDOW_MS} ms`
  );

  group('F — a finger');

  // **Nothing above this line has ever sent a touch.**
  // `Input.dispatchMouseEvent` is a mouse all the way down: it arrives as a
  // `pointerdown` whose `pointerType` is "mouse", `isPrimary` is never false, no
  // `pointercancel` is ever raised, and the browser does not consult
  // `touch-action` on the way. So the shim's touch handling, and the CSS that
  // decides whether the browser hands a gesture to the page at all, were shipped
  // with a green gate that could not see them.
  //
  // `Emulation.setTouchEmulationEnabled` is what makes the browser build touch
  // pointers for `Input.dispatchTouchEvent`, and it also flips `(hover: none)`
  // and `(pointer: coarse)` — the pair the demo pages swap their copy on — so
  // one call sets up both halves of this group.

  /** Both counts, so a check cannot pass on a selector that matches nothing. */
  const copyState = () =>
    evaluate(
      page,
      `(() => {
         const count = (selector) => {
           const all = [...document.querySelectorAll(selector)];
           return {
             total: all.length,
             shown: all.filter((el) => getComputedStyle(el).display !== 'none')
               .length,
           };
         };
         return {
           touch: count('.touch-only'),
           pointer: count('.pointer-only'),
           coarse: matchMedia('(hover: none) and (pointer: coarse)').matches,
           contacts: navigator.maxTouchPoints,
         };
       })()`
    );

  const withMouse = await copyState();
  check(
    'F',
    'a mouse gets the keyboard copy and none of the touch copy',
    withMouse.touch.total > 0 &&
      withMouse.touch.shown === 0 &&
      withMouse.pointer.total > 0 &&
      withMouse.pointer.shown === withMouse.pointer.total,
    `${withMouse.touch.shown}/${withMouse.touch.total} touch-only and ` +
      `${withMouse.pointer.shown}/${withMouse.pointer.total} pointer-only elements showing`
  );

  await page.send('Emulation.setTouchEmulationEnabled', {
    enabled: true,
    maxTouchPoints: MAX_TOUCH_POINTS,
  });

  const withFinger = await copyState();
  // The precondition for everything below, asserted rather than assumed: an
  // emulation call that silently did nothing would leave every touch check
  // dispatching events no browser would ever build, and they would fail as if
  // the engine had.
  check(
    'F',
    'touch emulation reports a coarse pointer with contacts',
    withFinger.coarse && withFinger.contacts > 0,
    `(hover: none) and (pointer: coarse) is ${withFinger.coarse}, ` +
      `maxTouchPoints ${withFinger.contacts}`
  );
  check(
    'F',
    'a coarse pointer swaps the keyboard copy for the touch copy',
    withFinger.touch.total > 0 &&
      withFinger.touch.shown === withFinger.touch.total &&
      withFinger.pointer.shown === 0,
    `${withFinger.touch.shown}/${withFinger.touch.total} touch-only and ` +
      `${withFinger.pointer.shown}/${withFinger.pointer.total} pointer-only elements showing`
  );

  // **A fresh page, booted with touch already on.** What follows is a phone's
  // visit, and by this point groups C to E have launched the ball, spent lives,
  // paused the demo and left whichever menu that ended on. Navigating to the
  // same URL is the whole reset: the shim's `pagehide` teardown runs and the
  // next document starts from the top.
  const beforeReload = hud().length;
  await page.send('Page.navigate', { url });
  await until(async () => evaluate(page, `document.readyState === 'complete'`));
  const rebooted = await until(async () => {
    const status = await evaluate(page, `crcbl.status()`);
    return status === 3 ? status : null;
  });
  /** The demo's HUD lines since the reload, so nothing reads the old run's. */
  const fresh = () => hud().slice(beforeReload);
  await until(async () => fresh().length > 0);
  check(
    'F',
    'the demo boots again with touch emulation on',
    rebooted === 3 && fresh().length > 0,
    fresh().at(0)?.trim() ??
      `status ${rebooted ?? 'never settled'}, no HUD line`
  );

  const canvas = await evaluate(
    page,
    `(() => { const c = document.getElementById('canvas');
              c.scrollIntoView({ block: 'center', behavior: 'instant' });
              const r = c.getBoundingClientRect();
              return { x: r.x, y: r.y, width: r.width, height: r.height }; })()`
  );
  /** A point on the canvas, as fractions of its box. */
  const spot = (fx, fy) => ({
    x: Math.round(canvas.x + fx * canvas.width),
    y: Math.round(canvas.y + fy * canvas.height),
  });
  const contact = (point, id = 0) => ({ x: point.x, y: point.y, id });
  const touch = (type, touchPoints = []) =>
    page.send('Input.dispatchTouchEvent', { type, touchPoints });

  /**
   * A tap, with the press and the release in **one** pump.
   *
   * Both messages go out before either is awaited. A finger is on the glass for
   * a fraction of a frame, so a real tap's press and release reach the engine in
   * the same batch — and a loop that only forwards a release it already believed
   * in drops that one, leaves the game holding the button and eats the *next*
   * tap. Awaiting the press first would let a frame run in between and hide
   * exactly the case a phone always takes.
   */
  const tap = async (point) =>
    Promise.all([touch('touchStart', [contact(point)]), touch('touchEnd')]);

  // What the browser delivered, counted in the page. This is about the browser
  // rather than the engine — whether a gesture was handed to the canvas at all —
  // and there is nowhere else to see it. The listeners are passive, so they
  // cannot change what the shim's own listeners then do with the same events.
  await evaluate(
    page,
    `(() => {
       const seen = { moves: 0, cancels: 0, secondary: 0 };
       globalThis.__crcblGateTouch = seen;
       const canvas = document.getElementById('canvas');
       const on = (type, handler) =>
         canvas.addEventListener(type, handler, { passive: true });
       on('pointermove', () => { seen.moves += 1; });
       on('pointercancel', () => { seen.cancels += 1; });
       on('pointerdown', (e) => { if (!e.isPrimary) seen.secondary += 1; });
       return true;
     })()`
  );
  const delivered = () =>
    evaluate(page, `({ ...globalThis.__crcblGateTouch, scroll: scrollY })`);

  /** A drag as a real one arrives: down, a run of moves, up. */
  const drag = async (from, to, steps = DRAG_STEPS) => {
    await touch('touchStart', [contact(spot(from.x, from.y))]);
    for (let i = 1; i <= steps; i += 1) {
      const at = spot(
        from.x + ((to.x - from.x) * i) / steps,
        from.y + ((to.y - from.y) * i) / steps
      );
      await touch('touchMove', [contact(at)]);
    }
    await touch('touchEnd');
  };

  // **`touch-action: none`, asserted on the browser's behaviour and not on the
  // stylesheet.** Reading the rule back out of `getComputedStyle` would pass on
  // a declaration the browser ignores, which is the half that matters: the
  // property's whole job is to stop the *browser* claiming the gesture for
  // scrolling. A drag with a large vertical component is the one that gets
  // claimed — the demo page scrolls — so this drags up and across, and then
  // asks the three questions a stolen gesture answers differently. Measured with
  // the declaration overridden to `auto`: the moves stop after the first, a
  // `pointercancel` arrives, and the page scrolls instead.
  //
  // It starts low on the canvas and finishes above the middle, so the press that
  // begins it lands below any centred menu and cannot fire a widget on the way.
  const beforeDrag = await delivered();
  await drag({ x: 0.5, y: 0.85 }, { x: 0.35, y: 0.35 });
  const afterDrag = await delivered();
  check(
    'F',
    'the canvas keeps a drag the browser would otherwise take for scrolling',
    afterDrag.moves - beforeDrag.moves === DRAG_STEPS &&
      afterDrag.cancels === beforeDrag.cancels &&
      afterDrag.scroll === beforeDrag.scroll,
    `${afterDrag.moves - beforeDrag.moves}/${DRAG_STEPS} moves delivered, ` +
      `${afterDrag.cancels - beforeDrag.cancels} cancel(s), ` +
      `scrollY ${beforeDrag.scroll} -> ${afterDrag.scroll}`
  );

  // **Two fingers, and the seam has room for both.**
  //
  // This is the multi-touch claim itself and it is about the *engine*, not about
  // any game: every demo runs the same loop, so it is asserted for every demo
  // rather than only for the ones that bind a pointer. Until `ShellEvent::Touch`
  // existed the shim dropped every non-primary contact on the floor, and the
  // check that stood here asserted exactly that — a second contact moving
  // nothing.
  //
  // The observable is the engine's own debug log of the events it folded, which
  // is the only place a contact is visible from a browser: the demos bind the
  // pointer, and none of them draws an on-screen control yet. `Pending::observe`
  // writes those lines, so a contact appearing there has crossed the browser,
  // the shim, the wasm ABI and the shell queue.
  const contactsFrom = (mark) =>
    consoleLines
      .slice(mark)
      .map((line) => TOUCH_LINE.exec(line))
      .filter((found) => found !== null)
      .map(([, id, phase, x, y]) => ({
        id: Number(id),
        phase,
        x: Number(x),
        y: Number(y),
      }));

  const dpr = await evaluate(page, `devicePixelRatio`);
  /** Where a dispatched point should turn up, in the canvas's device pixels. */
  const inCanvas = (point) => ({
    x: (point.x - canvas.x) * dpr,
    y: (point.y - canvas.y) * dpr,
  });
  const near = (got, want) =>
    Math.abs(got.x - want.x) <= CONTACT_TOLERANCE &&
    Math.abs(got.y - want.y) <= CONTACT_TOLERANCE;

  const debugOn = await evaluate(page, `crcbl.logLevel(${LOG_DEBUG})`);
  const contactMark = consoleLines.length;
  const first = spot(CONTACT_A, CONTACT_BAND);
  const second = spot(CONTACT_B, CONTACT_BAND);
  const secondMoved = spot(CONTACT_B_MOVED, CONTACT_BAND);
  await touch('touchStart', [contact(first, 1)]);
  await touch('touchStart', [contact(first, 1), contact(second, 2)]);
  // Only the second point changes, which is what makes "moving one moves only
  // that one" a question with two possible answers.
  await touch('touchMove', [contact(first, 1), contact(secondMoved, 2)]);
  await touch('touchEnd');
  // Found by *position*, not by counting: the gesture before this one is still
  // flushing its own lines through the log queue when the mark is taken, so the
  // window can legitimately open on a stray `Ended` from the drag above. Which
  // id landed where is the claim, and it is one a leftover line cannot answer.
  const idsOf = (seen) => {
    const began = seen.filter((c) => c.phase === 'Began');
    const idAt = (point) =>
      began.find((c) => near(c, inCanvas(point)))?.id ?? null;
    return { firstId: idAt(first), secondId: idAt(second) };
  };
  const endedIn = (seen) =>
    new Set(seen.filter((c) => c.phase === 'Ended').map((c) => c.id));

  // The lines arrive a frame later — the engine folds the batch on its next
  // pump and the page drains the log queue after that.
  //
  // **Waiting on these two contacts, not on a count of `Ended`s.** A count lets
  // the stray the comment above anticipates fill the quota, so the wait returns
  // one contact early and the claim below reads a line that had not arrived yet
  // as a contact the engine dropped. Seen in CI as `34/35` with the second
  // contact holding a `Began` and a `Moved` and no `Ended`.
  await until(async () => {
    const seen = contactsFrom(contactMark);
    const { firstId, secondId } = idsOf(seen);
    if (firstId === null || secondId === null || firstId === secondId) {
      return null;
    }
    const ended = endedIn(seen);
    return ended.has(firstId) && ended.has(secondId) ? seen : null;
  }, PADDLE_SETTLE_MS);
  await evaluate(page, `crcbl.logLevel(${LOG_INFO})`);

  const seen = contactsFrom(contactMark);
  const { firstId, secondId } = idsOf(seen);
  // Every later report of the first contact, which must still be where it was
  // put: a move credited to the wrong contact shows up here as the held finger
  // having jumped across the canvas.
  const heldStrayed = seen.some(
    (c) => c.id === firstId && c.phase !== 'Began' && !near(c, inCanvas(first))
  );
  const movedSecond = seen.filter(
    (c) => c.phase === 'Moved' && near(c, inCanvas(secondMoved))
  );
  const ended = endedIn(seen);
  check(
    'F',
    'a second contact arrives as its own contact and moves only itself',
    debugOn === 1 &&
      firstId !== null &&
      secondId !== null &&
      firstId !== secondId &&
      !heldStrayed &&
      movedSecond.length > 0 &&
      movedSecond.every((c) => c.id === secondId) &&
      ended.has(firstId) &&
      ended.has(secondId),
    debugOn === 1
      ? `contacts ${JSON.stringify(seen.map((c) => [c.id, c.phase, Math.round(c.x)]))}` +
          ` — held ${firstId}, moved ${secondId}`
      : 'the engine refused the debug log level, so no contact could be seen'
  );

  // Everything past here is about what the *game* does with a finger, so a demo
  // that binds no pointer input stops at the page-level claims above rather than
  // dispatching taps and asserting they did something. `EXPECTATIONS` says
  // which, the same way `key: null` says a demo takes no keyboard.
  if (EXPECTED.touch) {
    const paddleAt = async () => evaluate(page, SAMPLE_PADDLE('#canvas'));

    /**
     * Polls the frame until the paddle is where the finger asked for it.
     *
     * A drag ends when the last `touchMove` is acknowledged, which is before the
     * engine has pumped it and long before the frame carrying the result has
     * been composited — so a single read after the drag is a race, and the
     * reading it loses with is the paddle's *previous* position. Returns the
     * last sample either way, so a failure can say where the paddle actually
     * was.
     */
    const paddleReaches = async (target) => {
      let last = null;
      const reached = await until(async () => {
        last = await paddleAt();
        return last?.count > 0 && Math.abs(last.at - target) <= PADDLE_TOLERANCE
          ? last
          : null;
      }, PADDLE_SETTLE_MS);
      return { reached: Boolean(reached), last };
    };

    if (EXPECTED.touch.paddle) {
      await drag(
        { x: FIRST_DRAG.from, y: PADDLE_BAND },
        { x: FIRST_DRAG.to, y: PADDLE_BAND }
      );
      const firstDrag = await paddleReaches(FIRST_DRAG.to);
      check(
        'F',
        'a drag puts the paddle under the finger',
        firstDrag.reached,
        firstDrag.last?.count
          ? `asked for ${FIRST_DRAG.to}, paddle at ${firstDrag.last.at.toFixed(3)}`
          : 'no paddle in the bottom band of the frame'
      );

      // **The re-grab.** A finger that lifts and lands somewhere else is the
      // case a delta-composed drag cannot do: composed on the last position it
      // would move by the length of this drag instead of to its end, and land
      // nowhere near.
      await drag(
        { x: SECOND_DRAG.from, y: PADDLE_BAND },
        { x: SECOND_DRAG.to, y: PADDLE_BAND }
      );
      const secondDrag = await paddleReaches(SECOND_DRAG.to);
      check(
        'F',
        'a second drag from somewhere else moves it again',
        secondDrag.reached,
        secondDrag.last?.count
          ? `asked for ${SECOND_DRAG.to}, paddle at ${secondDrag.last.at.toFixed(3)}`
          : 'no paddle in the bottom band of the frame'
      );

      // **The `isPrimary` filter, which is now about the pointer only.** A
      // second finger does not move the mouse — that is the browser's own rule
      // for its emulated pointer events and the shim keeps it — so the paddle,
      // which binds `Binding::PointerPosition`, stays with the first contact
      // while a second one lands elsewhere and moves.
      //
      // The contact check above is what proves the second finger was not
      // *dropped*; this one is what proves it did not reach the pointer, and the
      // pair is the whole design: a game bound to a mouse plays with one finger
      // exactly as it did before multi-touch landed.
      //
      // Two things keep the negative claim honest. The page's count of
      // non-primary presses says the browser really did deliver a second
      // contact, so this cannot pass on a fumble that never happened. And the
      // first contact then moves, inside the same window, to somewhere the
      // second one never was: a window too short to show a move would fail
      // there rather than making "nothing moved" true for free.
      const held = spot(FIRST_DRAG.to, PADDLE_BAND);
      const second = spot(SECOND_DRAG.from, PADDLE_BAND);
      const moved = spot(SECOND_DRAG.to, PADDLE_BAND);
      await touch('touchStart', [contact(held, 1)]);
      const anchored = await paddleReaches(FIRST_DRAG.to);
      await touch('touchStart', [contact(held, 1), contact(second, 2)]);
      await touch('touchMove', [contact(held, 1), contact(moved, 2)]);
      await pause(PADDLE_SETTLE_MS);
      const fumbled = await paddleAt();
      await touch('touchMove', [
        contact(spot(FIRST_DRAG.from, PADDLE_BAND), 1),
        contact(moved, 2),
      ]);
      const followed = await paddleReaches(FIRST_DRAG.from);
      await touch('touchEnd');
      const secondary = (await delivered()).secondary;
      check(
        'F',
        'a second contact leaves the emulated pointer alone',
        secondary > 0 &&
          anchored.reached &&
          fumbled?.count > 0 &&
          Math.abs(fumbled.at - anchored.last.at) <= PADDLE_TOLERANCE &&
          followed.reached,
        secondary > 0
          ? `paddle at ${anchored.last?.at?.toFixed(3)} on one contact, ` +
              `${fumbled?.at?.toFixed(3)} while a second moved to ${SECOND_DRAG.to}, ` +
              `${followed.last?.at?.toFixed(3)} when the first moved to ${FIRST_DRAG.from}`
          : 'the browser delivered no non-primary press, so nothing was filtered'
      );
    }

    // **A tap on the menu.** Every demo here opens on a start panel, and a menu
    // on screen owns the button — so this is the first thing a phone visitor
    // touches, and until it works nothing else in the game can be reached by
    // finger at all. The centre of the canvas is where the panel's first item
    // is laid out, which is why every other check in this file deliberately
    // clicks a corner.
    const startMark = fresh().length;
    await tap(spot(0.5, 0.5));
    const startedByTap = await until(
      async () => fresh().slice(startMark).find(EXPECTED.started),
      LIFE_MS
    );
    check(
      'F',
      'a tap on the start menu starts the run',
      Boolean(startedByTap),
      (startedByTap ?? EXPECTED.startedFailure).trim()
    );

    if (EXPECTED.touch.lives) {
      const lives = (line) =>
        Number(line?.match(EXPECTED.touch.lives)?.[1] ?? NaN);
      const startingLives = lives(fresh().at(0));

      // Park the paddle at one edge so the ball, which starts in the middle, is
      // not caught by it. Waiting for a life to be lost is not a detour: the
      // start menu stays up until the run is under way, and the state a tap can
      // serve from is the one a lost life comes back to.
      await drag({ x: 0.4, y: PADDLE_BAND }, { x: PARK_X, y: PADDLE_BAND });

      // **A gesture the browser took away.** `pointercancel` fires *instead of*
      // `pointerup`, so a shim that does not translate it leaves the engine
      // holding the button — and a held button raises no press edge, so the
      // symptom is not a stuck control but a tap that silently stops working.
      // Two frames between the press and the cancel keep this about the cancel
      // rather than about a press and release landing in one pump, which is the
      // check below.
      await touch('touchStart', [contact(spot(PARK_X, PADDLE_BAND))]);
      await evaluate(
        page,
        `new Promise((ok) => requestAnimationFrame(() => requestAnimationFrame(ok)))`
      );
      await touch('touchCancel');

      const lostOne = await until(
        async () =>
          fresh().find(
            (line) => EXPECTED.waiting(line) && lives(line) < startingLives
          ),
        LIFE_MS
      );
      const afterCancel = fresh().length;
      await tap(spot(PARK_X, PADDLE_BAND));
      const servedAfterCancel = await until(
        async () => fresh().slice(afterCancel).find(EXPECTED.started),
        LIFE_MS
      );
      check(
        'F',
        'a tap after a cancelled gesture still serves',
        Boolean(lostOne) && Boolean(servedAfterCancel),
        lostOne
          ? (servedAfterCancel ?? 'the tap raised no edge').trim()
          : `no life was lost inside ${LIFE_MS} ms, so there was never a tap to make`
      );

      // **Two taps in a row.** The second is the one that matters: a tap is one
      // pump's press *and* release, and a loop that drops the release keeps the
      // button down, so the first tap works and the second does nothing. A
      // single tap and an assertion cannot tell those apart.
      const lostTwo = await until(
        async () =>
          fresh()
            .slice(afterCancel)
            .find(
              (line) =>
                EXPECTED.waiting(line) && lives(line) < startingLives - 1
            ),
        LIFE_MS
      );
      const afterSecond = fresh().length;
      await tap(spot(PARK_X, PADDLE_BAND));
      const servedAgain = await until(
        async () => fresh().slice(afterSecond).find(EXPECTED.started),
        LIFE_MS
      );
      check(
        'F',
        'a second tap in a row serves it again',
        Boolean(lostTwo) && Boolean(servedAgain),
        lostTwo
          ? (servedAgain ?? 'the second tap raised no edge').trim()
          : `no second life was lost inside ${LIFE_MS} ms`
      );
    }

    if (EXPECTED.touch.height) {
      // **A tap is a flap**, and the bird's height is the observable: gravity is
      // the only other thing that touches it and gravity only ever lowers it.
      //
      // **`CLIMB_ABOVE` and not "higher than it was"**, which is the version
      // this check shipped as for an afternoon and which passed with the game's
      // tap binding cut out of the build. The run's *own start* is a flap — the
      // menu button is bound to the same action — so the arc it throws the bird
      // through is above the starting height for a third of a second, and the
      // HUD's every-sixtieth-tick line lands in that arc often enough to look
      // like a tap that worked. A height one flap cannot reach is what tells the
      // two apart.
      const height = (line) =>
        Number(line?.match(EXPECTED.touch.height)?.[1] ?? NaN);
      const bar = height(fresh().at(-1)) + CLIMB_ABOVE;
      const climbMark = fresh().length;
      const climbed = await until(async () => {
        await tap(spot(0.5, 0.5));
        await pause(TAP_INTERVAL_MS);
        const above = fresh()
          .slice(climbMark)
          .map(height)
          .find((y) => y > bar);
        return above === undefined ? null : { above };
      }, CLIMB_MS);
      check(
        'F',
        'a tap lifts the bird',
        Boolean(climbed),
        climbed
          ? `y reached ${climbed.above}, over the ${bar} one flap could manage`
          : `y never passed ${bar} in ${CLIMB_MS} ms of tapping, which is ` +
              'what the flap the run started with does on its own'
      );
    }

    if (EXPECTED.touch.walk) {
      // **The on-screen stick**, and the first thing in this file that is not
      // the emulated pointer: a `crcbl-ui` widget takes the raw contacts, and
      // reports through `Binding::Virtual` into the same `move` action `WASD`
      // drives. The pointer cannot express this — a stick is a direction held
      // continuously, and `Binding::PointerPosition` is a place.
      const walkAt = (line) =>
        Number(line?.match(EXPECTED.touch.walk)?.[1] ?? NaN);
      /** The most recent position the HUD printed, or `NaN` if it never has. */
      const wizardX = () => {
        const seen = fresh().map(walkAt).filter(Number.isFinite);
        return seen.at(-1) ?? NaN;
      };
      // Boxed, because `until` polls for something *truthy* and the wizard
      // starts the run standing at exactly zero.
      const found = await until(
        async () => (Number.isFinite(wizardX()) ? { at: wizardX() } : null),
        WALK_MS
      );
      const start = found?.at ?? NaN;

      const grab = spot(STICK_FROM, STICK_BAND);
      const pushed = spot(STICK_TO, STICK_BAND);
      await touch('touchStart', [contact(grab, 1)]);
      await touch('touchMove', [contact(pushed, 1)]);
      // **The thumb stays down for the rest of this block.** A stick is a level
      // and not an edge: the finger reports nothing while it rests, so a game
      // that centred the stick between events would stop the wizard here.
      const walked = await until(async () => {
        const at = wizardX();
        return at > start + WALK_MARGIN ? at : null;
      }, WALK_MS);
      check(
        'F',
        'a thumb on the field walks the wizard',
        walked !== null,
        walked === null
          ? `x stayed at ${start} for ${WALK_MS} ms with a thumb pushing right`
          : `x ${start} -> ${walked}, pushed right`
      );

      if (EXPECTED.touch.pause) {
        // **Two fingers doing two things at once**, which no earlier demo could
        // be asked to do: the thumb above is the *primary* contact, so the
        // second finger raises no pointer event at all and its control is
        // reached through the contact stream or not at all.
        const box = await evaluate(
          page,
          `(() => { const c = document.getElementById('canvas');
                    return { w: c.width, h: c.height }; })()`
        );
        const pauseSpot = spot(1 - PAUSE_INSET / box.w, PAUSE_INSET / box.h);
        const running = await evaluate(page, `crcbl.status()`);
        const beforeSecond = wizardX();
        await touch('touchStart', [contact(pushed, 1), contact(pauseSpot, 2)]);
        // The control on the negative claim below: with *both* fingers down,
        // the wizard is still walking — so the second contact neither stole the
        // stick nor centred it, and a pause that follows cannot be the first
        // finger having been dropped.
        const bothDown = await until(async () => {
          const at = wizardX();
          return at > beforeSecond + WALK_MARGIN ? at : null;
        }, WALK_MS);
        // **Only the second finger lifts**, which is what makes the pause the
        // *second* one's doing. `Input.dispatchTouchEvent`'s `touchEnd` takes
        // the points being **released** — an empty list is the "release
        // everything" every other gesture in this file uses, and naming one
        // point lifts that one and leaves the rest of the hand where it is.
        await touch('touchEnd', [contact(pauseSpot, 2)]);
        const paused = await until(async () => {
          const status = await evaluate(page, `crcbl.status()`);
          return status === STATUS_PAUSED ? status : null;
        }, WALK_MS);
        const stoppedAt = wizardX();
        await pause(TICK_WINDOW_MS);
        const stillStopped = wizardX();
        check(
          'F',
          'a second finger pauses the run while the first keeps walking',
          running === STATUS_RUNNING &&
            bothDown !== null &&
            paused === STATUS_PAUSED &&
            stoppedAt === stillStopped,
          running !== STATUS_RUNNING
            ? `the demo was not running going in (status ${running})`
            : bothDown === null
              ? 'the second finger stopped the first one walking, so the ' +
                'pause below says nothing about two fingers'
              : `x reached ${bothDown} with two fingers down, status ` +
                `${paused ?? (await evaluate(page, `crcbl.status()`))}, ` +
                `x ${stoppedAt} -> ${stillStopped} while paused`
        );

        // **The panel is tapped shut with the thumb still on the stick**, which
        // is the lockout this whole contact route exists for: only the primary
        // contact drives the emulated pointer, so while this thumb is down no
        // other finger raises one, and `RESUME` could not be pressed by anybody
        // until the stick was let go. The third contact is a finger that never
        // touches the pointer at all.
        const lockedMark = consoleLines.length;
        await touch('touchStart', [
          contact(pushed, 1),
          contact(spot(0.5, 0.5), 3),
        ]);
        await touch('touchEnd', [contact(spot(0.5, 0.5), 3)]);
        const unlocked = await until(async () => {
          const status = await evaluate(page, `crcbl.status()`);
          return status === STATUS_RUNNING ? status : null;
        }, WALK_MS);
        check(
          'F',
          'a second finger taps the pause menu shut while the first holds the stick',
          unlocked === STATUS_RUNNING,
          unlocked === STATUS_RUNNING
            ? `resumed with contact 1 still down (${consoleLines.length - lockedMark} lines)`
            : `status ${await evaluate(page, `crcbl.status()`)} — the menu ` +
                'could not be reached while a control was held'
        );

        // **And the thumb that never lifted takes the stick back**, on its next
        // move rather than after being lifted and landed again. The panel
        // released the stick when it opened — deliberately, so the wizard does
        // not walk behind it — and a floating stick re-centres wherever the
        // thumb has got to, which is why this moves twice: once to take it, once
        // to push it.
        const regrabbed = wizardX();
        await touch('touchMove', [contact(spot(0.68, STICK_BAND), 1)]);
        await touch('touchMove', [contact(spot(0.95, STICK_BAND), 1)]);
        const walkedAgain = await until(async () => {
          const at = wizardX();
          return at > regrabbed + WALK_MARGIN ? at : null;
        }, WALK_MS);
        check(
          'F',
          'the thumb that never lifted walks again once the panel has gone',
          walkedAgain !== null,
          walkedAgain === null
            ? `x stayed at ${regrabbed} for ${WALK_MS} ms with a thumb that ` +
                'never left the glass pushing right'
            : `x ${regrabbed} -> ${walkedAgain} without lifting`
        );

        // Back into the pause menu for the stray-lift check below, with the
        // thumb still down — which is the state that makes that check about
        // anything at all — and back where it started, because where it *lifts*
        // is what that check is about.
        await touch('touchMove', [contact(pushed, 1)]);
        await touch('touchStart', [contact(pushed, 1), contact(pauseSpot, 2)]);
        await touch('touchEnd', [contact(pauseSpot, 2)]);
        await until(async () => {
          const status = await evaluate(page, `crcbl.status()`);
          return status === STATUS_PAUSED ? status : null;
        }, WALK_MS);

        // **The thumb that was already down lifts, over a panel it never
        // pressed.** It is the primary contact, so its lift is also a
        // `pointerup`, and it lands wherever the panel happened to open — which
        // for a centred menu is on a button. Nothing may fire: the press was
        // made on the field, before this panel existed.
        //
        // Found here rather than reasoned about: this run asked for fullscreen
        // when that thumb came off, and `crcbl::engine`'s
        // `a_press_made_before_a_panel_opened_does_not_fire_its_buttons` is the
        // fast test that came out of it.
        const strayMark = consoleLines.length;
        await touch('touchEnd');
        await pause(TICK_WINDOW_MS);
        const strayLines = consoleLines
          .slice(strayMark)
          .filter((line) => STRAY_MENU_ACTION.test(line));
        const stillPaused = await evaluate(page, `crcbl.status()`);
        check(
          'F',
          'a thumb that was down before the panel opened presses nothing',
          strayLines.length === 0 && stillPaused === STATUS_PAUSED,
          strayLines.length
            ? `the lift fired ${strayLines.length} menu action(s): ${strayLines[0]}`
            : `status ${stillPaused} after the stray lift`
        );

        // …and out through the pause menu's own button, which is what a phone
        // can reach now that something can open the panel. The demo is left
        // running, as every other group here leaves it.
        await tap(spot(0.5, 0.5));
        const resumed = await until(async () => {
          const status = await evaluate(page, `crcbl.status()`);
          return status === STATUS_RUNNING ? status : null;
        });
        check(
          'F',
          'the pause menu the button opened can be tapped shut again',
          resumed === STATUS_RUNNING,
          `status ${resumed ?? (await evaluate(page, `crcbl.status()`))}`
        );
      }
    }

    if (EXPECTED.touch.pause) {
      // **One finger, the whole round trip**, for every demo that draws the
      // button rather than only for the one with two controls: tap the corner,
      // the loop stops; tap the panel, it starts again. On breakout and flappy
      // that finger is also the emulated pointer the game binds its paddle and
      // its flap to, so this is the check that the button is not merely
      // *present* — a run that served or flapped on the way to pausing would
      // pause here too, and `crcbl.status()` alone would call that a pass. The
      // sample tests carry that half, where the paddle and the bird can be read
      // directly.
      //
      // Each attempt taps the middle first, which starts or restarts a run
      // whenever a panel is up: flappy's bird dies on its own clock, and a
      // corner tap under a death screen presses nothing at all.
      const box = await evaluate(
        page,
        `(() => { const c = document.getElementById('canvas');
                  return { w: c.width, h: c.height }; })()`
      );
      const corner = spot(1 - PAUSE_INSET / box.w, PAUSE_INSET / box.h);
      const pausedByButton = await until(async () => {
        await tap(spot(0.5, 0.5));
        await pause(TAP_INTERVAL_MS);
        await tap(corner);
        await pause(TAP_INTERVAL_MS);
        const status = await evaluate(page, `crcbl.status()`);
        return status === STATUS_PAUSED ? status : null;
      }, LIFE_MS);
      check(
        'F',
        'a tap on the pause button stops the run',
        pausedByButton === STATUS_PAUSED,
        pausedByButton === STATUS_PAUSED
          ? `status ${pausedByButton} after a tap ${PAUSE_INSET} px inside the corner`
          : `status ${await evaluate(page, `crcbl.status()`)} — the corner ` +
              `was tapped for ${LIFE_MS} ms and the loop never stopped`
      );

      // …and out again, which leaves the demo running as every other group here
      // leaves it.
      await tap(spot(0.5, 0.5));
      const runningAgain = await until(async () => {
        const status = await evaluate(page, `crcbl.status()`);
        return status === STATUS_RUNNING ? status : null;
      });
      check(
        'F',
        'the panel that button opened can be tapped shut',
        runningAgain === STATUS_RUNNING,
        `status ${runningAgain ?? (await evaluate(page, `crcbl.status()`))}`
      );
    }
  }

  // **THE ONLY GATE ON THE COMMAND STREAM'S ROUND TRIP.** Everything else about
  // that format is checked without a browser: `stream-decode.mjs` decodes the
  // fixture Rust commits, `reply-encode.mjs` re-encodes the replies Rust reads,
  // `stream-transport.mjs` drives the ABI against a synthetic
  // `WebAssembly.Memory`, and `gpu-replay.mjs` runs the replayer against a
  // `navigator.gpu` that is not one. **None of them can call the real thing**,
  // which is precisely what this slice added — so the facts left over are the
  // ones only a browser can establish, and they are established here: a command
  // encoded in wasm reaches `navigator.gpu`, what the browser answers gets back
  // into wasm through the reply channel, and **a device actually opens** on the
  // adapter that answer named — with the capabilities that device has rather
  // than the ones its adapter has.
  //
  // Every claim below is corroborated against something the page can see for
  // itself: the adapter's name and features against `navigator.gpu`, the
  // device's against a device the page opens with the same descriptor. A check
  // that only read back what wasm sent would prove the transport and nothing
  // else, and the transport already has a node suite.
  //
  // It runs for every demo whose site was built for `wgpu` — the default —
  // because those demos link `crcbl-webgpu` and none of them drives it:
  // `crcbl::backend`'s registry entry for `webgpu` is not the active backend, so
  // the channel the probe installs is the only one in the page. The engine's own
  // frame loop is drawing throughout, which is the other half of what this says
  // — the last check below is that the demo did not notice.
  //
  // **These groups are skipped in webgpu mode**, and must be: there the engine's
  // own `WebGpuDevice` installs that one channel, so the probe cannot install a
  // second and every group here would fail at "could not install a channel". The
  // demo rendering through the backend (groups A–F) is what proves the same path
  // in that mode. See `WEBGPU_MODE`.
  if (!WEBGPU_MODE) {
    group('G — the WebGPU command stream makes a round trip');

    // **NOTHING HERE DRAINS, REPLAYS OR DELIVERS.** The probe encodes a command
    // and the demo's own frame loop does the rest — one drain, one replay, one
    // delivery per frame, in `web/engine/demo.js`'s `pumpGpu`. So every check
    // below waits for that loop to have done the work, which is what makes this
    // group a gate on the loop the WebGPU backend will use rather than on a
    // replayer the harness stood up for itself. A demo whose loop stopped
    // replaying fails here even though the transport, the format and the replayer
    // are all still correct — that is the claim, and no other check makes it.
    //
    // The decoy goes into the demo's *own* registry, which is the only one there
    // is now, and group H is what reads it back. See DECOY_CANVAS_ID.
    const probe = await evaluate(
      page,
      `(async () => {
       const { startAdapterProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const decoy = document.createElement('canvas');
       // Deleted and put back so that the decoy is ahead of it: a Map keeps
       // insertion order, and a decoy behind the right answer is not one.
       const canvas = gpu.canvases.get(gpu.canvasId);
       gpu.canvases.delete(gpu.canvasId);
       gpu.canvases.set(${DECOY_CANVAS_ID}, decoy);
       gpu.canvases.set(gpu.canvasId, canvas);
       globalThis.crcblProbe = { decoy };
       const before = gpu.stats();
       return {
         started: startAdapterProbe({ exports }),
         replayed: before.replayed,
         delivered: before.delivered,
       };
     })()`
    );

    // Polled, because the command is encoded on this evaluation's frame and
    // replayed on the demo's next one. `stats().replayed` counting up is the loop
    // saying it took a frame off the channel; `commands` is what that frame
    // carried, decoded by the loop rather than by anything here.
    const carried = probe?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const stats = globalThis.crcbl.gpu.stats();
             return stats.replayed > ${probe.replayed} || stats.failure
               ? stats
               : null;
           })()`
          )
        )
      : null;
    check(
      'G',
      'wasm encoded an adapter enumeration and the demo loop replayed it',
      carried?.commands?.join(',') === 'EnumerateAdapters',
      carried
        ? `the loop replayed [${carried.commands.join(', ')}] at sequence ${carried.baseSequence}` +
            `${carried.failure ? ` — ${carried.failure}` : ''}`
        : probe?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'the probe could not install a channel — something else already has one'
    );

    // Then polled again, because the answer cannot be on the frame that asked:
    // WebGPU's adapter API is a promise and the stream is replayed synchronously
    // once a frame, so the reply is queued when the browser settles and the loop
    // hands it over on a later frame. `WAITING` is the ordinary answer until then
    // and is not a state to report.
    //
    // `delivered` comes from the loop's own counter rather than from this call,
    // which no longer moves any bytes: it is the loop saying wasm took a reply
    // buffer, beside wasm saying it understood one.
    const answered = await until(async () =>
      evaluate(
        page,
        `(async () => {
         const { readAdapterProbe, PROBE } = await import('/engine/gpu-probe.js');
         const { exports, memory, gpu } = globalThis.crcbl;
         const out = readAdapterProbe({ exports, memory });
         return out.state === PROBE.WAITING
           ? null
           : { ...out, delivered: gpu.stats().delivered };
       })()`
      )
    );
    check(
      'G',
      'the browser answered it and wasm read the answer back',
      answered?.name === 'GRANTED' &&
        answered.delivered > (probe?.delivered ?? 0),
      answered
        ? `${answered.name}: ${JSON.stringify(answered.text)}, after the loop delivered ` +
            `${answered.delivered - (probe?.delivered ?? 0)} reply buffer(s)`
        : `no answer in ${TIMEOUT_MS} ms — the reply never reached wasm`
    );

    // **The name is the browser's own, not a string the round trip invented.**
    // Asked of the same page, through the same joins `gpu-replay.js` makes, at
    // the same moment — so a replayer that answered with a constant, or with the
    // wrong adapter, differs here. Nothing else in this group would notice.
    //
    // The same evaluation also brings back the raw `adapter.features` and
    // `adapter.limits` — what the browser says, before anything of ours has
    // touched them — and what the page's own mapping makes of the limits. Those
    // are what the capability checks below are held against.
    const live = await evaluate(
      page,
      `(async () => {
       const { halLimitsFor } = await import('/engine/gpu-replay.js');
       const adapter = await navigator.gpu.requestAdapter();
       const info = adapter?.info ?? {};
       const mapped = halLimitsFor(adapter);
       return {
         name: [info.vendor, info.architecture, info.device, info.description]
           .filter(Boolean).join(' '),
         features: [...adapter.features].sort(),
         limits: {
           maxTextureDimension2D: adapter.limits.maxTextureDimension2D,
           maxTextureDimension3D: adapter.limits.maxTextureDimension3D,
           maxTextureArrayLayers: adapter.limits.maxTextureArrayLayers,
           maxStorageBufferBindingSize: adapter.limits.maxStorageBufferBindingSize,
           maxUniformBufferBindingSize: adapter.limits.maxUniformBufferBindingSize,
           maxBindGroups: adapter.limits.maxBindGroups,
           maxColorAttachments: adapter.limits.maxColorAttachments,
           maxComputeWorkgroupSizeX: adapter.limits.maxComputeWorkgroupSizeX,
           maxComputeWorkgroupSizeY: adapter.limits.maxComputeWorkgroupSizeY,
           maxComputeWorkgroupSizeZ: adapter.limits.maxComputeWorkgroupSizeZ,
           maxComputeInvocationsPerWorkgroup:
             adapter.limits.maxComputeInvocationsPerWorkgroup,
           maxComputeWorkgroupsPerDimension:
             adapter.limits.maxComputeWorkgroupsPerDimension,
           minUniformBufferOffsetAlignment:
             adapter.limits.minUniformBufferOffsetAlignment,
           minStorageBufferOffsetAlignment:
             adapter.limits.minStorageBufferOffsetAlignment,
         },
         // BigInts do not survive the wire back to this driver, and every one
         // of these is far below 2^53.
         mapped: Object.fromEntries(
           Object.entries(mapped).map(([key, value]) =>
             [key, typeof value === 'bigint' ? Number(value) : value])
         ),
       };
     })()`
    );
    check(
      'G',
      'the adapter name wasm received is the one this browser reports',
      typeof answered?.text === 'string' && answered.text === live?.name,
      answered?.text === live?.name
        ? `both say ${JSON.stringify(live?.name)}`
        : `wasm got ${JSON.stringify(answered?.text)}, the browser says ${JSON.stringify(live?.name)}`
    );

    // **The capabilities crossed too, and they are the browser's own.** The whole
    // of `AdapterInfo` now travels, and five of its seven wire fields are the
    // documented absences that a browser has nothing to disagree with — so the two
    // that vary are the two worth asking a browser about, and both are asked here.
    //
    // The expected bits are spelled out below rather than imported from
    // `gpu-replay.js`: a table taken from the thing under test agrees with it by
    // construction, and what this check is for is that the table itself is right
    // about this browser's feature set.
    const wasmCaps = answered?.caps;
    // `crcbl_hal::Features` bits: the four core WebGPU grants outright, then the
    // four with a `GPUFeatureName` behind them.
    const CORE_FEATURE_BITS = (1 << 8) | (1 << 7) | (1 << 14) | (1 << 18); // COMPUTE, OCCLUSION_QUERY, DEPTH_BIAS_CLAMP, DEBUG_MARKERS
    const NAMED_FEATURE_BITS = {
      'depth-clip-control': 1 << 13, // DEPTH_CLAMP
      'texture-compression-bc': 1 << 16, // TEXTURE_COMPRESSION_BC
      'timestamp-query': 1 << 5, // TIMESTAMP_QUERY
      'indirect-first-instance': 1 << 4, // INDIRECT_FIRST_INSTANCE
    };
    /** The `crcbl_hal::Features` word a list of `GPUFeatureName`s amounts to. */
    const featureBitsOf = (names) =>
      (names ?? []).reduce(
        (bits, name) => bits | (NAMED_FEATURE_BITS[name] ?? 0),
        CORE_FEATURE_BITS
      ) >>> 0;
    const expectedFeatures = featureBitsOf(live?.features);
    check(
      'G',
      'the feature set wasm received is what this browser actually reports',
      wasmCaps?.featuresLo === expectedFeatures && wasmCaps?.featuresHi === 0,
      `wasm got ${wasmCaps?.featuresLo?.toString(16)}/${wasmCaps?.featuresHi?.toString(16)},` +
        ` ${expectedFeatures.toString(16)} expected from [${(live?.features ?? []).join(', ')}]`
    );
    check(
      'G',
      'a limit wasm received is the number this browser reports',
      wasmCaps?.maxImage2d === live?.limits?.maxTextureDimension2D &&
        wasmCaps?.maxImage2d > 0,
      `wasm got ${wasmCaps?.maxImage2d}, navigator.gpu says` +
        ` maxTextureDimension2D ${live?.limits?.maxTextureDimension2D}`
    );

    // …and the other eighteen limits, which no export carries. Checked where they
    // can be: against the live adapter, in the page, through the same mapping the
    // replayer used to fill the reply. `gpu-replay.mjs` does this against a stub
    // with distinct numbers; this is the same table meeting a real browser's.
    const limitPairs = [
      ['maxImage2d', 'maxTextureDimension2D'],
      ['maxImage3d', 'maxTextureDimension3D'],
      ['maxImageArrayLayers', 'maxTextureArrayLayers'],
      ['maxStorageBufferRange', 'maxStorageBufferBindingSize'],
      ['maxUniformBufferRange', 'maxUniformBufferBindingSize'],
      ['maxBindGroups', 'maxBindGroups'],
      ['maxColorAttachments', 'maxColorAttachments'],
      [
        'maxComputeInvocationsPerWorkgroup',
        'maxComputeInvocationsPerWorkgroup',
      ],
      ['maxComputeWorkgroupsPerDimension', 'maxComputeWorkgroupsPerDimension'],
      ['minUniformBufferOffsetAlignment', 'minUniformBufferOffsetAlignment'],
      ['minStorageBufferOffsetAlignment', 'minStorageBufferOffsetAlignment'],
    ];
    const mismatched = limitPairs
      .filter(
        ([seam, webgpu]) => live?.mapped?.[seam] !== live?.limits?.[webgpu]
      )
      .map(
        ([seam, webgpu]) =>
          `${seam}=${live?.mapped?.[seam]} but ${webgpu}=${live?.limits?.[webgpu]}`
      );
    const workgroup = live?.mapped?.maxComputeWorkgroupSize ?? [];
    if (
      workgroup[0] !== live?.limits?.maxComputeWorkgroupSizeX ||
      workgroup[1] !== live?.limits?.maxComputeWorkgroupSizeY ||
      workgroup[2] !== live?.limits?.maxComputeWorkgroupSizeZ
    ) {
      mismatched.push(`maxComputeWorkgroupSize=[${workgroup.join(', ')}]`);
    }
    check(
      'G',
      'every limit the browser reports lands in the seam field that names it',
      mismatched.length === 0 && workgroup.length === 3,
      mismatched.length
        ? mismatched.join('; ')
        : `${limitPairs.length + 1} mapped, on maxTextureDimension2D ${live?.limits?.maxTextureDimension2D}`
    );

    // **AND THEN A DEVICE OPENS.** The enumeration proves a command crossed and an
    // answer came back; this proves the answer was *used*. wasm encodes a
    // `RequestDevice` naming the adapter it was just granted, the replayer turns
    // its `crcbl_hal::Features` words back into `GPUFeatureName`s, `requestDevice`
    // resolves, and the device's own capabilities come home. Nothing without a
    // browser can reach any of it: `gpu-replay.mjs` drives the same code against a
    // stub that is not WebGPU.
    const deviceProbe = await evaluate(
      page,
      `(async () => {
       const { startDeviceProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startDeviceProbe({ exports }),
         replayed: before.replayed,
         delivered: before.delivered,
       };
     })()`
    );
    const deviceCarried = deviceProbe?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const stats = globalThis.crcbl.gpu.stats();
             return stats.replayed > ${deviceProbe.replayed} || stats.failure
               ? stats
               : null;
           })()`
          )
        )
      : null;
    check(
      'G',
      'wasm encoded a device request and the demo loop replayed it',
      deviceCarried?.commands?.join(',') === 'RequestDevice',
      deviceCarried
        ? `the loop replayed [${deviceCarried.commands.join(', ')}] at sequence ${deviceCarried.baseSequence}` +
            `${deviceCarried.failure ? ` — ${deviceCarried.failure}` : ''}`
        : deviceProbe?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not ask — no adapter had been granted, or the waiting set is full'
    );

    const opened = await until(async () =>
      evaluate(
        page,
        `(async () => {
         const { readDeviceProbe, DEVICE } = await import('/engine/gpu-probe.js');
         const { exports, memory, gpu } = globalThis.crcbl;
         const out = readDeviceProbe({ exports, memory });
         return out.state === DEVICE.WAITING
           ? null
           : { ...out, delivered: gpu.stats().delivered };
       })()`
      )
    );
    check(
      'G',
      'the browser opened a device and wasm read its capabilities back',
      opened?.name === 'OPENED' &&
        opened.delivered > (deviceProbe?.delivered ?? 0),
      opened
        ? `${opened.name}${opened.reason ? `: ${JSON.stringify(opened.reason)}` : ''}` +
            `, after the loop delivered ${opened.delivered - (deviceProbe?.delivered ?? 0)} reply buffer(s)`
        : `no answer in ${TIMEOUT_MS} ms — requestDevice never settled, or the reply never reached wasm`
    );

    // **What the page can see for itself.** A device opened here, in this browser,
    // with the descriptor the probe uses — no optional features, and every limit
    // the adapter reports, which is what `requiredLimitsFor` asks for — is the
    // same device WebGPU gives the replayer, so its own `features` and `limits`
    // are what wasm's numbers are held to. Asked of the browser rather than of
    // anything of ours, which is what makes it evidence. The descriptor is
    // written out here rather than imported from the engine: a reference device
    // opened by the code under test would agree with it whatever it asked for.
    const liveDevice = await evaluate(
      page,
      `(async () => {
       const adapter = await navigator.gpu.requestAdapter();
       const requiredLimits = {};
       for (const key in adapter.limits) {
         const value = adapter.limits[key];
         if (typeof value === 'number' && Number.isFinite(value)) {
           requiredLimits[key] = value;
         }
       }
       const device = await adapter.requestDevice({ requiredLimits });
       return {
         features: [...device.features].sort(),
         maxTextureDimension2D: device.limits.maxTextureDimension2D,
         adapterMaxTextureDimension2D: adapter.limits.maxTextureDimension2D,
       };
     })()`
    );
    const deviceCaps = opened?.caps;
    const expectedDeviceFeatures = featureBitsOf(liveDevice?.features);
    check(
      'G',
      "the device's feature set wasm received is what this browser's own device reports",
      deviceCaps?.featuresLo === expectedDeviceFeatures &&
        deviceCaps?.featuresHi === 0,
      `wasm got ${deviceCaps?.featuresLo?.toString(16)}/${deviceCaps?.featuresHi?.toString(16)},` +
        ` ${expectedDeviceFeatures.toString(16)} expected from [${(liveDevice?.features ?? []).join(', ')}]`
    );
    check(
      'G',
      "a limit wasm received for the device is the number this browser's device reports",
      deviceCaps?.maxImage2d === liveDevice?.maxTextureDimension2D &&
        deviceCaps?.maxImage2d > 0,
      `wasm got ${deviceCaps?.maxImage2d}, the device says` +
        ` maxTextureDimension2D ${liveDevice?.maxTextureDimension2D}`
    );

    // **The device's capabilities are not the adapter's**, which is the claim a
    // backend gets wrong for free: the adapter is right there when the reply is
    // built. WebGPU grants a device what was asked for, and the probe asks for
    // nothing optional — so on any adapter reporting an optional feature the two
    // records differ, and wasm's two sets of numbers have to differ with them.
    //
    // **The limits can no longer carry this**, and saying so is the point of
    // this paragraph. The replayer asks for every limit the adapter reports, so
    // the device's are the adapter's by construction and a copy would be
    // indistinguishable there; the features are the axis that still separates
    // them. Where an adapter reports nothing optional either, the two records
    // legitimately coincide and this says so rather than claiming a distinction
    // it could not observe. The checks above still hold the device's numbers to
    // a device either way.
    const theAdapterHasOptionalFeatures =
      expectedFeatures !== expectedDeviceFeatures;
    const wasmSaysTheyDiffer = wasmCaps?.featuresLo !== deviceCaps?.featuresLo;
    check(
      'G',
      'the device wasm was told about is a device, not a copy of its adapter',
      theAdapterHasOptionalFeatures ? wasmSaysTheyDiffer : !wasmSaysTheyDiffer,
      theAdapterHasOptionalFeatures
        ? `adapter ${wasmCaps?.featuresLo?.toString(16)} against device ${deviceCaps?.featuresLo?.toString(16)}` +
            ` (both report maxImage2d ${deviceCaps?.maxImage2d}, which is the adapter's ceiling and asked for)`
        : 'this adapter reports nothing optional, so the two feature words ' +
            'genuinely coincide here and a copy could not be told apart'
    );

    // A channel installed under a running demo must be invisible to it. The
    // demo's loop has been draining every frame throughout, and from the moment
    // the probe installed a channel it stopped answering "nothing to do" and
    // started decoding real headers, replaying them against WebGPU and handing
    // replies back — all of it between the engine's frame and the next rAF.
    const survived = await evaluate(page, `crcbl.status()`);
    check(
      'G',
      'the demo kept running with a channel installed under it',
      survived === STATUS_RUNNING,
      `status ${survived}`
    );

    // **THE ONLY GATE ON A SURFACE.** `gpu-replay.mjs` drives the same two
    // commands under node against a stub canvas whose `getContext` returns a plain
    // object, so what it proves is the bookkeeping: that the `canvasId` on the
    // wire is the key that gets looked up, and that a destroy lets go. What it
    // cannot reach is the only thing a surface finally is — `getContext('webgpu')`
    // on a real element, answering a real `GPUCanvasContext`. Nothing else runs
    // that, and the registry it is resolved against is the demo's own.
    //
    // Polled rather than read straight back, for group G's reason and no other:
    // `CreateSurface` still makes no round trip — wasm names the handle and moves
    // on — but the replay is the demo loop's, so what there is to see appears on
    // the loop's next frame rather than in this call. See
    // `crates/crcbl-webgpu/src/probe.rs`.
    group('H — a surface resolves to a real canvas context');

    const surfaceStart = await evaluate(
      page,
      `(async () => {
       const { startSurfaceProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startSurfaceProbe({ exports, canvasId: gpu.canvasId }),
         replayed: before.replayed,
         canvasId: gpu.canvasId,
       };
     })()`
    );
    // A `SurfaceError` out of the replay is the page failing to resolve the
    // canvas, and it is the one failure this group exists to catch. It is thrown
    // in the demo's loop now, which latches it and reports it as `stats().failure`
    // — so it settles this poll and lands as a red check naming what threw,
    // rather than either aborting the run or timing out with nothing to say.
    const surfaceProbe = surfaceStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const { gpu } = globalThis.crcbl;
             const stats = gpu.stats();
             if (stats.replayed <= ${surfaceStart.replayed} && !stats.failure) {
               return null;
             }
             const entries = [...gpu.replayer.surfaces.entries()];
             const last = entries[entries.length - 1];
             const surface = last ? last[0] : null;
             const context = last ? last[1] : undefined;
             return {
               commands: stats.commands,
               failure: stats.failure,
               surface,
               held: entries.length,
               // Identity, not existence. \`GPUCanvasContext.canvas\` is the
               // element the context came out of, so this is the only thing
               // that separates "the right canvas answered" from "a canvas
               // answered". Against the page's own element rather than against
               // whatever the registry holds, so a registry pointing somewhere
               // else is a failure rather than a tautology.
               isTheCanvas: context?.canvas === document.getElementById('canvas'),
               isTheDecoy: context?.canvas === globalThis.crcblProbe.decoy,
               // …and that it is the browser's own class rather than something
               // shaped like it. Node has no \`GPUCanvasContext\` binding at all,
               // so this is what a silent fall back to a stub cannot survive.
               isRealContext:
                 typeof GPUCanvasContext === 'function' &&
                 context instanceof GPUCanvasContext,
               hasCurrentTexture: typeof context?.getCurrentTexture === 'function',
             };
           })()`
          )
        )
      : null;
    check(
      'H',
      'wasm encoded a surface creation and the demo loop replayed it',
      surfaceProbe?.commands?.join(',') === 'CreateSurface' &&
        Number.isInteger(surfaceProbe.surface),
      surfaceProbe
        ? `the loop replayed [${surfaceProbe.commands.join(', ')}] for surface ${surfaceProbe.surface}` +
            `${surfaceProbe.failure ? ` — ${surfaceProbe.failure}` : ''}`
        : surfaceStart?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not encode it — another channel is installed'
    );
    check(
      'H',
      'the surface resolved the canvas the page registered and not the decoy',
      surfaceProbe?.isTheCanvas === true && surfaceProbe?.isTheDecoy === false,
      surfaceProbe?.isTheCanvas
        ? `surface ${surfaceProbe.surface} holds the context of canvas ${surfaceStart?.canvasId}, of ${surfaceProbe.held} held`
        : `the context under surface ${surfaceProbe?.surface} belongs to ` +
            `${surfaceProbe?.isTheDecoy ? `the decoy at ${DECOY_CANVAS_ID}` : 'no canvas this page registered'}` +
            `${surfaceProbe?.failure ? ` — ${surfaceProbe.failure}` : ''}`
    );
    check(
      'H',
      'a real GPUCanvasContext came from that canvas, not a stub of one',
      surfaceProbe?.isRealContext === true &&
        surfaceProbe?.hasCurrentTexture === true,
      surfaceProbe?.isRealContext
        ? 'the context is an instance of this browser GPUCanvasContext and has getCurrentTexture'
        : `instanceof GPUCanvasContext: ${surfaceProbe?.isRealContext}, ` +
            `getCurrentTexture: ${surfaceProbe?.hasCurrentTexture}` +
            `${surfaceProbe?.failure ? ` — ${surfaceProbe.failure}` : ''}`
    );

    // **THE ONLY GATE ON A CAPABILITY QUERY.** `gpu-replay.mjs` drives the same
    // command under node against a stub whose `getPreferredCanvasFormat` returns
    // whatever that file wrote a line above, so what it proves is the encoding and
    // the translation. The fact left over is the one only a browser has: **what
    // this machine's WebGPU actually prefers to put on a canvas**, which varies by
    // browser and by platform and which no fixture can contain.
    //
    // So the check below asks the page for that answer directly, at the same
    // moment, and holds wasm's number to it. A replayer that answered with a
    // constant, or a decoder that read the format list off by one, differs there
    // and nowhere else in this file.
    //
    // **There is no refusal check here, and there is nothing to replace it with.**
    // This group used to drive a surface handle nothing created and expect
    // `InvalidHandle` back. `Command::SurfaceCaps` carries no ids now — the record
    // depends on neither, so an `impl Instance` refuses a stale handle against its
    // own tables without a round trip — and the only cause left, `Backend`, is the
    // browser naming a canvas format this seam has no `Format` for, which no page
    // can be made to do. The refusal path is a `cargo test` and a `gpu-replay.mjs`
    // check; the reply channel carrying one is not a browser fact any more.
    //
    // Nothing here drains, replays or delivers, for group G's reason.
    group('I — a surface says what it will accept');

    const capsStart = await evaluate(
      page,
      `(async () => {
       const { startSurfaceCapsProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startSurfaceCapsProbe({ exports }),
         replayed: before.replayed,
         delivered: before.delivered,
       };
     })()`
    );
    const capsCarried = capsStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const stats = globalThis.crcbl.gpu.stats();
             return stats.replayed > ${capsStart.replayed} || stats.failure
               ? stats
               : null;
           })()`
          )
        )
      : null;
    check(
      'I',
      'wasm encoded a surface-capability query and the demo loop replayed it',
      capsCarried?.commands?.join(',') === 'SurfaceCaps',
      capsCarried
        ? `the loop replayed [${capsCarried.commands.join(', ')}] at sequence ${capsCarried.baseSequence}` +
            `${capsCarried.failure ? ` — ${capsCarried.failure}` : ''}`
        : capsStart?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not encode it — another channel is installed, or the waiting set is full'
    );

    // Polled like group G's answers, and for a weaker version of the same reason:
    // this command is answered inside the replay rather than out of a promise, so
    // it settles on the frame the loop replays it — which is still not the frame
    // that asked.
    const capsAnswered = await until(async () =>
      evaluate(
        page,
        `(async () => {
         const { readSurfaceCapsProbe, CAPS } = await import('/engine/gpu-probe.js');
         const { exports, memory, gpu } = globalThis.crcbl;
         const out = readSurfaceCapsProbe({ exports, memory });
         return out.state === CAPS.WAITING
           ? null
           : { ...out, delivered: gpu.stats().delivered };
       })()`
      )
    );

    // Asked of the browser, in the same page, at the same moment — the half that
    // makes the next check a round trip rather than a restatement.
    const preferredFormat = await evaluate(
      page,
      `navigator.gpu.getPreferredCanvasFormat()`
    );
    const expectedFormatCode = SEAM_FORMAT_CODE[preferredFormat];
    check(
      'I',
      'the preferred format wasm received is the one this browser prefers',
      capsAnswered?.name === 'ANSWERED' &&
        expectedFormatCode !== undefined &&
        capsAnswered.format === expectedFormatCode,
      capsAnswered?.name === 'ANSWERED'
        ? `wasm got format code ${capsAnswered.format}, navigator.gpu prefers ` +
            `${JSON.stringify(preferredFormat)} which is code ${expectedFormatCode}` +
            `, after the loop delivered ${capsAnswered.delivered - (capsStart?.delivered ?? 0)} reply buffer(s)`
        : capsAnswered
          ? `${capsAnswered.name}: ${JSON.stringify(capsAnswered.reason)}`
          : `no answer in ${TIMEOUT_MS} ms — the reply never reached wasm`
    );

    // The two promises the rest of the record carries, and the reason they are
    // here beside a format that would decode correctly whatever happened to the
    // lists behind it: an empty mode list, or an extent that appeared from
    // nowhere, is a reader that lost its place after the first field. Both are
    // invariants rather than counts — `SurfaceCaps` promises Fifo is always
    // offered, and a browser has no `currentExtent` to report at all — so neither
    // asserts a number the replayer chose.
    check(
      'I',
      'the surface offers Fifo and reports no extent of its own',
      capsAnswered?.name === 'ANSWERED' &&
        (capsAnswered.presentModes & FIFO_BIT) === FIFO_BIT &&
        capsAnswered.hasExtent === false,
      capsAnswered?.name === 'ANSWERED'
        ? `present modes 0b${capsAnswered.presentModes.toString(2)}, currentExtent ` +
            `${capsAnswered.hasExtent ? 'present' : 'absent'}`
        : `state ${capsAnswered?.name}, which carries no capabilities to read`
    );

    // **THE ONLY GATE ON A RESOURCE ACTUALLY BEING MADE.** Everything above this
    // asks the browser questions — what it has, what it prefers, what it will
    // grant. This is the first command that tells it to *do* something and keeps
    // what came back: a `crcbl_hal::BufferDesc` encoded in wasm, replayed through
    // the demo's own loop, and a real `GPUBuffer` on the real device at the end of
    // it.
    //
    // `gpu-replay.mjs` drives the same two commands under node against a stub
    // device whose `createBuffer` hands back a plain object built from the
    // descriptor — so what it proves is the translation and the bookkeeping, and
    // it would pass just as well against a replayer that never called a browser at
    // all. What only a browser can establish is what is checked here: that
    // `createBuffer` accepted the descriptor this seam builds, and that the object
    // it answered reports the size, the label and the usage that were asked for.
    // `GPUBuffer.usage` is the interesting one — it is the browser reading back a
    // word two seam fields were folded into.
    group('J — a buffer is created on the real device');

    const bufferStart = await evaluate(
      page,
      `(async () => {
       const { startBufferProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startBufferProbe({ exports, size: ${PROBE_BUFFER_BYTES} }),
         replayed: before.replayed,
       };
     })()`
    );
    const bufferProbe = bufferStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const { gpu } = globalThis.crcbl;
             const stats = gpu.stats();
             if (stats.replayed <= ${bufferStart.replayed} && !stats.failure) {
               return null;
             }
             const entries = [...gpu.replayer.buffers.entries()];
             const last = entries[entries.length - 1];
             const buffer = last ? last[1] : undefined;
             return {
               commands: stats.commands,
               failure: stats.failure,
               handle: last ? last[0] : null,
               held: entries.length,
               // The browser's own class, as group H asks of a canvas context.
               // Node has no \`GPUBuffer\` binding at all, so this is what a
               // silent fall back to a stub cannot survive.
               isRealBuffer:
                 typeof GPUBuffer === 'function' && buffer instanceof GPUBuffer,
               size: buffer?.size,
               label: buffer?.label,
               usage: buffer?.usage,
               // Built from the browser's own namespace object rather than from
               // the seam's table, which is the whole point: STORAGE and
               // TRANSFER_DST are what \`probe_buffer_desc\` asks for, and these
               // are the bits this browser calls them.
               expectedUsage:
                 GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
               // A creation that failed has no reply to arrive in, so the reason
               // is queued where \`Device::take_error\` will drain it. Read
               // whatever the outcome, because it is the only thing that can say
               // why a buffer is missing.
               error: gpu.replayer.takeError(),
             };
           })()`
          )
        )
      : null;
    check(
      'J',
      'wasm encoded a buffer creation and the demo loop replayed it',
      bufferProbe?.commands?.join(',') === 'CreateBuffer' &&
        Number.isInteger(bufferProbe.handle),
      bufferProbe
        ? `the loop replayed [${bufferProbe.commands.join(', ')}] for buffer ${bufferProbe.handle}` +
            `${bufferProbe.failure ? ` — ${bufferProbe.failure}` : ''}` +
            `${bufferProbe.error ? ` — ${bufferProbe.error}` : ''}`
        : bufferStart?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'J',
      'a real GPUBuffer came back from the device with the size that was asked for',
      bufferProbe?.isRealBuffer === true &&
        bufferProbe?.size === PROBE_BUFFER_BYTES,
      bufferProbe?.isRealBuffer
        ? `an instance of this browser's GPUBuffer, of ${bufferProbe.size} bytes, of ${bufferProbe.held} held`
        : `instanceof GPUBuffer: ${bufferProbe?.isRealBuffer}, size ${bufferProbe?.size}` +
            ` (${PROBE_BUFFER_BYTES} asked for)` +
            `${bufferProbe?.error ? ` — ${bufferProbe.error}` : ''}`
    );
    check(
      'J',
      'the browser gave that buffer the usage and the label the seam asked for',
      bufferProbe?.usage === bufferProbe?.expectedUsage &&
        bufferProbe?.label === PROBE_BUFFER_LABEL,
      `usage 0x${bufferProbe?.usage?.toString(16)} against 0x${bufferProbe?.expectedUsage?.toString(16)}` +
        ` from GPUBufferUsage, label ${JSON.stringify(bufferProbe?.label)}`
    );

    // **THE ONLY GATE ON AN IMAGE, AND ON A RESOURCE MADE FROM ANOTHER ONE.**
    // Group J watches this seam make something on the device; this watches it make
    // something on *that*, which is the shape no command before it has: a
    // `GPUTextureView` comes from the texture and not from the device, so the
    // image handle on the wire has to resolve to a live `GPUTexture` in the
    // replayer's own table before there is anything to call.
    //
    // `gpu-replay.mjs` drives both commands under node against a stub device whose
    // `createTexture` hands back a plain object built from the descriptor, so what
    // it proves is the translation and the bookkeeping — every table row, every
    // refusal, and that the range's sentinel reaches `createView` as an absent
    // member rather than as `4294967295`. What only a browser can establish is
    // that a real `createTexture` accepted the descriptor this seam builds, that
    // the `GPUTexture` it answered reports the extent, format, mip count, usage
    // and label that were asked for, and that a real `createView` accepted the
    // resolved range — which the number on the wire would have been refused for.
    group('K — an image and a view of it are created on the real device');

    const imageStart = await evaluate(
      page,
      `(async () => {
       const { startImageProbe, startImageViewProbe } =
         await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       // Both on one frame, deliberately: the view names the image the command
       // before it created, so replaying them together is what says the
       // replayer's table is filled in before the lookup rather than a frame
       // later.
       const image = startImageProbe({
         exports,
         width: ${PROBE_IMAGE_WIDTH},
         height: ${PROBE_IMAGE_HEIGHT},
         mipLevels: ${PROBE_IMAGE_MIPS},
       });
       return {
         started: image && startImageViewProbe({ exports }),
         replayed: before.replayed,
       };
     })()`
    );
    const imageProbe = imageStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const { gpu } = globalThis.crcbl;
             const stats = gpu.stats();
             if (stats.replayed <= ${imageStart.replayed} && !stats.failure) {
               return null;
             }
             const images = [...gpu.replayer.images.entries()];
             const views = [...gpu.replayer.imageViews.entries()];
             const image = images.length ? images[images.length - 1][1] : undefined;
             const view = views.length ? views[views.length - 1][1] : undefined;
             return {
               commands: stats.commands,
               failure: stats.failure,
               imageHandle: images.length ? images[images.length - 1][0] : null,
               viewHandle: views.length ? views[views.length - 1][0] : null,
               heldImages: images.length,
               heldViews: views.length,
               // The browser's own classes, as groups H and J ask. Node has no
               // \`GPUTexture\` or \`GPUTextureView\` binding at all, so this is
               // what a silent fall back to a stub cannot survive.
               isRealTexture:
                 typeof GPUTexture === 'function' && image instanceof GPUTexture,
               isRealView:
                 typeof GPUTextureView === 'function' &&
                 view instanceof GPUTextureView,
               width: image?.width,
               height: image?.height,
               depthOrArrayLayers: image?.depthOrArrayLayers,
               mipLevelCount: image?.mipLevelCount,
               dimension: image?.dimension,
               format: image?.format,
               usage: image?.usage,
               label: image?.label,
               viewLabel: view?.label,
               // Built from the browser's own namespace object rather than from
               // the seam's table, as group J does: SAMPLED and TRANSFER_DST are
               // what \`probe_image_desc\` asks for, and these are the bits this
               // browser calls them.
               expectedUsage:
                 GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
             };
           })()`
          )
        )
      : null;

    // **THE ONLY THING THAT CAN SAY THE VIEW'S RANGE WAS ACCEPTED**, and it is
    // read a moment after the replay rather than during it. A `GPUTextureView`
    // reports nothing but its label, so one built from a range the browser refused
    // is indistinguishable from a good one by inspection — the refusal arrives on
    // the *device's* error channel instead, which is the queue
    // `Device::take_error` drains and which `gpu-replay.js` feeds from its
    // `uncapturederror` listener.
    //
    // WebGPU raises that error "in a future task", so reading the queue in the
    // same evaluation that saw the replay finish reads it too early and finds an
    // empty queue whatever happened — which is a check that cannot fail. Two
    // animation frames inside the page is the wait, and it is a wait for an
    // *absence*: there is no event that says "no error is coming", so a bounded
    // one is the only shape available. Measured against the failure it guards —
    // `ImageSubresourceRange::ALL` passed on as `4294967295`, which Chromium
    // refuses — this is enough and reading immediately is not.
    const deviceReport = imageProbe
      ? await evaluate(
          page,
          `(async () => {
           await new Promise((settle) =>
             requestAnimationFrame(() => requestAnimationFrame(settle))
           );
           // Wrapped rather than returned bare, so that an evaluation that
           // never ran is distinguishable from a queue that was empty. Both
           // would arrive here as \`null\`, and one of them is a check that
           // cannot fail.
           return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
         })()`
        )
      : null;
    check(
      'K',
      'wasm encoded an image and a view of it, and the demo loop replayed both',
      imageProbe?.commands?.join(',') === 'CreateImage,CreateImageView' &&
        Number.isInteger(imageProbe.imageHandle) &&
        Number.isInteger(imageProbe.viewHandle),
      imageProbe
        ? `the loop replayed [${imageProbe.commands.join(', ')}] for image ${imageProbe.imageHandle} and view ${imageProbe.viewHandle}` +
            `${imageProbe.failure ? ` — ${imageProbe.failure}` : ''}`
        : imageStart?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not encode them — no device has opened, or another channel is installed'
    );
    check(
      'K',
      'a real GPUTexture came back from the device with the extent, format and mip count asked for',
      imageProbe?.isRealTexture === true &&
        imageProbe?.width === PROBE_IMAGE_WIDTH &&
        imageProbe?.height === PROBE_IMAGE_HEIGHT &&
        imageProbe?.depthOrArrayLayers === 1 &&
        imageProbe?.mipLevelCount === PROBE_IMAGE_MIPS &&
        imageProbe?.dimension === '2d' &&
        imageProbe?.format === 'rgba8unorm',
      imageProbe?.isRealTexture
        ? `an instance of this browser's GPUTexture, ${imageProbe.width}x${imageProbe.height}x${imageProbe.depthOrArrayLayers} ${imageProbe.dimension} ${imageProbe.format}` +
            ` with ${imageProbe.mipLevelCount} mips, of ${imageProbe.heldImages} held`
        : `instanceof GPUTexture: ${imageProbe?.isRealTexture}, ${imageProbe?.width}x${imageProbe?.height}` +
            ` ${imageProbe?.dimension} ${imageProbe?.format}, ${imageProbe?.mipLevelCount} mips` +
            ` (${PROBE_IMAGE_WIDTH}x${PROBE_IMAGE_HEIGHT} 2d rgba8unorm, ${PROBE_IMAGE_MIPS} mips asked for)`
    );
    check(
      'K',
      'the browser gave that texture the usage and the label the seam asked for',
      imageProbe?.usage === imageProbe?.expectedUsage &&
        imageProbe?.label === PROBE_IMAGE_LABEL,
      `usage 0x${imageProbe?.usage?.toString(16)} against 0x${imageProbe?.expectedUsage?.toString(16)}` +
        ` from GPUTextureUsage, label ${JSON.stringify(imageProbe?.label)}`
    );
    check(
      'K',
      'a real GPUTextureView came back from that texture with the whole-image range accepted',
      imageProbe?.isRealView === true &&
        imageProbe?.viewLabel === PROBE_VIEW_LABEL &&
        deviceReport?.waited === true &&
        deviceReport?.error === null,
      deviceReport?.waited !== true
        ? 'the page never got as far as reading the device error queue'
        : imageProbe?.isRealView && deviceReport.error === null
          ? `an instance of this browser's GPUTextureView labelled ${JSON.stringify(imageProbe.viewLabel)}, of ${imageProbe.heldViews} held, and the device reported nothing`
          : `instanceof GPUTextureView: ${imageProbe?.isRealView}, label ${JSON.stringify(imageProbe?.viewLabel)}` +
            `${deviceReport.error ? ` — ${deviceReport.error}` : ''}`
    );

    // **THE ONLY GATE ON A SAMPLER, AND THE ONE WHERE THE OBJECT SAYS LEAST.**
    // Group J watches this seam make something the browser then describes back —
    // a `GPUBuffer` reports its size, usage and label — and group K watches it
    // make something that describes back nine members. A `GPUSampler` reports its
    // `label` and nothing else: no filters, no address modes, no clamps. So there
    // is no "pass a number in and read it back" check available here, and the two
    // things that are:
    //
    //   * the class of what came back, which `instanceof GPUSampler` settles and
    //     no stub can satisfy — node has no such binding at all; and
    //   * the device's error queue being empty afterwards, which is the only thing
    //     anywhere that can say `createSampler` *accepted* the descriptor this
    //     seam built.
    //
    // That second one is what this group exists for, because the descriptor
    // carries `lod_max: f32::MAX` — `SamplerDesc::default`'s "no limit" sentinel.
    // It crosses the wire verbatim, and the replayer has to hand WebGPU an
    // explicit `lodMaxClamp` holding it: omitting the member, which is how the
    // *view's* range sentinel is spelled one group earlier, would substitute
    // WebGPU's own default — a number rather than "the rest" — and nothing
    // downstream reports a mip clamp. `gpu-replay.mjs` proves the descriptor this
    // seam builds against a stub; only a real `createSampler` can say the browser
    // takes it.
    group('L — a sampler is created on the real device');

    const samplerStart = await evaluate(
      page,
      `(async () => {
       const { startSamplerProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startSamplerProbe({ exports }),
         replayed: before.replayed,
       };
     })()`
    );
    const samplerProbe = samplerStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const { gpu } = globalThis.crcbl;
             const stats = gpu.stats();
             if (stats.replayed <= ${samplerStart.replayed} && !stats.failure) {
               return null;
             }
             const entries = [...gpu.replayer.samplers.entries()];
             const last = entries[entries.length - 1];
             const sampler = last ? last[1] : undefined;
             return {
               commands: stats.commands,
               failure: stats.failure,
               handle: last ? last[0] : null,
               held: entries.length,
               // The browser's own class, as groups H, J and K ask. Node has no
               // \`GPUSampler\` binding at all, so this is what a silent fall
               // back to a stub cannot survive.
               isRealSampler:
                 typeof GPUSampler === 'function' &&
                 sampler instanceof GPUSampler,
               // Every other member of a \`GPUSampler\` is absent by design, so
               // this is the whole of what the object can be asked.
               label: sampler?.label,
             };
           })()`
          )
        )
      : null;

    // Read a moment after the replay rather than during it, for the reason group K
    // spells out: WebGPU raises a validation error "in a future task", so a queue
    // read in the evaluation that saw the replay finish is empty whatever
    // happened — a check that cannot fail. Two animation frames is the same
    // bounded wait for the same absence, and the failure it guards here is the one
    // this group exists for: `lod_max`'s sentinel reaching the browser as
    // something `createSampler` refuses.
    const samplerReport = samplerProbe
      ? await evaluate(
          page,
          `(async () => {
           await new Promise((settle) =>
             requestAnimationFrame(() => requestAnimationFrame(settle))
           );
           return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
         })()`
        )
      : null;
    check(
      'L',
      'wasm encoded a sampler creation and the demo loop replayed it',
      samplerProbe?.commands?.join(',') === 'CreateSampler' &&
        Number.isInteger(samplerProbe.handle),
      samplerProbe
        ? `the loop replayed [${samplerProbe.commands.join(', ')}] for sampler ${samplerProbe.handle}` +
            `${samplerProbe.failure ? ` — ${samplerProbe.failure}` : ''}`
        : samplerStart?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'L',
      'a real GPUSampler came back from the device with the no-limit lod clamp accepted',
      samplerProbe?.isRealSampler === true &&
        samplerProbe?.label === PROBE_SAMPLER_LABEL &&
        samplerReport?.waited === true &&
        samplerReport?.error === null,
      samplerReport?.waited !== true
        ? 'the page never got as far as reading the device error queue'
        : samplerProbe?.isRealSampler && samplerReport.error === null
          ? `an instance of this browser's GPUSampler labelled ${JSON.stringify(samplerProbe.label)}, of ${samplerProbe.held} held, and the device reported nothing`
          : `instanceof GPUSampler: ${samplerProbe?.isRealSampler}, label ${JSON.stringify(samplerProbe?.label)}` +
            `${samplerReport.error ? ` — ${samplerReport.error}` : ''}`
    );

    // **THE ONLY GATE ON A LIST.** Every command groups G to L put in front of a
    // browser is a fixed set of fields; this one's body is a counted list of
    // structs, each five fields deep, each carrying an enum whose variants have
    // different-length payloads. A stride out by a byte therefore does not
    // truncate — it decodes the next entry out of the middle of this one and
    // produces a layout that is well-formed and describes different resources.
    //
    // A `GPUBindGroupLayout` reports its `label` and nothing else, exactly as a
    // `GPUSampler` does, so the two things available here are group L's two: the
    // class of what came back, which `instanceof GPUBindGroupLayout` settles and
    // no stub can satisfy — node has no such binding — and the device's error
    // queue being empty afterwards, which is the only thing anywhere that can say
    // `createBindGroupLayout` *accepted* the four-entry descriptor this seam
    // built. `gpu-replay.mjs` proves that descriptor against a stub, entry for
    // entry, and proves every refusal; only a real device can say the browser
    // takes it.
    group('M — a bind-group layout is created on the real device');

    const layoutStart = await evaluate(
      page,
      `(async () => {
       const { startBindGroupLayoutProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startBindGroupLayoutProbe({ exports }),
         replayed: before.replayed,
       };
     })()`
    );
    const layoutProbe = layoutStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const { gpu } = globalThis.crcbl;
             const stats = gpu.stats();
             if (stats.replayed <= ${layoutStart.replayed} && !stats.failure) {
               return null;
             }
             const entries = [...gpu.replayer.bindGroupLayouts.entries()];
             const last = entries[entries.length - 1];
             const layout = last ? last[1] : undefined;
             return {
               commands: stats.commands,
               failure: stats.failure,
               handle: last ? last[0] : null,
               held: entries.length,
               // The browser's own class, as groups H, J, K and L ask. Node has
               // no \`GPUBindGroupLayout\` binding at all, so this is what a
               // silent fall back to a stub cannot survive.
               isRealLayout:
                 typeof GPUBindGroupLayout === 'function' &&
                 layout instanceof GPUBindGroupLayout,
               // Every other member of a \`GPUBindGroupLayout\` is absent by
               // design, so this is the whole of what the object can be asked.
               label: layout?.label,
             };
           })()`
          )
        )
      : null;

    // Read a moment after the replay rather than during it, for the reason groups
    // K and L spell out: WebGPU raises a validation error "in a future task", so a
    // queue read in the evaluation that saw the replay finish is empty whatever
    // happened. The failure it guards here is the list: an entry's `visibility`
    // arriving as zero, a `texture` member missing its `sampleType`, a
    // `hasDynamicOffset` under a name WebIDL ignores — every one of those is a
    // layout the browser refuses and nothing else reports.
    const layoutReport = layoutProbe
      ? await evaluate(
          page,
          `(async () => {
           await new Promise((settle) =>
             requestAnimationFrame(() => requestAnimationFrame(settle))
           );
           return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
         })()`
        )
      : null;
    check(
      'M',
      'wasm encoded a bind-group layout creation and the demo loop replayed it',
      layoutProbe?.commands?.join(',') === 'CreateBindGroupLayout' &&
        Number.isInteger(layoutProbe.handle),
      layoutProbe
        ? `the loop replayed [${layoutProbe.commands.join(', ')}] for layout ${layoutProbe.handle}` +
            `${layoutProbe.failure ? ` — ${layoutProbe.failure}` : ''}`
        : layoutStart?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'M',
      'a real GPUBindGroupLayout came back from the device with every entry accepted',
      layoutProbe?.isRealLayout === true &&
        layoutProbe?.label === PROBE_LAYOUT_LABEL &&
        layoutReport?.waited === true &&
        layoutReport?.error === null,
      layoutReport?.waited !== true
        ? 'the page never got as far as reading the device error queue'
        : layoutProbe?.isRealLayout && layoutReport.error === null
          ? `an instance of this browser's GPUBindGroupLayout labelled ${JSON.stringify(layoutProbe.label)}, built from ${PROBE_LAYOUT_ENTRIES} entries, of ${layoutProbe.held} held, and the device reported nothing`
          : `instanceof GPUBindGroupLayout: ${layoutProbe?.isRealLayout}, label ${JSON.stringify(layoutProbe?.label)}` +
            `${layoutReport.error ? ` — ${layoutReport.error}` : ''}`
    );

    // **THE ONLY GATE ON A COMMAND THAT NAMES OTHER RESOURCES.** Every command
    // groups G to M put in front of a browser stands alone; this one binds a
    // layout, a buffer, an image view and a sampler that have to exist first, so
    // wasm records a whole frame — the layout, the four resources, then the group —
    // and the group's entries carry one handle into each of three resource tables.
    // A handle carries no kind, so the entry's discriminant is the only thing that
    // says which table an id indexes, and the whole-buffer binding's size crosses as
    // the `u64::MAX` sentinel and has to reach WebGPU as an *absent* member.
    //
    // A `GPUBindGroup` reports its `label` and nothing else, exactly as a
    // `GPUBindGroupLayout` does, so the two things available here are group M's two:
    // the class of what came back, which `instanceof GPUBindGroup` settles and no
    // stub can satisfy — node has no such binding — and the device's error queue
    // being empty afterwards, which is the only thing that can say `createBindGroup`
    // *accepted* the descriptor, its `WHOLE_BUFFER` binding and all three resource
    // kinds. `gpu-replay.mjs` proves that descriptor against a stub and proves every
    // refusal; only a real device can say the browser takes it.
    group('N — a bind group is created on the real device');

    const bindGroupStart = await evaluate(
      page,
      `(async () => {
       const { startBindGroupProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startBindGroupProbe({ exports }),
         replayed: before.replayed,
       };
     })()`
    );
    const bindGroupProbe = bindGroupStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const { gpu } = globalThis.crcbl;
             const stats = gpu.stats();
             if (stats.replayed <= ${bindGroupStart.replayed} && !stats.failure) {
               return null;
             }
             const entries = [...gpu.replayer.bindGroups.entries()];
             const last = entries[entries.length - 1];
             const group = last ? last[1] : undefined;
             return {
               commands: stats.commands,
               failure: stats.failure,
               handle: last ? last[0] : null,
               held: entries.length,
               // The browser's own class, as groups H, J, K, L and M ask. Node has
               // no \`GPUBindGroup\` binding at all, so this is what a silent fall
               // back to a stub cannot survive.
               isRealGroup:
                 typeof GPUBindGroup === 'function' &&
                 group instanceof GPUBindGroup,
               // Every other member of a \`GPUBindGroup\` is absent by design, so
               // this is the whole of what the object can be asked.
               label: group?.label,
             };
           })()`
          )
        )
      : null;

    // Read a moment after the replay rather than during it, for the reason groups
    // K, L and M spell out: WebGPU raises a validation error "in a future task", so
    // a queue read in the evaluation that saw the replay finish is empty whatever
    // happened. The failure it guards here is the resolution: a `WHOLE_BUFFER` size
    // passed on as `18446744073709551615`, a resource resolved against the wrong
    // table, a layout the group does not match — every one of those is a group the
    // browser refuses and nothing else reports.
    const bindGroupReport = bindGroupProbe
      ? await evaluate(
          page,
          `(async () => {
           await new Promise((settle) =>
             requestAnimationFrame(() => requestAnimationFrame(settle))
           );
           return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
         })()`
        )
      : null;
    check(
      'N',
      'wasm encoded a bind group creation and the demo loop replayed it',
      bindGroupProbe?.commands?.join(',') ===
        'CreateBindGroupLayout,CreateBuffer,CreateImage,CreateImageView,CreateSampler,CreateBindGroup' &&
        Number.isInteger(bindGroupProbe.handle),
      bindGroupProbe
        ? `the loop replayed [${bindGroupProbe.commands.join(', ')}] for group ${bindGroupProbe.handle}` +
            `${bindGroupProbe.failure ? ` — ${bindGroupProbe.failure}` : ''}`
        : bindGroupStart?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'N',
      'a real GPUBindGroup came back from the device with the whole-buffer binding and all three resource kinds accepted',
      bindGroupProbe?.isRealGroup === true &&
        bindGroupProbe?.label === PROBE_BIND_GROUP_LABEL &&
        bindGroupReport?.waited === true &&
        bindGroupReport?.error === null,
      bindGroupReport?.waited !== true
        ? 'the page never got as far as reading the device error queue'
        : bindGroupProbe?.isRealGroup && bindGroupReport.error === null
          ? `an instance of this browser's GPUBindGroup labelled ${JSON.stringify(bindGroupProbe.label)}, binding a buffer, a view and a sampler, of ${bindGroupProbe.held} held, and the device reported nothing`
          : `instanceof GPUBindGroup: ${bindGroupProbe?.isRealGroup}, label ${JSON.stringify(bindGroupProbe?.label)}` +
            `${bindGroupReport.error ? ` — ${bindGroupReport.error}` : ''}`
    );

    // A `GPUShaderModule` reports its `label`, like a sampler, a layout and a bind
    // group — but it is the only object this seam makes where *compilation* happens,
    // so group O has a second piece of evidence the others do not: `getCompilationInfo()`.
    // The descriptor carries a known-good WGSL vertex entry, so beyond
    // `instanceof GPUShaderModule` — which no stub can satisfy, node having no such
    // binding — the gate reads the compilation info off the object and holds it to
    // no errors. That is stronger than existence: a module that came back but would
    // not compile is exactly what a browser answers for bad WGSL without throwing,
    // and only `getCompilationInfo()` catches it. `gpu-replay.mjs` proves the
    // descriptor — WGSL alone, the other three artifacts dropped — against a stub,
    // and proves the WGSL-less refusal; only this asks a real device to compile it.
    group('O — a shader module is compiled on the real device');

    const shaderStart = await evaluate(
      page,
      `(async () => {
       const { startShaderModuleProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startShaderModuleProbe({ exports }),
         replayed: before.replayed,
       };
     })()`
    );
    const shaderProbe = shaderStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const { gpu } = globalThis.crcbl;
             const stats = gpu.stats();
             if (stats.replayed <= ${shaderStart.replayed} && !stats.failure) {
               return null;
             }
             const entries = [...gpu.replayer.shaderModules.entries()];
             const last = entries[entries.length - 1];
             const module = last ? last[1] : undefined;
             return {
               commands: stats.commands,
               failure: stats.failure,
               handle: last ? last[0] : null,
               held: entries.length,
               // The browser's own class, as groups H, J–N ask. Node has no
               // \`GPUShaderModule\` binding at all, so this is what a silent fall
               // back to a stub cannot survive.
               isRealModule:
                 typeof GPUShaderModule === 'function' &&
                 module instanceof GPUShaderModule,
               label: module?.label,
             };
           })()`
          )
        )
      : null;

    // Read the compilation info in a second evaluation, because it is async and
    // needs the module object. This is the check no other group has: a shader
    // module is where compilation happens, and a browser reports a bad WGSL not by
    // throwing but through this report — so a clean one is the proof the WGSL this
    // seam sent compiled, which is stronger than the module merely existing. The
    // device error queue is read after a couple of frames too, as groups K–N do,
    // for anything WebGPU raises in a future task.
    const shaderReport = shaderProbe?.isRealModule
      ? await evaluate(
          page,
          `(async () => {
           const entries = [...globalThis.crcbl.gpu.replayer.shaderModules.entries()];
           const last = entries[entries.length - 1];
           const module = last ? last[1] : undefined;
           const info = module ? await module.getCompilationInfo() : null;
           const errors = info
             ? info.messages
                 .filter((m) => m.type === 'error')
                 .map((m) => m.message)
             : ['the module was gone before getCompilationInfo could run'];
           await new Promise((settle) =>
             requestAnimationFrame(() => requestAnimationFrame(settle))
           );
           return {
             waited: true,
             errors,
             error: globalThis.crcbl.gpu.replayer.takeError(),
           };
         })()`
        )
      : null;
    check(
      'O',
      'wasm encoded a shader module creation and the demo loop replayed it',
      shaderProbe?.commands?.join(',') === 'CreateShaderModule' &&
        Number.isInteger(shaderProbe.handle),
      shaderProbe
        ? `the loop replayed [${shaderProbe.commands.join(', ')}] for module ${shaderProbe.handle}` +
            `${shaderProbe.failure ? ` — ${shaderProbe.failure}` : ''}`
        : shaderStart?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'O',
      'a real GPUShaderModule came back from the device with clean compilation info for the known-good WGSL',
      shaderProbe?.isRealModule === true &&
        shaderProbe?.label === PROBE_SHADER_MODULE_LABEL &&
        shaderReport?.waited === true &&
        Array.isArray(shaderReport?.errors) &&
        shaderReport.errors.length === 0 &&
        shaderReport?.error === null,
      shaderReport?.waited !== true
        ? 'the page never got as far as reading the module’s compilation info'
        : shaderProbe?.isRealModule &&
            shaderReport.errors.length === 0 &&
            shaderReport.error === null
          ? `an instance of this browser's GPUShaderModule labelled ${JSON.stringify(shaderProbe.label)}, getCompilationInfo reported no errors, of ${shaderProbe.held} held, and the device reported nothing`
          : `instanceof GPUShaderModule: ${shaderProbe?.isRealModule}, label ${JSON.stringify(shaderProbe?.label)}, compilation errors ${JSON.stringify(shaderReport?.errors)}` +
            `${shaderReport?.error ? ` — ${shaderReport.error}` : ''}`
    );

    // A `GPUPipelineLayout` reports its `label` and nothing else — not its
    // bind-group layouts, not its push-constant ranges (WebGPU has none) — so group
    // P's evidence is group N's two: the class of what came back, which
    // `instanceof GPUPipelineLayout` settles and no stub can satisfy (node has no
    // such binding), and the device's error queue being empty afterwards, which is
    // the only thing that can say `createPipelineLayout` *accepted* the set list it
    // was handed. The probe records a bind-group layout and then a pipeline layout
    // built from it, with `push_constants: None` so it builds rather than being
    // refused. `gpu-replay.mjs` proves that descriptor and every refusal — a `Some`
    // push-constant range, an unresolvable set — against a stub; only a real device
    // can say the browser takes it.
    group('P — a pipeline layout is created on the real device');

    const pipelineLayoutStart = await evaluate(
      page,
      `(async () => {
       const { startPipelineLayoutProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startPipelineLayoutProbe({ exports }),
         replayed: before.replayed,
       };
     })()`
    );
    const pipelineLayoutProbe = pipelineLayoutStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const { gpu } = globalThis.crcbl;
             const stats = gpu.stats();
             if (stats.replayed <= ${pipelineLayoutStart.replayed} && !stats.failure) {
               return null;
             }
             const entries = [...gpu.replayer.pipelineLayouts.entries()];
             const last = entries[entries.length - 1];
             const layout = last ? last[1] : undefined;
             return {
               commands: stats.commands,
               failure: stats.failure,
               handle: last ? last[0] : null,
               held: entries.length,
               // The browser's own class, as groups H, J–O ask. Node has no
               // \`GPUPipelineLayout\` binding at all, so this is what a silent
               // fall back to a stub cannot survive.
               isRealLayout:
                 typeof GPUPipelineLayout === 'function' &&
                 layout instanceof GPUPipelineLayout,
               // Every other member of a \`GPUPipelineLayout\` is absent by
               // design, so this is the whole of what the object can be asked.
               label: layout?.label,
             };
           })()`
          )
        )
      : null;

    // Read a moment after the replay rather than during it, for the reason groups
    // K–N spell out: WebGPU raises a validation error "in a future task", so a
    // queue read in the evaluation that saw the replay finish is empty whatever
    // happened. The failure it guards here is the set resolution: a bind-group
    // layout the pipeline layout could not find, or a set list the browser refused.
    const pipelineLayoutReport = pipelineLayoutProbe
      ? await evaluate(
          page,
          `(async () => {
           await new Promise((settle) =>
             requestAnimationFrame(() => requestAnimationFrame(settle))
           );
           return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
         })()`
        )
      : null;
    check(
      'P',
      'wasm encoded a pipeline layout creation and the demo loop replayed it',
      pipelineLayoutProbe?.commands?.join(',') ===
        'CreateBindGroupLayout,CreatePipelineLayout' &&
        Number.isInteger(pipelineLayoutProbe.handle),
      pipelineLayoutProbe
        ? `the loop replayed [${pipelineLayoutProbe.commands.join(', ')}] for pipeline layout ${pipelineLayoutProbe.handle}` +
            `${pipelineLayoutProbe.failure ? ` — ${pipelineLayoutProbe.failure}` : ''}`
        : pipelineLayoutStart?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'P',
      'a real GPUPipelineLayout came back from the device with the bind-group layout set accepted',
      pipelineLayoutProbe?.isRealLayout === true &&
        pipelineLayoutProbe?.label === PROBE_PIPELINE_LAYOUT_LABEL &&
        pipelineLayoutReport?.waited === true &&
        pipelineLayoutReport?.error === null,
      pipelineLayoutReport?.waited !== true
        ? 'the page never got as far as reading the device error queue'
        : pipelineLayoutProbe?.isRealLayout &&
            pipelineLayoutReport.error === null
          ? `an instance of this browser's GPUPipelineLayout labelled ${JSON.stringify(pipelineLayoutProbe.label)}, built from one bind-group layout, of ${pipelineLayoutProbe.held} held, and the device reported nothing`
          : `instanceof GPUPipelineLayout: ${pipelineLayoutProbe?.isRealLayout}, label ${JSON.stringify(pipelineLayoutProbe?.label)}` +
            `${pipelineLayoutReport.error ? ` — ${pipelineLayoutReport.error}` : ''}`
    );

    // A `GPUComputePipeline` reports its `label`, like a pipeline layout — but it
    // is the first object this seam makes that resolves handles into two *different*
    // tables (its layout and its compute module) and where the shader is bound to
    // the layout, so group Q has evidence group P does not: `getBindGroupLayout(0)`,
    // the derived layout only a genuinely-built pipeline answers. So beyond
    // `instanceof GPUComputePipeline` — which no stub can satisfy, node having no
    // such binding — the gate calls `getBindGroupLayout(0)` and reads the device's
    // error queue after a settle. `gpu-replay.mjs` proves the descriptor — its two
    // resolved handles and, the field that matters most, no workgroup-size member —
    // and the two distinct resolution refusals against a stub; only this asks a real
    // device to build the pipeline from a real compute shader.
    group('Q — a compute pipeline is built on the real device');

    const computePipelineStart = await evaluate(
      page,
      `(async () => {
       const { startComputePipelineProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startComputePipelineProbe({ exports }),
         replayed: before.replayed,
       };
     })()`
    );
    const computePipelineProbe = computePipelineStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const { gpu } = globalThis.crcbl;
             const stats = gpu.stats();
             if (stats.replayed <= ${computePipelineStart.replayed} && !stats.failure) {
               return null;
             }
             const entries = [...gpu.replayer.computePipelines.entries()];
             const last = entries[entries.length - 1];
             const pipeline = last ? last[1] : undefined;
             let derivedLayout = false;
             try {
               // The call no stub and no half-built pipeline can answer: a real
               // GPUComputePipeline hands back a GPUBindGroupLayout for group 0,
               // because a pipeline is where the shader and its layout are bound.
               derivedLayout =
                 typeof GPUBindGroupLayout === 'function' &&
                 pipeline?.getBindGroupLayout(0) instanceof GPUBindGroupLayout;
             } catch (error) {
               derivedLayout = false;
             }
             return {
               commands: stats.commands,
               failure: stats.failure,
               handle: last ? last[0] : null,
               held: entries.length,
               // The browser's own class, as groups H, J–P ask. Node has no
               // \`GPUComputePipeline\` binding at all, so this is what a silent
               // fall back to a stub cannot survive.
               isRealPipeline:
                 typeof GPUComputePipeline === 'function' &&
                 pipeline instanceof GPUComputePipeline,
               derivedLayout,
               label: pipeline?.label,
             };
           })()`
          )
        )
      : null;

    // Read the device error queue a moment after the replay rather than during it,
    // for the reason groups K–P spell out: WebGPU raises a validation error "in a
    // future task", and `createComputePipeline` reports a bad entry point or a
    // shader/layout mismatch through `uncapturederror` a task later — so a queue
    // read in the evaluation that saw the replay finish is empty whatever happened.
    const computePipelineReport = computePipelineProbe
      ? await evaluate(
          page,
          `(async () => {
           await new Promise((settle) =>
             requestAnimationFrame(() => requestAnimationFrame(settle))
           );
           return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
         })()`
        )
      : null;
    check(
      'Q',
      'wasm encoded a compute pipeline creation and the demo loop replayed it',
      computePipelineProbe?.commands?.join(',') ===
        'CreateShaderModule,CreatePipelineLayout,CreateComputePipeline' &&
        Number.isInteger(computePipelineProbe.handle),
      computePipelineProbe
        ? `the loop replayed [${computePipelineProbe.commands.join(', ')}] for compute pipeline ${computePipelineProbe.handle}` +
            `${computePipelineProbe.failure ? ` — ${computePipelineProbe.failure}` : ''}`
        : computePipelineStart?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'Q',
      'a real GPUComputePipeline came back from the device and answered getBindGroupLayout',
      computePipelineProbe?.isRealPipeline === true &&
        computePipelineProbe?.derivedLayout === true &&
        computePipelineProbe?.label === PROBE_COMPUTE_PIPELINE_LABEL &&
        computePipelineReport?.waited === true &&
        computePipelineReport?.error === null,
      computePipelineReport?.waited !== true
        ? 'the page never got as far as reading the device error queue'
        : computePipelineProbe?.isRealPipeline &&
            computePipelineProbe?.derivedLayout &&
            computePipelineReport.error === null
          ? `an instance of this browser's GPUComputePipeline labelled ${JSON.stringify(computePipelineProbe.label)}, its getBindGroupLayout(0) a real GPUBindGroupLayout, of ${computePipelineProbe.held} held, and the device reported nothing`
          : `instanceof GPUComputePipeline: ${computePipelineProbe?.isRealPipeline}, getBindGroupLayout(0) real: ${computePipelineProbe?.derivedLayout}, label ${JSON.stringify(computePipelineProbe?.label)}` +
            `${computePipelineReport?.error ? ` — ${computePipelineReport.error}` : ''}`
    );

    // A `GPURenderPipeline` answers `getBindGroupLayout(0)` like a compute pipeline
    // — but it is the *largest* descriptor on the seam, so group R is what puts its
    // whole nested tree (a `TriangleList` primitive, a reversed-Z depth-stencil, a
    // single-sampled multisample state, and a blended `Rgba8Unorm` target) in front
    // of a real device. `gpu-replay.mjs` proves the descriptor and every "WebGPU
    // cannot express it" refusal against a stub; only this asks a real device to
    // build the pipeline from a real vertex and fragment shader. Beyond
    // `instanceof GPURenderPipeline` — which no stub can satisfy, node having no
    // such binding — the gate calls `getBindGroupLayout(0)` and reads the device's
    // error queue after a settle.
    group('R — a graphics pipeline is built on the real device');

    const graphicsPipelineStart = await evaluate(
      page,
      `(async () => {
       const { startGraphicsPipelineProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startGraphicsPipelineProbe({ exports }),
         replayed: before.replayed,
       };
     })()`
    );
    const graphicsPipelineProbe = graphicsPipelineStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(() => {
             const { gpu } = globalThis.crcbl;
             const stats = gpu.stats();
             if (stats.replayed <= ${graphicsPipelineStart.replayed} && !stats.failure) {
               return null;
             }
             const entries = [...gpu.replayer.graphicsPipelines.entries()];
             const last = entries[entries.length - 1];
             const pipeline = last ? last[1] : undefined;
             let derivedLayout = false;
             try {
               // The call no stub and no half-built pipeline can answer: a real
               // GPURenderPipeline hands back a GPUBindGroupLayout for group 0,
               // because a pipeline is where the shaders and their layout are bound.
               derivedLayout =
                 typeof GPUBindGroupLayout === 'function' &&
                 pipeline?.getBindGroupLayout(0) instanceof GPUBindGroupLayout;
             } catch (error) {
               derivedLayout = false;
             }
             return {
               commands: stats.commands,
               failure: stats.failure,
               handle: last ? last[0] : null,
               held: entries.length,
               // The browser's own class. Node has no \`GPURenderPipeline\`
               // binding at all, so this is what a silent fall back to a stub
               // cannot survive.
               isRealPipeline:
                 typeof GPURenderPipeline === 'function' &&
                 pipeline instanceof GPURenderPipeline,
               derivedLayout,
               label: pipeline?.label,
             };
           })()`
          )
        )
      : null;

    // Read the device error queue a moment after the replay, for group Q's reason:
    // `createRenderPipeline` reports a bad shader/layout or an unexpressible field
    // through `uncapturederror` a task later, so a queue read in the evaluation that
    // saw the replay finish is empty whatever happened.
    const graphicsPipelineReport = graphicsPipelineProbe
      ? await evaluate(
          page,
          `(async () => {
           await new Promise((settle) =>
             requestAnimationFrame(() => requestAnimationFrame(settle))
           );
           return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
         })()`
        )
      : null;
    check(
      'R',
      'wasm encoded a graphics pipeline creation and the demo loop replayed it',
      graphicsPipelineProbe?.commands?.join(',') ===
        'CreateShaderModule,CreatePipelineLayout,CreateGraphicsPipeline' &&
        Number.isInteger(graphicsPipelineProbe.handle),
      graphicsPipelineProbe
        ? `the loop replayed [${graphicsPipelineProbe.commands.join(', ')}] for graphics pipeline ${graphicsPipelineProbe.handle}` +
            `${graphicsPipelineProbe.failure ? ` — ${graphicsPipelineProbe.failure}` : ''}`
        : graphicsPipelineStart?.started
          ? `the demo loop replayed nothing in ${TIMEOUT_MS} ms`
          : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'R',
      'a real GPURenderPipeline came back from the device and answered getBindGroupLayout',
      graphicsPipelineProbe?.isRealPipeline === true &&
        graphicsPipelineProbe?.derivedLayout === true &&
        graphicsPipelineProbe?.label === PROBE_GRAPHICS_PIPELINE_LABEL &&
        graphicsPipelineReport?.waited === true &&
        graphicsPipelineReport?.error === null,
      graphicsPipelineReport?.waited !== true
        ? 'the page never got as far as reading the device error queue'
        : graphicsPipelineProbe?.isRealPipeline &&
            graphicsPipelineProbe?.derivedLayout &&
            graphicsPipelineReport.error === null
          ? `an instance of this browser's GPURenderPipeline labelled ${JSON.stringify(graphicsPipelineProbe.label)}, its getBindGroupLayout(0) a real GPUBindGroupLayout, of ${graphicsPipelineProbe.held} held, and the device reported nothing`
          : `instanceof GPURenderPipeline: ${graphicsPipelineProbe?.isRealPipeline}, getBindGroupLayout(0) real: ${graphicsPipelineProbe?.derivedLayout}, label ${JSON.stringify(graphicsPipelineProbe?.label)}` +
            `${graphicsPipelineReport?.error ? ` — ${graphicsPipelineReport.error}` : ''}`
    );

    // **THE DECISIVE GATE OF THE WHOLE TRACK, AND THE FIRST THAT READS PIXELS.**
    // Everything above builds objects — a buffer, a texture, a pipeline — and
    // proves the browser accepted the descriptor this seam encoded. None of them
    // proves the backend puts the *right pixels* in memory, because none of them
    // renders and reads back. This does: wasm records a clear of a 64×64 texture to
    // a colour exact in 8 bits, a copy of it into a host-readable buffer, a submit
    // and a readback request; the demo's own loop replays it; the browser's
    // `mapAsync` resolves a few frames later; and the bytes that come back on the
    // reply channel are asserted to be that colour, every pixel.
    //
    // `gpu-replay.mjs` cannot reach here: its stub buffer hands back whatever bytes
    // it likes, so a readback against it proves the bookkeeping and nothing about
    // the pixels. Only a real device clears a real texture, and only a real
    // `copyTextureToBuffer` and `mapAsync` carry the result back — which is why
    // this check is the one that a stub silently substituted for the backend fails.
    group('S — cleared pixels are read back from memory as the clear colour');

    const PROBE_READBACK_PIXELS = 64 * 64;
    const PROBE_READBACK_CLEAR_BYTES = [64, 128, 191, 255];

    const readbackStart = await evaluate(
      page,
      `(async () => {
       const { startReadbackProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       const before = gpu.stats();
       return {
         started: startReadbackProbe({ exports }),
         replayed: before.replayed,
       };
     })()`
    );
    // Poll across frames the way group G/H wait for the device: the demo's rAF loop
    // replays the poll and delivers its reply between these evaluations, and each
    // evaluation drains what has arrived, then queues another poll while the map is
    // still resolving. `readReadbackProbe` drains first, so a reply delivered since
    // the last frame is absorbed before another poll is queued — and a second poll
    // is never queued while one is unanswered, which `poll_readback` enforces.
    const readback = readbackStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(async () => {
             const { readReadbackProbe, pollReadbackProbe, READBACK } =
               await import('/engine/gpu-probe.js');
             const { exports, gpu } = globalThis.crcbl;
             const r = readReadbackProbe({ exports, memory: exports.memory });
             if (r.state === READBACK.UNDECODABLE) {
               return { done: true, state: r.name, error: gpu.replayer.takeError() };
             }
             if (r.state !== READBACK.READY) {
               // Not yet — queue another poll for the loop to replay, and wait.
               pollReadbackProbe({ exports });
               return null;
             }
             // Ready: check every pixel here rather than shipping 16384 numbers
             // out over the protocol. \`want\` is the clear colour in 8-bit-exact
             // channels — 0.25/0.5/0.75/1.0 → 64/128/191/255.
             const want = ${JSON.stringify(PROBE_READBACK_CLEAR_BYTES)};
             let allMatch = r.bytes.length === ${PROBE_READBACK_PIXELS} * 4;
             let firstWrong = -1;
             for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
               if (r.bytes[i] !== want[i % 4]) {
                 allMatch = false;
                 firstWrong = i;
               }
             }
             return {
               done: true,
               state: r.name,
               len: r.bytes.length,
               allMatch,
               firstWrong,
               // The first two texels, for a failure message a human can read.
               sample: [...r.bytes.slice(0, 8)],
               error: gpu.replayer.takeError(),
             };
           })()`
          )
        )
      : null;
    check(
      'S',
      'wasm encoded the readback setup frame — clear, copy, submit, request',
      readbackStart?.started === true,
      readbackStart?.started
        ? 'the clear-and-read frame is on the stream'
        : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'S',
      'the cleared pixels came back from memory as the clear colour, every one',
      readback?.done === true &&
        readback.allMatch === true &&
        readback.len === PROBE_READBACK_PIXELS * 4,
      readback?.done !== true
        ? `no readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
        : readback.allMatch
          ? `${readback.len} bytes, every texel [${PROBE_READBACK_CLEAR_BYTES.join(', ')}]`
          : `state ${readback.state}, ${readback.len ?? 0} bytes, ` +
            `first wrong at byte ${readback.firstWrong} (sample ${JSON.stringify(readback.sample)} ` +
            `against [${PROBE_READBACK_CLEAR_BYTES.join(', ')}])` +
            `${readback.error ? ` — ${readback.error}` : ''}`
    );

    // **THE DRAW GATE, AND THE FIRST THAT PROVES A DRAW.** Group S proves a clear
    // reaches host memory. This proves a `setPipeline` + `draw` *overwrites* that
    // clear: wasm records a colour-only pipeline, a pass that clears the 64×64
    // texture to the blue clear and then binds the pipeline and draws a fullscreen
    // triangle whose fragment shader writes constant red, a copy into a host
    // buffer, a submit and a readback; the demo's loop replays it; and the bytes
    // that come back are asserted to be the red draw colour, every pixel — not the
    // blue clear underneath. A stub that skips the draw leaves blue and fails here;
    // only a real draw leaves red. `gpu-replay.mjs` cannot reach this for group S's
    // reason: only a real device rasterises a triangle into a real texture.
    group(
      'T — a drawn triangle is read back as the draw colour, not the clear'
    );

    const PROBE_DRAW_PIXELS = 64 * 64;
    const PROBE_DRAW_COLOR_BYTES = [255, 0, 0, 255];
    const PROBE_DRAW_CLEAR_BYTES = [64, 128, 191, 255];

    const drawStart = await evaluate(
      page,
      `(async () => {
       const { startDrawProbe } = await import('/engine/gpu-probe.js');
       const { exports } = globalThis.crcbl;
       return { started: startDrawProbe({ exports }) };
     })()`
    );
    // Poll across frames exactly as group S does: the demo's rAF loop replays the
    // poll and delivers its reply between these evaluations, and each evaluation
    // drains what has arrived, then queues another poll while the map is still
    // resolving. `readDrawProbe` drains first, so a reply delivered since the last
    // frame is absorbed before another poll is queued.
    const draw = drawStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(async () => {
             const { readDrawProbe, pollDrawProbe, DRAW } =
               await import('/engine/gpu-probe.js');
             const { exports, gpu } = globalThis.crcbl;
             const r = readDrawProbe({ exports, memory: exports.memory });
             if (r.state === DRAW.UNDECODABLE) {
               return { done: true, state: r.name, error: gpu.replayer.takeError() };
             }
             if (r.state !== DRAW.READY) {
               // Not yet — queue another poll for the loop to replay, and wait.
               pollDrawProbe({ exports });
               return null;
             }
             // Ready: check every pixel here rather than shipping 16384 numbers
             // out. \`want\` is the fragment's constant red; the clear beneath it
             // was blue, so a texel still blue is a draw that did not happen.
             const want = ${JSON.stringify(PROBE_DRAW_COLOR_BYTES)};
             let allMatch = r.bytes.length === ${PROBE_DRAW_PIXELS} * 4;
             let firstWrong = -1;
             for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
               if (r.bytes[i] !== want[i % 4]) {
                 allMatch = false;
                 firstWrong = i;
               }
             }
             return {
               done: true,
               state: r.name,
               len: r.bytes.length,
               allMatch,
               firstWrong,
               // The first two texels, for a failure message a human can read.
               sample: [...r.bytes.slice(0, 8)],
               error: gpu.replayer.takeError(),
             };
           })()`
          )
        )
      : null;
    check(
      'T',
      'wasm encoded the draw setup frame — pipeline, clear, bind, draw, copy, request',
      drawStart?.started === true,
      drawStart?.started
        ? 'the draw-and-read frame is on the stream'
        : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'T',
      'the drawn pixels came back as the draw colour, not the clear, every one',
      draw?.done === true &&
        draw.allMatch === true &&
        draw.len === PROBE_DRAW_PIXELS * 4,
      draw?.done !== true
        ? `no draw readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
        : draw.allMatch
          ? `${draw.len} bytes, every texel [${PROBE_DRAW_COLOR_BYTES.join(', ')}] (red), not the clear [${PROBE_DRAW_CLEAR_BYTES.join(', ')}] (blue)`
          : `state ${draw.state}, ${draw.len ?? 0} bytes, ` +
            `first wrong at byte ${draw.firstWrong} (sample ${JSON.stringify(draw.sample)} ` +
            `against [${PROBE_DRAW_COLOR_BYTES.join(', ')}]; the clear was [${PROBE_DRAW_CLEAR_BYTES.join(', ')}])` +
            `${draw.error ? ` — ${draw.error}` : ''}`
    );

    // **THE DISPATCH GATE, AND THE FIRST THAT PROVES A COMPUTE SHADER RAN.** Group T
    // proves a draw overwrites a clear. This proves a `dispatchWorkgroups` writes a
    // storage buffer: wasm records a compute pipeline whose shader sets every slot
    // of a 64-`u32` storage buffer to 0xDEADBEEF, a pass that binds and dispatches
    // it, a buffer→buffer copy into a host buffer, a submit and a readback; the
    // demo's loop replays it; and the 256 bytes that come back are asserted to be
    // 0xDEADBEEF in every 4-byte little-endian word. A fresh WebGPU buffer is
    // zero-initialised, so a stub that skips the dispatch reads back all zeros —
    // only a dispatch that actually ran writes the pattern. `gpu-replay.mjs` cannot
    // reach this: only a real device runs a compute shader into a real buffer.
    group('U — a compute shader’s storage-buffer writes are read back');

    const PROBE_DISPATCH_WORDS = 64;
    // 0xDEADBEEF as the little-endian bytes a `u32` holds it as.
    const PROBE_DISPATCH_PATTERN_BYTES = [0xef, 0xbe, 0xad, 0xde];

    const computeStart = await evaluate(
      page,
      `(async () => {
       const { startComputeProbe } = await import('/engine/gpu-probe.js');
       const { exports } = globalThis.crcbl;
       return { started: startComputeProbe({ exports }) };
     })()`
    );
    // Poll across frames exactly as group T does: the demo's rAF loop replays the
    // poll and delivers its reply between these evaluations, and each evaluation
    // drains what has arrived, then queues another poll while the map is still
    // resolving. `readComputeProbe` drains first, so a reply delivered since the
    // last frame is absorbed before another poll is queued.
    const compute = computeStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(async () => {
             const { readComputeProbe, pollComputeProbe, COMPUTE } =
               await import('/engine/gpu-probe.js');
             const { exports, gpu } = globalThis.crcbl;
             const r = readComputeProbe({ exports, memory: exports.memory });
             if (r.state === COMPUTE.UNDECODABLE) {
               return { done: true, state: r.name, error: gpu.replayer.takeError() };
             }
             if (r.state !== COMPUTE.READY) {
               // Not yet — queue another poll for the loop to replay, and wait.
               pollComputeProbe({ exports });
               return null;
             }
             // Ready: check every word here rather than shipping 64 numbers out.
             // \`want\` is 0xDEADBEEF little-endian; a buffer left at zero is a
             // dispatch that did not happen.
             const want = ${JSON.stringify(PROBE_DISPATCH_PATTERN_BYTES)};
             let allMatch = r.bytes.length === ${PROBE_DISPATCH_WORDS} * 4;
             let firstWrong = -1;
             for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
               if (r.bytes[i] !== want[i % 4]) {
                 allMatch = false;
                 firstWrong = i;
               }
             }
             return {
               done: true,
               state: r.name,
               len: r.bytes.length,
               allMatch,
               firstWrong,
               // The first two words, for a failure message a human can read.
               sample: [...r.bytes.slice(0, 8)],
               error: gpu.replayer.takeError(),
             };
           })()`
          )
        )
      : null;
    check(
      'U',
      'wasm encoded the dispatch setup frame — pipeline, pass, bind, dispatch, copy, request',
      computeStart?.started === true,
      computeStart?.started
        ? 'the dispatch-and-read frame is on the stream'
        : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'U',
      'the dispatched storage buffer came back as 0xDEADBEEF, every word',
      compute?.done === true &&
        compute.allMatch === true &&
        compute.len === PROBE_DISPATCH_WORDS * 4,
      compute?.done !== true
        ? `no dispatch readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
        : compute.allMatch
          ? `${compute.len} bytes, every word 0xDEADBEEF [${PROBE_DISPATCH_PATTERN_BYTES.join(', ')}], not the zero-init a stub reads back`
          : `state ${compute.state}, ${compute.len ?? 0} bytes, ` +
            `first wrong at byte ${compute.firstWrong} (sample ${JSON.stringify(compute.sample)} ` +
            `against [${PROBE_DISPATCH_PATTERN_BYTES.join(', ')}])` +
            `${compute.error ? ` — ${compute.error}` : ''}`
    );

    // **THE COPY-CHAIN GATE, AND THE FIRST THAT PROVES A BUFFER→IMAGE AND AN
    // IMAGE→IMAGE COPY RAN.** wasm records a dispatch that fills a storage buffer
    // with red (`0xFF0000FF` per texel), a `pipeline_barrier` transitioning that
    // buffer from `ShaderWrite` to `TransferSrc`, a `copyBufferToTexture` into a
    // 64×64 texture, a `copyTextureToTexture` to a second texture, a
    // `copyTextureToBuffer` out to a host buffer, a submit and a readback; the
    // demo's loop replays it; and the 16384 bytes that come back are asserted red
    // in every texel. A fresh WebGPU texture is zero-initialised, so a stub that
    // skips EITHER copy reads back zeros — only both copies running carries the red
    // all the way out. The `pipeline_barrier` is the documented no-op: it sits
    // mid-frame between the dispatch and the first copy, and the red readback ALSO
    // proves it did not disturb replay. `gpu-replay.mjs` cannot reach the copies:
    // only a real device runs them.
    group('V — a buffer→image→image→buffer copy chain is read back red');

    const PROBE_COPYCHAIN_TEXELS = 64 * 64;
    // Opaque red as the little-endian bytes an `rgba8unorm` texel holds it as.
    const PROBE_COPYCHAIN_PATTERN_BYTES = [255, 0, 0, 255];

    const copyChainStart = await evaluate(
      page,
      `(async () => {
       const { startCopyChainProbe } = await import('/engine/gpu-probe.js');
       const { exports } = globalThis.crcbl;
       return { started: startCopyChainProbe({ exports }) };
     })()`
    );
    const copyChain = copyChainStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(async () => {
             const { readCopyChainProbe, pollCopyChainProbe, COPYCHAIN } =
               await import('/engine/gpu-probe.js');
             const { exports, gpu } = globalThis.crcbl;
             const r = readCopyChainProbe({ exports, memory: exports.memory });
             if (r.state === COPYCHAIN.UNDECODABLE) {
               return { done: true, state: r.name, error: gpu.replayer.takeError() };
             }
             if (r.state !== COPYCHAIN.READY) {
               pollCopyChainProbe({ exports });
               return null;
             }
             // Ready: check every texel here rather than shipping 16384 numbers
             // out. \`want\` is red; a texel left at zero is a copy that did not
             // happen.
             const want = ${JSON.stringify(PROBE_COPYCHAIN_PATTERN_BYTES)};
             let allMatch = r.bytes.length === ${PROBE_COPYCHAIN_TEXELS} * 4;
             let firstWrong = -1;
             for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
               if (r.bytes[i] !== want[i % 4]) {
                 allMatch = false;
                 firstWrong = i;
               }
             }
             return {
               done: true,
               state: r.name,
               len: r.bytes.length,
               allMatch,
               firstWrong,
               sample: [...r.bytes.slice(0, 8)],
               error: gpu.replayer.takeError(),
             };
           })()`
          )
        )
      : null;
    check(
      'V',
      'wasm encoded the copy-chain setup frame — dispatch, pipeline_barrier, buffer→image, image→image, image→buffer, request',
      copyChainStart?.started === true,
      copyChainStart?.started
        ? 'the copy-chain frame is on the stream'
        : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'V',
      'the copy chain came back red in every texel — both new copies ran',
      copyChain?.done === true &&
        copyChain.allMatch === true &&
        copyChain.len === PROBE_COPYCHAIN_TEXELS * 4,
      copyChain?.done !== true
        ? `no copy-chain readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
        : copyChain.allMatch
          ? `${copyChain.len} bytes, every texel [${PROBE_COPYCHAIN_PATTERN_BYTES.join(', ')}] (red), not the zero-init a stub reads back`
          : `state ${copyChain.state}, ${copyChain.len ?? 0} bytes, ` +
            `first wrong at byte ${copyChain.firstWrong} (sample ${JSON.stringify(copyChain.sample)} ` +
            `against [${PROBE_COPYCHAIN_PATTERN_BYTES.join(', ')}])` +
            `${copyChain.error ? ` — ${copyChain.error}` : ''}`
    );
    // The same readback, read for what it says about the no-op: the frame carries a
    // `pipeline_barrier` between the dispatch and the first copy, and a red result
    // means replay recognised it, recorded nothing, and carried on — a barrier that
    // threw or corrupted the encoder would leave this red-everywhere check failing
    // above. That it holds is the browser-frame evidence the barrier is inert.
    check(
      'V',
      'the pipeline_barrier mid-frame did not disturb replay — the barriered frame is still red',
      copyChain?.done === true &&
        copyChain.allMatch === true &&
        copyChain.len === PROBE_COPYCHAIN_TEXELS * 4,
      copyChain?.allMatch === true
        ? 'the frame with a pipeline_barrier in it read back red in every texel — the no-op is inert'
        : 'the barriered frame did not come back red — see the copy-chain check above for the bytes'
    );

    // **THE FILL GATE, AND THE FIRST THAT PROVES `clearBuffer` ZEROES EXACTLY ITS
    // SUB-RANGE.** wasm records a dispatch that fills a 256-byte storage buffer with
    // `0xDEADBEEF`, a `fill_buffer(offset 0, size 128, value 0)` that maps to
    // `clearBuffer` over the first half, a copy to a host buffer, a submit and a
    // readback; the demo's loop replays it; and the 256 bytes that come back are
    // asserted zero in bytes 0..128 and still `0xDEADBEEF` in bytes 128..256. A stub
    // `clearBuffer` leaves the pattern in the half that should be zero; a fill that
    // ran too far zeroes the pattern beyond its size. `gpu-replay.mjs` cannot reach
    // this: only a real device runs a compute shader and a clear into a real buffer.
    group('W — clearBuffer zeroes its sub-range and leaves the rest');

    const PROBE_FILL_WORDS = 64;
    const PROBE_FILL_ZEROED_BYTES = 128;
    // 0xDEADBEEF as the little-endian bytes a `u32` holds it as.
    const PROBE_FILL_PATTERN_BYTES = [0xef, 0xbe, 0xad, 0xde];

    const fillStart = await evaluate(
      page,
      `(async () => {
       const { startFillProbe } = await import('/engine/gpu-probe.js');
       const { exports } = globalThis.crcbl;
       return { started: startFillProbe({ exports }) };
     })()`
    );
    const fill = fillStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(async () => {
             const { readFillProbe, pollFillProbe, FILL } =
               await import('/engine/gpu-probe.js');
             const { exports, gpu } = globalThis.crcbl;
             const r = readFillProbe({ exports, memory: exports.memory });
             if (r.state === FILL.UNDECODABLE) {
               return { done: true, state: r.name, error: gpu.replayer.takeError() };
             }
             if (r.state !== FILL.READY) {
               pollFillProbe({ exports });
               return null;
             }
             // Ready: the first half must be zero (the fill ran) and the second
             // half still the pattern (the fill stopped at its size).
             const pattern = ${JSON.stringify(PROBE_FILL_PATTERN_BYTES)};
             const zeroed = ${PROBE_FILL_ZEROED_BYTES};
             let lenOk = r.bytes.length === ${PROBE_FILL_WORDS} * 4;
             let firstHalfZero = lenOk;
             let firstWrong = -1;
             for (let i = 0; i < zeroed && firstHalfZero; i += 1) {
               if (r.bytes[i] !== 0) {
                 firstHalfZero = false;
                 firstWrong = i;
               }
             }
             let secondHalfPattern = lenOk;
             for (let i = zeroed; i < r.bytes.length && secondHalfPattern; i += 1) {
               if (r.bytes[i] !== pattern[i % 4]) {
                 secondHalfPattern = false;
                 firstWrong = i;
               }
             }
             return {
               done: true,
               state: r.name,
               len: r.bytes.length,
               firstHalfZero,
               secondHalfPattern,
               firstWrong,
               // A byte from each half, for a failure message a human can read.
               sample: [...r.bytes.slice(124, 132)],
               error: gpu.replayer.takeError(),
             };
           })()`
          )
        )
      : null;
    check(
      'W',
      'wasm encoded the fill setup frame — dispatch, fill_buffer, copy, request',
      fillStart?.started === true,
      fillStart?.started
        ? 'the fill frame is on the stream'
        : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'W',
      'the fill zeroed the first half and left the second half as 0xDEADBEEF',
      fill?.done === true &&
        fill.firstHalfZero === true &&
        fill.secondHalfPattern === true &&
        fill.len === PROBE_FILL_WORDS * 4,
      fill?.done !== true
        ? `no fill readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
        : fill.firstHalfZero && fill.secondHalfPattern
          ? `${fill.len} bytes, first ${PROBE_FILL_ZEROED_BYTES} zero and the rest 0xDEADBEEF [${PROBE_FILL_PATTERN_BYTES.join(', ')}]`
          : `state ${fill.state}, ${fill.len ?? 0} bytes, ` +
            `first wrong at byte ${fill.firstWrong} (sample around the boundary ${JSON.stringify(fill.sample)}; ` +
            `want zero then [${PROBE_FILL_PATTERN_BYTES.join(', ')}])` +
            `${fill.error ? ` — ${fill.error}` : ''}`
    );

    // **THE PRESENT GATE, AND THE FIRST THAT PROVES THE REAL CANVAS-CONTEXT PATH.**
    // Every gate above rendered into a texture this replayer created; this one
    // renders into the frame a *canvas* handed back. wasm records a surface on the
    // a dedicated OffscreenCanvas the probe owns (see below), a swapchain
    // configured on it (a `configure` with COPY_SRC in the usage), an acquire
    // (`getCurrentTexture`), a pass that clears the acquired view to red, a copy of
    // that frame into a host buffer, a submit, a present (a no-op the browser
    // composites on rAF), and a readback; the demo's loop replays it; and the 16384
    // bytes that come back are asserted to be red, every pixel. A stub that skipped
    // the configure/acquire/render leaves a black/zero canvas and fails here; only
    // the real path — `context.configure`, `getCurrentTexture`, render,
    // `copyTextureToBuffer` off the acquired texture — leaves red. `gpu-replay.mjs`
    // cannot reach this: only a real browser has a canvas context.
    group('X — a presented canvas frame is read back as the render colour');

    const PROBE_PRESENT_PIXELS = 64 * 64;
    const PROBE_PRESENT_COLOR_BYTES = [255, 0, 0, 255];

    const presentStart = await evaluate(
      page,
      `(async () => {
       const { startPresentProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       // The present probe needs a canvas IT owns. Configuring the demo's own
       // canvas with the probe's device collides with the running demo's device
       // over the one GPUCanvasContext — two devices cannot configure one canvas,
       // and it also disagrees on the canvas's preferred format. So register a
       // dedicated 64x64 OffscreenCanvas in the very registry the replayer reads
       // (gpu.canvases is that Map) and point the probe at it: its device
       // configures a context nothing else touches. The demo keeps canvas 1.
       const PRESENT_CANVAS_ID = 0x50;
       if (!gpu.canvases.has(PRESENT_CANVAS_ID)) {
         gpu.canvases.set(PRESENT_CANVAS_ID, new OffscreenCanvas(64, 64));
       }
       return {
         started: startPresentProbe({ exports, canvasId: PRESENT_CANVAS_ID }),
       };
     })()`
    );
    // Poll across frames exactly as the draw gate does: the demo's rAF loop replays
    // the poll and delivers its reply between these evaluations. `readPresentProbe`
    // drains first, so a reply delivered since the last frame is absorbed before
    // another poll is queued.
    const present = presentStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(async () => {
             const { readPresentProbe, pollPresentProbe, PRESENT } =
               await import('/engine/gpu-probe.js');
             const { exports, gpu } = globalThis.crcbl;
             const r = readPresentProbe({ exports, memory: exports.memory });
             if (r.state === PRESENT.UNDECODABLE) {
               return { done: true, state: r.name, error: gpu.replayer.takeError() };
             }
             if (r.state !== PRESENT.READY) {
               // Not yet — queue another poll for the loop to replay, and wait.
               pollPresentProbe({ exports });
               return null;
             }
             // Ready: check every pixel here rather than shipping 16384 numbers
             // out. \`want\` is the red the pass cleared the acquired frame to;
             // a black/zero texel is a canvas path that did not run.
             const want = ${JSON.stringify(PROBE_PRESENT_COLOR_BYTES)};
             let allMatch = r.bytes.length === ${PROBE_PRESENT_PIXELS} * 4;
             let firstWrong = -1;
             for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
               if (r.bytes[i] !== want[i % 4]) {
                 allMatch = false;
                 firstWrong = i;
               }
             }
             return {
               done: true,
               state: r.name,
               len: r.bytes.length,
               allMatch,
               firstWrong,
               // The first two texels, for a failure message a human can read.
               sample: [...r.bytes.slice(0, 8)],
               error: gpu.replayer.takeError(),
             };
           })()`
          )
        )
      : null;
    check(
      'X',
      'wasm encoded the present setup frame — surface, swapchain, acquire, clear, copy, present, request',
      presentStart?.started === true,
      presentStart?.started
        ? 'the present-and-read frame is on the stream'
        : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'X',
      'the presented canvas frame came back from memory as the render colour, every pixel',
      present?.done === true &&
        present.allMatch === true &&
        present.len === PROBE_PRESENT_PIXELS * 4,
      present?.done !== true
        ? `no present readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
        : present.allMatch
          ? `${present.len} bytes, every texel [${PROBE_PRESENT_COLOR_BYTES.join(', ')}] (red) from the acquired canvas frame`
          : `state ${present.state}, ${present.len ?? 0} bytes, ` +
            `first wrong at byte ${present.firstWrong} (sample ${JSON.stringify(present.sample)} ` +
            `against [${PROBE_PRESENT_COLOR_BYTES.join(', ')}])` +
            `${present.error ? ` — ${present.error}` : ''}`
    );

    // **THE RECONFIGURE GATE.** Group X proved a presented canvas frame reads back
    // as the render colour; this one proves a swapchain reconfigured in place takes
    // the NEW format. wasm creates the swapchain `Rgba8Unorm`, reconfigures it
    // `Bgra8Unorm`, then acquires, clears red, copies out and reads back on its own
    // dedicated OffscreenCanvas. Red in an `Rgba8Unorm` frame reads back as
    // [255, 0, 0, 255]; red in a `Bgra8Unorm` frame reads back in BGRA byte order as
    // [0, 0, 255, 255]. Asserting the latter is the proof the reconfigure actually
    // re-ran `context.configure` with the new format — a stub that skipped it leaves
    // the swapchain `Rgba8Unorm` and fails here. `gpu-replay.mjs` cannot reach this:
    // only a real browser has a canvas context.
    group('Y — a reconfigured swapchain presents in the new format');

    const PROBE_RECONFIG_PIXELS = 64 * 64;
    const PROBE_RECONFIG_COLOR_BYTES = [0, 0, 255, 255];

    const reconfigStart = await evaluate(
      page,
      `(async () => {
       const { startReconfigureProbe } = await import('/engine/gpu-probe.js');
       const { exports, gpu } = globalThis.crcbl;
       // Its own canvas, for group X's reason: configuring a canvas the demo or
       // the present probe already drives collides over the one GPUCanvasContext.
       // A fresh 64x64 OffscreenCanvas under a fresh id, distinct from the present
       // probe's 0x50, so both gates can run in the one page.
       const RECONFIG_CANVAS_ID = 0x51;
       if (!gpu.canvases.has(RECONFIG_CANVAS_ID)) {
         gpu.canvases.set(RECONFIG_CANVAS_ID, new OffscreenCanvas(64, 64));
       }
       return {
         started: startReconfigureProbe({ exports, canvasId: RECONFIG_CANVAS_ID }),
       };
     })()`
    );
    // Poll across frames exactly as the present gate does: the demo's rAF loop
    // replays the poll and delivers its reply between these evaluations.
    const reconfig = reconfigStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(async () => {
             const { readReconfigureProbe, pollReconfigureProbe, RECONFIG } =
               await import('/engine/gpu-probe.js');
             const { exports, gpu } = globalThis.crcbl;
             const r = readReconfigureProbe({ exports, memory: exports.memory });
             if (r.state === RECONFIG.UNDECODABLE) {
               return { done: true, state: r.name, error: gpu.replayer.takeError() };
             }
             if (r.state !== RECONFIG.READY) {
               // Not yet — queue another poll for the loop to replay, and wait.
               pollReconfigureProbe({ exports });
               return null;
             }
             // Ready: check every pixel here rather than shipping 16384 numbers
             // out. \`want\` is the BGRA red the reconfigured (bgra8unorm) frame
             // holds; an rgba8unorm frame that skipped the reconfigure reads back
             // [255, 0, 0, 255] and fails.
             const want = ${JSON.stringify(PROBE_RECONFIG_COLOR_BYTES)};
             let allMatch = r.bytes.length === ${PROBE_RECONFIG_PIXELS} * 4;
             let firstWrong = -1;
             for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
               if (r.bytes[i] !== want[i % 4]) {
                 allMatch = false;
                 firstWrong = i;
               }
             }
             return {
               done: true,
               state: r.name,
               len: r.bytes.length,
               allMatch,
               firstWrong,
               // The first two texels, for a failure message a human can read.
               sample: [...r.bytes.slice(0, 8)],
               error: gpu.replayer.takeError(),
             };
           })()`
          )
        )
      : null;
    check(
      'Y',
      'wasm encoded the reconfigure setup frame — surface, swapchain, reconfigure, acquire, clear, copy, present, request',
      reconfigStart?.started === true,
      reconfigStart?.started
        ? 'the reconfigure-and-read frame is on the stream'
        : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'Y',
      'the reconfigured canvas frame came back in the new format, every pixel',
      reconfig?.done === true &&
        reconfig.allMatch === true &&
        reconfig.len === PROBE_RECONFIG_PIXELS * 4,
      reconfig?.done !== true
        ? `no reconfigure readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
        : reconfig.allMatch
          ? `${reconfig.len} bytes, every texel [${PROBE_RECONFIG_COLOR_BYTES.join(', ')}] (bgra red) from the reconfigured canvas frame`
          : `state ${reconfig.state}, ${reconfig.len ?? 0} bytes, ` +
            `first wrong at byte ${reconfig.firstWrong} (sample ${JSON.stringify(reconfig.sample)} ` +
            `against [${PROBE_RECONFIG_COLOR_BYTES.join(', ')}])` +
            `${reconfig.error ? ` — ${reconfig.error}` : ''}`
    );

    // **THE INDIRECT-DRAW GATE, AND THE FIRST THAT PROVES AN INDIRECT DRAW RAN.**
    // Group T proved a direct `draw` overwrites a clear. This proves a
    // `drawIndexedIndirect` — the live 3D-forward geometry path — puts exactly the
    // same pixels there: wasm records the same fullscreen-triangle pipeline, fills
    // an indirect-args buffer with `[3,1,0,0,0]` (a 3-index single draw) and an
    // index buffer with `[0,1,2,0]` via `write_buffer`, clears the texture blue,
    // binds the pipeline and index buffer, and records `drawIndexedIndirect`
    // reading its counts from the buffer; the demo's loop replays it; and the
    // bytes that come back are the red draw colour, every pixel — not the blue
    // clear. A stub that skips the draw leaves blue; only an indirect draw that
    // actually rasterised leaves red. `gpu-replay.mjs` cannot reach this for group
    // S's reason: only a real device reads args from a buffer and rasterises.
    group(
      'Z — an indirect-drawn triangle is read back as the draw colour, not the clear'
    );

    const PROBE_INDIRECT_PIXELS = 64 * 64;
    const PROBE_INDIRECT_COLOR_BYTES = [255, 0, 0, 255];
    const PROBE_INDIRECT_CLEAR_BYTES = [64, 128, 191, 255];

    const indirectStart = await evaluate(
      page,
      `(async () => {
       const { startIndirectProbe } = await import('/engine/gpu-probe.js');
       const { exports } = globalThis.crcbl;
       return { started: startIndirectProbe({ exports }) };
     })()`
    );
    // Poll across frames exactly as group T does: the demo's rAF loop replays the
    // poll and delivers its reply between these evaluations, and each evaluation
    // drains what has arrived, then queues another poll while the map is still
    // resolving. `readIndirectProbe` drains first, so a reply delivered since the
    // last frame is absorbed before another poll is queued.
    const indirect = indirectStart?.started
      ? await until(async () =>
          evaluate(
            page,
            `(async () => {
             const { readIndirectProbe, pollIndirectProbe, INDIRECT } =
               await import('/engine/gpu-probe.js');
             const { exports, gpu } = globalThis.crcbl;
             const r = readIndirectProbe({ exports, memory: exports.memory });
             if (r.state === INDIRECT.UNDECODABLE) {
               return { done: true, state: r.name, error: gpu.replayer.takeError() };
             }
             if (r.state !== INDIRECT.READY) {
               // Not yet — queue another poll for the loop to replay, and wait.
               pollIndirectProbe({ exports });
               return null;
             }
             // Ready: check every pixel here rather than shipping 16384 numbers
             // out. \`want\` is the fragment's constant red; the clear beneath it
             // was blue, so a texel still blue is an indirect draw that did not
             // happen.
             const want = ${JSON.stringify(PROBE_INDIRECT_COLOR_BYTES)};
             let allMatch = r.bytes.length === ${PROBE_INDIRECT_PIXELS} * 4;
             let firstWrong = -1;
             for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
               if (r.bytes[i] !== want[i % 4]) {
                 allMatch = false;
                 firstWrong = i;
               }
             }
             return {
               done: true,
               state: r.name,
               len: r.bytes.length,
               allMatch,
               firstWrong,
               // The first two texels, for a failure message a human can read.
               sample: [...r.bytes.slice(0, 8)],
               error: gpu.replayer.takeError(),
             };
           })()`
          )
        )
      : null;
    check(
      'Z',
      'wasm encoded the indirect setup frame — pipeline, args and index writes, clear, bind, indirect draw, copy, request',
      indirectStart?.started === true,
      indirectStart?.started
        ? 'the indirect-draw-and-read frame is on the stream'
        : 'wasm would not encode it — no device has opened, or another channel is installed'
    );
    check(
      'Z',
      'the indirect-drawn pixels came back as the draw colour, not the clear, every one',
      indirect?.done === true &&
        indirect.allMatch === true &&
        indirect.len === PROBE_INDIRECT_PIXELS * 4,
      indirect?.done !== true
        ? `no indirect readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
        : indirect.allMatch
          ? `${indirect.len} bytes, every texel [${PROBE_INDIRECT_COLOR_BYTES.join(', ')}] (red), not the clear [${PROBE_INDIRECT_CLEAR_BYTES.join(', ')}] (blue)`
          : `state ${indirect.state}, ${indirect.len ?? 0} bytes, ` +
            `first wrong at byte ${indirect.firstWrong} (sample ${JSON.stringify(indirect.sample)} ` +
            `against [${PROBE_INDIRECT_COLOR_BYTES.join(', ')}]; the clear was [${PROBE_INDIRECT_CLEAR_BYTES.join(', ')}])` +
            `${indirect.error ? ` — ${indirect.error}` : ''}`
    );
  } // end of the crcbl-webgpu probe groups, skipped in webgpu mode

  // Written whatever the outcome: a black PNG is the evidence for a failure and
  // the first thing a human will ask for. The canvas itself rather than a
  // viewport screenshot — the page's chrome is not what is under test.
  const png = await evaluate(
    page,
    `document.getElementById('canvas').toDataURL().slice(22)`
  );
  const shotPath = join(OUT, `${SLUG}-${chosen.mode}.png`);
  writeFileSync(shotPath, Buffer.from(png, 'base64'));
  writeFileSync(
    join(OUT, `${SLUG}-${chosen.mode}.log`),
    consoleLines.join('\n')
  );
  console.log(`\nweb e2e: canvas  ${shotPath}`);
  console.log(`web e2e: page log ${join(OUT, `${SLUG}-${chosen.mode}.log`)}`);

  page.close();
  exitCode = 0;
} catch (error) {
  console.error(`\nweb e2e: ${error.message}`);
  exitCode = 1;
} finally {
  browser?.stop();
  await site.close();
}

// ---------------------------------------------------------------------------
// The verdict
// ---------------------------------------------------------------------------

const failed = checks.filter((c) => !c.ok);

console.log('');
if (checks.length === 0) {
  // The trap `docs/plan/12-testing.md` names: a harness that checked nothing and
  // said so quietly is worse than no harness.
  console.error('web e2e: ZERO CHECKS RAN — the gate is not gating.');
  if (browser?.stderr.length)
    console.error(browser.stderr.slice(-40).join('\n'));
  process.exit(1);
}

console.log(
  `web e2e: ${checks.length - failed.length}/${checks.length} checks passed`
);

if (failed.length) {
  console.error('\nweb e2e: FAILED');
  for (const c of failed)
    console.error(`  ${c.group}: ${c.name}${c.detail ? ` — ${c.detail}` : ''}`);
  if (deviceErrors.length) {
    console.error('\nweb e2e: WebGPU device errors, in full:');
    for (const message of deviceErrors.slice(0, 4)) {
      console.error(
        message
          .split('\n')
          .map((line) => `    ${line}`)
          .join('\n')
      );
    }
    if (deviceErrors.length > 4)
      console.error(`    … and ${deviceErrors.length - 4} more`);
  }
  process.exit(1);
}

process.exit(exitCode);
