// The JavaScript half of `crcbl-webgpu`'s command stream: a buffer in, plain
// command objects out.
//
// `crates/crcbl-webgpu/src/tag.rs` is the contract — the magic, the version,
// the caps, the opcode table and every enum code — and `src/reader.rs` is the
// reference decoder this file mirrors, error taxonomy included. Neither half
// can see the other, and no compiler anywhere reads both, so the pair is held
// together from outside: a Rust test freezes the canonical stream into
// `crates/crcbl-webgpu/tests/fixtures/canonical-stream.bin`, and
// `web/tools/stream-decode.mjs` decodes that same file here and asserts every
// field. A number changed on one side and not the other turns one of those two
// red.
//
// STRICT, FOR THE REASON THE RUST DECODER IS. Every read is bounds-checked
// against the buffer, every enum code is one the encoder wrote, every unclaimed
// bitflag bit is an error rather than a truncation, and a presence byte that is
// neither canonical value is refused rather than read as truthy. A replayer
// that guessed instead would turn a corrupt stream into WebGPU calls.
//
// THE DETACHED-VIEW RULE STILL APPLIES. In the browser the bytes are wasm's, so
// the caller builds the view immediately before the call from the pointer it
// was just handed, and nothing here outlives the call: no view is stored, and
// `PushConstants.data` is copied out rather than returned as a window onto the
// heap that the next allocation would detach. See `web/engine/wasm.js`.
//
// SIXTY-FOUR BITS, AND WHERE THEY WOULD OTHERWISE BE LOST. A JS number is an
// exact integer only to 2^53, and this format carries fields above that, so
// nothing 64-bit here is read as one:
//
//   * A handle decodes to `{ index, generation }`, two `u32`s, each exact as a
//     number — and that is `crcbl_core::Handle`'s own shape rather than a
//     repacking of it. `Handle::to_bits` puts the **generation in the high
//     half** and the index in the low half; read as a single number the packed
//     form is already wrong for any generation above 2^21, and wrong silently.
//     Splitting also means the two halves cannot be swapped by accident here,
//     because each is named.
//   * `CreateBuffer.size` and a stream's sequence numbers are `BigInt`. Size
//     carries `WHOLE_BUFFER` — `u64::MAX` — through verbatim, and a number
//     would round it to 18446744073709552000: a different size that still looks
//     enormous, so nothing downstream would question it. Sequence numbers are
//     64-bit precisely because they are never allowed to wrap within a session.
//   * `RequestDevice`'s two feature words are `BigInt` too. `crcbl_hal::Features`
//     is a 64-bit bitflags and the flags it has today all sit in the low
//     twenty-seven bits, so a number would be exact *for now* — which is the
//     worst of the three possibilities, since the day a flag is added past bit
//     53 nothing here would fail, and a required feature would quietly go
//     missing.
//
// Everything else on the wire is 32 bits or narrower and stays a number.

import { COMPOSITE_ALPHA, FORMAT, PRESENT_MODE } from './gpu-reply.js';

// ── Header ───────────────────────────────────────────────────────────────────

/** `tag::STREAM_MAGIC` — the ASCII bytes every stream buffer starts with. */
const STREAM_MAGIC = new Uint8Array([
  0x43, 0x52, 0x43, 0x42, 0x4c, 0x47, 0x50, 0x55,
]); // "CRCBLGPU"

/** `tag::STREAM_VERSION`. */
const STREAM_VERSION = 1;

// ── Caps ─────────────────────────────────────────────────────────────────────

/** `tag::MAX_FIELD_BYTES` — largest single length-prefixed byte field. */
const MAX_FIELD_BYTES = 1 << 20;

/** `tag::MAX_ELEMENT_COUNT` — largest element count in a length-prefixed array. */
const MAX_ELEMENT_COUNT = 1 << 16;

// ── Command tags ─────────────────────────────────────────────────────────────

const CREATE_BUFFER_TAG = 0x00;
const CREATE_SURFACE_TAG = 0x01;
const CREATE_IMAGE_TAG = 0x02;
const CREATE_IMAGE_VIEW_TAG = 0x03;
const CREATE_SAMPLER_TAG = 0x04;
const CREATE_BIND_GROUP_LAYOUT_TAG = 0x05;
const CREATE_BIND_GROUP_TAG = 0x06;
const CREATE_SHADER_MODULE_TAG = 0x07;
const CREATE_PIPELINE_LAYOUT_TAG = 0x08;
const CREATE_COMPUTE_PIPELINE_TAG = 0x09;
const CREATE_GRAPHICS_PIPELINE_TAG = 0x0a;
// `request_readback` allocates a `ReadbackHandle` like every `create_*`, so it
// sits in the creation family rather than the device one — see `crcbl-webgpu`'s
// `tag` module.
const REQUEST_READBACK_TAG = 0x0b;
// The offscreen twin of `CREATE_SURFACE_TAG`, in the creation family beside it —
// see `crcbl-webgpu`'s `tag` module. It carries only the surface handle: an
// offscreen target names no canvas key, and its ring's size and format arrive
// later with the swapchain, not the surface.
const CREATE_OFFSCREEN_SURFACE_TAG = 0x0c;
// `create_query_set` allocates a `QuerySetHandle` from a descriptor, so it sits
// in the creation family for `REQUEST_READBACK_TAG`'s reason rather than in the
// query family — which holds the query *verbs* only. See `crcbl-webgpu`'s `tag`
// module.
const CREATE_QUERY_SET_TAG = 0x0d;
const DESTROY_BUFFER_TAG = 0x20;
const DESTROY_SURFACE_TAG = 0x21;
const DESTROY_IMAGE_TAG = 0x22;
const DESTROY_IMAGE_VIEW_TAG = 0x23;
const DESTROY_SAMPLER_TAG = 0x24;
const DESTROY_BIND_GROUP_LAYOUT_TAG = 0x25;
const DESTROY_BIND_GROUP_TAG = 0x26;
const DESTROY_SHADER_MODULE_TAG = 0x27;
const DESTROY_PIPELINE_LAYOUT_TAG = 0x28;
const DESTROY_COMPUTE_PIPELINE_TAG = 0x29;
const DESTROY_GRAPHICS_PIPELINE_TAG = 0x2a;
const DESTROY_COMMAND_BUFFER_TAG = 0x2b;
const DESTROY_READBACK_TAG = 0x2c;
const DESTROY_QUERY_SET_TAG = 0x2d;
const BEGIN_DEBUG_LABEL_TAG = 0x40;
const BEGIN_RENDER_PASS_TAG = 0x41;
const BIND_GRAPHICS_PIPELINE_TAG = 0x42;
const BIND_GROUP_TAG = 0x43;
const PUSH_CONSTANTS_TAG = 0x44;
// The encoder's lifecycle: opening the implicit-current encoder, closing a
// pass, and sealing the recorded buffer. `CreateCommandEncoder` names no handle
// and `Finish` names the caller-allocated command buffer — see `crcbl-webgpu`'s
// `Command` docs.
const CREATE_COMMAND_ENCODER_TAG = 0x45;
const END_RENDER_PASS_TAG = 0x46;
const FINISH_TAG = 0x47;
// The compute-pass encoder state: opening a compute pass (label only — compute
// has no attachments), binding a compute pipeline, and closing the pass.
const BEGIN_COMPUTE_PASS_TAG = 0x48;
const END_COMPUTE_PASS_TAG = 0x49;
const BIND_COMPUTE_PIPELINE_TAG = 0x4a;
// The documented no-op: it carries the barrier lists for wire fidelity, but the
// replayer records nothing because WebGPU tracks resource state itself.
const PIPELINE_BARRIER_TAG = 0x4b;
// The dynamic viewport and scissor the render graph sets on every pass, recorded
// on the open render pass as `setViewport` / `setScissorRect`.
const SET_VIEWPORT_TAG = 0x4c;
const SET_SCISSOR_TAG = 0x4d;
// The index buffer for the UI pass's indexed draw, recorded on the open render
// pass as `setIndexBuffer`.
const BIND_INDEX_BUFFER_TAG = 0x4e;
// The other two debug ops beside `BeginDebugLabel`: closing the region it opened
// (`popDebugGroup`, body-less) and a point-in-time marker (`insertDebugMarker`).
// The marker has its own tag because it opens no region — folding it onto the
// region's tag would leave an unbalanced group behind every marker.
const END_DEBUG_LABEL_TAG = 0x4f;
const INSERT_DEBUG_MARKER_TAG = 0x50;
// The dynamic stencil reference a pass sets before its masked draws, recorded on
// the open render pass as `setStencilReference`. Past the debug markers rather
// than beside the scissor because a tag byte is a wire value: the encoder family
// grows at its end so the committed fixture keeps meaning what it meant.
const SET_STENCIL_REFERENCE_TAG = 0x51;
const DRAW_TAG = 0x60;
// The indexed draw the UI pass records, on the open render pass as `drawIndexed`.
const DRAW_INDEXED_TAG = 0x61;
// The indirect draws the geometry path records, on the open render pass. WebGPU's
// `drawIndirect`/`drawIndexedIndirect` are single-draw, so the replayer unrolls
// `draw_count` into that many calls at `offset + i * stride`.
const DRAW_INDIRECT_TAG = 0x62;
const DRAW_INDEXED_INDIRECT_TAG = 0x63;
// The dispatch family: the workgroup counts for a compute dispatch, inline or
// read out of a buffer. The indirect form is NOT unrolled — WebGPU's
// `dispatchWorkgroupsIndirect` is a single dispatch, so it carries no count and
// no stride.
const DISPATCH_TAG = 0x70;
const DISPATCH_INDIRECT_TAG = 0x71;
const COPY_IMAGE_TO_BUFFER_TAG = 0x78;
// The buffer→buffer copy that carries a dispatch's storage-buffer output to a
// host-readable buffer — the only way a dispatch is observed.
const COPY_BUFFER_TO_BUFFER_TAG = 0x79;
// The buffer→image upload copy and the image→image copy, plus a buffer fill
// WebGPU can only perform for the value zero (`clearBuffer`).
const COPY_BUFFER_TO_IMAGE_TAG = 0x7a;
const COPY_IMAGE_TO_IMAGE_TAG = 0x7b;
const FILL_BUFFER_TAG = 0x7c;
// The host→buffer upload — `queue.writeBuffer` on the replayer, not an encoder
// op. In the copy-and-fill family because it is a queue-side data transfer, the
// upload counterpart of the copies.
const WRITE_BUFFER_TAG = 0x7d;
// The query family: the verbs, not the set. `ResetQuerySet` is the documented
// no-op — WebGPU has no reset, and an unwritten query resolves to zero by
// specification — carried so a range naming a set the replayer does not hold is
// a message rather than a silence. `ResolveQuerySet` is the encoder's
// `resolveQuerySet`. `QueryResults` is the only one of the three that is
// *answered*, by a `QueryResults` reply naming its sequence.
const RESET_QUERY_SET_TAG = 0x80;
const RESOLVE_QUERY_SET_TAG = 0x81;
const QUERY_RESULTS_TAG = 0x82;
// The presentation family: configuring a canvas swapchain, acquiring its frame,
// presenting (a no-op the browser composites on rAF), unconfiguring, and
// reconfiguring an already-configured swapchain in place.
const CREATE_SWAPCHAIN_TAG = 0x88;
const ACQUIRE_NEXT_FRAME_TAG = 0x89;
const PRESENT_TAG = 0x8a;
const DESTROY_SWAPCHAIN_TAG = 0x8b;
const RECONFIGURE_SWAPCHAIN_TAG = 0x8c;
const ENUMERATE_ADAPTERS_TAG = 0x90;
const REQUEST_DEVICE_TAG = 0x91;
const SURFACE_CAPS_TAG = 0x92;
// The device family: submission, the readback poll and the out-of-band error
// ask — the `Device` methods that make no object and release none.
const SUBMIT_TAG = 0xa0;
const POLL_READBACK_TAG = 0xa1;
const TAKE_ERROR_TAG = 0xa2;

// ── Optional fields ──────────────────────────────────────────────────────────

const ABSENT = 0;
const PRESENT = 1;

// ── Enum code tables ─────────────────────────────────────────────────────────
//
// Indexed by wire code, so a gap is `undefined` and therefore an error. The
// names are the HAL's variant names, which is what makes a decoded stream
// readable without this file open beside it.

/** `tag::LOAD_OP_*`. */
const LOAD_OP = ['Load', 'Clear', 'DontCare'];

/**
 * `tag::RESOURCE_STATE_*`.
 *
 * A barrier's `from`/`to` states. The whole vocabulary crosses even though a
 * `PipelineBarrier` is a no-op on WebGPU — the replayer records nothing, so a
 * state folded into a neighbour here would be a wire infidelity nothing
 * downstream could catch, and the `undefined` this table answers for an
 * unclaimed code is the only place it surfaces.
 */
const RESOURCE_STATE = [
  'Undefined',
  'ShaderRead',
  'ShaderWrite',
  'ShaderReadWrite',
  'ColorAttachment',
  'DepthStencilWrite',
  'DepthStencilRead',
  'TransferSrc',
  'TransferDst',
  'IndirectArgument',
  'IndexBuffer',
  'HostRead',
  'Present',
];

/** `tag::STORE_OP_*`. */
const STORE_OP = ['Store', 'Discard'];

/**
 * `tag::INDEX_FORMAT_*`.
 *
 * Carried by a `BindIndexBuffer`. The two widths cross as their own code — a
 * fold between them is an index buffer read at the wrong stride, not a refusal.
 */
const INDEX_FORMAT = ['Uint16', 'Uint32'];

/** `tag::MEMORY_*`. */
const MEMORY_LOCATION = ['DeviceLocal', 'HostUpload', 'HostReadback'];

/**
 * `tag::QUERY_KIND_*`.
 *
 * All three cross although `GPUQueryType` is exactly `'occlusion'` and
 * `'timestamp'`: the seam refuses the other two at `create_query_set`, and the
 * wire carries what the caller wrote so that a fold between the codes decodes to
 * a different command rather than creating the wrong pool. `gpu-replay.js` is
 * where each refusal is named.
 */
const QUERY_KIND = ['Timestamp', 'Occlusion', 'PipelineStatistics'];

/**
 * `tag::IMAGE_TYPE_*`.
 *
 * A GAP HERE IS THE COSTLIEST GAP IN THIS FILE. `Extent3d.depthOrLayers` is the
 * depth for `D3` and the array-layer count for everything else, so the same
 * three numbers describe two different images depending only on this byte —
 * a 64-deep volume against 64 flat slices, with mip chains of seven levels and
 * three. Nothing downstream can tell them apart, because the bytes are
 * identical; the `undefined` this table answers with for an unclaimed code is
 * the only place the difference is visible.
 */
const IMAGE_TYPE = ['D1', 'D2', 'D3'];

/**
 * `tag::IMAGE_VIEW_TYPE_*`.
 *
 * A separate table from {@link IMAGE_TYPE} and not a superset of it, because
 * the HAL keeps them separate: a view reinterprets its image's dimensionality,
 * so the two fields of a create pair legitimately disagree.
 *
 * `D2`/`D2Array` and `Cube`/`CubeArray` are adjacent codes, and each member of
 * a pair accepts exactly the `baseLayer` and `layerCount` the other does — so a
 * code read one along builds a view rather than refusing one, and a cube map's
 * six faces become six unrelated slices.
 */
const IMAGE_VIEW_TYPE = ['D1', 'D2', 'D2Array', 'Cube', 'CubeArray', 'D3'];

/**
 * `tag::FILTER_MODE_*`.
 *
 * Two rows, and read **three times per sampler** — `magFilter`, `minFilter` and
 * `mipFilter` are three of these bytes back to back. That is what makes so small
 * a table worth writing out rather than reading as a boolean: with two variants
 * there is no unclaimed code to land on, so a byte read out of position decodes
 * to a filter rather than to an error, and a sampler that filters correctly when
 * magnifying and not when minifying looks like a texture that is merely soft.
 */
const FILTER_MODE = ['Nearest', 'Linear'];

/**
 * `tag::SAMPLER_ADDRESS_*`.
 *
 * {@link FILTER_MODE}'s hazard with four rows: `addressMode` is three of these
 * bytes in a row, for U, V and W.
 *
 * `ClampToEdge` and `ClampToBorder` are the pair a gap would cost the most,
 * because they agree everywhere but the edge texel — one repeats it outwards and
 * the other fetches transparent black, so the wrong one of the two bleeds an
 * atlas's neighbour into every seam with nothing anywhere reporting an error.
 * They also part company at the backend: WebGPU has no border colour at all, so
 * `gpu-replay.js` refuses one of them and not the other.
 */
const SAMPLER_ADDRESS_MODE = [
  'Repeat',
  'MirrorRepeat',
  'ClampToEdge',
  'ClampToBorder',
];

/**
 * `tag::COMPARE_OP_*` — what a hardware-PCF sampler compares with.
 *
 * `Greater` and `Less` are the row pair that must never fold. `crcbl_hal`'s
 * `CompareOp` names the comparison performed rather than what it means for
 * visibility, and under this engine's reversed-Z it is `Greater` that asks "is
 * the fragment closer than the stored caster?" — so a shadow sampler that got
 * `Less` instead lights exactly the surfaces that should be in shadow and
 * shadows the rest, every frame, with no error anywhere. A `GPUSampler` reports
 * nothing about the comparison it was built with either, so this table answering
 * `undefined` for a code it does not claim is the only place the difference is
 * visible.
 */
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

/**
 * `tag::PRIMITIVE_TOPOLOGY_*` — how vertices assemble into primitives.
 *
 * A gap costs a *strip* read as a *list* or the reverse: `LineStrip` folded into
 * `LineList` connects segments meant to be independent, and the primitive
 * assembles either way with nothing downstream refusing it. Read into
 * `GPUPrimitiveState.topology` by `gpu-replay.js`.
 */
const PRIMITIVE_TOPOLOGY = [
  'PointList',
  'LineList',
  'LineStrip',
  'TriangleList',
  'TriangleStrip',
];

/**
 * `tag::FRONT_FACE_*` — which winding is front-facing.
 *
 * Two rows, and folded with {@link CULL_MODE} it decides which triangles
 * survive, so a winding read as its opposite culls exactly the faces that should
 * have been kept — a mesh inside-out, with the draw succeeding.
 */
const FRONT_FACE = ['Ccw', 'Cw'];

/**
 * `tag::CULL_MODE_*` — which faces to discard.
 *
 * `Front` and `Back` are the pair a gap costs the most: they discard the same
 * amount and disagree only on which half, so a fold shows the far side of every
 * object and hides the near one — geometry turned inside out rather than an
 * error.
 */
const CULL_MODE = ['None', 'Front', 'Back'];

/**
 * `tag::POLYGON_MODE_*` — fill or wireframe.
 *
 * `Line` is the one WebGPU cannot express — wireframe is `POLYGON_MODE_LINE`,
 * native-only — so it crosses verbatim and `gpu-replay.js` refuses it by name.
 * A gap folded into `Fill` would silently fill a wireframe pass the caller meant
 * to see through, the opposite of that loud refusal.
 */
const POLYGON_MODE = ['Fill', 'Line'];

/**
 * `tag::STENCIL_OP_*` — a stencil operation on a test outcome.
 *
 * Read **six times per pipeline** — two `StencilFaceState`s, three ops each — so
 * an op out of position lands on a real op rather than an error. The clamp/wrap
 * pairs (`IncrementClamp`/`IncrementWrap`) agree until the value saturates and
 * then silently disagree, which no draw reports.
 */
const STENCIL_OP = [
  'Keep',
  'Zero',
  'Replace',
  'Invert',
  'IncrementClamp',
  'DecrementClamp',
  'IncrementWrap',
  'DecrementWrap',
];

/**
 * `tag::BLEND_FACTOR_*` — a blend factor.
 *
 * Read four times per colour target with two {@link BLEND_OP}s interleaved, so a
 * factor out of position decodes to another factor. Each factor and its
 * `OneMinus` complement composite in exactly opposite directions, so a fold
 * inverts the blend and leaves a valid pipeline the browser accepts.
 */
const BLEND_FACTOR = [
  'Zero',
  'One',
  'Src',
  'OneMinusSrc',
  'SrcAlpha',
  'OneMinusSrcAlpha',
  'Dst',
  'OneMinusDst',
  'DstAlpha',
  'OneMinusDstAlpha',
];

/**
 * `tag::BLEND_OP_*` — how blended terms combine.
 *
 * `Subtract` and `ReverseSubtract` swap which operand is subtracted from which,
 * so a target reads its own colour where it meant the destination's, and the
 * pipeline is valid either way. `Min`/`Max` ignore their factors while the
 * arithmetic ops do not, so a fold across the two groups also changes whether
 * the factors above matter at all.
 */
const BLEND_OP = ['Add', 'Subtract', 'ReverseSubtract', 'Min', 'Max'];

/**
 * `tag::SAMPLE_TYPE_*` — what a sampled image's texels mean to the shader.
 *
 * Two rows and the loudest failure of any table this size. `crcbl_hal` carries
 * this field on the *layout* rather than reading it off the bound view because
 * **WebGPU does**: `GPUTextureBindingLayout.sampleType` is a member of a bind
 * group layout entry, and a depth-format view is only bindable through a slot
 * that says `'depth'`. So the two rows are not two spellings of a preference —
 * they are two different layouts, and a code folded into its neighbour produces
 * one the browser then refuses every bind group against, naming the group.
 */
const SAMPLE_TYPE = ['Float', 'Depth'];

/**
 * `tag::BINDING_KIND_*`, and the one table here whose rows have **bodies**.
 *
 * Every other enum on this stream is a byte that names a value; this one is a
 * byte that names a *shape*, because `crcbl_hal::BindingKind`'s variants carry
 * data and the payloads are different lengths — one presence byte, two presence
 * bytes, or two enum codes. That is what makes a fold here worse than anywhere
 * else in this file: a code read as its neighbour does not merely mis-name the
 * binding, it consumes the wrong number of bytes, and every field after it in
 * the entry — and every entry after that — decodes out of the wrong offsets and
 * still looks like a layout.
 *
 * The rows are read by {@link ByteReader#readBindingKind}, which is where the
 * bodies live; this table exists so the code and the name meet in one place and
 * so an unclaimed code is `undefined` rather than a row one along.
 */
const BINDING_KIND = [
  'UniformBuffer',
  'StorageBuffer',
  'SampledImage',
  'StorageImage',
  'Sampler',
];

/**
 * `tag::BINDING_RESOURCE_*`, and the second table here whose rows have
 * **bodies** — `crcbl_hal::BindingResource`'s variants carry data.
 *
 * **A fold here is the most dangerous confusion on the seam.** A
 * `crcbl_core::Handle` carries no kind, so a buffer, a view and a sampler may
 * hold identical bits, and this byte is the *only* thing that says which of the
 * replayer's three resource tables a handle indexes: a `Sampler` read as an
 * `ImageView` binds a sampler where a texture belongs, which the browser refuses
 * naming the bind group rather than the entry. The bodies are also different
 * lengths — `Buffer` carries a handle and two `BigInt`s, the other two a bare
 * handle — so a code read as its neighbour consumes the wrong number of bytes and
 * lands the cursor inside the next entry.
 *
 * The rows are read by {@link ByteReader#readBindingResource}; this table exists
 * so the code and the name meet in one place and an unclaimed code is `undefined`
 * rather than a row one along.
 */
const BINDING_RESOURCE = ['Buffer', 'ImageView', 'Sampler'];

/**
 * `tag::FORMAT_*`, code-indexed — the inverse of the table `gpu-reply.js`
 * exports, and not a second copy of it.
 *
 * The reply direction already carries formats and already keeps this mapping;
 * a table written out again here would be one more thing to keep in step with
 * `crates/crcbl-webgpu/src/tag.rs` and would drift from its twin rather than
 * from the Rust, which is worse — the fixture that pins one would not pin the
 * other. Inverting it costs a loop at module load and leaves a single place
 * where a code and a format meet.
 *
 * The names are therefore that table's spelling (`BGRA8_UNORM`) rather than the
 * HAL's (`Bgra8Unorm`), which is what the other tables here use. The reverse
 * lookup a replayer needs is the imported object itself.
 */
const IMAGE_FORMAT = [];
for (const [name, code] of Object.entries(FORMAT)) IMAGE_FORMAT[code] = name;

/**
 * `tag::PRESENT_MODE_*`, code-indexed — the inverse of the {@link PRESENT_MODE}
 * table `gpu-reply.js` exports, for {@link IMAGE_FORMAT}'s reason: one place
 * where a code and a mode meet, so the two directions cannot drift.
 *
 * A gap answers `undefined` rather than a neighbour: `Fifo` is the mode every
 * surface is promised, so a drifted table folding an unknown code onto it would
 * be indistinguishable from a surface that genuinely offers only that.
 */
const SWAPCHAIN_PRESENT_MODE = [];
for (const [name, code] of Object.entries(PRESENT_MODE))
  SWAPCHAIN_PRESENT_MODE[code] = name;

/**
 * `tag::COMPOSITE_ALPHA_*`, code-indexed — the inverse of the
 * {@link COMPOSITE_ALPHA} table `gpu-reply.js` exports, for {@link IMAGE_FORMAT}'s
 * reason.
 *
 * The two multiplied modes are adjacent codes and mean opposite things about the
 * colour channels, so folding an unknown code into either is a surface that
 * composites wrongly and never says why.
 */
const SWAPCHAIN_COMPOSITE_ALPHA = [];
for (const [name, code] of Object.entries(COMPOSITE_ALPHA))
  SWAPCHAIN_COMPOSITE_ALPHA[code] = name;

// ── Bitflag tables ───────────────────────────────────────────────────────────
//
// In ascending bit order, which is the order a decoded flag list comes back in.
// Each bit is an explicit `1 << n` in `crcbl-hal`, so these are chosen wire
// values rather than declaration positions — the one place the encoding is
// allowed to mirror the Rust type directly.

/** `crcbl_hal::BufferUsage`. */
const BUFFER_USAGE = [
  'TRANSFER_SRC',
  'TRANSFER_DST',
  'UNIFORM',
  'STORAGE',
  'INDEX',
  'INDIRECT',
  'DEVICE_ADDRESS',
  'QUERY_RESOLVE',
];

/**
 * `crcbl_hal::ShaderStages`.
 *
 * **Five bits, and the last two are the ones a browser cannot serve.** `MESH`
 * (1 << 3) and `TASK` (1 << 4) are real stages on this seam and WebGPU has no
 * `GPUShaderStage` bit for either, so they are refused by `gpu-replay.js` rather
 * than dropped here — a bitflags word goes over as `bits()` and comes back
 * through the equivalent of `from_bits`, and a stage silently missing from a
 * binding's visibility is a layout narrower than the one that was asked for.
 * The committed fixture carries a layout entry visible to each, which is what
 * holds this list at five rows rather than four.
 */
const SHADER_STAGES = ['VERTEX', 'FRAGMENT', 'COMPUTE', 'MESH', 'TASK'];

/**
 * `crcbl_hal::BindingFlags` — descriptor-indexing behaviour for one binding.
 *
 * All three require `Features::DESCRIPTOR_INDEXING`, which no WebGPU device ever
 * reports: WebGPU has no bindless model at all. They are decoded rather than
 * waved through for the reason that type's own docs give — "a backend without it
 * must reject a layout that sets any of them rather than silently ignoring it: a
 * bindless array quietly downgraded to a fixed one reads garbage at index 4097"
 * — and the rejecting is `gpu-replay.js`'s, which can only refuse what it was
 * told.
 */
const BINDING_FLAGS = [
  'PARTIALLY_BOUND',
  'UPDATE_AFTER_BIND',
  'VARIABLE_COUNT',
];

/** `crcbl_hal::ImageUsage`. */
const IMAGE_USAGE = [
  'TRANSFER_SRC',
  'TRANSFER_DST',
  'SAMPLED',
  'STORAGE',
  'COLOR_ATTACHMENT',
  'DEPTH_STENCIL_ATTACHMENT',
  'PRESENT',
];

/** `crcbl_hal::ImageAspect` — which planes of an image a view touches. */
const IMAGE_ASPECT = ['COLOR', 'DEPTH', 'STENCIL'];

/**
 * `crcbl_hal::ColorWrites` — which channels a colour target writes.
 *
 * A bitflags word like the two above, decoded to flag names in ascending bit
 * order and strict against an unclaimed bit — `from_bits`, never
 * `from_bits_truncate` — so a channel the encoder meant is never dropped. Each
 * bit maps to a `GPUColorWrite` bit in `gpu-replay.js`; the fixture carries one
 * `ALL` target and one `R | G` target, so a table narrower than Rust's refuses
 * the fixture.
 */
const COLOR_WRITES = ['R', 'G', 'B', 'A'];

/**
 * Every bit `crcbl_hal::Features` claims — `Features::all().bits()`.
 *
 * A MASK RATHER THAN A NAME TABLE, and the one bitflags field here that gets
 * one. The other two decode to lists of flag names, which is what makes a
 * decoded stream readable; a twenty-seven row table for this one would be
 * twenty-seven more things to keep in step with `crcbl-hal` for a value nothing
 * downstream reads by name — `gpu-replay.js` maps *bits* to `GPUFeatureName`s.
 *
 * It is still checked rather than waved through, because the seam's rule is
 * `from_bits` and never `from_bits_truncate`: an unclaimed bit is a build that
 * knows a flag this one does not, and dropping it would turn a feature the
 * caller required into one nobody asked for. The mask is held honest from
 * outside — the committed fixture carries a `RequestDevice` whose optional word
 * is `Features::all()`, so a mask narrower than Rust's refuses the fixture and
 * `stream-decode.mjs` goes red.
 */
const FEATURES_CLAIMED = (1n << 27n) - 1n;

// ── Errors ───────────────────────────────────────────────────────────────────

/**
 * Everything a malformed stream can be.
 *
 * `kind` mirrors a variant of `crcbl_webgpu::DecodeError` one for one, and
 * `details` carries that variant's fields under their Rust names. A caller
 * branches on `kind`; the message is for a human.
 *
 * @typedef {object} DecodeErrorDetails
 * @property {string} [field] Field that carried the offending value.
 * @property {number} [tag] The unknown tag byte.
 * @property {number} [offset] Where a too-short read started.
 * @property {number} [needed] Bytes that read wanted.
 * @property {number} [remaining] Bytes actually left.
 * @property {number} [len] A length or count past its cap.
 * @property {number|bigint} [code] An enum, bitflags or presence value no
 *   variant claims. A `bigint` for the one field that is sixty-four bits wide,
 *   `crcbl_hal::Features`, because a number could not carry the offending value
 *   back exactly — which is the whole of what this field is for.
 * @property {number} [found] Version the buffer declared.
 * @property {number} [expected] Version this build speaks.
 */
export class StreamDecodeError extends Error {
  /**
   * @param {'BadMagic'|'UnsupportedVersion'|'TooShort'|'UnknownTag'|'InvalidLength'|'InvalidEnum'|'NullHandle'|'NotUtf8'} kind
   * @param {string} message
   * @param {DecodeErrorDetails} [details]
   */
  constructor(kind, message, details = {}) {
    super(message);
    this.name = 'StreamDecodeError';
    this.kind = kind;
    this.details = details;
  }
}

// ── ByteReader ───────────────────────────────────────────────────────────────

/**
 * A cursor over a stream buffer that bounds-checks every read.
 *
 * Everything below goes through this rather than hand-rolling an offset test
 * per field, for the reason `reader.rs` gives: that is one more chance to get a
 * bound wrong at each of them, and this seam has a lot of fields.
 */
class ByteReader {
  /** @type {Uint8Array} */
  #bytes;
  /** @type {DataView} */
  #view;
  /** @type {TextDecoder} */
  #utf8 = new TextDecoder('utf-8', { fatal: true });
  #offset = 0;

  /** @param {Uint8Array} bytes */
  constructor(bytes) {
    this.#bytes = bytes;
    this.#view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  get remaining() {
    return this.#bytes.byteLength - this.#offset;
  }

  get isEmpty() {
    return this.remaining === 0;
  }

  /**
   * Reserves `needed` bytes and returns where they start, advancing past them.
   * Every read in this class begins here, so no other method indexes.
   *
   * @param {number} needed
   * @returns {number}
   */
  #take(needed) {
    if (this.remaining < needed) {
      throw new StreamDecodeError(
        'TooShort',
        `stream too short: need ${needed} bytes at offset ${this.#offset}, have ${this.remaining}`,
        { needed, offset: this.#offset, remaining: this.remaining }
      );
    }
    const at = this.#offset;
    this.#offset += needed;
    return at;
  }

  /**
   * A borrowed window on `len` bytes. It is a view over whatever the caller
   * handed in — wasm memory, in the browser — so it is compared or copied
   * before this method is called again, and never retained.
   *
   * @param {number} len
   * @returns {Uint8Array}
   */
  readBytes(len) {
    const at = this.#take(len);
    return this.#bytes.subarray(at, at + len);
  }

  readU8() {
    return this.#view.getUint8(this.#take(1));
  }

  readU16() {
    return this.#view.getUint16(this.#take(2), true);
  }

  readU32() {
    return this.#view.getUint32(this.#take(4), true);
  }

  readI32() {
    return this.#view.getInt32(this.#take(4), true);
  }

  readU64() {
    return this.#view.getBigUint64(this.#take(8), true);
  }

  readF32() {
    return this.#view.getFloat32(this.#take(4), true);
  }

  /**
   * A handle, as the `u64` `Handle::to_bits` packs: index low, generation high.
   * Bounds-checked as the eight bytes it is, so a truncation reports the same
   * numbers the Rust decoder does.
   *
   * @returns {{ index: number, generation: number }}
   */
  #readHandleBits() {
    const at = this.#take(8);
    return {
      index: this.#view.getUint32(at, true),
      generation: this.#view.getUint32(at + 4, true),
    };
  }

  /**
   * A handle that may not be absent. A zero generation is what
   * `Handle::from_bits` rejects, and no real handle ever has one.
   *
   * @param {string} field
   * @returns {{ index: number, generation: number }}
   */
  readHandle(field) {
    const handle = this.#readHandleBits();
    if (handle.generation === 0) {
      throw new StreamDecodeError('NullHandle', `${field} is a null handle`, {
        field,
      });
    }
    return handle;
  }

  /**
   * A handle that may be absent, as a bare `u64` with zero for `None`. No
   * presence byte: the generation's niche is what makes zero unambiguous.
   *
   * @returns {{ index: number, generation: number } | null}
   */
  readOptHandle() {
    const handle = this.#readHandleBits();
    return handle.generation === 0 ? null : handle;
  }

  /**
   * @param {string} field
   * @param {number} cap
   * @returns {number}
   */
  #readLen(field, cap) {
    const len = this.readU32();
    if (len > cap) {
      throw new StreamDecodeError(
        'InvalidLength',
        `invalid length for ${field}: ${len}`,
        { field, len }
      );
    }
    return len;
  }

  /**
   * A length-prefixed byte field, capped by `MAX_FIELD_BYTES`. Copied, because
   * the command object outlives the view it was decoded from.
   *
   * @param {string} field
   * @returns {Uint8Array}
   */
  readField(field) {
    return this.readBytes(this.#readLen(field, MAX_FIELD_BYTES)).slice();
  }

  /**
   * @param {string} field
   * @returns {string}
   */
  readString(field) {
    const bytes = this.readBytes(this.#readLen(field, MAX_FIELD_BYTES));
    try {
      return this.#utf8.decode(bytes);
    } catch {
      throw new StreamDecodeError('NotUtf8', `${field} is not valid UTF-8`, {
        field,
      });
    }
  }

  /**
   * A presence byte, then the string if there is one. `Some("")` and `None` are
   * different values and stay different here.
   *
   * @param {string} field
   * @returns {string | null}
   */
  readOptString(field) {
    return this.readPresent(field) ? this.readString(field) : null;
  }

  /**
   * A presence byte. Anything but the two canonical values is refused rather
   * than read as truthy.
   *
   * @param {string} field
   * @returns {boolean}
   */
  readPresent(field) {
    const code = this.readU8();
    if (code === ABSENT) return false;
    if (code === PRESENT) return true;
    throw new StreamDecodeError(
      'InvalidEnum',
      `invalid code for ${field}: 0x${code.toString(16)}`,
      { field, code }
    );
  }

  /**
   * An element count, capped by `MAX_ELEMENT_COUNT` *and* by the bytes left —
   * every element costs at least one byte, so a count past that cannot be
   * honest, and neither cap alone bounds the work on its own.
   *
   * @param {string} field
   * @returns {number}
   */
  readCount(field) {
    const count = this.#readLen(field, MAX_ELEMENT_COUNT);
    if (count > this.remaining) {
      throw new StreamDecodeError(
        'InvalidLength',
        `invalid length for ${field}: ${count}`,
        { field, len: count }
      );
    }
    return count;
  }

  /**
   * An enum code, through the table that claims it.
   *
   * @param {string} field
   * @param {readonly string[]} table
   * @returns {string}
   */
  readEnum(field, table) {
    const code = this.readU8();
    const name = table[code];
    if (name === undefined) {
      throw new StreamDecodeError(
        'InvalidEnum',
        `invalid code for ${field}: 0x${code.toString(16)}`,
        { field, code }
      );
    }
    return name;
  }

  /**
   * A presence byte, then an enum code if there is one.
   *
   * The optional-field rule applied to an enum rather than to a string: a
   * presence byte, because nothing but a handle has a niche to spare. It is two
   * reads rather than a table with a reserved "absent" row for a reason a
   * one-variant table would hide — `CompareOp::Never` is code `0`, so a reserved
   * absent row would put "no comparison at all" and "a comparison that always
   * fails" one byte apart, and a shadow sampler built with the second returns
   * zero everywhere.
   *
   * @param {string} field
   * @param {readonly string[]} table
   * @returns {string | null}
   */
  readOptEnum(field, table) {
    return this.readPresent(field) ? this.readEnum(field, table) : null;
  }

  /**
   * A presence byte, then a `u32` if there is one.
   *
   * The optional-field rule applied to a scalar rather than a string or an enum,
   * for `BindGroupDesc::variable_count`: a presence byte, because a bare `u32`
   * has no value to spare for `None` — `Some(0)` is a real variable count, so a
   * decoder that read the presence byte as the value would turn "no count" into
   * "a count of zero".
   *
   * @param {string} field
   * @returns {number | null}
   */
  readOptU32(field) {
    return this.readPresent(field) ? this.readU32() : null;
  }

  /**
   * A presence byte, then a `crcbl_hal::PushConstantRange` if there is one.
   *
   * The optional-field rule applied to a struct rather than a scalar. Its
   * `stages` reads through {@link ByteReader#readFlags} against {@link
   * SHADER_STAGES}, so a stage bit no flag claims is an error rather than a
   * truncation. The whole range crosses only so `gpu-replay.js` can refuse a
   * present one *by name* — WebGPU has no push constants at all — which is why
   * the strictness on the stages it carries still matters.
   *
   * @param {string} field
   * @returns {{ stages: string[], offset: number, size: number } | null}
   */
  readOptPushConstantRange(field) {
    if (!this.readPresent(field)) return null;
    return {
      stages: this.readFlags('PushConstantRange::stages', SHADER_STAGES),
      offset: this.readU32(),
      size: this.readU32(),
    };
  }

  /**
   * A `u32`-word slice: a count of words, then that many little-endian words.
   *
   * `crcbl_hal::ShaderModuleDesc::spirv`'s shape, and the one field on this stream
   * a browser never *uses* — WGSL is what `createShaderModule` consumes — but
   * still has to *traverse*, because it sits before the fields after it. Decoded
   * to a plain array of numbers, each `u32` and therefore exact, so the cursor
   * lands correctly on the WGSL that follows however many words there are. Absence
   * is the empty slice: a zero count is a real absent artifact, not a sentinel.
   *
   * @param {string} field
   * @returns {number[]}
   */
  readWords(field) {
    const count = this.readCount(field);
    const words = new Array(count);
    for (let i = 0; i < count; i += 1) words[i] = this.readU32();
    return words;
  }

  /**
   * A counted list of `(entry point, container)` pairs — the shape
   * `crcbl_hal::ShaderModuleDesc::dxil` crosses on, and the worst-shaped field on
   * the seam: each element is a length-prefixed string **and** a length-prefixed
   * byte slice, two variable-length leaves a fixed-stride reader cannot skip.
   *
   * The name is read first and the container second, each with its own length
   * prefix, so a pair whose container is empty is a *present* pair with a
   * zero-length blob rather than an absent one — the empty list is the only
   * absence. The container is copied, because the command object outlives the view
   * it was decoded from, exactly as {@link ByteReader#readField} is.
   *
   * @param {string} field
   * @returns {{ entryPoint: string, container: Uint8Array }[]}
   */
  readDxil(field) {
    const count = this.readCount(field);
    const pairs = new Array(count);
    for (let i = 0; i < count; i += 1) {
      pairs[i] = {
        entryPoint: this.readString(field),
        container: this.readField(field),
      };
    }
    return pairs;
  }

  /**
   * A bitflags value, as the names of the bits it sets in ascending bit order.
   *
   * Strict, like `from_bits` and unlike `from_bits_truncate`: a bit no flag
   * claims is an error, because truncating would silently drop something the
   * encoder meant.
   *
   * @param {string} field
   * @param {readonly string[]} table
   * @returns {string[]}
   */
  readFlags(field, table) {
    const bits = this.readU32();
    const claimed = 2 ** table.length - 1;
    if ((bits & ~claimed) >>> 0 !== 0) {
      throw new StreamDecodeError(
        'InvalidEnum',
        `invalid code for ${field}: 0x${bits.toString(16)}`,
        { field, code: bits }
      );
    }
    return table.filter((_, bit) => (bits & (1 << bit)) !== 0);
  }

  /**
   * A `crcbl_hal::Features` word, as the `BigInt` it is — sixty-four bits, and
   * a number is exact only to fifty-three.
   *
   * Strict for `readFlags`'s reason and refused the same way; see
   * {@link FEATURES_CLAIMED} for why this one stays a word rather than becoming
   * a list of names.
   *
   * @param {string} field
   * @returns {bigint}
   */
  readFeatures(field) {
    const bits = this.readU64();
    if ((bits & ~FEATURES_CLAIMED) !== 0n) {
      throw new StreamDecodeError(
        'InvalidEnum',
        `invalid code for ${field}: 0x${bits.toString(16)}`,
        { field, code: bits }
      );
    }
    return bits;
  }

  /** @returns {{ color: number[], depth: number, stencil: number }} */
  readClearValue() {
    return {
      color: [this.readF32(), this.readF32(), this.readF32(), this.readF32()],
      depth: this.readF32(),
      stencil: this.readU32(),
    };
  }

  /** @returns {{ x: number, y: number, width: number, height: number }} */
  readRect() {
    return {
      x: this.readI32(),
      y: this.readI32(),
      width: this.readU32(),
      height: this.readU32(),
    };
  }

  /**
   * A `crcbl_hal::Extent3d`. `depthOrLayers` is the depth for an `ImageType.D3`
   * and the array-layer count otherwise, and only the image type says which.
   *
   * @returns {{ width: number, height: number, depthOrLayers: number }}
   */
  readExtent() {
    return {
      width: this.readU32(),
      height: this.readU32(),
      depthOrLayers: this.readU32(),
    };
  }

  /**
   * A `crcbl_hal::ImageSubresourceRange`. `mipCount` and `layerCount` carry
   * `ImageSubresourceRange::ALL` — `0xFFFFFFFF`, "every remaining one" —
   * verbatim: resolving it needs the image's own counts, which the encoder does
   * not have either.
   *
   * @returns {{ aspect: string[], baseMip: number, mipCount: number, baseLayer: number, layerCount: number }}
   */
  readSubresourceRange() {
    return {
      aspect: this.readFlags('ImageSubresourceRange::aspect', IMAGE_ASPECT),
      baseMip: this.readU32(),
      mipCount: this.readU32(),
      baseLayer: this.readU32(),
      layerCount: this.readU32(),
    };
  }

  /**
   * A `crcbl_hal::ImageSubresourceLayers` — the single mip level a copy
   * addresses, distinct from the range above in that it has one `mip` rather
   * than a base-and-count. Its aspect goes through the same strict flag reader.
   *
   * @returns {{ aspect: string[], mip: number, baseLayer: number, layerCount: number }}
   */
  readSubresourceLayers() {
    return {
      aspect: this.readFlags('ImageSubresourceLayers::aspect', IMAGE_ASPECT),
      mip: this.readU32(),
      baseLayer: this.readU32(),
      layerCount: this.readU32(),
    };
  }

  /**
   * A `crcbl_hal::Offset3d` — three signed texel offsets, `x`, `y`, `z`.
   *
   * @returns {{ x: number, y: number, z: number }}
   */
  readOffset() {
    return { x: this.readI32(), y: this.readI32(), z: this.readI32() };
  }

  /**
   * A `crcbl_hal::SemaphoreWait` (or `SemaphoreSignal`, which is the same two
   * fields): the handle, then the `u64` value carried as a `BigInt`. WebGPU has
   * no semaphores, so the replayer refuses any non-empty list this appears in —
   * but the pair is decoded whole so the refusal can name it and the round trip
   * holds.
   *
   * @returns {{ semaphore: { index: number, generation: number }, value: bigint }}
   */
  readSemaphore(field) {
    return {
      semaphore: this.readHandle(field),
      value: this.readU64(),
    };
  }

  /**
   * A barrier's optional `crcbl_hal::QueueTransfer`: a presence byte, then the
   * releasing and acquiring queue handles if present. `null` when absent. WebGPU
   * has one implicit queue, so the replayer has none to honour a transfer with —
   * but it decodes whole so the round trip holds.
   *
   * @returns {{ from: { index: number, generation: number },
   *   to: { index: number, generation: number } } | null}
   */
  readQueueTransfer() {
    if (!this.readPresent('QueueTransfer')) return null;
    return {
      from: this.readHandle('QueueTransfer::from'),
      to: this.readHandle('QueueTransfer::to'),
    };
  }

  /**
   * A `crcbl_hal::BufferBarrier`: the buffer, its `from`/`to` states, then the
   * optional queue transfer. Carried whole though a `PipelineBarrier` is a
   * no-op — see {@link RESOURCE_STATE}.
   *
   * @returns {{ buffer: { index: number, generation: number }, from: string,
   *   to: string, queueTransfer: object|null }}
   */
  readBufferBarrier() {
    return {
      buffer: this.readHandle('BufferBarrier::buffer'),
      from: this.readEnum('BufferBarrier::from', RESOURCE_STATE),
      to: this.readEnum('BufferBarrier::to', RESOURCE_STATE),
      queueTransfer: this.readQueueTransfer(),
    };
  }

  /**
   * A `crcbl_hal::ImageBarrier`: the image, the subresource range it covers, its
   * `from`/`to` states, then the optional queue transfer.
   *
   * @returns {{ image: { index: number, generation: number }, range: object,
   *   from: string, to: string, queueTransfer: object|null }}
   */
  readImageBarrier() {
    return {
      image: this.readHandle('ImageBarrier::image'),
      range: this.readSubresourceRange(),
      from: this.readEnum('ImageBarrier::from', RESOURCE_STATE),
      to: this.readEnum('ImageBarrier::to', RESOURCE_STATE),
      queueTransfer: this.readQueueTransfer(),
    };
  }

  /**
   * A `crcbl_hal::BindingKind`: a code, then that variant's own fields.
   *
   * The one enum on this stream whose rows have bodies, and the reason the
   * refusal below is not optional: the bodies are different lengths, so a code
   * this build does not claim cannot be skipped past — there is no way to know
   * how far. See {@link BINDING_KIND}.
   *
   * The shape it answers with is `{ name, …fields }`, which is the shape a
   * command already has, so `gpu-replay.js` switches on `kind.name` exactly as
   * `replay` switches on `command.name`.
   *
   * @param {string} field
   * @returns {object}
   */
  readBindingKind(field) {
    const name = this.readEnum(field, BINDING_KIND);
    switch (name) {
      case 'UniformBuffer':
        return { name, dynamic: this.readPresent('BindingKind::dynamic') };
      case 'StorageBuffer':
        return {
          name,
          readOnly: this.readPresent('BindingKind::read_only'),
          dynamic: this.readPresent('BindingKind::dynamic'),
        };
      case 'SampledImage':
        return {
          name,
          viewType: this.readEnum('BindingKind::view_type', IMAGE_VIEW_TYPE),
          sampleType: this.readEnum('BindingKind::sample_type', SAMPLE_TYPE),
        };
      case 'StorageImage':
        return { name, readOnly: this.readPresent('BindingKind::read_only') };
      // The last row, and spelled out rather than left to a `default`: a
      // `default` here would give a variant added tomorrow an empty body and
      // leave the cursor one field short, which is the failure the table's own
      // docs describe.
      default:
        return {
          name,
          comparison: this.readPresent('BindingKind::comparison'),
        };
    }
  }

  /**
   * One `crcbl_hal::BindGroupLayoutEntry`, in the order the struct declares its
   * fields.
   *
   * `count` and `flags` cross **verbatim**, sentinel and all: `u32::MAX` means
   * "as many descriptors as this device can" and is resolved against a device's
   * own `max_bindless_descriptors`, which is a number this decoder does not have
   * — and `gpu-replay.js` has no binding arrays to resolve it into, so it
   * refuses. Both are the sentinel rule in `docs/plan/41-webgpu-stream.md`: the
   * encoder never decides, and what the resolution is remains the replayer's to
   * work out per field.
   *
   * @returns {{ binding: number, visibility: string[], kind: object,
   *             count: number, flags: string[] }}
   */
  readBindGroupLayoutEntry() {
    return {
      binding: this.readU32(),
      visibility: this.readFlags(
        'BindGroupLayoutEntry::visibility',
        SHADER_STAGES
      ),
      kind: this.readBindingKind('BindGroupLayoutEntry::kind'),
      count: this.readU32(),
      flags: this.readFlags('BindGroupLayoutEntry::flags', BINDING_FLAGS),
    };
  }

  /**
   * A `crcbl_hal::BindingResource`: a discriminant, then that variant's own
   * fields.
   *
   * The second enum on this stream whose rows have bodies, and the reason the
   * refusal below is not optional is {@link BINDING_RESOURCE}'s: the bodies are
   * different lengths, so a code this build does not claim cannot be skipped past.
   *
   * `offset` and `size` are `BigInt` — they are `u64` on the wire, and `size`
   * carries `BindingResource::WHOLE_BUFFER` (`u64::MAX`) through verbatim, which a
   * `Number` would round. The shape it answers with is `{ name, …fields }`, so
   * `gpu-replay.js` switches on `resource.name` exactly as `replay` switches on
   * `command.name`.
   *
   * @param {string} field
   * @returns {object}
   */
  readBindingResource(field) {
    const name = this.readEnum(field, BINDING_RESOURCE);
    switch (name) {
      case 'Buffer':
        return {
          name,
          buffer: this.readHandle('BindingResource::buffer'),
          offset: this.readU64(),
          size: this.readU64(),
        };
      case 'ImageView':
        return { name, view: this.readHandle('BindingResource::view') };
      // The last row, spelled out rather than left to a `default`, for
      // {@link ByteReader#readBindingKind}'s reason.
      default:
        return { name, sampler: this.readHandle('BindingResource::sampler') };
    }
  }

  /**
   * One `crcbl_hal::BindGroupEntry`, in the order the struct declares its fields.
   *
   * `arrayIndex` crosses beside `binding` and is not folded into it: it is the
   * bindless write path, and two entries that share a `binding` and differ only
   * in it are two distinct assignments the wire must keep apart.
   *
   * @returns {{ binding: number, arrayIndex: number, resource: object }}
   */
  readBindGroupEntry() {
    return {
      binding: this.readU32(),
      arrayIndex: this.readU32(),
      resource: this.readBindingResource('BindGroupEntry::resource'),
    };
  }

  readColorAttachment() {
    return {
      view: this.readHandle('ColorAttachment::view'),
      resolve: this.readOptHandle(),
      load: this.readEnum('ColorAttachment::load', LOAD_OP),
      store: this.readEnum('ColorAttachment::store', STORE_OP),
      clear: this.readClearValue(),
    };
  }

  readDepthStencilAttachment() {
    return {
      view: this.readHandle('DepthStencilAttachment::view'),
      readOnly: this.readPresent('DepthStencilAttachment::read_only'),
      depthLoad: this.readEnum('DepthStencilAttachment::depth_load', LOAD_OP),
      depthStore: this.readEnum(
        'DepthStencilAttachment::depth_store',
        STORE_OP
      ),
      stencilLoad: this.readEnum(
        'DepthStencilAttachment::stencil_load',
        LOAD_OP
      ),
      stencilStore: this.readEnum(
        'DepthStencilAttachment::stencil_store',
        STORE_OP
      ),
      clear: this.readClearValue(),
    };
  }

  /**
   * A `crcbl_hal::PrimitiveState`: four leaf enums and a `bool`, in the struct's
   * order. Each is named for its own field, so a byte out of position is an error
   * against the field it belongs to rather than a valid variant of the wrong one.
   *
   * @returns {{ topology: string, frontFace: string, cullMode: string, polygonMode: string, depthClamp: boolean }}
   */
  readPrimitiveState() {
    return {
      topology: this.readEnum('PrimitiveState::topology', PRIMITIVE_TOPOLOGY),
      frontFace: this.readEnum('PrimitiveState::front_face', FRONT_FACE),
      cullMode: this.readEnum('PrimitiveState::cull_mode', CULL_MODE),
      polygonMode: this.readEnum('PrimitiveState::polygon_mode', POLYGON_MODE),
      depthClamp: this.readPresent('PrimitiveState::depth_clamp'),
    };
  }

  /**
   * A `crcbl_hal::StencilFaceState`: compare, then the three `StencilOp`s in the
   * struct's order — `fail_op`, `depth_fail_op`, `pass_op`. Spelled out because
   * the three ops are the same table three times and any two read in the wrong
   * order still decodes to a face state.
   *
   * @returns {{ compare: string, failOp: string, depthFailOp: string, passOp: string }}
   */
  readStencilFaceState() {
    return {
      compare: this.readEnum('StencilFaceState::compare', COMPARE_OP),
      failOp: this.readEnum('StencilFaceState::fail_op', STENCIL_OP),
      depthFailOp: this.readEnum('StencilFaceState::depth_fail_op', STENCIL_OP),
      passOp: this.readEnum('StencilFaceState::pass_op', STENCIL_OP),
    };
  }

  /**
   * A `crcbl_hal::DepthStencilState` — the deepest optional chain on the seam.
   *
   * The stencil rides a presence byte and, when present, its `front` and `back`
   * faces come in that order — distinct in the fixture so a front/back swap goes
   * red — followed by the three masks. `reference` is decoded here even though it
   * is not a WebGPU pipeline field: it is per-pass state, so `gpu-replay.js`
   * drops it and takes what a draw compares against from the pass's own
   * `SetStencilReference` command instead. It round-trips but does not reach
   * `createRenderPipeline`. The three bias floats close it out.
   *
   * @returns {{ format: string, depthWrite: boolean, depthCompare: string,
   *   stencil: object | null, bias: { constant: number, slopeScale: number, clamp: number } }}
   */
  readDepthStencilState() {
    const format = this.readEnum('DepthStencilState::format', IMAGE_FORMAT);
    const depthWrite = this.readPresent('DepthStencilState::depth_write');
    const depthCompare = this.readEnum(
      'DepthStencilState::depth_compare',
      COMPARE_OP
    );
    let stencil = null;
    if (this.readPresent('DepthStencilState::stencil')) {
      stencil = {
        front: this.readStencilFaceState(),
        back: this.readStencilFaceState(),
        readMask: this.readU32(),
        writeMask: this.readU32(),
        reference: this.readU32(),
      };
    }
    return {
      format,
      depthWrite,
      depthCompare,
      stencil,
      bias: {
        constant: this.readF32(),
        slopeScale: this.readF32(),
        clamp: this.readF32(),
      },
    };
  }

  /**
   * A `crcbl_hal::MultisampleState`. `samples` and `mask` are adjacent `u32`s the
   * replayer reads different rules from — it refuses a `samples` count that is
   * neither 1 nor 4 — so they are read one at a time.
   *
   * @returns {{ samples: number, mask: number, alphaToCoverage: boolean }}
   */
  readMultisampleState() {
    return {
      samples: this.readU32(),
      mask: this.readU32(),
      alphaToCoverage: this.readPresent('MultisampleState::alpha_to_coverage'),
    };
  }

  /**
   * A `crcbl_hal::BlendState`: colour source/dest/op, then alpha source/dest/op,
   * in the struct's order — six leaf codes any two of which read in the wrong
   * order still decodes to a blend state, which is why the order is pinned here.
   *
   * @returns {{ colorSrc: string, colorDst: string, colorOp: string,
   *   alphaSrc: string, alphaDst: string, alphaOp: string }}
   */
  readBlendState() {
    return {
      colorSrc: this.readEnum('BlendState::color_src', BLEND_FACTOR),
      colorDst: this.readEnum('BlendState::color_dst', BLEND_FACTOR),
      colorOp: this.readEnum('BlendState::color_op', BLEND_OP),
      alphaSrc: this.readEnum('BlendState::alpha_src', BLEND_FACTOR),
      alphaDst: this.readEnum('BlendState::alpha_dst', BLEND_FACTOR),
      alphaOp: this.readEnum('BlendState::alpha_op', BLEND_OP),
    };
  }

  /**
   * A `crcbl_hal::ColorTargetState`: a format, an optional blend behind a
   * presence byte, and the `ColorWrites` mask. `None` for the blend is a shorter
   * body than a `Some` and stays distinct from an all-zero blend.
   *
   * @returns {{ format: string, blend: object | null, writeMask: string[] }}
   */
  readColorTargetState() {
    return {
      format: this.readEnum('ColorTargetState::format', IMAGE_FORMAT),
      blend: this.readPresent('ColorTargetState::blend')
        ? this.readBlendState()
        : null,
      writeMask: this.readFlags('ColorTargetState::write_mask', COLOR_WRITES),
    };
  }
}

// ── StreamReader ─────────────────────────────────────────────────────────────

/**
 * Decodes a buffer `crcbl_webgpu::StreamWriter` produced.
 *
 * Commands come out one at a time so a replayer can hold the sequence number of
 * the one it is executing: a WebGPU validation error names this decoder and not
 * the Rust that encoded the command, and the sequence is the only thing that
 * connects the two back up.
 */
export class StreamReader {
  /** @type {ByteReader} */
  #reader;
  /** @type {bigint} */
  #baseSequence;
  #decoded = 0n;
  /**
   * Set once a decode fails. Nothing is resumable after that: the cursor is
   * somewhere inside a command body and the next byte is not a tag.
   */
  #failed = false;

  /**
   * Opens a stream, checking its magic and version.
   *
   * The version check is not ceremony: the wasm and JavaScript halves ship as
   * separate artifacts and are cached independently, so a decoder meeting a
   * stream from a different build is reachable in a browser in a way it is not
   * in a single binary.
   *
   * @param {Uint8Array} bytes The whole buffer, header included.
   * @throws {StreamDecodeError} If the header is not this format's, or if there
   *   is not a whole header.
   */
  constructor(bytes) {
    this.#reader = new ByteReader(bytes);
    const magic = this.#reader.readBytes(STREAM_MAGIC.length);
    if (!STREAM_MAGIC.every((byte, at) => byte === magic[at])) {
      throw new StreamDecodeError(
        'BadMagic',
        'not a command stream: bad magic'
      );
    }
    const version = this.#reader.readU16();
    if (version !== STREAM_VERSION) {
      throw new StreamDecodeError(
        'UnsupportedVersion',
        `stream format version ${version}, expected ${STREAM_VERSION}`,
        { found: version, expected: STREAM_VERSION }
      );
    }
    this.#baseSequence = this.#reader.readU64();
  }

  /** The sequence number of the first command in this buffer. */
  get baseSequence() {
    return this.#baseSequence;
  }

  /**
   * The next command and the sequence number it carries, or `null` at the end
   * of the stream.
   *
   * Sequence numbers are positional — the nth command decoded is
   * `baseSequence + n` — which is why nothing per command is on the wire.
   *
   * Returns `null` forever after a throw: the cursor is then somewhere inside a
   * command body, so the next byte is not a tag and resuming would invent
   * commands out of a payload.
   *
   * @returns {{ sequence: bigint, command: object } | null}
   * @throws {StreamDecodeError} Anything the command body produces.
   */
  nextCommand() {
    if (this.#failed || this.#reader.isEmpty) return null;
    let command;
    try {
      command = decodeCommand(this.#reader);
    } catch (error) {
      this.#failed = true;
      throw error;
    }
    // Wrapping, because the base came off the wire: a buffer declaring
    // `u64::MAX` must not produce a sequence outside the range it is typed as.
    const sequence = BigInt.asUintN(64, this.#baseSequence + this.#decoded);
    this.#decoded += 1n;
    return { sequence, command };
  }
}

/**
 * Every command in a stream, in order.
 *
 * The convenience half of {@link StreamReader}, for a test or a dump that wants
 * the whole buffer at once. The sequence numbers are the reader's
 * `baseSequence` plus each command's index.
 *
 * @param {Uint8Array} bytes
 * @returns {object[]}
 * @throws {StreamDecodeError} The first error the stream produces; nothing
 *   after it is decoded.
 */
export function decodeStream(bytes) {
  const reader = new StreamReader(bytes);
  const commands = [];
  for (
    let next = reader.nextCommand();
    next !== null;
    next = reader.nextCommand()
  ) {
    commands.push(next.command);
  }
  return commands;
}

/**
 * One command body, dispatched on its tag.
 *
 * The tag comes first so this dispatches rather than trial-decodes, which is
 * what keeps "unknown command" and "malformed known command" distinguishable —
 * the two errors below.
 *
 * @param {ByteReader} r
 * @returns {object}
 */
function decodeCommand(r) {
  const tag = r.readU8();
  switch (tag) {
    case CREATE_BUFFER_TAG:
      return {
        name: 'CreateBuffer',
        buffer: r.readHandle('CreateBuffer::buffer'),
        label: r.readOptString('BufferDesc::label'),
        size: r.readU64(),
        usage: r.readFlags('BufferDesc::usage', BUFFER_USAGE),
        memory: r.readEnum('BufferDesc::memory', MEMORY_LOCATION),
      };
    case CREATE_SURFACE_TAG:
      // `canvasId` is a key into the shell's own canvas registry, not a handle
      // and not a `GPUCanvasContext`: the shim assigned the number so that no
      // string has to cross the boundary. A `u32`, so it stays a number.
      return {
        name: 'CreateSurface',
        surface: r.readHandle('CreateSurface::surface'),
        canvasId: r.readU32(),
      };
    case CREATE_OFFSCREEN_SURFACE_TAG:
      // The whole body is the handle — no canvas key to resolve, and no extent
      // or format, which belong to the swapchain rather than the surface. The
      // replayer marks the id as offscreen so a later swapchain naming it builds
      // an owned ring of textures instead of configuring a canvas context.
      return {
        name: 'CreateOffscreenSurface',
        surface: r.readHandle('CreateOffscreenSurface::surface'),
      };
    case CREATE_IMAGE_TAG: {
      // Spelled out rather than built inline, for `Draw`'s reason: `mipLevels`
      // and `samples` are adjacent, identically typed and mean different
      // things, which is the pair a decoder most easily swaps.
      //
      // Both are carried verbatim, ZERO INCLUDED. Neither value is one a device
      // accepts, and neither is a malformed stream: every `u32` is a value the
      // wire form claims, so there is nothing here to refuse that a flag word
      // or an enum table would refuse. An invalid descriptor is a creation
      // failure, and those arrive through `Device::take_error` rather than out
      // of a decoder.
      const image = r.readHandle('CreateImage::image');
      const label = r.readOptString('ImageDesc::label');
      const imageType = r.readEnum('ImageDesc::image_type', IMAGE_TYPE);
      const extent = r.readExtent();
      const format = r.readEnum('ImageDesc::format', IMAGE_FORMAT);
      const mipLevels = r.readU32();
      const samples = r.readU32();
      return {
        name: 'CreateImage',
        image,
        label,
        imageType,
        extent,
        format,
        mipLevels,
        samples,
        usage: r.readFlags('ImageDesc::usage', IMAGE_USAGE),
      };
    }
    case CREATE_IMAGE_VIEW_TAG: {
      // Two handles, and they mean opposite things: `view` is the id the
      // replayer stores the new object at, `image` the id it looks the viewed
      // object up by. Spelled out so the two cannot be read in the other order.
      const view = r.readHandle('CreateImageView::view');
      const label = r.readOptString('ImageViewDesc::label');
      const image = r.readHandle('ImageViewDesc::image');
      const viewType = r.readEnum('ImageViewDesc::view_type', IMAGE_VIEW_TYPE);
      const format = r.readEnum('ImageViewDesc::format', IMAGE_FORMAT);
      return {
        name: 'CreateImageView',
        view,
        label,
        image,
        viewType,
        format,
        range: r.readSubresourceRange(),
      };
    }
    case CREATE_SAMPLER_TAG: {
      // Spelled out one field at a time, for `CreateImage`'s reason with six
      // fields instead of two: three `FilterMode` bytes and three
      // `SamplerAddressMode` bytes go over back to back, and any two of the six
      // read in the wrong order still decodes to a sampler.
      //
      // The three floats cross verbatim, SENTINEL INCLUDED. `lodMax` is
      // `f32::MAX` for a `SamplerDesc::default`, meaning "no limit", and
      // resolving it is the replayer's job — `gpu-replay.js` argues what it
      // resolves to, and it is not an absent member. `anisotropy` is whatever
      // the caller passed, fractional values and values past the device's cap
      // included, because every `f32` bit pattern is a value the wire form
      // claims.
      const sampler = r.readHandle('CreateSampler::sampler');
      const label = r.readOptString('SamplerDesc::label');
      const magFilter = r.readEnum('SamplerDesc::mag_filter', FILTER_MODE);
      const minFilter = r.readEnum('SamplerDesc::min_filter', FILTER_MODE);
      const mipFilter = r.readEnum('SamplerDesc::mip_filter', FILTER_MODE);
      const addressMode = [
        r.readEnum('SamplerDesc::address_mode', SAMPLER_ADDRESS_MODE),
        r.readEnum('SamplerDesc::address_mode', SAMPLER_ADDRESS_MODE),
        r.readEnum('SamplerDesc::address_mode', SAMPLER_ADDRESS_MODE),
      ];
      const lodMin = r.readF32();
      const lodMax = r.readF32();
      const anisotropy = r.readF32();
      return {
        name: 'CreateSampler',
        sampler,
        label,
        magFilter,
        minFilter,
        mipFilter,
        addressMode,
        lodMin,
        lodMax,
        anisotropy,
        compare: r.readOptEnum('SamplerDesc::compare', COMPARE_OP),
      };
    }
    case CREATE_BIND_GROUP_LAYOUT_TAG: {
      // **A counted list of structs**, which nothing before this command
      // carries: `dynamic_offsets` is a list of scalars whose stride cannot be
      // wrong, and this one is five fields deep with an enum whose payloads have
      // different lengths.
      //
      // The entries are pushed in wire order and NEVER SORTED OR KEYED BY
      // `binding`. `docs/plan/41-webgpu-stream.md` says why: the slice is
      // order-sensitive, because a `VARIABLE_COUNT` entry must be both last in
      // it and highest-numbered, and the first half of that rule is a property
      // of the *list* rather than of its contents. The fixture carries a layout
      // whose two entries share a binding number, so a decoder that rebuilt the
      // list from them loses one.
      const layout = r.readHandle('CreateBindGroupLayout::layout');
      const label = r.readOptString('BindGroupLayoutDesc::label');
      const count = r.readCount('BindGroupLayoutDesc::entries');
      const entries = [];
      for (let i = 0; i < count; i += 1) {
        entries.push(r.readBindGroupLayoutEntry());
      }
      return { name: 'CreateBindGroupLayout', layout, label, entries };
    }
    case CREATE_BIND_GROUP_TAG: {
      // **The second counted list of structs**, and deeper than the layout's:
      // each entry carries a `BindingResource` whose variants have
      // different-length bodies, so a stride out by a byte decodes the next entry
      // out of the middle of this one.
      //
      // The entries are pushed in wire order and NEVER KEYED BY `binding`: an
      // entry's `arrayIndex` is the bindless write path, so two entries may share
      // a binding, and a decoder that rebuilt the list from binding numbers would
      // lose one.
      const group = r.readHandle('CreateBindGroup::group');
      const label = r.readOptString('BindGroupDesc::label');
      const layout = r.readHandle('BindGroupDesc::layout');
      const count = r.readCount('BindGroupDesc::entries');
      const entries = [];
      for (let i = 0; i < count; i += 1) {
        entries.push(r.readBindGroupEntry());
      }
      return {
        name: 'CreateBindGroup',
        group,
        label,
        layout,
        entries,
        // An optional scalar behind a presence byte, as `create_sampler`'s
        // `compare` is: `Some(0)` and `None` must stay distinct.
        variableCount: r.readOptU32('BindGroupDesc::variable_count'),
      };
    }
    case CREATE_SHADER_MODULE_TAG: {
      // **The heaviest descriptor on the seam, and every field is traversed even
      // though a browser reads only WGSL.** The four artifacts do not share an
      // absence convention, and each is decoded by its own convention: `spirv`
      // empty is absent (a word array), `wgsl` and `msl` keep `Some("")` apart
      // from `None` through {@link ByteReader#readOptString}, and `dxil`'s empty
      // list is absence while a pair with an empty container is a present,
      // truncated artifact. The words are decoded in order so the cursor lands on
      // the WGSL after them — reading a later field before skipping `spirv` would
      // decode the wrong bytes for every field that follows.
      const module = r.readHandle('CreateShaderModule::module');
      const label = r.readOptString('ShaderModuleDesc::label');
      const spirv = r.readWords('ShaderModuleDesc::spirv');
      const wgsl = r.readOptString('ShaderModuleDesc::wgsl');
      const msl = r.readOptString('ShaderModuleDesc::msl');
      return {
        name: 'CreateShaderModule',
        module,
        label,
        spirv,
        wgsl,
        msl,
        dxil: r.readDxil('ShaderModuleDesc::dxil'),
      };
    }
    case CREATE_PIPELINE_LAYOUT_TAG: {
      // **A counted list of bare handles, in set order** — what a shader's
      // `@group(n)` indexes — so it is pushed in wire order and never sorted, and
      // a single-element list would prove nothing about that. The fixture carries
      // a two-layout pipeline layout for exactly that reason. `pushConstants`
      // crosses whole even though WebGPU has none at all: `gpu-replay.js` refuses
      // a present one by name, and it can only refuse what it was told.
      const layout = r.readHandle('CreatePipelineLayout::layout');
      const label = r.readOptString('PipelineLayoutDesc::label');
      const count = r.readCount('PipelineLayoutDesc::bind_group_layouts');
      const bindGroupLayouts = [];
      for (let i = 0; i < count; i += 1) {
        bindGroupLayouts.push(
          r.readHandle('PipelineLayoutDesc::bind_group_layouts')
        );
      }
      return {
        name: 'CreatePipelineLayout',
        layout,
        label,
        bindGroupLayouts,
        pushConstants: r.readOptPushConstantRange(
          'PipelineLayoutDesc::push_constants'
        ),
      };
    }
    case CREATE_COMPUTE_PIPELINE_TAG: {
      // **TWO HANDLES INTO TWO DIFFERENT TABLES.** `layout` resolves against the
      // pipeline-layout table and `module` against the shader-module table, and a
      // handle carries no kind, so which table each indexes is the wire position
      // and nothing else — spelled out one at a time so the two cannot be read in
      // the other order. `entryPoint` is a bare string, always present.
      //
      // `workgroupSize` is three `u32`s and crosses whole even though a WebGPU
      // replayer drops it: WebGPU reads the size from the module's
      // `@workgroup_size`, but `crcbl_hal::ComputePipelineDesc` carries it because
      // Metal has nowhere else to declare it. `gpu-replay.js` drops it there.
      const pipeline = r.readHandle('CreateComputePipeline::pipeline');
      const label = r.readOptString('ComputePipelineDesc::label');
      const layout = r.readHandle('ComputePipelineDesc::layout');
      const module = r.readHandle('ShaderEntry::module');
      const entryPoint = r.readString('ShaderEntry::entry_point');
      const workgroupSize = [r.readU32(), r.readU32(), r.readU32()];
      return {
        name: 'CreateComputePipeline',
        pipeline,
        label,
        layout,
        module,
        entryPoint,
        workgroupSize,
      };
    }
    case CREATE_GRAPHICS_PIPELINE_TAG: {
      // **The largest descriptor on the seam, and the deepest.** The vertex stage
      // is a module handle and an entry point and no buffer layout — vertex
      // pulling, so `GPUVertexState.buffers` is the empty array — and the fragment
      // stage rides a presence byte, `null` for a depth-only pass. `layout`,
      // `vertexModule` and the fragment module all resolve out of different tables
      // by their wire position, since a handle carries no kind. The four state
      // blocks that follow are read through the field readers above, deepest of
      // them the depth-stencil chain. Nothing is validated: every "WebGPU cannot
      // express it" refusal is `gpu-replay.js`'s, which can only refuse what it
      // was told.
      const pipeline = r.readHandle('CreateGraphicsPipeline::pipeline');
      const label = r.readOptString('GraphicsPipelineDesc::label');
      const layout = r.readHandle('GraphicsPipelineDesc::layout');
      const vertexModule = r.readHandle('ShaderEntry::module');
      const vertexEntryPoint = r.readString('ShaderEntry::entry_point');
      const fragment = r.readPresent('GraphicsPipelineDesc::fragment')
        ? {
            module: r.readHandle('ShaderEntry::module'),
            entryPoint: r.readString('ShaderEntry::entry_point'),
          }
        : null;
      const primitive = r.readPrimitiveState();
      const depthStencil = r.readPresent('GraphicsPipelineDesc::depth_stencil')
        ? r.readDepthStencilState()
        : null;
      const multisample = r.readMultisampleState();
      const count = r.readCount('GraphicsPipelineDesc::color_targets');
      const colorTargets = [];
      for (let i = 0; i < count; i += 1) {
        colorTargets.push(r.readColorTargetState());
      }
      return {
        name: 'CreateGraphicsPipeline',
        pipeline,
        label,
        layout,
        vertexModule,
        vertexEntryPoint,
        fragment,
        primitive,
        depthStencil,
        multisample,
        colorTargets,
      };
    }
    case CREATE_QUERY_SET_TAG: {
      // The handle, the label, the kind code, then the count. All three kinds
      // decode although the replayer serves only `Occlusion`: the wire carries
      // what the caller wrote, and `gpu-replay.js` names each refusal.
      const set = r.readHandle('CreateQuerySet::set');
      const label = r.readOptString('QuerySetDesc::label');
      const kind = r.readEnum('QuerySetDesc::kind', QUERY_KIND);
      return { name: 'CreateQuerySet', set, label, kind, count: r.readU32() };
    }
    case DESTROY_QUERY_SET_TAG:
      // Its own tag and its own table again, and — like the pipeline destroys —
      // one whose empty slot is the *ordinary* case: a caller that asked for a
      // timestamp set got an `Err` and destroys the handle it pre-allocated.
      return {
        name: 'DestroyQuerySet',
        set: r.readHandle('DestroyQuerySet::set'),
      };
    case DESTROY_GRAPHICS_PIPELINE_TAG:
      // Its own tag and its own table again: a graphics pipeline's id is allowed
      // to be the same eight bytes as anything else's, and — like the
      // compute-pipeline destroy — the one whose empty slot is the *ordinary*
      // case, since the replayer refuses a pipeline it cannot build and the caller
      // destroys the pre-allocated handle regardless.
      return {
        name: 'DestroyGraphicsPipeline',
        pipeline: r.readHandle('DestroyGraphicsPipeline::pipeline'),
      };
    case DESTROY_COMPUTE_PIPELINE_TAG:
      // Its own tag and its own table again: a compute pipeline's id is allowed to
      // be the same eight bytes as anything else's, and — like the pipeline-layout
      // destroy — the one whose empty slot is the *ordinary* case, since the
      // replayer refuses a pipeline it cannot build (an unresolvable layout or
      // module) and the caller destroys the pre-allocated handle regardless.
      return {
        name: 'DestroyComputePipeline',
        pipeline: r.readHandle('DestroyComputePipeline::pipeline'),
      };
    case DESTROY_SHADER_MODULE_TAG:
      // Its own tag and its own table again: a shader module's id is allowed to
      // be the same eight bytes as anything else's.
      return {
        name: 'DestroyShaderModule',
        module: r.readHandle('DestroyShaderModule::module'),
      };
    case DESTROY_PIPELINE_LAYOUT_TAG:
      // Its own tag and its own table again, and — like the bind-group layout's
      // destroy — the one whose empty slot is the *ordinary* case: the replayer
      // refuses a layout it cannot express (a present push-constant range, an
      // unresolvable bind-group layout), so the handle the caller pre-allocated
      // is released with nothing behind it every time that happens.
      return {
        name: 'DestroyPipelineLayout',
        layout: r.readHandle('DestroyPipelineLayout::layout'),
      };
    case DESTROY_BIND_GROUP_LAYOUT_TAG:
      // Its own tag and its own table again, and the destroy whose empty slot is
      // the *ordinary* case: the replayer refuses a layout it cannot express —
      // any `BindingFlags`, any `count` but one, a mesh or task stage — so the
      // handle the caller pre-allocated is released with nothing behind it every
      // time that happens.
      return {
        name: 'DestroyBindGroupLayout',
        layout: r.readHandle('DestroyBindGroupLayout::layout'),
      };
    case DESTROY_BIND_GROUP_TAG:
      // Its own tag and its own table again: a bind group's id is allowed to be
      // the same eight bytes as anything else's.
      return {
        name: 'DestroyBindGroup',
        group: r.readHandle('DestroyBindGroup::group'),
      };
    case DESTROY_BUFFER_TAG:
      return {
        name: 'DestroyBuffer',
        buffer: r.readHandle('DestroyBuffer::buffer'),
      };
    case DESTROY_SURFACE_TAG:
      return {
        name: 'DestroySurface',
        surface: r.readHandle('DestroySurface::surface'),
      };
    case DESTROY_IMAGE_TAG:
      return {
        name: 'DestroyImage',
        image: r.readHandle('DestroyImage::image'),
      };
    case DESTROY_IMAGE_VIEW_TAG:
      // Its own tag rather than a kind byte on one destroy: the opcode is what
      // says which table an id indexes, and a view's table and its image's
      // genuinely issue identical bits.
      return {
        name: 'DestroyImageView',
        view: r.readHandle('DestroyImageView::view'),
      };
    case DESTROY_SAMPLER_TAG:
      // Its own tag again, and its own table on the far side: a sampler's id
      // and an image's are allowed to be the same eight bytes, and the probe's
      // deliberately are.
      return {
        name: 'DestroySampler',
        sampler: r.readHandle('DestroySampler::sampler'),
      };
    case BEGIN_DEBUG_LABEL_TAG:
      return {
        name: 'BeginDebugLabel',
        label: r.readString('BeginDebugLabel::label'),
      };
    case END_DEBUG_LABEL_TAG:
      // Body-less: it closes the region `BeginDebugLabel` opened, without naming
      // it — the replayer pops the scope that pushed. See `gpu-replay.js`.
      return { name: 'EndDebugLabel' };
    case INSERT_DEBUG_MARKER_TAG:
      // The same field `BeginDebugLabel` carries; only the tag says which of the
      // two the replayer calls.
      return {
        name: 'InsertDebugMarker',
        label: r.readString('InsertDebugMarker::label'),
      };
    case BEGIN_RENDER_PASS_TAG: {
      const label = r.readOptString('RenderPassDesc::label');
      const count = r.readCount('RenderPassDesc::color_attachments');
      const colorAttachments = [];
      for (let i = 0; i < count; i += 1) {
        colorAttachments.push(r.readColorAttachment());
      }
      const depthStencilAttachment = r.readPresent(
        'RenderPassDesc::depth_stencil_attachment'
      )
        ? r.readDepthStencilAttachment()
        : null;
      return {
        name: 'BeginRenderPass',
        label,
        colorAttachments,
        depthStencilAttachment,
        renderArea: r.readRect(),
      };
    }
    case BIND_GRAPHICS_PIPELINE_TAG:
      return {
        name: 'BindGraphicsPipeline',
        pipeline: r.readHandle('BindGraphicsPipeline::pipeline'),
      };
    case BIND_GROUP_TAG: {
      const slot = r.readU32();
      const group = r.readHandle('BindGroup::group');
      const count = r.readCount('BindGroup::dynamic_offsets');
      const dynamicOffsets = [];
      for (let i = 0; i < count; i += 1) {
        dynamicOffsets.push(r.readU32());
      }
      return {
        name: 'BindGroup',
        slot,
        group,
        dynamicOffsets,
        layout: r.readHandle('BindGroup::layout'),
      };
    }
    case PUSH_CONSTANTS_TAG:
      return {
        name: 'PushConstants',
        stages: r.readFlags('PushConstants::stages', SHADER_STAGES),
        offset: r.readU32(),
        data: r.readField('PushConstants::data'),
        layout: r.readHandle('PushConstants::layout'),
      };
    case SET_VIEWPORT_TAG: {
      // `Viewport` in declaration order: the rectangle's four floats, then the
      // depth range's two. See `gpu-replay.js`.
      const x = r.readF32();
      const y = r.readF32();
      const width = r.readF32();
      const height = r.readF32();
      const depthMin = r.readF32();
      const depthMax = r.readF32();
      return {
        name: 'SetViewport',
        viewport: { x, y, width, height, depthMin, depthMax },
      };
    }
    case SET_SCISSOR_TAG:
      // `Rect2d`, the same shape `render_area` carries. `x`/`y` are signed on
      // the wire; `setScissorRect` takes unsigned, so a negative is the
      // replayer's to refuse. See `gpu-replay.js`.
      return { name: 'SetScissor', rect: r.readRect() };
    case SET_STENCIL_REFERENCE_TAG:
      // One `u32`, which is what WebGPU's `GPUStencilValue` is too, so nothing
      // narrows it. See `gpu-replay.js`.
      return { name: 'SetStencilReference', reference: r.readU32() };
    case BIND_INDEX_BUFFER_TAG: {
      // The buffer, its byte offset (`u64`, `BigInt`), then the index-format
      // code. See `gpu-replay.js`.
      const buffer = r.readHandle('BindIndexBuffer::buffer');
      const offset = r.readU64();
      const format = r.readEnum('BindIndexBuffer::format', INDEX_FORMAT);
      return { name: 'BindIndexBuffer', buffer, offset, format };
    }
    case CREATE_COMMAND_ENCODER_TAG:
      // No handle: the encoder is the replayer's implicit-current one, as
      // `crcbl-hal`'s recording methods assume no receiver. `queue` crosses and
      // selects no queue — WebGPU has one implicit queue — so the replayer drops
      // it, the way it drops a compute pipeline's `workgroupSize`.
      return {
        name: 'CreateCommandEncoder',
        label: r.readOptString('CommandEncoderDesc::label'),
        queue: r.readHandle('CommandEncoderDesc::queue'),
      };
    case END_RENDER_PASS_TAG:
      // Body-less: it closes the pass `BeginRenderPass` opened, on the
      // implicit-current encoder.
      return { name: 'EndRenderPass' };
    case FINISH_TAG:
      // Names the caller-allocated command buffer the encoder seals into.
      return {
        name: 'Finish',
        commandBuffer: r.readHandle('Finish::command_buffer'),
      };
    case BEGIN_COMPUTE_PASS_TAG:
      // `ComputePassDesc` is only a label — compute has no attachments.
      return {
        name: 'BeginComputePass',
        label: r.readOptString('ComputePassDesc::label'),
      };
    case BIND_COMPUTE_PIPELINE_TAG:
      return {
        name: 'BindComputePipeline',
        pipeline: r.readHandle('BindComputePipeline::pipeline'),
      };
    case END_COMPUTE_PASS_TAG:
      // Body-less: it closes the pass `BeginComputePass` opened, on the
      // implicit-current encoder.
      return { name: 'EndComputePass' };
    case RESET_QUERY_SET_TAG: {
      // The set, then the range as a first index and a count — never as its two
      // ends, so an empty range is `0` rather than something this decoder would
      // have to check for inversion. The replayer records nothing for it; see
      // `gpu-replay.js`.
      const set = r.readHandle('ResetQuerySet::set');
      return {
        name: 'ResetQuerySet',
        set,
        firstQuery: r.readU32(),
        queryCount: r.readU32(),
      };
    }
    case RESOLVE_QUERY_SET_TAG: {
      // The set and its range, then the destination and its byte offset (`u64`,
      // `BigInt`). The two rules WebGPU imposes on that destination — a
      // 256-aligned offset and `QUERY_RESOLVE` usage — are the replayer's, which
      // is where the buffer's usage bits are.
      const set = r.readHandle('ResolveQuerySet::set');
      const firstQuery = r.readU32();
      const queryCount = r.readU32();
      const dst = r.readHandle('ResolveQuerySet::dst');
      return {
        name: 'ResolveQuerySet',
        set,
        firstQuery,
        queryCount,
        dst,
        dstOffset: r.readU64(),
      };
    }
    case QUERY_RESULTS_TAG: {
      // The set and the range to read. Answered, by a `QueryResults` reply
      // naming this command's sequence — see `gpu-replay.js` for why serving it
      // costs a resolve, a copy and a map.
      const set = r.readHandle('QueryResults::set');
      return {
        name: 'QueryResults',
        set,
        firstQuery: r.readU32(),
        queryCount: r.readU32(),
      };
    }
    case DISPATCH_TAG:
      // The three workgroup counts, in the order the HAL call takes them.
      return {
        name: 'Dispatch',
        x: r.readU32(),
        y: r.readU32(),
        z: r.readU32(),
      };
    case DISPATCH_INDIRECT_TAG: {
      // The argument buffer, then its byte offset (`u64`, `BigInt`). No count
      // and no stride: WebGPU's `dispatchWorkgroupsIndirect` is a single
      // dispatch, so there is nothing to unroll. See `gpu-replay.js`.
      const buffer = r.readHandle('DispatchIndirect::buffer');
      return { name: 'DispatchIndirect', buffer, offset: r.readU64() };
    }
    case COPY_IMAGE_TO_BUFFER_TAG: {
      // `BufferImageCopy` in declaration order; the direction is the tag, never a
      // field. `bufferRowLength` is in TEXELS and `0` is tightly packed — both
      // cross verbatim, and turning texels into 256-aligned bytes is the
      // replayer's, which is the only side that has the texture's format. See
      // `gpu-replay.js`.
      const buffer = r.readHandle('BufferImageCopy::buffer');
      const bufferOffset = r.readU64();
      const bufferRowLength = r.readU32();
      const bufferImageHeight = r.readU32();
      const image = r.readHandle('BufferImageCopy::image');
      const imageSubresource = r.readSubresourceLayers();
      const imageOffset = r.readOffset();
      const imageExtent = r.readExtent();
      return {
        name: 'CopyImageToBuffer',
        buffer,
        bufferOffset,
        bufferRowLength,
        bufferImageHeight,
        image,
        imageSubresource,
        imageOffset,
        imageExtent,
      };
    }
    case COPY_BUFFER_TO_BUFFER_TAG: {
      // `BufferCopy` in declaration order: source and its offset, destination
      // and its offset, then the size. The two offsets and the size are `u64`,
      // read as `BigInt`.
      const src = r.readHandle('BufferCopy::src');
      const srcOffset = r.readU64();
      const dst = r.readHandle('BufferCopy::dst');
      const dstOffset = r.readU64();
      const size = r.readU64();
      return {
        name: 'CopyBufferToBuffer',
        copy: { src, srcOffset, dst, dstOffset, size },
      };
    }
    case COPY_BUFFER_TO_IMAGE_TAG: {
      // `BufferImageCopy` in declaration order, the same layout
      // `CopyImageToBuffer` reads; the direction is the tag, never a field.
      // `bufferRowLength` is in TEXELS and `0` is tightly packed, the texel→byte
      // conversion the replayer's. See `gpu-replay.js`.
      const buffer = r.readHandle('BufferImageCopy::buffer');
      const bufferOffset = r.readU64();
      const bufferRowLength = r.readU32();
      const bufferImageHeight = r.readU32();
      const image = r.readHandle('BufferImageCopy::image');
      const imageSubresource = r.readSubresourceLayers();
      const imageOffset = r.readOffset();
      const imageExtent = r.readExtent();
      return {
        name: 'CopyBufferToImage',
        buffer,
        bufferOffset,
        bufferRowLength,
        bufferImageHeight,
        image,
        imageSubresource,
        imageOffset,
        imageExtent,
      };
    }
    case COPY_IMAGE_TO_IMAGE_TAG: {
      // `ImageCopy` in declaration order: source, its subresource and offset,
      // destination, its subresource and offset, then the shared extent.
      const src = r.readHandle('ImageCopy::src');
      const srcSubresource = r.readSubresourceLayers();
      const srcOffset = r.readOffset();
      const dst = r.readHandle('ImageCopy::dst');
      const dstSubresource = r.readSubresourceLayers();
      const dstOffset = r.readOffset();
      const extent = r.readExtent();
      return {
        name: 'CopyImageToImage',
        copy: {
          src,
          srcSubresource,
          srcOffset,
          dst,
          dstSubresource,
          dstOffset,
          extent,
        },
      };
    }
    case FILL_BUFFER_TAG: {
      // The buffer, its offset and size (`u64`, `BigInt`), then the `u32` value.
      // Only `0` is expressible on WebGPU (`clearBuffer`); the replayer refuses
      // any other value.
      const buffer = r.readHandle('FillBuffer::buffer');
      const offset = r.readU64();
      const size = r.readU64();
      const value = r.readU32();
      return { name: 'FillBuffer', buffer, offset, size, value };
    }
    case WRITE_BUFFER_TAG: {
      // A host→buffer upload: the buffer, its byte offset (`u64`, `BigInt`),
      // then the bytes. The payload arrives as its own `Uint8Array`, exactly as
      // `PushConstants::data` does. See `gpu-replay.js`.
      const buffer = r.readHandle('WriteBuffer::buffer');
      const offset = r.readU64();
      const data = r.readField('WriteBuffer::data');
      return { name: 'WriteBuffer', buffer, offset, data };
    }
    case CREATE_SWAPCHAIN_TAG: {
      // `SwapchainDesc` in declaration order behind the caller-allocated handle:
      // the surface, the format, the extent's two components, the image count,
      // then the present-mode and composite-alpha enum codes. `imageCount` and
      // `presentMode` are carried verbatim even though the replayer drops them —
      // a browser only offers fifo and manages its own buffering.
      const swapchain = r.readHandle('CreateSwapchain::swapchain');
      const label = r.readOptString('SwapchainDesc::label');
      const surface = r.readHandle('SwapchainDesc::surface');
      const format = r.readEnum('SwapchainDesc::format', IMAGE_FORMAT);
      const width = r.readU32();
      const height = r.readU32();
      const imageCount = r.readU32();
      const presentMode = r.readEnum(
        'SwapchainDesc::present_mode',
        SWAPCHAIN_PRESENT_MODE
      );
      const compositeAlpha = r.readEnum(
        'SwapchainDesc::composite_alpha',
        SWAPCHAIN_COMPOSITE_ALPHA
      );
      return {
        name: 'CreateSwapchain',
        swapchain,
        label,
        surface,
        format,
        extent: { width, height },
        imageCount,
        presentMode,
        compositeAlpha,
      };
    }
    case ACQUIRE_NEXT_FRAME_TAG: {
      // The swapchain, then the two caller-allocated handles the acquired texture
      // and its view are filed under — three handles that mean different things,
      // so spelled out one at a time.
      const swapchain = r.readHandle('AcquireNextFrame::swapchain');
      const image = r.readHandle('AcquireNextFrame::image');
      const view = r.readHandle('AcquireNextFrame::view');
      return { name: 'AcquireNextFrame', swapchain, image, view };
    }
    case PRESENT_TAG: {
      // The swapchain, the counted waits, then the optional `presentId` behind a
      // presence byte. The wait list is decoded whole so the replayer can refuse
      // a non-empty one by name — WebGPU has no semaphores. `presentId` is a
      // `u64`, a `BigInt`.
      const swapchain = r.readHandle('PresentInfo::swapchain');
      const waitCount = r.readCount('PresentInfo::waits');
      const waits = [];
      for (let i = 0; i < waitCount; i += 1) {
        waits.push(r.readHandle('PresentInfo::waits'));
      }
      const presentId = r.readPresent('PresentInfo::present_id')
        ? r.readU64()
        : null;
      return { name: 'Present', swapchain, waits, presentId };
    }
    case DESTROY_SWAPCHAIN_TAG:
      return {
        name: 'DestroySwapchain',
        swapchain: r.readHandle('DestroySwapchain::swapchain'),
      };
    case RECONFIGURE_SWAPCHAIN_TAG: {
      // The same `SwapchainDesc` layout `CreateSwapchain` decodes, behind the
      // handle of the swapchain to re-configure. Parallel to that arm rather than
      // sharing a helper: the two produce different command names and the fixture
      // round-trip is what proves they stay in step.
      const swapchain = r.readHandle('ReconfigureSwapchain::swapchain');
      const label = r.readOptString('SwapchainDesc::label');
      const surface = r.readHandle('SwapchainDesc::surface');
      const format = r.readEnum('SwapchainDesc::format', IMAGE_FORMAT);
      const width = r.readU32();
      const height = r.readU32();
      const imageCount = r.readU32();
      const presentMode = r.readEnum(
        'SwapchainDesc::present_mode',
        SWAPCHAIN_PRESENT_MODE
      );
      const compositeAlpha = r.readEnum(
        'SwapchainDesc::composite_alpha',
        SWAPCHAIN_COMPOSITE_ALPHA
      );
      return {
        name: 'ReconfigureSwapchain',
        swapchain,
        label,
        surface,
        format,
        extent: { width, height },
        imageCount,
        presentMode,
        compositeAlpha,
      };
    }
    case PIPELINE_BARRIER_TAG: {
      // The `Barriers` batch: the counted buffer list, the counted image list,
      // then the `global` flag. Decoded whole for wire fidelity — the replayer
      // records nothing, because WebGPU tracks resource state itself. See
      // `gpu-replay.js`.
      const bufferCount = r.readCount('Barriers::buffers');
      const buffers = [];
      for (let i = 0; i < bufferCount; i += 1) {
        buffers.push(r.readBufferBarrier());
      }
      const imageCount = r.readCount('Barriers::images');
      const images = [];
      for (let i = 0; i < imageCount; i += 1) {
        images.push(r.readImageBarrier());
      }
      const global = r.readPresent('Barriers::global');
      return { name: 'PipelineBarrier', buffers, images, global };
    }
    case DRAW_TAG: {
      // Spelled out rather than built inline: the two halves of each range are
      // read in source order either way, but relying on that to get the field
      // order right reads as a coincidence.
      const firstVertex = r.readU32();
      const lastVertex = r.readU32();
      const firstInstance = r.readU32();
      const lastInstance = r.readU32();
      return {
        name: 'Draw',
        vertices: { start: firstVertex, end: lastVertex },
        instances: { start: firstInstance, end: lastInstance },
      };
    }
    case DRAW_INDEXED_TAG: {
      // The index range's start and end, the signed `baseVertex`, then the
      // instance range's start and end — spelled out for `DRAW_TAG`'s reason.
      // See `gpu-replay.js`.
      const firstIndex = r.readU32();
      const lastIndex = r.readU32();
      const baseVertex = r.readI32();
      const firstInstance = r.readU32();
      const lastInstance = r.readU32();
      return {
        name: 'DrawIndexed',
        indices: { start: firstIndex, end: lastIndex },
        baseVertex,
        instances: { start: firstInstance, end: lastInstance },
      };
    }
    case DRAW_INDIRECT_TAG: {
      // The argument buffer, its byte offset (`u64`, `BigInt`), the CPU-known
      // draw count, then the stride. The replayer unrolls the count into that
      // many single-draw calls. See `gpu-replay.js`.
      const buffer = r.readHandle('DrawIndirect::buffer');
      const offset = r.readU64();
      const drawCount = r.readU32();
      const stride = r.readU32();
      return { name: 'DrawIndirect', buffer, offset, drawCount, stride };
    }
    case DRAW_INDEXED_INDIRECT_TAG: {
      // The same four fields `DRAW_INDIRECT_TAG` carries, in the same order —
      // only the argument layout the replayer's call reads differs. See
      // `gpu-replay.js`.
      const buffer = r.readHandle('DrawIndexedIndirect::buffer');
      const offset = r.readU64();
      const drawCount = r.readU32();
      const stride = r.readU32();
      return { name: 'DrawIndexedIndirect', buffer, offset, drawCount, stride };
    }
    case SUBMIT_TAG: {
      // The command buffers, then the counted waits and signals. The two
      // semaphore lists are empty on every frame WebGPU can honour; a non-empty
      // one is decoded whole so the replayer can refuse it by name.
      const bufferCount = r.readCount('SubmitInfo::command_buffers');
      const commandBuffers = [];
      for (let i = 0; i < bufferCount; i += 1) {
        commandBuffers.push(r.readHandle('SubmitInfo::command_buffers'));
      }
      const waitCount = r.readCount('SubmitInfo::waits');
      const waits = [];
      for (let i = 0; i < waitCount; i += 1) {
        waits.push(r.readSemaphore('SubmitInfo::waits'));
      }
      const signalCount = r.readCount('SubmitInfo::signals');
      const signals = [];
      for (let i = 0; i < signalCount; i += 1) {
        signals.push(r.readSemaphore('SubmitInfo::signals'));
      }
      return { name: 'Submit', commandBuffers, waits, signals };
    }
    case REQUEST_READBACK_TAG: {
      // The caller-allocated handle, then the `ReadbackDesc`. `after` rides a
      // presence byte: `null` is `mapAsync`, a present one is a semaphore wait
      // the replayer refuses. `offset` and `size` are `BigInt`s, a buffer's own
      // `u64`s.
      const readback = r.readHandle('RequestReadback::readback');
      const label = r.readOptString('ReadbackDesc::label');
      const buffer = r.readHandle('ReadbackDesc::buffer');
      const offset = r.readU64();
      const size = r.readU64();
      const after = r.readPresent('ReadbackDesc::after')
        ? r.readSemaphore('ReadbackDesc::after')
        : null;
      return {
        name: 'RequestReadback',
        readback,
        label,
        buffer,
        offset,
        size,
        after,
      };
    }
    case POLL_READBACK_TAG:
      return {
        name: 'PollReadback',
        readback: r.readHandle('PollReadback::readback'),
      };
    case TAKE_ERROR_TAG:
      // No body: the HAL call takes nothing and this seam holds one device, so
      // there is nothing to name. It is answered even when nothing went wrong —
      // an empty `deviceErrors` — because a command nobody answers leaves its
      // sequence waiting for ever on the far side, and this one is asked again
      // every time the engine's queue runs dry.
      return { name: 'TakeError' };
    case DESTROY_READBACK_TAG:
      return {
        name: 'DestroyReadback',
        readback: r.readHandle('DestroyReadback::readback'),
      };
    case DESTROY_COMMAND_BUFFER_TAG:
      return {
        name: 'DestroyCommandBuffer',
        commandBuffer: r.readHandle('DestroyCommandBuffer::command_buffer'),
      };
    case REQUEST_DEVICE_TAG: {
      // Spelled out rather than built inline, for `Draw`'s reason: the two
      // feature words are adjacent, identically typed and mean opposite things,
      // which is the pair a reader most easily swaps.
      const adapter = r.readU32();
      const label = r.readOptString('DeviceDesc::label');
      const requiredFeatures = r.readFeatures('DeviceDesc::required_features');
      const optionalFeatures = r.readFeatures('DeviceDesc::optional_features');
      return {
        name: 'RequestDevice',
        adapter,
        label,
        requiredFeatures,
        optionalFeatures,
        compatibleSurface: r.readOptHandle(),
      };
    }
    case SURFACE_CAPS_TAG:
      // No body: the HAL call's surface and adapter are validated where the
      // handle tables are and never cross. A decoder that still read them would
      // consume twelve bytes of whatever follows, which is why the corpus puts
      // a command after this one.
      return { name: 'SurfaceCaps' };
    case ENUMERATE_ADAPTERS_TAG:
      // No body either, and last in the corpus: a decoder that read one field
      // too many here runs off the end of the buffer rather than into a
      // neighbour, which is why `stream-decode.mjs` sweeps every truncation.
      return { name: 'EnumerateAdapters' };
    default:
      throw new StreamDecodeError(
        'UnknownTag',
        `unknown command tag: 0x${tag.toString(16).padStart(2, '0')}`,
        { tag }
      );
  }
}
