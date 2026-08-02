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
// WHAT IT ASSERTS. Four groups, printed in order:
//
//   A  the platform — `navigator.gpu`, an adapter, and **that this browser can
//      report canvas pixels at all** (see below; this one is not a formality)
//   B  the engine boots — canvas size, wgpu backend, swapchain, STATUS_RUNNING
//   C  input drives the simulation — a real click focuses the canvas, a real
//      Space key launches the ball, and the game's own HUD log line changes
//   D  it renders — no WebGPU device errors, the canvas is not one flat colour,
//      and the canvas changes from frame to frame while the ball is in flight
//
// THE READBACK IS THE PART THAT NEEDED PROVING. Three ways to get a WebGPU
// canvas's pixels into JS look equivalent and are not, measured on Chromium 150
// against a page that does nothing but clear a canvas to a known colour:
//
//   drawImage(canvas, …) + getImageData   transparent black, always
//   createImageBitmap(canvas)             transparent black, always
//   canvas.toDataURL()                    the actual pixels — on a hardware
//                                         adapter; transparent black on
//                                         SwiftShader
//
// The first two are the obvious spellings and both of them report a perfectly
// rendered frame as blank. That is a harness that fails a working engine, which
// is worse than no harness — so group A runs that known-colour clear first, in
// this same browser with these same flags, and refuses to interpret group D
// unless the control comes back with the colour it drew. `docs/plan/ROADMAP.md`
// puts it as "verify the checker, not just the code".
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
  createReadStream,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

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
  },
  flappy: {
    key: 'Space',
    waiting: (line) => line.includes('WaitingToStart'),
    started: (line) => line.includes('Playing'),
    startedLabel: 'Space starts the run',
    startedFailure: 'the state never left WaitingToStart',
    moving: /\bx: (-?[\d.]+)/,
    movingLabel: 'the bird advances after the flap',
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
 * whose pixels cannot be read is no use to this harness, and on Chromium 150
 * SwiftShader is exactly that.
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
// certificate. Node already has an HTTP server, so this is thirty lines rather
// than a dependency.

const MIME = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.png': 'image/png',
  '.wasm': 'application/wasm',
};

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

/** @returns {Promise<{ origin: string, close: () => Promise<void>, misses: string[] }>} */
function serve(root) {
  /** Requests that 404'd. A missing asset is a shim bug, not a warning. */
  const misses = [];
  const server = createServer((request, response) => {
    // Only the path; a query string is not part of a file name.
    const path = decodeURIComponent(
      new URL(request.url, 'http://localhost').pathname
    );
    if (path === CONTROL_PATH || path === `${CONTROL_PATH}/`) {
      response.writeHead(200, {
        'content-type': MIME['.html'],
        'cache-control': 'no-store',
      });
      response.end(CONTROL_PAGE);
      return;
    }
    // `normalize` collapses `..` before the prefix test, so a request for
    // `/../../etc/passwd` cannot escape the site directory.
    const target = normalize(
      join(root, path.endsWith('/') ? `${path}index.html` : path)
    );
    if (!target.startsWith(root)) {
      response.writeHead(403).end('outside the site');
      return;
    }
    let info;
    try {
      info = statSync(target);
    } catch {
      misses.push(path);
      response.writeHead(404).end('not found');
      return;
    }
    if (info.isDirectory()) {
      response.writeHead(301, { location: `${path}/` }).end();
      return;
    }
    const extension = target.slice(target.lastIndexOf('.'));
    response.writeHead(200, {
      'content-type': MIME[extension] ?? 'application/octet-stream',
      'content-length': info.size,
      // A stale artifact served to a fresh browser is a debugging session
      // nobody enjoys, and this server lives for one run.
      'cache-control': 'no-store',
    });
    createReadStream(target).pipe(response);
  });
  return new Promise((ok, no) => {
    server.on('error', no);
    // Port 0: the OS picks a free one. A hard-coded port turns two harnesses on
    // one machine into a flake, and this repository treats a flake as a bug.
    server.listen(0, '127.0.0.1', () => {
      const { port } = /** @type {import('node:net').AddressInfo} */ (
        server.address()
      );
      ok({
        origin: `http://localhost:${port}`,
        misses,
        close: () => new Promise((done) => server.close(() => done(undefined))),
      });
    });
  });
}

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
 * Every one of these was measured on Chromium 150 rather than copied. Without
 * the WebGPU pair for the chosen mode, `navigator.gpu.requestAdapter()`
 * resolves to `null` in headless and the demo stops at its own "this browser
 * has no WebGPU" banner.
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
    flags.push('--enable-unsafe-webgpu', '--use-webgpu-adapter=swiftshader');
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
           const out = { gpu: 'gpu' in navigator, secure: isSecureContext };
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

const site = await serve(SITE);
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
  // it is what a visitor to the Pages URL gets, and on Chromium 150 it is the
  // only one whose canvas pixels can be read back at all.
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
  // `/favicon.ico` is requested by the browser, not by the shim, and the site
  // deliberately has none. Every other 404 is an asset the page wanted.
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

  group('C — input drives the simulation');

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
  const rect = await evaluate(
    page,
    `(() => { const c = document.getElementById('canvas');
              c.scrollIntoView({ block: 'center', behavior: 'instant' });
              const r = c.getBoundingClientRect();
              return { x: Math.round(r.x + r.width / 2), y: Math.round(r.y + r.height / 2) }; })()`
  );
  const inViewport = await evaluate(
    page,
    `(() => { const r = document.getElementById('canvas').getBoundingClientRect();
              const cy = r.y + r.height / 2, cx = r.x + r.width / 2;
              return cy >= 0 && cy <= innerHeight && cx >= 0 && cx <= innerWidth; })()`
  );
  check(
    'C',
    'the canvas centre is inside the viewport to be clicked',
    inViewport === true,
    `centre (${rect.x}, ${rect.y}) is outside the window`
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

  // The key the demo's own instructions name. `code` is what the engine binds
  // to; `key` and the virtual key codes are what a real keyboard sends.
  for (const type of ['keyDown', 'keyUp']) {
    await page.send('Input.dispatchKeyEvent', {
      type,
      code: 'Space',
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

  // The ball's x is in every HUD line, and two different values is the
  // simulation advancing under its own steam — which nothing on the JS side
  // could fake.
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
      ? `x took ${positions.size} values: ${[...positions].join(', ')}`
      : 'x never changed'
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
  await pause(200);
  check(
    'E',
    'focus coming back does not resume on its own',
    (await evaluate(page, `crcbl.status()`)) === 6,
    `status ${await evaluate(page, `crcbl.status()`)}`
  );

  for (const type of ['keyDown', 'keyUp']) {
    await page.send('Input.dispatchKeyEvent', {
      type,
      code: 'Escape',
      key: 'Escape',
      windowsVirtualKeyCode: 27,
      nativeVirtualKeyCode: 27,
    });
  }
  const resumed = await until(async () => {
    const status = await evaluate(page, `crcbl.status()`);
    return status === 3 ? status : null;
  });
  check(
    'E',
    'Escape resumes the demo',
    resumed === 3,
    `status ${await evaluate(page, `crcbl.status()`)}`
  );
  const afterResume = await heartbeats();
  check(
    'E',
    'the simulation runs again after resuming',
    afterResume > 0,
    `${afterResume} HUD line(s) in ${TICK_WINDOW_MS} ms`
  );

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
