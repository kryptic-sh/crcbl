// The browser half of the WebGPU parity gate.
//
// It drives `apps/render-harness`'s wasm ABI over the whole golden `Scene` set:
// for each scene it calls `start(i)`, then on every `requestAnimationFrame`
// frame it `step()`s the offscreen open one poll and pumps the GPU command
// stream — the same drain→replay→deliver loop the demos run in
// `web/engine/demo.js`, reusing the very transport and replayer that loop does.
// When a scene reaches a terminal state (`opened` or `failed`) it records the
// outcome and moves to the next.
//
// THE RESULT IS READ OUT OF THE PAGE, NOT SHOWN IN IT. `web/tools/
// render-harness-e2e.mjs` loads this page in headless Chromium, waits for
// `window.harnessDone`, and reads `window.harnessResult`. So this file's job is
// to fill that object and set the flag, whatever happens — a thrown exception is
// a result too, not a blank page the gate times out on.
//
// WHY IT REUSES THE ENGINE TRANSPORT. `crcbl-webgpu` renders by emitting a
// stream of GPU commands that JS replays on the real `GPUDevice`; opening the
// instance installs the channel these two functions move bytes across. The
// offscreen open needs adapter enumeration replayed and its replies delivered
// before it can even reach surface creation, so without this pump `step()`
// parks on the first poll forever.

import init from './crcbl_render_harness.js';
import { Replayer } from '../engine/gpu-replay.js';
import { putReplyStream, takeCommandStream } from '../engine/gpu-transport.js';

/** Mirrors the state codes in `apps/render-harness/src/lib.rs`. */
const STATE = { IDLE: 0, OPENING: 1, OPENED: 2, FAILED: 3 };
const STATE_NAME = { 0: 'idle', 1: 'opening', 2: 'opened', 3: 'failed' };

/**
 * A hard ceiling on frames spent driving one scene, so a backend that never
 * completes an open cannot hang the whole gate. Six hundred frames is ten
 * seconds at 60 Hz — orders of magnitude more than an open that is going to
 * finish needs, and a clean "timed out" verdict for one that is not.
 */
const MAX_FRAMES = 600;

/**
 * How many device errors one scene records before the rest are only counted.
 * A device that starts refusing usually refuses every frame, and sixty copies
 * of one line say nothing the first copy did not.
 */
const MAX_SCENE_DEVICE_ERRORS = 8;

/** Reads a UTF-8 string out of wasm memory; empty when the pointer is null. */
function readUtf8(memory, ptr, len) {
  if (!ptr || !len) return '';
  return new TextDecoder().decode(new Uint8Array(memory.buffer, ptr, len));
}

/** Resolves on the next animation frame. */
function nextFrame() {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}

const log = document.getElementById('log');
function say(text) {
  if (log) log.textContent = text;
}

async function main() {
  /**
   * @type {{
   *   started: boolean, webgpu: boolean, adapter: boolean,
   *   fatal: string | null,
   *   scenes: Array<{ scene: string, state: number, stateName: string,
   *                   rendered: boolean, error: string, frames: number,
   *                   timedOut: boolean, replayFailure: string | null,
   *                   fatal: string | null, deviceErrors: string[],
   *                   deviceErrorsDropped: number }>,
   * }}
   */
  const result = {
    started: false,
    webgpu: false,
    adapter: false,
    fatal: null,
    scenes: [],
  };
  window.harnessResult = result;
  window.harnessDone = false;

  try {
    if (!('gpu' in navigator)) {
      result.fatal = 'this browser has no navigator.gpu';
      return;
    }
    result.webgpu = true;

    let adapter = null;
    try {
      adapter = await navigator.gpu.requestAdapter();
    } catch (error) {
      result.fatal = `navigator.gpu.requestAdapter() threw: ${error}`;
      return;
    }
    if (!adapter) {
      result.fatal =
        'navigator.gpu.requestAdapter() returned no adapter — no GPU to drive';
      return;
    }
    result.adapter = true;

    const exports = await init();
    const memory = /** @type {WebAssembly.Memory} */ (exports.memory);
    result.started = true;

    const count = exports.__crcbl_render_harness_scene_count();
    // The registry a `CreateSurface` would resolve against — empty because the
    // offscreen open names no canvas. One replayer for the whole run: an
    // enumeration replayed on one frame is answered on a later one.
    const replayer = new Replayer({ canvases: new Map() });
    const gpuStream = { exports, memory };
    /** What a replay threw during the current scene, or null. */
    let replayFailure = null;
    /** What the device reported out of band during the current scene. */
    let deviceErrors = [];
    /** How many further device errors this scene produced past the cap. */
    let deviceErrorsDropped = 0;

    /** Drain the command stream, replay it, deliver whatever replies resulted. */
    function pump() {
      try {
        const carried = takeCommandStream(gpuStream);
        replayer.replay(carried);
        if (
          replayer.hasReplies &&
          putReplyStream({ exports, memory, bytes: replayer.replies })
        ) {
          replayer.clear();
        }
      } catch (error) {
        // A replay fault is the page being unable to execute an opcode the
        // backend emitted — a crack of its own. Latch it for this scene rather
        // than throwing sixty times a second.
        replayFailure = String(error && error.stack ? error.stack : error);
      }
      // WHAT `Device::take_error` WOULD HAND BACK, READ HERE BECAUSE NOTHING
      // ELSE READS IT. A command WebGPU refuses does not throw — the replayer
      // queues the reason, and the stream has no command yet to carry it into
      // wasm, so `WebGpuDevice::take_error` answers `None`. Left undrained, a
      // scene whose every draw was refused still reaches `opened` and this gate
      // calls it rendered. Draining it here is what makes that verdict mean
      // something.
      for (
        let error = replayer.takeError();
        error !== null;
        error = replayer.takeError()
      ) {
        if (deviceErrors.length < MAX_SCENE_DEVICE_ERRORS)
          deviceErrors.push(error);
        else deviceErrorsDropped += 1;
      }
    }

    // THE SCENE THAT ABORTS THE MODULE MUST NOT TAKE THE TABLE WITH IT. A Rust
    // panic compiles to `unreachable` on wasm, so it arrives here as a thrown
    // RuntimeError and — before this was caught per scene — unwound the whole
    // loop into the outer `catch`, leaving `scenes` empty and the gate printing
    // an empty table with no reason in it. A trap also poisons the instance:
    // every later export call traps at once, so the scenes after it genuinely
    // cannot be driven and are recorded as such rather than run and misreported
    // as eight separate cracks.
    /** The scene whose abort trapped the module, or null. */
    let abortedAt = null;
    /** That scene's whole thrown text, stack included. */
    let abortedWith = '';

    for (let i = 0; i < count; i += 1) {
      const name = readUtf8(
        memory,
        exports.__crcbl_render_harness_scene_name_ptr(i),
        exports.__crcbl_render_harness_scene_name_len(i)
      );
      if (abortedAt !== null) {
        result.scenes.push({
          scene: name,
          state: STATE.IDLE,
          stateName: STATE_NAME[STATE.IDLE],
          rendered: false,
          error: '',
          frames: 0,
          timedOut: false,
          replayFailure: null,
          fatal: `not driven — ${abortedAt} aborted the wasm module`,
          deviceErrors: [],
          deviceErrorsDropped: 0,
        });
        continue;
      }

      replayFailure = null;
      deviceErrors = [];
      deviceErrorsDropped = 0;
      /** What the module threw while this scene was being driven, or null. */
      let fatal = null;
      let frames = 0;
      let state = STATE.IDLE;
      let error = '';
      try {
        exports.__crcbl_render_harness_start(i);
        state = exports.__crcbl_render_harness_state();
        while (state === STATE.OPENING || state === STATE.IDLE) {
          await nextFrame();
          state = exports.__crcbl_render_harness_step();
          pump();
          frames += 1;
          if (frames >= MAX_FRAMES) break;
        }
        error = readUtf8(
          memory,
          exports.__crcbl_render_harness_error_ptr(),
          exports.__crcbl_render_harness_error_len()
        );
      } catch (thrown) {
        fatal = String(thrown && thrown.stack ? thrown.stack : thrown);
        abortedAt = name;
        abortedWith = fatal;
      }

      const timedOut =
        frames >= MAX_FRAMES &&
        state !== STATE.OPENED &&
        state !== STATE.FAILED;
      result.scenes.push({
        scene: name,
        state,
        stateName: STATE_NAME[state] ?? String(state),
        rendered: state === STATE.OPENED,
        error,
        frames,
        timedOut,
        replayFailure,
        fatal,
        deviceErrors,
        deviceErrorsDropped,
      });
      say(
        `${i + 1}/${count} scenes driven; last: ${name} → ${
          fatal ? 'aborted' : (STATE_NAME[state] ?? state)
        }`
      );
    }
    // The whole thrown text, not its first line: an `unreachable` says nothing
    // on its own, and the frames under it — `__rust_start_panic`, then the
    // encoder method that reached the assert — are the entire diagnosis. The
    // table's detail column has room for one line, so this is where it goes.
    if (abortedAt !== null) {
      result.fatal = `${abortedAt} aborted the wasm module:\n${abortedWith}`;
    }
  } catch (error) {
    result.fatal = String(error && error.stack ? error.stack : error);
  } finally {
    window.harnessResult = result;
    window.harnessDone = true;
    say(
      result.fatal
        ? `done (fatal: ${result.fatal})`
        : `done: ${result.scenes.length} scene(s) driven`
    );
  }
}

void main();
