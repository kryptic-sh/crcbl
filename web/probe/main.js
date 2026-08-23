// The page half of the `crcbl-webgpu` seam gate: a GPU command channel with a
// pump behind it and no engine anywhere near it.
//
// WHAT THE PROBE ACTUALLY NEEDS, AND IT IS ONLY THIS. `crcbl-webgpu`'s probe
// exports install their own `StreamChannel` and encode commands into it, and a
// page has exactly one channel — so the probe works wherever *nothing else has
// installed one*. That used to be arranged by driving a demo built on `wgpu`,
// where `crcbl-webgpu` is linked and idle; a page with no engine running at all
// satisfies the same condition, needs no second backend — and needs no wgpu,
// which a browser build cannot link any more — and is what this is.
//
// WHY IT LOADS A DEMO'S MODULE AND NEVER BOOTS IT. The probe exports live in
// `crcbl-webgpu`, so any artifact that links that crate carries all of them, and
// on `wasm32` the umbrella names no other GPU backend — so every artifact
// `web/build.sh` produces does. `check-exports.mjs` proves the symbols are there
// on every build. Loading the module is inert: `web/demos/<name>/main.js` is
// what calls `bootDemo`, and nothing in the loader's `init()` reaches the
// engine, so importing it instantiates the module and stops. Nothing calls
// `prepare`, `boot` or `frame`; no shell is attached, no `WebGpuDevice` is
// opened, and the channel stays free for the probe to claim. Gating the
// *shipped* artifact is the point of taking it from `demos/` rather than
// building a wasm target for this page.
//
// THE PAGE OWNS THE PUMP, and that is the one thing here that is not a demo's
// job done somewhere else. `web/engine/demo.js`'s `pumpGpu` runs inside the
// engine's frame loop; there is no frame loop here, so this file runs the same
// drain→replay→deliver in its own `requestAnimationFrame` loop, over the same
// `gpu-transport.js` and the same `Replayer` the demos and
// `web/harness/main.js` use. Every probe command still crosses the real
// transport and is replayed against the real `navigator.gpu`.
//
// THE VERDICT IS NOT IN THIS PAGE. `web/tools/probe-e2e.mjs` evaluates the
// probe's own JS helpers (`web/engine/gpu-probe.js`) against `globalThis.crcbl`
// below and makes every assertion outside the browser. So this file's whole job
// is to publish that object, keep the pump running, and — whatever happens — say
// so through `window.probeReady` / `window.probeFatal`, because a page that
// simply never finishes booting is a timeout with no reason attached.

import { Replayer } from '../engine/gpu-replay.js';
import {
  putReplyStream,
  replyChannelInstalled,
  takeCommandStream,
} from '../engine/gpu-transport.js';
import init from '../demos/breakout/crcbl_breakout.js';

/**
 * The key the page's own canvas is registered under.
 *
 * `SurfaceTarget::Web` is an integer into this registry and nothing else, so the
 * number is the page's to choose; it matches `web/engine/demo.js`'s `CANVAS_ID`
 * so that a driver reading `crcbl.gpu.canvasId` sees the same shape either way.
 * The probe's own canvases — the present gate's and the reconfigure gate's — are
 * added by the driver under keys of its choosing, into this same Map.
 */
const CANVAS_ID = 1;

const log = document.getElementById('log');

/** @param {string} text */
function say(text) {
  if (log) log.textContent = text;
}

async function main() {
  const canvas = /** @type {HTMLCanvasElement} */ (
    document.getElementById('canvas')
  );

  // Asked here rather than left to the first replayed command, so a machine with
  // no adapter says that instead of failing every group with a replay fault.
  if (!('gpu' in navigator)) {
    throw new Error('this browser has no navigator.gpu');
  }
  const adapter = await navigator.gpu.requestAdapter();
  if (!adapter) {
    throw new Error(
      'navigator.gpu.requestAdapter() returned no adapter — no GPU to drive'
    );
  }

  say('Loading the module…');
  const exports = await init();
  const memory = /** @type {WebAssembly.Memory} */ (exports.memory);

  // THE COMMAND STREAM'S ARGUMENT, BUILT ONCE, for `pumpGpu`'s reason in
  // `web/engine/demo.js`: the drain runs every frame and answers "nothing" on
  // most of them, so it must cost one integer call and no allocation.
  const gpuStream = { exports, memory };

  // One replayer for the life of the page, not one per frame: an enumeration
  // replayed on one frame is answered on a later one, so a per-frame replayer
  // would drop every answer it started.
  const canvases = new Map([[CANVAS_ID, canvas]]);
  const replayer = new Replayer({ canvases });

  /** What a replay threw, or `null` while none has. */
  let gpuFailure = null;
  /** How many frames carrying at least one command have been replayed. */
  let gpuReplayed = 0;
  /** How many reply buffers wasm has taken. */
  let gpuDelivered = 0;
  /**
   * The last frame that carried anything, as it was decoded.
   *
   * @type {{ baseSequence: bigint, commands: object[] } | null}
   */
  let gpuLastFrame = null;

  /**
   * The page's whole half of the GPU channel: drain, replay, deliver.
   *
   * **THE ONLY PLACE EITHER BUFFER IS TOUCHED**, exactly as `pumpGpu` is in
   * `web/engine/demo.js`. There is one channel and one buffer behind it, so a
   * second drain anywhere in the page would take commands this one never sees,
   * and a second delivery would offer bytes this replayer has already cleared.
   * `gpu-probe.js` encodes and reads state; this is what moves the bytes for it.
   */
  function pump() {
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
          // now", and the same bytes are offered again next frame; dropping them
          // is the one unrecoverable bug this channel can have.
          replayer.clear();
          gpuDelivered += 1;
        } else if (!replyChannelInstalled(gpuStream)) {
          // THE ONE CASE WHERE HOLDING THEM IS THE BUG INSTEAD. A `false` with
          // no channel behind it is "never", not "not now": the run these
          // answer is over, nothing waits on them, and the next channel this
          // page installs starts its sequence numbers at 0 again — so offering
          // them there answers commands it never sent, and `ReplyInbox::drain`
          // refuses that whole buffer, taking its real answers with it.
          replayer.clear();
        }
      }
    } catch (error) {
      // LATCHED OFF, AND SAID ONCE. A throw out of here is an opcode this page
      // cannot execute or a canvas id it cannot resolve — the page being wrong
      // rather than a condition that passes — so the next frame would throw the
      // same way, and sixty identical lines a second is how a message is lost
      // rather than read. Latching also stops the drain, which is deliberate: a
      // channel nobody is answering must not look like one that is. The driver
      // reads it back as `stats().failure` and lands it on the group that was
      // waiting.
      gpuFailure = String(error);
      console.error(`crcbl: the GPU command stream stopped: ${gpuFailure}`);
      say(`The GPU command stream failed: ${gpuFailure}`);
    }
  }

  // The loop runs for the life of the page. There is no status to stop on: the
  // driver decides when the run is over by closing the tab.
  function frame() {
    if (gpuFailure === null) pump();
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);

  // THE OBJECT THE DRIVER TALKS TO, and deliberately the same shape
  // `web/engine/demo.js` publishes — `exports`, `memory`, and a `gpu` holding
  // the registry, the replayer and the pump's counters. The probe groups read
  // identity off `canvases` and `replayer`, which does not survive being
  // summarised into numbers, and read progress off `stats()`.
  globalThis.crcbl = {
    exports,
    memory,
    gpu: {
      canvasId: CANVAS_ID,
      canvases,
      replayer,
      // `baseSequence` as a string: it is a `BigInt`, and neither `JSON` nor
      // the DevTools protocol carries one.
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
  };

  say(`Pumping. Adapter: ${adapter.info?.description || 'unnamed'}.`);
}

window.probeReady = false;
window.probeFatal = null;
main()
  .then(() => {
    window.probeReady = true;
  })
  .catch((error) => {
    window.probeFatal = String(error && error.stack ? error.stack : error);
    console.error(error);
    say(`The probe page could not start: ${window.probeFatal}`);
  });
