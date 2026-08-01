// Breakout in the browser: the page's half of the boot sequence and the rAF
// loop that drives it.
//
// The order below is not arbitrary — `apps/breakout/src/web.rs` specifies it,
// and each step exists because the one before it is a precondition:
//
//   prepare()  installs the storage backends, so the two `__crcbl_web_*`
//              storage ABIs stop answering 0
//   pre-load   fills the asset cache, so the game's first `read` is a hash
//              lookup rather than forty frames of `Pending`
//   restore    fills the save store and calls `opfs_ready`, so the high score
//              is there before `Game::new` asks for it
//   attach     tells the shell which canvas it drives and reports the first
//              size, which is what un-parks start-up
//   boot()     opens the shell and starts the device request
//   rAF        polls that request for a few frames, then runs the game
//
// THE FRAME LOOP IS OUT HERE, in JS, and that is the whole point of the wasm
// work in P5: `docs/plan/10-wasm-webgpu.md` requires the engine's frame to be a
// `fn tick(dt)` an outer loop drives rather than a `loop {}` that owns the
// thread, because in a browser the outer loop belongs to the browser.

import init from './crcbl_breakout.js';
import { attachShell } from '../../engine/shell.js';
import { startAudio } from '../../engine/audio.js';
import { drainLog, LOG_INFO } from '../../engine/log.js';
import {
  drainFetch,
  flushOpfs,
  opfsSettled,
  preloadAssets,
  restoreOpfs,
} from '../../engine/storage.js';
import { readUtf8 } from '../../engine/wasm.js';

/** Mirrors the `STATUS_*` constants in `apps/breakout/src/web.rs`. */
const STATUS = {
  IDLE: 0,
  PREPARED: 1,
  BOOTING: 2,
  RUNNING: 3,
  STOPPED: 4,
  FAILED: 5,
};

/** Must match `ASSET_BASE` in `apps/breakout/src/web.rs`. */
const ASSET_BASE = 'assets/';

/** Any non-zero id; it only has to match the canvas's `data-raw-handle`. */
const CANVAS_ID = 1;

const canvas = /** @type {HTMLCanvasElement} */ (
  document.getElementById('canvas')
);
const statusLine = /** @type {HTMLElement} */ (
  document.getElementById('status')
);
const detailLine = /** @type {HTMLElement} */ (
  document.getElementById('detail')
);
const statusBar = /** @type {HTMLElement} */ (
  document.getElementById('statusbar')
);
const stopButton = /** @type {HTMLButtonElement} */ (
  document.getElementById('stop')
);

/**
 * @param {string} text
 * @param {string} [detail]
 * @param {boolean} [fatal]
 */
function say(text, detail = '', fatal = false) {
  statusLine.textContent = text;
  detailLine.textContent = detail;
  statusLine.classList.toggle('fatal', fatal);
  // The indicator beside the text is driven from the same call, so it cannot
  // drift into decoration: a dot that says "running" while the demo has
  // stopped is worse than no dot.
  statusBar.classList.toggle('failed', fatal);
  if (fatal) statusBar.classList.remove('running');
}

/** Reflect a terminal state: nothing left to stop, nothing left running. */
function settle() {
  statusBar.classList.remove('running');
  stopButton.disabled = true;
}

/**
 * The asset keys to pre-load.
 *
 * Read from a manifest that ships with the demo rather than hard-coded here, so
 * adding an asset is a data change. A missing or empty manifest is not an
 * error: breakout's meshes, its font atlas and its shaders are all compiled
 * into the wasm module, so it currently has nothing to fetch — the pre-load
 * path is wired and exercised, and the list it is given is empty.
 *
 * @returns {Promise<string[]>}
 */
async function assetKeys() {
  try {
    const response = await fetch(`${ASSET_BASE}manifest.json`);
    if (!response.ok) return [];
    const manifest = await response.json();
    return Array.isArray(manifest?.keys) ? manifest.keys : [];
  } catch {
    return [];
  }
}

/**
 * @param {Record<string, Function>} exports
 * @param {WebAssembly.Memory} memory
 * @returns {string}
 */
function lastError(exports, memory) {
  return readUtf8(
    memory,
    exports.__crcbl_breakout_error_ptr(),
    exports.__crcbl_breakout_error_len()
  );
}

async function main() {
  if (!('gpu' in navigator)) {
    say(
      'This browser has no WebGPU.',
      'Crucible renders through WebGPU in the browser. Chrome or Edge 113+, or Firefox with WebGPU enabled, will run it.',
      true
    );
    return;
  }

  say('Loading the engine…');
  const exports = await init();
  const memory = /** @type {WebAssembly.Memory} */ (exports.memory);

  const log = () =>
    drainLog({
      memory,
      take: exports.__crcbl_breakout_log_take,
      ptr: exports.__crcbl_breakout_log_ptr,
    });

  if (exports.__crcbl_breakout_prepare() !== 1) {
    say('The engine refused to start.', lastError(exports, memory), true);
    log();
    return;
  }
  exports.__crcbl_breakout_log_level(LOG_INFO);

  say('Loading assets…');
  // Pre-load and restore run together: they touch different ABIs and different
  // storage, and the game needs both before it boots.
  const [, root] = await Promise.all([
    assetKeys().then((keys) =>
      preloadAssets({ exports, memory, keys, base: ASSET_BASE })
    ),
    restoreOpfs({ exports, memory }),
  ]);
  log();

  say('Opening a GPU device…');
  const shell = attachShell({ exports, memory, canvas, canvasId: CANVAS_ID });
  if (exports.__crcbl_breakout_boot() !== 1) {
    say(
      'The engine could not open a window.',
      lastError(exports, memory),
      true
    );
    log();
    return;
  }
  // The window exists now, so the size it needs can finally be delivered. The
  // one `attachShell` already sent had nowhere to go — see `syncSize`.
  shell.syncSize(true);

  // Not awaited: the worklet's `addModule` is a network round trip, and there
  // is nothing about it the first frames need. Its first refill request is
  // answered with silence until the game — and therefore its `AudioSource` —
  // exists, which is exactly what the audio ABI's "answers 0 until installed"
  // rule is for.
  const audio = startAudio({
    exports,
    memory,
    workletUrl: new URL('../../engine/audio-worklet.js', import.meta.url).href,
  });

  const flush = () => void flushOpfs({ exports, memory, root });
  // A write returns when it is *queued*; these two events are the last chance a
  // page gets to put it on the disk.
  document.addEventListener('visibilitychange', flush);
  window.addEventListener('pagehide', () => {
    // Teardown first, then drain: the game's last write is queued during the
    // frame `shutdown` runs, and draining before it would miss exactly that one.
    exports.__crcbl_breakout_shutdown();
    flush();
  });

  // The page's own close button. `__crcbl_web_close` is a *question* — the
  // engine answers it by accepting, tearing the frame down in order, and
  // reporting `STOPPED` — which is the same path a compositor's close button
  // takes on the desktop.
  document
    .getElementById('stop')
    ?.addEventListener('click', () => shell.requestClose());

  let announced = -1;

  /** @param {number} now */
  function frame(now) {
    // The shell's event-clock reference for this frame, before anything reads
    // an event timestamp.
    exports.__crcbl_web_frame(now);
    const status = exports.__crcbl_breakout_frame(now);
    log();
    drainFetch({ exports, memory });
    flush();

    if (status !== announced) {
      announced = status;
      if (status === STATUS.RUNNING) {
        say(
          'Playing.',
          '← → move · SPACE launches · click the canvas for keyboard and sound'
        );
        statusBar.classList.add('running');
      } else if (status === STATUS.STOPPED) {
        say('Stopped.', opfsSettled(exports) ? 'High score saved.' : 'Saving…');
        settle();
      } else if (status === STATUS.FAILED) {
        say('The demo stopped.', lastError(exports, memory), true);
        settle();
      }
    }

    if (status === STATUS.BOOTING || status === STATUS.RUNNING) {
      requestAnimationFrame(frame);
      return;
    }
    shell.dispose();
    void audio.then((a) => a?.context.close());
    flush();
  }
  requestAnimationFrame(frame);

  // A debug readout the page can be asked for from the console, rather than a
  // HUD competing with the one the engine draws itself.
  Object.defineProperty(globalThis, 'crcbl', {
    value: {
      exports,
      memory,
      status: () => exports.__crcbl_breakout_status(),
      audio: () => audio.then((a) => a?.stats()),
      saves: () => ({
        pending: exports.__crcbl_web_opfs_pending(),
        inFlight: exports.__crcbl_web_opfs_inflight(),
      }),
      assets: () => ({
        pending: exports.__crcbl_web_fetch_pending(),
        inFlight: exports.__crcbl_web_fetch_inflight(),
      }),
      logLevel: (/** @type {number} */ level) =>
        exports.__crcbl_breakout_log_level(level),
    },
  });
}

main().catch((error) => {
  console.error(error);
  say('The demo failed to load.', String(error), true);
  settle();
});
