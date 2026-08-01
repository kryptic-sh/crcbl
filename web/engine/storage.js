// Assets over `fetch()` and saves in the Origin Private File System.
//
// Implements the JS side of `crcbl-store`'s browser backends
// (`crates/crcbl-store/src/web/{fetch,opfs}.rs`). Both are the same shape: a
// resident cache inside wasm that the engine reads synchronously, filled and
// drained out of band by the asynchronous calls only a browser can make.
//
// TWO OBLIGATIONS THAT ARE REAL BUGS WHEN MISSED, both stated by those modules:
//
//   1. A fetch slot that is neither committed nor failed *leaks* — the key
//      stays `InFlight` forever and the engine polls it forever. `fail` is the
//      obligation, not the optional path, so every `deliver` here has a
//      `catch`.
//   2. OPFS records for one key must be applied in the order they were taken.
//      A shim that runs them concurrently can land an older generation last and
//      the wasm side cannot detect it. `flushOpfs` drains strictly
//      sequentially, with a guard so two callers cannot interleave.

import { readBytes, readUtf8, writeBytes, writeUtf8 } from './wasm.js';

/** `__crcbl_web_opfs_environment`: the shim runs in a Worker, not a Window. */
export const ENV_WORKER = 1 << 0;
/** `FileSystemFileHandle.createSyncAccessHandle` exists. */
export const ENV_SYNC_ACCESS = 1 << 1;
/** Storage is persisted rather than best-effort. */
export const ENV_PERSISTED = 1 << 2;

// ---------------------------------------------------------------------------
// fetch
// ---------------------------------------------------------------------------

/**
 * Fetches `url` and delivers it into slot `id`, or fails the slot.
 *
 * @param {Record<string, Function>} exports
 * @param {WebAssembly.Memory} memory
 * @param {number} id
 * @param {string} url
 */
async function deliver(exports, memory, id, url) {
  try {
    const response = await fetch(url);
    if (!response.ok) {
      exports.__crcbl_web_fetch_fail(id, response.status);
      return;
    }
    const bytes = new Uint8Array(await response.arrayBuffer());
    // May grow wasm memory, so the view below is built after it — never before.
    const ptr = exports.__crcbl_web_fetch_buffer(id, bytes.length);
    if (ptr === 0) {
      exports.__crcbl_web_fetch_fail(id, 0);
      return;
    }
    writeBytes(memory, ptr, bytes);
    exports.__crcbl_web_fetch_commit(id, bytes.length);
  } catch {
    // Network error, CORS, abort. Never leave the slot open.
    exports.__crcbl_web_fetch_fail(id, 0);
  }
}

/**
 * Fetches every key in `keys` before the game boots.
 *
 * Pre-load is the intended mode, not an optimisation: the sample's start-up is
 * not written as a state machine that tolerates a missing asset for forty
 * frames, so by the time engine code calls `read` the answer has to be resident
 * already. The request/poll path below is what serves anything discovered later.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @param {string[]} options.keys
 * @param {string} options.base the URL prefix, ending in `/`
 * @returns {Promise<void>}
 */
export async function preloadAssets({ exports, memory, keys, base }) {
  const pending = [];
  for (const key of keys) {
    const ptr = exports.__crcbl_web_fetch_key_ptr();
    const capacity = exports.__crcbl_web_fetch_key_capacity();
    const len = writeUtf8(memory, ptr, capacity, key);
    if (len === null) {
      console.warn(`crcbl: asset key too long, skipped: ${key}`);
      continue;
    }
    const id = exports.__crcbl_web_fetch_begin(len);
    if (id === 0) {
      // The key was refused — see `canonical_key`. Do not retry it.
      console.warn(`crcbl: asset key refused, skipped: ${key}`);
      continue;
    }
    pending.push(deliver(exports, memory, id, base + key));
  }
  await Promise.all(pending);
}

/**
 * Starts a fetch for everything the engine has asked for since the last call.
 *
 * The URL comes from wasm rather than being rebuilt here from the key: there is
 * exactly one place a key becomes a URL and it is the place that validated the
 * key. See `crates/crcbl-store/src/web/mod.rs` on path containment.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 */
export function drainFetch({ exports, memory }) {
  for (;;) {
    const id = exports.__crcbl_web_fetch_take();
    if (id === 0) return;
    const url = readUtf8(
      memory,
      exports.__crcbl_web_fetch_url_ptr(id),
      exports.__crcbl_web_fetch_url_len(id),
    );
    void deliver(exports, memory, id, url);
  }
}

// ---------------------------------------------------------------------------
// OPFS
// ---------------------------------------------------------------------------

/**
 * The OPFS root, or `null` where there is none.
 *
 * `navigator.storage.getDirectory` is missing on browsers without OPFS and
 * throws in an insecure context (`file://`, plain HTTP off localhost). Both are
 * ordinary, and both must end with `__crcbl_web_opfs_ready` being called
 * anyway — see `restoreOpfs`.
 *
 * @returns {Promise<FileSystemDirectoryHandle | null>}
 */
async function openRoot() {
  try {
    if (!navigator.storage?.getDirectory) return null;
    return await navigator.storage.getDirectory();
  } catch (error) {
    console.warn('crcbl: no OPFS in this context; saves will not persist', error);
    return null;
  }
}

/**
 * Reads every file in the OPFS root into the wasm store, then declares the
 * restore complete.
 *
 * `__crcbl_web_opfs_ready` is called on **every** path, including the one where
 * there is no OPFS at all. Until it is called every read answers `Pending`,
 * which the engine treats as "ask again" — so a page that skipped it because
 * storage was unavailable would leave the game waiting for a save that is never
 * coming.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @returns {Promise<FileSystemDirectoryHandle | null>}
 */
export async function restoreOpfs({ exports, memory }) {
  const root = await openRoot();

  let flags = 0;
  if (typeof Window === 'undefined' || !(globalThis instanceof Window)) flags |= ENV_WORKER;
  if (
    typeof FileSystemFileHandle !== 'undefined' &&
    'createSyncAccessHandle' in FileSystemFileHandle.prototype
  ) {
    flags |= ENV_SYNC_ACCESS;
  }
  try {
    if (await navigator.storage?.persisted?.()) flags |= ENV_PERSISTED;
  } catch {
    // `persisted()` rejects in some contexts; the bit is a diagnostic.
  }
  exports.__crcbl_web_opfs_environment(flags);

  if (root) {
    for await (const [name, handle] of root.entries()) {
      if (handle.kind !== 'file') continue;
      const ptr = exports.__crcbl_web_opfs_key_ptr();
      const capacity = exports.__crcbl_web_opfs_key_capacity();
      const len = writeUtf8(memory, ptr, capacity, name);
      if (len === null) continue;
      const id = exports.__crcbl_web_opfs_restore_begin(len);
      // Not a name this store wrote — someone else's file in the same origin.
      if (id === 0) continue;
      try {
        const file = new Uint8Array(await (await handle.getFile()).arrayBuffer());
        // May grow wasm memory; the view is built after it.
        const buffer = exports.__crcbl_web_opfs_restore_buffer(id, file.length);
        if (buffer === 0) {
          exports.__crcbl_web_opfs_restore_abort(id);
          continue;
        }
        writeBytes(memory, buffer, file);
        exports.__crcbl_web_opfs_restore_commit(id, file.length);
      } catch (error) {
        console.warn(`crcbl: could not read ${name} from OPFS`, error);
        exports.__crcbl_web_opfs_restore_abort(id);
      }
    }
  }

  exports.__crcbl_web_opfs_ready();
  return root;
}

/** Guards `flushOpfs` so two callers cannot interleave two records. */
let draining = false;

/**
 * Applies every queued save record, strictly in order.
 *
 * Call it once per frame, and again from `visibilitychange` and `pagehide` —
 * a write returns as soon as it is *queued*, so a tab closed between the write
 * and this call loses it, and those two events are the last chance a page gets.
 *
 * @param {object} options
 * @param {Record<string, Function>} options.exports
 * @param {WebAssembly.Memory} options.memory
 * @param {FileSystemDirectoryHandle | null} options.root
 * @returns {Promise<void>}
 */
export async function flushOpfs({ exports, memory, root }) {
  if (draining) return;
  draining = true;
  try {
    for (;;) {
      const seq = exports.__crcbl_web_opfs_take();
      if (seq === 0) return;
      const kind = exports.__crcbl_web_opfs_record_kind(seq);
      const name = readUtf8(
        memory,
        exports.__crcbl_web_opfs_record_name_ptr(seq),
        exports.__crcbl_web_opfs_record_name_len(seq),
      );
      let ok = 0;
      try {
        if (!root) throw new Error('no OPFS');
        if (kind === 1) {
          // A copy, before any `await`: the record's bytes are freed by the
          // `ack` below, and `close()` resolves after it.
          const body = readBytes(
            memory,
            exports.__crcbl_web_opfs_record_data_ptr(seq),
            exports.__crcbl_web_opfs_record_data_len(seq),
          );
          const handle = await root.getFileHandle(name, { create: true });
          const writable = await handle.createWritable();
          await writable.write(body);
          await writable.close();
        } else {
          await root.removeEntry(name).catch(() => {});
        }
        ok = 1;
      } catch (error) {
        console.warn(`crcbl: OPFS record ${seq} (${name}) failed`, error);
        ok = 0;
      }
      exports.__crcbl_web_opfs_ack(seq, ok);
    }
  } finally {
    draining = false;
  }
}

/**
 * Whether every queued save has reached the disk.
 *
 * @param {Record<string, Function>} exports
 * @returns {boolean}
 */
export function opfsSettled(exports) {
  return exports.__crcbl_web_opfs_pending() === 0 && exports.__crcbl_web_opfs_inflight() === 0;
}
