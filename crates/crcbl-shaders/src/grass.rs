//! The blocks, the rows, the constants and the hash `grass_gen.slang` and
//! `grass.slang` declare, in the layouts those shaders declare — and the guards
//! over the code they copy.
//!
//! Same reason as [`crate::water`] and [`crate::wind`]: a shader fixes numbers
//! and a byte layout, every producer of those has to agree with it exactly, and
//! keeping both in the crate that owns the source means there is one place to
//! change rather than one per consumer.
//!
//! # The hash is here, not in the renderer
//!
//! [`hash`] and [`unit_pair`] are the arithmetic `grass_gen.slang`'s placement
//! is built out of, and `crcbl_render::grass::placement` answers
//! `docs/plan/57-grass.md`'s decision 2 — "is this position in grass, and how
//! tall" — with the same two functions. They are integer operations throughout,
//! so the two copies are the same bits on every target; that is the whole of
//! the determinism argument, and it only holds while the two are one piece of
//! code. This is that one piece.
//!
//! # What the guards hold
//!
//! There is no `#include` in this tree, so both shaders carry copies.
//! `docs/plan/51-volumetrics.md`'s arrangement is what they are held under:
//!
//! * [`tests::the_wind_sampler_is_the_field_s`] — `grass_gen.slang` against
//!   `wind.slang`, body for body, so the field a blade leans in is the field
//!   physics reads.
//! * [`tests::the_copied_functions_are_their_sources_bodies`] and
//!   [`tests::the_copied_constants_are_their_sources_declarations`] —
//!   `grass.slang` against `mesh.slang`, over the sun's cascade walk and the
//!   punctual lights'.
//! * [`tests::every_function_the_shader_shares_is_compared`] — the scan that
//!   stops a later copy from being added without a comparison.
//! * [`tests::the_two_shaders_declare_one_instance_row`] and its neighbours —
//!   the rows and the tile block the two grass shaders both declare.
//!
//! [`tests::the_wind_sampler_is_the_field_s`]: self
//! [`tests::the_copied_functions_are_their_sources_bodies`]: self
//! [`tests::the_copied_constants_are_their_sources_declarations`]: self
//! [`tests::every_function_the_shader_shares_is_compared`]: self
//! [`tests::the_two_shaders_declare_one_instance_row`]: self

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` on both of
/// `grass_gen.slang`'s entry points.
///
/// A generation dispatch is `cells².div_ceil(WORKGROUP_SIZE)` groups and the
/// clear is `slots.div_ceil(WORKGROUP_SIZE)`; both shaders discard the
/// invocations past their own bound that the last group brings.
pub const WORKGROUP_SIZE: u32 = 64;

/// Words in one non-indexed draw's argument structure: vertex count, instance
/// count, first vertex, first instance.
pub const DRAW_ARG_WORDS: u32 = 4;

/// Bytes of one slot's draw arguments.
pub const DRAW_ARGS_SIZE: usize = DRAW_ARG_WORDS as usize * 4;

/// Vertices one card instance draws: two crossed quads of
/// [`CARD_QUAD_VERTICES`].
pub const CARD_VERTICES: u32 = 12;

/// Vertices in one of those quads — two triangles.
pub const CARD_QUAD_VERTICES: u32 = 6;

/// The alpha a card's coverage must reach to draw.
///
/// **The cook's number as much as the shader's**: `crcbl_render::grass::card`
/// bisects each mip's scale so the fraction of its texels reaching exactly this
/// is the fraction the top level's did, so the two must be one number.
pub const ALPHA_CUTOFF: f32 = 0.5;

/// How much of its row's colour a blade may lose to its own tint lane.
pub const TINT_RANGE: f32 = 0.18;

/// Texels along one side of the card's top level.
///
/// **A number the shader fixes**, because `grass.slang` picks the chain's level
/// itself rather than leaving it to the hardware — see `grassCardLevel` there —
/// and the ratio it picks by is this over the card's width in pixels.
/// `crcbl_render::grass::card` cooks a chain of exactly this extent, and
/// re-exports this rather than declaring a second one.
pub const CARD_EXTENT: u32 = 64;

/// Levels in the card's chain, down to a single texel.
pub const CARD_LEVELS: u32 = CARD_EXTENT.ilog2() + 1;

/// The most of its own height a blade's tip is moved by the wind.
pub const MAX_BEND: f32 = 0.6;

/// The wind speed, in m/s, at which a blade has given half of [`MAX_BEND`].
pub const BEND_HALF_SPEED: f32 = 6.0;

/// The lane the density test is drawn from, exclusive-ored into a cell.
pub const DENSITY_SALT: u32 = 0x9e37_79b9;

/// The lane a blade's facing is drawn from.
pub const FACING_SALT: u32 = 0x85eb_ca6b;

/// The lane a blade's height and width are drawn from.
pub const SIZE_SALT: u32 = 0xc2b2_ae35;

/// The lane a blade's tint is drawn from.
pub const TINT_SALT: u32 = 0x27d4_eb2f;

/// Bytes of [`GenParams`]: five sixteen-byte `std140` rows.
pub const GEN_PARAMS_SIZE: usize = 16 * 5;

/// Bytes of [`Params`]: two sixteen-byte `std140` rows.
pub const PARAMS_SIZE: usize = 16 * 2;

/// Bytes of [`Tile`]: two sixteen-byte `std140` rows.
pub const TILE_SIZE: usize = 16 * 2;

/// Bytes of one [`GrassBlade`] row.
pub const BLADE_STRIDE: usize = 16 * 3;

/// Bytes of one [`GrassInstance`] row.
pub const INSTANCE_STRIDE: usize = 16 * 5;

/// Jarzynski and Olano's `pcg` hash (*Hash Functions for GPU Rendering*,
/// JCGT 9(3), 2020): PCG's 32-bit LCG step followed by its RXS-M-XS output
/// permutation. `grass_hash` in `grass_gen.slang` is the same five lines.
///
/// **Integer throughout, and that is the point.** A `u32` multiply wraps the
/// same way in Rust and in every target Slang compiles to, so a cell's lanes
/// are the same bits on the CPU and on the GPU — which is what lets
/// `crcbl_render::grass::placement` answer "is this position in grass" with the
/// set the shader actually generated rather than with an approximation of it.
#[must_use]
pub const fn hash(value: u32) -> u32 {
    let state = value.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// The bottom sixteen bits of `lane` as a number in `0..1`, and the top sixteen
/// as a second one. `grass_unit_pair` in `grass_gen.slang`.
///
/// Sixteen bits rather than the whole word because `f32` holds every integer
/// below `2²⁴` exactly and not one above it, and a jitter that rounded would
/// put two cells at one position on one target and not on another.
#[must_use]
pub fn unit_pair(lane: u32) -> [f32; 2] {
    [
        (lane & 0xffff) as f32 * (1.0 / 65536.0),
        ((lane >> 16) & 0xffff) as f32 * (1.0 / 65536.0),
    ]
}

/// An eight-bit unorm texel back as the integer it holds — `grass_unorm8` in
/// `grass_gen.slang`, over the byte the CPU copy already has.
///
/// Here so the two copies name one conversion. On the GPU the texel arrives as
/// `n / 255` and the rounding is what recovers `n`; on the CPU `n` is the byte
/// itself, and this is the identity. What both must agree about is that the
/// comparison downstream is between integers.
#[must_use]
pub const fn unorm8(texel: u8) -> u32 {
    texel as u32
}

/// Whether the cell `cell` carries a blade, given its cover texel's density.
///
/// `grass_gen.slang`'s density test, and it is integer arithmetic on both
/// sides: the texel is a count out of 255 and the lane is one out of 65536, so
/// `255 · 257 == 65535` is what makes a full-density texel keep every cell but
/// one in 65536 and a zero-density texel keep none of them — both exactly, on
/// every target.
#[must_use]
pub const fn accepts(cell: u32, density: u8) -> bool {
    (hash(cell ^ DENSITY_SALT) & 0xffff) < unorm8(density) * 257
}

/// The generation pass's uniform block, matching `struct GrassGenParams` in
/// `shaders/grass_gen.slang`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GenParams {
    /// The world-space camera, and in `[3]` how far from it a blade is still
    /// generated, in metres.
    pub camera: [f32; 4],
    /// `xy` the world XZ of the ground field's first texel centre, `z` metres
    /// of world per texel and `w` one over that.
    pub ground: [f32; 4],
    /// The same four numbers for the cover map.
    pub cover: [f32; 4],
    /// `x` texels across the ground field, `y` down it, `z` across the cover
    /// map, `w` down it.
    pub maps: [u32; 4],
    /// `x` instances one slot holds, `y` rows in the blade table, `z` cells
    /// along one side of a tile, `w` how many slots there are.
    pub limits: [u32; 4],
}

impl GenParams {
    /// The block as the bytes a uniform buffer holds, little-endian.
    #[must_use]
    pub fn to_bytes(self) -> [u8; GEN_PARAMS_SIZE] {
        let mut bytes = [0u8; GEN_PARAMS_SIZE];
        let mut at = 0;
        for row in [self.camera, self.ground, self.cover] {
            for value in row {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
        }
        for row in [self.maps, self.limits] {
            for value in row {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
        }
        bytes
    }

    /// The block read back out of the bytes [`Self::to_bytes`] writes — here so
    /// the round trip is a test rather than a claim.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; GEN_PARAMS_SIZE]) -> Self {
        let float = |index: usize| float_at(bytes, index);
        let word = |index: usize| word_at(bytes, index);
        Self {
            camera: core::array::from_fn(&float),
            ground: core::array::from_fn(|lane| float(4 + lane)),
            cover: core::array::from_fn(|lane| float(8 + lane)),
            maps: core::array::from_fn(|lane| word(12 + lane)),
            limits: core::array::from_fn(|lane| word(16 + lane)),
        }
    }
}

/// The grass pass's uniform block, matching `struct GrassParams` in
/// `shaders/grass.slang`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Params {
    /// `x` instances one slot holds, `y` rows in the blade table, `zw` written
    /// as zero and unread.
    pub limits: [u32; 4],
    /// `x` how many pixels a metre spans at one unit of view depth — half the
    /// viewport's width times the projection's own `x` scale. `yzw` are written
    /// as zero and unread.
    ///
    /// **This is what makes the card's level a property of the card**: the
    /// vertex stage turns a blade's width in metres into its width in pixels
    /// with it, and `grassCardLevel` reads the chain at the level that width
    /// asks for. See that function for why the hardware's own choice is not
    /// good enough here.
    pub screen: [f32; 4],
}

impl Params {
    /// The block as the bytes a uniform buffer holds, little-endian.
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0;
        for value in self.limits {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in self.screen {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes
    }
}

/// One tile's block, matching `struct GrassTile` in both grass shaders.
///
/// Bound at a dynamic offset, one row of a ring per tile: that is how "one
/// dispatch per tile, one draw per slot" reaches a shader without a push
/// constant, which a browser's WGSL has none of.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tile {
    /// `xy` the world XZ of the tile's minimum corner, `z` the tile's side in
    /// metres, `w` written as zero and unread.
    pub tile: [f32; 4],
    /// `x` the tile's slot, `y` its index along `+X`, `z` its index along
    /// `+Z`, `w` written as zero and unread.
    pub slot: [u32; 4],
}

impl Tile {
    /// The block as the bytes a uniform buffer holds, little-endian.
    #[must_use]
    pub fn to_bytes(self) -> [u8; TILE_SIZE] {
        let mut bytes = [0u8; TILE_SIZE];
        let mut at = 0;
        for value in self.tile {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        for value in self.slot {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes
    }
}

/// One row of the blade table, matching `struct GrassBlade` in both grass
/// shaders.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GrassBlade {
    /// The colour at the blade's root in `rgb`; `a` is written as zero.
    pub root_color: [f32; 4],
    /// The colour at its tip in `rgb`; `a` is written as zero.
    pub tip_color: [f32; 4],
    /// `x` height in metres, `y` half-width in metres, `z` the fraction of the
    /// height a blade may lose to its own hash, `w` the fraction of the
    /// half-width it may lose.
    pub size: [f32; 4],
}

impl GrassBlade {
    /// The row as the bytes a storage buffer holds, little-endian.
    #[must_use]
    pub fn to_bytes(self) -> [u8; BLADE_STRIDE] {
        let mut bytes = [0u8; BLADE_STRIDE];
        let mut at = 0;
        for row in [self.root_color, self.tip_color, self.size] {
            for value in row {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
        }
        bytes
    }
}

/// One generated blade, matching `struct GrassInstance` in both grass shaders.
///
/// Read back by `crates/crcbl/tests/render_e2e/grass.rs`, which is where the
/// determinism claim is measured, so [`GrassInstance::from_bytes`] is a reader
/// with a real caller rather than a round-trip helper.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GrassInstance {
    /// The world-space root in `xyz` and the blade's height in metres in `w`.
    pub root: [f32; 4],
    /// The unit facing on the XZ plane in `xy`, the half-width in `z` and the
    /// blade's own tint lane in `w`.
    pub facing: [f32; 4],
    /// The metres the tip is moved by the wind, in world space.
    pub lean: [f32; 4],
    /// The ground's unit normal under the root in `xyz`; `w` is zero.
    pub ground: [f32; 4],
    /// `x` the blade's cell in the field, `y` its blade row, `zw` zero.
    pub lanes: [u32; 4],
}

impl GrassInstance {
    /// The row read out of the bytes a generation dispatch wrote,
    /// little-endian.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; INSTANCE_STRIDE]) -> Self {
        let float = |index: usize| float_at(bytes, index);
        Self {
            root: core::array::from_fn(&float),
            facing: core::array::from_fn(|lane| float(4 + lane)),
            lean: core::array::from_fn(|lane| float(8 + lane)),
            ground: core::array::from_fn(|lane| float(12 + lane)),
            lanes: core::array::from_fn(|lane| word_at(bytes, 16 + lane)),
        }
    }

    /// The row as the bytes a storage buffer holds, little-endian.
    #[must_use]
    pub fn to_bytes(self) -> [u8; INSTANCE_STRIDE] {
        let mut bytes = [0u8; INSTANCE_STRIDE];
        let mut at = 0;
        for row in [self.root, self.facing, self.lean, self.ground] {
            for value in row {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
        }
        for value in self.lanes {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes
    }
}

/// Word `index` of `bytes` as a little-endian `f32`.
fn float_at(bytes: &[u8], index: usize) -> f32 {
    let at = index * 4;
    f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Word `index` of `bytes` as a little-endian `u32`.
fn word_at(bytes: &[u8], index: usize) -> u32 {
    let at = index * 4;
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volumetric::tests::{one_declaration, one_function, shader_scalar};

    /// The grass pass's source, hash-pinned by `spirv/manifest.txt`.
    const GRASS: &str = include_str!("../shaders/grass.slang");

    /// The generation pass's source, likewise.
    const GEN: &str = include_str!("../shaders/grass_gen.slang");

    /// The forward pass, which `grass.slang` copies its lighting from.
    const MESH: &str = include_str!("../shaders/mesh.slang");

    /// The wind field, which `grass_gen.slang` copies its sampler from.
    const WIND: &str = include_str!("../shaders/wind.slang");

    /// Every `static const` `grass.slang` copies from `mesh.slang`, by its type
    /// and name. The terminator is `};` for a table and `;` for a scalar.
    const COPIED_CONSTANTS: &[(&str, &str)] = &[
        ("uint SHADOW_CASCADES", ";"),
        ("uint SHADOW_LIGHT_TILES", ";"),
        ("uint SHADOW_POINT_FACES", ";"),
        ("uint SHADOW_ATLAS_TILES", ";"),
        ("uint PROBE_LEVELS", ";"),
        ("uint KIND_DIRECTIONAL", ";"),
        ("uint KIND_POINT", ";"),
        ("uint KIND_SPOT", ";"),
        ("uint KIND_RECT", ";"),
        ("uint CLUSTER_LIGHT_CAPACITY", ";"),
        ("uint CLUSTER_STRIDE", ";"),
        ("uint CLUSTER_DEPTH_SLICES", ";"),
        ("float CLUSTER_NEAR", ";"),
        ("float CLUSTER_FAR", ";"),
        ("float PUNCTUAL_DEPTH_BIAS_TEXELS", ";"),
        ("float PUNCTUAL_NORMAL_OFFSET_TEXELS", ";"),
        ("uint SHADOW_TAPS", ";"),
        ("float SHADOW_FILTER_TEXELS", ";"),
        ("float2 SHADOW_DISC", "};"),
        ("uint SHADOW_PROBE_TAPS", ";"),
        ("uint SHADOW_PROBE_INDEX", "};"),
        ("float2 SHADOW_ROTATIONS", "};"),
        ("uint SHADOW_DITHER", "};"),
        ("float SHADOW_CASTER_REACH", ";"),
        ("float SHADOW_SUN_TAN_RADIUS", ";"),
        ("float SHADOW_SEARCH_TEXELS", ";"),
        ("uint SHADOW_SEARCH_TAPS", ";"),
        ("float2 SHADOW_SEARCH_DISC", "};"),
        ("uint SHADOW_FILTER_PCSS", ";"),
        ("uint SHADOW_FILTER_DISC", ";"),
        ("uint SHADOW_FILTER_BOX", ";"),
        ("float CASCADE_FADE_FRACTION", ";"),
        ("uint CASCADE_NONE", ";"),
    ];

    /// Every function `grass.slang` copies from `mesh.slang`, by the signature
    /// that opens it. Both files name the frame block `frame`, so the
    /// substitution `one_function` makes is the identity here.
    const COPIED_FUNCTIONS: &[&str] = &[
        "uint light_tile(uint tile)",
        "float shadow_normal_offset(float3 geometric_normal, float3 to_light)",
        "float2 shadow_rotation(float2 pixel)",
        "uint shadow_filter_mode(float2 pixel)",
        "float2 atlas_uv(float4 rect, float2 tile_uv)",
        "float4 atlas_rect(uint tile)",
        "float2 atlas_step(float4 rect)",
        "float tile_texels(float4 rect)",
        "bool atlas_rect_is_empty(float4 rect)",
        "float tile_tap(float4 rect, float2 texel_step, float2 tile_uv, float2 spoke, \
         float2 rotation,",
        "float tile_pcf(uint tile, float2 tile_uv, float reference, float2 pixel, float radius)",
        "float tile_box_pcf(uint tile, float2 tile_uv, float reference)",
        "float sun_penumbra_texels(uint cascade, float2 tile_uv, float reference, \
         float2 rotation)",
        "float cascade_visibility(uint cascade, float3 world_position, float3 to_light,",
        "float sun_visibility(float3 world_position, float3 to_light, float n_dot_l,",
        "float punctual_visibility(uint tile, float3 world_position, float3 to_light, \
         float n_dot_l,",
        "float spot_visibility(GpuLight light, uint tile, float3 world_position, float3 to_light,",
        "uint point_face(float3 from_light)",
        "float point_visibility(GpuLight light, uint base, float3 world_position, \
         float3 to_light,",
        "float range_window(float distance, float radius)",
        "float punctual_falloff(float distance, float radius)",
        "float spot_cone(float3 to_light, float3 axis, float cos_outer, float cos_inner)",
        "uint froxel_of(float2 pixel, float depth)",
    ];

    /// The text of a `struct` declaration with comments dropped and whitespace
    /// collapsed — `crate::water`'s `declaration`, over the one shape both
    /// grass shaders spell.
    fn declaration(source: &str, name: &str) -> String {
        let opening = format!("\nstruct {name}\n");
        let at = source
            .find(&opening)
            .unwrap_or_else(|| panic!("no `struct {name}` in this shader"));
        let rest = &source[at..];
        let end = rest.find("\n};").expect("the block ends");
        rest[..end]
            .lines()
            .map(|line| line.split("//").next().unwrap_or(line))
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_workgroup_size_matches_the_numthreads_both_entry_points_declare() {
        let spelled = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        assert_eq!(
            GEN.matches(spelled.as_str()).count(),
            2,
            "grass_gen.slang does not declare `{spelled}` on both of its entry points; \
             WORKGROUP_SIZE has drifted"
        );
    }

    /// **This module's numbers are the shaders', compared as values.**
    ///
    /// A literal is parsed rather than matched as text for
    /// `crate::fog::shader_scalar`'s reason: `0.5` and `.5` are one number and
    /// two spellings, and a spelling comparison would also pass on a file that
    /// merely contained the right digits somewhere.
    #[test]
    fn the_shaders_spell_this_module_s_constants() {
        for (value, name, source) in [
            (ALPHA_CUTOFF, "GRASS_ALPHA_CUTOFF", GRASS),
            (TINT_RANGE, "GRASS_TINT_RANGE", GRASS),
            (MAX_BEND, "GRASS_MAX_BEND", GEN),
            (BEND_HALF_SPEED, "GRASS_BEND_HALF_SPEED", GEN),
        ] {
            assert_eq!(
                shader_scalar(source, name),
                value,
                "the shader's {name} is not this module's"
            );
        }
        for (value, name, source) in [
            (CARD_VERTICES, "GRASS_CARD_VERTICES", GRASS),
            (CARD_QUAD_VERTICES, "GRASS_CARD_QUAD_VERTICES", GRASS),
            (CARD_VERTICES, "GRASS_CARD_VERTICES", GEN),
            (CARD_EXTENT, "GRASS_CARD_EXTENT", GRASS),
            (CARD_LEVELS, "GRASS_CARD_LEVELS", GRASS),
            (DRAW_ARG_WORDS, "GRASS_DRAW_ARG_WORDS", GEN),
            (WORKGROUP_SIZE, "GRASS_WORKGROUP_SIZE", GEN),
        ] {
            let spelled = format!("static const uint {name} = {value};");
            assert!(
                source.contains(&spelled),
                "the shader does not declare `{spelled}`"
            );
        }
        for (value, name) in [
            (DENSITY_SALT, "GRASS_DENSITY_SALT"),
            (FACING_SALT, "GRASS_FACING_SALT"),
            (SIZE_SALT, "GRASS_SIZE_SALT"),
            (TINT_SALT, "GRASS_TINT_SALT"),
        ] {
            let spelled = format!("static const uint {name} = {value:#010x}u;");
            assert!(
                GEN.contains(&spelled),
                "grass_gen.slang does not declare `{spelled}`; a salt that drifted moves every \
                 blade in the field"
            );
        }
    }

    /// **The hash is PCG's, constant for constant.**
    ///
    /// Nothing a rendered frame does would catch a multiplier one digit out:
    /// the field would still be a field, and every golden blessed after the
    /// slip would agree with it. What the shader has to spell is this module's
    /// arithmetic, token for token.
    #[test]
    fn the_shader_spells_the_same_hash() {
        for line in [
            "uint state = value * 747796405u + 2891336453u;",
            "uint word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;",
            "return (word >> 22u) ^ word;",
        ] {
            assert!(
                GEN.contains(line),
                "grass_gen.slang does not spell `{line}`, so its hash is not `crcbl_shaders::\
                 grass::hash`"
            );
        }
    }

    /// The hash's own properties, over a sweep wide enough for them to mean
    /// something: distinct inputs give distinct outputs, and the outputs fill
    /// the range evenly.
    ///
    /// **This is not a check that the transcription is PCG's** — that is the
    /// test above, and the citation in [`hash`]'s own documentation. It is a
    /// check that the function is usable as a placement hash at all: a
    /// transcription slip that produced a constant, or a value correlated with
    /// its input's low bits, would draw every blade of a tile in one corner and
    /// would pass a byte-for-byte CPU-against-GPU comparison while doing it.
    #[test]
    fn the_hash_scatters_its_input() {
        const CELLS: u32 = 1 << 16;
        let mut seen = std::collections::HashSet::with_capacity(CELLS as usize);
        let mut buckets = [0usize; 16];
        for cell in 0..CELLS {
            let lane = hash(cell);
            seen.insert(lane);
            buckets[(lane >> 28) as usize] += 1;
        }
        assert_eq!(
            seen.len(),
            CELLS as usize,
            "the hash collided over {CELLS} consecutive cells"
        );
        let want = CELLS as usize / buckets.len();
        let (low, high) = buckets.iter().fold((usize::MAX, 0), |(low, high), count| {
            (low.min(*count), high.max(*count))
        });
        eprintln!("grass hash: {CELLS} cells, top-nibble buckets {low}..={high} against {want}");
        assert!(
            low * 4 > want * 3 && high * 3 < want * 4,
            "the top nibble is not even over {CELLS} cells: {low}..={high} against {want}"
        );
    }

    /// **`unit_pair` is exact, and the jitter it is is inside the unit
    /// square.**
    ///
    /// The exactness is the claim: every value it can return is a multiple of
    /// `2⁻¹⁶` an `f32` holds without rounding, so a cell lands at the same
    /// position on the CPU and on the GPU.
    #[test]
    fn the_unit_pair_is_exact_and_in_range() {
        for lane in (0..u32::MAX / 977).map(|step| step.wrapping_mul(977)) {
            let [low, high] = unit_pair(lane);
            for value in [low, high] {
                assert!(
                    (0.0..1.0).contains(&value),
                    "unit_pair({lane}) gave {value}"
                );
                let scaled = value * 65536.0;
                assert_eq!(scaled, scaled.floor(), "{value} is not a multiple of 2^-16");
            }
            assert_eq!(low * 65536.0, (lane & 0xffff) as f32);
            assert_eq!(high * 65536.0, ((lane >> 16) & 0xffff) as f32);
        }
    }

    /// **The density test is a probability, and its two ends are exact.**
    ///
    /// A zero-density texel carries nothing anywhere, and a full-density one
    /// carries all but one cell in 65536 — which is the whole of what
    /// "a denser tile draws more instances" rests on.
    #[test]
    fn the_density_test_runs_from_none_to_all_but_one() {
        for cell in 0..20_000u32 {
            assert!(!accepts(cell, 0), "a calm texel carried cell {cell}");
        }
        let full = (0..65_536u32).filter(|cell| accepts(*cell, 255)).count();
        eprintln!("grass density: {full} of 65536 cells at full density");
        assert!(
            full >= 65_500,
            "a full-density texel kept only {full} of 65536 cells"
        );
        let half = (0..65_536u32).filter(|cell| accepts(*cell, 128)).count();
        eprintln!("grass density: {half} of 65536 cells at half density");
        assert!(
            (32_000..33_800).contains(&half),
            "a half-density texel kept {half} of 65536 cells, which is not half of them"
        );
    }

    /// **Every block's bytes are its fields in declaration order**, and each
    /// row lands where `std140` puts it.
    #[test]
    fn the_blocks_write_their_fields_in_declaration_order() {
        let params = GenParams {
            camera: [1.0, 2.0, 3.0, 4.0],
            ground: [5.0, 6.0, 7.0, 8.0],
            cover: [9.0, 10.0, 11.0, 12.0],
            maps: [13, 14, 15, 16],
            limits: [17, 18, 19, 20],
        };
        let bytes = params.to_bytes();
        assert_eq!(bytes.len(), GEN_PARAMS_SIZE);
        assert_eq!(float_at(&bytes, 0), 1.0);
        assert_eq!(float_at(&bytes, 11), 12.0);
        assert_eq!(word_at(&bytes, 12), 13);
        assert_eq!(word_at(&bytes, 19), 20);
        assert_eq!(GenParams::from_bytes(&bytes), params);
        assert_eq!(
            declaration(GEN, "GrassGenParams"),
            "struct GrassGenParams { float4 camera; float4 ground; float4 cover; uint4 maps; \
             uint4 limits;"
        );

        let tile = Tile {
            tile: [1.5, 2.5, 3.5, 0.0],
            slot: [7, 1, 2, 0],
        };
        let bytes = tile.to_bytes();
        assert_eq!(bytes.len(), TILE_SIZE);
        assert_eq!(float_at(&bytes, 1), 2.5);
        assert_eq!(word_at(&bytes, 4), 7);
        assert_eq!(
            declaration(GEN, "GrassTile"),
            "struct GrassTile { float4 tile; uint4 slot;"
        );

        let bytes = Params {
            limits: [2, 3, 0, 0],
            screen: [128.5, 0.0, 0.0, 0.0],
        }
        .to_bytes();
        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(word_at(&bytes, 0), 2);
        assert_eq!(word_at(&bytes, 1), 3);
        assert_eq!(float_at(&bytes, 4), 128.5);
        assert_eq!(
            declaration(GRASS, "GrassParams"),
            "struct GrassParams { uint4 limits; float4 screen;"
        );
    }

    /// **The two rows are the shaders' structs, lane for lane.**
    #[test]
    fn the_rows_are_the_shaders_own() {
        let blade = GrassBlade {
            root_color: [0.1, 0.2, 0.3, 0.0],
            tip_color: [0.4, 0.5, 0.6, 0.0],
            size: [0.7, 0.8, 0.9, 1.0],
        }
        .to_bytes();
        assert_eq!(blade.len(), BLADE_STRIDE);
        assert_eq!(float_at(&blade, 4), 0.4);
        assert_eq!(float_at(&blade, 11), 1.0);

        let instance = GrassInstance {
            root: [1.0, 2.0, 3.0, 4.0],
            facing: [5.0, 6.0, 7.0, 8.0],
            lean: [9.0, 10.0, 11.0, 0.0],
            ground: [0.0, 1.0, 0.0, 0.0],
            lanes: [42, 1, 0, 0],
        };
        let bytes = instance.to_bytes();
        assert_eq!(bytes.len(), INSTANCE_STRIDE);
        assert_eq!(float_at(&bytes, 3), 4.0);
        assert_eq!(word_at(&bytes, 16), 42);
        assert_eq!(GrassInstance::from_bytes(&bytes), instance);
    }

    /// **The two grass shaders declare one instance row**, field for field.
    ///
    /// One writes it and the other reads it, so a field moved in one of them
    /// reads every field after it at another field's offset — a height where a
    /// tint should be, which draws rather than failing.
    #[test]
    fn the_two_shaders_declare_one_instance_row() {
        assert_eq!(
            declaration(GEN, "GrassInstance"),
            declaration(GRASS, "GrassInstance")
        );
    }

    /// The same for the blade table, which both shaders index.
    #[test]
    fn the_two_shaders_declare_one_blade_row() {
        assert_eq!(
            declaration(GEN, "GrassBlade"),
            declaration(GRASS, "GrassBlade")
        );
    }

    /// The same for the tile block, which is bound to both at the same dynamic
    /// offset.
    #[test]
    fn the_two_shaders_declare_one_tile_block() {
        assert_eq!(
            declaration(GEN, "GrassTile"),
            declaration(GRASS, "GrassTile")
        );
    }

    /// **The wind a blade leans in is `wind.slang`'s field**, block, constant
    /// and body.
    ///
    /// `docs/plan/56-wind.md`'s whole premise is that "a gust that bends the
    /// grass, lifts a character's hair, ruffles a lake and pushes a crate is
    /// one gust", and that only holds while every consumer samples the same
    /// field with the same formula. `grass_gen.slang` is the first consumer, so
    /// this is the first copy — and the copy is the thing that can drift.
    #[test]
    fn the_wind_sampler_is_the_field_s() {
        assert_eq!(
            declaration(WIND, "WindParams"),
            declaration(GEN, "WindParams"),
            "grass_gen.slang's `struct WindParams` is not wind.slang's"
        );
        assert_eq!(
            one_declaration(WIND, "float WIND_MIN_DIRECTION_LENGTH_SQUARED", ";"),
            one_declaration(GEN, "float WIND_MIN_DIRECTION_LENGTH_SQUARED", ";"),
        );
        for signature in [
            "float windSmoothTriangle(float u)",
            "float3 windSample(float3 posRel)",
        ] {
            assert_eq!(
                one_function(WIND, signature, "wind."),
                one_function(GEN, signature, "wind."),
                "`{signature}` has drifted between wind.slang and grass_gen.slang"
            );
        }
    }

    /// **The two borrowed blocks are the forward pass's own, field for
    /// field.**
    #[test]
    fn the_borrowed_blocks_are_the_forward_pass_s() {
        for name in ["FrameUniforms", "GpuLight"] {
            assert_eq!(
                declaration(MESH, name),
                declaration(GRASS, name),
                "grass.slang's `struct {name}` is not mesh.slang's"
            );
        }
    }

    /// **Every copied constant is its source's declaration, digit for digit.**
    #[test]
    fn the_copied_constants_are_their_sources_declarations() {
        for (name, terminator) in COPIED_CONSTANTS {
            assert_eq!(
                one_declaration(MESH, name, terminator),
                one_declaration(GRASS, name, terminator),
                "`static const {name}` has drifted between mesh.slang and grass.slang"
            );
        }
    }

    /// **Every copied function is its source's body**, with comments dropped
    /// and whitespace collapsed — `crate::water`'s comparison, over the sun's
    /// cascade walk and the punctual lights'.
    #[test]
    fn the_copied_functions_are_their_sources_bodies() {
        for signature in COPIED_FUNCTIONS {
            assert_eq!(
                one_function(MESH, signature, "frame."),
                one_function(GRASS, signature, "frame."),
                "`{signature}` has drifted between mesh.slang and grass.slang"
            );
        }
    }

    /// The name of every function `source` defines at the start of a line —
    /// `crate::water`'s `defined_functions`.
    fn defined_functions(source: &str) -> Vec<String> {
        source
            .lines()
            .filter(|line| {
                !line.starts_with(' ')
                    && !line.starts_with('/')
                    && !line.starts_with('[')
                    && !line.starts_with('#')
                    && !line.starts_with("static")
                    && !line.starts_with("struct")
                    && line.contains('(')
            })
            .filter_map(|line| {
                let name = line.split('(').next()?.split_whitespace().last()?;
                Some(name.to_owned())
            })
            .collect()
    }

    /// **Every function a grass shader shares a name with is compared.**
    ///
    /// [`COPIED_FUNCTIONS`] and the wind list are hand-written, and a
    /// hand-written list is what a later copy is not added to. So this reads
    /// both shaders for every function they define that `mesh.slang` or
    /// `wind.slang` also defines, and requires each to be on a list —
    /// `crate::water`'s `every_function_the_shader_shares_is_compared` over
    /// this pair. The entry points are the names a shader shares and never
    /// copies.
    #[test]
    fn every_function_the_shader_shares_is_compared() {
        let listed: Vec<&str> = COPIED_FUNCTIONS
            .iter()
            .map(|signature| {
                signature
                    .split('(')
                    .next()
                    .and_then(|head| head.split_whitespace().last())
                    .expect("a signature names its function")
            })
            .chain(["windSmoothTriangle", "windSample"])
            .collect();
        let elsewhere: Vec<String> = [MESH, WIND]
            .into_iter()
            .flat_map(defined_functions)
            .collect();
        let entry_points = ["vertexMain", "fragmentMain", "clearMain", "generateMain"];
        let shared: Vec<String> = [GRASS, GEN]
            .into_iter()
            .flat_map(defined_functions)
            .filter(|name| !entry_points.contains(&name.as_str()))
            .filter(|name| elsewhere.contains(name))
            .collect();
        assert!(
            shared.len() >= listed.len(),
            "the scan found {} shared functions and the lists hold {}, so the scan is not seeing \
             the copies",
            shared.len(),
            listed.len()
        );
        for name in shared {
            assert!(
                listed.contains(&name.as_str()),
                "a grass shader defines `{name}`, which another shader also defines, and no \
                 comparison here holds the two together"
            );
        }
    }

    /// **Neither shader spends a storage texture**, which is decision 1's
    /// WebGPU constraint — a browser guarantees four per stage and the forward
    /// path already wants them.
    ///
    /// The ground field and the cover map are `Texture2D` read with `Load`, and
    /// the card is sampled; the buffers a dispatch writes are storage buffers,
    /// which is a different budget.
    #[test]
    fn no_grass_shader_declares_a_storage_texture() {
        for (name, source) in [("grass.slang", GRASS), ("grass_gen.slang", GEN)] {
            assert!(
                !source.contains("RWTexture"),
                "{name} declares a storage texture; decision 1 spends none"
            );
        }
    }

    /// **The placement reads its two maps with `Load` and never samples
    /// them.**
    ///
    /// The determinism argument rests on it: a hardware bilinear filter has
    /// eight bits of sub-texel precision by specification, so a density
    /// threshold taken off a sampled texel is a threshold the CPU copy cannot
    /// be held to. The wind's own two taps are the only `SampleLevel` in the
    /// file, and they feed the lean rather than the placement.
    #[test]
    fn the_placement_maps_are_loaded_and_not_sampled() {
        assert_eq!(
            GEN.matches(".Load(").count(),
            2,
            "grass_gen.slang does not read exactly its two placement maps with `Load`"
        );
        assert_eq!(
            GEN.matches(".SampleLevel(").count(),
            2,
            "grass_gen.slang samples something other than the wind's two layers"
        );
        assert!(
            !GEN.contains("grassCover.Sample(") && !GEN.contains("grassGround.Sample("),
            "grass_gen.slang samples a placement map, so the CPU copy cannot match it"
        );
    }
}
