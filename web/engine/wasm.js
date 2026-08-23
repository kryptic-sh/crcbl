// Reading and writing wasm memory, and the one rule that governs all of it.
//
// THE DETACHED-VIEW TRAP. A `Uint8Array` (or `Float32Array`) built over
// `memory.buffer` is *detached* the moment wasm memory grows — every access
// through it throws, or worse, silently reads zeroes. Several engine exports
// allocate and can therefore grow memory: `__crcbl_web_fetch_buffer`,
// `__crcbl_web_opfs_restore_buffer`, `__crcbl_web_audio_configure`. Every
// module docs page in the engine says the same thing about it.
//
// The rule this file enforces, by never exporting a view: a view is built
// immediately before it is used, from the pointer that was just returned, and
// is never stored. Rebuilding costs a few nanoseconds against a network round
// trip or 2.7 ms of audio, so there is nothing to weigh it against.

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/**
 * Copies `text` as UTF-8 into `capacity` bytes at `ptr`.
 *
 * Returns the number of bytes written, or `null` when the string does not fit
 * — the caller must then drop whatever it was about to do rather than
 * truncate, because half a UTF-8 string is a different string and half a
 * `KeyboardEvent.code` is a different key.
 *
 * @param {WebAssembly.Memory} memory
 * @param {number} ptr
 * @param {number} capacity
 * @param {string} text
 * @returns {number | null}
 */
export function writeUtf8(memory, ptr, capacity, text) {
  if (ptr === 0) return null;
  const bytes = encoder.encode(text);
  if (bytes.length > capacity) return null;
  new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
  return bytes.length;
}

/**
 * Copies `bytes` into `len` bytes at `ptr`.
 *
 * @param {WebAssembly.Memory} memory
 * @param {number} ptr
 * @param {Uint8Array} bytes
 */
export function writeBytes(memory, ptr, bytes) {
  new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
}

/**
 * Decodes `len` bytes at `ptr` as UTF-8. Empty for a null pointer.
 *
 * **The copy is not optional.** `TextDecoder.decode` refuses a view onto a
 * `SharedArrayBuffer` — `The provided ArrayBufferView value must not be shared`
 * — and a threaded artifact's memory is nothing else, so decoding the view
 * directly turns every log line on such a build into a `TypeError`. Measured in
 * Chromium against `web/build.sh --threads`'s horde: the demo died on its first
 * `drainLog`. A `slice` of a few dozen bytes against a decode is not a cost
 * worth branching on, so it is unconditional and the non-threaded path pays it
 * too.
 *
 * @param {WebAssembly.Memory} memory
 * @param {number} ptr
 * @param {number} len
 * @returns {string}
 */
export function readUtf8(memory, ptr, len) {
  if (ptr === 0 || len === 0) return '';
  return decoder.decode(new Uint8Array(memory.buffer, ptr, len).slice());
}

/**
 * Copies `len` bytes at `ptr` out of wasm memory.
 *
 * A copy, not a view: the engine's record buffers are freed by the `ack` that
 * follows, and `createWritable().write()` resolves after it.
 *
 * @param {WebAssembly.Memory} memory
 * @param {number} ptr
 * @param {number} len
 * @returns {Uint8Array}
 */
export function readBytes(memory, ptr, len) {
  if (ptr === 0 || len === 0) return new Uint8Array(0);
  return new Uint8Array(memory.buffer, ptr, len).slice();
}
