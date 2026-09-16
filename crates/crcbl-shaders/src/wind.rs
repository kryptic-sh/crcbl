//! The two uniform blocks, the workgroup size and the constants `wind.slang`
//! declares, in the layouts that shader declares.
//!
//! Same reason as [`crate::compute_probe`] and [`crate::water`]: the shader
//! fixes numbers and a byte layout, every producer of those has to agree with
//! it exactly, and keeping both in the crate that owns the source means there
//! is one place to change rather than one per consumer.
//!
//! `crcbl-wind` fills [`WindParams`] from the authoritative CPU field — see
//! `crcbl_wind::WindField::gpu_params`, which is where the `f64` formula is
//! narrowed to `f32` — and `crcbl-render` binds it.

/// Invocations per workgroup, matching `[numthreads(64, 1, 1)]` in
/// `shaders/wind.slang`.
///
/// A caller dispatches `points.div_ceil(WORKGROUP_SIZE)` groups; the shader
/// discards the invocations past [`ProbeParams::count`] that the last group
/// brings.
pub const WORKGROUP_SIZE: u32 = 64;

/// Below this squared length a composed wind direction has no direction left,
/// and the base direction answers instead. Matches
/// `WIND_MIN_DIRECTION_LENGTH_SQUARED` in `shaders/wind.slang`, and
/// `crcbl_wind::MIN_DIRECTION_LENGTH_SQUARED` on the CPU.
pub const MIN_DIRECTION_LENGTH_SQUARED: f32 = 1e-12;

/// How far a GPU wind sample may sit from the CPU's, as a fraction of the
/// weather's base speed.
///
/// **A measurement, not a wish.** `docs/plan/56-wind.md`'s "CPU–GPU agreement"
/// check says "within a stated tolerance, on every backend", and this is the
/// statement. Three things put the two apart and none of them can be closed:
///
/// 1. **The bilinear filter**, which dominates. Vulkan guarantees only eight
///    bits of sub-texel precision on a linear filter, and Metal and D3D12 say
///    similar; the CPU copy interpolates exactly. A texel step's worth of
///    weight error is what that buys, so the disagreement scales with how far
///    apart neighbouring texels are and with the speed they are multiplied by.
/// 2. **Contraction.** A shader compiler may fuse a multiply and an add into an
///    FMA. Rust never does it on its own, so the two evaluate the same
///    expression with different rounding — the same freedom
///    `crcbl_shaders::trig` and [`crate::fog`] run under.
/// 3. **`f32` against `f64`.** The CPU copy is `f64` throughout because physics
///    is; the narrowing happens once, in `gpu_params`.
///
/// **Why a fraction of the base speed and not an absolute velocity.** The first
/// cause is a weight error, and a weight error multiplies whatever the weights
/// are carrying. Swept over all five `crcbl_wind::Beaufort` presets on
/// 2026-09-16, the absolute disagreement over the committed layers ran from
/// 0.0036 m/s at *still* to 0.113 m/s at *violent* on lavapipe — a factor of
/// thirty — while the ratio to the base speed stayed between 0.0058 and 0.0076
/// throughout. An absolute bound would therefore be unreachably loose at
/// Beaufort still or wrong at violent, and would say nothing about which.
///
/// **Where the number comes from.** The same sweep, on the checkerboard layer
/// pair `crates/crcbl/tests/render_e2e/wind.rs` builds — every neighbour at the
/// opposite end of the eight-bit range, which is the hardest thing a linear
/// filter can be handed. That reached 0.0126 of base speed on lavapipe
/// (Mesa 26.2.2, LLVM 22.1.8) and 0.0037 on an RX 7900 XTX under radv, so this
/// leaves roughly a factor of two over the worst case a software rasteriser
/// produced and eight over real hardware. Run that file's two tests to
/// reproduce it; both print every figure above.
pub const MAX_CPU_GPU_ERROR: f64 = 0.03;

/// Bytes of [`WindParams`]: four sixteen-byte `std140` rows.
pub const PARAMS_SIZE: usize = 16 * 4;

/// Bytes of [`ProbeParams`]: one `uint`, padded to the sixteen-byte row
/// `std140` gives a uniform buffer.
pub const PROBE_PARAMS_SIZE: usize = 16;

/// The wind's uniform block, matching `struct WindParams` in
/// `shaders/wind.slang`.
///
/// Everything in it is camera-relative or already wrapped, which is decision
/// 3's "the renderer receives the offset relative to the camera so a float
/// never holds a world-scale coordinate".
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindParams {
    /// Unit horizontal direction the wind blows towards, on the XZ plane.
    pub base_direction: [f32; 2],
    /// Base speed in m/s.
    pub base_speed: f32,
    /// Peak fractional swing of the gust about one. Zero is no gusting.
    pub gust_amplitude: f32,
    /// `dot(camera − o, base_direction) / wavelength`, whole turns dropped, so
    /// it is in `0..1` however far the session has scrolled.
    pub gust_phase: f32,
    /// One over the gust wavelength, in reciprocal metres.
    pub inv_gust_wavelength: f32,
    /// The camera's texture coordinate in the direction layer, wrapped.
    pub direction_uv: [f32; 2],
    /// Direction-layer texture coordinates per metre of world.
    pub direction_uv_per_metre: [f32; 2],
    /// The camera's texture coordinate in the intensity layer, wrapped.
    pub intensity_uv: [f32; 2],
    /// Intensity-layer texture coordinates per metre of world.
    pub intensity_uv_per_metre: [f32; 2],
}

impl WindParams {
    /// The block as the bytes a uniform buffer holds, little-endian.
    ///
    /// The one padded row is written as zeroes rather than left alone: a
    /// uniform buffer allocated for this block is [`PARAMS_SIZE`] bytes wide,
    /// and a partial write leaves its tail undefined.
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let words: [f32; PARAMS_SIZE / 4] = [
            self.base_direction[0],
            self.base_direction[1],
            self.base_speed,
            self.gust_amplitude,
            self.gust_phase,
            self.inv_gust_wavelength,
            0.0,
            0.0,
            self.direction_uv[0],
            self.direction_uv[1],
            self.direction_uv_per_metre[0],
            self.direction_uv_per_metre[1],
            self.intensity_uv[0],
            self.intensity_uv[1],
            self.intensity_uv_per_metre[0],
            self.intensity_uv_per_metre[1],
        ];
        let mut bytes = [0u8; PARAMS_SIZE];
        for (word, slot) in words.into_iter().zip(bytes.chunks_exact_mut(4)) {
            slot.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    /// The block read back out of the bytes [`Self::to_bytes`] writes.
    ///
    /// Here so the round trip is a test rather than a claim — the padded row is
    /// the part a hand-written layout gets wrong, and a writer with no reader
    /// cannot be shown to have got it right.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; PARAMS_SIZE]) -> Self {
        let word = |index: usize| {
            let at = index * 4;
            f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        Self {
            base_direction: [word(0), word(1)],
            base_speed: word(2),
            gust_amplitude: word(3),
            gust_phase: word(4),
            inv_gust_wavelength: word(5),
            direction_uv: [word(8), word(9)],
            direction_uv_per_metre: [word(10), word(11)],
            intensity_uv: [word(12), word(13)],
            intensity_uv_per_metre: [word(14), word(15)],
        }
    }
}

/// The agreement probe's uniform block, matching `struct WindProbeParams` in
/// `shaders/wind.slang`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProbeParams {
    /// Points to sample. Invocations at or past this index write nothing.
    pub count: u32,
}

impl ProbeParams {
    /// The block as the bytes a uniform buffer holds, little-endian, padded to
    /// [`PROBE_PARAMS_SIZE`].
    #[must_use]
    pub fn to_bytes(self) -> [u8; PROBE_PARAMS_SIZE] {
        let mut bytes = [0u8; PROBE_PARAMS_SIZE];
        bytes[0..4].copy_from_slice(&self.count.to_le_bytes());
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shader source, hash-pinned by `spirv/manifest.txt`, so this reads
    /// the same file the committed artifacts were built from.
    const SOURCE: &str = include_str!("../shaders/wind.slang");

    #[test]
    fn the_workgroup_size_matches_the_numthreads_wind_slang_declares() {
        let declaration = format!("[numthreads({WORKGROUP_SIZE}, 1, 1)]");
        assert!(
            SOURCE.contains(&declaration),
            "wind.slang does not declare `{declaration}`; WORKGROUP_SIZE has drifted"
        );
    }

    #[test]
    fn the_shader_spells_the_same_direction_floor() {
        assert!(
            SOURCE.contains("WIND_MIN_DIRECTION_LENGTH_SQUARED = 1e-12"),
            "wind.slang's direction floor has drifted from MIN_DIRECTION_LENGTH_SQUARED"
        );
        assert_eq!(MIN_DIRECTION_LENGTH_SQUARED, 1e-12);
    }

    /// Every member of the block, in the order the shader declares them.
    ///
    /// The layout is what a wrong answer looks like when nothing checks it: a
    /// swapped pair reads plausible numbers out of the wrong slots and the
    /// field points somewhere else, with no validation message anywhere.
    #[test]
    fn the_block_is_declared_in_the_order_it_is_written() {
        let declaration = SOURCE
            .split_once("struct WindParams\n{")
            .expect("wind.slang declares the block")
            .1
            .split_once('}')
            .expect("the block ends")
            .0;
        let members: Vec<&str> = declaration
            .lines()
            .filter_map(|line| line.trim().strip_suffix(';'))
            .filter_map(|line| line.rsplit_once(' '))
            .map(|(_, name)| name)
            .collect();
        assert_eq!(
            members,
            vec![
                "baseDirection",
                "baseSpeed",
                "gustAmplitude",
                "gustPhase",
                "invGustWavelength",
                "pad0",
                "directionUv",
                "directionUvPerMetre",
                "intensityUv",
                "intensityUvPerMetre",
            ]
        );
    }

    #[test]
    fn the_block_round_trips_and_pads_its_spare_row() {
        let params = WindParams {
            base_direction: [0.6, -0.8],
            base_speed: 9.0,
            gust_amplitude: 0.25,
            gust_phase: 0.125,
            inv_gust_wavelength: 1.0 / 32.0,
            direction_uv: [0.5, 0.25],
            direction_uv_per_metre: [1.0 / 256.0, 1.0 / 256.0],
            intensity_uv: [0.75, 0.125],
            intensity_uv_per_metre: [1.0 / 128.0, 1.0 / 128.0],
        };
        let bytes = params.to_bytes();
        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert!(
            bytes[24..32].iter().all(|byte| *byte == 0),
            "the spare half of the second row is written as zeroes: {:?}",
            &bytes[24..32]
        );
        assert_eq!(WindParams::from_bytes(&bytes), params);
    }

    #[test]
    fn the_probe_block_is_one_le_u32_in_a_sixteen_byte_row() {
        let bytes = ProbeParams { count: 0x0102_0304 }.to_bytes();
        assert_eq!(bytes.len(), PROBE_PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &[0x04, 0x03, 0x02, 0x01]);
        assert!(bytes[4..].iter().all(|byte| *byte == 0), "{bytes:?}");
    }

    /// The shader reads its layers through a sampler and never as a storage
    /// image — decision 6's WebGPU constraint, and a thing a later edit could
    /// undo without any test noticing.
    #[test]
    fn no_layer_is_a_storage_texture() {
        assert!(
            !SOURCE.contains("RWTexture"),
            "wind.slang declares a storage texture; decision 6 spends none"
        );
        // `.SampleLevel(` rather than `SampleLevel`, so the prose above the
        // code — which names the call to say why it is a sampler and not a
        // `Load` — is not counted as a tap.
        assert_eq!(
            SOURCE.matches(".SampleLevel(").count(),
            2,
            "the field is two taps: one direction layer and one intensity layer"
        );
    }
}
