// The JavaScript half of `crcbl-webgpu`'s command stream: a buffer in, plain
// command objects out.
//
// `crates/crcbl-webgpu/src/tag.rs` is the contract — the magic, the version,
// the caps, the opcode table and every enum code — and `src/reader.rs` is the
// reference decoder this file mirrors, error taxonomy included. Neither half
// can see the other, and no compiler anywhere reads both, so the pair is held
// together from outside: a Rust test freezes the canonical stream into
// `crates/crcbl-webgpu/tests/fixtures/canonical-stream.bin`, and
// `web/tools/stream-decode.mjs` decodes that same file here and asserts every
// field. A number changed on one side and not the other turns one of those two
// red.
//
// STRICT, FOR THE REASON THE RUST DECODER IS. Every read is bounds-checked
// against the buffer, every enum code is one the encoder wrote, every unclaimed
// bitflag bit is an error rather than a truncation, and a presence byte that is
// neither canonical value is refused rather than read as truthy. A replayer
// that guessed instead would turn a corrupt stream into WebGPU calls.
//
// THE DETACHED-VIEW RULE STILL APPLIES. In the browser the bytes are wasm's, so
// the caller builds the view immediately before the call from the pointer it
// was just handed, and nothing here outlives the call: no view is stored, and
// `PushConstants.data` is copied out rather than returned as a window onto the
// heap that the next allocation would detach. See `web/engine/wasm.js`.
//
// SIXTY-FOUR BITS, AND WHERE THEY WOULD OTHERWISE BE LOST. A JS number is an
// exact integer only to 2^53, and this format carries fields above that, so
// nothing 64-bit here is read as one:
//
//   * A handle decodes to `{ index, generation }`, two `u32`s, each exact as a
//     number — and that is `crcbl_core::Handle`'s own shape rather than a
//     repacking of it. `Handle::to_bits` puts the **generation in the high
//     half** and the index in the low half; read as a single number the packed
//     form is already wrong for any generation above 2^21, and wrong silently.
//     Splitting also means the two halves cannot be swapped by accident here,
//     because each is named.
//   * `CreateBuffer.size` and a stream's sequence numbers are `BigInt`. Size
//     carries `WHOLE_BUFFER` — `u64::MAX` — through verbatim, and a number
//     would round it to 18446744073709552000: a different size that still looks
//     enormous, so nothing downstream would question it. Sequence numbers are
//     64-bit precisely because they are never allowed to wrap within a session.
//   * `RequestDevice`'s two feature words are `BigInt` too. `crcbl_hal::Features`
//     is a 64-bit bitflags and the flags it has today all sit in the low
//     twenty-seven bits, so a number would be exact *for now* — which is the
//     worst of the three possibilities, since the day a flag is added past bit
//     53 nothing here would fail, and a required feature would quietly go
//     missing.
//
// Everything else on the wire is 32 bits or narrower and stays a number.

import { FORMAT } from './gpu-reply.js';

// ── Header ───────────────────────────────────────────────────────────────────

/** `tag::STREAM_MAGIC` — the ASCII bytes every stream buffer starts with. */
const STREAM_MAGIC = new Uint8Array([
  0x43, 0x52, 0x43, 0x42, 0x4c, 0x47, 0x50, 0x55,
]); // "CRCBLGPU"

/** `tag::STREAM_VERSION`. */
const STREAM_VERSION = 1;

// ── Caps ─────────────────────────────────────────────────────────────────────

/** `tag::MAX_FIELD_BYTES` — largest single length-prefixed byte field. */
const MAX_FIELD_BYTES = 1 << 20;

/** `tag::MAX_ELEMENT_COUNT` — largest element count in a length-prefixed array. */
const MAX_ELEMENT_COUNT = 1 << 16;

// ── Command tags ─────────────────────────────────────────────────────────────

const CREATE_BUFFER_TAG = 0x00;
const CREATE_SURFACE_TAG = 0x01;
const CREATE_IMAGE_TAG = 0x02;
const CREATE_IMAGE_VIEW_TAG = 0x03;
const CREATE_SAMPLER_TAG = 0x04;
const DESTROY_BUFFER_TAG = 0x20;
const DESTROY_SURFACE_TAG = 0x21;
const DESTROY_IMAGE_TAG = 0x22;
const DESTROY_IMAGE_VIEW_TAG = 0x23;
const DESTROY_SAMPLER_TAG = 0x24;
const BEGIN_DEBUG_LABEL_TAG = 0x40;
const BEGIN_RENDER_PASS_TAG = 0x41;
const BIND_GRAPHICS_PIPELINE_TAG = 0x42;
const BIND_GROUP_TAG = 0x43;
const PUSH_CONSTANTS_TAG = 0x44;
const DRAW_TAG = 0x60;
const ENUMERATE_ADAPTERS_TAG = 0x90;
const REQUEST_DEVICE_TAG = 0x91;
const SURFACE_CAPS_TAG = 0x92;

// ── Optional fields ──────────────────────────────────────────────────────────

const ABSENT = 0;
const PRESENT = 1;

// ── Enum code tables ─────────────────────────────────────────────────────────
//
// Indexed by wire code, so a gap is `undefined` and therefore an error. The
// names are the HAL's variant names, which is what makes a decoded stream
// readable without this file open beside it.

/** `tag::LOAD_OP_*`. */
const LOAD_OP = ['Load', 'Clear', 'DontCare'];

/** `tag::STORE_OP_*`. */
const STORE_OP = ['Store', 'Discard'];

/** `tag::MEMORY_*`. */
const MEMORY_LOCATION = ['DeviceLocal', 'HostUpload', 'HostReadback'];

/**
 * `tag::IMAGE_TYPE_*`.
 *
 * A GAP HERE IS THE COSTLIEST GAP IN THIS FILE. `Extent3d.depthOrLayers` is the
 * depth for `D3` and the array-layer count for everything else, so the same
 * three numbers describe two different images depending only on this byte —
 * a 64-deep volume against 64 flat slices, with mip chains of seven levels and
 * three. Nothing downstream can tell them apart, because the bytes are
 * identical; the `undefined` this table answers with for an unclaimed code is
 * the only place the difference is visible.
 */
const IMAGE_TYPE = ['D1', 'D2', 'D3'];

/**
 * `tag::IMAGE_VIEW_TYPE_*`.
 *
 * A separate table from {@link IMAGE_TYPE} and not a superset of it, because
 * the HAL keeps them separate: a view reinterprets its image's dimensionality,
 * so the two fields of a create pair legitimately disagree.
 *
 * `D2`/`D2Array` and `Cube`/`CubeArray` are adjacent codes, and each member of
 * a pair accepts exactly the `baseLayer` and `layerCount` the other does — so a
 * code read one along builds a view rather than refusing one, and a cube map's
 * six faces become six unrelated slices.
 */
const IMAGE_VIEW_TYPE = ['D1', 'D2', 'D2Array', 'Cube', 'CubeArray', 'D3'];

/**
 * `tag::FILTER_MODE_*`.
 *
 * Two rows, and read **three times per sampler** — `magFilter`, `minFilter` and
 * `mipFilter` are three of these bytes back to back. That is what makes so small
 * a table worth writing out rather than reading as a boolean: with two variants
 * there is no unclaimed code to land on, so a byte read out of position decodes
 * to a filter rather than to an error, and a sampler that filters correctly when
 * magnifying and not when minifying looks like a texture that is merely soft.
 */
const FILTER_MODE = ['Nearest', 'Linear'];

/**
 * `tag::SAMPLER_ADDRESS_*`.
 *
 * {@link FILTER_MODE}'s hazard with four rows: `addressMode` is three of these
 * bytes in a row, for U, V and W.
 *
 * `ClampToEdge` and `ClampToBorder` are the pair a gap would cost the most,
 * because they agree everywhere but the edge texel — one repeats it outwards and
 * the other fetches transparent black, so the wrong one of the two bleeds an
 * atlas's neighbour into every seam with nothing anywhere reporting an error.
 * They also part company at the backend: WebGPU has no border colour at all, so
 * `gpu-replay.js` refuses one of them and not the other.
 */
const SAMPLER_ADDRESS_MODE = [
  'Repeat',
  'MirrorRepeat',
  'ClampToEdge',
  'ClampToBorder',
];

/**
 * `tag::COMPARE_OP_*` — what a hardware-PCF sampler compares with.
 *
 * `Greater` and `Less` are the row pair that must never fold. `crcbl_hal`'s
 * `CompareOp` names the comparison performed rather than what it means for
 * visibility, and under this engine's reversed-Z it is `Greater` that asks "is
 * the fragment closer than the stored caster?" — so a shadow sampler that got
 * `Less` instead lights exactly the surfaces that should be in shadow and
 * shadows the rest, every frame, with no error anywhere. A `GPUSampler` reports
 * nothing about the comparison it was built with either, so this table answering
 * `undefined` for a code it does not claim is the only place the difference is
 * visible.
 */
const COMPARE_OP = [
  'Never',
  'Less',
  'Equal',
  'LessOrEqual',
  'Greater',
  'NotEqual',
  'GreaterOrEqual',
  'Always',
];

/**
 * `tag::FORMAT_*`, code-indexed — the inverse of the table `gpu-reply.js`
 * exports, and not a second copy of it.
 *
 * The reply direction already carries formats and already keeps this mapping;
 * a table written out again here would be one more thing to keep in step with
 * `crates/crcbl-webgpu/src/tag.rs` and would drift from its twin rather than
 * from the Rust, which is worse — the fixture that pins one would not pin the
 * other. Inverting it costs a loop at module load and leaves a single place
 * where a code and a format meet.
 *
 * The names are therefore that table's spelling (`BGRA8_UNORM`) rather than the
 * HAL's (`Bgra8Unorm`), which is what the other tables here use. The reverse
 * lookup a replayer needs is the imported object itself.
 */
const IMAGE_FORMAT = [];
for (const [name, code] of Object.entries(FORMAT)) IMAGE_FORMAT[code] = name;

// ── Bitflag tables ───────────────────────────────────────────────────────────
//
// In ascending bit order, which is the order a decoded flag list comes back in.
// Each bit is an explicit `1 << n` in `crcbl-hal`, so these are chosen wire
// values rather than declaration positions — the one place the encoding is
// allowed to mirror the Rust type directly.

/** `crcbl_hal::BufferUsage`. */
const BUFFER_USAGE = [
  'TRANSFER_SRC',
  'TRANSFER_DST',
  'UNIFORM',
  'STORAGE',
  'INDEX',
  'INDIRECT',
  'DEVICE_ADDRESS',
  'QUERY_RESOLVE',
];

/** `crcbl_hal::ShaderStages`. */
const SHADER_STAGES = ['VERTEX', 'FRAGMENT', 'COMPUTE', 'MESH'];

/** `crcbl_hal::ImageUsage`. */
const IMAGE_USAGE = [
  'TRANSFER_SRC',
  'TRANSFER_DST',
  'SAMPLED',
  'STORAGE',
  'COLOR_ATTACHMENT',
  'DEPTH_STENCIL_ATTACHMENT',
  'PRESENT',
];

/** `crcbl_hal::ImageAspect` — which planes of an image a view touches. */
const IMAGE_ASPECT = ['COLOR', 'DEPTH', 'STENCIL'];

/**
 * Every bit `crcbl_hal::Features` claims — `Features::all().bits()`.
 *
 * A MASK RATHER THAN A NAME TABLE, and the one bitflags field here that gets
 * one. The other two decode to lists of flag names, which is what makes a
 * decoded stream readable; a twenty-seven row table for this one would be
 * twenty-seven more things to keep in step with `crcbl-hal` for a value nothing
 * downstream reads by name — `gpu-replay.js` maps *bits* to `GPUFeatureName`s.
 *
 * It is still checked rather than waved through, because the seam's rule is
 * `from_bits` and never `from_bits_truncate`: an unclaimed bit is a build that
 * knows a flag this one does not, and dropping it would turn a feature the
 * caller required into one nobody asked for. The mask is held honest from
 * outside — the committed fixture carries a `RequestDevice` whose optional word
 * is `Features::all()`, so a mask narrower than Rust's refuses the fixture and
 * `stream-decode.mjs` goes red.
 */
const FEATURES_CLAIMED = (1n << 27n) - 1n;

// ── Errors ───────────────────────────────────────────────────────────────────

/**
 * Everything a malformed stream can be.
 *
 * `kind` mirrors a variant of `crcbl_webgpu::DecodeError` one for one, and
 * `details` carries that variant's fields under their Rust names. A caller
 * branches on `kind`; the message is for a human.
 *
 * @typedef {object} DecodeErrorDetails
 * @property {string} [field] Field that carried the offending value.
 * @property {number} [tag] The unknown tag byte.
 * @property {number} [offset] Where a too-short read started.
 * @property {number} [needed] Bytes that read wanted.
 * @property {number} [remaining] Bytes actually left.
 * @property {number} [len] A length or count past its cap.
 * @property {number|bigint} [code] An enum, bitflags or presence value no
 *   variant claims. A `bigint` for the one field that is sixty-four bits wide,
 *   `crcbl_hal::Features`, because a number could not carry the offending value
 *   back exactly — which is the whole of what this field is for.
 * @property {number} [found] Version the buffer declared.
 * @property {number} [expected] Version this build speaks.
 */
export class StreamDecodeError extends Error {
  /**
   * @param {'BadMagic'|'UnsupportedVersion'|'TooShort'|'UnknownTag'|'InvalidLength'|'InvalidEnum'|'NullHandle'|'NotUtf8'} kind
   * @param {string} message
   * @param {DecodeErrorDetails} [details]
   */
  constructor(kind, message, details = {}) {
    super(message);
    this.name = 'StreamDecodeError';
    this.kind = kind;
    this.details = details;
  }
}

// ── ByteReader ───────────────────────────────────────────────────────────────

/**
 * A cursor over a stream buffer that bounds-checks every read.
 *
 * Everything below goes through this rather than hand-rolling an offset test
 * per field, for the reason `reader.rs` gives: that is one more chance to get a
 * bound wrong at each of them, and this seam has a lot of fields.
 */
class ByteReader {
  /** @type {Uint8Array} */
  #bytes;
  /** @type {DataView} */
  #view;
  /** @type {TextDecoder} */
  #utf8 = new TextDecoder('utf-8', { fatal: true });
  #offset = 0;

  /** @param {Uint8Array} bytes */
  constructor(bytes) {
    this.#bytes = bytes;
    this.#view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  get remaining() {
    return this.#bytes.byteLength - this.#offset;
  }

  get isEmpty() {
    return this.remaining === 0;
  }

  /**
   * Reserves `needed` bytes and returns where they start, advancing past them.
   * Every read in this class begins here, so no other method indexes.
   *
   * @param {number} needed
   * @returns {number}
   */
  #take(needed) {
    if (this.remaining < needed) {
      throw new StreamDecodeError(
        'TooShort',
        `stream too short: need ${needed} bytes at offset ${this.#offset}, have ${this.remaining}`,
        { needed, offset: this.#offset, remaining: this.remaining }
      );
    }
    const at = this.#offset;
    this.#offset += needed;
    return at;
  }

  /**
   * A borrowed window on `len` bytes. It is a view over whatever the caller
   * handed in — wasm memory, in the browser — so it is compared or copied
   * before this method is called again, and never retained.
   *
   * @param {number} len
   * @returns {Uint8Array}
   */
  readBytes(len) {
    const at = this.#take(len);
    return this.#bytes.subarray(at, at + len);
  }

  readU8() {
    return this.#view.getUint8(this.#take(1));
  }

  readU16() {
    return this.#view.getUint16(this.#take(2), true);
  }

  readU32() {
    return this.#view.getUint32(this.#take(4), true);
  }

  readI32() {
    return this.#view.getInt32(this.#take(4), true);
  }

  readU64() {
    return this.#view.getBigUint64(this.#take(8), true);
  }

  readF32() {
    return this.#view.getFloat32(this.#take(4), true);
  }

  /**
   * A handle, as the `u64` `Handle::to_bits` packs: index low, generation high.
   * Bounds-checked as the eight bytes it is, so a truncation reports the same
   * numbers the Rust decoder does.
   *
   * @returns {{ index: number, generation: number }}
   */
  #readHandleBits() {
    const at = this.#take(8);
    return {
      index: this.#view.getUint32(at, true),
      generation: this.#view.getUint32(at + 4, true),
    };
  }

  /**
   * A handle that may not be absent. A zero generation is what
   * `Handle::from_bits` rejects, and no real handle ever has one.
   *
   * @param {string} field
   * @returns {{ index: number, generation: number }}
   */
  readHandle(field) {
    const handle = this.#readHandleBits();
    if (handle.generation === 0) {
      throw new StreamDecodeError('NullHandle', `${field} is a null handle`, {
        field,
      });
    }
    return handle;
  }

  /**
   * A handle that may be absent, as a bare `u64` with zero for `None`. No
   * presence byte: the generation's niche is what makes zero unambiguous.
   *
   * @returns {{ index: number, generation: number } | null}
   */
  readOptHandle() {
    const handle = this.#readHandleBits();
    return handle.generation === 0 ? null : handle;
  }

  /**
   * @param {string} field
   * @param {number} cap
   * @returns {number}
   */
  #readLen(field, cap) {
    const len = this.readU32();
    if (len > cap) {
      throw new StreamDecodeError(
        'InvalidLength',
        `invalid length for ${field}: ${len}`,
        { field, len }
      );
    }
    return len;
  }

  /**
   * A length-prefixed byte field, capped by `MAX_FIELD_BYTES`. Copied, because
   * the command object outlives the view it was decoded from.
   *
   * @param {string} field
   * @returns {Uint8Array}
   */
  readField(field) {
    return this.readBytes(this.#readLen(field, MAX_FIELD_BYTES)).slice();
  }

  /**
   * @param {string} field
   * @returns {string}
   */
  readString(field) {
    const bytes = this.readBytes(this.#readLen(field, MAX_FIELD_BYTES));
    try {
      return this.#utf8.decode(bytes);
    } catch {
      throw new StreamDecodeError('NotUtf8', `${field} is not valid UTF-8`, {
        field,
      });
    }
  }

  /**
   * A presence byte, then the string if there is one. `Some("")` and `None` are
   * different values and stay different here.
   *
   * @param {string} field
   * @returns {string | null}
   */
  readOptString(field) {
    return this.readPresent(field) ? this.readString(field) : null;
  }

  /**
   * A presence byte. Anything but the two canonical values is refused rather
   * than read as truthy.
   *
   * @param {string} field
   * @returns {boolean}
   */
  readPresent(field) {
    const code = this.readU8();
    if (code === ABSENT) return false;
    if (code === PRESENT) return true;
    throw new StreamDecodeError(
      'InvalidEnum',
      `invalid code for ${field}: 0x${code.toString(16)}`,
      { field, code }
    );
  }

  /**
   * An element count, capped by `MAX_ELEMENT_COUNT` *and* by the bytes left —
   * every element costs at least one byte, so a count past that cannot be
   * honest, and neither cap alone bounds the work on its own.
   *
   * @param {string} field
   * @returns {number}
   */
  readCount(field) {
    const count = this.#readLen(field, MAX_ELEMENT_COUNT);
    if (count > this.remaining) {
      throw new StreamDecodeError(
        'InvalidLength',
        `invalid length for ${field}: ${count}`,
        { field, len: count }
      );
    }
    return count;
  }

  /**
   * An enum code, through the table that claims it.
   *
   * @param {string} field
   * @param {readonly string[]} table
   * @returns {string}
   */
  readEnum(field, table) {
    const code = this.readU8();
    const name = table[code];
    if (name === undefined) {
      throw new StreamDecodeError(
        'InvalidEnum',
        `invalid code for ${field}: 0x${code.toString(16)}`,
        { field, code }
      );
    }
    return name;
  }

  /**
   * A presence byte, then an enum code if there is one.
   *
   * The optional-field rule applied to an enum rather than to a string: a
   * presence byte, because nothing but a handle has a niche to spare. It is two
   * reads rather than a table with a reserved "absent" row for a reason a
   * one-variant table would hide — `CompareOp::Never` is code `0`, so a reserved
   * absent row would put "no comparison at all" and "a comparison that always
   * fails" one byte apart, and a shadow sampler built with the second returns
   * zero everywhere.
   *
   * @param {string} field
   * @param {readonly string[]} table
   * @returns {string | null}
   */
  readOptEnum(field, table) {
    return this.readPresent(field) ? this.readEnum(field, table) : null;
  }

  /**
   * A bitflags value, as the names of the bits it sets in ascending bit order.
   *
   * Strict, like `from_bits` and unlike `from_bits_truncate`: a bit no flag
   * claims is an error, because truncating would silently drop something the
   * encoder meant.
   *
   * @param {string} field
   * @param {readonly string[]} table
   * @returns {string[]}
   */
  readFlags(field, table) {
    const bits = this.readU32();
    const claimed = 2 ** table.length - 1;
    if ((bits & ~claimed) >>> 0 !== 0) {
      throw new StreamDecodeError(
        'InvalidEnum',
        `invalid code for ${field}: 0x${bits.toString(16)}`,
        { field, code: bits }
      );
    }
    return table.filter((_, bit) => (bits & (1 << bit)) !== 0);
  }

  /**
   * A `crcbl_hal::Features` word, as the `BigInt` it is — sixty-four bits, and
   * a number is exact only to fifty-three.
   *
   * Strict for `readFlags`'s reason and refused the same way; see
   * {@link FEATURES_CLAIMED} for why this one stays a word rather than becoming
   * a list of names.
   *
   * @param {string} field
   * @returns {bigint}
   */
  readFeatures(field) {
    const bits = this.readU64();
    if ((bits & ~FEATURES_CLAIMED) !== 0n) {
      throw new StreamDecodeError(
        'InvalidEnum',
        `invalid code for ${field}: 0x${bits.toString(16)}`,
        { field, code: bits }
      );
    }
    return bits;
  }

  /** @returns {{ color: number[], depth: number, stencil: number }} */
  readClearValue() {
    return {
      color: [this.readF32(), this.readF32(), this.readF32(), this.readF32()],
      depth: this.readF32(),
      stencil: this.readU32(),
    };
  }

  /** @returns {{ x: number, y: number, width: number, height: number }} */
  readRect() {
    return {
      x: this.readI32(),
      y: this.readI32(),
      width: this.readU32(),
      height: this.readU32(),
    };
  }

  /**
   * A `crcbl_hal::Extent3d`. `depthOrLayers` is the depth for an `ImageType.D3`
   * and the array-layer count otherwise, and only the image type says which.
   *
   * @returns {{ width: number, height: number, depthOrLayers: number }}
   */
  readExtent() {
    return {
      width: this.readU32(),
      height: this.readU32(),
      depthOrLayers: this.readU32(),
    };
  }

  /**
   * A `crcbl_hal::ImageSubresourceRange`. `mipCount` and `layerCount` carry
   * `ImageSubresourceRange::ALL` — `0xFFFFFFFF`, "every remaining one" —
   * verbatim: resolving it needs the image's own counts, which the encoder does
   * not have either.
   *
   * @returns {{ aspect: string[], baseMip: number, mipCount: number, baseLayer: number, layerCount: number }}
   */
  readSubresourceRange() {
    return {
      aspect: this.readFlags('ImageSubresourceRange::aspect', IMAGE_ASPECT),
      baseMip: this.readU32(),
      mipCount: this.readU32(),
      baseLayer: this.readU32(),
      layerCount: this.readU32(),
    };
  }

  readColorAttachment() {
    return {
      view: this.readHandle('ColorAttachment::view'),
      resolve: this.readOptHandle(),
      load: this.readEnum('ColorAttachment::load', LOAD_OP),
      store: this.readEnum('ColorAttachment::store', STORE_OP),
      clear: this.readClearValue(),
    };
  }

  readDepthStencilAttachment() {
    return {
      view: this.readHandle('DepthStencilAttachment::view'),
      readOnly: this.readPresent('DepthStencilAttachment::read_only'),
      depthLoad: this.readEnum('DepthStencilAttachment::depth_load', LOAD_OP),
      depthStore: this.readEnum(
        'DepthStencilAttachment::depth_store',
        STORE_OP
      ),
      stencilLoad: this.readEnum(
        'DepthStencilAttachment::stencil_load',
        LOAD_OP
      ),
      stencilStore: this.readEnum(
        'DepthStencilAttachment::stencil_store',
        STORE_OP
      ),
      clear: this.readClearValue(),
    };
  }
}

// ── StreamReader ─────────────────────────────────────────────────────────────

/**
 * Decodes a buffer `crcbl_webgpu::StreamWriter` produced.
 *
 * Commands come out one at a time so a replayer can hold the sequence number of
 * the one it is executing: a WebGPU validation error names this decoder and not
 * the Rust that encoded the command, and the sequence is the only thing that
 * connects the two back up.
 */
export class StreamReader {
  /** @type {ByteReader} */
  #reader;
  /** @type {bigint} */
  #baseSequence;
  #decoded = 0n;
  /**
   * Set once a decode fails. Nothing is resumable after that: the cursor is
   * somewhere inside a command body and the next byte is not a tag.
   */
  #failed = false;

  /**
   * Opens a stream, checking its magic and version.
   *
   * The version check is not ceremony: the wasm and JavaScript halves ship as
   * separate artifacts and are cached independently, so a decoder meeting a
   * stream from a different build is reachable in a browser in a way it is not
   * in a single binary.
   *
   * @param {Uint8Array} bytes The whole buffer, header included.
   * @throws {StreamDecodeError} If the header is not this format's, or if there
   *   is not a whole header.
   */
  constructor(bytes) {
    this.#reader = new ByteReader(bytes);
    const magic = this.#reader.readBytes(STREAM_MAGIC.length);
    if (!STREAM_MAGIC.every((byte, at) => byte === magic[at])) {
      throw new StreamDecodeError(
        'BadMagic',
        'not a command stream: bad magic'
      );
    }
    const version = this.#reader.readU16();
    if (version !== STREAM_VERSION) {
      throw new StreamDecodeError(
        'UnsupportedVersion',
        `stream format version ${version}, expected ${STREAM_VERSION}`,
        { found: version, expected: STREAM_VERSION }
      );
    }
    this.#baseSequence = this.#reader.readU64();
  }

  /** The sequence number of the first command in this buffer. */
  get baseSequence() {
    return this.#baseSequence;
  }

  /**
   * The next command and the sequence number it carries, or `null` at the end
   * of the stream.
   *
   * Sequence numbers are positional — the nth command decoded is
   * `baseSequence + n` — which is why nothing per command is on the wire.
   *
   * Returns `null` forever after a throw: the cursor is then somewhere inside a
   * command body, so the next byte is not a tag and resuming would invent
   * commands out of a payload.
   *
   * @returns {{ sequence: bigint, command: object } | null}
   * @throws {StreamDecodeError} Anything the command body produces.
   */
  nextCommand() {
    if (this.#failed || this.#reader.isEmpty) return null;
    let command;
    try {
      command = decodeCommand(this.#reader);
    } catch (error) {
      this.#failed = true;
      throw error;
    }
    // Wrapping, because the base came off the wire: a buffer declaring
    // `u64::MAX` must not produce a sequence outside the range it is typed as.
    const sequence = BigInt.asUintN(64, this.#baseSequence + this.#decoded);
    this.#decoded += 1n;
    return { sequence, command };
  }
}

/**
 * Every command in a stream, in order.
 *
 * The convenience half of {@link StreamReader}, for a test or a dump that wants
 * the whole buffer at once. The sequence numbers are the reader's
 * `baseSequence` plus each command's index.
 *
 * @param {Uint8Array} bytes
 * @returns {object[]}
 * @throws {StreamDecodeError} The first error the stream produces; nothing
 *   after it is decoded.
 */
export function decodeStream(bytes) {
  const reader = new StreamReader(bytes);
  const commands = [];
  for (
    let next = reader.nextCommand();
    next !== null;
    next = reader.nextCommand()
  ) {
    commands.push(next.command);
  }
  return commands;
}

/**
 * One command body, dispatched on its tag.
 *
 * The tag comes first so this dispatches rather than trial-decodes, which is
 * what keeps "unknown command" and "malformed known command" distinguishable —
 * the two errors below.
 *
 * @param {ByteReader} r
 * @returns {object}
 */
function decodeCommand(r) {
  const tag = r.readU8();
  switch (tag) {
    case CREATE_BUFFER_TAG:
      return {
        name: 'CreateBuffer',
        buffer: r.readHandle('CreateBuffer::buffer'),
        label: r.readOptString('BufferDesc::label'),
        size: r.readU64(),
        usage: r.readFlags('BufferDesc::usage', BUFFER_USAGE),
        memory: r.readEnum('BufferDesc::memory', MEMORY_LOCATION),
      };
    case CREATE_SURFACE_TAG:
      // `canvasId` is a key into the shell's own canvas registry, not a handle
      // and not a `GPUCanvasContext`: the shim assigned the number so that no
      // string has to cross the boundary. A `u32`, so it stays a number.
      return {
        name: 'CreateSurface',
        surface: r.readHandle('CreateSurface::surface'),
        canvasId: r.readU32(),
      };
    case CREATE_IMAGE_TAG: {
      // Spelled out rather than built inline, for `Draw`'s reason: `mipLevels`
      // and `samples` are adjacent, identically typed and mean different
      // things, which is the pair a decoder most easily swaps.
      //
      // Both are carried verbatim, ZERO INCLUDED. Neither value is one a device
      // accepts, and neither is a malformed stream: every `u32` is a value the
      // wire form claims, so there is nothing here to refuse that a flag word
      // or an enum table would refuse. An invalid descriptor is a creation
      // failure, and those arrive through `Device::take_error` rather than out
      // of a decoder.
      const image = r.readHandle('CreateImage::image');
      const label = r.readOptString('ImageDesc::label');
      const imageType = r.readEnum('ImageDesc::image_type', IMAGE_TYPE);
      const extent = r.readExtent();
      const format = r.readEnum('ImageDesc::format', IMAGE_FORMAT);
      const mipLevels = r.readU32();
      const samples = r.readU32();
      return {
        name: 'CreateImage',
        image,
        label,
        imageType,
        extent,
        format,
        mipLevels,
        samples,
        usage: r.readFlags('ImageDesc::usage', IMAGE_USAGE),
      };
    }
    case CREATE_IMAGE_VIEW_TAG: {
      // Two handles, and they mean opposite things: `view` is the id the
      // replayer stores the new object at, `image` the id it looks the viewed
      // object up by. Spelled out so the two cannot be read in the other order.
      const view = r.readHandle('CreateImageView::view');
      const label = r.readOptString('ImageViewDesc::label');
      const image = r.readHandle('ImageViewDesc::image');
      const viewType = r.readEnum('ImageViewDesc::view_type', IMAGE_VIEW_TYPE);
      const format = r.readEnum('ImageViewDesc::format', IMAGE_FORMAT);
      return {
        name: 'CreateImageView',
        view,
        label,
        image,
        viewType,
        format,
        range: r.readSubresourceRange(),
      };
    }
    case CREATE_SAMPLER_TAG: {
      // Spelled out one field at a time, for `CreateImage`'s reason with six
      // fields instead of two: three `FilterMode` bytes and three
      // `SamplerAddressMode` bytes go over back to back, and any two of the six
      // read in the wrong order still decodes to a sampler.
      //
      // The three floats cross verbatim, SENTINEL INCLUDED. `lodMax` is
      // `f32::MAX` for a `SamplerDesc::default`, meaning "no limit", and
      // resolving it is the replayer's job — `gpu-replay.js` argues what it
      // resolves to, and it is not an absent member. `anisotropy` is whatever
      // the caller passed, fractional values and values past the device's cap
      // included, because every `f32` bit pattern is a value the wire form
      // claims.
      const sampler = r.readHandle('CreateSampler::sampler');
      const label = r.readOptString('SamplerDesc::label');
      const magFilter = r.readEnum('SamplerDesc::mag_filter', FILTER_MODE);
      const minFilter = r.readEnum('SamplerDesc::min_filter', FILTER_MODE);
      const mipFilter = r.readEnum('SamplerDesc::mip_filter', FILTER_MODE);
      const addressMode = [
        r.readEnum('SamplerDesc::address_mode', SAMPLER_ADDRESS_MODE),
        r.readEnum('SamplerDesc::address_mode', SAMPLER_ADDRESS_MODE),
        r.readEnum('SamplerDesc::address_mode', SAMPLER_ADDRESS_MODE),
      ];
      const lodMin = r.readF32();
      const lodMax = r.readF32();
      const anisotropy = r.readF32();
      return {
        name: 'CreateSampler',
        sampler,
        label,
        magFilter,
        minFilter,
        mipFilter,
        addressMode,
        lodMin,
        lodMax,
        anisotropy,
        compare: r.readOptEnum('SamplerDesc::compare', COMPARE_OP),
      };
    }
    case DESTROY_BUFFER_TAG:
      return {
        name: 'DestroyBuffer',
        buffer: r.readHandle('DestroyBuffer::buffer'),
      };
    case DESTROY_SURFACE_TAG:
      return {
        name: 'DestroySurface',
        surface: r.readHandle('DestroySurface::surface'),
      };
    case DESTROY_IMAGE_TAG:
      return {
        name: 'DestroyImage',
        image: r.readHandle('DestroyImage::image'),
      };
    case DESTROY_IMAGE_VIEW_TAG:
      // Its own tag rather than a kind byte on one destroy: the opcode is what
      // says which table an id indexes, and a view's table and its image's
      // genuinely issue identical bits.
      return {
        name: 'DestroyImageView',
        view: r.readHandle('DestroyImageView::view'),
      };
    case DESTROY_SAMPLER_TAG:
      // Its own tag again, and its own table on the far side: a sampler's id
      // and an image's are allowed to be the same eight bytes, and the probe's
      // deliberately are.
      return {
        name: 'DestroySampler',
        sampler: r.readHandle('DestroySampler::sampler'),
      };
    case BEGIN_DEBUG_LABEL_TAG:
      return {
        name: 'BeginDebugLabel',
        label: r.readString('BeginDebugLabel::label'),
      };
    case BEGIN_RENDER_PASS_TAG: {
      const label = r.readOptString('RenderPassDesc::label');
      const count = r.readCount('RenderPassDesc::color_attachments');
      const colorAttachments = [];
      for (let i = 0; i < count; i += 1) {
        colorAttachments.push(r.readColorAttachment());
      }
      const depthStencilAttachment = r.readPresent(
        'RenderPassDesc::depth_stencil_attachment'
      )
        ? r.readDepthStencilAttachment()
        : null;
      return {
        name: 'BeginRenderPass',
        label,
        colorAttachments,
        depthStencilAttachment,
        renderArea: r.readRect(),
      };
    }
    case BIND_GRAPHICS_PIPELINE_TAG:
      return {
        name: 'BindGraphicsPipeline',
        pipeline: r.readHandle('BindGraphicsPipeline::pipeline'),
      };
    case BIND_GROUP_TAG: {
      const slot = r.readU32();
      const group = r.readHandle('BindGroup::group');
      const count = r.readCount('BindGroup::dynamic_offsets');
      const dynamicOffsets = [];
      for (let i = 0; i < count; i += 1) {
        dynamicOffsets.push(r.readU32());
      }
      return {
        name: 'BindGroup',
        slot,
        group,
        dynamicOffsets,
        layout: r.readHandle('BindGroup::layout'),
      };
    }
    case PUSH_CONSTANTS_TAG:
      return {
        name: 'PushConstants',
        stages: r.readFlags('PushConstants::stages', SHADER_STAGES),
        offset: r.readU32(),
        data: r.readField('PushConstants::data'),
        layout: r.readHandle('PushConstants::layout'),
      };
    case DRAW_TAG: {
      // Spelled out rather than built inline: the two halves of each range are
      // read in source order either way, but relying on that to get the field
      // order right reads as a coincidence.
      const firstVertex = r.readU32();
      const lastVertex = r.readU32();
      const firstInstance = r.readU32();
      const lastInstance = r.readU32();
      return {
        name: 'Draw',
        vertices: { start: firstVertex, end: lastVertex },
        instances: { start: firstInstance, end: lastInstance },
      };
    }
    case REQUEST_DEVICE_TAG: {
      // Spelled out rather than built inline, for `Draw`'s reason: the two
      // feature words are adjacent, identically typed and mean opposite things,
      // which is the pair a reader most easily swaps.
      const adapter = r.readU32();
      const label = r.readOptString('DeviceDesc::label');
      const requiredFeatures = r.readFeatures('DeviceDesc::required_features');
      const optionalFeatures = r.readFeatures('DeviceDesc::optional_features');
      return {
        name: 'RequestDevice',
        adapter,
        label,
        requiredFeatures,
        optionalFeatures,
        compatibleSurface: r.readOptHandle(),
      };
    }
    case SURFACE_CAPS_TAG:
      // No body: the HAL call's surface and adapter are validated where the
      // handle tables are and never cross. A decoder that still read them would
      // consume twelve bytes of whatever follows, which is why the corpus puts
      // a command after this one.
      return { name: 'SurfaceCaps' };
    case ENUMERATE_ADAPTERS_TAG:
      // No body either, and last in the corpus: a decoder that read one field
      // too many here runs off the end of the buffer rather than into a
      // neighbour, which is why `stream-decode.mjs` sweeps every truncation.
      return { name: 'EnumerateAdapters' };
    default:
      throw new StreamDecodeError(
        'UnknownTag',
        `unknown command tag: 0x${tag.toString(16).padStart(2, '0')}`,
        { tag }
      );
  }
}
