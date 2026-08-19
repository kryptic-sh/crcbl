// Loads the render-harness page in a real browser, drives every golden scene
// through the browser GPU backend, and writes each frame it read back to disk.
// It is the browser end of the WebGPU parity gate: the one check that can say
// whether `crcbl::screenshot`'s offscreen path can be driven through
// `crcbl-webgpu` at all, which nothing native can answer.
//
// IT DOES NOT COMPARE ANYTHING. The pixels go to `--readback-dir` and
// `apps/render-harness/examples/compare-readback.rs` compares them against
// `crates/crcbl/tests/golden/<scene>.png` with `crcbl-golden` — the same
// comparator and tolerance every native golden test uses. A second pixel diff
// written in JS would be a second thing to tune and a second thing to be wrong;
// `web/run-render-harness-e2e.sh` runs the two halves back to back.
//
// It reads its verdict out of `window.harnessResult` over the DevTools protocol
// rather than off a canvas — the harness renders to an offscreen target and
// reads it back into wasm memory, so there is no canvas to snapshot. That also
// means it needs none of the Xvfb / shared-image-device machinery
// `web/tools/browser-e2e.mjs` needs for canvas readback; headless plus
// SwiftShader is enough.
//
// USAGE
//   node web/tools/render-harness-e2e.mjs <site-dir> [--readback-dir <dir>]
//                                                    [--result-json <path>]
//
//   <site-dir>       A directory with `harness/index.html` and the loader-plus-
//                    output beside it; `web/run-render-harness-e2e.sh`
//                    assembles one.
//   --readback-dir   Where to write the readbacks, as
//                    `<scene>.<width>x<height>.<order>.bin` — everything the
//                    comparator needs to read the bytes, in the name. Defaults
//                    to `<site-dir>/readback`.
//   --result-json    Where to write this run's per-scene outcome as JSON.
//                    `web/tools/render-harness-verdict.mjs` reads it beside the
//                    comparator's table, because half of what makes a scene a
//                    failure is only visible here: a scene that never rendered,
//                    and a scene that rendered while the device was refusing
//                    its commands. The exit code cannot carry that — it says
//                    *that* something failed, never which scene — and the
//                    expected-fail list is per scene.
//
// ENVIRONMENT
//   CRCBL_CHROMIUM           Path to the Chromium/Chrome binary. Otherwise the
//                            usual four names are tried.
//   CRCBL_CHROMIUM_FLAGS     Extra flags, space-separated.
//   CRCBL_CHROMIUM_NO_SANDBOX=1  Add --no-sandbox (also added automatically as
//                            root, whose user namespaces a sandbox needs).
//
// EXIT CODES
//   0  every scene rendered and its pixels were written, with the device
//      reporting nothing.
//   1  the harness ran but at least one scene did not render, or rendered while
//      the device was refusing its commands; the per-scene table names the
//      crack. A refused command does not throw, so the state alone cannot tell
//      the two apart — the device errors are listed under the table.
//   2  the harness could not run at all (no browser, no adapter, wasm failed).

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

import {
  LAUNCH_TIMEOUT_MS,
  browserFlags,
  findBrowser,
  readDevToolsPort,
  stopBrowser,
} from './browser-launch.mjs';
import { serve } from './serve.mjs';

const RUN_TIMEOUT_MS = 180_000;

function fail(message) {
  console.error(`render-harness-e2e: ${message}`);
  process.exit(2);
}

function pause(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// ---------------------------------------------------------------------------
// Browser launch
// ---------------------------------------------------------------------------

// `findBrowser`, the flag builder and the kill are in
// `web/tools/browser-launch.mjs`, shared with the two gates beside this one.
// **`swiftshader` is pinned rather than left to resolve**: this harness renders
// offscreen and compares every scene against a golden image, so it is asking a
// specific rasteriser for pixels a comparator will hold to a tolerance, and
// which device produced them is not a detail to leave to the machine.

async function launch(binary) {
  const profile = mkdtempSync(join(tmpdir(), 'crcbl-harness-e2e-'));
  const flags = browserFlags({ profile, mode: 'swiftshader' });
  const child = spawn(binary, [...flags, 'about:blank'], {
    stdio: ['ignore', 'ignore', 'pipe'],
    detached: true,
    env: { ...process.env, XDG_CONFIG_HOME: profile },
  });

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
      stopBrowser(child, profile);
    },
  };

  const portFile = join(profile, 'DevToolsActivePort');
  const deadline = Date.now() + LAUNCH_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (exited) {
      console.error(stderr.join('\n'));
      browser.stop();
      return fail(`the browser stopped before it listened (${exited})`);
    }
    const endpoint = readDevToolsPort(portFile);
    if (endpoint) {
      browser.endpoint = `ws://127.0.0.1:${endpoint.port}${endpoint.path}`;
      return browser;
    }
    await pause(50);
  }
  console.error(stderr.join('\n'));
  browser.stop();
  return fail('the browser never wrote DevToolsActivePort');
}

// ---------------------------------------------------------------------------
// A minimal Chrome DevTools Protocol client
// ---------------------------------------------------------------------------

class Cdp {
  #socket;
  #next = 0;
  #pending = new Map();

  static async connect(url) {
    const client = new Cdp();
    client.#socket = new WebSocket(url);
    await new Promise((ok, no) => {
      client.#socket.onopen = ok;
      client.#socket.onerror = () => no(new Error(`cannot reach ${url}`));
    });
    client.#socket.onmessage = (event) => {
      const message = JSON.parse(event.data);
      if (message.id === undefined) return;
      const slot = client.#pending.get(message.id);
      if (!slot) return;
      client.#pending.delete(message.id);
      if (message.error)
        slot.reject(
          new Error(`${message.error.message} (${message.error.code})`)
        );
      else slot.resolve(message.result);
    };
    return client;
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

// ---------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------

function pad(text, width) {
  return String(text).padEnd(width);
}

function printTable(scenes) {
  const nameWidth = Math.max(5, ...scenes.map((s) => s.scene.length));
  console.log(
    `\n${pad('scene', nameWidth)}  rendered  state     frame              detail`
  );
  console.log('-'.repeat(nameWidth + 2 + 8 + 2 + 8 + 2 + 18 + 2 + 40));
  for (const scene of scenes) {
    // The fatal comes first: a scene that aborted the module has no state worth
    // reading, and it is the only line that says why.
    const refused = scene.deviceErrors ?? [];
    const detail = scene.fatal
      ? `fatal: ${scene.fatal.split('\n')[0]}`
      : scene.replayFailure
        ? `replay: ${scene.replayFailure.split('\n')[0]}`
        : scene.timedOut
          ? `timed out after ${scene.frames} frames`
          : scene.error ||
            (refused.length > 0
              ? `device: ${refused[0]}${refused.length > 1 ? ` (+${refused.length - 1} more)` : ''}`
              : '');
    // The extent and channel order the frame actually came back in, because
    // both are how a comparison goes wrong quietly: a different extent compares
    // nothing to nothing, and the wrong channel order is a red/blue swap that
    // reads as a shader bug.
    const frame = scene.rendered
      ? `${scene.width}x${scene.height} ${scene.order}`
      : '-';
    console.log(
      `${pad(scene.scene, nameWidth)}  ${pad(scene.rendered ? 'yes' : 'no', 8)}  ${pad(
        scene.stateName,
        8
      )}  ${pad(frame, 18)}  ${detail}`
    );
  }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

/** Parses argv into the site directory, where the readbacks go, and where the
 * per-scene result is written. */
function parseArgs(argv) {
  let site = null;
  let readbackDir = null;
  let resultJson = null;
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--readback-dir') {
      readbackDir = argv[i + 1];
      if (!readbackDir) fail('--readback-dir needs a directory');
      i += 1;
    } else if (argv[i] === '--result-json') {
      resultJson = argv[i + 1];
      if (!resultJson) fail('--result-json needs a path');
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
      'usage: render-harness-e2e.mjs <site-dir> [--readback-dir <dir>] ' +
        '[--result-json <path>]'
    );
  }
  const resolved = resolve(site);
  return {
    site: resolved,
    readbackDir: readbackDir
      ? resolve(readbackDir)
      : join(resolved, 'readback'),
    resultJson: resultJson ? resolve(resultJson) : null,
  };
}

/**
 * Pulls one scene's pixels out of the page and writes them where the comparator
 * will look, returning the path or throwing.
 *
 * The base64 is fetched per scene rather than with the rest of the result: the
 * DevTools protocol is JSON, and eleven frames of it in one reply is several
 * megabytes on one message.
 */
async function writeReadback(page, dir, scene) {
  const base64 = await evaluate(
    page,
    `window.harnessPixels[${JSON.stringify(scene.scene)}] ?? null`
  );
  if (typeof base64 !== 'string') {
    throw new Error('the page recorded no pixels for it');
  }
  const bytes = Buffer.from(base64, 'base64');
  if (bytes.length !== scene.bytes) {
    throw new Error(
      `${bytes.length} byte(s) arrived where the harness reported ${scene.bytes}`
    );
  }
  const path = join(
    dir,
    `${scene.scene}.${scene.width}x${scene.height}.${scene.order}.bin`
  );
  writeFileSync(path, bytes);
  return path;
}

async function main() {
  const { site, readbackDir, resultJson } = parseArgs(process.argv.slice(2));
  if (!existsSync(join(site, 'harness', 'index.html'))) {
    fail(`${site}/harness/index.html not found — build the site first`);
  }
  mkdirSync(readbackDir, { recursive: true });

  const server = await serve(site, { host: '127.0.0.1' });
  const browser = await launch(findBrowser(fail));
  let page;
  try {
    page = await openPage(browser);
    await page.send('Runtime.enable');
    await page.send('Page.enable');

    const url = `${server.origin}/harness/index.html`;
    await page.send('Page.navigate', { url });

    // Poll for the harness to finish. It sets `window.harnessDone` once it has
    // driven every scene, whatever the outcome — a page that never sets it is a
    // page that failed to boot, which the timeout turns into a hard failure.
    const deadline = Date.now() + RUN_TIMEOUT_MS;
    let done = false;
    while (Date.now() < deadline) {
      done = await evaluate(page, 'Boolean(window.harnessDone)');
      if (done) break;
      await pause(250);
    }
    if (!done) fail(`the harness did not finish within ${RUN_TIMEOUT_MS} ms`);

    const result = await evaluate(page, 'window.harnessResult');
    if (!result || !result.started) {
      const reason = result?.fatal ?? 'the harness never started';
      console.error(browser.stderr.slice(-20).join('\n'));
      fail(`the harness could not run: ${reason}`);
    }

    const scenes = result.scenes ?? [];
    // Pulled out of the page BEFORE the table is printed, so a scene whose
    // pixels could not be fetched or written shows in the table as what it is:
    // a scene that produced nothing to compare. Reported as a crack rather than
    // skipped — a gate that quietly compares ten of eleven scenes is a gate
    // that can go green while one is broken.
    for (const scene of scenes) {
      if (!scene.rendered) continue;
      try {
        await writeReadback(page, readbackDir, scene);
      } catch (error) {
        scene.rendered = false;
        scene.fatal = scene.fatal ?? `readback not saved: ${error.message}`;
      }
    }

    // Written after the readback loop and before the table, so the file says
    // exactly what the table says: the loop is where a scene whose pixels could
    // not be saved has its `rendered` turned back off, and a JSON written before
    // it would disagree with the run it is supposed to describe.
    if (resultJson) {
      writeFileSync(
        resultJson,
        `${JSON.stringify({ readbackDir, fatal: result.fatal ?? null, scenes }, null, 2)}\n`
      );
    }

    printTable(scenes);

    const rendered = scenes.filter((s) => s.rendered).length;
    console.log(
      `\nrender-harness-e2e: ${rendered}/${scenes.length} scene(s) rendered and saved to ${readbackDir}`
    );

    // WHENEVER IT IS SET, NOT ONLY WHEN THE HARNESS NEVER STARTED. A page that
    // booted, drove a scene and then had the wasm module abort under it sets
    // `started` *and* `fatal`, and printing only the table left the run looking
    // like a silent nothing — the reason was reachable only by reading
    // `window.harnessResult` over the DevTools protocol by hand.
    if (result.fatal) {
      console.error(`\nrender-harness-e2e: fatal: ${result.fatal}`);
    }

    if (scenes.length === 0) {
      console.error('render-harness-e2e: no scenes were driven');
      process.exitCode = 1;
      return;
    }
    // A SCENE THAT OPENED WHILE THE DEVICE WAS REFUSING ITS COMMANDS HAS NOT
    // RENDERED. WebGPU reports a refused command out of band rather than by
    // throwing, so the offscreen open completes either way and the state alone
    // cannot tell the two apart. Without this the gate could be wired green over
    // a backend whose every draw was rejected.
    const refused = scenes.filter((s) => (s.deviceErrors ?? []).length > 0);
    if (refused.length > 0) {
      console.error(
        `\nrender-harness-e2e: ${refused.length} scene(s) reported device errors:`
      );
      for (const scene of refused) {
        for (const error of scene.deviceErrors) {
          console.error(`  ${scene.scene}: ${error}`);
        }
        if (scene.deviceErrorsDropped > 0) {
          console.error(
            `  ${scene.scene}: and ${scene.deviceErrorsDropped} further error(s), not recorded`
          );
        }
      }
      process.exitCode = 1;
    }

    if (rendered < scenes.length) {
      // The crack list. Exit 1 so the gate cannot be wired green over a scene
      // the browser backend cannot get through — and so the comparator that
      // runs next is never mistaken for the whole verdict, since it can only
      // speak for the frames that exist.
      const firstError =
        scenes.find((s) => s.fatal)?.fatal?.split('\n')[0] ??
        scenes.find((s) => s.error)?.error ??
        scenes.find((s) => s.replayFailure)?.replayFailure ??
        'no error text';
      console.error(
        `render-harness-e2e: ${scenes.length - rendered} scene(s) did not render. First crack: ${firstError}`
      );
      process.exitCode = 1;
      return;
    }
    if (refused.length === 0) {
      console.log(
        'render-harness-e2e: every scene rendered through the browser backend; ' +
          'compare-readback decides whether the pixels are right'
      );
    }
  } finally {
    if (page) page.close();
    await server.close();
    browser.stop();
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(2);
});
