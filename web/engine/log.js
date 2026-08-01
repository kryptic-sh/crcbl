// Draining the engine's log queue into the browser console.
//
// The wasm module imports nothing — not even `console.log`. `log::info!` inside
// the engine appends to a bounded queue in wasm memory and the page pops lines
// off it, which is the same exports-plus-polling shape every other ABI in the
// build uses. See `apps/breakout/src/web.rs` for why that property is worth a
// little awkwardness.

import { readUtf8 } from './wasm.js';

/**
 * Log levels, matching `__crcbl_breakout_log_level`.
 */
export const LOG_OFF = 0;
export const LOG_ERROR = 1;
export const LOG_WARN = 2;
export const LOG_INFO = 3;
export const LOG_DEBUG = 4;
export const LOG_TRACE = 5;

/**
 * Prints every queued line. Cheap when there are none — one call returning 0.
 *
 * @param {object} options
 * @param {WebAssembly.Memory} options.memory
 * @param {() => number} options.take pops a line and returns its byte length
 * @param {() => number} options.ptr the scratch address, read *after* `take`
 * @param {number} [options.limit] most lines to print in one call
 */
export function drainLog({ memory, take, ptr, limit = 256 }) {
  for (let i = 0; i < limit; i += 1) {
    const len = take();
    if (len === 0) return;
    // `ptr` after `take`, always: the buffer is refilled by `take` and may have
    // been reallocated by it.
    const line = readUtf8(memory, ptr(), len);
    if (line.startsWith('[ERROR]')) console.error(line);
    else if (line.startsWith('[WARN]')) console.warn(line);
    else console.log(line);
  }
}
