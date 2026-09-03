//! The reflective-shadow-map updater's constants and parameter block, in the
//! layout `shaders/probe_gather.slang` declares.
//!
//! ```text
//!  mesh.slang: rsmFragmentMain ──▶ albedo / normal / world targets
//!                                            │
//!  GatherParams (this module) ───────────────┼──▶ probe_gather.slang
//!  ProbeVolume::position, as data ───────────┘            │
//!                                                         ▼
//!                                        one workgroup per probe → GpuProbe row
//! ```
//!
//! `docs/plan/50-irradiance-probes.md`'s raster updater. `crcbl_render::rsm` is
//! the render pass that fills those targets and `crcbl_render::probe_gather` is
//! the dispatch that reads them; this module owns the two numbers both sides
//! have to agree on and the block the dispatch is parameterised by.
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
        put(0u32.to_le_bytes());
        debug_assert_eq!(at, GATHER_PARAMS_SIZE);
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
        assert_eq!(word_at(28), 0, "the trailing lane is padding and is zeroed");
    }
}
