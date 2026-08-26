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

import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
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

const RUN_TIMEOUT_MS = 180_000;

// `stopEverything` and the exit hooks that call it are in
// `web/tools/browser-launch.mjs`, with the launch that registers each browser.
// It is called here as well as from those hooks so that this path says at the
// point of the exit what it leaves behind: nothing.
function fail(message) {
  console.error(`render-harness-e2e: ${message}`);
  stopEverything();
  process.exit(2);
}

// ---------------------------------------------------------------------------
// Browser launch
// ---------------------------------------------------------------------------

// `findBrowser`, the launch, the CDP client and the kill are in
// `web/tools/browser-launch.mjs`, shared with the two gates beside this one.
// This gate adds nothing of its own to the launch: it asks for `ADAPTER` and
// takes the rest.
//
// **`swiftshader` is the default and stays the default**: this harness renders
// offscreen and compares every scene against a golden image, so it is asking a
// specific rasteriser for pixels a comparator will hold to a tolerance, and
// which device produced them is not a detail to leave to the machine.
//
// **It is a default rather than a pin since 2026-08-20, and only because one
// platform has no such device.** Chrome's Dawn has no Vulkan backend on macOS
// and therefore no software adapter at all — `web/tools/probe-e2e.mjs` carries
// that table — so a pinned SwiftShader is not "the deterministic choice" there,
// it is "this harness cannot run". `hardware` exists for that case and changes
// what the run means: the pixels come from the machine's own GPU, so comparing
// them against a golden blessed on lavapipe is a tolerance question nobody has
// answered. The intended use is to compare them against **that machine's own
// native backend** instead — `web/run-cross-backend-e2e.sh --reference mtl` —
// which needs no committed reference and so cannot drift with one.
const ADAPTER = process.env.CRCBL_WEB_E2E_ADAPTER ?? 'swiftshader';
if (!['hardware', 'swiftshader'].includes(ADAPTER)) {
  fail(
    `CRCBL_WEB_E2E_ADAPTER must be hardware or swiftshader, got "${ADAPTER}" ` +
      `— there is no \`auto\` here, because a harness that silently changed ` +
      `rasteriser would change what its comparison means without saying so`
  );
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
 * DevTools protocol is JSON, and every frame of it in one reply is several
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

  // **Printed, because which rasteriser produced these pixels decides what the
  // comparison downstream means.** A run that quietly changed adapter would
  // change the meaning of every scene's verdict without saying so, which is the
  // whole reason there is no `auto` here.
  console.log(`render-harness-e2e: adapter ${ADAPTER}`);

  const server = await serve(site, { host: '127.0.0.1' });
  const browser = await launch({
    binary: findBrowser(fail),
    mode: ADAPTER,
    profilePrefix: 'crcbl-harness-e2e-',
    fail,
  });
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
    //
    // Written out rather than run through the shared `until`, which the two
    // check gates use: that one answers `null` on the deadline and swallows a
    // throwing probe, and neither is right here. A page that cannot be
    // evaluated in at all is this gate's exit 2 rather than a condition not met
    // yet, and the deadline is a failure rather than an answer. The interval is
    // coarse because one boolean over the wire every quarter second is enough
    // to notice a run that takes minutes.
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
    // skipped — a gate that quietly compares all but one of the scenes is a gate
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
    // **Order matters: the browser dies before the server is awaited.** Chromium
    // holds a keep-alive socket to `server`, and `close()` does not resolve
    // while a connection is open, so awaiting it first leaves `browser.stop()`
    // unreachable and the process alive. `error-scope-bench.mjs` has always had
    // this order; this file had it reversed, and that is what hung the Windows
    // leg of this gate for three hours a run after every scene had
    // already rendered. `serve.mjs`'s `close` now ends the sockets itself as
    // well, so neither half depends on the other being right.
    if (page) page.close();
    browser.stop();
    await server.close();
  }
}

/**
 * How long the process may stay alive after `main` has finished.
 *
 * Teardown is a kill and a socket close; it takes well under a second. This is
 * generous by two orders so that a slow but working exit is never cut short.
 */
const EXIT_DEADLINE_MS = 60_000;

main()
  .then(() => {
    // **A watchdog that costs nothing when it is not needed.** The timer is
    // `unref`'d, so it does not itself keep the process alive: if teardown left
    // the event loop empty — the healthy path — node exits and this never
    // fires. It fires only when something else is still holding the loop open,
    // which is the state that stalled the Windows leg of this gate for three
    // hours a run with every scene already rendered and every result already
    // written. Whatever is holding it, the answer is not to wait: the work is
    // done, so say so and go.
    const watchdog = setTimeout(() => {
      console.error(
        'render-harness-e2e: teardown finished but something is still holding ' +
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
