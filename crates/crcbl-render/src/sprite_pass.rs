//! Sprite pass: instanced, world-space, alpha-blended quads cut out of sheets.
//!
//! ```text
//! SpriteRenderer ──register_sheet──▶ uploads a sheet, returns a SheetId
//!      │
//!      ├──begin_frame──▶ batches consecutive sprites sharing a sheet, uploads
//!      │                 one instance per sprite and one constant block per
//!      │                 batch
//!      │
//!      └──add_pass────▶ inserts an alpha-blended render pass into the graph:
//!                       one draw(0..6, 0..count) per batch, with the batch's
//!                       place in the instance buffer arriving as a dynamic
//!                       offset into the constants — see `add_pass` for why it
//!                       cannot be the draw's own firstInstance
//! ```
//!
//! # Why this exists
//!
//! `docs/plan/ROADMAP.md`'s S1B finding 1: **`ForwardRenderer` draws exactly one
//! instance**, so both samples put their world through [`crate::ui_pass`] — a
//! path that takes screen pixels, merges every quad into one vertex buffer and
//! re-triangulates all of it on the CPU every frame. Breakout did it for forty
//! bricks and flappy for its pipes, independently, for the same reason. This is
//! the instance path that finding asks for: one instance per sprite, the quad
//! from `SV_VertexID`, the per-sprite data from a storage buffer indexed by
//! `SV_InstanceID`, and world coordinates rather than pixels.
//!
//! # One shader, no tier split
//!
//! `sprite.slang` takes its constants from a uniform buffer on *every* tier, so
//! there is one artifact, one code path, and nothing here to pick between. A
//! push constant would save one indirection per vertex where they exist, and
//! cost a permutation axis: WebGPU has none at all, and an entry point reads
//! either a push-constant block or a bound one, so a second shader source is the
//! only way to serve both. [`crate::ui_pass`] paid that — a `ConstantDelivery`
//! enum and a `ui_tier_b.slang` kept in step by hand — until it took this
//! pass's trade instead.
//!
//! # Sample modes, and where the mode lives
//!
//! There is **one sampler**, linear, and every sheet's bind group names it. The
//! mode is not in the bind group at all: it rides on the instance, in
//! [`SpriteInstance::sheet`], alongside the sheet's size in texels.
//!
//! That is forced by what [`SampleMode::Pixel`] actually is.
//! `crcbl_sprite::SampleMode`'s own docs say it is *not* nearest-neighbour: at a
//! non-integer scale — a 320-wide field across a 1366-wide canvas — nearest
//! makes some art pixels four screen pixels across and their neighbours five,
//! and the unevenness crawls as the sprite moves. What it is instead is
//! **sharp-bilinear**: sample *linearly*, with the UV bent so the blend happens
//! only inside a one-fragment band at each texel boundary, plus the quad's
//! screen rectangle snapped to whole device pixels so its edges cannot straddle
//! a fragment. Both halves need a linear sampler and both need numbers — the
//! sheet's texel size, and the target's size in pixels — that a
//! `SamplerState` cannot supply. So the sampler stops being the carrier: one
//! sampler, one pipeline, one shader, and two `float4` lanes on a buffer that
//! was already written once per sprite per frame.
//!
//! [`SampleMode::Smooth`] samples the vertex stage's UV unchanged and is not
//! snapped, which is what art that is not pixel art wants.
//! `sprite.slang`'s `sharpen` carries the derivation of the band's width.
//!
//! # What the tests below are and are not
//!
//! They are recorder assertions: the instance bytes, the batching, the draw
//! coverage, the teardown. The evidence that this draws the right *picture* is
//! `crates/crcbl-vk/tests/vk_e2e.rs`'s sprite goldens, which run the real pass
//! against a real driver and read the pixels back — including the one assertion
//! that pins the sharp-bilinear arithmetic, that a `Pixel` sprite at a whole
//! scale is exactly flat inside each texel.

use core::ops::Range;

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, BlendState, BufferDesc,
    BufferHandle, BufferUsage, ColorTargetState, ColorWrites, DepthStencilState, Device,
    FilterMode, Format, GraphicsPipelineDesc, GraphicsPipelineHandle, HalError, ImageViewType,
    LoadOp, MemoryLocation, PipelineLayoutDesc, PipelineLayoutHandle, PrimitiveState, QueueHandle,
    SampleType, SamplerAddressMode, SamplerDesc, SamplerHandle, ShaderEntry, ShaderModuleDesc,
    ShaderStages, StoreOp,
};
use crcbl_shaders::{SPRITE, Stage};
use crcbl_sprite::SampleMode;
use glam::Mat4;

use crate::counters::FrameCounters;
use crate::graph::{ImageId, RenderGraph};
use crate::texture::{UploadedTexture, upload_texture};

// ---------------------------------------------------------------------------
// The shader ABI
// ---------------------------------------------------------------------------

/// One sprite, in the byte layout `struct SpriteInstance` in
/// `crates/crcbl-shaders/shaders/sprite.slang` declares.
///
/// Four `[f32; 4]`s: 64 bytes, no padding under any layout rule, and every
/// element of the array 16-byte aligned because 64 is a multiple of 16. A
/// `[f32; 2]` pair here would acquire alignment padding in the shader that this
/// side would have to reproduce exactly, which is a layout that drifts silently.
///
/// Public because it is an ABI, and an ABI with one private spelling is an ABI
/// nobody can check.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteInstance {
    /// World-space rectangle: `[x, y, w, h]` with `x, y` the **minimum** corner.
    /// Y is up.
    pub rect: [f32; 4],
    /// Normalised sheet UVs: `[u0, v0, u1, v1]` with `u0, v0` the **top-left**
    /// corner, in image order.
    pub uv: [f32; 4],
    /// Straight-alpha RGBA multiplied into the sampled texel.
    pub tint: [f32; 4],
    /// The sheet's texel size, its sample mode, and **this sprite's rotation**:
    /// `[width, height, mode, radians]`.
    ///
    /// The first three are [`sheet_lane`]'s packing; the fourth is
    /// [`Sprite::rotation`], stamped on by [`Sprite::instance`] because it is the
    /// one component of this lane that belongs to the sprite rather than to the
    /// sheet.
    ///
    /// **The size and the mode are properties of the sheet, and they travel per
    /// sprite anyway.** The shader cannot get them anywhere else: set 1 binds one
    /// image and one sampler, and a shader has no way to ask a `SamplerState`
    /// which filter it was created with. See `sprite.slang`'s
    /// `SpriteInstance::sheet` for the alternatives that were not taken — a third
    /// descriptor set, or a second pipeline — and why sixteen bytes on a buffer
    /// that is written per frame anyway is the cheaper one.
    ///
    /// **The fourth component was padding.** `sprite.slang` read `sheet.z` for
    /// the mode and handed `sheet.xyz` to the fragment stage, and nothing —
    /// neither the Slang source, nor the compiled WGSL, nor the compiled SPIR-V,
    /// where the only two uses of the loaded lane are a `CompositeExtract … 2`
    /// and a `VectorShuffle … 0 1 2` — ever touched `w`. So the angle cost no
    /// bytes: the instance is still four `float4`s and still
    /// [`INSTANCE_STRIDE`] 64.
    pub sheet: [f32; 4],
}

/// Bytes per instance. Named so the buffer arithmetic and the tests read the
/// same number rather than each spelling 64.
pub const INSTANCE_STRIDE: usize = size_of::<SpriteInstance>();

/// [`SpriteInstance::sheet`]`[2]` for [`SampleMode::Pixel`]: sharp-bilinear
/// sampling and a pixel-snapped quad.
///
/// The shader uses it as a `lerp` weight rather than as a branch, so the two
/// values must be exactly `1.0` and exactly `0.0` and nothing else.
pub const SAMPLE_PIXEL: f32 = 1.0;

/// [`SpriteInstance::sheet`]`[2]` for [`SampleMode::Smooth`]: plain linear
/// sampling of the UV the vertex stage produced, and no snapping.
pub const SAMPLE_SMOOTH: f32 = 0.0;

/// The [`SpriteInstance::sheet`] lane for a sheet of this size and mode.
///
/// A named function rather than a literal inside `register_sheet`, because it is
/// the one place the mode is turned into a number and the shader's `lerp` weight
/// is only a selection while that number is exactly 0 or 1.
///
/// The fourth component is left `0.0`: it is the *sprite's* rotation, not the
/// sheet's, and [`Sprite::instance`] writes it when the instance is built.
#[must_use]
pub fn sheet_lane(width: u32, height: u32, mode: SampleMode) -> [f32; 4] {
    let sample = match mode {
        SampleMode::Pixel => SAMPLE_PIXEL,
        SampleMode::Smooth => SAMPLE_SMOOTH,
    };
    [width as f32, height as f32, sample, 0.0]
}

/// The per-**batch** constant block, matching `struct SpriteConstants` in
/// `sprite.slang`.
///
/// A uniform buffer on every tier — see this module's docs. One block per batch
/// rather than one per frame, addressed by a dynamic offset: only
/// [`base`](Self::base) differs between the blocks of one frame, and
/// [`SpriteRenderer::add_pass`] explains why it cannot be the draw's
/// `firstInstance` instead.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteConstants {
    /// World → clip, column-major, exactly as [`Mat4::to_cols_array`] produces
    /// it. `sprite.slang`'s header records why no transpose is needed.
    pub view_proj: [f32; 16],
    /// The target's size in device pixels.
    ///
    /// Read by the vertex stage, which needs the map from NDC to the pixel grid
    /// to round a [`SampleMode::Pixel`] quad's corners onto it.
    pub viewport: [f32; 2],
    /// **This batch's first sprite, as an index into the frame's instance
    /// buffer.** The vertex stage adds it to `SV_InstanceID`.
    ///
    /// Every draw this pass emits starts at instance 0, so `SV_InstanceID` means
    /// the same thing on SPIR-V and on WGSL — which it does *not* when
    /// `firstInstance` is non-zero. [`SpriteRenderer::add_pass`] and
    /// `sprite.slang`'s header both carry the whole story.
    pub base: u32,
    /// Pads the block to 80 bytes. `std140` rounds a uniform block's size up to
    /// a multiple of 16, and naga *enforces* that where a Vulkan driver would
    /// quietly accept the short binding.
    pub pad: u32,
}

/// Bytes in one constant block: 64 for the matrix, 8 for the viewport, 4 for the
/// base and 4 of padding.
///
/// This is the *binding* size, not the stride between one batch's block and the
/// next — see [`SpriteRenderer::constant_stride`], which rounds it up to the
/// device's dynamic-offset alignment.
pub const CONSTANTS_SIZE: u64 = size_of::<SpriteConstants>() as u64;

// ---------------------------------------------------------------------------
// The caller's vocabulary
// ---------------------------------------------------------------------------

/// A sheet registered with a [`SpriteRenderer`].
///
/// Opaque and per-renderer: an id from one renderer means nothing to another,
/// and [`SpriteRenderer::begin_frame`] rejects one it does not know rather than
/// binding whatever is at that index.
///
/// The field is `pub(crate)` rather than private so that [`crate::nine_slice`]
/// and [`crate::layers`] — which build [`Sprite`]s without a device — can be
/// tested against an id at all. It stays opaque to every caller outside this
/// crate, and [`SpriteRenderer::begin_frame`] still refuses an id it did not
/// hand out, so nothing about the guarantee changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SheetId(pub(crate) u32);

impl SheetId {
    /// The index this id addresses, for tests and diagnostics.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// A sheet image to register: its pixels, its size, and how it is sampled.
///
/// A struct rather than five positional arguments, for the reason
/// [`crate::texture`]'s `UploadArgs` is one: two adjacent `u32`s that can be
/// swapped at the call site are a bug the compiler cannot see.
#[derive(Clone, Copy, Debug)]
pub struct SheetDesc<'a> {
    /// Names the image, its view and its staging buffer, so a capture says which
    /// sheet an upload belonged to.
    pub label: &'a str,
    /// The sheet image's width in texels — `crcbl_sprite::Sheet::width`.
    pub width: u32,
    /// The sheet image's height in texels — `crcbl_sprite::Sheet::height`.
    pub height: u32,
    /// Which sampler this sheet's bind group names.
    pub sample: SampleMode,
    /// Decoded, tightly packed, straight-alpha RGBA8 — exactly
    /// `width * height * 4` bytes.
    pub pixels: &'a [u8],
}

/// One sprite to draw this frame.
///
/// The submission order of a `&[Sprite]` **is** the compositing order. Nothing
/// here sorts, by sheet or by anything else: a caller that interleaves two
/// sheets gets two batches, which is the price of the ordering it asked for.
///
/// Layer and parallax ordering live above this, in [`crate::layers`]: a
/// [`LayerStack`](crate::layers::LayerStack) groups sprites into bands and
/// flattens them back to front when the *caller* asks it to, so the frame this
/// slice receives is still exactly the order it was handed.
///
/// # Building one, and why there is no literal
///
/// [`Sprite::new`] takes the three things a sprite cannot be drawn without — the
/// sheet, the rectangle and the UVs — and the two that mean something specific
/// when left alone arrive through [`with_rotation`](Self::with_rotation) and
/// [`with_tint`](Self::with_tint).
///
/// The type is `#[non_exhaustive]`, so a caller outside this crate cannot write
/// the struct literal or use functional-update syntax. That is deliberate:
/// `rotation` was added to a five-field literal and broke every construction in
/// the workspace, none of which had anything to do with turning, and the next
/// field would break them all again. The fields stay `pub`, so reading and
/// assigning one is unchanged; it is only *construction* that now goes through
/// the constructor, which is what makes the field after this one a non-event.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Sprite {
    /// Which registered sheet this sprite samples.
    pub sheet: SheetId,
    /// World-space rectangle **before rotation**: `[x, y, w, h]`, minimum corner
    /// first, Y up.
    pub rect: [f32; 4],
    /// Rotation in **radians, counter-clockwise, about the centre of
    /// [`rect`](Self::rect)** — `0.0` for a sprite that does not turn.
    ///
    /// # The pivot is the centre, and only the centre
    ///
    /// A ship, a rock and a bullet all turn about themselves, so the centre is
    /// the case every rotating sprite actually wants. An arbitrary pivot is two
    /// more floats on a lane that has one component left, and it is not needed to
    /// express one: rotating a rectangle about an outside point gives the same
    /// rectangle turned about its own centre and then translated, so a caller
    /// that wants an orbit computes the translated `rect` and gets it exactly.
    /// **A pivot offset is deliberately not half-built here** — see
    /// `docs/backlog.md`.
    ///
    /// # What it does to [`SampleMode`]
    ///
    /// Nothing, where it is `0.0`: `sprite.slang` branches on the angle and the
    /// zero case is the arithmetic that was there before rotation existed, not a
    /// general formula that happens to reduce to it, so a `Smooth` or an
    /// unrotated `Pixel` sprite is bit-for-bit what it always was.
    ///
    /// Where it is not zero, [`SampleMode::Pixel`]'s snap stops rounding each
    /// corner — a rotated quad has no axis-aligned rectangle to round onto, and
    /// rounding its corners independently shears it — and instead translates the
    /// whole quad rigidly so its *centre* lands on the pixel grid. Sharp-bilinear
    /// itself needs no change at all; `sprite.slang`'s `sharpen` records why.
    pub rotation: f32,
    /// Normalised sheet UVs: `[u0, v0, u1, v1]`, top-left corner first.
    pub uv: [f32; 4],
    /// Straight-alpha RGBA multiplied into the sampled texel. `[1.0; 4]` is
    /// "the sheet as authored".
    pub tint: [f32; 4],
}

impl Sprite {
    /// A sprite that draws `uv` of `sheet` into `rect`, unrotated and untinted.
    ///
    /// [`rotation`](Self::rotation) starts at `0.0` and [`tint`](Self::tint) at
    /// `[1.0; 4]` — both the value that means "do nothing", so a sprite built
    /// here and drawn is the sheet as authored, at the rectangle asked for.
    ///
    /// # Two `[f32; 4]`s the compiler cannot tell apart
    ///
    /// `rect` is **world units**, minimum corner first and Y up; `uv` is
    /// **normalised sheet coordinates**, top-left corner first. Swapping them at
    /// a call site compiles and draws a wrong picture — the hazard
    /// [`SheetDesc`] became a struct to avoid. Nothing here can catch it. What
    /// does is the golden images under `crates/crcbl-vk/tests/golden/` and
    /// `crates/crcbl/tests/golden/`, and the sample suites that assert on a
    /// resolved sprite's `rect` and `uv` — so a sprite call site is not a place
    /// to take the argument order on trust.
    #[must_use]
    pub const fn new(sheet: SheetId, rect: [f32; 4], uv: [f32; 4]) -> Self {
        Self {
            sheet,
            rect,
            rotation: 0.0,
            uv,
            tint: [1.0; 4],
        }
    }

    /// The same sprite, turned by `radians` — [`rotation`](Self::rotation)'s
    /// units, pivot and effect on [`SampleMode`] in full.
    #[must_use]
    pub const fn with_rotation(mut self, radians: f32) -> Self {
        self.rotation = radians;
        self
    }

    /// The same sprite, with [`tint`](Self::tint) multiplied into every texel it
    /// samples.
    #[must_use]
    pub const fn with_tint(mut self, tint: [f32; 4]) -> Self {
        self.tint = tint;
        self
    }

    /// The bytes the shader reads for this sprite.
    ///
    /// `sheet` is the [`sheet_lane`] of the sheet this sprite names, which the
    /// [`Sprite`] itself does not carry — a caller writes a [`SheetId`] and the
    /// renderer holds the size and the mode it was registered with, so there is
    /// exactly one place either can be wrong.
    ///
    /// The lane's fourth component is *not* the one `sheet_lane` produced: it is
    /// this sprite's [`rotation`](Self::rotation), because the angle is the one
    /// thing in that `float4` that belongs to the sprite. A zero rotation writes
    /// the `0.0` that was already there, so the bytes are unchanged.
    #[must_use]
    pub const fn instance(&self, sheet: [f32; 4]) -> SpriteInstance {
        SpriteInstance {
            rect: self.rect,
            uv: self.uv,
            tint: self.tint,
            sheet: [sheet[0], sheet[1], sheet[2], self.rotation],
        }
    }
}

// ---------------------------------------------------------------------------
// The renderer
// ---------------------------------------------------------------------------

/// How many frames of instance and constant buffers to keep in flight.
const FRAMES_IN_FLIGHT: usize = 2;

/// Starting size of the instance ring, in bytes. Sixteen sprites; a frame
/// that wants more grows it once.
const INITIAL_RING_BYTES: u64 = 1024;

/// How many per-batch constant blocks the ring starts with.
///
/// Eight, because a batch is a *sheet change* and not a sprite: both shipped
/// samples draw four, so eight is a frame's worth of headroom for one 2 KiB
/// allocation on Tier B and 1 KiB on Tier A. A frame with more grows it once.
const INITIAL_BATCH_BLOCKS: u64 = 8;

/// Set 0: the frame. Constants at binding 0, instances at binding 1.
const FRAME_SET: u32 = 0;

/// Set 1: the sheet. Image view at binding 0, sampler at binding 1.
const SHEET_SET: u32 = 1;

/// Vertices in the quad the vertex stage expands from `SV_VertexID`.
///
/// Six, not four plus an index buffer — `sprite.slang`'s `CORNERS` records the
/// trade.
const QUAD_VERTICES: u32 = 6;

/// Triangles one instance covers: [`QUAD_VERTICES`] of a triangle list.
///
/// A constant rather than a division at the one place it is used, because it is
/// the fact [`SpriteRenderer::counters`] reports and the pipeline's
/// `PrimitiveState::default()` — a triangle list — is what makes it true.
const TRIANGLES_PER_QUAD: u64 = QUAD_VERTICES as u64 / 3;

/// The size the instance ring grows to when `needed` bytes no longer fit.
///
/// Doubling rather than fitting exactly, so a scene that gains a sprite a frame
/// reallocates a handful of times rather than every frame.
fn grown(needed: u64) -> u64 {
    needed
        .max(INITIAL_RING_BYTES)
        .next_power_of_two()
        .next_multiple_of(256)
}

/// One consecutive run of sprites sharing a sheet: exactly one draw call.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Batch {
    sheet: SheetId,
    /// Where this batch's sprites are in the **frame's** instance buffer.
    ///
    /// Deliberately *not* what `draw` is handed — see
    /// [`SpriteRenderer::add_pass`]. The draw covers `0..count()`, and
    /// `instances.start` reaches the shader as [`SpriteConstants::base`].
    instances: Range<u32>,
    /// Byte offset of this batch's constant block, and the dynamic offset set 0
    /// is bound with. `n * SpriteRenderer::constant_stride()`.
    constant_offset: u32,
}

impl Batch {
    /// How many instances this batch draws.
    const fn count(&self) -> u32 {
        self.instances.end - self.instances.start
    }
}

/// A registered sheet: the uploaded image, the bind group naming it, and the
/// instance lane every sprite drawn from it carries.
#[derive(Clone, Copy, Debug)]
struct RegisteredSheet {
    texture: UploadedTexture,
    group: BindGroupHandle,
    /// [`sheet_lane`] for this sheet's size and [`SampleMode`], computed once at
    /// registration and copied into every instance that names it.
    lane: [f32; 4],
}

/// The sprite renderer.
///
/// Created once, sheets registered once, then `begin_frame` + `add_pass` per
/// frame.
#[derive(Debug)]
pub struct SpriteRenderer {
    // Pipeline state
    pipeline_layout: PipelineLayoutHandle,
    pipeline: GraphicsPipelineHandle,
    frame_layout: BindGroupLayoutHandle,
    sheet_layout: BindGroupLayoutHandle,

    /// **One sampler, linear, for every sheet whatever its [`SampleMode`].**
    ///
    /// Slice 3 created a nearest one beside it and gave it to
    /// [`SampleMode::Pixel`]; sharp-bilinear *is* linear sampling of a bent UV,
    /// so a nearest sampler would flatten the bend and undo the whole thing. The
    /// mode now travels on the instance instead — see [`SpriteInstance::sheet`].
    sampler: SamplerHandle,

    /// Registered sheets, indexed by [`SheetId::index`].
    sheets: Vec<RegisteredSheet>,
    /// The queue every sheet upload is submitted to.
    ///
    /// Taken once at construction rather than per `register_sheet`, so two
    /// sheets cannot be uploaded on two different queues and then be read by one
    /// pass — which the graph's barriers would not describe.
    queue: QueueHandle,

    // Per frame in flight.
    frame_groups: Vec<BindGroupHandle>,
    instance_buffers: Vec<BufferHandle>,
    constant_buffers: Vec<BufferHandle>,
    /// How many **bytes** each instance buffer holds. Compared against the bytes
    /// a frame needs — never against a sprite *count*, which is the comparison
    /// that made [`crate::ui_pass`] destroy and recreate both its rings and its
    /// bind group on every steady-state frame.
    instance_capacity: Vec<u64>,
    /// How many **bytes** each constant buffer holds, on the same terms. The
    /// constants are one [`constant_stride`](Self::constant_stride)-sized block
    /// per batch now, so this grows with the number of batches a frame draws.
    constant_capacity: Vec<u64>,
    /// Bytes between one batch's constant block and the next.
    ///
    /// [`CONSTANTS_SIZE`] rounded up to the device's
    /// `min_uniform_buffer_offset_alignment`, because a dynamic offset must be a
    /// multiple of it — 256 on WebGPU, 64 on a typical desktop Vulkan driver.
    /// Read from the device at construction rather than assumed: a hard-coded
    /// 256 wastes memory on Tier A and a hard-coded 64 is a validation error on
    /// Tier B.
    ///
    /// `u32` because that is what `bind_group` takes a dynamic offset as, so the
    /// conversion happens once here rather than at every bind.
    constant_stride: u32,
    /// This frame's draw calls, in submission order.
    batches: Vec<Vec<Batch>>,
    frame: usize,

    /// The format the pipeline was built for. Dynamic rendering checks the
    /// pipeline's colour-target format against the attachment at pass-begin, so
    /// a swapchain in a different format needs a different pipeline.
    target_format: Format,

    destroyed: bool,
}

impl SpriteRenderer {
    /// Creates the sprite pipeline, both samplers, and the per-frame buffers.
    ///
    /// `target_format` must be the format of the image the pass composites onto
    /// — normally the swapchain's. As [`crate::ui_pass::UiRenderer::new`], a
    /// pipeline built for the wrong one fails the frame at pass-begin rather
    /// than this constructor.
    ///
    /// No sheets are registered; a renderer with none draws nothing and is not
    /// an error.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. A failure part-way through releases
    /// everything already created, so a caller that retries or exits leaves
    /// nothing behind.
    pub fn new(
        device: &dyn Device,
        queue: QueueHandle,
        target_format: Format,
    ) -> Result<Self, HalError> {
        let mut rollback = Rollback::default();
        match Self::build(device, queue, target_format, &mut rollback) {
            Ok(renderer) => Ok(renderer),
            Err(error) => {
                rollback.run(device);
                Err(error)
            }
        }
    }

    /// The body of [`SpriteRenderer::new`], recording what it has created into
    /// `rollback` as it goes.
    fn build(
        device: &dyn Device,
        queue: QueueHandle,
        target_format: Format,
        rollback: &mut Rollback,
    ) -> Result<Self, HalError> {
        let sampler = device.create_sampler(&SamplerDesc {
            label: Some("sprite linear"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mip_filter: FilterMode::Nearest,
            address_mode: [SamplerAddressMode::ClampToEdge; 3],
            ..SamplerDesc::default()
        })?;
        rollback.samplers.push(sampler);

        let frame_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("sprite frame"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX,
                    // `dynamic: true`, unlike `ui_pass`'s equivalent binding.
                    // This is the one thing that lets a batch say where its
                    // instances start without the draw's `firstInstance` — which
                    // `add_pass` explains cannot be used, because the two
                    // shading languages disagree about what it does to
                    // `SV_InstanceID`. One buffer, one block per batch, one
                    // offset at bind time.
                    kind: BindingKind::UniformBuffer { dynamic: true },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX,
                    kind: BindingKind::StorageBuffer {
                        read_only: true,
                        dynamic: false,
                    },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
            ],
        })?;
        rollback.bind_group_layouts.push(frame_layout);

        let sheet_layout = device.create_bind_group_layout(&BindGroupLayoutDesc {
            label: Some("sprite sheet"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    kind: BindingKind::SampledImage {
                        view_type: ImageViewType::D2,
                        sample_type: SampleType::Float,
                    },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    kind: BindingKind::Sampler { comparison: false },
                    count: 1,
                    flags: BindingFlags::empty(),
                },
            ],
        })?;
        rollback.bind_group_layouts.push(sheet_layout);

        // A dynamic offset must be a multiple of the device's alignment, and the
        // block has to fit inside one stride. `u32` because that is what a
        // dynamic offset is; an alignment that did not fit would be a device
        // nobody can bind a dynamic uniform on at all.
        let alignment = device.caps().limits.min_uniform_buffer_offset_alignment;
        let constant_stride =
            u32::try_from(CONSTANTS_SIZE.next_multiple_of(alignment)).map_err(|_| {
                HalError::InvalidDescriptor(format!(
                    "min_uniform_buffer_offset_alignment is {alignment}, which no dynamic \
                     offset can express"
                ))
            })?;
        let initial_constant_bytes = u64::from(constant_stride) * INITIAL_BATCH_BLOCKS;

        let mut frame_groups = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut instance_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut constant_buffers = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut instance_capacity = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut constant_capacity = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut batches = Vec::with_capacity(FRAMES_IN_FLIGHT);
        for _ in 0..FRAMES_IN_FLIGHT {
            let instances = device.create_buffer(&BufferDesc {
                label: Some("sprite instances"),
                size: INITIAL_RING_BYTES,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(instances);
            let constants = device.create_buffer(&BufferDesc {
                label: Some("sprite constants"),
                size: initial_constant_bytes,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            rollback.buffers.push(constants);
            let group = device.create_bind_group(&BindGroupDesc {
                label: Some("sprite frame"),
                layout: frame_layout,
                entries: &frame_entries(constants, instances),
                variable_count: None,
            })?;
            rollback.bind_groups.push(group);

            instance_buffers.push(instances);
            constant_buffers.push(constants);
            frame_groups.push(group);
            instance_capacity.push(INITIAL_RING_BYTES);
            constant_capacity.push(initial_constant_bytes);
            batches.push(Vec::new());
        }

        let set_layouts = [frame_layout, sheet_layout];
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDesc {
            label: Some("sprite"),
            bind_group_layouts: &set_layouts,
            // None on every tier. That is the whole of "one shader, no tier
            // split" on this side of the seam.
            push_constants: None,
        })?;
        rollback.pipeline_layouts.push(pipeline_layout);

        // Entry points resolved before the module exists, as in `ui_pass`: a
        // manifest that disagreed with the artifact would otherwise fail inside
        // the descriptor literal with the module created and nothing holding it.
        let vertex_entry = entry(Stage::Vertex)?;
        let fragment_entry = entry(Stage::Fragment)?;
        let module = device.create_shader_module(&ShaderModuleDesc {
            label: Some("sprite.slang"),
            spirv: SPRITE.spirv(),
            wgsl: SPRITE.wgsl(),
            msl: SPRITE.msl(),
            // One container per entry point, both of them, in one module; see
            // `crcbl_render::forward`.
            dxil: &SPRITE.dxil_containers(),
        })?;
        let targets = [color_target(target_format)];
        let created = device.create_graphics_pipeline(&GraphicsPipelineDesc {
            label: Some("sprite"),
            layout: pipeline_layout,
            vertex: ShaderEntry {
                module,
                entry_point: vertex_entry,
            },
            fragment: Some(ShaderEntry {
                module,
                entry_point: fragment_entry,
            }),
            primitive: PrimitiveState::default(), // TriangleList, no culling
            depth_stencil: SPRITE_DEPTH_STENCIL,
            multisample: Default::default(),
            color_targets: &targets,
        });
        device.destroy_shader_module(module);
        let pipeline = created?;
        rollback.pipelines.push(pipeline);

        Ok(Self {
            pipeline_layout,
            pipeline,
            frame_layout,
            sheet_layout,
            sampler,
            sheets: Vec::new(),
            queue,
            frame_groups,
            instance_buffers,
            constant_buffers,
            instance_capacity,
            constant_capacity,
            constant_stride,
            batches,
            frame: 0,
            target_format,
            destroyed: false,
        })
    }

    /// The format the sprite pass composites into.
    #[must_use]
    pub const fn target_format(&self) -> Format {
        self.target_format
    }

    /// Bytes between one batch's constant block and the next, and therefore the
    /// dynamic offset [`add_pass`](Self::add_pass) binds set 0 with for batch
    /// `n`: `n * constant_stride()`.
    ///
    /// [`CONSTANTS_SIZE`] rounded up to the device's
    /// `min_uniform_buffer_offset_alignment`. Public so a test can predict the
    /// offsets rather than restate a number the device chose.
    #[must_use]
    pub const fn constant_stride(&self) -> u32 {
        self.constant_stride
    }

    /// How many sheets are registered.
    #[must_use]
    pub fn sheet_count(&self) -> usize {
        self.sheets.len()
    }

    /// What the last [`begin_frame`](Self::begin_frame) left this pass to draw.
    ///
    /// **Read off the batches themselves**, which is the same list
    /// [`add_pass`](Self::add_pass) walks — so a draw this reports is a draw the
    /// frame records, and a batching change moves both together. That is the
    /// point of it existing: `docs/plan/40-profiling.md`'s complaint is that a
    /// consumer keeping its own copy of a count is a copy that drifts, and this
    /// is the pass's own answer.
    ///
    /// Every counter is known here. Nothing on this path draws indirectly and
    /// nothing culls, so [`FrameCounters::drawn`] equals
    /// [`FrameCounters::instances`] and the triangles are exact:
    /// [`add_pass`](Self::add_pass) expands the same triangle-list quad per
    /// instance, so the count is that quad's own and not an estimate.
    ///
    /// A frame with no sprites reports nothing recorded, because
    /// [`add_pass`](Self::add_pass) adds no pass at all.
    #[must_use]
    pub fn counters(&self) -> FrameCounters {
        let batches = &self.batches[self.frame];
        let instances = u64::from(batches.iter().map(Batch::count).sum::<u32>());
        FrameCounters {
            draws: batches.len() as u64,
            instances,
            drawn: Some(instances),
            triangles: Some(instances * TRIANGLES_PER_QUAD),
            // A quad is not cluster geometry, so this pass drew no clusters and
            // says so as a number. `None` would be "nothing counted", which
            // would make the panel's cluster row unknown on every sprite frame.
            clusters: Some(0),
            // Nothing here came off the GPU: these are this frame's own counts,
            // on the one lag the whole section shares.
            cull_frame: None,
        }
    }

    /// Uploads a sheet image and returns the id a [`Sprite`] names it by.
    ///
    /// The upload is a staging copy that blocks on `wait_idle` — a **startup**
    /// path, like [`crate::ui_pass`]'s glyph atlas and [`crate::forward`]'s
    /// cube, and like both it must not be called between `begin_frame` and the
    /// graph's `execute`.
    ///
    /// The image is created as [`Format::Rgba8UnormSrgb`], so the sampler
    /// decodes to linear and the blend below happens in linear light. Sprite art
    /// comes out of a PNG sRGB-encoded, and the target this composites onto is
    /// an sRGB swapchain; taking the decode off the sampler would mean blending
    /// encoded values, which darkens every half-transparent edge.
    ///
    /// # Errors
    ///
    /// [`HalError`] from the upload or from bind-group creation. `pixels` must
    /// be exactly `width * height * 4` bytes; [`upload_texture`] says so by name
    /// when it is not. A failure leaves nothing behind and no id allocated.
    pub fn register_sheet(
        &mut self,
        device: &dyn Device,
        desc: &SheetDesc<'_>,
    ) -> Result<SheetId, HalError> {
        let texture = upload_texture(
            device,
            self.queue,
            desc.label,
            Format::Rgba8UnormSrgb,
            desc.width,
            desc.height,
            desc.pixels,
        )?;
        let group = match device.create_bind_group(&BindGroupDesc {
            label: Some(desc.label),
            layout: self.sheet_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    array_index: 0,
                    resource: BindingResource::ImageView(texture.view),
                },
                BindGroupEntry {
                    binding: 1,
                    array_index: 0,
                    resource: BindingResource::Sampler(self.sampler),
                },
            ],
            variable_count: None,
        }) {
            Ok(group) => group,
            Err(error) => {
                // The image exists by now, and nothing else will ever hold it.
                texture.destroy(device);
                return Err(error);
            }
        };

        let id = SheetId(u32::try_from(self.sheets.len()).map_err(|_| {
            HalError::InvalidDescriptor("more than u32::MAX sprite sheets".to_string())
        })?);
        self.sheets.push(RegisteredSheet {
            texture,
            group,
            lane: sheet_lane(desc.width, desc.height, desc.sample),
        });
        Ok(id)
    }

    /// Uploads this frame's sprites and constants, and advances the ring.
    ///
    /// `view_projection` is world → clip, normally
    /// [`Camera::view_projection`](crate::Camera::view_projection) with an
    /// [`Orthographic`](crate::Projection::Orthographic) projection. `extent` is
    /// the target's size in pixels.
    ///
    /// **`extent` is taken here and not in [`add_pass`](Self::add_pass)**, which
    /// is the one place this pass departs from [`crate::ui_pass`]'s shape.
    /// `ui_pass` takes it at `add_pass` because that is its *only* source of the
    /// constants; here `view_projection` is already computed from an aspect
    /// ratio, and an extent taken a second time somewhere else is a second thing
    /// that can disagree with the matrix it has to match. The graph sets the
    /// viewport and the scissor from the pass's own render area either way, so
    /// `add_pass` has no use for it.
    ///
    /// Sprites are batched into draws here, not at `add_pass`: consecutive
    /// sprites sharing a sheet become one draw, and **nothing is sorted**.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] if any sprite names a sheet this renderer
    /// did not register — refused by name rather than binding whatever group is
    /// at that index. [`HalError`] from any buffer call otherwise.
    pub fn begin_frame(
        &mut self,
        device: &dyn Device,
        sprites: &[Sprite],
        view_projection: Mat4,
        extent: (u32, u32),
    ) -> Result<(), HalError> {
        // Checked before anything is written, so a bad sheet id leaves the
        // previous frame's buffers untouched rather than half-overwritten.
        for (index, sprite) in sprites.iter().enumerate() {
            if sprite.sheet.index() as usize >= self.sheets.len() {
                return Err(HalError::InvalidDescriptor(format!(
                    "sprite {index} names sheet {}, but only {} are registered with this renderer",
                    sprite.sheet.index(),
                    self.sheets.len()
                )));
            }
        }

        self.frame = (self.frame + 1) % FRAMES_IN_FLIGHT;
        let idx = self.frame;

        // The sheet's size and mode are stamped onto the instance here, from the
        // sheet the sprite named — the loop above has already established that
        // every one of those indices is registered.
        let instances: Vec<SpriteInstance> = sprites
            .iter()
            .map(|sprite| sprite.instance(self.sheets[sprite.sheet.index() as usize].lane))
            .collect();

        // Grow only when this frame genuinely needs more room. Both sides are
        // **bytes**.
        let needed = (instances.len() * INSTANCE_STRIDE) as u64;
        let mut buffer_replaced = false;
        if needed > self.instance_capacity[idx] {
            let size = grown(needed);
            // Create before destroying: a creation that fails must not leave a
            // destroyed handle in the struct for `destroy` to hand back twice.
            let fresh = device.create_buffer(&BufferDesc {
                label: Some("sprite instances"),
                size,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })?;
            device.destroy_buffer(core::mem::replace(&mut self.instance_buffers[idx], fresh));
            self.instance_capacity[idx] = size;
            buffer_replaced = true;
        }

        if !instances.is_empty() {
            let bytes: &[u8] = bytemuck::cast_slice(&instances);
            device.write_buffer(self.instance_buffers[idx], 0, bytes)?;
        }

        // Batched here rather than at `add_pass`, and now before the constants
        // are written rather than after: there is one constant block per batch,
        // so the batching has to have happened before there is anything to write.
        let batches = batch(sprites, self.constant_stride);
        let blocks = constant_blocks(view_projection, extent, &batches, self.constant_stride);
        if blocks.len() as u64 > self.constant_capacity[idx] {
            let size = grown_constants(blocks.len() as u64, self.constant_stride);
            let fresh = device.create_buffer(&BufferDesc {
                label: Some("sprite constants"),
                size,
                usage: BufferUsage::UNIFORM,
                memory: MemoryLocation::HostUpload,
            })?;
            device.destroy_buffer(core::mem::replace(&mut self.constant_buffers[idx], fresh));
            self.constant_capacity[idx] = size;
            buffer_replaced = true;
        }
        device.write_buffer(self.constant_buffers[idx], 0, &blocks)?;

        // A frame that grew *either* buffer needs the group rewritten; a
        // steady-state frame writes no descriptors, which is still true now that
        // the constants can move too — a scene draws the same batch count frame
        // after frame.
        if buffer_replaced {
            let fresh = device.create_bind_group(&BindGroupDesc {
                label: Some("sprite frame"),
                layout: self.frame_layout,
                entries: &frame_entries(self.constant_buffers[idx], self.instance_buffers[idx]),
                variable_count: None,
            })?;
            device.destroy_bind_group(core::mem::replace(&mut self.frame_groups[idx], fresh));
        }

        self.batches[idx] = batches;
        Ok(())
    }

    /// The most passes [`add_pass`](Self::add_pass) adds to a frame.
    ///
    /// The most rather than the count: a frame with no sprites adds none, as the
    /// method's docs say. What a caller sizing
    /// [`PassTimers`](crate::timing::PassTimers) adds up — see
    /// [`MAX_TIMED_PASSES`](crate::timing::MAX_TIMED_PASSES).
    pub const MAX_PASSES: u32 = 1;

    /// Adds the sprite pass to `graph`, drawing on top of `target`.
    ///
    /// One `draw(0..6, 0..count)` per batch, in submission order, with **both**
    /// sets rebound between batches: set 1 for the sheet, and set 0 at the
    /// dynamic offset of that batch's constant block. A frame with no sprites
    /// adds **no pass at all** rather than an empty one: a render pass that draws
    /// nothing still costs a load and a store of the whole target.
    ///
    /// # Every draw starts at instance 0, and that is load-bearing
    ///
    /// The obvious spelling is `draw(0..6, batch.instances)` — point the draw at
    /// its slice of the frame's instance buffer with `firstInstance` and let the
    /// shader index the buffer absolutely. That is what this pass did, and it
    /// drew **every batch after the first from the first batch's instances**: a
    /// four-sheet frame put one rectangle on screen in the last sheet's colours
    /// and left the other three empty. Both shipped samples registered four
    /// sheets, so both were rendering wrong.
    ///
    /// The reason is that `SV_InstanceID` is not the same number on the two
    /// targets `sprite.slang` is compiled for. `slangc` lowers it to
    /// `InstanceIndex - BaseInstance` on SPIR-V, which is HLSL's definition —
    /// relative to the draw — and straight to `@builtin(instance_index)` on
    /// WGSL, which WebGPU defines to *start at* `firstInstance`. No shader source
    /// is correct on both while `firstInstance` is non-zero.
    ///
    /// So it is always zero, the two lowerings agree because with a zero base
    /// they are the same number, and the batch's offset into the buffer travels
    /// in [`SpriteConstants::base`] instead. The cost is one extra `bind_group`
    /// per batch, against a bind this pass was already doing for the sheet.
    ///
    /// `extent` is deliberately absent — see [`begin_frame`](Self::begin_frame).
    pub fn add_pass<'a>(&'a self, graph: &mut RenderGraph<'a>, target: ImageId) {
        let batches = &self.batches[self.frame];
        if batches.is_empty() {
            return;
        }

        let pipeline = self.pipeline;
        let pipeline_layout = self.pipeline_layout;
        let frame_group = self.frame_groups[self.frame];
        let sheets = &self.sheets;

        graph
            .add_render_pass("sprites")
            .color(target, LoadOp::Load, StoreOp::Store, Default::default())
            .execute(move |ctx| {
                let encoder = ctx.encoder();
                encoder.bind_graphics_pipeline(pipeline);
                for batch in batches {
                    // The block `begin_frame` wrote for this batch, and the only
                    // thing that tells the shader where the batch's instances
                    // start.
                    encoder.bind_group(
                        FRAME_SET,
                        frame_group,
                        &[batch.constant_offset],
                        pipeline_layout,
                    );
                    // Indexing is safe by construction: `begin_frame` refuses a
                    // sprite naming an unregistered sheet, and sheets are never
                    // removed.
                    let group = sheets[batch.sheet.index() as usize].group;
                    encoder.bind_group(SHEET_SET, group, &[], pipeline_layout);
                    encoder.draw(0..QUAD_VERTICES, 0..batch.count());
                }
            });
    }

    /// Destroys all GPU resources, including every registered sheet.
    ///
    /// The device must be idle.
    pub fn destroy(mut self, device: &dyn Device) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        for sheet in core::mem::take(&mut self.sheets) {
            device.destroy_bind_group(sheet.group);
            sheet.texture.destroy(device);
        }
        for group in self.frame_groups.drain(..) {
            device.destroy_bind_group(group);
        }
        for buffer in self.instance_buffers.drain(..) {
            device.destroy_buffer(buffer);
        }
        for buffer in self.constant_buffers.drain(..) {
            device.destroy_buffer(buffer);
        }
        device.destroy_sampler(self.sampler);
        device.destroy_graphics_pipeline(self.pipeline);
        device.destroy_pipeline_layout(self.pipeline_layout);
        device.destroy_bind_group_layout(self.frame_layout);
        device.destroy_bind_group_layout(self.sheet_layout);
    }
}

impl Drop for SpriteRenderer {
    fn drop(&mut self) {
        if !self.destroyed {
            log::warn!("SpriteRenderer dropped without calling destroy() — GPU resources leaked");
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The pipeline's colour target: straight alpha, all channels.
///
/// A named function rather than a literal inside `build` because the recorder
/// cannot see a pipeline descriptor — [`crcbl_hal::null::Event::Created`] keeps
/// a kind and a label and nothing else — so this is the only thing a test can
/// assert the pass's blend state against. It is the *same* value `build` hands
/// to `create_graphics_pipeline`, which is what makes the assertion worth
/// anything.
const fn color_target(format: Format) -> ColorTargetState {
    ColorTargetState {
        format,
        // `src * a + dst * (1 - a)`, spelled once in `crcbl_hal`.
        blend: Some(BlendState::alpha()),
        write_mask: ColorWrites::ALL,
    }
}

/// The pipeline's depth state: **none**.
///
/// No test and no write. Sprites composite in submission order, which is a
/// property of the draw order and of alpha blending being order-dependent; a
/// depth test would discard the half-transparent fragments that make a sprite a
/// sprite. Named for the same reason [`color_target`] is.
const SPRITE_DEPTH_STENCIL: Option<DepthStencilState> = None;

/// One frame's bind-group entries: constants at 0, instances at 1.
///
/// One function rather than the two copies `build` and `begin_frame` would
/// otherwise carry — they build the *same* group, and a binding added to one and
/// forgotten in the other is a group that stops matching its layout the first
/// time the ring grows.
const fn frame_entries(constants: BufferHandle, instances: BufferHandle) -> [BindGroupEntry; 2] {
    [
        BindGroupEntry {
            binding: 0,
            array_index: 0,
            // **One block, not the whole buffer.** The binding is dynamic, so
            // the offset the bind adds is on top of this one, and both Vulkan
            // and WebGPU require `offset + dynamic + size` to stay inside the
            // buffer. Bound whole, the very first non-zero dynamic offset would
            // be out of range — which is a validation error rather than a wrong
            // picture, but only on a machine running the layer.
            resource: BindingResource::Buffer {
                buffer: constants,
                offset: 0,
                size: CONSTANTS_SIZE,
            },
        },
        BindGroupEntry {
            binding: 1,
            array_index: 0,
            resource: BindingResource::whole_buffer(instances),
        },
    ]
}

/// How many draw calls `sprites` will cost this pass.
///
/// The batching rule without the batches: a run of consecutive sprites naming
/// one sheet is one draw, so `A A B A` is **three** and a distinct-sheet count
/// would say two. Pure, so a caller can ask before there is a device — which is
/// the point, because the alternative is what `apps/horde` used to do: restate
/// the rule in its own module and count with that. A second copy of a rule is
/// where the number stays right while the picture goes wrong.
///
/// It delegates to the pass's own batcher rather than walking the slice again,
/// so there is one implementation and this cannot drift from it. The
/// `Vec` it allocates is why this is a diagnostic and a test's question rather
/// than something to call every frame; the pass itself uses `batch` directly.
#[must_use]
pub fn batch_count(sprites: &[Sprite]) -> usize {
    // Stride zero: it only sets each batch's constant offset, which no count
    // depends on.
    batch(sprites, 0).len()
}

/// Splits `sprites` into runs of consecutive sprites sharing a sheet, giving
/// each run the byte offset of its constant block.
///
/// **Consecutive, not grouped.** `A A B A` is three batches, not two: the
/// submission order is the caller's compositing order, and reordering it behind
/// their back to save a bind is how a sprite ends up behind the thing it was
/// drawn after. Pure and free of a device, so the batching rule is testable
/// without one.
fn batch(sprites: &[Sprite], stride: u32) -> Vec<Batch> {
    let mut batches: Vec<Batch> = Vec::new();
    for (index, sprite) in sprites.iter().enumerate() {
        let index = index as u32;
        match batches.last_mut() {
            Some(last) if last.sheet == sprite.sheet => last.instances.end = index + 1,
            _ => batches.push(Batch {
                sheet: sprite.sheet,
                instances: index..index + 1,
                // Saturating rather than wrapping: an offset that wrapped would
                // bind a *valid-looking* block belonging to some other batch,
                // which is the failure mode this whole arrangement exists to
                // remove. A saturated one is out of the buffer and the layer
                // says so. Reaching it needs about 17 million batches.
                constant_offset: (batches.len() as u32).saturating_mul(stride),
            }),
        }
    }
    batches
}

/// The bytes of one frame's constant buffer: one [`SpriteConstants`] per batch,
/// laid out at `stride`.
///
/// Pure, so the one thing a recorder cannot see — that batch `n`'s block
/// actually carries batch `n`'s first instance — is assertable without a device.
/// **A frame with no batches still gets one block**, so a frame that draws
/// nothing writes the same number of buffers as one that draws something and the
/// buffer is never zero-sized.
fn constant_blocks(
    view_projection: Mat4,
    extent: (u32, u32),
    batches: &[Batch],
    stride: u32,
) -> Vec<u8> {
    let stride = stride as usize;
    let blocks = batches.len().max(1);
    let mut bytes = vec![0u8; blocks * stride];
    let view_proj = view_projection.to_cols_array();
    let viewport = [extent.0 as f32, extent.1 as f32];
    for index in 0..blocks {
        let constants = SpriteConstants {
            view_proj,
            viewport,
            // The empty-frame block draws nothing, so its base is 0 by the same
            // arithmetic rather than by a special case.
            base: batches.get(index).map_or(0, |batch| batch.instances.start),
            pad: 0,
        };
        bytes[index * stride..index * stride + CONSTANTS_SIZE as usize]
            .copy_from_slice(bytemuck::bytes_of(&constants));
    }
    bytes
}

/// The size the constant ring grows to when `needed` bytes no longer fit.
///
/// Doubling in **blocks** rather than in bytes, so the result stays a whole
/// number of `stride`-sized blocks and the offset arithmetic above never lands
/// past the end.
fn grown_constants(needed: u64, stride: u32) -> u64 {
    let stride = u64::from(stride);
    needed.div_ceil(stride).next_power_of_two() * stride
}

/// `sprite.slang`'s entry point for `stage`.
fn entry(stage: Stage) -> Result<&'static str, HalError> {
    SPRITE.entry_point(stage).ok_or_else(|| {
        HalError::ShaderCompilation(format!(
            "sprite.slang exposes no unambiguous {stage:?} entry point"
        ))
    })
}

/// What a partly-built [`SpriteRenderer`] has to give back.
///
/// `build` creates a dozen objects with `?` between them and the seam's
/// `destroy_*` is explicit, so a failure half way through would otherwise leak
/// everything created before it. [`crate::ui_pass`] and [`crate::forward`] carry
/// the same shape for the same reason.
#[derive(Default)]
struct Rollback {
    buffers: Vec<BufferHandle>,
    bind_groups: Vec<BindGroupHandle>,
    bind_group_layouts: Vec<BindGroupLayoutHandle>,
    pipeline_layouts: Vec<PipelineLayoutHandle>,
    pipelines: Vec<GraphicsPipelineHandle>,
    samplers: Vec<SamplerHandle>,
}

impl Rollback {
    /// Releases everything, in the same dependency order as
    /// [`SpriteRenderer::destroy`].
    fn run(self, device: &dyn Device) {
        for handle in self.bind_groups {
            device.destroy_bind_group(handle);
        }
        for handle in self.buffers {
            device.destroy_buffer(handle);
        }
        for handle in self.samplers {
            device.destroy_sampler(handle);
        }
        for handle in self.pipelines {
            device.destroy_graphics_pipeline(handle);
        }
        for handle in self.pipeline_layouts {
            device.destroy_pipeline_layout(handle);
        }
        for handle in self.bind_group_layouts {
            device.destroy_bind_group_layout(handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::{Command, Event, NullInstance, Recorder};
    use crcbl_hal::{
        CommandEncoderDesc, DeviceCaps, DeviceDesc, Extent3d, Features, ImageDesc,
        ImageSubresourceRange, ImageType, ImageUsage, ImageViewDesc, ImageViewType, Instance,
        Limits, QueueKind, ResourceState,
    };

    use crate::graph::ImportedImage;
    use crate::transient::TransientPool;

    const TARGET: Format = Format::Bgra8UnormSrgb;
    const EXTENT: (u32, u32) = (256, 192);

    fn open(recorder: &Recorder) -> (Box<dyn Device>, QueueHandle) {
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("sprite pass"),
                adapter: adapter.id,
                required_features: Features::GPU_DRIVEN,
                optional_features: Features::empty(),
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (device, queue)
    }

    /// A 2×2 opaque RGBA sheet — the smallest thing `upload_texture` accepts.
    fn sheet_desc<'a>(label: &'a str, pixels: &'a [u8], sample: SampleMode) -> SheetDesc<'a> {
        SheetDesc {
            label,
            width: 2,
            height: 2,
            sample,
            pixels,
        }
    }

    fn register(renderer: &mut SpriteRenderer, device: &dyn Device, label: &str) -> SheetId {
        let pixels = [255u8; 2 * 2 * 4];
        renderer
            .register_sheet(device, &sheet_desc(label, &pixels, SampleMode::Pixel))
            .expect("the null backend accepts this upload")
    }

    fn sprite(sheet: SheetId) -> Sprite {
        Sprite::new(sheet, [0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0])
    }

    /// A stand-in for a swapchain image, imported exactly as an acquired frame
    /// would be.
    fn target(device: &dyn Device) -> ImportedImage {
        let image = device
            .create_image(&ImageDesc {
                label: Some("fake swapchain image"),
                image_type: ImageType::D2,
                extent: Extent3d::d2(EXTENT.0, EXTENT.1),
                format: TARGET,
                mip_levels: 1,
                samples: 1,
                usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::PRESENT,
            })
            .expect("an image");
        let view = device
            .create_image_view(&ImageViewDesc {
                label: Some("fake swapchain view"),
                image,
                view_type: ImageViewType::D2,
                format: TARGET,
                range: ImageSubresourceRange::all(TARGET),
            })
            .expect("a view");
        ImportedImage {
            image,
            view,
            format: TARGET,
            extent: EXTENT,
            // `Load`, so the sprite pass composites onto something rather than
            // being the first writer.
            initial: ResourceState::ColorAttachment,
            final_state: ResourceState::Present,
        }
    }

    /// Runs one frame's sprite pass all the way to recorded HAL commands.
    fn run_pass(device: &dyn Device, queue: QueueHandle, renderer: &SpriteRenderer) {
        let imported = target(device);
        let mut pool = TransientPool::new();
        let mut graph = RenderGraph::new(queue);
        let swap = graph.import_image("swapchain", imported);
        renderer.add_pass(&mut graph, swap);
        let compiled = graph.compile(&pool).expect("a legal frame");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("sprite frame"),
            queue,
        });
        compiled
            .execute(device, &mut pool, encoder.as_mut(), None)
            .expect("execution succeeded");
        let commands = encoder.finish().expect("recording succeeded");
        device.destroy_command_buffer(commands);
        pool.destroy(device);
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);
    }

    fn draws(recorder: &Recorder) -> Vec<Range<u32>> {
        recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::Draw {
                    vertices,
                    instances,
                } => {
                    assert_eq!(
                        vertices,
                        0..QUAD_VERTICES,
                        "every sprite draw expands the same six-vertex quad"
                    );
                    Some(instances)
                }
                _ => None,
            })
            .collect()
    }

    /// The bind groups bound to set 1, in order — one per batch.
    fn sheet_binds(recorder: &Recorder) -> Vec<BindGroupHandle> {
        recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::BindGroup { slot, group, .. } if slot == SHEET_SET => Some(group),
                _ => None,
            })
            .collect()
    }

    /// The dynamic offsets each set-0 bind carried, in order.
    fn frame_bind_offsets(recorder: &Recorder) -> Vec<Vec<u32>> {
        recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::BindGroup {
                    slot,
                    dynamic_offsets,
                    ..
                } if slot == FRAME_SET => Some(dynamic_offsets),
                _ => None,
            })
            .collect()
    }

    fn as_floats(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|word| f32::from_le_bytes(word.try_into().expect("four bytes")))
            .collect()
    }

    // -----------------------------------------------------------------------
    // The ABI
    // -----------------------------------------------------------------------

    /// **The field-reordering test.** A `rect`/`uv`/`tint`/`sheet` permutation
    /// between Rust and Slang is the kind of mistake that renders *something*,
    /// so both sides are pinned here as well as in the goldens.
    ///
    /// The Rust half is the actual bytes, decoded back to floats at named
    /// offsets. The Slang half is the *compiled* WGSL, which `build.rs`
    /// hash-checks against the committed `.slang`, so a reorder on either side
    /// moves one of the two and this fails.
    #[test]
    fn an_instance_is_four_float4s_in_the_order_the_shader_reads() {
        assert_eq!(INSTANCE_STRIDE, 64);
        assert_eq!(align_of::<SpriteInstance>(), 4);
        assert_eq!(
            INSTANCE_STRIDE % 16,
            0,
            "every element of the array must land on a 16-byte boundary, or the \
             shader's float4 alignment is violated from element one onwards"
        );

        // Distinct in every lane, so a transposed or rotated field cannot
        // compare equal by accident — and, because it is built through
        // `Sprite::new`, a `rect` and a `uv` swapped inside the constructor
        // fail here too. The call sites are still on their own; see `new`.
        let sprite = Sprite::new(SheetId(0), [-3.5, 12.0, 64.0, 32.0], [0.25, 0.5, 0.75, 1.0])
            .with_rotation(1.25)
            .with_tint([0.1, 0.2, 0.3, 0.4]);
        let instance = sprite.instance(sheet_lane(16, 8, SampleMode::Pixel));
        let bytes = bytemuck::bytes_of(&instance);
        assert_eq!(bytes.len(), INSTANCE_STRIDE);
        let floats = as_floats(bytes);
        assert_eq!(&floats[0..4], &[-3.5, 12.0, 64.0, 32.0], "rect at byte 0");
        assert_eq!(&floats[4..8], &[0.25, 0.5, 0.75, 1.0], "uv at byte 16");
        assert_eq!(&floats[8..12], &[0.1, 0.2, 0.3, 0.4], "tint at byte 32");
        assert_eq!(
            &floats[12..16],
            &[16.0, 8.0, SAMPLE_PIXEL, 1.25],
            "sheet size, sample mode and rotation at byte 48 — the angle is the \
             fourth component and nothing else is"
        );

        // And the shader really does read them in that order. `slangc` names the
        // members and emits their alignment, so this is the compiled artifact's
        // own account of the layout rather than a restatement of the source.
        let wgsl = crcbl_shaders::SPRITE
            .wgsl()
            .expect("sprite has a WGSL half");
        let declared = wgsl
            .split_once("struct SpriteInstance_std430_0")
            .expect("the instance struct survives to WGSL")
            .1
            .split_once("};")
            .expect("and it is closed")
            .0;
        let members: Vec<&str> = declared
            .lines()
            .filter_map(|line| line.trim().strip_suffix(','))
            .collect();
        assert_eq!(
            members,
            [
                "@align(16) rect_0 : vec4<f32>",
                "@align(16) uv_0 : vec4<f32>",
                "@align(16) tint_0 : vec4<f32>",
                "@align(16) sheet_0 : vec4<f32>",
            ],
            "the shader's instance layout no longer matches SpriteInstance"
        );
    }

    /// The mode reaches the shader as an exact `lerp` weight, and the sheet's
    /// texel size reaches it at all.
    ///
    /// `sprite.slang` selects between the plain and the bent UV with
    /// `lerp(plain, sharp, sheet.z)` rather than a branch — the fragment stage
    /// must stay in uniform control flow — so a mode that arrived as anything
    /// other than exactly 0 or exactly 1 would silently blend the two filters.
    #[test]
    fn the_sample_mode_is_an_exact_zero_or_one_and_the_size_rides_beside_it() {
        assert_eq!(SAMPLE_PIXEL, 1.0);
        assert_eq!(SAMPLE_SMOOTH, 0.0);
        assert_eq!(
            sheet_lane(64, 32, SampleMode::Pixel),
            [64.0, 32.0, 1.0, 0.0]
        );
        assert_eq!(
            sheet_lane(64, 32, SampleMode::Smooth),
            [64.0, 32.0, 0.0, 0.0]
        );
        // Width and height in that order, and not swapped: a square sheet is the
        // one that cannot tell.
        let lane = sheet_lane(7, 3, SampleMode::Smooth);
        assert_eq!((lane[0], lane[1]), (7.0, 3.0));

        // The shader reads `sheet.xy` as the size and `sheet.z` as the mode, and
        // hands `xyz` on to the fragment stage flat — the compiled artifact's own
        // account of it, so a renamed or reordered varying fails here.
        let wgsl = crcbl_shaders::SPRITE
            .wgsl()
            .expect("sprite has a WGSL half");
        assert!(
            wgsl.contains("output_0.sheet_2 = s_0.sheet_0.xyz;"),
            "the vertex stage must pass the sheet lane through: {wgsl}"
        );
        assert!(
            wgsl.contains("@interpolate(flat)"),
            "the mode is a lerp weight and must not be interpolated: {wgsl}"
        );
        assert!(
            wgsl.contains("fwidth("),
            "sharp-bilinear's ramp width is a screen-space derivative; without \
             fwidth there is no ramp: {wgsl}"
        );
        assert!(
            wgsl.contains("round(pixels_0)"),
            "the vertex stage must snap the quad to the pixel grid: {wgsl}"
        );
        assert!(
            wgsl.contains("constants_0.viewport_0"),
            "and the snap must read the viewport rather than guess it: {wgsl}"
        );
    }

    /// **A zero angle writes the bytes the pass wrote before rotation existed.**
    ///
    /// The angle rides in the fourth component of the sheet lane, which was
    /// padding and was already `0.0`, so a sprite that does not turn produces
    /// exactly the sixty-four bytes it always did. That is what makes the eight
    /// golden images in `crates/crcbl-vk/tests/golden/` a regression check for
    /// this slice rather than eight pictures that had to be re-blessed: none of
    /// them draws a rotated sprite, and none of their instances changed.
    #[test]
    fn a_zero_rotation_writes_the_same_instance_bytes_as_a_sprite_without_one() {
        let lane = sheet_lane(16, 8, SampleMode::Pixel);
        let flat = Sprite::new(SheetId(0), [-3.5, 12.0, 64.0, 32.0], [0.25, 0.5, 0.75, 1.0])
            .with_tint([0.1, 0.2, 0.3, 0.4]);
        // What `instance` built before this slice: the sheet lane copied
        // verbatim, trailing pad and all.
        let before = SpriteInstance {
            rect: flat.rect,
            uv: flat.uv,
            tint: flat.tint,
            sheet: lane,
        };
        assert_eq!(
            bytemuck::bytes_of(&flat.instance(lane)),
            bytemuck::bytes_of(&before),
            "an unrotated sprite's instance must be byte-identical to the one \
             this pass produced before it could rotate at all"
        );

        // And a turned one differs in the last four bytes and nowhere else — so
        // the angle cost no space and displaced nothing.
        let turned = flat.with_rotation(core::f32::consts::FRAC_PI_2);
        let turned_instance = turned.instance(lane);
        let turned_bytes = bytemuck::bytes_of(&turned_instance);
        let flat_bytes = bytemuck::bytes_of(&before);
        assert_eq!(
            &turned_bytes[..60],
            &flat_bytes[..60],
            "rect, uv, tint and the sheet's own size and mode must not move"
        );
        assert_eq!(
            f32::from_le_bytes(turned_bytes[60..64].try_into().expect("four bytes")),
            core::f32::consts::FRAC_PI_2,
            "and the angle must land at byte 60, which is `sheet.w`"
        );
        assert_eq!(
            f32::from_le_bytes(flat_bytes[60..64].try_into().expect("four bytes")),
            0.0,
            "where an unrotated sprite leaves the zero that was already there"
        );
    }

    /// **The compiled shader reads the angle from where this side writes it, and
    /// the fragment stage never sees it.**
    ///
    /// The Rust half above is bytes; this is the *artifact's* account of what is
    /// done with them, so a `.slang` edited without regenerating, or regenerated
    /// without the rotation, fails here rather than only in a golden image.
    ///
    /// **The SPIR-V half was checked by hand rather than here**, because
    /// `crcbl_shaders::SPRITE::spirv` is a byte blob with no disassembler behind
    /// it. `spirv-dis` on the committed `spirv/sprite.spv` shows the same shape
    /// as the WGSL below: an `OpCompositeExtract %float … 3` for the angle, an
    /// `OpBranchConditional` on `angle != 0`, `GLSL.std.450 Sin`/`Cos` inside it,
    /// and — the part that matters — an untaken path whose instructions are the
    /// same `OpFMul`/`OpFAdd` pair the pass emitted before this slice.
    #[test]
    fn the_compiled_shader_turns_the_quad_from_the_lanes_fourth_component() {
        let wgsl = crcbl_shaders::SPRITE
            .wgsl()
            .expect("sprite has a WGSL half");
        assert!(
            wgsl.contains("s_0.sheet_0.w"),
            "the vertex stage must read the angle from the sheet lane's fourth \
             component: {wgsl}"
        );
        assert!(
            wgsl.contains("sin(angle_0)") && wgsl.contains("cos(angle_0)"),
            "and actually build a rotation from it: {wgsl}"
        );
        assert!(
            wgsl.contains("output_0.sheet_2 = s_0.sheet_0.xyz;"),
            "the varying is still xyz — rotation is a vertex-stage concern and \
             the fragment stage must not have grown a fourth component: {wgsl}"
        );
        assert!(
            wgsl.contains("round(pixels_0)"),
            "an unrotated Pixel quad still rounds its own corners: {wgsl}"
        );
        assert!(
            wgsl.contains("round(centrePixels_0)"),
            "and a rotated one snaps its centre instead — rounding four turned \
             corners independently shears the quad: {wgsl}"
        );
    }

    /// The same for the constant block: the matrix first, then the viewport the
    /// vertex stage snaps against, then the batch's base instance, then the
    /// padding `std140` insists on.
    #[test]
    fn the_constants_are_the_matrix_then_the_viewport_then_the_base() {
        assert_eq!(CONSTANTS_SIZE, 80);
        assert_eq!(
            CONSTANTS_SIZE % 16,
            0,
            "naga rejects a uniform block whose size is not a multiple of 16"
        );

        let constants = SpriteConstants {
            view_proj: Mat4::from_scale(glam::Vec3::new(2.0, 3.0, 4.0)).to_cols_array(),
            viewport: [1280.0, 720.0],
            base: 0x0102_0304,
            pad: 0,
        };
        let bytes = bytemuck::bytes_of(&constants);
        let floats = as_floats(bytes);
        assert_eq!(floats.len(), 20);
        assert_eq!(floats[0], 2.0, "m00 in the first four bytes");
        assert_eq!(floats[5], 3.0, "m11 twenty bytes in — column-major");
        assert_eq!(floats[10], 4.0, "m22");
        assert_eq!(&floats[16..18], &[1280.0, 720.0], "viewport at byte 64");
        assert_eq!(
            u32::from_le_bytes(bytes[72..76].try_into().expect("four bytes")),
            0x0102_0304,
            "the base instance sits at byte 72, which is `Offset 72` in the SPIR-V"
        );

        let wgsl = crcbl_shaders::SPRITE
            .wgsl()
            .expect("sprite has a WGSL half");
        let declared = wgsl
            .split_once("struct SpriteConstants_std140_0")
            .expect("the constant block survives to WGSL")
            .1
            .split_once("};")
            .expect("and it is closed")
            .0;
        let members: Vec<&str> = declared
            .lines()
            .filter_map(|line| line.trim().strip_suffix(','))
            .collect();
        assert_eq!(
            members,
            [
                "@align(16) view_proj_0 : _MatrixStorage_float4x4_ColMajorstd140_0",
                "@align(16) viewport_0 : vec2<f32>",
                "@align(8) base_0 : u32",
                "@align(4) pad_0 : u32",
            ],
            "the shader's constant layout no longer matches SpriteConstants"
        );
    }

    /// And `begin_frame` really writes that layout: one stride per sprite, at
    /// offset zero, into *this* frame's instance buffer.
    #[test]
    fn begin_frame_writes_one_stride_per_sprite_at_offset_zero() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = SpriteRenderer::new(device.as_ref(), queue, TARGET).expect("built");
        let sheet = register(&mut renderer, device.as_ref(), "sheet");

        let sprites = vec![sprite(sheet); 3];
        recorder.clear();
        renderer
            .begin_frame(device.as_ref(), &sprites, Mat4::IDENTITY, EXTENT)
            .expect("upload");

        let instance_buffer = renderer.instance_buffers[renderer.frame];
        let constant_buffer = renderer.constant_buffers[renderer.frame];
        let writes: Vec<(BufferHandle, u64, usize)> = recorder
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::BufferWritten {
                    buffer,
                    offset,
                    len,
                } => Some((buffer, offset, len)),
                _ => None,
            })
            .collect();
        assert_eq!(
            writes,
            [
                (instance_buffer, 0, 3 * INSTANCE_STRIDE),
                // Three sprites on one sheet is one batch, so one block.
                (constant_buffer, 0, renderer.constant_stride as usize),
            ],
            "a frame writes exactly its instances and its constants, once each"
        );

        renderer.destroy(device.as_ref());
    }

    // -----------------------------------------------------------------------
    // Batching
    // -----------------------------------------------------------------------

    /// **Submission order is the contract.** `A B A` is three batches, not two:
    /// grouping the two `A`s would put the second one on top of `B`, which the
    /// caller did not ask for.
    #[test]
    fn interleaving_two_sheets_makes_three_batches_not_two() {
        let (a, b) = (SheetId(0), SheetId(1));
        assert_eq!(
            batch(&[sprite(a), sprite(a), sprite(b), sprite(a)], 256),
            [
                Batch {
                    sheet: a,
                    instances: 0..2,
                    constant_offset: 0,
                },
                Batch {
                    sheet: b,
                    instances: 2..3,
                    constant_offset: 256,
                },
                Batch {
                    sheet: a,
                    instances: 3..4,
                    constant_offset: 512,
                },
            ]
        );
        // The consecutive case still collapses.
        assert_eq!(
            batch(&[sprite(a), sprite(a), sprite(a)], 256),
            [Batch {
                sheet: a,
                instances: 0..3,
                constant_offset: 0,
            }]
        );
        assert!(batch(&[], 256).is_empty());
    }

    /// The public count answers what the pass records, at every shape the rule
    /// distinguishes.
    ///
    /// [`batch_count`] exists so a consumer can stop keeping its own copy of the
    /// batching rule — `apps/horde` did, to put a number on its debug panel —
    /// and a count that could disagree with the batcher would be the same
    /// defect in a new place. It delegates, so this is a guard against someone
    /// later deciding the delegation is wasteful and walking the slice twice.
    #[test]
    fn the_public_batch_count_is_the_batchers_own_answer() {
        let (a, b) = (SheetId(0), SheetId(1));
        for sprites in [
            vec![],
            vec![sprite(a)],
            vec![sprite(a), sprite(a), sprite(a)],
            vec![sprite(a), sprite(b)],
            vec![sprite(a), sprite(a), sprite(b), sprite(a)],
            vec![sprite(a), sprite(b), sprite(a), sprite(b)],
        ] {
            assert_eq!(
                batch_count(&sprites),
                batch(&sprites, 256).len(),
                "{} sprites",
                sprites.len(),
            );
        }
        // And the value itself, so this is not two wrongs agreeing: `A A B A`
        // is the case a distinct-sheet count gets wrong.
        assert_eq!(
            batch_count(&[sprite(a), sprite(a), sprite(b), sprite(a)]),
            3
        );
        assert_eq!(batch_count(&[]), 0);
    }

    /// **The counters are what the pass recorded**, checked against the recorded
    /// stream rather than against a restatement of the batching rule.
    ///
    /// Two scenes whose answers differ in every field, so a counter wired to a
    /// constant, to the sprite count, or to the sheet count fails on one of
    /// them: `A A A` is one draw of three instances and `A A B A` is three draws
    /// of four. And the numbers are compared with the `Draw` commands the null
    /// backend actually saw, so a `counters` that stopped agreeing with
    /// `add_pass` fails here rather than reading plausibly on a panel.
    #[test]
    fn the_counters_are_the_draws_the_pass_records() {
        let recorder = Recorder::default();
        let (device, queue) = open(&recorder);
        let mut renderer =
            SpriteRenderer::new(device.as_ref(), queue, TARGET).expect("the null backend builds");
        let a = register(&mut renderer, device.as_ref(), "a");
        let b = register(&mut renderer, device.as_ref(), "b");

        // A scene of one run, and a scene the same rule splits into three.
        for (sprites, expected) in [
            (vec![sprite(a), sprite(a), sprite(a)], (1u64, 3u64)),
            (
                vec![sprite(a), sprite(a), sprite(b), sprite(a)],
                (3u64, 4u64),
            ),
        ] {
            recorder.clear();
            renderer
                .begin_frame(device.as_ref(), &sprites, Mat4::IDENTITY, EXTENT)
                .expect("the null backend accepts the upload");
            let counters = renderer.counters();
            run_pass(device.as_ref(), queue, &renderer);

            let recorded = draws(&recorder);
            let (draw_count, instance_count) = expected;
            assert_eq!(counters.draws, draw_count, "{} sprites", sprites.len());
            assert_eq!(
                counters.draws,
                recorded.len() as u64,
                "the counter and the recorded stream disagree",
            );
            assert_eq!(counters.instances, instance_count);
            assert_eq!(
                counters.instances,
                recorded
                    .iter()
                    .map(|range| u64::from(range.end))
                    .sum::<u64>(),
                "every draw covers 0..count, so the ends sum to the instances",
            );
            // Nothing here culls and nothing here is indirect, so both of the
            // `Option`s are answers rather than `indirect`.
            assert_eq!(counters.drawn, Some(instance_count));
            assert_eq!(counters.triangles, Some(instance_count * 2));
        }

        // A frame with no sprites adds no pass, so it recorded nothing — and
        // says so as a zero it knows, not as an unknown.
        recorder.clear();
        renderer
            .begin_frame(device.as_ref(), &[], Mat4::IDENTITY, EXTENT)
            .expect("an empty frame is legal");
        assert_eq!(renderer.counters(), FrameCounters::default());
        run_pass(device.as_ref(), queue, &renderer);
        assert!(draws(&recorder).is_empty());

        renderer.destroy(device.as_ref());
    }

    /// **Batch `n` gets batch `n`'s first sprite, and every block gets the same
    /// camera.** The recorder can see that a constant buffer was written and how
    /// long the write was; it cannot see what was *in* it, and what is in it is
    /// the entire fix — a block whose `base` was wrong would record identically.
    ///
    /// So the layout function is pure and this reads it directly.
    #[test]
    fn each_batchs_constant_block_carries_that_batchs_first_instance() {
        let (a, b) = (SheetId(0), SheetId(1));
        let stride = 256u32;
        let batches = batch(&[sprite(a), sprite(a), sprite(b), sprite(a)], stride);
        let blocks = constant_blocks(Mat4::IDENTITY, (640, 480), &batches, stride);

        assert_eq!(
            blocks.len(),
            3 * stride as usize,
            "one stride-sized block per batch"
        );
        let base_at = |index: usize| {
            let start = index * stride as usize + 72;
            u32::from_le_bytes(blocks[start..start + 4].try_into().expect("four bytes"))
        };
        assert_eq!(
            [base_at(0), base_at(1), base_at(2)],
            [0, 2, 3],
            "the bases must be the batches' own instance starts — all-zero here is \
             exactly the bug, every batch drawing the first batch's sprites"
        );
        // And every block still carries the frame's camera and viewport, which a
        // per-batch block is easy to leave stale in.
        for index in 0..3 {
            let start = index * stride as usize;
            let floats = as_floats(&blocks[start..start + CONSTANTS_SIZE as usize]);
            assert_eq!(floats[0], 1.0, "block {index} lost the identity matrix");
            assert_eq!(
                &floats[16..18],
                &[640.0, 480.0],
                "block {index} lost the viewport"
            );
        }

        // A frame with nothing to draw still writes one block, so the buffer is
        // never zero-sized and the write count does not depend on the scene.
        assert_eq!(
            constant_blocks(Mat4::IDENTITY, (640, 480), &[], stride).len(),
            stride as usize
        );
    }

    /// The constant ring grows in whole blocks, so an offset can never point
    /// past the end of the buffer that was allocated for it.
    ///
    /// **80 is in the list of strides on purpose.** A device whose
    /// `min_uniform_buffer_offset_alignment` is 16 gets a stride of exactly
    /// [`CONSTANTS_SIZE`], which is not a power of two — and rounding the *byte*
    /// count to a power of two instead of the block count passes every check at
    /// 128 and 256 and lands on a partial block there. `Limits::desktop` reports
    /// 64 and WebGPU reports 256, so nothing else in this suite would notice.
    #[test]
    fn the_constant_ring_grows_in_whole_blocks() {
        assert_eq!(grown_constants(256, 256), 256);
        assert_eq!(grown_constants(257, 256), 512);
        assert_eq!(grown_constants(5 * 256, 256), 8 * 256);
        for stride in [80u32, 128, 256] {
            for blocks in 1u64..40 {
                let needed = blocks * u64::from(stride);
                let size = grown_constants(needed, stride);
                assert_eq!(
                    size % u64::from(stride),
                    0,
                    "{blocks} blocks of {stride} rounded to a partial block: {size}"
                );
                assert!(
                    size >= needed,
                    "{blocks} blocks of {stride} shrank to {size}"
                );
            }
        }
    }

    /// And that reaches the command stream: three draws, in that order, each
    /// preceded by its own set-1 bind, with the first and third naming the
    /// *same* sheet group and the second a different one.
    #[test]
    fn three_batches_record_three_draws_over_the_ranges_they_claim() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = SpriteRenderer::new(device.as_ref(), queue, TARGET).expect("built");
        let a = register(&mut renderer, device.as_ref(), "a");
        let b = register(&mut renderer, device.as_ref(), "b");

        let sprites = [sprite(a), sprite(a), sprite(b), sprite(a)];
        renderer
            .begin_frame(device.as_ref(), &sprites, Mat4::IDENTITY, EXTENT)
            .expect("upload");

        recorder.clear();
        run_pass(device.as_ref(), queue, &renderer);

        // **Every draw starts at instance 0.** Not `0..2, 2..3, 3..4`, which is
        // what this recorded until the multi-sheet bug was found: a non-zero
        // `firstInstance` means one thing to SPIR-V's `SV_InstanceID` and the
        // opposite to WGSL's, so no shader can read it correctly on both. The
        // batch's place in the buffer travels in the constants instead — see the
        // dynamic offsets below and `add_pass`.
        assert_eq!(draws(&recorder), [0..2, 0..1, 0..1]);

        let offsets = frame_bind_offsets(&recorder);
        let stride = renderer.constant_stride;
        assert_eq!(
            offsets,
            [vec![0], vec![stride], vec![2 * stride]],
            "set 0 is rebound per batch at that batch's block; one bind for the \
             whole pass, or the same offset twice, is every batch reading the \
             first batch's instances again"
        );

        let binds = sheet_binds(&recorder);
        assert_eq!(binds.len(), 3, "one set-1 bind per batch");
        assert_eq!(
            binds[0], binds[2],
            "the first and third batches share a sheet"
        );
        assert_ne!(binds[0], binds[1], "the middle batch is the other sheet");
        assert_eq!(binds[0], renderer.sheets[a.index() as usize].group);
        assert_eq!(binds[1], renderer.sheets[b.index() as usize].group);

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **Layer 0 draws behind layer 1, all the way to the command stream.**
    ///
    /// The ordering slice 6 adds is [`crate::layers::LayerStack::resolve`]'s and
    /// nothing else's: this pass still batches consecutive sprites and still
    /// never sorts, so the evidence that back-to-front happened is that the
    /// *recorded draws* come out in layer order.
    ///
    /// Each layer gets its own sheet, so the two are distinguishable in the
    /// command stream — a draw range on its own cannot say which layer it came
    /// from. The submission is deliberately interleaved across layers, which is
    /// what a game does: a stack that appended to one flat list would record the
    /// two sheets alternating instead of two clean batches.
    #[test]
    fn the_back_layer_is_drawn_before_the_front_one() {
        use crate::layers::{LayerStack, Parallax};

        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = SpriteRenderer::new(device.as_ref(), queue, TARGET).expect("built");
        let back_sheet = register(&mut renderer, device.as_ref(), "background");
        let front_sheet = register(&mut renderer, device.as_ref(), "foreground");

        let mut stack = LayerStack::new();
        let back = stack.push_layer(Parallax::new(0.5).expect("finite"));
        let front = stack.push_layer(Parallax::WORLD);
        for (layer, sheet) in [
            (front, front_sheet),
            (back, back_sheet),
            (front, front_sheet),
            (back, back_sheet),
            (back, back_sheet),
        ] {
            stack.push(layer, sprite(sheet));
        }

        renderer
            .begin_frame(
                device.as_ref(),
                stack.resolve([0.0, 0.0]),
                Mat4::IDENTITY,
                EXTENT,
            )
            .expect("upload");

        recorder.clear();
        run_pass(device.as_ref(), queue, &renderer);

        assert_eq!(
            draws(&recorder),
            [0..3, 0..2],
            "the back layer's three sprites, then the front layer's two — five \
             draws would mean the stack never grouped, and one would mean the \
             pass sorted"
        );
        assert_eq!(
            frame_bind_offsets(&recorder),
            [vec![0], vec![renderer.constant_stride]],
            "and the front layer's draw reads the *front* layer's block — the \
             draw ranges above both start at zero, so this is the only thing \
             that says the second draw is not the first three sprites again"
        );
        let binds = sheet_binds(&recorder);
        assert_eq!(
            binds,
            [
                renderer.sheets[back_sheet.index() as usize].group,
                renderer.sheets[front_sheet.index() as usize].group,
            ],
            "and the first draw is the *back* layer's sheet"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **No sprite dropped, none drawn twice.** The batching arithmetic is the
    /// one place an off-by-one silently deletes the last sprite of every run.
    #[test]
    fn every_draw_together_covers_every_sprite_exactly_once() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = SpriteRenderer::new(device.as_ref(), queue, TARGET).expect("built");
        let sheets = [
            register(&mut renderer, device.as_ref(), "a"),
            register(&mut renderer, device.as_ref(), "b"),
            register(&mut renderer, device.as_ref(), "c"),
        ];
        // An awkward interleaving rather than a tidy one: runs of 1, 2 and 3,
        // starting and ending on different sheets.
        let order = [0usize, 1, 1, 2, 2, 2, 0, 1, 0, 0];
        let sprites: Vec<Sprite> = order.iter().map(|&i| sprite(sheets[i])).collect();
        renderer
            .begin_frame(device.as_ref(), &sprites, Mat4::IDENTITY, EXTENT)
            .expect("upload");

        recorder.clear();
        run_pass(device.as_ref(), queue, &renderer);

        // **A draw's range is no longer the sprites it covers.** Every draw
        // starts at instance 0; which sprites it lands on is the draw's count
        // *plus* the constant block the set-0 bind selected. So the absolute
        // range is reconstructed the way the shader reconstructs it — the block
        // the recorded dynamic offset names, plus the recorded count.
        //
        // That the block at offset `n * stride` really holds batch `n`'s first
        // sprite is `each_batchs_constant_block_carries_that_batchs_first_instance`,
        // which reads the bytes the recorder cannot.
        let stride = renderer.constant_stride;
        let batches = &renderer.batches[renderer.frame];
        let absolute: Vec<Range<u32>> = frame_bind_offsets(&recorder)
            .into_iter()
            .zip(draws(&recorder))
            .map(|(offsets, drawn)| {
                assert_eq!(offsets.len(), 1, "set 0 has exactly one dynamic binding");
                let block = (offsets[0] / stride) as usize;
                let base = batches[block].instances.start;
                assert_eq!(
                    offsets[0] % stride,
                    0,
                    "an offset that is not a whole block"
                );
                base + drawn.start..base + drawn.end
            })
            .collect();

        let mut covered = vec![0u32; sprites.len()];
        for range in &absolute {
            for index in range.clone() {
                covered[index as usize] += 1;
            }
        }
        assert_eq!(
            covered,
            vec![1u32; sprites.len()],
            "every sprite must be drawn exactly once, and the counts say which are not"
        );

        // And each draw's sheet is the sheet those sprites actually named.
        let binds = sheet_binds(&recorder);
        assert_eq!(binds.len(), absolute.len());
        for (group, range) in binds.iter().zip(&absolute) {
            for index in range.clone() {
                let expected =
                    renderer.sheets[sprites[index as usize].sheet.index() as usize].group;
                assert_eq!(
                    *group, expected,
                    "sprite {index} was drawn with another sheet"
                );
            }
        }

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// An empty frame records no draw — and no pass at all, because a render
    /// pass that draws nothing still costs a load and a store of the target.
    #[test]
    fn an_empty_frame_records_no_draw() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = SpriteRenderer::new(device.as_ref(), queue, TARGET).expect("built");
        let _sheet = register(&mut renderer, device.as_ref(), "unused");

        renderer
            .begin_frame(device.as_ref(), &[], Mat4::IDENTITY, EXTENT)
            .expect("an empty frame is not an error");

        recorder.clear();
        let imported = target(device.as_ref());
        let mut pool = TransientPool::new();
        let mut graph = RenderGraph::new(queue);
        let swap = graph.import_image("swapchain", imported);
        renderer.add_pass(&mut graph, swap);
        assert_eq!(graph.pass_count(), 0, "an empty frame adds no pass");

        // The graph still has the imported target and nothing to do with it,
        // which compiles and executes to the final transition alone.
        let compiled = graph.compile(&pool).expect("an empty frame is a legal one");
        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("empty frame"),
            queue,
        });
        compiled
            .execute(device.as_ref(), &mut pool, encoder.as_mut(), None)
            .expect("execution succeeded");
        let commands = encoder.finish().expect("recording succeeded");
        device.destroy_command_buffer(commands);
        pool.destroy(device.as_ref());
        device.destroy_image_view(imported.view);
        device.destroy_image(imported.image);

        assert!(draws(&recorder).is_empty(), "an empty frame draws nothing");
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A sprite naming a sheet nobody registered is refused by name, and the
    /// frame's buffers are left as they were rather than half-overwritten.
    #[test]
    fn a_sprite_naming_an_unregistered_sheet_is_refused() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = SpriteRenderer::new(device.as_ref(), queue, TARGET).expect("built");
        let sheet = register(&mut renderer, device.as_ref(), "only");

        recorder.clear();
        let error = renderer
            .begin_frame(
                device.as_ref(),
                &[sprite(sheet), sprite(SheetId(7))],
                Mat4::IDENTITY,
                EXTENT,
            )
            .expect_err("sheet 7 does not exist");
        let message = error.to_string();
        assert!(
            message.contains("sprite 1") && message.contains("sheet 7"),
            "the error must name the sprite and the sheet, got: {message}"
        );
        assert!(
            !recorder
                .events()
                .iter()
                .any(|event| matches!(event, Event::BufferWritten { .. })),
            "a refused frame must not have written anything"
        );

        renderer.destroy(device.as_ref());
    }

    // -----------------------------------------------------------------------
    // Pipeline state
    // -----------------------------------------------------------------------

    /// **The recorder cannot see pipeline state.**
    /// [`crcbl_hal::null::Event::Created`] keeps an [`ObjectKind`] and a label
    /// and discards the descriptor, so there is no command stream to assert the
    /// blend mode against — this checks the *value* `build` hands to
    /// `create_graphics_pipeline`, which is the strongest thing available
    /// without a golden image.
    ///
    /// [`ObjectKind`]: crcbl_hal::null::ObjectKind
    #[test]
    fn the_pipeline_blends_straight_alpha_and_never_tests_depth() {
        let target = color_target(TARGET);
        assert_eq!(target.format, TARGET);
        assert_eq!(
            target.blend,
            Some(BlendState::alpha()),
            "sprites composite with src*a + dst*(1-a); premultiplied would double the multiply"
        );
        assert_ne!(
            target.blend,
            Some(BlendState::premultiplied()),
            "the two differ only in the colour source factor, which is exactly why \
             this is asserted rather than eyeballed"
        );
        assert_eq!(target.write_mask, ColorWrites::ALL);

        assert!(
            SPRITE_DEPTH_STENCIL.is_none(),
            "a depth test would discard the half-transparent fragments that make a \
             sprite a sprite, and a depth write would order them by z rather than \
             by submission"
        );
    }

    /// **Both modes bind the same linear sampler, and the mode reaches the
    /// shader on the instance instead.**
    ///
    /// This is the test that says the seam moved. Slice 3 gave
    /// [`SampleMode::Pixel`] a nearest sampler as a named placeholder; a nearest
    /// sampler would flatten sharp-bilinear's bent UV back onto a texel centre
    /// and produce exactly the nearest-neighbour picture the mode exists to
    /// avoid. So there is one sampler, and the difference between the modes is
    /// entirely in the instance bytes — which is what this checks, end to end,
    /// through `begin_frame`'s real write.
    #[test]
    fn both_modes_share_one_linear_sampler_and_differ_only_on_the_instance() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = SpriteRenderer::new(device.as_ref(), queue, TARGET).expect("built");

        assert_eq!(
            recorder.live_objects(crcbl_hal::null::ObjectKind::Sampler),
            1,
            "one sampler for the whole pass: a second one would be the nearest \
             placeholder coming back"
        );

        // Two sheets of *different* sizes as well as different modes, so a lane
        // taken from the wrong sheet is visible and not just the mode being
        // wrong.
        let big = vec![255u8; 4 * 2 * 4];
        let small = [255u8; 2 * 2 * 4];
        let sharp = renderer
            .register_sheet(
                device.as_ref(),
                &SheetDesc {
                    label: "pixel sheet",
                    width: 4,
                    height: 2,
                    sample: SampleMode::Pixel,
                    pixels: &big,
                },
            )
            .expect("upload");
        let smooth = renderer
            .register_sheet(
                device.as_ref(),
                &SheetDesc {
                    label: "smooth sheet",
                    width: 2,
                    height: 2,
                    sample: SampleMode::Smooth,
                    pixels: &small,
                },
            )
            .expect("upload");

        assert_eq!(
            recorder.live_objects(crcbl_hal::null::ObjectKind::Sampler),
            1,
            "registering sheets in two different modes must not create a second \
             sampler either"
        );
        assert_eq!(
            renderer.sheets[sharp.index() as usize].lane,
            [4.0, 2.0, SAMPLE_PIXEL, 0.0],
            "a Pixel sheet's lane is its texel size and mode 1"
        );
        assert_eq!(
            renderer.sheets[smooth.index() as usize].lane,
            [2.0, 2.0, SAMPLE_SMOOTH, 0.0],
            "a Smooth sheet's lane is its texel size and mode 0"
        );

        // And that lane is what `begin_frame` stamps onto the instance, from the
        // sheet the sprite named rather than from the sprite.
        let stamped = sprite(smooth).instance(renderer.sheets[smooth.index() as usize].lane);
        assert_eq!(stamped.sheet, [2.0, 2.0, SAMPLE_SMOOTH, 0.0]);

        renderer
            .begin_frame(
                device.as_ref(),
                &[sprite(sharp), sprite(smooth)],
                Mat4::IDENTITY,
                EXTENT,
            )
            .expect("upload");

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    // -----------------------------------------------------------------------
    // Lifetime
    // -----------------------------------------------------------------------

    /// Teardown releases everything, including a registered sheet's image, its
    /// view and its bind group — the three objects `destroy` would leak if it
    /// only walked the per-frame vectors.
    #[test]
    fn teardown_releases_every_sheet_as_well_as_the_frame_resources() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let before = recorder.total_live_objects();

        let mut renderer = SpriteRenderer::new(device.as_ref(), queue, TARGET).expect("built");
        let a = register(&mut renderer, device.as_ref(), "a");
        let b = register(&mut renderer, device.as_ref(), "b");
        assert_eq!(renderer.sheet_count(), 2);
        assert_eq!(
            (a.index(), b.index()),
            (0, 1),
            "ids are handed out in order"
        );
        assert!(
            recorder.live_objects(crcbl_hal::null::ObjectKind::Image) >= 2,
            "the test is only meaningful if the sheets really were uploaded"
        );

        for _ in 0..FRAMES_IN_FLIGHT * 2 {
            renderer
                .begin_frame(
                    device.as_ref(),
                    &[sprite(a), sprite(b)],
                    Mat4::IDENTITY,
                    EXTENT,
                )
                .expect("upload");
        }
        renderer.destroy(device.as_ref());

        assert_eq!(
            recorder.total_live_objects(),
            before,
            "destroy must release the sheets' images, views and bind groups too"
        );
        recorder.assert_valid();
    }

    /// The failure path in `new`. A device that can bind only **one** descriptor
    /// set refuses this pass's pipeline layout, which asks for two — after the
    /// samplers, both layouts, four buffers and two bind groups already exist.
    #[test]
    fn a_new_that_fails_part_way_leaks_nothing() {
        let recorder = Recorder::new();
        let instance = NullInstance::new(DeviceCaps {
            features: Features::GPU_DRIVEN,
            limits: Limits {
                max_bind_groups: 1,
                ..Limits::desktop()
            },
        })
        .with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc {
                label: Some("one bind group"),
                adapter: adapter.id,
                required_features: Features::GPU_DRIVEN,
                optional_features: Features::empty(),
                compatible_surface: None,
            })
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let before = recorder.total_live_objects();

        let error = SpriteRenderer::new(device.as_ref(), queue, TARGET)
            .expect_err("two bind groups do not fit in max_bind_groups 1");
        assert!(
            error.to_string().contains("max_bind_groups"),
            "the failure must be the pipeline layout, not something earlier: {error}"
        );
        assert!(
            recorder
                .events()
                .iter()
                .any(|event| matches!(event, Event::Created { kind, .. } if *kind
                    == crcbl_hal::null::ObjectKind::BindGroup)),
            "the test is only meaningful if objects really were created before the failure"
        );
        assert_eq!(
            recorder.total_live_objects(),
            before,
            "everything created before the failure must be handed back"
        );
        recorder.assert_valid();
    }

    /// **The bug `ui_pass` had and this must not reinherit.** A scene whose
    /// sprite count has not changed must not allocate: the ring is compared in
    /// *bytes* against *bytes*, and comparing bytes against a sprite count is
    /// what made the UI pass destroy and recreate its buffers and its bind group
    /// every single frame.
    #[test]
    fn a_second_frame_of_the_same_size_creates_no_buffer() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = SpriteRenderer::new(device.as_ref(), queue, TARGET).expect("built");
        let sheet = register(&mut renderer, device.as_ref(), "sheet");
        let sprites = vec![sprite(sheet); 8];

        // Two frames to fill both slots of the ring, then measure.
        for _ in 0..FRAMES_IN_FLIGHT {
            renderer
                .begin_frame(device.as_ref(), &sprites, Mat4::IDENTITY, EXTENT)
                .expect("upload");
        }
        let buffers = renderer.instance_buffers.clone();
        let groups = renderer.frame_groups.clone();
        let settled = recorder.total_live_objects();
        recorder.clear();

        for _ in 0..8 {
            renderer
                .begin_frame(device.as_ref(), &sprites, Mat4::IDENTITY, EXTENT)
                .expect("upload");
        }
        assert!(
            !recorder
                .events()
                .iter()
                .any(|event| matches!(event, Event::Created { .. })),
            "a steady-state frame must create nothing at all"
        );
        assert_eq!(recorder.total_live_objects(), settled);
        assert_eq!(
            renderer.instance_buffers, buffers,
            "the instance ring must be reused, not reallocated"
        );
        assert_eq!(
            renderer.frame_groups, groups,
            "the frame bind group only changes when its instance buffer does"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// It still grows when a frame genuinely needs more room, and the old buffer
    /// is released rather than leaked.
    #[test]
    fn a_bigger_frame_grows_the_ring_and_rebinds_it() {
        let recorder = Recorder::new();
        let (device, queue) = open(&recorder);
        let mut renderer = SpriteRenderer::new(device.as_ref(), queue, TARGET).expect("built");
        let sheet = register(&mut renderer, device.as_ref(), "sheet");

        let small = vec![sprite(sheet); 4];
        renderer
            .begin_frame(device.as_ref(), &small, Mat4::IDENTITY, EXTENT)
            .expect("upload");
        assert_eq!(
            renderer.instance_capacity[renderer.frame],
            INITIAL_RING_BYTES
        );

        // 1024 bytes holds 16 instances; 64 needs 4096.
        let big = vec![sprite(sheet); 64];
        let before_groups = renderer.frame_groups.clone();
        for _ in 0..FRAMES_IN_FLIGHT {
            renderer
                .begin_frame(device.as_ref(), &big, Mat4::IDENTITY, EXTENT)
                .expect("upload");
        }
        let index = renderer.frame;
        assert!(
            renderer.instance_capacity[index] >= (64 * INSTANCE_STRIDE) as u64,
            "the ring must actually fit the frame, got {}",
            renderer.instance_capacity[index]
        );
        assert_ne!(
            renderer.frame_groups[index], before_groups[index],
            "a replaced instance buffer needs a bind group that names it"
        );

        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// `grown` rounds up rather than fitting exactly, so a scene that gains a
    /// sprite a frame reallocates a handful of times rather than every frame.
    #[test]
    fn the_ring_grows_in_power_of_two_steps() {
        assert_eq!(grown(1), INITIAL_RING_BYTES);
        assert_eq!(grown(INITIAL_RING_BYTES), INITIAL_RING_BYTES);
        assert_eq!(grown(INITIAL_RING_BYTES + 1), 2048);
        assert_eq!(grown(3072), 4096);
        for needed in [1u64, 49, 1025, 3072, 100_000] {
            let size = grown(needed);
            assert!(size >= needed);
            assert_eq!(size % 256, 0, "{needed} grew to an unaligned {size}");
        }
    }
}
