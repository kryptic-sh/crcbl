//! The uniform block, the two storage rows and the constants `water.slang`
//! declares, in the layouts that shader declares — and the guards over the
//! functions and blocks it copies from four other shaders.
//!
//! `docs/plan/55-water.md` rung 1's surface pass reads its own block, the
//! surface grid, one medium row per body, and three blocks that belong to other
//! passes: `mesh.slang`'s frame block for the camera, the ambient, the sun's
//! cascades and the analytic fog; `ssr.slang`'s for the probe volume and the sky
//! a reflection falls back to; and `volumetric_composite.slang`'s for the froxel
//! column. Each of those is bound as the very buffer its own pass reads, so the
//! struct in `water.slang` is a copy, and so is every function that reads one.
//!
//! # Every copy is compared
//!
//! There is no `#include` in these shaders, by design, and a copy nothing
//! compares is a drift with a schedule. So this module's tests hold every copied
//! block, constant and function body to the file it came from — see
//! [`tests::the_copied_functions_are_their_sources_bodies`] — and
//! [`tests::every_function_the_shader_shares_is_compared`] reads the shader for
//! function names another shader also defines, so a copy added later without a
//! comparison fails rather than going unguarded.
//!
//! `crate::fog`, `crate::volumetric`, `crate::probe` and `crate::probe_visibility`
//! each list `water.slang` beside the shaders they already held, so their own
//! comparisons against the host arithmetic cover this file too.
//!
//! [`tests::the_copied_functions_are_their_sources_bodies`]: self
//! [`tests::every_function_the_shader_shares_is_compared`]: self

/// Bytes of the uniform block: the sun's direction, its colour and the froxel
/// row, each sixteen bytes under `std140`.
pub const PARAMS_SIZE: usize = 16 + 16 + 16;

/// Bytes of one [`WaterVertex`]: a `float4` and a `uint4`.
pub const VERTEX_STRIDE: usize = 16 + 16;

/// Bytes of one [`WaterMedium`]: two `float4`s.
pub const MEDIUM_STRIDE: usize = 16 + 16;

/// Schlick's reflectance at normal incidence for an air–water interface,
/// matching `WATER_FRESNEL_F0` in `water.slang`: `((1.333 - 1) / (1.333 + 1))²`
/// is `0.0204`, and `0.02` is the value the models `docs/plan/55-water.md`
/// surveys use.
pub const FRESNEL_F0: f32 = 0.02;

/// The ratio of refractive indices a ray entering water from air bends by,
/// `1 / 1.333` to two places, matching `WATER_ETA` in `water.slang`.
pub const ETA: f32 = 0.75;

/// Metres of view ray under the surface at which the water is drawn whole,
/// matching `WATER_SHORE_FADE` in `water.slang`. Thinner water fades toward the
/// opaque frame behind it, which is the soft shoreline of
/// `docs/plan/55-water.md`'s rung 1.
pub const SHORE_FADE: f32 = 0.25;

/// Metres a ray under the surface is taken to travel when the frame holds
/// nothing behind it, matching `WATER_OPEN_THICKNESS` in `water.slang`.
pub const OPEN_THICKNESS: f32 = 1000.0;

/// The uniform block, matching `struct WaterParams` in `shaders/water.slang`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaterParams {
    /// The unit direction **towards** the sun in `xyz`; `w` is written as zero
    /// and unread.
    pub sun_direction: [f32; 4],
    /// The sun's colour in `rgb`; `w` is written as zero and unread.
    pub sun_color: [f32; 4],
    /// Whether this frame's froxel volume ran, so the air in front of the
    /// surface is read out of its column rather than evaluated from the frame
    /// block's fog.
    pub froxels: bool,
}

impl WaterParams {
    /// The block as the bytes a uniform buffer holds, little-endian.
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0;
        for value in self.sun_direction.into_iter().chain(self.sun_color) {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        // The froxel row: one word, and the three lanes of padding a
        // sixteen-byte row leaves, which stay the zeroes the array began as.
        bytes[at..at + 4].copy_from_slice(&u32::from(self.froxels).to_le_bytes());
        at += 16;
        debug_assert_eq!(at, PARAMS_SIZE, "three rows fill the block exactly");
        bytes
    }

    /// The block [`WaterParams::to_bytes`] wrote.
    ///
    /// Any non-zero froxel word reads as on, which is what the shader's `!= 0u`
    /// makes it.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; PARAMS_SIZE]) -> Self {
        let float = |at: usize| {
            f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        Self {
            sun_direction: [float(0), float(4), float(8), float(12)],
            sun_color: [float(16), float(20), float(24), float(28)],
            froxels: u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]) != 0,
        }
    }
}

/// One vertex of a surface grid, matching `struct WaterVertex` in
/// `shaders/water.slang`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaterVertex {
    /// World-space position, in metres.
    pub position: [f32; 3],
    /// Which row of the medium buffer the vertex's body is.
    pub body: u32,
}

impl WaterVertex {
    /// The row as the bytes a storage buffer holds, little-endian: the position
    /// with a zero `w`, then the body index with three zero lanes.
    #[must_use]
    pub fn to_bytes(self) -> [u8; VERTEX_STRIDE] {
        let mut bytes = [0u8; VERTEX_STRIDE];
        for (lane, value) in self.position.into_iter().enumerate() {
            bytes[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[16..20].copy_from_slice(&self.body.to_le_bytes());
        bytes
    }
}

/// One body's medium, matching `struct WaterMedium` in `shaders/water.slang`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaterMedium {
    /// Absorption per metre, per linear-RGB channel.
    pub absorption: [f32; 3],
    /// Scattering per metre, per linear-RGB channel.
    pub scattering: [f32; 3],
}

impl WaterMedium {
    /// The row as the bytes a storage buffer holds, little-endian: each
    /// coefficient triple with a zero `w`.
    #[must_use]
    pub fn to_bytes(self) -> [u8; MEDIUM_STRIDE] {
        let mut bytes = [0u8; MEDIUM_STRIDE];
        for (lane, value) in self.absorption.into_iter().enumerate() {
            bytes[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (lane, value) in self.scattering.into_iter().enumerate() {
            bytes[16 + lane * 4..16 + lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volumetric::tests::{one_declaration, one_function, shader_scalar};

    const WATER: &str = include_str!("../shaders/water.slang");
    const MESH: &str = include_str!("../shaders/mesh.slang");
    const SSR: &str = include_str!("../shaders/ssr.slang");
    const COMPOSITE: &str = include_str!("../shaders/volumetric_composite.slang");

    /// Every function `water.slang` copies, the file it copies it from, and the
    /// name that file gives the uniform block the body reads.
    ///
    /// The block name is the same on both sides for every row — the copy binds
    /// the very block its source does, under the same name — so a body that
    /// read a different block would differ in the comparison, not be excused by
    /// it.
    const COPIED_FUNCTIONS: &[(&str, &str, &str)] = &[
        ("mesh.slang", "frame.", "float fog_exp_neg(float x)"),
        (
            "mesh.slang",
            "frame.",
            "float fog_one_minus_exp_over(float d)",
        ),
        (
            "mesh.slang",
            "frame.",
            "float fog_optical_depth(float density, float falloff, float height_a, float height_b,",
        ),
        (
            "mesh.slang",
            "frame.",
            "float fog_transmittance(float optical_depth)",
        ),
        (
            "mesh.slang",
            "frame.",
            "float3 sky_irradiance(float3 normal)",
        ),
        (
            "mesh.slang",
            "frame.",
            "float shadow_normal_offset(float3 geometric_normal, float3 to_light)",
        ),
        (
            "mesh.slang",
            "frame.",
            "float2 shadow_rotation(float2 pixel)",
        ),
        (
            "mesh.slang",
            "frame.",
            "uint shadow_filter_mode(float2 pixel)",
        ),
        (
            "mesh.slang",
            "frame.",
            "float2 atlas_uv(float4 rect, float2 tile_uv)",
        ),
        ("mesh.slang", "frame.", "float4 atlas_rect(uint tile)"),
        ("mesh.slang", "frame.", "float2 atlas_step(float4 rect)"),
        ("mesh.slang", "frame.", "float tile_texels(float4 rect)"),
        (
            "mesh.slang",
            "frame.",
            "bool atlas_rect_is_empty(float4 rect)",
        ),
        (
            "mesh.slang",
            "frame.",
            "float tile_tap(float4 rect, float2 texel_step, float2 tile_uv, float2 spoke, \
             float2 rotation,",
        ),
        (
            "mesh.slang",
            "frame.",
            "float tile_pcf(uint tile, float2 tile_uv, float reference, float2 pixel, float radius)",
        ),
        (
            "mesh.slang",
            "frame.",
            "float tile_box_pcf(uint tile, float2 tile_uv, float reference)",
        ),
        (
            "mesh.slang",
            "frame.",
            "float sun_penumbra_texels(uint cascade, float2 tile_uv, float reference, \
             float2 rotation)",
        ),
        (
            "mesh.slang",
            "frame.",
            "float cascade_visibility(uint cascade, float3 world_position, float3 to_light,",
        ),
        (
            "mesh.slang",
            "frame.",
            "float sun_visibility(float3 world_position, float3 to_light, float n_dot_l,",
        ),
        (
            "volumetric_composite.slang",
            "params.",
            "float3 volumetric_unproject(float2 ndc, float depth)",
        ),
        (
            "volumetric_composite.slang",
            "params.",
            "float volumetric_phase(float g, float cos_theta)",
        ),
        (
            "volumetric_composite.slang",
            "params.",
            "float3 volumetric_source(float3 view_direction, float4 lit)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float2 decode_fixed_pair(float4 texel)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float2 fixed_pair_at(Texture2D<float4> table, float2 at)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float2 sky_prefilter_at(float up, float roughness)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float3 sky_prefiltered(float3 direction, float roughness)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float3 sky_view_at(float up, float azimuth_cosine)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float3 atmosphere_radiance(float3 direction)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float3 sky_environment(float3 direction, float roughness, float share)",
        ),
        ("ssr.slang", "camera.", "float sign_not_zero(float value)"),
        (
            "ssr.slang",
            "camera.",
            "float2 oct_encode(float3 direction)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float2 probe_moments(uint index, float3 direction)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float probe_chebyshev(uint index, float3 probe_position, float3 world_position, \
             float3 normal)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float probe_weight(uint index, float3 probe_position, float3 world_position, \
             float3 normal)",
        ),
        (
            "ssr.slang",
            "camera.",
            "uint probe_wrap(uint cell, uint offset, uint count)",
        ),
        (
            "ssr.slang",
            "camera.",
            "uint probe_row(uint level, uint3 cell)",
        ),
        (
            "ssr.slang",
            "camera.",
            "WeightedProbe lerp_probe(WeightedProbe a, WeightedProbe b, float t)",
        ),
        (
            "ssr.slang",
            "camera.",
            "WeightedProbe probe_corner(uint level, uint3 cell, float3 origin, float3 spacing,",
        ),
        (
            "ssr.slang",
            "camera.",
            "float probe_level_reach(float3 world_position, float3 origin, float3 inv_spacing, \
             float3 last)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float2 probe_level_of(float reach, uint levels)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float3 probe_level_environment(uint level, float3 world_position, float3 normal, \
             float3 direction)",
        ),
        (
            "ssr.slang",
            "camera.",
            "float3 probe_environment(float3 world_position, float3 normal, float3 direction)",
        ),
    ];

    /// Every `static const` `water.slang` copies, by its type and name, and the
    /// file it copies it from. The terminator is `};` for a table and `;` for a
    /// scalar.
    const COPIED_CONSTANTS: &[(&str, &str, &str)] = &[
        ("mesh.slang", "uint SHADOW_CASCADES", ";"),
        ("mesh.slang", "uint SHADOW_LIGHT_TILES", ";"),
        ("mesh.slang", "uint SHADOW_ATLAS_TILES", ";"),
        ("mesh.slang", "uint PROBE_LEVELS", ";"),
        ("ssr.slang", "float DEPTH_FAR", ";"),
        (
            "volumetric_composite.slang",
            "uint CLUSTER_DEPTH_SLICES",
            ";",
        ),
        ("volumetric_composite.slang", "float CLUSTER_NEAR", ";"),
        ("volumetric_composite.slang", "float CLUSTER_FAR", ";"),
        (
            "volumetric_composite.slang",
            "float CLUSTER_SLICE_RATIO",
            ";",
        ),
        ("mesh.slang", "float FOG_LOG2_E", ";"),
        ("mesh.slang", "float FOG_LN2_HI", ";"),
        ("mesh.slang", "float FOG_LN2_LO", ";"),
        ("mesh.slang", "float FOG_MAX_ARGUMENT", ";"),
        ("mesh.slang", "float FOG_MAX_OPTICAL_DEPTH", ";"),
        ("mesh.slang", "float FOG_SERIES_CUTOFF", ";"),
        ("mesh.slang", "float FOG_KERNEL", "};"),
        ("mesh.slang", "float FOG_RATIO_KERNEL", "};"),
        (
            "volumetric_composite.slang",
            "float VOLUMETRIC_INV_FOUR_PI",
            ";",
        ),
        (
            "volumetric_composite.slang",
            "float VOLUMETRIC_MAX_ANISOTROPY",
            ";",
        ),
        ("ssr.slang", "uint SKY_VIEW_WIDTH", ";"),
        ("ssr.slang", "uint SKY_VIEW_HEIGHT", ";"),
        ("ssr.slang", "float PROBE_TRANSFER_L0", ";"),
        ("ssr.slang", "float PROBE_TRANSFER_L1", ";"),
        ("ssr.slang", "float PROBE_LEVEL_BAND", ";"),
        ("ssr.slang", "float PROBE_VISIBILITY_SIDE", ";"),
        ("ssr.slang", "float PROBE_VISIBILITY_BORDER", ";"),
        ("ssr.slang", "float PROBE_SURFACE_BIAS", ";"),
        ("ssr.slang", "float PROBE_OCCLUDED_WEIGHT", ";"),
        ("mesh.slang", "uint SHADOW_TAPS", ";"),
        ("mesh.slang", "float SHADOW_FILTER_TEXELS", ";"),
        ("mesh.slang", "float2 SHADOW_DISC", "};"),
        ("mesh.slang", "uint SHADOW_PROBE_TAPS", ";"),
        ("mesh.slang", "uint SHADOW_PROBE_INDEX", "};"),
        ("mesh.slang", "float2 SHADOW_ROTATIONS", "};"),
        ("mesh.slang", "uint SHADOW_DITHER", "};"),
        ("mesh.slang", "float SHADOW_CASTER_REACH", ";"),
        ("mesh.slang", "float SHADOW_SUN_TAN_RADIUS", ";"),
        ("mesh.slang", "float SHADOW_SEARCH_TEXELS", ";"),
        ("mesh.slang", "uint SHADOW_SEARCH_TAPS", ";"),
        ("mesh.slang", "float2 SHADOW_SEARCH_DISC", "};"),
        ("mesh.slang", "uint SHADOW_FILTER_PCSS", ";"),
        ("mesh.slang", "uint SHADOW_FILTER_DISC", ";"),
        ("mesh.slang", "uint SHADOW_FILTER_BOX", ";"),
        ("mesh.slang", "float CASCADE_FADE_FRACTION", ";"),
        ("mesh.slang", "uint CASCADE_NONE", ";"),
    ];

    /// The source a table row names.
    fn source(file: &str) -> &'static str {
        match file {
            "mesh.slang" => MESH,
            "ssr.slang" => SSR,
            "volumetric_composite.slang" => COMPOSITE,
            other => panic!("no source named {other}"),
        }
    }

    /// A declaration's text with every comment line dropped and whitespace
    /// collapsed, from `opening` to the first `closing` after it.
    fn declaration(source: &str, opening: &str, closing: &str) -> String {
        let at = source
            .find(opening)
            .unwrap_or_else(|| panic!("no `{opening}` in this shader"));
        let rest = &source[at..];
        let end = rest
            .find(closing)
            .unwrap_or_else(|| panic!("`{opening}` never reaches `{closing}`"));
        rest[..end]
            .lines()
            .map(|line| line.split("//").next().unwrap_or(line))
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The lines of `source` from the first starting with `first` to the next
    /// ending with `last`, comments dropped and whitespace collapsed.
    fn lines_between(source: &str, first: &str, last: &str) -> String {
        let lines: Vec<&str> = source.lines().collect();
        let start = lines
            .iter()
            .position(|line| line.trim().starts_with(first))
            .unwrap_or_else(|| panic!("no line starts with `{first}`"));
        let end = start
            + lines[start..]
                .iter()
                .position(|line| line.trim().ends_with(last))
                .unwrap_or_else(|| panic!("no line after `{first}` ends with `{last}`"));
        lines[start..=end]
            .iter()
            .map(|line| line.split("//").next().unwrap_or(line))
            .flat_map(|line| line.split_whitespace())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// **The block's bytes are its fields in declaration order**, and each row
    /// lands where `std140` puts it.
    #[test]
    fn the_params_block_writes_its_fields_in_declaration_order() {
        let params = WaterParams {
            sun_direction: [0.5, 1.5, 2.5, 3.5],
            sun_color: [4.5, 5.5, 6.5, 7.5],
            froxels: true,
        };
        let bytes = params.to_bytes();
        let float_at =
            |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
        assert_eq!(float_at(0), 0.5);
        assert_eq!(float_at(12), 3.5);
        assert_eq!(float_at(16), 4.5);
        assert_eq!(float_at(28), 7.5);
        assert_eq!(
            u32::from_le_bytes(bytes[32..36].try_into().expect("four")),
            1
        );
        assert!(
            bytes[36..].iter().all(|byte| *byte == 0),
            "the row's padding"
        );
        assert_eq!(
            declaration(WATER, "struct WaterParams", "};"),
            "struct WaterParams { float4 sun_direction; float4 sun_color; uint4 froxels;",
            "the shader's block is not the three rows this module writes"
        );
    }

    /// Reading the block back gives the block that was written, both ways the
    /// froxel switch can be.
    #[test]
    fn the_params_block_reads_back_the_fields_it_wrote() {
        for froxels in [false, true] {
            let params = WaterParams {
                sun_direction: [0.25, -0.5, 0.75, 0.0],
                sun_color: [1.0, 2.0, 3.0, 0.0],
                froxels,
            };
            assert_eq!(WaterParams::from_bytes(&params.to_bytes()), params);
        }
    }

    /// **The two storage rows are the shader's structs, lane for lane.**
    #[test]
    fn the_vertex_and_medium_rows_are_the_shaders() {
        let vertex = WaterVertex {
            position: [1.0, 2.0, 3.0],
            body: 7,
        }
        .to_bytes();
        let float_at =
            |bytes: &[u8], at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().expect("4"));
        assert_eq!(
            [
                float_at(&vertex, 0),
                float_at(&vertex, 4),
                float_at(&vertex, 8)
            ],
            [1.0, 2.0, 3.0]
        );
        assert_eq!(float_at(&vertex, 12), 0.0);
        assert_eq!(u32::from_le_bytes(vertex[16..20].try_into().expect("4")), 7);
        assert_eq!(
            declaration(WATER, "struct WaterVertex", "};"),
            "struct WaterVertex { float4 position; uint4 body;"
        );

        let medium = WaterMedium {
            absorption: [0.1, 0.2, 0.3],
            scattering: [0.4, 0.5, 0.6],
        }
        .to_bytes();
        assert_eq!(float_at(&medium, 8), 0.3);
        assert_eq!(float_at(&medium, 16), 0.4);
        assert_eq!(float_at(&medium, 24), 0.6);
        assert_eq!(float_at(&medium, 28), 0.0);
        assert_eq!(
            declaration(WATER, "struct WaterMedium", "};"),
            "struct WaterMedium { float4 absorption; float4 scattering;"
        );
    }

    /// **This module's constants are the shader's**, compared as values.
    #[test]
    fn the_shader_spells_this_module_s_constants() {
        for (value, name) in [
            (FRESNEL_F0, "WATER_FRESNEL_F0"),
            (ETA, "WATER_ETA"),
            (SHORE_FADE, "WATER_SHORE_FADE"),
            (OPEN_THICKNESS, "WATER_OPEN_THICKNESS"),
        ] {
            assert_eq!(
                shader_scalar(WATER, name),
                value,
                "water.slang's {name} is not this module's"
            );
        }
    }

    /// **The three foreign blocks are their passes' own, field for field.**
    ///
    /// Each is bound as the buffer another pass is handed, so a copy with one
    /// field moved reads every field after it at another field's offset — a
    /// fog colour where an eye should be, which draws rather than failing.
    /// Compared as text with comments dropped, on
    /// `crate::volumetric`'s `the_two_shaders_declare_one_block` terms, and that
    /// test holds `VolumetricParams` for this file as it does for the composite.
    #[test]
    fn the_borrowed_blocks_are_their_passes_own() {
        for (file, name) in [
            ("mesh.slang", "FrameUniforms"),
            ("ssr.slang", "SsrParams"),
            ("ssr.slang", "GpuProbe"),
            ("ssr.slang", "WeightedProbe"),
        ] {
            let opening = format!("\nstruct {name}\n");
            assert_eq!(
                declaration(source(file), &opening, "\n};"),
                declaration(WATER, &opening, "\n};"),
                "water.slang's `struct {name}` is not {file}'s"
            );
        }
    }

    /// **Every copied constant is its source's declaration, digit for digit.**
    #[test]
    fn the_copied_constants_are_their_sources_declarations() {
        for (file, name, terminator) in COPIED_CONSTANTS {
            assert_eq!(
                one_declaration(source(file), name, terminator),
                one_declaration(WATER, name, terminator),
                "`static const {name}` has drifted between {file} and water.slang"
            );
        }
    }

    /// **Every copied function is its source's body**, with comments dropped
    /// and whitespace collapsed — `crate::volumetric`'s
    /// `both_shaders_spell_the_same_atlas_walk` comparison, over the sun's
    /// cascade walk, the reflection environment and the fog.
    #[test]
    fn the_copied_functions_are_their_sources_bodies() {
        for (file, block, signature) in COPIED_FUNCTIONS {
            assert_eq!(
                one_function(source(file), signature, block),
                one_function(WATER, signature, block),
                "`{signature}` has drifted between {file} and water.slang"
            );
        }
    }

    /// **The froxel lookup is the composite's**, line for line.
    ///
    /// `volumetric_composite.slang` does it inline in its fragment stage rather
    /// than in a function a body comparison could name, so the three stretches
    /// `water_froxel_air` copies are compared as runs of lines: the grid and the
    /// pixel's coordinates, the slice walk down to the froxel index, and the
    /// prefix read and the partial slice. The two lines between them that differ
    /// — where the depth comes from and what an out-of-range froxel returns —
    /// are the whole of what the copy changed.
    #[test]
    fn the_froxel_lookup_is_the_composite_s() {
        for (first, last) in [
            (
                "uint grid_x = max(params.grid_x, 1u);",
                "1.0 - (float(pixel.y) + 0.5) / viewport.y * 2.0);",
            ),
            (
                "float view_depth = CLUSTER_FAR;",
                "uint froxel = tile_x + tile_y * grid_x + slice * tiles;",
            ),
            (
                "float4 prefix = volumetrics[froxel];",
                "volumetric_source(view_direction, lighting[froxel]) * (1.0 - partial_survives);",
            ),
        ] {
            assert_eq!(
                lines_between(COMPOSITE, first, last),
                lines_between(WATER, first, last),
                "the froxel lookup from `{first}` to `{last}` has drifted between \
                 volumetric_composite.slang and water.slang"
            );
        }
        // And the composite's own sum, split into the two halves the surface
        // applies to the light it adds.
        assert!(COMPOSITE.contains(
            "float3 lit = scene.rgb * (prefix.a * partial_survives) + prefix.rgb\n               \
             + prefix.a * partial_radiance;"
        ));
        assert!(WATER.contains("air.survives = prefix.a * partial_survives;"));
        assert!(WATER.contains("air.inscatter = prefix.rgb + prefix.a * partial_radiance;"));
    }

    /// **The analytic fog is the forward pass's**, line for line, with the
    /// surface point where `mesh.slang` has its fragment's.
    #[test]
    fn the_analytic_fog_is_the_forward_pass_s() {
        let mesh = lines_between(
            MESH,
            "float3 fog_offset = frame.camera_position.xyz - input.world_position;",
            "float fog_survives = fog_transmittance(fog_depth);",
        );
        let water = lines_between(
            WATER,
            "float3 fog_offset = frame.camera_position.xyz - surface;",
            "float fog_survives = fog_transmittance(fog_depth);",
        );
        assert_eq!(mesh.replace("input.world_position", "surface"), water);
        assert!(WATER.contains("air.inscatter = frame.fog_color.rgb * (1.0 - fog_survives);"));
    }

    /// The name of every function `source` defines at the start of a line.
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
                    // A resource declaration, whose parenthesis is its D3D12
                    // register rather than a parameter list.
                    && !line.contains("D3D12_REGISTER(")
                    && line.contains('(')
            })
            .filter_map(|line| {
                let name = line.split('(').next()?.split_whitespace().last()?;
                Some(name.to_owned())
            })
            .collect()
    }

    /// **Every function `water.slang` shares a name with is compared.**
    ///
    /// [`COPIED_FUNCTIONS`] is a hand-written list, and a hand-written list is
    /// what a later copy is not added to. So this reads the shader for every
    /// function it defines that `mesh.slang`, `ssr.slang`,
    /// `volumetric_composite.slang` or `volumetric.slang` also defines, and
    /// requires each to be on the list — `crate::fog`'s
    /// `every_shader_that_spells_the_exponential_is_guarded` over this file's
    /// copies. The entry points are the one pair every shader shares a name
    /// for and none copies.
    #[test]
    fn every_function_the_shader_shares_is_compared() {
        let volumetric = include_str!("../shaders/volumetric.slang");
        let listed: Vec<&str> = COPIED_FUNCTIONS
            .iter()
            .map(|(_, _, signature)| {
                signature
                    .split('(')
                    .next()
                    .and_then(|head| head.split_whitespace().last())
                    .expect("a signature names its function")
            })
            .collect();
        let elsewhere: Vec<String> = [MESH, SSR, COMPOSITE, volumetric]
            .into_iter()
            .flat_map(defined_functions)
            .collect();
        let shared: Vec<String> = defined_functions(WATER)
            .into_iter()
            .filter(|name| name != "vertexMain" && name != "fragmentMain")
            .filter(|name| elsewhere.contains(name))
            .collect();
        assert!(
            shared.len() >= listed.len(),
            "the scan found {} shared functions and the list holds {}, so the scan is not \
             seeing the copies",
            shared.len(),
            listed.len()
        );
        for name in shared {
            assert!(
                listed.contains(&name.as_str()),
                "water.slang defines `{name}`, which another shader also defines, and no \
                 comparison here holds the two together"
            );
        }
    }
}
