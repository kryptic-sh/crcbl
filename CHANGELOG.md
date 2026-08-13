# Changelog

All notable changes to this workspace are recorded here, in
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) form. Versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html); below 1.0 a breaking
change bumps the minor.

Started partway through the project, so it covers changes from this point on
rather than the whole history — `git log` is the record before it. There are no
tags yet, so everything so far is unreleased.

Internal churn a release note would not mention — refactors with no outward
effect, test-only and docs-only changes, CI repairs — is deliberately left out.

## [Unreleased]

### Breaking

- **`crcbl_shaders::mesh::GpuMaterial` gained `metallic: f32` and
  `roughness: f32`, and `mesh.slang` shades with one GGX lobe driven by them.**
  Anything building a `GpuMaterial` literally has to name the two fields
  (`..GpuMaterial::UNTINTED` supplies them). `MATERIAL_STRIDE` is **unchanged at
  32** — both went into padding the row already had — so nothing that writes the
  table at a stride changes.

  `GpuMaterial::UNTINTED` is no longer "every factor `1.0`": it is
  `metallic 0.0, roughness 0.5`, an ordinary painted surface, because a lobe is
  evaluated rather than multiplied by and there is no neutral pair. Half is
  roughly where a Blinn exponent of 32 sat, so the shading a scene already had
  is the shading it keeps.

  `mesh.slang`'s `SPECULAR_POWER` and `SPECULAR_STRENGTH` are **deleted**. The
  lobe is Cook-Torrance — Trowbridge-Reitz `D`, Smith height-correlated
  visibility, Schlick Fresnel, Lambert diffuse — with
  `F0 = lerp(0.04, base_color, metallic)` and a diffuse albedo of
  `base_color * (1 - metallic)`. **A fully metallic surface therefore has no
  ambient term and is black until it has something to reflect**; screen-space
  reflections and irradiance probes are the two rows that give it one, and
  `docs/plan/18-render-features.md` is where that is argued. Every 3D golden
  moved; `sprite` and `ui` are byte-identical.

- **`crcbl_scene::gltf_import` fills both new factors, so an imported default
  material is no longer `GpuMaterial::UNTINTED`.** `metallicFactor` and
  `roughnessFactor` come off the same `pbrMetallicRoughness` accessor the base
  colour already did. glTF defaults a material to `metallic 1.0, roughness 1.0`
  — a fully rough conductor — where the engine's neutral row is a dielectric at
  half roughness, and the importer reports the document rather than the engine's
  preference. Callers that relied on the old equality have to name the
  specification's defaults instead.

- **`FrameUniforms` lost `light_direction`, `light_color` and `lod_params`.**
  The sun is a row in the light list now rather than a field, and `lod_params`
  was already dead — documented in-tree as "read by no shader since hysteresis
  landed, and written all the same". It gained `cluster_grid` and then
  `light_view_proj`, and is 656 bytes. Anything constructing `FrameUniforms` or
  reading those fields has to change.

- **`GpuLight::pad0` became `shadow_tile`, and `Light::row` takes it.** It names
  the first atlas tile the light occludes through — one tile for a spot, the
  first of six for a point. A row's default is no longer all-zero:
  `NO_SHADOW_TILE` is `u32::MAX`, because zero is a real tile and a row that
  forgot to say would occlude through whichever light holds it.

- **`BindingKind::SampledImage` gained `sample_type` and `BindingKind::Sampler`
  became a struct variant with `comparison`.** Every construction has to name
  them — `SampleType::Float` and `comparison: false` reproduce the old behaviour
  — and every `match` on `Sampler` becomes `Sampler { .. }`.

  A shadow map needs both and neither was expressible: WebGPU takes the sample
  type and the sampler's comparison mode in the **layout**, so a `D32Float` view
  bound as `Float { filterable: true }` is refused at pipeline creation whatever
  the sampler does. This is the gap `docs/backlog.md` predicted when `view_type`
  closed the dimension half. `crcbl-wgpu` consumes both; Vulkan, Metal and D3D12
  read them off the sampler and the view and each says so where it drops them.
  **The wgpu suite is the only local gate on this** — the other three would not
  have noticed a mistake.

- **`crcbl_hal::CommandEncoder` gained `draw_mesh_tasks_indirect`**, so anything
  implementing that trait outside this workspace has a new method to write. It
  takes the existing `DrawIndirect`, and the argument buffer holds **three
  consecutive `u32`s** — group counts x, y, z — 4-aligned, tight stride 12,
  `draw_count > 1` gated on `MULTI_DRAW_INDIRECT`. `DrawIndirect`'s own docs now
  say which structure each of the three indirect calls reads.

  Vulkan maps it to `cmd_draw_mesh_tasks_indirect`; the null backend records it;
  `crcbl-wgpu` returns `Unsupported` naming that WebGPU has no mesh stage; Metal
  and D3D12 refuse it exactly as they already refuse `draw_mesh_tasks`. **No new
  `Features` flag** — `VK_EXT_mesh_shader` defines both entry points together,
  D3D12 mesh tier 1 admits the `DISPATCH_MESH` signature, and a Metal device
  with mesh functions has the indirect draw, so no API offers one without the
  other.

- **The mixer remembers a listener, so a cue no longer carries one.**
  `crcbl_audio::spatial::Listener` is new — `#[non_exhaustive]`, built through
  `Listener::new(position)` — and `Mixer` gained `set_listener`, `listener` and
  `cue(emitter, grammar)`. `compute_cue` is unchanged and still takes an
  explicit listener: it is a pure function, and `Mixer::cue` is what supplies
  the remembered one.

  It exists because the engine had no listener at all, which left every game
  inventing where the ear was: the four samples spelled the same call three
  different ways — `play_panned(id, emitter_x)`,
  `play_at(id, listener_x, x, y)`, `play_at(id, x, y)` and
  `play_at(id, listener, at)`. All four are now `play_at(id, world_position)`,
  and each sample's listener convention is one `set_listener` line at the right
  point in its frame instead of a parameter on every cue: breakout and asteroids
  place theirs once in `Audio::new` because their camera never moves, flappy
  pushes one axis per tick, and horde reads the player's position under the same
  lock as the cue queue, so a cue raised on a tick is heard from where the
  player was on that tick.

  `Listener` is a type rather than three floats for a specific reason:
  `compute_cue` derives azimuth as the angle from +Z, which assumes the
  listener's orientation is fixed. A listener that can turn needs a forward
  vector, and that is the field this type exists to gain without breaking
  callers.

  With no listener set, cues are heard from `Listener::ORIGIN` — a real place,
  readable back through `Mixer::listener()`, not a sentinel. Refusing to cue
  until one arrives would have fired on the two samples that are _right_ to set
  theirs once and never touch it again.

- **`crcbl_render::Sprite` is `#[non_exhaustive]` and is built through
  `Sprite::new(sheet, rect, uv)`.** A struct literal from outside `crcbl-render`
  no longer compiles, and neither does `..base` functional update; `rotation`
  and `tint` are `with_rotation` and `with_tint`, both `const` and both
  returning the sprite. The fields stay `pub` and are still readable — `Sprite`
  has no invariant to protect, so this is about construction only.

  It exists because every new field was a breaking change to every caller:
  adding `rotation` broke nine literals that had nothing to do with turning, and
  the sample count is going up. The next field is now a non-event for anything
  outside the crate. The measurement behind the split, over all 34 construction
  sites: `sheet`, `rect` and `uv` are set by every one of them, while `rotation`
  is non-zero at five and `tint` non-white at five.

  `new` takes two adjacent `[f32; 4]`s, which is the argument-swap hazard
  `SheetDesc`'s own documentation names and the compiler cannot see. What
  catches it is the instance-layout test asserting `rect` at byte 0 and `uv` at
  byte 16 from distinct values, and the sprite golden frames — a swap inside
  `new` reds seven unit tests and
  `the_sprite_scene_draws_through_the_sprite_renderer_and_matches_its_golden`.
  Call sites remain on their own, and `new`'s docs say so.

- **`crcbl_hal::BindingKind::SampledImage` is now a struct variant carrying the
  view dimension**: `SampledImage { view_type: ImageViewType }`. Every
  construction has to name it (`ImageViewType::D2` reproduces the old
  behaviour), and every `match` arm has to become `SampledImage { .. }`.

  It exists because WebGPU takes the dimension in the **layout** rather than off
  the bound view: `wgpu::BindingType::Texture` has a `view_dimension`, and a
  layout that says `D2` while the view is `D2Array` is refused at pipeline
  creation with "expects dimension = D2, but given a view with dimension =
  D2Array". `crcbl-wgpu` used to hardcode `D2`, which was invisible while every
  sampled binding in the engine was a `Texture2D` and became a build failure the
  moment `mesh.slang` declared a `Texture2DArray`. Vulkan, Metal and D3D12 all
  read the dimension from the view and ignore the field; each backend's
  conversion says so where it drops it.

- **A material now carries a base-colour texture, and the vertex and material
  layouts both grew.** `crcbl_shaders::mesh::GpuMaterial` gained
  `base_color_texture: u32`, so `MATERIAL_STRIDE` is 32 rather than 16 and
  anything building a `GpuMaterial` literally has to name the field
  (`..GpuMaterial::UNTINTED` supplies it). `MeshVertex` gained `uv: [f32; 4]`,
  so `VERTEX_STRIDE` is 64 rather than 48 — every consumer in this workspace
  uses the constant, but a producer of vertex bytes that did not would now write
  short rows.

  `mesh.slang` gained binding 7 (`Texture2DArray`) and binding 8
  (`SamplerState`), both visible to the vertex and fragment stages for the
  reason binding 6 already was. **Any caller that builds its own bind-group
  layout for that module must add both**, because a pipeline layout that does
  not cover a binding the module declares is refused outright.

- **`crcbl_golden::Tolerance` gained `gross_channel_delta` and
  `max_gross_ratio`, and `Comparison` gained `gross_pixels` and `gross_ratio`.**
  Anything constructing a `Tolerance` literally has to name the two new fields;
  `Tolerance::EXACT` and `Tolerance::RASTERISER` are unchanged as names and
  every consumer in this workspace uses those. `Failure` gained a
  `TooManyGrossPixels` variant, so a `match` over it that was exhaustive is not
  any more, and `Comparison::summary()`'s line gained an
  `N grossly wrong (X.XXXX%)` field between "over tolerance" and "mean abs
  error" — a script parsing that line by position has to move.

  The comparator now scores **two** questions instead of trading one ratio
  against both. `max_failing_ratio` bounds how much of the frame may drift past
  `max_channel_delta`, and `max_gross_ratio` bounds how much may be past
  `gross_channel_delta`, out where drift does not reach. A driver that disagrees
  about many pixels slightly and a bug that gets a few pixels badly wrong are no
  longer measured against each other.

  This is what `Tolerance::RASTERISER` is now made of: `max_channel_delta: 2`
  and `max_failing_ratio: 0.01` for drift, `gross_channel_delta: 24` and
  `max_gross_ratio: 0.001` for defects, `min_ssim: 0.99` for structure. Every
  one is measured. A plainly visible sprite recolour — 361 pixels of a 256×192
  frame at delta 40, 0.7345% — used to pass a comparator whose only count-based
  bound was 2% of the frame; a single ratio tightened to refuse it had to sit
  between that recolour and WARP's legitimate sprite edges (76 pixels at delta
  13, 0.1546%), leaving 3.2× of room on one side and 1.47× on the other. Split
  in two, the same three frames have **6.5×** (WARP, on the drift budget),
  **7.3×** (the recolour, on the gross budget) and **24×** (metal's cube, 2
  pixels at delta 207, on the gross budget). The one band that loosens is
  0.5%–1% of a frame off by 3 to 24 levels, which nothing measured on any
  backend has ever occupied.

- **`mesh.slang` gained a seventh binding: the material table.** Binding 6 is a
  read-only storage buffer of `crcbl_shaders::mesh::GpuMaterial`, indexed by
  `GpuInstance::material` in the **fragment** stage, which the vertex stage
  reaches by handing it a `nointerpolation uint material : TEXCOORD0` varying.
  Anything building its own bind group or bind-group layout for that shader has
  to name it **and make it visible to the fragment stage** — a pipeline layout
  that does not cover a binding the shader declares, or covers it for the wrong
  stage, is refused at pipeline creation — and anything asserting the shader's
  declared registers gains one `Srv`. `GpuInstance::material` therefore stops
  being a reserved field: an instance now has to carry the id of a material that
  exists, because an unwritten table row is a base colour of zero and shades
  black.

- **`ShaderModuleDesc::dxil` is a list of `(entry point, container)` pairs**,
  `&[(&str, &[u8])]`, where it was `Option<&[u8]>`. A DXIL container holds one
  entry point, so a module drawing with a vertex and a fragment stage now offers
  a container for each and stays **one** module on every backend — where the
  alternative was one descriptor per stage, which would have made the three
  backends that carry every entry point in one artifact compile it twice.
  Absence is the empty slice, so `dxil: None` becomes `dxil: &[]` and
  `dxil: Some(bytes)` becomes `dxil: &[(entry_point, bytes)]`. `crcbl-dx12`
  picks the container named by the stage's `ShaderEntry::entry_point` and
  refuses by name when the module was given none for it.

- **`mesh.slang` gained a sixth binding and `DrawConstants` changed meaning.**
  Binding 5 is the per-bucket run of surviving instance indices the vertex stage
  now reads its instance out of, and `DrawConstants::base_instance` is
  `DrawConstants::base`: where this draw's run starts, not which instance it is.
  A caller building its own bind group for that shader — `crcbl-vk`'s depth
  probe is the only one — has to bind a run and pass a base. The byte layout is
  unchanged.

- **`ForwardRenderer::set_pyramid(None)` removes the instance** rather than
  skipping a draw. An instance in the pool is an object in the scene now that
  culling decides what draws, so hiding an object and culling it off screen take
  the same path out of the frame.

### Changed

- **`ImageDesc::memory` is gone.** Images are device-local, and the type says so
  by not having the field rather than by refusing the other values at run time —
  `CLAUDE.md`'s own rule, that a contract is enforced rather than documented,
  and a field every caller must fill and can still fill wrongly is the weaker
  form of it. 36 construction sites lost a line; the only one that was not
  `DeviceLocal` was the D3D12 test asserting the refusal, which went with it.

  The refusal added a commit earlier is deleted along with its test, and so is
  `crcbl-dx12`'s internal check — a guard against a state that can no longer be
  constructed is noise. `crcbl-vk`'s `create_owned_image` lost its location
  parameter entirely; Metal and D3D12 now name the one location at the call
  rather than forwarding a field, keeping the mapping shared with buffers; wgpu
  needed no edit because it never read it. `BufferDesc::memory` is untouched and
  still uses all three locations.

- **An image is always `DeviceLocal`, and the seam says so now.** `crcbl-dx12`
  refused any other setting at `create_image` and the seam's doc said only
  "almost always", so a caller could write code that worked on three backends
  and removed the device on the fourth. The null backend refuses it now, with
  the mechanism documented: D3D12's `UPLOAD`/`READBACK` heaps admit
  `D3D12_RESOURCE_DIMENSION_BUFFER` only, so a host-visible texture is not slow
  — it is uncreatable.

  **This is stronger than the buffer rule, not the same shape.** That one
  forbids a combination and leaves host-visible buffers legal elsewhere; this
  forbids the _value_, leaving `ImageDesc::memory` one legal setting. What
  decides it is that the seam has no way to touch an image's bytes from the CPU
  at all — there is no `write_image`, no mapping, no subresource layout — so the
  field buys a caller nothing observable on any backend while reliably removing
  a D3D12 device.

  Measured rather than assumed, on real hardware: Vulkan _accepts_ a
  host-visible image on radv and lavapipe, but `crcbl-vk` hardcodes optimal
  tiling, so what you get is an optimal-tiled image in host-visible memory —
  allocated, legal and useless, since `vkGetImageSubresourceLayout` is defined
  only for linear tiling. Metal honours the ask and is equally unreachable
  through this seam. **wgpu does not read the field at all** —
  `wgpu::TextureDescriptor` has no member for it — so it is the one backend that
  would silently mis-honour rather than refuse.

  The "almost always" hedge was covering nothing: of 58 `ImageDesc`
  constructions in the tree, the only non-`DeviceLocal` one is the D3D12 test
  asserting the refusal.

- **A buffer a shader writes must be `DeviceLocal`, and the seam says so now.**
  D3D12's upload and readback heaps refuse `ALLOW_UNORDERED_ACCESS` at creation
  and pin the resource to a state a shader cannot write from, so there is no
  unordered access view of one — and that rule lived only in `crcbl-dx12`, where
  a caller reading the seam could not find it. It has cost a D3D12 device twice.
  `MemoryLocation` documents it with the mechanism, `BufferUsage::STORAGE` and
  `BindingKind::StorageBuffer::read_only` point at it, and the **null backend
  refuses it** at `create_bind_group` and `update_bind_group`.

  **This is deliberately stricter than Vulkan and Metal**, which both permit it
  and where it can be a real optimisation on unified memory. The seam exists so
  that code working on one backend works on all four, and this particular
  divergence does not degrade — it removes the device. If host-visible shader
  writes are ever wanted they are a `Features` flag with a documented fallback,
  not a silent per-backend difference.

  **Read-only storage bindings of host-visible buffers are untouched**, which is
  how every uniform and read-only table in the engine works — dropping that
  exemption fails 28 tests across the sample crates, which is what says the
  carve-out is load-bearing rather than decorative. Nothing in the tree violated
  the new rule: the two devices it cost were already fixed.

- **Per-cluster culling now skips work, not just output.** The mesh dispatch was
  CPU-bounded at `(cluster_count, slot_count, 1)`, so a rejected cluster still
  had its workgroup launched and returned early. `draw_gen.slang` writes a
  per-bucket `MeshTasksArgs` — x from a new host-uploaded cluster table, y
  accumulated by the same atomic that fills `instance_count`, z one — and the
  forward pass records `draw_mesh_tasks_indirect` against it, so the extents
  come from GPU memory the cull pass wrote.

  The extents could not ride the existing draw-argument structure: `crcbl-wgpu`
  refuses a padded stride for `draw_indexed_indirect_count`, and the mesh path
  reads those arguments as a shader read in the same pass, where a resource has
  exactly one `ResourceState`. A second buffer was the way through.

  Proven by readback rather than by picture, since a golden cannot see a
  workgroup that was not launched: the box's bucket goes **1 → 0** when it
  leaves the scene while the cube's stays 1 — which a pool-sized extent cannot
  do, because `slot_count` never shrinks. Pinning the extent to the pool
  capacity instead gives `[16385, 16384, 16385]` and reds the test.

- **Bind-group-layout validation is one function on the seam, not five different
  ones.** `BindGroupLayoutDesc::check_entries(caps, backend)` and
  `BindGroupLayoutEntry::resolved_count(limits)` are new on `crcbl-hal`, beside
  the rules they enforce, and every backend calls them: `crcbl_vk::pipeline`'s
  `validate_bind_group_layout` and `layout_binding_count` are gone, as is the
  null backend's inline copy and `crcbl-wgpu`'s, and `crcbl-dx12`'s
  `check_entry` keeps only its root-descriptor rules. The rule was stated once
  and enforced four times with the wording, the coverage and the error types all
  drifting between them — a duplicated-binding refusal that named the binding in
  three backends and not in the fourth, a descriptor-indexing check two of five
  did not make, and two backends that silently ignored the count ceiling.

  Callers see the same message for the same mistake on every backend now.
  Notably the `VARIABLE_COUNT` rule reports **which** half failed — "not the
  last entry" and "not the highest-numbered binding" are separate messages, as
  D3D12 already did — where three backends emitted one sentence covering both
  and left the reader to work out which to fix.

- **Every backend now holds Vulkan's line on validation, and the tests assert
  it.** `crcbl-dx12`'s device tests assert a clean D3D12 debug-layer report at
  teardown, with warnings counting as failures and an **absent** layer failing
  rather than passing — `CRCBL_DX12_VALIDATION=0` is the opt-out for a machine
  without Windows' Graphics Tools. `debug::diagnosis` no longer clears the info
  queue, so an error quoted inside a `HalError` is the same one that fails
  teardown instead of consuming it.

  `crcbl-mtl` gained a validation report asserted at every device test's
  teardown, and it is **weaker than the other two by nature, not by omission**:
  Metal has no queryable validation channel, so it asserts that the debug layer
  interposed on the device and that no command buffer ended in
  `MTLCommandBufferStatus::Error`. An API misuse aborts the process rather than
  being reportable. `CRCBL_MTL_VALIDATION` is its requirement flag.

  Also fixed underneath it: a failed `MTLCommandBuffer` reported through nothing
  but its own `status`, so a submission nobody waited on failed in total
  silence. Failures are now tracked per submission and logged as errors.

### Added

- **`crcbl_render::scene` and `ForwardRenderer::with_scene`: the resident set is
  a description now, not something the renderer uploads to itself.** `SceneDesc`
  — `meshes` + `materials` + `page` + `capacities` — is host-side data with no
  device in it, so it can be built and compared with no GPU in the room.
  `MeshDesc::geometry` is `Geometry::Flat` (vertex bytes, indices, cooked
  clusters) or `Geometry::Dag` (a `ClusterDag` plus a vertex array per level);
  `PageDesc` owns layer 0 so the white texel a material naming no texture
  samples cannot be got wrong; `Capacities` is what the `POOL_*` constants were,
  with `Default` at the numbers the engine shipped.

  `ForwardRenderer::new` keeps its exact signature and is
  `with_scene(&scene::demo())`, so **every existing caller is untouched and no
  golden moved** — the demo scene is a caller of the API rather than a special
  case inside the renderer. `scene::demo` is the cube, the pyramid, the open box
  and the dunes DAG with the three material rows and two page layers the golden
  suite reads.

  A description's **order is load-bearing**, and the module docs say so at the
  four places it decides a frame: material row 0 is what an instance written
  without a material id shades through, mesh table ids come from upload order,
  page layer 0 has to decode to `1.0`, and the bucket table is one bucket per
  description mesh — built by walking that list, so `draw_gen.slang`'s
  first-match scatter cannot be given two buckets naming one mesh. A description
  that cannot be made resident is refused as `HalError::InvalidDescriptor`
  **before the first device object exists**, so a rejection leaks nothing.

  Instances are not a runtime API yet: the five `set_*` methods and
  `begin_frame` still name description meshes and rows by position, and
  `with_scene` refuses a description shorter than they need. That, and
  `Geometry::Dag`'s documented limitation that `crcbl_scene::simplify` is
  position-only so a coarse level's attributes are the caller's to supply, are
  in `docs/backlog.md`.

- **`crcbl screenshot --scene` reaches every scene the engine draws.**
  `crcbl::screenshot::Scene` has had nine variants for a while and the CLI
  parsed three of them, so `dunes`, `lights`, `spot`, `spot_shadow`,
  `point_shadow` and `ao` — every 3D lighting scene the render e2e blesses a
  golden for — could not be rendered by hand at all. Each name is now the
  **golden's file stem**, so a frame taken at any size and the 256×192 one CI
  compares are reachable by the same word. An unknown name is still exit 2
  rather than a silent fall back to the cube, and `--help` now lists them; a
  test asserts the help text names every scene the parser accepts, and
  `scene_name`'s match is exhaustive so a new variant stops the crate compiling
  until it is named.

- **Screen-space ambient occlusion**, the rasterised twin's AO row. A depth
  prepass — driven by the existing depth-only pipeline with the camera's own
  bind group and draws, so no new pipeline or shader — feeds an `ssao` pass that
  reconstructs normals from depth and takes eight hemisphere samples, a `4x4`
  blur weighted by view-space depth, and a texel fetch in the forward shader
  that multiplies `frame.ambient.rgb` **alone**. Darkening the tonemap's input
  instead would have darkened direct light and highlights, which is what the
  plan's one-line row invited and what it now refuses in writing.

  **The rotation comes from a sixteen-entry constant table indexed by
  `pixel.xy & 3`, never a float hash, and the blur is not optional.** Each AO
  sample is a binary depth comparison, so one landing on the threshold resolves
  differently on two drivers and swings that pixel by an eighth — far past the
  golden tolerance. Noise functions amplify float differences by construction;
  an integer index into a constant array is bit-identical by inspection, and the
  blur's footprint is exactly the noise tile.

  **The blur weights each tap by how far its view-space depth is from the
  centre's**, because a box kernel averages a foreground pixel's occlusion with
  a background that is not the same surface — and the far plane is written
  "fully unoccluded", so every silhouette carried a bright fringe one kernel
  deep. It unprojects through the same `SsaoParams` buffer the occlusion pass
  writes, so there is no second uniform block and no new knob: the tolerance is
  derived from the AO radius, half weight at one radius and none at two. The
  weight is a linear ramp rather than a threshold, since a binary test on the
  output pixel is the same driver-disagreement hazard the constant rotation
  table exists to avoid. The consequence to know about is that the sixteen-tap
  divisor is now sixteen only where every tap counts — full strength on a flat
  surface, weaker exactly at a silhouette.

  **The check is a structural ratio, not the golden**: a band inside a concave
  corner against a band on the same surface at the same camera distance, because
  an AO pass writing a constant 1.0 draws a perfectly plausible frame. The blur
  has one of its own — the plain pyramid's underside in the cube frame, whose
  pixels are the ambient term alone, must not brighten along the edge the clear
  stands behind. AO is always on, and its off-switch is a 1×1 white texture
  rather than a shader permutation. `ao`, `cube`, `lights`, `dunes`,
  `spot_shadow` and `point_shadow` were re-blessed; `spot`, `sprite` and `ui`
  are unchanged to the pixel, and `spot` staying so is what says the term is
  contact occlusion rather than a global scale.

- **Flappy and breakout can be paused with a finger, and a second finger can
  work a menu while the first holds a control.** Pause is the loop's rather than
  a game action and its menu is the only tappable route to fullscreen and the
  debug panel, so a phone could previously start a run and never stop it.
  `crcbl::engine::PauseControl` is shared across the three demos with touch —
  size, corner, palette, appear-condition and hit-test are one piece of
  knowledge, and it owns the extent so a sample needs no pixel conversion.

  The lockout fix landed in the menu's hit-testing: contacts are a second device
  driving the same widgets, the way the activate key already is. The contact
  carrying the emulated pointer is skipped, without which a one-finger tap fires
  twice. Contacts are now delivered before the pointer, because a sample cannot
  say "that pointer press belonged to my control" until it has heard about the
  finger — and the finger pressing pause _is_ the emulated pointer, so without
  that it flapped in flappy and served in breakout on the way to pausing.

  A control the panel took away also re-grabs on its next move now, instead of
  needing the thumb lifted and landed again.

- **Horde plays on a phone: a floating stick and a PAUSE button.**
  `crcbl_ui::touch`'s `TouchStick` and `TouchButton` are widgets acting as a
  virtual device, and `Binding::Virtual` is how `ActionMap` binds them — the
  same table row a key sits in, so horde's `move` action is one `Axis2` bound to
  WASD, the arrows and `Virtual("stick_move")` together. A stick's deflection
  lands in the same accumulator the `Wasd` composite uses, so a key and a thumb
  sum inside the unit disc rather than to twice the speed.

  The stick appears where the thumb lands rather than at a fixed corner, because
  every fixed position is wrong for some grip and a floating origin reads
  exactly zero on the frame the finger arrives. A second finger on a held
  control is refused and offered to the next one. Controls appear once a contact
  has arrived — not on `ShellCaps::TOUCH`, which a desktop touchscreen also sets
  — so a desktop player sees nothing change.

  Pause came with it because pause is the loop's rather than a game action, so a
  phone could otherwise start a run and never stop it, and the pause menu is the
  only tappable route to fullscreen and the debug panel. `HostedGame` gained
  `take_pending_pause`.

- **The seam carries multiple contacts.**
  `ShellEvent::Touch { contact, phase, position }` with `ContactId` and
  `TouchPhase`, routed to a new `HostedGame::touch_event`. The web shell stops
  throwing away every contact but the first — there is somewhere for them to go
  now.

  **A touchscreen produces both streams**: every contact as `Touch`, and the
  primary contact additionally as the emulated pointer, which is the browser's
  own compatibility rule and is now an obligation on any backend setting
  `ShellCaps::TOUCH`. A game bound only to `Binding::MouseButton` therefore sees
  exactly what it saw before. A contact id is unique among contacts that are
  down together and reused after one ends, so state keyed on one must be dropped
  when it ends. `Cancelled` is not `Ended`: the system took the gesture, so the
  position is the last one the platform knew rather than a place anyone chose,
  and a consumer undoes rather than commits.

  No desktop backend implements touch and none claims to — `caps.rs` names the
  path each would have to write and says the bit is clear because the code is
  not written. `Pending` is no longer `Copy`.

- **Flappy and breakout play on a touchscreen.** Flappy taps to flap; breakout's
  paddle follows a finger and a tap serves. `Binding::PointerPosition { axis }`
  is new and feeds an `Axis1` normalised to the surface at −1…+1 — not an
  `Axis2`, which would put a _place_ in the same value shape as
  `Binding::Wasd`'s _direction_. Within one action an absolute binding replaces
  the relative ones rather than summing, because a place plus a rate is neither.

  **`HostedGame::pointer_event` is new, and it is why none of this worked
  before.** Touch reached the shell, but the loop swallowed every pointer event
  — `Pending::observe` returned `Handled::Loop` for `ShellEvent::Button` and a
  game could only be handed keys, so no pointer binding could ever have fired.

  The pointer wins on the tick it moves and the keyboard owns every other tick,
  so arrow keys still work with a cursor over the field; a pointer that leaves
  keeps its last position, which is what stops the paddle walking to the middle
  on every tap. The canvas takes `touch-action: none`, without which the browser
  claims the gesture mid-drag.

  **Horde stays keyboard-only** — a movement stick needs on-screen controls and
  real multi-touch — and asteroids is excluded on purpose: three concurrent
  controls have no phone layout better than the keyboard one.

- **The culling stats come back off the GPU, so the culling win is visible.** A
  ring of `HostReadback` buffers, one per frame in flight plus one, fed by a
  copy the render graph schedules and resolved only when a slot comes back round
  — the shape `PassTimers` already uses, and for the same reason: the latency
  _is_ the synchronisation, so there is no fence, no `wait_idle` and no poll
  loop. `instances drawn` and `clusters drawn` are numbers now instead of
  `indirect`, and a new `cull frame` row says which frame they came from.

  **`RenderGraph::add_copy_pass` and `PassKind::Copy` are new**, and were
  unavoidable: the seam allows a copy only outside a pass scope, and every
  existing pass kind opens one. So a copy could not be a convention about what a
  compute body may do — it had to be a kind whose body runs with no scope open.
  `GraphError::AttachmentInComputePass` became
  `AttachmentOutsideRenderPass { kind, .. }` to cover both.

  Only the camera's cull is read. A cascade's survivors answer a different
  question about a different frustum, and summing them would produce a number
  larger than the instance count. A device that refuses the readback reports
  nothing rather than zero, and the cluster word — written by the amplification
  stage, so absent on three of the four ways the engine draws — reads `unknown`
  rather than `0` where nothing counted it.

- **One place a frame's draws and instances are counted.**
  `crcbl_render::FrameCounters`: each renderer answers with its own record and a
  caller sums them, the same shape the timed-pass bound already uses. The debug
  panel gains a `counters` section, and the numbers are sampled onto the trace
  as `crcbl_core::trace` counters — which nothing had done until now.

  **Two of the plan's counters are deliberately absent rather than
  approximated.** Instances drawn and triangles read `indirect` wherever a
  `ForwardRenderer` is in the frame, because the culling survivor count lives in
  a device-local buffer that nothing copies back: the readback the plan listed
  as already existing does not exist. A triangle count derived from a cluster
  count and a nominal triangles-per-cluster would look authoritative and be
  wrong. Clusters drawn and the level histogram are absent for the same reason,
  plus one more — that word is written by the amplification stage, so it is
  blank on three of the four ways the engine draws.

  `GameGpu::counters` has no default implementation: a bundle that forgot it
  would otherwise put `draws: 0` on the panel, which is "not counted" arriving
  as "nothing drawn".

- **The frame loop is instrumented, and the debug panel answers "am I
  GPU-bound?"** Six spans across `Loop::frame` — `frame` around `input`, `pace`,
  `tick`, `draw` and `present`, with `present-wait` nested inside the last — and
  a `budget` section showing CPU and GPU frame time as p50/p95 over a rolling
  120-frame window with which of the two is the budget. `CRCBL_TRACE` turns the
  profiler on the way `CRCBL_LOG` turns on logging, so it needs no rebuild.

  **CPU frame time is the frame span less the spans the loop spent blocked** —
  `pace` and `present-wait`. Including them would make the row read as the
  display's period on every machine under vsync, exceed the GPU total whatever
  the GPU was doing, and answer "CPU-bound" to a question it never looked at.

  The two halves are distributions over their own windows, not a pair: the GPU
  report is frames latent by design and nothing here stalls to "fix" that, so
  the row carries the frame number its newest GPU sample came from. Percentiles
  are refused below 20 samples, because nearest-rank p95 is just the maximum
  under that, and the section is absent until it has one — a run with the
  profiler off gets no row of dashes.

- **`crcbl_core::trace`: CPU spans and counters**, topic 40's span API. A scoped
  span with a static name, opened and closed by RAII and nesting freely; a
  counter is its sibling, a named `u64` sampled at the depth it was taken from.
  `drain()` is the frame boundary and hands back a snapshot per thread.

  **Always compiled and gated at runtime**, because a profiler you must rebuild
  to use is one nobody turns on mid-investigation. Disabled it costs one relaxed
  load, a test and a tail jump — read out of the release assembly rather than
  asserted — and the gate starts off.

  Records are a flat begin/end stream per thread rather than a tree, which is
  what Chrome Trace, a p50/p95 scan and per-thread tracks all want; each record
  carries the depth it sat at, so nesting is read rather than walked. A thread's
  buffer is fixed and **refuses rather than grows or evicts** — evicting the
  oldest record would take out the frame's own begin — and every refusal is
  counted and reported. Threads get a small numbered track with their name
  attached, since a Chrome Trace `tid` and a panel row both need a number and
  `ThreadId` has none.

  Nothing is instrumented yet: this slice is the mechanism, and the frame loop,
  the debug row and the trace export are the ones after it.

- **Point lights cast shadows too, through six atlas tiles rather than a cube
  map.** The grid is 4×2 now: two cascades and a six-tile light region. Faces
  are the cube-map order — `+X -X +Y -Y +Z -Z` — built by `shadow::face_axis` on
  the host and picked by `mesh.slang`'s `point_face` from the largest component
  of the offset to the light.

  **One cull per point light, not one per face**, which is the decision recorded
  in `docs/plan/18-render-features.md`: the six faces' union is the light's
  sphere and that is what the cull tests anyway, so one visible set feeds all
  six draws and a face discards what is behind it. The alternative would have
  been thirty megabytes of `DrawGen` for one light.

  That splits one number into three. `SHADOW_LIGHT_TILES` is atlas space,
  `shadow::LIGHT_SLOTS` is cull space, and the view count is the product — so
  the reachable states are **one point light or two spots**, and a light that
  fits neither budget still lights and simply does not occlude. A point light
  that cannot fit six consecutive tiles is skipped without taking the budget
  down with it, so a smaller light ranked behind it is still shadowed.

  `Scene::PointShadow` is the new golden: two casters standing on opposite sides
  of the light, so a frame that shadows one direction and not the other — the
  shape a face-indexing bug takes — fails rather than looking plausible.

- **Spot lights cast shadows.** The shadow atlas became a fixed grid of
  1024-texel tiles: the sun's cascades keep the first ones and the rest are
  handed out one per shadowed spot, which is `docs/plan/18-render-features.md`'s
  recorded decision. `shadow::Selection` ranks eligible lights by projected
  screen influence — radius over distance, the metric family LOD already uses —
  breaks ties by index, and holds an incumbent's tile until a challenger beats
  it by a quarter, so a shadow does not blink in and out as the camera drifts.

  **A light that gets no tile still lights and simply does not occlude.** A cone
  at or past 80° has no projection to build, so it is refused a tile by name and
  keeps lighting, as does every spot past the budget. `GpuLight` gained
  `shadow_tile`, spent out of the first padding word, so the row costs no more
  bytes than before; `NO_SHADOW_TILE` is `u32::MAX` rather than zero, because
  zero is a real tile, and `GpuLight::default()` is hand-written for that
  reason.

  A spot's map is a **perspective** projection down the cone, reversed-Z like
  everything else here, its field of view twice the outer half-angle so the map
  covers the cone exactly. It gets no texel snap and needs none: the cascades
  snap because their box follows the camera, and a spot's matrix is a pure
  function of the light. It biases in world units before projecting, in tile
  texels at the receiver, because a perspective map's depth precision is
  distributed nothing like a cascade's and the sun's constants do not transfer.

  `FrameUniforms` gained `light_view_proj`, appended after `cluster_grid` so no
  existing member moved and every cascade golden stayed byte-identical.
  `Light::row` now takes the slot. `Scene::SpotShadow` is the new golden: a
  pyramid between the light and the floor, asserting the floor is dark behind
  the caster and lit across the pool from it, and that removing the caster
  lights what it darkened.

- **Spot lights are drawn and their cone is asserted by pixels.** `Scene::Spot`
  is a floor lit from directly overhead, so cone axis, surface normal and view
  direction are all one axis and brightness is a function of distance from the
  frame's centre alone. Four luminance profiles out from the centre assert a lit
  floor, a core at least three times brighter, the axis as the maximum, and **at
  least twelve samples strictly inside the penumbra band** — the check that
  separates a ramp from a boolean.

  That last one earns its place: swapping the inner and outer angles produces a
  frame with the **same 697 at the axis and the same 106 at the edge** as a
  correct one, and every other assertion passes on it. Only the penumbra count
  moves, to zero.

  The froxel bound for a spot is a cone as well as a sphere now, each rejection
  slackened by the froxel's own bounding radius so it can only ever add froxels.
  One narrow spot goes from **144 froxels to 91**, a 37 % drop, with every
  golden bit-identical on radv, lavapipe and wgpu. Dropping the slack makes it
  too tight, and the spot scene catches that as a tile-shaped bite out of the
  pool — which is exactly the seam a too-tight cull produces and the reason the
  scene exists.

- **Many lights, gathered by a clustered-forward pass.** `crcbl_render::Light`
  with `PointLight` and `SpotLight`, an SSBO of rows the way instances and
  materials already are, and `light_cluster.slang` assigning them to a froxel
  grid — screen tiles by depth slices — that the fragment stage indexes by its
  own position. `Scene::Lights` is the new golden.

  **The sun is a row too**, flagged as reaching every froxel, so it stops being
  a special case in the shader — and the proof that the conversion is faithful
  is that **every existing golden is bit-identical**, measured byte-for-byte
  before and after rather than trusted to the comparator, which is
  tolerance-and-SSIM based and would have absorbed real drift.

  Depth slices are **exponential** (Olsson–Assarsson) because a uniform split
  over 0.1–1000 m would give a first slice 41 m deep holding every light. The
  slice index comes from **linear view depth**, not `SV_Position.z`: under this
  engine's reversed-Z that value runs backwards _and_ hyperbolically, so a
  uniform step in it would put one slice covering 2.4 m to infinity.
  `1/SV_Position.w` was avoided too — a reciprocal on some targets and not
  others, which is the class of cross-target disagreement `mesh.slang`'s header
  records being burned by twice. An orthographic camera has no view depth at all
  and runs on one slice through its own branch.

  Assignment is conservative by construction: a froxel is the convex hull of its
  eight corners so their AABB contains it exactly, the falloff window is exactly
  zero at the light's radius so cull and shading are the same statement, and a
  spot is bounded by its sphere rather than its cone — loose in the safe
  direction.

  **Cluster overflow is counted rather than dropped silently**, riding the
  existing delayed-readback counter: 21 lights over 288 froxels against a budget
  of 16 refuses 1440 assignments, and the zero case is asserted first so the
  counter is not wired to a constant.

- **`crcbl lod stats` and `crcbl lod gen`** — topic 25's tooling row, host-only.
  `stats` resolves every mesh the file draws and reports, per level, **where the
  geometry came from** (the file's own node and which convention declared it, or
  the DAG depth that generated it) with triangle and cluster counts and the
  group error range, then the shape of each DAG behind it. `gen` writes the
  cooked `.dag` artifact and decodes it back before reporting success.

  **Stalls are named rather than averaged away.** A level that kept more than
  three quarters of the level below it is reported `— STALLED`, and the real
  dunes patch trips it: levels 4 through 6 go 568 → 412 → 324 triangles with the
  error unchanged throughout. A report that smoothed that over would be hiding
  the one thing worth looking for.

  A hand-authored level below LOD0 reports no cluster count and no error, on
  purpose — it was never clustered or decimated here, so there is no engine
  number and printing one would invent it. LOD0 is the exception, being both the
  file's own geometry and DAG level 0.

  A refusal is an error rather than a row: an unimportable file, a level two
  nodes claim, an `MSFT_lod` id that draws nothing, or a gap the generator
  cannot reach all exit non-zero with no table. `--json` carries the same facts
  for the benchmark and editor consumers topic 40 anticipates.

  **`preview` is recognised and refused as unimplemented**, not absent — and the
  reason is bigger than it looks: `crcbl::screenshot::Scene` is a closed enum of
  three built-in scenes, so nothing anywhere can render arbitrary imported
  geometry offscreen. That scene has to exist before a preview can.

- **Shadow LOD bias: the shadow pass selects coarser casters than the camera.**
  `SHADOW_LOD_BIAS` multiplies both selection budgets for the whole pass. On the
  dunes patch at the shipped camera that is 57 clusters at `[13, 26, 18, …]` for
  the camera against 48 at `[5, 17, 24, 2, …]` for the shadow, with 7 of the 30
  groups the camera expanded staying collapsed.

  **The cascades were selecting from the wrong eye, and that is fixed.** They
  used `camera.eye + light_direction * cascade_far` — which is not the light's
  position, since a directional sun has none, but the camera's own eye pushed
  along the sun's direction, and it stepped per cascade so two cascades asked
  two different detail questions about one caster. They now select from the
  camera's eye at the camera's pixels-per-unit, because what a coarser caster
  costs is a shadow edge displaced by the group's error, and that displacement
  is seen by the camera at the camera's distance. The light remains the eye for
  the amplification stage's normal-cone test, where a shadow map's viewer
  genuinely is the light — two consumers that had been sharing one value now
  each get the one they need.

  A budget multiplier rather than "+N levels", because the descent has no level
  parameter and level-to-level error ratios are a property of the mesh: on this
  DAG level 0→1 steps about 2.4x, level 2→3 about 8.8x, and the top three levels
  share one error. Monotonicity survives because it is one positive constant
  over the whole pass, and a subset property falls out — the shadow cut is never
  finer than the camera's anywhere, which is what the test asserts.

  Per-cascade selection rings are new, because the colour pass is recorded last
  and overwrote the single selection buffer, so the shadow pass's descent had no
  observable at all and the bias would have been unmeasurable.

- **Hand-authored LOD levels are imported and win over generated ones.**
  `crcbl_scene::resolve_lod(scene, node)` resolves a mesh's chain and reports,
  per level, **where it came from** — `LodOrigin::Hand { node, mesh, via }`
  naming the glTF node and whether node naming, `MSFT_lod`, or both declared it,
  or `Generated { dag_level }` naming the DAG depth. LOD0 is always the file's
  own geometry. Gaps are filled by the generator and nothing else is; a fully
  hand-authored chain never runs it at all, observable as an empty `dags()`.

  **No silent substitution**, as the plan requires: a level two nodes claim, an
  `MSFT_lod` id that draws nothing, a node named like a level that draws
  nothing, and a gap the generator cannot reach are each a named error rather
  than a quiet stand-in.

  **Hand levels never enter the DAG**, structurally rather than by convention: a
  hand level is a mesh index into the file and a generated one is a depth into
  `dags()`, so there is no array where the distinction could be lost. A mesh
  with both is therefore selected **per instance** — an artist supplies
  whole-mesh geometry, not a crack-free cluster hierarchy, and a per-cluster cut
  across the two would crack.

  `MSFT_lod` needed `gltf`'s `extensions` feature, which costs nothing: both
  that crate's and `gltf-json`'s feature lists are empty, the `serde_json`
  behind the raw extension map is already non-optional in each, and `Cargo.lock`
  is unchanged. `MSFT_lod` on _materials_ is deliberately not read.

- **LOD hysteresis, so a camera drifting across a threshold stops flickering.**
  A group starts expanding above the budget and keeps expanding until its
  projected error falls to `LOD_HOLD_RATIO` of it. Measured on a
  boundary-straddling drift: **39 level changes over 40 host frames with one
  threshold, 0 with two**; on a real GPU, `[0, 1, 0, 1, …]` becomes `[0, 0, …]`.
  A decisive move still switches.

  **Per-group history, and that is a soundness requirement rather than a
  saving.** A cut is a cover only while expansion is monotone up the DAG, and a
  remembered answer can otherwise leave a child collapsed under an expanded
  parent — a hole. The two-threshold rule is monotone whenever the plain rule
  is, because a parent's error is at least its children's and its sphere
  contains theirs, so starting from all-zero every later frame is monotone by
  induction. Per-cluster history would have been 16.6 MB _and_ wrong; per group
  is 3.87 MB at the pool's instance capacity.

  The state is **one buffer, deliberately not a ring**: an instance the frustum
  rejected writes nothing, so its slot in a fresh ring holds a value from frames
  ago that is not its own history and need not be monotone. Ordering comes from
  the graph — the draw-argument pass declares it `ShaderReadWrite` and every
  mesh pass `ShaderRead`, so each frame's first barrier carries a real source
  scope over the previous frame's writes and reads.

  It also shrank `ClusterSelect` from 48 bytes to 16: a record now names two
  group _indices_ rather than carrying two copies of a group's error and sphere,
  so every cluster of a group reads the same word instead of bit-identical
  copies. Shadow cascades keep their own state, since sharing the camera's would
  have two eyes undoing each other's band every frame.

### Fixed

- **`crcbl-vk` freed a destroyed resource while a command buffer that was
  recorded and not yet submitted still referenced it — a use-after-free the
  driver reads through.** The seam permits record → destroy → submit, and the
  deletion queue kept a destroyed object parked until every submission _naming_
  it completed. A command buffer recorded against the same object and not yet
  submitted was invisible to that: no submission had extended its objects'
  retirement, so an earlier submission completing freed them under it. The
  validation layer reports it at the next submit as
  `VUID-vkQueueSubmit2-commandBuffer-03874` ("recorded but now has become
  invalid"), and lavapipe then reads the freed allocation and segfaults.

  `poll_retire` now refuses to free anything a recorded-but-unsubmitted command
  buffer names, and `submit` marks its command buffers submitted once the driver
  has accepted them. Nothing above the seam changes: an object still frees as
  soon as every recording that names it has been submitted or destroyed and the
  timeline has passed it. `crcbl-dx12` and `crcbl-mtl` never had this — their
  recordings take a COM/ARC reference to what they name.

- **A scaled instance's clusters were culled as if it were unscaled, so geometry
  silently vanished.** `cluster_survives` carried a cluster's mesh-space
  bounding radius into a world-space frustum test, documented as safe because
  `GpuInstance::transform` "is rigid" — a claim already false in two shipped
  scenes, where the true world radius is four to five times the local one. A
  large scaled object offset from the camera therefore lost every cluster and
  drew nothing, on devices with an amplification stage; the instance-level cull
  kept it correctly, which is why nothing upstream noticed.

  The radius is scaled by the square root of the largest absolute row sum of
  `BᵀB` — an upper bound on the basis's largest singular value, exact for any
  rotation-then-scale, and needing no contract about what callers may pass,
  which is what the previous code needed and did not have. It is `1.0` for a
  rigid transform, so nothing previously correct moved and no golden changed.
  The transformed cone axis is normalised for the same reason: unnormalised, the
  same shape at two sizes got two answers.

- **A press made before a panel opened fired that panel's buttons.** `UiState`
  latches while the pointer is down, so a pointer already held when a menu
  appears latched whatever button appeared beneath it and fired it on release —
  rare with a mouse, and the ordinary case on a phone, where the thumb on the
  movement stick _is_ the emulated pointer. Horde asked for fullscreen when that
  thumb came off a pause menu it never touched. A press now belongs to whoever
  was on screen when it landed, and a panel switch drops it.

- **A tap on a menu button did nothing, so no demo could actually be started on
  a phone.** For a touch pointer the browser fires `pointerleave` in the same
  pump as `pointerup`; the web shim reported that as a focus loss, so the
  position the release is hit-tested against was already gone by the time the
  engine looked. The identical click with a mouse worked, which is why it
  shipped. A finger between contacts is not hovering anywhere, so touch no
  longer reports enter and leave at all.

  Two more found with it: `pointercancel` handling was **inert**, because the
  spec gives it `button: -1` and that became a `PointerButton::Other` the engine
  ignores — the release was dropped and the game stayed holding the button,
  exactly the failure the handler existed to prevent. And the coarse-pointer
  copy swap did nothing, because `.key-row { display: flex }` ties on
  specificity with `.touch-only`/`.pointer-only` and won on source order, so
  every desktop saw the keyboard row and every phone saw `Esc`, `F11` and `F3`.

  All three were shipped green and all three were found by teaching the browser
  gate to dispatch touch. It drove `Input.dispatchMouseEvent` only, so the mouse
  path of shared plumbing was covered and nothing touch-specific ever ran.

- **wgpu reported a bindless ceiling no layout could be built at.**
  `max_bindless_descriptors` came straight from wgpu's
  `max_binding_array_elements_per_shader_stage`, which is the count
  `create_bind_group_layout` will not _reject_ — not one it will _accept_. wgpu
  eagerly creates a descriptor pool for 64 sets when a layout is registered, so
  radv's 8,388,606 asked the driver for roughly 537 million descriptors in one
  call and got `OUT_OF_HOST_MEMORY`, out of the very call the `u32::MAX` count
  sentinel resolves through. It is capped at the 500,000 wgpu commits to in
  writing for any device with binding arrays, the same reasoning `crcbl-dx12`
  gives for reporting the tier 2 heap constant on a tier 3 device: `Limits` is
  documented as what the backend _guarantees_.

  The portable bindless declaration therefore failed on every adapter generous
  enough to report a large ceiling, and worked on the software one CI pins —
  which is why the wgpu suite was green in CI and red on real hardware.

- **The samples' profiler HUD was timing the first eight passes of a fourteen-
  pass frame.** Every sample picked its own `MAX_TIMED_PASSES` — a literal that
  has to track how many passes the renderer records, and that nothing made track
  it. `crcbl_render::MAX_TIMED_PASSES` is that number now, summed from a
  `MAX_PASSES` each renderer states about itself, so a pass added anywhere moves
  it instead of seven copies drifting. Sandbox goes from 8 timed rows to all 14.

  The warning `PassTimers` logs when its capacity is short now fires once rather
  than every frame; a caller that sizes its own timers deliberately still gets
  it.

- **The LOD hysteresis state was host-visible and shader-written, which removes
  a D3D12 device.** Upload and readback heaps refuse `ALLOW_UNORDERED_ACCESS` at
  creation, so there is no unordered access view of one, and `crcbl-dx12`
  refused the binding by name. It is `DeviceLocal` now, zeroed by a start-up
  staging copy rather than a host write — **once**, before frame zero, because
  unlike the draw-generation counters this is history and zeroing it per frame
  would delete the hysteresis silently.

  `crcbl-render`'s
  `nothing_the_draw_generation_lets_a_shader_write_is_host_visible` is the
  guard: it builds a real `DrawGen` on the null backend and checks every buffer
  a shader writes. It needs no ICD, so it covers the WARP leg from a Linux box —
  which is where this class has now cost a device twice.

- **The mesh path's cut collapsed to the top level, from a bind range that had
  not grown with its struct.** `ClusterDrawConstants` went 16 to 32 bytes while
  the bind group still named `DRAW_CONSTANTS_SIZE` for that dynamic uniform —
  and a uniform read past a bound range is **not a fault, it is a zero**, so the
  group stride read as 0 and every instance descended against instance zero's
  state. Both the bind range and the dynamic stride now use one constant sized
  for the larger of the two blocks.

- **The uniform cut, so every geometry path draws a DAG mesh.** `draw_gen.slang`
  picks one level per instance for `IndirectCount` and `IndirectPerBatch`, and
  `Scene::Dunes` renders on all three paths. Until now per-cluster selection
  existed only where there is an amplification stage, which excludes every
  browser, WARP and the macOS runner.

  **The level chosen is the finest at which any group is expanded**, each group
  measured against its **own** sphere. That is provably the per-cluster cut's
  own floor rather than an approximation of it: nothing below it is drawn per
  cluster, and something at it is. Measuring against the root group or a
  whole-mesh sphere instead over-selects without bound — a sphere containing
  every group's is never further from the eye, so it reports a larger error, and
  on a patch seen from its own edge it saturates at level 0 from everywhere.

  The two paths are compared three ways, not by "both drew something": the host
  rule equals `cut(...).map(level).min()` over a sweep; two real devices — one
  opened with the mesh-stage features and one without — agree camera for camera;
  and at a budget where both resolve to level 0 the frames are
  **byte-identical**. Selected level goes 0 → 1 → 2 at 2, 200 and 1000 units
  back.

  `mesh::DrawConstants` gained `mesh`, because a DAG level is its own vertex
  range and a draw of level 2's indices needs level 2's base vertex while the
  instance still names level 0.

- **`OffscreenSetup` now asks for `TASK_SHADER`, which it never had.** Every
  `render_e2e` run on a mesh-shader adapter had been going through the
  un-amplified `meshMain` — the golden frames were real, but not of the path the
  device advertised. The suite's "lesser path" arm now subtracts **both**
  mesh-stage flags, because Vulkan enables `meshShader` when `taskShader` is
  requested, and without that both arms selected the same path and the
  self-comparison guard fired.

- **Per-cluster LOD selection on the GPU — topic 25's runtime half.** The
  amplification stage descends the cluster DAG against projected screen-space
  error, so one draw of one mesh renders at several detail levels across its own
  surface. On a real GPU, the near third of the dunes patch draws
  `{level 0: 13 clusters, level 1: 12}` while the far third draws
  `{level 2: 14}` — identical on radv and lavapipe.

  **The GPU's chosen cut is asserted equal to the host rule's**, cluster for
  cluster across all 254, using the very `pixels_per_unit` and budget the
  renderer wrote into the frame block. So the shader's implementation of
  `projected_error` is held to the same metric as the two Rust ones rather than
  trusted to agree.

  **Both halves of the decision index a group, never a cluster.** Each
  `ClusterSelect` record carries the producing and containing groups'
  `(error, centre, radius)` copied into every cluster that group touches, so a
  group's clusters evaluate bit-identical inputs and a cut cannot split one
  across a boundary it never locked. There is no cluster centre in the descent
  at all, and a DAG whose grouping misses a cluster is refused rather than
  defaulted.

  A parallel per-cluster buffer rather than a wider `Meshlet`: that record is
  the wire format of the committed `dunes.dag`, its 48-byte stride is pinned
  against the offsets slangc emits, and the fields are meaningless for the cube,
  pyramid and open box.

  `ClusterDag::check_cover` promotes the crack-free edge-cover check out of the
  tests, so the host sweep and the GPU test run one implementation, and the
  read-back cut is asserted crack-free **at the shipped configuration** rather
  than by inference from a sweep that did not include it. Tearing that real cut
  proves it bites: dropping one cluster reports a 45-edge hole, its own
  boundary; drawing every cluster twice reports 5446 crowded edges.

  `set_dunes` refuses without `Features::TASK_SHADER` — with no amplification
  stage there is no descent, and a DAG mesh would draw all seven levels at once.

- **A cooked cluster DAG reaches the renderer's crate, and a model built to
  exercise it.** `crcbl_shaders::dunes` is a 64x64 height-field patch — 4225
  vertices, 8192 triangles, 64 units across against a 4-unit amplitude — and
  `crates/crcbl-shaders/clusters/dunes.dag` is its cluster DAG cooked to a
  committed binary artifact: 7 levels, 103 leaf clusters down to 6.

  The seam mirrors the shader arrangement. `tools/cook-clusters.rs` generates
  the artifact from `crcbl_scene::cluster_dag::build_cluster_dag`, `--check`
  regenerates and compares, and CI runs it. **`crcbl-shaders` stays
  dependency-free**: `crcbl-scene` already depends on it, so cargo refuses a
  normal dependency back and a `[[bin]]` cannot see dev-dependencies — but a
  dev-dependency cycle is allowed and an _example_ can see one, so the generator
  is an example and `cargo build -p crcbl-shaders` builds that crate alone.

  Every DAG invariant is re-asserted **over the committed bytes** rather than an
  in-memory DAG: coverage, crack-free cuts by the position-bit edge count,
  monotone error, group spheres containing every sphere below. Nothing was lost
  in cooking.

  The height function moved into `crcbl-shaders` and `crcbl-scene`'s test
  fixture delegates to it, so the surface the decimator is tested against and
  the one the engine draws cannot drift — the 93 existing `crcbl-scene` tests
  passing unchanged, including ones pinning exact triangle counts, is the
  evidence the arithmetic is bit-identical. Vertex normals come from the
  **analytic gradient** of the height, so a decimated level is shaded against
  the real surface rather than against faces the simplifier moved.

  From an eye at the near edge, the near third of the patch draws levels 0 and 1
  while the far third draws level 2 — a two-level gap across one draw of one
  mesh, driven by distance.

- **The cluster DAG carries what a GPU descent needs, and states the selection
  rule.** `ClusterGroup` gained `error()`, `bounds()` and
  `projected_error(eye, pixels_per_unit)`; `GroupBounds` is the group's sphere;
  `DagLevel::bounds()` reports the producing group's sphere per cluster.

  **Monotone stored error does not survive division by a distance** — a closer
  group projects larger from a smaller number — so a group's sphere is built to
  **enclose** the spheres of every group below it, in the same fold that raises
  its error to dominate theirs. A containing sphere is never further from any
  eye than one inside it, so `error / distance` rises up the DAG for every
  camera rather than for the ones that happened to get tested. The radius is
  taken in `f64` and rounded up one `next_up`, because narrowing to `f32` can
  leave a part a rounding step outside the sphere meant to contain it.

  Both halves of the descent index a **group**, never a cluster, so every
  cluster a group produced evaluates a bit-identical predicate and a cut cannot
  split a group across a boundary it never locked. Scaling by each cluster's own
  sphere instead makes the mesh crack, and there is a test that says so.

- **`build_meshlets` grows clusters across shared edges instead of walking the
  index buffer.** A cluster seeds on a triangle and repeatedly takes the
  edge-adjacent triangle with the most vertex reuse, then nearest the seed's
  centroid, then lowest index. On a 32x32 dune field the mean cluster bounding
  sphere goes from **16.04 to 6.90** on a mesh 32 units across, with 21 of 23
  clusters under radius 8 where **0 of 34** were before.

  Adjacency rather than a space-filling curve, for two reasons: a curve sorts
  space, so two surfaces a hair apart interleave into one cluster whose sphere
  spans the gap; and the vertex bound — which closes most clusters — is about
  vertex _sharing_, which adjacency measures directly and proximity only
  predicts. Distance is measured from the **seed**, not the cluster's moving
  centre, because a moving centre finds both ends of a strip equidistant and
  grows into a strip as long as the mesh.

  A cluster jumps to a disconnected component only if it can take the whole
  thing. That keeps a seam-split mesh — a heap of two-triangle components —
  clustering sensibly instead of one cluster per two triangles, and it is what
  leaves the cooked cube, pyramid and open-box constants bit-identical, so no
  golden moved.

  It also removed a stall in the cluster DAG: levels went
  `2048 → 1024 → 512 → 272 → 206 → 128` to a clean
  `2048 → 1024 → 512 → 256 → 128`.

- **`crcbl_scene::cluster_dag` — the crack-free cluster hierarchy topic 25
  specifies.** `build_cluster_dag` clusters the base mesh, groups neighbouring
  clusters by partitioning the **shared-edge** adjacency graph, locks each
  group's outer boundary while simplifying its interior, re-splits, and repeats
  with different groupings — so an edge locked at one level becomes interior at
  the next. Every cut through the result is crack-free by construction, which is
  what a chain of independently-clustered levels cannot give.

  `simplify_with_locked_edges` is the prerequisite: the simplifier infers
  topological borders on its own, but a group's outer boundary is **interior**
  to the mesh and can only come from the caller. `simplify` is now a one-line
  delegation with an empty set, so every pre-existing test exercises the new
  path and proves the old behaviour is unchanged.

  **One `simplify` call per level, not one per group** — deliberately. Handing
  each group over as its own mesh would put its boundary on a topological border
  and lock it for free, leaving the new parameter decoration, and would split
  the level's vertices per group so the next level's adjacency could not see
  through them.

  **Error is carried per group, not per cluster.** A group simplifies as a unit,
  so its parents stand or fall together; a cut drawing one while descending into
  another would tear along a boundary the group never locked.

  The crack test keys every drawn edge by the **bit patterns** of its endpoint
  positions and requires each to appear exactly twice except on the base border.
  Two levels number their vertices independently, so a leaf's interface edge and
  a parent's can only collide if the coarser level kept the finer one's vertices
  bit-exactly. It sweeps every threshold at which the cut changes, and asserts
  that several of those cuts genuinely mix levels — a uniform cut is the chain,
  which was never the problem.

  Its fixture is a 32x32 dune field, 2048 triangles, 34 leaf clusters and 6 DAG
  levels. Its height function is quartic rather than trigonometric because a
  fixture pinned by equality that uses `sinf` differs in the last place between
  glibc, Apple libm and MSVC, and fails only on a CI runner.

- **The sun casts shadows — topic 18's cascaded shadow maps, at two cascades.**
  `crcbl_render::shadow` computes practical-split distances, a stable
  sphere-around-the-eye fit and texel-snapped reversed-Z orthographic
  projections; `ForwardRenderer` renders a depth-only pass into a cascades-wide
  `D32Float` atlas, one `DrawGen` cull dispatch per cascade as the plan asks,
  and `mesh.slang`'s `sun_visibility` selects a cascade by eye distance and
  filters it with 3x3 hardware PCF. It runs on every `GeometryPath` —
  `mesh_cluster.slang` shares the fragment stage — and `SHADOW_CASCADES` is a
  constant checked against both shader sources, so three is a number rather than
  a rewrite.

  The shadow pass reuses the colour pipeline's own vertex and mesh stages
  unmodified, by binding a second copy of the frame block whose `view_proj` is
  the cascade matrix. There is no second transform path to drift.

  Shadowing multiplies the sun's diffuse and specular only, so a shadowed
  surface keeps its ambient and reads as dark rather than black.

  `crates/crcbl/tests/golden/cube.png` was re-blessed on lavapipe because its
  three co-located pyramids now shadow one another. **Vulkan and wgpu render the
  new reference identically, at zero differing pixels each** — two independent
  backends agreeing is what says the picture is right rather than one backend's
  bug blessed into a file. `mesh.png`, `mesh_second.png` and `mesh_ortho.png`
  are unchanged at zero differing pixels, which is the evidence there is no
  acne: a lone cube is pixel-identical to before.

- **`crcbl_scene::lod` — the LOD chain topic 25 specifies.**
  `build_lod_chain(positions, indices, ratios)` composes the simplifier and the
  meshlet builder into levels, each carrying its geometry, its clusters and its
  error, with `DEFAULT_LOD_RATIOS` the plan's 50/25/12.5/6.25 %. LOD0 is the
  base verbatim at error zero, so the chain is one longer than the ratio list.

  **Every level is decimated from the base mesh, not from the level above**, and
  that is the whole design decision. A quadric run accumulates the planes of the
  mesh _it started from_, so a cascaded level's error is measured against its
  predecessor rather than against LOD0 — measured on a torus, cascading reports
  `0.4917831` where decimating from the base reports `0.6015088` for the same
  level, an 18 % understatement that compounds downward. Runtime selection asks
  "may this stand in for the full-quality mesh", so every level has to be on one
  scale or the numbers cannot be compared. Cascading is cheaper and is exactly
  the option that cannot fill the error column honestly.

  Error is non-decreasing up the chain, asserted per adjacent pair — though note
  that invariant holds for **both** designs and so does not distinguish them; a
  separate test re-derives each level from the base to pin the provenance.

  **This chain supports per-instance selection only.** Each level is clustered
  independently, so two levels' cluster boundaries have no relationship and
  drawing one level's cluster beside another's cracks along the shared edge.
  That is the `IndirectCount`/`IndirectPerBatch` granularity;
  `docs/plan/03-gpu-driven-rendering.md` §3.5's per-cluster selection needs the
  grouped, boundary-locked, re-split DAG instead, which is a different builder
  and an open decision.

- **`crcbl_scene::simplify` — QEM mesh simplification, topic 25's auto-LOD
  generator.** `simplify(positions, indices, target_triangles)` returns a
  `Simplified` carrying the decimated mesh and its `max_error`. Iterative edge
  collapse ordered by Garland–Heckbert quadric error, cited to the 1997 paper in
  the module docs so the arithmetic can be checked against it, with unweighted
  quadrics exactly as the paper defines them.

  The collapsed vertex goes to the quadric-optimal position; a singular **or
  near-singular** matrix falls back to the best of the two endpoints and the
  midpoint. The near-singular half matters: a finite-check alone is not enough,
  because a nearly-singular quadric inverts to a finite but absurd answer — the
  test derives a case whose escaped vertex lands about 10⁶ units from a mesh
  whose planes all pass within one unit of the origin.

  Deterministic as topic 25 requires, and deliberately so: no hash-map iteration
  anywhere, a strict total order on candidates keyed by cost then endpoints then
  versions, survivors renumbered in ascending original index, faces emitted in
  original order, `f64` internally.

  Guardrails: border and non-manifold vertices are locked, faces that would
  invert or become slivers are refused, and the **link condition** — the two
  endpoints sharing exactly two neighbours — is enforced. That last one is not
  in the plan's list and the closed-mesh requirement silently depends on it:
  without it a torus at 25 % gains an edge with four faces and stops being
  closed.

  `max_error` is the largest `sqrt((Q_a + Q_b)(v̄))` over the collapses
  performed, in model units — the square root of the summed squared distances to
  the planes folded into those quadrics. **It is not a certified Hausdorff
  bound**, and the docs say so.

  **Not attribute-aware.** UV and normal seams, material boundaries and skin
  weights are all constraints on data this function is never handed; a seam that
  shares positions is invisible to it and will drift. That is the plan's own
  named auto-LOD risk and it is recorded rather than implied away. No cluster
  hierarchy, no runtime selection, no consumer yet.

- **Per-cluster culling in the amplification stage — §3.5's second bullet.**
  `mesh_cluster.slang` gained a task stage that rejects a cluster on the frustum
  and on its normal cone, `ForwardRenderer::culls_clusters()` reports whether it
  is running, and the surviving-cluster count rides `draw_gen`'s existing
  delayed readback as a second word beside the instance count — §3.6 promises
  one readback in the frame loop and this keeps it at one. The instance cull is
  unchanged and still runs first.

  `Features::TASK_SHADER` is separate from `MESH_SHADER`: a device with mesh
  shaders and no task shaders builds no task stage, culls nothing, and draws
  exactly as before.

  **The documented cull rule was wrong and is fixed in the same change.**
  `ClusterBounds::cone_cutoff` stated the point-sized form, which treats every
  triangle as sharing the centre's view direction — so a cluster with a real
  radius close to the camera could hold a front-facing triangle and still be
  rejected. The conservative form adds the radius:
  `dot(axis, center - camera) > sqrt(1 - cutoff²) · |center - camera| + radius`.
  A randomised check over 400 000 samples dropped a front-facing cluster **0**
  times with the corrected form and **11 225** times with the old one. The
  `cone_cutoff > 0` guard is still needed beside it: `sqrt(1 - cutoff²)` is even
  in `cutoff`, so it cannot tell a narrow cone from one wider than a hemisphere.

  Culling that rejects nothing passes every golden, so the tests count instead
  of looking. Four cameras, each measured with the box in the scene and out of
  it so the numbers are attributable: all five clusters survive from two cameras
  that see every face, the cone rejects two from the golden's camera, and the
  frustum rejects two from inside the box. The fourth camera exists only to pin
  the radius term — dropping `+ radius` leaves the other three counts untouched.

- **A mesh that clusters into more than one meshlet, and renders.** Both
  resident meshes were a single cluster each, so a cluster with a non-zero
  `vertex_offset` _within_ a mesh was covered by unit tests and by no rendered
  frame — and per-cluster culling would have had nothing to reject.
  `crcbl_shaders::mesh` gains an open box: a unit cube missing its `+Y` face,
  each remaining face divided into 4×4 quads with unshared vertices, which
  clusters into **five**, one per face. `ForwardRenderer` grew a third bucket to
  draw it, resident but not instanced by default, so no existing golden moved.

  Every coordinate is a multiple of a quarter, which is deliberate: the test
  that pins the cooked clusters against the real builder compares bounds for
  equality, and a trig-derived mesh would differ in the last place between
  glibc, macOS and MSVC — a failure only a CI runner could show you. The one
  irrational value is a radius of `sqrt(0.5)`, a single correctly-rounded
  operation.

  The box is open and inward-facing so that a camera exists from which every one
  of its clusters is front-facing. A closed shape has none, and that camera is
  what the culling work needs to assert nothing is rejected that should not be.

- **Every sample that builds a renderer now asks for the mesh path too.**
  `horde`, `breakout`, `flappy` and `asteroids` each spell their own
  `optional_features` and were the four that stayed on `IndirectCount` after the
  flip below. What this buys is sample rule 12 — "every sample runs on every
  path the device offers, and says which it took" — and a downgrade line that
  now names `MESH_SHADER` where a device lacks it. **It is not a performance
  change**: `apps/sandbox` is the only sample that constructs a
  `ForwardRenderer`, and `EmitTail::from_caps` is the only reader of
  `geometry_path()`, so the four draw every sprite through the same unbranched
  `encoder.draw` as before. Measured on horde at 10 000 instances, three repeats
  per arm: the between-arm difference is smaller than the within-arm spread, and
  the GPU timings are identical.

  `apps/hud` is deliberately left out. Its `desc()` omits `GPU_DRIVEN` entirely
  with a stated reason — nothing in it issues an indirect draw — and it builds
  neither renderer, so a mesh stage there would be a flag with no consumer.

- **A mesh-capable device now actually draws through the mesh path.**
  `MESH_SHADER` is requested as an optional feature at both sites that open a
  device with `GPU_DRIVEN` — `crcbl::GpuContextDesc::default` and the new
  `OffscreenSetup::OPTIONAL_FEATURES` — so `apps/sandbox`, `apps/bare` and the
  golden-frame harness select `GeometryPath::MeshShader` where the adapter
  offers it. `OffscreenSetup::open_with` is new public API for naming a
  different set.

  **It is named beside `Features::GPU_DRIVEN`, deliberately not added to it.**
  That bundle is used as `required_features` in four places against a null
  backend that reports no mesh shaders, so folding `MESH_SHADER` in would refuse
  those devices outright — and it would make every `gpu_driven()` test select
  the mesh path, deleting the only coverage the other two `GeometryPath` arms
  have. The bundle is the data-layout axis; geometry is a separate selector.

  The golden tests now assert the device took the best path its adapter offers,
  because a golden passing is equally consistent with nothing having changed.
  Every scene `render_e2e` covers — `ui`, `sprite`, `cube` — is compared byte
  for byte between the two paths in one process, and all three are identical on
  an RX 7900 XTX and on lavapipe. No golden moved.

- **The forward pass draws through a mesh pipeline, and it is the same
  picture.** `docs/plan/03-gpu-driven-rendering.md` §3.5's geometry path exists:
  `EmitTail::Mesh` is selected from `GeometryPath::MeshShader`,
  `crcbl_render::cluster_pool` uploads a mesh's clusters, and
  `mesh_cluster.slang`'s mesh stage emits them.
  `ForwardRenderer::geometry_path()` reports which path a renderer resolved.

  The GPU-facing record is new — `crcbl_shaders::meshlet::Meshlet` with
  `MESHLET_STRIDE` and `ClusterBounds`, beside `GpuMaterial` and `MeshVertex`,
  whose offsets are pinned against what `spirv-dis` reports the shader expects.
  `crcbl_scene::meshlet::build_meshlets` is still the builder and re-exports it;
  the record lives in `crcbl-shaders` because `crcbl-render` must not depend on
  `crcbl-scene`, which would pull `gltf` into the renderer. The builder's
  `usize` offsets narrow through `Meshlet::new`, the only constructor, which
  refuses an offset a `u32` cannot hold rather than truncating it.

  **The mesh path matches the indirect paths' own golden, not one of its own** —
  `tests/golden/mesh.png` at zero differing pixels on an RX 7900 XTX, and
  `every_geometry_path_draws_the_same_frame` compares all three paths byte for
  byte in one process. A new golden for a new path would have passed whatever
  that path happened to draw.

  **No app selects it yet.** `Features::GPU_DRIVEN` does not include
  `MESH_SHADER`, and `crcbl::GpuContextDesc::default` asks for `GPU_DRIVEN`, so
  the samples and `crates/crcbl/tests/golden/cube.png` still run `IndirectCount`
  on hardware that could do better. Only the vk device tests request the flag.

  Not built, deliberately: per-cluster culling in an amplification stage (there
  is no amplification stage at all — `ClusterBounds` is uploaded and read by
  nothing), cluster LOD, and any bake cache.

- **`apps/hud` runs in a browser, so every sample now has a demo on the Pages
  site.** It gained a `web.rs`, a `cdylib` library named `crcbl_hud`, the polled
  `PolledGpu`/`PendingLoop` bring-up the other samples use, and an entry at each
  registration site — `web/build.sh`, `web/build-pages.py`,
  `web/tools/browser-e2e.mjs`, `web/pages/`, `web/demos/` and a step in the
  Pages workflow. The bin target is unchanged; the library rename means the
  binary now says `use crcbl_hud::…`.

  It is the smallest wasm artifact of the five at 2 720 934 bytes, against
  horde's 3 028 644. `Game::log_heartbeat` is new and is the one behavioural
  addition: hud logged nothing from inside its tick, and the browser gate reads
  both "a paused demo runs no ticks" and hud's own advancing state off that
  line.

  hud takes no input, so its gate row asserts no key press and the shared
  `run-browser-e2e.sh` no longer claims every demo "took a real key event" — the
  per-check lines still name the key where there was one.

- **`crcbl_scene::meshlet` clusters a triangle list into meshlets.**
  `build_meshlets(positions, indices)` returns the meshoptimizer/NVIDIA
  three-array layout — the original vertex indices run per cluster, three `u8`
  corners per triangle indexing into that cluster's own run, and a `Meshlet`
  record naming both runs plus a `ClusterBounds` — under `MAX_CLUSTER_VERTICES`
  and `MAX_CLUSTER_TRIANGLES`. It is `docs/plan/03-gpu-driven-rendering.md`
  §3.5's bake step and it is deterministic: the same two arrays give
  byte-identical output, which is what §3.5 asks for and what a bake cache will
  need.

  `ClusterBounds` carries a bounding sphere (AABB midpoint and furthest vertex —
  valid, deliberately not minimal) and a normal cone whose axis is the
  area-weighted sum of the triangle normals and whose `cone_cutoff` is the
  smallest dot product any of them makes with it. A cluster whose normals cancel
  — a closed shape, a fan of opposing faces, nothing but zero-area triangles —
  gets `OMNIDIRECTIONAL_CUTOFF` and a unit `OMNIDIRECTIONAL_AXIS` rather than a
  NaN, because a NaN reaching a backface cull silently drops geometry. The cull
  rule the cone exists for is written out on `cone_cutoff` for the consumer.

  **This is the builder and nothing else** — no GPU upload, no amplification or
  mesh shader, no bake cache, no `GeometryPath::MeshShader` emit tail, and no
  caller. Each is a later slice, and the module says so.

- **Materials have a base-colour texture, through one `ArrayPages` page.**
  `docs/plan/03-gpu-driven-rendering.md` §3.2's "texture indices + factors" now
  has both halves: `crcbl_render::forward` uploads a `D2Array` image whose
  layers material rows index, `mesh.slang` samples it in the fragment stage and
  multiplies the texel into the factor and the vertex albedo.

  **One binding model, and it is the one every device can run.** The page is a
  single image with `count: 1` and no `BindingFlags`, so the layout is legal on
  vk, wgpu, Metal and D3D12 alike — `BindingModel::Bindless` needs
  `Features::DESCRIPTOR_INDEXING`, which `crcbl-mtl` withdraws, so a descriptor
  array would have left Metal with no texture path. Nothing is refused anywhere;
  a bindless device runs the same declaration and will gain capacity rather than
  a second code path.

  Colour space is the trap and it is handled by the format: the page is created
  as `Rgba8UnormSrgb`, which is what glTF defines a base-colour texture to be,
  so the sampler decodes to linear and the shader multiplies two linear values.

  **`CRCBL_GPU=wgpu` cannot draw the cube scene until `crcbl-hal` can say a
  sampled image is an array.** `BindingKind::SampledImage` carries no view
  dimension, so `crcbl-wgpu` declares every one as `D2` and refuses the page's
  `D2Array` view at `create_bind_group`. Vulkan, Metal and D3D12 take the
  dimension from the view and are unaffected; the sprite and UI scenes are
  unaffected on wgpu too. It fails at build with a named error rather than
  drawing untextured — see `docs/backlog.md`.

- **`crcbl_render::upload_texture_layers`** uploads several equally sized layers
  into one `D2Array` image, beside `upload_texture`'s single-layer `D2`. It
  records one copy per layer, because a copy region's extent is 2D on every
  backend the engine has.

- **`ForwardRenderer::set_textured_pyramid`** puts a third instance of the
  pyramid mesh in the frame, shaded through a material that differs from
  `set_pyramid`'s in its page layer and in nothing else — the texture column's
  observable, beside `set_tinted_pyramid`'s for the factor column. The `cube`
  golden holds all three, and `tests/golden/cube.png` was re-blessed for it.

- **The null backend can now be resized and killed on demand.** Two injection
  hooks join the four `crcbl_hal::null::Recorder` already had.
  `report_swapchain_out_of_date()` latches the swapchain out of date, so
  `acquire_next_frame`, `present` and `wait_until_presented` all report
  `SurfaceError::OutOfDate` until a successful `reconfigure_swapchain` clears it
  — the variant the seam calls expected traffic and that this backend could not
  produce at all, since every `SurfaceError` it built was `SurfaceError::Hal`.
  `lose_device(message)` loses the device permanently: every later call that
  resolves a handle, plus `Device::wait_idle`, fails with
  `HalError::DeviceLost(message)` and nothing clears it. It is the deliberate
  opposite of `report_device_error`, which stays recoverable and one-shot.
  Between them the engine's three out-of-date arms and its device-loss policy —
  loss surfaces, the loop stops, nothing is rebuilt — are testable on a machine
  with no GPU, where before they ran only on a real driver mid-resize.

- **`crcbl-scene` — glTF 2.0 import, through the asset seam.**
  `crcbl_scene::import_gltf(source, key)` reads a `.gltf` or `.glb` and every
  external `.bin` it names through `crcbl_assets::AssetSource`, and returns a
  `GltfScene`: meshes as triangle lists (positions, normals, `TEXCOORD_0`,
  indices), materials as `crcbl_shaders::mesh::GpuMaterial` rows, and the node
  hierarchy flattened into instances carrying a composed column-major model →
  world matrix. glTF's `baseColorFactor` is linear RGBA and so is
  `GpuMaterial::base_color`, so the material mapping is an assignment with no
  colour conversion. Nothing touches `std::fs`: a source that answers
  `StorageError::Pending` makes the import `Pending` too, which is what lets it
  work in a browser. No GPU upload, no textures and no scene format yet — those
  are the rest of `docs/plan/06-assets-scenes.md`'s step 3 and its step 4.

  **The crate does its own validation rather than using `gltf`'s**, because
  `gltf` 1.4.1's validation panics on inputs it exists to reject: an
  out-of-range `POSITION` accessor index aborts in
  `gltf_json::mesh::primitive_validate_hook`, and a `.glb` header declaring a
  total length below its own 12 bytes subtracts with overflow in
  `Glb::from_slice`. Every accessor, buffer view and index the importer reads is
  bounds-checked first, so no file contents can panic it: a truncated `.glb`, a
  chunk length that overruns, a buffer view past the end of its buffer, an
  accessor count that overflows its own byte span, an index past its vertex
  array, a node hierarchy with a cycle and a buffer URI that escapes the asset
  root are all errors. `data:` URI buffers and sparse accessors are refused as
  `StorageError::Unsupported`; primitives that are not triangle lists are logged
  and skipped.

- **`crcbl_assets::StorageError`** is re-exported, so a crate that implements or
  calls `AssetSource` can name the error it returns without depending on
  `crcbl-store`.

- **`crcbl-assets` — asset ids, load states, and the IO seam under them.** A new
  workspace member carrying `AssetId` (128-bit, printed as 32 hex digits,
  derived from the canonical asset key), `AssetRegistry` with
  `crcbl_core::Handle`-based handles and a `Loading | Ready | Failed` state
  machine, the `AssetSource` trait — one `read` that is defined never to block
  and answers `StorageError::Pending` while IO is outstanding — and `DirSource`,
  the native implementation over a directory. Keys are validated with
  `crcbl_store::web::canonical_key`, the same rule the browser fetch backend
  applies, so a name that loads from a directory is a name that can be served
  over HTTP; anything that would escape the asset root, or that HTTP would read
  as a query, fragment, scheme or another origin, is refused. Nothing decodes an
  asset yet: this layer hands out bytes, and the glTF/PNG/WAV importers are the
  next slice.

- **`apps/hud` — sample 04 at its first milestone.** A HUD page built from the
  UI system's slice-1 primitives: health and mana bars, a four-slot ability row
  whose slots read `READY` or sweep down a cooldown, a wave banner and a damage
  ticker, all driven by a server-owned ticker over `InMemoryTransport` and laid
  out against the acquired extent rather than a fixed size. It contributes two
  debug-overlay modules, and the `page` one tallies its rows off the draw list,
  so the panel reports what the UI pass actually uploaded rather than what the
  sample believes it drew.

  **Milestone 1 only**, which is what `docs/plan/sample/04-hud.md` scopes to P4.
  The stylesheet subset, the two themes, the widget gallery, the UI inspector
  and the hot-reload demo are P10 and wait on a styling system that does not
  exist; the minimap frame is left out because the hard cap forbids the scene it
  would frame.

- **`crcbl_render::MaterialTable`, and a material id that indexes something.**
  `docs/plan/03-gpu-driven-rendering.md` §3.2's material table: a storage buffer
  of `GpuMaterial` rows, one `base_color` factor each, which `mesh.slang`'s
  vertex stage multiplies into the vertex albedo. Two instances of one mesh
  differing in nothing but `GpuInstance::material` are two colours in one draw,
  which the cube golden now shows — `ForwardRenderer::set_tinted_pyramid` is the
  second pyramid there for exactly that.

  **The factors half only.** §3.2 pairs the table with a bindless texture array
  or texture array pages, and there is no texture column: which of the two an
  index would mean is a decision the engine has not taken, and a column carried
  ahead of it is a field nothing reads. One buffer and no ring, unlike the
  instance array — a material is written when it is created, so
  `MaterialTable::set` is a start-up call on the terms `MeshPool::upload` is.

- **`CRCBL_DX12_VALIDATION` turns the D3D12 debug layer on or off**, and it is
  on by default in a debug build and off in a release one — the shape
  `CRCBL_VK_VALIDATION` already has on Vulkan. It needs Windows' _Graphics
  Tools_ optional feature; without it `crcbl-dx12` warns and carries on, because
  a missing optional component must not stop the engine running. Because the
  layer writes to a debugger and CI has none attached, its messages are also
  pulled out of the `ID3D12InfoQueue` and put into the error a caller actually
  sees.

- **Every device-removed failure `crcbl-dx12` reports now names its reason.**
  `DXGI_ERROR_DEVICE_REMOVED` is reported at the _next_ call rather than the one
  that caused it, so the code alone is a symptom; `GetDeviceRemovedReason`'s
  answer — spelled out, not left as an `HRESULT` — and whatever the debug layer
  stored are appended to the message by `HalError::DeviceLost` and
  `HalError::Backend` alike, on resource creation, swapchain creation, resize,
  present, `GetBuffer` and every fence wait.

- **`CRCBL_ADAPTER` picks which adapter a screenshot opens a device on**, for
  every backend. It names a device _class_ — `cpu`, `integrated`, `discrete` or
  `virtual` — rather than an index, because an index is a position in one
  machine's enumeration and moves when a GPU is added or removed. Unset keeps
  the previous behaviour, whatever the backend enumerated first. A pin that
  matches no adapter, matches more than one, or is not a class at all is a hard
  failure naming what _was_ enumerated — never a fallback, for the reason
  `CRCBL_VK_ICD` exists: a harness that asked for the software rasteriser and
  silently got a discrete GPU produces a green run about a device nobody chose.
  The resolver is `crcbl::adapter` (`select`, `pin`, `device_type_from_name`)
  and `crates/crcbl/tests/run-render-e2e.sh` passes it through.

  The measurement behind it: `crcbl::screenshot` took `adapters().first()`, and
  on `windows-latest` that adapter is not a usable device — a D3D12 frame died
  on its first buffer with `DXGI_ERROR_DEVICE_REMOVED` in a job whose D3D12 HAL
  suite had just passed 155/155 on WARP. `crcbl-dx12`'s own `CRCBL_DX12_ADAPTER`
  is unchanged and still serves that crate's suite; it is `#[cfg(test)]` and
  could not reach a harness in another crate.

- **`OffscreenSetup::adapter`** returns the `AdapterInfo` the frame's device was
  created on, beside the existing `backend()` and `caps()`. A screenshot could
  not say which of a machine's adapters drew it, so a pin that never reached the
  process and one that was honoured looked identical from outside.

- **`crcbl-dx12` honours dynamic offsets.** A
  `BindingKind::UniformBuffer { dynamic: true }` or its storage-buffer twin was
  refused at `create_bind_group_layout`, and `bind_group` refused a non-empty
  `dynamic_offsets` again; both now work. Such a binding leaves the set's
  descriptor table and becomes a **root descriptor** — a root CBV, SRV or UAV,
  which takes a GPU virtual address rather than a descriptor handle, so the
  offset is one addition on the way to
  `SetGraphicsRootConstantBufferView`/`SetComputeRootConstantBufferView` and
  their SRV/UAV siblings. It costs no descriptor in the group's block, and it
  still takes its HLSL register in declaration order beside the table's.
  `ForwardRenderer`'s mesh set — whose binding 3 is dynamic — is a layout this
  backend can now build, and the forward pass's
  `bind_group(0, group, &[constant_offset], layout)` records.

  Three things are refused by name rather than discovered later: a dynamic
  binding with `count` other than 1 or with any `BindingFlags`, because a root
  descriptor is one address and is not in a descriptor heap; a `bind_group`
  whose offset count, alignment or bounds do not fit the set, checked against
  the device's own `min_uniform_buffer_offset_alignment` (256 on D3D12) and
  `min_storage_buffer_offset_alignment` (16); and a **pipeline layout that
  exceeds D3D12's 64-DWORD root signature budget**, at `create_pipeline_layout`
  rather than at the draw — a descriptor table costs one DWORD and a root
  descriptor two, so 32 dynamic bindings across a layout's sets are the ceiling.

- **`crcbl-dx12` records every draw the seam has.** `bind_index_buffer` sets a
  `D3D12_INDEX_BUFFER_VIEW`, `draw_indexed` is `DrawIndexedInstanced`, and
  `draw_indirect`, `draw_indexed_indirect` and both `_count` siblings are
  `ExecuteIndirect` through command signatures the device caches per
  `(argument layout, stride)` — D3D12 puts `ByteStride` on the signature rather
  than on the call, so two callers striding differently need two objects. A draw
  with no pipeline bound, an indexed one with no index buffer bound, an argument
  span that runs past its buffer, an unaligned argument or count offset, and a
  multi-command stride below one argument structure are each refused by name at
  record time, because `ExecuteIndirect` reports none of them.

- **`crcbl-dx12` reports `DRAW_INDIRECT_COUNT`, `MULTI_DRAW_INDIRECT` and
  `INDIRECT_FIRST_INSTANCE`**, and `Limits::max_draw_indirect_count` moves off
  the floor to `u32::MAX` — `ExecuteIndirect`'s own `MaxCommandCount` is a
  `UINT` and D3D12 states no lower ceiling. All three are parameters and fields
  of that one call rather than capability bits, and each is reported now that
  the call behind it is made. **`DRAW_INDIRECT_COUNT` moves a selector**: every
  D3D12 adapter now derives `GeometryPath::IndirectCount` where it derived
  `IndirectPerBatch`, so a renderer on this backend takes the arm that reads its
  draw count out of GPU memory. Metal cannot follow, because it has no
  count-from-memory execution at all.

- **`crcbl_shaders::Shader::dxil_containers` hands over every DXIL container a
  shader holds**, each paired with its entry-point name, in the shape
  `ShaderModuleDesc::dxil` takes. `Shader::dxil(entry_point)` still answers for
  one entry point; a call site filling in a descriptor wants the new accessor,
  and every one of the engine's passes now does — so the graphics passes offer
  DXIL where they previously offered none.

- **`CRCBL_GPU=dx12` selects the Direct3D 12 backend on Windows.**
  `crcbl::backend::GpuBackend` gains a `Dx12` variant, spelled `dx12` or `d3d12`
  wherever a backend is named — the environment variable, `--backend`, and
  `GpuBackend::from_name`. The registry entry exists on Windows alone, exactly
  as the Metal entry exists on macOS alone, and it is **never auto-selected**:
  Windows already reaches a GPU through `crcbl-vk`, and D3D12 is the same engine
  through a different loader rather than a replacement for it, so an
  unconfigured run there picks Vulkan as before. Off Windows the name still
  parses and resolving it reports `GpuError::UnknownBackend` naming the backends
  that build does have.

  The rest of the seam is not there yet: a `CRCBL_GPU=dx12` run of anything that
  builds a `ForwardRenderer` now builds every pipeline the renderer needs and
  gets as far as the forward pass's `bind_index_buffer`, which refuses with
  "indexed draws (the DX12 pipeline slice)". Adapter enumeration, buffers,
  images, bind groups, graphics and compute pipelines, a clear, a triangle, a
  dispatch and a swapchain all work.

- **`crcbl-dx12` runs compute.** `Device::create_compute_pipeline` builds a
  `D3D12_COMPUTE_PIPELINE_STATE_DESC` from the same root signature and the same
  validated DXIL container the graphics path uses, and
  `CommandEncoder::bind_compute_pipeline`, `dispatch` and `dispatch_indirect`
  record against it — the last through an `ExecuteIndirect` command signature
  the device creates once. A bind group issued inside a compute pass now lands
  on the compute bind point rather than the graphics one, which is the only
  signal the seam carries. `Features::COMPUTE` is reported as of this change and
  not before it, and the test behind the flag dispatches `compute_probe.slang`
  and reads back what it wrote.

  The seam's `ComputePipelineDesc::workgroup_size` is checked against the
  artifact, not just against the device's limits: `[numthreads(x, y, z)]` is in
  every signed container's `PSV0` part, so a descriptor that disagrees with the
  shader is refused by name here exactly as `crcbl-vk` refuses it from SPIR-V.

- **`crcbl-dx12` accepts `SurfaceTarget::Offscreen`**, so a D3D12 device can
  render into a texture and read it back with no window — what
  `crcbl screenshot` and every headless harness need. It used to refuse with
  "offscreen surfaces (a later DX12 slice)". The "swapchain" on such a surface
  is a ring of plain `ID3D12Resource` textures with no `IDXGISwapChain3` behind
  it, driven through the same `acquire_next_frame`/`present` pair a window uses:
  acquire reads a ring cursor instead of `GetCurrentBackBufferIndex`, present
  bumps it instead of calling `Present`, and `reconfigure_swapchain` recreates
  the images instead of calling `ResizeBuffers`.

  `Instance::surface_caps` answers for an offscreen surface from the ring's own
  capabilities rather than a window's, and they genuinely differ: flip-discard's
  format list and its two-image floor do not apply, so a ring may be one image
  deep, offers the same formats in the same order as `crcbl-vk`'s offscreen ring
  — `Rgba8UnormSrgb` first — and reports no `current_extent`. Presents on a ring
  are unnumbered, so `wait_until_presented` answers immediately rather than
  blocking on a waitable object that does not exist.

- **A screenshot now says which backend drew it and what that device selected.**
  `crcbl::screenshot::OffscreenSetup::backend` returns the `BackendKind` the
  registry opened and `OffscreenSetup::caps` returns its `DeviceCaps`, so a
  caller can read the `GeometryPath`, `BindingModel` and `LightingPath` the
  frame was actually rendered through. Without them the frame is the only
  output, and every backend draws this scene identically by construction — so a
  run pinned with `CRCBL_GPU` that silently fell back to another backend
  produced a passing frame and proved nothing about the one that was asked for.

- **The forward pass draws from GPU-generated indirect arguments.** `cull.slang`
  and a new `draw_gen.slang` run as two compute passes in front of the forward
  pass, and the pass records **one indirect call per bucket whatever the scene
  holds** rather than one draw per object — topic 03 §3.3, both halves. Adding
  or removing an object is an instance in `InstancePool` and changes no recorded
  command; how many instances a bucket draws, and which, is written by the GPU
  into buffers the draw reads. The barriers between the three passes, including
  the transition into `ResourceState::IndirectArgument`, are the render graph's.

  New: `crcbl_render::draw_gen::{DrawGen, DrawGenDesc, GeneratedDraws}` owns the
  two dispatches and their buffers, `crcbl_shaders::draw_gen` owns the workgroup
  size, uniform block and `DrawIndexedArgs` layout
  (`VkDrawIndexedIndirectCommand`, which D3D12 and `wgpu` spell the same way),
  and `ForwardRenderer::{draws, frame}` expose the generated buffers so a caller
  can read the culling statistics back.

  Which call the pass records comes from `GeometryPath`: `IndirectCount` issues
  `draw_indexed_indirect_count` per bucket with a GPU-written count, and
  `IndirectPerBatch` — Metal, whose API has multi-draw-indirect and no GPU-side
  count — issues `draw_indexed_indirect` with a count of one and leans on the
  bucket's instance count being zero. Both draw the same frame byte for byte.
  `GeometryPath::MeshShader` has no tail here yet and degrades to an indirect
  one, with a log line saying so.

- **`InstancePool::slot_count`**, the array elements a walk of the pool has to
  cover: one past the highest slot ever handed out, which — unlike `len` — does
  not shrink when an instance is removed from the middle. The cull dispatch is
  sized by it.

- **A removed instance stops being drawn.** `GpuInstance::flags` gains its first
  defined bit, `GpuInstance::LIVE` (bit 0): set, the element is a live instance;
  clear, it is a slot whose instance was removed and is still holding the
  transform and mesh id it had. `cull.slang` asks that bit before it reads
  anything else in the record, and `crcbl_render::cull::visible_instances` — the
  CPU oracle — does the same, so a freed slot is no longer culled (and possibly
  kept) on stale data. The layout is unchanged: `flags` was already there and
  already 4 bytes at offset 76.

  `InstancePool` owns the bit rather than its callers. `insert` and `set` set it
  whatever the caller passed, `remove` clears it and marks the slot dirty so the
  next `begin_frame` carries the removal to the device, and **nothing else about
  a removed record is rewritten** — a zeroed instance would be a live-looking
  cube at the origin for any consumer that skipped the check.
  `InstancePool::new` now also clears its buffers, so a slot nothing has written
  reads as dead rather than as whatever the driver left there; a pass that walks
  the array from element zero is what makes that difference visible.

  The consequence for a caller is that the cull pass may be dispatched over the
  pool's whole capacity: correctness no longer rests on an `instance_count` that
  happens to stop before the first freed slot.

- **GPU frustum culling, checked against a CPU reference.** `crcbl-shaders`' new
  `cull.slang` is `docs/plan/03-gpu-driven-rendering.md` §3.3's compute pass:
  one thread per instance, the mesh's local-space AABB transformed by the
  instance transform, tested against six camera half-spaces, and the survivors
  appended to a compacted list of instance indices with an atomic counter.
  `crcbl_shaders::cull` carries its `Params` block (six `float4` planes, an
  instance count and a list capacity) and `WORKGROUP_SIZE`. The counter is the
  **true** survivor count and can exceed the list's capacity — an overflow is a
  number a caller can see rather than a list that quietly stops growing.

  `crcbl_render::cull` is the same cull in ordinary Rust: `Aabb`, `Frustum`
  (Gribb-Hartmann plane extraction from a view-projection matrix, deliberately
  **not** normalized — under the engine's reversed-Z infinite projection the far
  plane has a zero normal, and normalizing it produces `NaN`s that cull
  everything), and `visible_instances`. `Aabb::transformed` is the standard
  conservative absolute-value-matrix bound, because a rotated box is not a box.

  **Nothing consumes the visible list yet**: `ForwardRenderer` records the same
  draws it always has, and no pass in `crcbl-render` dispatches the shader.
  Indirect draw generation is the next slice. What exists now is the cull math
  and its proof — `crcbl-vk`'s `cull` e2e reads the list and the counter back
  and compares them against the Rust reference over instances placed inside,
  outside each of the six planes, straddling one, rotated back in, and naming a
  freed mesh.

- **A GPU-side mesh table, and `GpuInstance::mesh` now means something.**
  `MeshPool` maintains a third buffer beside the vertex and index pools: one
  `crcbl_shaders::mesh::GpuMesh { base_vertex, base_index, index_count, bounds_min, bounds_max }`
  (36 bytes, `MESH_ENTRY_STRIDE` — nine scalars, no padding) per mesh it can
  hold, at `mesh.slang`'s new binding 4. The bounds are the mesh's local-space
  box, computed by `MeshPool::upload` from the vertex positions it is handed and
  carried on `MeshRange::bounds`; they live in the range's own record because
  they share its lifetime exactly, and the cull pass above is what reads them.
  `MeshPool::table_index` is the id an instance carries and
  `MeshPool::table_buffer` is what a bind group names; `MeshPoolDesc` grew
  `mesh_capacity` and `MeshPoolError::MeshTableFull` reports a table with no
  entry left. **The vertex stage now resolves its own base vertex** — through
  the drawn instance's `mesh` id — instead of being handed one per draw. That is
  what `docs/plan/03-gpu-driven-rendering.md` §3.3's cull pass needs (it emits
  draws the CPU never looked at, so the geometry has to be resolvable from
  instance data alone), and it already buys something before that pass exists: a
  base vertex resolved per _instance_ lets one draw cover instances of different
  meshes, where a per-draw constant made every instance in a draw share a mesh.
  The rule that produced the block is untouched — every draw still passes zero
  for both of its own bases.

  A freed mesh's entry is **cleared**, so an instance still naming it resolves
  to the empty range (`index_count == 0`) rather than to whatever mesh next
  lands in that space; `MeshPool::free` therefore takes a `&dyn Device` and
  returns `Result<bool, MeshPoolError>`, and frees nothing if the clear fails.
  What clearing cannot cover is a table _slot_ reused by a later upload: a mesh
  id is a bare `u32` with no generation in it, so a stale id names the mesh that
  took the slot. `MeshHandle` is the generational one, and only it can tell
  those apart.

  Upgrading: `MeshPoolDesc` needs `mesh_capacity`; `MeshPool::free` needs the
  device; anything building `mesh.slang`'s descriptor set by hand must add
  binding 4 — a read-only storage buffer of `GpuMesh` — and every instance it
  draws must carry a mesh id that indexes it, because id 0 is a real entry and
  an instance that forgot its id draws whatever mesh sits there.

- **A second mesh in the geometry pool, and `ForwardRenderer::set_pyramid` to
  draw it.** `crcbl_shaders::mesh::pyramid_vertices` / `pyramid_indices` /
  `pyramid_vertex_bytes` are a square pyramid in five colours no cube face has,
  uploaded after the cube so it is the pool's first resident at a **non-zero**
  base vertex. Off by default, so the frame every sample draws is unchanged;
  `crcbl screenshot --scene cube` and a new `crcbl-vk` golden
  (`tests/golden/mesh_second.png`) turn it on. It exists to make a base vertex
  observable — see the fix below, which no picture could show while the pool
  held one mesh.

- **The instance array: `crcbl_render::instance_pool`, and the cube is now an
  instance.** `InstancePool` owns one `crcbl_shaders::mesh::GpuInstance` storage
  buffer per frame in flight and uploads **deltas** —
  `docs/plan/03-gpu-driven-rendering.md` §3.2's "changed instances only, dirty
  ranges, not full re-upload". `insert`/`set`/`remove` take generational
  `InstanceHandle`s; `index` gives the element number a shader addresses;
  `begin_frame` rotates to the next buffer, writes that buffer's outstanding
  changes, and returns the slot the caller's bind group and uniform ring should
  use. Adjacent writes coalesce into one `write_buffer` — instances 3, 4 and 5
  are one upload of three, 3 and 900 are two of one, in whatever order the
  writes arrive — and a frame in which nothing changed performs **no seam call
  at all**. The pool never grows: `InstancePoolError::PoolFull` names the
  capacity and what is in use.
- **`crcbl_shaders::mesh::GpuInstance` (80 bytes) and `INSTANCE_STRIDE`**, plus
  `mesh.slang`'s matching `struct GpuInstance` at binding 2. `transform` is a
  rigid model-to-sector `float4x4`; `mesh` indexes the mesh table (see the entry
  above, which is what made it mean something); `material`, `sector` and `flags`
  are **reserved and read by nothing**. In particular `sector` is _not_ working
  camera-relative rendering: §3.2's 2026-07-27 correction also calls for a
  per-frame f64 sector→camera offset table and a shader-side addition, and
  neither exists, so every instance is in sector 0 and `transform` is a plain
  model→world matrix. The field is in the format now because extending it after
  §3.3's shaders index it is the expensive path.
- **Global geometry pools: `crcbl_render::mesh_pool`, and the cube now lives in
  one.** One device-local vertex buffer and one index buffer, suballocated by a
  first-fit free list, so a mesh is
  `MeshRange { base_vertex, base_index, index_count }` — the three integers
  `docs/plan/03-gpu-driven-rendering.md` §3.1 asks for and everything above it
  (instance data, GPU culling, indirect draws, meshlets) assumes.
  `MeshPool::upload` suballocates both pools, stages the bytes and submits the
  copy against the pool's own timeline semaphore; `MeshPool::flush` waits for
  that value and retires the staging buffers; `MeshPool::mesh` hands out a range
  **only** for a mesh whose upload has completed, so the renderer cannot consume
  geometry the GPU has not received. `MeshPool::free` returns a mesh's space and
  retires its handle. `ForwardRenderer` no longer owns two buffers of its own:
  the cube is the pool's first resident, drawn as a range with `draw_indexed`'s
  own base vertex, and the `mesh` and `ortho mesh` goldens are unchanged by the
  move.
- **The pools never grow and never defragment, and say so by name.** Capacity is
  fixed at `MeshPool::new`; a request no single free block can satisfy fails
  with `MeshPoolError::PoolExhausted`, which names the largest free block _and_
  the total free so a caller can tell fragmentation from a full pool. This is
  §3.1's stated MVP — "free-list + offline compaction on load only, no live
  defrag" — rather than an omission; the free list does coalesce neighbouring
  frees, so an alloc/free/alloc cycle reuses its space.
- **Mesh shaders are usable end to end: `Device::create_mesh_pipeline`,
  `CommandEncoder::draw_mesh_tasks`, and a golden image of the result.**
  `Features::MESH_SHADER` was reported and nothing could ask for it. The seam
  now takes a `MeshPipelineDesc` — a task stage (optional), a mesh stage, a
  fragment stage, and **no vertex input at all** — and returns an ordinary
  `GraphicsPipelineHandle`, so a mesh pipeline is bound with
  `bind_graphics_pipeline` and destroyed with `destroy_graphics_pipeline` like
  any other. `draw_mesh_tasks(x, y, z)` is the draw, taking workgroup counts of
  whichever stage the pipeline starts with. `ShaderStages` grows `MESH` and
  `TASK`, deliberately **outside** `GRAPHICS` and `ALL`, because a stage flag
  naming a stage the device lacks is refused rather than ignored. `crcbl-vk`
  implements both through `VK_EXT_mesh_shader`; `crcbl-wgpu`, `crcbl-mtl` and
  `crcbl-dx12` refuse them with `HalError::Unsupported` and report no
  `MESH_SHADER`. A device that does not report the capability refuses pipeline
  creation by name rather than failing later, and `TASK_SHADER` is refused on
  its own flag.
- **`crcbl_shaders::MESH_SHADER`, from `shaders/mesh_shader.slang` — the first
  shader that is not all four targets.** One triangle emitted by a mesh stage,
  plus an amplification stage whose payload tints it, plus the fragment stage
  both share. It declares `spirv, msl, dxil` and **not** `wgsl`, because Slang
  refuses a mesh entry point for that target outright; this is the first real
  use of the per-shader target declaration, which exists precisely so the
  refusal is a build failure rather than a broken committed artifact. The
  compile script and `build.rs` learned the `meshext` and `taskext` execution
  models, and `crcbl_shaders::Stage` grew `Mesh` and `Task`.
  `crcbl_shaders::mesh_shader` carries the triangle's positions, its colours and
  the amplification tint for the tests that sample them, plus
  `vertex_bytes`/`VERTEX_STRIDE` for the storage buffer the mesh stage pulls
  them from.
- **A bind-group layout and a push-constant range may now name
  `ShaderStages::MESH` and `ShaderStages::TASK`, so a mesh shader can read a
  buffer.** Until this, nothing accepted either flag and `mesh_shader.slang`
  hardcoded its three vertices; it now pulls them from a `StructuredBuffer` at
  set 0 binding 0, the way `triangle.slang` does. A layout entry's `visibility`
  or a `PushConstantRange::stages` naming a stage the device does not report is
  refused up front with `HalError::Unsupported` — by
  `ShaderStages::check_supported`, which every backend calls, rather than by a
  driver VUID that names neither the binding nor the capability. The two stages
  are still outside `GRAPHICS` and `ALL`, so nothing that already worked
  changes. The amplification stage's payload became a local rather than a
  module-scope `groupshared`, which is what keeps the emitted MSL legal: Slang
  2026.14 hands every entry point of a module with any global shader parameter a
  copy of every global, so the `groupshared` one landed in the fragment function
  as a `threadgroup` declaration that `xcrun metal` refuses.
- **`crcbl-vk`'s e2e suite gained a `GeometryPath::MeshShader` golden**,
  `tests/golden/mesh_shader_triangle.png` — apex-down, so it cannot be satisfied
  by a copy of the raster triangle's — alongside tests that the mesh stage's
  three vertices reach memory, that the amplification stage's payload actually
  arrives (the tinted frame differs from the untinted one in a way only the task
  stage can produce), and that a mesh pipeline naming a fragment entry point as
  its mesh stage is refused by name.
- **`crcbl_core::log::capture`, so a log line can be asserted on.** Returns a
  `Capture` guard that collects every record the **calling thread** logs, as
  `CapturedRecord { level, target, message }` read back through
  `Capture::records`. Capture is thread-scoped so concurrent tests in one binary
  cannot interleave their records, and it sees every level regardless of
  `CRCBL_LOG`, so an assertion does not turn on the environment. It is additive:
  stderr still gets exactly what the filter admitted, and a process that never
  calls it behaves as before. `capture` panics rather than capturing nothing if
  this thread is already capturing or if another logger owns the process slot.
  It is now what holds four `crcbl` log lines to their wording, each of them the
  only evidence its decision was taken and none of them read by anything before:
  the capability downgrade line from `docs/plan/39-capabilities.md`, the
  present-feedback line whose existence is why `wait_until_presented` was left
  returning `()`, the pacing resolution's
  `hal: display timing …; asked for …, pacing …`, and
  `engine: the frame limit is …`. The last two were previously checked only by
  `crates/crcbl-shell/tests/run-wayland-e2e.sh`, so on a machine without a
  Wayland compositor they could be deleted with the suite staying green.
- **`crcbl screenshot --scene`, and a cross-backend comparison that runs every
  scene.** The subcommand takes `cube` (the default, unchanged: the lit cube
  through `ForwardRenderer`), `sprite` — four sprites over three
  `SpriteRenderer` batches in `A A B A` submission order, two sheets, one of
  them tinted — and `ui`, a panel, a translucent bar over its edge, an outline
  and two lines of glyph-atlas text through `UiRenderer`. The library side is
  `crcbl::screenshot::Scene`, taken by `OffscreenSetup::open`, which is a
  breaking change to that signature.
  `crates/crcbl/tests/run-cross-backend-e2e.sh` now renders **every** scene
  through both backends at every size and compares each; its anti-vacuity colour
  floor is per scene (`CRCBL_CROSS_MIN_COLORS_CUBE`, `_SPRITE`, `_UI`, replacing
  the single `CRCBL_CROSS_MIN_COLORS`) because a UI frame has 7 distinct colours
  where the lit cube has 36–41, and its "zero comparisons ran" guard is now
  checked per scene as well as overall. This is
  `docs/plan/02-vulkan-backend.md`'s shader-portability rule 5: semantic
  divergence between the four targets is caught by rendering, not by reading,
  and the gate previously drew one scene — so `sprite.slang` and `ui.slang`, the
  two shaders with an actual history of divergence, were not covered at all.

- **Every shader declares the targets it must compile to, and the compile script
  emits exactly those.** Each `crates/crcbl-shaders/shaders/*.slang` opens with
  a `// crcbl-targets: spirv, wgsl, msl, dxil` line; `tools/compile-shaders.sh`
  refuses a source with no declaration, an unknown target name, or a declaration
  without `spirv` (the entry points every other target is driven from are read
  out of the SPIR-V), and refuses an artifact left in the tree for a target its
  shader no longer declares. The declaration is recorded as a `targets` key in
  `spirv/manifest.txt` and reaches
  `crcbl_shaders::manifest::ShaderRecord::targets`, where a record whose
  declaration and artifact columns disagree is rejected — so the check also runs
  in `build.rs`, on machines with no shader compiler. Every shader shipped today
  declares all four; the mechanism exists for the first mesh-shader or
  ray-tracing source, which will have no WGSL form at all.

- **A per-target preprocessor define, so a shader can differ by target without
  being forked.** `tools/compile-shaders.sh` and `build.rs` pass exactly one of
  `CRCBL_TARGET_SPIRV`, `CRCBL_TARGET_WGSL`, `CRCBL_TARGET_MSL` and
  `CRCBL_TARGET_HLSL` (the DXIL leg, named for the language Slang emits on the
  way). Slang defines no target macro of its own, so until now the only way to
  differ per target was a second copy of the file. No committed artifact
  changed: the defines are inert in every shader that ignores them.

- **A lint that refuses a shader declaring its resources out of binding order.**
  Slang's Metal target ignores `[[vk::binding]]` and assigns argument-table
  indices in declaration order, while `crcbl-mtl` binds by ascending
  `(set, binding)`; when `ui.slang` disagreed with itself, its MSL put the push
  constants where the vertex buffer should have been and the UI pass drew
  nothing on macOS. `crcbl-shaders` now parses every `.slang` and asserts
  ascending `(set, binding)` with push constants last, which is where Slang's
  Metal target puts them and where `crcbl-mtl` leaves room. A comment was
  previously the only thing preventing a recurrence.

- **The WGSL and MSL artifacts are validated, where before only the SPIR-V
  was.** `crcbl-shaders` gained `tests/wgsl_validation.rs`, which parses and
  validates every committed `wgsl/*.wgsl` with naga — the same front end `wgpu`
  compiles WGSL through, so a module it rejects is a pipeline that fails to
  create — and cross-checks the set it swept against the manifest records
  declaring `wgsl`. `wgsl/ui.wgsl` shipped for months with an undecorated
  `var<uniform>` that naga refuses outright, which `crcbl-wgpu` could never have
  loaded; that artifact is checked in as a fixture so the failure path stays
  exercised. naga is a dev-dependency pinned to the version `wgpu` already
  resolves — the library itself still has no dependencies. The MSL is compiled
  with `xcrun metal` on the macOS CI job, which is the only place it can be
  checked at all, and that step fails if it compiled zero files. **naga
  accepting a module is not Dawn accepting it**: Dawn enforces WGSL's uniformity
  rule where naga does not, which is how the UI shader's non-uniform
  `textureSample` drew a black canvas in the browser. This narrows the gap; it
  does not close it.

- **The engine names every capability it asked for and did not get, once, at
  device creation.** `crcbl_hal::downgrades(requested, granted)` returns a
  `Downgrades` describing each absent optional feature and the path selector its
  absence moved (`Downgrade::feature`, `::name`, `::selected`, the last a
  `SelectedPath::{Geometry, Binding, Lighting}`); `GpuContext`'s open logs it
  when it is not empty, as
  `hal: this device does not have DESCRIPTOR_INDEXING -> binding ArrayPages, …`.
  A device that got everything logs **nothing**, which is what makes the silence
  readable: `IndirectPerBatch` in the path line is now distinguishable from a
  descriptor that never asked for the count. `GeometryPath::INPUTS`,
  `BindingModel::INPUTS` and `LightingPath::INPUTS` are new, and state which
  features each selector is derived from.

- **Five `crcbl_hal::Features` flags for mesh shading and ray tracing.**
  `MESH_SHADER`, `TASK_SHADER`, `RAY_QUERY`, `RAY_TRACING_PIPELINE` and
  `ACCELERATION_STRUCTURE`. `MESH_SHADER` is the best `GeometryPath` and
  `RAY_QUERY` plus `ACCELERATION_STRUCTURE` together select
  `LightingPath::RayTraced`. Vulkan reports them (below); wgpu, Metal, D3D12 and
  the null presets report every one of them clear, so a device on those backends
  still selects the same path it did before.

- **`crcbl-vk` reports mesh shading and ray tracing, and enables them when
  asked.** An adapter's `DeviceCaps` now carries `MESH_SHADER` / `TASK_SHADER`
  from `VK_EXT_mesh_shader`, and `ACCELERATION_STRUCTURE` / `RAY_QUERY` /
  `RAY_TRACING_PIPELINE` from `VK_KHR_acceleration_structure` (with its
  `VK_KHR_deferred_host_operations` dependency), `VK_KHR_ray_query` and
  `VK_KHR_ray_tracing_pipeline` — each only when the extension is listed **and**
  its feature bit came back true, and only when everything it depends on is
  there too: neither ray capability is reported without the acceleration
  structure it traverses, and the task stage is not reported without the mesh
  stage it feeds. `GeometryPath::MeshShader` and `LightingPath::RayTraced` are
  therefore reachable for the first time. The extensions are enabled only when a
  caller names the capability in `required_features` or `optional_features`, so
  device creation is unchanged for everyone else. **This is reporting only** —
  no mesh-shader pipelines, no acceleration structures, no ray-tracing commands
  yet.

- **The X11 F11 pass now asserts the summary-line extent.** `run-x11-e2e.sh`'s
  toggle pass used to press F11 at a running sandbox, check the engine's own log
  line about the mode, and SIGTERM the sandbox — so the _extent_ after F11 was
  never checked. The key sender (`crcbl-e2e-x11-key`) now walks the X11 tree
  from the root (`Peer::find_window`, a new QueryTree + `WM_CLASS` binding
  behind the `x11-e2e` feature), finds the sandbox's window, and asks it to
  close with `WM_DELETE_WINDOW`; the sandbox tears down cleanly and prints its
  end-of-run summary, and the script asserts it reads `at 1920x1080, borderless`
  under a window manager and `at 1280x720, windowed` without one. A new suite
  test pins the window-finding walk against a unique `WM_CLASS` both with and
  without `openbox`.

- **A shot that kills a rock now raises a flash where it died.** The rock used
  to vanish and split with only the explosion cue to mark the hit;
  `apps/asteroids` now draws a two-frame burst — a white-hot core for the first
  half of the flash's 0.15 s life, a wider, dimmer fade for the second — scaled
  to cover the rock that died. The flash lives in the seeded simulation beside
  the cue, so a recorded script replays the picture as well as the score;
  particles remain a hard non-goal and this is a sprite, not one.

- **The sandbox's pause menu can change pacing and the frame cap mid-run.** Two
  new rows — `PACING: AUTO` and `FPS: 1000`, each labelled with the value it is
  set to — cycle on Enter: pacing through `Auto` → `Vsync` → `Adaptive` → `Off`,
  the cap up 30 → 60 → 120 → 240 → 1000 → unlimited. The pacing change lands on
  the GPU on the first tick after resume, through the sample's own `Gpu` and
  `GpuContext::set_pacing`; the cap change is handed to the loop through the new
  `HostedGame::take_pending_frame_limit`, which applies it to its clock with
  `Clock::set_limit` and takes it so it is not re-applied every frame. This is
  the first code in the workspace to exercise either mid-run route from a
  running game; the games without a settings screen use the method's default
  `None` and are untouched.

- **`NineSliceSource` carries its own texels-per-unit scale.**
  `with_texels_per_unit` (default 1) makes the fixed bands of `expand` and
  `minimum_size` come back in the caller's units, so a game whose world is not
  one unit per texel no longer has to scale its sprite plane and camera to
  compensate. The flappy and breakout samples were migrated to world-unit sprite
  planes; the menu's camera workaround is still owed (backlog).

- **`--pacing` and `--fps`, so a run can pick its display sync and its frame cap
  from the command line.** `--pacing <auto|vsync|adaptive|off>` sets
  `GpuContextDesc::pacing` and `--fps <N>` sets the loop's `FrameLimit`; both
  are on `crcbl::args::Common`, so every sample that takes the shared flag set
  gets them, and `apps/sandbox` — which keeps its own parser — takes them too.
  The defaults are unchanged and are what they always were: `auto`, which is
  adaptive sync where the display is running it and vsync where it is not, and
  1000 fps, which is a runaway guard rather than a cap. `--fps 0` is unlimited,
  the spelling `FrameLimit::fps` already documented. An unknown pacing is
  refused by name and lists the four — `--pacing vrr` is told the word here is
  `adaptive` — and `--fps` refuses a value that is not a number or does not fit
  a `u32` rather than truncating it.

  **A run now says what it got.** `Clock::set_limit` logs one `info` line,
  `engine: the frame limit is 30 fps` (or `unlimited`), on the real clock only —
  a headless run has no frame limit to report. The pacing already appeared on
  the `hal: display timing …; asked for …, pacing …` line.

  **A game can still pick both, and change them while it runs.** The flags are
  the command line's route to values a game may set for itself: `Common` is an
  ordinary struct with public fields, `crcbl::engine::Loop::clock_source_mut` is
  new and is the frame limit's counterpart to `GpuContext::set_pacing`, and the
  `crcbl new` template documents both routes where a scaffolded game would look
  for them.

- **`--size <WxH>`, so a run picks the extent its window opens at.** The value
  is on `crcbl::args::Common` as `size: Option<PhysicalSize>`, so the four
  samples that take the shared flag set — and the `crcbl new` template — open
  their window at the size named instead of their hardcoded default; a `WxH`
  that is not two positive numbers is refused by name. It exists for the
  headless measurement the samples were otherwise stuck at one extent for: the
  offscreen ring takes its extent from the window, and `--size 1920x1080`
  renders at exactly 1920 × 1080 (the window request is logical at scale 1).
  `apps/sandbox`, which keeps its own parser, does not take it.

- **The HAL can be asked what the display is doing with presented frames, not
  just what was requested.** `crcbl_hal::DisplayTiming` is a new four-state
  answer — `Unknown`, `Fixed { cycle }`, `Variable { shortest }` and
  `Stepped { cycle, step }` — returned by the new
  `Device::display_timing(swapchain)`, and gated by the new
  `Features::PRESENT_TIMING` (outside `TIER_A`, like `PRESENT_FEEDBACK`). A
  `PresentMode` is a request; this is the observation, and it is the only thing
  in the seam that distinguishes a fixed 60 Hz panel from an adaptive one
  currently sitting at 60 Hz. **It is a live query — the answer changes when a
  laptop enters power-saving mode or a window moves to another monitor — so
  callers must not cache it.** A device without the capability answers
  `Ok(DisplayTiming::Unknown)` rather than erroring, exactly as
  `wait_until_presented` answers `Ok(())`; a foreign or destroyed swapchain is
  still `ForeignObject`/`InvalidHandle` on every backend. The free function
  `display_timing_from_refresh_nanos` is the conversion from a presentation
  engine's two nanosecond figures, exposed and unit-tested on its own because
  every subtle mistake in this feature lives there. `crcbl-vk` implements it
  against `VK_EXT_present_timing` through hand-written FFI (`ash` has no
  bindings for it); `crcbl-wgpu`, `crcbl-mtl` and `crcbl-dx12` answer `Unknown`
  and document what their platform would need to do better.

  **The engine reads it once, at start-up, and paces on the answer.**
  `GpuContextDesc::default()` asks for `Features::PRESENT_TIMING` beside
  `PRESENT_FEEDBACK`, so the extension chain is negotiated on a device that has
  it, and `GpuContext::submit_and_present` queries after its **first** present —
  after, because the platform may report nothing until an image has been
  presented; once, because a driver that only ever answers `Unknown` would
  otherwise be asked again every frame for the life of the process. The outcome
  is one `info` line beginning `hal: display timing `, naming all three of what
  was asked, what the display reported and what is in force
  (`hal: display timing Unknown; asked for Auto, pacing Vsync`), so "asked for
  `Auto` and the display said `Variable`" is distinguishable in a log from
  "asked for `Adaptive`". A failed query degrades to `Unknown` and a `debug`
  line; it never fails a frame that has already been presented. Resizes,
  display-mode changes and out-of-date presents do **not** re-run it — a window
  dragged onto a VRR monitor keeps the pacing it started with until the game
  asks for another.

- **`Pacing::Auto`, and games can switch pacing at runtime.**
  `GpuContext::set_pacing(Pacing)` changes how frames are paced mid-run,
  rebuilding the swapchain **only** when the present mode it resolves to differs
  from the one presenting — so a settings screen that re-applies every value on
  every apply costs nothing. `GpuContext::pacing()` reports what was asked for
  and `GpuContext::effective_pacing()` what is actually in force (never `Auto`),
  because a caller that asked for `Auto` and got vsync needs to tell that from
  having asked for vsync. A failed switch rolls both of them and the swapchain's
  mode back together, leaving the context usable on the pacing it already had.

- **The demo site is served cross-origin isolated, and the browser gate asserts
  it.** `web/tools/serve.mjs` is a new static server that sends
  `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp` — the pair a browser requires
  before it will hand out `SharedArrayBuffer`, and therefore before any wasm
  build with `+atomics` can run. `web/build.sh --serve` runs it in place of
  `python3 -m http.server`, and `web/tools/browser-e2e.mjs` imports it instead
  of keeping a second server of its own, so the origin the gate checks is the
  origin a human loads. Group A now asserts `crossOriginIsolated === true` and
  that `new WebAssembly.Memory({ shared: true })` actually succeeds, and
  `run-browser-e2e.sh` fails a run whose output does not contain that check by
  name — the headers are otherwise something nothing in the repository would
  notice going missing.

  `--serve` binds loopback only now, where `python3 -m http.server` bound every
  interface: `http://<lan-ip>:8000` is not a secure context, so it would have
  served a page that looked right and was not isolated.

  This is the local half of the question only. GitHub Pages cannot set either
  header, so the published demos are still not isolated; see `docs/backlog.md`.

- **`apps/horde` steers its crowd on the job pool, and `--workers` is the switch
  that proves it deterministic.** The separation pass — one broadphase
  neighbourhood query per enemy per tick, which is the workload the sample
  exists to produce — now decides every velocity through
  `crcbl_jobs::Pool::par_for` in chunks of 64, and writes them back serially.
  `--workers <N>` sizes the pool; `--workers 0` gives the pool a spawner with no
  threads, which is the shape the browser gets, so the design's `--threads 1`
  versus `--threads N` comparison can be run on one machine. Nothing about the
  game changes: the results are bit-identical at every worker count, because the
  chunk boundaries never depend on it and each enemy's arithmetic — including
  the neighbour sum, whose floating-point order is the BVH's — is untouched by
  which thread ran it.

  Measured on a 32-core machine, 600 headless frames with `--prefill 6000` and
  the null backend: **8.38 s at `--workers 0`, 1.48 s at the default**. There is
  no `cfg(target_arch)` in the sample; `crcbl_jobs::default_spawner` answers the
  browser question, and on `wasm32` the pool has no workers and runs every chunk
  on the calling thread.

  The other four samples were left alone. Breakout's forty bricks, flappy's
  handful of pipes and asteroids' forty-four rocks are smaller than one chunk,
  and sandbox and bare have no per-frame collection at all — a `par_for` over
  any of them would be slower than the loop it replaced.

- **`crcbl-phys` can answer overlap queries under a shared borrow, so several
  threads can ask at once.** `PhysicsWorld::overlap_queries` and
  `PhysicsSystem::overlap_queries` take `&mut self` once, build the broadphase,
  and hand back an `OverlapQueries` / `EntityOverlapQueries` — `Copy`, `Sync`,
  and valid only while the world cannot be mutated, which is the type system
  enforcing what a comment would otherwise have to ask for. Their
  `overlap_sphere_into` takes a caller-owned `QueryScratch` in place of the
  world's own buffers, so a data-parallel pass gives one to each thread and
  still allocates nothing in the steady state. The `&mut self` forms are
  unchanged for callers and now delegate to the same traversal, so there are not
  two implementations to drift apart.

- **The umbrella re-exports the job system as `crcbl::jobs`**, so a game reaches
  `Pool`, `par_for` and `default_spawner` without naming a second workspace path
  — the same arrangement the other nine simulation crates already had.

- **`crcbl-jobs` has a work-stealing pool, and `par_for` works with or without
  threads.** `Pool::new(spawner)` sizes itself to `Spawn::parallelism` minus the
  thread that drives it, `Pool::with_workers(spawner, n)` names a count for a
  caller that knows what else is running, and `Pool::par_for(items, chunk, f)`
  calls `f(start, chunk)` once per fixed-size chunk of a `&mut [T]`. The pool is
  built through the `Spawn` seam rather than `std::thread`, which is what gives
  it a browser story: on a spawner with no threads it has no workers and runs
  every chunk on the calling thread, and **the chunk boundaries come from the
  caller's chunk length and the slice, never from the worker count**, so the
  same call reaches the same closure calls and the same bytes in both modes.

  `par_for` takes `&mut self`, so one thread drives a pool at a time; a
  subsystem that wants its own parallelism builds its own pool. The driving
  thread is a participant rather than a waiter — it runs chunks off its own end
  of the deque while the workers steal from the other — so a call completes even
  if no worker ever wakes, and waking one is throughput rather than correctness.

  **A panicking chunk does not poison the pool**: it is caught where it runs,
  the other chunks still run, and the panic is re-raised on the calling thread
  afterwards. Where several panic, the lowest-numbered chunk's is the one
  re-raised, so the failure reported is the same with and without threads.
  Dropping the pool wakes every parked worker and returns without waiting for
  them, because the seam detaches its threads by design.

  The deque behind it is a **bounded Chase-Lev**, written here rather than
  taken: `crossbeam-deque` is the ecosystem's answer and is not in this
  workspace's lockfile, and adding a dependency is not this crate's call.
  Bounded because growing is what forces epoch-based reclamation; a push the
  queue will not take is run on the spot. Its slots hold pointers in atomics so
  that the speculative read a thief does before it knows the item is its cannot
  be a data race.

- **The seam can now be asked when a frame actually reached the display, and the
  engine asks.** `Device::wait_until_presented(swapchain, present_id, timeout)`
  blocks until a numbered present has completed, `PresentInfo::present_id`
  numbers it, and `Features::PRESENT_FEEDBACK` says whether a device can answer.
  The names are the capability's rather than any one platform's, because the
  three that have it disagree on the shape — one numbers a present and blocks on
  the number, one hands out a waitable object with no number, one only calls
  back once a drawable has been shown — so the id is the caller's currency and
  each backend maps it onto whatever it has.

  **A device without the capability returns `Ok(())` at once rather than
  refusing**, which is what keeps the wait out of every caller's per-frame
  branching: a condition that cannot change after device creation should not be
  re-tested every frame, and a caller that skipped the test would turn a missing
  capability into a failed frame. So does a `present_id` the backend has no
  record of — never presented, or from before the last `reconfigure_swapchain`,
  which restarts the numbering.

  `crcbl::engine`'s `GpuContext::acquire` waits for the present
  `FRAMES_IN_FLIGHT` behind the frame it is about to start, before it takes an
  image and before any work is recorded. Not the frame just submitted: that
  drains the pipeline to a single frame and costs more than not waiting at all.
  `Pacing::Off` waits for nothing, since being paced by the display is the one
  thing that mode exists to avoid, and `PRESENT_WAIT_TIMEOUT` bounds the wait so
  a compositor that stopped answering cannot hang the loop. The frame limiter is
  unchanged and still needed — it answers "am I running faster than the cap",
  which is a different question.

  **`crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` implement it**; `crcbl-wgpu` and
  the null backend still answer immediately and advertise nothing.

- **`crcbl-dx12` presents to a window, and paces on the display while doing
  it.** `Instance::create_surface` finally reads `SurfaceTarget::Win32`'s
  `hwnd`, `Instance::surface_caps` answers from DXGI and the window, and
  `Device::create_swapchain` / `reconfigure_swapchain` / `acquire_next_frame` /
  `present` / `destroy_swapchain` build and drive a
  `DXGI_SWAP_EFFECT_FLIP_DISCARD` swapchain on it. Every other `SurfaceTarget`
  variant is refused by name, and the two kinds of refusal stay apart:
  `Offscreen` names an unwritten slice, while a Wayland, XCB, AppKit or canvas
  target names the backend that owns it, because D3D12's only presentation
  target is an `HWND`.

  **`surface_caps` offers only what `CreateSwapChainForHwnd` will accept.**
  Flip-model takes four back-buffer layouts and rejects everything else, so the
  format list is those four plus the two sRGB spellings — presented the way
  D3D12 requires, as a linear back buffer under an sRGB render target view,
  which is the one differing-format cast this backend permits. Present modes are
  `Fifo` always and `Immediate` **only where the factory reports
  `DXGI_FEATURE_PRESENT_ALLOW_TEARING`**, since a flip-model present with a zero
  sync interval and no tearing flag does not tear and offering it would be a
  mode that does not do what its name says. `Mailbox` and `FifoRelaxed` are
  absent: DXGI has neither. `current_extent` comes from `GetClientRect`, which
  is the only thing on Windows that knows.

  Acquire is the **implicit** shape the seam already documents for `crcbl-wgpu`
  and `crcbl-mtl` — the index comes from `GetCurrentBackBufferIndex`, so both
  semaphores are `None` — and `suboptimal` is always `false` and
  `SurfaceError::OutOfDate` never produced, because DXGI has no such condition
  and inventing one would put a frame loop into an unending reconfigure.

  Present feedback ships in the same change rather than after it, because
  `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT` is a **creation** flag:
  designing the swapchain without it would have meant replacing it immediately.
  `Features::PRESENT_FEEDBACK` is reported for every adapter — `IDXGISwapChain2`
  predates D3D12, so there is no machine where it would have been probed and
  come back no — and `Device::wait_until_presented` blocks on
  `GetFrameLatencyWaitableObject`'s handle. That handle carries **no id**, so
  the backend keeps its own record of the ids it was given and answers the
  seam's immediate cases from it: zero numbers nothing, and an id above the
  highest this swapchain object presented names a frame it was never asked for —
  a present that failed after the caller spent the id, or one from before a
  `reconfigure_swapchain`, where `ResizeBuffers` restarts the numbering.

- **A Vulkan device paces on the display where the driver can say when a frame
  landed.** `crcbl-vk` requests `VK_KHR_present_id` and `VK_KHR_present_wait`,
  chains `VkPresentIdKHR` onto each present and answers
  `Device::wait_until_presented` with `vkWaitForPresentKHR`. The pair is
  optional and asked for only after `vkEnumerateDeviceExtensionProperties` lists
  both and `vkGetPhysicalDeviceFeatures2` returns both feature bits — requesting
  an absent device extension fails `vkCreateDevice` outright — so
  `Features::PRESENT_FEEDBACK` on an `AdapterInfo` or a `DeviceCaps` means the
  device really can answer. It is driver-dependent in practice: radv has the
  pair, lavapipe does not.

  `GpuContextDesc::default()` now asks for `PRESENT_FEEDBACK` among its optional
  features, so a game built on the engine gets the closed loop without naming
  it. A device that does not have it keeps the open-loop frame limiter, exactly
  as before.

  Three cases still answer at once rather than blocking, because
  `vkWaitForPresentKHR` would otherwise sit out the whole timeout for a frame
  that will never arrive: an offscreen image ring, which has no `VkSwapchainKHR`
  at all; an id whose present failed with `OutOfDate` after the caller had
  already spent it; and an id from before a `reconfigure_swapchain`, which
  builds a new swapchain object that never saw it.

- **A Metal device paces on the display too, and every Metal device can.**
  `crcbl-mtl` reports `Features::PRESENT_FEEDBACK` unconditionally and answers
  `Device::wait_until_presented` from `MTLDrawable::addPresentedHandler:` —
  Metal numbers no present and offers nothing to block on, so `present` attaches
  a handler carrying the caller's own `PresentInfo::present_id` and the wait
  sleeps on a condition variable until that number is reported back. The flag is
  unconditional because the handler is a plain drawable method with no query
  behind it; there is no Metal device that cannot answer.

  The flag is on the **device** while the drawable is a property of a
  **swapchain**, so a device driving the offscreen ring advertises it and its
  ring still answers every wait at once, through the seam's own "nothing to wait
  for" case. Withholding the flag instead would make every macOS window
  unpaceable, since the seam then requires an immediate `Ok(())` forever.

  An id the swapchain was never given also answers at once, and a reconfigure
  restarts the numbering: the ledger belongs to the swapchain and a rebuilt one
  starts empty. An id that does not strictly increase is refused and its present
  goes out unnumbered, with a warning, rather than renumbering the swapchain
  backwards.

  Adds a direct dependency on `block2`, the Objective-C block ABI —
  `addPresentedHandler:` takes a block and there is no other way to reach it. It
  is the same binding family as `objc2` and was already in `Cargo.lock`.

- **The Metal backend's hardware suite now runs in CI, so `crcbl-mtl`'s draws
  are verified by a machine rather than by nobody.** A `mtl e2e (macos-latest)`
  job runs `crates/crcbl-mtl/tests/run-mtl-e2e.sh`, which turns on the `mtl-e2e`
  feature and the crate's `#[ignore]`d tests — the triangle draw, the engine's
  own `triangle.slang` draw through a bind group, the indexed draw and the
  multi-draw-indirect. Those four had never been executed anywhere: they were
  gated on the belief that a CI runner's `Apple Paravirtual device` cannot run a
  shader, which was measured on macos-14 and is not true of the image
  `macos-latest` resolves to today. The tests stay `#[ignore]`d, so a plain
  `--all-features` run on a machine without a usable GPU is still green, and the
  script still fails when the suite reports zero tests run.

  One test is held out of the CI job: the layer swapchain's drawable
  acquisition, which depends on a headless container vending a `CAMetalLayer`
  drawable rather than on shader execution. Running the script on a real Mac
  covers it, and covers a non-virtual GPU besides.

- **`crcbl-shaders` ships DXIL, and `crcbl-dx12` draws a triangle with it.** The
  artifact pipeline grew a fourth target: `dxil/<shader>.<entry>.dxil`, compiled
  in two steps — `slangc -target hlsl` then a **pinned** `dxc` at Shader Model
  6.6 — because Slang's own `-target dxil` shells out to whichever `dxc` it
  finds. `CRCBL_DXC` is required with **no `PATH` fallback**: distributions ship
  Shader Model 6.10 preview builds that abort on a trivial shader, and a
  fallback would find one silently. The script verifies the container signature
  of every artifact it generates, because an unsigned container hashes and
  commits like any other and is then refused by every real D3D12 driver.

  DXIL is the one target with an artifact **per entry point** — `dxc` compiles a
  single `-E`, and a D3D12 pipeline takes one blob per stage — so
  `crcbl_shaders::EntryPoint::dxil` and `Shader::dxil(entry_point)` are
  per-entry-point where `Shader::wgsl` and `Shader::msl` are per-shader, and
  `spirv/manifest.txt` records one `dxil` line per entry point beside a
  `dxc-version` and `dxil-model` pin.

  `crcbl-dx12` consumes it: shader modules over validated DXIL containers, root
  signatures from pipeline layouts, bind group layouts and bind groups over a
  shader-visible descriptor heap, `D3D12_GRAPHICS_PIPELINE_STATE_DESC` built
  from the seam's descriptors, and `bind_graphics_pipeline` / `bind_group` /
  `draw` on the encoder. The measurement is a triangle drawn through the real
  seam and read back with its texels asserted, not a call that returned `Ok`.

  Still refused by name: compute pipelines, indexed and indirect draws, index
  buffers, dispatches, query sets, semaphores, swapchains, dynamic offsets and
  push constants — the last two as `InvalidDescriptor` rather than
  `Unsupported`, because a descriptor table has no offset to apply and this
  device reports no `PUSH_CONSTANTS`.

- **`crcbl-shaders`: the UI shaders declare their resources in binding order,
  which is what makes text appear on Metal.** `ui.slang` and `ui_tier_b.slang`
  declared `constants` first while numbering it last, and Slang's Metal target
  assigns argument-table indices in _declaration_ order — so their MSL bound
  `constants` at `buffer(0)` and `vertices` at `buffer(1)`, while `crcbl-mtl`
  flattens `(set, binding)` by ascending binding number and bound them the other
  way round. The UI vertex stage read the viewport constants as its vertex
  array: every quad went nowhere, silently, and macOS ran flappy with no HUD, no
  score and no menu labels. Reordering the two declarations fixes it; SPIR-V and
  WGSL are byte-identical afterwards, because `[[vk::binding]]` already pinned
  those. `crcbl_mtl::binding` carries the rule and the obligation it puts on new
  shaders.

- **The Metal backend is selectable, and on macOS it is what `open()` picks.**
  `crcbl`'s GPU registry grew a `GpuBackend::Metal` entry behind
  `cfg(target_os = "macos")`, so `crcbl-mtl` is finally reachable from a game:
  `--backend mtl` (or `metal`, or `CRCBL_GPU=mtl`) names it, and an ordinary run
  with no flag gets it. This is the wire-up every Metal slice since MTL1
  deferred — a registry entry for a backend that could not yet hand back a
  device would have been a path that exists only to fail — and MTL2 through MTL6
  landed the device, the swapchain, pipelines, bind groups and draws it was
  waiting on.

  **Vulkan is still registered on macOS but is no longer selected automatically
  there.** Apple platforms are Metal only per
  `docs/plan/09-backends-metal-dx12.md`'s 2026-08-05 correction, and a Mac
  without MoltenVK has no `libvulkan.dylib` for `ash` to `dlopen` at all — so
  what the old order produced was not a fallback but the only outcome: every
  sample on macOS exited with "no GPU backend available (tried: vk)" and a hint
  to run the null backend. `CRCBL_GPU=vk` still reaches Vulkan by name for
  whoever installed a loader and means it. Selection elsewhere is unchanged:
  Vulkan on the rest of native, wgpu in a browser, and null never automatic
  anywhere.

- **`crcbl-shaders`**: `COMPUTE_PROBE`, the crate's first **compute** shader —
  every other source it ships is a drawing shader. `shaders/compute_probe.slang`
  squares a `StructuredBuffer<uint>` element-wise into an `RWStructuredBuffer`,
  bounded by a `count` in a uniform buffer, with SPIR-V, WGSL and MSL artifacts
  and a manifest entry like every other shader. The companion
  `crcbl_shaders::compute_probe` module carries `WORKGROUP_SIZE` and the
  `Params` uniform layout, so a caller computing its dispatch size reads the
  number the shader declares rather than one it remembers; a unit test reads the
  `.slang` source and fails if the two drift.

  It exists to make the compute half of `crcbl-hal` testable against a real
  driver — a dispatch that silently does nothing returns `Ok` too — and the MSL
  and WGSL artifacts are emitted even though no Metal or wgpu code path
  dispatches compute yet, because the compile script drives all three targets
  and the manifest hashes all three.

- **`crcbl-mtl`**: a new crate, opening P14 with **the only path to a GPU on
  macOS and iOS** — Apple platforms are Metal only, per the 2026-08-05 platform
  decision, so nothing else reaches a device there. Its first slice is **adapter
  enumeration and nothing else**: `MetalInstance::open` calls
  `MTLCopyAllDevices` and turns every device into an `AdapterInfo` whose
  `DeviceCaps` come from real queries — `argumentBuffersSupport` for
  `DESCRIPTOR_INDEXING`, the `MTLGPUFamily::Metal3` query for
  `BUFFER_DEVICE_ADDRESS`, `supportsBCTextureCompression`, and `maxBufferLength`
  / `maxThreadsPerThreadgroup` / a `supportsTextureSampleCount:` probe for the
  limits Metal will answer before a device exists.

  **Every other entry point refuses by name.** `create_surface`, `surface_caps`
  and `request_device` return `HalError::Unsupported` whose `what` says which
  slice the answer arrives in, so a caller reads "not yet" rather than "broken";
  an out-of-range adapter still gets `NoSuchAdapter`, because that is a caller
  bug this slice can genuinely diagnose and hiding it behind a refusal would
  lose it.

  **It advertises Tier B today, and that is not a claim about Metal.**
  `DeviceCaps::tier` is derived from `Features` precisely so a backend cannot
  assert a tier it has not earned, and `DRAW_INDIRECT_COUNT` /
  `MULTI_DRAW_INDIRECT` wait on the indirect-command-buffer decision the command
  slice makes. The hardware is Tier A; this backend is not yet.

  Off macOS the crate is documentation with no public items, so `objc2-metal` is
  never fetched or built there. Nothing instantiated it at this slice — no app,
  and no entry in the engine's backend selection; the registry entry above is
  what closed that, once there was a device to hand back.

  **Its second slice opens a real device.** `request_device` now checks adapter,
  then required features, then `compatible_surface`, and hands back a
  `PendingDevice` that completes on its first poll. `MetalDevice` implements the
  resource half of the seam — buffers, images, image views and samplers in
  `crcbl-core` `Pool`s, plus `write_buffer`, `queue` and a `wait_idle` that
  really commits a command buffer and waits on it. The instance now keeps its
  `MTLDevice` objects behind an `Arc` shared with every device it opens, which
  is how the seam's "a `Device` outlives its `Instance`" obligation is
  discharged.

  `MemoryLocation` maps to `Private` for `DeviceLocal` and `Shared` for both
  host locations. **`Managed` is deliberately never produced**: it is the
  two-copy mode and both directions need a call this slice does not have, so
  choosing it for readback would return stale bytes on an Intel Mac and correct
  ones on Apple silicon — right on one class of Mac only. `write_buffer` refuses
  `DeviceLocal` by name rather than silently writing nothing, matching what
  `crcbl-vk` answers for the same call.

  All 29 seam formats have an exact `MTLPixelFormat` counterpart, and the
  mapping is tested for **injectivity** — two formats sharing one Metal format
  is invisible at run time (the image is created, the sample succeeds, the
  colour is wrong), and it is the same class of defect as the missing sRGB
  encode that made the browser build render too dark.

  **Its third slice records and submits GPU work, and produces the first
  pixel.** `MetalCommandEncoder` owns one open Metal encoder at a time and
  closes it before opening another, because a second concurrent encoder raises.
  `begin_render_pass` builds a real `MTLRenderPassDescriptor` — colour slots,
  MSAA resolve folded into the store action, the reversed-Z depth clear passed
  through untouched, stencil attached only when the view's format has a stencil
  plane. Copies go through `MTLBlitCommandEncoder`, `submit` takes waits and
  signals, timeline semaphores are `MTLSharedEvent`, and readback is request /
  poll / destroy that genuinely observes command-buffer completion rather than
  assuming it. Draws and dispatches fail the encoder rather than being dropped,
  so `finish` returns the refusal instead of a command buffer that submits and
  draws nothing.

  **`pipeline_barrier` ends the open blit encoder and records nothing else — the
  encoder boundary is the barrier.** Metal tracks hazards automatically between
  encoders for resources whose `hazardTrackingMode` is `Tracked`, which is the
  default for everything allocated straight from an `MTLDevice`, and this
  backend allocates nothing else. What would break that — heaps, parallel render
  encoders, a barrier inside a pass — is written down where the decision is, and
  a test asserts the premise on real objects instead of trusting it.

  A submission may no longer wait on a timeline value that only a _later_
  submission signals. With one queue and no CPU-side signal in the seam that can
  never be satisfied, and its failure mode is a queue that stops with the
  process alive and nothing in any log — so it is refused up front, by name.

  **Its fourth slice compiles shaders and draws.** `create_shader_module` goes
  through `newLibraryWithSource:options:error:` and carries Metal's own
  `NSError` text into `HalError::ShaderCompilation`, because that message is the
  only debugging aid a shader author gets. Graphics and compute pipelines build,
  and a draw paints pixels a test asserts exactly.

  **That draw test does not run in CI, and the reason is the runner rather than
  the code.** GitHub's `macos-latest` exposes an `Apple Paravirtual device` that
  hangs the command buffer on any shader execution — measured, with both
  encoders reporting `completed` rather than faulted. The test is therefore
  feature-gated behind `mtl-e2e` and `#[ignore]`d, run by
  `crates/crcbl-mtl/tests/run-mtl-e2e.sh` on a real Mac. Metal has no software
  rasteriser to substitute the way lavapipe does for Vulkan, so this is a
  coverage gap rather than a workaround; `docs/backlog.md` states it as one.
  Everything short of shader execution — clears, blit copies, semaphores,
  readback, MSL compilation, pipeline-state creation — does run there, and does
  pass.

  **An `MTLRenderPipelineState` is only half of `GraphicsPipelineDesc`.** Cull
  mode, winding, fill mode, depth clip, depth bias, the depth/stencil state and
  the primitive topology are all encoder or draw-call state in Metal rather than
  pipeline state, so they are stored beside the pipeline object and replayed
  when it is bound — otherwise half the descriptor would silently not apply.

  The engine's own `triangle.slang` **compiles into a real pipeline** but is not
  yet drawn: it pulls vertices from a `StructuredBuffer`, which needs bind
  groups, and those are still refused. The pixel test draws a resource-free
  `[[vertex_id]]` triangle instead, generated from the same constant the
  assertion uses so the two cannot drift.

- **`crcbl-mtl`** presents. Its fifth slice adds surfaces over `CAMetalLayer`
  for `SurfaceTarget::AppKit`, an offscreen image ring for
  `SurfaceTarget::Offscreen`, and the whole swapchain half of the seam —
  `surface_caps`, create / reconfigure / destroy, `acquire_next_frame` and
  `present`. **macOS now has a native GPU path from window to pixel**, which
  since the 2026-08-05 platform decision it otherwise did not have at all.

  **The offscreen ring is the half CI can actually run**, and it does: acquire →
  render-pass clear → barrier → blit → submit → present → readback, with the
  exact texels asserted, on the runner's real (if paravirtual) device. Acquiring
  a `CAMetalLayer` drawable needs a display, so that one test is gated behind
  `mtl-e2e` like the triangle.

  **No semaphore is created for WSI, and that is Metal's shape rather than a
  shortcut.** `nextDrawable` blocks the CPU and returns a ready texture, so
  there is no presentation-engine signal to reconcile: `acquire_semaphore` and
  `present_semaphore` are both `None`, the implicit-acquire form the seam
  already documents for `crcbl-wgpu`. Presenting goes through
  `MTLCommandBuffer::presentDrawable:` rather than `MTLDrawable::present`, which
  would hand the drawable over while the GPU may still be writing it.

  A layer is offered `Bgra8UnormSrgb` first and **never the RGBA8 pair** —
  `CAMetalLayer::pixelFormat` raises on RGBA8 — so `preferred_format` lands on
  an sRGB format the layer will actually accept. A test reads the format back
  off the layer and pins it against the conversion table, because the value that
  would make it false is `BGRA8Unorm`: exactly the missing-encode bug that made
  the browser build render too dark.

- **`crcbl-mtl`** binds resources and draws indexed and indirect. Bind group
  layouts, bind groups, `update_bind_group`, pipeline layouts naming them, index
  buffers, `draw_indexed`, `draw_indirect` and `draw_indexed_indirect` are all
  real calls now. **The engine's own `triangle.slang` can finally be drawn** —
  it compiles, builds a pipeline over a layout naming its `StructuredBuffer`,
  binds a real vertex buffer and draws.

  **Bind groups map to flat per-stage argument tables, not argument buffers**,
  and the artifacts decided it: every MSL file `crcbl-shaders` commits declares
  plain arguments (`device Vertex* [[buffer(0)]]`), because Slang's Metal target
  emits no argument-buffer struct. Binding a descriptor block where a shader
  declared a vertex pointer does not fail — the shader reads descriptor words as
  vertex data. So argument buffers were the option that silently draws garbage
  with the shaders that exist. A consequence worth having: directly bound
  resources are made resident and hazard-tracked by Metal itself, so there is no
  `useResource` residency management and MTL3's barrier-is-an-encoder-boundary
  argument stays intact.

  **The backend still reports Tier B, and `DESCRIPTOR_INDEXING` was withdrawn.**
  It had been reported from `argumentBuffersSupport == Tier2` — true of the
  hardware — but flat tables have no runtime-sized array, so the backend refuses
  every `BindingFlags`, and the seam says a backend that refuses them must not
  claim the feature. `MULTI_DRAW_INDIRECT` and `INDIRECT_FIRST_INSTANCE` were
  earned. `DRAW_INDIRECT_COUNT` remains unreachable while the backend encodes
  straight into the command buffer, because Metal's only GPU-count execution
  needs an indirect command buffer populated by a compute pass that would have
  to run before the render encoder exists. Nothing above the seam is affected —
  every layout in `crcbl-render` already uses `BindingFlags::empty()`.

- **`crcbl-dx12`**: a new crate, opening the DX12 half of P14. This first slice
  is **adapter enumeration and nothing else** — surfaces, surface caps and
  device creation refuse by name, while an out-of-range adapter still gets
  `NoSuchAdapter`.

  **D3D12 has no adapter-level capability query.** `CheckFeatureSupport` lives
  on `ID3D12Device` and there is no physical-device object, so enumeration opens
  a device per adapter at feature level 11.0, asks, and drops it. An adapter
  DXGI lists but D3D12 refuses is dropped — and the id counter advances only on
  a kept adapter, or every id past the gap would name the wrong GPU.

  **It exists to settle whether WARP clears Tier A.** WARP is D3D12's software
  rasteriser and ships in Windows; `windows-latest` currently has no GPU at all,
  so Windows has never had golden images or render coverage. Each adapter prints
  its `ResourceBindingTier`, `HighestShaderModel` and SM6.6 dynamic-resource
  answer, and a CI step publishes the line — nextest hides a passing test's
  stdout, which is exactly the run where a measurement wants reading.

  `DESCRIPTOR_INDEXING` is reported from tier 3 **and** shader model 6.6, both
  required and neither implying the other. The indirect features are withheld
  despite `ExecuteIndirect` being a direct fit, because no call in the crate
  makes them true yet — the precedent `crcbl-mtl` set by withdrawing a flag it
  could not honour.

  **Its second slice opens a real device.** `request_device` now checks adapter,
  then required features, then `compatible_surface`, and hands back a
  `PendingDevice` that completes on its first poll; behind it are a real
  `ID3D12Device` and a `D3D12_COMMAND_LIST_TYPE_DIRECT` queue. `Dx12Device`
  implements the resource half of the seam — buffers, images, image views and
  samplers in `crcbl-core` `Pool`s, plus `write_buffer`, `queue` and a
  `wait_idle` that signals an `ID3D12Fence` and blocks on it. The instance now
  keeps its DXGI factory and adapters behind an `Arc` shared with every device
  it opens, which is how the seam's "a `Device` outlives its `Instance`"
  obligation is discharged.

  **An image view is a descriptor, not an object**, so the crate gained a small
  allocator over CPU-visible descriptor heaps — one per D3D12 heap type, grown a
  chunk at a time. One seam view becomes up to four descriptors, because a
  texture that is sampled and rendered to needs an SRV _and_ an RTV; the image's
  `ImageUsage` decides which. Every combination D3D12 has no member for — a
  depth stencil view of a volume, a multisampled UAV, a cube whose layers are
  not whole cubes, a view whose dimensionality is not its image's — is refused
  with `InvalidDescriptor` naming it, because `CreateShaderResourceView` and its
  three siblings return `void` and cannot report anything.

  **`ImageViewDesc::format` must equal its image's format on this backend**,
  which is a documented divergence from `crcbl-mtl`: D3D12 permits the sRGB
  reinterpretation the seam describes only from a typeless resource, or where an
  optional casting capability is reported, and neither is worth making the
  seam's promise machine-dependent or every render target uncompressed. A
  **sampled depth** image is not that case and does work: it is stored typeless
  (`R32_TYPELESS` and friends) with the depth-stencil view and the shader view
  each naming their own concrete format.

  `MemoryLocation` maps to D3D12's three standard heaps, and each gets the only
  initial resource state D3D12 accepts for it. An image on a host-visible heap
  is refused — those heaps hold buffers only — as is `write_buffer` on a
  `DeviceLocal` buffer, which D3D12 reaches through a copy rather than a map.

  Every seam format has an exact `DXGI_FORMAT` and the mapping is tested for
  injectivity, as is the separate typeless/depth-read table: two seam formats
  collapsing onto one API format is invisible at run time — the image is
  created, the sample succeeds, the colour is wrong.

  `TEXTURE_COMPRESSION_BC` and `SAMPLER_ANISOTROPY` are now reported, each
  because the call behind it landed: BC support is measured with a real
  `CheckFeatureSupport(D3D12_FEATURE_FORMAT_SUPPORT)` per BC format, and
  `max_sampler_anisotropy` moves to `D3D12_REQ_MAXANISOTROPY`. The tier is still
  B on every adapter.

  **Its third slice records, submits and clears.** `Dx12CommandEncoder` is a
  real `ID3D12GraphicsCommandList` over its own `ID3D12CommandAllocator`, taken
  when the encoder is created so a queue handle from another device is a
  `ForeignObject` the encoder carries to `finish`. `begin_render_pass` binds
  attachments with `OMSetRenderTargets` and honours `LoadOp::Clear` through
  `ClearRenderTargetView`/`ClearDepthStencilView`, with viewport and scissor set
  from the pass's render area; `Device::submit` runs the lists on the queue and
  signals an `ID3D12Fence`; `request_readback`/`poll_readback` observe that
  fence and map the buffer. So a cleared pixel is now written, copied and **read
  back and asserted** rather than assumed — which is the measurement
  `docs/backlog.md` asked for about whether WARP can execute anything at all, as
  opposed to merely reporting `ResourceBindingTier=3`.

  **A clear honours `RenderPassDesc::render_area`, unlike `crcbl-mtl`.** D3D12's
  clears take a rectangle list, so the area is passed through — Vulkan's
  semantic; a Metal `loadAction` clears the whole attachment whatever the pass
  said. `StoreOp::Discard` is honoured as `Store`: `OMSetRenderTargets` has no
  store op, and storing when the caller did not need it is slower and never
  wrong.

  **`destroy_*` freeing a resource with work in flight is no longer a
  use-after-free.** A D3D12 command list retains nothing it references, so the
  encoder now takes its own reference to every resource it records against and a
  submission parks that set on a fence-keyed retire queue — along with the
  command list and allocator, which `ExecuteCommandLists` does not retain
  either. `destroy_buffer` and its siblings are unchanged and still free on the
  spot, because the reference keeping the resource alive is the submission's.
  That is a smaller mechanism than `crcbl-vk`'s deletion queue needs, and the
  reason is that COM refcounts the bookkeeping Vulkan handles cannot.

  `pipeline_barrier` becomes `ResourceBarrier` transitions, per subresource or
  whole-resource; a barrier on a host-visible buffer is dropped rather than
  recorded, because D3D12 pins upload and readback resources to one state for
  their lifetime. Buffer↔buffer and buffer↔image copies are recorded, with
  D3D12's 256-byte row pitch and 512-byte placement alignments refused by name —
  neither is expressible in `BufferImageCopy`, and `CopyTextureRegion` returns
  `void`, so an unaligned footprint would arrive as a readback of the wrong
  bytes. Draws, dispatches, bind groups, push constants, index buffers, buffer
  fills, image-to-image copies, MSAA resolves and read-only depth attachments
  all **fail the encoder**, so `finish` returns the refusal rather than a
  command buffer that submits and does nothing. Semaphore waits and signals on a
  submission, and `ReadbackDesc::after`, are `InvalidHandle` — no semaphore
  exists to have issued one.

  Everything past that — pipelines, bind groups, shader modules, swapchains and
  queries — still refuses by name.

- **`crcbl-shaders`** now emits **MSL** beside the SPIR-V and WGSL. Slang's
  `-target metal` output is committed as `msl/*.metal`, hashed into
  `spirv/manifest.txt` exactly like the other two, verified by `build.rs` on
  every machine and byte-recompiled by the `shaders` CI job. `Shader::msl()`
  joins `.spirv()` and `.wgsl()`. Regenerating left every existing `.spv` and
  `.wgsl` byte-identical, which is independent evidence the pinned `slangc` is
  the one the artifacts were built with.

- **`crcbl-hal`**: `ShaderModuleDesc` gained `msl`, and `ShaderSources` gained
  `MSL`. A backend that can only compile one language now reports the gap by
  name, so an MSL-only descriptor handed to `crcbl-vk` says so rather than
  failing obscurely. Every call site in `crcbl-render`, `crcbl-vk`, the null
  backend and the seam suites was updated in the same change.

- **`crcbl-jobs`**: a new crate, opening P5B with **the seam every engine thread
  will start through**. `Spawn` has three methods — `threaded`, `parallelism`
  and `spawn` — with two backends behind it: `Threads` over `std::thread`, and
  `Inline`, which has none and refuses every spawn by name. `default_spawner`
  picks between them and is the only place in the threading model that spells
  `cfg(target_arch)`.

  **It exists because `std::thread::spawn` compiles on `wasm32-unknown-unknown`
  and fails at run time.** `std`'s wasm-with-atomics arm takes only `sleep` from
  `thread/wasm.rs`; `Thread::new` comes from `thread/unsupported.rs` and returns
  `UNSUPPORTED_PLATFORM`, because a wasm module cannot instantiate its own
  worker — only the host can, against the shared `WebAssembly.Memory`. A pool
  written on `std::thread` would therefore compile for the browser and have no
  browser story, so the seam lands before anything is built on it. `Threads` is
  not nameable on `wasm32` at all, which makes reaching for it there a compile
  error rather than a run-time `Err`.

  **Degrading is a decision, not an error**: `Spawn::threaded` is asked once
  while a subsystem is being built, and a caller picks a long-lived thread or a
  tick-driven loop from the answer. A `spawn` that fails afterwards is a real
  error, and the closure is gone by then either way.

  Not here yet: the SPSC rings, the work-stealing pool and `par_for` — the
  slices above this one — and the browser's worker backend.

- **`crcbl-jobs`**: `mailbox` — the latest-wins triple buffer a _state_ crosses
  a thread boundary through. One producer publishes complete states at its own
  cadence, one consumer takes the newest, and neither ever waits: a slow
  producer publishes less often and a slow consumer skips the states in between.
  `Publisher::publish` swaps an index rather than copying the payload, and
  `Subscriber::read` always returns a whole state — never an `Option`, because a
  frame drawn from a state one tick old is the outcome this design prefers to a
  frame that waited. `Subscriber::has_new` is the staleness the profiler will
  report.

  Three slots, because that is the count at which neither side ever waits: one
  the producer owns, one the consumer owns, one in the handoff. The `unsafe`
  rests on `{producer, handoff, consumer}` staying a permutation of `{0, 1, 2}`
  — both sides only ever exchange their own index with the handoff's — and the
  tests assert that permutation directly after every operation rather than
  arguing for it. `crcbl-jobs` joins the weekly Miri job, which runs the
  two-thread stress test for real: a torn read is reported there as a data race,
  and nothing else in this workspace can detect one.

  **Deliberately not for streams.** Input edges, audio commands and net packets
  must not be droppable, and this drops by construction — a 3 ms tap between two
  reads would simply not be there. Those want the ring below.

- **`crcbl-jobs`**: `ring` — the bounded SPSC queue a _stream_ crosses a thread
  boundary through, and the opposite discipline to the mailbox. Every item is
  delivered, in order; a producer that outruns its consumer is refused rather
  than allowed to overwrite. `Producer::push` hands the item **back** in a
  `Full<T>` rather than dropping it, so shedding load is always the caller's
  decision and never a silent one, and `Consumer::overflows` counts the refusals
  for the profiler. Capacity rounds up to a power of two so the index wrap is a
  mask.

  **Drop-oldest is not implemented**, though the design lists it as a policy: it
  cannot be done from the producer, because the read cursor belongs to the
  consumer and advancing it would make the producer a second writer to it —
  which is what makes an SPSC ring cheap in the first place. Documented at the
  module and recorded in `docs/backlog.md` rather than left to be discovered.

  Both primitives run under the weekly Miri job. **The memory orderings are
  checked by Miri and by nothing else**, and that is the hardware's doing rather
  than the suite's: on x86-64 a `Release` store and a `Relaxed` one compile to
  the same instruction. Measured — weakening the ring's push to `Relaxed` left
  the whole suite green while Miri reported the data race in `pop`.

- **horde**: **health potions, dropped by brutes and drunk by walking over
  them.** A `potion` frame in `apps/horde/assets/actors.crpix` — a stoppered
  flask in a crimson that appears nowhere else in the sheet, drawn to the same
  14-texel collider the gem is — and a `game::PickupKind` that says what walking
  over a thing pays out. A potion is a **variant of the existing pickup**, not a
  second population: the same `Vec`, the same entity index, the same
  `MAX_PICKUPS` ceiling, the same trigger collider and the same collection
  query, so the soak test's two exact leak equalities and its entity growth
  bound are unchanged rather than each gaining a term.

  **Brutes only, and one brute in twenty.** The brute does more contact damage
  than the other two kinds together and is the one slow enough to walk away
  from, so the heal is paid out by the fight that cost the hit points — the same
  argument `EnemyKind::xp` already makes for experience. The rate came off a
  measurement rather than a feel: at one brute in three the kiting soak
  (`a_long_run_leaks_nothing`) stopped reaching a death at all, which is contact
  damage ceasing to be the pressure the genre is made of. Over the first hundred
  seconds of the default seed it is now **2 potions from 219 kills**, and
  `potions_drop_from_brutes_at_the_rate_the_constant_says` is where that is
  measured.

  **The roll is simulation state.** `game::drops_potion` hashes the run's kill
  counter under a `LOOT_HAND` salt on the run seed — the same construction the
  prop scatter uses, for the same reason — so a drop is identical in a replay,
  on a server and on a client. `the_same_script_replays_bit_identically` now
  compares the loot on the ground and the potion count as well.

  Healing clamps to `Stats::max_hp` and **never to `PLAYER_MAX_HP`**, through a
  new `heal_player` that `Upgrade::Vitality` now shares: the ceiling moves when
  a run takes that upgrade, and a heal clamped to the constant would stop paying
  out with nothing on screen to show it. A potion is worth a quarter of the
  starting bar (`POTION_HEAL`), which is a couple of seconds inside the mass and
  under one inside a brute.

  A **sixth spatial cue**, `audio::SOUND_HEAL`, rather than a second use of the
  gem's: a gem sounds for very nearly every kill and a potion for about one kill
  in a hundred, and the rarest event in the game played through the most common
  sound is the same as not playing it.

  `game::XP_RADIUS` is renamed **`game::LOOT_RADIUS`**, since it is now the
  collider of both pickups; `RenderState`'s `PickupView` carries a `kind`. The
  batch count is unchanged — the potion is another frame of the one actors
  sheet.

- **horde**: **trees and bushes scattered over the arena, and the player cannot
  walk through them.** A new `apps/horde/assets/props.crpix` — one 36-texel
  frame size, a 0.9-unit tree and a 0.5-unit bush, each drawn to its own
  collider to the texel — on a sheet and a layer of its own between the grass
  and everything that moves. `game::scatter_props` deals them from a jittered
  lattice as a pure function of the game's seed, so two games built from one
  `Setup` stand in the same arena on every machine and in a replay.

  **The collision is the player's alone**: enemies walk through props and bolts
  fly through them, which is `docs/plan/sample/03-horde.md`'s hard cap on
  pathfinding doing its job — a prop the horde had to route around would be an
  obstacle query per enemy per tick on the one loop this sample exists to keep
  flat. So a prop is a `game::PropView` in a plain `Vec` with no entity and no
  collider, and the soak test's two exact leak equalities are unchanged.
  `game::push_out_of_props` moves the player to the nearest point on the prop's
  surface, so walking into a trunk off-centre slides round it rather than
  sticking, and it runs beside the arena clamp — props first, then the wall,
  which is the order that terminates. The lattice keeps every prop far enough
  from a wall that the clamp can never hand the player back to one, and far
  enough from the spawn that a run never starts inside a tree; both are `const`
  assertions as well as tests.

  `RenderState` carries `props`, `SceneStats` gained a `props` count beside
  `ground`, and a populated frame is **four** batches instead of three. The
  claim that number exists to make visible is unchanged and is the one it always
  was: the batch count is flat in the size of the horde, not any particular
  value.

- **horde**: the three enemy kinds are **Diablo II monsters**. The frames in
  `apps/horde/assets/actors.crpix` are renamed and redrawn — `grunt` → `fallen`
  (a horned, hunched imp with a crude bone blade), `runner` → `quill-rat` (a low
  wide body under a fan of spines, on four thin legs) and `brute` → `overlord`
  (a head sunk between two shoulder masses, lit brow, tusks) — and
  `art::enemy_frame` is where a kind is mapped to one. **`EnemyKind`'s variants
  are unchanged**: `Grunt`, `Runner` and `Brute` name the roles the spawn table
  and `EnemyKind::from_roll` reason about, and nothing in `game.rs` moved. Each
  silhouette is still drawn to its own collider to the texel, which is why the
  runner became a quadruped: thirteen texels does not carry a humanoid.

  The palette that came with them is muted earth and blood — three shades of
  blood, three of hide, three of dead skin and one bone — and two new tests say
  where it has to sit.
  `art::tests::the_monsters_sit_between_the_grass_and_the_player_in_luma` puts
  every kind's average texel above the brightest texel of `assets/terrain.crpix`
  and every monster texel below the player's average, and
  `art::tests::the_monsters_have_a_dark_rim_and_the_player_a_bright_one` finds
  the boundary of each silhouette and asserts the dark-rim/bright-rim asymmetry
  in both directions rather than leaving it a sentence in the sheet.

- **horde**: the player is a **wizard**, and it moves like one. Five new frames
  in `apps/horde/assets/actors.crpix` — a standing pose and a four-frame walk
  cycle held four ticks a frame, played from `RenderState::elapsed` so a replay
  animates the way the run it replays did. The wizard **faces the way the input
  last pointed**, not the way the gun is aiming and not the way the arena clamp
  left it moving: `RenderState` carries `player_facing` and `player_walking`,
  and a released key leaves the facing where it was rather than snapping back.
  Facing left is the same art with its `u` range reversed, so there is no second
  column of frames.

  **Bolts leave the head of the staff.** `game::MUZZLE_OFFSET` — a distance
  along the aim — is replaced by `game::STAFF_MUZZLE` and `game::staff_muzzle`,
  a point that mirrors with the figure;
  `art::tests::the_staff_head_is_where_the_muzzle_says_it_is` measures the baked
  art against it, so the orb and the shot cannot drift apart. The gun still
  _chooses_ its target from the player's centre and now _aims_ from the staff,
  which is what stops a shot fired from an offset muzzle travelling parallel to
  the line that would have hit. When the wizard is facing away from its target
  the bolt still starts at the drawn staff and crosses the body; that choice is
  written down on `staff_muzzle`.

  The batch count is unchanged — every frame of the wizard is a frame of the one
  actors sheet — and the hold guard in `apps/horde/build.rs` and `art.rs` is
  real for the first time, because until now no horde clip held a frame for
  longer than the one tick that survives almost any wrong tick↔millisecond
  arithmetic.

- **horde**: a **tiled grass ground** under the field, from a new
  `apps/horde/assets/terrain.crpix` — four 2-unit variants chosen per tile by
  `crcbl::core::rand`'s index hash, so a tile draws the same grass whichever way
  it is walked into and the ground has no visible lattice. Laid over the view
  rather than the arena, so its cost is bounded by the window. The sample's art
  is three sheets now instead of two; `SceneStats` gained a `ground` count and
  reports three batches on a populated frame instead of two. The claim that
  number exists to make visible is unchanged and is stated as what it always
  was: the batch count is **flat in the size of the horde**, not any particular
  value.

- **crcbl-shell**: an **AppKit end-to-end pass**, so macOS is held to the
  standard the other three backends already were. It extends
  `crates/crcbl-shell/tests/appkit_session.rs` — the `harness = false` target
  that exists because `libtest` runs every body on a thread it spawns and AppKit
  raises off the main thread — rather than adding a second one, because two
  processes each bootstrapping an `NSApplication` would fight over which is
  frontmost and injected input follows whichever wins.

  **Input the window system generated**, through `CGEventPost`: a key press and
  its release, an arrow key, pointer motion, a click and a wheel notch. That is
  what reaches `interpretKeyEvents:`, which nothing had ever reached — so
  `ShellEvent::TextCommit` on macOS was in exactly the state the Win32 backend's
  was in before its own e2e suite found `TranslateMessage` missing from the
  pump. It also observes the asymmetry `appkit::pointer` exists to describe: a
  cursor moved down the screen comes back with a _larger_ window Y and a
  _positive_ raw delta, because `locationInWindow` is Y-up and Quartz's delta is
  not.

  **A pasteboard round trip against `pbcopy` and `pbpaste`**, in both directions
  — Apple's own processes, with no `crcbl-shell` in them, which is what
  separates "the pasteboard server has the bytes" from "the shell answered its
  own read out of a cache". A helper binary of ours was considered and declined;
  `docs/backlog.md` records that this covers text only, since `pbpaste` cannot
  be asked for the engine's own format.

  **AppKit as the judge** rather than the backend's own bookkeeping, through
  three new `crcbl_shell::session_support` entry points — `window_facts`,
  `key_window` and `resize_window` — and `activation`, which now takes the title
  of the window to describe. Three of the five switches `appkit::view` lists as
  "structural rather than verified" are now read back off the live window —
  `acceptsMouseMovedEvents`, the first responder being `CrcblView` rather than
  the window, and the registered dragged types — and a resize AppKit performed,
  a borderless flip that covers the `NSScreen` it names exactly, and the
  restored title bar are all checked against `NSWindow` and `NSScreen`.

  **None of that readback goes through `-[NSApp keyWindow]` any more**, which is
  the correction the first macOS run forced. That run reported
  `app_active: false` with `can_become_key: true`: a GitHub runner gives an
  unbundled binary a window server and a window but not activation, so the key
  window was nil and every assertion behind it was being discarded over a
  precondition it did not have. `window_facts` finds this process's own window
  by title among `-[NSApp windows]`, and reports `app_active` and `is_key` as
  fields rather than requiring them; `key_window` remains for the one caller
  that genuinely needs the keyboard, which is `CGEventPost`. The harness then
  asks the session for activation itself —
  `-[NSRunningApplication activateWithOptions:]`, which reaches a lever the
  backend is right not to have, since a game does not get to steal the focus —
  and **the runner grants it**, so the window becomes key and the injected input
  runs. If it is ever refused, the injected-input assertions and the warp
  readback are skipped with a printed account of what did not run and why,
  rather than failing the session or going quietly green.

  **A warp is not an event**, which the same run found:
  `CGWarpMouseCursorPosition` moves the cursor and posts nothing, so reading a
  warp back needs a real `kCGEventMouseMoved` posted at the point the cursor was
  moved to. That makes the check stronger than it was — the seam's conversion
  into Quartz's global space and the backend's conversion out of
  `locationInWindow` are now judged against each other, rather than one of them
  against a tracking-area crossing that a boundary-crossing warp happened to
  produce.

  **And a synthesized mouse event carries no delta unless the poster sets one.**
  `CGEventCreateMouseEvent` leaves `kCGMouseEventDeltaX`/`Y` at zero and
  `-[NSEvent deltaX]` reads exactly those, so `raw_delta` came back `(0.0, 0.0)`
  — correctly. The harness now writes a known delta onto the event, so the seam
  is held to reporting _that_ pair rather than merely something non-zero, and
  the asymmetry `appkit::pointer` exists to describe is observed for the first
  time: a move right and **up** comes back with a larger window X, a smaller
  window Y, and a delta whose Y is still negative, because `locationInWindow` is
  flipped into the seam's space and Quartz's delta is already in it.

  **`ShellEvent::TextCommit` from a real keystroke now has executable coverage
  on macOS.** The injected `kVK_ANSI_A` reaches `interpretKeyEvents:` through
  `sendEvent:` and the first responder, and commits `"a"` — the chain that was
  written blind and is the macOS counterpart of the `TranslateMessage` gap the
  Win32 backend shipped with. That also settles the risk the slice was written
  around: **TCC does not gate `CGEventPost` for events delivered back to the
  posting process.**

  **And the scroll notch reaches the event it is posted on.**
  `CGEventCreateScrollWheelEvent`'s `wheel1` is a _named_ parameter — only
  `wheel2` and `wheel3` are variadic — and the harness had declared the `...`
  one parameter early, so on Apple silicon the amount went to the stack while
  the callee read a register and the event scrolled by zero. The same class of
  defect `appkit::ffi` guards against for `objc_msgSend`, arriving through a
  hand-written C variadic instead.

  **The sample-level pass has no macOS equivalent**, on the same terms as
  Windows: it needs a renderer and macOS has no Vulkan until MoltenVK clears its
  P14 gate. `docs/plan/ROADMAP.md`'s 2026-08-04 correction says so, and
  `docs/backlog.md` carries it as a gap rather than approximating it.

- **crcbl-shell**: **the clipboard and file drops on the AppKit backend**, so
  `ShellCaps::CLIPBOARD` and `ShellCaps::DRAG_DROP` are set there and
  `clipboard_offer`/`clipboard_request` answer instead of returning
  `Unsupported`. macOS is now the fourth backend to implement the whole seam.

  A copy publishes every offered format at once under its own `NSPasteboard`
  type: text under `public.utf8-plain-text`, which is what TextEdit and every
  other application reads, and the engine's own `application/x-crcbl+ron` under
  that mime string verbatim — the same spelling the other three backends use, so
  an engine-to-engine copy is lossless and byte-identical across platforms. An
  empty offer slice **clears** the pasteboard, because macOS has no owner to
  release: a pasteboard is content the server holds. Reads answer the three
  `ClipboardContent` outcomes distinctly, and the answer names the format that
  was _asked_ for — a pasteboard type is a UTI rather than a mime, so there is
  no peer spelling to report.

  Nothing is provided lazily and nothing is held after a write:
  `setData:forType:` copies the bytes to the pasteboard server, so this backend
  carries no deadline, no retry budget and no state between pumps — the only one
  of the four whose clipboard needs none of them.
  `pasteboard:provideDataForType:` is refused for the same structural reason the
  Win32 backend refuses `WM_RENDERFORMAT`, and `docs/backlog.md` says not to
  revisit it without a seam change.

  File drops arrive through `registerForDraggedTypes:` and the
  `NSDraggingDestination` methods on the content view, honouring
  `WindowDesc::accept_drops` — and there the gate is the **system's**: AppKit
  sends no dragging message at all to a view that has not registered, which is
  the same strength as Win32's `WS_EX_ACCEPTFILES` and stronger than Wayland's.
  Each `public.file-url` goes through the shared `parse_uri_list`, so a
  percent-encoded name, a `file://localhost/…` authority and a filename that is
  not valid UTF-8 all behave exactly as they do on the other backends, and a
  dragged _URL_ is not turned into a path that looks plausible and does not
  exist. Promised files (`com.apple.pasteboard.promised-file-url`) are not
  accepted; the seam has no way to name a destination for one.

- **crcbl-shell**: **input on the AppKit backend** — keyboard, text, pointer,
  scroll, relative motion, pointer lock, cursors and warping, so a game is
  playable on macOS rather than merely windowed. `POINTER_LOCK`, `POINTER_WARP`,
  `RAW_POINTER_MOTION` and `TEXT_IME` join the capability set, and
  `ShellCaps::has_mouselook()` is true there.

  Keys carry Apple's `kVK_*` codes mapped to `KeyCode` (a third numbering, which
  coincides with neither evdev nor PS/2 set 1 at any point), an X11 keysym, the
  auto-repeat flag and the modifiers of that event. **Four keys the seam names
  are unreachable on macOS** — `PrintScreen`, `ScrollLock`, `Pause` and
  `ContextMenu` have no `kVK_*` code, and those positions on a Mac keyboard are
  `F13`–`F15`, which are their own keys. **Num Lock is not a modifier there**:
  macOS has no such latch, and `NSEventModifierFlagNumericPad` means "this key
  is on the keypad", so `Modifiers::NUM_LOCK` is never set. **Option is reported
  as `ALT` and never `ALT_GR`**, because the same key is macOS's Alt and its
  level-3 shift and no third key distinguishes them — the opposite conclusion
  the Win32 backend reaches, from the same starting point.

  Text goes through a real `NSTextInputClient` and `interpretKeyEvents:`, so
  commits arrive from the **input method** and dead keys compose — reading
  `-[NSEvent characters]` instead would leave every accented character
  unreachable. Pre-edit is tracked and never surfaced (the seam has no event for
  one), so an input method's candidate window appears at the window's origin
  rather than under a caret.

  The pointer reports both scroll units — a trackpad's `ScrollDelta::Pixels` and
  a wheel's `Lines`, the first backend where both arms are reachable — buttons
  past the fifth through `otherMouseDown:`, and enter/leave from an
  `NSTrackingArea`. `PointerMode::Locked` freezes the cursor with
  `CGAssociateMouseAndMouseCursorPosition(false)` and needs none of the
  clip-and-recentre machinery Win32 and X11 carry.

  Two things a consumer must know. **`PointerMode::Confined` is refused,
  permanently**: macOS has no confine API, only warping the cursor back after it
  has already left, so `POINTER_CONFINE` stays clear — the only desktop backend
  where the two capture modes come apart. And **`RAW_POINTER_MOTION` here is
  unclamped but _accelerated_**: `NSEvent`'s deltas are separate from the
  absolute position and keep flowing at the screen edge, which is what makes a
  camera work, but macOS publishes no way to remove the system's pointer
  acceleration from them.

- **crcbl-shell**: an **AppKit backend**, registered and selected automatically
  on macOS — so `crcbl_shell::open()` now returns a real window there instead of
  `NoBackend`. The window lifecycle: `NSApplication` bootstrap, create, show,
  hide, destroy, title, close-request interception (`windowShouldClose:` answers
  `NO`, and the seam asks), windowed ↔ borderless on a **named** display with
  the windowed style mask and frame restored exactly, size constraints through
  `setContentMinSize:`/`setContentMaxSize:`/`setContentAspectRatio:`, `NSScreen`
  enumeration with visible frame, backing scale, refresh rate and hotplug, an
  event pump and a blocking `wait_events`, and `SurfaceTarget::AppKit` for the
  HAL. Built on hand-written Objective-C runtime FFI — `objc_getClass`,
  `sel_registerName`, `objc_msgSend` and runtime-built classes — with no `objc2`
  and no `cocoa`.

  The shell creates and owns the `CAMetalLayer` and hosts it on its `NSView`, so
  `SurfaceTarget::AppKit` carries the layer and **no HAL backend ever touches
  AppKit**. Borderless is a frameless window at the display's size, not
  `toggleFullScreen:`: the desktop's mode is untouched and there is no Spaces
  transition. `ASPECT_HINT_HONORED`, `WINDOW_POSITION`, `SERVER_DECORATIONS`,
  `MULTI_WINDOW` and `EVENT_WAIT` are set; the pasteboard and drag-and-drop are
  the slice after this one and every bit they would set stays clear.

  Four macOS facts a consumer may need. **`AppKitShell::open` requires the
  process's main thread** and returns `ShellError::Backend` naming that rule
  anywhere else — AppKit raises an Objective-C exception otherwise, which
  unwinding into Rust is undefined behaviour. **`FRACTIONAL_SCALE` is clear**,
  because `backingScaleFactor` is 1.0 or 2.0 and a "scaled" HiDPI mode changes
  the point resolution rather than the factor. **`MonitorInfo::bounds` does not
  tile** across displays of different scales, because AppKit's global coordinate
  space is points rather than pixels — the caveat that field already documents
  for Wayland, now true on a second platform; window placement is unaffected,
  because it is expressed in points. And `MonitorInfo::refresh_millihertz` can
  finally be non-integral: `CGDisplayModeGetRefreshRate` reports 59.94 as 59.94,
  which no other backend's API is able to.

- **crcbl-shell**: a **Win32 backend**, registered and selected automatically on
  Windows — so `crcbl_shell::open()` now returns a real window there instead of
  `NoBackend`. The window lifecycle: create, show, hide, destroy, title,
  close-request interception, windowed ↔ borderless on a named monitor with the
  windowed placement restored exactly, size constraints (`WM_GETMINMAXINFO`
  limits and a live `WM_SIZING` aspect lock), monitor enumeration with work
  area, refresh rate and per-monitor DPI, per-monitor-v2 DPI awareness with
  `WM_DPICHANGED` handled mid-session, a message pump, a blocking `wait_events`,
  and `SurfaceTarget::Win32` for the HAL. Built on hand-written
  `extern "system"` declarations for `user32`, `gdi32`, `shcore` and `kernel32`
  — there is no `windows-rs` and no `winapi`.

- **crcbl-shell** (Win32): **input**. Keyboard events carry a PS/2 set-1 scan
  code with its `E0` prefix folded in, the `KeyCode` for that physical position,
  the layout's `Keysym`, the modifiers and the auto-repeat flag; `WM_CHAR`
  becomes `TextCommit`, with surrogate pairs reassembled so an astral codepoint
  arrives whole and control characters dropped. The pump calls
  `TranslateMessage`, which is what makes a `WM_CHAR` exist at all — dead keys,
  AltGr and an input method's commit all arrive through it, and without it
  typing into a Crucible window produced no text whatever. Pointer motion, all
  five buttons including the two thumb buttons, derived enter and real leave,
  mouse capture so a button released outside the window is still reported, and
  both wheel axes with high-resolution fractions of a detent preserved.
  `WM_INPUT` raw relative motion, with an absolute-reporting device — a
  remote-desktop session, a tablet — differenced into a delta instead of being
  read as one. `PointerMode::Confined` and `PointerMode::Locked` through
  `ClipCursor`, and `warp_pointer` through `SetCursorPos`. Cursor shapes are the
  stock `IDC_*` set applied from `WM_SETCURSOR`, and hiding goes through a
  balanced `ShowCursor` count.

  A confined pointer's clip is the client rectangle **intersected with the
  virtual screen**: `ClipCursor` clamps, so a window larger than the desktop is
  confined to the part of itself that is on screen.

  `POINTER_LOCK`, `POINTER_CONFINE`, `POINTER_WARP` and `RAW_POINTER_MOTION` are
  now set on this backend — the last of them latched on whether
  `RegisterRawInputDevices` was accepted — and `set_cursor` applies rather than
  records. **`TEXT_IME` stays clear**: nothing here touches `WM_IME_*`, so there
  is no composition string and no candidate-window placement, and typing working
  through `WM_CHAR` is not the same claim.

  Three Windows facts worth knowing before building on it: a window frozen
  during a user drag-resize is the system's modal message loop and not a hang; a
  monitor's refresh rate is a whole hertz here, so 59.94 Hz reports as 60; and a
  `DeviceId` names a device _kind_ rather than a device, so two mice cannot be
  told apart yet.

- **crcbl-shell** (Win32): **the clipboard and file drops**, so `CLIPBOARD` and
  `DRAG_DROP` are now set and `clipboard_offer`/`clipboard_request` work instead
  of returning `Unsupported`.

  A write publishes each offered format at once — `CF_UNICODETEXT` for
  `text/plain;charset=utf-8`, and a `RegisterClipboardFormatW` format named
  after the mime for everything else — so one copy reaches Notepad as text _and_
  round-trips through another Crucible as `application/x-crcbl+ron` without
  loss. The reader picks. Windows synthesizes `CF_TEXT` and `CF_OEMTEXT` from
  the Unicode text in both directions, so there is no `TARGETS`-style format
  negotiation to do. An empty `offers` slice empties the clipboard: Win32 has no
  selection _owner_ to relinquish, so that is what "release" can mean here.

  Reads are answered inside `clipboard_request` and delivered on the next
  `pump`, exactly once. `Win32` has neither Wayland's focus gate nor its serial
  requirement — any window may open the clipboard at any time — so a read is
  never _held_ and `clipboard_offer` never returns `NeedsUserInteraction`. The
  one real wait is `OpenClipboard` being refused while another process has the
  clipboard open, which is routine; it is retried for a bounded 70 ms and then
  reported `Unavailable` rather than failing a paste over a refusal that was
  over before the user noticed.

  Files dropped on a window created with `WindowDesc::accept_drops` arrive as
  one `ShellEvent::DroppedFile` per file, with the drop point in client pixels,
  through `DragAcceptFiles` and `WM_DROPFILES`. The gate is enforced by the
  system as well as by this backend: without `WS_EX_ACCEPTFILES` no drop message
  is ever sent. **There is no drag feedback** — no drop cursor and no hover
  highlight while a file is still in the air — because that is `IDropTarget`,
  which is COM; the drop itself works.

- **crcbl-shell** (Win32): `wait_events` now drains the message queue before it
  sleeps and no longer passes `MWMO_INPUTAVAILABLE`. A message _sent_ to a
  window (rather than posted) leaves `QS_SENDMESSAGE` set after `PeekMessage`
  has dispatched it, and that flag asks to be woken by exactly that bit — so the
  wait returned immediately, forever, and an application idling at zero frames
  per second span a core instead. Draining first is the stronger form of what
  the flag was there for. That removed `QS_SENDMESSAGE` from the picture and did
  not make the wait sleep on a CI runner, where a _posted_ message still wakes
  it; `docs/backlog.md` carries what is known and what is not.

- **crcbl-shell**: `DisplayMode::satisfied_by`, the request-versus-answer
  comparison `WindowState::mode_request_honoured` now uses.

- **crcbl-shell**: a **Win32 end-to-end suite** behind the new `win32-e2e`
  feature (off by default), run by `crates/crcbl-shell/tests/run-win32-e2e.ps1`
  and by a CI job of its own against a real Windows desktop — the treatment
  Wayland and X11 got at P0.5/P0.6. It drives the backend through `open_backend`
  and `dyn Shell` only, and covers what no in-process test can reach:
  keystrokes, clicks and wheel notches **injected from another process** with
  `SendInput`, so they arrive as posted, queued, translated and dispatched
  messages; mode flips and resize storms judged by `GetWindowRect` rather than
  by the backend's own bookkeeping; monitors, DPI and focus against the desktop
  the machine actually has; and a clipboard round trip with a second process, in
  both directions, with this shell's message loop stopped.

  Two helper binaries come with it, `crcbl-e2e-win32-input` and
  `crcbl-e2e-win32-clip`, on the same terms as the two Linux key senders:
  `required-features`, and a `main` that fails loudly on any other platform.

  **The harness defeats Windows' foreground lock, and the backend does not learn
  how.** `SetForegroundWindow` is granted only to a process that already owns
  the foreground or received the last input event, and under `nextest` every
  test is a fresh process with neither — so three tests spent twenty seconds
  each being refused by the job's own console window. The suite now lowers
  `SPI_SETFOREGROUNDLOCKTIMEOUT` for the session (restoring it on the way out,
  for a desktop that is not a CI runner) and attaches its input queue to the
  foreground thread's around the request, which is what an automated harness
  does to arrange a precondition a human would have arranged by clicking. None
  of it is in `src/win32/`: a game does not get to steal focus, and a backend
  that knew how could do it to a user.

  **The sample-level pass has no Windows equivalent yet.** The Linux suites
  press F11 at a running game, which needs a renderer, and no runner on this
  platform has a Vulkan device — `docs/plan/ROADMAP.md` schedules it for P14.

- **`apps/horde` takes `--choose <N>`**, so a headless run can reach past the
  level-up screen. The screen has no way out but a digit key, which parked
  `horde --headless --frames 600 --prefill 200` at its first level-up at three
  seconds — no headless invocation could reach a potion, so every measurement of
  the drop rate came from `game::tests`. The flag presses the digit for the
  player once per distinct offer, tracked by the same level-and-offer identity
  the panel rebuilds on. The digit is validated `1..=UPGRADE_CHOICES` at parse
  time, because a choice out of range is silently ignored by `apply_choice`.

- **asteroids interpolates positions between ticks, snapping across the wrap.**
  Every angle was lerped across the frame's alpha and every position was the
  last tick's, so a rock at 60 Hz on a 144 Hz display moved in sixtieths. Each
  body now publishes `(previous position, current position, teleported)`: the
  wrap sets the flag on the tick it moves a body, a respawn and every spawn
  reset the pair, and the renderer lerps between the pair or snaps on a flagged
  tick — the naive "lerp the positions too" would fly a wrapped body back across
  the whole field.

### Fixed

- **Four samples had silently lost present-based pacing.** `horde`, `breakout`,
  `flappy` and `asteroids` each hand-wrote an `optional_features` set that was
  `crcbl::GpuContextDesc::default`'s **minus `PRESENT_FEEDBACK` and
  `PRESENT_TIMING`** — stale copies of a default from before those were added. A
  device opened without `PRESENT_FEEDBACK` cannot observe its own presents, so
  `GpuContext::acquire`'s closed loop was unreachable in all four: dead code,
  and nothing said so. `apps/sandbox` logged
  `hal: pacing on presents, 2 frames deep` and the four games logged nothing.

  All four now inherit the engine's set rather than restating it, and each has a
  test asserting its `optional_features` equals `GpuContextDesc::default`'s —
  the copies were the mechanism, so the fix removes the copies rather than
  adding two flags to four files. Verified past the log line: run windowed
  against a real Wayland swapchain, each of the four now reaches
  `crcbl-vk: vkWaitForPresentKHR on present 1; the loop is closed`.

  **No frame budget moved.** Horde at 10 000 instances under its own documented
  conditions is 0.130 ms CPU before and after, GPU total 0.045–0.046 ms either
  way, and a windowed 120-frame run is 1.96 s in both. Expected: FIFO already
  paced the loop through `vkQueuePresentKHR`, so closing the loop changes where
  the CPU waits, not how long. Browsers and wgpu grant neither flag, so those
  paths are unchanged and keep the open-loop limiter.

- **A browser with `navigator.gpu` and no adapter killed demo boot with an
  uncaught `TypeError`.** Reported against the live site on Chromium 151 under
  Wayland with `--render-node-override` on a hybrid Intel/NVIDIA laptop, whose
  `chrome://gpu` reads `Vulkan: Disabled` — and Chrome runs WebGPU on Vulkan
  there, so every adapter request is refused.

  `GPU.requestAdapter()` resolves to **`null`** in that case, and wgpu 30 loses
  it: the vendored binding types the nullable WebIDL return as
  `js_sys::JsOption<GpuAdapter>`, whose `into_option` counts only `undefined` as
  absent, so JS `null` arrives as `Some(null)`. `enumerate_adapters` then yields
  a one-element list holding it and `Adapter::get_info()` reads `.info` off
  `null`. Nothing generated for a structural getter has a `try`, so the
  `TypeError` unwound through wasm uncatchably instead of reaching the "no
  usable adapter" arm `WgpuInstance::new_async` already had.

  It now asks the browser for an adapter before enumerating anything — wgpu's
  own `is_browser_webgpu_supported`, which tests the result for null before
  reading a property off it — and returns `None` with a named reason in the log.
  No adapter metadata is invented. `web/engine/demo.js` also asks, before
  downloading the engine, and says "This browser has WebGPU, but no GPU to run
  it on" with a pointer to the browser's own GPU report, warning that its WebGPU
  line can read "Hardware accelerated" while every adapter is still refused.

- **The portable bindless declaration failed on wgpu and overflowed on D3D12.**
  `BindGroupLayoutEntry::count` of `u32::MAX` is the seam's "as many as you
  can": `crcbl-vk` clamped it to `Limits::max_bindless_descriptors` and the null
  backend mirrored that, while `crcbl-wgpu` handed it to wgpu verbatim and got a
  hard rejection — so the one spelling meant to be portable built a layout on
  Vulkan and errored on the web backend. Worse on `crcbl-dx12`: a `u32::MAX`
  binding **without** `BindingFlags::VARIABLE_COUNT` planned a descriptor range
  of `u32::MAX`, and the running offset then overflowed for every range after
  it. Both now resolve the sentinel through the seam's `resolved_count`.

  `crcbl-mtl` deliberately still **refuses** it rather than clamping. It reports
  `max_bindless_descriptors: 0` because flat argument tables have no
  runtime-sized array, so clamping would hand back a one-element array on a
  backend that cannot do bindless at all — the quiet downgrade the seam exists
  to forbid. A named refusal is the honest answer there.

  The field's own documentation did not state the sentinel before this — only
  the module header mentioned it in passing — which is how two backends came to
  ignore it. It says so now.

- **`crcbl-wgpu` silently dropped three things the seam says it must refuse.**
  `create_bind_group_layout` read `visibility`, `kind` and `count` and nothing
  else, so a layout setting any `BindingFlags` on a device without
  `Features::DESCRIPTOR_INDEXING` was built as an ordinary fixed array wearing a
  bindless declaration, and a `VARIABLE_COUNT` entry that broke the ordering
  rule — it must be both the last entry of the slice and the highest binding
  number — was accepted. `create_bind_group` dropped
  `BindGroupDesc::variable_count` without a word. Each is now refused by name,
  in the wording `crcbl-vk` and `crcbl-mtl` already use, so all four backends
  answer the same descriptor the same way.

  `variable_count` is **validated rather than honoured**, and the reason is in
  the code: on Vulkan the number sizes an allocation that `update_bind_group`
  fills in later, and wgpu has neither half — a binding array's length _is_ the
  length of the slice handed to `create_bind_group`, and this backend's
  `update_bind_group` is `Unsupported` because WebGPU bind groups are immutable.
  So the number says nothing the entry list has not, and it is checked against
  the entries and the layout's declared ceiling instead.

  Two smaller ones alongside. `count: 0` is refused rather than mapped to a
  scalar binding, which vk, D3D12, Metal and the null backend all already did.
  And `create_bind_group_layout` is now error-scoped like the pipelines and bind
  groups: wgpu reports a rejected layout to the error handler and **still
  returns an object**, so a poisoned layout used to arrive as `Ok` and surface
  as a validation failure in whichever pipeline later named it.

- **`crcbl-wgpu` could not fill an array binding, so every descriptor-indexing
  bind group it built was broken.** `Device::create_bind_group` resolved each
  `crcbl_hal::BindGroupEntry` to a scalar `wgpu::BindingResource` keyed on
  `binding` alone — `BindGroupEntry::array_index` appeared nowhere in the crate
  — so two entries naming elements 0 and 1 of one binding arrived as two
  `wgpu::BindGroupEntry`s with the same binding number. The layout half already
  mapped the seam's `count` onto wgpu's `Some(NonZero)`, so the layout was
  expressible while the group was not, and the backend reports
  `Features::DESCRIPTOR_INDEXING`. Entries are now bucketed by binding, sorted
  by `array_index`, and emitted as `TextureViewArray` / `SamplerArray` /
  `BufferArray` when the **layout** declares a count — wgpu picks the spelling
  off the layout, not off how many entries a group happens to supply.

  Two things a caller can now see. Fills wgpu has no spelling for are refused as
  `HalError::InvalidDescriptor` naming the binding and the index, rather than
  packed: a hole (wgpu's arrays are dense, so element _i_ of the slice **is**
  array element _i_, and closing a gap would silently shift every later element
  down one), an index written twice, an index past the declared count, one
  binding filled with more than one kind of resource, and an entry naming a
  binding the layout never declared. A trailing shortfall — elements `0..n` with
  `n` below the count — is the one partial fill wgpu accepts and still builds.
  And `create_bind_group` is now error-scoped like the pipelines already were:
  wgpu reports a rejected bind group to the error handler and **still returns an
  object**, so a bad group used to arrive as `Ok` and surface as a validation
  failure in whichever pass later bound it. It is now
  `HalError::Backend("wgpu create_bind_group: …")` at the call that made it.

- **`crcbl::screenshot`'s readback barriers lied about the swapchain image's
  state, and never put it back.** `OffscreenSetup::draw_and_readback` declared
  its pre-copy transition as coming from `ResourceState::ColorAttachment` — the
  state the frame's last pass leaves the target in, not the state the graph
  hands it back in, which is `ForwardRenderer::present_target`'s
  `final_state: Present` — and then presented the image still in
  `ResourceState::TransferSrc`. Vulkan reported the first as
  `VUID-VkImageMemoryBarrier2-oldLayout-01197` on every screenshot ever taken;
  the second is a D3D12 debug-layer error on the second trip round the ring,
  where the declared before-state `COMMON` meets an image left in `COPY_SOURCE`.
  The copy is now bracketed by `Present` → `TransferSrc` and `TransferSrc` →
  `Present`. Pixels are unchanged — the golden cube still matches to zero
  differing pixels.

- **The three GPU draw-generation counters are device-local, zeroed by a
  dispatch inside the frame.** `crcbl_render::draw_gen` put its survivor count,
  indirect arguments and draw counts on `MemoryLocation::HostUpload` and bound
  them writable, so that `DrawGen::begin_frame` could zero them from the CPU —
  the seam allows a buffer fill only outside a pass, and a render-graph frame is
  passes end to end. D3D12 has no unordered access view of an upload-heap
  resource at all, so that arrangement is what removed its device. A new
  `clear_counters.slang` pass, scheduled by `DrawGen::add_passes` ahead of the
  cull dispatch and barriered into it by the graph, writes the zeroes instead;
  all five of the stage's buffers are now `MemoryLocation::DeviceLocal`, and the
  three the pass owns also carry `BufferUsage::TRANSFER_DST` so a test can
  poison them. `DrawGen::begin_frame` still writes the cull parameters and no
  longer touches the counters. A frame now records three compute dispatches
  ahead of the draws rather than two, and the per-pass GPU timer report names
  `clear-counters` first. Nothing rendered changes.

- **A uniform buffer smaller than 256 bytes removed the D3D12 device.** A
  constant buffer view's `SizeInBytes` must be a multiple of 256 and a view may
  not run past the end of its resource, so `crcbl-dx12` rounding the view up
  over a 16-byte buffer was `DXGI_ERROR_INVALID_CALL` and a removed device —
  reported at whatever call came next, which is why it looked like an offscreen
  swapchain failure. `create_buffer` now pads the **allocation** of any buffer
  carrying `BufferUsage::UNIFORM` up to the same 256-byte block, and every
  constant buffer view is checked against that allocation instead of assuming
  it. Nothing above the seam can see the padding: the size a caller asked for is
  still the size `write_buffer`, `WHOLE_BUFFER` and every bounds check use, and
  `Limits::max_uniform_buffer_range` is a limit on a bindable range rather than
  on an allocation.

- **`crcbl-dx12` refuses a host-visible buffer bound for writing instead of
  taking the device down.** D3D12 has no unordered access view of an upload- or
  readback-heap resource — the flag is rejected at creation and the heap pins
  the resource to a state a shader cannot write from — and the seam permits the
  combination because Vulkan does. Binding one to a
  `BindingKind::StorageBuffer { read_only: false }` slot is now
  `HalError::InvalidDescriptor` naming the binding, the heap and the fix, where
  it used to be a `CreateUnorderedAccessView` that wrote nothing and a device
  removed at the next call. Read-only storage bindings of a host-visible buffer
  are unaffected, and remain how the engine's instance and table buffers are
  read. **A shader that writes a buffer needs `MemoryLocation::DeviceLocal` on
  this backend**; `crcbl-render`'s GPU draw generation still asks for the other
  thing, so the D3D12 frame does not yet run.

- **A `crcbl-dx12` buffer view is bounded and aligned rather than truncated.** A
  storage binding's raw view is refused when its offset is not a multiple of
  D3D12's 16-byte raw-view alignment, or when its range is shorter than one
  four-byte element, or when the element count would not fit `NumElements` —
  which was previously clamped to `u32::MAX`, i.e. to a view running past the
  end of the buffer. A constant buffer binding is likewise refused when its
  offset is not a multiple of `Limits::min_uniform_buffer_offset_alignment`.
  Every one of those was a `Create*View` call that returns `void` and diagnoses
  nothing.

- **Every `crcbl-mtl` draw hung on Apple's paravirtual GPU, and the call was
  `setDepthStencilState:nil`.** `bind_graphics_pipeline` passed nil for any
  pipeline whose descriptor carried no `depth_stencil` — which is every pipeline
  drawing into a colour-only pass — and that argument hangs the virtualised
  device GitHub's macOS runners expose, faulting the command buffer with
  `kIOGPUCommandBufferCallbackErrorHang` while render-pass clears on the same
  device succeeded. A ten-probe bisect isolated it: a hand-encoded pass plus
  `setDepthStencilState:nil` hung, the same pass plus a real
  `MTLDepthStencilState` passed, and each of the five other rasteriser calls
  passed alone.

  Metal documents nil as "restore the default state", so the driver is at fault,
  but the fix costs one object per device and removes the nil path entirely: a
  `MetalDevice` now builds one always-pass, never-write `MTLDepthStencilState`
  when it opens, and a pipeline that declares no depth/stencil state carries
  that instead of `None`. The substituted state compares `Always` with depth
  writes off and keeps on every stencil outcome, so it tests nothing and writes
  nothing — it cannot change an image.

- **`crcbl-dx12` built root signatures naming registers its shaders do not
  read.** `BaseShaderRegister` was the seam's binding number and `RegisterSpace`
  was the set index, on the theory that `[[vk::binding(binding, set)]]` reaches
  HLSL unchanged. It does not: the attribute is Vulkan-only, and `dxc` numbers
  each register class from zero in declaration order across the whole source, in
  space 0 — so a set holding a `ConstantBuffer`, a `StructuredBuffer` and an
  `RWStructuredBuffer` at bindings 0, 1 and 2 is `b0`/`t0`/`u0` in the container
  and was being described as `b0`/`t1`/`u2`. Pipeline creation rejects that, so
  every shader in this workspace whose set mixes resource classes — `mesh`,
  `cull`, `draw_gen`, `compute_probe`, `sprite`, `ui` — could not have been used
  from this backend. Only `triangle.slang`, whose set is one storage buffer,
  happened to work.

  Registers are now assigned per class in ascending `(set, binding)` order,
  threaded across a whole pipeline layout, and the rule is checked against the
  resource table in every committed DXIL container by a test that needs no
  Windows.

### Changed

- **Breaking: `ComputePipelineDesc` carries a `workgroup_size`, and Metal can
  dispatch.** `crcbl-mtl` refused `bind_compute_pipeline`, `dispatch` and
  `dispatch_indirect` outright, because
  `dispatchThreadgroups:threadsPerThreadgroup:` takes the
  threads-per-threadgroup at the _call_ while SPIR-V, DXIL and WGSL bake it into
  the module — so MSL had nowhere to declare it and the seam had no field
  carrying it. `crcbl_hal::ComputePipelineDesc` now has
  `workgroup_size: [u32; 3]`, which every caller must add; take it from the
  `WORKGROUP_SIZE` constant `crcbl-shaders` publishes beside each compute shader
  (`[crcbl_shaders::cull::WORKGROUP_SIZE, 1, 1]`) rather than writing a literal,
  since that constant is pinned to the shader's own `[numthreads(…)]`.

  Two guards keep the new field from becoming a second, independent number.
  `ComputePipelineDesc::check_workgroup_size` refuses a zero, an over-limit
  dimension or too many invocations per workgroup, and every backend calls it;
  and `crcbl-vk` additionally reads the `LocalSize` out of the SPIR-V it is
  compiling and fails with `HalError::ShaderCompilation` naming both sizes when
  the descriptor disagrees with the shader. Metal cannot perform the second
  check — MSL declares no thread count — which is exactly why it is done where
  it can be.

  `crcbl-mtl`'s compute pass now opens a real `MTLComputeCommandEncoder` whose
  lifetime is the pass's, and `bind_group` reaches its argument tables. A copy
  inside a compute pass is now refused rather than silently ending the pass's
  encoder and taking its pipeline state with it, and a barrier inside one is
  ignored exactly as it already was inside a render pass. `Features::COMPUTE` on
  Metal now means the whole path rather than "compute pipelines exist".

- **Breaking: `crcbl_shaders::mesh::FrameUniforms` no longer has a `model`
  field, and the block is 128 bytes rather than 192.** The per-object transform
  moved into the instance array, so `mesh.slang`'s uniform block holds only what
  is genuinely per frame and its vertex stage reads
  `instances[SV_InstanceID].transform`. `ForwardRenderer::begin_frame` keeps its
  signature — the `model: Mat4` it takes is now written into the instance pool —
  so a caller that only drives the renderer is unaffected; a caller that builds
  a `FrameUniforms` itself must drop the field and bind a `GpuInstance` storage
  buffer at `(set 0, binding 2)`. Every `mesh.slang` artifact is regenerated and
  the `mesh` and `ortho mesh` goldens are unchanged by the move.
- **Breaking: the UI pass has one constant path, and `ConstantDelivery` is
  gone.** `crcbl_render::ConstantDelivery`, `UiRenderer::constant_delivery` and
  the `ui_tier_b` shader (`shaders/ui_tier_b.slang` and its `spirv/`, `wgsl/`,
  `msl/` and `dxil/` artifacts) are removed. `ui.slang` takes its viewport from
  a uniform buffer at `(set 0, binding 3)` on every target instead of a
  `[[vk::push_constant]]` block, so one artifact set serves every backend and
  `UiRenderer` builds the same pipeline layout, bind-group layout, buffers and
  command stream whatever the device reports for `Features::PUSH_CONSTANTS`. The
  cost is one indirection per vertex where a push constant would have served;
  the saving is a permutation axis and a second `.slang` that had to be kept in
  step by hand. The sample binaries no longer ask for `PUSH_CONSTANTS` at all —
  nothing in the engine reads one now.

- **`wgsl/ui.wgsl` is a loadable artifact for the first time.** It declares
  `@binding(3) @group(0) var<uniform> constants_0`, where the push-constant form
  lowered to a module-scope `var<uniform>` with no `@group`/`@binding` that naga
  rejects outright — so `crcbl-wgpu`, the only backend that ingests WGSL, could
  not create the UI module from it and resolved `ui_tier_b` instead. Verified by
  parsing and validating every `wgsl/*.slang` output with naga 30: all six pass,
  and the previous `ui.wgsl` fails with "Binding decoration is missing or not
  applicable". The regenerated `spirv/ui.spv`, `wgsl/ui.wgsl` and `msl/ui.metal`
  are byte-identical to the deleted `ui_tier_b` ones, and every Vulkan golden
  image — `button_skin_widths` and `menu_frame_two_sizes` among them — is
  unchanged at zero differing pixels.

- **Breaking: the two-valued renderer tier is gone, replaced by device
  capabilities and three derived path selectors.** `crcbl_hal::RendererTier` and
  `DeviceCaps::tier` are removed; `Features::TIER_A` is renamed
  `Features::GPU_DRIVEN` and documented as a named bundle to pass as
  `optional_features`, never as a requirement. In their place
  `DeviceCaps::geometry_path`, `DeviceCaps::binding_model` and
  `DeviceCaps::lighting_path` answer with
  `GeometryPath::{MeshShader, IndirectCount, IndirectPerBatch}`,
  `BindingModel::{Bindless, ArrayPages}` and
  `LightingPath::{RayTraced, Rasterised}` — each ordered best-first, each
  degrading monotonically, and each also constructible from a bare `Features`
  through `from_features`. Log lines and `Debug` impls that printed a tier now
  print the three selected paths. A tier could not express three independent
  axes, and forcing a device into the wrong bucket is a lie the renderer then
  acts on. The null backend's two presets are renamed with it:
  `NullInstance::tier_a` is now `NullInstance::gpu_driven` and
  `NullInstance::tier_b` is now `NullInstance::portable`.

- **Breaking: `DeviceDesc::for_adapter` requires only what nothing can work
  without.** `required_features` is now
  `Features::COMPUTE | Features::TIMELINE_SEMAPHORE`, with
  `Features::GPU_DRIVEN` moved to `optional_features`. It used to demand the
  whole GPU-driven bundle, so a device was refused over one absent flag while
  having the rest — the reason `crcbl-mtl` was refused outright over
  `DRAW_INDIRECT_COUNT`, which is absent from Metal's API rather than
  unimplemented. That backend now opens on the seam's own constructor and
  degrades. A caller that genuinely cannot render without a feature still names
  it in `required_features` and still gets a named `UnsupportedFeatures`
  failure.

- **`ModeRequest::mode` answers `None` when there is no window to read, instead
  of an invented `Windowed`.** The `DisplayMode` it returned for a dead window
  read exactly like a genuinely windowed run — the defect the `mode_at_exit`
  fallback exists to paper over for summaries. Callers with a live window
  (`Loop::display_mode`, `ModeRequest::toggle`) unwrap it; a run that ended
  still reports through `mode_at_exit`, which keeps the last mode the window was
  seen in rather than inventing one.

- **Breaking: `FrameLimit` stores the rate it was asked for and derives the
  period.** `FrameLimit::fps` is now `const` and `FrameLimit::period` is not;
  `rate()` is new, and `Display` prints `1000 fps` or `unlimited`. Nothing about
  the pacing changes — this is what lets a log report the number that was typed
  instead of a 33.333333 ms period, or a rate recovered from one by a division
  that rounds.

- **Breaking: `LoopConfig` gained `limit`, and `PolledGpu::request` /
  `PolledBoot::request` take a `GpuOptions` in place of an
  `Option<GpuBackend>`.** `GpuOptions` is the half of `GpuContextDesc` that
  comes from the command line rather than from the game — the backend and the
  pacing — so a game's own `desc` ends `..GpuContextDesc::from(gpu)` and the
  next run-level knob is a field there rather than another parameter threaded
  through five bring-up paths. `Common::gpu()` and `Common::loop_config()` are
  the two calls a sample makes; the four games' identical six-line `LoopConfig`
  literals are now one call each.

- **Breaking: `Pacing` has a fourth variant and a new default.** `Pacing::Auto`
  is now `Pacing::default()`; `Pacing::Vsync` is not. Any `match` on `Pacing`
  must gain an arm, and — the quieter half — **every caller that took
  `GpuContextDesc::default()` has changed behaviour without changing a line**:
  such a context now opens on vsync, asks the display once after its first
  present, and rebuilds itself onto the adaptive present mode if the display
  reports `DisplayTiming::Variable` or `Stepped`. `Fixed`, `Unknown` and a
  failed query all stay on vsync, which is what every machine this repo can test
  on reports.

  **A caller that wants the old behaviour writes `pacing: Pacing::Vsync`** in
  its `GpuContextDesc` (or calls `set_pacing(Pacing::Vsync)`): a concrete
  `Vsync`, `Adaptive` or `Off` is never overridden by the observation, which
  refines `Auto` and nothing else. `Pacing::Auto.preferences()` is the vsync
  list — the swapchain genuinely opens on `Fifo`, because the present mode is
  chosen before any present exists and `VK_EXT_present_timing` is specified to
  report nothing until one has — so `Auto` and `Vsync` differ in what happens
  after the first present, not before it.

- **`crcbl_hal::ShaderModuleDesc` gained a `dxil` field**, and
  `crcbl_hal::ShaderSources` a matching `DXIL` bit. It is `Option<&'a [u8]>` — a
  DXIL container is a signed binary blob, so it is closer to `spirv: &[u32]`
  than to the `Option<&str>` source text of `wgsl` and `msl`, and it is an
  `Option` because a zero-byte container is a _truncated_ file rather than an
  absent one. Every construction site must name the field; a module carrying two
  entry points passes `None`, which is the truthful answer rather than an
  omission.

- **`crcbl-shaders`: `Shader::new` and `EntryPoint::new` changed shape.**
  `EntryPoint::new` takes the entry point's DXIL container as a third argument
  and `Shader::new` is unchanged in arity — the DXIL hangs off the entry point,
  because that is what a container holds. Only the generated table calls either.

- **`crcbl-shaders`: the SPIR-V, WGSL and MSL artifacts are byte-identical.**
  Nothing about the existing three targets changed; a moved hash there would be
  a bug, not a re-bless.

- **`crcbl-shaders`**: `tools/compile-shaders.sh` now passes
  `-fvk-use-entrypoint-name` to the SPIR-V target, so a module's entry point
  keeps its source name in `OpEntryPoint`. Without it Slang renames a module's
  _only_ entry point to `main` while the WGSL and MSL targets keep the real
  name, which would have made a single-entry-point module addressable as `main`
  on Vulkan and as its own name everywhere else. Every existing artifact is
  byte-identical with and without the flag — each has two entry points, which is
  the case Slang does not rename — so no committed `.spv` moved and no golden
  image needed re-blessing.

- **crcbl-mtl**: **a GPU fault now names the encoder that caused it.** Every
  `MTLCommandBuffer` this backend creates is built from an
  `MTLCommandBufferDescriptor` carrying
  `MTLCommandBufferErrorOptionEncoderExecutionStatus`, and the `HalError`
  reported by `poll_readback` and `wait_idle` unpacks
  `MTLCommandBufferEncoderInfoErrorKey` out of the failure's `userInfo`: each
  encoder in recording order, with its label, its debug signposts and whether it
  faulted, was merely affected, or never started. The message also carries the
  `NSError` domain and code and the `MTLDevice`'s own name. Where a fault
  previously read `Caused GPU Hang Error (00000003:…)` and stopped, it now says
  which of a command buffer's encoders died — the difference between a broken
  render pass and a copy that never ran. Every encoder is labelled to make that
  legible: the copy encoder is `crcbl copies`, and a render pass with no
  `RenderPassDesc::label` is `crcbl render pass` rather than nameless.

- **crcbl-shell** (Wayland): the effective mode of a fullscreen window now names
  the monitor it is on, taken from `wl_surface.enter`. Asking for a monitor is
  only a hint on this platform, but which one the compositor used is observable,
  and without it `mode_request_honoured` answered "no" to a request the
  compositor had honoured exactly. A summary line that read `borderless` may now
  read `borderless on monitor 2`. `None` still means the backend cannot say —
  the surface is on no output or on two.

- **asteroids**: the ship draws a flame under its nozzle while thrusting. The
  sheet gains a second frame (`assets/ship.crpix`), `RenderState` carries the
  thrust intent from the tick, and `art::Scene::build` picks the frame — the
  ship is no longer one picture whether or not the engine is on.

### Fixed

- **A mesh anywhere but the start of the geometry pool drew another mesh's
  vertices, on Vulkan and Direct3D but not on WebGPU or Metal.** `mesh.slang`
  pulled its vertex with `vertices[SV_VertexID]` while
  `crcbl_render::ForwardRenderer` passed the mesh's `MeshRange::base_vertex`
  through `draw_indexed`'s own base-vertex argument — and Slang lowers
  `SV_VertexID` to `gl_VertexIndex - BaseVertex` on SPIR-V, which subtracted
  that base straight back out. The same disagreement covers `SV_InstanceID`.
  Invisible while the cube was the pool's only resident, because its base is 0.

  Fixed the way `sprite.slang` resolved its half: **every draw the forward pass
  records now passes zero for both of its bases**, which is the one value all
  four targets agree on, and the real ones arrive as data. The instance index is
  a `crcbl_shaders::mesh::DrawConstants` block (binding 3, one 16-byte block per
  draw, reached through a dynamic offset); the base vertex is the mesh table's,
  reached through the drawn instance — see the mesh-table entry above, which
  moved it there. Nothing in the picture now depends on how a target lowers a
  builtin. The mesh pool still stores indices mesh-relative, so a mesh's bytes
  still do not depend on where it landed.

  Upgrading: anything that builds `mesh.slang`'s descriptor set by hand must add
  binding 3 — a uniform buffer holding `DrawConstants` — or the pipeline draws
  nothing.

- **Asteroids: a rock straddling a field edge was drawn once, so half of it
  vanished for the whole of a crossing.** The field wraps, and the half past the
  seam belongs at the opposite edge — now it is drawn there: every rock is
  emitted at its own position plus a ghost per wrapped offset (`wrapped_offsets`
  in `apps/asteroids/src/art.rs`), with the corner case (a rock crossing a
  corner needs the diagonal copy too) covered. A wave spawns its rocks **on**
  the border, so this was visible at every wave start rather than only during
  mid-flight crossings. The ship and the shots straddle the same seams and are
  left single — their crossings are shorter and the missing half less
  conspicuous.

- **`crcbl-shaders`**: `build.rs`'s byte-for-byte recompile check invoked
  `slangc` with an **absolute** source path while `tools/compile-shaders.sh`
  uses one relative to the crate root. Slang copies the path it was given into
  the `#line` directives of its Metal output, so the check compared an artifact
  against a differently-pathed rebuild of itself and failed the build outright —
  on every machine with the pinned compiler installed, which is to say on every
  machine belonging to someone editing a shader. The recompile now runs from the
  crate root with the manifest's own relative path.

- **`crcbl-vk`**: **a surface handle crossed instances silently, and freeing it
  destroyed the wrong object.** Each `VkInstance` owns its own surface pool, so
  two instances issue byte-identical handles; the ownership check compared the
  entry's owner against the looking-up instance, which is trivially true for
  whatever _that_ instance holds at the same index. So instance A answered
  `surface_caps` for instance B's handle with A's own surface, accepted it as
  `compatible_surface` in `request_device`, and — the one that corrupts state —
  `A.destroy_surface(b)` freed **A's** surface while B went on using a handle it
  still believed live. `crcbl-hal`'s obligation 3 requires
  `HalError::ForeignObject` here; the arm existed and was unreachable.

  Surface handles now carry their issuing instance, reusing the tagging scheme
  the device-scoped handles already used, so the check is against the handle's
  own tag rather than against the pool it was looked up in. A handle no instance
  issued is still `InvalidHandle`, and one belonging to another instance is now
  `ForeignObject`. Found by writing the cross-instance test the reference
  backend did not have — `crcbl-hal`'s null backend had covered this case all
  along.

- **crcbl-dx12**: **a software rasteriser was enumerated as an integrated GPU.**
  `is_software` consulted only `DXGI_ADAPTER_FLAG_SOFTWARE`, and DXGI lists
  "Microsoft Basic Render Driver" — which is WARP — with that flag _clear_, so
  `Instance::adapters` reported it as `DeviceType::Integrated`. A caller ranking
  `Discrete > Integrated > Cpu` to prefer real hardware picked the software
  rasteriser and believed it had a GPU. Measured on `windows-latest`, where
  neither listed adapter is hardware; a machine carrying the Basic Render Driver
  beside a real GPU is where it would have cost a frame rate.

  The test is now that flag **or** Microsoft's own vendor and device ids, named
  as constants in `crcbl_dx12::adapter` and read off the runner's own
  enumeration line. Both halves of the pair are required, and the flag is still
  consulted first, so an adapter that sets it is caught whatever its ids say.
  One consequence reaches `Instance::adapters`: such an entry is now skipped by
  the hardware pass and appended once by `EnumWarpAdapter`, so a machine that
  listed it twice lists it once, as `Cpu`.

  The LUID de-duplication is unchanged, and was never what fixed this: those two
  entries carry different LUIDs, so DXGI considers them two adapters and there
  was nothing for it to collapse.

- **crcbl-wgpu**: **the browser build presented every frame far too dark.**
  WebGPU's supported context formats are all linear — `getPreferredCanvasFormat`
  returns `rgba8unorm` or `bgra8unorm`, and `configure` refuses an `-srgb` one —
  so `SurfaceCaps::preferred_format` fell through to its "first format offered"
  fallback and picked a linear target. Every pass above the seam writes
  display-referred values and leaves the sRGB encode to the hardware, so on a
  linear target the encode simply never happened: the horde's grass, authored at
  `#19211a`, reached the canvas as roughly `#020302` while the same bytes were
  right on Vulkan.

  A WebGPU surface now advertises the sRGB counterpart of each 8-bit format it
  reports, and a swapchain asked for one is configured with the linear
  counterpart plus that format in `viewFormats` — the encode comes from the view
  `acquire_next_frame` builds. Nothing changes for a native surface, which
  offers its sRGB formats outright: the counterparts are appended only where the
  surface did not already list them, and only for a canvas.

- **crcbl** (engine): **a key held when a menu opened stayed held forever.**
  `MenuPump` claims Up, Down and Enter while a menu is showing, and it was
  claiming the _release_ as well as the press — so a movement key pressed before
  the menu opened and let go under it was never reported up to the game. In the
  horde: hold Down, level up, pick an upgrade, and the wizard walked south with
  nothing pressed until the key was tapped again.

  The held-key list is now what its documentation always said it was — the keys
  the game has been told are down — and a claimed release is forwarded when the
  key is on it. A claimed _press_ still does not reach the game and no longer
  joins the list, so the game only ever sees matched pairs. The list already
  cleared correctly here; clearing it was never the fix, because nothing but
  focus loss reads it.

- **crcbl-shell** (AppKit): `CrcblWindow` overrides
  `constrainFrameRect:toScreen:` to answer the proposed rectangle unchanged, so
  AppKit can no longer silently rewrite a frame this backend sets. The default
  keeps a title bar clear of the menu bar, which is right for a window a person
  dragged and wrong for every frame here — all of them are computed from an
  `NSScreen` rectangle and are on that screen by construction. `setFrame:` also
  now reads the frame back and logs when a window did not go where it was put,
  which nothing above this layer could otherwise notice: `WindowState` carries
  an extent and no position.

  That override was necessary and not sufficient; the defect that prompted it is
  fixed in the entry below.

- **crcbl-shell** (AppKit): **a mode change put the window back where it was
  created.** `DisplayMode::Borderless` produced a window of exactly the right
  size at the wrong origin — hanging off two edges of the display — and the way
  back was worse, restoring the creation frame's origin _and size_ rather than
  the placement the window had before the flip. Neither was visible through the
  seam, which carries an extent and no position.

  The cause is a fact about AppKit worth stating on its own:
  **`-[NSApplication setPresentationOptions:]` returns every window of the
  application to its creation frame.** Not the window it is called about — the
  property is on `NSApplication` — and not "constrains it to the screen". The
  backend applied the borderless presentation options _after_ placing the
  window, so every frame it set was immediately thrown away, on both legs of the
  round trip.

  `apply_mode` now applies the style mask, then the presentation options, then
  the frame, making the frame the last geometry it sets. The middle position
  matters as much as the last: applying the options before the style mask
  changes makes AppKit raise `NSInvalidArgumentException`, and an Objective-C
  exception unwinding through Rust aborts the process. `appkit::window`'s module
  docs carry the measurement and all three positions, since anyone reordering
  those statements would otherwise reintroduce one defect or the other.

- **crcbl-shell** (AppKit): **a mode change took the keyboard away from the
  view.** `-[NSWindow setStyleMask:]` rebuilds a window's frame view and the
  content view stops being the first responder — so after a flip to
  `DisplayMode::Borderless`, or back, `sendEvent:` delivered every key event to
  the window and `CrcblView` received none. A game that pressed F11 went
  permanently deaf, silently, with no error anywhere. `apply_mode` now re-claims
  the first responder after each style-mask change, sharing `focus_content_view`
  with window creation so the two cannot drift, and the session asserts the view
  still has the keyboard **after the borderless leg** as well as after a full
  round trip — a game stays borderless, so a responder restored only on the way
  out would be a game that is deaf for as long as it is being played.

- **crcbl-shell** (AppKit): windows no longer take part in **macOS state
  restoration**. `isRestorable` defaults to `YES`, which enrols a window in a
  feature this backend cannot honour and should not want: restoration re-creates
  windows at launch through a `restorationClass` or an application-delegate
  callback, neither of which exists here — the backend deliberately never takes
  the delegate slot — and it makes the operating system a second, invisible
  source of truth for a placement the seam hands to `WindowDesc` and a game
  hands to its settings screen. It also writes saved state to disk keyed by an
  application identity an unbundled binary does not stably have.
  `setRestorable:` is now `NO` at creation. Argued on its own merits; whether it
  also accounts for the borderless-origin defect above is a separate question.

- **crcbl-shell** (X11): hiding a window with `set_visible(false)` unmapped it
  without telling the window manager. ICCCM 4.1.4 requires a synthetic
  `UnmapNotify` to the root alongside the unmap, because a reparenting manager
  watches the frame it created rather than the client window inside it and may
  never see the real event. Under `openbox` the window was unmapped and remapped
  before the application could observe it hidden.

- **crcbl-shell**: `WindowState::mode_request_honoured` compared the requested
  and effective modes with `==`, which is wrong whenever the backend can name
  the monitor. `Borderless { monitor: None }` means "wherever the window already
  is" as a _request_, so an answer of `Borderless { monitor: Some(..) }`
  satisfies it — but the two are not equal, so every granted fullscreen on X11
  read as refused and a UI toggle over a fullscreen window would have shown
  "off". The comparison is now `DisplayMode::satisfied_by`, which keeps the
  asymmetry: a request naming a monitor is still not answered by one that cannot
  say which.

- **crcbl-shell** (X11): the backend never wrote `WM_HINTS`, so it never told a
  window manager that its window wants the keyboard. ICCCM 4.1.7 lets a window
  manager assume convenient values when the property is absent, and "this window
  takes no input" is one of them — a game whose window is never focused receives
  no key for its whole run. It now writes `input = True` with `NormalState`,
  which is ICCCM's passive focus model and what every toolkit does. **Changed
  nothing measurable under openbox**, which defaults the other way; this is
  conformance rather than an observed repair.

- **crcbl-shell** (X11): a `set_mode` issued after a window was configured but
  before its `MapNotify` arrived was silently dropped. `apply_fullscreen` chose
  between writing `_NET_WM_STATE` and sending a `ClientMessage` on whether the
  window was mapped — which follows `MapNotify` — but a window manager begins
  managing a window at the map _request_, and on X11 the first configure also
  arrives before `MapNotify`. A game that opened a window, waited for its size
  and asked for fullscreen landed in that gap every time: it wrote a property
  the window manager then overwrote with its own view. It now branches on
  `XWindow::map_requested`, and the whole X11 suite runs under `openbox` in CI
  as well as under bare Xvfb.

- **crcbl** (`engine`): a run that ended because the player closed the window
  reported `DisplayMode::Windowed` whatever mode it had been in. Accepting a
  close request destroys the window, and the summary is built afterwards, so
  `ModeRequest::mode` had nothing left to read and fell back to its default — in
  the same words a genuinely windowed run uses, so nothing downstream could tell
  the two apart. `ModeRequest` now records the mode it last saw and the new
  `ModeRequest::mode_at_exit` prefers the live answer, falling back to that.
  `Loop::finish` and `apps/bare` both use it.

- **crcbl-shell** (X11): a window created with
  `WindowDesc { mode: Borderless, .. }` reported its own request back as the
  effective mode when no window manager was running. EWMH has the _client_ write
  `_NET_WM_STATE` to request an initial state — before a window is mapped there
  is no window manager conversation to have — and a window manager then takes
  ownership of the property. The backend worked out the effective mode by
  reading that property back, so with nobody to take ownership it read its own
  write: `effective_mode()` said borderless and `mode_request_honoured()` said
  true, for a window still at its windowed size that nothing had touched. It now
  trusts `_NET_WM_STATE` only when `_NET_SUPPORTING_WM_CHECK` says something is
  there to have written it.

  `set_mode` after mapping was never affected — that path sends a client message
  to the root window and never writes the property — so the bug was reachable
  only through the creation path, which is exactly the path the new
  `--fullscreen` flag takes. Every WM-less X session, kiosk and CI runner would
  have had a summary line claiming a fullscreen it did not have.

- **horde**: `--wall-clock` stopped reaching the clock. Hosting the game in the
  engine's loop changed the wiring from `Clock::new(!real_clock())` to
  `Clock::new(headless)`, so a headless run with the flag read the fake
  fixed-step clock and the debug panel's frame timing reported the step rather
  than the frame — every headless scale measurement since was measuring nothing.
  The wiring is restored, and a regression test pins a headless `--wall-clock`
  run on the real clock while a headless run without it keeps the fixed step.

### Added

- **crcbl** (`args::Common`): `--fullscreen`, and `Common::display_mode()` that
  turns it into a `DisplayMode` for `WindowDesc::mode`. Asked for at window
  creation rather than switched to afterwards, so a fullscreen game does not
  show a decorated window for the frames a `set_mode` would take to land. `F11`
  still toggles from either starting point. Every sample honours it —
  `apps/sandbox` through its own parser, which predates the shared one.

- **samples**: the summary line each binary prints now names the display mode
  the window system actually settled on, beside the extent. `RunSummary::mode`
  already carried it and nothing reported it, which left a refused fullscreen
  indistinguishable from an honoured one from outside the process. `apps/bare`
  gained a `Summary::mode` field to do the same from a hand-written loop, via
  the public `engine::ModeRequest::mode`.

- **crcbl-sprite** (`bake::bake_dir`): the generated table now declares
  `ART_TICK_HZ`, the rate the holds were baked at. A `.crpix` counts holds in
  simulation ticks and an Aseprite sidecar counts milliseconds, so the
  conversion runs once at bake time and once at load time and the two must agree
  — and a build script cannot `use` the crate it builds, so every consumer
  declared the number a second time beside its loader. Five copies (`apps/*` and
  `crcbl-render`) are deleted; the `build.rs` value is the only source.

- **crcbl-phys**: `DampingForce::world_force(velocity, mass, dt)` and
  `DragForce::world_force(velocity)`, beside the `ThrustForce::world_force` that
  already existed. A force provider applies to **every** dynamic body, so a game
  damping one entity among a field of others could not use the pipeline;
  `apps/asteroids` wrote `-k·v` and the `mass/dt` cap out by hand instead, and
  that copy is now deleted. The cap travels with the route — it is what stops a
  coarse tick rate from over-damping past zero and flying the body backwards.

- **crcbl-phys**: `overlap_sphere_into` on both `PhysicsSystem` and
  `PhysicsWorld`, and `Bvh::traverse_aabb_into`, so a game that queries once per
  body per tick can hoist one buffer out of its loop. The owned forms cost three
  `Vec`s per call — the result, the collider ids, and the BVH's candidate list —
  and the descent stack a fourth; the `_into` path clears and refills the
  caller's buffer and keeps the rest as fields, so a crowd steers without
  allocating. The owned forms remain, unchanged for every existing caller, and
  now delegate.

- **crcbl-phys**: `PhysicsSystem::body_mut(entity) -> Option<&mut RigidBody>`,
  for a game that chooses a velocity rather than having one integrated onto it.
  `set_body` was the only writer and it costs two hash operations — an insert
  into the body map and a touch of the transform map — to change one `DVec3`,
  which a crowd pays once per agent per tick; `apply_force` is not an
  alternative, because a kinematic body's zero inverse mass makes a force a
  no-op. It cannot move a collider: position lives in the transform, and
  `set_transform` is still what tells the broadphase.

- **crcbl-render** (`sprite_pass`): `batch_count(&[Sprite]) -> usize` answers
  how many draw calls a sprite list will cost, without a device. The batching
  rule — a run of consecutive sprites naming one sheet is one draw, so `A A B A`
  is three and not two — was previously readable only by writing it out again,
  which `apps/horde` did to put the number on its debug panel. It delegates to
  the batcher the pass itself uses, so it cannot drift from it.

- **crcbl**: the simulation half of the engine is re-exported, so a game names
  `crcbl` and the standard library and nothing else. `crcbl::ecs`,
  `crcbl::phys`, `crcbl::net`, `crcbl::server`, `crcbl::client`, `crcbl::input`,
  `crcbl::audio`, `crcbl::store` and `crcbl::sprite` join the graphics stack
  that was already there, and `crcbl::log` re-exports the logging facade — its
  macros resolve through `$crate`, so `crcbl::log::info!` expands exactly as
  `log::info!` does and no wrapper macro exists.

  The umbrella's headline claim has been "one dependency for a game" since it
  was written, and until now only `apps/sandbox` could keep it: the other four
  samples each named eleven workspace paths beside it. None of the nine crates
  depends on `crcbl`, so this is nine `pub use` lines rather than a
  restructuring — the arrows already pointed this way and nobody had drawn them.

  `crcbl::sprite` is the reader (`load`), never the encoder. A build script that
  bakes art still names `crcbl-sprite` itself with its `bake` feature, which is
  the one dependency a sample continues to spell out, and is what keeps a PNG
  encoder out of a shipped binary.

- **crcbl** (`crcbl::engine`): `Pending` folds the whole of a pump batch that
  belongs to the loop rather than the game — the pointer, focus loss, and the
  three reserved keys `DEBUG_OVERLAY_KEY` (F3), `PAUSE_KEY` (Escape) and
  `FULLSCREEN_KEY` (F11), which are now the engine's constants. `observe`
  returns `Handled::Loop` or `Handled::Game`, so a sample's pump closure is a
  guard and its own key handling; `Pending::carrying` starts a batch from where
  the last frame left the cursor.

  The pointer half was **byte-identical in all four samples**, and it is not
  trivial code: it carries the last position across frames because motion and
  buttons arrive as separate events and a click carries a position only on some
  backends. The reserved keys were three constants spelled out five times, and
  they are the engine's because the thing F3 opens is the engine's.

  196 code lines out of the four `app.rs` files. What is left there is the loop
  — the fixed-step accumulator, teardown, the summary — which is still four
  copies.

- **crcbl** (`crcbl::args`): the flags every sample has. `Common` holds
  `--headless`, `--frames`, `--tick-hz`, `--backend` and the debug-overlay pair,
  with `frame_budget` and `debug_overlay_visible` on it; `Common::consume`
  offers one argument to that set and answers `Yes`, `Help`, `Bad(message)` or
  `No`. `Invocation<T>` wraps a game's own options, `COMMON_OPTIONS_HELP` and
  `COMMON_TAIL_HELP` are the shared `--help` blocks, and `positive`/`number`
  parse a flag's value with the rejection wording the samples already used.

  **Offered, not imposed.** A game keeps its parse loop and its `Options`
  struct, and claims what `consume` hands back — which is how `--seed`,
  `--max-enemies`, `--prefill` and `--wall-clock` stay per-game, and how
  `apps/sandbox` goes on taking `--camera` and `--title` while not being a
  consumer of this at all.

  The four game parsers were the same file: flappy's and asteroids' differed in
  **eight lines**, six of them usage prose. 894 code lines across the four
  became 599 against 270 in the engine, and the flags themselves are now tested
  once rather than four times. Each sample keeps one test that the engine's
  cannot make — that its parser actually _calls_ `consume`, since one that
  forgot would pass every test in `crcbl::args` and still reject `--headless`.

  The drift this closes was real: three of the four parsers had dropped
  breakout's assertion that the default backend stays `None`, which is what
  stranded CI on a machine with no driver. Each sample's `USAGE` now asserts it
  contains both shared help blocks byte for byte, so a reworded flag description
  reddens the build instead of shipping.

- **crcbl-store** (`crcbl::store::record`): `Record`, one `u32` kept between
  sessions. `Backing` picks where — `None` for a headless run that must leave no
  trace, `Backing::config(app)` for the platform's config directory, and
  `Backing::Browser` for a store the page's shim installed. `raise` writes only
  when the new value is larger; `set` is for the game whose better is smaller.

  The crate handed out a `StorageSource`, an atomic write and a
  platform-standard root and stopped there, so every sample that wanted a high
  score wrote the platform arms, the little-endian encode, the corrupt-file case
  and the headless rule itself. Four did, and the bodies matched line for line
  under names that agreed about nothing — `HighScore` in `high_score.bin`,
  `Best` in `best.bin`, and horde's `Best` whose number is a run length rather
  than a score. 987 lines of sample code became 389, and what is left is the
  part the engine could not have guessed: which directory, which file name, and
  which browser store.

- **crcbl** (`crcbl::session`): `Loopback`, the single-player session. Pairs an
  in-memory transport, builds the `Server` on one end and the `Client` on the
  other with the same tick rate and the same `ProtocolCompatibility`, hands the
  server its `GameModule`, and spends both clocks' first update at time zero.
  `tick_period`, `server`/`server_mut`, `client`/`client_mut` and `both_mut`
  reach the halves.

  "Single-player is a loopback server" is the engine's architectural decision —
  it is why `crcbl-server` and `crcbl-client` exist at all — and until now
  nothing in `crcbl` expressed it, so all four games implemented it from
  scratch. What stays the game's is what genuinely is: its
  `ProtocolCompatibility`, whose `schema_hash` is what stops one game's client
  hand-shaking with another's server, and its `GameModule`. Neither has a
  default, because a default for either is the wrong answer quietly.

  The baseline update at time zero is the subtle half. A `FrameClock`
  establishes itself on its first update and runs no ticks for it; doing that at
  construction is what lets a game's `tick` promise that every later call runs
  exactly one. Left to the caller, the first frame of the game silently
  simulates nothing.

- **crcbl-audio** (`crcbl::audio::synth`): waveform generators. `sine` for a
  one-shot beep, `looped_sine` for a tone that joins to itself, `noise_burst`
  for a decaying impact, and `fade_gain` for the click-free envelope under the
  first and last. Deterministic: `noise_burst` draws from a caller-supplied seed
  through `crcbl_core::rand`, so the sound a build ships is the sound every
  build ships.

  The crate had a mixer, a sound bank, an output stream and a spatial cue
  grammar, and no way to make a _sound_ — so all four samples wrote one. `sine`
  and its fade helper were byte-identical in flappy, asteroids and horde;
  breakout had the same pair under the names `gen_sine` and `fade_env`.

  Three functions, not a synthesiser: no envelope generator, no filter bank, no
  configurable oscillator type. Three is what the four samples between them
  actually use. Horde's swept `rise` has one caller and stays in horde, now
  built on `synth::fade_gain` and `synth::TONE_AMPLITUDE` so its level cannot
  drift from the engine's.

  **Nothing about the shipped audio changed** — the generators were adopted
  verbatim, and the sample buffers were compared to the engine's element by
  element before the copies were deleted.

- **crcbl** (`crcbl::engine`): frame pacing. `FrameLimit` caps how fast a
  real-time loop runs — a thousand frames a second by default, which is a
  runaway guard rather than a pacing policy, and `Clock::set_limit` changes it.
  The limiter lives on the clock rather than in the loop because every sample
  already calls `Clock::advance` once a frame, so a game gets it without asking;
  and because a manual clock has no wall clock to wait against, a headless run
  is unpaced **by construction** rather than by a check somebody has to
  remember.

  `Pacing` — `Vsync`, `Adaptive` or `Off` — replaces the hard-coded present-mode
  preference and is set through `GpuContextDesc::pacing`. One value rather than
  two flags, so "vsync on, adaptive sync on" is a state that cannot be written
  down instead of one the engine rejects at run time.

  **Nothing here turns adaptive sync on**, and that is not an omission: VRR is
  negotiated between display, driver and compositor, and an application never
  enables it. What changes is what presenting means — on a VRR panel the present
  does not wait for a fixed vblank, the panel follows the presents — so the
  engine's job is choosing a present mode and then staying inside the panel's
  range, which is what the limiter is for. Whether a panel is _actually_ running
  variable-refresh needs `VK_EXT_present_timing`, which is provisional and has
  no bindings in the pinned `ash`; until then `Adaptive` is a request rather
  than an observation.

- **crcbl** (`crcbl::engine`): `Loop`, the frame owned by the engine, and
  `HostedGame`, the seam a game reaches it through. `Loop::frame` pumps the
  shell, routes the input, runs the ticks the clock owes, draws and presents;
  `HostedGame` is the six things that genuinely differed between five samples —
  `menus`, `tick`, `key_event`, `menu_action`/`apply`, `menu_kind`, `draw` — and
  `summary`, which adds a game's own fields to the shared `RunSummary`.
  `FrameInfo` tells a `draw` what its frame did, and `LoopConfig` carries the
  three values that come from the command line rather than the game. `Loop`
  implements `GameLoop`, so `drive` and `crcbl::web::App` step it unchanged.

  `GameGpu` is the frame's half of a game's GPU bundle — `atlas`, `set_menu`,
  `take_draw_list`, `timings`, `frame`, `destroy` — and all five samples already
  had every one of them, with these signatures, as inherent methods.

  **`HostedGame` is not `crcbl::ecs::GameModule`.** That one is the simulation
  the server hosts and a wasm binding will have to reproduce bit for bit; this
  one is the presentation the loop hosts. A game implements both.

  `PolledGpu`'s `extent` and `resize` move to a new `GpuSurface` supertrait,
  which `PolledGpu` and `GameGpu` both require — the same two questions, asked
  by start-up and by the running frame, and declaring them twice on one type is
  how the two answers drift apart. The four samples with a browser build split
  their existing `impl` accordingly; nothing else changes for them.

  `apps/bare` never adopts it: it is the guard that the library path —
  assembling `GpuContext`, `Pending` and `FrameBudget` by hand — keeps working,
  and `crates/crcbl/tests/library_seam.rs` is what proves it from outside the
  crate.

  585 lines of engine and 343 of fixture and tests, against a `FakeGpu` that
  counts presents and a `FakeGame` that records what the loop asked of it —
  including an assertion that the loop never asks a game about a reserved
  `WidgetId`, which is what would silently re-point a resume button.

### Changed

- **crcbl-cli** (`crcbl new`): the scaffold now hands you the engine-owned loop.
  `src/main.rs` was 276 lines that opened the shell, called
  `unsafe { instance.create_surface(&target) }` itself, configured its own
  swapchain and ran its own `loop {}` — while every sample had stopped doing any
  of that and no crate under `apps/` contains an `unsafe` block at all. Its doc
  comment argued the loop was "deliberately yours rather than the engine's"
  because "an engine that owned it could not run in a browser", which
  `crcbl::web` had already disproved in four published demos.

  A generated project is now a `HostedGame` and a `GameGpu` over
  `crcbl::engine::GpuContext`, and arrives with a pause menu on `ESC`,
  fullscreen on `F11`, the debug panel with per-pass GPU timings on `F3`, mouse
  and keyboard menu navigation, and resize handling — none of which the old
  template had. It parses its flags with `crcbl::args::Common` and builds its
  help text from the engine's own two blocks, so `--tick-hz`, `--backend` and
  the debug-overlay pair work and cannot drift. `log = "0.4"` is gone from the
  generated manifest: `crcbl::log` covers it, so a new project starts with the
  same single dependency the samples have. The template ships three unit tests
  and `crcbl-cli`'s scaffold e2e now runs them.

  One consequence to know about: a generated project goes through
  `crcbl::backend`'s real registry, where the old template hardcoded
  `NullInstance`. That registry never falls back to null on its own, so the
  generated `.github/workflows/ci.yml` names `--backend null` — a stock CI
  runner has no driver, and without it the first push fails with
  `ERROR_INCOMPATIBLE_DRIVER`. Drop the flag once that job installs one.

  The library-style loop is still supported and is still `apps/bare`, guarded
  from outside the crate by `crates/crcbl/tests/library_seam.rs`. What changed
  is which of the two a new project starts from.

- **breakout**: the first game hosted by `crcbl::engine::Loop`. `Breakout` is
  seven `HostedGame` methods and three fields — the simulation, the state it
  renders from, and its HUD — where `app.rs` used to carry the whole frame.
  `Loop<S>` is now a type alias for the engine's, so `run`, `start` and
  `with_shell` are free functions rather than inherent methods on it.

  Its menu vocabulary shrank to the part that was ever breakout's: `Launch`, on
  `LAUNCH_ID = FIRST_GAME_ID`. `MenuAction::{Resume, Fullscreen, DebugOverlay}`
  and the ids that carry them are the engine's, and `web.rs` lost its whole
  `WebLoop` impl — `crcbl::web` blanket-implements it for every engine loop,
  taking the name and the summary line from `HostedGame::NAME` and
  `HostedGame::log_summary`.

  **Nothing about the game changed**, and its own tests are the evidence: all 79
  pass unmodified except where they reached a field that is now behind an
  accessor, and the browser gate ran 27/27 checks against a real WebGPU device.
  `app.rs` lost 309 lines and `web.rs` 27, against 30 of `GameGpu` forwards in
  `gpu.rs`.

- **flappy**: hosted by `crcbl::engine::Loop` too, on the same shape as breakout
  — `Flappy` is seven `HostedGame` methods over the simulation, its render state
  and its HUD; `Flap` on `FLAP_ID = FIRST_GAME_ID` is all its menu vocabulary
  still declares; `web.rs` lost its `WebLoop` impl.

  It needed nothing the seam did not already have, which is the useful result:
  the bird's wing animation is stepped by `FrameInfo::ticks`, the field added
  for exactly this. Its own 86 tests pass and its browser gate ran 27/27.
  `app.rs` lost 288 lines and `web.rs` 28, against 30 of `GameGpu` forwards.

- **asteroids**: hosted by `crcbl::engine::Loop` as well, and it gained a fix on
  the way: **a refused fullscreen is now reported.** The sample never called
  `check_mode_request`, so a player on a tiling window manager pressed F11 and
  got no window change and no log line saying why; the engine's loop checks once
  a frame for every game it hosts.

  `Fire` on `FIRE_ID = FIRST_GAME_ID` is what its menu vocabulary still
  declares. `render_alpha` stays — this is the sample that interpolates
  rotations across a tick, and `FrameInfo::alpha` is where the number now comes
  from. `app.rs` lost 234 lines and `web.rs` 29; its 93 tests pass and its
  browser gate ran 27/27.

  The seam grew `Loop::{set_paused, gpu_mut}` for it: a test paused the loop by
  assignment, and its sprite read-back takes `&mut self`.
  - **sandbox**: the last conversion, and the one that measures the others.
    `Sandbox` is a struct with **no fields**: the sandbox has no simulation, no
    HUD and no score, and it still runs, pauses, opens a menu, goes fullscreen
    and reports a summary — all of that is the engine's now. Its `MenuAction` is
    `Infallible`, which makes `MenuAction::Game` uninhabited and is the type
    system agreeing that its three buttons are the loop's.

  It also stops declaring the six reserved keys for itself. `DEBUG_OVERLAY_KEY`
  and its five siblings were the engine's constants already, and a second
  declaration is how "the same key does the same thing in every sample" quietly
  stops being true.

  `app.rs` lost 379 lines and `menu.rs` 29; its 35 tests pass.

  `FrameInfo::tick_dt` and `HostedGame::tick` widened from `f32` to `f64`, which
  is what `FrameClock::tick_dt_secs` reports — the sandbox is the only game that
  reads it, and narrowing it was the engine deciding a precision on a game's
  behalf. `Loop::events` joins the accessors for the same reason the others did:
  a test read the field.

- **horde**: hosted by `crcbl::engine::Loop`, and the sample that stretched the
  seam. Its level-up panel is three upgrades the run's seed picked, so
  `HostedGame::menu_kind` now takes the loop's own `MenuSet` and a game may
  rebuild a panel before the kind it returns is shown. Its debug panel carries a
  section no other sample has, so `HostedGame::debug_sections` exists — empty by
  default, because "this game adds no section" is the honest answer for the
  other four. And it is the first game with **two** menu actions, `Restart` on
  `RESTART_ID` and `Choose(n)` on a reserved block above it.

  It also gains the refused-fullscreen report, for the same reason asteroids
  did. `app.rs` lost 205 lines and `web.rs` 32; its 124 tests pass and its
  browser gate ran 27/27.

  **The CPU frame report moved into the engine.** `Loop::finish` logs the clock
  it was driven from, the frame count, and mean/fps/best/worst — `apps/horde`
  wrote that itself and `--wall-clock` exists to make it mean something; every
  hosted game gets it now. The scene stats it used to carry are on horde's own
  `Summary` instead, so `main.rs` prints them natively and `log_summary` does in
  the browser.

- **crcbl** (`crcbl::engine`, `crcbl::web`): the sample loops' shared machinery
  moves into the engine, in four further slices.

  `open_window` logs the backend, aligns the shell's event clock with the
  engine's and creates the window, taking the caller's `WindowDesc` because a
  title and a size are the game's. `MAX_FRAME_STEP` joins it as an engine
  constant: the browser behaviour it guards against is the shell's.

  `PolledBoot`, with the `PolledGpu` trait, owns browser start-up — the pump,
  the configure/device state machine, the fix for a canvas resized while the
  device request is in flight, and the refusal to restart a boot that already
  finished or failed. It hands back `Booted` rather than a loop, because
  assembling one is the game's.

  `MenuPump` owns the menu's half of a pump batch: the three menu keys
  (`MENU_UP_KEY`, `MENU_DOWN_KEY` and `MENU_ACTIVATE_KEY`, now the engine's
  alongside the three reserved ones), the select/press/activate routing, and the
  held-key list. It answers with a `WidgetId`, leaving the mapping to a game's
  own action enum where it belongs.

  `crcbl::web` takes the browser entry point's shared half: the status codes — a
  wire format the JS shim switches on, so one definition is the only way they
  stay in step — the bounded log queue, and the whole `App` lifecycle behind the
  `WebLoop` and `WebPending` traits. It is deliberately not gated to `wasm32`,
  because gating it would put its tests on the one target the suite never runs.

  `run_ticks` is the fixed-step accumulator, with the rule that a **paused**
  frame still drains — the alternative banks the pause and spends it in one
  catch-up burst on the frame the player resumes. `FrameBudget` replaces the
  three fields every sample carried separately, because the reconfigure cap
  exists only so that a budget counting _presented_ frames stays reachable.
  `lose_focus` releases every held key before pausing, so a game does not resume
  believing a key is still down. `drive` is the native driver, behind a
  `GameLoop` trait that `crcbl::web::WebLoop` now requires — so the native and
  browser paths provably step the same loop.

  `PointerCapture` holds what the loop remembers about the pointer between
  frames — where it was left and whether its button is down — and resolves a
  batch into a `PointerInput`. `ModeRequest` holds the fullscreen request and
  whether the window system agreed, reporting what the window actually is rather
  than what was asked for.

  Measured: the four `app.rs` files lost 919 lines, and the four `web.rs` files
  went from 2642 to 1466. What the samples keep is what genuinely differs — each
  game's `assemble`, its `MenuAction` handler, its HUD, and the one log line
  reporting what a finished run was worth.

- **crcbl** (`crcbl::engine`): `LoopError<G>` replaces the error enum each
  sample wrote out for itself. The five loop failures — `NoWindowSystem`,
  `Shell`, `Configure`, `NeverPresented` and `Gpu` — belong to the loop however
  the game above them is spelled, and `G` names whatever the game itself
  refuses. A game with nothing of its own to refuse leaves it at the default
  `Infallible`, which makes the `Game` variant uninhabited and costs nothing.

  `BreakoutError`, `FlappyError`, `AsteroidsError`, `HordeError` and
  `SandboxError` are now aliases for it, so they keep their names and every
  `Err(FlappyError::Gpu(…))` still reads the same. `ShellError`,
  `ConfigureError` and `GpuError` still convert with `?`; a game error is
  wrapped by name, `.map_err(FlappyError::Game)`, because a blanket `From<G>`
  cannot coexist with the three concrete ones — `G` may itself be `ShellError`.

  Two messages change as a result. The sandbox's `NoWindowSystem` hint no longer
  names a roadmap phase for the missing Win32 and AppKit backends, since the
  engine has no business quoting one; it still says a platform may have no shell
  backend and still points at `--headless`. And its `NeverPresented` message
  loses a run of eighteen spaces that a missing line continuation had baked into
  the string literal.

- **samples**: `apps/{breakout,flappy,asteroids,horde}` drop eleven dependencies
  apiece and `apps/sandbox` drops its last one. `glam::` is `crcbl::math::` and
  `log::` is `crcbl::log::` at every call site — the same crates through the
  umbrella, so no version can drift and no two copies of a `Mat4` can meet.

- **crcbl** (`crcbl::engine`): the default present mode is now `Fifo` rather
  than `Mailbox`. A windowed native run vsyncs unless it asks not to, where it
  previously ran uncapped. The browser is unchanged: its swapchain already
  logged `Fifo` before this and logs it after, because the WebGPU surface does
  not offer `Mailbox` for the old preference to have found.

- **horde** (`apps/horde`): the engine's fourth game and its scale sample — the
  core loop. One arena, one player with WASD movement and an auto-aiming weapon,
  three enemy kinds that seek and push off each other, contact damage, hit
  points, death and restart. Native and headless; `--max-enemies` sets the
  ceiling on live enemies (default 1500). Drawn as untextured quads through the
  UI pass, which the art sub-slice replaces.

  Where the earlier samples ask what the engine can host, this one asks **what
  one tick costs per live body**, so the interesting part is the query pattern.
  Separation is one `PhysicsSystem::overlap_sphere` per enemy per tick, of
  radius `r_self + slack` — and the omission of the _neighbour's_ radius is
  exact rather than sloppy, because a shape-aware overlap of radius `R` returns
  everything within `R + r_b`, which is precisely the pair set separation wants.
  Contact damage is one more such query, at `PLAYER_RADIUS`, where every result
  is by construction a hit. Aiming is a third, at the weapon's range, instead of
  a scan of the enemy list. The weapon itself is segment CCD.

  Provisional numbers were taken here and **superseded by the scale sub-slice
  below**, which measures a fixture that fits inside the arena and which
  separates a spread crowd from a converged one. Both sets are in
  `docs/plan/sample/03-horde.md` with their conditions.

  Two divergences from asteroids are deliberate. **The gun fires after the bolt
  sweep**, because a projectile swept on the tick it was created is swept from a
  point one whole step behind the muzzle, through the thing that fired it —
  asteroids has the same order the other way round, and the same latent segment.
  **A wall clamp is not a teleport**: it moves a body by at most one tick of
  travel, so it is a refit rather than the remove-and-re-insert asteroids'
  screen wrap needs.

- **horde** (`apps/horde`): art and progression. `.crpix` sprites for the
  player, the three enemy kinds and the XP pickups, baked by a `build.rs` and
  drawn through `SpriteRenderer` with `SampleMode::Pixel`, replacing the
  untextured quads the core loop shipped with. XP gems drop where an enemy died
  and are collected by walking over them; banking a threshold opens a "pick 1 of
  3" level-up screen over the frozen field, from a fixed pool of six upgrades
  (`RAPID FIRE`, `HEAVY BOLTS`, `SWIFT BOOTS`, `LONG BARREL`, `VITALITY`,
  `MAGNET`). Pause, level-up and death menus over `crcbl_render`'s shared menu
  art, with the pointer, F11 and focus handling the other samples have.

  **Two sheets, and the split is a batching decision.** `SpriteRenderer` starts
  a batch whenever consecutive sprites name a different sheet, so the player,
  all three enemy kinds and the gems are one 34-texel frame size in one sheet:
  the whole field is a single batch **whatever order it is emitted in**, with no
  grouping pass over the crowd and no way for the batch count to grow with the
  horde. Asteroids has to emit its rocks largest-first to hold three batches;
  this cannot get it wrong. What it costs is the transparent margin round the
  two small kinds — a runner is 13 texels of art inside a 34-texel quad — and
  that is bounded by the screen rather than by the field.

  The scale is 20 texels a world unit, chosen from the runner: three enemy kinds
  have to be told apart at a glance in a crowd, which needs about thirteen
  texels across, and 13 / 0.64 units is 20.3. No scale makes all three enemy
  collider boxes a whole number of texels — the radii were picked for how the
  game plays, and it would take 50 texels a unit — so the shared frame is the
  largest one, which at 20 is exactly 34, and each silhouette is drawn to its
  own collider inside it.

  A level-up **freezes the field**, and the freeze is simulation state rather
  than the loop's pause: which upgrade a run took changes what the simulation
  does, so a seeded replay has to reproduce it, and the menu presses a real
  digit key into the action map rather than calling into the game. The freeze
  costs one pass on the tick it opens — a zero velocity written to the player,
  every enemy and every bolt — rather than a branch on the tick's hot path.

- **horde** (`apps/horde`): audio, the longest run, the browser demo, and the
  scale measurement the sample exists for. Five procedural spatial cues — the
  gun, an enemy coming apart, a gem banked, a level gained and the player's own
  end — with the listener **on the player**, which is the first sample whose
  listener moves. The longest run survived is kept in `~/.config/horde/best.bin`
  or the browser's Origin Private File System, in whole seconds so the record
  compares as the `m:ss` the HUD shows. The demo is live at
  `https://crcbl.kryptic.sh/demos/horde/` and the browser gate covers it at
  26/26, alongside the other three.

  **`crcbl-audio` has no voice limit, and this is the first sample that could
  not ignore it.** A kill is a cue and a gem is a cue against a fire cooldown
  whose floor is a twentieth of a second, so a late run raises about forty a
  second and each is a voice that lives until it runs out. The sample caps
  itself at sixteen, refuses the newest, and counts the refusals — and keeps
  counting the _cue_, because "did this happen" and "was there a speaker free"
  are different questions and only the first is what a test should be able to
  ask.

  Two flags carry the measurement, and both are in the shipped binary because
  the numbers have to be reproducible from a command line: **`--prefill N`**
  stages `N` enemies over the whole arena before the first frame (the spawner
  would take over ten minutes to reach the plan's target and nothing survives
  that long) and raises `--max-enemies` to fit them; **`--wall-clock`** drives a
  headless run from the real monotonic clock, so the debug panel's frame-timing
  module measures the frame instead of reporting the fixed step a headless clock
  hands it. The panel also gains this sample's own `scene` section — field,
  culled, drawn, batches — so the numbers the sample's argument rests on are
  readable in the running game.

  **The measurement, with its conditions in `docs/plan/sample/03-horde.md`.** On
  a Radeon RX 7900 XTX (radv), release, headless offscreen ring at 960 × 720,
  single-threaded:
  - **The render side is flat and the exit criterion is met.** CPU frame time
    0.096 ms on an empty field and on a field of a thousand, and 0.120 ms with
    ten thousand — nine thousand more enemies for 24 µs a frame, 0.14 % of a
    16.67 ms budget. With the driver taken out (`--backend null`) the game's own
    share is 0.005 ms to 0.033 ms. The `sprites` GPU pass goes 0.006 ms to 0.023
    ms.
  - **The batching claim holds.** Two draw calls at every count, and still two
    over ten thousand sprites with the whole field packed inside the view so
    that nothing is culled.
  - **The transparent margin is visible and does not matter.** The average enemy
    fills 31.5 % of its shared 34 × 34 quad, weighted by the mix the spawner
    deals, so about 12 µs of the sprite pass is margin at a full screen of the
    crowd — 0.07 % of the budget, against a grouping pass and an emission order
    to get wrong.
  - **The tick is what breaks, and it breaks on _density_ rather than on
    count.** Ten thousand enemies cost 14.66 ms a tick spread over the arena and
    84.09 ms once the crowd has converged on the player. Separation is one
    broadphase query per body and a query costs what its answer costs; a horde
    converges by construction. So the sample carries about ten thousand spread
    and about three thousand converged, and the plan's single figure was always
    going to be one or the other.

  **What that says about P7 and P8**, which is the reason the sample was built
  out of order in the first place: P8 (`crcbl-jobs`, the parallel schedule) is
  worth the whole of the gap — the steering pass is order-independent by
  construction and has no shared mutable state — and P7 (GPU culling, indirect
  draws, instance deltas) can return at most 0.7 % of a frame here, because the
  CPU cull it deletes costs 28 µs. The roadmap had horde waiting on P7; it was
  waiting on P8.

- **crcbl-render**: `Sprite::rotation` — sprites can turn. A per-sprite angle in
  radians, counter-clockwise, about the centre of the sprite's own `rect`. It
  rides in the fourth component of `SpriteInstance::sheet`, which was padding,
  so the instance is still 64 bytes and no buffer, stride or bind group changed.
  `Sprite` gains a field, so every struct literal that builds one needs
  `rotation: 0.0`; that is the only source-breaking part.

  Rotation interacts with `SampleMode::Pixel`, and both halves are decided
  rather than left to fall out. The **snap** stops rounding each corner once the
  quad is turned — a rotated quad has no axis-aligned rectangle to round onto,
  and rounding four corners independently shears it, changes its size and
  changes its effective angle, so a slowly turning ship would wobble — and
  instead translates the whole quad rigidly so its _centre_ lands on the pixel
  grid, which keeps the shape exact and still removes the sub-pixel crawl that
  translation causes. **Sharp bilinear needs no change at all**: `fwidth` is a
  per-fragment screen-space derivative, so it tracks the turned UV gradient by
  itself; being an L1 norm it reports up to root two times the scale on the
  diagonal, which widens the crossover band to about 1.4 fragments and never
  narrows it.

  A sprite with `rotation: 0.0` is **bit-identical** to one from before this
  change, by construction rather than by rounding luck: `sprite.slang` branches
  on the angle and the zero path is the arithmetic that was already there, down
  to the same SPIR-V `OpFMul`/`OpFAdd` pair. All eight existing golden images
  pass unchanged, at zero differing pixels.

- **crcbl-phys**: the broadphase BVH is **dynamic**. `Bvh::insert` and
  `Bvh::remove` add and drop one element along a single root-to-leaf path, and
  `PhysicsWorld::add_*` / `PhysicsWorld::remove` use them, so a world whose tree
  already exists no longer throws it away on every spawn and kill. A game that
  fires a bullet per shot and splits a rock into two used to pay a full
  `O(n log n)` rebuild for each of those events, every frame, on a tree it had
  just built. Batch population before the first query is unchanged: with no tree
  yet, adds accumulate and one bulk `Bvh::build` still runs, which produces a
  better tree than the same elements inserted one at a time.

  Insertion picks where a leaf goes by the surface area heuristic and the walk
  back to the root **rebalances** (AVL single rotation), which is what makes the
  quality claim hold rather than depend on the input. Measured over 20k
  insert/remove pairs: peak depth 13 at 1024 elements against an ideal of 11
  (`ceil(log2 n) + 1`), and 9 at 64 against 7. Without the rotation the same run
  on 1024 _coincident_ boxes — where every candidate site costs the same and the
  heuristic has nothing to choose by — reached depth 623, a tree that is very
  nearly a linked list. `Bvh::depth`, `Bvh::len` and `Bvh::is_empty` are public
  so the property is observable; `crates/crcbl-phys/tests/churn.rs` bounds depth
  by the AVL bound over thousands of operations and checks every query against a
  brute-force scan after each one.

- **crcbl-phys**: `ThrustForce` and `DampingForce`, the first two L1 force
  providers driven by a game rather than by physics for its own sake.

  `ThrustForce` is the first force that reads the body's _orientation_:
  `F = magnitude · (rotation × local_direction)`. The local axis is named rather
  than fixed at `Transform::forward` (`-Z`) because a top-down 2D game turns its
  ship about Z, where `-Z` points at the camera and thrusting along it would
  drive the ship out of the playfield plane. `ThrustForce::world_force` exposes
  the same vector to callers who are not using the provider pipeline.

  `DampingForce` is `F = -min(k, m/dt)·v`. The cap is the point: plain `-k·v`
  integrated at `k·dt/m ≥ 2` _reverses_ the velocity and then grows it, so a
  coefficient that behaves at a 240 Hz substep explodes at a 10 Hz one. With the
  cap the worst case is a velocity that reaches exactly zero. `DragForce` is
  deliberately left uncapped — it is the physical law, and a caller modelling a
  fluid wants the law.

- **crcbl-phys**: `PhysicsSystem::apply_force(entity, force)` adds a force to
  one entity for the next `step`. Force providers are global — every dynamic
  body gets every provider — which is right for gravity and wrong for the thrust
  of the one ship among a screenful of rocks.

- **crcbl-ui**, **crcbl-render**, **breakout**, **flappy**, **sandbox**: the
  samples' start, pause and end-of-game states are **menus** — a nine-sliced
  pixel-art window frame with skinned buttons inside it, centred in the
  framebuffer at every aspect ratio, replacing the flat rectangle and three
  lines of text each sample drew from its own `draw_pause_menu`.

  The art is **shared** and lives in `crates/crcbl-render/assets/menu.crpix`,
  baked by that crate's new `build.rs`: `apps/*` cannot depend on each other, so
  per-sample art would have been the same window authored three times and three
  games that looked like three engines. `crcbl_ui::menu` owns the model and the
  layout — `Menu`, `MenuItem`, `MenuStyle`, `MenuLayout`, all in screen pixels
  with no device in the room — and `crcbl_render::menu` owns the pictures:
  `MenuArt` cuts the five frames out of the sheet, `MenuRenderer` draws them
  through a `SpriteRenderer` of its own with a screen-space camera, and the
  labels stay on the UI pass. `crcbl_render::ButtonSkin` and
  `crcbl_ui::Button::with_skin`, which shipped unused, are what the buttons are
  drawn with.

  **The keyboard still works, and the mouse now does too.** Every key a sample
  bound still does exactly what it did, and each is printed on the button beside
  it; the menus add Up, Down and Enter, taken only while a menu is on screen.
  Pointer motion and clicks reach `Menu::point` through `UiState`'s press
  capture, so a press that starts on one button and is released over another
  fires neither. Both devices produce the same action.

  Behind the menu the game keeps drawing and is dimmed by a scrim sprite — drawn
  by the menu's own pass, between the game and the UI, so the panel and its
  labels are not dimmed with it. Breakout's start menu is a fresh game only:
  `WaitingForLaunch` is also where a player waits after losing a life, and a
  modal between every life would be three panels a game.

- **breakout**, **flappy**, **sandbox**: a pause state, entered and left with
  **Escape** and entered by losing window focus. A paused loop stops calling the
  game's tick, so the simulation does not advance at all; the HUD's status line
  reads `PAUSED` rather than whatever the server last thought, and a menu is
  drawn over the frame — text through the existing HUD path, behind a single
  `draw_pause_menu(&mut DrawList, extent)` per sample that the art slice
  replaces without touching the state machine. Pause is the loop's, not
  `GameState`'s: it is the loop declining to advance the simulation, and a
  `Paused` variant would put a value in the authoritative server's state that
  depends on which window a compositor has focused. `Loop::is_paused` and
  `Summary::paused` report it.
- **breakout**, **flappy**, **sandbox**: a fullscreen toggle on **F11**, which
  asks the shell for `DisplayMode::Borderless` and reads back what the window
  system actually did. There is no remembered `fullscreen` flag to disagree with
  the compositor — `Loop::display_mode` and `Summary::mode` are the _effective_
  mode, the toggle picks its target from it, and a request the window system
  refuses is logged once and reported as the mode the window really has.
- **crcbl-shell**: `__crcbl_web_fullscreen(canvas, state)`, the web backend's
  new shim entry point. A browser grants `requestFullscreen` only from inside a
  user-gesture handler and wasm is never inside one, so the page's shim makes
  the call from its own `keydown` and reports the outcome here; the backend
  moves `WindowConfiguration::mode` to match, which is what finally lets
  `WindowState::mode_request_honoured` answer `true` in a browser. An exit
  nobody asked for — Escape, which reaches no key handler — is reported the same
  way.
- **web**: `engine/shell.js` handles **F11** itself (and swallows the browser's
  own, which fullscreens the window rather than the canvas), listens for
  `fullscreenchange`, and synthesizes a focus loss on `visibilitychange` — a tab
  switch does not always blur the focused element, so `blur` alone leaves a game
  holding keys it will never see released. The demo pages gained a
  `STATUS_PAUSED` (6) status line, and `tools/browser-e2e.mjs` gained a
  focus/pause group that blurs the canvas in a real browser, checks that the HUD
  heartbeat stops, that focus coming back does not resume on its own, and that
  Escape does.

  **On a canvas, the click that restores focus is also a click in the game.**
  There is no title bar to click, so `shell.js` calls `canvas.focus()` from its
  own `pointerdown` handler — which means "clicking back in" lands a real press
  at a real position, and a press that lands on the pause menu's `RESUME` button
  resumes, exactly as it would with the game already focused. Focus itself still
  never resumes, on any platform. The two are separate and the samples' new
  `a_focusing_click_off_every_button_leaves_the_game_paused` pins them apart.

- **crcbl-ui**: `crcbl_ui::debug` — the modular debug overlay every sample now
  ships. `DebugPanel` holds `DebugSection`s and names no system; a system
  contributes by implementing `DebugModule`, whose one method fills a section it
  is handed, and the frame calls `DebugPanel::add` once per system it actually
  has. `FrameStats` is the module every frame has: a rolling window of frame
  intervals reporting FPS, average, last, best and worst. FPS is frames divided
  by the time they took, not the mean of the instantaneous rates — the two
  disagree in exactly the case a profiler exists for. `DebugOverlay` bundles the
  panel with the frame window so a sample switches the whole thing on in one
  line. `Anchor::position` is the panel's anchoring arithmetic, lifted off
  `HudPanel` so there is one copy of it.
- **crcbl-render**: `FrameTimings` implements `crcbl_ui::debug::DebugModule`, so
  the per-pass GPU timestamps that already existed appear in the overlay as a
  `gpu` section — one row per pass, plus the total and the frame number. The
  adapter lives here rather than in `crcbl-ui` because the overlay is not
  allowed to know that a render pass exists.
- **breakout**, **flappy**, **sandbox**: the debug overlay, toggled with **F3**
  and defaulting to visible in a debug build. `--debug-overlay` and
  `--no-debug-overlay` override the default. Neither game has a network module —
  both run over `InMemoryTransport` — which is what makes them the check that
  the panel composes rather than hard-codes its sections. The sandbox gained a
  UI pass to carry it; it still has no HUD and is not getting one.
- **flappy**: a second game, playable natively and at
  `https://crcbl.kryptic.sh/demos/flappy/`. One button, a bird under gravity,
  and an endless procession of pipes whose gaps are a pure function of a seed
  and the pipe's index — so the client and the server agree about the course
  without a byte of it crossing between them. It exists to find out whether the
  engine could host a game that was not breakout; what it found is written down
  in `docs/plan/ROADMAP.md`.
- **asteroids**: a third game, playable headless and natively, and the
  workspace's first sample built around **entity churn** rather than around a
  fixed world. A ship that turns, thrusts and wraps; bullets that never miss;
  rocks in three sizes that split twice; waves that grow to a ceiling; score,
  three lives, game over and restart. Every random-looking number — where a wave
  enters, which way a split throws its children — is a pure function of a seed
  and an index, so a recorded script replays bit-identically and two games on
  one seed are the same game.

  It is the first consumer of the P6 physics slice, and the seams it uses are
  the ones that slice was bought for: `ThrustForce::world_force` through
  `PhysicsSystem::apply_force` for the engine, `sweep_sphere` over a
  `prev → cur` segment for every bullet, and `overlap_sphere` against the
  broadphase for the ship. **A wrap is a teleport, and a teleport is a
  remove-and-re-insert** — the rule `docs/backlog.md` left to whoever wrote the
  wrap, chosen here and applied uniformly to everything in the broadphase.

  It is drawn as **pixel art through the sprite pass**: five `.crpix` sheets
  under `apps/asteroids/assets/` — a ship, a shot, and one per rock size — baked
  to PNG by its own `build.rs` and drawn with `SampleMode::Pixel`. Ten texels to
  the world unit, chosen by the small rock: eleven texels is the least a rock
  can be and still have a lump stick out and a bite go in, and eleven over that
  rock's 1.1-unit diameter fixes the scale. Every rock's frame is then its
  collider's bounding square to the texel — 34, 20 and 11 — and the three are
  three drawings rather than one at three magnifications, which is what makes a
  split read as a rock breaking rather than as a rock shrinking.

  **It is also the first sample where a drawn thing turns**, which the
  `Sprite::rotation` above only made possible. The ship's heading and every
  rock's tumble are integrated once per simulation tick, so drawing the newest
  value on every frame stutters at any refresh rate that is not the tick rate;
  the renderer interpolates instead, with the frame clock's alpha.
  `game::lerp_angle` takes the **short way round**, which is the whole
  difficulty: a plain lerp from 350° to 10° spins the long way, once, on the
  frame after the heading crosses zero — and `turn_ship` keeps the heading in
  `[0, τ)`, so it crosses constantly. Positions are deliberately _not_
  interpolated: this playfield wraps, and unlike an angle a wrapped position is
  a real discontinuity.

  Presentation is the shape the other two samples set: start, pause and
  game-over menus through `crcbl_render::MenuRenderer`, Escape to pause, F11 for
  fullscreen, F3 for the debug panel, and a window that loses focus pausing and
  releasing every key it was holding. That last one matters more here than in
  either earlier sample, because turning and thrusting are _held_ actions: a
  release that never arrives is a ship that spins for the rest of the session.

  **Sound**: three spatial cues through `crcbl-audio`'s grammar — the engine,
  the gun, and a rock (or the ship) coming apart. The listener is the camera at
  the middle of the field and it never moves, so unlike in either earlier sample
  the pan and the distance both swing their full range: emitters are spread over
  the whole 32 × 24 playfield and cross it constantly. The explosion is a
  decaying burst of low-passed noise from a fixed seed rather than a tone,
  because a beep reads as scoring rather than as destruction. Thrust is the
  first _sustained_ cue any sample has needed and `crcbl-audio` has no looping
  voice, so it is a one-shot re-fired every `THRUST_CUE_PERIOD` — a constant
  that lives in the simulation, because the cue is raised inside the
  deterministic tick.

  **A best score**, kept in `~/.config/asteroids/best.bin` natively, in the
  Origin Private File System in a browser, and nowhere at all under
  `--headless`. Recorded once, on the edge into game over.

  **A browser build**: `apps/asteroids` is a `cdylib` on
  `wasm32-unknown-unknown` and the demo is live at
  `https://crcbl.kryptic.sh/demos/asteroids/`. `Loop` gained
  `PendingLoop`/`set_frame_step` and `Gpu` gained `request_open`, so start-up is
  polled across `requestAnimationFrame` frames instead of blocking on a promise
  the page's own event loop has to resolve. `web/run-browser-e2e.sh` drives it
  in a real Chromium for 26/26 checks, the same as the other two.

- **crcbl-hal**: `Device::take_error`, for the failures a backend learns about
  outside the call that caused them. Defaults to `None`, so a backend that
  reports everything through its return values is unaffected.
- **breakout**: the ball's speed ramps 2% per brick broken, capped at 1.6x the
  launch speed. A lost life and a restart both put it back.
- **crcbl-render**: `texture::upload_texture` and `UploadedTexture`, a
  format-agnostic staging upload. It replaces `ui_pass`'s private R8-only
  helper, whose row pitch was computed in texels and passed to a copy that wants
  bytes — correct only because `R8Unorm` is one byte per texel. The pitch is now
  computed in bytes and converted back once, at the copy, so an RGBA8 upload
  lands where it says it does.
- **crcbl-sprite**: a `load` feature — `decode_png`, `read_aseprite_json` and
  `load`, which take a baked sheet back apart into a `Sheet` and tightly packed
  RGBA8. §7 of `docs/specs/crcbl/pix.md` specified what the sidecar contains and
  nothing read it, so a baked sidecar was write-only. `SampleMode` does not
  survive the trip — Aseprite's schema has nowhere to put it — and that is
  asserted rather than assumed.
- **crcbl-render**: `SpriteRenderer` and `sprite.slang`, an instanced
  world-space pass that draws one quad per sprite out of a registered sheet,
  alpha blended, batched by sheet in submission order. This is the instance path
  S1B finding 1 asks for: `ForwardRenderer` draws exactly one instance, which is
  why both samples push their worlds through the UI pass. Constants go through a
  uniform buffer on every tier, so unlike `ui.slang` there is no second source
  file to keep in step.
- **crcbl-render**: `SampleMode::Pixel` is sharp bilinear, not nearest. The
  linear blend is squeezed into a band one fragment wide at each texel boundary,
  so art pixels stay flat inside and cross over in one screen pixel at any
  scale, and the sprite's screen rect is snapped to whole device pixels.
  Nearest-neighbour was the placeholder: at a non-integer scale it makes some
  art pixels four screen pixels across and their neighbours five, and the
  unevenness crawls as the sprite moves. `SpriteInstance` grew a fourth `float4`
  carrying the sheet's size and the mode, so its layout changed.
- **crcbl-sprite**: `Playback`, which advances a clip over ticks — a bare `u64`
  cursor answering `frame_index` and `finished` as a closed form, so catching up
  after a stall lands exactly where tick-by-tick would. Ping-pong shows each end
  once (period `2n - 2` looping, `2n - 1` for a one-shot that has to walk home),
  and reverse carries each frame's hold with the frame rather than reversing the
  holds too. Also `Sheet::uv`, the frame rect as normalised UVs, which every
  caller was spelling out by hand.
- **crcbl-render**: `NineSliceSource::expand`, which turns stored insets into
  the quads that draw them — corners at their natural size, edges stretched on
  one axis, centre on both. Empty bands emit nothing, so a three-slice is three
  quads and a frame with no insets is one; the cut lines are computed once and
  indexed, so adjacent quads share their edges exactly and no seam opens up. A
  target below the corners' combined size shrinks them proportionally rather
  than letting them overlap and mirror.
- **crcbl-render**: `LayerStack`, `Layer` and `Parallax` — sprites grouped into
  back-to-front bands, each taking a chosen fraction of the camera's motion. A
  layer is a container rather than a field on `Sprite`, so nothing sorts and
  submission order inside a layer is still exactly what the caller gave.
- **crcbl-ui / crcbl-render**: skinned buttons. `Button::with_skin` takes the
  nine-slice insets its art was cut with, so its minimum size and its label's
  centring follow the frame rather than being guessed; `ButtonSkin` turns a
  state and a rectangle into the quads that draw it. Resizing moves the edges
  and leaves the corners alone, which is the whole point. The skin goes through
  the sprite pass rather than the UI pass — the UI atlas is a single-channel
  glyph mask, and `crcbl-render` already depends on `crcbl-ui`, so the reverse
  could never have happened.
- **crcbl-cli**: `crcbl crpix`, which turns PNG frames into one `.crpix` sheet
  in the order given, with `--nine`, `--sample`, `--clip` and `--hold`. Frames
  are named after their file stems; two inputs whose stems collide, or a stem
  the format cannot spell back, are refused rather than written out. An existing
  output is left alone without `--force`.
- **crcbl-ui**: `MenuSet<K>`, the container a game keeps its menus in. `Menu` is
  one panel; a game has several and needs to say which one a frame draws, to
  switch between them without carrying a half-finished click across, and to
  share one `UiState` so a press and its release are tested against the same
  capture. `K` is the game's own state type rather than one this crate dictates,
  and **a `K` the set holds no menu for draws nothing** — which is how "no menu
  this frame" is spelled, with no separate `Option`. `show`, `current`,
  `current_mut`, `is_showing`, `kind`, `select_next`, `select_previous`,
  `press`, `activate`, `point`, and `replace` for a panel whose buttons are
  built while the game runs. Both `show` and `replace` drop the pointer's
  capture; two entries claiming the same `K` are refused at construction,
  because the second would be unreachable.

### Changed

- **`crcbl-audio`**: the `Mixer` can now be driven by the game that owns it, and
  all four samples use it instead of a hand-rolled copy.

  `Mixer::play` took `&mut self` while `AudioStream::open` consumes its source,
  so once the stream was running nothing could reach the mixer to play through
  it — the shipped mixer was unreachable, and `apps/breakout`, `apps/flappy`,
  `apps/asteroids` and `apps/horde` had each written their own `Sound`, `Voice`,
  `VoiceQueue` and `MixerSource` around it. `play` now takes `&self` and answers
  with a `VoiceId`; `AudioSource` is implemented for `Arc<T>`, so
  `AudioStream::open(Arc::clone(&mixer))` leaves the game a handle to go on
  playing through. Existing callers keep compiling: no signature was narrowed,
  and `Mixer::play`'s new return value can be ignored.

  New alongside it: `Mixer::stop`, `Mixer::is_playing`, `Mixer::set_mix` and
  `Mixer::voice_mixes`; `VoiceId` and `VoiceMix`, with
  `VoiceMix::from(&SpatialCue)` as the "play this buffer once, panned" glue each
  sample was writing by hand (the cue's `itd_samples` is dropped — a `Voice` has
  no delay line); `Voice::with_mix`, `Voice::mix`, `Voice::is_looping` and
  `Voice::from_shared`; and `SoundBank::sound` / `SoundBank::insert_shared`.

  **`SoundBank::create_voice` no longer copies the sound.** `Voice` holds
  `Arc<[AudioSample]>`, so a voice is a playhead over the bank's buffer rather
  than a clone of it — at horde's cue rate that was an allocation the size of
  the sound per cue.

- **asteroids**: the engine is a real held sound, and an audio detail has left
  the simulation. `game::THRUST_CUE_PERIOD` and `GameLogic`'s `thrust_cue_timer`
  are **removed**: thrust used to be a one-shot re-fired on a countdown that
  lived in the deterministic tick, because the crate had no reachable looping
  voice. It is now one looping voice that `audio::Audio::set_thrust` starts on
  the first burning tick, re-aims at the ship every tick after (so the engine
  still pans across the field), and stops the tick the key comes up or the ship
  dies. What the simulation keeps is a plain `thrusting` bool, mirrored onto
  `Game::thrusting`.

  `THRUST_CUE_PERIOD` was re-exported from `apps/asteroids/src/lib.rs` and is
  gone from there too.

- **horde**: the game no longer starts itself. It opens on a `HORDE` start
  screen with a `PLAY` button — `Space`, which is the key breakout, flappy and
  asteroids print on theirs, and `R` still works — and the simulation does not
  advance until it is pressed: no spawns, no clock, no shots. The new
  `GameState::WaitingToStart` short-circuits the tick the way `LevelUp` already
  did, so a player looking at the title screen is looking at a still, empty
  arena rather than at a run that has been taking hit points off them since the
  window opened.

  **`TRY AGAIN` on the death screen now lands on that start screen**, not
  straight back into a run, which is what asteroids and flappy already do —
  restarting is two presses. `--prefill` starts its own run so the scale
  measurement still measures a running one. The sample deliberately shipped
  without a start screen; `docs/backlog.md` carries why that call was reversed.

- **flappy**: the game has art. A bird with a three-frame flap, a three-sliced
  pipe, and hills and a ground band on parallax layers, all authored as `.crpix`
  text under `apps/flappy/assets/` and baked to PNG + sidecar by a new
  `build.rs` — nothing baked is committed, so the text is the only source of
  truth and editing it rebuilds the game. The pipes were screen-space UI quads
  and the bird a lit cube through the forward pass; both are sprites in world
  coordinates now, drawn by `SpriteRenderer` between a `sky` clear and the HUD.
  Nothing about how the game _plays_ changed.
- **flappy**: `ForwardRenderer` is gone from the frame, and with it the HDR
  scene target, the depth buffer, the tonemap pass and the cube. The forward
  pass drew exactly one instance and the bird was it; a one-line `clear_color`
  pass replaces the clear it also happened to do.
- **breakout**: the board is art. Four bevelled brick frames — a brick's frame
  is read back out of its row, so a row's colour follows its position rather
  than being tracked beside it — a paddle, a ball, and a nine-sliced stone court
  whose wall faces land exactly on the colliders the ball bounces off. Authored
  as `.crpix` under `apps/breakout/assets/`, baked by a `build.rs` like
  flappy's. The forty bricks went through the UI draw list and the paddle was
  the one lit mesh; both are sprites now, and `ForwardRenderer` is gone from
  breakout too.
- **flappy**: the wing beats when the player flaps. The clip was a free-running
  loop that never looked at the bird, so the animation and the button had
  nothing to do with each other; a rising vertical velocity is exactly a flap,
  and it restarts the clip.
- **demo site**: the demo window is **one template**. The terminal frame, the
  canvas, the status bar, the focus note, the three keys the engine's loop keeps
  and the console note were the same markup written out per demo page; they are
  `web/templates/demo-*.html` now, pulled into a page with `<!--include …-->`.
  `build-pages.py` fails the build for a demo page that does not include them,
  so the next demo cannot go back to a copy.
- **demo site**: `web/engine/demo.js` is the boot sequence and the frame loop
  for every demo. `web/demos/breakout/main.js` and `web/demos/flappy/main.js`
  were 288 lines each and differed in the sample name, one status line and one
  comment — the shape that had already shipped breakout's control hint on
  flappy's page. Each is ~30 lines now: this sample's `__crcbl_<name>_*`
  symbols, written out literally so `check-exports.mjs` still sees every one,
  plus what to press and what it saves.
- **web tooling**: `check-exports.mjs` and `smoke.mjs` take `--sample <name>`,
  and `run-browser-e2e.sh` takes `CRCBL_WEB_E2E_DEMO`. Each was written when
  there was one demo and asserted against the whole workspace or against
  breakout's own strings, so the second demo broke all three. A sample's
  contract is now scoped to that sample, and the browser gate refuses a demo it
  has no expectations for rather than passing on a game that never started.

### Fixed

- **asteroids**: rocks kept shattering and scoring behind the game-over panel —
  leftover bullets swept unconditionally, and the score the best never saw could
  exceed the recorded one. The bullet sweep and `shatter`'s score line are now
  gated on the playing state. The tick also allocates nothing now: the sweep,
  wrap and view-refresh paths borrow or hoist their per-tick buffers.

- **breakout**: the Start menu popped up between lives when the first life was
  lost at score 0 with the grid still full — that state is indistinguishable
  from a fresh game by score and grid alone. `MenuKind::of` now also requires
  full lives, so "never started" and "one life down" are told apart.

- **horde**: a bolt still in flight when the player died kept killing enemies
  behind the death panel — the kill counter, kill sound and gem drops continued
  for up to `BOLT_LIFE`, contradicting the documented "the kill count is
  frozen". The bolt sweep is now gated on the playing state.

- **crcbl-ui**: a drag from one menu item onto a neighbour drew the neighbour
  `Pressed` — the drawn state came from a menu-global "something is down" flag
  plus whatever was hovered, not from `UiState`'s capture. The item the press
  belongs to is now tracked, so a drag-off leaves both items `Idle`.

- **crcbl-store**: `ReplayWriter::encode` wrote a `>4 GiB` entry's length as a
  truncated `u32` — a corrupt file with no error. It now refuses with the
  format's u32 length named, exactly as `save.rs` does.

- **crcbl-sprite**: crpix bake-time pixel math overflowed `u32` on a
  large-but-parseable file — a 32768×32768 frame's `width × height × 4` wrapped
  to zero, and the strip's `sheet_w × fh × 4` wrapped too, producing a truncated
  sheet or an OOB index panic. Frame and strip sizes are now checked in `u64` at
  parse time and refused with a named `TooLarge` error.

- **crcbl-wl-scanner**: an attribute value ending in `/` was mistaken for the
  self-closing marker — `<arg summary="foo/">` had its slash stripped from the
  value and the tag flagged empty. The trailing-slash test is now quote-aware.

- **crcbl** (`engine`): a menu key pressed before a menu opened stayed in
  `held_keys` forever when released while the menu was showing — the menu-key
  arms dropped the release before the held-key bookkeeping ran. The bookkeeping
  now runs for every key, matching its own documentation.

- **crcbl-phys**: a sweep shorter than the quadratic solver's EPSILON floor
  (below ~1.5e-8 m) was reported as a miss even when it started inside the
  target — `solve_quadratic` rejects `a <= EPSILON` outright, so the swept
  queries now treat anything at or below that floor as stationary and report the
  resting contact. Overlap queries against an inverted box (`Aabb::EMPTY`) also
  panicked on the clamp; they now answer "no overlap" instead.

- **crcbl-phys**: `RigidBody::new_dynamic(0.0)` was a silent NaN cascade in
  release builds — the only guard was a `debug_assert`, so `inverse_mass = +inf`
  poisoned every query in the world. The contract now panics in every build.

- **crcbl-audio**: the synth generators overflowed on hostile parameters —
  `(sample_rate × seconds) as usize` saturates to `usize::MAX` and
  `frames × CHANNELS` then wraps or aborts, and `looped_sine(0.0, …)` divided by
  zero. Frame counts are now computed in f64, capped at a minute, and a zero
  frequency returns an empty buffer.

- **crcbl-input**: `begin_tick` accepted a negative `dt`, moving the clock
  backwards so a held button reported a negative `Held` duration. Only forward
  time is accepted now.

- **crcbl-render**: `upload_texture` sized a compressed format's row as
  `width × block_size` — for BC formats the block covers 4×4 texels, so a BC1
  row is `ceil(width/4) × 8` bytes, and a compressed upload was silently wrong
  by a factor of four. Compressed formats are now refused by name before any
  device call; no caller uploads one.

- **crcbl-render**: `UiRenderer::begin_frame` committed the new element counts
  before the buffer uploads that make them true — a failed `write_buffer` left
  the new counts over stale indices and the next draw read out of bounds. The
  counts are now committed only after the uploads succeed.

- **crcbl-wgpu**: `DeviceDesc::compatible_surface` was never validated — a
  destroyed or foreign surface handle was accepted where the null backend
  returns `InvalidHandle`. `request()` now checks the handle against the
  instance's surface pool. `write_buffer` also accepted `HostReadback` buffers
  (mappable, but not a valid target); it now requires `HostUpload` exactly,
  matching the null backend.

- **crcbl-wgpu**: a padded indirect-draw `stride` was silently ignored — wgpu
  reads tightly packed argument structs while crcbl-vk honours a stride, so a
  padded one rendered garbage. All four indirect draw methods now refuse a
  non-tight stride loudly.

- **crcbl-shell** (Wayland): a `wl_data_offer` the compositor announced and
  never claimed leaked when refused — a drag `enter` for a vanished seat or
  naming an unannounced id sent `accept(null)` but never destroyed the proxy,
  and a second `enter` without `leave` overwrote the first drag without
  destroying its offer. Refused offers and the previous drag's offer are now
  destroyed.

- **crcbl-shell** (X11): an INCR chunk that could not be read was mistaken for
  the transfer terminator — a null reply or an over-cap property mapped to the
  empty slice that means "paste complete", so a hostile or broken owner's
  truncated paste was reported as a successful transfer. A type-less property
  (the ICCCM terminator) is now returned distinctly from a read failure, and a
  failed chunk read leaves the transfer to time out as `Unavailable` instead.

- **crcbl-shell** (X11): `refresh_server_time` burned its full 250 ms deadline
  when the probe's notify carried the same server millisecond as the previous
  event — at ≥1 kHz event rates the `last_server_time != before` wait could
  never be satisfied. The loop now waits for the probe's own notify to arrive,
  regardless of its stamp.

- **crcbl-shell** (Win32): a window hidden while borderless was re-shown by a
  second borderless request, and by the windowed restore. Both read the
  `WS_VISIBLE` bit from the style snapshot captured at the first borderless
  entry. They now use the live style, and the restore's `SetWindowPlacement`
  gets `SW_HIDE` for a hidden window instead of the saved `showCmd`.

- **crcbl-shell** (AppKit): a borderless window dragged to another display
  published nothing and kept naming the old monitor — `effective_mode.monitor`
  is written only by `apply_mode`, so a move that left size and scale unchanged
  also left the configuration unchanged. `refresh_configuration` now re-derives
  the borderless monitor from the screen the window is actually on.

- **crcbl-net**, **crcbl-server**: a delta in the last 25 bytes before the
  transport's 64 KiB cap encoded and sealed fine but was dropped by
  `send_unreliable` every tick — and the server had already retained it as the
  next delta's baseline, evicting the client's real one and leaving it desynced
  until the world shrank. The encode cap now leaves room for the seal by
  construction, and a snapshot is retained as a baseline only after the
  transport accepted it.

- **crcbl-cli**: `crcbl crpix` frame names that are clip keywords silently
  corrupted the written `.crpix` — a frame named `loop` wrote
  `clip flap: loop loop`, which parses as zero frames plus the loop flag, with
  exit 0. The shared name guard now refuses exact `loop`, `reverse`, `pingpong`
  and `@`, which the format reads as flags rather than frame names.

- **crcbl**, **sandbox**: `--tick-hz` values above `1_000_000_000` parsed
  cleanly and then panicked the engine — `1e9 / hz` truncates to a zero
  nanosecond period, which `FrameClock` asserts against after the GPU is already
  open (exit 101 instead of the documented exit 2). Both parsers now refuse
  rates past `MAX_TICK_RATE`, the same bound `sim` already carried.

- **crcbl-sprite**: `decode_png` sized its output buffer from the PNG's IHDR
  width and height alone — `output_buffer_size` trusts the file's claim and is
  capped only at `isize::MAX`, so a ~100-byte hostile PNG declaring 65536×65536
  forced a multi-gigabyte allocation (2²⁰×2²⁰ aborts the process). The declared
  pixel count is now bounded against `1 << 28` before any allocation, the same
  guard `crcbl-golden`'s `load_png` carries.

- **crcbl-audio**: native audio played 48 kHz-authored voices at the device rate
  — on a 44.1 kHz device everything ran ~9% slow, ~147 cents flat, the exact
  failure the browser path resamples to avoid. The mixer now steps each voice's
  playhead at the internal rate per output frame, so pitch and duration hold on
  any hardware (and are bit-identical when the rates match).

- **crcbl-audio**: the mono and multichannel output paths allocated a scratch
  `Vec` on the OS audio thread every block. The scratch is now owned by the
  stream's callback and reused — one allocation, then a resize and zero per
  block, with no malloc on the realtime path after the first block.

- **crcbl-wgpu**: an MSAA pass silently dropped its resolve target — every
  `wgpu::RenderPassColorAttachment` hardcoded `resolve_target: None`, so a 4x
  pass rendered into the MSAA image and nothing was ever resolved. The resolve
  views are now resolved from the pool and wired into the pass, and a stale
  resolve handle fails loudly instead of dropping the resolve unnoticed.

- **crcbl-wgpu**: push-constant range addition overflowed — `offset + size` in
  plain u32 arithmetic panicked in debug and wrapped to 0 in release. The end is
  now computed with `saturating_add` and ranges past the device's maximum are
  refused with `InvalidDescriptor`, matching the null backend.

- **crcbl-shell** (Wayland): announced-but-unclaimed `wl_data_offer`s grew
  without bound — a hostile compositor that announces offers and never claims
  them accumulated proxies, sink entries and per-offer mime strings for the
  whole session. A `Device` now holds at most 8 pending offers, evicting (and
  destroying) the oldest past the cap, and each offer's format list is capped at
  32 — the same bound every transfer already carried.

- **crcbl-shell** (X11): an `INCR` clipboard transfer **to one of our own
  windows** — a self-paste, or one of our windows pasting our own offer, of a
  payload over the server's request limit — replaced that window's event mask
  with `{PropertyChange}` and stripped every input event off it permanently.
  `ChangeWindowAttributes(EVENT_MASK, …)` is a replace, not an OR, and our own
  windows already select `PropertyChange` through `WINDOW_EVENT_MASK`; the call
  is now skipped for them, and kept only for foreign requestors, whose mask we
  cannot know.

- **crcbl-shell** (Win32): minimizing a captured window re-applied the pointer
  clip from the iconic window's 0×0 client area — both corners mapped to the
  same point, pinning the cursor for the whole minimized period. The `WM_SIZE`
  `SIZE_MINIMIZED` arm now releases the clip (keeping the recorded target, so
  restore re-clips from the real rectangle) instead of falling through to
  `reclip`.

- **crcbl-shell** (AppKit): a hidden window — created `visible: false` or hidden
  with `set_visible(false)` — popped on screen and took key focus when
  `set_mode(Borderless)` ran: the borderless arm ordered the window front with
  no visibility check, and `window_state().visible` reported true for a window
  nobody showed. `apply_mode` now guards `makeKeyAndOrderFront:` behind AppKit's
  `isVisible`, matching the `WS_VISIBLE` the Win32 sibling carries across its
  style change.

- **crcbl-vk**: the deletion queue freed a destroyed object one submission after
  it was parked — right for one future submission, a GPU-side use-after-free for
  two: an object recorded into two command buffers was freed when the first
  completed while the second was still queued or running. Command buffers now
  record the raw objects their commands use, and a submission extends the
  retirement of every referenced parked object to its own completion, so a
  destroyed object stays alive until the **last** submission referencing it
  finishes. The retire scan frees every entry whose own key is reached, so an
  extended key cannot hold up a ready successor.

- **crcbl-vk**: a readback whose explicit wait semaphore was destroyed between
  `request_readback` and `poll_readback` was undefined behaviour — the
  completion point was stored as the raw `VkSemaphore` and dereferenced at poll
  time with no liveness check. It is now stored as a generational handle and
  re-resolved through the device pool, exactly like the readback buffer, so a
  destroyed semaphore reports `InvalidHandle` instead.

- **crcbl-vk**: query commands with caller-supplied ranges no longer hand
  out-of-range values to the driver. `reset_query_set`, `write_timestamp` and
  `resolve_query_set` now bounds-check against the pool's query count at record
  time and fail with `InvalidDescriptor`, matching `Device::query_results` and
  the null backend — an over-large range used to be recorded and reached
  `vkCmdCopyQueryPoolResults`/`vkCmdResetQueryPool` as a validation violation.

- **crcbl-server**: a reconnect hello that arrived **after** the grace deadline
  expired the session without marking it terminated, so the next fresh join
  silently re-issued the dead session's token and id — and the departed client
  could still reconnect against the "new" session with its old credential. The
  expiry inside `handle_hello` now sets `session_terminated`, so the fresh join
  rotates to a new session and token.

- **crcbl-client**: a client holding a resume token a restarted server no longer
  recognised retried the stale token forever at capped backoff, wedged at
  "connecting" with no fresh join ever sent. Two consecutive
  `INVALID_SESSION_TOKEN` rejections now drop the token and session id and fall
  back to a fresh token-less join (two rather than one, so a single forged
  reject cannot throw away a still-valid credential).

- **asteroids**: a bullet could hit a rock sitting **behind** the ship on the
  tick it left the gun. Segment CCD reconstructs where a projectile was as
  `position - velocity * dt`, so one created this tick was swept from a point a
  whole step behind the muzzle — through the hull and out the other side. The
  gun fires after the sweep now, as `apps/horde` already did, so a bullet's
  first sweep is its first real step. 0.4 of a unit at 60 Hz and six units at
  `--tick-hz 4`, which is where the new test looks.

- **crcbl-vk**: reusing an image from the **offscreen ring** was ordered against
  nothing, so the frame that took the image back could write it while the
  previous frame was still reading it. A headless frame ends in
  `vkCmdCopyImageToBuffer` — a read — and the next frame opens with a layout
  transition out of `ResourceState::Undefined`, which is a write that discards
  the contents. `Undefined` maps to `srcStageMask = NONE`, which is right for a
  WSI image because the acquire semaphore already carries that dependency, and
  wrong for a ring image because there is no such semaphore: the seam hands one
  back with an implicit acquire. Nothing separated the two.

  The transition out of `Undefined` on a ring image now widens its source stage
  to `ALL_COMMANDS`, whose first synchronisation scope covers everything already
  submitted to the queue — the missing dependency, and nothing more: the access
  mask stays empty, because a write-after-read needs execution ordering and no
  cache flush, and the contents are still discarded. WSI images, ordinary
  images, and the seam's public shape are all unchanged, and no caller needs a
  change.

  Affects offscreen and headless Vulkan rendering that outlives the ring:
  `crcbl screenshot`, the `crcbl-vk` e2e suite, and `--headless --backend vk`.
  Windowed rendering is untouched. Validation reports the bug as
  `SYNC-HAZARD-WRITE-AFTER-READ` with `read_barriers: VkPipelineStageFlags2(0)`
  — that empty mask being precisely the `NONE` above; without a layer it is a
  race whose outcome the GPU's speed decides.

- **crcbl-render**, **crcbl-shaders**: the sprite pass drew **every batch after
  the first from the first batch's sprites** on Vulkan. A batch is a run of
  sprites sharing a sheet, and `SpriteRenderer::add_pass` pointed each draw at
  its slice of the frame's instance buffer with `firstInstance` — but `slangc`
  lowers `SV_InstanceID` to `InstanceIndex - BaseInstance` for SPIR-V, so the
  index restarted at zero for every batch and each one redrew the first batch's
  sprites with a later sheet bound. A four-sheet frame put one rectangle on
  screen and left the rest empty. **Both samples register four sheets**, so
  `breakout` and `flappy` were affected on every native run since the pass
  shipped; the browser was not, because `slangc` lowers the same source to
  WGSL's `@builtin(instance_index)`, which WebGPU defines to include
  `firstInstance`.

  No shader source is correct on both targets while `firstInstance` is non-zero,
  so it is now always zero: every draw is `draw(0..6, 0..count)` and the batch's
  offset arrives in the new `SpriteConstants::base` field, through a
  dynamic-offset binding of set 0. **`SpriteConstants` is one block per batch
  rather than one per frame**, laid out at `SpriteRenderer::constant_stride()` —
  `CONSTANTS_SIZE` rounded up to the device's
  `min_uniform_buffer_offset_alignment` — and its `pad: [f32; 2]` has become
  `base: u32, pad: u32`. Callers of the pass are unaffected; anyone building
  `SpriteConstants` by hand is not.

  `crates/crcbl-vk/tests/vk_e2e.rs` gains a golden of three solid-colour sheets
  at four rectangles, which is red against the old pass; the batching tests in
  `crcbl-render` now pin the draw ranges at zero and the dynamic offset per
  batch.

- **breakout**, **flappy**: a window that lost focus kept playing, and kept
  saying so. The samples ignored `ShellEvent::Focus` entirely — on every
  platform, native and browser — so alt-tabbing away left the simulation running
  with the HUD reading `Playing`, and a life was lost while nobody was looking.
  Focus loss now pauses the loop and releases every key the game thinks is held,
  which is the obligation `ShellEvent::Focus`'s own documentation states: no
  platform delivers releases for keys held when focus leaves. Flappy had the
  worse half of it — its flap is an edge, and an action map that never saw Space
  come up raises no further `just_pressed`, so the bird could never flap again.
  Regaining focus deliberately does not resume.

- **crcbl-wgpu**: a shader module or pipeline that fails to build is reported.
  WebGPU hands back an object either way and delivers the reason to the device's
  error channel, so failures were invisible: the backend built a pipeline on a
  module that had not compiled and every submission after it was silently
  discarded, which presents as a black canvas over a game that reports itself as
  playing. Creation calls now return `HalError::Backend`, and the asynchronous
  half — the browser's, which no call can be blamed for — stops the frame loop
  from `GpuContext::acquire` with the driver's own message.
- **breakout**: the ball is no longer under gravity. It launches at a constant
  speed and collisions change only its direction, which is what makes a shot
  aimable.
- **breakout**: the paddle steers, by being moved. A paddle standing still
  mirrors the ball like a wall; a paddle being driven left or right decides
  which way the ball goes next, and turns a ball back the way it came rather
  than rebounding it onward.
- **breakout**: the whole play field is on screen at every aspect ratio. The
  orthographic camera derived its width from a fixed half height, so a 4:3
  surface — the size the window opens at, and the aspect the web demo's canvas
  is styled with — cropped two world units from each side and the ball
  disappeared off the edge before bouncing back.
- **crcbl-phys**: `PhysicsWorld::sweep_sphere` reports contacts it used to miss.
  The broadphase traversed the sphere's centre line, so anything the sphere
  overlapped by less than its radius was dropped before the exact test, and a
  contact landed only once the centre reached the surface.
- **crcbl-store**: `canonical_key` and the browser backends split keys on `/` on
  every platform. Parsing went through `std::path::Path`, whose separators are
  the host's, so `a\b` was refused on Linux and quietly rewritten to `a/b` on
  Windows.

[Unreleased]: https://github.com/kryptic-sh/crcbl/commits/main
