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
const CREATE_IMAGE_TAG = 0x02;
const CREATE_IMAGE_VIEW_TAG = 0x03;
const CREATE_SAMPLER_TAG = 0x04;
const BIND_GROUP_TAG = 0x43;
const PUSH_CONSTANTS_TAG = 0x44;
const DRAW_TAG = 0x60;
const CREATE_BIND_GROUP_LAYOUT_TAG = 0x05;
const CREATE_BIND_GROUP_TAG = 0x06;
const CREATE_PIPELINE_LAYOUT_TAG = 0x08;
const CREATE_GRAPHICS_PIPELINE_TAG = 0x0a;
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
  // Every `ImageType` and every `ImageViewType` appears below, because a row of
  // either code table in `gpu-stream.js` that the fixture never carries is a
  // row nothing checks. Within each command the extent's three components,
  // `mipLevels` and `samples` are all different numbers, so a field read in the
  // wrong order decodes to a different value rather than the same one.
  {
    name: 'CreateImage',
    image: handle(61, 62),
    label: 'gbuffer albedo',
    imageType: 'D2',
    extent: { width: 1280, height: 720, depthOrLayers: 3 },
    format: 'RGBA8_UNORM_SRGB',
    mipLevels: 11,
    samples: 1,
    usage: ['SAMPLED', 'COLOR_ATTACHMENT'],
  },
  // `depthOrLayers` is a depth here and an array-layer count above, decided by
  // nothing but the `imageType` byte.
  {
    name: 'CreateImage',
    image: handle(63, 64),
    label: null,
    imageType: 'D3',
    extent: { width: 160, height: 90, depthOrLayers: 64 },
    format: 'R16_FLOAT',
    mipLevels: 7,
    samples: 4,
    usage: ['TRANSFER_SRC', 'STORAGE'],
  },
  // Zero `mipLevels` and zero `samples`: no device accepts either, and both
  // cross verbatim. The encoding refuses malformed *streams*, not descriptors a
  // replayer will reject through `take_error`. `ImageUsage::all()` is what pins
  // the claimed-bit mask `readFlags` derives from the table's length — a table
  // shorter than the HAL's refuses this very command.
  {
    name: 'CreateImage',
    image: handle(65, 66),
    label: '',
    imageType: 'D1',
    extent: { width: 256, height: 1, depthOrLayers: 1 },
    format: 'R8_UNORM',
    mipLevels: 0,
    samples: 0,
    usage: [
      'TRANSFER_SRC',
      'TRANSFER_DST',
      'SAMPLED',
      'STORAGE',
      'COLOR_ATTACHMENT',
      'DEPTH_STENCIL_ATTACHMENT',
      'PRESENT',
    ],
  },
  // Two handles per view, distinct in both halves: the id being filled in
  // cannot be confused with the id being read.
  {
    name: 'CreateImageView',
    view: handle(67, 68),
    label: 'cascade 2',
    image: handle(61, 62),
    viewType: 'D2Array',
    format: 'D32_FLOAT_S8_UINT',
    range: {
      aspect: ['DEPTH', 'STENCIL'],
      baseMip: 1,
      mipCount: 2,
      baseLayer: 3,
      layerCount: 4,
    },
  },
  // `ImageSubresourceRange::ALL` is `0xFFFFFFFF` and crosses as itself. One of
  // the two counts is the sentinel and the other is not, so the pair cannot be
  // swapped unnoticed.
  {
    name: 'CreateImageView',
    view: handle(69, 70),
    label: null,
    image: handle(63, 64),
    viewType: 'D3',
    format: 'R16_FLOAT',
    range: {
      aspect: ['COLOR'],
      baseMip: 5,
      mipCount: 4294967295,
      baseLayer: 6,
      layerCount: 7,
    },
  },
  {
    name: 'CreateImageView',
    view: handle(71, 72),
    label: '',
    image: handle(65, 66),
    viewType: 'D1',
    format: 'R8_UNORM',
    range: {
      aspect: ['COLOR'],
      baseMip: 9,
      mipCount: 10,
      baseLayer: 11,
      layerCount: 12,
    },
  },
  {
    name: 'CreateImageView',
    view: handle(73, 74),
    label: 'sky cube',
    image: handle(61, 62),
    viewType: 'Cube',
    format: 'RGBA8_UNORM',
    range: {
      aspect: ['COLOR'],
      baseMip: 13,
      mipCount: 14,
      baseLayer: 15,
      layerCount: 16,
    },
  },
  // The other half of each adjacent pair of view types, so a table that folded
  // `Cube` into `CubeArray` cannot stay green.
  {
    name: 'CreateImageView',
    view: handle(75, 76),
    label: null,
    image: handle(63, 64),
    viewType: 'CubeArray',
    format: 'BGRA8_UNORM_SRGB',
    range: {
      aspect: ['COLOR'],
      baseMip: 17,
      mipCount: 8,
      baseLayer: 18,
      layerCount: 4294967295,
    },
  },
  // The stencil-only view: with `COLOR` and `DEPTH | STENCIL` above, all three
  // aspect bits are exercised and the claimed-bit mask is held to three.
  {
    name: 'CreateImageView',
    view: handle(77, 78),
    label: 'stencil',
    image: handle(65, 66),
    viewType: 'D2',
    format: 'D24_UNORM_S8_UINT',
    range: {
      aspect: ['STENCIL'],
      baseMip: 19,
      mipCount: 20,
      baseLayer: 21,
      layerCount: 22,
    },
  },
  // Four samplers, because two of the descriptor's field groups are three
  // identically typed values in a row. `magFilter`/`minFilter`/`mipFilter` has
  // only two variants to draw on, so no single command can make the trio
  // distinct — the first three below each put the single `Linear` in a different
  // slot instead, and every pairwise transposition changes at least one of them.
  // `addressMode` spells three *different* modes in each command, in a different
  // rotation each time, and all four `SamplerAddressMode` rows appear across the
  // set — a row the fixture never carries is a row nothing checks.
  {
    name: 'CreateSampler',
    sampler: handle(83, 84),
    label: 'shadow pcf',
    magFilter: 'Linear',
    minFilter: 'Nearest',
    mipFilter: 'Nearest',
    addressMode: ['Repeat', 'MirrorRepeat', 'ClampToEdge'],
    lodMin: 0.5,
    // `f32::MAX` — `SamplerDesc::default`'s "no limit" sentinel, which crosses
    // the wire verbatim. Only the replayer resolves it, and `gpu-replay.mjs` is
    // where what it resolves to is asserted.
    lodMax: 3.4028234663852886e38,
    anisotropy: 1,
    // The reversed-Z shadow test. Its opposite is two commands down, so a table
    // that folded the pair cannot stay green.
    compare: 'Greater',
  },
  {
    name: 'CreateSampler',
    sampler: handle(85, 86),
    label: null,
    magFilter: 'Nearest',
    minFilter: 'Linear',
    mipFilter: 'Nearest',
    addressMode: ['ClampToBorder', 'Repeat', 'MirrorRepeat'],
    // `0.1` is not representable in binary: the nearest `f32` is
    // `0.100000001490116119384765625`, which is this number read back as a
    // `f64`. An encoding that went through a decimal string, or that widened to
    // `f64` and narrowed back through a different rounding, lands elsewhere —
    // and a mip clamp a half-ulp out is a sampler nobody can tell is wrong.
    lodMin: 0.10000000149011612,
    lodMax: 12.25,
    anisotropy: 1,
    // Absent, against the three present ones around it. `Never` is code 0, so a
    // decoder that read the presence byte as the code would turn this into a
    // comparison that always fails rather than into no comparison at all.
    compare: null,
  },
  {
    name: 'CreateSampler',
    sampler: handle(87, 88),
    label: '',
    magFilter: 'Nearest',
    minFilter: 'Nearest',
    mipFilter: 'Linear',
    addressMode: ['ClampToEdge', 'ClampToBorder', 'Repeat'],
    lodMin: 2,
    lodMax: 3,
    // Past 1 while the filters are not all linear, which WebGPU forbids: the
    // wire carries it and the replayer is what refuses it.
    anisotropy: 16,
    compare: 'Less',
  },
  {
    name: 'CreateSampler',
    sampler: handle(89, 90),
    label: 'aniso',
    magFilter: 'Linear',
    minFilter: 'Linear',
    mipFilter: 'Linear',
    addressMode: ['MirrorRepeat', 'ClampToEdge', 'Repeat'],
    lodMin: 1,
    lodMax: 8,
    // Fractional, which WebGPU's `GPUSize32` cannot carry: verbatim here, and
    // narrowed by the replayer.
    anisotropy: 4.5,
    compare: 'Always',
  },
  // **Six layouts, because this is the first counted list of structs.** An entry
  // is five fields deep and carries an enum whose variants have different-length
  // payloads, so a stride out by a byte does not truncate — it decodes the next
  // entry out of the middle of this one and answers something well-formed.
  //
  // The first is the long one: every `BindingKind` WebGPU can express, each with
  // both values of every `bool` it carries.
  {
    name: 'CreateBindGroupLayout',
    layout: handle(93, 94),
    label: 'frame',
    entries: [
      {
        binding: 0,
        visibility: ['VERTEX'],
        kind: { name: 'StorageBuffer', readOnly: true, dynamic: false },
        count: 1,
        flags: [],
      },
      // The same kind with both bools the other way round, so a decoder that
      // read one of them twice cannot stay green.
      {
        binding: 1,
        visibility: ['COMPUTE'],
        kind: { name: 'StorageBuffer', readOnly: false, dynamic: true },
        count: 1,
        flags: [],
      },
      {
        binding: 2,
        visibility: ['VERTEX', 'FRAGMENT'],
        kind: { name: 'UniformBuffer', dynamic: true },
        count: 1,
        flags: [],
      },
      {
        binding: 3,
        visibility: ['FRAGMENT'],
        kind: { name: 'UniformBuffer', dynamic: false },
        count: 1,
        flags: [],
      },
      // A `Depth` slot beside a comparison sampler, which is the pair WebGPU
      // checks against each other.
      {
        binding: 4,
        visibility: ['FRAGMENT'],
        kind: {
          name: 'SampledImage',
          viewType: 'D2Array',
          sampleType: 'Depth',
        },
        count: 1,
        flags: [],
      },
      {
        binding: 5,
        visibility: ['FRAGMENT'],
        kind: { name: 'Sampler', comparison: true },
        count: 1,
        flags: [],
      },
      {
        binding: 6,
        visibility: ['FRAGMENT'],
        kind: { name: 'SampledImage', viewType: 'Cube', sampleType: 'Float' },
        count: 1,
        flags: [],
      },
      {
        binding: 7,
        visibility: ['COMPUTE'],
        kind: { name: 'Sampler', comparison: false },
        count: 1,
        flags: [],
      },
    ],
  },
  // The portable bindless declaration, and the unlabelled twin: `u32::MAX` means
  // "as many as this device can" and crosses verbatim, beside all three
  // `BindingFlags`, on the entry that is both last and highest-numbered.
  {
    name: 'CreateBindGroupLayout',
    layout: handle(95, 96),
    label: null,
    entries: [
      {
        binding: 0,
        visibility: ['FRAGMENT'],
        kind: { name: 'StorageBuffer', readOnly: true, dynamic: false },
        count: 1,
        flags: [],
      },
      {
        binding: 1,
        visibility: ['FRAGMENT'],
        kind: { name: 'SampledImage', viewType: 'D2', sampleType: 'Float' },
        count: 0xffffffff,
        // In ascending bit order, which is the order `readFlags` answers in.
        flags: ['PARTIALLY_BOUND', 'UPDATE_AFTER_BIND', 'VARIABLE_COUNT'],
      },
    ],
  },
  // Present-and-empty label, and an empty entry list: the counted list at zero,
  // which is the length a reader most easily treats as "read until something
  // stops you". The command after it is what would be eaten if it did.
  {
    name: 'CreateBindGroupLayout',
    layout: handle(97, 98),
    label: '',
    entries: [],
  },
  // A fixed-size array — neither 1 nor the sentinel, which is the case a
  // replayer treating "not the sentinel" as "one descriptor" would silently
  // shrink.
  {
    name: 'CreateBindGroupLayout',
    layout: handle(99, 100),
    label: 'texture page',
    entries: [
      {
        binding: 8,
        visibility: ['FRAGMENT'],
        kind: {
          name: 'SampledImage',
          viewType: 'D2Array',
          sampleType: 'Float',
        },
        count: 64,
        flags: [],
      },
    ],
  },
  // The two stages WebGPU has no `GPUShaderStage` bit for. `TASK` is bit 4, so
  // this command is what holds `SHADER_STAGES` at five rows rather than four: a
  // claimed-bit mask that stopped at `MESH` refuses it outright.
  {
    name: 'CreateBindGroupLayout',
    layout: handle(101, 102),
    label: null,
    entries: [
      {
        binding: 9,
        visibility: ['MESH'],
        kind: {
          name: 'SampledImage',
          viewType: 'D2Array',
          sampleType: 'Float',
        },
        count: 1,
        flags: [],
      },
      {
        binding: 10,
        visibility: ['TASK'],
        kind: { name: 'StorageBuffer', readOnly: true, dynamic: false },
        count: 1,
        flags: [],
      },
    ],
  },
  // **Two entries differing in exactly one field, and sharing a binding
  // number.** The one differing field is `readOnly`, so a decoder that read the
  // kind's payload once and copied it fails here and nowhere else; the shared
  // binding number is what says the list is kept in slice order rather than
  // rebuilt from binding numbers, because a decoder that keyed on them would
  // collapse these two into one. `check_entries` in `crcbl-hal` rejects a
  // duplicate binding — this encoding refuses a malformed stream and nothing
  // else, which is the division of labour.
  //
  // `StorageImage` is here for a second reason of the same kind: it is the one
  // `BindingKind` WebGPU cannot express, and a replayer can only refuse what it
  // was told.
  {
    name: 'CreateBindGroupLayout',
    layout: handle(103, 104),
    label: 'gbuffer store',
    entries: [
      {
        binding: 11,
        visibility: ['COMPUTE'],
        kind: { name: 'StorageImage', readOnly: false },
        count: 1,
        flags: [],
      },
      {
        binding: 11,
        visibility: ['COMPUTE'],
        kind: { name: 'StorageImage', readOnly: true },
        count: 1,
        flags: [],
      },
    ],
  },
  // **The first command whose entries carry handles into three different
  // resource tables.** One of each `BindingResource` shape — a `Buffer` with a
  // numbered range, an `ImageView`, a `Sampler`, and a `Buffer` whose `size` is
  // the `WHOLE_BUFFER` sentinel — and the discriminant is the only thing that
  // says which table each id indexes. `offset` and `size` are `BigInt`: a
  // `WHOLE_BUFFER` read as a `Number` would round to 18446744073709552000.
  {
    name: 'CreateBindGroup',
    group: handle(107, 108),
    label: 'material',
    layout: handle(93, 94),
    entries: [
      {
        binding: 0,
        arrayIndex: 0,
        resource: {
          name: 'Buffer',
          buffer: handle(11, 12),
          offset: 256n,
          size: 1024n,
        },
      },
      {
        binding: 1,
        arrayIndex: 0,
        resource: { name: 'ImageView', view: handle(67, 68) },
      },
      {
        binding: 2,
        arrayIndex: 0,
        resource: { name: 'Sampler', sampler: handle(83, 84) },
      },
      // `WHOLE_BUFFER` — `u64::MAX` — which crosses verbatim and which only the
      // replayer resolves, to an absent `GPUBufferBinding.size`.
      {
        binding: 3,
        arrayIndex: 0,
        resource: {
          name: 'Buffer',
          buffer: handle(13, 14),
          offset: 0n,
          size: 18446744073709551615n,
        },
      },
    ],
    // `None`, against the `Some(2)` on the twin below.
    variableCount: null,
  },
  // `variableCount: 2`, and two entries sharing binding 0 that differ only in
  // `arrayIndex` — the bindless write path, and the pair a decoder that keyed on
  // binding would collapse.
  {
    name: 'CreateBindGroup',
    group: handle(109, 110),
    label: null,
    layout: handle(95, 96),
    entries: [
      {
        binding: 0,
        arrayIndex: 0,
        resource: { name: 'ImageView', view: handle(69, 70) },
      },
      {
        binding: 0,
        arrayIndex: 1,
        resource: { name: 'ImageView', view: handle(71, 72) },
      },
    ],
    variableCount: 2,
  },
  // **The heaviest descriptor on the seam, all four artifacts non-trivial.** A
  // decoder that stopped traversing one field lands the cursor wrong for the
  // rest; a decoder that read a later field before skipping `spirv` decodes the
  // wrong bytes for it. `spirv` is a word array a browser never uses but must
  // step over; the two `dxil` pairs differ in both their string and container
  // lengths, so a wrong length for either leaf is caught.
  {
    name: 'CreateShaderModule',
    module: handle(113, 114),
    label: 'mesh.slang',
    spirv: [0x07230203, 0x00010600, 0x0000002a, 0x00000007, 0x00000000],
    wgsl: '@vertex fn vs() -> @builtin(position) vec4f { return vec4f(0.0); }',
    msl: '#include <metal_stdlib>\nvertex float4 vs() { return 0; }',
    dxil: [
      {
        entryPoint: 'vsMain',
        container: new Uint8Array([0xde, 0xad, 0xbe, 0xef]),
      },
      { entryPoint: 'fragment', container: new Uint8Array([0x01, 0x02]) },
    ],
  },
  // The first absence trap: `spirv` empty (absent), `wgsl` `''` (present and
  // empty — a valid module with no entry points), `msl` `null` (absent), `dxil`
  // the empty list (absent). A decoder that read `Some("")` as `null` fails on
  // the WGSL here.
  {
    name: 'CreateShaderModule',
    module: handle(115, 116),
    label: 'empty.wgsl',
    spirv: [],
    wgsl: '',
    msl: null,
    dxil: [],
  },
  // The mirror trap: `wgsl` `null` (absent, where the module above was present
  // and empty), `msl` `''` (present and empty, where the module above was
  // absent), and `dxil` a single pair whose container is empty — a present,
  // truncated artifact rather than the absent empty list above.
  {
    name: 'CreateShaderModule',
    module: handle(117, 118),
    label: null,
    spirv: [],
    wgsl: null,
    msl: '',
    dxil: [{ entryPoint: 'truncated', container: new Uint8Array([]) }],
  },
  // The last thing a pipeline is built from, and a counted list of bare handles
  // rather than of structs. `bindGroupLayouts` is in *set order* — what a
  // shader's `@group(n)` indexes — so a decoder that reversed it answers a
  // different layout, and two distinct handles are what make that visible.
  // `pushConstants` is `null`, the ordinary value.
  {
    name: 'CreatePipelineLayout',
    layout: handle(121, 122),
    label: 'gbuffer',
    bindGroupLayouts: [handle(93, 94), handle(95, 96)],
    pushConstants: null,
  },
  // `pushConstants` present, which WebGPU has no way to express at all: it
  // crosses whole so `gpu-replay.js` can refuse it by name. `stages` names two
  // bits, and `offset` differs from `size` so the pair cannot be swapped
  // unnoticed. The single bind-group layout keeps this list's length distinct
  // from the two-entry one above.
  {
    name: 'CreatePipelineLayout',
    layout: handle(123, 124),
    label: null,
    bindGroupLayouts: [handle(97, 98)],
    pushConstants: { stages: ['VERTEX', 'FRAGMENT'], offset: 16, size: 128 },
  },
  // The first command resolving handles into two *different* non-buffer tables:
  // `layout` names a pipeline layout and `module` a shader module, and which
  // table each indexes is the wire position and nothing else. `workgroupSize` is
  // `[8, 4, 2]`, non-uniform on purpose — three distinct numbers, so a
  // transposition of the components changes the decode rather than reproducing
  // it, and `gpu-replay.js` drops the field because WebGPU reads the real value
  // from the module's `@workgroup_size`. `entryPoint` is a real string.
  {
    name: 'CreateComputePipeline',
    pipeline: handle(127, 128),
    label: 'cull',
    layout: handle(121, 122),
    module: handle(113, 114),
    entryPoint: 'computeMain',
    workgroupSize: [8, 4, 2],
  },
  // The largest descriptor on the seam, and the deepest. `layout`, `vertexModule`
  // and the fragment module resolve into three different tables by wire position;
  // the fragment stage is present (a colour pass, not depth-only). Every enum
  // differs from the ones beside it, so a transposition anywhere goes red. The
  // depth-stencil chain is `Some(Some(..))` with `front` and `back` distinct in
  // every field, so a front/back swap is caught; the bias `slopeScale` is the
  // nearest f32 to 0.1, so an encoding that went through a decimal string would
  // land on a different number. Two colour targets with distinct formats, one
  // blended and one not.
  {
    name: 'CreateGraphicsPipeline',
    pipeline: handle(131, 132),
    label: 'gbuffer',
    layout: handle(121, 122),
    vertexModule: handle(113, 114),
    vertexEntryPoint: 'vertexMain',
    fragment: { module: handle(115, 116), entryPoint: 'fragmentMain' },
    primitive: {
      topology: 'TriangleStrip',
      frontFace: 'Cw',
      cullMode: 'Back',
      polygonMode: 'Fill',
      depthClamp: false,
    },
    depthStencil: {
      format: 'D32_FLOAT_S8_UINT',
      depthWrite: false,
      depthCompare: 'GreaterOrEqual',
      stencil: {
        front: {
          compare: 'Less',
          failOp: 'Keep',
          depthFailOp: 'IncrementWrap',
          passOp: 'Replace',
        },
        back: {
          compare: 'Greater',
          failOp: 'Zero',
          depthFailOp: 'DecrementClamp',
          passOp: 'Invert',
        },
        readMask: 0x0f,
        writeMask: 0xf0,
        reference: 0x2a,
      },
      // `slopeScale` is the nearest f32 to 0.1 — the bit pattern the wire carries
      // — computed with `Math.fround` rather than written as `0.1`, which is a
      // different double. `constant` and `clamp` are exact.
      bias: { constant: -2, slopeScale: Math.fround(0.1), clamp: 0.25 },
    },
    multisample: { samples: 4, mask: 0xff, alphaToCoverage: true },
    colorTargets: [
      {
        format: 'RGBA16_FLOAT',
        blend: {
          colorSrc: 'SrcAlpha',
          colorDst: 'OneMinusSrcAlpha',
          colorOp: 'Add',
          alphaSrc: 'One',
          alphaDst: 'OneMinusSrcAlpha',
          alphaOp: 'Add',
        },
        writeMask: ['R', 'G', 'B', 'A'],
      },
      { format: 'RG16_FLOAT', blend: null, writeMask: ['R', 'G'] },
    ],
  },
  { name: 'DestroyBuffer', buffer: handle(17, 18) },
  { name: 'DestroySurface', surface: handle(47, 48) },
  // A view and the image it views are separate objects in separate tables, so
  // these are two commands rather than one standing for both.
  { name: 'DestroyImage', image: handle(79, 80) },
  { name: 'DestroyImageView', view: handle(81, 82) },
  // Its own command and its own table again: a sampler's id and an image's are
  // allowed to be the same eight bytes.
  { name: 'DestroySampler', sampler: handle(91, 92) },
  // Its own command and its own table again, and the destroy whose empty slot is
  // the ordinary case: a layout the replayer refused still has its pre-allocated
  // handle destroyed.
  { name: 'DestroyBindGroupLayout', layout: handle(105, 106) },
  // Its own command and its own table again: a bind group's id is allowed to be
  // the same eight bytes as anything else's.
  { name: 'DestroyBindGroup', group: handle(111, 112) },
  // Its own command and its own table again: a shader module's id is allowed to
  // be the same eight bytes as anything else's, and this is the destroy
  // `crcbl-render` leans on hardest.
  { name: 'DestroyShaderModule', module: handle(119, 120) },
  // Its own command and its own table again, and — like the bind-group layout's
  // destroy — the one whose empty slot is the ordinary case: a pipeline layout
  // the replayer refused (a present push-constant range, an unresolvable
  // bind-group layout) still has its pre-allocated handle destroyed.
  { name: 'DestroyPipelineLayout', layout: handle(125, 126) },
  // Its own command and its own table again: a compute pipeline's id is allowed
  // to be the same eight bytes as anything else's, and — like the pipeline-layout
  // destroy — the one whose empty slot is the ordinary case, since the replayer
  // refuses a pipeline it cannot build and the caller destroys the handle anyway.
  { name: 'DestroyComputePipeline', pipeline: handle(129, 130) },
  // Its own command and its own table again, and — like the compute-pipeline
  // destroy — the one whose empty slot is the ordinary case: the replayer refuses
  // a pipeline it cannot build and the caller destroys the handle anyway.
  { name: 'DestroyGraphicsPipeline', pipeline: handle(133, 134) },
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
  // The dynamic viewport and scissor the render graph sets on every pass. All
  // six viewport floats are distinct and the depth range is non-default, so a
  // transposition among them shows; the scissor carries a negative origin the
  // signed wire preserves.
  {
    name: 'SetViewport',
    viewport: {
      x: 1,
      y: 2,
      width: 640,
      height: 480,
      depthMin: 0.25,
      depthMax: 0.75,
    },
  },
  {
    name: 'SetScissor',
    rect: { x: -3, y: 4, width: 320, height: 200 },
  },
  // The index buffer for the UI pass's indexed draw. Distinct handle and a
  // non-zero `BigInt` offset, `Uint16` so the format pairs with a `Uint32`
  // nowhere else.
  {
    name: 'BindIndexBuffer',
    buffer: handle(202, 203),
    offset: 48n,
    format: 'Uint16',
  },
  {
    name: 'Draw',
    vertices: { start: 6, end: 9 },
    instances: { start: 1, end: 5 },
  },
  // The indexed draw — its two ranges and the negative `baseVertex` between them
  // are five distinct values so a transposition among them shows.
  {
    name: 'DrawIndexed',
    indices: { start: 12, end: 30 },
    baseVertex: -7,
    instances: { start: 2, end: 6 },
  },
  // The compute-pass commands. `BeginComputePass` is a label only — compute has
  // no attachments — with its `None` twin below; the dispatch's three counts are
  // distinct so a transposition among x/y/z is visible.
  { name: 'BeginComputePass', label: 'cull' },
  { name: 'BindComputePipeline', pipeline: handle(174, 175) },
  { name: 'Dispatch', x: 1000, y: 2000, z: 3000 },
  { name: 'EndComputePass' },
  { name: 'BeginComputePass', label: null },
  // The copy that carries a dispatch's storage-buffer output to a host buffer.
  // Distinct source and destination, and its two offsets and the size are three
  // different `BigInt`s so a transposition among the `u64` fields is visible.
  {
    name: 'CopyBufferToBuffer',
    copy: {
      src: handle(176, 177),
      srcOffset: 1111n,
      dst: handle(178, 179),
      dstOffset: 2222n,
      size: 3333n,
    },
  },
  // The buffer→image upload copy — the same `BufferImageCopy` shape as
  // `CopyImageToBuffer` but the opposite direction, with its own distinct handles
  // and a non-zero texel pitch and image height. `bufferOffset` is `BigInt`.
  {
    name: 'CopyBufferToImage',
    buffer: handle(180, 181),
    bufferOffset: 512n,
    bufferRowLength: 120,
    bufferImageHeight: 240,
    image: handle(182, 183),
    imageSubresource: {
      aspect: ['COLOR'],
      mip: 1,
      baseLayer: 2,
      layerCount: 3,
    },
    imageOffset: { x: 5, y: -6, z: 7 },
    imageExtent: { width: 32, height: 24, depthOrLayers: 1 },
  },
  // The image→image copy, across different mip levels and offsets on the two
  // sides so a source/destination transposition cannot round-trip.
  {
    name: 'CopyImageToImage',
    copy: {
      src: handle(184, 185),
      srcSubresource: {
        aspect: ['COLOR'],
        mip: 1,
        baseLayer: 0,
        layerCount: 1,
      },
      srcOffset: { x: 8, y: 9, z: 0 },
      dst: handle(186, 187),
      dstSubresource: {
        aspect: ['COLOR'],
        mip: 3,
        baseLayer: 4,
        layerCount: 1,
      },
      dstOffset: { x: -1, y: 2, z: 3 },
      extent: { width: 16, height: 12, depthOrLayers: 1 },
    },
  },
  // The buffer fill, with a NON-ZERO value so the fixture carries the wire the
  // replayer refuses (WebGPU's `clearBuffer` is zero-only). `offset` and `size`
  // are `BigInt`; the value is a `u32`.
  {
    name: 'FillBuffer',
    buffer: handle(188, 189),
    offset: 64n,
    size: 256n,
    value: 0xdeadbeef,
  },
  // The host→buffer upload — `queue.writeBuffer` on the replayer, not an encoder
  // op. Buffer, offset and payload are all distinct so a transposition shows;
  // `offset` is `BigInt` and `data` is its own `Uint8Array`.
  {
    name: 'WriteBuffer',
    buffer: handle(200, 201),
    offset: 96n,
    data: new Uint8Array([0x12, 0x34, 0x56, 0x78, 0x9a]),
  },
  // The pipeline barrier, the documented no-op — carried whole for wire fidelity
  // though the replayer records nothing. The empty case first: a global-only
  // barrier with no transitions, pinning the two zero counts and the flag.
  {
    name: 'PipelineBarrier',
    buffers: [],
    images: [],
    global: true,
  },
  // And the populated case: one buffer barrier with a `Some` queue transfer, one
  // image barrier over a non-default subresource range with distinct from/to
  // states. `global` is `false` so both flag values appear.
  {
    name: 'PipelineBarrier',
    buffers: [
      {
        buffer: handle(190, 191),
        from: 'ShaderWrite',
        to: 'TransferSrc',
        queueTransfer: { from: handle(192, 193), to: handle(194, 195) },
      },
    ],
    images: [
      {
        image: handle(196, 197),
        range: {
          aspect: ['COLOR'],
          baseMip: 1,
          mipCount: 2,
          baseLayer: 3,
          layerCount: 4,
        },
        from: 'Undefined',
        to: 'ColorAttachment',
        queueTransfer: null,
      },
    ],
    global: false,
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
  // The readback path, in the order a frame records it — every number distinct
  // so a transposition among the copy's many fields is visible, and every
  // optional field both ways. `bufferOffset`, the readback offsets/sizes and the
  // semaphore values are `BigInt`, a buffer's own `u64`s.
  {
    name: 'CreateCommandEncoder',
    label: 'readback encoder',
    queue: handle(140, 141),
  },
  {
    name: 'CopyImageToBuffer',
    buffer: handle(142, 143),
    bufferOffset: 256n,
    bufferRowLength: 100,
    bufferImageHeight: 200,
    image: handle(144, 145),
    imageSubresource: {
      aspect: ['COLOR'],
      mip: 2,
      baseLayer: 3,
      layerCount: 5,
    },
    imageOffset: { x: -7, y: 9, z: 11 },
    imageExtent: { width: 64, height: 48, depthOrLayers: 1 },
  },
  { name: 'EndRenderPass' },
  { name: 'Finish', commandBuffer: handle(146, 147) },
  // Waits and signals non-empty, which no browser honours but the encoding
  // carries field for field — the replayer is where they are refused.
  {
    name: 'Submit',
    commandBuffers: [handle(148, 149), handle(150, 151)],
    waits: [{ semaphore: handle(152, 153), value: 0x0102030405060708n }],
    signals: [{ semaphore: handle(154, 155), value: 9n }],
  },
  // The empty-list twin — the only case WebGPU maps — and one command buffer.
  {
    name: 'Submit',
    commandBuffers: [handle(156, 157)],
    waits: [],
    signals: [],
  },
  // `after` present: a semaphore wait the replayer refuses, with a full `u64`.
  {
    name: 'RequestReadback',
    readback: handle(158, 159),
    label: 'stats readback',
    buffer: handle(160, 161),
    offset: 32n,
    size: 64n,
    after: { semaphore: handle(162, 163), value: 0x1122334455667788n },
  },
  // `after` absent — `mapAsync` — no label, and a `size` past a `u32`.
  {
    name: 'RequestReadback',
    readback: handle(164, 165),
    label: null,
    buffer: handle(166, 167),
    offset: 0n,
    size: 0x100000000n,
    after: null,
  },
  { name: 'PollReadback', readback: handle(168, 169) },
  { name: 'DestroyReadback', readback: handle(170, 171) },
  { name: 'DestroyCommandBuffer', commandBuffer: handle(172, 173) },
  // The presentation family. A non-default present mode and composite alpha and a
  // non-square extent, so a dropped field or a swapped enum byte decodes to a
  // different value. `presentMode` and `compositeAlpha` are the reply direction's
  // spelling, which is how `gpu-stream.js` inverts the tables to read them.
  {
    name: 'CreateSwapchain',
    swapchain: handle(174, 175),
    label: 'swapchain',
    surface: handle(176, 177),
    format: 'BGRA8_UNORM_SRGB',
    extent: { width: 800, height: 600 },
    imageCount: 3,
    presentMode: 'MAILBOX',
    compositeAlpha: 'PRE_MULTIPLIED',
  },
  {
    name: 'AcquireNextFrame',
    swapchain: handle(178, 179),
    image: handle(180, 181),
    view: handle(182, 183),
  },
  // A non-empty waits list and a `Some(presentId)`, so the refusal-carrying wire
  // is exercised; `presentId` is a `u64`, a `BigInt`.
  {
    name: 'Present',
    swapchain: handle(184, 185),
    waits: [handle(186, 187)],
    presentId: 0x0a0b0c0d0e0f1011n,
  },
  // The empty-list twin — the only case WebGPU maps — and `presentId` absent.
  {
    name: 'Present',
    swapchain: handle(188, 189),
    waits: [],
    presentId: null,
  },
  { name: 'DestroySwapchain', swapchain: handle(190, 191) },
  // Reconfigure in place — the same descriptor as `CreateSwapchain`, with a
  // DIFFERENT format (`RGBA8_UNORM`, not the create's `BGRA8_UNORM_SRGB`) and a
  // different extent, present mode and composite alpha, so an arm that decoded a
  // create's fields — or dropped or swapped one — decodes to a different value.
  {
    name: 'ReconfigureSwapchain',
    swapchain: handle(192, 193),
    label: 'reconfigured swapchain',
    surface: handle(194, 195),
    format: 'RGBA8_UNORM',
    extent: { width: 1024, height: 768 },
    imageCount: 2,
    presentMode: 'IMMEDIATE',
    compositeAlpha: 'OPAQUE',
  },
  // Body-less: the surface and the adapter the HAL call takes are validated
  // against an impl's own tables and never cross. A decoder that still read
  // them would consume the twelve bytes after this tag, which are the command
  // below and the end of the buffer — so the pair decodes one command short.
  { name: 'SurfaceCaps' },
  // Body-less too, and last in the corpus so that the byte offsets the checks
  // below count from the *first* command stay where they are. A decoder that
  // read one field too many here would run off the end of the buffer, which is
  // what the truncation sweep sees.
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
 * `value` as the four little-endian bytes the wire carries an `f32` as.
 *
 * Through a `DataView` rather than by hand, because what crosses is the **bit
 * pattern** of the nearest `f32` and nothing here should be re-deriving IEEE-754
 * rounding. The hand-built sampler bodies below only need the floats to be
 * well-formed; what pins their *values* is the fixture.
 *
 * @param {number} value
 * @returns {number[]}
 */
function f32le(value) {
  const bytes = new Uint8Array(4);
  new DataView(bytes.buffer).setFloat32(0, value, true);
  return [...bytes];
}

/**
 * A hand-built `create_sampler` body, with every byte after the label named.
 *
 * `mag`, `min`, `mip`, then U, V and W, then the three floats, then the
 * comparison's presence byte and — when it is present — its code. The checks
 * below vary one of those at a time, which is what makes each of them about one
 * table rather than about the command.
 *
 * @param {object} fields
 * @param {number[]} [fields.filters] The three `FilterMode` codes.
 * @param {number[]} [fields.address] The three `SamplerAddressMode` codes.
 * @param {number[]} [fields.compare] The presence byte, and the code if there is
 *   one.
 * @returns {number[]}
 */
function samplerBody({
  filters = [1, 1, 1],
  address = [0, 0, 0],
  compare = [0],
}) {
  return [
    CREATE_SAMPLER_TAG,
    ...u32le(1), // the handle's index
    ...u32le(1), // …and its generation
    0, // the label, absent
    ...filters,
    ...address,
    ...f32le(0), // lodMin
    ...f32le(1), // lodMax
    ...f32le(1), // anisotropy
    ...compare,
  ];
}

/**
 * A hand-built `create_bind_group_layout` body holding one entry.
 *
 * The handle, an absent label, an entry count of one, then the entry: `binding`,
 * `visibility`, the `BindingKind` code and its body, `count` and `flags`. The
 * checks below vary one of those at a time, which is what makes each of them
 * about one table rather than about the command.
 *
 * @param {object} fields
 * @param {number[]} [fields.visibility] The `ShaderStages` word.
 * @param {number[]} [fields.kind] The `BindingKind` code and its payload.
 * @param {number[]} [fields.count] The descriptor count.
 * @param {number[]} [fields.flags] The `BindingFlags` word.
 * @returns {number[]}
 */
function layoutBody({
  visibility = u32le(1), // ShaderStages::VERTEX
  kind = [1, 1, 0], // StorageBuffer { read_only: true, dynamic: false }
  count = u32le(1),
  flags = u32le(0),
} = {}) {
  return [
    CREATE_BIND_GROUP_LAYOUT_TAG,
    ...u32le(1), // the handle's index
    ...u32le(1), // …and its generation
    0, // the label, absent
    ...u32le(1), // one entry
    ...u32le(7), // binding
    ...visibility,
    ...kind,
    ...count,
    ...flags,
  ];
}

/**
 * A hand-built `create_bind_group` body holding one entry.
 *
 * The group handle, an absent label, the layout handle, an entry count of one,
 * then the entry — `binding`, `arrayIndex`, and the `BindingResource`
 * discriminant with its body — then an absent `variable_count`. The checks below
 * vary the discriminant, which is what makes them about that one table.
 *
 * @param {object} fields
 * @param {number[]} [fields.resource] The `BindingResource` code and its payload.
 * @returns {number[]}
 */
function bindGroupBody({ resource = [1, ...u32le(2), ...u32le(2)] } = {}) {
  // The default resource is an `ImageView` (code 1) naming handle (2, 2).
  return [
    CREATE_BIND_GROUP_TAG,
    ...u32le(1), // the group handle's index
    ...u32le(1), // …and its generation
    0, // the label, absent
    ...u32le(9), // the layout handle's index
    ...u32le(9), // …and its generation
    ...u32le(1), // one entry
    ...u32le(0), // binding
    ...u32le(0), // arrayIndex
    ...resource,
    0, // variable_count, absent
  ];
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

  // ---- a shader module's four absence conventions each survive -------------
  // The four artifacts do not share one, and the difference is load-bearing:
  // stated on its own so a decoder that collapsed `Some("")` into `null`, or an
  // empty `dxil` container into an absent artifact, fails with a message naming
  // which rule rather than only somewhere in a field-for-field diff.
  const shaders = decoded.filter(
    (command) => command.name === 'CreateShaderModule'
  );
  checkEqual(
    shaders.map((s) => [
      s.spirv.length === 0 ? 'absent' : `present(${s.spirv.length} words)`,
      s.wgsl === null ? 'None' : `Some(${JSON.stringify(s.wgsl)})`,
      s.msl === null ? 'None' : `Some(${JSON.stringify(s.msl)})`,
      s.dxil.length === 0
        ? 'absent'
        : `present(${s.dxil.map((p) => p.container.length).join(',')} bytes)`,
    ]),
    [
      [
        'present(5 words)',
        'Some("@vertex fn vs() -> @builtin(position) vec4f { return vec4f(0.0); }")',
        'Some("#include <metal_stdlib>\\nvertex float4 vs() { return 0; }")',
        'present(4,2 bytes)',
      ],
      ['absent', 'Some("")', 'None', 'absent'],
      ['absent', 'None', 'Some("")', 'present(0 bytes)'],
    ],
    'spirv empty≠present, wgsl/msl None≠Some("")≠Some("code"), and dxil empty-list≠a-pair-with-an-empty-container all decode distinctly'
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

  // ---- the body-less commands really have no body -------------------------
  // The fixture ends with `SurfaceCaps` then `EnumerateAdapters`, and each is
  // one tag byte and nothing else. So cutting one byte drops the last, cutting
  // two drops both, and only the third cut lands inside a body — the last field
  // of the `RequestDevice` before them — and is short.
  //
  // Byte for byte, which is what makes it a check on the *shape* rather than on
  // the count: a decoder that read any body at all for either empty command
  // fails the first two, and one that read a byte too few fails the third.
  for (const dropped of [1, 2]) {
    const short = fixture.subarray(0, fixture.length - dropped);
    checkEqual(
      failureOf(short) ?? decodeStream(short),
      EXPECTED.slice(0, -dropped),
      `cutting ${dropped} byte(s) drops ${dropped} body-less command(s) and decodes the rest`
    );
  }
  checkRefused(
    fixture.subarray(0, fixture.length - 3),
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

  // ---- the two dimensionality tables have no catch-all --------------------
  // Hand-built rather than reached for by offset into the fixture: these pin
  // the codes *one past* the last claimed one, which is where an off-by-one in
  // either table lands and where 0xFF never would. A table with a row too many
  // accepts this byte; a table with a row too few fails the fixture above.
  checkRefused(
    streamOf(header, [
      CREATE_IMAGE_TAG,
      ...someHandle,
      0, // the label, absent
      3, // one past ImageType::D3
    ]),
    { kind: 'InvalidEnum', field: 'ImageDesc::image_type', code: 3 },
    'an ImageType code no variant claims is refused rather than folded into a neighbour'
  );
  checkRefused(
    streamOf(header, [
      CREATE_IMAGE_VIEW_TAG,
      ...someHandle, // the view's own id
      0,
      ...someHandle, // the image it views
      6, // one past ImageViewType::D3
    ]),
    { kind: 'InvalidEnum', field: 'ImageViewDesc::view_type', code: 6 },
    'an ImageViewType code no variant claims is refused rather than folded into a neighbour'
  );
  // The format table is the one this slice reuses rather than writing a second
  // copy of, so it is checked through an image's own field: the byte after the
  // extent's twelve.
  checkRefused(
    streamOf(header, [
      CREATE_IMAGE_TAG,
      ...someHandle,
      0,
      1, // ImageType::D2
      ...u32le(8),
      ...u32le(8),
      ...u32le(1),
      0x1d, // one past Format::Bc7RgbaUnormSrgb
    ]),
    { kind: 'InvalidEnum', field: 'ImageDesc::format', code: 0x1d },
    'a Format code no variant claims is refused where an image names one'
  );

  // ---- the sampler's three code tables have no catch-all either ------------
  // Hand-built and one past the last claimed row of each, for the two
  // dimensionality tables' reason: that is where an off-by-one lands and where
  // 0xFF never would. Each of the three filter bytes is varied on its own, so a
  // decoder that read one of them three times names the wrong field here.
  for (const [at, field] of [
    [0, 'SamplerDesc::mag_filter'],
    [1, 'SamplerDesc::min_filter'],
    [2, 'SamplerDesc::mip_filter'],
  ]) {
    const filters = [1, 1, 1];
    filters[at] = 2; // one past FilterMode::Linear
    checkRefused(
      streamOf(header, samplerBody({ filters })),
      { kind: 'InvalidEnum', field, code: 2 },
      `a FilterMode code no variant claims is refused where ${field} names one`
    );
  }
  for (const at of [0, 1, 2]) {
    const address = [0, 0, 0];
    address[at] = 4; // one past SamplerAddressMode::ClampToBorder
    checkRefused(
      streamOf(header, samplerBody({ address })),
      { kind: 'InvalidEnum', field: 'SamplerDesc::address_mode', code: 4 },
      `a SamplerAddressMode code no variant claims is refused at U/V/W position ${at}`
    );
  }
  checkRefused(
    streamOf(header, samplerBody({ compare: [PRESENT, 8] })), // one past Always
    { kind: 'InvalidEnum', field: 'SamplerDesc::compare', code: 8 },
    'a CompareOp code no variant claims is refused rather than folded into a neighbour'
  );
  // The presence byte and the code behind it are separate refusals naming the
  // same field: a byte that is neither presence value is not a comparison this
  // build has never heard of.
  checkRefused(
    streamOf(header, samplerBody({ compare: [2, 0] })),
    { kind: 'InvalidEnum', field: 'SamplerDesc::compare', code: 2 },
    "a comparison's presence byte of 2 is refused rather than read as truthy"
  );

  // ---- every CompareOp row is exercised, not just the fixture's four -------
  // The fixture carries `Greater`, `Less`, `Always` and an absent one, which is
  // four of nine outcomes. The rest of the table is driven here, spelled out
  // rather than read off the decoder — a list taken from `gpu-stream.js` would
  // agree with `gpu-stream.js` whatever it said. `Greater` and `Less` are the
  // pair that matters: under reversed-Z one is the shadow test and the other is
  // its exact inverse, and nothing downstream can tell which a sampler got.
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
  const decodedCompares = COMPARE_OP.map((_, code) => {
    const [command] = decodeStream(
      streamOf(header, samplerBody({ compare: [PRESENT, code] }))
    );
    return command.compare;
  });
  checkEqual(
    decodedCompares,
    COMPARE_OP,
    'every CompareOp code decodes to the variant it names, Greater and Less included'
  );

  // ---- the two new bitflags words are strict too ---------------------------
  checkRefused(
    streamOf(header, [
      CREATE_IMAGE_TAG,
      ...someHandle,
      0,
      1, // ImageType::D2
      ...u32le(8),
      ...u32le(8),
      ...u32le(1),
      0x00, // Format::R8Unorm
      ...u32le(1), // mip levels
      ...u32le(1), // samples
      ...u32le(0xffffffff), // the usage word
    ]),
    { kind: 'InvalidEnum', field: 'ImageDesc::usage', code: 0xffffffff },
    'a bitflags bit no ImageUsage claims is refused rather than truncated away'
  );
  checkRefused(
    streamOf(header, [
      CREATE_IMAGE_VIEW_TAG,
      ...someHandle,
      0,
      ...someHandle,
      1, // ImageViewType::D2
      0x00, // Format::R8Unorm
      ...u32le(0xffffffff), // the aspect word, which opens the range
    ]),
    {
      kind: 'InvalidEnum',
      field: 'ImageSubresourceRange::aspect',
      code: 0xffffffff,
    },
    'a bitflags bit no ImageAspect claims is refused rather than truncated away'
  );

  // ---- a bind-group layout entry's own tables have no catch-all -----------
  // Hand-built and one past the last claimed row of each, for the two
  // dimensionality tables' reason. The `BindingKind` code is the one that costs
  // the most: its rows have different-length bodies, so a code read as its
  // neighbour consumes the wrong number of bytes and every field after it in the
  // entry decodes out of the wrong offsets.
  checkRefused(
    streamOf(header, layoutBody({ kind: [5, 0] })), // one past BindingKind::Sampler
    { kind: 'InvalidEnum', field: 'BindGroupLayoutEntry::kind', code: 5 },
    'a BindingKind code no variant claims is refused rather than folded into a neighbour'
  );
  // …and each payload behind a claimed code is its own field. A `bool` is a
  // presence byte, so a third value is an error rather than truth.
  checkRefused(
    streamOf(header, layoutBody({ kind: [1, 2, 0] })),
    { kind: 'InvalidEnum', field: 'BindingKind::read_only', code: 2 },
    "a StorageBuffer's read_only byte of 2 is refused rather than read as truthy"
  );
  checkRefused(
    streamOf(header, layoutBody({ kind: [1, 1, 2] })),
    { kind: 'InvalidEnum', field: 'BindingKind::dynamic', code: 2 },
    'and its dynamic byte is a separate field, named separately'
  );
  checkRefused(
    streamOf(header, layoutBody({ kind: [2, 6, 0] })), // one past ImageViewType::D3
    { kind: 'InvalidEnum', field: 'BindingKind::view_type', code: 6 },
    "a SampledImage's view_type is refused where no ImageViewType claims it"
  );
  checkRefused(
    streamOf(header, layoutBody({ kind: [2, 1, 2] })), // one past SampleType::Depth
    { kind: 'InvalidEnum', field: 'BindingKind::sample_type', code: 2 },
    'a SampleType code no variant claims is refused rather than folded into Float'
  );
  // The two bitflags words, which are the fields a `from_bits_truncate` would
  // quietly narrow: a visibility missing a stage is a binding the shader may not
  // read, and a dropped `BindingFlags` bit is the bindless downgrade
  // `crcbl_hal::BindingFlags` forbids by name.
  checkRefused(
    streamOf(header, layoutBody({ visibility: u32le(0xffffffff) })),
    {
      kind: 'InvalidEnum',
      field: 'BindGroupLayoutEntry::visibility',
      code: 0xffffffff,
    },
    'a ShaderStages bit no flag claims is refused rather than truncated away'
  );
  // One bit past `TASK`, which is where a table that stopped a stage short lands
  // and where 0xFFFFFFFF would not distinguish itself.
  checkRefused(
    streamOf(header, layoutBody({ visibility: u32le(1 << 5) })),
    {
      kind: 'InvalidEnum',
      field: 'BindGroupLayoutEntry::visibility',
      code: 1 << 5,
    },
    'the ShaderStages table is five rows exactly, not merely wide enough'
  );
  checkRefused(
    streamOf(header, layoutBody({ flags: u32le(1 << 3) })),
    {
      kind: 'InvalidEnum',
      field: 'BindGroupLayoutEntry::flags',
      code: 1 << 3,
    },
    'a BindingFlags bit no flag claims is refused rather than truncated away'
  );
  // An entry count past what the bytes can hold is refused like every other
  // counted list, and names this command's field rather than a neighbour's.
  checkRefused(
    streamOf(header, [
      CREATE_BIND_GROUP_LAYOUT_TAG,
      ...someHandle,
      0,
      ...u32le(0xffffffff),
    ]),
    {
      kind: 'InvalidLength',
      field: 'BindGroupLayoutDesc::entries',
      len: 0xffffffff,
    },
    'an entry count past MAX_ELEMENT_COUNT is refused'
  );

  // ---- a bind group's BindingResource table has no catch-all --------------
  // The most dangerous fold on the stream: a handle carries no kind, so this
  // discriminant is the only thing saying which of three resource tables an id
  // indexes, and the bodies are different lengths besides. One past the last
  // claimed code, which is where an off-by-one lands and where 0xFF never would.
  checkRefused(
    streamOf(
      header,
      bindGroupBody({ resource: [3, ...u32le(1), ...u32le(1)] })
    ),
    { kind: 'InvalidEnum', field: 'BindGroupEntry::resource', code: 3 },
    'a BindingResource code no variant claims is refused rather than folded into a neighbour'
  );
  // …and each shape's body is read at its own length: a `Buffer` (code 0) is a
  // handle and two `u64`s, so a stream carrying one and then a second command
  // decodes both, where an `ImageView`-length read of it would land inside the
  // next command. Driven through the whole decode rather than a hand-built body,
  // because what it pins is the *stride*.
  const twoGroups = decodeStream(
    streamOf(header, [
      ...bindGroupBody({
        resource: [
          0,
          ...u32le(11),
          ...u32le(12),
          ...u64le(256n),
          ...u64le(1024n),
        ],
      }),
      ...bindGroupBody(),
    ])
  );
  checkEqual(
    twoGroups.map((command) => command.entries[0].resource.name),
    ['Buffer', 'ImageView'],
    'a Buffer resource is read at its own length, leaving the next command intact'
  );

  // ---- every BindingKind row is exercised, not just the fixture's ---------
  // The fixture carries all five, but only in the combinations a real layout
  // would hold. This drives each code on its own with a body of the right
  // length, spelled out rather than read off the decoder — a list taken from
  // `gpu-stream.js` would agree with `gpu-stream.js` whatever it said.
  const decodedKinds = [
    [[0, 1], { name: 'UniformBuffer', dynamic: true }],
    [[1, 0, 1], { name: 'StorageBuffer', readOnly: false, dynamic: true }],
    [
      [2, 4, 1],
      { name: 'SampledImage', viewType: 'CubeArray', sampleType: 'Depth' },
    ],
    [[3, 1], { name: 'StorageImage', readOnly: true }],
    [[4, 1], { name: 'Sampler', comparison: true }],
  ].map(([kind, expected]) => {
    const [command] = decodeStream(streamOf(header, layoutBody({ kind })));
    return [command.entries[0].kind, expected];
  });
  checkEqual(
    decodedKinds.map(([actual]) => actual),
    decodedKinds.map(([, expected]) => expected),
    'every BindingKind code decodes to the variant it names, with its own body'
  );

  // ---- a pipeline layout's push-constant range is checked, both halves ----
  // The range crosses only so `gpu-replay.js` can refuse a present one by name,
  // and its `stages` still rides the strict bitflags rule: a bit no flag claims
  // is an error rather than a truncation, exactly as a layout entry's visibility
  // is. Hand-built with an empty set list, so the presence byte and the stages
  // word are the last few bytes.
  checkRefused(
    streamOf(header, [
      CREATE_PIPELINE_LAYOUT_TAG,
      ...someHandle, // the layout's own id
      0, // the label, absent
      ...u32le(0), // an empty bind-group-layout list
      PRESENT, // push_constants present
      ...u32le(0xffffffff), // stages — a bit no ShaderStages flag claims
      ...u32le(0), // offset
      ...u32le(4), // size
    ]),
    {
      kind: 'InvalidEnum',
      field: 'PushConstantRange::stages',
      code: 0xffffffff,
    },
    'a push-constant range stage bit no flag claims is refused rather than truncated'
  );
  // …and the presence byte itself is refused when it is neither canonical value,
  // naming the optional field rather than the range's stages.
  checkRefused(
    streamOf(header, [
      CREATE_PIPELINE_LAYOUT_TAG,
      ...someHandle,
      0,
      ...u32le(0),
      2, // neither ABSENT nor PRESENT
    ]),
    {
      kind: 'InvalidEnum',
      field: 'PipelineLayoutDesc::push_constants',
      code: 2,
    },
    "a push-constant range's presence byte of 2 is refused rather than read as truthy"
  );

  // ---- the graphics pipeline's nested tree is read in the right order -----
  // The deepest descriptor on the seam, hand-built with the fragment and
  // depth-stencil absent so the primitive block, the multisample block and one
  // blended colour target are the last bytes and each leaf sits at a named
  // offset. Corrupting one leaf at a time is what says the reader walks the tree
  // rather than landing a field one along — a decoder that read the blend's alpha
  // factors before its colour factors would name `color_src` where this expects
  // `alpha_src`.
  const graphicsPipelineBody = (mut = (b) => b) => {
    const body = [
      CREATE_GRAPHICS_PIPELINE_TAG,
      ...someHandle, // the pipeline's own id
      0, // label, absent
      ...someHandle, // layout
      ...someHandle, // vertex module
      ...u32le(2), // vertex entry point length
      0x76,
      0x73, // "vs"
      0, // fragment, absent
      // primitive: topology, front_face, cull_mode, polygon_mode, depth_clamp
      3, // TriangleList
      1, // Cw
      2, // Back
      0, // Fill
      0, // depth_clamp false
      0, // depth_stencil, absent
      // multisample: samples, mask, alpha_to_coverage
      ...u32le(4),
      ...u32le(0xff),
      0,
      ...u32le(1), // one colour target
      // target 0: format, blend present, blend body, write mask
      0x0a, // Format::Rgba16Float
      PRESENT, // blend present
      4, // color_src  SrcAlpha
      5, // color_dst  OneMinusSrcAlpha
      0, // color_op   Add
      1, // alpha_src  One
      5, // alpha_dst  OneMinusSrcAlpha
      0, // alpha_op   Add
      ...u32le(0x0f), // write mask R|G|B|A
    ];
    return mut(body.slice());
  };
  // The colour-target block starts after the primitive (5), the absent
  // depth-stencil (1) and the multisample block (9) and target count (4).
  const targetAt = 1 + 8 + 1 + 8 + 8 + (4 + 2) + 1 + 5 + 1 + 9 + 4; // from the tag byte
  const formatAt = HEADER_BYTES + targetAt;
  const blendColorSrcAt = formatAt + 2; // past format + blend presence
  const blendAlphaSrcAt = blendColorSrcAt + 3; // past color src/dst/op
  const topologyAt = HEADER_BYTES + 1 + 8 + 1 + 8 + 8 + (4 + 2) + 1;

  for (const [at, field, code] of [
    [topologyAt, 'PrimitiveState::topology', 0x7f],
    [formatAt, 'ColorTargetState::format', 0x7f],
    [blendColorSrcAt, 'BlendState::color_src', 0x7f],
    [blendAlphaSrcAt, 'BlendState::alpha_src', 0x7f],
  ]) {
    checkRefused(
      streamOf(
        header,
        graphicsPipelineBody((b) => ((b[at - HEADER_BYTES] = code), b))
      ),
      { kind: 'InvalidEnum', field, code },
      `a graphics pipeline's ${field} code no variant claims is refused, not folded`
    );
  }
  // The whole tree decodes when nothing is corrupted, so the sweep is refusing a
  // real command rather than a body that never decoded.
  {
    const [command] = decodeStream(streamOf(header, graphicsPipelineBody()));
    check(
      command.name === 'CreateGraphicsPipeline' &&
        command.fragment === null &&
        command.depthStencil === null &&
        command.colorTargets.length === 1 &&
        command.colorTargets[0].blend.alphaSrc === 'One',
      'the hand-built graphics pipeline decodes, so the sweep refuses a real one'
    );
  }

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
