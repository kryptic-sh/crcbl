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
import { Replayer } from './gpu-replay.js';
import {
  putReplyStream,
  replyChannelInstalled,
  takeCommandStream,
} from './gpu-transport.js';
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
 * How many bytes of host entropy `__crcbl_web_seed_entropy` consumes.
 *
 * The size of the buffer `__crcbl_web_seed_entropy_ptr` hands back, which is
 * fixed by `crcbl_rand`'s `SEED_INPUT`; writing fewer leaves the tail of a
 * previous seed in place and writing more overruns it.
 */
const SEED_BYTES = 32;

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
 * @property {() => Promise<Record<string, any>>} init the loader module's
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

    // Seed the wasm entropy source before booting: `boot()` constructs the
    // engine's `Server`, whose first act is drawing a resume token, and on
    // wasm32 that draw fails closed until the host seeds the CSPRNG. So this
    // must land before `boot()`, or `Server::new` errors and the boot gate is
    // red. Take the buffer pointer and write a whole `SEED_BYTES` of fresh
    // secure bytes into it in one shot — nothing between the two can
    // `memory.grow()` and detach the view — then commit, which seeds the stream
    // and zeroes the buffer.
    //
    // Drawn into a buffer of this page's own and then copied in, rather than
    // drawn straight into wasm memory: `crypto.getRandomValues` refuses a view
    // onto a `SharedArrayBuffer`, which is what a threaded artifact's memory is,
    // and `set` does not. The write is still the one shot the paragraph above
    // asks for — nothing between the pointer and the `set` can grow memory.
    const seedPtr = exports.__crcbl_web_seed_entropy_ptr();
    const seed = new Uint8Array(SEED_BYTES);
    crypto.getRandomValues(seed);
    new Uint8Array(memory.buffer, seedPtr, seed.length).set(seed);
    exports.__crcbl_web_seed_entropy();

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

    // THE PAGE'S CANVAS REGISTRY, AND THE ONE REPLAYER THAT READS IT.
    // `SurfaceTarget::Web` is an integer key into this map and nothing else —
    // no string crosses the wasm boundary — so this is where the demo's canvas
    // gets the number a `CreateSurface` may name it by, and `CANVAS_ID` is
    // already that number for the shell.
    //
    // One replayer for the life of the page, not one per frame: an enumeration
    // replayed on one frame is answered on a later one, so a per-frame replayer
    // would drop every answer it started.
    const canvases = new Map([[CANVAS_ID, canvas]]);
    const replayer = new Replayer({ canvases });

    /** What a replay threw, or `null` while none has. See {@link pumpGpu}. */
    let gpuFailure = null;
    /** How many frames carrying at least one command have been replayed. */
    let gpuReplayed = 0;
    /** How many reply buffers wasm has taken. */
    let gpuDelivered = 0;
    /**
     * The last frame that carried anything, as it was decoded.
     *
     * Kept whole rather than summarised so that recording it costs an
     * assignment and no allocation: the decode has already built these objects,
     * and `gpu-transport.js` copies every payload out of wasm memory before it
     * returns, so none of them is a view that a later `memory.grow()` detaches.
     *
     * @type {{ baseSequence: bigint, commands: object[] } | null}
     */
    let gpuLastFrame = null;

    /**
     * The page's whole half of the GPU channel: drain, replay, deliver.
     *
     * **THE ONE PLACE EITHER BUFFER IS TOUCHED.** There is one channel and one
     * buffer behind it, so a second drain anywhere in the page would take
     * commands this one never sees, and a second delivery would offer bytes
     * this replayer has already cleared. `gpu-probe.js` used to do both and now
     * does neither — it encodes and it reads state, and this is what moves the
     * bytes for it and for whatever installs a channel next.
     *
     * The order is the seam's: the frame wasm recorded comes out, is executed,
     * and then whatever answers are ready go back in. The replies crossing here
     * belong to commands an *earlier* frame started, which is why the replayer
     * outlives a frame at all.
     */
    function pumpGpu() {
      try {
        const carried = takeCommandStream(gpuStream);
        replayer.replay(carried);
        if (carried !== null && carried.commands.length > 0) {
          gpuReplayed += 1;
          gpuLastFrame = carried;
        }
        if (replayer.hasReplies) {
          if (putReplyStream({ exports, memory, bytes: replayer.replies })) {
            // Cleared only once wasm has actually taken them. A `false` is "not
            // now", and the same bytes are offered again next frame; dropping
            // them is the one unrecoverable bug this channel can have.
            replayer.clear();
            gpuDelivered += 1;
          } else if (!replyChannelInstalled(gpuStream)) {
            // THE ONE CASE WHERE HOLDING THEM IS THE BUG INSTEAD. A `false`
            // with no channel behind it is "never", not "not now": the run
            // these answer is over, nothing waits on them, and the next channel
            // this page installs starts its sequence numbers at 0 again — so
            // offering them there answers commands it never sent, and
            // `ReplyInbox::drain` refuses that whole buffer, taking its real
            // answers with it.
            replayer.clear();
          }
        }
      } catch (error) {
        // LATCHED OFF, AND SAID ONCE, LOUDLY. A throw out of here is an opcode
        // this page cannot execute or a canvas id it cannot resolve — the page
        // being wrong rather than a condition that passes — so the next frame
        // would throw the same way and sixty identical lines a second is how a
        // message gets lost rather than read. Latching also stops the drain,
        // which is deliberate: a channel nobody is answering must not look like
        // one that is.
        //
        // The rAF loop itself keeps going. It is the browser's loop and the
        // engine draws through it, so killing it over a channel fault would
        // take the demo's picture down as well — and the picture is what says
        // which half broke. The console line is for the log the browser gate
        // reads; the status line is for whoever is looking at the page instead.
        gpuFailure = String(error);
        console.error(`crcbl: the GPU command stream stopped: ${gpuFailure}`);
        say('The GPU command stream failed.', gpuFailure, true);
      }
    }

    let announced = -1;

    /** @param {number} now */
    function frame(now) {
      // The shell's event-clock reference for this frame, before anything reads
      // an event timestamp.
      exports.__crcbl_web_frame(now);
      const status = api.frame(now);
      log();
      drainFetch({ exports, memory });
      // THE GPU CHANNEL, DRIVEN FROM THE DEMO'S OWN LOOP AND NOWHERE ELSE.
      // Almost every frame of almost every demo finds nothing: no HAL
      // implementation writes through `StreamChannel::encode` yet, so unless
      // something has installed a channel — today only `crcbl-webgpu`'s probe,
      // which the browser gate drives — `len` answers 0 and this costs one
      // integer call, the documented "a shim that started before the engine
      // did" case rather than a failure.
      //
      // It runs anyway, and it runs *here*, because a loop that only ever drops
      // what it decodes proves nothing about replaying it: the transport, the
      // replayer and the reply channel would all be meeting their first real
      // frame in a browser, under a page nobody had driven. This is the loop the
      // WebGPU backend will use, doing the work that backend will need.
      if (gpuFailure === null) pumpGpu();
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
        // The GPU channel, and the two live objects behind it: the registry a
        // `CreateSurface` resolves against and the replayer that resolves it.
        // They are here rather than summarised because what a console — or the
        // browser gate — has to ask about them is identity, which does not
        // survive being turned into a number: whether a surface holds the
        // context of *this* canvas is a comparison of elements.
        gpu: {
          canvasId: CANVAS_ID,
          canvases,
          replayer,
          // `baseSequence` as a string: it is a `BigInt`, and neither `JSON`
          // nor the DevTools protocol carries one.
          stats: () => ({
            replayed: gpuReplayed,
            delivered: gpuDelivered,
            commands: (gpuLastFrame?.commands ?? []).map((command) =>
              String(command.name)
            ),
            baseSequence:
              gpuLastFrame === null ? null : String(gpuLastFrame.baseSequence),
            surfaces: replayer.surfaces.size,
            failure: gpuFailure,
          }),
        },
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
