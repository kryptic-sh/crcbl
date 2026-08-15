// The wasm side of the command stream: a frame out of wasm memory, decoded.
//
// `crates/crcbl-webgpu/src/web.rs` is the contract — the three exports, who
// owns the buffer, and when it is valid — and this file is the shim its module
// docs describe. It is deliberately separate from `gpu-stream.js`, which is the
// *format* and knows nothing about wasm: that decoder takes a `Uint8Array` and
// is driven under node with no wasm instance anywhere, and folding an ABI into
// it would give it a dependency it does not have. Format on one side of the
// seam, transport on the other, exactly as `wasm.js` and `storage.js` divide.
//
// ONE DIRECTION. Wasm → JS and nothing else. There is no reply channel yet: no
// export here takes a value, and nothing in a frame waits for an answer. The
// JS → wasm half arrives with the first call that needs one.
//
// THE DETACHED-VIEW RULE, AND WHERE IT BITES HERE. A `Uint8Array` over
// `memory.buffer` is detached the moment wasm memory grows, and every access
// through it then throws. The one view in this file is built from the pointer
// the call above it just returned and is dropped before the function returns —
// the rule `wasm.js` enforces by never exporting a view at all.
//
// What could detach it is narrower than usual, and worth stating exactly:
// neither `__crcbl_web_gpu_stream_ptr` nor `__crcbl_web_gpu_stream_len` nor
// `__crcbl_web_gpu_stream_release` allocates, so none of the three can grow
// memory. What can is *encoding*, which happens inside the engine's own
// per-frame export. So a view built here is safe for the whole decode, and
// would be stale the moment the next frame is recorded. Nothing this function
// returns is a view: `gpu-stream.js` copies a push-constant block out and
// decodes a label to a string, so the commands outlive the frame they came
// from and a later `memory.grow()` cannot reach into them.

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
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {{ baseSequence: bigint, commands: object[] } | null}
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
    return { baseSequence: reader.baseSequence, commands };
  } finally {
    exports.__crcbl_web_gpu_stream_release();
  }
}
