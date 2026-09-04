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
//
// AND ONE SWITCH THAT IS NOT A RED CHECK: `?no-isolation`.
//
//   It selects the *other* configuration rather than sabotaging this one. GitHub
//   Pages sends no COOP/COEP, so the published origin has no
//   `SharedArrayBuffer`, the threaded artifact cannot be instantiated there at
//   all, and every artifact on the demo site degrades onto `Inline`'s behaviour
//   — which `docs/plan/21-jobs.md`'s first rung records as a **supported
//   configuration** rather than a gap. A supported configuration the gate never
//   ran was still a gap, so `web/run-jobs-e2e.sh` drives this page a second time
//   with `web/tools/serve.mjs --no-isolation` and this switch, and
//   {@link runWithoutIsolation} is the list it runs.
//
//   The switch says what is *expected*, never what is true: every assertion
//   under it reads the document and the artifact, so pairing it with an isolated
//   origin fails on its first line rather than passing quietly. `?force-host-ready`
//   composes with it, and is the falsifier for the one assertion there that
//   nothing else can move — announcing the plain artifact anyway is the only way
//   `Inline` becomes `Workers` on an origin with no shared memory.

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

/** `crcbl_jobs`'s browser backend, by the name its type has. */
const WORKERS = 'Workers';

/** The behaviour it degrades onto, by the name *that* type has. */
const INLINE = 'Inline';

/**
 * Which of the two behaviours the artifact is actually running, asked of the
 * artifact rather than assumed from the page.
 *
 * **The type is not the observable; its answer is.** `default_spawner` returns
 * `Workers` on `wasm32` and there is no second choice to make, so naming the
 * type would name a constant. What a caller above the seam sees is
 * `Spawn::threaded` — false until a page announces through the shim — and until
 * it does, `crcbl_jobs`'s own docs say the artifact "degrades onto `Inline`'s
 * behaviour". So that is what this reports, and it is what the two runs of this
 * page differ in: an isolated origin reaches {@link WORKERS}, the published one
 * reaches {@link INLINE}.
 *
 * @param {Record<string, any>} exports
 * @returns {string}
 */
function backendName(exports) {
  return exports.gate_threaded() === 1 ? WORKERS : INLINE;
}

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

/**
 * The list for the origin a visitor to the published site actually gets.
 *
 * **Nothing here is sabotage.** GitHub Pages sends no COOP/COEP pair, so on
 * crcbl.kryptic.sh `SharedArrayBuffer` does not exist, the artifact this page
 * loads for the threaded run cannot be instantiated at all, and every artifact
 * the demo site ships degrades onto `Inline`'s behaviour. `docs/plan/21-jobs.md`
 * calls that a supported configuration — the seam exists so that a page without
 * shared memory still runs — and a supported configuration nothing ever ran was
 * the gap this closes.
 *
 * Two things are asserted that the isolated run cannot ask: that the threaded
 * artifact is refused *outright* rather than quietly given an unshared memory,
 * and that what a consumer above the seam is left holding is {@link INLINE} by
 * name, at parallelism one, still reaching the same checksum.
 *
 * @param {number} requested How many workers the pool is asked for anyway.
 * @param {boolean} forceHostReady `?force-host-ready`, which is what makes the
 *   backend-by-name check below falsifiable: announcing the plain artifact
 *   anyway is the one thing that turns `Inline` into `Workers` here.
 */
async function runWithoutIsolation(requested, forceHostReady) {
  note('CONFIGURATION: no COOP/COEP — the origin the published site has');

  // The document first: every assertion below is a consequence of this one, and
  // on an isolated origin each would be a fact about something else entirely.
  check(
    globalThis.crossOriginIsolated === false,
    'the document is not cross-origin isolated'
  );
  // **The constructor is not the gate, and measuring that is what this check is
  // for.** `new WebAssembly.Memory({ shared: true })` *succeeds* on a
  // non-isolated origin in Chromium — the buffer it hands back even reports
  // `SharedArrayBuffer` as its constructor's name — while the
  // `SharedArrayBuffer` global itself is absent and `new SharedArrayBuffer(8)`
  // is a `ReferenceError`. So the two things asserted here are the two that
  // actually differ: the global the platform exposes, and the step
  // {@link WorkerHost.start} performs, which is handing that memory to a
  // `Worker`. The second is the one the backend dies on.
  check(
    typeof SharedArrayBuffer === 'undefined',
    'SharedArrayBuffer is not exposed on this origin'
  );
  let transfer = null;
  const probeUrl = URL.createObjectURL(
    new Blob(['self.onmessage = () => {};'], { type: 'text/javascript' })
  );
  const probe = new Worker(probeUrl);
  try {
    probe.postMessage(
      new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true })
    );
  } catch (error) {
    transfer = String(error);
  } finally {
    probe.terminate();
    URL.revokeObjectURL(probeUrl);
  }
  check(
    transfer !== null,
    'a shared memory cannot be handed to a Worker here' +
      (transfer ? ` — ${transfer}` : ', but one was')
  );

  // Refused outright, not degraded: the threaded artifact's memory is an
  // *import*, and there is nothing to hand it. An instance that came back would
  // mean the loader had quietly given it an unshared memory, which is the one
  // way this page could look healthy while every worker read a private heap.
  let refused = null;
  try {
    await WorkerHost.load(ARTIFACT);
  } catch (error) {
    refused = String(error);
  }
  check(
    refused !== null && refused.includes('cross-origin isolated'),
    'the threaded artifact is refused rather than instantiated' +
      (refused ? ` — ${refused}` : ', and it was instantiated')
  );

  // The artifact that does load here is the plain one, which is the shape every
  // artifact on the demo site has.
  const plain = await WorkerHost.load(PLAIN_ARTIFACT);
  const ex = plain.exports;
  check(
    plain.threaded === false && plain.refusal !== null,
    `the artifact shape the site publishes is refused workers${
      plain.refusal ? ` — ${plain.refusal}` : ' (it was not)'
    }`
  );
  const announced = forceHostReady
    ? ex.__crcbl_web_jobs_host_ready(requested + 1)
    : plain.announce(requested + 1);
  check(
    announced === 0,
    'announcing it answers 0, so nothing above the seam is told it has threads'
  );

  // The backend by name, which is the whole point of running this twice: the
  // isolation flag alone is satisfied by a pool that fell back, and a pool that
  // fell back is exactly what this configuration is supposed to produce.
  check(
    backendName(ex) === INLINE,
    `${INLINE} is what the artifact reports on an origin with no shared memory`
  );
  check(
    ex.gate_parallelism() === 1,
    `${INLINE}'s parallelism is one, so nothing asks for a second thread`
  );
  check(
    ex.gate_pool(requested) === 0,
    `a pool asked for ${requested} workers gets none rather than workers that never arrive`
  );
  check(
    ex.__crcbl_web_jobs_pending() === 0,
    'and queues nothing for a host that could not drain it'
  );

  // The degradation is a whole answer rather than a broken one, which is the
  // claim the seam is for.
  const expected = ex.gate_expected();
  check(
    ex.gate_run() === expected,
    `${INLINE} reaches the same checksum the threaded run has to reproduce`
  );
  check(ex.gate_threads() === 1, 'and every chunk ran on the driver itself');
  check(
    ex.gate_clobbered() === 0,
    'with no chunk finding its stack array changed underneath it'
  );
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

  // The other configuration, not another red check: see the file header.
  if (params.has('no-isolation')) {
    await runWithoutIsolation(requested, params.has('force-host-ready'));
    return;
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
  // Named rather than counted, in both runs of this page: `threaded()` and
  // `parallelism()` are the whole of what a consumer above the seam can see, and
  // a pool that fell back would satisfy a check on the isolation flag alone.
  check(
    backendName(ex) === WORKERS,
    `the ${WORKERS} backend is what the artifact reports once the host has spoken`
  );
  check(
    ex.gate_parallelism() === requested + 1,
    `the ${WORKERS} backend reported in with the worker count the host announced`
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
