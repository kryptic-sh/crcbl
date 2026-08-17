// The module every page imports as `init`, copied beside each artifact as
// `<lib>.js` by `web/build.sh` and `web/run-render-harness-e2e.sh`.
//
// WHY THIS IS HAND-WRITTEN AND NOT `wasm-bindgen`'s OUTPUT. `crcbl-wgpu` was
// the only thing in a browser build reaching `web-sys`, and it is a
// `cfg(not(target_arch = "wasm32"))` dependency of the umbrella now — so the
// artifacts import zero functions and no `wasm-bindgen` crate is linked into
// them at all. That is not merely "the tool has nothing to do": with none of
// its runtime intrinsics present the CLI **refuses**, with
// `failed to find intrinsics to enable ‘clone_ref’ function`, rather than
// emitting a passthrough. Keeping the step was not an option; this is what
// replaced it.
//
// THE CONTRACT IS THE ONE THE GLUE HAD, unchanged, because every caller —
// `web/engine/demo.js`, `web/probe/main.js`, `web/harness/main.js` — was
// written against it: a default export returning a promise for the instance's
// raw exports object, `memory` included, memoised so a second call hands back
// the same instance rather than a second one.
//
// ONE COPY SERVES EVERY ARTIFACT. The module is the file beside this one with
// `_bg.wasm` in place of `.js`, which is the layout `wasm-bindgen --target web`
// produced and which every page already imports by name — `crcbl_breakout.js`
// loads `crcbl_breakout_bg.wasm`. So the build copies this file per demo
// instead of generating a variant of it.

/** @type {Promise<Record<string, any>> | undefined} */
let started;

/**
 * Instantiates the artifact and hands back its exports.
 *
 * @param {RequestInfo | URL} [source] Where the module is. Defaults to the
 *   `_bg.wasm` beside this file, which is what every page relies on.
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
  const response = await fetch(source);
  if (!response.ok) {
    // A 404 here otherwise surfaces as `expected magic word 00 61 73 6d`,
    // which reads like a corrupt build rather than a missing file.
    throw new Error(
      `crcbl: ${response.status} ${response.statusText} fetching ${response.url}`
    );
  }
  // The import object is empty on purpose. The engine's ABI is
  // exports-plus-polling — nothing crosses into wasm except through a call on
  // an export — and `web/tools/check-exports.mjs` fails the build if an import
  // ever appears, so an artifact that needed one would not reach a page.
  const imports = {};
  // `instantiateStreaming` needs `Content-Type: application/wasm`;
  // `web/tools/serve.mjs` sends it, and so does GitHub Pages. The buffered path
  // is for a host that does not, where streaming throws a `TypeError` naming
  // the MIME type and nothing else.
  const type = response.headers.get('content-type') ?? '';
  const { instance } = type.startsWith('application/wasm')
    ? await WebAssembly.instantiateStreaming(response, imports)
    : await WebAssembly.instantiate(await response.arrayBuffer(), imports);
  return /** @type {Record<string, any>} */ (instance.exports);
}
