// The JavaScript half of `crcbl-webgpu`'s reply stream: answers in, one buffer
// out.
//
// THIS IS THE PRODUCTION ENCODER. The mirror image of `gpu-stream.js`, which is
// the production *decoder*: commands are encoded in Rust and decoded here,
// replies are encoded here and decoded in Rust. `crates/crcbl-webgpu/src/tag.rs`
// is the contract — the magic, the version, the caps and the reply tag table —
// and `src/reply.rs` is the reference encoder this file mirrors. Neither half
// can see the other, and no compiler anywhere reads both, so the pair is held
// together from outside: a Rust test freezes the canonical replies into
// `crates/crcbl-webgpu/tests/fixtures/canonical-replies.bin`, and
// `web/tools/reply-encode.mjs` re-encodes the same replies here and asserts the
// bytes match. A number changed on one side and not the other turns one of
// those two red.
//
// EVERY REPLY NAMES THE COMMAND IT ANSWERS, by the sequence number that command
// was assigned. Unlike the command stream — where the nth command's number is
// the header's base plus n — a reply's position says nothing about what it
// answers: this side replies when the browser has an answer, so replies arrive
// out of order, spread over frames, or never. The number is therefore a field,
// and wasm refuses a reply for a sequence it is not waiting on rather than
// taking it as an answer to something else.
//
// STRICT, FOR THE REASON THE DECODER IS. This writer asserts the same caps the
// Rust reader enforces — the field cap, the element cap, and a handle whose
// generation is not zero — so nothing encoded here is something the far side
// would refuse. A silent truncation would arrive as a `TooShort` a frame later,
// naming a field rather than the call that overfilled it.
//
// SIXTY-FOUR BITS ARE `BigInt` HERE TOO. A JS number is an exact integer only to
// 2^53. A sequence number is 64-bit precisely so it never wraps within a
// session, a query result is a raw GPU tick count, and an adapter's feature word
// and several of its limits are `u64` on the seam — so all of them are `BigInt`
// and a number is refused rather than rounded. Everything else on the wire is 32
// bits or narrower and stays a number.
//
// A MISSING FIELD IS REFUSED, NOT WRITTEN AS ZERO. `DataView.setUint32` turns
// `undefined` into `0` without a word, and `0` is a legal value for almost every
// numeric field on this wire — a vendor id, a feature word, a limit. So a
// misspelled or absent key would arrive on the far side as a device that
// genuinely reports nothing, which is the one failure this format cannot detect
// for itself. Every numeric write therefore names its field and checks its
// value.

// ── Header ───────────────────────────────────────────────────────────────────

/** `tag::REPLY_MAGIC` — the ASCII bytes every reply buffer starts with. */
const REPLY_MAGIC = new Uint8Array([
  0x43, 0x52, 0x43, 0x42, 0x4c, 0x52, 0x50, 0x4c,
]); // "CRCBLRPL"

/**
 * `tag::REPLY_VERSION`.
 *
 * `2` since an `Adapter` reply stopped being an id and a name and became the
 * whole of `AdapterInfo`. The word exists for exactly this: the two halves ship
 * as separate artifacts and are cached independently, so a page holding
 * yesterday's JavaScript against today's wasm has to be refused at the header
 * rather than read a name's length prefix as a vendor id.
 */
const REPLY_VERSION = 2;

// ── Caps ─────────────────────────────────────────────────────────────────────

/** `tag::MAX_FIELD_BYTES` — largest single length-prefixed byte field. */
const MAX_FIELD_BYTES = 1 << 20;

/** `tag::MAX_ELEMENT_COUNT` — largest element count in a length-prefixed array. */
const MAX_ELEMENT_COUNT = 1 << 16;

// ── Reply tags ───────────────────────────────────────────────────────────────
//
// Their own space, not a continuation of the command table's: the two never
// meet in a buffer, because a reply buffer opens with `REPLY_MAGIC` and a
// command buffer never does.

const ADAPTER_REPLY_TAG = 0x00;
const NO_ADAPTER_REPLY_TAG = 0x01;
const DEVICE_REPLY_TAG = 0x02;
const DEVICE_FAILED_REPLY_TAG = 0x03;
const SURFACE_CAPS_REPLY_TAG = 0x04;
const SURFACE_CAPS_FAILED_REPLY_TAG = 0x05;
const READBACK_PENDING_REPLY_TAG = 0x10;
const READBACK_READY_REPLY_TAG = 0x11;
const QUERY_RESULTS_REPLY_TAG = 0x18;

// ── DeviceType codes ─────────────────────────────────────────────────────────

/**
 * `tag::DEVICE_TYPE_*` — the wire codes for `crcbl_hal::DeviceType`.
 *
 * A REPLAYER READING `navigator.gpu` ONLY EVER SENDS `OTHER`, which is the
 * variant `crcbl-hal` documents as "the backend declined to say". WebGPU does
 * not report whether an adapter is discrete or integrated — it is a
 * fingerprinting surface the spec leaves out — and `GPUAdapter.isFallbackAdapter`
 * is not the same claim, since it grades performance rather than device class.
 * The other four exist because the field is a `DeviceType` and a wire form for
 * one has to be total; `gpu-replay.js` is where the choice of `OTHER` is made
 * and explained.
 */
export const DEVICE_TYPE = Object.freeze({
  CPU: 0x00,
  INTEGRATED: 0x01,
  DISCRETE: 0x02,
  VIRTUAL: 0x03,
  OTHER: 0x04,
});

// ── Optional fields ──────────────────────────────────────────────────────────
//
// `tag::ABSENT` and `tag::PRESENT`. Every optional field that is not a handle
// carries one of these two bytes and nothing else may appear there — the far
// side refuses a third value rather than reading it as truthy. An
// `Option<Handle>` needs none of this, because a zero generation is already
// unambiguous; nothing else on this wire has that niche.

const ABSENT = 0;
const PRESENT = 1;

// ── Surface capability codes ─────────────────────────────────────────────────
//
// `tag::FORMAT_*`, `tag::PRESENT_MODE_*` and `tag::COMPOSITE_ALPHA_*`. Chosen
// wire numbers, not positions: none of the HAL enums behind them carries
// explicit discriminants, and `crcbl_hal::Format` is deliberately open to a
// variant being inserted in the middle, so an encoder that sent declaration
// order would renumber everything below the insertion point and the failure
// would land here rather than at the edit. `crates/crcbl-webgpu/src/tag.rs`
// holds the same tables; the committed fixture keeps the two identical.

/** `tag::FORMAT_*` — the wire codes for `crcbl_hal::Format`. */
export const FORMAT = Object.freeze({
  R8_UNORM: 0x00,
  RG8_UNORM: 0x01,
  RGBA8_UNORM: 0x02,
  RGBA8_UNORM_SRGB: 0x03,
  BGRA8_UNORM: 0x04,
  BGRA8_UNORM_SRGB: 0x05,
  RGB10A2_UNORM: 0x06,
  R11G11B10_FLOAT: 0x07,
  R16_FLOAT: 0x08,
  RG16_FLOAT: 0x09,
  RGBA16_FLOAT: 0x0a,
  R32_FLOAT: 0x0b,
  RG32_FLOAT: 0x0c,
  RGBA32_FLOAT: 0x0d,
  R32_UINT: 0x0e,
  RG32_UINT: 0x0f,
  D32_FLOAT: 0x10,
  D32_FLOAT_S8_UINT: 0x11,
  D24_UNORM_S8_UINT: 0x12,
  D16_UNORM: 0x13,
  BC1_RGBA_UNORM: 0x14,
  BC1_RGBA_UNORM_SRGB: 0x15,
  BC3_RGBA_UNORM: 0x16,
  BC3_RGBA_UNORM_SRGB: 0x17,
  BC4_R_UNORM: 0x18,
  BC5_RG_UNORM: 0x19,
  BC6H_RGB_UFLOAT: 0x1a,
  BC7_RGBA_UNORM: 0x1b,
  BC7_RGBA_UNORM_SRGB: 0x1c,
});

/**
 * `tag::PRESENT_MODE_*` — the wire codes for `crcbl_hal::PresentMode`.
 *
 * A REPLAYER READING `navigator.gpu` ONLY EVER SENDS `FIFO`. WebGPU has no
 * present-mode selection at all: a canvas presents at the
 * `requestAnimationFrame` boundary, which is what `Fifo` describes. The other
 * three exist because the field is a `PresentMode` and a wire form for one has
 * to be total.
 */
export const PRESENT_MODE = Object.freeze({
  FIFO: 0x00,
  FIFO_RELAXED: 0x01,
  MAILBOX: 0x02,
  IMMEDIATE: 0x03,
});

/**
 * `tag::COMPOSITE_ALPHA_*` — the wire codes for `crcbl_hal::CompositeAlpha`.
 *
 * WebGPU's `GPUCanvasAlphaMode` is `'opaque'` or `'premultiplied'`, which are
 * the first two; the other two have no WebGPU spelling and a replayer never
 * offers them.
 */
export const COMPOSITE_ALPHA = Object.freeze({
  OPAQUE: 0x00,
  PRE_MULTIPLIED: 0x01,
  POST_MULTIPLIED: 0x02,
  INHERIT: 0x03,
});

/**
 * `tag::SURFACE_CAPS_FAILURE_*` — why a capability query answered nothing.
 *
 * THE HALF OF A REFUSAL A CALLER MAY BRANCH ON, and the reason a refusal is a
 * reply at all rather than a throw: `Instance::surface_caps` is the only call
 * that says whether an adapter can present to a window, so its own docs oblige a
 * caller doing adapter selection to treat a failure as an ordinary step rather
 * than a fault, and a channel with nowhere to put one would lose the frame.
 *
 * ONE ENTRY, BECAUSE THE COMMAND CARRIES NO ARGUMENTS. A stale surface handle
 * and an adapter index nothing enumerated were the two things this table used to
 * spell, and both are now refused by an `impl Instance` against its own tables
 * without a round trip. "The adapter cannot present to this surface" is not here
 * either: the command names no adapter and no surface, so it is not an answer
 * the question has. What is left is the query itself failing, which is
 * `BACKEND`, and `gpu-replay.js` raises it in exactly one place.
 *
 * The table has no holes, so `0x01` — the code `INVALID_HANDLE` had — must be
 * refused by the far side rather than decoded as a neighbour. That is what pins
 * these two halves together now that there is only one code to agree on.
 */
export const SURFACE_CAPS_FAILURE = Object.freeze({
  BACKEND: 0x00,
});

// ── Errors ───────────────────────────────────────────────────────────────────

/**
 * A reply this side refused to encode.
 *
 * Always a bug in the caller rather than a condition to recover from: a payload
 * past the cap, a handle with no generation, a sequence that is not a `BigInt`.
 * It is thrown where the mistake is, which is the whole point — the alternative
 * is a `DecodeError` out of wasm a frame later, naming a field.
 *
 * @typedef {object} ReplyEncodeErrorDetails
 * @property {string} [field] Field that carried the offending value.
 * @property {number} [len] A length or count past its cap.
 */
export class ReplyEncodeError extends Error {
  /**
   * @param {'InvalidLength'|'NullHandle'|'NotABigInt'|'NotANumber'} kind
   * @param {string} message
   * @param {ReplyEncodeErrorDetails} [details]
   */
  constructor(kind, message, details = {}) {
    super(message);
    this.name = 'ReplyEncodeError';
    this.kind = kind;
    this.details = details;
  }
}

// ── ByteWriter ───────────────────────────────────────────────────────────────

/**
 * A growing byte buffer that writes one field at a time.
 *
 * The counterpart of `gpu-stream.js`'s `ByteReader`, and everything below goes
 * through it rather than indexing: little-endian throughout, a `u32` length
 * prefix before every variable-length field, and the caps asserted where the
 * value arrives.
 */
class ByteWriter {
  /** @type {Uint8Array} */
  #bytes = new Uint8Array(256);
  /** @type {DataView} */
  #view = new DataView(this.#bytes.buffer);
  /** @type {TextEncoder} */
  #utf8 = new TextEncoder();
  #length = 0;

  /** What has been written, as a view over this writer's own buffer. */
  get bytes() {
    return this.#bytes.subarray(0, this.#length);
  }

  /** How much has been written, for {@link ByteWriter#truncate}. */
  get length() {
    return this.#length;
  }

  /**
   * Drops everything written past `at`.
   *
   * What makes a refused reply leave no trace — see {@link ReplyWriter}. Never
   * a way to *shorten* a reply: the only caller passes a length it took before
   * the reply began.
   *
   * @param {number} at
   */
  truncate(at) {
    this.#length = at;
  }

  /** Drops what was written, keeping the allocation. */
  clear() {
    this.#length = 0;
  }

  /**
   * Makes room for `needed` more bytes and returns where they start, advancing
   * past them. Every write begins here, so no other method grows the buffer.
   *
   * @param {number} needed
   * @returns {number}
   */
  #take(needed) {
    if (this.#length + needed > this.#bytes.length) {
      let capacity = this.#bytes.length * 2;
      while (capacity < this.#length + needed) capacity *= 2;
      const grown = new Uint8Array(capacity);
      grown.set(this.bytes);
      this.#bytes = grown;
      this.#view = new DataView(grown.buffer);
    }
    const at = this.#length;
    this.#length += needed;
    return at;
  }

  // EVERY WRITE TAKES ITS OFFSET FIRST, ON ITS OWN LINE. `#take` may replace
  // `#bytes` and `#view` with the grown pair, and JavaScript evaluates the
  // member expression of a call before its arguments — so
  // `this.#view.setUint32(this.#take(4), …)` resolves `setUint32` against the
  // view the writer had *before* growing and writes into the buffer it just
  // stopped using. That was this file's first bug, and nothing caught it until
  // the corpus carried a payload big enough to grow: the whole canonical set
  // fitted in the initial buffer.

  /**
   * A single byte — a tag, or an enum code. Checked for [`putU32`]'s reason:
   * `DEVICE_TYPE.CPU` is `0`, so a misspelled lookup writing `undefined` would
   * arrive as a plausible answer rather than as a malformed one.
   *
   * @param {number} value
   * @param {string} field
   */
  putU8(value, field) {
    if (!Number.isInteger(value) || value < 0 || value > 0xff) {
      throw new ReplyEncodeError(
        'NotANumber',
        `${field} must be an integer in 0..256, not ${String(value)}`,
        { field }
      );
    }
    const at = this.#take(1);
    this.#view.setUint8(at, value);
  }

  /** @param {number} value */
  putU16(value) {
    const at = this.#take(2);
    this.#view.setUint16(at, value, true);
  }

  /**
   * A `u32`, refusing anything that is not one.
   *
   * `setUint32` would take `undefined`, `NaN` or `-1` and write a number the far
   * side cannot tell from a real answer — see the header. The check is here
   * rather than at each call site because there are now dozens of them.
   *
   * @param {number} value
   * @param {string} field
   */
  putU32(value, field) {
    if (!Number.isInteger(value) || value < 0 || value > 0xffff_ffff) {
      throw new ReplyEncodeError(
        'NotANumber',
        `${field} must be an integer in 0..2^32, not ${String(value)}`,
        { field }
      );
    }
    const at = this.#take(4);
    this.#view.setUint32(at, value, true);
  }

  /**
   * An `f32`. Finite, because the seam's two float limits are a ratio and a
   * tick period and neither has a meaning for `NaN` or an infinity — and
   * because `NaN` does not compare equal to itself, so one written here would
   * make every round-trip check on the far side fail in a way that reads as a
   * transport bug.
   *
   * The value is `f64` until it lands: what crosses is the nearest `f32`, so a
   * number that is not exactly representable is rounded here rather than
   * refused. Every value this file's callers pass is exact in `f32`.
   *
   * @param {number} value
   * @param {string} field
   */
  putF32(value, field) {
    if (typeof value !== 'number' || !Number.isFinite(value)) {
      throw new ReplyEncodeError(
        'NotANumber',
        `${field} must be a finite number, not ${String(value)}`,
        { field }
      );
    }
    const at = this.#take(4);
    this.#view.setFloat32(at, value, true);
  }

  /**
   * @param {bigint} value
   * @param {string} field
   */
  putU64(value, field) {
    if (typeof value !== 'bigint') {
      throw new ReplyEncodeError(
        'NotABigInt',
        `${field} must be a BigInt: a number is exact only to 2^53`,
        { field }
      );
    }
    const at = this.#take(8);
    this.#view.setBigUint64(at, BigInt.asUintN(64, value), true);
  }

  /** @param {Uint8Array} bytes */
  putRaw(bytes) {
    const at = this.#take(bytes.length);
    this.#bytes.set(bytes, at);
  }

  /**
   * A handle, as the `u64` `Handle::to_bits` packs: index low, generation high.
   * A zero generation is what `Handle::from_bits` rejects, so it is refused
   * here rather than sent to be refused there.
   *
   * @param {{ index: number, generation: number }} handle
   * @param {string} field
   */
  putHandle(handle, field) {
    if (!handle || handle.generation === 0) {
      throw new ReplyEncodeError('NullHandle', `${field} is a null handle`, {
        field,
      });
    }
    const at = this.#take(8);
    this.#view.setUint32(at, handle.index, true);
    this.#view.setUint32(at + 4, handle.generation, true);
  }

  /**
   * A length-prefixed byte field, capped by `MAX_FIELD_BYTES`.
   *
   * @param {Uint8Array} bytes
   * @param {string} field
   */
  putBytes(bytes, field) {
    if (bytes.length > MAX_FIELD_BYTES) {
      throw new ReplyEncodeError(
        'InvalidLength',
        `${field} is ${bytes.length} bytes, past the stream's cap, which the reader enforces`,
        { field, len: bytes.length }
      );
    }
    this.putU32(bytes.length, field);
    this.putRaw(bytes);
  }

  /**
   * A string, as its UTF-8 bytes. The cap is on the *bytes*, not the code
   * points: a name of astrophysical length in Japanese is three times as long
   * on the wire as `length` suggests.
   *
   * @param {string} text
   * @param {string} field
   */
  putString(text, field) {
    this.putBytes(this.#utf8.encode(text), field);
  }

  /**
   * An element count, capped by `MAX_ELEMENT_COUNT`.
   *
   * @param {number} count
   * @param {string} field
   */
  putCount(count, field) {
    if (count > MAX_ELEMENT_COUNT) {
      throw new ReplyEncodeError(
        'InvalidLength',
        `${field} has ${count} elements, past the stream's cap, which the reader enforces`,
        { field, len: count }
      );
    }
    this.putU32(count, field);
  }

  /**
   * A counted list of enum codes: the count, then one byte per element.
   *
   * One method for the three lists a `SurfaceCaps` carries rather than three
   * loops, because they are the same knowledge and not merely the same shape —
   * the cap, the element width and the ordering rule are one decision.
   *
   * The elements go through {@link ByteWriter#putU8}, so a code that came out
   * of a misspelled table lookup arrives as `undefined` and is refused here
   * rather than written as `0` — which is `FORMAT.R8_UNORM`,
   * `PRESENT_MODE.FIFO` and `COMPOSITE_ALPHA.OPAQUE`, all three of them
   * plausible answers.
   *
   * @param {ArrayLike<number>} items
   * @param {string} field
   */
  putEnumList(items, field) {
    if (!Array.isArray(items)) {
      throw new ReplyEncodeError('NotANumber', `${field} must be an array`, {
        field,
      });
    }
    this.putCount(items.length, field);
    for (const code of items) this.putU8(code, field);
  }

  /**
   * `crcbl_hal::SurfaceCaps`, field by field in declaration order.
   *
   * THE THREE LISTS KEEP THEIR ORDER. `SurfaceCaps::formats` is documented as
   * best first and `preferred_format` reads it that way, so the order is a
   * value rather than a presentation detail — a writer that sorted or
   * de-duplicated would change which format a swapchain is created with.
   *
   * `currentExtent` IS A PRESENCE BYTE, NOT A SENTINEL, and `null` is the only
   * spelling of absent this method takes. `undefined` is refused for the
   * header's reason — a key that never arrived would otherwise encode as a
   * surface that reports no size, which is a real answer — and no numeric pair
   * could stand in for absent either: `[0, 0]` is what an unconfigured or
   * minimised window reports, and Vulkan's `0xFFFFFFFF` pair is a value
   * `crcbl-hal` requires a backend to turn into `None` before it reaches the
   * seam at all.
   *
   * @param {{
   *   formats: number[],
   *   presentModes: number[],
   *   compositeAlpha: number[],
   *   minImageCount: number,
   *   maxImageCount: number,
   *   currentExtent: [number, number] | null,
   * }} caps
   */
  putSurfaceCaps(caps) {
    this.putEnumList(caps.formats, 'SurfaceCaps::formats');
    this.putEnumList(caps.presentModes, 'SurfaceCaps::present_modes');
    this.putEnumList(caps.compositeAlpha, 'SurfaceCaps::composite_alpha');
    this.putU32(caps.minImageCount, 'SurfaceCaps::min_image_count');
    this.putU32(caps.maxImageCount, 'SurfaceCaps::max_image_count');
    const extent = caps.currentExtent;
    if (extent === null) {
      this.putU8(ABSENT, 'SurfaceCaps::current_extent');
      return;
    }
    if (!Array.isArray(extent) || extent.length !== 2) {
      throw new ReplyEncodeError(
        'NotANumber',
        'SurfaceCaps::current_extent must be two numbers or null',
        { field: 'SurfaceCaps::current_extent' }
      );
    }
    this.putU8(PRESENT, 'SurfaceCaps::current_extent');
    this.putU32(extent[0], 'SurfaceCaps::current_extent');
    this.putU32(extent[1], 'SurfaceCaps::current_extent');
  }

  /**
   * `crcbl_hal::Limits`, field by field in declaration order.
   *
   * EVERY FIELD, not the handful the engine reads today. `crcbl-render` reads
   * four of them and `crcbl-hal`'s own descriptor validation reads nine more,
   * and a field left off the wire is one the far side would have to invent a
   * ceiling for. `crates/crcbl-webgpu/src/reply.rs` holds the same list in the
   * same order; the committed fixture is what keeps the two identical.
   *
   * The five `u64` fields are `BigInt` for the header's reason. The two `f32`s
   * are plain numbers, since neither has a range a `double` cannot hold.
   *
   * @param {import('./gpu-replay.js').HalLimits} limits
   */
  putLimits(limits) {
    this.putU32(limits.maxImage2d, 'Limits::max_image_2d');
    this.putU32(limits.maxImage3d, 'Limits::max_image_3d');
    this.putU32(limits.maxImageArrayLayers, 'Limits::max_image_array_layers');
    this.putU64(
      limits.maxStorageBufferRange,
      'Limits::max_storage_buffer_range'
    );
    this.putU64(
      limits.maxUniformBufferRange,
      'Limits::max_uniform_buffer_range'
    );
    this.putU32(limits.maxBindGroups, 'Limits::max_bind_groups');
    this.putU32(
      limits.maxBindlessDescriptors,
      'Limits::max_bindless_descriptors'
    );
    this.putU32(limits.maxPushConstantSize, 'Limits::max_push_constant_size');
    this.putU32(limits.maxColorAttachments, 'Limits::max_color_attachments');
    this.putU32(limits.maxSampleCount, 'Limits::max_sample_count');
    this.putU32(limits.maxDrawIndirectCount, 'Limits::max_draw_indirect_count');
    const workgroup = limits.maxComputeWorkgroupSize;
    if (!Array.isArray(workgroup) || workgroup.length !== 3) {
      throw new ReplyEncodeError(
        'NotANumber',
        'Limits::max_compute_workgroup_size must be three numbers',
        { field: 'Limits::max_compute_workgroup_size' }
      );
    }
    for (const axis of workgroup) {
      this.putU32(axis, 'Limits::max_compute_workgroup_size');
    }
    this.putU32(
      limits.maxComputeInvocationsPerWorkgroup,
      'Limits::max_compute_invocations_per_workgroup'
    );
    this.putU32(
      limits.maxComputeWorkgroupsPerDimension,
      'Limits::max_compute_workgroups_per_dimension'
    );
    this.putU64(
      limits.minUniformBufferOffsetAlignment,
      'Limits::min_uniform_buffer_offset_alignment'
    );
    this.putU64(
      limits.minStorageBufferOffsetAlignment,
      'Limits::min_storage_buffer_offset_alignment'
    );
    this.putU64(
      limits.optimalBufferCopyOffsetAlignment,
      'Limits::optimal_buffer_copy_offset_alignment'
    );
    this.putF32(limits.maxSamplerAnisotropy, 'Limits::max_sampler_anisotropy');
    this.putF32(limits.timestampPeriodNs, 'Limits::timestamp_period_ns');
  }

  /**
   * `crcbl_hal::DeviceCaps`: the feature bits, then every limit.
   *
   * The half of an adapter record that is also a device record, written through
   * one method for that reason — `crates/crcbl-webgpu/src/reply.rs` has the same
   * pairing, and two copies would be two places for the field order to drift.
   *
   * @param {{ features: bigint, limits: import('./gpu-replay.js').HalLimits }} caps
   */
  putDeviceCaps(caps) {
    this.putU64(caps.features, 'DeviceCaps::features');
    this.putLimits(caps.limits);
  }

  /**
   * `crcbl_hal::AdapterInfo` in declaration order, `backend` omitted and `caps`
   * expanded in place.
   *
   * `backend` IS NOT ON THE WIRE. It says which crate enumerated the adapter,
   * not anything about the adapter, so the far side answers it with its own
   * identity — `BackendKind::WebGpu`, because `crcbl-webgpu` is what decoded.
   * Sending it would let this file claim to be Vulkan and be believed.
   *
   * @param {import('./gpu-replay.js').HalAdapterInfo} info
   */
  putAdapterInfo(info) {
    this.putU32(info.id, 'AdapterInfo::id');
    this.putString(info.name, 'Adapter::name');
    this.putU32(info.vendorId, 'AdapterInfo::vendor_id');
    this.putU32(info.deviceId, 'AdapterInfo::device_id');
    this.putU8(info.deviceType, 'AdapterInfo::device_type');
    this.putString(info.driver, 'Adapter::driver');
    this.putDeviceCaps(info);
  }
}

// ── ReplyWriter ──────────────────────────────────────────────────────────────

/**
 * Encodes the replies wasm is waiting for into one buffer.
 *
 * One writer per frame's worth of answers: encode as they arrive, hand
 * {@link ReplyWriter#bytes} to `putReplyStream` in `gpu-transport.js` at the
 * `requestAnimationFrame` boundary, then {@link ReplyWriter#clear} — or, if
 * wasm would not take them this frame, keep them and offer the same buffer
 * again next frame.
 *
 * Every method takes the sequence number of the command it answers, as a
 * `BigInt`. It comes from the command stream: `StreamReader` hands each command
 * out with its own `sequence`, and a replayer that needs to answer one keeps
 * that number.
 *
 * @throws {ReplyEncodeError} From any method, for a payload past a cap, a
 *   handle with a zero generation, or a sequence that is not a `BigInt`.
 */
export class ReplyWriter {
  /** @type {ByteWriter} */
  #writer = new ByteWriter();

  constructor() {
    this.#putHeader();
  }

  /** The encoded replies, header included, as a view over this writer's buffer. */
  get bytes() {
    return this.#writer.bytes;
  }

  /** Drops the encoded replies, leaving a fresh header. */
  clear() {
    this.#writer.clear();
    this.#putHeader();
  }

  #putHeader() {
    this.#writer.putRaw(REPLY_MAGIC);
    this.#writer.putU16(REPLY_VERSION);
  }

  /**
   * Writes one whole reply, or none of it.
   *
   * A REFUSED REPLY MUST LEAVE NO BYTES BEHIND. A record is written field by
   * field and any field can be refused, so a throw part-way through would leave
   * a truncated reply in the buffer — and the caller that catches it goes on to
   * write a *different* reply after it, because a dropped reply is a command
   * wasm waits on for ever. That is exactly what `gpu-replay.js` does: an
   * adapter record that will not encode is caught and answered with a
   * `NoAdapter` instead. Without this, the buffer it sends would be half an
   * adapter followed by a refusal, and the far side would refuse the whole
   * frame's replies as undecodable.
   *
   * @param {() => void} write
   */
  #atomic(write) {
    const before = this.#writer.length;
    try {
      write();
    } catch (error) {
      this.#writer.truncate(before);
      throw error;
    }
  }

  /**
   * Opens a reply: its tag, then the sequence it answers.
   *
   * @param {number} tag
   * @param {bigint} sequence
   */
  #open(tag, sequence) {
    this.#writer.putU8(tag, 'Reply::tag');
    this.#writer.putU64(sequence, 'Reply::sequence');
  }

  /**
   * One entry of an adapter enumeration — the whole of `AdapterInfo`.
   *
   * Build the argument with `halAdapterInfoFor` in `gpu-replay.js`, which is
   * where WebGPU's vocabulary is translated into this one and where the fields
   * WebGPU cannot supply get the values that *mean absent* rather than values
   * that look real. This method only writes what it is given.
   *
   * `info.backend` is neither taken nor written; see
   * {@link ByteWriter#putAdapterInfo}.
   *
   * @param {bigint} sequence
   * @param {import('./gpu-replay.js').HalAdapterInfo} info
   */
  adapter(sequence, info) {
    this.#atomic(() => {
      this.#open(ADAPTER_REPLY_TAG, sequence);
      this.#writer.putAdapterInfo(info);
    });
  }

  /**
   * The enumeration found nothing, with the reason the browser gave.
   *
   * Not an `adapter` call carrying a sentinel: an enumeration is answered
   * exactly once — wasm refuses a second reply naming a sequence it has already
   * had one for — and an empty `name` is a browser that granted an adapter and
   * declined to name it, which is a different fact.
   *
   * @param {bigint} sequence
   * @param {string} reason What `requestAdapter()` said, for a log or a banner.
   */
  noAdapter(sequence, reason) {
    this.#atomic(() => {
      this.#open(NO_ADAPTER_REPLY_TAG, sequence);
      this.#writer.putString(reason, 'NoAdapter::reason');
    });
  }

  /**
   * A device opened, with what **that device** can do.
   *
   * Not the adapter's capabilities. WebGPU grants a device the features that
   * were asked for and the limits that were requested — the specification's
   * defaults when a request names none — so an adapter with `timestamp-query`
   * and a 16384-pixel texture yields a device with neither unless the
   * descriptor said so. Build the argument with `halDeviceCapsFor` in
   * `gpu-replay.js`, which reads them off the `GPUDevice` rather than off the
   * adapter it came from.
   *
   * @param {bigint} sequence
   * @param {{ features: bigint, limits: import('./gpu-replay.js').HalLimits }} caps
   */
  device(sequence, caps) {
    this.#atomic(() => {
      this.#open(DEVICE_REPLY_TAG, sequence);
      this.#writer.putDeviceCaps(caps);
    });
  }

  /**
   * No device opened, with the reason and the features that were missing.
   *
   * `unsupported` is a `crcbl_hal::Features` word — the machine-readable half of
   * `reason`, and empty when the failure was not about features at all. This
   * side knows bits and `GPUFeatureName`s; the flag *names* belong to
   * `crcbl-hal`, which is why the word crosses rather than a phrase built here.
   *
   * A REQUEST THAT FAILED IS NOT A DEVICE THAT WAS LOST. This answers a
   * `RequestDevice` that never produced a device. A `GPUDevice.lost` on a device
   * that is open and working is a different event with a different home —
   * `Device::take_error` — and there is no reply for it yet.
   *
   * @param {bigint} sequence
   * @param {string} reason
   * @param {bigint} unsupported `crcbl_hal::Features` bits, `0n` for none.
   */
  deviceFailed(sequence, reason, unsupported) {
    this.#atomic(() => {
      this.#open(DEVICE_FAILED_REPLY_TAG, sequence);
      this.#writer.putString(reason, 'DeviceFailed::reason');
      this.#writer.putU64(unsupported, 'DeviceFailed::unsupported');
    });
  }

  /**
   * What a surface will accept on the adapter the command named.
   *
   * The whole of `crcbl_hal::SurfaceCaps`; see
   * {@link ByteWriter#putSurfaceCaps} for the field order, why the three lists
   * keep theirs, and why `currentExtent` is `null` rather than a sentinel when
   * there is no answer — which, in a browser, is always: WebGPU has no
   * `currentExtent` query, so a replayer's own size comes from the canvas and
   * belongs to the shell rather than to this record.
   *
   * @param {bigint} sequence
   * @param {Parameters<ByteWriter['putSurfaceCaps']>[0]} caps
   */
  surfaceCaps(sequence, caps) {
    this.#atomic(() => {
      this.#open(SURFACE_CAPS_REPLY_TAG, sequence);
      this.#writer.putSurfaceCaps(caps);
    });
  }

  /**
   * No capabilities, with the reason and which failure it was.
   *
   * `cause` is one of {@link SURFACE_CAPS_FAILURE} — the machine-readable half
   * of `reason`, and the field an `impl Instance` turns into the matching
   * `HalError`. It goes through `putU8`, so a code that came out of a misspelled
   * lookup is `undefined` and refused here rather than written as `0` — which is
   * `BACKEND`, the one legal code, so a mistake would be indistinguishable from
   * the intended value on the wire.
   *
   * A REFUSAL IS AN ORDINARY ANSWER, not an error path. `surface_caps` is how
   * adapter selection is done, so a caller asks it about adapters expecting some
   * of them to say no — which is why this reply exists instead of the replayer
   * throwing and taking the frame with it.
   *
   * @param {bigint} sequence
   * @param {string} reason
   * @param {number} cause One of {@link SURFACE_CAPS_FAILURE}.
   */
  surfaceCapsFailed(sequence, reason, cause) {
    this.#atomic(() => {
      this.#open(SURFACE_CAPS_FAILED_REPLY_TAG, sequence);
      this.#writer.putString(reason, 'SurfaceCapsFailed::reason');
      this.#writer.putU8(cause, 'SurfaceCapsFailed::cause');
    });
  }

  /**
   * `poll_readback` answering `Pending`: the bytes are not there yet.
   *
   * @param {bigint} sequence
   * @param {{ index: number, generation: number }} readback
   */
  readbackPending(sequence, readback) {
    this.#atomic(() => {
      this.#open(READBACK_PENDING_REPLY_TAG, sequence);
      this.#writer.putHandle(readback, 'ReadbackPending::readback');
    });
  }

  /**
   * `poll_readback` answering `Ready`, with the bytes.
   *
   * The length is the payload's own. `poll_readback`'s contract is exactly
   * `ReadbackDesc::size` bytes, and the far side checks the length it got
   * against the descriptor it kept — nothing in the buffer says what that was.
   *
   * @param {bigint} sequence
   * @param {{ index: number, generation: number }} readback
   * @param {Uint8Array} data
   */
  readbackReady(sequence, readback, data) {
    this.#atomic(() => {
      this.#open(READBACK_READY_REPLY_TAG, sequence);
      this.#writer.putHandle(readback, 'ReadbackReady::readback');
      this.#writer.putBytes(data, 'ReadbackReady::data');
    });
  }

  /**
   * `query_results`: raw values, one per query, in query order.
   *
   * @param {bigint} sequence
   * @param {{ index: number, generation: number }} set
   * @param {number} firstQuery
   * @param {ArrayLike<bigint>} values
   */
  queryResults(sequence, set, firstQuery, values) {
    this.#atomic(() => {
      this.#open(QUERY_RESULTS_REPLY_TAG, sequence);
      this.#writer.putHandle(set, 'QueryResults::set');
      this.#writer.putU32(firstQuery, 'QueryResults::first_query');
      this.#writer.putCount(values.length, 'QueryResults::values');
      for (let i = 0; i < values.length; i += 1) {
        this.#writer.putU64(values[i], 'QueryResults::values');
      }
    });
  }
}
