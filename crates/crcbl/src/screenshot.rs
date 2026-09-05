//! Offscreen render → readback → golden image — the `crcbl screenshot` path.
//!
//! Opens a GPU backend, creates an offscreen surface and swapchain, renders
//! one frame of the [`Scene`] the caller named, and reads the pixels back.
//!
//! # Why there is more than one scene
//!
//! `docs/plan/02-vulkan-backend.md`'s shader-portability rule 5: a shader can
//! compile cleanly to SPIR-V, WGSL, MSL and DXIL and *mean something different*
//! on each, and no lint can see it — `SV_InstanceID` lowers to
//! `InstanceIndex - BaseInstance` on SPIR-V and to a bare
//! `@builtin(instance_index)` on WGSL, which is why every sprite batch after the
//! first drew the first batch's instances while every gate stayed green. The
//! only detector is rendering the same scene through two backends and comparing
//! pixels, which is what `web/run-cross-backend-e2e.sh` does — and it can only
//! catch what it draws. [`Scene::Cube`] exercises `mesh.slang` and
//! `tonemap.slang`; [`Scene::Sprite`] and [`Scene::Ui`] are here so
//! `sprite.slang` and `ui.slang` — the two with an actual history of divergence
//! — are covered too.
//!
//! `SV_VertexID` turned out to be the same story, and it is why [`Scene::Cube`]
//! draws the mesh pool's *second* resident beside the cube: a base vertex only
//! means something for a mesh that is not at base 0, and this is the only frame
//! in the tree that renders one through both backends.
//!
//! What comes back is the swapchain image's bytes *as they are in memory*, in
//! [`OffscreenSetup::format`]'s channel order — which on an ordinary desktop
//! surface is BGRA, not RGBA. Turning them into a `crcbl_golden::Image` is
//! the caller's job and needs `Image::from_readback` with the matching
//! `ChannelOrder`, not `Image::from_rgba8`; this module deliberately does not
//! guess, because the format is the thing it knows and the image type is not
//! its dependency to reach for.
//!
//! This module is the render half of the CLI subcommand; the CLI module
//! owns the argument parsing and I/O.
//!
//! # Poll-based core, native convenience
//!
//! The path used to block in three places — `crate::backend::open`, a blocking
//! device creation, and a `std::thread::sleep` poll loop waiting for the readback
//! copy to land — and a browser's main thread may not block. It is now a pair of
//! poll-driven state machines instead: [`OffscreenSetup::request`] →
//! [`PendingOffscreen::poll`] opens the instance, device and swapchain a frame at
//! a time on [`crate::backend::request_open`] and
//! [`Instance::request_device`], and [`OffscreenSetup::begin_readback`] →
//! [`PendingReadback::poll`] drives the copy on [`Device::poll_readback`]. Both
//! are free of `#[cfg]` and build on `wasm32`, so a future browser harness can
//! drive them across `requestAnimationFrame` frames.
//!
//! The blocking entry points — `OffscreenSetup::open`,
//! `OffscreenSetup::open_with`, `OffscreenSetup::open_forward` and
//! `OffscreenSetup::draw_and_readback` — remain as a **native-only**
//! convenience: each drives its poll core to completion with
//! [`std::thread::yield_now`] (never a sleep) and is
//! `#[cfg(not(target_arch = "wasm32"))]`, because a busy loop on the browser's
//! one thread would hang it. `crcbl screenshot` and every headless test use
//! them; browser code drives the poll core directly. They are spelled without a
//! doc link on purpose: they are absent from a `wasm32` build, where this module
//! and its docs still compile.
//!
//! # Which adapter drew it
//!
//! This module used to pick `adapters().first()` and never report which one
//! that was. On `windows-latest` that adapter is not a usable device, and the
//! frame died on its first buffer with `DXGI_ERROR_DEVICE_REMOVED` before
//! anything was drawn — see [`crate::adapter`], which is now what chooses.
//! [`crate::adapter::ADAPTER_ENV_VAR`] names a device class, a miss is a hard
//! failure rather than a fallback, and [`OffscreenSetup::adapter`] reports what
//! answered so a harness can check the pin landed.
//!
//! **The remaining half is the CLI's.** `crcbl screenshot` installs no logger,
//! so the backends' own adapter lines still go nowhere and the subcommand prints
//! nothing about the device; closing that means a `--json` field naming the
//! adapter, which is `crcbl-cli`'s call to make. `tests/run-render-e2e.sh` reads
//! [`OffscreenSetup::adapter`] out of its suite instead, the way `run-vk-e2e.sh`
//! reads the adapter line out of its own.

use std::time::Duration;

use crate::hal::{
    Barriers, BufferDesc, BufferHandle, BufferImageCopy, BufferUsage, CommandBufferHandle,
    CommandEncoderDesc, Device, DeviceDesc, DeviceRequestState, Extent3d, Features, Format,
    ImageAspect, ImageBarrier, ImageSubresourceLayers, ImageSubresourceRange, Instance,
    MemoryLocation, Offset3d, PendingDevice, PresentInfo, PresentMode, QueueHandle, QueueKind,
    ReadbackDesc, ReadbackHandle, ReadbackState, ResourceState, SubmitInfo, SurfaceError,
    SurfaceHandle, SurfaceTarget, SwapchainDesc, SwapchainHandle,
};
use crate::render::scene::{
    DEMO_CUBE, DEMO_DUNES, DEMO_OPEN_BOX, DEMO_PYRAMID, DEMO_TEXTURED, DEMO_TINTED, DEMO_UNTINTED,
};
use crate::render::{
    Camera, DirectionalLight, EffectOverride, EffectRequest, FontAtlas, ForwardRenderer,
    FrameCounters, GraphError, InstanceDesc, ProbeUpdate, Projection, RenderEffects, RenderGraph,
    SampleMode, SheetDesc, SheetId, Sprite, SpriteRenderer, TransientPool, UiRenderer,
};
use crate::ui::draw_list::DrawList;

// ---------------------------------------------------------------------------
// Scenes
// ---------------------------------------------------------------------------

/// What a screenshot draws.
///
/// One variant per engine shader pair that has pixels of its own, because the
/// cross-backend comparison this feeds can only catch divergence in a shader it
/// actually ran — see the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scene {
    /// The sandbox's lit cube through [`ForwardRenderer`]: `mesh.slang` into an
    /// HDR target and `tonemap.slang` back out of it.
    ///
    /// The default, because it is the frame every caller of this module drew
    /// before there was anything to choose between.
    ///
    /// The pyramid beside it is the mesh pool's second resident, and it is here
    /// for the reason the module docs give: it is the only geometry in the tree
    /// at a non-zero base vertex, so it is the only thing this comparison can
    /// use to tell the four targets' `SV_VertexID` lowerings apart.
    ///
    /// **There are three of it**, and they are the same mesh at the same
    /// orientation differing in nothing but their material id — which is what
    /// makes this frame the observable for
    /// `docs/plan/03-gpu-driven-rendering.md` §3.2's material table. The three
    /// are one row and two single-column edits of it: see `TINTED_PYRAMID_AT`,
    /// whose row differs in its base-colour *factor*, and
    /// `TEXTURED_PYRAMID_AT`, whose row differs in its base-colour *texture*.
    /// One pair per column, so neither column's evidence is the other's.
    #[default]
    Cube,
    /// `docs/plan/25-lod.md`'s dunes patch through [`ForwardRenderer`], seen
    /// from its own near edge.
    ///
    /// **The scene that proves a device with no amplification stage can draw a
    /// cluster DAG at all.** [`Scene::Cube`]'s three residents are flat meshes,
    /// so a renderer whose level selection picked nothing — or picked a level it
    /// then failed to draw — produces that frame unchanged. This one is the only
    /// geometry in the tree with a hierarchy, so on the two indirect tails every
    /// triangle of it arrives through a level `draw_gen.slang` chose.
    ///
    /// The camera is `DUNES_CAMERA_AT`: at the patch's near edge, low, looking
    /// along it as it recedes — which is where the near end and the far end are
    /// tens of times apart in distance and the metric has something to say.
    Dunes,
    /// Four sprites over three [`SpriteRenderer`] batches: `sprite.slang`.
    Sprite,
    /// `docs/plan/18-render-features.md`'s light list: the cube scene's geometry
    /// under three coloured **point** lights and a sun turned down far enough
    /// that they are what is lighting it.
    ///
    /// The scene the clustered-forward slice exists for, and it is deliberately
    /// the same geometry as [`Scene::Cube`]: the two goldens differ in the light
    /// list and in nothing else, so a diff between them is the feature rather
    /// than a new arrangement of objects.
    Lights,
    /// `docs/plan/18-render-features.md`'s **spot** light: one cone of light on
    /// a flat floor, seen from straight above.
    ///
    /// **The only frame in the tree that draws a cone.** [`Scene::Lights`] is
    /// three point lights, so `mesh.slang`'s `spot_cone` compiled to all four
    /// targets and lit no pixel anywhere — and a cone test with its sign
    /// inverted, its two angles the wrong way round, or a step where its ramp
    /// should be draws a plausible picture in each case and passes every other
    /// golden here unchanged.
    ///
    /// The geometry is the cube alone, scaled into a floor, because what this
    /// scene needs is a surface for the cone to land on and nothing else in the
    /// frame to read the shape off instead. See this module's `spot_camera` for
    /// why the camera looks straight down and `SPOT_HEIGHT` for what the light's
    /// height buys the pixel assertions.
    Spot,
    /// `docs/plan/18-render-features.md`'s **shadowed spot**: [`Scene::Spot`]'s
    /// floor and cone with an object standing in the light.
    ///
    /// **The only frame in the tree where a light other than the sun occludes.**
    /// A shadow map that is never written, never sampled, or sampled through a
    /// matrix that disagrees with the one it was rendered with all produce a
    /// plainly plausible picture here — an evenly lit pool — so the assertions
    /// in `tests/render_e2e.rs` are about *where* the dark region is, and
    /// `crcbl-vk`'s spot suite is what moves the caster and watches it follow.
    ///
    /// It is a scene of its own rather than an occluder added to [`Scene::Spot`]
    /// for two reasons, and the first is the stronger: that scene's assertions
    /// are a radial profile out from the frame's centre through the cone's
    /// penumbra, and an object in the middle of it destroys exactly the
    /// measurement it exists to make. The second is that its golden would have
    /// had to be re-blessed for a feature it is not evidence about.
    ///
    /// The camera looks straight down and the light comes in at 45°, which is
    /// the arrangement that separates the shadow from the caster: with both
    /// overhead the shadow hides under the object that casts it. See
    /// `spot_shadow_camera` and `SPOT_SHADOW_LIGHT_AT`.
    SpotShadow,
    /// `docs/plan/18-render-features.md`'s **shadowed point light**: one light
    /// low over a floor with a caster on either side of it, seen from above.
    ///
    /// **The only frame in the tree where one light occludes in more than one
    /// direction**, and that is the whole of what it is for. A point light's map
    /// is six atlas tiles and the fragment stage picks one of them from the
    /// direction to the light, so five of the six can be wrong — sampled from the
    /// neighbouring face, or from a fixed one — and the frame still has a shadow
    /// in it. The two casters are on *different* faces, so a frame that got the
    /// face selection wrong loses one of the two shadows while keeping the other,
    /// which `tests/render_e2e.rs` asserts side by side rather than one at a
    /// time.
    ///
    /// See this module's `POINT_LIGHT_UP` for why the light is low — a light
    /// high over the floor puts every receiver on the `-Y` face and the scene
    /// stops distinguishing anything — and `POINT_CASTER_AT` for where the
    /// casters stand.
    PointShadow,
    /// `docs/plan/44-lighting.md`'s **rectangular area light**: a dark glossy
    /// floor under two strip lights, looked straight down at.
    ///
    /// **The only frame in the tree that draws a polygon's highlight, and the
    /// first that drew a fill light at all** — [`Scene::FillLight`] is the same
    /// experiment on the two punctual kinds. `mesh.slang`'s linearly
    /// transformed cosine path compiled to all four targets and lit no pixel in
    /// any golden here, and neither did the flag that takes a light's specular
    /// lobe away — a rectangle read as a point at its own centre, one whose `v`
    /// axis came out negated, or a fill flag wired to nothing draws a plausible
    /// picture in each case and leaves every other golden in this file
    /// untouched.
    ///
    /// It is `tests/mesh_e2e/area_light.rs`'s lighting set-up rather than a new
    /// one: the same strip, at the same half-extents and the same hue, over the
    /// same tighter lobe. What differs is that there are **two of them**,
    /// mirrored across the frame's axis and differing in exactly one field —
    /// one is a fill light and the other is not — so one frame carries both of
    /// that file's claims instead of a pair of frames it can only compare
    /// off-line. The two figures this scene does move are the ones its own
    /// framing forced, and each says so where it stands: `AREA_ALBEDO` and
    /// `AREA_RADIANCE`.
    ///
    /// # Why the mirror is exact
    ///
    /// The camera looks straight down the axis the strips are mirrored about,
    /// the floor is one flat plane, and the sun points **straight down** —
    /// [`Scene::Ao`]'s third reason, buying more here than a missing shadow.
    /// Two points mirrored across that axis are the same distance from the eye,
    /// carry the same normal, and take the same directional diffuse, the same
    /// ambient, the same occlusion, and the same Lambert and falloff from each
    /// of the two strips. The one term left that can separate them is the
    /// specular lobe the fill flag removes, which is what
    /// `tests/render_e2e.rs` measures — at the highlight's centre, along the
    /// rectangle's length, and across its width, because a point light wearing
    /// a rectangle's row passes the first two and fails the third.
    ///
    /// The floor is the cube through a **dark, glossy** row of this scene's own
    /// — see `AREA_ALBEDO`, which is where the darkness is argued and measured.
    /// Its roughness is `crcbl_render::scene::PYRAMID_ROUGHNESS`, the tighter
    /// lobe `tests/mesh_e2e/area_light.rs`'s `SLAB_MATERIAL` picked that row
    /// for: an area light's claim is about the *edge* of its highlight, and the
    /// neutral row's broad lobe smears a strip and a point into the same soft
    /// blob. See this module's `area_camera` for the framing, `AREA_STRIP_AT`
    /// for where the strips hang, and `area_sun` for the sun that makes the
    /// mirror a control.
    AreaLight,
    /// `docs/plan/44-lighting.md`'s **fill flag on the two punctual kinds**: the
    /// same dark glossy floor under four lights — a point pair and a spot pair,
    /// each mirrored across the frame's axis and each differing in `fill` alone.
    ///
    /// **The only frame in the tree that draws a fill point light or a fill
    /// spot.** [`Scene::AreaLight`] is the flag's other picture and it draws the
    /// flag on a rectangle; `crcbl_shaders::light::FLAG_FILL` is kind-agnostic
    /// in `mesh.slang`, so a `Light::row` that set the bit for one kind and not
    /// the others — or a shading path that dropped the lobe on the rectangle's
    /// arm alone — draws that scene exactly right and this one wrong.
    ///
    /// It is that scene's floor, camera and sun rather than a second set of
    /// each: `area_scene` builds the floor, `AREA_ALBEDO` is where its darkness
    /// is argued and measured, `area_camera` is the framing and `area_sun` is
    /// the sun. What differs is the light list — see `fill_light_pairs`.
    ///
    /// # Why the mirror is exact
    ///
    /// Reflecting the scene about the plane `x = 0` maps this light list onto
    /// itself with `fill` swapped: every lit light lands on its own fill twin,
    /// at the same height, the same reach, the same colour and the same cone.
    /// So two mirrored floor points take the same total diffuse — the same sum
    /// in a different order — and, the camera being on that plane and the sun
    /// pointing straight down, the same ambient, the same occlusion and the same
    /// directional term. What is left is the specular lobe, which a fill light
    /// contributes nothing to, and `tests/render_e2e.rs` measures it at each lit
    /// light's highlight against that highlight's mirror.
    ///
    /// The one term the mirror does **not** carry by construction is the
    /// shadow: `crcbl_render::shadow::Selection` refuses a fill light a tile and
    /// hands its twin one. Nothing in this frame casts — the floor is the only
    /// geometry — so both halves are fully lit, and that is a claim rather than
    /// an assumption: `tests/render_e2e.rs` measures a mirrored pair of bands
    /// out where neither highlight reaches and holds them to a tolerance its
    /// own sweep set.
    FillLight,
    /// `docs/plan/43-render-standards.md` §3's **alpha-mask cutout**: a masked
    /// plate floating over a small flat floor, under a sun tilted far enough
    /// that the plate's shadow lands clear of the plate's own footprint.
    ///
    /// **The only frame in the tree that discards a fragment.** Every other
    /// scene here shades an opaque surface, so every row's
    /// `crcbl_render::MaterialTable::mode` is `OPAQUE`, `mesh.slang`'s
    /// `alpha_masked` compiled to four targets and cut nothing anywhere, and the
    /// second depth-only pipeline `ForwardRenderer` builds is in no frame's
    /// masked partition and is recorded by no frame at all. A cutout is honoured in three passes and each of them can be wired
    /// to nothing on its own, so this scene separates the three into three
    /// regions of one picture rather than needing three frames:
    ///
    /// * **the colour pass** — the plate's cut half is not drawn, and the floor
    ///   is seen through it;
    /// * **the depth prepass** — the floor seen through the hole is *shaded*.
    ///   The colour pass tests `GreaterOrEqual` with depth writes off against
    ///   the prepass's depth, so a prepass that wrote the plate's whole
    ///   silhouette rejects the floor fragment behind the hole and the hole
    ///   reads as `crcbl_render::forward::SCENE_CLEAR` rather than as floor;
    /// * **the shadow atlas** — the plate's shadow on the floor carries the same
    ///   hole, because the cascade is drawn with that same pipeline.
    ///
    /// # The sun's tilt, and the arithmetic that set it
    ///
    /// The default sun is `(0.4, 0.8, 0.6)` normalised, which offsets a shadow
    /// by `h * (0.4, 0.6) / 0.8` for a caster at height `h` — `(0.375, 0.5625)`
    /// here, and the plate is `0.7` by `0.25`, so most of its shadow would land
    /// under the plate itself where nothing can read it. `alpha_sun` tilts to
    /// `(0, 1, 1)` instead: no `x` component at all, so the shadow moves purely
    /// along `-z` by exactly the caster's height and the mask's cut — which runs
    /// along `z` — is at the same `x` in the plate and in its shadow. At
    /// `ALPHA_PLATE_UP` the slab's shadow covers `z` in `-1.01 ..= -0.49` where
    /// the plate's own footprint is `-0.25 ..= 0.25`: near a quarter of a unit
    /// of lit floor between the two, which is the gap the bands in
    /// `tests/render_e2e.rs` are placed in.
    ///
    /// # What is in the frame, and why each piece is the size it is
    ///
    /// The floor is the cube on `spot_floor`'s plan — scaled and dropped so its
    /// `+Y` face is the plane `y = 0` — but at `ALPHA_FLOOR_SCALE` rather than
    /// that helper's, and the smaller floor is load-bearing: this scene's second
    /// claim needs a region the frame **never drew geometry into**, and a floor
    /// that runs past every edge leaves none. See `alpha_floor`.
    ///
    /// The plate is the same cube scaled flat and wide — see `ALPHA_PLATE_SCALE`
    /// — through a row carrying `crcbl_shaders::mesh::GpuMaterial::ALPHA_MODE_MASK`
    /// and `ALPHA_MASK_LAYER`, whose texels are half transparent and half
    /// opaque at the same RGB. **The same RGB is the point**: a golden in which
    /// the hole were a colour difference rather than an absence would pass every
    /// assertion below while the discard did nothing.
    ///
    /// The camera looks straight down, on `spot_camera`'s terms, so the floor is
    /// a plane at a known scale and a pixel maps to a floor position by a
    /// division.
    AlphaMask,
    /// `docs/plan/43-render-standards.md` §2's rung 4, second half: **glTF's
    /// `doubleSided`** — back-face culling off, and the back face lit through a
    /// reversed normal.
    ///
    /// Three flat quads hanging over a floor, seen from straight above and lit
    /// by a sun at 45°. Every one of them is the same single mesh, and that mesh
    /// **faces down**: its authored normal is `-Y` and its winding puts its
    /// front face on the `-Y` side, so the camera and the sun are both behind
    /// it. What differs between the three is the material row and one transform.
    ///
    /// * **The culled one**, on the axis, through a single-sided row. The
    ///   rasteriser drops it in every pass, so the frame shows the floor where
    ///   it is and the floor under it carries no shadow.
    /// * **The double-sided one**, out along `-x`, through a row carrying
    ///   `crcbl_shaders::mesh::GpuMaterial::DOUBLE_SIDED`. It is drawn — that is
    ///   the cull mode — and it is **lit**, which is the reversed normal: a back
    ///   face shaded through its authored `-Y` normal has the sun behind it and
    ///   reads as the ambient term alone.
    /// * **The mirror**, out along `+x` at the same distance, through the
    ///   *single-sided* row and turned over by `double_sided_mirror` so its
    ///   front face is up. Nothing double-sided touches it: it is what a front
    ///   face at that orientation reads, and the double-sided quad has to match
    ///   it.
    ///
    /// # Why the mirror is the reference and not a number
    ///
    /// "Lit rather than ambient" is a threshold somebody has to choose. The
    /// mirror is the same mesh, the same material factors, the same height and
    /// the same distance from the frame's axis on the other side — and the sun
    /// has no `x` component, so under `x → -x` the Lambert term, the
    /// half-vector's `y` and therefore the whole lobe are unchanged. So the two
    /// quads must read the *same brightness*, and a build that drew the back
    /// face without reversing its normal reads the ambient term against a lit
    /// surface.
    ///
    /// # The shadow half, and why it needs an open quad
    ///
    /// A cull mode changes nothing about a closed solid's shadow: whichever face
    /// the light sees, some face of the slab is front-facing to it. An open quad
    /// is where it shows, and this one's front face is away from the sun — so
    /// the single-sided quads' shadows are the claim. The culled one casts
    /// **none**, the double-sided one casts one, and the mirror — whose front
    /// face is towards the sun — casts one as well, which is what says the
    /// shadow pass draws at all and that the difference is the side.
    ///
    /// # What is in the frame, and why each piece is the size it is
    ///
    /// The floor is [`Scene::AlphaMask`]'s exactly — the cube at
    /// `DOUBLE_FLOOR_SCALE`, dropped so its `+Y` face is the plane `y = 0` —
    /// and the camera and the sun are that scene's too, for its reasons: an
    /// overhead camera makes a pixel a floor position by a division, and a sun
    /// with no `x` component moves a shadow purely along `-z` so each quad's
    /// shadow is directly down-frame of it.
    ///
    /// Every quad's edge lands on an integer pixel column and row of the golden
    /// frame — see `DOUBLE_PLATE_HALF_X`, which is
    /// [`Scene::SpecularAa`]'s `SPECULAR_STRIP_PITCH` argument at this
    /// fixture's own scale.
    DoubleSided,
    /// `docs/plan/44-lighting.md`'s rung 4: **specular antialiasing by
    /// roughness regularisation**, and the one frame in the tree whose normal
    /// field moves faster than the pixel grid samples it.
    ///
    /// A single flat plate seen through a long lens from straight above, with a
    /// conductor's material on it and the sun placed so its highlight lands on
    /// the plate. The plate is geometrically one plane; what varies is the
    /// *authored* vertex normal, and it varies in two ways across the two
    /// halves of the same mesh:
    ///
    /// * the **corrugated band**, `+z` of the centre line, whose normal swings
    ///   by `SPECULAR_SWING` every strip — and a strip is two pixels wide, so
    ///   the mirror direction crosses the sun twice inside distances the frame
    ///   cannot resolve. That is specular aliasing, and it is the one
    ///   aliasing `docs/plan/49-antialiasing.md`'s whole ladder is silent about:
    ///   no coverage-based pass can average a signal the shading generated.
    /// * the **control band**, `-z` of it, one quad whose normal is constant.
    ///   `mesh.slang`'s `specular_aa_kernel` returns exactly zero there — one
    ///   normal at every corner interpolates to that same normal, so both
    ///   derivatives are exactly zero — and the band is what says the
    ///   regularisation costs an unregularised surface nothing at all.
    ///
    /// # Why a mesh and not a page
    ///
    /// `crcbl_render::scene::PAGE_EXTENT` is two texels a side and the page is
    /// read through a trilinear sampler, so tiling it tightly enough to alias
    /// is tiling it tightly enough that the mip chain hands the fragment a flat
    /// surface — correct filtering, and nothing left for a kernel to do. A
    /// vertex normal reaches the fragment through the rasteriser's interpolator
    /// instead, which has no such filter, so a mesh is the only fixture in this
    /// engine that can put the signal in front of the kernel at all.
    /// `specular_plate_mesh` is the geometry.
    ///
    /// # Why a conductor
    ///
    /// `metallic` of one removes the diffuse lobe *and* the ambient term, which
    /// multiplies the diffuse albedo — so every lit pixel of this plate is the
    /// specular lobe and nothing else. A band's mean is then the lobe's energy
    /// and its maximum over that mean is how badly the lobe was undersampled,
    /// which is what `tests/render_e2e.rs` reads.
    ///
    /// # What is deliberately switched off
    ///
    /// `RenderEffects::REFLECTIONS`, because
    /// [`crcbl_shaders::ssr::ROUGHNESS_CUTOFF`] is a half and this plate is
    /// smoother than that: a screen-space march would write its own answer into
    /// the very pixels both bands are read out of.
    ///
    /// [`crcbl_shaders::ssr::ROUGHNESS_CUTOFF`]: crate::shaders::ssr::ROUGHNESS_CUTOFF
    SpecularAa,
    /// `docs/plan/18-render-features.md`'s **screen-space ambient occlusion**:
    /// the inside of a box, looked straight down into, lit almost entirely by
    /// ambient.
    ///
    /// **The only frame in the tree whose subject is a term that darkens nothing
    /// else.** AO multiplies `frame.ambient.rgb` alone, so a scene with a strong
    /// key light shows it as a rounding error: this one turns the sun down to a
    /// trace and leaves the ambient at full, which makes the floor's brightness
    /// very nearly the occlusion value itself.
    ///
    /// It is a scene of its own rather than an occluder added to
    /// [`Scene::Spot`], for [`Scene::SpotShadow`]'s two reasons and a third that
    /// is stronger than either: **the sun points straight down**. A vertical
    /// light casts no shadow from a vertical wall, so the two floor bands
    /// `tests/render_e2e.rs` compares receive identical direct light, identical
    /// ambient and identical Lambert — and the *only* thing left that can
    /// separate them is the occlusion term. Under any other light the
    /// measurement would be a shadow-map result wearing AO's name.
    ///
    /// The geometry is the open box alone — see
    /// `crcbl_shaders::mesh::OPEN_BOX_FACES`, whose five faces point inward —
    /// scaled by `ao_box` into a long narrow **trough**, so that its two long
    /// walls are in frame and its two ends are not. See `ao_camera` for why the
    /// view is straight down and `AO_RUN` for why the trough is not a square
    /// room.
    Ao,
    /// `docs/plan/18-render-features.md`'s **screen-space reflections**: a
    /// smooth floor with the plain pyramid standing on it, seen from just above
    /// the floor.
    ///
    /// **The case the technique is good at, and the one its determinism section
    /// asks a fixture for**: the reflected content is one large, flat, brightly
    /// lit object, so a driver picking the neighbouring tap at the crossing
    /// fetches very nearly the same colour. A fixture reflecting fine detail
    /// would make the golden a function of which pixel each rasteriser landed
    /// on.
    ///
    /// The floor is the cube through the **tinted** material row, which is the
    /// only row in the demo scene under the pass's roughness cutoff — see
    /// `crcbl_render::scene::PYRAMID_ROUGHNESS`. Everything else in this frame,
    /// the pyramid included, is the untinted row at exactly the cutoff and
    /// therefore weighs zero, so the reflection is the floor's and nothing
    /// else's.
    ///
    /// It is a scene of its own rather than a camera change to [`Scene::Spot`]
    /// for that reason alone: this needs a *smooth* floor, and that scene's is
    /// the neutral row whose whole job is to be what an instance shades through
    /// by omission.
    ///
    /// See `SSR_CAMERA_UP` for why the eye is low — a Fresnel term head-on is a
    /// twenty-fifth — and `ssr_sun` for why the sun has no X component, which is
    /// what makes the two bands beside the reflection its controls.
    Ssr,
    /// `docs/plan/43-render-standards.md` §8's sky **in a mirror**: a metallic
    /// floor to the horizon under an atmosphere with a low sun, looked at
    /// level.
    ///
    /// **The frame is one claim and the frame's own symmetry is what makes
    /// it.** The camera looks along `-Z` with no tilt, so the horizon is the
    /// middle row and two floor pixels at mirrored columns of one row reflect
    /// two directions with the *same* elevation and opposite azimuths. Every
    /// term the reflection is multiplied by is therefore equal at the two — the
    /// Fresnel pair, the split-sum's `DFG`, the roughness ramp — and so is
    /// [`crcbl_shaders::sky_prefilter`]'s convolution of the three bands, which
    /// is a function of the direction's `y` alone.
    ///
    /// So the gradient the reflection pass used to reflect an atmosphere with
    /// predicts those two bands as **exactly equal**, and the sky-view LUT does
    /// not: `atmosphere_mirror_sun` sits about 45° to the right of the camera's
    /// forward, so the right-hand band reflects the bright limb beside the sun
    /// and the left-hand one the sky nearly ninety degrees away from it.
    /// `tests/render_e2e.rs`'s `an_atmosphere_mirror_reflects_the_luts_limb` is
    /// what reads the ratio, and the fixture would have nothing to say about
    /// `ssr.slang`'s `sky_environment` without the mirroring.
    ///
    /// # Every measured pixel is the reflection and nothing else
    ///
    /// * The floor is **fully metallic**, so `mesh.slang`'s diffuse albedo is
    ///   exactly zero and neither the flat ambient, the probe grid nor the
    ///   sky's own L1 term can reach it.
    /// * Its light has zero colour and zero ambient — see
    ///   `atmosphere_mirror_sun` — so the specular lobe is zero too, and the
    ///   forward pass leaves the floor black.
    /// * The demo scene authors no probes, so `ssr.slang`'s probe fallback is
    ///   zero and the environment it adds is the sky alone.
    ///
    /// What is left in a floor pixel is `sky_environment × (f0·scale + bias)`,
    /// which is what makes an absolute comparison against the host's own
    /// `SkyView` possible rather than only a ratio.
    ///
    /// Nothing else stands on the floor: an object would be something a ray
    /// could hit, and what these bands are evidence about is the miss.
    AtmosphereMirror,
    /// [`Scene::AtmosphereMirror`]'s plate under a **gradient sky and no
    /// atmosphere**: the same white conductor to the horizon, the same level
    /// camera, the same light, and `crcbl_render::ForwardRenderer::set_sky`'s
    /// three authored bands where that scene has a marched LUT.
    ///
    /// **The frame `ssr.slang`'s early return had never been drawn in.**
    /// `sky_environment` returns `sky_prefiltered`'s bands before it reads the
    /// sky-view LUT whenever `camera.atmosphere.w` is
    /// [`crcbl_shaders::sky::ATMOSPHERE_OFF`](crate::shaders::sky::ATMOSPHERE_OFF),
    /// and that return is what makes a frame with no atmosphere byte-identical
    /// to the one it drew before the LUT existed rather than merely close.
    /// Nothing in the tree could see it: the only other reflective fixture
    /// without an atmosphere is [`Scene::Ssr`], whose sky is
    /// `crcbl_render::Sky::NONE`, so its bands are zero and a LUT that is also
    /// zero blends to the same zero whichever way the shader is written. This
    /// scene is the one that separates them — bands worth reflecting, and no
    /// atmosphere to reflect — so a `sky_environment` that read the LUT anyway
    /// darkens this floor by the share `sharpness_of` gives a near-mirror.
    ///
    /// # Every measured pixel is the reflection and nothing else
    ///
    /// [`Scene::AtmosphereMirror`]'s three reasons, unchanged and for the same
    /// arithmetic: the floor is fully metallic so no diffuse term reaches it,
    /// its light has no colour and no ambient so the specular lobe is zero, and
    /// the demo scene authors no probes so the environment is the sky alone.
    /// What is left in a floor pixel is `sky_prefiltered × (f0·scale + bias)`,
    /// which `tests/render_e2e.rs` predicts absolutely.
    GradientMirror,
    /// `docs/plan/18-render-features.md`'s **bloom chain**: a flat floor with
    /// one small, very bright patch on it, looked straight down at.
    ///
    /// **The only frame in the tree with content above the display range that is
    /// not spread over the whole picture**, and that is the whole of what a
    /// threshold-free chain needs to show itself. The patch is one material row
    /// away from the floor it sits on — the same mesh, the same plane, the same
    /// normal, the same sun — differing only in a base-colour factor of
    /// `BLOOM_EMITTER_GAIN`, so it is HDR-bright while nothing else in the
    /// frame is and it lights nothing around it.
    ///
    /// **It sits off the frame's centre on the `+X` side**, which is what gives
    /// `tests/render_e2e.rs` its control: the camera looks straight down and the
    /// sun points straight down, so the point at `-x` mirroring a point at `+x`
    /// is the same distance from the eye, carries the same normal and takes
    /// exactly the same direct light, ambient and occlusion. Proximity to the
    /// patch is the only term left that can separate them, and proximity is what
    /// a bloom is.
    ///
    /// It is a scene of its own rather than a bright object added to
    /// [`Scene::Spot`], for [`Scene::Ao`]'s third reason: **the sun points
    /// straight down**, so no shadow and no falloff can be mistaken for a halo.
    /// A patch bright because a light is near it would light its neighbourhood
    /// too, and the ratio would be measuring the falloff.
    ///
    /// See `bloom_camera` for the framing and `BLOOM_BAND_AT` for where the
    /// bands sit.
    Bloom,
    /// `docs/plan/18-render-features.md`'s **antialiasing resolve**: one flat
    /// slab turned about the view axis, so its silhouette runs diagonally across
    /// a dark frame.
    ///
    /// **The only frame in the tree whose subject is an edge.** Every other
    /// fixture here is about a value — a cone's brightness, a corner's occlusion,
    /// a halo's reach — and a filter that runs over the whole image changes those
    /// by almost nothing. What it changes is the handful of pixels along a
    /// silhouette, so this scene is arranged to have one long silhouette, at a
    /// slope no axis and no diagonal special case covers, between two flat levels
    /// — see this module's `AA_SLAB_TILT` and `aa_sun`.
    ///
    /// **Its golden cannot make the claim**, which is why `tests/render_e2e.rs`
    /// builds this scene twice through [`aa_forward`] instead. A resolve pass
    /// that copied its input would draw a frame with a clean hard edge in it, and
    /// a clean hard edge is what a golden of a slab looks like; only the same
    /// scene without the pass says which one this is.
    ///
    /// It draws with `RenderEffects::DEFAULT_STACK`, which carries
    /// [`crcbl_render::RenderEffects::ANTIALIASING`]: every frame in the tree is
    /// resolved. [`Scene::Bloom`] is the contrast — the lens is *not* in the
    /// default stack, so a fixture that wants it has to add it.
    Aa,
    /// `docs/plan/18-render-features.md`'s **irradiance probes**: the inside of
    /// a room, looked straight down into, lit by the probe grid and by nothing
    /// else at all.
    ///
    /// The floor has two broad, flat probe-lit regions separated by the grid's
    /// narrow interpolation interval. This keeps the fixture's probe-only mirror
    /// check while confining rasteriser disagreement to the transition instead of
    /// making the entire frame a smooth gradient.
    ///
    /// **The only built-in screenshot fixture that isolates the probe term.**
    /// Slice 1 landed the whole data path with every existing golden
    /// byte-identical, which is exactly what an
    /// additive term that is everywhere zero looks like — so `mesh.slang`'s
    /// `probe_irradiance` and
    /// [`crcbl_shaders::probe::irradiance_at`](crate::shaders::probe::irradiance_at)
    /// were two implementations of one thing with nothing comparing them. This
    /// scene is what compares them: `tests/render_e2e.rs` evaluates the Rust
    /// mirror at the same world positions the device shaded and asserts the two
    /// agree, which needs a frame whose pixels are the probe term and a scene
    /// whose probes it can read. [`probe_grid`] is that scene's rows, public for
    /// that reason.
    ///
    /// # Every measured pixel is the probe term
    ///
    /// * `DirectionalLight::ambient` and its direct colour are exactly zero — see
    ///   `probe_sun` — so neither flat ambient, Lambert, nor the specular lobe can
    ///   contribute on any surface.
    /// * The measured bands are centred a full unit from every wall — twice
    ///   `crcbl_render::ForwardRenderer`'s occlusion radius — so `ssao.slang`
    ///   finds nothing within reach and the occlusion scaling is exactly one.
    ///
    /// What is left is `albedo × probe_irradiance`, which makes an absolute
    /// comparison against the Rust mirror possible rather than only a ratio.
    ///
    /// The geometry is the open box alone — the cube every other forward scene
    /// places is not here, because this frame's whole content is one floor under
    /// one authored environment. See `probe_room` for the room's shape and
    /// `probe_grid` for why the two probes differ in the *direction* their light
    /// arrives from and not in how much of it there is.
    Probes,
    /// Rectangles, an outline and glyph-atlas text through [`UiRenderer`]:
    /// `ui.slang`.
    Ui,
}

/// How far from the cube's own column each pyramid sits, in world units.
///
/// The camera is two units back with a 60° vertical field of view, so at
/// `z = 0` the frame is about 2.3 units tall and 3.1 wide at 4:3. The cube
/// spans `±0.5` and each pyramid `±0.4`, so a column at `±1.05` sits fully
/// inside the frame with a gap on both sides of it — and a gap is what makes
/// "the pyramid drew the cube's vertices" a visibly different picture rather
/// than an overlap.
const PYRAMID_COLUMN: f32 = 1.05;

/// How far above or below the cube's own row each pyramid sits.
///
/// The pyramids used to share the cube's row, and a **third** of them is what
/// moved them off it: two rows are what a scene needs to hold two pairs, and
/// there is no room for a third column — the cube is 0.5 wide and each pyramid
/// 0.4, so a fourth object beside them would leave the frame.
///
/// `0.55` is what fits. A pyramid spans `-0.4 ..= 0.5` about its own origin and
/// the frame is `±1.15` tall, so a row at `+0.55` reaches `1.05` and one at
/// `-0.55` reaches `-0.95`: both inside, with `0.2` of clear sky between the
/// rows so no two pyramids touch.
const PYRAMID_ROW: f32 = 0.55;

/// Where [`Scene::Cube`] puts the plain pyramid: top left.
///
/// It is the one both other pyramids are compared against, and its material is
/// the untinted, untextured row — so each of the two below differs from *this*
/// object in exactly one material column.
const PYRAMID_AT: glam::Vec3 = glam::Vec3::new(-PYRAMID_COLUMN, PYRAMID_ROW, 0.0);

/// Where [`Scene::Cube`] puts the **factor** pyramid: top right, beside
/// [`PYRAMID_AT`].
///
/// The two are the same mesh at the same orientation and the same size, and the
/// only field their instances differ in is the material id — which is the whole
/// of what makes this frame evidence about §3.2's material table. A frame in
/// which the two pyramids are the same colour is a frame where that id indexed
/// nothing, and it is a *visibly* different frame rather than a subtly wrong
/// one.
///
/// Beside it rather than anywhere else so the difference is read across a row:
/// the two are at the same height, lit by the same directional light, and the
/// only thing that can make them different colours is the factor in the row
/// their id names.
const TINTED_PYRAMID_AT: glam::Vec3 = glam::Vec3::new(PYRAMID_COLUMN, PYRAMID_ROW, 0.0);

/// Where [`Scene::Cube`] puts the **texture** pyramid: below [`PYRAMID_AT`].
///
/// [`TINTED_PYRAMID_AT`]'s argument moved one column of the material row along.
/// This instance's material has [`PYRAMID_AT`]'s factor exactly and a different
/// base-colour page layer, so the pair above and below each other is the
/// observable for §3.2's *texture indices* where the pair across the top row is
/// the observable for its *factors*.
///
/// The layer it names is four unequal texels rather than a flat colour, so this
/// pyramid's faces are quartered where the plain one's are flat — which means
/// the frame also fails if the texture coordinate never reached the fragment
/// stage, not only if the index did.
///
/// Below rather than beside, because there is no third column: see
/// [`PYRAMID_ROW`]. The bottom-right corner is left empty for the same reason a
/// fourth pyramid would prove nothing new — a row differing in *both* columns
/// is a picture neither pair could be told from.
const TEXTURED_PYRAMID_AT: glam::Vec3 = glam::Vec3::new(-PYRAMID_COLUMN, -PYRAMID_ROW, 0.0);

/// How far in front of the geometry [`Scene::Lights`] hangs its point lights.
///
/// The cube and the pyramids sit at `z = 0` and the camera is on `+Z`, so a
/// light nearer than they are lights the faces the camera can see. Near enough
/// that the falloff is steep across one object, which is what makes the three
/// pools of colour separate rather than a wash.
const LIGHT_Z: f32 = 1.1;

/// How far apart two of these lights are, at their closest.
///
/// The two left-hand pyramids, which [`PYRAMID_ROW`] puts one above and one
/// below the cube's row. [`LIGHT_RADIUS`] is held under this, which is what
/// makes each pool one colour rather than three overlapping ones.
const LIGHT_SPACING: f32 = 2.0 * PYRAMID_ROW;

/// How far a [`Scene::Lights`] point light reaches, in world units.
///
/// **A hard bound**: `crcbl_render::PointLight::radius` is the radius the
/// shading window reaches zero at and the radius the clustering pass culls
/// against, so this is also how many froxels each light lands in.
///
/// Under [`LIGHT_SPACING`] and over the light's own distance to the faces below
/// it, so each pyramid is lit by its own light and by no other — which is what
/// makes the golden legible: a fragment stage that ignored the froxel grid and
/// summed the whole list would put all three colours on all three pyramids, and
/// that is a picture, not an error.
const LIGHT_RADIUS: f32 = 1.05;

/// How bright each [`Scene::Lights`] point light is, before its colour.
///
/// Above 1.0, like the sun's: the scene target is `Rgba16Float` and the tonemap
/// pass is what brings it back, so a light that peaked at 1.0 would be one this
/// pipeline could not tell from a darker one. Below the point where the
/// specular lobe clips, so the pools carry a gradient rather than a flat white
/// core.
const LIGHT_INTENSITY: f32 = 2.0;

/// The three [`Scene::Lights`] point lights: one over each pyramid, in a colour
/// family none of the materials is.
///
/// Red, blue and green rather than three whites, so which light lit a pixel is
/// legible in the golden — a wrong froxel assignment shows up as a pool in the
/// wrong place rather than as a slightly different brightness.
fn scene_lights() -> [crcbl_render::Light; 3] {
    const {
        assert!(
            LIGHT_RADIUS < LIGHT_SPACING,
            "a radius reaching the next pyramid puts two colours in one pool, and the \
             golden stops saying which light lit what"
        );
    }
    let at = |x: f32, y: f32, colour: glam::Vec3| {
        crcbl_render::Light::Point(crcbl_render::PointLight {
            position: glam::Vec3::new(x, y, LIGHT_Z),
            radius: LIGHT_RADIUS,
            color: colour * LIGHT_INTENSITY,
            fill: false,
        })
    };
    // **Each light's colour is chosen against the material under it**, because a
    // fragment's colour is the product of the two: [`PYRAMID_TINT`] makes the
    // top-right pyramid strongly blue, so a green light there would come out
    // blue and say nothing about which light lit it. The blue light goes where
    // the blue material is; the two neutral pyramids take the other two.
    //
    // **"The material" is the mesh's own vertex colour as much as the row's
    // factor**, and the green light is where that was learnt. Every pyramid
    // turns the same purple `+Z` face at the camera — `PYRAMID_SIDE_COLORS`'
    // third entry, whose blue is nearly three times its green — so the light
    // over the untinted-but-textured pyramid is the one fighting its own
    // surface. It carried the frame while `mesh.slang` shaded with a Blinn lobe
    // at a fixed strength of 0.35: that highlight was white and bright enough to
    // pull the pixel to the light's hue on its own. A GGX lobe at four per cent
    // dielectric reflectance is several times dimmer there, so the tie is now
    // decided by the diffuse alone — and the diffuse of a green light on a
    // purple face is a coin toss. Its blue is therefore held down to the same
    // figure the red light's blue already carries.
    [
        at(
            -PYRAMID_COLUMN,
            PYRAMID_ROW,
            glam::Vec3::new(1.0, 0.15, 0.1),
        ),
        at(PYRAMID_COLUMN, PYRAMID_ROW, glam::Vec3::new(0.15, 0.3, 1.0)),
        at(
            -PYRAMID_COLUMN,
            -PYRAMID_ROW,
            glam::Vec3::new(0.1, 1.0, 0.1),
        ),
    ]
}

/// [`DirectionalLight::default`] at `key` of its brightness and `ambient` of its
/// ambient, for a scene whose subject is the lights beside the sun.
///
/// **Turned down rather than removed**, and that is a claim each of those scenes
/// makes: the sun is a row of the same list the punctual lights are rows of, so
/// a frame in which it stopped contributing would be as wrong as one in which
/// they did.
///
/// Two factors rather than one because the ambient does a different job — it is
/// what keeps a face turned away from every light dark rather than black — and a
/// scene wants to dim the key light much further than the floor under it.
fn dimmed_sun(key: f32, ambient: f32) -> crcbl_render::DirectionalLight {
    let bright = crcbl_render::DirectionalLight::default();
    crcbl_render::DirectionalLight {
        direction: bright.direction,
        color: bright.color * key,
        ambient: bright.ambient * ambient,
    }
}

/// The sun [`Scene::Lights`] runs under: dim enough that the pools of colour are
/// unmistakably the three point lights' work.
///
/// **The key is what decides that, and the checker is what set it.** The lights
/// probe reads each quadrant's brightest pixel, and on the textured pyramid the
/// brightest pixel is the checker's white texel under whatever lights it — the
/// sun as much as the pool. While the page sampled nearest that texel was a
/// flat quarter the green pool sat on; sampled bilinear it is one point on a
/// ramp, and at twice this key the sun on that point out-shone the pool by a
/// step. The ambient is not the lever: sweeping it from `0.35` to `0.05` moved
/// that pixel's blue by seven steps, halving the key moved it by twenty-one.
fn dim_sun() -> crcbl_render::DirectionalLight {
    dimmed_sun(0.06, 0.35)
}

/// The sun [`Scene::Spot`] runs under, which is dimmer still.
///
/// That scene's claim is a **ratio** — the cone's core against the floor outside
/// it — so a sun bright enough to be a large part of that floor is a sun that
/// decides the ratio. Dim enough that the floor is unmistakably the dark side of
/// the cone's edge, and not so dim that it is black: a black floor would make
/// "dark outside the cone" a statement about an unpainted frame.
///
/// [`SPOT_INTENSITY`]'s doc says what the other end of the ratio is.
fn spot_sun() -> crcbl_render::DirectionalLight {
    dimmed_sun(0.03, 0.09)
}

/// How far above the floor [`Scene::Spot`] hangs its light.
///
/// **Large next to the cone's own footprint, and that ratio is what the pixel
/// assertions rest on.** Distance falloff and Lambert fall off across the pool
/// as well as the cone does, so on a low light a cone that was a hard cut rather
/// than a ramp would still leave a gradient behind it and "the penumbra varies"
/// would be satisfied by arithmetic that has nothing to do with the cone. From
/// up here the far edge of the pool is barely further from the light, and barely
/// more oblique to it, than the axis is — so what changes across the penumbra is
/// the cone and very little else.
const SPOT_HEIGHT: f32 = 1.6;

/// The radius of the cone's bright core where it lands on [`Scene::Spot`]'s
/// floor.
///
/// Written as a radius on the floor rather than as a half-angle in radians,
/// which is also how [`spot_light`] passes it: the radius is what the frame
/// shows and what the assertions talk about, and the angle is the arctangent of
/// it over [`SPOT_HEIGHT`].
const SPOT_CORE_RADIUS: f32 = 0.14;

/// The radius at which the cone has closed, on the same terms.
///
/// The band between this and [`SPOT_CORE_RADIUS`] is the penumbra — the part of
/// the picture that separates a working ramp from a boolean — so the two are set
/// far enough apart that it is tens of pixels wide under the camera
/// [`spot_camera`] puts over it, and `SPOT_PENUMBRA_MIN` in
/// `tests/render_e2e.rs` is what holds that to a number.
const SPOT_EDGE_RADIUS: f32 = 0.4;

/// How far [`Scene::Spot`]'s light reaches, in world units.
///
/// Twice [`SPOT_HEIGHT`], so the quartic window that
/// `crcbl_render::PointLight::radius` documents is nowhere near its zero where
/// the cone lands: the pool's edge is then the cone's doing rather than the
/// radius', which is the thing being drawn.
const SPOT_REACH: f32 = 2.0 * SPOT_HEIGHT;

/// How bright [`Scene::Spot`]'s light is, before its colour.
///
/// Well above [`LIGHT_INTENSITY`] because the falloff is an inverse square and
/// [`SPOT_HEIGHT`] is the distance it runs over — the product is what matters,
/// not either number. Chosen so the axis lands *under* the top of the swapchain
/// once the tonemap has brought it back: a core that clipped would be a plateau
/// with no shape in it, and the cone's own core is part of what this frame is
/// meant to show.
const SPOT_INTENSITY: f32 = 5.0;

/// How far above the floor [`Scene::Spot`]'s camera stands.
///
/// Lower than [`SPOT_HEIGHT`], so the light is behind the eye and nothing in the
/// frame is between the two. Far enough up that the frame's short half-axis on
/// the floor is comfortably wider than [`SPOT_EDGE_RADIUS`] — the pool then sits
/// well inside the frame with dark floor between it and every edge, which is
/// what lets a profile out from the centre run past the cone and reach that
/// floor.
const SPOT_CAMERA_UP: f32 = 1.2;

/// How much [`Scene::Spot`] scales the cube by to get its floor.
///
/// Uniform, so each face keeps the axis-aligned normal it was built with, and
/// large enough that the `+Y` face runs past every edge of the frame — the scene
/// is one flat lit surface and the cone on it, with no silhouette anywhere for a
/// reader or an assertion to take the shape off instead.
const SPOT_FLOOR_SCALE: f32 = 8.0;

/// The floor [`Scene::Spot`], its two shadow siblings and [`Scene::Ssr`] all
/// stand on: the cube scaled by [`SPOT_FLOOR_SCALE`] and dropped so its `+Y`
/// face is the plane `y = 0`.
///
/// The cube spans half a unit either side of its origin, so the drop is half the
/// scale.
///
/// One helper rather than one per scene, on [`place_pyramids`]' terms: four
/// frames stand on the same plane at the same scale, and a floor that moved in
/// one of them would be a difference nobody asked for in a golden that is about
/// something else. What [`Scene::Ssr`] varies is the material row it places this
/// transform through, not the transform.
fn spot_floor() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.5 * SPOT_FLOOR_SCALE, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::splat(SPOT_FLOOR_SCALE))
}

/// The camera [`Scene::Spot`] is drawn with: straight down at the floor.
///
/// **Overhead rather than oblique, and the pixel assertions rest on that.** With
/// the cone's axis, the floor's normal and the view direction all `Y`, every
/// term in the shading — the cone, the distance falloff, Lambert and the
/// specular lobe alike — is a function of the distance from the frame's centre
/// alone, and each of them falls off with it. So the pool is a circle about a
/// pixel the test can name, and a profile out from that pixel is the cone's own
/// cross-section rather than a diagonal cut through an ellipse whose position a
/// test would have to reconstruct the projection to find.
///
/// `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the floor's
/// `+Z` axis at the top of the frame.
fn spot_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, SPOT_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// [`Scene::Spot`]'s one light: a cone pointing straight down at the floor.
///
/// Near-white rather than a colour of its own, because the floor's albedo is the
/// cube's green `+Y` face and a coloured light would tint the pool where the
/// point of it is to show the pool's *shape*.
fn spot_light() -> crcbl_render::Light {
    crcbl_render::Light::Spot(crcbl_render::SpotLight {
        position: glam::Vec3::new(0.0, SPOT_HEIGHT, 0.0),
        radius: SPOT_REACH,
        color: glam::Vec3::new(1.0, 0.95, 0.85) * SPOT_INTENSITY,
        // Along the cone, away from the light — the opposite convention from the
        // sun's, which `crcbl_render::SpotLight::direction` is where it is
        // spelled out.
        direction: glam::Vec3::NEG_Y,
        inner_angle: (SPOT_CORE_RADIUS / SPOT_HEIGHT).atan(),
        outer_angle: (SPOT_EDGE_RADIUS / SPOT_HEIGHT).atan(),
        fill: false,
    })
}

/// How far apart [`Scene::Ao`]'s two facing walls stand, in world units.
///
/// **This is the scene's whole subject**: a trough, narrow enough that its walls
/// close over the floor between them and wide enough that the middle of that
/// floor is well outside `crcbl_render::ForwardRenderer`'s occlusion radius. The
/// bands `tests/render_e2e.rs` measures sit a tenth of a unit off each wall.
const AO_TROUGH: f32 = 1.6;

/// How far [`Scene::Ao`]'s trough runs, in world units.
///
/// **Several times [`AO_TROUGH`], and that asymmetry is what makes the scene
/// measurable.** The two ends are then far enough out to be off frame *and* many
/// occlusion radii from the middle — so a band out along the run is open floor
/// while a band the same distance out across the trough is against a wall. Two
/// bands at the same distance from the eye, on the same surface, under the same
/// light, differing in occlusion alone: a square room has no such pair, because
/// every point at a corner's distance from the centre is itself in a corner.
const AO_RUN: f32 = 6.0;

/// How tall [`Scene::Ao`]'s walls are, in world units.
///
/// Taller than the trough is wide, so each wall closes a good half of the
/// hemisphere over the floor beside it rather than a sliver of it — occlusion is
/// a solid angle, and a kerb would subtend almost none.
const AO_WALL: f32 = 2.0;

/// How far above the floor [`Scene::Ao`]'s camera stands, in world units.
///
/// Just above the wall tops, looking into the trough. It also sets the scale of
/// the picture: with the 60° vertical field of view `ao_camera` uses, the frame's
/// short half-axis on the floor is `AO_CAMERA_UP * tan(30°)` — so this is what
/// puts both walls inside the frame and both ends of the run outside it, which is
/// the framing every band below depends on.
const AO_CAMERA_UP: f32 = 2.2;

/// [`Scene::Ao`]'s trough: the open box scaled to
/// [`AO_RUN`] × [`AO_WALL`] × [`AO_TROUGH`] and lifted so its floor is the plane
/// `y = 0`.
///
/// **A non-uniform scale, and it needs nothing special of this mesh.** Every
/// face of `crcbl_shaders::mesh::OPEN_BOX_FACES` is axis aligned, so an
/// axis-aligned scale leaves each normal on its own axis whatever the shader
/// does with it — and a mesh with an oblique face is fine here too, since
/// `mesh.slang` takes a normal through `normal_basis`, the cofactor matrix.
///
/// **Centred on the camera's axis because that is where the claim is measured**,
/// and no longer to avoid anything. This once said a box this large slid
/// sideways stopped drawing altogether: a cluster's bounding radius was tested
/// against a world-space frustum while still in mesh space, so a scaled instance
/// lost every cluster while the instance-level cull kept it. This very mesh is
/// what that fix was measured on — its world radius is 3.10 against a local 0.71.
/// Re-checked by sliding this box 3.0 along `x` and rendering: both geometry
/// paths draw, and the only assertion that fails is the occlusion ratio, because
/// the camera then frames open floor instead of the corner.
fn ao_box() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.5 * AO_WALL, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::new(AO_RUN, AO_WALL, AO_TROUGH))
}

/// Where [`Scene::Ao`] parks the cube.
///
/// Every scene here places the cube — see [`place_cube`] — and this one has no
/// use for it: a lit box in the middle of the trough is a second shape in a frame
/// whose whole content is a floor and the two walls closing over it, and it would
/// stand exactly where the unoccluded bands are measured. So it is put out along
/// the run, past the frame and inside the trough — inside, so it cannot poke
/// through a wall on a future change to the framing.
fn ao_parked_cube() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.45 * AO_RUN, 0.5, 0.0))
}

/// The camera [`Scene::Ao`] is drawn with: straight down into the trough.
///
/// **Overhead, and the band placement rests on it.** With the view direction
/// along `Y` and the floor's normal along `Y`, four points the same distance from
/// the frame's centre on that floor are the same distance from the eye, carry the
/// same normal and — the sun being directional — take the same direct light. Two
/// of them are against a wall and two are on open floor, and the occlusion term is
/// then the only thing that can tell the pairs apart.
///
/// `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the trough's
/// `+Z` wall at the top of the frame, exactly as `spot_camera` does.
fn ao_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, AO_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// The sun [`Scene::Ao`] runs under: **straight down**, and barely there.
///
/// Both halves are load-bearing and neither is a taste.
///
/// * **Straight down** — a vertical light throws no shadow from a vertical wall,
///   so the floor is lit identically right up to where it meets one. Under the
///   default sun's tilt a wall [`AO_WALL`] tall would lay a shadow across the
///   floor beside it and the bands would be measuring the shadow map.
///   `crcbl_render::shadow` picks a second up vector for exactly this direction,
///   so the cascades are built rather than degenerate.
/// * **Barely there** — occlusion scales the ambient term and nothing else, so a
///   bright key light is something the measurement has to see through. Turned
///   down rather than removed, for `dimmed_sun`'s reason: a sun that stopped
///   contributing is a row of the light list that stopped working, and a scene
///   without one would not notice.
fn ao_sun() -> crcbl_render::DirectionalLight {
    crcbl_render::DirectionalLight {
        direction: glam::Vec3::Y,
        ..dimmed_sun(0.01, 1.0)
    }
}

/// How far above the floor [`Scene::Bloom`]'s camera stands, in world units.
///
/// Sets the scale of the picture, on [`AO_CAMERA_UP`]'s terms: with the 60°
/// vertical field of view `bloom_camera` uses, the frame's short half-axis on the
/// floor is `BLOOM_CAMERA_UP * tan(30°)` — so this is what decides how many
/// pixels the emitter is across and how far the halo has to reach before a band
/// can be measured in it.
///
/// Chosen so the patch is about a sixth of the frame's height: small enough that
/// the chain's coarse levels spread it well past its own edge, large enough that
/// it is not one texel of the smallest level.
const BLOOM_CAMERA_UP: f32 = 2.6;

/// How far along `+X` [`Scene::Bloom`] puts its emitter, in world units.
///
/// **Off centre, and that is the fixture.** The mirror of a band beside the
/// patch is a band the same distance from the frame's centre on the other side,
/// which under this camera and this sun differs from it in proximity to the
/// patch and in nothing else. A patch at the origin would have no such mirror:
/// every control would be further from the eye than the band it controls.
const BLOOM_EMITTER_AT: f32 = 0.75;

/// How wide [`Scene::Bloom`]'s emitter is, in world units.
///
/// Half a unit — a `0.25` half-width, so with [`BLOOM_CAMERA_UP`]'s framing it
/// is about a thirty-second of the frame's width and its `+X` edge lands at
/// `BLOOM_EMITTER_AT + 0.25`.
const BLOOM_EMITTER_SIZE: f32 = 0.5;

/// How thick [`Scene::Bloom`]'s emitter is, in world units.
///
/// **A slab lying on the floor rather than a box standing on it**, and thin
/// enough to occlude nothing: the occlusion pass gathers within
/// `crcbl_render`'s own world-space radius, and a lip this shallow darkens no
/// pixel a band is measured in. A box would put its own ambient occlusion
/// exactly where the halo is, which is a term working against the measurement
/// for no gain — the patch is here to be bright, not to be an object.
const BLOOM_EMITTER_THICKNESS: f32 = 0.02;

/// The base-colour factor [`Scene::Bloom`]'s emitter shades through.
///
/// **Far above one, which is the point.** `GpuMaterial::base_color` is a linear
/// multiplier into the vertex albedo and nothing clamps it, so this is how a
/// surface in this engine is made brighter than the display without inventing an
/// emissive term or putting a light where it would spill onto the floor. The
/// tonemap flattens it to white; the chain, which runs before the tonemap, sees
/// the whole of it.
const BLOOM_EMITTER_GAIN: f32 = 120.0;

/// [`Scene::Bloom`]'s emitter row, in the description `bloom_scene` builds.
///
/// The demo scene's three rows and this one after them, so the three every other
/// fixture names keep the indices they have always had.
const BLOOM_EMITTER: usize = 3;

/// [`Scene::Bloom`]'s scene: the engine's own, with the emitter row appended.
///
/// A description of its own for [`Scene::Probes`]' reason — this is the only
/// fixture that needs a material the demo scene does not have — and the *only*
/// thing that differs is that one row.
fn bloom_scene() -> crate::render::scene::SceneDesc<'static> {
    let mut scene = crate::render::scene::demo();
    scene.materials.push(crate::shaders::mesh::GpuMaterial {
        base_color: [
            BLOOM_EMITTER_GAIN,
            BLOOM_EMITTER_GAIN,
            BLOOM_EMITTER_GAIN,
            1.0,
        ],
        ..crate::shaders::mesh::GpuMaterial::UNTINTED
    });
    debug_assert_eq!(
        scene.materials.len() - 1,
        BLOOM_EMITTER,
        "the emitter row is the one past the demo scene's three"
    );
    scene
}

/// [`Scene::Bloom`]'s emitter: the cube flattened into a slab and laid on the
/// floor at [`BLOOM_EMITTER_AT`].
///
/// The cube spans half a unit either side of its origin, so the lift is half the
/// thickness and the slab's underside is the floor plane exactly.
fn bloom_emitter() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(
        BLOOM_EMITTER_AT,
        0.5 * BLOOM_EMITTER_THICKNESS,
        0.0,
    )) * glam::Mat4::from_scale(glam::Vec3::new(
        BLOOM_EMITTER_SIZE,
        BLOOM_EMITTER_THICKNESS,
        BLOOM_EMITTER_SIZE,
    ))
}

/// The camera [`Scene::Bloom`] is drawn with: straight down at the floor.
///
/// [`ao_camera`]'s arrangement and [`ao_camera`]'s reason: with the view
/// direction along `Y` and the floor's normal along `Y`, two points the same
/// distance from the frame's centre are the same distance from the eye and carry
/// the same normal, so a directional sun gives them the same direct light. The
/// halo is then the only thing that can tell them apart.
///
/// `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the floor's
/// `+Z` axis at the top of the frame, exactly as `ao_camera` and `spot_camera`
/// do.
fn bloom_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, BLOOM_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// The sun [`Scene::Bloom`] runs under: **straight down**, and turned down to a
/// mid-tone.
///
/// * **Straight down** — every point of a flat floor then takes exactly the same
///   direct light, which is what makes a band and its mirror controls for each
///   other. Under the default sun's tilt the two sides of the frame would differ
///   in `N·L` and the ratio would be measuring the light rather than the halo.
///   `crcbl_render::shadow` picks a second up vector for this direction, so the
///   cascades are built rather than degenerate — [`ao_sun`] leans on the same
///   fact.
/// * **A mid-tone floor** — the halo is *added* to whatever the floor already
///   shows, so a floor near white would clip the measurement away and a black one
///   would make "brighter than the floor" a statement about an unpainted frame.
fn bloom_sun() -> crcbl_render::DirectionalLight {
    crcbl_render::DirectionalLight {
        direction: glam::Vec3::Y,
        ..dimmed_sun(0.2, 1.0)
    }
}

/// How far back from the slab [`Scene::Aa`]'s camera stands, in world units.
///
/// Sets the scale of the picture on [`BLOOM_CAMERA_UP`]'s terms: with the 60°
/// vertical field of view [`aa_camera`] uses, the frame's short half-axis at the
/// slab's face is `AA_CAMERA_BACK * tan(30°)`. Chosen so that a slab of
/// [`AA_SLAB_SIZE`] crosses most of the frame without its corners leaving it —
/// a silhouette that runs off the edge is a silhouette whose length depends on
/// the framing rather than on the shape.
const AA_CAMERA_BACK: f32 = 2.6;

/// How wide and tall [`Scene::Aa`]'s slab is, in world units.
const AA_SLAB_SIZE: f32 = 1.6;

/// How deep [`Scene::Aa`]'s slab is, in world units.
///
/// Thin, so the frame contains the front face and nothing else: a slab with
/// visible sides has a second edge inside its own silhouette, shaded differently,
/// and the count [`Scene::Aa`]'s claim makes would be over two edges of which
/// only one is the subject.
const AA_SLAB_DEPTH: f32 = 0.05;

/// How far [`Scene::Aa`] turns its slab about the view axis, in radians.
///
/// **Not an eighth of a turn, and not a sixteenth.** A silhouette at exactly 45°
/// is the one case where the staircase has a step every pixel and its rise is
/// exactly its run, which is the easiest slope there is to filter and the least
/// representative of anything. This is a little over 20°, where the steps are
/// several pixels long and unevenly spaced — the case a filter has to estimate a
/// direction for rather than read off the two neighbours.
///
/// Nor is it near an axis: a silhouette within a degree or two of vertical is one
/// long straight run of pixels, which has no staircase to remove.
const AA_SLAB_TILT: f32 = 0.36;

/// [`Scene::Aa`]'s slab: the cube flattened and turned about the view axis.
fn aa_slab() -> glam::Mat4 {
    glam::Mat4::from_rotation_z(AA_SLAB_TILT)
        * glam::Mat4::from_scale(glam::Vec3::new(AA_SLAB_SIZE, AA_SLAB_SIZE, AA_SLAB_DEPTH))
}

/// How far below the slab [`Scene::Aa`] parks the pyramid, in world units.
const AA_PARK_DOWN: f32 = 12.0;

/// Where [`Scene::Aa`] parks the pyramid.
///
/// The demo scene's other resident has no place in a frame whose whole content is
/// one silhouette against the background — a second shape would put its own edges
/// in the count. Parked out of frame on [`ao_parked_cube`]'s terms: out rather
/// than absent, so the instance pool holds the slot it holds in every other
/// fixture.
///
/// **Straight down, and not behind the camera.** Behind the camera is out of
/// frame and still in the light: [`aa_sun`] shines along `-Z`, so a pyramid
/// parked at `+Z` stands between the sun and the slab and lays its shadow across
/// the middle of the face — which is what this arrangement did on its first
/// render, a dark blob where the flat level was supposed to be. Straight down is
/// out of the frustum and out of the sun's path to the slab both.
fn aa_parked_pyramid() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, -AA_PARK_DOWN, 0.0))
}

/// The camera [`Scene::Aa`] is drawn with: square on to the slab, down `-Z`.
///
/// Head-on and not at an angle, because the tilt that makes the silhouette
/// diagonal is the slab's own — see [`AA_SLAB_TILT`]. Turning the camera instead
/// would foreshorten the face and give the silhouette a perspective slope that
/// changes along its length, so the two halves of one edge would be different
/// slopes and the frame would stop being about one of them.
fn aa_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, 0.0, AA_CAMERA_BACK),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Y,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// The sun [`Scene::Aa`] runs under: **straight at the slab's face**, with no
/// ambient worth the name.
///
/// Both halves exist to make the face one flat value.
///
/// * **Along the view axis** — `DirectionalLight::direction` is the direction
///   *towards* the light, so `+Z` puts it behind the eye and every point of a
///   face whose normal is `+Z` takes the same `N·L`. A tilted sun would shade the
///   face across its width, and the silhouette's contrast would then differ along
///   its own length.
/// * **Almost no ambient** — the background this frame is measured against is
///   whatever the pass clears to, and the claim is a count of values *between*
///   the two levels. A large ambient term lifts the face and the frame both, and
///   narrows the gap the count is taken across.
fn aa_sun() -> crcbl_render::DirectionalLight {
    crcbl_render::DirectionalLight {
        direction: glam::Vec3::Z,
        ..dimmed_sun(1.0, 0.02)
    }
}

/// [`Scene::Aa`]'s renderer, built with `effects` as its camera stack.
///
/// **Public, and the effect set is a parameter, because the claim this fixture
/// supports is a comparison against itself.** An antialiased edge is not
/// recognisable in isolation: a frame whose filter did nothing is a frame with a
/// clean hard silhouette in it, which is exactly what a golden of this scene
/// would be blessed as. So `tests/render_e2e.rs` builds this scene twice —
/// [`Scene::Aa`]'s own set and the default stack — and compares the two, and
/// that needs the build to be reachable with a set the [`Scene`] enum does not
/// name.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built.
pub fn aa_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    effects: crcbl_render::RenderEffects,
) -> Result<ForwardScene, OffscreenError> {
    let mut renderer = ForwardRenderer::new(device, queue, format)?;
    renderer.set_effect_request(EffectRequest {
        camera: effects,
        ..EffectRequest::default()
    });
    // The cube as the slab — still the first insertion, so it still holds the
    // pool slot every other scene gives it — and the pyramid out of frame.
    place(&mut renderer, DEMO_CUBE, DEMO_UNTINTED, aa_slab());
    place(
        &mut renderer,
        DEMO_PYRAMID,
        DEMO_UNTINTED,
        aa_parked_pyramid(),
    );
    Ok(ForwardScene {
        camera: aa_camera(),
        sun: aa_sun(),
        renderer: Box::new(renderer),
    })
}

/// [`Scene::Ssr`]'s build, with the sky a parameter.
///
/// The cube as the floor and the pyramid standing on it, and nothing else —
/// [`Scene::Spot`]'s reason: what this frame is about is one flat reflective
/// surface and one thing for it to reflect.
///
/// **The cube is placed through `DEMO_TINTED` rather than through
/// `place_cube`.** It is still the first insertion, so it still holds the pool
/// slot every other scene gives it; what differs is the row, and that row's
/// roughness is the only one in the demo scene the reflection pass can see.
///
/// **Public, and both the sky and the effect set are parameters, for
/// [`aa_forward`]'s reason**: the claim this fixture supports is a comparison
/// against itself.
///
/// A ray that leaves the floor and finds no geometry falls back to the
/// environment, and what that environment *is* cannot be read off one frame — a
/// sky and a brighter floor look alike. Worse, a sky is not only the
/// reflection's fallback: it lights the ambient term too, and the floor's
/// normal points the same way its reflected rays go, so switching a sky on
/// brightens that floor twice for two different reasons.
///
/// **Which is why the effect set is here.** `tests/render_e2e.rs` renders this
/// scene four ways — each sky with the reflection pass on and with it off — and
/// reads the difference between the pair. The ambient half is identical inside
/// a pair and cancels, leaving the reflection alone.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built.
pub fn ssr_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    sky: crcbl_render::Sky,
    effects: crcbl_render::RenderEffects,
) -> Result<ForwardScene, OffscreenError> {
    ssr_forward_on(
        device,
        queue,
        format,
        sky,
        effects,
        &crate::render::scene::demo(),
        DEMO_TINTED,
    )
}

/// The three suns [`atmosphere_forward`] draws its sky under.
///
/// Not normalised, deliberately: `crcbl_render::Atmosphere` normalises on the
/// way in, and handing it a vector that is already unit would leave that
/// untested. The three are chosen to bracket what the sky-view LUT's two axes
/// have to get right — a sun overhead, where the field is azimuthally symmetric
/// and the column map cannot be observed at all; a sun low and **across** the
/// camera's forward, where the bright limb is at the frame's edge; and a sun
/// low and **behind** the frame's centre, where it is not on screen and the
/// aureole is only its glow.
pub const ATMOSPHERE_SUNS: [[f32; 3]; 3] = [[0.0, 4.0, 0.0], [3.0, 1.0, 0.0], [0.0, 0.35, -2.0]];

/// How bright the sun is above the atmosphere in [`atmosphere_forward`].
///
/// One in every channel, so what the frame shows is the atmosphere's own
/// transfer and not an exposure — and so a test predicting a pixel has one
/// fewer number to be wrong about. `crcbl_shaders::atmosphere`'s
/// `the_sky_is_linear_in_the_suns_illuminance` is what says the rest of the
/// range is this one scaled.
const ATMOSPHERE_ILLUMINANCE: f32 = 1.0;

/// The atmosphere [`atmosphere_forward`] draws under sun `index`.
///
/// **Public because the test predicts the frame from it**, on
/// [`probe_grid`]'s terms: the claim is that the device's sky and
/// `crcbl_shaders::atmosphere::SkyView`'s are the same sky, and a test that
/// built its own atmosphere would be comparing two guesses.
///
/// # Panics
///
/// If `index` is not one of [`ATMOSPHERE_SUNS`].
#[must_use]
pub fn atmosphere_sky(index: usize) -> crcbl_render::Atmosphere {
    let sun = ATMOSPHERE_SUNS[index];
    crcbl_render::Atmosphere {
        sun_direction: glam::Vec3::from_array(sun),
        sun_illuminance: glam::Vec3::splat(ATMOSPHERE_ILLUMINANCE),
        altitude_km: 0.0,
    }
}

/// The sky-view LUT [`atmosphere_sky`] resolves to, marched on the host.
///
/// The mirror the device frame is read against. It goes through
/// `crcbl_render::Atmosphere` rather than being built from
/// `ATMOSPHERE_SUNS` directly, so the normalisation the renderer applies is on
/// this side of the comparison too.
///
/// # Panics
///
/// If `index` is not one of [`ATMOSPHERE_SUNS`].
#[must_use]
pub fn atmosphere_view(index: usize) -> crate::shaders::atmosphere::SkyView {
    crate::shaders::atmosphere::SkyView::build(&atmosphere_sky(index).parameters())
}

/// Where [`atmosphere_forward`] is seen from.
///
/// Public so the test can unproject a pixel into the same world ray
/// `sky.slang` does — the two matrices are this camera's, and reproducing them
/// from a second spelling is how a test ends up measuring its own arithmetic.
///
/// It looks a little above the horizon along `−Z`, so the frame spans the band
/// where an atmosphere actually changes: the horizon's own bright rim at the
/// bottom, the zenith's blue at the top.
#[must_use]
pub fn atmosphere_camera() -> Camera {
    Camera {
        eye: glam::Vec3::ZERO,
        target: glam::Vec3::new(0.0, 0.25, -1.0),
        up: glam::Vec3::Y,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.05,
        },
    }
}

/// An empty scene under [`atmosphere_sky`]: every pixel of the frame is the
/// atmosphere.
///
/// **No instances at all**, which is the point rather than a shortcut. The sky
/// pass draws where the depth attachment is still the reversed-Z far plane, so
/// a frame with nothing in it is a frame that is entirely the sky pass's
/// output — and a band read anywhere in it is the LUT along one ray, with no
/// surface's shading between the two.
///
/// **Every effect refused**, on `Scene::Probes`' terms and more of them: bloom
/// would bleed the bright limb across the frame, the reflection pass and the
/// occlusion pass would both read the background they are not about, and the
/// antialiasing passes would filter the very gradient this measures.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built.
///
/// # Panics
///
/// If `index` is not one of [`ATMOSPHERE_SUNS`].
pub fn atmosphere_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    index: usize,
) -> Result<ForwardScene, OffscreenError> {
    let mut renderer =
        ForwardRenderer::with_scene(device, queue, format, &crate::render::scene::demo())?;
    renderer.set_effect_request(EffectRequest {
        camera: crcbl_render::RenderEffects::empty(),
        ..EffectRequest::default()
    });
    renderer.set_atmosphere(Some(atmosphere_sky(index)));
    Ok(ForwardScene {
        camera: atmosphere_camera(),
        // A sun that lights nothing, since there is nothing to light: the
        // frame's whole content is the background, and a directional light
        // would only put an ambient term where no surface is.
        sun: crcbl_render::DirectionalLight {
            color: glam::Vec3::ZERO,
            ambient: glam::Vec3::ZERO,
            ..crcbl_render::DirectionalLight::default()
        },
        renderer: Box::new(renderer),
    })
}

/// The sun [`Scene::AtmosphereMirror`]'s atmosphere is built around.
///
/// Not normalised, for [`ATMOSPHERE_SUNS`]' reason: `crcbl_render::Atmosphere`
/// normalises on the way in.
///
/// Both components are the scene. The horizontal part is `(1, 0, -1)`, which is
/// **45° to the right of the camera's forward** — see
/// [`atmosphere_mirror_camera`] — so the sun's own azimuth sits just outside
/// the frame's right edge and the band reflected there is a few degrees off the
/// limb while the band at the mirrored column is nearly ninety degrees away
/// from it. The vertical part puts the sun about eleven degrees up: low enough
/// that the aureole is the brightest thing the LUT holds and high enough that
/// the band elevations this scene measures at — about fifteen degrees — are
/// still inside it.
const ATMOSPHERE_MIRROR_SUN: [f32; 3] = [1.0, 0.28, -1.0];

/// How rough [`Scene::AtmosphereMirror`]'s floor is.
///
/// **Not zero, and the reason is a constant a test would otherwise have to
/// copy.** `mesh.slang` clamps a material's roughness to its own
/// `MIN_ROUGHNESS` before shading and before the reflectivity store, and that
/// number has no Rust mirror — so a floor authored at zero would reload as a
/// level the host could only predict by re-spelling a shader constant. Above
/// it, `crcbl_shaders::ssr::stored_roughness` is the whole of what the
/// attachment does to this value, and that function is already public for
/// exactly this.
///
/// It is far under `ssr.slang`'s `ROUGHNESS_CUTOFF`, so the sharpness ramp —
/// which is also the share of the reflection that reads the sky-view LUT
/// rather than the three bands — is close to one.
pub const ATMOSPHERE_MIRROR_ROUGHNESS: f32 = 0.05;

/// How far above the floor [`Scene::AtmosphereMirror`]'s eye stands, in world
/// units.
///
/// One, and nothing rests on the value: the camera is level, so a floor pixel's
/// reflected *direction* is a function of the pixel alone and the eye height
/// only decides which floor point that pixel is. What it does have to be is
/// above the floor and well inside the near plane.
const ATMOSPHERE_MIRROR_EYE_UP: f32 = 1.0;

/// How wide [`Scene::AtmosphereMirror`]'s floor plate is, in world units.
///
/// **Large enough that its far edge lands within a pixel of the horizon**,
/// which is what makes the frame floor below the middle row and sky above it
/// with nothing between. A plate ending short of the horizon would show a strip
/// of the sky-view LUT's lower hemisphere — deliberately black, see
/// `docs/backlog.md` — as a dark band across the picture, and a band a reader
/// would have to be told to ignore.
///
/// At this size the far edge sits about `0.29°` below the horizon, against the
/// `0.31°` a row of the golden's extent spans — so it is inside the first row
/// under the middle one.
const ATMOSPHERE_MIRROR_FLOOR_SCALE: f32 = 400.0;

/// The atmosphere [`Scene::AtmosphereMirror`] is drawn under.
///
/// **Public because the test predicts the frame from it**, on
/// [`atmosphere_sky`]'s terms: the claim is that the mirror's reflection is the
/// host's own `SkyView` along the reflected direction, and a test that built
/// its own atmosphere would be comparing two guesses.
#[must_use]
pub fn atmosphere_mirror_sky() -> crcbl_render::Atmosphere {
    crcbl_render::Atmosphere {
        sun_direction: glam::Vec3::from_array(ATMOSPHERE_MIRROR_SUN),
        sun_illuminance: glam::Vec3::splat(ATMOSPHERE_ILLUMINANCE),
        altitude_km: 0.0,
    }
}

/// The sky-view LUT [`atmosphere_mirror_sky`] resolves to, marched on the host.
///
/// [`atmosphere_view`]'s job for this scene, and it goes through
/// `crcbl_render::Atmosphere` for that function's reason — the normalisation
/// the renderer applies is on this side of the comparison too.
#[must_use]
pub fn atmosphere_mirror_view() -> crate::shaders::atmosphere::SkyView {
    crate::shaders::atmosphere::SkyView::build(&atmosphere_mirror_sky().parameters())
}

/// The camera [`Scene::AtmosphereMirror`] is drawn with: level, on the `+Y`
/// axis, looking along `-Z`.
///
/// **Level, and every band rests on that.** With the eye's forward horizontal,
/// a pixel's view ray has an elevation that depends on its row and its column
/// only through `|ndc.x|`, so two pixels at mirrored columns of one row look
/// down at exactly the same angle and their reflections rise at exactly the
/// same angle. The three-band gradient is a function of that elevation alone
/// and therefore predicts the two identically; the sky-view LUT has an azimuth
/// in it and does not. That difference is this fixture's whole subject.
///
/// It is also what puts the horizon on the frame's middle row, so the picture
/// is floor under sky with no tilt to reconstruct.
///
/// Public so the test can unproject a pixel into the same world ray the shaders
/// do — [`atmosphere_camera`]'s reason.
#[must_use]
pub fn atmosphere_mirror_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, ATMOSPHERE_MIRROR_EYE_UP, 0.0),
        target: glam::Vec3::new(0.0, ATMOSPHERE_MIRROR_EYE_UP, -1.0),
        up: glam::Vec3::Y,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.05,
        },
    }
}

/// [`Scene::AtmosphereMirror`]'s mesh index, the one past the demo scene's
/// four.
const ATMOSPHERE_MIRROR_MESH: usize = 4;

/// [`Scene::AtmosphereMirror`]'s floor: **one authored quad**, in the plane
/// `y = 0` and facing `+Y`, [`ATMOSPHERE_MIRROR_FLOOR_SCALE`] on a side.
///
/// **Not the demo cube, and that is the fixture rather than a preference.**
/// `crcbl_shaders::mesh::FACES` gives every face of that cube a vertex colour —
/// the `+Y` one is green — and `mesh.slang` multiplies it into the albedo, so a
/// mirror made of it has a green `F0` and reflects a green sky. This scene's
/// claim is an absolute comparison against a host `SkyView`, so the surface has
/// to be the neutral one: a white vertex tint, a white material factor, and an
/// `F0` of exactly one that needs no rounding step modelled on the host.
///
/// `double_sided_quad_mesh` is the same construction facing the other way, and
/// the paragraphs it carries about the cluster's cone and the absent tangents
/// are this function's too.
fn atmosphere_mirror_mesh() -> crate::render::scene::MeshDesc<'static> {
    use crate::shaders::mesh::MeshVertex;
    use crate::shaders::meshlet::{MeshClusters, Meshlet};

    let range = crate::shaders::mesh::demo_uv_range();
    let tint = [1.0, 1.0, 1.0, 1.0];
    let normal = [0.0, 1.0, 0.0];
    let half = 0.5 * ATMOSPHERE_MIRROR_FLOOR_SCALE;
    // `crcbl_shaders::mesh`'s `+Y` face corner order, which `0 1 2, 0 2 3`
    // winds counter-clockwise seen from above.
    let corners = [
        [-half, 0.0, half],
        [half, 0.0, half],
        [half, 0.0, -half],
        [-half, 0.0, -half],
    ];
    let vertices: Vec<MeshVertex> = corners
        .iter()
        .zip(&SPECULAR_QUAD_UV)
        .map(|(corner, uv)| MeshVertex::from_normal(*corner, normal, tint, *uv, &range))
        .collect();

    let mut clusters = MeshClusters::default();
    // `specular_cluster_bounds` already writes the `+Y` axis and the cutoff of
    // one that a single flat quad in this plane has exactly.
    let bounds = specular_cluster_bounds(&corners);
    clusters.clusters.push(
        Meshlet::new(0, 4, 0, 2, bounds)
            .unwrap_or_else(|error| unreachable!("four vertices of fixture geometry: {error}")),
    );
    clusters.vertices.extend_from_slice(&[0, 1, 2, 3]);
    clusters.corners.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    clusters
        .check(vertices.len())
        .unwrap_or_else(|fault| unreachable!("the fixture's own cluster is in range: {fault}"));

    crate::render::scene::MeshDesc {
        label: std::borrow::Cow::Borrowed("atmosphere mirror floor"),
        geometry: crate::render::scene::Geometry::Flat {
            vertices: std::borrow::Cow::Owned(crate::shaders::mesh::vertex_bytes(&vertices)),
            uv_range: range,
            indices: std::borrow::Cow::Owned(vec![0u32, 1, 2, 0, 2, 3]),
            clusters,
            // No authored tangents, on `specular_plate_mesh`'s terms.
            flags: 0,
        },
    }
}

/// The light [`Scene::AtmosphereMirror`] runs under: the atmosphere's own sun
/// direction, and **no light at all**.
///
/// The direction is the sun's so the frame is honest about where its sky's sun
/// is; the colour and the ambient are zero so nothing in the frame is lit by
/// it. Both halves are load-bearing: a metallic floor has no diffuse lobe, so
/// what a non-zero colour would add is the specular gleam of a directional
/// light — a second bright term on the very floor whose reflection the bands
/// measure, and one the host would have to model to predict a pixel.
fn atmosphere_mirror_sun() -> crcbl_render::DirectionalLight {
    crcbl_render::DirectionalLight {
        direction: glam::Vec3::from_array(ATMOSPHERE_MIRROR_SUN).normalize(),
        color: glam::Vec3::ZERO,
        ambient: glam::Vec3::ZERO,
    }
}

/// [`Scene::AtmosphereMirror`]'s material row: the one past the demo scene's
/// three.
const ATMOSPHERE_MIRROR_MATERIAL: usize = 3;

/// [`Scene::AtmosphereMirror`]'s scene: the demo scene with
/// [`atmosphere_mirror_mesh`] appended and one material row after it — a white
/// conductor at [`ATMOSPHERE_MIRROR_ROUGHNESS`].
///
/// White and fully metallic, which is what makes the prediction short: `F0` is
/// `(1, 1, 1)` exactly, it stores into the `Rgba8Unorm` reflectivity attachment
/// without a rounding step, and the split-sum's `f0 · scale + bias` is the
/// table's own energy at that roughness rather than a tint the host would have
/// to carry through.
fn atmosphere_mirror_scene() -> crate::render::scene::SceneDesc<'static> {
    let mut scene = crate::render::scene::demo();
    scene.meshes.push(atmosphere_mirror_mesh());
    debug_assert_eq!(
        scene.meshes.len() - 1,
        ATMOSPHERE_MIRROR_MESH,
        "the plate is the mesh past the demo scene's four"
    );
    scene.materials.push(crate::shaders::mesh::GpuMaterial {
        metallic: 1.0,
        roughness: ATMOSPHERE_MIRROR_ROUGHNESS,
        ..crate::shaders::mesh::GpuMaterial::UNTINTED
    });
    debug_assert_eq!(
        scene.materials.len() - 1,
        ATMOSPHERE_MIRROR_MATERIAL,
        "the conductor is the row past the demo scene's three"
    );
    scene
}

/// The three bands [`Scene::GradientMirror`]'s sky is authored as.
///
/// **Public because the test predicts the frame from it**, on
/// [`atmosphere_mirror_sky`]'s terms exactly: the claim is that the floor
/// reflects `crcbl_shaders::sky_prefilter`'s convolution of these three bands,
/// and a test that authored its own sky would be comparing two guesses. It is
/// the shader crate's record rather than [`crcbl_render::Sky`] so that the one
/// value serves both sides — `gradient_mirror_sky` is the renderer's view of
/// it, and the prediction reads the bands themselves.
///
/// Warm at the horizon and deep blue at the zenith, so the bands are three
/// different colours and the elevations this frame reflects — the horizon at
/// the middle row, steepening towards the bottom — read as a gradient in every
/// channel rather than as one number. A convolution that collapsed the bands
/// to their mean would draw a flat plate.
///
/// The ground band is what a ray below the horizon would see, and no ray in
/// this frame looks there — `docs/backlog.md` carries that as a gap in the
/// tree rather than as this fixture's job. It is well under the horizon's, so
/// the little a near-mirror lobe reaches down for is a rounding step rather
/// than a second bright band, and so the *background* under the horizon is
/// unmistakably darker than the floor drawn over it.
///
/// Every band is far under one, so `tonemap.slang`'s `saturate` is the
/// identity on this frame and the host's prediction has no clamp to model.
pub const GRADIENT_MIRROR_SKY: crate::shaders::sky::SkyGradient =
    crate::shaders::sky::SkyGradient {
        zenith: [0.02, 0.05, 0.15],
        horizon: [0.30, 0.20, 0.10],
        ground: [0.05, 0.04, 0.03],
    };

/// [`GRADIENT_MIRROR_SKY`] in the form [`ForwardRenderer::set_sky`] takes.
///
/// [`ForwardRenderer::set_sky`]: crcbl_render::ForwardRenderer::set_sky
fn gradient_mirror_sky() -> crcbl_render::Sky {
    crcbl_render::Sky {
        zenith: glam::Vec3::from_array(GRADIENT_MIRROR_SKY.zenith),
        horizon: glam::Vec3::from_array(GRADIENT_MIRROR_SKY.horizon),
        ground: glam::Vec3::from_array(GRADIENT_MIRROR_SKY.ground),
    }
}

/// [`Scene::Probes`]' room under an atmosphere and nothing else: **the frame
/// whose floor is lit by the sky's L1 rows alone.**
///
/// **What `frame.sky_sh_*` is for, on a device.**
/// `crcbl_render::ForwardRenderer`'s frame block carries
/// `crcbl_shaders::atmosphere::SkyView::irradiance` of the marched LUT and
/// `mesh.slang`'s `sky_irradiance` evaluates it as three dot products — and
/// until this fixture no frame in the tree shaded a surface through those
/// rows. [`atmosphere_forward`] puts no geometry in its scene at all, and
/// [`Scene::AtmosphereMirror`]'s floor is a conductor, whose diffuse albedo is
/// exactly zero and so has nothing for an ambient term to reach.
///
/// **The floor is left with exactly one term.** `probe_sun`'s light has no
/// colour and no ambient, the demo scene authors no probes, and the reflection
/// pair is refused on [`Scene::Probes`]' terms — so `mesh.slang`'s ambient sum
/// is `0 + sky_irradiance(N) + 0`, and a floor pixel is that times the floor's
/// own albedo through the tonemap's identity and the sRGB encode. Every band
/// `tests/render_e2e.rs` reads is therefore an absolute prediction rather than
/// a ratio.
///
/// **The atmosphere is a parameter**, on [`ssr_forward`]'s terms: a floor lit
/// by a sky and a floor lit by anything else look alike, so the fixture is
/// drawn with an atmosphere and without one and the second frame is what says
/// the term arrived from the sky rather than from the fixture.
///
/// The room and the camera are [`Scene::Probes`]', so the eye looks straight
/// down at the plane `y = 0` and every measured pixel is floor facing `+Y`.
/// One normal is one prediction: `sky_irradiance` reads the normal and nothing
/// else, so this floor is uniform and a band may be read anywhere on it.
///
/// That one normal is also the whole of what this pose cannot say, and
/// [`sky_ambient_wall_forward`] is the pose that can: `+Y` multiplies the
/// row's two horizontal lanes by zero, so no band of *this* frame observes
/// them.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built.
pub fn sky_ambient_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    atmosphere: Option<crcbl_render::Atmosphere>,
) -> Result<ForwardScene, OffscreenError> {
    let mut renderer =
        ForwardRenderer::with_scene(device, queue, format, &crate::render::scene::demo())?;
    // `Scene::Probes`' refusal and its reason: the measured pixels are a
    // diffuse ambient term predicted absolutely, and a rough surface's
    // environment specular would be an unmodelled second term in every one of
    // them.
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none().force(RenderEffects::REFLECTIONS, Some(false)),
        ..EffectRequest::default()
    });
    renderer.set_atmosphere(atmosphere);
    place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, probe_room());
    Ok(ForwardScene {
        camera: probe_camera(),
        sun: probe_sun(),
        renderer: Box::new(renderer),
    })
}

/// The face of `crcbl_shaders::mesh::OPEN_BOX_FACES` that fills
/// [`sky_ambient_wall_forward`]'s frame.
///
/// **Public because the test predicts the frame from it**, on [`probe_grid`]'s
/// terms: the band's albedo and its normal are both this face's own, and a
/// test that named a wall of its own would be predicting a surface the frame
/// does not draw.
pub const SKY_AMBIENT_WALL_FACE: &str = "-Z wall";

/// How far in front of [`SKY_AMBIENT_WALL_FACE`] [`sky_ambient_wall_camera`]
/// stands, in world units.
///
/// **Close enough that the frame is nothing but that wall.** The frame's short
/// half-axis covers this distance times the tangent of half [`probe_camera`]'s
/// field of view, which is under the wall's own half-height — so the floor
/// below it and the room's open top above it are both outside the frustum, and
/// every pixel of the frame carries the wall's one normal. The long axis is the
/// short one times the frame's aspect and stays inside the wall's half-width
/// for any aspect under the room's own width-to-height ratio.
///
/// Far enough that the bands `tests/render_e2e.rs` reads clear every edge of
/// the wall by more than `crcbl_render::ForwardRenderer`'s occlusion radius,
/// which is that file's `PROBE_BAND_AT` condition on the floor bands exactly:
/// a band close enough to an edge to be darkened would be measuring
/// `ssao.slang` instead of the sky.
const SKY_AMBIENT_WALL_BACK: f32 = 1.2;

/// Where [`sky_ambient_wall_forward`] is seen from: level with the middle of
/// [`SKY_AMBIENT_WALL_FACE`] and facing it square on.
///
/// [`probe_camera`]'s own projection, spread from it rather than written out
/// again, so the two poses differ in where the eye stands and in nothing else.
fn sky_ambient_wall_camera() -> Camera {
    let middle = glam::Vec3::new(0.0, 0.5 * PROBE_ROOM_HEIGHT, -0.5 * PROBE_ROOM_DEPTH);
    Camera {
        eye: middle + glam::Vec3::Z * SKY_AMBIENT_WALL_BACK,
        target: middle,
        up: glam::Vec3::Y,
        ..probe_camera()
    }
}

/// [`sky_ambient_forward`] from the pose that fills the frame with a **wall**:
/// the same room, the same refusals and the same colourless sun, seen from
/// `sky_ambient_wall_camera`.
///
/// **The one thing the floor pose cannot say.** `mesh.slang`'s
/// `sky_irradiance` is `dot(sh, float4(N, 1))`, and a normal of `+Y`
/// multiplies the row's `x` and `z` lanes by zero — so every band
/// [`sky_ambient_forward`]'s frame offers reads the `y` lane and the constant
/// band and nothing else, whatever the other two hold. This wall's normal is
/// `+Z`, so a band on it is the first measurement in the tree that a wrong
/// horizontal lane can move.
///
/// The frame is that one face throughout, so a band may be read anywhere in it
/// for [`sky_ambient_forward`]'s reason: one normal is one prediction.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built.
pub fn sky_ambient_wall_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    atmosphere: Option<crcbl_render::Atmosphere>,
) -> Result<ForwardScene, OffscreenError> {
    Ok(ForwardScene {
        camera: sky_ambient_wall_camera(),
        ..sky_ambient_forward(device, queue, format, atmosphere)?
    })
}

/// [`ssr_forward`] with its floor **fully rough**: the same scene through a
/// fourth material row, `DEMO_TINTED` at a roughness of one.
///
/// A floor the march skips — one is past `ssr.slang`'s cutoff — takes the
/// environment fallback alone, and at that roughness the fallback reads the sky
/// through `crcbl_shaders::sky_prefilter`'s roughest row rather than along the
/// one mirror direction. `tests/render_e2e.rs` holds the two floors against
/// each other under a sky lit only *below* the horizon: the mirror floor's
/// upward rays see none of it, and the rough floor's lobe does.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built.
pub fn ssr_rough_floor_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    sky: crcbl_render::Sky,
    effects: crcbl_render::RenderEffects,
) -> Result<ForwardScene, OffscreenError> {
    let mut scene = crate::render::scene::demo();
    let rough = scene.materials.len();
    scene.materials.push(crate::shaders::mesh::GpuMaterial {
        roughness: 1.0,
        ..scene.materials[DEMO_TINTED]
    });
    ssr_forward_on(device, queue, format, sky, effects, &scene, rough)
}

/// The ssr scene over `scene`, with its floor shaded through material row
/// `floor`: [`ssr_forward`] and [`ssr_rough_floor_forward`] differ in nothing
/// else.
fn ssr_forward_on(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    sky: crcbl_render::Sky,
    effects: crcbl_render::RenderEffects,
    scene: &crate::render::scene::SceneDesc<'_>,
    floor: usize,
) -> Result<ForwardScene, OffscreenError> {
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, scene)?;
    renderer.set_effect_request(EffectRequest {
        camera: effects,
        ..EffectRequest::default()
    });
    renderer.set_sky(sky);
    place(&mut renderer, DEMO_CUBE, floor, spot_floor());
    place(&mut renderer, DEMO_PYRAMID, DEMO_UNTINTED, ssr_pyramid());
    Ok(ForwardScene {
        camera: ssr_camera(),
        sun: ssr_sun(),
        renderer: Box::new(renderer),
    })
}

/// How much [`Scene::Ssr`] scales the pyramid by.
///
/// Large enough that its reflection is tens of pixels across at the golden
/// suite's 256×192 — the reflected image is foreshortened to a fraction of the
/// object's own height, so a pyramid the size the cube scene draws would leave a
/// smear a band could not be centred in. Small enough that the whole of it, and
/// the floor in front of it that carries the reflection, are both in frame.
const SSR_PYRAMID_SCALE: f32 = 1.2;

/// How far above the floor [`Scene::Ssr`]'s camera stands, in world units.
///
/// **Low, and that is the scene.** The reflectance of a dielectric is a Fresnel
/// term: about a twenty-fifth head-on and rising steeply towards grazing, so a
/// floor seen from overhead reflects almost nothing whatever the march does. From
/// here the floor in front of the pyramid is seen at about fifteen degrees, where
/// the term is several times its head-on value and the reflection is a thing a
/// band can measure.
///
/// It is also what puts the reflection *in frame*: a reflected ray leaves the
/// floor at the same angle it arrived, so a low eye is a shallow ray that runs a
/// long way up the frame before it reaches the object — and a march that runs off
/// the top of the screen finds nothing.
const SSR_CAMERA_UP: f32 = 0.8;

/// How far back along `+Z` [`Scene::Ssr`]'s camera stands.
///
/// Far enough that the pyramid at the origin is comfortably inside the frame with
/// floor visible on both sides of it — the bands this scene compares are floor,
/// and two of the three are beside the reflection rather than in it.
const SSR_CAMERA_BACK: f32 = 3.2;

/// How high [`Scene::Ssr`]'s camera looks.
///
/// Below [`SSR_CAMERA_UP`], so the view tilts down and the floor fills the lower
/// half of the frame; above the floor, so the pyramid is not pinned to the top
/// edge. The reflection lands between the pyramid's base and the bottom of the
/// frame, which is the band the assertions read.
const SSR_CAMERA_LOOK: f32 = 0.35;

/// [`Scene::Ssr`]'s pyramid: scaled and dropped so its base sits exactly on the
/// floor.
///
/// **On the floor rather than floating**, on `spot_shadow_caster`'s terms: the
/// reflection of an object standing on a mirror meets the object at the contact
/// line, and a gap there is the first thing wrong with a march that has its
/// start point or its normal offset wrong.
fn ssr_pyramid() -> glam::Mat4 {
    // The pyramid's base is at `-0.4` in its own space, so lifting it by that
    // much of the scale puts the base on `y = 0`.
    glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.4 * SSR_PYRAMID_SCALE, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::splat(SSR_PYRAMID_SCALE))
}

/// The camera [`Scene::Ssr`] is drawn with: low, on the `+Z` axis, looking
/// slightly down at the pyramid.
///
/// **On the axis, and every band rests on that.** With the eye at `x = 0` and the
/// look direction in the plane `x = 0`, the frame is symmetric about its own
/// centre column: two floor points at `±x` on one row are the same distance from
/// the eye, carry the same normal, take the same directional light and lie in the
/// same shadow. So the only thing that can separate the middle of that row from
/// its two ends is the reflection, which is what [`ssr_sun`] finishes the
/// argument for.
fn ssr_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, SSR_CAMERA_UP, SSR_CAMERA_BACK),
        target: glam::Vec3::new(0.0, SSR_CAMERA_LOOK, 0.0),
        up: glam::Vec3::Y,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// The sun [`Scene::Ssr`] runs under: the default one with its **X component
/// removed**.
///
/// Both halves are load-bearing.
///
/// * **No X**, so the whole frame is symmetric about the plane `x = 0`: the
///   Lambert term, the specular lobe and the shadow the pyramid throws are all
///   the same at `+x` and `-x`. The default sun comes over the viewer's right
///   shoulder, which would make the two side bands different by the lighting and
///   leave the assertion measuring that instead.
/// * **Still from behind the eye and above**, so the pyramid's shadow falls
///   *away* from the camera and the reflection — which is on the floor between
///   the pyramid and the eye — lands on lit floor. A sun in front would lay the
///   shadow across exactly the band this scene measures.
///
/// The colour and the ambient are the default's, unlike every other scene here:
/// this one's claim is a ratio between three bands of lit floor, so there is
/// nothing to turn down.
fn ssr_sun() -> crcbl_render::DirectionalLight {
    let bright = crcbl_render::DirectionalLight::default();
    crcbl_render::DirectionalLight {
        direction: glam::Vec3::new(0.0, bright.direction.y, bright.direction.z).normalize(),
        ..bright
    }
}

/// How wide the narrow interpolation interval in [`Scene::Probes`] is, in world
/// units.
///
/// Most of the floor lies outside this interval and clamps to one of the two
/// probes, forming flat regions. The interval itself still exercises coefficient
/// interpolation and is checked against the Rust mirror.
const PROBE_BLEND_WIDTH: f32 = 0.1;

/// How wide [`Scene::Probes`]' room is, in world units — the axis its probe grid
/// runs along.
///
/// Wide enough that the `±X` walls stay outside the frame: WARP's arithmetic
/// disagreement was local to their thin, oblique strips rather than this scene's
/// flat floor measurement. The `±Z` walls remain visible, so the room still has
/// context, while every visible floor sample is a broad flat-facing region.
///
/// A unit clear of either wall is still well inside the frame, which is the whole
/// of what this number buys: occlusion scales the probe term exactly as it scales
/// the flat ambient, and a band close enough to a wall to be darkened would be
/// measuring `ssao.slang` instead. A unit is twice
/// `crcbl_render::ForwardRenderer`'s occlusion radius, so the occlusion there is
/// not merely near one, it is one.
const PROBE_ROOM_WIDTH: f32 = 3.6;

/// How deep the room is, across the grid's axis.
///
/// Narrower than [`PROBE_ROOM_WIDTH`] so both of the `±Z` walls are inside the
/// frame `probe_camera` looks through — the room reads as a room rather than as
/// an unbounded plane — and still a full unit from the bands, which sit on the
/// frame's `z = 0` axis.
const PROBE_ROOM_DEPTH: f32 = 2.0;

/// How tall its walls are.
///
/// [`AO_WALL`]'s height, and nothing measured here depends on it: the probe
/// field is a function of `x` alone, the walls carry no part of the linear band
/// because their normals are horizontal, and they are context rather than
/// measurement.
const PROBE_ROOM_HEIGHT: f32 = 2.0;

/// How far above the floor [`Scene::Probes`]' camera stands, in world units.
///
/// `AO_CAMERA_UP`'s height, and public for the reason [`Scene::Probes`] gives:
/// `tests/render_e2e.rs` has to turn a pixel back into the world position the
/// fragment stage shaded there before it can evaluate the Rust mirror at it, and
/// that inversion is this number and the field of view. Written down once here
/// rather than twice, because the two copies would agree until somebody moved
/// the camera.
pub const PROBE_CAMERA_UP: f32 = 2.2;

/// The radiance of each of the two coloured sources `probe_grid` projects, in
/// linear RGB.
///
/// Chosen so the *brightest* floor pixel in the frame stays under what the
/// swapchain holds.
///
/// A source of radiance `L` covering a fraction `Ω/4π` of the sphere peaks at
/// `(Â₀ + 3Â₁)·Ω/(4π)` times `L` on a surface facing it — the two transfer
/// coefficients are [`crcbl_shaders::probe`](crate::shaders::probe)'s, and the
/// surface is the floor face of `crcbl_shaders::mesh::OPEN_BOX_FACES`, whose
/// albedo scales the product. A source bright enough for that to reach one would
/// flatten a lit region, and `tonemap.slang`'s `saturate` is where that would
/// happen without saying so.
const PROBE_RADIANCE: f32 = 0.7;

/// The radiance arriving from the four horizontal directions, in linear RGB.
///
/// **A dim neutral surround, and it is not decoration.** The two sources are
/// pure red and pure blue, so without this the green channel of every pixel in
/// the frame would be exactly zero and the room would be a two-colour gradient
/// rather than a lit room — which is a poorer picture for a reviewer and one
/// fewer channel for the mirror comparison to read.
///
/// **Deliberately small**, because all it can reach is the constant band: it is
/// identical in both probes, so it adds the same amount to both of the bands
/// `tests/render_e2e.rs` compares and pulls the ratio between them towards one.
/// The measurement gets weaker as this grows, and buys nothing but a lighter
/// room.
const PROBE_SURROUND: f32 = 0.035;

/// [`Scene::Probes`]' room: the open box scaled to [`PROBE_ROOM_WIDTH`] ×
/// [`PROBE_ROOM_HEIGHT`] × [`PROBE_ROOM_DEPTH`] and lifted so its floor is the
/// plane `y = 0`.
///
/// [`ao_box`]'s transform for [`ao_box`]'s reason, including why a non-uniform
/// scale is safe on this mesh alone. The floor being exactly `y = 0` is what
/// lets `tests/render_e2e.rs` name a band's world position without
/// reconstructing anything.
fn probe_room() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.5 * PROBE_ROOM_HEIGHT, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::new(
            PROBE_ROOM_WIDTH,
            PROBE_ROOM_HEIGHT,
            PROBE_ROOM_DEPTH,
        ))
}

/// The camera [`Scene::Probes`] is drawn with: straight down at the floor.
///
/// [`ao_camera`]'s view, and the band placement rests on it for that function's
/// reason and one of this scene's own: looking down `-Y` at a plane `y = 0` maps
/// world to pixels **linearly**, so the inverse `tests/render_e2e.rs` needs to
/// evaluate the mirror at a pixel is a division rather than an unprojection.
fn probe_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, PROBE_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// The sun [`Scene::Probes`] runs under with its direct and flat ambient terms
/// exactly zero, leaving the probe grid as the only lighting contribution.
fn probe_sun() -> crcbl_render::DirectionalLight {
    dimmed_sun(0.0, 0.0)
}

/// [`Scene::Probes`]' irradiance volume: two probes along `x`, at the endpoints
/// of the narrow central blend interval.
///
/// **Public so `tests/render_e2e.rs` evaluates the Rust mirror over the rows the
/// device was actually given**, rather than over a second copy of them written
/// out beside the assertion. A copy is a thing that can drift, and the whole
/// point of that comparison is that one set of coefficients went two ways.
///
/// # The two probes differ in *where their light comes from* and in nothing else
///
/// Each one is the same six-direction environment: a coloured source overhead, a
/// coloured source underfoot, and a dim neutral surround from each of the four
/// horizontal directions. What the two swap is which colour is which — red is
/// overhead at the `-X` end and underfoot at the `+X` end, blue the other way
/// round.
///
/// That swap is the design of the fixture, and the alternative is what it is
/// avoiding. Two probes differing in how *much* light they hold would light the
/// two ends of the floor differently through their **constant** band alone, and
/// then a shader that evaluated `sh.w` and dropped the three dot products
/// entirely would draw the same gradient and pass. Swapping the poles leaves
/// every constant band identical between the two rows — each holds one source's
/// worth of red and one of blue whichever way up they are — so the only thing
/// that can separate the two ends of the floor is `dot(sh, float4(N, 1))`'s
/// linear half. Zero the linear coefficients and the frame goes flat.
///
/// A field like this is not a room anybody has stood in, and that is the same
/// bargain `ao_sun` makes by pointing a sun straight up: the scene is built so
/// exactly one term can move the measurement.
///
/// # Why a grid of two and not one probe
///
/// A `1×1×1` volume never addresses a row past the first, never interpolates and
/// never exercises the `x`-fastest index — it is the degenerate volume with one
/// row filled in. This two-probe grid confines interpolation to
/// `PROBE_BLEND_WIDTH` at the room's centre: the broad regions outside it clamp
/// to either probe, while the centre still detects a lookup that ignores world
/// position or reads the wrong axis.
#[must_use]
pub fn probe_grid() -> crate::render::scene::ProbeGrid {
    // Six directions of equal solid angle, which is the coarsest partition of
    // the sphere that can hold a source on each pole and a surround around the
    // middle. `accumulate` is the only correct way to fill a row — the band
    // scales and the basis normalisations are folded into it — so the fixture
    // authors an environment and projects it rather than writing coefficients.
    let solid_angle = 4.0 * std::f32::consts::PI / 6.0;
    let end = |overhead: [f32; 3], underfoot: [f32; 3]| {
        let mut probe = crate::shaders::probe::GpuProbe::ZERO;
        probe.accumulate([0.0, 1.0, 0.0], overhead, solid_angle);
        probe.accumulate([0.0, -1.0, 0.0], underfoot, solid_angle);
        for horizontal in [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ] {
            probe.accumulate(horizontal, [PROBE_SURROUND; 3], solid_angle);
        }
        probe
    };
    let red = [PROBE_RADIANCE, 0.0, 0.0];
    let blue = [0.0, 0.0, PROBE_RADIANCE];
    crate::render::scene::ProbeGrid {
        update: ProbeUpdate::Authored,
        volume: crate::shaders::probe::ProbeVolume {
            // The probes are close together at the room's centre. Everything
            // beyond their interval clamps to an endpoint, producing broad flat
            // regions while the central interval still blends both rows.
            origin: [-0.5 * PROBE_BLEND_WIDTH, 0.5 * PROBE_ROOM_HEIGHT, 0.0],
            inv_spacing: [1.0 / PROBE_BLEND_WIDTH, 0.0, 0.0],
            counts: [2, 1, 1],
            // **One level**, so this fixture and its golden are the uniform
            // grid they were before the clipmap: a single level clamps at its
            // own edge and blends towards nothing.
            levels: 1,
            steps: crate::shaders::probe::ProbeSteps::default(),
        },
        probes: vec![end(red, blue), end(blue, red)],
    }
}

// ---------------------------------------------------------------------------
// The probe-leak fixture
// ---------------------------------------------------------------------------

/// How wide [`probe_leak_forward`]'s room is, in world units.
///
/// Wide enough that its `±X` walls stay well outside the frame, so the only
/// vertical surface in the picture is the divider — and so the divider is the
/// only thing that can be between a probe and a band.
const LEAK_ROOM_WIDTH: f32 = 6.0;

/// How deep it is. [`PROBE_ROOM_DEPTH`], for that constant's reason: both `±Z`
/// walls stay in frame, so the room reads as a room.
const LEAK_ROOM_DEPTH: f32 = PROBE_ROOM_DEPTH;

/// How tall it is, and therefore how tall the divider is.
///
/// [`PROBE_ROOM_HEIGHT`]. The divider is the room's full height, so a probe
/// cannot see over it — a shorter wall would leave a path from the far probe to
/// the near floor, and the fixture would be measuring the wall's height rather
/// than the visibility test.
const LEAK_ROOM_HEIGHT: f32 = PROBE_ROOM_HEIGHT;

/// How thick the divider is.
///
/// Thin enough to cover almost none of the floor the camera sees, and thick
/// enough to be several times a probe map's angular resolution at this range —
/// a divider a ray could pass through diagonally would occlude some directions
/// and not others, and the fixture would read as a partial drop.
const LEAK_WALL_THICKNESS: f32 = 0.1;

/// How far either side of the divider the two probes stand.
///
/// It is also the grid's spacing, so the fraction the trilinear blend gives a
/// band is that band's own distance from a probe over this — which is what
/// `tests/render_e2e.rs` computes its predictions from.
const LEAK_PROBE_REACH: f32 = 1.5;

/// The radiance of the constant environment the `+X` probe holds.
///
/// Chosen so the brightest band stays under what the swapchain holds:
/// [`PROBE_RADIANCE`]'s reasoning with a constant environment in place of two
/// poles, where the peak is `π · L` times the floor's albedo times the weight
/// the blend gives that probe. **The band that decides it is the `+X` one with
/// the wall in place**, which takes the whole of that probe rather than three
/// quarters of it — the first value tried put both `+X` bands over one, and
/// `tonemap.slang`'s `saturate` then made the gain this fixture measures a
/// tenth of a level instead of tens.
const LEAK_RADIANCE: f32 = 0.35;

/// How far above the floor [`probe_leak_forward`]'s camera stands.
///
/// [`PROBE_CAMERA_UP`], so the frame maps world to pixels exactly as
/// [`Scene::Probes`]' does and `tests/render_e2e.rs` reuses that inverse rather
/// than deriving a second one.
pub const LEAK_CAMERA_UP: f32 = PROBE_CAMERA_UP;

/// Where the two bands are measured, in world units either side of the divider.
///
/// Half of `LEAK_PROBE_REACH`, so the trilinear blend gives the near probe
/// three quarters and the far one a quarter — both bands read *both* probes,
/// which is what the measurement needs. It is also well outside
/// `crcbl_render::ForwardRenderer`'s occlusion radius from the divider, so the
/// ambient-occlusion pass is not what moves the band.
pub const LEAK_BAND_AT: f32 = 0.75;

/// [`probe_leak_forward`]'s room: the open box at [`LEAK_ROOM_WIDTH`] ×
/// [`LEAK_ROOM_HEIGHT`] × [`LEAK_ROOM_DEPTH`], floor on the plane `y = 0`.
fn leak_room() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.5 * LEAK_ROOM_HEIGHT, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::new(
            LEAK_ROOM_WIDTH,
            LEAK_ROOM_HEIGHT,
            LEAK_ROOM_DEPTH,
        ))
}

/// The divider: the demo cube squashed into a slab on the plane `x = 0`, the
/// room's full height and depth.
fn leak_wall() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.5 * LEAK_ROOM_HEIGHT, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::new(
            LEAK_WALL_THICKNESS,
            LEAK_ROOM_HEIGHT,
            LEAK_ROOM_DEPTH,
        ))
}

/// [`probe_leak_forward`]'s volume: two probes on the `x` axis, one either side
/// of the divider, at half the room's height.
///
/// **The `-X` probe is black and the `+X` probe is a constant environment**, so
/// the only light in the frame is the one on the far side of the wall from the
/// `-X` band. That asymmetry is the fixture: with the wall in place the `-X`
/// band must lose it, and the `+X` band — which the same wall cuts off from the
/// *black* probe — must gain.
///
/// Public so `tests/render_e2e.rs` predicts the frame from the rows the device
/// was actually given, on [`probe_grid`]'s terms.
#[must_use]
pub fn probe_leak_grid() -> crate::render::scene::ProbeGrid {
    // A constant environment, projected the only correct way — see
    // `probe_grid`, which argues why a fixture authors an environment and
    // projects it rather than writing coefficients.
    let solid_angle = 4.0 * std::f32::consts::PI / 6.0;
    let mut lit = crate::shaders::probe::GpuProbe::ZERO;
    for direction in [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ] {
        lit.accumulate(direction, [LEAK_RADIANCE; 3], solid_angle);
    }
    crate::render::scene::ProbeGrid {
        update: ProbeUpdate::Authored,
        volume: leak_volume(),
        probes: vec![crate::shaders::probe::GpuProbe::ZERO, lit],
    }
}

/// The volume both divider fixtures place their two probes in: one either side
/// of the wall, at half the room's height.
///
/// One function because the two fixtures are the same room and the same wall —
/// [`probe_leak_grid`] authors its rows and [`probe_bounce_grid`] has the
/// updater fill them — so a probe that moved in one and not the other would put
/// the wall in a different place in each.
///
/// One level, on `probe_grid`'s terms: the claim both fixtures make is about a
/// wall, and a second level would put a second read between the wall and the
/// band.
fn leak_volume() -> crate::shaders::probe::ProbeVolume {
    crate::shaders::probe::ProbeVolume {
        origin: [-LEAK_PROBE_REACH, 0.5 * LEAK_ROOM_HEIGHT, 0.0],
        inv_spacing: [1.0 / (2.0 * LEAK_PROBE_REACH), 0.0, 0.0],
        counts: [2, 1, 1],
        levels: 1,
        steps: crate::shaders::probe::ProbeSteps::default(),
    }
}

/// The camera it is drawn with: straight down at the floor, on
/// [`probe_camera`]'s terms exactly.
fn leak_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, LEAK_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// **The leak fixture**: one room, two probes, and a wall between them that is
/// there or is not.
///
/// `docs/plan/50-irradiance-probes.md`'s rung is that a probe on the far side of
/// a wall from a surface contributes nothing to it. Nothing in a single frame
/// says whether that happened — a room lit by a probe grid looks like a room —
/// so the claim is a comparison of one scene against itself with the wall taken
/// away, which is what makes `wall` a parameter here rather than a second
/// [`Scene`]. [`aa_forward`] and [`ssr_forward`] are public for the same reason.
///
/// Every band the comparison reads is the probe term and nothing else: the sun's
/// direct and flat ambient terms are zero — `probe_sun` — the floor is one
/// flat quad of one albedo, and the reflection pair is refused so no specular
/// can reach it.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built or the capture cannot
/// be uploaded.
pub fn probe_leak_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    wall: bool,
) -> Result<ForwardScene, OffscreenError> {
    let probes = probe_leak_grid();
    let mut scene = crate::render::scene::demo();
    scene.capacities.probes = probes.volume.total();
    scene.probes = probes;
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    // On `Scene::Probes`' terms: the measured pixels are diffuse probe
    // irradiance, and a rough surface's reflection would put specular into them.
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(crcbl_render::RenderEffects::REFLECTIONS, Some(false)),
        ..EffectRequest::default()
    });
    place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, leak_room());
    if wall {
        place(&mut renderer, DEMO_CUBE, DEMO_UNTINTED, leak_wall());
    }
    // **After the geometry and before the frame**, which is the whole shape of
    // the capture — see `ForwardRenderer::capture_probe_visibility`. The two
    // arms of this fixture differ in exactly what this call then records.
    renderer.capture_probe_visibility(device, queue)?;
    Ok(ForwardScene {
        camera: leak_camera(),
        sun: probe_sun(),
        renderer: Box::new(renderer),
    })
}

/// The roughness [`probe_leak_reflection_forward`]'s room is shaded at.
///
/// Zero, so `ssr.slang`'s `sharpness_of` gives the march its whole weight:
/// `GpuMaterial::UNTINTED` sits at that shader's `ROUGHNESS_CUTOFF` exactly and
/// derives no reflection at all, which is why this fixture appends a row rather
/// than reusing the one [`probe_leak_forward`] draws with.
const LEAK_MIRROR_ROUGHNESS: f32 = 0.0;

/// The metalness it is shaded at.
///
/// One, and it does two things this fixture needs. A metal has no diffuse lobe,
/// so the probe grid's *diffuse* term — which [`probe_leak_forward`] measures —
/// is gone from these pixels and cannot be mistaken for the specular one; and
/// its `F0` is its base colour rather than a dielectric's 4%, so what the
/// reflection carries is most of the environment it gathered rather than a
/// twenty-fifth of it.
const LEAK_MIRROR_METALLIC: f32 = 1.0;

/// **The leak fixture through a mirror**: [`probe_leak_forward`]'s room, probes
/// and wall, with the room shaded as a mirror and the reflection pair allowed.
///
/// `docs/plan/50-irradiance-probes.md`'s rung reaches the specular path here.
/// `ssr.slang` marches the depth buffer, finds nothing along a ray that leaves
/// this floor upward and outward, and falls back to the same eight probe rows
/// `mesh.slang`'s diffuse gather reads — so a probe on the far side of the
/// divider must contribute nothing to the reflection either.
///
/// `effects` is the caller's rather than forced, because the claim is a
/// comparison: `tests/render_e2e.rs` draws each arm with the reflection pair and
/// again without it, and the difference is the pass's own output and nothing
/// else. [`ssr_forward`] takes its effect set for the same reason.
///
/// **The sky is refused outright.** The fallback is the probe grid *plus* the
/// prefiltered sky, and a sky would add the same term to every arm — which a
/// ratio between two arms is not free to ignore.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built or the capture cannot
/// be uploaded.
pub fn probe_leak_reflection_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    wall: bool,
    effects: crcbl_render::RenderEffects,
) -> Result<ForwardScene, OffscreenError> {
    let probes = probe_leak_grid();
    let mut scene = crate::render::scene::demo();
    scene.capacities.probes = probes.volume.total();
    scene.probes = probes;
    // Appended rather than edited in place, on `ssr_rough_floor_forward`'s
    // terms: every other row keeps the value every other fixture draws with.
    let mirror = scene.materials.len();
    scene.materials.push(crate::shaders::mesh::GpuMaterial {
        metallic: LEAK_MIRROR_METALLIC,
        roughness: LEAK_MIRROR_ROUGHNESS,
        ..scene.materials[DEMO_UNTINTED]
    });
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    renderer.set_effect_request(EffectRequest {
        camera: effects,
        ..EffectRequest::default()
    });
    renderer.set_sky(crcbl_render::Sky::NONE);
    place(&mut renderer, DEMO_OPEN_BOX, mirror, leak_room());
    if wall {
        // The divider through the ordinary row: it is what stands between a
        // probe and a band, and a mirror one would put the room's own walls into
        // the pixels either side of it.
        place(&mut renderer, DEMO_CUBE, DEMO_UNTINTED, leak_wall());
    }
    // [`probe_leak_forward`]'s call, in the same place and for the same reason.
    renderer.capture_probe_visibility(device, queue)?;
    Ok(ForwardScene {
        camera: leak_camera(),
        sun: probe_sun(),
        renderer: Box::new(renderer),
    })
}

// ---------------------------------------------------------------------------
// The updater fixture
// ---------------------------------------------------------------------------

/// How thick the panel [`probe_bounce_forward`] stands against its `+X` wall
/// is.
///
/// [`BLOOM_EMITTER_THICKNESS`]'s reasoning stood on end: thin enough that its
/// own ambient occlusion darkens no pixel a band is measured in, thick enough
/// that the depth buffer resolves it against the wall behind it rather than
/// z-fighting with it.
const BOUNCE_PANEL_THICKNESS: f32 = 0.02;

/// How far out from the room's centre the panel stands, in world units.
///
/// Against the `+X` wall — half of [`LEAK_ROOM_WIDTH`], less half the panel's
/// own thickness, so the two surfaces touch and neither pokes through the other.
const BOUNCE_PANEL_AT: f32 = 0.5 * LEAK_ROOM_WIDTH - 0.5 * BOUNCE_PANEL_THICKNESS;

/// The base-colour factor the panel reflects the sun with.
///
/// **Red and above one**, on [`BLOOM_EMITTER_GAIN`]'s terms: `base_color` is a
/// linear multiplier into the vertex albedo, so this is how the fixture gets one
/// surface whose bounce is unmistakably its own without inventing an emissive
/// term. Red because the measurement is a channel difference — the sun is white
/// and its direct term lands in all three channels together, so a red channel
/// that moved and a blue one that did not is the panel's flux and nothing else.
const BOUNCE_PANEL_GAIN: f32 = 40.0;

/// The base colour the divider is drawn with in this fixture.
///
/// **Nearly black, and that is what makes the measurement a difference of
/// one thing.** The sun comes from `-X`, so the divider's own `-X` face is lit
/// by it — and a white face there is a second thing the arms differ by, since
/// taking the divider away takes that bounce with it. At this albedo the divider
/// occludes and reflects almost nothing, so what the `-X` probe loses to it is
/// the panel across the room and little else.
const BOUNCE_DIVIDER_ALBEDO: f32 = 0.02;

/// The panel: the demo cube flattened into a slab and stood against the `+X`
/// wall, the room's full height and depth.
///
/// **Vertical, and that is the whole fixture.** The surface a probe gathers
/// lights the surfaces its own flux travels *towards*, so a red patch on the
/// floor would light the ceiling and leave the floor — where the bands are
/// measured, and where the camera looks — exactly as it found it. A wall lit by
/// a low sun throws its bounce sideways and down, onto the floor either side of
/// the divider.
fn bounce_panel() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(
        BOUNCE_PANEL_AT,
        0.5 * LEAK_ROOM_HEIGHT,
        0.0,
    )) * glam::Mat4::from_scale(glam::Vec3::new(
        BOUNCE_PANEL_THICKNESS,
        LEAK_ROOM_HEIGHT,
        LEAK_ROOM_DEPTH,
    ))
}

/// The sun [`probe_bounce_forward`] runs under: **low, from `-X`, and the only
/// light in the room**.
///
/// * **From `-X` at forty-five degrees** — it has to reach the panel's inner
///   face, which a sun straight overhead never does, and it has to leave the
///   `-X` band where the measurement is taken lit identically in both arms. Both
///   follow from the direction: the divider's shadow falls on its `+X` side,
///   away from that band, and the panel stands in the light rather than in that
///   shadow.
/// * **No ambient at all** — the flat term is the same in both arms, so it
///   cannot fake the difference, but it is white and it dilutes the channel
///   difference the measurement reads. Zero leaves the direct sun and the
///   updater's bounce as the only two things in a pixel.
fn bounce_sun() -> crcbl_render::DirectionalLight {
    crcbl_render::DirectionalLight {
        direction: glam::Vec3::new(-1.0, 1.0, 0.0).normalize(),
        ..dimmed_sun(0.30, 0.0)
    }
}

/// How far out from the `-X` wall [`probe_bounce_forward`]'s camera stands, in
/// world units.
///
/// Close enough that the wall's inner face fills the frame's middle — the
/// measurement is a block at the centre and nothing else, so there is no
/// projection to invert — and far enough out that the near plane is nowhere near
/// it and the `-X` probe stands between the two rather than behind the eye.
const BOUNCE_CAMERA_OUT: f32 = 2.2;

/// The camera it is drawn with: level, from inside the `-X` half, looking
/// straight at that wall's inner face.
///
/// **Not the top-down camera every other probe fixture uses**, and the reason is
/// the measurement. A surface is lit by the flux arriving *at* its own normal,
/// so the floor — which is what a top-down camera sees — reads the hemisphere
/// above it, where a room's bounce has already been counted into the probe's
/// downward lobe and cancels. The `-X` wall's inner face looks the way the
/// panel's flux travels, and the sun never touches it: with the light at `-X`
/// and above, that face is turned away from it. So every level in the block this
/// fixture measures is the probe term.
fn bounce_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(
            BOUNCE_CAMERA_OUT - 0.5 * LEAK_ROOM_WIDTH,
            0.5 * LEAK_ROOM_HEIGHT,
            0.0,
        ),
        target: glam::Vec3::new(-0.5 * LEAK_ROOM_WIDTH, 0.5 * LEAK_ROOM_HEIGHT, 0.0),
        up: glam::Vec3::Y,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// **The updater fixture's volume**: `leak_volume`'s, with every row at zero and
/// [`ProbeUpdate::EveryFrame`] on it.
///
/// The rows are zero because they are the updater's to fill — an authored row
/// here would be a value the frame could show whether the reflective shadow map
/// ran or not. Public so `tests/render_e2e.rs` can reserve the volume's own
/// capacity and read its count rather than writing a second copy of either.
#[must_use]
pub fn probe_bounce_grid() -> crate::render::scene::ProbeGrid {
    let volume = leak_volume();
    let probes = vec![crate::shaders::probe::GpuProbe::ZERO; volume.total() as usize];
    crate::render::scene::ProbeGrid {
        update: ProbeUpdate::EveryFrame,
        volume,
        probes,
    }
}

/// **The updater fixture**: a sunlit red slab on one side of a divider, two
/// probes the reflective shadow map fills every frame, and a wall between them
/// that is there or is not.
///
/// `docs/plan/50-irradiance-probes.md`'s raster updater, measured the way
/// [`probe_leak_forward`] measures the authored rows: one scene against itself
/// with the divider taken away, because nothing in a single frame says whether a
/// probe gathered flux it should not have.
///
/// **What separates the two arms is one channel.** The sun is white, points
/// straight down and casts no shadow from a vertical wall, so the direct term at
/// a band on the `-X` floor is the same in both arms. The only red in the room
/// is the slab's, it is on the `+X` floor, and the `-X` probe can gather it only
/// when nothing stands between them. So the `-X` band's red channel rises when
/// the wall goes and its blue channel does not — which is the shape no uniform
/// change can fake, and the shape a gather that ignored
/// `probe_chebyshev` would destroy by lighting both arms alike.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built or the capture cannot
/// be uploaded.
pub fn probe_bounce_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    wall: bool,
) -> Result<ForwardScene, OffscreenError> {
    let probes = probe_bounce_grid();
    let mut scene = crate::render::scene::demo();
    scene.capacities.probes = probes.volume.total();
    scene.probes = probes;
    // Appended rather than edited in place, on `probe_leak_reflection_forward`'s
    // terms: every other row keeps the value every other fixture draws with.
    let panel = scene.materials.len();
    scene.materials.push(crate::shaders::mesh::GpuMaterial {
        base_color: [BOUNCE_PANEL_GAIN, 0.0, 0.0, 1.0],
        ..crate::shaders::mesh::GpuMaterial::UNTINTED
    });
    let divider = scene.materials.len();
    scene.materials.push(crate::shaders::mesh::GpuMaterial {
        base_color: [
            BOUNCE_DIVIDER_ALBEDO,
            BOUNCE_DIVIDER_ALBEDO,
            BOUNCE_DIVIDER_ALBEDO,
            1.0,
        ],
        ..crate::shaders::mesh::GpuMaterial::UNTINTED
    });
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    // On `probe_leak_forward`'s terms: the measured pixels are the direct sun
    // and the diffuse probe term, and a rough surface's reflection would put
    // specular into them.
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(crcbl_render::RenderEffects::REFLECTIONS, Some(false)),
        ..EffectRequest::default()
    });
    // No sky, for the same reason `bounce_sun` carries no ambient: the sky is a
    // second environment in the pixel, and this fixture is about the first one.
    renderer.set_sky(crcbl_render::Sky::NONE);
    place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, leak_room());
    place(&mut renderer, DEMO_CUBE, panel, bounce_panel());
    if wall {
        place(&mut renderer, DEMO_CUBE, divider, leak_wall());
    }
    // **After the geometry and before the frame**, which is the whole shape of
    // the capture — and here it is also what the gather reads: the two arms
    // differ in the visibility this call records, and the updater weighs every
    // map texel by it.
    renderer.capture_probe_visibility(device, queue)?;
    Ok(ForwardScene {
        camera: bounce_camera(),
        sun: bounce_sun(),
        renderer: Box::new(renderer),
    })
}

// ---------------------------------------------------------------------------
// The clipmap fixture
// ---------------------------------------------------------------------------

/// How far apart [`probe_clipmap_grid`]'s **level 0** probes stand, in world
/// units.
///
/// Three probes a level, so level 0 reaches this far either side of the room's
/// centre and level 1 reaches twice that. **What sets it is that every probe of
/// every level has to stand inside the room**: level 1's outermost pair is two
/// of these from the centre, and the walls are at half of
/// [`PROBE_ROOM_WIDTH`]. A probe standing in a wall would record that wall as
/// the nearest surface in every downward direction and the Chebyshev test would
/// take its light off the floor — which is the visibility rung working, and
/// would leave this fixture measuring it instead of the level pick.
const PROBE_CLIPMAP_SPACING: f32 = 0.8;

/// How many probes each of [`probe_clipmap_grid`]'s levels holds along `x`.
///
/// Three: two endpoints and a middle, which is the fewest that puts a probe at
/// the centre of each level. `y` and `z` hold one, so the field is a function
/// of `x` alone — [`probe_grid`]'s shape, for [`probe_grid`]'s reason.
const PROBE_CLIPMAP_COUNT: u32 = 3;

/// The radiance of the constant environment each of its levels holds, in linear
/// RGB.
///
/// [`LEAK_RADIANCE`]'s size and [`PROBE_RADIANCE`]'s reasoning: a constant
/// environment of radiance `L` reaches a surface as `π·L`, and the floor face's
/// albedo scales that — so the brightest floor pixel stays under what the
/// swapchain holds and `tonemap.slang`'s `saturate` never touches the
/// measurement.
const PROBE_CLIPMAP_RADIANCE: f32 = 0.35;

/// **The clipmap fixture's volume**: two levels of one grid, the fine one red
/// and the coarse one blue.
///
/// `docs/plan/50-irradiance-probes.md`'s layered density. The claim it exists
/// for is that a fragment reads the finest level containing it and *fades* into
/// the next one rather than switching — so the two levels are made as different
/// as two rows can be, and each level's rows are made identical to each other:
///
/// * **Every row of a level is the same constant environment**, so the
///   trilinear gather within a level is flat and the only thing that can move a
///   pixel along the floor is which level it read and in what share.
/// * **The two levels are different colours**, so a shader that picked the
///   wrong level draws the wrong colour rather than a slightly wrong
///   brightness. A constant environment has no linear band at all, which is
///   deliberate: [`Scene::Probes`] is where the spherical harmonic's linear
///   half is measured, and this fixture is about the level pick alone.
///
/// Public so `tests/render_e2e.rs` evaluates the Rust mirror over the rows the
/// device was actually given, on [`probe_grid`]'s terms exactly.
#[must_use]
pub fn probe_clipmap_grid() -> crate::render::scene::ProbeGrid {
    let solid_angle = 4.0 * std::f32::consts::PI / 6.0;
    let constant = |radiance: [f32; 3]| {
        let mut probe = crate::shaders::probe::GpuProbe::ZERO;
        for direction in [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ] {
            probe.accumulate(direction, radiance, solid_angle);
        }
        probe
    };
    let volume = crate::shaders::probe::ProbeVolume {
        origin: [-PROBE_CLIPMAP_SPACING, 0.5 * PROBE_ROOM_HEIGHT, 0.0],
        inv_spacing: [1.0 / PROBE_CLIPMAP_SPACING, 0.0, 0.0],
        counts: [PROBE_CLIPMAP_COUNT, 1, 1],
        levels: 2,
        steps: crate::shaders::probe::ProbeSteps::default(),
    };
    // The levels one after another, finest first — `ProbeGrid::probes` says so
    // and `ProbeVolume::level_row` is what indexes it.
    let mut probes = Vec::with_capacity(volume.total() as usize);
    for radiance in [
        [PROBE_CLIPMAP_RADIANCE, 0.0, 0.0],
        [0.0, 0.0, PROBE_CLIPMAP_RADIANCE],
    ] {
        for _ in 0..volume.per_level() {
            probes.push(constant(radiance));
        }
    }
    crate::render::scene::ProbeGrid {
        volume,
        probes,
        update: ProbeUpdate::Authored,
    }
}

/// **The clipmap fixture**: [`Scene::Probes`]' room and camera over a volume of
/// two levels.
///
/// Public rather than a [`Scene`] for [`probe_leak_forward`]'s reason: what it
/// measures is a *profile across one frame* — the floor's colour along `x` as
/// the read crosses level 0's edge — which no golden of a single image says
/// anything about, and which `tests/render_e2e.rs` reads pixel by pixel against
/// `crcbl_shaders::probe::irradiance_at`.
///
/// Every pixel it measures is the probe term and nothing else, on
/// [`probe_leak_forward`]'s terms exactly: `probe_sun` leaves the direct and
/// flat ambient contributions at zero, the floor is one quad of one albedo, and
/// the reflection pair is refused.
///
/// **The visibility capture runs**, and it is part of what this fixture is
/// about: one capture has to cover every level of the clipmap, which it does
/// because a level's rows are a range of the same table and the image keeps one
/// layer per row.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built or the capture cannot
/// be uploaded.
pub fn probe_clipmap_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
) -> Result<ForwardScene, OffscreenError> {
    let probes = probe_clipmap_grid();
    let mut scene = crate::render::scene::demo();
    scene.capacities.probes = probes.volume.total();
    scene.probes = probes;
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(crcbl_render::RenderEffects::REFLECTIONS, Some(false)),
        ..EffectRequest::default()
    });
    place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, probe_room());
    renderer.capture_probe_visibility(device, queue)?;
    Ok(ForwardScene {
        camera: probe_camera(),
        sun: probe_sun(),
        renderer: Box::new(renderer),
    })
}

// ---------------------------------------------------------------------------
// The scrolling fixtures
// ---------------------------------------------------------------------------

/// How many probes [`probe_scroll_grid`]'s one level holds along `x`.
///
/// Four, which is the least that makes a wrap something other than a swap: a
/// level of two rotates onto itself after one step and a shader that dropped the
/// wrap entirely would read the same two rows in the same two places half the
/// time.
pub const SCROLL_COUNT: u32 = 4;

/// How far apart those probes stand, in world units.
///
/// Chosen so the whole level is inside the frame at [`PROBE_CAMERA_UP`] — the
/// four probes span `3 · SCROLL_SPACING` and the visible floor is about `3.4`
/// wide — and so a step is a distance the trilinear blend resolves rather than a
/// nudge.
pub const SCROLL_SPACING: f32 = 0.6;

/// The radiance each of [`probe_scroll_grid`]'s constant environments is built
/// at.
///
/// [`PROBE_CLIPMAP_RADIANCE`]'s value and its reason: the brightest band a
/// blend of these can reach stays inside what the swapchain holds, so no
/// measurement is taken through `tonemap.slang`'s `saturate`.
const SCROLL_RADIANCE: f32 = 0.35;

/// **The scrolling fixture's volume**: one level of [`SCROLL_COUNT`] probes
/// along `x`, each holding a *different* constant environment, scrolled by
/// `steps` whole probe steps.
///
/// `docs/plan/50-irradiance-probes.md`'s toroidal addressing, as something a
/// frame can be wrong about. A level that has stepped `k` probes wraps cell `c`
/// onto row `(c + k) mod count`, and the only way to see that happen is for the
/// rows to differ from one another: these four hold red, green, blue and yellow,
/// so a step rotates the floor's colours along `x` and a shader that dropped the
/// wrap draws them in the order they were authored in.
///
/// Every row is a *constant* environment and therefore has no linear band at
/// all, which is deliberate on [`probe_clipmap_grid`]'s terms: what this fixture
/// is about is which row a fragment reads, and a directional row would put a
/// second thing in the answer.
///
/// Public so `tests/render_e2e.rs` predicts the frame from the rows and the
/// header the device was actually given, on [`probe_grid`]'s terms.
#[must_use]
pub fn probe_scroll_grid(steps: i32) -> crate::render::scene::ProbeGrid {
    let solid_angle = 4.0 * std::f32::consts::PI / 6.0;
    let constant = |radiance: [f32; 3]| {
        let mut probe = crate::shaders::probe::GpuProbe::ZERO;
        for direction in [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ] {
            probe.accumulate(direction, radiance, solid_angle);
        }
        probe
    };
    let mut scrolled = crate::shaders::probe::ProbeSteps::default();
    scrolled[0] = [steps, 0, 0];
    let volume = crate::shaders::probe::ProbeVolume {
        // Centred on the room, so the level is symmetric about the frame before
        // it is scrolled and the measured bands are as far from either `±X`
        // wall as each other.
        origin: [
            -0.5 * (SCROLL_COUNT - 1) as f32 * SCROLL_SPACING,
            0.5 * PROBE_ROOM_HEIGHT,
            0.0,
        ],
        inv_spacing: [1.0 / SCROLL_SPACING, 0.0, 0.0],
        counts: [SCROLL_COUNT, 1, 1],
        levels: 1,
        steps: scrolled,
    };
    let rows = [
        [SCROLL_RADIANCE, 0.0, 0.0],
        [0.0, SCROLL_RADIANCE, 0.0],
        [0.0, 0.0, SCROLL_RADIANCE],
        [SCROLL_RADIANCE, SCROLL_RADIANCE, 0.0],
    ];
    crate::render::scene::ProbeGrid {
        volume,
        probes: rows.into_iter().map(constant).collect(),
        update: ProbeUpdate::Authored,
    }
}

/// **The scrolling fixture**: [`Scene::Probes`]' room and camera over
/// [`probe_scroll_grid`] at `steps`.
///
/// Public rather than a [`Scene`] for [`probe_clipmap_forward`]'s reason: what
/// it measures is a *profile across one frame* at a scroll offset, which no
/// golden of a single image says anything about, and which `tests/render_e2e.rs`
/// reads pixel by pixel against `crcbl_shaders::probe::irradiance_at` over the
/// same header.
///
/// Every pixel it measures is the probe term and nothing else, on
/// [`probe_leak_forward`]'s terms exactly.
///
/// **The volume is scrolled before the capture**, so the maps are about where
/// the probes actually stand. That is the addressing question by itself; what
/// happens when a volume scrolls *after* a capture is
/// [`probe_slab_forward`]'s.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built or the capture cannot
/// be uploaded.
pub fn probe_scroll_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    steps: i32,
) -> Result<ForwardScene, OffscreenError> {
    let probes = probe_scroll_grid(steps);
    let mut scene = crate::render::scene::demo();
    scene.capacities.probes = probes.volume.total();
    scene.probes = probes;
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(crcbl_render::RenderEffects::REFLECTIONS, Some(false)),
        ..EffectRequest::default()
    });
    place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, probe_room());
    renderer.capture_probe_visibility(device, queue)?;
    Ok(ForwardScene {
        camera: probe_camera(),
        sun: probe_sun(),
        renderer: Box::new(renderer),
    })
}

/// How wide [`probe_slab_forward`]'s room is, in world units.
///
/// Wide enough that the level stands wholly inside it **before** the step —
/// its far probe is at `SLAB_ORIGIN + SLAB_SPACING`, and a probe outside the
/// room would capture the room's outer shell and occlude everything, which is
/// not the stale reading this fixture is about. The `±X` walls stay far outside
/// the frame at [`SLAB_CAMERA_UP`].
const SLAB_ROOM_WIDTH: f32 = 14.0;

/// How deep it is, and how tall: [`LEAK_ROOM_DEPTH`] and [`LEAK_ROOM_HEIGHT`],
/// so the divider is the room's full height and a probe cannot see over it.
const SLAB_ROOM_DEPTH: f32 = LEAK_ROOM_DEPTH;

/// How tall it is. See [`SLAB_ROOM_DEPTH`].
const SLAB_ROOM_HEIGHT: f32 = LEAK_ROOM_HEIGHT;

/// How far above the floor [`probe_slab_forward`]'s camera stands.
///
/// Higher than [`PROBE_CAMERA_UP`], because this fixture measures a band out at
/// the retained probe as well as two either side of the divider, and the
/// retained probe stands a whole spacing out. `tests/render_e2e.rs` derives its
/// own world-to-pixel mapping from this constant rather than reusing the probe
/// room's.
pub const SLAB_CAMERA_UP: f32 = 4.0;

/// How far apart [`probe_slab_grid`]'s two probes stand, in world units — and
/// therefore how far one whole probe step moves the level.
///
/// **Wide, and the divider is what makes it so.** The band that reads the
/// arrived probe *through* the wall has to clear the wall by more than
/// `crcbl_render::ForwardRenderer`'s occlusion radius, or the ambient-occlusion
/// pass rather than the visibility map would be what darkens it — so the two
/// probes have to be far enough apart that such a band still takes most of its
/// blend from the arrived one.
pub const SLAB_SPACING: f32 = 3.0;

/// Where probe `(0, 0, 0)` stands before the step, on `x`.
///
/// Both probes are on the `+X` side of the divider to begin with, which is the
/// whole setup: the step is what brings one of them across, and the map that row
/// is holding until it is re-captured is one taken from over there.
///
/// It is also where the probe that **stays** ends up standing, which is what
/// `tests/render_e2e.rs` reads the floor over.
pub const SLAB_ORIGIN: f32 = 2.7;

/// Where the level's near probe lands after one step back — [`SLAB_ORIGIN`] a
/// spacing nearer.
///
/// A quarter of a unit from the divider's face, so the map that probe *should*
/// have puts the wall well inside the distance to the `+X` band and well outside
/// the distance to the floor beneath it.
pub const SLAB_ARRIVED_AT: f32 = SLAB_ORIGIN - SLAB_SPACING;

/// The radiance of each of [`probe_slab_grid`]'s two constant environments.
///
/// [`LEAK_RADIANCE`]'s value and its reason: the brightest band stays inside
/// what the swapchain holds, so the measurement is not taken through
/// `tonemap.slang`'s `saturate`.
const SLAB_RADIANCE: f32 = 0.35;

/// **The slab fixture's volume**: two probes a [`SLAB_SPACING`] apart on `x`,
/// both starting on the `+X` side of the divider.
///
/// **Row 1 is red and row 0 is green**, and which is which is the fixture. One
/// step back wraps cell 0 onto row 1, so the probe that *arrives* on the `-X`
/// side is the red one and the probe that *stays* — at [`SLAB_ORIGIN`], where it
/// already was — is the green one. A band beyond the divider may then carry
/// green and must carry no red.
///
/// Public so `tests/render_e2e.rs` reads the rows the device was given.
#[must_use]
pub fn probe_slab_grid() -> crate::render::scene::ProbeGrid {
    let solid_angle = 4.0 * std::f32::consts::PI / 6.0;
    let constant = |radiance: [f32; 3]| {
        let mut probe = crate::shaders::probe::GpuProbe::ZERO;
        for direction in [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ] {
            probe.accumulate(direction, radiance, solid_angle);
        }
        probe
    };
    crate::render::scene::ProbeGrid {
        volume: crate::shaders::probe::ProbeVolume {
            origin: [SLAB_ORIGIN, 0.5 * SLAB_ROOM_HEIGHT, 0.0],
            inv_spacing: [1.0 / SLAB_SPACING, 0.0, 0.0],
            counts: [2, 1, 1],
            levels: 1,
            steps: crate::shaders::probe::ProbeSteps::default(),
        },
        probes: vec![
            constant([0.0, SLAB_RADIANCE, 0.0]),
            constant([SLAB_RADIANCE, 0.0, 0.0]),
        ],
        update: ProbeUpdate::Authored,
    }
}

/// The point [`probe_slab_forward`] follows to, which is one whole probe step
/// back from where the level was authored.
///
/// Named rather than written into the fixture because `tests/render_e2e.rs`
/// asserts the step it produces: the level's authored centre is a probe step
/// past this, so the nearest whole step is exactly one back.
#[must_use]
pub fn slab_follow_point() -> [f32; 3] {
    let centre = probe_slab_grid().volume.centre();
    [centre[0] - SLAB_SPACING, centre[1], centre[2]]
}

/// **The slab fixture**: a room with a divider, a volume captured on one side of
/// it, and one whole probe step that brings a probe across.
///
/// `docs/plan/50-irradiance-probes.md`'s recapture: the slab a scroll exposes is
/// captured in the frame it appears, and this is the claim as something a frame
/// can be wrong about. The red probe is captured at [`SLAB_ORIGIN`] `+`
/// [`SLAB_SPACING`], where the divider is nowhere near it; one step back stands
/// it at [`SLAB_ARRIVED_AT`], a quarter unit from the divider's face — and until
/// its map is taken again it reports open space in the direction of the `+X`
/// band, which is light through a wall.
///
/// `follow` is a parameter rather than a second fixture because the claim is a
/// comparison of one scene against itself: with it the level scrolls and the
/// exposed row is re-captured, without it the level stands where it was
/// authored.
///
/// Every band it measures is the probe term and nothing else, on
/// [`probe_leak_forward`]'s terms exactly.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built, the capture cannot
/// be uploaded, or the follow's recapture fails.
pub fn probe_slab_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    follow: bool,
) -> Result<ForwardScene, OffscreenError> {
    let probes = probe_slab_grid();
    let mut scene = crate::render::scene::demo();
    scene.capacities.probes = probes.volume.total();
    scene.probes = probes;
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(crcbl_render::RenderEffects::REFLECTIONS, Some(false)),
        ..EffectRequest::default()
    });
    let room = glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.5 * SLAB_ROOM_HEIGHT, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::new(
            SLAB_ROOM_WIDTH,
            SLAB_ROOM_HEIGHT,
            SLAB_ROOM_DEPTH,
        ));
    place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, room);
    place(&mut renderer, DEMO_CUBE, DEMO_UNTINTED, leak_wall());
    renderer.capture_probe_visibility(device, queue)?;
    if follow {
        // **After the capture**, which is the whole point: the maps this moves
        // are ones that already exist, and the row the step exposes is
        // re-captured here and nowhere else.
        renderer.follow_probe_volume(device, queue, slab_follow_point())?;
    }
    Ok(ForwardScene {
        camera: Camera {
            eye: glam::Vec3::new(0.0, SLAB_CAMERA_UP, 0.0),
            target: glam::Vec3::ZERO,
            up: glam::Vec3::Z,
            projection: Projection::Perspective {
                fov_y: std::f32::consts::FRAC_PI_3,
                near: 0.01,
            },
        },
        sun: probe_sun(),
        renderer: Box::new(renderer),
    })
}

// ---------------------------------------------------------------------------
// The sealed-cell fixture
// ---------------------------------------------------------------------------

/// How wide [`probe_sealed_forward`]'s room is, in world units.
///
/// [`LEAK_ROOM_WIDTH`]'s reason over a wider grid: the `±X` walls stay well
/// outside the frame, so the only vertical surfaces in the picture are the two
/// vaults and nothing else can stand between a probe and the band.
const SEALED_ROOM_WIDTH: f32 = 6.0;

/// How deep it is.
///
/// **Twice [`PROBE_ROOM_DEPTH`], and the band is what buys the difference.** The
/// measurement has to sit *beside* the vaults rather than under them — the
/// camera looks straight down and a vault hides the floor it stands over — and
/// it has to clear every wall by more than
/// `crcbl_render::ForwardRenderer`'s occlusion radius, on the terms
/// `tests/render_e2e.rs`'s `PROBE_BAND_AT` is chosen under — that constant lives
/// in the test crate, so this is its name rather than a link. Those two put it a
/// unit past where a two-deep room's `±Z` walls are.
/// The walls then leave the frame, which is the bargain [`PROBE_ROOM_WIDTH`]
/// already makes on the other axis.
const SEALED_ROOM_DEPTH: f32 = 4.0;

/// How tall it is. [`PROBE_ROOM_HEIGHT`], which is the height every probe room
/// in this file stands at and the one both vaults are sized under.
const SEALED_ROOM_HEIGHT: f32 = PROBE_ROOM_HEIGHT;

/// How far either side of the room's centre [`probe_sealed_grid`]'s two probes
/// stand, and therefore half the grid's spacing.
///
/// **Wide, because the two corners have to disagree.** The band reads both
/// probes, and what this fixture is about is the *divisor* that combines them:
/// a band near the middle of a narrow cell stands almost equally far from the
/// two probes, which gives their visibility maps almost equal readings and
/// leaves the divisor with nothing to say. At this reach the band stands three
/// times as far along the grid's own axis from one probe as from the other.
const SEALED_PROBE_REACH: f32 = 1.2;

/// Half the width of the vault sealing the `-X` probe, in world units.
///
/// **Deliberately not [`SEALED_LIT_VAULT_HALF`], and the difference is the
/// fixture's own red check.** The bound a sealed corner reports is the variance
/// of what its map can see over the square of how far past that the surface is,
/// cubed — so it scales steeply with the size of the room the probe is shut in,
/// and two probes sealed in *identically* sized vaults report bounds close
/// enough to each other that the weighted mean and the plain trilinear one are
/// the same picture. Nothing in the frame would then notice
/// `PROBE_OCCLUDED_WEIGHT` being deleted from `mesh.slang`. A tight vault
/// against a roomy one puts orders of magnitude between the two bounds while
/// leaving both far under the floor, which is the case this scene exists to put
/// on a device.
///
/// Wide enough all the same that the probe stands clear of every face by far
/// more than `crcbl_render::probe_capture`'s near plane, so no face of it is
/// clipped away and read as open space.
const SEALED_DARK_VAULT_HALF: f32 = 0.15;

/// Half the width of the vault sealing the `+X` probe.
///
/// [`SEALED_DARK_VAULT_HALF`]'s reason, from the other end of the ratio. It is
/// also the vault that has to stay inside the room: at this half-width its top
/// is [`SEALED_ROOM_HEIGHT`]'s upper quarter and its floor is clear of the
/// room's.
const SEALED_LIT_VAULT_HALF: f32 = 0.6;

/// Where the band is read along `x`, in world units.
///
/// Half of `SEALED_PROBE_REACH` towards the `-X` probe, so the trilinear blend
/// gives that probe three quarters and the `+X` one a quarter. **Not the cell's
/// centre**, which is where the two corners are mirror images of one another and
/// their visibility maps read the same by symmetry — see
/// `SEALED_DARK_VAULT_HALF`, which is the other half of the same argument.
///
/// Public so `tests/render_e2e.rs` names the band once rather than twice.
pub const SEALED_BAND_X: f32 = -0.5 * SEALED_PROBE_REACH;

/// Where it is read along `z`.
///
/// Out past both vaults, so the camera sees floor here rather than a lid, and
/// still more than `crcbl_render::ForwardRenderer`'s occlusion radius from every
/// vault and from the `+Z` wall — the condition `PROBE_BAND_AT` is chosen under,
/// which is what makes the occlusion over this block exactly one.
pub const SEALED_BAND_Z: f32 = 0.9;

/// [`probe_sealed_forward`]'s room: the open box at [`SEALED_ROOM_WIDTH`] ×
/// [`SEALED_ROOM_HEIGHT`] × [`SEALED_ROOM_DEPTH`], floor on the plane `y = 0`.
///
/// `probe_room`'s transform for `probe_room`'s reason, the floor at exactly
/// `y = 0` included: it is what lets `tests/render_e2e.rs` name the band's world
/// position without reconstructing anything.
fn sealed_room() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.5 * SEALED_ROOM_HEIGHT, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::new(
            SEALED_ROOM_WIDTH,
            SEALED_ROOM_HEIGHT,
            SEALED_ROOM_DEPTH,
        ))
}

/// A vault: the demo cube as a closed box of half-width `half`, centred on the
/// probe standing at `x` and at the probes' own height.
///
/// A cube because it is closed, which is the whole of the seal:
/// `crcbl_render::probe_capture` draws the scene's triangles with no culling, so
/// a probe standing inside one records its inner faces and every direction it
/// can look along is a wall.
fn sealed_vault(x: f32, half: f32) -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(x, 0.5 * SEALED_ROOM_HEIGHT, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::splat(2.0 * half))
}

/// [`probe_sealed_forward`]'s volume: two probes on the `x` axis
/// `SEALED_PROBE_REACH` either side of the room's centre, at half its height.
///
/// **The rows are [`probe_leak_grid`]'s** — the `-X` probe black and the `+X`
/// probe a constant environment — because this fixture wants what that one
/// wants of them: with one row at zero the band's value is the share of the
/// *other* row the blend gave it, and a share is exactly the quantity a divisor
/// decides. Taken from that function rather than rebuilt so the two fixtures
/// cannot drift into holding different light.
///
/// Public so `tests/render_e2e.rs` predicts the frame from the rows the device
/// was actually given, on [`probe_grid`]'s terms.
#[must_use]
pub fn probe_sealed_grid() -> crate::render::scene::ProbeGrid {
    crate::render::scene::ProbeGrid {
        volume: crate::shaders::probe::ProbeVolume {
            origin: [-SEALED_PROBE_REACH, 0.5 * SEALED_ROOM_HEIGHT, 0.0],
            inv_spacing: [1.0 / (2.0 * SEALED_PROBE_REACH), 0.0, 0.0],
            counts: [2, 1, 1],
            // One level, on `leak_volume`'s terms: the claim is about a cell's
            // eight corners, and a second level would put a second read between
            // them and the band.
            levels: 1,
            steps: crate::shaders::probe::ProbeSteps::default(),
        },
        ..probe_leak_grid()
    }
}

/// **The sealed-cell fixture**: one room, two probes, and a vault around each
/// of them that is there or is not.
///
/// `mesh.slang`'s `probe_irradiance` divides its weighted sum of eight corners
/// by the sum of their weights, and `PROBE_OCCLUDED_WEIGHT` is the floor that
/// keeps that divisor off zero — so a fragment whose every corner is hidden
/// falls back to the plain trilinear blend rather than to a division by nothing.
/// No other probe fixture in this file reaches that case — [`Scene::Probes`]
/// hides none of a band's corners and the divider fixtures hide some — and it is
/// the only arrangement in which the floor is what decides the pixel.
///
/// **`both` is the seal, and it is a parameter for [`probe_leak_forward`]'s
/// reason.** A frame in which every corner is occluded is a frame that looks
/// exactly like one in which none of them are — that is what the fallback
/// *means* — so a single picture cannot say whether the vaults occluded
/// anything at all. Drawn with `both`, the cell is sealed and the band must be
/// the plain blend. Drawn without it, only the black `-X` probe is sealed, its
/// three quarters of the blend are weighed out, and the band must jump to
/// nearly the whole of the lit probe. The second arm is what makes the first
/// one a measurement rather than a tautology.
///
/// Every band either arm reads is the probe term and nothing else, on
/// [`probe_leak_forward`]'s terms exactly: `probe_sun` leaves the direct and
/// flat ambient contributions at zero, the floor is one quad of one albedo, and
/// the reflection pair is refused. The camera is `probe_camera`'s rather than
/// one of its own, because `tests/render_e2e.rs` turns the band's pixels back
/// into world positions through that camera's mapping and a second copy of it
/// would agree only until somebody moved one of them.
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built or the capture cannot
/// be uploaded.
pub fn probe_sealed_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    both: bool,
) -> Result<ForwardScene, OffscreenError> {
    let probes = probe_sealed_grid();
    let mut scene = crate::render::scene::demo();
    scene.capacities.probes = probes.volume.total();
    scene.probes = probes;
    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(crcbl_render::RenderEffects::REFLECTIONS, Some(false)),
        ..EffectRequest::default()
    });
    place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, sealed_room());
    place(
        &mut renderer,
        DEMO_CUBE,
        DEMO_UNTINTED,
        sealed_vault(-SEALED_PROBE_REACH, SEALED_DARK_VAULT_HALF),
    );
    if both {
        place(
            &mut renderer,
            DEMO_CUBE,
            DEMO_UNTINTED,
            sealed_vault(SEALED_PROBE_REACH, SEALED_LIT_VAULT_HALF),
        );
    }
    // After the geometry and before the frame, on `probe_leak_forward`'s terms:
    // the two arms differ in exactly what this call records.
    renderer.capture_probe_visibility(device, queue)?;
    Ok(ForwardScene {
        camera: probe_camera(),
        sun: probe_sun(),
        renderer: Box::new(renderer),
    })
}

/// The engine's own description with [`probe_grid`] in it, and nothing else
/// changed.
///
/// The residents, the material rows and the page are [`scene::demo`]'s, so this
/// frame's geometry is the same open box every other scene here can draw and the
/// only difference from the description behind [`ForwardRenderer::new`] is the
/// volume — which is what makes this golden evidence about the probes rather
/// than about a new scene.
///
/// The capacity comes from the volume's own `total` rather than from the row
/// count, so the reservation and the grid cannot disagree about a number
/// `ProbeGrid::check` would then refuse.
///
/// [`scene::demo`]: crate::render::scene::demo
/// [`ForwardRenderer::new`]: crate::render::ForwardRenderer::new
fn probe_scene() -> crate::render::scene::SceneDesc<'static> {
    let probes = probe_grid();
    let mut scene = crate::render::scene::demo();
    scene.capacities.probes = probes.volume.total();
    scene.probes = probes;
    scene
}

/// Where [`Scene::SpotShadow`] puts its light.
///
/// **45° from vertical, and that angle is the whole scene.** The camera looks
/// straight down, so a light straight down too would throw every shadow directly
/// under the object that casts it, where the object's own image covers it — and
/// a camera lower than the light magnifies the caster more than the light does,
/// so the shadow can never grow out from under it. Tilting the light moves the
/// shadow sideways by the caster's height, which at 45° is one for one.
///
/// Close enough in that the cone's pool still fits the frame at
/// [`SPOT_SHADOW_OUTER_ANGLE`], and high enough that the tilt is a tilt rather
/// than a grazing light whose pool runs off the bottom of the frame.
const SPOT_SHADOW_LIGHT_AT: glam::Vec3 = glam::Vec3::new(0.0, 1.2, 1.2);

/// How far [`Scene::SpotShadow`]'s light reaches, in world units.
///
/// Twice its distance to the floor's centre, on [`SPOT_REACH`]'s terms exactly:
/// the pool's edge is then the cone's doing rather than the radius'.
const SPOT_SHADOW_REACH: f32 = 3.4;

/// The half-angle at which [`Scene::SpotShadow`]'s cone closes, in radians.
///
/// Written as an angle rather than as a radius on the floor — unlike
/// [`SPOT_EDGE_RADIUS`] — because this cone lands on the floor at 45° and its
/// pool is an ellipse, so there is no one radius to name it by. Wide enough that
/// the pool covers the caster and the whole of the shadow it throws, narrow
/// enough that dark floor is still visible in the frame's corners.
const SPOT_SHADOW_OUTER_ANGLE: f32 = 0.28;

/// The half-angle of its bright core.
///
/// Far enough inside [`SPOT_SHADOW_OUTER_ANGLE`] that the shadow and the
/// penumbra are separate things in the frame: this scene's claim is about a dark
/// region with a lit region beside it, and a cone that was all penumbra would put
/// a gradient across both.
const SPOT_SHADOW_INNER_ANGLE: f32 = 0.18;

/// How much [`Scene::SpotShadow`] scales the pyramid by to get its caster.
///
/// The pyramid rather than a second cube, because the cube is already the floor
/// and one resident cannot be two instances of different sizes here. Small
/// enough that the pool has lit floor on both sides of the shadow, large enough
/// that the shadow is tens of pixels across at the extent
/// `tests/render_e2e.rs` renders.
const SPOT_SHADOW_CASTER_SCALE: f32 = 0.5;

/// How far above the floor [`Scene::SpotShadow`]'s camera stands.
///
/// Low enough that the pool fills a good part of the frame — the assertions
/// measure bands of it, and a distant camera makes each band a handful of
/// pixels. The light is at [`SPOT_SHADOW_LIGHT_AT`] and off the camera's axis
/// entirely, so nothing here has to clear it.
const SPOT_SHADOW_CAMERA_UP: f32 = 1.3;

/// [`Scene::SpotShadow`]'s caster: the pyramid, scaled and dropped so its base
/// sits exactly on the floor.
///
/// **On the floor rather than floating**, so the contact point of the shadow is
/// in the frame: a shadow detached from its caster is what too much depth bias
/// looks like, and a floating caster would hide that failure behind a gap that
/// is meant to be there.
fn spot_shadow_caster() -> glam::Mat4 {
    // The pyramid's base is at `-PYRAMID_HALF_BASE` in its own space, so lifting
    // it by that much of the scale puts the base on `y = 0`.
    glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.4 * SPOT_SHADOW_CASTER_SCALE, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::splat(SPOT_SHADOW_CASTER_SCALE))
}

/// The camera [`Scene::SpotShadow`] is drawn with: straight down at the floor,
/// on [`spot_camera`]'s terms.
///
/// Overhead so the floor is a plane at a known scale and a pixel maps to a floor
/// position by a division — which is what lets the assertions name the band the
/// shadow falls in rather than searching for it.
fn spot_shadow_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, SPOT_SHADOW_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the
        // floor's `+Z` axis at the top of the frame, which is the direction the
        // light comes *from* and so the direction the shadow falls away from.
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// [`Scene::SpotShadow`]'s one light: a cone aimed at the floor's centre from
/// 45° up and behind it.
fn spot_shadow_light() -> crcbl_render::Light {
    crcbl_render::Light::Spot(crcbl_render::SpotLight {
        position: SPOT_SHADOW_LIGHT_AT,
        radius: SPOT_SHADOW_REACH,
        color: glam::Vec3::new(1.0, 0.95, 0.85) * SPOT_INTENSITY,
        // Along the cone, away from the light — so from the light to the floor's
        // centre, which is what makes the pool's axis land on the frame's.
        direction: -SPOT_SHADOW_LIGHT_AT,
        inner_angle: SPOT_SHADOW_INNER_ANGLE,
        outer_angle: SPOT_SHADOW_OUTER_ANGLE,
        fill: false,
    })
}

/// How far above the floor [`Scene::PointShadow`] hangs its light.
///
/// **Low, and that is the scene.** A point light's six faces are picked by the
/// largest component of the direction from the light, so a receiver at `(x, -h)`
/// from a light `h` up is on the `-Y` face while `|x| < h` and on a side face
/// past that. A light high over a floor therefore puts the whole visible floor —
/// casters, shadows and all — on one face, and a frame drawn with every face
/// selection wired to that one would be identical. From here the four side faces
/// own everything past `|x| = h`, which is where both shadows fall.
///
/// Above the casters, or nothing casts anything; low enough that a caster a third
/// of its height throws a shadow several times its own length, which is what
/// makes the dark band wide enough to measure.
const POINT_LIGHT_UP: f32 = 0.5;

/// How far [`Scene::PointShadow`]'s light reaches, in world units.
///
/// Past the far end of both shadows, so the pool's edge is off the frame and
/// every band the assertions measure is lit by the same falloff — the claim is a
/// ratio between two bands equidistant from the light, and a radius cutting
/// through one of them would decide it.
const POINT_REACH: f32 = 3.0;

/// How far from the light's axis each of [`Scene::PointShadow`]'s casters
/// stands.
///
/// Past [`POINT_LIGHT_UP`], so a caster is already on the side face its shadow
/// falls across and the two are one face's business rather than a shadow
/// straddling a seam — the seam is real and untested here on purpose, and
/// `docs/backlog.md` is where that belongs.
const POINT_CASTER_AT: f32 = 0.6;

/// How much [`Scene::PointShadow`] scales the pyramid by to get each caster.
///
/// Short next to [`POINT_LIGHT_UP`]: the shadow's tip is at
/// `POINT_CASTER_AT * up / (up - height)`, which runs away to infinity as a
/// caster approaches the light's own height. At this scale the tip lands
/// comfortably inside the frame.
const POINT_CASTER_SCALE: f32 = 0.3;

/// How far above the floor [`Scene::PointShadow`]'s camera stands.
///
/// High enough that both shadows and both of their mirror bands are inside the
/// frame: the far one reaches about `1.3` from the centre and the frame's short
/// half-axis on the floor is `up * tan(30°)`.
const POINT_CAMERA_UP: f32 = 2.2;

/// The camera [`Scene::PointShadow`] is drawn with: straight down at the floor,
/// on [`spot_camera`]'s terms exactly.
fn point_shadow_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, POINT_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the
        // world's `+Z` axis at the top of the frame.
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// [`Scene::PointShadow`]'s one light: a point light just above the floor's
/// centre.
///
/// Near-white, on [`spot_light`]'s terms: the floor's albedo is the cube's green
/// `+Y` face and a coloured light would tint the bands the assertions compare
/// rather than leaving the comparison to the shadow.
fn point_shadow_light() -> crcbl_render::Light {
    crcbl_render::Light::Point(crcbl_render::PointLight {
        position: glam::Vec3::new(0.0, POINT_LIGHT_UP, 0.0),
        radius: POINT_REACH,
        color: glam::Vec3::new(1.0, 0.95, 0.85) * SPOT_INTENSITY,
        fill: false,
    })
}

/// One of [`Scene::PointShadow`]'s casters: the pyramid, scaled and dropped so
/// its base sits on the floor, at `offset` from the light's axis.
///
/// **On the floor rather than floating**, on `spot_shadow_caster`'s terms: a
/// shadow detached from its caster is what too much depth bias looks like, and a
/// floating caster hides that behind a gap that is meant to be there.
fn point_shadow_caster(offset: glam::Vec3) -> glam::Mat4 {
    // The pyramid's base is at `-0.4` in its own space, so lifting it by that
    // much of the scale puts the base on `y = 0`.
    glam::Mat4::from_translation(offset + glam::Vec3::new(0.0, 0.4 * POINT_CASTER_SCALE, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::splat(POINT_CASTER_SCALE))
}

/// The base-colour factor [`Scene::AreaLight`]'s floor shades through.
///
/// **Dark, and that is what makes the highlight the subject.** A rectangle's
/// specular lobe is `F0` of the radiance leaving its face — four per cent for a
/// dielectric — and this camera is nearly head-on to the floor, which is the one
/// angle where Fresnel adds nothing to that four per cent. The rectangle's
/// *diffuse* has no such factor, so on an ordinary floor it is the larger term
/// everywhere including inside the highlight.
///
/// Measured, not argued: the first frame of this scene put the floor through the
/// demo scene's tinted row and read a mean channel level of `255.0` inside the
/// highlight against `198.8` on its fill mirror — a ratio of `1.28`, with the
/// highlight clipped flat across the whole band, which is a golden that cannot
/// carry the edge it exists to show. Darkening the albedo does not touch the
/// specular term at all; at this factor the same measurement reads `194.6`
/// against `81.7`, unclipped.
///
/// A **grey** factor rather than a second tint, so the floor's colour is the
/// cube's own `+Y` face darkened and nothing else, and the roughness stays
/// `crcbl_render::scene::PYRAMID_ROUGHNESS`: the lobe is what
/// `tests/mesh_e2e/area_light.rs` chose that row for, and the lobe is what this
/// scene keeps.
const AREA_ALBEDO: f32 = 0.15;

/// [`Scene::AreaLight`]'s floor row, in the description `area_scene` builds.
///
/// The demo scene's three rows and this one after them, so the three every other
/// fixture names keep the indices they have always had — `BLOOM_EMITTER`'s
/// arrangement exactly.
const AREA_FLOOR: usize = 3;

/// [`Scene::AreaLight`]'s and [`Scene::FillLight`]'s scene: the engine's own,
/// with the dark glossy floor row appended.
///
/// A description of its own for [`Scene::Bloom`]'s reason — this is one of two
/// fixtures needing a material the demo scene does not have — and the only thing
/// that differs is that one row.
///
/// **One description for the two scenes rather than one each.** They are the
/// same experiment on two light kinds and the floor is the control: a row that
/// moved in one of them and not the other would leave the pair comparable only
/// by eye.
fn area_scene() -> crate::render::scene::SceneDesc<'static> {
    let mut scene = crate::render::scene::demo();
    scene.materials.push(crate::shaders::mesh::GpuMaterial {
        base_color: [AREA_ALBEDO, AREA_ALBEDO, AREA_ALBEDO, 1.0],
        roughness: crate::render::scene::PYRAMID_ROUGHNESS,
        ..crate::shaders::mesh::GpuMaterial::UNTINTED
    });
    debug_assert_eq!(
        scene.materials.len() - 1,
        AREA_FLOOR,
        "the floor row is the one past the demo scene's three"
    );
    scene
}

/// How far above the floor [`Scene::AreaLight`] hangs each of its two strips.
///
/// **A rectangle's highlight is its own mirror image in the floor**, and the
/// mirror is what this height sets. With the eye on the axis, the reflection of
/// a strip sits at the strip's own position scaled by
/// `AREA_CAMERA_UP / (AREA_CAMERA_UP + AREA_STRIP_UP)` and is scaled by the same
/// factor — about seven tenths here, which puts [`AREA_STRIP_AT`]'s strips'
/// reflections at `1.0` from the axis and makes each about `0.61` long.
///
/// Low enough that the strip subtends a wide angle, which is what stops the
/// highlight collapsing into the lobe's own round blob: a light smaller than the
/// lobe draws a shape the *lobe* chose, and the whole claim here is that the
/// shape is the *rectangle's*. High enough that its reflection and its diffuse
/// pool are not the same spot, so the fill strip's side of the frame has a pool
/// with no gleam in it rather than nothing at all.
const AREA_STRIP_UP: f32 = 0.8;

/// Half the length of each of [`Scene::AreaLight`]'s strips, along `z`.
///
/// `tests/mesh_e2e/area_light.rs`'s `STRIP_LONG` unchanged, because the whole
/// point of this scene is that it is that file's light rather than a second
/// rectangle nobody has looked at.
const AREA_STRIP_LONG: f32 = 0.85;

/// Half its width, across that axis.
///
/// That file's `STRIP_SHORT` unchanged, and an order of magnitude under
/// [`AREA_STRIP_LONG`] for its reason: a near-square strip is its own rotation,
/// and a highlight that reaches as far across as it does along is one no
/// assertion can tell from a point light's.
const AREA_STRIP_SHORT: f32 = 0.07;

/// How far out along `x` each of [`Scene::AreaLight`]'s strips hangs from the
/// frame's axis.
///
/// The strips lie along `z` and are separated along `x`, which is the
/// arrangement that fits: each highlight runs about `0.6` up and down the frame
/// and reaches about `0.25` either side of its own axis once the lobe has
/// spread the reflection's few hundredths, so two of them side by side leave
/// most of the frame's width between them while both stay well inside its
/// height.
///
/// Far enough out that the two highlights are separate objects in the picture —
/// a frame where they touch has no floor between them for the comparison to
/// stand on — and near enough that both are inside the frame with margin. At
/// [`AREA_CAMERA_UP`]'s framing the frame covers about `1.54` of floor either
/// side of the axis and each highlight lands at about `1.0`.
const AREA_STRIP_AT: f32 = 1.4;

/// How far each strip's influence reaches from its own centre, in world units.
///
/// Comfortably past the frame's far corner, on `tests/mesh_e2e/area_light.rs`'s
/// `STRIP_REACH`'s terms: the quartic window `crcbl_render::RectLight::radius`
/// documents is then nowhere near its zero anywhere in the picture, so what
/// shapes this frame is the polygon integral and not the radius.
const AREA_REACH: f32 = 12.0;

/// The radiance leaving each strip's face.
///
/// `tests/mesh_e2e/area_light.rs`'s `STRIP_COLOR` at two fifths, keeping its hue
/// exactly. Well above one for that constant's reason — the scene target is
/// `Rgba16Float` and the tonemap is what brings it down — and below it for this
/// scene's: over `AREA_ALBEDO`'s floor the full figure clipped the highlight
/// flat, and a band of saturated pixels is a golden with no edge in it. **This
/// is the exposure and not the contrast**: every term in the frame scales with
/// it, so it moves where the highlight sits in the display range and not how far
/// it leads the floor beside it. At this figure the highlight's band reads
/// `194.6` of a possible `255`.
const AREA_RADIANCE: glam::Vec3 = glam::Vec3::new(8.8, 7.92, 6.76);

/// How far above the floor [`Scene::AreaLight`]'s camera stands.
///
/// Sets the scale of the picture on [`POINT_CAMERA_UP`]'s terms — the frame's
/// short half-axis on the floor is `up * tan(30°)` — and, with
/// [`AREA_STRIP_UP`], where each highlight lands. High enough that both
/// highlights and the floor either side of them are in frame; low enough that
/// each is tens of pixels across at the golden suite's 256×192.
const AREA_CAMERA_UP: f32 = 2.0;

/// The camera [`Scene::AreaLight`] and [`Scene::FillLight`] are drawn with:
/// straight down at the floor, on [`spot_camera`]'s terms exactly.
///
/// **On the axis the lights are mirrored about**, which is the whole of what
/// makes either comparison a control rather than an estimate: an off-axis eye
/// sees the two halves at different angles, and the specular lobe is a function
/// of that angle.
fn area_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, AREA_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the
        // world's `+Z` axis at the top of the frame, which is the axis the
        // strips run along.
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// The sun [`Scene::AreaLight`] and [`Scene::FillLight`] run under: **straight
/// down**, and barely there.
///
/// Both halves are load-bearing, on [`ao_sun`]'s terms and for a second reason
/// of this scene's own.
///
/// * **Straight down** — a directional light along the view axis over a flat
///   floor contributes the same diffuse everywhere and a specular term that
///   depends only on the distance from the frame's centre. Two points mirrored
///   across the frame's axis therefore take *identical* sun, which is what makes
///   the mirror an exact control. Under the default sun's tilt they would not,
///   and the difference between them would be a sun term wearing the fill flag's
///   name. `crcbl_render::shadow` picks a second up vector for exactly this
///   direction, so the cascades are built rather than degenerate.
/// * **Barely there** — the claim is a ratio between the two mirrored
///   highlights, and a sun bright enough to be a large part of either is a sun
///   that decides the ratio. Turned down rather than removed, for
///   [`dimmed_sun`]'s reason: a sun that stopped contributing is a light row
///   that stopped working, and a scene without one would not notice.
fn area_sun() -> crcbl_render::DirectionalLight {
    crcbl_render::DirectionalLight {
        direction: glam::Vec3::Y,
        ..dimmed_sun(0.03, 0.09)
    }
}

/// One of [`Scene::AreaLight`]'s strips: lying along `z` at `x`, facing straight
/// down, and a fill light or not.
///
/// The half-extents stay with the axes rather than with the light — `tangent` is
/// the rectangle's `u` axis and `half_width` is measured along it — so the two
/// strips are the same rectangle in the same orientation and `fill` really is
/// the only field between them.
fn area_strip(x: f32, fill: bool) -> crcbl_render::Light {
    crcbl_render::Light::Rect(crcbl_render::RectLight {
        position: glam::Vec3::new(x, AREA_STRIP_UP, 0.0),
        radius: AREA_REACH,
        color: AREA_RADIANCE,
        // Away from the panel, at the floor below it — the convention
        // `crcbl_render::RectLight::direction` states.
        direction: glam::Vec3::NEG_Y,
        tangent: glam::Vec3::Z,
        half_width: AREA_STRIP_LONG,
        half_height: AREA_STRIP_SHORT,
        fill,
    })
}

/// [`Scene::AreaLight`]'s two strips: the ordinary one out along `-x`, the fill
/// one out along `+x`.
///
/// Two rows that differ in one boolean and in the sign of one coordinate. A
/// helper rather than two literals at the call site, so there is one place the
/// pair can be read and no way for a field to drift between them — which is the
/// same argument [`place_pyramids`] makes about the geometry it places.
fn area_strips() -> [crcbl_render::Light; 2] {
    [
        area_strip(-AREA_STRIP_AT, false),
        area_strip(AREA_STRIP_AT, true),
    ]
}

/// How far above the floor each of [`Scene::FillLight`]'s four lights hangs.
///
/// **The gleam and the pool have to be different places, and this is what puts
/// them apart.** With the eye on the axis at [`AREA_CAMERA_UP`], a light at
/// horizontal offset `p` puts its specular highlight at `p` scaled by
/// `camera / (camera + up)` — [`AREA_STRIP_UP`]'s arithmetic — while its
/// brightest diffuse is under the light itself. A light on the floor would put
/// the two in the same spot and a light at the camera's own height would leave
/// the gleam on the axis, and in neither case would a band at the highlight be
/// measuring a lobe.
const FILL_LIGHT_UP: f32 = 0.8;

/// How far out along `x` each of [`Scene::FillLight`]'s lights hangs from the
/// frame's axis.
///
/// [`AREA_STRIP_AT`]'s offset for its reason: far enough out that the lit half's
/// highlight and the fill half's mirror of it are separate objects in the
/// picture with open floor between them, and near enough that both are inside
/// the frame with margin at [`AREA_CAMERA_UP`]'s framing.
const FILL_LIGHT_AT: f32 = 1.4;

/// How far out along `z` each **pair** sits from the frame's centre: the point
/// pair on `-z`, the spot pair on `+z`.
///
/// One frame carries both kinds because the flag is one flag — `Light::row`
/// sets [`FLAG_FILL`](crcbl_shaders::light::FLAG_FILL) from `Light::is_fill`,
/// which is one question asked of three variants — so a scene that drew the
/// kinds separately would be two frames of the same claim.
///
/// Split along `z` rather than `x` because `x` is the axis the mirror is about:
/// separating the pairs along it would put a point light and a spot light on
/// opposite sides of that mirror, and the two halves would then differ in a
/// light's *kind* as well as in its `fill`. Far enough apart that the four
/// highlights are four separate objects in the frame, and near enough that all
/// four are inside it — every size this scene is drawn at is wider than it is
/// tall, so this axis has the less room of the two.
const FILL_PAIR_Z: f32 = 0.7;

/// The radius of each [`Scene::FillLight`] spot's bright core where it lands on
/// the floor.
///
/// Written as a radius on the floor rather than as a half-angle, which is also
/// how [`fill_spot`] passes it — [`SPOT_CORE_RADIUS`]'s convention. Wide enough
/// to reach past the spot's own highlight, which sits [`FILL_LIGHT_AT`] and
/// [`FILL_PAIR_Z`] scaled by [`FILL_LIGHT_UP`]'s factor away from the point
/// under the light: a highlight sitting in the penumbra would have the cone's
/// ramp multiplying it, and the ramp is not what this frame is about.
const FILL_SPOT_CORE: f32 = 0.6;

/// The radius at which each of those cones has closed, on the same terms.
///
/// Comfortably outside [`FILL_SPOT_CORE`] so the cone is visibly a cone in the
/// picture — a spot whose penumbra is a step is one no reader can tell from a
/// point light — and the band between the two is where the frame says which kind
/// the row was read as.
const FILL_SPOT_EDGE: f32 = 0.95;

/// How far each of [`Scene::FillLight`]'s lights reaches from its own position.
///
/// [`AREA_REACH`]'s figure for its reason, and its own constant rather than that
/// one so the two scenes' framings stay independent: the quartic window
/// `crcbl_render::PointLight::radius` documents is nowhere near its zero
/// anywhere in the picture, so what shapes this frame is the inverse square, the
/// cone and the lobe rather than the radius.
const FILL_REACH: f32 = 12.0;

/// The colour each of [`Scene::FillLight`]'s four lights carries, intensity
/// included.
///
/// Near-white on [`spot_light`]'s terms: the floor is [`AREA_ALBEDO`]'s grey and
/// a coloured light would tint a frame whose whole subject is a *level*.
///
/// **Under one, where [`AREA_RADIANCE`] is well over it**, and the two are not
/// comparable figures: a rectangle's is the radiance leaving a face and this is
/// a punctual light's intensity, which the inverse square then divides by a
/// distance under a unit. **It is the exposure and not the contrast**, on that
/// constant's terms — every term in the frame scales with it, so it moves where
/// the highlights sit in the display range and not how far each leads its
/// mirror.
///
/// Swept rather than guessed. At four times this figure the highlight's band
/// read a flat `255.0` of a possible `255` and the frame's brightest channel was
/// saturated with it, which is a golden whose highlight has no falloff left in
/// it; here the band reads `203.4` and the brightest channel anywhere in the
/// frame is `232`.
const FILL_INTENSITY: glam::Vec3 = glam::Vec3::new(0.5, 0.475, 0.425);

/// One of [`Scene::FillLight`]'s point lights: on the point pair's row at `x`,
/// and a fill light or not.
fn fill_point(x: f32, fill: bool) -> crcbl_render::Light {
    crcbl_render::Light::Point(crcbl_render::PointLight {
        position: glam::Vec3::new(x, FILL_LIGHT_UP, -FILL_PAIR_Z),
        radius: FILL_REACH,
        color: FILL_INTENSITY,
        fill,
    })
}

/// One of [`Scene::FillLight`]'s spots: on the spot pair's row at `x`, pointing
/// straight down, and a fill light or not.
///
/// Straight down, and the mirror is what fixes that: reflecting about `x = 0`
/// has to carry each cone onto its twin's, so the aim may have no `x` component
/// at all. Straight down is the one such aim that also puts the cone's axis
/// through the brightest part of its own pool, which is what keeps the two
/// cones' footprints the same shape as well as the same size.
fn fill_spot(x: f32, fill: bool) -> crcbl_render::Light {
    crcbl_render::Light::Spot(crcbl_render::SpotLight {
        position: glam::Vec3::new(x, FILL_LIGHT_UP, FILL_PAIR_Z),
        radius: FILL_REACH,
        color: FILL_INTENSITY,
        // Along the cone, away from the light — `spot_light`'s convention, which
        // `crcbl_render::SpotLight::direction` is where it is spelled out.
        direction: glam::Vec3::NEG_Y,
        inner_angle: (FILL_SPOT_CORE / FILL_LIGHT_UP).atan(),
        outer_angle: (FILL_SPOT_EDGE / FILL_LIGHT_UP).atan(),
        fill,
    })
}

/// [`Scene::FillLight`]'s four lights: a point pair and a spot pair, the
/// ordinary one of each out along `-x` and the fill one out along `+x`.
///
/// A helper rather than four literals at the call site, on [`area_strips`]'
/// terms: this is the one place the mirror can be read, and there is no way for
/// a height, a reach or a colour to drift between a light and its twin.
fn fill_light_pairs() -> [crcbl_render::Light; 4] {
    [
        fill_point(-FILL_LIGHT_AT, false),
        fill_point(FILL_LIGHT_AT, true),
        fill_spot(-FILL_LIGHT_AT, false),
        fill_spot(FILL_LIGHT_AT, true),
    ]
}

/// [`Scene::AlphaMask`]'s page layer, in the description `alpha_mask_scene`
/// builds.
///
/// The demo scene's one layer and this one after it, so
/// `crcbl_render::scene::CHECKER_LAYER` keeps the index it has always had —
/// [`AREA_FLOOR`]'s arrangement, one page along.
const ALPHA_MASK_LAYER: u32 = 1;

/// [`ALPHA_MASK_LAYER`]: one column of texels at alpha `0x00` and one at
/// `0xFF`, at the same white RGB.
///
/// **A straight cut and not a speckle.** `crcbl_render::scene::PAGE_EXTENT` is
/// two texels a side and the page is read through the renderer's trilinear
/// sampler, so a single transparent texel would arrive as a blend of its
/// neighbours and the mip chain would average it away entirely. Half the layer
/// is the one shape that survives both: the sampled alpha is exactly `0` for
/// the first quarter of the axis, exactly `1` for the last quarter and a ramp
/// between, so the cut against `alpha_cutoff` lands on the plane where the ramp
/// crosses it — the middle of the plate — however the sampler filters.
///
/// The cut is in **u**, which on the cube's `+Y` face runs along `+x` — see
/// `crcbl_shaders::mesh::cube_vertices`, whose quad puts `u = 0` at `-x`. So
/// the transparent half is the plate's `-x` half, which
/// [`alpha_camera`](fn@alpha_camera)'s framing puts on the **right** of the
/// frame.
///
/// **Every texel's RGB is [`PageDesc::WHITE`]**, cut and kept alike, which is
/// what makes the hole an absence rather than a colour. A layer whose
/// transparent half were also a different colour would draw very nearly this
/// picture with the discard wired to nothing, and every band below would still
/// read what it expects.
///
/// [`PageDesc::WHITE`]: crate::render::scene::PageDesc::WHITE
const ALPHA_MASK_TEXELS: [u8; 16] = [
    0xFF, 0xFF, 0xFF, 0x00, // (0, 0) — cut
    0xFF, 0xFF, 0xFF, 0xFF, // (1, 0) — kept
    0xFF, 0xFF, 0xFF, 0x00, // (0, 1) — cut
    0xFF, 0xFF, 0xFF, 0xFF, // (1, 1) — kept
];

/// [`Scene::AlphaMask`]'s floor row, in the description `alpha_mask_scene`
/// builds.
const ALPHA_FLOOR: usize = 3;

/// Its masked plate's row, the one past the floor's.
const ALPHA_PLATE: usize = 4;

/// The base-colour factor [`Scene::AlphaMask`]'s floor shades through.
///
/// A **grey** factor rather than a tint, on [`AREA_ALBEDO`]'s terms exactly: the
/// floor's colour is then the cube's own `+Y` face and nothing else, and the
/// plate is the only thing in the frame carrying a hue of its own. Bright rather
/// than dark, because two of this scene's three claims are about a *shadow* on
/// this floor and the third is about seeing it through a hole — all three want
/// the lit floor as far above the ambient as the sun can put it.
const ALPHA_FLOOR_ALBEDO: f32 = 0.8;

/// The base-colour factor [`Scene::AlphaMask`]'s plate shades through.
///
/// **Red against the floor's green, and the hue is what the first claim is read
/// in.** The cube's `+Y` face is `(0.25, 0.80, 0.30)`, so this row's green
/// channel lands near a seventh of the floor's while its red lands above it: a
/// band inside the hole and a band on the solid half of the plate are then
/// separated by a factor in one channel rather than by a brightness a shading
/// bug could also produce. `tests/render_e2e.rs` reads exactly that channel —
/// see its `ALPHA_HOLE_RATIO`.
const ALPHA_PLATE_TINT: [f32; 4] = [1.0, 0.12, 0.12, 1.0];

/// [`Scene::AlphaMask`]'s scene: the engine's own, with the mask layer and the
/// two material rows appended.
///
/// A description of its own for [`Scene::Bloom`]'s reason — this fixture needs
/// materials the demo scene does not have — and it is the only description in
/// this module that appends a **page layer**, because it is the only fixture
/// whose subject is a texel's alpha.
fn alpha_mask_scene() -> crate::render::scene::SceneDesc<'static> {
    const {
        let extent = crate::render::scene::PAGE_EXTENT as usize;
        assert!(
            ALPHA_MASK_TEXELS.len() == extent * extent * 4,
            "the mask layer is authored at the page's own extent, and a layer of any other \
             length is refused before a frame is drawn"
        );
    }
    let mut scene = crate::render::scene::demo();
    let mask = scene.page.push_layer(
        crate::render::scene::PageKind::BaseColor,
        &ALPHA_MASK_TEXELS[..],
    );
    debug_assert_eq!(
        mask, ALPHA_MASK_LAYER,
        "the mask is the layer past the demo scene's checker"
    );
    scene.materials.push(crate::shaders::mesh::GpuMaterial {
        base_color: [
            ALPHA_FLOOR_ALBEDO,
            ALPHA_FLOOR_ALBEDO,
            ALPHA_FLOOR_ALBEDO,
            1.0,
        ],
        ..crate::shaders::mesh::GpuMaterial::UNTINTED
    });
    debug_assert_eq!(
        scene.materials.len() - 1,
        ALPHA_FLOOR,
        "the floor row is the one past the demo scene's three"
    );
    scene.materials.push(crate::shaders::mesh::GpuMaterial {
        base_color: ALPHA_PLATE_TINT,
        base_color_texture: mask,
        // The whole of what makes this scene a fixture: without the bit the row
        // is an ordinary opaque one, `MaterialTable::mode` answers `OPAQUE`, the
        // plate's instances scatter into the opaque bucket, and the frame draws
        // a solid plate over a solid shadow.
        flags: crate::shaders::mesh::GpuMaterial::ALPHA_MODE_MASK,
        ..crate::shaders::mesh::GpuMaterial::UNTINTED
    });
    debug_assert_eq!(
        scene.materials.len() - 1,
        ALPHA_PLATE,
        "the plate's row is the one past the floor's"
    );
    scene
}

/// How much [`Scene::AlphaMask`] scales the cube by to get its floor.
///
/// **Deliberately smaller than [`SPOT_FLOOR_SCALE`], and that is a claim rather
/// than a framing choice.** This scene's second claim is that the hole reads as
/// *floor* and not as the frame's clear — which is what a depth prepass that
/// wrote the plate's whole silhouette would leave there — and a claim of that
/// shape needs a band of clear in the same frame to be read against. A floor
/// running past every edge leaves none, so this one stops inside all four: at
/// [`ALPHA_CAMERA_UP`] the frame covers `±1.85` of floor across and `±1.39`
/// along, and the floor reaches `±1.2`.
///
/// Large enough that the plate, the whole of its shadow and every band beside
/// them are on it: the furthest is the shadow's own far edge at `z = -1.01`.
const ALPHA_FLOOR_SCALE: f32 = 2.4;

/// [`Scene::AlphaMask`]'s floor: the cube scaled by [`ALPHA_FLOOR_SCALE`] and
/// dropped so its `+Y` face is the plane `y = 0`.
///
/// [`spot_floor`]'s shape at this scene's own scale — the cube spans half a unit
/// either side of its origin, so the drop is half the scale. Not that helper
/// itself, for the reason [`ALPHA_FLOOR_SCALE`] gives.
fn alpha_floor() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.5 * ALPHA_FLOOR_SCALE, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::splat(ALPHA_FLOOR_SCALE))
}

/// How far above the floor [`Scene::AlphaMask`] hangs its plate.
///
/// **This is the separation between the shadow and the thing casting it.**
/// [`alpha_sun`] comes in at 45° with no `x` component, so a caster at height
/// `h` throws its shadow `h` along `-z` — and the plate reaches `0.25` along
/// that axis, so any height past `0.5` puts the shadow clear of the footprint.
/// This is half again as far, which leaves near a quarter of a unit of lit
/// floor between the plate's near edge and its shadow's, wide enough to be
/// legible in the golden.
///
/// Not higher: the shadow's far edge is already at `z = -1.01` and the floor
/// stops at `-1.2`. Past `crcbl_shaders::ssao::RADIUS_DEFAULT`, which is what
/// the occlusion pass gathers within: nothing of the plate is then in reach of
/// the floor under it, so the band inside the hole is lit floor and not a floor
/// darkened by the plate's proximity.
const ALPHA_PLATE_UP: f32 = 0.75;

/// How much [`Scene::AlphaMask`] scales the cube by to get its plate: flat and
/// wide.
///
/// Wide along `x`, which is the axis the mask cuts across, so both halves are
/// tens of pixels wide at the golden suite's 256×192 and a band fits inside
/// each. Short along `z`, which is the axis the shadow travels along, because
/// the gap between the plate and its own shadow is [`ALPHA_PLATE_UP`] less this
/// half-depth — a deeper plate walks its shadow back under itself.
///
/// **Thin, and the thinness was swept rather than picked.** A slab's two faces
/// cast two shadow edges a slab-thickness apart, so at a thickness comparable to
/// the cascade filter's own width the near edge of the shadow is a double edge
/// inside that filter — which is exactly where a last-bit depth difference
/// between the two geometry paths flips a tap. Measured on lavapipe, as the
/// channels `render_e2e.rs`'s
/// `the_alpha_mask_scene_draws_the_same_frame_on_every_geometry_path` reports:
/// `0.02` and `0.24` disagree about **nothing**, `0.12` about one channel by one
/// level, and `0.06` about sixteen channels by up to seven — every one of them
/// along that near edge, and radv answers zero at all four. So the thickness is
/// either well inside the filter or well outside it, and this is the first:
/// `0.02` puts the two edges about one and a half pixels apart at this framing,
/// which is one edge.
///
/// Not zero: a zero-thickness slab has a back face coincident with its front
/// one, and the depth prepass this scene is evidence about would then be
/// measuring z-fighting.
const ALPHA_PLATE_SCALE: glam::Vec3 = glam::Vec3::new(1.4, 0.02, 0.5);

/// [`Scene::AlphaMask`]'s plate: the cube at [`ALPHA_PLATE_SCALE`], centred over
/// the floor's origin at [`ALPHA_PLATE_UP`].
fn alpha_plate() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, ALPHA_PLATE_UP, 0.0))
        * glam::Mat4::from_scale(ALPHA_PLATE_SCALE)
}

/// How far above the floor [`Scene::AlphaMask`]'s camera stands.
///
/// Sets the scale of the picture on [`POINT_CAMERA_UP`]'s terms — the frame's
/// short half-axis on the floor is `up * tan(30°)`. High enough that the floor's
/// own edges are inside the frame on all four sides, which is what
/// [`ALPHA_FLOOR_SCALE`] needs of it; low enough that the plate's two halves and
/// the two halves of its shadow are each tens of pixels across.
const ALPHA_CAMERA_UP: f32 = 2.4;

/// The camera [`Scene::AlphaMask`] is drawn with: straight down at the floor, on
/// [`spot_camera`]'s terms exactly.
///
/// Overhead so the floor is a plane at a known scale and a pixel maps to a floor
/// position by a division — which is what lets the assertions name the band the
/// hole and its shadow fall in rather than searching for them.
fn alpha_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, ALPHA_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the
        // world's `+Z` axis at the top of the frame, which is the direction the
        // light comes from and so the direction the shadow falls away from.
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// The sun [`Scene::AlphaMask`] runs under: [`DirectionalLight::default`]'s
/// colour and ambient, tilted to 45° in the `z` plane alone.
///
/// **No `x` component, and that is the whole of the tilt's job.** The mask's cut
/// runs along `z` at the plate's own `x = 0`, so a sun with an `x` component
/// would slide the shadow's cut away from the plate's and the two bands under
/// the shadow would no longer be the shadow of the two bands on the plate. With
/// the `x` component gone the shadow is the plate translated along `-z` by
/// exactly its height, and every band below is its own mirror one step down the
/// frame.
///
/// 45° rather than steeper, on [`SPOT_SHADOW_LIGHT_AT`]'s terms: the shadow
/// moves by the caster's height, which is what carries it out from under the
/// plate. Not shallower, which would run the shadow off the floor — see
/// [`ALPHA_PLATE_UP`], where the arithmetic is.
///
/// The colour and ambient are the default sun's rather than a dimmed pair: this
/// scene's claims are ratios between lit floor, shadowed floor and a plate, and
/// each of them is widest under a bright key.
fn alpha_sun() -> crcbl_render::DirectionalLight {
    crcbl_render::DirectionalLight {
        direction: glam::Vec3::new(0.0, 1.0, 1.0).normalize(),
        ..crcbl_render::DirectionalLight::default()
    }
}

/// [`Scene::DoubleSided`]'s floor row, in the description
/// `double_sided_scene` builds — the row past the demo scene's three.
const DOUBLE_FLOOR: usize = 3;

/// Its single-sided quad row, the one past the floor's.
///
/// Worn by two of the three quads: the one the camera and the sun both see the
/// back of, which is therefore culled everywhere, and the mirror, which is the
/// same row turned over.
const DOUBLE_SINGLE: usize = 4;

/// Its double-sided quad row, the one past that — the same factors with
/// [`GpuMaterial::DOUBLE_SIDED`] set and nothing else.
///
/// **The same factors is the whole of what makes the frame a comparison.** The
/// claim is that the double-sided quad reads what the mirror reads, so a row
/// that also differed in a colour or a roughness would make the two bands
/// incomparable and the claim untestable.
///
/// [`GpuMaterial::DOUBLE_SIDED`]: crate::shaders::mesh::GpuMaterial::DOUBLE_SIDED
const DOUBLE_DOUBLE: usize = 5;

/// [`Scene::DoubleSided`]'s quad, the mesh past the demo scene's four.
const DOUBLE_QUAD_MESH: usize = 4;

/// The frame height every pixel figure in this fixture is arithmetic for, on
/// [`SPECULAR_ALIGNED_ROWS`]' terms exactly.
const DOUBLE_ALIGNED_ROWS: f32 = 192.0;

/// The frame width that goes with it.
const DOUBLE_ALIGNED_COLUMNS: f32 = 256.0;

/// The tangent of half [`double_camera`]'s vertical field of view.
///
/// Thirty degrees, written out because a trigonometric function is not `const`.
/// Nothing rests on the last digit: [`double_sided_quad_mesh`] asserts the pixel
/// alignment through the camera's own matrix, so an inaccurate constant here
/// fails there rather than drifting into the golden.
const DOUBLE_TAN_HALF_FOV: f32 = 0.577_350_3;

/// How far above the floor [`Scene::DoubleSided`]'s camera stands, and how far
/// the frame reaches across the floor because of it.
///
/// [`ALPHA_CAMERA_UP`] exactly, and for its reasons — the floor's own edges are
/// inside the frame on all four sides, and each quad and each shadow is tens of
/// pixels across.
const DOUBLE_CAMERA_UP: f32 = ALPHA_CAMERA_UP;

/// How far above the floor the three quads hang.
///
/// [`ALPHA_PLATE_UP`] exactly: [`double_sun`] comes in at 45° with no `x`
/// component, so a caster at height `h` throws its shadow `h` along `-z`, and
/// this height clears the quad's own footprint with a band of lit floor left
/// between the two. Past `crcbl_shaders::ssao::RADIUS_DEFAULT` for that
/// constant's reason as well: the floor under a quad is lit floor and not floor
/// darkened by the quad's proximity.
const DOUBLE_QUAD_UP: f32 = ALPHA_PLATE_UP;

/// How many pixels of the golden frame one world unit **at the quads' height**
/// covers.
///
/// The three quads are one plane at one distance under a camera looking
/// straight down, so their projection is affine and this is the whole of it —
/// [`SPECULAR_ALIGNED_PIXELS_PER_UNIT`]'s arithmetic with the plane's own
/// distance from the eye in place of the plate's.
const DOUBLE_QUAD_PIXELS_PER_UNIT: f32 =
    DOUBLE_ALIGNED_ROWS * 0.5 / ((DOUBLE_CAMERA_UP - DOUBLE_QUAD_UP) * DOUBLE_TAN_HALF_FOV);

/// Half a quad's extent along `x`, in world units — **eleven whole pixels of
/// the golden frame either side of its centre.**
///
/// [`SPECULAR_STRIP_PITCH`]'s argument at this fixture's scale: Vulkan
/// guarantees only four `subPixelPrecisionBits`, so a vertex at an arbitrary
/// fraction of a pixel snaps to different points on radv's eighth-bit grid and
/// SwiftShader's sixteenth, and a quad edge is the highest-contrast thing in
/// this frame. On an integer coordinate it snaps to the same point on any of
/// those grids. [`double_sided_quad_mesh`] asserts the property through the
/// scene's own camera and model matrices rather than trusting this arithmetic.
///
/// Thirty-six pixels across is wide enough for a band of twenty-four with six
/// clear either side, and narrow enough that three of them and the gaps between
/// them fit inside the floor — see [`DOUBLE_QUAD_AT`].
const DOUBLE_PLATE_HALF_X: f32 = 18.0 / DOUBLE_QUAD_PIXELS_PER_UNIT;

/// Half a quad's extent along `z`, on [`DOUBLE_PLATE_HALF_X`]'s terms and the
/// same size: square, so the shadow it throws is as legible along the axis the
/// sun moves it on as the quad is across it.
const DOUBLE_PLATE_HALF_Z: f32 = DOUBLE_PLATE_HALF_X;

/// How far out along `x` the double-sided quad and its mirror stand, in world
/// units — thirty pixels of the golden frame, and a whole number for
/// [`DOUBLE_PLATE_HALF_X`]'s reason.
///
/// **The pair is symmetric about the frame's axis, and that is what makes the
/// mirror a reference rather than a second reading.** [`double_sun`] has no `x`
/// component and the quads' normals have none either, so under `x → -x` the
/// Lambert term is unchanged, the eye vector's `y` is unchanged and the
/// half-vector's `y` with it — every term of the lobe is the same number on
/// both sides.
///
/// Far enough out that the three quads do not touch: their projected half-width
/// is [`DOUBLE_PLATE_HALF_X`]'s eighteen pixels, so forty-four leaves eight
/// clear between the axis quad and each neighbour. Near enough in that the
/// outer edge at sixty-two pixels is inside the floor's own edge, which
/// [`DOUBLE_FLOOR_SCALE`] puts at about eighty-three.
const DOUBLE_QUAD_AT: f32 = 44.0 / DOUBLE_QUAD_PIXELS_PER_UNIT;

/// How far off a pixel boundary [`double_sided_quad_mesh`] lets a projected
/// vertex sit — [`SPECULAR_ALIGNMENT_SLACK`]'s thousandth of a pixel, for its
/// reason.
const DOUBLE_ALIGNMENT_SLACK: f32 = SPECULAR_ALIGNMENT_SLACK;

/// How much [`Scene::DoubleSided`] scales the cube by to get its floor —
/// [`ALPHA_FLOOR_SCALE`], for that constant's reasons.
const DOUBLE_FLOOR_SCALE: f32 = ALPHA_FLOOR_SCALE;

/// The base-colour factor [`Scene::DoubleSided`]'s floor shades through —
/// [`ALPHA_FLOOR_ALBEDO`]'s grey, so the floor's colour is the cube's own `+Y`
/// face and the quads are the only things in the frame carrying a hue.
const DOUBLE_FLOOR_ALBEDO: f32 = ALPHA_FLOOR_ALBEDO;

/// The base-colour factor all three quads shade through.
///
/// [`ALPHA_PLATE_TINT`]'s red against the floor's green, and for its reason: a
/// band on a quad and a band of floor seen where a quad was culled are then
/// separated by a factor in one channel rather than by a brightness a shading
/// change could also produce. `tests/render_e2e.rs` reads that channel.
const DOUBLE_QUAD_TINT: [f32; 4] = ALPHA_PLATE_TINT;

/// The roughness all three quads shade through.
///
/// **Rough on purpose.** The claim is that the double-sided quad and its mirror
/// read the same brightness, and the two are symmetric about the frame's axis
/// rather than at the same place — so the flatter the specular lobe, the less
/// any residual asymmetry in the frame can separate them. A near-mirror lobe
/// would put the highlight on one of them and not the other.
///
/// Under one, so the row is not the extreme the clamp in `mesh.slang` would
/// pin, and a dielectric — `UNTINTED`'s zero metallic — so the diffuse term
/// carries the band.
const DOUBLE_QUAD_ROUGHNESS: f32 = 0.9;

/// The transform that turns one of [`Scene::DoubleSided`]'s quads over, so its
/// front face and its normal both point up.
///
/// **A rotation, not a mirror.** A half turn about `z` written as a scale by
/// `(-1, -1, 1)`: its determinant is `+1`, so the winding is preserved and the
/// face still faces along its own normal, and every entry is exact in binary so
/// the turned quad's vertices land on the same pixel grid as the others. A
/// negative scale on one axis would flip the winding *and* need the cofactor
/// matrix to carry the normal, which is two ways for a fixture to be measuring
/// its own transform rather than the renderer.
///
/// The quad is centred on its own origin and square, so the turn moves no
/// vertex anywhere the untouched quad does not already reach.
fn double_sided_mirror() -> glam::Mat4 {
    glam::Mat4::from_scale(glam::Vec3::new(-1.0, -1.0, 1.0))
}

/// [`Scene::DoubleSided`]'s quad: one square in the plane `y = 0`, facing `-Y`.
///
/// **Both halves of "facing `-Y`" and they have to agree.** The authored normal
/// is `(0, -1, 0)`, which is what the lighting reads, and the corner order is
/// `crcbl_shaders::mesh`'s own `-Y` face — so the winding the rasteriser reads
/// puts the front face on the same side. A quad whose two disagreed would be
/// drawn by the cull mode of one side and lit by the normal of the other, and
/// nothing in the frame could say which.
///
/// **An open quad and not a slab**, which is what the shadow half of this
/// fixture needs: whichever way a closed solid is lit, some face of it is
/// front-facing to the light, so a cull mode cannot change its shadow. This has
/// one face, and it is turned away from the sun.
///
/// **The cluster carries its real normal cone**, which points the way the face
/// does: straight down, away from a camera above it. That is what makes the
/// mesh path's per-cluster cull the thing this fixture measures — the
/// amplification stage rejects a wholly back-facing cluster, so the
/// double-sided quad reaches the rasteriser only because
/// `mesh_cluster.slang`'s `cone_may_reject` reads the instance's material mode
/// and declines to run the cone test at all. A cone softened here would be a
/// fixture that draws the same frame whether or not that predicate exists.
fn double_sided_quad_mesh() -> crate::render::scene::MeshDesc<'static> {
    use crate::shaders::mesh::MeshVertex;
    use crate::shaders::meshlet::{MeshClusters, Meshlet};

    let range = crate::shaders::mesh::demo_uv_range();
    let tint = [1.0, 1.0, 1.0, 1.0];
    let normal = [0.0, -1.0, 0.0];
    // `crcbl_shaders::mesh`'s `-Y` face corner order, which `0 1 2, 0 2 3`
    // winds counter-clockwise seen from below.
    let corners = [
        [-DOUBLE_PLATE_HALF_X, 0.0, -DOUBLE_PLATE_HALF_Z],
        [DOUBLE_PLATE_HALF_X, 0.0, -DOUBLE_PLATE_HALF_Z],
        [DOUBLE_PLATE_HALF_X, 0.0, DOUBLE_PLATE_HALF_Z],
        [-DOUBLE_PLATE_HALF_X, 0.0, DOUBLE_PLATE_HALF_Z],
    ];
    let vertices: Vec<MeshVertex> = corners
        .iter()
        .zip(&SPECULAR_QUAD_UV)
        .map(|(corner, uv)| MeshVertex::from_normal(*corner, normal, tint, *uv, &range))
        .collect();

    // Every vertex of every placed quad on an integer pixel of the golden
    // frame, checked through the scene's own camera and each instance's own
    // model matrix rather than re-derived from the constants above — which is
    // the check `SPECULAR_STRIP_PITCH` argues for, and the mirror's turn is one
    // of the transforms it has to hold under.
    let clip = double_camera().view_projection(DOUBLE_ALIGNED_COLUMNS / DOUBLE_ALIGNED_ROWS);
    for model in double_sided_quads().map(|(_, model)| model) {
        for corner in &corners {
            let projected = clip * model * glam::Vec4::new(corner[0], corner[1], corner[2], 1.0);
            let ndc = projected.truncate() / projected.w;
            for (coordinate, extent, axis) in [
                (ndc.x, DOUBLE_ALIGNED_COLUMNS, "column"),
                (ndc.y, DOUBLE_ALIGNED_ROWS, "row"),
            ] {
                let frame = (coordinate * 0.5 + 0.5) * extent;
                // A full assertion rather than a debug one, on
                // `specular_plate_mesh`'s terms: the browser gate draws this
                // scene through a second rasteriser in release.
                assert!(
                    (frame - frame.round()).abs() <= DOUBLE_ALIGNMENT_SLACK,
                    "a quad has a vertex at {axis} {frame}, off the pixel grid, so two \
                     rasterisers may put its edge in different pixels: see \
                     DOUBLE_PLATE_HALF_X"
                );
            }
        }
    }

    let mut clusters = MeshClusters::default();
    // `crcbl_scene::meshlet`'s own rule for this cluster, spelled out because
    // this mesh is authored here rather than cooked: the axis is the
    // area-weighted average of the triangles' normals, which for one flat quad
    // is the face normal, and the cutoff is the smallest dot of that with them,
    // which is one. `specular_cluster_bounds` writes that cutoff already and an
    // axis of `+Y`, which is the plate it is named for rather than this quad.
    let bounds = crate::shaders::meshlet::ClusterBounds {
        cone_axis: normal,
        ..specular_cluster_bounds(&corners)
    };
    clusters.clusters.push(
        Meshlet::new(0, 4, 0, 2, bounds)
            .unwrap_or_else(|error| unreachable!("four vertices of fixture geometry: {error}")),
    );
    clusters.vertices.extend_from_slice(&[0, 1, 2, 3]);
    clusters.corners.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    clusters
        .check(vertices.len())
        .unwrap_or_else(|fault| unreachable!("the fixture's own cluster is in range: {fault}"));

    crate::render::scene::MeshDesc {
        label: std::borrow::Cow::Borrowed("double sided quad"),
        geometry: crate::render::scene::Geometry::Flat {
            vertices: std::borrow::Cow::Owned(crate::shaders::mesh::vertex_bytes(&vertices)),
            uv_range: range,
            indices: std::borrow::Cow::Owned(vec![0u32, 1, 2, 0, 2, 3]),
            clusters,
            // No authored tangents, on `specular_plate_mesh`'s terms.
            flags: 0,
        },
    }
}

/// The three quads [`Scene::DoubleSided`] places, as the material row each
/// wears and the transform it wears it at.
///
/// One list because two readers need exactly the same one: the scene builder
/// places them, and [`double_sided_quad_mesh`] projects each corner through
/// each of these matrices to check the pixel alignment. A second copy is a
/// second chance for a transform to move in one and not the other.
///
/// The order is left to right down the `x` axis as the world spells it, which
/// the overhead camera puts **right to left** across the frame — see
/// `tests/render_e2e.rs`'s reader, which is where each band's column comes
/// from.
fn double_sided_quads() -> impl Iterator<Item = (usize, glam::Mat4)> {
    [
        // The double-sided one, out along `-x`.
        (
            DOUBLE_DOUBLE,
            glam::Mat4::from_translation(glam::Vec3::new(-DOUBLE_QUAD_AT, DOUBLE_QUAD_UP, 0.0)),
        ),
        // The culled one, on the axis: a single-sided row and the mesh as
        // authored, so both the camera and the sun see its back.
        (
            DOUBLE_SINGLE,
            glam::Mat4::from_translation(glam::Vec3::new(0.0, DOUBLE_QUAD_UP, 0.0)),
        ),
        // The mirror, out along `+x`: the same single-sided row, turned over.
        (
            DOUBLE_SINGLE,
            glam::Mat4::from_translation(glam::Vec3::new(DOUBLE_QUAD_AT, DOUBLE_QUAD_UP, 0.0))
                * double_sided_mirror(),
        ),
    ]
    .into_iter()
}

/// [`Scene::DoubleSided`]'s floor: the cube at [`DOUBLE_FLOOR_SCALE`], dropped
/// so its `+Y` face is the plane `y = 0` — [`alpha_floor`]'s shape at this
/// scene's own scale.
fn double_floor() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.5 * DOUBLE_FLOOR_SCALE, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::splat(DOUBLE_FLOOR_SCALE))
}

/// The camera [`Scene::DoubleSided`] is drawn with — [`alpha_camera`]'s, at
/// this scene's own height, and straight down for that camera's reason.
fn double_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, DOUBLE_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// The sun [`Scene::DoubleSided`] runs under — [`alpha_sun`]'s, and for its
/// reason: no `x` component, so each quad's shadow is that quad translated
/// along `-z` by exactly its height and sits directly down-frame of it.
///
/// It is also what puts the sun on the **camera's** side of every quad, which is
/// the whole of the lighting claim: a quad whose authored normal points away
/// from both has a negative Lambert term until something reverses it.
fn double_sun() -> crcbl_render::DirectionalLight {
    crcbl_render::DirectionalLight {
        direction: glam::Vec3::new(0.0, 1.0, 1.0).normalize(),
        ..crcbl_render::DirectionalLight::default()
    }
}

/// [`Scene::DoubleSided`]'s scene: the engine's own, with the quad mesh and the
/// three material rows appended.
///
/// A description of its own on [`specular_aa_scene`]'s terms — this fixture
/// needs a mesh and materials the demo scene has not got — and the two quad rows
/// differ in one flags word, which is what the frame is a comparison of.
fn double_sided_scene() -> crate::render::scene::SceneDesc<'static> {
    use crate::shaders::mesh::GpuMaterial;

    let mut scene = crate::render::scene::demo();
    scene.meshes.push(double_sided_quad_mesh());
    debug_assert_eq!(
        scene.meshes.len() - 1,
        DOUBLE_QUAD_MESH,
        "the quad is the mesh past the demo scene's four"
    );
    scene.materials.push(GpuMaterial {
        base_color: [
            DOUBLE_FLOOR_ALBEDO,
            DOUBLE_FLOOR_ALBEDO,
            DOUBLE_FLOOR_ALBEDO,
            1.0,
        ],
        ..GpuMaterial::UNTINTED
    });
    debug_assert_eq!(
        scene.materials.len() - 1,
        DOUBLE_FLOOR,
        "the floor row is the one past the demo scene's three"
    );
    let quad = GpuMaterial {
        base_color: DOUBLE_QUAD_TINT,
        roughness: DOUBLE_QUAD_ROUGHNESS,
        ..GpuMaterial::UNTINTED
    };
    scene.materials.push(quad);
    debug_assert_eq!(scene.materials.len() - 1, DOUBLE_SINGLE);
    scene.materials.push(GpuMaterial {
        // The whole of what makes this scene a fixture, and the only field that
        // differs from the row above: without the bit the row is an ordinary
        // single-sided one, its quad is culled in every pass, and the frame
        // shows floor where the evidence should be.
        flags: GpuMaterial::DOUBLE_SIDED,
        ..quad
    });
    debug_assert_eq!(scene.materials.len() - 1, DOUBLE_DOUBLE);
    scene
}

// ---------------------------------------------------------------------------
// The double-sided updater fixture
// ---------------------------------------------------------------------------

/// How much [`double_bounce_quad`] scales [`double_sided_quad_mesh`] by, so the
/// square stands nine tenths of the room's height along each of its own axes.
///
/// **Uniform**, which is what lets the fixture turn the quad over with a
/// rotation and leave the authored normal a unit vector on both arms — a
/// non-uniform scale would need the cofactor matrix to carry the normal and the
/// fixture would be measuring that instead of the reflective shadow map.
///
/// Nine tenths rather than the whole: the quad hangs clear of the floor, the
/// ceiling's opening and both `±Z` walls, so no edge of it is coincident with a
/// wall the depth buffer would then have to separate it from.
const DOUBLE_BOUNCE_SCALE: f32 = 0.9 * LEAK_ROOM_HEIGHT / (2.0 * DOUBLE_PLATE_HALF_X);

/// How far in from the `+X` wall's inner face [`double_bounce_quad`] stands, in
/// world units.
///
/// The quad has no thickness, so it needs a gap the depth buffer can resolve
/// against the wall behind it — [`BOUNCE_PANEL_THICKNESS`]'s reasoning for a
/// surface that has none of its own — and small enough that the sun reaching
/// the wall past its edges is the same sliver on both arms.
const DOUBLE_BOUNCE_GAP: f32 = 0.05;

/// The base-colour factor [`double_bounce_forward`]'s quad reflects the sun
/// with — [`BOUNCE_PANEL_GAIN`]'s red, and for its reasons.
const DOUBLE_BOUNCE_GAIN: f32 = BOUNCE_PANEL_GAIN;

/// Where [`double_bounce_forward`] stands its quad, and which way round.
///
/// **One square in one place, turned two ways.** [`double_sided_quad_mesh`]'s
/// authored normal is `-Y`, so a quarter turn about `z` points it at `+X` — into
/// the `+X` wall, away from [`bounce_sun`] — and the opposite quarter turn
/// points it at `-X`, into the sun. The quad is square and centred on its own
/// origin, so the two turns put its four corners on exactly the same four
/// points: what differs between the arms is the normal the lighting reads, the
/// winding the rasteriser reads, and nothing else.
///
/// Both turns are rotations rather than mirrors — determinant `+1` — so the
/// face still faces along its own normal on both arms, which is
/// [`double_sided_mirror`]'s argument at this fixture's scale.
fn double_bounce_quad(mirror: bool) -> glam::Mat4 {
    let turn = if mirror {
        -std::f32::consts::FRAC_PI_2
    } else {
        std::f32::consts::FRAC_PI_2
    };
    glam::Mat4::from_translation(glam::Vec3::new(
        0.5 * LEAK_ROOM_WIDTH - DOUBLE_BOUNCE_GAP,
        0.5 * LEAK_ROOM_HEIGHT,
        0.0,
    )) * glam::Mat4::from_rotation_z(turn)
        * glam::Mat4::from_scale(glam::Vec3::splat(DOUBLE_BOUNCE_SCALE))
}

/// **The double-sided updater fixture**: one sunlit red quad against the `+X`
/// wall of [`probe_bounce_forward`]'s room, standing either back to the sun with
/// [`GpuMaterial::DOUBLE_SIDED`] set or face to it as an ordinary single-sided
/// surface, and two probes the reflective shadow map fills every frame.
///
/// **What it is for.** `mesh.slang`'s `rsmFragmentMain` calls
/// `double_sided_normal` before it records a patch's normal, and
/// `probe_gather.slang`'s `gather_patch` drops any patch whose recorded normal
/// faces away from the probe — `facing = dot(sample_normal, -direction)`, and a
/// patch with `facing <= 0` returns before it is weighed. So a build whose
/// reflective shadow map wrote the *unreversed* normal records this quad facing
/// into the wall, every probe in the room drops it, and the room loses the only
/// red in it. Nothing in the tree drew that: `Scene::DoubleSided` has no probe
/// volume, and `probe_bounce_forward`'s panel is a closed slab whose sunlit face
/// is front-facing whatever the cull mode is.
///
/// **The two arms are the same square in the same place**, which is what makes
/// the comparison a claim about the reversal rather than about two scenes: see
/// `double_bounce_quad`. The single-sided arm's quad faces the sun and needs no
/// reversal at all, so it is the reading the double-sided arm has to match.
///
/// **What is measured is the probe term alone** — `bounce_camera`'s frame, for
/// its reasons: the camera looks straight at the `-X` wall's inner face, which
/// `bounce_sun` never touches, the sun carries no ambient and the sky is
/// off. There is no divider here, because visibility is not what this fixture is
/// about and a wall between the quad and the measured face would take the whole
/// of the reading on both arms.
///
/// [`GpuMaterial::DOUBLE_SIDED`]: crate::shaders::mesh::GpuMaterial::DOUBLE_SIDED
///
/// # Errors
///
/// [`OffscreenError::Hal`] if the renderer cannot be built or the capture cannot
/// be uploaded.
pub fn double_bounce_forward(
    device: &dyn Device,
    queue: QueueHandle,
    format: Format,
    mirror: bool,
) -> Result<ForwardScene, OffscreenError> {
    use crate::shaders::mesh::GpuMaterial;

    let probes = probe_bounce_grid();
    let mut scene = crate::render::scene::demo();
    scene.capacities.probes = probes.volume.total();
    scene.probes = probes;
    scene.meshes.push(double_sided_quad_mesh());
    let quad_mesh = scene.meshes.len() - 1;
    // Appended rather than edited in place, on `probe_bounce_forward`'s terms.
    // Two rows differing in one flags word, on `double_sided_scene`'s: the same
    // colour and the same roughness, so the two arms cannot differ in what the
    // quad reflects.
    let lit = GpuMaterial {
        base_color: [DOUBLE_BOUNCE_GAIN, 0.0, 0.0, 1.0],
        ..GpuMaterial::UNTINTED
    };
    scene.materials.push(lit);
    let single = scene.materials.len() - 1;
    scene.materials.push(GpuMaterial {
        flags: GpuMaterial::DOUBLE_SIDED,
        ..lit
    });
    let double = scene.materials.len() - 1;

    let mut renderer = ForwardRenderer::with_scene(device, queue, format, &scene)?;
    // On `probe_bounce_forward`'s terms: the measured pixels are the diffuse
    // probe term, and a rough surface's reflection would put specular into them.
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(crcbl_render::RenderEffects::REFLECTIONS, Some(false)),
        ..EffectRequest::default()
    });
    renderer.set_sky(crcbl_render::Sky::NONE);
    place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, leak_room());
    place(
        &mut renderer,
        quad_mesh,
        if mirror { single } else { double },
        double_bounce_quad(mirror),
    );
    // After the geometry and before the frame, on `probe_bounce_forward`'s
    // terms: the gather weighs every map texel by the visibility this records.
    renderer.capture_probe_visibility(device, queue)?;
    Ok(ForwardScene {
        camera: bounce_camera(),
        sun: bounce_sun(),
        renderer: Box::new(renderer),
    })
}

/// How many quad strips [`Scene::SpecularAa`] cuts the corrugated half of its
/// plate into.
///
/// A multiple of [`SPECULAR_STRIPS_PER_CLUSTER`], so every cluster below is
/// full and none of them is a remainder with its own bounds arithmetic.
///
/// The count is what sets the plate's width, because a strip's own width is
/// fixed by [`SPECULAR_STRIP_PITCH`]: at this many the plate covers 186 of the
/// golden frame's 256 columns and leaves a margin of unwritten pixels either
/// side, which is the region `tests/render_e2e.rs` reads its background level
/// out of.
///
/// Two pixels a strip makes the corrugation's period eight, so the normal turns
/// through [`SPECULAR_SWING`] over two pixels — far inside the specular lobe's
/// own width, which is the aliasing `mesh.slang`'s `specular_aa_kernel` exists
/// for.
const SPECULAR_STRIPS: usize = 93;

/// The frame height every pixel figure in this fixture is arithmetic for.
///
/// The golden suite draws this scene at 256×192, so [`SPECULAR_STRIP_PITCH`]'s
/// alignment is a statement about *that* frame. Asked for another size the
/// plate still draws correctly, but its strips no longer land on pixel
/// boundaries; the golden, which is what the alignment exists for, is always
/// this one.
const SPECULAR_ALIGNED_ROWS: f32 = 192.0;

/// How many pixels of that frame one world unit of the plate covers.
///
/// Half the rows over [`SPECULAR_HALF_HEIGHT`], which is the figure
/// [`specular_camera`] derives its field of view from — and it holds
/// horizontally too, because the projection's aspect is the frame's own and so
/// its pixels are square.
const SPECULAR_ALIGNED_PIXELS_PER_UNIT: f32 = SPECULAR_ALIGNED_ROWS * 0.5 / SPECULAR_HALF_HEIGHT;

/// How wide one corrugation strip is, in world units.
///
/// **Two pixels exactly, with every strip edge on an integer pixel column. That
/// is a portability property, not a tidiness one.**
///
/// The plate lies in a single plane at a constant distance under a camera
/// looking straight down at it, so its projection is affine: a point at world
/// `x` lands at frame column `columns / 2 - x *
/// SPECULAR_ALIGNED_PIXELS_PER_UNIT`, with no perspective divide to make the
/// spacing uneven. Two pixels is therefore `2 /
/// SPECULAR_ALIGNED_PIXELS_PER_UNIT` of world. The plate is centred on the
/// frame's axis and cut into whole strips, so strip `k`'s edge sits at column
/// `columns / 2 + SPECULAR_STRIPS - 2 * k`: an integer for every `k`, which
/// puts each pixel centre a quarter and three quarters of the way across its
/// strip.
///
/// **Why that matters.** Vulkan guarantees only four `subPixelPrecisionBits`,
/// so a rasteriser is entitled to snap a vertex to a sixteenth of a pixel; radv
/// carries eight bits and SwiftShader four. An edge at an arbitrary fraction
/// therefore lands up to a sixteenth of a pixel apart on the two, and on this
/// plate's lobe flank — tens of levels per pixel — that is a difference a
/// golden can see. An edge already on an integer coordinate snaps to the same
/// point on any of those grids. [`specular_plate_mesh`] asserts the property
/// through the scene's own camera rather than trusting the arithmetic, so a
/// later change to the camera or the extent fails loudly instead of reappearing
/// as a cross-backend diff.
const SPECULAR_STRIP_PITCH: f32 = 2.0 / SPECULAR_ALIGNED_PIXELS_PER_UNIT;

/// The frame width that goes with [`SPECULAR_ALIGNED_ROWS`].
const SPECULAR_ALIGNED_COLUMNS: f32 = 256.0;

/// How far off a pixel boundary [`specular_plate_mesh`] lets a vertex sit.
///
/// A thousandth of a pixel: far under the sixteenth
/// [`SPECULAR_STRIP_PITCH`] is about, and far over the rounding of the two
/// multiplications that get there.
const SPECULAR_ALIGNMENT_SLACK: f32 = 1e-3;

/// How many strips one cluster of that plate carries.
///
/// A strip adds one vertex column of two vertices, so a run of `n` strips
/// spans `2 * (n + 1)` vertices: 31 is the largest run that fits
/// `crcbl_shaders::meshlet::MAX_CLUSTER_VERTICES`, and its 62 triangles are
/// well inside `MAX_CLUSTER_TRIANGLES`.
const SPECULAR_STRIPS_PER_CLUSTER: usize = 31;

/// Half [`Scene::SpecularAa`]'s plate along `x`, the axis the corrugation runs
/// across.
///
/// The strips' own total width, so it is derived rather than chosen: the
/// alignment [`SPECULAR_STRIP_PITCH`] argues for holds only if the plate is
/// exactly the strips it is cut into. It comes out inside the frame's own half
/// width at this camera — [`SPECULAR_HALF_HEIGHT`] times the aspect is
/// `1.333` — leaving the margin `tests/render_e2e.rs` reads its background
/// level out of.
const SPECULAR_PLATE_HALF_X: f32 = SPECULAR_STRIPS as f32 * SPECULAR_STRIP_PITCH * 0.5;

/// How far from the plate's centre line each of its two bands starts.
///
/// The bands are separate quads with their own vertices, so the flat one's
/// normal cannot bleed into the corrugated one's across a shared edge — which
/// would put a kernel of its own into the control band's first rows. This is
/// the gap between them, and it is a few pixels rather than none so that the
/// seam is visible in the golden.
///
/// A whole number of pixels off the axis, in [`SPECULAR_STRIP_PITCH`]'s terms:
/// the `z` edges are the ones the rasteriser snaps vertically, and the same
/// sixteenth-of-a-pixel argument applies to them.
const SPECULAR_BAND_INNER_Z: f32 = 6.0 / SPECULAR_ALIGNED_PIXELS_PER_UNIT;

/// How far each band reaches away from that centre line — a whole number of
/// pixels again, and inside [`SPECULAR_HALF_HEIGHT`] so both bands are wholly
/// on screen.
const SPECULAR_BAND_OUTER_Z: f32 = 84.0 / SPECULAR_ALIGNED_PIXELS_PER_UNIT;

/// How far the corrugation swings [`Scene::SpecularAa`]'s authored normal
/// either side of [`SPECULAR_MIRROR_TILT`], in radians.
///
/// **The authored normals do not match the flat geometry, and that is the
/// fixture.** A page cannot do this job — `crcbl_render::scene::PAGE_EXTENT` is
/// two texels a side and the trilinear sampler mips a tiled page of that size
/// to flat — so the corrugation is in the vertex normals of a plate that is
/// geometrically one plane.
///
/// Wide enough that the swing carries the mirror direction past the sun and
/// back twice per period, so every period holds two highlight streaks and the
/// pixels between them are the lobe's floor.
const SPECULAR_SWING: f32 = 0.436_332_3;

/// Which way [`Scene::SpecularAa`]'s sun stands, in radians from straight up,
/// tilted in the `x` plane alone.
///
/// No `z` component, on [`alpha_sun`]'s terms: the corrugation runs across `x`,
/// so a sun with a `z` component would make the mirror condition a function of
/// the row as well as the column and no band could be read as one population.
const SPECULAR_SUN_TILT: f32 = 0.436_332_3;

/// The tilt of the normal that reflects this camera into that sun: half
/// [`SPECULAR_SUN_TILT`].
///
/// The eye looks straight down and the sun stands at `SPECULAR_SUN_TILT`, so
/// the half vector between them is at half that angle and a surface whose
/// normal is there sits on the lobe's peak. The corrugation swings about it.
const SPECULAR_MIRROR_TILT: f32 = SPECULAR_SUN_TILT * 0.5;

/// How far off that peak [`Scene::SpecularAa`]'s flat control band is tilted,
/// in radians.
///
/// **The control band has to be specular-bright without being clipped**, which
/// is what makes its byte-for-byte stability evidence rather than decoration: a
/// band sitting at the peak saturates to white and would read the same however
/// wide the lobe got, and a band on the lobe's floor is a dark region a
/// widening cannot move either. This puts it on the shoulder, where the same
/// kernel the corrugated band gets would visibly darken it — see
/// `tests/render_e2e.rs`, which measures exactly that.
const SPECULAR_CONTROL_OFFSET: f32 = 0.12;

/// How high above the plate [`Scene::SpecularAa`]'s camera stands.
///
/// **A long lens, and it is load-bearing.** `mesh.slang` builds `to_eye` from
/// the camera's position per fragment, so a wide-angle overhead camera puts a
/// different mirror direction under every column of the plate and a band across
/// it would be a gradient rather than a population. At this distance the view
/// direction varies by under two degrees corner to corner, so every strip meets
/// the sun on the same terms and the two bands are comparable to each other.
const SPECULAR_CAMERA_UP: f32 = 40.0;

/// How high the plate itself floats.
///
/// Off the origin so the camera distance above is a distance to the *plate*,
/// and low enough that it changes nothing else: nothing is under it and its
/// shadow falls on nothing.
const SPECULAR_PLATE_UP: f32 = 0.75;

/// Half the world height [`Scene::SpecularAa`]'s frame covers at the plate.
///
/// The camera's field of view is derived from this and the distance rather than
/// written down, so `tests/render_e2e.rs` can turn a plate coordinate into a
/// pixel by one multiplication: the frame's half height in pixels over this.
const SPECULAR_HALF_HEIGHT: f32 = 1.0;

/// [`Scene::SpecularAa`]'s plate in [`crate::render::scene::SceneDesc::meshes`]
/// — the entry past the demo scene's four.
const SPECULAR_PLATE_MESH: usize = 4;

/// Its material row, the one past the demo scene's three.
const SPECULAR_MATERIAL: usize = 3;

/// The base-colour factor [`Scene::SpecularAa`]'s plate shades through, and it
/// is a **conductor**.
///
/// `metallic` of one is what makes this frame an instrument: a metal has no
/// diffuse lobe at all, so every lit pixel of the plate is the specular term
/// and nothing else — no albedo floor for a firefly to be measured against, and
/// no ambient, because the ambient sum multiplies the diffuse albedo a
/// conductor does not have. The colour is a neutral steel, so the three
/// channels carry the same signal and a luminance is the lobe.
const SPECULAR_PLATE_TINT: [f32; 4] = [0.55, 0.58, 0.62, 1.0];

/// The roughness that plate shades at.
///
/// **Swept rather than picked**, against the two things the frame has to do at
/// once: a lobe narrow enough that the corrugation undersamples it, and a peak
/// the `Rgba16Float` target can still separate from its neighbours after the
/// tonemap. See `docs/plan/44-lighting.md`'s rung 4 for the measured numbers.
const SPECULAR_ROUGHNESS: f32 = 0.18;

/// How bright [`Scene::SpecularAa`]'s sun is, in every channel.
///
/// **Far under the default sun's, and the dimming is what makes the frame an
/// instrument.** A specular lobe's peak is `1 / alpha2`, which at
/// [`SPECULAR_ROUGHNESS`] is in the hundreds; under a key light of ordinary
/// strength every pixel of both bands clips to white and a widened lobe draws
/// the same picture as a narrow one. Swept so that the regularised corrugated
/// band sits below the swapchain's ceiling while the unregularised one runs
/// past it — which is the difference `tests/render_e2e.rs` measures.
///
/// Neutral rather than the default sun's warm tint, so the three channels carry
/// one signal and a luminance over the band is the lobe itself.
const SPECULAR_SUN_LEVEL: f32 = 0.24;

/// The four texture coordinates every quad of [`specular_plate_mesh`] carries,
/// in the corner order the `+Y` face of `crcbl_shaders::mesh::FACES` uses.
///
/// The plate names no page, so nothing samples through these — they are here
/// because a vertex has a UV lane and [`demo_uv_range`] is what it is quantised
/// against, and the unit square is the range that module declares.
///
/// [`demo_uv_range`]: crcbl_shaders::mesh::demo_uv_range
const SPECULAR_QUAD_UV: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// The bounding sphere and normal cone of a run of this plate's vertices.
///
/// Every triangle of the plate is in the plane `y = 0` and wound the same way,
/// so the cone is exact rather than conservative: one axis, and a cutoff of one
/// meaning "every triangle faces exactly here". The **geometric** normal, which
/// is what a cluster cone is about — the authored normals swing either side of
/// it and no cull reads them.
fn specular_cluster_bounds(positions: &[[f32; 3]]) -> crate::shaders::meshlet::ClusterBounds {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for position in positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    let center = [
        0.5 * (min[0] + max[0]),
        0.5 * (min[1] + max[1]),
        0.5 * (min[2] + max[2]),
    ];
    let mut radius: f32 = 0.0;
    for position in positions {
        let offset = [
            position[0] - center[0],
            position[1] - center[1],
            position[2] - center[2],
        ];
        radius = radius
            .max((offset[0] * offset[0] + offset[1] * offset[1] + offset[2] * offset[2]).sqrt());
    }
    crate::shaders::meshlet::ClusterBounds {
        center,
        radius,
        cone_axis: [0.0, 1.0, 0.0],
        cone_cutoff: 1.0,
    }
}

/// How far the corrugation's authored normal is from
/// [`SPECULAR_MIRROR_TILT`] at vertex column `column`, as a multiple of
/// [`SPECULAR_SWING`].
///
/// A four-column zigzag — level, up, level, down — so the normal crosses the
/// mirror direction twice a period and every period holds two highlight
/// streaks. Sampled at the vertices and interpolated between them, which is
/// what makes the derivative the kernel reads constant inside a strip and
/// exactly what the paper's filter is written for.
fn specular_swing_at(column: usize) -> f32 {
    match column % 4 {
        1 => 1.0,
        3 => -1.0,
        _ => 0.0,
    }
}

/// [`Scene::SpecularAa`]'s plate: a flat control quad and
/// [`SPECULAR_STRIPS`] corrugated strips, in one mesh a description can carry.
///
/// **Geometrically one plane.** Every vertex is at `y = 0` and every triangle
/// faces `+Y`; what differs between the two halves is the *authored* normal —
/// constant across the control quad, swinging by [`SPECULAR_SWING`] every strip
/// across the corrugated run. So `mesh.slang`'s `geometric_normal_of` reads one
/// facet over the whole plate while `specular_aa_kernel` reads a normal field
/// that turns faster than the pixel grid samples it, which is the one case the
/// rung is about.
fn specular_plate_mesh() -> crate::render::scene::MeshDesc<'static> {
    use crate::shaders::mesh::MeshVertex;
    use crate::shaders::meshlet::{MeshClusters, Meshlet};

    let range = crate::shaders::mesh::demo_uv_range();
    let tint = [1.0, 1.0, 1.0, 1.0];
    let normal_at = |tilt: f32| [tilt.sin(), tilt.cos(), 0.0];

    // The control quad first, so its four vertices are 0 to 3 and every
    // corrugated column below is at a known offset past them.
    let control = normal_at(SPECULAR_MIRROR_TILT - SPECULAR_CONTROL_OFFSET);
    let control_corners = [
        [-SPECULAR_PLATE_HALF_X, 0.0, -SPECULAR_BAND_INNER_Z],
        [SPECULAR_PLATE_HALF_X, 0.0, -SPECULAR_BAND_INNER_Z],
        [SPECULAR_PLATE_HALF_X, 0.0, -SPECULAR_BAND_OUTER_Z],
        [-SPECULAR_PLATE_HALF_X, 0.0, -SPECULAR_BAND_OUTER_Z],
    ];
    let mut positions = Vec::with_capacity(4 + 2 * (SPECULAR_STRIPS + 1));
    let mut vertices = Vec::with_capacity(4 + 2 * (SPECULAR_STRIPS + 1));
    for (corner, uv) in control_corners.iter().zip(&SPECULAR_QUAD_UV) {
        positions.push(*corner);
        vertices.push(MeshVertex::from_normal(*corner, control, tint, *uv, &range));
    }

    // Then the corrugated run: two vertices a column, the far one first, so a
    // strip's four corners are two consecutive columns and the cluster corners
    // below are arithmetic rather than a table.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a few hundred strips, each exactly representable"
    )]
    for column in 0..=SPECULAR_STRIPS {
        // `(2k - strips) / pixels-per-unit`, which is
        // `-SPECULAR_PLATE_HALF_X + k * SPECULAR_STRIP_PITCH` written so the
        // numerator is a small integer and there is one rounded divide: a pitch
        // added up a column at a time would leave the last edge less exact than
        // the first, and it is the edges the two rasterisers have to agree
        // about.
        let x = (2.0 * column as f32 - SPECULAR_STRIPS as f32) / SPECULAR_ALIGNED_PIXELS_PER_UNIT;
        let normal = normal_at(SPECULAR_MIRROR_TILT + SPECULAR_SWING * specular_swing_at(column));
        for (z, uv) in [
            (SPECULAR_BAND_OUTER_Z, SPECULAR_QUAD_UV[0]),
            (SPECULAR_BAND_INNER_Z, SPECULAR_QUAD_UV[3]),
        ] {
            let position = [x, 0.0, z];
            positions.push(position);
            vertices.push(MeshVertex::from_normal(position, normal, tint, uv, &range));
        }
    }

    // Every vertex on an integer pixel of the golden frame, checked rather
    // than trusted — the whole of `SPECULAR_STRIP_PITCH`'s argument, and the
    // kind of property a later edit to the camera or the extent breaks without
    // breaking anything that looks related. **Through the scene's own camera
    // and model matrix**, not through the constants the pitch was derived
    // from: a check that re-derives the pixel from `SPECULAR_ALIGNED_PIXELS_PER_UNIT`
    // passes for any field of view, and the field of view is exactly what a
    // later edit would move.
    let clip = specular_camera().view_projection(SPECULAR_ALIGNED_COLUMNS / SPECULAR_ALIGNED_ROWS)
        * specular_plate();
    for position in &positions {
        let projected = clip * glam::Vec4::new(position[0], position[1], position[2], 1.0);
        let ndc = projected.truncate() / projected.w;
        for (coordinate, extent, axis) in [
            (ndc.x, SPECULAR_ALIGNED_COLUMNS, "column"),
            (ndc.y, SPECULAR_ALIGNED_ROWS, "row"),
        ] {
            // Which way the axis runs does not matter to whether the result is
            // whole; the viewport maps `-1..1` onto `0..extent` either way.
            let frame = (coordinate * 0.5 + 0.5) * extent;
            // A full assertion rather than a debug one: the release build is
            // where this matters, because the browser gate renders the scene
            // through a second rasteriser in release and a drifted edge shows
            // up there as an unexplained golden diff.
            assert!(
                (frame - frame.round()).abs() <= SPECULAR_ALIGNMENT_SLACK,
                "the plate has a vertex at {axis} {frame}, off the pixel grid, \
                 so two rasterisers may put its edge in different pixels: see \
                 SPECULAR_STRIP_PITCH"
            );
        }
    }

    // `0 1 2, 0 2 3` per quad, preserving the counter-clockwise corner order
    // `crcbl_shaders::mesh::cube_indices` uses for the same `+Y` facing.
    let mut indices = vec![0u32, 1, 2, 0, 2, 3];
    for strip in 0..SPECULAR_STRIPS {
        let far = 4 + 2 * strip as u32;
        let (near, next_far, next_near) = (far + 1, far + 2, far + 3);
        indices.extend_from_slice(&[far, next_far, next_near, far, next_near, near]);
    }

    // The control quad is a cluster of its own and the corrugated run is cut
    // into full chunks — see `SPECULAR_STRIPS_PER_CLUSTER`, which is what makes
    // both the vertex count and the triangle count fit a meshlet.
    let mut clusters = MeshClusters::default();
    let too_large = |error| unreachable!("a few hundred vertices of fixture geometry: {error}");
    clusters.clusters.push(
        Meshlet::new(0, 4, 0, 2, specular_cluster_bounds(&positions[..4]))
            .unwrap_or_else(too_large),
    );
    clusters.vertices.extend_from_slice(&[0, 1, 2, 3]);
    clusters.corners.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    for chunk in 0..SPECULAR_STRIPS / SPECULAR_STRIPS_PER_CLUSTER {
        let first = chunk * SPECULAR_STRIPS_PER_CLUSTER;
        let vertex_offset = clusters.vertices.len();
        let triangle_offset = clusters.corners.len();
        let mut run = Vec::with_capacity(2 * (SPECULAR_STRIPS_PER_CLUSTER + 1));
        for column in first..=first + SPECULAR_STRIPS_PER_CLUSTER {
            run.push(4 + 2 * column as u32);
            run.push(4 + 2 * column as u32 + 1);
        }
        for strip in 0..SPECULAR_STRIPS_PER_CLUSTER {
            let far = u8::try_from(2 * strip)
                .unwrap_or_else(|_| unreachable!("a cluster holds at most 256 vertices"));
            let (near, next_far, next_near) = (far + 1, far + 2, far + 3);
            clusters
                .corners
                .extend_from_slice(&[far, next_far, next_near, far, next_near, near]);
        }
        let span: Vec<[f32; 3]> = run
            .iter()
            .map(|&vertex| positions[vertex as usize])
            .collect();
        clusters.clusters.push(
            Meshlet::new(
                vertex_offset,
                run.len(),
                triangle_offset,
                // Two triangles a strip, which is what the corner run above
                // just wrote — a count in strips would draw half of every
                // cluster and leave the other half to the index buffer alone,
                // so only the mesh path would lose it.
                2 * SPECULAR_STRIPS_PER_CLUSTER,
                specular_cluster_bounds(&span),
            )
            .unwrap_or_else(too_large),
        );
        clusters.vertices.extend_from_slice(&run);
    }
    clusters
        .check(vertices.len())
        .unwrap_or_else(|fault| unreachable!("the fixture's own clusters are in range: {fault}"));

    crate::render::scene::MeshDesc {
        label: std::borrow::Cow::Borrowed("specular aa plate"),
        geometry: crate::render::scene::Geometry::Flat {
            vertices: std::borrow::Cow::Owned(crate::shaders::mesh::vertex_bytes(&vertices)),
            uv_range: range,
            indices: std::borrow::Cow::Owned(indices),
            clusters,
            // No authored tangents: `MeshVertex::from_normal` fills the frame
            // with the stand-in basis, on `crcbl_render::scene::demo`'s terms.
            flags: 0,
        },
    }
}

/// [`Scene::SpecularAa`]'s scene: the engine's own, with the plate and its
/// conductor row appended.
///
/// A description of its own on [`alpha_mask_scene`]'s terms — this fixture
/// needs a mesh and a material the demo scene has not got — and it is the only
/// description in this module that appends a **mesh**, because it is the only
/// fixture whose subject is a vertex normal.
fn specular_aa_scene() -> crate::render::scene::SceneDesc<'static> {
    let mut scene = crate::render::scene::demo();
    scene.meshes.push(specular_plate_mesh());
    debug_assert_eq!(
        scene.meshes.len() - 1,
        SPECULAR_PLATE_MESH,
        "the plate is the mesh past the demo scene's four"
    );
    scene.materials.push(crate::shaders::mesh::GpuMaterial {
        base_color: SPECULAR_PLATE_TINT,
        metallic: 1.0,
        roughness: SPECULAR_ROUGHNESS,
        ..crate::shaders::mesh::GpuMaterial::UNTINTED
    });
    debug_assert_eq!(
        scene.materials.len() - 1,
        SPECULAR_MATERIAL,
        "the conductor row is the one past the demo scene's three"
    );
    scene
}

/// [`Scene::SpecularAa`]'s plate, lifted to [`SPECULAR_PLATE_UP`].
///
/// A pure translation, and that is a requirement rather than a simplification:
/// the plate's normals are authored to disagree with its geometry, so a scale
/// in the model matrix would send them through the cofactor transform and turn
/// the corrugation into an angle nobody chose. The mesh is authored at the size
/// it is drawn at.
fn specular_plate() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, SPECULAR_PLATE_UP, 0.0))
}

/// The camera [`Scene::SpecularAa`] is drawn with: straight down at the plate
/// through the long lens [`SPECULAR_CAMERA_UP`] argues for.
///
/// The field of view is derived from [`SPECULAR_HALF_HEIGHT`] and the distance
/// to the plate rather than written down, so the frame covers that much world
/// whatever the height becomes and `tests/render_e2e.rs` turns a plate
/// coordinate into a pixel by one division.
fn specular_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, SPECULAR_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the
        // world's `+Z` at the top of the frame, on `alpha_camera`'s terms.
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: 2.0 * (SPECULAR_HALF_HEIGHT / (SPECULAR_CAMERA_UP - SPECULAR_PLATE_UP)).atan(),
            near: 0.01,
        },
    }
}

/// The sun [`Scene::SpecularAa`] runs under: the default sun's colour, tilted
/// by [`SPECULAR_SUN_TILT`] in the `x` plane alone.
fn specular_sun() -> crcbl_render::DirectionalLight {
    crcbl_render::DirectionalLight {
        direction: glam::Vec3::new(SPECULAR_SUN_TILT.sin(), SPECULAR_SUN_TILT.cos(), 0.0),
        color: glam::Vec3::splat(SPECULAR_SUN_LEVEL),
        ..crcbl_render::DirectionalLight::default()
    }
}

/// The colour [`Scene::Sprite`] and [`Scene::Ui`] composite onto, in **linear**
/// light — which is what a clear value on an sRGB attachment means.
///
/// Neither pass clears: both load what is already in the target, because
/// compositing onto it is what they are for. So the scene clears first, and
/// deliberately not to black — alpha blending onto black is the one background
/// where `src * a + dst * (1 - a)` and `src * a` agree, so a premultiplication
/// mistake would be invisible in exactly the frame meant to reveal it. Same
/// value, and the same reasoning, as `crcbl-vk`'s sprite suite.
const SCENE_CLEAR: [f32; 4] = [0.10, 0.20, 0.35, 1.0];

/// Half the world height [`Scene::Sprite`]'s orthographic camera shows.
///
/// The frame size is the caller's (`--size`), so the scene is laid out in world
/// units and the projection scales it: every rectangle below sits inside
/// `|x| <= 95`, which is on screen for any frame at least as wide as it is tall.
const SPRITE_HALF_HEIGHT: f32 = 100.0;

/// [`Scene::Sprite`]'s first sheet: 4×2 texels, two 2×2 frames side by side,
/// and no two texels alike.
///
/// ```text
///   frame A          frame B
///   red    green  |  cyan   magenta
///   blue   yellow |  white  black
/// ```
///
/// Asymmetric for the reason `crcbl-vk`'s sprite suite records: a flipped V
/// swaps red with blue, a flipped U swaps red with green, and a symmetric test
/// image passes through both while looking entirely plausible.
const SPRITE_SHEET_A: [u8; 32] = [
    255, 0, 0, 255, // A top-left: red
    0, 255, 0, 255, // A top-right: green
    0, 255, 255, 255, // B top-left: cyan
    255, 0, 255, 255, // B top-right: magenta
    0, 0, 255, 255, // A bottom-left: blue
    255, 255, 0, 255, // A bottom-right: yellow
    255, 255, 255, 255, // B bottom-left: white
    0, 0, 0, 255, // B bottom-right: black
];

/// [`Scene::Sprite`]'s second sheet: 2×2 texels in four colours that appear
/// nowhere in [`SPRITE_SHEET_A`], so "which sheet was bound" is readable
/// straight off the picture.
const SPRITE_SHEET_B: [u8; 16] = [
    255, 128, 0, 255, // orange
    128, 0, 255, 255, // violet
    0, 128, 128, 255, // teal
    128, 128, 128, 255, // grey
];

/// The tint on the fourth sprite.
///
/// A different factor per channel, because a tint that left any channel alone
/// would let the tinted rectangle share colours with the untinted one it is
/// there to be told apart from.
const SPRITE_TINT: [f32; 4] = [0.5, 0.7, 0.9, 1.0];

/// Frame A of [`SPRITE_SHEET_A`], as normalised UVs.
const SPRITE_FRAME_A: [f32; 4] = [0.0, 0.0, 0.5, 1.0];
/// Frame B of [`SPRITE_SHEET_A`].
const SPRITE_FRAME_B: [f32; 4] = [0.5, 0.0, 1.0, 1.0];

/// [`Scene::Sprite`]'s four rectangles, in world units: `[x, y, w, h]`, minimum
/// corner first, Y up. Ten units apart, so no two of them touch.
const SPRITE_RECTS: [[f32; 4]; 4] = [
    [-95.0, -20.0, 40.0, 40.0],
    [-45.0, -20.0, 40.0, 40.0],
    [5.0, -20.0, 40.0, 40.0],
    [55.0, -20.0, 40.0, 40.0],
];

/// Which sheet each of [`SPRITE_RECTS`] samples: **A A B A**, not A A B B.
///
/// The submission order of a `&[Sprite]` is the batching, so this is three
/// batches over two sheets and the third batch starts at instance 3. That
/// arrangement is the whole point of the scene: one batch is exactly the case
/// that hid the `SV_InstanceID` divergence, because with a single draw the
/// SPIR-V and WGSL lowerings of the instance index agree. A backend that reads
/// the wrong one draws the last rectangle in the *first* rectangle's place,
/// leaving its own slot at the clear colour.
const SPRITE_ORDER: [usize; 4] = [0, 0, 1, 0];

/// How far past the patch's near edge [`Scene::Dunes`]'s camera stands, and how
/// far up.
///
/// The same arrangement `crcbl-vk`'s `dunes_camera` uses and
/// `crcbl_shaders::cluster_dag`'s receding-patch test reads its histograms from:
/// the patch is centred on the origin in `x` and `z` with its height on `y`, so
/// an eye at negative `z` and a small `y` is a viewer standing at one end of a
/// ground plane whose far edge is tens of times further away than its near one.
const DUNES_CAMERA_BACK: f32 = 2.0;
const DUNES_CAMERA_UP: f32 = 4.0;

/// The camera [`Scene::Dunes`] is drawn with.
///
/// **Perspective**, and that is not decoration: the metric divides a projected
/// error by the distance to a group's sphere, an orthographic projection has no
/// such falloff, and `ForwardRenderer` answers that honestly by selecting the
/// base level whole. A scene meant to show a level being *chosen* has to have a
/// distance term to choose by.
fn dunes_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(
            0.0,
            DUNES_CAMERA_UP,
            -crcbl_shaders::dunes::DUNES_EXTENT - DUNES_CAMERA_BACK,
        ),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Y,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// The camera [`Scene::Sprite`] is drawn with: orthographic, looking down −Z at
/// the plane the sprites live on.
fn sprite_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, 0.0, 1.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Y,
        projection: Projection::Orthographic {
            half_height: SPRITE_HALF_HEIGHT,
            near: 0.1,
            far: 10.0,
        },
    }
}

/// [`Scene::Sprite`]'s four sprites, over the two registered sheets.
fn sprite_scene(sheets: [SheetId; 2]) -> [Sprite; 4] {
    let uv = |index: usize| match index {
        0 => SPRITE_FRAME_A,
        1 => SPRITE_FRAME_B,
        // The second sheet has one frame, which is the whole image.
        2 => [0.0, 0.0, 1.0, 1.0],
        _ => SPRITE_FRAME_A,
    };
    std::array::from_fn(|index| {
        let sprite = Sprite::new(sheets[SPRITE_ORDER[index]], SPRITE_RECTS[index], uv(index));
        // Only the last one is tinted, so the two rectangles that share frame A
        // are the same picture in different colours.
        if index == 3 {
            sprite.with_tint(SPRITE_TINT)
        } else {
            sprite
        }
    })
}

/// [`Scene::Ui`]'s draw list, in the pass's Y-down screen pixels.
///
/// Laid out as fractions of `extent` so the same picture arrives at every
/// `--size`, and built from all three command kinds: a filled rectangle, a
/// translucent one straddling its edge, an outline, and two lines of text
/// through the glyph atlas. Text alone would leave a broken atlas binding
/// looking like an empty frame; a rectangle alone would never sample the atlas
/// at all.
fn ui_draw_list(extent: (u32, u32)) -> DrawList {
    use glam::Vec2;

    let (width, height) = (extent.0 as f32, extent.1 as f32);
    let at = |x: f32, y: f32| Vec2::new(width * x, height * y);

    let mut list = DrawList::new();
    // The panel, then the translucent bar half on it and half off it: the two
    // colours it blends to are two more distinct colours in the frame, and they
    // are the only evidence the pass blends rather than replaces.
    list.rect(at(0.08, 0.10), at(0.92, 0.62), [0.15, 0.20, 0.55, 1.0]);
    list.rect(at(0.30, 0.45), at(0.70, 0.85), [1.0, 0.45, 0.10, 0.5]);
    list.rect_outline(
        at(0.03, 0.04),
        at(0.97, 0.96),
        (height * 0.02).max(1.0),
        [1.0, 0.85, 0.0, 1.0],
    );
    list.text(at(0.12, 0.16), "CRCBL", [1.0, 1.0, 1.0, 1.0], height * 0.18);
    list.text(
        at(0.12, 0.66),
        "ui scene",
        [0.2, 1.0, 0.4, 1.0],
        height * 0.10,
    );
    list
}

/// A forward scene an **application** built for itself: what to draw, where it
/// is seen from, and the sun it stands under.
///
/// The way in for a caller whose room is not one of the [`Scene`] variants —
/// `apps/lantern` is the first, and the reason this exists. Everything below the
/// renderer is the same offscreen path either way: the surface, the adapter
/// pin, the ring, the barriers around the readback and the row unpadding are
/// [`OffscreenSetup`]'s, and a sample rebuilding them for itself is
/// `docs/plan/sample/00-samples-overview.md` rule 1's "reaching around the
/// facade".
///
/// The renderer is the application's because
/// [`ForwardRenderer::with_scene`](crate::render::ForwardRenderer::with_scene)
/// is: the description, the material rows, the page and the instances are all
/// the caller's, and there is no shape this module could take that would not be
/// a second scene vocabulary beside `crcbl_render::scene`.
///
/// # The punctual lights are not here
///
/// They are already the renderer's, through
/// [`ForwardRenderer::set_lights`](crate::render::ForwardRenderer::set_lights),
/// and a field here would be a second place for them to be set — the sun is
/// separate only because
/// [`begin_frame`](crate::render::ForwardRenderer::begin_frame) takes it as an
/// argument.
#[allow(missing_debug_implementations)]
pub struct ForwardScene {
    /// Where the frame is seen from.
    pub camera: Camera,
    /// The sun it is lit by — the light that owns the ambient term and the
    /// shadow cascades.
    pub sun: DirectionalLight,
    /// The renderer, built and filled by the caller.
    ///
    /// Boxed because it is much the largest thing here: it carries the geometry
    /// pools and the instance ring, and moving one by value is a memcpy of all
    /// of it.
    pub renderer: Box<ForwardRenderer>,
}

impl From<ForwardScene> for SceneState {
    fn from(scene: ForwardScene) -> Self {
        Self::Forward {
            camera: scene.camera,
            light: scene.sun,
            renderer: scene.renderer,
        }
    }
}

/// The renderer, and the content, for the scene being drawn.
///
/// One variant per [`Scene`], plus the [`ForwardScene`] an application supplies;
/// the frame's per-scene work is the three arms of
/// `OffscreenSetup::draw_and_readback` and nothing else keys off it.
enum SceneState {
    /// Every scene drawn through [`ForwardRenderer`]: one camera, one sun and
    /// one set of residents, differing in what is put in the scene and where the
    /// camera stands.
    Forward {
        camera: Camera,
        light: DirectionalLight,
        /// Boxed because it is much the largest of the three: it carries the
        /// geometry pools and the instance ring, and an unboxed variant would
        /// make every `SceneState` — including the two small ones — that size.
        renderer: Box<ForwardRenderer>,
    },
    Sprite {
        renderer: SpriteRenderer,
        sheets: [SheetId; 2],
    },
    Ui {
        renderer: UiRenderer,
        atlas: FontAtlas,
    },
}

/// Puts one of the demo scene's meshes in the frame at `model`.
///
/// **Insertion order is the caller's and it is load-bearing**, so every scene
/// below places its objects in the order the frame has always held them — the
/// cube first, then whatever stands on it. The slot an object lands in is
/// `docs/plan/25-lod.md`'s hysteresis key, and it is what kept the goldens still
/// when the five demo setters that used to do this were retired.
///
/// The insert cannot fail here: the pool is
/// [`Capacities::instances`](crate::render::Capacities::instances) wide and no
/// scene here places more than three objects.
fn place(renderer: &mut ForwardRenderer, mesh: usize, material: usize, model: glam::Mat4) {
    renderer
        .add_instance(&InstanceDesc {
            mesh,
            material,
            transform: model,
        })
        .expect("an instance pool of thousands has room for a screenshot scene");
}

/// The demo scene's cube, placed **before anything else**.
///
/// Every forward scene here draws it but [`Scene::Probes`], whose room is the
/// open box alone — as the subject in [`Scene::Cube`], as the floor in
/// [`Scene::Spot`] and its two shadow siblings, and parked out of frame in
/// [`Scene::Ao`]. The renderer used to insert it at build and rewrite it from
/// every `begin_frame`; it is an ordinary instance now, so the caller places it,
/// and placing it first is what keeps it in the pool slot it has always had.
fn place_cube(renderer: &mut ForwardRenderer, model: glam::Mat4) {
    place(renderer, DEMO_CUBE, DEMO_UNTINTED, model);
}

/// The three pyramids [`Scene::Cube`] and [`Scene::Lights`] both stand on the
/// cube, in the order and the rows they have always carried.
///
/// One helper because the two scenes are the same geometry on purpose — their
/// goldens differ in the light list and in nothing else — so a transform or a
/// row that moved in one and not the other would make that pair prove nothing.
fn place_pyramids(renderer: &mut ForwardRenderer) {
    place(
        renderer,
        DEMO_PYRAMID,
        DEMO_UNTINTED,
        glam::Mat4::from_translation(PYRAMID_AT),
    );
    place(
        renderer,
        DEMO_PYRAMID,
        DEMO_TINTED,
        glam::Mat4::from_translation(TINTED_PYRAMID_AT),
    );
    place(
        renderer,
        DEMO_PYRAMID,
        DEMO_TEXTURED,
        glam::Mat4::from_translation(TEXTURED_PYRAMID_AT),
    );
}

impl SceneState {
    /// What this scene's renderer recorded for the frame just drawn.
    ///
    /// One renderer per scene, so this is that renderer's own
    /// [`counters`](ForwardRenderer::counters) rather than a sum — a game adds
    /// up several, and this module draws exactly one thing.
    fn counters(&self) -> FrameCounters {
        match self {
            Self::Forward { renderer, .. } => renderer.counters(),
            Self::Sprite { renderer, .. } => renderer.counters(),
            Self::Ui { renderer, .. } => renderer.counters(),
        }
    }

    /// Builds the renderer this scene needs, and uploads whatever it draws.
    fn open(
        scene: Scene,
        device: &dyn Device,
        queue: QueueHandle,
        format: Format,
    ) -> Result<Self, OffscreenError> {
        Ok(match scene {
            Scene::Cube => {
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                place_cube(&mut renderer, ForwardRenderer::spin(0.0));
                place_pyramids(&mut renderer);
                Self::Forward {
                    camera: Camera::default().with_projection(Projection::Perspective {
                        fov_y: std::f32::consts::FRAC_PI_3,
                        near: 0.01,
                    }),
                    light: DirectionalLight::default(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Lights => {
                // The cube scene's geometry exactly, so the two goldens differ
                // in their light lists and in nothing else.
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                place_cube(&mut renderer, ForwardRenderer::spin(0.0));
                place_pyramids(&mut renderer);
                renderer.set_lights(&scene_lights());
                Self::Forward {
                    camera: Camera::default().with_projection(Projection::Perspective {
                        fov_y: std::f32::consts::FRAC_PI_3,
                        near: 0.01,
                    }),
                    light: dim_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Spot => {
                // **The cube alone, and it is the floor.** Every other resident
                // stays off: a pyramid beside the pool would be a second lit
                // shape in a frame whose whole content is meant to be one cone
                // on one flat surface.
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                place_cube(&mut renderer, spot_floor());
                renderer.set_lights(&[spot_light()]);
                Self::Forward {
                    camera: spot_camera(),
                    light: spot_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::SpotShadow => {
                // The spot scene's floor exactly, with one thing added: the
                // pyramid, standing in the light. Everything else that differs
                // — the camera, the light's tilt — is what makes the shadow
                // separable from its caster, and `Scene::SpotShadow` is where
                // that is argued.
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                place_cube(&mut renderer, spot_floor());
                place(
                    &mut renderer,
                    DEMO_PYRAMID,
                    DEMO_UNTINTED,
                    spot_shadow_caster(),
                );
                renderer.set_lights(&[spot_shadow_light()]);
                Self::Forward {
                    camera: spot_shadow_camera(),
                    light: spot_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::PointShadow => {
                // The spot-shadow scene's floor, with **two** casters on
                // different sides of the light: one out along `+X` and one out
                // along `-Z`, so their shadows fall across two different faces
                // of the light's map. One caster would prove a point light casts
                // *a* shadow, which is what a single working face already does.
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                place_cube(&mut renderer, spot_floor());
                place(
                    &mut renderer,
                    DEMO_PYRAMID,
                    DEMO_UNTINTED,
                    point_shadow_caster(glam::Vec3::new(POINT_CASTER_AT, 0.0, 0.0)),
                );
                place(
                    &mut renderer,
                    DEMO_PYRAMID,
                    DEMO_TINTED,
                    point_shadow_caster(glam::Vec3::new(0.0, 0.0, -POINT_CASTER_AT)),
                );
                renderer.set_lights(&[point_shadow_light()]);
                Self::Forward {
                    camera: point_shadow_camera(),
                    light: spot_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::AreaLight => {
                // **The floor alone, and it is the cube.** Nothing stands on it,
                // for `Scene::Spot`'s reason and one sharper: an area light's
                // whole difference from a point light is the *shape* of its
                // highlight, and a second object in the frame is something a
                // reader — or an assertion — can take that shape off instead.
                // The cube is placed rather than parked, so it is still the
                // first insertion and still holds the pool slot every other
                // forward scene gives it, and it is placed through the dark
                // glossy row `area_scene` appends for it.
                let mut renderer =
                    ForwardRenderer::with_scene(device, queue, format, &area_scene())?;
                place(&mut renderer, DEMO_CUBE, AREA_FLOOR, spot_floor());
                // **The reflection pair, refused**, on `Scene::Probes`' terms
                // and with a sharper exposure: this floor carries
                // `PYRAMID_ROUGHNESS`, which is under `ssr.slang`'s cutoff, so a
                // march over the depth buffer would write into exactly the
                // pixels the highlight is measured in. It would also put this
                // scene in `tests/render_e2e.rs`'s `path_lsb_channels` budget
                // for a term the scene is not about — with the pair refused, the
                // two geometry paths draw this frame byte for byte.
                renderer.set_effect_request(EffectRequest {
                    programmatic: EffectOverride::none()
                        .force(RenderEffects::REFLECTIONS, Some(false)),
                    ..EffectRequest::default()
                });
                renderer.set_lights(&area_strips());
                Self::Forward {
                    camera: area_camera(),
                    light: area_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::FillLight => {
                // `Scene::AreaLight`'s floor, placed the same way and through
                // the same row, and one more reason of this scene's own for the
                // floor being the only thing in it: a fill light is refused a
                // shadow tile and its lit twin is handed one, so a caster here
                // would put a shadow on one half of the mirror and nothing on
                // the other, and the difference the bands measure would be a
                // shadow wearing the fill flag's name.
                let mut renderer =
                    ForwardRenderer::with_scene(device, queue, format, &area_scene())?;
                place(&mut renderer, DEMO_CUBE, AREA_FLOOR, spot_floor());
                // **The reflection pair, refused**, on `Scene::AreaLight`'s
                // terms exactly: this is that floor and it carries that
                // roughness, which is under `ssr.slang`'s cutoff, so a march
                // over the depth buffer would write into the pixels the
                // highlights are measured in.
                renderer.set_effect_request(EffectRequest {
                    programmatic: EffectOverride::none()
                        .force(RenderEffects::REFLECTIONS, Some(false)),
                    ..EffectRequest::default()
                });
                renderer.set_lights(&fill_light_pairs());
                Self::Forward {
                    camera: area_camera(),
                    light: area_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::AlphaMask => {
                // **The floor first, and the plate over it.** The cube is
                // placed rather than parked, so it is still the first insertion
                // and still holds the pool slot every other forward scene gives
                // it — see `place`, where insertion order is argued — and it is
                // placed through the grey row `alpha_mask_scene` appends for it
                // rather than through `DEMO_UNTINTED`, which is why there is no
                // `place_cube` call.
                //
                // The plate is the second and last: nothing else is in the
                // frame, for `Scene::Spot`'s reason and one sharper. Two of this
                // scene's three claims are read off bands of *floor* — one seen
                // through the hole, one in the shadow's hole — and any other
                // object standing on that floor is something a band could be
                // measuring instead.
                let mut renderer =
                    ForwardRenderer::with_scene(device, queue, format, &alpha_mask_scene())?;
                place(&mut renderer, DEMO_CUBE, ALPHA_FLOOR, alpha_floor());
                place(&mut renderer, DEMO_CUBE, ALPHA_PLATE, alpha_plate());
                Self::Forward {
                    camera: alpha_camera(),
                    light: alpha_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::DoubleSided => {
                // **The floor first, and the three quads over it**, on
                // `Scene::AlphaMask`'s terms exactly: the cube is placed rather
                // than parked so it holds the pool slot every forward scene
                // gives it, and through the grey row `double_sided_scene`
                // appends for it rather than through `DEMO_UNTINTED`.
                //
                // Nothing else is in the frame. Four of this scene's bands are
                // read off *floor* — one where a culled quad is not, one of open
                // lit floor, and two inside shadows — and any other object
                // standing on that floor is something a band could be measuring
                // instead.
                let mut renderer =
                    ForwardRenderer::with_scene(device, queue, format, &double_sided_scene())?;
                place(&mut renderer, DEMO_CUBE, DOUBLE_FLOOR, double_floor());
                for (material, model) in double_sided_quads() {
                    place(&mut renderer, DOUBLE_QUAD_MESH, material, model);
                }
                Self::Forward {
                    camera: double_camera(),
                    light: double_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::SpecularAa => {
                let mut renderer =
                    ForwardRenderer::with_scene(device, queue, format, &specular_aa_scene())?;
                // **Nothing marches over this frame.** The plate is smoother
                // than `ssr.slang`'s cutoff, so a screen-space reflection pass
                // would compose its own answer into both bands — and what the
                // bands are evidence about is one lobe evaluated in one stage.
                renderer.set_effect_request(EffectRequest {
                    programmatic: EffectOverride::none()
                        .force(RenderEffects::REFLECTIONS, Some(false)),
                    ..EffectRequest::default()
                });
                // The plate alone: nothing else may stand in a band, and there
                // is no floor for its shadow to fall on either — a caster over
                // an empty frame is the whole scene, which is what leaves the
                // margin either side of the plate readable as background.
                place(
                    &mut renderer,
                    SPECULAR_PLATE_MESH,
                    SPECULAR_MATERIAL,
                    specular_plate(),
                );
                Self::Forward {
                    camera: specular_camera(),
                    light: specular_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Dunes => {
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                place_cube(&mut renderer, ForwardRenderer::spin(0.0));
                // **Refused rather than drawn empty.** `selects_levels` says no
                // on a device that reports a mesh stage and no amplification
                // stage, where the un-amplified path would emit every cluster of
                // every level at once; every other device draws the patch, per
                // cluster or through a uniform cut. Asking before placing the
                // patch is the caller's job — `add_instance` has no vocabulary
                // for that refusal — and not asking is what made this scene a
                // frame of clear colour that a golden would have been blessed
                // from.
                if !renderer.selects_levels() {
                    renderer.destroy(device);
                    return Err(OffscreenError::Unusable(
                        "this device reports a mesh stage and no amplification stage, so the \
                         dunes patch's cluster DAG has nothing that can select a level in it",
                    ));
                }
                place(
                    &mut renderer,
                    DEMO_DUNES,
                    DEMO_UNTINTED,
                    glam::Mat4::IDENTITY,
                );
                Self::Forward {
                    camera: dunes_camera(),
                    light: DirectionalLight::default(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Sprite => {
                let mut renderer = SpriteRenderer::new(device, queue, format)?;
                let mut register = |label, width, height, pixels| {
                    renderer.register_sheet(
                        device,
                        &SheetDesc {
                            label,
                            width,
                            height,
                            // Pixel art's sampler, and the branch of
                            // `sprite.slang` a game actually ships on.
                            sample: SampleMode::Pixel,
                            pixels,
                        },
                    )
                };
                let sheets = match (
                    register("screenshot sheet A", 4, 2, &SPRITE_SHEET_A),
                    register("screenshot sheet B", 2, 2, &SPRITE_SHEET_B),
                ) {
                    (Ok(a), Ok(b)) => [a, b],
                    // Whichever failed, the renderer owns everything that did
                    // upload and gives it back here rather than at drop.
                    (Err(error), _) | (Ok(_), Err(error)) => {
                        renderer.destroy(device);
                        return Err(OffscreenError::Hal(error));
                    }
                };
                Self::Sprite { renderer, sheets }
            }
            Scene::Ao => {
                // The open box alone, and the cube parked out of frame — see
                // `ao_parked_cube`. Every other resident stays off for
                // `Scene::Spot`'s reason: what this frame is about is one
                // concave corner and the flat floor beside it.
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                place_cube(&mut renderer, ao_parked_cube());
                place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, ao_box());
                Self::Forward {
                    camera: ao_camera(),
                    light: ao_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Ssr => {
                // The whole of it is in `ssr_forward`, on `Scene::Aa`'s terms
                // below: `tests/render_e2e.rs` builds the same scene through
                // that function under three different skies, because what the
                // sky adds to a reflection is only recognisable against the
                // same frame without it.
                ssr_forward(
                    device,
                    queue,
                    format,
                    crcbl_render::Sky::NONE,
                    RenderEffects::DEFAULT_STACK,
                )?
                .into()
            }
            Scene::AtmosphereMirror => {
                // The floor, and nothing else in the frame — see the variant.
                // It is a plate of its own rather than the demo cube, for the
                // reason `atmosphere_mirror_mesh` gives: that cube's faces
                // carry vertex colours and a green mirror is not what this
                // fixture is predicting.
                let mut renderer =
                    ForwardRenderer::with_scene(device, queue, format, &atmosphere_mirror_scene())?;
                // **The reflection pair and nothing else.** Shadows have no
                // caster and no lit surface to fall on; the occlusion pass
                // scales an ambient term a conductor does not have; the
                // antialiasing resolve would filter the very gradient the
                // bands measure. Each of those is a term the host would have
                // to model to predict a floor pixel, and none of them is what
                // this frame is about.
                renderer.set_effect_request(EffectRequest {
                    camera: RenderEffects::REFLECTIONS,
                    ..EffectRequest::default()
                });
                renderer.set_atmosphere(Some(atmosphere_mirror_sky()));
                // Authored at its final size, so the instance carries the
                // identity: a plate whose corners are already world-space is
                // one fewer transform between the fixture and the host's own
                // prediction of a reflected ray.
                place(
                    &mut renderer,
                    ATMOSPHERE_MIRROR_MESH,
                    ATMOSPHERE_MIRROR_MATERIAL,
                    glam::Mat4::IDENTITY,
                );
                Self::Forward {
                    camera: atmosphere_mirror_camera(),
                    light: atmosphere_mirror_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::GradientMirror => {
                // The arm above with `set_sky` in place of `set_atmosphere`,
                // and everything else shared: the same plate, the same camera,
                // the same effect request. What differs between the two
                // goldens is therefore which sky the reflection pass read, and
                // that is the whole of what this fixture is for.
                //
                // `atmosphere_mirror_sun` is reused because it contributes
                // nothing to either frame — its colour and its ambient are
                // both zero — so the direction it names is the only thing it
                // carries, and no pixel here can observe it.
                let mut renderer =
                    ForwardRenderer::with_scene(device, queue, format, &atmosphere_mirror_scene())?;
                renderer.set_effect_request(EffectRequest {
                    camera: RenderEffects::REFLECTIONS,
                    ..EffectRequest::default()
                });
                renderer.set_sky(gradient_mirror_sky());
                place(
                    &mut renderer,
                    ATMOSPHERE_MIRROR_MESH,
                    ATMOSPHERE_MIRROR_MATERIAL,
                    glam::Mat4::IDENTITY,
                );
                Self::Forward {
                    camera: atmosphere_mirror_camera(),
                    light: atmosphere_mirror_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Bloom => {
                // The floor every other overhead fixture stands on, and the
                // emitter laid on it — see `bloom_emitter`. Nothing else is in
                // frame, for `Scene::Spot`'s reason: what this frame is about is
                // one bright patch and the flat floor its halo spreads over.
                //
                // **The cube is placed as the floor rather than parked**, which
                // is why there is no `place_cube` call: it is still the first
                // insertion and still holds the pool slot every other scene
                // gives it.
                let mut renderer =
                    ForwardRenderer::with_scene(device, queue, format, &bloom_scene())?;
                // **The one fixture that asks for the lens.**
                // `RenderEffects::DEFAULT_STACK` leaves bloom out — a view that
                // has declared no render stack has declared no lens — so this is
                // the camera-stack layer being exercised as topic 18 describes
                // it, and it is what keeps every other golden in the tree
                // untouched by this slice.
                //
                // **The default stack plus the lens, not `all()`.** They were
                // the same set until the antialiasing resolve joined the effects
                // outside the default — see `RenderEffects::DEFAULT_STACK` — and
                // `all()` here would have quietly added that resolve to this
                // fixture, which is a halo measured through an edge filter. Each
                // effect held out of the default gets the fixture it is about;
                // this one is about the chain.
                renderer.set_effect_request(EffectRequest {
                    camera: RenderEffects::DEFAULT_STACK.union(RenderEffects::BLOOM),
                    ..EffectRequest::default()
                });
                place(&mut renderer, DEMO_CUBE, DEMO_UNTINTED, spot_floor());
                place(&mut renderer, DEMO_CUBE, BLOOM_EMITTER, bloom_emitter());
                Self::Forward {
                    camera: bloom_camera(),
                    light: bloom_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Aa => {
                // The whole of it is in `aa_forward`, because
                // `tests/render_e2e.rs` builds the same scene through that
                // function with a different effect set — see its doc for why the
                // comparison cannot be made against a golden.
                aa_forward(device, queue, format, RenderEffects::DEFAULT_STACK)?.into()
            }
            Scene::Probes => {
                // **The only scene here built from a description of its own**,
                // and the only thing that differs from `scene::demo`'s is the
                // probe grid — see `probe_scene`. The room is the open box and
                // nothing else: no cube, parked or otherwise, because a second
                // object standing on this floor is a second thing occluding the
                // bands that are the measurement.
                let mut renderer =
                    ForwardRenderer::with_scene(device, queue, format, &probe_scene())?;
                // The fixture's measured pixels are diffuse probe irradiance.
                // Reflections now evaluate rough surfaces too, so refuse their
                // pair here rather than letting specular contaminate the Rust
                // mirror comparison below.
                renderer.set_effect_request(EffectRequest {
                    programmatic: EffectOverride::none()
                        .force(RenderEffects::REFLECTIONS, Some(false)),
                    ..EffectRequest::default()
                });
                place(&mut renderer, DEMO_OPEN_BOX, DEMO_UNTINTED, probe_room());
                // The room is placed, so the probes can record it — see
                // `ForwardRenderer::capture_probe_visibility`, which is a call
                // rather than part of `with_scene` because a description has no
                // instances in it. Both probes stand in open air above this
                // floor and neither is behind anything, so what the capture buys
                // *here* is that the fixture exercises the read at all; the
                // leak it exists to stop is checked where geometry stands
                // between a probe and a surface.
                renderer.capture_probe_visibility(device, queue)?;
                Self::Forward {
                    camera: probe_camera(),
                    light: probe_sun(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Ui => Self::Ui {
                renderer: UiRenderer::new(device, queue, format)?,
                atlas: FontAtlas::built_in(),
            },
        })
    }

    /// Releases the scene's GPU resources. The device must be idle.
    fn destroy(self, device: &dyn Device) {
        match self {
            Self::Forward { renderer, .. } => renderer.destroy(device),
            Self::Sprite { renderer, .. } => renderer.destroy(device),
            Self::Ui { renderer, .. } => renderer.destroy(device),
        }
    }
}

// ---------------------------------------------------------------------------
// OffscreenSetup
// ---------------------------------------------------------------------------

/// The largest edge, in pixels, an offscreen frame may have.
///
/// 16384 is `maxImageDimension2D` on every implementation the engine targets,
/// so anything past it would be refused by swapchain creation anyway — but
/// only *after* this module had already tried to allocate a host-visible
/// staging buffer for it. `16384x16384` RGBA8 is one gibibyte, which is the
/// most this path will ever ask an allocator for.
///
/// The CLI checks `--size` against this at parse time so an absurd request is
/// a bad *invocation* (exit 2) rather than a failed command;
/// `OffscreenSetup::request` (and `open` over it) checks it again because a
/// library may not have gone through the CLI.
pub const MAX_DIMENSION: u32 = 16_384;

/// Row-pitch alignment, in bytes, for the readback staging buffer.
///
/// **Not a performance hint — a portability requirement, and it outlived the
/// backend that exposed it.** WebGPU specifies `bytesPerRow` in *bytes* and
/// requires a multiple of 256 for any multi-row buffer↔image copy, so a tightly
/// packed readback — which is legal on Vulkan and is what this module used to
/// record — is invalid for every width that is not a multiple of 64.
/// `crcbl-webgpu` enforces it at the seam (see `command.rs`'s copy encoders and
/// `writer.rs`), so removing this padding breaks the browser backend.
///
/// It was found on `crcbl-wgpu`, which is deleted; the transcript is kept
/// because it is the clearest statement of the failure and no surviving native
/// backend can reproduce it — `webgpu` refuses to open on native, so there is no
/// command here that shows it any more:
///
/// ```text
/// $ CRCBL_GPU=wgpu crcbl screenshot --size 32x32 --output /tmp/x.png
/// crcbl: render/readback failed: HAL: invalid descriptor: a buffer↔image copy
///   of 32 texel(s) per row is 128 bytes, which wgpu requires to be a multiple
///   of 256
/// ```
///
/// Found by the cross-backend harness at P5.12, which compares this path's
/// output between the two backends at more than one frame size — `256x192`, the
/// only size anything had ever asked for, happens to be a multiple of 64 and hid
/// it. Vulkan imposes no such rule and pads harmlessly, so the padded pitch is
/// unconditional rather than a backend-specific branch: nothing above
/// `crcbl-hal` may key off which backend is behind the seam.
///
/// The padding never reaches the caller — [`PendingReadback::poll`] compacts the
/// rows before returning.
pub const READBACK_ROW_ALIGNMENT: u32 = 256;

/// How long `OffscreenSetup::draw_and_readback` waits for the copy to land.
///
/// Generous because an offscreen ring on a software rasteriser can take
/// hundreds of milliseconds for a single frame. Public because the two error
/// paths that mention it are public, and a deadline a caller can be hit by is
/// one it should be able to read.
pub const READBACK_DEADLINE: Duration = Duration::from_secs(10);

/// How many images the offscreen ring holds.
///
/// More than one so the path a windowed swapchain takes — acquire a *different*
/// image, present, come round again — is the path a screenshot takes too. It is
/// named rather than written into the descriptor because the barrier test below
/// has to draw more frames than this to reach a re-used image at all, and a lap
/// count derived from a literal two files away is one that silently stops
/// meaning what it says.
const RING_IMAGES: u32 = 2;

/// The closure `finish_open` calls to build the scene's renderer once the device
/// is ready.
///
/// Boxed rather than a generic parameter so [`PendingOffscreen`] is one concrete
/// type a browser harness can hold across frames — and so the private
/// [`SceneState`] it returns never surfaces in a public generic signature. The
/// `'b` is the closure's own capture lifetime: [`OffscreenSetup::request`]'s scene
/// closure is `'static`, while `OffscreenSetup::open_forward_with`'s — native
/// only, so not linkable from a `wasm32` build of these docs — may borrow
/// a caller's locals, and a native open drives it to completion before returning
/// so the borrow never outlives them.
type BuildScene<'b> =
    Box<dyn FnOnce(&dyn Device, QueueHandle, Format) -> Result<SceneState, OffscreenError> + 'b>;

/// One frame read back: its `(width, height)` and its tightly packed bytes, in
/// [`OffscreenSetup::format`]'s channel order.
type ReadbackFrame = ((u32, u32), Vec<u8>);

/// Where a [`PendingOffscreen`] has got to: opening the instance, then opening
/// the device on it.
///
/// The two async steps of an open — [`crate::backend::request_open`] and
/// [`Instance::request_device`] — with the synchronous surface/adapter/format
/// work done at the transition between them.
#[allow(clippy::large_enum_variant)]
enum OpenPhase {
    /// Polling [`crate::backend::PendingInstance`] for the backend.
    Instance {
        pending: crate::backend::PendingInstance,
    },
    /// The instance is open and the surface, adapter and format are decided;
    /// polling [`PendingDevice`] for the device.
    Device {
        instance: Box<dyn Instance>,
        surface: SurfaceHandle,
        adapter: crate::hal::AdapterInfo,
        format: Format,
        pending: Box<dyn PendingDevice>,
    },
    /// The setup has been handed over (or an open failed). A further poll is a
    /// caller bug.
    Finished,
}

/// An [`OffscreenSetup`] open in flight — the non-blocking half of
/// [`OffscreenSetup::request`].
///
/// Poll it once per frame with [`Self::poll`] until it hands over the setup. It
/// blocks nowhere and builds on `wasm32`, which is the whole reason it exists:
/// the browser's one thread cannot sit in `OffscreenSetup::open`'s busy loop,
/// so browser code drives this from `requestAnimationFrame` instead.
#[allow(missing_debug_implementations)]
pub struct PendingOffscreen<'b> {
    width: u32,
    height: u32,
    optional_features: Features,
    /// The scene builder, taken once the device is ready. `None` only after the
    /// setup has been handed over.
    build: Option<BuildScene<'b>>,
    phase: OpenPhase,
}

impl PendingOffscreen<'_> {
    /// Advances the open. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// Drives the instance open, then the device open, then builds the ring and
    /// the scene — the synchronous middle steps run in the same poll that
    /// finishes the instance, so a caller sees only the two genuine waits.
    ///
    /// # Errors
    ///
    /// Anything `OffscreenSetup::open` can report, surfaced from whichever step
    /// reached it. Polling after the setup was handed over reports
    /// [`OffscreenError::Unusable`].
    pub fn poll(&mut self) -> Result<Option<OffscreenSetup>, OffscreenError> {
        loop {
            match core::mem::replace(&mut self.phase, OpenPhase::Finished) {
                OpenPhase::Instance { mut pending } => match pending.poll()? {
                    None => {
                        self.phase = OpenPhase::Instance { pending };
                        return Ok(None);
                    }
                    // Fall through to the device phase and poll it in this same
                    // call: the surface and adapter work between the two is
                    // synchronous, so parking here would waste a frame.
                    Some(instance) => {
                        self.phase =
                            OffscreenSetup::start_device(instance, self.optional_features)?;
                    }
                },
                OpenPhase::Device {
                    instance,
                    surface,
                    adapter,
                    format,
                    mut pending,
                } => match pending.poll()? {
                    DeviceRequestState::Pending => {
                        self.phase = OpenPhase::Device {
                            instance,
                            surface,
                            adapter,
                            format,
                            pending,
                        };
                        return Ok(None);
                    }
                    DeviceRequestState::Ready(device) => {
                        let build = self
                            .build
                            .take()
                            .expect("the scene builder is present until the setup is handed over");
                        let setup = OffscreenSetup::finish_open(
                            build,
                            instance,
                            surface,
                            adapter,
                            format,
                            (self.width, self.height),
                            device,
                        )?;
                        return Ok(Some(setup));
                    }
                },
                OpenPhase::Finished => {
                    return Err(OffscreenError::Unusable(
                        "the offscreen setup was already opened",
                    ));
                }
            }
        }
    }
}

/// Holds everything needed to render one frame offscreen: a GPU instance,
/// device, offscreen swapchain ring, and the chosen scene's renderer.
///
/// The caller drives one frame via [`Self::begin_readback`] (or the blocking
/// `draw_and_readback` over it on native), then tears down with [`Self::finish`].
#[allow(missing_debug_implementations)]
pub struct OffscreenSetup {
    instance: Box<dyn Instance>,
    device: Box<dyn Device>,
    surface: SurfaceHandle,
    swapchain: SwapchainHandle,
    queue: QueueHandle,
    format: Format,
    /// The adapter the device was created on, kept so [`Self::adapter`] can
    /// answer after the enumeration it came from has been dropped.
    adapter: crate::hal::AdapterInfo,
    /// The renderer for the [`Scene`] this setup was opened with, and whatever
    /// that scene draws.
    scene: SceneState,
    pool: TransientPool,
}

/// Reasons an offscreen render might fail before a pixel is written.
#[derive(Debug, thiserror::Error)]
pub enum OffscreenError {
    /// No GPU backend could be opened.
    #[error("GPU backend: {0}")]
    Backend(#[from] crate::backend::GpuError),

    /// No adapter, no queue, no format, or a surface-cap query failed.
    #[error("device not usable: {0}")]
    Unusable(&'static str),

    /// [`ADAPTER_ENV_VAR`](crate::adapter::ADAPTER_ENV_VAR) named a device class
    /// this backend's enumeration does not have exactly one of, or a word that
    /// is not a class at all.
    ///
    /// Never a fallback: a frame drawn on an adapter nobody asked for is a green
    /// run that is evidence about the wrong device.
    #[error("{0}")]
    AdapterPin(#[from] crate::adapter::PinMiss),

    /// A HAL call failed.
    #[error("HAL: {0}")]
    Hal(#[from] crate::hal::HalError),

    /// A surface operation failed.
    #[error("surface: {0}")]
    Surface(#[from] crate::hal::SurfaceError),

    /// A graph compile or execute failed.
    #[error("graph: {0}")]
    Graph(#[from] GraphError),

    /// The swapchain went out of date before the first frame completed.
    #[error("offscreen swapchain is out of date")]
    OutOfDate,

    /// The requested frame is larger than [`MAX_DIMENSION`] on an edge, or its
    /// byte count does not fit this machine's address space.
    #[error("{width}x{height} is larger than the {MAX_DIMENSION}x{MAX_DIMENSION} offscreen limit")]
    TooLarge {
        /// Requested width, in pixels.
        width: u32,
        /// Requested height, in pixels.
        height: u32,
    },

    /// A failure the device reported out of band, drained by [`OffscreenSetup::finish`].
    ///
    /// The Vulkan validation layer arrives here: a specification violation the
    /// driver accepted returns success from the call that committed it —
    /// `vkCmdDraw` has no return value — and the message lands on the debug
    /// messenger afterwards, so a caller watching return values sees a healthy
    /// device and a frame that is not legal. [`crate::engine`]'s frame loop
    /// already drains [`Device::take_error`] every frame; a screenshot never
    /// runs that loop, which left the offscreen path the one place a frame could
    /// be illegal and still be saved, compared against a golden, and reported as
    /// a pass.
    #[error("the device reported out of band: {0}")]
    DeviceReported(String),

    /// The readback did not land within [`READBACK_DEADLINE`].
    ///
    /// A `Result` rather than a panic because the CLI's contract is exit 1 with
    /// a `--json`-shaped message, and a `panic!` here aborted with exit 101
    /// past `report::emit` entirely.
    #[error("readback did not complete within {0:?}")]
    ReadbackTimeout(Duration),
}

impl OffscreenSetup {
    /// What [`Self::request`] asks the device for, optionally.
    ///
    /// [`Features::MESH_SHADER`] is in here because
    /// `docs/plan/03-gpu-driven-rendering.md` §3.5 makes the mesh path the
    /// **primary** geometry path and a device is only on it if something asked:
    /// the flag is not part of [`Features::GPU_DRIVEN`] — that bundle is the
    /// data-layout axis, and folding a second selector into it would make it a
    /// tier again — so it has to be named beside it. Every scene here draws
    /// identically on either path, which `tests/render_e2e.rs` checks by drawing
    /// each one twice through [`Self::request_with`].
    ///
    /// Optional, never required: a device without any of these opens and draws
    /// the same picture through a lesser tail.
    pub const OPTIONAL_FEATURES: Features = Features::GPU_DRIVEN
        .union(Features::MESH_SHADER)
        // **Not implied by `MESH_SHADER`**, and asked for separately because
        // `ForwardRenderer` builds §3.5's amplification stage only where the
        // device has one — and without it `mesh_cluster.slang`'s un-amplified
        // `meshMain` emits every cluster of every level, which
        // [`Scene::Dunes`]' hierarchy cannot survive. A mesh adapter that has it
        // and a setup that did not ask is the same defect this list already
        // records for `MESH_SHADER`: the frame comes out of a lesser path and
        // nothing says so.
        .union(Features::TASK_SHADER)
        // What `ForwardRenderer::anisotropy_for` reads: without it the page is
        // sampled isotropically on hardware that could do better, and the frame
        // says nothing about the omission.
        .union(Features::SAMPLER_ANISOTROPY)
        .union(Features::DEBUG_MARKERS);

    /// Opens the auto-selected GPU backend, creates an offscreen surface,
    /// adapter, device, swapchain, and `scene`'s renderer for a frame of
    /// `(width, height)` pixels.
    ///
    /// [`Scene::default`] is [`Scene::Cube`], the frame this module drew before
    /// there was anything to choose between.
    ///
    /// Which backend is [`crate::backend`]'s decision and which adapter inside
    /// it is [`crate::adapter`]'s; [`Self::backend`] and [`Self::adapter`]
    /// report what both of them answered.
    ///
    /// Returns `Err` if the frame is not between `1x1` and
    /// [`MAX_DIMENSION`]`x`[`MAX_DIMENSION`], if no GPU is available (lavapipe,
    /// swiftshader, or a real card), if
    /// [`ADAPTER_ENV_VAR`](crate::adapter::ADAPTER_ENV_VAR) names no adapter
    /// this backend enumerated, if the device is unusable, or if any HAL call
    /// fails.
    ///
    /// **Blocks**, so `wasm32` has no `open`: browser code drives
    /// [`Self::request`] → [`PendingOffscreen::poll`] across its frame loop
    /// instead. Same rule, same reason as [`Instance::create_device`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(width: u32, height: u32, scene: Scene) -> Result<Self, OffscreenError> {
        Self::open_with(width, height, scene, Self::OPTIONAL_FEATURES)
    }

    /// Starts opening an [`OffscreenSetup`] for `scene`, without blocking.
    ///
    /// The non-blocking half of `Self::open`, and the entry a browser drives:
    /// poll the returned [`PendingOffscreen`] once per `requestAnimationFrame`
    /// frame until it hands over the setup. The instance and device are opened on
    /// [`crate::backend::request_open`] and [`Instance::request_device`], neither
    /// of which blocks, so this whole path builds and runs on `wasm32`.
    ///
    /// The size checks the blocking `open` makes are made here, before a backend
    /// is touched: an absurd `--size` costs a comparison rather than a device.
    ///
    /// # Errors
    ///
    /// The same as `Self::open`, except that everything that depends on a
    /// backend answering is deferred to [`PendingOffscreen::poll`].
    pub fn request(
        width: u32,
        height: u32,
        scene: Scene,
    ) -> Result<PendingOffscreen<'static>, OffscreenError> {
        Self::request_with(width, height, scene, Self::OPTIONAL_FEATURES)
    }

    /// [`Self::request`] asking the device for `optional_features` instead of
    /// [`Self::OPTIONAL_FEATURES`] — the non-blocking half of `Self::open_with`.
    ///
    /// # Errors
    ///
    /// The same as [`Self::request`].
    pub fn request_with(
        width: u32,
        height: u32,
        scene: Scene,
        optional_features: Features,
    ) -> Result<PendingOffscreen<'static>, OffscreenError> {
        Self::request_built(
            width,
            height,
            optional_features,
            Box::new(move |device, queue, format| SceneState::open(scene, device, queue, format)),
        )
    }

    /// [`Self::open`] asking the device for `optional_features` instead of
    /// [`Self::OPTIONAL_FEATURES`].
    ///
    /// The features are optional here for the same reason they are there: a
    /// device that lacks one still opens and still draws. What this adds is the
    /// ability to open a device *without* something the adapter has, which is
    /// the only way a caller on one machine can reach more than one
    /// [`GeometryPath`](crate::hal::GeometryPath) — every path but the best one
    /// the adapter reports is otherwise code no run here executes. `crcbl-vk`'s
    /// `Headless::open_for_mesh_with` is the same knob one layer down, and
    /// `tests/render_e2e.rs` uses this one to draw each scene twice and compare.
    ///
    /// # Errors
    ///
    /// The same as [`Self::open`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_with(
        width: u32,
        height: u32,
        scene: Scene,
        optional_features: Features,
    ) -> Result<Self, OffscreenError> {
        Self::block_open(Self::request_with(width, height, scene, optional_features)?)
    }

    /// [`Self::open`] drawing a [`ForwardScene`] the **caller** built, rather
    /// than one of the [`Scene`] variants this module owns.
    ///
    /// `build` is handed the device, the graphics queue and the surface format
    /// the ring was created with — everything
    /// [`ForwardRenderer::with_scene`](crate::render::ForwardRenderer::with_scene)
    /// needs — and hands back the renderer it filled, the camera and the sun.
    /// The frame it draws is [`Self::draw_and_readback`]'s, unchanged: same
    /// passes, same barriers, same tightly packed bytes in [`Self::format`]'s
    /// channel order.
    ///
    /// The device asks for [`Self::OPTIONAL_FEATURES`], so the room is drawn on
    /// the best path the adapter offers — [`Self::caps`] is what says which that
    /// was. [`Self::open_forward_with`] is the same frame on a path the caller
    /// names instead.
    ///
    /// A `build` that fails hands its error back and **nothing is left behind**:
    /// the swapchain, the surface and the device are released before this
    /// returns, exactly as a failing [`Scene`] arm's are.
    ///
    /// # Errors
    ///
    /// The same as [`Self::open`], plus whatever `build` returns.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_forward<F>(width: u32, height: u32, build: F) -> Result<Self, OffscreenError>
    where
        F: FnOnce(&dyn Device, QueueHandle, Format) -> Result<ForwardScene, OffscreenError>,
    {
        Self::open_forward_with(width, height, Self::OPTIONAL_FEATURES, build)
    }

    /// [`Self::open_forward`] asking the device for `optional_features` instead
    /// of [`Self::OPTIONAL_FEATURES`].
    ///
    /// [`Self::open_with`] is this knob one scene down, and it is here for the
    /// same reason: an adapter reports what it reports, so the only way a caller
    /// on one machine reaches more than one
    /// [`GeometryPath`](crate::hal::GeometryPath) is to open a device *without*
    /// a feature the adapter has. Without it every frame an application's scene
    /// draws comes off the best tail this machine offers, and the lesser ones —
    /// which is what browsers and Apple devices run — are code no run here
    /// executes. `apps/lantern/tests/golden.rs` draws its room through both and
    /// holds the two arms to one golden.
    ///
    /// # Errors
    ///
    /// The same as [`Self::open_forward`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_forward_with<F>(
        width: u32,
        height: u32,
        optional_features: Features,
        build: F,
    ) -> Result<Self, OffscreenError>
    where
        F: FnOnce(&dyn Device, QueueHandle, Format) -> Result<ForwardScene, OffscreenError>,
    {
        let pending = Self::request_built(
            width,
            height,
            optional_features,
            Box::new(move |device, queue, format| {
                build(device, queue, format).map(SceneState::from)
            }),
        )?;
        Self::block_open(pending)
    }

    /// [`Self::request`] on an instance that has already been opened.
    ///
    /// The split exists so the barrier test below can drive the whole of
    /// [`Self::draw_and_readback`] against `crcbl_hal::null`, whose recorder is
    /// the only thing in the tree that can be *asked* what command stream this
    /// module produced. The size checks stay in [`Self::request`]: they are about
    /// the caller's `--size`, and refusing before a backend is opened is the
    /// property their test asserts. Its only callers are those tests — it drives
    /// its poll core to completion with [`Self::block_open`] — so it is
    /// `#[cfg(test)]`, which is also always native.
    #[cfg(test)]
    fn open_on(
        instance: Box<dyn Instance>,
        width: u32,
        height: u32,
        scene: Scene,
        optional_features: Features,
    ) -> Result<Self, OffscreenError> {
        // The instance is already open, so this pending starts in its device
        // phase — `start_device` records the surface and adapter and starts the
        // device request the poll then drives.
        let pending = PendingOffscreen {
            width,
            height,
            optional_features,
            build: Some(Box::new(move |device, queue, format| {
                SceneState::open(scene, device, queue, format)
            })),
            phase: Self::start_device(instance, optional_features)?,
        };
        Self::block_open(pending)
    }

    /// The shared non-blocking constructor: size checks, then start the instance
    /// open and wrap it with `build` for [`PendingOffscreen::poll`] to finish.
    ///
    /// One body for every entry point, because a second copy is where the size
    /// refusals, the adapter pin, the ring depth or the teardown-on-refusal would
    /// come to differ between the engine's own scenes and an application's.
    fn request_built<'b>(
        width: u32,
        height: u32,
        optional_features: Features,
        build: BuildScene<'b>,
    ) -> Result<PendingOffscreen<'b>, OffscreenError> {
        // Checked before the backend is opened, so an absurd `--size` costs a
        // comparison rather than a device.
        if width == 0 || height == 0 {
            return Err(OffscreenError::Unusable("a frame must be at least 1x1"));
        }
        if width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(OffscreenError::TooLarge { width, height });
        }

        Ok(PendingOffscreen {
            width,
            height,
            optional_features,
            build: Some(build),
            phase: OpenPhase::Instance {
                pending: crate::backend::request_open()?,
            },
        })
    }

    /// The surface, adapter and format decided the moment an instance is open,
    /// then the device request the caller polls until it is ready.
    ///
    /// Everything here answers *now* — a foreign surface, a missing adapter, an
    /// unusable format all fail from this call. Only the device itself waits on a
    /// driver, and that is what [`PendingOffscreen::poll`] drives from the
    /// returned [`OpenPhase::Device`].
    fn start_device(
        instance: Box<dyn Instance>,
        optional_features: Features,
    ) -> Result<OpenPhase, OffscreenError> {
        let target = SurfaceTarget::Offscreen;
        // SAFETY: `Offscreen` names no platform object, so nothing can dangle.
        let surface = unsafe {
            instance
                .create_surface(&target)
                .map_err(OffscreenError::Hal)?
        };

        let adapters = instance.adapters();
        // Not `.first()`: the first enumerated adapter is not a device that
        // works on every machine, and a frame drawn on one nobody named is not
        // evidence. See [`crate::adapter`] for the measurement that moved this.
        let adapter = crate::adapter::select(crate::adapter::pin().as_deref(), &adapters)?;

        let caps = instance
            .surface_caps(surface, adapter.id)
            .map_err(OffscreenError::Hal)?;

        let format = caps
            .preferred_format()
            .ok_or(OffscreenError::Unusable("no surface format"))?;

        let pending = instance
            .request_device(&DeviceDesc {
                label: Some("crcbl screenshot"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features,
                compatible_surface: Some(surface),
            })
            .map_err(OffscreenError::Hal)?;

        Ok(OpenPhase::Device {
            instance,
            surface,
            // Cloned out of `adapters`, which is dropped at the end of this call;
            // `Self::adapter` answers from it after the enumeration is gone.
            adapter: adapter.clone(),
            format,
            pending,
        })
    }

    /// The ring and the scene, once the device is ready — the tail of an open.
    ///
    /// A `build` that fails hands its error back and **nothing is left behind**:
    /// the swapchain and the surface go back before this returns, exactly as
    /// `Scene::Dunes`' "no amplification stage" refusal must — it once destroyed
    /// its own renderer and left the swapchain and the surface behind.
    fn finish_open(
        build: BuildScene<'_>,
        instance: Box<dyn Instance>,
        surface: SurfaceHandle,
        adapter: crate::hal::AdapterInfo,
        format: Format,
        extent: (u32, u32),
        device: Box<dyn Device>,
    ) -> Result<Self, OffscreenError> {
        let queue = device
            .queue(QueueKind::Graphics)
            .ok_or(OffscreenError::Unusable("no graphics queue"))?;

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("screenshot ring"),
                surface,
                format,
                extent,
                image_count: RING_IMAGES,
                present_mode: PresentMode::Fifo,
                composite_alpha: crate::hal::CompositeAlpha::Opaque,
            })
            .map_err(OffscreenError::Surface)?;

        let scene = match build(device.as_ref(), queue, format) {
            Ok(scene) => scene,
            Err(error) => {
                device.destroy_swapchain(swapchain);
                instance.destroy_surface(surface);
                return Err(error);
            }
        };

        Ok(Self {
            instance,
            device,
            surface,
            swapchain,
            queue,
            format,
            adapter,
            scene,
            pool: TransientPool::new(),
        })
    }

    /// Drives a [`PendingOffscreen`] to completion with a tight busy-poll.
    ///
    /// The native backends make progress synchronously, so each poll either
    /// finishes the open or is one that will next time — [`std::thread::yield_now`]
    /// rather than a sleep, and never a hot spin that would starve a truly
    /// asynchronous backend on a single-core runner. Absent on `wasm32`, where a
    /// busy loop on the one thread would hang the page; browser code polls the
    /// [`PendingOffscreen`] from its frame loop instead.
    #[cfg(not(target_arch = "wasm32"))]
    fn block_open(mut pending: PendingOffscreen<'_>) -> Result<Self, OffscreenError> {
        loop {
            if let Some(setup) = pending.poll()? {
                return Ok(setup);
            }
            std::thread::yield_now();
        }
    }

    /// The surface format the readback bytes are in.
    ///
    /// The swapchain's preferred format is `Bgra8UnormSrgb` on most surfaces,
    /// and [`Self::begin_readback`] copies the swapchain image *raw*. A
    /// caller turning those bytes into an image therefore has to know whether
    /// to swizzle red and blue, and this is how it knows. Feeding them to an
    /// RGBA constructor unconditionally produces a channel-swapped PNG on
    /// every ordinary desktop surface.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// Which backend [`crate::backend`] selected for this frame.
    ///
    /// The frame itself cannot say: every backend renders the same scene
    /// through the same [`ForwardRenderer`], so a caller comparing pixels has
    /// no way to tell which one produced them. A test that pinned
    /// [`BACKEND_ENV_VAR`](crate::backend::BACKEND_ENV_VAR) and never checked
    /// would therefore pass identically on the backend it asked for and on the
    /// one it fell back to — "not supported here" arriving as a green run.
    #[must_use]
    pub fn backend(&self) -> crate::hal::BackendKind {
        self.device.backend()
    }

    /// Which adapter of that backend's enumeration the device was created on.
    ///
    /// The third of the same family as [`Self::backend`] and [`Self::caps`],
    /// and the one whose absence cost a CI run: this module took
    /// `adapters().first()` and said nothing, so a frame that died with
    /// `DXGI_ERROR_DEVICE_REMOVED` named neither the adapter it had opened nor
    /// the one it should have. A harness that pins
    /// [`ADAPTER_ENV_VAR`](crate::adapter::ADAPTER_ENV_VAR) reads this back to
    /// check the pin landed — a variable that never reached the process and a
    /// pin that was honoured look identical from outside.
    #[must_use]
    pub const fn adapter(&self) -> &crate::hal::AdapterInfo {
        &self.adapter
    }

    /// What the opened device reported it can do.
    ///
    /// The selector this exists for is
    /// [`geometry_path`](crate::hal::DeviceCaps::geometry_path): the forward
    /// pass's indirect tail is chosen from it once, at build, and is otherwise
    /// invisible from outside — so this is how a caller learns which arm a
    /// frame was actually drawn through rather than assuming the one its
    /// developer's GPU happens to select.
    #[must_use]
    pub fn caps(&self) -> crate::hal::DeviceCaps {
        self.device.caps()
    }

    /// What the last `draw_and_readback` (or [`begin_readback`](Self::begin_readback)) recorded,
    /// and what the GPU has told it since.
    ///
    /// The same [`FrameCounters`] a game puts on its debug panel, which is what
    /// makes it worth asserting here: a headless run is the only place a
    /// cross-backend test can watch the culling counters come back off the GPU,
    /// and `crcbl_render::cull_stats`' ring means they arrive several frames
    /// after the frame they describe — see
    /// [`FrameCounters::cull_frame`](crate::render::FrameCounters::cull_frame),
    /// which says which frame that was.
    #[must_use]
    pub fn counters(&self) -> FrameCounters {
        self.scene.counters()
    }

    /// Records, submits, and reads back one frame, blocking until it lands.
    ///
    /// Returns the swapchain image's bytes as `((width, height), Vec<u8>)`,
    /// four bytes per pixel, row-major, top row first, in [`Self::format`]'s
    /// channel order — **not** necessarily RGBA.
    ///
    /// The pose is fixed at `t = 0`: a screenshot is a golden-image input, and
    /// a deterministic frame is the only kind worth comparing against a
    /// reference. (There was an `advance`/`elapsed` pair here to move the
    /// clock; nothing ever called it, so every screenshot rendered `t = 0`
    /// anyway and the state only made the frame look configurable.)
    ///
    /// **Blocks** on the copy landing, so `wasm32` has none: browser code drives
    /// [`Self::begin_readback`] → [`PendingReadback::poll`] across its frame loop
    /// instead. This is the convenience over exactly that, spinning with
    /// [`std::thread::yield_now`] — never a sleep — until the copy is ready or
    /// [`READBACK_DEADLINE`] passes.
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Hal`] if recording, submission, or readback fail,
    /// [`OffscreenError::OutOfDate`] if the swapchain is stale,
    /// [`OffscreenError::TooLarge`] if the acquired extent's byte count does
    /// not fit in a `usize`, or [`OffscreenError::ReadbackTimeout`] if the copy
    /// has not landed after [`READBACK_DEADLINE`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn draw_and_readback(&mut self) -> Result<ReadbackFrame, OffscreenError> {
        let mut pending = self.begin_readback()?;
        let deadline = std::time::Instant::now() + READBACK_DEADLINE;
        loop {
            if let Some(frame) = pending.poll()? {
                return Ok(frame);
            }
            if std::time::Instant::now() > deadline {
                // The in-flight resources are left alone deliberately: the GPU
                // may still be reading them, and destroying them now would be
                // worse than leaking them until `finish` waits the device idle
                // and drops it. Dropping `pending` does not touch them.
                return Err(OffscreenError::ReadbackTimeout(READBACK_DEADLINE));
            }
            std::thread::yield_now();
        }
    }

    /// Records and submits one frame, returning the readback still in flight.
    ///
    /// The non-blocking half of `Self::draw_and_readback`: it does everything
    /// up to and including the readback request, then hands back a
    /// [`PendingReadback`] whose [`poll`](PendingReadback::poll) the caller drives
    /// until the copy lands. Builds on `wasm32`, so a browser harness reads a
    /// frame back across `requestAnimationFrame` frames without blocking.
    ///
    /// The frame is the blocking path's, unchanged — same passes, same
    /// barriers, same fixed `t = 0` pose, same bytes in [`Self::format`]'s channel
    /// order.
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Hal`] if recording or submission fail,
    /// [`OffscreenError::OutOfDate`] if the swapchain is stale, or
    /// [`OffscreenError::TooLarge`] if the acquired extent's byte count does not
    /// fit in a `usize`. The copy landing (and its own failures) is
    /// [`PendingReadback::poll`]'s.
    pub fn begin_readback(&mut self) -> Result<PendingReadback<'_>, OffscreenError> {
        let device = self.device.as_ref();
        let acquired = device
            .acquire_next_frame(self.swapchain)
            .map_err(|error| match error {
                SurfaceError::OutOfDate => OffscreenError::OutOfDate,
                other => OffscreenError::Surface(other),
            })?;

        let extent = acquired.extent;
        // The extent comes back from the swapchain rather than from `open`, so
        // it is checked here too — see `ReadbackLayout::for_extent`, which is
        // where the arithmetic lives now that the engine's `--screenshot`
        // capture reads a swapchain image back through the same steps.
        let layout = ReadbackLayout::for_extent(extent).ok_or(OffscreenError::TooLarge {
            width: extent.0,
            height: extent.1,
        })?;

        // ---- render the frame through the graph ----

        let compiled = {
            let mut graph = RenderGraph::new(self.queue);
            // The same swapchain import for every scene: what differs between
            // them is which passes are hung off it, not how it is presented.
            let target = graph.import_image(
                "swapchain",
                ForwardRenderer::present_target(acquired.image, acquired.view, self.format, extent),
            );
            match &mut self.scene {
                SceneState::Forward {
                    camera,
                    light,
                    renderer,
                } => {
                    renderer.begin_frame(device, camera, light, extent)?;
                    let _hdr = renderer.add_passes(&mut graph, &self.pool, target, extent);
                }
                SceneState::Sprite { renderer, sheets } => {
                    let sprites = sprite_scene(*sheets);
                    let aspect = extent.0 as f32 / extent.1 as f32;
                    renderer.begin_frame(
                        device,
                        &sprites,
                        sprite_camera().view_projection(aspect),
                        extent,
                    )?;
                    // Both of the passes below load rather than clear, so the
                    // scene supplies the background they composite onto.
                    graph
                        .add_render_pass("scene background")
                        .clear_color(target, SCENE_CLEAR)
                        .execute(|_| {});
                    renderer.add_pass(&mut graph, target);
                }
                SceneState::Ui { renderer, atlas } => {
                    // `scale` is 1.0 because every size in the draw list is
                    // already a fraction of this frame's extent; a second
                    // multiplier is a second thing that can disagree with it.
                    renderer.begin_frame(device, &ui_draw_list(extent), atlas, 1.0)?;
                    graph
                        .add_render_pass("scene background")
                        .clear_color(target, SCENE_CLEAR)
                        .execute(|_| {});
                    renderer.add_pass(&mut graph, target, extent);
                }
            }
            graph.compile(&self.pool)?
        };

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("screenshot frame"),
            queue: self.queue,
        });
        compiled.execute(device, &mut self.pool, encoder.as_mut(), None)?;

        // ---- readback: barrier to TransferSrc, copy, barrier back, submit ----
        //
        // The copy is recorded into **this** encoder, after the graph, so the
        // frame and its readback are one submission. `record_image_readback`
        // owns the barrier pair and says why both ends of it are `Present`.

        let staging = record_image_readback(
            device,
            encoder.as_mut(),
            acquired.image,
            self.format,
            extent,
            &layout,
            "screenshot readback",
        )?;

        let commands = encoder.finish()?;
        device.submit(self.queue, &SubmitInfo::new(&[commands]))?;
        device.present(
            self.queue,
            &PresentInfo {
                swapchain: self.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )?;

        let readback = device.request_readback(&ReadbackDesc {
            label: Some("screenshot readback"),
            buffer: staging,
            offset: 0,
            size: layout.byte_count,
            after: None,
        })?;

        let staged = vec![0u8; layout.staged_capacity];

        Ok(PendingReadback {
            setup: self,
            commands,
            staging,
            readback,
            staged,
            extent,
            layout,
        })
    }

    /// Tears down in correct order: wait idle, destroy the scene and the
    /// transient pool, then swapchain → surface → device.
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Hal`] if the device could not be brought to idle — a
    /// device lost during the frame surfaces here and nowhere else, and a
    /// caller about to save the pixels as a golden image needs to be told
    /// before it trusts them. The teardown still runs either way; the failure
    /// is reported after it.
    ///
    /// [`OffscreenError::DeviceReported`] if the device reported a failure its
    /// return values did not carry. Same reason, one layer out: pixels produced
    /// by commands the validation layer refused are not evidence about anything,
    /// and this is the only moment the offscreen path asks.
    pub fn finish(mut self) -> Result<(), OffscreenError> {
        let idle = self.device.wait_idle();
        // After the wait, because both of these hand handles back to a device
        // that may still be reading them. `SpriteRenderer` and `UiRenderer` warn
        // on a drop that skipped this; `ForwardRenderer` and `TransientPool`
        // leak silently, which is why the screenshot path used to.
        self.scene.destroy(self.device.as_ref());
        self.pool.destroy(self.device.as_ref());
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        // After the teardown and before the device goes: the destruction order
        // above is itself something the layer is watching, so asking any earlier
        // would report on the frames and miss the release.
        let reported = self.device.take_error();
        drop(self.device);
        drop(self.instance);
        // A device lost outranks it: it explains every other symptom, including
        // any message the layer produced on the way down.
        idle.map_err(OffscreenError::Hal)?;
        match reported {
            Some(message) => Err(OffscreenError::DeviceReported(message)),
            None => Ok(()),
        }
    }
}

/// A recorded, submitted frame whose readback copy has not yet landed — the
/// non-blocking half of `OffscreenSetup::draw_and_readback`.
///
/// Returned by [`OffscreenSetup::begin_readback`]; poll it once per frame with
/// [`Self::poll`] until it hands over the pixels. It blocks nowhere and builds on
/// `wasm32`, so a browser harness drives it from `requestAnimationFrame`.
///
/// Dropping one before the copy lands abandons the in-flight command buffer,
/// staging buffer and readback rather than destroying them: the GPU may still be
/// reading them, and [`OffscreenSetup::finish`] waits the device idle and frees
/// them. That is why the blocking `draw_and_readback` can drop this on a
/// timeout and leave the leak for `finish`, exactly as it always has.
#[allow(missing_debug_implementations)]
pub struct PendingReadback<'a> {
    setup: &'a OffscreenSetup,
    commands: CommandBufferHandle,
    staging: BufferHandle,
    readback: ReadbackHandle,
    /// The padded readback bytes, filled by [`Device::poll_readback`] once ready.
    staged: Vec<u8>,
    extent: (u32, u32),
    layout: ReadbackLayout,
}

impl PendingReadback<'_> {
    /// Advances the readback. `Ok(None)` means "not landed yet, poll again".
    ///
    /// On `Ready` it destroys the frame's command buffer, staging buffer and
    /// readback, drops the row padding, and hands back the pixels as
    /// [`OffscreenSetup::begin_readback`] documents — four bytes per pixel,
    /// row-major, top row first, in [`OffscreenSetup::format`]'s channel order.
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Hal`] if the readback poll fails.
    pub fn poll(&mut self) -> Result<Option<ReadbackFrame>, OffscreenError> {
        let device = self.setup.device.as_ref();
        match device.poll_readback(self.readback, &mut self.staged)? {
            ReadbackState::Pending => Ok(None),
            ReadbackState::Ready => {
                device.destroy_command_buffer(self.commands);
                device.destroy_buffer(self.staging);
                device.destroy_readback(self.readback);

                // Drop the row padding. Done here rather than left to the caller
                // because the pitch is this module's decision, and a caller that
                // forgot it would get a sheared image — the one failure a
                // structural comparison sees and a per-pixel one does not
                // describe usefully.
                let pixels = compact_rows(
                    &self.staged,
                    self.layout.staged_pitch,
                    self.layout.packed_pitch,
                    self.layout.host_capacity,
                );
                Ok(Some((self.extent, pixels)))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Readback, shared with the engine
// ---------------------------------------------------------------------------

/// The staging geometry of a readback of one four-bytes-per-texel image.
///
/// Two callers, which is why it is a type rather than six locals:
/// [`OffscreenSetup::begin_readback`] here, and
/// [`GpuContext::submit_and_present`](crate::engine::GpuContext::submit_and_present)
/// for `--screenshot`. Both have to pad to [`READBACK_ROW_ALIGNMENT`] and both
/// have to drop the padding again, and a second copy of that arithmetic is a
/// second chance to shear the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReadbackLayout {
    /// The padded row pitch the copy writes, in bytes.
    pub(crate) staged_pitch: u64,
    /// The tight row pitch the caller gets back, in bytes.
    pub(crate) packed_pitch: u64,
    /// The whole staging buffer, in bytes.
    pub(crate) byte_count: u64,
    /// `byte_count` as a host length.
    pub(crate) staged_capacity: usize,
    /// The compacted image's length, in bytes.
    pub(crate) host_capacity: usize,
    /// `staged_pitch` in *texels*, which is the unit `BufferImageCopy` names it
    /// in.
    pub(crate) staged_row_texels: u32,
}

impl ReadbackLayout {
    /// The layout for an `extent`-sized frame, or `None` if it does not fit.
    ///
    /// `None` rather than an error because the two callers report the failure
    /// in their own vocabularies — `OffscreenError::TooLarge` here and a
    /// screenshot failure in the engine — and neither of them wants the
    /// other's type.
    ///
    /// Every step is checked: `u32 * u32 * 4` overflows a `u32`, and the
    /// product has to survive narrowing to a `usize` before it can size a
    /// staging buffer or a `Vec`.
    pub(crate) fn for_extent(extent: (u32, u32)) -> Option<Self> {
        let packed_pitch = u64::from(extent.0).checked_mul(4)?;
        let staged_pitch =
            packed_pitch.checked_next_multiple_of(u64::from(READBACK_ROW_ALIGNMENT))?;
        let byte_count = staged_pitch.checked_mul(u64::from(extent.1))?;
        let packed_bytes = packed_pitch.checked_mul(u64::from(extent.1))?;
        Some(Self {
            staged_pitch,
            packed_pitch,
            byte_count,
            staged_capacity: usize::try_from(byte_count).ok()?,
            host_capacity: usize::try_from(packed_bytes).ok()?,
            // `buffer_row_length` is in texels, and the padded pitch is a
            // multiple of 4 for every 4-byte format, so this division is exact.
            staged_row_texels: u32::try_from(staged_pitch / 4).ok()?,
        })
    }
}

/// Records the barrier → copy → barrier that reads a **presented** swapchain
/// image into a fresh host-visible buffer, and returns that buffer.
///
/// The caller owns the buffer from here: submit `encoder`, request a readback
/// over it, and destroy it once the copy has landed.
///
/// # Both ends of the barrier pair are `ResourceState::Present`
///
/// Not `ColorAttachment`. A render graph does not hand the image back in the
/// state its last pass left it in: `ForwardRenderer::present_target` declares
/// `final_state: Present`, and `CompiledGraph::execute` emits a trailing
/// barrier to reach it. So `Present` is what the image is in when the copy
/// starts, and declaring anything else is a lie the API checks — lavapipe's
/// validation layer reported this one as
/// `VUID-VkImageMemoryBarrier2-oldLayout-01197`, "cannot transition … from
/// VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL when the previous known layout is
/// VK_IMAGE_LAYOUT_PRESENT_SRC_KHR".
///
/// And the second barrier is why there is a pair at all. `present` takes the
/// image back into the ring, and the next trip round declares `Undefined` —
/// legal from any layout on Vulkan, but on D3D12 `Undefined` and `Present` are
/// both `COMMON` and the declared before-state is validated, so an image left
/// in `COPY_SOURCE` makes that next declaration false. `crcbl-dx12`'s own
/// offscreen-ring suite ends every frame with this same transition for that
/// reason.
///
/// # Errors
///
/// [`HalError`](crate::hal::HalError) if the staging buffer could not be
/// allocated.
pub(crate) fn record_image_readback(
    device: &dyn Device,
    encoder: &mut dyn crate::hal::CommandEncoder,
    image: crate::hal::ImageHandle,
    format: Format,
    extent: (u32, u32),
    layout: &ReadbackLayout,
    label: &str,
) -> Result<BufferHandle, crate::hal::HalError> {
    let staging = device.create_buffer(&BufferDesc {
        label: Some(label),
        size: layout.byte_count,
        usage: BufferUsage::TRANSFER_DST,
        memory: MemoryLocation::HostReadback,
    })?;

    let range = ImageSubresourceRange::all(format);
    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            image,
            range,
            ResourceState::Present,
            ResourceState::TransferSrc,
        )],
        ..Barriers::default()
    });

    encoder.copy_image_to_buffer(&BufferImageCopy {
        buffer: staging,
        buffer_offset: 0,
        buffer_row_length: layout.staged_row_texels,
        buffer_image_height: 0,
        image,
        image_subresource: ImageSubresourceLayers {
            aspect: ImageAspect::COLOR,
            mip: 0,
            base_layer: 0,
            layer_count: 1,
        },
        image_offset: Offset3d::default(),
        image_extent: Extent3d::d2(extent.0, extent.1),
    });

    encoder.pipeline_barrier(&Barriers {
        images: &[ImageBarrier::new(
            image,
            range,
            ResourceState::TransferSrc,
            ResourceState::Present,
        )],
        ..Barriers::default()
    });

    Ok(staging)
}

/// The channel order a readback in `format` arrives in.
///
/// Named after the memory order, like the HAL format itself, so the mapping is
/// a rename rather than a judgement.
///
/// **The bug it exists to stop:** an ordinary desktop surface prefers
/// `Bgra8UnormSrgb`, and a readback of one handed to
/// [`Image::from_rgba8`](crcbl_golden::Image::from_rgba8) writes a PNG with red
/// and blue swapped — swapped in a way a structural comparison cannot see,
/// because SSIM is computed on luma and a channel swap barely moves it.
/// [`Image::from_readback`](crcbl_golden::Image::from_readback) does the
/// swizzle, and this is what tells it which one.
///
/// Native-only because [`crcbl_golden`] is: it is what puts a PNG encoder in a
/// binary, and a browser build has no file to write one to.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn channel_order(format: Format) -> crcbl_golden::ChannelOrder {
    match format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => crcbl_golden::ChannelOrder::Bgra,
        _ => crcbl_golden::ChannelOrder::Rgba,
    }
}

/// Copies `packed_pitch` bytes out of every `staged_pitch`-byte row.
///
/// A free function so the arithmetic is testable with no GPU: this is the step
/// that turns a padded readback back into an image, and getting it wrong shears
/// the frame by a few pixels per row — which looks like a rendering bug and is
/// not one.
///
/// A short final row is copied as far as it goes rather than dropped: a backend
/// that wrote less than it promised should produce a visibly truncated image,
/// not a panic in the middle of a screenshot.
pub(crate) fn compact_rows(
    staged: &[u8],
    staged_pitch: u64,
    packed_pitch: u64,
    packed_len: usize,
) -> Vec<u8> {
    if staged_pitch == packed_pitch {
        let end = packed_len.min(staged.len());
        return staged[..end].to_vec();
    }
    // Both pitches sized a `Vec` above, so both fit a `usize`.
    let staged_pitch = staged_pitch as usize;
    let packed_pitch = packed_pitch as usize;
    let mut packed = Vec::with_capacity(packed_len);
    let mut offset = 0usize;
    while offset < staged.len() && packed.len() < packed_len {
        let row_end = (offset + packed_pitch).min(staged.len());
        packed.extend_from_slice(&staged[offset..row_end]);
        offset += staged_pitch;
    }
    packed.truncate(packed_len);
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The padding is only correct if it is dropped again, and this is the
    /// arithmetic that drops it. A GPU is not needed to check it, so it is
    /// checked in the plain suite that runs everywhere rather than only in the
    /// e2e run that needs two backends.
    #[test]
    fn a_padded_readback_compacts_back_to_a_tight_image() {
        // Three rows of two RGBA pixels each, staged at a 16-byte pitch.
        let staged = vec![
            1, 2, 3, 4, 5, 6, 7, 8, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, //
            9, 10, 11, 12, 13, 14, 15, 16, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, //
            17, 18, 19, 20, 21, 22, 23, 24, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
        ];
        let packed = compact_rows(&staged, 16, 8, 24);
        assert_eq!(
            packed,
            (1u8..=24).collect::<Vec<u8>>(),
            "the padding bytes must not survive"
        );
    }

    /// The unpadded case is the one every existing caller was already on, and
    /// it must stay a straight copy.
    #[test]
    fn an_unpadded_readback_is_copied_verbatim() {
        let staged: Vec<u8> = (0u8..32).collect();
        assert_eq!(compact_rows(&staged, 8, 8, 32), staged);
        // A staging buffer larger than the image is truncated, never read past.
        assert_eq!(compact_rows(&staged, 8, 8, 16), staged[..16].to_vec());
    }

    /// A backend that returned a short buffer must produce a short image, not
    /// an out-of-range slice.
    #[test]
    fn a_truncated_readback_does_not_panic() {
        let staged: Vec<u8> = (0u8..20).collect();
        let packed = compact_rows(&staged, 16, 8, 24);
        assert_eq!(packed, vec![0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19]);
    }

    /// The pitch rule the wgpu backend enforces, stated as arithmetic: every
    /// width has to land on a multiple of 256 bytes.
    #[test]
    fn the_staged_pitch_is_always_a_legal_wgpu_row_pitch() {
        for width in [1u32, 3, 32, 63, 64, 97, 256, 1920, 4096] {
            let packed = u64::from(width) * 4;
            let staged = packed
                .checked_next_multiple_of(u64::from(READBACK_ROW_ALIGNMENT))
                .expect("no overflow at these widths");
            assert!(staged >= packed, "{width}: padding may not shrink a row");
            assert_eq!(
                staged % u64::from(READBACK_ROW_ALIGNMENT),
                0,
                "{width}: wgpu refuses this pitch",
            );
            assert_eq!(staged % 4, 0, "{width}: texel count must be exact");
        }
    }

    /// `--size 4000000000x4000000000` used to reach an unchecked
    /// `width * height * 4`, and `--size 100000x100000` a 40 GB allocation.
    /// Both are now refused, and refused *before* a GPU is opened, which is
    /// what makes this testable without one.
    #[test]
    fn an_absurd_frame_is_refused_before_a_backend_is_opened() {
        for (width, height) in [
            (u32::MAX, u32::MAX),
            (4_000_000_000, 4_000_000_000),
            (100_000, 100_000),
            (MAX_DIMENSION + 1, 1),
            (1, MAX_DIMENSION + 1),
        ] {
            let error = OffscreenSetup::open(width, height, Scene::default())
                .err()
                .unwrap_or_else(|| panic!("{width}x{height} is not a frame"));
            assert!(
                matches!(error, OffscreenError::TooLarge { .. }),
                "{width}x{height}: {error}"
            );
        }
    }

    /// A zero edge would make a swapchain nothing can present, and the error
    /// says so rather than being a validation-layer message later.
    #[test]
    fn a_zero_sized_frame_is_refused() {
        for (width, height) in [(0, 16), (16, 0), (0, 0)] {
            assert!(
                matches!(
                    OffscreenSetup::open(width, height, Scene::default()),
                    Err(OffscreenError::Unusable(_))
                ),
                "{width}x{height} should be unusable"
            );
        }
    }

    /// The sprite scene is only diagnostic if it **batches**, and batching is
    /// the submission order: `A A B A` is three batches, `A A B B` is two and
    /// `A A A A` is one. One batch is exactly the case that hid the
    /// `SV_InstanceID` divergence, because with a single draw the SPIR-V and
    /// WGSL lowerings of the instance index are the same number.
    #[test]
    fn the_sprite_scene_submits_three_batches_and_returns_to_the_first_sheet() {
        let batches: Vec<usize> = SPRITE_ORDER.iter().fold(Vec::new(), |mut runs, sheet| {
            if runs.last() != Some(sheet) {
                runs.push(*sheet);
            }
            runs
        });
        assert_eq!(
            batches,
            vec![0, 1, 0],
            "the scene must interleave its sheets, not group them"
        );
        // The last batch is what the bug got wrong: its instances start at 3,
        // and a backend reading the wrong index draws instance 0 instead.
        assert!(
            SPRITE_ORDER.len() > batches.len(),
            "at least one batch must carry more than one instance"
        );
    }

    /// Every rectangle has to be inside the frame at every size the harness
    /// renders, or the scene silently loses the batch the size cropped — and a
    /// missing batch is the very thing it is there to catch.
    #[test]
    fn every_sprite_rectangle_is_on_screen_at_the_harness_sizes() {
        for (width, height) in [(256u32, 192u32), (97, 61), (1920, 1080), (192, 192)] {
            let half_width = SPRITE_HALF_HEIGHT * (width as f32 / height as f32);
            for rect in SPRITE_RECTS {
                let (left, right) = (rect[0], rect[0] + rect[2]);
                let (bottom, top) = (rect[1], rect[1] + rect[3]);
                assert!(
                    left > -half_width && right < half_width,
                    "{width}x{height}: {rect:?} runs off the side of a ±{half_width} view"
                );
                assert!(
                    bottom > -SPRITE_HALF_HEIGHT && top < SPRITE_HALF_HEIGHT,
                    "{width}x{height}: {rect:?} runs off the top or bottom"
                );
            }
        }
    }

    /// The alignment `SPECULAR_STRIP_PITCH` argues for is what keeps
    /// `specular_aa`'s golden the same picture on a rasteriser carrying the
    /// four sub-pixel bits Vulkan guarantees as on one carrying eight. The
    /// builder asserts it vertex by vertex through the scene's camera; this
    /// pins the constant the vertices are spaced by and the count they come
    /// in, so the arithmetic is stated in a test that names it rather than
    /// only inside a loop.
    #[test]
    fn the_specular_plate_is_cut_into_strips_two_pixels_wide() {
        let pixels = SPECULAR_STRIP_PITCH * SPECULAR_ALIGNED_PIXELS_PER_UNIT;
        assert!(
            (pixels - 2.0).abs() <= SPECULAR_ALIGNMENT_SLACK,
            "a strip is {pixels} pixels across, not the two the alignment needs"
        );
        assert_eq!(
            SPECULAR_STRIPS % SPECULAR_STRIPS_PER_CLUSTER,
            0,
            "a strip count that is not whole clusters leaves a remainder cluster with bounds \
             arithmetic of its own"
        );
        // Which is the builder's own assertion, over every vertex it writes,
        // projected through `specular_camera` and `specular_plate`.
        let _ = specular_plate_mesh();
    }

    /// The UI scene has to exercise rectangles, outlines and text — a frame of
    /// rectangles never samples the glyph atlas, so a broken atlas binding
    /// would draw an identical picture on both backends. Strokes are asserted
    /// absent rather than ignored, so adding one to the scene has to come back
    /// here and say what the comparison should expect of it.
    #[test]
    fn the_ui_scene_draws_text_a_rect_and_an_outline_inside_the_frame() {
        use crate::ui::draw_list::DrawCommand;

        for extent in [(256u32, 192u32), (97, 61), (1920, 1080)] {
            let list = ui_draw_list(extent);
            let (mut rects, mut outlines, mut texts, mut strokes) = (0, 0, 0, 0);
            for command in list.commands() {
                match command {
                    DrawCommand::Rect { min, max, .. } => {
                        rects += 1;
                        assert!(min.x >= 0.0 && min.y >= 0.0, "{extent:?}: {command:?}");
                        assert!(
                            max.x <= extent.0 as f32 && max.y <= extent.1 as f32,
                            "{extent:?}: {command:?}"
                        );
                    }
                    DrawCommand::RectOutline { .. } => outlines += 1,
                    // The scene draws no strokes; counted so that adding one
                    // later has to come back here and say what it expects.
                    DrawCommand::Line { .. } | DrawCommand::Polyline { .. } => strokes += 1,
                    // The glyphs' extent is the atlas's business, so only the
                    // anchor is checked here.
                    DrawCommand::Text { pos, .. } => {
                        texts += 1;
                        assert!(
                            pos.x >= 0.0 && pos.y >= 0.0 && pos.y < extent.1 as f32,
                            "{extent:?}: {command:?}"
                        );
                    }
                }
            }
            assert!(
                rects >= 2 && outlines == 1 && texts >= 1 && strokes == 0,
                "{extent:?}: {rects} rect(s), {outlines} outline(s), {texts} text(s), \
                 {strokes} stroke(s)"
            );
        }
    }

    /// One more frame than the ring is deep, so the last one is drawn into an
    /// image an earlier one already used.
    const LAPS: usize = RING_IMAGES as usize + 1;

    /// Every barrier [`OffscreenSetup::draw_and_readback`] records on the
    /// acquired swapchain image tells the truth, and every image goes back into
    /// the ring in [`ResourceState::Present`].
    ///
    /// # Why this is a state machine and not two `assert_eq!`s
    ///
    /// A barrier is a *claim* about the state its image is already in, and both
    /// halves of a wrong claim are silent here: the frame still renders, the
    /// pixels still compare equal, and nothing above the seam ever reads the
    /// state back. This module shipped with `from: ColorAttachment` on the
    /// pre-copy barrier — the state the *last pass* leaves the target in, not
    /// the state the graph hands it back in, which is
    /// [`ForwardRenderer::present_target`]'s `final_state: Present` — and with
    /// no barrier back at all, and the golden suite passed on every backend. It
    /// took Vulkan's validation layer to say so, and only on the first of the
    /// two.
    ///
    /// So the observable has to be the command stream itself, which is what
    /// `crcbl_hal::null`'s recorder is: replaying it with a tracker is the same
    /// check a driver's validation layer performs, in the plain suite that runs
    /// with no GPU at all.
    ///
    /// # Why three frames
    ///
    /// The ring is [`RING_IMAGES`] deep, so the third acquire is the first that
    /// hands back an image a previous frame already used. A residual state is
    /// invisible until then: it is legal for the graph to declare `Undefined`
    /// coming in — every backend accepts that as "discard the contents" — but
    /// D3D12 spells both `Undefined` and `Present` `D3D12_RESOURCE_STATE_COMMON`
    /// and validates the *declared* before-state, so an image left in
    /// `TransferSrc` makes the next trip's declaration false. `Present` at the
    /// hand-back is what makes it true, and lap three is where the two differ.
    #[test]
    fn every_readback_barrier_declares_the_state_the_image_is_actually_in() {
        use crate::hal::null::{Command, Event, NullInstance, Recorder};

        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let mut setup = OffscreenSetup::open_on(
            Box::new(instance),
            16,
            16,
            Scene::Cube,
            OffscreenSetup::OPTIONAL_FEATURES,
        )
        .expect("the null backend opens an offscreen setup");
        for _ in 0..LAPS {
            setup
                .draw_and_readback()
                .expect("the null backend records a frame and reads it back");
        }
        setup.finish().expect("the null device reaches idle");

        // The state each swapchain image was last left in, by handle. Two
        // entries at most, so a list rather than a map.
        let mut tracked: Vec<(crate::hal::ImageHandle, ResourceState)> = Vec::new();
        // The image each lap acquired, in order — asserted below to repeat.
        let mut per_lap: Vec<crate::hal::ImageHandle> = Vec::new();
        // The lap's own barriers, held until its copy names the image they are
        // about: the pre-copy barrier is recorded before the copy that
        // identifies it.
        let mut pending: Vec<crate::hal::ImageBarrier> = Vec::new();
        let mut current: Option<crate::hal::ImageHandle> = None;

        for event in recorder.events() {
            match event {
                Event::Acquired { .. } => {
                    assert!(
                        current.is_none(),
                        "a lap acquired again before presenting the image it held"
                    );
                    pending.clear();
                }
                Event::Command {
                    command: Command::Barrier { images, .. },
                    ..
                } => pending.extend(images),
                Event::Command {
                    command: Command::CopyImageToBuffer(copy),
                    ..
                } => {
                    assert!(
                        current.is_none(),
                        "this frame copies one image back per lap; a second copy \
                         means the image a barrier is about can no longer be \
                         identified by it"
                    );
                    current = Some(copy.image);
                    per_lap.push(copy.image);
                }
                Event::Presented { .. } => {
                    let image = current
                        .take()
                        .expect("a lap presents the image it copied back");
                    for barrier in pending.drain(..).filter(|it| it.image == image) {
                        let slot = tracked.iter_mut().find(|(handle, _)| *handle == image);
                        let was = slot.as_ref().map_or(ResourceState::Undefined, |(_, s)| *s);
                        // `Undefined` is the one declaration that is true from
                        // anywhere: it discards the contents rather than
                        // claiming to know them.
                        assert!(
                            barrier.from == ResourceState::Undefined || barrier.from == was,
                            "a barrier declared {:?} -> {:?} on a swapchain image that is \
                             in {was:?}",
                            barrier.from,
                            barrier.to,
                        );
                        match slot {
                            Some((_, state)) => *state = barrier.to,
                            None => tracked.push((image, barrier.to)),
                        }
                    }
                    let (_, state) = tracked
                        .iter()
                        .find(|(handle, _)| *handle == image)
                        .expect("the lap barriered the image it copied");
                    assert_eq!(
                        *state,
                        ResourceState::Present,
                        "present takes the image back into the ring, and the next trip \
                         round declares Undefined — which D3D12 spells COMMON and \
                         validates, so anything but Present here is a state the next \
                         lap's declaration contradicts"
                    );
                }
                _ => {}
            }
        }

        assert_eq!(
            per_lap.len(),
            LAPS,
            "every lap must have copied its image back, or the barriers of the \
             ones that did not were never checked"
        );
        assert_eq!(
            per_lap[0],
            per_lap[LAPS - 1],
            "the ring is {RING_IMAGES} deep, so lap {LAPS} must re-use lap 1's image; \
             a ring that handed out a fresh image every time would never re-read a \
             residual state"
        );
        assert_ne!(
            per_lap[0], per_lap[1],
            "consecutive laps must take different ring images"
        );
    }

    /// The frame the requested [`Scene`] promises, and nothing left behind.
    ///
    /// Every scene reaches the same [`OffscreenSetup::draw_and_readback`], and
    /// what tells them apart is only which passes get hung off the swapchain
    /// import — so the passes the device actually executed are the observable
    /// that says the right one ran. A `Sprite` frame that quietly drew a cube,
    /// or a `Ui` frame whose composite pass dropped out because
    /// [`UiRenderer::add_pass`](crate::render::UiRenderer::add_pass) found
    /// nothing to draw, both hand back the same pixel count and the same `Ok`.
    ///
    /// This test replaced one that opened the null backend, hand-rolled a
    /// swapchain and a graph beside this module rather than through it, and
    /// asserted nothing at all — its own doc comment conceded it proved the
    /// module compiles. [`crate::backend`] already covers opening the null
    /// backend by name, and [`crate::render`] covers the forward pass list, so
    /// what is left for this module to own is the composition above: which
    /// passes each scene contributes, the shape of the bytes handed back, and
    /// [`OffscreenSetup::finish`] giving back everything the setup took.
    #[test]
    fn every_scene_records_the_passes_it_names_and_gives_back_every_object_at_finish() {
        use crate::hal::null::{Event, NullInstance, Recorder};

        // `(kind, label)` as `Command::opens_pass` reports them.
        //
        // **The cube scene's compute triple appears once per cull, and there is
        // one cull per shadow cascade beside the camera's.** Built rather than
        // written out, so the list tracks
        // `crcbl_render::shadow::CASCADES` instead of being a literal that
        // silently stops matching when the count changes — which is the one
        // thing this assertion exists to notice, since a cascade whose cull
        // never ran draws an empty tile and an entirely lit frame.
        //
        // Topic 18's clustering dispatch joins the camera's triple and no
        // cascade's, because a cascade shades nothing: one froxel grid per
        // camera is the whole of what the light list costs a frame.
        //
        // **A shadowed light adds one more triple and nothing else**, which is
        // the one thing the spot slice changed about a frame's shape: a light
        // that was given an atlas tile gets a cull of its own, exactly as a
        // cascade does, and a light that was not given one costs nothing at all.
        // `forward_passes(0)` and `forward_passes(1)` differing by one triple is
        // what says the free tiles really are free.
        let forward_passes = |shadowed_lights: usize| {
            let mut passes: Vec<(&str, &str)> = Vec::new();
            for cull in 0..=(crcbl_render::shadow::CASCADES + shadowed_lights) {
                passes.extend([
                    ("compute", "clear-counters"),
                    ("compute", "cull"),
                    ("compute", "draw-args"),
                ]);
                if cull == 0 {
                    passes.push(("compute", "light-cluster"));
                }
            }
            // `docs/plan/18-render-features.md`'s occlusion slice added the
            // middle four, in this order and no other: the prepass has to write
            // the depth `ssao` reads, `ssao-blur` has to have raw occlusion to
            // blur, `ssao-upsample` has to have a blurred half-resolution
            // channel to widen, and `forward` has to have the full-resolution
            // channel before it can scale its ambient by it. A frame that runs them in any other order
            // still draws — each pass reads whatever the last frame left in the
            // pooled transient — so the sequence is asserted here rather than
            // trusted to the graph.
            //
            // **`ssr` and `ssr-blur` are after `forward` and before `tonemap`,
            // and every half matters.** The march reads the depth prepass for a
            // colour out of the scene target, so it cannot run before the pass
            // that wrote that target; the blur cannot run before the march it
            // filters; and the blur *is* the composite, so a tonemap scheduled
            // first would resolve the frame without the reflections in it. None
            // of those mistakes is visible as anything but a picture.
            passes.extend([
                ("render", "shadow"),
                ("render", "depth-prepass"),
                ("render", "ssao"),
                ("render", "ssao-blur"),
                // **A second blur is a pass, so it is a row here.**
                // `r_ssao_blur_passes` defaults to two since 2026-09-03 and a
                // pass runs per count, which is also what makes a run at the
                // higher count checkable rather than asserted: this row appears
                // only when a second blur really ran. If that default moves
                // back, this test is what goes red — which is the job of a
                // fixture expectation.
                ("render", "ssao-blur-2"),
                ("render", "ssao-upsample"),
                ("render", "forward"),
                // **The pyramid the march climbs, and it is part of the march
                // rather than of the prepass**: `crcbl_render::hiz` records a
                // reduction per level only on the frames that reflect, so this
                // row disappears with `ssr` below rather than with
                // `depth-prepass` above. One level, because these fixtures
                // render at a size that halves once before hitting the floor
                // `crcbl_render::hiz` keeps — the same arithmetic that gives the
                // bloom chain below its single downsample.
                ("render", "hiz-1"),
                ("render", "ssr"),
                ("render", "ssr-blur"),
                ("render", "tonemap"),
                // **After the tonemap, and that is where the resolve belongs**:
                // it reads the display-space image the tonemap wrote and writes
                // the target. A resolve scheduled before it would filter the
                // scene's high-dynamic-range values, where the thresholds it
                // tests were fitted to a displayable image.
                ("render", "fxaa"),
            ]);
            passes
        };
        let cube_passes = forward_passes(0);
        // The probe fixture disables reflections: its Rust mirror predicts only
        // diffuse irradiance, and a rough probe fallback would be an unmodelled
        // specular term in every measured floor pixel.
        let mut probe_passes = forward_passes(0);
        probe_passes.retain(|(_, label)| !matches!(*label, "hiz-1" | "ssr" | "ssr-blur"));
        // The spot scene's one light is a spot, it is the only candidate, and
        // there are tiles left after the cascades — so it holds one.
        let spot_shadow_passes = forward_passes(1);
        // **Two triples, not three**, and that is the point-light budget visible
        // in a frame's shape: `Scene::Lights` has three point lights, each of
        // which is `shadow::POINT_FACES` tiles, and the light region holds two
        // such runs — so the two most influential are shadowed and the third
        // lights without occluding.
        let lights_passes = forward_passes(2);
        // **The one fixture whose camera stack asks for the lens**, and the only
        // row here whose length is a function of the frame's size:
        // `crcbl_render::bloom` derives the chain from the extent, and a 16×16
        // target has room for exactly one level — so one downsample and the
        // composite that ends it.
        //
        // Spliced in front of the tonemap rather than written out from scratch,
        // because *where* it goes is the claim: the chain reads the image the
        // tonemap would have read and hands it a different one, so a chain
        // scheduled after the tonemap would resolve the frame without the halo in
        // it and still draw a picture.
        let mut bloom_passes = forward_passes(0);
        let before_tonemap = bloom_passes
            .iter()
            .position(|(_, label)| *label == "tonemap")
            .expect("every forward frame ends in a tonemap");
        bloom_passes.splice(
            before_tonemap..before_tonemap,
            [("render", "bloom-down-1"), ("render", "bloom-composite")],
        );
        // **`Scene::AtmosphereMirror` is the shortest forward frame here**, and
        // written out rather than derived because what it differs from the cube
        // list by is most of that list: its request is the reflection pair
        // alone, so there is no occlusion chain and no resolve, and with the
        // shadow bit clear the cascades cost no culls — one triple for the
        // camera and nothing else. `crcbl_render::shadow` still opens its atlas
        // pass to clear the tiles it will not draw into, which is why `shadow`
        // is still a row and why deriving this list from `forward_passes` would
        // have hidden that.
        let mirror_passes: &[(&str, &str)] = &[
            ("compute", "clear-counters"),
            ("compute", "cull"),
            ("compute", "draw-args"),
            ("compute", "light-cluster"),
            ("render", "shadow"),
            ("render", "depth-prepass"),
            ("render", "forward"),
            ("render", "sky"),
            ("render", "hiz-1"),
            ("render", "ssr"),
            ("render", "ssr-blur"),
            ("render", "tonemap"),
        ];
        let expected: [(Scene, &[(&str, &str)]); 14] = [
            (Scene::Cube, &cube_passes),
            // The cube scene's list again, and that is the whole of what
            // `Scene::Aa` costs a frame now: the resolve is in
            // `RenderEffects::DEFAULT_STACK`, so every forward row here carries
            // it and this fixture's own request adds nothing. What makes the
            // scene a fixture is its content — see that variant.
            (Scene::Aa, &cube_passes),
            // Not the cube scene's passes: `Scene::Lights` is the cube scene
            // with a longer light list, the clustering dispatch is one per
            // camera however many lights it assigns — and two of those lights
            // are shadowed, which costs the cull triples `lights_passes`
            // carries.
            (Scene::Lights, &lights_passes),
            // The same passes: `Scene::Dunes` is the same renderer with
            // different content, and a scene that quietly stopped running the
            // cull pair would still draw a plausible frame.
            (Scene::Dunes, &cube_passes),
            // **One triple more**, and it is the whole of what a shadowed light
            // costs a frame: `crcbl_render::shadow::Selection` gave this scene's
            // spot a tile, so the shadow pass runs a cull against the light's own
            // frustum before drawing into it. A scene whose light was refused a
            // tile records the cube scene's list unchanged — which is
            // `docs/plan/18-render-features.md`'s "still lights, does not
            // occlude" visible in the frame's shape rather than only in its
            // pixels.
            (Scene::SpotShadow, &spot_shadow_passes),
            // **One triple, not six.** A point light's six faces are six
            // viewports and six matrices over *one* cull — topic 18's fourth
            // decision — so a shadowed point light costs a frame exactly what a
            // shadowed spot does. A frame that culled per face would record five
            // more triples here.
            (Scene::PointShadow, &spot_shadow_passes),
            // The cube scene's passes again: `Scene::Ao` is a different room
            // under a different sun, and neither of those is a pass. Its sun is
            // directional and its light list is empty, so it holds no atlas tile
            // — which is what makes this row `cube_passes` and not
            // `spot_shadow_passes`.
            (Scene::Ao, &cube_passes),
            // And again for `Scene::Ssr`, which is the cube scene's list for the
            // same two reasons: a directional sun and an empty light list, so no
            // atlas tile and no cull of its own. The `ssr` pass itself is in
            // every row here — it is not a scene's to opt into.
            (Scene::Ssr, &cube_passes),
            // The one row whose frame draws a **background**, and the one row
            // that asks for the reflection pair alone — see the variant, whose
            // bands are predicted absolutely and so cannot carry an occlusion
            // term or a resolve. `sky` sits after `forward` and before the
            // march, which is where `crcbl_render::forward` puts it: a sky
            // composited after the reflection would be a background the mirror
            // never saw.
            (Scene::AtmosphereMirror, mirror_passes),
            // The same list, and that is a claim rather than a coincidence:
            // `Scene::GradientMirror` is the row above with a gradient in
            // place of the atmosphere, and a sky is data the same passes read
            // rather than a pass of its own. The `sky` row is still here
            // because a frame with either kind of sky draws a background.
            (Scene::GradientMirror, mirror_passes),
            // The only row that is not one of the two lists above: every other
            // fixture draws `RenderEffects::DEFAULT_STACK`, which leaves the
            // lens effect out — see that constant, and see `Scene::Bloom` for
            // why this one asks for it.
            (Scene::Bloom, &bloom_passes),
            // Unlike the other forward scenes, `Scene::Probes` explicitly
            // refuses the reflection pair: its absolute Rust-mirror assertion
            // measures diffuse irradiance alone, and rough probe specular would
            // otherwise be an unmodelled addition to every floor pixel.
            (Scene::Probes, &probe_passes),
            (
                Scene::Sprite,
                &[("render", "scene background"), ("render", "sprites")],
            ),
            (
                Scene::Ui,
                &[("render", "scene background"), ("render", "ui-composite")],
            ),
        ];

        for (scene, passes) in expected {
            let recorder = Recorder::new();
            let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
            let before = recorder.total_live_objects();

            let mut setup = OffscreenSetup::open_on(
                Box::new(instance),
                16,
                16,
                scene,
                OffscreenSetup::OPTIONAL_FEATURES,
            )
            .expect("the null backend opens an offscreen setup");
            recorder.clear(); // the setup's uploads are not this frame's passes

            let (extent, pixels) = setup
                .draw_and_readback()
                .expect("the null backend records a frame and reads it back");
            assert_eq!(extent, (16, 16), "{scene:?}");
            assert_eq!(
                pixels.len(),
                16 * 16 * 4,
                "{scene:?}: the caller gets tightly packed RGBA, padding dropped"
            );

            let recorded: Vec<(String, String)> = recorder
                .events()
                .into_iter()
                .filter_map(|event| match event {
                    Event::Command { command, .. } => command.opens_pass().map(|(kind, label)| {
                        (
                            kind.to_string(),
                            label
                                .expect("every pass this module adds is labelled")
                                .to_string(),
                        )
                    }),
                    _ => None,
                })
                .collect();
            let expected: Vec<(String, String)> = passes
                .iter()
                .map(|(kind, label)| ((*kind).to_string(), (*label).to_string()))
                .collect();
            assert_eq!(recorded, expected, "{scene:?}");
            // And the bound every sample sizes its timers with really does
            // cover them. Asserted here rather than beside the list above
            // because these are the passes the *device* was handed, which is
            // the same stream `PassTimers` brackets — a bound short of it times
            // a prefix of the frame and drops the rest, which is what the
            // samples' hand-picked `8` had been doing.
            assert!(
                recorded.len() <= crcbl_render::MAX_TIMED_PASSES as usize,
                "{scene:?}: {} passes recorded, past the bound of {}",
                recorded.len(),
                crcbl_render::MAX_TIMED_PASSES
            );

            setup.finish().expect("the null device reaches idle");
            assert_eq!(
                recorder.total_live_objects(),
                before,
                "{scene:?}: finish must give back every object the setup and the frame took"
            );
            recorder.assert_valid();
        }
    }
}
