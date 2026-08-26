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
// WHAT IT ASSERTS. Nine groups, printed in order:
//
//   A  the platform — `navigator.gpu`, an adapter, **that this browser can
//      report canvas pixels at all** (see below; this one is not a formality),
//      and that the origin is cross-origin isolated, which is what decides
//      whether a threaded wasm build could run here at all
//   B  the engine boots — canvas size, wgpu backend, swapchain, STATUS_RUNNING
//   C  input drives the simulation — a real click focuses the canvas, a real
//      Space key launches the ball, and the game's own HUD log line changes.
//      A demo with no input at all (`key: null` in EXPECTATIONS) skips the key
//      and keeps the log line, which is the half it can still make good on.
//      A demo with a drop target (`drop`, and only viewer has one) also has a
//      document dragged onto its canvas, and one that is not a document at all
//   D  it renders — no WebGPU device errors, the canvas is not one flat colour,
//      and the canvas changes from frame to frame while the ball is in flight
//   E  focus and pause — a blurred canvas pauses and runs no ticks, focus
//      coming back does not resume on its own, and Escape does
//   F  a finger — real touch contacts, which a dispatched mouse is not
//   G  the frame is sRGB-encoded — the demo's own flat clear colour, read off
//      the canvas as the browser composited it and compared against the byte an
//      sRGB target holds. Only for a demo whose clear reaches the screen
//   H  the reporting channels are open — three of the checks above assert a
//      *silence*, and a closed channel reports silence too. This group breaks
//      each of the three on purpose and asserts the break was seen. It runs
//      after every check whose silence it is proving, so it cannot dirty one
//   I  the demo lets go of what it took — the replayer's handle tables, read
//      after the demo has been stopped through its own button. `crcbl-vk`
//      names every object a caller never destroyed at device teardown and its
//      e2e runners fail on that line; this is the same question asked of the
//      browser side, whose tables are the handle table `crcbl-webgpu` has
//      instead of one of its own. It runs after group H because it ends the run
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

import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  evaluate,
  findBrowser,
  launch as launchBrowser,
  openPage,
  pause,
  stopEverything,
  until as pollUntil,
} from './browser-launch.mjs';
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
 * no key that *starts* it rather than that nobody got round to writing the
 * checks. Group C skips its start-key half for such a demo — there is no waiting
 * state to leave, so a dispatched key and an assertion about what it did would be
 * a check wired to nothing — and keeps the half that is about the simulation
 * advancing, which is the claim every demo can make. `started`, `startedLabel`
 * and `startedFailure` are then unused and left out. `waiting` and `moving` are
 * required of every row.
 *
 * Three rows use it and they are `null` for different reasons: hud takes no
 * input at all, while lantern and quarry take plenty but have no run to begin —
 * which is why this says "no start key" rather than "no input".
 *
 * `drop` is the third optional key, and only viewer has one: it is the
 * document this harness drags onto the canvas, and the numbers the `[HUD]` line
 * has to reach once the page has opened it. See {@link quadPairGlb} for why the
 * document is built here rather than checked in.
 *
 * `playing` is the fourth, and only viewer has one of those either: a second
 * pattern read the way `moving` is, for the demo whose subject is a *document*
 * rather than a simulation. `moving` says the page is running; `playing` says
 * the animation inside the file it opened is being sampled. A row without one
 * is a demo that ships no rig, which is every other demo here.
 *
 * `walk` is the fifth, and only puppet has one: the demo whose subject is a
 * character *controller*, where "it moved" is not enough on its own. It names
 * the key to hold, the readings to watch, and the two step heights the map puts
 * either side of the offset the controller climbs — so the block that reads it
 * can require the character to advance while the key is held, to stop when it is
 * released, to climb the step under that offset and to be refused by the one
 * over it. Every positive in it has its control beside it, because each half
 * passes on its own for a demo that is broken in the other direction.
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
 * over every pixel of `art::GROUND`; lantern has no `clear_color` pass at all —
 * it is a lit room, and the nearest thing to a flat region in its frame is under
 * a tenth of the canvas; and quarry has none either, and the flat region behind
 * its face is whatever `crcbl-render` clears a 3D frame to rather than a byte
 * this sample chose. None of the four can make this claim, and a row that
 * quietly skipped it would be the check passing for the wrong reason.
 */
/**
 * A `.glb` of two quads, one at the origin and one three metres along `x`.
 *
 * **Built here rather than vendored**, which is the argument
 * `apps/viewer/src/fixture.rs` and `crcbl-scene`'s `gltf_fixture` both make and
 * make for the same reason: a `.glb` is a binary container, so a checked-in one
 * is a fixture nobody reviewing a change can read, and every number this file
 * asserts on ought to be written out in this file. It is the same document
 * those two build, in the same layout, with a second node added — because the
 * point of the drop check is that the page is showing a document it did not
 * ship with, and the instance count is what says so.
 *
 * Two nodes and no skin, so the `[HUD]` line reads `instances: 2  …  joints: 0`
 * where the demo document reads `instances: 3  …  joints: 2`. Neither number
 * can be reached by a page that took the bytes and did nothing with them.
 *
 * **THE TWO NODES ARE PLACED THE WAY THEY ARE BECAUSE GROUP D READS THIS
 * CANVAS AFTERWARDS**, and a document the viewer frames badly makes that group
 * fail on a page that is working. Both of them:
 *
 * - **They stand above the grid**, which is where a model out of any DCC tool
 *   stands. `OrbitCamera::frame` aims at the bounding box's centre and keeps
 *   its pitch, so a document centred *on* the ground plane puts the eye exactly
 *   in that plane: the grid goes edge-on and disappears, and the quads are
 *   back-face culled for half of the turntable's sweep — the whole canvas comes
 *   back as one flat clear colour. Measured, not guessed: that is precisely how
 *   group D failed when these nodes sat at `y = 0`.
 * - **The box has depth on every axis**, the second one turned a quarter turn
 *   about `y` so the pair is not one flat sheet. `apps/viewer/src/demo_model.rs`
 *   holds the same property of the demo document in
 *   `the_demo_documents_world_box_is_finite_and_has_depth_on_every_axis`, and
 *   this is the same trap seen from the other side.
 *
 * @returns {Buffer} the container, ready to be handed to a `File`
 */
function quadPairGlb() {
  // One metre across in the XY plane, facing +Z; two triangles wound
  // counter-clockwise seen from there, which is the front face.
  const positions = [
    [-0.5, -0.5, 0],
    [0.5, -0.5, 0],
    [0.5, 0.5, 0],
    [-0.5, 0.5, 0],
  ];
  const normals = [
    [0, 0, 1],
    [0, 0, 1],
    [0, 0, 1],
    [0, 0, 1],
  ];
  const indices = [0, 1, 2, 0, 2, 3];

  const POSITIONS_AT = 0;
  const NORMALS_AT = positions.length * 3 * 4;
  const INDICES_AT = NORMALS_AT + normals.length * 3 * 4;
  const bin = Buffer.alloc(INDICES_AT + indices.length * 2);
  let at = POSITIONS_AT;
  for (const value of [...positions, ...normals].flat()) {
    bin.writeFloatLE(value, at);
    at += 4;
  }
  for (const index of indices) {
    bin.writeUInt16LE(index, at);
    at += 2;
  }

  // `metallicFactor` is written out as zero for the reason
  // `crates/crcbl/tests/gltf_e2e.rs` gives: glTF defaults a material to a fully
  // rough conductor, which has no diffuse lobe at all, so a document that left
  // it out would be nearly black in any frame drawn from it — and group D reads
  // this canvas afterwards.
  const json = JSON.stringify({
    asset: { version: '2.0' },
    scene: 0,
    scenes: [{ nodes: [0, 1] }],
    nodes: [
      { name: 'panel', mesh: 0, translation: [0, 1, 0] },
      {
        name: 'panel',
        mesh: 0,
        translation: [1.5, 1, 1.5],
        // A quarter turn about +y as an `xyzw` quaternion, so this quad faces
        // +x where the other faces +z.
        rotation: [0, Math.SQRT1_2, 0, Math.SQRT1_2],
      },
    ],
    meshes: [
      {
        name: 'panel',
        primitives: [
          {
            attributes: { POSITION: 0, NORMAL: 1 },
            indices: 2,
            material: 0,
          },
        ],
      },
    ],
    materials: [
      {
        name: 'paint',
        pbrMetallicRoughness: {
          baseColorFactor: [0.8, 0.8, 0.8, 1.0],
          metallicFactor: 0.0,
          roughnessFactor: 1.0,
        },
      },
    ],
    accessors: [
      { bufferView: 0, componentType: 5126, count: 4, type: 'VEC3' },
      { bufferView: 1, componentType: 5126, count: 4, type: 'VEC3' },
      { bufferView: 2, componentType: 5123, count: 6, type: 'SCALAR' },
    ],
    bufferViews: [
      { buffer: 0, byteOffset: POSITIONS_AT, byteLength: NORMALS_AT },
      {
        buffer: 0,
        byteOffset: NORMALS_AT,
        byteLength: INDICES_AT - NORMALS_AT,
      },
      {
        buffer: 0,
        byteOffset: INDICES_AT,
        byteLength: bin.length - INDICES_AT,
      },
    ],
    buffers: [{ byteLength: bin.length }],
  });

  // Both chunks are padded to a multiple of four, the JSON one with spaces and
  // the binary one with zeroes, which the container format requires.
  const jsonChunk = Buffer.from(json, 'utf8');
  const jsonPad = Buffer.alloc((4 - (jsonChunk.length % 4)) % 4, 0x20);
  const binPad = Buffer.alloc((4 - (bin.length % 4)) % 4, 0);
  const jsonLen = jsonChunk.length + jsonPad.length;
  const binLen = bin.length + binPad.length;

  const header = Buffer.alloc(12);
  header.write('glTF', 0, 'ascii');
  header.writeUInt32LE(2, 4);
  header.writeUInt32LE(12 + 8 + jsonLen + 8 + binLen, 8);
  const jsonHeader = Buffer.alloc(8);
  jsonHeader.writeUInt32LE(jsonLen, 0);
  jsonHeader.write('JSON', 4, 'ascii');
  const binHeader = Buffer.alloc(8);
  binHeader.writeUInt32LE(binLen, 0);
  binHeader.write('BIN\0', 4, 'ascii');

  return Buffer.concat([
    header,
    jsonHeader,
    jsonChunk,
    jsonPad,
    binHeader,
    bin,
    binPad,
  ]);
}

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
  // **The other demo with no start key.** `apps/lantern` is a lighting fixture
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
  // or fell over before the first tick. `Lantern::log_heartbeat` prints the
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
  lantern: {
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
  // pause-menu row rather than a key, exactly as it is in lantern.
  //
  // `waiting` is this sample's own claim and it is the exit criterion the page
  // exists to satisfy: "web demo renders the scene on `IndirectPerBatch` … and
  // the summary line names the path it took". A browser has **no mesh stage and
  // no GPU-side draw count**, so `GeometryPath::IndirectPerBatch` is the arm the
  // selector resolves to by construction — the same shape of claim lantern's row
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
    // The other demo whose cull pass has something to count — see lantern's row
    // and group D. This one is the sample that pass exists for.
    culls: true,
    key: null,
    waiting: (line) =>
      line.includes('[HUD] tick: 60') &&
      line.includes('geometry: IndirectPerBatch'),
    moving: /eye z: (-?[\d.]+)/,
    movingLabel: 'the dolly keeps running down the face under its own steam',
  },
  // **The sample whose subject does not move.** viewer is a tool rather than a
  // game: it opens a document and shows it, and left alone it would draw the
  // same frame for ever — which group C, the one check every demo here makes,
  // cannot tell from a loop that has stopped. `apps/viewer`'s idle turntable is
  // what gives it something that advances under its own steam, and `turn` is
  // that angle on the debug panel. It stops the moment anyone drags or scrolls,
  // so this gate reads it before group F touches the canvas.
  viewer: {
    key: null,
    // `instances` is the document's own three, so this also says the generated
    // `.glb` reached the renderer rather than an empty scene arriving as a
    // successful boot.
    //
    // `joints` is the demo document's rig — a skin over two joint nodes, built
    // by `apps/viewer/src/demo_model.rs`. Nothing in this engine poses a
    // skeleton, so the count on the heartbeat is the *only* place a browser can
    // see that the import read one: `crcbl::scene::GltfScene::skins` reaching
    // `crate::model::Rig` reaching the `[HUD]` line. Asked for by its value
    // rather than by "is a number", because zero is what both a document with
    // no rig and an import that dropped the skin report. Counts only on this
    // line — the clip names are on the viewer's own listing panel, since a name
    // out of someone else's file could hold the space or the colon this parse
    // is made of.
    waiting: (line) =>
      line.includes('[HUD] tick:') &&
      line.includes('instances: 3') &&
      line.includes('joints: 2'),
    moving: /turn: ([\d.]+)/,
    movingLabel: 'the turntable carries the camera under its own steam',
    // **The document advancing, which is a different claim from the frame
    // advancing.** `turn` above is this page's own turntable — it would go on
    // moving over a document the engine never looked at twice. `pose` is how
    // far the demo document's own clip has carried its skeleton from its rest
    // pose, in metres, and it is read off the joint palette and off nothing
    // else: a playhead ticking over a pose nobody ever composed leaves it at
    // `0.00` for ever, and so does a conversion that handed the sampler joint
    // indices that name no joint. See `apps/viewer/src/anim.rs`, which is the
    // one consumer `crcbl-anim` has.
    //
    // Two different values is the whole assertion, exactly as it is for
    // `moving`: the clip loops, so the number sweeps rather than climbing.
    playing: /pose: ([\d.]+)/,
    playingLabel: 'the clip in the document plays under its own steam',
    // **The geometry being deformed by that clip, which `playing` above cannot
    // see.** `pose` is a number this application computes on the CPU and prints:
    // a page that samples the clip, composes the palette and then hands the GPU
    // nothing to skin with reports exactly the same sweep, draws the document's
    // bind pose for ever, and passes every other check in this file.
    //
    // So this one reads the canvas. The turntable is stopped first through the
    // viewer's own drag, because a moving camera changes the picture whatever
    // the geometry is doing, and then the two controls that stop this being a
    // check of nothing are asserted beside it: `turn` must have stopped taking
    // new values, and `pose` must still be taking them. Without the first, the
    // frames differ because the camera moved; without the second, they differ
    // because — well, nothing, and the check has no business passing.
    //
    // It uses the `moving` and `playing` patterns above, so a row carrying this
    // has to carry both of those. viewer is the only one that does.
    deforming: true,
    deformingLabel:
      'the document is deformed by its own clip, with the camera held still',
    // **The one demo that takes a file from the visitor**, so the one row with
    // a `drop`. `document` is dragged onto the canvas and `opened` is what the
    // `[HUD]` line has to say afterwards — numbers only a *loaded* document can
    // produce, and neither of them the demo document's own. `broken` is the
    // other half of the claim: bytes that are not a document at all, which must
    // leave the page drawing what it already had.
    drop: {
      name: 'dropped.glb',
      document: quadPairGlb(),
      opened: (line) =>
        line.includes('instances: 2') && line.includes('joints: 0'),
      openedLabel: 'instances: 2 and joints: 0',
      brokenName: 'broken.glb',
      broken: Buffer.from('not a glTF document', 'utf8'),
    },
  },
  // **The sample that flies itself.** orbit takes input, but a page that has
  // just loaded has had none — and a rocket standing on its pad is the same
  // still frame a stopped loop would draw. A script flies the ascent from the
  // sixtieth tick, so `alt` climbs without anyone touching a key, and the first
  // key a visitor presses ends the script for good. Group C reads it before
  // group F touches anything.
  bracket: {
    key: null,
    // Read off the *first* line, and what it asks is that `error:` is a number.
    // That readout is the sample's entire claim — how far the ladder is from
    // the true skills — and it is a mean over a population, so a single
    // non-finite rating anywhere in it poisons the whole figure and every later
    // one. A `NaN` reads as a number to everything downstream, including the
    // plot, which would draw a curve with a hole in it rather than fail. The
    // place to catch it is where it is printed.
    waiting: (line) =>
      line.includes('[HUD] tick:') && /\berror: -?\d/.test(line),
    // Nothing here takes input, so the demo has to be advancing by itself for
    // this to move at all — which is the point: a visitor sees a ladder sorting
    // itself out and nothing they did.
    moving: /matches: (\d+)/,
    movingLabel: 'the population plays matches under its own steam',
  },
  // **The demo whose subject is the controller itself.** puppet takes input,
  // but a page that has just loaded has had none — and a character standing
  // still is the same still frame a stopped loop would draw. It paces a slow
  // circuit on the spawn pad from the first tick, so `px` moves without anyone
  // touching a key, and the first movement key ends the circuit for good. Group
  // C reads `moving` before the `walk` block below presses anything.
  puppet: {
    // The third demo that draws mesh instances, so the third whose cull pass
    // has something to count. See lantern's row and group D.
    culls: true,
    key: null,
    // Read off the *first* line, which is the character still on the spawn pad.
    // What it asks is that the controller has already run and found the map:
    // `ground: yes` is `MoveOutcome::grounded`, which no page that failed to
    // sweep a capsule against `apps/puppet/src/map.rs`'s world can report, and
    // `pilot: circuit` is what says nothing has taken the controls yet — the
    // waiting state this demo has instead of a start screen.
    waiting: (line) =>
      line.includes('[HUD] tick: 60') &&
      line.includes('ground: yes') &&
      line.includes('pilot: circuit'),
    moving: /\bpx: (-?[\d.]+)/,
    movingLabel: 'the character paces its circuit under its own steam',
    // **The controller, driven and then let go of** — the block below. Every
    // number here comes from `apps/puppet/src/map.rs`'s constants, and the demo
    // asserts the same pair natively in `game.rs`'s
    // `it_gets_onto_the_low_step_and_no_further`, so a failure here is a failure
    // of the browser path rather than a mystery about the map.
    walk: {
      // `code` is what the engine binds to; `text` and the virtual key code are
      // what a real keyboard sends. `KeyW` walks away from the camera, and the
      // camera opens at a yaw of zero — looking down `-Z`, which is where the
      // lane is — so a held `W` is a walk down `pz` and nothing else.
      code: 'KeyW',
      text: 'w',
      virtualKeyCode: 87,
      // Which reading advances under that key, and which way. `pz` falls.
      advance: /\bpz: (-?[\d.]+)/,
      // Where the character's feet are, and the highest they have ever been.
      feet: /\bpy: (-?[\d.]+)/,
      highest: /\btop: (-?[\d.]+)/,
      // `MoveOutcome::stepped_up` and `MoveOutcome::hit_wall`, counted. The
      // first is the positive — a step actually climbed — and the second is
      // what says the character was *pushing* against the one it did not climb
      // rather than standing near it.
      climbed: /\bclimbed: (\d+)/,
      blocked: /\bblocked: (\d+)/,
      // `map::LOW_STEP_TOP` and `map::HIGH_STEP_TOP`, in metres.
      lowStep: 0.3,
      highStep: 0.9,
      // **And the locomotion blend, which is milestone 2.** These three come
      // off a `[POSE]` line rather than the `[HUD]` one, because
      // `apps/puppet/src/app.rs` logs them on the frame's clock and the
      // heartbeat above is logged on the simulation's — that file argues it.
      //
      // * `blend` — where the character sits across `apps/puppet/src/anim.rs`'s
      //   locomotion set, 0 at the idle stance and 1 at the walk. It is a
      //   function of the *measured* speed, so a demo whose pose followed the
      //   keyboard instead would report it while standing against a wall.
      // * `mid` — how many frames the weight has spent strictly between the two
      //   stops. The counter is the whole anti-snap claim: these lines are a
      //   second apart and a crossing takes about a second and a half, so
      //   sampling the weight cannot tell a sweep from a jump and counting the
      //   frames can. It stops rising the moment the weight reaches an end.
      // * `dev` — how far the pose has carried a joint from the rest pose, in
      //   metres. It sweeps while the character walks and holds still while it
      //   stands, because `apps/puppet/src/rig.rs`'s idle is a stance.
      blend: /\bblend: ([\d.]+)/,
      crossings: /\bmid: (\d+)/,
      deviation: /\bdev: ([\d.]+)/,
    },
  },
  // **The demo whose subject is a budget.** sparks takes no input at all — the
  // effects run from a fixed seed, which is the whole point of a per-particle
  // hash — so `moving` reads a count the simulation moves on its own, and the
  // `budget` block below reads the two claims that a moving count cannot make.
  sparks: {
    // The fourth demo that draws mesh instances, so the fourth whose cull pass
    // has something to count. Every particle on this page is an instance; see
    // lantern's row and group D.
    culls: true,
    key: null,
    // Read off the *first* line. What it asks is that the effects are already
    // running — a pool with particles in it and a share the greedy effect is
    // being held to — because this demo has no start screen and no waiting
    // state, and a page that booted its renderer and simulated nothing would
    // otherwise look identical.
    waiting: (line) =>
      line.includes('[HUD] tick:') &&
      /\blive: [1-9]\d*/.test(line) &&
      /\bspam-share: \d+/.test(line),
    moving: /\blive: (\d+)/,
    movingLabel: 'the effects spawn and retire under their own steam',
    // **The two pairs the block below reads.** Every pattern here is a field of
    // the `[HUD]` line `apps/sparks/src/app.rs` logs, and that file argues why
    // each is on it.
    budget: {
      // Whether the switchable emitter is running, and how many particles it
      // holds. The pair is the rise-and-fall claim: a count that climbed while
      // the emitter ran proves nothing on its own, because a demo whose
      // particles never retired would report the same thing for ever.
      emitting: /\bpuff-emitting: (yes|no)/,
      held: /\bpuff: (\d+)/,
      // What the greedy effect holds, and what it is allowed. The one number on
      // the page that must *not* move.
      greedy: /\bspam: (\d+)/,
      share: /\bspam-share: (\d+)/,
      // How many spawns its budget has refused. The control for the pair above:
      // a count sitting on its share could be an emitter that happens to ask
      // for exactly that many, and a refusal counter climbing while the count
      // holds still cannot be anything but a clamp.
      refused: /\bclamped: (\d+)/,
    },
  },
  // **The demo whose subject is a controller seen from the other end**, and the
  // other half of `apps/puppet`'s claim: the same
  // `crcbl::phys::CharacterController`, driven from a first-person camera that
  // shares no code with puppet's orbit one. Everything in the `range` block
  // below is that seam being exercised in a browser.
  breach: {
    // The fifth demo that draws mesh instances, so the fifth whose cull pass
    // has something to count. See lantern's row and group D.
    culls: true,
    // **No start key, and no click either.** breach binds its trigger to a key
    // alone — `apps/breach/src/app.rs`'s `ACTION_FIRE` argues why — and it has
    // no waiting state to leave: the range runs its own demonstration from the
    // first tick and the first thing the player does ends it. So there is
    // nothing for group C's `Space` to start, and the `range` block below
    // presses everything this demo answers to.
    key: null,
    // **This demo's heartbeat is twice as fast as everyone else's**, at half a
    // simulated second — `apps/breach/src/game.rs`'s `HEARTBEAT_TICKS` says why,
    // and it is this gate: every wait the two blocks below make is a whole
    // number of heartbeats, so the period is what the step costs. Declared here
    // because group E divides by it to work out how far behind real time this
    // machine is running the demo, and a wrong denominator there shrinks every
    // budget that reading scales.
    beatMs: 500,
    // Read off the *first* line, and it asks three things a page that merely
    // booted cannot say. `ground: yes` is `MoveOutcome::grounded`, which no run
    // that failed to sweep a capsule against `apps/breach/src/map.rs`'s world
    // can report. `pilot: range` is the waiting state this demo has instead of
    // a start screen. And the three selectors are rule 12 — a browser has no
    // mesh stage, no bindless and no ray query, so these are the arms the
    // capability model resolves to *by construction*, and this line is the only
    // place anything checks that they are the ones the frame took. The same
    // shape of claim lantern and quarry make, asked of all three selectors at
    // once. `Breach::log_heartbeat` prints their own `Debug`, which is a
    // deliberate coupling to `crates/crcbl-hal/src/caps.rs`: a renamed variant
    // fails here loudly.
    waiting: (line) =>
      line.includes('[HUD] tick: 30') &&
      line.includes('ground: yes') &&
      line.includes('pilot: range') &&
      line.includes('geometry: IndirectPerBatch') &&
      line.includes('binding: ArrayPages') &&
      line.includes('lighting: Rasterised'),
    // The travelling plate on the far lane, and it is the strictest value this
    // demo has: `map::plate_x` is a pure function of the *simulated* time, so a
    // page presenting frames without ticking leaves it standing still, and
    // **nothing a player does can move it** — which is what keeps this check
    // about the loop rather than about the input that group C has already
    // dispatched by the time it runs. `map::MOVER_PERIOD_S` is slow enough to
    // be tracked by a shooter and still moves the plate far more than the two
    // decimal places the line prints it to, between beats a second apart.
    moving: /\bmover: (-?[\d.]+)/,
    movingLabel:
      'the travelling target keeps crossing its lane under its own steam',
    // **The controller and the pistol, driven and then let go of** — the block
    // below. Every pattern here is a field of the `[HUD]` line
    // `apps/breach/src/app.rs` logs, and that file argues why each is on it.
    range: {
      // `code` is what the engine binds to; `text` and the virtual key code are
      // what a real keyboard sends.
      //
      // `KeyW` walks away from the eye, and the range squares the shooter up
      // down the near lane the moment they take the controls — so a held `W` is
      // a walk down `pz` and nothing else.
      //
      // `ArrowRight` and `ArrowUp` are the look. **A browser has no mouselook
      // at all**: the web shell reports no `RAW_POINTER_MOTION`, because
      // `movementX`/`movementY` under Pointer Lock are accelerated by the same
      // OS layer the capability exists to bypass, so the engine declines the
      // lock — which `docs/plan/sample/11-breach.md` names as one of the four
      // reasons the competitive game is native only. The arrows are what this
      // page is aimed with, and therefore what this gate drives.
      walk: { code: 'KeyW', key: 'w', text: 'w', virtualKeyCode: 87 },
      turn: { code: 'ArrowRight', key: 'ArrowRight', virtualKeyCode: 39 },
      turnBack: { code: 'ArrowLeft', key: 'ArrowLeft', virtualKeyCode: 37 },
      tilt: { code: 'ArrowUp', key: 'ArrowUp', virtualKeyCode: 38 },
      tiltBack: { code: 'ArrowDown', key: 'ArrowDown', virtualKeyCode: 40 },
      fire: { code: 'Space', key: ' ', text: ' ', virtualKeyCode: 32 },
      // Which reading advances under the walk key, and which way. `pz` falls.
      advance: /\bpz: (-?[\d.]+)/,
      // Where the view is pointing. The engine's own number, read off the
      // heartbeat rather than off a pixel: a check that compared two canvases
      // could not tell a turned view from a target that moved.
      yaw: /\byaw: (-?[\d.]+)/,
      // And how far above or below level it is, which is the other half of
      // putting the view back where the range is before the generic groups
      // judge what is on the canvas.
      pitch: /\bpitch: (-?[\d.]+)/,
      // Which pilot the range is under. The mark the look checks measure their
      // "the view was standing still" control from — before it the range is
      // sweeping its own aim, and a stillness claim there would be false.
      pilot: /\bpilot: (range|player)/,
      // How far down the range the walk must still stop short of, in the same
      // metres `advance` reads. **This is what stops "and stops where they are"
      // passing for a player the firing line stopped**: the range is a kerb the
      // controller will not climb, so a build that ignored every key release
      // walks to the line and then stands perfectly still there — three equal
      // readings, and a check with nothing to say about the key. The line is at
      // `map::FIRING_LINE_Z`, which the heartbeat reports as about 0.5; the
      // honest run settles six metres short of it.
      stopsShortOf: 3.0,
      // The score, and what the crosshair is on.
      shots: /\bshots: (\d+)/,
      hits: /\bhits: (\d+)/,
      aim: /\baim: ([a-z-]+)/,
      // The nearest lane's plate, up or down — the observable a hit has on the
      // *range* rather than on the score.
      nearest: /\bnear: (up|down)/,
      // What `aim` reads when the crosshair is on the room rather than on a
      // target: `game::Aim::Range`'s label. The miss is aimed by tilting the
      // view up a nudge at a time until the crosshair has cleared every plate
      // — which the ceiling then stands behind, so the shot has something to
      // land on rather than reaching its range in mid-air.
      offTarget: 'range',
    },
    // **AND THE OTHER MAP**, which is milestone 0's other half: the bot
    // practice map. `apps/breach` is the only demo on the site with two of
    // them, and the only one a query string chooses between — see
    // `web/demos/breach/main.js` and `apps/breach/src/web.rs`.
    //
    // Every pattern here is a field of the `[HUD]` line
    // `apps/breach/src/app.rs` logs on that map, and that file argues why each
    // is on it. Nothing in this block presses a key: three bots walking their
    // patrols is already a picture that moves, and one of them is in the open
    // in front of the spawn shooting at a visitor who has touched nothing —
    // which is what lets every claim below be about the simulation rather than
    // about the input path group C has already exercised.
    practice: {
      // What `web/demos/breach/main.js` turns into
      // `__crcbl_breach_map(1)`, before boot.
      query: '?map=practice',
      // Which map the rest of the line is about, and the name this one has.
      map: /\bmap: ([a-z]+)/,
      mapName: 'practice',
      // The player's own position. `pz` is the same field the `range` block's
      // `advance` reads and is taken from there rather than spelled twice;
      // `px` is this block's, because nothing else needs it.
      playerAcross: /\bpx: (-?[\d.]+)/,
      // Where the first bot's feet are. **The pair this block's liveness claim
      // rests on**, and it is a *bot's* position: nothing on this map moves it
      // but that bot's own `move_and_slide`.
      botAcross: /\bbotx: (-?[\d.]+)/,
      botAlong: /\bbotz: (-?[\d.]+)/,
      // How many bots are on their feet.
      alive: /\bbots: (\d+)/,
      // How many have the player in sight, and how many are near enough to and
      // cannot because something is in the way. The second is the control for
      // the first — see `apps/breach/src/bots.rs`, where the sighting is one
      // `PhysicsWorld::cast_ray` and cover is the only thing that answers it.
      seen: /\bseen: (\d+)/,
      covered: /\bcovered: (\d+)/,
      // What the player has left, how many times the bots have run them out of
      // it, and the bots' trigger pulls against the ones that arrived.
      health: /\bhp: (\d+)/,
      downs: /\bdowns: (\d+)/,
      fired: /\bfired: (\d+)/,
      taken: /\btaken: (\d+)/,
      // The field only the *firing range*'s heartbeat carries, which is how
      // this block tells the two maps' lines apart by shape rather than by
      // trusting the slice it took: a travelling plate is the other map's
      // liveness fixture and must not be what answers for this one.
      moverField: /\bmover: /,
      // How many bots the map has, from `map::practice::BOTS`. Read rather
      // than assumed: a page that opened the practice map with no bots on it
      // would pass every "a number changed" check going.
      count: 3,
    },
  },
  // **The demo whose subject is the fallback paths carrying real content**, and
  // the site's first 3D one that is a *game* rather than a fixture.
  // `docs/plan/sample/15-shard.md`'s milestone 1 exists to put a lit interior
  // through `IndirectPerBatch`, `ArrayPages` and `LightingPath::Rasterised`,
  // which are the arms a browser resolves to by construction — so the `waiting`
  // line below is the same rule-12 claim breach makes, asked of a scene with
  // more lights than shadow slots and a baked irradiance volume in it.
  //
  // The `zone` block after it is where this row earns its length: it drives the
  // walk key, and then it puts the torches out.
  shard: {
    // The sixth demo that draws mesh instances, so the sixth whose cull pass has
    // something to count. See lantern's row and group D.
    culls: true,
    // **No start key.** The zone is already lit and already flickering when the
    // page opens, and the character is standing in it — there is no waiting
    // state to leave, so group C's start-key half would be a check wired to
    // nothing. The `zone` block presses everything this demo answers to.
    key: null,
    // This demo's heartbeat is a quarter of a simulated second, which is the
    // shortest on the site — `apps/shard/src/game.rs`'s `HEARTBEAT_TICKS`
    // argues it, and the argument is this gate: shard is the heaviest scene
    // here, so every wait measured in beats is what its browser step costs.
    // Group E divides by this to work out how far behind real time the machine
    // is running the demo.
    beatMs: 250,
    // Read off the *first* line. `ground: yes` is `MoveOutcome::grounded`, which
    // no run that failed to sweep a capsule against the colliders
    // `apps/shard/src/zone.rs` builds from `LAYOUT` can report. `torches: lit`
    // is the switch this demo's whole light block turns on, read before
    // anything has touched it. And the three selectors are rule 12: that plan
    // says path reporting "matters here more than anywhere, because this is the
    // sample where the fallback paths carry real content", and this line is the
    // only place anything checks that the frames went through the arms the
    // capability model is supposed to have chosen. `Shard::log_heartbeat`
    // prints their own `Debug`, which is a deliberate coupling to
    // `crates/crcbl-hal/src/caps.rs`: a renamed variant fails here loudly.
    waiting: (line) =>
      line.includes('[HUD] tick: 15') &&
      line.includes('ground: yes') &&
      line.includes('torches: lit') &&
      line.includes('geometry: IndirectPerBatch') &&
      line.includes('binding: ArrayPages') &&
      line.includes('lighting: Rasterised'),
    // How bright the first torch is, and the strictest value this demo has:
    // `light::flame` is a pure function of the *simulated* seconds, so a page
    // presenting frames without ticking leaves it standing still, and
    // **nothing a player's walk can move** — which is what keeps this check
    // about the loop rather than about the input group C has already
    // dispatched. Its two periods are incommensurable and neither divides the
    // half-second beat, so consecutive heartbeats cannot land on one value.
    moving: /\bflame: ([\d.]+)/,
    movingLabel: 'the torchlight keeps flickering under its own steam',
    // **The walk, and the light** — the block below. Every pattern here is a
    // field of the `[HUD]` line `apps/shard/src/app.rs` logs, and that file
    // argues why each is on it.
    zone: {
      // `code` is what the engine binds to; `text` and the virtual key code are
      // what a real keyboard sends. `KeyW` walks away from the camera, and the
      // camera opens at a bearing of zero looking down `-Z` — so a held `W` is a
      // walk down `pz` and nothing else. The spawn is eighteen metres down that
      // axis from the far wall, which is what makes the "and then it stopped"
      // control below a claim about the key rather than about the room.
      walk: { code: 'KeyW', key: 'w', text: 'w', virtualKeyCode: 87 },
      // The one key on this page that changes the *picture* rather than the
      // position. `apps/shard/src/app.rs` handles it outside the action map,
      // because it is presentation.
      torch: { code: 'KeyL', key: 'l', text: 'l', virtualKeyCode: 76 },
      // Which reading advances under the walk key, and which way. `pz` falls.
      advance: /\bpz: (-?[\d.]+)/,
      // `MoveOutcome::hit_wall`, counted. **The control that stops "and then it
      // stopped" passing for a character the zone stopped**: three equal
      // readings are what a capsule pressed against stone reports too, and this
      // is what says nothing was pressed against.
      blocked: /\bblocked: (\d+)/,
      // The switch itself, so the canvas comparisons either side of the torch
      // key are anchored to a reading the *engine* made rather than to a
      // keystroke the driver merely dispatched.
      torches: /\btorches: (lit|out)/,
    },
    // **AND THE ZONE BEING FOUGHT IN**, which is slice 2 and the second of
    // `docs/plan/sample/15-shard.md`'s six verbs. Every pattern here is a field
    // of the `[HUD]` line `apps/shard/src/app.rs` logs, and that file argues why
    // each is on it.
    //
    // **This block runs after the lighting block, and that is load-bearing.**
    // The doused window above asks for a canvas that does not change *at all*,
    // and a body walking through the frame is a body to redraw. `foe::POSTS`
    // puts every foe out of `foe::NOTICE_M` of the spawn and out of the frame
    // the zone opens on — both asserted natively, in
    // `no_foe_can_reach_the_character_where_the_zone_opens` and
    // `no_foe_is_in_the_frame_the_zone_opens_on` — so nothing in this zone moves
    // until this block walks the character at something.
    fight: {
      // Walk and strike. `KeyW` is the same binding the `zone` block holds; the
      // blow is `Space`, for the reason `apps/breach` binds its trigger there
      // and not to a mouse button — see `apps/shard/src/app.rs`.
      walk: { code: 'KeyW', key: 'w', text: 'w', virtualKeyCode: 87 },
      strike: { code: 'Space', key: ' ', text: ' ', virtualKeyCode: 32 },
      // How many foes are on their feet. **Monotone**: nothing in this zone
      // respawns, so a count that has fallen cannot be missed by a reader that
      // polls late — which is the property that makes the kill check immune to
      // however slowly the renderer is drawing.
      alive: /\bfoes: (\d+)/,
      // How many of them have the character. **Not** monotone — a foe that is
      // felled stops being engaged — so every claim about it is made over the
      // whole retained buffer of heartbeats rather than off the latest line.
      engaged: /\bengaged: (\d+)/,
      // What the character has left, and how many times they have been put down
      // and returned to the spawn.
      health: /\bhp: (\d+)/,
      downs: /\bdowns: (\d+)/,
      // Blows swung, and the bodies they landed on. `swings` rising with `hits`
      // flat is a blow that reached nothing, which is the control for the cleave
      // being resolved against `PhysicsWorld::cast_ray` rather than counted.
      swings: /\bswings: (\d+)/,
      hits: /\bhits: (\d+)/,
      // How much health each side has taken off the other, summed. `taken` is
      // the monotone half `hp` cannot be: health comes back when the character
      // is put down, so a reader that missed the dip would see a full bar and no
      // evidence.
      dealt: /\bdealt: (\d+)/,
      taken: /\btaken: (\d+)/,
      // What the cleave would answer — `foe::Kind::label`, or the word below.
      // The same reading the trigger resolves with, which is what makes the
      // control blow deliberately a blow at nothing rather than a lucky miss.
      target: /\btarget: ([a-z]+)/,
      // What `target` reads when nothing is in reach.
      nothing: 'none',
      // How many foes the zone posts, from `foe::FOES`. Read rather than
      // assumed: a page that opened a zone with no foes in it would pass every
      // "a number changed" check going.
      count: 3,
      // What the character starts with, from `foe::HEALTH_MAX`. The control for
      // "they can be hurt" is that every beat before the fight read exactly
      // this.
      full: 100,
    },
    // **AND THE CHARACTER COMING BACK**, which is slice 3 and the last two of
    // `docs/plan/sample/15-shard.md`'s six verbs. Every pattern here is a field
    // of the `[HUD]` line `apps/shard/src/app.rs` logs.
    //
    // The two fields this block turns on are the ones `apps/shard/src/save.rs`
    // put on that line, and each is unmoved by how fast frames arrive:
    //
    // * `resumed` is decided once, in `assemble`, before the first tick — a
    //   session either opened from a save or it did not — so a reader that
    //   polls late reads what a reader that polled early would.
    // * `saves` is monotone and rises on the **simulated** clock:
    //   `save::SAVE_PERIOD_S` is a second of simulated time, so a machine
    //   drawing this zone at a fifth of real time writes exactly as often per
    //   second *of play*. Waiting for it to move is therefore a wait in beats
    //   rather than a wait in milliseconds.
    //
    // And the pair is what makes the reference reading exact rather than
    // approximate. `save::save_ticks` counts the autosave period in **ticks**
    // and at the default rate it is a whole number of `game::HEARTBEAT_TICKS`,
    // so a write and the line reporting it happen on the *same* tick —
    // `Shard::tick` autosaves and then logs, off one `Stats`.
    // `a_save_lands_on_a_heartbeat_at_the_default_rate` in
    // `apps/shard/src/save.rs` is what holds that, and it is why the tolerance
    // below is a rounding window rather than a guess: the heartbeat carrying a
    // raised `saves` carries the very state that was written, so this block
    // compares the resumed session against that line rather than against
    // whatever the page happened to be showing when it was asked.
    save: {
      // Whether this session opened from a save. Read off the *first* heartbeat
      // after a boot, which is a fixed simulated tick and so the same line on
      // every machine.
      resumed: /\bresumed: (yes|no)\b/,
      // How many times the character has been written out.
      writes: /\bsaves: (\d+)/,
      // The rest of the reading, shared with the blocks above: where they are,
      // what they have left, how many times they were put down, and how many
      // foes are still standing.
      along: /\bpz: (-?[\d.]+)/,
      health: /\bhp: (\d+)/,
      downs: /\bdowns: (\d+)/,
      alive: /\bfoes: (\d+)/,
      // What a *fresh* zone reads on all four, from `zone::LAYOUT`'s spawn and
      // `foe::HEALTH_MAX`/`foe::FOES`. These are the control's expectations, and
      // they are the reason the resume check has anything to say: a build that
      // always started in the same place reports exactly this after a reload
      // too.
      spawnAlong: 6.0,
      full: 100,
      count: 3,
      // How far off the spawn a restored position has to be before it is a
      // reading a fresh boot could not have produced, in metres. The zone block
      // above walks the character at least `WALK_ADVANCE_M` and the fight block
      // walks them the length of the corridor, so this is a wide margin on what
      // has actually happened by the time this block runs.
      awayFromSpawn: 1.0,
      // How far the restored position may sit from the one the save's own
      // heartbeat reported, in metres. A rounding window and nothing more —
      // that line is printed to two decimal places and, per the note above, it
      // is the same tick's reading as the bytes on the disk. Observed
      // difference on this machine: zero, twice.
      tolerance: 0.05,
      // What `crcbl-store`'s OPFS backend calls the file on the disk:
      // `save::SAVE_FILE` with the generation suffix `crates/crcbl-store/src/web/opfs.rs`
      // appends. Both slots are legal — the ping-pong writes to whichever the
      // last generation is not in.
      files: ['character.crb~0', 'character.crb~1'],
      // The two magics a real file on the disk carries, read out of its bytes.
      // `CRWB` at offset 0 is the OPFS record frame
      // (`crates/crcbl-store/src/web/opfs.rs`), and `CRCBLSVE` at the end of
      // that 52-byte header is the save container
      // (`crates/crcbl-store/src/save.rs`). Both, because either alone would
      // pass for half a write: a frame with nothing in it, or a container the
      // shim never framed.
      recordMagic: 'CRWB',
      frameHeader: 52,
      magic: 'CRCBLSVE',
    },
  },
  orbit: {
    key: null,
    // Read off the *first* line, which is the ship still on the pad — so it
    // cannot ask about the ascent. What it asks instead is that `apo:` is a
    // number. A body at rest is at its own apoapsis, and the formula that used
    // to compute one divided the semi-latus rectum by `1 - e`, which for a
    // motionless ship is `0 / 0`: the readout came back `NaN`, and the flight
    // computer that compared against it concluded it had already arrived and
    // shut the engine down. A `NaN` reads as a number everywhere downstream,
    // so the place to catch it is where it is printed.
    waiting: (line) => line.includes('[HUD] tick:') && /\bapo: -?\d/.test(line),
    moving: /alt: (-?[\d.]+)/,
    movingLabel: 'the rocket climbs under its own steam',
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
 * How long the backdrop check waits between two of those samples.
 *
 * Without it the eight `toDataURL` calls span a few milliseconds and are eight
 * looks at **one** frame, which is no better than a single sample. Scaled by the
 * measured slowdown at the call site, like every other budget here.
 */
const BACKDROP_INTERVAL_MS = 120;

/**
 * How long the backdrop check gives the demo to be *playing* before it reads the
 * clear, and how many times it presses the demo's own start key to get there.
 *
 * A game decides for itself what covers its clear colour, and by this point in
 * the run the demo has been played: flappy's bird is usually dead, and its death
 * screen dims the whole sky. That is not a hypothetical —
 * `[HUD] Dead score: 0` was printed beside `rgb(63,105,141)` on every one of six
 * consecutive samples here, and the Pages run of 2026-08-20 went red on exactly
 * that frame while nothing was wrong with the encode.
 */
const BACKDROP_PLAY_MS = 4_000;
const BACKDROP_PLAY_ATTEMPTS = 3;

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
 * How far the character must get past where it started, in metres, before the
 * walk key is credited with having reached the controller.
 *
 * The demos that drive `crcbl::phys::CharacterController` all walk at a few
 * metres a second — `apps/puppet` at 3.2, `apps/breach` at 3.4, `apps/shard` at
 * 4.2 — and all log a heartbeat every simulated second or half-second, so one
 * beat under a held key is at least a metre and a half. This is comfortably
 * inside one beat and far outside anything a settling capsule could drift, for
 * any of them.
 */
const WALK_ADVANCE_M = 1.0;

/**
 * How many consecutive heartbeats must report the *same* position before the
 * character counts as stopped.
 *
 * Two would do it — the demo's readings move several metres a beat — and three
 * is what makes it a claim about staying stopped rather than about one line.
 */
const WALK_STILL_BEATS = 3;

/**
 * How long a look key is held for the first nudge of the view, in milliseconds,
 * and how many times that is doubled before the nudge is given up on.
 *
 * **Nudged rather than held until the heartbeat notices.** `apps/breach` turns
 * at over a radian a second and logs one heartbeat a second, so a key held
 * until a new angle is reported has already spun the player most of the way
 * round — and a demo left facing a blank ceiling fails group D's "the canvas
 * changes between frames" for a reason that is the driver's doing rather than
 * the demo's.
 *
 * **And doubled rather than fixed**, because a look key is read on the frame
 * and a software rasteriser under load draws frames a second apart: a hold
 * shorter than one frame is a press and a release the demo sees in the same
 * breath, which is nothing held at all. Doubling makes the shortest hold that
 * works the one that gets used, on a fast machine and a slow one.
 *
 * **The ladder starts where it used to reach, and is three rungs rather than
 * five.** Every rung that fails costs its own hold *and* a heartbeat to find
 * out, and the rungs under half a second are the ones that were always going to
 * fail on the machine the ladder exists for — a frame there is longer than they
 * are. Starting at 500 ms lands in one round on every machine measured, and
 * caps the worst case at 3.5 s of holds instead of 6.2 s and three heartbeats
 * instead of five. This is the constant the Pages step that hit a ten-minute cap
 * spent its minutes in.
 */
const LOOK_NUDGE_MS = 500;
const LOOK_NUDGE_ROUNDS = 3;

/**
 * How far the view must have swung, in radians, for a nudge to count as having
 * landed — and for a look key to be credited with turning the view.
 *
 * Well under what one nudge buys and far above the nothing a view that ignored
 * the key would move.
 */
const LOOK_NUDGE_RAD = 0.1;

/**
 * How square the view must be left before the generic groups judge the canvas,
 * in radians, and how many rounds are spent getting it there.
 *
 * The travelling target is the only thing in this room that moves on its own,
 * so a driver that walked away leaving the camera pointed at a wall would hand
 * group D a still picture of a demo that is running perfectly well. Half a
 * radian is loose on purpose: the far lane only has to be somewhere in a frame
 * a good deal wider than that, and a tighter tolerance would be a driver
 * chasing a number rather than a demo pointing at its range.
 */
const LOOK_SQUARE_RAD = 0.5;
const LOOK_SQUARE_ROUNDS = 4;

/**
 * How far a foot height may sit from the step it is standing on, in metres.
 *
 * A settled capsule rests one `CharacterConfig::skin_width` above its ground —
 * a centimetre — and the readings are printed to two decimal places, so this is
 * five times the gap that is actually expected. It is nowhere near the distance
 * between the two steps the check tells apart, which is 0.6 m.
 */
const WALK_STEP_TOLERANCE_M = 0.05;

/**
 * How near the walk end of puppet's locomotion set the blend weight must get
 * while the character is walking.
 *
 * `apps/puppet/src/anim.rs` puts the walk stop at the speed the clip is
 * authored for, which is under the speed the controller commands — so a walking
 * character saturates the set at exactly 1. This leaves room for the beat the
 * reading was taken on to have landed during a turn.
 */
const BLEND_WALK_MIN = 0.9;

/**
 * And how near the idle end it must come back to once the key is released.
 *
 * `apps/puppet/src/game.rs`'s `STANDING_SPEED` deadband makes a stopped
 * character report exactly zero, so the weight lands on 0.00 rather than
 * approaching it; this is the printed precision and nothing more.
 */
const BLEND_IDLE_MAX = 0.05;

/**
 * How many times shard's canvas is sampled in each of the torch block's three
 * phases, and how far apart, in milliseconds.
 *
 * The gap is small because the sampling is not: `toDataURL` on this canvas,
 * decoded back and counted, measured about 0.7 s a sample on the software
 * rasteriser the gate runs on, so five samples span some four seconds of the
 * demo whatever this number is. Four rather than more because each one costs
 * that 0.7 s three times over — this block runs a lit window, a doused one and a
 * lit one again — and a browser gate that spends minutes learning nothing is
 * what `docs/plan/sample/11-breach.md`'s ten-minute timeout was made of.
 *
 * Four rather than fewer because of what the window has to contain: the shorter
 * of the two sine waves `apps/shard/src/light.rs` flickers on has a period of
 * 0.79 *simulated* seconds, and the demo runs at about a fifth of real time on
 * that rasteriser, so four samples span roughly two thirds of a cycle. That is
 * what the swing below was measured over.
 */
const TORCH_SAMPLES = 4;
const TORCH_SAMPLE_GAP_MS = 80;

/**
 * How far the canvas's mean luminance must swing across the lit window, and how
 * little it may swing across the doused one, in 0..255 bytes.
 *
 * **These are a measured pair and not a guess**, and the pair is what makes the
 * check non-vacuous: the same reading has to move for the lit zone and hold
 * still for the doused one, so a page drawing noise fails the second and a page
 * showing one frame for ever fails the first.
 *
 * Measured on the SwiftShader adapter this gate runs on, over five runs: the lit
 * windows swung between 0.12 and 0.80 of a byte, and every doused window swung
 * nothing at all — its samples were the same frame down to the last bit, so the
 * spread was exactly zero rather than merely small. The two thresholds are
 * therefore set either side of a gap with no reading anywhere in it, which is
 * the only way a pair like this is worth having.
 */
const TORCH_FLICKER_LUMA = 0.04;
const TORCH_STILL_LUMA = 0.01;

/**
 * How much of the doused canvas the frame must be *left* with, as a fraction of
 * the lit canvas's mean luminance — and how much of it one flat colour may
 * cover.
 *
 * **The first is the control for the control.** A build whose torch key merely
 * *froze* the flicker would hand this block a still frame with the heartbeat
 * still running and the picture still a picture, and pass everything else here;
 * what it cannot do is get darker. Measured: the lit windows read 14.18 and
 * 14.01 of mean luminance and the doused one read 9.18, which is 0.65 of them —
 * the zone keeps its shrine spot and its baked irradiance volume when the
 * torches go out, so the drop is the torches' share of the frame rather than a
 * fade to nothing.
 *
 * **The second is the control for the stillness claim.** "The picture stopped
 * changing when the lights went out" has a cheap wrong explanation — the frame
 * went black, and a black frame is trivially still — so the doused canvas has
 * to still be a *picture*: no single quantised colour covering more of it than
 * this. Measured at 0.53 with the torches out, against 0.39 with them lit.
 */
const TORCH_DARKER_RATIO = 0.95;
const TORCH_FLAT_SHARE = 0.85;

/**
 * How many particles sparks' switchable emitter must hold before its count
 * counts as having climbed.
 *
 * `apps/sparks/src/effects.rs`'s smoke puff emits 120 a second with a lifetime
 * of about a second, so a running emitter settles around a hundred; a stopped
 * one drains to exactly zero. This sits far above zero and far below the
 * steady state, so neither half of the pair can be satisfied by the other's
 * state.
 */
const PUFF_RUNNING_MIN = 20;

/**
 * How many consecutive `[POSE]` lines must report the same pose before the
 * character counts as having settled into its idle stance.
 *
 * The same argument as `WALK_STILL_BEATS`, on the other heartbeat: two lines
 * would do it and three makes it a claim about staying still.
 */
const POSE_STILL_BEATS = 3;

/**
 * The **floor** on how long the focus/pause group watches for a HUD heartbeat.
 *
 * Both samples log one every sixty ticks — a second of *simulated* time. Under
 * SwiftShader a frame is slow enough that the accumulator's 64 ms clamp makes
 * simulated time run behind wall time, so the window is several times that
 * second rather than a hair over it.
 *
 * **It used to be the whole window, and that was a constant tuned for one
 * machine.** On a GitHub runner quarry's frames are slow enough that a
 * heartbeat does not reliably land inside four seconds — measured 2026-08-20,
 * Pages run 32363651779 — so both of this group's heartbeat checks read
 * `0 HUD line(s) in 4000 ms`, and "a paused demo runs no ticks at all" passed
 * for free on a run where no heartbeat could appear in any state. Raising the
 * number would buy that runner and lose the next slower one, so the window is
 * derived from the demo's own observed beat instead — see [`heartbeatMs`] — and
 * this is only the lower bound, so no machine gets a *shorter* window than the
 * constant already gave it.
 */
const TICK_WINDOW_MS = 4_000;

/**
 * How many observed beats a watch window spans.
 *
 * Two rather than one because the phase is unknown: a window exactly one beat
 * long, started at an arbitrary moment, holds a beat only if the timing is
 * kind. Two guarantees one whichever way the phase falls, whatever it does to
 * the "paused" check's patience.
 */
const TICK_WINDOW_BEATS = 2;

/**
 * What one heartbeat is *supposed* to take, in wall-clock milliseconds.
 *
 * Most samples log one every sixtieth tick, and sixty ticks is a second of
 * simulated time — so on a machine keeping up, the beat and this number are the
 * same. The ratio between them is therefore how far behind real time this
 * machine is running the demo, which is the one factor every fixed budget in
 * this file needs and none of them had: they were all chosen on a desktop where
 * the ratio is 1.
 *
 * **A demo that logs on a different period says so**, through a `beatMs` row in
 * `EXPECTATIONS`: the ratio is only a measure of the machine if the numerator
 * and the denominator are the same interval, and a demo whose heartbeat is
 * twice as fast would otherwise report a machine twice as quick as it is and
 * shrink every budget scaled by it.
 */
const NOMINAL_BEAT_MS = 1_000;

/**
 * How long the run waits for two heartbeats before calling the tick loop dead.
 *
 * Generous, because the whole point is that this machine's pace is unknown; the
 * check it feeds is what fails when nothing arrives, so an over-long deadline
 * costs time on a broken run and never turns a red one green. Bounded by
 * `TIMEOUT_MS`, so `--timeout` still governs the run.
 */
const HEARTBEAT_DEADLINE_MS = 60_000;

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
 * How many pointer movements the turntable-stopping drag sends, and how far
 * apart in CSS pixels. Named apart from `DRAG_STEPS`, which is group F's touch
 * drag and answers a different question.
 *
 * A press alone is not a drag — `apps/viewer/src/controls.rs` needs movement
 * while a button is held — so there has to be more than one, and the viewer
 * latches its turntable off on the first one that arrives. They are small on
 * purpose: the drag orbits the camera, and a large one can swing the model off
 * screen entirely, which would stop the canvas changing for a reason that has
 * nothing to do with the geometry the check is about.
 */
const TURNTABLE_DRAG_STEPS = 4;
/** @see TURNTABLE_DRAG_STEPS */
const TURNTABLE_DRAG_STEP_PX = 2;

/**
 * How many heartbeats the deform check watches for, once the camera is still.
 *
 * Beats rather than milliseconds, so the window is this machine's rather than a
 * desktop's: a runner that takes four wall seconds to log a simulated one gets
 * four times as long to show a change, which is the direction a check watching
 * for motion has to scale in. Three of them span more than one cycle of the demo
 * document's clip — `apps/viewer/src/demo_model.rs`'s `CLIP_TIMES` ends before
 * two simulated seconds — so the pose sweeps rather than being sampled at one
 * phase.
 */
const DEFORM_BEATS = 3;

/**
 * How much of the deform check's sampling has to be a frame it has not seen
 * before.
 *
 * **A canvas that takes two values is not a canvas that is changing**, and the
 * difference is the whole check. Geometry following a clip in front of a still
 * camera gives a new picture almost every sample; a mesh whose vertices are
 * fixed gives one, and a mesh drawn alternately out of two fixed runs gives
 * exactly two however long the window is — 2 out of 25 on the run this constant
 * was written for. A half is nowhere near either.
 *
 * **The passing side of this is not measured**, because nothing has yet drawn
 * this document deformed; `docs/backlog.md` carries that as a gap. If a working
 * run ever lands near this share rather than near one, the number to look at is
 * how many frames the page presents per sample, not this.
 */
const DEFORM_CHANGING_SHARE = 0.5;

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
 *
 * **This is the value at a slowdown of 1**, and group F scales it: see `budget`.
 * The bare constant expired on a GitHub runner before the two contacts it waits
 * for were logged, which is what put every fixed budget in this file under the
 * measured beat.
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

/**
 * The string every one of group H's deliberate provocations carries.
 *
 * One string in three places — thrown out of a timer, asked of the server as a
 * file name, and hung on a buffer's label — so each of that group's checks can
 * insist the channel reported *its* provocation rather than something that
 * happened to arrive alongside it. A channel that reports the wrong thing would
 * not have reported the real failure either.
 *
 * It is also what the failure report at the bottom of this file matches on, to
 * keep this group's own device error out of a dump of real ones.
 */
const PROVOCATION = 'crcbl-e2e-deliberate';

/**
 * How long each of group H's provocations is given to come back.
 *
 * A deadline on a poll and not a sleep, like every other window here. Each
 * provocation is a single asynchronous hop — a timer firing, a request being
 * answered, an `uncapturederror` being dispatched — so this is generous by
 * orders of magnitude; the slowest of the three was measured at single-digit
 * milliseconds, and the check that measured it says so. What the window buys is
 * that a channel which is *closed* says so in seconds rather than spending the
 * run's whole `--timeout` proving a negative.
 */
const PROVOCATION_MS = 5_000;

/**
 * The substring every backend's teardown-leak warning carries.
 *
 * `crcbl-vk`, `crcbl-dx12` and `crcbl-mtl` write it from their device's
 * destructor and `crcbl-webgpu` writes it from `Replayer#replay` when the
 * command stream ends; `crates/crcbl-vk/tests/run-vk-e2e.sh`,
 * `crates/crcbl-shell/tests/run-x11-e2e.sh`, `run-wayland-e2e.sh`,
 * `tools/run-samples-windowed.sh` and `web/run-browser-e2e.sh` each fail a run
 * on it. One literal so that one grep covers all four backends — which is why
 * it is a constant here rather than a phrase typed into a filter.
 */
const TEARDOWN_LEAK = 'object(s) still alive at device teardown';

/**
 * Whether a 404 is the *page* asking for an asset it did not get.
 *
 * The pages declare `/favicon.svg`, which exists, so this is the belt to that
 * braces: a browser that ignores the declaration and asks for `/favicon.ico`
 * anyway is not the page wanting an asset. Every other 404 is.
 *
 * Named rather than written inline at group B's check because group H's control
 * has to be a miss this filter **cannot** swallow, and the only way to be sure
 * of that is to run the control's own path back through the same predicate.
 */
const isRealMiss = (path) => !path.endsWith('favicon.ico');

// `stopEverything` and the exit hooks that call it are in
// `web/tools/browser-launch.mjs`, with the launch that registers each browser.
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

/**
 * Polls until the condition holds, on this gate's own `--timeout` by default.
 *
 * The poll itself is shared — see `until` in `web/tools/browser-launch.mjs`,
 * which takes its deadline rather than keeping one.
 */
const until = (probe, timeout = TIMEOUT_MS) => pollUntil(probe, timeout);

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

// `findBrowser`, the launch, the CDP client and the kill are all in
// `web/tools/browser-launch.mjs`: the three gates in this directory need the
// same browser started the same way and driven the same way, and every
// platform-specific thing about doing that — where the binary lives, which
// flags name the real device, how to take a process tree down — is decided
// there once. What stays here is the part this gate alone owns: the window size
// its pixel counts depend on, and the adapter mode its readback control picks.

/**
 * Starts the browser this gate needs.
 *
 * The window size is fixed because the canvas is sized by CSS against the
 * viewport, so a fixed window makes this gate's pixel counts mean the same
 * thing on every machine. `mode` is whichever adapter the preflight is asking
 * about.
 */
const launch = (binary, mode) =>
  launchBrowser({
    binary,
    mode,
    profilePrefix: 'crcbl-web-e2e-',
    extra: ['--window-size=1024,768'],
    fail,
  });

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
 *
 * `luma` is the exception to that quantisation, and it is the *unquantised*
 * mean over every pixel: Rec. 709 relative luminance of the bytes as they came
 * back, averaged. It is what shard's torch block reads, and it exists because
 * `hash` and `distinct` can only say a frame differs, never which way — "the
 * room got darker when the lights went out" is a claim about a magnitude, and a
 * hash cannot make it. Being unquantised is deliberate for the other half of
 * that block: a five-bit shift throws away exactly the small, smooth brightness
 * swings a flickering torch produces on a wall.
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
  let light = 0;
  for (let i = 0; i < pixels.length; i += 4) {
    const key = ((pixels[i] >> 3) << 10) | ((pixels[i + 1] >> 3) << 5) | (pixels[i + 2] >> 3);
    histogram.set(key, (histogram.get(key) ?? 0) + 1);
    hash = Math.imul(hash ^ pixels[i], 16777619);
    hash = Math.imul(hash ^ pixels[i + 1], 16777619);
    hash = Math.imul(hash ^ pixels[i + 2], 16777619);
    light += 0.2126 * pixels[i] + 0.7152 * pixels[i + 1] + 0.0722 * pixels[i + 2];
  }
  const ranked = [...histogram.entries()].sort((a, b) => b[1] - a[1]);
  const total = pixels.length / 4;
  return {
    width: scratch.width,
    height: scratch.height,
    distinct: histogram.size,
    hash: hash >>> 0,
    luma: light / total,
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

/** @type {{ group: string, name: string, ok: boolean, detail: string, ms: number }[]} */
const checks = [];

/**
 * When the last check was recorded, so the next one can say how long it took.
 *
 * **This exists because a browser gate's cost is a CI failure of its own.** The
 * Pages run of 2026-08-26 lost breach's step to the ten-minute cap while every
 * assertion in it was green, and working out *which* wait had grown meant
 * guessing — every budget in this file is scaled by a measured slowdown, so the
 * expensive one is not the one with the largest constant. A per-check elapsed,
 * summarised at the end, turns that into a reading.
 */
let checkedAt = Date.now();

function check(group, name, ok, detail = '') {
  const now = Date.now();
  checks.push({ group, name, ok: Boolean(ok), detail, ms: now - checkedAt });
  checkedAt = now;
  console.log(
    `  ${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? ` — ${detail}` : ''}`
  );
  return Boolean(ok);
}

/** How many of the slowest checks the verdict names. */
const SLOWEST_REPORTED = 5;

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
  // [`isRealMiss`] says which 404s are the page's own; group H's control is a
  // path chosen so that predicate cannot drop it.
  const missing = site.misses.filter(isRealMiss);
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
  /**
   * Where the focus click lands, with the canvas scrolled into view first.
   *
   * A function rather than a value read once, because a navigation replaces the
   * document: the element an old rect described is gone, and so is the focus it
   * had.
   */
  const focusPoint = async () =>
    evaluate(
      page,
      `(() => { const c = document.getElementById('canvas');
              c.scrollIntoView({ block: 'center', behavior: 'instant' });
              const r = c.getBoundingClientRect();
              return { x: Math.round(r.x + ${FOCUS_CLICK_INSET}), y: Math.round(r.y + ${FOCUS_CLICK_INSET}),
                       left: r.x, top: r.y }; })()`
    );
  /** One real click at `at`, through the browser's own input pipeline. */
  const clickAt = async (/** @type {{x: number, y: number}} */ at) => {
    for (const type of ['mousePressed', 'mouseReleased']) {
      await page.send('Input.dispatchMouseEvent', {
        type,
        x: at.x,
        y: at.y,
        button: 'left',
        clickCount: 1,
        buttons: type === 'mousePressed' ? 1 : 0,
      });
    }
  };

  const rect = await focusPoint();
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
  await clickAt(rect);
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
  // The key the demo's own instructions name. `code` is what the engine binds
  // to; `key` and the virtual key codes are what a real keyboard sends — and
  // they are spelled for Space, which is what every keyed row asks for. A row
  // naming a different key has to bring its own three values with it.
  //
  // A named helper rather than an inline loop because group G presses it too, to
  // put the demo back in play before reading its clear colour.
  const pressStartKey = async () => {
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
  };

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

    await pressStartKey();

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
  /** Every distinct value `pattern` has captured on a HUD line since the start. */
  const valuesOf = (/** @type {RegExp} */ pattern) =>
    new Set(
      hud()
        .slice(beforeLaunch)
        .map((line) => line.match(pattern)?.[1])
        .filter(Boolean)
    );
  const values = () => valuesOf(EXPECTED.moving);
  const positions = await until(async () => {
    const seen = values();
    return seen.size > 1 ? seen : null;
  });
  // A failure here has two quite different causes and one of them is not about
  // the value at all: a demo whose loop stopped logs no second heartbeat, and a
  // demo that is running while the number is stuck logs many. "It never
  // changed" reads as the second and is usually the first, so the report counts
  // the lines as well as the values — see `apps/viewer`'s intermittent macOS
  // stall in `docs/backlog.md`, which was diagnosed off exactly this number.
  const beats = hud().length - beforeLaunch;
  // **And, where the demo reports one, whether its camera was taken hold of.**
  // A stuck number with the loop still running has two causes that want
  // opposite investigations — the loop stopped feeding the camera, or something
  // stopped the camera on purpose — and only the demo knows which. `viewer`
  // prints a `held` row for exactly this; a demo that prints none adds nothing
  // to the message, which is why this is a suffix and not a field.
  const heldNote = () => {
    const seen = new Set(
      hud()
        .map((line) => line.match(/\bheld: (\w+)/)?.[1])
        .filter(Boolean)
    );
    return seen.size === 0 ? '' : `, with held: ${[...seen].join(', ')}`;
  };
  check(
    'C',
    EXPECTED.movingLabel,
    Boolean(positions),
    positions
      ? `it took ${positions.size} values: ${[...positions].join(', ')}`
      : `it never changed — ${beats} HUD line(s) since the start, ` +
          `${values().size} value(s): ${[...values()].join(', ') || 'none'}` +
          heldNote()
  );

  // **AND THE CONTROLLER BEING DRIVEN, WHICH THE CHECK ABOVE CANNOT SEE.**
  // Only puppet has one. `moving` above says the simulation is advancing, and
  // it would go on saying so for a demo whose input path is severed: that
  // sample walks a circuit of its own until somebody takes the controls, and a
  // page that dropped every key would pace it for ever.
  //
  // So this block drives the demo's own walk key through CDP and reads what the
  // character did, in four claims that are two pairs:
  //
  // * it **advances** while the key is held, and **stops** when it is released.
  //   The second is the control for the first: a demo that drifts passes "a
  //   number changed" and fails this.
  // * it **climbs** the step under the controller's step offset, and is
  //   **refused** by the one over it. The second is the control for the first:
  //   a controller that stepped over anything at all would pass the climb and
  //   fail the refusal, and one that climbed nothing would fail the climb while
  //   passing the refusal for the wrong reason.
  //
  // Every number it asserts on comes from `apps/puppet/src/map.rs`, and the
  // failure messages carry the readings, so a red run here says which of the
  // four went wrong and at what value.
  if (EXPECTED.walk) {
    const walk = EXPECTED.walk;

    /** The most recent value `pattern` captured on a HUD line, as a number. */
    const latest = (/** @type {RegExp} */ pattern) => {
      const lines = hud();
      for (let at = lines.length - 1; at >= 0; at -= 1) {
        const found = lines[at].match(pattern);
        if (found) return Number(found[1]);
      }
      return null;
    };
    /** Every value `pattern` has captured on the HUD lines from `since` on. */
    const since = (/** @type {RegExp} */ pattern, /** @type {number} */ from) =>
      hud()
        .slice(from)
        .map((line) => line.match(pattern)?.[1])
        .filter((value) => value !== undefined);
    /**
     * The `[POSE]` lines — the client's heartbeat, which carries what the
     * locomotion blend did. A second stream rather than more terms on the
     * `[HUD]` line, because the two are logged off different clocks; see the
     * `puppet` row above and `apps/puppet/src/lib.rs`.
     */
    const pose = () => consoleLines.filter((line) => line.includes('[POSE]'));
    /** The most recent value `pattern` captured on a `[POSE]` line. */
    const lastPose = (/** @type {RegExp} */ pattern) => {
      const lines = pose();
      for (let at = lines.length - 1; at >= 0; at -= 1) {
        const found = lines[at].match(pattern);
        if (found) return Number(found[1]);
      }
      return null;
    };
    /** Every value `pattern` has captured on the `[POSE]` lines from `from` on. */
    const posesSince = (
      /** @type {RegExp} */ pattern,
      /** @type {number} */ from
    ) =>
      pose()
        .slice(from)
        .map((line) => line.match(pattern)?.[1])
        .filter((value) => value !== undefined);

    /** Presses or releases the walk key, through the browser's own pipeline. */
    const walkKey = async (/** @type {string} */ type) =>
      page.send('Input.dispatchKeyEvent', {
        type,
        code: walk.code,
        key: walk.text,
        windowsVirtualKeyCode: walk.virtualKeyCode,
        nativeVirtualKeyCode: walk.virtualKeyCode,
        ...(type === 'keyDown' ? { text: walk.text } : {}),
      });

    // ---- the pair about input reaching the controller at all ----------------
    const startedAt = latest(walk.advance);
    await walkKey('keyDown');
    const advanced = await until(async () => {
      const now = latest(walk.advance);
      return startedAt !== null &&
        now !== null &&
        startedAt - now >= WALK_ADVANCE_M
        ? now
        : null;
    });
    check(
      'C',
      'the character advances while the walk key is held',
      advanced !== null,
      advanced === null
        ? `it started at ${startedAt} and never got ${WALK_ADVANCE_M} m past it — ` +
            `last reading ${latest(walk.advance)} over ${hud().length} HUD line(s)`
        : `it walked from ${startedAt} to ${advanced}`
    );

    // **It has to have still been moving when the key came up**, or "it
    // stopped" is a claim about a character the map had already stopped. The
    // two lines before the release are both under a held key, so a pair of
    // equal readings there is the vacuous case and this is what catches it.
    const atRelease = hud().length;
    const poseAtRelease = pose().length;
    const blendHeld = lastPose(walk.blend);
    const crossingsHeld = lastPose(walk.crossings);
    await walkKey('keyUp');
    const lastHeld = hud()[atRelease - 1]?.match(walk.advance)?.[1];
    const priorHeld = hud()[atRelease - 2]?.match(walk.advance)?.[1];
    const stillMoving = Boolean(
      lastHeld && priorHeld && lastHeld !== priorHeld
    );

    const settled = await until(async () => {
      const readings = since(walk.advance, atRelease);
      if (readings.length < WALK_STILL_BEATS) return null;
      const tail = readings.slice(-WALK_STILL_BEATS);
      return tail.every((value) => value === tail[0]) ? tail[0] : null;
    });
    check(
      'C',
      'and stops where it is when the key is released',
      Boolean(settled) && stillMoving,
      !stillMoving
        ? `it was already standing still before the release (${priorHeld} then ` +
            `${lastHeld}), so this check would pass on a demo that never moved`
        : settled
          ? `it held ${settled} for ${WALK_STILL_BEATS} beats after the release`
          : `it kept moving with nothing held: ${since(walk.advance, atRelease).join(', ')}`
    );

    // ---- the pair about the blend the measured speed selects ----------------
    // **The one thing on this page that only a blend can produce.** Every check
    // above passes against a demo drawing a rigid capsule: they are about where
    // the character is, and none of them can see what it is posed as. These two
    // are about the weight between the idle stance and the walk, and they are a
    // pair for the usual reason — "it reached the walk" passes for a demo that
    // is *always* at the walk, and the control is what says it came back.
    //
    // The control carries the anti-snap claim as well, through `mid`: the
    // counter has to have risen across the release, which is the demo reporting
    // that the weight spent frames strictly between the two stops on its way
    // down. A blend that jumped would arrive at the same 0.00 with the counter
    // where it was.
    check(
      'C',
      'the blend weight follows the measured speed to the walk end',
      blendHeld !== null && blendHeld >= BLEND_WALK_MIN,
      blendHeld === null
        ? `the demo never reported a blend weight — ${pose().length} [POSE] line(s)`
        : `it was at ${blendHeld} while the key was held, of a walk end at 1 ` +
            `(over ${pose().length} [POSE] line(s))`
    );

    const idled = await until(async () => {
      const weight = lastPose(walk.blend);
      const crossings = lastPose(walk.crossings);
      return weight !== null && crossings !== null && weight <= BLEND_IDLE_MAX
        ? { weight, crossings }
        : null;
    });
    const swept =
      idled !== null &&
      crossingsHeld !== null &&
      idled.crossings > crossingsHeld;
    check(
      'C',
      'and sweeps back to the idle end rather than snapping to it',
      Boolean(idled) && swept,
      idled === null
        ? `it never came back under ${BLEND_IDLE_MAX}; last weight ` +
            `${lastPose(walk.blend)} over ${pose().length} [POSE] line(s)`
        : swept
          ? `it fell to ${idled.weight}, spending ` +
            `${idled.crossings - crossingsHeld} frame(s) between the two stops`
          : `it reached ${idled.weight} with the between-the-stops counter still at ` +
            `${idled.crossings}, so the weight jumped rather than swept`
    );

    // ---- and the pair about the pose that weight selects --------------------
    // `blend` is a number the demo computes; `dev` is what the rig actually did
    // with it, so this is the half that fails for a palette nothing composed.
    const walkedPoses = new Set(
      posesSince(walk.deviation, 0).slice(0, poseAtRelease)
    );
    check(
      'C',
      'the character is posed by its own clip while it walks',
      walkedPoses.size > 1,
      walkedPoses.size > 1
        ? `it took ${walkedPoses.size} poses: ${[...walkedPoses].join(', ')}`
        : `it never changed pose across ${poseAtRelease} [POSE] line(s): ` +
            `${[...walkedPoses].join(', ') || 'none'}`
    );

    const stance = await until(async () => {
      const readings = posesSince(walk.deviation, poseAtRelease);
      if (readings.length < POSE_STILL_BEATS) return null;
      const tail = readings.slice(-POSE_STILL_BEATS);
      return tail.every((value) => value === tail[0]) ? tail[0] : null;
    });
    // Non-zero as well as still: `apps/puppet/src/rig.rs`'s idle is a *stance*
    // and not the rest pose, so a demo that composed no palette at all would
    // hold 0.000 for ever and pass a check that only asked for stillness.
    const posedStance = stance !== null && Number(stance) > 0;
    check(
      'C',
      'and settles into its idle stance when it stands',
      posedStance,
      stance === null
        ? `it kept moving with nothing held: ` +
            `${posesSince(walk.deviation, poseAtRelease).join(', ')}`
        : posedStance
          ? `it held ${stance} m off the rest pose for ${POSE_STILL_BEATS} beats`
          : `it settled on ${stance}, which is the rest pose — nothing posed the rig`
    );

    // ---- the pair about what the controller decided -------------------------
    const climbedBefore = latest(walk.climbed) ?? 0;
    const blockedBefore = latest(walk.blocked) ?? 0;
    await walkKey('keyDown');
    const lane = await until(async () => {
      const climbed = latest(walk.climbed);
      const blocked = latest(walk.blocked);
      const feet = latest(walk.feet);
      const highest = latest(walk.highest);
      if (
        climbed === null ||
        blocked === null ||
        feet === null ||
        highest === null
      ) {
        return null;
      }
      return climbed > climbedBefore &&
        blocked > blockedBefore &&
        Math.abs(feet - walk.lowStep) <= WALK_STEP_TOLERANCE_M
        ? { climbed, blocked, feet, highest }
        : null;
    });
    await walkKey('keyUp');
    const reading = lane ?? {
      climbed: latest(walk.climbed),
      blocked: latest(walk.blocked),
      feet: latest(walk.feet),
      highest: latest(walk.highest),
    };
    const readingLine =
      `climbed ${reading.climbed}, blocked ${reading.blocked}, ` +
      `feet ${reading.feet} m, highest ${reading.highest} m`;
    check(
      'C',
      'the controller climbs the step inside its own step offset',
      Boolean(lane),
      lane
        ? `it stepped up onto ${walk.lowStep} m and is pushing at the next riser — ${readingLine}`
        : `it never got onto the ${walk.lowStep} m step while pushing — ${readingLine}`
    );

    // The control, and it is a claim about the **whole run**: `highest` is a
    // record rather than a reading, so a character that got onto the taller
    // step at any point in this session cannot hide it by having come back down.
    const top = reading.highest;
    check(
      'C',
      'and is refused by the step above that offset',
      top !== null &&
        top < walk.highStep - WALK_STEP_TOLERANCE_M &&
        Math.abs(top - walk.lowStep) <= WALK_STEP_TOLERANCE_M,
      top === null
        ? 'the demo never reported how high its feet had been'
        : `the highest its feet reached was ${top} m; the step it climbed is ` +
            `${walk.lowStep} m and the one it must not is ${walk.highStep} m`
    );
  }

  // **AND THE FIRST-PERSON HALF OF THE SAME CLAIM.** Only breach has one.
  // `moving` above reads a plate that travels its lane on a timer, so it says
  // the loop is running and would go on saying so for a page whose input path
  // is severed — nothing a player does can move that number, which is exactly
  // what makes it a good liveness check and a useless input check.
  //
  // So this block drives the demo's own keys and reads what they did, in three
  // pairs, each a positive and the control that stops it passing for the wrong
  // reason:
  //
  // * the player **advances** while the walk key is held, and **stops** when it
  //   is released, **short of the firing line**. A demo that drifts passes "a
  //   number changed" and fails the first half; a demo that never reads a key
  //   release walks to the line the controller will not climb and stands there
  //   perfectly still, which passes the second half until the third clause is
  //   there to ask where it stopped.
  // * a shot with a plate in the crosshair **scores**, and a shot aimed away
  //   from every plate **does not**. A build that scored on every trigger pull
  //   passes the first and fails the second.
  // * a look key **turns the view**, and the view **was standing still before
  //   it was pressed**. Without the control, "the yaw took two values" passes
  //   for the range's own warm-up, which sweeps its aim between the lanes with
  //   nobody touching anything.
  //
  // Both shots are aimed by driving the demo's own look input and reading the
  // `aim` field back, rather than by reaching past it into the simulation — so
  // what the gate proves is that a player can aim, not that a test can.
  if (EXPECTED.range) {
    const range = EXPECTED.range;

    /** The most recent value `pattern` captured on a HUD line, as a number. */
    const latest = (/** @type {RegExp} */ pattern) => {
      const lines = hud();
      for (let at = lines.length - 1; at >= 0; at -= 1) {
        const found = lines[at].match(pattern);
        if (found) return Number(found[1]);
      }
      return null;
    };
    /** The most recent value `pattern` captured, as the word it is. */
    const word = (/** @type {RegExp} */ pattern) => {
      const lines = hud();
      for (let at = lines.length - 1; at >= 0; at -= 1) {
        const found = lines[at].match(pattern);
        if (found) return found[1];
      }
      return null;
    };
    /** Every value `pattern` has captured on the HUD lines from `from` on. */
    const since = (/** @type {RegExp} */ pattern, /** @type {number} */ from) =>
      hud()
        .slice(from)
        .map((line) => line.match(pattern)?.[1])
        .filter((value) => value !== undefined);
    /**
     * Presses or releases one of this demo's keys, through the browser.
     *
     * `key` and `text` are not the same field and cannot be folded into one:
     * `key` is the DOM key value every binding has, and `text` is the
     * character the press types, which only a printable key has. Sending a
     * `text` of `'ArrowRight'` is rejected outright — `Invalid 'text'
     * parameter` — so a binding without a character omits it.
     */
    const key = async (
      /** @type {string} */ type,
      /** @type {{code: string, key: string, text?: string, virtualKeyCode: number}} */ binding
    ) =>
      page.send('Input.dispatchKeyEvent', {
        type,
        code: binding.code,
        key: binding.key,
        windowsVirtualKeyCode: binding.virtualKeyCode,
        nativeVirtualKeyCode: binding.virtualKeyCode,
        ...(type === 'keyDown' && binding.text !== undefined
          ? { text: binding.text }
          : {}),
      });
    /** One press and release — which for the trigger is one shot. */
    const tap = async (
      /** @type {{code: string, key: string, text?: string, virtualKeyCode: number}} */ binding
    ) => {
      await key('keyDown', binding);
      await key('keyUp', binding);
    };
    /** A key held for `ms`, which for a look key is a measured swing. */
    const nudge = async (
      /** @type {{code: string, key: string, text?: string, virtualKeyCode: number}} */ binding,
      /** @type {number} */ ms
    ) => {
      await key('keyDown', binding);
      await pause(ms);
      await key('keyUp', binding);
    };
    /** Waits for one heartbeat past `mark`, so a nudge can be read back. */
    const beat = async (/** @type {number} */ mark) =>
      until(async () => (hud().length > mark ? hud().length : null));
    /**
     * Holds a look key until the angle `pattern` reports has moved, doubling
     * the hold until it does.
     *
     * Both angles are read from heartbeats logged with nothing held — one
     * before the press and one after the release — so the swing and the time
     * it took cover the same interval and their ratio is the rate the view
     * turns at, which is what puts it back afterwards.
     */
    const swingBy = async (
      /** @type {{code: string, key: string, text?: string, virtualKeyCode: number}} */ binding,
      /** @type {RegExp} */ pattern
    ) => {
      let ms = LOOK_NUDGE_MS;
      for (let round = 0; round < LOOK_NUDGE_ROUNDS; round += 1) {
        const from = latest(pattern);
        await nudge(binding, ms);
        const mark = hud().length;
        await beat(mark);
        const to = latest(pattern);
        if (
          from !== null &&
          to !== null &&
          Math.abs(to - from) >= LOOK_NUDGE_RAD
        ) {
          return { from, to, ms };
        }
        ms *= 2;
      }
      return null;
    };
    /**
     * The first HUD line since `from` that reports more shots than `before`,
     * with the score it carried.
     *
     * Read off **one line** rather than by sampling the two counters
     * separately: a shot and the hit it did or did not score are one tick's
     * work, and two reads a poll apart could straddle the next trigger pull.
     */
    const shotAfter = async (
      /** @type {number} */ from,
      /** @type {number} */ before
    ) =>
      until(async () => {
        for (const line of hud().slice(from)) {
          const shots = Number(line.match(range.shots)?.[1]);
          const hits = Number(line.match(range.hits)?.[1]);
          if (
            Number.isFinite(shots) &&
            Number.isFinite(hits) &&
            shots > before
          ) {
            return { shots, hits, line };
          }
        }
        return null;
      });

    // ---- the pair about input reaching the controller at all ----------------
    const startedAt = latest(range.advance);
    await key('keyDown', range.walk);
    const advanced = await until(async () => {
      const now = latest(range.advance);
      return startedAt !== null &&
        now !== null &&
        startedAt - now >= WALK_ADVANCE_M
        ? now
        : null;
    });
    check(
      'C',
      'the player advances while the walk key is held',
      advanced !== null,
      advanced === null
        ? `they started at ${startedAt} and never got ${WALK_ADVANCE_M} m past it — ` +
            `last reading ${latest(range.advance)} over ${hud().length} HUD line(s)`
        : `they walked from ${startedAt} to ${advanced}`
    );

    // **They have to have still been moving when the key came up**, or "it
    // stopped" is a claim about a player the firing line had already stopped.
    // The two lines before the release are both under a held key, so a pair of
    // equal readings there is the vacuous case and this is what catches it.
    const atRelease = hud().length;
    await key('keyUp', range.walk);
    const lastHeld = hud()[atRelease - 1]?.match(range.advance)?.[1];
    const priorHeld = hud()[atRelease - 2]?.match(range.advance)?.[1];
    const stillMoving = Boolean(
      lastHeld && priorHeld && lastHeld !== priorHeld
    );

    const settled = await until(async () => {
      const readings = since(range.advance, atRelease);
      if (readings.length < WALK_STILL_BEATS) return null;
      const tail = readings.slice(-WALK_STILL_BEATS);
      return tail.every((value) => value === tail[0]) ? tail[0] : null;
    });
    const shortOfTheLine =
      settled !== null && Number(settled) > range.stopsShortOf;
    check(
      'C',
      'and stops where they are when the key is released',
      Boolean(settled) && stillMoving && shortOfTheLine,
      !stillMoving
        ? `they were already standing still before the release (${priorHeld} then ` +
            `${lastHeld}), so this check would pass on a demo that never moved`
        : settled === null
          ? `they kept moving with nothing held: ${since(range.advance, atRelease).join(', ')}`
          : shortOfTheLine
            ? `they held ${settled} for ${WALK_STILL_BEATS} beats after the release, ` +
              `${(Number(settled) - range.stopsShortOf).toFixed(2)} m short of where the ` +
              `firing line would have stopped them anyway`
            : `they stopped at ${settled}, which is on the firing line — the range ` +
              `stopped them, not the key coming up`
    );

    // ---- the pair about the pistol ------------------------------------------
    // The range squares the shooter up down the near lane when they step up to
    // it, and the walk above only moved them along that lane — so a plate is
    // already in the crosshair. Asserted rather than assumed, because it is
    // what makes the shot below deliberate: firing at whatever happened to be
    // there and calling a hit a hit would be the check passing for the wrong
    // reason.
    const aimed = await until(async () => {
      const at = word(range.aim);
      return at && at !== range.offTarget && at !== 'none' && at !== 'down'
        ? at
        : null;
    });
    check(
      'C',
      'a plate is in the crosshair before the shot',
      Boolean(aimed),
      aimed
        ? `the crosshair is on the ${aimed} lane`
        : `the crosshair reports "${word(range.aim) ?? 'nothing'}" — there is no ` +
            `target to shoot at, so a hit below would be an accident`
    );

    const shotsBeforeHit = latest(range.shots) ?? 0;
    const hitsBeforeHit = latest(range.hits) ?? 0;
    const hitMark = hud().length;
    await tap(range.fire);
    const scored = await shotAfter(hitMark, shotsBeforeHit);
    // **Two observables, not one.** The score is what the demo counted; the
    // plate going down is what happened to the range. A build that incremented
    // a counter and left the target standing passes the first and fails the
    // second, and it is the second a visitor would notice.
    const knocked =
      scored !== null && since(range.nearest, hitMark).includes('down');
    const hitScored = scored !== null && scored.hits > hitsBeforeHit;
    check(
      'C',
      'a shot at a plate scores a hit and knocks it down',
      hitScored && knocked,
      hitScored && knocked
        ? `${scored.line.trim()} — and the ${aimed} plate went down with it`
        : scored === null
          ? `no heartbeat after the shot reported more than ${shotsBeforeHit} shot(s) — ` +
            `the trigger never reached the simulation`
          : hitScored
            ? `${scored.line.trim()} — but the ${aimed} plate never went down: ` +
              `${since(range.nearest, hitMark).join(', ') || 'no readings'}`
            : `hits stayed at ${hitsBeforeHit} while shots went to ${scored.shots}: ` +
              `${scored.line.trim()}`
    );

    // ---- the pair about the view --------------------------------------------
    // The control first, and it is measured from the tick the player took the
    // range over: before that the warm-up is sweeping its own aim between the
    // lanes, so "the yaw was standing still" would be false there and this
    // check would be measuring the wrong thing.
    const tookOver = await until(async () => {
      const at = hud().findIndex(
        (line) => line.match(range.pilot)?.[1] === 'player'
      );
      return at >= 0 && hud().length - at >= WALK_STILL_BEATS ? at : null;
    });
    const held = tookOver === null ? [] : since(range.yaw, tookOver);
    const wasStill =
      held.length > 1 && held.every((value) => value === held[0]);

    const beforeTurn = hud().length;
    const turned = await swingBy(range.turn, range.yaw);
    check(
      'C',
      'a look key turns the view',
      turned !== null && wasStill,
      !wasStill
        ? `the view was already taking new angles with nothing held ` +
            `(${held.join(', ') || 'no readings'}), so this check would pass on a ` +
            `demo whose camera drifts`
        : turned !== null
          ? `${turned.ms} ms of the turn key swung it from ${turned.from} to ${turned.to}`
          : `the yaw never moved: ` +
            `${[...new Set(since(range.yaw, beforeTurn))].join(', ') || 'none'}`
    );

    // **The rate the view turns at, measured rather than assumed**, so the
    // view can be put back where it was without this file carrying a copy of
    // the demo's look speed — a constant that would go quietly wrong the day
    // somebody tuned it.
    const lookRate =
      turned === null
        ? null
        : Math.abs(turned.to - turned.from) / (turned.ms / 1000);
    /** Holds a look key for as long as `radians` of swing takes. */
    const swing = async (
      /** @type {{code: string, key: string, text?: string, virtualKeyCode: number}} */ binding,
      /** @type {number} */ radians
    ) => {
      if (!lookRate) return;
      // Floored at the hold that was measured to work here: a shorter one is a
      // press and a release inside a single frame, which turns nothing.
      const ms = Math.max(turned.ms, (1000 * radians) / lookRate);
      await nudge(binding, Math.min(4000, ms));
    };

    // ---- and the control for the shot ---------------------------------------
    // Aimed by tilting the view up a nudge at a time until three consecutive
    // heartbeats agree the crosshair is off every target — three, so the shot
    // cannot land on a frame that was about to sweep back onto one. A nudge at
    // a time rather than held to the pitch clamp, because the clamp is the
    // ceiling and a demo left staring at a ceiling has nothing moving in it.
    //
    // **The clamp is read, not broken out of.** `swingBy` returns null once the
    // view stops moving, and for a tilt that means the pitch clamp — which is
    // the ceiling, the one bearing on this map where no plate can ever be. The
    // first version treated that null as a dead end and gave up at exactly the
    // moment it had arrived, so a run that had to tilt the whole way failed
    // with the crosshair pointing somewhere it could not possibly hit a target.
    // That is what reddened `main` on `350c98d`: `the crosshair never came off
    // every target: range, near, far`, on a runner slow enough that the far
    // lane's travelling plate crossed the aim during the three-beat wait.
    let offTarget = null;
    let atTheClamp = false;
    for (
      let round = 0;
      round < LOOK_SQUARE_ROUNDS && !offTarget && !atTheClamp;
      round += 1
    ) {
      atTheClamp = (await swingBy(range.tilt, range.pitch)) === null;
      const beforeTilt = hud().length;
      await until(async () =>
        since(range.aim, beforeTilt).length >= WALK_STILL_BEATS ? true : null
      );
      const tail = since(range.aim, beforeTilt).slice(-WALK_STILL_BEATS);
      if (
        tail.length === WALK_STILL_BEATS &&
        tail.every((at) => at === range.offTarget)
      ) {
        offTarget = tail;
      }
    }
    const aimReadings = () => [
      ...new Set(hud().map((line) => line.match(range.aim)?.[1])),
    ];

    const shotsBeforeMiss = latest(range.shots) ?? 0;
    const hitsBeforeMiss = latest(range.hits) ?? 0;
    const missMark = hud().length;
    await tap(range.fire);
    const missed = await shotAfter(missMark, shotsBeforeMiss);
    check(
      'C',
      'and a shot aimed away from every plate does not score',
      offTarget !== null && missed !== null && missed.hits === hitsBeforeMiss,
      offTarget === null
        ? `the crosshair never came off every target: ` +
            `${aimReadings().join(', ') || 'no readings'}`
        : missed === null
          ? `no heartbeat after the shot reported more than ${shotsBeforeMiss} shot(s)`
          : `${missed.line.trim()} — hits went ${hitsBeforeMiss} → ${missed.hits}`
    );

    // ---- and the view goes back where the range is --------------------------
    // Not a check: it is what the groups below are entitled to, having been
    // handed a demo this block spent a minute aiming somewhere else. The only
    // thing in this room that moves on its own is the travelling target, and
    // "the canvas changes between frames" is a question about the demo, not
    // about where the last driver left the camera pointing.
    for (let round = 0; round < LOOK_SQUARE_ROUNDS; round += 1) {
      const yawNow = latest(range.yaw) ?? 0;
      const pitchNow = latest(range.pitch) ?? 0;
      if (
        Math.abs(yawNow) < LOOK_SQUARE_RAD &&
        Math.abs(pitchNow) < LOOK_SQUARE_RAD
      ) {
        break;
      }
      const mark = hud().length;
      await swing(yawNow > 0 ? range.turnBack : range.turn, Math.abs(yawNow));
      await swing(
        pitchNow > 0 ? range.tiltBack : range.tilt,
        Math.abs(pitchNow)
      );
      await beat(mark);
    }
  }

  // **AND THE OTHER MAP, WHICH IS MILESTONE 0'S OTHER HALF.** Only breach has
  // one. Everything above is the firing range; `docs/plan/sample/11-breach.md`
  // asks for a bot practice map beside it, "playable in a browser from the same
  // build that runs natively", and this block is the only thing anywhere that
  // opens it in a browser at all.
  //
  // It gets there the way a visitor would: `?map=practice`, which
  // `web/demos/breach/main.js` turns into `__crcbl_breach_map` before boot.
  // Navigating is the whole reset — the shim's `pagehide` teardown runs and the
  // next document starts from the top — and it is the same move group F makes
  // to get a page booted with touch on.
  //
  // Nothing here presses a key, and that is the point: three bots walk their
  // authored routes, one of them is in the open in front of the spawn, and it
  // shoots. So the three pairs below are about the simulation and not about the
  // input path the `range` block has already driven:
  //
  // * a bot **advances along its own patrol**, and the **player did not move
  //   and no travelling plate reported anything**. Without the control, "a
  //   number changed" passes for the other map's mover or for a demo that
  //   walked the player instead of a bot.
  // * a bot **notices the player**, and another **is in range and cannot**.
  //   The second is the control for the first: a build that noticed
  //   unconditionally passes the sighting and leaves `covered` at zero for
  //   ever, which is exactly the failure a sighting check on its own cannot
  //   tell from success. This is the pair that says
  //   `PhysicsWorld::cast_ray` is doing the work rather than a distance test.
  // * a bot's round **takes the player's health**, and a round with cover in
  //   the way **does not arrive**. Without the control, "health fell" passes
  //   for a build whose bots hit whatever they fired at, and the map's cover
  //   would be scenery.
  if (EXPECTED.practice) {
    const practice = EXPECTED.practice;
    const mapUrl = `${url}${practice.query}`;
    const before = hud().length;
    await page.send('Page.navigate', { url: mapUrl });
    await until(async () =>
      evaluate(page, `document.readyState === 'complete'`)
    );
    const rebooted = await until(async () => {
      const status = await evaluate(page, `crcbl.status()`);
      return status === 3 ? status : null;
    });

    /** Every heartbeat this map has logged since the navigation. */
    const beats = () =>
      hud()
        .slice(before)
        .filter((line) => line.match(practice.map)?.[1] === practice.mapName);
    /** Every value `pattern` has captured on those, as the words they are. */
    const words = (/** @type {RegExp} */ pattern) =>
      beats()
        .map((line) => line.match(pattern)?.[1])
        .filter((value) => value !== undefined);
    /** …and as numbers, with anything unreadable dropped. */
    const readings = (/** @type {RegExp} */ pattern) =>
      words(pattern).map(Number).filter(Number.isFinite);
    /** The highest value `pattern` has reported, or null if it never did. */
    const peak = (/** @type {RegExp} */ pattern) => {
      const seen = readings(pattern);
      return seen.length > 0 ? Math.max(...seen) : null;
    };

    const opened = await until(async () =>
      beats().length > 0 && peak(practice.alive) === practice.count
        ? beats().at(-1)
        : null
    );
    check(
      'C',
      "the practice map opens from the page's own query string",
      Boolean(opened) && rebooted === 3,
      opened
        ? opened.trim()
        : `${beats().length} heartbeat(s) said "${practice.mapName}" out of ` +
            `${hud().length - before} since ${mapUrl} was loaded, and the ` +
            `status is ${rebooted ?? 'never settled'} — the last line was ` +
            `"${(hud().at(-1) ?? 'none').trim()}"`
    );

    // ---- the pair about the patrol -----------------------------------------
    /** Where the first bot's feet were on each beat, as one string a beat. */
    const patrol = () => {
      const across = words(practice.botAcross);
      const along = words(practice.botAlong);
      return across.map((x, at) => `${x},${along[at] ?? '?'}`);
    };
    const walked = await until(async () => {
      const steps = new Set(patrol());
      return steps.size > 1 ? steps : null;
    });
    check(
      'C',
      'a bot walks its patrol under its own steam',
      walked !== null,
      walked
        ? `its feet took ${walked.size} positions: ${[...walked].join(' ')}`
        : `it never left ${patrol()[0] ?? 'anywhere'} over ${beats().length} beat(s)`
    );

    // **And it was a bot that moved.** Two ways the check above could pass for
    // the wrong reason, and both are closed here.
    //
    // The first is that the moving reading is not a bot's at all. Asking
    // whether the *player* stood still does not close it — nothing has pressed
    // a key since the navigation, so a build reporting the player's position
    // under a bot's name reports a number that does not move, which fails the
    // positive and leaves this one green. What closes it is that the two
    // readings are **different readings**: the bot's feet and the player's are
    // compared on every beat, and a heartbeat printing one of them twice is
    // caught however still either of them is. Measured that way, sabotaging
    // `arena_stats` to report the player's position here fails this check
    // instead of quietly passing it.
    //
    // The second is that the page never left the firing range, whose
    // travelling plate moves on a timer and is what group C's `moving` check
    // already reads. A heartbeat carrying that field at all is the range's, so
    // none of them may.
    const together = beats().filter((line) => {
      const bot = `${line.match(practice.botAcross)?.[1]},${line.match(practice.botAlong)?.[1]}`;
      const player = `${line.match(practice.playerAcross)?.[1]},${line.match(EXPECTED.range.advance)?.[1]}`;
      return bot === player;
    });
    const stood = new Set(
      beats().map(
        (line) =>
          `${line.match(practice.playerAcross)?.[1]},` +
          `${line.match(EXPECTED.range.advance)?.[1]}`
      )
    );
    const plateReported = beats().filter((line) =>
      practice.moverField.test(line)
    );
    check(
      'C',
      'and it is a bot that moved, not the player and not the other map',
      beats().length > 0 && together.length === 0 && plateReported.length === 0,
      plateReported.length > 0
        ? `${plateReported.length} of ${beats().length} beat(s) still reported a ` +
            `travelling plate, so this is the firing range and the query string ` +
            `did nothing: "${plateReported.at(-1)?.trim()}"`
        : together.length > 0
          ? `${together.length} of ${beats().length} beat(s) put the bot's feet exactly ` +
            `where the player's are, so the reading above is the player's under ` +
            `another name: "${together.at(-1)?.trim()}"`
          : `the bot's feet were never the player's, who held ` +
            `${[...stood].join(' ')} for all ${beats().length} beat(s)`
    );

    // ---- the pair about the sighting ---------------------------------------
    const noticed = await until(async () =>
      (peak(practice.seen) ?? 0) > 0 ? peak(practice.seen) : null
    );
    check(
      'C',
      'a bot notices the player when it can see them',
      noticed !== null,
      noticed
        ? `${noticed} bot(s) had the player in sight at once`
        : `no bot ever saw the player over ${beats().length} beat(s), though ` +
            `${peak(practice.covered) ?? 0} were near enough and blocked`
    );

    // **The control, and it is the whole claim about `cast_ray`.** `covered`
    // counts bots that are inside the notice range and still cannot see the
    // player, so a build that noticed anything within range — a distance test
    // wearing a ray's name — reports it as zero for the whole run while the
    // sighting check above stays green.
    const blocked = await until(async () =>
      (peak(practice.covered) ?? 0) > 0 ? peak(practice.covered) : null
    );
    check(
      'C',
      'and a bot behind cover does not, though it is near enough to',
      blocked !== null,
      blocked
        ? `${blocked} bot(s) were in range with the pillar in the way`
        : `every bot in range saw the player on all ${beats().length} beat(s) ` +
            `(seen went up to ${peak(practice.seen) ?? 0} of ${practice.count}), ` +
            `so the cover on this map stops nothing`
    );

    // ---- the pair about being shot at --------------------------------------
    const hurt = await until(async () => {
      const health = readings(practice.health);
      const arrived = peak(practice.taken) ?? 0;
      const downs = peak(practice.downs) ?? 0;
      const fell = health.some((value, at) => at > 0 && value < health[at - 1]);
      return arrived > 0 && (fell || downs > 0) ? { arrived, downs } : null;
    });
    check(
      'C',
      "a bot's round reaches the player and costs them health",
      hurt !== null,
      hurt
        ? `${hurt.arrived} round(s) arrived and put them down ${hurt.downs} time(s); ` +
            `health read ${readings(practice.health).join(', ')}`
        : `${peak(practice.taken) ?? 0} round(s) of ${peak(practice.fired) ?? 0} ` +
            `arrived and health held at ${[...new Set(readings(practice.health))].join(', ') || 'nothing'}`
    );

    // **The control.** A bot goes on shooting for a moment after it loses
    // sight, and those rounds land in whatever is now in the way — so `fired`
    // ahead of `taken` is cover having stopped one. A build whose bots hit
    // whatever they fired at reports the two in step for the whole run and
    // passes the check above regardless.
    const stopped = await until(async () => {
      const rounds = beats()
        .map((line) => ({
          fired: Number(line.match(practice.fired)?.[1]),
          taken: Number(line.match(practice.taken)?.[1]),
        }))
        .filter(
          ({ fired, taken }) => Number.isFinite(fired) && Number.isFinite(taken)
        );
      const short = rounds.filter(({ fired, taken }) => fired > taken).at(-1);
      return short ?? null;
    });
    check(
      'C',
      'and a round with cover in the way never arrives',
      stopped !== null,
      stopped
        ? `${stopped.fired - stopped.taken} of ${stopped.fired} round(s) went into ` +
            `something other than the player`
        : `every one of the ${peak(practice.fired) ?? 0} round(s) fired arrived, ` +
            `so nothing on this map ever stopped one`
    );

    // ---- and the page goes back to the map the rest of this run judges ------
    // Not a check: the groups below are entitled to the demo they have always
    // been handed, which is the firing range — `moving` above read its
    // travelling plate and group D's canvas checks were written against its
    // room. Leaving the run on a second map would be this block changing what
    // every check after it is looking at.
    const backAt = hud().length;
    await page.send('Page.navigate', { url });
    await until(async () =>
      evaluate(page, `document.readyState === 'complete'`)
    );
    await until(async () => {
      const status = await evaluate(page, `crcbl.status()`);
      return status === 3 ? status : null;
    });
    // **And the canvas gets its keyboard back.** A navigation replaces the
    // document, so the click group C made was on an element that no longer
    // exists — and group E's first claim is that *blurring* the canvas pauses
    // the demo, which a canvas that never had focus cannot do. Without this the
    // whole of group E fails on this demo and on no other, for a reason that is
    // this block's doing rather than the page's.
    await clickAt(await focusPoint());
    await until(async () =>
      (await evaluate(page, `document.activeElement?.id ?? ''`)) === 'canvas'
        ? true
        : null
    );
    await until(async () => (hud().length > backAt ? hud().length : null));
  }

  // **AND THE ZONE BEING WALKED, AND THEN UNLIT.**
  // Only shard has one. `moving` above says the torchlight is flickering, and
  // it would go on saying so for a page whose input was severed and for a page
  // whose lighting was a picture of a lit room rather than a lit room — the
  // flame reading is computed on the simulation's clock and printed, and
  // nothing about it has been near a pixel.
  //
  // So this block drives the demo's own keys and reads what happened, in two
  // pairs and a control:
  //
  // * the character **advances** while the walk key is held and **stops** when
  //   it is released. The second is the control for the first, and it has a
  //   control of its own: `blocked` must not have risen, because three equal
  //   positions are exactly what a capsule pressed against stone reports and
  //   that would be the room stopping the character rather than the key.
  // * the canvas **keeps changing while the torches burn** and **holds still
  //   when they are put out**, with the loop still beating through the stillness
  //   and the frame still a picture rather than a black rectangle. This is the
  //   one claim on this page that is about the lighting being *computed*: every
  //   other check here reads a number the demo printed about itself, and a build
  //   that logged `lighting: Rasterised` while drawing an unlit scene would pass
  //   all of them. What it cannot fake is a canvas whose brightness follows a
  //   light nobody is looking at the log of.
  // * and lighting them again **brings the movement back**, which is what says
  //   the stillness was a light going out rather than the demo dying.
  //
  // Every failure message carries the readings, so a red run says which went
  // wrong and at what value.
  // The four helpers the two shard blocks below share. Declared out here rather
  // than inside the first of them because the second needs the same four, and a
  // second copy of a key dispatcher is a second copy that can dispatch a
  // slightly different event.
  /** Presses or releases one of this demo's keys, through the browser's own pipeline. */
  const zoneKey = async (
    /** @type {{code: string, key: string, text: string, virtualKeyCode: number}} */ binding,
    /** @type {string} */ type
  ) =>
    page.send('Input.dispatchKeyEvent', {
      type,
      code: binding.code,
      key: binding.key,
      windowsVirtualKeyCode: binding.virtualKeyCode,
      nativeVirtualKeyCode: binding.virtualKeyCode,
      ...(type === 'keyDown' ? { text: binding.text } : {}),
    });
  /** A press and a release, for a key that is tapped rather than held. */
  const tapZoneKey = async (/** @type {any} */ binding) => {
    await zoneKey(binding, 'keyDown');
    await zoneKey(binding, 'keyUp');
  };
  /** The most recent value `pattern` captured on a HUD line, as a number. */
  const latest = (/** @type {RegExp} */ pattern) => {
    const lines = hud();
    for (let at = lines.length - 1; at >= 0; at -= 1) {
      const found = lines[at].match(pattern);
      if (found) return Number(found[1]);
    }
    return null;
  };
  /** Every value `pattern` has captured on the HUD lines from `from` on. */
  const since = (/** @type {RegExp} */ pattern, /** @type {number} */ from) =>
    hud()
      .slice(from)
      .map((line) => line.match(pattern)?.[1])
      .filter((value) => value !== undefined);

  if (EXPECTED.zone) {
    const zone = EXPECTED.zone;

    // ---- the pair about input reaching the controller at all ----------------
    const startedAt = latest(zone.advance);
    const blockedBefore = latest(zone.blocked);
    await zoneKey(zone.walk, 'keyDown');
    const advanced = await until(async () => {
      const now = latest(zone.advance);
      return startedAt !== null &&
        now !== null &&
        startedAt - now >= WALK_ADVANCE_M
        ? now
        : null;
    });
    check(
      'C',
      'the walk key reaches the controller and the character walks the zone',
      advanced !== null,
      advanced === null
        ? `it started at ${startedAt} and never got ${WALK_ADVANCE_M} m past it — ` +
            `last reading ${latest(zone.advance)} over ${hud().length} HUD line(s)`
        : `they walked from ${startedAt} to ${advanced}`
    );

    // **They have to have still been moving when the key came up**, or "they
    // stopped" is a claim about a character something else had already stopped.
    // The two lines before the release are both under a held key, so a pair of
    // equal readings there is the vacuous case and this is what catches it.
    const atRelease = hud().length;
    await zoneKey(zone.walk, 'keyUp');
    const lastHeld = hud()[atRelease - 1]?.match(zone.advance)?.[1];
    const priorHeld = hud()[atRelease - 2]?.match(zone.advance)?.[1];
    const stillMoving = Boolean(
      lastHeld && priorHeld && lastHeld !== priorHeld
    );
    const settled = await until(async () => {
      const readings = since(zone.advance, atRelease);
      if (readings.length < WALK_STILL_BEATS) return null;
      const tail = readings.slice(-WALK_STILL_BEATS);
      return tail.every((value) => value === tail[0]) ? tail[0] : null;
    });
    const blockedAfter = latest(zone.blocked);
    const unobstructed =
      blockedBefore !== null && blockedAfter === blockedBefore;
    check(
      'C',
      'and stops where they are when it is released',
      Boolean(settled) && stillMoving && unobstructed,
      !stillMoving
        ? `they were already standing still before the release (${priorHeld} then ` +
            `${lastHeld}), so this check would pass on a demo that never moved`
        : !unobstructed
          ? `the zone refused the walk ${blockedAfter} time(s) against ` +
            `${blockedBefore} before it, so stone stopped them and not the key`
          : settled
            ? `they held ${settled} for ${WALK_STILL_BEATS} beats after the ` +
              `release, with blocked flat at ${blockedAfter}`
            : `they kept walking with nothing held: ${since(zone.advance, atRelease).join(', ')}`
    );

    // ---- the pair about the light being computed rather than declared -------
    /**
     * Samples the canvas across a window wide enough to hold a flicker cycle,
     * and reduces it to what the three checks below compare.
     *
     * `beats` is how many heartbeats arrived while the samples were being
     * taken, and it is the reason the doused window can claim stillness at all:
     * a canvas that stopped changing because the demo stopped running is the
     * other explanation for every still frame, and a rising heartbeat is what
     * rules it out. `flattest` is the largest share any one quantised colour
     * took of any frame in the window — the black-rectangle control.
     */
    const sampleWindow = async () => {
      const from = hud().length;
      const taken = [];
      for (let at = 0; at < TORCH_SAMPLES; at += 1) {
        if (at > 0) await pause(TORCH_SAMPLE_GAP_MS);
        const sample = await evaluate(page, SAMPLE_CANVAS('#canvas'));
        if (sample) taken.push(sample);
      }
      const lumas = taken.map((sample) => sample.luma);
      return {
        samples: taken.length,
        frames: new Set(taken.map((sample) => sample.hash)).size,
        beats: hud().length - from,
        mean: lumas.length
          ? lumas.reduce((sum, value) => sum + value, 0) / lumas.length
          : 0,
        spread: lumas.length ? Math.max(...lumas) - Math.min(...lumas) : 0,
        flattest: taken.length
          ? Math.max(...taken.map((sample) => sample.top[0]?.share ?? 1))
          : 1,
      };
    };
    /** What a window measured, for a failure message to carry. */
    const readWindow = (/** @type {any} */ window) =>
      `${window.frames} distinct frame(s) in ${window.samples} sample(s) over ` +
      `${window.beats} beat(s), mean luma ${window.mean.toFixed(2)} swinging ` +
      `${window.spread.toFixed(2)}, flattest colour ${(window.flattest * 100).toFixed(1)}%`;

    const lit = await sampleWindow();
    check(
      'C',
      'the torchlight keeps the picture changing with nothing held',
      lit.samples === TORCH_SAMPLES &&
        lit.frames > 1 &&
        lit.spread >= TORCH_FLICKER_LUMA,
      `${readWindow(lit)}; ${TORCH_FLICKER_LUMA} of swing asked for`
    );

    // The switch, read off the demo's own heartbeat rather than assumed from
    // the keystroke — a dispatched key that went nowhere would otherwise be
    // reported below as a lighting failure.
    const beforeDouse = hud().length;
    await tapZoneKey(zone.torch);
    const doused = await until(async () =>
      since(zone.torches, beforeDouse).includes('out') ? hud().length : null
    );
    const out = await sampleWindow();
    check(
      'C',
      'and dousing them leaves a still frame that is darker but not blank',
      doused !== null &&
        out.samples === TORCH_SAMPLES &&
        out.beats > 0 &&
        out.frames === 1 &&
        out.spread <= TORCH_STILL_LUMA &&
        out.mean <= lit.mean * TORCH_DARKER_RATIO &&
        out.flattest < TORCH_FLAT_SHARE,
      doused === null
        ? `no heartbeat in ${TIMEOUT_MS} ms said the torches were out — the key ` +
            `never reached the game, so nothing below is about the lighting`
        : `${readWindow(out)}; asked for a swing under ${TORCH_STILL_LUMA}, a ` +
            `mean under ${(lit.mean * TORCH_DARKER_RATIO).toFixed(2)} (the lit ` +
            `window read ${lit.mean.toFixed(2)}) and no colour over ` +
            `${(TORCH_FLAT_SHARE * 100).toFixed(0)}%`
    );

    // And back, which is what says the stillness was a light going out rather
    // than the page dying with a heartbeat still ticking.
    const beforeRelight = hud().length;
    await tapZoneKey(zone.torch);
    const relit = await until(async () =>
      since(zone.torches, beforeRelight).includes('lit') ? hud().length : null
    );
    const back = await sampleWindow();
    check(
      'C',
      'and lighting them again brings the flicker back',
      relit !== null &&
        back.samples === TORCH_SAMPLES &&
        back.frames > 1 &&
        back.spread >= TORCH_FLICKER_LUMA,
      relit === null
        ? `no heartbeat in ${TIMEOUT_MS} ms said the torches were lit again`
        : `${readWindow(back)}; ${TORCH_FLICKER_LUMA} of swing asked for`
    );
  }

  // **AND THE ZONE BEING FOUGHT IN, WHICH NOTHING ABOVE CAN SEE.**
  // Only shard has one. Everything in the block above is a character walking a
  // room and a light going out, and all of it passes on a zone with nothing
  // alive in it.
  //
  // **This block runs last, and after the lighting block on purpose.** That
  // block asks for a canvas that does not change at all while the torches are
  // out, and a body walking through the frame is a body to redraw. Every foe
  // stands on a post outside `foe::NOTICE_M` of the spawn and outside the frame
  // the zone opens on, so nothing moves until this block walks the character at
  // something — and the two native tests named in the `fight` row above are what
  // hold the posts there.
  //
  // Three pairs, each a positive and the control that stops it passing for the
  // wrong reason:
  //
  // * a foe **engages** once the character comes at it, and **had engaged
  //   nothing** on every heartbeat before the walk key went down. The second is
  //   what tells "it reacted" from "it was always like that", which a sighting
  //   check on its own cannot.
  // * a blow **fells a foe** — the alive count falls — and a blow swung with
  //   **nothing in reach fells nothing**. The second is the whole claim that the
  //   cleave is resolved against `PhysicsWorld::cast_ray`: a build that counted
  //   key presses passes the kill and fails this.
  // * a foe's ability **costs the character health**, and the character **had
  //   taken nothing** before the fight. The second is what tells damage from a
  //   number that only ever counts up: a counter that was always climbing was
  //   climbing on those beats too.
  //
  // **Every observable here is read over the whole retained buffer of
  // heartbeats, not off the latest line.** `foes`, `swings`, `hits` and `taken`
  // are monotone, so a reading that has moved cannot be missed however slowly
  // frames are arriving; `engaged` is not — a felled foe stops being engaged —
  // so its claims are made against the maximum any beat reported rather than
  // against the state at the moment of asking.
  if (EXPECTED.fight) {
    const fight = EXPECTED.fight;

    /** Every value `pattern` has captured on the beats from `from` on, as numbers. */
    const numbersSince = (
      /** @type {RegExp} */ pattern,
      /** @type {number} */ from
    ) => since(pattern, from).map(Number).filter(Number.isFinite);
    /** The highest value `pattern` has reported since `from`, or null for none. */
    const peakSince = (
      /** @type {RegExp} */ pattern,
      /** @type {number} */ from
    ) => {
      const seen = numbersSince(pattern, from);
      return seen.length > 0 ? Math.max(...seen) : null;
    };

    // ---- the control for both of the pairs below, taken before anything ------
    // Read *first*, so it is a statement about the run up to this point rather
    // than about a window this block chose after the fact.
    const quiet = numbersSince(fight.engaged, 0);
    const untouched = numbersSince(fight.taken, 0);
    const unhurt = numbersSince(fight.health, 0);
    const restingBeats = quiet.length;

    // ---- the control for the blow: a swing with nothing in reach ------------
    // Nothing is within the cleave's reach where the lighting block left the
    // character, and `target` is read rather than assumed — a page that had
    // walked somewhere unexpected would otherwise have this pass for a swing
    // that missed by luck.
    const aimedAt = since(fight.target, 0).at(-1);
    const beforeSwing = hud().length;
    const swungBefore = latest(fight.swings) ?? 0;
    const hitsBefore = latest(fight.hits) ?? 0;
    const aliveBefore = latest(fight.alive) ?? 0;
    await tapZoneKey(fight.strike);
    const swung = await until(async () => {
      const now = latest(fight.swings);
      return now !== null && now > swungBefore ? now : null;
    });
    const missed =
      swung !== null &&
      aimedAt === fight.nothing &&
      aliveBefore === fight.count &&
      (peakSince(fight.hits, beforeSwing) ?? 0) === hitsBefore &&
      (peakSince(fight.alive, beforeSwing) ?? 0) === fight.count;
    check(
      'C',
      'a blow swung with nothing in reach fells nothing',
      missed,
      swung === null
        ? `the strike key never reached the game: swings held at ${swungBefore} ` +
            `over ${hud().length - beforeSwing} beat(s)`
        : aimedAt !== fight.nothing
          ? `something was already in reach ("${aimedAt}"), so this swing was ` +
            `not a blow at nothing`
          : `it swung ${swungBefore} → ${swung} with hits at ` +
            `${peakSince(fight.hits, beforeSwing)} and ` +
            `${peakSince(fight.alive, beforeSwing)} of ${fight.count} foes standing`
    );

    // ---- the walk that starts the fight -------------------------------------
    // The walk key and the blow are both held from here: the character walks up
    // the corridor at the nearest post, and the cadence the *server* owns is
    // what decides how often the blow actually swings.
    const beforeFight = hud().length;
    await zoneKey(fight.walk, 'keyDown');
    await zoneKey(fight.strike, 'keyDown');

    const noticed = await until(async () =>
      (peakSince(fight.engaged, beforeFight) ?? 0) > 0
        ? peakSince(fight.engaged, beforeFight)
        : null
    );
    const wasQuiet = restingBeats > 0 && quiet.every((value) => value === 0);
    check(
      'C',
      'a foe engages the character once they come at it',
      noticed !== null && wasQuiet,
      !wasQuiet
        ? `${quiet.filter((value) => value > 0).length} of ${restingBeats} beat(s) ` +
            `before the walk key already reported an engaged foe, so this demo ` +
            `opens engaged and nothing here is about the sighting`
        : noticed
          ? `${noticed} foe(s) had the character over ` +
            `${hud().length - beforeFight} beat(s), against 0 on all ` +
            `${restingBeats} beat(s) before the key went down`
          : `nothing noticed the character over ${hud().length - beforeFight} ` +
            `beat(s) of walking at them; ${latest(fight.alive)} of ${fight.count} ` +
            `foes are standing`
    );

    // ---- the pair about being hurt ------------------------------------------
    const hurt = await until(async () => {
      const arrived = peakSince(fight.taken, beforeFight) ?? 0;
      const health = numbersSince(fight.health, beforeFight);
      const downs = peakSince(fight.downs, beforeFight) ?? 0;
      const fell = health.some((value) => value < fight.full);
      return arrived > 0 && (fell || downs > 0) ? { arrived, downs } : null;
    });
    const wasWhole =
      untouched.length > 0 &&
      untouched.every((value) => value === 0) &&
      unhurt.every((value) => value === fight.full);
    check(
      'C',
      "and a foe's ability costs them health, which nothing before it did",
      hurt !== null && wasWhole,
      !wasWhole
        ? `the character was already losing health before anything engaged: ` +
            `taken read ${[...new Set(untouched)].join(', ')} and hp read ` +
            `${[...new Set(unhurt)].join(', ')} over ${untouched.length} beat(s), ` +
            `so "taken went up" says nothing here`
        : hurt
          ? `${hurt.arrived} damage arrived and put them down ${hurt.downs} time(s); ` +
            `hp read ${[...new Set(numbersSince(fight.health, beforeFight))].join(', ')} ` +
            `against ${fight.full} on all ${untouched.length} beat(s) before`
          : `${peakSince(fight.taken, beforeFight) ?? 0} damage arrived and hp held ` +
            `at ${[...new Set(numbersSince(fight.health, beforeFight))].join(', ') || 'nothing'}`
    );

    // ---- and the pair about felling one --------------------------------------
    const felled = await until(async () => {
      const standing = numbersSince(fight.alive, beforeFight);
      const landed = peakSince(fight.hits, beforeFight) ?? 0;
      const lowest = standing.length > 0 ? Math.min(...standing) : fight.count;
      return lowest < fight.count && landed > hitsBefore
        ? { lowest, landed }
        : null;
    });
    check(
      'C',
      'and a blow that reaches one fells it',
      felled !== null,
      felled
        ? `${fight.count - felled.lowest} of ${fight.count} foe(s) went down under ` +
            `${felled.landed - hitsBefore} landed blow(s), for ` +
            `${peakSince(fight.dealt, beforeFight) ?? 0} damage`
        : `${peakSince(fight.hits, beforeFight) ?? 0} blow(s) landed and ` +
            `${latest(fight.alive)} of ${fight.count} foes are still standing after ` +
            `${hud().length - beforeFight} beat(s); the cleave last had ` +
            `"${since(fight.target, beforeFight).at(-1) ?? 'nothing'}" in reach`
    );

    // Not a check: the groups below are entitled to a demo nobody is leaning on
    // a key of, exactly as the walk block above hands one back.
    await zoneKey(fight.strike, 'keyUp');
    await zoneKey(fight.walk, 'keyUp');
  }

  // **AND THE CHARACTER COMING BACK, WHICH NOTHING ABOVE CAN SEE.**
  // Only shard has one. Everything above is one session: a character walks, a
  // light goes out, a foe goes down — and every bit of it passes on a build
  // that keeps nothing at all, because a page that is never reloaded never has
  // to answer for what it wrote.
  //
  // **This block runs last of shard's three, and it reloads the page twice.**
  // The blocks above are entitled to the session they have always been handed,
  // and the second reload is what hands the groups below a demo booted from an
  // empty store — which is the state every other demo's page is in.
  //
  // Three claims, and each carries the control that stops it passing for the
  // wrong reason:
  //
  // * the save **is on the browser's own disk** — the OPFS directory holds the
  //   file, framed and containing the save container's magic, with nothing left
  //   queued inside wasm. The control is the wipe below: clearing that
  //   directory leaves it empty, which is what says the file found here was
  //   this demo's rather than something else in the origin.
  // * a reloaded page **comes back where it left off**, and it is checked
  //   against the heartbeat that reported the write rather than against a
  //   guess. The state asserted is state a fresh boot cannot have: a foe that
  //   is felled — `foes` is monotone and nothing in this zone respawns — and a
  //   position metres off the spawn.
  // * and the control for that: **a page booted with the store cleared comes up
  //   fresh**, at the spawn, at full health, with the whole zone standing.
  //   Without it, "it resumed" passes for a build that never saved anything and
  //   simply always starts in the same place.
  //
  // **Nothing here waits on a wall clock for the simulation.** The reference
  // reading is the heartbeat carrying a raised `saves`, which is a fixed
  // simulated tick; every reading after a reload is taken from the *first*
  // heartbeat that boot logged, which is another one. A machine drawing this
  // zone five times slower reaches both at the same simulated moment.
  if (EXPECTED.save) {
    const save = EXPECTED.save;

    /** One heartbeat, as the fields this block reads. */
    const reading = (/** @type {string} */ line) => {
      const fields = {
        resumed: line.match(save.resumed)?.[1],
        writes: Number(line.match(save.writes)?.[1]),
        along: Number(line.match(save.along)?.[1]),
        health: Number(line.match(save.health)?.[1]),
        downs: Number(line.match(save.downs)?.[1]),
        alive: Number(line.match(save.alive)?.[1]),
      };
      const numbers = [
        fields.writes,
        fields.along,
        fields.health,
        fields.downs,
        fields.alive,
      ];
      if (
        fields.resumed === undefined ||
        numbers.some((n) => !Number.isFinite(n))
      )
        return null;
      return { ...fields, line };
    };

    /** What the OPFS directory holds, and what wasm still has queued for it. */
    const storage = async () =>
      evaluate(
        page,
        `(async () => {
           const queue = crcbl.saves();
           let files = [];
           try {
             const root = await navigator.storage.getDirectory();
             for await (const [name, handle] of root.entries()) {
               if (handle.kind !== 'file') continue;
               const blob = await handle.getFile();
               const head = new Uint8Array(
                 await blob.slice(0, ${save.frameHeader} + ${save.magic.length}).arrayBuffer()
               );
               const text = (from, to) =>
                 String.fromCharCode(...head.slice(from, to));
               files.push({
                 name,
                 bytes: blob.size,
                 record: text(0, ${save.recordMagic.length}),
                 container: text(
                   ${save.frameHeader},
                   ${save.frameHeader} + ${save.magic.length}
                 ),
               });
             }
           } catch (error) {
             return { queue, files: null, error: String(error) };
           }
           return { queue, files };
         })()`
      );

    // ---- the write this block is about ---------------------------------------
    // A save that *postdates* the fight, so the state it holds is the state the
    // blocks above produced rather than one from before the first blow. The
    // wait is one autosave period of simulated time and no more.
    // The highest count any beat has reported, not the last line's: a line the
    // console truncated would read as zero and let a beat from before the fight
    // answer for one after it.
    const beats = () =>
      hud()
        .map(reading)
        .filter((r) => r !== null);
    const writesBefore = Math.max(0, ...beats().map((r) => r.writes));
    const written = await until(async () => {
      const beat = beats().find((r) => r.writes > writesBefore);
      return beat ?? null;
    });

    // ---- and the claim that it reached the disk ------------------------------
    // `pending + inFlight === 0` is `crcbl-store`'s own answer to "is it on the
    // disk yet" — the record has been taken by the shim and acknowledged — and
    // the file is then read back out of OPFS to see that the bytes are there
    // and are the ones this engine writes.
    const landed = written
      ? await until(async () => {
          const seen = await storage();
          if (seen.files === null) return null;
          const file = seen.files.find((f) => save.files.includes(f.name));
          return file &&
            seen.queue.pending === 0 &&
            seen.queue.inFlight === 0 &&
            file.record === save.recordMagic &&
            file.container === save.magic
            ? { ...seen, file }
            : null;
        })
      : null;
    const beforeReload = hud().length;
    check(
      'C',
      "the character is on the browser's own file system, not in the page",
      landed !== null,
      landed
        ? `${landed.file.name} holds ${landed.file.bytes} bytes framed ` +
            `"${landed.file.record}" around a "${landed.file.container}" container, ` +
            `with nothing queued in wasm — written on the beat that reported ` +
            `saves: ${written.writes}`
        : written === null
          ? `no heartbeat in ${TIMEOUT_MS} ms reported a save past the ` +
            `${writesBefore} this page had already made`
          : `after ${written.writes} save(s) the OPFS root held ` +
            `${JSON.stringify((await storage()).files)}, queued ` +
            `${JSON.stringify((await storage()).queue)}`
    );

    // ---- the page comes back where it left off -------------------------------
    await page.send('Page.navigate', { url });
    await until(async () =>
      evaluate(page, `document.readyState === 'complete'`)
    );
    await until(async () => {
      const status = await evaluate(page, `crcbl.status()`);
      return status === 3 ? status : null;
    });
    const resumedBeat = await until(async () => {
      const beat = hud()
        .slice(beforeReload)
        .map(reading)
        .find((r) => r !== null);
      return beat ?? null;
    });
    const restored =
      written !== null &&
      resumedBeat !== null &&
      resumedBeat.resumed === 'yes' &&
      resumedBeat.alive === written.alive &&
      resumedBeat.alive < save.count &&
      (resumedBeat.health < save.full || resumedBeat.downs > 0) &&
      Math.abs(resumedBeat.along - written.along) < save.tolerance &&
      Math.abs(resumedBeat.along - save.spawnAlong) > save.awayFromSpawn;
    check(
      'C',
      'a reloaded page resumes the character the last one saved',
      restored,
      resumedBeat === null
        ? `no heartbeat in ${TIMEOUT_MS} ms after the reload`
        : `it came up "resumed: ${resumedBeat.resumed}" at pz ${resumedBeat.along} ` +
            `with ${resumedBeat.alive} of ${save.count} foes standing, ` +
            `${resumedBeat.health} health and ${resumedBeat.downs} down(s); ` +
            `the save's own beat read pz ${written?.along} with ${written?.alive} ` +
            `standing, ${written?.health} health and ${written?.downs} down(s), ` +
            `against a spawn at pz ${save.spawnAlong}`
    );

    // ---- and the control: the same page with the store cleared ---------------
    // **Paused first, and that is not tidiness.** This demo autosaves once per
    // simulated second, so a directory emptied while it is still ticking is one
    // the next write refills — and the control would then be reading a save
    // this block created after wiping the one it meant to remove. A blurred
    // canvas runs no ticks at all, which group E asserts for every demo on this
    // site, so nothing can be written between the wipe and the navigation.
    //
    // **The click comes first, and it is what makes the blur mean anything.**
    // The navigation above replaced the document, so this canvas has never held
    // the keyboard — and focusing the stop button blurs nothing, leaves the
    // demo running, and leaves the wait below to spend the whole timeout
    // discovering it. Measured: 92 s of a 197 s run, every second of it this
    // one missing click.
    await clickAt(await focusPoint());
    await until(async () =>
      (await evaluate(page, `document.activeElement?.id ?? ''`)) === 'canvas'
        ? true
        : null
    );
    await evaluate(page, `document.getElementById('stop').focus()`);
    await until(async () => {
      const status = await evaluate(page, `crcbl.status()`);
      return status === 6 ? status : null;
    });
    // …and with the queue drained as well, so nothing the page had already
    // handed the shim can be written back after the directory is emptied. The
    // `pagehide` teardown flushes whatever is left, which on this path is the
    // last chance a record would get to reappear.
    const settled = await until(async () => {
      const queue = await evaluate(page, `crcbl.saves()`);
      return queue.pending === 0 && queue.inFlight === 0 ? queue : null;
    });
    const wiped = settled
      ? await evaluate(
          page,
          `(async () => {
             const root = await navigator.storage.getDirectory();
             const names = [];
             for await (const [name] of root.entries()) names.push(name);
             for (const name of names) await root.removeEntry(name);
             const left = [];
             for await (const [name] of root.entries()) left.push(name);
             return { removed: names, left };
           })()`
        )
      : null;
    const cleared =
      wiped !== null && wiped.left.length === 0 && wiped.removed.length > 0;

    const afterWipe = hud().length;
    await page.send('Page.navigate', { url });
    await until(async () =>
      evaluate(page, `document.readyState === 'complete'`)
    );
    await until(async () => {
      const status = await evaluate(page, `crcbl.status()`);
      return status === 3 ? status : null;
    });
    const freshBeat = await until(async () => {
      const beat = hud()
        .slice(afterWipe)
        .map(reading)
        .find((r) => r !== null);
      return beat ?? null;
    });
    const opened =
      cleared &&
      freshBeat !== null &&
      freshBeat.resumed === 'no' &&
      freshBeat.alive === save.count &&
      freshBeat.health === save.full &&
      freshBeat.downs === 0 &&
      Math.abs(freshBeat.along - save.spawnAlong) < save.tolerance;
    check(
      'C',
      'and the same page with the store cleared comes up fresh',
      opened,
      !cleared
        ? `the OPFS root would not empty: removed ` +
            `${JSON.stringify(wiped?.removed ?? null)}, left ` +
            `${JSON.stringify(wiped?.left ?? null)}, queued ` +
            `${JSON.stringify(settled)}`
        : freshBeat === null
          ? `no heartbeat in ${TIMEOUT_MS} ms after the reload`
          : `with ${wiped.removed.join(', ')} removed it came up ` +
            `"resumed: ${freshBeat.resumed}" at pz ${freshBeat.along} with ` +
            `${freshBeat.alive} of ${save.count} foes standing, ` +
            `${freshBeat.health} health and ${freshBeat.downs} down(s)`
    );

    // **And the canvas gets its keyboard back.** Two navigations replaced the
    // document, so the click group C made was on an element that no longer
    // exists — and group E's first claim is that *blurring* the canvas pauses
    // the demo, which a canvas that never had focus cannot do.
    const handedBack = hud().length;
    await clickAt(await focusPoint());
    await until(async () =>
      (await evaluate(page, `document.activeElement?.id ?? ''`)) === 'canvas'
        ? true
        : null
    );
    await until(async () => (hud().length > handedBack ? hud().length : null));
  }

  // **AND THE BUDGET, WHICH THE CHECK ABOVE CANNOT SEE EITHER.**
  // Only sparks has one. `moving` above says a particle count is changing, and
  // it would go on saying so for a page whose effects never retired, whose
  // emitters could not be stopped, and whose pool budget was not enforced at
  // all — a number that only ever climbs still changes.
  //
  // So this block reads four claims off the demo's own heartbeat, in two pairs:
  //
  // * an emitter's count **climbs while it runs** and **comes back to zero
  //   after it stops**. The second is the control for the first: without it,
  //   "the count went up" passes for a simulation that never retires anything.
  //   Nothing here presses a key — the demo switches that emitter on its own
  //   schedule — so what the pair proves is the simulation rather than the
  //   input path.
  // * the greedy effect **holds at its pool share**, and its emitter is
  //   **being refused** rather than merely idle. The second is the control for
  //   the first: a count parked on its share could be an emitter that happens
  //   to ask for exactly that many, and only a refusal counter climbing while
  //   the count stands still says a clamp is what is holding it.
  //
  // Every failure message carries the values, so a red run says which of the
  // four went wrong and at what reading.
  if (EXPECTED.budget) {
    const budget = EXPECTED.budget;

    /** Every HUD line, as the fields this block cares about. */
    const readings = () =>
      hud()
        .map((line) => {
          const emitting = line.match(budget.emitting);
          const held = line.match(budget.held);
          const greedy = line.match(budget.greedy);
          const share = line.match(budget.share);
          const refused = line.match(budget.refused);
          if (!emitting || !held || !greedy || !share || !refused) return null;
          return {
            line,
            emitting: emitting[1] === 'yes',
            held: Number(held[1]),
            greedy: Number(greedy[1]),
            share: Number(share[1]),
            refused: Number(refused[1]),
          };
        })
        .filter((reading) => reading !== null);

    // ---- the emitter, running -------------------------------------------
    // `until` treats a falsy value as "not yet", so this hands back an object
    // rather than the index — the first line is index `0`, and returning that
    // would poll until the timeout on a demo that passed immediately.
    const filled = await until(async () => {
      const seen = readings();
      const at = seen.findIndex(
        (reading) => reading.emitting && reading.held >= PUFF_RUNNING_MIN
      );
      return at >= 0 ? { at, reading: seen[at] } : null;
    });
    const seen = readings();
    check(
      'C',
      "the switchable emitter's particle count climbs while it runs",
      Boolean(filled),
      filled
        ? `it reached ${filled.reading.held} particles`
        : `no heartbeat reported ${PUFF_RUNNING_MIN} or more particles under a ` +
            `running emitter in ${seen.length} line(s); the highest was ` +
            `${Math.max(0, ...seen.map((r) => r.held))}`
    );

    // ---- and the control: the emitter, stopped ---------------------------
    // Strictly *after* the line above, so this is the same emitter coming down
    // rather than the moment before it ever started.
    const drained = filled
      ? await until(async () =>
          readings()
            .slice(filled.at + 1)
            .find((reading) => !reading.emitting && reading.held === 0)
        )
      : null;
    const tail = filled ? readings().slice(filled.at + 1) : [];
    check(
      'C',
      'and falls back to nothing once its emitter stops',
      Boolean(drained),
      drained
        ? `it drained to 0 after ${tail.length} further heartbeat(s)`
        : `the emitter never reported an empty stopped state in ` +
            `${tail.length} reading(s) after it filled; the lowest it reached ` +
            `while stopped was ` +
            `${Math.min(Infinity, ...tail.filter((r) => !r.emitting).map((r) => r.held))}`
    );

    // ---- the greedy effect, held ------------------------------------------
    const budgeted = readings();
    const over = budgeted.find((reading) => reading.greedy > reading.share);
    const saturated = budgeted.filter(
      (reading) => reading.greedy === reading.share
    );
    check(
      'C',
      'the greedy effect holds at its pool share',
      !over && saturated.length > 0 && budgeted.length > 0,
      over
        ? `it reached ${over.greedy} particles against a share of ${over.share}`
        : saturated.length === 0
          ? `it never filled its share, so the clamp was never tested: the ` +
            `highest of ${budgeted.length} reading(s) was ` +
            `${Math.max(0, ...budgeted.map((r) => r.greedy))} against a share of ` +
            `${budgeted[0]?.share ?? '?'}`
          : `held at ${saturated[0].share} across ${budgeted.length} heartbeat(s)`
    );

    // ---- and the control: it is being refused, not idle --------------------
    const first = budgeted[0];
    const last = budgeted[budgeted.length - 1];
    const climbed = Boolean(first && last && last.refused > first.refused);
    check(
      'C',
      'and its emitter is being refused rather than merely idle',
      climbed,
      first && last
        ? `the refusal counter went ${first.refused} → ${last.refused} over ` +
            `${budgeted.length} heartbeat(s)`
        : 'no heartbeat carried a refusal counter at all'
    );
  }

  // **AND THE DOCUMENT'S OWN CLIP PLAYING, WHICH THE CHECK ABOVE CANNOT SEE.**
  // Only viewer has one. Every other demo in this file advances because its
  // *simulation* does; this one opens a file and shows it, and what has to
  // advance is the animation inside that file — sampled, composed down the
  // joint hierarchy and reported as a distance the pose is from rest. Nothing
  // on the JS side can move it, and neither can the turntable the check above
  // reads: `pose` comes off the palette and would sit at `0.00` through a
  // whole run of a page whose skeleton was never posed.
  //
  // **IT RUNS BEFORE THE DROP BLOCK BELOW, AND THAT IS LOAD-BEARING.** The
  // document dropped there has no skin and no clip, so from that moment on
  // `pose` is `0.00` and can never take a second value again.
  if (EXPECTED.playing) {
    const posed = await until(async () => {
      const seen = valuesOf(EXPECTED.playing);
      return seen.size > 1 ? seen : null;
    });
    const beats = hud().length - beforeLaunch;
    check(
      'C',
      EXPECTED.playingLabel,
      Boolean(posed),
      posed
        ? `it took ${posed.size} values: ${[...posed].join(', ')}`
        : `it never changed — ${beats} HUD line(s) since the start, ` +
            `${valuesOf(EXPECTED.playing).size} value(s): ` +
            `${[...valuesOf(EXPECTED.playing)].join(', ') || 'none'}`
    );
  }

  // **AND THE GEOMETRY BEING DEFORMED, WHICH THE CHECK ABOVE CANNOT SEE
  // EITHER.** `pose` is composed on the CPU and printed; the palette reaching a
  // skinning dispatch, the dispatch running, and a draw reading what it wrote
  // are three further steps, and a page that lost any of them prints the same
  // sweep while drawing the document's bind pose for ever.
  //
  // **IT RUNS BEFORE THE DROP BLOCK, FOR THE REASON THE CHECK ABOVE DOES**: the
  // document dropped there has no skin, so nothing below this point in the run
  // has any skinned geometry to look at.
  if (EXPECTED.deforming) {
    // **The viewer's own control, not a switch this harness reaches past it.**
    // `apps/viewer/src/app.rs` latches the turntable off the first time
    // `Controls::is_dragging` is true, which is a press *and* a movement — a
    // click alone is not a drag, which is exactly what the focus click above
    // relies on.
    //
    // A few pixels, and from the same corner the focus click used. A large drag
    // swings the orbit somewhere the crate may not be on screen at all, and the
    // failure that produces is a canvas that stops changing for a reason that
    // has nothing to do with skinning.
    //
    // **What the angle was doing before the drag, because a turntable that was
    // never running makes this check pass for free.** The check below asserts
    // that two consecutive lines report the same angle, and a stalled turntable
    // satisfies that without the drag doing anything at all — which is what
    // happened on the macOS runner, where the turntable check above failed and
    // this one passed on the same frozen reading. Recorded here rather than
    // inferred from that check's verdict: the driver keeps going after a
    // failure, so its result is not in hand.
    const turningBefore = new Set(
      hud()
        .map((line) => line.match(EXPECTED.moving)?.[1])
        .filter(Boolean)
    );

    const grip = { x: rect.x, y: rect.y };
    await page.send('Input.dispatchMouseEvent', {
      type: 'mousePressed',
      x: grip.x,
      y: grip.y,
      button: 'left',
      clickCount: 1,
      buttons: 1,
    });
    for (let step = 1; step <= TURNTABLE_DRAG_STEPS; step += 1) {
      await page.send('Input.dispatchMouseEvent', {
        type: 'mouseMoved',
        x: grip.x + step * TURNTABLE_DRAG_STEP_PX,
        y: grip.y,
        button: 'left',
        buttons: 1,
      });
    }
    await page.send('Input.dispatchMouseEvent', {
      type: 'mouseReleased',
      x: grip.x + TURNTABLE_DRAG_STEPS * TURNTABLE_DRAG_STEP_PX,
      y: grip.y,
      button: 'left',
      clickCount: 1,
      buttons: 0,
    });

    // **The drag has to have landed before anything below means anything.** The
    // events are queued and the next heartbeat may still come from a frame the
    // turntable was running in, so the sampling window starts only once two
    // consecutive lines report the same angle — which is the observable that
    // says it stopped, and the same one the check then requires to hold.
    const held = await until(async () => {
      const lines = hud();
      const last = lines.at(-1)?.match(EXPECTED.moving)?.[1];
      const previous = lines.at(-2)?.match(EXPECTED.moving)?.[1];
      return last && previous && last === previous ? last : null;
    });
    check(
      'C',
      'a drag takes the turntable out of the picture',
      Boolean(held) && turningBefore.size > 1,
      turningBefore.size <= 1
        ? `the turntable was already still before the drag — it reported ` +
            `${turningBefore.size} angle(s) (${[...turningBefore].join(', ') || 'none'}) ` +
            `across ${hud().length} HUD line(s)${heldNote()} — so stopping it proves ` +
            `nothing and the check below is measuring a camera that was never moving`
        : held
          ? `turn held at ${held}, having taken ${turningBefore.size} angle(s) before the drag`
          : `no two consecutive HUD lines reported the same angle after the drag, so the ` +
            `turntable never stopped and the check below cannot tell a deformed mesh ` +
            `from a moving camera`
    );
    const still = hud().length;

    // Sampled until all three hold at once, rather than for a fixed spell: the
    // clip is under two seconds long and the heartbeat is a simulated second, so
    // waiting for `DEFORM_BEATS` lines is a window that covers a whole cycle on
    // a fast machine and stretches by itself on a slow one, where the beats are
    // what get longer.
    const hashes = new Set();
    let samples = 0;
    const readings = () => {
      const lines = hud().slice(still);
      const distinct = (pattern) =>
        new Set(lines.map((line) => line.match(pattern)?.[1]).filter(Boolean));
      return {
        lines: lines.length,
        turns: distinct(EXPECTED.moving),
        poses: distinct(EXPECTED.playing),
      };
    };
    const changing = () => hashes.size >= samples * DEFORM_CHANGING_SHARE;
    const deformed = await until(async () => {
      const sample = await evaluate(page, SAMPLE_CANVAS('#canvas'));
      if (sample) {
        hashes.add(sample.hash);
        samples += 1;
      }
      const { lines, turns, poses } = readings();
      return lines >= DEFORM_BEATS &&
        turns.size === 1 &&
        poses.size > 1 &&
        changing()
        ? { lines, turns, poses }
        : null;
    });
    const { lines, turns, poses } = deformed ?? readings();
    const counted = `${hashes.size} distinct frame(s) in ${samples} sample(s) over ${lines} beat(s)`;
    check(
      'C',
      EXPECTED.deformingLabel,
      Boolean(deformed),
      deformed
        ? `${counted}, with turn held at ${[...turns].join(', ')} and pose taking ` +
            `${poses.size} values: ${[...poses].join(', ')}`
        : `${counted}; turn took ${turns.size} value(s) ` +
            `(${[...turns].join(', ') || 'none'}) and pose took ${poses.size} ` +
            `(${[...poses].join(', ') || 'none'}) — ` +
            (turns.size !== 1
              ? 'the camera never stopped, so this could not have been a check about geometry'
              : poses.size <= 1
                ? 'the clip stopped playing, so there was nothing for the mesh to be ' +
                  'deformed by'
                : 'the clip played and the camera stood still and the picture did not keep ' +
                  'changing: the geometry on screen is not following the palette. A single ' +
                  'frame means the draw resolved a base vertex the dispatch never wrote to ' +
                  '— GpuInstance::BASE_VERTEX_OVERRIDE is what points it at the region')
    );
  }

  // **A FILE THE VISITOR CHOSE, WHICH IS THE OTHER WAY INPUT REACHES THIS
  // ENGINE.** Only viewer has one — `apps/viewer/src/web.rs`'s drop target,
  // `docs/plan/sample/05-viewer.md`'s V-F5 — and it belongs in this group for
  // the group's own reason: everything above is a pointer or a key arriving
  // through the browser's input pipeline and changing what the engine says
  // about itself, and a dropped document is the same claim about a different
  // pipeline.
  //
  // **IT RUNS LAST IN THE GROUP, AND THAT IS LOAD-BEARING.** `EXPECTED.waiting`
  // asks for the demo document's own `instances: 3`, and the whole point of
  // this block is that the page stops showing that document — so every check
  // that reads `waiting` has to have read it already. Nothing after group C
  // does: `EXPECTED.backdrop` is absent for this demo, and the `touch` rows
  // that read it belong to demos that have no `drop`.
  //
  // The events are dispatched from page script rather than through
  // `Input.dispatchDragEvent`, because CDP's drag interception needs a real
  // file on the browser's own filesystem and the document under test is built
  // in this process. What is dispatched is the event the handler listens for,
  // carrying a `DataTransfer` with a real `File` in it, which is what a browser
  // delivers.
  if (EXPECTED.drop) {
    // **A drag the page does not claim is a tab that navigates to the file**,
    // taking the canvas, the device and the log with it. `dragover` is the
    // event that decides it, so it is the one asked about; the frame lighting
    // up is the visitor's half of the same answer.
    const dragged = await evaluate(
      page,
      `(() => {
         const canvas = document.getElementById('canvas');
         const stage = canvas.closest('.stage');
         const event = new DragEvent('dragover', {
           dataTransfer: new DataTransfer(), bubbles: true, cancelable: true });
         canvas.dispatchEvent(event);
         const during = stage ? getComputedStyle(stage).borderColor : '';
         canvas.dispatchEvent(new DragEvent('dragleave', {
           dataTransfer: new DataTransfer(), bubbles: true, cancelable: true }));
         const after = stage ? getComputedStyle(stage).borderColor : '';
         return { prevented: event.defaultPrevented, during, after };
       })()`
    );
    check(
      'C',
      'a file dragged over the canvas is claimed rather than navigated to',
      dragged?.prevented === true,
      dragged?.prevented === true
        ? 'dragover was cancelled'
        : 'dragover was not cancelled, so dropping a file here would replace the page with it'
    );
    check(
      'C',
      'the frame says it will take the file, and stops saying it',
      Boolean(dragged) && dragged.during !== dragged.after,
      Boolean(dragged) && dragged.during !== dragged.after
        ? `${dragged.during} while over the canvas, ${dragged.after} after`
        : `the border was ${dragged?.after ?? 'unreadable'} throughout — a ` +
            'visitor holding a file over the canvas is told nothing'
    );

    // The document goes across as base64 because that is what survives being
    // spliced into an expression string; the page turns it back into the bytes
    // this process built.
    /**
     * @param {string} name
     * @param {Buffer} bytes
     * @returns {Promise<boolean>} whether the handler claimed the drop
     */
    const dropFile = async (name, bytes) =>
      evaluate(
        page,
        `(() => {
           const bytes = Uint8Array.from(atob(${JSON.stringify(bytes.toString('base64'))}),
                                         (ch) => ch.charCodeAt(0));
           const carried = new DataTransfer();
           carried.items.add(new File([bytes], ${JSON.stringify(name)}));
           const event = new DragEvent('drop', {
             dataTransfer: carried, bubbles: true, cancelable: true });
           document.getElementById('canvas').dispatchEvent(event);
           return event.defaultPrevented;
         })()`
      );
    const detailLine = () =>
      evaluate(page, `document.getElementById('detail')?.textContent ?? ''`);

    const beforeDrop = hud().length;
    await dropFile(EXPECTED.drop.name, EXPECTED.drop.document);
    const dropped = await until(async () =>
      hud()
        .slice(beforeDrop)
        .find((line) => EXPECTED.drop.opened(line))
    );
    check(
      'C',
      'a dropped document replaces the one the page opened with',
      Boolean(dropped),
      dropped
        ? dropped.trim()
        : `no heartbeat in ${TIMEOUT_MS} ms said ${EXPECTED.drop.openedLabel} — ` +
            `the last of ${hud().length - beforeDrop} since the drop was ` +
            `"${(hud().at(-1) ?? 'none').trim()}", and the status bar says ` +
            `"${await detailLine()}"`
    );
    const said = await until(async () => {
      const text = await detailLine();
      return text.includes(EXPECTED.drop.name) && text.includes('instance(s)')
        ? text
        : null;
    });
    check(
      'C',
      'the page tells the visitor what became of the file they dropped',
      Boolean(said),
      said ??
        `the status bar says "${await detailLine()}", which does not name ` +
          `${EXPECTED.drop.name} and what it came to`
    );

    // **AND BYTES THAT ARE NOT A DOCUMENT MUST NOT TAKE THE PAGE DOWN.** The
    // native viewer's whole claim is that a file either loads or says why not,
    // and a browser owes a visitor the same — except that a page has no exit
    // code, so "says why not" means the frame already on screen keeps drawing
    // and the status bar carries the loader's own sentence. Asked here rather
    // than left to group D's "no uncaught exception": that check passes just as
    // happily against a page that stopped drawing.
    const beforeBroken = hud().length;
    await dropFile(EXPECTED.drop.brokenName, EXPECTED.drop.broken);
    // **THE ENGINE'S ANSWER, NOT THE PAGE'S ACKNOWLEDGEMENT.** Every line
    // `web/demos/viewer/main.js` writes about a drop names the file too — it is
    // saying it has handed the bytes over, or that nothing came back — so a
    // check that only looked for the name passes against a page whose engine
    // never answered at all. It did: under a `take_dropped_document` sabotaged
    // to return nothing, this check stayed green on "broken.glb was handed over
    // and no frame has opened it."
    //
    // What is asked for instead is `LoadError`'s own shape, which is the file
    // and then what was wrong with it — `apps/viewer/src/model.rs` writes every
    // one of its variants that way, and no line the page composes for itself
    // begins with the name.
    const refused = await until(async () => {
      const text = await detailLine();
      return text.startsWith(`${EXPECTED.drop.brokenName}:`) ? text : null;
    });
    check(
      'C',
      'a file that is not a document is refused in a sentence, not a crash',
      Boolean(refused),
      refused ??
        `the status bar says "${await detailLine()}", which is not the ` +
          `loader's own account of ${EXPECTED.drop.brokenName}`
    );
    const survived = await until(async () =>
      hud()
        .slice(beforeBroken)
        .filter((line) => EXPECTED.drop.opened(line)).length > 1
        ? hud().at(-1)
        : null
    );
    check(
      'C',
      'the document already on screen is still there and still drawing',
      Boolean(survived),
      survived
        ? survived.trim()
        : `fewer than two heartbeats since the refusal still said ` +
            `${EXPECTED.drop.openedLabel} — the page either stopped drawing or ` +
            `lost the document it had, and the last line was ` +
            `"${(hud().at(-1) ?? 'none').trim()}"`
    );
  }

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
  // **The beat is measured, not assumed.** Waiting for two lines and timing the
  // gap is what makes every window below fit this machine: a desktop answers in
  // well under the floor and nothing changes, and a runner whose frames are
  // slow enough to stretch a simulated second past four wall seconds gets a
  // window that still holds a beat. Two lines rather than one because an
  // interval needs two ends — the wait for the first one absorbs whatever phase
  // the run happens to start in.
  const heartbeatMs = async () => {
    const start = hud().length;
    const began = Date.now();
    const first = await until(
      async () => (hud().length > start ? Date.now() : null),
      Math.min(HEARTBEAT_DEADLINE_MS, TIMEOUT_MS)
    );
    if (first === null) return null;
    const second = await until(
      async () => (hud().length > start + 1 ? Date.now() : null),
      Math.min(HEARTBEAT_DEADLINE_MS, TIMEOUT_MS)
    );
    return second === null
      ? null
      : { beat: second - first, waited: second - began };
  };

  // The control, and it is what makes every check after it mean something: a
  // tick loop that emits nothing fails here, loudly, instead of the pause check
  // passing for free on a run where no heartbeat could have appeared anyway.
  const beat = await heartbeatMs();

  // **How far behind real time this machine is running the demo**, and the one
  // number every later budget is scaled by. `TICK_WINDOW_MS` was not the only
  // constant calibrated on a fast desktop — `PADDLE_SETTLE_MS` is 1500 ms for a
  // batch fold plus a log drain, and on a runner advancing a simulated second
  // every 27 seconds it expired before the two lifts it waits for arrived, which
  // is one red CI run per constant if they are fixed one at a time.
  //
  // Never below 1, so a machine that keeps up is unchanged and no budget can
  // come out *shorter* than the constant already gave it — that direction would
  // weaken every check watching nothing happen. A machine faster than nominal
  // therefore reports `1.0x` rather than a fraction: the ratio is a curiosity,
  // the factor actually applied is what a reader needs when a later check fails.
  // `beat === null` means the control below has already failed, so the run is
  // red whatever this says.
  const nominalBeat = EXPECTED.beatMs ?? NOMINAL_BEAT_MS;
  const slowdown = beat ? Math.max(1, beat.beat / nominalBeat) : 1;
  const budget = (ms) => Math.round(ms * slowdown);

  check(
    'E',
    'a running demo logs its HUD from inside the tick',
    beat !== null,
    beat === null
      ? `no second HUD line in ${Math.min(HEARTBEAT_DEADLINE_MS, TIMEOUT_MS)} ms`
      : `two HUD lines ${beat.beat} ms apart, in ${beat.waited} ms — ` +
          `every later budget scaled ${slowdown.toFixed(1)}x`
  );

  // **Never shorter than the constant used to give.** `TICK_WINDOW_MS` is the
  // floor, so a fast machine watches exactly as long as it did before this was
  // adaptive; a slow one watches longer. Shortening it on a fast machine would
  // make "a paused demo runs no ticks at all" easier to satisfy, which is the
  // one direction this change must not move.
  const windowMs = Math.max(
    TICK_WINDOW_MS,
    (beat?.beat ?? 0) * TICK_WINDOW_BEATS
  );

  const heartbeats = async () => {
    const before = hud().length;
    await pause(windowMs);
    return hud().length - before;
  };

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
    `${whilePaused} HUD line(s) in ${windowMs} ms`
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
    `${afterResume} HUD line(s) in ${windowMs} ms`
  );

  group('F — a finger');

  // **Every wall-clock budget below is this machine's, not this desktop's.**
  // Each of these was a bare constant until 2026-08-20, chosen where the
  // slowdown is 1, and `PADDLE_SETTLE_MS`'s 1500 ms was the one that failed on a
  // GitHub runner: the two contacts it waits to see lift had not been logged
  // yet. Scaling them all by the measured `slowdown` is what stops the next one
  // costing its own red run. `TICK_WINDOW_MS`'s two uses here take `windowMs`,
  // which is the same number arrived at one step earlier.
  //
  // **`TAP_INTERVAL_MS` is deliberately not scaled.** It is the gap between two
  // synthetic taps — an input cadence rather than a budget for observing a
  // result — and the loops that tap are bounded by `climbMs` and `lifeMs`, so a
  // slow machine already gets *more* taps rather than fewer. Untested at a
  // slowdown above 1: if a tap sequence ever fails on a slow runner where the
  // deadline plainly did not expire, coalescing is the first thing to suspect.
  const settleMs = budget(PADDLE_SETTLE_MS);
  const lifeMs = budget(LIFE_MS);
  const climbMs = budget(CLIMB_MS);
  const walkMs = budget(WALK_MS);

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
  }, settleMs);
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
      }, settleMs);
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
      await pause(settleMs);
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
      lifeMs
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
        lifeMs
      );
      const afterCancel = fresh().length;
      await tap(spot(PARK_X, PADDLE_BAND));
      const servedAfterCancel = await until(
        async () => fresh().slice(afterCancel).find(EXPECTED.started),
        lifeMs
      );
      check(
        'F',
        'a tap after a cancelled gesture still serves',
        Boolean(lostOne) && Boolean(servedAfterCancel),
        lostOne
          ? (servedAfterCancel ?? 'the tap raised no edge').trim()
          : `no life was lost inside ${lifeMs} ms, so there was never a tap to make`
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
        lifeMs
      );
      const afterSecond = fresh().length;
      await tap(spot(PARK_X, PADDLE_BAND));
      const servedAgain = await until(
        async () => fresh().slice(afterSecond).find(EXPECTED.started),
        lifeMs
      );
      check(
        'F',
        'a second tap in a row serves it again',
        Boolean(lostTwo) && Boolean(servedAgain),
        lostTwo
          ? (servedAgain ?? 'the second tap raised no edge').trim()
          : `no second life was lost inside ${lifeMs} ms`
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
      }, climbMs);
      check(
        'F',
        'a tap lifts the bird',
        Boolean(climbed),
        climbed
          ? `y reached ${climbed.above}, over the ${bar} one flap could manage`
          : `y never passed ${bar} in ${climbMs} ms of tapping, which is ` +
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
        walkMs
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
      }, walkMs);
      check(
        'F',
        'a thumb on the field walks the wizard',
        walked !== null,
        walked === null
          ? `x stayed at ${start} for ${walkMs} ms with a thumb pushing right`
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
        }, walkMs);
        // **Only the second finger lifts**, which is what makes the pause the
        // *second* one's doing. `Input.dispatchTouchEvent`'s `touchEnd` takes
        // the points being **released** — an empty list is the "release
        // everything" every other gesture in this file uses, and naming one
        // point lifts that one and leaves the rest of the hand where it is.
        await touch('touchEnd', [contact(pauseSpot, 2)]);
        const paused = await until(async () => {
          const status = await evaluate(page, `crcbl.status()`);
          return status === STATUS_PAUSED ? status : null;
        }, walkMs);
        const stoppedAt = wizardX();
        await pause(windowMs);
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
        }, walkMs);
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
        }, walkMs);
        check(
          'F',
          'the thumb that never lifted walks again once the panel has gone',
          walkedAgain !== null,
          walkedAgain === null
            ? `x stayed at ${regrabbed} for ${walkMs} ms with a thumb that ` +
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
        }, walkMs);

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
        await pause(windowMs);
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
      }, lifeMs);
      check(
        'F',
        'a tap on the pause button stops the run',
        pausedByButton === STATUS_PAUSED,
        pausedByButton === STATUS_PAUSED
          ? `status ${pausedByButton} after a tap ${PAUSE_INSET} px inside the corner`
          : `status ${await evaluate(page, `crcbl.status()`)} — the corner ` +
              `was tapped for ${lifeMs} ms and the loop never stopped`
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

    // **THE DEMO IS PUT IN PLAY FIRST, AND THAT IS NOT LENIENCY.** A game decides
    // for itself what covers its clear colour, and by this point in the run it
    // has been played: flappy's bird is usually dead and its death screen dims
    // the whole sky. Read in that state the check went red on the Pages run of
    // 2026-08-20 with `rgb(63,105,141)` against the expected `rgb(107,173,229)`,
    // and the numbers say what that was — a **uniform 0.61 multiply**, an
    // overlay. A transfer-function error is a power curve, and the expected
    // colour decoded is `rgb(37,107,200)`, nothing like what arrived. Six
    // consecutive samples here printed `[HUD] Dead score: 0` beside that colour.
    //
    // Pressing the demo's own start key until its own `started` line appears is
    // the same state group C establishes, and it makes this the *only* check
    // here that does not race the simulation. It cannot hide a broken encode: a
    // run with no encode shows the linear colour on a live frame too, which is
    // what `unencoded` in the row below is compared against.
    //
    // Rebooting the page instead was tried and is worse — `crcbl.status()`
    // reaches RUNNING before the first frame is presented, so the sample is an
    // all-black canvas, and breakout's clear is only uncovered once its start
    // menu has been dismissed.
    if (EXPECTED.started) {
      let playing = false;
      for (
        let attempt = 0;
        attempt < BACKDROP_PLAY_ATTEMPTS && !playing;
        attempt += 1
      ) {
        if (!EXPECTED.started(hud().at(-1) ?? '')) await pressStartKey();
        playing = Boolean(
          await until(
            async () => EXPECTED.started(hud().at(-1) ?? '') || null,
            budget(BACKDROP_PLAY_MS)
          )
        );
      }
      check(
        'G',
        'the demo is in play when its clear colour is read',
        playing,
        playing
          ? (hud().at(-1) ?? '').trim().slice(-72)
          : `the demo never reported its started state in ` +
              `${BACKDROP_PLAY_ATTEMPTS} presses, so whatever the sample below ` +
              `reads is whichever menu it was left on`
      );
    }

    const { source, encoded, unencoded, share } = EXPECTED.backdrop;
    let best = null;
    for (let i = 0; i < BACKDROP_SAMPLES; i += 1) {
      const sample = await evaluate(
        page,
        SAMPLE_BACKDROP('#canvas', encoded, unencoded)
      );
      if (sample && (!best || sample.encoded > best.encoded)) best = sample;
      if (best && best.encoded >= share) break;
      // Spaced so consecutive samples are different *frames*: eight
      // `toDataURL` calls back to back span a few milliseconds and are eight
      // looks at one of them.
      await pause(budget(BACKDROP_INTERVAL_MS));
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

  // **THREE OF THE CHECKS ABOVE ASSERT A SILENCE, AND A CLOSED CHANNEL REPORTS
  // SILENCE TOO.** Group A's "the page raised no uncaught exception", group B's
  // "every asset the page asked for exists" and group D's "the browser reported
  // no WebGPU device errors" each read an array that a listener somewhere is
  // supposed to fill. A listener that was never attached, a filter that swallows
  // everything, a server that stopped recording its 404s: every one of those
  // presents as an empty array, which is exactly what a passing run looks like.
  //
  // That is not hypothetical. `crcbl-vk`'s suites asserted the validation layer
  // had been silent across a 66,836-line job log in which the layer's own
  // deliberate-violation test also produced nothing, because no `log::Log` was
  // installed — the whole suite green and proving nothing.
  // `vk_e2e::validation_gate::a_deliberate_violation_is_caught_by_the_layer` is
  // the fix and the precedent for this group: commit the violation, then assert
  // the channel noticed.
  //
  // **IT RUNS LAST, AND THAT IS THE WHOLE OF ITS PLACEMENT.** Every provocation
  // below deliberately dirties one of the three arrays, so all of them have to
  // happen after the last check that reads one. It needs no `EXPECTATIONS` row
  // either, and deliberately: nothing here is about the game, so every demo
  // makes all three claims.
  //
  // **WHAT READS THOSE ARRAYS AFTERWARDS.** `pageErrors` and `site.misses` have
  // no reader past their own check, so a deliberate entry in either is inert.
  // `deviceErrors` has one — the failure report at the bottom of this file
  // prints it in full whenever any check fails. It cannot turn a run red, since
  // only `checks` decides that, but it would print this group's own error beside
  // real ones, so that reader filters [`PROVOCATION`] out and says so there.
  // **Nothing is removed from any of the three**: groups A, B and D have already
  // read them, and an array edited behind a check that has run is the shape of
  // trick this group exists to rule out.
  group('H — the reporting channels are open');

  // **Thrown from a timer rather than from the `evaluate` itself.** An
  // expression that throws comes back as this file's own rejection —
  // [`evaluate`] turns `exceptionDetails` into an `Error` — and never reaches
  // the page's uncaught channel at all. A `setTimeout` callback has no caller to
  // catch it, so it goes where a real bug in the shim would go:
  // `Runtime.exceptionThrown`, which is the event the run subscribed to when it
  // opened the page.
  const errorsBefore = pageErrors.length;
  const thrown = `${PROVOCATION} page exception`;
  await evaluate(
    page,
    `(setTimeout(() => { throw new Error(${JSON.stringify(thrown)}); }, 0), true)`
  );
  const raised = await until(
    async () =>
      pageErrors.length > errorsBefore ? pageErrors.slice(errorsBefore) : null,
    PROVOCATION_MS
  );
  check(
    'H',
    'a deliberate uncaught exception reaches the page-error channel',
    raised !== null && raised.some((line) => String(line).includes(thrown)),
    raised === null
      ? `nothing arrived in ${PROVOCATION_MS} ms — group A's "the page raised ` +
          'no uncaught exception" is reading an array nothing fills'
      : raised.some((line) => String(line).includes(thrown))
        ? String(raised.find((line) => String(line).includes(thrown)))
            .split('\n')[0]
            .trim()
        : `${raised.length} entr(y/ies) arrived and none names "${thrown}": ` +
          `${String(raised[0]).split('\n')[0].trim()}`
  );

  // A path under the demo's own directory, so the request goes to the server the
  // page's assets come from and `serve.mjs` records it in the same `misses`
  // array group B reads. **Not a `favicon.ico`**: [`isRealMiss`] drops that one
  // name, and a provocation the check under test filters back out would be a
  // control its own filter could swallow — so the name is deliberately chosen to
  // survive that predicate, and the predicate is applied here to prove it did.
  const missesBefore = site.misses.length;
  const missingAsset = `${PROVOCATION}-missing-asset.bin`;
  const missStatus = await evaluate(
    page,
    `fetch(${JSON.stringify(missingAsset)}).then((response) => response.status)`
  );
  const recorded = await until(
    async () =>
      site.misses.length > missesBefore
        ? site.misses.slice(missesBefore).filter(isRealMiss)
        : null,
    PROVOCATION_MS
  );
  const missSeen =
    recorded?.filter((path) => path.endsWith(missingAsset)) ?? [];
  check(
    'H',
    'a request for an asset that is not there is recorded as a miss',
    missStatus === 404 && missSeen.length > 0,
    missStatus !== 404
      ? `the server answered ${missStatus} for ${missingAsset}, so nothing was ` +
          'missing and this control provoked nothing'
      : missSeen.length > 0
        ? missSeen[0]
        : `the fetch 404'd and ${recorded?.length ?? 0} miss(es) were recorded ` +
          `in ${PROVOCATION_MS} ms, none of them ${missingAsset} — group B's ` +
          '"every asset the page asked for exists" is reading an array nothing fills'
  );

  // **A device this page opens for the purpose, and not the engine's.**
  // `deviceErrors` is filled from one place: the `Log.entryAdded` handler above,
  // for an entry whose `source` is `rendering` — Chrome's own report of an
  // uncaptured WebGPU error, whichever device in this page raised it. So a
  // throwaway device exercises the whole of what that array reads, from Dawn's
  // validation through the browser's console to the handler and the array.
  //
  // Provoking the *engine's* device instead was the other candidate and is
  // rejected on purpose. `crcbl.gpu.replayer.device` is reachable from here, but
  // an error on it also travels a second path: `gpu-replay.js`'s
  // `uncapturederror` listener files it in the replayer's error log, wasm drains
  // that through `Command::TakeError`, and `Gpu::acquire` in
  // `crates/crcbl/src/engine.rs` turns the next frame into a `GpuError` — so the
  // control would end the run by taking the demo to FAILED, and the canvas and
  // page log written a few lines below would be evidence for a demo the gate
  // broke itself.
  //
  // **WHAT THIS THEREFORE DOES NOT PROVE**: that the engine's own device is
  // reported through the same console. It would not be if something called
  // `preventDefault()` on the `uncapturederror` event; nothing does — that
  // listener in `web/engine/gpu-replay.js` only files the message — and there is
  // no other listener on that device. That reasoning is the seam this control
  // leaves uncovered, and reading the listener is the whole of the evidence for
  // it.
  //
  // **THE TICK IS LOAD-BEARING.** Dawn queues an uncaptured error and dispatches
  // it when the device next ticks, so a device given no work does not report for
  // seconds — measured here at over four, and at nothing at all inside the
  // window this check would have allowed it. `onSubmittedWorkDone` is a tick
  // with nothing else attached to it: with the await in place the entry arrived
  // 2–4 ms later on every run. **And no `pushErrorScope`**, which is the other
  // half: an error inside a scope is *captured*, so the console never hears
  // about it and this control would provoke a silence of its own.
  const deviceErrorsBefore = deviceErrors.length;
  const badBuffer = `${PROVOCATION} device error`;
  const provoked = await evaluate(
    page,
    `(async () => {
       const adapter = await navigator.gpu.requestAdapter();
       if (!adapter) return 'no adapter';
       const device = await adapter.requestDevice({
         label: ${JSON.stringify(`${PROVOCATION} control device`)},
       });
       // MAP_READ may only be combined with COPY_DST and MAP_WRITE only with
       // COPY_SRC, so the pair is a validation error in the call itself — no
       // command is recorded and nothing is submitted, which is the same reason
       // the Vulkan gate provokes its layer with a copy it throws away.
       device.createBuffer({
         label: ${JSON.stringify(badBuffer)},
         size: 4,
         usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.MAP_WRITE,
       });
       await device.queue.onSubmittedWorkDone();
       return 'provoked';
     })()`
  );
  const reported = await until(
    async () =>
      deviceErrors.length > deviceErrorsBefore
        ? deviceErrors.slice(deviceErrorsBefore)
        : null,
    PROVOCATION_MS
  );
  const named = reported?.filter((text) => text.includes(badBuffer)) ?? [];
  check(
    'H',
    'a deliberate WebGPU validation error reaches the device-error channel',
    provoked === 'provoked' && named.length > 0,
    provoked !== 'provoked'
      ? `no device to provoke: ${provoked}`
      : named.length > 0
        ? named[0].split('\n')[0].trim()
        : `${reported?.length ?? 0} device error(s) arrived in ` +
          `${PROVOCATION_MS} ms and none names "${badBuffer}" — group D's ` +
          '"the browser reported no WebGPU device errors" is reading an array ' +
          'nothing fills'
  );

  // Written whatever the outcome: a black PNG is the evidence for a failure and
  // the first thing a human will ask for. The canvas itself rather than a
  // viewport screenshot — the page's chrome is not what is under test.
  //
  // **BEFORE GROUP I AND NOT AFTER**, because that group stops the demo: a
  // shot taken past it is a picture of a torn-down game, which is the one
  // frame nobody is asking about. The page log is written after it instead,
  // so that whatever the teardown says lands in the file too.
  const png = await evaluate(
    page,
    `document.getElementById('canvas').toDataURL().slice(22)`
  );
  const shotPath = join(OUT, `${SLUG}-${chosen.mode}.png`);
  writeFileSync(shotPath, Buffer.from(png, 'base64'));

  group('I — the demo lets go of what it took');

  // `crcbl-vk`'s teardown warning, asked of the browser side.
  //
  // `crates/crcbl-vk/src/device.rs`'s `Drop for DeviceInner` names every object
  // a caller never destroyed, by kind and count, and the vk e2e runners fail on
  // that line rather than logging it. `crcbl-dx12` and `crcbl-mtl` carry the
  // same warning. `crcbl-webgpu` has none — but the browser side of it is a
  // handle table like any other, one per kind, and an object the stream created
  // and never destroyed is a slot still occupied. `Replayer#liveObjects` reads
  // those tables; this is the gate that asks.
  //
  // **IT RUNS FOR EVERY DEMO**, and last, for two reasons of its own. Every
  // demo, because the question is about the engine's own resource discipline
  // rather than about anything one game does — a leak on the acquire path
  // belongs to all of them and a leak in a game's own pipelines belongs to one,
  // and a per-demo count is what tells those apart. Last, because it stops the
  // demo, and every check above needs a demo that is still running.
  const naming = (held) =>
    held.map(({ kind, count }) => `${count} ${kind}`).join(', ');

  // **THE OBSERVATION IS CHECKED BEFORE THE VERDICT IS READ OFF IT.** The
  // failure that costs the most here is not a leak: it is `liveObjects()`
  // answering an empty list because it is reading nothing — a renamed getter, a
  // handle to a replayer that never replayed, an `evaluate` that came back
  // `undefined`. Every one of those reads as a clean teardown, which is the
  // exact shape of check this repository keeps throwing out. So the first thing
  // asked is whether it can see anything at all, of a demo that has been
  // rendering for the whole run and therefore certainly holds a surface, a
  // swapchain and the pipelines it drew with.
  const heldRunning = await evaluate(page, `crcbl.gpu.replayer.liveObjects()`);
  const observing =
    Array.isArray(heldRunning) &&
    heldRunning.length > 0 &&
    heldRunning.every(
      (entry) => typeof entry?.kind === 'string' && entry.count > 0
    );
  check(
    'I',
    'the replayer can say what it is holding while the demo runs',
    observing,
    observing
      ? naming(heldRunning)
      : `liveObjects() answered ${JSON.stringify(heldRunning)} for a demo that ` +
          'has been rendering all run — the leak check below would read that as ' +
          'a clean teardown, so it is not one'
  );

  // **A STEADY-STATE FRAME MUST TAKE NOTHING IT DOES NOT GIVE BACK**, which is
  // the half of this group's question the teardown check cannot ask. A run that
  // hands the replayer one more object every frame and destroys them all at
  // shutdown passes the check below and is still a page that grows without
  // bound for as long as somebody looks at it — which is exactly what the
  // acquire path did until 2026-08-23, minting a fresh image and image-view
  // handle per frame that nothing ever retired. It was measured at 581 of each
  // after one second of breakout, and a shutdown-only gate saw nothing wrong.
  //
  // Counted in **frames rather than seconds**, because a frame is the unit the
  // growth was in and a slow runner would otherwise watch a shorter run. The
  // demo has been drawing for the whole suite by now, so nothing here is
  // warm-up: a kind that climbs at this point is climbing for good.
  // **GROWTH HAS TO BE SUSTAINED, NOT MERELY OBSERVED ONCE.** This used to take
  // one sample, wait, take another, and fail on any kind that had risen. That
  // cannot tell a leak from a fixed-size ring filling up, and one of the kinds
  // here is ring-shaped: `readbacks` is the instantaneous occupancy of
  // `CullStatsRing`, which holds `FRAMES_IN_FLIGHT + 1` slots per renderer and
  // whose `take_slot` will not reuse a slot that is still polling — so it is
  // bounded at three per renderer and legitimately *rises* when a readback's
  // round trip takes more frames, which is exactly what a loaded CI runner
  // does. It failed twice that way (lantern 2 -> 5 against a ceiling of six,
  // quarry 1 -> 2 against three), both strictly inside the bound.
  //
  // **THE WINDOW IS SMALL BECAUSE EACH ONE IS BOUNDED BY `TIMEOUT_MS`, AND THE
  // SLOWEST DEMO SETS THE SIZE.** Every window polls through `until`, so a
  // window that the demo cannot finish inside the poll ceiling fails the check
  // for want of frames rather than for anything it holds on to. quarry draws at
  // roughly two frames every three seconds under SwiftShader on a CI runner —
  // measured, not guessed: a 60-frame window took 91.7 s there against a 90 s
  // ceiling, and the run before this comment failed with `drew fewer than 60
  // more frames`. Three windows of this size cost the same total frames as the
  // single 60-frame window that preceded them, so the step's wall clock is
  // unchanged, while each window now finishes in about a third of its ceiling
  // instead of just over it. Raising it buys no teeth — a leak that mints one
  // object per frame moves the count by the window size, and any size at all
  // makes it climb — so the size exists to fit the ceiling, not to catch more.
  //
  // So the question asked is whether a kind climbs in **every** window rather
  // than in one. A ring saturates and stops; a leak does not, and the leak this
  // check exists for — a fresh image and view minted per frame and never
  // retired — climbs in all of them, at 581 per second. The teeth are the same
  // and the false positive is gone. Per-kind ceilings were the alternative and
  // were not taken: they would put the engine's ring depths in this file, to be
  // silently wrong the day one changed.
  const FRAMES_WATCHED = 20;
  const WINDOWS = 3;
  const replayedNow = () => evaluate(page, `crcbl.gpu.stats().replayed`);
  const countsOf = (held) =>
    new Map((held ?? []).map(({ kind, count }) => [kind, count]));
  const heldSamples = [heldRunning];
  let watched = true;
  for (let window = 0; window < WINDOWS && watched; window += 1) {
    const framesBefore = await replayedNow();
    const drewMore = await until(async () => {
      const now = await replayedNow();
      return typeof now === 'number' && now >= framesBefore + FRAMES_WATCHED
        ? { now }
        : null;
    });
    watched = Boolean(drewMore);
    heldSamples.push(
      watched ? await evaluate(page, `crcbl.gpu.replayer.liveObjects()`) : null
    );
  }
  const readSamples = heldSamples.every((held) => Array.isArray(held));
  // A kind counts as growing only if it rose across every window in turn.
  const grew = readSamples
    ? [...countsOf(heldSamples[heldSamples.length - 1])]
        .filter(([kind]) =>
          heldSamples.every(
            (held, index) =>
              index === 0 ||
              (countsOf(held).get(kind) ?? 0) >
                (countsOf(heldSamples[index - 1]).get(kind) ?? 0)
          )
        )
        .map(
          ([kind]) =>
            `${kind} ${heldSamples
              .map((held) => countsOf(held).get(kind) ?? 0)
              .join(' -> ')}`
        )
    : [];
  check(
    'I',
    'a steady-state frame gives back everything it takes',
    watched && readSamples && grew.length === 0,
    !watched
      ? `the demo drew fewer than ${FRAMES_WATCHED} more frames in ${TIMEOUT_MS} ms, ` +
          'so nothing here is a verdict about what a frame holds on to'
      : !readSamples
        ? `liveObjects() answered ${JSON.stringify(heldSamples.find((held) => !Array.isArray(held)))} on one of the heldSamples`
        : grew.length === 0
          ? `nothing climbed across all ${WINDOWS} windows of ${FRAMES_WATCHED} frames`
          : `over ${WINDOWS} windows of ${FRAMES_WATCHED} frames: ${grew.join(', ')} — a frame that ` +
            'takes one more of something every time is a page that grows for as ' +
            'long as it is open, whatever its teardown then destroys'
  );

  // **THE PAGE'S OWN STOP BUTTON, AND NOT `crcbl.exports`.** `demo.js` wires it
  // to `shell.requestClose()`, which asks the engine to close the way a
  // compositor's close button does on the desktop: the next `api.frame()` runs
  // `Flow::Stop`, tears the loop down and reports STOPPED, and `pumpGpu()` runs
  // *after* that call in the same iteration — so any command the teardown
  // encodes is drained before the loop ends. Reaching past the page to the wasm
  // exports would skip that drain and blame the engine for a frame the shim
  // never delivered.
  //
  // Not `pagehide` either, which is the other teardown `demo.js` has: it calls
  // `api.shutdown()` with no frame left to follow it, so nothing pumps what the
  // teardown wrote and the tables could not empty however clean the engine was.
  await evaluate(page, `(document.getElementById('stop').click(), true)`);
  const settledStop = await until(async () => {
    const status = await evaluate(page, `crcbl.status()`);
    // 4 is STATUS_STOPPED and 5 STATUS_FAILED; both are terminal, so stop
    // asking, and the check below still insists on 4.
    return [4, 5].includes(status) ? { status } : null;
  });
  const shutDown = check(
    'I',
    'the demo shuts down when its own stop button is pressed',
    settledStop?.status === 4,
    settledStop?.status === 4
      ? 'STATUS_STOPPED'
      : settledStop?.status === 5
        ? `status 5 (FAILED) — ${await evaluate(page, `document.getElementById('detail')?.textContent ?? ''`)}`
        : `status ${(await evaluate(page, `crcbl.status()`)) ?? 'unreadable'} after ` +
          `${TIMEOUT_MS} ms — the demo never reached a terminal state`
  );

  const heldAfter = await evaluate(page, `crcbl.gpu.replayer.liveObjects()`);
  const stillHeld = Array.isArray(heldAfter)
    ? heldAfter.reduce((total, entry) => total + entry.count, 0)
    : -1;
  check(
    'I',
    'the demo destroyed every GPU object it created',
    shutDown && stillHeld === 0,
    !shutDown
      ? 'the demo never stopped, so nothing here is a verdict about its teardown'
      : stillHeld === 0
        ? 'every handle table is empty'
        : `${stillHeld} object(s) still held after shutdown (${naming(heldAfter)})` +
          ' — see crcbl-vk\'s "still alive at device teardown" warning for the' +
          ' same finding on the same engine'
  );

  // **THE ENGINE'S OWN REPORTER, ASKED WHETHER IT RAN.** The check above is
  // this harness reading the tables; the one below is about the page reporting
  // for itself, with nothing driving it. `Replayer#replay` warns
  // `N object(s) still alive at device teardown (…)` — the line `crcbl-vk`,
  // `crcbl-dx12` and `crcbl-mtl` each write from their device's destructor, and
  // the line every e2e runner greps — when the stream ends holding anything, and
  // `web/run-browser-e2e.sh` fails this run on it exactly as `run-vk-e2e.sh`
  // does.
  //
  // **THAT GREP CANNOT PROVE ITSELF**, which is the whole reason this check
  // exists. A clean run produces no line, and so does a run where the reporter
  // never fired at all: a `WebGpuDevice::drop` that did not `retain`, a shim
  // that stopped pumping, a final frame that threw. `Replayer#teardownReport` is
  // the receipt that tells those apart — `null` until the stream ends, and then
  // the list it reported on, empty or not. A gate reading only the console is
  // the "green light wired to nothing" this group was built to remove.
  const teardownReport = shutDown
    ? await evaluate(page, `crcbl.gpu.replayer.teardownReport`)
    : null;
  check(
    'I',
    'the engine reported for itself that its command stream ended',
    Array.isArray(teardownReport),
    !shutDown
      ? 'the demo never stopped, so its stream never ended'
      : Array.isArray(teardownReport)
        ? teardownReport.length === 0
          ? 'the stream ended holding nothing, and said so'
          : `the stream ended holding ${naming(teardownReport)}`
        : 'teardownReport is null after a demo that stopped — the stream never ' +
          'reported its own end, so the "still alive at device teardown" line ' +
          'web/run-browser-e2e.sh greps for could never have been written'
  );

  // Lifted out of the page log and onto this run's own output, because the
  // shell wrapper greps what it can see. The page log is written below and is
  // the fuller record; these are the lines that fail the run.
  for (const line of consoleLines.filter((line) =>
    line.includes(TEARDOWN_LEAK)
  ))
    console.log(`web e2e: ${line}`);

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

// **What the run spent its time on**, in the order that matters when a step is
// about to hit a CI timeout. The elapsed is the wall clock from the previous
// check to this one, so it covers the waiting the check did as well as the
// assertion itself — which is the whole of where a browser gate's minutes go.
const slowest = [...checks]
  .sort((a, b) => b.ms - a.ms)
  .slice(0, SLOWEST_REPORTED);
console.log(
  `web e2e: slowest ${slowest.length} of ${checks.length} check(s): ` +
    slowest.map((c) => `${c.name} ${(c.ms / 1000).toFixed(1)}s`).join(', ')
);

if (failed.length) {
  console.error('\nweb e2e: FAILED');
  for (const c of failed)
    console.error(`  ${c.group}: ${c.name}${c.detail ? ` — ${c.detail}` : ''}`);
  // **The one reader of `deviceErrors` past group D's check**, and the reason
  // group H filters rather than deletes. That group provokes a validation error
  // on purpose, on a device it opens for the purpose, and a run that fails
  // anything at all would otherwise print it here among the real ones — the same
  // error the check above reports as a *pass*, reappearing as evidence for a
  // failure. It is filtered out by name; nothing is taken out of the array,
  // because group D has already read it and an array edited behind a check that
  // has run is what this whole exercise is about.
  const realDeviceErrors = deviceErrors.filter(
    (message) => !message.includes(PROVOCATION)
  );
  if (realDeviceErrors.length) {
    console.error('\nweb e2e: WebGPU device errors, in full:');
    for (const message of realDeviceErrors.slice(0, 4)) {
      console.error(
        message
          .split('\n')
          .map((line) => `    ${line}`)
          .join('\n')
      );
    }
    if (realDeviceErrors.length > 4)
      console.error(`    … and ${realDeviceErrors.length - 4} more`);
  }
  process.exit(1);
}

process.exit(exitCode);
