#!/usr/bin/env node
// The other half of the reply stream's drift check.
//
// `web/engine/gpu-reply.js` encodes replies in JavaScript and that encoder is
// the production one — the Rust `ReplyWriter` exists to test the encoding and to
// produce the fixture, not to run in a browser. Two hand-written halves of one
// format will drift, and no compiler anywhere reads both.
//
// So the two are pinned to a third thing they both have to agree with:
// `crates/crcbl-webgpu/tests/fixture.rs` freezes the canonical replies into
// `tests/fixtures/canonical-replies.bin`, and this file encodes the same replies
// here and asserts the bytes are identical. Change a tag, a field order or a cap
// in Rust and the fixture test goes red; make the same change only in JavaScript
// and this one does.
//
// THE DIRECTION IS THE OPPOSITE OF `stream-decode.mjs`, and so is what each
// check protects. There, Rust encodes and JS decodes, so the node tool decodes
// the committed bytes and asserts every field. Here, JS encodes and Rust
// decodes, so the node tool *re-encodes* and asserts every byte — a decode would
// prove nothing about the writer, which is the half that ships.
//
// The replies below are therefore deliberately spelled out rather than read from
// anything. A list derived from the fixture would agree with the fixture by
// construction.
//
// ONE EXCEPTION, AND IT IS THE POINT OF IT. The canonical adapter's feature word
// is not written out here: it is produced by running `gpu-replay.js`'s real
// `halFeaturesFor` over a stub adapter holding every `GPUFeatureName` the
// mapping table claims, plus names it does not. That is not derived from the
// fixture — it is derived from the production mapping, and compared against a
// fixture whose bytes came from Rust naming `crcbl_hal::Features` flags. So the
// comparison below is a cross-language check on the WebGPU→HAL feature mapping
// itself, not only on the byte writer. Spelling the number out here would have
// checked nothing but this file's arithmetic.
//
// It also asserts what the writer must refuse: a payload past the field cap, an
// array past the element cap, a handle with no generation, and a sequence handed
// in as a number rather than a `BigInt`. Each has to throw a typed
// `ReplyEncodeError` where the mistake is, rather than producing a buffer wasm
// rejects a frame later.
//
// Usage:
//   node web/tools/reply-encode.mjs [path-to-fixture.bin]

import { readFile } from 'node:fs/promises';

import {
  COMPOSITE_ALPHA,
  DEVICE_TYPE,
  FORMAT,
  PRESENT_MODE,
  SURFACE_CAPS_FAILURE,
  ReplyEncodeError,
  ReplyWriter,
} from '../engine/gpu-reply.js';
import { halFeaturesFor } from '../engine/gpu-replay.js';

/** The fixture `crcbl-webgpu`'s `fixture.rs` writes. */
const FIXTURE = new URL(
  '../../crates/crcbl-webgpu/tests/fixtures/canonical-replies.bin',
  import.meta.url
);

/** `tag::REPLY_HEADER_BYTES`: the magic and the version word. No base sequence:
 * every reply carries the number it answers. */
const REPLY_HEADER_BYTES = 8 + 2;

/** `tag::MAX_FIELD_BYTES`. */
const MAX_FIELD_BYTES = 1 << 20;

/** `tag::MAX_ELEMENT_COUNT`. */
const MAX_ELEMENT_COUNT = 1 << 16;

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
 * @param {number} index
 * @param {number} generation
 */
function handle(index, generation) {
  return { index, generation };
}

/**
 * The payload `replies::growth_payload` generates: big enough to force this
 * writer past its initial buffer and double it, with a value per byte.
 *
 * The one part of either corpus that is computed rather than written out, and
 * the rule is duplicated here on purpose — a payload imported from the decoder
 * would agree with it by construction, and a growth that wrote through the view
 * it held *before* growing is precisely what this catches. It did catch one.
 */
function growthPayload() {
  const bytes = new Uint8Array(512);
  for (let i = 0; i < bytes.length; i += 1) bytes[i] = (i * 7) % 251;
  return bytes;
}

/**
 * An adapter with every `GPUFeatureName` the mapping table lists, and two it
 * does not.
 *
 * The unmapped pair is not decoration: the mapping iterates its own table, so a
 * name outside it must contribute nothing, and a rewrite that iterated
 * `adapter.features` instead would set no bit for them and still pass — unless
 * they are present and the expected bits are a fixed word from the other
 * language. They are.
 */
function everyMappedFeature() {
  return {
    features: new Set([
      'depth-clip-control',
      'texture-compression-bc',
      'timestamp-query',
      'indirect-first-instance',
      'shader-f16',
      'float32-filterable',
    ]),
  };
}

/**
 * `crcbl_hal::Limits` as `replies::every_limit` spells it, in the seam's names.
 *
 * Written out rather than mapped from a stub `GPUSupportedLimits`, because this
 * check is about the *bytes*: `gpu-replay.mjs` is where the WebGPU→seam limit
 * mapping is checked field by field, against a stub whose numbers are all
 * distinct.
 */
function everyLimit() {
  return {
    maxImage2d: 16384,
    maxImage3d: 2048,
    maxImageArrayLayers: 256,
    maxStorageBufferRange: 134_217_728n,
    maxUniformBufferRange: 65536n,
    maxBindGroups: 4,
    maxBindlessDescriptors: 0,
    maxPushConstantSize: 0,
    maxColorAttachments: 8,
    maxSampleCount: 4,
    maxDrawIndirectCount: 1,
    maxComputeWorkgroupSize: [256, 254, 64],
    maxComputeInvocationsPerWorkgroup: 256,
    maxComputeWorkgroupsPerDimension: 65535,
    minUniformBufferOffsetAlignment: 256n,
    minStorageBufferOffsetAlignment: 128n,
    optimalBufferCopyOffsetAlignment: 512n,
    maxSamplerAnisotropy: 1,
    timestampPeriodNs: 1,
  };
}

/**
 * A well-formed adapter record, for the checks that break exactly one thing
 * about it. Fresh each call, since several of them mutate it.
 */
function browserShapedAdapter() {
  return {
    id: 0,
    name: 'stub',
    vendorId: 0,
    deviceId: 0,
    deviceType: DEVICE_TYPE.OTHER,
    driver: '',
    features: 0n,
    limits: everyLimit(),
  };
}

/**
 * The canonical replies, in the order `replies::every_reply` holds them.
 *
 * Three things about this list are load-bearing beyond the field values:
 *
 *   * **The sequences are not in order and not contiguous.** A writer that
 *     numbered replies by position would produce a buffer that decodes and is
 *     wrong; these numbers are what make that a byte difference.
 *   * **One sequence and one value are past 2^32.** Both are `BigInt` here for
 *     that reason; as numbers they would round to something that still looks
 *     plausible.
 *   * **The first adapter's feature word comes from the production mapping**
 *     rather than from a literal — see the header — while the second adapter is
 *     deliberately *not* a browser's answer: it fills the four fields WebGPU can
 *     never fill. The wire has to carry them, because the far side decodes what
 *     it is sent rather than what a browser could have sent.
 *
 * @param {ReplyWriter} replies
 */
function encodeCanonicalReplies(replies) {
  replies.adapter(9n, {
    id: 3,
    name: 'Apple M2 — ✱',
    vendorId: 0,
    deviceId: 0,
    deviceType: DEVICE_TYPE.OTHER,
    driver: '',
    features: halFeaturesFor(everyMappedFeature()),
    limits: everyLimit(),
  });
  replies.adapter(2n, {
    id: 0,
    name: '',
    vendorId: 0x1002,
    deviceId: 0x744c,
    deviceType: DEVICE_TYPE.DISCRETE,
    driver: 'radv 25.1.4',
    features: 0n,
    // `Limits::minimum()` on the Rust side — the floor every backend clears.
    limits: {
      maxImage2d: 8192,
      maxImage3d: 2048,
      maxImageArrayLayers: 256,
      maxStorageBufferRange: 134_217_728n,
      maxUniformBufferRange: 65536n,
      maxBindGroups: 4,
      maxBindlessDescriptors: 0,
      maxPushConstantSize: 0,
      maxColorAttachments: 4,
      maxSampleCount: 4,
      maxDrawIndirectCount: 1,
      maxComputeWorkgroupSize: [256, 256, 64],
      maxComputeInvocationsPerWorkgroup: 256,
      maxComputeWorkgroupsPerDimension: 65535,
      minUniformBufferOffsetAlignment: 256n,
      minStorageBufferOffsetAlignment: 256n,
      optimalBufferCopyOffsetAlignment: 256n,
      maxSamplerAnisotropy: 1,
      timestampPeriodNs: 0,
    },
  });
  replies.readbackPending(17n, handle(51, 52));
  replies.readbackReady(
    5n,
    handle(53, 54),
    new Uint8Array([0x0b, 0xad, 0xf0, 0x0d])
  );
  replies.readbackReady(23n, handle(55, 56), new Uint8Array());
  replies.readbackReady(31n, handle(61, 62), growthPayload());
  replies.queryResults(0x0000_0001_0000_002an, handle(57, 58), 4, [
    0xffff_ffff_ffff_ffffn,
    0n,
    1_234_567_890_123n,
  ]);
  replies.queryResults(11n, handle(59, 60), 0, []);
  // Appended rather than filed beside the two adapters, where they read better:
  // `replies::every_reply` explains why, and the two lists have to be in the
  // same order for the same bytes to come out.
  replies.noAdapter(13n, 'requestAdapter() resolved null — ✱');
  replies.noAdapter(1n, '');
  // **The device's own capabilities, and not the adapter's.** The feature word
  // comes from the production mapping again — over a *device* stub with no
  // features, which is what `requestDevice()` returns when nothing optional was
  // asked for — so this number is the four core flags and nothing else, while
  // the first adapter above has those four plus four more. A replayer that
  // answered a device request by copying its adapter record would put a
  // different word here.
  replies.device(7n, {
    features: halFeaturesFor(defaultDevice()),
    limits: {
      ...everyLimit(),
      maxImage2d: 8192,
      timestampPeriodNs: 0,
    },
  });
  replies.device(19n, {
    features: 0n,
    // `Limits::minimum()` on the Rust side.
    limits: {
      maxImage2d: 8192,
      maxImage3d: 2048,
      maxImageArrayLayers: 256,
      maxStorageBufferRange: 134_217_728n,
      maxUniformBufferRange: 65536n,
      maxBindGroups: 4,
      maxBindlessDescriptors: 0,
      maxPushConstantSize: 0,
      maxColorAttachments: 4,
      maxSampleCount: 4,
      maxDrawIndirectCount: 1,
      maxComputeWorkgroupSize: [256, 256, 64],
      maxComputeInvocationsPerWorkgroup: 256,
      maxComputeWorkgroupsPerDimension: 65535,
      minUniformBufferOffsetAlignment: 256n,
      minStorageBufferOffsetAlignment: 256n,
      optimalBufferCopyOffsetAlignment: 256n,
      maxSamplerAnisotropy: 1,
      timestampPeriodNs: 0,
    },
  });
  // The refusal, with the gap. `TIMELINE_SEMAPHORE` is bit 9 — the flag
  // `DeviceDesc::for_adapter` requires and WebGPU has no answer for.
  replies.deviceFailed(
    3n,
    'no WebGPU feature satisfies Features(TIMELINE_SEMAPHORE) — ✱',
    1n << 9n
  );
  // …and a failure that is not about features: the two halves are independent.
  replies.deviceFailed(29n, '', 0n);
  // **Three lists of three different lengths**, which is the point of this
  // entry: three equal counts is what a browser's own answer looks like, and a
  // writer that emitted one count for all three would still produce the right
  // bytes for that. The formats end on the highest code the table claims, so an
  // off-by-one at the end of it is a byte difference here.
  replies.surfaceCaps(37n, {
    formats: [
      FORMAT.BGRA8_UNORM_SRGB,
      FORMAT.RGBA16_FLOAT,
      FORMAT.BC7_RGBA_UNORM_SRGB,
    ],
    presentModes: [
      PRESENT_MODE.FIFO,
      PRESENT_MODE.FIFO_RELAXED,
      PRESENT_MODE.MAILBOX,
      PRESENT_MODE.IMMEDIATE,
    ],
    compositeAlpha: [COMPOSITE_ALPHA.OPAQUE, COMPOSITE_ALPHA.INHERIT],
    minImageCount: 2,
    maxImageCount: 8,
    currentExtent: [1920, 1080],
  });
  // What a WebGPU canvas actually reports, and the **absent** extent: there is
  // no `currentExtent` query in WebGPU at all, so `null` is the ordinary answer
  // from this side rather than a corner case.
  replies.surfaceCaps(41n, {
    formats: [FORMAT.BGRA8_UNORM],
    presentModes: [PRESENT_MODE.FIFO],
    compositeAlpha: [COMPOSITE_ALPHA.OPAQUE, COMPOSITE_ALPHA.PRE_MULTIPLIED],
    minImageCount: 2,
    maxImageCount: 2,
    currentExtent: null,
  });
  // **A zero extent that is present.** `[0, 0]` is what an unconfigured or
  // minimised window reports, so it cannot be the spelling of absent — and
  // these bytes differ from the entry above by the presence byte itself, not
  // only by the two zeroes, so a writer that collapsed the two would produce a
  // buffer eight bytes shorter here rather than the same one.
  replies.surfaceCaps(43n, {
    formats: [FORMAT.R8_UNORM],
    presentModes: [PRESENT_MODE.FIFO],
    compositeAlpha: [COMPOSITE_ALPHA.POST_MULTIPLIED],
    minImageCount: 0,
    maxImageCount: 0,
    currentExtent: [0, 0],
  });
  // **One per cause, and the query has one.** Every other consumer of this table
  // agrees with itself by construction — the replayer's own checks read the same
  // `SURFACE_CAPS_FAILURE` the writer does — so this byte against a fixture Rust
  // wrote is the only thing holding the two languages' failure codes together. A
  // `BACKEND` still spelled `0x03`, as it was while the wire carried four
  // causes, is a byte difference here and nowhere else. The reason and the cause
  // are independent halves, so the second of these carries no reason.
  replies.surfaceCapsFailed(
    47n,
    'getPreferredCanvasFormat() answered "rgba32float" — ✱',
    SURFACE_CAPS_FAILURE.BACKEND
  );
  replies.surfaceCapsFailed(53n, '', SURFACE_CAPS_FAILURE.BACKEND);
}

/**
 * A well-formed capability record, for the checks that break exactly one thing
 * about it. Fresh each call, since several of them mutate it.
 */
function browserShapedSurfaceCaps() {
  return {
    formats: [FORMAT.BGRA8_UNORM],
    presentModes: [PRESENT_MODE.FIFO],
    compositeAlpha: [COMPOSITE_ALPHA.OPAQUE, COMPOSITE_ALPHA.PRE_MULTIPLIED],
    minImageCount: 2,
    maxImageCount: 2,
    currentExtent: null,
  };
}

/**
 * A `GPUDevice` stub as `requestDevice()` returns one when nothing optional was
 * asked for: no features of its own.
 *
 * Its `features` is an empty set rather than absent, because that is what a real
 * device has — and the four bits the mapping produces from it are the ones core
 * WebGPU grants with no name behind them. Only the features are read from it
 * here; the limits below are spelled out for `everyLimit`'s reason.
 */
function defaultDevice() {
  return { features: new Set() };
}

/**
 * The offset of the first byte the two disagree on, or `-1` when they are
 * identical. The same figure `fixture.rs` prints on the other side.
 *
 * @param {Uint8Array} committed
 * @param {Uint8Array} encoded
 */
function firstDifference(committed, encoded) {
  const shared = Math.min(committed.length, encoded.length);
  for (let at = 0; at < shared; at += 1) {
    if (committed[at] !== encoded[at]) return at;
  }
  return committed.length === encoded.length ? -1 : shared;
}

/**
 * Runs `encode` and reports whether it threw the `ReplyEncodeError` it should.
 *
 * @param {string} what
 * @param {string} kind
 * @param {() => void} encode
 */
function checkRefused(what, kind, encode) {
  let thrown = null;
  try {
    encode();
  } catch (error) {
    thrown = error;
  }
  check(
    thrown instanceof ReplyEncodeError && thrown.kind === kind,
    `${what} (got ${thrown === null ? 'no throw' : String(thrown)})`
  );
}

async function main() {
  const override = process.argv.slice(2).find((arg) => !arg.startsWith('--'));
  const path = override === undefined ? FIXTURE : override;
  const committed = new Uint8Array(await readFile(path));

  console.log(
    `reply-encode: ${override ?? FIXTURE.pathname} (${committed.length} bytes)`
  );

  // ---- the canonical replies encode to exactly the committed bytes ---------
  const replies = new ReplyWriter();
  check(
    replies.bytes.length === REPLY_HEADER_BYTES,
    `a fresh writer is a header and nothing else (${replies.bytes.length} bytes)`
  );
  encodeCanonicalReplies(replies);
  const encoded = replies.bytes;

  const at = firstDifference(committed, encoded);
  check(
    at === -1,
    at === -1
      ? `the canonical replies encode to the committed fixture (${encoded.length} bytes)`
      : `the canonical replies encode to the committed fixture — first difference at byte ${at}` +
          ` (committed ${committed.length} bytes, encoded ${encoded.length});` +
          ' both halves of this format are hand-written, so a change in web/engine/gpu-reply.js' +
          ' is a change crates/crcbl-webgpu/src/{tag,reply}.rs has to make too'
  );

  // ---- a cleared writer starts again from a header ------------------------
  // The per-frame path: a writer is reused across frames, and a `clear` that
  // left the old replies behind would send every answer twice — which the far
  // side reports as an unexpected sequence, a frame after the mistake.
  replies.clear();
  check(
    replies.bytes.length === REPLY_HEADER_BYTES,
    `a cleared writer is a header again (${replies.bytes.length} bytes)`
  );
  encodeCanonicalReplies(replies);
  check(
    firstDifference(committed, replies.bytes) === -1,
    'and encodes the same bytes the second time round'
  );

  // ---- what the writer must refuse ----------------------------------------
  // Each of these is a cap the Rust reader enforces, so encoding past one would
  // produce a buffer wasm refuses. Throwing here names the call that did it.
  checkRefused('a payload past the field cap is refused', 'InvalidLength', () =>
    new ReplyWriter().readbackReady(
      1n,
      handle(1, 1),
      new Uint8Array(MAX_FIELD_BYTES + 1)
    )
  );
  checkRefused(
    'an array past the element cap is refused',
    'InvalidLength',
    () =>
      new ReplyWriter().queryResults(
        1n,
        handle(1, 1),
        0,
        new BigUint64Array(MAX_ELEMENT_COUNT + 1)
      )
  );
  checkRefused(
    'a handle with no generation is refused rather than sent as a null handle',
    'NullHandle',
    () => new ReplyWriter().readbackPending(1n, handle(1, 0))
  );
  checkRefused(
    'a sequence handed in as a number is refused rather than rounded',
    'NotABigInt',
    // @ts-expect-error — the mistake this check exists for.
    () => new ReplyWriter().readbackPending(1, handle(1, 1))
  );
  checkRefused(
    'a query value handed in as a number is refused too',
    'NotABigInt',
    // @ts-expect-error — as above, one level down.
    () => new ReplyWriter().queryResults(1n, handle(1, 1), 0, [7])
  );

  // ---- a name is capped by its bytes, not its code points -----------------
  // The cap the far side enforces is on the encoded UTF-8, so a name that is
  // under the cap in characters and over it in bytes has to be refused. A
  // writer that checked `String.length` would send it and be rejected by wasm.
  checkRefused(
    'a name under the cap in characters and over it in bytes is refused',
    'InvalidLength',
    () =>
      new ReplyWriter().adapter(1n, {
        ...browserShapedAdapter(),
        name: '✱'.repeat(MAX_FIELD_BYTES / 2),
      })
  );

  // ---- a field left out is refused, not written as zero -------------------
  // The one failure this format cannot detect for itself: `0` is a legal value
  // for a vendor id, a feature word and most limits, so a key that never
  // arrived would decode as a device that genuinely reports nothing. Each of
  // these drops one field, and each has to throw where the mistake is.
  for (const [field, drop] of [
    ['vendorId', (info) => delete info.vendorId],
    ['deviceType', (info) => delete info.deviceType],
    ['limits.maxBindGroups', (info) => delete info.limits.maxBindGroups],
    [
      'limits.maxComputeWorkgroupSize',
      (info) => delete info.limits.maxComputeWorkgroupSize,
    ],
    [
      'limits.timestampPeriodNs',
      (info) => delete info.limits.timestampPeriodNs,
    ],
  ]) {
    checkRefused(
      `an adapter missing ${field} is refused rather than written as zero`,
      'NotANumber',
      () => {
        const info = browserShapedAdapter();
        drop(info);
        new ReplyWriter().adapter(1n, info);
      }
    );
  }
  // …and the `u64` half of the same mistake, which lands on the `BigInt` rule.
  checkRefused(
    'an adapter whose feature word is a number rather than a BigInt is refused',
    'NotABigInt',
    () =>
      new ReplyWriter().adapter(1n, { ...browserShapedAdapter(), features: 0 })
  );
  checkRefused(
    'a limit that must be a BigInt is refused as a number',
    'NotABigInt',
    () => {
      const info = browserShapedAdapter();
      info.limits.maxStorageBufferRange = 134_217_728;
      new ReplyWriter().adapter(1n, info);
    }
  );
  // ---- the device replies refuse the same mistakes ------------------------
  // A device record is the capability half of an adapter's and goes through the
  // same writer, so what it must refuse is the same list — stated here because
  // "it shares a method" is a fact about today's code rather than a contract.
  checkRefused(
    'a device whose feature word is a number rather than a BigInt is refused',
    'NotABigInt',
    () => new ReplyWriter().device(1n, { features: 0, limits: everyLimit() })
  );
  checkRefused(
    'a device missing a limit is refused rather than written as zero',
    'NotANumber',
    () => {
      const limits = everyLimit();
      delete limits.maxColorAttachments;
      new ReplyWriter().device(1n, { features: 0n, limits });
    }
  );
  // The failure's two halves: the reason is a string and the gap is a `u64`, so
  // the gap is where the `BigInt` rule lands.
  checkRefused(
    'a failure whose missing-features word is a number is refused',
    'NotABigInt',
    // @ts-expect-error — the mistake this check exists for.
    () => new ReplyWriter().deviceFailed(1n, 'why', 0)
  );
  checkRefused(
    'a failure reason past the field cap is refused',
    'InvalidLength',
    () =>
      new ReplyWriter().deviceFailed(1n, 'x'.repeat(MAX_FIELD_BYTES + 1), 0n)
  );

  // ---- the surface-capability record refuses the same mistakes ------------
  // Its own list, because its fields are shaped unlike an adapter's: three
  // counted lists and one optional pair, none of which the checks above reach.
  for (const [field, drop] of [
    ['formats', (caps) => delete caps.formats],
    ['presentModes', (caps) => delete caps.presentModes],
    ['compositeAlpha', (caps) => delete caps.compositeAlpha],
    ['minImageCount', (caps) => delete caps.minImageCount],
    ['maxImageCount', (caps) => delete caps.maxImageCount],
  ]) {
    checkRefused(
      `surface caps missing ${field} are refused rather than written as zero`,
      'NotANumber',
      () => {
        const caps = browserShapedSurfaceCaps();
        drop(caps);
        new ReplyWriter().surfaceCaps(1n, caps);
      }
    );
  }
  // A code that came out of a misspelled table lookup is `undefined`, and every
  // list's first legal code is `0` — `FORMAT.R8_UNORM`, `PRESENT_MODE.FIFO`,
  // `COMPOSITE_ALPHA.OPAQUE` — so writing it as zero would be a plausible
  // answer rather than a malformed one.
  checkRefused(
    'a format code that is not a number is refused rather than written as zero',
    'NotANumber',
    () =>
      new ReplyWriter().surfaceCaps(1n, {
        ...browserShapedSurfaceCaps(),
        formats: [undefined],
      })
  );
  // **An absent extent is `null` and nothing else.** A key that never arrived
  // is `undefined`, and treating that as absent is exactly the failure this
  // format cannot detect for itself: the far side would decode a surface that
  // genuinely reports no size, which is what a browser legitimately says.
  checkRefused(
    'a missing currentExtent is refused rather than encoded as absent',
    'NotANumber',
    () => {
      const caps = browserShapedSurfaceCaps();
      delete caps.currentExtent;
      new ReplyWriter().surfaceCaps(1n, caps);
    }
  );
  checkRefused(
    'a currentExtent that is not two numbers is refused',
    'NotANumber',
    () =>
      new ReplyWriter().surfaceCaps(1n, {
        ...browserShapedSurfaceCaps(),
        currentExtent: [1920],
      })
  );
  // The two spellings that must not converge, checked as bytes rather than as a
  // throw: `[0, 0]` is a size, `null` is the absence of one.
  {
    const absent = new ReplyWriter();
    absent.surfaceCaps(1n, browserShapedSurfaceCaps());
    const zero = new ReplyWriter();
    zero.surfaceCaps(1n, {
      ...browserShapedSurfaceCaps(),
      currentExtent: [0, 0],
    });
    check(
      zero.bytes.length === absent.bytes.length + 8,
      `a zero extent is present and an absent one is not (${absent.bytes.length} vs ${zero.bytes.length} bytes)`
    );
  }

  // ---- and so does the refusal that answers the same query ----------------
  // Its cause is one byte, and `0` is the only legal value of it, so a code that
  // never arrived would be written as exactly the byte a correct call writes —
  // indistinguishable on the wire and undetectable by the far side. That is why
  // this check is here rather than left to the decoder.
  checkRefused(
    'a failure with no cause is refused rather than written as BACKEND',
    'NotANumber',
    // @ts-expect-error — the mistake this check exists for.
    () => new ReplyWriter().surfaceCapsFailed(1n, 'stale surface')
  );
  checkRefused(
    'a failure cause that is not a byte is refused',
    'NotANumber',
    () => new ReplyWriter().surfaceCapsFailed(1n, 'stale surface', 256)
  );
  checkRefused(
    'a failure reason past the field cap is refused here too',
    'InvalidLength',
    () =>
      new ReplyWriter().surfaceCapsFailed(
        1n,
        'x'.repeat(MAX_FIELD_BYTES + 1),
        SURFACE_CAPS_FAILURE.BACKEND
      )
  );
  // **And a refused one leaves no bytes behind.** This reply is the one whose
  // caller has the least room to recover: it is already the answer to a query
  // that failed, so a caller that caught a throw from it has nothing left to
  // send but a shorter version of the same refusal — and would be appending it
  // to a reason and a tag already in the buffer. The rollback is what makes that
  // second attempt land in a buffer wasm can still decode.
  {
    const replies = new ReplyWriter();
    replies.readbackPending(1n, handle(1, 1));
    const good = replies.bytes.slice();
    let thrown = null;
    try {
      // The reason encodes, the cause does not — so the throw lands after the
      // tag, the sequence and a length-prefixed string are already written.
      replies.surfaceCapsFailed(2n, 'half a refusal', undefined);
    } catch (error) {
      thrown = error;
    }
    check(
      thrown instanceof ReplyEncodeError,
      `a half-written refusal was refused (${String(thrown)})`
    );
    check(
      firstDifference(good, replies.bytes) === -1 &&
        replies.bytes.length === good.length,
      `and the buffer is byte for byte what it was before it (${good.length} vs ${replies.bytes.length} bytes)`
    );
    replies.surfaceCapsFailed(2n, '', SURFACE_CAPS_FAILURE.BACKEND);
    check(
      replies.bytes.length > good.length,
      'and the refusal written in its place still lands'
    );
  }

  // A workgroup size of the wrong length is a shape mistake rather than a
  // missing field, and would otherwise write two axes and run on into the
  // fields after them.
  checkRefused(
    'a workgroup size that is not three numbers is refused',
    'NotANumber',
    () => {
      const info = browserShapedAdapter();
      info.limits.maxComputeWorkgroupSize = [64, 64];
      new ReplyWriter().adapter(1n, info);
    }
  );

  if (failures.length > 0) {
    console.error(`\nreply-encode: FAILED (${failures.length})`);
    process.exit(1);
  }
  console.log('\nreply-encode: OK');
}

await main();
