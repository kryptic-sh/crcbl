// Loads the render-harness page in a real browser, drives every golden scene
// through the browser GPU backend, and prints — per scene — how far the
// offscreen open got. It is the browser end of the WebGPU parity gate: the one
// check that can say whether `crcbl-webgpu` can be driven through
// `crcbl::screenshot`'s offscreen path at all, which nothing native can answer.
//
// It reads its verdict out of `window.harnessResult` over the DevTools protocol
// rather than off a canvas — the harness renders to an offscreen target and
// reads it back into wasm memory, so there is no canvas to snapshot. That also
// means it needs none of the Xvfb / shared-image-device machinery
// `web/tools/browser-e2e.mjs` needs for canvas readback; headless plus
// SwiftShader is enough.
//
// USAGE
//   node web/tools/render-harness-e2e.mjs <site-dir> [--headed]
//
//   <site-dir>  A directory with `harness/index.html` and the wasm-bindgen
//               output beside it; `web/run-render-harness-e2e.sh` assembles one.
//
// ENVIRONMENT
//   CRCBL_CHROMIUM           Path to the Chromium/Chrome binary. Otherwise the
//                            usual four names are tried.
//   CRCBL_CHROMIUM_FLAGS     Extra flags, space-separated.
//   CRCBL_CHROMIUM_NO_SANDBOX=1  Add --no-sandbox (also added automatically as
//                            root, whose user namespaces a sandbox needs).
//
// EXIT CODES
//   0  every scene rendered (reached `opened`) — the state once the offscreen
//      surface command lands.
//   1  the harness ran but at least one scene did not render; the per-scene
//      table names the crack. This is the expected state today.
//   2  the harness could not run at all (no browser, no adapter, wasm failed).

import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

import { serve } from './serve.mjs';

const LAUNCH_TIMEOUT_MS = 30_000;
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

function findBrowser() {
  if (process.env.CRCBL_CHROMIUM) return process.env.CRCBL_CHROMIUM;
  const names = [
    'google-chrome',
    'google-chrome-stable',
    'chromium',
    'chromium-browser',
  ];
  for (const name of names) {
    // `spawnSync('command -v', …)` would be a shell; probe PATH directly.
    for (const dir of (process.env.PATH ?? '').split(':')) {
      if (dir && existsSync(join(dir, name))) return join(dir, name);
    }
  }
  return fail(
    `no browser found. Set CRCBL_CHROMIUM, or install one of: ${names.join(', ')}`
  );
}

function browserFlags(profile) {
  const flags = [
    ...(process.env.CRCBL_WEB_E2E_HEADED === '1' ? [] : ['--headless=new']),
    '--remote-debugging-port=0',
    `--user-data-dir=${profile}`,
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-dev-shm-usage',
    '--disable-background-networking',
    '--disable-component-update',
    '--disable-extensions',
    '--mute-audio',
    // Point both WebGPU (Dawn) and the Vulkan backend at SwiftShader, and lift
    // Chrome's refusal to expose WebGPU when the GPU is software-only. The same
    // set `web/tools/browser-e2e.mjs` documents at length; a CI runner has no
    // GPU, so this is the box it lands in.
    '--enable-unsafe-webgpu',
    '--use-webgpu-adapter=swiftshader',
    '--enable-features=Vulkan',
    '--use-vulkan=swiftshader',
  ];
  if (
    process.env.CRCBL_CHROMIUM_NO_SANDBOX === '1' ||
    process.getuid?.() === 0
  ) {
    flags.push('--no-sandbox');
  }
  const extra = (process.env.CRCBL_CHROMIUM_FLAGS ?? '')
    .split(' ')
    .filter(Boolean);
  return [...flags, ...extra];
}

async function launch(binary) {
  const profile = mkdtempSync(join(tmpdir(), 'crcbl-harness-e2e-'));
  const flags = browserFlags(profile);
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
      try {
        process.kill(-child.pid, 'SIGKILL');
      } catch {
        // Already gone, which is what this wanted.
      }
      rmSync(profile, { recursive: true, force: true });
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
  console.log(`\n${pad('scene', nameWidth)}  rendered  state    detail`);
  console.log('-'.repeat(nameWidth + 2 + 8 + 2 + 7 + 2 + 40));
  for (const scene of scenes) {
    const detail = scene.replayFailure
      ? `replay: ${scene.replayFailure.split('\n')[0]}`
      : scene.timedOut
        ? `timed out after ${scene.frames} frames`
        : scene.error || '';
    console.log(
      `${pad(scene.scene, nameWidth)}  ${pad(scene.rendered ? 'yes' : 'no', 8)}  ${pad(
        scene.stateName,
        7
      )}  ${detail}`
    );
  }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  const siteArg = process.argv[2];
  if (!siteArg) fail('usage: render-harness-e2e.mjs <site-dir>');
  const site = resolve(siteArg);
  if (!existsSync(join(site, 'harness', 'index.html'))) {
    fail(`${site}/harness/index.html not found — build the site first`);
  }

  const server = await serve(site, { host: '127.0.0.1' });
  const browser = await launch(findBrowser());
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
    printTable(scenes);

    const rendered = scenes.filter((s) => s.rendered).length;
    console.log(
      `\nrender-harness-e2e: ${rendered}/${scenes.length} scene(s) rendered through the browser backend`
    );

    if (scenes.length === 0) {
      console.error('render-harness-e2e: no scenes were driven');
      process.exitCode = 1;
      return;
    }
    if (rendered < scenes.length) {
      // The expected state until the offscreen surface command lands: every
      // scene refused at the same wall. Printed as the crack list, exit 1 so
      // the gate cannot be wired green while the wall stands.
      const firstError =
        scenes.find((s) => s.error)?.error ??
        scenes.find((s) => s.replayFailure)?.replayFailure ??
        'no error text';
      console.error(
        `render-harness-e2e: ${scenes.length - rendered} scene(s) did not render. First crack: ${firstError}`
      );
      process.exitCode = 1;
      return;
    }
    console.log(
      'render-harness-e2e: every scene rendered through the browser backend'
    );
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
