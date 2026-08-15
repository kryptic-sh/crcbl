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
// session, and a query result is a raw GPU tick count, so both are `BigInt` and
// a number is refused rather than rounded. Everything else on the wire is 32
// bits or narrower and stays a number.

// ── Header ───────────────────────────────────────────────────────────────────

/** `tag::REPLY_MAGIC` — the ASCII bytes every reply buffer starts with. */
const REPLY_MAGIC = new Uint8Array([
  0x43, 0x52, 0x43, 0x42, 0x4c, 0x52, 0x50, 0x4c,
]); // "CRCBLRPL"

/** `tag::REPLY_VERSION`. */
const REPLY_VERSION = 1;

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
const READBACK_PENDING_REPLY_TAG = 0x10;
const READBACK_READY_REPLY_TAG = 0x11;
const QUERY_RESULTS_REPLY_TAG = 0x18;

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
   * @param {'InvalidLength'|'NullHandle'|'NotABigInt'} kind
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

  /** @param {number} value */
  putU8(value) {
    const at = this.#take(1);
    this.#view.setUint8(at, value);
  }

  /** @param {number} value */
  putU16(value) {
    const at = this.#take(2);
    this.#view.setUint16(at, value, true);
  }

  /** @param {number} value */
  putU32(value) {
    const at = this.#take(4);
    this.#view.setUint32(at, value, true);
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
    this.putU32(bytes.length);
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
    this.putU32(count);
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
   * Opens a reply: its tag, then the sequence it answers.
   *
   * @param {number} tag
   * @param {bigint} sequence
   */
  #open(tag, sequence) {
    this.#writer.putU8(tag);
    this.#writer.putU64(sequence, 'Reply::sequence');
  }

  /**
   * One entry of an adapter enumeration — a partial `AdapterInfo`.
   *
   * @param {bigint} sequence
   * @param {number} id Position in the enumeration.
   * @param {string} name Human-readable device name.
   */
  adapter(sequence, id, name) {
    this.#open(ADAPTER_REPLY_TAG, sequence);
    this.#writer.putU32(id);
    this.#writer.putString(name, 'Adapter::name');
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
    this.#open(NO_ADAPTER_REPLY_TAG, sequence);
    this.#writer.putString(reason, 'NoAdapter::reason');
  }

  /**
   * `poll_readback` answering `Pending`: the bytes are not there yet.
   *
   * @param {bigint} sequence
   * @param {{ index: number, generation: number }} readback
   */
  readbackPending(sequence, readback) {
    this.#open(READBACK_PENDING_REPLY_TAG, sequence);
    this.#writer.putHandle(readback, 'ReadbackPending::readback');
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
    this.#open(READBACK_READY_REPLY_TAG, sequence);
    this.#writer.putHandle(readback, 'ReadbackReady::readback');
    this.#writer.putBytes(data, 'ReadbackReady::data');
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
    this.#open(QUERY_RESULTS_REPLY_TAG, sequence);
    this.#writer.putHandle(set, 'QueryResults::set');
    this.#writer.putU32(firstQuery);
    this.#writer.putCount(values.length, 'QueryResults::values');
    for (let i = 0; i < values.length; i += 1) {
      this.#writer.putU64(values[i], 'QueryResults::values');
    }
  }
}
