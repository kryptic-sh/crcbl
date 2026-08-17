// The `AudioWorkletProcessor` half of `crcbl-audio`'s browser output.
//
// Loaded with `ctx.audioWorklet.addModule()`, so this file runs inside
// `AudioWorkletGlobalScope` — no DOM, no `fetch`, no imports it can rely on.
// It is deliberately self-contained.
//
// WHY THIS IS SHAPE B. `crates/crcbl-audio/src/web.rs` describes two shapes.
// Shape A instantiates the wasm module *inside* this scope and calls
// `__crcbl_web_audio_render` from `process()`; it has the lower latency and is
// the module's primary. This shim uses shape B — render on the main thread,
// ship blocks across with `postMessage` — for two independent reasons, either
// of which alone would decide it:
//
//   1. The module cannot be instantiated here. `wgpu` reaches WebGPU through
//      `web-sys`, so the artifact carries 300-odd `wasm-bindgen` imports whose
//      glue touches `document`, `window` and `fetch`. `AudioWorkletGlobalScope`
//      has none of them. The audio module's own docs name exactly this case as
//      shape B's reason for existing.
//   2. Even if it could, it would be a *second* wasm instance with its own
//      linear memory, and the voices breakout queues live in the first one.
//      There is no `play(id)` in the audio ABI — what is playing is the
//      application's business — so a second instance would render silence.
//
// There is no `SharedArrayBuffer` in either shape: GitHub Pages cannot set the
// COOP/COEP headers it needs, which `docs/plan/10-wasm-webgpu.md`'s 2026-07-27
// correction settles. Blocks cross as transferred `ArrayBuffer`s instead.
//
// THE UNDERRUN RULE, from the same module docs: when there is not enough audio,
// zero-fill the remainder of the quantum. Never repeat the previous block (a
// buzz) and never return `false` from `process()` (that tears the node down for
// a transient).

/** Frames of lead the queue tries to hold: ~43 ms at 48 kHz. */
const TARGET_FRAMES = 2048;

/** Ask for more once the queue drops below this: ~21 ms at 48 kHz. */
const LOW_WATER_FRAMES = 1024;

class CrcblAudioProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    /** @type {{ channels: Float32Array[], offset: number, frames: number }[]} */
    this.queue = [];
    /** Frames sitting in `queue`, summed. */
    this.buffered = 0;
    /** A refill has been asked for and not yet answered. */
    this.awaitingRefill = false;
    /** Quanta that had to be zero-filled, for the page's debug readout. */
    this.underruns = 0;

    this.port.onmessage = (event) => {
      const { channels, frames } = event.data ?? {};
      this.awaitingRefill = false;
      if (!channels || !frames) return;
      this.queue.push({ channels, offset: 0, frames });
      this.buffered += frames;
    };
  }

  /**
   * @param {Float32Array[][]} _inputs
   * @param {Float32Array[][]} outputs
   * @returns {boolean}
   */
  process(_inputs, outputs) {
    const out = outputs[0];
    if (!out || out.length === 0) return true;
    const wanted = out[0].length;
    let filled = 0;

    while (filled < wanted && this.queue.length > 0) {
      const block = this.queue[0];
      const take = Math.min(wanted - filled, block.frames - block.offset);
      for (let channel = 0; channel < out.length; channel += 1) {
        // Mono output from a stereo block, or the other way round, both read
        // channel 0 rather than going silent.
        const source =
          block.channels[Math.min(channel, block.channels.length - 1)];
        out[channel].set(
          source.subarray(block.offset, block.offset + take),
          filled
        );
      }
      block.offset += take;
      filled += take;
      this.buffered -= take;
      if (block.offset >= block.frames) this.queue.shift();
    }

    if (filled < wanted) {
      this.underruns += 1;
      for (const channel of out) channel.fill(0, filled);
    }

    if (!this.awaitingRefill && this.buffered < LOW_WATER_FRAMES) {
      this.awaitingRefill = true;
      this.port.postMessage({
        want: TARGET_FRAMES - this.buffered,
        underruns: this.underruns,
      });
    }
    return true;
  }
}

registerProcessor('crcbl-audio', CrcblAudioProcessor);
