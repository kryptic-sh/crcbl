// The loader a demo gets on a threaded site: the same contract as
// `wasm-loader.js`, over an artifact a Web Worker can attach to.
//
// `web/build.sh --threads` copies this beside each demo as `<lib>.js`, exactly
// where the default build copies `web/tools/wasm-loader.js`, so the pages,
// `web/engine/demo.js` and every `web/demos/<name>/main.js` are the same files
// on both sites and neither has a `cfg` for which one it is on. What differs is
// only what happens between `fetch` and the exports coming back.
//
// WHY THERE ARE TWO LOADERS AND NOT ONE WITH A BRANCH.
//   `wasm-loader.js` instantiates with an **empty import object** and streams
//   the compile, and that is a property of the published site worth keeping
//   rather than a detail: an artifact that imports nothing cannot be handed
//   anything, and `web/tools/check-exports.mjs` fails a site build where one
//   grew an import. A threaded artifact is the other shape — it imports
//   `env.memory` because a shared memory is one the *host* constructs — and the
//   limits it wants are not in `WebAssembly.Module.imports()`, so choosing
//   between them means reading the bytes before compiling. Folding that into the
//   published loader would make every visitor pay a buffered compile for a mode
//   that can never reach them: GitHub Pages sends no COOP/COEP pair, so a
//   `SharedArrayBuffer` cannot exist there and nothing threaded is published.
//
// THE ANNOUNCE IS NOT THIS FILE'S DECISION. `__crcbl_web_jobs_host_ready` is
// the whole of `Spawn::threaded`'s answer, and announcing for an artifact no
// worker can attach to makes the backend claim threads it can never get. That
// judgement lives once, in `web/engine/jobs.js`'s `WorkerHost`, which refuses
// both the artifact that owns its memory and the document that cannot construct
// a shared one. This file calls `announce` and takes `0` for an answer.
//
// WHAT DRIVES THE DRAIN. A pool's workers are spawned when the game builds one,
// which is inside `boot()` — long after this returns — so there is nothing to
// drain at load time. The queue is polled instead, on an interval, which costs
// one integer call per tick and needs no hook in `web/engine/demo.js`'s frame
// loop. `crcbl_jobs::workers` is built for exactly this: `Spawn::spawn` queues
// and returns, and a `par_for` runs its chunks on the calling thread until a
// worker arrives to steal one.

/** How often to look for spawn requests, in milliseconds. */
const DRAIN_INTERVAL_MS = 16;

/**
 * How many workers to announce when `?workers=` does not say.
 *
 * Not `navigator.hardwareConcurrency`: a pool sizes itself to what it is told,
 * so on a 32-thread machine that is 31 `Worker`s each holding a megabyte of
 * stack that `crcbl_jobs::workers` leaks on purpose. This is a page deciding how
 * much of the machine it wants, which is the number the ABI asks for.
 */
const DEFAULT_WORKERS = 4;

/** @type {Promise<Record<string, any>> | undefined} */
let started;

/**
 * Instantiates the artifact and hands back its exports.
 *
 * The contract `web/engine/demo.js` is written against, unchanged: a promise for
 * the instance's raw exports, `memory` included, memoised so a second call hands
 * back the same instance.
 *
 * @param {RequestInfo | URL} [source] Where the module is. Defaults to the
 *   `_bg.wasm` beside this file.
 * @returns {Promise<Record<string, any>>} the instance's raw exports.
 */
export default function init(source) {
  started ??= instantiate(
    source ?? new URL(import.meta.url.replace(/\.js$/, '_bg.wasm'))
  );
  return started;
}

/**
 * @param {RequestInfo | URL} source
 * @returns {Promise<Record<string, any>>}
 */
async function instantiate(source) {
  // Imported by URL rather than by specifier, and dynamically, for this file's
  // own reason: it is *copied* two directories deeper than it lives, so a
  // static `'../../engine/jobs.js'` would be resolved against the wrong path in
  // the repository and the right one only by accident. `import.meta.url` is the
  // only thing that knows where this module was served from — the same trick
  // that finds the artifact above.
  const { WorkerHost } = await import(
    new URL('../../engine/jobs.js', import.meta.url).href
  );

  const host = await WorkerHost.load(source);
  if (!host.threaded) {
    // The degradation, not a failure: `crcbl_jobs::workers` answers
    // `threaded()` false until a host announces, and nothing has. The game runs
    // every `par_for` chunk on the calling thread, which is what the published
    // site does and what `crcbl_jobs::Inline` always did.
    console.warn(`crcbl: no worker threads — ${host.refusal}`);
    return host.exports;
  }

  const asked = requestedWorkers();
  // THE RED SWITCH, and the only one this file has. Skipping the announcement
  // leaves `Spawn::threaded()` false, so the backend refuses every spawn and the
  // game runs its `par_for` chunks on the calling thread — which is precisely
  // the outcome `web/run-horde-threads-e2e.sh` exists to rule out, and the only
  // way to show that its assertion can go red. Nothing but that gate sets it.
  const announced = params().has('no-host-ready') ? 0 : host.announce(asked);
  console.log(
    `crcbl: worker threads announced (${announced}), draining spawns every ` +
      `${DRAIN_INTERVAL_MS} ms`
  );

  const drain = setInterval(() => {
    const requests = host.take();
    if (requests.length > 0) host.start(requests);
  }, DRAIN_INTERVAL_MS);

  // The page's readout, and what `web/tools/horde-threads-e2e.mjs` reads to say
  // *why* a run had no second thread: an announcement that never happened, a
  // queue nothing was ever put on, or workers that refused to come up. The
  // counters that say whether the sim actually used them belong to the sample —
  // `__crcbl_horde_sim_threads` — because nothing out here can see a chunk run.
  globalThis.crcblWorkers = {
    announced,
    asked,
    stop: () => clearInterval(drain),
    stats: () => ({
      announced,
      asked,
      up: host.up,
      errors: host.errors,
    }),
  };

  // A threaded module does **not** export its memory — the host owns it — so it
  // is put back where every caller already looks for it. A fresh object rather
  // than an assignment: an instance's exports are not extensible.
  return { ...host.exports, memory: host.memory };
}

/**
 * How many workers `?workers=N` asked for, or {@link DEFAULT_WORKERS}.
 *
 * @returns {number}
 */
function requestedWorkers() {
  const asked = params().get('workers');
  if (asked === null) return DEFAULT_WORKERS;
  const count = Number(asked);
  if (!Number.isInteger(count) || count < 1) {
    throw new Error(`crcbl: ?workers=${asked} is not a worker count`);
  }
  return count;
}

/**
 * This page's query string.
 *
 * @returns {URLSearchParams}
 */
function params() {
  return new URLSearchParams(globalThis.location?.search ?? '');
}
