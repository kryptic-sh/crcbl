//! The uniform block and the two constants `ssr.slang` declares, in the layouts
//! that shader declares.
//!
//! Same reason as [`crate::ssao`]: the shader fixes a byte layout and a pair of
//! values, every producer of those has to agree with it exactly, and keeping
//! both in the crate that owns the source means there is one place to change
//! rather than one per consumer.
//!
//! This module also carries the guard that holds the **shared screen-space
//! helpers** together — see
//! [`tests::the_shared_screen_space_helpers_have_not_drifted`]. `ssr.slang`
//! copies `depth_at`, `unproject`, `view_position` and `normal_at` out of
//! `ssao.slang` verbatim and `ssr_blur.slang` copies `depth_at`, `unproject`
//! and `view_z`, because this repo has no include mechanism by design; two
//! copies is already the bug, and four without a guard is a drift with a
//! schedule.
//!
//! [`tests::the_shared_screen_space_helpers_have_not_drifted`]: self

/// Bytes of the uniform block: three `float4x4`, two `float4`, and two `uint4`.
///
/// `std140` gives each row sixteen-byte alignment, so the three matrices, the
/// probe-volume header and the pyramid row fill the block without tail padding.
pub const PARAMS_SIZE: usize = 64 + 64 + 64 + crate::probe::PROBE_VOLUME_SIZE + 16 + 48;

/// The roughness at which SSR's sharpness ramp reaches zero, matching
/// `static const float ROUGHNESS_CUTOFF` in `ssr.slang` — the one shader that
/// declares it.
///
/// `mesh.slang` stores the lobe's roughness in the `Rgba8Unorm` attachment,
/// quantised to [`REFLECTIVITY_LEVELS`] so the store has no rounding tie, and
/// `ssr.slang` derives the ramp from the reload with its `sharpness_of`;
/// [`stored_roughness`] is the quantisation on the CPU, and the tests below pin
/// that [`crate::mesh::GpuMaterial::UNTINTED`] reloads above the cutoff.
///
/// Half, so [`crate::mesh::GpuMaterial::UNTINTED`]'s roughness lands on the zero
/// end exactly. Public because a *sample* has to distinguish materials the
/// screen-space march can see from rough conductors that receive only probe
/// fallback — see `lantern`'s room.
///
/// **The blur widens the lobe a single ray can stand for and this number has
/// deliberately not moved with it.** Raising it past `lantern`'s brass block at
/// 0.55 would take `UNTINTED` in as well, because no monotone ramp passes 0.55
/// and stops at 0.5 — and that costs the unconditional claim the assertion below
/// makes. It is a change of blast radius rather than of filtering, and
/// `docs/backlog.md` carries it as the slice that owns the decision.
pub const ROUGHNESS_CUTOFF: f32 = 0.5;

/// `UNTINTED` is the row an instance written by omission shades through, and it
/// must skip the screen-space march exactly.
///
/// A zero probe volume then contributes exact zero, preserving the old frame;
/// authored probes are deliberately not covered by that property because even
/// rough geometry now receives their environment fallback. A compile-time
/// assertion rather than a test because both sides are constants, and this way a
/// build that lowered either one never links.
const _: () = assert!(
    crate::mesh::GpuMaterial::UNTINTED.roughness >= ROUGHNESS_CUTOFF,
    "GpuMaterial::UNTINTED's roughness is under ssr.slang's cutoff, so every surface nobody \
     gave a material to would reflect"
);

/// The levels an `Rgba8Unorm` channel has above zero, matching
/// `static const float REFLECTIVITY_LEVELS` in `mesh.slang`, which quantises
/// the reflectivity attachment's alpha to them before the store.
///
/// The rounding happens in the shader because a raw store of `0.5 * 255` is a
/// tie the output merger resolves as it likes — one backend would reload
/// `UNTINTED` as 127 and another as 128, and only one of those derives a zero
/// ramp. A level handed over exactly leaves the merger nothing to decide.
pub const REFLECTIVITY_LEVELS: f32 = 255.0;

/// What the reflectivity attachment holds where nothing reflects, matching
/// `static const float4 NO_REFLECTION` in `mesh.slang`: no `F0` and a fully
/// rough surface, so `ssr.slang` marches nothing from the pixel and reads no
/// mirror in it. `crcbl_render::forward` clears the attachment to it and
/// `mesh.slang`'s debug views write it.
pub const NO_REFLECTION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// The roughness the reflectivity attachment reloads for a stored `roughness`:
/// `mesh.slang`'s rounding to [`REFLECTIVITY_LEVELS`], on the CPU, so a test can
/// say what the attachment holds without re-deriving the shader's arithmetic.
#[must_use]
pub fn stored_roughness(roughness: f32) -> f32 {
    (roughness * REFLECTIVITY_LEVELS + 0.5).floor() / REFLECTIVITY_LEVELS
}

/// The uniform block, matching `struct SsrParams` in `shaders/ssr.slang`.
///
/// Both matrices are **column-major**, the order `glam::Mat4::to_cols_array`
/// produces and the order every other block in this crate is written in — see
/// [`crate::ssao::SsaoParams`], whose first two fields these are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SsrParams {
    /// Clip → view: the inverse of the camera's projection alone, **not** of its
    /// view-projection. The march is a question about what a surface can see
    /// from where it is, and view space is where the eye is at the origin and
    /// the depth buffer unprojects without a second matrix.
    pub inv_proj: [f32; 16],
    /// View → clip, for projecting the reflected ray onto the screen. Clip space
    /// is affine in the ray parameter, which is the whole of what lets the
    /// segment be clipped once and interpolated per step.
    pub proj: [f32; 16],
    /// View → world, so the screen-space origin and reflection direction can
    /// evaluate the world-space probe grid.
    pub inv_view: [f32; 16],
    /// The probe grid header, matching [`crate::probe::ProbeVolume`].
    pub probe_volume: crate::probe::ProbeVolume,
    /// How many levels of the Hi-Z pyramid this frame built, which is the
    /// deepest level the march may climb to.
    ///
    /// Zero is a valid frame: an extent too small to halve once gets no pyramid,
    /// and the march walks the full-resolution prepass a texel at a time. The
    /// shader clamps this to its own `MAX_HIZ_LEVEL` — `crcbl_render::hiz`'s
    /// `MAX_LEVELS` is the Rust mirror of that number, and this field is what
    /// the frame actually has.
    pub hiz_levels: u32,
    /// The gradient sky a ray that hit nothing sees, one row per band —
    /// zenith, horizon, ground, each a linear RGB radiance in `xyz` with `w`
    /// unread padding.
    ///
    /// **The gradient itself and not its L1 projection**, which is the one
    /// place this block and `mesh::FrameUniforms` deliberately disagree about
    /// how to carry the same sky. The ambient term wants the cosine-weighted
    /// integral of the environment and L1 *is* that integral; a reflection
    /// wants the radiance along one direction, and reconstructing that from
    /// four irradiance coefficients would blur a gradient this side can
    /// evaluate exactly for three multiplies. [`crate::sky::SkyGradient`] owns
    /// the bands; `ssr.slang`'s `sky_prefiltered` weights them by
    /// [`crate::sky_prefilter`]'s table at the surface's roughness, so a mirror
    /// reads the gradient itself and a rough lobe the cone it gathers.
    ///
    /// **Zero is exactly off**: a black sky adds nothing to the probe
    /// environment a missed ray already fell back to, so this rung arrived
    /// switched off like the ambient half did.
    ///
    /// **Last in the block**, so no existing member's offset moves.
    pub sky: [[f32; 4]; 3],
}

impl SsrParams {
    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian throughout. There is no padding to write: the matrices and
    /// probe volume fill [`PARAMS_SIZE`] exactly.
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0;
        for value in self
            .inv_proj
            .into_iter()
            .chain(self.proj)
            .chain(self.inv_view)
        {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes[at..at + crate::probe::PROBE_VOLUME_SIZE]
            .copy_from_slice(&self.probe_volume.to_bytes());
        at += crate::probe::PROBE_VOLUME_SIZE;
        // The pyramid row: the level count, and three lanes of the padding a
        // sixteen-byte row leaves. They stay zero, which is what the array was
        // initialised to.
        bytes[at..at + 4].copy_from_slice(&self.hiz_levels.to_le_bytes());
        at += 16;
        for row in self.sky {
            for value in row {
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                at += 4;
            }
        }
        debug_assert_eq!(
            at, PARAMS_SIZE,
            "the matrices, probe volume, pyramid row and sky fill the block exactly"
        );
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every shader source that carries a copy of the screen-space helpers.
    ///
    /// Adding another screen-space pass means adding it here; a pass that
    /// copied a helper and was not listed is a copy this guard does not hold,
    /// which is the state the guard exists to end.
    const SOURCES: [(&str, &str); 5] = [
        ("ssao.slang", include_str!("../shaders/ssao.slang")),
        (
            "ssao_blur.slang",
            include_str!("../shaders/ssao_blur.slang"),
        ),
        ("ssr.slang", include_str!("../shaders/ssr.slang")),
        ("ssr_blur.slang", include_str!("../shaders/ssr_blur.slang")),
        (
            "contact_shadows.slang",
            include_str!("../shaders/contact_shadows.slang"),
        ),
    ];

    /// The body of the function named `signature` in `source`, brace to brace.
    ///
    /// [`None`] when that file has no such function, which is how a helper only
    /// some of the sources carry is skipped rather than reported as a
    /// difference.
    fn body_of(source: &str, signature: &str) -> Option<String> {
        let at = source.find(signature)?;
        let open = source[at..].find('{')? + at;
        let mut depth = 0usize;
        for (offset, byte) in source[open..].bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(source[open..open + offset + 1].to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// **The copies must be identical, character for character.**
    ///
    /// `ssr.slang` re-declares `depth_at`, `unproject`, `view_position` and
    /// `normal_at`, `ssr_blur.slang` re-declares `depth_at`, `unproject` and
    /// `view_z`, and
    /// `contact_shadows.slang` re-declares those three and the march's own five
    /// beside them, because the manifest hashes one source per artifact and an
    /// `#include` would be a file whose edits nothing downstream notices. Nothing else in the tree would
    /// notice one copy being fixed and the others left: the shaders compile
    /// either way, and the failure is a reflection sampling a pixel the
    /// occlusion pass would have called a different one.
    ///
    /// The bodies compare rather than the whole declarations, because the doc
    /// comments above each are allowed to say what that file uses it for — and
    /// they do.
    #[test]
    fn the_shared_screen_space_helpers_have_not_drifted() {
        for signature in [
            "float depth_at(int2 pixel, int2 extent)",
            // The two every reconstruction below goes through, and the widest
            // copies here: all five of `SOURCES` carry both, because all five
            // unproject something and the full form is written in terms of the
            // depth-only one.
            "float2 unproject_z(float depth)",
            "float4 unproject(float2 ndc, float depth)",
            "float view_z(int2 pixel, float depth, float2 extent)",
            "float3 view_position(int2 pixel, float depth, float2 extent)",
            "float3 normal_at(int2 pixel, float3 centre, int2 extent, float2 size)",
            // The march's own five, shared by `ssr.slang` and
            // `contact_shadows.slang` since the contact rung landed. They were
            // one file's alone until then, which is exactly the state this
            // guard exists to catch on the way past.
            "float2 pixel_of(float2 ndc, float2 size)",
            "float2 ndc_of(float2 at, float2 size)",
            "float thickness_at(float advance, float depth)",
            "float view_z_of(float depth)",
            "float cell_exit(float2 at, float2 forward, float size, float reach)",
        ] {
            let copies: Vec<(&str, String)> = SOURCES
                .iter()
                .filter_map(|(name, source)| Some((*name, body_of(source, signature)?)))
                .collect();
            assert!(
                copies.len() > 1,
                "`{signature}` was found in {} of the screen-space shaders, so this guard is \
                 holding nothing together — either the signature moved or `SOURCES` is stale",
                copies.len()
            );
            let (first_name, first) = &copies[0];
            for (name, body) in &copies[1..] {
                assert_eq!(
                    body, first,
                    "`{signature}` differs between {first_name} and {name}; the screen-space \
                     helpers are copied verbatim and one copy has drifted"
                );
            }
        }
    }

    /// The far plane every one of these shaders names is one value, and it is
    /// [`crate::ssao::DEPTH_FAR`].
    ///
    /// The reflection pair declares its own rather than reaching for the
    /// occlusion pass's, so this is what says all of them are the same number.
    /// The shaders compile either way and a mismatch shows up only as a march
    /// that starts at the sky or a blur that divides by zero.
    #[test]
    fn the_far_plane_matches_the_constant_the_reflection_pair_declares() {
        let declaration = format!(
            "static const float DEPTH_FAR = {:.1};",
            crate::ssao::DEPTH_FAR
        );
        for (name, source) in SOURCES {
            assert!(
                source.contains(&declaration),
                "{name} does not declare `{declaration}`; the far planes have drifted"
            );
        }
    }

    /// The cutoff's shader copy and Rust mirror must name the same roughness,
    /// and `ssr.slang` must be the only shader that has one.
    ///
    /// `mesh.slang` used to carry a second copy to encode the ramp with; now it
    /// stores roughness and the ramp is `ssr.slang`'s alone, so a copy
    /// reappearing there is a drift waiting to happen.
    #[test]
    fn the_roughness_cutoff_is_ssr_slangs_alone() {
        let declaration = format!("static const float ROUGHNESS_CUTOFF = {ROUGHNESS_CUTOFF:.1};");
        assert!(
            include_str!("../shaders/ssr.slang").contains(&declaration),
            "ssr.slang does not declare `{declaration}`; ROUGHNESS_CUTOFF has drifted"
        );
        assert!(
            !include_str!("../shaders/mesh.slang").contains("ROUGHNESS_CUTOFF"),
            "mesh.slang names ROUGHNESS_CUTOFF again; the ramp belongs to ssr.slang"
        );
    }

    /// The attachment's quantisation and its empty value are spelled the same in
    /// `mesh.slang` and here.
    #[test]
    fn the_reflectivity_attachment_constants_match_mesh_slang() {
        let source = include_str!("../shaders/mesh.slang");
        let levels = format!("static const float REFLECTIVITY_LEVELS = {REFLECTIVITY_LEVELS:.1};");
        assert!(
            source.contains(&levels),
            "mesh.slang does not declare `{levels}`; REFLECTIVITY_LEVELS has drifted"
        );
        let [r, g, b, a] = NO_REFLECTION;
        let empty =
            format!("static const float4 NO_REFLECTION = float4({r:.1}, {g:.1}, {b:.1}, {a:.1});");
        assert!(
            source.contains(&empty),
            "mesh.slang does not declare `{empty}`; NO_REFLECTION has drifted"
        );
    }

    /// `UNTINTED`'s roughness sits on the cutoff, which a raw store would round
    /// either way; the shader's rounding lands it on the level above, so the
    /// ramp `ssr.slang` derives is exactly zero.
    #[test]
    fn untinted_reloads_above_the_cutoff() {
        assert!(stored_roughness(crate::mesh::GpuMaterial::UNTINTED.roughness) >= ROUGHNESS_CUTOFF);
        assert_eq!(stored_roughness(0.0), 0.0, "a mirror is level zero");
        assert_eq!(stored_roughness(1.0), 1.0, "fully rough is the top level");
        assert_eq!(
            stored_roughness(0.25),
            64.0 / REFLECTIVITY_LEVELS,
            "the probe scene's conductor rounds its tie up"
        );
    }

    /// The shader and Rust mirror undo the same per-band irradiance transfer.
    #[test]
    fn the_probe_transfer_constants_match_ssr_slang() {
        let source = include_str!("../shaders/ssr.slang");
        for (name, expected) in [
            ("PROBE_TRANSFER_L0", crate::probe::TRANSFER_L0),
            ("PROBE_TRANSFER_L1", crate::probe::TRANSFER_L1),
        ] {
            let declaration = format!("static const float {name} = ");
            let at = source
                .find(&declaration)
                .unwrap_or_else(|| panic!("ssr.slang does not declare `{name}`"));
            let rest = &source[at + declaration.len()..];
            let end = rest
                .find(';')
                .unwrap_or_else(|| panic!("ssr.slang's `{name}` declaration has no semicolon"));
            let actual: f32 = rest[..end]
                .parse()
                .unwrap_or_else(|error| panic!("ssr.slang's `{name}` is not a float: {error}"));
            assert_eq!(
                actual, expected,
                "ssr.slang decodes a probe with {name}={actual}, but GpuProbe::radiance uses \
                 {expected}"
            );
        }
    }

    /// A zero probe volume must leave a hit's old `hit_color * env_brdf * confidence`
    /// arithmetic byte-for-byte intact; only the miss share is added.
    #[test]
    fn zero_environment_keeps_the_hit_multiplication_order() {
        let source = include_str!("../shaders/ssr.slang");
        assert!(
            source.contains(
                "reflection = hit_color * env_brdf * confidence\n                    + environment * env_brdf * (1.0 - confidence);"
            ),
            "ssr.slang must retain hit_color * env_brdf * confidence before adding the probe fallback"
        );

        let hit = 0.700_000_05f32;
        let env_brdf = 0.300_000_04f32;
        let confidence = 0.900_000_04f32;
        let old = hit * env_brdf * confidence;
        let with_zero_environment =
            hit * env_brdf * confidence + 0.0f32 * env_brdf * (1.0 - confidence);
        assert_eq!(
            with_zero_environment.to_bits(),
            old.to_bits(),
            "adding a zero probe fallback must not round a valid SSR hit differently"
        );
    }

    /// Rough geometry has no screen-space ray, but still carries its authored
    /// environment and the composite must not send that value through the
    /// sharpness kernel whose denominator is zero.
    #[test]
    fn rough_surfaces_skip_the_march_and_composite_their_probe_fallback() {
        let march = include_str!("../shaders/ssr.slang");
        assert!(
            march.contains(
                "if (sharpness <= 0.0)\n    {\n        return float4(environment * env_brdf, 0.0);\n    }"
            ),
            "rough surfaces must return their probe fallback with zero sharpness before march setup"
        );
        let blur = include_str!("../shaders/ssr_blur.slang");
        assert!(
            blur.contains(
                "if (sharpness <= 0.0)\n    {\n        return float4(lit.rgb + centre.rgb, lit.a);\n    }"
            ),
            "the blur must add a zero-sharpness probe fallback directly instead of filtering it"
        );

        let lit = 0.125f32;
        let environment = 0.6f32;
        let env_brdf = 0.8f32;
        let fallback = environment * env_brdf;
        assert_eq!(lit + fallback, 0.605);
        assert_eq!(lit + 0.0f32 * env_brdf, lit);
    }

    /// Positive sharpness blends continuously from the direct centre fallback to
    /// the fully filtered reflection. The square-root curve retains enough
    /// filtering at the middle of the stored linear ramp to remove fixed-stride
    /// march steps without changing the exact zero fallback.
    #[test]
    fn the_blur_filters_partially_rough_surfaces_without_discontinuity() {
        let blur = include_str!("../shaders/ssr_blur.slang");
        assert!(
            blur.contains(
                "float3 filtered = total / weight;\n    float filter_share = sqrt(sharpness);\n    return float4(lit.rgb + lerp(centre.rgb, filtered, filter_share), lit.a);"
            ),
            "ssr_blur.slang must use the continuous square-root filter share"
        );

        let centre = 2.0f32;
        let filtered = 10.0f32;
        let near_zero_sharpness = 1.0e-8f32;
        let near_zero = centre + (filtered - centre) * near_zero_sharpness.sqrt();
        assert!(
            (near_zero - centre).abs() < 0.001,
            "a nearly rough surface must approach the zero-sharpness centre: {near_zero}"
        );

        let middle = 0.5f32.sqrt();
        assert!(
            middle > 0.5 && middle < 1.0,
            "a partially rough surface must retain more filtering than the linear ramp: {middle}"
        );
        assert_eq!(centre + (filtered - centre) * 1.0f32.sqrt(), filtered);
    }

    /// files must name the same number for it.
    ///
    /// `ssr_blur.slang` has no ray, so the only length the march has that it can
    /// still evaluate is `THICKNESS_FLOOR` — the least thickness a surface is
    /// credited with, as a share of view depth. It re-declares it for this
    /// repo's no-`#include` reason, and a drift would leave the blur filtering
    /// over a length the march does not use with nothing to say so: the picture
    /// would simply be a little more or less smeared.
    ///
    /// `contact_shadows.slang` is the third copy, and it is a march again rather
    /// than a filter: the number decides what its own crossing is allowed to
    /// call a surface. **Every copy is compared, not the first two**, so a
    /// fourth screen-space pass added to [`SOURCES`] is held to the same number
    /// without this test being edited — the shape the count-of-two here used to
    /// prevent.
    #[test]
    fn the_thickness_floor_matches_the_one_the_march_declares() {
        let declaration = "static const float THICKNESS_FLOOR = ";
        let mut copies = Vec::new();
        for (name, source) in SOURCES {
            let Some(at) = source.find(declaration) else {
                continue;
            };
            let rest = &source[at + declaration.len()..];
            let end = rest.find(';').expect("the declaration ends in a semicolon");
            copies.push((name, &rest[..end]));
        }
        assert!(
            copies.len() > 1,
            "`THICKNESS_FLOOR` is declared in {copies:?}; a guard over one copy is holding \
             nothing together — either the declaration moved or `SOURCES` is stale"
        );
        let (first_name, first) = copies[0];
        for (name, floor) in &copies[1..] {
            assert_eq!(
                floor, &first,
                "{first_name} and {name} declare different thickness floors; every march and \
                 filter in this family credits a surface with the same least thickness"
            );
        }
    }

    /// The block the shader declares, member for member and in this order.
    #[test]
    fn the_uniform_block_matches_the_struct_ssr_slang_declares() {
        let source = include_str!("../shaders/ssr.slang");
        let inv_proj = source
            .find("float4x4 inv_proj;")
            .expect("ssr.slang declares `float4x4 inv_proj;`");
        let proj = source
            .find("float4x4 proj;")
            .expect("ssr.slang declares `float4x4 proj;`");
        let inv_view = source
            .find("float4x4 inv_view;")
            .expect("ssr.slang declares `float4x4 inv_view;`");
        // The clipmap header's own four rows are held in order by
        // `crate::probe`'s `the_grid_header_matches_the_block_the_shaders_declare`,
        // which checks every file that declares them; this only has to place
        // the group between the matrices and the pyramid row.
        let probe_counts = source
            .find("uint4 probe_counts;")
            .expect("ssr.slang declares `uint4 probe_counts;`");
        let probe_inv_spacing = source
            .find("float4 probe_level_inv_spacing[PROBE_LEVELS];")
            .expect("ssr.slang declares `float4 probe_level_inv_spacing[PROBE_LEVELS];`");
        let hiz = source
            .find("uint4 hiz;")
            .expect("ssr.slang declares `uint4 hiz;`");
        let sky = source
            .find("float4 sky[3];")
            .expect("ssr.slang declares `float4 sky[3];`");
        assert!(
            inv_proj < proj
                && proj < inv_view
                && inv_view < probe_counts
                && probe_counts < probe_inv_spacing
                && probe_inv_spacing < hiz
                && hiz < sky,
            "ssr.slang declares the block in a different order than `to_bytes` writes it"
        );
    }

    /// The layout claim, checked rather than asserted in prose.
    #[test]
    fn the_block_serializes_matrices_and_probe_volume() {
        let mut inv_proj = [0.0f32; 16];
        inv_proj[0] = 1.0;
        let mut proj = [0.0f32; 16];
        proj[15] = 2.0;
        let inv_view = [3.0f32; 16];
        let probe_volume = crate::probe::ProbeVolume {
            origin: [4.0, 5.0, 6.0],
            inv_spacing: [7.0, 8.0, 9.0],
            counts: [10, 11, 12],
            levels: 1,
        };
        let bytes = SsrParams {
            inv_proj,
            proj,
            inv_view,
            probe_volume,
            hiz_levels: 3,
            // A different number in every lane of every row: the three bands
            // are the same width and would permute silently, and a sky whose
            // ground colour reached the zenith is a picture rather than an
            // error.
            sky: [
                [13.0, 14.0, 15.0, 16.0],
                [17.0, 18.0, 19.0, 20.0],
                [21.0, 22.0, 23.0, 24.0],
            ],
        }
        .to_bytes();

        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[124..128], &2.0f32.to_le_bytes());
        assert_eq!(&bytes[128..132], &3.0f32.to_le_bytes());
        // The clipmap header, whose first row is the counts and whose fourth
        // lane is the rows the whole table holds. Level 0's origin is the
        // volume's own, at the head of the per-level rows.
        let probes = 192;
        assert_eq!(&bytes[probes..probes + 4], &10u32.to_le_bytes());
        assert_eq!(&bytes[probes + 12..probes + 16], &1320u32.to_le_bytes());
        assert_eq!(&bytes[probes + 32..probes + 36], &4.0f32.to_le_bytes());
        // The pyramid row past the header: the level count in `x` and nothing
        // in the three lanes `std140` padded it out to.
        let hiz = probes + crate::probe::PROBE_VOLUME_SIZE;
        assert_eq!(&bytes[hiz..hiz + 4], &3u32.to_le_bytes());
        assert_eq!(&bytes[hiz + 4..hiz + 16], &[0u8; 12]);
        // And the sky's three rows past it, which is where the block now ends.
        let sky = hiz + 16;
        for (lane, expected) in (13u32..=24).enumerate() {
            let at = sky + lane * 4;
            assert_eq!(
                &bytes[at..at + 4],
                &(expected as f32).to_le_bytes(),
                "the sky's lane {lane} at offset {at}"
            );
        }
        assert_eq!(sky + 48, PARAMS_SIZE);
    }
}
