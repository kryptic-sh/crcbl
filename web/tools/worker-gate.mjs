#!/usr/bin/env node
// The gate over `crcbl_jobs::workers` — the Web Worker backend behind the
// `Spawn` seam.
//
// It is the only thing anywhere that brings a worker up through that ABI and
// makes it run Rust, so it is where the backend's claims are actually checked
// rather than argued. `check-exports.mjs --threads` proves the *symbols* a
// worker needs exist; this proves the sequence built on them works.
//
// WHAT IT ASSERTS, in the order the run makes them observable:
//
//   1. `Spawn::threaded()` is false, and `parallelism()` is one, before the
//      host has announced anything. A `+atomics` artifact loaded by a page with
//      no worker shim has no threads, and the backend must not claim otherwise.
//   2. `Pool::with_workers` in that state gets **zero** workers — the
//      degradation path the pool already had, still working.
//   3. After `__crcbl_web_jobs_host_ready(n)`: `threaded()` is true and
//      `parallelism()` is `n`.
//   4. Each `spawn` lands one request on the queue, with the thread name it was
//      given, and a handle nothing else was given.
//   5. `__crcbl_web_jobs_entry` refuses a handle this module never handed out,
//      **without dereferencing it** — the handle is an index, not an address.
//   6. A worker brought up on the documented sequence runs the closure.
//   7. **It runs it on a stack of its own.** This is the one that needs saying
//      twice: a worker whose `__stack_pointer` was never written shares the main
//      thread's stack, and a closure that merely allocates still returns the
//      right answer every time (`docs/plan/21-jobs.md`, finding 1's 2026-08-23
//      correction). So the chunk work writes a large array on its own stack,
//      holds it, and reads every word back — `gate_clobbered` and the run's
//      checksum are both downstream of that.
//   8. **And it has thread-locals of its own.** A worker that skipped
//      `__wasm_init_tls` does not necessarily trap: measured here, `__tls_base`
//      is left at zero and every worker's thread-locals alias one address, read
//      and written without complaint. So the assertion is `gate_tls_shared` — a
//      thread finding a frame address in its thread-local that its own stack
//      could not have produced.
//
// THE TWO RED SWITCHES exist because assertions 6 and 7 have to be shown to
// fail. `--no-stack-pointer` skips the `__stack_pointer` write and
// `--no-init-tls` skips the `__wasm_init_tls` call, each leaving the rest of
// the sequence alone. A run with either must go red; that is what makes a green
// run mean something.
//
// NODE, NOT A BROWSER. `node:worker_threads` is what is available without a
// browser in the loop, and the bootstrap it exercises is the same one: a
// structured-cloned `WebAssembly.Module`, one shared `WebAssembly.Memory`, and
// per-instance globals. What it is NOT evidence for is that a browser `Worker`
// accepts a cloned module on all three engines, or that `Atomics.wait` behaves
// the way it does here — node lets its main thread block and a browser does
// not. See `docs/backlog.md`.
//
// Usage:
//   node web/tools/worker-gate.mjs <path-to.wasm> [--workers N]
//   node web/tools/worker-gate.mjs <path-to.wasm> --no-stack-pointer
//   node web/tools/worker-gate.mjs <path-to.wasm> --no-init-tls

import { readFile } from 'node:fs/promises';
import { Worker } from 'node:worker_threads';

import { importedMemoryLimits } from './wasm-memory.mjs';

/** How long to keep driving `par_for` while waiting for a worker to steal. */
const DEADLINE_MS = 20_000;

/** The name `crcbl_jobs::pool` gives every worker it spawns. */
const POOL_THREAD_NAME = 'pool';

/**
 * The worker half of the bootstrap, run with `eval: true` so the whole gate is
 * one file.
 *
 * A browser does this from a `Blob` URL or a real module file instead; the five
 * steps are the same, and their order is the contract `crcbl_jobs::workers`
 * documents.
 */
const WORKER_SOURCE = `
const { workerData, parentPort } = require('node:worker_threads');
const { wasm, memory, handle, stackTop, tlsPtr, skipStackPointer, skipInitTls } = workerData;
const instance = new WebAssembly.Instance(wasm, { env: { memory } });
const exports = instance.exports;
// Wasm globals are per-instance, so this one starts where the linker put the
// main thread's stack. Leaving it there is the silent failure this gate exists
// for.
if (!skipStackPointer) exports.__stack_pointer.value = stackTop;
if (!skipInitTls) exports.__wasm_init_tls(tlsPtr);
parentPort.postMessage({ up: true });
// Does not return for a pool worker: the closure is the worker's whole loop.
const ran = exports.__crcbl_web_jobs_entry(handle);
parentPort.postMessage({ up: false, ran });
`;

/** @type {string[]} */
const failures = [];
let quiet = false;

/**
 * @param {boolean} condition
 * @param {string} what
 */
function check(condition, what) {
  if (condition) {
    if (!quiet) console.log(`  ok   ${what}`);
  } else {
    console.log(`  FAIL ${what}`);
    failures.push(what);
  }
}

/** @param {number} ms */
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * A UTF-8 string out of the module's memory.
 *
 * The copy is not optional: `TextDecoder` refuses a view onto a
 * `SharedArrayBuffer`, which is the only kind of buffer a threaded artifact
 * has.
 *
 * @param {WebAssembly.Memory} memory
 * @param {number} ptr
 * @param {number} len
 */
function readString(memory, ptr, len) {
  if (ptr === 0 || len === 0) return '';
  const copy = new Uint8Array(memory.buffer, ptr, len).slice();
  return new TextDecoder().decode(copy);
}

async function main() {
  const args = process.argv.slice(2);
  quiet = args.includes('--quiet');
  const skipStackPointer = args.includes('--no-stack-pointer');
  const skipInitTls = args.includes('--no-init-tls');
  const workersFlag = args.indexOf('--workers');
  const requested = workersFlag >= 0 ? Number(args[workersFlag + 1]) : 3;
  // The guard on `workersFlag` is not decoration: without `--workers`, the
  // index is -1 and `args[0]` — the artifact — is what gets skipped.
  const count = workersFlag >= 0 ? args[workersFlag + 1] : undefined;
  const wasmPath = args.find((a) => !a.startsWith('--') && a !== count);
  if (!wasmPath || !Number.isInteger(requested) || requested < 1) {
    console.error(
      'usage: node web/tools/worker-gate.mjs <path-to.wasm> [--workers N] ' +
        '[--no-stack-pointer] [--no-init-tls] [--quiet]'
    );
    process.exit(2);
  }

  const bytes = await readFile(wasmPath);
  const limits = importedMemoryLimits(bytes);
  if (limits === undefined || !limits.shared) {
    console.error(
      `worker-gate: ${wasmPath} does not import a shared \`env.memory\`.\n` +
        '    Build it with `web/build.sh --threads`; ' +
        '`check-exports.mjs --threads` names the missing link argument.'
    );
    process.exit(2);
  }

  const wasm = new WebAssembly.Module(bytes);
  const memory = new WebAssembly.Memory({
    initial: limits.minimum,
    maximum: limits.maximum,
    shared: true,
  });
  const { exports } = await WebAssembly.instantiate(wasm, { env: { memory } });
  const ex = /** @type {Record<string, Function>} */ (
    /** @type {unknown} */ (exports)
  );
  const globalValue = (/** @type {string} */ name) =>
    /** @type {WebAssembly.Global} */ (exports[name]).value;

  console.log(`worker-gate: ${wasmPath}`);
  console.log(
    `  memory:  ${limits.minimum}..${limits.maximum ?? 'unbounded'} pages, ` +
      `buffer is a ${memory.buffer.constructor.name}`
  );
  console.log(
    `  tls:     __tls_size=${globalValue('__tls_size')}, ` +
      `__tls_align=${globalValue('__tls_align')}`
  );
  if (skipStackPointer) console.log('  RED CHECK: skipping __stack_pointer');
  if (skipInitTls) console.log('  RED CHECK: skipping __wasm_init_tls');

  // ---- before the host says anything --------------------------------------
  check(ex.gate_threaded() === 0, 'threaded() is false before the host speaks');
  check(ex.gate_parallelism() === 1, 'parallelism() is one before it too');
  check(
    ex.gate_pool(4) === 0,
    'a pool built in that state gets zero workers and runs inline'
  );
  check(
    ex.__crcbl_web_jobs_pending() === 0,
    'a refused spawn queues nothing for a host to drain'
  );

  // ---- the host announces --------------------------------------------------
  check(
    ex.__crcbl_web_jobs_host_ready(requested + 1) === requested + 1,
    'host_ready answers the worker count it recorded'
  );
  check(ex.gate_threaded() === 1, 'threaded() is true once the host has');
  check(
    ex.gate_parallelism() === requested + 1,
    'parallelism() is what the host reported'
  );

  // Before any worker exists, because it runs the same stack probes this
  // thread's stack would otherwise be sharing.
  const expected = ex.gate_expected();
  check(
    ex.gate_clobbered() === 0,
    'the single-threaded reference run does not clobber its own stack'
  );

  // ---- the spawn queue -----------------------------------------------------
  check(
    ex.gate_pool(requested) === requested,
    `a pool asked for ${requested} workers gets them`
  );
  check(
    ex.__crcbl_web_jobs_pending() === requested,
    'every spawn landed one request on the queue'
  );

  /** @type {{ handle: number, stackTop: number, tlsPtr: number }[]} */
  const taken = [];
  const names = new Set();
  while (ex.__crcbl_web_jobs_pending() > 0) {
    const handle = ex.__crcbl_web_jobs_take();
    if (handle === 0) break;
    names.add(
      readString(
        memory,
        ex.__crcbl_web_jobs_name_ptr(),
        ex.__crcbl_web_jobs_name_len()
      )
    );
    taken.push({
      handle,
      stackTop: ex.__crcbl_web_jobs_stack_alloc(),
      tlsPtr: ex.__crcbl_web_jobs_tls_alloc(
        globalValue('__tls_size'),
        globalValue('__tls_align')
      ),
    });
  }
  check(taken.length === requested, 'every request comes back off the queue');
  check(
    new Set(taken.map((t) => t.handle)).size === taken.length,
    'no two requests share a handle'
  );
  check(
    names.size === 1 && names.has(POOL_THREAD_NAME),
    `the thread name reaches the host as \`${POOL_THREAD_NAME}\`, got ${[...names].join(', ')}`
  );
  check(
    taken.every((t) => t.stackTop !== 0 && t.tlsPtr !== 0),
    'a stack and a TLS block are allocated for every worker'
  );
  check(
    new Set(taken.map((t) => t.stackTop)).size === taken.length,
    'no two workers are handed the same stack'
  );
  check(
    ex.__crcbl_web_jobs_take() === 0,
    'a drained queue answers 0 rather than a stale handle'
  );
  check(ex.__crcbl_web_jobs_name_ptr() === 0, 'and reports no name with it');
  check(
    ex.__crcbl_web_jobs_tls_alloc(globalValue('__tls_size'), 32) === 0,
    'a TLS alignment coarser than the allocator can promise is refused'
  );
  check(
    ex.__crcbl_web_jobs_tls_alloc(0, globalValue('__tls_align')) === 0,
    'a TLS block of no size is refused'
  );

  // A handle nothing handed out. It has to be rejected by lookup — if the ABI
  // passed pointers, this call would be a wild dereference instead.
  const invented = Math.max(...taken.map((t) => t.handle)) + 1000;
  check(
    ex.__crcbl_web_jobs_entry(invented) === 0,
    'an invented handle is refused rather than run'
  );

  // ---- bring the workers up ------------------------------------------------
  /** @type {Worker[]} */
  const running = [];
  /** @type {string[]} */
  const workerErrors = [];
  let up = 0;
  for (const { handle, stackTop, tlsPtr } of taken) {
    const worker = new Worker(WORKER_SOURCE, {
      eval: true,
      workerData: {
        wasm,
        memory,
        handle,
        stackTop,
        tlsPtr,
        skipStackPointer,
        skipInitTls,
      },
    });
    worker.on('message', (message) => {
      if (message.up) up += 1;
    });
    worker.on('error', (error) => workerErrors.push(String(error)));
    running.push(worker);
  }

  const deadline = Date.now() + DEADLINE_MS;
  while (up < taken.length && Date.now() < deadline) await pause(10);
  check(up === taken.length, 'every worker instantiated and entered');

  // ---- the run ------------------------------------------------------------
  // Driven until a second thread has taken a chunk, because `par_for` finishes
  // on the calling thread whether or not a worker ever wakes — that is a
  // property of the pool, and it is why "the answer was right" cannot stand in
  // for "a worker ran it".
  let runs = 0;
  let wrong = 0;
  /** @type {string[]} */
  const traps = [];
  while (Date.now() < deadline) {
    runs += 1;
    // A trap is a result, not a crash: a worker on the driver's stack corrupts
    // the driver's own frames, and what comes back is an unreachable or a bad
    // table index rather than a wrong number. Reported by name so the gate says
    // which assertion went red instead of dying with a stack trace.
    try {
      if (ex.gate_run() !== expected) wrong += 1;
    } catch (error) {
      traps.push(String(error));
      break;
    }
    if (ex.gate_threads() >= 2 && runs >= 4) break;
    await pause(1);
  }

  const threads = ex.gate_threads();
  console.log(
    `  drove ${runs} par_for call(s); ${threads} thread(s) ran chunks`
  );
  for (const error of workerErrors) console.log(`  worker error: ${error}`);

  for (const trap of traps) console.log(`  trapped: ${trap}`);

  check(traps.length === 0, 'no run trapped');
  check(threads >= 2, 'a chunk ran on a thread that is not the driver');
  check(
    wrong === 0,
    `every run reproduced the single-threaded checksum (${wrong}/${runs} did not)`
  );
  check(
    ex.gate_clobbered() === 0,
    'no chunk found its stack array changed underneath it'
  );
  check(
    ex.gate_tls_shared() === 0,
    "no thread found another thread's value in its own thread-local"
  );

  if (failures.length > 0) {
    console.error(`\nworker-gate: FAILED (${failures.length})`);
    process.exit(1);
  }
  console.log('\nworker-gate: OK');
  // The pool's workers never return, so nothing here can be awaited into an
  // exit. Terminating them is the whole of the teardown.
  await Promise.all(running.map((worker) => worker.terminate()));
  process.exit(0);
}

await main();
