#!/usr/bin/env node
// Drives `crcbl-webgpu`'s HAL seam directly, in a real browser, against a real
// `GPUDevice` — groups G through AE, which live in `web/tools/probe-groups.mjs`.
//
//   node web/tools/probe-e2e.mjs [--site target/site] [--timeout 90000]
//                                [--adapter auto] [--expect-fail X,Y,Z,AA]
//
// `web/run-probe-e2e.sh` is the entry point a human or CI runs; this file is the
// half that needs a JS engine. Plain Node with no dependencies, like every other
// tool in `web/`.
//
// WHY IT IS ITS OWN PAGE AND ITS OWN DRIVER. The probe exports install their own
// `StreamChannel`, and a page has exactly one — so they work only where nothing
// else has claimed it. That used to mean a site built on `wgpu`, where
// `crcbl-webgpu` is linked and idle while the demo renders through something
// else, and it made twenty groups of seam coverage depend on a second backend
// being present. `web/probe/` satisfies the same condition by having **no engine
// running at all**: it loads a demo's wasm module without booting it and pumps
// the command channel itself. So these groups now run against the very artifact
// the site ships, `crcbl-webgpu` and all, and need nothing else linked.
//
// WHAT IT ASSERTS is entirely in `probe-groups.mjs`, which is the point of the
// split: this file owns the browser, the page and the tally, that one owns the
// claims. It runs no demo groups — `web/tools/browser-e2e.mjs` is what boots a
// game, presses keys and reads the canvas back, and the two gates are run
// separately because they need different things of the browser.
//
// NO CANVAS SNAPSHOT ANYWHERE. Every byte these groups compare is read back into
// **wasm memory** by the backend itself and handed out over the reply channel,
// so there is no `toDataURL` here and no readback control to run first — the
// preflight `web/tools/browser-e2e.mjs` needs, and the shared-image-device flags
// it documents at length, are both about a canvas snapshot this gate never
// takes.
//
// **A DISPLAY IS STILL NOT OPTIONAL** on a machine with no GPU, and that was
// measured rather than assumed. Under `--headless=new` plus SwiftShader, group X
// — the first that presents a canvas frame — encodes its frame and then never
// resolves the readback map, and groups Y, Z and AA go down with it; inside Xvfb
// all four pass. Measured on Chromium 151. So `web/run-probe-e2e.sh` brings up an
// Xvfb the way the demo gate does, and a headless run on such a machine fails
// loudly rather than skipping.
//
// ONE STUCK PRESENT TAKES THE WHOLE TAIL OF THE RUN WITH IT, and that is not a
// guess either: with `startPresentProbe` and `startReconfigureProbe` stubbed out
// so X and Y submit nothing, in that same headless SwiftShader browser, Z and AA
// both pass. The page has ONE `GPUDevice` and therefore one queue, and a
// `mapAsync` resolves only behind the submits before it — so a present the
// compositor never completes strands every readback queued after it. Z draws
// indirectly into a texture the replayer owns and AA copies a depth plane;
// neither touches a canvas, and both fail anyway. **The dividing line is not
// "uses a canvas" and not "reads back" — it is "is queued behind X".**
//
// ENVIRONMENT
//   SITE_DIR                     Where the built site is. Default `target/site`.
//   CRCBL_CHROMIUM               Path to the browser. Otherwise this platform's
//                                names are tried on PATH and its usual install
//                                locations after that.
//   CRCBL_WEB_E2E_ADAPTER        `auto` (default), `hardware` or `swiftshader`.
//                                Also `--adapter`. What `auto` resolves to is
//                                below.
//   CRCBL_CHROMIUM_FLAGS         Extra flags, space-separated. The escape hatch
//                                for a runner nobody here has; it only ever
//                                appends, so the adapter switch is what picks
//                                between the GPU flag sets.
//   CRCBL_CHROMIUM_NO_SANDBOX=1  Add --no-sandbox (also added automatically as
//                                root, whose user namespaces a sandbox needs).
//   CRCBL_WEB_E2E_HEADED=1       Drop --headless=new, for a run inside Xvfb or
//                                on a Windows runner's own desktop session.
//   CRCBL_WEB_E2E_TIMEOUT_MS     How long each poll is given. SwiftShader is
//                                slow, and the readback groups map buffers.
//   CRCBL_WEB_E2E_EXPECT_FAIL    Group letters this platform is not expected to
//                                produce a verdict for, comma- or
//                                space-separated: `X,Y,Z,AA`. Also
//                                `--expect-fail`. They still run and still
//                                print; see EXIT STATUS.
//
// EXIT STATUS. Non-zero if any check fails *or* if zero checks ran. The second
// half is not decoration: `docs/plan/12-testing.md` names a silently-skipped e2e
// job as a known trap, and a gate whose browser never started would otherwise
// print nothing and succeed.
//
// `--expect-fail` narrows the first half and cannot touch the second. It is
// exact in both directions, because a list that merely *tolerates* failure is
// worse than no list at all — it silently absorbs the regression it was not
// written for. A named group that fails is excused and named as excused; a named
// group that PASSES fails the run as a stale list; a named group that recorded
// no checks fails it too. The groups themselves are untouched either way: they
// run, their failures print, and they count towards the tally.

import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync } from 'node:fs';
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
import { runProbeGroups } from './probe-groups.mjs';
import { serve } from './serve.mjs';

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

/**
 * Every browser this process started and has not stopped.
 *
 * Declared above the argument parsing because `fail` reads it and `fail` runs
 * during that parsing: a `const` does not hoist, and with this further down the
 * file an argument error would die in the temporal dead zone with its real
 * message buried under a stack trace pointing at the cleanup path.
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
  console.error(`probe e2e: ${message}`);
  stopEverything();
  process.exit(2);
}

/** @type {Record<string, string>} */
const args = {};
for (let i = 2; i < process.argv.length; i += 1) {
  const arg = process.argv[i];
  if (!arg.startsWith('--')) fail(`unexpected argument ${arg}`);
  const [name, inline] = arg.slice(2).split('=', 2);
  args[name] = inline ?? process.argv[++i] ?? '';
}

const SITE = resolve(REPO, args.site ?? process.env.SITE_DIR ?? 'target/site');
const TIMEOUT_MS = Number(
  args.timeout ?? process.env.CRCBL_WEB_E2E_TIMEOUT_MS ?? 90_000
);

/**
 * Which WebGPU adapter Chromium is told to use.
 *
 * `web/tools/browser-e2e.mjs` takes the same three values and resolves `auto` by
 * *trying* both modes and keeping whichever one's readback control passes. This
 * gate cannot: it reads every byte back into wasm memory rather than off a
 * canvas, so it has no control frame to judge a mode by, and a page that has to
 * be reloaded under a second adapter would re-enumerate the whole seam. So
 * `auto` here is resolved by platform, once, from what each one can actually
 * serve:
 *
 *   Linux    swiftshader — a CI runner has no GPU, and this is the one mode
 *            whose Dawn and shared-image devices can both be moved to the same
 *            software Vulkan. It is what this gate has always used.
 *   macOS    hardware — Chrome's Dawn has no Vulkan backend there at all, so
 *            SwiftShader is not a fallback; `macos-15` runners expose an Apple
 *            Paravirtual Metal device.
 *   Windows  hardware — SwiftShader moves Dawn while the shared-image device
 *            stays on D3D11, and `docs/backlog.md` records what that mismatch
 *            does. See `web/tools/browser-launch.mjs`.
 */
const ADAPTER = args.adapter ?? process.env.CRCBL_WEB_E2E_ADAPTER ?? 'auto';
const MODE =
  ADAPTER === 'auto'
    ? process.platform === 'linux'
      ? 'swiftshader'
      : 'hardware'
    : ADAPTER;

/**
 * Groups whose failure here is a known property of the platform, not a bug.
 *
 * Handed in from outside rather than decided by a `process.platform` test in
 * this file, and deliberately: this driver knows about browsers and adapters and
 * has no business knowing what Windows is. `.github/workflows/pages.yml` sets it
 * per job, beside the comment carrying the run it was measured from.
 *
 * **Exact in both directions**, which is the whole design. A list that only
 * tolerates failure is worse than no list — the day the group it excuses starts
 * working, it goes on excusing the *next* thing that breaks there, and nothing
 * says so. So a named group that passes is a hard failure, spelled "the list is
 * stale", and the same for a named group that ran nothing at all: both mean the
 * list no longer describes the run it was written against.
 *
 * @type {string[]}
 */
const EXPECT_FAIL = (
  args['expect-fail'] ??
  process.env.CRCBL_WEB_E2E_EXPECT_FAIL ??
  ''
)
  .split(/[\s,]+/)
  .filter(Boolean);

if (!existsSync(join(SITE, 'probe', 'index.html'))) {
  fail(`no probe page at ${SITE}/probe/index.html — run web/build.sh first`);
}
if (!Number.isFinite(TIMEOUT_MS) || TIMEOUT_MS <= 0) {
  fail(
    `--timeout must be a positive number of milliseconds, got ${TIMEOUT_MS}`
  );
}
if (!['auto', 'hardware', 'swiftshader'].includes(ADAPTER)) {
  fail(`--adapter must be auto, hardware or swiftshader, got "${ADAPTER}"`);
}
// Shape only. Which letters exist is not restated here — a letter that names no
// group is caught at the verdict, by the same rule that catches a letter whose
// group has started passing, and with a message that says which run it stopped
// describing.
const MISSHAPEN = EXPECT_FAIL.filter((letter) => !/^[A-Z]{1,2}$/.test(letter));
if (MISSHAPEN.length) {
  fail(
    `--expect-fail takes group letters like "X,Y,Z,AA", got "${MISSHAPEN.join('", "')}"`
  );
}

const pause = (ms) => new Promise((ok) => setTimeout(ok, ms));

// ---------------------------------------------------------------------------
// The browser
// ---------------------------------------------------------------------------

// `findBrowser`, the flag builder and the kill are all in
// `web/tools/browser-launch.mjs`. The three browser gates in this directory need
// the same browser started the same way, and every platform-specific thing about
// doing that — where the binary lives, which flags name the real device, how to
// take a process tree down — is decided there once rather than three times.

/**
 * Starts the browser and returns its DevTools endpoint.
 *
 * The endpoint comes from `DevToolsActivePort`, which Chrome writes into the
 * profile once it is listening. Polling that file is how the launch is
 * synchronised: a sleep would be a flake on a slow machine and wasted time on a
 * fast one.
 */
async function launch(binary) {
  const profile = mkdtempSync(join(tmpdir(), 'crcbl-probe-e2e-'));
  const flags = browserFlags({ profile, mode: MODE });
  const child = spawn(binary, [...flags, 'about:blank'], {
    stdio: ['ignore', 'ignore', 'pipe'],
    // Its own process group, so `stop` can kill the whole tree: Chromium is
    // half a dozen processes and a `kill` aimed at the parent leaves the GPU
    // process and the zygotes behind when the parent is wedged.
    detached: true,
    // A developer's `~/.config/chromium-flags.conf` is read by the launcher on
    // some distributions and appended to the command line; on a Wayland desktop
    // it sets `--ozone-platform=wayland`, which in a headless run takes the GPU
    // process down and hides WebGPU entirely.
    env: { ...process.env, XDG_CONFIG_HOME: profile },
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
    stderr,
    flags,
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
 * browser has it.
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
  console.log(`\nprobe e2e: ${name}`);
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

const site = await serve(SITE, { host: '127.0.0.1' });
const binary = findBrowser(fail);

/**
 * Everything the page logged, in order.
 *
 * Printed only on a failure, and only its tail: the page's own account of the
 * pump — a latched channel fault says so here first — is the thing a red run is
 * diagnosed from, and there is no screenshot to fall back on.
 */
const consoleLines = [];
/** How many of those lines a failure prints. */
const CONSOLE_TAIL = 20;
/** WebGPU device errors, which Chrome reports through the Log domain. */
const deviceErrors = [];
/** Uncaught exceptions in page JS. */
const pageErrors = [];

let browser = null;
let exitCode = 1;

try {
  console.log(`probe e2e: browser ${binary}`);
  console.log(`probe e2e: serving ${SITE} at ${site.origin}`);
  // Named on its own line, and with what `auto` resolved to rather than the word
  // `auto`: on a red run from a platform nobody here has, which adapter the
  // browser was actually asked for is the first thing to know.
  console.log(
    `probe e2e: adapter mode "${MODE}"` +
      `${ADAPTER === 'auto' ? ` (auto on ${process.platform})` : ''}`
  );
  // Said before the run as well as after it. A reader who stops at the top of a
  // green log has to be able to see what this run's verdict does not cover.
  if (EXPECT_FAIL.length) {
    console.log(
      `probe e2e: --expect-fail ${EXPECT_FAIL.join(' ')} — those groups still run, ` +
        'and a pass from any of them fails this run as a stale list'
    );
  }

  browser = await launch(binary);
  console.log(
    `probe e2e: flags ${browser.flags.filter((f) => !f.startsWith('--user-data-dir')).join(' ')}`
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
    if (entry.source === 'rendering' && entry.level !== 'info')
      deviceErrors.push(entry.text);
  });
  await page.send('Runtime.enable');
  await page.send('Log.enable');
  await page.send('Page.enable');

  const url = `${site.origin}/probe/index.html`;
  await page.send('Page.navigate', { url });

  // The page pumps for the life of the tab, so there is no terminal state to
  // wait for — only the moment it has a module, a registry and a loop, which it
  // says with `probeReady`. `probeFatal` is the other answer and is why this is
  // not just a poll for `globalThis.crcbl`: a page that could not get an adapter
  // has a reason to report, and a bare timeout would throw it away.
  const started = await until(async () =>
    evaluate(
      page,
      `(() => {
         if (window.probeFatal) return { fatal: window.probeFatal };
         return window.probeReady ? { ready: true } : null;
       })()`
    )
  );
  if (!started?.ready) {
    throw new Error(
      started?.fatal
        ? `the probe page could not start: ${started.fatal}`
        : `the probe page never became ready in ${TIMEOUT_MS} ms` +
            `${pageErrors.length ? ` — ${pageErrors[0]}` : ''}`
    );
  }
  console.log(`probe e2e: ${url} is pumping, with no engine on it`);

  await runProbeGroups({ page, evaluate, until, check, group, TIMEOUT_MS });

  page.close();
  exitCode = 0;
} catch (error) {
  console.error(`\nprobe e2e: ${error.message}`);
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
  console.error('probe e2e: ZERO CHECKS RAN — the gate is not gating.');
  if (browser?.stderr.length)
    console.error(browser.stderr.slice(-40).join('\n'));
  process.exit(1);
}

// The group letters, in the order they were printed, beside the count. A page
// that silently ran only some of them is the failure mode here, and a bare
// "n/m checks passed" cannot show it: the letters can.
const letters = [...new Set(checks.map((c) => c.group))];
console.log(`probe e2e: groups ${letters.join(' ')}`);
console.log(
  `probe e2e: ${checks.length - failed.length}/${checks.length} checks passed`
);

// What the expected-fail list actually did, group by group. `stale` is the half
// that keeps it honest: a named group with nothing failing in it has outlived
// the defect it was written for, and left alone it would go on covering whatever
// breaks there next.
const stale = [];
const absent = [];
for (const letter of EXPECT_FAIL) {
  const ran = checks.filter((c) => c.group === letter);
  if (ran.length === 0) absent.push(letter);
  else if (ran.every((c) => c.ok)) stale.push(letter);
}
const excused = failed.filter((c) => EXPECT_FAIL.includes(c.group));
const unexpected = failed.filter((c) => !EXPECT_FAIL.includes(c.group));

if (EXPECT_FAIL.length) {
  // On a green run too, and check by check rather than only by letter: "the run
  // passed" and "these four claims were not made" have to be readable off the
  // same log, or the second one is not really said.
  console.log(
    `probe e2e: expected to fail on this platform: ${EXPECT_FAIL.join(' ')}`
  );
  for (const c of excused)
    console.log(
      `  excused ${c.group}: ${c.name}${c.detail ? ` — ${c.detail}` : ''}`
    );
}

if (unexpected.length) {
  console.error('\nprobe e2e: FAILED');
  for (const c of unexpected)
    console.error(`  ${c.group}: ${c.name}${c.detail ? ` — ${c.detail}` : ''}`);
  if (deviceErrors.length) {
    console.error('\nprobe e2e: WebGPU device errors, in full:');
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
  if (consoleLines.length) {
    console.error('\nprobe e2e: the last of the page log:');
    for (const line of consoleLines.slice(-CONSOLE_TAIL))
      console.error(`    ${line}`);
  }
}

// Reported after the real failures rather than instead of them, so a run that
// has both says both: a stale entry and a regression arriving together is
// exactly the case where seeing only one sends the reader to the wrong half.
if (stale.length || absent.length) {
  console.error(
    '\nprobe e2e: THE EXPECTED-FAIL LIST NO LONGER DESCRIBES THIS RUN'
  );
  for (const letter of stale)
    console.error(
      `  ${letter}: every check in it passed, and --expect-fail names it. ` +
        'Drop it from the list — this platform serves that group now.'
    );
  for (const letter of absent)
    console.error(
      `  ${letter}: --expect-fail names it and it recorded no checks at all. ` +
        'Either it is not a group letter, or the run never reached it.'
    );
}

if (unexpected.length || stale.length || absent.length) process.exit(1);

process.exit(exitCode);
