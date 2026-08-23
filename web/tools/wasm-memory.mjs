// The limits a module imports `env.memory` with, decoded out of the binary.
//
// `WebAssembly.Module.imports()` reports the module, the name and the kind and
// stops there — whether a memory import is **shared** is not in it, and that is
// the property the whole worker story stands on. So the import section is read
// directly: WebAssembly core binary format, section id 2, whose entries end in
// a `limits` whose flag byte carries `0x01` for "has a maximum" and `0x02` for
// "shared".
//
// Two tools need it and therefore it lives here: `check-exports.mjs --threads`
// asserts the import is shared, and `worker-gate.mjs` has to construct a memory
// matching it before anything can be instantiated at all.

/**
 * One LEB128-encoded `u32`, and where the next byte is.
 *
 * @param {Uint8Array} bytes
 * @param {number} at
 * @returns {[number, number]}
 */
function readVarU32(bytes, at) {
  let value = 0;
  for (let shift = 0; shift < 35; shift += 7) {
    const byte = bytes[at];
    at += 1;
    value |= (byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) return [value >>> 0, at];
  }
  throw new Error('wasm-memory: LEB128 u32 longer than five bytes');
}

/**
 * @param {Uint8Array} bytes
 * @param {number} at start of a `limits`
 * @returns {number} the byte after it
 */
function skipLimits(bytes, at) {
  const flags = bytes[at];
  at += 1;
  [, at] = readVarU32(bytes, at);
  if ((flags & 0x01) !== 0) [, at] = readVarU32(bytes, at);
  return at;
}

/**
 * @param {Uint8Array} bytes the whole `.wasm` file
 * @returns {{ shared: boolean, minimum: number, maximum: number | undefined }
 *   | undefined} `undefined` when there is no `env.memory` import to describe.
 */
export function importedMemoryLimits(bytes) {
  const text = new TextDecoder();
  let at = 8; // the magic number and the version
  while (at < bytes.length) {
    const id = bytes[at];
    at += 1;
    let size;
    [size, at] = readVarU32(bytes, at);
    if (id !== 2) {
      at += size;
      continue;
    }
    let count;
    [count, at] = readVarU32(bytes, at);
    for (let i = 0; i < count; i += 1) {
      let length;
      [length, at] = readVarU32(bytes, at);
      const module = text.decode(bytes.subarray(at, at + length));
      at += length;
      [length, at] = readVarU32(bytes, at);
      const name = text.decode(bytes.subarray(at, at + length));
      at += length;
      const kind = bytes[at];
      at += 1;
      if (kind === 0x00) {
        [, at] = readVarU32(bytes, at); // a function: its type index
      } else if (kind === 0x01) {
        at = skipLimits(bytes, at + 1); // a table: a reftype, then limits
      } else if (kind === 0x02) {
        const flags = bytes[at];
        at += 1;
        let minimum;
        [minimum, at] = readVarU32(bytes, at);
        let maximum;
        if ((flags & 0x01) !== 0) [maximum, at] = readVarU32(bytes, at);
        if (module === 'env' && name === 'memory') {
          return { shared: (flags & 0x02) !== 0, minimum, maximum };
        }
      } else if (kind === 0x03) {
        at += 2; // a global: a valtype and a mutability byte
      } else {
        return undefined; // an import kind this decoder does not know
      }
    }
    return undefined;
  }
  return undefined;
}
