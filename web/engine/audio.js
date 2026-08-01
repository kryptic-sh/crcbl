// The main-thread half of `crcbl-audio`'s browser output.
//
// Implements the JS side of `crates/crcbl-audio/src/web.rs`, in the shape B the
// worklet file explains: the wasm instance renders here, on the main thread,
// and blocks cross to the audio thread as transferred `ArrayBuffer`s.
//
// AUTOPLAY. An `AudioContext` created before a user gesture starts `suspended`,
// and nothing plays. `resumeOnGesture` hangs one-shot listeners on the document
// so the first click or key press starts it — which is also the first thing a
// player does to a game.

/** Frames the renderer will size its buffers for. Well under `MAX_BLOCK_FRAMES`. */
const MAX_BLOCK_FRAMES = 4096;

/**
 * Starts an `AudioContext`, loads the worklet and answers its refill requests.
 *
 * Never throws: a page with no `AudioContext`, a blocked worklet or a browser
 * that refuses to start one is a game with no sound, not a game that fails to
 * load. Returns `null` in that case.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @param {string} options.workletUrl
 * @returns {Promise<{ context: AudioContext, resume: () => void, stats: () => object } | null>}
 */
export async function startAudio({ exports, memory, workletUrl }) {
  const AudioContextClass = window.AudioContext ?? window.webkitAudioContext;
  if (!AudioContextClass) {
    console.warn('crcbl: no AudioContext; the demo will be silent');
    return null;
  }

  let context;
  let node;
  try {
    context = new AudioContextClass();
    await context.audioWorklet.addModule(workletUrl);
    node = new AudioWorkletNode(context, 'crcbl-audio', {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [2],
    });
  } catch (error) {
    console.warn('crcbl: could not start the audio worklet; the demo will be silent', error);
    try {
      await context?.close();
    } catch {
      // Nothing further to do about a context that will not close.
    }
    return null;
  }

  let workletUnderruns = 0;

  node.port.onmessage = (event) => {
    const want = event.data?.want ?? 0;
    workletUnderruns = event.data?.underruns ?? workletUnderruns;
    if (want <= 0) {
      node.port.postMessage({});
      return;
    }

    // Idempotent when the arguments have not changed, and it keeps the
    // resampler's phase across the repeat — so calling it here rather than once
    // at start-up is what lets the source be installed *after* the worklet is,
    // which is the order boot actually happens in: the game (and therefore its
    // `AudioSource`) does not exist until the device promise resolves.
    if (exports.__crcbl_web_audio_configure(context.sampleRate, MAX_BLOCK_FRAMES) === 0) {
      node.port.postMessage({});
      return;
    }

    const frames = Math.min(want, MAX_BLOCK_FRAMES);
    const rendered = exports.__crcbl_web_audio_render(frames);
    if (rendered <= 0) {
      node.port.postMessage({});
      return;
    }

    // Views are built after `render`, never cached across a call: wasm memory
    // can grow and detach them. See `web/engine/wasm.js`.
    const channelCount = exports.__crcbl_web_audio_channels();
    const channels = [];
    for (let channel = 0; channel < channelCount; channel += 1) {
      const byteOffset = exports.__crcbl_web_audio_channel(channel);
      if (byteOffset === 0) break;
      const view = new Float32Array(memory.buffer, byteOffset, rendered);
      // A copy: the transfer below gives the buffer away, and wasm memory must
      // not be the thing given away.
      channels.push(view.slice());
    }
    if (channels.length === 0) {
      node.port.postMessage({});
      return;
    }
    node.port.postMessage(
      { channels, frames: rendered },
      channels.map((c) => c.buffer),
    );
  };

  node.connect(context.destination);

  /** Starts a context the browser suspended pending a user gesture. */
  function resume() {
    if (context.state === 'suspended') void context.resume();
  }
  for (const type of ['pointerdown', 'keydown']) {
    document.addEventListener(type, resume, { once: false, passive: true });
  }

  return {
    context,
    resume,
    stats() {
      return {
        state: context.state,
        sampleRate: context.sampleRate,
        renderRate: exports.__crcbl_web_audio_render_rate(),
        // Two different underrun counts, deliberately: wasm's is "the source
        // could not fill a block", the worklet's is "the queue ran dry". They
        // have different causes and only one of them is the engine's fault.
        engineUnderruns: exports.__crcbl_web_audio_underruns(),
        workletUnderruns,
      };
    },
  };
}
