#!/usr/bin/env node
// The other half of the command stream's drift check.
//
// `crcbl-webgpu` encodes the stream in Rust; `web/engine/gpu-stream.js` decodes
// it in JavaScript, and that decoder is the production one — the Rust reader
// exists to test the encoding, not to run in a browser. Two hand-written halves
// of one format will drift, and no compiler anywhere reads both.
//
// So the two are pinned to a third thing they both have to agree with:
// `crates/crcbl-webgpu/tests/fixture.rs` freezes the canonical stream into
// `tests/fixtures/canonical-stream.bin`, and this file decodes those bytes and
// asserts every field of every command against a list written out here. Change
// a tag, a field order or a cap in Rust and the fixture test goes red; make the
// same change only in JavaScript and this one does.
//
// The expected commands below are therefore deliberately spelled out rather
// than derived from anything the decoder produces. A list built by decoding
// would agree with any decoder, including a wrong one.
//
// It also asserts what a decoder must do with a stream that is *not* the
// fixture: a truncation, a corrupt tag, a length prefix past the cap and a
// presence byte that is neither canonical value each have to come back as a
// typed `StreamDecodeError`, never as a `TypeError` out of an index that ran
// off the end and never as a decode that quietly succeeded.
//
// Usage:
//   node web/tools/stream-decode.mjs [path-to-fixture.bin]

import { readFile } from 'node:fs/promises';
import { deepStrictEqual } from 'node:assert/strict';

import {
  StreamDecodeError,
  StreamReader,
  decodeStream,
} from '../engine/gpu-stream.js';

/** The fixture `crcbl-webgpu`'s `fixture.rs` writes. */
const FIXTURE = new URL(
  '../../crates/crcbl-webgpu/tests/fixtures/canonical-stream.bin',
  import.meta.url
);

/** `tag::HEADER_BYTES`: the magic, the version word, and the base sequence. */
const HEADER_BYTES = 8 + 2 + 8;

/** `tag::PRESENT` — the only byte other than `tag::ABSENT` a presence field may hold. */
const PRESENT = 1;

/** `tag::MAX_FIELD_BYTES`. */
const MAX_FIELD_BYTES = 1 << 20;

/** `tag::MAX_ELEMENT_COUNT`. */
const MAX_ELEMENT_COUNT = 1 << 16;

// The tags the hand-built streams below open with. Restated here rather than
// imported for the reason the expected commands are: a value taken from the
// decoder agrees with the decoder by construction.
const BIND_GROUP_TAG = 0x43;
const PUSH_CONSTANTS_TAG = 0x44;
const DRAW_TAG = 0x60;
const REQUEST_DEVICE_TAG = 0x91;

/** @type {string[]} */
const failures = [];

/**
 * @param {boolean} condition
 * @param {string} what
 */
function check(condition, what) {
  if (condition) console.log(`  ok   ${what}`);
  else {
    console.log(`  FAIL ${what}`);
    failures.push(what);
  }
}

/**
 * Field-for-field, through the standard library's comparator so a `BigInt`, a
 * `Uint8Array` and a `null` are each held to what they are rather than to what
 * they coerce to.
 *
 * @param {unknown} actual
 * @param {unknown} expected
 * @param {string} what
 */
function checkEqual(actual, expected, what) {
  try {
    deepStrictEqual(actual, expected);
    check(true, what);
  } catch (error) {
    const detail = String(error instanceof Error ? error.message : error)
      .split('\n')
      .map((line) => `       ${line}`)
      .join('\n');
    check(false, `${what}\n${detail}`);
  }
}

/**
 * What decoding `bytes` threw, or `null` if it did not throw.
 *
 * @param {Uint8Array} bytes
 * @returns {unknown}
 */
function failureOf(bytes) {
  try {
    decodeStream(bytes);
    return null;
  } catch (error) {
    return error;
  }
}

/**
 * Asserts that `bytes` is refused with a typed error carrying the named fields.
 *
 * `expected` holds `kind` plus whichever `details` keys matter; nothing else is
 * compared, so a test does not have to restate a whole error to pin one field.
 *
 * @param {Uint8Array} bytes
 * @param {Record<string, unknown>} expected
 * @param {string} what
 */
function checkRefused(bytes, expected, what) {
  const error = failureOf(bytes);
  if (error === null) {
    check(false, `${what} (decoded cleanly instead)`);
    return;
  }
  if (!(error instanceof StreamDecodeError)) {
    const named =
      error instanceof Error
        ? `${error.name}: ${error.message}`
        : String(error);
    check(false, `${what} (threw ${named} rather than a StreamDecodeError)`);
    return;
  }
  /** @type {Record<string, unknown>} */
  const actual = { kind: error.kind };
  for (const key of Object.keys(expected)) {
    if (key !== 'kind') actual[key] = error.details[key];
  }
  checkEqual(actual, expected, what);
}

/**
 * @param {number} index
 * @param {number} generation
 */
function handle(index, generation) {
  return { index, generation };
}

/**
 * Every command in the fixture, in order — the JavaScript statement of
 * `corpus::every_command` in `crates/crcbl-webgpu/tests/corpus/mod.rs`.
 *
 * No two fields share a value there, deliberately, so a decoder that reads two
 * of them in the wrong order does not still compare equal here.
 */
const EXPECTED = [
  {
    name: 'CreateBuffer',
    buffer: handle(11, 12),
    label: 'instances',
    size: 4096n,
    usage: ['TRANSFER_DST', 'STORAGE'],
    memory: 'DeviceLocal',
  },
  {
    name: 'CreateBuffer',
    buffer: handle(13, 14),
    // Absent, and the next one is present-and-empty. The two must not converge.
    label: null,
    size: 1n,
    usage: ['UNIFORM'],
    memory: 'HostUpload',
  },
  {
    name: 'CreateBuffer',
    buffer: handle(15, 16),
    label: '',
    // `WHOLE_BUFFER`. A JS number would round this to 18446744073709552000.
    size: 18446744073709551615n,
    usage: ['TRANSFER_SRC'],
    memory: 'HostReadback',
  },
  // The canvas key is its own field beside the handle, and its value differs
  // from both halves of that handle: a decoder that read it where a handle half
  // is, or dropped it, would not still compare equal here.
  { name: 'CreateSurface', surface: handle(45, 46), canvasId: 19 },
  { name: 'DestroyBuffer', buffer: handle(17, 18) },
  { name: 'DestroySurface', surface: handle(47, 48) },
  { name: 'BeginDebugLabel', label: 'gbuffer — ✱' },
  {
    name: 'BeginRenderPass',
    label: 'shading',
    colorAttachments: [
      {
        view: handle(21, 22),
        resolve: handle(23, 24),
        load: 'Clear',
        store: 'Store',
        clear: { color: [0.25, 0.5, 0.75, 1], depth: 0, stencil: 7 },
      },
      {
        view: handle(25, 26),
        resolve: null,
        load: 'DontCare',
        store: 'Discard',
        clear: { color: [0, 0, 0, 0], depth: 0, stencil: 0 },
      },
    ],
    depthStencilAttachment: {
      view: handle(27, 28),
      readOnly: true,
      depthLoad: 'Load',
      depthStore: 'Discard',
      stencilLoad: 'Clear',
      stencilStore: 'Store',
      // `depth::NEAR` is 1.0 and `depth::CLEAR` is 0.0: the depth buffer is
      // reversed-Z, so the clear above is not the conventional 1.0.
      clear: { color: [1, 2, 3, 4], depth: 1, stencil: 9 },
    },
    renderArea: { x: -3, y: -5, width: 1920, height: 1080 },
  },
  {
    name: 'BeginRenderPass',
    label: null,
    colorAttachments: [],
    depthStencilAttachment: null,
    renderArea: { x: 0, y: 0, width: 2, height: 3 },
  },
  { name: 'BindGraphicsPipeline', pipeline: handle(31, 32) },
  {
    name: 'BindGroup',
    slot: 2,
    group: handle(33, 34),
    dynamicOffsets: [256, 512, 768],
    layout: handle(35, 36),
  },
  {
    name: 'BindGroup',
    slot: 0,
    group: handle(37, 38),
    dynamicOffsets: [],
    layout: handle(39, 40),
  },
  {
    name: 'PushConstants',
    stages: ['VERTEX', 'FRAGMENT'],
    offset: 16,
    data: new Uint8Array([0xde, 0xad, 0xbe, 0xef, 0x00, 0x01]),
    layout: handle(41, 42),
  },
  {
    name: 'Draw',
    vertices: { start: 6, end: 9 },
    instances: { start: 1, end: 5 },
  },
  // Both feature words are `BigInt`, and the required one carries
  // `TIMELINE_SEMAPHORE` (1 << 9) — a flag WebGPU cannot satisfy, which crosses
  // anyway because the replayer is what refuses it.
  {
    name: 'RequestDevice',
    adapter: 3,
    label: 'device',
    // COMPUTE (1 << 8) | TIMELINE_SEMAPHORE (1 << 9).
    requiredFeatures: 0x300n,
    // TIMESTAMP_QUERY (1 << 5) | TEXTURE_COMPRESSION_BC (1 << 16).
    optionalFeatures: 0x10020n,
    compatibleSurface: handle(43, 44),
  },
  {
    name: 'RequestDevice',
    adapter: 0,
    label: null,
    requiredFeatures: 0n,
    // `Features::all()` — every bit the seam claims, and what pins the
    // claimed-bit mask in `gpu-stream.js`: a mask narrower than Rust's refuses
    // this command outright.
    optionalFeatures: 0x7ffffffn,
    compatibleSurface: null,
  },
  // The only body-less command, and last in the corpus so that the byte offsets
  // the checks below count from the *first* command stay where they are. A
  // decoder that read one field too many here would run off the end of the
  // buffer, which is what the truncation sweep sees.
  { name: 'EnumerateAdapters' },
];

/**
 * `value` as the four little-endian bytes the wire carries it as.
 *
 * @param {number} value
 * @returns {number[]}
 */
function u32le(value) {
  return [
    value & 0xff,
    (value >>> 8) & 0xff,
    (value >>> 16) & 0xff,
    (value >>> 24) & 0xff,
  ];
}

/**
 * `value` as the eight little-endian bytes the wire carries a `u64` as.
 *
 * @param {bigint} value
 * @returns {number[]}
 */
function u64le(value) {
  const bytes = [];
  for (let at = 0n; at < 8n; at += 1n) {
    bytes.push(Number((value >> (at * 8n)) & 0xffn));
  }
  return bytes;
}

/**
 * A hand-built stream: a real header, then `body`. The header is taken from the
 * fixture so these cases test a command body and nothing else.
 *
 * @param {Uint8Array} header
 * @param {number[]} body
 * @returns {Uint8Array}
 */
function streamOf(header, body) {
  const bytes = new Uint8Array(HEADER_BYTES + body.length);
  bytes.set(header);
  bytes.set(body, HEADER_BYTES);
  return bytes;
}

/**
 * @param {Uint8Array} fixture
 * @param {number} at
 * @param {number} byte
 * @returns {Uint8Array}
 */
function withByte(fixture, at, byte) {
  const copy = fixture.slice();
  copy[at] = byte;
  return copy;
}

async function main() {
  const override = process.argv.slice(2).find((arg) => !arg.startsWith('--'));
  const path = override === undefined ? FIXTURE : override;
  const fixture = new Uint8Array(await readFile(path));

  console.log(
    `stream-decode: ${override ?? FIXTURE.pathname} (${fixture.length} bytes)`
  );

  // ---- the fixture decodes to exactly the commands that were encoded -------
  /** @type {object[]} */
  let decoded;
  try {
    decoded = decodeStream(fixture);
  } catch (error) {
    // The first thing a drifted tag or field order does is refuse the fixture
    // outright, and nothing below this line means anything once it has. Caught
    // so that lands as a failing check rather than as a stack trace.
    check(false, `the fixture decodes at all (threw ${String(error)})`);
    console.error(`\nstream-decode: FAILED (${failures.length})`);
    process.exit(1);
  }
  check(
    decoded.length === EXPECTED.length,
    `the fixture holds ${EXPECTED.length} commands (decoded ${decoded.length})`
  );
  for (const [index, expected] of EXPECTED.entries()) {
    checkEqual(
      decoded[index],
      expected,
      `command ${index} is ${expected.name}, field for field`
    );
  }

  // ---- sequence numbers are positional from the header --------------------
  const reader = new StreamReader(fixture);
  check(reader.baseSequence === 0n, 'the header declares base sequence 0');
  /** @type {bigint[]} */
  const sequences = [];
  for (
    let next = reader.nextCommand();
    next !== null;
    next = reader.nextCommand()
  ) {
    sequences.push(next.sequence);
  }
  checkEqual(
    sequences,
    EXPECTED.map((_, index) => BigInt(index)),
    'the nth command carries base + n, with nothing per command on the wire'
  );

  // ---- the empty label is not the absent one ------------------------------
  // Both are `CreateBuffer`s above; this states the distinction on its own so a
  // decoder that collapsed them fails with a message that says which rule.
  const labels = decoded
    .filter((command) => command.name === 'CreateBuffer')
    .map((command) => command.label);
  checkEqual(
    labels,
    ['instances', null, ''],
    'Some("") and None decode to distinct labels'
  );

  // ---- the byte payload is bytes, and a copy ------------------------------
  const push = decoded.find((command) => command.name === 'PushConstants');
  check(
    push !== undefined && push.data instanceof Uint8Array,
    'a push-constant block decodes to bytes'
  );
  check(
    push !== undefined && push.data.buffer !== fixture.buffer,
    'the payload is copied out rather than left as a view on the stream'
  );

  // ---- a truncation is short, never a partial or over-running decode -------
  // Every cut, not just one: this is what says no read anywhere in the decoder
  // runs off the end of the buffer it was handed.
  let sweep = null;
  for (
    let cut = HEADER_BYTES;
    cut < fixture.length && sweep === null;
    cut += 1
  ) {
    const short = fixture.subarray(0, cut);
    let commands;
    try {
      commands = decodeStream(short);
    } catch (error) {
      if (!(error instanceof StreamDecodeError)) {
        sweep = `truncating to ${cut} bytes threw ${String(error)}`;
      } else if (error.kind !== 'TooShort' && error.kind !== 'InvalidLength') {
        sweep = `truncating to ${cut} bytes gave ${error.kind}: ${error.message}`;
      }
      continue;
    }
    // A cut landing exactly on a command boundary is a shorter but perfectly
    // well-formed stream.
    if (commands.length >= EXPECTED.length) {
      sweep = `truncating to ${cut} bytes decoded the whole stream`;
    }
  }
  check(
    sweep === null,
    sweep ?? 'every truncation is short rather than a partial decode'
  );

  // ---- the body-less command really has no body ---------------------------
  // The fixture ends with `EnumerateAdapters`, whose entire encoding is its tag
  // byte, so cutting one byte off drops that command and leaves a shorter but
  // well-formed stream — while cutting two lands inside `Draw`'s last field and
  // is short. A decoder that read any body at all for the empty command would
  // fail the first of these, and one that read a byte too few would fail the
  // second.
  //
  // Stated as two cuts rather than one because that is what changed: the check
  // here used to be "one byte short is TooShort", which was true only for as
  // long as the last command in the corpus had a body.
  checkEqual(
    failureOf(fixture.subarray(0, fixture.length - 1)) ??
      decodeStream(fixture.subarray(0, fixture.length - 1)),
    EXPECTED.slice(0, -1),
    'cutting one byte drops the body-less command and decodes the rest'
  );
  checkRefused(
    fixture.subarray(0, fixture.length - 2),
    { kind: 'TooShort' },
    'a fixture cut inside a command body is TooShort'
  );

  // ---- a corrupt tag is unknown, not a malformed known command ------------
  // The stated reason the tag comes first: without it the two are one error.
  checkRefused(
    withByte(fixture, HEADER_BYTES, 0xff),
    { kind: 'UnknownTag', tag: 0xff },
    'a corrupted tag is reported as unknown'
  );

  // ---- a length prefix past the cap is refused, not allocated for ---------
  const header = fixture.subarray(0, HEADER_BYTES);
  // `push_constants` is the unbounded byte payload: tag, stages, offset, then
  // the length prefix.
  checkRefused(
    streamOf(header, [
      PUSH_CONSTANTS_TAG,
      ...u32le(1), // ShaderStages::VERTEX
      ...u32le(0), // offset
      ...u32le(0xffffffff), // the length prefix
    ]),
    { kind: 'InvalidLength', field: 'PushConstants::data', len: 0xffffffff },
    'a length prefix past MAX_FIELD_BYTES is refused'
  );
  // One byte past the cap, which pins the cap's *value*: a decoder whose cap
  // were larger would get as far as reading the bytes and answer TooShort.
  checkRefused(
    streamOf(header, [
      PUSH_CONSTANTS_TAG,
      ...u32le(1),
      ...u32le(0),
      ...u32le(MAX_FIELD_BYTES + 1),
    ]),
    {
      kind: 'InvalidLength',
      field: 'PushConstants::data',
      len: MAX_FIELD_BYTES + 1,
    },
    'the byte cap is MAX_FIELD_BYTES exactly, not merely some cap'
  );

  // `bind_group` is the element count: tag, slot, group handle, then the count.
  const someHandle = [...u32le(1), ...u32le(1)]; // index 1, generation 1
  checkRefused(
    streamOf(header, [
      BIND_GROUP_TAG,
      ...u32le(0),
      ...someHandle,
      ...u32le(0xffffffff),
    ]),
    {
      kind: 'InvalidLength',
      field: 'BindGroup::dynamic_offsets',
      len: 0xffffffff,
    },
    'an element count past MAX_ELEMENT_COUNT is refused'
  );
  // Under the cap and still dishonest: every element costs at least one byte,
  // so a count past what is left cannot be true. Neither bound catches this on
  // its own.
  checkRefused(
    streamOf(header, [
      BIND_GROUP_TAG,
      ...u32le(0),
      ...someHandle,
      ...u32le(100),
    ]),
    { kind: 'InvalidLength', field: 'BindGroup::dynamic_offsets', len: 100 },
    'an element count past the bytes left is refused even though it is under the cap'
  );
  // One element past the cap, *with* the bytes to make the second bound pass,
  // so the cap's own value is the only thing that can refuse it.
  checkRefused(
    streamOf(header, [
      BIND_GROUP_TAG,
      ...u32le(0),
      ...someHandle,
      ...u32le(MAX_ELEMENT_COUNT + 1),
      ...new Array(MAX_ELEMENT_COUNT + 1).fill(0),
    ]),
    {
      kind: 'InvalidLength',
      field: 'BindGroup::dynamic_offsets',
      len: MAX_ELEMENT_COUNT + 1,
    },
    'the element cap is MAX_ELEMENT_COUNT exactly, not merely some cap'
  );

  // ---- a handle field that may not be absent refuses zero bits ------------
  // `Option<Handle>` is a bare `u64` with zero for `None`, so the same eight
  // zero bytes have to mean two different things depending on the field — and
  // this is the field where they mean an error.
  const nulled = fixture.slice();
  nulled.fill(0, HEADER_BYTES + 1, HEADER_BYTES + 1 + 8);
  checkRefused(
    nulled,
    { kind: 'NullHandle', field: 'CreateBuffer::buffer' },
    'a handle field that cannot be absent refuses zero bits'
  );

  // ---- a presence byte that is neither value is an error, not truthy ------
  // The first command is a `CreateBuffer`: a tag, then `CreateBuffer::buffer`,
  // then the label's presence byte.
  const presenceAt = HEADER_BYTES + 1 + 8;
  check(
    fixture[presenceAt] === PRESENT,
    'the first label in the fixture is present (the byte the next check flips)'
  );
  checkRefused(
    withByte(fixture, presenceAt, 2),
    { kind: 'InvalidEnum', field: 'BufferDesc::label', code: 2 },
    'a presence byte of 2 is refused rather than read as truthy'
  );

  // ---- a label that is not UTF-8 is refused, not repaired -----------------
  // The nine bytes of `instances` follow that presence byte and its length
  // prefix; 0xFF starts no UTF-8 sequence.
  checkRefused(
    withByte(fixture, presenceAt + 1 + 4, 0xff),
    { kind: 'NotUtf8', field: 'BufferDesc::label' },
    'a label that is not UTF-8 is refused rather than filled with replacements'
  );

  // ---- a bit no flag claims is an error, not a truncation -----------------
  // The first command's usage word, reached over: the tag, the handle, the
  // label's presence byte and length prefix, the label itself, and the size.
  const usageAt = HEADER_BYTES + 1 + 8 + 1 + 4 + 'instances'.length + 8;
  const unclaimed = fixture.slice();
  new DataView(
    unclaimed.buffer,
    unclaimed.byteOffset,
    unclaimed.byteLength
  ).setUint32(usageAt, 0xffffffff, true);
  checkRefused(
    unclaimed,
    { kind: 'InvalidEnum', field: 'BufferDesc::usage', code: 0xffffffff },
    'a bitflags bit no BufferUsage claims is refused rather than truncated away'
  );

  // ---- a feature bit no flag claims is an error too -----------------------
  // The sixty-four bit version of the rule above, and the reason `readFeatures`
  // keeps a claimed-bit mask rather than waving the word through: a bit this
  // build does not know is a build that knows a flag this one does not, and
  // truncating it would move a *required* feature out of a request.
  const unclaimedFeature = 1n << 40n;
  checkRefused(
    streamOf(header, [
      REQUEST_DEVICE_TAG,
      ...u32le(0), // adapter
      0, // the label, absent
      ...u64le(unclaimedFeature),
    ]),
    {
      kind: 'InvalidEnum',
      field: 'DeviceDesc::required_features',
      code: unclaimedFeature,
    },
    'a Features bit no flag claims is refused rather than truncated away'
  );
  // …and the optional word is its own field, one further on: a decoder that
  // read one word for both would report this as the required one.
  checkRefused(
    streamOf(header, [
      REQUEST_DEVICE_TAG,
      ...u32le(0),
      0,
      ...u64le(0n),
      ...u64le(unclaimedFeature),
    ]),
    {
      kind: 'InvalidEnum',
      field: 'DeviceDesc::optional_features',
      code: unclaimedFeature,
    },
    'the two feature words are separate fields and are named separately'
  );

  // ---- an enum code no variant claims is refused --------------------------
  // The memory location is the byte after that usage word, and the last of the
  // body. Nothing may be folded into a neighbouring variant.
  checkRefused(
    withByte(fixture, usageAt + 4, 0x7f),
    { kind: 'InvalidEnum', field: 'BufferDesc::memory', code: 0x7f },
    'a MemoryLocation code no variant claims is refused'
  );

  // ---- a failed reader stays failed rather than resyncing mid-body --------
  // After a throw the cursor is somewhere inside a command body, so the next
  // byte is not a tag and resuming would invent commands out of a payload.
  const latched = new StreamReader(
    streamOf(header, [0xff, DRAW_TAG, ...new Array(16).fill(0)])
  );
  const latchedError = (() => {
    try {
      latched.nextCommand();
      return null;
    } catch (error) {
      return error;
    }
  })();
  check(
    latchedError instanceof StreamDecodeError &&
      latchedError.kind === 'UnknownTag',
    'a bad tag mid-stream throws out of the reader'
  );
  check(
    latched.nextCommand() === null,
    'a reader that has failed stays failed rather than resyncing inside a body'
  );

  // ---- the header gate ----------------------------------------------------
  checkRefused(
    withByte(fixture, 0, 0x00),
    { kind: 'BadMagic' },
    'a buffer that is not a command stream is refused by its magic'
  );
  // The version is a `u16` at offset 8, so its low byte alone says 2. The two
  // halves ship as separate artifacts and are cached independently, which is
  // what makes this reachable at all.
  checkRefused(
    withByte(fixture, 8, 2),
    { kind: 'UnsupportedVersion', found: 2, expected: 1 },
    'a stream from a build that speaks another version is refused'
  );

  // ---- no byte anywhere turns a decode into an indexing throw -------------
  // Every offset, set to 0xFF: the decode may fail, and may even still succeed,
  // but what it must never do is leave this module's own error type.
  let corruption = null;
  for (let at = 0; at < fixture.length && corruption === null; at += 1) {
    const error = failureOf(withByte(fixture, at, 0xff));
    if (error !== null && !(error instanceof StreamDecodeError)) {
      corruption = `byte ${at} set to 0xFF threw ${String(error)}`;
    }
  }
  check(
    corruption === null,
    corruption ??
      'every single-byte corruption is a typed decode error or a clean decode'
  );

  if (failures.length > 0) {
    console.error(`\nstream-decode: FAILED (${failures.length})`);
    process.exit(1);
  }
  console.log('\nstream-decode: OK');
}

await main();
