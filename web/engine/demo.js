// A demo in the browser: the page's half of the boot sequence and the rAF loop
// that drives it. One file for every demo — `web/demos/<name>/main.js` is the
// ABI binding and the two strings that differ, and nothing else.
//
// WHY THIS IS SHARED. `apps/breakout/src/web.rs` and `apps/flappy/src/web.rs`
// are the same file with a different symbol prefix (see `docs/backlog.md`), and
// their shims were the same file too — 288 lines each, differing in the sample
// name, one status line and one comment. A copy is not a template: the first
// time flappy shipped, it carried breakout's control hint into production,
// because the only thing keeping the two in step was remembering to edit both.
//
// The order below is not arbitrary — `apps/<sample>/src/web.rs` specifies it,
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
//
// THE SAMPLE'S OWN SYMBOLS STAY IN THE SAMPLE'S OWN FILE. `bind` is handed the
// raw exports and returns this sample's `__crcbl_<sample>_*` calls, written out
// literally there. That is not ceremony: `web/tools/check-exports.mjs` finds the
// symbols a shim uses by scanning for `.__crcbl_…` in the JS, so building the
// names here with a template literal would take every sample-specific symbol
// out of the gate's sight and leave a typo to be found in a browser.

import { attachShell } from './shell.js';
import { startAudio } from './audio.js';
import { takeCommandStream } from './gpu-transport.js';
import { drainLog, LOG_INFO } from './log.js';
import {
  drainFetch,
  flushOpfs,
  opfsSettled,
  preloadAssets,
  restoreOpfs,
} from './storage.js';
import { readUtf8 } from './wasm.js';

/** Mirrors the `STATUS_*` constants in `apps/<sample>/src/web.rs`. */
export const STATUS = {
  IDLE: 0,
  PREPARED: 1,
  BOOTING: 2,
  RUNNING: 3,
  STOPPED: 4,
  FAILED: 5,
  PAUSED: 6,
};

/** Must match `ASSET_BASE` in `apps/<sample>/src/web.rs`. */
const ASSET_BASE = 'assets/';

/** Any non-zero id; it only has to match the canvas's `data-raw-handle`. */
const CANVAS_ID = 1;

/**
 * This sample's slice of the engine ABI, with every symbol spelled out.
 *
 * @typedef {object} SampleApi
 * @property {() => number} prepare
 * @property {() => number} boot
 * @property {(now: number) => number} frame
 * @property {() => number} status
 * @property {() => void} shutdown
 * @property {(level: number) => void} logLevel
 * @property {() => number} logTake
 * @property {() => number} logPtr
 * @property {() => number} errorPtr
 * @property {() => number} errorLen
 */

/**
 * @typedef {object} DemoSpec
 * @property {() => Promise<Record<string, any>>} init the wasm-bindgen glue's
 *   default export, which resolves to the instance's raw exports
 * @property {(exports: Record<string, any>) => SampleApi} bind
 * @property {string} hint what to press, shown beside "Playing."
 * @property {string} [savedLabel] what the demo persists, shown on stop
 */

/**
 * Boot a demo into the window `templates/demo-window.html` renders.
 *
 * @param {DemoSpec} spec
 */
export function bootDemo(spec) {
  const { init, bind, hint, savedLabel = 'High score' } = spec;

  // The ids come from `web/templates/demo-window.html`, which every demo page
  // includes — so there is one place they are written and one place they are
  // read.
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
   * Read from a manifest that ships with the demo rather than hard-coded here,
   * so adding an asset is a data change. A missing or empty manifest is not an
   * error: a sample whose meshes, font atlas and shaders are all compiled into
   * the wasm module has nothing to fetch — the pre-load path is wired and
   * exercised, and the list it is given is empty.
   *
   * Relative, and therefore resolved against the *page*: `fetch` uses the
   * document's base URL, not this module's, so `/demos/flappy/` reads
   * `/demos/flappy/assets/` even though the code lives in `/engine/`.
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
   * @param {SampleApi} api
   * @param {WebAssembly.Memory} memory
   * @returns {string}
   */
  function lastError(api, memory) {
    return readUtf8(memory, api.errorPtr(), api.errorLen());
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

    // `navigator.gpu` existing is only half the question, and the half that is
    // never the one that fails on a real machine. The property is there on
    // every Chrome build; `requestAdapter()` still resolves to `null` when the
    // GPU process cannot reach a device — a blocklisted driver, a
    // `--render-node-override` naming the wrong node on a hybrid-graphics
    // laptop, a session with no GPU. Asked here rather than left to the engine
    // for two reasons: the answer arrives before the megabytes of wasm below
    // it, and it is the only place that can say it in a sentence aimed at a
    // person instead of at whoever reads the backend's error.
    let adapter = null;
    let refusal = '';
    try {
      adapter = await navigator.gpu.requestAdapter();
    } catch (error) {
      refusal = String(error);
    }
    if (!adapter) {
      say(
        'This browser has WebGPU, but no GPU to run it on.',
        refusal ||
          'navigator.gpu.requestAdapter() returned no adapter. The browser’s own GPU report — chrome://gpu, or about:support in Firefox — says why, but read past the WebGPU line: it can say “Hardware accelerated” while every adapter is still refused. On Linux, Chrome runs WebGPU on Vulkan, so a “Vulkan: Disabled” line there is the usual reason.',
        true
      );
      return;
    }

    say('Loading the engine…');
    const exports = await init();
    const memory = /** @type {WebAssembly.Memory} */ (exports.memory);
    const api = bind(exports);

    const log = () => drainLog({ memory, take: api.logTake, ptr: api.logPtr });

    if (api.prepare() !== 1) {
      say('The engine refused to start.', lastError(api, memory), true);
      log();
      return;
    }
    api.logLevel(LOG_INFO);

    say('Loading assets…');
    // Pre-load and restore run together: they touch different ABIs and
    // different storage, and the game needs both before it boots.
    const [, root] = await Promise.all([
      assetKeys().then((keys) =>
        preloadAssets({ exports, memory, keys, base: ASSET_BASE })
      ),
      restoreOpfs({ exports, memory }),
    ]);
    log();

    say('Opening a GPU device…');
    const shell = attachShell({ exports, memory, canvas, canvasId: CANVAS_ID });
    if (api.boot() !== 1) {
      say('The engine could not open a window.', lastError(api, memory), true);
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
      workletUrl: new URL('./audio-worklet.js', import.meta.url).href,
    });

    const flush = () => void flushOpfs({ exports, memory, root });
    // A write returns when it is *queued*; these two events are the last chance
    // a page gets to put it on the disk.
    document.addEventListener('visibilitychange', flush);
    window.addEventListener('pagehide', () => {
      // Teardown first, then drain: the game's last write is queued during the
      // frame `shutdown` runs, and draining before it would miss exactly that
      // one.
      api.shutdown();
      flush();
    });

    // The page's own close button. `__crcbl_web_close` is a *question* — the
    // engine answers it by accepting, tearing the frame down in order, and
    // reporting `STOPPED` — which is the same path a compositor's close button
    // takes on the desktop.
    stopButton?.addEventListener('click', () => shell.requestClose());

    // THE COMMAND STREAM'S ARGUMENT, BUILT ONCE. The drain below runs on every
    // frame of every demo, so the not-ready path has to cost a single integer
    // call and nothing else — a fresh `{ exports, memory }` per frame would be
    // an allocation per frame for a poll that answers "nothing" every time.
    const gpuStream = { exports, memory };

    let announced = -1;

    /** @param {number} now */
    function frame(now) {
      // The shell's event-clock reference for this frame, before anything reads
      // an event timestamp.
      exports.__crcbl_web_frame(now);
      const status = api.frame(now);
      log();
      drainFetch({ exports, memory });
      // NOTHING ENCODES INTO THE STREAM YET, so this returns `null` on every
      // frame and there is nothing to replay. `crcbl-webgpu` is linked and its
      // three `__crcbl_web_gpu_stream_*` exports are in the artifact, but no HAL
      // implementation writes through `StreamChannel::encode` and nothing calls
      // `crcbl_webgpu::web::install`, so `len` answers 0 — the documented "a
      // shim that started before the engine did" case, and not a failure.
      //
      // It is called anyway because `gpu-transport.js` was otherwise a module no
      // page imported. A transport nothing drives is one whose first execution
      // is also its first real frame, in a browser, under a replayer — so this
      // runs it on every frame of every demo instead, where the browser gate
      // sees it. The replayer goes where this result is dropped, when the WebGPU
      // backend arrives to fill the buffer.
      takeCommandStream(gpuStream);
      flush();

      if (status !== announced) {
        announced = status;
        if (status === STATUS.RUNNING) {
          say('Playing.', `${hint} · click the canvas for keyboard and sound`);
          statusBar.classList.add('running');
        } else if (status === STATUS.PAUSED) {
          // A canvas that lost focus reports this too, which is the point: the
          // line used to read "Playing." for as long as the demo was alive,
          // including while it sat unfocused behind another window.
          say(
            'Paused.',
            'ESC resumes · click the canvas to give it the keyboard'
          );
          statusBar.classList.add('running');
        } else if (status === STATUS.STOPPED) {
          say(
            'Stopped.',
            opfsSettled(exports) ? `${savedLabel} saved.` : 'Saving…'
          );
          settle();
        } else if (status === STATUS.FAILED) {
          say('The demo stopped.', lastError(api, memory), true);
          settle();
        }
      }

      // PAUSED keeps the loop going: a paused demo still draws its menu, and
      // it is one keystroke from playing.
      if (
        status === STATUS.BOOTING ||
        status === STATUS.RUNNING ||
        status === STATUS.PAUSED
      ) {
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
        status: () => api.status(),
        audio: () => audio.then((a) => a?.stats()),
        saves: () => ({
          pending: exports.__crcbl_web_opfs_pending(),
          inFlight: exports.__crcbl_web_opfs_inflight(),
        }),
        assets: () => ({
          pending: exports.__crcbl_web_fetch_pending(),
          inFlight: exports.__crcbl_web_fetch_inflight(),
        }),
        logLevel: (/** @type {number} */ level) => api.logLevel(level),
      },
    });
  }

  main().catch((error) => {
    console.error(error);
    say('The demo failed to load.', String(error), true);
    settle();
  });
}
