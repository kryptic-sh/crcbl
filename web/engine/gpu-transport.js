// The wasm side of both channels: a frame of commands out of wasm memory, and
// a buffer of replies back into it.
//
// `crates/crcbl-webgpu/src/web.rs` is the contract — the seven exports, who
// owns each buffer, and when it is valid — and this file is the shim its module
// docs describe. It is deliberately separate from `gpu-stream.js` and
// `gpu-reply.js`, which are the *formats* and know nothing about wasm: those
// take and produce a `Uint8Array` and are driven under node with no wasm
// instance anywhere, and folding an ABI into them would give them a dependency
// they do not have. Format on one side of the seam, transport on the other,
// exactly as `wasm.js` and `storage.js` divide.
//
// TWO DIRECTIONS. Commands come out once a frame; replies go in whenever there
// are any. They are separate buffers with separate lifetimes and nothing in a
// frame blocks on the other direction — a reply arrives whenever the browser
// has the answer, named by the sequence of the command it answers.
//
// THE DETACHED-VIEW RULE, AND WHERE IT BITES HERE. A `Uint8Array` over
// `memory.buffer` is detached the moment wasm memory grows, and every access
// through it then throws. Each view in this file is built from the pointer the
// call above it just returned and is dropped before its function returns — the
// rule `wasm.js` enforces by never exporting a view at all.
//
// Which calls can actually detach one is narrower than usual, and worth stating
// exactly:
//
//   * `__crcbl_web_gpu_reply_buffer` CAN. It sizes a `Vec` and is the one export
//     in that module that allocates. Its view is therefore built strictly after
//     it returns, from what it returned.
//   * The stream exports cannot: two read a pointer and a length, one clears a
//     `Vec` and keeps its allocation. What moves that buffer is *encoding*,
//     which happens inside the engine's own per-frame export. So the view built
//     in `takeCommandStream` is safe for the whole decode, and would be stale
//     the moment the next frame is recorded.
//
// Nothing `takeCommandStream` returns is a view: `gpu-stream.js` copies a
// push-constant block out and decodes a label to a string, so the commands
// outlive the frame they came from and a later `memory.grow()` cannot reach
// into them.

import { StreamReader } from './gpu-stream.js';

/**
 * The frame wasm has recorded, decoded, with the buffer handed back.
 *
 * `null` means there is nothing to ask — no channel installed, so the engine
 * has not booted yet. That is distinct from a frame that recorded no commands,
 * which comes back as an empty `commands` array: wasm answers a header's worth
 * of bytes for one and zero for the other, and the two facts stay apart.
 *
 * `baseSequence` is the sequence number of `commands[0]`, straight out of the
 * buffer's header. The nth command's own number is `baseSequence + n` — nothing
 * per command is on the wire — and it is what connects a WebGPU validation
 * error raised by a replayer back to the Rust that encoded the command.
 *
 * The buffer is released before this returns, including when the decode throws:
 * a frame left unreleased is one the next frame appends to, so a shim that gave
 * up on a corrupt stream would meet the same bytes again forever.
 *
 * `streamEnded` says this frame is the last one — the run's teardown, drained.
 * It is read *after* the release and not before, because the release is the
 * event: `crates/crcbl-webgpu/src/web.rs` parks a strong handle on the channel
 * when the last encoder drops, and handing the buffer back is what lets that
 * handle go. Asking first would answer `false` on the one frame it matters for.
 *
 * **It cannot be inferred from a length of zero**, which is the reason the
 * export exists at all: `__crcbl_web_gpu_stream_len` answers `0` for a page that
 * started before the engine did *and* for a run that has finished, so a shim
 * watching only the length cannot tell a run that has not begun from one that is
 * over. `Replayer#replay` reads this flag as the cue to report what the browser
 * side is still holding, which is `crcbl-vk`'s device-teardown warning asked on
 * this side of the seam.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ baseSequence: bigint, commands: object[], streamEnded: boolean } | null}
 * @throws {import('./gpu-stream.js').StreamDecodeError} Whatever the buffer's
 *   contents produce. The two halves of this format are hand-written, so this
 *   is a bug in one of them rather than a condition to recover from.
 */
export function takeCommandStream({ exports, memory }) {
  // The readiness test is the length, not the pointer: an installed channel
  // always holds at least a header, so zero here means "no channel" and cannot
  // mean "an empty frame". The two answer together, which is why the pointer is
  // not tested again below.
  const len = exports.__crcbl_web_gpu_stream_len();
  if (len === 0) return null;
  const ptr = exports.__crcbl_web_gpu_stream_ptr();

  /** @type {{ baseSequence: bigint, commands: object[], streamEnded: boolean }} */
  let frame;
  try {
    // THE VIEW. Built after the pointer call, used before anything calls back
    // into wasm, and not stored.
    const reader = new StreamReader(new Uint8Array(memory.buffer, ptr, len));
    const commands = [];
    for (
      let next = reader.nextCommand();
      next !== null;
      next = reader.nextCommand()
    ) {
      commands.push(next.command);
    }
    frame = { baseSequence: reader.baseSequence, commands, streamEnded: false };
  } finally {
    exports.__crcbl_web_gpu_stream_release();
  }
  // After the release, for the reason in this function's docs. A decode that
  // threw never gets here, and that is deliberate: the frame's own destroys did
  // not all run, so what the tables hold would name the throw's casualties as
  // things the engine leaked.
  frame.streamEnded = exports.__crcbl_web_gpu_stream_ended() === 1;
  return frame;
}

/**
 * Whether there is a channel for replies to reach at all.
 *
 * The one thing a `false` from {@link putReplyStream} cannot say on its own,
 * and the difference between "not now" and "never". Three of that `false`'s
 * four causes are a channel that is there and busy, and the caller's answer to
 * those is to hold the bytes and offer them again. The fourth is that there is
 * no channel — and a channel that has gone is not one that comes back: the next
 * one is a *fresh* `StreamChannel` whose sequence counter starts at `0` again,
 * so bytes queued against the old one answer commands the new one never sent.
 *
 * **What that costs, when it happens.** `ReplyInbox::drain` in
 * `crates/crcbl-webgpu/src/web.rs` refuses a reply naming a sequence nothing
 * waits on — and refuses the *whole buffer* with it, so the stale answer takes
 * that frame's real answers down too. The answers do not come again: the
 * browser sent them once. That is a run wedged for good, and it is why a caller
 * holding replies must ask this before deciding a `false` was "not now".
 *
 * A page that has not booted yet answers `false` here as well, and that costs
 * nothing: a replayer that has replayed no commands has no replies to drop.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @returns {boolean}
 */
export function replyChannelInstalled({ exports }) {
  // The same readiness test `putReplyStream` makes, and named here so a caller
  // can make it without offering bytes first.
  return exports.__crcbl_web_gpu_reply_capacity() !== 0;
}

/**
 * Hands a buffer of encoded replies to wasm, or reports that it would not take
 * them.
 *
 * `bytes` is what a `ReplyWriter` from `gpu-reply.js` produced: a header and
 * every reply this side has an answer for, each naming the sequence of the
 * command it answers.
 *
 * `false` is not a failure and not an error — it is "not now", and the caller
 * **keeps the replies and offers the same buffer again next frame**. There are
 * four ways to get it, and none of them loses an answer:
 *
 * - no channel installed, so the engine has not booted (`capacity` is `0`);
 * - the buffer is larger than wasm will accept in one go;
 * - the engine has not drained the last buffer, so wasm will not overwrite it;
 * - wasm refused the commit, which means the bytes were not delivered.
 *
 * Discarding them on a `false` would be the one unrecoverable bug this channel
 * can have: a dropped reply is a command that waits for ever. **Except on the
 * first of those four**, where the opposite is true and holding them is the
 * bug — see {@link replyChannelInstalled}, which is how a caller tells that
 * case from the other three.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @param {Uint8Array} options.bytes A `ReplyWriter`'s `bytes`.
 * @returns {boolean} Whether wasm took them.
 */
export function putReplyStream({ exports, memory, bytes }) {
  // The readiness test is the capacity, not the pending count: an installed
  // channel always answers a non-zero cap, while `pending` is zero on almost
  // every frame. It doubles as the size check, which is why it is read rather
  // than hard-coded — the number is a Rust constant, and a shim that baked it in
  // would keep its old value across a wasm update it did not know about.
  const capacity = exports.__crcbl_web_gpu_reply_capacity();
  if (capacity === 0 || bytes.length > capacity) return false;

  // THE VIEW. Built after the call that may have grown memory, from the pointer
  // that call returned, used before anything else calls back into wasm, and not
  // stored.
  const ptr = exports.__crcbl_web_gpu_reply_buffer(bytes.length);
  if (ptr === 0) return false;
  new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
  return exports.__crcbl_web_gpu_reply_commit(bytes.length) === 1;
}
