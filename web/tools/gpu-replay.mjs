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
// THE IMAGE PAIR AND THE VIEW PAIR ANSWER NOTHING EITHER, and they are where
// the translations get interesting: `ImageDesc`'s five fields become a
// `GPUTextureDescriptor`'s seven, three of them through tables that lose
// something, and `ImageViewDesc`'s range becomes members WebGPU wants *absent*
// rather than numbered. Each table is driven row by row below against values
// written out here, and each refusal — a usage flag with no bit, a format this
// device did not enable, an aspect combination WebGPU has no word for, a
// dimensionality no table claims — is driven through a command and read back out
// of the `take_error` queue.
//
// AND A VIEW NAMES AN IMAGE, which is the one lookup any of these commands
// makes: a `GPUTextureView` comes from the texture and not from the device, so a
// `CreateImageView` whose image handle resolves to nothing has nothing to call.
// All three ways it can — never created, destroyed, a generation the slot has
// moved past — are driven here, and every one of them has to reach the error
// queue **without throwing**: a throw would abandon the rest of the frame over a
// far side that got its ordering wrong.
//
// AND THE TABLE UNDER BOTH PAIRS IS THE SAME ONE, which is what the generation
// checks here are about. A handle is `{ index, generation }` so that a stale one
// is detectable; the checks below produce a stale one deliberately — an index
// reissued at a higher generation — and insist that a destroy naming it releases
// nothing.
//
// AND THE SAMPLER PAIR ANSWERS NOTHING EITHER, and is the one where what is
// checked here is nearly all there is: a `GPUSampler` reports its `label` and
// nothing else, so no browser can be asked afterwards whether its filters,
// address modes or clamps are the ones the seam sent. What this file checks is
// the `GPUSamplerDescriptor` the device was *handed* — which is where the three
// decisions live that a `GPUSamplerDescriptor` forces and no other descriptor
// here does: an address mode WebGPU has no word for, a "no limit" sentinel whose
// WebGPU spelling is an explicit number rather than an absent member — the exact
// opposite of the view's range, one command earlier — and a float narrowed to a
// `GPUSize32` whose validity depends on the filters beside it. Each of the three
// is driven case by case below, and each refusal is read back out of the
// `take_error` queue.
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
  webgpuAddressModesFor,
  webgpuBindingLayoutFor,
  webgpuBufferUsageFor,
  webgpuFeaturesFor,
  webgpuMaxAnisotropyFor,
  webgpuShaderStageFor,
  webgpuTextureAspectFor,
  webgpuTextureFormatFor,
  webgpuTextureUsageFor,
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

/**
 * The `GPUTextureUsage` bits, from the WebGPU specification.
 *
 * Written out for {@link GPU_BUFFER_USAGE}'s reason, and held against a real
 * browser's own namespace object by `browser-e2e.mjs` for the same one.
 */
const GPU_TEXTURE_USAGE = Object.freeze({
  COPY_SRC: 0x01,
  COPY_DST: 0x02,
  TEXTURE_BINDING: 0x04,
  STORAGE_BINDING: 0x08,
  RENDER_ATTACHMENT: 0x10,
});

/**
 * `ImageSubresourceRange::ALL`, as it arrives off the wire.
 *
 * Written out rather than imported for every other expectation's reason, and
 * this one earns it twice over: what the checks below assert is that this number
 * **does not reach a browser**, so a constant taken from the translation that is
 * meant to remove it would be exactly the wrong source.
 */
const SUBRESOURCE_ALL = 0xffff_ffff;

/**
 * `f32::MAX`, which is `SamplerDesc::default`'s `lod_max` — "no limit".
 *
 * Written out for {@link SUBRESOURCE_ALL}'s reason and earning it the same way
 * twice over, in the opposite direction: what the checks below assert is that
 * this number **does** reach the browser, as an explicit `lodMaxClamp` rather
 * than as an omitted member, so a constant taken from the file that decides that
 * would be exactly the wrong source.
 */
const LOD_NO_LIMIT = 3.4028234663852886e38;

/**
 * `BindingResource::WHOLE_BUFFER`, which is `u64::MAX` on the wire.
 *
 * Written out for {@link SUBRESOURCE_ALL}'s reason, and earning it the same way:
 * what the checks below assert is that this number **does not reach a browser** —
 * it resolves to an *absent* `GPUBufferBinding.size` — so a constant taken from
 * the translation that removes it would be the wrong source. A `BigInt` because
 * it is a `u64`, which is what carries it verbatim.
 */
const BUFFER_WHOLE = 0xffff_ffff_ffff_ffffn;

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

/**
 * The fixture's three `CreateImage`s, with the one usage this backend refuses
 * taken off the third.
 *
 * **The third carries `ImageUsage::all()`**, which includes `PRESENT` — a flag
 * WebGPU has no bit for, so as written it is a refusal rather than a creation
 * and is driven as one below. Two of its views are the only ones exercising a
 * `D1` view and a stencil-only aspect, though, so those need it to exist: this
 * narrows the usage to the two flags a `D1` LUT would really carry and changes
 * nothing else. Every other field is still the command the Rust encoder wrote.
 *
 * @param {object[]} images
 */
function creatableImages(images) {
  return images.map((image, at) =>
    at === 2 ? { ...image, usage: ['TRANSFER_DST', 'SAMPLED'] } : image
  );
}

/**
 * A replayer holding all three of the fixture's images, so that its views have
 * something to name.
 *
 * @param {object} [options]
 * @param {object} [options.device] What `requestDevice` resolves to.
 * @param {object[]} images The fixture's `CreateImage` commands.
 */
async function readyWithImages(images, { device = stubDevice() } = {}) {
  const { replayer } = await readyWithDevice({ device });
  const made = creatableImages(images);
  for (const [at, image] of made.entries()) {
    replayer.replay(frameOf(image, BigInt(100 + at)));
  }
  return { replayer, device, images: made };
}

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
 * A `GPUTextureView` as `createView` answers one.
 *
 * `label` is read off the descriptor for {@link stubBuffer}'s reason. `of` is
 * **not** a `GPUTextureView` member and could not be: WebGPU gives a view no way
 * back to its texture at all, which is why "a view of *that* image" is a claim
 * only a stub can check and why the browser gate checks something else instead.
 *
 * @param {{ label?: string }} desc
 * @param {object} texture The `GPUTexture` whose `createView` made it.
 */
function stubTextureView(desc, texture) {
  return { label: desc.label ?? '', of: texture };
}

/**
 * A `GPUTexture` as `createTexture` answers one.
 *
 * The members a real one reports back are read off the descriptor rather than
 * stored beside it, for {@link stubBuffer}'s reason — and there are nine of them
 * rather than three, which is what makes the image checks below able to compare
 * what came back against what was asked for field by field.
 *
 * `views` is the descriptor list `createView` was handed, in order. It is the
 * only place the sentinel resolution is visible: an absent `mipLevelCount` and a
 * present one are two different objects here, and `deepStrictEqual` can tell.
 *
 * @param {{ label?: string, dimension: string, format: string,
 *           mipLevelCount: number, sampleCount: number, usage: number,
 *           size: { width: number, height: number,
 *                   depthOrArrayLayers: number } }} desc
 */
function stubTexture(desc) {
  const texture = {
    label: desc.label ?? '',
    width: desc.size.width,
    height: desc.size.height,
    depthOrArrayLayers: desc.size.depthOrArrayLayers,
    mipLevelCount: desc.mipLevelCount,
    sampleCount: desc.sampleCount,
    dimension: desc.dimension,
    format: desc.format,
    usage: desc.usage,
    /** How many times the replayer destroyed it. */
    destroys: 0,
    /** @type {object[]} Every `GPUTextureViewDescriptor` it was handed. */
    views: [],
    destroy() {
      texture.destroys += 1;
    },
    /** @param {{ label?: string }} viewDesc */
    createView(viewDesc) {
      texture.views.push(viewDesc);
      return stubTextureView(viewDesc, texture);
    },
  };
  return texture;
}

/**
 * A `GPUSampler` as `createSampler` answers one.
 *
 * **Two members, and one of them is not a `GPUSampler`'s.** A real one reports
 * its `label` and nothing else — no filters, no address modes, no clamps — which
 * is why every claim about a sampler's descriptor here is made against
 * `device.createdSamplers` rather than against what came back. `of` is the
 * descriptor it was made from, kept for the checks that need to say *which*
 * sampler a table holds, exactly as {@link stubTextureView}'s is and with the
 * same caveat: no browser offers it.
 *
 * @param {{ label?: string }} desc
 */
function stubSampler(desc) {
  return { label: desc.label ?? '', of: desc };
}

/**
 * A `GPUBindGroupLayout` as `createBindGroupLayout` answers one.
 *
 * {@link stubSampler}'s shape and for its reason: a real one reports its `label`
 * and nothing else — not its entries, not their bindings, not their visibility —
 * so every claim about a layout's descriptor below is made against
 * `device.createdLayouts` rather than against what came back. `of` is the
 * descriptor it was made from, kept for the checks that need to say *which*
 * layout a table holds, with the same caveat: no browser offers it.
 *
 * @param {{ label?: string }} desc
 */
function stubBindGroupLayout(desc) {
  return { label: desc.label ?? '', of: desc };
}

/**
 * A `GPUBindGroup` as `createBindGroup` answers one.
 *
 * {@link stubBindGroupLayout}'s shape and for its reason: a real one reports its
 * `label` and nothing else — not its layout, not its entries — so every claim
 * about a group's descriptor below is made against `device.createdBindGroups`
 * rather than against what came back. `of` is the descriptor it was made from,
 * kept for the checks that need to say *which* group a table holds, with the same
 * caveat: no browser offers it.
 *
 * @param {{ label?: string }} desc
 */
function stubBindGroup(desc) {
  return { label: desc.label ?? '', of: desc };
}

/**
 * A `GPUPipelineLayout` as `createPipelineLayout` answers one.
 *
 * {@link stubBindGroupLayout}'s shape and for its reason: a real one reports its
 * `label` and nothing else — not its bind-group layouts, not its push-constant
 * ranges (WebGPU has none) — so every claim about a pipeline layout's descriptor
 * below is made against `device.createdPipelineLayouts` rather than against what
 * came back. `of` is the descriptor it was made from.
 *
 * @param {{ label?: string }} desc
 */
function stubPipelineLayout(desc) {
  return { label: desc.label ?? '', of: desc };
}

/**
 * A `GPUComputePipeline` as `createComputePipeline` answers one.
 *
 * {@link stubPipelineLayout}'s shape, with one addition: a real
 * `GPUComputePipeline` answers `getBindGroupLayout(n)` — the derived layout a
 * genuinely-built pipeline can hand back, which no other object here has — so the
 * stub carries it too, and the browser gate is what actually calls it. `of` is
 * the `GPUComputePipelineDescriptor` it was made from, kept for the checks that
 * need to say *what* the pipeline was built with — including the one that matters
 * most: that it carries no workgroup-size member at all.
 *
 * @param {{ label?: string }} desc
 */
function stubComputePipeline(desc) {
  return {
    label: desc.label ?? '',
    of: desc,
    getBindGroupLayout: (index) => ({ label: `derived ${index}` }),
  };
}

/**
 * A `GPURenderPipeline` as `createRenderPipeline` answers one.
 *
 * {@link stubComputePipeline}'s shape for the same reason: a real one reports its
 * `label` and answers `getBindGroupLayout(n)`, the derived layout only a
 * genuinely-built pipeline can hand back. `of` is the `GPURenderPipelineDescriptor`
 * it was made from, kept for the checks that need to say *what* the pipeline was
 * built with — the empty `vertex.buffers`, the omitted or present `fragment`, the
 * depth-stencil with no `stencilReference`.
 *
 * @param {{ label?: string }} desc
 */
function stubRenderPipeline(desc) {
  return {
    label: desc.label ?? '',
    of: desc,
    getBindGroupLayout: (index) => ({ label: `derived ${index}` }),
  };
}

/**
 * A `GPUShaderModule` as `createShaderModule` answers one.
 *
 * {@link stubSampler}'s shape and for its reason: a real one reports its `label`,
 * so every claim about *what was compiled* is made against
 * `device.createdShaderModules` rather than against what came back. `of` is the
 * descriptor it was made from. `getCompilationInfo` is here because a
 * `GPUShaderModule` really has one and the browser gate reads it — the stub
 * answers "no messages", which is what a clean compile reports; this file's job
 * is the descriptor the replayer builds, and the browser's is whether the code
 * compiles.
 *
 * @param {{ label?: string }} desc
 */
function stubShaderModule(desc) {
  return {
    label: desc.label ?? '',
    of: desc,
    getCompilationInfo: async () => ({ messages: [] }),
  };
}

/**
 * A `GPURenderPassEncoder` as `beginRenderPass` answers one, recording every
 * `setPipeline`, `setBindGroup` and `draw` it is handed.
 *
 * The draw arms have no reply and no browser object to read back — a bound
 * pipeline or a draw leaves nothing on the device's error channel when it
 * succeeds — so what a check reads is the call the replayer made: `setPipelines`
 * is the pipelines passed to `setPipeline`, `setBindGroups` the
 * `{ slot, group, dynamicOffsets }` triples, and `draws` the four counts the
 * replayer computed from a `Draw`'s two ranges. `ended` counts `end` so a pass
 * left open is visible.
 */
function stubRenderPass() {
  const pass = {
    /** @type {object[]} The pipelines `setPipeline` was handed, in order. */
    setPipelines: [],
    /**
     * @type {Array<{ slot: number, group: object, dynamicOffsets: number[] }>}
     *   Every `setBindGroup`, in order.
     */
    setBindGroups: [],
    /**
     * @type {Array<{ vertexCount: number, instanceCount: number,
     *   firstVertex: number, firstInstance: number }>} Every `draw`, in order.
     */
    draws: [],
    ended: 0,
    /** @param {object} pipeline */
    setPipeline(pipeline) {
      pass.setPipelines.push(pipeline);
    },
    /**
     * @param {number} slot
     * @param {object} group
     * @param {number[]} dynamicOffsets
     */
    setBindGroup(slot, group, dynamicOffsets) {
      pass.setBindGroups.push({ slot, group, dynamicOffsets });
    },
    /**
     * @param {number} vertexCount
     * @param {number} instanceCount
     * @param {number} firstVertex
     * @param {number} firstInstance
     */
    draw(vertexCount, instanceCount, firstVertex, firstInstance) {
      pass.draws.push({
        vertexCount,
        instanceCount,
        firstVertex,
        firstInstance,
      });
    },
    end() {
      pass.ended += 1;
    },
  };
  return pass;
}

/**
 * A `GPUComputePassEncoder` as `beginComputePass` answers one, recording every
 * `setPipeline`, `setBindGroup` and `dispatchWorkgroups` it is handed.
 *
 * {@link stubRenderPass}'s compute twin and for its reason: the dispatch arms
 * have no reply and no browser object to read back, so what a check reads is the
 * call the replayer made. `setPipelines` is the pipelines passed to
 * `setPipeline`, `setBindGroups` the `{ slot, group, dynamicOffsets }` triples
 * (proving the generalized bind arm reaches a compute pass), and `dispatches`
 * the `{ x, y, z }` workgroup counts. `ended` counts `end` so a pass left open is
 * visible.
 */
function stubComputePass() {
  const pass = {
    /** @type {object[]} The pipelines `setPipeline` was handed, in order. */
    setPipelines: [],
    /**
     * @type {Array<{ slot: number, group: object, dynamicOffsets: number[] }>}
     *   Every `setBindGroup`, in order.
     */
    setBindGroups: [],
    /**
     * @type {Array<{ x: number, y: number, z: number }>} Every
     *   `dispatchWorkgroups`, in order.
     */
    dispatches: [],
    ended: 0,
    /** @param {object} pipeline */
    setPipeline(pipeline) {
      pass.setPipelines.push(pipeline);
    },
    /**
     * @param {number} slot
     * @param {object} group
     * @param {number[]} dynamicOffsets
     */
    setBindGroup(slot, group, dynamicOffsets) {
      pass.setBindGroups.push({ slot, group, dynamicOffsets });
    },
    /**
     * @param {number} x
     * @param {number} y
     * @param {number} z
     */
    dispatchWorkgroups(x, y, z) {
      pass.dispatches.push({ x, y, z });
    },
    end() {
      pass.ended += 1;
    },
  };
  return pass;
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
 * @param {unknown} [options.refuseTextures] The same for `createTexture`.
 * @param {unknown} [options.refuseSamplers] The same for `createSampler`.
 * @param {unknown} [options.refuseLayouts] The same for
 *   `createBindGroupLayout`.
 * @param {unknown} [options.refuseGroups] The same for `createBindGroup`.
 * @param {unknown} [options.refuseShaderModules] The same for
 *   `createShaderModule` — which a real WebGPU device does not do for bad WGSL,
 *   but may for an out-of-memory or a device already lost.
 * @param {unknown} [options.refusePipelineLayouts] The same for
 *   `createPipelineLayout`.
 * @param {unknown} [options.refuseComputePipelines] The same for
 *   `createComputePipeline` — which a real WebGPU device does not do for a bad
 *   entry point (that surfaces asynchronously through `uncapturederror`), but may
 *   for an out-of-memory or a device already lost.
 */
function stubDevice(
  features = [],
  {
    refuseBuffers,
    refuseTextures,
    refuseSamplers,
    refuseLayouts,
    refuseGroups,
    refuseShaderModules,
    refusePipelineLayouts,
    refuseComputePipelines,
    refuseRenderPipelines,
  } = {}
) {
  const device = {
    features: new Set(features),
    limits: deviceLimits(),
    /** @type {object[]} Every `GPUBufferDescriptor` it was handed, in order. */
    created: [],
    /** @type {object[]} Every `GPUTextureDescriptor` it was handed, in order. */
    createdTextures: [],
    /** @type {object[]} Every `GPUSamplerDescriptor` it was handed, in order. */
    createdSamplers: [],
    /**
     * @type {object[]} Every `GPUBindGroupLayoutDescriptor` it was handed, in
     *   order.
     */
    createdLayouts: [],
    /** @type {object[]} Every `GPUBindGroupDescriptor` it was handed, in order. */
    createdGroups: [],
    /**
     * @type {object[]} Every `GPUShaderModuleDescriptor` it was handed, in
     *   order.
     */
    createdShaderModules: [],
    /**
     * @type {object[]} Every `GPUPipelineLayoutDescriptor` it was handed, in
     *   order.
     */
    createdPipelineLayouts: [],
    /**
     * @type {object[]} Every `GPUComputePipelineDescriptor` it was handed, in
     *   order.
     */
    createdComputePipelines: [],
    /**
     * @type {object[]} Every `GPURenderPipelineDescriptor` it was handed, in
     *   order.
     */
    createdRenderPipelines: [],
    /**
     * @type {object[]} Every command encoder `createCommandEncoder` opened,
     *   newest last — each carrying the render pass it last began at `.pass`, so
     *   a draw scenario can read back the calls the replayer made on it.
     */
    createdEncoders: [],
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
    /** @param {object} desc */
    createTexture(desc) {
      device.createdTextures.push(desc);
      if (refuseTextures !== undefined) throw refuseTextures;
      return stubTexture(desc);
    },
    /** @param {object} desc */
    createSampler(desc) {
      device.createdSamplers.push(desc);
      if (refuseSamplers !== undefined) throw refuseSamplers;
      return stubSampler(desc);
    },
    /** @param {object} desc */
    createBindGroupLayout(desc) {
      device.createdLayouts.push(desc);
      if (refuseLayouts !== undefined) throw refuseLayouts;
      return stubBindGroupLayout(desc);
    },
    /** @param {object} desc */
    createBindGroup(desc) {
      device.createdGroups.push(desc);
      if (refuseGroups !== undefined) throw refuseGroups;
      return stubBindGroup(desc);
    },
    /** @param {object} desc */
    createShaderModule(desc) {
      device.createdShaderModules.push(desc);
      if (refuseShaderModules !== undefined) throw refuseShaderModules;
      return stubShaderModule(desc);
    },
    /** @param {object} desc */
    createPipelineLayout(desc) {
      device.createdPipelineLayouts.push(desc);
      if (refusePipelineLayouts !== undefined) throw refusePipelineLayouts;
      return stubPipelineLayout(desc);
    },
    /** @param {object} desc */
    createComputePipeline(desc) {
      device.createdComputePipelines.push(desc);
      if (refuseComputePipelines !== undefined) throw refuseComputePipelines;
      return stubComputePipeline(desc);
    },
    /** @param {object} desc */
    createRenderPipeline(desc) {
      device.createdRenderPipelines.push(desc);
      if (refuseRenderPipelines !== undefined) throw refuseRenderPipelines;
      return stubRenderPipeline(desc);
    },
    /**
     * A `GPUCommandEncoder` whose `beginRenderPass` hands back a
     * {@link stubRenderPass} and keeps it at `.pass`, whose `beginComputePass`
     * hands back a {@link stubComputePass} at `.computePass`, and whose
     * `copyBufferToBuffer` records the call at `.bufferCopies` — each for a check
     * to read.
     *
     * @param {{ label?: string }} [desc]
     */
    createCommandEncoder(desc) {
      const encoder = {
        label: desc?.label ?? '',
        /** @type {object[]} Every `GPURenderPassDescriptor`, in order. */
        beganPasses: [],
        /** @type {object|null} The render pass it last began. */
        pass: null,
        /** @type {object[]} Every `GPUComputePassDescriptor`, in order. */
        beganComputePasses: [],
        /** @type {object|null} The compute pass it last began. */
        computePass: null,
        /**
         * @type {Array<{ src: object, srcOffset: number, dst: object,
         *   dstOffset: number, size: number }>} Every `copyBufferToBuffer`.
         */
        bufferCopies: [],
        /**
         * @type {Array<{ source: object, destination: object, size: object }>}
         *   Every `copyBufferToTexture`.
         */
        bufferToImageCopies: [],
        /**
         * @type {Array<{ source: object, destination: object, size: object }>}
         *   Every `copyTextureToTexture`.
         */
        imageCopies: [],
        /**
         * @type {Array<{ buffer: object, offset: number, size: number }>} Every
         *   `clearBuffer`.
         */
        bufferFills: [],
        /** @param {object} passDesc */
        beginRenderPass(passDesc) {
          encoder.beganPasses.push(passDesc);
          encoder.pass = stubRenderPass();
          return encoder.pass;
        },
        /** @param {object} passDesc */
        beginComputePass(passDesc) {
          encoder.beganComputePasses.push(passDesc);
          encoder.computePass = stubComputePass();
          return encoder.computePass;
        },
        /**
         * @param {object} src
         * @param {number} srcOffset
         * @param {object} dst
         * @param {number} dstOffset
         * @param {number} size
         */
        copyBufferToBuffer(src, srcOffset, dst, dstOffset, size) {
          encoder.bufferCopies.push({ src, srcOffset, dst, dstOffset, size });
        },
        /**
         * @param {object} source A `GPUImageCopyBuffer`.
         * @param {object} destination A `GPUImageCopyTexture`.
         * @param {object} size A `GPUExtent3D`.
         */
        copyBufferToTexture(source, destination, size) {
          encoder.bufferToImageCopies.push({ source, destination, size });
        },
        /**
         * @param {object} source A `GPUImageCopyTexture`.
         * @param {object} destination A `GPUImageCopyTexture`.
         * @param {object} size A `GPUExtent3D`.
         */
        copyTextureToTexture(source, destination, size) {
          encoder.imageCopies.push({ source, destination, size });
        },
        /**
         * @param {object} buffer
         * @param {number} offset
         * @param {number} size
         */
        clearBuffer(buffer, offset, size) {
          encoder.bufferFills.push({ buffer, offset, size });
        },
      };
      device.createdEncoders.push(encoder);
      return encoder;
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
 * A replayer with a device open, which is what every creation below needs.
 *
 * The enumeration and the device request are the fixture's own, and their
 * replies are cleared in between so that what a check then reads out of the
 * buffer is its own command's doing. `create_buffer`, `create_image` and
 * `create_image_view` are all *device* methods, so this two-step is the seam's
 * shape rather than scaffolding: a creation arriving before the device has
 * opened is a real case and each has its own check below.
 *
 * @param {object} [options]
 * @param {object} [options.device] What `requestDevice` resolves to.
 */
async function readyWithDevice({ device = stubDevice() } = {}) {
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

  // The image commands, from the fixture for the same reason again — and here
  // the corpus buys the most of any command family: three images covering every
  // `ImageType`, six views covering every `ImageViewType`, both label cases, all
  // three aspect bits and the `ImageSubresourceRange::ALL` sentinel in each of
  // the two counts that can carry it. Every one of those is a row of a table
  // below, replayed as the Rust encoder wrote it.
  const images = commands.filter((command) => command.name === 'CreateImage');
  const views = commands.filter(
    (command) => command.name === 'CreateImageView'
  );
  const destroyImage = commands.find(
    (command) => command.name === 'DestroyImage'
  );
  const destroyImageView = commands.find(
    (command) => command.name === 'DestroyImageView'
  );
  check(
    images.length === 3 &&
      views.length === 6 &&
      destroyImage !== undefined &&
      destroyImageView !== undefined,
    `the committed stream carries three CreateImages, six CreateImageViews and a destroy for each (${images.length} images, ${views.length} views)`
  );
  // The sampler pair, from the fixture for the same reason again. The corpus
  // carries four, which is what the descriptor's shape costs: `mag`, `min` and
  // `mip` are three `FilterMode`s in a row with only two variants between them,
  // so no single command can make the trio distinct and three of these each put
  // the single `Linear` in a different slot. Between them they also spell every
  // `SamplerAddressMode` — including the one WebGPU cannot express — both
  // comparison cases and the two anisotropy values this backend has a rule for.
  const samplers = commands.filter(
    (command) => command.name === 'CreateSampler'
  );
  const destroySampler = commands.find(
    (command) => command.name === 'DestroySampler'
  );
  check(
    samplers.length === 4 && destroySampler !== undefined,
    `the committed stream carries four CreateSamplers and a DestroySampler (${samplers.length} creates)`
  );
  const [shadowSampler, borderSampler, coarseSampler, anisoSampler] = samplers;

  // The bind-group-layout pair, from the fixture for the same reason again — and
  // here the corpus buys the whole of the refusal set as well as the creation:
  // one layout holding every `BindingKind` WebGPU can express, one carrying the
  // portable bindless declaration, one with no entries at all, one with a fixed
  // array count, one visible to the two stages WebGPU has no bit for, and one
  // whose kind needs a texture format the seam does not carry. Every one of
  // those is a branch below, replayed as the Rust encoder wrote it.
  const layouts = commands.filter(
    (command) => command.name === 'CreateBindGroupLayout'
  );
  const destroyLayout = commands.find(
    (command) => command.name === 'DestroyBindGroupLayout'
  );
  check(
    layouts.length === 6 && destroyLayout !== undefined,
    `the committed stream carries six CreateBindGroupLayouts and a DestroyBindGroupLayout (${layouts.length} creates)`
  );
  const [
    frameLayout,
    bindlessLayout,
    emptyLayout,
    arrayLayout,
    meshLayout,
    storageImageLayout,
  ] = layouts;

  // The bind-group pair, from the fixture for the same reason again. The corpus
  // carries two: one with a `variable_count` of `None` whose four entries use all
  // three `BindingResource` shapes and carry the `WHOLE_BUFFER` sentinel, and one
  // with `variable_count: Some(2)` and a non-zero `array_index` — the two things
  // WebGPU cannot express, driven as refusals below.
  const groups = commands.filter(
    (command) => command.name === 'CreateBindGroup'
  );
  const destroyGroup = commands.find(
    (command) => command.name === 'DestroyBindGroup'
  );
  check(
    groups.length === 2 && destroyGroup !== undefined,
    `the committed stream carries two CreateBindGroups and a DestroyBindGroup (${groups.length} creates)`
  );
  const [materialGroup, bindlessGroup] = groups;

  // The shader-module pair, from the fixture for the same reason again — and here
  // the corpus buys all four absence conventions across three modules: one with
  // every artifact non-trivial, one whose `wgsl` is `Some("")` (a valid empty
  // module this backend builds), and one whose `wgsl` is `None` (a module this
  // backend refuses, because a WebGPU device compiles only WGSL). Each is a
  // branch below, replayed as the Rust encoder wrote it.
  const shaders = commands.filter(
    (command) => command.name === 'CreateShaderModule'
  );
  const destroyShader = commands.find(
    (command) => command.name === 'DestroyShaderModule'
  );
  check(
    shaders.length === 3 && destroyShader !== undefined,
    `the committed stream carries three CreateShaderModules and a DestroyShaderModule (${shaders.length} creates)`
  );
  const [fullShader, emptyWgslShader, noWgslShader] = shaders;

  // The pipeline-layout pair, from the fixture for the same reason again — the
  // last thing a pipeline is built from. The corpus carries two: one whose set
  // list holds two bind-group layouts (which pins set order) with no push
  // constants, and one whose `push_constants` is `Some` — the case WebGPU cannot
  // express at all, driven as a refusal below.
  const pipelineLayouts = commands.filter(
    (command) => command.name === 'CreatePipelineLayout'
  );
  const destroyPipelineLayout = commands.find(
    (command) => command.name === 'DestroyPipelineLayout'
  );
  check(
    pipelineLayouts.length === 2 && destroyPipelineLayout !== undefined,
    `the committed stream carries two CreatePipelineLayouts and a DestroyPipelineLayout (${pipelineLayouts.length} creates)`
  );
  const [twoSetLayout, pushConstantLayout] = pipelineLayouts;

  // The compute-pipeline pair, from the fixture for the same reason again — the
  // first command that resolves handles into two *different* tables. The corpus
  // carries one create, whose `layout` and `module` name existing handles and
  // whose `workgroupSize` is the non-uniform `[8, 4, 2]`, and one destroy.
  const computePipeline = commands.find(
    (command) => command.name === 'CreateComputePipeline'
  );
  const destroyComputePipeline = commands.find(
    (command) => command.name === 'DestroyComputePipeline'
  );
  check(
    computePipeline !== undefined && destroyComputePipeline !== undefined,
    `the committed stream carries a CreateComputePipeline and a DestroyComputePipeline (${computePipeline ? 'found' : 'missing'})`
  );

  const graphicsPipeline = commands.find(
    (command) => command.name === 'CreateGraphicsPipeline'
  );
  const destroyGraphicsPipeline = commands.find(
    (command) => command.name === 'DestroyGraphicsPipeline'
  );
  check(
    graphicsPipeline !== undefined && destroyGraphicsPipeline !== undefined,
    `the committed stream carries a CreateGraphicsPipeline and a DestroyGraphicsPipeline (${graphicsPipeline ? 'found' : 'missing'})`
  );

  const [flatImage, volumeImage, lutImage] = images;
  const [
    cascadeView,
    volumeView,
    lutView,
    cubeView,
    cubeArrayView,
    stencilView,
  ] = views;

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
      'CreateImage',
      'DestroyImage',
      'CreateImageView',
      'DestroyImageView',
      'CreateSampler',
      'DestroySampler',
      'CreateBindGroupLayout',
      'DestroyBindGroupLayout',
      'CreateBindGroup',
      'DestroyBindGroup',
      'CreateShaderModule',
      'DestroyShaderModule',
      'CreatePipelineLayout',
      'DestroyPipelineLayout',
      'CreateComputePipeline',
      'DestroyComputePipeline',
      'CreateGraphicsPipeline',
      'DestroyGraphicsPipeline',
      // The readback path and the draw arms. Each records or answers rather than
      // throwing; the ones that cannot proceed (a pass with no encoder, a bind
      // or draw with no pass open, a poll of a readback nothing requested) route
      // to the error queue, which is not a throw. `PushConstants` is refused on
      // the error queue too — WebGPU has none — rather than thrown.
      // `PipelineBarrier` is the documented no-op: recognised, and it neither
      // throws nor touches the encoder, error queue, or device — its own gate
      // below observes that.
      'CreateCommandEncoder',
      'BeginRenderPass',
      'EndRenderPass',
      'BindGraphicsPipeline',
      'BindGroup',
      'PushConstants',
      'Draw',
      'BeginComputePass',
      'EndComputePass',
      'BindComputePipeline',
      'Dispatch',
      'CopyImageToBuffer',
      'CopyBufferToBuffer',
      'CopyBufferToImage',
      'CopyImageToImage',
      'FillBuffer',
      'PipelineBarrier',
      'Finish',
      'Submit',
      'RequestReadback',
      'PollReadback',
      'DestroyReadback',
      'DestroyCommandBuffer',
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
    const { replayer, device } = await readyWithDevice();
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
    const { replayer, device } = await readyWithDevice();
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
    const { replayer, device } = await readyWithDevice();
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
    const { replayer } = await readyWithDevice();
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
    const { replayer } = await readyWithDevice();
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
    const { replayer } = await readyWithDevice();
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
    const { replayer } = await readyWithDevice();
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
    const { replayer, device } = await readyWithDevice();
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
    const { replayer, device } = await readyWithDevice();
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
    const { replayer } = await readyWithDevice({ device });
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
    const { replayer, device } = await readyWithDevice();
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
    const { replayer, device } = await readyWithDevice();
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

  // ---- the image pair ------------------------------------------------------
  //
  // The buffer pair's shape with a descriptor that loses more: `ImageDesc`'s
  // five fields become a `GPUTextureDescriptor`'s seven, through three tables
  // that each have something to refuse. What is checked is what reached
  // `createTexture`, what the table holds afterwards, and where each refusal
  // went.
  {
    // **The descriptor WebGPU is handed**, field for field, from the fixture's
    // own labelled 2D image.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(flatImage, 40n));
    checkEqual(
      device.createdTextures,
      [
        {
          label: 'gbuffer albedo',
          dimension: '2d',
          size: { width: 1280, height: 720, depthOrArrayLayers: 3 },
          format: 'rgba8unorm-srgb',
          mipLevelCount: 11,
          sampleCount: 1,
          // TEXTURE_BINDING (0x4) for SAMPLED and RENDER_ATTACHMENT (0x10) for
          // COLOR_ATTACHMENT, which is the only attachment bit WebGPU has.
          usage: 0x14,
        },
      ],
      'a CreateImage reaches the device as one GPUTextureDescriptor'
    );
    const made = replayer.images.get(flatImage.image);
    check(
      made !== undefined &&
        made.width === 1280 &&
        made.height === 720 &&
        made.depthOrArrayLayers === 3 &&
        made.format === 'rgba8unorm-srgb' &&
        made.mipLevelCount === 11 &&
        made.usage === 0x14,
      `and the image is findable at its handle with the extent, format, mip count and usage asked for (${made?.width}x${made?.height}x${made?.depthOrArrayLayers}, ${made?.format}, ${made?.mipLevelCount} mips, usage 0x${made?.usage?.toString(16)})`
    );
    check(
      replayer.images.get(
        handle(flatImage.image.index, flatImage.image.generation + 1)
      ) === undefined,
      'a lookup with a stale generation does not find the live image'
    );
    check(
      !replayer.hasReplies &&
        replayer.inFlight === 0 &&
        replayer.pendingErrors === 0 &&
        replayer.takeError() === null,
      `an image command queues no reply, starts nothing and reports no error (queued ${replayer.hasReplies}, in flight ${replayer.inFlight}, errors ${replayer.pendingErrors})`
    );
  }
  {
    // **The volume**, which is the case `depth_or_layers` exists to make
    // dangerous: the same three numbers describe a 64-deep 3D texture here and
    // 64 flat slices in the image above, and only the dimension says which.
    // WebGPU folds the two meanings into `depthOrArrayLayers` on exactly the
    // same rule, so what this checks is that the fold survived rather than that
    // a number was copied.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(volumeImage, 41n));
    checkEqual(
      device.createdTextures,
      [
        {
          dimension: '3d',
          size: { width: 160, height: 90, depthOrArrayLayers: 64 },
          format: 'r16float',
          mipLevelCount: 7,
          sampleCount: 4,
          // STORAGE_BINDING (0x8) for STORAGE and COPY_SRC (0x1) for
          // TRANSFER_SRC.
          usage: 0x09,
        },
      ],
      'a D3 image is a 3d texture whose depthOrArrayLayers is its depth'
    );
    check(
      !('label' in (device.createdTextures[0] ?? {})),
      `a descriptor with no label passes none rather than an empty one (${JSON.stringify(device.createdTextures[0]?.label)})`
    );
  }
  {
    // The `D1` case, with the empty label the seam distinguishes from none and
    // with `mip_levels` and `samples` **passed on as zero**. No device accepts
    // either, and that is deliberately not this file's business: an invalid
    // descriptor is a creation failure the browser reports, exactly as a size no
    // device will allocate is for a buffer.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(creatableImages(images)[2], 42n));
    checkEqual(
      device.createdTextures,
      [
        {
          label: '',
          dimension: '1d',
          size: { width: 256, height: 1, depthOrArrayLayers: 1 },
          format: 'r8unorm',
          mipLevelCount: 0,
          sampleCount: 0,
          // COPY_DST (0x2) for TRANSFER_DST and TEXTURE_BINDING (0x4) for
          // SAMPLED, which is what `creatableImages` narrowed this one to.
          usage: 0x06,
        },
      ],
      'a D1 image passes its zero mip and sample counts on rather than second-guessing them'
    );
    // …and as the fixture actually wrote it, with `ImageUsage::all()`, the same
    // command is a refusal: that word carries `PRESENT`.
    const asWritten = await readyWithDevice();
    asWritten.replayer.replay(frameOf(lutImage, 43n));
    const reason = asWritten.replayer.takeError();
    check(
      asWritten.device.createdTextures.length === 0 &&
        asWritten.replayer.images.size === 0 &&
        String(reason).includes('PRESENT'),
      `an ImageUsage flag with no GPUTextureUsage bit is refused and named (${asWritten.device.createdTextures.length} created, ${JSON.stringify(reason)})`
    );
  }
  {
    // **A destroy destroys the texture as well as the slot**, which is the whole
    // reason this seam destroys explicitly rather than dropping a reference.
    const { replayer } = await readyWithDevice();
    replayer.replay(frameOf(flatImage, 44n));
    const made = replayer.images.get(flatImage.image);
    replayer.replay(frameOf({ ...destroyImage, image: flatImage.image }, 45n));
    check(
      made?.destroys === 1 &&
        replayer.images.get(flatImage.image) === undefined &&
        replayer.images.size === 0,
      `DestroyImage destroys the texture and lets go of the slot (${made?.destroys} destroys, ${replayer.images.size} held)`
    );
  }
  {
    // **A destroy of an empty slot**, which the fixture's own carries: its
    // handle is one no create in the corpus ever used.
    const { replayer } = await readyWithDevice();
    let thrown = null;
    try {
      replayer.replay(frameOf(destroyImage, 46n));
      replayer.replay(frameOf(destroyImageView, 47n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        replayer.images.size === 0 &&
        replayer.imageViews.size === 0,
      `a destroy for a handle nothing created is a no-op for both kinds (${String(thrown)})`
    );
  }
  {
    // **A DESTROY NAMING A STALE GENERATION RELEASES NOTHING.** The live
    // occupant of a reissued index must survive with its `destroy()` never
    // called — a destroyed texture something still holds a handle to is worse
    // than a leaked one.
    const { replayer } = await readyWithDevice();
    const reissued = handle(
      flatImage.image.index,
      flatImage.image.generation + 1
    );
    replayer.replay(frameOf({ ...flatImage, image: reissued }, 48n));
    const live = replayer.images.get(reissued);
    let thrown = null;
    try {
      replayer.replay(
        frameOf({ ...destroyImage, image: flatImage.image }, 49n)
      );
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        live !== undefined &&
        live.destroys === 0 &&
        replayer.images.get(reissued) === live &&
        replayer.images.size === 1,
      `a DestroyImage naming a stale generation is a no-op and leaves the live image alone (${String(thrown)}, ${live?.destroys} destroys, ${replayer.images.size} held)`
    );
  }
  {
    // **An ImageType no table claims.** A decoder that grew a variant this file
    // has not, and the one enum here where a default would be silent: guessing
    // `'2d'` for a `D3` turns a volume into a stack of slices, and the bytes are
    // identical either way.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf({ ...flatImage, imageType: 'D4' }, 50n));
    const reason = replayer.takeError();
    check(
      device.createdTextures.length === 0 &&
        replayer.images.size === 0 &&
        String(reason).includes('ImageType::D4'),
      `an ImageType with no GPUTextureDimension is refused rather than defaulted (${JSON.stringify(reason)})`
    );
  }
  {
    // **An image command before any device opened**, which is the buffer pair's
    // ordering rule one command kind further along.
    const replayer = new Replayer({ gpu: stubGpu(async () => null) });
    replayer.replay(frameOf(flatImage, 51n));
    const reason = replayer.takeError();
    check(
      replayer.images.size === 0 &&
        String(reason).includes('before any device opened'),
      `a CreateImage with no device is refused and says so (${JSON.stringify(reason)})`
    );
  }
  {
    // **A `createTexture` that throws.** Most WebGPU failures arrive on the
    // device's error channel instead, but an allocation failure may throw, and a
    // throw out of `replay` would abandon every command after it in the frame.
    const device = stubDevice([], {
      refuseTextures: new Error('out of memory'),
    });
    const { replayer } = await readyWithDevice({ device });
    let thrown = null;
    try {
      replayer.replay(frameOf(flatImage, 52n));
    } catch (error) {
      thrown = error;
    }
    const reason = replayer.takeError();
    check(
      thrown === null &&
        replayer.images.size === 0 &&
        String(reason).includes('out of memory'),
      `a createTexture that throws is recorded rather than thrown on (${String(thrown)}, ${JSON.stringify(reason)})`
    );
  }

  // ---- the view pair -------------------------------------------------------
  //
  // The one creation that reads a table: a `GPUTextureView` comes from the
  // texture rather than from the device, so the image handle in the descriptor
  // has to resolve before there is anything to call. That is what the first
  // checks here are about, and the sentinel resolution is what the rest are.
  {
    // **The descriptor the image is handed**, from the fixture's cube view —
    // every subresource field holding its own number, so a transposition inside
    // the range is visible.
    const { replayer, images: made } = await readyWithImages(images);
    replayer.replay(frameOf(cubeView, 60n));
    const image = replayer.images.get(made[0].image);
    checkEqual(
      image?.views,
      [
        {
          label: 'sky cube',
          dimension: 'cube',
          format: 'rgba8unorm',
          // ImageAspect::COLOR is 'all': a colour format has one plane and
          // 'all' is the only aspect WebGPU accepts for it.
          aspect: 'all',
          baseMipLevel: 13,
          mipLevelCount: 14,
          baseArrayLayer: 15,
          arrayLayerCount: 16,
        },
      ],
      'a CreateImageView reaches the image it names as one GPUTextureViewDescriptor'
    );
    const view = replayer.imageViews.get(cubeView.view);
    check(
      view !== undefined && view.of === image,
      `and the view is findable at its handle, and is a view of *that* image (${view?.of === image})`
    );
    check(
      replayer.imageViews.get(
        handle(cubeView.view.index, cubeView.view.generation + 1)
      ) === undefined,
      'a lookup with a stale generation does not find the live view'
    );
  }
  {
    // **THE SENTINEL BECOMES AN ABSENCE.** `ImageSubresourceRange::ALL` is
    // `0xFFFFFFFF` on the wire and WebGPU spells "the rest" as an absent
    // descriptor member, so what reaches `createView` must not carry the number
    // at all — passing it on would be refused by the browser and look like a
    // corrupt stream. The fixture puts the sentinel in `mip_count` on one view
    // and in `layer_count` on another, so neither can hide the other.
    const { replayer, images: made } = await readyWithImages(images);
    replayer.replay(frameOf(volumeView, 61n));
    replayer.replay(frameOf(cubeArrayView, 62n));
    const volume = replayer.images.get(made[1].image);
    checkEqual(
      volume?.views,
      [
        {
          dimension: '3d',
          format: 'r16float',
          aspect: 'all',
          baseMipLevel: 5,
          // No `mipLevelCount`: this is the sentinel, resolved.
          baseArrayLayer: 6,
          arrayLayerCount: 7,
        },
        {
          dimension: 'cube-array',
          format: 'bgra8unorm-srgb',
          aspect: 'all',
          baseMipLevel: 17,
          mipLevelCount: 8,
          baseArrayLayer: 18,
          // …and no `arrayLayerCount` here, which is the half a subtraction
          // would get wrong: WebGPU's default for an absent one depends on the
          // view's dimension, and is 6 for a 'cube' however many layers the
          // texture has.
        },
      ],
      'ImageSubresourceRange::ALL reaches the browser as an absent member, in either count'
    );
    const carried = (volume?.views ?? []).flatMap((desc) =>
      Object.entries(desc).filter(([, value]) => value === SUBRESOURCE_ALL)
    );
    check(
      carried.length === 0,
      `and nothing in either descriptor carries ${SUBRESOURCE_ALL} (${JSON.stringify(carried)})`
    );
  }
  {
    // **Every aspect the fixture spells**, through the commands rather than
    // through the table alone: a stencil-only view of a depth-stencil image, and
    // the `D1` view whose colour aspect is `'all'`.
    const { replayer, images: made } = await readyWithImages(images);
    replayer.replay(frameOf(stencilView, 63n));
    replayer.replay(frameOf(lutView, 64n));
    const lut = replayer.images.get(made[2].image);
    checkEqual(
      (lut?.views ?? []).map((desc) => [desc.dimension, desc.aspect]),
      [
        ['2d', 'stencil-only'],
        ['1d', 'all'],
      ],
      'ImageAspect::STENCIL is stencil-only and ImageAspect::COLOR is all'
    );
  }
  {
    // **A view naming an image that resolves to nothing**, in all three of the
    // ways it can, and none of them may throw: a view arriving before its image
    // is a far side that got its ordering wrong mid-frame, and a throw would
    // abandon every command after it in the frame.
    const { replayer, images: made } = await readyWithImages(images);
    const never = handle(200, 1);
    const stale = handle(made[0].image.index, made[0].image.generation + 1);
    let thrown = null;
    try {
      replayer.replay(frameOf({ ...cubeView, image: never }, 65n));
      replayer.replay(frameOf({ ...cubeView, image: stale }, 66n));
      // …and one whose image existed and has since been destroyed, which is the
      // case a table keyed on presence alone would still resolve.
      replayer.replay(frameOf({ ...destroyImage, image: made[1].image }, 67n));
      replayer.replay(frameOf(volumeView, 68n));
    } catch (error) {
      thrown = error;
    }
    const reasons = [
      replayer.takeError(),
      replayer.takeError(),
      replayer.takeError(),
    ];
    check(
      thrown === null &&
        replayer.imageViews.size === 0 &&
        reasons.every(
          (reason) =>
            typeof reason === 'string' && reason.includes('no live image under')
        ),
      `a CreateImageView naming an unresolvable image goes to the error queue and does not throw (${String(thrown)}, ${JSON.stringify(reasons)})`
    );
    check(
      replayer.takeError() === null,
      'and there is one error per view rather than one per frame'
    );
  }
  {
    // **A format this device did not enable the feature for.** The fixture's
    // cascade view asks for `D32FloatS8Uint`, which WebGPU gates behind
    // `depth32float-stencil8` — so the same command is a refusal on one device
    // and a view on another, which is what says the check reads the *device's*
    // features rather than a constant.
    const plain = await readyWithImages(images);
    plain.replayer.replay(frameOf(cascadeView, 73n));
    const reason = plain.replayer.takeError();
    check(
      plain.replayer.imageViews.size === 0 &&
        String(reason).includes('depth32float-stencil8'),
      `a Format the device did not enable the feature for is refused and names the feature (${JSON.stringify(reason)})`
    );

    const capable = await readyWithImages(images, {
      device: stubDevice(['depth32float-stencil8']),
    });
    capable.replayer.replay(frameOf(cascadeView, 74n));
    const view = capable.replayer.imageViews.get(cascadeView.view);
    checkEqual(
      capable.replayer.images.get(capable.images[0].image)?.views,
      [
        {
          label: 'cascade 2',
          dimension: '2d-array',
          format: 'depth32float-stencil8',
          // ImageAspect::DEPTH | STENCIL is 'all' too, and it is not a
          // collision with COLOR's: the format says which planes exist.
          aspect: 'all',
          baseMipLevel: 1,
          mipLevelCount: 2,
          baseArrayLayer: 3,
          arrayLayerCount: 4,
        },
      ],
      'and the same command is a view on a device that did enable it'
    );
    check(
      view !== undefined && capable.replayer.takeError() === null,
      `with nothing in the error queue (${JSON.stringify(capable.replayer.takeError())})`
    );
  }
  {
    // **A destroy lets go of the view**, and there is nothing else to let go
    // of: a `GPUTextureView` has no `destroy()` at all, so the slot is the whole
    // of the release.
    const { replayer } = await readyWithImages(images);
    replayer.replay(frameOf(cubeView, 75n));
    const reissued = handle(cubeView.view.index, cubeView.view.generation + 1);
    replayer.replay(frameOf({ ...destroyImageView, view: reissued }, 76n));
    check(
      replayer.imageViews.get(cubeView.view) !== undefined &&
        replayer.imageViews.size === 1,
      `a DestroyImageView naming a stale generation leaves the live view alone (${replayer.imageViews.size} held)`
    );
    replayer.replay(frameOf({ ...destroyImageView, view: cubeView.view }, 77n));
    check(
      replayer.imageViews.get(cubeView.view) === undefined &&
        replayer.imageViews.size === 0,
      `and one naming the live handle lets go of it (${replayer.imageViews.size} held)`
    );
  }
  {
    // **Four kinds, one set of handle bits.** The image and its view carry
    // identical bits here — as the probe's own do, deliberately — and neither
    // table may see the other: the opcode is the only thing that says which
    // table an id indexes.
    const { replayer } = await readyWithDevice();
    const same = flatImage.image;
    replayer.replay(frameOf({ ...flatImage, image: same }, 80n));
    replayer.replay(frameOf({ ...cubeView, view: same, image: same }, 81n));
    const image = replayer.images.get(same);
    const view = replayer.imageViews.get(same);
    check(
      image !== undefined &&
        view !== undefined &&
        view.of === image &&
        replayer.images.size === 1 &&
        replayer.imageViews.size === 1,
      `a view may carry its image's handle bits without either table losing an entry (${replayer.images.size} images, ${replayer.imageViews.size} views)`
    );
    replayer.replay(frameOf({ ...destroyImageView, view: same }, 82n));
    check(
      replayer.images.get(same) === image && image.destroys === 0,
      `and destroying the view leaves the image alone (${image?.destroys} destroys)`
    );
  }

  // ---- the ImageUsage → GPUTextureUsage mapping, flag by flag --------------
  //
  // Spelled out rather than compared against the table it is testing, for the
  // buffer mapping's reason. Each flag on its own, so one wired to the wrong bit
  // names itself instead of hiding inside a union.
  {
    for (const [name, bit, webgpu] of [
      ['TRANSFER_SRC', GPU_TEXTURE_USAGE.COPY_SRC, 'COPY_SRC'],
      ['TRANSFER_DST', GPU_TEXTURE_USAGE.COPY_DST, 'COPY_DST'],
      ['SAMPLED', GPU_TEXTURE_USAGE.TEXTURE_BINDING, 'TEXTURE_BINDING'],
      ['STORAGE', GPU_TEXTURE_USAGE.STORAGE_BINDING, 'STORAGE_BINDING'],
      [
        'COLOR_ATTACHMENT',
        GPU_TEXTURE_USAGE.RENDER_ATTACHMENT,
        'RENDER_ATTACHMENT',
      ],
      [
        'DEPTH_STENCIL_ATTACHMENT',
        GPU_TEXTURE_USAGE.RENDER_ATTACHMENT,
        'RENDER_ATTACHMENT',
      ],
    ]) {
      checkEqual(
        webgpuTextureUsageFor([name]),
        { bits: bit, unsatisfiable: [] },
        `ImageUsage::${name} maps to GPUTextureUsage.${webgpu} and to nothing else`
      );
    }
    // The two attachment flags share a bit, which is not a loss: WebGPU has one
    // attachment usage and reads which kind off the format. Asserted rather than
    // left implicit in the rows above, because it is the one place two seam
    // values legitimately produce the same number.
    checkEqual(
      webgpuTextureUsageFor(['COLOR_ATTACHMENT', 'DEPTH_STENCIL_ATTACHMENT']),
      { bits: GPU_TEXTURE_USAGE.RENDER_ATTACHMENT, unsatisfiable: [] },
      'both attachment flags are RENDER_ATTACHMENT, which WebGPU splits by format'
    );
    // The one flag with nothing behind it. A presentable image is not something
    // `createTexture` can make at all — a canvas's texture comes from
    // `getCurrentTexture()` — so it is named rather than dropped, and the bits
    // that did map are still reported.
    checkEqual(
      webgpuTextureUsageFor(['SAMPLED', 'PRESENT']),
      {
        bits: GPU_TEXTURE_USAGE.TEXTURE_BINDING,
        unsatisfiable: ['ImageUsage::PRESENT'],
      },
      'ImageUsage::PRESENT has no GPUTextureUsage bit and comes back unsatisfiable'
    );
  }

  // ---- the Format → GPUTextureFormat mapping, row by row -------------------
  //
  // Every one of the seam's formats, its WebGPU name written out here, and the
  // `GPUFeatureName` gating it where one does. The gated rows are driven twice —
  // against a device that enabled the feature and one that did not — because the
  // whole point of reading the *device's* features is that the answer moves.
  {
    /** Every `Format` name, its WebGPU spelling, and its gate. */
    const rows = [
      ['R8_UNORM', 'r8unorm', null],
      ['RG8_UNORM', 'rg8unorm', null],
      ['RGBA8_UNORM', 'rgba8unorm', null],
      ['RGBA8_UNORM_SRGB', 'rgba8unorm-srgb', null],
      ['BGRA8_UNORM', 'bgra8unorm', null],
      ['BGRA8_UNORM_SRGB', 'bgra8unorm-srgb', null],
      ['RGB10A2_UNORM', 'rgb10a2unorm', null],
      // WebGPU renamed this one: `rg11b10ufloat` carries R:11 G:11 B:10, which
      // is what the seam spells `R11g11b10Float`.
      ['R11G11B10_FLOAT', 'rg11b10ufloat', null],
      ['R16_FLOAT', 'r16float', null],
      ['RG16_FLOAT', 'rg16float', null],
      ['RGBA16_FLOAT', 'rgba16float', null],
      ['R32_FLOAT', 'r32float', null],
      ['RG32_FLOAT', 'rg32float', null],
      ['RGBA32_FLOAT', 'rgba32float', null],
      ['R32_UINT', 'r32uint', null],
      ['RG32_UINT', 'rg32uint', null],
      ['D32_FLOAT', 'depth32float', null],
      ['D32_FLOAT_S8_UINT', 'depth32float-stencil8', 'depth32float-stencil8'],
      // **The one inexact row.** WebGPU deleted `depth24unorm-stencil8`, and
      // what is left promises *at least* 24 bits of unsigned-normalised depth —
      // so this widens what the caller gets rather than narrowing it, and the
      // seam has no call that could see the difference.
      ['D24_UNORM_S8_UINT', 'depth24plus-stencil8', null],
      ['D16_UNORM', 'depth16unorm', null],
      ['BC1_RGBA_UNORM', 'bc1-rgba-unorm', 'texture-compression-bc'],
      ['BC1_RGBA_UNORM_SRGB', 'bc1-rgba-unorm-srgb', 'texture-compression-bc'],
      ['BC3_RGBA_UNORM', 'bc3-rgba-unorm', 'texture-compression-bc'],
      ['BC3_RGBA_UNORM_SRGB', 'bc3-rgba-unorm-srgb', 'texture-compression-bc'],
      ['BC4_R_UNORM', 'bc4-r-unorm', 'texture-compression-bc'],
      ['BC5_RG_UNORM', 'bc5-rg-unorm', 'texture-compression-bc'],
      ['BC6H_RGB_UFLOAT', 'bc6h-rgb-ufloat', 'texture-compression-bc'],
      ['BC7_RGBA_UNORM', 'bc7-rgba-unorm', 'texture-compression-bc'],
      ['BC7_RGBA_UNORM_SRGB', 'bc7-rgba-unorm-srgb', 'texture-compression-bc'],
    ];
    const everything = new Set([
      'texture-compression-bc',
      'depth32float-stencil8',
    ]);
    const wrong = [];
    const ungated = [];
    for (const [name, webgpu, feature] of rows) {
      const granted = webgpuTextureFormatFor(name, everything);
      if (granted.name !== webgpu || granted.reason !== null) {
        wrong.push(`${name}: ${JSON.stringify(granted)}`);
      }
      // On a device with nothing enabled, a gated format is refused with the
      // feature named and an ungated one still answers.
      const bare = webgpuTextureFormatFor(name, new Set());
      if (feature === null) {
        if (bare.name !== webgpu)
          ungated.push(`${name}: ${JSON.stringify(bare)}`);
      } else if (bare.name !== null || !String(bare.reason).includes(feature)) {
        ungated.push(`${name}: ${JSON.stringify(bare)}`);
      }
    }
    check(
      wrong.length === 0,
      wrong[0] ??
        `every crcbl_hal::Format has the GPUTextureFormat this file names (${rows.length} of them)`
    );
    check(
      ungated.length === 0,
      ungated[0] ??
        'and a device with no features gets the ungated ones and a named refusal for the rest'
    );
    // The device's features and never the adapter's, which is the distinction
    // the whole check rests on: an adapter that could have granted the feature
    // says nothing about a device nobody asked for it for.
    check(
      webgpuTextureFormatFor('BC7_RGBA_UNORM', undefined).name === null,
      'a source with no features at all refuses a gated format rather than throwing'
    );
    // A format with no row is this file and `gpu-reply.js`'s FORMAT table having
    // drifted, and is refused rather than answered with a neighbour: a nearby
    // format is a colour-space or precision bug nothing downstream could
    // attribute.
    const drifted = webgpuTextureFormatFor('ASTC_4X4_UNORM', everything);
    check(
      drifted.name === null &&
        String(drifted.reason).includes('ASTC_4X4_UNORM'),
      `a Format this file has no row for is refused and named (${JSON.stringify(drifted)})`
    );
  }

  // ---- the ImageAspect → GPUTextureAspect mapping --------------------------
  //
  // A bitflags set meeting a three-valued enum: eight combinations spellable,
  // three words to spell them in. Every combination is driven, because the four
  // with no spelling are exactly the ones a translation would silently round to
  // `'all'`.
  {
    for (const [aspect, spelled] of [
      [['COLOR'], 'all'],
      [['DEPTH', 'STENCIL'], 'all'],
      [['DEPTH'], 'depth-only'],
      [['STENCIL'], 'stencil-only'],
    ]) {
      checkEqual(
        webgpuTextureAspectFor(aspect),
        { aspect: spelled, reason: null },
        `ImageAspect::${aspect.join(' | ')} is GPUTextureAspect '${spelled}'`
      );
    }
    const refused = [];
    for (const aspect of [
      [],
      ['COLOR', 'DEPTH'],
      ['COLOR', 'STENCIL'],
      ['COLOR', 'DEPTH', 'STENCIL'],
    ]) {
      const answer = webgpuTextureAspectFor(aspect);
      if (answer.aspect !== null || answer.reason === null) {
        refused.push(`${JSON.stringify(aspect)}: ${JSON.stringify(answer)}`);
      }
    }
    check(
      refused.length === 0,
      refused[0] ??
        'and every aspect combination WebGPU has no word for is refused rather than rounded to all'
    );
  }

  // ---- the sampler pair ----------------------------------------------------
  //
  // The creation with the most to decide and the least to check afterwards: a
  // `GPUSampler` reports its `label` and nothing else, so every claim below is
  // made against the `GPUSamplerDescriptor` the device was *handed* rather than
  // against what it answered. Three of the nine seam fields arrive as something
  // WebGPU cannot hold, and each of the three has its own block.
  {
    // **The descriptor WebGPU is handed**, field for field, from the fixture's
    // labelled shadow sampler — the one of the four this backend can build.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(shadowSampler, 90n));
    checkEqual(
      device.createdSamplers,
      [
        {
          label: 'shadow pcf',
          // U, V and W in the descriptor's order, and three different modes so a
          // translation that wrote one of them into all three cannot pass.
          addressModeU: 'repeat',
          addressModeV: 'mirror-repeat',
          addressModeW: 'clamp-to-edge',
          magFilter: 'linear',
          minFilter: 'nearest',
          // WebGPU spells the seam's `mip_filter` `mipmapFilter`, and an
          // unrecognised member is not an error — WebIDL ignores it and the
          // sampler gets the default `'nearest'` instead. Named here so the
          // spelling is what the check is about.
          mipmapFilter: 'nearest',
          lodMinClamp: 0.5,
          // **The sentinel, resolved to itself and written explicitly.**
          lodMaxClamp: LOD_NO_LIMIT,
          maxAnisotropy: 1,
          // The reversed-Z shadow test. `'less'` would be its exact inverse and
          // the browser would accept it just as happily.
          compare: 'greater',
        },
      ],
      'a CreateSampler reaches the device as one GPUSamplerDescriptor'
    );
    const made = replayer.samplers.get(shadowSampler.sampler);
    check(
      made !== undefined && made.label === 'shadow pcf',
      `and the sampler is findable at its handle with the label that was asked for (${JSON.stringify(made?.label)})`
    );
    check(
      replayer.samplers.get(
        handle(
          shadowSampler.sampler.index,
          shadowSampler.sampler.generation + 1
        )
      ) === undefined,
      'a lookup with a stale generation does not find the live sampler'
    );
    check(
      !replayer.hasReplies &&
        replayer.inFlight === 0 &&
        replayer.pendingErrors === 0 &&
        replayer.takeError() === null,
      `a sampler command queues no reply, starts nothing and reports no error (queued ${replayer.hasReplies}, in flight ${replayer.inFlight}, errors ${replayer.pendingErrors})`
    );
  }
  {
    // **THE SENTINEL IS A MEMBER, NOT AN ABSENCE — WHICH IS THE OPPOSITE OF THE
    // VIEW'S RANGE.** `ImageSubresourceRange::ALL` reaches `createView` by being
    // left out, because WebGPU's default for an absent `arrayLayerCount` really
    // is "the rest". `lod_max`'s `f32::MAX` cannot: WebGPU's default for an
    // absent `lodMaxClamp` is a *number*, so omitting the member would swap "no
    // limit" for a clamp — and a mip clamp is not something a `GPUSampler`, a
    // draw, or an error channel ever reports, so nothing downstream could
    // attribute the softness that follows.
    //
    // Two claims, because one of them alone would pass on an omission: that the
    // member is present, and that it holds the number the wire carried.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(shadowSampler, 91n));
    const [desc] = device.createdSamplers;
    check(
      desc !== undefined && 'lodMaxClamp' in desc,
      `lodMaxClamp is written rather than omitted (members: ${Object.keys(desc ?? {}).join(', ')})`
    );
    check(
      desc?.lodMaxClamp === LOD_NO_LIMIT,
      `and it carries the wire's own f32::MAX rather than a resolution of it (${desc?.lodMaxClamp})`
    );
    check(
      shadowSampler.lodMax === LOD_NO_LIMIT,
      `the command under test really carries the sentinel (${shadowSampler.lodMax})`
    );
    // …and `lodMinClamp` is written too, for the same reason one step less
    // dramatic: its WebGPU default is 0, which is also the seam's, so an
    // omission would be invisible on every descriptor but this one.
    check(
      desc !== undefined && 'lodMinClamp' in desc && desc.lodMinClamp === 0.5,
      `lodMinClamp is written too and carries what the wire said (${desc?.lodMinClamp})`
    );
  }
  {
    // **An absent comparison is an absent member**, not one holding `undefined`
    // — the care `#createImageView` takes with the range's counts, applied to
    // the one optional member here. `CompareOp::Never` is code 0 on the wire, so
    // "no comparison" and "a comparison that always fails" are a byte apart, and
    // a sampler built with the second returns zero everywhere.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf({ ...shadowSampler, compare: null }, 92n));
    replayer.replay(
      frameOf(
        {
          ...shadowSampler,
          sampler: handle(shadowSampler.sampler.index + 1, 1),
          compare: 'Never',
        },
        93n
      )
    );
    checkEqual(
      device.createdSamplers.map((desc) => [
        'compare' in desc,
        desc.compare ?? null,
      ]),
      [
        [false, null],
        [true, 'never'],
      ],
      'compare: None is an absent member and compare: Some(Never) is a present one'
    );
  }
  {
    // **`ClampToBorder` HAS NO `GPUAddressMode` AND IS REFUSED**, which is
    // `BufferUsage::DEVICE_ADDRESS`'s decision applied to an enum. Both of the
    // fixture's commands that carry it are driven, because they carry it on
    // *different axes* and the refusal has to say which — all three members are
    // the same enum, so a message naming only the variant leaves a reader
    // guessing.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(borderSampler, 94n));
    const atU = replayer.takeError();
    replayer.replay(frameOf(coarseSampler, 95n));
    const atV = replayer.takeError();
    check(
      device.createdSamplers.length === 0 &&
        replayer.samplers.size === 0 &&
        String(atU).includes('U is ClampToBorder') &&
        String(atV).includes('V is ClampToBorder'),
      `a SamplerAddressMode with no GPUAddressMode is refused and names its axis (${JSON.stringify([atU, atV])})`
    );
  }
  {
    // **A FRACTIONAL ANISOTROPY IS FLOORED, AND A LARGE ONE IS NOT WRAPPED.**
    // `maxAnisotropy` is a `GPUSize32` and WebIDL's conversion to one is
    // modular rather than clamping, so `4.5` would reach the device as `4` by
    // truncation and `4294967297` as `1` — the first is what this file chooses
    // deliberately and the second is what it exists to prevent. The fixture's
    // all-linear sampler asks for the fractional one.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(anisoSampler, 96n));
    check(
      anisoSampler.anisotropy === 4.5 &&
        device.createdSamplers[0]?.maxAnisotropy === 4,
      `a fractional anisotropy is floored rather than rounded (${anisoSampler.anisotropy} → ${device.createdSamplers[0]?.maxAnisotropy})`
    );
    check(
      replayer.samplers.get(anisoSampler.sampler) !== undefined &&
        replayer.takeError() === null,
      'and the sampler is made, because all three of its filters are linear'
    );
  }
  {
    // …and the same value with a filter that is not `'linear'` is refused,
    // which is WebGPU's own rule rather than arithmetic: a descriptor breaking
    // it is answered with a `GPUSampler` object and a validation error a turn
    // later, so the handle would hold an invalid sampler and every draw using it
    // would fail again with nothing naming the creation. The fixture's coarse
    // sampler carries anisotropy 16 with two nearest filters — and also a
    // `ClampToBorder`, so the address modes are replaced here to leave this
    // check about the one rule it is named for.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(
      frameOf(
        { ...coarseSampler, addressMode: ['Repeat', 'Repeat', 'Repeat'] },
        97n
      )
    );
    const reason = replayer.takeError();
    check(
      device.createdSamplers.length === 0 &&
        replayer.samplers.size === 0 &&
        String(reason).includes("all three are 'linear'"),
      `an anisotropy above 1 beside a non-linear filter is refused and says why (${JSON.stringify(reason)})`
    );
  }
  {
    // **A CLAMP THAT IS NOT FINITE IS REFUSED HERE**, because `lodMinClamp` and
    // `lodMaxClamp` are WebIDL `float`s: a NaN or an infinity is a `TypeError`
    // out of `createSampler` naming neither the member nor the value, which this
    // method would then report as "could not be created" and nothing more. The
    // wire carries every `f32` bit pattern, so this is the half that has to say
    // which of them WebGPU cannot hold.
    const { replayer, device } = await readyWithDevice();
    const reasons = [];
    for (const [at, field] of [
      ['lodMin', 'lod_min'],
      ['lodMax', 'lod_max'],
    ]) {
      replayer.replay(frameOf({ ...shadowSampler, [at]: Infinity }, 98n));
      reasons.push(replayer.takeError());
      replayer.replay(frameOf({ ...shadowSampler, [at]: NaN }, 99n));
      reasons.push([field, replayer.takeError()]);
    }
    check(
      device.createdSamplers.length === 0 &&
        replayer.samplers.size === 0 &&
        reasons.every((reason) => String(reason).includes('clamp')),
      `an infinite or NaN lod clamp is refused rather than thrown at the browser (${JSON.stringify(reasons)})`
    );
  }
  {
    // **A filter or a comparison no table here claims** is a decoder that has
    // grown a variant this file has not. Refused rather than defaulted, for the
    // reason `#createImage` refuses an unknown `ImageType`: a `GPUSamplerDescriptor`
    // ignores a member it does not recognise, so a wrong *name* would build a
    // sampler with WebGPU's default filter and say nothing.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf({ ...shadowSampler, mipFilter: 'Cubic' }, 100n));
    const filter = replayer.takeError();
    replayer.replay(frameOf({ ...shadowSampler, compare: 'Nearer' }, 101n));
    const compare = replayer.takeError();
    check(
      device.createdSamplers.length === 0 &&
        String(filter).includes('FilterMode::') &&
        String(filter).includes('Cubic') &&
        String(compare).includes('CompareOp::Nearer'),
      `a FilterMode or CompareOp with no WebGPU spelling is refused and named (${JSON.stringify([filter, compare])})`
    );
  }
  {
    // **A destroy lets go of the sampler, and there is nothing else to let go
    // of**: a `GPUSampler` has no `destroy()` at all, so the slot is the whole
    // of the release — `#destroyImageView`'s shape. Both no-ops are driven, and
    // the stale one first: the live occupant of a reissued index must survive.
    const { replayer } = await readyWithDevice();
    replayer.replay(frameOf(shadowSampler, 102n));
    const live = replayer.samplers.get(shadowSampler.sampler);
    const reissued = handle(
      shadowSampler.sampler.index,
      shadowSampler.sampler.generation + 1
    );
    let thrown = null;
    try {
      replayer.replay(frameOf({ ...destroySampler, sampler: reissued }, 103n));
      // …and the fixture's own destroy, whose handle no create here ever used.
      replayer.replay(frameOf(destroySampler, 104n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        replayer.samplers.get(shadowSampler.sampler) === live &&
        replayer.samplers.size === 1,
      `a DestroySampler naming a stale generation or an empty slot is a no-op (${String(thrown)}, ${replayer.samplers.size} held)`
    );
    replayer.replay(
      frameOf({ ...destroySampler, sampler: shadowSampler.sampler }, 105n)
    );
    check(
      replayer.samplers.get(shadowSampler.sampler) === undefined &&
        replayer.samplers.size === 0,
      `and one naming the live handle lets go of it (${replayer.samplers.size} held)`
    );
  }
  {
    // **Five kinds, one set of handle bits.** The probe's sampler carries the
    // same eight bytes as its image, its view, its buffer and its surface,
    // deliberately — so this is the shape a replayer with one table would break,
    // and there are now five tables that must not see each other.
    const { replayer } = await readyWithDevice();
    const same = flatImage.image;
    replayer.replay(frameOf({ ...flatImage, image: same }, 106n));
    replayer.replay(frameOf({ ...cubeView, view: same, image: same }, 107n));
    replayer.replay(frameOf({ ...shadowSampler, sampler: same }, 108n));
    const image = replayer.images.get(same);
    check(
      image !== undefined &&
        replayer.imageViews.get(same) !== undefined &&
        replayer.samplers.get(same) !== undefined &&
        replayer.images.size === 1 &&
        replayer.imageViews.size === 1 &&
        replayer.samplers.size === 1,
      `a sampler may carry an image's and a view's handle bits without any table losing an entry (${replayer.images.size} images, ${replayer.imageViews.size} views, ${replayer.samplers.size} samplers)`
    );
    replayer.replay(frameOf({ ...destroySampler, sampler: same }, 109n));
    check(
      replayer.images.get(same) === image &&
        image.destroys === 0 &&
        replayer.imageViews.get(same) !== undefined,
      `and destroying the sampler leaves the image and the view alone (${image?.destroys} destroys)`
    );
  }
  {
    // **A sampler command before any device opened**, and **a `createSampler`
    // that throws** — the two failures every creation here shares, driven for
    // this one because an error that goes nowhere is the same bug as a dropped
    // reply one seam over.
    const bare = new Replayer({ gpu: stubGpu(async () => null) });
    bare.replay(frameOf(shadowSampler, 110n));
    const early = bare.takeError();

    const device = stubDevice([], {
      refuseSamplers: new Error('out of memory'),
    });
    const { replayer } = await readyWithDevice({ device });
    let thrown = null;
    try {
      replayer.replay(frameOf(shadowSampler, 111n));
    } catch (error) {
      thrown = error;
    }
    const refused = replayer.takeError();
    check(
      bare.samplers.size === 0 &&
        String(early).includes('before any device opened') &&
        thrown === null &&
        replayer.samplers.size === 0 &&
        String(refused).includes('out of memory'),
      `a CreateSampler with no device, and a createSampler that throws, are both recorded rather than thrown on (${JSON.stringify([early, refused])}, ${String(thrown)})`
    );
  }

  // ---- the SamplerAddressMode → GPUAddressMode mapping, row by row ---------
  //
  // Spelled out rather than compared against the table it is testing, for the
  // usage mappings' reason. Each mode in each of the three positions, so one
  // wired to the wrong word names itself instead of hiding behind its neighbour.
  {
    const wrong = [];
    for (const [name, webgpu] of [
      ['Repeat', 'repeat'],
      ['MirrorRepeat', 'mirror-repeat'],
      ['ClampToEdge', 'clamp-to-edge'],
    ]) {
      const answer = webgpuAddressModesFor([name, name, name]);
      if (answer.reason !== null || answer.modes.some((m) => m !== webgpu)) {
        wrong.push(`${name}: ${JSON.stringify(answer)}`);
      }
    }
    check(
      wrong.length === 0,
      wrong[0] ??
        'every SamplerAddressMode WebGPU can express maps to the GPUAddressMode this file names'
    );
    // The order is U, V, W and is not sorted, symmetrical or otherwise
    // recoverable: three different modes at once is the only shape that says so.
    checkEqual(
      webgpuAddressModesFor(['ClampToEdge', 'Repeat', 'MirrorRepeat']),
      { modes: ['clamp-to-edge', 'repeat', 'mirror-repeat'], reason: null },
      'and the three come back in the order the wire had them'
    );
    // The one with no row. Named per axis, because all three members are the
    // same enum and a message naming only the variant leaves a reader guessing.
    const refusedPerAxis = [];
    for (const at of [0, 1, 2]) {
      const modes = ['Repeat', 'Repeat', 'Repeat'];
      modes[at] = 'ClampToBorder';
      const answer = webgpuAddressModesFor(modes);
      const axis = 'UVW'[at];
      if (
        answer.modes !== null ||
        !String(answer.reason).includes(`${axis} is ClampToBorder`)
      ) {
        refusedPerAxis.push(`${axis}: ${JSON.stringify(answer)}`);
      }
    }
    check(
      refusedPerAxis.length === 0,
      refusedPerAxis[0] ??
        'ClampToBorder has no GPUAddressMode and is refused by the axis that carried it'
    );
    // A mode with no row at all is this file and `gpu-stream.js`'s table having
    // drifted, and is refused rather than answered with a neighbour.
    const drifted = webgpuAddressModesFor([
      'MirrorClampToEdge',
      'Repeat',
      'Repeat',
    ]);
    check(
      drifted.modes === null &&
        String(drifted.reason).includes('MirrorClampToEdge'),
      `a SamplerAddressMode this file has no row for is refused and named (${JSON.stringify(drifted)})`
    );
  }

  // ---- the anisotropy rule, case by case -----------------------------------
  //
  // A float meeting a `GPUSize32` whose WebIDL conversion is **modular**, so
  // every case is decided in `gpu-replay.js` rather than left to the browser.
  // The rows below are the whole rule, and the ones that pass are as important
  // as the ones that do not: flattening every ask to 1 would also make every
  // refusal here vacuous.
  {
    const linear = ['linear', 'linear', 'linear'];
    const mixed = ['linear', 'nearest', 'linear'];
    for (const [anisotropy, filters, expected, what] of [
      [1, mixed, 1, '1.0 disables anisotropy and needs no linear filter'],
      [1.9, mixed, 1, 'a value under 2 floors to 1, which is still disabled'],
      [4.5, linear, 4, 'a fractional value floors rather than rounding up'],
      [16, linear, 16, 'an integer ask beside linear filters is passed on'],
      [
        0xffff_ffff,
        linear,
        0xffff_ffff,
        'the largest GPUSize32 is still a value',
      ],
    ]) {
      checkEqual(
        webgpuMaxAnisotropyFor(anisotropy, filters),
        { maxAnisotropy: expected, reason: null },
        `maxAnisotropy: ${what}`
      );
    }
    const refusals = [];
    for (const [anisotropy, filters, phrase, what] of [
      [Number.NaN, linear, 'not a ratio', 'a NaN is not a ratio'],
      [Infinity, linear, 'not a ratio', 'an infinity is not a ratio'],
      [0.5, linear, 'below the 1.0', 'a value below the seam’s own floor'],
      [0, linear, 'below the 1.0', 'zero, which WebGPU validates against too'],
      [
        4_294_967_296,
        linear,
        'past the largest maxAnisotropy',
        'one past a GPUSize32, which WebIDL would wrap to 0',
      ],
      [
        LOD_NO_LIMIT,
        linear,
        'past the largest maxAnisotropy',
        'f32::MAX, which is lod_max’s sentinel and not this field’s',
      ],
      [
        16,
        mixed,
        "all three are 'linear'",
        'an ask above 1 beside a nearest filter',
      ],
    ]) {
      const answer = webgpuMaxAnisotropyFor(anisotropy, filters);
      if (
        answer.maxAnisotropy !== null ||
        !String(answer.reason).includes(phrase)
      ) {
        refusals.push(`${what}: ${JSON.stringify(answer)}`);
      }
    }
    check(
      refusals.length === 0,
      refusals[0] ??
        'and every anisotropy WebGPU cannot carry is refused with the reason named'
    );
  }

  // ---- the bind-group-layout pair ------------------------------------------
  //
  // The creation whose descriptor WebGPU has the least room for, and the first
  // whose body is a **list**. A `GPUBindGroupLayout` reports its `label` and
  // nothing else — no entries, no bindings, no visibility — so every claim below
  // is made against the `GPUBindGroupLayoutDescriptor` the device was *handed*,
  // as the sampler's are.
  {
    // **The whole descriptor, entry for entry**, from the fixture's long layout
    // — every `BindingKind` this backend can express, each with both values of
    // every `bool` it carries. Spelled out rather than derived: a list built by
    // calling the translation would agree with it whatever it said.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(frameLayout, 120n));
    checkEqual(
      device.createdLayouts,
      [
        {
          label: 'frame',
          entries: [
            {
              binding: 0,
              visibility: 0x1, // GPUShaderStage.VERTEX
              buffer: { type: 'read-only-storage', hasDynamicOffset: false },
            },
            {
              binding: 1,
              visibility: 0x4, // GPUShaderStage.COMPUTE
              buffer: { type: 'storage', hasDynamicOffset: true },
            },
            {
              binding: 2,
              visibility: 0x1 | 0x2,
              buffer: { type: 'uniform', hasDynamicOffset: true },
            },
            {
              binding: 3,
              visibility: 0x2, // GPUShaderStage.FRAGMENT
              buffer: { type: 'uniform', hasDynamicOffset: false },
            },
            {
              binding: 4,
              visibility: 0x2,
              texture: {
                sampleType: 'depth',
                viewDimension: '2d-array',
                multisampled: false,
              },
            },
            { binding: 5, visibility: 0x2, sampler: { type: 'comparison' } },
            {
              binding: 6,
              visibility: 0x2,
              texture: {
                sampleType: 'float',
                viewDimension: 'cube',
                multisampled: false,
              },
            },
            { binding: 7, visibility: 0x4, sampler: { type: 'filtering' } },
          ],
        },
      ],
      'a CreateBindGroupLayout reaches the device as one GPUBindGroupLayoutDescriptor'
    );
    const made = replayer.bindGroupLayouts.get(frameLayout.layout);
    check(
      made !== undefined && made.label === 'frame',
      `and the layout is findable at its handle with the label that was asked for (${JSON.stringify(made?.label)})`
    );
    check(
      replayer.bindGroupLayouts.get(
        handle(frameLayout.layout.index, frameLayout.layout.generation + 1)
      ) === undefined,
      'a lookup with a stale generation does not find the live layout'
    );
    check(
      !replayer.hasReplies &&
        replayer.inFlight === 0 &&
        replayer.pendingErrors === 0 &&
        replayer.takeError() === null,
      `a layout command queues no reply, starts nothing and reports no error (queued ${replayer.hasReplies}, in flight ${replayer.inFlight}, errors ${replayer.pendingErrors})`
    );
  }
  {
    // **THE ENTRIES KEEP THE SLICE'S ORDER AND ARE NOT KEYED BY BINDING
    // NUMBER.** `docs/plan/41-webgpu-stream.md` requires it because a
    // `VARIABLE_COUNT` entry must be *last in the slice* as well as
    // highest-numbered, and the fixture's storage-image layout is what makes it
    // checkable from this side: its two entries share binding 11 and differ in
    // exactly one field, so a replayer that built its list from a map keyed on
    // `binding` would hand the device one entry instead of two.
    //
    // It is driven with the kind replaced, because `StorageImage` is the one
    // WebGPU cannot express and is refused two blocks down — leaving this check
    // about the ordering it is named for. Every other field is the command the
    // Rust encoder wrote.
    const sampled = {
      ...storageImageLayout,
      entries: storageImageLayout.entries.map((entry) => ({
        ...entry,
        kind: {
          name: 'SampledImage',
          viewType: 'D2',
          sampleType: entry.kind.readOnly ? 'Depth' : 'Float',
        },
      })),
    };
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(sampled, 121n));
    checkEqual(
      device.createdLayouts[0]?.entries?.map((entry) => [
        entry.binding,
        entry.texture?.sampleType,
      ]),
      [
        [11, 'float'],
        [11, 'depth'],
      ],
      'two entries sharing a binding number both reach the device, in slice order'
    );
    check(
      replayer.bindGroupLayouts.get(sampled.layout) !== undefined &&
        replayer.takeError() === null,
      'and the layout is built: a duplicate binding is the browser’s to refuse, not this file’s'
    );
  }
  {
    // **THE `u32::MAX` COUNT IS REFUSED, AND SO IS EVERY OTHER COUNT BUT ONE.**
    // WebGPU core has no binding arrays: `GPUBindGroupLayoutEntry` has no
    // `count` member and no `GPUBindGroup` syntax fills one. Accepting either of
    // these and building a single-descriptor binding is the worst outcome
    // available — every later write to slot 1 upward would name a descriptor
    // that does not exist, and the browser would report it against the *bind
    // group* rather than against the layout that was wrong.
    //
    // The two carry different messages deliberately: the sentinel is the
    // portable bindless declaration and a fixed count is a caller asking for an
    // array of a size it knows, and a reader has to be able to tell which
    // arrived.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(arrayLayout, 122n));
    const fixed = replayer.takeError();
    // The bindless layout's own flags are refused before its count is reached,
    // so the sentinel is driven with them cleared — the count is what this check
    // is named for, and the flags have their own block below.
    const sentinelOnly = {
      ...bindlessLayout,
      entries: bindlessLayout.entries.map((entry) => ({ ...entry, flags: [] })),
    };
    replayer.replay(frameOf(sentinelOnly, 123n));
    const sentinel = replayer.takeError();
    check(
      device.createdLayouts.length === 0 &&
        replayer.bindGroupLayouts.size === 0 &&
        String(fixed).includes('64 descriptors') &&
        String(sentinel).includes('as many descriptors as the device can hold'),
      `a count that is not 1 is refused, and the u32::MAX sentinel says which it was (${JSON.stringify([fixed, sentinel])})`
    );
    check(
      arrayLayout.entries[0]?.count === 64 &&
        sentinelOnly.entries[1]?.count === 0xffff_ffff,
      `the commands under test really carry the counts (${arrayLayout.entries[0]?.count}, ${sentinelOnly.entries[1]?.count})`
    );
  }
  {
    // **EVERY `BindingFlags` IS REFUSED, AND THE REFUSAL NAMES THEM.** All three
    // require `Features::DESCRIPTOR_INDEXING`, which this backend never reports
    // because WebGPU has no bindless model at all — so a layout setting any of
    // them is one this seam cannot build. `BindingFlags`'s own docs make the
    // loudness obligatory: "a bindless array quietly downgraded to a fixed one
    // reads garbage at index 4097."
    //
    // Each flag on its own as well as the fixture's set of three, because one
    // standing for the others is a reading the loop would hide — and
    // `VARIABLE_COUNT` in particular is the one whose *ordering* rule this file
    // then does not have to re-check, precisely because it never gets this far.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(bindlessLayout, 124n));
    const all = replayer.takeError();
    const named = [];
    for (const [at, flag] of [
      'PARTIALLY_BOUND',
      'UPDATE_AFTER_BIND',
      'VARIABLE_COUNT',
    ].entries()) {
      replayer.replay(
        frameOf(
          {
            ...frameLayout,
            entries: [{ ...frameLayout.entries[0], flags: [flag] }],
          },
          BigInt(125 + at)
        )
      );
      named.push(String(replayer.takeError()));
    }
    check(
      device.createdLayouts.length === 0 &&
        replayer.bindGroupLayouts.size === 0 &&
        String(all).includes('BindingFlags::VARIABLE_COUNT') &&
        named.every((reason, at) =>
          reason.includes(
            `BindingFlags::${['PARTIALLY_BOUND', 'UPDATE_AFTER_BIND', 'VARIABLE_COUNT'][at]}`
          )
        ),
      `every BindingFlags is refused and named, one at a time and together (${JSON.stringify([all, ...named])})`
    );
  }
  {
    // **`MESH` AND `TASK` HAVE NO `GPUShaderStage` BIT AND ARE REFUSED**, which
    // is `ImageUsage::PRESENT`'s decision applied to a stage. Dropping the bit
    // would be worse than refusing: a layout narrower than the caller asked for
    // does not fail at creation, it fails at the *pipeline*, as a shader reading
    // a binding the layout says it may not see.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(meshLayout, 130n));
    const mesh = replayer.takeError();
    replayer.replay(
      frameOf({ ...meshLayout, entries: [meshLayout.entries[1]] }, 131n)
    );
    const task = replayer.takeError();
    check(
      device.createdLayouts.length === 0 &&
        replayer.bindGroupLayouts.size === 0 &&
        String(mesh).includes('ShaderStages::MESH') &&
        String(task).includes('ShaderStages::TASK'),
      `a visibility naming a stage WebGPU has no bit for is refused and named (${JSON.stringify([mesh, task])})`
    );
  }
  {
    // **`StorageImage` IS THE ONE `BindingKind` WEBGPU CANNOT EXPRESS FROM THIS
    // SEAM**, and the reason is a *missing* field rather than a missing word:
    // `GPUStorageTextureBindingLayout.format` is required and has no default,
    // and `crcbl_hal::BindingKind::StorageImage` carries `read_only` and nothing
    // else — the other three backends take the format off the bound view. There
    // is no value this backend could supply that would not be a guess.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(storageImageLayout, 132n));
    const reason = replayer.takeError();
    check(
      device.createdLayouts.length === 0 &&
        replayer.bindGroupLayouts.size === 0 &&
        String(reason).includes('BindingKind::StorageImage') &&
        String(reason).includes('format'),
      `a StorageImage binding is refused and says which member is missing (${JSON.stringify(reason)})`
    );
  }
  {
    // **AN EMPTY ENTRY LIST IS A LAYOUT**, which is the length a loop most
    // easily treats as nothing to do. WebGPU accepts one, and this seam has to
    // pass it on rather than skip the creation: the handle wasm allocated has to
    // hold something, or every bind group made against it fails instead.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(emptyLayout, 133n));
    checkEqual(
      device.createdLayouts,
      // A present-and-empty label, which is not an absent one: the fixture's
      // third layout carries `Some("")`.
      [{ label: '', entries: [] }],
      'a layout with no entries is created with an empty entry list'
    );
    check(
      replayer.bindGroupLayouts.get(emptyLayout.layout) !== undefined &&
        replayer.takeError() === null,
      'and it lands in the table like any other'
    );
  }
  {
    // **A destroy lets go of the layout, and there is nothing else to let go
    // of**: a `GPUBindGroupLayout` has no `destroy()`, so the slot is the whole
    // of the release. Both no-ops are driven, and the stale one first: the live
    // occupant of a reissued index must survive.
    const { replayer } = await readyWithDevice();
    replayer.replay(frameOf(frameLayout, 134n));
    const live = replayer.bindGroupLayouts.get(frameLayout.layout);
    const reissued = handle(
      frameLayout.layout.index,
      frameLayout.layout.generation + 1
    );
    let thrown = null;
    try {
      replayer.replay(frameOf({ ...destroyLayout, layout: reissued }, 135n));
      // …and the fixture's own destroy, whose handle no create here ever used.
      replayer.replay(frameOf(destroyLayout, 136n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        replayer.bindGroupLayouts.get(frameLayout.layout) === live &&
        replayer.bindGroupLayouts.size === 1,
      `a DestroyBindGroupLayout naming a stale generation or an empty slot is a no-op (${String(thrown)}, ${replayer.bindGroupLayouts.size} held)`
    );
    replayer.replay(
      frameOf({ ...destroyLayout, layout: frameLayout.layout }, 137n)
    );
    check(
      replayer.bindGroupLayouts.get(frameLayout.layout) === undefined &&
        replayer.bindGroupLayouts.size === 0,
      `and one naming the live handle lets go of it (${replayer.bindGroupLayouts.size} held)`
    );
  }
  {
    // **Six kinds, one set of handle bits.** The probe's layout carries the same
    // eight bytes as its sampler, its image, its view, its buffer and its
    // surface, deliberately — so this is the shape a replayer with one table
    // would break, and there are now six tables that must not see each other.
    const { replayer } = await readyWithDevice();
    const same = flatImage.image;
    replayer.replay(frameOf({ ...flatImage, image: same }, 138n));
    replayer.replay(frameOf({ ...cubeView, view: same, image: same }, 139n));
    replayer.replay(frameOf({ ...shadowSampler, sampler: same }, 140n));
    replayer.replay(frameOf({ ...frameLayout, layout: same }, 141n));
    const image = replayer.images.get(same);
    check(
      image !== undefined &&
        replayer.imageViews.get(same) !== undefined &&
        replayer.samplers.get(same) !== undefined &&
        replayer.bindGroupLayouts.get(same) !== undefined &&
        replayer.bindGroupLayouts.size === 1,
      `a layout may carry an image's, a view's and a sampler's handle bits without any table losing an entry (${replayer.images.size} images, ${replayer.imageViews.size} views, ${replayer.samplers.size} samplers, ${replayer.bindGroupLayouts.size} layouts)`
    );
    replayer.replay(frameOf({ ...destroyLayout, layout: same }, 142n));
    check(
      replayer.images.get(same) === image &&
        image.destroys === 0 &&
        replayer.samplers.get(same) !== undefined,
      `and destroying the layout leaves the image and the sampler alone (${image?.destroys} destroys)`
    );
  }
  {
    // **A layout command before any device opened**, and **a
    // `createBindGroupLayout` that throws** — the two failures every creation
    // here shares.
    const bare = new Replayer({ gpu: stubGpu(async () => null) });
    bare.replay(frameOf(frameLayout, 143n));
    const early = bare.takeError();

    const device = stubDevice([], {
      refuseLayouts: new Error('too many bind group layouts'),
    });
    const { replayer } = await readyWithDevice({ device });
    let thrown = null;
    try {
      replayer.replay(frameOf(frameLayout, 144n));
    } catch (error) {
      thrown = error;
    }
    const refused = replayer.takeError();
    check(
      bare.bindGroupLayouts.size === 0 &&
        String(early).includes('before any device opened') &&
        thrown === null &&
        replayer.bindGroupLayouts.size === 0 &&
        String(refused).includes('too many bind group layouts'),
      `a CreateBindGroupLayout with no device, and a createBindGroupLayout that throws, are both recorded rather than thrown on (${JSON.stringify([early, refused])}, ${String(thrown)})`
    );
  }

  // ---- the bind-group pair -------------------------------------------------
  //
  // The first creation that reads FOUR tables — its layout, and each entry's
  // resource out of the buffer, image-view or sampler table — and the first whose
  // entries carry handles into three different tables at once. A handle carries no
  // kind, so the entry's discriminant is the only thing that says which table an
  // id indexes; the checks below drive a group whose handles resolve, three that
  // name a missing resource of each kind, and the two shapes WebGPU cannot
  // express. A `GPUBindGroup` reports its `label` and nothing else, so every claim
  // is made against the `GPUBindGroupDescriptor` the device was *handed*, as the
  // sampler's and the layout's are.
  //
  // The resources a bind group names have to be built first, so these helpers
  // record a layout and a resource of each kind at a chosen handle. Every
  // descriptor is one this backend accepts, so what is under test is the group.
  const emptyLayoutAt = (h) => ({
    name: 'CreateBindGroupLayout',
    layout: h,
    label: null,
    entries: [],
  });
  const storageBufferAt = (h) => ({
    name: 'CreateBuffer',
    buffer: h,
    label: null,
    size: 4096n,
    usage: ['STORAGE'],
    memory: 'DeviceLocal',
  });
  const sampledImageAt = (h) => ({
    name: 'CreateImage',
    image: h,
    label: null,
    imageType: 'D2',
    extent: { width: 4, height: 4, depthOrLayers: 1 },
    format: 'RGBA8_UNORM',
    mipLevels: 1,
    samples: 1,
    usage: ['SAMPLED'],
  });
  const viewOf = (h, imageHandle) => ({
    name: 'CreateImageView',
    view: h,
    label: null,
    image: imageHandle,
    viewType: 'D2',
    format: 'RGBA8_UNORM',
    range: {
      aspect: ['COLOR'],
      baseMip: 0,
      mipCount: SUBRESOURCE_ALL,
      baseLayer: 0,
      layerCount: SUBRESOURCE_ALL,
    },
  });
  const samplerAt = (h) => ({
    name: 'CreateSampler',
    sampler: h,
    label: null,
    magFilter: 'Linear',
    minFilter: 'Linear',
    mipFilter: 'Linear',
    addressMode: ['ClampToEdge', 'Repeat', 'MirrorRepeat'],
    lodMin: 0,
    lodMax: LOD_NO_LIMIT,
    anisotropy: 1,
    compare: null,
  });
  {
    // **THE HAPPY PATH: the fixture's material group, built against resources
    // created at the handles it names.** Every entry resolves against the right
    // table, the buffer offset and size reach the descriptor, and — the field
    // this whole command exists to put in front of a device — the `WHOLE_BUFFER`
    // sentinel reaches WebGPU as an *absent* `size` member rather than as
    // `18446744073709551615`, which is how WebGPU spells "to the end".
    const { replayer, device } = await readyWithDevice();
    const bufA = materialGroup.entries[0].resource.buffer;
    const bufB = materialGroup.entries[3].resource.buffer;
    const view = materialGroup.entries[1].resource.view;
    const sampler = materialGroup.entries[2].resource.sampler;
    const image = handle(220, 1);
    replayer.replay(frameOf(emptyLayoutAt(materialGroup.layout), 200n));
    replayer.replay(frameOf(storageBufferAt(bufA), 201n));
    replayer.replay(frameOf(storageBufferAt(bufB), 202n));
    replayer.replay(frameOf(sampledImageAt(image), 203n));
    replayer.replay(frameOf(viewOf(view, image), 204n));
    replayer.replay(frameOf(samplerAt(sampler), 205n));
    replayer.replay(frameOf(materialGroup, 206n));

    const bd = device.createdGroups[0];
    const built =
      device.createdGroups.length === 1 &&
      bd.layout === replayer.bindGroupLayouts.get(materialGroup.layout) &&
      bd.entries.length === 4 &&
      bd.entries[0].binding === 0 &&
      bd.entries[0].resource.buffer === replayer.buffers.get(bufA) &&
      bd.entries[0].resource.offset === 256 &&
      bd.entries[0].resource.size === 1024 &&
      bd.entries[1].binding === 1 &&
      bd.entries[1].resource === replayer.imageViews.get(view) &&
      bd.entries[2].binding === 2 &&
      bd.entries[2].resource === replayer.samplers.get(sampler) &&
      bd.entries[3].binding === 3 &&
      bd.entries[3].resource.buffer === replayer.buffers.get(bufB) &&
      bd.entries[3].resource.offset === 0;
    check(
      built,
      built
        ? 'a CreateBindGroup reaches the device as one GPUBindGroupDescriptor, each entry resolved against the right table'
        : `the built descriptor is not what was asked for (${JSON.stringify(bd, (_, v) => (typeof v === 'bigint' ? String(v) : v))})`
    );
    // **The sentinel, on its own line.** A real numbered size passes through and
    // the sentinel becomes an absent member — a check on both, because a
    // translation that omitted *every* size, or passed the max integer on, would
    // fail one half or the other.
    check(
      bd.entries[0].resource.size === 1024 &&
        !('size' in bd.entries[3].resource) &&
        materialGroup.entries[3].resource.size === BUFFER_WHOLE,
      `WHOLE_BUFFER resolves to an absent size member while a real size passes through (${'size' in bd.entries[3].resource}, ${materialGroup.entries[3].resource.size})`
    );
    check(
      replayer.bindGroups.get(materialGroup.group) !== undefined &&
        !replayer.hasReplies &&
        replayer.takeError() === null,
      `and the group lands in its table, queues no reply and reports no error (queued ${replayer.hasReplies})`
    );
  }
  {
    // **A `variable_count` of `Some` is refused — a runtime-sized array WebGPU
    // has none of.** It could only pair with a `VARIABLE_COUNT` layout, which the
    // layout command already refuses, so it never reaches a layout this replayer
    // holds, and refusing it here is the only honest answer. Refused *before* the
    // layout is even looked up.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(bindlessGroup, 210n));
    const reason = replayer.takeError();
    check(
      device.createdGroups.length === 0 &&
        replayer.bindGroups.size === 0 &&
        bindlessGroup.variableCount === 2 &&
        String(reason).includes('variable_count') &&
        String(reason).includes('runtime-sized'),
      `a Some variable_count is refused as a runtime-sized array (${JSON.stringify(reason)})`
    );
  }
  {
    // **A non-zero `array_index` is refused — the bindless write path, which
    // WebGPU cannot express.** A `GPUBindGroupEntry` indexes by `binding` only,
    // and slice 5d refused every layout with a count above one, so no layout this
    // replayer holds could accept an element past the first. Driven with the
    // fixture's bindless group and its `variable_count` cleared. Its layout and
    // both the views its two entries name are created first, so the refusal is the
    // `array_index` on the *second* entry and not a missing layout or a missing
    // resource on the first — which resolves cleanly at `array_index` 0.
    const { replayer, device } = await readyWithDevice();
    const image = handle(230, 1);
    replayer.replay(frameOf(emptyLayoutAt(bindlessGroup.layout), 218n));
    replayer.replay(frameOf(sampledImageAt(image), 219n));
    replayer.replay(
      frameOf(viewOf(bindlessGroup.entries[0].resource.view, image), 220n)
    );
    replayer.replay(
      frameOf(viewOf(bindlessGroup.entries[1].resource.view, image), 220n)
    );
    replayer.replay(frameOf({ ...bindlessGroup, variableCount: null }, 221n));
    const reason = replayer.takeError();
    check(
      device.createdGroups.length === 0 &&
        replayer.bindGroups.size === 0 &&
        bindlessGroup.entries[1].arrayIndex === 1 &&
        String(reason).includes('array index') &&
        String(reason).includes('binding only'),
      `a non-zero array_index is refused as the bindless write path WebGPU has no syntax for (${JSON.stringify(reason)})`
    );
  }
  {
    // **A handle that resolves to nothing goes to the error queue and names which
    // resource and which kind** — `#createImageView`'s judgement applied to three
    // tables at once, and never a throw: a bind group naming a stale handle is a
    // far side that got its ordering wrong, and taking the frame down would
    // abandon every command after it. One case per table, plus the layout, plus
    // a discriminant no arm claims.
    const { replayer } = await readyWithDevice();
    replayer.replay(frameOf(emptyLayoutAt(handle(50, 1)), 230n));
    const against = (resource) => ({
      name: 'CreateBindGroup',
      group: handle(60, 1),
      label: null,
      layout: handle(50, 1),
      entries: [{ binding: 0, arrayIndex: 0, resource }],
      variableCount: null,
    });
    let thrown = null;
    let noBuffer;
    let noView;
    let noSampler;
    let noKind;
    let noLayout;
    try {
      replayer.replay(
        frameOf(
          against({
            name: 'Buffer',
            buffer: handle(70, 7),
            offset: 0n,
            size: 16n,
          }),
          231n
        )
      );
      noBuffer = replayer.takeError();
      replayer.replay(
        frameOf(against({ name: 'ImageView', view: handle(71, 7) }), 232n)
      );
      noView = replayer.takeError();
      replayer.replay(
        frameOf(against({ name: 'Sampler', sampler: handle(72, 7) }), 233n)
      );
      noSampler = replayer.takeError();
      // A discriminant no arm claims — the decoder refuses this before replay, so
      // it can only arrive from a bug, and the arm is here to name it rather than
      // bind it as a neighbour.
      replayer.replay(frameOf(against({ name: 'Nonsense' }), 234n));
      noKind = replayer.takeError();
      // …and the layout itself missing, which is looked up before any entry.
      replayer.replay(
        frameOf(
          {
            name: 'CreateBindGroup',
            group: handle(61, 1),
            label: null,
            layout: handle(99, 9),
            entries: [],
            variableCount: null,
          },
          235n
        )
      );
      noLayout = replayer.takeError();
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        replayer.bindGroups.size === 0 &&
        String(noBuffer).includes('no live buffer') &&
        String(noView).includes('no live image view') &&
        String(noSampler).includes('no live sampler') &&
        String(noKind).includes('BindingResource::Nonsense') &&
        String(noLayout).includes('no live bind group layout'),
      `an unresolvable buffer, view, sampler, discriminant or layout each names itself without throwing (${String(thrown)}, ${JSON.stringify([noBuffer, noView, noSampler, noKind, noLayout])})`
    );
  }
  {
    // **A bind group may carry its layout's handle bits.** The probe files six
    // kinds under one set of bits, deliberately, so this is the shape a replayer
    // with one table would break: the group is built against a layout whose handle
    // is the same eight bytes, and both tables keep their entry.
    const { replayer } = await readyWithDevice();
    const same = handle(1, 1);
    replayer.replay(frameOf(emptyLayoutAt(same), 240n));
    replayer.replay(
      frameOf(
        {
          name: 'CreateBindGroup',
          group: same,
          label: null,
          layout: same,
          entries: [],
          variableCount: null,
        },
        241n
      )
    );
    check(
      replayer.bindGroupLayouts.get(same) !== undefined &&
        replayer.bindGroups.get(same) !== undefined &&
        replayer.bindGroupLayouts.size === 1 &&
        replayer.bindGroups.size === 1,
      `a bind group may carry its layout's handle bits without either table losing an entry (${replayer.bindGroupLayouts.size} layouts, ${replayer.bindGroups.size} groups)`
    );
  }
  {
    // **A destroy lets go of the group, and there is nothing else to let go of**:
    // a `GPUBindGroup` has no `destroy()`, so the slot is the whole of the
    // release. Both no-ops are driven, and the stale one first: the live occupant
    // of a reissued index must survive.
    const { replayer } = await readyWithDevice();
    replayer.replay(frameOf(emptyLayoutAt(handle(50, 1)), 250n));
    const group = handle(60, 1);
    replayer.replay(
      frameOf(
        {
          name: 'CreateBindGroup',
          group,
          label: null,
          layout: handle(50, 1),
          entries: [],
          variableCount: null,
        },
        251n
      )
    );
    const live = replayer.bindGroups.get(group);
    const reissued = handle(group.index, group.generation + 1);
    let thrown = null;
    try {
      replayer.replay(frameOf({ ...destroyGroup, group: reissued }, 252n));
      // …and the fixture's own destroy, whose handle no create here ever used.
      replayer.replay(frameOf(destroyGroup, 253n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        replayer.bindGroups.get(group) === live &&
        replayer.bindGroups.size === 1,
      `a DestroyBindGroup naming a stale generation or an empty slot is a no-op (${String(thrown)}, ${replayer.bindGroups.size} held)`
    );
    replayer.replay(frameOf({ ...destroyGroup, group }, 254n));
    check(
      replayer.bindGroups.get(group) === undefined &&
        replayer.bindGroups.size === 0,
      `and one naming the live handle lets go of it (${replayer.bindGroups.size} held)`
    );
  }
  {
    // **A bind group before any device opened**, and **a `createBindGroup` that
    // throws** — the two failures every creation here shares.
    const bare = new Replayer({ gpu: stubGpu(async () => null) });
    bare.replay(frameOf(materialGroup, 260n));
    const early = bare.takeError();

    const device = stubDevice([], {
      refuseGroups: new Error('too many bind groups'),
    });
    const { replayer } = await readyWithDevice({ device });
    replayer.replay(frameOf(emptyLayoutAt(handle(50, 1)), 261n));
    let thrown = null;
    try {
      replayer.replay(
        frameOf(
          {
            name: 'CreateBindGroup',
            group: handle(60, 1),
            label: null,
            layout: handle(50, 1),
            entries: [],
            variableCount: null,
          },
          262n
        )
      );
    } catch (error) {
      thrown = error;
    }
    const refused = replayer.takeError();
    check(
      bare.bindGroups.size === 0 &&
        String(early).includes('before any device opened') &&
        thrown === null &&
        replayer.bindGroups.size === 0 &&
        String(refused).includes('too many bind groups'),
      `a CreateBindGroup with no device, and a createBindGroup that throws, are both recorded rather than thrown on (${JSON.stringify([early, refused])}, ${String(thrown)})`
    );
  }

  // ---- the shader-module pair ----------------------------------------------
  {
    // **ONLY WGSL CROSSES TO THE BROWSER, AND EVERY OTHER ARTIFACT IS DROPPED.**
    // The full module carries `spirv`, `wgsl`, `msl` and `dxil`; a WebGPU device
    // compiles only WGSL, so what reaches `createShaderModule` is `{ label, code }`
    // and nothing else. The SPIR-V words, the MSL source and the DXIL containers
    // are for the Rust decoders — `crcbl-dx12` reads the DXIL, `crcbl-mtl` the MSL
    // — and have no WebGPU path, so they are read off the wire and then dropped.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(fullShader, 300n));
    checkEqual(
      device.createdShaderModules,
      [{ label: 'mesh.slang', code: fullShader.wgsl }],
      'a CreateShaderModule reaches the device as { label, code } with only its WGSL'
    );
    const made = replayer.shaderModules.get(fullShader.module);
    check(
      made !== undefined && made.label === 'mesh.slang',
      `and the module is findable at its handle with the label asked for (${JSON.stringify(made?.label)})`
    );
    check(
      replayer.shaderModules.get(
        handle(fullShader.module.index, fullShader.module.generation + 1)
      ) === undefined,
      'a lookup with a stale generation does not find the live module'
    );
    check(
      !replayer.hasReplies &&
        replayer.inFlight === 0 &&
        replayer.pendingErrors === 0 &&
        replayer.takeError() === null,
      `a shader-module command queues no reply, starts nothing and reports no error (queued ${replayer.hasReplies}, in flight ${replayer.inFlight}, errors ${replayer.pendingErrors})`
    );
  }
  {
    // **`Some("")` IS A VALID EMPTY MODULE AND BUILDS.** An empty WGSL string is a
    // module with no entry points — a real thing — and must not be confused with
    // `None`; a replayer that refused it would turn a real, if useless, module
    // into a failure the seam does not have. So `code: ''` reaches the device and
    // the module is filed, with nothing on the error queue.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(emptyWgslShader, 301n));
    checkEqual(
      device.createdShaderModules,
      [{ label: 'empty.wgsl', code: '' }],
      'a Some("") WGSL builds a module with no entry points rather than being refused'
    );
    check(
      replayer.shaderModules.get(emptyWgslShader.module) !== undefined &&
        replayer.takeError() === null,
      'the empty module is built and the device reported nothing'
    );
  }
  {
    // **A MODULE CARRYING NO WGSL IS REFUSED, BY NAME, AND NOTHING REACHES THE
    // DEVICE.** `ShaderModuleDesc::unusable`: a WebGPU backend compiles only WGSL,
    // and this module — `spirv` empty, `wgsl` `None`, `msl` `Some("")`, `dxil` one
    // pair — carries MSL and DXIL and no WGSL. Refused synchronously, as
    // `#createBuffer` refuses a usage WebGPU has no bit for, rather than built
    // empty: an empty module filed under the handle is a shader no pipeline can
    // use, and every pipeline naming it would fail with the reason nowhere near
    // the creation. The message names both sides of the gap.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(noWgslShader, 302n));
    const refused = replayer.takeError();
    check(
      device.createdShaderModules.length === 0 &&
        replayer.shaderModules.size === 0 &&
        typeof refused === 'string' &&
        refused.includes('can only compile WGSL') &&
        refused.includes('MSL and DXIL'),
      `a module with no WGSL is refused before the device is asked, naming what it was given (${JSON.stringify(refused)})`
    );
  }
  {
    // **A shader module before any device is refused, like every device method.**
    // `Device::create_shader_module` is a device method, so a stream carrying one
    // before its `RequestDevice` settled is asking the replayer for what it has
    // not got — recorded rather than thrown, for the buffer pair's reason.
    const bare = new Replayer({ gpu: stubGpu(async () => null) });
    bare.replay(frameOf(fullShader, 303n));
    const early = bare.takeError();
    check(
      typeof early === 'string' && early.includes('before any device opened'),
      `a shader module before a device is a device error, not a throw (${JSON.stringify(early)})`
    );
  }
  {
    // **THE DESTROYS: AN EMPTY SLOT IS A NO-OP AND A STALE GENERATION LEAVES THE
    // LIVE OCCUPANT ALONE.** `crcbl-render` leans on this destroy hardest — it
    // pre-allocates the handle, destroys it, and only then applies `?` — so a
    // destroy naming a generation the slot has moved past must not release the
    // reissued index's current module, and a destroy naming an empty slot must not
    // throw. Both are the one branch {@link HandleTable#remove} answers `undefined`
    // for.
    const { replayer } = await readyWithDevice();
    replayer.replay(frameOf(fullShader, 304n));
    const live = replayer.shaderModules.get(fullShader.module);
    const reissued = handle(
      fullShader.module.index,
      fullShader.module.generation + 1
    );
    let thrown = null;
    try {
      replayer.replay(frameOf({ ...destroyShader, module: reissued }, 305n));
      replayer.replay(frameOf(destroyShader, 306n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        live !== undefined &&
        replayer.shaderModules.get(fullShader.module) === live,
      `a stale-generation destroy leaves the live module and an empty-slot destroy does not throw (${String(thrown)})`
    );
    replayer.replay(
      frameOf({ ...destroyShader, module: fullShader.module }, 307n)
    );
    check(
      replayer.shaderModules.get(fullShader.module) === undefined,
      'and destroying the module by its own handle releases it'
    );
  }

  // ---- the pipeline-layout pair --------------------------------------------
  //
  // The last thing a pipeline is built from, and the second creation that reads
  // the bind-group-layout table: a pipeline layout's set list is what a shader's
  // `@group(n)` indexes. A `GPUPipelineLayout` reports its `label` and nothing
  // else — not its bind-group layouts, not its push-constant ranges (WebGPU has
  // none) — so every claim below is made against the `GPUPipelineLayoutDescriptor`
  // the device was *handed*, as the layout's and the group's are.
  {
    // **The set list resolves in order, and the layout builds.** The two
    // bind-group layouts the fixture's pipeline layout names are created first,
    // and the descriptor the device is handed carries the two resolved
    // `GPUBindGroupLayout` objects in set order — distinct objects, so a decoder
    // that reversed the list would be caught here rather than only at the shader.
    const { replayer, device } = await readyWithDevice();
    const [setA, setB] = twoSetLayout.bindGroupLayouts;
    replayer.replay(frameOf(emptyLayoutAt(setA), 320n));
    replayer.replay(frameOf(emptyLayoutAt(setB), 321n));
    replayer.replay(frameOf(twoSetLayout, 322n));
    const pd = device.createdPipelineLayouts[0];
    const built =
      device.createdPipelineLayouts.length === 1 &&
      pd.label === 'gbuffer' &&
      pd.bindGroupLayouts.length === 2 &&
      pd.bindGroupLayouts[0] === replayer.bindGroupLayouts.get(setA) &&
      pd.bindGroupLayouts[1] === replayer.bindGroupLayouts.get(setB) &&
      pd.bindGroupLayouts[0] !== pd.bindGroupLayouts[1] &&
      twoSetLayout.pushConstants === null;
    check(
      built,
      built
        ? 'a CreatePipelineLayout reaches the device as one GPUPipelineLayoutDescriptor, its set list resolved in order'
        : `the built descriptor is not what was asked for (${JSON.stringify(pd, (_, v) => (typeof v === 'bigint' ? String(v) : v))})`
    );
    check(
      replayer.pipelineLayouts.get(twoSetLayout.layout) !== undefined &&
        !replayer.hasReplies &&
        replayer.pendingErrors === 0 &&
        replayer.takeError() === null,
      `and the layout lands in its table, queues no reply and reports no error (queued ${replayer.hasReplies})`
    );
  }
  {
    // **A `Some` push-constant range is refused — WebGPU has no push constants at
    // all.** WGSL cannot express one, so `Features::PUSH_CONSTANTS` is never
    // reported and there is no `GPUPipelineLayout` member to carry the range;
    // `Device::create_pipeline_layout`'s own docs require this fail loudly rather
    // than dropping the writes later. Refused *before* the set list is looked up,
    // so the fixture's push-constant layout is driven with none of its set
    // layouts created and the refusal is still the push constants.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(pushConstantLayout, 323n));
    const reason = replayer.takeError();
    check(
      device.createdPipelineLayouts.length === 0 &&
        replayer.pipelineLayouts.size === 0 &&
        pushConstantLayout.pushConstants !== null &&
        String(reason).includes('push-constant range') &&
        String(reason).includes('no push constants'),
      `a Some push-constant range is refused as something WebGPU cannot express (${JSON.stringify(reason)})`
    );
  }
  {
    // **A set handle that resolves to nothing goes to the error queue and names
    // which set index** — `#createBindGroup`'s judgement applied to the set list,
    // and never a throw. Index 0 resolves and index 1 does not, so the message is
    // about set 1 and not a push-constant refusal or a wholly empty table.
    const { replayer, device } = await readyWithDevice();
    const present = handle(300, 1);
    const missing = handle(301, 7);
    replayer.replay(frameOf(emptyLayoutAt(present), 330n));
    let thrown = null;
    let reason = null;
    try {
      replayer.replay(
        frameOf(
          {
            name: 'CreatePipelineLayout',
            layout: handle(310, 1),
            label: null,
            bindGroupLayouts: [present, missing],
            pushConstants: null,
          },
          331n
        )
      );
      reason = replayer.takeError();
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        device.createdPipelineLayouts.length === 0 &&
        replayer.pipelineLayouts.size === 0 &&
        String(reason).includes('set 1') &&
        String(reason).includes('no live bind group layout'),
      `an unresolvable set handle is refused to the error queue naming its index, without throwing (${String(thrown)}, ${JSON.stringify(reason)})`
    );
  }
  {
    // **Destroys: a stale generation is a no-op that leaves the live occupant, and
    // an empty slot does not throw** — the ordinary case here, since every layout
    // the replayer refuses above leaves its handle empty for the caller to destroy
    // anyway.
    const { replayer } = await readyWithDevice();
    const [setA, setB] = twoSetLayout.bindGroupLayouts;
    replayer.replay(frameOf(emptyLayoutAt(setA), 340n));
    replayer.replay(frameOf(emptyLayoutAt(setB), 341n));
    replayer.replay(frameOf(twoSetLayout, 342n));
    const live = replayer.pipelineLayouts.get(twoSetLayout.layout);
    const stale = handle(
      twoSetLayout.layout.index,
      twoSetLayout.layout.generation + 1
    );
    let thrown = null;
    try {
      replayer.replay(
        frameOf({ ...destroyPipelineLayout, layout: stale }, 343n)
      );
      replayer.replay(frameOf(destroyPipelineLayout, 344n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        live !== undefined &&
        replayer.pipelineLayouts.get(twoSetLayout.layout) === live,
      `a stale-generation destroy leaves the live pipeline layout and an empty-slot destroy does not throw (${String(thrown)})`
    );
    replayer.replay(
      frameOf({ ...destroyPipelineLayout, layout: twoSetLayout.layout }, 345n)
    );
    check(
      replayer.pipelineLayouts.get(twoSetLayout.layout) === undefined,
      'and destroying the layout by its own handle releases it'
    );
  }

  // ---- the compute-pipeline pair -------------------------------------------
  //
  // The first command that resolves handles into two *different* non-buffer
  // tables: `layout` against the pipeline-layout table and `module` against the
  // shader-module table. A `GPUComputePipeline` reports its `label` and answers
  // `getBindGroupLayout(n)`; the descriptor the device is *handed* is what the
  // checks below read, as the layout's and the group's are — and the one field
  // that matters most about it is the one that is not there.
  //
  // The fixture's compute pipeline names `fullShader`'s module handle and
  // `twoSetLayout`'s layout handle, so those two are what the frame builds first.
  const emptyPipelineLayoutAt = (h) => ({
    name: 'CreatePipelineLayout',
    layout: h,
    label: null,
    bindGroupLayouts: [],
    pushConstants: null,
  });
  {
    // **The happy path: layout and module resolve, and the pipeline builds — with
    // NO workgroup-size member on the descriptor.** WebGPU reads the workgroup
    // size from the module's `@workgroup_size` and `GPUComputePipelineDescriptor`
    // has no member for it, so the replayer drops the field it carried; the
    // `compute` stage the device is handed is exactly `{ module, entryPoint }`,
    // its two resolved objects, and the two ids resolved out of two different
    // tables.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(fullShader, 400n));
    replayer.replay(
      frameOf(emptyPipelineLayoutAt(computePipeline.layout), 401n)
    );
    replayer.replay(frameOf(computePipeline, 402n));
    const pd = device.createdComputePipelines[0];
    const built =
      device.createdComputePipelines.length === 1 &&
      pd.label === 'cull' &&
      pd.layout === replayer.pipelineLayouts.get(computePipeline.layout) &&
      pd.compute.module ===
        replayer.shaderModules.get(computePipeline.module) &&
      pd.compute.entryPoint === computePipeline.entryPoint &&
      // The compute stage is its two members and nothing else — no workgroup size
      // smuggled inside it.
      Object.keys(pd.compute).sort().join(',') === 'entryPoint,module';
    check(
      built,
      built
        ? 'a CreateComputePipeline reaches the device with its layout and module resolved out of two different tables'
        : `the built descriptor is not what was asked for (${JSON.stringify(pd, (_, v) => (typeof v === 'bigint' ? String(v) : v))})`
    );
    // **The workgroup size is carried in Rust but not passed to WebGPU.** Asserted
    // on the descriptor built for the browser, at the top level and inside the
    // stage, because `GPUComputePipelineDescriptor` has no member for it — dropping
    // it changes nothing, unlike dropping a push-constant range.
    const dropped =
      !('workgroupSize' in pd) &&
      !('workgroup_size' in pd) &&
      !('workgroupSize' in pd.compute) &&
      !('workgroup_size' in pd.compute);
    check(
      dropped,
      dropped
        ? 'the workgroup size is carried on the wire and dropped rather than passed to createComputePipeline'
        : `a workgroup-size member reached the descriptor (${JSON.stringify(pd, (_, v) => (typeof v === 'bigint' ? String(v) : v))})`
    );
    check(
      replayer.computePipelines.get(computePipeline.pipeline) !== undefined &&
        !replayer.hasReplies &&
        replayer.pendingErrors === 0 &&
        replayer.takeError() === null,
      `and the pipeline lands in its table, queues no reply and reports no error (queued ${replayer.hasReplies})`
    );
  }
  {
    // **An unresolvable layout goes to the error queue naming the layout, and an
    // unresolvable module names the module — two DISTINCT messages.** This is the
    // whole reason the pair of tables is worth stating: a stale layout and a stale
    // module must not read the same in the log. Neither throws. The module is
    // built for the layout case and the layout for the module case, so each isolates
    // the one handle that could not be resolved.
    const layoutCase = await readyWithDevice();
    layoutCase.replayer.replay(frameOf(fullShader, 410n));
    let layoutThrew = null;
    let layoutReason = null;
    try {
      layoutCase.replayer.replay(frameOf(computePipeline, 411n));
      layoutReason = layoutCase.replayer.takeError();
    } catch (error) {
      layoutThrew = error;
    }

    const moduleCase = await readyWithDevice();
    moduleCase.replayer.replay(
      frameOf(emptyPipelineLayoutAt(computePipeline.layout), 420n)
    );
    let moduleThrew = null;
    let moduleReason = null;
    try {
      moduleCase.replayer.replay(frameOf(computePipeline, 421n));
      moduleReason = moduleCase.replayer.takeError();
    } catch (error) {
      moduleThrew = error;
    }

    const distinct =
      layoutThrew === null &&
      moduleThrew === null &&
      layoutCase.device.createdComputePipelines.length === 0 &&
      moduleCase.device.createdComputePipelines.length === 0 &&
      layoutCase.replayer.computePipelines.size === 0 &&
      moduleCase.replayer.computePipelines.size === 0 &&
      String(layoutReason).includes('pipeline layout') &&
      String(layoutReason).includes('as its layout') &&
      !String(layoutReason).includes('shader module') &&
      String(moduleReason).includes('shader module') &&
      String(moduleReason).includes('as its compute stage') &&
      !String(moduleReason).includes('pipeline layout') &&
      String(layoutReason) !== String(moduleReason);
    check(
      distinct,
      distinct
        ? 'an unresolvable layout and an unresolvable module each go to the error queue naming which, in two distinct messages'
        : `the two failures are not distinct (${JSON.stringify(layoutThrew ?? moduleThrew)}, layout ${JSON.stringify(layoutReason)}, module ${JSON.stringify(moduleReason)})`
    );
  }
  {
    // **A compute pipeline before any device is refused, like every device
    // method** — recorded rather than thrown, for the buffer pair's reason.
    const bare = new Replayer({ gpu: stubGpu(async () => null) });
    bare.replay(frameOf(computePipeline, 430n));
    const early = bare.takeError();
    check(
      typeof early === 'string' && early.includes('before any device opened'),
      `a compute pipeline before a device is a device error, not a throw (${JSON.stringify(early)})`
    );
  }
  {
    // **Destroys: a stale generation is a no-op that leaves the live occupant, and
    // an empty slot does not throw** — the ordinary case here, since every pipeline
    // the replayer refuses above leaves its handle empty for the caller to destroy
    // anyway.
    const { replayer } = await readyWithDevice();
    replayer.replay(frameOf(fullShader, 440n));
    replayer.replay(
      frameOf(emptyPipelineLayoutAt(computePipeline.layout), 441n)
    );
    replayer.replay(frameOf(computePipeline, 442n));
    const live = replayer.computePipelines.get(computePipeline.pipeline);
    const stale = handle(
      computePipeline.pipeline.index,
      computePipeline.pipeline.generation + 1
    );
    let thrown = null;
    try {
      replayer.replay(
        frameOf({ ...destroyComputePipeline, pipeline: stale }, 443n)
      );
      replayer.replay(frameOf(destroyComputePipeline, 444n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        live !== undefined &&
        replayer.computePipelines.get(computePipeline.pipeline) === live,
      `a stale-generation destroy leaves the live compute pipeline and an empty-slot destroy does not throw (${String(thrown)})`
    );
    replayer.replay(
      frameOf(
        { ...destroyComputePipeline, pipeline: computePipeline.pipeline },
        445n
      )
    );
    check(
      replayer.computePipelines.get(computePipeline.pipeline) === undefined,
      'and destroying the pipeline by its own handle releases it'
    );
  }

  // ---- the graphics pipeline: the largest descriptor on the seam ----------
  //
  // The corpus `CreateGraphicsPipeline` names `fullShader`'s module for its
  // vertex stage, `emptyWgslShader`'s for its fragment stage, and a pipeline
  // layout at its own `layout` handle — three ids into two tables — and its depth
  // format is `D32_FLOAT_S8_UINT`, which is gated behind `depth32float-stencil8`.
  // So the frame that builds it opens a device with that feature and creates the
  // two shaders and the layout first.
  const rasterReady = () =>
    readyWithDevice({ device: stubDevice(['depth32float-stencil8']) });
  const rasterFrame = (replayer) => {
    replayer.replay(frameOf(fullShader, 500n));
    replayer.replay(frameOf(emptyWgslShader, 501n));
    replayer.replay(
      frameOf(emptyPipelineLayoutAt(graphicsPipeline.layout), 502n)
    );
  };
  {
    // **The happy path: three handles resolve and the pipeline builds.** The
    // descriptor the device is handed is what proves the imperfect mappings: the
    // vertex stage carries an empty `buffers` array (vertex pulling has no
    // vertex-buffer layout), the fragment is present with its two targets, the
    // depth-stencil carries `stencilFront`/`stencilBack` but no `stencilReference`
    // — that is a per-pass value, dropped like the workgroup size — and the
    // multisample count is the 4 the corpus asked for.
    const { replayer, device } = await rasterReady();
    rasterFrame(replayer);
    replayer.replay(frameOf(graphicsPipeline, 503n));
    const pd = device.createdRenderPipelines[0];
    const built =
      device.createdRenderPipelines.length === 1 &&
      pd.label === 'gbuffer' &&
      pd.layout === replayer.pipelineLayouts.get(graphicsPipeline.layout) &&
      pd.vertex.module ===
        replayer.shaderModules.get(graphicsPipeline.vertexModule) &&
      pd.vertex.entryPoint === 'vertexMain' &&
      Array.isArray(pd.vertex.buffers) &&
      pd.vertex.buffers.length === 0 &&
      pd.fragment.module ===
        replayer.shaderModules.get(graphicsPipeline.fragment.module) &&
      pd.fragment.entryPoint === 'fragmentMain' &&
      pd.fragment.targets.length === 2 &&
      pd.multisample.count === 4;
    check(
      built,
      built
        ? 'a CreateGraphicsPipeline reaches the device with its layout and two shader stages resolved, and vertex.buffers empty'
        : `the built render pipeline is not what was asked for (${JSON.stringify(pd, (_, v) => (typeof v === 'bigint' ? String(v) : v))})`
    );
    // **The stencil reference is carried in Rust but not passed to WebGPU** — it
    // belongs to the pass. Asserted on the descriptor: the depth-stencil has
    // stencil faces but no `stencilReference` and no bare `reference` anywhere.
    const referenceDropped =
      pd.depthStencil !== undefined &&
      pd.depthStencil.stencilFront !== undefined &&
      pd.depthStencil.stencilBack !== undefined &&
      !('stencilReference' in pd.depthStencil) &&
      !('reference' in pd.depthStencil) &&
      !('reference' in pd.depthStencil.stencilFront);
    check(
      referenceDropped,
      referenceDropped
        ? 'the stencil reference is carried on the wire and dropped rather than passed to createRenderPipeline (it is a per-pass value)'
        : `a stencil reference reached the descriptor (${JSON.stringify(pd.depthStencil)})`
    );
    check(
      replayer.graphicsPipelines.get(graphicsPipeline.pipeline) !== undefined &&
        !replayer.hasReplies &&
        replayer.pendingErrors === 0 &&
        replayer.takeError() === null,
      `and the render pipeline lands in its table, queues no reply and reports no error (queued ${replayer.hasReplies})`
    );
  }
  {
    // **Three handle misses, three DISTINCT messages** — the layout, the vertex
    // module, and the fragment module. Each case builds everything but the one
    // handle it is isolating, so the message names exactly which could not be
    // resolved and none reads like another.
    const layoutCase = await rasterReady();
    layoutCase.replayer.replay(frameOf(fullShader, 510n));
    layoutCase.replayer.replay(frameOf(emptyWgslShader, 511n));
    layoutCase.replayer.replay(frameOf(graphicsPipeline, 512n));
    const layoutReason = layoutCase.replayer.takeError();

    const vertexCase = await rasterReady();
    vertexCase.replayer.replay(frameOf(emptyWgslShader, 520n));
    vertexCase.replayer.replay(
      frameOf(emptyPipelineLayoutAt(graphicsPipeline.layout), 521n)
    );
    vertexCase.replayer.replay(frameOf(graphicsPipeline, 522n));
    const vertexReason = vertexCase.replayer.takeError();

    const fragmentCase = await rasterReady();
    fragmentCase.replayer.replay(frameOf(fullShader, 530n));
    fragmentCase.replayer.replay(
      frameOf(emptyPipelineLayoutAt(graphicsPipeline.layout), 531n)
    );
    fragmentCase.replayer.replay(frameOf(graphicsPipeline, 532n));
    const fragmentReason = fragmentCase.replayer.takeError();

    const distinct =
      layoutCase.device.createdRenderPipelines.length === 0 &&
      vertexCase.device.createdRenderPipelines.length === 0 &&
      fragmentCase.device.createdRenderPipelines.length === 0 &&
      String(layoutReason).includes('as its layout') &&
      String(vertexReason).includes('as its vertex stage') &&
      String(fragmentReason).includes('as its fragment stage') &&
      new Set([
        String(layoutReason),
        String(vertexReason),
        String(fragmentReason),
      ]).size === 3;
    check(
      distinct,
      distinct
        ? 'an unresolvable layout, vertex module and fragment module each go to the error queue naming which, in three distinct messages'
        : `the three failures are not distinct (layout ${JSON.stringify(layoutReason)}, vertex ${JSON.stringify(vertexReason)}, fragment ${JSON.stringify(fragmentReason)})`
    );
  }
  {
    // **Every "WebGPU cannot express it" refusal fires and is named**, each on a
    // device where the pipeline would otherwise build. Nothing throws; each is a
    // device error, and nothing is filed.
    const cases = [
      [
        {
          ...graphicsPipeline,
          primitive: { ...graphicsPipeline.primitive, polygonMode: 'Line' },
        },
        'PolygonMode::Line',
        () => rasterReady(),
      ],
      [
        {
          ...graphicsPipeline,
          primitive: { ...graphicsPipeline.primitive, depthClamp: true },
        },
        'depth-clip-control',
        () => rasterReady(),
      ],
      [
        {
          ...graphicsPipeline,
          multisample: { ...graphicsPipeline.multisample, samples: 3 },
        },
        'must be 1 or 4',
        () => rasterReady(),
      ],
      [
        {
          ...graphicsPipeline,
          depthStencil: {
            ...graphicsPipeline.depthStencil,
            bias: { ...graphicsPipeline.depthStencil.bias, constant: 1.5 },
          },
        },
        'GPUDepthBias is an integer',
        () => rasterReady(),
      ],
      // The depth format is gated, so the same command is a refusal on a device
      // without the feature — the pipeline the happy path built.
      [graphicsPipeline, 'depth32float-stencil8', () => readyWithDevice()],
    ];
    /** @type {string[]} */
    const wrong = [];
    for (const [command, needle, ready] of cases) {
      const { replayer, device } = await ready();
      // Build the two shaders and the layout so the refusal is about the field
      // and not a missing handle.
      replayer.replay(frameOf(fullShader, 540n));
      replayer.replay(frameOf(emptyWgslShader, 541n));
      replayer.replay(
        frameOf(emptyPipelineLayoutAt(graphicsPipeline.layout), 542n)
      );
      let thrown = null;
      try {
        replayer.replay(frameOf(command, 543n));
      } catch (error) {
        thrown = error;
      }
      const reason = replayer.takeError();
      if (
        thrown !== null ||
        device.createdRenderPipelines.length !== 0 ||
        replayer.graphicsPipelines.size !== 0 ||
        !String(reason).includes(needle)
      ) {
        wrong.push(
          `${needle}: thrown ${String(thrown)}, reason ${JSON.stringify(reason)}`
        );
      }
    }
    check(
      wrong.length === 0,
      wrong[0] ??
        'PolygonMode::Line, depth_clamp without the feature, a bad sample count, a fractional depthBias and a gated depth format are each refused by name, none thrown'
    );
  }
  {
    // **The gated depth format builds on a device that DID enable the feature** —
    // the half that keeps the refusal above honest, so it is refusing what the
    // device lacks rather than everything.
    const { replayer, device } = await rasterReady();
    rasterFrame(replayer);
    replayer.replay(frameOf(graphicsPipeline, 550n));
    check(
      device.createdRenderPipelines.length === 1 &&
        replayer.graphicsPipelines.get(graphicsPipeline.pipeline) !== undefined,
      'the D32_FLOAT_S8_UINT depth format builds on a device that enabled depth32float-stencil8'
    );
  }
  {
    // **A depth-only pass: fragment None omits the whole GPUFragmentState.** The
    // vertex stage still resolves; the descriptor carries no `fragment` member.
    const depthOnly = { ...graphicsPipeline, fragment: null, colorTargets: [] };
    const { replayer, device } = await rasterReady();
    replayer.replay(frameOf(fullShader, 560n));
    replayer.replay(
      frameOf(emptyPipelineLayoutAt(graphicsPipeline.layout), 561n)
    );
    replayer.replay(frameOf(depthOnly, 562n));
    const pd = device.createdRenderPipelines[0];
    check(
      pd !== undefined &&
        !('fragment' in pd) &&
        replayer.graphicsPipelines.get(graphicsPipeline.pipeline) !==
          undefined &&
        replayer.takeError() === null,
      `a fragment: null pipeline omits the GPUFragmentState member entirely (${pd ? JSON.stringify(Object.keys(pd)) : 'not built'})`
    );
  }
  {
    // **A graphics pipeline before any device is refused, like every device
    // method** — recorded rather than thrown.
    const bare = new Replayer({ gpu: stubGpu(async () => null) });
    bare.replay(frameOf(graphicsPipeline, 570n));
    const early = bare.takeError();
    check(
      typeof early === 'string' && early.includes('before any device opened'),
      `a graphics pipeline before a device is a device error, not a throw (${JSON.stringify(early)})`
    );
  }
  {
    // **Destroys: a stale generation leaves the live occupant, and an empty slot
    // does not throw** — the ordinary case here, since every refused pipeline
    // leaves its handle empty for the caller to destroy anyway.
    const { replayer } = await rasterReady();
    rasterFrame(replayer);
    replayer.replay(frameOf(graphicsPipeline, 580n));
    const live = replayer.graphicsPipelines.get(graphicsPipeline.pipeline);
    const stale = handle(
      graphicsPipeline.pipeline.index,
      graphicsPipeline.pipeline.generation + 1
    );
    let thrown = null;
    try {
      replayer.replay(
        frameOf({ ...destroyGraphicsPipeline, pipeline: stale }, 581n)
      );
      replayer.replay(frameOf(destroyGraphicsPipeline, 582n));
    } catch (error) {
      thrown = error;
    }
    check(
      thrown === null &&
        live !== undefined &&
        replayer.graphicsPipelines.get(graphicsPipeline.pipeline) === live,
      `a stale-generation destroy leaves the live graphics pipeline and an empty-slot destroy does not throw (${String(thrown)})`
    );
    replayer.replay(
      frameOf(
        { ...destroyGraphicsPipeline, pipeline: graphicsPipeline.pipeline },
        583n
      )
    );
    check(
      replayer.graphicsPipelines.get(graphicsPipeline.pipeline) === undefined,
      'and destroying the render pipeline by its own handle releases it'
    );
  }

  // ---- the draw arms: a pipeline bound and a triangle drawn into a pass ----
  //
  // `BindGraphicsPipeline`, `BindGroup` and `Draw` record onto the render pass
  // the encoder opened, so a check reads the call the replayer made on the pass
  // (spied by `stubRenderPass`). None answers a reply and none has a browser
  // object to read back, so a malformed one — no pass open, an unresolvable
  // handle — leaves a line on the device's error queue instead of throwing.
  // `PushConstants` is refused outright, since WebGPU has none.
  const encoderCommand = {
    name: 'CreateCommandEncoder',
    label: null,
    queue: handle(1, 1),
  };
  const openPass = {
    name: 'BeginRenderPass',
    label: null,
    colorAttachments: [],
    depthStencilAttachment: null,
    renderArea: { x: 0, y: 0, width: 1, height: 1 },
  };
  /**
   * Opens an encoder and a render pass on `replayer` and hands back the pass
   * the device recorded, so a check can read the calls made on it.
   *
   * @param {Replayer} replayer
   * @param {{ createdEncoders: object[] }} device
   */
  const openedPass = (replayer, device) => {
    replayer.replay(frameOf(encoderCommand, 600n));
    replayer.replay(frameOf(openPass, 601n));
    return device.createdEncoders.at(-1).pass;
  };
  const drawOf = (vertices, instances) => ({
    name: 'Draw',
    vertices,
    instances,
  });
  {
    // **A draw's two ranges become WebGPU's four counts.** `0..3` vertices and
    // `0..1` instances is three vertices of one instance, both from the origin.
    const { replayer, device } = await readyWithDevice();
    const pass = openedPass(replayer, device);
    replayer.replay(
      frameOf(drawOf({ start: 0, end: 3 }, { start: 0, end: 1 }), 602n)
    );
    checkEqual(
      pass.draws,
      [{ vertexCount: 3, instanceCount: 1, firstVertex: 0, firstInstance: 0 }],
      'a Draw records draw(vertexCount, instanceCount, firstVertex, firstInstance) from its ranges'
    );
    check(
      replayer.pendingErrors === 0,
      'a valid draw leaves the error queue empty'
    );
  }
  {
    // **Non-zero starts prove the spans are widths, not ends.** `2..5`/`1..3`
    // is three vertices and two instances, from vertex 2 and instance 1 — the
    // case that fails if the arm passed the range ends through as counts.
    const { replayer, device } = await readyWithDevice();
    const pass = openedPass(replayer, device);
    replayer.replay(
      frameOf(drawOf({ start: 2, end: 5 }, { start: 1, end: 3 }), 602n)
    );
    checkEqual(
      pass.draws,
      [{ vertexCount: 3, instanceCount: 2, firstVertex: 2, firstInstance: 1 }],
      'a Draw with non-zero range starts carries the widths and the firsts through'
    );
  }
  {
    // **A draw with no pass open is a mid-frame ordering fault** — the error
    // queue, not a throw that would take the rest of the frame down.
    const { replayer } = await readyWithDevice();
    replayer.replay(
      frameOf(drawOf({ start: 0, end: 3 }, { start: 0, end: 1 }), 602n)
    );
    const error = replayer.takeError();
    check(
      error !== null && error.includes('no render pass open'),
      `a draw with no pass open goes to the error queue (${JSON.stringify(error)})`
    );
  }
  {
    // **A bound pipeline reaches the pass as the resolved GPURenderPipeline.**
    // The corpus pipeline is built first, exactly as the section above does, so
    // its handle resolves; the pass then gets that same object.
    const { replayer, device } = await rasterReady();
    rasterFrame(replayer);
    replayer.replay(frameOf(graphicsPipeline, 503n));
    const pipeline = replayer.graphicsPipelines.get(graphicsPipeline.pipeline);
    const pass = openedPass(replayer, device);
    replayer.replay(
      frameOf(
        { name: 'BindGraphicsPipeline', pipeline: graphicsPipeline.pipeline },
        602n
      )
    );
    check(
      pass.setPipelines.length === 1 &&
        pass.setPipelines[0] === pipeline &&
        replayer.pendingErrors === 0,
      'BindGraphicsPipeline sets the resolved pipeline on the pass and queues no error'
    );
  }
  {
    // **An unresolvable pipeline handle is named on the error queue**, as
    // `index.generation`, and nothing is set on the pass.
    const { replayer, device } = await readyWithDevice();
    const pass = openedPass(replayer, device);
    replayer.replay(
      frameOf({ name: 'BindGraphicsPipeline', pipeline: handle(9, 4) }, 602n)
    );
    const error = replayer.takeError();
    check(
      pass.setPipelines.length === 0 && error !== null && error.includes('9.4'),
      `an unresolvable pipeline handle goes to the error queue naming it (${JSON.stringify(error)})`
    );
  }
  {
    // **Binding a pipeline with no pass open is a mid-frame ordering fault.**
    const { replayer } = await readyWithDevice();
    replayer.replay(
      frameOf({ name: 'BindGraphicsPipeline', pipeline: handle(1, 1) }, 602n)
    );
    const error = replayer.takeError();
    check(
      error !== null && error.includes('no render pass open'),
      `binding a pipeline with no pass open goes to the error queue (${JSON.stringify(error)})`
    );
  }
  {
    // **A bound group reaches the pass with its slot and dynamic offsets.** The
    // fixture's material group is built against resources at the handles it
    // names — the bind-group section's own setup — so its handle resolves.
    const { replayer, device } = await readyWithDevice();
    const bufA = materialGroup.entries[0].resource.buffer;
    const bufB = materialGroup.entries[3].resource.buffer;
    const view = materialGroup.entries[1].resource.view;
    const sampler = materialGroup.entries[2].resource.sampler;
    const image = handle(220, 1);
    replayer.replay(frameOf(emptyLayoutAt(materialGroup.layout), 200n));
    replayer.replay(frameOf(storageBufferAt(bufA), 201n));
    replayer.replay(frameOf(storageBufferAt(bufB), 202n));
    replayer.replay(frameOf(sampledImageAt(image), 203n));
    replayer.replay(frameOf(viewOf(view, image), 204n));
    replayer.replay(frameOf(samplerAt(sampler), 205n));
    replayer.replay(frameOf(materialGroup, 206n));
    const group = replayer.bindGroups.get(materialGroup.group);
    const pass = openedPass(replayer, device);
    replayer.replay(
      frameOf(
        {
          name: 'BindGroup',
          slot: 3,
          group: materialGroup.group,
          dynamicOffsets: [16, 32],
          layout: handle(7, 7),
        },
        602n
      )
    );
    check(
      pass.setBindGroups.length === 1 &&
        pass.setBindGroups[0].slot === 3 &&
        pass.setBindGroups[0].group === group &&
        replayer.pendingErrors === 0,
      'BindGroup sets the resolved group on its slot and queues no error'
    );
    checkEqual(
      pass.setBindGroups[0].dynamicOffsets,
      [16, 32],
      'and the dynamic offsets reach setBindGroup as given'
    );
  }
  {
    // **An unresolvable group handle is named on the error queue.**
    const { replayer, device } = await readyWithDevice();
    const pass = openedPass(replayer, device);
    replayer.replay(
      frameOf(
        {
          name: 'BindGroup',
          slot: 0,
          group: handle(5, 2),
          dynamicOffsets: [],
          layout: handle(7, 7),
        },
        602n
      )
    );
    const error = replayer.takeError();
    check(
      pass.setBindGroups.length === 0 &&
        error !== null &&
        error.includes('5.2'),
      `an unresolvable group handle goes to the error queue naming it (${JSON.stringify(error)})`
    );
  }
  {
    // **Binding a group with NEITHER pass open is a mid-frame ordering fault.**
    // The arm is generalized to bind onto whichever pass is open, so its refusal
    // names both.
    const { replayer } = await readyWithDevice();
    replayer.replay(
      frameOf(
        {
          name: 'BindGroup',
          slot: 0,
          group: handle(1, 1),
          dynamicOffsets: [],
          layout: handle(7, 7),
        },
        602n
      )
    );
    const error = replayer.takeError();
    check(
      error !== null && error.includes('no render or compute pass open'),
      `binding a group with no pass open goes to the error queue (${JSON.stringify(error)})`
    );
  }
  {
    // **PushConstants is refused outright and does not throw.** WebGPU has no
    // push constants, so a range never survives `createPipelineLayout` and a
    // valid stream never reaches this arm; a hand-written one is named here
    // rather than falling to the dispatch's throwing default.
    const { replayer } = await readyWithDevice();
    let thrown = null;
    try {
      replayer.replay(
        frameOf(
          {
            name: 'PushConstants',
            stages: ['VERTEX', 'FRAGMENT'],
            offset: 16,
            data: new Uint8Array([0xde, 0xad, 0xbe, 0xef]),
            layout: handle(1, 1),
          },
          602n
        )
      );
    } catch (error) {
      thrown = error;
    }
    const error = replayer.takeError();
    check(
      thrown === null && error !== null && error.includes('no push constants'),
      `PushConstants is refused on the error queue without throwing (threw ${String(thrown)}, said ${JSON.stringify(error)})`
    );
  }

  // ---- the compute-pass arms: a pipeline bound and a dispatch recorded -----
  //
  // `BeginComputePass`, `BindComputePipeline`, `Dispatch` and `EndComputePass`
  // record onto the compute pass the encoder opened (spied by
  // `stubComputePass`); `BindGroup` binds onto WHICHEVER pass is open — here the
  // compute one, which is what proves the generalized arm; and
  // `CopyBufferToBuffer` records onto the encoder itself. None answers a reply,
  // so a malformed one — no pass open, an unresolvable handle — leaves a line on
  // the device's error queue instead of throwing.
  const beginComputePass = { name: 'BeginComputePass', label: null };
  /**
   * Opens an encoder and a compute pass on `replayer` and hands back the compute
   * pass the device recorded, so a check can read the calls made on it.
   *
   * @param {Replayer} replayer
   * @param {{ createdEncoders: object[] }} device
   */
  const openedComputePass = (replayer, device) => {
    replayer.replay(frameOf(encoderCommand, 700n));
    replayer.replay(frameOf(beginComputePass, 701n));
    return device.createdEncoders.at(-1).computePass;
  };
  {
    // **A compute pass opens on the encoder and is kept for the arms that
    // follow.** The device records the descriptor and hands the pass back.
    const { replayer, device } = await readyWithDevice();
    const pass = openedComputePass(replayer, device);
    check(
      pass !== null &&
        pass !== undefined &&
        device.createdEncoders.at(-1).beganComputePasses.length === 1 &&
        replayer.pendingErrors === 0,
      'BeginComputePass opens a compute pass on the encoder and queues no error'
    );
  }
  {
    // **A compute pass with no encoder open is a mid-frame ordering fault** — the
    // error queue, not a throw.
    const { replayer } = await readyWithDevice();
    replayer.replay(frameOf(beginComputePass, 701n));
    const error = replayer.takeError();
    check(
      error !== null && error.includes('no command encoder open'),
      `a compute pass begun with no encoder goes to the error queue (${JSON.stringify(error)})`
    );
  }
  {
    // **A bound compute pipeline reaches the pass as the resolved
    // GPUComputePipeline.** The corpus compute pipeline is built first — its
    // shader and layout, then the pipeline — so its handle resolves; the pass
    // then gets that same object.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(fullShader, 690n));
    replayer.replay(
      frameOf(emptyPipelineLayoutAt(computePipeline.layout), 691n)
    );
    replayer.replay(frameOf(computePipeline, 692n));
    const pipeline = replayer.computePipelines.get(computePipeline.pipeline);
    const pass = openedComputePass(replayer, device);
    replayer.replay(
      frameOf(
        { name: 'BindComputePipeline', pipeline: computePipeline.pipeline },
        702n
      )
    );
    check(
      pass.setPipelines.length === 1 &&
        pass.setPipelines[0] === pipeline &&
        replayer.pendingErrors === 0,
      'BindComputePipeline sets the resolved pipeline on the compute pass and queues no error'
    );
  }
  {
    // **An unresolvable compute pipeline handle is named on the error queue.**
    const { replayer, device } = await readyWithDevice();
    const pass = openedComputePass(replayer, device);
    replayer.replay(
      frameOf({ name: 'BindComputePipeline', pipeline: handle(9, 4) }, 702n)
    );
    const error = replayer.takeError();
    check(
      pass.setPipelines.length === 0 && error !== null && error.includes('9.4'),
      `an unresolvable compute pipeline handle goes to the error queue naming it (${JSON.stringify(error)})`
    );
  }
  {
    // **Binding a compute pipeline with no compute pass open is a mid-frame
    // ordering fault.**
    const { replayer } = await readyWithDevice();
    replayer.replay(
      frameOf({ name: 'BindComputePipeline', pipeline: handle(1, 1) }, 702n)
    );
    const error = replayer.takeError();
    check(
      error !== null && error.includes('no compute pass open'),
      `binding a compute pipeline with no pass open goes to the error queue (${JSON.stringify(error)})`
    );
  }
  {
    // **A dispatch's three counts reach dispatchWorkgroups unchanged.**
    const { replayer, device } = await readyWithDevice();
    const pass = openedComputePass(replayer, device);
    replayer.replay(frameOf({ name: 'Dispatch', x: 4, y: 3, z: 2 }, 702n));
    checkEqual(
      pass.dispatches,
      [{ x: 4, y: 3, z: 2 }],
      'a Dispatch records dispatchWorkgroups(x, y, z)'
    );
    check(
      replayer.pendingErrors === 0,
      'a valid dispatch leaves the error queue empty'
    );
  }
  {
    // **A dispatch with no compute pass open is a mid-frame ordering fault.**
    const { replayer } = await readyWithDevice();
    replayer.replay(frameOf({ name: 'Dispatch', x: 1, y: 1, z: 1 }, 702n));
    const error = replayer.takeError();
    check(
      error !== null && error.includes('no compute pass open'),
      `a dispatch with no compute pass open goes to the error queue (${JSON.stringify(error)})`
    );
  }
  {
    // **A bind group binds onto the OPEN COMPUTE pass** — the whole point of the
    // generalized arm, which used to guard on a render pass alone. The fixture's
    // material group is built against resources at the handles it names so its
    // handle resolves, then a compute pass is opened and the group bound onto it.
    const { replayer, device } = await readyWithDevice();
    const bufA = materialGroup.entries[0].resource.buffer;
    const bufB = materialGroup.entries[3].resource.buffer;
    const view = materialGroup.entries[1].resource.view;
    const sampler = materialGroup.entries[2].resource.sampler;
    const image = handle(220, 1);
    replayer.replay(frameOf(emptyLayoutAt(materialGroup.layout), 200n));
    replayer.replay(frameOf(storageBufferAt(bufA), 201n));
    replayer.replay(frameOf(storageBufferAt(bufB), 202n));
    replayer.replay(frameOf(sampledImageAt(image), 203n));
    replayer.replay(frameOf(viewOf(view, image), 204n));
    replayer.replay(frameOf(samplerAt(sampler), 205n));
    replayer.replay(frameOf(materialGroup, 206n));
    const group = replayer.bindGroups.get(materialGroup.group);
    const pass = openedComputePass(replayer, device);
    replayer.replay(
      frameOf(
        {
          name: 'BindGroup',
          slot: 1,
          group: materialGroup.group,
          dynamicOffsets: [8],
          layout: handle(7, 7),
        },
        702n
      )
    );
    check(
      pass.setBindGroups.length === 1 &&
        pass.setBindGroups[0].slot === 1 &&
        pass.setBindGroups[0].group === group &&
        replayer.pendingErrors === 0,
      'BindGroup binds onto the open compute pass — the generalized arm reaches both'
    );
    checkEqual(
      pass.setBindGroups[0].dynamicOffsets,
      [8],
      'and the dynamic offsets reach the compute pass setBindGroup as given'
    );
  }
  {
    // **Ending a compute pass with none open is a malformed stream**, into the
    // error queue rather than thrown.
    const { replayer } = await readyWithDevice();
    replayer.replay(frameOf({ name: 'EndComputePass' }, 702n));
    const error = replayer.takeError();
    check(
      error !== null && error.includes('with none open'),
      `ending a compute pass with none open goes to the error queue (${JSON.stringify(error)})`
    );
  }
  {
    // **A buffer→buffer copy reaches the encoder with both buffers resolved and
    // the u64 offsets and size narrowed to Number.** Two buffers are built at the
    // handles the copy names, then an encoder is opened and the copy recorded.
    const { replayer, device } = await readyWithDevice();
    const srcH = handle(80, 1);
    const dstH = handle(81, 1);
    replayer.replay(frameOf(storageBufferAt(srcH), 700n));
    replayer.replay(frameOf(storageBufferAt(dstH), 701n));
    const src = replayer.buffers.get(srcH);
    const dst = replayer.buffers.get(dstH);
    replayer.replay(frameOf(encoderCommand, 702n));
    replayer.replay(
      frameOf(
        {
          name: 'CopyBufferToBuffer',
          copy: {
            src: srcH,
            srcOffset: 16n,
            dst: dstH,
            dstOffset: 32n,
            size: 64n,
          },
        },
        703n
      )
    );
    const copies = device.createdEncoders.at(-1).bufferCopies;
    check(
      copies.length === 1 &&
        copies[0].src === src &&
        copies[0].dst === dst &&
        copies[0].srcOffset === 16 &&
        copies[0].dstOffset === 32 &&
        copies[0].size === 64 &&
        replayer.pendingErrors === 0,
      'CopyBufferToBuffer records copyBufferToBuffer(src, srcOffset, dst, dstOffset, size) with resolved buffers'
    );
  }
  {
    // **An unresolvable source or destination is named on the error queue — two
    // DISTINCT messages.** The destination is built for the source case and the
    // source for the destination case, so each isolates the handle that did not
    // resolve.
    const copyOf = (src, dst) => ({
      name: 'CopyBufferToBuffer',
      copy: { src, srcOffset: 0n, dst, dstOffset: 0n, size: 4n },
    });
    const srcCase = await readyWithDevice();
    srcCase.replayer.replay(frameOf(storageBufferAt(handle(81, 1)), 700n));
    srcCase.replayer.replay(frameOf(encoderCommand, 701n));
    srcCase.replayer.replay(
      frameOf(copyOf(handle(80, 1), handle(81, 1)), 702n)
    );
    const srcError = srcCase.replayer.takeError();

    const dstCase = await readyWithDevice();
    dstCase.replayer.replay(frameOf(storageBufferAt(handle(80, 1)), 700n));
    dstCase.replayer.replay(frameOf(encoderCommand, 701n));
    dstCase.replayer.replay(
      frameOf(copyOf(handle(80, 1), handle(81, 1)), 702n)
    );
    const dstError = dstCase.replayer.takeError();
    check(
      srcError !== null &&
        srcError.includes('80.1') &&
        srcError.includes('from') &&
        dstError !== null &&
        dstError.includes('81.1') &&
        dstError.includes('into'),
      `an unresolvable copy source names it (from) and a destination names it (into), distinctly (${JSON.stringify([srcError, dstError])})`
    );
  }
  {
    // **A buffer copy with no encoder open is a mid-frame ordering fault** — the
    // error queue, not a throw, and refused before either buffer is resolved.
    const { replayer } = await readyWithDevice();
    replayer.replay(
      frameOf(
        {
          name: 'CopyBufferToBuffer',
          copy: {
            src: handle(80, 1),
            srcOffset: 0n,
            dst: handle(81, 1),
            dstOffset: 0n,
            size: 4n,
          },
        },
        702n
      )
    );
    const error = replayer.takeError();
    check(
      error !== null && error.includes('no command encoder open'),
      `a buffer copy with no encoder goes to the error queue (${JSON.stringify(error)})`
    );
  }

  // ---- the remaining copies and the buffer fill ----------------------------
  //
  // `CopyBufferToImage` and `CopyImageToImage` record onto the encoder itself
  // (spied by `bufferToImageCopies` / `imageCopies`), and `FillBuffer` maps a
  // zero fill to `clearBuffer` (spied by `bufferFills`) and refuses any other
  // value to the error queue — the same judgement `crcbl-wgpu` makes. None
  // answers a reply, so a malformed one leaves a line on the device's error
  // queue instead of throwing.
  {
    // **A buffer→image copy reaches the encoder with buffer and image resolved,
    // the texel pitch turned into bytesPerRow, and the arguments in
    // copyBufferToTexture's order (source = buffer layout, destination =
    // texture) — the reverse of copyTextureToBuffer.** `bufferRowLength` is 0
    // here, so the row is the copy width (4 texels × 4 bytes = 16).
    const { replayer, device } = await readyWithDevice();
    const bufH = handle(80, 1);
    const imgH = handle(90, 1);
    replayer.replay(frameOf(storageBufferAt(bufH), 700n));
    replayer.replay(frameOf(sampledImageAt(imgH), 701n));
    const buffer = replayer.buffers.get(bufH);
    const image = replayer.images.get(imgH);
    replayer.replay(frameOf(encoderCommand, 702n));
    replayer.replay(
      frameOf(
        {
          name: 'CopyBufferToImage',
          buffer: bufH,
          bufferOffset: 8n,
          bufferRowLength: 0,
          bufferImageHeight: 0,
          image: imgH,
          imageSubresource: {
            aspect: ['COLOR'],
            mip: 0,
            baseLayer: 0,
            layerCount: 1,
          },
          imageOffset: { x: 2, y: 3, z: 0 },
          imageExtent: { width: 4, height: 4, depthOrLayers: 1 },
        },
        703n
      )
    );
    const copies = device.createdEncoders.at(-1).bufferToImageCopies;
    check(
      copies.length === 1 &&
        copies[0].source.buffer === buffer &&
        copies[0].source.offset === 8 &&
        copies[0].source.bytesPerRow === 16 &&
        copies[0].destination.texture === image &&
        copies[0].destination.origin.x === 2 &&
        copies[0].destination.origin.y === 3 &&
        copies[0].size.width === 4 &&
        copies[0].size.height === 4 &&
        replayer.pendingErrors === 0,
      'CopyBufferToImage records copyBufferToTexture(bufferLayout, textureView, size) with resolved handles'
    );
  }
  {
    // **An unresolvable source buffer or destination image is named on the error
    // queue — two DISTINCT messages.** The image is built for the buffer case and
    // the buffer for the image case, so each isolates the handle that did not
    // resolve.
    const copyOf = (buffer, image) => ({
      name: 'CopyBufferToImage',
      buffer,
      bufferOffset: 0n,
      bufferRowLength: 0,
      bufferImageHeight: 0,
      image,
      imageSubresource: {
        aspect: ['COLOR'],
        mip: 0,
        baseLayer: 0,
        layerCount: 1,
      },
      imageOffset: { x: 0, y: 0, z: 0 },
      imageExtent: { width: 4, height: 4, depthOrLayers: 1 },
    });
    const bufCase = await readyWithDevice();
    bufCase.replayer.replay(frameOf(sampledImageAt(handle(90, 1)), 700n));
    bufCase.replayer.replay(frameOf(encoderCommand, 701n));
    bufCase.replayer.replay(
      frameOf(copyOf(handle(80, 1), handle(90, 1)), 702n)
    );
    const bufError = bufCase.replayer.takeError();

    const imgCase = await readyWithDevice();
    imgCase.replayer.replay(frameOf(storageBufferAt(handle(80, 1)), 700n));
    imgCase.replayer.replay(frameOf(encoderCommand, 701n));
    imgCase.replayer.replay(
      frameOf(copyOf(handle(80, 1), handle(90, 1)), 702n)
    );
    const imgError = imgCase.replayer.takeError();
    check(
      bufError !== null &&
        bufError.includes('80.1') &&
        bufError.includes('from buffer') &&
        imgError !== null &&
        imgError.includes('90.1') &&
        imgError.includes('image'),
      `an unresolvable buffer→image source names the buffer and destination names the image, distinctly (${JSON.stringify([bufError, imgError])})`
    );
  }
  {
    // **A buffer→image copy with no encoder open is a mid-frame ordering fault.**
    const { replayer } = await readyWithDevice();
    replayer.replay(
      frameOf(
        {
          name: 'CopyBufferToImage',
          buffer: handle(80, 1),
          bufferOffset: 0n,
          bufferRowLength: 0,
          bufferImageHeight: 0,
          image: handle(90, 1),
          imageSubresource: {
            aspect: ['COLOR'],
            mip: 0,
            baseLayer: 0,
            layerCount: 1,
          },
          imageOffset: { x: 0, y: 0, z: 0 },
          imageExtent: { width: 4, height: 4, depthOrLayers: 1 },
        },
        702n
      )
    );
    const error = replayer.takeError();
    check(
      error !== null && error.includes('no command encoder open'),
      `a buffer→image copy with no encoder goes to the error queue (${JSON.stringify(error)})`
    );
  }
  {
    // **An image→image copy reaches the encoder with both images resolved and the
    // two sides' mip levels, origins and the shared extent passed through.** The
    // two sides differ in mip and origin so a source/destination transposition is
    // visible.
    const { replayer, device } = await readyWithDevice();
    const srcH = handle(90, 1);
    const dstH = handle(91, 1);
    replayer.replay(frameOf(sampledImageAt(srcH), 700n));
    replayer.replay(frameOf(sampledImageAt(dstH), 701n));
    const src = replayer.images.get(srcH);
    const dst = replayer.images.get(dstH);
    replayer.replay(frameOf(encoderCommand, 702n));
    replayer.replay(
      frameOf(
        {
          name: 'CopyImageToImage',
          copy: {
            src: srcH,
            srcSubresource: {
              aspect: ['COLOR'],
              mip: 0,
              baseLayer: 0,
              layerCount: 1,
            },
            srcOffset: { x: 1, y: 2, z: 0 },
            dst: dstH,
            dstSubresource: {
              aspect: ['COLOR'],
              mip: 0,
              baseLayer: 0,
              layerCount: 1,
            },
            dstOffset: { x: 3, y: 4, z: 0 },
            extent: { width: 2, height: 2, depthOrLayers: 1 },
          },
        },
        703n
      )
    );
    const copies = device.createdEncoders.at(-1).imageCopies;
    check(
      copies.length === 1 &&
        copies[0].source.texture === src &&
        copies[0].source.origin.x === 1 &&
        copies[0].source.origin.y === 2 &&
        copies[0].destination.texture === dst &&
        copies[0].destination.origin.x === 3 &&
        copies[0].destination.origin.y === 4 &&
        copies[0].size.width === 2 &&
        copies[0].size.height === 2 &&
        replayer.pendingErrors === 0,
      'CopyImageToImage records copyTextureToTexture(src, dst, extent) with resolved images'
    );
  }
  {
    // **An unresolvable source or destination image is named on the error queue —
    // two DISTINCT messages.**
    const copyOf = (src, dst) => ({
      name: 'CopyImageToImage',
      copy: {
        src,
        srcSubresource: {
          aspect: ['COLOR'],
          mip: 0,
          baseLayer: 0,
          layerCount: 1,
        },
        srcOffset: { x: 0, y: 0, z: 0 },
        dst,
        dstSubresource: {
          aspect: ['COLOR'],
          mip: 0,
          baseLayer: 0,
          layerCount: 1,
        },
        dstOffset: { x: 0, y: 0, z: 0 },
        extent: { width: 2, height: 2, depthOrLayers: 1 },
      },
    });
    const srcCase = await readyWithDevice();
    srcCase.replayer.replay(frameOf(sampledImageAt(handle(91, 1)), 700n));
    srcCase.replayer.replay(frameOf(encoderCommand, 701n));
    srcCase.replayer.replay(
      frameOf(copyOf(handle(90, 1), handle(91, 1)), 702n)
    );
    const srcError = srcCase.replayer.takeError();

    const dstCase = await readyWithDevice();
    dstCase.replayer.replay(frameOf(sampledImageAt(handle(90, 1)), 700n));
    dstCase.replayer.replay(frameOf(encoderCommand, 701n));
    dstCase.replayer.replay(
      frameOf(copyOf(handle(90, 1), handle(91, 1)), 702n)
    );
    const dstError = dstCase.replayer.takeError();
    check(
      srcError !== null &&
        srcError.includes('90.1') &&
        srcError.includes('from') &&
        dstError !== null &&
        dstError.includes('91.1') &&
        dstError.includes('into'),
      `an unresolvable image copy source names it (from) and a destination names it (into), distinctly (${JSON.stringify([srcError, dstError])})`
    );
  }
  {
    // **An image→image copy with no encoder open is a mid-frame ordering fault.**
    const { replayer } = await readyWithDevice();
    replayer.replay(
      frameOf(
        {
          name: 'CopyImageToImage',
          copy: {
            src: handle(90, 1),
            srcSubresource: {
              aspect: ['COLOR'],
              mip: 0,
              baseLayer: 0,
              layerCount: 1,
            },
            srcOffset: { x: 0, y: 0, z: 0 },
            dst: handle(91, 1),
            dstSubresource: {
              aspect: ['COLOR'],
              mip: 0,
              baseLayer: 0,
              layerCount: 1,
            },
            dstOffset: { x: 0, y: 0, z: 0 },
            extent: { width: 2, height: 2, depthOrLayers: 1 },
          },
        },
        702n
      )
    );
    const error = replayer.takeError();
    check(
      error !== null && error.includes('no command encoder open'),
      `an image→image copy with no encoder goes to the error queue (${JSON.stringify(error)})`
    );
  }
  {
    // **A zero fill maps to clearBuffer(buffer, offset, size) with the buffer
    // resolved and the u64 offset and size narrowed to Number.**
    const { replayer, device } = await readyWithDevice();
    const bufH = handle(80, 1);
    replayer.replay(frameOf(storageBufferAt(bufH), 700n));
    const buffer = replayer.buffers.get(bufH);
    replayer.replay(frameOf(encoderCommand, 701n));
    replayer.replay(
      frameOf(
        { name: 'FillBuffer', buffer: bufH, offset: 16n, size: 64n, value: 0 },
        702n
      )
    );
    const fills = device.createdEncoders.at(-1).bufferFills;
    check(
      fills.length === 1 &&
        fills[0].buffer === buffer &&
        fills[0].offset === 16 &&
        fills[0].size === 64 &&
        replayer.pendingErrors === 0,
      'FillBuffer with value 0 records clearBuffer(buffer, offset, size) with the buffer resolved'
    );
  }
  {
    // **A non-zero fill is refused to the error queue and records NO clearBuffer**
    // — WebGPU's clearBuffer is zero-only, the same refusal crcbl-wgpu makes. The
    // buffer is built so the refusal is the value's, not an unresolvable handle.
    const { replayer, device } = await readyWithDevice();
    const bufH = handle(80, 1);
    replayer.replay(frameOf(storageBufferAt(bufH), 700n));
    replayer.replay(frameOf(encoderCommand, 701n));
    replayer.replay(
      frameOf(
        {
          name: 'FillBuffer',
          buffer: bufH,
          offset: 0n,
          size: 64n,
          value: 0xdeadbeef,
        },
        702n
      )
    );
    const error = replayer.takeError();
    check(
      error !== null &&
        error.includes('non-zero') &&
        error.includes('clearBuffer') &&
        device.createdEncoders.at(-1).bufferFills.length === 0,
      `a non-zero fill is refused and records no clearBuffer (${JSON.stringify(error)})`
    );
  }
  {
    // **A fill with no encoder open is a mid-frame ordering fault.**
    const { replayer } = await readyWithDevice();
    replayer.replay(
      frameOf(
        {
          name: 'FillBuffer',
          buffer: handle(80, 1),
          offset: 0n,
          size: 4n,
          value: 0,
        },
        702n
      )
    );
    const error = replayer.takeError();
    check(
      error !== null && error.includes('no command encoder open'),
      `a fill with no encoder goes to the error queue (${JSON.stringify(error)})`
    );
  }
  {
    // **A zero fill of an unresolvable buffer is named on the error queue.** The
    // encoder is open, so the refusal is the buffer's.
    const { replayer } = await readyWithDevice();
    replayer.replay(frameOf(encoderCommand, 701n));
    replayer.replay(
      frameOf(
        {
          name: 'FillBuffer',
          buffer: handle(80, 1),
          offset: 0n,
          size: 4n,
          value: 0,
        },
        702n
      )
    );
    const error = replayer.takeError();
    check(
      error !== null && error.includes('80.1'),
      `a fill of an unresolvable buffer names it on the error queue (${JSON.stringify(error)})`
    );
  }

  // ---- the pipeline barrier: the documented no-op --------------------------
  //
  // `PipelineBarrier` is recognised (not the dispatch `default` throw) and does
  // NOTHING: WebGPU tracks resource state itself, so there is no transition for
  // the replayer to record. The evidence for a no-op is negative — no method
  // reaches the encoder or its passes, no reply is queued, and no line lands on
  // the device's error queue — so that is what these two gates read back. The
  // populated barrier carries a non-empty buffer AND image list (with a `Some`
  // queue transfer) precisely so a replay that quietly acted on them would show;
  // the empty/global one pins the other shape.
  {
    // **A populated barrier, mid-frame after an encoder is open, touches
    // nothing.** The encoder's every spy array stays empty, so no `copy*`,
    // `clearBuffer`, `beginRenderPass` or `beginComputePass` was called; no reply
    // is in flight and no error is queued; and the replay did not throw.
    const { replayer, device } = await readyWithDevice();
    replayer.replay(frameOf(encoderCommand, 700n));
    let threw = null;
    try {
      replayer.replay(
        frameOf(
          {
            name: 'PipelineBarrier',
            buffers: [
              {
                buffer: handle(80, 1),
                from: 'ShaderWrite',
                to: 'TransferSrc',
                queueTransfer: { from: handle(82, 1), to: handle(83, 1) },
              },
            ],
            images: [
              {
                image: handle(90, 1),
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
          701n
        )
      );
    } catch (caught) {
      threw = caught;
    }
    const encoder = device.createdEncoders.at(-1);
    const untouched =
      encoder.beganPasses.length === 0 &&
      encoder.beganComputePasses.length === 0 &&
      encoder.bufferCopies.length === 0 &&
      encoder.bufferToImageCopies.length === 0 &&
      encoder.imageCopies.length === 0 &&
      encoder.bufferFills.length === 0;
    check(
      threw === null &&
        untouched &&
        !replayer.hasReplies &&
        replayer.inFlight === 0 &&
        replayer.pendingErrors === 0 &&
        replayer.takeError() === null,
      `a populated pipeline barrier records nothing on the encoder, queues no reply, and reports no error (threw ${String(threw)}, errors ${replayer.pendingErrors})`
    );
  }
  {
    // **A global-only barrier with empty lists is the same no-op**, exercised
    // even with no encoder open — a barrier with nothing to record is still
    // nothing to do, so it does not reach the error queue the copies would.
    const { replayer } = await readyWithDevice();
    let threw = null;
    try {
      replayer.replay(
        frameOf(
          { name: 'PipelineBarrier', buffers: [], images: [], global: true },
          700n
        )
      );
    } catch (caught) {
      threw = caught;
    }
    check(
      threw === null &&
        !replayer.hasReplies &&
        replayer.inFlight === 0 &&
        replayer.pendingErrors === 0 &&
        replayer.takeError() === null,
      `a global-only pipeline barrier is a silent no-op with no encoder open (threw ${String(threw)}, errors ${replayer.pendingErrors})`
    );
  }

  // ---- the ShaderStages → GPUShaderStage mapping, bit by bit ---------------
  //
  // Spelled out rather than compared against the table it is testing, for the
  // usage mappings' reason. Every bit on its own and then together, so one wired
  // to the wrong constant names itself instead of hiding behind a union.
  {
    const wrong = [];
    for (const [name, bit] of [
      ['VERTEX', 0x1],
      ['FRAGMENT', 0x2],
      ['COMPUTE', 0x4],
    ]) {
      const answer = webgpuShaderStageFor([name]);
      if (answer.bits !== bit || answer.unsatisfiable.length > 0) {
        wrong.push(`${name}: ${JSON.stringify(answer)}`);
      }
    }
    const all = webgpuShaderStageFor(['VERTEX', 'FRAGMENT', 'COMPUTE']);
    check(
      wrong.length === 0 && all.bits === 0x7,
      wrong[0] ??
        `every ShaderStages bit maps to its own GPUShaderStage and they or together (0x${all.bits.toString(16)})`
    );
    // …and the two with no bit at all are reported rather than dropped, which is
    // what makes the union above evidence: a mapping that answered 0 for
    // everything would satisfy neither half.
    const missing = webgpuShaderStageFor(['VERTEX', 'MESH', 'TASK']);
    check(
      missing.bits === 0x1 &&
        missing.unsatisfiable.join(',') ===
          'ShaderStages::MESH,ShaderStages::TASK',
      `MESH and TASK are reported as unsatisfiable rather than dropped (${JSON.stringify(missing)})`
    );
    // An empty visibility is a real value — `ShaderStages::empty()` — and is not
    // a refusal here. WebGPU validates a zero `visibility` itself, which is the
    // duplicate-binding judgement applied to a second rule.
    checkEqual(
      webgpuShaderStageFor([]),
      { bits: 0, unsatisfiable: [] },
      'an empty visibility is passed on for the browser to refuse'
    );
  }

  // ---- the BindingKind → GPUBindGroupLayoutEntry mapping, row by row -------
  //
  // Which *member* each variant becomes, and what goes in it. Spelled out for
  // the mappings above's reason, and every boolean both ways: `read_only` is a
  // buffer `type` rather than a flag beside one, and `comparison` is a sampler
  // `type`, so a translation that inverted either produces a layout WebGPU
  // accepts and then refuses every bind group against.
  {
    const rows = [
      [
        { name: 'UniformBuffer', dynamic: false },
        { buffer: { type: 'uniform', hasDynamicOffset: false } },
      ],
      [
        { name: 'UniformBuffer', dynamic: true },
        { buffer: { type: 'uniform', hasDynamicOffset: true } },
      ],
      [
        { name: 'StorageBuffer', readOnly: true, dynamic: false },
        { buffer: { type: 'read-only-storage', hasDynamicOffset: false } },
      ],
      [
        { name: 'StorageBuffer', readOnly: false, dynamic: true },
        { buffer: { type: 'storage', hasDynamicOffset: true } },
      ],
      [
        { name: 'Sampler', comparison: false },
        { sampler: { type: 'filtering' } },
      ],
      [
        { name: 'Sampler', comparison: true },
        { sampler: { type: 'comparison' } },
      ],
      [
        { name: 'SampledImage', viewType: 'D2', sampleType: 'Float' },
        {
          texture: {
            sampleType: 'float',
            viewDimension: '2d',
            multisampled: false,
          },
        },
      ],
      [
        { name: 'SampledImage', viewType: 'CubeArray', sampleType: 'Depth' },
        {
          texture: {
            sampleType: 'depth',
            viewDimension: 'cube-array',
            multisampled: false,
          },
        },
      ],
    ];
    checkEqual(
      rows.map(([kind]) => webgpuBindingLayoutFor(kind).layout),
      rows.map(([, expected]) => expected),
      'every BindingKind WebGPU can express becomes the member it names'
    );
    // …and the three it cannot: the variant with no format, a view dimension no
    // table claims, and a sample type no table claims. Each is refused with the
    // reason named rather than defaulted, for `webgpuTextureFormatFor`'s reason.
    const refusals = [
      [{ name: 'StorageImage', readOnly: false }, 'format'],
      [
        { name: 'SampledImage', viewType: 'D4', sampleType: 'Float' },
        'GPUTextureViewDimension',
      ],
      [
        { name: 'SampledImage', viewType: 'D2', sampleType: 'Sint' },
        'GPUTextureSampleType',
      ],
      [{ name: 'ExternalTexture' }, 'no GPUBindGroupLayoutEntry member'],
    ].map(([kind, phrase]) => {
      const answer = webgpuBindingLayoutFor(kind);
      return answer.layout === null && String(answer.reason).includes(phrase)
        ? null
        : `${kind.name}: ${JSON.stringify(answer)}`;
    });
    check(
      refusals.every((wrong) => wrong === null),
      refusals.find((wrong) => wrong !== null) ??
        'and every BindingKind WebGPU cannot express is refused with the reason named'
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
