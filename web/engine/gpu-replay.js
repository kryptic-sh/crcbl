// The replayer: decoded commands in, WebGPU calls out, replies back.
//
// `gpu-stream.js` turns a frame of wasm's bytes into command objects and
// `gpu-reply.js` turns answers back into bytes; this is what sits between them
// and actually calls the browser. `gpu-transport.js` is what moves either
// buffer across the wasm seam, and this file knows nothing about that — it takes
// what `takeCommandStream` returned and produces what `putReplyStream` sends,
// so it can be driven under node against a stub `navigator.gpu` with no wasm
// instance anywhere. `web/tools/gpu-replay.mjs` does exactly that.
//
// TWO OF THE COMMANDS IT REPLAYS ASK THE BROWSER SOMETHING. `EnumerateAdapters`
// calls `navigator.gpu.requestAdapter()` and answers with the whole of the
// seam's `AdapterInfo` — the browser's name for it, its features and its limits
// in the seam's vocabulary, and the documented absence for the four fields
// WebGPU has no answer for — or with the reason there is none. `RequestDevice`
// calls `requestDevice()` on the adapter that enumeration granted and answers
// with the **device's own** `DeviceCaps`, or with the reason there is no device.
// The translation is the block below this header and is where every one of
// those choices is argued.
//
// THE FEATURE MAPPING RUNS BOTH WAYS AND IS ONE TABLE. `halFeaturesFor` turns a
// browser's `GPUFeatureName`s into `crcbl_hal::Features` bits; `webgpuFeaturesFor`
// turns a `DeviceDesc`'s bits back into the names `requestDevice` wants. Both
// read `FEATURE_MAP` and `CORE_FEATURES`, so the pair cannot drift from each
// other — and the direction that matters for correctness is the new one:
// `requiredFeatures` fails the *whole* request if it names something the adapter
// lacks, and a HAL flag with no WebGPU name at all can never be satisfied. Such
// a flag therefore fails the request loudly rather than being dropped from the
// list, because a device that opened without something the caller declared it
// could not run without is the one outcome `required` exists to prevent.
//
// AND TWO SURFACE COMMANDS, WHICH MAKE NO ROUND TRIP AT ALL. `CreateSurface`
// looks its `canvasId` up in the canvas registry this replayer was handed —
// `SurfaceTarget::Web` is that key and nothing else — asks the canvas for its
// `webgpu` context and holds it against the surface handle; `DestroySurface`
// drops it again. Neither queues a reply, because neither has one on this
// channel: wasm allocated the handle itself and moved on, so there is nothing
// waiting to be answered. Two things about them are decisions rather than
// omissions, and each is argued where it is made: the context is deliberately
// **not** configured, and a lookup that finds no canvas throws.
//
// Every other command in the stream is *unimplemented*, and says so: `replay`
// throws a `ReplayError` naming the command and the sequence that carried it,
// rather than skipping it. A skipped command is a draw that never happened and a
// frame that renders almost right, which is the hardest kind of bug to see; a
// throw names the opcode the day it first arrives. Later slices fill them in.
//
// ASYNCHRONY AGAINST A SYNCHRONOUS REPLAY, WHICH IS THE WHOLE PROBLEM HERE.
// WebGPU's adapter API is a promise and the stream is replayed once per frame,
// synchronously, at the `requestAnimationFrame` boundary — the frame cannot wait
// and must not be blocked. So `replay` returns as soon as it has *started* the
// work, and the answer is queued into this replayer's `ReplyWriter` whenever the
// promise settles. It goes to wasm on whichever frame comes next, named by the
// sequence of the command that asked, which is exactly what the sequence number
// is for: a reply's position says nothing, so three frames later still names a
// command nothing else has been.
//
// Two consequences worth stating rather than discovering:
//
//   * A frame that replays an enumeration produces NO reply on that frame. The
//     caller must be willing to ask again — `hasReplies` is false and that is
//     the ordinary case, not a failure.
//   * A reply is queued exactly once per command, on every path. A promise that
//     rejected, a browser with no `navigator.gpu` at all, an adapter that
//     resolved `null`: each answers, because a dropped reply is a command wasm
//     waits on for ever.
//
// SEQUENCE NUMBERS ARE POSITIONAL ON THE WAY IN. Nothing per command is on the
// wire: the nth command's number is the buffer's `baseSequence` plus n, and it
// is computed here rather than carried, which is why `replay` takes the base
// alongside the list rather than the list alone.

import { DEVICE_TYPE, ReplyWriter } from './gpu-reply.js';

// ─────────────────────────────────────────────────────────────────────────────
// WebGPU's vocabulary, in the seam's
// ─────────────────────────────────────────────────────────────────────────────
//
// THIS IS THE ONE PLACE THE TRANSLATION HAPPENS, and it lives here rather than
// in Rust on purpose. This file is the WebGPU-facing half, so it is the half
// that should own "what a `GPUFeatureName` means in `crcbl_hal::Features`" —
// exactly as `crates/crcbl-wgpu/src/instance.rs` owns it for wgpu's enum. The
// wire then speaks the seam's vocabulary end to end, as it already does for
// load ops, store ops and handles, instead of carrying one foreign spelling.
//
// THE MAPPING IS LOSSY IN BOTH DIRECTIONS AND BOTH LISTS ARE WRITTEN OUT BELOW.
// A reader needs to know which HAL flags can never be set here, so that their
// absence is not read as a browser that declined; and which WebGPU features are
// dropped, so that nobody looks for their effect on the far side.
//
// IT REPORTS WHAT THE BROWSER SAID, NOT WHAT `crcbl-webgpu` CAN EXECUTE. There
// is no `create_query_set` command yet, so a page on a browser with
// `timestamp-query` sends `TIMESTAMP_QUERY` while nothing in the stream could
// serve it. That is right while there is no device — the channel's job is to
// carry the browser's answer intact — and it stops being right the day an
// `impl Instance` exists, because a capability on the seam is a promise about
// what a caller may *ask for*. That impl must intersect this set with what the
// stream can encode; `crates/crcbl-webgpu/src/instance.rs` says so too.

/**
 * `crcbl_hal::Features` bits that core WebGPU grants unconditionally.
 *
 * No `GPUFeatureName` gates any of them — every WebGPU implementation has them
 * or is not a WebGPU implementation:
 *
 *   * `COMPUTE` (1 << 8) — compute pipelines are core.
 *   * `TIMELINE_SEMAPHORE`? No. See the never-set list below.
 *   * `OCCLUSION_QUERY` (1 << 7) — `GPURenderPassDescriptor.occlusionQuerySet`
 *     is core; only *timestamp* queries need a feature.
 *   * `DEPTH_BIAS_CLAMP` (1 << 14) — `GPUDepthStencilState.depthBiasClamp` is
 *     core and honoured.
 *   * `DEBUG_MARKERS` (1 << 18) — `pushDebugGroup` and `insertDebugMarker` are
 *     core on every encoder and reach the browser's own capture tooling.
 */
const CORE_FEATURES = (1n << 8n) | (1n << 7n) | (1n << 14n) | (1n << 18n); // COMPUTE | OCCLUSION_QUERY | DEPTH_BIAS_CLAMP | DEBUG_MARKERS

/**
 * Every `GPUFeatureName` that has a `crcbl_hal::Features` bit, and the bit.
 *
 * Iterated rather than searched, so a feature name this table does not list is
 * dropped by construction and no browser can widen the set by adding one.
 *
 * WHAT IS DROPPED, and it is most of the spec's list: `core-features-and-limits`,
 * `depth32float-stencil8`, `texture-compression-bc-sliced-3d`,
 * `texture-compression-etc2`, `texture-compression-astc`,
 * `texture-compression-astc-sliced-3d`, `shader-f16`, `rg11b10ufloat-renderable`,
 * `bgra8unorm-storage`, `float32-filterable`, `float32-blendable`,
 * `clip-distances`, `dual-source-blending` and `subgroups`. The seam has no flag
 * for any of them — most are format or shading-language capabilities, which
 * `crcbl_hal::Features` does not model at all — so a device with them reports
 * the same set as a device without, and anything wanting them will need a HAL
 * flag first.
 */
const FEATURE_MAP = Object.freeze({
  /** `Features::DEPTH_CLAMP` (1 << 13). WebGPU's `unclippedDepth` is depth
   * clamping under another name, and it is the mapping `crcbl-wgpu` makes for
   * wgpu's `DEPTH_CLIP_CONTROL`. */
  'depth-clip-control': 1n << 13n,
  /** `Features::TEXTURE_COMPRESSION_BC` (1 << 16). */
  'texture-compression-bc': 1n << 16n,
  /** `Features::TIMESTAMP_QUERY` (1 << 5). */
  'timestamp-query': 1n << 5n,
  /** `Features::INDIRECT_FIRST_INSTANCE` (1 << 4). Core WebGPU requires
   * `firstInstance` to be zero in an indirect draw; this is the feature that
   * lifts it. */
  'indirect-first-instance': 1n << 4n,
});

// WHAT CAN NEVER BE SET, and why — the other half of the lossiness, spelled out
// so an absence here is read as "WebGPU has no such thing" rather than as "this
// browser declined". Nineteen of the seam's twenty-seven flags:
//
//   DESCRIPTOR_INDEXING, BUFFER_DEVICE_ADDRESS   WebGPU has no bindless model
//                                                and no raw GPU pointers.
//   DRAW_INDIRECT_COUNT, MULTI_DRAW_INDIRECT     `drawIndirect` emits exactly
//                                                one draw and takes no count
//                                                buffer.
//   PIPELINE_STATISTICS_QUERY                    only occlusion and timestamp
//                                                queries exist.
//   TIMELINE_SEMAPHORE                           WebGPU has no semaphores at
//                                                all; `onSubmittedWorkDone` is
//                                                a promise, not a counter.
//   ASYNC_COMPUTE_QUEUE, TRANSFER_QUEUE          one queue, and no way to ask
//                                                for another.
//   PUSH_CONSTANTS                               WGSL cannot express one.
//   POLYGON_MODE_LINE                            no fill mode in
//                                                `GPUPrimitiveState`.
//   SAMPLER_ANISOTROPY                           `maxAnisotropy` is accepted
//                                                but nothing reports a
//                                                *ceiling*, and `Limits` is
//                                                documented as what the backend
//                                                guarantees. See `limitsFor`.
//   SHADER_DEBUG_PRINTF                          no GPU-side print.
//   PRESENT_FEEDBACK, PRESENT_TIMING             the page learns nothing about
//                                                when a frame was shown beyond
//                                                `requestAnimationFrame`.
//   MESH_SHADER, TASK_SHADER                     no mesh pipeline.
//   RAY_QUERY, RAY_TRACING_PIPELINE,             no ray tracing of any kind.
//   ACCELERATION_STRUCTURE

/**
 * `crcbl_hal::Limits`, as `gpu-reply.js` wants it.
 *
 * @typedef {object} HalLimits
 * @property {number} maxImage2d
 * @property {number} maxImage3d
 * @property {number} maxImageArrayLayers
 * @property {bigint} maxStorageBufferRange
 * @property {bigint} maxUniformBufferRange
 * @property {number} maxBindGroups
 * @property {number} maxBindlessDescriptors
 * @property {number} maxPushConstantSize
 * @property {number} maxColorAttachments
 * @property {number} maxSampleCount
 * @property {number} maxDrawIndirectCount
 * @property {number[]} maxComputeWorkgroupSize
 * @property {number} maxComputeInvocationsPerWorkgroup
 * @property {number} maxComputeWorkgroupsPerDimension
 * @property {bigint} minUniformBufferOffsetAlignment
 * @property {bigint} minStorageBufferOffsetAlignment
 * @property {bigint} optimalBufferCopyOffsetAlignment
 * @property {number} maxSamplerAnisotropy
 * @property {number} timestampPeriodNs
 */

/**
 * `crcbl_hal::AdapterInfo` without `backend`, which never crosses the wire.
 *
 * @typedef {object} HalAdapterInfo
 * @property {number} id
 * @property {string} name
 * @property {number} vendorId
 * @property {number} deviceId
 * @property {number} deviceType One of `DEVICE_TYPE`.
 * @property {string} driver
 * @property {bigint} features `crcbl_hal::Features` bits.
 * @property {HalLimits} limits
 */

/**
 * WebGPU's guaranteed `bytesPerRow` alignment for a buffer↔texture copy.
 *
 * A constant in the specification rather than a limit anything reports, which is
 * why it is written here: `GPUImageDataLayout.bytesPerRow` must be a multiple of
 * 256, and no `GPUSupportedLimits` member says so. `crcbl-wgpu` reports the same
 * number from wgpu's `COPY_BYTES_PER_ROW_ALIGNMENT`.
 */
const COPY_BYTES_PER_ROW_ALIGNMENT = 256n;

/**
 * The sample counts WebGPU guarantees, as a ceiling.
 *
 * `GPUTextureDescriptor.sampleCount` is specified to be exactly `1` or `4` and
 * there is no limit to read a larger one from, so this is the spec's number and
 * not a guess about the hardware.
 */
const MAX_SAMPLE_COUNT = 4;

/**
 * The `crcbl_hal::Features` bits a `GPUAdapter`'s or `GPUDevice`'s own
 * `features` amount to.
 *
 * BOTH KINDS, AND THE DIFFERENCE IS NOT COSMETIC. `GPUSupportedFeatures` is the
 * same shape on either, but the *sets* differ: an adapter reports what it could
 * grant and a device reports what was actually asked for and given. Reading a
 * device's caps off its adapter is how a renderer ends up selecting a path
 * against a feature the device does not have.
 *
 * @param {{ features?: ReadonlySet<string> }} source An adapter or a device.
 * @returns {bigint}
 */
export function halFeaturesFor(source) {
  let bits = CORE_FEATURES;
  const features = source.features;
  if (features) {
    for (const [name, bit] of Object.entries(FEATURE_MAP)) {
      if (features.has(name)) bits |= bit;
    }
  }
  return bits;
}

/**
 * The inverse: which `GPUFeatureName`s a `crcbl_hal::Features` word asks for,
 * and which of its bits nothing in WebGPU can ever satisfy.
 *
 * THE SAME TABLE READ BACKWARDS, deliberately — two independently written
 * mappings would agree until the day one of them was edited. Three kinds of bit
 * come out of it:
 *
 *   * bits in `CORE_FEATURES`, which core WebGPU grants with no name behind
 *     them, so they need nothing in `requiredFeatures` and are dropped here;
 *   * bits in `FEATURE_MAP`, which become their `GPUFeatureName`;
 *   * everything else — nineteen of the seam's twenty-seven flags, listed above
 *     — which come back in `unsatisfiable`.
 *
 * The caller decides what that means, and the two words in a `DeviceDesc` decide
 * it differently: for `required_features` an unsatisfiable bit fails the
 * request, and for `optional_features` it is simply not asked for.
 *
 * @param {bigint} bits
 * @returns {{ names: string[], unsatisfiable: bigint }}
 */
export function webgpuFeaturesFor(bits) {
  // Core first: those are granted whether or not anybody names them, so they
  // are satisfied rather than unsatisfiable.
  let left = bits & ~CORE_FEATURES;
  const names = [];
  for (const [name, bit] of Object.entries(FEATURE_MAP)) {
    if ((left & bit) !== 0n) {
      names.push(name);
      left &= ~bit;
    }
  }
  return { names, unsatisfiable: left };
}

/**
 * The `crcbl_hal::Features` bits a list of `GPUFeatureName`s stands for.
 *
 * `webgpuFeaturesFor` the other way round again, for the one case that needs it:
 * a required feature whose name exists but whose *adapter* does not have it. The
 * refusal has to name the same bits the request did, so that both kinds of
 * "unsupported" reach the far side in the same shape.
 *
 * @param {readonly string[]} names
 * @returns {bigint}
 */
function featuresFromNames(names) {
  let bits = 0n;
  for (const name of names) bits |= FEATURE_MAP[name] ?? 0n;
  return bits;
}

/**
 * The bits set in `word`, as the `1 << n` indices `crcbl-hal` declares them
 * with.
 *
 * For a message a person reads. The flag *names* are `crcbl_hal::Features`'s
 * and stay there: a twenty-seven row copy of them here would be a second thing
 * to keep in step for the sake of one string, and the reply carries the word
 * itself so the Rust side can print the names.
 *
 * @param {bigint} word
 * @returns {number[]}
 */
function featureBitIndices(word) {
  const bits = [];
  for (let bit = 0n; bit < 64n; bit += 1n) {
    if ((word >> bit) & 1n) bits.push(Number(bit));
  }
  return bits;
}

/**
 * A `GPUAdapter`'s or `GPUDevice`'s `limits` in the seam's names and units.
 *
 * BOTH KINDS, for `halFeaturesFor`'s reason and with a sharper consequence: an
 * adapter's limits are the ceilings it *could* grant, and a device's are the
 * ones it was created with — the specification's defaults unless the request
 * asked for more. The two differ on most real hardware, and reporting the
 * adapter's for a device would promise a texture size the device will refuse.
 *
 * Four of the nineteen have no `GPUSupportedLimits` member behind them, and each
 * is a value the specification fixes rather than a number invented to fill a
 * gap: two are `0` because the feature they bound is absent — which is what
 * `crcbl_hal::Limits` documents `max_bindless_descriptors` and
 * `max_push_constant_size` to mean — and two are spec constants named above.
 * `max_draw_indirect_count` is `1` for the same reason: without
 * `DRAW_INDIRECT_COUNT`, one indirect call emits one draw.
 *
 * `max_sampler_anisotropy` is `1.0`, the floor, and deliberately not `16`.
 * WebGPU accepts a larger `maxAnisotropy` and clamps it to whatever the device
 * does, and reports that number nowhere — so there is no ceiling this backend
 * can *guarantee*, which is what a `Limits` field is. `SAMPLER_ANISOTROPY` is
 * withheld to match: a feature saying "anisotropy, see the limit for the cap"
 * beside a cap of 1 would be a contradiction.
 *
 * @param {{ limits?: object, features?: ReadonlySet<string> }} source An
 *   adapter or a device.
 * @returns {HalLimits}
 */
export function halLimitsFor(source) {
  const limits = source.limits ?? {};
  const timestamps = Boolean(source.features?.has('timestamp-query'));
  /**
   * One `GPUSupportedLimits` member as the `BigInt` the seam wants, naming it
   * if the browser had no number there. `BigInt(undefined)` throws a `TypeError`
   * that says only "cannot convert", which in a promise callback three frames
   * from the call is nothing to go on.
   *
   * @param {unknown} value
   * @param {string} name
   */
  const big = (value, name) => {
    if (typeof value !== 'number' || !Number.isInteger(value)) {
      throw new TypeError(
        `the adapter reported no integer ${name}: ${String(value)}`
      );
    }
    return BigInt(value);
  };
  return {
    maxImage2d: limits.maxTextureDimension2D,
    maxImage3d: limits.maxTextureDimension3D,
    maxImageArrayLayers: limits.maxTextureArrayLayers,
    maxStorageBufferRange: big(
      limits.maxStorageBufferBindingSize,
      'maxStorageBufferBindingSize'
    ),
    maxUniformBufferRange: big(
      limits.maxUniformBufferBindingSize,
      'maxUniformBufferBindingSize'
    ),
    maxBindGroups: limits.maxBindGroups,
    // No bindless model, so the count `Limits` documents for its absence.
    maxBindlessDescriptors: 0,
    // WGSL has no push constants, so likewise.
    maxPushConstantSize: 0,
    maxColorAttachments: limits.maxColorAttachments,
    maxSampleCount: MAX_SAMPLE_COUNT,
    maxDrawIndirectCount: 1,
    maxComputeWorkgroupSize: [
      limits.maxComputeWorkgroupSizeX,
      limits.maxComputeWorkgroupSizeY,
      limits.maxComputeWorkgroupSizeZ,
    ],
    maxComputeInvocationsPerWorkgroup: limits.maxComputeInvocationsPerWorkgroup,
    maxComputeWorkgroupsPerDimension: limits.maxComputeWorkgroupsPerDimension,
    minUniformBufferOffsetAlignment: big(
      limits.minUniformBufferOffsetAlignment,
      'minUniformBufferOffsetAlignment'
    ),
    minStorageBufferOffsetAlignment: big(
      limits.minStorageBufferOffsetAlignment,
      'minStorageBufferOffsetAlignment'
    ),
    optimalBufferCopyOffsetAlignment: COPY_BYTES_PER_ROW_ALIGNMENT,
    maxSamplerAnisotropy: 1,
    // WebGPU's timestamps are specified in nanoseconds, so one tick is one
    // nanosecond — and `0` is what `Limits` documents for a device with no
    // timestamp queries to have a period for.
    timestampPeriodNs: timestamps ? 1 : 0,
  };
}

/**
 * Everything the seam wants to know about an adapter, from everything WebGPU
 * will say about one.
 *
 * FOUR FIELDS HAVE NO WEBGPU ANSWER AT ALL, and each gets the value that means
 * absent rather than one that looks real. `GPUAdapterInfo` is four strings —
 * `vendor`, `architecture`, `device`, `description` — and nothing else:
 *
 *   * `vendorId` and `deviceId` are `0`, which `crcbl_hal::AdapterInfo`
 *     documents as "unknown". `info.vendor` is a *name* like `"apple"`; turning
 *     one into a PCI id would be an invention the far side could not tell from a
 *     real id.
 *   * `deviceType` is `OTHER` — "the backend declined to say". WebGPU does not
 *     report discrete versus integrated. `adapter.isFallbackAdapter` is the
 *     nearest thing on offer and is a different claim: it grades performance,
 *     not device class, so reading it as `CPU` would be a guess.
 *   * `driver` is the empty string. There is no driver name or version anywhere
 *     in WebGPU; empty is the absence, not a driver called `""`.
 *
 * `id` is `0` because `requestAdapter()` grants one adapter or none, so there is
 * no second position to be in.
 *
 * @param {GPUAdapter} adapter
 * @returns {HalAdapterInfo}
 */
export function halAdapterInfoFor(adapter) {
  return {
    id: 0,
    name: adapterName(adapter),
    vendorId: 0,
    deviceId: 0,
    deviceType: DEVICE_TYPE.OTHER,
    driver: '',
    features: halFeaturesFor(adapter),
    limits: halLimitsFor(adapter),
  };
}

/**
 * `crcbl_hal::DeviceCaps` for the device that was actually opened.
 *
 * Read off the `GPUDevice` and never off the adapter it came from — see
 * `halFeaturesFor` and `halLimitsFor` for what differs between the two, and
 * `crates/crcbl-webgpu/src/reply.rs` for why the seam insists on the
 * distinction.
 *
 * @param {GPUDevice} device
 * @returns {{ features: bigint, limits: HalLimits }}
 */
export function halDeviceCapsFor(device) {
  return { features: halFeaturesFor(device), limits: halLimitsFor(device) };
}

/**
 * The reason recorded when `requestAdapter()` grants nothing.
 *
 * A promise that resolves `null` carries no message of its own, and the reason
 * field must not be empty for the case that has the most to explain — a browser
 * with WebGPU and no GPU behind it is a real machine, and this string is what a
 * log line or a banner ends up showing.
 */
const NO_ADAPTER_REASON = 'navigator.gpu.requestAdapter() granted no adapter';

/**
 * How much of a refusal's reason is kept.
 *
 * The reason comes from another vendor's runtime, and the reply writer refuses a
 * field past the stream's cap by throwing — inside a promise callback, where the
 * throw would become an unhandled rejection and the reply would simply never be
 * queued, leaving wasm waiting for ever. Truncating is the cheap guard against
 * that; no real message comes anywhere near it.
 */
const MAX_REASON_CHARS = 512;

/**
 * A command the stream carries and this replayer cannot execute yet.
 *
 * `sequence` is what makes it actionable: it names the command in the same
 * numbering wasm's own error attribution uses, so the Rust that encoded it can
 * be found again.
 */
export class ReplayError extends Error {
  /**
   * @param {string} command The `name` the decoder gave the command.
   * @param {bigint} sequence The sequence that command was assigned.
   */
  constructor(command, sequence) {
    super(
      `the replayer has no implementation for ${command} (command ${sequence})`
    );
    this.name = 'ReplayError';
    this.kind = 'Unimplemented';
    this.command = command;
    this.sequence = sequence;
  }
}

/**
 * A `CreateSurface` naming a canvas this page cannot present to.
 *
 * NOT A `ReplayError`, AND THE DIFFERENCE IS THE WHOLE POINT. That one means
 * the stream carried an opcode this replayer has no code for; this one means
 * the code ran and the page could not do it — the registry has no canvas under
 * that key, or the canvas will not give up a `webgpu` context because the
 * browser has none or something already took the canvas for a `2d` one. A
 * reader who cannot tell those apart cannot tell a slice that has not landed
 * from a page that is misconfigured.
 *
 * THROWN OUT OF `replay`, WHICH IS THE LOUDEST HONEST ANSWER AVAILABLE.
 * `create_surface` has no entry on the reply channel — wasm allocated the
 * handle and moved on — so there is no way to tell the far side, now or later,
 * and the two quieter options are both worse than a throw:
 *
 *   * Recording nothing and carrying on leaves a handle wasm believes in and
 *     this replayer has no context for. The failure then surfaces at the first
 *     command that presents to it, frames or slices away from the canvas id
 *     that was actually wrong.
 *   * Queueing a `DeviceFailed` would put a reply on the wire for a sequence
 *     nothing is waiting on. Wasm attributes replies by sequence, so that is
 *     not a loud failure but a wrong one.
 *
 * A throw stops the frame at the command that failed and names the canvas id it
 * could not resolve, which is the thing a person has to change.
 */
export class SurfaceError extends Error {
  /**
   * @param {'NoSuchCanvas'|'NoCanvasContext'} kind
   * @param {string} message
   * @param {bigint} sequence The sequence the failing command was assigned.
   * @param {number} canvasId The registry key it named.
   */
  constructor(kind, message, sequence, canvasId) {
    super(`${message} (command ${sequence})`);
    this.name = 'SurfaceError';
    this.kind = kind;
    this.sequence = sequence;
    this.canvasId = canvasId;
  }
}

/**
 * A device request that failed before the browser was even asked.
 *
 * Every one of these is something WebGPU cannot express and the seam can, so
 * refusing here is the only honest answer: passing the request on would open a
 * device that is missing what the caller said it needed, or attach it to
 * something that does not exist. It is caught by the replayer and answered as a
 * `DeviceFailed` reply — never left to reject a promise, because a dropped reply
 * is a command wasm waits on for ever.
 *
 * `unsupported` carries the `crcbl_hal::Features` bits that could not be
 * satisfied, `0n` when the refusal was not about features.
 */
export class DeviceRequestError extends Error {
  /**
   * @param {'UnsupportedFeatures'|'NoSuchAdapter'|'ForeignSurface'} kind
   * @param {string} message
   * @param {bigint} [unsupported]
   */
  constructor(kind, message, unsupported = 0n) {
    super(message);
    this.name = 'DeviceRequestError';
    this.kind = kind;
    this.unsupported = unsupported;
  }
}

/**
 * Everything the browser can say about an adapter, as one line.
 *
 * `GPUAdapter.info` is a `GPUAdapterInfo` with four strings, any of which a
 * browser may leave empty — Chrome fills `vendor`, `architecture` and `device`
 * on some platforms and only `description` on others, and Firefox differs again.
 * They are joined rather than picked between so the name says as much as the
 * browser was willing to, and an adapter that answered nothing at all comes back
 * as the empty string rather than as `undefined`: the reply's `name` field is
 * allowed to be empty, and that is a real answer.
 *
 * @param {GPUAdapter} adapter
 * @returns {string}
 */
function adapterName(adapter) {
  const info = adapter.info ?? {};
  return [info.vendor, info.architecture, info.device, info.description]
    .filter(Boolean)
    .join(' ');
}

/**
 * Replays decoded command streams against WebGPU and collects the answers.
 *
 * One replayer for the life of a page, not one per frame: the reply buffer
 * outlives the frame that started the work, which is the point.
 */
export class Replayer {
  /** @type {GPU | undefined} */
  #gpu;
  /** @type {ReplyWriter} */
  #replies = new ReplyWriter();
  /** Whether anything has been written into `#replies` since the last clear. */
  #queued = false;
  /** Commands started and not yet answered. */
  #inFlight = 0;
  /**
   * The adapters this replayer has granted, indexed as it numbered them.
   *
   * A `DeviceDesc` names an `AdapterId`, and that id is a position in the list
   * `Instance::adapters` returned — so the list has to still be here when the
   * device request arrives, a frame or more later. WebGPU grants one adapter or
   * none, so this holds at most one entry.
   *
   * @type {GPUAdapter[]}
   */
  #adapters = [];
  /**
   * The device this replayer opened, held for its whole life.
   *
   * Not decoration and not for later: the `GPUDevice` lives on this side of the
   * seam — wasm has an id and nothing more — so dropping the reference would
   * make it collectable while the reply says a device is open. Later slices
   * record commands against it; this one only has to keep it alive and let a
   * test see that it is the device whose capabilities were reported.
   *
   * @type {GPUDevice | null}
   */
  #device = null;
  /**
   * The canvases a `CreateSurface` may name, by the key it names them with.
   *
   * `SurfaceTarget::Web` is "an integer key into the shell's JS-side canvas
   * registry" and nothing else — no string crosses the wasm boundary — so this
   * is that registry, and the lookup is the whole of resolving a surface
   * target.
   *
   * @type {{ get(canvasId: number): HTMLCanvasElement | undefined }}
   */
  #canvases;
  /**
   * The `GPUCanvasContext` behind each live surface, by the handle's index.
   *
   * One flat table for this resource kind and keyed on the index alone, which
   * is what `crcbl-webgpu`'s crate docs require: handles are typed and each
   * kind's indexes are its own, so a single table shared across kinds would let
   * a buffer and a surface holding the same index stand on each other.
   *
   * @type {Map<number, GPUCanvasContext>}
   */
  #surfaces = new Map();

  /**
   * @param {object} [options]
   * @param {GPU} [options.gpu] The `navigator.gpu` to replay against. Injected
   *   rather than reached for so the replayer can be driven under node, where
   *   there is none — and so a test can hand it one that refuses.
   * @param {{ get(canvasId: number): HTMLCanvasElement | undefined }}
   *   [options.canvases] The shell's canvas registry, injected for the same two
   *   reasons: node has no DOM to query, and a test needs to hand over one that
   *   does not have the canvas being asked for. It defaults to empty rather
   *   than to a document lookup so that constructing a replayer costs nothing
   *   and touches no DOM.
   */
  constructor({ gpu = globalThis.navigator?.gpu, canvases = new Map() } = {}) {
    this.#gpu = gpu;
    this.#canvases = canvases;
  }

  /** Whether there is at least one reply waiting to go to wasm. */
  get hasReplies() {
    return this.#queued;
  }

  /** How many commands have been started and not yet answered. */
  get inFlight() {
    return this.#inFlight;
  }

  /** The device this replayer opened, or `null` if none has opened. */
  get device() {
    return this.#device;
  }

  /**
   * The contexts of the surfaces that are live right now, by handle index.
   *
   * The live table rather than a copy, as `device` hands back the real device:
   * later slices read it to find the context a present or a swapchain names.
   * For now the only reader is a test, and what it is there to see is the pair
   * of things a surface command has to get right — that a `CreateSurface`
   * resolved the canvas its `canvasId` named, and that a `DestroySurface` let
   * go of it.
   *
   * @type {Map<number, GPUCanvasContext>}
   */
  get surfaces() {
    return this.#surfaces;
  }

  /**
   * The encoded replies, header included, as a view over the writer's buffer.
   *
   * Hand it to `putReplyStream` in `gpu-transport.js`; call {@link clear} only
   * once that returned `true`. A `false` is "not now", and the same bytes are
   * offered again next frame — discarding them there is the one unrecoverable
   * bug this channel can have.
   */
  get replies() {
    return this.#replies.bytes;
  }

  /** Drops the queued replies. Only after wasm has taken them. */
  clear() {
    this.#replies.clear();
    this.#queued = false;
  }

  /**
   * Executes one frame's worth of commands.
   *
   * Synchronous: it returns once every command has been executed or started,
   * never once they have finished. Nothing here awaits, and nothing blocks the
   * frame.
   *
   * @param {{ baseSequence: bigint, commands: object[] }} frame What
   *   `takeCommandStream` returned. `null` is accepted and does nothing, which
   *   is what that function answers when no channel is installed.
   * @throws {ReplayError} On the first command this replayer cannot execute.
   *   The commands before it have already run, which is the same position a
   *   backend is in when a call fails part-way through a frame.
   * @throws {SurfaceError} On a `CreateSurface` whose canvas this page does not
   *   have, which is a command that ran and failed rather than one with no
   *   implementation. See that class for why it is not swallowed.
   */
  replay(frame) {
    if (frame === null || frame === undefined) return;
    const { baseSequence, commands } = frame;
    for (let i = 0; i < commands.length; i += 1) {
      // Positional, and wrapped to 64 bits for the reason the decoders wrap:
      // the base came off the wire, so a buffer declaring the largest possible
      // number must not produce a sequence outside the range it is typed as.
      const sequence = BigInt.asUintN(64, baseSequence + BigInt(i));
      const command = commands[i];
      switch (command.name) {
        case 'EnumerateAdapters':
          this.#enumerateAdapters(sequence);
          break;
        case 'RequestDevice':
          this.#requestDevice(sequence, command);
          break;
        case 'CreateSurface':
          this.#createSurface(sequence, command);
          break;
        case 'DestroySurface':
          this.#destroySurface(command);
          break;
        default:
          throw new ReplayError(String(command.name), sequence);
      }
    }
  }

  /**
   * Asks the browser for an adapter and queues the answer for a later frame.
   *
   * The request is deferred to a microtask rather than issued inline, so that
   * `replay` cannot be handed a `navigator.gpu` whose `requestAdapter` does its
   * work synchronously and thereby make one frame's cost depend on the browser.
   * Every path below queues exactly one reply.
   *
   * @param {bigint} sequence
   */
  #enumerateAdapters(sequence) {
    this.#inFlight += 1;
    Promise.resolve()
      .then(() => {
        if (!this.#gpu) throw new Error('navigator.gpu is not available here');
        return this.#gpu.requestAdapter();
      })
      .then((adapter) => {
        if (adapter) this.#adapter(sequence, adapter);
        else this.#noAdapter(sequence, NO_ADAPTER_REASON);
      })
      .catch((error) => this.#noAdapter(sequence, String(error)))
      .finally(() => {
        this.#inFlight -= 1;
      });
  }

  /**
   * Opens a device on the adapter the enumeration granted, and queues the
   * answer for a later frame.
   *
   * Deferred to a microtask and answered exactly once on every path, as
   * `#enumerateAdapters` is and for the same reasons.
   *
   * THE DESCRIPTOR IS CHECKED BEFORE THE BROWSER IS ASKED, and each refusal is
   * something WebGPU has no way to express:
   *
   *   * an `adapter` id no enumeration granted — the id is a position in the
   *     list this replayer answered with, and there is nothing to open;
   *   * a `compatibleSurface` — the seam's surfaces are a table this replayer
   *     does not have yet, so a request that names one cannot be honoured and
   *     must not be honoured *silently*, which would open a headless device for
   *     a caller that asked for a presentable one;
   *   * a required feature with no `GPUFeatureName` behind it. `requiredFeatures`
   *     can only carry names, so such a bit cannot be passed on, and dropping it
   *     would grant a device the caller declared it could not use.
   *
   * Optional features go the other way: only the ones this adapter actually has
   * are asked for, because `requestDevice` fails the *whole* request over a
   * feature the adapter lacks — which would turn "nice to have" into fatal.
   *
   * THE LIMITS ARE NOT REQUESTED AT ALL, so the device gets the specification's
   * defaults. `DeviceDesc` carries no limits to ask with, and inventing a
   * request from the adapter's ceilings would be this file deciding something
   * the caller did not.
   *
   * A DEVICE LOST LATER IS NOT THIS. `GPUDevice.lost` and `uncapturederror`
   * report a device that opened and then failed, which is
   * `Device::take_error`'s territory and has no reply on this channel yet;
   * nothing below listens for either, and a `DeviceFailed` answers only the
   * request itself.
   *
   * @param {bigint} sequence
   * @param {{ adapter: number, label: string | null, requiredFeatures: bigint,
   *           optionalFeatures: bigint,
   *           compatibleSurface: { index: number, generation: number } | null }} command
   */
  #requestDevice(sequence, command) {
    this.#inFlight += 1;
    Promise.resolve()
      .then(() => {
        const adapter = this.#adapters[command.adapter];
        if (!adapter) {
          throw new DeviceRequestError(
            'NoSuchAdapter',
            `no adapter ${command.adapter} has been enumerated`
          );
        }
        if (command.compatibleSurface) {
          throw new DeviceRequestError(
            'ForeignSurface',
            `compatible_surface names surface ${command.compatibleSurface.index}, ` +
              'and this replayer has no surface table yet'
          );
        }
        const required = webgpuFeaturesFor(command.requiredFeatures);
        if (required.unsatisfiable !== 0n) {
          throw new DeviceRequestError(
            'UnsupportedFeatures',
            'no WebGPU feature satisfies required crcbl_hal::Features ' +
              `0x${required.unsatisfiable.toString(16)} ` +
              `(bit${featureBitIndices(required.unsatisfiable).length === 1 ? '' : 's'} ` +
              `${featureBitIndices(required.unsatisfiable).join(', ')})`,
            required.unsatisfiable
          );
        }
        const missing = required.names.filter(
          (name) => !adapter.features?.has(name)
        );
        if (missing.length > 0) {
          // The adapter's own gap rather than WebGPU's: the names exist, this
          // adapter does not have them. Reported with the bits that named them,
          // so the far side sees the same shape either way.
          throw new DeviceRequestError(
            'UnsupportedFeatures',
            `the adapter does not have required ${missing.join(', ')}`,
            featuresFromNames(missing)
          );
        }
        const optional = webgpuFeaturesFor(command.optionalFeatures).names;
        return adapter.requestDevice({
          ...(command.label === null ? {} : { label: command.label }),
          requiredFeatures: [
            ...required.names,
            ...optional.filter((name) => adapter.features?.has(name)),
          ],
        });
      })
      .then((device) => {
        this.#device = device;
        this.#replies.device(sequence, halDeviceCapsFor(device));
        this.#queued = true;
      })
      .catch((error) => {
        this.#replies.deviceFailed(
          sequence,
          String(error).slice(0, MAX_REASON_CHARS),
          error instanceof DeviceRequestError ? error.unsupported : 0n
        );
        this.#queued = true;
      })
      .finally(() => {
        this.#inFlight -= 1;
      });
  }

  /**
   * Resolves a canvas key to its `GPUCanvasContext` and holds it against the
   * surface handle wasm allocated.
   *
   * Synchronous, unlike the two commands above, because nothing here is a
   * promise: `getContext` answers in the call. There is no reply either — the
   * handle came in with the command, so nothing on the far side is waiting to
   * learn it.
   *
   * THE CONTEXT IS DELIBERATELY NOT CONFIGURED, and this is the paragraph that
   * exists because "we forgot" and "not here on purpose" look identical in
   * code. `GPUCanvasContext.configure` takes a `GPUDevice`, and this is
   * `Instance::create_surface` — an `Instance` method, which the seam lets a
   * caller make before any device exists, and `crcbl-render` does exactly that:
   * the surface is what `DeviceDesc::compatible_surface` names when the device
   * is *then* requested. Configuring here would need a device this replayer may
   * not have, and would pick a format, alpha mode and size that belong to a
   * swapchain nobody has described yet. That configure call is swapchain
   * creation's, in a later slice.
   *
   * A LOOKUP THAT FAILS THROWS. Both ways it can — no canvas under that key, or
   * a canvas that will not give up a `webgpu` context — are the page being
   * wrong rather than the stream being malformed, and {@link SurfaceError} is
   * where the choice is argued.
   *
   * @param {bigint} sequence
   * @param {{ surface: { index: number, generation: number }, canvasId: number }} command
   */
  #createSurface(sequence, command) {
    const canvas = this.#canvases.get(command.canvasId);
    if (!canvas) {
      throw new SurfaceError(
        'NoSuchCanvas',
        `no canvas is registered under id ${command.canvasId}`,
        sequence,
        command.canvasId
      );
    }
    // `'webgpu'` and never a variable: it is the one context type a surface can
    // present through, and a canvas that already handed out a `'2d'` context
    // answers `null` here rather than throwing.
    const context = canvas.getContext('webgpu');
    if (!context) {
      throw new SurfaceError(
        'NoCanvasContext',
        `canvas ${command.canvasId} gave up no webgpu context — this browser ` +
          'has no WebGPU, or the canvas is already bound to another context',
        sequence,
        command.canvasId
      );
    }
    this.#surfaces.set(command.surface.index, context);
  }

  /**
   * Lets go of a surface's context.
   *
   * A DESTROY OF AN EMPTY SLOT IS A NO-OP, not an error, and that is the
   * stream's rule rather than this file's convenience: `crcbl-render` destroys
   * a resource whose creation returned an `Err` before it applies `?`, so an id
   * nothing ever created still arrives here. `crcbl-webgpu`'s own decoder
   * consults no table for the same reason. `Map.delete` is already that
   * behaviour, and the `if` a reader might expect to see is what is absent.
   *
   * Dropping the reference is the whole of the release. There is no
   * `unconfigure` to make because {@link Replayer#surfaces}'s contexts are
   * never configured — see `#createSurface` — and the swapchain slice that
   * starts configuring them is the one that has to unconfigure here.
   *
   * @param {{ surface: { index: number, generation: number } }} command
   */
  #destroySurface(command) {
    this.#surfaces.delete(command.surface.index);
  }

  /**
   * @param {bigint} sequence
   * @param {GPUAdapter} adapter
   */
  #adapter(sequence, adapter) {
    // Kept for the device request that may name it, by the id this reply gives
    // it — which is `0`, because `requestAdapter()` grants one adapter or none.
    this.#adapters = [adapter];
    this.#replies.adapter(sequence, halAdapterInfoFor(adapter));
    this.#queued = true;
  }

  /**
   * @param {bigint} sequence
   * @param {string} reason
   */
  #noAdapter(sequence, reason) {
    this.#replies.noAdapter(sequence, reason.slice(0, MAX_REASON_CHARS));
    this.#queued = true;
  }
}
