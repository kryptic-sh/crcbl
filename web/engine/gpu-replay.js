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
// TWO OF THE COMMANDS IT REPLAYS ASK THE BROWSER SOMETHING IT CANNOT ANSWER AT
// ONCE — the two whose answer is a promise. `EnumerateAdapters`
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
// AND THE BUFFER PAIR, WHICH RUNS AGAINST THE DEVICE AND ANSWERS NOTHING
// EITHER. `CreateBuffer` translates a `crcbl_hal::BufferDesc` into a
// `GPUBufferDescriptor` and calls `createBuffer` on the device `RequestDevice`
// opened; `DestroyBuffer` calls `destroy()` on what it made and lets go of the
// slot. Neither queues a reply, for `CreateSurface`'s reason — wasm allocated
// the handle and moved on. The translation is `webgpuBufferUsageFor` and every
// bit WebGPU cannot express is named there.
//
// AND THE IMAGE PAIR AND THE VIEW PAIR, WHICH RUN AGAINST THE DEVICE AND THE
// IMAGE. `CreateImage` translates a `crcbl_hal::ImageDesc` into a
// `GPUTextureDescriptor` and calls `createTexture` on that same device;
// `CreateImageView` looks its image handle up in the table the first one filled
// and calls `createView` on the `GPUTexture` it finds, because a view in WebGPU
// is made by the texture rather than by the device. `DestroyImage` calls
// `destroy()`; `DestroyImageView` only lets go, because a `GPUTextureView` has
// no destroy to call. None of the four queues a reply, for `CreateSurface`'s
// reason. The translations are the block below `webgpuTextureUsageFor` and every
// seam value WebGPU cannot express is named there — including the two whose
// failure has to be reported rather than translated: a format this device did
// not enable the feature for, and an image handle that resolves to nothing.
//
// AND THE SAMPLER PAIR, WHICH RUNS AGAINST THE DEVICE AND ANSWERS NOTHING
// EITHER. `CreateSampler` translates a `crcbl_hal::SamplerDesc` into a
// `GPUSamplerDescriptor`; `DestroySampler` only lets go, because a `GPUSampler`
// has no destroy to call. It is the descriptor with the most to decide and the
// least to check afterwards: three of its nine fields arrive as something WebGPU
// cannot hold — an address mode it has no word for, a "no limit" sentinel whose
// spelling is not an absence, and a float where it wants an integer whose
// validity depends on the filters beside it — and what comes back reports its
// `label` and nothing else. The block below `webgpuAddressModesFor` is where
// every one of those is argued.
//
// AND THE BIND-GROUP-LAYOUT PAIR, WHICH IS THE FIRST COMMAND CARRYING A LIST OF
// STRUCTS. `CreateBindGroupLayout` translates a `crcbl_hal::BindGroupLayoutDesc`
// into a `GPUBindGroupLayoutDescriptor` — one entry at a time, in the slice's own
// order — and calls `createBindGroupLayout` on the device; `DestroyBindGroupLayout`
// only lets go, because a `GPUBindGroupLayout` has no destroy to call either.
// Neither queues a reply, for `CreateSurface`'s reason. It is the descriptor
// whose fields WebGPU has the *least* room for: an entry's `count` and its
// `BindingFlags` describe a bindless model WebGPU does not have, two of the five
// `ShaderStages` have no `GPUShaderStage` bit, and one `BindingKind` needs a
// texture format the seam does not carry. Every one of those is refused rather
// than smoothed over, and the block below `webgpuShaderStageFor` is where each is
// argued — including which of `check_entries`'s rules this file re-checks and
// which it leaves to the browser.
//
// SO A BUFFER THAT CANNOT BE MADE HAS NOWHERE TO BE REPORTED, and the answer is
// the seam's own: `Device::take_error`, which `crcbl_hal` documents as existing
// *for WebGPU* and which `docs/plan/41-webgpu-stream.md` has `Gpu::acquire`
// draining at the top of every frame. This replayer keeps that queue — see
// {@link DeviceErrorLog} — and everything that can go wrong with a buffer goes
// into it: the refusals this file makes before asking the browser, a
// `createBuffer` that throws, and the errors the device reports asynchronously
// through `uncapturederror`. Not a throw, and not a reply of its own; the
// `TakeError` command is what carries them across, because a reply has to name a
// sequence something is waiting on and an `uncapturederror` names nothing.
//
// AND `SurfaceCaps`, WHICH ANSWERS WITHIN THE CALL. It is the third command
// with a reply and the only one whose answer is ready immediately: WebGPU has no
// asynchronous capability query, and almost nothing it has at all — one string
// from `getPreferredCanvasFormat()`, and a handful of facts the specification
// fixes. So it queues its reply during `replay` rather than a frame later, and
// the translation below `surfaceCapsFor` is where every field that has no
// browser answer is named as such rather than filled in with a plausible number.
// It carries no arguments, because the record depends on none — that string
// comes off `navigator.gpu` and not off a canvas — so there is nothing here to
// look up and nothing to refuse for. The one way it can fail is the browser
// naming a canvas format this seam has no `Format` for, and that is answered
// with a `SurfaceCapsFailed` reply rather than thrown, because
// `Instance::surface_caps` is how adapter selection is done and a refusal is an
// ordinary step of it rather than a reason to lose the frame.
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

import {
  COMPOSITE_ALPHA,
  DEVICE_TYPE,
  FORMAT,
  MAX_DEVICE_ERRORS,
  PRESENT_MODE,
  SURFACE_CAPS_FAILURE,
  ReplyWriter,
} from './gpu-reply.js';

// ─────────────────────────────────────────────────────────────────────────────
// WebGPU's vocabulary, in the seam's
// ─────────────────────────────────────────────────────────────────────────────
//
// THIS IS THE ONE PLACE THE TRANSLATION HAPPENS, and it lives here rather than
// in Rust on purpose. This file is the WebGPU-facing half, so it is the half
// that should own "what a `GPUFeatureName` means in `crcbl_hal::Features`" —
// exactly as `crcbl-wgpu's instance module (deleted 2026-08-21)` owns it for wgpu's enum. The
// wire then speaks the seam's vocabulary end to end, as it already does for
// load ops, store ops and handles, instead of carrying one foreign spelling.
//
// THE MAPPING IS LOSSY IN BOTH DIRECTIONS AND BOTH LISTS ARE WRITTEN OUT BELOW.
// A reader needs to know which HAL flags can never be set here, so that their
// absence is not read as a browser that declined; and which WebGPU features are
// dropped, so that nobody looks for their effect on the far side.
//
// IT REPORTS WHAT THE BROWSER SAID, NOT WHAT `crcbl-webgpu` CAN EXECUTE. The
// channel's job is to carry the browser's answer intact; a capability on the
// seam is a promise about what a caller may *ask for*, so that set has to be
// intersected somewhere with what the stream can encode.
// `crates/crcbl-webgpu/src/instance.rs` says so too. `TIMESTAMP_QUERY` used to
// be the standing example of a bit the stream could not serve, and is no longer
// one: `CreateQuerySet` takes the timestamp kind and both pass commands carry
// `timestampWrites`, so a browser that reports `timestamp-query` gets a bit that
// means something.
//
// WHAT IS ENFORCED, AND WHAT IS NOT — two halves of that intersection, and only
// one of them has anything holding it. `web/tools/gpu-replay.mjs`'s "every
// mapped feature has a command that can serve it" section reads the keys of
// `FEATURE_MAP` out of the table itself and drives, for each one, the command
// that serves it against a stub device that opened with it. A row added here
// for a feature whose commands do not exist yet has no such command to drive
// and fails that suite by construction, so the **mapping** can no longer
// outrun the stream unnoticed. Every row today passes it.
//
// THE RUNTIME INTERSECTION IS THE HALF THAT STILL DOES NOT HAPPEN.
// `crcbl_webgpu::hal::WebGpuInstance` implements `Instance`, and its `adapters`
// hands back the `AdapterInfo`s this file filled, unchanged: no bit is withheld
// there for a command the stream cannot encode. That is sound only while the
// table above is honest, which is precisely what the gate keeps it — a check on
// this table, not a filter on the reply.

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
 *   * `DEBUG_MARKERS` (1 << 18) — `pushDebugGroup`, `popDebugGroup` and
 *     `insertDebugMarker` are core on every encoder and reach the browser's own
 *     capture tooling. All three are replayed — see
 *     {@link Replayer#beginDebugLabel} — which is what makes granting the bit
 *     honest: a capability the device reports and the stream then refuses would
 *     take down a whole command buffer at `finish()`.
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
 * `GPUMapMode.READ`, spelled as its specification value rather than reached for
 * off the global.
 *
 * A fixed number in the WebGPU specification — `GPUMapMode.READ` is `0x0001` —
 * so writing it here is not a guess, and it lets {@link Replayer#requestReadback}
 * run under node against a stub buffer where the `GPUMapMode` namespace does not
 * exist, exactly as {@link GPU_SHADER_STAGE} does for the shader-stage bits.
 */
const GPU_MAP_READ = 0x0001;

/**
 * The alignment `GPUCommandEncoder.resolveQuerySet`'s `destinationOffset` must
 * satisfy.
 *
 * A constant of the specification rather than a limit anything reports, like
 * {@link COPY_BYTES_PER_ROW_ALIGNMENT}: `wgpu-types` spells the same number
 * `QUERY_RESOLVE_BUFFER_ALIGNMENT`, and `wgpu-core`'s
 * `command::query::resolve_query_set` checks it before anything else about the
 * call. Refused here rather than left to the browser because a WebGPU validation
 * error arrives out of band — a frame later, attributed to nothing — so the
 * command that carried the offset would not be the one named.
 */
const QUERY_RESOLVE_BUFFER_ALIGNMENT = 256n;

/**
 * Bytes one resolved query occupies in the destination buffer.
 *
 * `wgpu-types`' `QUERY_SIZE`: a `GPUQuerySet` resolves one `u64` per query, for
 * both of the types WebGPU has. It is what {@link Replayer#queryResults} sizes
 * its scratch buffers with and what turns a query count into a byte length.
 */
const QUERY_RESULT_BYTES = 8;

/**
 * What one texel block of a colour format occupies in a linear buffer, keyed by
 * the `GPUTextureFormat` string {@link TEXTURE_FORMAT} maps to.
 *
 * **Why this table and not {@link TEXTURE_FORMAT}**: a copy carries no format —
 * it names the image, and the texture's own `format` is read back off it — so
 * the conversion from `crcbl_hal::BufferImageCopy::buffer_row_length` (which is
 * in *texels*) to WebGPU's `bytesPerRow` (which is in *bytes*) needs a footprint
 * the format string does not carry.
 *
 * **A TEXEL IS THE DEGENERATE BLOCK, WHICH IS WHY THERE IS NO SECOND TABLE FOR
 * THE COMPRESSED ONES.** WebGPU's `GPUTexelCopyBufferLayout` strides between
 * *block* rows and counts *block* rows — `bytesPerRow` is the distance between
 * one row of whole blocks and the next, and `rowsPerImage` is how many such rows
 * an image has — while the seam, mirroring Vulkan, states both pitches in
 * texels. `width` and `height` are the block's extent in texels and `bytes` is
 * what one block occupies, which is `crcbl_hal::Format::block_extent` and
 * `Format::block_size` restated in the two numbers this file's arithmetic wants.
 * An uncompressed format's extent is `1 × 1`, so dividing by it is a no-op and
 * {@link Replayer#textureCopyLayout} needs one formula rather than a compressed
 * branch beside an uncompressed one — two branches being where the arithmetic
 * for the rarer case would rot unnoticed.
 *
 * Every BC row is gated behind `texture-compression-bc` at creation, so a
 * texture reaching a copy with one of these formats is one the device enabled;
 * {@link webgpuTextureFormatFor} is where that was decided, and a row here for a
 * format that never got created costs nothing.
 *
 * A depth or stencil plane is the case this table does *not* answer: its
 * footprint depends on which plane the copy names, which is
 * {@link DEPTH_STENCIL_COPY}'s table. No depth format is block compressed, so
 * each of its planes is a `1 × 1` block of that plane's own byte count.
 *
 * @see https://www.w3.org/TR/webgpu/#gputexelcopybufferlayout
 */
const BLOCK_FOOTPRINT = Object.freeze({
  r8unorm: { width: 1, height: 1, bytes: 1 },
  rg8unorm: { width: 1, height: 1, bytes: 2 },
  rgba8unorm: { width: 1, height: 1, bytes: 4 },
  'rgba8unorm-srgb': { width: 1, height: 1, bytes: 4 },
  bgra8unorm: { width: 1, height: 1, bytes: 4 },
  'bgra8unorm-srgb': { width: 1, height: 1, bytes: 4 },
  rgb10a2unorm: { width: 1, height: 1, bytes: 4 },
  rg11b10ufloat: { width: 1, height: 1, bytes: 4 },
  r16float: { width: 1, height: 1, bytes: 2 },
  rg16float: { width: 1, height: 1, bytes: 4 },
  rgba16float: { width: 1, height: 1, bytes: 8 },
  r32float: { width: 1, height: 1, bytes: 4 },
  rg32float: { width: 1, height: 1, bytes: 8 },
  rgba32float: { width: 1, height: 1, bytes: 16 },
  r32uint: { width: 1, height: 1, bytes: 4 },
  rg32uint: { width: 1, height: 1, bytes: 8 },
  'bc1-rgba-unorm': { width: 4, height: 4, bytes: 8 },
  'bc1-rgba-unorm-srgb': { width: 4, height: 4, bytes: 8 },
  'bc3-rgba-unorm': { width: 4, height: 4, bytes: 16 },
  'bc3-rgba-unorm-srgb': { width: 4, height: 4, bytes: 16 },
  'bc4-r-unorm': { width: 4, height: 4, bytes: 8 },
  'bc5-rg-unorm': { width: 4, height: 4, bytes: 16 },
  'bc6h-rgb-ufloat': { width: 4, height: 4, bytes: 16 },
  'bc7-rgba-unorm': { width: 4, height: 4, bytes: 16 },
  'bc7-rgba-unorm-srgb': { width: 4, height: 4, bytes: 16 },
});

/**
 * What WebGPU lets a buffer↔texture copy do with each plane of each
 * depth-stencil format this backend can create. The specification's own table,
 * transcribed — not a judgement made here.
 *
 * **A DEPTH PLANE IS NOT A COLOUR PLANE OF A DIFFERENT WIDTH.** Three things
 * differ at once, and each is a wrong picture rather than an error if it is
 * guessed:
 *
 *   * **The footprint is per plane.** `depth32float-stencil8` moves four bytes a
 *     texel through its depth plane and one through its stencil plane, so
 *     {@link BLOCK_FOOTPRINT}'s one-row-per-format shape cannot hold it.
 *   * **So is the direction.** `depth32float` is a legal copy *source* and an
 *     illegal copy *destination*, and `depth24plus-stencil8`'s depth plane is
 *     neither — `depth24plus` names whatever the driver chose to store, so there
 *     is no memory layout to lay a buffer out against.
 *   * **A copy names ONE plane.** `'all'` is rejected for a depth-stencil format
 *     however the buffer is laid out, which a colour copy never has to think
 *     about because `'all'` is the only aspect a colour format has.
 *
 * Keyed by `GPUTextureFormat`, then by the `GPUTextureAspect`
 * {@link webgpuTextureAspectFor} answers. A format absent from this table is a
 * colour one and goes through {@link BLOCK_FOOTPRINT}; an aspect absent from a
 * row is a plane the format does not have or one WebGPU will not copy in either
 * direction. `source` and `destination` are the two directions separately,
 * because that is how the specification states them.
 *
 * The four rows are the four depth formats {@link TEXTURE_FORMAT} can produce.
 * `stencil8` and `depth24plus` have no `crcbl_hal::Format` spelling, so no
 * texture this replayer holds can carry either.
 *
 * @see https://www.w3.org/TR/webgpu/#depth-formats
 */
const DEPTH_STENCIL_COPY = Object.freeze({
  depth16unorm: { 'depth-only': { bytes: 2, source: true, destination: true } },
  depth32float: {
    'depth-only': { bytes: 4, source: true, destination: false },
  },
  'depth32float-stencil8': {
    'depth-only': { bytes: 4, source: true, destination: false },
    'stencil-only': { bytes: 1, source: true, destination: true },
  },
  'depth24plus-stencil8': {
    'stencil-only': { bytes: 1, source: true, destination: true },
  },
});

/**
 * The `GPULoadOp` a decoded `crcbl_hal::LoadOp` lowers to.
 *
 * `DontCare` has no WebGPU spelling — WebGPU offers only `'load'` and `'clear'`
 * — and it lowers to **`'clear'`**, not `'load'`. `LoadOp::DontCare` means "the
 * previous contents are not meaningful", which is true precisely when the pass
 * writes every pixel or a swapchain image was just acquired; lowering it to
 * `'clear'` writes the attachment's own clear value, which is deterministic,
 * where `'load'` would preserve contents WebGPU leaves undefined and a validation
 * layer may flag as uninitialised. The seam's `ColorAttachment` always carries a
 * clear value, so there is always one to use.
 */
const LOAD_OP = Object.freeze({
  Load: 'load',
  Clear: 'clear',
  DontCare: 'clear',
});

/** The `GPUStoreOp` a decoded `crcbl_hal::StoreOp` lowers to. */
const STORE_OP = Object.freeze({ Store: 'store', Discard: 'discard' });

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

// ─────────────────────────────────────────────────────────────────────────────
// A buffer's vocabulary, in WebGPU's
// ─────────────────────────────────────────────────────────────────────────────
//
// TWO SEAM FIELDS LAND ON ONE WEBGPU FIELD, which is the whole shape of this
// translation. `crcbl_hal::BufferDesc` says both what a buffer is *for*
// (`usage`) and where its memory *lives* (`memory`); `GPUBufferDescriptor` has
// only `usage`, because WebGPU has no heap selection at all — an implementation
// places a buffer from the uses it was declared with. So both seam fields
// become usage bits, and the second one has almost nothing to become.

/**
 * The `GPUBufferUsage` bits, as the specification fixes them.
 *
 * Spelled out rather than read off `globalThis.GPUBufferUsage`, for the reason
 * {@link MAX_SAMPLE_COUNT} is spelled out: these are constants of the format
 * rather than facts about a browser, and node has no `GPUBufferUsage` to read
 * them from — this file has to be drivable there. `web/tools/browser-e2e.mjs`
 * holds them against the real namespace object in a real browser, which is
 * where a wrong value would be caught.
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
 * Every `crcbl_hal::BufferUsage` flag with a `GPUBufferUsage` bit, and the bit.
 *
 * Keyed by the names `gpu-stream.js` decodes a usage word into, so a flag this
 * table does not list cannot be mapped by accident — it comes back from
 * {@link webgpuBufferUsageFor} as unsatisfiable instead.
 *
 * THE ONE FLAG WITH NO WEBGPU BIT IS `DEVICE_ADDRESS`, and it is **refused
 * rather than dropped**. WebGPU has no buffer device address, no bindless model
 * and no raw GPU pointers — which is why `Features::BUFFER_DEVICE_ADDRESS` is
 * on the never-set list above, and `crcbl_hal::BufferUsage::DEVICE_ADDRESS`
 * documents itself as requiring that feature. So a buffer asking for it is a
 * caller using a capability this device reported it does not have, and the two
 * quieter answers are both worse: dropping the bit hands back a buffer whose
 * address cannot be taken, and the failure then surfaces at whatever shader
 * dereferences it, which is nowhere near the creation that was wrong.
 *
 * NOTHING GOES THE OTHER WAY, and one omission is worth naming: WebGPU's
 * `VERTEX` has no seam flag, because this engine pulls vertices out of storage
 * buffers rather than binding vertex buffers — `BufferUsage::STORAGE`'s own
 * documentation says so. `MAP_READ` and `MAP_WRITE` are not usages on this seam
 * either; they are what {@link MEMORY_LOCATION_USAGE} is about.
 */
const BUFFER_USAGE_MAP = Object.freeze({
  TRANSFER_SRC: GPU_BUFFER_USAGE.COPY_SRC,
  TRANSFER_DST: GPU_BUFFER_USAGE.COPY_DST,
  UNIFORM: GPU_BUFFER_USAGE.UNIFORM,
  STORAGE: GPU_BUFFER_USAGE.STORAGE,
  INDEX: GPU_BUFFER_USAGE.INDEX,
  INDIRECT: GPU_BUFFER_USAGE.INDIRECT,
  QUERY_RESOLVE: GPU_BUFFER_USAGE.QUERY_RESOLVE,
});

/**
 * What each `crcbl_hal::MemoryLocation` adds to a buffer's usage.
 *
 * WEBGPU HAS NO HEAP TO CHOOSE, so this is not a heap type under another name.
 * The only mapping-related things `GPUBufferDescriptor` has are `MAP_READ` and
 * `MAP_WRITE`, and the specification forbids either from being combined with
 * anything but one copy usage: a buffer with `MAP_WRITE` may carry `COPY_SRC`
 * and nothing else, and one with `MAP_READ` may carry `COPY_DST` and nothing
 * else. That constraint is what decides all three rows.
 *
 *   * `DeviceLocal` adds **nothing**, and needs to: a buffer with no mapping
 *     usage is the one an implementation is free to place in device-local
 *     memory, which is exactly what this location asks for.
 *   * `HostReadback` adds `MAP_READ`. This is the one location WebGPU can
 *     express outright — `mapAsync(GPUMapMode.READ)` needs the bit at creation
 *     and `Device::poll_readback` is the seam call that will need it — and the
 *     combination the specification permits alongside it, `COPY_DST`, is
 *     precisely what the seam's readback ring is: a copy destination the GPU
 *     never writes through a shader. A descriptor that pairs this location with
 *     some other usage is refused by the browser rather than quietly stripped
 *     here, because a readback buffer that cannot be mapped is not the thing
 *     that was asked for.
 *   * `HostUpload` adds `COPY_DST`, **and this is the row WebGPU cannot
 *     express.** The location means CPU-writable and GPU-readable, and the
 *     buffers the engine puts there are uniform blocks and read-only storage
 *     tables — none of which may carry `MAP_WRITE` at all under the rule above,
 *     so the obvious mapping is one `createBuffer` would reject outright. What
 *     WebGPU offers for the same job is `queue.writeBuffer`, which is what
 *     `Device::write_buffer` becomes on this backend and which requires
 *     `COPY_DST`. So the location becomes the usage that mechanism needs.
 *     It **widens** what the buffer may be used for rather than narrowing it,
 *     which can cost placement but never correctness, and it is written down
 *     here because a reader comparing this row against Vulkan's memory
 *     properties will otherwise read it as a mistake.
 */
const MEMORY_LOCATION_USAGE = Object.freeze({
  DeviceLocal: 0,
  HostUpload: GPU_BUFFER_USAGE.COPY_DST,
  HostReadback: GPU_BUFFER_USAGE.MAP_READ,
});

/**
 * The `GPUBufferUsage` word a decoded `BufferDesc`'s two fields amount to, and
 * whatever in them WebGPU has no bit for.
 *
 * `unsatisfiable` is a list of seam names rather than a word, because it is for
 * a message a person reads — the caller refuses the creation and says which
 * flag did it. It is empty for every descriptor this backend can honour.
 *
 * @param {readonly string[]} usage `crcbl_hal::BufferUsage` flag names, as
 *   `gpu-stream.js` decodes them.
 * @param {string} memory A `crcbl_hal::MemoryLocation` variant name.
 * @returns {{ bits: number, unsatisfiable: string[] }}
 */
export function webgpuBufferUsageFor(usage, memory) {
  let bits = 0;
  const unsatisfiable = [];
  for (const name of usage) {
    const bit = BUFFER_USAGE_MAP[name];
    if (bit === undefined) unsatisfiable.push(`BufferUsage::${name}`);
    else bits |= bit;
  }
  const located = MEMORY_LOCATION_USAGE[memory];
  // A location this table does not know is a decoder that has grown a variant
  // this file has not. Named rather than treated as `DeviceLocal`, which would
  // be this file guessing about where memory the seam placed deliberately ought
  // to live.
  if (located === undefined) unsatisfiable.push(`MemoryLocation::${memory}`);
  else bits |= located;
  return { bits, unsatisfiable };
}

// ─────────────────────────────────────────────────────────────────────────────
// An image's vocabulary, in WebGPU's
// ─────────────────────────────────────────────────────────────────────────────
//
// FIVE SEAM FIELDS AND ONE WEBGPU DESCRIPTOR, and every one of the five is a
// translation with something to argue. `crcbl_hal::ImageDesc` says what an image
// *is* (`image_type`, `extent`), what it holds (`format`), how much of it there
// is (`mip_levels`, `samples`) and what it is *for* (`usage`);
// `GPUTextureDescriptor` has a member for each, and the members are not the same
// shape. The tables below are where each difference is written down rather than
// smoothed over, in the order `ImageDesc` declares them.
//
// AND `ImageViewDesc` IS THE SECOND HALF, with a difference that decides its
// whole shape: a view is made by the *image*, not by the device —
// `GPUTexture.createView` — so the image handle in the descriptor is a lookup
// this replayer must do rather than a field it passes on. See
// {@link Replayer#images}.
//
// WHAT `ImageDesc` CANNOT SAY, AND WEBGPU NEEDS TO BE TOLD. A
// `GPUTextureDescriptor` carries `viewFormats`, the list of formats a view of
// this texture may reinterpret it as, and WebGPU **refuses a view whose format
// is not the texture's own or one of that list**. `ImageViewDesc::format`
// documents itself as free to differ "for sRGB reinterpretation", and there is
// no field on `ImageDesc` that could carry the permission — so this backend
// creates every texture with WebGPU's default empty list, and a reinterpreting
// view is refused by the browser on the device's error channel. Nothing here can
// fix that: the texture is already made by the time the view names its format,
// and inventing a list would be this file granting a permission the caller never
// asked for. It is a gap in the seam rather than in this translation, and it is
// written here because that is where a reader meets it. No caller has ever
// asked: every `ImageViewDesc` the engine builds copies the format of the image
// it views.
//
// A SWAPCHAIN FRAME IS THE ONE VIEW THIS FILE DOES REINTERPRET, and it is not an
// exception to the above — it goes nowhere near `CreateImageView`. A canvas
// frame's texture is made by `GPUCanvasContext.configure`, which this file
// calls, so it is the one texture whose `viewFormats` this file is entitled to
// set: see {@link Replayer#configureSwapchain}, which configures the canvas with
// the browser's linear preferred format and names the sRGB counterpart the
// caller's `SwapchainDesc` asked for.

/**
 * The `GPUTextureUsage` bits, as the specification fixes them.
 *
 * Spelled out rather than read off `globalThis.GPUTextureUsage`, for
 * {@link GPU_BUFFER_USAGE}'s reason: these are constants of the format rather
 * than facts about a browser, and node has none of them to read.
 * `web/tools/browser-e2e.mjs` holds them against the real namespace object.
 */
const GPU_TEXTURE_USAGE = Object.freeze({
  COPY_SRC: 0x01,
  COPY_DST: 0x02,
  TEXTURE_BINDING: 0x04,
  STORAGE_BINDING: 0x08,
  RENDER_ATTACHMENT: 0x10,
});

/**
 * The sentinel a `CreateOffscreenSurface` files under its handle, where a
 * `CreateSurface` files a `GPUCanvasContext`.
 *
 * An offscreen surface has no context to resolve — it names no canvas — so its
 * slot holds this shared marker rather than per-surface state: the ring that
 * replaces `getCurrentTexture()` is sized and allocated at swapchain creation,
 * not here, so there is nothing per-surface to keep. A frozen object rather than
 * a boolean so a lookup that returns it is unambiguous against a real context,
 * which never carries an `offscreen` property.
 */
const OFFSCREEN_SURFACE = Object.freeze({ offscreen: true });

/**
 * Every `crcbl_hal::ImageUsage` flag with a `GPUTextureUsage` bit, and the bit.
 *
 * Keyed by the names `gpu-stream.js` decodes a usage word into, as
 * {@link BUFFER_USAGE_MAP} is, so a flag this table does not list cannot be
 * mapped by accident.
 *
 * TWO SEAM FLAGS LAND ON ONE WEBGPU BIT, and it is not a loss.
 * `COLOR_ATTACHMENT` and `DEPTH_STENCIL_ATTACHMENT` are both
 * `RENDER_ATTACHMENT`: WebGPU has one attachment usage and reads *which kind*
 * off the format, so a `depth32float` texture with `RENDER_ATTACHMENT` is a
 * depth attachment and an `rgba8unorm` one is a colour attachment, with no way
 * to say either wrongly. The seam's two flags carry the same information twice
 * over — `ImageAspect::of` derives the same split from the same format — so
 * nothing is dropped by folding them.
 *
 * THE ONE FLAG WITH NO WEBGPU BIT IS `PRESENT`, and it is **refused rather than
 * dropped**, exactly as `BufferUsage::DEVICE_ADDRESS` is and for a sharper
 * version of that reason. A presentable image is not something WebGPU's
 * `createTexture` can make at all: a canvas's texture comes from
 * `GPUCanvasContext.getCurrentTexture()`, its usage comes from
 * `GPUCanvasConfiguration.usage`, and it is owned by the canvas for one frame.
 * So a `CreateImage` asking for `PRESENT` is asking this backend to hand-build a
 * swapchain image, and dropping the bit would answer with an ordinary texture
 * that can never be presented — a failure that then surfaces at the present, a
 * whole frame away from the creation that was wrong.
 *
 * NOTHING GOES THE OTHER WAY: every `GPUTextureUsage` bit has a seam flag above.
 */
const IMAGE_USAGE_MAP = Object.freeze({
  TRANSFER_SRC: GPU_TEXTURE_USAGE.COPY_SRC,
  TRANSFER_DST: GPU_TEXTURE_USAGE.COPY_DST,
  SAMPLED: GPU_TEXTURE_USAGE.TEXTURE_BINDING,
  STORAGE: GPU_TEXTURE_USAGE.STORAGE_BINDING,
  COLOR_ATTACHMENT: GPU_TEXTURE_USAGE.RENDER_ATTACHMENT,
  DEPTH_STENCIL_ATTACHMENT: GPU_TEXTURE_USAGE.RENDER_ATTACHMENT,
});

/**
 * The `GPUTextureUsage` word a decoded `ImageDesc`'s usage amounts to, and
 * whatever in it WebGPU has no bit for.
 *
 * {@link webgpuBufferUsageFor}'s twin, in shape and in what `unsatisfiable`
 * means: seam names rather than a word, because it is for a message a person
 * reads, and empty for every descriptor this backend can honour.
 *
 * @param {readonly string[]} usage `crcbl_hal::ImageUsage` flag names, as
 *   `gpu-stream.js` decodes them.
 * @returns {{ bits: number, unsatisfiable: string[] }}
 */
export function webgpuTextureUsageFor(usage) {
  let bits = 0;
  const unsatisfiable = [];
  for (const name of usage) {
    const bit = IMAGE_USAGE_MAP[name];
    if (bit === undefined) unsatisfiable.push(`ImageUsage::${name}`);
    else bits |= bit;
  }
  return { bits, unsatisfiable };
}

/**
 * Every `crcbl_hal::Format` as WebGPU spells it, and the `GPUFeatureName` that
 * gates it where one does.
 *
 * Keyed by the names `gpu-stream.js` decodes a format code into — `gpu-reply.js`'s
 * `FORMAT` spelling, `RGBA8_UNORM` rather than the HAL's `Rgba8Unorm` — because
 * that is what arrives in a command.
 *
 * EVERY SEAM FORMAT HAS A WEBGPU NAME, AND ONE OF THEM IS INEXACT.
 * `D24_UNORM_S8_UINT` becomes `depth24plus-stencil8`, which is not the same
 * claim: WebGPU deleted `depth24unorm-stencil8` from the specification and what
 * is left promises *at least* 24 bits of unsigned-normalised depth beside 8 bits
 * of stencil, leaving the implementation free to back it with a 32-bit float.
 * It is taken rather than refused for {@link MEMORY_LOCATION_USAGE}'s
 * `HostUpload` reason — it widens what the caller gets rather than narrowing it,
 * so it can cost memory and never correctness — and the seam has no call that
 * could see the difference: there is no image mapping and no subresource layout
 * on it, so no caller can read the depth plane's bytes and find the layout it
 * did not expect.
 *
 * THE GATED ONES ARE THE TEN A DEVICE MAY NOT HAVE. `texture-compression-bc`
 * gates every BC format and `depth32float-stencil8` gates the one whose name it
 * shares; a device that did not enable the feature cannot use the format, and
 * {@link webgpuTextureFormatFor} is where that is decided.
 *
 * `R11G11B10_FLOAT` is `rg11b10ufloat` and is deliberately **not** listed as
 * gated. The format itself is core; `rg11b10ufloat-renderable` is a feature
 * about using it as a *render attachment*, which is a property of the usage
 * rather than of the format, and refusing every such image would refuse the
 * sampled ones a core device can make perfectly well. A render attachment a
 * device cannot render to is refused by the browser, which is where a
 * usage-dependent rule belongs.
 *
 * THE DEPTH ROWS ALSO CARRY WHICH PLANES THEY HAVE, because
 * {@link attachmentPlanesFor} needs that and nothing else in this file knows it.
 * A colour row carries neither flag and answers "no depth, no stencil", which is
 * what a colour format is for the attachment rule. It lives on the rows rather
 * than in a second table keyed by `GPUTextureFormat` name so there is one place
 * that says which seam formats are depth formats — two would be free to drift.
 *
 * `storage` IS THE THIRD SUCH FLAG, and it rides here for the same reason: it
 * says whether {@link webgpuBindingLayoutFor} may put the format in a
 * `GPUStorageTextureBindingLayout`. **Seven of the twenty-nine may.** WebGPU's
 * storage-binding list is short and this seam meets it in exactly `rgba8unorm`,
 * `rgba16float`, `r32float`, `rg32float`, `rgba32float`, `r32uint` and
 * `rg32uint`; the specification's remaining storage formats — the snorm, sint
 * and uint widths, and `rgba32sint` — have no seam spelling to reach them from.
 *
 * WHAT IS REFUSED, AND IT IS NOT AN OVERSIGHT. Every sRGB row: a storage write
 * has no encode step, so the specification allows no `-srgb` format as a storage
 * texture at all. Every depth and stencil row, and every BC row, for the same
 * flat reason — neither is in the list. `bgra8unorm` is the one that is only
 * *nearly* allowed: WebGPU gates it behind the `bgra8unorm-storage` feature,
 * which {@link FEATURE_MAP} does not carry because `crcbl_hal::Features` has no
 * bit for it, so no device this replayer opens has ever enabled it and a row
 * claiming it would promise a layout every such device refuses. The narrower
 * formats — `r8unorm`, `rg8unorm`, `r16float`, `rg16float`, `rgb10a2unorm`,
 * `rg11b10ufloat` — are storage only under the `texture-formats-tier1` feature,
 * which is the same story with a different name.
 */
const TEXTURE_FORMAT = Object.freeze({
  R8_UNORM: { name: 'r8unorm' },
  RG8_UNORM: { name: 'rg8unorm' },
  RGBA8_UNORM: { name: 'rgba8unorm', storage: true },
  RGBA8_UNORM_SRGB: { name: 'rgba8unorm-srgb' },
  BGRA8_UNORM: { name: 'bgra8unorm' },
  BGRA8_UNORM_SRGB: { name: 'bgra8unorm-srgb' },
  RGB10A2_UNORM: { name: 'rgb10a2unorm' },
  R11G11B10_FLOAT: { name: 'rg11b10ufloat' },
  R16_FLOAT: { name: 'r16float' },
  RG16_FLOAT: { name: 'rg16float' },
  RGBA16_FLOAT: { name: 'rgba16float', storage: true },
  R32_FLOAT: { name: 'r32float', storage: true },
  RG32_FLOAT: { name: 'rg32float', storage: true },
  RGBA32_FLOAT: { name: 'rgba32float', storage: true },
  R32_UINT: { name: 'r32uint', storage: true },
  RG32_UINT: { name: 'rg32uint', storage: true },
  D32_FLOAT: { name: 'depth32float', depth: true },
  D32_FLOAT_S8_UINT: {
    name: 'depth32float-stencil8',
    feature: 'depth32float-stencil8',
    depth: true,
    stencil: true,
  },
  D24_UNORM_S8_UINT: {
    name: 'depth24plus-stencil8',
    depth: true,
    stencil: true,
  },
  D16_UNORM: { name: 'depth16unorm', depth: true },
  BC1_RGBA_UNORM: { name: 'bc1-rgba-unorm', feature: 'texture-compression-bc' },
  BC1_RGBA_UNORM_SRGB: {
    name: 'bc1-rgba-unorm-srgb',
    feature: 'texture-compression-bc',
  },
  BC3_RGBA_UNORM: { name: 'bc3-rgba-unorm', feature: 'texture-compression-bc' },
  BC3_RGBA_UNORM_SRGB: {
    name: 'bc3-rgba-unorm-srgb',
    feature: 'texture-compression-bc',
  },
  BC4_R_UNORM: { name: 'bc4-r-unorm', feature: 'texture-compression-bc' },
  BC5_RG_UNORM: { name: 'bc5-rg-unorm', feature: 'texture-compression-bc' },
  BC6H_RGB_UFLOAT: {
    name: 'bc6h-rgb-ufloat',
    feature: 'texture-compression-bc',
  },
  BC7_RGBA_UNORM: { name: 'bc7-rgba-unorm', feature: 'texture-compression-bc' },
  BC7_RGBA_UNORM_SRGB: {
    name: 'bc7-rgba-unorm-srgb',
    feature: 'texture-compression-bc',
  },
});

/**
 * The `GPUTextureFormat` a decoded `Format` names on **this device**, or why it
 * names none.
 *
 * THE DEVICE'S FEATURES AND NOT THE ADAPTER'S, for `halFeaturesFor`'s reason
 * with a consequence one step sharper: a device reports what was actually
 * granted, so an adapter that *could* have given `texture-compression-bc` to a
 * device nobody asked for it for says nothing about whether this texture can be
 * created.
 *
 * **REFUSED HERE RATHER THAN LEFT TO THE BROWSER**, which is the decision this
 * function exists to make, and it is the shape `#createBuffer` already set for
 * `BufferUsage::DEVICE_ADDRESS` rather than a new one. The difference between
 * the two cases is worth stating: that flag has no WebGPU spelling *at all*,
 * while these formats have one this device may not use — so this is not "WebGPU
 * cannot express it" but "the device this stream opened cannot". Both are
 * refused for the same reason, which is what happens to the handle otherwise.
 * WebGPU answers an unusable format with a `GPUTexture` object and a validation
 * error a turn of the event loop later, so passing it on would file an *invalid*
 * texture under the handle and every command naming it afterwards would fail
 * again, one error per use, none of them naming the creation that was wrong.
 * Refusing leaves the slot empty and produces exactly one error, at the command
 * that asked, naming the format and the feature.
 *
 * A format with no row at all is the third case and is a different fault: this
 * file and `gpu-reply.js`'s `FORMAT` table having drifted. It is refused too,
 * because a nearby format is a colour-space or precision bug nothing downstream
 * could attribute.
 *
 * @param {string} format A `crcbl_hal::Format` name, as `gpu-stream.js` decodes
 *   it.
 * @param {ReadonlySet<string> | undefined} features The **device's** own
 *   `features`.
 * @returns {{ name: string | null, reason: string | null }} `reason` is a phrase
 *   for the message a person reads, and is `null` exactly when `name` is not.
 */
export function webgpuTextureFormatFor(format, features) {
  const row = TEXTURE_FORMAT[format];
  if (row === undefined) {
    return {
      name: null,
      reason: `asks for Format::${format}, which this backend has no GPUTextureFormat for`,
    };
  }
  if (row.feature !== undefined && !features?.has(row.feature)) {
    return {
      name: null,
      reason:
        `asks for Format::${format}, which WebGPU spells ${row.name} and gates ` +
        `behind the ${row.feature} feature this device did not enable`,
    };
  }
  return { name: row.name, reason: null };
}

/**
 * The `GPUTextureAspect` a decoded `ImageAspect` names, or why it names none.
 *
 * A BITFLAGS SET MEETING A THREE-VALUED ENUM, which is the whole of this
 * translation. `crcbl_hal::ImageAspect` can spell eight combinations and
 * `GPUTextureAspect` has three words, so four of the eight have no spelling and
 * two of them share one:
 *
 *   * `COLOR` alone is `'all'`. A colour format has one plane, and `'all'` is
 *     the only aspect WebGPU accepts for it.
 *   * `DEPTH | STENCIL` is `'all'` too, **and that is not a collision**: the
 *     texture's format already says which planes exist, so `'all'` means "the
 *     colour plane" on a colour format and "both planes" on a depth-stencil one.
 *     The seam's two values carry the same information the format does — which
 *     is what `ImageAspect::of` is — so nothing is lost.
 *   * `DEPTH` alone is `'depth-only'` and `STENCIL` alone is `'stencil-only'`,
 *     the two views WGSL needs to sample a depth-stencil image at all.
 *   * Everything else — the empty set, and any combination pairing `COLOR` with
 *     a depth or stencil plane — is refused. WebGPU has no word for "the colour
 *     plane beside the depth one", and no format on this seam has both.
 *
 * @param {readonly string[]} aspect `crcbl_hal::ImageAspect` flag names, as
 *   `gpu-stream.js` decodes them.
 * @returns {{ aspect: string | null, reason: string | null }}
 */
export function webgpuTextureAspectFor(aspect) {
  const color = aspect.includes('COLOR');
  const depth = aspect.includes('DEPTH');
  const stencil = aspect.includes('STENCIL');
  if (color && !depth && !stencil) return { aspect: 'all', reason: null };
  if (!color && depth && stencil) return { aspect: 'all', reason: null };
  if (!color && depth && !stencil)
    return { aspect: 'depth-only', reason: null };
  if (!color && !depth && stencil)
    return { aspect: 'stencil-only', reason: null };
  return {
    aspect: null,
    reason:
      `names aspect ${aspect.length === 0 ? '(none)' : aspect.join(' | ')}, ` +
      "which is no GPUTextureAspect: WebGPU has 'all', 'depth-only' and " +
      "'stencil-only' and no way to spell a colour plane beside a depth or " +
      'stencil one, or no plane at all',
  };
}

/**
 * Which planes a view of `format` restricted to `aspect` presents to a render
 * pass — the two facts WebGPU decides a depth-stencil attachment's load and
 * store ops by.
 *
 * **A PLANE THE ATTACHMENT DOES NOT HAVE MUST HAVE ITS OPS ABSENT**, not set to
 * some harmless-looking value, and a plane it does have must have both unless
 * that plane is read-only. WebGPU rejects the whole `finish()` otherwise — not
 * the pass, the command buffer — so a `stencilLoadOp` on a `depth32float`
 * attachment costs every command in the frame, including the ones nowhere near
 * it. `crcbl_hal::DepthStencilAttachment` carries all four ops whatever the
 * format, because a HAL that names `Format::D32Float` has already said which
 * planes exist; this is where that gets put back together.
 *
 * IT TAKES THE ASPECT AS WELL AS THE FORMAT because the view narrows the
 * format: a `'depth-only'` view of `depth24plus-stencil8` is an attachment with
 * no stencil plane, exactly as a `depth32float` one is, and answering from the
 * format alone would set stencil ops on it.
 *
 * Neither fact survives into the `GPUTextureView` — WebGPU puts no `format` or
 * `aspect` on a view object — so a caller that wants this must ask while it
 * still has the descriptor, which is what {@link Replayer} does at creation.
 *
 * A format with no row answers no planes. That is a refusal
 * {@link webgpuTextureFormatFor} has already made by the time any view exists,
 * so it is unreachable through a created view rather than a case with a
 * meaningful answer.
 *
 * @param {string} format A `crcbl_hal::Format` name, as `gpu-stream.js` decodes
 *   it.
 * @param {string} aspect The `GPUTextureAspect` the view was created with, as
 *   {@link webgpuTextureAspectFor} answers one.
 * @returns {{ depth: boolean, stencil: boolean }}
 */
export function attachmentPlanesFor(format, aspect) {
  const row = TEXTURE_FORMAT[format];
  return {
    depth: (row?.depth ?? false) && aspect !== 'stencil-only',
    stencil: (row?.stencil ?? false) && aspect !== 'depth-only',
  };
}

/**
 * `crcbl_hal::ImageType` as `GPUTextureDescriptor.dimension`.
 *
 * One to one, and the *other* half of the pair is where the care goes:
 * `Extent3d::depth_or_layers` is the depth for `D3` and the array-layer count
 * otherwise, and `GPUExtent3D.depthOrArrayLayers` is defined with exactly that
 * rule — the depth of a `'3d'` texture, the layer count of a `'1d'` or `'2d'`
 * one. **So the pass-through is a real translation and not a coincidence**: both
 * vocabularies fold the same two meanings into one number and split them on the
 * same field, which is why the byte that says which is the one
 * `image_type_from_code` refuses to guess at.
 *
 * The two rules differ in one place, and it is a narrowing rather than a
 * disagreement: WebGPU permits no array layers on a `'1d'` texture at all, so a
 * `D1` image asking for more than one is refused by the browser. Nothing here
 * refuses it, because that is a device rule about a descriptor rather than
 * something this seam cannot express — the same judgement `#createBuffer` makes
 * about a size the device will not allocate.
 */
const TEXTURE_DIMENSION = Object.freeze({ D1: '1d', D2: '2d', D3: '3d' });

/**
 * `crcbl_hal::ImageViewType` as `GPUTextureViewDescriptor.dimension`.
 *
 * One to one again, and this time WebGPU has every one of the six — including
 * the two pairs `image_view_type_from_code` warns about, which is what makes a
 * decoder that folded one into its neighbour produce a *valid* view of the wrong
 * shape rather than a refusal.
 *
 * **THE LAYER COUNT IS PART OF THE DIMENSION HERE**, which is why this table is
 * never read without {@link Replayer#createImageView} resolving the range beside
 * it. WebGPU validates `arrayLayerCount` against the dimension: exactly `1` for
 * `'1d'`, `'2d'` and `'3d'`, exactly `6` for `'cube'`, and a multiple of `6` for
 * `'cube-array'`. A range that disagrees is refused by the browser — and the
 * defaults it applies when the count is absent are dimension-aware in the same
 * way, which is the reason `ImageSubresourceRange::ALL` must reach it as an
 * absence. See {@link subresourceCount}.
 */
const VIEW_DIMENSION = Object.freeze({
  D1: '1d',
  D2: '2d',
  D2Array: '2d-array',
  Cube: 'cube',
  CubeArray: 'cube-array',
  D3: '3d',
});

/**
 * `ImageSubresourceRange::ALL`, which is `u32::MAX` on the wire.
 *
 * `docs/plan/41-webgpu-stream.md` fixes the rule this is half of: a sentinel
 * meaning "all of it" crosses verbatim, because resolving one is answering a
 * question only the replayer has the information to answer.
 */
const SUBRESOURCE_ALL = 0xffff_ffff;

/**
 * A `mip_count` or `layer_count` as `GPUTextureViewDescriptor` wants it.
 *
 * **THE SENTINEL BECOMES AN ABSENCE, WHICH IS HOW WEBGPU SPELLS "THE REST".**
 * The wire carries {@link SUBRESOURCE_ALL} and this is the one place with enough
 * information to resolve it — and the resolution is not a subtraction. Passing
 * `4294967295` on would be refused outright, and computing
 * `texture.mipLevelCount - baseMipLevel` here would be right for the mip count
 * and wrong for the layers: WebGPU's default for an absent `arrayLayerCount`
 * depends on the view's *dimension*, and is `6` for a `'cube'` and `1` for a
 * `'2d'` however many layers the texture has. Omitting the member is what gets
 * every one of those, and it is the only thing that does.
 *
 * @param {number} count
 * @returns {number | undefined} `undefined` for the sentinel, which is what a
 *   caller spreads into a descriptor as an absent member.
 */
function subresourceCount(count) {
  return count === SUBRESOURCE_ALL ? undefined : count;
}

/**
 * A copy's `GPUOrigin3D`, from a `crcbl_hal::ImageSubresourceLayers` and the
 * `crcbl_hal::Offset3d` beside it.
 *
 * **THE ARRAY LAYER IS `origin.z`, AND IT ARRIVES IN THE SUBRESOURCE.** The
 * seam spells a copy the way Vulkan does — `VkBufferImageCopy` names the layer
 * in `imageSubresource.baseArrayLayer` and leaves `imageOffset.z` for a 3D
 * image's depth — while WebGPU has one `z` carrying both. So the two are added
 * here, exactly as `crcbl-wgpu`'s `texture_info` adds them, and the two
 * backends stay one mapping rather than two.
 *
 * Dropping the layer does not fail: every copy lands on layer 0, the last one
 * wins, and an array page reads back as its final layer everywhere. That is a
 * picture, not an error, and no validation message names it.
 *
 * @param {{ baseLayer: number }} subresource
 * @param {{ x: number, y: number, z: number }} offset
 * @returns {{ x: number, y: number, z: number }}
 */
function copyOrigin(subresource, offset) {
  return { x: offset.x, y: offset.y, z: offset.z + subresource.baseLayer };
}

// ─────────────────────────────────────────────────────────────────────────────
// A sampler's vocabulary, in WebGPU's
// ─────────────────────────────────────────────────────────────────────────────
//
// NINE SEAM FIELDS AND ONE WEBGPU DESCRIPTOR, and this is the descriptor where
// the two vocabularies disagree about *types* rather than about names.
// `GPUSamplerDescriptor` has a member for every one of `crcbl_hal::SamplerDesc`'s
// fields, and three of the nine arrive as something WebGPU cannot hold:
//
//   * `address_mode` has a fourth variant WebGPU has no word for at all;
//   * `lod_max` carries a *sentinel* whose WebGPU spelling is not an absence;
//   * `anisotropy` is a float where WebGPU wants an integer, and an integer
//     whose validity depends on the three filters beside it.
//
// The tables and the two functions below are where each of those is written down
// rather than smoothed over, in the order `SamplerDesc` declares them.
//
// AND WHAT COMES BACK REPORTS NOTHING. A `GPUBuffer` reports its size and usage
// and a `GPUTexture` reports nine members; a `GPUSampler` reports its `label` and
// nothing else — no filters, no address modes, no clamps. So every decision here
// is one no inspection of the result can check afterwards, and the only two
// things that can: the browser refusing the descriptor on the device's error
// channel, and `web/tools/gpu-replay.mjs` reading the descriptor this file built
// before it is handed over.

/**
 * `crcbl_hal::FilterMode` as WebGPU spells it.
 *
 * One to one, and read three times per sampler — `magFilter`, `minFilter` and
 * `mipmapFilter`. **The third member is not called `mipFilter`**, which is the
 * one place a name can be typed straight through from the seam and be wrong:
 * WebGPU spells it `mipmapFilter`, and a `GPUSamplerDescriptor` with an
 * unrecognised member is not an error — WebIDL ignores it and the sampler is
 * built with the *default* mipmap filter instead, which is `'nearest'`. That is
 * a trilinear sampler quietly becoming bilinear, on every mip transition, with
 * nothing reporting anything.
 */
const SAMPLER_FILTER = Object.freeze({ Nearest: 'nearest', Linear: 'linear' });

/**
 * `crcbl_hal::SamplerAddressMode` as WebGPU spells it, where it has a spelling.
 *
 * THREE OF THE FOUR HAVE ONE. `GPUAddressMode` is `'clamp-to-edge'`, `'repeat'`
 * and `'mirror-repeat'`, and those are exactly `ClampToEdge`, `Repeat` and
 * `MirrorRepeat`.
 *
 * **`ClampToBorder` IS THE ONE WITH NO ROW, AND IT IS REFUSED RATHER THAN
 * DROPPED**, which is `BufferUsage::DEVICE_ADDRESS`'s and `ImageUsage::PRESENT`'s
 * decision applied to an enum instead of a flag. WebGPU has no border colour at
 * all: there is no `GPUAddressMode` for it and no member to put one in, so there
 * is nothing to ask for. The quiet answers are both worse than a refusal, and
 * worse here than for those two flags, because the nearest neighbour looks
 * right: `'clamp-to-edge'` agrees with a transparent-black border everywhere
 * except the edge texel, so an atlas sampled with it bleeds its neighbour's
 * colour into every seam, on every frame, and the failure is a fringe a person
 * has to see rather than anything a log could carry.
 *
 * NOTHING GOES THE OTHER WAY: every `GPUAddressMode` has a seam variant above.
 * WebGPU has no `'mirror-clamp-to-edge'` either — that is a Vulkan and D3D12
 * mode, and `crcbl_hal::SamplerAddressMode` does not declare one, so there is
 * nothing on this seam for it to be missing from.
 */
const SAMPLER_ADDRESS_MODE = Object.freeze({
  Repeat: 'repeat',
  MirrorRepeat: 'mirror-repeat',
  ClampToEdge: 'clamp-to-edge',
});

/**
 * `crcbl_hal::CompareOp` as WebGPU spells it.
 *
 * All eight, one to one: `GPUCompareFunction` has exactly the seam's set. The
 * only translation is the punctuation — `LessOrEqual` is `'less-equal'` and not
 * `'less-or-equal'`, and a string WebGPU does not have is a `TypeError` out of
 * `createSampler` rather than a silent default, because this member is an enum
 * rather than an ignorable dictionary key.
 *
 * **`Greater` and `Less` are the pair to check twice.** `crcbl_hal::CompareOp`
 * names the comparison performed rather than what it means for visibility, and
 * under this engine's reversed-Z it is `Greater` that asks "is the fragment
 * closer than the stored caster?" A shadow sampler built with `'less'` instead
 * lights exactly the surfaces that should be in shadow and shadows the rest,
 * every frame; the browser reports nothing, because both are valid, and the
 * `GPUSampler` that comes back cannot be asked which it got.
 */
const SAMPLER_COMPARE_FUNCTION = Object.freeze({
  Never: 'never',
  Less: 'less',
  Equal: 'equal',
  LessOrEqual: 'less-equal',
  Greater: 'greater',
  NotEqual: 'not-equal',
  GreaterOrEqual: 'greater-equal',
  Always: 'always',
});

/**
 * `crcbl_hal::PrimitiveTopology` as `GPUPrimitiveTopology`.
 *
 * One to one, and the only translation is the punctuation — `TriangleList` is
 * `'triangle-list'`. A string WebGPU does not have is a `TypeError` out of
 * `createRenderPipeline` rather than a silent default.
 */
const PRIMITIVE_TOPOLOGY = Object.freeze({
  PointList: 'point-list',
  LineList: 'line-list',
  LineStrip: 'line-strip',
  TriangleList: 'triangle-list',
  TriangleStrip: 'triangle-strip',
});

/** `crcbl_hal::FrontFace` as `GPUFrontFace`. */
const FRONT_FACE = Object.freeze({ Ccw: 'ccw', Cw: 'cw' });

/**
 * `crcbl_hal::CullMode` as `GPUCullMode`. `None` is `'none'` — the string, not
 * the absence — which is why this is a table rather than an omitted member.
 */
const CULL_MODE = Object.freeze({ None: 'none', Front: 'front', Back: 'back' });

/** `crcbl_hal::StencilOp` as `GPUStencilOperation`. */
const STENCIL_OPERATION = Object.freeze({
  Keep: 'keep',
  Zero: 'zero',
  Replace: 'replace',
  Invert: 'invert',
  IncrementClamp: 'increment-clamp',
  DecrementClamp: 'decrement-clamp',
  IncrementWrap: 'increment-wrap',
  DecrementWrap: 'decrement-wrap',
});

/** `crcbl_hal::BlendFactor` as `GPUBlendFactor`. */
const BLEND_FACTOR = Object.freeze({
  Zero: 'zero',
  One: 'one',
  Src: 'src',
  OneMinusSrc: 'one-minus-src',
  SrcAlpha: 'src-alpha',
  OneMinusSrcAlpha: 'one-minus-src-alpha',
  Dst: 'dst',
  OneMinusDst: 'one-minus-dst',
  DstAlpha: 'dst-alpha',
  OneMinusDstAlpha: 'one-minus-dst-alpha',
});

/** `crcbl_hal::BlendOp` as `GPUBlendOperation`. */
const BLEND_OPERATION = Object.freeze({
  Add: 'add',
  Subtract: 'subtract',
  ReverseSubtract: 'reverse-subtract',
  Min: 'min',
  Max: 'max',
});

/**
 * Each `crcbl_hal::ColorWrites` channel as its `GPUColorWrite` bit.
 *
 * `gpu-stream.js` decodes the mask to a list of channel names in ascending bit
 * order; this maps each back to the flag WebGPU's `writeMask` is a bitmask of.
 * The seam's `ALL` is `R | G | B | A`, which reduces to `0xF` here rather than
 * needing its own row.
 */
const COLOR_WRITE_BIT = Object.freeze({ R: 1, G: 2, B: 4, A: 8 });

/**
 * The largest integer a `GPUDepthBias` (`[EnforceRange] long`, an `i32`) holds.
 *
 * `crcbl_hal::DepthBias::constant` is an `f32` on the seam but WebGPU's
 * `depthBias` is an integer, so a value outside `i32` — or a fractional one —
 * cannot be passed as-is: WebIDL's `[EnforceRange]` conversion of either throws
 * a `TypeError` synchronously out of `createRenderPipeline`. Refused here with
 * the value named instead. See {@link Replayer#createGraphicsPipeline}.
 */
const MAX_DEPTH_BIAS = 0x7fff_ffff;
const MIN_DEPTH_BIAS = -0x8000_0000;

/**
 * The largest `maxAnisotropy` a `GPUSize32` carries.
 *
 * WebIDL converts a `double` to an `unsigned long` **modularly** unless the
 * member is annotated otherwise, and `GPUSamplerDescriptor.maxAnisotropy` is
 * not: a value past this wraps rather than being clamped or refused, so
 * `4294967297` reaches the device as `1` and `f32::MAX` reaches it as something
 * with no relation to what was asked. {@link webgpuMaxAnisotropyFor} is what
 * stops that conversion ever happening.
 */
const MAX_ANISOTROPY = 0xffff_ffff;

/**
 * The `GPUAddressMode`s a decoded `SamplerDesc::address_mode` names, or why it
 * names none.
 *
 * {@link webgpuTextureFormatFor}'s shape — `{ value, reason }`, with `reason` a
 * phrase for a message a person reads and `null` exactly when the value is not.
 * The three are resolved together rather than one at a time because the refusal
 * has to say *which axis* carried the mode WebGPU cannot express: all three are
 * the same enum, so "ClampToBorder" alone would leave a reader guessing.
 *
 * @param {readonly string[]} modes The three `crcbl_hal::SamplerAddressMode`
 *   variant names, for U, V and W in that order, as `gpu-stream.js` decodes
 *   them.
 * @returns {{ modes: string[] | null, reason: string | null }}
 */
export function webgpuAddressModesFor(modes) {
  const axes = ['U', 'V', 'W'];
  const named = [];
  const refused = [];
  for (const [at, mode] of modes.entries()) {
    const name = SAMPLER_ADDRESS_MODE[mode];
    if (name === undefined) refused.push(`${axes[at] ?? at} is ${mode}`);
    else named.push(name);
  }
  if (refused.length > 0) {
    return {
      modes: null,
      reason:
        `addresses ${refused.join(', ')}, which is no GPUAddressMode: WebGPU has ` +
        "'repeat', 'mirror-repeat' and 'clamp-to-edge' and no border colour at all",
    };
  }
  if (named.length !== axes.length) {
    return {
      modes: null,
      reason: `names ${named.length} address mode(s) rather than one per axis`,
    };
  }
  return { modes: named, reason: null };
}

/**
 * The `maxAnisotropy` a decoded `SamplerDesc::anisotropy` amounts to, or why it
 * amounts to none.
 *
 * **A FLOAT MEETING A `GPUSize32`, WHICH IS THE WHOLE OF THIS TRANSLATION.**
 * `crcbl_hal::SamplerDesc::anisotropy` is an `f32` where `1.0` disables, which
 * is Vulkan's and Metal's shape; WebGPU's `maxAnisotropy` is an integer with a
 * floor of `1`, and WebIDL's conversion to it is modular rather than clamping —
 * see {@link MAX_ANISOTROPY}. So every case is decided here rather than left to
 * the browser:
 *
 *   * **Not finite** — refused. A NaN or an infinity is not a ratio, and WebIDL
 *     would answer `0`, which then fails WebGPU's own floor with a message
 *     naming neither the field nor the value.
 *   * **Below `1`** — refused. `1.0` is the seam's own floor, documented as
 *     "disables", and WebGPU's is the same number; there is nothing below it a
 *     caller could mean. Clamping up would hide the bug rather than fix it.
 *   * **Fractional** — floored, not rounded. WebGPU cannot carry the fraction
 *     and rounding *up* would ask the device for more filtering than the caller
 *     did; flooring can only ask for less, which is the direction a clamp
 *     already goes. `1.9` therefore disables anisotropy, exactly as `1.0` does.
 *   * **Past {@link MAX_ANISOTROPY}** — refused, with the number written out,
 *     which is the judgement `#createBuffer` makes about a size past
 *     `Number.MAX_SAFE_INTEGER`. This is the case `f32::MAX` lands in, and it is
 *     why `lod_max`'s sentinel has nothing to do with this field.
 *   * **Above `1` with a filter that is not `'linear'`** — refused, and this is
 *     the only rule here that is WebGPU's rather than arithmetic.
 *     `GPUSamplerDescriptor` requires all three of `magFilter`, `minFilter` and
 *     `mipmapFilter` to be `'linear'` when `maxAnisotropy` is above `1`, and a
 *     descriptor that breaks it is a validation error the browser answers with a
 *     `GPUSampler` object and a message a turn of the event loop later — so the
 *     handle would be filled in with an invalid sampler and every draw using it
 *     would fail again, none of them naming the creation that was wrong. That is
 *     {@link webgpuTextureFormatFor}'s argument, unchanged.
 *
 * **Above `1` with all three filters linear is passed on**, and the clamp is the
 * device's. {@link halLimitsFor} reports `max_sampler_anisotropy` as `1` because
 * there is no ceiling this backend can *guarantee* — WebGPU accepts a larger ask
 * and clamps it to whatever the device does, and reports that number nowhere —
 * which is a different claim from "more than one is refused". Flattening the ask
 * to `1` here would make the two claims the same and would cost every caller its
 * anisotropic filtering silently.
 *
 * @param {number} anisotropy The `f32` as it came off the wire.
 * @param {readonly string[]} filters The three `GPUFilterMode`s already
 *   translated, in `mag`, `min`, `mipmap` order.
 * @returns {{ maxAnisotropy: number | null, reason: string | null }}
 */
export function webgpuMaxAnisotropyFor(anisotropy, filters) {
  if (!Number.isFinite(anisotropy)) {
    return {
      maxAnisotropy: null,
      reason: `asks for anisotropy ${anisotropy}, which is not a ratio`,
    };
  }
  if (anisotropy < 1) {
    return {
      maxAnisotropy: null,
      reason:
        `asks for anisotropy ${anisotropy}, which is below the 1.0 that ` +
        'SamplerDesc documents as disabling it and below the floor WebGPU validates',
    };
  }
  const value = Math.floor(anisotropy);
  if (value > MAX_ANISOTROPY) {
    return {
      maxAnisotropy: null,
      reason: `asks for anisotropy ${anisotropy}, which is past the largest maxAnisotropy a GPUSize32 carries`,
    };
  }
  if (value > 1 && filters.some((filter) => filter !== 'linear')) {
    return {
      maxAnisotropy: null,
      reason:
        `asks for anisotropy ${anisotropy} with filters ${filters.join(', ')}, and ` +
        "WebGPU allows a maxAnisotropy above 1 only when all three are 'linear'",
    };
  }
  return { maxAnisotropy: value, reason: null };
}

// ─────────────────────────────────────────────────────────────────────────────
// A bind-group layout's vocabulary, in WebGPU's
// ─────────────────────────────────────────────────────────────────────────────
//
// THE TRANSLATION WITH THE MOST THE FAR SIDE CANNOT SAY, and the first where
// what a field means is *which member exists* rather than what a member holds.
// `crcbl_hal::BindGroupLayoutEntry` has five fields and
// `GPUBindGroupLayoutEntry` has `binding`, `visibility` and exactly one of
// `buffer`, `sampler`, `texture`, `storageTexture` or `externalTexture` — so
// three of the seam's five fields land somewhere WebGPU has no room for at all:
//
//   * `count` — WebGPU CORE HAS NO BINDING ARRAYS. There is no `count` member on
//     a layout entry, and no `GPUBindGroup` syntax for filling one. Every count
//     but `1` is refused by {@link Replayer#createBindGroupLayout}, the
//     `u32::MAX` sentinel loudest of all.
//   * `flags` — all three `BindingFlags` require
//     `Features::DESCRIPTOR_INDEXING`, which this backend never reports because
//     WebGPU has no bindless model. Any flag at all is refused.
//   * `visibility` — three of the seam's five `ShaderStages` have a
//     `GPUShaderStage` bit; `MESH` and `TASK` have none, because WebGPU has no
//     mesh pipeline. Refused rather than dropped, which is the `PRESENT` usage
//     bit's decision applied to a stage.
//
// EACH OF THOSE IS A REFUSAL AND NOT A NARROWING, and the `count` one is the
// case worth stating on its own: silently accepting a bindless declaration and
// creating a single-descriptor binding would be the worst outcome available,
// because every later write to slot 1 upward would target a descriptor that does
// not exist and the browser would name the *bind group* rather than the layout
// that was wrong.
//
// WHAT THIS FILE DOES NOT CHECK, stated here so nobody assumes it does.
// `BindGroupLayoutDesc::check_entries` in `crcbl-hal` enforces the seam's own
// rules — a zero `count`, a binding number declared twice, the `VARIABLE_COUNT`
// ordering, the bindless ceiling, a stage the device has not got — and **this
// replayer cannot call it**: it has no `DeviceCaps` and no `crcbl-hal`. So the
// division is:
//
//   * The **ordering rule** — a `VARIABLE_COUNT` entry must be both last in the
//     slice and highest-numbered — is not re-checked here, and needs no
//     re-check, because `VARIABLE_COUNT` is refused outright above: no layout
//     carrying it ever reaches `createBindGroupLayout`, so there is no position
//     left to get wrong. What this file *does* keep is the order itself —
//     `gpu-stream.js` decodes the entries in slice order and the loop below
//     preserves it — because the encoder's caller is entitled to have run
//     `check_entries` against the list it wrote.
//   * **Duplicate binding numbers** are left to the browser, deliberately.
//     WebGPU validates them itself and reports on `uncapturederror`, which is
//     the same queue `Device::take_error` drains, so the refusal is real and
//     lands where a person will read it. Re-checking here would duplicate a rule
//     that is already enforced twice — by `check_entries` before the encode and
//     by the browser after the replay — and a third copy is a third thing to
//     drift.
//
// AND WHAT COMES BACK REPORTS ITS LABEL AND NOTHING ELSE, exactly as a
// `GPUSampler` does: a `GPUBindGroupLayout` exposes no entries, no bindings and
// no visibility. So every decision here is one no inspection of the result can
// check afterwards, and the two things that can are the browser refusing the
// descriptor on the device's error channel and `web/tools/gpu-replay.mjs`
// reading the descriptor this file built before it is handed over.

/**
 * The `GPUShaderStage` bits, from the WebGPU specification.
 *
 * Written out rather than read off the global, because this file is driven under
 * node where there is no `GPUShaderStage` at all — {@link GPU_BUFFER_USAGE}'s
 * reason exactly, and `browser-e2e.mjs` is what holds these three against a real
 * browser's own namespace object.
 */
const GPU_SHADER_STAGE = Object.freeze({
  VERTEX: 0x1,
  FRAGMENT: 0x2,
  COMPUTE: 0x4,
});

/**
 * Every `crcbl_hal::ShaderStages` flag with a `GPUShaderStage` bit behind it.
 *
 * TWO OF THE FIVE ARE MISSING AND ARE REFUSED RATHER THAN DROPPED. `MESH` and
 * `TASK` are stages WebGPU does not have — there is no mesh pipeline, and
 * `crcbl_hal::MeshPipelineDesc` has no WebGPU counterpart to be visible to — so
 * a binding declared visible to one of them cannot be built here at all. Dropping
 * the bit would produce a layout narrower than the caller asked for, and a
 * narrower layout does not fail at creation: it fails at the draw, as a shader
 * reading a binding the pipeline layout says it may not see.
 */
const SHADER_STAGE_MAP = Object.freeze({
  VERTEX: GPU_SHADER_STAGE.VERTEX,
  FRAGMENT: GPU_SHADER_STAGE.FRAGMENT,
  COMPUTE: GPU_SHADER_STAGE.COMPUTE,
});

/**
 * The `GPUShaderStageFlags` word a decoded `visibility` amounts to, and whatever
 * in it WebGPU has no bit for.
 *
 * {@link webgpuTextureUsageFor}'s twin, in shape and in what `unsatisfiable`
 * means: seam names rather than a word, because it is for a message a person
 * reads, and empty for every visibility this backend can honour.
 *
 * @param {readonly string[]} visibility `crcbl_hal::ShaderStages` flag names, as
 *   `gpu-stream.js` decodes them.
 * @returns {{ bits: number, unsatisfiable: string[] }}
 */
export function webgpuShaderStageFor(visibility) {
  let bits = 0;
  const unsatisfiable = [];
  for (const name of visibility) {
    const bit = SHADER_STAGE_MAP[name];
    if (bit === undefined) unsatisfiable.push(`ShaderStages::${name}`);
    else bits |= bit;
  }
  return { bits, unsatisfiable };
}

/**
 * `crcbl_hal::SampleType` as `GPUTextureBindingLayout.sampleType`.
 *
 * `Float` is `'float'` — filterable — rather than `'unfilterable-float'`,
 * because that is what the variant means: `SampleType`'s own docs call it
 * "ordinary filterable colour texels". `Depth` is `'depth'`, which is the row
 * that makes this table load-bearing: a depth-format view is bindable only
 * through a slot that says `'depth'`, and a comparison sampler is bindable only
 * beside one.
 *
 * WebGPU's other two — `'sint'` and `'uint'` — have no seam variant, which is
 * `SampleType`'s own decision rather than a gap here: "integer and multisampled
 * sampled images are things no shader in this engine declares, and a variant
 * nothing constructs is a variant no backend's mapping was ever checked
 * against."
 */
const SAMPLE_TYPE = Object.freeze({ Float: 'float', Depth: 'depth' });

/**
 * The `GPUStorageTextureAccess` a `BindingKind::StorageImage`'s `read_only`
 * names.
 *
 * **A `bool` meeting a three-valued enum, and the missing value is the one the
 * seam arguably means.** WebGPU spells the access `'write-only'`, `'read-only'`
 * or `'read-write'`; the seam carries one flag, so `read_only: true` is
 * `'read-only'` and `false` is `'write-only'`. `'read-write'` is unreachable
 * from here, and that is a deliberate narrowing rather than an oversight, so say
 * what it costs: `crcbl_hal::BindingKind::StorageImage` calls itself "a
 * read/write storage image", so a `false` permits a shader that reads as well as
 * writes, and this maps it to a layout that permits only writing.
 *
 * It is the safe direction of the two. WebGPU allows `'read-write'` on a much
 * shorter format list than the storage list itself — `r32uint`, `r32sint` and
 * `r32float` in core — so mapping `false` to it would refuse the `rgba8unorm`
 * and `rgba16float` layouts this seam actually asks for. What the narrowing
 * costs instead is a *loud* failure and never a silent one: a WGSL module that
 * reads through a `write` binding is rejected at pipeline creation naming the
 * binding, so nothing reads garbage. Widening it means a second seam field
 * distinguishing "writes" from "reads and writes", which nothing declares yet.
 *
 * Keyed by the two booleans as strings, so a value that is neither is
 * `undefined` rather than falling through to whichever member `false` indexes.
 */
const STORAGE_TEXTURE_ACCESS = Object.freeze({
  true: 'read-only',
  false: 'write-only',
});

/**
 * The `GPUTextureViewDimension`s a storage texture may not be.
 *
 * WebGPU validates a `GPUStorageTextureBindingLayout` by rejecting `'cube'` and
 * `'cube-array'` outright — a cube face is addressed through a sampler's
 * direction vector, and a storage write takes integer texel coordinates — while
 * accepting the other four. Refused here by name for
 * {@link webgpuTextureFormatFor}'s reason: the browser answers a bad layout with
 * an object and a validation error a turn of the event loop later, so passing it
 * on files an invalid layout under the handle and every bind group made against
 * it fails again, none of them naming the layout that was wrong.
 */
const STORAGE_TEXTURE_FORBIDDEN_DIMENSIONS = Object.freeze([
  'cube',
  'cube-array',
]);

/**
 * The `count` that means "as many descriptors as this device can", which is
 * `u32::MAX` on the wire.
 *
 * `BindGroupLayoutEntry::count` documents it as the value the portable bindless
 * declaration is written with, because a caller cannot know a device's ceiling
 * before it opens one, and every backend resolves it through `resolved_count`.
 * **This backend resolves it to a refusal**, which is a resolution and not an
 * omission: WebGPU has no binding arrays, so there is no number it could become.
 */
const BINDING_COUNT_DEVICE_MAX = 0xffff_ffff;

/**
 * `BindingResource::WHOLE_BUFFER`, which is `u64::MAX` on the wire.
 *
 * **THE SENTINEL BECOMES AN ABSENCE, AND THIS TIME THAT IS THE RIGHT
 * RESOLUTION** — the opposite of `lod_max`, which is why the rule
 * `docs/plan/41-webgpu-stream.md` sets is that the *encoder* never resolves and
 * the replayer works it out per field. WebGPU's absent `GPUBufferBinding.size`
 * means "to the end of the buffer", which is exactly what `WHOLE_BUFFER` means,
 * so omitting the member is the faithful translation. `lodMaxClamp` absent means
 * a *number* rather than "the rest", so there the sentinel had to be written out;
 * here writing `18446744073709551615` out would be refused by the browser
 * outright. A `BigInt` because it is a `u64` and a `Number` would round it.
 */
const BUFFER_BINDING_WHOLE = 0xffff_ffff_ffff_ffffn;

/**
 * The largest byte offset or size a `GPUSize64` carries exactly.
 *
 * A buffer binding's `offset` and `size` are `u64` on the wire and
 * `GPUSize64`s — JavaScript numbers — in WebGPU, so a value past this would be
 * passed on rounded, binding a range nobody asked for. Refused with the number
 * written out, which is `#createBuffer`'s judgement about a size no `GPUSize64`
 * holds exactly.
 */
const MAX_BUFFER_BINDING = BigInt(Number.MAX_SAFE_INTEGER);

/**
 * The `GPUBindGroupLayoutEntry` member a decoded `BindingKind` becomes, or why
 * it becomes none.
 *
 * **A MEMBER AND NOT A VALUE**, which is the whole shape of this translation and
 * the reason it is not a table of strings. WebGPU has no flat binding type: an
 * entry carries exactly one of `buffer`, `sampler`, `texture`, `storageTexture`
 * or `externalTexture`, each an object with its own fields, so this decides
 * *which member exists* as well as what goes in it. The five seam variants land
 * as:
 *
 *   * `UniformBuffer` → `buffer: { type: 'uniform', hasDynamicOffset }`. The
 *     dynamic offset is the seam's substitute for push constants, which WebGPU
 *     has none of, so it is the member that matters most here.
 *   * `StorageBuffer` → `buffer: { type: 'read-only-storage' | 'storage',
 *     hasDynamicOffset }`. `read_only` is a *type* in WebGPU rather than a flag
 *     beside one.
 *   * `SampledImage` → `texture: { sampleType, viewDimension, multisampled:
 *     false }`. Both of the seam's fields are here because WebGPU puts them in
 *     the layout, which is why `crcbl_hal::BindingKind::SampledImage` carries
 *     them at all. `multisampled` is written explicitly and always `false`:
 *     `SampleType` has no multisampled variant — deliberately, per its own docs
 *     — so there is nothing this seam could set it from, and an omitted member
 *     would be the same `false` arrived at by accident.
 *   * `Sampler` → `sampler: { type: 'comparison' | 'filtering' }`. WebGPU's
 *     third value, `'non-filtering'`, has no seam variant.
 *   * `StorageImage` → `storageTexture: { access, format, viewDimension }`.
 *     **This used to be the one WebGPU could not express**, and the gap was the
 *     seam's descriptor rather than the API's:
 *     `GPUStorageTextureBindingLayout.format` is a *required* member with no
 *     default, and `crcbl_hal::BindingKind::StorageImage` carried `read_only`
 *     and nothing else, because Vulkan, Metal and D3D12 take the format off the
 *     bound view. The variant now names a `view_type` and a `format` for the
 *     same reason `SampledImage` does, so all three members come off the wire
 *     and none is guessed. `access` is the one that is *derived*: the seam has a
 *     flag where WebGPU has three words — see {@link STORAGE_TEXTURE_ACCESS}.
 *     Two shapes are still refused rather than passed on, each named: a format
 *     WebGPU does not allow as a storage texture, and the two view dimensions it
 *     forbids.
 *
 * `externalTexture` is the fifth WebGPU member and nothing on this seam maps to
 * it: a `GPUExternalTexture` is a video frame, which `crcbl-hal` has no concept
 * of.
 *
 * @param {{ name: string, dynamic?: boolean, readOnly?: boolean,
 *           viewType?: string, sampleType?: string, format?: string,
 *           comparison?: boolean }} kind
 *   A `crcbl_hal::BindingKind`, as `gpu-stream.js` decodes it.
 * @returns {{ layout: object | null, reason: string | null }} `reason` is a
 *   phrase for the message a person reads, and is `null` exactly when `layout`
 *   is not.
 */
export function webgpuBindingLayoutFor(kind) {
  switch (kind.name) {
    case 'UniformBuffer':
      return {
        layout: {
          buffer: { type: 'uniform', hasDynamicOffset: kind.dynamic },
        },
        reason: null,
      };
    case 'StorageBuffer':
      return {
        layout: {
          buffer: {
            type: kind.readOnly ? 'read-only-storage' : 'storage',
            hasDynamicOffset: kind.dynamic,
          },
        },
        reason: null,
      };
    case 'SampledImage': {
      const viewDimension = VIEW_DIMENSION[kind.viewType];
      if (viewDimension === undefined) {
        return {
          layout: null,
          reason: `is a SampledImage of ImageViewType::${kind.viewType}, which is no GPUTextureViewDimension`,
        };
      }
      const sampleType = SAMPLE_TYPE[kind.sampleType];
      if (sampleType === undefined) {
        return {
          layout: null,
          reason: `is a SampledImage of SampleType::${kind.sampleType}, which is no GPUTextureSampleType`,
        };
      }
      return {
        layout: {
          texture: { sampleType, viewDimension, multisampled: false },
        },
        reason: null,
      };
    }
    case 'StorageImage': {
      const viewDimension = VIEW_DIMENSION[kind.viewType];
      if (viewDimension === undefined) {
        return {
          layout: null,
          reason: `is a StorageImage of ImageViewType::${kind.viewType}, which is no GPUTextureViewDimension`,
        };
      }
      if (STORAGE_TEXTURE_FORBIDDEN_DIMENSIONS.includes(viewDimension)) {
        return {
          layout: null,
          reason: `is a StorageImage of ImageViewType::${kind.viewType}, which WebGPU spells ${viewDimension} and forbids on a GPUStorageTextureBindingLayout`,
        };
      }
      // The device's features are deliberately not consulted: every row this
      // table marks `storage` is core WebGPU, and the ones it does not are
      // gated behind features `FEATURE_MAP` cannot ask for, so there is no
      // device on which the answer would differ. Refusing by name here is
      // `webgpuTextureFormatFor`'s rule applied to a second member.
      const row = TEXTURE_FORMAT[kind.format];
      if (row === undefined) {
        return {
          layout: null,
          reason: `is a StorageImage of Format::${kind.format}, which this backend has no GPUTextureFormat for`,
        };
      }
      if (row.storage !== true) {
        return {
          layout: null,
          reason: `is a StorageImage of Format::${kind.format}, which WebGPU spells ${row.name} and does not allow as a storage texture`,
        };
      }
      const access = STORAGE_TEXTURE_ACCESS[String(kind.readOnly)];
      if (access === undefined) {
        return {
          layout: null,
          reason: `is a StorageImage whose read_only decoded as ${JSON.stringify(kind.readOnly)}, which is no GPUStorageTextureAccess`,
        };
      }
      return {
        layout: {
          storageTexture: { access, format: row.name, viewDimension },
        },
        reason: null,
      };
    }
    case 'Sampler':
      return {
        layout: {
          sampler: { type: kind.comparison ? 'comparison' : 'filtering' },
        },
        reason: null,
      };
    // A kind with no case at all is this file and `gpu-stream.js`'s BINDING_KIND
    // table having drifted, and is refused for `webgpuTextureFormatFor`'s
    // reason: a nearby member would build a layout describing a different kind
    // of resource, and every bind group made against it would be refused
    // naming the group rather than the layout.
    default:
      return {
        layout: null,
        reason: `is a BindingKind::${kind.name}, which this backend has no GPUBindGroupLayoutEntry member for`,
      };
  }
}

/**
 * A `GPUAdapter`'s or `GPUDevice`'s `limits` in the seam's names and units.
 *
 * BOTH KINDS, for `halFeaturesFor`'s reason and with a sharper consequence: an
 * adapter's limits are the ceilings it *could* grant, and a device's are the
 * ones it was created with — the specification's defaults for every member the
 * request did not name. Reporting the adapter's for a device would promise a
 * texture size a default device refuses, which is what this separation prevents.
 *
 * For the device *this backend* opens the two now coincide by construction,
 * because {@link requiredLimitsFor} asks for every member the adapter reports.
 * That is not a licence to read one off the other: the features still differ —
 * an optional feature the caller did not ask for is not granted — and any device
 * opened with a different descriptor differs on both.
 *
 * A few of the fields have no `GPUSupportedLimits` member behind them, and each
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
  };
}

/**
 * An adapter's own ceilings, as the `requiredLimits` a device is asked for.
 *
 * WHY THE CEILINGS AND NOT THE DEFAULTS. WebGPU is the only backend under this
 * seam that caps *per-stage binding counts*, and it caps them low: eight storage
 * buffers per stage, where `crcbl-render`'s draw-argument pass binds fourteen in
 * one compute layout. Nothing in `crcbl_hal::Limits` can express that number —
 * the seam has no per-stage binding field at all, because Vulkan, Metal and DX12
 * do not need one — so a device opened with the specification's defaults refuses
 * that bind group layout, then its pipeline, then every draw behind it. That is
 * a device this backend cannot render with, whatever the caller asked for.
 *
 * WHY IT IS NOT THIS FILE DECIDING SOMETHING THE CALLER DID NOT. `crcbl-wgpu`
 * opens its device with `adapter.limits()` for the same reason (see
 * `WgpuDevice::request`), so "everything this adapter offers" is the engine's
 * established policy for opening a device rather than a number chosen here.
 * Asking for exactly the adapter's own ceilings is also the one request that
 * cannot be refused over limits: every member of `GPUSupportedLimits` is by
 * definition supported by the adapter reporting it, and `requestDevice` rejects
 * the *whole* request over one that is not — the same hazard `requiredFeatures`
 * has above.
 *
 * NOTHING IS CLAMPED AND NOTHING IS HIDDEN. The device is asked for the ceilings
 * and then reports its own limits back through {@link halLimitsFor}, which is
 * what `Reply::Device` carries and what `crcbl_hal::Device::caps` answers — so a
 * browser that granted less than was asked for says so where the caller already
 * looks, rather than leaving the engine believing it got what it asked for.
 *
 * The keys are the adapter's own, walked rather than listed: `requiredLimits`
 * rejects a key WebGPU does not define, and a hand-written list is a second
 * place for a spec addition to go missing. Non-numeric members are skipped so a
 * future `GPUSupportedLimits` member that is not a number cannot turn a device
 * request into a `TypeError`.
 *
 * @param {{ limits?: object }} adapter The `GPUAdapter` being opened.
 * @returns {Record<string, number>} A `GPUDeviceDescriptor.requiredLimits`.
 */
export function requiredLimitsFor(adapter) {
  /** @type {Record<string, number>} */
  const asked = {};
  const limits = adapter.limits;
  if (!limits) return asked;
  for (const key in limits) {
    const value = limits[key];
    if (typeof value === 'number' && Number.isFinite(value)) asked[key] = value;
  }
  return asked;
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
 * The formats a `GPUCanvasContext` may be configured with, and their seam codes.
 *
 * THE SPECIFICATION'S SET, not this browser's, because there is no query for the
 * latter: `getPreferredCanvasFormat()` answers exactly one of these two and
 * nothing anywhere reports the rest. Both are required of every WebGPU
 * implementation — a canvas configured with either is valid by the specification
 * — so listing both is a fact rather than a guess, and it is the same kind of
 * fact as `MAX_SAMPLE_COUNT` above.
 *
 * `rgba16float` IS DELIBERATELY ABSENT. It is a supported context format in the
 * current specification and was not always one, and nothing a browser reports
 * distinguishes an implementation that takes it from one that does not. A
 * `SurfaceCaps::formats` entry is a promise that a swapchain can be created with
 * it, so an entry that might be refused at `configure()` is worse than a shorter
 * list: the caller has no way to find out which it was.
 *
 * EACH ROW ALSO CARRIES ITS `-srgb` COUNTERPART, AND THAT IS WHAT KEEPS THE
 * BROWSER BUILD FROM BEING DARK. `GPUCanvasContext.configure` refuses an `-srgb`
 * format outright, so a canvas is *configured* linear and always will be — but
 * `GPUCanvasConfiguration.viewFormats` exists exactly so the frames it hands
 * back can be viewed through the sRGB counterpart of the same bytes, and the two
 * differ in nothing else. Every pass above the seam writes display-referred
 * values and leaves the encode to the hardware, so a linear *view* skips the
 * encode and the whole frame presents a transfer function too dark.
 * {@link surfaceCapsFor} offers the counterparts and
 * {@link Replayer#configureSwapchain} is what makes the offer good.
 */
const CANVAS_FORMAT = Object.freeze({
  bgra8unorm: {
    code: FORMAT.BGRA8_UNORM,
    srgb: 'bgra8unorm-srgb',
    srgbCode: FORMAT.BGRA8_UNORM_SRGB,
  },
  rgba8unorm: {
    code: FORMAT.RGBA8_UNORM,
    srgb: 'rgba8unorm-srgb',
    srgbCode: FORMAT.RGBA8_UNORM_SRGB,
  },
});

/**
 * The `GPUCanvasConfiguration.format` each canvas sRGB view format is a view of.
 *
 * Derived from {@link CANVAS_FORMAT} rather than written out a second time: a
 * hand-kept reverse table is one edit away from naming the wrong base format,
 * and a canvas configured `rgba8unorm` and viewed `bgra8unorm-srgb` swaps the
 * red and blue channels of every frame with no error anywhere.
 */
const CANVAS_BASE_FORMAT = Object.freeze(
  Object.fromEntries(
    Object.entries(CANVAS_FORMAT).map(([base, row]) => [row.srgb, base])
  )
);

/**
 * The `GPUCanvasAlphaMode` a decoded `CompositeAlpha` names, or why it names
 * none.
 *
 * TWO OF THE SEAM'S FOUR HAVE A CANVAS SPELLING AND TWO DO NOT. `GPUCanvasContext`
 * offers only `'opaque'` and `'premultiplied'`, which is exactly the pair
 * `surfaceCapsFor` reports a canvas surface accepts — so `Opaque` and
 * `PreMultiplied` map, and `PostMultiplied` and `Inherit` are refused *by name*
 * rather than folded onto the nearest legal value. A silent fold would composite
 * the canvas differently from what was asked with nothing reporting it, the same
 * class of quiet wrong this file refuses everywhere else; and the caller was
 * promised only the two `SurfaceCaps` listed, so a swapchain asking for either of
 * the other two is a far-side bug that has to surface.
 *
 * The decoded name is the reply direction's spelling (`PRE_MULTIPLIED`), because
 * `gpu-stream.js` inverts `gpu-reply.js`'s `COMPOSITE_ALPHA` table to read it.
 *
 * @param {string} compositeAlpha A `crcbl_hal::CompositeAlpha` code's name.
 * @returns {{ mode: string | null, reason: string | null }} `reason` is a phrase
 *   for the message a person reads, and is `null` exactly when `mode` is not.
 */
export function webgpuAlphaModeFor(compositeAlpha) {
  const mode = ALPHA_MODE[compositeAlpha];
  if (mode === undefined) {
    return {
      mode: null,
      reason:
        `asks for CompositeAlpha::${compositeAlpha}, which a GPUCanvasContext has no ` +
        'alphaMode for — a browser canvas offers only opaque and premultiplied',
    };
  }
  return { mode, reason: null };
}

/**
 * `crcbl_hal::CompositeAlpha` code names to the `GPUCanvasAlphaMode` strings, for
 * the two a canvas can express. Keyed by the reply direction's spelling, which is
 * how {@link webgpuAlphaModeFor}'s argument arrives; `POST_MULTIPLIED` and
 * `INHERIT` are absent on purpose, so a lookup answers `undefined` and the
 * swapchain is refused.
 */
const ALPHA_MODE = Object.freeze({
  OPAQUE: 'opaque',
  PRE_MULTIPLIED: 'premultiplied',
});

/**
 * The image count a WebGPU canvas offers, as a range of exactly one.
 *
 * `crcbl_hal::SurfaceCaps` records the decision this implements: the fields are
 * WSI vocabulary and stay, and "a WebGPU backend reports the range its platform
 * actually offers (`2..=2` for a WebGPU canvas, which has one implicit ring)".
 * WebGPU exposes no swapchain image count at all, so this is not a number read
 * off anything — it is the statement that there is exactly one configuration
 * available and a caller's clamp has nothing to choose between. A wider range
 * would offer a knob that does not exist.
 */
const CANVAS_IMAGE_COUNT = 2;

/**
 * What a canvas surface will accept, in the seam's vocabulary.
 *
 * FIELD BY FIELD, BECAUSE ONLY ONE OF THEM IS SOMETHING THE BROWSER SAYS:
 *
 *   * `formats` — `getPreferredCanvasFormat()` answers one format, and its
 *     **sRGB counterpart** is the one to put first. `preferred_format()` takes
 *     the first sRGB entry and falls through to the first entry of all when
 *     there is none, so leading with the counterpart is what hands the engine a
 *     display-referred target: every pass above the seam writes display-referred
 *     values and leaves the encode to the hardware, and a linear one skips the
 *     encode and presents a transfer function too dark. The counterpart is not a
 *     format a canvas can be *configured* with — no `-srgb` one is — and it does
 *     not have to be: {@link Replayer#configureSwapchain} configures the base
 *     format and reaches it through `viewFormats`, so the browser's own
 *     preference is still what the canvas is configured with and there is still
 *     no full-canvas copy per present. The two linear formats follow, because a
 *     canvas can be configured with either and they stay askable; within each
 *     pair the browser's preference leads, for the same "no extra copy" reason
 *     the ordering has always carried.
 *   * `presentModes` — `[FIFO]`, and WebGPU has **no present-mode concept at
 *     all**. This is not a gap filled with a plausible value: a canvas presents
 *     at the `requestAnimationFrame` boundary in lockstep with the display,
 *     which is what `Fifo` describes, and `SurfaceCaps` promises `Fifo` is
 *     always present. Reporting `Mailbox` or `Immediate` would offer a caller a
 *     mode that cannot be selected and would silently be `Fifo` anyway.
 *   * `compositeAlpha` — `GPUCanvasConfiguration.alphaMode` is `'opaque'` or
 *     `'premultiplied'` and those are the two reported, in that order because
 *     `'opaque'` is WebGPU's default and `CompositeAlpha::Opaque` is the
 *     engine's. `PostMultiplied` and `Inherit` have no WebGPU spelling and are
 *     never offered.
 *   * `minImageCount` / `maxImageCount` — see {@link CANVAS_IMAGE_COUNT}. WebGPU
 *     has no image count; this says there is one configuration, not that a ring
 *     of two was measured.
 *   * `currentExtent` — `null`, **and this is a field with no honest answer
 *     here.** WebGPU has no `currentExtent` query. The canvas has `width` and
 *     `height`, and they are the wrong thing twice over: the seam documents this
 *     field as what the *surface* believes, "a cross-check, never the source of
 *     truth", against the shell's own size — and a canvas's size is a number the
 *     page itself set. Answering with it would hand the shell its own request
 *     back as independent confirmation, which is the one thing a cross-check
 *     must never be. Wayland reports nothing here for the same reason and the
 *     seam already spells `None` as the answer for it.
 *
 * @param {GPU} gpu The `navigator.gpu` to ask.
 * @returns {Parameters<ReplyWriter['surfaceCaps']>[0]}
 * @throws {SurfaceCapsError} If the browser names a canvas format this seam has
 *   no `crcbl_hal::Format` for — which is a query that failed, not a surface
 *   that supports nothing.
 */
function surfaceCapsFor(gpu) {
  const preferred = gpu.getPreferredCanvasFormat();
  const preferredRow = CANVAS_FORMAT[preferred];
  if (preferredRow === undefined) {
    throw new SurfaceCapsError(
      SURFACE_CAPS_FAILURE.BACKEND,
      `getPreferredCanvasFormat() answered ${JSON.stringify(preferred)}, ` +
        'which is not a canvas format this seam has a Format for'
    );
  }
  const rest = Object.entries(CANVAS_FORMAT)
    .filter(([name]) => name !== preferred)
    .map(([, row]) => row);
  return {
    formats: [
      // sRGB first — see the doc above: `preferred_format()` takes the first
      // sRGB entry, and this is the one the browser's own base format is viewed
      // through.
      preferredRow.srgbCode,
      ...rest.map((row) => row.srgbCode),
      preferredRow.code,
      ...rest.map((row) => row.code),
    ],
    presentModes: [PRESENT_MODE.FIFO],
    compositeAlpha: [COMPOSITE_ALPHA.OPAQUE, COMPOSITE_ALPHA.PRE_MULTIPLIED],
    minImageCount: CANVAS_IMAGE_COUNT,
    maxImageCount: CANVAS_IMAGE_COUNT,
    currentExtent: null,
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
 * A capability query that cannot be answered.
 *
 * NEITHER A `ReplayError` NOR A `SurfaceError`, and the difference from the
 * second one is the interesting half. A `CreateSurface` that cannot resolve its
 * canvas throws out of `replay`, because that command has no reply on this
 * channel and there is no way to tell the far side at all. This one *does* have
 * a reply, so throwing would be choosing to lose the frame over an answer wasm
 * is waiting for — and `Instance::surface_caps` is the call adapter selection is
 * made of, where "no" is a routine answer rather than a fault. So this is thrown
 * only inside `#surfaceCaps` and never out of it: it is caught there and becomes
 * the `cause` of a `SurfaceCapsFailed` reply.
 *
 * `code` is one of `SURFACE_CAPS_FAILURE`, which is what the far side turns into
 * a `HalError`. There is one such code, and the constructor still takes it
 * rather than assuming it: the table is what the two languages have to agree on,
 * and a throw site that spelled the value itself would agree with nothing.
 */
export class SurfaceCapsError extends Error {
  /**
   * @param {number} code One of `SURFACE_CAPS_FAILURE`.
   * @param {string} message
   */
  constructor(code, message) {
    super(message);
    this.name = 'SurfaceCapsError';
    this.code = code;
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
 * One resource kind's live objects, keyed the way a `crcbl_core::Handle` is.
 *
 * ONE OF THESE PER KIND, NEVER ONE FOR ALL OF THEM. `crcbl-webgpu`'s crate docs
 * and `docs/plan/41-webgpu-stream.md` both say why: a handle carries no kind, so
 * a buffer and a surface can hold the same eight bytes, and the opcode is the
 * only thing that says which table an id indexes. A single table keyed on handle
 * bits would let two kinds stand on each other.
 *
 * A SLOT REMEMBERS THE GENERATION IT WAS FILLED AT, which is the half a plain
 * `Map<index, object>` cannot do. A `Handle` is `{ index, generation }` precisely
 * so that a stale one is detectable: an index is reissued when the resource it
 * named is destroyed, and the generation is what distinguishes the new occupant
 * from the old. So a destroy naming a stale handle finds a slot whose generation
 * has moved on, and is the same no-op an empty slot already is — rather than
 * releasing whatever now lives at that index, which is the failure this class
 * exists to make impossible. A lookup answers `undefined` for the same reason.
 *
 * The rule the seam sets for the empty slot is in `crcbl-webgpu`'s crate docs:
 * `crcbl-render` destroys the handle it pre-allocated even when the creation it
 * belonged to failed, so a destroy naming an id nothing ever created is a legal
 * stream op and not corruption. A stale generation is the same case one step
 * later.
 *
 * @template {object} T
 */
export class HandleTable {
  /**
   * The occupied slots, by index.
   *
   * @type {Map<number, { generation: number, value: T }>}
   */
  #slots = new Map();

  /**
   * Files `value` under `handle`, replacing whatever that index held.
   *
   * Replacing rather than refusing, because a create naming an index that is
   * already occupied is what an id pool does after a destroy this replayer never
   * saw — a frame that was dropped, or a probe that asks twice — and the newer
   * handle is by construction the newer generation.
   *
   * @param {{ index: number, generation: number }} handle
   * @param {T} value
   */
  insert(handle, value) {
    this.#slots.set(handle.index, {
      generation: handle.generation,
      value,
    });
  }

  /**
   * What `handle` names, or `undefined` if its slot is empty or has been
   * reissued since.
   *
   * @param {{ index: number, generation: number }} handle
   * @returns {T | undefined}
   */
  get(handle) {
    const slot = this.#slots.get(handle.index);
    if (slot === undefined || slot.generation !== handle.generation) {
      return undefined;
    }
    return slot.value;
  }

  /**
   * Takes what `handle` names out of the table and hands it back, or answers
   * `undefined` and changes nothing.
   *
   * The `undefined` covers both no-ops at once — an empty slot and a stale
   * generation — so a caller releasing a resource writes one `if` rather than
   * two, and cannot release the live occupant of a reissued index by mistake.
   *
   * @param {{ index: number, generation: number }} handle
   * @returns {T | undefined}
   */
  remove(handle) {
    const slot = this.#slots.get(handle.index);
    if (slot === undefined || slot.generation !== handle.generation) {
      return undefined;
    }
    this.#slots.delete(handle.index);
    return slot.value;
  }

  /** How many slots are occupied. */
  get size() {
    return this.#slots.size;
  }

  /**
   * Every live object with the index it is filed under, in insertion order.
   *
   * The index alone and not the whole handle, because this is for a reader
   * looking at what is *there* — a console, the browser gate — rather than for
   * a lookup, and a lookup has {@link HandleTable#get}, which is the one that
   * must be given a generation to check.
   *
   * @returns {IterableIterator<[number, T]>}
   */
  *entries() {
    for (const [index, slot] of this.#slots) yield [index, slot.value];
  }
}

/**
 * How many out-of-band errors are held before the queue stops growing.
 *
 * `uncapturederror` can fire once a frame for as long as a page is open, and a
 * reader may be slow or absent, so an unbounded queue is a leak on a page that
 * is doing badly. The first errors are the ones worth keeping: what went wrong
 * first is what caused the rest. Nothing past the cap is *lost* silently,
 * though; see {@link DeviceErrorLog#take}.
 *
 * Independent of `MAX_DEVICE_ERRORS`, which bounds one *reply* rather than this
 * queue — the two are equal today, so a single `TakeError` empties a full log,
 * but neither number follows the other.
 */
const MAX_PENDING_ERRORS = 64;

/**
 * The names of the two things that drain the error log.
 *
 * TWO READERS OVER ONE LOG, AND NEITHER TAKES WHAT THE OTHER NEEDS. The errors
 * are captured in one place — {@link Replayer#deviceError} — and read by two:
 * the engine, through a `TakeError` command that carries them to
 * `Device::take_error` in wasm, and this file's own callers, through
 * {@link Replayer#takeError}, which is how `web/tools/gpu-replay.mjs` and the
 * browser parity gate assert that a scene produced no device errors at all.
 *
 * A single shared queue would break that gate the day wasm started draining it:
 * the engine would eat the errors mid-frame and the gate would then prove
 * nothing, silently and while still passing. So each reader has its own cursor
 * into the same log, every message is delivered to both, and a message is
 * dropped only once *both* have taken it.
 */
const ERROR_READERS = Object.freeze(['gate', 'wasm']);

/**
 * The filters one flush's nested error scopes cover, outermost first.
 *
 * ALL THREE, BECAUSE A SCOPE IS EXCLUSIVE RATHER THAN ADDITIONAL. An error a
 * scope captures never reaches `uncapturederror`; an error no open scope's
 * filter matches propagates outward and does. So partial coverage loses nothing
 * — a flush that pushed only `'validation'` would attribute validation failures
 * and go on receiving the other two on the listener, unattributed — but it is
 * incomplete for no saving worth having, since the cost of a flush's scopes is
 * paid once per flush however many there are.
 *
 * These are `GPUErrorFilter`'s whole domain, so between them every error the
 * device reports on the content timeline is attributed rather than merely
 * delivered. Should WebGPU grow a fourth error type, errors of it would fall
 * through to the listener exactly as they do now: unattributed, not lost.
 *
 * The filters are disjoint, so the nesting order decides nothing about which
 * scope an error lands in; it is fixed only so that the pops can be spelled as
 * the reverse of the pushes.
 */
const ERROR_SCOPE_FILTERS = Object.freeze([
  'internal',
  'out-of-memory',
  'validation',
]);

/**
 * The device's out-of-band errors, oldest first, with one cursor per reader.
 *
 * `Device::take_error`'s queue on this side of the seam: each reader is handed
 * each message exactly once, in the order the device reported them, which is the
 * order that says which failure caused the rest.
 */
class DeviceErrorLog {
  /**
   * Messages at least one reader has not taken yet, oldest first.
   *
   * @type {string[]}
   */
  #messages = [];
  /**
   * How many of {@link DeviceErrorLog#messages} each reader has taken. Indexes
   * into that array, so trimming it moves these down with it.
   *
   * @type {Record<string, number>}
   */
  #taken = Object.fromEntries(ERROR_READERS.map((reader) => [reader, 0]));
  /** How many messages were refused for want of room, ever. */
  #dropped = 0;
  /**
   * How much of {@link DeviceErrorLog##dropped} each reader has been told about.
   * A running total rather than a flag, so a second flood is reported again and
   * one reader being told does not hide it from the other.
   *
   * @type {Record<string, number>}
   */
  #droppedReported = Object.fromEntries(
    ERROR_READERS.map((reader) => [reader, 0])
  );

  /**
   * Records what the device reported.
   *
   * @param {string} message
   */
  push(message) {
    if (this.#messages.length >= MAX_PENDING_ERRORS) {
      this.#dropped += 1;
      return;
    }
    this.#messages.push(message);
  }

  /**
   * How many messages `reader` has not taken yet.
   *
   * @param {string} reader One of {@link ERROR_READERS}.
   */
  pending(reader) {
    return this.#messages.length - this.#taken[reader];
  }

  /**
   * The oldest message `reader` has not taken, or `null`.
   *
   * The last thing out, once that reader has emptied the log, is a synthesised
   * line naming how many were refused for want of room — so a page that
   * produced more than {@link MAX_PENDING_ERRORS} learns that it did rather
   * than being told the first few were all there was.
   *
   * @param {string} reader One of {@link ERROR_READERS}.
   * @returns {string | null}
   */
  take(reader) {
    const at = this.#taken[reader];
    if (at < this.#messages.length) {
      const message = this.#messages[at];
      this.#taken[reader] = at + 1;
      this.#forgetWhatEveryReaderHasTaken();
      return message;
    }
    if (this.#droppedReported[reader] === this.#dropped) return null;
    const dropped = this.#dropped - this.#droppedReported[reader];
    this.#droppedReported[reader] = this.#dropped;
    return `and ${dropped} further device error(s) were dropped: this replayer holds ${MAX_PENDING_ERRORS} and they were not taken in time`;
  }

  /**
   * Drops the messages every reader has already had, which is what keeps the
   * log from growing for the life of a page.
   */
  #forgetWhatEveryReaderHasTaken() {
    const behind = Math.min(
      ...ERROR_READERS.map((reader) => this.#taken[reader])
    );
    if (behind === 0) return;
    this.#messages.splice(0, behind);
    for (const reader of ERROR_READERS) this.#taken[reader] -= behind;
  }
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
   * What `GPUDevice.lost` settled with, or `null` while the device is alive.
   *
   * THE TERMINAL STATE OF THIS REPLAYER. `lost` settles once and means the
   * device is gone — every buffer, texture, pipeline and encoder made on it is
   * unusable, and nothing this replayer can do brings it back — so it is
   * recorded here rather than pushed onto the error queue and carried on from.
   * {@link Replayer#replay} reads it before every command and
   * {@link Replayer#answerLost} is what a command gets instead of being run.
   *
   * `text` is the one sentence every one of those answers carries, composed
   * once so that the loss reads identically wherever it surfaces — a command's
   * reply, a readback's failure, the `take_error` queue. It is not truncated
   * here: it is another runtime's prose like every `uncapturederror` message,
   * so it is cut where those are cut, by `putMessage` in `gpu-reply.js`, to
   * `tag::MAX_DEVICE_ERROR_BYTES` with the shared marker.
   *
   * @type {{ reason: string, message: string, text: string } | null}
   */
  #lost = null;
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
   * The `GPUCanvasContext` behind each live surface.
   *
   * One flat table for this resource kind, which is what `crcbl-webgpu`'s crate
   * docs require: handles are typed and each kind's indexes are its own, so a
   * single table shared across kinds would let a buffer and a surface holding
   * the same index stand on each other. {@link HandleTable} is that table, and
   * is the same one {@link Replayer#buffers} uses.
   *
   * @type {HandleTable<GPUCanvasContext>}
   */
  #surfaces = new HandleTable();
  /**
   * What each live swapchain handle configured.
   *
   * A table of its own for {@link Replayer#surfaces}'s reason — a swapchain
   * handle and a surface handle can carry identical bits. An entry is one of two
   * shapes, told apart by which field it carries:
   *
   *   * A CANVAS swapchain is `{ context, format }` — the configured
   *     `GPUCanvasContext` and the `GPUTextureFormat` string it was configured
   *     with, so `AcquireNextFrame` can call `getCurrentTexture` and
   *     `DestroySwapchain` can `unconfigure`. The format is kept beside the
   *     context because a canvas reports no way back to what it was configured
   *     with.
   *   * An OFFSCREEN swapchain is `{ ring, index, format }` — the owned ring of
   *     `GPUTexture`s that replaces the canvas's `getCurrentTexture`, the
   *     next-frame cursor into it, and the format the ring was allocated with.
   *     Destroying it destroys the textures rather than unconfiguring a context.
   *
   * @type {HandleTable<{ context: GPUCanvasContext, format: string } |
   *   { ring: GPUTexture[], index: number, format: string }>}
   */
  #swapchains = new HandleTable();
  /**
   * The `GPUBuffer` behind each live buffer handle.
   *
   * {@link Replayer#surfaces}'s twin in every respect, including the one that
   * matters: it is a table of its own rather than a share of that one, because
   * a buffer handle and a surface handle can carry identical bits.
   *
   * @type {HandleTable<GPUBuffer>}
   */
  #buffers = new HandleTable();
  /**
   * The `GPUTexture` behind each live image handle.
   *
   * {@link Replayer#buffers}'s twin, and a table of its own for that one's
   * reason — but with a second job the other two do not have: a
   * `CreateImageView` **looks its image up here**, because WebGPU makes a view
   * from the texture rather than from the device. So this is the one of these
   * tables a command reads rather than only writes, and a lookup that finds
   * nothing is a failure with somewhere to go rather than a `undefined` to carry
   * on past.
   *
   * @type {HandleTable<GPUTexture>}
   */
  #images = new HandleTable();
  /**
   * The `GPUTextureView` behind each live image-view handle.
   *
   * Its own table rather than a share of {@link Replayer#images}, for the reason
   * every table here is its own: a view handle and its image's handle are
   * genuinely allowed to carry identical bits, and the fixture's own do.
   *
   * @type {HandleTable<GPUTextureView>}
   */
  #imageViews = new HandleTable();
  /**
   * Which depth and stencil planes each live `GPUTextureView` presents, for
   * {@link attachmentPlanesFor}'s rule.
   *
   * **KEYED ON THE VIEW OBJECT RATHER THAN ON ITS HANDLE**, which is what makes
   * this a record and not a second table to keep in step with
   * {@link Replayer#imageViews}: two `HandleTable`s written by the same creates
   * and read by the same destroys are two chances to forget one, and what they
   * would disagree about — whether an attachment has a stencil plane — is
   * silently wrong rather than loud. A `WeakMap` cannot drift: the planes are
   * filed against the view itself, at the one moment the format and aspect are
   * both in hand, and they go when it does.
   *
   * Every path that files a view files its planes, so a view found in
   * {@link Replayer#imageViews} and missing here is this class's own bug, and
   * `#beginRenderPass` says so rather than guessing.
   *
   * @type {WeakMap<GPUTextureView, { depth: boolean, stencil: boolean }>}
   */
  #viewPlanes = new WeakMap();
  /**
   * The `GPUSampler` behind each live sampler handle.
   *
   * Its own table for the reason every table here is its own, and the probe's
   * handles make the case concrete: `PROBE_SAMPLER` carries the same eight bytes
   * as `PROBE_IMAGE`, `PROBE_IMAGE_VIEW`, `PROBE_BUFFER` and `PROBE_SURFACE`,
   * deliberately, so five kinds share one index and one generation and only the
   * opcode says which of these tables an id belongs to.
   *
   * @type {HandleTable<GPUSampler>}
   */
  #samplers = new HandleTable();
  /**
   * The `GPUBindGroupLayout` behind each live layout handle.
   *
   * Its own table for the reason every table here is its own, and this is the
   * kind that will be *read* next: a `GPUBindGroup` is made against a layout, so
   * this is the second table a later command looks something up in — as
   * {@link Replayer#images} already is for a view.
   *
   * @type {HandleTable<GPUBindGroupLayout>}
   */
  #bindGroupLayouts = new HandleTable();
  /**
   * The `GPUBindGroup` behind each live bind-group handle.
   *
   * Its own table for the reason every table here is its own — and the kind that
   * *reads* the most: a `CreateBindGroup` looks its layout up in
   * {@link Replayer#bindGroupLayouts} and each of its entries up in
   * {@link Replayer#buffers}, {@link Replayer#imageViews} or
   * {@link Replayer#samplers}, so a bind group's handle and every handle it names
   * may all carry identical bits and only the opcode and the entry's discriminant
   * say which table each indexes.
   *
   * @type {HandleTable<GPUBindGroup>}
   */
  #bindGroups = new HandleTable();
  /**
   * The `GPUShaderModule` behind each live shader-module handle.
   *
   * Its own table for the reason every table here is its own: a shader-module
   * handle is genuinely allowed to carry the same eight bytes as a buffer's or a
   * sampler's, and the probe's `PROBE_SHADER_MODULE` deliberately does. It holds
   * only the modules built from WGSL — the one artifact a browser consumes —
   * because a module carrying no WGSL is refused before there is anything to file.
   *
   * @type {HandleTable<GPUShaderModule>}
   */
  #shaderModules = new HandleTable();
  /**
   * The `GPUPipelineLayout` behind each live pipeline-layout handle.
   *
   * Its own table for the reason every table here is its own, and — like
   * {@link Replayer#bindGroupLayouts} — a table a later command *reads*: a
   * pipeline layout is the resource layout a pipeline is built against, so a
   * graphics or compute pipeline (a later slice) looks its layout up here. It is
   * also the second kind to read one, since a `CreatePipelineLayout` looks each
   * of its own set handles up in {@link Replayer#bindGroupLayouts}.
   *
   * @type {HandleTable<GPUPipelineLayout>}
   */
  #pipelineLayouts = new HandleTable();
  /**
   * The `GPUComputePipeline` behind each live compute-pipeline handle.
   *
   * Its own table for the reason every table here is its own — a compute-pipeline
   * handle is allowed to carry the same eight bytes as a buffer's or a shader
   * module's, and the probe's `PROBE_COMPUTE_PIPELINE` deliberately does. It is
   * the first creation to resolve handles into *two* other tables: a
   * `CreateComputePipeline` looks its layout up in {@link Replayer#pipelineLayouts}
   * and its compute module up in {@link Replayer#shaderModules}.
   *
   * @type {HandleTable<GPUComputePipeline>}
   */
  #computePipelines = new HandleTable();
  /**
   * Graphics (render) pipelines, by handle.
   *
   * Its own table for {@link Replayer#computePipelines}'s reason — a handle
   * carries no kind — and the one whose creation reads the most other tables: a
   * `CreateGraphicsPipeline` resolves its layout out of
   * {@link Replayer#pipelineLayouts} and both its vertex and fragment modules out
   * of {@link Replayer#shaderModules}. The probe's `PROBE_GRAPHICS_PIPELINE`
   * carries the same bits as its layout and its module deliberately.
   *
   * @type {HandleTable<GPURenderPipeline>}
   */
  #graphicsPipelines = new HandleTable();
  /**
   * Finished command buffers, filed by {@link Replayer#finish} at the handle
   * wasm allocated and resolved by {@link Replayer#submit}. Its own table for
   * {@link Replayer#computePipelines}'s reason — a handle carries no kind.
   *
   * @type {HandleTable<GPUCommandBuffer>}
   */
  #commandBuffers = new HandleTable();
  /**
   * In-flight readbacks, filed by {@link Replayer#requestReadback} at the handle
   * wasm allocated. Each entry is
   * `{ buffer, offset, size, state, bytes, reason, abandoned }`: `state` is
   * `'mapping'` until `mapAsync` settles and `'ready'` or `'failed'` after,
   * `bytes` is the copied-out `Uint8Array` a `PollReadback` answers with, and
   * `reason` is the text it answers with instead when the map settled the wrong
   * way. A request this replayer refused outright is filed `'failed'` with no
   * `buffer` at all — the browser was never asked.
   *
   * `abandoned` is set by {@link Replayer#destroyReadback} and read by the map's
   * own handlers, which is the one piece of this state that outlives the entry:
   * the entry leaves the table on the destroy while the promise it belongs to is
   * still to settle, and the handlers hold their own reference to it.
   *
   * **Not the persistent-object tables' shape**, which is the whole reason it is
   * separate: those hold a browser object for the frames between its create and
   * its destroy, and this holds transient poll state that a `mapAsync` promise
   * mutates a turn of the event loop later.
   *
   * @type {HandleTable<{ buffer: GPUBuffer | null, offset: number, size: number, state: 'mapping' | 'ready' | 'failed', bytes: Uint8Array | null, reason: string | null, abandoned: boolean }>}
   */
  #readbacks = new HandleTable();
  /**
   * The `GPUQuerySet` behind each live query-set handle, and how many queries it
   * holds.
   *
   * Its own table for the reason every table here is its own — a handle carries
   * no kind — and one that four later commands *read*: the reset, the resolve,
   * the direct read and the destroy all name a set. The `count` is kept beside
   * the object because `GPUQuerySet.count` is the number the browser was asked
   * for and every range this replayer refuses is refused against it: WebGPU
   * would report an out-of-range resolve out of band, a frame later and
   * attributed to nothing.
   *
   * @type {HandleTable<{ set: GPUQuerySet, count: number }>}
   */
  #querySets = new HandleTable();
  /**
   * The encoder recording commands right now, or `null` between a
   * {@link Replayer#finish} and the next {@link Replayer#createCommandEncoder}.
   *
   * **Implicit-current, not named by a handle**, because `crcbl-hal`'s recording
   * methods name no encoder — a `Box<dyn CommandEncoder>` records into itself —
   * and this stream keeps that model. A `BeginRenderPass`, a `CopyImageToBuffer`
   * or a `Finish` with none open is a malformed stream routed to the error
   * queue, not a throw.
   *
   * @type {GPUCommandEncoder | null}
   */
  #currentEncoder = null;
  /**
   * The render pass open on {@link #currentEncoder}, or `null`. Set by
   * {@link Replayer#beginRenderPass} and cleared by
   * {@link Replayer#endRenderPass}.
   *
   * @type {GPURenderPassEncoder | null}
   */
  #currentPass = null;
  /**
   * The compute pass open on {@link #currentEncoder}, or `null`. Set by
   * {@link Replayer#beginComputePass} and cleared by
   * {@link Replayer#endComputePass}. Its own field beside {@link #currentPass}
   * because a compute pass and a render pass are distinct WebGPU objects with
   * distinct methods, and {@link Replayer#bindGroup} routes to whichever is open.
   *
   * @type {GPUComputePassEncoder | null}
   */
  #currentComputePass = null;
  /**
   * The scope each open debug region was pushed onto, innermost last.
   *
   * **The objects, not the labels**, and that is the whole reason this exists.
   * `pushDebugGroup` lives on the command encoder AND on both pass encoders, and
   * the three keep independent group stacks — so a region opened on the encoder
   * and closed after a render pass has begun must still pop the *encoder*.
   * Resolving the scope again at pop time would pop the pass instead, leaving
   * one stack unbalanced and the other short, and WebGPU refuses the whole
   * `finish()` over an unbalanced group.
   *
   * Emptied whenever the implicit-current encoder goes away — a region left open
   * on a finished encoder has nothing left to pop.
   *
   * @type {Array<GPUCommandEncoder | GPURenderPassEncoder | GPUComputePassEncoder>}
   */
  #debugGroups = [];
  /**
   * Errors the device reported out of band, oldest first, with a cursor for
   * each of the two things that read them — see {@link DeviceErrorLog}.
   *
   * @type {DeviceErrorLog}
   */
  #errors = new DeviceErrorLog();

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
   * The surfaces that are live right now.
   *
   * The live table rather than a copy, as `device` hands back the real device:
   * later slices read it to find the context a present or a swapchain names.
   * For now the only readers are a test and the browser gate, and what they are
   * there to see is the pair of things a surface command has to get right —
   * that a `CreateSurface` resolved the canvas its `canvasId` named, and that a
   * `DestroySurface` let go of it.
   *
   * @type {HandleTable<GPUCanvasContext>}
   */
  get surfaces() {
    return this.#surfaces;
  }

  /**
   * The swapchains that are configured right now, on {@link Replayer#surfaces}'s
   * terms. Each entry is the `{ context, format }` `CreateSwapchain` filed.
   *
   * @type {HandleTable<{ context: GPUCanvasContext, format: string }>}
   */
  get swapchains() {
    return this.#swapchains;
  }

  /**
   * The buffers that are live right now, on {@link Replayer#surfaces}'s terms.
   *
   * @type {HandleTable<GPUBuffer>}
   */
  get buffers() {
    return this.#buffers;
  }

  /**
   * The query sets that are live right now, on the same terms — each the
   * `{ set, count }` {@link Replayer#createQuerySet} filed.
   *
   * @type {HandleTable<{ set: GPUQuerySet, count: number }>}
   */
  get querySets() {
    return this.#querySets;
  }

  /**
   * The images that are live right now, on {@link Replayer#surfaces}'s terms.
   *
   * @type {HandleTable<GPUTexture>}
   */
  get images() {
    return this.#images;
  }

  /**
   * The image views that are live right now, on the same terms.
   *
   * @type {HandleTable<GPUTextureView>}
   */
  get imageViews() {
    return this.#imageViews;
  }

  /**
   * The samplers that are live right now, on the same terms.
   *
   * **The only thing a reader can learn about one of these**, beyond that it is
   * there: a `GPUSampler` reports its `label` and nothing else, so the browser
   * gate's evidence is this table's contents and the device's error queue rather
   * than anything read back off the object.
   *
   * @type {HandleTable<GPUSampler>}
   */
  get samplers() {
    return this.#samplers;
  }

  /**
   * The bind-group layouts that are live right now, on the same terms.
   *
   * **A `GPUBindGroupLayout` reports its `label` and nothing else** — not its
   * entries, not their bindings, not their visibility — so this table's contents
   * and the device's error queue are the whole of what a reader can learn, as
   * they are for a sampler. What the descriptor was is checkable only before it
   * is handed over, which is what `web/tools/gpu-replay.mjs` does.
   *
   * @type {HandleTable<GPUBindGroupLayout>}
   */
  get bindGroupLayouts() {
    return this.#bindGroupLayouts;
  }

  /**
   * The bind groups that are live right now, on the same terms.
   *
   * **A `GPUBindGroup` reports its `label` and nothing else** — not its layout,
   * not its entries — so this table's contents and the device's error queue are
   * the whole of what a reader can learn, as they are for a sampler and a layout.
   * What the descriptor bound is checkable only before it is handed over, which is
   * what `web/tools/gpu-replay.mjs` does.
   *
   * @type {HandleTable<GPUBindGroup>}
   */
  get bindGroups() {
    return this.#bindGroups;
  }

  /**
   * The shader modules that are live right now, on {@link Replayer#surfaces}'s
   * terms.
   *
   * **A `GPUShaderModule` reports its `label`, and — unlike a sampler or a
   * layout — one thing more that matters here: `getCompilationInfo()`**, because
   * a shader module is where compilation happens. So this table's contents, the
   * device's error queue, and that async report are what a reader can learn.
   * `web/tools/browser-e2e.mjs` reads the compilation info; `web/tools/gpu-replay.mjs`
   * proves the descriptor against a stub.
   *
   * @type {HandleTable<GPUShaderModule>}
   */
  get shaderModules() {
    return this.#shaderModules;
  }

  /**
   * The pipeline layouts that are live right now, on {@link Replayer#surfaces}'s
   * terms.
   *
   * **A `GPUPipelineLayout` reports its `label` and nothing else** — not its
   * bind-group layouts, not its push-constant ranges (WebGPU has none) — so this
   * table's contents and the device's error queue are the whole of what a reader
   * can learn, as they are for a sampler, a bind-group layout and a bind group.
   * What the descriptor was is checkable only before it is handed over, which is
   * what `web/tools/gpu-replay.mjs` does.
   *
   * @type {HandleTable<GPUPipelineLayout>}
   */
  get pipelineLayouts() {
    return this.#pipelineLayouts;
  }

  /**
   * The compute pipelines that are live right now, on {@link Replayer#surfaces}'s
   * terms.
   *
   * **A `GPUComputePipeline` reports its `label` and — unlike a layout — one thing
   * more that matters here: `getBindGroupLayout(n)`**, the derived layout only a
   * genuinely-built pipeline can answer, because a pipeline is where the shader
   * and its layout are validated against each other. So this table's contents,
   * that call, and the device's error queue are what a reader can learn.
   * `web/tools/browser-e2e.mjs` reads `getBindGroupLayout`; `web/tools/gpu-replay.mjs`
   * proves the descriptor against a stub.
   *
   * @type {HandleTable<GPUComputePipeline>}
   */
  get computePipelines() {
    return this.#computePipelines;
  }

  /**
   * The graphics (render) pipelines that are live right now, on
   * {@link Replayer#surfaces}'s terms.
   *
   * **A `GPURenderPipeline` reports its `label` and — like a compute pipeline —
   * `getBindGroupLayout(n)`**, the derived layout only a genuinely-built pipeline
   * can answer, because a pipeline is where the shaders and their layout are
   * validated against each other. So this table's contents, that call, and the
   * device's error queue are what a reader can learn.
   * `web/tools/browser-e2e.mjs` reads `getBindGroupLayout`; `web/tools/gpu-replay.mjs`
   * proves the descriptor against a stub.
   *
   * @type {HandleTable<GPURenderPipeline>}
   */
  get graphicsPipelines() {
    return this.#graphicsPipelines;
  }

  /**
   * Finished command buffers, keyed by the handle wasm allocated.
   *
   * @type {HandleTable<GPUCommandBuffer>}
   */
  get commandBuffers() {
    return this.#commandBuffers;
  }

  /**
   * In-flight readbacks, keyed by the handle wasm allocated — the poll state a
   * `PollReadback` answers from, not a browser object. The browser gate reads
   * the ready entry's `bytes` to prove the clear colour reached memory.
   *
   * @type {HandleTable<{ buffer: GPUBuffer, offset: number, size: number, state: string, bytes: Uint8Array | null }>}
   */
  get readbacks() {
    return this.#readbacks;
  }

  /**
   * How many out-of-band errors this file's own callers have not taken yet.
   *
   * The gate's cursor, not the engine's: a `TakeError` carrying errors to wasm
   * leaves this number where it was, because the two readers drain the same log
   * independently — see {@link DeviceErrorLog}.
   */
  get pendingErrors() {
    return this.#errors.pending('gate');
  }

  /**
   * The oldest error the device reported out of band that *this file's callers*
   * have not taken, or `null`.
   *
   * `crcbl_hal::Device::take_error` seen from this side, and named for it: each
   * error is reported once to each reader — taking it clears it for that reader
   * — and `docs/plan/41-webgpu-stream.md` has `Gpu::acquire` draining it at the
   * top of every frame. That draining now happens: a `TakeError` command carries
   * the same messages to wasm through {@link Replayer#takeErrorCommand}, on a
   * cursor of its own, so `web/tools/gpu-replay.mjs` and the browser gate go on
   * seeing every error the engine also sees.
   *
   * **WHY A QUEUE AND NOT A THROW OR A REPLY.** A `CreateBuffer` that cannot be
   * honoured has nowhere else to go, and the two alternatives are both wrong
   * here rather than merely worse:
   *
   *   * A throw, as `#createSurface` makes for a canvas it cannot resolve,
   *     abandons the rest of the frame — every command after the create,
   *     including the draws that would have used it. That is right for a
   *     surface, which fails once at start-up because a *page* is misconfigured
   *     and which a person fixes by changing the canvas id; it is wrong for a
   *     buffer, which fails mid-frame because a *device* ran out of room or was
   *     asked for something invalid, and which WebGPU itself does not report by
   *     throwing at all: `createBuffer` hands back a `GPUBuffer` and the reason
   *     arrives later on the device's error channel.
   *   * A reply *of its own* would name a sequence nothing is waiting on.
   *     Identity here is positional — wasm allocated the handle and moved on —
   *     so no wait is registered, and `crcbl-webgpu`'s reader turns a reply for
   *     an unawaited sequence into a `DecodeError::UnexpectedSequence` that
   *     refuses the *whole frame's* replies, stranding every other answer in it.
   *     That is why the errors leave on the back of a command that *asks* for
   *     them, whose sequence something is waiting on by construction, rather
   *     than at the moment they happen.
   *
   * The last of these to come out, once the queue has been emptied, is a
   * synthesised line naming how many were refused for want of room — so a page
   * that produced more than {@link MAX_PENDING_ERRORS} learns that it did
   * rather than being told the first few were all there was.
   *
   * @returns {string | null}
   */
  takeError() {
    return this.#errors.take('gate');
  }

  /**
   * What the device was lost with, or `null` while it is alive.
   *
   * The one way to see a loss that was **not** a failure: a device destroyed
   * deliberately is recorded here and deliberately kept off the error queue, so
   * {@link Replayer#pendingErrors} says nothing about it and only this does.
   * `web/tools/gpu-replay.mjs` reads it to tell that case from a driver's.
   *
   * @returns {{ reason: string, message: string, text: string } | null}
   */
  get lost() {
    return this.#lost;
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
    // A frame that carries nothing is not scoped. The pump runs on every
    // animation frame of every page and almost every one of them finds an empty
    // stream, so scoping here would be a stack push, a pop and a round trip to
    // the GPU process sixty times a second to ask about commands that were
    // never issued.
    if (commands.length === 0) return;
    // THE SCOPES ARE OPENED BEFORE THE FIRST COMMAND AND CLOSED AFTER THE LAST,
    // WHICH IS WHAT MAKES THE ATTRIBUTION A SEQUENCE RANGE. See
    // {@link Replayer#openErrorScopes}. `null` means this flush is unscoped —
    // no device yet, or a device that is already lost — and its errors reach
    // the `uncapturederror` listener as they always have.
    const scoped = this.#openErrorScopes();
    try {
      this.#replayEach(baseSequence, commands);
    } finally {
      // IN A `finally` BECAUSE THE SCOPE STACK IS THE DEVICE'S, NOT THIS CALL'S.
      // `replay` throws on a `ReplayError` and on a `SurfaceError`, and a throw
      // that left three scopes pushed would leave them there for the life of the
      // page — every later flush nesting three more, and every one of those
      // capturing errors nothing will ever pop.
      if (scoped !== null) {
        this.#closeErrorScopes(scoped, baseSequence, commands.length);
      }
    }
  }

  /**
   * Executes the commands of one frame, in order, dispatching each to its
   * handler.
   *
   * The whole of what {@link Replayer#replay} used to be, split out so that the
   * error scopes wrapping it can be closed in a `finally` — the pair has to
   * balance whichever way this returns, and a `try` around two hundred lines of
   * `switch` says less about what is guarded than a `try` around one call.
   *
   * @param {bigint} baseSequence
   * @param {object[]} commands
   */
  #replayEach(baseSequence, commands) {
    for (let i = 0; i < commands.length; i += 1) {
      // Positional, and wrapped to 64 bits for the reason the decoders wrap:
      // the base came off the wire, so a buffer declaring the largest possible
      // number must not produce a sequence outside the range it is typed as.
      const sequence = BigInt.asUintN(64, baseSequence + BigInt(i));
      const command = commands[i];
      // THE TERMINAL STATE, CHECKED ONCE RATHER THAN IN EVERY HANDLER. A lost
      // device serves nothing, so running the handler would only produce a
      // second failure naming whichever call happened to touch the corpse —
      // `createBuffer` reporting an allocation failure on a GPU with two spare
      // gigabytes, a `mapAsync` reporting an `AbortError` — and leave whoever
      // reads it looking for a cause in the wrong place.
      if (this.#lost !== null) {
        this.#answerLost(sequence, command);
        continue;
      }
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
        case 'CreateOffscreenSurface':
          this.#createOffscreenSurface(command);
          break;
        case 'DestroySurface':
          this.#destroySurface(command);
          break;
        case 'CreateBuffer':
          this.#createBuffer(sequence, command);
          break;
        case 'DestroyBuffer':
          this.#destroyBuffer(command);
          break;
        case 'CreateImage':
          this.#createImage(sequence, command);
          break;
        case 'DestroyImage':
          this.#destroyImage(command);
          break;
        case 'CreateImageView':
          this.#createImageView(sequence, command);
          break;
        case 'DestroyImageView':
          this.#destroyImageView(command);
          break;
        case 'CreateSampler':
          this.#createSampler(sequence, command);
          break;
        case 'DestroySampler':
          this.#destroySampler(command);
          break;
        case 'CreateBindGroupLayout':
          this.#createBindGroupLayout(sequence, command);
          break;
        case 'DestroyBindGroupLayout':
          this.#destroyBindGroupLayout(command);
          break;
        case 'CreateBindGroup':
          this.#createBindGroup(sequence, command);
          break;
        case 'DestroyBindGroup':
          this.#destroyBindGroup(command);
          break;
        case 'CreateShaderModule':
          this.#createShaderModule(sequence, command);
          break;
        case 'DestroyShaderModule':
          this.#destroyShaderModule(command);
          break;
        case 'CreatePipelineLayout':
          this.#createPipelineLayout(sequence, command);
          break;
        case 'DestroyPipelineLayout':
          this.#destroyPipelineLayout(command);
          break;
        case 'CreateComputePipeline':
          this.#createComputePipeline(sequence, command);
          break;
        case 'DestroyComputePipeline':
          this.#destroyComputePipeline(command);
          break;
        case 'CreateGraphicsPipeline':
          this.#createGraphicsPipeline(sequence, command);
          break;
        case 'DestroyGraphicsPipeline':
          this.#destroyGraphicsPipeline(command);
          break;
        case 'SurfaceCaps':
          this.#surfaceCaps(sequence);
          break;
        case 'CreateSwapchain':
          this.#createSwapchain(sequence, command);
          break;
        case 'AcquireNextFrame':
          this.#acquireNextFrame(sequence, command);
          break;
        case 'Present':
          this.#present(sequence, command);
          break;
        case 'DestroySwapchain':
          this.#destroySwapchain(command);
          break;
        case 'ReconfigureSwapchain':
          this.#reconfigureSwapchain(sequence, command);
          break;
        case 'CreateCommandEncoder':
          this.#createCommandEncoder(sequence, command);
          break;
        case 'BeginDebugLabel':
          this.#beginDebugLabel(sequence, command);
          break;
        case 'EndDebugLabel':
          this.#endDebugLabel(sequence);
          break;
        case 'InsertDebugMarker':
          this.#insertDebugMarker(sequence, command);
          break;
        case 'BeginRenderPass':
          this.#beginRenderPass(sequence, command);
          break;
        case 'EndRenderPass':
          this.#endRenderPass(sequence);
          break;
        case 'BindGraphicsPipeline':
          this.#bindGraphicsPipeline(sequence, command);
          break;
        case 'BindGroup':
          this.#bindGroup(sequence, command);
          break;
        case 'PushConstants':
          this.#pushConstants(sequence, command);
          break;
        case 'SetViewport':
          this.#setViewport(sequence, command);
          break;
        case 'SetScissor':
          this.#setScissor(sequence, command);
          break;
        case 'SetStencilReference':
          this.#setStencilReference(sequence, command);
          break;
        case 'BindIndexBuffer':
          this.#bindIndexBuffer(sequence, command);
          break;
        case 'Draw':
          this.#draw(sequence, command);
          break;
        case 'DrawIndexed':
          this.#drawIndexed(sequence, command);
          break;
        case 'DrawIndirect':
          this.#drawIndirect(sequence, command);
          break;
        case 'DrawIndexedIndirect':
          this.#drawIndexedIndirect(sequence, command);
          break;
        case 'BeginComputePass':
          this.#beginComputePass(sequence, command);
          break;
        case 'EndComputePass':
          this.#endComputePass(sequence);
          break;
        case 'BindComputePipeline':
          this.#bindComputePipeline(sequence, command);
          break;
        case 'Dispatch':
          this.#dispatch(sequence, command);
          break;
        case 'DispatchIndirect':
          this.#dispatchIndirect(sequence, command);
          break;
        case 'CopyImageToBuffer':
          this.#copyImageToBuffer(sequence, command);
          break;
        case 'CopyBufferToBuffer':
          this.#copyBufferToBuffer(sequence, command);
          break;
        case 'CopyBufferToImage':
          this.#copyBufferToImage(sequence, command);
          break;
        case 'CopyImageToImage':
          this.#copyImageToImage(sequence, command);
          break;
        case 'ClearBuffer':
          this.#clearBuffer(sequence, command);
          break;
        case 'WriteBuffer':
          this.#writeBuffer(sequence, command);
          break;
        case 'PipelineBarrier':
          this.#pipelineBarrier(sequence, command);
          break;
        case 'Finish':
          this.#finish(sequence, command);
          break;
        case 'Submit':
          this.#submit(sequence, command);
          break;
        case 'RequestReadback':
          this.#requestReadback(sequence, command);
          break;
        case 'PollReadback':
          this.#pollReadback(sequence, command);
          break;
        case 'TakeError':
          this.#takeErrorCommand(sequence);
          break;
        case 'DestroyReadback':
          this.#destroyReadback(command);
          break;
        case 'DestroyCommandBuffer':
          this.#destroyCommandBuffer(command);
          break;
        case 'CreateQuerySet':
          this.#createQuerySet(sequence, command);
          break;
        case 'DestroyQuerySet':
          this.#destroyQuerySet(command);
          break;
        case 'ResetQuerySet':
          this.#resetQuerySet(sequence, command);
          break;
        case 'ResolveQuerySet':
          this.#resolveQuerySet(sequence, command);
          break;
        case 'QueryResults':
          this.#queryResults(sequence, command);
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
   *   * a required feature with no `GPUFeatureName` behind it. `requiredFeatures`
   *     can only carry names, so such a bit cannot be passed on, and dropping it
   *     would grant a device the caller declared it could not use.
   *
   * Optional features go the other way: only the ones this adapter actually has
   * are asked for, because `requestDevice` fails the *whole* request over a
   * feature the adapter lacks — which would turn "nice to have" into fatal.
   *
   * THE LIMITS ASKED FOR ARE THE ADAPTER'S OWN CEILINGS, not the
   * specification's defaults: those defaults allow eight storage buffers per
   * shader stage and `crcbl-render` binds fourteen in one compute layout, so a
   * default device refuses that layout and every pipeline and draw behind it.
   * `DeviceDesc` carries no limits to ask with — `crcbl_hal::Limits` has no
   * per-stage binding field for the caller to have named one in — so the policy
   * is `crcbl-wgpu`'s, which opens its device with `adapter.limits()`. See
   * {@link requiredLimitsFor}.
   *
   * A DEVICE LOST LATER IS NOT THIS. `GPUDevice.lost` and `uncapturederror`
   * report a device that opened and then failed, and a `DeviceFailed` answers
   * only the request itself. That is `Device::take_error`'s territory, and the
   * listener registered below is where this replayer picks it up: WebGPU
   * reports a validation failure or an allocation failure **on the device
   * rather than to the call**, so a `createBuffer` whose descriptor the browser
   * refuses looks like a success here and says so a turn of the event loop
   * later. Registered once for the device's whole life rather than around each
   * command — see {@link Replayer#takeError} — and unguarded, because a
   * `GPUDevice` is an `EventTarget` in every implementation and a stub that is
   * not one should fail loudly rather than quietly stop reporting errors.
   *
   * `GPUDevice.lost` IS WATCHED, AND NOT THROUGH THAT QUEUE. It is a promise
   * that settles once and means the device is gone rather than that a call
   * failed, so handing it to the error queue would report a dead device as one
   * more error to log and carry on from — which is why it went unwatched while
   * this channel had no device-lost path. It has one now: a `ReadbackFailed`
   * becomes a `HalError::DeviceLost` on the far side, so a loss has somewhere to
   * arrive that says what it is. {@link Replayer#loseDevice} is what the
   * listener runs, and it makes the loss terminal rather than another entry in
   * a log. Registered here, unguarded, for the `uncapturederror` listener's
   * reason: `lost` is a promise on every `GPUDevice`, and a stub that has none
   * should fail loudly rather than quietly stop watching for the one failure
   * nothing else on this channel reports.
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
        // A `compatibleSurface` is *dropped*, not refused. WebGPU has no
        // surface-specific device — `requestDevice` takes no surface and every
        // device a browser opens can present to any canvas — so the field is a
        // carried-over Vulkan-ism with no WebGPU meaning. Refusing over it would
        // fail device-open under the real engine, which passes the surface it
        // just created; ignoring it opens the same device the caller would get
        // by omitting it, which is the correct one.
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
          requiredLimits: requiredLimitsFor(adapter),
        });
      })
      .then((device) => {
        // Before the device is held, so that a device this replayer cannot
        // listen to is a failed request rather than a live device with no error
        // channel behind it.
        device.addEventListener('uncapturederror', (event) => {
          // `event.error` is a `GPUError`, whose `message` is the whole of what
          // the browser has to say. Named as the device's own rather than
          // attributed to a command, and this is now the *narrower* of the two
          // paths into that queue rather than the only one:
          // {@link Replayer#openErrorScopes} scopes each flush, and a scope is
          // exclusive, so everything a command issued is captured there and
          // arrives with the sequence range it came from. What is left for this
          // listener is what happens with no flush open — an error from the
          // continuation of a `mapAsync`, a device opening, anything the browser
          // reports between frames — and for those there genuinely is no
          // currently-executing command, so a number here would be a guess
          // dressed as attribution.
          this.#deviceError(
            `the device reported ${String(event.error?.message ?? event.error)}`
          );
        });
        // Attached before the device is held, for the listener's reason: a
        // device whose loss this replayer cannot hear about must be a failed
        // request rather than a live device that will die silently.
        device.lost.then((info) => this.#loseDevice(info));
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
    this.#surfaces.insert(command.surface, context);
  }

  /**
   * Marks a surface id as offscreen — the counterpart of {@link Replayer#createSurface}
   * for a target that names no canvas.
   *
   * NOTHING IS RESOLVED AND NOTHING IS ALLOCATED HERE, and for the same reason
   * `#createSurface` configures no context: a surface is created before any
   * device exists, and an offscreen ring is `device.createTexture` calls that
   * need one. There is also no extent or format yet — {@link SurfaceTarget}'s
   * offscreen variant deliberately carries no size, and the format belongs to
   * the swapchain — so both arrive with `CreateSwapchain`, which is where the
   * ring is built. All this does is file {@link OFFSCREEN_SURFACE} under the
   * handle, so a later `CreateSwapchain` naming it takes the ring branch instead
   * of the canvas `configure` branch.
   *
   * SYNCHRONOUS AND WITH NO REPLY, exactly as the canvas surface pair is: wasm
   * allocated the handle and moved on.
   *
   * @param {{ surface: { index: number, generation: number } }} command
   */
  #createOffscreenSurface(command) {
    this.#surfaces.insert(command.surface, OFFSCREEN_SURFACE);
  }

  /**
   * Lets go of a surface's context.
   *
   * A DESTROY OF AN EMPTY SLOT IS A NO-OP, not an error, and that is the
   * stream's rule rather than this file's convenience: `crcbl-render` destroys
   * a resource whose creation returned an `Err` before it applies `?`, so an id
   * nothing ever created still arrives here. `crcbl-webgpu`'s own decoder
   * consults no table for the same reason. {@link HandleTable#remove} is
   * already that behaviour — and it is also the no-op for a handle whose index
   * has been *reissued* since, which a table keyed on the index alone could not
   * tell from the live occupant.
   *
   * Dropping the reference is the whole of the release. There is no
   * `unconfigure` to make because {@link Replayer#surfaces}'s contexts are
   * never configured — see `#createSurface` — and the swapchain slice that
   * starts configuring them is the one that has to unconfigure here.
   *
   * @param {{ surface: { index: number, generation: number } }} command
   */
  #destroySurface(command) {
    this.#surfaces.remove(command.surface);
  }

  /**
   * Configures a canvas swapchain: resolves the surface's `GPUCanvasContext` and
   * calls `configure`, then files `{ context, format }` under the handle wasm
   * allocated.
   *
   * SWAPCHAIN CREATION ON WEBGPU IS A CANVAS `configure`, not an allocation — the
   * `configure` `#createSurface` deliberately does not make, because it needs a
   * device this replayer may not have had then and a format nobody had described
   * yet. Both exist now: the device is open and the descriptor carries the
   * format. Synchronous and with no reply, as the surface pair is.
   *
   * `imageCount` AND `presentMode` ARE CARRIED AND DROPPED, the way a compute
   * pipeline's `workgroupSize` is: a browser only offers fifo and manages its own
   * buffering, so a canvas `configure` has no knob for either. `extent` is
   * informational too — the canvas owns its size — so it is read for nothing here.
   * Everything that can go wrong goes to {@link Replayer#takeError}: no device, a
   * surface that resolves to no context, a format or alpha mode a canvas cannot
   * express, or a `configure` that throws — the last four in
   * {@link Replayer#configureSwapchain}, which
   * {@link Replayer#reconfigureSwapchain} shares.
   *
   * @param {bigint} sequence
   * @param {object} command
   */
  #createSwapchain(sequence, command) {
    const named = `swapchain ${command.swapchain.index}.${command.swapchain.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was configured before any device opened`);
      return;
    }
    const context = this.#surfaces.get(command.surface);
    if (context === undefined) {
      this.#deviceError(
        `${named} names surface ${command.surface.index}.${command.surface.generation}, ` +
          'which this replayer holds no live surface under'
      );
      return;
    }
    if (context === OFFSCREEN_SURFACE) {
      this.#configureOffscreenSwapchain(named, command);
      return;
    }
    this.#configureSwapchain(named, command, context);
  }

  /**
   * Re-configures an already-configured swapchain in place — the same descriptor
   * {@link Replayer#createSwapchain} takes, on a swapchain this replayer already
   * holds.
   *
   * WEBGPU HAS NO SEPARATE RECONFIGURE: it is `context.configure(...)` called a
   * second time, so the format, usage and alpha mode change while the canvas keeps
   * its size and the handle stays valid. This resolves the EXISTING swapchain
   * entry rather than the surface — the context to reconfigure is the one already
   * filed — and re-runs the same map-and-`configure` body
   * {@link Replayer#configureSwapchain} holds, which OVERWRITES the stored
   * `{ context, format }` so a later acquire or copy sees the new format.
   *
   * A RECONFIGURE NAMING NO CONFIGURED SWAPCHAIN GOES TO THE ERROR QUEUE, as a
   * far-side ordering bug rather than a throw — the same treatment
   * {@link Replayer#acquireNextFrame} gives an unresolved swapchain.
   *
   * @param {bigint} sequence
   * @param {object} command
   */
  #reconfigureSwapchain(sequence, command) {
    const named = `swapchain ${command.swapchain.index}.${command.swapchain.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was reconfigured before any device opened`);
      return;
    }
    const entry = this.#swapchains.get(command.swapchain);
    if (entry === undefined) {
      this.#deviceError(
        `${named} was reconfigured, and this replayer holds none configured under it`
      );
      return;
    }
    if (entry.ring !== undefined) {
      // An offscreen ring has no `configure` to re-run: its size and format are
      // baked into `GPUTexture`s, so a reconfigure destroys the old ring and
      // builds a new one at the descriptor's extent and format under the same
      // handle, exactly as the canvas branch overwrites its stored format.
      for (const texture of entry.ring) texture.destroy();
      this.#configureOffscreenSwapchain(named, command);
      return;
    }
    this.#configureSwapchain(named, command, entry.context);
  }

  /**
   * Maps the descriptor's format and alpha, calls `configure` on `context`, and
   * files `{ context, format }` under the command's swapchain handle — the body
   * {@link Replayer#createSwapchain} and {@link Replayer#reconfigureSwapchain}
   * share.
   *
   * THE `COPY_SRC` IN THE USAGE IS DELIBERATE AND LOAD-BEARING. `SwapchainDesc`
   * carries no usage field, and a canvas context defaults to `RENDER_ATTACHMENT`
   * only — which cannot be copied *from*. The WebGPU backend configures the canvas
   * as a copy source as well, a benign superset of the render-target usage, so an
   * acquired frame can be read back and used as a copy source. Without it the
   * present probe's `copyTextureToBuffer` off the acquired texture would be a
   * validation error, and the golden-image path could never read a presented
   * frame.
   *
   * {@link HandleTable#insert} REPLACES whatever the index held, so a reconfigure
   * updates the stored format in place — which is the point, since a later acquire
   * or copy must see the format the reconfigure changed to.
   *
   * AN sRGB FORMAT IS CONFIGURED AS ITS BASE AND REACHED THROUGH `viewFormats`,
   * and that is the whole of why the browser build is not dark.
   * `GPUCanvasConfiguration.format` takes only `bgra8unorm` and `rgba8unorm` and
   * refuses an `-srgb` one outright, so an engine that asked for
   * `Bgra8UnormSrgb` — which is what {@link surfaceCapsFor} offers it and what
   * `preferred_format()` picks — could not be answered by passing the name
   * through. `viewFormats` is exactly the mechanism: the canvas is configured
   * with the *base* format, which is still the browser's own preferred one and
   * so still costs no copy per present, and the frames it hands back may be
   * viewed through the counterpart named here. {@link Replayer#acquireNextFrame}
   * is what creates that view, off the format this stores.
   *
   * @param {string} named A phrase naming the swapchain and command, for errors.
   * @param {object} command
   * @param {GPUCanvasContext} context The context to configure.
   */
  #configureSwapchain(named, command, context) {
    const format = webgpuTextureFormatFor(
      command.format,
      this.#device.features
    );
    if (format.reason !== null) {
      this.#deviceError(`${named} ${format.reason}`);
      return;
    }
    const alpha = webgpuAlphaModeFor(command.compositeAlpha);
    if (alpha.reason !== null) {
      this.#deviceError(`${named} ${alpha.reason}`);
      return;
    }
    // A base format is configured as itself and viewed as itself; an sRGB one is
    // configured as the base it reinterprets and named in `viewFormats`. See the
    // method docs — the frame's view is created in `format.name` either way.
    const base = CANVAS_BASE_FORMAT[format.name] ?? format.name;
    try {
      context.configure({
        device: this.#device,
        format: base,
        viewFormats: base === format.name ? [] : [format.name],
        // COPY_SRC beside RENDER_ATTACHMENT: the acquired frame must be readable
        // as a copy source. See the method docs — this is the load-bearing line.
        usage: GPU_TEXTURE_USAGE.RENDER_ATTACHMENT | GPU_TEXTURE_USAGE.COPY_SRC,
        alphaMode: alpha.mode,
      });
    } catch (error) {
      this.#deviceError(`${named} could not be configured: ${String(error)}`);
      return;
    }
    this.#swapchains.insert(command.swapchain, {
      context,
      format: format.name,
    });
  }

  /**
   * Allocates an offscreen swapchain's ring of textures and files it under the
   * command's swapchain handle — the offscreen counterpart of
   * {@link Replayer#configureSwapchain}.
   *
   * WHERE THE CANVAS BRANCH CALLS `context.configure`, THIS OWNS ITS TEXTURES.
   * There is no canvas to hand back frames, so the ring is `imageCount`
   * `GPUTexture`s this replayer creates at the descriptor's extent and format,
   * and `AcquireNextFrame` hands out the next one in place of
   * `context.getCurrentTexture()`. `imageCount` and `extent` are the load-bearing
   * fields here — the canvas branch drops both because a canvas owns its own
   * buffering and size, but an offscreen ring has neither unless this reads them.
   *
   * THE `RENDER_ATTACHMENT | COPY_SRC` USAGE IS THE CANVAS BRANCH'S, and for its
   * reason: a frame is drawn into as a render target and then read back as a copy
   * source, so the golden path's `copyTextureToBuffer` off an acquired texture is
   * valid. See {@link Replayer#configureSwapchain}'s note on that pair.
   *
   * EVERYTHING THAT CAN GO WRONG GOES TO {@link Replayer#takeError}: a format the
   * device cannot express, an `imageCount` of zero (a ring with no textures could
   * never answer an acquire), or a `createTexture` that throws. The device is
   * already known present — {@link Replayer#createSwapchain} checks it before
   * resolving the surface.
   *
   * @param {string} named A phrase naming the swapchain and command, for errors.
   * @param {object} command
   */
  #configureOffscreenSwapchain(named, command) {
    const format = webgpuTextureFormatFor(
      command.format,
      this.#device.features
    );
    if (format.reason !== null) {
      this.#deviceError(`${named} ${format.reason}`);
      return;
    }
    if (command.imageCount < 1) {
      this.#deviceError(
        `${named} asks for an offscreen ring of ${command.imageCount} textures, and a ring ` +
          'with none could never answer an acquire'
      );
      return;
    }
    const { width, height } = command.extent;
    const ring = [];
    try {
      for (let i = 0; i < command.imageCount; i += 1) {
        ring.push(
          this.#device.createTexture({
            label: command.label ?? undefined,
            size: [width, height, 1],
            format: format.name,
            // The canvas branch's usage: a render target that is also readable as
            // a copy source, so an acquired frame can be read back.
            usage:
              GPU_TEXTURE_USAGE.RENDER_ATTACHMENT | GPU_TEXTURE_USAGE.COPY_SRC,
          })
        );
      }
    } catch (error) {
      for (const texture of ring) texture.destroy();
      this.#deviceError(
        `${named} could not allocate its offscreen ring: ${String(error)}`
      );
      return;
    }
    this.#swapchains.insert(command.swapchain, {
      ring,
      index: 0,
      format: format.name,
    });
  }

  /**
   * Acquires the swapchain's current frame and files it and its view under the
   * handles wasm allocated.
   *
   * SYNCHRONOUS AND DETERMINISTIC ON WEBGPU — `getCurrentTexture()` answers in the
   * call — so there is no reply; wasm allocated the image and view handles and
   * moved on. The acquired `GPUTexture` is filed under `image` exactly as
   * `CreateImage` files a created one, and its `createView()` under `view` exactly
   * as `CreateImageView` files a created view, so every command downstream that
   * names either resolves it through the tables it already reads.
   *
   * A SWAPCHAIN THAT RESOLVES TO NOTHING GOES TO THE ERROR QUEUE and does not
   * throw — a far side that acquired before it configured, which is the ordering
   * bug {@link Replayer#takeError} exists to keep from taking the frame down.
   *
   * @param {bigint} sequence
   * @param {object} command
   */
  #acquireNextFrame(sequence, command) {
    const named = `acquire (command ${sequence})`;
    const entry = this.#swapchains.get(command.swapchain);
    if (entry === undefined) {
      this.#deviceError(
        `${named} names swapchain ${command.swapchain.index}.${command.swapchain.generation}, ` +
          'which this replayer holds no configured swapchain under'
      );
      return;
    }
    let texture;
    if (entry.ring !== undefined) {
      // The offscreen branch hands out the next texture in the ring, in place of
      // the canvas `getCurrentTexture()`, and advances the index — the way a
      // canvas rolls its own buffer once a frame is presented. The acquired
      // texture is filed under `command.image` as a specific object, so a later
      // readback reads exactly this frame regardless of where the index has since
      // moved.
      texture = entry.ring[entry.index];
      entry.index = (entry.index + 1) % entry.ring.length;
    } else {
      try {
        texture = entry.context.getCurrentTexture();
      } catch (error) {
        this.#deviceError(
          `${named} could not get the current texture: ${String(error)}`
        );
        return;
      }
    }
    this.#images.insert(command.image, texture);
    // NAMED RATHER THAN DEFAULTED, AND THAT IS THE sRGB ENCODE. A canvas is
    // configured with a linear base format — `configure` refuses an `-srgb` one
    // — so a defaulted view is linear and every pass above the seam, which
    // writes display-referred values and leaves the encode to the hardware,
    // lands unencoded and presents a transfer function too dark.
    // {@link Replayer#configureSwapchain} put the sRGB counterpart in the
    // canvas's `viewFormats`, so this view is the format the engine asked for.
    // The offscreen ring stores its textures' own format, so naming it there is
    // the default spelled out.
    const view = texture.createView({ format: entry.format });
    this.#imageViews.insert(command.view, view);
    // A swapchain frame is a colour target and has neither plane: WebGPU has no
    // presentable depth format, so `GPUCanvasConfiguration.format` and the
    // offscreen ring's are colour by construction. Recorded rather than left out
    // so that every view in `#imageViews` has an entry in `#viewPlanes`, which
    // is what lets a missing one be a bug rather than a shrug.
    this.#viewPlanes.set(view, { depth: false, stencil: false });
  }

  /**
   * Presents a swapchain — a documented NO-OP.
   *
   * WEBGPU HAS NO EXPLICIT PRESENT: the browser composites the configured canvas
   * on its own `requestAnimationFrame`, so there is nothing to call and this
   * touches nothing. It is here so the command is recognised rather than thrown,
   * exactly as `PipelineBarrier` is.
   *
   * A NON-EMPTY `waits` IS REFUSED BY NAME, the way {@link Replayer#submit}'s is:
   * WebGPU has no semaphores, so there is nothing to wait on, and silently
   * dropping a wait is a synchronisation bug. `presentId` is dropped because
   * WebGPU has no present-completion query to number.
   *
   * @param {bigint} sequence
   * @param {object} command
   */
  #present(sequence, command) {
    if (command.waits.length > 0) {
      this.#deviceError(
        `present (command ${sequence}) carries ${command.waits.length} wait(s), and WebGPU has ` +
          'no semaphores to satisfy them — the browser composites the configured canvas on its ' +
          'own requestAnimationFrame'
      );
    }
    // Otherwise nothing: the present is the browser's rAF composite, which this
    // replayer does not and cannot drive.
  }

  /**
   * Unconfigures a swapchain's canvas context and lets go of its slot.
   *
   * `GPUCanvasContext.unconfigure()` is the counterpart of the `configure`
   * {@link Replayer#createSwapchain} made, and it is what makes destroying a
   * swapchain an explicit op on this seam. A destroy that names nothing live is a
   * no-op in both of its ways — an empty slot and a stale generation — because
   * {@link HandleTable#remove} answers `undefined` for both, so a stale handle is
   * left alone exactly as every other destroy leaves one.
   *
   * @param {{ swapchain: { index: number, generation: number } }} command
   */
  #destroySwapchain(command) {
    const entry = this.#swapchains.remove(command.swapchain);
    if (entry === undefined) return;
    if (entry.ring !== undefined) {
      // An offscreen ring owns its textures, so releasing it is destroying each
      // one — the counterpart of the canvas branch's `unconfigure`.
      for (const texture of entry.ring) texture.destroy();
    } else {
      entry.context.unconfigure();
    }
  }

  /**
   * Creates a buffer on the open device and files it under the handle wasm
   * allocated.
   *
   * Synchronous, as the surface pair is: `createBuffer` answers in the call.
   * There is no reply either — the handle came in with the command — so
   * everything that can go wrong goes to {@link Replayer#takeError}, which is
   * where that choice is argued. Four things can, and each is refused *before*
   * the browser is asked except the last:
   *
   *   * no device. `Device::create_buffer` is a device method, so a stream that
   *     carries one before its `RequestDevice` has settled is asking this
   *     replayer for something it has not got. Recording it is what makes an
   *     ordering bug visible at the command that was too early.
   *   * a usage flag or a memory location with nothing behind it in WebGPU —
   *     see {@link webgpuBufferUsageFor}, which names the flag.
   *   * a size no `GPUSize64` can carry exactly. The wire's size is a `u64` and
   *     WebGPU's is a JavaScript number, so anything past
   *     `Number.MAX_SAFE_INTEGER` would be passed on rounded — a buffer of a
   *     size nobody asked for, created successfully. Refused with the number
   *     written out, rather than silently made a little smaller or larger.
   *   * a `createBuffer` that throws. Most WebGPU failures do not throw — an
   *     invalid usage combination or a size over `maxBufferSize` is reported
   *     asynchronously, which is what the `uncapturederror` listener in
   *     `#requestDevice` is for — but an allocation failure may, and a throw
   *     out of `replay` here would take the frame down for the reason
   *     {@link Replayer#takeError} says it must not.
   *
   * @param {bigint} sequence
   * @param {{ buffer: { index: number, generation: number },
   *           label: string | null, size: bigint, usage: string[],
   *           memory: string }} command
   */
  #createBuffer(sequence, command) {
    const named = `buffer ${command.buffer.index}.${command.buffer.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was created before any device opened`);
      return;
    }
    const usage = webgpuBufferUsageFor(command.usage, command.memory);
    if (usage.unsatisfiable.length > 0) {
      this.#deviceError(
        `${named} asks for ${usage.unsatisfiable.join(', ')}, which WebGPU has no GPUBufferUsage bit for`
      );
      return;
    }
    if (command.size > BigInt(Number.MAX_SAFE_INTEGER)) {
      this.#deviceError(
        `${named} asks for ${command.size} bytes, which is past the largest size a GPUSize64 carries exactly`
      );
      return;
    }
    let buffer;
    try {
      buffer = this.#device.createBuffer({
        // A descriptor with no label passes none rather than an empty one, as
        // `#requestDevice` does. WebGPU cannot tell the two apart afterwards —
        // `GPUObjectBase.label` is `''` either way — but what is sent is still
        // what the seam said.
        ...(command.label === null ? {} : { label: command.label }),
        size: Number(command.size),
        usage: usage.bits,
      });
    } catch (error) {
      this.#deviceError(`${named} could not be created: ${String(error)}`);
      return;
    }
    this.#buffers.insert(command.buffer, buffer);
  }

  /**
   * Destroys a buffer and lets go of its slot.
   *
   * `GPUBuffer.destroy()` is the release — it drops the allocation at once
   * rather than waiting for the object to be collected, which is the whole
   * point of an explicit destroy on this seam — and the slot goes with it so
   * nothing can reach a destroyed buffer through its handle.
   *
   * A DESTROY THAT NAMES NOTHING LIVE IS A NO-OP, in both of the ways it can:
   * an empty slot, which the crate docs make legal because `crcbl-render`
   * destroys a handle whose creation returned an `Err`, and a slot holding a
   * *different generation*, which is a handle whose index has since been
   * reissued. {@link HandleTable#remove} answers `undefined` for both, which is
   * why this reads as one branch rather than two.
   *
   * @param {{ buffer: { index: number, generation: number } }} command
   */
  #destroyBuffer(command) {
    this.#buffers.remove(command.buffer)?.destroy();
  }

  /**
   * Creates a texture on the open device and files it under the handle wasm
   * allocated.
   *
   * `#createBuffer`'s shape in every respect — synchronous, no reply, every
   * failure into {@link Replayer#takeError} — with two refusals of its own that
   * the buffer pair has no equivalent of, and one it deliberately does not make:
   *
   *   * a `Format` this device cannot use, or that this file has no
   *     `GPUTextureFormat` for at all. See {@link webgpuTextureFormatFor}, which
   *     is where the choice to refuse here rather than let the browser refuse is
   *     argued.
   *   * an `ImageType` this file has no `dimension` for, which is a decoder that
   *     has grown a variant this table has not. Refused rather than defaulted to
   *     `'2d'`, because `depth_or_layers` means the depth on a `D3` and the
   *     layer count on everything else — so a guess here turns a volume into a
   *     stack of slices with nothing downstream able to tell.
   *   * **`mip_levels` and `samples` are passed on exactly as they arrived, zero
   *     included.** The fixture carries a descriptor with both at zero, and no
   *     device accepts either; that is a descriptor the browser refuses rather
   *     than a stream this replayer should second-guess, which is the same
   *     judgement `#createBuffer` makes about a size no device will allocate.
   *
   * @param {bigint} sequence
   * @param {{ image: { index: number, generation: number },
   *           label: string | null, imageType: string,
   *           extent: { width: number, height: number, depthOrLayers: number },
   *           format: string, mipLevels: number, samples: number,
   *           usage: string[] }} command
   */
  #createImage(sequence, command) {
    const named = `image ${command.image.index}.${command.image.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was created before any device opened`);
      return;
    }
    const dimension = TEXTURE_DIMENSION[command.imageType];
    if (dimension === undefined) {
      this.#deviceError(
        `${named} asks for ImageType::${command.imageType}, which is no GPUTextureDimension`
      );
      return;
    }
    const format = webgpuTextureFormatFor(
      command.format,
      this.#device.features
    );
    if (format.reason !== null) {
      this.#deviceError(`${named} ${format.reason}`);
      return;
    }
    const usage = webgpuTextureUsageFor(command.usage);
    if (usage.unsatisfiable.length > 0) {
      this.#deviceError(
        `${named} asks for ${usage.unsatisfiable.join(', ')}, which WebGPU has no GPUTextureUsage bit for`
      );
      return;
    }
    let texture;
    try {
      texture = this.#device.createTexture({
        // No label rather than an empty one, as `#createBuffer` passes it.
        ...(command.label === null ? {} : { label: command.label }),
        dimension,
        size: {
          width: command.extent.width,
          height: command.extent.height,
          // The seam's one number under WebGPU's name for the same one. See
          // `TEXTURE_DIMENSION` for why that is a translation rather than a
          // coincidence.
          depthOrArrayLayers: command.extent.depthOrLayers,
        },
        format: format.name,
        mipLevelCount: command.mipLevels,
        sampleCount: command.samples,
        usage: usage.bits,
      });
    } catch (error) {
      this.#deviceError(`${named} could not be created: ${String(error)}`);
      return;
    }
    this.#images.insert(command.image, texture);
  }

  /**
   * Destroys an image and lets go of its slot.
   *
   * `#destroyBuffer`'s twin exactly, including both no-ops: an empty slot and a
   * slot holding a different generation are the same one branch, because
   * {@link HandleTable#remove} answers `undefined` for both.
   *
   * **IT DOES NOT TOUCH THIS IMAGE'S VIEWS**, which is a decision rather than an
   * omission. WebGPU makes every view of a destroyed texture unusable on its
   * own, so there is nothing here that needs doing; and the stream carries a
   * `DestroyImageView` for each view the caller made, so releasing them here
   * would let go of slots the far side still believes in and whose ids it will
   * destroy in their own command.
   *
   * @param {{ image: { index: number, generation: number } }} command
   */
  #destroyImage(command) {
    this.#images.remove(command.image)?.destroy();
  }

  /**
   * Creates a view of an image this replayer already holds, and files it under
   * the handle wasm allocated.
   *
   * **THE ONE CREATION THAT READS A TABLE**, which is the whole of what makes it
   * different from the three before it: a `GPUTextureView` comes from
   * `GPUTexture.createView` and not from the device, so the image handle in the
   * descriptor has to resolve to something live before there is anything to call
   * at all.
   *
   * A HANDLE THAT RESOLVES TO NOTHING GOES TO THE ERROR QUEUE AND DOES NOT
   * THROW, which is {@link Replayer#takeError}'s argument applied to the one
   * case that most looks like it deserves a throw. It covers three faults at
   * once — an image nothing ever created, one already destroyed, and a
   * generation the slot has moved past — and all three are a far side that got
   * its ordering wrong mid-frame, which is precisely the case that class says
   * must not take the rest of the frame down with it. A throw would abandon
   * every command after this one, including draws that have nothing to do with
   * this view; and there is no reply to send, because wasm allocated the view's
   * handle and moved on.
   *
   * THE SUBRESOURCE RANGE IS WHERE THE SENTINEL RESOLVES. See
   * {@link subresourceCount}: `ImageSubresourceRange::ALL` reaches WebGPU as an
   * *absent* member, which is how it spells "the rest", and passing the
   * `4294967295` on the wire would be refused outright.
   *
   * @param {bigint} sequence
   * @param {{ view: { index: number, generation: number },
   *           label: string | null,
   *           image: { index: number, generation: number },
   *           viewType: string, format: string,
   *           range: { aspect: string[], baseMip: number, mipCount: number,
   *                    baseLayer: number, layerCount: number } }} command
   */
  #createImageView(sequence, command) {
    const named = `image view ${command.view.index}.${command.view.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was created before any device opened`);
      return;
    }
    const image = this.#images.get(command.image);
    if (image === undefined) {
      this.#deviceError(
        `${named} views image ${command.image.index}.${command.image.generation}, ` +
          'which this replayer holds no live image under'
      );
      return;
    }
    const dimension = VIEW_DIMENSION[command.viewType];
    if (dimension === undefined) {
      this.#deviceError(
        `${named} asks for ImageViewType::${command.viewType}, which is no GPUTextureViewDimension`
      );
      return;
    }
    const format = webgpuTextureFormatFor(
      command.format,
      this.#device.features
    );
    if (format.reason !== null) {
      this.#deviceError(`${named} ${format.reason}`);
      return;
    }
    const aspect = webgpuTextureAspectFor(command.range.aspect);
    if (aspect.reason !== null) {
      this.#deviceError(`${named} ${aspect.reason}`);
      return;
    }
    const mipLevelCount = subresourceCount(command.range.mipCount);
    const arrayLayerCount = subresourceCount(command.range.layerCount);
    let view;
    try {
      view = image.createView({
        ...(command.label === null ? {} : { label: command.label }),
        dimension,
        format: format.name,
        aspect: aspect.aspect,
        baseMipLevel: command.range.baseMip,
        // Spread rather than assigned, so that the sentinel is an *absent*
        // member and not a member holding `undefined`. WebIDL treats the two
        // alike, and a reader of a recorded descriptor does not.
        ...(mipLevelCount === undefined ? {} : { mipLevelCount }),
        baseArrayLayer: command.range.baseLayer,
        ...(arrayLayerCount === undefined ? {} : { arrayLayerCount }),
      });
    } catch (error) {
      this.#deviceError(`${named} could not be created: ${String(error)}`);
      return;
    }
    this.#imageViews.insert(command.view, view);
    this.#viewPlanes.set(
      view,
      attachmentPlanesFor(command.format, aspect.aspect)
    );
  }

  /**
   * Lets go of an image view.
   *
   * **LETTING GO IS THE WHOLE OF THE RELEASE**, and that is WebGPU rather than a
   * shortcut: a `GPUTextureView` has no `destroy()` — it holds no allocation of
   * its own, it is a description of one — so there is nothing to call and
   * dropping the reference is everything there is to do. `#destroySurface` is
   * the same shape for the same kind of reason.
   *
   * A destroy naming nothing live is a no-op in both of its ways, exactly as
   * every other destroy here.
   *
   * @param {{ view: { index: number, generation: number } }} command
   */
  #destroyImageView(command) {
    this.#imageViews.remove(command.view);
  }

  /**
   * Creates a sampler on the open device and files it under the handle wasm
   * allocated.
   *
   * `#createBuffer`'s shape once more — synchronous, no reply, every failure
   * into {@link Replayer#takeError} — and the creation with the *most* to decide
   * before the browser is asked, because a `GPUSamplerDescriptor`'s members are
   * a different shape from the seam's in three places. Each refusal is argued
   * where the table or the function that makes it is: an address mode WebGPU has
   * no word for at all ({@link webgpuAddressModesFor}); an anisotropy no
   * `GPUSize32` can carry, or one WebGPU forbids beside these filters
   * ({@link webgpuMaxAnisotropyFor}); and a filter or comparison name no table
   * here claims, which is a decoder that has grown a variant this file has not.
   *
   * **A NON-FINITE CLAMP IS REFUSED HERE RATHER THAN LEFT TO THE BROWSER.**
   * `lodMinClamp` and `lodMaxClamp` are WebIDL `float`s, and WebIDL refuses a
   * NaN or an infinity for one with a `TypeError` naming neither the member nor
   * the value — thrown out of `createSampler`, which this method would then
   * report as "could not be created" and nothing more. Every `f32` bit pattern
   * is a value the wire form claims, so the encoder carries them; this is the
   * half that has to say which of them WebGPU cannot hold.
   *
   * **`lod_max`'s SENTINEL RESOLVES HERE, AND IT RESOLVES TO ITSELF.**
   * `SamplerDesc::default` sets it to `f32::MAX` meaning "no limit", and the
   * rule `docs/plan/41-webgpu-stream.md` sets is that a sentinel crosses
   * verbatim and the replayer resolves it. What it must **not** resolve to is an
   * absent member, which is how {@link subresourceCount} spells
   * `ImageSubresourceRange::ALL` and which is exactly wrong here: WebGPU's
   * default for an absent `lodMaxClamp` is a *number*, not "the rest", so
   * omitting the member would silently replace "no limit" with a clamp — and a
   * mip clamp is not a value anything downstream reports, so every sampler in
   * the engine would quietly stop reaching its smallest mips with nothing
   * anywhere to attribute it to. Both clamps are therefore always written.
   *
   * @param {bigint} sequence
   * @param {{ sampler: { index: number, generation: number },
   *           label: string | null, magFilter: string, minFilter: string,
   *           mipFilter: string, addressMode: string[], lodMin: number,
   *           lodMax: number, anisotropy: number,
   *           compare: string | null }} command
   */
  #createSampler(sequence, command) {
    const named = `sampler ${command.sampler.index}.${command.sampler.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was created before any device opened`);
      return;
    }
    // Named `mipmapFilter` from here on, which is WebGPU's spelling of the
    // seam's `mip_filter` — see `SAMPLER_FILTER` for what a straight-through
    // name would cost.
    const [magFilter, minFilter, mipmapFilter] = [
      command.magFilter,
      command.minFilter,
      command.mipFilter,
    ].map((mode) => SAMPLER_FILTER[mode]);
    if (
      magFilter === undefined ||
      minFilter === undefined ||
      mipmapFilter === undefined
    ) {
      this.#deviceError(
        `${named} asks for FilterMode::${command.magFilter}/${command.minFilter}/${command.mipFilter},` +
          ' one of which is no GPUFilterMode'
      );
      return;
    }
    const address = webgpuAddressModesFor(command.addressMode);
    if (address.reason !== null) {
      this.#deviceError(`${named} ${address.reason}`);
      return;
    }
    for (const [clamp, field] of [
      [command.lodMin, 'lod_min'],
      [command.lodMax, 'lod_max'],
    ]) {
      if (!Number.isFinite(clamp)) {
        this.#deviceError(
          `${named} asks for ${field} ${clamp}, which is no GPUSamplerDescriptor clamp`
        );
        return;
      }
    }
    const anisotropy = webgpuMaxAnisotropyFor(command.anisotropy, [
      magFilter,
      minFilter,
      mipmapFilter,
    ]);
    if (anisotropy.reason !== null) {
      this.#deviceError(`${named} ${anisotropy.reason}`);
      return;
    }
    // `null` is the absent comparison and stays absent; anything else has to be
    // a name this table claims, because `GPUCompareFunction` is an enum and a
    // string outside it is a `TypeError` rather than a default.
    const compare =
      command.compare === null
        ? undefined
        : SAMPLER_COMPARE_FUNCTION[command.compare];
    if (command.compare !== null && compare === undefined) {
      this.#deviceError(
        `${named} asks for CompareOp::${command.compare}, which is no GPUCompareFunction`
      );
      return;
    }
    let sampler;
    try {
      sampler = this.#device.createSampler({
        // No label rather than an empty one, as `#createBuffer` passes it.
        ...(command.label === null ? {} : { label: command.label }),
        addressModeU: address.modes[0],
        addressModeV: address.modes[1],
        addressModeW: address.modes[2],
        magFilter,
        minFilter,
        mipmapFilter,
        lodMinClamp: command.lodMin,
        // Always written, never omitted — see this method's docs for what an
        // absent member would substitute for "no limit".
        lodMaxClamp: command.lodMax,
        maxAnisotropy: anisotropy.maxAnisotropy,
        // Spread rather than assigned, so an absent comparison is an *absent*
        // member and not one holding `undefined`. WebIDL treats the two alike
        // and a reader of a recorded descriptor does not — the same care
        // `#createImageView` takes with the range's counts.
        ...(compare === undefined ? {} : { compare }),
      });
    } catch (error) {
      this.#deviceError(`${named} could not be created: ${String(error)}`);
      return;
    }
    this.#samplers.insert(command.sampler, sampler);
  }

  /**
   * Lets go of a sampler.
   *
   * **LETTING GO IS THE WHOLE OF THE RELEASE**, as it is for a view and for a
   * surface's context: a `GPUSampler` has no `destroy()` — it holds no
   * allocation of its own, it is a piece of filter state — so there is nothing
   * to call.
   *
   * A destroy naming nothing live is a no-op in both of its ways, exactly as
   * every other destroy here.
   *
   * @param {{ sampler: { index: number, generation: number } }} command
   */
  #destroySampler(command) {
    this.#samplers.remove(command.sampler);
  }

  /**
   * Creates a bind-group layout on the open device and files it under the handle
   * wasm allocated.
   *
   * `#createSampler`'s shape once more — synchronous, no reply, every failure
   * into {@link Replayer#takeError} — and the first creation whose body is a
   * **list**. Three things about that are decisions rather than mechanics.
   *
   * **THE WHOLE LAYOUT IS REFUSED, NEVER AN ENTRY DROPPED.** A layout missing one
   * of its bindings is not a smaller layout, it is a different one: the shader
   * compiled against the full set declares a binding the pipeline layout says
   * does not exist, and WebGPU refuses that at the *pipeline*, a command away
   * from the layout that was wrong. So the loop below returns on the first entry
   * it cannot express, leaving the slot empty and exactly one error naming the
   * binding.
   *
   * **`count` IS THE ONE THAT MUST NOT BE SMOOTHED OVER.** WebGPU core has no
   * binding arrays at all — `GPUBindGroupLayoutEntry` has no `count` member —
   * so `1` is the only count that has a WebGPU spelling. The `u32::MAX` sentinel
   * gets a message of its own rather than falling into the general one, because
   * it is a different request: "as many as this device can" is the portable
   * bindless declaration and a caller writing it wants a bindless page, where a
   * caller writing `64` wants a fixed array. Accepting either and building a
   * one-descriptor binding is the worst outcome available here, and it is the
   * one the browser could not report: every later write to slot 1 upward names a
   * descriptor that does not exist, and the error arrives against the *bind
   * group*.
   *
   * **THE ENTRIES KEEP THEIR ORDER**, which is `gpu-stream.js`'s doing and this
   * loop's to preserve. What this file does *not* re-check — the seam's own
   * `check_entries` rules — is set out in the block above
   * {@link webgpuShaderStageFor}, along with which side does check each of them.
   *
   * @param {bigint} sequence
   * @param {{ layout: { index: number, generation: number },
   *           label: string | null,
   *           entries: Array<{ binding: number, visibility: string[],
   *                            kind: object, count: number,
   *                            flags: string[] }> }} command
   */
  #createBindGroupLayout(sequence, command) {
    const named = `bind group layout ${command.layout.index}.${command.layout.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was created before any device opened`);
      return;
    }
    const entries = [];
    for (const entry of command.entries) {
      const at = `binding ${entry.binding}`;
      if (entry.flags.length > 0) {
        this.#deviceError(
          `${named} sets ${entry.flags
            .map((flag) => `BindingFlags::${flag}`)
            .join(' | ')} on ${at}, and WebGPU has no bindless model at all: ` +
            'every one of them needs Features::DESCRIPTOR_INDEXING, which no ' +
            'WebGPU device reports'
        );
        return;
      }
      if (entry.count === BINDING_COUNT_DEVICE_MAX) {
        this.#deviceError(
          `${named} asks for as many descriptors as the device can hold on ${at} ` +
            '(the u32::MAX count), and WebGPU has no binding arrays: a ' +
            'GPUBindGroupLayoutEntry has no count member, so there is no number ' +
            'the sentinel could resolve to'
        );
        return;
      }
      if (entry.count !== 1) {
        this.#deviceError(
          `${named} asks for ${entry.count} descriptors on ${at}, and WebGPU has ` +
            'no binding arrays: a GPUBindGroupLayoutEntry has no count member, ' +
            'and one descriptor is not what was asked for'
        );
        return;
      }
      const visibility = webgpuShaderStageFor(entry.visibility);
      if (visibility.unsatisfiable.length > 0) {
        this.#deviceError(
          `${named} makes ${at} visible to ${visibility.unsatisfiable.join(', ')}, ` +
            'which WebGPU has no GPUShaderStage bit for'
        );
        return;
      }
      const layout = webgpuBindingLayoutFor(entry.kind);
      if (layout.reason !== null) {
        this.#deviceError(`${named} ${at} ${layout.reason}`);
        return;
      }
      entries.push({
        binding: entry.binding,
        visibility: visibility.bits,
        ...layout.layout,
      });
    }
    let made;
    try {
      made = this.#device.createBindGroupLayout({
        // No label rather than an empty one, as `#createBuffer` passes it.
        ...(command.label === null ? {} : { label: command.label }),
        entries,
      });
    } catch (error) {
      this.#deviceError(`${named} could not be created: ${String(error)}`);
      return;
    }
    this.#bindGroupLayouts.insert(command.layout, made);
  }

  /**
   * Lets go of a bind-group layout.
   *
   * **LETTING GO IS THE WHOLE OF THE RELEASE**, as it is for a sampler and a
   * view: a `GPUBindGroupLayout` has no `destroy()` — it is a description rather
   * than an allocation — so there is nothing to call.
   *
   * A destroy naming nothing live is a no-op in both of its ways, and here that
   * is the **ordinary** path rather than an edge one: every layout this file
   * refuses above leaves its handle empty, and the caller that pre-allocated the
   * handle destroys it all the same.
   *
   * @param {{ layout: { index: number, generation: number } }} command
   */
  #destroyBindGroupLayout(command) {
    this.#bindGroupLayouts.remove(command.layout);
  }

  /**
   * Creates a bind group on the open device and files it under the handle wasm
   * allocated.
   *
   * `#createBindGroupLayout`'s shape once more — synchronous, no reply, every
   * failure into {@link Replayer#takeError} — and **the first creation that reads
   * four tables**: the layout out of {@link Replayer#bindGroupLayouts}, and each
   * entry's resource out of {@link Replayer#buffers},
   * {@link Replayer#imageViews} or {@link Replayer#samplers}. A handle carries no
   * kind, so the entry's discriminant is the only thing that says which table an
   * id indexes; a stale one, one never created, or one resolved against the wrong
   * table is a failure named by which resource and which kind.
   *
   * Three things WebGPU cannot express are refused rather than smoothed over,
   * each because the seam can say it and WebGPU cannot:
   *
   *   * **a `Some` `variableCount`** — a runtime-sized array. WGSL has none, and a
   *     `variableCount` could only pair with a `VARIABLE_COUNT` layout, which
   *     `#createBindGroupLayout` already refuses — so this never reaches a layout
   *     this replayer holds, and the honest answer is to refuse it here too.
   *   * **a non-zero `array_index`** — the bindless write path. WebGPU indexes a
   *     bind group by `binding` only; a `GPUBindGroupEntry` has no array-index
   *     concept, and slice 5d refused every layout with a `count` above one, so no
   *     layout this replayer holds could accept an element past the first. This is
   *     a genuine "WebGPU cannot do it" case, handled like the others.
   *   * **the whole layout, never one entry** — a bind group missing one of its
   *     bindings is a different group WebGPU refuses at the draw, so the loop
   *     returns on the first entry it cannot resolve.
   *
   * The `WHOLE_BUFFER` sentinel resolves here, to an *absent*
   * `GPUBufferBinding.size` — see {@link BUFFER_BINDING_WHOLE} for why that is the
   * right resolution where `lod_max`'s was not.
   *
   * @param {bigint} sequence
   * @param {{ group: { index: number, generation: number },
   *           label: string | null,
   *           layout: { index: number, generation: number },
   *           entries: Array<{ binding: number, arrayIndex: number,
   *                            resource: object }>,
   *           variableCount: number | null }} command
   */
  #createBindGroup(sequence, command) {
    const named = `bind group ${command.group.index}.${command.group.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was created before any device opened`);
      return;
    }
    if (command.variableCount !== null) {
      this.#deviceError(
        `${named} sets variable_count ${command.variableCount}, and WebGPU has no ` +
          'runtime-sized arrays: a layout that could accept one would carry ' +
          'BindingFlags::VARIABLE_COUNT, which this backend refuses at layout creation'
      );
      return;
    }
    const layout = this.#bindGroupLayouts.get(command.layout);
    if (layout === undefined) {
      this.#deviceError(
        `${named} is against layout ${command.layout.index}.${command.layout.generation}, ` +
          'which this replayer holds no live bind group layout under'
      );
      return;
    }
    const entries = [];
    for (const entry of command.entries) {
      const at = `binding ${entry.binding}`;
      if (entry.arrayIndex !== 0) {
        this.#deviceError(
          `${named} writes array index ${entry.arrayIndex} on ${at}, and WebGPU ` +
            'indexes a bind group by binding only: a GPUBindGroupEntry has no ' +
            'array-index concept, and no layout this backend builds has a count above one'
        );
        return;
      }
      const resource = this.#bindGroupResourceFor(named, at, entry.resource);
      if (resource === undefined) return;
      entries.push({ binding: entry.binding, resource });
    }
    let made;
    try {
      made = this.#device.createBindGroup({
        // No label rather than an empty one, as `#createBuffer` passes it.
        ...(command.label === null ? {} : { label: command.label }),
        layout,
        entries,
      });
    } catch (error) {
      this.#deviceError(`${named} could not be created: ${String(error)}`);
      return;
    }
    this.#bindGroups.insert(command.group, made);
  }

  /**
   * The `GPUBindGroupEntry.resource` a decoded `BindingResource` becomes, or
   * `undefined` after recording why it becomes none.
   *
   * The discriminant is what says which table a handle indexes — a buffer, a view
   * and a sampler may hold identical bits — so each shape resolves against exactly
   * one of them and a miss names both the resource and its kind. Returning
   * `undefined` rather than throwing keeps a bind group naming a stale handle to
   * the error queue, which is `#createImageView`'s judgement applied to three
   * tables at once.
   *
   * @param {string} named The bind group, for the message.
   * @param {string} at The binding, for the message.
   * @param {{ name: string, buffer?: object, offset?: bigint, size?: bigint,
   *           view?: object, sampler?: object }} resource
   * @returns {object | undefined}
   */
  #bindGroupResourceFor(named, at, resource) {
    switch (resource.name) {
      case 'Buffer': {
        const buffer = this.#buffers.get(resource.buffer);
        if (buffer === undefined) {
          this.#deviceError(
            `${named} binds buffer ${resource.buffer.index}.${resource.buffer.generation} on ${at}, ` +
              'which this replayer holds no live buffer under'
          );
          return undefined;
        }
        if (resource.offset > MAX_BUFFER_BINDING) {
          this.#deviceError(
            `${named} binds ${at} at offset ${resource.offset}, which is past the largest a GPUSize64 carries exactly`
          );
          return undefined;
        }
        // The sentinel becomes an *absent* size member, which is how WebGPU
        // spells "to the end" — see BUFFER_BINDING_WHOLE. Spread rather than
        // assigned, so it is absent and not a member holding `undefined`.
        if (
          resource.size !== BUFFER_BINDING_WHOLE &&
          resource.size > MAX_BUFFER_BINDING
        ) {
          this.#deviceError(
            `${named} binds ${at} with size ${resource.size}, which is past the largest a GPUSize64 carries exactly`
          );
          return undefined;
        }
        return {
          buffer,
          offset: Number(resource.offset),
          ...(resource.size === BUFFER_BINDING_WHOLE
            ? {}
            : { size: Number(resource.size) }),
        };
      }
      case 'ImageView': {
        const view = this.#imageViews.get(resource.view);
        if (view === undefined) {
          this.#deviceError(
            `${named} binds image view ${resource.view.index}.${resource.view.generation} on ${at}, ` +
              'which this replayer holds no live image view under'
          );
          return undefined;
        }
        return view;
      }
      // The last shape, spelled out rather than left to a `default`: a variant
      // added tomorrow must not be bound as a sampler by accident.
      case 'Sampler': {
        const sampler = this.#samplers.get(resource.sampler);
        if (sampler === undefined) {
          this.#deviceError(
            `${named} binds sampler ${resource.sampler.index}.${resource.sampler.generation} on ${at}, ` +
              'which this replayer holds no live sampler under'
          );
          return undefined;
        }
        return sampler;
      }
      default:
        this.#deviceError(
          `${named} binds a BindingResource::${resource.name} on ${at}, which this backend has no GPUBindGroupEntry resource for`
        );
        return undefined;
    }
  }

  /**
   * Lets go of a bind group.
   *
   * **LETTING GO IS THE WHOLE OF THE RELEASE**, as it is for a sampler, a view
   * and a layout: a `GPUBindGroup` has no `destroy()` — it is a description of a
   * binding rather than an allocation of its own — so there is nothing to call.
   *
   * A destroy naming nothing live is a no-op in both of its ways, exactly as
   * every other destroy here.
   *
   * @param {{ group: { index: number, generation: number } }} command
   */
  #destroyBindGroup(command) {
    this.#bindGroups.remove(command.group);
  }

  /**
   * Creates a shader module on the open device and files it under the handle wasm
   * allocated.
   *
   * `#createBuffer`'s shape — synchronous, no reply, every failure into
   * {@link Replayer#takeError} — for a descriptor that carries **four artifacts
   * and this backend consumes exactly one**. `spirv`, `msl` and `dxil` all cross
   * so the Rust decoder (`crcbl-dx12` reads the DXIL, `crcbl-mtl` the MSL) has
   * them, but a WebGPU backend has no path for any of them — naga cannot take the
   * `DrawParameters` SPIR-V this engine ships — so `wgsl` is the only field read
   * here. Two of its states are decided before the browser is asked, and one is
   * left to the browser exactly as `#createBuffer` leaves an allocation failure:
   *
   *   * **`wgsl` is `null` — refused, by name.** A module carrying no WGSL is
   *     `ShaderModuleDesc::unusable` for this backend, and this is the same
   *     synchronous refusal `#createBuffer` makes for a usage WebGPU has no bit
   *     for: the two quieter answers are worse. Building an empty module would
   *     file a shader no pipeline can use under the handle, and every pipeline
   *     naming it would then fail with the reason nowhere near the creation.
   *   * **`wgsl` is `''` — built.** An empty WGSL module is *valid* — one with no
   *     entry points — and must not be confused with `null`; refusing it would
   *     turn a real, if useless, module into a failure the seam does not have.
   *   * **`wgsl` is source that will not compile — built anyway, and NOT inspected
   *     here.** `createShaderModule` never throws for bad WGSL: WebGPU hands back
   *     a `GPUShaderModule` regardless and reports compilation errors through the
   *     async `getCompilationInfo()`, with some but not all also reaching
   *     `uncapturederror`. Inspecting compilation info would mean deferring this
   *     creation to a promise — the machinery `#enumerateAdapters` needs and the
   *     whole buffer/image/sampler family deliberately does without — for a
   *     failure that already has a home: a bad shader fails at pipeline creation
   *     in a later slice, and the device's `uncapturederror` listener
   *     (`#requestDevice`) feeds the same queue a `#createBuffer` throw does. So
   *     this leaves it there rather than growing an async path the seam answers
   *     positionally. `web/tools/browser-e2e.mjs` is where `getCompilationInfo()`
   *     *is* read — off a known-good module, to prove compilation ran clean, which
   *     is the browser's job and not this replay's.
   *
   * @param {bigint} sequence
   * @param {{ module: { index: number, generation: number },
   *           label: string | null, spirv: number[], wgsl: string | null,
   *           msl: string | null,
   *           dxil: { entryPoint: string, container: Uint8Array }[] }} command
   */
  #createShaderModule(sequence, command) {
    const named = `shader module ${command.module.index}.${command.module.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was created before any device opened`);
      return;
    }
    if (command.wgsl === null) {
      // Named the way `ShaderModuleDesc::unusable` names it: which formats were
      // offered and which this backend needed. `spirv`/`msl`/`dxil` are the ones
      // it was given and cannot use; WGSL is the one it needed and did not get.
      const offered = [];
      if (command.spirv.length > 0) offered.push('SPIR-V');
      if (command.msl !== null) offered.push('MSL');
      if (command.dxil.length > 0) offered.push('DXIL');
      const given = offered.length > 0 ? offered.join(' and ') : 'nothing';
      this.#deviceError(
        `${named} was given ${given}, but this backend can only compile WGSL and the module carried none`
      );
      return;
    }
    let module;
    try {
      module = this.#device.createShaderModule({
        // No label rather than an empty one, as `#createBuffer` passes it.
        ...(command.label === null ? {} : { label: command.label }),
        code: command.wgsl,
      });
    } catch (error) {
      this.#deviceError(`${named} could not be created: ${String(error)}`);
      return;
    }
    this.#shaderModules.insert(command.module, module);
  }

  /**
   * Lets go of a shader module.
   *
   * **LETTING GO IS THE WHOLE OF THE RELEASE**, as it is for a sampler, a view, a
   * layout and a bind group: a `GPUShaderModule` has no `destroy()` — it holds no
   * allocation of its own — so there is nothing to call.
   *
   * A destroy naming nothing live is a no-op in both of its ways — an empty slot
   * and a stale generation — exactly as every other destroy here, because
   * {@link HandleTable#remove} answers `undefined` for both. This is the destroy
   * `crcbl-render` leans on hardest, so the stale-generation no-op matters: it
   * pre-allocates the handle, destroys it, and only then applies `?`, so the id
   * may name a module whose creation failed and whose index has since been reused.
   *
   * @param {{ module: { index: number, generation: number } }} command
   */
  #destroyShaderModule(command) {
    this.#shaderModules.remove(command.module);
  }

  /**
   * Creates a pipeline layout on the open device and files it under the handle
   * wasm allocated.
   *
   * `#createBindGroup`'s shape once more — synchronous, no reply, every failure
   * into {@link Replayer#takeError} — and the second creation that reads the
   * {@link Replayer#bindGroupLayouts} table: a pipeline layout is built from the
   * bind-group layouts a shader's `@group(n)` will index. Two things are refused
   * rather than smoothed over, each because the seam can say it and WebGPU
   * cannot.
   *
   *   * **a `Some` `pushConstants`** — a push-constant range. **WebGPU has no push
   *     constants at all**: WGSL cannot express one, so `Features::PUSH_CONSTANTS`
   *     is on the never-set list above and there is no `GPUPipelineLayout` member
   *     to carry a range. This is the case `crcbl_hal::Device::create_pipeline_layout`'s
   *     own docs single out — it *must fail loudly rather than dropping the writes
   *     later* — because a layout accepted with the range silently discarded
   *     produces a pipeline whose per-draw constants go nowhere, and the failure
   *     surfaces at whatever shader reads them rather than at the creation. So it
   *     is refused here, before the browser is asked, exactly as `#createBuffer`
   *     refuses `BufferUsage::DEVICE_ADDRESS` and `#createBindGroupLayout` refuses
   *     a `VARIABLE_COUNT` entry.
   *   * **a set handle that resolves to nothing** — a bind-group layout never
   *     created, one already destroyed, or one at a generation the slot has moved
   *     past. A handle carries no kind, so this is `#createImageView`'s judgement
   *     applied to the pipeline-layout's set list: the miss names *which set
   *     index* could not be resolved, and returns rather than throwing, because a
   *     pipeline layout naming a stale layout is a far side that got its ordering
   *     wrong mid-frame and taking the frame down would abandon every command
   *     after it.
   *
   * **THE SET LIST KEEPS ITS ORDER**, which is `gpu-stream.js`'s doing and this
   * loop's to preserve: `bindGroupLayouts` is what `@group(n)` indexes, so a
   * reordered list binds the wrong set to the wrong slot — a failure WebGPU
   * reports at the pipeline, a command away from the layout that was wrong.
   *
   * @param {bigint} sequence
   * @param {{ layout: { index: number, generation: number },
   *           label: string | null,
   *           bindGroupLayouts: { index: number, generation: number }[],
   *           pushConstants: { stages: string[], offset: number,
   *                            size: number } | null }} command
   */
  #createPipelineLayout(sequence, command) {
    const named = `pipeline layout ${command.layout.index}.${command.layout.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was created before any device opened`);
      return;
    }
    if (command.pushConstants !== null) {
      const stages =
        command.pushConstants.stages
          .map((stage) => `ShaderStages::${stage}`)
          .join(' | ') || '(no stages)';
      this.#deviceError(
        `${named} sets a push-constant range (${stages}, offset ${command.pushConstants.offset}, ` +
          `size ${command.pushConstants.size}), and WebGPU has no push constants at all: WGSL ` +
          'cannot express one, so Features::PUSH_CONSTANTS is never reported and a ' +
          'GPUPipelineLayoutDescriptor has no member to carry the range'
      );
      return;
    }
    const bindGroupLayouts = [];
    for (const [at, handle] of command.bindGroupLayouts.entries()) {
      const bindGroupLayout = this.#bindGroupLayouts.get(handle);
      if (bindGroupLayout === undefined) {
        this.#deviceError(
          `${named} names bind group layout ${handle.index}.${handle.generation} at set ${at}, ` +
            'which this replayer holds no live bind group layout under'
        );
        return;
      }
      bindGroupLayouts.push(bindGroupLayout);
    }
    let made;
    try {
      made = this.#device.createPipelineLayout({
        // No label rather than an empty one, as `#createBuffer` passes it.
        ...(command.label === null ? {} : { label: command.label }),
        bindGroupLayouts,
      });
    } catch (error) {
      this.#deviceError(`${named} could not be created: ${String(error)}`);
      return;
    }
    this.#pipelineLayouts.insert(command.layout, made);
  }

  /**
   * Lets go of a pipeline layout.
   *
   * **LETTING GO IS THE WHOLE OF THE RELEASE**, as it is for a sampler, a view, a
   * bind-group layout and a bind group: a `GPUPipelineLayout` has no `destroy()`
   * — it is a description of a resource interface rather than an allocation — so
   * there is nothing to call.
   *
   * A destroy naming nothing live is a no-op in both of its ways, and — like the
   * bind-group layout's — that is the **ordinary** path rather than an edge one:
   * every layout `#createPipelineLayout` refuses above (a `Some` range, an
   * unresolvable set) leaves its handle empty, and the caller that pre-allocated
   * the handle destroys it all the same.
   *
   * @param {{ layout: { index: number, generation: number } }} command
   */
  #destroyPipelineLayout(command) {
    this.#pipelineLayouts.remove(command.layout);
  }

  /**
   * Creates a compute pipeline on the open device and files it under the handle
   * wasm allocated.
   *
   * `#createBuffer`'s shape — synchronous, no reply, every failure into
   * {@link Replayer#takeError} — and **the first creation that resolves handles
   * into two *different* non-buffer tables**: `layout` against
   * {@link Replayer#pipelineLayouts} and `module` against
   * {@link Replayer#shaderModules}. A handle carries no kind, so the two could
   * hold identical bits; the field each arrived in is what says which table it
   * indexes. Three things are decided here.
   *
   *   * **Either handle resolving to nothing goes to the error queue naming which
   *     one** — the layout or the module — never a throw. This is
   *     `#createPipelineLayout`'s set-resolution judgement with a sharper edge:
   *     there are *two* tables, so the two failures must read distinctly, or a
   *     stale layout and a stale module would be indistinguishable in the log. A
   *     pipeline naming a stale resource is a far side that got its ordering wrong
   *     mid-frame, and taking the frame down would abandon every command after it.
   *   * **`workgroupSize` is carried on the wire and DROPPED here, and that is NOT
   *     a refusal.** WebGPU — like Vulkan — reads the workgroup size from the
   *     shader's `@workgroup_size(x, y, z)` attribute, not from the descriptor:
   *     `GPUComputePipelineDescriptor` has no member for it, and only Metal reads
   *     it from the descriptor, which is why `crcbl_hal::ComputePipelineDesc`
   *     carries it at all. So this does not pass it and does not refuse it — the
   *     authoritative copy is in the WGSL the module already carries, and the
   *     descriptor's copy is a cross-check for backends that cannot see the
   *     shader's. Dropping it changes nothing, unlike dropping a push-constant
   *     range, which would lose data — which is why `#createPipelineLayout` refuses
   *     a `Some` range and this passes `workgroupSize` by in silence.
   *   * **`createComputePipeline` errors are async, exactly like
   *     `createShaderModule`.** A bad entry point, or a shader/layout mismatch,
   *     returns a `GPUComputePipeline` object and reports the error through
   *     `uncapturederror` a task later — `createComputePipelineAsync` is the
   *     alternative and is deliberately not used, because it would mean deferring
   *     this creation to a promise, the machinery the whole buffer/image/pipeline
   *     family does without. So this builds synchronously and lets the failure
   *     surface through the device's `uncapturederror` listener
   *     (`#requestDevice`) into the same queue a `#createBuffer` throw feeds.
   *
   * @param {bigint} sequence
   * @param {{ pipeline: { index: number, generation: number },
   *           label: string | null,
   *           layout: { index: number, generation: number },
   *           module: { index: number, generation: number },
   *           entryPoint: string, workgroupSize: number[] }} command
   */
  #createComputePipeline(sequence, command) {
    const named = `compute pipeline ${command.pipeline.index}.${command.pipeline.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was created before any device opened`);
      return;
    }
    const layout = this.#pipelineLayouts.get(command.layout);
    if (layout === undefined) {
      this.#deviceError(
        `${named} names pipeline layout ${command.layout.index}.${command.layout.generation} as its layout, ` +
          'which this replayer holds no live pipeline layout under'
      );
      return;
    }
    const module = this.#shaderModules.get(command.module);
    if (module === undefined) {
      this.#deviceError(
        `${named} names shader module ${command.module.index}.${command.module.generation} as its compute stage, ` +
          'which this replayer holds no live shader module under'
      );
      return;
    }
    let pipeline;
    try {
      // `command.workgroupSize` is deliberately not passed: WebGPU reads it from
      // the module's `@workgroup_size`, and `GPUComputePipelineDescriptor` has no
      // member for it. See the method docs.
      pipeline = this.#device.createComputePipeline({
        // No label rather than an empty one, as `#createBuffer` passes it.
        ...(command.label === null ? {} : { label: command.label }),
        layout,
        compute: { module, entryPoint: command.entryPoint },
      });
    } catch (error) {
      this.#deviceError(`${named} could not be created: ${String(error)}`);
      return;
    }
    this.#computePipelines.insert(command.pipeline, pipeline);
  }

  /**
   * Lets go of a compute pipeline.
   *
   * **LETTING GO IS THE WHOLE OF THE RELEASE**, as it is for a shader module and a
   * pipeline layout: a `GPUComputePipeline` has no `destroy()` — it holds no
   * allocation this side can free — so there is nothing to call.
   *
   * A destroy naming nothing live is a no-op in both of its ways — a stale
   * generation and an empty slot — exactly as every other destroy here, because
   * {@link HandleTable#remove} answers `undefined` for both. Like the
   * pipeline-layout's, the empty slot is the *ordinary* case: every pipeline
   * `#createComputePipeline` refuses above (an unresolvable layout or module)
   * leaves its handle empty, and the caller that pre-allocated it destroys it all
   * the same.
   *
   * @param {{ pipeline: { index: number, generation: number } }} command
   */
  #destroyComputePipeline(command) {
    this.#computePipelines.remove(command.pipeline);
  }

  /**
   * Creates a render pipeline on the open device and files it under the handle
   * wasm allocated.
   *
   * **The largest descriptor on the seam**, and the one whose imperfect mappings
   * are the substance of this slice. `#createComputePipeline`'s shape once more —
   * synchronous, no reply, every failure into {@link Replayer#takeError} — with
   * *three* handles resolved and several fields WebGPU cannot take as the seam
   * spells them.
   *
   *   * **Three handles, three distinct misses.** `layout` resolves against
   *     {@link Replayer#pipelineLayouts}; `vertexModule` and — when the fragment
   *     is present — the fragment module against {@link Replayer#shaderModules}. A
   *     handle carries no kind, so a miss names *which* — the layout, the vertex
   *     module, or the fragment module — never a throw, for `#createImageView`'s
   *     reason: a stale handle is a far side that got its ordering wrong mid-frame.
   *   * **`vertex.buffers` is always `[]`.** The engine pulls geometry from
   *     storage buffers — there is no vertex-buffer layout on this seam — so there
   *     is nothing to describe and the empty array is the faithful translation.
   *   * **`fragment: null` omits the whole `GPUFragmentState`** — a depth-only
   *     pass (a shadow map, a prepass) — rather than passing an empty one.
   *   * **`PolygonMode::Line` is refused.** Wireframe is `Features::POLYGON_MODE_LINE`,
   *     native-only, and WebGPU has no core expression for it: there is no
   *     `GPUPrimitiveState` member and a fill mode is the only one. `Fill`
   *     proceeds; `Line` is refused by name, as `#createPipelineLayout` refuses a
   *     push-constant range.
   *   * **`depth_clamp` is `primitive.unclippedDepth`, feature-gated.** `true`
   *     needs `depth-clip-control`; a device that did not enable it refuses `true`,
   *     the way {@link webgpuTextureFormatFor} refuses a gated format against the
   *     device's own features. `false` proceeds and sets nothing.
   *   * **There is no stencil `reference` to drop.** WebGPU has no
   *     `stencilReference` in the pipeline — it is per-pass state — and neither
   *     does the seam, so nothing on the wire carries one and
   *     {@link Replayer#setStencilReference} is the only thing that decides what
   *     a draw compares against. Binding a pipeline must therefore leave the
   *     pass's current reference alone, which on WebGPU it cannot help doing.
   *   * **`DepthBias.constant` is `f32` on the seam and `GPUDepthBias` is an
   *     integer.** A non-integer (or out-of-`i32`) value would make WebIDL's
   *     `[EnforceRange] long` conversion throw synchronously, so it is refused by
   *     name rather than silently truncated — `1.9` must not become `1`. **This is
   *     a seam/WebGPU precision mismatch worth a backlog entry:** the engine's
   *     reversed-Z bias is tuned as a float, and an integer bias may not reproduce
   *     it. `depthBiasSlopeScale` and `depthBiasClamp` are floats and map directly.
   *   * **`samples` must be 1 or 4.** WebGPU allows no other `count`; any other is
   *     refused by name.
   *   * **Each format goes through {@link webgpuTextureFormatFor}** — the
   *     depth-stencil's and every colour target's — so a device-gated or unknown
   *     format is refused here, at the command that asked, exactly as `#createImage`
   *     does. An empty `targets` list with a fragment stage is a real
   *     writes-nothing pass and proceeds.
   *   * **`createRenderPipeline` errors are async**, exactly as
   *     `#createComputePipeline` and `#createShaderModule`: built synchronously,
   *     not awaited, a bad pipeline surfacing through the device's
   *     `uncapturederror` listener rather than through `createRenderPipelineAsync`,
   *     the machinery this family does without.
   *
   * @param {bigint} sequence
   * @param {{ pipeline: { index: number, generation: number },
   *           label: string | null,
   *           layout: { index: number, generation: number },
   *           vertexModule: { index: number, generation: number },
   *           vertexEntryPoint: string,
   *           fragment: { module: { index: number, generation: number },
   *                       entryPoint: string } | null,
   *           primitive: object, depthStencil: object | null,
   *           multisample: object, colorTargets: object[] }} command
   */
  #createGraphicsPipeline(sequence, command) {
    const named = `graphics pipeline ${command.pipeline.index}.${command.pipeline.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was created before any device opened`);
      return;
    }
    const layout = this.#pipelineLayouts.get(command.layout);
    if (layout === undefined) {
      this.#deviceError(
        `${named} names pipeline layout ${command.layout.index}.${command.layout.generation} as its layout, ` +
          'which this replayer holds no live pipeline layout under'
      );
      return;
    }
    const vertexModule = this.#shaderModules.get(command.vertexModule);
    if (vertexModule === undefined) {
      this.#deviceError(
        `${named} names shader module ${command.vertexModule.index}.${command.vertexModule.generation} as its vertex stage, ` +
          'which this replayer holds no live shader module under'
      );
      return;
    }
    let fragmentModule;
    if (command.fragment !== null) {
      fragmentModule = this.#shaderModules.get(command.fragment.module);
      if (fragmentModule === undefined) {
        this.#deviceError(
          `${named} names shader module ${command.fragment.module.index}.${command.fragment.module.generation} as its fragment stage, ` +
            'which this replayer holds no live shader module under'
        );
        return;
      }
    }

    // PolygonMode::Line has no core WebGPU expression — wireframe is native-only.
    if (command.primitive.polygonMode === 'Line') {
      this.#deviceError(
        `${named} asks for PolygonMode::Line, and WebGPU has no wireframe fill mode: ` +
          'a line polygon mode is Features::POLYGON_MODE_LINE, which is native-only, ' +
          'and a GPUPrimitiveState has no member for it'
      );
      return;
    }
    // depth_clamp maps to primitive.unclippedDepth, which is feature-gated.
    let unclippedDepth = false;
    if (command.primitive.depthClamp) {
      if (!this.#device.features?.has('depth-clip-control')) {
        this.#deviceError(
          `${named} sets depth_clamp, which WebGPU spells primitive.unclippedDepth and gates ` +
            'behind the depth-clip-control feature this device did not enable'
        );
        return;
      }
      unclippedDepth = true;
    }
    const primitive = {
      topology: PRIMITIVE_TOPOLOGY[command.primitive.topology],
      frontFace: FRONT_FACE[command.primitive.frontFace],
      cullMode: CULL_MODE[command.primitive.cullMode],
      ...(unclippedDepth ? { unclippedDepth: true } : {}),
    };

    let depthStencil;
    if (command.depthStencil !== null) {
      const ds = command.depthStencil;
      const format = webgpuTextureFormatFor(ds.format, this.#device.features);
      if (format.reason !== null) {
        this.#deviceError(`${named} depth-stencil ${format.reason}`);
        return;
      }
      const constant = ds.bias.constant;
      if (
        !Number.isInteger(constant) ||
        constant < MIN_DEPTH_BIAS ||
        constant > MAX_DEPTH_BIAS
      ) {
        // A seam/WebGPU precision mismatch: DepthBias.constant is an f32 and
        // GPUDepthBias is an i32. Refused rather than silently truncated (`1.9`
        // must not become `1`), and worth a backlog entry — the reversed-Z bias
        // is tuned as a float and an integer bias may not reproduce it.
        this.#deviceError(
          `${named} sets a depthBias constant of ${constant}, and GPUDepthBias is an integer (i32): ` +
            "WebGPU's [EnforceRange] long conversion throws on a non-integer or out-of-range value, " +
            'so the seam refuses it rather than rounding the float the engine tuned'
        );
        return;
      }
      depthStencil = {
        format: format.name,
        depthWriteEnabled: ds.depthWrite,
        depthCompare: SAMPLER_COMPARE_FUNCTION[ds.depthCompare],
        depthBias: constant,
        depthBiasSlopeScale: ds.bias.slopeScale,
        depthBiasClamp: ds.bias.clamp,
      };
      if (ds.stencil !== null) {
        // One GPUStencilFaceState per facing. There is no `reference` to set:
        // it is not a pipeline field in WebGPU and not one on the seam either —
        // setStencilReference on the open pass is the only channel.
        const faceToGpu = (face) => ({
          compare: SAMPLER_COMPARE_FUNCTION[face.compare],
          failOp: STENCIL_OPERATION[face.failOp],
          depthFailOp: STENCIL_OPERATION[face.depthFailOp],
          passOp: STENCIL_OPERATION[face.passOp],
        });
        depthStencil.stencilFront = faceToGpu(ds.stencil.front);
        depthStencil.stencilBack = faceToGpu(ds.stencil.back);
        depthStencil.stencilReadMask = ds.stencil.readMask;
        depthStencil.stencilWriteMask = ds.stencil.writeMask;
      }
    }

    const samples = command.multisample.samples;
    if (samples !== 1 && samples !== 4) {
      this.#deviceError(
        `${named} asks for ${samples} samples, and WebGPU's multisample count must be 1 or 4`
      );
      return;
    }
    const multisample = {
      count: samples,
      // Every sample covered. The seam carries no mask — Metal's pipeline
      // descriptor has no member for one — so the value is stated here rather
      // than decoded, and `GPUMultisampleState.mask` defaults to this anyway.
      mask: 0xffffffff,
      alphaToCoverageEnabled: command.multisample.alphaToCoverage,
    };

    const targets = [];
    for (const [at, target] of command.colorTargets.entries()) {
      const format = webgpuTextureFormatFor(
        target.format,
        this.#device.features
      );
      if (format.reason !== null) {
        this.#deviceError(`${named} colour target ${at} ${format.reason}`);
        return;
      }
      let writeMask = 0;
      for (const channel of target.writeMask)
        writeMask |= COLOR_WRITE_BIT[channel];
      const entry = { format: format.name, writeMask };
      if (target.blend !== null) {
        entry.blend = {
          color: {
            srcFactor: BLEND_FACTOR[target.blend.colorSrc],
            dstFactor: BLEND_FACTOR[target.blend.colorDst],
            operation: BLEND_OPERATION[target.blend.colorOp],
          },
          alpha: {
            srcFactor: BLEND_FACTOR[target.blend.alphaSrc],
            dstFactor: BLEND_FACTOR[target.blend.alphaDst],
            operation: BLEND_OPERATION[target.blend.alphaOp],
          },
        };
      }
      targets.push(entry);
    }

    let pipeline;
    try {
      pipeline = this.#device.createRenderPipeline({
        // No label rather than an empty one, as `#createBuffer` passes it.
        ...(command.label === null ? {} : { label: command.label }),
        layout,
        // Empty buffers, always: vertex pulling is the only geometry path, so
        // there is no vertex-buffer layout to describe. See the method docs.
        vertex: {
          module: vertexModule,
          entryPoint: command.vertexEntryPoint,
          buffers: [],
        },
        primitive,
        ...(depthStencil === undefined ? {} : { depthStencil }),
        multisample,
        // Omit the fragment member entirely for a depth-only pass.
        ...(command.fragment === null
          ? {}
          : {
              fragment: {
                module: fragmentModule,
                entryPoint: command.fragment.entryPoint,
                targets,
              },
            }),
      });
    } catch (error) {
      this.#deviceError(`${named} could not be created: ${String(error)}`);
      return;
    }
    this.#graphicsPipelines.insert(command.pipeline, pipeline);
  }

  /**
   * Lets go of a graphics pipeline.
   *
   * **LETTING GO IS THE WHOLE OF THE RELEASE**, as it is for a compute pipeline: a
   * `GPURenderPipeline` has no `destroy()` — it holds no allocation this side can
   * free — so there is nothing to call.
   *
   * A destroy naming nothing live is a no-op in both of its ways — a stale
   * generation and an empty slot — and, like the compute-pipeline's, the empty
   * slot is the *ordinary* case: every pipeline `#createGraphicsPipeline` refuses
   * above (a `Line` polygon mode, an unresolvable layout or module, a forbidden
   * `samples` count) leaves its handle empty, and the caller that pre-allocated it
   * destroys it all the same.
   *
   * @param {{ pipeline: { index: number, generation: number } }} command
   */
  #destroyGraphicsPipeline(command) {
    this.#graphicsPipelines.remove(command.pipeline);
  }

  /**
   * Opens the implicit-current command encoder.
   *
   * **No handle to file it under**, unlike every creation above: the encoder is
   * the one `crcbl-hal`'s recording methods record into without naming it, so it
   * is held in {@link #currentEncoder} rather than a table. {@link Replayer#finish}
   * is what turns it into a command buffer at a handle.
   *
   * `queue` is DROPPED, and that is not a refusal. WebGPU has one implicit queue,
   * `device.queue`, so there is no queue for a handle to select — the field
   * crosses so a transposition is visible and is ignored here, exactly as a
   * compute pipeline's `workgroupSize` is.
   *
   * @param {bigint} sequence
   * @param {{ label: string | null, queue: { index: number, generation: number } }} command
   */
  #createCommandEncoder(sequence, command) {
    if (!this.#device) {
      this.#deviceError(
        `command encoder (command ${sequence}) was created before any device opened`
      );
      return;
    }
    this.#currentEncoder = this.#device.createCommandEncoder(
      command.label === null ? {} : { label: command.label }
    );
    this.#currentPass = null;
    this.#currentComputePass = null;
    this.#debugGroups.length = 0;
  }

  // WHY THE THREE DEBUG COMMANDS GET NO PROBE GROUP, and probably never will.
  //
  // `web/tools/probe-groups.mjs` gates this replayer command by command against
  // a real `GPUDevice`, and every group there works the same way: encode a
  // frame, read something back, and hold the bytes against a value only the
  // command under test could have produced. `BeginDebugLabel`, `EndDebugLabel`
  // and `InsertDebugMarker` cannot be gated that way, because WebGPU gives a
  // page no way to observe them at all. `pushDebugGroup`, `popDebugGroup` and
  // `insertDebugMarker` return nothing, change no resource, and are readable
  // only inside a native capture tool attached to the browser process — there
  // is no query, no error and no pixel that differs between a frame that
  // recorded them and one that did not.
  //
  // So a group here would encode the three calls and then assert the frame
  // still submitted, which is the shape this repository keeps deleting: a check
  // that passes identically whether the commands were replayed or dropped on
  // the floor. What IS checkable is which scope each call lands on and what the
  // malformed cases do, and that is already covered without a browser:
  // `web/tools/gpu-replay.mjs` replays all three against a stub device whose
  // encoder and pass objects record their own `pushDebugGroup`,
  // `popDebugGroup` and `insertDebugMarker` calls, so a push that went to the
  // wrong object — the unbalanced group that would cost a real `finish()` — and
  // every error-queue arm below fail there by name.
  //
  // If that changes — a WebGPU extension that reports recorded markers back, or
  // a headless capture the harness can read — a group belongs here and this
  // comment is what should be deleted.

  /**
   * The scope a debug op records onto: the innermost open one.
   *
   * `pushDebugGroup`, `popDebugGroup` and `insertDebugMarker` exist on
   * `GPUCommandEncoder`, `GPURenderPassEncoder` and `GPUComputePassEncoder`
   * alike, so a marker belongs to whichever of the three the stream is inside —
   * {@link #currentPass} if a render pass is open, else
   * {@link #currentComputePass}, else the encoder itself. `null` means there is
   * no encoder either, which every caller turns into an error-queue line.
   *
   * The pass encoders are checked FIRST because a pass is open *on* the encoder:
   * recording a group on the encoder while a pass is open is what WebGPU
   * forbids, and it costs the whole `finish()`.
   *
   * @returns {GPUCommandEncoder | GPURenderPassEncoder | GPUComputePassEncoder | null}
   */
  #debugScope() {
    return (
      this.#currentPass ?? this.#currentComputePass ?? this.#currentEncoder
    );
  }

  /**
   * Opens a labelled region on the innermost open scope — `BeginDebugLabel` →
   * `pushDebugGroup(label)`.
   *
   * THE SCOPE IS REMEMBERED, not re-derived at pop time: see
   * {@link #debugGroups} for why that is the difference between a balanced
   * frame and a refused `finish()`.
   *
   * A REGION WITH NO ENCODER OPEN GOES TO THE ERROR QUEUE and does not throw —
   * the mid-frame ordering fault every recording arm refuses.
   *
   * @param {bigint} sequence
   * @param {{ label: string }} command
   */
  #beginDebugLabel(sequence, command) {
    const scope = this.#debugScope();
    if (!scope) {
      this.#deviceError(
        `a debug label was begun (command ${sequence}) with no command encoder open`
      );
      return;
    }
    scope.pushDebugGroup(command.label);
    this.#debugGroups.push(scope);
  }

  /**
   * Closes the innermost open region — `EndDebugLabel` → `popDebugGroup()` on
   * the scope that pushed it.
   *
   * POPPING WITH NO REGION OPEN IS A MALFORMED STREAM, routed to the error queue
   * rather than thrown — {@link Replayer#endRenderPass}'s judgement, and the
   * same reasoning: an unbalanced label is the far side's bug, and throwing
   * would abandon every command after it in the frame.
   *
   * @param {bigint} sequence
   */
  #endDebugLabel(sequence) {
    const scope = this.#debugGroups.pop();
    if (scope === undefined) {
      this.#deviceError(
        `a debug label was ended (command ${sequence}) with none open`
      );
      return;
    }
    scope.popDebugGroup();
  }

  /**
   * Marks a point in time on the innermost open scope — `InsertDebugMarker` →
   * `insertDebugMarker(label)`.
   *
   * IT OPENS NO REGION, which is why it is a command of its own rather than a
   * flag on {@link Replayer#beginDebugLabel}: nothing is pushed, so
   * {@link #debugGroups} is untouched and a later `EndDebugLabel` does not see
   * it. A marker with no encoder open goes to the error queue.
   *
   * @param {bigint} sequence
   * @param {{ label: string }} command
   */
  #insertDebugMarker(sequence, command) {
    const scope = this.#debugScope();
    if (!scope) {
      this.#deviceError(
        `a debug marker was inserted (command ${sequence}) with no command encoder open`
      );
      return;
    }
    scope.insertDebugMarker(command.label);
  }

  /**
   * Opens a render pass on the implicit-current encoder.
   *
   * This slice records a pass with a `LoadOp::Clear` colour attachment and no
   * draws — the clear is the whole of it — so what matters here is that every
   * attachment's view resolves and the load/store ops lower correctly. Draws,
   * bindings and pipelines are a later slice and have no arm yet.
   *
   * A PASS WITH NO ENCODER, OR AN UNRESOLVABLE VIEW, GOES TO THE ERROR QUEUE and
   * does not throw — both are a far side that got its ordering wrong mid-frame,
   * which is {@link Replayer#takeError}'s case, not a reason to abandon the rest
   * of the frame. `#currentPass` is left `null` so a later `EndRenderPass` is
   * itself a named malformed-stream error rather than a throw.
   *
   * @param {bigint} sequence
   * @param {object} command
   */
  #beginRenderPass(sequence, command) {
    const named = `render pass (command ${sequence})`;
    if (!this.#currentEncoder) {
      this.#deviceError(`${named} was begun with no command encoder open`);
      return;
    }
    const colorAttachments = [];
    for (let i = 0; i < command.colorAttachments.length; i += 1) {
      const attachment = command.colorAttachments[i];
      const view = this.#imageViews.get(attachment.view);
      if (view === undefined) {
        this.#deviceError(
          `${named} colour attachment ${i} names image view ` +
            `${attachment.view.index}.${attachment.view.generation}, which this ` +
            'replayer holds no live view under'
        );
        return;
      }
      const entry = {
        view,
        loadOp: LOAD_OP[attachment.load],
        storeOp: STORE_OP[attachment.store],
        clearValue: attachment.clear.color,
      };
      if (attachment.resolve !== null) {
        const resolveTarget = this.#imageViews.get(attachment.resolve);
        if (resolveTarget === undefined) {
          this.#deviceError(
            `${named} colour attachment ${i} resolves into image view ` +
              `${attachment.resolve.index}.${attachment.resolve.generation}, which this ` +
              'replayer holds no live view under'
          );
          return;
        }
        entry.resolveTarget = resolveTarget;
      }
      colorAttachments.push(entry);
    }

    const descriptor = { colorAttachments };
    if (command.label !== null) descriptor.label = command.label;
    const ds = command.depthStencilAttachment;
    if (ds !== null) {
      const view = this.#imageViews.get(ds.view);
      if (view === undefined) {
        this.#deviceError(
          `${named} depth-stencil attachment names image view ` +
            `${ds.view.index}.${ds.view.generation}, which this replayer holds no live view under`
        );
        return;
      }
      // **EACH PLANE IS DESCRIBED ONLY IF THE ATTACHMENT HAS IT.** See
      // {@link attachmentPlanesFor}: the seam carries four ops whatever the
      // format, and WebGPU refuses the whole `finish()` — every command in the
      // frame, not just this pass — if ops are set for a plane the attachment
      // does not have. A `depth32float` shadow atlas is the case that costs a
      // frame: it has no stencil plane, so `stencilLoadOp` and `stencilStoreOp`
      // must be *absent* rather than any value.
      const planes = this.#viewPlanes.get(view);
      if (planes === undefined) {
        this.#deviceError(
          `${named} depth-stencil attachment names image view ` +
            `${ds.view.index}.${ds.view.generation}, which this replayer holds no ` +
            'plane record for'
        );
        return;
      }
      // `read_only` maps to both `depthReadOnly` and `stencilReadOnly`: the HAL
      // carries one flag because a read-only depth attachment is read-only in
      // both planes. WebGPU forbids load/store ops on a read-only plane too, so
      // a read-only attachment reaches the browser with none of the four — the
      // same absence as an absent plane, for the other reason.
      descriptor.depthStencilAttachment = {
        view,
        ...(planes.depth
          ? {
              depthReadOnly: ds.readOnly,
              ...(ds.readOnly
                ? {}
                : {
                    depthLoadOp: LOAD_OP[ds.depthLoad],
                    depthStoreOp: STORE_OP[ds.depthStore],
                    depthClearValue: ds.clear.depth,
                  }),
            }
          : {}),
        ...(planes.stencil
          ? {
              stencilReadOnly: ds.readOnly,
              ...(ds.readOnly
                ? {}
                : {
                    stencilLoadOp: LOAD_OP[ds.stencilLoad],
                    stencilStoreOp: STORE_OP[ds.stencilStore],
                    stencilClearValue: ds.clear.stencil,
                  }),
            }
          : {}),
      };
    }
    const timestampWrites = this.#resolveTimestampWrites(
      named,
      command.timestampWrites
    );
    if (timestampWrites === undefined) return;
    if (timestampWrites !== null) descriptor.timestampWrites = timestampWrites;
    this.#currentPass = this.#currentEncoder.beginRenderPass(descriptor);
  }

  /**
   * Resolves a pass's `timestampWrites` into the GPU object WebGPU wants, or
   * `null` when the pass is untimed.
   *
   * Returns `undefined` for a pair this replayer refuses, having already put the
   * reason on the error queue — the caller must abandon the pass, because
   * beginning it without the writes would produce a pass that runs and measures
   * nothing, and a profiler would read the unwritten queries back as a frame
   * that took no time. That is the failure the seam moved timestamps into the
   * pass descriptor to prevent, so it must not be reintroduced here.
   *
   * The two indices must be distinct and inside the set — both are WebGPU's own
   * validation, refused by name here so the diagnosis is this command rather
   * than a device error attributed to nothing a frame later.
   *
   * @param {string} named
   * @param {{ set: { index: number, generation: number },
   *           beginningOfPass: number, endOfPass: number } | null | undefined} writes
   * @returns {GPUComputePassTimestampWrites | null | undefined}
   */
  #resolveTimestampWrites(named, writes) {
    if (!writes) return null;
    const entry = this.#querySets.get(writes.set);
    if (entry === undefined) {
      this.#deviceError(
        `${named} takes its timestamps in query set ` +
          `${writes.set.index}.${writes.set.generation}, which this replayer holds no live set under`
      );
      return undefined;
    }
    if (writes.beginningOfPass === writes.endOfPass) {
      this.#deviceError(
        `${named} writes both of its timestamps into query ${writes.beginningOfPass}; WebGPU ` +
          'requires beginningOfPassWriteIndex and endOfPassWriteIndex to be distinct'
      );
      return undefined;
    }
    for (const index of [writes.beginningOfPass, writes.endOfPass]) {
      if (index >= entry.count) {
        this.#deviceError(
          `${named} writes a timestamp into query ${index} of a ${entry.count}-query set`
        );
        return undefined;
      }
    }
    return {
      querySet: entry.set,
      beginningOfPassWriteIndex: writes.beginningOfPass,
      endOfPassWriteIndex: writes.endOfPass,
    };
  }

  /**
   * Closes the render pass on the implicit-current encoder.
   *
   * ENDING WITH NO OPEN PASS IS A MALFORMED STREAM, routed to the error queue
   * rather than thrown — a mis-nested pass is the far side's ordering bug, and
   * throwing would take down every command after it in the frame.
   *
   * @param {bigint} sequence
   */
  #endRenderPass(sequence) {
    if (!this.#currentPass) {
      this.#deviceError(
        `a render pass was ended (command ${sequence}) with none open`
      );
      return;
    }
    this.#currentPass.end();
    this.#currentPass = null;
  }

  /**
   * Binds a graphics pipeline onto the open render pass.
   *
   * A BIND WITH NO PASS OPEN, OR AN UNRESOLVABLE PIPELINE, GOES TO THE ERROR
   * QUEUE and does not throw — both are a far side that got its ordering wrong
   * mid-frame, {@link Replayer#takeError}'s case, not a reason to abandon the
   * rest of the frame.
   *
   * @param {bigint} sequence
   * @param {{ pipeline: { index: number, generation: number } }} command
   */
  #bindGraphicsPipeline(sequence, command) {
    if (!this.#currentPass) {
      this.#deviceError(
        `a graphics pipeline was bound (command ${sequence}) with no render pass open`
      );
      return;
    }
    const pipeline = this.#graphicsPipelines.get(command.pipeline);
    if (pipeline === undefined) {
      this.#deviceError(
        `a graphics pipeline was bound (command ${sequence}) naming pipeline ` +
          `${command.pipeline.index}.${command.pipeline.generation}, which this ` +
          'replayer holds no live graphics pipeline under'
      );
      return;
    }
    this.#currentPass.setPipeline(pipeline);
  }

  /**
   * Binds a bind group to a slot on WHICHEVER pass is open — render or compute.
   *
   * A `BindGroup` is legal in both a render pass and a compute pass, and
   * `setBindGroup(slot, group, dynamicOffsets)` has the same shape on both
   * encoders, so this resolves the active pass: {@link #currentPass} if a render
   * pass is open, else {@link #currentComputePass} if a compute pass is, else the
   * error queue.
   *
   * A BIND WITH NO PASS OPEN, OR AN UNRESOLVABLE GROUP, GOES TO THE ERROR QUEUE
   * and does not throw — the same mid-frame ordering fault
   * {@link Replayer#bindGraphicsPipeline} refuses.
   *
   * `command.layout` is CARRIED AND NOT USED — WebGPU derives the binding layout
   * from the bound pipeline, so there is no descriptor member to hand it to. It
   * crosses so a transposition is visible and is dropped here, exactly as
   * {@link Replayer#createCommandEncoder}'s `queue` is.
   *
   * @param {bigint} sequence
   * @param {{ slot: number, group: { index: number, generation: number },
   *           dynamicOffsets: number[],
   *           layout: { index: number, generation: number } }} command
   */
  #bindGroup(sequence, command) {
    const pass = this.#currentPass ?? this.#currentComputePass;
    if (!pass) {
      this.#deviceError(
        `a bind group was bound (command ${sequence}) with no render or compute pass open`
      );
      return;
    }
    const group = this.#bindGroups.get(command.group);
    if (group === undefined) {
      this.#deviceError(
        `a bind group was bound (command ${sequence}) naming bind group ` +
          `${command.group.index}.${command.group.generation}, which this ` +
          'replayer holds no live bind group under'
      );
      return;
    }
    pass.setBindGroup(command.slot, group, command.dynamicOffsets);
  }

  /**
   * Refuses a push-constant write, by name.
   *
   * WebGPU HAS NO PUSH CONSTANTS AT ALL, so this arm routes UNCONDITIONALLY to
   * the error queue, mirroring the refusal {@link Replayer#createPipelineLayout}
   * already makes of a push-constant range.
   *
   * A push-constant range never survives `createPipelineLayout` — it is refused
   * there — so a valid stream cannot reach this arm inside a pass. The arm
   * exists so a hand-written encoder that emits a `PushConstants` is refused by
   * name rather than throwing off the dispatch's `default`.
   *
   * @param {bigint} sequence
   * @param {{ stages: string[], offset: number,
   *           layout: { index: number, generation: number } }} command
   */
  #pushConstants(sequence, command) {
    const stages =
      command.stages.map((stage) => `ShaderStages::${stage}`).join(' | ') ||
      '(no stages)';
    this.#deviceError(
      `a push constant was written (command ${sequence}) (${stages}, offset ` +
        `${command.offset}), and WebGPU has no push constants at all: WGSL ` +
        'cannot express one, so Features::PUSH_CONSTANTS is never reported and a ' +
        'GPUPipelineLayoutDescriptor has no member to carry the range'
    );
  }

  /**
   * Records a draw on the open render pass.
   *
   * A DRAW WITH NO PASS OPEN GOES TO THE ERROR QUEUE and does not throw — the
   * same mid-frame ordering fault the binds above refuse.
   *
   * `vertices` and `instances` are `{ start, end }` half-open ranges — the HAL's
   * `Range<u32>` — so the counts are their spans and the firsts are their starts.
   *
   * @param {bigint} sequence
   * @param {{ vertices: { start: number, end: number },
   *           instances: { start: number, end: number } }} command
   */
  #draw(sequence, command) {
    if (!this.#currentPass) {
      this.#deviceError(
        `a draw was recorded (command ${sequence}) with no render pass open`
      );
      return;
    }
    const { vertices, instances } = command;
    this.#currentPass.draw(
      vertices.end - vertices.start,
      instances.end - instances.start,
      vertices.start,
      instances.start
    );
  }

  /**
   * Binds the index buffer for subsequent indexed draws on the open render pass —
   * `BindIndexBuffer` → `setIndexBuffer(buffer, format, offset)`.
   *
   * ON THE PASS, for {@link Replayer#draw}'s reason: `setIndexBuffer` is a
   * `GPURenderPassEncoder` method. The decoded `'Uint16'`/`'Uint32'` — the HAL
   * enum's own spelling — becomes WebGPU's `'uint16'`/`'uint32'`, and the `u64`
   * offset is narrowed with `Number`. With no pass open, or an unresolvable
   * buffer, the reason goes to the error queue rather than throwing.
   *
   * @param {bigint} sequence
   * @param {{ buffer: { index: number, generation: number }, offset: bigint,
   *   format: string }} command
   */
  #bindIndexBuffer(sequence, command) {
    const named = `index buffer bind (command ${sequence})`;
    if (!this.#currentPass) {
      this.#deviceError(`${named} was recorded with no render pass open`);
      return;
    }
    const buffer = this.#buffers.get(command.buffer);
    if (buffer === undefined) {
      this.#deviceError(
        `${named} binds buffer ${command.buffer.index}.${command.buffer.generation}, ` +
          'which this replayer holds no live buffer under'
      );
      return;
    }
    const format = command.format === 'Uint16' ? 'uint16' : 'uint32';
    this.#currentPass.setIndexBuffer(buffer, format, Number(command.offset));
  }

  /**
   * Records an indexed draw on the open render pass — `DrawIndexed` →
   * `drawIndexed(indexCount, instanceCount, firstIndex, baseVertex,
   * firstInstance)`.
   *
   * The two ranges become spans-and-firsts exactly as {@link Replayer#draw} does,
   * with the signed `baseVertex` passed through between them. ON THE PASS, and a
   * draw with none open goes to the error queue.
   *
   * @param {bigint} sequence
   * @param {{ indices: { start: number, end: number }, baseVertex: number,
   *   instances: { start: number, end: number } }} command
   */
  #drawIndexed(sequence, command) {
    if (!this.#currentPass) {
      this.#deviceError(
        `an indexed draw was recorded (command ${sequence}) with no render pass open`
      );
      return;
    }
    const { indices, baseVertex, instances } = command;
    this.#currentPass.drawIndexed(
      indices.end - indices.start,
      instances.end - instances.start,
      indices.start,
      baseVertex,
      instances.start
    );
  }

  /**
   * Records an indirect non-indexed draw on the open render pass — `DrawIndirect`
   * → one or more `drawIndirect(indirectBuffer, indirectOffset)`.
   *
   * UNROLLED, because WebGPU's `drawIndirect` is a SINGLE draw — multi-draw
   * indirect is not core WebGPU — so a HAL `drawCount` becomes that many calls,
   * the `i`th reading its argument structure at `offset + i * stride`.
   * `drawCount === 1` is the common case and one call. Each call reads
   * `[vertexCount, instanceCount, firstVertex, firstInstance]` from the buffer.
   *
   * ON THE PASS, for {@link Replayer#draw}'s reason, and a draw with none open
   * goes to the error queue. An unresolvable argument buffer is named there too,
   * mirroring {@link Replayer#bindIndexBuffer}, rather than throwing.
   *
   * @param {bigint} sequence
   * @param {{ buffer: { index: number, generation: number }, offset: bigint,
   *   drawCount: number, stride: number }} command
   */
  #drawIndirect(sequence, command) {
    const named = `an indirect draw (command ${sequence})`;
    if (!this.#currentPass) {
      this.#deviceError(`${named} was recorded with no render pass open`);
      return;
    }
    const buffer = this.#buffers.get(command.buffer);
    if (buffer === undefined) {
      this.#deviceError(
        `${named} reads args from buffer ${command.buffer.index}.${command.buffer.generation}, ` +
          'which this replayer holds no live buffer under'
      );
      return;
    }
    const offset = Number(command.offset);
    for (let i = 0; i < command.drawCount; i += 1) {
      this.#currentPass.drawIndirect(buffer, offset + i * command.stride);
    }
  }

  /**
   * Records an indirect indexed draw on the open render pass —
   * `DrawIndexedIndirect` → one or more `drawIndexedIndirect(indirectBuffer,
   * indirectOffset)`.
   *
   * {@link Replayer#drawIndirect}'s indexed twin and unrolled for its reason; the
   * only difference is the argument structure each call reads: `[indexCount,
   * instanceCount, firstIndex, baseVertex, firstInstance]`. Needs a bound index
   * buffer, exactly as {@link Replayer#drawIndexed} does.
   *
   * @param {bigint} sequence
   * @param {{ buffer: { index: number, generation: number }, offset: bigint,
   *   drawCount: number, stride: number }} command
   */
  #drawIndexedIndirect(sequence, command) {
    const named = `an indexed indirect draw (command ${sequence})`;
    if (!this.#currentPass) {
      this.#deviceError(`${named} was recorded with no render pass open`);
      return;
    }
    const buffer = this.#buffers.get(command.buffer);
    if (buffer === undefined) {
      this.#deviceError(
        `${named} reads args from buffer ${command.buffer.index}.${command.buffer.generation}, ` +
          'which this replayer holds no live buffer under'
      );
      return;
    }
    const offset = Number(command.offset);
    for (let i = 0; i < command.drawCount; i += 1) {
      this.#currentPass.drawIndexedIndirect(
        buffer,
        offset + i * command.stride
      );
    }
  }

  /**
   * Sets the viewport on the open render pass — `Viewport` → `setViewport(x, y,
   * width, height, minDepth, maxDepth)`.
   *
   * ON THE PASS, not the encoder: WebGPU's `setViewport` is a
   * `GPURenderPassEncoder` method, so this needs {@link #currentPass}. With none
   * open the viewport belongs to nothing, which goes to the error queue rather
   * than throwing — {@link Replayer#draw}'s judgement.
   *
   * @param {bigint} sequence
   * @param {{ viewport: { x: number, y: number, width: number, height: number,
   *   depthMin: number, depthMax: number } }} command
   */
  #setViewport(sequence, command) {
    if (!this.#currentPass) {
      this.#deviceError(
        `a viewport was set (command ${sequence}) with no render pass open`
      );
      return;
    }
    const { x, y, width, height, depthMin, depthMax } = command.viewport;
    this.#currentPass.setViewport(x, y, width, height, depthMin, depthMax);
  }

  /**
   * Sets the scissor rectangle on the open render pass — `Rect2d` →
   * `setScissorRect(x, y, width, height)`.
   *
   * ON THE PASS, for {@link Replayer#setViewport}'s reason. The wire `x`/`y` are
   * signed; `setScissorRect` takes unsigned coordinates and the browser rejects a
   * scissor that runs outside the attachment, so a negative origin surfaces there
   * as a device error rather than being clamped here — the seam carries what the
   * caller gave. With no pass open it goes to the error queue.
   *
   * @param {bigint} sequence
   * @param {{ rect: { x: number, y: number, width: number, height: number } }}
   *   command
   */
  #setScissor(sequence, command) {
    if (!this.#currentPass) {
      this.#deviceError(
        `a scissor was set (command ${sequence}) with no render pass open`
      );
      return;
    }
    const { x, y, width, height } = command.rect;
    this.#currentPass.setScissorRect(x, y, width, height);
  }

  /**
   * Sets the stencil reference on the open render pass — the wire `u32` →
   * `setStencilReference(reference)`.
   *
   * ON THE PASS, for {@link Replayer#setViewport}'s reason, and the value passes
   * straight through: `GPUStencilValue` is a `u32` too, so there is nothing to
   * narrow and nothing a negative could mean. What the pipeline's
   * `stencilReadMask` does not cover, the comparison does not see — which is the
   * pipeline's business, not this arm's. With no pass open it goes to the error
   * queue, {@link Replayer#setScissor}'s judgement: a mid-frame ordering fault on
   * the far side, not a reason to abandon the rest of the frame.
   *
   * @param {bigint} sequence
   * @param {{ reference: number }} command
   */
  #setStencilReference(sequence, command) {
    if (!this.#currentPass) {
      this.#deviceError(
        `a stencil reference was set (command ${sequence}) with no render pass open`
      );
      return;
    }
    this.#currentPass.setStencilReference(command.reference);
  }

  /**
   * Opens a compute pass on the implicit-current encoder.
   *
   * `ComputePassDesc` is only a label — compute has no attachments — so the whole
   * of the descriptor is `{ label }`, and the label is omitted when absent so the
   * descriptor an unlabelled pass hands `beginComputePass` is empty.
   *
   * A PASS WITH NO ENCODER GOES TO THE ERROR QUEUE and does not throw — a far side
   * that got its ordering wrong mid-frame, {@link Replayer#takeError}'s case, not
   * a reason to abandon the rest of the frame. {@link #currentComputePass} is left
   * `null` so a later `EndComputePass` is itself a named malformed-stream error.
   *
   * @param {bigint} sequence
   * @param {{ label: string | null,
   *           timestampWrites?: { set: { index: number, generation: number },
   *                               beginningOfPass: number, endOfPass: number } | null }} command
   */
  #beginComputePass(sequence, command) {
    if (!this.#currentEncoder) {
      this.#deviceError(
        `compute pass (command ${sequence}) was begun with no command encoder open`
      );
      return;
    }
    const descriptor = command.label === null ? {} : { label: command.label };
    const timestampWrites = this.#resolveTimestampWrites(
      `compute pass (command ${sequence})`,
      command.timestampWrites
    );
    if (timestampWrites === undefined) return;
    if (timestampWrites !== null) descriptor.timestampWrites = timestampWrites;
    this.#currentComputePass =
      this.#currentEncoder.beginComputePass(descriptor);
  }

  /**
   * Closes the compute pass on the implicit-current encoder.
   *
   * ENDING WITH NO OPEN PASS IS A MALFORMED STREAM, routed to the error queue
   * rather than thrown — {@link Replayer#endRenderPass}'s judgement on the compute
   * pass.
   *
   * @param {bigint} sequence
   */
  #endComputePass(sequence) {
    if (!this.#currentComputePass) {
      this.#deviceError(
        `a compute pass was ended (command ${sequence}) with none open`
      );
      return;
    }
    this.#currentComputePass.end();
    this.#currentComputePass = null;
  }

  /**
   * Binds a compute pipeline onto the open compute pass.
   *
   * A BIND WITH NO COMPUTE PASS OPEN, OR AN UNRESOLVABLE PIPELINE, GOES TO THE
   * ERROR QUEUE and does not throw — {@link Replayer#bindGraphicsPipeline}'s
   * judgement on the compute pass and its pipeline table.
   *
   * @param {bigint} sequence
   * @param {{ pipeline: { index: number, generation: number } }} command
   */
  #bindComputePipeline(sequence, command) {
    if (!this.#currentComputePass) {
      this.#deviceError(
        `a compute pipeline was bound (command ${sequence}) with no compute pass open`
      );
      return;
    }
    const pipeline = this.#computePipelines.get(command.pipeline);
    if (pipeline === undefined) {
      this.#deviceError(
        `a compute pipeline was bound (command ${sequence}) naming pipeline ` +
          `${command.pipeline.index}.${command.pipeline.generation}, which this ` +
          'replayer holds no live compute pipeline under'
      );
      return;
    }
    this.#currentComputePass.setPipeline(pipeline);
  }

  /**
   * Records a dispatch on the open compute pass.
   *
   * A DISPATCH WITH NO COMPUTE PASS OPEN GOES TO THE ERROR QUEUE and does not
   * throw — the same mid-frame ordering fault the binds refuse. The three counts
   * are workgroup counts, handed straight to `dispatchWorkgroups`.
   *
   * @param {bigint} sequence
   * @param {{ x: number, y: number, z: number }} command
   */
  #dispatch(sequence, command) {
    if (!this.#currentComputePass) {
      this.#deviceError(
        `a dispatch was recorded (command ${sequence}) with no compute pass open`
      );
      return;
    }
    this.#currentComputePass.dispatchWorkgroups(
      command.x,
      command.y,
      command.z
    );
  }

  /**
   * Records an indirect dispatch on the open compute pass — `DispatchIndirect` →
   * `dispatchWorkgroupsIndirect(indirectBuffer, indirectOffset)`.
   *
   * NOT UNROLLED, unlike {@link Replayer#drawIndirect}: WebGPU's indirect
   * dispatch is a single dispatch and so is the HAL call, so exactly one call is
   * made and the command carries no count and no stride. The argument structure
   * WebGPU reads is `[workgroupCountX, workgroupCountY, workgroupCountZ]`, and
   * the `u64` offset is narrowed with `Number` as every other buffer offset here
   * is.
   *
   * A DISPATCH WITH NO COMPUTE PASS OPEN, OR AN UNRESOLVABLE ARGUMENT BUFFER,
   * GOES TO THE ERROR QUEUE and does not throw — {@link Replayer#dispatch}'s and
   * {@link Replayer#drawIndirect}'s judgement.
   *
   * @param {bigint} sequence
   * @param {{ buffer: { index: number, generation: number }, offset: bigint }}
   *   command
   */
  #dispatchIndirect(sequence, command) {
    const named = `an indirect dispatch (command ${sequence})`;
    if (!this.#currentComputePass) {
      this.#deviceError(`${named} was recorded with no compute pass open`);
      return;
    }
    const buffer = this.#buffers.get(command.buffer);
    if (buffer === undefined) {
      this.#deviceError(
        `${named} reads its workgroup counts from buffer ` +
          `${command.buffer.index}.${command.buffer.generation}, which this ` +
          'replayer holds no live buffer under'
      );
      return;
    }
    this.#currentComputePass.dispatchWorkgroupsIndirect(
      buffer,
      Number(command.offset)
    );
  }

  /**
   * The shared buffer↔texture copy layout — the mapping both
   * {@link Replayer#copyImageToBuffer} and {@link Replayer#copyBufferToImage}
   * need, resolved once from a `crcbl_hal::BufferImageCopy`.
   *
   * THE 256-BYTE TRAP. `BufferImageCopy::buffer_row_length` is in TEXELS (`0` =
   * tightly packed), and WebGPU's `bytesPerRow` is in BYTES and must be a
   * multiple of {@link COPY_BYTES_PER_ROW_ALIGNMENT}. So the texel pitch is
   * converted through the format's block footprint here, and the result is
   * passed through UNPADDED: padding it would change the buffer layout the other
   * side expects, so a misaligned pitch is left for WebGPU to refuse on
   * `uncapturederror` rather than silently repaired. The copy-chain probe picks
   * a 64×64 `rgba8unorm` texture precisely so its natural row is 64 × 4 = 256
   * bytes, already aligned, and the happy path needs no padding at all.
   *
   * AND THE CONVERSION IS IN BLOCKS, WHICH IS NOT THE SAME STATEMENT. WebGPU
   * measures a buffer layout's two pitches in whole texel blocks and the seam
   * measures both in texels, so a block-compressed format's row is a quarter of
   * the texels it names — {@link BLOCK_FOOTPRINT} carries the extent to divide
   * by, and carries `1 × 1` for every uncompressed format so the one formula
   * covers them too. The copy `size` is the exception and stays in TEXELS: it is
   * a `GPUExtent3D`, which WebGPU keeps in texels for every format.
   *
   * The texture side's origin comes from {@link copyOrigin}: a page's layer
   * arrives in `imageSubresource.baseLayer`, and WebGPU wants it in `origin.z`.
   *
   * THE ASPECT IS PART OF THE COPY, not decoration. A colour format has one
   * plane and `'all'` is the only aspect it accepts, so the colour path reads as
   * though the field were absent; a depth-stencil format accepts exactly one
   * named plane and rejects `'all'`, and which plane it is decides both the
   * footprint above and whether this direction is legal at all —
   * {@link DEPTH_STENCIL_COPY} is where those two facts live.
   *
   * Returns the `GPUImageCopyTexture`, the `GPUImageDataLayout` WITHOUT its
   * buffer — the caller adds the resolved buffer, whose role (source or
   * destination) and error wording differ by direction — and the copy size; or
   * queues a `#deviceError` naming `named` and returns `null` when the image is
   * unresolvable, its aspect is one WebGPU cannot spell, or the format and
   * aspect together have no copy in this direction.
   *
   * @param {object} command
   * @param {string} named
   * @param {'source'|'destination'} role Which side of the copy the TEXTURE is
   *   — the direction a depth or stencil plane is permitted separately for.
   * @returns {{ texture: object,
   *   textureView: { texture: object, mipLevel: number,
   *     origin: { x: number, y: number, z: number }, aspect: string },
   *   bufferLayout: { offset: number, bytesPerRow: number, rowsPerImage: number },
   *   size: { width: number, height: number, depthOrArrayLayers: number } } | null}
   */
  #textureCopyLayout(command, named, role) {
    const texture = this.#images.get(command.image);
    if (texture === undefined) {
      this.#deviceError(
        `${named} names image ${command.image.index}.${command.image.generation}, ` +
          'which this replayer holds no live image under'
      );
      return null;
    }
    const aspect = webgpuTextureAspectFor(command.imageSubresource.aspect);
    if (aspect.aspect === null) {
      this.#deviceError(`${named} ${aspect.reason}`);
      return null;
    }
    const planes = DEPTH_STENCIL_COPY[texture.format];
    let block;
    if (planes === undefined) {
      block = BLOCK_FOOTPRINT[texture.format];
      if (block === undefined) {
        this.#deviceError(
          `${named} touches a ${texture.format} texture, which has no block footprint ` +
            'this replayer can turn buffer_row_length into a bytesPerRow with'
        );
        return null;
      }
    } else if (aspect.aspect === 'all') {
      this.#deviceError(
        `${named} resolves to the 'all' aspect of a ${texture.format} texture. A buffer↔texture ` +
          'copy of a depth-stencil format moves exactly one plane, so it must name the depth or ' +
          'the stencil one'
      );
      return null;
    } else if (planes[aspect.aspect] === undefined) {
      this.#deviceError(
        `${named} names the ${aspect.aspect} plane of a ${texture.format} texture, which WebGPU ` +
          'copies in neither direction: depth24plus leaves its depth plane to the driver and so ' +
          'has no layout to lay a buffer out against, and a format without a stencil plane has ' +
          'no stencil aspect to copy'
      );
      return null;
    } else if (!planes[aspect.aspect][role]) {
      this.#deviceError(
        `${named} makes the ${aspect.aspect} plane of a ${texture.format} texture the copy's ` +
          `${role}, which WebGPU's depth-stencil formats table does not permit — that plane ` +
          `copies in the other direction only`
      );
      return null;
    } else {
      // No depth-stencil format is block compressed, so a plane's block is one
      // texel wide and one texel tall and holds that plane's own byte count.
      block = { width: 1, height: 1, bytes: planes[aspect.aspect].bytes };
    }
    // `0` means tightly packed, so the pitch is the copy extent; otherwise the
    // caller's explicit one.
    const rowTexels =
      command.bufferRowLength === 0
        ? command.imageExtent.width
        : command.bufferRowLength;
    const imageTexelRows =
      command.bufferImageHeight === 0
        ? command.imageExtent.height
        : command.bufferImageHeight;
    // BOTH PITCHES ARRIVE IN TEXELS AND WEBGPU WANTS BLOCKS, so each is divided
    // by the format's block extent — by `1` for an uncompressed format, which is
    // why there is one formula here and not two. Rounded up rather than
    // truncated: a copy that reaches the image's edge may end in a partial
    // block, and a partial block still occupies a whole one in the buffer, so
    // truncating would hand WebGPU a `GPUSize32` short of a block per row.
    // The byte pitch is passed through UNPADDED — see the 256-byte trap above.
    const bytesPerRow = Math.ceil(rowTexels / block.width) * block.bytes;
    const rowsPerImage = Math.ceil(imageTexelRows / block.height);
    return {
      texture,
      textureView: {
        texture,
        mipLevel: command.imageSubresource.mip,
        origin: copyOrigin(command.imageSubresource, command.imageOffset),
        aspect: aspect.aspect,
      },
      bufferLayout: {
        offset: Number(command.bufferOffset),
        bytesPerRow,
        rowsPerImage,
      },
      size: {
        width: command.imageExtent.width,
        height: command.imageExtent.height,
        depthOrArrayLayers: command.imageExtent.depthOrLayers,
      },
    };
  }

  /**
   * Records an image→buffer copy on the implicit-current encoder — the readback
   * path's copy.
   *
   * The buffer is the copy's DESTINATION, so the TEXTURE is the copy's
   * `'source'` — which is the direction a depth plane is permitted separately
   * for, and the reason that word is passed down.
   * {@link Replayer#textureCopyLayout} resolves the image side and the
   * 256-byte-trap layout. A missing encoder, an unresolvable buffer or image, or
   * a format and aspect with no readable footprint all go to the error queue.
   *
   * @param {bigint} sequence
   * @param {object} command
   */
  #copyImageToBuffer(sequence, command) {
    const named = `image→buffer copy (command ${sequence})`;
    if (!this.#currentEncoder) {
      this.#deviceError(`${named} was recorded with no command encoder open`);
      return;
    }
    const buffer = this.#buffers.get(command.buffer);
    if (buffer === undefined) {
      this.#deviceError(
        `${named} copies into buffer ${command.buffer.index}.${command.buffer.generation}, ` +
          'which this replayer holds no live buffer under'
      );
      return;
    }
    const layout = this.#textureCopyLayout(command, named, 'source');
    if (!layout) return;
    this.#currentEncoder.copyTextureToBuffer(
      layout.textureView,
      { buffer, ...layout.bufferLayout },
      layout.size
    );
  }

  /**
   * Records a buffer→image copy on the implicit-current encoder — the upload
   * counterpart of {@link Replayer#copyImageToBuffer}.
   *
   * The buffer is the copy's SOURCE, so the TEXTURE is the copy's
   * `'destination'` — and this is the direction WebGPU withholds from a float
   * depth plane, so a `depth32float` upload is refused here while the readback
   * above is not. {@link Replayer#textureCopyLayout} resolves the image side and
   * the 256-byte-trap layout. `copyBufferToTexture` takes its arguments
   * (source = buffer layout, destination = texture view, size) in the OPPOSITE
   * order to `copyTextureToBuffer`. A missing encoder, an unresolvable buffer or
   * image, or a format and aspect with no writable footprint all go to the error
   * queue.
   *
   * @param {bigint} sequence
   * @param {object} command
   */
  #copyBufferToImage(sequence, command) {
    const named = `buffer→image copy (command ${sequence})`;
    if (!this.#currentEncoder) {
      this.#deviceError(`${named} was recorded with no command encoder open`);
      return;
    }
    const buffer = this.#buffers.get(command.buffer);
    if (buffer === undefined) {
      this.#deviceError(
        `${named} copies from buffer ${command.buffer.index}.${command.buffer.generation}, ` +
          'which this replayer holds no live buffer under'
      );
      return;
    }
    const layout = this.#textureCopyLayout(command, named, 'destination');
    if (!layout) return;
    this.#currentEncoder.copyBufferToTexture(
      { buffer, ...layout.bufferLayout },
      layout.textureView,
      layout.size
    );
  }

  /**
   * Records an image→image copy on the implicit-current encoder.
   *
   * Both sides are textures — each with its own mip level, array layer and texel
   * origin — so this resolves the two images and maps straight to
   * `copyTextureToTexture`; there is no buffer layout and no 256-byte trap. Each
   * side's origin comes from {@link copyOrigin}, which is where the subresource's
   * array layer joins the offset's `z`. A missing encoder or an unresolvable
   * source or destination image goes to the error queue naming the handle,
   * distinctly by direction.
   *
   * @param {bigint} sequence
   * @param {{ copy: { src: { index: number, generation: number },
   *   srcSubresource: { mip: number, baseLayer: number },
   *   srcOffset: { x: number, y: number, z: number },
   *   dst: { index: number, generation: number },
   *   dstSubresource: { mip: number, baseLayer: number },
   *   dstOffset: { x: number, y: number, z: number },
   *   extent: { width: number, height: number, depthOrLayers: number } } }} command
   */
  #copyImageToImage(sequence, command) {
    const named = `image→image copy (command ${sequence})`;
    if (!this.#currentEncoder) {
      this.#deviceError(`${named} was recorded with no command encoder open`);
      return;
    }
    const { copy } = command;
    const src = this.#images.get(copy.src);
    if (src === undefined) {
      this.#deviceError(
        `${named} copies from image ${copy.src.index}.${copy.src.generation}, ` +
          'which this replayer holds no live image under'
      );
      return;
    }
    const dst = this.#images.get(copy.dst);
    if (dst === undefined) {
      this.#deviceError(
        `${named} copies into image ${copy.dst.index}.${copy.dst.generation}, ` +
          'which this replayer holds no live image under'
      );
      return;
    }
    this.#currentEncoder.copyTextureToTexture(
      {
        texture: src,
        mipLevel: copy.srcSubresource.mip,
        origin: copyOrigin(copy.srcSubresource, copy.srcOffset),
      },
      {
        texture: dst,
        mipLevel: copy.dstSubresource.mip,
        origin: copyOrigin(copy.dstSubresource, copy.dstOffset),
      },
      {
        width: copy.extent.width,
        height: copy.extent.height,
        depthOrArrayLayers: copy.extent.depthOrLayers,
      }
    );
  }

  /**
   * Records a buffer fill on the implicit-current encoder.
   *
   * WEBGPU HAS NO VALUED DEVICE-SIDE FILL. Its `clearBuffer` zeroes and nothing
   * else, so a `value` of `0` maps to `clearBuffer(buffer, offset, size)` and any
   * other value is a write this replayer refuses to the error queue — the same
   * judgement the `crcbl-wgpu` backend makes. A missing encoder or an
   * unresolvable buffer also goes to the error queue.
   *
   * @param {bigint} sequence
   * @param {{ buffer: { index: number, generation: number }, offset: bigint,
   *   size: bigint, value: number }} command
   */
  #clearBuffer(sequence, command) {
    const named = `buffer clear (command ${sequence})`;
    if (!this.#currentEncoder) {
      this.#deviceError(`${named} was recorded with no command encoder open`);
      return;
    }
    const buffer = this.#buffers.get(command.buffer);
    if (buffer === undefined) {
      this.#deviceError(
        `${named} clears buffer ${command.buffer.index}.${command.buffer.generation}, ` +
          'which this replayer holds no live buffer under'
      );
      return;
    }
    this.#currentEncoder.clearBuffer(
      buffer,
      Number(command.offset),
      Number(command.size)
    );
  }

  /**
   * Uploads bytes into a buffer through the queue — `Device::write_buffer`.
   *
   * ON THE QUEUE, NOT AN ENCODER. `write_buffer` is a `Device` method, not a
   * `CommandEncoder` one, so this needs {@link #device} rather than an open
   * encoder — `queue.writeBuffer` submits its own copy directly, between frames
   * and without a command buffer. A write before any device opened, or of an
   * unresolvable buffer, goes to the error queue naming the handle rather than
   * throwing — {@link Replayer#clearBuffer}'s judgement.
   *
   * The `u64` offset arrives as `BigInt` and is narrowed with `Number` for the
   * WebGPU call, exactly as {@link Replayer#clearBuffer}'s is: an offset past
   * `Number.MAX_SAFE_INTEGER` is not one this seam produces. The bytes are the
   * `Uint8Array` the decoder read them into.
   *
   * @param {bigint} sequence
   * @param {{ buffer: { index: number, generation: number }, offset: bigint,
   *   data: Uint8Array }} command
   */
  #writeBuffer(sequence, command) {
    const named = `buffer write (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} ran before any device opened`);
      return;
    }
    const buffer = this.#buffers.get(command.buffer);
    if (buffer === undefined) {
      this.#deviceError(
        `${named} writes buffer ${command.buffer.index}.${command.buffer.generation}, ` +
          'which this replayer holds no live buffer under'
      );
      return;
    }
    this.#device.queue.writeBuffer(
      buffer,
      Number(command.offset),
      command.data
    );
  }

  /**
   * Records a buffer→buffer copy on the implicit-current encoder — the path a
   * dispatch's storage-buffer output takes to a host-readable buffer.
   *
   * On the encoder, NOT a pass: a copy is recorded between passes, so this needs
   * {@link #currentEncoder} rather than an open pass. A missing encoder or an
   * unresolvable source or destination buffer goes to the error queue naming the
   * handle, {@link Replayer#copyImageToBuffer}'s judgement.
   *
   * The three `u64` fields — the two offsets and the size — arrive as `BigInt`
   * and are narrowed with `Number` for the WebGPU call, exactly as
   * {@link Replayer#copyImageToBuffer}'s `bufferOffset` is: a copy region past
   * `Number.MAX_SAFE_INTEGER` is not one this seam produces.
   *
   * @param {bigint} sequence
   * @param {{ copy: { src: { index: number, generation: number },
   *           srcOffset: bigint, dst: { index: number, generation: number },
   *           dstOffset: bigint, size: bigint } }} command
   */
  #copyBufferToBuffer(sequence, command) {
    const named = `buffer→buffer copy (command ${sequence})`;
    if (!this.#currentEncoder) {
      this.#deviceError(`${named} was recorded with no command encoder open`);
      return;
    }
    const { copy } = command;
    const src = this.#buffers.get(copy.src);
    if (src === undefined) {
      this.#deviceError(
        `${named} copies from buffer ${copy.src.index}.${copy.src.generation}, ` +
          'which this replayer holds no live buffer under'
      );
      return;
    }
    const dst = this.#buffers.get(copy.dst);
    if (dst === undefined) {
      this.#deviceError(
        `${named} copies into buffer ${copy.dst.index}.${copy.dst.generation}, ` +
          'which this replayer holds no live buffer under'
      );
      return;
    }
    this.#currentEncoder.copyBufferToBuffer(
      src,
      Number(copy.srcOffset),
      dst,
      Number(copy.dstOffset),
      Number(copy.size)
    );
  }

  /**
   * The documented no-op: a pipeline barrier records NOTHING on the encoder.
   *
   * THIS IS THE ONE ARM WHOSE CORRECTNESS IS "DOES NOTHING, ON PURPOSE." WebGPU
   * serialises its single implicit queue and inserts every hazard barrier itself
   * — the same reason a `Submit`'s `waits`/`signals` have no home (see
   * {@link Replayer#submit}) — so there is no transition for this to record and
   * nothing for a `commandEncoder` to do with the barrier lists. wasm still sends
   * them for wire fidelity (the barrier is a faithful transposition of the HAL
   * call), and {@link decodeCommand} decodes them whole, but replay ends here.
   *
   * Unlike the copies and the fill, this does NOT route a missing encoder to the
   * error queue: a barrier with no encoder open is still nothing to do, so there
   * is no failure to report. It is recognised — reaching this method rather than
   * the dispatch `default` throw is the whole point — and it returns having
   * touched neither {@link Replayer#currentEncoder} nor the device.
   *
   * @param {bigint} _sequence
   * @param {{ buffers: object[], images: object[], global: boolean }} _command
   */
  #pipelineBarrier(_sequence, _command) {
    // Intentionally empty — see the doc comment above.
  }

  /**
   * Seals the implicit-current encoder into a command buffer at the handle wasm
   * allocated.
   *
   * A FINISH WITH NO ENCODER OPEN IS A MALFORMED STREAM, into the error queue.
   * On success `#currentEncoder` is cleared, so a stray recording command after
   * it is a named error rather than a call on a finished encoder.
   *
   * @param {bigint} sequence
   * @param {{ commandBuffer: { index: number, generation: number } }} command
   */
  #finish(sequence, command) {
    if (!this.#currentEncoder) {
      this.#deviceError(
        `an encoder was finished (command ${sequence}) with none open`
      );
      return;
    }
    let buffer;
    try {
      buffer = this.#currentEncoder.finish();
    } catch (error) {
      this.#deviceError(
        `the command buffer ${command.commandBuffer.index}.${command.commandBuffer.generation} ` +
          `(command ${sequence}) could not be finished: ${String(error)}`
      );
      this.#currentEncoder = null;
      this.#currentPass = null;
      this.#currentComputePass = null;
      this.#debugGroups.length = 0;
      return;
    }
    this.#commandBuffers.insert(command.commandBuffer, buffer);
    this.#currentEncoder = null;
    this.#currentPass = null;
    this.#currentComputePass = null;
    this.#debugGroups.length = 0;
  }

  /**
   * Submits command buffers to the device's one implicit queue.
   *
   * A NON-EMPTY `waits` OR `signals` IS REFUSED BY NAME. WebGPU serialises its
   * single queue and inserts hazard barriers itself, so it has no semaphores at
   * all — dropping a wait would be a silent synchronisation bug, and the engine's
   * real frame will carry them, so this is the loud refusal
   * `docs/plan/41-webgpu-stream.md`'s reasoning asks for rather than a quiet
   * omission. An unresolvable command buffer is refused too. Both are the far
   * side's bug, so both go to the error queue rather than throwing.
   *
   * @param {bigint} sequence
   * @param {object} command
   */
  #submit(sequence, command) {
    const named = `submit (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} ran before any device opened`);
      return;
    }
    if (command.waits.length > 0 || command.signals.length > 0) {
      this.#deviceError(
        `${named} carries ${command.waits.length} wait(s) and ${command.signals.length} ` +
          'signal(s), and WebGPU has no semaphores to satisfy them — its single queue ' +
          'is ordered and it inserts hazard barriers itself'
      );
      return;
    }
    const buffers = [];
    for (let i = 0; i < command.commandBuffers.length; i += 1) {
      const handle = command.commandBuffers[i];
      const buffer = this.#commandBuffers.get(handle);
      if (buffer === undefined) {
        this.#deviceError(
          `${named} names command buffer ${handle.index}.${handle.generation}, which this ` +
            'replayer holds no finished command buffer under'
        );
        return;
      }
      buffers.push(buffer);
    }
    this.#device.queue.submit(buffers);
  }

  /**
   * Starts a readback: maps the buffer asynchronously and files the in-flight
   * request under the handle wasm allocated.
   *
   * NO REPLY — the handle came in with the command, so nothing is waiting to
   * learn it; {@link Replayer#pollReadback} is what is answered. The `mapAsync`
   * is NOT awaited inside `replay`: the request is filed `'mapping'`, and when
   * the promise resolves the mapped range is copied out and the state becomes
   * `'ready'`. Copied, not viewed — `getMappedRange` returns a view that
   * `unmap`/destroy invalidates, so a poll a frame later would read a detached
   * buffer.
   *
   * REFUSALS WEBGPU CANNOT EXPRESS, each into the error queue AND onto the
   * request itself: a request before any device opened, a `Some` `after` (a
   * semaphore wait — WebGPU's `mapAsync` is exactly "everything submitted so
   * far", the `None` case, and it has no way to wait on a value), and an
   * unresolvable buffer. A `mapAsync` that rejects — a buffer destroyed before
   * it resolved, a range the browser will not map — and a `getMappedRange` that
   * throws after it resolved land there too. A DEVICE LOST MID-MAP DOES NOT:
   * it is one event for the whole replayer rather than this request's own
   * failure, so {@link Replayer#loseDevice} files it on every entry in flight
   * with the reason the loss composed, and the `AbortError` that then arrives
   * here is left alone.
   *
   * BOTH DESTINATIONS, AND THEY ANSWER DIFFERENT QUESTIONS. The error queue is
   * what `Device::take_error` drains, which tells the engine its device is
   * unwell; {@link Replayer#pollReadback} is what tells the one caller waiting
   * on *these bytes* that they are never arriving. Recording only the first is
   * what used to leave a request filed `'mapping'` for ever, with
   * `poll_readback` answering `Pending` every frame to a caller that had no
   * other way to stop.
   *
   * NEITHER DESTINATION HEARS FROM A READBACK THAT WAS ABANDONED. A caller may
   * {@link Replayer#destroyReadback} one whose map has not settled — the seam
   * allows it, and `crcbl-render`'s culling-statistics ring releases a slot's
   * request once per frame whether or not it settled — and the `unmap` that
   * release consists of is specified to reject the map in flight with an
   * `AbortError`. That rejection is this replayer cancelling its
   * own request, not the device reporting a fault, so the handlers below leave
   * on the flag rather than filing it; see {@link Replayer#destroyReadback} for
   * why there is nothing to tell it apart from a genuine one by.
   *
   * @param {bigint} sequence
   * @param {object} command
   */
  #requestReadback(sequence, command) {
    const named = `readback ${command.readback.index}.${command.readback.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#failReadback(
        command.readback,
        `${named} was requested before any device opened`
      );
      return;
    }
    if (command.after !== null) {
      this.#failReadback(
        command.readback,
        `${named} names an 'after' semaphore, which WebGPU has no way to wait on — ` +
          'its mapAsync observes everything submitted so far and nothing finer'
      );
      return;
    }
    const buffer = this.#buffers.get(command.buffer);
    if (buffer === undefined) {
      this.#failReadback(
        command.readback,
        `${named} reads buffer ${command.buffer.index}.${command.buffer.generation}, which this ` +
          'replayer holds no live buffer under'
      );
      return;
    }
    const offset = Number(command.offset);
    const size = Number(command.size);
    const entry = {
      buffer,
      offset,
      size,
      state: 'mapping',
      bytes: null,
      reason: null,
      abandoned: false,
    };
    this.#readbacks.insert(command.readback, entry);
    this.#inFlight += 1;
    buffer
      .mapAsync(GPU_MAP_READ, offset, size)
      .then(() => {
        // Resolved and then destroyed, in that order and inside one turn of the
        // event loop: the destroy has already unmapped, so there is nothing left
        // to copy out and `getMappedRange` would throw on a buffer whose mapping
        // is gone. Nobody can ask for the bytes either — the entry left the
        // table with the destroy.
        if (entry.abandoned) return;
        // The device died while this was in flight: the entry is already filed
        // `'failed'` with the loss, and a map that somehow resolved anyway is
        // reading memory a dead device owns.
        if (this.#lost !== null) return;
        // A fresh copy: `getMappedRange` is a view onto memory `unmap` reclaims,
        // so the bytes have to leave it before a destroy can.
        entry.bytes = new Uint8Array(
          buffer.getMappedRange(offset, size).slice(0)
        );
        entry.state = 'ready';
      })
      .catch((error) => {
        // The `.catch` covers the `.then` above it as well as the map, which is
        // deliberate: a buffer destroyed between the two makes `getMappedRange`
        // throw, and that is the same outcome for the same request.
        //
        // Except when the request was abandoned, where the rejection is the
        // destroy's own `unmap` arriving back. Filing it made the ordinary
        // release of an outstanding readback look like a device fault, and
        // `Engine::acquire` takes any `take_error` message as a reason to stop
        // the frame loop.
        if (entry.abandoned) return;
        // AND WHEN THE DEVICE DIED, THIS REJECTION IS THE DEATH ARRIVING
        // THROUGH THIS CALL. WebGPU rejects an outstanding map with an
        // `AbortError` when the device goes, and filing that is how a lost
        // device got reported as "readback N could not be mapped: AbortError" —
        // a sentence naming a call that was fine. {@link Replayer#loseDevice}
        // has already filed this entry with the loss by then, so the rejection
        // is left alone — "by then" being the specification's order for "lose
        // the device", which resolves `lost` before it completes the steps that
        // wait on a loss, and what a real browser was watched doing under a real
        // loss. `docs/backlog.md` records the one ordering that does not hold.
        if (this.#lost !== null) return;
        const reason = `${named} could not be mapped: ${String(error)}`;
        this.#deviceError(reason);
        entry.state = 'failed';
        entry.reason = reason;
      })
      .finally(() => {
        this.#inFlight -= 1;
      });
  }

  /**
   * Files a readback as failed before it ever reached the browser, and records
   * the reason on the error queue too.
   *
   * The entry exists so the poll has something to answer: a refusal that filed
   * nothing leaves {@link Replayer#pollReadback} looking at an absent handle,
   * which is the same dead end from the caller's side.
   *
   * @param {{ index: number, generation: number }} readback
   * @param {string} reason
   */
  #failReadback(readback, reason) {
    this.#deviceError(reason);
    this.#readbacks.insert(readback, {
      buffer: null,
      offset: 0,
      size: 0,
      state: 'failed',
      bytes: null,
      reason,
      abandoned: false,
    });
  }

  /**
   * Answers where a readback has got to, on the reply channel.
   *
   * ANSWERED WITHIN THE CALL, like {@link Replayer#surfaceCaps}: the map runs on
   * its own promise and this only reads the state it left, so deferring would add
   * a frame for nothing. A `'ready'` request answers {@link ReplyWriter#readbackReady}
   * with the copied bytes, a `'failed'` one {@link ReplyWriter#readbackFailed}
   * with the reason, and anything else {@link ReplyWriter#readbackPending}. Every
   * path queues exactly one reply naming this command's sequence, because a poll
   * that named nothing would leave the far side waiting for ever.
   *
   * PENDING IS ONLY HONEST WHILE SOMETHING IS STILL COMING. `Pending` means "ask
   * again next frame", so answering it to a request that has settled the wrong
   * way is an instruction to poll for ever — the caller has no deadline and no
   * other channel. That is why a rejected map and a handle nothing was requested
   * under both answer `ReadbackFailed`: neither will ever produce bytes, and the
   * far side turns the reason into a `HalError::DeviceLost` the caller can act
   * on. The error queue still gets a copy, for the engine's own health check.
   *
   * @param {bigint} sequence
   * @param {{ readback: { index: number, generation: number } }} command
   */
  #pollReadback(sequence, command) {
    const entry = this.#readbacks.get(command.readback);
    if (entry === undefined) {
      const reason =
        `poll (command ${sequence}) names readback ` +
        `${command.readback.index}.${command.readback.generation}, which this replayer holds ` +
        'no in-flight readback under';
      this.#deviceError(reason);
      this.#replies.readbackFailed(sequence, command.readback, reason);
      this.#queued = true;
      return;
    }
    if (entry.state === 'ready') {
      this.#replies.readbackReady(sequence, command.readback, entry.bytes);
    } else if (entry.state === 'failed') {
      this.#replies.readbackFailed(sequence, command.readback, entry.reason);
    } else {
      this.#replies.readbackPending(sequence, command.readback);
    }
    this.#queued = true;
  }

  /**
   * Releases a readback and unmaps its buffer.
   *
   * `GPUBuffer.unmap()` is the release WebGPU calls for — abandoning a readback
   * without it leaks a mapped buffer — and it is what makes destroying a readback
   * an explicit op on this seam. It does NOT destroy the buffer: the buffer has
   * its own `DestroyBuffer`.
   *
   * A DESTROY THAT NAMES NOTHING LIVE IS A NO-OP, in both of its ways — an empty
   * slot and a stale generation — because {@link HandleTable#remove} answers
   * `undefined` for both. A request refused before it reached the browser has no
   * buffer to unmap either: nothing was ever mapped for it.
   *
   * DESTROYING ONE WHOSE MAP HAS NOT SETTLED IS LEGAL, AND IT IS WHAT `abandoned`
   * IS FOR. `unmap()`'s first specified step is "if `this.[[pending_map]]` is not
   * null, reject `this.[[pending_map]]` with an `AbortError`", so cancelling a
   * map in flight is not a misuse of WebGPU — it *is* WebGPU's cancellation, and
   * the rejection is the acknowledgement rather than a failure. The flag is set
   * before the `unmap` so that {@link Replayer#requestReadback}'s handlers, which
   * hold this same entry, can leave rather than file it. Without it a caller that
   * gave up on an outstanding readback got the cancellation back as a device
   * error, which is what `Engine::acquire` stops the frame loop over.
   *
   * NOTHING TELLS THAT REJECTION APART FROM A GENUINE ONE, which is why the flag
   * and not a check of the error. WebGPU rejects a map with an `AbortError` when
   * the device is lost too, and the specification's map-failure steps say so in
   * as many words: "this is the same error type produced by cancelling the map
   * using `unmap()`". A device lost is not told apart *here* either, and does
   * not have to be: {@link Replayer#loseDevice} has already recorded it and
   * failed this entry with the reason by the time such a rejection arrives, so
   * the handler leaves on that rather than on this flag.
   *
   * THE SETTLED STATES NEED NO BRANCH. The same steps end "if `this.[[mapping]]`
   * is null, return", so unmapping a request whose map rejected — mapped nothing,
   * holds nothing — is a no-op rather than an error, and a `'ready'` one has had
   * its bytes copied out already.
   *
   * @param {{ readback: { index: number, generation: number } }} command
   */
  #destroyReadback(command) {
    const entry = this.#readbacks.remove(command.readback);
    if (!entry?.buffer) return;
    entry.abandoned = true;
    entry.buffer.unmap();
  }

  /**
   * Lets go of a finished command buffer.
   *
   * **Letting go is the whole of the release**: a `GPUCommandBuffer` has no
   * `destroy()`, so dropping the reference is everything there is to do, exactly
   * as it is for a pipeline or a view. A destroy naming nothing live is a no-op
   * in both of its ways.
   *
   * @param {{ commandBuffer: { index: number, generation: number } }} command
   */
  #destroyCommandBuffer(command) {
    this.#commandBuffers.remove(command.commandBuffer);
  }

  /**
   * Creates a `GPUQuerySet` and files it under the handle wasm allocated.
   *
   * {@link Replayer#createBuffer}'s shape — synchronous, no reply, every failure
   * to the error queue — with one refusal of its own beside the usual three.
   *
   * **`GPUQueryType` IS EXACTLY `'occlusion'` AND `'timestamp'`**, so a
   * `PipelineStatistics` set has nothing to become and is refused by name.
   *
   * `'occlusion'` needs no `GPUFeatureName`, so nothing here consults the
   * device's features for it: every device this replayer opens serves it.
   * `'timestamp'` needs `'timestamp-query'`, and `createQuerySet` on a device
   * without it throws — refused by name here instead, so the reason names the
   * command rather than arriving as an uncaught validation error a frame later.
   * The seam refuses the same set on the same flag before it is encoded, so this
   * is the two halves agreeing rather than one of them deciding.
   *
   * @param {bigint} sequence
   * @param {{ set: { index: number, generation: number }, label: string | null,
   *           kind: string, count: number }} command
   */
  #createQuerySet(sequence, command) {
    const named = `query set ${command.set.index}.${command.set.generation} (command ${sequence})`;
    if (!this.#device) {
      this.#deviceError(`${named} was created before any device opened`);
      return;
    }
    if (command.kind !== 'Occlusion' && command.kind !== 'Timestamp') {
      this.#deviceError(
        `${named} asks for a ${command.kind} query set, and GPUQueryType is exactly ` +
          "'occlusion' and 'timestamp', so there is no such set to create"
      );
      return;
    }
    if (
      command.kind === 'Timestamp' &&
      !this.#device.features?.has('timestamp-query')
    ) {
      this.#deviceError(
        `${named} asks for a timestamp query set, and this device opened without the ` +
          "'timestamp-query' feature"
      );
      return;
    }
    let set;
    try {
      set = this.#device.createQuerySet({
        ...(command.label === null ? {} : { label: command.label }),
        type: command.kind === 'Timestamp' ? 'timestamp' : 'occlusion',
        count: command.count,
      });
    } catch (error) {
      this.#deviceError(`${named} could not be created: ${String(error)}`);
      return;
    }
    this.#querySets.insert(command.set, { set, count: command.count });
  }

  /**
   * Destroys a query set and lets go of its slot.
   *
   * `GPUQuerySet.destroy()` is the release, exactly as it is for a buffer, and a
   * destroy naming nothing live is a no-op in both of its ways — an empty slot
   * and a stale generation. The empty slot is the ordinary case here rather than
   * an edge one: a caller that asked for a timestamp set got an `Err` from the
   * seam and still destroys the handle it pre-allocated.
   *
   * @param {{ set: { index: number, generation: number } }} command
   */
  #destroyQuerySet(command) {
    this.#querySets.remove(command.set)?.set.destroy();
  }

  /**
   * The documented no-op: **WebGPU has no query reset**, so this records
   * nothing.
   *
   * A `GPUQuerySet` is not a pool a caller re-arms, and the specification defines
   * an unwritten query to resolve as zero — so there is nothing for a reset to do
   * and nothing it could break. The seam requires every caller to record one all
   * the same, because Vulkan forbids reading a pool that was never reset and
   * making everybody call it keeps the Vulkan path from being the special case.
   *
   * **What it does do is check**, and that is the reason the command crosses at
   * all: `CommandEncoder::reset_query_set` returns `()`, so a range naming a set
   * this replayer does not hold — or running past that set's `count` — has
   * nowhere else to be reported. Recording nothing and checking nothing would be
   * indistinguishable from the command never having been sent.
   *
   * @param {bigint} sequence
   * @param {{ set: { index: number, generation: number }, firstQuery: number,
   *           queryCount: number }} command
   */
  #resetQuerySet(sequence, command) {
    const named = `a query reset (command ${sequence})`;
    if (!this.#currentEncoder) {
      this.#deviceError(`${named} ran with no command encoder open`);
      return;
    }
    this.#resolveQueryRange(named, command);
  }

  /**
   * Copies a query range into a buffer — `resolveQuerySet(querySet, firstQuery,
   * queryCount, destination, destinationOffset)`.
   *
   * **ON THE ENCODER, NOT A PASS.** `resolveQuerySet` is a `GPUCommandEncoder`
   * method, so a stream that reached it with a pass still open is a malformed
   * recording; it goes to the error queue rather than throwing, which is
   * {@link Replayer#setScissor}'s judgement about a mid-frame ordering fault.
   *
   * **TWO VALIDATION RULES ARE REFUSED BY NAME HERE**, and both are WebGPU's
   * rather than this stream's:
   *
   *   * `destinationOffset` must be a multiple of
   *     {@link QUERY_RESOLVE_BUFFER_ALIGNMENT}. `wgpu-core`'s
   *     `command::query::resolve_query_set` checks
   *     `destination_offset.is_multiple_of(QUERY_RESOLVE_BUFFER_ALIGNMENT)`
   *     before anything else about the call.
   *   * `destination` must carry `GPUBufferUsage.QUERY_RESOLVE`. The same
   *     function calls `check_usage(BufferUsages::QUERY_RESOLVE)` on it, and
   *     `GPUBuffer.usage` is readable here so the check needs nothing the
   *     browser has not already published.
   *
   * Both would otherwise be a WebGPU validation error reported on the device — a
   * frame later, attributed to no command — so naming them at the command that
   * carried them is what makes the diagnosis the recording rather than the
   * device.
   *
   * @param {bigint} sequence
   * @param {{ set: { index: number, generation: number }, firstQuery: number,
   *           queryCount: number, dst: { index: number, generation: number },
   *           dstOffset: bigint }} command
   */
  #resolveQuerySet(sequence, command) {
    const named = `a query resolve (command ${sequence})`;
    if (!this.#currentEncoder) {
      this.#deviceError(`${named} ran with no command encoder open`);
      return;
    }
    if (this.#currentPass || this.#currentComputePass) {
      this.#deviceError(
        `${named} ran inside an open pass, and resolveQuerySet is a GPUCommandEncoder method`
      );
      return;
    }
    const entry = this.#resolveQueryRange(named, command);
    if (!entry) return;
    const dst = this.#buffers.get(command.dst);
    if (dst === undefined) {
      this.#deviceError(
        `${named} resolves into buffer ${command.dst.index}.${command.dst.generation}, which this ` +
          'replayer holds no live buffer under'
      );
      return;
    }
    if (command.dstOffset % QUERY_RESOLVE_BUFFER_ALIGNMENT !== 0n) {
      this.#deviceError(
        `${named} resolves at destination offset ${command.dstOffset}, which WebGPU requires to ` +
          `be a multiple of ${QUERY_RESOLVE_BUFFER_ALIGNMENT}`
      );
      return;
    }
    if ((dst.usage & GPU_BUFFER_USAGE.QUERY_RESOLVE) === 0) {
      this.#deviceError(
        `${named} resolves into a buffer whose usage ${dst.usage} does not include ` +
          `QUERY_RESOLVE (${GPU_BUFFER_USAGE.QUERY_RESOLVE})`
      );
      return;
    }
    this.#currentEncoder.resolveQuerySet(
      entry.set,
      command.firstQuery,
      command.queryCount,
      dst,
      Number(command.dstOffset)
    );
  }

  /**
   * Resolves a command's query set and checks its range against that set's
   * `count`.
   *
   * Shared by the reset, the resolve and the direct read because all three name a
   * range of one set and all three are wrong in the same two ways — and because
   * an out-of-range range is what WebGPU reports out of band, so each of them
   * needs the same sentence at the command rather than a device error a frame
   * later.
   *
   * @param {string} named
   * @param {{ set: { index: number, generation: number }, firstQuery: number,
   *           queryCount: number }} command
   * @returns {{ set: GPUQuerySet, count: number } | null} The entry, or `null`
   *   once the reason is on the error queue.
   */
  #resolveQueryRange(named, command) {
    const entry = this.#querySets.get(command.set);
    if (entry === undefined) {
      this.#deviceError(
        `${named} names query set ${command.set.index}.${command.set.generation}, which this ` +
          'replayer holds no live query set under'
      );
      return null;
    }
    if (command.firstQuery + command.queryCount > entry.count) {
      this.#deviceError(
        `${named} covers queries ${command.firstQuery}..` +
          `${command.firstQuery + command.queryCount} of a ${entry.count}-query set`
      );
      return null;
    }
    return entry;
  }

  /**
   * Answers a direct query read — the values, on the reply channel.
   *
   * **WEBGPU CANNOT READ A `GPUQuerySet` AT ALL**, which is the whole shape of
   * this method: there is no accessor, so the only way to a value is to resolve
   * the range into a buffer, copy that into a mappable one and map it. Three
   * objects and a submit for what the seam calls a read, and it is not an
   * extravagance — it is what the API offers.
   *
   * ANSWERED LATER, like {@link Replayer#enumerateAdapters} and unlike
   * {@link Replayer#pollReadback}: the map is a promise, so the reply is queued
   * when it settles rather than inside this call. The far side's
   * `Device::query_results` reports that the answer is not here yet and asks
   * again next frame, which is the same protocol `Device::take_error` runs.
   *
   * **EXACTLY ONE REPLY ON EVERY PATH, AND AN EMPTY ONE MEANS FAILED.**
   * `Reply::QueryResults` has no failed counterpart, and a sequence nobody
   * answers stays registered on the far side for ever — so a read this replayer
   * cannot serve is answered with an empty `values` list and its reason goes on
   * the error queue. Empty is unambiguous because the seam never asks for zero
   * values: `Device::query_results` with an empty `out` is answered without
   * touching the wire.
   *
   * @param {bigint} sequence
   * @param {{ set: { index: number, generation: number }, firstQuery: number,
   *           queryCount: number }} command
   */
  #queryResults(sequence, command) {
    const named = `a query read (command ${sequence})`;
    if (!this.#device) {
      this.#failQueryResults(
        sequence,
        command,
        `${named} ran before any device opened`
      );
      return;
    }
    const entry = this.#querySets.get(command.set);
    if (entry === undefined) {
      this.#failQueryResults(
        sequence,
        command,
        `${named} names query set ${command.set.index}.${command.set.generation}, which this ` +
          'replayer holds no live query set under'
      );
      return;
    }
    if (command.firstQuery + command.queryCount > entry.count) {
      this.#failQueryResults(
        sequence,
        command,
        `${named} covers queries ${command.firstQuery}..` +
          `${command.firstQuery + command.queryCount} of a ${entry.count}-query set`
      );
      return;
    }
    const bytes = command.queryCount * QUERY_RESULT_BYTES;
    let resolved;
    let mapped;
    try {
      // Both are this replayer's own and neither outlives the read: the scratch
      // one exists because `resolveQuerySet` needs a QUERY_RESOLVE destination
      // and the mappable one because MAP_READ may not be combined with it.
      resolved = this.#device.createBuffer({
        label: 'crcbl query resolve',
        size: bytes,
        usage: GPU_BUFFER_USAGE.QUERY_RESOLVE | GPU_BUFFER_USAGE.COPY_SRC,
      });
      mapped = this.#device.createBuffer({
        label: 'crcbl query readback',
        size: bytes,
        usage: GPU_BUFFER_USAGE.COPY_DST | GPU_BUFFER_USAGE.MAP_READ,
      });
      const encoder = this.#device.createCommandEncoder({
        label: 'crcbl query read',
      });
      encoder.resolveQuerySet(
        entry.set,
        command.firstQuery,
        command.queryCount,
        resolved,
        0
      );
      encoder.copyBufferToBuffer(resolved, 0, mapped, 0, bytes);
      this.#device.queue.submit([encoder.finish()]);
    } catch (error) {
      resolved?.destroy();
      mapped?.destroy();
      this.#failQueryResults(
        sequence,
        command,
        `${named} could not be recorded: ${String(error)}`
      );
      return;
    }
    this.#inFlight += 1;
    mapped
      .mapAsync(GPU_MAP_READ, 0, bytes)
      .then(() => {
        // A fresh copy for `getMappedRange`'s reason — the view is onto memory
        // the destroy below reclaims — and a `BigUint64Array` because a resolved
        // query is a `u64` and the reply writer takes `BigInt`s.
        const values = [
          ...new BigUint64Array(mapped.getMappedRange(0, bytes).slice(0)),
        ];
        this.#replies.queryResults(
          sequence,
          command.set,
          command.firstQuery,
          values
        );
        this.#queued = true;
      })
      .catch((error) => {
        this.#failQueryResults(
          sequence,
          command,
          `${named} could not be mapped: ${String(error)}`
        );
      })
      .finally(() => {
        resolved.destroy();
        mapped.destroy();
        this.#inFlight -= 1;
      });
  }

  /**
   * Answers a query read that cannot be served: an empty value list, and the
   * reason on the error queue.
   *
   * Both destinations, for {@link Replayer#requestReadback}'s reason — the error
   * queue is what tells the engine its device is unwell, and the reply is what
   * stops the one caller waiting on *these values* from waiting for ever.
   *
   * @param {bigint} sequence
   * @param {{ set: { index: number, generation: number }, firstQuery: number }} command
   * @param {string} reason
   */
  #failQueryResults(sequence, command, reason) {
    this.#deviceError(reason);
    this.#replies.queryResults(sequence, command.set, command.firstQuery, []);
    this.#queued = true;
  }

  /**
   * Opens this flush's error scopes on the device, or answers `null`.
   *
   * WHAT THIS BUYS, AND WHAT IT DOES NOT. WebGPU reports its own validation and
   * out-of-memory failures **on the device rather than to the call**, so
   * `createBuffer` returns a plausible `GPUBuffer` and the refusal arrives
   * later with no currently-executing command to name — which is why
   * {@link Replayer#takeError}'s queue used to record every one of them as the
   * device's and unattributed. A scope around the flush changes what can be
   * said about them from nothing to a **range**: the failure came from one of
   * the commands between the first and last sequence this flush replayed.
   *
   * **IT IS STILL NOT SYNCHRONOUS WITH THE FAILING CALL, and per-command scopes
   * would not have been either.** `popErrorScope` hands back a promise, so the
   * attribution exists a round trip after the frame it belongs to whatever the
   * granularity. What per-command would buy over this is precision — one
   * sequence instead of a range — and
   * `web/tools/error-scope-bench.mjs` is what measured the price of that
   * precision.
   *
   * **A GPU-TIMELINE ERROR IS ATTRIBUTED TO THE FLUSH THAT ISSUED IT, NOT TO
   * WHICHEVER ONE HAPPENS TO BE OPEN WHEN IT SURFACES.** WebGPU routes an error
   * to the scope that was current when the operation was *issued*, so a
   * submission this flush made that fails after `replay` has returned still
   * lands in this flush's scope rather than in the next frame's.
   *
   * `null` — an unscoped flush — for the two states where there is nothing to
   * push onto: no device has opened yet, and a device that is already lost.
   * Both are exactly the states in which the commands are not reaching WebGPU
   * either, so there is no error for a scope to capture.
   *
   * @returns {GPUDevice | null} The device the scopes were pushed on.
   */
  #openErrorScopes() {
    const device = this.#device;
    if (device === null || this.#lost !== null) return null;
    // Unguarded, for the reason {@link Replayer#requestDevice} registers its
    // listener unguarded: `pushErrorScope` is on every `GPUDevice`, and a stub
    // that has none should fail loudly rather than quietly stop attributing.
    for (const filter of ERROR_SCOPE_FILTERS) device.pushErrorScope(filter);
    return device;
  }

  /**
   * Pops this flush's scopes and files whatever they caught, stamped with the
   * sequence range the flush covered.
   *
   * **THE CAPTURED ERRORS GO TO THE SAME LOG `uncapturederror` FEEDS, AND THAT
   * IS THE WHOLE POINT.** A scope is exclusive rather than additional: an error
   * it captures never reaches the listener {@link Replayer#requestDevice}
   * registered. So a `popErrorScope` whose result went anywhere but
   * {@link Replayer#deviceError} would not merely fail to attribute — it would
   * make the device stop reporting altogether, on a path where every gate stays
   * green because nothing counts errors that were never delivered.
   * `web/tools/gpu-replay.mjs` holds the arms that fail when that path is cut.
   *
   * **THE ATTRIBUTED LINE ARRIVES AFTER THE REPLAYER'S OWN REFUSALS FROM THE
   * SAME FLUSH.** Those are pushed inside the call; this one is pushed when the
   * promise settles, a round trip later. The log's order is still "what went
   * wrong first is what caused the rest" for everything the replayer decided
   * itself, and the device's own answer about a flush lands after it — which is
   * the order it landed in before this existed, through the listener.
   *
   * A REJECTION IS NOT SWALLOWED. `popErrorScope` rejects when the device is
   * lost, which {@link Replayer#loseDevice} has already recorded as the terminal
   * thing it is, so that case adds nothing; anything else is this replayer
   * having lost track of the scope stack and is filed as the fault it is.
   *
   * @param {GPUDevice} device What {@link Replayer#openErrorScopes} returned.
   * @param {bigint} baseSequence The sequence of the flush's first command.
   * @param {number} count How many commands it carried; at least one.
   */
  #closeErrorScopes(device, baseSequence, count) {
    const last = BigInt.asUintN(64, baseSequence + BigInt(count - 1));
    const during =
      last === baseSequence
        ? `during command ${baseSequence}`
        : `during commands ${baseSequence}–${last}`;
    for (let i = 0; i < ERROR_SCOPE_FILTERS.length; i += 1) {
      device.popErrorScope().then(
        (error) => {
          // Null is the ordinary answer: a scope that caught nothing. Only a
          // `GPUError` is worth a line.
          if (!error) return;
          this.#deviceError(
            `the device reported ${String(error.message ?? error)} ${during}`
          );
        },
        (reason) => {
          if (this.#lost !== null) return;
          this.#deviceError(
            `the device's error scope ${during} could not be popped: ${String(reason)}`
          );
        }
      );
    }
  }

  /**
   * Records an error the far side will take through `Device::take_error`.
   *
   * @param {string} message
   */
  #deviceError(message) {
    this.#errors.push(message);
  }

  /**
   * Records that `GPUDevice.lost` settled, and settles everything waiting on the
   * device with it.
   *
   * **A LOSS IS NOT AN ERROR ON A WORKING DEVICE**, which is why this does not
   * simply push a line and return. `lost` settles once and every object made on
   * that device is unusable from then on, so the loss is written to
   * {@link Replayer#lost} — the state {@link Replayer#replay} consults before
   * every command — and the queue gets it once, as the *first* thing a reader
   * sees, rather than once per call that then failed for a reason of its own.
   *
   * **A DELIBERATE DESTROY IS NOT A FAILURE AND IS NOT REPORTED AS ONE.**
   * `GPUDeviceLostReason` is exactly `'destroyed'` and `'unknown'`, and the
   * specification reaches `'destroyed'` down one path only — the page calling
   * `GPUDevice.destroy()`. Nothing in this engine calls it: teardown is
   * `demo.js`'s `pagehide` handler, which runs `shutdown()` and lets the page
   * go, and the specification says a device merely collected never settles
   * `lost` at all. So a `'destroyed'` loss is somebody tearing down on purpose,
   * and putting it on the error queue would hand `Engine::acquire` a reason to
   * report a failure on every ordinary page close. It is still terminal — a
   * destroyed device serves nothing either — so it is recorded, and the
   * commands and readbacks behind it are answered exactly as any other loss is.
   *
   * **THE READBACKS IN FLIGHT ARE FAILED HERE, WITH THE LOSS.** WebGPU rejects
   * an outstanding `mapAsync` with an `AbortError` when the device goes, and
   * that name says nothing about why; the reason the caller needs is this one.
   *
   * **AND SO ARE THE ONES A REJECTION HAS ALREADY CLAIMED, WHICH IS THE HALF
   * THAT CANNOT BE LEFT TO ORDERING.** The specification's "lose the device"
   * resolves `lost` before it completes "any outstanding steps that are waiting
   * until device becomes lost", so a rejection *caused by the loss* arrives
   * after this and {@link Replayer#requestReadback}'s handler leaves it alone.
   * `GPUDevice.destroy()` does not follow that route: it cancels an outstanding
   * map through the *buffer*, and Chromium was watched rejecting one with
   * "Buffer was unmapped before mapping was resolved" a whole task ahead of
   * `lost`. So `rejected` — set by that handler, and true only of a reason a map
   * rejection wrote — is re-filed here too, and the entry ends up saying the
   * same thing whichever of the two the browser settled first.
   *
   * A rejection this replaces was pushed to the error queue when it landed and
   * cannot be taken back, so on that one path the queue reads the browser's
   * sentence before this one. `docs/backlog.md` carries what closing that would
   * take.
   *
   * @param {{ reason?: string, message?: string }} info The `GPUDeviceLostInfo`
   *   the promise resolved with.
   */
  #loseDevice(info) {
    // `lost` settles once, so a second call is a stub or a browser repeating
    // itself; the first loss is the one that explains the rest either way.
    if (this.#lost !== null) return;
    const reason = String(info?.reason ?? 'unknown');
    const message = String(info?.message ?? '');
    this.#lost = {
      reason,
      message,
      text: `the device was lost: ${reason}: ${message}`,
    };
    if (reason !== 'destroyed') this.#deviceError(this.#lost.text);
    for (const [, entry] of this.#readbacks.entries()) {
      if (entry.state !== 'mapping' && !entry.rejected) continue;
      entry.state = 'failed';
      entry.reason = this.#lost.text;
      entry.rejected = false;
    }
  }

  /**
   * Answers one command against a device that is gone.
   *
   * **THE SAME SENTENCE EVERY TIME.** Every reply this queues carries
   * {@link Replayer#lost}'s `text`, so a caller draining a frame's answers reads
   * one cause rather than a different downstream symptom per call. The loss
   * itself reached the error queue once, in {@link Replayer#loseDevice}, and is
   * not pushed again here: identical repeats would fill
   * {@link MAX_PENDING_ERRORS} and then be counted as dropped, which reads as a
   * flood of errors rather than the single event it was.
   *
   * **ONLY THE COMMANDS THAT OWE A REPLY APPEAR BELOW.** A `CreateBuffer` or a
   * `Draw` answers nothing on a healthy device either — its failures go to the
   * error queue, which already holds the loss — so there is nothing for it to
   * say here. The ones named are the ones a sequence is registered against on
   * the far side, and leaving any of them unanswered would strand that sequence
   * for ever, which is the one outcome worse than a failure.
   *
   * A name this replayer does not know is dropped rather than refused, unlike
   * {@link Replayer#replay}'s `default`: the opcode check exists to stop a
   * stream being executed wrongly, and past a loss nothing is executed at all.
   *
   * @param {bigint} sequence
   * @param {{ name: string }} command
   */
  #answerLost(sequence, command) {
    const reason = this.#lost.text;
    switch (command.name) {
      case 'EnumerateAdapters':
        // The adapter may well still be there — but a replayer that granted one
        // would then be asked to open a device on it, and this one is holding a
        // corpse. `NoAdapter` is the honest answer with the loss as its reason.
        this.#noAdapter(sequence, reason);
        break;
      case 'RequestDevice':
        this.#replies.deviceFailed(
          sequence,
          reason.slice(0, MAX_REASON_CHARS),
          0n
        );
        this.#queued = true;
        break;
      case 'SurfaceCaps':
        this.#replies.surfaceCapsFailed(
          sequence,
          reason.slice(0, MAX_REASON_CHARS),
          SURFACE_CAPS_FAILURE.BACKEND
        );
        this.#queued = true;
        break;
      case 'RequestReadback':
        // Filed rather than ignored, for {@link Replayer#failReadback}'s
        // reason: a request that recorded nothing leaves the poll below looking
        // at an absent handle. The reason is not pushed to the error queue —
        // the loss that caused it is already there.
        this.#readbacks.insert(command.readback, {
          buffer: null,
          offset: 0,
          size: 0,
          state: 'failed',
          bytes: null,
          reason,
          abandoned: false,
        });
        break;
      case 'PollReadback':
        // The ordinary poll, because it already answers from the state the loss
        // left: `'failed'` with this reason for anything that was in flight or
        // has been asked for since. A readback that had already been copied out
        // still answers `Ready` with its bytes — they are in JS memory and a
        // dead device does not unmake them.
        this.#pollReadback(sequence, command);
        break;
      case 'QueryResults':
        // An empty value list, which is what a query read that cannot be served
        // answers — see {@link Replayer#failQueryResults}. The reason goes
        // nowhere new: this command's reason channel is the error queue, and the
        // loss is already the first thing in it.
        this.#replies.queryResults(
          sequence,
          command.set,
          command.firstQuery,
          []
        );
        this.#queued = true;
        break;
      case 'TakeError':
        // The one command that must still run: it is how the loss crosses to
        // wasm at all, and on a deliberate destroy it is how the far side hears
        // the queue is empty and nothing is wrong.
        this.#takeErrorCommand(sequence);
        break;
      default:
        break;
    }
  }

  /**
   * Answers a `TakeError`: the errors the device reported since wasm last asked.
   *
   * ANSWERED WITHIN THE CALL, and answered even when there is nothing to say —
   * an empty list. Silence is not an option: one command is answered exactly
   * once, and a sequence that is never answered stays registered on the far side
   * for ever. This command is asked again every time the engine's own queue runs
   * dry, so a replayer that stayed quiet on a healthy page would fill wasm's
   * waiting set and stop it asking anything at all.
   *
   * AT MOST `MAX_DEVICE_ERRORS`, AND THE REST ARE KEPT, NOT DROPPED. The cursor
   * only moves for the messages that go into this reply, so anything past the
   * cap is the next ask's — which is a frame later, and a frame later is what
   * this whole channel already is.
   *
   * The cursor is the `'wasm'` one, so nothing taken here is taken from
   * {@link Replayer#takeError} — see {@link DeviceErrorLog}.
   *
   * @param {bigint} sequence
   */
  #takeErrorCommand(sequence) {
    const messages = [];
    while (messages.length < MAX_DEVICE_ERRORS) {
      const message = this.#errors.take('wasm');
      if (message === null) break;
      messages.push(message);
    }
    this.#replies.deviceErrors(sequence, messages);
    this.#queued = true;
  }

  /**
   * Answers what a canvas surface on this instance will accept, or why it
   * cannot.
   *
   * ANSWERED WITHIN THE CALL, unlike the two commands that make a round trip:
   * nothing here is a promise, so deferring would only make the answer arrive a
   * frame later than it has to. The reply still names the sequence, and the
   * buffer still goes to wasm at the frame boundary.
   *
   * NOTHING IS LOOKED UP, BECAUSE THE COMMAND NAMES NOTHING. `surface_caps` is
   * per-surface and per-adapter on this seam because Vulkan's is: presentation
   * support is a property of a queue family. WebGPU has no such query — the
   * canvas formats come from `navigator.gpu`, whichever canvas and whichever
   * adapter — so the two ids the HAL call takes are validated by an
   * `impl Instance` against its own tables and never travel. This replayer's
   * surface table and adapter list are therefore not consulted here, which is a
   * decision rather than an omission.
   *
   * WHAT IS LEFT CAN STILL FAIL, AND DOES NOT THROW. `surfaceCapsFor` refuses a
   * canvas format this seam has no `Format` for, and the reply writer refuses a
   * field it will not encode; both are the `Err` half of a call whose `Err` half
   * is ordinary — see {@link SurfaceCapsError} — so both are replies. Every path
   * below queues exactly one, and a record that will not encode is rolled back
   * whole by the writer before the failure is written, so nothing half-written
   * can reach the buffer.
   *
   * @param {bigint} sequence
   */
  #surfaceCaps(sequence) {
    try {
      this.#replies.surfaceCaps(sequence, surfaceCapsFor(this.#gpu));
    } catch (error) {
      this.#replies.surfaceCapsFailed(
        sequence,
        String(error).slice(0, MAX_REASON_CHARS),
        // Anything this file did not anticipate — a browser with no
        // `navigator.gpu`, a reply the writer refused — is `Backend` too: the
        // cause that promises nothing, which is the only honest thing to say
        // about a failure nobody wrote a branch for.
        error instanceof SurfaceCapsError
          ? error.code
          : SURFACE_CAPS_FAILURE.BACKEND
      );
    }
    this.#queued = true;
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
