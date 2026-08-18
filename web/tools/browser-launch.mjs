// Finding, configuring and killing the browser the three gates in this
// directory drive.
//
// `browser-e2e.mjs`, `probe-e2e.mjs` and `render-harness-e2e.mjs` each own a
// page, a set of checks and a verdict — but all three need the same browser,
// found the same way and started with the same flags, and that half lived here
// three times over. It was pulled out when the gates were asked to run on macOS
// and Windows, because all three copies were wrong in the same three ways at
// once and a fourth copy of the fix was not worth having.
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

import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, rmSync } from 'node:fs';
import { delimiter, join } from 'node:path';

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
    console.error(`  ..   left ${profile} behind: ${error.message}`);
  }
}
