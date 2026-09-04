#!/usr/bin/env node
// Loads the worker-backend gate page in a real browser and reports what it
// checked.
//
// It decides nothing on its own: `web/jobs/main.js` runs the checks inside the
// page — that is the only place with a `Worker`, a shared `WebAssembly.Memory`
// and the artifact's `gate_*` exports — and this reads the list out of
// `window.jobsResult` over the DevTools protocol and turns it into an exit code.
// `web/run-jobs-e2e.sh` assembles the site, runs this three more times with the
// red switches on, and insists the right assertions broke.
//
// NO GPU IS INVOLVED. There is no canvas and no `navigator.gpu` call anywhere on
// the page, so this needs none of the Xvfb and shared-image-device machinery
// `web/tools/browser-e2e.mjs` needs to read a canvas back: headless is enough.
// The adapter mode below only picks flags nothing here exercises, and is fixed
// rather than configurable for that reason.
//
// USAGE
//   node web/tools/jobs-e2e.mjs <site-dir> [--query <query-string>]
//                                          [--timeout <ms>] [--no-isolation]
//
//   <site-dir>      A directory with `jobs/index.html` and the artifact beside
//                   it; `web/run-jobs-e2e.sh` assembles one.
//   --query         Appended to the page URL. The page's red switches live there
//                   — `no-stack-pointer`, `no-init-tls`, `no-host-ready`,
//                   `workers=N` — and so does `no-isolation`, which is the page
//                   asserting the degradation rather than the isolated run.
//   --timeout       How long the page has to finish. Default below.
//   --no-isolation  Serve without the COOP/COEP pair, which is the origin
//                   GitHub Pages gives every visitor. It says nothing to the
//                   page: pair it with `--query no-isolation` so the page runs
//                   the list that expects the degradation, and expect the
//                   isolated list to go red if you forget. `web/run-jobs-e2e.sh`
//                   passes both.
//
// ENVIRONMENT
//   CRCBL_CHROMIUM               Path to the Chromium/Chrome binary.
//   CRCBL_CHROMIUM_FLAGS         Extra flags, space-separated.
//   CRCBL_CHROMIUM_NO_SANDBOX=1  Add --no-sandbox (also added automatically as
//                                root, whose user namespaces a sandbox needs).
//
// EXIT CODES
//   0  every check the page ran passed, and there was at least one.
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

/** How long the page has to publish `window.jobsDone`. */
const RUN_TIMEOUT_MS = 120_000;

/** How often to ask. Coarse: the page takes seconds, not frames. */
const POLL_MS = 100;

function fail(message) {
  console.error(`jobs-e2e: ${message}`);
  stopEverything();
  process.exit(2);
}

function parseArgs(argv) {
  let site = null;
  let query = '';
  let timeout = RUN_TIMEOUT_MS;
  let isolated = true;
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--query') {
      query = argv[i + 1];
      if (query === undefined) fail('--query needs a query string');
      i += 1;
    } else if (argv[i] === '--timeout') {
      timeout = Number(argv[i + 1]);
      if (!Number.isInteger(timeout) || timeout < 1) {
        fail('--timeout needs a whole number of milliseconds');
      }
      i += 1;
    } else if (argv[i] === '--no-isolation') {
      isolated = false;
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
      'usage: jobs-e2e.mjs <site-dir> [--query <query-string>] [--timeout <ms>] ' +
        '[--no-isolation]'
    );
  }
  return {
    site: resolve(site),
    query: query.replace(/^\?/, ''),
    timeout,
    isolated,
  };
}

async function main() {
  const { site, query, timeout, isolated } = parseArgs(process.argv.slice(2));
  if (!existsSync(join(site, 'jobs', 'index.html'))) {
    fail(`${site}/jobs/index.html not found — run web/run-jobs-e2e.sh`);
  }

  // Cross-origin isolation is the whole precondition for the threaded run, and
  // it comes from these response headers rather than from anything the page
  // does. `serve.mjs` is the same server `web/build.sh --serve` runs, so there
  // is one set of headers and the page's own `crossOriginIsolated` check gates
  // them. Under `--no-isolation` the pair is withheld and the page is expected
  // to be driven with `?no-isolation`, which is the configuration a visitor to
  // the published site gets.
  const server = await serve(site, { host: '127.0.0.1', isolated });
  const browser = await launch({
    binary: findBrowser(fail),
    mode: 'swiftshader',
    profilePrefix: 'crcbl-jobs-e2e-',
    fail,
  });

  let page;
  try {
    page = await openPage(browser);
    await page.send('Runtime.enable');
    await page.send('Page.enable');

    /** Anything the page threw that it did not catch itself. */
    const pageErrors = [];
    page.on('Runtime.exceptionThrown', ({ exceptionDetails }) => {
      pageErrors.push(
        exceptionDetails.exception?.description ??
          exceptionDetails.text ??
          'an exception with no description'
      );
    });

    const url = `${server.origin}/jobs/index.html${query ? `?${query}` : ''}`;
    console.log(
      `jobs-e2e: ${url}` +
        (server.isolated ? '' : ' (served without COOP/COEP)')
    );
    await page.send('Page.navigate', { url });

    // Written out rather than run through the shared `until`: that one swallows
    // a probe that throws, and a page this gate cannot evaluate in at all is
    // exit 2 rather than a condition not met yet.
    const deadline = Date.now() + timeout;
    let done = false;
    while (Date.now() < deadline) {
      done = await evaluate(page, 'Boolean(window.jobsDone)');
      if (done) break;
      await pause(POLL_MS);
    }
    if (!done) {
      for (const error of pageErrors) console.error(`  page error: ${error}`);
      console.error(browser.stderr.slice(-20).join('\n'));
      fail(`the page did not finish within ${timeout} ms`);
    }

    const result = await evaluate(page, 'window.jobsResult');
    if (!result || !Array.isArray(result.checks)) {
      fail('the page finished without publishing a result');
    }

    for (const line of result.notes ?? []) console.log(`  ${line}`);
    for (const { name, ok } of result.checks) {
      console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${name}`);
    }
    // A page that threw part-way through has checked less than it looks like.
    // Printed after the list so the last thing on screen is the reason.
    if (result.fatal) console.error(`\njobs-e2e: fatal: ${result.fatal}`);
    for (const error of pageErrors)
      console.error(`jobs-e2e: page error: ${error}`);

    const passed = result.checks.filter((c) => c.ok).length;
    console.log(`\njobs-e2e: ${passed}/${result.checks.length} checks passed`);

    // The guard every harness here carries: a run that checked nothing must not
    // be able to report success.
    if (result.checks.length === 0) {
      console.error(
        'jobs-e2e: the page ran no checks — the gate is not gating'
      );
      process.exitCode = 1;
      return;
    }
    if (passed < result.checks.length || result.fatal) {
      process.exitCode = 1;
      return;
    }
    console.log(
      server.isolated
        ? 'jobs-e2e: a Web Worker brought up through the spawn ABI ran Rust on ' +
            'a stack and a thread-local of its own'
        : 'jobs-e2e: on an origin with no COOP/COEP the backend degraded onto ' +
            "Inline's behaviour and still reached the same answer"
    );
  } finally {
    // The browser dies before the server is awaited: Chromium holds a keep-alive
    // socket, and awaiting `close()` first leaves `browser.stop()` unreachable.
    // Same order, for the same reason, as `render-harness-e2e.mjs`.
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
        'jobs-e2e: teardown finished but something is still holding the event ' +
          'loop open; exiting rather than hanging'
      );
      process.exit(process.exitCode ?? 0);
    }, EXIT_DEADLINE_MS);
    watchdog.unref();
  })
  .catch((error) => {
    console.error(error);
    process.exit(2);
  });
