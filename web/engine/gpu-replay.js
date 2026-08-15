// The replayer: decoded commands in, WebGPU calls out, replies back.
//
// `gpu-stream.js` turns a frame of wasm's bytes into command objects and
// `gpu-reply.js` turns answers back into bytes; this is what sits between them
// and actually calls the browser. `gpu-transport.js` is what moves either
// buffer across the wasm seam, and this file knows nothing about that — it takes
// what `takeCommandStream` returned and produces what `putReplyStream` sends,
// so it can be driven under node against a stub `navigator.gpu` with no wasm
// instance anywhere. `web/tools/gpu-replay.mjs` does exactly that.
//
// WHAT IT REPLAYS SO FAR IS ONE COMMAND. `EnumerateAdapters` calls
// `navigator.gpu.requestAdapter()` and answers with the adapter's name or the
// reason there is none. Every other command in the stream is *unimplemented*,
// and says so: `replay` throws a `ReplayError` naming the command and the
// sequence that carried it, rather than skipping it. A skipped command is a
// draw that never happened and a frame that renders almost right, which is the
// hardest kind of bug to see; a throw names the opcode the day it first
// arrives. Later slices fill them in.
//
// ASYNCHRONY AGAINST A SYNCHRONOUS REPLAY, WHICH IS THE WHOLE PROBLEM HERE.
// WebGPU's adapter API is a promise and the stream is replayed once per frame,
// synchronously, at the `requestAnimationFrame` boundary — the frame cannot wait
// and must not be blocked. So `replay` returns as soon as it has *started* the
// work, and the answer is queued into this replayer's `ReplyWriter` whenever the
// promise settles. It goes to wasm on whichever frame comes next, named by the
// sequence of the command that asked, which is exactly what the sequence number
// is for: a reply's position says nothing, so three frames later still names a
// command nothing else has been.
//
// Two consequences worth stating rather than discovering:
//
//   * A frame that replays an enumeration produces NO reply on that frame. The
//     caller must be willing to ask again — `hasReplies` is false and that is
//     the ordinary case, not a failure.
//   * A reply is queued exactly once per command, on every path. A promise that
//     rejected, a browser with no `navigator.gpu` at all, an adapter that
//     resolved `null`: each answers, because a dropped reply is a command wasm
//     waits on for ever.
//
// SEQUENCE NUMBERS ARE POSITIONAL ON THE WAY IN. Nothing per command is on the
// wire: the nth command's number is the buffer's `baseSequence` plus n, and it
// is computed here rather than carried, which is why `replay` takes the base
// alongside the list rather than the list alone.

import { ReplyWriter } from './gpu-reply.js';

/**
 * The reason recorded when `requestAdapter()` grants nothing.
 *
 * A promise that resolves `null` carries no message of its own, and the reason
 * field must not be empty for the case that has the most to explain — a browser
 * with WebGPU and no GPU behind it is a real machine, and this string is what a
 * log line or a banner ends up showing.
 */
const NO_ADAPTER_REASON = 'navigator.gpu.requestAdapter() granted no adapter';

/**
 * How much of a refusal's reason is kept.
 *
 * The reason comes from another vendor's runtime, and the reply writer refuses a
 * field past the stream's cap by throwing — inside a promise callback, where the
 * throw would become an unhandled rejection and the reply would simply never be
 * queued, leaving wasm waiting for ever. Truncating is the cheap guard against
 * that; no real message comes anywhere near it.
 */
const MAX_REASON_CHARS = 512;

/**
 * A command the stream carries and this replayer cannot execute yet.
 *
 * `sequence` is what makes it actionable: it names the command in the same
 * numbering wasm's own error attribution uses, so the Rust that encoded it can
 * be found again.
 */
export class ReplayError extends Error {
  /**
   * @param {string} command The `name` the decoder gave the command.
   * @param {bigint} sequence The sequence that command was assigned.
   */
  constructor(command, sequence) {
    super(
      `the replayer has no implementation for ${command} (command ${sequence})`
    );
    this.name = 'ReplayError';
    this.kind = 'Unimplemented';
    this.command = command;
    this.sequence = sequence;
  }
}

/**
 * Everything the browser can say about an adapter, as one line.
 *
 * `GPUAdapter.info` is a `GPUAdapterInfo` with four strings, any of which a
 * browser may leave empty — Chrome fills `vendor`, `architecture` and `device`
 * on some platforms and only `description` on others, and Firefox differs again.
 * They are joined rather than picked between so the name says as much as the
 * browser was willing to, and an adapter that answered nothing at all comes back
 * as the empty string rather than as `undefined`: the reply's `name` field is
 * allowed to be empty, and that is a real answer.
 *
 * @param {GPUAdapter} adapter
 * @returns {string}
 */
function adapterName(adapter) {
  const info = adapter.info ?? {};
  return [info.vendor, info.architecture, info.device, info.description]
    .filter(Boolean)
    .join(' ');
}

/**
 * Replays decoded command streams against WebGPU and collects the answers.
 *
 * One replayer for the life of a page, not one per frame: the reply buffer
 * outlives the frame that started the work, which is the point.
 */
export class Replayer {
  /** @type {GPU | undefined} */
  #gpu;
  /** @type {ReplyWriter} */
  #replies = new ReplyWriter();
  /** Whether anything has been written into `#replies` since the last clear. */
  #queued = false;
  /** Enumerations started and not yet answered. */
  #inFlight = 0;

  /**
   * @param {object} [options]
   * @param {GPU} [options.gpu] The `navigator.gpu` to replay against. Injected
   *   rather than reached for so the replayer can be driven under node, where
   *   there is none — and so a test can hand it one that refuses.
   */
  constructor({ gpu = globalThis.navigator?.gpu } = {}) {
    this.#gpu = gpu;
  }

  /** Whether there is at least one reply waiting to go to wasm. */
  get hasReplies() {
    return this.#queued;
  }

  /** How many commands have been started and not yet answered. */
  get inFlight() {
    return this.#inFlight;
  }

  /**
   * The encoded replies, header included, as a view over the writer's buffer.
   *
   * Hand it to `putReplyStream` in `gpu-transport.js`; call {@link clear} only
   * once that returned `true`. A `false` is "not now", and the same bytes are
   * offered again next frame — discarding them there is the one unrecoverable
   * bug this channel can have.
   */
  get replies() {
    return this.#replies.bytes;
  }

  /** Drops the queued replies. Only after wasm has taken them. */
  clear() {
    this.#replies.clear();
    this.#queued = false;
  }

  /**
   * Executes one frame's worth of commands.
   *
   * Synchronous: it returns once every command has been executed or started,
   * never once they have finished. Nothing here awaits, and nothing blocks the
   * frame.
   *
   * @param {{ baseSequence: bigint, commands: object[] }} frame What
   *   `takeCommandStream` returned. `null` is accepted and does nothing, which
   *   is what that function answers when no channel is installed.
   * @throws {ReplayError} On the first command this replayer cannot execute.
   *   The commands before it have already run, which is the same position a
   *   backend is in when a call fails part-way through a frame.
   */
  replay(frame) {
    if (frame === null || frame === undefined) return;
    const { baseSequence, commands } = frame;
    for (let i = 0; i < commands.length; i += 1) {
      // Positional, and wrapped to 64 bits for the reason the decoders wrap:
      // the base came off the wire, so a buffer declaring the largest possible
      // number must not produce a sequence outside the range it is typed as.
      const sequence = BigInt.asUintN(64, baseSequence + BigInt(i));
      const command = commands[i];
      switch (command.name) {
        case 'EnumerateAdapters':
          this.#enumerateAdapters(sequence);
          break;
        default:
          throw new ReplayError(String(command.name), sequence);
      }
    }
  }

  /**
   * Asks the browser for an adapter and queues the answer for a later frame.
   *
   * The request is deferred to a microtask rather than issued inline, so that
   * `replay` cannot be handed a `navigator.gpu` whose `requestAdapter` does its
   * work synchronously and thereby make one frame's cost depend on the browser.
   * Every path below queues exactly one reply.
   *
   * @param {bigint} sequence
   */
  #enumerateAdapters(sequence) {
    this.#inFlight += 1;
    Promise.resolve()
      .then(() => {
        if (!this.#gpu) throw new Error('navigator.gpu is not available here');
        return this.#gpu.requestAdapter();
      })
      .then((adapter) => {
        if (adapter) this.#adapter(sequence, adapter);
        else this.#noAdapter(sequence, NO_ADAPTER_REASON);
      })
      .catch((error) => this.#noAdapter(sequence, String(error)))
      .finally(() => {
        this.#inFlight -= 1;
      });
  }

  /**
   * @param {bigint} sequence
   * @param {GPUAdapter} adapter
   */
  #adapter(sequence, adapter) {
    // Always id 0. It is the adapter's position in the enumeration, and WebGPU
    // has no enumeration API to have a second position in: `requestAdapter()`
    // grants one adapter or none.
    this.#replies.adapter(sequence, 0, adapterName(adapter));
    this.#queued = true;
  }

  /**
   * @param {bigint} sequence
   * @param {string} reason
   */
  #noAdapter(sequence, reason) {
    this.#replies.noAdapter(sequence, reason.slice(0, MAX_REASON_CHARS));
    this.#queued = true;
  }
}
