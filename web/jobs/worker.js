// The worker half of the `crcbl_jobs::workers` bring-up.
//
// Five steps, and their order is the contract
// `crates/crcbl-jobs/src/workers.rs` documents rather than a style choice:
//
//   1. instantiate the *same* module against the *same* `env.memory` — a
//      second module, or a memory of its own, is a thread with no access to the
//      heap the work lives on;
//   2. write the stack top the host allocated into `__stack_pointer`. Wasm
//      globals are per-instance, so this one starts wherever the linker put the
//      main thread's stack, and **leaving it there is silent**: a closure that
//      merely allocates returns the right answer while the two threads write
//      over each other's frames;
//   3. call `__wasm_init_tls` with the block the host allocated. Skipping it is
//      silent too, in a way that surprised this repository once already — in
//      the artifact `web/tools/worker-gate.mjs` measures, `__tls_base` is left
//      at zero and every worker's thread-locals alias one address, read and
//      written without a trap;
//   4. tell the page it is up, *before* entering, because entry does not
//      return;
//   5. enter. For a pool worker the closure is the worker's whole loop.
//
// `skipStackPointer` and `skipInitTls` leave step 2 or step 3 out. They exist
// so that the gate's assertions about those two steps can be shown to go red,
// which is the only reason to trust them when they are green; nothing but the
// gate ever sets them.

self.addEventListener('message', (event) => {
  const {
    module,
    memory,
    handle,
    stackTop,
    tlsPtr,
    skipStackPointer,
    skipInitTls,
  } = event.data;
  let exports;
  try {
    exports = new WebAssembly.Instance(module, { env: { memory } }).exports;
    if (!skipStackPointer) exports.__stack_pointer.value = stackTop;
    if (!skipInitTls) exports.__wasm_init_tls(tlsPtr);
  } catch (error) {
    // Reported rather than thrown: an exception here reaches the page as an
    // `error` event with no detail on some engines, and the gate's verdict
    // depends on knowing which of the five steps refused.
    self.postMessage({ error: `bringing the worker up: ${error}` });
    return;
  }
  self.postMessage({ up: true });
  try {
    // Does not return for a pool worker. Anything it does throw is the work's,
    // not the bring-up's, and the page needs to see it either way.
    const ran = exports.__crcbl_web_jobs_entry(handle);
    self.postMessage({ up: false, ran });
  } catch (error) {
    self.postMessage({ error: `running the work: ${error}` });
  }
});
