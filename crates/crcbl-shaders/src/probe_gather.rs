//! The reflective-shadow-map updater's constants and parameter block, in the
//! layout `shaders/probe_gather.slang` declares.
//!
//! ```text
//!  mesh.slang: rsmFragmentMain ──▶ the sun's map ─────┐
//!                               └▶ the punctual atlas ┤
//!                                            │        │
//!  GatherParams (this module) ───────────────┼────────┼──▶ probe_gather.slang
//!  PunctualProducer rows (this module) ──────┤        │           │
//!  ProbeVolume::position, as data ───────────┘        │           ▼
//!                                        one workgroup per probe → GpuProbe row
//! ```
//!
//! `docs/plan/50-irradiance-probes.md`'s raster updater. `crcbl_render::rsm` is
//! the pair of render passes that fill those targets and
//! `crcbl_render::probe_gather` is the dispatch that reads them; this module
//! owns the numbers both sides have to agree on, the block the dispatch is
//! parameterised by and the row that describes one punctual producer.
//!
//! # One dispatch walks every producer
//!
//! The sun's map and every punctual face are gathered by the same dispatch
//! rather than by one each, which is what lets `probe_gather.slang` end in a
//! plain store: a second dispatch over the same rows would erase the first's
//! instead of adding to it. It also pays the `groupshared` tree reduction once
//! rather than once per producer, and it keeps a row a function of one dispatch
//! — `docs/backlog.md`'s survey constraint C2, which no accumulation across
//! dispatches could hold.
//!
//! # [`RSM_SIDE`](crate::probe_gather::RSM_SIDE) is one constant, and it arrives
//! in the shader as data
//!
//! The map's resolution decides the pass's whole cost on both tiers, so it is a
//! number this crate owns rather than a literal in the Slang. The shader reads
//! it out of [`GatherParams::rsm_side`](crate::probe_gather::GatherParams);
//! nothing in `probe_gather.slang` names
//! it. That is also what makes the sweep in `docs/plan/50-irradiance-probes.md`
//! a change to one line.

/// Texels along one side of the reflective shadow map.
///
/// **Small on purpose, because every probe gathers every texel every frame.**
/// That is what makes the sample pattern *the image*: neither shader maps a
/// probe to a subset of the map, so there is no mapping to transcribe and no
/// history to carry — `docs/backlog.md`'s survey constraint C2 holds by
/// construction. The price is quadratic in this number and linear in the probe
/// count, which is why it is 64 and not 256.
///
/// **Swept at 32 and 64 before it was fixed**, on both tiers, through
/// `lantern --headless --frames 400 --size 1920x1080` (p50 of three runs, two
/// recordings a frame — lantern draws the room again for the screen in it). On
/// radv the gather went 0.020 ms at 32 to 0.047 ms at 64 and the map's own pass
/// did not move off 0.060 ms, which is that pass being draw-bound rather than
/// fill-bound at these extents; on lavapipe the map went 0.842 ms to 1.136 ms
/// and the gather stayed at 0.052 ms. Against a 1.28 ms radv frame and an 82 ms
/// lavapipe one, so both resolutions are affordable and the choice is not a
/// budget one.
///
/// **64, for the flux the coarser map misses.** The two resolutions' frames
/// differ by 0.15% RMSE with a peak channel difference of 17/255 in lantern's
/// room, and its measured points move by under 2% (the mirror's probe fallback
/// reads 26.0 at 32 and 25.6 at 64) — a small difference in a small room, which
/// is exactly the case a quadratic sample count flatters. 64 is the one that
/// keeps four times the samples for a cost neither tier notices, and lantern is
/// the smallest scene the updater will ever run in rather than the largest.
pub const RSM_SIDE: u32 = 64;

/// Texels along one side of **one punctual face's tile** in the punctual
/// reflective shadow map.
///
/// Its own number rather than [`RSM_SIDE`], because the two maps are priced
/// differently. A frame draws one cascade and up to
/// `crcbl_render::shadow::LIGHT_TILES` punctual faces, and the gather walks
/// every texel of every one of them for every probe — so the punctual half's
/// cost is this squared **times the faces a frame lights**, where the sun's is
/// [`RSM_SIDE`] squared paid once. That is what stops the argument [`RSM_SIDE`]
/// takes its own larger value on from transferring here.
///
/// **Swept at 16, 24 and 32 before it was fixed**, on both tiers, through
/// `lantern --headless --frames 400 --size 1920x1080` (p50 of three runs, two
/// recordings a frame — lantern draws the room again for the screen in it), and
/// against the tint `apps/lantern`'s frame claim 6 measures. Lantern lights
/// seven faces: a point light's cube and a spot.
///
/// ```text
///   side   radv frame   rsm-punctual   probe-gather   tint (radv / lavapipe)
///    16      1.455         0.122          0.099          1.3130 / 1.3124
///    24      1.523         0.122          0.148          1.3133 / 1.3121
///    32      1.575         0.124          0.221          1.3107 / 1.3107
/// ```
///
/// On lavapipe the map's own pass is **draw-bound and does not move**: 4.451,
/// 4.637 and 4.428 ms against an 88.9, 88.2 and 86.4 ms frame, with the gather
/// at 0.055 ms throughout. So the extent decides this rung's cost on the
/// hardware tier alone, and there it decides all of it — the gather is quadratic
/// in it and the frame follows, 8% of a radv frame between the ends of the
/// sweep.
///
/// **16, because nothing above it buys a picture.** The tint the fixture
/// measures is one value to within 0.2% at all three, which is less than the two
/// tiers differ from each other; the 16 and 32 frames differ by 0.077% RMSE with
/// a peak channel difference of 15/255 over lantern's whole room, half of what
/// [`RSM_SIDE`]'s own 32-against-64 pair showed. Where that constant could take
/// the larger of its pair for a cost neither tier noticed, this one is a cost
/// radv notices, and it is multiplied by every face a frame lights rather than
/// paid once.
///
/// What would move it is a bounce visibly coarser than this room's: lantern's
/// producers are a few metres from the walls they light, so a face's texel
/// covers about a third of a metre there. A scene lighting a hall from one lamp
/// would want this measured again rather than assumed.
pub const PUNCTUAL_RSM_SIDE: u32 = 16;

/// Invocations per workgroup in `probe_gather.slang` — its
/// `PROBE_GATHER_THREADS`, and the width of the `groupshared` reduction.
///
/// **The dispatch is one workgroup per probe**, so this is threads *within* a
/// probe rather than probes per group: a scene of sixty probes dispatched
/// `div_ceil(60, 64)` would run the whole volume on a single workgroup.
pub const GATHER_WORKGROUP_SIZE: u32 = 64;

/// Bytes in the gather's parameter block, matching `struct GatherParams` in
/// `shaders/probe_gather.slang`: one `float4`, one `float` and three `uint`.
pub const GATHER_PARAMS_SIZE: usize = 32;

const _: () = assert!(
    GATHER_PARAMS_SIZE.is_multiple_of(16),
    "std140 rounds a uniform block to 16 bytes"
);

/// What the gather cannot derive for itself, matching `struct GatherParams` in
/// `shaders/probe_gather.slang`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GatherParams {
    /// The sun's colour premultiplied by its intensity, exactly as the light row
    /// carries it. May exceed one. The fourth lane is unread padding.
    pub sun_color: [f32; 3],
    /// The world area one map texel covers, in square world units — measured
    /// **across the sun's beam**, which is what the cascade's orthographic axes
    /// are perpendicular to.
    ///
    /// The cascade's world footprint divided by [`RSM_SIDE`], squared — see
    /// `crcbl_render::rsm::texel_area`, which is where the cascade's own extent
    /// is read. It is every sample's weight that this scales, so a value out by
    /// a factor scales the whole bounce by it.
    pub texel_area: f32,
    /// [`RSM_SIDE`], as the shader reads it.
    pub rsm_side: u32,
    /// Rows the dispatch covers — the volume's
    /// [`total`](crate::probe::ProbeVolume::total). A group past it writes
    /// nothing.
    pub probes: u32,
    /// How many [`PunctualProducer`] rows the dispatch walks after the sun's
    /// map, which is how many punctual faces this frame drew.
    ///
    /// Zero is an ordinary frame rather than a disabled one: a scene with no
    /// shadowed punctual light draws no face, and the loop that reads this runs
    /// no iterations.
    pub producers: u32,
}

impl GatherParams {
    /// The bytes the parameter block holds, in `std140` order.
    ///
    /// The colour is padded to a `float4` because that is what a uniform block
    /// does to a `float3`; the padding is written as zero and the shader does not
    /// read it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; GATHER_PARAMS_SIZE] {
        let mut bytes = [0u8; GATHER_PARAMS_SIZE];
        let mut at = 0usize;
        let mut put = |value: [u8; 4]| {
            bytes[at..at + 4].copy_from_slice(&value);
            at += 4;
        };
        for value in self.sun_color {
            put(value.to_le_bytes());
        }
        put(0f32.to_le_bytes());
        put(self.texel_area.to_le_bytes());
        put(self.rsm_side.to_le_bytes());
        put(self.probes.to_le_bytes());
        put(self.producers.to_le_bytes());
        debug_assert_eq!(at, GATHER_PARAMS_SIZE);
        bytes
    }
}

/// Bytes one [`PunctualProducer`] row occupies, matching
/// `struct PunctualProducer` in `shaders/probe_gather.slang`: three `float4`
/// and one `uint4`.
pub const PRODUCER_STRIDE: usize = 64;

const _: () = assert!(
    PRODUCER_STRIDE.is_multiple_of(16),
    "std430 rounds a structured-buffer element to its largest member's alignment"
);

/// One punctual shadow face drawn into the punctual reflective shadow map, and
/// everything the gather needs to weigh its texels.
///
/// **One row per face rather than per light**, because a point light's six faces
/// are six tiles of the map and the gather walks a tile at a time. The light's
/// own terms — where it is, how far it reaches, what colour it is and how its
/// cone closes — are the same in all six, and repeating them is sixty-four bytes
/// against a second indirection in the innermost loop of the pass.
///
/// # What the gather does with it, and why these fields and no others
///
/// A texel of a face is a patch lit by this light alone, so its reflected flux
/// is `albedo · color · spot_cone · punctual_falloff(d, radius) · ω · d²` with
/// `d` the patch's distance to the light and `ω` the solid angle the texel
/// subtends from it — the surface cosine cancels between the patch's radiance
/// and its area, exactly as it does for the sun. `ω` is a closed form of the
/// texel's own position in its tile, so nothing here carries it.
///
/// **The falloff is the engine's own**, not a physical inverse square: the
/// bounce multiplies the same `range_window` and the same `1 / (d² + 1)` that
/// `mesh.slang` shades this light's direct term with, so a light whose direct
/// contribution the engine has already bent keeps that bend in its bounce.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PunctualProducer {
    /// Where the light is, in render space — `crcbl_render::Light::sphere`'s
    /// centre.
    pub position: [f32; 3],
    /// How far it reaches, in world units: the radius `range_window` closes at
    /// and `light_cluster.slang` culls against.
    pub radius: f32,
    /// Colour premultiplied by intensity, exactly as the light's row carries it.
    /// May exceed one.
    pub color: [f32; 3],
    /// Cosine of the cone's outer half-angle — [`crate::light::GpuLight::direction`]'s `w`.
    /// Unread unless [`kind`](Self::kind) is [`KIND_SPOT`](crate::light::KIND_SPOT).
    pub cos_outer: f32,
    /// The direction the cone points **along**, away from the light, normalised.
    /// Unread for a point light.
    pub axis: [f32; 3],
    /// Cosine of the cone's inner half-angle — [`crate::light::GpuLight::cos_inner`]. Unread
    /// for a point light.
    pub cos_inner: f32,
    /// Where this face's tile starts in the punctual map, in texels.
    pub origin: [u32; 2],
    /// The tile's side in texels — [`PUNCTUAL_RSM_SIDE`], carried per row so the
    /// shader names no extent of its own.
    pub side: u32,
    /// [`KIND_POINT`](crate::light::KIND_POINT) or
    /// [`KIND_SPOT`](crate::light::KIND_SPOT), which is the light row's own
    /// field: it decides whether the cone is applied and nothing else.
    ///
    /// The row's kind rather than a flag of this module's, so a producer and the
    /// light it was built from cannot disagree about what the light is.
    pub kind: u32,
}

impl PunctualProducer {
    /// The bytes one row holds, in `std430` order.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; PRODUCER_STRIDE] {
        let mut bytes = [0u8; PRODUCER_STRIDE];
        let mut at = 0usize;
        let mut put = |value: [u8; 4]| {
            bytes[at..at + 4].copy_from_slice(&value);
            at += 4;
        };
        for value in self.position {
            put(value.to_le_bytes());
        }
        put(self.radius.to_le_bytes());
        for value in self.color {
            put(value.to_le_bytes());
        }
        put(self.cos_outer.to_le_bytes());
        for value in self.axis {
            put(value.to_le_bytes());
        }
        put(self.cos_inner.to_le_bytes());
        for value in self.origin {
            put(value.to_le_bytes());
        }
        put(self.side.to_le_bytes());
        put(self.kind.to_le_bytes());
        debug_assert_eq!(at, PRODUCER_STRIDE);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::{PROJECT_L0, PROJECT_L1};

    /// The shader source this module is the mirror of.
    const SOURCE: &str = include_str!("../shaders/probe_gather.slang");

    /// **The gather multiplies a sample by the two numbers
    /// [`crate::probe::GpuProbe::accumulate`] multiplies it by**, and nothing
    /// else in the tree would notice if it did not.
    ///
    /// `the_visibility_map_constants_match_the_ones_the_shaders_declare`'s
    /// pattern, applied to the projection weights: each file compiles with any
    /// value, and a drift shows up only as a bounce that is uniformly too bright
    /// or too flat — a perfectly plausible picture, and one a re-bless accepts.
    /// [`crate::probe`]'s own docs say why these two in particular: they are the
    /// arithmetic a caller doing the projection by hand gets plausibly wrong,
    /// and this shader is exactly such a caller.
    ///
    /// Reading the source is the check, and the source is hash-pinned by the
    /// manifest, so it is the same file the committed artifact was built from.
    #[test]
    fn the_gather_projects_a_sample_the_way_this_module_does() {
        for declaration in [
            format!("static const float PROBE_PROJECT_L0 = {PROJECT_L0};"),
            format!("static const float PROBE_PROJECT_L1 = {PROJECT_L1};"),
        ] {
            assert!(
                SOURCE.contains(&declaration),
                "probe_gather.slang does not declare `{declaration}`; the projection weights \
                 have drifted from the module the row is defined by"
            );
        }
    }

    /// **The workgroup width this module names is the one the shader launches**,
    /// which is also the width of its `groupshared` reduction — a mismatch would
    /// leave the top lanes' partial sums unread and the row short of the light
    /// they carried.
    #[test]
    fn the_shader_launches_the_workgroup_this_module_sizes() {
        assert!(
            SOURCE.contains(&format!(
                "static const uint PROBE_GATHER_THREADS = {GATHER_WORKGROUP_SIZE};"
            )),
            "probe_gather.slang does not declare PROBE_GATHER_THREADS = {GATHER_WORKGROUP_SIZE}"
        );
        assert!(
            SOURCE.contains(&format!("[numthreads({GATHER_WORKGROUP_SIZE}, 1, 1)]")),
            "probe_gather.slang does not launch {GATHER_WORKGROUP_SIZE} threads a group"
        );
        assert!(
            GATHER_WORKGROUP_SIZE.is_power_of_two(),
            "the tree reduction halves each round and has no odd lane to carry"
        );
    }

    /// **The map's resolution reaches the shader as data and is declared in it
    /// nowhere**, which is what makes the sweep a change to [`RSM_SIDE`] alone.
    ///
    /// The check is on the *declaration*, not on the digits: `64` is also this
    /// file's workgroup width, and a test that refused the number outright would
    /// be refusing two unrelated constants that happen to be equal today.
    #[test]
    fn the_map_s_resolution_is_not_written_into_the_shader() {
        assert!(
            SOURCE.contains("params.rsm_side"),
            "probe_gather.slang does not read the map's side out of its parameter block"
        );
        for line in SOURCE.lines() {
            let line = line.trim();
            assert!(
                !(line.starts_with("static const") && line.contains("RSM")),
                "probe_gather.slang declares `{line}`; the map's side is \
                 `GatherParams::rsm_side` and nothing else"
            );
        }
    }

    /// The block writes its fields in declaration order, at the offsets `std140`
    /// puts them at: 0, 16, 20, 24, 28.
    #[test]
    fn the_params_block_writes_its_fields_in_declaration_order() {
        let params = GatherParams {
            sun_color: [4.0, 5.0, 6.0],
            texel_area: 7.0,
            rsm_side: 8,
            probes: 9,
            producers: 10,
        };
        let bytes = params.to_bytes();
        let float_at = |offset: usize| {
            f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
        };
        let word_at = |offset: usize| {
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
        };
        for (offset, want) in [(0, 4.0), (4, 5.0), (8, 6.0), (12, 0.0)] {
            assert_eq!(float_at(offset), want, "sun colour at {offset}");
        }
        assert_eq!(float_at(16), 7.0);
        assert_eq!(word_at(20), 8);
        assert_eq!(word_at(24), 9);
        assert_eq!(word_at(28), 10);
    }

    /// A producer row writes its fields in declaration order too, and every one
    /// of them is distinct — a row whose `w` lanes were swapped would light a
    /// cone by its radius and reach as far as a cosine.
    #[test]
    fn a_producer_row_writes_its_fields_in_declaration_order() {
        let producer = PunctualProducer {
            position: [1.0, 2.0, 3.0],
            radius: 4.0,
            color: [5.0, 6.0, 7.0],
            cos_outer: 8.0,
            axis: [9.0, 10.0, 11.0],
            cos_inner: 12.0,
            origin: [13, 14],
            side: 15,
            kind: crate::light::KIND_SPOT,
        };
        let bytes = producer.to_bytes();
        let float_at = |offset: usize| {
            f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
        };
        let word_at = |offset: usize| {
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
        };
        for offset in 0..12 {
            let at = offset * 4;
            #[expect(
                clippy::cast_precision_loss,
                reason = "a dozen small integers written as floats"
            )]
            let want = (offset + 1) as f32;
            assert_eq!(float_at(at), want, "the float lane at {at}");
        }
        assert_eq!(word_at(48), 13);
        assert_eq!(word_at(52), 14);
        assert_eq!(word_at(56), 15);
        assert_eq!(word_at(60), crate::light::KIND_SPOT);
    }

    /// **The gather's falloff and cone are `mesh.slang`'s own**, character for
    /// character, which is what makes a light's bounce agree with the direct
    /// term the same light already contributed.
    ///
    /// Slang has no `#include`, so the two copies are what there is; each file
    /// compiles with any body, and a drift shows up only as a bounce that is
    /// brighter or dimmer than the light casting it — a plausible picture, and
    /// one a re-bless accepts. `the_gather_projects_a_sample_the_way_this_module_does`
    /// is the same argument applied to the projection weights.
    #[test]
    fn the_gather_falls_off_the_way_the_shading_does() {
        let mesh = include_str!("../shaders/mesh.slang");
        for body in [
            "float ratio = distance / max(radius, 1e-6);",
            "float window = saturate(1.0 - ratio * ratio * ratio * ratio);",
            "return window * window;",
            "return range_window(distance, radius) / (distance * distance + 1.0);",
            "float cosine = dot(-to_light, normalize(axis));",
            "return saturate((cosine - cos_outer) / max(cos_inner - cos_outer, 1e-4));",
        ] {
            assert!(
                mesh.contains(body),
                "mesh.slang no longer contains `{body}`; the shading this gather \
                 is a copy of has moved"
            );
            assert!(
                SOURCE.contains(body),
                "probe_gather.slang does not contain `{body}`; the bounce has \
                 drifted from the direct term it is supposed to agree with"
            );
        }
    }

    /// The producer's kind is the light row's kind, and the shader declares the
    /// one value it compares against.
    ///
    /// A gather that read `KIND_POINT` where a row carries `KIND_SPOT` would
    /// light a cone's whole sphere, which is a brighter room rather than an
    /// error.
    #[test]
    fn the_gather_names_the_spot_kind_the_light_row_carries() {
        assert!(
            SOURCE.contains(&format!(
                "static const uint PROBE_LIGHT_KIND_SPOT = {};",
                crate::light::KIND_SPOT
            )),
            "probe_gather.slang does not declare the spot kind `crate::light` defines"
        );
    }
}
