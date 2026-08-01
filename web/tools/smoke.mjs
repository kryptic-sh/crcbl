#!/usr/bin/env node
// A browserless smoke test of the wasm module's boot sequence.
//
// `check-exports.mjs` proves the two halves agree about *names*. This proves
// the module actually runs: it instantiates the artifact under Node with every
// `wasm-bindgen` import stubbed to throw, and drives the exact call order
// `apps/breakout/src/web.rs` specifies, up to the point where a real GPU would
// be needed.
//
// WHERE IT STOPS, and why that is the honest line. The first
// `__crcbl_breakout_frame` after a size has been reported starts the device
// request, which is `navigator.gpu` reached through `web-sys` — a stubbed
// import here, and a browser everywhere else. Everything before that line is
// plain Rust with a browser-shaped ABI in front of it, and all of it runs here:
// the storage backends, the log queue, the key scratch, the shell, the window.
// Everything after it needs a GPU and a browser, and this file does not pretend
// otherwise.
//
// It has caught, and would catch again: a `#[no_mangle]` that stopped being
// exported, a scratch pointer of 0 because a thread-local was never
// initialised, a panic inside `prepare`, and a status code that disagrees with
// the docs.
//
// Usage:
//   node web/tools/smoke.mjs <path-to.wasm>

import { readFile } from 'node:fs/promises';

/** Mirrors `STATUS_*` in `apps/breakout/src/web.rs`. */
const STATUS_PREPARED = 1;
const STATUS_BOOTING = 2;

const decoder = new TextDecoder();
const encoder = new TextEncoder();

/** @type {string[]} */
const failures = [];

/**
 * @param {boolean} condition
 * @param {string} what
 */
function check(condition, what) {
  if (condition) console.log(`  ok   ${what}`);
  else {
    console.log(`  FAIL ${what}`);
    failures.push(what);
  }
}

/**
 * Every import stubbed. They are all `wasm-bindgen`'s, and nothing this file
 * calls should reach one — a stub that throws turns "it quietly used the DOM"
 * into a stack trace instead of a pass.
 *
 * @param {WebAssembly.Module} module
 */
function stubImports(module) {
  /** @type {Record<string, Record<string, unknown>>} */
  const imports = {};
  for (const descriptor of WebAssembly.Module.imports(module)) {
    imports[descriptor.module] ??= {};
    const name = `${descriptor.module}.${descriptor.name}`;
    switch (descriptor.kind) {
      case 'function':
        imports[descriptor.module][descriptor.name] = () => {
          throw new Error(`smoke: reached the browser through ${name}`);
        };
        break;
      case 'table':
        imports[descriptor.module][descriptor.name] = new WebAssembly.Table({
          initial: 0,
          element: 'anyfunc',
        });
        break;
      case 'memory':
        imports[descriptor.module][descriptor.name] = new WebAssembly.Memory({ initial: 1 });
        break;
      default:
        imports[descriptor.module][descriptor.name] = 0;
    }
  }
  return imports;
}

async function main() {
  const wasmPath = process.argv[2];
  if (!wasmPath) {
    console.error('usage: node web/tools/smoke.mjs <path-to.wasm>');
    process.exit(2);
  }

  const module = new WebAssembly.Module(await readFile(wasmPath));
  const instance = new WebAssembly.Instance(module, stubImports(module));
  const ex = /** @type {Record<string, Function>} */ (instance.exports);
  const memory = /** @type {WebAssembly.Memory} */ (instance.exports.memory);

  /** Drains the engine's log queue, exactly as `web/engine/log.js` does. */
  function drainLog() {
    const lines = [];
    for (;;) {
      const len = ex.__crcbl_breakout_log_take();
      if (len === 0) return lines;
      lines.push(decoder.decode(new Uint8Array(memory.buffer, ex.__crcbl_breakout_log_ptr(), len)));
    }
  }

  console.log(`smoke: ${wasmPath}`);

  // ---- before `prepare`, every storage export answers 0 ------------------
  check(ex.__crcbl_breakout_status() === 0, 'status is IDLE before prepare');
  check(ex.__crcbl_web_fetch_key_ptr() === 0, 'the fetch scratch is absent before prepare');
  check(ex.__crcbl_web_opfs_key_ptr() === 0, 'the OPFS scratch is absent before prepare');

  // ---- prepare -----------------------------------------------------------
  check(ex.__crcbl_breakout_prepare() === 1, 'prepare succeeds');
  check(ex.__crcbl_breakout_status() === STATUS_PREPARED, 'status is PREPARED');
  check(ex.__crcbl_breakout_prepare() === 0, 'a second prepare is refused');
  check(
    drainLog().some((line) => line.includes('prepared')),
    'the log queue delivers the line prepare wrote',
  );
  check(ex.__crcbl_breakout_log_take() === 0, 'a drained log queue answers 0');

  // ---- the key scratch: the ABI correction this slice made ---------------
  const keyPtr = ex.__crcbl_web_key_scratch_ptr();
  const keyCapacity = ex.__crcbl_web_key_scratch_capacity();
  check(keyPtr !== 0 && keyCapacity >= 64, 'the key scratch has an address and a capacity');
  check(ex.__crcbl_web_key_scratch_ptr() === keyPtr, 'the key scratch address is stable');

  // ---- the fetch pre-load round trip -------------------------------------
  const fetchPtr = ex.__crcbl_web_fetch_key_ptr();
  const fetchCapacity = ex.__crcbl_web_fetch_key_capacity();
  check(fetchPtr !== 0 && fetchCapacity > 0, 'the fetch scratch exists after prepare');
  const key = encoder.encode('sprites/ball.png');
  new Uint8Array(memory.buffer, fetchPtr, key.length).set(key);
  const slot = ex.__crcbl_web_fetch_begin(key.length);
  check(slot !== 0, 'a pre-load slot opens for a valid key');
  check(ex.__crcbl_web_fetch_inflight() === 1, 'the slot is counted in flight');
  const body = ex.__crcbl_web_fetch_buffer(slot, 4);
  check(body !== 0, 'the body buffer is allocated');
  new Uint8Array(memory.buffer, body, 4).set([1, 2, 3, 4]);
  check(ex.__crcbl_web_fetch_commit(slot, 4) === 1, 'the body commits');
  check(ex.__crcbl_web_fetch_inflight() === 0, 'no slot is left open');

  // A key that could escape the asset root is refused rather than fetched.
  const escape = encoder.encode('../../etc/passwd');
  new Uint8Array(memory.buffer, ex.__crcbl_web_fetch_key_ptr(), escape.length).set(escape);
  check(ex.__crcbl_web_fetch_begin(escape.length) === 0, 'a key that escapes the root is refused');

  // ---- OPFS restore ------------------------------------------------------
  check(ex.__crcbl_web_opfs_environment(1) === 1, 'the environment is declared');
  check(ex.__crcbl_web_opfs_ready() === 1, 'the restore phase can be closed');
  check(ex.__crcbl_web_opfs_take() === 0, 'nothing is queued for the disk yet');

  // ---- boot --------------------------------------------------------------
  ex.__crcbl_web_canvas(1);
  check(ex.__crcbl_breakout_boot() === 1, 'boot opens the shell and the window');
  check(ex.__crcbl_breakout_status() === STATUS_BOOTING, 'status is BOOTING');
  check(
    drainLog().some((line) => line.includes('web backend')),
    'the web shell backend is the one that opened',
  );

  // A key event, delivered exactly as `web/engine/shell.js` delivers one. It
  // queues rather than being dropped, because the window now exists.
  const code = encoder.encode('ArrowLeft');
  const composed = encoder.encode('ArrowLeft');
  new Uint8Array(memory.buffer, keyPtr, code.length).set(code);
  new Uint8Array(memory.buffer, keyPtr + code.length, composed.length).set(composed);
  ex.__crcbl_web_key(
    1,
    keyPtr,
    code.length,
    keyPtr + code.length,
    composed.length,
    123.5,
    1 << 4,
  );
  check(
    !drainLog().some((line) => line.includes('dropped')),
    'a key event for the live canvas is not dropped',
  );

  // ---- the line this file does not cross ---------------------------------
  // `__crcbl_breakout_frame` after a size would request a device, and a device
  // is `navigator.gpu`. Everything from here needs a browser.
  check(ex.__crcbl_breakout_shutdown() === 1, 'shutdown tears down a booting loop');

  if (failures.length > 0) {
    console.error(`\nsmoke: FAILED (${failures.length})`);
    process.exit(1);
  }
  console.log('\nsmoke: OK');
}

await main();
