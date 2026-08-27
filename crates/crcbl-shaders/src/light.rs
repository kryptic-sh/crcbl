//! The light list and the froxel grid `light_cluster.slang` fills, in the byte
//! layouts those shaders declare.
//!
//! `docs/plan/18-render-features.md`'s "Many lights" section, both halves: "a
//! light is a row" in a storage buffer exactly as [`GpuInstance`] and
//! [`GpuMaterial`] already are, and "a compute pass assigns them to a froxel
//! grid — screen tiles by depth slices — which the fragment stage indexes by its
//! own position".
//!
//! # The sun is a row like any other
//!
//! [`KIND_DIRECTIONAL`](crate::light::KIND_DIRECTIONAL) exists so the sun stops being a
//! pair of fields in the
//! frame block that only one term in one shader knows how to read. A directional
//! row has no position and no radius; the clustering pass puts it in **every**
//! froxel, which is what "affects every cluster" means in a list the fragment
//! stage walks without asking what kind of frame it is in.
//!
//! [`GpuInstance`]: crate::mesh::GpuInstance
//! [`GpuMaterial`]: crate::mesh::GpuMaterial

/// Bytes per [`GpuLight`], and the stride of the light storage buffer.
///
/// Three `float4` then two 4-byte scalars and two words of padding — 64 rather
/// than 56, because the row's alignment is the `float4`s' 16 and `std430` rounds
/// the size up to a multiple of it. Spelled out in the shader struct rather than
/// implied, exactly as [`GpuMaterial`](crate::mesh::GpuMaterial) spells its own.
/// Checked against the `ArrayStride` and `Offset` decorations `slangc` emits by
/// this module's `the_light_layout_matches_the_offsets_slangc_emits`.
pub const LIGHT_STRIDE: usize = 64;

/// [`GpuLight::kind`] for the sun: no position, no radius, no falloff, and a
/// member of every froxel.
pub const KIND_DIRECTIONAL: u32 = 0;

/// [`GpuLight::kind`] for a point light: [`GpuLight::position`]'s `w` is the
/// radius its falloff reaches zero at, and past which it is culled.
pub const KIND_POINT: u32 = 1;

/// [`GpuLight::kind`] for a spot light: a point light with
/// [`GpuLight::direction`] as its cone axis and the two cosines closing it.
pub const KIND_SPOT: u32 = 2;

/// How wide a froxel is on screen, in pixels, and how tall.
///
/// Square tiles, so a froxel's screen footprint does not change shape with the
/// aspect ratio. 64 is the usual choice and it is a trade with nothing clever in
/// it: halving it quadruples the grid (and the clustering dispatch), doubling it
/// puts more lights in each cluster's list and so in each fragment's loop.
pub const CLUSTER_TILE_PIXELS: u32 = 64;

/// How many depth slices the grid has, and the `z` extent of the froxel grid.
///
/// The slices are distributed **exponentially** over
/// [`CLUSTER_NEAR`]..[`CLUSTER_FAR`] — see [`slice_scale`] — so this is a count
/// of near-constant *ratios* of depth rather than of equal metres.
pub const CLUSTER_DEPTH_SLICES: u32 = 24;

/// **The budget**: how many light indices one froxel's list holds.
///
/// A froxel that wants more keeps the first this many, in light-list order, and
/// counts the rest into [`CLUSTER_OVERFLOW_WORD`] — never silently dropping
/// them, which is `docs/plan/18-render-features.md`'s requirement on this
/// number.
pub const CLUSTER_LIGHT_CAPACITY: u32 = 16;

/// Words one froxel occupies in the grid buffer: its light count, then
/// [`CLUSTER_LIGHT_CAPACITY`] slots for indices.
///
/// The count lives **in** the list rather than in a buffer beside it, so the
/// grid is one binding and one allocation. A fragment reads word 0 of its own
/// froxel and then that many of the words after it.
pub const CLUSTER_STRIDE: u32 = CLUSTER_LIGHT_CAPACITY + 1;

/// The word of the culling-statistics buffer the clustering pass counts
/// **dropped light assignments** into.
///
/// One increment per (froxel, light) pair that did not fit, not one per froxel:
/// the first says how much of the scene's lighting the budget is refusing, the
/// second only that some froxel refused something. Deterministic — a froxel
/// walks the light list in index order and keeps a prefix of it — so a test can
/// assert the exact number, which is what
/// `crcbl`'s `forward_e2e::lights` does, on every backend.
///
/// It is a word of `cull.slang`'s statistics buffer rather than a counter of its
/// own for the reason [`crate::cull::CLUSTER_SURVIVOR_WORD`] is: topic 03 §3.6
/// allows the frame loop **one** readback, and a second buffer would be a second
/// one. `clear_counters.slang` zeroes it with the rest because that pass is told
/// how many words the buffer has rather than assuming.
pub const CLUSTER_OVERFLOW_WORD: u32 = 2;

const _: () = assert!(CLUSTER_OVERFLOW_WORD < crate::cull::STATS_WORDS);
const _: () = assert!(CLUSTER_OVERFLOW_WORD != crate::cull::INSTANCE_SURVIVOR_WORD);
const _: () = assert!(CLUSTER_OVERFLOW_WORD != crate::cull::CLUSTER_SURVIVOR_WORD);
const _: () = assert!(CLUSTER_OVERFLOW_WORD != crate::cull::CLUSTER_FRUSTUM_REJECT_WORD);
const _: () = assert!(CLUSTER_OVERFLOW_WORD != crate::cull::CLUSTER_CONE_REJECT_WORD);

/// The view depth the first slice starts at, in world units.
///
/// **Not the camera's near plane**, deliberately. The near plane is a precision
/// knob and callers are told to raise it; the slice distribution is a statement
/// about where lights are, and tying the two would make a camera with a 1 mm
/// near plane spend a third of its slices inside the first centimetre. A
/// fragment nearer than this lands in slice 0, which is correct — slice 0's
/// froxel is built from this depth and the clustering pass clamps a light's
/// depth band into the grid rather than off it.
pub const CLUSTER_NEAR: f32 = 0.1;

/// The view depth the last slice ends at, in world units.
///
/// Finite, unlike the camera's far plane, because an exponential distribution
/// needs a ratio and infinity has none. It is not a cull distance: a fragment
/// beyond it uses the last slice, and the clustering pass gives the last slice
/// an **unbounded** far side so a light out there is still in the list. See
/// `light_cluster.slang`'s per-light depth cut.
pub const CLUSTER_FAR: f32 = 1000.0;

/// The ratio between one depth slice's start and the next:
/// `(CLUSTER_FAR / CLUSTER_NEAR)^(1 / CLUSTER_DEPTH_SLICES)`.
///
/// The same split [`slice_near`] writes as a power, named because
/// `volumetric.slang` and `volumetric_composite.slang` walk it as a **chain of
/// multiplies** rather than calling `pow`. That is not a micro-optimisation: a
/// slice boundary is an integer index in `light_cluster.slang`, where a
/// last-place disagreement changes nothing, and an endpoint of an optical-depth
/// integral in the volumetric pair, where it reaches a colour and
/// `docs/plan/44-lighting.md`'s shading rule applies. A multiply chain is exact
/// arithmetic on every target and has no add in it for a compiler to contract.
///
/// `the_slice_ratio_is_the_split_it_claims` is what holds the constant to the
/// power it stands for.
pub const SLICE_RATIO: f32 = 1.467_799_3;

/// Bytes in the clustering pass's parameter block, matching
/// `struct LightClusterParams` in `shaders/light_cluster.slang`.
///
/// One `float4x4` (64), two `float4` (16 each) and eight `uint`s (32). Checked
/// against the `Offset` decorations `slangc` emits — 0, 64, 80, 96, 100, 104,
/// 108, 112, 116, 120, 124 — by this module's
/// `the_cluster_params_block_writes_its_fields_in_declaration_order`.
pub const CLUSTER_PARAMS_SIZE: usize = 128;

/// Invocations per workgroup in `light_cluster.slang`, one per froxel.
pub const CLUSTER_WORKGROUP_SIZE: u32 = 64;

/// How many slices one doubling of view depth is worth: the `scale` of
/// `slice = floor(log2(depth) * scale + bias)`.
///
/// The distribution is Olsson and Assarsson's exponential split,
/// `z(k) = near * (far / near)^(k / slices)`, inverted. **Exponential and not
/// uniform**: a uniform split over `near..far` spends every slice but the first
/// on the far field, where a froxel is so large that its light list is the whole
/// list.
///
/// Neither this nor [`slice_bias`] is uploaded. `mesh.slang` computes both from
/// the same four constants it declares copies of, so there is no pair of numbers
/// travelling between the two sides to be written wrong; these exist so the host
/// can say what the shader will decide, which is what
/// `the_slice_mapping_round_trips` and `crcbl-vk`'s light suite need.
#[must_use]
pub fn slice_scale() -> f32 {
    CLUSTER_DEPTH_SLICES as f32 / (CLUSTER_FAR / CLUSTER_NEAR).log2()
}

/// Where that count starts from: the `bias` of
/// `slice = floor(log2(depth) * scale + bias)`.
#[must_use]
pub fn slice_bias() -> f32 {
    -slice_scale() * CLUSTER_NEAR.log2()
}

/// Which slice a fragment at view depth `depth` lands in, given a grid of
/// `slices` — the same arithmetic `mesh.slang`'s `froxel_of` performs.
///
/// The host's copy of the mapping, for a caller that has to know which froxel a
/// point will be shaded out of. Clamped at both ends exactly as the shader
/// clamps: a depth nearer than [`CLUSTER_NEAR`] is slice 0 and one past
/// [`CLUSTER_FAR`] is the last, which is what makes the last slice's unbounded
/// far side reachable at all.
#[must_use]
pub fn slice_of(depth: f32, slices: u32) -> u32 {
    let raw = (depth.max(CLUSTER_NEAR).log2() * slice_scale() + slice_bias()).floor();
    raw.clamp(0.0, (slices.max(1) - 1) as f32) as u32
}

/// The view depth slice `index` starts at — the same
/// `near * (far / near)^(k / slices)` [`slice_scale`] inverts.
///
/// Public because the clustering pass needs the same numbers on the GPU and a
/// test needs them on the host; `the_slice_mapping_round_trips` is what says the
/// two are inverses.
#[must_use]
pub fn slice_near(index: u32) -> f32 {
    CLUSTER_NEAR * (CLUSTER_FAR / CLUSTER_NEAR).powf(index as f32 / CLUSTER_DEPTH_SLICES as f32)
}

/// One light, matching `struct GpuLight` in `shaders/light_cluster.slang` and in
/// `shaders/mesh.slang`.
///
/// # Nine floats and two scalars, and not a `float3` in sight
///
/// Under `std430` a `float3` is 16-byte aligned and 12 bytes wide, so a struct
/// mixing them with scalars grows padding the CPU side has to reproduce exactly.
/// Three `float4`s carry every vector here with the scalar each of them needs
/// tucked into the `w` it would otherwise waste — which is why the radius lives
/// in [`position`](Self::position) and the outer cone cosine in
/// [`direction`](Self::direction) rather than in fields of their own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuLight {
    /// World-space position in `xyz` and the radius its influence ends at in
    /// `w`.
    ///
    /// Both are unread for [`KIND_DIRECTIONAL`], which has neither. For the two
    /// punctual kinds the radius is a **hard** bound and not a fade: the shading
    /// window below reaches exactly zero there, which is what lets the
    /// clustering pass cull against it without leaving a seam where a froxel
    /// stopped listing a light that was still contributing.
    pub position: [f32; 4],
    /// Colour premultiplied by intensity in `rgb`; `w` unused.
    ///
    /// **May exceed 1.0**, like every colour in this pass: the scene target is
    /// `Rgba16Float` and the tonemap pass is what maps it into the swapchain.
    pub color: [f32; 4],
    /// For [`KIND_DIRECTIONAL`], the unit vector **towards** the light in `xyz`
    /// — the vector both the Lambert term and the specular lobe's half-vector
    /// want. For
    /// [`KIND_SPOT`], the unit vector the cone points **along**, away from the
    /// light. Unread for [`KIND_POINT`].
    ///
    /// `w` is the cosine of the spot's outer half-angle, where the cone closes;
    /// unread by the other two kinds.
    pub direction: [f32; 4],
    /// Which of [`KIND_DIRECTIONAL`], [`KIND_POINT`], [`KIND_SPOT`] this is.
    pub kind: u32,
    /// The cosine of a spot's inner half-angle, inside which it is at full
    /// brightness. Unread by the other two kinds.
    ///
    /// Must be **greater** than the outer cosine in
    /// [`direction`](Self::direction)'s `w` — a larger cosine is a narrower cone
    /// — because the shader divides by the difference.
    pub cos_inner: f32,
    /// The **first** light tile this light occludes through, or
    /// [`NO_SHADOW_TILE`] if it was given none.
    ///
    /// An index into [`FrameUniforms::light_view_proj`](crate::mesh::FrameUniforms::light_view_proj),
    /// and through `crcbl_render::shadow::light_tile` the atlas tile the map was
    /// rendered into. `crcbl_render::shadow::Selection` is what fills it and
    /// `docs/plan/18-render-features.md`'s 2026-08-13 decision is the rule it
    /// applies.
    ///
    /// **The first, because a light may own more than one.** A spot owns this
    /// tile alone; a point light owns the
    /// [`SHADOW_POINT_FACES`](crate::mesh::SHADOW_POINT_FACES) tiles from here,
    /// one per cube face, and the shader adds the face it selected to this
    /// number.
    ///
    /// **[`NO_SHADOW_TILE`] is the ordinary case, not an error.** The atlas has
    /// room for [`SHADOW_LIGHT_TILES`](crate::mesh::SHADOW_LIGHT_TILES) tiles and
    /// a scene may want more than they hold; a light that misses out still lights
    /// and simply does not occlude, which is what makes the budget a quality knob
    /// rather than a correctness cliff. A directional row carries it too — the
    /// sun occludes through the cascades, which are a different array and a
    /// different code path.
    ///
    /// This was the first of the row's two padding words. It is spent rather
    /// than added to the row because `std430` had already rounded the row up to
    /// [`LIGHT_STRIDE`] to hold it, so the shadow tile costs no bytes at all.
    pub shadow_tile: u32,
    /// Pads the row to [`LIGHT_STRIDE`], which is what `std430` rounds it to
    /// anyway.
    pub pad1: u32,
}

/// [`GpuLight::shadow_tile`] for a light with no shadow map of its own.
///
/// Deliberately not zero: zero is tile 0, which is a real tile, so a row that
/// forgot to say would occlude through whichever light does hold that tile.
/// `0xffff_ffff` is past every tile there will ever be, and the shader compares
/// against the tile count rather than against this value — so a row carrying
/// anything else out of range is refused the same way.
pub const NO_SHADOW_TILE: u32 = u32::MAX;

impl GpuLight {
    /// The bytes a light row holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; LIGHT_STRIDE] {
        let mut bytes = [0u8; LIGHT_STRIDE];
        let mut at = 0usize;
        let mut put = |value: [u8; 4]| {
            bytes[at..at + 4].copy_from_slice(&value);
            at += 4;
        };
        for vector in [&self.position, &self.color, &self.direction] {
            for value in vector {
                put(value.to_le_bytes());
            }
        }
        put(self.kind.to_le_bytes());
        put(self.cos_inner.to_le_bytes());
        put(self.shadow_tile.to_le_bytes());
        put(self.pad1.to_le_bytes());
        debug_assert_eq!(at, LIGHT_STRIDE);
        bytes
    }
}

/// A row that lights nothing and occludes nothing.
///
/// **Hand-written rather than derived**, and that is the whole point of it:
/// `#[derive(Default)]` would zero [`shadow_tile`](GpuLight::shadow_tile), and
/// zero is tile 0 — a real tile. A row defaulted that way would occlude through
/// whichever light actually holds that tile, in a scene where nothing said it
/// should cast at all. [`NO_SHADOW_TILE`] is the only correct zero here.
impl Default for GpuLight {
    fn default() -> Self {
        Self {
            position: [0.0; 4],
            color: [0.0; 4],
            direction: [0.0; 4],
            kind: KIND_DIRECTIONAL,
            cos_inner: 0.0,
            shadow_tile: NO_SHADOW_TILE,
            pad1: 0,
        }
    }
}

/// What the clustering pass cannot derive for itself, matching
/// `struct LightClusterParams` in `shaders/light_cluster.slang`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClusterParams {
    /// Clip → world, column-major, for unprojecting a froxel's screen corners
    /// onto the camera's near plane.
    pub inverse_view_proj: [f32; 16],
    /// World-space eye position in `xyz`; `w` unused. Every froxel corner is a
    /// point on the ray from here through the near plane, which is what makes a
    /// corner at an arbitrary view depth one multiply rather than a second
    /// unprojection.
    pub eye: [f32; 4],
    /// Row 3 of the view-projection, so `dot(depth_row, float4(p, 1))` is `p`'s
    /// view depth — the same `clip.w` the fragment stage recomputes to pick its
    /// own slice. Passed rather than derived from `inverse_view_proj` so the two
    /// sides cannot disagree about which row it is.
    pub depth_row: [f32; 4],
    /// Froxels across the screen.
    pub grid_x: u32,
    /// Froxels down the screen.
    pub grid_y: u32,
    /// Depth slices, which is [`CLUSTER_DEPTH_SLICES`] for a perspective camera
    /// and `1` for an orthographic one — see [`perspective`](Self::perspective).
    pub slices: u32,
    /// Rows in the light buffer.
    pub light_count: u32,
    /// The viewport's width in pixels.
    ///
    /// A tile's NDC rectangle is derived from its **pixel** rectangle and this,
    /// rather than from its share of the grid: the grid is
    /// `ceil(width / CLUSTER_TILE_PIXELS)` wide, so a viewport the tile size
    /// does not divide leaves the last column part-empty, and a froxel built as
    /// `tile / grid_x` of the screen would be a different rectangle from the one
    /// the fragment stage divides its pixel down into.
    pub viewport_x: u32,
    /// The viewport's height in pixels, for the same reason.
    pub viewport_y: u32,
    /// `1` for a perspective camera and `0` for an orthographic one.
    ///
    /// The two build a froxel differently and neither is a degenerate case of
    /// the other: a perspective froxel's corners are points along rays from the
    /// eye at a given view depth, and an orthographic camera has no eye those
    /// rays converge on — its `clip.w` is `1` everywhere, so it has no view
    /// depth for a slice index to come from either. An orthographic frame
    /// therefore runs with [`slices`](Self::slices) at `1` and one froxel per
    /// tile spanning its whole depth range: coarser, correct, and not silently
    /// something else.
    pub perspective: u32,
    /// How many pixels wide and tall one tile is, matching
    /// [`FrameUniforms::cluster_grid`](crate::mesh::FrameUniforms::cluster_grid)'s
    /// `w`.
    ///
    /// [`CLUSTER_TILE_PIXELS`] at any viewport whose grid fits the buffer the
    /// renderer allocated, and doubled where it does not — a very large frame
    /// gets coarser tiles rather than a grid running off the end of its buffer.
    /// A parameter rather than the constant, because both sides have to agree on
    /// the doubled value too.
    pub tile_pixels: u32,
}

impl ClusterParams {
    /// The bytes the parameter block holds, in `std140` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; CLUSTER_PARAMS_SIZE] {
        let mut bytes = [0u8; CLUSTER_PARAMS_SIZE];
        let mut at = 0usize;
        let mut put = |value: [u8; 4]| {
            bytes[at..at + 4].copy_from_slice(&value);
            at += 4;
        };
        for value in self.inverse_view_proj {
            put(value.to_le_bytes());
        }
        for vector in [&self.eye, &self.depth_row] {
            for value in vector {
                put(value.to_le_bytes());
            }
        }
        for value in [
            self.grid_x,
            self.grid_y,
            self.slices,
            self.light_count,
            self.viewport_x,
            self.viewport_y,
            self.perspective,
            self.tile_pixels,
        ] {
            put(value.to_le_bytes());
        }
        debug_assert_eq!(at, CLUSTER_PARAMS_SIZE);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = include_str!("../shaders/light_cluster.slang");
    const MESH_SOURCE: &str = include_str!("../shaders/mesh.slang");

    #[test]
    fn the_workgroup_size_matches_the_numthreads_light_cluster_slang_declares() {
        assert!(
            SOURCE.contains(&format!("[numthreads({CLUSTER_WORKGROUP_SIZE}, 1, 1)]")),
            "light_cluster.slang must declare [numthreads({CLUSTER_WORKGROUP_SIZE}, 1, 1)]"
        );
    }

    /// Every number this module names and the shaders re-declare, in one sweep.
    ///
    /// A `static const` in Slang and a `pub const` here are two spellings of one
    /// value and nothing but this compares them — a grid the fragment stage
    /// indexes at one stride while the compute pass writes at another is a
    /// picture, not an error.
    #[test]
    fn the_shaders_declare_the_grid_constants_this_module_names() {
        for (name, value) in [
            ("CLUSTER_LIGHT_CAPACITY", CLUSTER_LIGHT_CAPACITY),
            ("CLUSTER_STRIDE", CLUSTER_STRIDE),
            ("CLUSTER_OVERFLOW_WORD", CLUSTER_OVERFLOW_WORD),
        ] {
            let declaration = format!("static const uint {name} = {value};");
            for (file, source) in [("light_cluster.slang", SOURCE), ("mesh.slang", MESH_SOURCE)] {
                assert!(
                    source.contains(&declaration),
                    "{file} must declare `{declaration}`"
                );
            }
        }
        for (file, source) in [("light_cluster.slang", SOURCE), ("mesh.slang", MESH_SOURCE)] {
            let declaration =
                format!("static const uint CLUSTER_DEPTH_SLICES = {CLUSTER_DEPTH_SLICES};");
            assert!(
                source.contains(&declaration),
                "{file} must declare `{declaration}`"
            );
        }
        for (name, value) in [
            ("KIND_DIRECTIONAL", KIND_DIRECTIONAL),
            ("KIND_POINT", KIND_POINT),
            ("KIND_SPOT", KIND_SPOT),
        ] {
            let declaration = format!("static const uint {name} = {value};");
            for (file, source) in [("light_cluster.slang", SOURCE), ("mesh.slang", MESH_SOURCE)] {
                assert!(
                    source.contains(&declaration),
                    "{file} must declare `{declaration}`"
                );
            }
        }
    }

    /// [`SLICE_RATIO`] really is the split it claims, and walking it forward
    /// lands on the same boundaries [`slice_near`] computes.
    ///
    /// The two forms are the two halves of one grid: `light_cluster.slang`
    /// calls `pow`, and the volumetric pair multiplies. A constant transcribed
    /// with one digit wrong would compile, cut the frustum into slices that are
    /// still monotonic, and put the medium's boundaries somewhere the light
    /// grid's are not — which no image would report as an error.
    #[test]
    fn the_slice_ratio_is_the_split_it_claims() {
        let wanted = f64::from(CLUSTER_FAR / CLUSTER_NEAR)
            .powf(1.0 / f64::from(CLUSTER_DEPTH_SLICES)) as f32;
        assert_eq!(
            SLICE_RATIO, wanted,
            "SLICE_RATIO is not the ratio of the exponential split"
        );

        let mut walked = CLUSTER_NEAR;
        for index in 0..=CLUSTER_DEPTH_SLICES {
            let error = (walked - slice_near(index)).abs() / slice_near(index);
            assert!(
                error < 1e-5,
                "slice {index} starts at {walked} by multiplication and {} by power",
                slice_near(index)
            );
            walked *= SLICE_RATIO;
        }
    }

    /// `slice_near` and the `scale`/`bias` pair are inverses, so the froxel the
    /// clustering pass builds is the froxel the fragment stage looks itself up
    /// in.
    ///
    /// This is the one place the two halves of the depth split meet: the compute
    /// side walks slice boundaries forwards and the fragment side maps a depth
    /// back to an index, and nothing else in the engine would notice them
    /// drifting — a fragment reading its neighbour's list is a shading seam, not
    /// an error.
    #[test]
    fn the_slice_mapping_round_trips() {
        let scale = slice_scale();
        let bias = slice_bias();
        for index in 0..CLUSTER_DEPTH_SLICES {
            let near = slice_near(index);
            // Just inside the slice, because the boundary itself is where a
            // rounding difference is allowed to land either way.
            let inside = near * 1.000_01;
            let mapped = (inside.log2() * scale + bias).floor() as i32;
            assert_eq!(
                mapped, index as i32,
                "a depth just past slice {index}'s start ({near}) must map back to it"
            );
            assert_eq!(
                slice_of(inside, CLUSTER_DEPTH_SLICES),
                index,
                "and `slice_of` is the same mapping"
            );
        }
        assert!(
            (slice_near(0) - CLUSTER_NEAR).abs() < 1e-6,
            "slice 0 must start at CLUSTER_NEAR"
        );
        assert!(
            (slice_near(CLUSTER_DEPTH_SLICES) - CLUSTER_FAR).abs() < 1e-2,
            "the slice past the last must end at CLUSTER_FAR, got {}",
            slice_near(CLUSTER_DEPTH_SLICES)
        );
    }

    /// A depth nearer than the first slice and one past the last both clamp into
    /// the grid rather than indexing off it.
    #[test]
    fn a_depth_outside_the_grid_clamps_into_it() {
        assert_eq!(slice_of(CLUSTER_NEAR * 0.001, CLUSTER_DEPTH_SLICES), 0);
        assert_eq!(
            slice_of(CLUSTER_FAR * 1000.0, CLUSTER_DEPTH_SLICES),
            CLUSTER_DEPTH_SLICES - 1
        );
        // An orthographic frame has one slice, and every depth is in it.
        assert_eq!(slice_of(1.0, 1), 0);
        assert_eq!(slice_of(CLUSTER_FAR, 1), 0);
    }

    #[test]
    fn the_light_row_writes_its_fields_in_declaration_order() {
        let light = GpuLight {
            position: [1.0, 2.0, 3.0, 4.0],
            color: [5.0, 6.0, 7.0, 8.0],
            direction: [9.0, 10.0, 11.0, 12.0],
            kind: KIND_SPOT,
            cos_inner: 13.0,
            shadow_tile: NO_SHADOW_TILE,
            pad1: 0,
        };
        let bytes = light.to_bytes();
        let float_at = |offset: usize| {
            f32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        let word_at = |offset: usize| {
            u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        for slot in 0..12 {
            assert_eq!(
                float_at(slot * 4),
                (slot + 1) as f32,
                "the three float4s occupy the first 48 bytes, in order"
            );
        }
        assert_eq!(word_at(48), KIND_SPOT, "kind at offset 48");
        assert_eq!(float_at(52), 13.0, "cos_inner at offset 52");
        assert_eq!(word_at(56), NO_SHADOW_TILE, "shadow_tile at offset 56");
        assert_eq!(word_at(60), 0, "pad1 at offset 60");
    }

    #[test]
    fn the_cluster_params_block_writes_its_fields_in_declaration_order() {
        let params = ClusterParams {
            inverse_view_proj: [0.5; 16],
            eye: [1.5; 4],
            depth_row: [2.5; 4],
            grid_x: 7,
            grid_y: 11,
            slices: 13,
            light_count: 17,
            viewport_x: 19,
            viewport_y: 23,
            perspective: 1,
            tile_pixels: 29,
        };
        let bytes = params.to_bytes();
        let float_at = |offset: usize| {
            f32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        let word_at = |offset: usize| {
            u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        assert_eq!(CLUSTER_PARAMS_SIZE % 16, 0, "std140 rounds a block to 16");
        assert_eq!(float_at(0), 0.5, "inverse_view_proj at offset 0");
        assert_eq!(float_at(60), 0.5, "and it is sixteen floats long");
        assert_eq!(float_at(64), 1.5, "eye at offset 64");
        assert_eq!(float_at(80), 2.5, "depth_row at offset 80");
        assert_eq!(word_at(96), 7, "grid_x at offset 96");
        assert_eq!(word_at(100), 11, "grid_y at offset 100");
        assert_eq!(word_at(104), 13, "slices at offset 104");
        assert_eq!(word_at(108), 17, "light_count at offset 108");
        assert_eq!(word_at(112), 19, "viewport_x at offset 112");
        assert_eq!(word_at(116), 23, "viewport_y at offset 116");
        assert_eq!(word_at(120), 1, "perspective at offset 120");
        assert_eq!(word_at(124), 29, "tile_pixels at offset 124");
    }
}
