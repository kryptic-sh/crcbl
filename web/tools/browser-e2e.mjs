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
 * no input at all rather than that nobody got round to writing the checks. Group
 * C skips its start-key half for such a demo — there is no waiting state to
 * leave, so a dispatched key and an assertion about what it did would be a check
 * wired to nothing — and keeps the half that is about the simulation advancing,
 * which is the claim every demo can make. `started`, `startedLabel` and
 * `startedFailure` are then unused and left out. `waiting` and `moving` are
 * required of every row.
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
    // visible result and [`SAMPLE_PADDLE`] can read it. `lives` is how the gate
    // knows the start menu has gone: `menu::MenuKind::of` shows it while the run
    // is untouched, a menu on screen owns the button, and a lost life is what
    // returns the game to WAITING with nothing over it — the state a tap serves
    // from, and the reason the tap binding exists at all.
    touch: { paddle: true, lives: /Lives: (\d+)/ },
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
    touch: { height: /\by: (-?[\d.]+)/ },
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
 * The checks need two — one to hold and one to fumble with, which is what the
 * shim's `isPrimary` filter is about — and a phone reports about this many.
 */
const MAX_TOUCH_POINTS = 5;

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
 * window "a second contact moves nothing" watches nothing happen for, and a
 * negative claim has no observable to poll. The control inside that check — the
 * first contact moving, inside this same window — is what stops a window too
 * short to show a move from making it pass for free.
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

/**
 * Every browser this process started and has not stopped.
 *
 * A leaked Chromium is not a tidiness problem: it holds a GPU context and a
 * profile directory, and a developer who runs the harness a few times ends up
 * with several of them. [`fail`] and the exit hook below close over this so
 * that no exit path can skip the kill.
 *
 * @type {Set<{ stop: () => void }>}
 */
const running = new Set();

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
  check(
    'B',
    'the wgpu backend opened a device',
    said('hal: wgpu adapter'),
    said('hal: wgpu adapter')?.trim() ?? 'no adapter line'
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

      // **The `isPrimary` filter.** The engine's pointer seam has no contact id,
      // so a second finger is not a second pointer to it — it is the same one
      // teleporting. One contact holds the paddle while another lands elsewhere
      // and moves; the paddle must stay with the first.
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
        'a second contact moves nothing',
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
