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
// WHAT IT REPLAYS SO FAR IS ONE COMMAND. `EnumerateAdapters` calls
// `navigator.gpu.requestAdapter()` and answers with the whole of the seam's
// `AdapterInfo` — the browser's name for it, its features and its limits in the
// seam's vocabulary, and the documented absence for the four fields WebGPU has
// no answer for — or with the reason there is none. The translation is the block
// below this header and is where every one of those choices is argued.
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
 * The `crcbl_hal::Features` bits `adapter.features` amounts to.
 *
 * @param {GPUAdapter} adapter
 * @returns {bigint}
 */
export function halFeaturesFor(adapter) {
  let bits = CORE_FEATURES;
  const features = adapter.features;
  if (features) {
    for (const [name, bit] of Object.entries(FEATURE_MAP)) {
      if (features.has(name)) bits |= bit;
    }
  }
  return bits;
}

/**
 * `adapter.limits` in the seam's names and units.
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
 * @param {GPUAdapter} adapter
 * @returns {HalLimits}
 */
export function halLimitsFor(adapter) {
  const limits = adapter.limits ?? {};
  const timestamps = Boolean(adapter.features?.has('timestamp-query'));
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
  /** Enumerations started and not yet answered. */
  #inFlight = 0;

  /**
   * @param {object} [options]
   * @param {GPU} [options.gpu] The `navigator.gpu` to replay against. Injected
   *   rather than reached for so the replayer can be driven under node, where
   *   there is none — and so a test can hand it one that refuses.
   */
  constructor({ gpu = globalThis.navigator?.gpu } = {}) {
    this.#gpu = gpu;
  }

  /** Whether there is at least one reply waiting to go to wasm. */
  get hasReplies() {
    return this.#queued;
  }

  /** How many commands have been started and not yet answered. */
  get inFlight() {
    return this.#inFlight;
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
   * @param {bigint} sequence
   * @param {GPUAdapter} adapter
   */
  #adapter(sequence, adapter) {
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
