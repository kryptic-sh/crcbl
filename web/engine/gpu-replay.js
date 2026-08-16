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
// {@link Replayer#takeError} — and everything that can go wrong with a buffer
// goes into it: the refusals this file makes before asking the browser, a
// `createBuffer` that throws, and the errors the device reports asynchronously
// through `uncapturederror`. Not a throw, and not a reply; that class argues
// why.
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
// written here because that is where a reader meets it.

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
 */
const TEXTURE_FORMAT = Object.freeze({
  R8_UNORM: { name: 'r8unorm' },
  RG8_UNORM: { name: 'rg8unorm' },
  RGBA8_UNORM: { name: 'rgba8unorm' },
  RGBA8_UNORM_SRGB: { name: 'rgba8unorm-srgb' },
  BGRA8_UNORM: { name: 'bgra8unorm' },
  BGRA8_UNORM_SRGB: { name: 'bgra8unorm-srgb' },
  RGB10A2_UNORM: { name: 'rgb10a2unorm' },
  R11G11B10_FLOAT: { name: 'rg11b10ufloat' },
  R16_FLOAT: { name: 'r16float' },
  RG16_FLOAT: { name: 'rg16float' },
  RGBA16_FLOAT: { name: 'rgba16float' },
  R32_FLOAT: { name: 'r32float' },
  RG32_FLOAT: { name: 'rg32float' },
  RGBA32_FLOAT: { name: 'rgba32float' },
  R32_UINT: { name: 'r32uint' },
  RG32_UINT: { name: 'rg32uint' },
  D32_FLOAT: { name: 'depth32float' },
  D32_FLOAT_S8_UINT: {
    name: 'depth32float-stencil8',
    feature: 'depth32float-stencil8',
  },
  D24_UNORM_S8_UINT: { name: 'depth24plus-stencil8' },
  D16_UNORM: { name: 'depth16unorm' },
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
 *   * `StorageImage` → **nothing, and this is the one WebGPU cannot express.**
 *     `GPUStorageTextureBindingLayout.format` is a *required* member with no
 *     default, and `crcbl_hal::BindingKind::StorageImage` carries no format at
 *     all: it has `read_only` and nothing else, because Vulkan, Metal and D3D12
 *     take the format off the bound view. There is no value this file could
 *     supply that would not be a guess, and a guessed storage-texture format is
 *     a shader writing the wrong number of channels with nothing reporting it.
 *     So it is refused, and the refusal says which member is missing.
 *
 * `externalTexture` is the fifth WebGPU member and nothing on this seam maps to
 * it: a `GPUExternalTexture` is a video frame, which `crcbl-hal` has no concept
 * of.
 *
 * @param {{ name: string, dynamic?: boolean, readOnly?: boolean,
 *           viewType?: string, sampleType?: string, comparison?: boolean }} kind
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
    case 'StorageImage':
      return {
        layout: null,
        reason:
          'is a BindingKind::StorageImage, which WebGPU cannot express from this ' +
          'seam: GPUStorageTextureBindingLayout.format is required and has no ' +
          'default, and BindingKind::StorageImage carries no format for it',
      };
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
 */
const CANVAS_FORMAT = Object.freeze({
  bgra8unorm: FORMAT.BGRA8_UNORM,
  rgba8unorm: FORMAT.RGBA8_UNORM,
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
 *   * `formats` — `getPreferredCanvasFormat()` answers one format, and it is the
 *     one to put first: `SurfaceCaps::formats` is documented best-first and
 *     `preferred_format()` reads it that way, and the browser's preference is
 *     exactly the "no extra copy on present" claim that ordering means. The
 *     other member of {@link CANVAS_FORMAT} follows it, because a canvas can be
 *     configured with either. None of them is sRGB, so `preferred_format()`
 *     falls through to the first entry — which is the browser's own preference,
 *     and the right answer.
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
  const preferredCode = CANVAS_FORMAT[preferred];
  if (preferredCode === undefined) {
    throw new SurfaceCapsError(
      SURFACE_CAPS_FAILURE.BACKEND,
      `getPreferredCanvasFormat() answered ${JSON.stringify(preferred)}, ` +
        'which is not a canvas format this seam has a Format for'
    );
  }
  return {
    formats: [
      preferredCode,
      ...Object.values(CANVAS_FORMAT).filter((code) => code !== preferredCode),
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
 * Nothing drains it yet — the `take_error` command is a later slice — and
 * `uncapturederror` can fire once a frame for as long as a page is open, so an
 * unbounded queue is a leak on a page that is doing badly. The first errors are
 * the ones worth keeping: what went wrong first is what caused the rest.
 * Nothing past the cap is *lost* silently, though; see
 * {@link Replayer#takeError}.
 */
const MAX_PENDING_ERRORS = 64;

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
   * Errors the device reported out of band, oldest first.
   *
   * `Device::take_error`'s queue, on this side of the seam — see
   * {@link Replayer#takeError}.
   *
   * @type {string[]}
   */
  #errors = [];
  /** How many errors were refused for want of room in {@link #errors}. */
  #errorsDropped = 0;

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
   * The buffers that are live right now, on {@link Replayer#surfaces}'s terms.
   *
   * @type {HandleTable<GPUBuffer>}
   */
  get buffers() {
    return this.#buffers;
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

  /** How many out-of-band errors are waiting to be taken. */
  get pendingErrors() {
    return this.#errors.length;
  }

  /**
   * The oldest error the device reported out of band, or `null`.
   *
   * `crcbl_hal::Device::take_error` seen from this side, and named for it:
   * each error is reported once — taking it clears it — and
   * `docs/plan/41-webgpu-stream.md` has `Gpu::acquire` draining this at the top
   * of every frame once there is a command to carry it. There is no such
   * command yet, so today's readers are `web/tools/gpu-replay.mjs` and the
   * browser gate.
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
   *   * A reply would name a sequence nothing is waiting on. Identity here is
   *     positional — wasm allocated the handle and moved on — so no wait is
   *     registered, and `crcbl-webgpu`'s reader turns a reply for an unawaited
   *     sequence into a `DecodeError::UnexpectedSequence` that refuses the
   *     *whole frame's* replies, stranding every other answer in it.
   *
   * The last of these to come out, once the queue has been emptied, is a
   * synthesised line naming how many were refused for want of room — so a page
   * that produced more than {@link MAX_PENDING_ERRORS} learns that it did
   * rather than being told the first few were all there was.
   *
   * @returns {string | null}
   */
  takeError() {
    const error = this.#errors.shift();
    if (error !== undefined) return error;
    if (this.#errorsDropped === 0) return null;
    const dropped = this.#errorsDropped;
    this.#errorsDropped = 0;
    return `and ${dropped} further device error(s) were dropped: this replayer holds ${MAX_PENDING_ERRORS} and nothing has been draining them`;
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
        case 'SurfaceCaps':
          this.#surfaceCaps(sequence);
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
   * `GPUDevice.lost` is deliberately still not watched. It is a promise that
   * settles once and means the device is gone rather than that a call failed,
   * so what it wants is the seam's device-lost path, and there is none on this
   * channel yet; adding it to this queue would report a dead device as one more
   * error to log and carry on from.
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
        // Before the device is held, so that a device this replayer cannot
        // listen to is a failed request rather than a live device with no error
        // channel behind it.
        device.addEventListener('uncapturederror', (event) => {
          // `event.error` is a `GPUError`, whose `message` is the whole of what
          // the browser has to say. Named as the device's own rather than
          // attributed to a command: these arrive after the frame that caused
          // them, in submission order rather than by sequence, so a number here
          // would be a guess dressed as attribution.
          this.#deviceError(
            `the device reported ${String(event.error?.message ?? event.error)}`
          );
        });
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
   * Records an error the far side will take through `Device::take_error`.
   *
   * @param {string} message
   */
  #deviceError(message) {
    if (this.#errors.length >= MAX_PENDING_ERRORS) {
      this.#errorsDropped += 1;
      return;
    }
    this.#errors.push(message);
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
