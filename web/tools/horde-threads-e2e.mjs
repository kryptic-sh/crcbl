#!/usr/bin/env node
// Drives the horde demo in a real browser and says whether its simulation ran
// off the main thread.
//
// THE ONE QUESTION. `apps/horde`'s `steer_enemies` is the engine's only sample
// consumer of `crcbl_jobs::Pool::par_for`, and it is **bit-identical at any
// worker count by construction** — that is the determinism rule its own docs
// state, and `steering_is_bit_identical_however_many_workers_run_it` is what
// holds it. So a threaded run and an inline run draw the same frames, and no
// screenshot, status code or log line anywhere in this repository can tell them
// apart. `__crcbl_horde_sim_threads` can: it counts the distinct threads that
// have run a steering chunk, and this asserts it reached two.
//
// WHY IT IS A DRIVER AND NOT A PAGE OF CHECKS. `web/tools/jobs-e2e.mjs` reads a
// verdict out of `window.jobsResult` because `web/jobs/main.js` is a page
// written to produce one. The page here is the **demo a visitor loads**, shipped
// unchanged — that is the whole claim — so it has no verdict to publish and the
// checks are made out here, from what it exposes: `globalThis.crcbl` from
// `web/engine/demo.js`, `globalThis.crcblHordeSim` from
// `web/demos/horde/main.js`, and `globalThis.crcblWorkers` from
// `web/tools/wasm-loader-threads.js`.
//
// THE CROWD IS PART OF THE MEASUREMENT, NOT SET DRESSING. `par_for` runs a
// single chunk inline whatever the pool holds, and `STEER_CHUNK` is 64 enemies,
// so a run with a small field never leaves the calling thread however many
// workers are up. `?prefill=N` stages a field through `--prefill`'s own code
// path — the flag the scale measurement uses — and `--prefill` is what makes the
// pass split. A run that is not given one waits at horde's title screen and
// steers nothing at all, which is red check B below.
//
// USAGE
//   node web/tools/horde-threads-e2e.mjs <site-dir> [--prefill N] [--query q]
//                                                   [--timeout ms]
//
//   <site-dir>   A site with `demos/horde/`; `web/build.sh --threads` writes
//                one, and `web/build.sh` writes the non-threaded one red check
//                C runs against.
//   --prefill    Enemies to stage before the first tick. Default below.
//   --query      Extra query string for the page. The threaded loader's red
//                switch, `no-host-ready`, lives there.
//   --timeout    How long the run has to reach a second thread. Default below.
//
// ENVIRONMENT
//   CRCBL_CHROMIUM               Path to the Chromium/Chrome binary.
//   CRCBL_CHROMIUM_FLAGS         Extra flags, space-separated.
//   CRCBL_CHROMIUM_NO_SANDBOX=1  Add --no-sandbox (also added automatically as
//                                root, whose user namespaces a sandbox needs).
//   CRCBL_WEB_E2E_HEADED=1       Drop `--headless=new`; set by the shell script
//                                once it has an Xvfb display, because a WebGPU
//                                canvas on SwiftShader needs one.
//
// EXIT CODES
//   0  every check passed, and there was at least one.
//   1  the page ran and at least one check failed. Every check is named.
//   2  it could not run at all — no browser, no page, no result.

import { existsSync } from 'node:fs';
import { join, resolve } from 'node:path';

import {
  evaluate,
  findBrowser,
  launch,
  openPage,
  pause,
  stopEverything,
} from './browser-launch.mjs';
import { serve } from './serve.mjs';

/** How long the run has to reach a second steering thread. */
const RUN_TIMEOUT_MS = 120_000;

/** How often to ask. The answer moves once a tick, not once a frame. */
const POLL_MS = 200;

/**
 * How many enemies to stage when `--prefill` does not say.
 *
 * More than one `STEER_CHUNK`, and by enough that the split is not marginal:
 * `apps/horde`'s own `steering_is_bit_identical_however_many_workers_run_it`
 * stages the same crowd for the same reason, and says why 64 in a chunk means a
 * field this size is more chunks than the machine has cores.
 */
const DEFAULT_PREFILL = 800;

/**
 * The size the browser window is fixed at.
 *
 * Nothing here reads a pixel, but horde's camera and its sprite batching are
 * both a function of the surface, and a gate that behaves differently on a
 * laptop and a runner is one nobody can reproduce.
 */
const WINDOW_SIZE = '--window-size=1280,800';

/** @type {{ name: string, ok: boolean }[]} */
const checks = [];

/**
 * @param {boolean} condition
 * @param {string} what
 */
function check(condition, what) {
  checks.push({ name: what, ok: Boolean(condition) });
}

function fail(message) {
  console.error(`horde-threads-e2e: ${message}`);
  stopEverything();
  process.exit(2);
}

function parseArgs(argv) {
  let site = null;
  let query = '';
  let prefill = DEFAULT_PREFILL;
  let timeout = RUN_TIMEOUT_MS;
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--query') {
      query = argv[i + 1];
      if (query === undefined) fail('--query needs a query string');
      i += 1;
    } else if (argv[i] === '--prefill') {
      prefill = Number(argv[i + 1]);
      if (!Number.isInteger(prefill) || prefill < 0) {
        fail('--prefill needs a whole number of enemies');
      }
      i += 1;
    } else if (argv[i] === '--timeout') {
      timeout = Number(argv[i + 1]);
      if (!Number.isInteger(timeout) || timeout < 1) {
        fail('--timeout needs a whole number of milliseconds');
      }
      i += 1;
    } else if (argv[i].startsWith('--')) {
      fail(`unknown option ${argv[i]}`);
    } else if (site === null) {
      site = argv[i];
    } else {
      fail(`unexpected argument ${argv[i]}`);
    }
  }
  if (!site) {
    fail(
      'usage: horde-threads-e2e.mjs <site-dir> [--prefill N] [--query q] [--timeout ms]'
    );
  }
  return {
    site: resolve(site),
    query: query.replace(/^\?/, ''),
    prefill,
    timeout,
  };
}

/**
 * Everything the page can be asked about itself, in one round trip.
 *
 * Every field is guarded: on the non-threaded site `crcblWorkers` is simply not
 * there, because `web/tools/wasm-loader.js` is a different loader — and that
 * absence is the observation red check C is built on, not an error.
 */
const SNAPSHOT = `(() => {
  const sim = globalThis.crcblHordeSim ? globalThis.crcblHordeSim() : null;
  const workers = globalThis.crcblWorkers ? globalThis.crcblWorkers.stats() : null;
  return {
    isolated: globalThis.crossOriginIsolated === true,
    status: globalThis.crcbl ? globalThis.crcbl.status() : -1,
    statusText: document.getElementById('status')?.textContent ?? '',
    detail: document.getElementById('detail')?.textContent ?? '',
    simThreads: sim ? sim.threads : -1,
    simWorkers: sim ? sim.workers : -1,
    threadedArtifact: workers !== null,
    announced: workers ? workers.announced : 0,
    up: workers ? workers.up : 0,
    workerErrors: workers ? workers.errors : [],
  };
})()`;

/** `STATUS_RUNNING` and `STATUS_PAUSED` from `apps/horde/src/web.rs`. */
const STATUS_RUNNING = 3;
const STATUS_PAUSED = 6;

async function main() {
  const { site, query, prefill, timeout } = parseArgs(process.argv.slice(2));
  if (!existsSync(join(site, 'demos', 'horde', 'index.html'))) {
    fail(
      `${site}/demos/horde/index.html not found — run web/build.sh --threads`
    );
  }

  // Cross-origin isolation comes from these response headers and from nothing
  // the page does; `serve.mjs` is the same server `web/build.sh --serve` runs.
  // Without them a shared `WebAssembly.Memory` cannot be constructed at all,
  // which is why GitHub Pages can never carry this build.
  const server = await serve(site, { host: '127.0.0.1' });
  const browser = await launch({
    binary: findBrowser(fail),
    mode: 'swiftshader',
    profilePrefix: 'crcbl-horde-threads-e2e-',
    extra: [WINDOW_SIZE],
    fail,
  });

  let page;
  try {
    page = await openPage(browser);
    await page.send('Runtime.enable');
    await page.send('Page.enable');

    /** Anything the page threw that nothing caught. */
    const pageErrors = [];
    page.on('Runtime.exceptionThrown', ({ exceptionDetails }) => {
      pageErrors.push(
        exceptionDetails.exception?.description ??
          exceptionDetails.text ??
          'an exception with no description'
      );
    });

    const search = [`prefill=${prefill}`, query].filter(Boolean).join('&');
    const url = `${server.origin}/demos/horde/?${search}`;
    console.log(`horde-threads-e2e: ${url}`);
    await page.send('Page.navigate', { url });

    // One loop for the whole run: it stops as soon as a second thread has taken
    // a chunk, and otherwise keeps the demo playing until the deadline. Written
    // out rather than run through the shared `until`, for `jobs-e2e.mjs`'s
    // reason: that one swallows a probe that throws, and a page this cannot
    // evaluate in at all is exit 2 rather than a condition not yet met.
    const deadline = Date.now() + timeout;
    /** @type {any} */
    let snap = null;
    /** Whether the demo was ever seen playing, which a later failure cannot undo. */
    let played = false;
    while (Date.now() < deadline) {
      snap = await evaluate(page, SNAPSHOT);
      if (snap.status === STATUS_RUNNING || snap.status === STATUS_PAUSED) {
        played = true;
      }
      if (snap.simThreads >= 2) break;
      await pause(POLL_MS);
    }
    if (snap === null) fail('the page was never evaluated');

    console.log(`  status:      ${snap.status} — ${snap.statusText}`);
    if (snap.detail) console.log(`  detail:      ${snap.detail}`);
    console.log(
      `  loader:      threaded=${snap.threadedArtifact}, ` +
        `announced=${snap.announced}, workers up=${snap.up}`
    );
    console.log(
      `  steering:    ${snap.simThreads} thread(s), pool of ${snap.simWorkers} worker(s)`
    );
    for (const error of snap.workerErrors)
      console.log(`  worker error: ${error}`);
    for (const error of pageErrors) console.log(`  page error:  ${error}`);

    check(snap.isolated, 'the document is cross-origin isolated');
    check(played, 'the demo booted a GPU device and started playing');
    check(
      snap.threadedArtifact,
      'the artifact imports a shared memory this page could give it'
    );
    check(
      snap.announced > 0,
      'the page announced worker threads to the backend'
    );
    check(
      snap.up > 0 && snap.workerErrors.length === 0,
      `every worker the page started came up (${snap.up} up, ` +
        `${snap.workerErrors.length} refused)`
    );
    // Before the two claims about *which* threads, because a run whose crowd
    // never reached the pass would otherwise fail them for a reason that has
    // nothing to do with threads. This is what separates "no workers" from "no
    // work".
    check(snap.simThreads >= 1, 'the steering pass ran at all');
    check(
      snap.simWorkers >= 1,
      `the steering pool has worker threads (it has ${snap.simWorkers})`
    );
    // THE EXIT CRITERION. Everything above is a precondition for it.
    check(
      snap.simThreads >= 2,
      'a steering chunk ran on a thread that is not the main thread'
    );
    check(pageErrors.length === 0, 'the page reported no uncaught exception');

    for (const { name, ok } of checks) {
      console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${name}`);
    }
    const passed = checks.filter((c) => c.ok).length;
    console.log(
      `\nhorde-threads-e2e: ${passed}/${checks.length} checks passed`
    );

    // The guard every harness here carries: a run that checked nothing must not
    // be able to report success.
    if (checks.length === 0) {
      console.error(
        'horde-threads-e2e: no checks ran — the gate is not gating'
      );
      process.exitCode = 1;
      return;
    }
    if (passed < checks.length) {
      process.exitCode = 1;
      return;
    }
    console.log(
      "horde-threads-e2e: horde's steering pass ran on a Web Worker in a real " +
        'browser'
    );
  } finally {
    // The browser dies before the server is awaited: Chromium holds a keep-alive
    // socket, and awaiting `close()` first leaves `browser.stop()` unreachable.
    if (page) page.close();
    browser.stop();
    await server.close();
  }
}

/** How long the process may stay alive after `main` has finished. */
const EXIT_DEADLINE_MS = 60_000;

main()
  .then(() => {
    const watchdog = setTimeout(() => {
      console.error(
        'horde-threads-e2e: teardown finished but something is still holding ' +
          'the event loop open; exiting rather than hanging'
      );
      process.exit(process.exitCode ?? 0);
    }, EXIT_DEADLINE_MS);
    watchdog.unref();
  })
  .catch((error) => {
    console.error(error);
    process.exit(2);
  });
