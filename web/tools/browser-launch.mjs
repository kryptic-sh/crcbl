// Finding, configuring, driving and killing the browser the three gates in this
// directory drive.
//
// `browser-e2e.mjs`, `probe-e2e.mjs` and `render-harness-e2e.mjs` each own a
// page, a set of checks and a verdict — but all three need the same browser,
// found the same way and started with the same flags, and that half lived here
// three times over. It was pulled out when the gates were asked to run on macOS
// and Windows, because all three copies were wrong in the same three ways at
// once and a fourth copy of the fix was not worth having.
//
// What is here is everything up to the first check: the flags, the launch and
// its `DevToolsActivePort` poll, the kill and the exit hooks that guarantee it,
// a CDP client, the fresh tab, and `Runtime.evaluate`. Each gate keeps its own
// `fail` — the name it prints and the exit code it chooses are its own — and
// hands it in, the way {@link findBrowser} has always taken it.
//
// EVERYTHING PLATFORM-SPECIFIC ABOUT LAUNCHING CHROME IS IN THIS FILE, and none
// of it is a detail:
//
//   * **Where the binary is.** A bare name on `PATH` finds Chrome on Linux and
//     nothing on either other platform: macOS installs a bundle under
//     `/Applications` and Windows a `chrome.exe` under Program Files, neither
//     of which is on `PATH`. `PATH` itself is `;`-separated on Windows, so even
//     the search was Linux-only.
//   * **Which GPU flags mean "the real device".** `--use-angle=vulkan` is right
//     on Linux, and on macOS it asks for a backend Chrome's Dawn does not have
//     there — Metal is the only one. Windows wants `d3d11`.
//   * **How to kill it.** `process.kill(-pid)` is a POSIX process-group kill and
//     the only way to take a Chromium's half-dozen processes down together. A
//     negative pid is not a process group on Windows: the call throws, the
//     browser survives holding handles on the profile directory, and the
//     `rmSync` that follows fails with `EPERM` — from inside an exit handler,
//     which turns a run that passed every check red at the very last moment.
//
// SwiftShader IS NOT A FALLBACK EVERYWHERE. It is Chromium's bundled software
// *Vulkan*, so `--use-vulkan=swiftshader` asks for a backend that exists on
// Linux and Windows and not on macOS. On Windows it is worse than absent: it
// moves Dawn to SwiftShader while Chromium's shared-image device stays on
// D3D11, and a canvas handed between two different devices reads back as
// uninitialised memory rather than as black — the two-device mismatch
// `docs/backlog.md` records from Chrome 151, and the reason the gates run
// Windows in `hardware` mode.

import { spawn, spawnSync } from 'node:child_process';
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';

// ---------------------------------------------------------------------------
// Printing, which has to survive the process being killed
// ---------------------------------------------------------------------------

/**
 * Write one line to a file descriptor and do not return until it is written.
 *
 * **`console.log` does not do this.** Node's stdout is asynchronous when it is
 * a pipe on POSIX, and CI runs every gate in this directory as
 * `node … 2>&1 | tee`, so a "printed" line can still be sitting in a queue when
 * the step's `timeout-minutes` kills the process — at which point it is never
 * written at all. A gate that hangs is read backwards from its last line, and
 * that reading is worthless if the last line in the log is not the last line
 * the run reached.
 *
 * Two failures are expected of a pipe and neither is this caller's to handle.
 * `EPIPE` means the reader is gone, so there is nowhere left to print and
 * nothing to say about it. `EAGAIN` means the pipe is full — Node puts stdout
 * in non-blocking mode, so a full pipe refuses the write rather than waiting —
 * and retrying in place *is* the wait, spinning against a reader that is
 * draining as fast as it can.
 */
function emit(fd, line) {
  const bytes = Buffer.from(`${line}\n`, 'utf8');
  let at = 0;
  while (at < bytes.length) {
    try {
      at += writeSync(fd, bytes.subarray(at));
    } catch (error) {
      const code = /** @type {NodeJS.ErrnoException} */ (error).code;
      if (code === 'EPIPE') return;
      if (code !== 'EAGAIN') throw error;
    }
  }
}

/** {@link emit} to stdout: `console.log`, minus the queue. */
export const say = (line = '') => emit(1, line);

/** {@link emit} to stderr: `console.error`, minus the queue. */
export const warn = (line = '') => emit(2, line);

/**
 * The browser binaries worth trying on this platform, in preference order.
 *
 * `names` are looked for on `PATH`, `paths` are absolute and checked as they
 * are. `google-chrome` leads the Linux list because that is what GitHub's Ubuntu
 * images ship as a real binary; a `chromium` that is a snap wrapper cannot see a
 * `--user-data-dir` under `/tmp`, which is a miserable failure to debug from a
 * CI log.
 *
 * The Windows list is built from the environment rather than from a literal
 * `C:\Program Files`, because the drive and the localised directory name are
 * both the machine's to choose. CI pins `CRCBL_CHROMIUM` from the registry —
 * `HKLM:\…\App Paths\chrome.exe`, which is where the installer records it — and
 * this list is what a developer who has not done that falls back on.
 */
function candidates() {
  if (process.platform === 'darwin') {
    return {
      names: ['google-chrome', 'chromium'],
      paths: [
        '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
        '/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary',
        '/Applications/Chromium.app/Contents/MacOS/Chromium',
      ],
    };
  }
  if (process.platform === 'win32') {
    const suffix = join('Google', 'Chrome', 'Application', 'chrome.exe');
    const roots = [
      process.env.ProgramFiles,
      process.env['ProgramFiles(x86)'],
      process.env.LOCALAPPDATA,
    ];
    return {
      names: ['chrome.exe', 'chromium.exe'],
      paths: roots.filter(Boolean).map((root) => join(root, suffix)),
    };
  }
  return {
    names: [
      'google-chrome',
      'google-chrome-stable',
      'chromium',
      'chromium-browser',
    ],
    paths: [],
  };
}

/**
 * The first browser binary on this machine that could drive WebGPU.
 *
 * `fail` is the caller's own — each gate prefixes its name and stops whatever it
 * has already started — and this never returns without a binary: it either hands
 * one back or calls `fail`, which exits.
 *
 * @param {(message: string) => never} fail
 * @returns {string}
 */
/**
 * Reads Chrome's `DevToolsActivePort`, or answers `null` if it is not readable
 * *yet*.
 *
 * # Why this is not just `readFileSync`
 *
 * Chrome writes this file and **keeps the handle open**. On Windows that handle
 * is exclusive, so a reader that arrives while Chrome still holds it gets
 * `EBUSY: resource busy or locked` rather than the contents — and the callers
 * here poll on a deadline, so the right answer to that is "not yet", not an
 * exception. A first Windows CI run died exactly this way, before it could even
 * reach the question it was there to ask.
 *
 * The file is also written in two steps, so a read can legitimately land on a
 * half-written file with a port and no path. That is the same "not yet".
 *
 * `ENOENT` is included for the window before Chrome has written it at all.
 * Anything else is thrown, **including `EACCES`**: a caller that retried past a
 * permissions fault would spend its whole deadline and then report "the browser
 * never wrote DevToolsActivePort", which is a true sentence about the wrong
 * problem. If a Windows run ever produces `EACCES` for a file Chrome merely
 * holds, add it here with that run cited — not in advance.
 *
 * @param {string} portFile
 * @returns {{port: string, path: string} | null}
 */
/**
 * How long to wait for Chrome to write `DevToolsActivePort` before giving up.
 *
 * **Here rather than in each driver, because the three of them disagreed.** On
 * 2026-08-20 two were raised from thirty seconds and the third was not — it
 * held an unnamed `30_000` literal — which is precisely the failure mode
 * duplication produces. It was the first piece of the plumbing to be shared;
 * the loop it bounds followed it into {@link launch} here.
 *
 * **Two minutes, and it was thirty seconds.** Three of the last eight
 * non-cancelled Pages runs on `main` failed at this deadline, and the failing
 * run's own stderr says the browser was still starting rather than wedged: it
 * reached dbus initialisation 22 seconds in and had not written the port file at
 * 30. Every caller's loop separately reports a browser that *exited*, so this
 * deadline is only ever reached by one that is alive and has not got there yet.
 *
 * For scale, a healthy runner drives a whole gate — launch, eleven scenes and
 * every readback — in 14 to 19 seconds. A launch alone approaching thirty is
 * already pathological, so this is headroom for a runner having a bad minute and
 * it is spent only on the path that would otherwise fail.
 */
export const LAUNCH_TIMEOUT_MS = 120_000;

export function readDevToolsPort(portFile) {
  let contents;
  try {
    contents = readFileSync(portFile, 'utf8');
  } catch (error) {
    const code = /** @type {NodeJS.ErrnoException} */ (error).code;
    if (code === 'EBUSY' || code === 'ENOENT') {
      return null;
    }
    throw error;
  }
  const [port, path] = contents.split('\n');
  return port && path ? { port, path } : null;
}

export function findBrowser(fail) {
  const explicit = process.env.CRCBL_CHROMIUM;
  if (explicit) {
    if (!existsSync(explicit))
      fail(`CRCBL_CHROMIUM=${explicit} does not exist`);
    return explicit;
  }
  const { names, paths } = candidates();
  for (const name of names) {
    // `spawnSync('command -v', …)` would be a shell; probe PATH directly.
    for (const dir of (process.env.PATH ?? '').split(delimiter)) {
      if (dir && existsSync(join(dir, name))) return join(dir, name);
    }
  }
  for (const path of paths) {
    if (existsSync(path)) return path;
  }
  return fail(
    `no browser found on ${process.platform}. Tried ${names.join(', ')} on PATH` +
      (paths.length ? `, and ${paths.join(', ')}` : '') +
      '.\n' +
      '  Set CRCBL_CHROMIUM to a Chromium or Chrome binary with WebGPU support.'
  );
}

/**
 * The GPU flags for one adapter mode on this platform.
 *
 * `hardware` is what a visitor to the Pages URL gets, and what a machine with a
 * real device — or a runner with a paravirtual one — is asked for. The pair it
 * pushes is the one that stops the GPU process falling back to ANGLE's
 * SwiftShader GL, which leaves `chrome://gpu` reporting
 * `webgpu: unavailable_software` however capable the machine is; which pair that
 * is depends entirely on the platform's own graphics API.
 *
 * `swiftshader` is Chromium's bundled software Vulkan, for a runner with no GPU
 * at all. Both halves are needed and neither is enough alone: the WebGPU pair
 * moves *Dawn*, and `--use-vulkan=swiftshader` moves the **shared image** device
 * Chromium hands canvases around on. A canvas is rendered by one and read back
 * by the other, so when they are different Vulkan implementations Chrome cannot
 * hand the texture across:
 *
 *   AssociateMailbox: Accessing an uncleared texture requires passing a usage
 *   that supports lazy clearing
 *   GPUDevice: [Invalid Texture] is invalid … While validating
 *   CopyTextureForBrowser
 *
 * and the snapshot is uninitialised memory after that — largely zero-alpha,
 * which is what makes it read as transparent black. Measured on Chromium 151.
 *
 * `--enable-unsafe-webgpu` is on both paths because it is what lifts Chrome's
 * refusal to expose WebGPU when the GPU feature status is anything short of
 * fully enabled — the box a headless runner lands in whether its adapter is
 * SwiftShader, a paravirtual Metal device or a WARP one.
 */
function gpuFlags(mode) {
  if (mode !== 'hardware') {
    return [
      '--enable-unsafe-webgpu',
      '--use-webgpu-adapter=swiftshader',
      '--enable-features=Vulkan',
      '--use-vulkan=swiftshader',
    ];
  }
  if (process.platform === 'darwin') {
    // Chrome's Dawn has no Vulkan backend on macOS — Metal is the only one, and
    // asking for Vulkan here gets an adapter that cannot exist.
    return ['--enable-unsafe-webgpu', '--use-angle=metal'];
  }
  if (process.platform === 'win32') {
    return ['--enable-unsafe-webgpu', '--use-angle=d3d11'];
  }
  return [
    '--enable-unsafe-webgpu',
    '--enable-features=Vulkan',
    '--use-angle=vulkan',
  ];
}

/**
 * The flags, and why each one is here.
 *
 * Every one of these was measured rather than copied — on Chromium 150 first,
 * and the SwiftShader set again on 151. Without the WebGPU pair for the chosen
 * mode, `navigator.gpu.requestAdapter()` resolves to `null` in headless and a
 * demo stops at its own "this browser has no WebGPU" banner.
 *
 * `extra` is the caller's own, inserted before the GPU flags: the demo gate
 * fixes a window size there so its pixel counts mean the same thing on every
 * machine, and the other two have no canvas to size.
 *
 * @param {{ profile: string, mode: string, extra?: string[] }} options
 */
export function browserFlags({ profile, mode, extra = [] }) {
  const flags = [
    // Modern headless. The old one is a separate browser with no GPU stack at
    // all, so WebGPU is simply absent there. `CRCBL_WEB_E2E_HEADED=1` drops it
    // for a run inside Xvfb — or on a Windows runner, whose own desktop session
    // is what the canvas reaches a compositor through.
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
    // updater's failures are noise in the log a gate prints on failure.
    '--disable-background-networking',
    '--disable-component-update',
    '--disable-extensions',
    // These gates boot real games and press real keys, so they play their cues
    // out of the machine's speakers. Nothing asserts anything about audio, so
    // muting the output costs no coverage; the `AudioContext` and the worklet
    // still run, which is what `smoke.mjs` and the shim's own checks care about.
    '--mute-audio',
    ...extra,
    ...gpuFlags(mode),
  ];

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
  // flags are the part of these harnesses most likely to need one more switch on
  // a runner nobody here has, and the alternative to an escape hatch is a
  // patched copy of this file. Printed with the rest of the command line, so a
  // run that used one says so.
  const env = (process.env.CRCBL_CHROMIUM_FLAGS ?? '')
    .split(' ')
    .filter(Boolean);
  return [...flags, ...env];
}

/**
 * Kills the browser and throws its profile directory away.
 *
 * On POSIX the negative pid is the process *group* `detached: true` created:
 * Chromium is half a dozen processes and a `kill` aimed at the parent leaves the
 * GPU process and the zygotes behind when the parent is wedged. `SIGKILL` rather
 * than `SIGTERM` because there is nothing to save — the profile goes next — and
 * a Chromium that ignores the polite signal is exactly the one worth being rude
 * to.
 *
 * Windows has no process groups to signal, so `taskkill /T` walks the tree
 * instead, and it is `spawnSync` rather than `spawn` because the profile is
 * removed on the very next line and a process still holding a handle on it
 * cannot be. The removal retries for the same reason: a handle can outlive the
 * process that held it by a moment.
 *
 * A profile that will not go is *reported* and not thrown, on every platform. It
 * is a temporary directory, this runs from an exit handler, and a run that
 * passed every check must not go red over a directory nobody will look at.
 *
 * @param {import('node:child_process').ChildProcess} child
 * @param {string} profile
 */
export function stopBrowser(child, profile) {
  if (child.pid) {
    try {
      if (process.platform === 'win32') {
        spawnSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], {
          stdio: 'ignore',
        });
      } else {
        process.kill(-child.pid, 'SIGKILL');
      }
    } catch {
      // Already gone, which is the outcome this wanted.
    }
  }
  try {
    rmSync(profile, {
      recursive: true,
      force: true,
      maxRetries: 20,
      retryDelay: 100,
    });
  } catch (error) {
    warn(`  ..   left ${profile} behind: ${error.message}`);
  }
}

// ---------------------------------------------------------------------------
// The launch, and the browsers this process has to answer for
// ---------------------------------------------------------------------------

/**
 * Every browser {@link launch} started and has not stopped.
 *
 * A leaked Chromium is not a tidiness problem: it holds a GPU context and a
 * profile directory, and a developer who runs a gate a few times ends up with
 * several of them.
 *
 * @type {Set<{ stop: () => void }>}
 */
const running = new Set();

/** Kills every browser this process started. Safe to call twice. */
export function stopEverything() {
  for (const browser of running) browser.stop();
  running.clear();
}

// `process.exit` does not unwind, so a `finally` around the run is not enough on
// its own, and a signal does not run one at all.
//
// **Registered here, on import, because one gate did not have them.**
// `render-harness-e2e.mjs` stopped its browser from a `finally` in `main` and
// called `process.exit` from `fail` — so every startup error it diagnosed (a
// harness that never finished, a harness that could not run) left the Chromium
// and its profile directory behind, and so did every Ctrl-C. The two gates that
// did register these had them written out twice.
process.on('exit', stopEverything);
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => {
    stopEverything();
    process.exit(130);
  });
}

/** Resolves after `ms`. */
export const pause = (ms) => new Promise((ok) => setTimeout(ok, ms));

/**
 * Starts the browser and returns it, listening, with its DevTools endpoint.
 *
 * The endpoint comes from `DevToolsActivePort`, which Chrome writes into the
 * profile once it is listening. Polling that file is how the launch is
 * synchronised: a sleep would be a flake on a slow machine and wasted time on a
 * fast one.
 *
 * `profilePrefix` names the throwaway profile directory, so a leaked one says
 * which gate left it. `mode` and `extra` go straight to {@link browserFlags}.
 * `fail` is the caller's own, as it is for {@link findBrowser}: it prefixes the
 * gate's name and does not return, and the browser is registered above before
 * the poll begins, so the exit hook kills it whichever way `fail` leaves.
 *
 * @param {{
 *   binary: string,
 *   mode: string,
 *   profilePrefix: string,
 *   extra?: string[],
 *   fail: (message: string) => never,
 * }} options
 * @returns {Promise<{
 *   stderr: string[],
 *   flags: string[],
 *   endpoint: string,
 *   stop: () => void,
 * }>}
 */
export async function launch({
  binary,
  mode,
  profilePrefix,
  extra = [],
  fail,
}) {
  const profile = mkdtempSync(join(tmpdir(), profilePrefix));
  const flags = browserFlags({ profile, mode, extra });
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
      warn(stderr.join('\n'));
      fail(`the browser stopped before it listened (${exited})`);
    }
    const endpoint = readDevToolsPort(portFile);
    if (endpoint) {
      browser.endpoint = `ws://127.0.0.1:${endpoint.port}${endpoint.path}`;
      return browser;
    }
    await pause(50);
  }
  warn(stderr.join('\n'));
  return fail('the browser never wrote DevToolsActivePort');
}

// ---------------------------------------------------------------------------
// A Chrome DevTools Protocol client, in about forty lines
// ---------------------------------------------------------------------------
//
// Node 22 shipped a global `WebSocket`, so a CDP session needs no library. The
// protocol is JSON both ways: `{ id, method, params }` out, `{ id, result }` or
// `{ method, params }` back.

/**
 * How long one CDP command may go unanswered before the gate abandons it.
 *
 * Generous by two orders, on the evidence: instrumented on 2026-08-28, the
 * slowest command in a whole quarry run was a 234 ms `Runtime.evaluate` that
 * read the canvas back as a data URL, and every other one was under 50 ms. So
 * this is not a budget any working command has to fit — it is the line past
 * which waiting longer has stopped being useful, and the step caps it has to
 * beat are measured in minutes.
 */
export const CDP_DEADLINE_MS = 30_000;

/** How much of the command's own parameters that failure quotes back. */
const CDP_PARAMS_REPORTED = 200;

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
      // **The deadline CDP does not have.** A command is a promise that
      // settles when Chrome answers, and Chrome is under no obligation to:
      // `Runtime.evaluate` with `awaitPromise` waits on a page promise that
      // may never resolve, and nothing below this line would ever notice. The
      // macOS leg of the Pages workflow has burnt its whole step cap three
      // times over waiting for something it could not name, which is the state
      // this removes: `until` has had a deadline all along, and this is the
      // same guarantee for the layer underneath it. Giving up after
      // {@link CDP_DEADLINE_MS} and naming the command turns a silent cap into
      // a failure someone can act on.
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(
          new Error(
            `${method} went unanswered for ${CDP_DEADLINE_MS} ms: ` +
              JSON.stringify(params).slice(0, CDP_PARAMS_REPORTED)
          )
        );
      }, CDP_DEADLINE_MS);
      // The timer must not be what keeps the process alive once the answer is
      // in — it is cleared on either outcome, and `unref` covers the window
      // before that.
      timer.unref?.();
      const settle = (/** @type {(value: any) => void} */ done) => (value) => {
        clearTimeout(timer);
        done(value);
      };
      this.#pending.set(id, {
        resolve: settle(resolve),
        reject: settle(reject),
      });
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
export async function openPage(browser) {
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
export async function evaluate(page, expression) {
  let result;
  try {
    result = await page.send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
  } catch (error) {
    // The command's own parameters carry the expression, but truncated to the
    // first couple of hundred characters — and the checks here hand over whole
    // page-side functions, which is exactly the case where the interesting
    // part is not at the front. Say which expression, on one line, so the
    // failure names a call site rather than a protocol method.
    throw new Error(
      `${error.message}\n  evaluating: ${expression.replace(/\s+/g, ' ').trim()}`
    );
  }
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
 *
 * `timeout` has no default here on purpose: the two gates that poll take theirs
 * from their own `--timeout`, and a default in this file would be a third
 * deadline able to disagree with both.
 */
export async function until(probe, timeout) {
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
