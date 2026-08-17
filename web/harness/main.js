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
   *                   timedOut: boolean, replayFailure: string | null }>,
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
    }

    for (let i = 0; i < count; i += 1) {
      const name = readUtf8(
        memory,
        exports.__crcbl_render_harness_scene_name_ptr(i),
        exports.__crcbl_render_harness_scene_name_len(i)
      );
      replayFailure = null;
      exports.__crcbl_render_harness_start(i);

      let frames = 0;
      let state = exports.__crcbl_render_harness_state();
      while (state === STATE.OPENING || state === STATE.IDLE) {
        await nextFrame();
        state = exports.__crcbl_render_harness_step();
        pump();
        frames += 1;
        if (frames >= MAX_FRAMES) break;
      }

      const error = readUtf8(
        memory,
        exports.__crcbl_render_harness_error_ptr(),
        exports.__crcbl_render_harness_error_len()
      );
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
      });
      say(
        `${i + 1}/${count} scenes driven; last: ${name} → ${
          STATE_NAME[state] ?? state
        }`
      );
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
