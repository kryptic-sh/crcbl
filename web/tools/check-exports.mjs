#!/usr/bin/env node
// The symbol-level contract check between the Rust half and the JS half.
//
// There is no browser in CI, so nothing here runs the game. What *can* be
// checked, and is:
//
//   1. Every `__crcbl_*` the Rust sources declare with `#[unsafe(no_mangle)]`
//      is actually exported by the built artifact. A `#[no_mangle]` symbol in a
//      dependency rlib is not guaranteed to survive into a `cdylib` — it does
//      today, and this is what would notice the day it stops.
//   2. Every `__crcbl_*` the shim calls exists in that export list. A typo in a
//      symbol name is otherwise a `TypeError` the first time a player presses a
//      key.
//   3. `memory` is exported, because every buffer in every one of these ABIs is
//      an offset into it.
//   4. **The module imports nothing.** Not "nothing outside the glue" — nothing
//      at all. The engine's ABI is exports-plus-polling by design, and since
//      `crcbl-wgpu` stopped being a wasm dependency of the umbrella there is
//      nothing left in a browser build reaching `web-sys`. An `extern "C" { fn
//      … }` that crept in somewhere would otherwise be a `LinkError` in a
//      browser; here it is a failed build.
//
// It reports the reverse direction too — exports the shim never calls — as
// information rather than failure: `__crcbl_web_pointer_wheel` is unused by
// breakout and that is fine, but a whole ABI going unused usually means a shim
// forgot a step.
//
// A sample owns its own `__crcbl_<sample>_*` prefix, so the check is run once
// per sample and scoped to it: another sample's Rust sources and another
// sample's shim are not this artifact's contract. Without that scoping the
// second sample in the repo makes the first one's check fail, which is exactly
// what happened when flappy landed.
//
// `--threads` checks the *other* artifact — the one `web/build.sh --threads`
// builds, which a Web Worker can attach to. Rules 1 and 2 are unchanged there;
// rules 3 and 4 are replaced, because a threaded module does not own its memory
// and therefore does not export it:
//
//   3'. `env.memory` is **imported**, and imported as a **shared** memory. A
//       worker cannot attach to a memory the module owns — only to one the host
//       constructs and every instance imports.
//   4'. That import is the **only** one. Rule 4 is narrowed rather than
//       relaxed: the memory is the one thing that cannot be an export, and
//       anything else crossing into wasm fails here exactly as it does above.
//
//   plus the symbols a worker brings itself up on: `__wasm_init_tls` as a
//   function, `__tls_base`/`__tls_size`/`__tls_align`/`__stack_pointer` as
//   globals, and `__stack_pointer` **writable from JS** — checked by writing
//   it, since a worker that cannot set its own stack runs on the main thread's.
//   Every one of those names a link argument, because that is what a reader can
//   act on.
//
// Usage:
//   node web/tools/check-exports.mjs <path-to.wasm> --sample <name> [--quiet]
//   node web/tools/check-exports.mjs <path-to.wasm> --sample <name> --threads

import { readFile, readdir, stat } from 'node:fs/promises';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { importedMemoryLimits } from './wasm-memory.mjs';

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** Where `#[unsafe(no_mangle)] pub … extern "C" fn __crcbl_…` may live. */
const RUST_ROOTS = [join(REPO, 'crates'), join(REPO, 'apps')];

/** Where the shared half of the shim lives. The per-demo half is added by
 * `--sample`, because one demo's `main.js` says nothing about another's
 * artifact. */
const JS_SHARED = join(REPO, 'web', 'engine');

/**
 * The declaration form the engine uses for every wasm export.
 *
 * `#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]` on the engine crates
 * and a plain `#[unsafe(no_mangle)]` in the sample; both are followed, possibly
 * after doc comments and other attributes, by an `extern "C" fn __crcbl_…`.
 */
const RUST_EXPORT = /\bextern\s+"C"\s+fn\s+(__crcbl_[A-Za-z0-9_]+)/g;

/** How the shim names an export: `exports.__crcbl_x` or `ex.__crcbl_x`. */
const JS_USE = /\.(__crcbl_[A-Za-z0-9_]+)\b/g;

/**
 * @param {string} root
 * @param {(name: string) => boolean} keep
 * @returns {Promise<string[]>}
 */
async function walk(root, keep) {
  const found = [];
  let entries;
  try {
    entries = await readdir(root, { withFileTypes: true });
  } catch {
    return found;
  }
  for (const entry of entries) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === 'target' || entry.name === 'node_modules') continue;
      found.push(...(await walk(path, keep)));
    } else if (keep(entry.name)) {
      found.push(path);
    }
  }
  return found;
}

/**
 * @param {string[]} roots
 * @param {(name: string) => boolean} keep
 * @param {RegExp} pattern
 * @returns {Promise<Map<string, string[]>>} symbol → files that mention it
 */
async function collect(roots, keep, pattern) {
  /** @type {Map<string, string[]>} */
  const symbols = new Map();
  for (const root of roots) {
    for (const file of await walk(root, keep)) {
      const text = await readFile(file, 'utf8');
      for (const match of text.matchAll(pattern)) {
        const where = symbols.get(match[1]) ?? [];
        if (!where.includes(file)) where.push(file);
        symbols.set(match[1], where);
      }
    }
  }
  return symbols;
}

/**
 * @param {string} label
 * @param {Iterable<string>} names
 */
function list(label, names) {
  const sorted = [...names].sort();
  if (sorted.length === 0) return;
  console.log(`\n${label} (${sorted.length}):`);
  for (const name of sorted) console.log(`  ${name}`);
}

/**
 * Whether `file` belongs to a sample other than `sample`.
 *
 * Attribution by path rather than by symbol name: it needs no naming
 * convention, and it is exact. `apps/breakout/src/web.rs` declares breakout's
 * exports and nothing else's, whatever they happen to be called.
 *
 * @param {string} file
 * @param {string} sample
 */
function belongsToAnotherSample(file, sample) {
  const match = file.slice(REPO.length + 1).match(/^apps[/\\]([^/\\]+)[/\\]/);
  return match !== null && match[1] !== sample;
}

/** The globals a worker reads or sets while bringing itself up. Each is
 * exported by a `-C link-arg=--export=<name>` of its own. */
const WORKER_GLOBALS = [
  '__tls_base',
  '__tls_size',
  '__tls_align',
  '__stack_pointer',
];

/**
 * What a Web Worker needs from the artifact, and the link argument behind each.
 *
 * Nothing here can be inferred from the non-threaded checks: that build imports
 * nothing, exports its memory, and has none of these symbols, and it still
 * builds and runs perfectly well single-threaded. Every assertion below is
 * therefore about a flag someone can drop without anything else noticing.
 *
 * @param {WebAssembly.Module} module
 * @param {Uint8Array} bytes the same artifact, unparsed
 * @param {WebAssembly.ModuleImportDescriptor[]} imports
 * @param {WebAssembly.ModuleExportDescriptor[]} exportList
 * @returns {Promise<{ failures: string[], notes: string[] }>}
 */
async function workerSurface(module, bytes, imports, exportList) {
  /** @type {string[]} */
  const failures = [];
  /** @type {string[]} */
  const notes = [];

  const isMemoryImport = (
    /** @type {WebAssembly.ModuleImportDescriptor} */ i
  ) => i.module === 'env' && i.name === 'memory' && i.kind === 'memory';

  // **`env.memory` is the only import allowed, and it is required.** The
  // non-threaded rule — nothing at all — is narrowed here rather than lifted:
  // the memory is the one thing a shared build cannot express as an export,
  // and anything else crossing into wasm is the same failure it is there.
  const stray = imports.filter((i) => !isMemoryImport(i));
  if (stray.length > 0) {
    failures.push(
      `the artifact imports ${stray.length} thing(s) besides \`env.memory\`:\n` +
        stray.map((i) => `    ${i.module}.${i.name}  (${i.kind})`).join('\n') +
        '\n    A threaded artifact imports its memory and nothing else.'
    );
  }

  const limits = importedMemoryLimits(bytes);
  if (!imports.some(isMemoryImport)) {
    failures.push(
      'the artifact does not import `env.memory`, so no worker can attach to it.\n' +
        '    A module that owns its memory cannot share that memory with a second\n' +
        '    instance; only a memory the host built and every instance imports is\n' +
        '    shared. Add `-C link-arg=--import-memory`.'
    );
  } else if (limits === undefined) {
    failures.push(
      'the import section does not decode: `env.memory` is in the JS view of the\n' +
        '    imports but not in the bytes this check reads. That is a bug in\n' +
        '    `importedMemoryLimits` in `web/tools/wasm-memory.mjs`, not in the\n' +
        '    artifact.'
    );
  } else if (!limits.shared) {
    failures.push(
      'the artifact imports `env.memory`, but not as a **shared** memory.\n' +
        '    An unshared import gives each worker its own heap, which is not\n' +
        '    threading. Add `-C link-arg=--shared-memory` (and the\n' +
        '    `-C link-arg=--max-memory=…` a shared memory has to declare).'
    );
  }

  const kindOf = new Map(exportList.map((e) => [e.name, e.kind]));
  if (kindOf.get('__wasm_init_tls') !== 'function') {
    failures.push(
      `\`__wasm_init_tls\` is ${kindOf.get('__wasm_init_tls') ?? 'not exported'}, not a function.\n` +
        '    A worker calls it before it runs any Rust; without the call the first\n' +
        '    thread-local access traps. Add\n' +
        '    `-C link-arg=--export=__wasm_init_tls`.'
    );
  }
  for (const name of WORKER_GLOBALS) {
    if (kindOf.get(name) !== 'global') {
      failures.push(
        `\`${name}\` is ${kindOf.get(name) ?? 'not exported'}, not a global.\n` +
          `    Add \`-C link-arg=--export=${name}\`.`
      );
    }
  }

  // **Written, not read.** A global's mutability is not in the JS export
  // descriptor at all, and the failure this catches — a build without
  // `+mutable-globals` — is precisely one where the symbol is present and the
  // assignment does not take. `WebAssembly.Global`'s setter throws on an
  // immutable global, so both halves are covered by trying it.
  //
  // Instantiating is also the only thing that proves the module *runs* against
  // a memory it does not own: its data segments are copied in by the start
  // function, out of the `SharedArrayBuffer` constructed right here.
  if (limits !== undefined && kindOf.get('__stack_pointer') === 'global') {
    const memory = new WebAssembly.Memory({
      initial: limits.minimum,
      maximum: limits.maximum,
      shared: limits.shared,
    });
    // A `LinkError` here is the artifact's failure, not this tool's: an import
    // it cannot satisfy, or a memory whose shared-ness does not match. Caught
    // and reported, because a stack trace out of a gate says nothing about
    // which flag produced it.
    let instance;
    try {
      instance = await WebAssembly.instantiate(module, { env: { memory } });
    } catch (error) {
      return {
        failures: [
          ...failures,
          `the artifact does not instantiate against the memory it asks for: ${error}`,
        ],
        notes,
      };
    }
    const stackPointer = /** @type {WebAssembly.Global} */ (
      instance.exports.__stack_pointer
    );
    const before = stackPointer.value;
    // Any value that is not the one already there: a global that silently
    // ignores the write must not read as one that took it.
    const probe = before - 16;
    let wrote = true;
    try {
      stackPointer.value = probe;
    } catch (error) {
      wrote = false;
      failures.push(
        `\`__stack_pointer\` cannot be written from JS: ${error}\n` +
          '    Every worker sets its own stack before it runs anything, so a\n' +
          '    read-only one is unusable. It is exported mutable by\n' +
          '    `-C target-feature=+mutable-globals`.'
      );
    }
    if (wrote && stackPointer.value !== probe) {
      failures.push(
        `\`__stack_pointer\` did not take a write: set ${probe}, read back ${stackPointer.value}.`
      );
    }
    notes.push(
      `memory:          env.memory, ${limits.shared ? 'shared' : 'NOT shared'}, ` +
        `${limits.minimum}..${limits.maximum ?? 'unbounded'} pages` +
        `, buffer is a ${memory.buffer.constructor.name}`
    );
    // `?.` on every one of these: the report runs on a failing artifact too,
    // and a missing export has already been reported by name above. Reading it
    // here regardless turns that named failure into a stack trace.
    const global = (/** @type {string} */ name) =>
      /** @type {WebAssembly.Global | undefined} */ (instance.exports[name])
        ?.value ?? 'absent';
    notes.push(
      `worker surface:  __tls_size=${global('__tls_size')}, ` +
        `__tls_align=${global('__tls_align')}, ` +
        `__stack_pointer ${wrote ? 'writable' : 'READ-ONLY'}`
    );
  }

  return { failures, notes };
}

async function main() {
  const args = process.argv.slice(2);
  const quiet = args.includes('--quiet');
  const threads = args.includes('--threads');
  const positional = args.filter((a) => !a.startsWith('--'));
  const sampleFlag = args.indexOf('--sample');
  const wasmPath = positional.find((a) => a !== args[sampleFlag + 1]);
  const sample = sampleFlag >= 0 ? args[sampleFlag + 1] : undefined;
  if (!wasmPath || !sample) {
    console.error(
      'usage: node web/tools/check-exports.mjs <path-to.wasm> --sample <name> [--quiet] [--threads]'
    );
    process.exit(2);
  }
  try {
    await stat(wasmPath);
  } catch {
    console.error(`check-exports: no such artifact: ${wasmPath}`);
    console.error('build it first: web/build.sh');
    process.exit(2);
  }

  const bytes = await readFile(wasmPath);
  const module = new WebAssembly.Module(bytes);
  const exportList = WebAssembly.Module.exports(module);
  const exported = new Set(exportList.map((e) => e.name));
  const imports = WebAssembly.Module.imports(module);

  const allDeclared = await collect(
    RUST_ROOTS,
    (n) => n.endsWith('.rs'),
    RUST_EXPORT
  );
  const declared = new Map(
    [...allDeclared].filter(([, files]) =>
      files.some((file) => !belongsToAnotherSample(file, sample))
    )
  );
  const used = await collect(
    [JS_SHARED, join(REPO, 'web', 'demos', sample)],
    (n) => n.endsWith('.js'),
    JS_USE
  );

  /** @type {string[]} */
  const failures = [];

  const missingFromArtifact = [...declared.keys()].filter(
    (n) => !exported.has(n)
  );
  if (missingFromArtifact.length > 0) {
    failures.push(
      `${missingFromArtifact.length} symbol(s) declared in Rust are not exported by ${basename(wasmPath)}:\n` +
        missingFromArtifact
          .map(
            (n) =>
              `    ${n}  (${(declared.get(n) ?? []).map((f) => f.slice(REPO.length + 1)).join(', ')})`
          )
          .join('\n')
    );
  }

  const missingForShim = [...used.keys()].filter((n) => !exported.has(n));
  if (missingForShim.length > 0) {
    failures.push(
      `${missingForShim.length} symbol(s) the shim calls do not exist in the artifact:\n` +
        missingForShim
          .map(
            (n) =>
              `    ${n}  (${(used.get(n) ?? []).map((f) => f.slice(REPO.length + 1)).join(', ')})`
          )
          .join('\n')
    );
  }

  /** @type {string[]} */
  let notes = [];
  if (threads) {
    // The two rules below are about the artifact the site ships. A threaded
    // one answers a different question — see the header — and the checks above
    // this point, which are about `__crcbl_*` symbols, apply to both.
    const surface = await workerSurface(module, bytes, imports, exportList);
    failures.push(...surface.failures);
    notes = surface.notes;
  } else {
    if (!exported.has('memory')) {
      failures.push(
        'the artifact does not export `memory`; every ABI here is an offset into it'
      );
    }

    // **Empty, and that is the assertion.** This used to hold the two
    // `__wbindgen_*` placeholders and the glue module wasm-bindgen rewrote them
    // to, because `wgpu` reached WebGPU through `web-sys` and every artifact
    // imported ~340 functions from it. `crcbl-wgpu` is a
    // `cfg(not(target_arch = "wasm32"))` dependency of the umbrella now, nothing
    // in a browser build reaches `web-sys`, and the check therefore says what the
    // engine always meant: **this module imports nothing at all.** An import of
    // any kind, from any module, fails here.
    const ALLOWED_IMPORT_MODULES = new Set();
    const strayModules = [...new Set(imports.map((i) => i.module))].filter(
      (m) => !ALLOWED_IMPORT_MODULES.has(m)
    );
    if (strayModules.length > 0) {
      failures.push(
        `the artifact imports from ${strayModules.length} module(s): ${strayModules.join(', ')}.\n` +
          '    The engine ABI is exports-plus-polling, so a browser artifact imports nothing.\n' +
          `    See \`apps/${sample}/src/web.rs\`.`
      );
    }
  }

  if (!quiet) {
    console.log(`artifact:        ${wasmPath}  (sample: ${sample})`);
    console.log(
      `exports:         ${exported.size} total, ${[...exported].filter((n) => n.startsWith('__crcbl_')).length} __crcbl_*`
    );
    console.log(
      `imports:         ${imports.length} from ${[...new Set(imports.map((i) => i.module))].join(', ')}`
    );
    for (const note of notes) console.log(note);
    console.log(`declared in Rust: ${declared.size}`);
    console.log(`called by the shim: ${used.size}`);
    list(
      'exported but never called by the shim',
      [...declared.keys()].filter((n) => !used.has(n))
    );
  }

  if (failures.length > 0) {
    console.error('\ncheck-exports: FAILED');
    for (const failure of failures) console.error(`  - ${failure}`);
    process.exit(1);
  }
  console.log('\ncheck-exports: OK');
}

await main();
