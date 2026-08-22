// The `crcbl-webgpu` seam groups, and nothing else.
//
// The letters are allocation order rather than run order: AC, AD and AE run
// between W and X, where a group that reads bytes back has to sit on a platform
// whose canvas readback strands everything queued behind it. Each one's own
// comment says why.
//
// **THE ONLY GATE THAT DRIVES THIS SEAM COMMAND BY COMMAND.** Everything else
// about the command stream is checked without a browser: `stream-decode.mjs`
// decodes the fixture Rust commits, `reply-encode.mjs` re-encodes the replies
// Rust reads, `stream-transport.mjs` drives the ABI against a synthetic
// `WebAssembly.Memory`, and `gpu-replay.mjs` runs the replayer against a
// `navigator.gpu` that is not one. **None of them can call the real thing**,
// which is exactly what these groups do: a command encoded in wasm reaches
// `navigator.gpu`, what the browser answers gets back into wasm through the
// reply channel, a device actually opens on the adapter that answer named, real
// resources are made on it — and from S onward the bytes come back and their
// *values* are compared.
//
// The other two browser gates ask different questions of the same backend, and
// neither can be asked one command at a time: `web/run-browser-e2e.sh` boots a
// demo and reads its canvas, and `web/run-render-harness-e2e.sh` drives whole
// golden scenes through it and compares the frames against the images the native
// suites compare against.
//
// Every claim is corroborated against something the page can see for itself: the
// adapter's name and features against `navigator.gpu`, the device's against a
// device the page opens with the same descriptor, the pixels against the colour
// the command asked for. A check that only read back what wasm sent would prove
// the transport and nothing else, and the transport already has a node suite.
//
// WHERE THEY RUN. `web/probe/` — a page with no engine on it, which is the whole
// condition the probe has: it installs its own `StreamChannel`, a page has
// exactly one, so nothing else may have claimed it. That page also owns the
// pump, so "the loop replayed it" below means the page's own
// drain→replay→deliver over `gpu-transport.js` and the shared `Replayer`, on its
// own `requestAnimationFrame` frames. `web/tools/probe-e2e.mjs` is the driver
// that loads the page and calls this.
//
// **ONE CHECK FROM THE ORIGINAL SET IS NOT HERE.** Group G used to end with "the
// demo kept running with a channel installed under it", read off `crcbl.status()`
// — a claim about a channel being invisible to an engine running underneath it.
// There is no engine on the probe page and therefore nothing that could notice,
// so the check is dropped rather than reworded into one that asserts less than it
// says. Groups A–F of `web/tools/browser-e2e.mjs` are what still drive a demo
// through this backend.
//
// It exports one function and holds no state of its own: the driver owns the
// browser, the page and the check list, and hands them in.

/**
 * The key group G adds a second, wrong canvas to the page's registry under.
 *
 * `SurfaceTarget::Web` is an integer into the JS-side canvas registry and
 * nothing else, so this number is the page's to choose and means nothing to
 * wasm. The *right* key is not here: `web/probe/main.js` owns the registry and
 * registers the page's own canvas under its own `CANVAS_ID`, which the driver
 * reads back as `crcbl.gpu.canvasId` rather than restating.
 *
 * **The decoy is why a second one is added at all.** With one canvas in the
 * registry, a replayer that ignored the `canvasId` on the wire and took whatever
 * it found would hand back the right context by accident, and no identity check
 * could tell. It is inserted *ahead of* the page's canvas — group G re-inserts
 * that one to make it so — because a `Map` keeps insertion order, and the decoy
 * is only a decoy while "the first entry" is also the wrong answer.
 * `web/tools/gpu-replay.mjs` runs the same shape against a stub.
 */
const DECOY_CANVAS_ID = 6;

/**
 * The `crcbl_hal::Format` codes each canvas format the specification defines
 * has — its own, and its sRGB counterpart's — from
 * `crates/crcbl-webgpu/src/tag.rs`.
 *
 * Spelled out here rather than imported from `gpu-replay.js` for the reason the
 * feature bits in group G are: a table taken from the thing under test agrees
 * with it by construction, and what the check below is for is that the table
 * itself is right about what this browser prefers.
 *
 * BOTH CODES, BECAUSE THE ANSWER IS THE COUNTERPART AND THE FAILURE IS THE
 * LINEAR ONE. `SurfaceCaps::preferred_format` takes the first sRGB entry, and
 * `surfaceCapsFor` leads with the counterpart of the browser's own preference —
 * so `srgb` is what wasm must receive, and receiving `linear` instead is exactly
 * the browser build that presented every frame a transfer function too dark.
 * Naming both is what lets the check say which happened.
 */
const SEAM_FORMAT_CODE = Object.freeze({
  rgba8unorm: { linear: 0x02, srgb: 0x03 },
  bgra8unorm: { linear: 0x04, srgb: 0x05 },
});

/** The bit `SurfaceCaps::present_modes` sets for `PresentMode::Fifo`. */
const FIFO_BIT = 1 << 0;

/**
 * How many bytes group J asks wasm to make a buffer of.
 *
 * **The driver's number rather than wasm's**, which is what makes the size check
 * evidence: `__crcbl_web_gpu_probe_buffer` takes it as an argument, a browser
 * reports `GPUBuffer.size` off the object it created, and the two are compared.
 * A size fixed in `probe.rs` would be a constant checked against itself.
 *
 * Nothing about the value matters beyond being a size no default produces —
 * `4096` is the fixture's, so this is deliberately not that either.
 */
const PROBE_BUFFER_BYTES = 12_288;

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `probe_buffer_desc` sets.
 *
 * Restated rather than exported through the ABI: it is a `&'static str` on the
 * Rust side and the point of checking it is that the string *crossed* — a label
 * the driver read out of wasm would agree with wasm whatever the replayer did
 * with it.
 */
const PROBE_BUFFER_LABEL = 'crcbl-webgpu probe buffer';

/**
 * The extent and mip count group K asks wasm to make a texture with.
 *
 * The driver's numbers rather than wasm's, for {@link PROBE_BUFFER_BYTES}'s
 * reason: `__crcbl_web_gpu_probe_image` takes all three as arguments and a
 * browser reports `GPUTexture.width`, `.height` and `.mipLevelCount` off the
 * object it created, so the two are compared rather than restated.
 *
 * A power of two in each axis with a chain shorter than the full one, so the mip
 * count is a number the extent permits and is visibly not the default of `1`.
 */
const PROBE_IMAGE_WIDTH = 256;
const PROBE_IMAGE_HEIGHT = 128;
const PROBE_IMAGE_MIPS = 4;

/**
 * The labels `crates/crcbl-webgpu/src/probe.rs` sets on the image and its view.
 *
 * Restated rather than exported through the ABI, for {@link PROBE_BUFFER_LABEL}'s
 * reason — the point of checking one is that the string *crossed*.
 */
const PROBE_IMAGE_LABEL = 'crcbl-webgpu probe image';
const PROBE_VIEW_LABEL = 'crcbl-webgpu probe view';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_SAMPLER_DESC` sets, and
 * **the only member of the sampler a browser reports at all**.
 *
 * There is no `PROBE_SAMPLER_*` number beside it, unlike the buffer's size and
 * the image's extent, and that is the object rather than an omission: a
 * `GPUSampler` has no readable filters, address modes or clamps, so there is
 * nothing a page could pass in and read back. What group L has instead is the
 * class of what came back and the silence of the device's error queue — see the
 * group's own comment.
 */
const PROBE_SAMPLER_LABEL = 'crcbl-webgpu probe sampler';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_BIND_GROUP_LAYOUT_DESC`
 * sets, and — as with a sampler — **the only member of the layout a browser
 * reports at all**.
 *
 * A `GPUBindGroupLayout` exposes no entries, no bindings and no visibility, so
 * group M's evidence is the class of what came back and the silence of the
 * device's error queue afterwards. What the descriptor was is checkable only
 * before it is handed over, which is `web/tools/gpu-replay.mjs`'s job.
 */
const PROBE_LAYOUT_LABEL = 'crcbl-webgpu probe layout';

/**
 * How many entries `PROBE_BIND_GROUP_LAYOUT_ENTRIES` declares.
 *
 * Not readable off the `GPUBindGroupLayout` — nothing about a layout is — so
 * this is not compared against the object. It is what the group's message
 * *says*, so a person reading a failure knows how much of a list the browser was
 * asked to take: four entries, not one, because this is the first command whose
 * body is a counted list of structs and a single-entry layout would decode the
 * same whatever the stride.
 */
const PROBE_LAYOUT_ENTRIES = 4;

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_BIND_GROUP_DESC` sets,
 * and — as with a sampler and a layout — **the only member of the group a browser
 * reports at all**.
 *
 * A `GPUBindGroup` exposes no layout and no entries, so group N's evidence is the
 * class of what came back and the silence of the device's error queue afterwards.
 * What the group *bound* is checkable only before it is handed over, which is
 * `web/tools/gpu-replay.mjs`'s job.
 */
const PROBE_BIND_GROUP_LABEL = 'crcbl-webgpu probe bind group';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_SHADER_MODULE_DESC` sets.
 *
 * A `GPUShaderModule` reports its `label` like every object above — but, unlike
 * them, it is **where compilation happens**, so group O has a second and stronger
 * piece of evidence than the label and the silent error queue: `getCompilationInfo()`.
 * The descriptor carries a known-good WGSL vertex entry, so a clean compile is a
 * fact a real browser can state and a stub cannot fake.
 */
const PROBE_SHADER_MODULE_LABEL = 'crcbl-webgpu probe shader';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_PIPELINE_LAYOUT_DESC`
 * sets.
 *
 * A `GPUPipelineLayout` reports its `label` and nothing else — not its
 * bind-group layouts, not its push-constant ranges (WebGPU has none) — so group
 * P's evidence is group N's two: the class of what came back, which
 * `instanceof GPUPipelineLayout` settles and no stub can satisfy, and the
 * device's error queue being empty afterwards, which is the only thing that can
 * say `createPipelineLayout` *accepted* the set list it was handed.
 */
const PROBE_PIPELINE_LAYOUT_LABEL = 'crcbl-webgpu probe pipeline layout';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_COMPUTE_PIPELINE_DESC`
 * sets.
 *
 * A `GPUComputePipeline` reports its `label` like a pipeline layout — but, unlike
 * it, it also answers `getBindGroupLayout(n)`, the derived layout only a
 * genuinely-built pipeline can hand back, because a pipeline is where the shader
 * and its layout are validated against each other. So group Q has that call as a
 * second piece of evidence beyond `instanceof GPUComputePipeline` and the silent
 * error queue.
 */
const PROBE_COMPUTE_PIPELINE_LABEL = 'crcbl-webgpu probe compute pipeline';

/**
 * The label `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_GRAPHICS_PIPELINE_DESC`
 * sets.
 *
 * A `GPURenderPipeline` reports its `label` and answers `getBindGroupLayout(n)`
 * exactly as a compute pipeline does, so group R has the same two pieces of
 * evidence — but it is the *largest* descriptor on the seam, so what group R
 * really puts in front of the device is a whole nested tree: the primitive
 * state, the reversed-Z depth-stencil, the multisample state, and a blended
 * colour target, all of which a real `createRenderPipeline` must accept.
 */
const PROBE_GRAPHICS_PIPELINE_LABEL = 'crcbl-webgpu probe raster pipeline';

/**
 * Runs every `crcbl-webgpu` seam group against an already-loaded probe page.
 *
 * The caller owns the browser and the tally; this owns what is asserted. Each
 * dependency is passed rather than imported so that the driver's `check` is the
 * one counting, which is what makes "how many checks ran" a number the driver
 * can print and a script can guard.
 *
 * @param {object} deps
 * @param {object} deps.page a connected DevTools page, for `evaluate`
 * @param {(page: object, expression: string) => Promise<any>} deps.evaluate
 * @param {(probe: () => Promise<any>) => Promise<any>} deps.until polls to a
 *   deadline and answers `null` when it passes
 * @param {(group: string, name: string, ok: any, detail?: string) => boolean}
 *   deps.check
 * @param {(name: string) => void} deps.group
 * @param {number} deps.TIMEOUT_MS what `until` gives up after, for the messages
 */
export async function runProbeGroups({
  page,
  evaluate,
  until,
  check,
  group,
  TIMEOUT_MS,
}) {
  group('G — the WebGPU command stream makes a round trip');

  // **NOTHING HERE DRAINS, REPLAYS OR DELIVERS.** The probe encodes a command
  // and the page's own frame loop does the rest — one drain, one replay, one
  // delivery per frame, in `web/probe/main.js`'s `pump`, which is
  // `web/engine/demo.js`'s `pumpGpu` over the same transport and the same
  // `Replayer`. So every check below waits for that loop to have done the work,
  // which is what makes this group a gate on the loop the WebGPU backend uses
  // rather than on a replayer the driver stood up for itself. A page whose loop
  // stopped replaying fails here even though the transport, the format and the
  // replayer are all still correct — that is the claim, and no other check makes
  // it.
  //
  // The decoy goes into the page's *own* registry, which is the only one there
  // is, and group H is what reads it back. See DECOY_CANVAS_ID.
  const probe = await evaluate(
    page,
    `(async () => {
     const { startAdapterProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const decoy = document.createElement('canvas');
     // Deleted and put back so that the decoy is ahead of it: a Map keeps
     // insertion order, and a decoy behind the right answer is not one.
     const canvas = gpu.canvases.get(gpu.canvasId);
     gpu.canvases.delete(gpu.canvasId);
     gpu.canvases.set(${DECOY_CANVAS_ID}, decoy);
     gpu.canvases.set(gpu.canvasId, canvas);
     globalThis.crcblProbe = { decoy };
     const before = gpu.stats();
     return {
       started: startAdapterProbe({ exports }),
       replayed: before.replayed,
       delivered: before.delivered,
     };
   })()`
  );

  // Polled, because the command is encoded on this evaluation's frame and
  // replayed on the page's next one. `stats().replayed` counting up is the loop
  // saying it took a frame off the channel; `commands` is what that frame
  // carried, decoded by the loop rather than by anything here.
  const carried = probe?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const stats = globalThis.crcbl.gpu.stats();
           return stats.replayed > ${probe.replayed} || stats.failure
             ? stats
             : null;
         })()`
        )
      )
    : null;
  check(
    'G',
    'wasm encoded an adapter enumeration and the page loop replayed it',
    carried?.commands?.join(',') === 'EnumerateAdapters',
    carried
      ? `the loop replayed [${carried.commands.join(', ')}] at sequence ${carried.baseSequence}` +
          `${carried.failure ? ` — ${carried.failure}` : ''}`
      : probe?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'the probe could not install a channel — something else already has one'
  );

  // Then polled again, because the answer cannot be on the frame that asked:
  // WebGPU's adapter API is a promise and the stream is replayed synchronously
  // once a frame, so the reply is queued when the browser settles and the loop
  // hands it over on a later frame. `WAITING` is the ordinary answer until then
  // and is not a state to report.
  //
  // `delivered` comes from the loop's own counter rather than from this call,
  // which no longer moves any bytes: it is the loop saying wasm took a reply
  // buffer, beside wasm saying it understood one.
  const answered = await until(async () =>
    evaluate(
      page,
      `(async () => {
       const { readAdapterProbe, PROBE } = await import('/engine/gpu-probe.js');
       const { exports, memory, gpu } = globalThis.crcbl;
       const out = readAdapterProbe({ exports, memory });
       return out.state === PROBE.WAITING
         ? null
         : { ...out, delivered: gpu.stats().delivered };
     })()`
    )
  );
  check(
    'G',
    'the browser answered it and wasm read the answer back',
    answered?.name === 'GRANTED' &&
      answered.delivered > (probe?.delivered ?? 0),
    answered
      ? `${answered.name}: ${JSON.stringify(answered.text)}, after the loop delivered ` +
          `${answered.delivered - (probe?.delivered ?? 0)} reply buffer(s)`
      : `no answer in ${TIMEOUT_MS} ms — the reply never reached wasm`
  );

  // **The name is the browser's own, not a string the round trip invented.**
  // Asked of the same page, through the same joins `gpu-replay.js` makes, at
  // the same moment — so a replayer that answered with a constant, or with the
  // wrong adapter, differs here. Nothing else in this group would notice.
  //
  // The same evaluation also brings back the raw `adapter.features` and
  // `adapter.limits` — what the browser says, before anything of ours has
  // touched them — and what the page's own mapping makes of the limits. Those
  // are what the capability checks below are held against.
  const live = await evaluate(
    page,
    `(async () => {
     const { halLimitsFor } = await import('/engine/gpu-replay.js');
     const adapter = await navigator.gpu.requestAdapter();
     const info = adapter?.info ?? {};
     const mapped = halLimitsFor(adapter);
     return {
       name: [info.vendor, info.architecture, info.device, info.description]
         .filter(Boolean).join(' '),
       features: [...adapter.features].sort(),
       limits: {
         maxTextureDimension2D: adapter.limits.maxTextureDimension2D,
         maxTextureDimension3D: adapter.limits.maxTextureDimension3D,
         maxTextureArrayLayers: adapter.limits.maxTextureArrayLayers,
         maxStorageBufferBindingSize: adapter.limits.maxStorageBufferBindingSize,
         maxUniformBufferBindingSize: adapter.limits.maxUniformBufferBindingSize,
         maxBindGroups: adapter.limits.maxBindGroups,
         maxColorAttachments: adapter.limits.maxColorAttachments,
         maxComputeWorkgroupSizeX: adapter.limits.maxComputeWorkgroupSizeX,
         maxComputeWorkgroupSizeY: adapter.limits.maxComputeWorkgroupSizeY,
         maxComputeWorkgroupSizeZ: adapter.limits.maxComputeWorkgroupSizeZ,
         maxComputeInvocationsPerWorkgroup:
           adapter.limits.maxComputeInvocationsPerWorkgroup,
         maxComputeWorkgroupsPerDimension:
           adapter.limits.maxComputeWorkgroupsPerDimension,
         minUniformBufferOffsetAlignment:
           adapter.limits.minUniformBufferOffsetAlignment,
         minStorageBufferOffsetAlignment:
           adapter.limits.minStorageBufferOffsetAlignment,
       },
       // BigInts do not survive the wire back to this driver, and every one
       // of these is far below 2^53.
       mapped: Object.fromEntries(
         Object.entries(mapped).map(([key, value]) =>
           [key, typeof value === 'bigint' ? Number(value) : value])
       ),
     };
   })()`
  );
  check(
    'G',
    'the adapter name wasm received is the one this browser reports',
    typeof answered?.text === 'string' && answered.text === live?.name,
    answered?.text === live?.name
      ? `both say ${JSON.stringify(live?.name)}`
      : `wasm got ${JSON.stringify(answered?.text)}, the browser says ${JSON.stringify(live?.name)}`
  );

  // **The capabilities crossed too, and they are the browser's own.** The whole
  // of `AdapterInfo` now travels, and five of its seven wire fields are the
  // documented absences that a browser has nothing to disagree with — so the two
  // that vary are the two worth asking a browser about, and both are asked here.
  //
  // The expected bits are spelled out below rather than imported from
  // `gpu-replay.js`: a table taken from the thing under test agrees with it by
  // construction, and what this check is for is that the table itself is right
  // about this browser's feature set.
  const wasmCaps = answered?.caps;
  // `crcbl_hal::Features` bits: the four core WebGPU grants outright, then the
  // four with a `GPUFeatureName` behind them.
  const CORE_FEATURE_BITS = (1 << 8) | (1 << 7) | (1 << 14) | (1 << 18); // COMPUTE, OCCLUSION_QUERY, DEPTH_BIAS_CLAMP, DEBUG_MARKERS
  const NAMED_FEATURE_BITS = {
    'depth-clip-control': 1 << 13, // DEPTH_CLAMP
    'texture-compression-bc': 1 << 16, // TEXTURE_COMPRESSION_BC
    'timestamp-query': 1 << 5, // TIMESTAMP_QUERY
    'indirect-first-instance': 1 << 4, // INDIRECT_FIRST_INSTANCE
  };
  /** The `crcbl_hal::Features` word a list of `GPUFeatureName`s amounts to. */
  const featureBitsOf = (names) =>
    (names ?? []).reduce(
      (bits, name) => bits | (NAMED_FEATURE_BITS[name] ?? 0),
      CORE_FEATURE_BITS
    ) >>> 0;
  const expectedFeatures = featureBitsOf(live?.features);
  check(
    'G',
    'the feature set wasm received is what this browser actually reports',
    wasmCaps?.featuresLo === expectedFeatures && wasmCaps?.featuresHi === 0,
    `wasm got ${wasmCaps?.featuresLo?.toString(16)}/${wasmCaps?.featuresHi?.toString(16)},` +
      ` ${expectedFeatures.toString(16)} expected from [${(live?.features ?? []).join(', ')}]`
  );
  check(
    'G',
    'a limit wasm received is the number this browser reports',
    wasmCaps?.maxImage2d === live?.limits?.maxTextureDimension2D &&
      wasmCaps?.maxImage2d > 0,
    `wasm got ${wasmCaps?.maxImage2d}, navigator.gpu says` +
      ` maxTextureDimension2D ${live?.limits?.maxTextureDimension2D}`
  );

  // …and then the mapping itself, in the page: every limit the browser reports,
  // against the field `halLimitsFor` put it in. `gpu-replay.mjs` does this
  // against a stub with distinct numbers; this is the same table meeting a real
  // browser's. It is the first half of the pair — browser → mapped — and the
  // check after it is the second.
  const limitPairs = [
    ['maxImage2d', 'maxTextureDimension2D'],
    ['maxImage3d', 'maxTextureDimension3D'],
    ['maxImageArrayLayers', 'maxTextureArrayLayers'],
    ['maxStorageBufferRange', 'maxStorageBufferBindingSize'],
    ['maxUniformBufferRange', 'maxUniformBufferBindingSize'],
    ['maxBindGroups', 'maxBindGroups'],
    ['maxColorAttachments', 'maxColorAttachments'],
    ['maxComputeInvocationsPerWorkgroup', 'maxComputeInvocationsPerWorkgroup'],
    ['maxComputeWorkgroupsPerDimension', 'maxComputeWorkgroupsPerDimension'],
    ['minUniformBufferOffsetAlignment', 'minUniformBufferOffsetAlignment'],
    ['minStorageBufferOffsetAlignment', 'minStorageBufferOffsetAlignment'],
  ];
  const mismatched = limitPairs
    .filter(([seam, webgpu]) => live?.mapped?.[seam] !== live?.limits?.[webgpu])
    .map(
      ([seam, webgpu]) =>
        `${seam}=${live?.mapped?.[seam]} but ${webgpu}=${live?.limits?.[webgpu]}`
    );
  const workgroup = live?.mapped?.maxComputeWorkgroupSize ?? [];
  if (
    workgroup[0] !== live?.limits?.maxComputeWorkgroupSizeX ||
    workgroup[1] !== live?.limits?.maxComputeWorkgroupSizeY ||
    workgroup[2] !== live?.limits?.maxComputeWorkgroupSizeZ
  ) {
    mismatched.push(`maxComputeWorkgroupSize=[${workgroup.join(', ')}]`);
  }
  check(
    'G',
    'every limit the browser reports lands in the seam field that names it',
    mismatched.length === 0 && workgroup.length === 3,
    mismatched.length
      ? mismatched.join('; ')
      : `${limitPairs.length + 1} mapped, on maxTextureDimension2D ${live?.limits?.maxTextureDimension2D}`
  );

  // **AND THE SECOND HALF: MAPPED → WASM.** The check above says the mapping put
  // each of the browser's limits in the field that names it; this one says the
  // numbers that mapping produced are the numbers the module ended up holding,
  // after the reply encoder wrote them and `crcbl-webgpu` decoded them back into
  // a `crcbl_hal::Limits`. Between the two, every scalar of that struct is held
  // against this browser end to end.
  //
  // **Which is what only `maxImage2d` used to be.** One export carried one
  // limit, so a decode that transposed two fields — `max_image_3d` receiving
  // `max_image_array_layers` — agreed with the browser on the one number that
  // crossed and was invisible on every one that did not.
  //
  // The expected side is `live.mapped` rather than `live.limits`: this half is
  // about the wire, and holding it against the browser directly would fail here
  // for a mapping fault the check above already names better.
  const wasmLimits = wasmCaps?.limits ?? {};
  const mappedLimits = live?.mapped ?? {};
  const mappedFields = Object.keys(mappedLimits);
  /** One limit beside another, `maxComputeWorkgroupSize`'s array included. */
  const sameLimit = (got, want) =>
    Array.isArray(want)
      ? Array.isArray(got) &&
        got.length === want.length &&
        got.every((value, index) => value === want[index])
      : got === want;
  const show = (value) =>
    Array.isArray(value) ? `[${value.join(', ')}]` : String(value);
  /**
   * How many scalars a mapped `Limits` is, `maxComputeWorkgroupSize`'s three
   * included — which is why this counts values rather than keys. Both halves
   * of this pair hold it against wasm's own `limitCount`.
   */
  const countScalars = (limits) =>
    Object.values(limits).reduce(
      (total, value) => total + (Array.isArray(value) ? value.length : 1),
      0
    );
  const wrongLimits = mappedFields
    .filter((field) => !sameLimit(wasmLimits[field], mappedLimits[field]))
    .map(
      (field) =>
        `${field}: wasm has ${show(wasmLimits[field])}, the page mapped ${show(mappedLimits[field])}`
    );
  // **THE ARITY IS PART OF THE CLAIM.** `limitCount` is wasm's own count of the
  // scalars it flattened `Limits` into, and the loop above walks the fields the
  // page mapped — so a seam that grew a limit neither side's table knows about
  // is a shorter walk that would otherwise pass on the fields it did reach.
  const mappedScalars = countScalars(mappedLimits);
  if (wasmCaps?.limitCount !== mappedScalars) {
    wrongLimits.unshift(
      `wasm reports ${wasmCaps?.limitCount} limit scalars, the page mapped ${mappedScalars}`
    );
  }
  if (mappedFields.length === 0) {
    wrongLimits.unshift(
      'the page mapped no limits at all, so nothing was compared'
    );
  }
  check(
    'G',
    'every limit the page mapped is the number wasm ended up holding',
    wrongLimits.length === 0,
    wrongLimits.length
      ? wrongLimits.join('; ')
      : `${mappedScalars} scalars agree, on maxImage3d ${show(wasmLimits.maxImage3d)}` +
          ` and maxComputeWorkgroupSize ${show(wasmLimits.maxComputeWorkgroupSize)}`
  );

  // **AND THEN A DEVICE OPENS.** The enumeration proves a command crossed and an
  // answer came back; this proves the answer was *used*. wasm encodes a
  // `RequestDevice` naming the adapter it was just granted, the replayer turns
  // its `crcbl_hal::Features` words back into `GPUFeatureName`s, `requestDevice`
  // resolves, and the device's own capabilities come home. Nothing without a
  // browser can reach any of it: `gpu-replay.mjs` drives the same code against a
  // stub that is not WebGPU.
  const deviceProbe = await evaluate(
    page,
    `(async () => {
     const { startDeviceProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startDeviceProbe({ exports }),
       replayed: before.replayed,
       delivered: before.delivered,
     };
   })()`
  );
  const deviceCarried = deviceProbe?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const stats = globalThis.crcbl.gpu.stats();
           return stats.replayed > ${deviceProbe.replayed} || stats.failure
             ? stats
             : null;
         })()`
        )
      )
    : null;
  check(
    'G',
    'wasm encoded a device request and the page loop replayed it',
    deviceCarried?.commands?.join(',') === 'RequestDevice',
    deviceCarried
      ? `the loop replayed [${deviceCarried.commands.join(', ')}] at sequence ${deviceCarried.baseSequence}` +
          `${deviceCarried.failure ? ` — ${deviceCarried.failure}` : ''}`
      : deviceProbe?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not ask — no adapter had been granted, or the waiting set is full'
  );

  const opened = await until(async () =>
    evaluate(
      page,
      `(async () => {
       const { readDeviceProbe, DEVICE } = await import('/engine/gpu-probe.js');
       const { exports, memory, gpu } = globalThis.crcbl;
       const out = readDeviceProbe({ exports, memory });
       return out.state === DEVICE.WAITING
         ? null
         : { ...out, delivered: gpu.stats().delivered };
     })()`
    )
  );
  check(
    'G',
    'the browser opened a device and wasm read its capabilities back',
    opened?.name === 'OPENED' &&
      opened.delivered > (deviceProbe?.delivered ?? 0),
    opened
      ? `${opened.name}${opened.reason ? `: ${JSON.stringify(opened.reason)}` : ''}` +
          `, after the loop delivered ${opened.delivered - (deviceProbe?.delivered ?? 0)} reply buffer(s)`
      : `no answer in ${TIMEOUT_MS} ms — requestDevice never settled, or the reply never reached wasm`
  );

  // **What the page can see for itself.** A device opened here, in this browser,
  // with the descriptor the probe uses — `timestamp-query` where the adapter has
  // it and nothing else optional, and every limit the adapter reports, which is
  // what `requiredLimitsFor` asks for — is the same device WebGPU gives the
  // replayer, so its own `features` and `limits` are what wasm's numbers are held
  // to. Asked of the browser rather than of anything of ours, which is what makes
  // it evidence. The descriptor is written out here rather than imported from the
  // engine: a reference device opened by the code under test would agree with it
  // whatever it asked for.
  //
  // The optional features are in the request because `probe_device_desc` asks
  // for them — group AF has nothing to observe without a `'timestamp'` query
  // set, group AH nothing without a pipeline WebGPU will accept `unclippedDepth`
  // on, group AI nothing without an indirect draw WebGPU will accept a non-zero
  // `firstInstance` in, and group AK nothing without a texture WebGPU will let
  // it create in a BC format at all. Optional on both sides: a browser without
  // any of them opens a device here and there, and the two still match. **A name
  // missing from this list is a red group G on any machine whose adapter has
  // it**, because wasm's device would carry a feature this reference device
  // does not.
  const liveDevice = await evaluate(
    page,
    `(async () => {
     const { halLimitsFor } = await import('/engine/gpu-replay.js');
     const adapter = await navigator.gpu.requestAdapter();
     const requiredLimits = {};
     for (const key in adapter.limits) {
       const value = adapter.limits[key];
       if (typeof value === 'number' && Number.isFinite(value)) {
         requiredLimits[key] = value;
       }
     }
     const requiredFeatures = [
       'timestamp-query',
       'depth-clip-control',
       'indirect-first-instance',
       'texture-compression-bc',
     ].filter((name) => adapter.features.has(name));
     const device = await adapter.requestDevice({ requiredLimits, requiredFeatures });
     return {
       features: [...device.features].sort(),
       maxTextureDimension2D: device.limits.maxTextureDimension2D,
       adapterMaxTextureDimension2D: adapter.limits.maxTextureDimension2D,
       // Mapped off the GPUDevice and never off the adapter it came from,
       // which is the distinction the check below exists to hold. BigInts do
       // not survive the wire back to this driver, as above.
       mapped: Object.fromEntries(
         Object.entries(halLimitsFor(device)).map(([key, value]) =>
           [key, typeof value === 'bigint' ? Number(value) : value])
       ),
     };
   })()`
  );
  const deviceCaps = opened?.caps;
  const expectedDeviceFeatures = featureBitsOf(liveDevice?.features);
  check(
    'G',
    "the device's feature set wasm received is what this browser's own device reports",
    deviceCaps?.featuresLo === expectedDeviceFeatures &&
      deviceCaps?.featuresHi === 0,
    `wasm got ${deviceCaps?.featuresLo?.toString(16)}/${deviceCaps?.featuresHi?.toString(16)},` +
      ` ${expectedDeviceFeatures.toString(16)} expected from [${(liveDevice?.features ?? []).join(', ')}]`
  );
  check(
    'G',
    "a limit wasm received for the device is the number this browser's device reports",
    deviceCaps?.maxImage2d === liveDevice?.maxTextureDimension2D &&
      deviceCaps?.maxImage2d > 0,
    `wasm got ${deviceCaps?.maxImage2d}, the device says` +
      ` maxTextureDimension2D ${liveDevice?.maxTextureDimension2D}`
  );

  // **AND THE SAME PAIR AGAIN, FOR THE DEVICE.** The check above holds one of
  // the device's limits against a device; this holds every one of them — what
  // the page maps off the `GPUDevice` it opened for itself, against what wasm
  // ended up holding once the reply encoder wrote the device's `Limits` and
  // `crcbl-webgpu` decoded them back into one.
  //
  // **The device had the blind spot the adapter used to have.**
  // `__crcbl_web_gpu_probe_device_max_image_2d` was the only export carrying one
  // of its limits, so a decode that transposed two fields on the *device* reply
  // agreed with the browser on the one number that crossed and was invisible on
  // every one that did not. The adapter's table cannot see that: `Reply::Device`
  // is a different record, decoded separately.
  //
  // Held against `liveDevice.mapped` rather than against `device.limits`
  // directly, for the reason the adapter's half gives: this is the wire's half
  // of the pair, and a mapping fault belongs to the check that names it.
  const deviceWasmLimits = deviceCaps?.limits ?? {};
  const deviceMappedLimits = liveDevice?.mapped ?? {};
  const deviceMappedFields = Object.keys(deviceMappedLimits);
  const wrongDeviceLimits = deviceMappedFields
    .filter(
      (field) => !sameLimit(deviceWasmLimits[field], deviceMappedLimits[field])
    )
    .map(
      (field) =>
        `${field}: wasm has ${show(deviceWasmLimits[field])}, the page mapped ${show(deviceMappedLimits[field])}`
    );
  // The arity is part of this claim too, on the adapter half's terms and for
  // its reason: a walk over fewer fields than exist would otherwise pass on the
  // ones it did reach.
  const deviceMappedScalars = countScalars(deviceMappedLimits);
  if (deviceCaps?.limitCount !== deviceMappedScalars) {
    wrongDeviceLimits.unshift(
      `wasm reports ${deviceCaps?.limitCount} limit scalars for the device, the page mapped ${deviceMappedScalars}`
    );
  }
  if (deviceMappedFields.length === 0) {
    wrongDeviceLimits.unshift(
      "the page mapped none of its own device's limits, so nothing was compared"
    );
  }
  check(
    'G',
    'every limit the page mapped off its own device is the number wasm holds for the device',
    wrongDeviceLimits.length === 0,
    wrongDeviceLimits.length
      ? wrongDeviceLimits.join('; ')
      : `${deviceMappedScalars} scalars agree, on maxImage3d ${show(deviceWasmLimits.maxImage3d)}` +
          ` and maxComputeWorkgroupSize ${show(deviceWasmLimits.maxComputeWorkgroupSize)}`
  );

  // **The device's capabilities are not the adapter's**, which is the claim a
  // backend gets wrong for free: the adapter is right there when the reply is
  // built. WebGPU grants a device what was asked for, and the probe asks for
  // nothing optional — so on any adapter reporting an optional feature the two
  // records differ, and wasm's two sets of numbers have to differ with them.
  //
  // **The limits can no longer carry this**, and saying so is the point of
  // this paragraph. The replayer asks for every limit the adapter reports, so
  // the device's are the adapter's by construction and a copy would be
  // indistinguishable there; the features are the axis that still separates
  // them. Where an adapter reports nothing optional either, the two records
  // legitimately coincide and this says so rather than claiming a distinction
  // it could not observe. The checks above still hold the device's numbers to
  // a device either way.
  const theAdapterHasOptionalFeatures =
    expectedFeatures !== expectedDeviceFeatures;
  const wasmSaysTheyDiffer = wasmCaps?.featuresLo !== deviceCaps?.featuresLo;
  check(
    'G',
    'the device wasm was told about is a device, not a copy of its adapter',
    theAdapterHasOptionalFeatures ? wasmSaysTheyDiffer : !wasmSaysTheyDiffer,
    theAdapterHasOptionalFeatures
      ? `adapter ${wasmCaps?.featuresLo?.toString(16)} against device ${deviceCaps?.featuresLo?.toString(16)}` +
          ` (both report maxImage2d ${deviceCaps?.maxImage2d}, which is the adapter's ceiling and asked for)`
      : 'this adapter reports nothing optional, so the two feature words ' +
          'genuinely coincide here and a copy could not be told apart'
  );

  // **THE ONLY GATE ON A SURFACE.** `gpu-replay.mjs` drives the same two
  // commands under node against a stub canvas whose `getContext` returns a plain
  // object, so what it proves is the bookkeeping: that the `canvasId` on the
  // wire is the key that gets looked up, and that a destroy lets go. What it
  // cannot reach is the only thing a surface finally is — `getContext('webgpu')`
  // on a real element, answering a real `GPUCanvasContext`. Nothing else runs
  // that, and the registry it is resolved against is the page's own.
  //
  // Polled rather than read straight back, for group G's reason and no other:
  // `CreateSurface` still makes no round trip — wasm names the handle and moves
  // on — but the replay is the page loop's, so what there is to see appears on
  // the loop's next frame rather than in this call. See
  // `crates/crcbl-webgpu/src/probe.rs`.
  group('H — a surface resolves to a real canvas context');

  const surfaceStart = await evaluate(
    page,
    `(async () => {
     const { startSurfaceProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startSurfaceProbe({ exports, canvasId: gpu.canvasId }),
       replayed: before.replayed,
       canvasId: gpu.canvasId,
     };
   })()`
  );
  // A `SurfaceError` out of the replay is the page failing to resolve the
  // canvas, and it is the one failure this group exists to catch. It is thrown
  // in the page loop now, which latches it and reports it as `stats().failure`
  // — so it settles this poll and lands as a red check naming what threw,
  // rather than either aborting the run or timing out with nothing to say.
  const surfaceProbe = surfaceStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const { gpu } = globalThis.crcbl;
           const stats = gpu.stats();
           if (stats.replayed <= ${surfaceStart.replayed} && !stats.failure) {
             return null;
           }
           const entries = [...gpu.replayer.surfaces.entries()];
           const last = entries[entries.length - 1];
           const surface = last ? last[0] : null;
           const context = last ? last[1] : undefined;
           return {
             commands: stats.commands,
             failure: stats.failure,
             surface,
             held: entries.length,
             // Identity, not existence. \`GPUCanvasContext.canvas\` is the
             // element the context came out of, so this is the only thing
             // that separates "the right canvas answered" from "a canvas
             // answered". Against the page's own element rather than against
             // whatever the registry holds, so a registry pointing somewhere
             // else is a failure rather than a tautology.
             isTheCanvas: context?.canvas === document.getElementById('canvas'),
             isTheDecoy: context?.canvas === globalThis.crcblProbe.decoy,
             // …and that it is the browser's own class rather than something
             // shaped like it. Node has no \`GPUCanvasContext\` binding at all,
             // so this is what a silent fall back to a stub cannot survive.
             isRealContext:
               typeof GPUCanvasContext === 'function' &&
               context instanceof GPUCanvasContext,
             hasCurrentTexture: typeof context?.getCurrentTexture === 'function',
           };
         })()`
        )
      )
    : null;
  check(
    'H',
    'wasm encoded a surface creation and the page loop replayed it',
    surfaceProbe?.commands?.join(',') === 'CreateSurface' &&
      Number.isInteger(surfaceProbe.surface),
    surfaceProbe
      ? `the loop replayed [${surfaceProbe.commands.join(', ')}] for surface ${surfaceProbe.surface}` +
          `${surfaceProbe.failure ? ` — ${surfaceProbe.failure}` : ''}`
      : surfaceStart?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not encode it — another channel is installed'
  );
  check(
    'H',
    'the surface resolved the canvas the page registered and not the decoy',
    surfaceProbe?.isTheCanvas === true && surfaceProbe?.isTheDecoy === false,
    surfaceProbe?.isTheCanvas
      ? `surface ${surfaceProbe.surface} holds the context of canvas ${surfaceStart?.canvasId}, of ${surfaceProbe.held} held`
      : `the context under surface ${surfaceProbe?.surface} belongs to ` +
          `${surfaceProbe?.isTheDecoy ? `the decoy at ${DECOY_CANVAS_ID}` : 'no canvas this page registered'}` +
          `${surfaceProbe?.failure ? ` — ${surfaceProbe.failure}` : ''}`
  );
  check(
    'H',
    'a real GPUCanvasContext came from that canvas, not a stub of one',
    surfaceProbe?.isRealContext === true &&
      surfaceProbe?.hasCurrentTexture === true,
    surfaceProbe?.isRealContext
      ? 'the context is an instance of this browser GPUCanvasContext and has getCurrentTexture'
      : `instanceof GPUCanvasContext: ${surfaceProbe?.isRealContext}, ` +
          `getCurrentTexture: ${surfaceProbe?.hasCurrentTexture}` +
          `${surfaceProbe?.failure ? ` — ${surfaceProbe.failure}` : ''}`
  );

  // **THE ONLY GATE ON A CAPABILITY QUERY.** `gpu-replay.mjs` drives the same
  // command under node against a stub whose `getPreferredCanvasFormat` returns
  // whatever that file wrote a line above, so what it proves is the encoding and
  // the translation. The fact left over is the one only a browser has: **what
  // this machine's WebGPU actually prefers to put on a canvas**, which varies by
  // browser and by platform and which no fixture can contain.
  //
  // So the check below asks the page for that answer directly, at the same
  // moment, and holds wasm's number to it. A replayer that answered with a
  // constant, or a decoder that read the format list off by one, differs there
  // and nowhere else in this file.
  //
  // **There is no refusal check here, and there is nothing to replace it with.**
  // This group used to drive a surface handle nothing created and expect
  // `InvalidHandle` back. `Command::SurfaceCaps` carries no ids now — the record
  // depends on neither, so an `impl Instance` refuses a stale handle against its
  // own tables without a round trip — and the only cause left, `Backend`, is the
  // browser naming a canvas format this seam has no `Format` for, which no page
  // can be made to do. The refusal path is a `cargo test` and a `gpu-replay.mjs`
  // check; the reply channel carrying one is not a browser fact any more.
  //
  // Nothing here drains, replays or delivers, for group G's reason.
  group('I — a surface says what it will accept');

  const capsStart = await evaluate(
    page,
    `(async () => {
     const { startSurfaceCapsProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startSurfaceCapsProbe({ exports }),
       replayed: before.replayed,
       delivered: before.delivered,
     };
   })()`
  );
  const capsCarried = capsStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const stats = globalThis.crcbl.gpu.stats();
           return stats.replayed > ${capsStart.replayed} || stats.failure
             ? stats
             : null;
         })()`
        )
      )
    : null;
  check(
    'I',
    'wasm encoded a surface-capability query and the page loop replayed it',
    capsCarried?.commands?.join(',') === 'SurfaceCaps',
    capsCarried
      ? `the loop replayed [${capsCarried.commands.join(', ')}] at sequence ${capsCarried.baseSequence}` +
          `${capsCarried.failure ? ` — ${capsCarried.failure}` : ''}`
      : capsStart?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not encode it — another channel is installed, or the waiting set is full'
  );

  // Polled like group G's answers, and for a weaker version of the same reason:
  // this command is answered inside the replay rather than out of a promise, so
  // it settles on the frame the loop replays it — which is still not the frame
  // that asked.
  const capsAnswered = await until(async () =>
    evaluate(
      page,
      `(async () => {
       const { readSurfaceCapsProbe, CAPS } = await import('/engine/gpu-probe.js');
       const { exports, memory, gpu } = globalThis.crcbl;
       const out = readSurfaceCapsProbe({ exports, memory });
       return out.state === CAPS.WAITING
         ? null
         : { ...out, delivered: gpu.stats().delivered };
     })()`
    )
  );

  // Asked of the browser, in the same page, at the same moment — the half that
  // makes the next check a round trip rather than a restatement.
  const preferredFormat = await evaluate(
    page,
    `navigator.gpu.getPreferredCanvasFormat()`
  );
  const expectedFormat = SEAM_FORMAT_CODE[preferredFormat];
  check(
    'I',
    'the preferred format wasm received is the one this browser prefers',
    capsAnswered?.name === 'ANSWERED' &&
      expectedFormat !== undefined &&
      capsAnswered.format === expectedFormat.srgb,
    capsAnswered?.name === 'ANSWERED'
      ? `wasm got format code ${capsAnswered.format}, navigator.gpu prefers ` +
          `${JSON.stringify(preferredFormat)} whose sRGB counterpart is code ${expectedFormat?.srgb}` +
          (capsAnswered.format === expectedFormat?.linear
            ? ' — the LINEAR code, so nothing sRGB was offered and every frame ' +
              'this canvas presents is a transfer function too dark'
            : '') +
          `, after the loop delivered ${capsAnswered.delivered - (capsStart?.delivered ?? 0)} reply buffer(s)`
      : capsAnswered
        ? `${capsAnswered.name}: ${JSON.stringify(capsAnswered.reason)}`
        : `no answer in ${TIMEOUT_MS} ms — the reply never reached wasm`
  );

  // The two promises the rest of the record carries, and the reason they are
  // here beside a format that would decode correctly whatever happened to the
  // lists behind it: an empty mode list, or an extent that appeared from
  // nowhere, is a reader that lost its place after the first field. Both are
  // invariants rather than counts — `SurfaceCaps` promises Fifo is always
  // offered, and a browser has no `currentExtent` to report at all — so neither
  // asserts a number the replayer chose.
  check(
    'I',
    'the surface offers Fifo and reports no extent of its own',
    capsAnswered?.name === 'ANSWERED' &&
      (capsAnswered.presentModes & FIFO_BIT) === FIFO_BIT &&
      capsAnswered.hasExtent === false,
    capsAnswered?.name === 'ANSWERED'
      ? `present modes 0b${capsAnswered.presentModes.toString(2)}, currentExtent ` +
          `${capsAnswered.hasExtent ? 'present' : 'absent'}`
      : `state ${capsAnswered?.name}, which carries no capabilities to read`
  );

  // **THE ONLY GATE ON A RESOURCE ACTUALLY BEING MADE.** Everything above this
  // asks the browser questions — what it has, what it prefers, what it will
  // grant. This is the first command that tells it to *do* something and keeps
  // what came back: a `crcbl_hal::BufferDesc` encoded in wasm, replayed through
  // the page's own loop, and a real `GPUBuffer` on the real device at the end of
  // it.
  //
  // `gpu-replay.mjs` drives the same two commands under node against a stub
  // device whose `createBuffer` hands back a plain object built from the
  // descriptor — so what it proves is the translation and the bookkeeping, and
  // it would pass just as well against a replayer that never called a browser at
  // all. What only a browser can establish is what is checked here: that
  // `createBuffer` accepted the descriptor this seam builds, and that the object
  // it answered reports the size, the label and the usage that were asked for.
  // `GPUBuffer.usage` is the interesting one — it is the browser reading back a
  // word two seam fields were folded into.
  group('J — a buffer is created on the real device');

  const bufferStart = await evaluate(
    page,
    `(async () => {
     const { startBufferProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startBufferProbe({ exports, size: ${PROBE_BUFFER_BYTES} }),
       replayed: before.replayed,
     };
   })()`
  );
  const bufferProbe = bufferStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const { gpu } = globalThis.crcbl;
           const stats = gpu.stats();
           if (stats.replayed <= ${bufferStart.replayed} && !stats.failure) {
             return null;
           }
           const entries = [...gpu.replayer.buffers.entries()];
           const last = entries[entries.length - 1];
           const buffer = last ? last[1] : undefined;
           return {
             commands: stats.commands,
             failure: stats.failure,
             handle: last ? last[0] : null,
             held: entries.length,
             // The browser's own class, as group H asks of a canvas context.
             // Node has no \`GPUBuffer\` binding at all, so this is what a
             // silent fall back to a stub cannot survive.
             isRealBuffer:
               typeof GPUBuffer === 'function' && buffer instanceof GPUBuffer,
             size: buffer?.size,
             label: buffer?.label,
             usage: buffer?.usage,
             // Built from the browser's own namespace object rather than from
             // the seam's table, which is the whole point: STORAGE and
             // TRANSFER_DST are what \`probe_buffer_desc\` asks for, and these
             // are the bits this browser calls them.
             expectedUsage:
               GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
             // A creation that failed has no reply to arrive in, so the reason
             // is queued where \`Device::take_error\` will drain it. Read
             // whatever the outcome, because it is the only thing that can say
             // why a buffer is missing.
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'J',
    'wasm encoded a buffer creation and the page loop replayed it',
    bufferProbe?.commands?.join(',') === 'CreateBuffer' &&
      Number.isInteger(bufferProbe.handle),
    bufferProbe
      ? `the loop replayed [${bufferProbe.commands.join(', ')}] for buffer ${bufferProbe.handle}` +
          `${bufferProbe.failure ? ` — ${bufferProbe.failure}` : ''}` +
          `${bufferProbe.error ? ` — ${bufferProbe.error}` : ''}`
      : bufferStart?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'J',
    'a real GPUBuffer came back from the device with the size that was asked for',
    bufferProbe?.isRealBuffer === true &&
      bufferProbe?.size === PROBE_BUFFER_BYTES,
    bufferProbe?.isRealBuffer
      ? `an instance of this browser's GPUBuffer, of ${bufferProbe.size} bytes, of ${bufferProbe.held} held`
      : `instanceof GPUBuffer: ${bufferProbe?.isRealBuffer}, size ${bufferProbe?.size}` +
          ` (${PROBE_BUFFER_BYTES} asked for)` +
          `${bufferProbe?.error ? ` — ${bufferProbe.error}` : ''}`
  );
  check(
    'J',
    'the browser gave that buffer the usage and the label the seam asked for',
    bufferProbe?.usage === bufferProbe?.expectedUsage &&
      bufferProbe?.label === PROBE_BUFFER_LABEL,
    `usage 0x${bufferProbe?.usage?.toString(16)} against 0x${bufferProbe?.expectedUsage?.toString(16)}` +
      ` from GPUBufferUsage, label ${JSON.stringify(bufferProbe?.label)}`
  );

  // **THE ONLY GATE ON AN IMAGE, AND ON A RESOURCE MADE FROM ANOTHER ONE.**
  // Group J watches this seam make something on the device; this watches it make
  // something on *that*, which is the shape no command before it has: a
  // `GPUTextureView` comes from the texture and not from the device, so the
  // image handle on the wire has to resolve to a live `GPUTexture` in the
  // replayer's own table before there is anything to call.
  //
  // `gpu-replay.mjs` drives both commands under node against a stub device whose
  // `createTexture` hands back a plain object built from the descriptor, so what
  // it proves is the translation and the bookkeeping — every table row, every
  // refusal, and that the range's sentinel reaches `createView` as an absent
  // member rather than as `4294967295`. What only a browser can establish is
  // that a real `createTexture` accepted the descriptor this seam builds, that
  // the `GPUTexture` it answered reports the extent, format, mip count, usage
  // and label that were asked for, and that a real `createView` accepted the
  // resolved range — which the number on the wire would have been refused for.
  group('K — an image and a view of it are created on the real device');

  const imageStart = await evaluate(
    page,
    `(async () => {
     const { startImageProbe, startImageViewProbe } =
       await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     // Both on one frame, deliberately: the view names the image the command
     // before it created, so replaying them together is what says the
     // replayer's table is filled in before the lookup rather than a frame
     // later.
     const image = startImageProbe({
       exports,
       width: ${PROBE_IMAGE_WIDTH},
       height: ${PROBE_IMAGE_HEIGHT},
       mipLevels: ${PROBE_IMAGE_MIPS},
     });
     return {
       started: image && startImageViewProbe({ exports }),
       replayed: before.replayed,
     };
   })()`
  );
  const imageProbe = imageStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const { gpu } = globalThis.crcbl;
           const stats = gpu.stats();
           if (stats.replayed <= ${imageStart.replayed} && !stats.failure) {
             return null;
           }
           const images = [...gpu.replayer.images.entries()];
           const views = [...gpu.replayer.imageViews.entries()];
           const image = images.length ? images[images.length - 1][1] : undefined;
           const view = views.length ? views[views.length - 1][1] : undefined;
           return {
             commands: stats.commands,
             failure: stats.failure,
             imageHandle: images.length ? images[images.length - 1][0] : null,
             viewHandle: views.length ? views[views.length - 1][0] : null,
             heldImages: images.length,
             heldViews: views.length,
             // The browser's own classes, as groups H and J ask. Node has no
             // \`GPUTexture\` or \`GPUTextureView\` binding at all, so this is
             // what a silent fall back to a stub cannot survive.
             isRealTexture:
               typeof GPUTexture === 'function' && image instanceof GPUTexture,
             isRealView:
               typeof GPUTextureView === 'function' &&
               view instanceof GPUTextureView,
             width: image?.width,
             height: image?.height,
             depthOrArrayLayers: image?.depthOrArrayLayers,
             mipLevelCount: image?.mipLevelCount,
             dimension: image?.dimension,
             format: image?.format,
             usage: image?.usage,
             label: image?.label,
             viewLabel: view?.label,
             // Built from the browser's own namespace object rather than from
             // the seam's table, as group J does: SAMPLED and TRANSFER_DST are
             // what \`probe_image_desc\` asks for, and these are the bits this
             // browser calls them.
             expectedUsage:
               GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
           };
         })()`
        )
      )
    : null;

  // **THE ONLY THING THAT CAN SAY THE VIEW'S RANGE WAS ACCEPTED**, and it is
  // read a moment after the replay rather than during it. A `GPUTextureView`
  // reports nothing but its label, so one built from a range the browser refused
  // is indistinguishable from a good one by inspection — the refusal arrives on
  // the *device's* error channel instead, which is the queue
  // `Device::take_error` drains and which `gpu-replay.js` feeds from its
  // `uncapturederror` listener.
  //
  // WebGPU raises that error "in a future task", so reading the queue in the
  // same evaluation that saw the replay finish reads it too early and finds an
  // empty queue whatever happened — which is a check that cannot fail. Two
  // animation frames inside the page is the wait, and it is a wait for an
  // *absence*: there is no event that says "no error is coming", so a bounded
  // one is the only shape available. Measured against the failure it guards —
  // `ImageSubresourceRange::ALL` passed on as `4294967295`, which Chromium
  // refuses — this is enough and reading immediately is not.
  const deviceReport = imageProbe
    ? await evaluate(
        page,
        `(async () => {
         await new Promise((settle) =>
           requestAnimationFrame(() => requestAnimationFrame(settle))
         );
         // Wrapped rather than returned bare, so that an evaluation that
         // never ran is distinguishable from a queue that was empty. Both
         // would arrive here as \`null\`, and one of them is a check that
         // cannot fail.
         return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
       })()`
      )
    : null;
  check(
    'K',
    'wasm encoded an image and a view of it, and the page loop replayed both',
    imageProbe?.commands?.join(',') === 'CreateImage,CreateImageView' &&
      Number.isInteger(imageProbe.imageHandle) &&
      Number.isInteger(imageProbe.viewHandle),
    imageProbe
      ? `the loop replayed [${imageProbe.commands.join(', ')}] for image ${imageProbe.imageHandle} and view ${imageProbe.viewHandle}` +
          `${imageProbe.failure ? ` — ${imageProbe.failure}` : ''}`
      : imageStart?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not encode them — no device has opened, or another channel is installed'
  );
  check(
    'K',
    'a real GPUTexture came back from the device with the extent, format and mip count asked for',
    imageProbe?.isRealTexture === true &&
      imageProbe?.width === PROBE_IMAGE_WIDTH &&
      imageProbe?.height === PROBE_IMAGE_HEIGHT &&
      imageProbe?.depthOrArrayLayers === 1 &&
      imageProbe?.mipLevelCount === PROBE_IMAGE_MIPS &&
      imageProbe?.dimension === '2d' &&
      imageProbe?.format === 'rgba8unorm',
    imageProbe?.isRealTexture
      ? `an instance of this browser's GPUTexture, ${imageProbe.width}x${imageProbe.height}x${imageProbe.depthOrArrayLayers} ${imageProbe.dimension} ${imageProbe.format}` +
          ` with ${imageProbe.mipLevelCount} mips, of ${imageProbe.heldImages} held`
      : `instanceof GPUTexture: ${imageProbe?.isRealTexture}, ${imageProbe?.width}x${imageProbe?.height}` +
          ` ${imageProbe?.dimension} ${imageProbe?.format}, ${imageProbe?.mipLevelCount} mips` +
          ` (${PROBE_IMAGE_WIDTH}x${PROBE_IMAGE_HEIGHT} 2d rgba8unorm, ${PROBE_IMAGE_MIPS} mips asked for)`
  );
  check(
    'K',
    'the browser gave that texture the usage and the label the seam asked for',
    imageProbe?.usage === imageProbe?.expectedUsage &&
      imageProbe?.label === PROBE_IMAGE_LABEL,
    `usage 0x${imageProbe?.usage?.toString(16)} against 0x${imageProbe?.expectedUsage?.toString(16)}` +
      ` from GPUTextureUsage, label ${JSON.stringify(imageProbe?.label)}`
  );
  check(
    'K',
    'a real GPUTextureView came back from that texture with the whole-image range accepted',
    imageProbe?.isRealView === true &&
      imageProbe?.viewLabel === PROBE_VIEW_LABEL &&
      deviceReport?.waited === true &&
      deviceReport?.error === null,
    deviceReport?.waited !== true
      ? 'the page never got as far as reading the device error queue'
      : imageProbe?.isRealView && deviceReport.error === null
        ? `an instance of this browser's GPUTextureView labelled ${JSON.stringify(imageProbe.viewLabel)}, of ${imageProbe.heldViews} held, and the device reported nothing`
        : `instanceof GPUTextureView: ${imageProbe?.isRealView}, label ${JSON.stringify(imageProbe?.viewLabel)}` +
          `${deviceReport.error ? ` — ${deviceReport.error}` : ''}`
  );

  // **THE ONLY GATE ON A SAMPLER, AND THE ONE WHERE THE OBJECT SAYS LEAST.**
  // Group J watches this seam make something the browser then describes back —
  // a `GPUBuffer` reports its size, usage and label — and group K watches it
  // make something that describes back nine members. A `GPUSampler` reports its
  // `label` and nothing else: no filters, no address modes, no clamps. So there
  // is no "pass a number in and read it back" check available here, and the two
  // things that are:
  //
  //   * the class of what came back, which `instanceof GPUSampler` settles and
  //     no stub can satisfy — node has no such binding at all; and
  //   * the device's error queue being empty afterwards, which is the only thing
  //     anywhere that can say `createSampler` *accepted* the descriptor this
  //     seam built.
  //
  // That second one is what this group exists for, because the descriptor
  // carries `lod_max: f32::MAX` — `SamplerDesc::default`'s "no limit" sentinel.
  // It crosses the wire verbatim, and the replayer has to hand WebGPU an
  // explicit `lodMaxClamp` holding it: omitting the member, which is how the
  // *view's* range sentinel is spelled one group earlier, would substitute
  // WebGPU's own default — a number rather than "the rest" — and nothing
  // downstream reports a mip clamp. `gpu-replay.mjs` proves the descriptor this
  // seam builds against a stub; only a real `createSampler` can say the browser
  // takes it.
  group('L — a sampler is created on the real device');

  const samplerStart = await evaluate(
    page,
    `(async () => {
     const { startSamplerProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startSamplerProbe({ exports }),
       replayed: before.replayed,
     };
   })()`
  );
  const samplerProbe = samplerStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const { gpu } = globalThis.crcbl;
           const stats = gpu.stats();
           if (stats.replayed <= ${samplerStart.replayed} && !stats.failure) {
             return null;
           }
           const entries = [...gpu.replayer.samplers.entries()];
           const last = entries[entries.length - 1];
           const sampler = last ? last[1] : undefined;
           return {
             commands: stats.commands,
             failure: stats.failure,
             handle: last ? last[0] : null,
             held: entries.length,
             // The browser's own class, as groups H, J and K ask. Node has no
             // \`GPUSampler\` binding at all, so this is what a silent fall
             // back to a stub cannot survive.
             isRealSampler:
               typeof GPUSampler === 'function' &&
               sampler instanceof GPUSampler,
             // Every other member of a \`GPUSampler\` is absent by design, so
             // this is the whole of what the object can be asked.
             label: sampler?.label,
           };
         })()`
        )
      )
    : null;

  // Read a moment after the replay rather than during it, for the reason group K
  // spells out: WebGPU raises a validation error "in a future task", so a queue
  // read in the evaluation that saw the replay finish is empty whatever
  // happened — a check that cannot fail. Two animation frames is the same
  // bounded wait for the same absence, and the failure it guards here is the one
  // this group exists for: `lod_max`'s sentinel reaching the browser as
  // something `createSampler` refuses.
  const samplerReport = samplerProbe
    ? await evaluate(
        page,
        `(async () => {
         await new Promise((settle) =>
           requestAnimationFrame(() => requestAnimationFrame(settle))
         );
         return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
       })()`
      )
    : null;
  check(
    'L',
    'wasm encoded a sampler creation and the page loop replayed it',
    samplerProbe?.commands?.join(',') === 'CreateSampler' &&
      Number.isInteger(samplerProbe.handle),
    samplerProbe
      ? `the loop replayed [${samplerProbe.commands.join(', ')}] for sampler ${samplerProbe.handle}` +
          `${samplerProbe.failure ? ` — ${samplerProbe.failure}` : ''}`
      : samplerStart?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'L',
    'a real GPUSampler came back from the device with the no-limit lod clamp accepted',
    samplerProbe?.isRealSampler === true &&
      samplerProbe?.label === PROBE_SAMPLER_LABEL &&
      samplerReport?.waited === true &&
      samplerReport?.error === null,
    samplerReport?.waited !== true
      ? 'the page never got as far as reading the device error queue'
      : samplerProbe?.isRealSampler && samplerReport.error === null
        ? `an instance of this browser's GPUSampler labelled ${JSON.stringify(samplerProbe.label)}, of ${samplerProbe.held} held, and the device reported nothing`
        : `instanceof GPUSampler: ${samplerProbe?.isRealSampler}, label ${JSON.stringify(samplerProbe?.label)}` +
          `${samplerReport.error ? ` — ${samplerReport.error}` : ''}`
  );

  // **THE ONLY GATE ON A LIST.** Every command groups G to L put in front of a
  // browser is a fixed set of fields; this one's body is a counted list of
  // structs, each five fields deep, each carrying an enum whose variants have
  // different-length payloads. A stride out by a byte therefore does not
  // truncate — it decodes the next entry out of the middle of this one and
  // produces a layout that is well-formed and describes different resources.
  //
  // A `GPUBindGroupLayout` reports its `label` and nothing else, exactly as a
  // `GPUSampler` does, so the two things available here are group L's two: the
  // class of what came back, which `instanceof GPUBindGroupLayout` settles and
  // no stub can satisfy — node has no such binding — and the device's error
  // queue being empty afterwards, which is the only thing anywhere that can say
  // `createBindGroupLayout` *accepted* the four-entry descriptor this seam
  // built. `gpu-replay.mjs` proves that descriptor against a stub, entry for
  // entry, and proves every refusal; only a real device can say the browser
  // takes it.
  group('M — a bind-group layout is created on the real device');

  const layoutStart = await evaluate(
    page,
    `(async () => {
     const { startBindGroupLayoutProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startBindGroupLayoutProbe({ exports }),
       replayed: before.replayed,
     };
   })()`
  );
  const layoutProbe = layoutStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const { gpu } = globalThis.crcbl;
           const stats = gpu.stats();
           if (stats.replayed <= ${layoutStart.replayed} && !stats.failure) {
             return null;
           }
           const entries = [...gpu.replayer.bindGroupLayouts.entries()];
           const last = entries[entries.length - 1];
           const layout = last ? last[1] : undefined;
           return {
             commands: stats.commands,
             failure: stats.failure,
             handle: last ? last[0] : null,
             held: entries.length,
             // The browser's own class, as groups H, J, K and L ask. Node has
             // no \`GPUBindGroupLayout\` binding at all, so this is what a
             // silent fall back to a stub cannot survive.
             isRealLayout:
               typeof GPUBindGroupLayout === 'function' &&
               layout instanceof GPUBindGroupLayout,
             // Every other member of a \`GPUBindGroupLayout\` is absent by
             // design, so this is the whole of what the object can be asked.
             label: layout?.label,
           };
         })()`
        )
      )
    : null;

  // Read a moment after the replay rather than during it, for the reason groups
  // K and L spell out: WebGPU raises a validation error "in a future task", so a
  // queue read in the evaluation that saw the replay finish is empty whatever
  // happened. The failure it guards here is the list: an entry's `visibility`
  // arriving as zero, a `texture` member missing its `sampleType`, a
  // `hasDynamicOffset` under a name WebIDL ignores — every one of those is a
  // layout the browser refuses and nothing else reports.
  const layoutReport = layoutProbe
    ? await evaluate(
        page,
        `(async () => {
         await new Promise((settle) =>
           requestAnimationFrame(() => requestAnimationFrame(settle))
         );
         return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
       })()`
      )
    : null;
  check(
    'M',
    'wasm encoded a bind-group layout creation and the page loop replayed it',
    layoutProbe?.commands?.join(',') === 'CreateBindGroupLayout' &&
      Number.isInteger(layoutProbe.handle),
    layoutProbe
      ? `the loop replayed [${layoutProbe.commands.join(', ')}] for layout ${layoutProbe.handle}` +
          `${layoutProbe.failure ? ` — ${layoutProbe.failure}` : ''}`
      : layoutStart?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'M',
    'a real GPUBindGroupLayout came back from the device with every entry accepted',
    layoutProbe?.isRealLayout === true &&
      layoutProbe?.label === PROBE_LAYOUT_LABEL &&
      layoutReport?.waited === true &&
      layoutReport?.error === null,
    layoutReport?.waited !== true
      ? 'the page never got as far as reading the device error queue'
      : layoutProbe?.isRealLayout && layoutReport.error === null
        ? `an instance of this browser's GPUBindGroupLayout labelled ${JSON.stringify(layoutProbe.label)}, built from ${PROBE_LAYOUT_ENTRIES} entries, of ${layoutProbe.held} held, and the device reported nothing`
        : `instanceof GPUBindGroupLayout: ${layoutProbe?.isRealLayout}, label ${JSON.stringify(layoutProbe?.label)}` +
          `${layoutReport.error ? ` — ${layoutReport.error}` : ''}`
  );

  // **THE ONLY GATE ON A COMMAND THAT NAMES OTHER RESOURCES.** Every command
  // groups G to M put in front of a browser stands alone; this one binds a
  // layout, a buffer, an image view and a sampler that have to exist first, so
  // wasm records a whole frame — the layout, the four resources, then the group —
  // and the group's entries carry one handle into each of three resource tables.
  // A handle carries no kind, so the entry's discriminant is the only thing that
  // says which table an id indexes, and the whole-buffer binding's size crosses as
  // the `u64::MAX` sentinel and has to reach WebGPU as an *absent* member.
  //
  // A `GPUBindGroup` reports its `label` and nothing else, exactly as a
  // `GPUBindGroupLayout` does, so the two things available here are group M's two:
  // the class of what came back, which `instanceof GPUBindGroup` settles and no
  // stub can satisfy — node has no such binding — and the device's error queue
  // being empty afterwards, which is the only thing that can say `createBindGroup`
  // *accepted* the descriptor, its `WHOLE_BUFFER` binding and all three resource
  // kinds. `gpu-replay.mjs` proves that descriptor against a stub and proves every
  // refusal; only a real device can say the browser takes it.
  group('N — a bind group is created on the real device');

  const bindGroupStart = await evaluate(
    page,
    `(async () => {
     const { startBindGroupProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startBindGroupProbe({ exports }),
       replayed: before.replayed,
     };
   })()`
  );
  const bindGroupProbe = bindGroupStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const { gpu } = globalThis.crcbl;
           const stats = gpu.stats();
           if (stats.replayed <= ${bindGroupStart.replayed} && !stats.failure) {
             return null;
           }
           const entries = [...gpu.replayer.bindGroups.entries()];
           const last = entries[entries.length - 1];
           const group = last ? last[1] : undefined;
           return {
             commands: stats.commands,
             failure: stats.failure,
             handle: last ? last[0] : null,
             held: entries.length,
             // The browser's own class, as groups H, J, K, L and M ask. Node has
             // no \`GPUBindGroup\` binding at all, so this is what a silent fall
             // back to a stub cannot survive.
             isRealGroup:
               typeof GPUBindGroup === 'function' &&
               group instanceof GPUBindGroup,
             // Every other member of a \`GPUBindGroup\` is absent by design, so
             // this is the whole of what the object can be asked.
             label: group?.label,
           };
         })()`
        )
      )
    : null;

  // Read a moment after the replay rather than during it, for the reason groups
  // K, L and M spell out: WebGPU raises a validation error "in a future task", so
  // a queue read in the evaluation that saw the replay finish is empty whatever
  // happened. The failure it guards here is the resolution: a `WHOLE_BUFFER` size
  // passed on as `18446744073709551615`, a resource resolved against the wrong
  // table, a layout the group does not match — every one of those is a group the
  // browser refuses and nothing else reports.
  const bindGroupReport = bindGroupProbe
    ? await evaluate(
        page,
        `(async () => {
         await new Promise((settle) =>
           requestAnimationFrame(() => requestAnimationFrame(settle))
         );
         return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
       })()`
      )
    : null;
  check(
    'N',
    'wasm encoded a bind group creation and the page loop replayed it',
    bindGroupProbe?.commands?.join(',') ===
      'CreateBindGroupLayout,CreateBuffer,CreateImage,CreateImageView,CreateSampler,CreateBindGroup' &&
      Number.isInteger(bindGroupProbe.handle),
    bindGroupProbe
      ? `the loop replayed [${bindGroupProbe.commands.join(', ')}] for group ${bindGroupProbe.handle}` +
          `${bindGroupProbe.failure ? ` — ${bindGroupProbe.failure}` : ''}`
      : bindGroupStart?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'N',
    'a real GPUBindGroup came back from the device with the whole-buffer binding and all three resource kinds accepted',
    bindGroupProbe?.isRealGroup === true &&
      bindGroupProbe?.label === PROBE_BIND_GROUP_LABEL &&
      bindGroupReport?.waited === true &&
      bindGroupReport?.error === null,
    bindGroupReport?.waited !== true
      ? 'the page never got as far as reading the device error queue'
      : bindGroupProbe?.isRealGroup && bindGroupReport.error === null
        ? `an instance of this browser's GPUBindGroup labelled ${JSON.stringify(bindGroupProbe.label)}, binding a buffer, a view and a sampler, of ${bindGroupProbe.held} held, and the device reported nothing`
        : `instanceof GPUBindGroup: ${bindGroupProbe?.isRealGroup}, label ${JSON.stringify(bindGroupProbe?.label)}` +
          `${bindGroupReport.error ? ` — ${bindGroupReport.error}` : ''}`
  );

  // A `GPUShaderModule` reports its `label`, like a sampler, a layout and a bind
  // group — but it is the only object this seam makes where *compilation* happens,
  // so group O has a second piece of evidence the others do not: `getCompilationInfo()`.
  // The descriptor carries a known-good WGSL vertex entry, so beyond
  // `instanceof GPUShaderModule` — which no stub can satisfy, node having no such
  // binding — the gate reads the compilation info off the object and holds it to
  // no errors. That is stronger than existence: a module that came back but would
  // not compile is exactly what a browser answers for bad WGSL without throwing,
  // and only `getCompilationInfo()` catches it. `gpu-replay.mjs` proves the
  // descriptor — WGSL alone, the other three artifacts dropped — against a stub,
  // and proves the WGSL-less refusal; only this asks a real device to compile it.
  group('O — a shader module is compiled on the real device');

  const shaderStart = await evaluate(
    page,
    `(async () => {
     const { startShaderModuleProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startShaderModuleProbe({ exports }),
       replayed: before.replayed,
     };
   })()`
  );
  const shaderProbe = shaderStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const { gpu } = globalThis.crcbl;
           const stats = gpu.stats();
           if (stats.replayed <= ${shaderStart.replayed} && !stats.failure) {
             return null;
           }
           const entries = [...gpu.replayer.shaderModules.entries()];
           const last = entries[entries.length - 1];
           const module = last ? last[1] : undefined;
           return {
             commands: stats.commands,
             failure: stats.failure,
             handle: last ? last[0] : null,
             held: entries.length,
             // The browser's own class, as groups H, J–N ask. Node has no
             // \`GPUShaderModule\` binding at all, so this is what a silent fall
             // back to a stub cannot survive.
             isRealModule:
               typeof GPUShaderModule === 'function' &&
               module instanceof GPUShaderModule,
             label: module?.label,
           };
         })()`
        )
      )
    : null;

  // Read the compilation info in a second evaluation, because it is async and
  // needs the module object. This is the check no other group has: a shader
  // module is where compilation happens, and a browser reports a bad WGSL not by
  // throwing but through this report — so a clean one is the proof the WGSL this
  // seam sent compiled, which is stronger than the module merely existing. The
  // device error queue is read after a couple of frames too, as groups K–N do,
  // for anything WebGPU raises in a future task.
  const shaderReport = shaderProbe?.isRealModule
    ? await evaluate(
        page,
        `(async () => {
         const entries = [...globalThis.crcbl.gpu.replayer.shaderModules.entries()];
         const last = entries[entries.length - 1];
         const module = last ? last[1] : undefined;
         const info = module ? await module.getCompilationInfo() : null;
         const errors = info
           ? info.messages
               .filter((m) => m.type === 'error')
               .map((m) => m.message)
           : ['the module was gone before getCompilationInfo could run'];
         await new Promise((settle) =>
           requestAnimationFrame(() => requestAnimationFrame(settle))
         );
         return {
           waited: true,
           errors,
           error: globalThis.crcbl.gpu.replayer.takeError(),
         };
       })()`
      )
    : null;
  check(
    'O',
    'wasm encoded a shader module creation and the page loop replayed it',
    shaderProbe?.commands?.join(',') === 'CreateShaderModule' &&
      Number.isInteger(shaderProbe.handle),
    shaderProbe
      ? `the loop replayed [${shaderProbe.commands.join(', ')}] for module ${shaderProbe.handle}` +
          `${shaderProbe.failure ? ` — ${shaderProbe.failure}` : ''}`
      : shaderStart?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'O',
    'a real GPUShaderModule came back from the device with clean compilation info for the known-good WGSL',
    shaderProbe?.isRealModule === true &&
      shaderProbe?.label === PROBE_SHADER_MODULE_LABEL &&
      shaderReport?.waited === true &&
      Array.isArray(shaderReport?.errors) &&
      shaderReport.errors.length === 0 &&
      shaderReport?.error === null,
    shaderReport?.waited !== true
      ? 'the page never got as far as reading the module’s compilation info'
      : shaderProbe?.isRealModule &&
          shaderReport.errors.length === 0 &&
          shaderReport.error === null
        ? `an instance of this browser's GPUShaderModule labelled ${JSON.stringify(shaderProbe.label)}, getCompilationInfo reported no errors, of ${shaderProbe.held} held, and the device reported nothing`
        : `instanceof GPUShaderModule: ${shaderProbe?.isRealModule}, label ${JSON.stringify(shaderProbe?.label)}, compilation errors ${JSON.stringify(shaderReport?.errors)}` +
          `${shaderReport?.error ? ` — ${shaderReport.error}` : ''}`
  );

  // A `GPUPipelineLayout` reports its `label` and nothing else — not its
  // bind-group layouts, not its push-constant ranges (WebGPU has none) — so group
  // P's evidence is group N's two: the class of what came back, which
  // `instanceof GPUPipelineLayout` settles and no stub can satisfy (node has no
  // such binding), and the device's error queue being empty afterwards, which is
  // the only thing that can say `createPipelineLayout` *accepted* the set list it
  // was handed. The probe records a bind-group layout and then a pipeline layout
  // built from it, with `push_constants: None` so it builds rather than being
  // refused. `gpu-replay.mjs` proves that descriptor and every refusal — a `Some`
  // push-constant range, an unresolvable set — against a stub; only a real device
  // can say the browser takes it.
  group('P — a pipeline layout is created on the real device');

  const pipelineLayoutStart = await evaluate(
    page,
    `(async () => {
     const { startPipelineLayoutProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startPipelineLayoutProbe({ exports }),
       replayed: before.replayed,
     };
   })()`
  );
  const pipelineLayoutProbe = pipelineLayoutStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const { gpu } = globalThis.crcbl;
           const stats = gpu.stats();
           if (stats.replayed <= ${pipelineLayoutStart.replayed} && !stats.failure) {
             return null;
           }
           const entries = [...gpu.replayer.pipelineLayouts.entries()];
           const last = entries[entries.length - 1];
           const layout = last ? last[1] : undefined;
           return {
             commands: stats.commands,
             failure: stats.failure,
             handle: last ? last[0] : null,
             held: entries.length,
             // The browser's own class, as groups H, J–O ask. Node has no
             // \`GPUPipelineLayout\` binding at all, so this is what a silent
             // fall back to a stub cannot survive.
             isRealLayout:
               typeof GPUPipelineLayout === 'function' &&
               layout instanceof GPUPipelineLayout,
             // Every other member of a \`GPUPipelineLayout\` is absent by
             // design, so this is the whole of what the object can be asked.
             label: layout?.label,
           };
         })()`
        )
      )
    : null;

  // Read a moment after the replay rather than during it, for the reason groups
  // K–N spell out: WebGPU raises a validation error "in a future task", so a
  // queue read in the evaluation that saw the replay finish is empty whatever
  // happened. The failure it guards here is the set resolution: a bind-group
  // layout the pipeline layout could not find, or a set list the browser refused.
  const pipelineLayoutReport = pipelineLayoutProbe
    ? await evaluate(
        page,
        `(async () => {
         await new Promise((settle) =>
           requestAnimationFrame(() => requestAnimationFrame(settle))
         );
         return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
       })()`
      )
    : null;
  check(
    'P',
    'wasm encoded a pipeline layout creation and the page loop replayed it',
    pipelineLayoutProbe?.commands?.join(',') ===
      'CreateBindGroupLayout,CreatePipelineLayout' &&
      Number.isInteger(pipelineLayoutProbe.handle),
    pipelineLayoutProbe
      ? `the loop replayed [${pipelineLayoutProbe.commands.join(', ')}] for pipeline layout ${pipelineLayoutProbe.handle}` +
          `${pipelineLayoutProbe.failure ? ` — ${pipelineLayoutProbe.failure}` : ''}`
      : pipelineLayoutStart?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'P',
    'a real GPUPipelineLayout came back from the device with the bind-group layout set accepted',
    pipelineLayoutProbe?.isRealLayout === true &&
      pipelineLayoutProbe?.label === PROBE_PIPELINE_LAYOUT_LABEL &&
      pipelineLayoutReport?.waited === true &&
      pipelineLayoutReport?.error === null,
    pipelineLayoutReport?.waited !== true
      ? 'the page never got as far as reading the device error queue'
      : pipelineLayoutProbe?.isRealLayout && pipelineLayoutReport.error === null
        ? `an instance of this browser's GPUPipelineLayout labelled ${JSON.stringify(pipelineLayoutProbe.label)}, built from one bind-group layout, of ${pipelineLayoutProbe.held} held, and the device reported nothing`
        : `instanceof GPUPipelineLayout: ${pipelineLayoutProbe?.isRealLayout}, label ${JSON.stringify(pipelineLayoutProbe?.label)}` +
          `${pipelineLayoutReport.error ? ` — ${pipelineLayoutReport.error}` : ''}`
  );

  // A `GPUComputePipeline` reports its `label`, like a pipeline layout — but it
  // is the first object this seam makes that resolves handles into two *different*
  // tables (its layout and its compute module) and where the shader is bound to
  // the layout, so group Q has evidence group P does not: `getBindGroupLayout(0)`,
  // the derived layout only a genuinely-built pipeline answers. So beyond
  // `instanceof GPUComputePipeline` — which no stub can satisfy, node having no
  // such binding — the gate calls `getBindGroupLayout(0)` and reads the device's
  // error queue after a settle. `gpu-replay.mjs` proves the descriptor — its two
  // resolved handles and, the field that matters most, no workgroup-size member —
  // and the two distinct resolution refusals against a stub; only this asks a real
  // device to build the pipeline from a real compute shader.
  group('Q — a compute pipeline is built on the real device');

  const computePipelineStart = await evaluate(
    page,
    `(async () => {
     const { startComputePipelineProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startComputePipelineProbe({ exports }),
       replayed: before.replayed,
     };
   })()`
  );
  const computePipelineProbe = computePipelineStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const { gpu } = globalThis.crcbl;
           const stats = gpu.stats();
           if (stats.replayed <= ${computePipelineStart.replayed} && !stats.failure) {
             return null;
           }
           const entries = [...gpu.replayer.computePipelines.entries()];
           const last = entries[entries.length - 1];
           const pipeline = last ? last[1] : undefined;
           let derivedLayout = false;
           try {
             // The call no stub and no half-built pipeline can answer: a real
             // GPUComputePipeline hands back a GPUBindGroupLayout for group 0,
             // because a pipeline is where the shader and its layout are bound.
             derivedLayout =
               typeof GPUBindGroupLayout === 'function' &&
               pipeline?.getBindGroupLayout(0) instanceof GPUBindGroupLayout;
           } catch (error) {
             derivedLayout = false;
           }
           return {
             commands: stats.commands,
             failure: stats.failure,
             handle: last ? last[0] : null,
             held: entries.length,
             // The browser's own class, as groups H, J–P ask. Node has no
             // \`GPUComputePipeline\` binding at all, so this is what a silent
             // fall back to a stub cannot survive.
             isRealPipeline:
               typeof GPUComputePipeline === 'function' &&
               pipeline instanceof GPUComputePipeline,
             derivedLayout,
             label: pipeline?.label,
           };
         })()`
        )
      )
    : null;

  // Read the device error queue a moment after the replay rather than during it,
  // for the reason groups K–P spell out: WebGPU raises a validation error "in a
  // future task", and `createComputePipeline` reports a bad entry point or a
  // shader/layout mismatch through `uncapturederror` a task later — so a queue
  // read in the evaluation that saw the replay finish is empty whatever happened.
  const computePipelineReport = computePipelineProbe
    ? await evaluate(
        page,
        `(async () => {
         await new Promise((settle) =>
           requestAnimationFrame(() => requestAnimationFrame(settle))
         );
         return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
       })()`
      )
    : null;
  check(
    'Q',
    'wasm encoded a compute pipeline creation and the page loop replayed it',
    computePipelineProbe?.commands?.join(',') ===
      'CreateShaderModule,CreatePipelineLayout,CreateComputePipeline' &&
      Number.isInteger(computePipelineProbe.handle),
    computePipelineProbe
      ? `the loop replayed [${computePipelineProbe.commands.join(', ')}] for compute pipeline ${computePipelineProbe.handle}` +
          `${computePipelineProbe.failure ? ` — ${computePipelineProbe.failure}` : ''}`
      : computePipelineStart?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'Q',
    'a real GPUComputePipeline came back from the device and answered getBindGroupLayout',
    computePipelineProbe?.isRealPipeline === true &&
      computePipelineProbe?.derivedLayout === true &&
      computePipelineProbe?.label === PROBE_COMPUTE_PIPELINE_LABEL &&
      computePipelineReport?.waited === true &&
      computePipelineReport?.error === null,
    computePipelineReport?.waited !== true
      ? 'the page never got as far as reading the device error queue'
      : computePipelineProbe?.isRealPipeline &&
          computePipelineProbe?.derivedLayout &&
          computePipelineReport.error === null
        ? `an instance of this browser's GPUComputePipeline labelled ${JSON.stringify(computePipelineProbe.label)}, its getBindGroupLayout(0) a real GPUBindGroupLayout, of ${computePipelineProbe.held} held, and the device reported nothing`
        : `instanceof GPUComputePipeline: ${computePipelineProbe?.isRealPipeline}, getBindGroupLayout(0) real: ${computePipelineProbe?.derivedLayout}, label ${JSON.stringify(computePipelineProbe?.label)}` +
          `${computePipelineReport?.error ? ` — ${computePipelineReport.error}` : ''}`
  );

  // A `GPURenderPipeline` answers `getBindGroupLayout(0)` like a compute pipeline
  // — but it is the *largest* descriptor on the seam, so group R is what puts its
  // whole nested tree (a `TriangleList` primitive, a reversed-Z depth-stencil, a
  // single-sampled multisample state, and a blended `Rgba8Unorm` target) in front
  // of a real device. `gpu-replay.mjs` proves the descriptor and every "WebGPU
  // cannot express it" refusal against a stub; only this asks a real device to
  // build the pipeline from a real vertex and fragment shader. Beyond
  // `instanceof GPURenderPipeline` — which no stub can satisfy, node having no
  // such binding — the gate calls `getBindGroupLayout(0)` and reads the device's
  // error queue after a settle.
  group('R — a graphics pipeline is built on the real device');

  const graphicsPipelineStart = await evaluate(
    page,
    `(async () => {
     const { startGraphicsPipelineProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startGraphicsPipelineProbe({ exports }),
       replayed: before.replayed,
     };
   })()`
  );
  const graphicsPipelineProbe = graphicsPipelineStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(() => {
           const { gpu } = globalThis.crcbl;
           const stats = gpu.stats();
           if (stats.replayed <= ${graphicsPipelineStart.replayed} && !stats.failure) {
             return null;
           }
           const entries = [...gpu.replayer.graphicsPipelines.entries()];
           const last = entries[entries.length - 1];
           const pipeline = last ? last[1] : undefined;
           let derivedLayout = false;
           try {
             // The call no stub and no half-built pipeline can answer: a real
             // GPURenderPipeline hands back a GPUBindGroupLayout for group 0,
             // because a pipeline is where the shaders and their layout are bound.
             derivedLayout =
               typeof GPUBindGroupLayout === 'function' &&
               pipeline?.getBindGroupLayout(0) instanceof GPUBindGroupLayout;
           } catch (error) {
             derivedLayout = false;
           }
           return {
             commands: stats.commands,
             failure: stats.failure,
             handle: last ? last[0] : null,
             held: entries.length,
             // The browser's own class. Node has no \`GPURenderPipeline\`
             // binding at all, so this is what a silent fall back to a stub
             // cannot survive.
             isRealPipeline:
               typeof GPURenderPipeline === 'function' &&
               pipeline instanceof GPURenderPipeline,
             derivedLayout,
             label: pipeline?.label,
           };
         })()`
        )
      )
    : null;

  // Read the device error queue a moment after the replay, for group Q's reason:
  // `createRenderPipeline` reports a bad shader/layout or an unexpressible field
  // through `uncapturederror` a task later, so a queue read in the evaluation that
  // saw the replay finish is empty whatever happened.
  const graphicsPipelineReport = graphicsPipelineProbe
    ? await evaluate(
        page,
        `(async () => {
         await new Promise((settle) =>
           requestAnimationFrame(() => requestAnimationFrame(settle))
         );
         return { waited: true, error: globalThis.crcbl.gpu.replayer.takeError() };
       })()`
      )
    : null;
  check(
    'R',
    'wasm encoded a graphics pipeline creation and the page loop replayed it',
    graphicsPipelineProbe?.commands?.join(',') ===
      'CreateShaderModule,CreatePipelineLayout,CreateGraphicsPipeline' &&
      Number.isInteger(graphicsPipelineProbe.handle),
    graphicsPipelineProbe
      ? `the loop replayed [${graphicsPipelineProbe.commands.join(', ')}] for graphics pipeline ${graphicsPipelineProbe.handle}` +
          `${graphicsPipelineProbe.failure ? ` — ${graphicsPipelineProbe.failure}` : ''}`
      : graphicsPipelineStart?.started
        ? `the page loop replayed nothing in ${TIMEOUT_MS} ms`
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'R',
    'a real GPURenderPipeline came back from the device and answered getBindGroupLayout',
    graphicsPipelineProbe?.isRealPipeline === true &&
      graphicsPipelineProbe?.derivedLayout === true &&
      graphicsPipelineProbe?.label === PROBE_GRAPHICS_PIPELINE_LABEL &&
      graphicsPipelineReport?.waited === true &&
      graphicsPipelineReport?.error === null,
    graphicsPipelineReport?.waited !== true
      ? 'the page never got as far as reading the device error queue'
      : graphicsPipelineProbe?.isRealPipeline &&
          graphicsPipelineProbe?.derivedLayout &&
          graphicsPipelineReport.error === null
        ? `an instance of this browser's GPURenderPipeline labelled ${JSON.stringify(graphicsPipelineProbe.label)}, its getBindGroupLayout(0) a real GPUBindGroupLayout, of ${graphicsPipelineProbe.held} held, and the device reported nothing`
        : `instanceof GPURenderPipeline: ${graphicsPipelineProbe?.isRealPipeline}, getBindGroupLayout(0) real: ${graphicsPipelineProbe?.derivedLayout}, label ${JSON.stringify(graphicsPipelineProbe?.label)}` +
          `${graphicsPipelineReport?.error ? ` — ${graphicsPipelineReport.error}` : ''}`
  );

  // **THE DECISIVE GATE OF THE WHOLE TRACK, AND THE FIRST THAT READS PIXELS.**
  // Everything above builds objects — a buffer, a texture, a pipeline — and
  // proves the browser accepted the descriptor this seam encoded. None of them
  // proves the backend puts the *right pixels* in memory, because none of them
  // renders and reads back. This does: wasm records a clear of a 64×64 texture to
  // a colour exact in 8 bits, a copy of it into a host-readable buffer, a submit
  // and a readback request; the page's own loop replays it; the browser's
  // `mapAsync` resolves a few frames later; and the bytes that come back on the
  // reply channel are asserted to be that colour, every pixel.
  //
  // `gpu-replay.mjs` cannot reach here: its stub buffer hands back whatever bytes
  // it likes, so a readback against it proves the bookkeeping and nothing about
  // the pixels. Only a real device clears a real texture, and only a real
  // `copyTextureToBuffer` and `mapAsync` carry the result back — which is why
  // this check is the one that a stub silently substituted for the backend fails.
  group('S — cleared pixels are read back from memory as the clear colour');

  const PROBE_READBACK_PIXELS = 64 * 64;
  const PROBE_READBACK_CLEAR_BYTES = [64, 128, 191, 255];

  const readbackStart = await evaluate(
    page,
    `(async () => {
     const { startReadbackProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     const before = gpu.stats();
     return {
       started: startReadbackProbe({ exports }),
       replayed: before.replayed,
     };
   })()`
  );
  // Poll across frames the way group G/H wait for the device: the page's rAF loop
  // replays the poll and delivers its reply between these evaluations, and each
  // evaluation drains what has arrived, then queues another poll while the map is
  // still resolving. `readReadbackProbe` drains first, so a reply delivered since
  // the last frame is absorbed before another poll is queued — and a second poll
  // is never queued while one is unanswered, which `poll_readback` enforces.
  const readback = readbackStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readReadbackProbe, pollReadbackProbe, READBACK } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readReadbackProbe({ exports, memory: exports.memory });
           if (r.state === READBACK.FAILED || r.state === READBACK.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== READBACK.READY) {
             // Not yet — queue another poll for the loop to replay, and wait.
             pollReadbackProbe({ exports });
             return null;
           }
           // Ready: check every pixel here rather than shipping 16384 numbers
           // out over the protocol. \`want\` is the clear colour in 8-bit-exact
           // channels — 0.25/0.5/0.75/1.0 → 64/128/191/255.
           const want = ${JSON.stringify(PROBE_READBACK_CLEAR_BYTES)};
           let allMatch = r.bytes.length === ${PROBE_READBACK_PIXELS} * 4;
           let firstWrong = -1;
           for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
             if (r.bytes[i] !== want[i % 4]) {
               allMatch = false;
               firstWrong = i;
             }
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             allMatch,
             firstWrong,
             // The first two texels, for a failure message a human can read.
             sample: [...r.bytes.slice(0, 8)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'S',
    'wasm encoded the readback setup frame — clear, copy, submit, request',
    readbackStart?.started === true,
    readbackStart?.started
      ? 'the clear-and-read frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'S',
    'the cleared pixels came back from memory as the clear colour, every one',
    readback?.done === true &&
      readback.allMatch === true &&
      readback.len === PROBE_READBACK_PIXELS * 4,
    readback?.done !== true
      ? `no readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : readback.allMatch
        ? `${readback.len} bytes, every texel [${PROBE_READBACK_CLEAR_BYTES.join(', ')}]`
        : `state ${readback.state}, ${readback.len ?? 0} bytes, ` +
          `first wrong at byte ${readback.firstWrong} (sample ${JSON.stringify(readback.sample)} ` +
          `against [${PROBE_READBACK_CLEAR_BYTES.join(', ')}])` +
          `${readback.error ? ` — ${readback.error}` : ''}`
  );

  // **THE DRAW GATE, AND THE FIRST THAT PROVES A DRAW.** Group S proves a clear
  // reaches host memory. This proves a `setPipeline` + `draw` *overwrites* that
  // clear: wasm records a colour-only pipeline, a pass that clears the 64×64
  // texture to the blue clear and then binds the pipeline and draws a fullscreen
  // triangle whose fragment shader writes constant red, a copy into a host
  // buffer, a submit and a readback; the page loop replays it; and the bytes
  // that come back are asserted to be the red draw colour, every pixel — not the
  // blue clear underneath. A stub that skips the draw leaves blue and fails here;
  // only a real draw leaves red. `gpu-replay.mjs` cannot reach this for group S's
  // reason: only a real device rasterises a triangle into a real texture.
  group('T — a drawn triangle is read back as the draw colour, not the clear');

  const PROBE_DRAW_PIXELS = 64 * 64;
  const PROBE_DRAW_COLOR_BYTES = [255, 0, 0, 255];
  const PROBE_DRAW_CLEAR_BYTES = [64, 128, 191, 255];

  const drawStart = await evaluate(
    page,
    `(async () => {
     const { startDrawProbe } = await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     return { started: startDrawProbe({ exports }) };
   })()`
  );
  // Poll across frames exactly as group S does: the page's rAF loop replays the
  // poll and delivers its reply between these evaluations, and each evaluation
  // drains what has arrived, then queues another poll while the map is still
  // resolving. `readDrawProbe` drains first, so a reply delivered since the last
  // frame is absorbed before another poll is queued.
  const draw = drawStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readDrawProbe, pollDrawProbe, DRAW } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readDrawProbe({ exports, memory: exports.memory });
           if (r.state === DRAW.FAILED || r.state === DRAW.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== DRAW.READY) {
             // Not yet — queue another poll for the loop to replay, and wait.
             pollDrawProbe({ exports });
             return null;
           }
           // Ready: check every pixel here rather than shipping 16384 numbers
           // out. \`want\` is the fragment's constant red; the clear beneath it
           // was blue, so a texel still blue is a draw that did not happen.
           const want = ${JSON.stringify(PROBE_DRAW_COLOR_BYTES)};
           let allMatch = r.bytes.length === ${PROBE_DRAW_PIXELS} * 4;
           let firstWrong = -1;
           for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
             if (r.bytes[i] !== want[i % 4]) {
               allMatch = false;
               firstWrong = i;
             }
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             allMatch,
             firstWrong,
             // The first two texels, for a failure message a human can read.
             sample: [...r.bytes.slice(0, 8)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'T',
    'wasm encoded the draw setup frame — pipeline, clear, bind, draw, copy, request',
    drawStart?.started === true,
    drawStart?.started
      ? 'the draw-and-read frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'T',
    'the drawn pixels came back as the draw colour, not the clear, every one',
    draw?.done === true &&
      draw.allMatch === true &&
      draw.len === PROBE_DRAW_PIXELS * 4,
    draw?.done !== true
      ? `no draw readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : draw.allMatch
        ? `${draw.len} bytes, every texel [${PROBE_DRAW_COLOR_BYTES.join(', ')}] (red), not the clear [${PROBE_DRAW_CLEAR_BYTES.join(', ')}] (blue)`
        : `state ${draw.state}, ${draw.len ?? 0} bytes, ` +
          `first wrong at byte ${draw.firstWrong} (sample ${JSON.stringify(draw.sample)} ` +
          `against [${PROBE_DRAW_COLOR_BYTES.join(', ')}]; the clear was [${PROBE_DRAW_CLEAR_BYTES.join(', ')}])` +
          `${draw.error ? ` — ${draw.error}` : ''}`
  );

  // **THE DISPATCH GATE, AND THE FIRST THAT PROVES A COMPUTE SHADER RAN.** Group T
  // proves a draw overwrites a clear. This proves a `dispatchWorkgroups` writes a
  // storage buffer: wasm records a compute pipeline whose shader sets every slot
  // of a 64-`u32` storage buffer to 0xDEADBEEF, a pass that binds and dispatches
  // it, a buffer→buffer copy into a host buffer, a submit and a readback; the
  // page loop replays it; and the 256 bytes that come back are asserted to be
  // 0xDEADBEEF in every 4-byte little-endian word. A fresh WebGPU buffer is
  // zero-initialised, so a stub that skips the dispatch reads back all zeros —
  // only a dispatch that actually ran writes the pattern. `gpu-replay.mjs` cannot
  // reach this: only a real device runs a compute shader into a real buffer.
  group('U — a compute shader’s storage-buffer writes are read back');

  const PROBE_DISPATCH_WORDS = 64;
  // 0xDEADBEEF as the little-endian bytes a `u32` holds it as.
  const PROBE_DISPATCH_PATTERN_BYTES = [0xef, 0xbe, 0xad, 0xde];

  const computeStart = await evaluate(
    page,
    `(async () => {
     const { startComputeProbe } = await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     return { started: startComputeProbe({ exports }) };
   })()`
  );
  // Poll across frames exactly as group T does: the page's rAF loop replays the
  // poll and delivers its reply between these evaluations, and each evaluation
  // drains what has arrived, then queues another poll while the map is still
  // resolving. `readComputeProbe` drains first, so a reply delivered since the
  // last frame is absorbed before another poll is queued.
  const compute = computeStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readComputeProbe, pollComputeProbe, COMPUTE } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readComputeProbe({ exports, memory: exports.memory });
           if (r.state === COMPUTE.FAILED || r.state === COMPUTE.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== COMPUTE.READY) {
             // Not yet — queue another poll for the loop to replay, and wait.
             pollComputeProbe({ exports });
             return null;
           }
           // Ready: check every word here rather than shipping 64 numbers out.
           // \`want\` is 0xDEADBEEF little-endian; a buffer left at zero is a
           // dispatch that did not happen.
           const want = ${JSON.stringify(PROBE_DISPATCH_PATTERN_BYTES)};
           let allMatch = r.bytes.length === ${PROBE_DISPATCH_WORDS} * 4;
           let firstWrong = -1;
           for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
             if (r.bytes[i] !== want[i % 4]) {
               allMatch = false;
               firstWrong = i;
             }
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             allMatch,
             firstWrong,
             // The first two words, for a failure message a human can read.
             sample: [...r.bytes.slice(0, 8)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'U',
    'wasm encoded the dispatch setup frame — pipeline, pass, bind, dispatch, copy, request',
    computeStart?.started === true,
    computeStart?.started
      ? 'the dispatch-and-read frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'U',
    'the dispatched storage buffer came back as 0xDEADBEEF, every word',
    compute?.done === true &&
      compute.allMatch === true &&
      compute.len === PROBE_DISPATCH_WORDS * 4,
    compute?.done !== true
      ? `no dispatch readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : compute.allMatch
        ? `${compute.len} bytes, every word 0xDEADBEEF [${PROBE_DISPATCH_PATTERN_BYTES.join(', ')}], not the zero-init a stub reads back`
        : `state ${compute.state}, ${compute.len ?? 0} bytes, ` +
          `first wrong at byte ${compute.firstWrong} (sample ${JSON.stringify(compute.sample)} ` +
          `against [${PROBE_DISPATCH_PATTERN_BYTES.join(', ')}])` +
          `${compute.error ? ` — ${compute.error}` : ''}`
  );

  // **THE INDIRECT-DISPATCH GATE, AND THE ONLY THING ANYWHERE THAT RUNS
  // `dispatchWorkgroupsIndirect` IN A BROWSER.** Group U proves a dispatch ran;
  // this proves a dispatch ran *with the workgroup counts a buffer named*. wasm
  // records the same compute frame with three differences: an args buffer filled
  // by `queue.writeBuffer`, a `dispatchWorkgroupsIndirect` reading it at a
  // non-zero offset, and a shader that tallies which workgroup indices ran
  // instead of writing one flat pattern. One invocation per workgroup, so the
  // tally IS the workgroup grid: the first slot counts every workgroup, and one
  // tally region per axis after it holds a mark at index `i` if a workgroup with
  // that index on that axis ran.
  //
  // WHY A WRONG COUNT FAILS RATHER THAN GOING UNNOTICED. The counts are not all
  // ones and no two are equal, and the bytes at the START of the args buffer are
  // a `1, 1, 1` decoy — so a replayer that dropped the offset dispatches one
  // workgroup, successfully, and reads back a tally of one. The slots past each
  // axis's count are asserted still zero, so dispatching MORE than was asked for
  // fails too, and the counter is asserted against the product, so three right
  // maxima reached by a wrong grid fail as well. A frame where the dispatch
  // never happened reads back all zeros. `gpu-replay.mjs` cannot reach any of
  // this: only a real device runs a compute shader off a real buffer.
  group('AG — a compute dispatch reads its workgroup counts out of a buffer');

  // `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_DISPATCH_INDIRECT_*`, spelled
  // out here rather than imported: this half is the independent statement of
  // what the bytes must be, and `js_mirror` holds the state codes.
  const PROBE_DISPATCH_INDIRECT_WORDS = 64;
  const PROBE_DISPATCH_INDIRECT_COUNTS = [5, 3, 2];
  const PROBE_DISPATCH_INDIRECT_AXIS_SLOTS = 16;
  // The counter, then the X, Y and Z tally regions.
  const PROBE_DISPATCH_INDIRECT_BASES = [1, 17, 33];
  const PROBE_DISPATCH_INDIRECT_MARK = 0xc0ffee;
  // Where the real counts sit in the args buffer, and where the decoy the
  // dispatch must NOT read sits.
  const PROBE_DISPATCH_INDIRECT_ARGS_OFFSET = 16;
  const PROBE_DISPATCH_INDIRECT_DECOY_OFFSET = 0;
  const PROBE_DISPATCH_INDIRECT_WORKGROUPS =
    PROBE_DISPATCH_INDIRECT_COUNTS.reduce((a, b) => a * b, 1);

  const dispatchIndirectStart = await evaluate(
    page,
    `(async () => {
     const { startDispatchIndirectProbe } = await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     return { started: startDispatchIndirectProbe({ exports }) };
   })()`
  );
  // Poll across frames exactly as group U does: the page's rAF loop replays the
  // poll and delivers its reply between these evaluations, and each evaluation
  // drains what has arrived, then queues another poll while the map is still
  // resolving.
  const dispatchIndirect = dispatchIndirectStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const {
             readDispatchIndirectProbe,
             pollDispatchIndirectProbe,
             DISPATCH_INDIRECT,
           } = await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readDispatchIndirectProbe({ exports, memory: exports.memory });
           if (
             r.state === DISPATCH_INDIRECT.FAILED ||
             r.state === DISPATCH_INDIRECT.UNDECODABLE
           ) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== DISPATCH_INDIRECT.READY) {
             // Not yet — queue another poll for the loop to replay, and wait.
             pollDispatchIndirectProbe({ exports });
             return null;
           }
           // Ready: read the tally here rather than shipping every word out.
           const word = (at) =>
             (r.bytes[at * 4] |
               (r.bytes[at * 4 + 1] << 8) |
               (r.bytes[at * 4 + 2] << 16) |
               (r.bytes[at * 4 + 3] << 24)) >>> 0;
           const mark = ${PROBE_DISPATCH_INDIRECT_MARK};
           const axisSlots = ${PROBE_DISPATCH_INDIRECT_AXIS_SLOTS};
           const bases = ${JSON.stringify(PROBE_DISPATCH_INDIRECT_BASES)};
           const words = Math.floor(r.bytes.length / 4);
           // Per axis: how many slots from the region's start carry the mark,
           // and the first slot past them that is not still zero. A dispatch of
           // more workgroups than were asked for shows up as the second.
           const counts = [];
           const strays = [];
           for (const base of bases) {
             let count = 0;
             while (count < axisSlots && word(base + count) === mark) count += 1;
             counts.push(count);
             let stray = -1;
             for (let i = count; i < axisSlots && stray < 0; i += 1) {
               if (word(base + i) !== 0) stray = i;
             }
             strays.push(stray);
           }
           // And the slots past the last region, which nothing should touch.
           let tail = -1;
           const after = bases[bases.length - 1] + axisSlots;
           for (let i = after; i < words && tail < 0; i += 1) {
             if (word(i) !== 0) tail = i;
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             total: word(0),
             counts,
             strays,
             tail,
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'AG',
    'wasm encoded the indirect-dispatch setup frame — pipeline, args write, pass, bind, dispatch_indirect, copy, request',
    dispatchIndirectStart?.started === true,
    dispatchIndirectStart?.started
      ? 'the indirect-dispatch-and-read frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'AG',
    'the browser ran the number of workgroups the args buffer named, not the 1×1×1 decoy at its start',
    dispatchIndirect?.done === true &&
      dispatchIndirect.len === PROBE_DISPATCH_INDIRECT_WORDS * 4 &&
      dispatchIndirect.total === PROBE_DISPATCH_INDIRECT_WORKGROUPS,
    dispatchIndirect?.done !== true
      ? `no indirect-dispatch readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : dispatchIndirect.total === PROBE_DISPATCH_INDIRECT_WORKGROUPS &&
          dispatchIndirect.len === PROBE_DISPATCH_INDIRECT_WORDS * 4
        ? `${dispatchIndirect.total} workgroups ran, the product of ` +
          `[${PROBE_DISPATCH_INDIRECT_COUNTS.join(', ')}] at args offset ` +
          `${PROBE_DISPATCH_INDIRECT_ARGS_OFFSET} and not the 1 the decoy at ` +
          `offset ${PROBE_DISPATCH_INDIRECT_DECOY_OFFSET} asks for`
        : `state ${dispatchIndirect.state}, ${dispatchIndirect.len ?? 0} bytes ` +
          `(expected ${PROBE_DISPATCH_INDIRECT_WORDS * 4}), counter read ` +
          `${dispatchIndirect.total}, expected ${PROBE_DISPATCH_INDIRECT_WORKGROUPS}` +
          `${dispatchIndirect.error ? ` — ${dispatchIndirect.error}` : ''}`
  );
  check(
    'AG',
    'the per-axis tally spells the three workgroup counts the args buffer named, and nothing past them',
    dispatchIndirect?.done === true &&
      JSON.stringify(dispatchIndirect.counts) ===
        JSON.stringify(PROBE_DISPATCH_INDIRECT_COUNTS) &&
      JSON.stringify(dispatchIndirect.strays) ===
        JSON.stringify([-1, -1, -1]) &&
      dispatchIndirect.tail === -1,
    dispatchIndirect?.done !== true
      ? `no indirect-dispatch readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : `read [${(dispatchIndirect.counts ?? []).join(', ')}] against ` +
          `[${PROBE_DISPATCH_INDIRECT_COUNTS.join(', ')}]; ` +
          `first marked slot past each count ${JSON.stringify(dispatchIndirect.strays)} ` +
          `(−1 is none), first non-zero slot past the last region ${dispatchIndirect.tail}` +
          `${dispatchIndirect.error ? ` — ${dispatchIndirect.error}` : ''}`
  );

  // **THE COPY-CHAIN GATE, AND THE FIRST THAT PROVES A BUFFER→IMAGE AND AN
  // IMAGE→IMAGE COPY RAN.** wasm records a dispatch that fills a storage buffer
  // with red (`0xFF0000FF` per texel), a `pipeline_barrier` transitioning that
  // buffer from `ShaderWrite` to `TransferSrc`, a `copyBufferToTexture` into a
  // 64×64 texture, a `copyTextureToTexture` to a second texture, a
  // `copyTextureToBuffer` out to a host buffer, a submit and a readback; the
  // page loop replays it; and the 16384 bytes that come back are asserted red
  // in every texel. A fresh WebGPU texture is zero-initialised, so a stub that
  // skips EITHER copy reads back zeros — only both copies running carries the red
  // all the way out. The `pipeline_barrier` is the documented no-op: it sits
  // mid-frame between the dispatch and the first copy, and the red readback ALSO
  // proves it did not disturb replay. `gpu-replay.mjs` cannot reach the copies:
  // only a real device runs them.
  group('V — a buffer→image→image→buffer copy chain is read back red');

  const PROBE_COPYCHAIN_TEXELS = 64 * 64;
  // Opaque red as the little-endian bytes an `rgba8unorm` texel holds it as.
  const PROBE_COPYCHAIN_PATTERN_BYTES = [255, 0, 0, 255];

  const copyChainStart = await evaluate(
    page,
    `(async () => {
     const { startCopyChainProbe } = await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     return { started: startCopyChainProbe({ exports }) };
   })()`
  );
  const copyChain = copyChainStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readCopyChainProbe, pollCopyChainProbe, COPYCHAIN } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readCopyChainProbe({ exports, memory: exports.memory });
           if (r.state === COPYCHAIN.FAILED || r.state === COPYCHAIN.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== COPYCHAIN.READY) {
             pollCopyChainProbe({ exports });
             return null;
           }
           // Ready: check every texel here rather than shipping 16384 numbers
           // out. \`want\` is red; a texel left at zero is a copy that did not
           // happen.
           const want = ${JSON.stringify(PROBE_COPYCHAIN_PATTERN_BYTES)};
           let allMatch = r.bytes.length === ${PROBE_COPYCHAIN_TEXELS} * 4;
           let firstWrong = -1;
           for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
             if (r.bytes[i] !== want[i % 4]) {
               allMatch = false;
               firstWrong = i;
             }
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             allMatch,
             firstWrong,
             sample: [...r.bytes.slice(0, 8)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'V',
    'wasm encoded the copy-chain setup frame — dispatch, pipeline_barrier, buffer→image, image→image, image→buffer, request',
    copyChainStart?.started === true,
    copyChainStart?.started
      ? 'the copy-chain frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'V',
    'the copy chain came back red in every texel — both new copies ran',
    copyChain?.done === true &&
      copyChain.allMatch === true &&
      copyChain.len === PROBE_COPYCHAIN_TEXELS * 4,
    copyChain?.done !== true
      ? `no copy-chain readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : copyChain.allMatch
        ? `${copyChain.len} bytes, every texel [${PROBE_COPYCHAIN_PATTERN_BYTES.join(', ')}] (red), not the zero-init a stub reads back`
        : `state ${copyChain.state}, ${copyChain.len ?? 0} bytes, ` +
          `first wrong at byte ${copyChain.firstWrong} (sample ${JSON.stringify(copyChain.sample)} ` +
          `against [${PROBE_COPYCHAIN_PATTERN_BYTES.join(', ')}])` +
          `${copyChain.error ? ` — ${copyChain.error}` : ''}`
  );
  // The same readback, read for what it says about the no-op: the frame carries a
  // `pipeline_barrier` between the dispatch and the first copy, and a red result
  // means replay recognised it, recorded nothing, and carried on — a barrier that
  // threw or corrupted the encoder would leave this red-everywhere check failing
  // above. That it holds is the browser-frame evidence the barrier is inert.
  check(
    'V',
    'the pipeline_barrier mid-frame did not disturb replay — the barriered frame is still red',
    copyChain?.done === true &&
      copyChain.allMatch === true &&
      copyChain.len === PROBE_COPYCHAIN_TEXELS * 4,
    copyChain?.allMatch === true
      ? 'the frame with a pipeline_barrier in it read back red in every texel — the no-op is inert'
      : 'the barriered frame did not come back red — see the copy-chain check above for the bytes'
  );

  // **THE FILL GATE, AND THE FIRST THAT PROVES `clearBuffer` ZEROES EXACTLY ITS
  // SUB-RANGE.** wasm records a dispatch that fills a 256-byte storage buffer with
  // `0xDEADBEEF`, a `fill_buffer(offset 0, size 128, value 0)` that maps to
  // `clearBuffer` over the first half, a copy to a host buffer, a submit and a
  // readback; the page loop replays it; and the 256 bytes that come back are
  // asserted zero in bytes 0..128 and still `0xDEADBEEF` in bytes 128..256. A stub
  // `clearBuffer` leaves the pattern in the half that should be zero; a fill that
  // ran too far zeroes the pattern beyond its size. `gpu-replay.mjs` cannot reach
  // this: only a real device runs a compute shader and a clear into a real buffer.
  group('W — clearBuffer zeroes its sub-range and leaves the rest');

  const PROBE_FILL_WORDS = 64;
  const PROBE_FILL_ZEROED_BYTES = 128;
  // 0xDEADBEEF as the little-endian bytes a `u32` holds it as.
  const PROBE_FILL_PATTERN_BYTES = [0xef, 0xbe, 0xad, 0xde];

  const fillStart = await evaluate(
    page,
    `(async () => {
     const { startFillProbe } = await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     return { started: startFillProbe({ exports }) };
   })()`
  );
  const fill = fillStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readFillProbe, pollFillProbe, FILL } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readFillProbe({ exports, memory: exports.memory });
           if (r.state === FILL.FAILED || r.state === FILL.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== FILL.READY) {
             pollFillProbe({ exports });
             return null;
           }
           // Ready: the first half must be zero (the fill ran) and the second
           // half still the pattern (the fill stopped at its size).
           const pattern = ${JSON.stringify(PROBE_FILL_PATTERN_BYTES)};
           const zeroed = ${PROBE_FILL_ZEROED_BYTES};
           let lenOk = r.bytes.length === ${PROBE_FILL_WORDS} * 4;
           let firstHalfZero = lenOk;
           let firstWrong = -1;
           for (let i = 0; i < zeroed && firstHalfZero; i += 1) {
             if (r.bytes[i] !== 0) {
               firstHalfZero = false;
               firstWrong = i;
             }
           }
           let secondHalfPattern = lenOk;
           for (let i = zeroed; i < r.bytes.length && secondHalfPattern; i += 1) {
             if (r.bytes[i] !== pattern[i % 4]) {
               secondHalfPattern = false;
               firstWrong = i;
             }
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             firstHalfZero,
             secondHalfPattern,
             firstWrong,
             // A byte from each half, for a failure message a human can read.
             sample: [...r.bytes.slice(124, 132)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'W',
    'wasm encoded the fill setup frame — dispatch, fill_buffer, copy, request',
    fillStart?.started === true,
    fillStart?.started
      ? 'the fill frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'W',
    'the fill zeroed the first half and left the second half as 0xDEADBEEF',
    fill?.done === true &&
      fill.firstHalfZero === true &&
      fill.secondHalfPattern === true &&
      fill.len === PROBE_FILL_WORDS * 4,
    fill?.done !== true
      ? `no fill readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : fill.firstHalfZero && fill.secondHalfPattern
        ? `${fill.len} bytes, first ${PROBE_FILL_ZEROED_BYTES} zero and the rest 0xDEADBEEF [${PROBE_FILL_PATTERN_BYTES.join(', ')}]`
        : `state ${fill.state}, ${fill.len ?? 0} bytes, ` +
          `first wrong at byte ${fill.firstWrong} (sample around the boundary ${JSON.stringify(fill.sample)}; ` +
          `want zero then [${PROBE_FILL_PATTERN_BYTES.join(', ')}])` +
          `${fill.error ? ` — ${fill.error}` : ''}`
  );

  // **THE STENCIL GATE, AND THE ONLY EXERCISE `Capability::StencilReference` HAS
  // ON THIS BACKEND.** The native seam suite that holds the other four backends
  // to that declaration (`exercise_stencil_reference` in
  // `crates/crcbl/tests/hal_seam_e2e.rs`) is a native binary and cannot open this
  // one, so without this group the `Support::Yes` is a sentence nothing tests.
  //
  // WHAT IT DOES: wasm records a `depth24plus-stencil8` plane cleared to 0x2A, an
  // `Rgba8Unorm` target cleared to a background colour, and a pipeline that
  // compares the plane `Equal` against whatever the pass last set — the seam has
  // no pipeline-side reference, and WebGPU has no field for one either. Two draws
  // follow in one pass, each preceded by its own `setStencilReference`: 0x2A,
  // which the plane holds, then the first triangle; 0x11, which it does not, then
  // the second. The first of the two is recorded *before* the pipeline bind, so a
  // replayer that reset the reference on a bind lands on the background reading.
  // The page loop replays it and the 16384 texels that come back say which draws
  // survived.
  //
  // **THE OBSERVABLE IS A VALUE, NOT A SURVIVED CALL**, which is the whole design
  // constraint: a `setStencilReference` that is dropped raises no error, the draw
  // still runs, and the readback still resolves. So the three possible readings
  // are made to mean three different things and only one of them is a pass:
  //
  //   the first colour   both references were applied — the first draw passed the
  //                      test and the second was discarded. The only pass.
  //   the second colour  the SECOND reference never took effect, so the draw that
  //                      should have been discarded drew over the first. That is
  //                      a dropped call, or a stencil test not enabled at all.
  //   the background     the FIRST reference never took effect either, so both
  //                      draws were tested against the 0 a pass opens holding —
  //                      a dropped call, or a pipeline bind that reset it.
  //
  // The order of the two draws is not free to reverse: with the rejected
  // reference first, "the test is not enabled" and "both references were applied"
  // would produce the same texel, because the last draw would win either way.
  //
  // The three colours are pairwise distinct as *multisets*, so a channel swap
  // anywhere on the way out cannot turn one reading into another, and every
  // channel is a mid-tone away from the 0 and 255 an untouched or saturated one
  // reads as.
  //
  // WHERE IT BELONGS IN THE RUN: **before X**, and that is not cosmetic. X, Y, Z
  // and AA are expected to fail on Windows because the page has one `GPUDevice`
  // and therefore one queue, and X's stuck canvas readback strands every
  // `mapAsync` queued after it. This group reads bytes back, so behind X it would
  // be stranded too — and it is not in that platform's `--expect-fail` list, so
  // it would turn the Windows job red. Ahead of X it resolves like S through W
  // do, and it makes X no worse: what strands the canvas groups is their own
  // present, not the number of submits before them. Its letter is the next free
  // one rather than its position, because renumbering X onwards would rewrite an
  // `--expect-fail` list that records a measurement.
  group('AC — a stencil reference decides which of two draws survives');

  const PROBE_STENCIL_PIXELS = 64 * 64;
  // `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_STENCIL_*_BYTES`, spelled out
  // here rather than imported, as every expected value in this file is.
  const PROBE_STENCIL_FIRST_BYTES = [60, 130, 200, 255];
  const PROBE_STENCIL_SECOND_BYTES = [200, 70, 110, 255];
  const PROBE_STENCIL_BACKGROUND_BYTES = [30, 90, 150, 255];

  const stencilStart = await evaluate(
    page,
    `(async () => {
     const { startStencilProbe } = await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     return { started: startStencilProbe({ exports }) };
   })()`
  );
  // Poll across frames exactly as group T does.
  const stencil = stencilStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readStencilProbe, pollStencilProbe, STENCIL } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readStencilProbe({ exports, memory: exports.memory });
           if (r.state === STENCIL.FAILED || r.state === STENCIL.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== STENCIL.READY) {
             pollStencilProbe({ exports });
             return null;
           }
           // Ready: classify every texel here rather than shipping 16384 of them
           // out. Which of the three colours came back is the whole verdict, so
           // the counts are what crosses — a run that produced a mixture is a
           // different failure from one that produced the wrong colour evenly,
           // and the message has to be able to say which.
           const want = ${JSON.stringify(PROBE_STENCIL_FIRST_BYTES)};
           const second = ${JSON.stringify(PROBE_STENCIL_SECOND_BYTES)};
           const background = ${JSON.stringify(PROBE_STENCIL_BACKGROUND_BYTES)};
           const same = (at, colour) =>
             r.bytes[at] === colour[0] &&
             r.bytes[at + 1] === colour[1] &&
             r.bytes[at + 2] === colour[2] &&
             r.bytes[at + 3] === colour[3];
           let firstCount = 0;
           let secondCount = 0;
           let backgroundCount = 0;
           let otherCount = 0;
           let firstWrong = -1;
           for (let at = 0; at + 3 < r.bytes.length; at += 4) {
             if (same(at, want)) firstCount += 1;
             else {
               if (firstWrong < 0) firstWrong = at;
               if (same(at, second)) secondCount += 1;
               else if (same(at, background)) backgroundCount += 1;
               else otherCount += 1;
             }
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             firstCount,
             secondCount,
             backgroundCount,
             otherCount,
             firstWrong,
             sample: [...r.bytes.slice(0, 8)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'AC',
    'wasm encoded the stencil setup frame — two targets, the masked pipeline, a reference before each of two draws, the copy, the request',
    stencilStart?.started === true,
    stencilStart?.started
      ? 'the stencil draw-and-read frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'AC',
    'the stencil reference decided which draw survived — the whole target is the first draw’s colour',
    stencil?.done === true &&
      stencil.len === PROBE_STENCIL_PIXELS * 4 &&
      stencil.firstCount === PROBE_STENCIL_PIXELS,
    stencil?.done !== true
      ? `no stencil readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : stencil.firstCount === PROBE_STENCIL_PIXELS
        ? `${stencil.len} bytes, every texel [${PROBE_STENCIL_FIRST_BYTES.join(', ')}] — the 0x11 draw was discarded and the 0x2A one was not`
        : `state ${stencil.state}, ${stencil.len ?? 0} bytes: ` +
          `${stencil.firstCount} first, ${stencil.secondCount} second ` +
          '(the second setStencilReference changed nothing), ' +
          `${stencil.backgroundCount} background ` +
          '(neither did the first, so the pass’s initial 0 decided both), ' +
          `${stencil.otherCount} some other colour; first wrong at byte ${stencil.firstWrong} ` +
          `(sample ${JSON.stringify(stencil.sample)})` +
          `${stencil.error ? ` — ${stencil.error}` : ''}`
  );

  // **THE MSAA-RESOLVE GATE, AND THE ONLY EXERCISE
  // `Capability::MsaaResolveAttachment` HAS ON THIS BACKEND.** The native seam
  // suite that holds the other four backends to that declaration
  // (`exercise_msaa_resolve` in `crates/crcbl/tests/hal_seam_e2e.rs`) is a native
  // binary and cannot open this one, so without this group the `Support::Yes` is a
  // sentence nothing tests. Until it landed, every `ColorAttachment` this backend
  // built anywhere carried `resolve: None` and the replayer's resolve arm was
  // reached only by `web/tools/gpu-replay.mjs`, which has no GPU at all.
  //
  // WHAT IT DOES: wasm records a 4×-multisampled `Rgba8Unorm` target and a
  // single-sampled one of the same size, a `write_buffer` and a buffer→image copy
  // that PRIME the single-sampled one with a poison colour, and a render pass with
  // **no draws and no pipeline** — its only content is a clear of the multisampled
  // target, and its one attachment names the primed target in
  // `ColorAttachment::resolve`. A clear needs no pipeline, and a resolve is an
  // end-of-pass operation over whatever the samples hold, which is why this gate
  // costs no shader. The page loop replays it and the 1024 bytes that come back
  // say whether the resolve ran.
  //
  // **THE OBSERVABLE IS A VALUE, NOT A SURVIVED CALL**, which is what the prime is
  // for: a resolve view that is dropped raises no error — the pass runs, the copy
  // runs, the readback resolves — so a fresh target's undefined contents would
  // decide the verdict by luck. Primed, the three readings mean three things and
  // only one is a pass:
  //
  //   the clear    the resolve reached the target. The only pass.
  //   the poison   the resolve was accepted and never performed. That is the bug
  //                that shipped in `crcbl-wgpu` once, where every attachment was
  //                built with `resolve_target: None` — no error, a wrong picture.
  //   anything     the resolve wrote and wrote the wrong thing, or the copy read
  //   else         the wrong subresource.
  //
  // The two colours are not a channel permutation of each other and their alphas
  // differ, so no swap on the way out can turn one reading into another, and every
  // colour channel is a mid-tone away from the 0 and 255 an untouched or saturated
  // one reads as.
  //
  // **THE SAMPLE COUNT IS ASKED OF THE DEVICE, NOT ASSUMED.** wasm compares the
  // `max_sample_count` the opened device reported against the count it wants and
  // encodes nothing if the device is below it, so the first check below fails
  // naming the number the device gave rather than passing on a single-sampled
  // target that would prove nothing. On every browser today that number is 4:
  // WebGPU specifies `sampleCount` to be exactly 1 or 4 and has no limit to read a
  // larger one from, so `MAX_SAMPLE_COUNT` in `web/engine/gpu-replay.js` is the
  // specification's constant rather than a hardware query — which is precisely why
  // the check has to say what came back instead of assuming what it will be.
  //
  // WHERE IT BELONGS IN THE RUN: **before X**, group AC's reason exactly. X, Y, Z
  // and AA are expected to fail on Windows because the page has one `GPUDevice`
  // and therefore one queue, and X's stuck canvas readback strands every
  // `mapAsync` queued after it. This group reads bytes back, so behind X it would
  // be stranded too — and it is not in that platform's `--expect-fail` list, so it
  // would turn the Windows job red. Ahead of X it resolves like AC does, and it
  // makes X no worse: what strands the canvas groups is their own present, not the
  // number of submits before them. Its letter is the next free one rather than its
  // position, because renumbering X onwards would rewrite an `--expect-fail` list
  // that records a measurement.
  group(
    'AD — a multisampled clear resolves into the single-sampled target it named'
  );

  // `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_MSAA_*`, spelled out here rather
  // than imported, as every expected value in this file is.
  const PROBE_MSAA_SAMPLES = 4;
  const PROBE_MSAA_TEXELS = 64 * 4;
  const PROBE_MSAA_CLEAR_BYTES = [75, 160, 115, 255];
  const PROBE_MSAA_POISON_BYTES = [165, 60, 210, 17];

  const msaaStart = await evaluate(
    page,
    `(async () => {
     const { startMsaaProbe, msaaSampleCount } =
       await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     // The count first: once the request is refused it is the only thing that
     // says why, and reading it afterwards would read the same number anyway.
     const samples = msaaSampleCount({ exports });
     return { samples, started: startMsaaProbe({ exports }) };
   })()`
  );
  // Poll across frames exactly as group AC does.
  const msaa = msaaStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readMsaaProbe, pollMsaaProbe, MSAA } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readMsaaProbe({ exports, memory: exports.memory });
           if (
             r.state === MSAA.FAILED ||
             r.state === MSAA.UNDECODABLE ||
             r.state === MSAA.UNSUPPORTED
           ) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== MSAA.READY) {
             pollMsaaProbe({ exports });
             return null;
           }
           // Ready: classify every texel here rather than shipping 256 of them
           // out. Which of the two colours came back is the whole verdict, so the
           // counts are what crosses — a run that resolved part of the target is a
           // different failure from one that resolved none of it, and the message
           // has to be able to say which.
           const want = ${JSON.stringify(PROBE_MSAA_CLEAR_BYTES)};
           const poison = ${JSON.stringify(PROBE_MSAA_POISON_BYTES)};
           const same = (at, colour) =>
             r.bytes[at] === colour[0] &&
             r.bytes[at + 1] === colour[1] &&
             r.bytes[at + 2] === colour[2] &&
             r.bytes[at + 3] === colour[3];
           let clearCount = 0;
           let poisonCount = 0;
           let otherCount = 0;
           let firstWrong = -1;
           for (let at = 0; at + 3 < r.bytes.length; at += 4) {
             if (same(at, want)) clearCount += 1;
             else {
               if (firstWrong < 0) firstWrong = at;
               if (same(at, poison)) poisonCount += 1;
               else otherCount += 1;
             }
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             clearCount,
             poisonCount,
             otherCount,
             firstWrong,
             sample: [...r.bytes.slice(0, 8)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'AD',
    'the device serves a multisampled colour target, and wasm encoded the resolve frame — two targets, the prime, a clear-only pass naming the resolve view, the copy, the request',
    msaaStart?.started === true,
    msaaStart?.started
      ? `max_sample_count ${msaaStart.samples}, and the clear-and-resolve frame is on the stream`
      : `wasm would not encode it — the device reported max_sample_count ${msaaStart?.samples ?? 'nothing'}, ` +
          `and this probe resolves a ${PROBE_MSAA_SAMPLES}× target; or no device has opened, or another channel is installed. ` +
          'A resolve declared supported on a device with no multisampled target to resolve from is the finding, not an excuse'
  );
  check(
    'AD',
    'the multisampled clear reached the single-sampled target it resolved into — every texel is the clear, none is the poison it was primed with',
    msaa?.done === true &&
      msaa.len === PROBE_MSAA_TEXELS * 4 &&
      msaa.clearCount === PROBE_MSAA_TEXELS,
    msaa?.done !== true
      ? `no msaa readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : msaa.clearCount === PROBE_MSAA_TEXELS
        ? `${msaa.len} bytes, every texel [${PROBE_MSAA_CLEAR_BYTES.join(', ')}] — the resolve overwrote the [${PROBE_MSAA_POISON_BYTES.join(', ')}] prime`
        : `state ${msaa.state}, ${msaa.len ?? 0} bytes: ` +
          `${msaa.clearCount} the clear, ${msaa.poisonCount} still the poison ` +
          '(the resolve view was accepted and never performed), ' +
          `${msaa.otherCount} some other colour; first wrong at byte ${msaa.firstWrong} ` +
          `(sample ${JSON.stringify(msaa.sample)})` +
          `${msaa.error ? ` — ${msaa.error}` : ''}`
  );

  // **THE QUERY GATE, AND THE ONLY EXERCISE `Capability::OcclusionQuery` HAS ON
  // THIS BACKEND.** The native seam suite that holds the other four backends to
  // that declaration (`exercise_query_set_creation` in
  // `crates/crcbl/tests/hal_seam_e2e.rs`) is a native binary and cannot open this
  // one, so without this group the `Support::Yes` is a sentence nothing tests.
  //
  // WHAT THE CAPABILITY CLAIMS, AND IT IS WORTH BEING EXACT: a
  // `QueryKind::Occlusion` query set, and nothing more.
  // `crcbl_hal::CommandEncoder` has no begin/end query verb — its whole query
  // vocabulary is the reset, the timestamp write and the resolve — so nothing a
  // caller records through this seam can ever *write* an occlusion query, here or
  // on the Vulkan backend whose `Yes` means the same thing. There is no count to
  // be plausible about, and this group does not pretend there is one.
  //
  // WHAT IT DOES: wasm records a 32-query occlusion set, a `QUERY_RESOLVE`
  // destination filled with a sentinel byte, the seam's `reset_query_set` over
  // the whole range (which WebGPU has no call for and the replayer records
  // nothing for), a `resolve_query_set` over that sentinel, a copy into a
  // host-readable buffer, and the readback — plus a `query_results` ask on the
  // same queries, which the replayer serves through a resolve, a copy and a map
  // of its own because a `GPUQuerySet` has no accessor. The page loop replays it,
  // and the 256 bytes that come back each way say what happened.
  //
  // **THE SENTINEL IS WHAT MAKES THE ZERO MEAN SOMETHING**, and that is the whole
  // design constraint: an unwritten query resolves to zero, and zero is also what
  // an untouched allocation reads as, so "all zero" on its own is the answer a
  // replayer that did nothing would give. Primed, the two readings mean two
  // things and only one is a pass:
  //
  //   every byte zero        the resolve ran, over a set the browser really
  //                          created, and overwrote the sentinel. The only pass.
  //   the sentinel survives  the resolve was dropped or refused. A WebGPU
  //                          validation error is reported out of band and the
  //                          copy still runs, so this is exactly what a refused
  //                          resolve looks like from here.
  //
  // **THE ZERO IS THE IMPLEMENTATION'S, NOT THE SPECIFICATION'S.**
  // `gpuweb/gpuweb#1072` opened the question of whether resolving a never-begun
  // query should be disallowed or should follow D3D12 and Metal in producing 0,
  // and `resolveQuerySet`'s published validation rules — a 256-aligned
  // `destinationOffset`, a `QUERY_RESOLVE` destination, a range inside the set —
  // say nothing about the value. This gate runs under Chromium on all three
  // platforms (`web/run-probe-e2e.sh` says so), so what it asserts is one
  // implementation's answer; a browser that answered otherwise fails here naming
  // the value it gave rather than passing quietly.
  //
  // AND THE SECOND PATH IS NOT A SECOND ASSERTION OF THE FIRST. The readback
  // reads bytes a resolve *wasm recorded* wrote into a buffer wasm owns; the
  // direct read is `Device::query_results`, which reaches the same queries
  // through machinery the replayer builds for itself. Two mechanisms agreeing on
  // one answer is what neither of them alone can say.
  //
  // WHERE IT BELONGS IN THE RUN: **before X**, groups AC and AD's reason exactly.
  // X, Y, Z and AA are expected to fail on Windows because the page has one
  // `GPUDevice` and therefore one queue, and X's stuck canvas readback strands
  // every `mapAsync` queued after it. This group reads bytes back — twice, since
  // the direct read is a `mapAsync` of the replayer's own — so behind X it would
  // be stranded too, and it is not in that platform's `--expect-fail` list. Ahead
  // of X it resolves like AC and AD do, and it makes X no worse: what strands the
  // canvas groups is their own present, not the number of submits before them.
  // Its letter is the next free one rather than its position, because renumbering
  // X onwards would rewrite an `--expect-fail` list that records a measurement.
  group('AE — an occlusion query set is built, resolved and read back');

  // `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_OCCLUSION_*`, spelled out here
  // rather than imported, as every expected value in this file is.
  const PROBE_OCCLUSION_QUERIES = 32;
  const PROBE_OCCLUSION_BYTES = PROBE_OCCLUSION_QUERIES * 8;
  const PROBE_OCCLUSION_SENTINEL = 0xa7;

  const occlusionStart = await evaluate(
    page,
    `(async () => {
     const { startOcclusionProbe } = await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     return { started: startOcclusionProbe({ exports }) };
   })()`
  );
  // Poll across frames exactly as group AC does, and wait for BOTH answers: the
  // readback's poll settles on its own schedule and the direct read settles when
  // the replayer's map does, so a run that took only the first would report on
  // whichever arrived earlier.
  const occlusion = occlusionStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readOcclusionProbe, readOcclusionValues, pollOcclusionProbe,
                   OCCLUSION, OCCLUSION_VALUES } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readOcclusionProbe({ exports, memory: exports.memory });
           if (r.state === OCCLUSION.FAILED || r.state === OCCLUSION.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           const v = readOcclusionValues({ exports, memory: exports.memory });
           if (r.state !== OCCLUSION.READY ||
               v.state !== OCCLUSION_VALUES.READY) {
             pollOcclusionProbe({ exports });
             return null;
           }
           // Both are in: classify every byte here rather than shipping 512 of
           // them out. Which of the two readings came back is the whole verdict,
           // so the counts are what crosses — a run that overwrote part of the
           // destination is a different failure from one that overwrote none of
           // it, and the message has to be able to say which.
           const classify = (bytes) => {
             let zero = 0;
             let sentinel = 0;
             let other = 0;
             let firstWrong = -1;
             for (let at = 0; at < bytes.length; at += 1) {
               if (bytes[at] === 0) zero += 1;
               else {
                 if (firstWrong < 0) firstWrong = at;
                 if (bytes[at] === ${PROBE_OCCLUSION_SENTINEL}) sentinel += 1;
                 else other += 1;
               }
             }
             return { len: bytes.length, zero, sentinel, other, firstWrong };
           };
           return {
             done: true,
             state: r.name,
             valuesState: v.name,
             readback: classify(r.bytes),
             values: classify(v.bytes),
             sample: [...r.bytes.slice(0, 8)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'AE',
    'wasm encoded the occlusion setup frame — the set, the sentinel, the reset, the resolve, the copy, the request, and the direct read beside it',
    occlusionStart?.started === true,
    occlusionStart?.started
      ? 'the query set-and-resolve frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'AE',
    'the resolve reached the destination it named — every resolved byte is zero and none is the sentinel it was primed with',
    occlusion?.done === true &&
      occlusion.readback?.len === PROBE_OCCLUSION_BYTES &&
      occlusion.readback.zero === PROBE_OCCLUSION_BYTES,
    occlusion?.done !== true
      ? `no occlusion readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : occlusion.readback?.zero === PROBE_OCCLUSION_BYTES
        ? `${occlusion.readback.len} bytes, every one zero — the resolve overwrote all ${PROBE_OCCLUSION_QUERIES} queries' worth of the 0x${PROBE_OCCLUSION_SENTINEL.toString(16)} sentinel`
        : `state ${occlusion.state}, ${occlusion.readback?.len ?? 0} bytes: ` +
          `${occlusion.readback?.zero ?? 0} zero, ${occlusion.readback?.sentinel ?? 0} still the sentinel ` +
          '(the resolve was dropped or refused), ' +
          `${occlusion.readback?.other ?? 0} some other value; first wrong at byte ${occlusion.readback?.firstWrong ?? -1} ` +
          `(sample ${JSON.stringify(occlusion.sample)})` +
          `${occlusion.error ? ` — ${occlusion.error}` : ''}`
  );
  check(
    'AE',
    'Device::query_results read the same set the other way and answered the same values — one u64 per query, every one zero',
    occlusion?.done === true &&
      occlusion.values?.len === PROBE_OCCLUSION_BYTES &&
      occlusion.values.zero === PROBE_OCCLUSION_BYTES,
    occlusion?.done !== true
      ? `no direct read in ${TIMEOUT_MS} ms — the replayer's own map never resolved or the reply never reached wasm`
      : occlusion.values?.len === 0
        ? `state ${occlusion.valuesState} with no values, which is the only way a QueryResults reply says the read could not be served${occlusion.error ? ` — ${occlusion.error}` : ''}`
        : occlusion.values?.zero === PROBE_OCCLUSION_BYTES
          ? `${occlusion.values.len} bytes, every one zero — the same answer the resolve wrote, through machinery the replayer built for itself`
          : `state ${occlusion.valuesState}, ${occlusion.values?.len ?? 0} bytes: ` +
            `${occlusion.values?.zero ?? 0} zero, ${occlusion.values?.other ?? 0} something else; ` +
            `first wrong at byte ${occlusion.values?.firstWrong ?? -1}` +
            `${occlusion.error ? ` — ${occlusion.error}` : ''}`
  );

  // **THE TIMESTAMP GATE, AND THE ONLY EXERCISE `Capability::TimestampQuery` HAS
  // ON THIS BACKEND.** Group AE's standing exactly: the native seam suite's
  // `exercise_timestamp_query` holds the other backends to the same declaration
  // and cannot open this one, so without this group the `Support::Yes` is a
  // sentence nothing tests.
  //
  // WHAT IT IS ABOUT. This capability was refused on this backend until the seam
  // moved its timestamps into the pass descriptor, and the reason is worth
  // restating because it is what the checks below are shaped around: WebGPU
  // takes a timestamp **only** through a render or compute pass's
  // `timestampWrites`, and the seam used to have a free-standing
  // `write_timestamp` naming an arbitrary point in the command stream. A set
  // created for a verb that could never reach the browser is a handle a profiler
  // would fill with zeros and report as timings. So the failure this group has
  // to be able to see is not an error — it is **two zeros**.
  //
  // WHAT IT DOES: wasm records a two-query `'timestamp'` set, the seam's reset,
  // an empty compute pass whose descriptor names both queries, a submit, and a
  // `query_results` ask that reads them. The pass is empty on purpose — what is
  // being observed is whether the browser writes the two queries at all, not how
  // long anything took, and a pass with work in it would need a pipeline and a
  // shader the claim does not.
  //
  // WHAT THE VALUES MEAN. An unwritten query resolves to zero by specification,
  // so:
  //
  //   two non-zero ticks     the browser honoured `timestampWrites`. The pass.
  //   two zeros              the descriptor was taken and neither query written
  //                          — the outcome the refusal existed to prevent.
  //   no values at all       the replayer could not serve the read, which is the
  //                          only thing an empty `QueryResults` reply can mean.
  //
  // **THE ORDERING IS ASSERTED AND THE DURATION IS NOT.** `end >= start` is the
  // one relationship a clock obeys whatever it counts. A *strict* inequality is
  // not asserted here although the native suite asserts one, because a browser
  // is free to quantise its timestamp-query results for privacy and an empty
  // pass can then legitimately open and close inside one quantum and report the
  // same tick twice. The native suite times a pass that clears a megabyte on
  // hardware that does not quantise, and asserts the strict form there.
  //
  // WHAT WAS ACTUALLY MEASURED HERE, AND WHAT WAS NOT. Group AL's sweep on
  // 2026-08-23 found **no quantisation on either adapter of the machine it ran
  // on**: over 288 distinct ticks on SwiftShader the greatest common divisor of
  // the gaps between adjacent ones was 1 ns, over 320 on Chromium's
  // `amd rdna-3` every intra-frame value sat on a 40 ns grid, and no empty pass
  // in 152 samples reported zero. Passing
  // `--enable-dawn-features=timestamp_quantization` explicitly changed nothing.
  // **The mechanism was not established** — the suspicion is
  // `--enable-unsafe-webgpu`, which `browserFlags` passes on every platform and
  // every adapter mode, but only the outcome is known, and nothing here should
  // be read as a claim about which toggle decides it. Neither is any of this
  // measured on CI's runners, whose browsers may well quantise; that is why the
  // check below stays non-strict and why group AL compares two spans with each
  // other instead of either with a number.
  //
  // WHERE IT BELONGS IN THE RUN: beside AE and **before X**, for AE's reason —
  // the read is a `mapAsync` of the replayer's own making, so behind X's stuck
  // canvas readback it would be stranded, and it is not in any platform's
  // `--expect-fail` list.
  group('AF — a pass is timed through its own descriptor');

  const timestampStart = await evaluate(
    page,
    `(async () => {
     const { startTimestampProbe, timestampSupported } =
       await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     // Supported first: it is what tells a browser that cannot serve a
     // timestamp set from a request that never happened.
     const supported = timestampSupported({ exports });
     return { supported, started: startTimestampProbe({ exports }) };
   })()`
  );
  const timestamp = timestampStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readTimestampProbe, TIMESTAMP } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readTimestampProbe({ exports, memory: exports.memory });
           if (r.state !== TIMESTAMP.READY) return null;
           // Read the two ticks here rather than shipping the bytes out: a
           // BigInt does not survive the DevTools JSON round trip, so they
           // cross as decimal strings and as the two facts asserted on.
           const view = new DataView(r.bytes.buffer, r.bytes.byteOffset,
                                     r.bytes.byteLength);
           const ticks = [];
           for (let at = 0; at + 8 <= r.bytes.byteLength; at += 8) {
             ticks.push(view.getBigUint64(at, true));
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.byteLength,
             ticks: ticks.map(String),
             bothNonZero: ticks.length === 2 && ticks.every((t) => t !== 0n),
             ordered: ticks.length === 2 && ticks[1] >= ticks[0],
             delta: ticks.length === 2 ? String(ticks[1] - ticks[0]) : null,
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  // **A BROWSER WITHOUT `timestamp-query` OWES THIS GROUP NOTHING.** The
  // capability is `NotOnThisDevice` there, which the seam treats as an honest
  // answer and not a refusal the backend chose — and CI's macOS runner is
  // exactly that browser, on a paravirtual Metal device that advertises no
  // counter set. Failing it would report a device's limit as this page's bug.
  //
  // The checks below keep their teeth everywhere the feature exists, which is
  // both other platforms this job runs on: SwiftShader grants it on Linux and
  // on Windows, and those two arms are what red-checking this group proves out.
  // A browser that grants the feature and then writes nothing still fails.
  const timestampAbsent = timestampStart?.supported === false;
  const absentDetail =
    "this browser opened a device without 'timestamp-query', so the capability " +
    'is NotOnThisDevice here and there is nothing to hold it to';
  check(
    'AF',
    "wasm encoded the timed frame — a 'timestamp' query set, the reset, and a compute pass naming both of its queries",
    timestampStart?.started === true || timestampAbsent,
    timestampStart?.started
      ? 'the timed pass and the read of its two queries are on the stream'
      : timestampStart?.supported === false
        ? "this browser opened a device without 'timestamp-query', so no GPUQuerySet of that " +
          'type could exist — the capability is declared NotOnThisDevice here and there is ' +
          'nothing this page can hold it to'
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'AF',
    'the browser wrote both queries the pass named — two ticks, and neither of them the zero an unwritten query resolves to',
    timestampAbsent ||
      (timestamp?.done === true &&
        timestamp.len === 16 &&
        timestamp.bothNonZero),
    timestampAbsent
      ? absentDetail
      : timestamp?.done !== true
        ? `no answer in ${TIMEOUT_MS} ms — the replayer's own map never resolved or the reply never reached wasm`
        : timestamp.len === 0
          ? `state ${timestamp.state} with no values, which is the only way a QueryResults reply says the read could not be served${timestamp.error ? ` — ${timestamp.error}` : ''}`
          : timestamp.bothNonZero
            ? `${timestamp.ticks[0]} then ${timestamp.ticks[1]}`
            : `the pass reported ${JSON.stringify(timestamp.ticks)} — a zero is a query nothing wrote, ` +
              'which is a pass whose timestampWrites were accepted and dropped' +
              `${timestamp.error ? ` — ${timestamp.error}` : ''}`
  );
  check(
    'AF',
    'and the closing tick is not before the opening one, so the two came from a clock',
    timestampAbsent || (timestamp?.done === true && timestamp.ordered),
    timestampAbsent
      ? absentDetail
      : timestamp?.done !== true
        ? 'no ticks to order'
        : timestamp.ordered
          ? `${timestamp.delta} ns between the two boundaries of an empty pass` +
            ' (a browser reports nanoseconds, quantised)'
          : `the pass opened at ${timestamp.ticks?.[0]} and closed at ${timestamp.ticks?.[1]}, ` +
            'which is a clock running backwards or two queries read in the wrong order'
  );

  // **THE CONTRAST GATE FOR `Features::TIMESTAMP_QUERY`, AND THE ONLY EXERCISE
  // ANYWHERE SHOWING THAT ITS NUMBERS TRACK WORK.** Group AF establishes that
  // the browser writes the two queries a pass names. It cannot go on to ask what
  // the *difference* between two ticks means, because it has one difference and
  // nothing to hold it against — so until this group the capability was declared
  // on the strength of two ticks that were merely present and ordered.
  //
  // WHAT IT DOES: wasm submits two submissions, each holding a run of empty
  // compute passes and a run of passes that dispatch PASS_SPAN_WORKGROUPS
  // workgroups of a dependent-arithmetic loop, every one timed through its own
  // descriptor — then reads every boundary back with one `query_results`. The
  // two kinds of pass differ in the dispatch and in nothing else: same label,
  // same set, same submission, adjacent in the same command buffer.
  //
  // **THE CLAIM IS A SEPARATION, NOT A DURATION, AND THAT IS THE WHOLE DESIGN.**
  // No nanosecond constant enters any assertion here. Every busy span is held
  // against every idle span *from the same frame*, so the two sides come off one
  // clock and a browser that rounds its timestamps for privacy rounds both. An
  // absolute figure — "at least a microsecond", "never zero" — would be a claim
  // about a browser's quantisation wearing this seam's clothes, and the empty
  // passes are exactly where a coarse quantiser lands on zero.
  //
  // WHAT WAS MEASURED, AND WHERE THE RUNG COMES FROM. Swept on 2026-08-23 over
  // workgroup counts 0…65535, eight passes of each kind per rung, on the two
  // adapters of one developer machine. At 4096 workgroups the smallest busy span
  // beat the largest idle span by **229×** on the hardware adapter (Chromium's
  // `amd rdna-3`, an RX 7900 XTX: idle 3360–3440 ns, busy 788040–789600 ns) and
  // by **38297×** on SwiftShader (idle 251–2705 ns, busy 103593401–109525860
  // ns). Those figures are the evidence for choosing 4096 and are checked by
  // nothing — the assertion compares the two sides and names no number. The rung
  // above it, 16384, buys a fatter hardware margin and costs SwiftShader 4.6 s
  // of wall clock, which is the wrong trade for the runner CI's Linux job has.
  //
  // **THE QUANTISATION ON CI'S RUNNERS IS UNMEASURED.** The same sweep found
  // none on either adapter it ran on — see group AF's comment for the numbers
  // and for what that does not establish — but those are two adapters on one
  // machine. A runner whose browser *does* quantise still passes the separation
  // check, because rounding moves both sides; what stops describing that runner
  // is the pair of margins above. The checks are the claim, the margins are
  // provenance, and nobody has read the margins off a runner.
  //
  // WHICH RUNNER TAKES WHICH BRANCH, read off the Pages run of `ab706a0` on
  // 2026-08-23 rather than reasoned about: **macOS opens a device without
  // `timestamp-query` and takes the absent branch every run**, Windows opens one
  // with it and takes the present branch, and Linux runs SwiftShader, which
  // grants it. That is a fact about runner images and about the Apple
  // Paravirtual device behind them, so it is dated — `ci.yml` already records
  // that macOS runner images differ between versions on exactly this kind of
  // question.
  //
  // Which is why the absent branch below is a **check and not an excuse**. It is
  // load-bearing here in a way group AK's is not: taken on a third of the
  // matrix, every run.
  //
  // WHERE IT BELONGS IN THE RUN: beside AF and **before X**, for AF's reason —
  // the read is a `mapAsync` of the replayer's own making, so behind X's stuck
  // canvas readback it would be stranded.
  group('AL — a pass that dispatches work outlasts the empty one beside it');

  // How many workgroups each busy pass dispatches. The one number in this group
  // chosen from a measurement rather than from an argument — see the block
  // above — and it is the driver's rather than wasm's for the reason
  // PROBE_BUFFER_BYTES is: a count fixed in `probe.rs` would be a constant
  // checked against itself.
  const PASS_SPAN_WORKGROUPS = 4096;

  const passSpanStart = await evaluate(
    page,
    `(async () => {
     const { startPassSpanProbe, passSpanSupported } =
       await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     // Supported first: it is what tells a device that withheld the feature
     // from a request that never happened.
     const supported = passSpanSupported({ exports });
     // **THE ABSENT BRANCH'S OWN CLAIM.** Ask the real device for the object
     // this whole probe is built on, inside a validation scope so the answer is
     // caught rather than reported as an uncaptured error. What a device without
     // the feature refuses is a 'timestamp' set of *any* size, so the smallest
     // one is what is asked for — the two-query set group AF creates. Either way
     // this is the browser answering, not wasm.
     const device = gpu.replayer.device;
     let refusal = null;
     let asked = false;
     if (device) {
       asked = true;
       device.pushErrorScope('validation');
       let set = null;
       try {
         set = device.createQuerySet({ type: 'timestamp', count: 2 });
       } catch (error) {
         refusal = String(error?.message ?? error);
       }
       const scoped = await device.popErrorScope();
       if (scoped) refusal = scoped.message;
       try {
         set?.destroy();
       } catch {
         // An invalid query set may refuse to be destroyed; it is going away
         // with the device either way and this is not what is under test.
       }
     }
     return {
       supported,
       asked,
       refusal,
       started: startPassSpanProbe({
         exports,
         workgroups: ${PASS_SPAN_WORKGROUPS},
       }),
     };
   })()`
  );
  const passSpan = passSpanStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readPassSpanProbe, PASS_SPAN } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readPassSpanProbe({ exports, memory: exports.memory });
           if (r.state !== PASS_SPAN.READY) return null;
           // Decimal strings, for group AF's reason: a BigInt does not survive
           // the DevTools JSON round trip.
           const view = new DataView(r.bytes.buffer, r.bytes.byteOffset,
                                     r.bytes.byteLength);
           const ticks = [];
           for (let at = 0; at + 8 <= r.bytes.byteLength; at += 8) {
             ticks.push(view.getBigUint64(at, true));
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.byteLength,
             ticks: ticks.map(String),
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;

  // The layout `probe_pass_span_query` lays down, read back: four runs in encode
  // order — the first submission's empty passes, its busy ones, then the second
  // submission's two runs — each holding one pair of boundaries per pass. Every
  // count comes off the array's own length, so nothing here restates a constant
  // `probe.rs` owns, and a probe that came back with a different number of
  // queries is read as the layout it actually sent.
  const PASS_SPAN_RUNS = [
    'submit 0 idle',
    'submit 0 busy',
    'submit 1 idle',
    'submit 1 busy',
  ];
  /** @param {{ ticks: Array<string> } | null} answer */
  function passSpanReading(answer) {
    if (answer?.done !== true || answer.ticks.length === 0) return null;
    const values = answer.ticks.map((tick) => BigInt(tick));
    if (values.length % (PASS_SPAN_RUNS.length * 2) !== 0) return null;
    const perRun = values.length / PASS_SPAN_RUNS.length;
    const idle = [];
    const busy = [];
    PASS_SPAN_RUNS.forEach((name, run) => {
      const slice = values.slice(run * perRun, (run + 1) * perRun);
      const spans = name.endsWith('busy') ? busy : idle;
      for (let at = 0; at + 1 < slice.length; at += 2) {
        spans.push(slice[at + 1] - slice[at]);
      }
    });
    // Non-decreasing over the WHOLE array, which is what makes this one claim
    // cover three things at once: the two boundaries of each pass, consecutive
    // passes inside a submission, and the submission boundary in the middle.
    let ordered = true;
    let firstBackwards = -1;
    for (let at = 1; at < values.length; at += 1) {
      if (values[at] < values[at - 1] && ordered) {
        ordered = false;
        firstBackwards = at;
      }
    }
    const max = (list) => list.reduce((a, b) => (b > a ? b : a));
    const min = (list) => list.reduce((a, b) => (b < a ? b : a));
    return {
      values,
      idle,
      busy,
      ordered,
      firstBackwards,
      idleMax: max(idle),
      idleMin: min(idle),
      busyMax: max(busy),
      busyMin: min(busy),
      separated: min(busy) > max(idle),
    };
  }
  const passSpanRead = passSpanReading(passSpan);

  // A browser without `timestamp-query` cannot be held to the two measurement
  // checks — there is no set to time a pass with — and CI's macOS runner is that
  // browser on every run. What it *is* held to is the last check in this group,
  // which runs everywhere and can fail on either side.
  const passSpanAbsent = passSpanStart?.supported === false;
  const passSpanAbsentDetail =
    "this browser opened a device without 'timestamp-query', so there is no " +
    'timestamp set to time a pass with — the last check in this group is what ' +
    'holds that answer to the browser rather than excusing it';
  check(
    'AL',
    'wasm encoded the measuring frame — two submissions, each of empty compute passes and passes that dispatch a loop, every one timed through its own descriptor',
    passSpanStart?.started === true || passSpanAbsent,
    passSpanStart?.started
      ? `the ${PASS_SPAN_WORKGROUPS}-workgroup frame and the read of every boundary are on the stream`
      : passSpanAbsent
        ? passSpanAbsentDetail
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'AL',
    'the browser timed the work — every pass that dispatched the loop spans more of its own clock than every empty pass in the same frame',
    passSpanAbsent || passSpanRead?.separated === true,
    passSpanAbsent
      ? passSpanAbsentDetail
      : passSpan?.done !== true
        ? `no answer in ${TIMEOUT_MS} ms — the replayer's own map never resolved or the reply never reached wasm`
        : passSpanRead === null
          ? `state ${passSpan.state} with ${passSpan.len} bytes, which is neither a whole set of boundaries nor a read this page can lay out${passSpan.error ? ` — ${passSpan.error}` : ''}`
          : passSpanRead.separated
            ? `${passSpanRead.busy.length} busy passes span ${passSpanRead.busyMin}–${passSpanRead.busyMax} and ${passSpanRead.idle.length} empty ones ${passSpanRead.idleMin}–${passSpanRead.idleMax}, so the smallest measured piece of work is ${passSpanRead.busyMin / passSpanRead.idleMax}× the largest measured piece of nothing`
            : `busy spans ${passSpanRead.busyMin}–${passSpanRead.busyMax} against empty ones ${passSpanRead.idleMin}–${passSpanRead.idleMax} — the two overlap, which is a clock that did not resolve the dispatch or a dispatch that did not happen` +
              `${passSpan.error ? ` — ${passSpan.error}` : ''}`
  );
  check(
    'AL',
    'and no boundary is before the one recorded ahead of it — across the two of a pass, across consecutive passes, and across the submission in the middle',
    passSpanAbsent || passSpanRead?.ordered === true,
    passSpanAbsent
      ? passSpanAbsentDetail
      : passSpanRead === null
        ? 'no boundaries to order'
        : passSpanRead.ordered
          ? `${passSpanRead.values.length} boundaries rise from ${passSpanRead.values[0]} to ${passSpanRead.values.at(-1)}, over two submissions`
          : `boundary ${passSpanRead.firstBackwards} reads ${passSpanRead.values[passSpanRead.firstBackwards]} after ${passSpanRead.values[passSpanRead.firstBackwards - 1]}, which is a clock running backwards or a set read in the wrong order`
  );
  // **THE CHECK THAT MAKES THE ABSENT BRANCH A CHECK**, on group AK's terms —
  // and unlike AK's, this one has a runner that really takes the absent side:
  // macOS, every run, as of the date in the block above. Every platform runs it
  // and it can fail on either side: a device that lacks the feature must refuse
  // the query set this probe is built on, and a device that has it must not. It
  // is what stops "not supported here" arriving as "passed".
  const passSpanRefused =
    passSpanStart?.refusal !== null && passSpanStart?.refusal !== undefined;
  check(
    'AL',
    "the browser's own answer about a 'timestamp' query set is the one wasm acted on — the real device refuses one where the feature is absent and accepts one where it is present",
    passSpanStart?.asked === true &&
      passSpanRefused === !passSpanStart?.supported,
    passSpanStart?.asked !== true
      ? 'no GPUDevice to ask — the replayer has not opened one, so this proves nothing and ' +
          'fails rather than being skipped'
      : passSpanRefused === !passSpanStart?.supported
        ? passSpanStart.supported
          ? 'the device has the feature, wasm encoded the frame, and createQuerySet accepted a ' +
            "'timestamp' set with no validation error"
          : 'the device lacks the feature, wasm encoded nothing, and createQuerySet was refused ' +
            `by the browser — ${passSpanStart.refusal}`
        : passSpanStart.supported
          ? 'wasm reports the device HAS timestamp-query and the browser refused a timestamp ' +
            `query set anyway — ${passSpanStart.refusal}`
          : 'wasm reports the device WITHOUT timestamp-query and the browser created a ' +
            'timestamp query set regardless, so the probe skipped a frame this device could ' +
            'have run — which is exactly the shape that reads as covered while proving nothing'
  );

  // **THE DEPTH-CLAMP GATE, AND THE ONLY PLACE `Features::DEPTH_CLAMP` IS EVER
  // TESTED AGAINST A GPU.** This crate reports that feature to callers off the
  // browser's `depth-clip-control`, and until this group existed no
  // Rust-originated `CreateGraphicsPipeline` had ever carried
  // `primitive.depth_clamp` set: `PROBE_GRAPHICS_PIPELINE_DESC` keeps it `false`
  // so the pipeline builds on a device that withheld the feature, and the `true`
  // path was exercised only by `web/tools/gpu-replay.mjs` under node, against a
  // stub device that records the descriptor it is handed. A recorded descriptor
  // is not a clamped fragment, and the capability was reported on the strength of
  // one.
  //
  // WHAT IT DOES: wasm records two `Rgba8Unorm` targets of the same size, two
  // pipelines that differ in `depth_clamp` and in nothing else, and a render pass
  // into each — same shader, same oversized triangle, same clear. The triangle is
  // emitted at a clip-space `z` **past the far plane** with `w` at 1, so it is
  // outside the depth range every fragment of it. Both targets are copied into one
  // buffer, the clamped one's block first, and the page loop replays it.
  //
  // **THE OBSERVABLE IS A DIFFERENCE IN THE PICTURE.** Depth clipping is what
  // separates the two blocks: with `unclippedDepth` the primitive is not clipped
  // against near and far and rasterises, without it the primitive is discarded
  // before it reaches a fragment. So the pair of readings is the claim, and only
  // one pair is a pass:
  //
  //   draw,  clear   the flag reached the browser and the GPU honoured it. The
  //                  only pass.
  //   clear, clear   the flag reached nothing — dropped on the wire, or taken and
  //                  ignored. Both pipelines behaved as the control.
  //   draw,  draw    nothing was clipped anywhere, so the triangle is not outside
  //                  the range after all and the clamped block means nothing.
  //                  That is this probe's own fixture failing, and it is why the
  //                  control is here: without it, geometry that quietly landed
  //                  inside the range would paint both blocks and read as a pass.
  //   clear, draw    inverted — the pipelines, the passes or the copies were
  //                  crossed.
  //
  // "The pipeline was created without error" is deliberately not the check, and
  // neither is "`unclippedDepth` appeared in the descriptor": the first proves a
  // browser accepted a descriptor and the second is what the node stub already
  // does. What is asserted here is which texels came back.
  //
  // **A BROWSER WITHOUT `depth-clip-control` OWES THIS GROUP NOTHING — BUT IT
  // HAS TO BE THE BROWSER THAT SAID SO.** WebGPU refuses `unclippedDepth` on a
  // device that did not enable it, so wasm encodes nothing there and the probe
  // answers `UNSUPPORTED` by name. That excuse is only honest if the adapter
  // really lacks the feature, so the page asks `navigator.gpu` for itself and the
  // two answers are held together: an adapter that HAS `depth-clip-control` while
  // wasm reports it unsupported is a device request that lost the optional
  // feature, and it fails here rather than being waved through as a platform
  // limit. That is the "not supported here arriving as passed" failure, and it is
  // the one this shape exists to keep out.
  //
  // WHERE IT BELONGS IN THE RUN: **before X**, group AC's reason exactly. X, Y, Z
  // and AA are expected to fail on Windows because the page has one `GPUDevice`
  // and therefore one queue, and X's stuck canvas readback strands every
  // `mapAsync` queued after it. This group reads bytes back, so behind X it would
  // be stranded too — and it is not in that platform's `--expect-fail` list, so it
  // would turn the Windows job red.
  group(
    'AH — a triangle past the far plane survives the clamped pipeline and not the one beside it'
  );

  const PROBE_CLAMP_PIXELS = 64 * 64;
  // `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_CLAMP_*_BYTES`, spelled out here
  // rather than imported, as every expected value in this file is.
  const PROBE_CLAMP_CLEAR_BYTES = [45, 125, 85, 255];
  const PROBE_CLAMP_DRAW_BYTES = [215, 95, 35, 240];

  const clampStart = await evaluate(
    page,
    `(async () => {
     const { startClampProbe, clampSupported } =
       await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     // Supported first: it is what tells a device that withheld the feature
     // from a request that never happened.
     const supported = clampSupported({ exports });
     // And what the browser itself says, so the excuse below cannot be given
     // by the thing under test. Asked of navigator.gpu, not of anything of ours.
     const adapter = await navigator.gpu.requestAdapter();
     const adapterHas = adapter?.features?.has('depth-clip-control') === true;
     return { supported, adapterHas, started: startClampProbe({ exports }) };
   })()`
  );
  // Poll across frames exactly as group AC does.
  const clamp = clampStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readClampProbe, pollClampProbe, CLAMP } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readClampProbe({ exports, memory: exports.memory });
           if (r.state === CLAMP.FAILED || r.state === CLAMP.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== CLAMP.READY) {
             pollClampProbe({ exports });
             return null;
           }
           // Ready: classify every texel here rather than shipping 32768 bytes
           // out. Which colour each block came back as is the whole verdict, so
           // the counts are what crosses — a block that produced a mixture is a
           // different failure from one that produced the wrong colour evenly,
           // and the message has to be able to say which.
           const draw = ${JSON.stringify(PROBE_CLAMP_DRAW_BYTES)};
           const clear = ${JSON.stringify(PROBE_CLAMP_CLEAR_BYTES)};
           const same = (at, colour) =>
             r.bytes[at] === colour[0] &&
             r.bytes[at + 1] === colour[1] &&
             r.bytes[at + 2] === colour[2] &&
             r.bytes[at + 3] === colour[3];
           const block = r.bytes.length / 2;
           const tally = (from) => {
             let drawn = 0;
             let cleared = 0;
             let other = 0;
             let firstOther = -1;
             for (let at = from; at + 3 < from + block; at += 4) {
               if (same(at, draw)) drawn += 1;
               else if (same(at, clear)) cleared += 1;
               else {
                 if (firstOther < 0) firstOther = at;
                 other += 1;
               }
             }
             return { drawn, cleared, other, firstOther };
           };
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             clamped: tally(0),
             clipped: tally(block),
             sample: [...r.bytes.slice(0, 4), ...r.bytes.slice(block, block + 4)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;

  const clampAbsent =
    clampStart?.supported === false && clampStart?.adapterHas === false;
  const absentClampDetail =
    "this browser's adapter reports no 'depth-clip-control', so the device could " +
    'not enable it and WebGPU would refuse the clamped pipeline — the capability ' +
    'is NotOnThisDevice here and there is nothing to hold it to';
  check(
    'AH',
    'wasm encoded the depth-clamp frame — two targets, two pipelines differing only in depth_clamp, a pass into each, both copies, the request',
    clampStart?.started === true || clampAbsent,
    clampStart?.started
      ? 'the clamped and clipped passes are on the stream'
      : clampStart?.supported === false
        ? clampStart?.adapterHas
          ? "this browser's adapter HAS 'depth-clip-control' and wasm still reports the " +
            'device without it — the optional feature was lost between the request and the ' +
            'device that opened, which is a bug here and not a platform limit'
          : absentClampDetail
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'AH',
    'depth clamping decided which fragments survived — the clamped target is the draw colour and the control target is still the clear',
    clampAbsent ||
      (clamp?.done === true &&
        clamp.len === PROBE_CLAMP_PIXELS * 4 * 2 &&
        clamp.clamped?.drawn === PROBE_CLAMP_PIXELS &&
        clamp.clipped?.cleared === PROBE_CLAMP_PIXELS),
    clampAbsent
      ? absentClampDetail
      : clampStart?.started !== true
        ? 'no frame was encoded, so there is nothing to read — see the check above for why'
        : clamp?.done !== true
          ? `no depth-clamp readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
          : clamp.clamped?.drawn === PROBE_CLAMP_PIXELS &&
              clamp.clipped?.cleared === PROBE_CLAMP_PIXELS
            ? `${clamp.len} bytes: the clamped target is [${PROBE_CLAMP_DRAW_BYTES.join(', ')}] throughout ` +
              'and the control target is untouched clear, so the triangle past the far plane ' +
              'rasterised through one pipeline and was clipped away by the other'
            : `state ${clamp.state}, ${clamp.len ?? 0} bytes: clamped ` +
              `${clamp.clamped?.drawn} drawn / ${clamp.clamped?.cleared} clear / ${clamp.clamped?.other} other, ` +
              `control ${clamp.clipped?.drawn} drawn / ${clamp.clipped?.cleared} clear / ${clamp.clipped?.other} other — ` +
              (clamp.clamped?.cleared === PROBE_CLAMP_PIXELS &&
              clamp.clipped?.cleared === PROBE_CLAMP_PIXELS
                ? 'both were clipped, so depth_clamp reached nothing'
                : clamp.clamped?.drawn === PROBE_CLAMP_PIXELS &&
                    clamp.clipped?.drawn === PROBE_CLAMP_PIXELS
                  ? 'neither was clipped, so the triangle is not outside the depth range and ' +
                    'the clamped reading proves nothing'
                  : clamp.clamped?.cleared === PROBE_CLAMP_PIXELS &&
                      clamp.clipped?.drawn === PROBE_CLAMP_PIXELS
                    ? 'the two are inverted — the flag is on the pipeline that was meant to be the control'
                    : 'a mixture, so something other than the flag decided which fragments survived') +
              ` (first texel of each block ${JSON.stringify(clamp.sample)})` +
              `${clamp.error ? ` — ${clamp.error}` : ''}`
  );

  // **THE GATE THAT SHOWS A NON-ZERO `firstInstance` REACHING A GPU.** Group Z
  // proves an indirect draw happened and walked a padded stride; every argument
  // structure it reads sets `firstInstance` to zero, because core WebGPU accepts
  // no other value. `Features::INDIRECT_FIRST_INSTANCE` is reported to callers
  // off the browser's `indirect-first-instance` and, until this group, nothing
  // anywhere had ever put another number in one of those structures — no probe
  // shader even read `@builtin(instance_index)`, so a constant-colour fullscreen
  // triangle could not have shown the difference if one had.
  //
  // Group AH's shape exactly: the *same* half-width quad, through the *same*
  // pipeline, off the *same* args buffer, into two targets of the same size
  // cleared to the same colour — and the only difference between the two draws
  // is the `firstInstance` in the structure each reads. The shader shifts the
  // quad right by `instance_index`, which WebGPU defines as `firstInstance` plus
  // the instance number and both draws set `instanceCount` to one, so the zero
  // draw must paint the left half of its target and the one draw the right half
  // of its. What is asserted is which texels came back, not that a draw survived:
  // a browser that read the arguments and dropped the field raises no error, and
  // the readback is the only place it shows.
  //
  // **A BROWSER WITHOUT `indirect-first-instance` OWES THIS GROUP NOTHING — BUT
  // IT HAS TO BE THE BROWSER THAT SAID SO.** Group AH's rule and its reason: the
  // page asks `navigator.gpu` for itself, and an adapter that HAS the feature
  // while wasm reports the device without it is an optional feature lost between
  // the request and the device that opened — a bug here, not a platform limit,
  // and it fails rather than being waved through.
  //
  // WHERE IT BELONGS IN THE RUN: **before X**, group AH's reason exactly. This
  // group reads bytes back, so behind X's stuck canvas readback it would be
  // stranded on Windows, and it is not in that platform's `--expect-fail` list.
  group(
    'AI — an indirect draw’s firstInstance decides which half of the target is painted'
  );

  // `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_FIRST_INSTANCE_*`, spelled out
  // here rather than imported, as every expected value in this file is.
  const PROBE_FIRST_INSTANCE_SIZE = 64;
  const PROBE_FIRST_INSTANCE_HALF_PIXELS =
    (PROBE_FIRST_INSTANCE_SIZE / 2) * PROBE_FIRST_INSTANCE_SIZE;
  const PROBE_FIRST_INSTANCE_CLEAR_BYTES = [35, 105, 175, 255];
  const PROBE_FIRST_INSTANCE_DRAW_BYTES = [200, 145, 55, 235];

  const firstInstanceStart = await evaluate(
    page,
    `(async () => {
     const { startFirstInstanceProbe, firstInstanceSupported } =
       await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     // Supported first: it is what tells a device that withheld the feature
     // from a request that never happened.
     const supported = firstInstanceSupported({ exports });
     // And what the browser itself says, so the excuse below cannot be given
     // by the thing under test. Asked of navigator.gpu, not of anything of ours.
     const adapter = await navigator.gpu.requestAdapter();
     const adapterHas =
       adapter?.features?.has('indirect-first-instance') === true;
     return {
       supported,
       adapterHas,
       started: startFirstInstanceProbe({ exports }),
     };
   })()`
  );
  // Poll across frames exactly as group AH does.
  const firstInstance = firstInstanceStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readFirstInstanceProbe, pollFirstInstanceProbe, FIRST_INSTANCE } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readFirstInstanceProbe({ exports, memory: exports.memory });
           if (
             r.state === FIRST_INSTANCE.FAILED ||
             r.state === FIRST_INSTANCE.UNDECODABLE
           ) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== FIRST_INSTANCE.READY) {
             pollFirstInstanceProbe({ exports });
             return null;
           }
           // Ready: classify every texel here rather than shipping 32768 bytes
           // out. Which colour each HALF of each block came back as is the whole
           // verdict, so the counts are what crosses — a half that produced a
           // mixture is a different failure from one that produced the wrong
           // colour evenly, and the message has to be able to say which.
           const draw = ${JSON.stringify(PROBE_FIRST_INSTANCE_DRAW_BYTES)};
           const clear = ${JSON.stringify(PROBE_FIRST_INSTANCE_CLEAR_BYTES)};
           const size = ${PROBE_FIRST_INSTANCE_SIZE};
           const same = (at, colour) =>
             r.bytes[at] === colour[0] &&
             r.bytes[at + 1] === colour[1] &&
             r.bytes[at + 2] === colour[2] &&
             r.bytes[at + 3] === colour[3];
           const block = r.bytes.length / 2;
           // Rows are tight — 64 Rgba8Unorm texels is 256 bytes, the alignment
           // copyTextureToBuffer wants — so a texel's column is its index within
           // the row and nothing has to be skipped.
           const tally = (from, leftHalf) => {
             let drawn = 0;
             let cleared = 0;
             let other = 0;
             let firstOther = -1;
             for (let texel = 0; texel < block / 4; texel += 1) {
               const column = texel % size;
               if (column < size / 2 !== leftHalf) continue;
               const at = from + texel * 4;
               if (same(at, draw)) drawn += 1;
               else if (same(at, clear)) cleared += 1;
               else {
                 if (firstOther < 0) firstOther = at;
                 other += 1;
               }
             }
             return { drawn, cleared, other, firstOther };
           };
           const midpoint = (size / 2) * 4;
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             zeroLeft: tally(0, true),
             zeroRight: tally(0, false),
             oneLeft: tally(block, true),
             oneRight: tally(block, false),
             sample: [
               ...r.bytes.slice(0, 4),
               ...r.bytes.slice(midpoint, midpoint + 4),
               ...r.bytes.slice(block, block + 4),
               ...r.bytes.slice(block + midpoint, block + midpoint + 4),
             ],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;

  const firstInstanceAbsent =
    firstInstanceStart?.supported === false &&
    firstInstanceStart?.adapterHas === false;
  const absentFirstInstanceDetail =
    "this browser's adapter reports no 'indirect-first-instance', so the device " +
    'could not enable it and WebGPU would refuse a non-zero firstInstance in an ' +
    'indirect draw — the capability is NotOnThisDevice here and there is nothing ' +
    'to hold it to';
  check(
    'AI',
    'wasm encoded the first-instance frame — two targets, one pipeline, two indirect draws off argument structures differing only in firstInstance, both copies, the request',
    firstInstanceStart?.started === true || firstInstanceAbsent,
    firstInstanceStart?.started
      ? 'the zero and one passes are on the stream'
      : firstInstanceStart?.supported === false
        ? firstInstanceStart?.adapterHas
          ? "this browser's adapter HAS 'indirect-first-instance' and wasm still reports the " +
            'device without it — the optional feature was lost between the request and the ' +
            'device that opened, which is a bug here and not a platform limit'
          : absentFirstInstanceDetail
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  const firstInstanceLanded =
    firstInstance?.done === true &&
    firstInstance.len ===
      PROBE_FIRST_INSTANCE_SIZE * PROBE_FIRST_INSTANCE_SIZE * 4 * 2 &&
    firstInstance.zeroLeft?.drawn === PROBE_FIRST_INSTANCE_HALF_PIXELS &&
    firstInstance.zeroRight?.cleared === PROBE_FIRST_INSTANCE_HALF_PIXELS &&
    firstInstance.oneLeft?.cleared === PROBE_FIRST_INSTANCE_HALF_PIXELS &&
    firstInstance.oneRight?.drawn === PROBE_FIRST_INSTANCE_HALF_PIXELS;
  // Which half of each block was painted, named so a failure says what came back
  // where rather than only that something did.
  const halfName = (left, right) =>
    left?.drawn === PROBE_FIRST_INSTANCE_HALF_PIXELS &&
    right?.cleared === PROBE_FIRST_INSTANCE_HALF_PIXELS
      ? 'left'
      : right?.drawn === PROBE_FIRST_INSTANCE_HALF_PIXELS &&
          left?.cleared === PROBE_FIRST_INSTANCE_HALF_PIXELS
        ? 'right'
        : left?.cleared === PROBE_FIRST_INSTANCE_HALF_PIXELS &&
            right?.cleared === PROBE_FIRST_INSTANCE_HALF_PIXELS
          ? 'neither'
          : 'a mixture';
  const zeroHalf = halfName(firstInstance?.zeroLeft, firstInstance?.zeroRight);
  const oneHalf = halfName(firstInstance?.oneLeft, firstInstance?.oneRight);
  check(
    'AI',
    'the firstInstance in the argument structure decided where the quad landed — the zero draw painted the left half and the one draw the right',
    firstInstanceAbsent || firstInstanceLanded,
    firstInstanceAbsent
      ? absentFirstInstanceDetail
      : firstInstanceStart?.started !== true
        ? 'no frame was encoded, so there is nothing to read — see the check above for why'
        : firstInstance?.done !== true
          ? `no first-instance readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
          : firstInstanceLanded
            ? `${firstInstance.len} bytes: the zero draw's target is [${PROBE_FIRST_INSTANCE_DRAW_BYTES.join(', ')}] ` +
              'across its left half and untouched clear across its right, and the one draw’s ' +
              'target is the mirror of that — so the same quad, drawn through the same ' +
              'pipeline, landed a half-target apart because of the firstInstance it read'
            : `state ${firstInstance.state}, ${firstInstance.len ?? 0} bytes: the zero draw painted ${zeroHalf} ` +
              `(${firstInstance.zeroLeft?.drawn} drawn / ${firstInstance.zeroLeft?.cleared} clear / ${firstInstance.zeroLeft?.other} other on the left, ` +
              `${firstInstance.zeroRight?.drawn} / ${firstInstance.zeroRight?.cleared} / ${firstInstance.zeroRight?.other} on the right), ` +
              `the one draw painted ${oneHalf} ` +
              `(${firstInstance.oneLeft?.drawn} / ${firstInstance.oneLeft?.cleared} / ${firstInstance.oneLeft?.other} on the left, ` +
              `${firstInstance.oneRight?.drawn} / ${firstInstance.oneRight?.cleared} / ${firstInstance.oneRight?.other} on the right) — ` +
              (zeroHalf === 'left' && oneHalf === 'left'
                ? 'both landed on the left, so the second draw’s firstInstance reached nothing'
                : zeroHalf === 'right' && oneHalf === 'right'
                  ? 'both landed on the right, so something other than the argument structure is ' +
                    'choosing the instance index and the pair proves nothing'
                  : zeroHalf === 'right' && oneHalf === 'left'
                    ? 'the two are inverted — the non-zero firstInstance is on the draw that was ' +
                      'meant to be the control'
                    : zeroHalf === 'neither' || oneHalf === 'neither'
                      ? 'a target came back untouched, so a draw produced no fragments at all'
                      : 'a mixture, so something other than the firstInstance decided where the ' +
                        'quad landed') +
              ` (first texel of each half ${JSON.stringify(firstInstance.sample)},` +
              ` first unexpected texel at ${firstInstance.zeroLeft?.firstOther}/${firstInstance.zeroRight?.firstOther}/` +
              `${firstInstance.oneLeft?.firstOther}/${firstInstance.oneRight?.firstOther})` +
              `${firstInstance.error ? ` — ${firstInstance.error}` : ''}`
  );

  // **THE GATE THAT SHOWS A TEXTURE REACHING A FRAGMENT SHADER.** Group M asks a
  // browser to build a bind-group layout declaring a `SampledImage` and a
  // `Sampler`, and group N asks it to build a bind group holding a real view and
  // a real sampler — and both stop there, because a `GPUBindGroupLayout` and a
  // `GPUBindGroup` each report their `label` and nothing else. Until this group
  // no probe shader contained a `textureSample` at all: the seam could describe a
  // sampled-image binding and nothing had ever shown that a texture bound through
  // one reaches a shader, let alone that the texel it delivers is the right one.
  //
  // **WHAT IS ASSERTED IS WHICH COLOUR EACH QUADRANT CAME BACK AS.** wasm uploads
  // a two-by-two `rgba8unorm` source whose four texels are four *different*
  // colours, binds its view and a **nearest** sampler, and draws a fullscreen quad
  // whose fragment shader samples it — so the target's four quadrants must be the
  // four source texels, each in the corner it came from. Every wrong way of
  // sampling produces a different block, and each is a separate reading here:
  //
  //   * one colour everywhere — the shader returned a constant and sampled
  //     nothing. The four colours differ, so no single value satisfies more than
  //     one quadrant.
  //   * top and bottom exchanged — the V axis is flipped. Clip space runs `y` up
  //     and texture space runs `v` down, so this is the one an author gets wrong
  //     by writing the obvious thing, and it renders a picture either way.
  //   * left and right exchanged — the U axis is flipped.
  //   * top-right and bottom-left exchanged, the diagonal unmoved — `u` and `v`
  //     are transposed. Those two quadrants are the ONLY place a transpose shows,
  //     which is why the four are checked corner by corner rather than as "all
  //     four colours are present somewhere".
  //   * a channel permutation everywhere — a swap on the way out. The four
  //     colours are pairwise distinct as multisets and no colour is a permutation
  //     of itself, so no swap can land on a value this accepts. Their alphas are
  //     pairwise distinct and none is `255`, so a dropped alpha cannot either.
  //   * the clear everywhere — the draw produced no fragments. The clear is a
  //     fifth colour held to the same rules, so this reads as "nothing was drawn"
  //     rather than "the wrong colour".
  //   * a gradient — the sampler filtered. Every texel of a quadrant has to
  //     match, not a majority.
  //
  // **NO FEATURE GATES IT, so there is no absent branch.** Groups AH and AI each
  // excuse a browser whose adapter lacks the feature they need; sampling a
  // `rgba8unorm` texture is core WebGPU and every runner — SwiftShader included —
  // owes this group its bytes. There is no `supported` flag to read and no
  // `UNSUPPORTED` state to reach.
  //
  // WHERE IT BELONGS IN THE RUN: **before X**, group AI's reason exactly. This
  // group reads bytes back, so behind X's stuck canvas readback it would be
  // stranded on Windows, and it is not in that platform's `--expect-fail` list.
  group(
    'AJ — a sampled texture reaches a fragment shader and the right texel comes out'
  );

  // `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_TEXTURE_SAMPLE_*`, spelled out
  // here rather than imported, as every expected value in this file is.
  const PROBE_TEXTURE_SAMPLE_SIZE = 64;
  const PROBE_TEXTURE_SAMPLE_QUADRANT_PIXELS =
    (PROBE_TEXTURE_SAMPLE_SIZE / 2) * (PROBE_TEXTURE_SAMPLE_SIZE / 2);
  const PROBE_TEXTURE_SAMPLE_TOP_LEFT_BYTES = [43, 99, 214, 233];
  const PROBE_TEXTURE_SAMPLE_TOP_RIGHT_BYTES = [214, 62, 33, 191];
  const PROBE_TEXTURE_SAMPLE_BOTTOM_LEFT_BYTES = [88, 176, 71, 143];
  const PROBE_TEXTURE_SAMPLE_BOTTOM_RIGHT_BYTES = [131, 120, 245, 90];
  const PROBE_TEXTURE_SAMPLE_CLEAR_BYTES = [150, 30, 105, 60];

  const textureSampleStart = await evaluate(
    page,
    `(async () => {
     const { startTextureSampleProbe } = await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     return { started: startTextureSampleProbe({ exports }) };
   })()`
  );
  // Poll across frames exactly as group AI does.
  const textureSample = textureSampleStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readTextureSampleProbe, pollTextureSampleProbe, TEXTURE_SAMPLE } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readTextureSampleProbe({ exports, memory: exports.memory });
           if (
             r.state === TEXTURE_SAMPLE.FAILED ||
             r.state === TEXTURE_SAMPLE.UNDECODABLE
           ) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== TEXTURE_SAMPLE.READY) {
             pollTextureSampleProbe({ exports });
             return null;
           }
           // Ready: classify every texel here rather than shipping 16384 bytes
           // out. Each quadrant crosses as two things — how many of its texels
           // are the colour that BELONGS there, and which named colour it holds
           // uniformly if it holds one — because those are different failures.
           // The count alone cannot say whether a wrong quadrant is a flipped
           // axis or a mixture, and the name alone cannot say how much of it is.
           const size = ${PROBE_TEXTURE_SAMPLE_SIZE};
           const palette = [
             ['top-left', ${JSON.stringify(PROBE_TEXTURE_SAMPLE_TOP_LEFT_BYTES)}],
             ['top-right', ${JSON.stringify(PROBE_TEXTURE_SAMPLE_TOP_RIGHT_BYTES)}],
             ['bottom-left', ${JSON.stringify(PROBE_TEXTURE_SAMPLE_BOTTOM_LEFT_BYTES)}],
             ['bottom-right', ${JSON.stringify(PROBE_TEXTURE_SAMPLE_BOTTOM_RIGHT_BYTES)}],
             ['the clear', ${JSON.stringify(PROBE_TEXTURE_SAMPLE_CLEAR_BYTES)}],
           ];
           const same = (at, colour) =>
             r.bytes[at] === colour[0] &&
             r.bytes[at + 1] === colour[1] &&
             r.bytes[at + 2] === colour[2] &&
             r.bytes[at + 3] === colour[3];
           // Rows are tight — 64 Rgba8Unorm texels is 256 bytes, the alignment
           // copyTextureToBuffer wants — so a texel's offset is its own index
           // times four and nothing has to be skipped.
           const quadrant = (topHalf, leftHalf, expected) => {
             let matched = 0;
             let holds = null;
             let firstWrong = -1;
             let first = null;
             for (let row = 0; row < size; row += 1) {
               if (row < size / 2 !== topHalf) continue;
               for (let column = 0; column < size; column += 1) {
                 if (column < size / 2 !== leftHalf) continue;
                 const at = (row * size + column) * 4;
                 if (first === null) first = [...r.bytes.slice(at, at + 4)];
                 if (same(at, expected)) matched += 1;
                 else if (firstWrong < 0) firstWrong = at;
                 const named = palette.find(([, colour]) => same(at, colour));
                 const label = named ? named[0] : 'an unnamed colour';
                 if (holds === null) holds = label;
                 else if (holds !== label) holds = 'a mixture';
               }
             }
             return { matched, holds, firstWrong, first };
           };
           const midpoint = (size / 2) * 4;
           const half = ((size * size) / 2) * 4;
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             topLeft: quadrant(true, true, palette[0][1]),
             topRight: quadrant(true, false, palette[1][1]),
             bottomLeft: quadrant(false, true, palette[2][1]),
             bottomRight: quadrant(false, false, palette[3][1]),
             sample: [
               ...r.bytes.slice(0, 4),
               ...r.bytes.slice(midpoint, midpoint + 4),
               ...r.bytes.slice(half, half + 4),
               ...r.bytes.slice(half + midpoint, half + midpoint + 4),
             ],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;

  check(
    'AJ',
    'wasm encoded the texture-sample frame — a two-by-two source uploaded with four different texels, a nearest sampler, a bind group holding that source view and that sampler, a pass that samples it across a fullscreen quad, the copy, the request',
    textureSampleStart?.started === true,
    textureSampleStart?.started
      ? 'the upload, the bind group and the sampling pass are on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is ' +
          'installed. Nothing else can refuse this frame: sampling is core WebGPU ' +
          'and no device can withhold it'
  );
  // The arity first, then the four corners. A block of the wrong length means the
  // quadrant walk read texels that are not there, and its tallies would be
  // meaningless rather than merely wrong.
  const textureSampleLanded =
    textureSample?.done === true &&
    textureSample.len ===
      PROBE_TEXTURE_SAMPLE_SIZE * PROBE_TEXTURE_SAMPLE_SIZE * 4 &&
    textureSample.topLeft?.matched === PROBE_TEXTURE_SAMPLE_QUADRANT_PIXELS &&
    textureSample.topRight?.matched === PROBE_TEXTURE_SAMPLE_QUADRANT_PIXELS &&
    textureSample.bottomLeft?.matched ===
      PROBE_TEXTURE_SAMPLE_QUADRANT_PIXELS &&
    textureSample.bottomRight?.matched === PROBE_TEXTURE_SAMPLE_QUADRANT_PIXELS;
  // Which source texel each quadrant actually holds, named so a failure says what
  // arrived where rather than only that something did.
  const held = [
    textureSample?.topLeft?.holds,
    textureSample?.topRight?.holds,
    textureSample?.bottomLeft?.holds,
    textureSample?.bottomRight?.holds,
  ];
  const holding = (expected) => held.every((name, at) => name === expected[at]);
  // **A LABEL CANNOT SAY TWO STRANGERS APART.** Every colour the palette does not
  // recognise is labelled `an unnamed colour`, so four *different* wrong colours
  // compare equal by name and read as one — which is how a shader that returned
  // the right rgb with the alpha overwritten was diagnosed as returning a
  // constant. These compare the bytes instead: `uniform` is the claim "every
  // quadrant really is the same colour", and `channelsWrong` is the set of
  // channels that is wrong in the same way everywhere while the rest are right,
  // which is what a dropped or overwritten channel looks like.
  const firsts = [
    textureSample?.topLeft?.first,
    textureSample?.topRight?.first,
    textureSample?.bottomLeft?.first,
    textureSample?.bottomRight?.first,
  ];
  const expectedFirsts = [
    PROBE_TEXTURE_SAMPLE_TOP_LEFT_BYTES,
    PROBE_TEXTURE_SAMPLE_TOP_RIGHT_BYTES,
    PROBE_TEXTURE_SAMPLE_BOTTOM_LEFT_BYTES,
    PROBE_TEXTURE_SAMPLE_BOTTOM_RIGHT_BYTES,
  ];
  const complete = firsts.every(
    (texel) => Array.isArray(texel) && texel.length === 4
  );
  const uniform =
    complete &&
    firsts.every((texel) => texel.every((byte, at) => byte === firsts[0][at]));
  const CHANNELS = ['red', 'green', 'blue', 'alpha'];
  const channelsWrong = complete
    ? CHANNELS.map((_, channel) => channel).filter((channel) =>
        firsts.every(
          (texel, at) => texel[channel] !== expectedFirsts[at][channel]
        )
      )
    : [];
  const channelsRight = complete
    ? CHANNELS.map((_, channel) => channel).filter((channel) =>
        firsts.every(
          (texel, at) => texel[channel] === expectedFirsts[at][channel]
        )
      )
    : [];
  // Every channel is accounted for one way or the other, so the wrong ones are
  // wrong *everywhere* rather than in a quadrant or two — the shape a shader
  // that touched one channel leaves, and not the shape a wrong uv leaves.
  const onlyChannels =
    complete &&
    channelsWrong.length > 0 &&
    channelsWrong.length + channelsRight.length === CHANNELS.length;
  const wrongWay = holding([
    'bottom-left',
    'bottom-right',
    'top-left',
    'top-right',
  ])
    ? 'top and bottom are exchanged, so the V axis is flipped — clip space runs y up ' +
      'and texture space runs v down, and the shader did not turn one into the other'
    : holding(['top-right', 'top-left', 'bottom-right', 'bottom-left'])
      ? 'left and right are exchanged, so the U axis is flipped'
      : holding(['top-left', 'bottom-left', 'top-right', 'bottom-right'])
        ? 'the diagonal is unmoved and the other two corners are exchanged, so u and v ' +
          'are transposed'
        : held.every((name) => name === 'the clear')
          ? 'every quadrant is the clear, so the draw produced no fragments at all'
          : onlyChannels
            ? `the ${channelsWrong.map((channel) => CHANNELS[channel]).join(' and ')} channel ` +
              'is wrong in every quadrant and the rest are right, so the texel arrived at the ' +
              'correct uv and the shader dropped or overwrote that channel on the way out'
            : uniform && held.every((name) => name !== 'a mixture')
              ? 'every quadrant holds the same colour byte for byte, so the fragment shader ' +
                'returned a constant rather than the texel under it'
              : held.some((name) => name === 'a mixture')
                ? 'a quadrant holds more than one colour, so the sampler filtered across the ' +
                  'source or something other than the uv decided which texel each fragment read'
                : 'the four corners hold colours the fixture does not account for';
  check(
    'AJ',
    'a texture reached the fragment shader and the texel it delivered was the right one — each quadrant of the target is the source texel from the same corner',
    textureSampleLanded,
    textureSampleStart?.started !== true
      ? 'no frame was encoded, so there is nothing to read — see the check above for why'
      : textureSample?.done !== true
        ? `no texture-sample readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
        : textureSampleLanded
          ? `${textureSample.len} bytes: the four quadrants are ` +
            `[${PROBE_TEXTURE_SAMPLE_TOP_LEFT_BYTES.join(', ')}], ` +
            `[${PROBE_TEXTURE_SAMPLE_TOP_RIGHT_BYTES.join(', ')}], ` +
            `[${PROBE_TEXTURE_SAMPLE_BOTTOM_LEFT_BYTES.join(', ')}] and ` +
            `[${PROBE_TEXTURE_SAMPLE_BOTTOM_RIGHT_BYTES.join(', ')}] throughout, ` +
            'each in the corner its source texel came from — so a real GPUTexture and a ' +
            'real GPUSampler were bound to a fragment shader and sampled at the right uv'
          : `state ${textureSample.state}, ${textureSample.len ?? 0} bytes: the quadrants hold ` +
            `${held.join(', ')} in reading order from the top left ` +
            `(${textureSample.topLeft?.matched}/${textureSample.topRight?.matched}/` +
            `${textureSample.bottomLeft?.matched}/${textureSample.bottomRight?.matched} of ` +
            `${PROBE_TEXTURE_SAMPLE_QUADRANT_PIXELS} texels right in each) — ${wrongWay}` +
            ` (first texel of each quadrant ${JSON.stringify(textureSample.sample)},` +
            ` first wrong texel at ${textureSample.topLeft?.firstWrong}/${textureSample.topRight?.firstWrong}/` +
            `${textureSample.bottomLeft?.firstWrong}/${textureSample.bottomRight?.firstWrong})` +
            `${textureSample.error ? ` — ${textureSample.error}` : ''}`
  );

  // **THE GATE THAT SHOWS A COMPRESSED TEXTURE BEING DECODED.** This crate
  // reports `Features::TEXTURE_COMPRESSION_BC` to callers off the browser's
  // `texture-compression-bc`, `crcbl_hal::Format` spells nine BC formats and
  // `gpu-replay.js` maps every one of them — and the whole of that was answered
  // by `web/tools/gpu-replay.mjs` under node, against a stub device that records
  // the descriptor it is handed. A recorded descriptor is not a decoded block:
  // the feature gate, the block-pitch conversion and the bytes themselves had
  // never met a browser.
  //
  // WHAT IT DOES: group AJ's frame with the source swapped for a compressed one.
  // wasm uploads an 8×8 `bc1-rgba-unorm` source — two BC1 blocks each way —
  // through a copy whose `bytesPerRow` has to be a multiple of 256, which for
  // 8-byte blocks means a row pitch of 128 texels where the image is 8. The same
  // nearest sampler, the same fullscreen quad, the same shader.
  //
  // **NOTHING ON THIS SIDE DECOMPRESSES BC1.** The blocks are laid out byte by
  // byte in `probe.rs` and handed to WebGPU; what comes back is whatever the
  // GPU's own decoder produced. So the quadrants are a claim about the hardware
  // and the wire together — that the block reached the texture at the right
  // pitch, and that its endpoint decoded to the colour that endpoint spells.
  //
  // **WHY THE FOUR COLOURS ARE CUBE CORNERS AND NOT MID-TONES**, which looks
  // like a worse fixture and is the only defensible one. There is no single
  // decoded value for a mid-tone BC1 endpoint: the Khronos Data Format
  // Specification 1.3 §18.1 — which WebGPU defers to so completely that it never
  // defines the decode itself — gives an exact rational (`R = c^15..11 / 31`),
  // while D3D 11.3 §19.5.3 gives bit replication, and the two disagree by a step
  // at 5-bit 3, 7, 24, 28 and 6-bit 11..15, 48..52. D3D §19.5.2 licenses both,
  // and its tolerance is stated "for all channels of all texels" — the endpoints
  // are NOT privileged, so "assert on the endpoints and avoid the interpolants"
  // is not on its own enough. What §19.5.2 does guarantee is that values the
  // reference decodes to 0.0 or 1.0 "must always be exact", and that is also the
  // only place the two expansions agree. So every channel of every endpoint here
  // is at a rail, and the comparison below is exact **because the specification
  // says so** rather than because this GPU obliges.
  //
  // The cost is group AJ's colour rules: a cube corner has two channels alike,
  // so no multiset rule can hold. The discrimination is positional instead — no
  // channel permutation carries the ordered four onto itself, and a channel
  // stuck at either rail moves at least one quadrant.
  // `the_four_bc_sample_colours_are_told_apart_where_they_sit` holds that.
  //
  // **AND THERE IS NO ALPHA CHECK HERE, WHICH GROUP AJ HAS.** A four-colour BC1
  // block is opaque: every texel decodes with alpha 1.0, so these four cannot
  // carry four distinct alphas and an alpha dropped between the source and the
  // readback is invisible to this group. Group AJ is where that is caught, on a
  // source whose alphas can differ. Nothing below should be read as checking it.
  //
  // **A BROWSER WITHOUT `texture-compression-bc` OWES THIS GROUP ITS BYTES — BUT
  // IT HAS TO SHOW THAT IT REFUSES THEM.** This is the absent branch CI's Linux
  // runner takes, because SwiftShader does not report the feature, so it is the
  // one that must not read as covered while proving nothing. Group AH's rule is
  // kept — the page asks `navigator.gpu` itself, and an adapter that HAS the
  // feature while wasm reports the device without it fails here — and one more
  // is added on top: the page asks the **real GPUDevice** to create a
  // `bc1-rgba-unorm` texture inside a validation error scope, and holds the
  // answer to what wasm did. Refused where wasm encoded nothing, accepted where
  // wasm encoded the frame. That check has teeth on both platforms rather than
  // excusing one of them.
  //
  // WHERE IT BELONGS IN THE RUN: **before X**, group AJ's reason exactly. This
  // group reads bytes back, so behind X's stuck canvas readback it would be
  // stranded on Windows, and it is not in that platform's `--expect-fail` list.
  group('AK — a BC1 block is decoded by the GPU and its endpoint comes out');

  // `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_BC_SAMPLE_*`, spelled out here
  // rather than imported, as every expected value in this file is. The target is
  // group AJ's, so its size and its clear are group AJ's too.
  const PROBE_BC_SAMPLE_SOURCE_SIZE = 8;
  const PROBE_BC_SAMPLE_UPLOAD_ROW_TEXELS = 128;
  const PROBE_BC_SAMPLE_TOP_LEFT_BYTES = [255, 0, 0, 255];
  const PROBE_BC_SAMPLE_TOP_RIGHT_BYTES = [0, 255, 0, 255];
  const PROBE_BC_SAMPLE_BOTTOM_LEFT_BYTES = [0, 0, 255, 255];
  const PROBE_BC_SAMPLE_BOTTOM_RIGHT_BYTES = [255, 255, 0, 255];

  const bcStart = await evaluate(
    page,
    `(async () => {
     const { startBcSampleProbe, bcSampleSupported } =
       await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     // Supported first: it is what tells a device that withheld the feature
     // from a request that never happened.
     const supported = bcSampleSupported({ exports });
     // And what the browser itself says, so the excuse below cannot be given
     // by the thing under test. Asked of navigator.gpu, not of anything of ours.
     const adapter = await navigator.gpu.requestAdapter();
     const adapterHas =
       adapter?.features?.has('texture-compression-bc') === true;
     // **THE ABSENT BRANCH'S OWN CLAIM.** Ask the real device for the very
     // texture this probe needs, inside a validation scope so the answer is
     // caught rather than reported as an uncaptured error. A device without the
     // feature must refuse it; a device with it must not. Either way this is the
     // browser answering, not wasm.
     const device = gpu.replayer.device;
     let refusal = null;
     let asked = false;
     if (device) {
       asked = true;
       device.pushErrorScope('validation');
       let texture = null;
       try {
         texture = device.createTexture({
           size: [${PROBE_BC_SAMPLE_SOURCE_SIZE}, ${PROBE_BC_SAMPLE_SOURCE_SIZE}],
           format: 'bc1-rgba-unorm',
           usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
         });
       } catch (error) {
         refusal = String(error?.message ?? error);
       }
       const scoped = await device.popErrorScope();
       if (scoped) refusal = scoped.message;
       try {
         texture?.destroy();
       } catch {
         // An invalid texture may refuse to be destroyed; it is going away with
         // the device either way and this is not what is under test.
       }
     }
     return {
       supported,
       adapterHas,
       asked,
       refusal,
       started: startBcSampleProbe({ exports }),
     };
   })()`
  );
  // Poll across frames exactly as group AJ does.
  const bcSample = bcStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readBcSampleProbe, pollBcSampleProbe, BC_SAMPLE } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readBcSampleProbe({ exports, memory: exports.memory });
           if (
             r.state === BC_SAMPLE.FAILED ||
             r.state === BC_SAMPLE.UNDECODABLE
           ) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== BC_SAMPLE.READY) {
             pollBcSampleProbe({ exports });
             return null;
           }
           // Ready: classify every texel here rather than shipping 16384 bytes
           // out, group AJ's reason and its shape. Each quadrant crosses as how
           // many of its texels are the colour that BELONGS there and which
           // named colour it holds uniformly if it holds one.
           const size = ${PROBE_TEXTURE_SAMPLE_SIZE};
           const palette = [
             ['top-left', ${JSON.stringify(PROBE_BC_SAMPLE_TOP_LEFT_BYTES)}],
             ['top-right', ${JSON.stringify(PROBE_BC_SAMPLE_TOP_RIGHT_BYTES)}],
             ['bottom-left', ${JSON.stringify(PROBE_BC_SAMPLE_BOTTOM_LEFT_BYTES)}],
             ['bottom-right', ${JSON.stringify(PROBE_BC_SAMPLE_BOTTOM_RIGHT_BYTES)}],
             ['the clear', ${JSON.stringify(PROBE_TEXTURE_SAMPLE_CLEAR_BYTES)}],
           ];
           const same = (at, colour) =>
             r.bytes[at] === colour[0] &&
             r.bytes[at + 1] === colour[1] &&
             r.bytes[at + 2] === colour[2] &&
             r.bytes[at + 3] === colour[3];
           const quadrant = (topHalf, leftHalf, expected) => {
             let matched = 0;
             let holds = null;
             let firstWrong = -1;
             let first = null;
             for (let row = 0; row < size; row += 1) {
               if (row < size / 2 !== topHalf) continue;
               for (let column = 0; column < size; column += 1) {
                 if (column < size / 2 !== leftHalf) continue;
                 const at = (row * size + column) * 4;
                 if (first === null) first = [...r.bytes.slice(at, at + 4)];
                 if (same(at, expected)) matched += 1;
                 else if (firstWrong < 0) firstWrong = at;
                 const named = palette.find(([, colour]) => same(at, colour));
                 const label = named ? named[0] : 'an unnamed colour';
                 if (holds === null) holds = label;
                 else if (holds !== label) holds = 'a mixture';
               }
             }
             return { matched, holds, firstWrong, first };
           };
           const midpoint = (size / 2) * 4;
           const half = ((size * size) / 2) * 4;
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             topLeft: quadrant(true, true, palette[0][1]),
             topRight: quadrant(true, false, palette[1][1]),
             bottomLeft: quadrant(false, true, palette[2][1]),
             bottomRight: quadrant(false, false, palette[3][1]),
             sample: [
               ...r.bytes.slice(0, 4),
               ...r.bytes.slice(midpoint, midpoint + 4),
               ...r.bytes.slice(half, half + 4),
               ...r.bytes.slice(half + midpoint, half + midpoint + 4),
             ],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;

  const bcAbsent =
    bcStart?.supported === false && bcStart?.adapterHas === false;
  const absentBcDetail =
    "this browser's adapter reports no 'texture-compression-bc', so no " +
    'bc1-rgba-unorm texture could be created here — the capability is ' +
    'NotOnThisDevice and there is no decoded block to hold anything to';
  check(
    'AK',
    'wasm encoded the BC frame — a bc1-rgba-unorm source two blocks square uploaded at a padded block pitch, a nearest sampler, a bind group holding that compressed view, a pass that samples it across a fullscreen quad, the copy, the request',
    bcStart?.started === true || bcAbsent,
    bcStart?.started
      ? 'the block upload and the sampling pass are on the stream'
      : bcStart?.supported === false
        ? bcStart?.adapterHas
          ? "this browser's adapter HAS 'texture-compression-bc' and wasm still reports the " +
            'device without it — the optional feature was lost between the request and the ' +
            'device that opened, which is a bug here and not a platform limit'
          : absentBcDetail
        : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  // The arity first, then the four corners — group AJ's order and for its
  // reason: a block of the wrong length means the quadrant walk read texels that
  // are not there, and its tallies would be meaningless rather than merely wrong.
  const bcLanded =
    bcSample?.done === true &&
    bcSample.len ===
      PROBE_TEXTURE_SAMPLE_SIZE * PROBE_TEXTURE_SAMPLE_SIZE * 4 &&
    bcSample.topLeft?.matched === PROBE_TEXTURE_SAMPLE_QUADRANT_PIXELS &&
    bcSample.topRight?.matched === PROBE_TEXTURE_SAMPLE_QUADRANT_PIXELS &&
    bcSample.bottomLeft?.matched === PROBE_TEXTURE_SAMPLE_QUADRANT_PIXELS &&
    bcSample.bottomRight?.matched === PROBE_TEXTURE_SAMPLE_QUADRANT_PIXELS;
  const bcHeld = [
    bcSample?.topLeft?.holds,
    bcSample?.topRight?.holds,
    bcSample?.bottomLeft?.holds,
    bcSample?.bottomRight?.holds,
  ];
  const bcHolding = (expected) =>
    bcHeld.every((name, at) => name === expected[at]);
  const bcWrongWay = bcHolding([
    'bottom-left',
    'bottom-right',
    'top-left',
    'top-right',
  ])
    ? 'top and bottom are exchanged, so the V axis is flipped or the two block rows were ' +
      'uploaded in the wrong order'
    : bcHolding(['top-right', 'top-left', 'bottom-right', 'bottom-left'])
      ? 'left and right are exchanged, so the U axis is flipped or the two blocks of each ' +
        'row were written the wrong way round'
      : bcHolding(['top-left', 'bottom-left', 'top-right', 'bottom-right'])
        ? 'the diagonal is unmoved and the other two corners are exchanged, so u and v are ' +
          'transposed'
        : bcHeld.every((name) => name === 'the clear')
          ? 'every quadrant is the clear, so the draw produced no fragments at all'
          : bcHeld.every((name) => name === 'an unnamed colour')
            ? 'no quadrant holds a colour the fixture named, which is what a block read at the ' +
              'wrong offset looks like — the decoder saw bytes that were not a block of ours. ' +
              'The row pitch is the first thing to suspect: it is 256 bytes, which is ' +
              `${PROBE_BC_SAMPLE_UPLOAD_ROW_TEXELS} texels through the block extent, and a ` +
              'pitch converted per texel instead would read every row from the wrong place'
            : bcHeld.some((name) => name === 'a mixture')
              ? 'a quadrant holds more than one colour, so the sampler filtered across a block ' +
                'boundary or the indices in a block are not all selecting its endpoint'
              : 'the four corners hold colours the fixture does not account for';
  check(
    'AK',
    'a BC1 block was decoded by the GPU and its endpoint came out — each quadrant of the target is the colour the block in that corner spells',
    bcAbsent || bcLanded,
    bcAbsent
      ? absentBcDetail
      : bcStart?.started !== true
        ? 'no frame was encoded, so there is nothing to read — see the check above for why'
        : bcSample?.done !== true
          ? `no BC readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
          : bcLanded
            ? `${bcSample.len} bytes: the four quadrants are ` +
              `[${PROBE_BC_SAMPLE_TOP_LEFT_BYTES.join(', ')}], ` +
              `[${PROBE_BC_SAMPLE_TOP_RIGHT_BYTES.join(', ')}], ` +
              `[${PROBE_BC_SAMPLE_BOTTOM_LEFT_BYTES.join(', ')}] and ` +
              `[${PROBE_BC_SAMPLE_BOTTOM_RIGHT_BYTES.join(', ')}] throughout — so a real ` +
              'GPUTexture in a compressed format was decoded by the GPU and every endpoint ' +
              'came back exactly, which is the only comparison the specifications license'
            : `state ${bcSample.state}, ${bcSample.len ?? 0} bytes: the quadrants hold ` +
              `${bcHeld.join(', ')} in reading order from the top left ` +
              `(${bcSample.topLeft?.matched}/${bcSample.topRight?.matched}/` +
              `${bcSample.bottomLeft?.matched}/${bcSample.bottomRight?.matched} of ` +
              `${PROBE_TEXTURE_SAMPLE_QUADRANT_PIXELS} texels right in each) — ${bcWrongWay}` +
              ` (first texel of each quadrant ${JSON.stringify(bcSample.sample)},` +
              ` first wrong texel at ${bcSample.topLeft?.firstWrong}/${bcSample.topRight?.firstWrong}/` +
              `${bcSample.bottomLeft?.firstWrong}/${bcSample.bottomRight?.firstWrong})` +
              `${bcSample.error ? ` — ${bcSample.error}` : ''}`
  );
  // **THE CHECK THAT MAKES THE ABSENT BRANCH A CHECK.** Every platform runs it
  // and it can fail on either side: a device that lacks the feature must refuse
  // the texture this probe is built on, and a device that has it must not. It is
  // what stops "not supported here" arriving as "passed" on the Linux runner,
  // where the branch above is excused.
  const bcRefused = bcStart?.refusal !== null && bcStart?.refusal !== undefined;
  check(
    'AK',
    "the browser's own answer about bc1-rgba-unorm is the one wasm acted on — the real device refuses that texture where the feature is absent and accepts it where it is present",
    bcStart?.asked === true && bcRefused === !bcStart?.supported,
    bcStart?.asked !== true
      ? 'no GPUDevice to ask — the replayer has not opened one, so this proves nothing and ' +
          'fails rather than being skipped'
      : bcRefused === !bcStart?.supported
        ? bcStart.supported
          ? 'the device has the feature, wasm encoded the frame, and createTexture accepted a ' +
            'bc1-rgba-unorm texture with no validation error'
          : 'the device lacks the feature, wasm encoded nothing, and createTexture was refused ' +
            `by the browser — ${bcStart.refusal}`
        : bcStart.supported
          ? 'wasm reports the device HAS texture-compression-bc and the browser refused a ' +
            `bc1-rgba-unorm texture anyway — ${bcStart.refusal}`
          : 'wasm reports the device WITHOUT texture-compression-bc and the browser created a ' +
            'bc1-rgba-unorm texture regardless, so the probe skipped a frame this device could ' +
            'have run — which is exactly the shape that reads as covered while proving nothing'
  );

  // **THE PRESENT GATE, AND THE FIRST THAT PROVES THE REAL CANVAS-CONTEXT PATH.**
  // Every gate above rendered into a texture this replayer created; this one
  // renders into the frame a *canvas* handed back. wasm records a surface on the
  // a dedicated OffscreenCanvas the probe owns (see below), a swapchain
  // configured on it (a `configure` with COPY_SRC in the usage), an acquire
  // (`getCurrentTexture`), a pass that clears the acquired view, a copy of that
  // frame into a host buffer, a submit, a present (a no-op the browser composites
  // on rAF), and a readback; the page loop replays it; and the 16384 bytes that
  // come back are asserted against the clear colour, every pixel. A stub that
  // skipped the configure/acquire/render leaves a black/zero canvas and fails
  // here; only the real path — `context.configure`, `getCurrentTexture`, render,
  // `copyTextureToBuffer` off the acquired texture — leaves the colour.
  // `gpu-replay.mjs` cannot reach this: only a real browser has a canvas context.
  //
  // **AND THE SWAPCHAIN IT ASKS FOR IS sRGB, WHICH IS THE OTHER HALF OF THIS
  // GATE.** `GPUCanvasConfiguration.format` refuses an `-srgb` format outright, so
  // the page has to configure the base format, name the counterpart in
  // `viewFormats`, and create the acquired frame's view in it; skip either half
  // and the frame is written unencoded. That is not a subtle difference — every
  // pass above the seam writes display-referred values and leaves the encode to
  // the hardware, so an unencoded frame is the whole picture a transfer function
  // too dark, which is what the demo site shipped. The colour below is what
  // catches it: red is a fixed point of the sRGB encode and reads back identically
  // either way, so this probe clears to mid-tones instead and the two outcomes
  // share no component.
  group('X — a presented canvas frame is read back as the render colour');

  const PROBE_PRESENT_PIXELS = 64 * 64;
  // `crates/crcbl-webgpu/src/probe.rs`'s `PROBE_PRESENT_COLOR_BYTES` — the sRGB
  // encode of its `PROBE_PRESENT_COLOR`, `(0.25, 0.75, 0.125, 1.0)`. Spelled out
  // here rather than imported, as every expected value in this file is.
  const PROBE_PRESENT_COLOR_BYTES = [137, 225, 99, 255];
  // The same colour a LINEAR target would hold — `round(u * 255)` per component.
  // Not expected: it is the specific wrong answer, so the failure detail can say
  // "unencoded" rather than only "unequal".
  const PROBE_PRESENT_UNENCODED_BYTES = [64, 191, 32, 255];
  // `PROBE_PRESENT_COLOR_TOLERANCE`. The hardware evaluates the transfer function
  // and the specification fixes neither its precision nor its rounding — 0.75
  // encodes to 224.61, a tenth of a level from a byte boundary — so the window is
  // two levels wide. The nearest thing it must still separate is 34 levels away.
  const PROBE_PRESENT_TOLERANCE = 2;

  const presentStart = await evaluate(
    page,
    `(async () => {
     const { startPresentProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     // The present probe needs a canvas IT owns. The page's own canvas is the
     // one group H resolved a surface against, and configuring it here would
     // collide over the single GPUCanvasContext an element has — two devices
     // cannot configure one canvas, and they disagree on its preferred format
     // besides. So register a dedicated 64x64 OffscreenCanvas in the very
     // registry the replayer reads (gpu.canvases is that Map) and point the
     // probe at it: its device configures a context nothing else touches. The
     // page keeps canvas 1.
     const PRESENT_CANVAS_ID = 0x50;
     if (!gpu.canvases.has(PRESENT_CANVAS_ID)) {
       gpu.canvases.set(PRESENT_CANVAS_ID, new OffscreenCanvas(64, 64));
     }
     return {
       started: startPresentProbe({ exports, canvasId: PRESENT_CANVAS_ID }),
     };
   })()`
  );
  // Poll across frames exactly as the draw gate does: the page's rAF loop replays
  // the poll and delivers its reply between these evaluations. `readPresentProbe`
  // drains first, so a reply delivered since the last frame is absorbed before
  // another poll is queued.
  const present = presentStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readPresentProbe, pollPresentProbe, PRESENT } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readPresentProbe({ exports, memory: exports.memory });
           if (r.state === PRESENT.FAILED || r.state === PRESENT.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== PRESENT.READY) {
             // Not yet — queue another poll for the loop to replay, and wait.
             pollPresentProbe({ exports });
             return null;
           }
           // Ready: check every pixel here rather than shipping 16384 numbers
           // out. \`want\` is the sRGB encode of the colour the pass cleared the
           // acquired frame to; a black/zero texel is a canvas path that did not
           // run, and \`unencoded\` is the same clear written into a linear target.
           const want = ${JSON.stringify(PROBE_PRESENT_COLOR_BYTES)};
           const unencoded = ${JSON.stringify(PROBE_PRESENT_UNENCODED_BYTES)};
           const tolerance = ${PROBE_PRESENT_TOLERANCE};
           let allMatch = r.bytes.length === ${PROBE_PRESENT_PIXELS} * 4;
           let firstWrong = -1;
           for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
             if (Math.abs(r.bytes[i] - want[i % 4]) > tolerance) {
               allMatch = false;
               firstWrong = i;
             }
           }
           // Whether the frame that came back is the LINEAR one, checked on the
           // first texel only: that is the difference between "the canvas path is
           // broken" and "the canvas path ran and skipped the sRGB encode", and a
           // gate that cannot say which sends the next reader to the wrong half.
           const wasUnencoded =
             r.bytes.length >= 4 &&
             [0, 1, 2, 3].every(
               (c) => Math.abs(r.bytes[c] - unencoded[c]) <= tolerance
             );
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             allMatch,
             firstWrong,
             wasUnencoded,
             // The first two texels, for a failure message a human can read.
             sample: [...r.bytes.slice(0, 8)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'X',
    'wasm encoded the present setup frame — surface, swapchain, acquire, clear, copy, present, request',
    presentStart?.started === true,
    presentStart?.started
      ? 'the present-and-read frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'X',
    'the presented canvas frame came back from memory sRGB-encoded as the render colour, every pixel',
    present?.done === true &&
      present.allMatch === true &&
      present.len === PROBE_PRESENT_PIXELS * 4,
    present?.done !== true
      ? `no present readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : present.allMatch
        ? `${present.len} bytes, every texel [${PROBE_PRESENT_COLOR_BYTES.join(', ')}] (the sRGB encode of the clear) from the acquired canvas frame`
        : `state ${present.state}, ${present.len ?? 0} bytes, ` +
          `first wrong at byte ${present.firstWrong} (sample ${JSON.stringify(present.sample)} ` +
          `against [${PROBE_PRESENT_COLOR_BYTES.join(', ')}] ±${PROBE_PRESENT_TOLERANCE})` +
          (present.wasUnencoded
            ? ` — THE FRAME CAME BACK UNENCODED, [${PROBE_PRESENT_UNENCODED_BYTES.join(', ')}]: ` +
              'the canvas was configured without its -srgb viewFormat, or the acquired ' +
              'frame was viewed in the base format, and every frame it presents is a ' +
              'transfer function too dark'
            : '') +
          `${present.error ? ` — ${present.error}` : ''}`
  );

  // **THE RECONFIGURE GATE.** Group X proved a presented canvas frame reads back
  // as the render colour; this one proves a swapchain reconfigured in place takes
  // the NEW format. wasm creates the swapchain `Rgba8Unorm`, reconfigures it
  // `Bgra8Unorm`, then acquires, clears red, copies out and reads back on its own
  // dedicated OffscreenCanvas. Red in an `Rgba8Unorm` frame reads back as
  // [255, 0, 0, 255]; red in a `Bgra8Unorm` frame reads back in BGRA byte order as
  // [0, 0, 255, 255]. Asserting the latter is the proof the reconfigure actually
  // re-ran `context.configure` with the new format — a stub that skipped it leaves
  // the swapchain `Rgba8Unorm` and fails here. `gpu-replay.mjs` cannot reach this:
  // only a real browser has a canvas context.
  group('Y — a reconfigured swapchain presents in the new format');

  const PROBE_RECONFIG_PIXELS = 64 * 64;
  const PROBE_RECONFIG_COLOR_BYTES = [153, 102, 51, 255];

  const reconfigStart = await evaluate(
    page,
    `(async () => {
     const { startReconfigureProbe } = await import('/engine/gpu-probe.js');
     const { exports, gpu } = globalThis.crcbl;
     // Its own canvas, for group X's reason: configuring a canvas the page or
     // the present probe already drives collides over the one GPUCanvasContext.
     // A fresh 64x64 OffscreenCanvas under a fresh id, distinct from the present
     // probe's 0x50, so both gates can run in the one page.
     const RECONFIG_CANVAS_ID = 0x51;
     if (!gpu.canvases.has(RECONFIG_CANVAS_ID)) {
       gpu.canvases.set(RECONFIG_CANVAS_ID, new OffscreenCanvas(64, 64));
     }
     return {
       started: startReconfigureProbe({ exports, canvasId: RECONFIG_CANVAS_ID }),
     };
   })()`
  );
  // Poll across frames exactly as the present gate does: the page's rAF loop
  // replays the poll and delivers its reply between these evaluations.
  const reconfig = reconfigStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readReconfigureProbe, pollReconfigureProbe, RECONFIG } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readReconfigureProbe({ exports, memory: exports.memory });
           if (r.state === RECONFIG.FAILED || r.state === RECONFIG.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== RECONFIG.READY) {
             // Not yet — queue another poll for the loop to replay, and wait.
             pollReconfigureProbe({ exports });
             return null;
           }
           // Ready: check every pixel here rather than shipping 16384 numbers
           // out. \`want\` is the BGRA red the reconfigured (bgra8unorm) frame
           // holds; an rgba8unorm frame that skipped the reconfigure reads back
           // [255, 0, 0, 255] and fails.
           const want = ${JSON.stringify(PROBE_RECONFIG_COLOR_BYTES)};
           let allMatch = r.bytes.length === ${PROBE_RECONFIG_PIXELS} * 4;
           let firstWrong = -1;
           for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
             if (r.bytes[i] !== want[i % 4]) {
               allMatch = false;
               firstWrong = i;
             }
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             allMatch,
             firstWrong,
             // The first two texels, for a failure message a human can read.
             sample: [...r.bytes.slice(0, 8)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'Y',
    'wasm encoded the reconfigure setup frame — surface, swapchain, reconfigure, acquire, clear, copy, present, request',
    reconfigStart?.started === true,
    reconfigStart?.started
      ? 'the reconfigure-and-read frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'Y',
    'the reconfigured canvas frame came back in the new format, every pixel',
    reconfig?.done === true &&
      reconfig.allMatch === true &&
      reconfig.len === PROBE_RECONFIG_PIXELS * 4,
    reconfig?.done !== true
      ? `no reconfigure readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : reconfig.allMatch
        ? `${reconfig.len} bytes, every texel [${PROBE_RECONFIG_COLOR_BYTES.join(', ')}] (bgra 0.2/0.4/0.6) from the reconfigured canvas frame`
        : `state ${reconfig.state}, ${reconfig.len ?? 0} bytes, ` +
          `first wrong at byte ${reconfig.firstWrong} (sample ${JSON.stringify(reconfig.sample)} ` +
          `against [${PROBE_RECONFIG_COLOR_BYTES.join(', ')}])` +
          `${reconfig.error ? ` — ${reconfig.error}` : ''}`
  );

  // **THE INDIRECT-DRAW GATE, AND THE ONLY EVIDENCE ANYWHERE FOR
  // `Capability::IndirectArgumentPaddedStride` ON THIS BACKEND.**
  // Group T proved a direct `draw` overwrites a clear. This proves a
  // `drawIndexedIndirect` — the live 3D-forward geometry path — puts exactly the
  // same pixels there: wasm records the same fullscreen-triangle pipeline, fills
  // an index buffer with `[0,1,2,0]` and an indirect-args buffer with THREE
  // argument structures via `write_buffer`, clears the texture blue, binds the
  // pipeline and index buffer, and records `drawIndexedIndirect` reading TWO of
  // them at a PADDED stride of two structures; the page loop replays it, which
  // means `gpu-replay.js` unrolling the draw into one `drawIndirect` per
  // structure at `offset + i * stride`; and the bytes that come back are the red
  // draw colour, every pixel — not the blue clear.
  //
  // WHICH STRUCTURE DRAWS IS THE WHOLE CHECK. The first structure draws nothing
  // and so does the decoy sitting one *tight* stride in; only the structure the
  // padded stride reaches has the triangle's three indices. So blue is left by a
  // stub that skips the draw, by a replayer that walked the arguments tightly,
  // and by one that ignored the draw count — and red is left only by an indirect
  // draw that rasterised from the offset it was told to. `crcbl_webgpu::probe`'s
  // `PROBE_INDIRECT_ARGS_BYTES` is where that layout is written and asserted.
  // `gpu-replay.mjs` cannot reach this for group S's reason: only a real device
  // reads args from a buffer and rasterises.
  group(
    'Z — an indirect draw walking a padded stride is read back as the draw colour, not the clear'
  );

  const PROBE_INDIRECT_PIXELS = 64 * 64;
  const PROBE_INDIRECT_COLOR_BYTES = [255, 0, 0, 255];
  const PROBE_INDIRECT_CLEAR_BYTES = [64, 128, 191, 255];

  const indirectStart = await evaluate(
    page,
    `(async () => {
     const { startIndirectProbe } = await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     return { started: startIndirectProbe({ exports }) };
   })()`
  );
  // Poll across frames exactly as group T does: the page's rAF loop replays the
  // poll and delivers its reply between these evaluations, and each evaluation
  // drains what has arrived, then queues another poll while the map is still
  // resolving. `readIndirectProbe` drains first, so a reply delivered since the
  // last frame is absorbed before another poll is queued.
  const indirect = indirectStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readIndirectProbe, pollIndirectProbe, INDIRECT } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readIndirectProbe({ exports, memory: exports.memory });
           if (r.state === INDIRECT.FAILED || r.state === INDIRECT.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== INDIRECT.READY) {
             // Not yet — queue another poll for the loop to replay, and wait.
             pollIndirectProbe({ exports });
             return null;
           }
           // Ready: check every pixel here rather than shipping 16384 numbers
           // out. \`want\` is the fragment's constant red; the clear beneath it
           // was blue, so a texel still blue is an indirect draw that did not
           // happen — or one that read its second argument structure somewhere
           // other than where the padded stride put it.
           const want = ${JSON.stringify(PROBE_INDIRECT_COLOR_BYTES)};
           let allMatch = r.bytes.length === ${PROBE_INDIRECT_PIXELS} * 4;
           let firstWrong = -1;
           for (let i = 0; i < r.bytes.length && allMatch; i += 1) {
             if (r.bytes[i] !== want[i % 4]) {
               allMatch = false;
               firstWrong = i;
             }
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             allMatch,
             firstWrong,
             // The first two texels, for a failure message a human can read.
             sample: [...r.bytes.slice(0, 8)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'Z',
    'wasm encoded the indirect setup frame — pipeline, args and index writes, clear, bind, indirect draw, copy, request',
    indirectStart?.started === true,
    indirectStart?.started
      ? 'the indirect-draw-and-read frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'Z',
    'the indirect-drawn pixels came back as the draw colour, not the clear, every one — so the draw read its second argument structure at the padded stride',
    indirect?.done === true &&
      indirect.allMatch === true &&
      indirect.len === PROBE_INDIRECT_PIXELS * 4,
    indirect?.done !== true
      ? `no indirect readback in ${TIMEOUT_MS} ms — the map never resolved or the reply never reached wasm`
      : indirect.allMatch
        ? `${indirect.len} bytes, every texel [${PROBE_INDIRECT_COLOR_BYTES.join(', ')}] (red), not the clear [${PROBE_INDIRECT_CLEAR_BYTES.join(', ')}] (blue)`
        : `state ${indirect.state}, ${indirect.len ?? 0} bytes, ` +
          `first wrong at byte ${indirect.firstWrong} (sample ${JSON.stringify(indirect.sample)} ` +
          `against [${PROBE_INDIRECT_COLOR_BYTES.join(', ')}]; the clear was [${PROBE_INDIRECT_CLEAR_BYTES.join(', ')}])` +
          `${indirect.error ? ` — ${indirect.error}` : ''}`
  );

  // **THE DEPTH GATE, AND THE ONLY PLACE `Capability::DepthImageCopy` IS
  // EXERCISED ON THIS BACKEND.** The seam suite that holds every other backend to
  // its capability declarations is a native binary; this backend runs in a
  // browser, so a `Support::Yes` there is a claim and this is the evidence. wasm
  // records a `depth32float` atlas, a pass whose only attachment is that atlas
  // cleared to a known depth and stored, an image→buffer copy of its DEPTH PLANE
  // — `ImageAspect::DEPTH`, whole subresource, which is all WebGPU permits — a
  // submit and a readback; the page loop replays it; and the 16384 floats that
  // come back are asserted to be the clear value.
  //
  // Three ways it fails loudly rather than passing wrongly. A replayer that
  // refuses the depth format at all queues a device error and the readback never
  // arrives. One that copied the wrong plane, or read the row pitch as a colour
  // format's, returns floats that are not the clear. One that left the aspect at
  // WebGPU's `'all'` default has its whole command buffer rejected. And the clear
  // is deliberately neither 0.0 nor 1.0, which are the two values an untouched
  // depth plane holds.
  group('AA — a cleared depth plane is copied out and read back');

  const PROBE_DEPTH_TEXELS = 64 * 64;
  // `PROBE_DEPTH_CLEAR` in `crates/crcbl-webgpu/src/probe.rs`, restated by hand:
  // a value imported from the thing under test agrees with it by construction.
  const PROBE_DEPTH_CLEAR = 0.4275;
  // The float comparison's slack. A `depth32float` plane stores the clear value
  // itself, so an exact match is what a correct path produces; this is wide
  // enough to survive the f32 round trip and far narrower than the distance to
  // either 0.0 or 1.0, which is what a plane nothing wrote reads back as.
  const PROBE_DEPTH_TOLERANCE = 1e-6;

  const depthStart = await evaluate(
    page,
    `(async () => {
     const { startDepthProbe } = await import('/engine/gpu-probe.js');
     const { exports } = globalThis.crcbl;
     return { started: startDepthProbe({ exports }) };
   })()`
  );
  const depth = depthStart?.started
    ? await until(async () =>
        evaluate(
          page,
          `(async () => {
           const { readDepthProbe, pollDepthProbe, DEPTH } =
             await import('/engine/gpu-probe.js');
           const { exports, gpu } = globalThis.crcbl;
           const r = readDepthProbe({ exports, memory: exports.memory });
           if (r.state === DEPTH.FAILED || r.state === DEPTH.UNDECODABLE) {
             const error = gpu.replayer.takeError();
             return { done: true, state: r.name, error: r.reason || error };
           }
           if (r.state !== DEPTH.READY) {
             pollDepthProbe({ exports });
             return null;
           }
           // Ready: compare the 16384 floats here rather than shipping them out.
           // The bytes are a copy already, so the view is safe to build over
           // them.
           const want = ${PROBE_DEPTH_CLEAR};
           const tolerance = ${PROBE_DEPTH_TOLERANCE};
           let allMatch = r.bytes.length === ${PROBE_DEPTH_TEXELS} * 4;
           let firstWrong = -1;
           let firstWrongValue = null;
           if (allMatch) {
             const depths = new Float32Array(
               r.bytes.buffer,
               r.bytes.byteOffset,
               r.bytes.length / 4
             );
             for (let i = 0; i < depths.length; i += 1) {
               if (Math.abs(depths[i] - want) > tolerance) {
                 allMatch = false;
                 firstWrong = i;
                 firstWrongValue = depths[i];
                 break;
               }
             }
           }
           return {
             done: true,
             state: r.name,
             len: r.bytes.length,
             allMatch,
             firstWrong,
             firstWrongValue,
             sample: [...r.bytes.slice(0, 4)],
             error: gpu.replayer.takeError(),
           };
         })()`
        )
      )
    : null;
  check(
    'AA',
    'wasm encoded the depth setup frame — a depth32float atlas, a clear-and-store pass, the depth-plane copy, the request',
    depthStart?.started === true,
    depthStart?.started
      ? 'the depth clear-and-copy frame is on the stream'
      : 'wasm would not encode it — no device has opened, or another channel is installed'
  );
  check(
    'AA',
    'the depth plane came back from the browser as the cleared depth, every texel',
    depth?.done === true &&
      depth.allMatch === true &&
      depth.len === PROBE_DEPTH_TEXELS * 4,
    depth?.done !== true
      ? `no depth readback in ${TIMEOUT_MS} ms — the copy was refused, the map never resolved, or the reply never reached wasm`
      : depth.allMatch
        ? `${depth.len} bytes, every depth32float texel ${PROBE_DEPTH_CLEAR} — neither the 0.0 nor the 1.0 an unwritten plane reads back as`
        : `state ${depth.state}, ${depth.len ?? 0} bytes, ` +
          `first wrong at texel ${depth.firstWrong} (${depth.firstWrongValue} against ${PROBE_DEPTH_CLEAR}; ` +
          `the first four bytes were ${JSON.stringify(depth.sample)})` +
          `${depth.error ? ` — ${depth.error}` : ''}`
  );

  // **THE PARITY REPORT, AND THE ONLY GROUP HERE THAT CONSTRUCTS A
  // `WebGpuDevice` AT ALL.** Every other group drives `StreamWriter` and the
  // replayer: `crates/crcbl-webgpu/src/probe.rs` imports the writer, the
  // transport and the reply decoder and nothing from `crate::hal`, so the
  // groups above prove the encoding and the replay and prove nothing about
  // `impl Device` — which is where every `Support` answer lives. The native
  // seam suite that holds the other four backends to their declarations
  // (`the_parity_report_matches_the_reviewed_divergence_list` in
  // `crates/crcbl/tests/hal_seam_e2e.rs`) is a native binary and cannot reach a
  // browser. So until this group existed, every `supports()` answer this
  // backend gives was checked by nothing anywhere.
  //
  // WHAT IT ACTUALLY DOES: wasm builds a `WebGpuDevice` around the `DeviceCaps`
  // the browser reported for the device this page opened in group G, walks
  // `Capability::ALL` through `Device::supports`, and runs each answer through
  // `crcbl_hal::parity_verdict` against `DIVERGENCES`. Both directions fail: a
  // refusal with no row saying why, and a row for something this device turns
  // out to have. The comparison is in Rust because `DIVERGENCES` is Rust data
  // — nothing on this side of the boundary can see it — so what crosses is the
  // matrix and the disagreements.
  //
  // WHERE IT BELONGS IN THE RUN, and why last is safe: this is the one probe
  // that asks the browser nothing. It puts no command on the stream, registers
  // no wait and drains nothing, so it cannot be stranded by work queued ahead
  // of it — which matters on Windows, where X, Y, Z and AA are expected to fail
  // because a stuck canvas readback strands everything behind it on the single
  // queue. Those four time out rather than hanging, so the run reaches this
  // group; if they ever became a hang, this group would move to just before X.
  // All it needs is that group G's device opened.
  group(
    'AB — a WebGpuDevice’s supports() matrix matches the reviewed divergence list'
  );

  const parity = await evaluate(
    page,
    `(async () => {
     const { runParityProbe } = await import('/engine/gpu-probe.js');
     const { exports, memory } = globalThis.crcbl;
     return runParityProbe({ exports, memory });
   })()`
  );

  // The vacuity guard, and it is the whole reason `checked` and `held` are
  // exported beside the verdict. A report that walked no capabilities agrees
  // with every list there is; one where every capability was left unprovable by
  // a device that withheld its gating feature settled nothing. Both would print
  // `MATCHED`. The token count is checked against `checked` as well, so a matrix
  // that lost entries on the way out is not read as a shorter run.
  const tokens = parity?.report ? parity.report.split(' ') : [];
  check(
    'AB',
    'a WebGpuDevice was built on the caps the browser reported and its whole supports() matrix came back',
    parity?.name !== undefined &&
      parity.name !== 'NO_DEVICE' &&
      parity.checked > 0 &&
      parity.held > 0 &&
      tokens.length === parity.checked,
    parity?.name === undefined
      ? 'the parity export answered nothing — no wasm instance on the page'
      : parity.name === 'NO_DEVICE'
        ? 'no device has opened, so there were no caps to build a WebGpuDevice around'
        : `${parity.checked} capabilities, ${parity.held} settled and ` +
          `${parity.checked - parity.held} unprovable on this device: ${parity.report}`
  );

  // The verdict. `failures` is asserted empty beside it rather than instead of
  // it: the state and the text are written from the same walk, and a check on
  // one alone would let a mismatch with no message — or a message with no
  // mismatch — through.
  check(
    'AB',
    'every capability’s declaration agrees with crcbl_hal::DIVERGENCES, in both directions',
    parity?.name === 'MATCHED' && parity.failures === '',
    parity?.name === 'MATCHED' && parity.failures === ''
      ? `${parity.held} settled declarations, every one reviewed`
      : `${parity?.name}: ` +
          (parity?.failures
            ? parity.failures.trimEnd().split('\n').join(' | ')
            : 'no report ran')
  );
}
