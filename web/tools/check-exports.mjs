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
//   4. The module imports nothing outside `wasm-bindgen`'s own glue. The
//      engine's ABI is exports-plus-polling by design; an `extern "C" { fn … }`
//      that crept in somewhere would otherwise be a `LinkError` in a browser.
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
// Usage:
//   node web/tools/check-exports.mjs <path-to.wasm> --sample <name> [--quiet]

import { readFile, readdir, stat } from 'node:fs/promises';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

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

async function main() {
  const args = process.argv.slice(2);
  const quiet = args.includes('--quiet');
  const positional = args.filter((a) => !a.startsWith('--'));
  const sampleFlag = args.indexOf('--sample');
  const wasmPath = positional.find((a) => a !== args[sampleFlag + 1]);
  const sample = sampleFlag >= 0 ? args[sampleFlag + 1] : undefined;
  if (!wasmPath || !sample) {
    console.error(
      'usage: node web/tools/check-exports.mjs <path-to.wasm> --sample <name> [--quiet]'
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

  const module = new WebAssembly.Module(await readFile(wasmPath));
  const exported = new Set(
    WebAssembly.Module.exports(module).map((e) => e.name)
  );
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

  if (!exported.has('memory')) {
    failures.push(
      'the artifact does not export `memory`; every ABI here is an offset into it'
    );
  }

  // wasm-bindgen rewrites the placeholder module to the name of the glue file it
  // emits, so the accepted set is "whatever single module the glue owns" plus
  // the pre-bindgen placeholders (a raw `cargo build` artifact).
  const ALLOWED_IMPORT_MODULES = new Set([
    '__wbindgen_placeholder__',
    '__wbindgen_externref_xform__',
    './' + basename(wasmPath).replace(/_bg\.wasm$/, '_bg.js'),
  ]);
  const strayModules = [...new Set(imports.map((i) => i.module))].filter(
    (m) => !ALLOWED_IMPORT_MODULES.has(m)
  );
  if (strayModules.length > 0) {
    failures.push(
      `the artifact imports from ${strayModules.length} unexpected module(s): ${strayModules.join(', ')}.\n` +
        "    The engine ABI is exports-plus-polling; the only imports allowed are wasm-bindgen's\n" +
        `    own glue for \`wgpu\`. See \`apps/${sample}/src/web.rs\`.`
    );
  }

  if (!quiet) {
    console.log(`artifact:        ${wasmPath}  (sample: ${sample})`);
    console.log(
      `exports:         ${exported.size} total, ${[...exported].filter((n) => n.startsWith('__crcbl_')).length} __crcbl_*`
    );
    console.log(
      `imports:         ${imports.length} from ${[...new Set(imports.map((i) => i.module))].join(', ')}`
    );
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
