# Backends — records

Records kept so they are not re-derived: measurements, investigations, ideas
considered and declined, and lessons. Open work lives in `docs/backlog.md`.

## Metal binds by reflection (2026-09-07)

`crcbl-mtl` now creates every raster and compute pipeline with
`MTLPipelineOption::BindingInfo`, reduces the reflection's per-stage
`MTLBinding` list to a bitmask in `crcbl_mtl::binding_mask`, and skips any
argument-table `set*` the compiled pipeline's own `isUsed` says it cannot read.
`crcbl_render::forward`'s mesh layout gained `ShaderStages::FRAGMENT` on
bindings 1 to 5 in the same change. This is the record of what was measured,
what was decided, and what is still owed to a device.

**The finding.** CI's `mtl e2e (macos-latest)` log under
`MTL_DEBUG_LAYER_WARNING_MODE=nslog` carries two warning classes from the mesh
suite, and they are opposite halves of one question:

- `Fragment Function(fragmentMain): missing Buffer binding at index 3/4/5`.
  Slang's Metal emission materialises every module global into every entry
  point, so `msl/mesh.metal`'s `fragmentMain` declares `draw`, `meshes`,
  `visible_instances`, `vertices` and `instances` as parameters. The layout made
  bindings 1 to 5 visible to the geometry stage alone, so the fragment argument
  table held nothing at those indices. `crcbl-shaders`'
  `the_mesh_fragment_stage_stores_the_geometry_globals_without_reading_them` is
  what now holds the artifact to "declared, stored into `KernelContext`, never
  dereferenced" — and its doc comment says what a future real read would oblige.
- `unused binding in encoder at Buffer index 6/7/8/9` and `index 0`. The
  material table, light list, froxel grid and probe rows are declared
  `geometry | FRAGMENT` and bound on both stages, and only one stage reads each;
  `frame` is bound to the fragment stage of `depthMaskedFragmentMain` and
  `rsmFragmentMain`, neither of which reads it.

**Why the layout cannot answer either.** A `BindGroupLayoutEntry`'s visibility
says which stages are _permitted_ to reach a binding; Slang's materialisation
means the permission and the read are different sets, on the same shader, in
both directions. Only the compiled pipeline knows, and only Metal will say. So
the layout is now the permission and the reflection is the decision, which is
what makes widening bindings 1 to 5 to `FRAGMENT` free rather than a trade of
one warning class for the other.

**Why 3, 4 and 5 and not 1 and 2 is still unknown.** `vertices [[buffer(1)]]`
and `instances [[buffer(2)]]` are declared by `fragmentMain` on exactly the same
terms as the three the layer complains about, and the layer says nothing about
them. The only reading consistent with both classes is that the
`missing binding` check consults `isUsed` too, and that the Metal compiler kept
the argument alive for three of the five and not the other two. Nothing here can
test that: it needs `MTLBinding::isUsed` read on a device.

**The quirk decision.** The brief this was built from said `crcbl_mtl::quirk`'s
mesh section records the reflection selector misbehaving on the paravirtual GPU.
It does not. What `check_mesh_support` records is stronger and simpler: that
device answers `false` to `supportsFamily:MTLGPUFamilyMetal3`, so a Metal mesh
pipeline cannot be created on it at all and there is nothing for a mask to be
wrong about. Mesh pipelines take `BindingMask::all()` anyway, for a reason of
this backend's own rather than that device's: `crcbl_mtl::binding` binds through
`setVertexBuffer:`/`setFragmentBuffer:` and has no object or mesh sibling, so
`objectBindings`/`meshBindings` describe tables this backend does not write, and
mapping one onto the other would be a guess. The module's rule is that a
fallback never binds _less_.

**Ordering is the sharp edge, and it is structural.** The seam does not require
a pipeline to be bound before `bind_group` — it takes a pipeline _layout_, as
`vkCmdBindDescriptorSets` does — so the mask that decides a group's binds may
not exist when the group arrives and changes under it whenever a pipeline with a
different one is bound. A pass therefore opens holding `BindingMask::none()`,
`crcbl_mtl::command` keeps the groups in force per slot, and a pipeline bind
re-applies only those whose mask actually moved. The second edge is that a
_skipped_ bind must not be recorded in `crcbl_mtl::bind_cache` as made,
otherwise the next pipeline that does read the slot is told the argument is
already there. That is `BindingMask::issue`, which takes the cache call as a
closure so the order is the compiler's business rather than a convention
repeated at every bind site.

**What it cost.** `crcbl_render::forward`'s mesh layout was at
`PORTABLE_STORAGE_BUFFERS_PER_STAGE` in the vertex stage with no headroom; it is
now at the same ceiling in the fragment stage too, because bindings 1, 2, 4 and
5 are storage buffers. A row added to either raster stage is now a renderer that
cannot be built in a browser, and `check_portable_storage_buffers` is what says
so at the layout rather than at somebody else's `createPipelineLayout`.

**What was verified, and where.** On this machine, which has no Metal device:
the mask module's own host tests; a cross-target
`cargo clippy --target aarch64-apple-darwin` and the matching
`cargo doc --document-private-items`, which type-check and document every
`cfg(target_os = "macos")` line of the change; `crcbl-shaders`' two new artifact
guards, both shown red by splicing a read and a moved index into a copy of the
text; and the Vulkan render and forward e2e suites on radv and lavapipe, which
is what says the widened visibility moved no pixel.

**What is owed to CI.** Everything about Metal itself. Nothing here executed a
Metal call, `crcbl_mtl::binding`'s masked `apply` has never run, and the
`crcbl-mtl` host suite does not compile it. The measurement that says whether
this worked is the `mtl e2e (macos-latest)` job's warning counts under
`MTL_DEBUG_LAYER_WARNING_MODE=nslog` — both classes should go to zero on a
device whose reflection is faithful — and the `cargo test` in the macOS job is
the only thing that runs `crcbl-mtl`'s new
`the_forward_mesh_layout_flattens_onto_the_artifacts_argument_tables`, because
`crate::binding` is `cfg(target_os = "macos")` and its tests are not host tests.

### SHIPPED — the depth-bias constant is an `i32` on the seam

Closes "WebGPU cannot carry a fractional depth-bias constant", decided and built
2026-09-06. `crcbl_hal::DepthBias::constant` was an `f32`; it is now an `i32`,
and the WebGPU replayer's refusal of a fractional one is gone because the type
can no longer express one.

**Why the integer is the right side to land on.** The field counts the depth
buffer's minimum resolvable difference, and the two APIs that name that count in
their own descriptor both spell it as a signed 32-bit integer:
`D3D12_RASTERIZER_DESC::DepthBias` and WebGPU's `GPUDepthBias`. WebGPU will not
take anything else — its `[EnforceRange] long` conversion throws synchronously
out of `createRenderPipeline` on a non-integer — so on that backend a fraction
was never a smaller nudge, it was a refused pipeline. wgpu's
`DepthBiasState::constant` is an `i32` for the same reason. Vulkan and Metal
spell the count as a float (`depthBiasConstantFactor`,
`setDepthBias:slopeScale:clamp:`), and both take an integral value exactly, so
they were the two that could give ground.

**The one thing an `f32` bought and this gives up**, stated so it is not
rediscovered as a bug: `crates/crcbl-vk/src/pipeline.rs` and
`crates/crcbl-mtl/src/pipeline.rs` widen the `i32` to the `f32` their API takes,
which is exact only up to the 24-bit significand — `2^24 + 1` reaches those two
as `2^24`. D3D12 and WebGPU carry the full `i32`. Nothing in the engine is
anywhere near that magnitude (the renderer's own pipelines all use
`DepthBias::default()`, and sundial's shadow bias is a separate console constant
counted in cascade texels, not this field), so the divergence is documented
rather than guarded.

**The wire moved with it.** `tag::STREAM_VERSION` is `6`: the constant crosses
through `put_i32`/`read_i32` instead of `put_f32`/`read_f32`, and
`web/engine/gpu-stream.js`'s `readDepthStencilState` reads a signed word. Four
bytes stayed four bytes, which is precisely the changed-record shape only a
version word can catch — an older decoder takes a small positive constant as a
denormal and a negative one as a NaN, refuses neither, and finds every byte
after it still lined up.
`crates/crcbl-webgpu/tests/fixtures/canonical-stream.bin` was re-blessed;
`cmp -l` against the old file shows two runs and nothing else, the version byte
and the four bias bytes (`00 00 00 C0` → `FE FF FF FF`).

**What was verified, and where.** The wire claim is
`the_bias_constant_crosses_as_an_integer_rather_than_as_a_float` in
`crates/crcbl-webgpu/tests/stream.rs`, which round-trips `2^24 + 1` — the
smallest positive integer an `f32` cannot hold — so a stream still carrying the
constant as a float decodes one short. Shown red by putting `put_f32`/`read_f32`
back: `left: 16777216, right: 16777217`. The replayer's side is a check in
`web/tools/gpu-replay.mjs` that the decoded constant reaches
`createRenderPipeline` as an integer unchanged, shown red by adding `0.5` to it.
D3D12's `the_depth_bias_constant_reaches_d3d12_whole` in
`crates/crcbl-dx12/src/pipeline.rs` pins that nothing truncates any more — it
runs in CI's `build + test (windows-latest)` sweep, not on this machine, so it
has been type-checked here and never executed. Metal's widening has no test at
all; `crates/crcbl-mtl/src/pipeline.rs`'s `raster_state` is only type-checked,
by the cross-target clippy pass.

### SHIPPED — `crcbl-mtl` stops re-binding what the encoder already holds

Half (1) of "Metal's debug layer warns on every bind, and the mesh suite pays
for it", built 2026-09-06. The other half — the "unused binding" class, which is
a bind-layout question — is untouched and stays in `docs/backlog.md`.

**What is in the tree now.** `crates/crcbl-mtl/src/bind_cache.rs` is a plain
Rust mirror of one encoder's argument tables: a `Stage` (vertex, fragment,
compute — Metal's three independent table sets), a slot number, and what that
slot last had put in it. `crcbl_mtl::binding`'s `apply` and `apply_compute` and
`crcbl_mtl::command`'s `push_constants` ask it before every `set*` and make the
call only when the answer differs, which is the state cache wgpu-hal's Metal
`CommandState` and MoltenVK's `MVKResourcesCommandEncoderState` both carry.

Four decisions in it are worth not re-deriving:

- **A resource is its address.** `ResourceId` holds the object's pointer as a
  `usize` and nothing else, which is what keeps the module free of Objective-C
  and therefore testable on a Linux host. The reuse hazard — a freed object's
  address handed to the next allocation, so a stale entry matches a different
  resource — cannot arise, because an address only enters the cache when its
  object is passed to a `set*` on an encoder of this command buffer, and an
  `MTLCommandBuffer` retains every resource it references. That is the same fact
  `crcbl_mtl::device`'s header already rests on when it explains why this
  backend has no deletion queue; the cache adds no new assumption.
- **`setBytes:` is compared by its bytes.** It copies its argument into the
  encoder and leaves no object to key on, so two calls are interchangeable
  exactly when the blocks are equal — same length, same contents. The
  alternative, not caching inline blocks at all, was declined: the push-constant
  block is re-sent whole at every write (`crcbl_mtl::argument` says why), so an
  unchanged block is exactly the repeat this is for, and the comparison is a
  `memcmp` over a block bounded by `Limits::max_push_constant_size`.
- **`setBuffer:` and `setBytes:` share one table.** They write the same
  argument-table entry, so the cache models a buffer slot as one of the two
  rather than as two independent entries. Without that, a pipeline layout whose
  push-constant block lands at the index the previous layout bound a buffer at
  would find the buffer's record there and skip a write it has to make.
  `an_inline_block_and_a_buffer_share_one_slot` is the test.
- **`useResource:usage:` is not cached, and neither is the pack encoder.**
  Residency is a declaration rather than a table slot, and the debug layer has
  no "redundant" finding for it; the count-limited draw's prologue
  (`encode_pack`) binds three buffers per dispatch on a compute encoder of its
  own and is not part of what the job's log was counting.

The cache lives exactly as long as the encoder it mirrors, because Metal's
argument tables do: `endEncoding` takes them and the next encoder starts empty.
So `encode_render_pass` keeps one on the stack beside the
`MTLRenderCommandEncoder` it opens and ends there, and
`MetalCommandEncoder::binds` holds the compute pass's, cleared by `close_open`
with the rest of the state an encoder takes with it.

`RenderCommand::PushConstants` now carries `slot: u32` where it carried an
already-widened `NSUInteger`, because the cache keys on the seam's own width and
`to_ns` is what the Metal call takes.

**What was verified, and where.** This machine has no Metal, so nothing here is
a measurement of the flood. What ran green:
`cargo clippy -p crcbl-mtl --all-targets --all-features --locked --target aarch64-apple-darwin -- -D warnings`
and the same crate's `cargo doc` with `-D warnings` on that target, which is
what type-checks every call site; and nine host unit tests in `bind_cache`,
which `cargo test -p crcbl-mtl` runs on any host. Both directions of the cache
were shown to fail: made to never report a hit, four of the nine go red; made to
always report one, eight do.

**What the reviewer has to read to close this.** The `mtl e2e (macos-latest)`
job, and two numbers in it. First the per-step durations, against the 37 / 29 /
25 / 22 minutes the backlog entry recorded for "Draw the lit mesh on Metal".
Second the count of `Set Vertex Texture Validation` lines in that step's log,
against the 161,289 the entry counted for its first twenty minutes — and of the
five other finding texts beside it. A drop in the "redundant setting" texts is
this change; whatever remains is the "unused binding" half, which is still there
by design.

### SHIPPED — a read-only depth state that said it writes

Record of the decision and of what landed. Both narrowings were taken on
2026-09-06 and built the same day.

**What is in the tree now.** `crcbl_hal::ResourceState::is_write` answers
`false` for `DepthStencilRead`, and `crcbl-vk`'s `conv::state_masks` expands it
to `DEPTH_STENCIL_ATTACHMENT_READ` alone. The two had to move together, because
`conv`'s `write_states_expand_to_write_accesses` asserts the equality, and that
test is now what holds the narrowing in place from the Vulkan side while
`write_states_are_classified` holds it from the seam's.

`crcbl-mtl` gained `conv::depth_store_action`, which answers
`MTLStoreAction::Store` for a read-only attachment whatever store op the caller
passed, and `crcbl-mtl`'s `command` uses it for both the depth and the stencil
plane. Metal has no no-op store action, so the texture is written back either
way and the only choice is between storing contents the pass did not change and
discarding them; storing keeps the depth buffer, and unchanged contents are why
declaring no write still holds. wgpu-hal's Metal backend answers `Store` under
`depthReadOnly` for the same reason.

Nothing changed in `crcbl-dx12` (`conv::resource_state` already mapped the state
to `D3D12_RESOURCE_STATE_DEPTH_READ`), in `crcbl-webgpu` (a read-only plane
reaches WebGPU as `depthReadOnly` with no store op at all) or in `crcbl-hal`'s
`Null` backend, which maps no states.

**The consequence at the graph.** `ResourceState::needs_barrier` now sees
read→read in one layout between two depth-test-only passes and emits nothing;
`crcbl-render`'s `back_to_back_read_only_depth_passes_need_no_barrier` is the
same test that used to require the barrier, flipped. The transition out of the
prepass is a write→read and is unchanged.

**The narrowing is what makes the Vulkan store op observable on a device**,
which it was not before — the gap the entry this replaces had to state. With the
write bit in the mask, a barrier out of `DepthStencilRead` covered the store
whether or not one happened, so the suites passed either way. Without it, the
barrier's source scope is `VK_ACCESS_2_DEPTH_STENCIL_ATTACHMENT_READ_BIT` alone
and the layer notices. Measured 2026-09-06 by making `conv::depth_store_op`
answer `STORE` for a read-only attachment and running
`crates/crcbl/tests/run-render-e2e.sh` on lavapipe with
`CRCBL_VK_SYNC_VALIDATION`, which reported
`32/58 tests run: 12 passed, 20 failed` before the runner gave up, every failure
reading

> `SYNC-HAZARD-WRITE-AFTER-WRITE: vkCmdPipelineBarrier2(): WRITE_AFTER_WRITE hazard detected […] which was previously written at the end of the render pass instance (vkCmdEndRendering) by the attachment storeOp.`

So the two halves now hold each other up on hardware, and the
`vk e2e (lavapipe)` job is where that verdict comes from.

**Coverage gap, stated plainly:** nothing observes the Metal store action on a
device. `conv`'s `load_and_store_actions_are_not_transposed` pins the mapping,
and it only compiles here — the cross-target clippy run for
`aarch64-apple-darwin` is the whole local verdict on that crate, and the
`mtl e2e (macos-latest)` job is the only one that runs it at all.

The argument that produced the decision follows.

`crcbl-vk` stopped storing a read-only depth attachment first:
`conv::depth_store_op` answers `VK_ATTACHMENT_STORE_OP_NONE` — Vulkan 1.3 core,
no extension and no feature — whenever
`crcbl_hal::DepthStencilAttachment::read_only` is set. **No seam change was
needed** for that half: the flag already existed and `crcbl-render`'s
`graph.rs::attachments` already set it from the pass's own `write` flag, which
the entry this replaces had wrong.

What was left is what this entry closes: `ResourceState::DepthStencilRead` still
declared `DEPTH_STENCIL_ATTACHMENT_WRITE` in `conv::state_masks`, and
`crcbl_hal::ResourceState::is_write` still answered `true` for it. Both were
conservatism rather than description.

**Measured** against CI's own layer 1.3.275 and lavapipe from Ubuntu, driving
`cargo test -p viewer` with `CRCBL_GPU=vk` and the fatal sync gate on. The
viewer's ground grid was chosen because it tests depth without writing it and is
last to touch the image — **it is no longer the only pass that reads depth
read-only**, re-checked 2026-09-02: `crcbl_render::sky_pass`,
`crcbl_render::debug_draw` (which cites `crate::grid`'s terms in its own
comment) and `forward.rs`'s wireframe fill path all take `depth_read` or
`DepthStencilState::equal_depth_read_only`. Whether any of them is _last_ to
touch the image in a frame was not established, so the fixture stands as
measured and this note only retires the "only pass" clause:

| store op | vk access mask | `is_write` | result                                          |
| -------- | -------------- | ---------- | ----------------------------------------------- |
| `STORE`  | write declared | `true`     | 75 passed (what shipped before this)            |
| `STORE`  | read only      | `false`    | **15 failed** — the original bug                |
| `STORE`  | read only      | `true`     | **15 failed** — `is_write` alone never fixed it |
| `NONE`   | read only      | `false`    | 75 passed (what ships now)                      |
| `NONE`   | read only      | `true`     | 75 passed                                       |

So on Vulkan the write declaration was unnecessary and only cost barrier
strength on every depth-test-only pass. **The question was the other backends**,
because `is_write` answers for all four:

- **WebGPU: does not write.** `web/engine/gpu-replay.js` omits all four
  load/store ops on a read-only plane and sets `depthReadOnly`, which the
  specification requires.
- **DX12: does not write.** D3D12 has no store op at all, and a depth-test-only
  pass runs with `D3D12_DEPTH_WRITE_MASK_ZERO`. Read, not measured — there is no
  D3D12 machine here.
- **Metal: writes, and cannot not.** `MTLStoreAction` has no no-op action, so
  the texture is written back either way. That is what the explicit read-only
  arm answers: the bytes are the ones the attachment already held, so no reader
  can miss anything. Read, not measured, and Metal is deferred.

The options were: narrow both and accept Metal's arm being argued rather than
measured; narrow both and give Metal's `store_action` an explicit read-only arm
first; or leave it, at the cost of one unnecessary barrier per read-only depth
pass. **The second was taken.** Nothing was wrong before either way — the
conservative answer over-synchronised, it did not race.

### `Features::BUFFER_DEVICE_ADDRESS` on Metal rides a query that is wrong

`crcbl_mtl::adapter::features_of` reports it from
`supportsFamily(MTLGPUFamily::Metal3)`, on the reasoning that `gpuAddress` is a
Metal 3 property. That reasoning is **measurably false**: CI's
`Apple Paravirtual device` answers `supportsFamily(Metal3) = false` and returns
usable `gpuAddress` values anyway — `crcbl_mtl::binding`'s bindless probe read
four non-zero addresses on it and a kernel dereferenced every one. The family
query describes a feature set; the selector's availability is a macOS version
question, and they are not the same.

`Features::DESCRIPTOR_INDEXING` had the same gate and was fixed, because leaving
it would have reported the bindless capability closed while switching it off on
the one device that had proven it. **`BUFFER_DEVICE_ADDRESS` was deliberately
left alone**: nothing on this backend exercises `BufferUsage::DEVICE_ADDRESS`,
so correcting the gate would turn on a path no test covers — adding unproven
surface rather than fixing a measured defect. The honest fix, when something
needs it, is `respondsToSelector:` on `gpuAddress`, which is what
`DESCRIPTOR_INDEXING` uses now.

Worth knowing generally: **`supportsFamily:` is not an availability check.** Any
other capability here gated on a family as a proxy for "this selector exists" is
wrong the same way.

**Swept, and this is the only one.** Every other `supportsFamily:` in
`crcbl-mtl` is a probe printing what a device answers, not a gate deciding a
capability. Metal's mesh rows in particular do **not** ride a family query. They
did read "this backend builds no `MTLMeshRenderPipelineDescriptor`" until the
mesh slice landed one; `DIVERGENCES` now says the calls exist and no device has
run them, which is why `crcbl_mtl::adapter` reports no `Features::MESH_SHADER`
and the rows stay `Unwritten`. The runner's `Metal3 = false` is why they cannot
be _verified_ here, not why they are reported unsupported today, and those are
different claims — read `crates/crcbl-hal/src/capability.rs` for the current
wording rather than this file.

### The `crcbl-wgpu` deletion bar was already answered — what it costs, measured

**This was filed on 2026-08-20 as a decision needing the owner, and that was
wrong: it had been answered on 2026-08-19.** "DECIDED — the `crcbl-wgpu`
deletion bar, and quarry is next" below carries it, and the bar is
**`parity_blockers()` empty**, explicitly _"empty rather than explained"_, plus
a replacement for the cross-backend oracle. Re-opening a settled question is the
same defect this file keeps catching in other entries, so the correction is kept
here rather than quietly deleted. What follows is the measurement that entry did
not have.

**The second half of the bar is met.** `web/run-cross-backend-e2e.sh` holds the
browser against a native backend and runs in `pages.yml` — on Linux against vk,
and since 2026-08-20 on macOS against **Metal on the same device**, which is
stronger than the vk↔wgpu job it replaces: eleven scenes rather than three, and
a genuinely separate implementation rather than a second abstraction over one
driver.

**The first half is not, and every row needs hardware nobody here has** — dx12's
two mesh rows, Metal's two mesh rows and Metal's two counter-sampled query rows.
Read `REVIEWED_BLOCKERS`; this file does not restate the count, having had it
wrong twice.

**What the deletion is worth, counted rather than estimated.** Dropping
`crcbl`'s dependency removes **43 of the workspace's 254 resolved packages** —
17% of the graph — including the whole `wgpu` family (`wgpu`, `wgpu-core`,
`wgpu-hal`, `wgpu-types`, the three `wgpu-core-deps-*`), `glow`, `khronos-egl`,
`gl_generator`, `gpu-allocator`, `parking_lot`, `raw-window-handle` and
`raw-window-metal`. Plus a CI job, a runner script, a registry entry and
`CRCBL_GPU=wgpu`.

**DECIDED 2026-08-21 by the owner: `naga` stays**, because it is what validates
the shaders `crcbl-webgpu` ships. It is a **dev-dependency of `crcbl-shaders`**,
not a `crcbl-wgpu` transitive, so nothing about the deletion touches it — what
changes is only that it stops riding `wgpu`'s resolution and becomes a pin of
its own in the lockfile. `crates/crcbl-shaders/Cargo.toml` already argued
exactly that and called the cost worth paying; this settles it. Dropping it
would have removed the only check that shipped WGSL parses before a browser sees
it, which is how the uniformity bug shipped, and the standing "remove
`wgpu`/`naga` from the dependency graph" no longer applies to the second name.

**What is not in question.** `crcbl-wgpu` is not a conformance oracle and this
entry does not argue for keeping it as one: on Linux it runs Vulkan underneath,
so agreement with `crcbl-vk` proves less than it looks.

### The nanosecond refactor is validated on every backend CI can run

Worth recording because the change was landed with a "dx12 and Metal are
type-checked only" caveat, and CI settled it in one run: `mtl e2e`, `dx12 e2e`,
`wgpu e2e` and the cross-backend image compare all passed on the first attempt.

The strongest single piece of evidence is dx12's
`d3d12_timestamps_advance_and_both_read_paths_report_the_same_ticks`, which
**passed on WARP**. It holds `resolve_query_set`'s GPU-side tick copy against
`query::timestamp_nanos` of the `query_results` read — so it fails both a path
reaching a different heap, range or stride _and_ a `query_results` that forgot
to convert. That is exactly the asymmetry the refactor introduced, checked on a
real device rather than reasoned about.

So the caveat is retired: the only unexercised half is Metal's, and that is
because Metal still refuses timestamp query sets entirely, not because the
conversion is unproven there.

### The Metal ICB line of attack, and why it is closed

Kept because it cost four CI runs and the conclusion is worth not re-deriving.
`Capability::DrawIndirectCount` on Metal was attempted three times as an
**indirect command buffer** and hung the GPU every time — the same three
`render_e2e` goldens, with every encoder reporting `completed` and no API
violation in 8 MB of validation log:

| attempt | change                                                         | result           |
| ------- | -------------------------------------------------------------- | ---------------- |
| 1       | `executeCommandsInBuffer:withRange:`                           | Hang             |
| 2       | execution range read from GPU memory                           | Hang             |
| 3       | + blit `optimizeIndirectCommandBuffer` between kernel and pass | Hang, ~3x slower |

The isolated ICB probes pass on that exact device — a kernel encodes an ICB and
the ICB executes — so the mechanism works and something about a full frame does
not. **Nothing ever localised it**, and the `try/mtl-icb-indirect-range` branch
that held the reproduction is on neither this checkout nor the remote any more,
so reproducing it means writing it again from the table above.

**What shipped instead needs no ICB at all**, and that is the lesson: the
backend already issued plain indirect draws on a path that passed on this
runner, and a GPU-side count only needs the surplus draws to become no-ops.
`crcbl_mtl:: indirect_count` packs the arguments and zeroes the instance counts
past the count; the pass issues ordinary draws. It went green on the first CI
run, including the goldens that had hung three times.

Look for what the backend already does successfully before building new
machinery on top of it.

**The isolated probes are intermittent, which was not known when this closed.**
On 2026-08-20 both of them —
`a_compute_kernel_encodes_the_draw_an_indirect_command_buffer_executes` and
`an_indirect_command_buffer_executes_the_triangle_the_direct_draw_paints` —
failed in run 32297440428 at `assert_ink_triangle`: "the centre of the image is
not the triangle's colour, so nothing was drawn". The ICB executed and painted
nothing. An hour later run 32298272395 passed the same 68-test suite with both
of them green, on the same Metal code; the run that failed was a dx12 diagnostic
branch whose diff touches `crcbl-dx12` and one dx12 CI step and nothing else.

So "the isolated ICB probes pass on that exact device" is true on average rather
than every time, and the sentence above should be read that way. It makes the
decision already taken safer rather than shakier: a mechanism whose smallest
probe silently draws nothing on some runs is not one to have built the indirect
count on, and `crcbl_mtl::indirect_count` does not.

## DEFERRED — D3D12 registers are assigned by counting, and the mesh path collides

Decision record; the decision is in `docs/backlog.md`.

**The options, and the trade-off is real:**

- **(a) Make the two shaders declare the same binding set.**
  `mesh_cluster.slang` gains the rows it does not use and `mesh.slang` gains the
  mesh-path rows, so both files' declaration order matches one layout. Cheapest
  to reason about and needs no change to `crcbl-dx12`. The cost is dead
  declarations in both files that exist only to hold a register position, and
  nothing stops the next shader from drifting again — the invariant stays
  unenforced.
- **(b) Make registers follow the binding number instead of the count.** Emit
  `register(t17)` from `[[vk::binding(17)]]` so both sides agree by construction
  and a layout that omits a row cannot shift anything. This is what the
  collision argues for and it cannot rot. The cost is every shader gains
  explicit register annotations, `binding::ranges` and `root::assign_registers`
  change shape, and register numbers become sparse — root signatures grow
  descriptor ranges with gaps, which is legal but is a real change to how every
  D3D12 pipeline is built.
- **(c) Give the mesh path its own fragment shader in `mesh_cluster.slang`.**
  Removes the cross-file half of the problem, leaving only the layout-vs-source
  question that (a) or (b) still has to answer for a single file. Not a fix on
  its own.

## DEFERRED — dx12 mesh shading: WARP claims it and dies, hardware works

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

**Deferred 2026-08-21, mid-investigation and one step from the answer.** Work on
`crcbl-dx12` and `crcbl-mtl` is stopped by the owner's decision; see
`docs/plan/09-backends-metal-dx12.md`. Everything below stands and the state is
resumable, so this records the exact next step rather than leaving it to be
re-derived.

**Where it got to.** Seven probes narrowed the removal from "the renderer's mesh
path" to one shape: **a mesh pipeline with zero render targets**. The same mesh
stages draw with a colour target; the same depth-only pass draws over a vertex
pipeline; a depth attachment cleared and copied back with no pipeline at all
draws. Only the combination fails.

**One hypothesis was tested and is dead.** Presenting the pixel-shader subobject
empty rather than omitting it — matching the raster path — changed nothing: run
32421732642 removed the device exactly as before. That change stayed anyway, as
consistency, and its comment says it is not the fix.

**Two repros are in the tree**, both named in
`crates/crcbl-dx12/tests/known-red.txt`, which `run-dx12-e2e.sh` and `ci.yml`'s
adapter-report step both read:
`a_depth_only_mesh_pipeline_draws_the_toy_triangle_on_this_device` is the
minimal one, and `the_cluster_shaders_dag_descent_draws_the_cut_it_chose` drives
`mesh_cluster.slang`'s own containers. The second one's synthetic data and
expected values have never been checked against a device that survived, so it
should not be trusted as a signal until the first one passes.

**Option (d) has been tried, and it does not fix it.** Run 32297440428 on
`diagnose/dx12-warp-redist` gave the runner `Microsoft.Direct3D.WARP` 1.0.20 and
reported the mesh flags. The device was removed exactly as before.

The load is verified rather than assumed: both dx12 steps log
`driver="D3D12 UMD 1.0.20.0"`, against the OS WARP's
`D3D12 UMD 10.0.26100.33158` that every other Windows job still reports. So the
newest WARP Microsoft ships reproduces it.

**Two things that run narrowed it.** The crate's own suite — every D3D12 device
test, with `MESH_SHADER` and `TASK_SHADER` reported — **passed** on that WARP;
only `render_e2e`'s frame fails. And the failure is unchanged in kind:
`DXGI_ERROR_DEVICE_REMOVED` from `ID3D12Resource::Map`, zero debug-layer errors,
DRED still reporting `0 command list(s) with recorded work`. The three warnings
it does hold are `ClearDepthStencilView` perf notices about a missing optimised
clear value, which is not this.

**What that does to the options.** The entry's own bar was that a bug
reproducing on the newest WARP is far more likely ours than theirs, so (d)
answers (c)'s question in the direction of our defect and weakens (a) — (a)
would report a capability on the strength of hardware evidence for a path that
now fails on two WARP versions. It leaves (b) and (c) as the live pair.

Three measurements bracket the rest of this, and they assumed the runner's WARP
was the only WARP. That assumption is now tested.

Together the measurements turn this from "our mesh path is broken" into a
question about what to report.

1. **The renderer's mesh path is correct on real hardware.**
   `CRCBL_GPU=vk crates/crcbl/tests/run-render-e2e.sh` on an AMD RX 7900 XTX
   (RADV NAVI31) passes 26/26, and
   `the_cube_scene_draws_the_same_frame_on_every_geometry_path` reports
   `cube on MeshShader against IndirectCount — 0 channel(s) differ, worst by 0`.
   So the amplification stage descending the cluster DAG, its bind groups and
   its indirect dispatch extents all produce a byte-identical frame through the
   mesh path on a real GPU.
2. **WARP reports `MeshShaderTier = TIER_1` and then loses the device** on that
   same path, with **zero debug-layer errors** — so it is not an API misuse the
   validation layer can see — and **zero DRED breadcrumbs**, so nothing names
   the operation.
3. **`crcbl-dx12`'s own mesh probe passes on that same WARP runner**, drawing
   through a mesh pipeline and an amplification stage and reading the attachment
   back. So D3D12-on-WARP can run _a_ mesh pipeline; it is the renderer's larger
   use of one that kills it.

**What is still not proven** is whose defect it is. (1) is Vulkan, not D3D12, so
nothing has run `crcbl-render`'s mesh path through `crcbl-dx12` on a
**hardware** D3D12 GPU. WARP failing where hardware succeeds is the likeliest
reading, but a dx12-specific bug that only a software rasteriser exposes fits
the evidence just as well.

**A much narrower hypothesis, from reading the two paths rather than from new
evidence (2026-08-21).** "The renderer's larger use of one" was never narrow
enough to act on, and the difference turns out to be a single call:

- the probe that **passes** on WARP issues `encoder.draw_mesh_tasks(1, 1, 1)` —
  a **direct** `DispatchMesh` (`crcbl-dx12/src/device.rs`, in the mesh probe);
- the renderer that **kills** it issues `encoder.draw_mesh_tasks_indirect(..)` —
  an **`ExecuteIndirect`** with a `DISPATCH_MESH` command signature whose
  extents come from a GPU-written buffer (`crcbl-render/src/forward.rs`, the
  `EmitTail::Mesh` arm);
- and `draw_mesh_tasks_indirect` appears in `crcbl-dx12`'s own tests exactly
  once, in a _recording refusal_ check against an unissued handle. **Nothing has
  ever executed an indirect mesh dispatch on WARP.**

That fits the evidence that otherwise made no sense: zero debug-layer errors and
zero DRED breadcrumbs are what a fault _inside_ WARP's `ExecuteIndirect`
implementation would look like, before any command-list work is recorded.

**The experiment is in the tree**:
`an_indirect_mesh_dispatch_of_the_same_extents_draws_the_same_triangle` in
`crates/crcbl-dx12/src/device.rs`. Both mesh tests now go through one shared
`MeshProbe`, so the difference is structural rather than two copies staying in
step: same module, layout, bind group, attachment and one workgroup, and the
_only_ thing that varies is the dispatch closure — `draw_mesh_tasks(1, 1, 1)`
against `draw_mesh_tasks_indirect` reading three `u32`s from a device-local
buffer in `ResourceState::IndirectArgument`.

It asserts the same three texels the direct probe does, and the centre one is
what makes a pass mean something: a device that survived the dispatch and
executed nothing still leaves the clear there. There is deliberately no skip,
catch or tolerance that would let it pass on a device that removed itself.

`MeshProbe::frame` also calls `still_alive` between the submit and the readback,
so a removal surfaces as `GetDeviceRemovedReason` plus DRED breadcrumbs against
the frame's label rather than as `ID3D12Resource::Map failed` —
`DXGI_ERROR_DEVICE_REMOVED` is reported at the next call, and the `Map` was that
call.

**It passed. `ExecuteIndirect(DISPATCH_MESH)` works on WARP**, run 32405… —
`PASS [0.109s] (41/79)`, confirmed by name in the job log rather than inferred
from a green job. So the hypothesis is dead and one suspect is gone. That job
also prints what WARP claims: `MeshShaderTier = 10 (TIER_1)` and
`highest shader model by descending probe = 6.8`.

**What that leaves, and the next step is already in the tree.** The passing test
used `task: None` — a mesh stage with **no amplification**. The renderer uses an
amplification stage _and_ an indirect dispatch, and that combination had still
never run.
`an_indirect_dispatch_through_the_amplification_stage_draws_the_same_triangle`
drives it, so all four combinations of {direct, indirect} × {mesh only,
amplified} are now covered here and the last one is the smallest step from a
frame WARP survives to the frame it does not.

**It passed too**, `PASS [0.101s] (39/80)`, again confirmed by name in the job
log. So **the pipeline shape is eliminated entirely**: WARP survives every
combination of {direct, indirect} × {mesh only, amplified}.

**Two suspects down, and the search is now about size or content rather than
shape.** Each of those four dispatches **one** group; `crcbl-render`'s frame
dispatches one per (cluster, surviving instance).
`many_indirect_amplification_groups_do_not_remove_the_device` asks a single
`ExecuteIndirect` for `MANY_GROUPS` (1024) amplification groups — well inside
D3D12's bound of 65535 per axis and 2^22 in product, so it is a size a
conforming device must serve rather than a limit being probed.

**It passed as well** — `PASS [0.084s] (54/81)`, 1024 amplification groups
through one `ExecuteIndirect`, no removal. **So shape and scale are both
eliminated**, on three negative results and no hardware.

**DECIDED (2026-08-21) — (a), grow the probe shader toward the real one.** The
owner chose the bisect over shipping the capability and letting the renderer's
frame fail, so every step is a shader plus a test and each answer stays clean.
(b) is not refused, only deferred: it re-keys every D3D12 golden before any
diagnosis starts, and that cost does not shrink by waiting.

**One suspect on that list was wrong, and the probes already say so.**
`mesh_shader.slang`'s `taskMain` builds an `Amplification` payload and passes it
to `DispatchMesh`, and `amplifiedMeshMain` reads it back through `in payload` —
so an amplification payload rides through both passing amplified probes. It is
not a difference between the frames WARP survives and the one it does not, and
the list below no longer names it.

**Fourth negative result: a zero-group `DispatchMesh`.** `mesh_cluster.slang`'s
`taskMain` ends `DispatchMesh(keep, 1, 1, payload)` where `keep` is `0` on a
culled cluster, so the renderer asks D3D12 to dispatch **nothing** on every
frame that culls anything — and no probe had ever asked for that.
`a_zero_group_dispatch_mesh_does_not_remove_the_device` drives
`zero_dispatch_probe.slang`'s `culledTaskMain`, which dispatches one mesh group
for odd `SV_GroupID` and none for even, so both arms run inside one
`ExecuteIndirect`. **It passed** — `PASS [0.095s] (37/82)` on run 32410198419,
confirmed by name in the job log. So a zero dispatch count is not it either.

**Fifth negative result: storage writes from the amplification stage.**
`task_write_probe.slang`'s `writingTaskMain` does an atomic `count[0].add(1u)`
on odd groups and a plain `slots[group.x] = group.x + 1u` on even ones, the two
write kinds `mesh_cluster.slang`'s `taskMain` performs before it dispatches.
`storage_writes_from_the_amplification_stage_do_not_remove_the_device` asserts
the buffers rather than survival — the counter holds half the groups, and the
untouched odd slots still hold the priming sentinel, which is what shows the
branch branched. **It passed** — `PASS [0.122s] (56/83)` on run 32412768533.

**Two more suspects died on reading `mesh_cluster.slang` rather than on a
probe**, and both were this entry's own guesses:

- **There is no `groupshared` in it.** Not in the amplification stage, not in
  the mesh stage, nowhere in the file. It was never a difference.
- **It is not bindless.** No unbounded array is declared anywhere in it; the
  texture binding is a `Texture2DArray<float4>` with a `SamplerState` beside it.
  "The bindless descriptor heap the toy shader does not touch" described
  something the real shader does not touch either.

**What actually still differs**, measured:

- **23 `[[vk::binding]]` declarations against the probe's one or three**, and
  with them a much larger root signature, plus a **texture and a sampler** — the
  probes bind storage buffers alone.
- **The cluster-DAG descent**, which is the only real control flow in either
  stage, and a task-stage DXIL of 10,992 bytes against the probe's 2,648.
- **The mesh-specific part of the frame** — the draw-gen pass that writes the
  indirect args for this path.

**And one thing is exonerated by a run nobody set up for it:** WARP renders the
whole of `render_e2e`'s non-mesh path, with the same materials, textures,
culling compute pass and several draws per frame. So "the rest of the renderer's
frame" is only a suspect where it is _mesh-specific_.

**Driving `mesh_cluster.slang` itself reproduced the removal.** The probe ran
its own containers, 22 bindings, a texture, a sampler and a real DAG descent
through an `ExecuteIndirect`, and WARP removed the device with the renderer's
exact signature — so **there is now a repro with no renderer in it**, on
run 32416192662. It was reverted from `main` rather than left red (`cd1f653`);
the patch is recoverable from `76939d0`.

**ANSWERED, and it is not either shader.** `mesh_cluster.slang` has no fragment
stage, so that probe took `depth_pipeline`'s shape — `fragment: None`, no colour
targets, a `D32Float` depth attachment as the observable — and `crcbl-dx12` had
never built a depth-only mesh pipeline or copied a depth image back. The
discriminator drove that same shape with `mesh_shader.slang`'s **toy** stages,
the ones six colour-target probes pass on, through the same indirect amplified
dispatch.

**The toy died too**, on run 32418483641, with the identical signature:
`ID3D12Resource::Map failed … DXGI_ERROR_DEVICE_REMOVED`, DRED reporting
`0 command list(s) with recorded work`.

So the defect is **the depth-only mesh pipeline**, not the cluster shader, not
the DAG descent, not the 22 bindings, and not the register skew. The same stages
draw correctly the moment there is a colour target and a fragment stage. Both
probes are kept and both are named in `crates/crcbl-dx12/tests/known-red.txt`,
which `run-dx12-e2e.sh` and `ci.yml`'s adapter-report step both read and which
announces them on every run.

**That also explains the renderer**, which is the part worth noticing: the mesh
frame does not fail somewhere exotic. `ForwardRenderer` renders its shadow
cascades through `depth_pipeline` — depth-only, no colour target — so on the
mesh path it builds exactly the pipeline these two probes build, every frame.

**What is still not separated**, and it is three cheap probes rather than one:

- **the `D32Float` readback itself** — clear a depth image and copy it back with
  no pipeline at all. If that removes the device, none of this is about mesh
  shading and `plan_copy`/`copy_footprint_format` is where to look;
- **`fragment: None` on a mesh pipeline** — the same mesh stages with a fragment
  stage and a colour target _plus_ a depth attachment. If that draws, the empty
  fragment stage is the trigger;
- **depth-only on a raster pipeline** — the same depth-only shape with an
  ordinary vertex pipeline. If that draws, it is the combination of mesh and
  depth-only rather than either alone.

**What is no longer in question**: WARP's `ExecuteIndirect` of `DISPATCH_MESH`,
its amplification stage, both together at a thousand groups, a dispatch count of
zero, and an atomic plus a plain store from that stage all work. Anything that
begins "WARP cannot do mesh shading" is contradicted by the probes in
`crates/crcbl-dx12/src/device.rs`.

**Worth stating because it is the shape of the whole exercise:** every one of
these is a _negative_ result, and each is still progress. The entry started at
"the renderer's larger use of one kills it", which named no call; it is now
"every pipeline shape survives at one group, and here is the next variable".
None of it needed hardware nobody has — only a test that runs where the failure
lives.

So the decision:

- **(a) Withhold `MESH_SHADER` on software adapters and report it on hardware.**
  Exactly the shape `crcbl_mtl::quirk` already uses — that module's rule is that
  a quirk needs a measurement contradicting an unconditional API guarantee, and
  says what was measured, on which device, and what every other device does. It
  closes two blockers (nine rows to seven). The price: CI would report a
  capability **no CI job can ever exercise**, since every dx12 job is WARP. That
  is a real loss — it is the failure mode the parity mechanism exists to prevent
  — and it should be a deliberate choice, not a side effect.
- **(b) Keep both rows withheld** until a hardware Windows GPU can run them.
  Honest, costs two blockers that may never close on the hardware this project
  has, and matches how Metal's four unprovable rows are already treated.
- **(d) Give the runner a newer WARP — tried 2026-08-20, and it did not fix
  it.** Kept because the reasoning is what makes the result mean something.
  Microsoft ships WARP as a NuGet redistributable,
  [`Microsoft.Direct3D.WARP`](https://www.nuget.org/packages/Microsoft.Direct3D.WARP)
  (1.0.20, 28 May 2026), precisely so a developer can "try out changes or
  improvements to WARP without having to fully update your Windows operating
  system". **Two of its release notes land on this symptom:**
  - **1.0.12 — "Fix mesh shaders on Win11 retail D3D12."** The retail OS D3D12
    stack had a mesh-shader defect that the redistributable fixes. Our dx12 jobs
    run the OS WARP.
  - **1.0.20 — "Don't remove the device on free-threaded DDI failures."** A
    device-removal fix, and device removal with no debug-layer error and no DRED
    breadcrumb is exactly measurement (2) above.

  Earlier notes also mention fixes for root-signature visibility for
  amplification and mesh shaders, and for uninitialized memory affecting
  amplification shaders. Measurement (3) — the small mesh probe passes while the
  renderer's larger mesh path kills the device — is the shape a root-signature
  or uninitialized-memory bug would take.

  **The mechanism is cheap**: per Microsoft's WARP guide, "simply place
  `D3d10warp.dll` next to your application .exe file". No Agility SDK, no code
  change, no `EnumWarpAdapter` change. The practical wrinkle is that cargo runs
  test binaries out of `target/<profile>/deps/`, so the copy has to land beside
  whichever executable the job runs, not beside the crate.

  **The constraint, stated because it is a real one:** the same guide says these
  DLLs "cannot be redistributed, as there is no guarantee that future versions
  of Windows will maintain compatibility with them". Fetching one in CI for
  testing is the documented use; shipping it to users is not, and nothing here
  would.

  **Why this dominates (a) if it works.** Option (a)'s price is reporting a
  capability _no CI job can ever exercise_. If a newer WARP survives the mesh
  path, both rows close **and** CI exercises them — and `MeshShading` and
  `TaskShaderStage` become drivable in `tests/hal_seam_e2e.rs`, taking its
  coverage from 21 of 24 to 23. If it does **not** survive, that is (c)'s answer
  arriving for the price of one CI run: a bug reproducing on the newest WARP
  Microsoft ships is far more likely to be ours than theirs.

  **Tried, and the harness is kept.** Branch `diagnose/dx12-warp-redist` carries
  both halves — the unconditional `MESH_SHADER | TASK_SHADER` in `features_of`
  with the three geometry-path assertions neutralised, and the CI step that
  fetches the redistributable and copies `d3d10warp.dll` beside the test
  binaries in `target/debug` and `target/debug/deps`. Run it with
  `gh workflow run ci.yml --ref diagnose/dx12-warp-redist`; it is expected red
  and the job log is the deliverable. Never merge it. It is rebuilt on main
  rather than on the older `diagnose/dx12-mesh-device-removal`, which is far
  enough behind that a result off it would say nothing.

- **(c) Narrow it first.** Build a smaller repro on WARP — the amplification
  dispatch alone, then with the DAG descent, then with the real bind groups —
  until one of them removes the device. That would say whose bug it is, which is
  what actually decides between (a) and (b). Costs a Windows debugging session
  measured in CI round trips rather than minutes, since nothing local can run
  it.

My reading is (c) then (a) or (b) on what it finds, because (a) taken now is
reporting a capability on the strength of a _different backend's_ evidence.

**The harness for (c) already exists**: branch
`diagnose/dx12-mesh-device-removal`, pushed and kept deliberately. It reports
`MESH_SHADER` unconditionally from `features_of` and neutralises the two
assertions that would otherwise fail the dx12 job before it reaches its "Draw a
frame through ForwardRenderer on WARP" step. Run it with
`gh workflow run ci.yml --ref diagnose/dx12-mesh-device-removal`; it is expected
red, and the job log is the deliverable. Never merge it.

The two hypotheses eliminated by reading are unchanged and still worth not
re-testing: the indirect argument size is right (`IndirectKind::DispatchMesh`
reports the 12 bytes `D3D12_DISPATCH_MESH_ARGUMENTS` wants), and the asymmetric
resource states on the mesh path are deliberate, not a bug.

### Reporting dx12 mesh shading removes the WARP device — measured, then reverted

**This was attempted and reverted, and the reason is a real defect rather than a
CI accident.** Reporting `Features::MESH_SHADER` from
`D3D12_FEATURE_DATA_D3D12_OPTIONS7::MeshShaderTier` routes `crcbl-render` onto
`GeometryPath::MeshShader` for every D3D12 adapter. On WARP the frame then never
completes:

```
the cube frame renders on MeshShader: HAL: ID3D12Resource::Map failed:
The GPU device instance has been suspended. Use GetDeviceRemovedReason to
determine the appropriate action. (0x887A0005)
```

`0x887A0005` is `DXGI_ERROR_DEVICE_REMOVED`. Four `render_e2e` tests fail that
way — `the_cube_scene_draws_the_same_frame_on_every_geometry_path`, the same for
`ao`, and both scenes' golden tests — and every one of them fails inside
`draw_and_readback`, so **the frame never renders**; no pixel is ever compared.

**The narrowing that matters:** `crcbl-dx12`'s own
`a_mesh_pipeline_draws_through_d3d12_and_its_amplification_stage_is_visible`
**passes on the same WARP runner**, drawing through a mesh pipeline and an
amplification stage and reading the attachment back. So the backend's mesh
pipeline is not broken in general. What removes the device is `crcbl-render`'s
mesh path specifically — the amplification stage descending the cluster DAG, its
bind groups, and its dispatch sizes — none of which the probe exercises.

**Both of the hypotheses this entry used to list are now eliminated by reading,
and so is a third**, which matters because each would otherwise cost a Windows
session:

- **A `DispatchMesh` group count past TIER_1's ceiling.** The extents are
  `draw_gen.slang`'s: `MESH_ARG_GROUP_X` is `bucket_clusters(index)`,
  `MESH_ARG_GROUP_Y` is the surviving-instance count, `MESH_ARG_GROUP_Z` is `1`.
  D3D12 allows 65535 per dimension and 2^22 as a product; the ao and cube
  fixtures have single-digit clusters and instances. Not close.
- **A payload larger than the amplification-stage limit.** `ClusterPayload` is
  **two `uint`s — eight bytes** — against D3D12's 16 KB. The struct exists
  precisely so the mesh stage re-reads from buffers rather than copying a
  record, which is why it is this small.
- **A stale or uninitialised Y extent.** Plausible, since `MESH_ARG_GROUP_Y` is
  accumulated by atomic add and would be last frame's value if nothing reset it.
  `clear_counters.slang` does zero it every frame, and says so where it does.

So the remaining space is narrower and nastier than "some limit is exceeded":
whatever removes the device is not a count, a size, or an uninitialised word.
`crcbl-dx12`'s own mesh probe passing on the same WARP runner already said the
mesh _pipeline_ is fine, so what is left is the interaction — the amplification
stage descending the cluster DAG, its bind groups, and the indirect dispatch —
none of which the probe exercises. Confirming any of that needs a Windows
machine, which nothing here has.

**Two hypotheses have been eliminated by reading, so nobody spends a Windows
session on them:**

- **The indirect argument size is right.** `IndirectKind::DispatchMesh` reports
  12 bytes, which is `D3D12_DISPATCH_MESH_ARGUMENTS`' three `u32`s exactly. A
  wrong stride here would have produced garbage thread-group counts, which is
  the most obvious route to a removal.
- **The resource states are right, and deliberately asymmetric.** On the mesh
  path `crcbl-render`'s forward pass declares `draws.args_id` as a **shader
  read** and `draws.counts_id` as `ResourceState::IndirectArgument`, while the
  lesser path declares both as `IndirectArgument`. That looks like a bug and is
  not: the mesh path reads the draw arguments as shader data and executes its
  thread-group extents out of the _counts_ buffer, which is what
  `draw_mesh_tasks_indirect` is handed (`args: draws.counts`). The two live in
  different buffers precisely because a resource holds one state per pass, and
  the code says so where it splits them.

Also checked: the command signature passes no root signature, which is correct
for a `DISPATCH_MESH`-only layout — that argument kind writes no root argument,
so D3D12 requires null there.

**The next attempt should name the operation rather than the `HRESULT`.**
`crcbl_dx12::dred` now forces DRED auto-breadcrumbs on before the first device
is created and prints them beside `GetDeviceRemovedReason`, so a re-run of the
reverted commit on the WARP runner should say which command list stopped and on
which operation — `DISPATCHMESH` versus the `ExecuteIndirect` after it versus a
barrier is most of the narrowing above, answered from a CI log. That has **not**
been observed: see the DRED entry below for what is unverified about it.

**Why it is a revert and not a workaround.** Gating the report on the adapter
name would hide a real defect behind CI's specific device, and the flag is
either honest or it is not. `crcbl-vk` proves the paths _can_ agree: on an RX
7900 XTX the same test draws `MeshShader` against `IndirectCount` with **0
channels differing, budget 0**. dx12 must reach the same bar.

**What the attempt is worth keeping for.** The implementation, the tier+shader
model gate (`TIER_1` and SM 6.6 together, because the committed DXIL is built at
`6_6`), the `FeatureQuery` move out of `mod tests`, the `instance.rs` derivation
assertion, and the seam exercise are all written and reviewed; the revert is
`6fe2d41` and they can be recovered from it rather than rewritten. The blocking
question is only the device removal.

### DRED has now run, and WARP records nothing

Settled by a throwaway branch that re-reported `Features::MESH_SHADER` and let
one CI run reach the removal. Three of this entry's four open questions are
answered:

- **`D3D12GetDebugInterface` does answer for
  `ID3D12DeviceRemovedExtendedDataSettings` on a stock `windows-latest` runner
  with no Graphics Tools feature.** `crcbl_dx12::dred` logged "DRED
  auto-breadcrumbs and page-fault reporting are on". The module docs' argument
  for enabling it unconditionally rather than behind `CRCBL_DX12_VALIDATION`
  holds.
- **Breadcrumbs are NOT populated on WARP.** The report reads
  `DRED auto-breadcrumbs: 0 command list(s) with recorded work` on a genuinely
  removed device. This was listed here as the thing worth finding out, and the
  answer closes the avenue: DRED cannot name the failing operation on a software
  adapter. It costs nothing and stays enabled — on a hardware Windows GPU it is
  still the right tool.
- **The walk survives**: it ran against a real removed device and returned a
  report rather than faulting, though with an empty history it dereferenced
  little.

Still unknown: the `IN FLIGHT` marker's off-by-one, which needs a driver that
actually writes breadcrumbs.

**What the run also fixed.** The diagnosis never reached the caller: a readback
`Map` failure raised a bare `HalError::Backend`, and `debug::diagnosis` was
attached only to `Signal`, the fence waits and the submit paths. A `Map` is
where a removal surfaces, since it is the first call touching memory the GPU was
writing. All three `Map` sites now carry it. That is why the first two
diagnostic runs printed nothing but `0x887A0005`.

### Nothing in `crcbl-render` uses push constants

`exercise_push_constants_on_graphics` in `crates/crcbl/tests/hal_seam_e2e.rs`
closed the coverage gap this entry used to record: it draws twice in one render
pass with different `push_constant_raster.slang` blocks and asserts each draw
saw its own, with the vertex stage taking its rectangle from the block and the
fragment stage its colour. Verified on vk against real hardware (RADV Navi31).

**The Metal and dx12 arms have since run green.** They were type-checked only
when this was written — `--target aarch64-apple-darwin` and
`--target x86_64-pc-windows-msvc` — and CI has run them since: the seam reports
`PushConstants supported` on both, and push constants are absent from each run's
unexercised list, which is the print that would have said otherwise. Three
pieces of arithmetic ran there for the first time, and none was wrong:

- `crcbl_mtl::argument::plan` computing a block index for a layout with **no
  bind groups at all**. The index is zero, and `msl/push_constant_raster.metal`
  puts the block at `[[buffer(0)]]` in both entry points, but no push constant
  has occupied index 0 before — `push_constant_probe`'s sits behind one binding.
- `crcbl-mtl`'s **render** arm of `push_constants`, which records
  `RenderCommand::PushConstants` with a `Vec<u8>` copy and replays it as
  `setVertexBytes:`/`setFragmentBytes:`. That copy is exactly what the second
  draw's assertion is about, and nothing had ever made two draws either side of
  a `push_constants` on Metal.
- `crcbl_dx12::conv::shader_visibility` resolving `VERTEX | FRAGMENT` to
  `D3D12_SHADER_VISIBILITY_ALL`, and `push_constants` taking its
  `SetGraphicsRoot32BitConstants` branch rather than the compute one.

**`crcbl-render` still passes `push_constants: None`** at every render-pass site
it has, so the renderer never exercises the path in anger and the seam suite is
the only thing that does. Not a defect — recorded so the closed coverage gap is
not read as the engine having started using push constants.

### An unwritten timestamp query does not read back as zero

Measured while closing the render-only gap above (now closed — the exercise
times a compute pass beside the render one). With the compute pass's
`timestamp_writes` removed so queries 2 and 3 are never written, `query_results`
over the whole set came back **all four zero on vk — including the render pair
that was written**. Vulkan zeroes the entire read when any query in the range is
unavailable rather than reporting per query.

Two consequences worth keeping:

- **Zero is not a per-query sentinel.** A test priming its destination and
  checking "did this query get written" cannot rely on the others surviving. The
  timestamp exercise primes with `QUERY_POISON` _and_ checks zero, because which
  one an unwritten query keeps is the backend's business.
- **The whole-range zero is why the render branch fires first there.** The
  compute pair's own assertion is unreachable on vk for that particular break —
  it exists for the asymmetric case where a backend writes one pass kind and
  drops the other, which is exactly what `crcbl-webgpu`'s separate
  `begin_compute_pass` encoding makes possible and nothing else would catch.

### DECIDED — the `crcbl-wgpu` deletion bar, and quarry is next

Answered by the owner on 2026-08-19.

**quarry (S4C) is the next demo.** Started.

**`crcbl-wgpu` goes when every feature is implemented on the other backends and
the golden comparisons no longer need it.** One wrinkle to make that checkable:
"every feature on every backend" can never be literally true while rows are
`ApiAbsence` — `BufferFillWord` has no encoding on Metal, whose
`fillBuffer:range:value:` takes a byte, nor on WebGPU. The workable reading is
**`parity_blockers()` empty** — which excludes `ApiAbsence` by construction and
is already the query the snapshot test guards — **plus** the separate condition
that `cross-backend-e2e` no longer needs wgpu as its comparison oracle.

**That second half is measured now.** `web/run-cross-backend-e2e.sh` compares
the browser against vk (`--reference vk`, two scenes excused by name with
`--expect-fail ssr,ui`) and runs in `pages.yml`, so a replacement oracle exists
and is exercised. The old vk↔wgpu comparison still runs from `ci.yml`; retiring
it is the mechanical part. What is left of the bar is the first half —
`parity_blockers()` is not empty, and its rows are dx12's two mesh rows and
Metal's four, none of which "measured unprovable" empties, because the decided
bar asks for empty rather than for explained.

### `DivergenceKind::Declined` is gone too, and can come back

The two dx12 valued fills were the only `Declined` rows in the whole table, so
removing them emptied the kind and `every_kind_describes_at_least_one_real_row`
failed — the same rule that retired `Unclassified` earlier the same day. Its
reasoning is worth keeping: **expressible, and deliberately not done**, where
the reason says what was chosen instead and why that was enough. It blocked
parity all the same, because the decline was _ours_ and a caller who needs the
thing can overturn it, which is exactly what an `ApiAbsence` row can never be.

`DivergenceKind` is now `ApiAbsence`, `Unwritten` and `Unrun`. Reintroducing
`Declined` is a variant, an arm in `blocks_parity`, and an entry in that test's
list.

**Note this is a consequence, not a decision that was asked for.** Dropping the
valued fills was; emptying a kind was what it did, and the alternative was
weakening the test that noticed.

### Three offscreen fixtures hand-roll what `GpuContext::open_offscreen` now does

`GpuContext::open_offscreen` landed with `apps/quarry`'s frame test as its
caller, and it is not the first place in the tree to open an instance, an
offscreen surface, a device and a two-image ring in that order.
`crates/crcbl/tests/gpu_scene/harness.rs`'s `Headless::open_at_format` and
`crates/crcbl-vk/tests/vk_e2e/harness.rs` both do it by hand, and each carries
its own copy of the adapter selection, the format choice and the teardown order.

**Not converged in the same slice, deliberately.** Both fixtures do more than
open a ring — a `DeviceSlot` a `finish` can empty, a validation report asserted
clean, a pinned format checked against the caps with a divergence message — and
none of that belongs on the engine's public context. Converging them means
deciding which of those are fixture concerns and which are missing engine API,
which is its own piece of work rather than a rename.

What would make it worth doing: a fourth copy. Two fixtures with different
requirements are a resemblance; a third caller needing the same extras is
duplicated knowledge.

### MEASURED — what deleting `crcbl-wgpu` does to the parity mechanism

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

Re-derived from the tree on 2026-08-19, so the deletion is a reviewed change
rather than a grep. Two halves: what actually references the crate, and what its
removal does to `parity_blockers`.

**One question the deletion foreclosed, and it was not on the bar.** See
"DECIDED — the browser has no WebGL2 fallback" in `docs/notes/browser.md`. Every
other item gating this deletion could be revisited afterwards; that one could
not, because deleting the crate deleted the only WebGL2 path there was to build.

**The code surface is five files and two manifests.** Everything else that names
`wgpu` names it in prose. Counting only lines that are not comments:

| file                                 | references                                                  |
| ------------------------------------ | ----------------------------------------------------------- |
| `crates/crcbl-hal/src/capability.rs` | 18 `BackendKind::Wgpu`, 1 `crcbl_wgpu`                      |
| `crates/crcbl/src/backend.rs`        | 9 `GpuBackend::Wgpu`, 4 `BackendKind::Wgpu`, 1 `crcbl_wgpu` |
| `crates/crcbl-hal/src/caps.rs`       | 2 `BackendKind::Wgpu`                                       |
| `crates/crcbl/src/args.rs`           | 1 `GpuBackend::Wgpu`                                        |
| `crates/crcbl-hal/src/error.rs`      | 1 `BackendKind::Wgpu`                                       |

The manifests are the workspace root's `[workspace.dependencies]` entry and
`crates/crcbl/Cargo.toml`'s. **`crcbl-mtl` and `crcbl-shaders` name it only in
comments**, which confirms the earlier reading — what they owe is a reworded
justification, not a dependency fix.

**And the mechanism half, which is the part a grep does not show.** Divergence
rows per backend, counted from `DIVERGENCES`:

| backend | parity target | rows | blocking |
| ------- | ------------- | ---- | -------- |
| vulkan  | yes           | 0    | 0        |
| webgpu  | yes           | 13   | **0**    |
| metal   | yes           | 4    | 4        |
| dx12    | yes           | 2    | 2        |
| wgpu    | **no**        | 12   | **7**    |

**The bookkeeping this entry opened is closed, and only the coverage half is
still live** (re-checked 2026-09-02). The `crcbl-wgpu` crate is gone,
`BackendKind::is_parity_target` with it, and `parity_blockers()` in
`crates/crcbl-hal/src/capability.rs` now filters on
`entry.backend.is_gpu() && entry.kind.blocks_parity()`. The prediction this
entry made held: the blocker count is still six — two `Dx12` and four `Metal` —
because wgpu's rows were deleted rather than closed, and WebGPU has none left.

### MEASURED — vk↔WebGPU replaces vk↔wgpu as the cross-backend oracle

**This settles the second half of the `crcbl-wgpu` deletion bar** — "we don't
need wgpu for the golden comparisons any more". Measured on 2026-08-19, on one
Linux machine, with pieces that all already exist:

1. `crcbl screenshot --scene <name> --size 256x192` through `CRCBL_GPU=vk`
   renders each of the eleven scenes `crcbl_render_harness::golden_names` lists
   — the CLI's `--scene` names and that list are the same eleven strings.
2. `web/tools/render-harness-e2e.mjs` drives the same eleven through
   `crcbl-webgpu` in headless Chromium on SwiftShader and writes each readback.
3. `cargo run -p render-harness --example compare-readback -- <readbacks> --golden-dir <the vk PNGs>`
   compares them directly. `--golden-dir` is an existing flag; nothing new was
   written to take this measurement.

**The result: 9 of 11 match, and the two that do not are `ssr` and `ui` — the
same two the browser gate already carries as `--expect-fail`.** So the direct
cross-backend comparison agrees, scene for scene, with what is already gated.

**It is a sharper instrument than the golden comparison, not a weaker one**,
which is the property the old vk↔wgpu job was kept for:

| scene    | vs the committed golden | vs vk directly    |
| -------- | ----------------------- | ----------------- |
| `ssr`    | 25,611 pixels differ    | **1,355**         |
| `ui`     | 3,872 pixels differ     | **506**           |
| `sprite` | matches                 | **0** — identical |

`sprite` being byte-identical between radv and Chromium-on-SwiftShader is worth
recording on its own: two unrelated rasterisers, one number.

**The gate is built**: `web/run-cross-backend-e2e.sh`, wired into `pages.yml`'s
`render-harness` job after the golden comparison, reusing that step's readbacks
so the wasm build and the browser run happen once. It renders each scene through
`CRCBL_GPU=vk` on lavapipe and compares. Verified locally against **both**
reference drivers — radv and lavapipe — with the same 9/11 verdict either way,
which is what says the gate measures the browser backend rather than the
reference's rasteriser.

**What it does not do.** It compares eleven scenes at one size, where the job it
replaces compared three at two. That job is gone — it went with `crcbl-wgpu` on
2026-08-21, and this is what stands in its place rather than beside it.

The remaining half of the deletion bar is the parity blockers, all of them rows
of hardware nobody here has. The count is deliberately not written here — it has
been wrong in this file twice. `REVIEWED_BLOCKERS` in
`crates/crcbl-hal/src/capability.rs` is the list, and
`the_parity_blockers_are_exactly_the_reviewed_list` fails when it drifts.

### What the withheld-features pass still reports, and on which backends

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

**`crcbl-dx12` and `crcbl-mtl` had vk's exact inconsistency and it is fixed.**
Both declared `Capability::TimelineWaitBeforeSignal => Support::Yes`
unconditionally while the arm beside it gated the timeline group on
`Features::TIMELINE_SEMAPHORE` — and `ID3D12Fence` and `MTLSharedEvent` are both
core, so `create_semaphore` built a timeline regardless. Found by reading, since
nothing here runs either backend. Each got the two changes `crcbl-vk` got, and
each job got the narrow step, so the fix is exercised rather than asserted.

**Both were landed type-checked only**, against `x86_64-pc-windows-msvc` and
`aarch64-apple-darwin` — the project's own practice for platform code, and the
same footing the nanosecond refactor was landed on. CI is what settles them.

**`crcbl-webgpu` was examined by reading and is clean on this point.** The
native suite cannot open it — `crcbl::backend::open` answers "the crcbl-webgpu
backend is not active in this build — it reaches a device only on wasm32",
measured rather than assumed — but the defect the narrow pass finds is visible
in `supports` without running anything, and this backend does not have it:
`TimelineWaitBeforeSignal` is grouped _with_ the other three timeline rows as
`Support::No(NO_TIMELINE)` rather than answering `Yes` beside them.

**And part of its refusal direction now runs, in an ordinary unit test.**
`a_capability_declared_unsupported_is_refused_by_its_own_call` in
`crcbl-webgpu/src/hal/tests.rs` needs no browser: `WebGpuDevice` records
commands to a stream rather than executing them, so a refusal is a decision this
crate makes in Rust. It covers the rows whose refusal is a single device call —
the four timeline rows through `create_semaphore`, `PipelineStatisticsQuery`
through `create_query_set` — asserts each is `HalError::Unsupported`
specifically, and checks the accepting side too so it cannot pass by refusing
everything.

**Three more are covered by a sibling test**, and the sentence that used to sit
here — "each of which needs a pipeline or a layout built first" — was a blanket
claim made about seven rows without checking them one at a time. It was wrong
for three. `CommandEncoder`'s verbs return nothing, so `draw_indirect_count`,
`draw_indexed_indirect_count` and `draw_mesh_tasks` record the refusal and
surface it at `finish`; `record_unsupported` sets a field, reads no pass state
and needs no pipeline.
`an_encoder_verb_declared_unsupported_is_refused_at_finish` drives all three on
a bare encoder, one verb per encoder so the first recorded refusal cannot mask
the others, and finishes a clean encoder too so it cannot pass on a backend that
refused every command buffer.

**Nine of this backend's thirteen unsupported rows are checked natively**, and
the four that are not cannot be — which is a structural fact rather than a cost.
The thirteen is `DIVERGENCES`' own row count for `BackendKind::WebGpu`, not a
hand tally; an earlier version of this entry said "twelve", which came from
counting something else entirely.

**The line falls exactly where the refusal is decided.** Nine are refused by
this crate in Rust — a device method returning `Err`, or an encoder verb
recording one for `finish` — so an ordinary unit test sees them. The other four
are refused by `gpu-replay.js` in the browser, by the stream's own design rule:
the writer "carries what the caller gives" and validates nothing, because "the
replayer is the half that faces WebGPU". `push_constants` encodes and crosses;
`create_graphics_pipeline` lets a `PolygonMode::Line`, a `depth_clamp` the
device cannot serve and a forbidden sample count all cross verbatim.

So `PushConstants`, `BindlessDescriptorArray`, `PolygonModeLine` and
`IndirectArgumentPaddedStride` are not "uncovered because they need a pipeline
built first" — the Rust side deliberately does not decide them, so **no native
test can ever cover them** and a browser probe group is the only possible route,
not merely the convenient one. Three earlier versions of this paragraph gave a
setup-cost reason instead, and each was wrong about which rows it applied to.

So of the backends the pattern was checked on: three had it (`crcbl-vk`,
`crcbl-dx12`, `crcbl-mtl`, all fixed) and `crcbl-webgpu` did not. `crcbl-wgpu`
had a different defect in the same family and was deleted before it was fixed —
see "A capability refusal must be `Unsupported`" below.

### SHIPPED — occlusion queries are refused at the seam

Record of the decision and of what landed. Option (2) was taken on 2026-09-06
and built the same day.

**What is in the tree now.** `crcbl_hal::QueryKind::check_supported` refuses
`QueryKind::Occlusion` with `HalError::Unsupported`, and every backend's
`create_query_set` — `crcbl-vk`, `crcbl-dx12`, `crcbl-mtl`, `crcbl-webgpu` and
`crcbl-hal`'s `Null` — calls it before it builds anything.
`Capability::OcclusionQuery` answers `Support::No` on all of them, carrying
`crcbl_hal::NO_OCCLUSION_QUERY_VERB`, which is the one sentence the refusal, the
declaration and the `DIVERGENCES` rows all read from. The four GPU backends each
gained an `Unwritten` divergence row, which is four parity blockers the report
honestly grew; `DEFERRED_CAPABILITIES` in `crcbl-hal/src/capability.rs` is what
records that those four are parked by this decision rather than by either
backend deferral, and it is what
`a_divergence_is_something_some_backend_actually_has` now consults before
allowing a capability to be absent everywhere.

The backends' own occlusion plumbing was **kept**, unreachable behind the
refusal: `crcbl-mtl`'s `new_visibility_buffer` and `QuerySetRaw::Visibility`,
`crcbl-dx12`'s `D3D12_QUERY_HEAP_TYPE_OCCLUSION` mapping, `crcbl-vk`'s
`VK_QUERY_TYPE_OCCLUSION` arm. Each is what option (1) would reach, and each is
still constructed syntactically, so nothing is dead code.

The argument that produced the decision follows.

The seam audit recorded in one line that `Capability::OcclusionQuery` "is
unfalsifiable everywhere". Re-derived and **measured** on 2026-08-20, it is
worse than unfalsifiable.

**What the seam has.** `Device::create_query_set`, `destroy_query_set`,
`query_results`, and `CommandEncoder::reset_query_set`, `resolve_query_set`.
**There is no begin/end-query verb anywhere on `CommandEncoder`** — timestamps
are written through `PassTimestampWrites` on the pass descriptor, and nothing
equivalent exists for occlusion. Read off the trait, not inferred.

**So the result a caller gets is not an occlusion count.** Measured through the
seam suite on radv: create a `QueryKind::Occlusion` set, reset it, resolve it,
read it — `query_results` returns **`[0, 0]`**. Zero is not a neutral answer for
this query; it is "nothing was visible". A caller who wired occlusion culling to
this seam would cull the entire scene and every return value would say success.

**Which backends this reaches, measured rather than assumed.** `crcbl-vk`
answers `Support::Yes` (gated, and radv has the feature) and is where the
`[0, 0]` came from. `crcbl-webgpu` answers `Support::Yes` unconditionally, and
`crcbl-dx12` and `crcbl-mtl` gate on a device feature, so on any device that has
it the same read is available. The `Null` backend records and answers `Yes`.

`Capability::OcclusionQuery` is honest about this — its doc is "a
`QueryKind::Occlusion` query set", nothing more, unlike `TimestampQuery` whose
doc names the writes and the read. The capability is not lying. The **seam** is
offering a resource whose only purpose it cannot serve.

**And this was already known in-code**, which is worth saying: the comment on
`crcbl-webgpu`'s arm in `hal/device.rs` states it outright — "`CommandEncoder`
has no begin/end query verb, so nothing a caller records through this seam can
ever write one, and the same is true of the Vulkan backend's `Yes`". What is new
here is the measured value a caller receives instead, and that it is the one
value that silently means the opposite of the truth.

Nothing plans to use it: no document under `docs/plan/` schedules GPU occlusion
queries or occlusion culling (the roadmap's "occlusion" is audio, and topic 31's
vis-culling rays are CPU/server side), and no caller outside the backends' own
plumbing and tests constructs one.

**The options.**

1. **Finish it.** A pass-descriptor field plus a per-draw index verb, on
   `PassTimestampWrites`' precedent — and it is genuinely backend-agnostic:
   WebGPU has `occlusionQuerySet` on the render-pass descriptor and
   `beginOcclusionQuery(index)` on the pass encoder, Metal has
   `visibilityResultBuffer` plus `setVisibilityResultMode:offset:`, Vulkan has
   `vkCmdBeginQuery`/`vkCmdEndQuery` inside a pass, D3D12
   `BeginQuery`/`EndQuery` with `ResolveQueryData`. Real work across five
   backends, two of which nothing here can run, for a feature nothing has asked
   for.
2. **Refuse it until then.** `create_query_set` returns `HalError::Unsupported`
   for `QueryKind::Occlusion`, and `Capability::OcclusionQuery` becomes an
   `Unwritten` divergence on every backend. Small, and it turns a silent wrong
   answer into a loud refusal, which is what this repository does everywhere
   else. Costs a parity blocker per backend on the report — honestly, since the
   work genuinely is unwritten. **This is the one that was taken.**
3. **Delete it.** `QueryKind::Occlusion` and `Capability::OcclusionQuery` leave
   the seam, exactly as the valued `fill_buffer` forms did: a promise three
   backends could not keep was removed rather than implemented, and the
   changelog records that as the right call. Cheapest, and the one that loses
   information if occlusion culling is ever wanted.

**My reading:** (2) now and (1) if occlusion culling is ever scheduled. (3) is
defensible but the `fill_buffer` case differed in an important way — that call
_could not_ be honoured by the API on three backends, whereas every backend here
can do occlusion queries and this seam simply never grew the verb. Deleting
would record "we decided against it" for something nobody has decided against.

What made it urgent enough to write down rather than leave in a one-line note:
until one of the three happened, the seam had a resource that returned
"everything is hidden" and reported success doing it.

**What the browser gate now witnesses.** Probe group AE (`crcbl-webgpu`'s
`probe.rs`, `PROBE_OCCLUSION_*`) still builds a 32-query occlusion set, resolves
it over a sentinel and reads it back — writing to the `StreamWriter` directly,
since `WebGpuDevice::create_query_set` would refuse it. It is no longer evidence
for a capability; it is evidence for the divergence row's _kind_. The browser
and the replayer serve the whole spine, so the only missing piece really is the
seam verb, which is what makes WebGPU's row `Unwritten` rather than an
`ApiAbsence` and is a measurement rather than a reading of the WebIDL.

### `DivergenceKind::Unclassified` is gone, and can come back

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

Removed on 2026-08-19. It held exactly two rows — Metal's `TimestampQuery` and
`PipelineStatisticsQuery` — and its whole meaning was "which of the other three
this is cannot be settled from here, and the reason says what measurement would
settle it". `crcbl_mtl::adapter`'s counter-sampling probe has now taken that
measurement on CI, so both rows are classified and the variant held nothing.
`every_kind_describes_at_least_one_real_row` is the test that forces the issue:
a kind with no row is vocabulary rather than classification.

**The concept was good and may be wanted again**, so the reasoning is kept here
rather than only in `git log`. It was an admission about evidence rather than a
fourth kind of divergence: a property of a device this workspace cannot open,
where "a guess written into the data would read exactly like the classifications
that were checked". It blocked parity, because "nobody has looked" is not
"done". Reintroducing it is a variant, an arm in `blocks_parity`, and an entry
in that test's list.

**What settled the two rows**, from the `mtl e2e (macos-latest)` job:

```text
crcbl-mtl counters: supportsCounterSampling AtStageBoundary = false
crcbl-mtl counters: supportsCounterSampling AtDrawBoundary = true
crcbl-mtl counters: supportsCounterSampling AtDispatchBoundary = true
crcbl-mtl counters: supportsCounterSampling AtBlitBoundary = true
crcbl-mtl counters: counterSets = 0
crcbl-mtl counters: sampleTimestamps ... did not move across a 50ms sleep
```

Metal expresses both features — `MTLCounterSampleBuffer`,
`MTLCommonCounterSetStatistic` — so neither is an `ApiAbsence`; the code is
unwritten, which is `Unwritten`. **The blocker count did not move**, and that is
the point: reclassifying an unanswered question as unwritten work is honesty
about what is owed, not progress against it. (The number this paragraph gave —
eight — was stale when re-checked on 2026-09-06: `REVIEWED_BLOCKERS` holds six.)

### Four mesh tests stayed on Vulkan, and one degrades quietly

The mesh cluster split; what stayed did so for a reason worth keeping written
down, because "why is this still vk-only?" is the question a future reader asks.

**`per_cluster_culling_rejects_the_clusters_a_camera_hides`,
`a_scaled_instance_keeps_the_clusters_its_own_size_puts_on_screen`,
`the_gpu_descends_the_dag_to_the_cut_the_host_rule_says` and
`the_shadow_cascades_select_coarser_than_the_camera` are not splittable.** Each
reads a buffer only the amplification stage writes — `CLUSTER_SURVIVOR_WORD`,
`cluster_selection`, `shadow_selection`. On a backend with no mesh-shader path
those words are never written, so the agnostic half would be a counter nobody
incremented: a test that passes because nothing ran.

**`the_mesh_dispatch_extent_is_the_culled_instance_count`** stayed for the
opposite reason — its agnostic half already exists as `draw_gen_e2e`'s
`a_bucket_fills_and_empties_as_its_instance_comes_and_goes`, and splitting would
have duplicated a test that already runs on all four backends.

**That gap is closed, and the check the entry asked for was taken.** Run
32105356561's `vk e2e (lavapipe)` and `vk e2e (lavapipe, windows)` job logs
contain the degrade string **zero times** and report `MESH_SHADER | TASK_SHADER`
in every feature set they print, so the branch had executed on no device
anywhere: not radv here, not lavapipe on either CI arm. It now panics with the
reason rather than printing and continuing — a test claiming "the two geometry
paths agree" over one path is asserting nothing, and the uniform arm cannot
stand in for the missing half because it _is_ the other half.

Red-checked by withholding `TASK_SHADER` from that arm's open, which is the
project's own subtract-the-feature move: `41/48 tests run: 40 passed, 1 failed`,
naming the adapter. A driver genuinely without the feature needs a second device
to compare against, which is a change to make deliberately rather than a
condition to loosen.

**`the_mesh_dispatch_extent_is_the_culled_instance_count` had the same branch
and got the same treatment**, found by sweeping the tree for a `println!`
followed by a `return` inside a test. Its own message already said "radv and
lavapipe both report `VK_EXT_mesh_shader`", which is the argument for deleting
the branch rather than for keeping it. It is an `assert_eq!` on the selected
path now, red-checked the same way.

**The sweep that found it is worth keeping**, because it is cheap and it is a
different shape from the `features.contains` one already recorded above: a
capability branch can be written as an early return with no `if features` in
sight. Both queries are clean as of 2026-08-20 — no feature branch in the tree
has an `else` that only prints, and every remaining print-and-return site was
read and falls into one of four healthy shapes:

- **A bimodal exercise whose other arm asserts a refusal** —
  `vk_e2e/compute.rs`'s `update_bind_group` and `vk_e2e/indirect.rs`'s indirect
  count both `assert!(matches!(error, HalError::Unsupported { .. }))` before
  returning, so the lesser device is tested rather than excused.
- **A device-limit guard in `hal_seam_e2e` that returns a _classified_ outcome**
  rather than passing: `exercise_msaa_resolve` answers
  `Exercise::Unexercised(NO_MULTISAMPLING)` and the anisotropy exercise answers
  `Exercise::SilentlyIgnored`. Both are legitimate — a Vulkan device may
  genuinely offer fewer than four samples — and **neither has ever fired**:
  checked across all four backend jobs of run 32366511311, where the two strings
  appear zero times while `hal seam e2e` appears 38 to 50 times, so the channel
  was open and the exercises really ran. Left alone deliberately: unlike the
  mesh branches above, these guard a limit the specification permits rather than
  a feature every adapter grants.
- **A `Null`-backend guard** in `apps/quarry`'s device suite, which names the
  backend and the flag to rerun with.
- **A helper binary's usage message** in `crcbl-shell`'s `tests/bin/`.

### Five Metal divergence reasons that are wrong

Record; the live rows are in docs/backlog.md under the same heading. What
follows is the original text of the two items that have since closed.

1. ~~`PushConstants`~~ — **closed.** The obstacle never existed, and it was
   written down three times. Measured from the artifact the block lands at
   `buffer(1)`, _behind_ the bound buffer, so nothing shifted and the slice was
   smaller than its row implied. Left here as the record of what the wrong
   reason cost: three planning passes took it at face value.

   The original wording, for reference: The row, `MetalDevice::supports`,
   `features_of`'s docs and the pipeline-layout refusal all say the committed
   MSL puts a push-constant block at `buffer(0)`, ahead of every bound buffer.
   **No committed MSL declares a push-constant block at all** — `msl/ui.metal`
   has `vertices` at `buffer(0)` and `constants` at `buffer(1)`, and
   `binding.rs`'s own module docs say so, so two files in one crate contradict
   each other. This is the **same 2026-08 shader change** that made dx12's
   push-constant reason wrong: the block was replaced by a uniform buffer
   because WGSL cannot carry one. The obstacle is a missing test artifact — and
   it is the _same_ artifact dx12 needs, so build it once.

2. **`DrawIndirectCount`: the shared constant omits what the backend states
   twice.** It describes the work as a compute kernel encoding into an
   `MTLIndirectCommandBuffer`. True and insufficient: that kernel must be
   dispatched **before the render encoder was opened**, and the seam calls
   `draw_indirect_count` inside the pass. The constant exists precisely so the
   list and the backend cannot drift, and they have drifted the other way — the
   backend now carries a material fact the shared sentence does not. It changes
   the estimate from "encode an ICB" to "split the render encoder, or change the
   seam".

### WebGPU has no blockers left

Planned 2026-08-18 and closed the same day. `crcbl-webgpu`'s `TimestampQuery`
row is gone, `StorageImageBinding`'s went before it, and `parity_blockers()` now
names only `crcbl-dx12` and `crcbl-mtl`: every remaining WebGPU refusal is
WebGPU itself refusing, which is an `ApiAbsence` and not a blocker.

**The landmine this section warned about did not go off, and it is worth
recording why**, because the shape recurs. The warning was that the moment
`create_query_set` accepts `QueryKind::Timestamp`, `PassTimers::new` stops
returning `None` and the engine records query verbs every frame — and a verb
that `record_unsupported`s takes down the whole command buffer at `finish()`.
What kept it shut was gating **both** on one flag: `create_query_set` refuses
the timestamp kind on a device that opened without the browser's
`timestamp-query`, and `PassTimers::new` gates on the same
`Features::TIMESTAMP_QUERY`. A browser without the feature therefore builds no
timers and records no query verbs; one with it records verbs that are all wired.
The demo gate ran green with the hud demo creating a timestamp set and timing
every pass.

### Three WebGPU divergence reasons that are wrong

Record; the rows still resting on nothing are in docs/backlog.md under the same
heading. The `TimestampQuery` entry that used to sit here — "there is no
arbitrary-point write to narrow, implement rather than narrow" — was half right
and the half it got wrong is the interesting half. It reasoned that because
every `write_timestamp` in the repository already sat outside a pass, the
replayer could open an empty compute pass carrying `timestampWrites` around each
one. That would have worked and it would have been a _convention_ holding it up:
nothing stopped a caller putting a write somewhere the replayer could not wrap.
The seam took the other route and moved the two writes into the pass descriptor,
which is `timestampWrites`' own shape, so the replayer passes it straight
through and there is no free-standing call left to place wrongly. The feature
leak the same section named is closed the way it asked — by implementing.

### The seam's two attachment usages, which half the backends fold

Decision record; the decision is in docs/backlog.md. Found on 2026-08-20 by the
impossible case in `a_created_image_is_one_the_device_can_serve`: ask for
`Format::Rgba8Unorm` with `ImageUsage::DEPTH_STENCIL_ATTACHMENT` and

- **`crcbl-vk` refuses it** — `vkGetPhysicalDeviceImageFormatProperties2`
  answers `VK_ERROR_FORMAT_NOT_SUPPORTED`, on radv and on lavapipe alike;
- **`crcbl-wgpu` served it**, with no pending error. That crate was deleted on
  2026-08-21 and `crcbl-webgpu` folds the two usages the same way, which is why
  this entry outlives it.

**Neither backend is wrong on its own terms**, which is what makes this a seam
problem rather than a bug report. `crcbl_wgpu::conv::map_image_usage` folded
`COLOR_ATTACHMENT` and `DEPTH_STENCIL_ATTACHMENT` into the single
`wgpu::TextureUsages::RENDER_ATTACHMENT`, because that is what WebGPU has:
whether a texture can be a depth attachment is decided by its **format**, not by
a usage bit. So the caller's distinction is lost in the mapping, and what comes
back is a perfectly good colour render target that cannot be a depth attachment.

**Measured on every backend then in the tree, and the split is 3–2 the other way
from the first reading.** `Format::Rgba8Unorm` asked for as an
`ImageUsage::DEPTH_STENCIL_ATTACHMENT`:

| backend                                     | answer                                                              |
| ------------------------------------------- | ------------------------------------------------------------------- |
| `crcbl-vk` (radv, lavapipe)                 | **refused** — `VK_ERROR_FORMAT_NOT_SUPPORTED` from the format query |
| `crcbl-dx12` (WARP)                         | **refused** — `CreateCommittedResource` returns `0x80070057`        |
| `crcbl-mtl` (CI Mac)                        | served                                                              |
| `crcbl-wgpu` (lavapipe, deleted 2026-08-21) | served                                                              |

**This corrects an earlier reading here that called the fold "inherent to
WebGPU".** It is not a browser property. `crcbl_mtl::conv::texture_usage` folds
the same two flags into the single `MTLTextureUsage::RenderTarget`, so **half
the APIs the seam targets have one render-target usage** — Metal and WebGPU —
and only Vulkan and D3D12 carry two. The seam's two flags are the minority
position, not the norm one backend family fails to meet.

**And it closes the dx12 question this entry opened.** `crcbl-dx12`'s refusal
path was unexercised when the contract test only asked about depth formats,
since WARP serves both; the impossible case reaches it, and D3D12 refuses on its
own without any per-format query. dx12 is proven to refuse rather than merely
untested.

**It survived the `crcbl-wgpu` deletion.** `gpu-replay.js` folds the same two
flags for the same reason and says so at length: "`COLOR_ATTACHMENT` and
`DEPTH_STENCIL_ATTACHMENT` are both `RENDER_ATTACHMENT`: WebGPU has one
attachment usage and reads _which kind_ off the format". So `crcbl-webgpu`
behaves the same way, and this divergence is permanent unless the seam closes
it.

**That comment argues the fold is lossless, and it is right about the texture
and incomplete about the descriptor.** Its claim — "the seam's two flags carry
the same information twice over … so nothing is dropped by folding them" — holds
for what gets _created_: the format decides the kind, and there is no way to end
up with a texture that is the wrong one. What the fold does drop is the
**contradiction**. A caller who writes `Rgba8Unorm` with
`DEPTH_STENCIL_ATTACHMENT` has said two incompatible things, and folding
discards the disagreement instead of reporting it. Vulkan catches that through
the format query; WebGPU structurally cannot, because there is nothing left to
disagree with.

**Which sharpens the fix rather than changing it.** Only the seam can catch this
one, on any browser backend, ever — so if it is worth catching, it is worth
catching there. That is a stronger argument for a seam-level check than the
original measurement gave, and it still applies with `crcbl-wgpu` gone.

**The parity mechanism cannot see this**, and that is the interesting part. It
is not a capability — no `Capability` names it, both backends would answer the
same for every row — it is a _validation_ difference on an identical descriptor.
The seam's contract test catches the class where a backend returns a handle the
device cannot serve; it does not catch one where the backend returns a handle
that is fine but is not what was asked for.

**The fix that would make the seam agnostic**, and it is small: no API anywhere
permits a colour format as a depth-stencil attachment, so the _seam_ can refuse
it before any backend sees it. `Format` already knows whether it is a depth
format, so a shared helper in `crcbl-hal` called at the top of each
`create_image` would make every backend refuse identically, and the impossible
case above would then be refused everywhere rather than on some backends.

**The option the seam rule actually points at, and my first write-up missed it:
delete the distinction.** The standing rule is "refactor anything that cannot
work on all the backends — the seam must be fully backend agnostic", and this is
a seam flag one backend family cannot express. The precedent is
`CommandEncoder::fill_buffer`: the seam promised a repeating 32-bit value three
backends could not keep, and the _parameter was removed_ rather than emulated.

Applied here that means one `ImageUsage::RENDER_ATTACHMENT` in place of
`COLOR_ATTACHMENT` and `DEPTH_STENCIL_ATTACHMENT`, with each backend deriving
the API bit it needs from the format — which `crcbl-vk` can do, since the format
determines which of `VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT` and
`VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT` is correct, and which **half the
backends already do**: Metal and WebGPU both fold the pair onto one
render-target bit today, so the merge adopts the majority shape rather than
inventing one. It makes the contradictory descriptor _unwritable_ rather than
merely refused, which is the difference between a lock and a convention.

**Its cost is what makes it a decision rather than a task.** It is a breaking
change to `ImageUsage` and therefore to every caller — the renderer, the samples
and every suite — where the two validation options touch only `create_image`. It
also gives up a real thing: today a caller states intent and Vulkan checks it,
and after the merge nobody states intent at all, so a caller who _meant_ depth
and passed a colour format gets a colour attachment silently on every backend
rather than a refusal on some.

So the four options are: merge the flags (the seam rule's answer, breaking),
validate in a shared helper (a convention), validate through a type the backends
cannot bypass (a lock, more machinery), or document the divergence.

**Why it was recorded rather than done:** every one of the four is a design call
with a real cost, and the cheapest is not obviously right. A shared helper is a
convention, and a convention is not a lock — the same objection this repository
raises about `unsafe` contracts. The merge is the rule's answer and the most
expensive. Worth deciding rather than reaching for the first shape.

### How a caller asks whether a format is usable

Decision record; the decision is in docs/backlog.md. Found while building the
seam suite's raster fixture, which needed a depth-stencil attachment and hit it:

- **`crcbl-vk::create_image` does not check format support.** Asking for
  `D24UnormS8Uint` as a `DEPTH_STENCIL_ATTACHMENT` on radv returns `Ok`, while
  the validation layer reports `VK_ERROR_FORMAT_NOT_SUPPORTED` from
  `vkGetPhysicalDeviceImageFormatProperties2` and then two more VUIDs at view
  and pipeline creation. The first draft of that fixture **passed on undefined
  behaviour** before the layer output was read.

**The root cause is a seam gap, not two backend bugs.** `DeviceCaps` carries
features and numeric limits and **no format table**, so a caller has no portable
way to ask which depth-stencil format is usable as an attachment. The fixture
works around it by trying `D32FloatS8Uint`, then `D24UnormS8Uint`, and checking
_both_ channels — the returned `HalError` and `Device::take_error` — which is
the shape every caller would otherwise have to reinvent.

**The decision is one question: does `DeviceCaps` grow a format-capability
query, or do the backends validate at `create_image` and refuse?** The first is
more useful — a caller can choose a format instead of guessing and retrying —
and it is the one that satisfies the seam rule, since without it there is no
backend-agnostic way to ask. The second is cheaper.

**But they are not really alternatives, and treating them as one question hid
that.** A backend returning `Ok` for an image it cannot create is a correctness
bug on its own terms, and **`crcbl-vk`'s half is fixed**: `create_image` now
calls `vkGetPhysicalDeviceImageFormatProperties2` before creating and refuses
with `HalError::Unsupported`. Measured both ways by forcing the raster fixture
to try `D24UnormS8Uint` first — it returned `Ok` before and is refused now — and
it refuses nothing in use across every suite on radv.

**The contract now has a test, which is what makes this stop recurring.** It
asserts a successful `create_image` yields a usable image, reads both error
channels — a backend may refuse through the return value, as `crcbl-mtl` does,
or leave the refusal on `Device::take_error`, which is how `crcbl-wgpu` did it
before it was deleted — and requires at least one format to be served, so a run
where everything was refused cannot pass as coverage of the accepting path.
Measured: radv serves `D32FloatS8Uint` and refuses `D24UnormS8Uint`, lavapipe
both.

**Answered on 2026-08-20 by the new contract test, and the two answers differ.**
`a_created_image_is_one_the_device_can_serve` ran on both platforms in CI:

- **`crcbl-mtl` refuses, and helpfully** — "Format::D24UnormS8Uint is not
  supported by this device — Apple silicon reports no; use Format::D32Float,
  which the seam already prefers". So it validates format support already, by a
  different route than a per-format query, and never had this bug. **Proven
  clean**: the refusal path is the one that ran.
- **`crcbl-dx12` on WARP serves both formats — 2 of 2 — so its refusal path did
  not run at all.** That is _not_ a clean bill of health, and the distinction is
  the same one that hid the parity suite's missing half: a branch the
  environment cannot reach is not covered however green the run. What dx12 does
  with a format WARP cannot serve is still unknown.

Closing that last gap needs a format WARP genuinely refuses, which means finding
one rather than guessing — `D3D12_FEATURE_FORMAT_SUPPORT` on the WARP job would
name one, and `crcbl-dx12/src/adapter.rs` already makes that query for its own
capability reporting.

What remains a decision is only whether the seam grows the query.

**Not currently reachable by our own code**, which is why it is not urgent: the
seam suite's raster fixture negotiates — it tries `D32FloatS8Uint`, then
`D24UnormS8Uint`, and checks both the returned `HalError` and
`Device::take_error`. That workaround is the shape every caller would otherwise
have to reinvent, which is the argument for the query.

### MEASURED — CI's Metal device can serve neither query, and no mesh

Record; the decision it leaves is in docs/backlog.md under the same heading. The
counter probe ran. `Apple Paravirtual device`, macOS 26.5.2, and the answers
settle three open questions at once:

```
supportsFamily Metal3 = false   Apple7 = false   Mac2 = true
name contains "virtual" = true
supportsCounterSampling  AtStageBoundary=false  AtDrawBoundary=true
                         AtDispatchBoundary=true  AtTileDispatchBoundary=false
                         AtBlitBoundary=true
counterSets = 0
common set timestamp / stage utilization / statistic  present = false
```

The timestamp correlation answered too:
`wall_ns=53410875 cpu_delta=0 gpu_delta=0`. `sampleTimestamps:gpuTimestamp:` is
**inert** on this device — a real 53 ms of wall clock and neither clock moved.

**Both `Unclassified` rows are settled, and not in the direction the plan
hoped.** The device reports sampling at three points and then exposes **zero
counter sets**, so no `MTLCounterSampleBufferDescriptor` can name one and
neither a timestamp nor a statistics sample buffer can be built here at all.
`TimestampQuery` and `PipelineStatisticsQuery` on Metal are therefore
**implementable but unprovable on this CI** — the reason changes from "nobody
has looked" to "this device cannot, and CI has only this device". They are no
longer guesses, which is what `Unclassified` was for.

**The mesh precondition is settled too, and it is a no.** `wgpu-hal`'s gate is
`family_check && (Metal3 || Apple7 || Mac2) && !is_virtual`. This runner is
`Mac2` but its name contains "virtual", so mesh pipelines are excluded by that
formula regardless of the family. **Metal's `MeshShading` and `TaskShaderStage`
cannot be proved by this CI**, however the backend is written.

**A third thing this device cannot drive, found later:** both indirect
exercises. Their observable is which of two argument structures a draw read, and
this device reports no `max_draw_indirect_count` above one — so a single call
can only ever reach the first. `DrawIndirectCount` and
`IndirectArgumentPaddedStride` are therefore unexercised on Metal while every
other backend drives them. The exercise says so in its reason rather than
scoring a pass it did not earn, which is the right behaviour and also means the
Metal arm silently covers two capabilities less than the others.

### MEASURED — Metal's mesh rows are unprovable on the Mac CI has

The ICB probe's family list answers a question the mesh rows had only guessed
at. CI's Apple Paravirtual device reports **`Metal3 = false`**, and `false` for
every Apple family above `Apple5`; the highest it claims are `Apple5` and
`Mac2`.

**Metal mesh shading is a Metal 3 feature**, gated on
`supportsFamily:MTLGPUFamilyMetal3` — Apple's own check, and what
`MTLGPUFamily.metal3` documents itself as covering. So a
`MTLMeshRenderPipelineDescriptor` cannot be created on that device at all.

**What this changes, and what it does not.** `Capability::MeshShading` and
`Capability::TaskShaderStage` for Metal are `DivergenceKind::Unrun`
(`crates/crcbl-hal/src/capability.rs`): `crcbl-mtl`'s mesh half is written and
compiled, and what is missing is a device that will run it. What changes is the
_cost_ of finishing them — the work could be done and CI could never show it
working, so it would land as code no gate exercises. That is the same shape as
the two `Unclassified` counter-sampled query rows, which wait on a Mac that
advertises a counter set.

Both rows' `why` in `crates/crcbl-hal/src/capability.rs` now carry the
measurement, so the parity report says it rather than leaving the next reader to
rediscover it.

**It also bounds the deletion-bar decision.** **Every** Metal blocker in
`REVIEWED_BLOCKERS` is now measured unprovable on available hardware — the two
mesh rows here and the two counter-sampled query rows, which is the whole set.
This said "four of six" until the other two shipped, which is the concrete
content behind option (2) in "what bar the deletion clears": every row either
closed or _measured_ unprovable, with `Support::NotOnThisDevice` already able to
say so.

### MEASURED — where a push-constant block actually lands, per target

The artifact that three separate divergence rows described and that did not
exist now ships: `push_constant_probe.slang`, emitted for spirv, msl and dxil,
whose dispatch writes the constants into a buffer word by word. Read out of the
**emitted files**, not assumed:

| target | the block lands at                                                       | the bound buffer |
| ------ | ------------------------------------------------------------------------ | ---------------- |
| SPIR-V | `PushConstant` storage class, member offset 0, 16 bytes — no set/binding | set 0, binding 0 |
| MSL    | `buffer(1)`                                                              | `buffer(0)`      |
| DXIL   | `cb0` — register `b0`, space 0, size 16                                  | `u0`             |

**This corrects Metal's row, and makes that slice smaller.** The MSL puts the
block **behind** the bound buffer, not ahead of it. `ui.slang`'s old artifact
put it first only because it declared the push constant first, and the
declaration-order lint now has its first shipped exercise — so `crcbl-mtl` can
bind the block one past the last binding rather than shifting every table entry.

Two of the three are asserted by a test that reads the committed artifact;
DXIL's is read but not asserted, because its resource table lives in the DXBC
`RDEF` chunk and this crate is dependency-free by design. The test asserts the
container ships and the doc says which claim is which.

**WGSL is excluded and that was checked rather than asserted:** slangc _does_
emit a WGSL artifact for this source, and naga refuses it for a missing binding
decoration. So the validation sweep is not weakened — a shader declaring no wgsl
target contributes nothing to it, and one that declares it again is caught.

**What this unblocked:** dx12's `PushConstants` row is closed; Metal's still
stands, and its slice is smaller than planned because the block lands behind the
bound buffer rather than ahead of it.

### Slang cannot write a Metal ICB, and what the render-pass deferral left

Record; the gaps the deferral left are in docs/backlog.md under the same
heading. **`Capability::DrawIndirectCount` on Metal is closed** —
`crcbl_mtl::adapter` reports `Features::DRAW_INDIRECT_COUNT` unconditionally,
`crcbl_mtl::command` implements both indirect-count draws, and the
`(DrawIndirectCount, Metal)` row is gone from `DIVERGENCES`, which
`a_shared_reason_is_the_reason_its_row_carries` in `crcbl-hal`'s `capability`
module holds it to. It closed without an indirect command buffer: see "The Metal
ICB line of attack, and why it is closed" above for what shipped instead. What
follows is what that route left behind and must not be re-derived.

**Slang has no Metal indirect-command-buffer support, and that is still true.**
An ICB parameter is silently dropped rather than diagnosed, and Slang's own
"Metal-Specific Functionalities" page lists what the target does implement —
mesh shaders, parameter blocks as argument buffers, `SubpassInput` framebuffer
fetch, specialization constants as function constants, address spaces — with no
`command_buffer` or `render_command` anywhere, and an explicit unsupported list
that does not mention them either because they were never in scope. Every shader
in this repo is Slang and the
`shaders (committed artifacts match their sources)` CI job hashes each source
against its artifacts, so anything needing an ICB kernel needs hand-written MSL
outside that pipeline. Nothing does today — `indirect_count_args.slang` is an
ordinary Slang kernel with a committed `msl/indirect_count_args.metal` — and
this is the fact to check first if something ever wants one.

**Two alternatives to the shipped route, considered and declined, with reasons
that still hold:**

- **MoltenVK's route** — read the count back and loop on the CPU. It is what the
  reference Vulkan-on-Metal implementation does for `vkCmdDrawIndirectCount`,
  and it needs a CPU–GPU synchronisation to see a number the GPU wrote. Correct
  and simple, and it stalls the frame, so it fails the performance half of this
  project's bar. Worth knowing that MoltenVK gave up here — it is why nobody
  should read Metal's silence on this as "there is an easy way we missed".
- **Splitting the render pass at the call**, rather than deferring the whole
  pass's encoding. Ending and reopening a render encoder on a tiler stores every
  attachment out of tile memory and reloads it — per call, and the seam makes
  one call per bucket per frame. On Apple silicon that is the expensive
  operation, not a rounding error, and it drops any in-pass memory guarantee
  across the split. The deferral (`crcbl_mtl::command`'s
  `RenderCommand`/`RenderRecording` and the pure `crcbl_mtl::pass`) was taken
  instead and is the performant answer.

## What each remaining blocker row would take

**Every row is parked, and by one of two different decisions.** Six belong to
`crcbl-dx12` or `crcbl-mtl`, where work stopped on 2026-08-21 — see
`docs/plan/09-backends-metal-dx12.md`. The other four are `OcclusionQuery`, one
per GPU backend, parked on 2026-09-06 by the decision recorded above: the work
is a begin/end verb on `crcbl_hal::CommandEncoder` plus five implementations,
and it waits until something wants the counts. So `parity_blockers()` will not
reach empty, and that is a scope decision rather than work outstanding. The
mechanism is unaffected and stays enforced: the `Capability` enum is still
exhaustive, every backend still answers every row through a `match`, and a
capability added to `crcbl-vk` or `crcbl-webgpu` still fails to compile until
the deferred backends answer for it too. What changed is only which rows anybody
is working.

`REVIEWED_BLOCKERS` in `crates/crcbl-hal/src/capability.rs` is the answer at any
moment; this says what stands between each row and zero. `DEFERRED_BACKENDS` and
`DEFERRED_CAPABILITIES` beside it are the two ways a row comes to be parked, and
`every_parity_blocker_is_deferred_by_its_backend_or_by_its_capability` is what
stops a third kind of row joining them quietly.

| rows                                              | kind        | what closes them                                                                     |
| ------------------------------------------------- | ----------- | ------------------------------------------------------------------------------------ |
| dx12 `MeshShading`, `TaskShaderStage`             | `Unwritten` | the WARP device removal — `features_of` still never asks for `MeshShaderTier`        |
| Metal `TimestampQuery`, `PipelineStatisticsQuery` | `Unrun`     | **hardware.** Written and compiled; the runner advertises no `counterSets`           |
| `OcclusionQuery` on all four                      | `Unwritten` | **a seam verb.** A begin/end pair on `CommandEncoder`, then five backends serving it |

**The two kinds are not the same distance from done, which is why `Unrun`
exists.** Metal's two remaining rows are written — `crcbl-mtl`'s `query.rs`
builds the `MTLCounterSampleBuffer`, resolves it, and its `conv.rs` pins the
result layouts against Apple's own structs at compile time — and they are
blocked only on a device that advertises a counter set. The mesh pair left this
table on 2026-09-11: an M3 Pro ran the native proof harness, the adapter now
reports the flags through the device/OS gate, and the reference is
[metal-geometry-preference](metal-geometry-preference.md). dx12's two are not
written at all: the adapter does not report `Features::MESH_SHADER`, because
reporting it on a device that removes itself would be worse than not reporting
it.

**Two of the ten can be moved without new hardware**, and they are different in
kind from each other. dx12's pair is the subject of "DEFERRED — dx12 mesh
shading: WARP claims it and dies, hardware works". The occlusion four are one
piece of work, not four: the verb, then the five backends, and all of it
runnable here — they are parked because nothing wants the counts, not because
this machine cannot reach them. The two remaining Metal rows are unprovable here
whatever anyone writes; the honest reachable state on this machine is two rows,
not zero.

### Cross-format image views were declined, and the seam now says a view keeps its image's format

`ImageViewDesc::format` used to document itself as free to differ from the
image's "for sRGB reinterpretation", and no two backends agreed: `crcbl-dx12`
refused every differing format, `crcbl-mtl` refused only across depth or
stencil, `crcbl-vk` and `null` did not check, and `crcbl-webgpu` could not have
honoured it at all. `ImageViewDesc::check` now refuses any format but the
image's own, on every backend, so the seam keeps a promise all five can.

The two routes to the opposite decision were considered and declined, so that
they are not re-proposed:

- **A `view_formats` list on `ImageDesc`**, mirroring
  `GPUTextureDescriptor.viewFormats`, `VkImageFormatListCreateInfo` and D3D12's
  equivalent. It is the honest cross-backend shape, and it is a new field on the
  most-used descriptor in the seam plus a plumbing change in five backends, for
  a capability nothing in the workspace asks for — every render target and
  sampled image here is created in the format it is read in.
- **Typeless D3D12 resources**, or gating on `CastingFullyTypedFormatSupported`.
  The first costs compression on every render target; the second makes a seam
  promise depend on the machine. Both arguments are in `crcbl-dx12`'s
  `create_image_view`.

If a caller ever needs sRGB reinterpretation, the `view_formats` route is the
one to take — not a per-backend relaxation of the check.

**The shape that closes a seam-rule divergence, worth copying for the next
one:** the check lives in the shared descriptor's `check`, the seam's doc
**names the error** rather than only the rule, and **two tests** hold everyone
to it — the agnostic seam suite for the four native backends, and
`crcbl-webgpu`'s own `hal::tests` for the browser one, because the seam suite is
a native binary and `CRCBL_GPU` names no browser. That last split is the part
that keeps being needed: an agnostic suite reaching four of five backends cannot
be the only guard for a rule the fifth is the most likely to break. Both
`BufferDesc::size` (2026-08-24, where `crcbl-webgpu` alone served a zero) and
this view-format rule diverged exactly there.

## Two deferred backends keep their own copy of the image rules (2026-08-24)

Record; the coverage gap it leaves is in docs/backlog.md under the same heading.
`ImageDesc::check` lives on the seam (`crcbl-hal/src/resource.rs`), lifted
verbatim out of the null backend, and is now called by the null backend, by
`crcbl-webgpu` and by `crcbl-vk`. `crcbl-mtl` (`device.rs`) and `crcbl-dx12`
(`validate.rs`) still carry their own, with their own wording.

**The audit that consolidating `crcbl-vk` forced is the point of this entry**,
because the copies had in fact drifted, in different directions:

- **`crcbl-vk` did not check `mip_levels == 0` at all** and clamped it with
  `.max(1)` on the way to `vkCreateImage`. Fixed, and covered by
  `an_image_descriptor_the_seam_refuses_never_reaches_the_driver` in
  `crates/crcbl-vk/tests/vk_e2e/resources.rs`.
- **`crcbl-mtl` silently clamps a zero to one** —
  `let mip_levels = desc.mip_levels.max(1);` in its `create_image`. That is
  worse than an absent check: the caller asked for something the seam calls a
  bug and got a working image, so nothing will ever tell them.
- **`crcbl-dx12` does not check it either.** Its `validate.rs` checks
  `samples > 1 && mip_levels > 1` and nothing about zero.

Both are DEFERRED, so these are records rather than tasks. Consolidating them
means changing the wording their own tests assert, on two backends this machine
cannot run — one CI round trip each — so it is one slice per backend when work
resumes.

## `crcbl-mtl` still takes a writable storage binding of host memory (2026-08-24)

`BufferDesc::memory` states the rule: a buffer a shader writes must be
`MemoryLocation::DeviceLocal`, because D3D12's upload and readback heaps refuse
`ALLOW_UNORDERED_ACCESS` at creation and pin the resource to a state a shader
cannot write from. It is now enforced by the null backend
(`check_shader_writable_memory`), by `crcbl-dx12` (`binding.rs`), by `crcbl-vk`
(`write_descriptors`, covered by
`a_writable_storage_binding_refuses_host_visible_memory` in
`crates/crcbl-vk/tests/vk_e2e/pipeline.rs`) and by `crcbl-webgpu`
(`check_buffer_bindings`, covered by
`a_buffer_binding_is_held_to_its_slots_ceiling_and_memory` in its own
`hal::tests`). One backend does not:

- **`crcbl-mtl`** (`binding.rs`, the buffer arm of its bind-group builder) —
  DEFERRED, so this is a record, not a task. Metal has no such restriction of
  its own, so a caller who gets this wrong sees it only on D3D12. Worth noting
  what the WebGPU half turned out to be, since the same reasoning may apply
  here: the browser catches nothing of this rule, because
  `MEMORY_LOCATION_USAGE` in `web/engine/gpu-replay.js` maps `HostUpload` to
  `COPY_DST`, and `COPY_DST | STORAGE` is an ordinary legal WebGPU buffer.
  (`HostReadback` becomes `MAP_READ`, which may carry only `COPY_DST`, so that
  half was refused at `createBuffer` — as a usage error, one call earlier than
  the seam states the rule.)

No test anywhere exercised this rule before the Vulkan one above — verified by
searching the workspace for both backends' error wording, which appears only in
their own `src/`. That is why the gap survived on three backends at once.

## What the AppKit backend has and has not been run against

Record; what is uncovered is in docs/backlog.md under the same heading.

### Considered and declined

- **`PointerMode::Confined` is not implemented, and `POINTER_CONFINE` is
  clear.** macOS has no confine API; the only technique is warping back after
  the cursor has already crossed, which runs a frame late, fights the user's
  motion and manufactures events a consumer cannot tell from real ones.
  Approximating it would set a capability bit with no mechanism behind it. **Do
  not revisit without a public API to point at.**
- **`RAW_POINTER_MOTION` is set although the deltas are accelerated.** `NSEvent`
  deltas satisfy the half of that bit that decides whether a camera works and
  not the "unaccelerated" half; GLFW answers
  `glfwRawMouseMotionSupported() == false` on this platform for the same reason.
  Closing it properly means IOKit, a slice of its own.
- **`DeviceId` is a constant per device _kind_, as on X11 and Win32.** An
  `NSEvent`'s `deviceID` identifies a tablet and is meaningful only for the
  tablet family; the real answer is IOKit, the same slice as above.
- **The IME candidate window is placed at the window's origin, not at a caret.**
  The seam does not model a caret — nothing above `crcbl-shell` says where text
  is being typed. Closing it needs a seam addition ("the caret is here"), a
  decision above this crate to be taken once for every backend with an IME.
- **Lazy pasteboard provision (`pasteboard:provideDataForType:`) is not used,
  and it is structurally unavailable.** The callback arrives on the main run
  loop driven by the pasteboard server on behalf of a reader in another process
  — between two `Shell::pump`s an engine is rendering, so there is no run-loop
  turn to service it in — and a lazy owner must stay messageable until the
  flush, leaving the server holding an unretained pointer if the host process
  survives the shell. The same refusal `win32::clipboard` makes about
  `WM_RENDERFORMAT`. **Do not revisit without a seam that gives the shell a
  run-loop turn it owns.**
- **The engine's own format is published under its mime string, not a `dyn.*`
  UTI.** A pasteboard type is an arbitrary string, the mime is unique to this
  engine by construction, and it is byte-identical to what the other three
  backends name the same format with. Only text uses a system UTI.
- **Drag and drop _out_ is not implemented on any backend.** `15-windowing.md`
  scopes drag-and-drop to "file paths in"; `NSDraggingSource` is absent by plan
  decision rather than by gap.
- **No menu bar.** An unbundled Regular-policy application gets the system's
  default menu bar — enough to be focusable, not enough to ship (no ⌘Q).
  Building one is `NSMenu`/`NSMenuItem` and a decision about what belongs in it,
  which is above this crate.
- **`HW_UPSCALE` is clear although macOS has it.** A `CAMetalLayer`'s
  `drawableSize` is independent of its bounds, exactly what `wp_viewport` buys —
  but **the seam has no way to ask for it**. Setting the bit would be a claim
  with no mechanism behind it; closing it is a seam change (a render-scale
  request on `Shell`), a decision above this crate to be taken once for both
  backends.
- **`app_id` has nowhere to go.** macOS's equivalent is `CFBundleIdentifier` in
  an `Info.plist`, which cannot be set by a running process; the descriptor is
  validated for a NUL byte so a rejected descriptor is rejected here too, and is
  otherwise unused.
- **A live resize drag freezes the window**, on the same terms as the Win32
  modal loop and with the same unavailable fix.

## This machine's validation layer cannot see the two-submission hazard

Record; the measurement and what it costs are in docs/backlog.md under the same
heading. What stands in for it locally is
`reusing_an_offscreen_vulkan_ring_image_is_ordered_against_the_frame_that_had_it`
in `crates/crcbl-vk/tests/vk_e2e/swapchain.rs`, which provokes the same missing
dependency at record-time distance — a one-image ring, both trips recorded into
one command buffer — where every layer build sees it. It was falsified by
disabling the widening in `VkCommandEncoder::pipeline_barrier`: red, with the
layer naming
`vkCmdPipelineBarrier2 performs image layout transition on the VkImage ... which was previously read by vkCmdCopyImageToBuffer`.

**A CPU wait was tried first and does not work**, which is worth keeping because
it is a plausible idea. `acquire_next_frame` blocked on the retire timeline
before handing a reused image back. Instrumentation confirmed it ran (`reuse=3`
and `reuse=4` on the third and fourth frames of the failing test), and CI
reported the identical hazard anyway: a host-side wait establishes real ordering
but is not a queue dependency, and syncval reasons about submitted commands. It
also costs exactly the frame overlap the ring exists to provide. It was removed
rather than kept alongside the barrier.

**`crcbl-wgpu`'s offscreen path does not have this gap** — checked 2026-08-04,
and the untested assumption that used to sit here is deleted. Its acquire does
report `acquire_semaphore: None`, but the hazard needs the discarding transition
and that transition cannot reach wgpu: `WgpuCommandEncoder::pipeline_barrier` is
a no-op, so the `ResourceState::Undefined` the seam records is dropped at the
backend boundary, and wgpu-core inserts its own transitions from its usage
tracker — `command/transfer.rs`'s `transition_textures(&src_barrier)` before a
texture→buffer copy, and `device/queue.rs`'s
`insert_barriers_from_device_tracker` in front of each submitted command buffer,
which is what carries a texture's state across submissions (read in wgpu-core
30.0.0, the resolved version).

`reusing_an_offscreen_wgpu_ring_image_is_ordered_against_the_frame_that_had_it`
in `crcbl-wgpu`'s own suite (deleted 2026-08-21) is the check: a one-image ring,
trip one clears and copies out, trip two clears the same image to the reversed
colour, and the staging buffer must still hold trip one's. Green on radv, on
lavapipe and on the GL backend. Falsified both ways — writing trip two's colour
in trip one, and deleting the copy — each red, and each for its own reason.

**And the layer agrees, with a control that proves the layer was listening.**
Sync validation is not something wgpu-hal requests, so it was forced at layer
level: a settings file with `khronos_validation.validate_sync = true` reached
through `VK_LAYER_SETTINGS_PATH`, which makes the layer print
`Current Validation Enabled: … Synchronization` at `vkCreateInstance`. Under it
the wgpu test reports no hazard. The control is the same file and the same ICD
against `crcbl-vk` with the widening in `pipeline_barrier` disabled: red, with
`SYNC-HAZARD-WRITE-AFTER-READ … previously read by vkCmdCopyImageToBuffer`. So
the silence on the wgpu side is a verdict rather than an absence.

## Considered and declined: an OpenGL / GLES backend

**Decided 2026-08-05.** GL is a dying support surface and the engine will not
grow a `crcbl-gl`. The platform matrix is Vulkan for Windows, Linux and Android;
Metal for macOS and iOS; DX12 for Windows as the second Windows path. Nothing
else — see the Apple decision below, taken the same day, which closed the
MoltenVK option this entry originally listed alongside them.

Reasons, so this is not re-argued:

- **GL was already reachable and nobody needed a crate for it.** `crcbl-wgpu`
  enumerated `wgpu::Backends::all()` and wgpu's default feature set includes
  `gles`, so a GL device was enumerable through the existing backend — present
  and unproven rather than supported, since nothing in CI exercised it. **This
  reason expired on 2026-08-21**: that crate is deleted, so nothing in the tree
  enumerates a GL device any more. The decision stands on the two reasons below,
  which are the load-bearing ones.
- **The blocker is above the seam, not at it.** The renderer targets exactly two
  tiers, and Tier B is not a low bar: per-batch indirect draws, indexed SSBO
  lookups, and culling still running in compute. GLES 3.0 has no compute, no
  SSBOs and no indirect draw — those arrive in 3.1 — so the old hardware GL
  would be added _for_ cannot reach even Tier B. A Tier C is a renderer change
  with a third draw-emission path and a third set of golden images, which is far
  more expensive than the backend crate it would sit under.
- **GL fights this seam specifically.** No command buffers (the seam hands out a
  `CommandEncoder` and submits; GL executes immediately), thread-affine contexts
  against a seam that requires `Device: Send + Sync` on native, no explicit sync
  to map `pipeline_barrier` onto, and reversed-Z — locked engine-wide — needing
  `glClipControl`, which is core in GL 4.5 but only an extension on GLES.
- **It is the wrong tool for mobile anyway.** iOS is Metal-only and has
  deprecated GL ES since iOS 12; modern Android ships Vulkan. The Android gap is
  a `crcbl-shell` surface backend, not a HAL backend — `crcbl-vk` already exists
  and is the best-tested path in the workspace.

## Considered and declined: Vulkan on macOS and iOS

**Decided 2026-08-05. Apple platforms are Metal only.** `crcbl-vk` is not
expected to run there, MoltenVK is not a shipping path, and the MoltenVK spike
`docs/plan/09-backends-metal-dx12.md` scheduled as P14's first task **will not
be run** — the gate it was meant to inform is closed by this decision instead.

What that buys, and what it costs:

- **One macOS path instead of two.** The alternative was shipping on MoltenVK
  while native Metal caught up, which means two GPU paths to test on the
  platform with the least CI capacity, and bug reports that begin with "which
  one were you on".
- **iOS was never in question.** There is no Vulkan loader or ICD story on iOS
  at all; MoltenVK is linked directly into the app. Metal is the only path
  there, so choosing it for macOS as well makes the whole Apple side one
  backend.
- **The cost is that `crcbl-mtl` is now load-bearing rather than an
  optimisation.** Until it can present a frame, macOS has no native GPU path —
  `crcbl-wgpu` is the only thing that runs, at Tier B. That raises the stakes on
  MTL3 (first pixel) and MTL5 (swapchain) and is the reason they are the two
  slices worth watching.

The technical question the spike would have answered is recorded here because it
is the same question `crcbl-mtl` itself has to answer, and the answer is now
expected from the Metal side rather than the Vulkan one: `crcbl-vk` demands
`Features::GPU_DRIVEN` outright rather than degrading, that set includes
`DRAW_INDIRECT_COUNT`, and `crates/crcbl-vk/src/adapter.rs` reads it straight
off `VkPhysicalDeviceVulkan12Features`. **Metal has no native indirect-count
draw**, which is exactly why `crcbl-mtl` reports Tier B today and why MTL6's
indirect-command-buffer work is what moves it. MoltenVK would have hit the same
wall from the other side.

One framing note kept because it explains why "the user installs MoltenVK" was
never the shape this would have taken: MoltenVK ships **bundled with the
application**. The Vulkan SDK's macOS installer places an ICD for development,
but a shipped app embeds `libMoltenVK.dylib`. It describes a developer's
machine, not a player's.

## Confirmed: DX12 stays, alongside Vulkan on Windows, and last

**Decided 2026-08-05**, closing a question that had been half-answered twice —
`docs/plan/09-backends-metal-dx12.md`'s original text justified DX12 as old-iGPU
coverage, its 2026-07-27 correction retracted that and substituted the Xbox door
plus Windows tooling, and neither pass weighed it against simply using
`crcbl-vk` on Windows.

**Windows keeps both backends. DX12 is never a replacement for Vulkan there.**

### The asymmetry that settles the "instead of" framing

`crcbl-vk` has to exist regardless — it is the Linux path and, per the same
day's platform decision, the Android one. Windows support falls out of it at
approximately zero marginal cost, because it is the same code reaching a
different loader. So dropping Vulkan _from Windows_ saves nothing: the crate,
its tests and its maintenance all stay. Replacing it with DX12 would pay for a
new backend to obtain a working path that already exists.

It would also cost the one thing Windows is uniquely placed to give:
**cross-backend differential debugging on identical hardware.** "Does it repro
on the other backend?" is reason #1 in `crcbl-hal`'s own argument for dynamic
dispatch and for compiling two backends into one binary, and Windows is the only
platform where both can run against the same GPU.

### Why it is still worth building

- **Xbox.** The only item here obtainable no other way.
- **A GPU device on the Windows CI runner.** Every software-rasteriser job in
  `ci.yml` is `ubuntu-latest`/lavapipe; `windows-latest` has no device at all,
  which is why Windows has no golden images and no sample-level render pass.
  WARP is D3D12's software rasteriser and ships in Windows, so this would be
  Windows' lavapipe. **Confirmed** — see "WARP clears the bindless bar —
  measured, 2026-08-05" below.
- **Robustness against a missing or stale vendor ICD.** D3D12 is part of the OS;
  Vulkan is not.
- **Windows-on-ARM**, where D3D12 is first-class and Vulkan is patchier.
- **PIX and DRED**, and DXGI's waitable swapchain object — a mature answer to
  the closed-loop frame pacing this backlog already has open, where the Vulkan
  side needed `VK_KHR_present_wait` (bound in the pinned `ash`) and
  `VK_EXT_present_timing` (ratified, but not in `ash` at all, so genuine
  hand-written FFI) — both of which have since landed.

### Why it is last

- It maps near-1:1 onto the Vulkan-shaped seam, so **it finds no HAL leaks**.
  That is a cost saving and a value reduction at once: Metal is the backend that
  stresses the abstraction, which is why the plan orders it first.
- Its value is infrastructure and optionality, not capability. Nothing renders
  today that it would render better.
- It is a crate comparable in size to `crcbl-vk`, the largest in the workspace,
  plus a third shader artifact (DXIL) in `crcbl-shaders` and its manifest, plus
  another pinned toolchain in the `shaders` job, plus a second Windows path to
  test permanently.

Ranked below finishing Metal — which after the same day's decision is the _only_
Apple path — and below an Android surface in `crcbl-shell`, which is the largest
coverage win available and needs no new HAL backend at all.

## Considered and deferred: console backends

**Decided 2026-08-05. No console support now; open to it if someone asks for
it.** Nothing is being built speculatively, and nothing in the engine forecloses
it. The canonical platform matrix is in `docs/plan/01-foundations.md`.

### What each console would actually need

- **Xbox — comes free with DX12.** It is D3D12X through the GDK rather than
  desktop D3D12, so it is not literally the same backend, but `crcbl-dx12` is
  the prerequisite and the delta is small. This is already the strongest item in
  DX12's justification (see the DX12 entry above).
- **PlayStation — a private crate.** There is no Vulkan on PlayStation, ever.
  PS5 is AGC (with a GNM compatibility layer), PS4 is GNM/GNMX, and shaders are
  PSSL. **The blocker is legal rather than technical**: the SDK, its headers and
  the API's detailed shape are under NDA, and downloading any of it requires
  licensed-developer status with an approved concept. So it cannot live in this
  repository and cannot be written speculatively by anyone.
- **Switch — probably `crcbl-vk` with a shell backend.** It has a working Vulkan
  driver. NVN is the faster native path and what shipping titles use, but Vulkan
  is a genuine bring-up route, which makes Switch by far the cheapest console to
  reach and the only one needing no new HAL backend.

### Why this costs nothing to defer

**The seam is what makes a console backend possible at all.** A closed crate
implementing the public `crcbl-hal` traits drops into a private workspace as a
path dependency, with zero changes above the seam — the renderer, ECS, UI and
every game compile unchanged. That property is already load-bearing for the four
public backends; consoles just exercise it under an NDA.

AGC is also close to the shape already built: explicit command buffers, explicit
sync, bindless descriptors, GPU virtual addresses. The Vulkan-flavoured seam is
roughly right for it, for the same reason DX12 maps near-1:1.

The genuinely new axis is **shaders**. PSSL is HLSL-like and the platform
toolchain consumes HLSL-ish input, so the path is Slang → HLSL → PSSL — a fourth
artifact after SPIR-V, WGSL, MSL and DXIL, and the only one whose compiler could
never run in public CI.

### `BackendKind` would need a variant — and that is not a problem

`crcbl_hal::BackendKind` is a closed enum —
`Vulkan | WebGpu | Metal | Dx12 | Null` — so a console backend needs a new
variant (naming a console is not an NDA breach) or a `Custom(&'static str)`,
because a private crate cannot add one to a public enum it does not control.

**Add it when a console backend actually exists.** This was first written up as
something to settle before the seam freezes, on the grounds that a new variant
is a breaking change to a public API. That reasoning does not apply here: the
workspace is `0.1.0` with no tags, everything so far is unreleased, and the
project's own convention is that below 1.0 a breaking change bumps the minor.
Breaking changes are routine and expected, so there is nothing to buy by
deciding early — and adding a variant nothing implements would be the
speculative machinery this codebase deletes rather than keeps.

## `render_area` does not exist in Metal, and clears diverge because of it

Decision record; the decision is in docs/backlog.md. Options, none taken:

1. Document `render_area` as affecting rasterisation only, and require a caller
   wanting a partial clear to draw one. Cheapest; makes the seam honest about
   the weaker guarantee.
2. Have the Metal backend emulate a partial clear with a draw when `render_area`
   is a sub-rect and the load op is `Clear`. Costs a pipeline in the backend.
3. Drop `render_area` from the seam entirely and give the encoder a scissor
   call. Largest change, and closest to what Metal, DX12 and WebGPU all do.

Wants a decision before anything starts relying on the Vulkan behaviour. Both
backends must then be re-verified.

## WARP clears the bindless bar — measured, 2026-08-05

Record; the two rows deferred inside DX4 are in docs/backlog.md under the same
heading. The question this file told the DX12 phase to settle is settled: the
`windows-latest` runner reports
`ResourceBindingTier=3  HighestShaderModel=6.8 sm66-dynamic-resources=yes` for
both the DXGI lists and `EnumWarpAdapter`, and `crcbl_dx12::device`'s
`a_pulled_triangle_is_drawn_by_d3d12_and_read_back_texel_by_texel` has since
passed there — so WARP supports SM6.6 dynamic resources **and executes a
shader**, which closes the coverage hole that `windows-latest` has never had
golden images or a render pass: Windows can have a software rasteriser, the way
Linux has lavapipe. What that does not cover is hardware: WARP is one
implementation with one set of tolerances, and no D3D12 code in this workspace
has run on a GPU. `renderer-tier=B` in the run's lines is the backend's own gap
— `TIMELINE_SEMAPHORE` waits on a call no slice has written.

Deferred inside DX4, each with what it would take:

Two things DX1 decided that a later slice may have to undo:

- **`DESCRIPTOR_INDEXING` is reported ahead of a call** — the opposite of what
  `crcbl-mtl` ended up doing, deliberate because `adapters()` is where the WARP
  question is asked, so the flag has to be derivable before any device exists.
  The binding slice must withdraw it if D3D12 bind groups cannot deliver a
  runtime-sized array, exactly as Metal's did.
- **`driver` comes from `CheckInterfaceSupport(IID_IDXGIDevice)`**, documented
  as a Direct3D 10 interface check, with a fallback string when it refuses. WARP
  is the adapter most likely to refuse it; if the CI line shows the fallback on
  real hardware too, the field needs a different source.

**Do not write a LUID into code or an assertion.** DXGI's `AdapterLuid` is
per-boot — two CI runs reported different LUIDs for the same two adapters. It is
an identity _within_ one enumeration and nothing more: fit for de-duplicating a
list, unfit for a fixture, a golden value or a comparison across runs.

## Only `null` enforces pass scoping, and that is the design

**Not a defect in `crcbl-vk`, and worth saying so before someone fixes it.**
`begin_compute_pass` checks only whether a compute pass is already open, and
`begin_render_pass` only whether a render pass is — so a compute pass opened
_inside_ a render pass, or the reverse, is accepted, and `dispatch` checks no
scope at all. The null recorder rejects every one of those as `NestedPass` or
`OutsidePass`.

That asymmetry is what `CommandEncoder`'s own documentation asks for. It lists
the scoping rules — including "passes do not nest" — and then says a backend
**may assume** they hold, naming `crcbl_hal::null` as the one that checks them
so the graph's unit suite can catch a violation without a GPU. `crcbl-vk` is
conformant; `null` is the reference.

The checking half is guarded: `crcbl-hal`'s `null::tests` asserts `NestedPass`
for a compute pass opened inside another, and `OutsidePass` at three more sites.
No test asserts the _absence_ of a check in `crcbl-vk` — that would pin a gap in
place if the design ever changed.

Recorded because it is the second place the mock is stricter than the backend it
models, after the cross-instance surface bug, and that pattern is worth watching
rather than rediscovering.

The illegal _commands_ are still caught by the validation layer at record time.
The illegal _pass bookkeeping_ is caught nowhere.

### Metal compute works, confirmed on hardware

Record; the gap it leaves is in docs/backlog.md under the same heading.
`ComputePipelineDesc` carries `workgroup_size`, `crcbl-mtl` implements
`bind_compute_pipeline`/`dispatch`/`dispatch_indirect`, and the macOS CI job ran
all three new tests on a real device:
`a_compute_dispatch_writes_the_values_it_ was_asked_for`,
`an_indirect_dispatch_reads_its_workgroup_count_from_the_buffer` and
`the_compute_pass_opens_an_encoder_and_its_calls_fail_only_as_themselves` all
PASS (112 tests run, 6 skipped). **Compute is no longer a Vulkan-and-wgpu
capability.** `indirect_count` is a separate Metal refusal and still stands.

The 6 skipped are the pre-existing draw tests that fault on that runner
(excluded by name in `.github/workflows/ci.yml`) — unrelated to compute, but
worth knowing the device is not fully healthy before reading any green macOS
run.

### Settled: `setDepthStencilState(nil)` hung every Metal draw

**Found by bisect, fixed in `8e40f55`.** For months every draw `crcbl-mtl`
recorded hung on GitHub's macOS runner with
`kIOGPUCommandBufferCallbackErrorHang` while render-pass clears succeeded, and
six tests were quarantined for it. Two hypotheses were wrong before this one.

The final round: ten probes, each the known-good hand-encoded pass plus exactly
one call, with a known-red and a known-green control. **7 passed, 3 failed**,
and the three failures are precisely the ones passing `nil` to
`setDepthStencilState:`. Its twin — same selector, real state object — passed,
as did `setCullMode`, `setFrontFacingWinding`, `setTriangleFillMode`,
`setDepthClipMode` and `setDepthBias:slopeScale:clamp:` individually.

**The fix makes `None` unrepresentable** rather than substituting at the bind
site: a pipeline without depth-stencil state holds a default object the device
builds once at open, so nothing in the crate can produce nil and the type says
so. Every descriptor field is set explicitly — `Always`, no depth write, `Keep`
on all three stencil outcomes — because `objc2-metal` is a generated binding
that documents no defaults, and guessing them would trade a hang for wrong
pictures.

Things worth carrying out of this investigation:

- **Three hypotheses, two wrong, and both wrong ones were "what's left standing"
  arguments.** The render-target format and the long draw forms were each the
  last candidate after eliminating others. What settled it was a _controlled
  comparison_ — one call reproducing the hang and its near-identical variant not
  — rather than an elimination.
- **The bug was invisible to every picture-based test by construction.** All six
  replay calls are image-neutral for a pipeline with no culling and no depth
  attachment. No golden could ever have caught it; only a device that faults.
- **Carry a known-red and a known-green control in any bisect** whose baseline
  would otherwise be a previous log. Without them "everything passed" cannot be
  distinguished from "the runner changed".
- The probes were deleted once they answered. A diagnostic that keeps running
  after it has reported is noise in the next run's signal.

### Metal draw coverage in CI: what the ecosystem does

Researched 2026-08-10, because "is this just us?" was worth answering before
buying hardware. It is a real and widely-hit gap, but **it is not our failure**
— see the entry above.

- **GitHub's own position**: "Add support for Metal in macOS images" is an open
  discussion; a GitHub staff reply says _"There is no ETA for now but it's on
  our radar."_ Real GPU passthrough for hosted macOS runners is an open feature
  request.
- **Godot hit the paravirtual device too**, differently: it aborts with
  `-[AppleParavirtDevice newArgumentEncoderWithLayout:]: unrecognized selector`
  on `Apple Paravirtual device (Apple5)`. Closed unresolved; the reporter asked
  only for a graceful error. So the device is genuinely feature-poor — but ours
  fails on draws it demonstrably supports.
- **The asymmetry that matters for the plan**: Linux and Windows both have
  software rasterisers CI can install — lavapipe, which we already use, and
  **WARP** on Windows. macOS has no equivalent, which is why this gap is
  macOS-shaped rather than general.

**The consequence this named was acted on.**
`crates/crcbl-dx12/tests/run-dx12-e2e.sh` runs on `windows-latest` pinned to
WARP, which is the D3D12 software rasteriser closing that gap the way lavapipe
closes Vulkan's. macOS still has no equivalent, which is the whole of what makes
this gap macOS-shaped.

### Settled: the render layer runs on all four backends

**D3D12 drew the cube frame on `4907b7e`**, and with the tightest golden match
of any backend:

```
dx12 selected IndirectCount / Bindless / Rasterised
device on adapter 0 "Microsoft Basic Render Driver" type=Cpu (CRCBL_ADAPTER=cpu)
golden cube on dx12 — 256x192: max channel delta 1, 0 over tolerance (0.0000%),
ssim 0.999879
```

So `render_e2e.rs` now passes on **Vulkan, Metal and D3D12** and the step is a
real gate — the `continue-on-error` is gone. The fourth backend reaches the same
goldens by a different road: `crcbl-webgpu` has no native binary to run this
suite from, so `pages.yml`'s `render-harness` job renders every scene in a real
browser and compares it against the same committed references. One golden,
blessed on lavapipe, matched by four independent implementations. (It read
"Vulkan, native wgpu, Metal and D3D12" until `crcbl-wgpu` was deleted on
2026-08-21; the count did not change, the fourth name did.)

The two causes, both found by asking the device rather than reasoning about it:

1. **A constant buffer view outran its buffer.** D3D12 requires a CBV's
   `SizeInBytes` be a multiple of 256; `crcbl-dx12` rounded the _view_ up while
   the allocation stayed 16 bytes. The allocation is padded now, only for
   `UNIFORM` usage.
2. **Three draw-generation buffers were on an upload heap and bound writable.**
   D3D12 refuses `ALLOW_UNORDERED_ACCESS` on that heap at creation and pins the
   resource to a state no shader can write from. They are `DeviceLocal` now, and
   the frame zeroes them with a clear dispatch.

**The second was not a D3D12 bug at all** — it was a compromise this file had
already recorded under GPU-driven draw generation, kept because `fill_buffer` is
legal only outside a pass and the graph had no fill step. Vulkan tolerated it
for months. Worth remembering: **a portability compromise that one backend
accepts is not a compromise, it is a latent failure with a delay on it.**

Also settled by that work: a graph-level fill was the obvious fix and the wrong
one. `fill_buffer` is four separate backend promises — Metal repeats a byte,
wgpu clears only to zero, `crcbl-dx12` refuses it entirely — so it would have
moved the blocker one call later. A dispatch's portability is held by
construction.

**Two follow-ups this leaves:**

- ~~The `dx12 e2e (WARP)` job is misnamed~~ — renamed to
  `dx12 e2e (software adapter)`. `CRCBL_ADAPTER=cpu` selects the single
  `DeviceType::Cpu` adapter, and on that runner it is **Microsoft Basic Render
  Driver** rather than WARP. Naming the job after a specific implementation
  claimed something the pin never asked for.
- ~~`crcbl-dx12::fill_buffer` wants recording as a deliberate non-fix~~ — done,
  at the refusal itself. D3D12's fill needs a shader-visible descriptor heap
  this backend does not create, and nothing in the workspace needs it now that
  the counters are cleared by dispatch. A caller who wants it should say why a
  dispatch will not do.

### What WARP has actually proven

Record; what is still unproven is in docs/backlog.md under the same heading.
Worth separating from what is merely implemented, because this backend is
written blind and only CI ever executes it.

Proven on hardware:

- Compute dispatch, indirect dispatch, and a workgroup size refused for
  disagreeing with the container's `[numthreads]`.
- **Indexed draws, indirect draws, and indirect-count draws reading a GPU-side
  count** — all four passed on `c4e8655`.
- The root-signature register fix, implicitly: `compute_probe`'s pipeline could
  not have been created at all under the old `[[vk::binding]]`-derived rule.

**A rot to expect, and it has now happened three times.** `c4e8655` reddened
WARP on
`the_metal_slices_that_have_not_arrived_still_refuse_and_name_themselves` — a
test asserting the unimplemented calls still answer `Unsupported` — because
three of them had just started working. The Metal mesh slice reddened it again
on 2026-08-20, and the counter-query slice emptied it entirely on 2026-08-21:
its last two members were `QueryKind::Timestamp` and
`QueryKind::PipelineStatistics`, and with those implemented the list had nothing
in it.

It is now `the_query_slice_refuses_for_the_device_or_builds_the_object`, which
asserts **both** arms against what the device reports rather than asserting an
absence: a machine carrying the counter set must build the object, one without
it must refuse naming `counterSets`. That shape cannot rot the same way, because
implementing something does not falsify it.

**The general lesson, which cost three red runs to learn:** a test whose subject
is "this is not implemented yet" is a test that fails on success, and it runs
only on the platform that can implement it — so the failure always arrives from
CI, never locally. Prefer asserting what a call _does_ on each side of a
capability to asserting that it does nothing.

### Settled: base vertex and base instance never reach a shader

Recorded because it is a rule for every future shader here, and because it is
the first time the differential render gate caught a real divergence rather than
a hypothetical one.

`SV_VertexID` and `SV_InstanceID` mean **different things per target**, measured
rather than assumed: SPIR-V subtracts `BaseVertex`/`BaseInstance` (HLSL's
meaning), DXIL passes them through with D3D12 excluding both bases, and WGSL and
MSL index raw builtins that _include_ them. A pooled mesh at a non-zero base
vertex therefore rendered a correct pyramid through wgpu and a corrupted slab
through Vulkan — one source, two pictures — and `run-cross-backend-e2e.sh`
failed on it at 10.09% of pixels with a structural mismatch.

**The rule: every draw passes zero for both bases, and the real values arrive in
a per-draw constants block.** Zero is the one value all four lowerings agree on,
so nothing in the picture depends on how a target lowers a builtin.
`sprite.slang` reached the same conclusion independently for its own case; this
makes it the pattern rather than one shader's workaround.

The gate only caught it because `Scene::Cube` was changed to draw a second mesh
at a non-zero base. **A path nothing exercises is a path the gate cannot see** —
which is the general form of this and worth remembering before trusting any
green run over content that does not use the feature.

### The first-triangle milestone is four different claims, not one written four ways

Recorded because the opposite is the obvious guess and unifying the four names
would flatten a real difference. All four were read end to end:

- `crcbl-mtl`'s
  `a_metal_triangle_draw_paints_the_centre_and_leaves_the_corners_clear` draws a
  hand-written MSL triangle with **no bindings at all** — geometry from
  `[[vertex_id]]`, a fragment shader returning the `INK` literal — and
  `assert_ink_triangle` checks that the centre texel is exactly the ink colour,
  all four corners exactly the clear, and every other texel is one of those two.
  `ink_msl`'s own doc says why it is not the engine's shader: pulling vertices
  needs bind groups.
- `crcbl-dx12`'s
  `a_pulled_triangle_is_drawn_by_d3d12_and_read_back_texel_by_texel` runs the
  engine's `crcbl_shaders::triangle` through an SRV over a storage buffer, and
  `assert_triangle_drawn` asserts three fixed probes are red-, blue- and
  green-dominant and that each probe's channels sum to full scale — the
  barycentric property that catches a wrong element stride.
- `crcbl-vk`'s `a_triangle_pulled_from_a_vulkan_storage_buffer_reaches_memory`
  makes the same pulled-vertex claim but derives its probes from the geometry
  (75% of the way from centroid to each vertex) rather than fixing pixel
  coordinates, and adds a centre-blend assertion for interpolation.
- `crcbl-vk`'s `the_vulkan_triangle_matches_its_golden_image` is the P1
  golden-image gate against `tests/golden/triangle.png` at
  `Tolerance::RASTERISER`.

So the flat-colour coverage check, the two dominance checks and the golden
compare are four distinct assertions; only the backend qualifier was missing and
only that was added. What is genuinely absent is a golden-image gate on Metal
and D3D12 — `crates/crcbl/tests/render_e2e.rs` is the backend-agnostic golden
and covers whole scenes, not the triangle.

## Decisions taken 2026-08-10, so they are not re-argued

Each of these was a question the coverage audit raised and left open. They are
answered here rather than carried, with the reasoning, so a later session can
disagree with the argument rather than rediscover the question.

### Decided: device loss surfaces, it does not self-heal

The engine will not recreate the device. `HalError::DeviceLost` propagates and
the loop stops with an error naming it.

Recreation means rebuilding every resource the frame graph, the pools and the
renderers hold, on a code path that by construction almost never runs — the
classic shape of a recovery path that is broken when it is finally needed.
Surfacing it is honest, testable in one assertion, and leaves the harder policy
available later for whoever has a real reason to want it. A game that wants to
survive a lost device can restart the engine; nothing in the samples does.

**Implemented and pinned.** `Recorder::lose_device` reports a device as gone and
keeps it gone, and `a_lost_device_stops_the_driven_loop_with_an_error_naming_it`
in `crates/crcbl/src/engine.rs` drives `drive` over a real `GpuContext` on it:
the run ends on the frame that hit the loss, with the driver's own message, with
its frame budget unspent and with no rebuild attempted. The last of those is
asserted off the `hal: reconfiguring the swapchain to ` log line rather than off
the recorder, because a rebuild that failed records no event — so an engine that
never tried and one that tried and was refused look identical in the stream, and
those are exactly the two policies this entry chose between.

### Declined: minimum-count floors on the e2e harnesses

Both backend harnesses now select `--run-ignored only`, so the number they guard
on is the device-test count — 70 on the Metal runner and 73 on the D3D12 one,
measured. The zero-count guard still passes a selection that collapsed from 73
to 3.

A floor would catch that, and it is **not** being added: any threshold below the
real number is arbitrary, and a threshold equal to it fails CI every time a
device test is added, which trains people to bump it without reading. The counts
are printed by both harnesses and visible in the run log, and the classification
that produces them is now documented in `docs/plan/12-testing.md`. Revisit if a
collapse ever actually happens — at that point the floor has evidence behind it
instead of a guess.

### `crcbl-dx12` has no timeline-semaphore test because it has no timeline semaphore

Recorded so it is not mistaken for a coverage gap. `crcbl-vk` and `crcbl-mtl`
each have
`a_<backend>_timeline_semaphore_signals_from_a_submission_and_the_cpu_sees_it`;
D3D12 has none because the feature is unimplemented there, not because the test
was forgotten.

## The owner tag cannot separate two owners whose tags collide

Found while implementing the seam's third obligation in `crcbl-wgpu`, deleted
2026-08-21. The finding is about every backend that carries the side table
rather than about that crate, which is why it outlives it.

**The slot's `u64` cannot separate owners whose tags collide.** True of
`crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` for the same reason: every pool holds
exactly one owner's rows, so a foreign handle that gets past the tag lands on a
row the _looking-up_ owner filled, the id agrees, and the lookup succeeds. The
id half is what catches a shared pool and what stops `handle::remove` from
taking a row this owner does not own — it is not a second line of defence
against a colliding tag, and a test written to claim it was failed and said
otherwise. The hole opens only after `OWNER_TAG_COUNT` owners in one process.

**Owner tagging is a hand-written copy per backend, and extracting it was
declined.** Each backend spells the same idea itself, which is duplicated
knowledge that will drift. Pulling it into `crcbl-hal` is the obvious move and
was deliberately not taken while the existing copies work; revisit when a
backend has to grow a fresh one, which is the point at which the duplication
stops being tolerable.

### Accepted: CI will not have a real Metal GPU, and that is not a task

Recorded as a decision so it stops reading as work somebody could pick up.

GitHub's hosted macOS images expose an `Apple Paravirtual device`. Real GPU
passthrough is an open feature request on their side with no date, so no amount
of work in this repository changes it. The options that would are a self-hosted
runner or a Mac in somebody's office, and both are a standing cost for a gap
that is narrower than it first looks.

**What the paravirtual device does cover**, and this was itself a correction —
it was long assumed to run no shaders at all, generalised from macos-14, the one
image whose `MTLCreateSystemDefaultDevice()` returns nil. macos-15 and macos-26
run compute dispatches and triangle draws correctly, `macos-latest` resolves to
macos-26, and the Metal suite's device tests pass there. The render e2e draws
every scene on it and matches goldens blessed on lavapipe.

**What it does not cover**, stated so nothing implies otherwise: a discrete or
unvirtualised Apple GPU, and anything a real driver does that a paravirtual one
does not. `crates/crcbl-mtl/tests/run-mtl-e2e.sh`'s header already says this and
should keep saying it. Metal has no software rasteriser, so unlike Vulkan
(lavapipe) and D3D12 (WARP) there is no second implementation to cross-check
against — the cross-backend comparison is the substitute, and it is weaker
because it compares Metal against a _different API_ rather than against a second
Metal.

The mitigation is the one already in place: a person on a real Mac can run
`run-mtl-e2e.sh` unchanged, and that remains the only thing that covers a
non-virtual GPU. Nothing else is owed here.

### The split comparator: what CI has to confirm, and what was declined

The scoring split landed — `Tolerance` carries `gross_channel_delta: 24` and
`max_gross_ratio: 0.001` beside a `max_failing_ratio` relaxed to 0.01, and
`compare` counts each pixel against both thresholds on its one existing visit.
What is left is verification nobody here can run, plus the alternatives that
were tried on paper and rejected, so they are not re-proposed.

**Not verified locally, and CI is the only verdict: the two frames the bound is
sized against.** Neither backend runs on this machine.

- **D3D12 / WARP's sprite scene** — 76 pixels of 49 152 over the delta at up
  to 13. It now clears the drift budget by 6.5× where it used to clear one ratio
  by 3.2×, and its 13 is under `gross_channel_delta`, so it scores nothing at
  all on the gross budget. The exposure is the second of those: if a future
  sprite scene puts WARP's edge disagreements past delta 24 on more than 0.1% of
  the frame, D3D12 goes red where the old ratio passed it. Delta 13 on an edge
  texel is a function of the contrast across that edge, not of driver quality,
  so a higher-contrast sprite could plausibly reach it. Nothing measured has.
- **Metal's cube on a paravirtual device** — 2 pixels at delta 207, 0.0041%.
  This is the one legitimate frame that scores on the gross budget at all, and
  it sits 24× under it. At 97×61, the smallest size the gate runs at, that
  budget is five pixels.

Both are pinned by fixtures in `compare.rs`'s tests
(`warps_sprite_edges_pass_and_are_what_the_ratio_is_sized_against`,
`the_worst_measured_cross_backend_frame_still_passes_with_room_to_spare`) that
reproduce the reported per-pixel numbers, so a future tightening argues with a
test. A fixture is not the frame, though: it reproduces the counts and deltas,
not the pixels.

**The one place this is looser than what it replaced.** A frame with between
0.5% and 1% of its pixels off by 3 to 24 levels was refused by the 0.005 ratio
and passes now. That band is empty in every measurement across vk, wgpu, dx12
and metal, and the alternative is leaving WARP 3.2× from a false alarm on the
backend nobody here can debug. Recorded because it is a real trade, not a free
win: the criterion the split had to meet was more room on **both** sides, and
more room on the legitimate side necessarily means a looser drift budget.

**Declined: a budget on `mean_abs_error`**, which is the shape this entry used
to propose and which the data refutes. P1.3's HDR frame is legitimate at 0.2284
mean abs error — 91% of the frame off by one level, a quantisation boundary the
whole background lands on — and the sprite recolour that must fail is 0.0734.
Any total-error budget loose enough for the first passes the second by a factor
of three, whichever way it is normalised, because total error cannot tell a
level spread over the frame from a patch that is badly wrong. Separation has to
be on **per-pixel magnitude**, which is what a second delta threshold does.
Restricting the sum to pixels already over `max_channel_delta` does work — it
separates WARP from the recolour by 16× — but only 4× on each side, which is
under the bar the split had to clear, and it costs a metric nothing else reads.

**Declined: scoring how _localised_ the differing pixels are.** It is the real
physical difference — WARP's 76 are scattered along quad edges, the recolour's
361 are a 19×19 block — and `differing_bounds` already computes a box. It was
not built because metal's legitimate 2 pixels are adjacent, so density does not
separate that pair; because a real bug need not be contiguous; and because it
needs a second traversal or a running per-region accumulator where the two delta
thresholds cost one comparison inside the existing loop. If the gross budget
ever proves too blunt, this is the next idea, not a new ratio.

**Worth keeping from the entry this replaces:** the original derivation was
built from a table of per-backend figures that did not include D3D12's sprite
scene, because that number had never appeared in a log anybody had read. It was
not wrong about the data it had. That is the ordinary shape of a bound
calibrated on the backends that are easy to measure, and the reason the Metal
and D3D12 jobs upload their diffs.

### Re-affirmed: no Vulkan on macOS, and two facts the original decision lacked

`docs/plan/09-backends-metal-dx12.md`'s 2026-08-05 correction made Apple
platforms Metal-only and cancelled the MoltenVK spike. It was reconsidered on
2026-08-11 and **kept**. The plan doc still carries the reasoning; this records
the reconsideration so the question is not opened a third time, and two things
found while costing it that the original argument did not use.

**`crcbl-vk` cannot enumerate a portability driver at all.** There is no
`VK_KHR_portability_enumeration`, no `ENUMERATE_PORTABILITY_BIT_KHR` on the
instance create flags, and no `VK_KHR_portability_subset` handling anywhere in
`crates/crcbl-vk/src/`. Without those, `vkEnumeratePhysicalDevices` returns zero
devices on macOS whether or not MoltenVK is installed. So "install MoltenVK and
it works" was never true — it is a code change first, and a small one, but it
means no macOS Vulkan support exists to accidentally regress.

**MoltenVK runs on Metal, so a macOS Vulkan CI job adds no GPU coverage.** It
would exercise `crcbl-vk`'s portability against the same paravirtual device
`crcbl-mtl` already uses, not a second driver. That is worth something — it
would have tested the capability model's degradation, since MoltenVK has neither
`DRAW_INDIRECT_COUNT` nor `VK_EXT_mesh_shader` — but it is not the independent
coverage a second backend usually buys, and the original decision's cost (two
GPU paths on the platform with the least CI capacity) stands unchanged against
it.

**If it is ever revisited, the tooling question has a trap in it.** `ash-molten`
statically links MoltenVK and would make a bare `cargo build` sufficient, but it
bypasses the Vulkan loader, and with no loader there are no validation layers —
which `crcbl-vk`'s harness asserts the presence of by design, because a suite
that passes for want of a layer proves nothing. The configuration that keeps
that guarantee is the LunarG SDK, which ships MoltenVK, the loader and the
layers together. Downloading any of it from a `build.rs` was considered and is
the wrong mechanism regardless: it breaks `--offline` and sandboxed builds, runs
in every job including the ones that need nothing, and is invisible to the
`cargo deny` gate this workspace already has.

### `a_copy_d3d12_cannot_place_is_refused_by_name` provokes a real layer error

Recorded because it is the one D3D12 test whose validation report is dirty on
purpose, and a future reader will otherwise try to "fix" it.

The refusal it asserts is **D3D12's own**: the seam does not reject a 252-byte
row pitch before the call, so the copy reaches the driver and the debug layer
says so. It calls `defuse()`, exactly as `crcbl-vk`'s gate tests decline to call
`Headless::finish`. The first run with teardown assertions enabled found this
and nothing else across all 75 tests — one deliberate provocation, correctly
flagged.

### Verified, not a problem: the D3D12 info queue does not leak across tests

Recorded so it is not re-investigated. `debug::read_queue` reads from index 0
and never clears, which looks like it would let one test's messages fail
another's teardown. It does not: every device test opens its own `ID3D12Device`
through `device::tests::open_device`, and `debug::attach` clears _that device's_
queue at creation, so a report means "since this device was created" by
construction. Evidence: in run 31454155654, message 597 — the gate's own
deliberate violation — appears exactly once, inside the expected panic of the
test that raises it, and that test passed.

## Bindless is a Vulkan-only path now, so what does P7 owe?

Decision record; the decision is in docs/backlog.md. **Option A — implement
`BindingModel::Bindless` as a second, Vulkan-only path.** Selected by
`caps.binding_model()`, so a WebGPU run keeps `ArrayPages`. Cost: a descriptor
array with a runtime bound, `BindingFlags::VARIABLE_COUNT` and
`BindGroupDesc::variable_count` used for the first time, a per-material index
that is a descriptor slot rather than a layer, a second `mesh.slang` arm, and
two paths that must render the same frame — a new cross-path observable on top
of the cross-backend one. Lifts all three constraints, on Vulkan only. Against:
it is the only place in the renderer where the two active backends would run
structurally different shading code, and the browser — the platform whose
content is most likely to be imported — is the one that does not get it.

**Option B — leave `ArrayPages` and re-scope P7's row.** Strike the bindless
page from P7's owed list, keep the capability declared and exercised by
`hal_seam_e2e`'s `exercise_bindless_descriptor_array` (which is a seam test, not
a renderer path), and say in the row that the renderer is `ArrayPages` on both
active backends by choice. Cost: nothing built; the three constraints stay, so
imported content still has to be conformed to one page. Honest, and it leaves
the engine unable to draw a scene whose textures differ in size.

**Option C — several `ArrayPages`, bucketed by extent/format/mip class.** One
`Texture2DArray` binding per bucket in `mesh.slang`, and
`GpuMaterial::base_color_texture` splits into a bucket and a layer. Lifts the
same three constraints for any content that falls into one of the buckets, needs
no descriptor arrays, and runs identically on both active backends — so the
frame stays one code path and the existing cross-backend goldens keep their
meaning. Against: the bucket count is fixed when the shader is compiled, not at
runtime, so this is "N size classes", not "any texture"; content outside every
bucket still has to be rescaled or re-encoded. The ceiling is the
sampled-texture limit — WebGPU's default `maxSampledTexturesPerShaderStage` is
16 and `mesh.slang` already declares eight — base colour, the shadow atlas,
ambient occlusion, the specular DFG table, normal maps, the LTC matrix table,
contact shadows and probe visibility — which has not been checked against a real
adapter's reported limit, and which makes option C's headroom the narrower half
of the argument rather than an afterthought.

**Not yet measured, and it would decide between B and C:** what the samples and
`crates/crcbl/tests/render_e2e.rs` fixtures actually feed the page today, and
how many distinct extent/format/mip classes an imported glTF scene produces.
Nobody has counted. C is only worth its bindings if that number is small and
stable.

## The host-visible-write rule has now cost two devices, and the seam could enforce it

Record; what is left is in docs/backlog.md under the same heading. `crcbl-dx12`
refuses a shader-written binding that names a host-visible buffer — D3D12's
upload and readback heaps refuse `ALLOW_UNORDERED_ACCESS` at creation, so there
is no UAV of one. The refusal is correct and its message is excellent. It has
now caught the same mistake **twice**: the draw-generation counters, and the LOD
hysteresis state.

**Done.** `NullDevice` refuses a `read_only: false` entry naming a mappable
buffer at `create_bind_group` and `update_bind_group`, and `MemoryLocation`
carries the rule with its D3D12 mechanism. Nothing in the tree violated it, so
there was no third latent instance. Read-only bindings of host-visible buffers
stay legal — removing that exemption fails 28 tests, which is the measure of how
load-bearing it is.

**The image half is closed too**, and it turned out stronger than the buffer
rule: `ImageDesc::memory` has exactly one legal value, because the seam has no
way to touch an image's bytes from the CPU — no `write_image`, no mapping, no
subresource layout — so the field buys nothing observable on any backend while
removing a D3D12 device. **Done — the field is deleted.** Taken as a sane
default under the standing instruction, on `CLAUDE.md`'s rule that a contract is
enforced rather than documented: a field with one legal value is one every
caller must fill and can still fill wrongly, and removing it makes the state
unrepresentable instead of refused. 36 sites, 20 files, no golden moved, and the
run-time refusal added one commit earlier is deleted with it — a guard against
the unconstructable is noise.

## Settled: the `D2Array` page samples on Metal and D3D12

Was an open coverage gap — `SampledImage { view_type }` is dropped by
`crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` (each takes the dimension off the bound
view, and each says so at the arm that drops it), and neither Metal nor D3D12
runs a draw on this machine, so both were type-checked only.

CI confirmed it on `7c4042b`: `golden cube on metal` and `golden cube on dx12`
each came back **max channel delta 1, 0 over tolerance, 0 grossly wrong**
against the lavapipe-blessed golden. Metal is the one that mattered — it is the
only `ArrayPages` device, because it withdraws `Features::DESCRIPTOR_INDEXING` —
and its cube previously carried `max channel delta 207` with 2 pixels grossly
wrong, so agreement went strictly up. Kept only as the record that this was
checked and how; there is nothing owed.

## The command stream's contract, read from the other side

Record; the `MEMORY_*` naming is in `docs/backlog.md` under this heading.

`crates/crcbl-webgpu` encodes the browser command stream and
`web/engine/gpu-stream.js` decodes it. Writing the second implementation against
the first surfaced six things about the contract. Three were fixed in the same
change and are gone from here; these are the three that were not.

### A bad presence byte and a bad enum code report the same error

`read_opt_string` passes its own field name down to `read_present`, so a
malformed presence byte on `BufferDesc::label` surfaces as
`InvalidEnum { field: "BufferDesc::label", code: 2 }` — the same shape a bad
`MemoryLocation` code produces. The two are different defects: one is a
structural framing error, the other a value the far side does not recognise.
Nothing in the docs says which a reader is looking at, and a JS implementer has
to trace two calls to find out.

A distinct `DecodeError` variant for a non-canonical presence byte would say it
plainly. Not done because it widens a public error enum for a case that has not
bitten anyone, and the JS half matched the current behaviour deliberately so the
two agree.

### The JS decoder is hand-written against the Rust tag table

The fixture check catches drift, which is why this is not urgent — but it
catches it rather than preventing it. Generating `gpu-stream.js`'s constants
from `tag.rs` would make a whole class of disagreement impossible instead of
merely detected. Declined for now: it adds a codegen step to a build that has
none, and the failure it prevents already fails loudly in the Pages job on every
pull request. Worth revisiting if the tag table grows to the full surface and
the two tables start being edited in separate sessions.

## `webgpu` is a refusal on native, and the browser's only backend

Record; the two coverage gaps are in `docs/backlog.md` under this heading.

`crcbl::backend::REGISTRY`'s `GpuBackend::WebGpu` entry returns
`WEBGPU_NOT_IMPLEMENTED` on native. On `wasm32` it does not: its `open` starts a
real `crcbl_webgpu::WebGpuInstanceOpen`, and with `crcbl-wgpu` deleted on
2026-08-21 there is no second candidate a build flag could reach, so it is the
browser's only automatic backend.
`exactly_one_backend_is_auto_selectable_and_it_depends_on_the_target` is what
pins that, so a change there is a deliberate edit rather than something that can
drift.

It is registered rather than left out on purpose: an unregistered name yields
`UnknownBackend`, which reads as a typo, where the registered refusal reads as
work not yet done.

### The e2e scripts' backend hints omit `webgpu` deliberately

`crates/crcbl/tests/run-render-e2e.sh` and
`apps/lantern/tests/run-lantern-golden.sh` carry a "Name one:" usage hint
listing the backends that can draw a golden. `webgpu` is not among them and
should not be until it can render. **No script validates backend names against a
whitelist** — every one passes `CRCBL_GPU` and `--backend` straight through and
lets the Rust reject them — so these hints are documentation, not gates, and
nothing fails if they lag.

## The adapter reply is not filtered, though the mapping is now gated

Record; the unfiltered `Instance::adapters`, the probe gate's better fix and the
corroboration gaps are in `docs/backlog.md` under this heading.

`web/engine/gpu-replay.js` maps a browser's `adapter.features` onto
`crcbl_hal::Features` and reports what the browser said. Two different things
could go wrong with that, and as of 2026-08-22 they have different answers.

**Half closed: the mapping can no longer outrun the stream.**
`web/tools/gpu-replay.mjs` gained a section that reads `FEATURE_MAP`'s keys out
of the table itself — `halFeaturesFor` is handed a `Set` whose `has` records
what it is asked and answers `false`, so the walk yields the table's keys in its
own order — and for each key replays the command that feature governs against a
stub device that opened with it, reading back what reached WebGPU. A row added
for a feature whose commands do not exist yet has nothing to drive and fails.
Red-checked both ways: a bogus `shader-f16` row fails naming it, and making
`Replayer#createGraphicsPipeline` drop `unclippedDepth` fails
`depth-clip-control` and prints the descriptor that was recorded. `pages.yml`
runs the suite, so it is a CI gate and not a local one.

`indirect-first-instance` is the weak row and the code says so: WebGPU exposes
no field for it — the feature lifts core's `firstInstance == 0` rule and the
value lives in the indirect buffer — so the evidence is only that the indirect
draws it governs are replayed at all.

**What a runtime filter would still add is narrower than it sounds, measured
2026-08-22.** The new gate drives each mapped feature against a **stub** device
in node and reads the descriptor back, so it answers for the _replayer_. It does
not answer for the Rust encoder: the gate builds the command object in JS
(`{ ...graphicsPipeline, primitive: { ...primitive, depthClamp: true } }`), so a
`crcbl-webgpu` that could not encode the field would still pass it. Each link of
that chain is separately held — `writer.rs` puts the bool, `reader.rs` reads it,
`gpu-stream.js` decodes it — which is why this is a coverage note and not a
defect.

**The instance this named has shipped.** It read: every pipeline in `probe.rs`
sets `depth_clamp: false`, so no Rust-originated command had ever carried
`depth_clamp: true` to a real browser, and the bit was reported to callers on
the strength of a node stub. Group AH now draws a triangle past the far plane
through two pipelines differing only in that flag and reads back that the
clamped one kept its fragments and the control one did not — on SwiftShader
under Xvfb and on the RX 7900 XTX. `probe_device_desc` asks for `DEPTH_CLAMP`
optionally to get a device that can, and the module's parsimony argument is now
an admission test rather than a list.

What that leaves is the general point, which still stands: the JS gate answers
for the replayer, and only a group like AH answers for the wire.
`indirect-first-instance` has since got its group — **AI**, two indirect draws
off argument structures differing only in `firstInstance`, landing a half-target
apart — so **two mapped features are left without one**:

- **`texture-compression-bc` has its group — AK — and what it left owed is a
  branch nothing here can reach.** The group uploads an 8×8 BC1 source as four
  blocks, one per quadrant, every index zero so every texel decodes to its
  block's `color0`, and holds each quadrant against that endpoint byte for byte.
  The endpoints are **cube corners** (`red`/`green`/`blue`/`yellow`), and that
  is the whole design rather than a detail: D3D 11.3 §19.5.2 permits a decode
  tolerance across every channel of every texel — about ±8.65 in UNORM8 against
  a black `color1` — with no carve-out for the endpoints, and mandates bit
  accuracy only for BC6H and BC7. Its one exactness clause is that values the
  reference decodes to 0.0 or 1.0 must always be exact, and the rails are also
  the only place D3D's bit replication and Khronos Data Format 1.3 §18.1's
  rational agree (they part at 5-bit 3, 7, 24, 28 and 6-bit 11–15, 48–52).
  `the_four_bc_sample_colours_are_exact_on_every_decoder` holds the four to both
  formulas and carries a vacuity guard proving the two are different arithmetic.
  Both local adapters decoded them byte-exact.

  **This entry said "SwiftShader does not report the feature" and that was
  wrong** — measured 2026-08-23, its adapter lists `texture-compression-bc` and
  the device opens with it. So **AK's absent branch is taken by no adapter
  available here**, neither SwiftShader under Xvfb nor the RX 7900 XTX. It is
  guarded rather than merely present — a device that drops the feature while the
  adapter keeps it fails the group, and a probe reporting the feature absent on
  a device that can create a BC texture fails it too, so "not supported" cannot
  arrive as "passed" — but the branch itself has run nowhere. Whether the macOS
  or Windows runner reaches it is the open question; if none does, the branch is
  reasoned-about code with no execution anywhere, which is the same standing as
  group AI's absent branch.

  **Answered 2026-08-23 on `ab706a0`'s Pages run: no runner reaches it either.**
  Both seam-probe jobs report `texture-compression-bc` on the device — Windows
  `[core-features-and-limits, depth-clip-control, indirect-first-instance, texture-compression-bc, timestamp-query]`,
  macOS the same without `timestamp-query`. So AK's absent branch is taken by no
  adapter and no runner, and is code reasoned about and never executed. Group
  **AL** is the contrast: macOS lacks `timestamp-query`, so its absent branch
  runs on a real job every time, which makes it the first of these whose absent
  half anybody can watch. Runner images drift, so both readings are dated and
  were read off a CI log rather than reasoned about.

- **`timestamp-query` has group AL, and the sweep it demanded refuted the
  premise this entry gave.** The entry said browsers quantise timestamps for
  privacy, so "non-zero" and "strictly increasing" were unsafe without measuring
  first. The measuring happened on 2026-08-23 and **nothing quantises on either
  adapter here**: SwiftShader's ticks have gcd 1 with gaps as small as 70 ns
  over 288 distinct values, and `amd rdna-3` sits on a 40 ns intra-frame grid
  over 320 — what a 25 MHz counter gives. An empty pass's span was never zero in
  152 samples. Explicitly _enabling_ `timestamp_quantization` changed nothing,
  so the mechanism is not established; `--enable-unsafe-webgpu`, which
  `browserFlags` passes everywhere, is a suspicion and no more. Group AF's
  comment asserted the 100 µs figure and now states what was measured instead.

  The advice survives its own premise: AL gates a **separation**, not a duration
  — every busy pass outspans every empty pass in the same frame, plus a
  non-decreasing boundary array — so no nanosecond constant enters the assertion
  and a quantising browser only widens the gap.

  **The Windows runner is measured now, and it does not quantise either.**
  `14c89de`'s Pages run reported AL's margin there as **155053×** — 8 busy
  passes spanning 527181300–532648300 ns against 8 empty ones at 400–3400 ns. A
  browser quantising to 100 µs would have floored every empty span to zero; none
  did. That is a third independent environment agreeing with the sweep, and the
  rung needs no revisiting. macOS took the absent branch and **asserted** it —
  "opened a device without `timestamp-query`, so no `GPUQuerySet` of that type
  could exist" — which is the branch running rather than being excused.

  The original note, kept for what it still covers: Linux runs SwiftShader and
  macOS takes the absent branch, so Windows is the one job where the present
  branch runs on a device nobody here has profiled. AL's second check prints the
  ratio, so the next Pages run reports it: a margin near 1 means the workgroup
  count wants revisiting, and a quantised one would be the first evidence of
  what quantisation does to this group. Both gcd findings are from one developer
  machine.

AI cost about as much as AH and bought the same kind of witness, so the pattern
is proven; what is unanswered is only whether these two are worth it.

**Every mapped feature is served today**, so nothing is currently misreported
through either half. `TIMESTAMP_QUERY` used to be the standing example and is
not one any more: `crcbl-webgpu/src/command.rs` defines
`Command::CreateQuerySet`, `probe.rs` calls `stream.create_query_set`, and
`gpu-replay.js` has `createQuerySet`.

### Considered and declined

- **`GPUAdapter.isFallbackAdapter` as `DeviceType::Cpu`.** It grades
  _performance_, not device class; a fallback adapter is not necessarily a CPU
  one, and the mapping would put a guess where the honest answer is "declined to
  say".
- **Reporting `max_sampler_anisotropy: 16` and granting `SAMPLER_ANISOTROPY`**,
  the way `crcbl-wgpu` did before it was deleted. WebGPU accepts `maxAnisotropy`
  above 1 but reports no queryable ceiling, and `Limits` is what the backend
  _guarantees_ — 16 would be a number nothing told us.

### Group AI's absent branch has never been taken

Measured on Pages run for `eb8b6b2`, the first run since AI landed whose seam
probes were not cancelled. Every browser this project can reach reports
`indirect-first-instance`: Chromium on Linux locally, the hardware RDNA-3
adapter locally, SwiftShader on the Windows runner and the browser on the macOS
runner all took the **present** branch and produced the pixel verdict. So the
`NotOnThisDevice` half of group AI — the one that passes with a message saying
there is nothing to hold the capability to — is written and unrun.

Not a defect and not obviously fixable: the branch exists because the capability
is optional, and no runner here lacks it. It is recorded because an untaken
branch that always passes is the shape `docs/plan/12-testing.md` warns about,
and because the same branch in group AF **is** taken — the macOS runner opens a
device without `timestamp-query` — which shows the machinery works in general
while saying nothing about AI's copy of it. If a runner ever loses the feature,
this is the entry that says the path was never proven.

## DECISION NEEDED — the seam does not say whether an acquire is exclusive

Decision record; the decision is in docs/backlog.md.

Found while fixing dx12's missing acquire tracking; not a dx12 bug, and not
something to fix on one backend alone.

- **Acquiring twice without presenting is accepted** by dx12, vk and mtl — each
  simply overwrites the outstanding acquire — and by `crcbl-webgpu`, whose
  `acquire_next_frame` in `hal/device.rs` allocates a fresh image and view and
  returns `Ok` with no outstanding-acquire state at all. **The one backend that
  refused it was `crcbl-wgpu`**
  (`"acquire_next_frame with a frame already acquired; present it first"`),
  deleted 2026-08-21 — so the divergence that raised this question is gone and
  every surviving backend now accepts the call. The question it raised is still
  open, and making them refuse would turn dx12's own e2e job red today: the
  windowed loop in `crcbl-dx12/src/swapchain.rs` acquires and then calls
  `draw_and_present`, which acquires again before presenting. Decide what the
  seam means before adding it to `hal_seam_e2e.rs`.

The suite that would hold every native backend to an answer already exists
(`crates/crcbl/tests/hal_seam_e2e.rs`, run by CI on WARP, lavapipe and Metal),
so this is a decision rather than infrastructure.

**The question is answered: accepting a second acquire is right, and refusing it
would have been stricter than every API underneath.** Vulkan permits more than
one image to be acquired at once — that is what lets a mailbox swapchain keep a
frame in flight while the next is drawn — and bounds it by the ring's size
rather than at one; Metal's `nextDrawable` vends from a finite pool and blocks
when it is empty rather than refusing; WebGPU has no acquire to call twice,
since `getCurrentTexture` hands back the same texture for the rest of the frame;
and DXGI has no acquire at all, only a current back-buffer index. A seam that
refused the second call would forbid on all four what three of them offer, and
it would turn dx12's own e2e job red for a loop that is not doing anything
wrong. **So the seam should say an acquire is not exclusive**, and `crcbl-vk`'s
single `entry.acquired` slot — which overwrites, in `acquire_next_frame` in
`crcbl-vk/src/device.rs` — is the thing that does not model the ring rather than
the caller being at fault. Verified in the tree; the per-API statements are from
the specifications and were not re-read for this note.

**Two method lessons from the half that closed** — `acquire` → `reconfigure` →
`present`, now refused by `a_present_without_an_acquire_is_refused` in
`crates/crcbl/tests/hal_seam_e2e.rs` on every backend that suite runs, by
`a_present_without_a_matching_acquire_is_refused` in `crcbl-hal`'s null backend,
and — since `crcbl-webgpu` is a native binary the agnostic suite does not reach
— by `a_present_with_no_acquired_frame_is_refused` in that crate's own
`hal::tests`. Both lessons are about how the work was parked, not about the
code:

- Parking it assumed closing the hole _required_ an agnostic test, which would
  have reddened the two deferred backends. It did not. **"Fixing it needs an
  agnostic test" is the assumption to check before parking anything else on the
  deferral.**
- `crcbl-webgpu` had to record both an outstanding acquire and the fact of
  having presented it, which the other backends keep in one place.
  `SwapchainState::presented` records the second without disturbing the first.
  **Worth remembering as a shape**: "these two facts are stored in one place on
  the other backends" is not a reason they must be here.

## What the indirect-rule wiring left owed (2026-08-24)

The offset, stride and bound rules now live in `crcbl_hal::indirect` and
`crcbl-vk`, the null backend and `crcbl-webgpu` call them. Three things that
slice deliberately did not do.

- **`crcbl-webgpu` does not enforce the bound, by design.** Its encoder holds a
  channel and a handle pool and cannot reach a buffer's length, so it calls
  `check_layout` (offset and stride) and leaves the bound to the browser, which
  validates the indirect range itself and reports it. This is an
  enforcement-location difference rather than a capability divergence, so it is
  deliberately **not** a `parity_blockers()` row — recorded here so it is not
  rediscovered as a gap. Closing it would mean giving the encoder a handle on
  the device's buffer table (the `buffers` map already exists for
  `check_buffer_range`), which is a larger change than the rule was worth; the
  option is real if a reason appears.

## Decision: should `crcbl-webgpu`'s encoder see device state? (2026-08-24)

Decision record; the decision is in docs/backlog.md. The 2026-08-30 answer
recorded here was superseded on 2026-09-06 by option B.

**DECIDED 2026-08-30 — option A, leave it:** the browser validates the range and
the offsets itself; the seam's asterisk stays recorded here.
`WebGpuCommandEncoder` holds a `SharedChannel` and a `HandlePool` and nothing
else. The device holds the tables — `buffers`, `layouts`, `images`, `swapchains`
— so any rule that needs to know something about a resource can be checked in a
`Device` method and **cannot** be checked in an encoder method. That has now
blocked two seam rules, which is what makes it a decision rather than a quirk:

**Option A — leave it.** The browser validates the indirect range itself and
reports a `GPUValidationError`, and it validates dynamic offsets too, so nothing
is unchecked in the end; it is checked late, by a different party, with a
different message. Cost: two seam rules whose answer on this backend is "the
browser will tell you", and an asterisk on any claim that the seam's rules are
enforced uniformly.

**Option B — give the encoder a handle on the device's tables.** An
`Arc<DeviceState>` holding the existing maps, cloned into the encoder at
`create_command_encoder`. Closes both rules and any future one, and matches what
`crcbl-vk` does (its encoder reaches `self.device.state()` freely). The real
cost is **a mutex acquisition per recorded call on the hot path** — `bind_group`
and `draw_indirect` run per draw, per frame — where today the encoder touches no
shared state at all. An `RwLock`, or sharding per table, would soften it;
measuring it before choosing is the honest route, and the browser gate's
frame-timing numbers are where that would show.

**Option C — check in the replayer instead.** `web/engine/gpu-replay.js` sees
every resource. But `crates/crcbl-webgpu/src/writer.rs` already warns in its own
comments that splitting enforcement between the encoder and the replayer is
where "an unenforced rule both sides assume the other checks" hides, and this
would put the seam's rules in JavaScript, in a second copy, in a language where
nothing type-checks them against `crcbl_hal`. Recorded so it is not re-proposed:
this is the option to avoid.

**What I would pick, and why it is not simply done:** B, if the per-call lock
measures as noise — it is the only option that makes the seam's rules mean the
same thing on every backend, which is the whole point of the exhaustive
`Capability` enum and the agnostic suites. It is not done here because it is a
change to how every recording method on that backend works, it wants a
measurement first, and it is not what either of the two slices that hit the wall
was about. **Needs the user's call**, or an explicit decision to measure.
