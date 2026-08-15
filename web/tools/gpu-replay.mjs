#!/usr/bin/env node
// The replayer, driven under node against a `navigator.gpu` that is not one.
//
// `web/engine/gpu-replay.js` is the half of this seam that actually calls the
// browser, and calling the browser is exactly what no check in CI can do. What
// *can* be checked without one, and is checked here, is everything around that
// call: which command it answers, which sequence the answer names, that the
// answer is not available on the frame that asked for it, and that every way of
// getting no adapter still produces a reply rather than silence.
//
// WHY THAT LAST ONE IS THE POINT. A dropped reply is a command wasm waits on for
// ever — the one unrecoverable bug this channel can have. There are four ways to
// fail to get an adapter: the promise resolves `null`, the promise rejects,
// `navigator.gpu` is absent altogether, and the reason string is too long for
// the writer's cap. Each is driven below, and each has to end with a reply.
//
// AND THE DEVICE REQUEST HAS FIVE MORE, which is why it has a section of its
// own: an adapter id nothing enumerated, a `compatible_surface` this replayer
// has no table for, a required feature with no `GPUFeatureName` at all, a
// required feature whose name exists and whose adapter does not have it, and a
// `requestDevice` that simply rejects. The first four are refused *before* the
// browser is asked — each is something WebGPU cannot be told — and every one of
// the five still answers.
//
// THE SURFACE COMMANDS ANSWER NOTHING, so what is checked about them is what
// they did rather than what they replied: that `CreateSurface` resolved the
// canvas its own `canvasId` named — a registry with one canvas in it would pass
// against a replayer that ignored the key altogether, so there are two — that it
// asked that canvas for `'webgpu'`, that it did **not** configure the context it
// got, that a `DestroySurface` for a handle nothing created is a no-op, and that
// one for a live handle lets go. A canvas the registry does not have is the
// failure this seam has no reply channel for, and the check below is that it
// throws rather than carrying on with a handle wasm believes in.
//
// THE BUFFER PAIR ANSWERS NOTHING EITHER, and what is checked about it is what
// reached the device: that a `BufferDesc`'s four fields become a
// `GPUBufferDescriptor`'s three, that the usage word and the memory location
// both land in `usage` and that the flags with no WebGPU bit are refused rather
// than dropped, and that a destroy releases the `GPUBuffer` as well as the slot.
// Where a creation cannot happen at all there is no reply to carry the reason,
// so the reason goes to the replayer's `take_error` queue — and every way of
// failing is driven below, because an error that goes nowhere is the same bug as
// a dropped reply one seam over.
//
// AND THE TABLE UNDER BOTH PAIRS IS THE SAME ONE, which is what the generation
// checks here are about. A handle is `{ index, generation }` so that a stale one
// is detectable; the checks below produce a stale one deliberately — an index
// reissued at a higher generation — and insist that a destroy naming it releases
// nothing.
//
// THE CAPABILITY QUERY IS THE THIRD COMMAND WITH A REPLY, and the only one
// answered inside the call — WebGPU has no asynchronous capability query, and
// hardly a synchronous one either. So almost every field of that record is a
// decision about what a browser can honestly claim rather than something it
// said, and each is read back out of the buffer and compared against a number
// spelled out here. The one field WebGPU does answer is checked against two
// different stub browsers, because a list written out as a constant cannot move
// when the browser's answer moves. Its two refusals — a surface with no context
// here, an adapter no enumeration granted — are checked to be *replies*: a throw
// would lose the frame over what `Instance::surface_caps` calls an ordinary step
// of adapter selection.
//
// THE OTHER CLAIM THIS SECTION MAKES is that a device's capabilities are the
// device's. The stub adapter's limits and the stub device's differ in every
// member and their feature sets differ too, so a replayer that built its reply
// from the adapter it opened — the obvious mistake, since the adapter is right
// there — produces different numbers rather than plausible ones.
//
// WHAT THIS DOES *NOT* CHECK, so nobody reads more into a green run than is
// there. The expected bytes below are built with the same `ReplyWriter` the
// replayer uses, so this says nothing about the reply *format* — a tag or a
// field order wrong in `gpu-reply.js` would produce matching bytes on both
// sides here. That is `reply-encode.mjs`'s job, against the fixture Rust
// commits. What is not circular is everything this file is actually about: the
// replayer's choice of reply, of sequence, and of when.
//
// The command stream it replays is the committed fixture rather than a
// hand-built buffer, so the command it dispatches on is one the Rust encoder
// really wrote.
//
// Usage:
//   node web/tools/gpu-replay.mjs [path-to-fixture.bin]

import { readFile } from 'node:fs/promises';
import { deepStrictEqual } from 'node:assert/strict';

import { DEVICE_TYPE, ReplyWriter } from '../engine/gpu-reply.js';
import {
  HandleTable,
  ReplayError,
  Replayer,
  SurfaceError,
  halAdapterInfoFor,
  halDeviceCapsFor,
  halFeaturesFor,
  halLimitsFor,
  webgpuBufferUsageFor,
  webgpuFeaturesFor,
} from '../engine/gpu-replay.js';
import { decodeStream } from '../engine/gpu-stream.js';

/** The fixture `crcbl-webgpu`'s `fixture.rs` writes. */
const FIXTURE = new URL(
  '../../crates/crcbl-webgpu/tests/fixtures/canonical-stream.bin',
  import.meta.url
);

/**
 * `NO_ADAPTER_REASON` in `web/engine/gpu-replay.js`, restated rather than
 * imported: a value taken from the thing under test agrees with it by
 * construction, and this string is what a page ends up showing a person.
 */
const NO_ADAPTER_REASON = 'navigator.gpu.requestAdapter() granted no adapter';

/** `MAX_REASON_CHARS` in `web/engine/gpu-replay.js`, restated for that reason. */
const MAX_REASON_CHARS = 512;

/** `MAX_PENDING_ERRORS` in `web/engine/gpu-replay.js`, restated for that reason. */
const MAX_PENDING_ERRORS = 64;

/**
 * The `GPUBufferUsage` bits, from the WebGPU specification.
 *
 * Spelled out here as well as in `gpu-replay.js` and deliberately not imported
 * from it: every expected value in this file is written out, because one taken
 * from the thing under test agrees with it whatever it says. These are the
 * numbers a browser's own `GPUBufferUsage` holds, and `browser-e2e.mjs` is what
 * holds this seam's mapping against that object in a real browser.
 */
const GPU_BUFFER_USAGE = Object.freeze({
  MAP_READ: 0x0001,
  MAP_WRITE: 0x0002,
  COPY_SRC: 0x0004,
  COPY_DST: 0x0008,
  INDEX: 0x0010,
  VERTEX: 0x0020,
  UNIFORM: 0x0040,
  STORAGE: 0x0080,
  INDIRECT: 0x0100,
  QUERY_RESOLVE: 0x0200,
});

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
 * Lets every queued microtask and promise callback run.
 *
 * A `setTimeout` rather than an `await Promise.resolve()`: the replayer's chain
 * is several `.then`s deep and a macrotask boundary drains all of them however
 * many that becomes.
 */
function settle() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/**
 * A `navigator.gpu` that answers however the test needs.
 *
 * `canvasFormat` IS DELIBERATELY THE LESS COMMON ONE. Most browsers on most
 * machines prefer `'bgra8unorm'`, and a replayer that wrote its format list out
 * as a constant would almost certainly write that one first — so the stub says
 * `'rgba8unorm'`, and the expected list below has it first. A hardcoded answer
 * fails rather than passing by resemblance.
 *
 * @param {() => Promise<object | null>} answer
 * @param {object} [options]
 * @param {string} [options.canvasFormat] What `getPreferredCanvasFormat()` says.
 */
function stubGpu(answer, { canvasFormat = 'rgba8unorm' } = {}) {
  const stub = {
    calls: 0,
    /** How many times the replayer asked which format the canvas prefers. */
    formatCalls: 0,
    requestAdapter() {
      stub.calls += 1;
      return answer();
    },
    getPreferredCanvasFormat() {
      stub.formatCalls += 1;
      return canvasFormat;
    },
  };
  return stub;
}

/**
 * A canvas as much as this seam sees of one: something with a `getContext`.
 *
 * `label` is what identifies the context it hands out, because the thing the
 * surface checks are about is *which* canvas answered — an implementation that
 * took the first entry in the registry rather than the one the command named
 * produces a context that is somebody else's, and nothing but identity
 * distinguishes them.
 *
 * `getContext` records what it was asked for and answers the same context
 * whatever that was, deliberately: a stub that returned `null` for anything but
 * `'webgpu'` would turn a replayer asking for the wrong string into a *surface
 * failure*, which is a different check's red. Here it lands on the one check
 * that is about the string.
 *
 * @param {string} label
 * @param {object} [options]
 * @param {boolean} [options.grants] Whether the canvas gives up a context at
 *   all. `false` is a browser with no WebGPU, or a canvas already bound to a
 *   `2d` context: `getContext` answers `null` rather than throwing.
 */
function stubCanvas(label, { grants = true } = {}) {
  const context = {
    label,
    /** How many times the replayer configured it. Must stay zero. */
    configures: 0,
    configure() {
      context.configures += 1;
    },
  };
  const canvas = {
    label,
    context,
    /** @type {string[]} Every `getContext` argument, in order. */
    asked: [],
    /** @param {string} type */
    getContext(type) {
      canvas.asked.push(type);
      return grants ? context : null;
    },
  };
  return canvas;
}

/**
 * A `GPUSupportedLimits` with a **distinct** value in every member.
 *
 * Distinct on purpose and not plausible on purpose: the mapping is nineteen
 * assignments between two sets of names that mostly resemble each other, so two
 * members swapped is the likeliest way to get it wrong, and a stub full of
 * realistic numbers — where several members legitimately agree — would let that
 * pass. Every number here is its own.
 */
function stubLimits() {
  return {
    maxTextureDimension2D: 101,
    maxTextureDimension3D: 102,
    maxTextureArrayLayers: 103,
    maxStorageBufferBindingSize: 104,
    maxUniformBufferBindingSize: 105,
    maxBindGroups: 106,
    maxColorAttachments: 107,
    maxComputeWorkgroupSizeX: 108,
    maxComputeWorkgroupSizeY: 109,
    maxComputeWorkgroupSizeZ: 110,
    maxComputeInvocationsPerWorkgroup: 111,
    maxComputeWorkgroupsPerDimension: 112,
    minUniformBufferOffsetAlignment: 113,
    minStorageBufferOffsetAlignment: 114,
    // Members WebGPU has that the seam does not read. Present so the stub is
    // shaped like the real thing, and so a mapping that reached for one of them
    // by mistake would land on a number nothing expects.
    maxBufferSize: 115,
    maxVertexBuffers: 116,
    maxBindingsPerBindGroup: 117,
  };
}

/**
 * An adapter shaped like the browser's, with every `info` field filled.
 *
 * `features` and `limits` are what a real `GPUAdapter` always has; the replayer
 * reads both to build its reply, so a stub without them is not one.
 *
 * @param {object} info
 * @param {string[]} [features] `GPUFeatureName`s the adapter reports.
 */
function stubAdapter(info, features = []) {
  return { info, features: new Set(features), limits: stubLimits() };
}

/**
 * The commands this file dispatches on, taken from the committed fixture by
 * `main` before anything below runs.
 *
 * From the fixture rather than written out here, so what is replayed is a
 * command the Rust encoder really wrote — the same reason the expected fields
 * in `stream-decode.mjs` are *not* taken from a decoder.
 *
 * @type {{ enumerate: object, requestDevice: object, createSurface: object,
 *          surfaceCaps: object, createBuffer: object }}
 */
const FROM_FIXTURE = {
  enumerate: null,
  requestDevice: null,
  createSurface: null,
  surfaceCaps: null,
  createBuffer: null,
};

/** A one-command frame carrying `command` at `sequence`. */
function frameOf(command, sequence) {
  return { baseSequence: sequence, commands: [command] };
}

/**
 * A `GPUSupportedLimits` for a **device**, distinct in every member from
 * `stubLimits`.
 *
 * The point of the whole device section: WebGPU gives a device the limits it
 * was created with — the specification's defaults, since a `DeviceDesc` asks
 * for none — and those are not the adapter's ceilings. A replayer that read a
 * device's capabilities off its adapter would produce `stubLimits`'s numbers
 * here, and every one of them differs.
 */
function deviceLimits() {
  return {
    maxTextureDimension2D: 201,
    maxTextureDimension3D: 202,
    maxTextureArrayLayers: 203,
    maxStorageBufferBindingSize: 204,
    maxUniformBufferBindingSize: 205,
    maxBindGroups: 206,
    maxColorAttachments: 207,
    maxComputeWorkgroupSizeX: 208,
    maxComputeWorkgroupSizeY: 209,
    maxComputeWorkgroupSizeZ: 210,
    maxComputeInvocationsPerWorkgroup: 211,
    maxComputeWorkgroupsPerDimension: 212,
    minUniformBufferOffsetAlignment: 213,
    minStorageBufferOffsetAlignment: 214,
  };
}

/**
 * A `GPUBuffer` as `createBuffer` answers one.
 *
 * The three members a real one reports back — `label`, `size` and `usage` — are
 * read off the descriptor rather than stored beside it, because that is what a
 * `GPUBuffer` does and because a check reading them back is then reading what
 * was actually *asked for*. `label` defaults to `''` for the same reason: a
 * descriptor with no label produces a buffer whose label is the empty string,
 * which is why WebGPU cannot tell `None` from `Some("")`.
 *
 * @param {{ label?: string, size: number, usage: number }} desc
 */
function stubBuffer(desc) {
  const buffer = {
    label: desc.label ?? '',
    size: desc.size,
    usage: desc.usage,
    /** How many times the replayer destroyed it. */
    destroys: 0,
    destroy() {
      buffer.destroys += 1;
    },
  };
  return buffer;
}

/**
 * A `GPUDevice` as `requestDevice()` resolves one: its own features, its own
 * limits, the buffer creation this slice drives, and the error channel WebGPU
 * reports asynchronous failures on.
 *
 * `addEventListener` is not optional and is not a courtesy to the replayer: a
 * `GPUDevice` is an `EventTarget`, `uncapturederror` is the only way a browser
 * says a `createBuffer` was invalid, and a stub without one would let a replayer
 * that stopped listening pass here.
 *
 * @param {string[]} [features]
 * @param {object} [options]
 * @param {unknown} [options.refuseBuffers] What `createBuffer` throws instead of
 *   answering — an allocation failure, which is the one buffer failure WebGPU
 *   raises in the call rather than on the device.
 */
function stubDevice(features = [], { refuseBuffers } = {}) {
  const device = {
    features: new Set(features),
    limits: deviceLimits(),
    /** @type {object[]} Every `GPUBufferDescriptor` it was handed, in order. */
    created: [],
    /** @type {Array<[string, Function]>} */
    listeners: [],
    /**
     * @param {string} type
     * @param {Function} listener
     */
    addEventListener(type, listener) {
      device.listeners.push([type, listener]);
    },
    /**
     * Reports an error the way a browser does: on the device, after the call
     * that caused it has already returned a plausible object.
     *
     * @param {string} message
     */
    report(message) {
      for (const [type, listener] of device.listeners) {
        if (type === 'uncapturederror') listener({ error: { message } });
      }
    },
    /** @param {{ label?: string, size: number, usage: number }} desc */
    createBuffer(desc) {
      device.created.push(desc);
      if (refuseBuffers !== undefined) throw refuseBuffers;
      return stubBuffer(desc);
    },
  };
  return device;
}

/**
 * An adapter that opens devices and records what it was asked for.
 *
 * `requests` is what says the descriptor this replayer built is the one WebGPU
 * would want — the feature *names*, which is the half of the mapping the reply
 * comparisons cannot see.
 *
 * @param {object} [options]
 * @param {string[]} [options.features] `GPUFeatureName`s the adapter has.
 * @param {object} [options.device] What `requestDevice` resolves to.
 * @param {unknown} [options.refuse] What it rejects with instead.
 */
function openingAdapter({ features = [], device, refuse } = {}) {
  const adapter = {
    info: { vendor: 'crcbl', device: 'stub' },
    features: new Set(features),
    limits: stubLimits(),
    /** @type {object[]} */
    requests: [],
    async requestDevice(desc) {
      adapter.requests.push(desc);
      if (refuse !== undefined) throw refuse;
      return device ?? stubDevice();
    },
  };
  return adapter;
}

/**
 * Enumerates `adapter`, then replays `command` and lets the answer settle.
 *
 * The two-step is the seam's own shape and not test scaffolding: a device
 * request names an adapter by the id an *enumeration* gave it, so a replayer
 * that had never enumerated has nothing to open. The enumeration's own reply is
 * cleared in between so what comes back is the device answer alone — a `clear`
 * a page would only ever do once wasm had taken the bytes.
 *
 * @param {object} adapter
 * @param {object} command
 * @param {bigint} sequence
 */
async function openDevice(adapter, command, sequence) {
  const replayer = new Replayer({ gpu: stubGpu(async () => adapter) });
  replayer.replay(frameOf(FROM_FIXTURE.enumerate, 0n));
  await settle();
  replayer.clear();
  replayer.replay(frameOf(command, sequence));
  await settle();
  return replayer;
}

/**
 * A replayer that has enumerated one adapter and holds the fixture's surface.
 *
 * **Neither is what the query reads**, and that is deliberate rather than left
 * over: the command carries no ids, so this replayer's surface table and adapter
 * list are not consulted when it is answered. The two-step is here so the
 * checks below run against the state a real page is in, and the bare replayer a
 * few checks further down is what says the answer does not depend on it.
 * The enumeration's reply is cleared in between, so what comes back afterwards
 * is the capability answer alone.
 *
 * @param {object} [options]
 * @param {string} [options.canvasFormat] What the stub browser prefers.
 */
/**
 * A replayer with a device open, which is what a buffer command needs.
 *
 * The enumeration and the device request are the fixture's own, and their
 * replies are cleared in between so that what a buffer check then reads out of
 * the buffer is a buffer command's doing. `Device::create_buffer` is a *device*
 * method, so this two-step is the seam's shape rather than scaffolding: a
 * `CreateBuffer` arriving before the device has opened is a real case and has
 * its own check below.
 *
 * @param {object} [options]
 * @param {object} [options.device] What `requestDevice` resolves to.
 */
async function readyForBuffers({ device = stubDevice() } = {}) {
  const adapter = openingAdapter({ device });
  const replayer = await openDevice(adapter, FROM_FIXTURE.requestDevice, 20n);
  replayer.clear();
  return { replayer, device };
}

async function readyForCaps({ canvasFormat } = {}) {
  const gpu = stubGpu(async () => openingAdapter(), { canvasFormat });
  const canvases = new Map([
    [FROM_FIXTURE.createSurface.canvasId, stubCanvas('caps')],
  ]);
  const replayer = new Replayer({ gpu, canvases });
  replayer.replay(frameOf(FROM_FIXTURE.enumerate, 0n));
  await settle();
  replayer.replay(frameOf(FROM_FIXTURE.createSurface, 1n));
  replayer.clear();
  return { replayer, gpu };
}

/**
 * How long a whole `SurfaceCapsFailed` reply is, for a given reason.
 *
 * The header, the tag, the sequence, the reason's length prefix and its bytes,
 * and the cause. What it is for is the buffer *around* the reply: a writer that
 * left a half-written record behind before answering would produce a longer
 * buffer holding one decodable reply, and every check that decodes the first
 * reply would still pass.
 *
 * @param {string} reason
 */
function failureReplyBytes(reason) {
  return 10 + 1 + 8 + 4 + new TextEncoder().encode(reason).length + 1;
}

/**
 * @param {number} index
 * @param {number} generation
 */
function handle(index, generation) {
  return { index, generation };
}

/**
 * The one reply in `bytes`, as far as the checks here need to read it.
 *
 * A READER FOR THIS FILE, AND NOT A CODEC. The reply format has exactly two
 * implementations and both are held to the committed fixture — Rust writes it,
 * `reply-encode.mjs` re-encodes it byte for byte — so nothing here defines
 * anything. What this exists for is the refusals: their reason is a string the
 * replayer *composes*, and asserting on whole buffers would mean restating that
 * wording verbatim, which pins prose rather than behaviour. So the two fields
 * that are behaviour — which reply came back, and the feature bits it blames —
 * are read out and asserted, and the wording is only ever checked for the part
 * that has to be actionable.
 *
 * Only the two device replies and the two surface-capability ones are
 * understood; anything else is a bug in the check that called this. Of a
 * `Device` it reads the feature word and the first limit, which are the two the
 * checks below are about — and reading them *out of the buffer* is what makes
 * those checks about the replayer rather than about the mapping function they
 * would otherwise call twice. The capability record is read whole for the same
 * reason and a sharper one: every field of it is a decision this replayer made
 * about what a browser can honestly answer, and comparing against numbers spelled
 * out below is what says those are the decisions it made.
 *
 * A BUFFER WITH NO REPLY IN IT IS `undefined`, not a crash. "Nothing came back"
 * is a real outcome and the one several checks below exist to rule out — a
 * replayer that threw instead of answering leaves exactly this — so it has to
 * arrive at the check that asserts on it rather than as a `RangeError` from a
 * `DataView` three frames up, which names nothing and stops the run.
 *
 * @param {Uint8Array} bytes A whole reply buffer, header included.
 * @returns {{ tag: string, sequence: bigint, features?: bigint,
 *             maxImage2d?: number, reason?: string, unsupported?: bigint,
 *             caps?: object, cause?: number } | undefined}
 */
function decodeOneReply(bytes) {
  // The header is the magic and the version word; every reply then opens with
  // its tag and the sequence it answers.
  if (bytes.length <= 10) return undefined;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 10;
  const tag = view.getUint8(at);
  const sequence = view.getBigUint64(at + 1, true);
  at += 9;
  /** A `u32` count, then that many one-byte enum codes. */
  const enumList = () => {
    const count = view.getUint32(at, true);
    at += 4;
    const codes = [...bytes.subarray(at, at + count)];
    at += count;
    return codes;
  };
  /** A `u32` length prefix, then that many UTF-8 bytes. */
  const string = () => {
    const length = view.getUint32(at, true);
    at += 4;
    const text = new TextDecoder('utf-8', { fatal: true }).decode(
      bytes.subarray(at, at + length)
    );
    at += length;
    return text;
  };
  if (tag === 0x02) {
    return {
      tag: 'Device',
      sequence,
      // `DeviceCaps` is the feature word and then `Limits` in declaration
      // order, whose first field is `max_image_2d`.
      features: view.getBigUint64(at, true),
      maxImage2d: view.getUint32(at + 8, true),
    };
  }
  if (tag === 0x03) {
    const reason = string();
    return {
      tag: 'DeviceFailed',
      sequence,
      reason,
      unsupported: view.getBigUint64(at, true),
    };
  }
  if (tag === 0x04) {
    // `SurfaceCaps` in declaration order: three counted lists of codes, two
    // counts, then an extent behind a presence byte. Spelled out one statement
    // at a time rather than as an object literal, so that reading the lists
    // before the counts is stated rather than left to evaluation order.
    const formats = enumList();
    const presentModes = enumList();
    const compositeAlpha = enumList();
    const minImageCount = view.getUint32(at, true);
    const maxImageCount = view.getUint32(at + 4, true);
    at += 8;
    const currentExtent =
      view.getUint8(at) === 1
        ? [view.getUint32(at + 1, true), view.getUint32(at + 5, true)]
        : null;
    return {
      tag: 'SurfaceCaps',
      sequence,
      caps: {
        formats,
        presentModes,
        compositeAlpha,
        minImageCount,
        maxImageCount,
        currentExtent,
      },
    };
  }
  if (tag !== 0x05) {
    throw new Error(`decodeOneReply does not read tag 0x${tag.toString(16)}`);
  }
  const reason = string();
  return {
    tag: 'SurfaceCapsFailed',
    sequence,
    reason,
    cause: view.getUint8(at),
  };
}

/**
 * One decoded reply as a line a failure message can carry.
 *
 * Hand-written rather than `JSON.stringify`, which throws on a `BigInt` — and
 * both interesting fields here are one.
 *
 * @param {ReturnType<typeof decodeOneReply> | undefined} reply
 */
function shown(reply) {
  if (reply === undefined) return 'nothing';
  return (
    `${reply.tag} answering ${reply.sequence}` +
    `, unsupported 0x${(reply.unsupported ?? 0n).toString(16)}` +
    `, reason ${JSON.stringify(reply.reason ?? '')}`
  );
}

/**
 * What a `ReplyWriter` produces for one reply, for comparing against what the
 * replayer produced. See the header for what this does and does not prove.
 *
 * @param {(replies: ReplyWriter) => void} encode
 */
function expectedReplies(encode) {
  const replies = new ReplyWriter();
  encode(replies);
  return replies.bytes;
}

async function main() {
  const override = process.argv.slice(2).find((arg) => !arg.startsWith('--'));
  const path = override === undefined ? FIXTURE : override;
  const fixture = new Uint8Array(await readFile(path));
  // Caught for `stream-decode.mjs`'s reason: a decoder that has drifted from
  // the fixture refuses it outright, and nothing below this line means anything
  // once it has. That is that tool's failure to report, not this one's, so it
  // lands as one named check rather than as a stack trace out of a file this
  // suite is not testing.
  /** @type {object[]} */
  let commands;
  try {
    commands = decodeStream(fixture);
  } catch (error) {
    check(false, `the fixture decodes at all (threw ${String(error)})`);
    console.error(`\ngpu-replay: FAILED (${failures.length})`);
    process.exit(1);
  }

  console.log(
    `gpu-replay: ${override ?? FIXTURE.pathname} (${commands.length} commands)`
  );

  // ---- the command this replays is one the Rust encoder writes -------------
  const enumerateAt = commands.findIndex(
    (command) => command.name === 'EnumerateAdapters'
  );
  check(
    enumerateAt >= 0,
    `the committed stream carries an EnumerateAdapters (at ${enumerateAt})`
  );
  const enumerate = commands[enumerateAt];
  FROM_FIXTURE.enumerate = enumerate;

  // The device request the fixture carries with an adapter id an enumeration
  // can actually have granted. The corpus holds two; the other names adapter 3
  // and a surface, and is used below for exactly those refusals.
  const requestDevice = commands.find(
    (command) => command.name === 'RequestDevice' && command.adapter === 0
  );
  check(
    requestDevice !== undefined,
    'the committed stream carries a RequestDevice for adapter 0'
  );
  FROM_FIXTURE.requestDevice = requestDevice;

  // The surface pair, from the fixture for the same reason and with the same
  // consequence: the canvas key replayed below is the `u32` the Rust encoder
  // wrote, not a number invented here.
  const createSurface = commands.find(
    (command) => command.name === 'CreateSurface'
  );
  const destroySurface = commands.find(
    (command) => command.name === 'DestroySurface'
  );
  check(
    createSurface !== undefined && destroySurface !== undefined,
    'the committed stream carries a CreateSurface and a DestroySurface'
  );
  FROM_FIXTURE.createSurface = createSurface;

  // The capability query, from the fixture for the same reason again — and here
  // the point is what it does *not* carry: a decoder that still read a surface
  // and an adapter off the wire would hand this file an object with fields, and
  // the replay below would then be driven by something the encoder never wrote.
  const surfaceCaps = commands.find(
    (command) => command.name === 'SurfaceCaps'
  );
  check(
    surfaceCaps !== undefined,
    'the committed stream carries a SurfaceCaps'
  );
  FROM_FIXTURE.surfaceCaps = surfaceCaps;

  // The buffer pair, from the fixture for the same reason again — and here it
  // buys three descriptors rather than one: the corpus carries a labelled
  // device-local buffer, an unlabelled host-upload one, and one whose size is
  // `u64::MAX`, which is the size no `GPUSize64` can carry exactly. Every
  // memory location and both label cases are therefore commands the Rust
  // encoder really wrote.
  const buffers = commands.filter((command) => command.name === 'CreateBuffer');
  const destroyBuffer = commands.find(
    (command) => command.name === 'DestroyBuffer'
  );
  check(
    buffers.length === 3 && destroyBuffer !== undefined,
    `the committed stream carries three CreateBuffers and a DestroyBuffer (${buffers.length} creates)`
  );
  const [deviceLocalBuffer, hostUploadBuffer, hostReadbackBuffer] = buffers;
  FROM_FIXTURE.createBuffer = deviceLocalBuffer;

  // ---- the answer is not available on the frame that asked ----------------
  // The whole shape of this seam. `replay` returns having *started* the work;
  // the browser answers on its own schedule, and the sequence number is what
  // lets the reply arrive whenever that turns out to be.
  {
    const gpu = stubGpu(async () =>
      stubAdapter({ vendor: 'crcbl', device: 'stub', description: 'a stub' })
    );
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 7n));
    check(
      gpu.calls === 0 && !replayer.hasReplies && replayer.inFlight === 1,
      `replay returns without an answer (calls ${gpu.calls}, queued ${replayer.hasReplies}, in flight ${replayer.inFlight})`
    );

    await settle();
    check(
      gpu.calls === 1 && replayer.hasReplies && replayer.inFlight === 0,
      `the answer lands later (calls ${gpu.calls}, queued ${replayer.hasReplies}, in flight ${replayer.inFlight})`
    );
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.adapter(
          7n,
          halAdapterInfoFor(
            stubAdapter({
              vendor: 'crcbl',
              device: 'stub',
              description: 'a stub',
            })
          )
        )
      ),
      'the reply is an Adapter naming the command that asked, id 0, every info field joined'
    );
  }

  // ---- the sequence is positional, and 64-bit ------------------------------
  // Nothing per command is on the wire: the nth command's number is the
  // buffer's base plus n. A base past 2^53 is where a replayer that did that
  // arithmetic in `Number` stops being exact — and stops in a way that still
  // looks like a plausible sequence.
  {
    const base = 9_007_199_254_740_993n; // 2^53 + 1
    const gpu = stubGpu(async () => stubAdapter({ device: 'positional' }));
    const replayer = new Replayer({ gpu });
    replayer.replay({ baseSequence: base, commands: [enumerate] });
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.adapter(
          base,
          halAdapterInfoFor(stubAdapter({ device: 'positional' }))
        )
      ),
      'a command at base + 0 is answered with exactly that number, past 2^53'
    );
  }

  // ---- an adapter that will not name itself is still an adapter -----------
  {
    const gpu = stubGpu(async () => stubAdapter({}));
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 4n));
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.adapter(4n, halAdapterInfoFor(stubAdapter({})))
      ),
      'an adapter with no info is granted with an empty name rather than refused'
    );
  }

  // ---- every way of getting no adapter still answers ----------------------
  {
    const gpu = stubGpu(async () => null);
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 5n));
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) => replies.noAdapter(5n, NO_ADAPTER_REASON)),
      'a promise resolving null is a NoAdapter with a reason a person can read'
    );
  }
  {
    const gpu = stubGpu(async () => {
      throw new Error('the GPU process is gone');
    });
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 6n));
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.noAdapter(6n, 'Error: the GPU process is gone')
      ),
      'a rejected promise is a NoAdapter carrying what it threw'
    );
  }
  {
    // No `gpu` at all — a browser without WebGPU, where the property is simply
    // absent. Nothing may be silent here: wasm has registered a wait.
    const replayer = new Replayer({ gpu: undefined });
    replayer.replay(frameOf(enumerate, 8n));
    await settle();
    check(
      replayer.hasReplies,
      'a browser with no navigator.gpu is answered rather than left waiting'
    );
  }
  {
    // A reason past the writer's cap would throw *inside a promise callback*,
    // where the throw becomes an unhandled rejection and the reply is simply
    // never queued. Truncation is the guard, and this is what holds it.
    const shout = 'x'.repeat(4 * 1024 * 1024);
    const gpu = stubGpu(async () => {
      throw new Error(shout);
    });
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 9n));
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.noAdapter(9n, `Error: ${shout}`.slice(0, MAX_REASON_CHARS))
      ),
      'a reason past the cap is truncated rather than dropping the reply'
    );
  }

  // ---- replies accumulate across frames -----------------------------------
  // One writer for the life of the page: two enumerations started on different
  // frames land in one buffer, and `clear` is what empties it — after wasm has
  // taken them, never before.
  {
    const gpu = stubGpu(async () => stubAdapter({ device: 'both' }));
    const replayer = new Replayer({ gpu });
    replayer.replay(frameOf(enumerate, 1n));
    await settle();
    replayer.replay(frameOf(enumerate, 2n));
    await settle();
    checkEqual(
      replayer.replies,
      expectedReplies((replies) => {
        replies.adapter(1n, halAdapterInfoFor(stubAdapter({ device: 'both' })));
        replies.adapter(2n, halAdapterInfoFor(stubAdapter({ device: 'both' })));
      }),
      'two frames of answers share one buffer, in the order they settled'
    );
    replayer.clear();
    check(
      !replayer.hasReplies && replayer.replies.length === 10,
      `clear leaves a header and nothing else (${replayer.replies.length} bytes)`
    );
  }

  // ---- a command with no implementation says so, loudly -------------------
  // Skipping it would be a draw that never happened and a frame that renders
  // almost right. Every command in the corpus that is not the one this slice
  // implements has to throw, and the error has to name the sequence — the
  // number wasm's own error attribution is keyed on.
  {
    const replayer = new Replayer({ gpu: stubGpu(async () => null) });
    /** @type {string[]} */
    const wrong = [];
    const implemented = [
      'EnumerateAdapters',
      'RequestDevice',
      'CreateSurface',
      'DestroySurface',
      'SurfaceCaps',
      'CreateBuffer',
      'DestroyBuffer',
    ];
    for (const [index, command] of commands.entries()) {
      if (implemented.includes(command.name)) continue;
      let thrown = null;
      try {
        replayer.replay(frameOf(command, BigInt(index)));
      } catch (error) {
        thrown = error;
      }
      if (
        !(thrown instanceof ReplayError) ||
        thrown.command !== command.name ||
        thrown.sequence !== BigInt(index)
      ) {
        wrong.push(`${command.name} at ${index}: ${String(thrown)}`);
      }
    }
    const unimplemented = commands.filter(
      (command) => !implemented.includes(command.name)
    ).length;
    check(
      wrong.length === 0,
      wrong[0] ??
        `every command this slice cannot replay throws a ReplayError naming it and its sequence (${unimplemented} of them)`
    );
  }

  // ---- the device request --------------------------------------------------
  //
  // The second call that makes the round trip, and the first that has a
  // descriptor to get wrong. Four things are checked here and nowhere else: the
  // reply carries the **device's** capabilities rather than its adapter's, the
  // `requiredFeatures` list handed to WebGPU is the inverse of the mapping the
  // adapter direction uses, every refusal still answers, and a refusal carries
  // the bits that caused it.
  {
    // The answer is not available on the frame that asked, exactly as an
    // enumeration's is not.
    const adapter = openingAdapter();
    const replayer = new Replayer({ gpu: stubGpu(async () => adapter) });
    replayer.replay(frameOf(enumerate, 0n));
    await settle();
    replayer.clear();
    replayer.replay(frameOf(requestDevice, 4n));
    check(
      adapter.requests.length === 0 &&
        !replayer.hasReplies &&
        replayer.inFlight === 1,
      `replay returns without a device (asked ${adapter.requests.length}, queued ${replayer.hasReplies}, in flight ${replayer.inFlight})`
    );
    await settle();
    check(
      adapter.requests.length === 1 &&
        replayer.hasReplies &&
        replayer.inFlight === 0,
      `the device lands later (asked ${adapter.requests.length}, queued ${replayer.hasReplies}, in flight ${replayer.inFlight})`
    );
  }
  {
    // **The capabilities are the device's own.** The adapter here has four
    // mapped features and `stubLimits`'s ceilings; the device it opens has one
    // feature and `deviceLimits`'s numbers, and every one of them differs.
    const device = stubDevice(['timestamp-query']);
    const adapter = openingAdapter({
      features: [
        'depth-clip-control',
        'texture-compression-bc',
        'timestamp-query',
        'indirect-first-instance',
      ],
      device,
    });
    const replayer = await openDevice(adapter, requestDevice, 4n);
    checkEqual(
      replayer.replies,
      expectedReplies((replies) =>
        replies.device(4n, halDeviceCapsFor(device))
      ),
      'the reply is a Device naming the command that asked'
    );

    // …and what that record actually holds, **read back out of the buffer**
    // rather than recomputed. A check that called `halDeviceCapsFor` again
    // would agree with the replayer whichever object the replayer had passed
    // it, which is exactly the mistake this section exists to catch. CORE is
    // the four flags core WebGPU grants; `timestamp-query` is bit 5 and is the
    // only optional one this *device* has.
    const CORE = (1n << 8n) | (1n << 7n) | (1n << 14n) | (1n << 18n);
    const sent = decodeOneReply(replayer.replies);
    checkEqual(
      sent.features,
      CORE | (1n << 5n),
      "the feature word that crossed is the device's own, not the adapter's"
    );
    checkEqual(
      sent.maxImage2d,
      201,
      "the limit that crossed is the device's own, not the adapter's"
    );
    check(
      halFeaturesFor(adapter) !== sent.features &&
        halLimitsFor(adapter).maxImage2d !== sent.maxImage2d,
      'the adapter and the device disagree, so a copy of either would be visible'
    );
    check(
      replayer.device === device,
      'the replayer holds the device it reported, so nothing may collect it'
    );
  }
  {
    // **The descriptor WebGPU is handed.** The fixture's request asks for every
    // optional feature the seam has; only the mapped names this adapter
    // actually reports may reach `requiredFeatures`, because `requestDevice`
    // fails the whole request over one it lacks — which would turn "optional"
    // into fatal.
    const adapter = openingAdapter({
      features: ['timestamp-query', 'texture-compression-bc'],
    });
    await openDevice(adapter, requestDevice, 4n);
    const asked = adapter.requests[0] ?? {};
    checkEqual(
      [...(asked.requiredFeatures ?? [])].sort(),
      ['texture-compression-bc', 'timestamp-query'],
      'only the optional features this adapter has are asked for'
    );
    check(
      !('requiredLimits' in asked),
      'no limits are requested, so the device gets the specification defaults'
    );
    check(
      !('label' in asked),
      'a descriptor with no label passes none rather than an empty one'
    );
  }
  {
    // A label that *is* present crosses, and lands on the descriptor.
    const adapter = openingAdapter();
    await openDevice(
      adapter,
      { ...requestDevice, label: 'engine', optionalFeatures: 0n },
      4n
    );
    checkEqual(
      adapter.requests[0]?.label,
      'engine',
      "the descriptor's label reaches requestDevice"
    );
  }
  {
    // **A required feature with no WebGPU name at all.** `TIMELINE_SEMAPHORE`
    // is bit 9, WebGPU has no semaphores, and this is the case that must fail
    // loudly rather than open a device without it — which is what dropping the
    // bit from the list would do.
    const adapter = openingAdapter();
    const replayer = await openDevice(
      adapter,
      { ...requestDevice, requiredFeatures: 1n << 9n },
      6n
    );
    check(
      adapter.requests.length === 0,
      'the browser is never asked for a feature it cannot express'
    );
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' && answered.unsupported === 1n << 9n,
      `the refusal names the bits that caused it (${shown(answered)})`
    );
    check(
      String(answered?.reason).includes('bit 9'),
      `and says which flag, in a message a person can act on (${answered?.reason})`
    );
  }
  {
    // **A required feature the browser does not have.** The name exists —
    // `timestamp-query` — and this adapter simply does not report it. Refused
    // here rather than handed to `requestDevice`, which would reject with a
    // message that names no seam flag at all.
    const adapter = openingAdapter({ features: [] });
    const replayer = await openDevice(
      adapter,
      { ...requestDevice, requiredFeatures: 1n << 5n },
      7n
    );
    check(
      adapter.requests.length === 0,
      'a required feature the adapter lacks is refused before the request'
    );
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' && answered.unsupported === 1n << 5n,
      `the refusal carries the same shape as an unmappable one (${shown(answered)})`
    );
  }
  {
    // A rejected `requestDevice`. The browser was asked and said no, so the
    // reason is the browser's and there is no feature gap to report.
    const adapter = openingAdapter({
      refuse: new Error('device creation failed'),
    });
    const replayer = await openDevice(
      adapter,
      { ...requestDevice, optionalFeatures: 0n },
      8n
    );
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' &&
        answered.unsupported === 0n &&
        String(answered.reason).includes('device creation failed'),
      `a rejected requestDevice is answered rather than dropped (${shown(answered)})`
    );
  }
  {
    // A reason past the writer's cap would throw inside a promise callback and
    // strand the command; truncation is the guard here as it is for an adapter.
    const adapter = openingAdapter({
      refuse: new Error('x'.repeat(4 * 1024 * 1024)),
    });
    const replayer = await openDevice(
      adapter,
      { ...requestDevice, optionalFeatures: 0n },
      9n
    );
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' &&
        String(answered.reason).length === MAX_REASON_CHARS,
      `a reason past the cap is truncated rather than dropping the reply (${String(answered?.reason).length} chars)`
    );
  }
  {
    // A device request with no enumeration behind it. The id is a position in a
    // list this replayer never answered with, so there is nothing to open — and
    // it still has to answer, because wasm is waiting on that sequence.
    const replayer = new Replayer({
      gpu: stubGpu(async () => openingAdapter()),
    });
    replayer.replay(frameOf(requestDevice, 3n));
    await settle();
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' &&
        String(answered.reason).includes('no adapter 0'),
      `an unenumerated adapter is refused and answered (${shown(answered)})`
    );
  }
  {
    // The fixture's *other* request: adapter 3, and a `compatible_surface`.
    // Both are things this replayer cannot honour, and the surface is the one
    // that must not be honoured silently — a headless device handed to a caller
    // that asked for a presentable one renders nothing and reports nothing.
    const withSurface = commands.find(
      (command) =>
        command.name === 'RequestDevice' && command.compatibleSurface !== null
    );
    check(
      withSurface !== undefined,
      'the committed stream carries a RequestDevice naming a surface'
    );
    const adapter = openingAdapter();
    const replayer = await openDevice(
      adapter,
      { ...withSurface, adapter: 0 },
      5n
    );
    check(
      adapter.requests.length === 0,
      'a request naming a surface never reaches the browser'
    );
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'DeviceFailed' &&
        String(answered.reason).includes('surface'),
      `a compatible_surface this replayer cannot resolve is refused (${shown(answered)})`
    );
  }

  // ---- the surface pair ----------------------------------------------------
  //
  // Neither command has a reply, so every check here reads what the replayer
  // *holds* rather than what it queued. The command and the canvas key are the
  // fixture's, so the id being resolved is the `u32` the Rust encoder wrote.
  {
    // Two canvases, and the one the command names is deliberately not the first
    // the registry offers: a replayer that ignored `canvasId` and took whatever
    // came to hand would pass a one-canvas check and has to fail this one.
    const other = stubCanvas('registered-first');
    const wanted = stubCanvas('the-one-named');
    const canvases = new Map([
      [createSurface.canvasId + 1, other],
      [createSurface.canvasId, wanted],
    ]);
    const replayer = new Replayer({ gpu: stubGpu(async () => null), canvases });
    replayer.replay(frameOf(createSurface, 2n));
    const held = replayer.surfaces.get(createSurface.surface);
    check(
      held === wanted.context && other.asked.length === 0,
      `CreateSurface holds the context of the canvas its id named (${held?.label ?? 'nothing'},` +
        ` and the other canvas was asked ${other.asked.length} times)`
    );
    checkEqual(
      wanted.asked,
      ['webgpu'],
      'and asks that canvas for the webgpu context specifically'
    );
    // **The check that catches the plausible wrong version.** `configure` needs
    // a `GPUDevice` and this command may legitimately run before one exists, so
    // configuring belongs to swapchain creation and not here.
    check(
      wanted.context.configures === 0,
      `and does not configure it, which is the swapchain's call (${wanted.context.configures} configures)`
    );
    check(
      !replayer.hasReplies && replayer.inFlight === 0,
      `a surface command queues no reply and starts nothing (queued ${replayer.hasReplies}, in flight ${replayer.inFlight})`
    );
  }
  {
    // **A destroy of an empty slot.** `crcbl-render` destroys the handle it
    // pre-allocated even when the creation it belonged to failed, so an id
    // nothing here ever created is a legal stream op and not corruption. The
    // fixture's own destroy names a different handle from its create, so this
    // is that command replayed exactly as it was written.
    const replayer = new Replayer({ gpu: stubGpu(async () => null) });
    let thrown = null;
    try {
      replayer.replay(frameOf(destroySurface, 3n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null && replayer.surfaces.size === 0,
      `DestroySurface for a handle nothing created is a no-op (${String(thrown)})`
    );
  }
  {
    // …and one for a live handle lets go of it. The destroy is the fixture's
    // with the create's handle put in, because the two the fixture carries name
    // different surfaces on purpose.
    const canvas = stubCanvas('released');
    const replayer = new Replayer({
      gpu: stubGpu(async () => null),
      canvases: new Map([[createSurface.canvasId, canvas]]),
    });
    replayer.replay(frameOf(createSurface, 4n));
    const held =
      replayer.surfaces.get(createSurface.surface) === canvas.context;
    replayer.replay(
      frameOf({ ...destroySurface, surface: createSurface.surface }, 5n)
    );
    const still = replayer.surfaces.get(createSurface.surface) !== undefined;
    check(
      held && !still,
      `DestroySurface releases the context its handle held (held ${held}, still there ${still})`
    );
  }
  {
    // **A canvas key the page does not have.** There is no reply on this
    // channel for a `create_surface`, so nothing can be told about it — and the
    // two quiet options are worse than a throw: carrying on leaves wasm holding
    // a handle with no context behind it, and inventing a reply would name a
    // sequence nothing is waiting on. The registry here is the default one, a
    // `Replayer` constructed with no canvases at all.
    const replayer = new Replayer({ gpu: stubGpu(async () => null) });
    let thrown = null;
    try {
      replayer.replay(frameOf(createSurface, 6n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown instanceof SurfaceError &&
        thrown.kind === 'NoSuchCanvas' &&
        thrown.canvasId === createSurface.canvasId &&
        thrown.sequence === 6n &&
        String(thrown.message).includes(String(createSurface.canvasId)),
      `an unregistered canvas id throws, naming the id and the sequence (${String(thrown)})`
    );
    check(
      replayer.surfaces.size === 0,
      `and no surface is recorded for it (${replayer.surfaces.size} held)`
    );
  }
  {
    // **A canvas that gives up no context.** A browser without WebGPU, or a
    // canvas something already took for a `2d` context: `getContext` answers
    // `null`, which is a failure of the same kind and is refused the same way.
    const canvas = stubCanvas('already-2d', { grants: false });
    const replayer = new Replayer({
      gpu: stubGpu(async () => null),
      canvases: new Map([[createSurface.canvasId, canvas]]),
    });
    let thrown = null;
    try {
      replayer.replay(frameOf(createSurface, 7n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown instanceof SurfaceError &&
        thrown.kind === 'NoCanvasContext' &&
        thrown.canvasId === createSurface.canvasId &&
        replayer.surfaces.size === 0,
      `a canvas with no webgpu context throws rather than recording nothing quietly (${String(thrown)})`
    );
  }

  // ---- the handle table, which both pairs share ---------------------------
  //
  // A handle is `{ index, generation }` so that a stale one is detectable, and
  // this is the only place that fact is checked on its own rather than through a
  // command. What it is for: an index is reissued once the resource it named is
  // destroyed, so a table keyed on the index alone answers a *stale* handle with
  // whatever moved in — and a destroy carrying one would release a live
  // resource somebody else is using.
  {
    const table = new HandleTable();
    const live = handle(4, 2);
    const stale = handle(4, 1);
    const other = handle(5, 2);
    table.insert(live, 'the live one');
    check(
      table.get(live) === 'the live one' && table.size === 1,
      `a handle finds what was filed under it (${table.get(live)}, ${table.size} held)`
    );
    check(
      table.get(stale) === undefined,
      `a stale generation does not resolve to the live occupant (${table.get(stale)})`
    );
    check(
      table.get(other) === undefined,
      `and neither does another index (${table.get(other)})`
    );
    check(
      table.remove(stale) === undefined &&
        table.size === 1 &&
        table.get(live) === 'the live one',
      `removing a stale handle releases nothing (${table.size} held, ${table.get(live)})`
    );
    check(
      table.remove(other) === undefined && table.size === 1,
      `removing an empty slot releases nothing and does not throw (${table.size} held)`
    );
    checkEqual(
      [...table.entries()],
      [[4, 'the live one']],
      'entries names the index a value is filed under'
    );
    check(
      table.remove(live) === 'the live one' && table.size === 0,
      `removing the live handle hands the value back (${table.size} held)`
    );
    // **One table per resource kind, never one keyed on handle bits.** Every
    // HAL handle is the same eight bytes and the opcode is the only thing that
    // says which table an id indexes, so two kinds holding identical bits must
    // not see each other.
    const buffers = new HandleTable();
    const surfaces = new HandleTable();
    const same = handle(7, 3);
    buffers.insert(same, 'a buffer');
    surfaces.insert(same, 'a surface');
    check(
      buffers.get(same) === 'a buffer' && surfaces.get(same) === 'a surface',
      `two kinds may hold the same handle bits (${buffers.get(same)}, ${surfaces.get(same)})`
    );
    buffers.remove(same);
    check(
      surfaces.get(same) === 'a surface' && buffers.size === 0,
      `and destroying one leaves the other alone (${surfaces.get(same)})`
    );
  }

  // ---- the buffer pair -----------------------------------------------------
  //
  // The first commands that reach the *device* rather than the instance, and the
  // first with a descriptor whose translation loses something. Neither has a
  // reply — wasm allocated the handle and moved on — so what is checked is what
  // reached `createBuffer`, what the table holds afterwards, and, where the
  // creation could not happen, what went into the queue `Device::take_error`
  // will drain.
  {
    // **The descriptor WebGPU is handed.** Four seam fields become three: the
    // label passes through, the size narrows from a `u64` to a number, and
    // `usage` and `memory` are both folded into one `GPUBufferUsage` word.
    const { replayer, device } = await readyForBuffers();
    replayer.replay(frameOf(deviceLocalBuffer, 21n));
    checkEqual(
      device.created,
      [
        {
          label: 'instances',
          size: 4096,
          // COPY_DST (0x8) for TRANSFER_DST and STORAGE (0x80) for STORAGE.
          // `DeviceLocal` adds nothing, because a buffer with no mapping usage
          // is the one an implementation may place in device-local memory.
          usage: 0x88,
        },
      ],
      'a CreateBuffer reaches the device as one GPUBufferDescriptor'
    );
    const made = replayer.buffers.get(deviceLocalBuffer.buffer);
    check(
      made !== undefined && made.size === 4096 && made.usage === 0x88,
      `and the buffer is findable at its handle with the size and usage asked for (${made?.size} bytes, usage 0x${made?.usage?.toString(16)})`
    );
    check(
      replayer.buffers.get(
        handle(
          deviceLocalBuffer.buffer.index,
          deviceLocalBuffer.buffer.generation + 1
        )
      ) === undefined,
      'a lookup with a stale generation does not find the live occupant'
    );
    check(
      !replayer.hasReplies &&
        replayer.inFlight === 0 &&
        replayer.pendingErrors === 0 &&
        replayer.takeError() === null,
      `a buffer command queues no reply, starts nothing and reports no error (queued ${replayer.hasReplies}, in flight ${replayer.inFlight}, errors ${replayer.pendingErrors})`
    );
  }
  {
    // **Both other memory locations**, from the fixture's own descriptors. This
    // is the field WebGPU has nowhere to put — there is no heap to select — so
    // each row is a decision `gpu-replay.js` argues and this is what says which
    // decision it made.
    const { replayer, device } = await readyForBuffers();
    replayer.replay(frameOf(hostUploadBuffer, 22n));
    checkEqual(
      device.created,
      [
        {
          size: 1,
          // UNIFORM (0x40), plus the COPY_DST (0x8) `HostUpload` becomes:
          // WebGPU's MAP_WRITE may be combined with COPY_SRC and nothing else,
          // so a host-written uniform buffer cannot carry it, and
          // `queue.writeBuffer` — which is what `Device::write_buffer` is here
          // — needs COPY_DST instead.
          usage: 0x48,
        },
      ],
      'a HostUpload buffer is asked for as a copy destination, not as a mappable one'
    );
    check(
      !('label' in (device.created[0] ?? {})),
      `a descriptor with no label passes none rather than an empty one (${JSON.stringify(device.created[0]?.label)})`
    );
  }
  {
    // `HostReadback` is the one location WebGPU can express outright, and the
    // size is put back inside the safe range because the fixture's own is
    // `u64::MAX` — which is a refusal of its own, two checks below.
    const { replayer, device } = await readyForBuffers();
    replayer.replay(frameOf({ ...hostReadbackBuffer, size: 64n }, 23n));
    checkEqual(
      device.created,
      [
        {
          label: '',
          size: 64,
          // COPY_SRC (0x4) for TRANSFER_SRC, and MAP_READ (0x1), which is what
          // `mapAsync(GPUMapMode.READ)` needs at creation.
          usage: 0x05,
        },
      ],
      'a HostReadback buffer is asked for with MAP_READ, which is what a readback needs'
    );
    check(
      'label' in (device.created[0] ?? {}),
      'and an empty label is passed as one, because the seam distinguishes it from none'
    );
  }
  {
    // **A destroy releases the GPUBuffer as well as the slot.** Dropping the
    // reference alone would leave the allocation alive until the object was
    // collected, which is the whole reason this seam destroys explicitly.
    const { replayer } = await readyForBuffers();
    replayer.replay(frameOf(deviceLocalBuffer, 24n));
    const made = replayer.buffers.get(deviceLocalBuffer.buffer);
    replayer.replay(
      frameOf({ ...destroyBuffer, buffer: deviceLocalBuffer.buffer }, 25n)
    );
    check(
      made?.destroys === 1 &&
        replayer.buffers.get(deviceLocalBuffer.buffer) === undefined &&
        replayer.buffers.size === 0,
      `DestroyBuffer destroys the buffer and lets go of the slot (${made?.destroys} destroys, ${replayer.buffers.size} held)`
    );
  }
  {
    // **A destroy of an empty slot.** The fixture's own destroy names a handle
    // no create in it ever used, which is the legal stream op `crcbl-render`
    // produces when it destroys the handle it pre-allocated for a creation that
    // failed.
    const { replayer } = await readyForBuffers();
    let thrown = null;
    try {
      replayer.replay(frameOf(destroyBuffer, 26n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null && replayer.buffers.size === 0,
      `DestroyBuffer for a handle nothing created is a no-op (${String(thrown)})`
    );
  }
  {
    // **A DESTROY NAMING A STALE GENERATION RELEASES NOTHING.** The case a
    // table keyed on the index alone cannot see: the index is the live buffer's,
    // the generation is the one it had before, and the live occupant must
    // survive — with its `destroy()` never called, because a destroyed buffer
    // that something still holds a handle to is worse than a leaked one.
    const { replayer } = await readyForBuffers();
    const reissued = handle(
      deviceLocalBuffer.buffer.index,
      deviceLocalBuffer.buffer.generation + 1
    );
    replayer.replay(frameOf({ ...deviceLocalBuffer, buffer: reissued }, 27n));
    const live = replayer.buffers.get(reissued);
    let thrown = null;
    try {
      // The stale handle: the same index at the generation the slot no longer
      // holds.
      replayer.replay(
        frameOf({ ...destroyBuffer, buffer: deviceLocalBuffer.buffer }, 28n)
      );
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        live !== undefined &&
        live.destroys === 0 &&
        replayer.buffers.get(reissued) === live &&
        replayer.buffers.size === 1,
      `a destroy naming a stale generation is a no-op and leaves the live buffer alone (${String(thrown)}, ${live?.destroys} destroys, ${replayer.buffers.size} held)`
    );
  }
  {
    // The same claim for a surface, because the two share one table type and the
    // migration is what this is guarding: the surface pair behaved this way by
    // accident before — an index was never reissued — and has to behave this way
    // on purpose now.
    const canvas = stubCanvas('reissued');
    const replayer = new Replayer({
      gpu: stubGpu(async () => null),
      canvases: new Map([[createSurface.canvasId, canvas]]),
    });
    const reissued = handle(
      createSurface.surface.index,
      createSurface.surface.generation + 1
    );
    replayer.replay(frameOf({ ...createSurface, surface: reissued }, 29n));
    replayer.replay(
      frameOf({ ...destroySurface, surface: createSurface.surface }, 30n)
    );
    check(
      replayer.surfaces.get(reissued) === canvas.context &&
        replayer.surfaces.size === 1,
      `a DestroySurface naming a stale generation leaves the live surface alone (${replayer.surfaces.size} held)`
    );
  }
  {
    // **Two kinds, one set of handle bits, through the commands themselves.**
    // The buffer takes the surface's own handle, and neither table may see the
    // other — the opcode is the only thing that says which table an id indexes.
    const canvas = stubCanvas('shared-bits');
    const { replayer } = await readyForBuffers();
    replayer.replay(frameOf(deviceLocalBuffer, 31n));
    // A canvas the surface command can resolve, on the replayer that already
    // has a device: the registry is fixed at construction, so this drives the
    // surface against its own replayer and compares the tables' keys instead.
    const surfaced = new Replayer({
      gpu: stubGpu(async () => null),
      canvases: new Map([[createSurface.canvasId, canvas]]),
    });
    surfaced.replay(
      frameOf({ ...createSurface, surface: deviceLocalBuffer.buffer }, 32n)
    );
    check(
      replayer.buffers.get(deviceLocalBuffer.buffer) !== undefined &&
        replayer.surfaces.size === 0 &&
        surfaced.surfaces.get(deviceLocalBuffer.buffer) === canvas.context &&
        surfaced.buffers.size === 0,
      `a buffer handle indexes the buffer table and nothing else (${replayer.surfaces.size} surfaces beside the buffer, ${surfaced.buffers.size} buffers beside the surface)`
    );
  }

  // ---- where a buffer that cannot be created goes -------------------------
  //
  // There is no reply for `create_buffer` — identity is positional — so a
  // failure has one place to go: the queue `Device::take_error` drains, which
  // `crcbl_hal` documents as existing for WebGPU and which
  // `docs/plan/41-webgpu-stream.md` has `Gpu::acquire` reading every frame.
  // Every way of failing is driven here, because an error that goes nowhere is
  // the same bug as a dropped reply one seam over.
  {
    // **A usage flag WebGPU has no bit for.** `DEVICE_ADDRESS` requires
    // `Features::BUFFER_DEVICE_ADDRESS`, which this backend can never report —
    // WebGPU has no raw GPU pointers — so the creation is refused rather than
    // granted without it. Dropping the bit would hand back a buffer whose
    // address cannot be taken and move the failure to whatever dereferences it.
    const { replayer, device } = await readyForBuffers();
    replayer.replay(
      frameOf(
        { ...deviceLocalBuffer, usage: ['STORAGE', 'DEVICE_ADDRESS'] },
        33n
      )
    );
    const reason = replayer.takeError();
    check(
      device.created.length === 0 &&
        replayer.buffers.size === 0 &&
        String(reason).includes('DEVICE_ADDRESS'),
      `a usage flag with no GPUBufferUsage bit is refused and named (${device.created.length} created, ${JSON.stringify(reason)})`
    );
    check(
      replayer.takeError() === null,
      'and the queue is empty once that error has been taken'
    );
  }
  {
    // **A size no `GPUSize64` carries exactly.** The wire's size is a `u64` and
    // WebGPU's is a JavaScript number, so the fixture's `u64::MAX` would be
    // passed on rounded — a buffer of a size nobody asked for, created
    // successfully. This is the fixture's third descriptor replayed exactly as
    // it was written.
    const { replayer, device } = await readyForBuffers();
    replayer.replay(frameOf(hostReadbackBuffer, 34n));
    const reason = replayer.takeError();
    check(
      device.created.length === 0 &&
        replayer.buffers.size === 0 &&
        String(reason).includes(String(hostReadbackBuffer.size)),
      `a size past 2^53 is refused with the number written out (${device.created.length} created, ${JSON.stringify(reason)})`
    );
  }
  {
    // **A buffer command before any device opened.** `create_buffer` is a device
    // method, so this is an ordering bug on the far side, and recording it is
    // what makes it visible at the command that was too early rather than at
    // whatever draws with the handle later.
    const replayer = new Replayer({ gpu: stubGpu(async () => null) });
    replayer.replay(frameOf(deviceLocalBuffer, 35n));
    const reason = replayer.takeError();
    check(
      replayer.buffers.size === 0 &&
        String(reason).includes('before any device opened'),
      `a CreateBuffer with no device is refused and says so (${JSON.stringify(reason)})`
    );
  }
  {
    // **A `createBuffer` that throws.** Most WebGPU failures do not — they
    // arrive on the device's error channel, which is the check below — but an
    // allocation failure may, and a throw out of `replay` would abandon every
    // command after it in the frame.
    const device = stubDevice([], {
      refuseBuffers: new Error('out of memory'),
    });
    const { replayer } = await readyForBuffers({ device });
    let thrown = null;
    try {
      replayer.replay(frameOf(deviceLocalBuffer, 36n));
    } catch (error) {
      thrown = error;
    }
    const reason = replayer.takeError();
    check(
      thrown === null &&
        replayer.buffers.size === 0 &&
        String(reason).includes('out of memory'),
      `a createBuffer that throws is recorded rather than thrown on (${String(thrown)}, ${JSON.stringify(reason)})`
    );
  }
  {
    // **What the browser says after the fact.** An invalid usage combination or
    // a size over `maxBufferSize` is not raised in the call at all: WebGPU hands
    // back a `GPUBuffer` and reports the reason on the device. A replayer that
    // never listened would see nothing but success.
    const { replayer, device } = await readyForBuffers();
    replayer.replay(frameOf(deviceLocalBuffer, 37n));
    device.report('Buffer usage (MAP_READ|STORAGE) is invalid');
    const reason = replayer.takeError();
    check(
      String(reason).includes('MAP_READ|STORAGE'),
      `an uncapturederror on the device reaches the take_error queue (${JSON.stringify(reason)})`
    );
    check(
      replayer.takeError() === null,
      'and each error is reported once, which is what take_error promises'
    );
  }
  {
    // **The queue is bounded, and says when it dropped something.** Nothing
    // drains it yet — the `take_error` command is a later slice — and
    // `uncapturederror` can fire every frame for as long as a page is open. The
    // first errors are kept, because what went wrong first caused the rest; what
    // must not happen is the rest disappearing silently.
    const { replayer, device } = await readyForBuffers();
    const over = 3;
    for (let i = 0; i < MAX_PENDING_ERRORS + over; i += 1) {
      device.report(`error ${i}`);
    }
    check(
      replayer.pendingErrors === MAX_PENDING_ERRORS,
      `the queue stops at ${MAX_PENDING_ERRORS} (${replayer.pendingErrors} held)`
    );
    const drained = [];
    for (
      let error = replayer.takeError();
      error !== null;
      error = replayer.takeError()
    ) {
      drained.push(error);
    }
    check(
      drained.length === MAX_PENDING_ERRORS + 1 &&
        // The oldest are the ones kept: what went wrong first caused the rest.
        drained[0].endsWith('error 0') &&
        drained[MAX_PENDING_ERRORS - 1].endsWith(
          `error ${MAX_PENDING_ERRORS - 1}`
        ) &&
        drained[MAX_PENDING_ERRORS].includes(String(over)),
      `and the last thing out of it names the ${over} it refused (${JSON.stringify(drained[drained.length - 1])})`
    );
    check(
      replayer.takeError() === null,
      'after which it is empty rather than repeating the summary'
    );
  }

  // ---- the BufferUsage → GPUBufferUsage mapping, flag by flag -------------
  //
  // Spelled out here rather than compared against the table it is testing, for
  // the reason the feature mapping below is: a check that imported the mapping
  // would agree with whatever it says. Each flag on its own, so one wired to the
  // wrong bit names itself instead of hiding inside a union.
  {
    for (const [name, bit, webgpu] of [
      ['TRANSFER_SRC', GPU_BUFFER_USAGE.COPY_SRC, 'COPY_SRC'],
      ['TRANSFER_DST', GPU_BUFFER_USAGE.COPY_DST, 'COPY_DST'],
      ['UNIFORM', GPU_BUFFER_USAGE.UNIFORM, 'UNIFORM'],
      ['STORAGE', GPU_BUFFER_USAGE.STORAGE, 'STORAGE'],
      ['INDEX', GPU_BUFFER_USAGE.INDEX, 'INDEX'],
      ['INDIRECT', GPU_BUFFER_USAGE.INDIRECT, 'INDIRECT'],
      ['QUERY_RESOLVE', GPU_BUFFER_USAGE.QUERY_RESOLVE, 'QUERY_RESOLVE'],
    ]) {
      checkEqual(
        webgpuBufferUsageFor([name], 'DeviceLocal'),
        { bits: bit, unsatisfiable: [] },
        `BufferUsage::${name} maps to GPUBufferUsage.${webgpu} and to nothing else`
      );
    }
    // The one flag with nothing behind it. Named rather than dropped, and the
    // bits that *did* map are still reported — the caller refuses the whole
    // creation, so what matters is that the refusal can say which flag did it.
    checkEqual(
      webgpuBufferUsageFor(['STORAGE', 'DEVICE_ADDRESS'], 'DeviceLocal'),
      {
        bits: GPU_BUFFER_USAGE.STORAGE,
        unsatisfiable: ['BufferUsage::DEVICE_ADDRESS'],
      },
      'BufferUsage::DEVICE_ADDRESS has no GPUBufferUsage bit and comes back unsatisfiable'
    );
    for (const [memory, bits, why] of [
      [
        'DeviceLocal',
        0,
        'nothing, which is what lets the buffer be device-local',
      ],
      [
        'HostUpload',
        GPU_BUFFER_USAGE.COPY_DST,
        'COPY_DST, for queue.writeBuffer',
      ],
      ['HostReadback', GPU_BUFFER_USAGE.MAP_READ, 'MAP_READ, for mapAsync'],
    ]) {
      checkEqual(
        webgpuBufferUsageFor([], memory),
        { bits, unsatisfiable: [] },
        `MemoryLocation::${memory} adds ${why}`
      );
    }
    // A location this file does not know is a decoder that grew a variant, and
    // it must not be quietly treated as device-local — that would place memory
    // the seam placed deliberately somewhere else.
    checkEqual(
      webgpuBufferUsageFor([], 'HostSomethingElse'),
      { bits: 0, unsatisfiable: ['MemoryLocation::HostSomethingElse'] },
      'a memory location with no row is refused rather than read as DeviceLocal'
    );
  }

  // ---- the capability query ------------------------------------------------
  //
  // The third command with a reply and the only one answered inside the call:
  // WebGPU has no asynchronous capability query and almost nothing synchronous
  // either, so most of this record is what the replayer decided a browser can
  // honestly claim. Every field is therefore read back **out of the buffer** and
  // compared against a number spelled out here — a check that called the
  // translation again would agree with whatever it produced.
  //
  // AND ITS REFUSAL IS A REPLY. `Instance::surface_caps` is the only call that
  // says whether an adapter can present to a window, so its docs oblige a caller
  // doing selection to treat a failure as an ordinary step. A replayer that
  // threw would take the frame down over one.
  {
    const { replayer, gpu } = await readyForCaps();
    replayer.replay(frameOf(surfaceCaps, 11n));
    check(
      replayer.hasReplies && replayer.inFlight === 0,
      `the answer is queued inside the call, with nothing left in flight (queued ${replayer.hasReplies}, in flight ${replayer.inFlight})`
    );
    const answered = decodeOneReply(replayer.replies);
    check(
      answered?.tag === 'SurfaceCaps' && answered?.sequence === 11n,
      `the reply is a SurfaceCaps naming the command that asked (${answered?.tag} answering ${answered?.sequence})`
    );
    checkEqual(
      answered?.caps,
      {
        // The stub prefers `rgba8unorm` (0x02), so that is what comes first —
        // `formats` is best-first and the browser's preference is what "best"
        // means here. `bgra8unorm` (0x04) follows, because a canvas can be
        // configured with either.
        formats: [0x02, 0x04],
        // FIFO, and only FIFO: WebGPU has no present-mode concept, and a canvas
        // presents at the `requestAnimationFrame` boundary.
        presentModes: [0x00],
        // `GPUCanvasConfiguration.alphaMode` is `'opaque' | 'premultiplied'`,
        // and nothing else in WebGPU spells the other two.
        compositeAlpha: [0x00, 0x01],
        // One implicit ring, so a range of exactly one — the answer
        // `crcbl_hal::SurfaceCaps`'s own decision note prescribes for a canvas.
        minImageCount: 2,
        maxImageCount: 2,
        // **The field with no browser answer.** There is no `currentExtent`
        // query, and the canvas's own size is a number the page set — handing it
        // back as a cross-check on the shell's size would confirm nothing.
        currentExtent: null,
      },
      'every capability field is what the browser said or the documented absence'
    );
    check(
      gpu.formatCalls === 1,
      `and the format list came from asking the browser (${gpu.formatCalls} calls)`
    );
  }
  {
    // **The same replayer against a different browser.** The one field WebGPU
    // does answer has to move when the browser's answer moves; a list written
    // out as a constant cannot.
    const { replayer } = await readyForCaps({ canvasFormat: 'bgra8unorm' });
    replayer.replay(frameOf(surfaceCaps, 12n));
    const answered = decodeOneReply(replayer.replies);
    checkEqual(
      answered?.caps?.formats,
      [0x04, 0x02],
      'a browser preferring bgra8unorm is answered with bgra8unorm first'
    );
    checkEqual(
      answered?.caps?.presentModes,
      [0x00],
      'and Fifo is still the mode offered, which is the promise SurfaceCaps makes'
    );
  }
  {
    // **THE QUERY DEPENDS ON NEITHER TABLE, AND THIS IS WHERE THAT IS SHOWN.**
    // A bare replayer: nothing enumerated, no canvas registered, no surface
    // created, and nothing cleared out of the way. Answering it anyway is the
    // whole of what dropping the two ids from the command means — the surface
    // and the adapter `Instance::surface_caps` takes are an impl's to validate,
    // and this side has nothing to say about either.
    //
    // A replayer that kept its old lookups fails here rather than anywhere
    // else in this file, because every other block above has a surface and an
    // adapter in place.
    const gpu = stubGpu(async () => openingAdapter());
    const replayer = new Replayer({ gpu });
    let thrown = null;
    try {
      replayer.replay(frameOf(surfaceCaps, 13n));
    } catch (error) {
      thrown = error;
    }
    const answered = decodeOneReply(replayer.replies);
    check(
      thrown === null &&
        answered?.tag === 'SurfaceCaps' &&
        answered?.sequence === 13n,
      `a replayer with no surface and no adapter still answers the query (${String(thrown)}, ${answered?.tag} answering ${answered?.sequence})`
    );
    check(
      replayer.surfaces.size === 0 && gpu.calls === 0,
      `and it consulted neither table to do it (${replayer.surfaces.size} surfaces, ${gpu.calls} requestAdapter calls)`
    );
  }
  {
    // **A browser naming a canvas format this seam has no `Format` for.** The
    // one thing the replayer actually asks WebGPU is the one thing that can come
    // back unusable, and the honest answer is that the query failed — not a
    // format list with a plausible guess in it.
    const { replayer } = await readyForCaps({ canvasFormat: 'rgba32float' });
    let thrown = null;
    try {
      replayer.replay(frameOf(surfaceCaps, 15n));
    } catch (error) {
      thrown = error;
    }
    const answered = decodeOneReply(replayer.replies);
    check(
      thrown === null &&
        answered?.tag === 'SurfaceCapsFailed' &&
        // Spelled out rather than read off `SURFACE_CAPS_FAILURE`, for the
        // reason every expected value in this file is: a check that imported
        // the table would agree with whatever it says.
        answered?.cause === 0x00 &&
        String(answered?.reason).includes('rgba32float'),
      `an unknown canvas format is a Backend refusal naming it (${String(thrown)}, ${answered?.tag} cause ${answered?.cause}: ${answered?.reason})`
    );
    // …and the refusal is the *whole* of what is in the buffer. A record
    // abandoned part-way through and then answered again would leave a decodable
    // reply behind a stub of one, which every check above would still pass.
    check(
      replayer.replies.length === failureReplyBytes(answered?.reason),
      `and nothing else reached the buffer (${replayer.replies.length} bytes, ${failureReplyBytes(answered?.reason)} expected)`
    );
  }

  // ---- the WebGPU → seam mapping, field by field --------------------------
  //
  // NOT CIRCULAR, unlike the reply comparisons above, and deliberately kept
  // apart from them for that reason. Those build their expectation with the
  // same `halAdapterInfoFor` the replayer calls, which is fine for what they
  // assert — the choice of reply, its sequence, and when it arrives — and says
  // nothing about the mapping. Everything below spells the expected value out.
  {
    const bare = stubAdapter({});

    // The four flags core WebGPU grants with no feature name behind them:
    // COMPUTE (1<<8), OCCLUSION_QUERY (1<<7), DEPTH_BIAS_CLAMP (1<<14),
    // DEBUG_MARKERS (1<<18).
    const CORE = (1n << 8n) | (1n << 7n) | (1n << 14n) | (1n << 18n);
    checkEqual(
      halFeaturesFor(bare),
      CORE,
      'an adapter with no optional features still reports what core WebGPU grants'
    );

    // Each mapped name, one at a time, so a table row wired to the wrong bit
    // names itself instead of hiding inside a union.
    for (const [name, bit, flag] of [
      ['depth-clip-control', 1n << 13n, 'DEPTH_CLAMP'],
      ['texture-compression-bc', 1n << 16n, 'TEXTURE_COMPRESSION_BC'],
      ['timestamp-query', 1n << 5n, 'TIMESTAMP_QUERY'],
      ['indirect-first-instance', 1n << 4n, 'INDIRECT_FIRST_INSTANCE'],
    ]) {
      checkEqual(
        halFeaturesFor(stubAdapter({}, [name])),
        CORE | bit,
        `${name} maps to Features::${flag} and to nothing else`
      );
    }

    // **The other direction's loss.** Most `GPUFeatureName`s have no seam flag,
    // and a device with them has to report exactly what a device without them
    // does — otherwise a bit is being set by a name nobody chose.
    checkEqual(
      halFeaturesFor(
        stubAdapter({}, [
          'shader-f16',
          'float32-filterable',
          'float32-blendable',
          'dual-source-blending',
          'clip-distances',
          'subgroups',
          'texture-compression-etc2',
          'texture-compression-astc',
          'depth32float-stencil8',
          'rg11b10ufloat-renderable',
          'bgra8unorm-storage',
          'core-features-and-limits',
        ])
      ),
      CORE,
      'a WebGPU feature with no seam flag is dropped rather than setting one'
    );

    // The limits, against the distinct stub. Written out rather than looped
    // over the stub, so a mapping that read the wrong member has to disagree
    // with a number here.
    checkEqual(
      halLimitsFor(stubAdapter({})),
      {
        maxImage2d: 101,
        maxImage3d: 102,
        maxImageArrayLayers: 103,
        maxStorageBufferRange: 104n,
        maxUniformBufferRange: 105n,
        maxBindGroups: 106,
        // No bindless model and no push constants in WGSL, so the `0` that
        // `crcbl_hal::Limits` documents for each of their absences.
        maxBindlessDescriptors: 0,
        maxPushConstantSize: 0,
        maxColorAttachments: 107,
        // `sampleCount` is specified to be 1 or 4 and no limit reports it.
        maxSampleCount: 4,
        // Without a count buffer, one indirect call emits one draw.
        maxDrawIndirectCount: 1,
        maxComputeWorkgroupSize: [108, 109, 110],
        maxComputeInvocationsPerWorkgroup: 111,
        maxComputeWorkgroupsPerDimension: 112,
        minUniformBufferOffsetAlignment: 113n,
        minStorageBufferOffsetAlignment: 114n,
        // A spec constant — `bytesPerRow` must be a multiple of 256 — and not a
        // member of `GPUSupportedLimits` at all.
        optimalBufferCopyOffsetAlignment: 256n,
        // The floor, because WebGPU reports no anisotropy ceiling to promise.
        maxSamplerAnisotropy: 1,
        // No timestamp queries on this stub, so no tick period.
        timestampPeriodNs: 0,
      },
      'every GPUSupportedLimits member lands in the seam field that names it'
    );

    checkEqual(
      halLimitsFor(stubAdapter({}, ['timestamp-query'])).timestampPeriodNs,
      1,
      "a browser's timestamps are nanoseconds, so the period is 1 once they exist"
    );

    // **The same table read backwards**, which is what a device request needs.
    // Spelled out here too: the two directions share a table precisely so they
    // cannot disagree, and this is what says the sharing works rather than
    // that it happened.
    checkEqual(
      webgpuFeaturesFor(CORE),
      { names: [], unsatisfiable: 0n },
      'the core flags need no GPUFeatureName and ask for none'
    );
    for (const [name, bit] of [
      ['depth-clip-control', 1n << 13n],
      ['texture-compression-bc', 1n << 16n],
      ['timestamp-query', 1n << 5n],
      ['indirect-first-instance', 1n << 4n],
    ]) {
      checkEqual(
        webgpuFeaturesFor(bit),
        { names: [name], unsatisfiable: 0n },
        `${name} is what that bit asks WebGPU for, and nothing else`
      );
      // …and the round trip: what the name maps to is the bit it came from.
      checkEqual(
        halFeaturesFor(stubAdapter({}, [name])) & ~CORE,
        bit,
        `${name} survives both directions of the mapping`
      );
    }
    // Nineteen of the seam's twenty-seven flags have nothing behind them in
    // WebGPU. Each comes back as unsatisfiable rather than as silence, which is
    // what lets a *required* one fail the request.
    const unmappable =
      (1n << 9n) | (1n << 0n) | (1n << 22n) | (1n << 24n) | (1n << 12n);
    checkEqual(
      webgpuFeaturesFor(unmappable),
      { names: [], unsatisfiable: unmappable },
      'TIMELINE_SEMAPHORE, DESCRIPTOR_INDEXING, MESH_SHADER, RAY_QUERY and PUSH_CONSTANTS have no WebGPU name'
    );
    checkEqual(
      webgpuFeaturesFor(CORE | (1n << 5n) | (1n << 9n)),
      { names: ['timestamp-query'], unsatisfiable: 1n << 9n },
      'a mixed word is split into what WebGPU can be asked for and what it cannot'
    );

    // **The fields WebGPU cannot supply.** Each is the value that means absent
    // rather than one that looks real — a fabricated vendor id would be
    // indistinguishable downstream from a true one.
    const info = halAdapterInfoFor(
      stubAdapter({ vendor: 'crcbl', architecture: 'stub-1', device: 'gpu' })
    );
    check(
      info.vendorId === 0 &&
        info.deviceId === 0 &&
        info.deviceType === DEVICE_TYPE.OTHER &&
        info.driver === '' &&
        info.id === 0,
      'the fields WebGPU has no answer for are the documented absences' +
        ` (vendor ${info.vendorId}, device ${info.deviceId},` +
        ` type ${info.deviceType}, driver ${JSON.stringify(info.driver)})`
    );
    check(
      info.name === 'crcbl stub-1 gpu',
      `the name is every GPUAdapterInfo string the browser filled (${JSON.stringify(info.name)})`
    );

    // A `GPUAdapter` whose limits are not numbers is a browser lying to us, and
    // it has to be named rather than arriving as `BigInt(undefined)` from three
    // frames away — or, worse, as a zero.
    let thrown = null;
    try {
      halLimitsFor({ info: {}, features: new Set(), limits: {} });
    } catch (error) {
      thrown = error;
    }
    check(
      thrown instanceof TypeError &&
        thrown.message.includes('maxStorageBufferBindingSize'),
      `an adapter with no limits names the member it was missing (${String(thrown)})`
    );
  }

  // ---- a reply that will not encode leaves no bytes behind ----------------
  // The failure this guards is specific and silent: `#adapter` writes a record
  // field by field, its caller catches a throw and answers `NoAdapter` instead,
  // and without a rollback the buffer would hold half an adapter followed by a
  // refusal. wasm refuses that whole frame's replies as undecodable, which
  // strands every other answer in it.
  {
    const replies = new ReplyWriter();
    replies.readbackPending(1n, handle(1, 1));
    const good = replies.bytes.slice();
    let thrown = null;
    try {
      // A record with a missing field, which `putAdapterInfo` refuses part-way
      // through — after the tag and the sequence have been written.
      replies.adapter(2n, {
        id: 0,
        name: 'half',
        vendorId: 0,
        deviceId: 0,
        deviceType: DEVICE_TYPE.OTHER,
        driver: '',
        features: 0n,
        limits: {},
      });
    } catch (error) {
      thrown = error;
    }
    check(
      thrown !== null,
      `the half-written reply was refused (${String(thrown)})`
    );
    checkEqual(
      replies.bytes,
      good,
      'and the buffer is byte for byte what it was before the refused call'
    );
    // …and the writer still works afterwards, which is what the caller does
    // next: it answers the same sequence with a refusal instead.
    replies.noAdapter(2n, 'could not encode this adapter');
    check(
      replies.bytes.length > good.length,
      'a reply written after a refused one still lands'
    );
  }

  // ---- a frame with nothing in it is not an error -------------------------
  // `takeCommandStream` answers `null` when no channel is installed, and an
  // empty `commands` array when a frame encoded nothing. Both are ordinary.
  {
    const replayer = new Replayer({ gpu: stubGpu(async () => null) });
    let thrown = null;
    try {
      replayer.replay(null);
      replayer.replay({ baseSequence: 0n, commands: [] });
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null && !replayer.hasReplies,
      `no channel and an empty frame are both nothing to do (${String(thrown)})`
    );
  }

  if (failures.length > 0) {
    console.error(`\ngpu-replay: FAILED (${failures.length})`);
    process.exit(1);
  }
  console.log('\ngpu-replay: OK');
}

await main();
