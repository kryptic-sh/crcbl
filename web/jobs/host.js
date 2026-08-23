// The host half of `crcbl_jobs::workers`, in a browser.
//
// A wasm module cannot start a thread. `crcbl_jobs::workers` therefore starts
// nothing: `Spawn::spawn` **queues** the request and waits for a page to drain
// that queue into real workers. This file is that page's half: {@link
// WorkerHost.announce} for the announcement, {@link WorkerHost.take} for the
// drain, {@link WorkerHost.start} for the workers. The five steps each worker
// then runs are `web/jobs/worker.js`, and their order is not ours to choose —
// it is the sequence `crates/crcbl-jobs/src/workers.rs`'s module docs specify,
// with the reason each step has to come where it does.
//
// `web/tools/worker-gate.mjs` is the same sequence under `node:worker_threads`.
// This is not a port of it: what node cannot show is whether a browser `Worker`
// accepts a structured-cloned `WebAssembly.Module`, whether a page's main thread
// survives driving a pool that parks its workers on `memory.atomic.wait32`, and
// whether the artifact can be given a shared memory at all — which is a property
// of the *document*, not of the build.
//
// THE ONE THING THIS FILE MUST NOT GET WRONG
//   `__crcbl_web_jobs_host_ready` is the whole of `Spawn::threaded`'s answer.
//   Call it, and every consumer above the seam believes this runtime has
//   threads: `Pool::with_workers` asks for workers, `Spawn::spawn` starts
//   queueing instead of refusing, and a `par_for` waits for chunks that are
//   never coming. So {@link WorkerHost.announce} calls it only when this host
//   can *actually* start a worker, which needs two things at once:
//
//     1. the artifact imports a **shared** `env.memory` — a worker attaches to
//        the memory the host constructed, and cannot attach to one the module
//        owns. `web/build.sh --threads` is what builds such an artifact; the
//        published site artifacts import nothing at all.
//     2. the document is **cross-origin isolated** — without it
//        `new WebAssembly.Memory({ shared: true })` throws, so there is no
//        memory to attach to. `web/tools/serve.mjs` sends the COOP/COEP pair
//        that earns it; GitHub Pages cannot, which is why nothing threaded is
//        published.
//
//   Either one missing and {@link WorkerHost.load} leaves the backend exactly
//   as it found it — `threaded()` false, `parallelism()` one, every spawn
//   refused — which is the degradation `crcbl_jobs::Inline` already had.

import { importedMemoryLimits } from '../tools/wasm-memory.mjs';

/**
 * The worker bring-up script, beside this file wherever the site put it.
 *
 * A `Worker` needs a URL, and `import.meta.url` is the only thing that knows
 * where this module was served from — the gate lays the two out together and a
 * demo site would have to as well.
 */
const WORKER_URL = new URL('./worker.js', import.meta.url);

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

/**
 * One spawn request the host has taken off the queue and not yet turned into a
 * worker.
 *
 * @typedef {{ handle: number, name: string, stackTop: number, tlsPtr: number }}
 *   Request
 */

/**
 * The two switches that make this host's own claims falsifiable.
 *
 * Skipping either step is a *silent* failure rather than a loud one — a worker
 * with no stack of its own shares the main thread's, and one that never
 * initialised its TLS reads whatever `__tls_base` was left pointing at — so the
 * only way to know the assertions about them can go red is to leave the step
 * out on purpose and watch them do it. `web/tools/worker-gate.mjs` carries the
 * same pair for the same reason.
 *
 * @typedef {{ skipStackPointer?: boolean, skipInitTls?: boolean }} BringUp
 */

/**
 * A loaded artifact, and the workers this page has started for it.
 *
 * Build one with {@link WorkerHost.load}; it is the only thing that can decide
 * whether the artifact is one a worker could ever attach to.
 */
export class WorkerHost {
  /** @type {WebAssembly.Module} */
  #module;
  /** @type {WebAssembly.Memory | undefined} */
  #memory;
  /** @type {Record<string, any>} */
  #exports;
  /** Whether this host can start a worker at all. See the file header. */
  #threaded;
  /** Why not, in a sentence, when it cannot. `null` when it can. */
  #refusal;
  /** @type {Worker[]} Every worker started, so the page can stop them. */
  #workers = [];
  /** How many workers have reported that they instantiated and entered. */
  #up = 0;
  /** @type {string[]} Anything a worker reported instead of coming up. */
  #errors = [];

  /**
   * @param {WebAssembly.Module} module
   * @param {WebAssembly.Memory | undefined} memory
   * @param {Record<string, any>} exports
   * @param {string | null} refusal
   */
  constructor(module, memory, exports, refusal) {
    this.#module = module;
    this.#memory = memory;
    this.#exports = exports;
    this.#refusal = refusal;
    this.#threaded = refusal === null;
  }

  /**
   * Fetches an artifact, gives it a shared memory if it wants one and this
   * document can make one, and instantiates it.
   *
   * The bytes are read rather than streamed because the *shared* flag on a
   * memory import is not in `WebAssembly.Module.imports()` — only the binary
   * says it, which is what `web/tools/wasm-memory.mjs` decodes. That is also why
   * this is not `web/tools/wasm-loader.js`: the site loader instantiates with an
   * empty import object on purpose, and a threaded artifact needs a memory it
   * cannot construct without reading the limits first.
   *
   * @param {RequestInfo | URL} source
   * @returns {Promise<WorkerHost>}
   */
  static async load(source) {
    const response = await fetch(source);
    if (!response.ok) {
      throw new Error(
        `crcbl jobs: ${response.status} ${response.statusText} fetching ${response.url}`
      );
    }
    const bytes = new Uint8Array(await response.arrayBuffer());
    const limits = importedMemoryLimits(bytes);
    const module = await WebAssembly.compile(bytes);

    if (limits === undefined) {
      // An artifact that owns its memory. Every published one is this, and a
      // worker cannot attach to it — so it is instantiated exactly as
      // `web/tools/wasm-loader.js` does and never announced.
      const instance = await WebAssembly.instantiate(module, {});
      return new WorkerHost(
        module,
        undefined,
        instance.exports,
        'the artifact owns its memory rather than importing a shared one, so a ' +
          'worker has nothing to attach to — build it with `web/build.sh --threads`'
      );
    }
    if (!limits.shared) {
      const memory = new WebAssembly.Memory({
        initial: limits.minimum,
        maximum: limits.maximum,
      });
      const instance = await WebAssembly.instantiate(module, {
        env: { memory },
      });
      return new WorkerHost(
        module,
        memory,
        instance.exports,
        'the artifact imports `env.memory` unshared, so a second instance would ' +
          'get a copy rather than the module’s heap — the build is missing ' +
          '`--shared-memory`'
      );
    }
    if (globalThis.crossOriginIsolated !== true) {
      // Not a degradation: a shared memory cannot be constructed here at all,
      // so there is no instance to hand back and nothing to degrade to.
      throw new Error(
        'crcbl jobs: this document is not cross-origin isolated, so ' +
          '`new WebAssembly.Memory({ shared: true })` is refused and a threaded ' +
          'artifact cannot be instantiated. Serve it through web/tools/serve.mjs, ' +
          'which sends the COOP/COEP pair.'
      );
    }
    const memory = new WebAssembly.Memory({
      initial: limits.minimum,
      maximum: limits.maximum,
      shared: true,
    });
    const instance = await WebAssembly.instantiate(module, { env: { memory } });
    return new WorkerHost(module, memory, instance.exports, null);
  }

  /** The instance's raw exports. */
  get exports() {
    return this.#exports;
  }

  /** The memory every instance shares, or `undefined` when the module owns it. */
  get memory() {
    return this.#memory;
  }

  /** Whether this host could start a worker for this artifact. */
  get threaded() {
    return this.#threaded;
  }

  /** Why it could not, or `null` when it can. */
  get refusal() {
    return this.#refusal;
  }

  /** How many workers have instantiated and called into `entry`. */
  get up() {
    return this.#up;
  }

  /** Whatever the workers reported instead of coming up. */
  get errors() {
    return [...this.#errors];
  }

  /**
   * Tells the backend it has a host, and how many workers the machine has.
   *
   * **The one call that must not be made on faith.** It is refused here rather
   * than made conditionally at every call site, because a host that cannot
   * start a worker and says otherwise turns `Spawn::threaded` into a lie — see
   * the file header.
   *
   * @param {number} [concurrency] Defaults to `navigator.hardwareConcurrency`.
   * @returns {number} the worker count the backend recorded, or `0` if this
   *   host refused to announce.
   */
  announce(concurrency) {
    if (!this.#threaded) return 0;
    const asked = concurrency ?? navigator.hardwareConcurrency ?? 1;
    return this.#exports.__crcbl_web_jobs_host_ready(asked);
  }

  /**
   * Takes every queued spawn request off the backend and prepares it.
   *
   * The three calls per request are the host's half of the contract: the name
   * for the worker's label, one stack, and one TLS block sized from the
   * artifact's own `__tls_size`/`__tls_align` globals rather than from anything
   * assumed here.
   *
   * @returns {Request[]} oldest first, empty when nothing was waiting.
   */
  take() {
    const ex = this.#exports;
    /** @type {Request[]} */
    const requests = [];
    if (!this.#threaded) return requests;
    while (ex.__crcbl_web_jobs_pending() > 0) {
      const handle = ex.__crcbl_web_jobs_take();
      if (handle === 0) break;
      const name = readString(
        /** @type {WebAssembly.Memory} */ (this.#memory),
        ex.__crcbl_web_jobs_name_ptr(),
        ex.__crcbl_web_jobs_name_len()
      );
      requests.push({
        handle,
        name,
        stackTop: ex.__crcbl_web_jobs_stack_alloc(),
        tlsPtr: ex.__crcbl_web_jobs_tls_alloc(
          this.global('__tls_size'),
          this.global('__tls_align')
        ),
      });
    }
    return requests;
  }

  /**
   * Starts one `Worker` per request and hands it everything it needs to bring
   * itself up.
   *
   * The module travels as a structured-cloned `WebAssembly.Module` and the
   * memory as the `SharedArrayBuffer` behind it, so the worker instantiates the
   * *same* module against the *same* heap. `bringUp` is the gate's two red
   * switches and is empty everywhere else.
   *
   * @param {Request[]} requests
   * @param {BringUp} [bringUp]
   */
  start(requests, bringUp = {}) {
    for (const request of requests) {
      const worker = new Worker(WORKER_URL, {
        type: 'module',
        name: request.name,
      });
      worker.addEventListener('message', (event) => {
        if (event.data?.up) this.#up += 1;
        if (event.data?.error) this.#errors.push(String(event.data.error));
      });
      worker.addEventListener('error', (event) =>
        this.#errors.push(event.message ?? String(event))
      );
      worker.postMessage({
        module: this.#module,
        memory: this.#memory,
        handle: request.handle,
        stackTop: request.stackTop,
        tlsPtr: request.tlsPtr,
        skipStackPointer: bringUp.skipStackPointer === true,
        skipInitTls: bringUp.skipInitTls === true,
      });
      this.#workers.push(worker);
    }
  }

  /**
   * The value of one of the artifact's exported wasm globals.
   *
   * @param {string} name
   * @returns {number}
   */
  global(name) {
    return /** @type {WebAssembly.Global} */ (this.#exports[name]).value;
  }

  /**
   * Stops every worker this host started.
   *
   * A pool worker's closure is its whole loop and never returns, so there is
   * nothing to await: terminating them is the whole of the teardown.
   */
  stop() {
    for (const worker of this.#workers) worker.terminate();
    this.#workers = [];
  }
}
