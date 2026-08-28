// The page that drives the Web Worker spawn backend, and the checks that say
// whether Rust actually ran on a second thread.
//
// It loads `crates/crcbl-jobs/examples/web_worker_gate.rs`, built by
// `web/build.sh --threads`. An example rather than a demo, because what it has
// to expose is the backend's own internals — a stack address per chunk, a
// thread-local holding a frame pointer, a checksum only a corrupted run gets
// wrong — and an example cannot reach a site artifact by any route.
//
// THIS IS NOT THE GATE ON A SAMPLE, AND THE TWO ARE DIFFERENT CLAIMS.
// `web/run-horde-threads-e2e.sh` drives the horde demo on a threaded site and
// asserts `__crcbl_horde_sim_threads() >= 2`: a real game's real pass, split
// across real workers, through the loader and the shim a visitor uses. What it
// cannot do is fail *usefully* — a game that stopped rendering, an asset that
// did not arrive and a worker that never started all read as "no second thread".
// This page has no engine, no canvas and no assets, so when it goes red the
// thing that broke is the spawn ABI.
//
// WHAT MAKES THIS A BROWSER GATE RATHER THAN A SECOND COPY OF THE NODE ONE.
// `web/tools/worker-gate.mjs` brings the same sequence up under
// `node:worker_threads` and asserts the same things about it. Four claims are
// simply not in its reach, and each is one this file is here for:
//
//   * a browser `Worker` accepts a structured-cloned `WebAssembly.Module` and a
//     shared `WebAssembly.Memory` at all — node clones its own;
//   * the memory can be constructed in the first place, which is a property of
//     the **document** (`crossOriginIsolated`) rather than of the build, and is
//     the reason nothing threaded is publishable;
//   * a page's main thread survives *driving* a pool whose workers park on
//     `memory.atomic.wait32`. Node lets its main thread block; a browser traps
//     rather than blocking, and `crcbl_jobs::workers` says in as many words
//     that no gate there can show it;
//   * the whole of it works in the engine a visitor would use.
//
// THE OBSERVABLE IS NOT "A WORKER STARTED". A `Worker` object constructs on any
// page, and `__crcbl_web_jobs_host_ready` returns a number whether or not a
// thread ever exists. What is asserted below is `gate_threads() >= 2` — chunks
// counted by the *stack address* they ran on — together with `gate_clobbered()`
// and `gate_tls_shared()`, which say that those stacks and thread-locals were
// the workers' own. Nothing on one thread can produce that.
//
// THE RED SWITCHES, in the query string, one per assertion that would otherwise
// be untestable:
//
//   ?no-stack-pointer   the worker never writes `__stack_pointer`
//   ?no-init-tls        the worker never calls `__wasm_init_tls`
//   ?no-host-ready      the page never announces itself
//   ?force-host-ready   announce the *non-threaded* artifact anyway — the one
//                       thing this page must never do, and the only way to show
//                       that the refusal below is load-bearing rather than
//                       decorative
//   ?workers=N          how many workers to ask the pool for
//
// Each must make the run go red, and go red *differently*; `web/run-jobs-e2e.sh`
// runs all four and checks which assertions each one broke.

import { WorkerHost } from '../engine/jobs.js';

/** The artifact, laid out beside this page by `web/run-jobs-e2e.sh`. */
const ARTIFACT = './web_worker_gate_bg.wasm';

/**
 * The *same example*, built the way every published artifact is built: no
 * atomics, no shared memory, no imports at all.
 *
 * It is here as a negative control, and it is the one this page could do real
 * damage with. `__crcbl_web_jobs_host_ready` is the whole of `Spawn::threaded`'s
 * answer, so announcing for an artifact no worker can attach to would make the
 * backend claim threads it can never have — the precise lie
 * `crcbl_jobs::workers` is shaped to prevent. Every artifact on the demo site
 * has exactly this shape, so this is not a contrived case; it is the common one.
 */
const PLAIN_ARTIFACT = './web_worker_gate_plain_bg.wasm';

/** How long to keep driving `par_for` while waiting for a worker to steal. */
const DEADLINE_MS = 20_000;

/** The name `crcbl_jobs::pool` gives every worker it spawns. */
const POOL_THREAD_NAME = 'pool';

/** How many workers to ask for when the query string does not say. */
const DEFAULT_WORKERS = 3;

/** @type {{ name: string, ok: boolean }[]} */
const checks = [];
/** @type {string[]} */
const notes = [];

/**
 * @param {boolean} condition
 * @param {string} what
 */
function check(condition, what) {
  checks.push({ name: what, ok: Boolean(condition) });
}

/** @param {string} line */
function note(line) {
  notes.push(line);
}

/** @param {number} ms */
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function render() {
  const log = document.getElementById('log');
  if (!log) return;
  const lines = checks.map((c) => `${c.ok ? 'ok  ' : 'FAIL'} ${c.name}`);
  log.textContent = [...notes, '', ...lines].join('\n');
}

async function run() {
  const params = new URLSearchParams(location.search);
  const requested = Number(params.get('workers') ?? DEFAULT_WORKERS);
  if (!Number.isInteger(requested) || requested < 1) {
    throw new Error(`?workers=${params.get('workers')} is not a worker count`);
  }
  const bringUp = {
    skipStackPointer: params.has('no-stack-pointer'),
    skipInitTls: params.has('no-init-tls'),
  };
  const skipHostReady = params.has('no-host-ready');
  for (const [flag, on] of [
    ['no-stack-pointer', bringUp.skipStackPointer],
    ['no-init-tls', bringUp.skipInitTls],
    ['no-host-ready', skipHostReady],
    ['force-host-ready', params.has('force-host-ready')],
  ]) {
    if (on) note(`RED CHECK: ?${flag}`);
  }

  // Named exactly as `web/tools/browser-e2e.mjs` names it, because it is the
  // same precondition and `web/run-jobs-e2e.sh` greps for it: every other check
  // here would still run on an origin with no COOP/COEP, and would fail for a
  // reason that says nothing about the headers.
  check(
    globalThis.crossOriginIsolated === true,
    'the document is cross-origin isolated'
  );

  // ---- the negative control, before anything else -------------------------
  // An artifact that owns its memory, on the very page that *can* start
  // workers, so the refusal is the host's own judgement rather than an absent
  // capability.
  const plain = await WorkerHost.load(PLAIN_ARTIFACT);
  check(
    plain.threaded === false && plain.refusal !== null,
    `an artifact that owns its memory is refused workers${
      plain.refusal ? ` — ${plain.refusal}` : ' (it was not)'
    }`
  );
  const plainAnnounced = params.has('force-host-ready')
    ? plain.exports.__crcbl_web_jobs_host_ready(requested + 1)
    : plain.announce(requested + 1);
  check(plainAnnounced === 0, 'announcing a non-threaded artifact answers 0');
  check(
    plain.exports.gate_threaded() === 0,
    'threaded() stays false for an artifact no worker could attach to'
  );
  check(
    plain.exports.gate_pool(requested) === 0,
    'a pool on it gets zero workers rather than workers that never arrive'
  );
  check(
    plain.exports.__crcbl_web_jobs_pending() === 0,
    'and queues nothing for a host that could not drain it'
  );
  // The degradation is a whole answer, not a broken one: the same checksum the
  // threaded run has to reproduce comes out of the inline path.
  const plainExpected = plain.exports.gate_expected();
  check(
    plain.exports.gate_run() === plainExpected,
    'it still computes the right answer inline'
  );

  const host = await WorkerHost.load(ARTIFACT);
  const ex = host.exports;
  check(
    host.threaded,
    `the artifact imports a shared memory this document can construct${
      host.refusal ? ` — ${host.refusal}` : ''
    }`
  );
  note(`memory: buffer is a ${host.memory?.buffer.constructor.name}`);
  note(
    `tls:    __tls_size=${host.global('__tls_size')}, ` +
      `__tls_align=${host.global('__tls_align')}`
  );
  note(`navigator.hardwareConcurrency = ${navigator.hardwareConcurrency}`);

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
  const announced = skipHostReady ? 0 : host.announce(requested + 1);
  check(
    announced === requested + 1,
    'host_ready answers the worker count it recorded'
  );
  check(ex.gate_threaded() === 1, 'threaded() is true once the host has');
  check(
    ex.gate_parallelism() === requested + 1,
    'parallelism() is what the host reported'
  );

  // Before any worker exists, because it runs the same stack probes a worker
  // without a stack of its own would be sharing.
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

  const taken = host.take();
  // `taken.length > 0` on each of these and not only on the first: a set built
  // from an empty list has the size an empty list has, and `every` on one is
  // true, so a run that drained nothing — which is exactly what `?no-host-ready`
  // produces — would report four of these five as `ok` while nothing had
  // happened at all.
  check(taken.length === requested, 'every request comes back off the queue');
  check(
    taken.length > 0 &&
      new Set(taken.map((t) => t.handle)).size === taken.length,
    'no two requests share a handle'
  );
  const names = new Set(taken.map((t) => t.name));
  check(
    names.size === 1 && names.has(POOL_THREAD_NAME),
    `the thread name reaches the host as \`${POOL_THREAD_NAME}\`` +
      `, got ${[...names].join(', ') || 'nothing'}`
  );
  check(
    taken.length > 0 && taken.every((t) => t.stackTop !== 0 && t.tlsPtr !== 0),
    'a stack and a TLS block are allocated for every worker'
  );
  check(
    taken.length > 0 &&
      new Set(taken.map((t) => t.stackTop)).size === taken.length,
    'no two workers are handed the same stack'
  );

  // ---- bring the workers up ------------------------------------------------
  host.start(taken, bringUp);
  const deadline = Date.now() + DEADLINE_MS;
  while (host.up < taken.length && Date.now() < deadline) await pause(10);
  check(
    host.up === taken.length && taken.length > 0,
    'every worker instantiated and entered'
  );

  // ---- the run ------------------------------------------------------------
  // Driven until two workers have taken a chunk, because `par_for` finishes on
  // the calling thread whether or not a worker ever wakes — so "the answer was
  // right" cannot stand in for "a worker ran it", and one worker running is not
  // enough for the shared-TLS assertion below to have anything to see.
  // The driver plus two workers, or as many as the pool actually took.
  const wantThreads = Math.min(3, taken.length + 1);
  let runs = 0;
  let wrong = 0;
  /** @type {string[]} */
  const traps = [];
  while (Date.now() < deadline) {
    runs += 1;
    // A trap is a result, not a crash: a worker running on the driver's stack
    // corrupts the driver's own frames, and what comes back is an unreachable
    // or a bad table index rather than a wrong number.
    try {
      if (ex.gate_run() !== expected) wrong += 1;
    } catch (error) {
      traps.push(String(error));
      break;
    }
    // **A second *worker*, not a second thread.** `gate_threads` counts the
    // driver as one, so stopping at two stopped as soon as a single worker had
    // taken a chunk — and the shared-TLS assertion needs a *second* worker to
    // arrive and find the first one's frame in what should be its own
    // thread-local. A run that stopped at two could not observe the thing this
    // gate is about, which is how the `no-init-tls` red check came back green
    // on a CI runner that gave one worker all four chunks. In a healthy build
    // every worker has a cell of its own, so the driver plus two workers is
    // three; in a sabotaged one the count sticks at two and the observation
    // itself is what ends the loop.
    if (
      runs >= 4 &&
      (ex.gate_threads() >= wantThreads || ex.gate_tls_shared() !== 0)
    ) {
      break;
    }
    await pause(1);
  }

  const threads = ex.gate_threads();
  note(`drove ${runs} par_for call(s); ${threads} thread(s) ran chunks`);
  for (const error of host.errors) note(`worker error: ${error}`);
  for (const trap of traps) note(`trapped: ${trap}`);

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

  host.stop();
}

// `window.jobsResult` is the whole verdict and `window.jobsDone` is the flag the
// driver polls. Set in a `finally` so that a page which threw still reports what
// it managed to check — a run that boots and dies leaves the driver reading a
// timeout otherwise, which says nothing about why.
let fatal = null;
try {
  await run();
} catch (error) {
  fatal = String(error?.stack ?? error);
} finally {
  render();
  window.jobsResult = {
    checks,
    notes,
    fatal,
    failed: checks.filter((c) => !c.ok).map((c) => c.name),
  };
  window.jobsDone = true;
}
