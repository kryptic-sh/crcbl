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
// WHAT IT ASSERTS. Seven groups, printed in order:
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
//   F  a finger — real touch contacts, which a dispatched mouse is not
//   G  the frame is sRGB-encoded — the demo's own flat clear colour, read off
//      the canvas as the browser composited it and compared against the byte an
//      sRGB target holds. Only for a demo whose clear reaches the screen
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
// GROUP G IS THE ONE EXCEPTION TO THAT, AND IT IS DELIBERATELY NARROW. It
// compares no rendered image; it compares the demo's own *clear colour*, which
// is a flat fill of one exact byte that every rasteriser produces identically
// and that therefore needs no tolerance table and no expected-fail list. What
// that buys is the one claim group D cannot make: the frame went out sRGB-
// encoded. A canvas configured without its `-srgb` viewFormat presents every
// value a transfer function too dark, and each of group D's checks passes on
// such a frame — which is how it reached a visitor. See the group for why a
// mid-range colour is the whole of the design.
//
// EXIT STATUS. Non-zero if any check fails *or* if zero checks ran. The second
// half is not decoration: `docs/plan/12-testing.md` names a silently-skipped
// e2e job as a known trap, and a harness whose browser never started would
// otherwise print nothing and succeed.

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  LAUNCH_TIMEOUT_MS,
  browserFlags,
  findBrowser,
  readDevToolsPort,
  stopBrowser,
} from './browser-launch.mjs';
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
 * Three rows use it and they are `null` for different reasons: hud takes no
 * input at all, while lumen and quarry take plenty but have no run to begin —
 * which is why this says "no start key" rather than "no input".
 *
 * `backdrop` is the other optional key, and it is what group G reads. It names
 * the demo's clear colour twice over: `encoded` is the byte an sRGB target holds
 * and `unencoded` is the byte the same clear leaves in a linear one, with
 * `share` the fraction of the canvas the flat fill has to cover for the sample
 * to count. `source` names the Rust constant the pair was computed from, so the
 * next reader can check the arithmetic rather than trust it.
 *
 * **A row without one is a demo whose clear colour is not a constant it owns**,
 * and there are four: asteroids ends its run under the start menu's full-screen
 * scrim, so `art::SPACE` is only ever seen dimmed; horde tiles grass sprites
 * over every pixel of `art::GROUND`; lumen has no `clear_color` pass at all —
 * it is a lit room, and the nearest thing to a flat region in its frame is under
 * a tenth of the canvas; and quarry has none either, and the flat region behind
 * its face is whatever `crcbl-render` clears a 3D frame to rather than a byte
 * this sample chose. None of the four can make this claim, and a row that
 * quietly skipped it would be the check passing for the wrong reason.
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
    // `apps/breakout/src/art.rs`'s `SURROUND`, the letterbox either side of the
    // play field. The smallest share any row here claims, because it is only the
    // margins — but it is a flat clear at an exact byte all the same, and the
    // two candidate colours are nine levels apart.
    backdrop: {
      source: 'breakout::art::SURROUND',
      encoded: [10, 10, 15],
      unencoded: [1, 1, 1],
      share: 0.15,
    },
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
    // `apps/flappy/src/art.rs`'s `SKY`, `[0.147, 0.420, 0.787]`, and the widest
    // separation any row here has: seventy levels in red between the encoded
    // colour and the linear one, on three quarters of the canvas. Nothing about
    // that colour is a fixed point of the transfer function in any channel.
    backdrop: {
      source: 'flappy::art::SKY',
      encoded: [107, 173, 229],
      unencoded: [37, 107, 201],
      share: 0.4,
    },
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
    // `apps/hud/src/page.rs`'s `BACKDROP`, `[0.05, 0.06, 0.09]`, and the row
    // this check was written against: hud is a scripted page rather than a game,
    // so nothing it draws ever covers the clear and nothing it does depends on
    // when the sample lands. The pre-fix canvas this gate saved into
    // `target/web-e2e/` held rgb(13,15,23) over the same share of the same
    // frame — the linear colour below, the shipped bug, on disk.
    backdrop: {
      source: 'hud::page::BACKDROP',
      encoded: [63, 69, 85],
      unencoded: [13, 15, 23],
      share: 0.6,
    },
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
    // **One of the two demos that draw mesh instances**, so one of the two whose
    // cull pass has anything to count — quarry is the other. Every demo builds a
    // `ForwardRenderer`, and the rest fill their frames with sprites and text,
    // which that pass does not see — so the stats readback correctly never
    // arrives for them. See group D.
    culls: true,
    key: null,
    waiting: (line) =>
      line.includes('[HUD] tick: 60') && line.includes('lighting: Rasterised'),
    moving: /lamp x: (-?[\d.]+)/,
    movingLabel: 'the lamp keeps orbiting under its own steam',
  },
  // **The third demo with no start key**, and the second that draws mesh
  // geometry. `apps/quarry` is the geometry acceptance fixture: there is no run
  // to begin and no state to leave, so there is nothing for a `Space` to do. It
  // takes input — the free camera flies on WASD — but reaching that camera is a
  // pause-menu row rather than a key, exactly as it is in lumen.
  //
  // `waiting` is this sample's own claim and it is the exit criterion the page
  // exists to satisfy: "web demo renders the scene on `IndirectPerBatch` … and
  // the summary line names the path it took". A browser has **no mesh stage and
  // no GPU-side draw count**, so `GeometryPath::IndirectPerBatch` is the arm the
  // selector resolves to by construction — the same shape of claim lumen's row
  // makes about `LightingPath::Rasterised` — and the first heartbeat naming it
  // is what separates a per-batch page from one that opened some other device or
  // fell over before the first tick. `Quarry::log_heartbeat` prints the
  // selector's own `Debug`, which is a deliberate coupling to
  // `crates/crcbl-hal/src/caps.rs`: a renamed variant fails here loudly.
  //
  // `moving` is the camera's own position down the face, and it is the strictest
  // value this sample has. The page opens on `CameraMode::Dolly`, whose
  // accumulator is stepped in `HostedGame::tick` and nowhere else — so a page
  // presenting frames without ticking, or stuck on its first tick, leaves it
  // standing still. A frame counter, a wall clock and the heartbeat's own
  // cadence cannot move it: the number is metres, read off the camera the frame
  // was actually drawn from. `app::DOLLY_SECONDS` is slow enough to be watchable
  // and still moves the eye three metres between consecutive heartbeats, which
  // is far above the two decimal places the line prints it to.
  quarry: {
    // The other demo whose cull pass has something to count — see lumen's row
    // and group D. This one is the sample that pass exists for.
    culls: true,
    key: null,
    waiting: (line) =>
      line.includes('[HUD] tick: 60') &&
      line.includes('geometry: IndirectPerBatch'),
    moving: /eye z: (-?[\d.]+)/,
    movingLabel: 'the dolly keeps running down the face under its own steam',
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

/** How many times the canvas is read back once the ball is in flight. */
const SAMPLE_COUNT = 16;

/**
 * How far a backdrop channel may drift from the byte `EXPECTATIONS` names.
 *
 * The same window `crates/crcbl-webgpu/src/probe.rs` allows its present probe,
 * and for the same reason: the hardware evaluates the sRGB transfer function and
 * the specification fixes neither its precision nor its rounding, so a component
 * landing a level either side of the arithmetic answer is conforming. Two levels
 * is nowhere near enough to reach the unencoded colour — the nearest pair any
 * row here uses is nine levels apart, and the widest is seventy.
 *
 * Both adapters this gate runs against — `google swiftshader` and a real
 * `amd rdna-3` — returned every backdrop byte *exactly*, so this window has
 * never had to absorb anything. If a platform ever needs it widened, that is a
 * finding to report rather than a number to raise.
 */
const BACKDROP_TOLERANCE = 2;

/**
 * How many frames the backdrop check looks at before giving its verdict.
 *
 * It takes the *best* of them, and that is deliberate rather than lenient. A
 * game decides for itself what covers its clear colour at any instant — flappy
 * puts a menu and a scrim over the whole sky the moment the bird dies, which is
 * a second or two after group C launches it — so a single sample is a race
 * against the simulation and nothing else. One frame holding the encoded colour
 * over a large area is the whole claim; a run with no encode has no such frame
 * at any instant, because every one of them is the linear colour instead.
 */
const BACKDROP_SAMPLES = 8;

/**
 * The culling-statistics line `crcbl_render::cull_stats` logs when a readback
 * answers.
 *
 * Both numbers are matched rather than the prefix alone: that module's other
 * lines — the one it says when a device refuses a readback, and the one for a
 * request nothing ever answered — also begin `cull stats:`, and either of those
 * is the failure this check exists to catch rather than the success.
 */
const CULL_STATS_LINE = /cull stats: frame \d+ kept \d+ instances/;

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
 * The engine's `LevelFilter::Trace`, the level `CULL_STATS_LINE` needs.
 *
 * Raised for that one check and lowered again: it is the only per-frame line the
 * engine logs, and a run left at trace would push the rest of this file's
 * evidence out of the page's bounded log queue.
 */
const LOG_TRACE = 5;

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

/**
 * The control page's clear colour, and what it must read back as.
 *
 * These are the **unencoded** bytes — `0.2 x 255 = 51`, `0.8 x 255 = 204` — and
 * that is the right answer here, because the control configures a plain
 * `bgra8unorm` canvas of its own with no sRGB view. A canvas that wrongly
 * encoded would read back near `124, 200`, far outside the tolerance below, so
 * this check does discriminate; only do not read it as asserting an encode.
 */
const CONTROL_RGB = [0, 51, 204];

/**
 * How far a control channel may drift, per channel.
 *
 * The PNG round trip is exact and the clear is one colour, so the only source
 * of drift is the rasteriser's last bits and whether the canvas format is 8-bit
 * sRGB one way round or the other.
 */
const CONTROL_TOLERANCE = 8;

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
            // Reads back as ${CONTROL_RGB.join(', ')}: no sRGB view, so no encode.
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

// `findBrowser`, the flag builder and the kill are all in
// `web/tools/browser-launch.mjs`: the three gates in this directory need the
// same browser started the same way, and every platform-specific thing about
// doing that — where the binary lives, which flags name the real device, how to
// take a process tree down — is decided there once. What stays here is the part
// this gate alone owns: the window size its pixel counts depend on, and the
// adapter mode its readback control picks.

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
  // The canvas is sized by CSS against the viewport, so a fixed window makes
  // this gate's pixel counts mean the same thing on every machine.
  const flags = browserFlags({
    profile,
    mode,
    extra: ['--window-size=1024,768'],
  });
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
      stopBrowser(child, profile);
    },
  };
  running.add(browser);

  const portFile = join(profile, 'DevToolsActivePort');
  const deadline = Date.now() + LAUNCH_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (exited) {
      console.error(stderr.join('\n'));
      fail(`the browser stopped before it listened (${exited})`);
    }
    const endpoint = readDevToolsPort(portFile);
    if (endpoint) {
      browser.endpoint = `ws://127.0.0.1:${endpoint.port}${endpoint.path}`;
      return browser;
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
 * How much of the canvas is the demo's clear colour, encoded and unencoded.
 *
 * **This is the sampler the sRGB check reads, and it cannot be
 * {@link SAMPLE_CANVAS}**: that one quantises to five bits a channel so that two
 * rasterisers disagreeing in the last bit do not read as different frames, and
 * the claim here is about an exact byte. `rgb(10,10,15)` and `rgb(8,8,16)` are
 * one colour after that shift, and the second is not the sRGB encode of
 * anything.
 *
 * Two counts come back rather than one, because "the backdrop is not the colour
 * it should be" and "the backdrop is the colour a target with no encode would
 * hold" are different failures and only the second names its own cause. The
 * exact dominant colour comes back beside them so a frame that is neither can
 * still be read by a human.
 *
 * `toDataURL()` and not `drawImage(canvas, …)`, for the reason in the header.
 *
 * @param {string} selector The canvas to sample.
 * @param {number[]} encoded The sRGB encode of the clear colour, per channel.
 * @param {number[]} unencoded What the same clear leaves in a linear target.
 */
const SAMPLE_BACKDROP = (selector, encoded, unencoded) => `(async () => {
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
  const want = ${JSON.stringify(encoded)};
  const linear = ${JSON.stringify(unencoded)};
  const tolerance = ${BACKDROP_TOLERANCE};
  const histogram = new Map();
  let hit = 0;
  let dark = 0;
  for (let i = 0; i < pixels.length; i += 4) {
    const key = (pixels[i] << 16) | (pixels[i + 1] << 8) | pixels[i + 2];
    histogram.set(key, (histogram.get(key) ?? 0) + 1);
    let isEncoded = true;
    let isLinear = true;
    for (let c = 0; c < 3; c += 1) {
      if (Math.abs(pixels[i + c] - want[c]) > tolerance) isEncoded = false;
      if (Math.abs(pixels[i + c] - linear[c]) > tolerance) isLinear = false;
    }
    if (isEncoded) hit += 1;
    if (isLinear) dark += 1;
  }
  const total = pixels.length / 4;
  let bestKey = 0;
  let bestCount = -1;
  for (const [key, count] of histogram) {
    if (count > bestCount) {
      bestCount = count;
      bestKey = key;
    }
  }
  return {
    width: scratch.width,
    height: scratch.height,
    encoded: hit / total,
    unencoded: dark / total,
    dominant: [(bestKey >> 16) & 255, (bestKey >> 8) & 255, bestKey & 255],
    dominantShare: bestCount / total,
  };
})()`;

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
    // One clear, so one colour, and it must be the one the control asked for,
    // within `CONTROL_TOLERANCE` per channel.
    const seen = sample?.top?.[0]?.rgb ?? [-1, -1, -1];
    const matches =
      sample &&
      sample.top[0].share > 0.99 &&
      seen.every(
        (value, i) => Math.abs(value - CONTROL_RGB[i]) <= CONTROL_TOLERANCE
      );
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
const binary = findBrowser(fail);

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
  // `Instance::backend()`. Reading the backend's own line — rather than any
  // adapter line — is what makes this a check that the *right* backend opened
  // the device rather than that some backend did. There is one right answer
  // here: `crcbl-wgpu` is a `cfg(not(target_arch = "wasm32"))` dependency of the
  // umbrella, so `crcbl-webgpu` is the only GPU backend a demo's wasm links, and
  // a `hal: wgpu adapter` line from a browser would mean the manifest had
  // regressed. This used to be a `CRCBL_WEB_BACKEND`-selected pair of strings.
  const backendAdapterLine = 'hal: webgpu adapter';
  check(
    'B',
    'the webgpu backend opened a device',
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

  // **A number that only comes back if a readback completed**, and the one
  // check here that is about the GPU answering rather than about pixels.
  //
  // `crcbl_render::cull_stats` copies the cull's survivor count into a
  // host-readable buffer and polls for it across frames; what it gets reaches
  // the debug panel, which is *glyphs on the canvas* — there is no DOM element,
  // no wasm export and nothing in `crcbl.gpu.stats()` carrying it. So the ring's
  // own `trace!` line, emitted where the answer lands, is the only thing a page
  // can be asked for, and it is turned on for the length of this check alone.
  //
  // It is worth a check because this failed **in a browser and nowhere else**:
  // `crcbl-webgpu` answers the first `poll_readback` on a handle with `Pending`
  // and sends the real answer back a frame later, so a ring that polled a slot
  // once and released it on the next line threw every answer away unread, and
  // the panel's culling rows were empty for ever here while every native backend
  // passed. Nothing else in this file would notice: the canvas looks identical.
  // **Only a demo that culls something can report having culled it.** The ring
  // records a stats copy when the cull pass runs over mesh instances; a demo
  // whose frame is sprites and text builds a `ForwardRenderer` all the same but
  // gives that pass nothing to count, so the readback correctly never arrives
  // and asserting it here would be asserting a thing that is not true. `culls`
  // in EXPECTATIONS names the demos with mesh geometry — the ones this claim is
  // about — rather than skipping the check quietly everywhere else.
  if (EXPECTED.culls) {
    await evaluate(page, `(globalThis.crcbl.logLevel(${LOG_TRACE}), true)`);
    const culled = await until(
      async () =>
        consoleLines.find((line) => CULL_STATS_LINE.test(line)) ?? null
    );
    await evaluate(page, `(globalThis.crcbl.logLevel(${LOG_INFO}), true)`);
    check(
      'D',
      'the culling statistics come back off the GPU',
      Boolean(culled),
      culled?.trim() ?? 'the cull-stats readback never answered'
    );
  }

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

  // **THE ONE CHECK HERE THAT IS ABOUT THE COLOUR OF THE FRAME RATHER THAN
  // ABOUT THERE BEING ONE**, and the gap the rest of this file left open. Group
  // D asks whether the canvas has more than one colour and whether it changes;
  // both of those pass identically on a frame that is a transfer function too
  // dark, which is exactly what the demo site shipped — a canvas configured
  // without its `-srgb` viewFormat, so every value the engine wrote went out
  // unencoded. `crcbl-golden` and the render harness compare pixels but render
  // *offscreen*, through `Replayer#configureOffscreenSwapchain`, which owns its
  // own textures and never calls `context.configure` — the path that was never
  // broken.
  //
  // **A FLAT CLEAR AT A MID-RANGE COLOUR IS THE WHOLE DESIGN.** 0.0 and 1.0 are
  // fixed points of the sRGB transfer function and encode to themselves, so a
  // clear to black or white reads back the same whether the encode happened or
  // not — the shape of a check that cannot fail, and the reason the probe's own
  // present gate stopped clearing to red. Every colour in `EXPECTATIONS` is
  // mid-range in at least one channel and none is a fixed point in any. A flat
  // fill also needs no per-platform tolerance and no expected-fail list: it is
  // one clear, so every rasteriser produces the same bytes, and both of the ones
  // this gate has run against produced them exactly.
  //
  // **AND IT IS A PICTURE, WHICH IS WHAT THE PROBE'S GROUPS I AND X ARE NOT.**
  // Those two are not a gap this fills — reintroduce the shipped bug and group I
  // goes red on its own — but every byte they compare is copied out on the GPU
  // by `crcbl-webgpu` and handed to wasm over the reply channel, and both drive
  // the probe exports on a page with no engine running. Neither asks the browser
  // what it *composited*, and neither is a demo. This reads the result off the
  // element a visitor looks at, through the same `toDataURL` the header measured
  // as the one spelling that reports a WebGPU canvas at all.
  //
  // The canvas written below is this group's evidence: it is the same
  // `toDataURL` of the same element a moment later, so a failure here comes with
  // the picture that produced it.
  if (EXPECTED.backdrop) {
    group('G — the frame is sRGB-encoded');

    const { source, encoded, unencoded, share } = EXPECTED.backdrop;
    let best = null;
    for (let i = 0; i < BACKDROP_SAMPLES; i += 1) {
      const sample = await evaluate(
        page,
        SAMPLE_BACKDROP('#canvas', encoded, unencoded)
      );
      if (sample && (!best || sample.encoded > best.encoded)) best = sample;
      if (best && best.encoded >= share) break;
    }

    const want = `rgb(${encoded.join(',')})`;
    const linear = `rgb(${unencoded.join(',')})`;
    check(
      'G',
      `the ${source} clear reaches the canvas sRGB-encoded`,
      best !== null && best.encoded >= share,
      best === null
        ? 'nothing sampled — the canvas reported no pixels'
        : best.encoded >= share
          ? `${want} over ${(best.encoded * 100).toFixed(1)}% of the canvas, ` +
            `against the ${(share * 100).toFixed(1)}% this demo owes`
          : `expected ${want} over at least ${(share * 100).toFixed(1)}% of the ` +
            `canvas and found it on ${(best.encoded * 100).toFixed(1)}%; the ` +
            `dominant colour is rgb(${best.dominant.join(',')}) at ` +
            `${(best.dominantShare * 100).toFixed(1)}%` +
            (best.unencoded >= share
              ? ` — THE FRAME CAME BACK UNENCODED, ${linear} on ` +
                `${(best.unencoded * 100).toFixed(1)}% of it: the canvas was ` +
                `configured without its -srgb viewFormat, or the acquired frame ` +
                `was viewed in the base format, and every frame this demo ` +
                `presents is a transfer function too dark`
              : '')
    );
  }

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
