//! The uniform block and the far-plane constant `ssao.slang` declares, in the
//! layouts that shader declares.
//!
//! Same reason as [`crate::compute_probe`]: the shader fixes a byte layout and a
//! value, every producer of those has to agree with it exactly, and keeping both
//! in the crate that owns the source means there is one place to change rather
//! than one per consumer.
//!
//! `ssao_blur.slang` has no module of its own, because it has no block and no
//! constant a caller has to match — it reads one texture and writes one channel.

/// Bytes of the uniform block: three `float4x4` and one `float4` row.
///
/// `std140` gives a `float4x4` four sixteen-byte columns and a `float4` one row,
/// and the total is already a multiple of sixteen, so there is no tail padding
/// to write. See [`SsaoParams::to_bytes`].
pub const PARAMS_SIZE: usize = 64 + 64 + 64 + 16;

/// The reversed-Z far plane, matching `static const float DEPTH_FAR` in
/// `shaders/ssao.slang`.
///
/// The value `crcbl_hal::depth::CLEAR` holds, restated here because this crate
/// does not depend on the seam and the shader cannot include either of them.
/// `crcbl_render::forward` is where the two are asserted equal, which is the
/// link that makes this a mirror rather than a second opinion.
///
/// **The far plane has no surface.** `ssao.slang` leaves early at exactly this
/// depth: an infinite reversed-Z projection takes `clip.w` to zero here, so a
/// pixel the geometry never covered would otherwise reconstruct a view-space
/// position by dividing by nothing.
pub const DEPTH_FAR: f32 = 0.0;

/// The factor the occlusion pair's extent is divided by on each axis, matching
/// `static const int RESOLUTION_DIVISOR` in `shaders/ssao.slang` and in the two
/// shaders beside it.
///
/// **The occlusion passes do not run per pixel.** `ssao.slang` marches its
/// horizons over an image this many times smaller on each side and
/// `ssao_blur.slang` filters it there; `ssao_upsample.slang` is what
/// reconstructs the full-resolution channel the forward pass binds, weighting
/// each sample by how near its surface is to the pixel being written. Occlusion
/// is low-frequency, so a quarter of the marches carries very nearly the same
/// picture — the reference implementations halve it for that reason, and
/// `ssao.slang`'s header is where the trade is argued.
///
/// `crcbl_render::ssao::half_extent` is what sizes the images with it, and it is
/// the only place in the engine that may: a second halving spelled somewhere
/// else is an image the passes would render a fraction of.
pub const RESOLUTION_DIVISOR: u32 = 2;

/// Planes through the eye each pixel sweeps by default, matching
/// `static const uint SLICE_COUNT_DEFAULT` in `shaders/ssao.slang`.
///
/// Two: the tile's own direction and its quarter turn. It is the floor
/// `slice_count` clamps to — so a producer that leaves [`SsaoParams::slices`]
/// at zero gets this frame rather than an unoccluded one — and that is the
/// whole of what "default" means here.
///
/// **It is not what the engine ships.** `crcbl_render::ssao`'s `r_ssao_slices`
/// has defaulted to [`SLICE_COUNT_MAX`] since 2026-09-03, and the goldens were
/// re-blessed at that count; a frame reaching this one is a frame whose
/// producer wrote nothing.
pub const SLICE_COUNT_DEFAULT: u8 = 2;

/// The most planes a pixel may sweep, matching `static const uint
/// SLICE_COUNT_MAX` in `shaders/ssao.slang`.
///
/// Four, the extra pair at an eighth turn from the first. The shader's own
/// constant carries the arithmetic: the turn is exact because it is done on the
/// direction table's integers, and what it buys is four plane orientations the
/// table cannot otherwise reach.
pub const SLICE_COUNT_MAX: u8 = 4;

/// The AO intensity a producer that writes nothing gets, matching `static const
/// float INTENSITY_DEFAULT` in `shaders/ssao_upsample.slang`.
///
/// One, and it is the exponent that changes nothing: the reconstruction returns
/// the visibility its taps averaged, untouched. Every golden in this workspace
/// was blessed here, and it is what a zero in [`SsaoParams::intensity`] is
/// answered with rather than the floor a clamp would map it to — see that
/// field, and `ao_intensity` in the shader.
pub const INTENSITY_DEFAULT: f32 = 1.0;

/// The weakest AO intensity the reconstruction will honour, matching `static
/// const float INTENSITY_MIN` in `shaders/ssao_upsample.slang`.
///
/// The shader's constant carries the argument: the curve's slope at an
/// unoccluded surface is the exponent itself, so a quarter is where four levels
/// of the channel the blur wrote move the reconstruction by one.
pub const INTENSITY_MIN: f32 = 0.25;

/// The strongest AO intensity the reconstruction will honour, matching `static
/// const float INTENSITY_MAX` in `shaders/ssao_upsample.slang`.
///
/// [`INTENSITY_MIN`] reciprocated, which is the only symmetry an exponent has,
/// and the same argument from the other end: at four, one level of the gathered
/// channel is four of the reconstructed one.
pub const INTENSITY_MAX: f32 = 4.0;

/// The three above are an intensity *control*, checked where they are written.
///
/// A compile-time block rather than a test, because every term is a constant and
/// a test over constants is one nothing but this edit could make fail. What it
/// holds: the default changes nothing, the floor weakens the measurement without
/// reaching the exponent that erases it, and the ceiling is over the default — a
/// control whose ceiling *is* the default can only ever weaken the occlusion,
/// which is the half `docs/backlog.md` says is not the one wanted.
const _: () = {
    assert!(
        INTENSITY_DEFAULT == 1.0,
        "a default that is not one is a curve applied to every frame nobody asked to change"
    );
    assert!(
        INTENSITY_MIN > 0.0 && INTENSITY_MIN < INTENSITY_DEFAULT,
        "the floor must weaken the measurement without reaching the exponent that erases it"
    );
    assert!(
        INTENSITY_MAX > INTENSITY_DEFAULT,
        "an intensity control whose ceiling is the default can only weaken the occlusion"
    );
};

/// How long the mean of a set of bent directions has to be before anything in
/// the occlusion chain calls it a direction, matching `static const float
/// BENT_NORMAL_MIN_LENGTH` in `shaders/ssao.slang` and in the three shaders
/// beside it.
///
/// **One value, one question, four declarations.** `ssao.slang` averages the
/// bisectors of its slices, the two filters average their taps, and
/// `mesh.slang` decodes what arrives; each of them is asking whether the
/// directions it has agree enough to have an average. Every direction going in
/// is a unit vector, so the mean's length is a coherence in `0..=1` — one where
/// they all point the same way, zero where they cancel — and this is where
/// "they cancelled" begins.
///
/// It is also what separates a decoded direction from the decoded sentinel: the
/// zero direction encodes to `0.5` in each channel and [`BENT_NORMAL_NONE`] is
/// the byte that lands on, which decodes back to under a hundredth of a unit
/// while a real direction survives `Rgba8Unorm` to within about the same. Half
/// way between those is a threshold no rounding on any target can reach from
/// either side.
pub const BENT_NORMAL_MIN_LENGTH: f32 = 0.5;

/// The byte each of the three bent-direction channels holds where there is no
/// direction.
///
/// `ssao.slang` writes the zero direction as `0.0 * 0.5 + 0.5`, which an
/// `Rgba8Unorm` target quantises to this — so a pixel the gather had nothing to
/// say about and the 1×1 placeholder `crcbl_render::forward` binds when no
/// occlusion pass runs hold the same bytes, and "no pass ran" and "no direction
/// here" are one case with one answer. See [`BENT_NORMAL_MIN_LENGTH`], which is
/// what a consumer tests, and `mesh.slang`'s `bent_normal_at`, which answers it
/// with the shading normal.
pub const BENT_NORMAL_NONE: u8 = 0x80;

/// Radians [`acos_approx`] is allowed to differ from `f64::acos`.
///
/// Measured rather than chosen, over `-1..=1` at two million steps —
/// `the_arc_cosine_polynomial_is_as_accurate_as_it_claims` is the sweep. The
/// worst case sits at the middle of the domain, where the square root's
/// derivative is smallest and the polynomial is carrying the whole answer.
///
/// What it is worth in a frame: the occlusion integral's range is about `π/2`
/// and it lands in an `R8Unorm` channel, so one channel level is roughly `6e-3`
/// radians of horizon angle. This error is two orders of magnitude under that,
/// which is what makes an approximation admissible here at all.
pub const MAX_ACOS_ERROR: f64 = 7e-5;

/// The polynomial [`acos_approx`] evaluates, index `i` the coefficient of `x^i`.
///
/// **Abramowitz and Stegun 4.4.45**, the degree-three minimax fit of
/// `acos(x) / sqrt(1 - x)` on `0..=1`. Transcribed rather than invented, and
/// `the_shader_evaluates_this_modules_polynomial` is what holds `ssao.slang`'s
/// copy to it.
const ACOS_KERNEL: [f32; 4] = [1.570_728_8, -0.212_114_4, 0.074_261, -0.018_729_3];

/// `acos(x)` for `x` in `-1..=1`, using only operations IEEE-754 specifies
/// exactly.
///
/// # Why not the intrinsic
///
/// `ssao.slang` sums a **horizon integral**, and the angles in it come from
/// dot products through an arc cosine. Every target has an `acos`, and no
/// target's is specified to any accuracy — Vulkan, D3D and Metal each leave it
/// to the implementation — so two rasterisers drawing the same frame would
/// disagree by however much their libraries disagree, which is not a number
/// anyone can bound. A polynomial and a `sqrt` are both exactly specified, so
/// this is bit-identical wherever the compiler does not reassociate.
///
/// # The reduction
///
/// The fit is on `0..=1` only. A negative argument is answered through
/// `acos(-x) = π - acos(x)`, which is exact in the sense that matters: it adds
/// no error of its own beyond the one rounding of the subtraction.
///
/// Exact at `x = 1`, where the square root takes the whole expression to zero —
/// which is the case a grazing horizon lands on, and the one an approximation
/// that merely got close would turn into a rim of occlusion around every
/// silhouette.
#[must_use]
pub fn acos_approx(x: f32) -> f32 {
    let magnitude = x.abs().min(1.0);
    let mut kernel = ACOS_KERNEL[3];
    for coefficient in ACOS_KERNEL[..3].iter().rev() {
        kernel = kernel * magnitude + coefficient;
    }
    let positive = kernel * (1.0 - magnitude).sqrt();
    if x < 0.0 {
        std::f32::consts::PI - positive
    } else {
        positive
    }
}

/// The uniform block, matching `struct SsaoParams` in `shaders/ssao.slang`.
///
/// Both matrices are **column-major**, the order `glam::Mat4::to_cols_array`
/// produces and the order every other block in this crate is written in — see
/// [`crate::mesh::FrameUniforms`], whose `view_proj` this file's `proj` is a
/// sibling of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SsaoParams {
    /// Clip → view: the inverse of the camera's projection alone, **not** of its
    /// view-projection. The occlusion integral is a question about the
    /// neighbourhood of a surface, and view space is where that neighbourhood is
    /// isotropic and the eye is at the origin.
    pub inv_proj: [f32; 16],
    /// View → clip, for projecting a sample point back to the pixel whose depth
    /// answers for it.
    pub proj: [f32; 16],
    /// View → world, for the bent direction alone.
    ///
    /// The occlusion integral is entirely a view-space question — see
    /// [`inv_proj`] — and the bent direction is the one value that leaves the
    /// pass. Its consumer is `mesh.slang`'s ambient term, which evaluates a
    /// world-space L1 environment, so the rotation happens in `ssao.slang`
    /// before the channel is ever written. `crcbl_shaders::ssr::SsrParams`
    /// carries the same member under the same name for the same reason.
    ///
    /// [`inv_proj`]: Self::inv_proj
    pub inv_view: [f32; 16],
    /// The sampling radius, in world units.
    ///
    /// A depth bias sat beside it until GTAO replaced the hemisphere of depth
    /// comparisons that needed one; `ssao.slang` says on `SsaoParams::params`
    /// why a horizon integral does not. [`slices`] is what took the slot it
    /// left.
    ///
    /// [`slices`]: Self::slices
    pub radius: f32,
    /// Planes through the eye each pixel sweeps for a horizon.
    ///
    /// [`SLICE_COUNT_DEFAULT`] is the floor and [`SLICE_COUNT_MAX`] the
    /// ceiling; `ssao.slang`'s `slice_count` clamps whatever arrives into that
    /// range, so a producer that writes nothing here gets the floor rather than
    /// a frame with no slices in it. What the engine *ships* is the ceiling —
    /// `r_ssao_slices` has defaulted to it since 2026-09-03.
    ///
    /// A `u8` because the range is that small and because the block carries it
    /// as a float: every value it can hold converts without rounding, which is
    /// what lets the shader read it back with a `uint` cast rather than a
    /// nearest-integer search.
    pub slices: u8,
    /// The exponent `ssao_upsample.slang` raises the reconstructed visibility
    /// to, which is how much occlusion a frame asks for against how much the
    /// horizons measured.
    ///
    /// [`INTENSITY_DEFAULT`] is what ships and returns the measurement
    /// untouched; [`INTENSITY_MIN`] and [`INTENSITY_MAX`] are the ends the
    /// shader clamps into. **A zero is answered with [`INTENSITY_DEFAULT`]**
    /// rather than with the floor, on [`slices`]' terms and for a sharper
    /// version of its reason: a producer that writes nothing leaves this word
    /// as the padding it used to be, and every visibility raised to zero is
    /// one — a frame with no occlusion in it at all. `ao_intensity` in the
    /// shader is where that is turned away.
    ///
    /// [`slices`]: Self::slices
    pub intensity: f32,
    /// Whether the gather writes a bent direction beside the scalar.
    ///
    /// **A `false` is what an unwritten word means**, and that is the
    /// degenerate this way round deliberately: the lane used to be the row's
    /// padding, an unwritten uniform buffer reads as zero there, and a frame
    /// with no bent direction is the frame the chain drew before the channel
    /// was widened — every consumer answers the sentinel with the shading
    /// normal it already had. [`slices`] cannot be defaulted that way, which is
    /// why the two lanes turn an unwritten block away in opposite directions.
    ///
    /// `crcbl_render::ssao::r_ssao_bent_normals` is what a frame sets it from,
    /// and `bent_normals` in `shaders/ssao.slang` is the reader.
    ///
    /// [`slices`]: Self::slices
    pub bent_normals: bool,
}

impl SsaoParams {
    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian throughout, and every word of the row is written: the
    /// buffer is [`PARAMS_SIZE`] wide and a partial write leaves the tail
    /// undefined, which is [`crate::compute_probe::Params`]'s reason. The row's
    /// last word was that padding until [`bent_normals`] took it.
    ///
    /// [`bent_normals`]: Self::bent_normals
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
        bytes[at..at + 4].copy_from_slice(&self.radius.to_le_bytes());
        at += 4;
        bytes[at..at + 4].copy_from_slice(&f32::from(self.slices).to_le_bytes());
        at += 4;
        bytes[at..at + 4].copy_from_slice(&self.intensity.to_le_bytes());
        at += 4;
        // The switch rides as a float because the row is a `float4`, and the
        // shader tests it against zero rather than for equality with one — see
        // `bent_normals` there.
        bytes[at..at + 4].copy_from_slice(&f32::from(u8::from(self.bent_normals)).to_le_bytes());
        at += 4;
        debug_assert_eq!(at, PARAMS_SIZE, "the row closes the block");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three shaders the occlusion channel passes through, in the order it
    /// passes through them.
    ///
    /// The constants below are declared in more than one of them — they are one
    /// channel's worth of agreement rather than three shaders' opinions — and
    /// this is the list the checks sweep. A pass added to the chain and left off
    /// this list is a copy nothing holds.
    const OCCLUSION_SOURCES: [(&str, &str); 3] = [
        ("ssao.slang", include_str!("../shaders/ssao.slang")),
        (
            "ssao_blur.slang",
            include_str!("../shaders/ssao_blur.slang"),
        ),
        (
            "ssao_upsample.slang",
            include_str!("../shaders/ssao_upsample.slang"),
        ),
    ];

    /// The constant and the shaders must name the same far plane.
    ///
    /// Nothing else can catch this: the shaders compile either way, and a
    /// mismatch shows up only as an unoccluded sky or a division by zero on
    /// whatever machine happens to look. Reading the source is the check, and the
    /// source is hash-pinned by the manifest, so it is the same file the
    /// committed artifact was built from.
    #[test]
    fn the_far_plane_matches_the_constant_ssao_slang_declares() {
        let declaration = format!("static const float DEPTH_FAR = {DEPTH_FAR:.1};");
        for (name, source) in OCCLUSION_SOURCES {
            assert!(
                source.contains(&declaration),
                "{name} does not declare `{declaration}`; DEPTH_FAR has drifted from the shaders"
            );
        }
    }

    /// The slice counts and the shader must name the same two numbers.
    ///
    /// `the_far_plane_matches_the_constant_ssao_slang_declares`'s check exactly,
    /// for its reason: the shader compiles with any pair of these, and a
    /// disagreement shows up only as a frame drawn at a count nobody chose —
    /// a host asking for four while the shader clamps to two is a silent
    /// no-op, and a floor that drifted below two is every surface fully lit.
    #[test]
    fn the_slice_counts_match_the_constants_ssao_slang_declares() {
        let source = include_str!("../shaders/ssao.slang");
        for declaration in [
            format!("static const uint SLICE_COUNT_MAX = {SLICE_COUNT_MAX};"),
            format!("static const uint SLICE_COUNT_DEFAULT = {SLICE_COUNT_DEFAULT};"),
        ] {
            assert!(
                source.contains(&declaration),
                "ssao.slang does not declare `{declaration}`; the slice counts have drifted from \
                 the shader"
            );
        }
        assert!(
            source.contains("clamp(uint(camera.params.y), SLICE_COUNT_DEFAULT, SLICE_COUNT_MAX)"),
            "ssao.slang no longer clamps the requested count into those two, so a block whose \
             `params.y` was never written sweeps no slices and lights every surface"
        );
    }

    /// Every shader of the occlusion trio must divide the extent by the same
    /// number.
    ///
    /// `the_far_plane_matches_the_constant_ssao_slang_declares`'s check, and the
    /// failure it catches is worse: three passes reading each other's images
    /// while disagreeing about how the two grids line up still compile and still
    /// draw. The blur would weight a tap by a stranger's depth and the upsample
    /// would reconstruct a field shifted across the frame, and both are
    /// pictures.
    #[test]
    fn the_resolution_divisor_matches_the_constant_the_shaders_declare() {
        let declaration = format!("static const int RESOLUTION_DIVISOR = {RESOLUTION_DIVISOR};");
        for (name, source) in OCCLUSION_SOURCES {
            assert!(
                source.contains(&declaration),
                "{name} does not declare `{declaration}`; RESOLUTION_DIVISOR has drifted from \
                 the shaders"
            );
        }
    }

    /// The reconstruction must weigh each tap by its **depth**, not only by its
    /// distance.
    ///
    /// **No golden notices if it stops.** Measured 2026-09-02 by replacing
    /// `ssao_upsample.slang`'s ramp with a constant one, which is a plain
    /// bilinear upsample: `crcbl`'s `render_e2e` still reported 31 of 32 passing
    /// with the same one golden over tolerance, and lantern's golden still
    /// matched. The goldens are rendered at fixture sizes where the
    /// half-resolution grid is a few samples across a silhouette, so the halo a
    /// distance-only weight draws is under every tolerance they carry — and the
    /// frame it is visible in is the 1920×1080 one no golden runs at.
    ///
    /// **`crcbl`'s `forward_e2e::occlusion::the_reconstruction_does_not_halo_a_silhouette`
    /// is the behavioural check, and it runs that exact frame**: the same
    /// sabotage makes a bar standing in front of a wall dip 45 levels there
    /// against a bound of 4. This test is kept beside it rather than replaced by
    /// it because that one is `#[ignore]`d and needs a GPU — it runs only under
    /// `run-forward-e2e.sh`, so on a machine or a CI job with no device the
    /// source check is the only thing left standing.
    ///
    /// It names the two halves that make the reconstruction depth-aware: the
    /// ramp on the view-space difference, and the far-plane rejection that keeps
    /// the sky out of a silhouette's rim. A shader that lost either is a shader
    /// this test is about. Being a source check, it fails on a *rewording* that
    /// is still bilateral; the behavioural test is what settles that case.
    #[test]
    fn the_reconstruction_weighs_a_tap_by_its_depth() {
        let source = include_str!("../shaders/ssao_upsample.slang");
        for expression in [
            "saturate(1.0 - away / tolerance)",
            "depth <= DEPTH_FAR ? 0.0 :",
        ] {
            assert!(
                source.contains(expression),
                "ssao_upsample.slang no longer contains `{expression}`, so its taps may be \
                 weighted by distance alone -- which is a plain bilinear upsample, and a halo \
                 along every silhouette that no golden in this workspace is rendered large \
                 enough to catch"
            );
        }
    }

    /// The blur and the upsample must reject a tap at the same distance.
    ///
    /// They are two filters over the same two images answering the same question
    /// — is this sample on the surface being written — and a tolerance that
    /// drifted between them is a silhouette one of them crosses and the other
    /// does not, which is a halo in the reconstruction that the blur's own
    /// rejection was chosen to prevent. Neither shader has a Rust mirror of the
    /// number, so this compares the two declarations against each other rather
    /// than against a constant here.
    #[test]
    fn the_two_occlusion_filters_share_one_depth_tolerance() {
        let declaration = "static const float DEPTH_TOLERANCE_RADII = ";
        let tolerance_of = |source: &str| {
            let at = source
                .find(declaration)
                .map(|at| at + declaration.len())
                .expect("both filters declare a depth tolerance");
            source[at..]
                .split(';')
                .next()
                .expect("the declaration ends in a semicolon")
                .to_string()
        };
        let blur = tolerance_of(include_str!("../shaders/ssao_blur.slang"));
        let upsample = tolerance_of(include_str!("../shaders/ssao_upsample.slang"));
        assert_eq!(
            blur, upsample,
            "ssao_blur.slang rejects a tap at {blur} radii and ssao_upsample.slang at \
             {upsample}; the two filters have drifted"
        );
    }

    /// The block the shader declares, member for member and in this order.
    ///
    /// The offsets below are what [`SsaoParams::to_bytes`] writes, so this is the
    /// check that the *shader* agrees about which matrix comes first — swapping
    /// them produces a frame that is occluded everywhere or nowhere, and both are
    /// pictures.
    #[test]
    fn the_uniform_block_matches_the_struct_ssao_slang_declares() {
        let source = include_str!("../shaders/ssao.slang");
        for declaration in [
            "float4x4 inv_proj;",
            "float4x4 proj;",
            "float4x4 inv_view;",
            "float4 params;",
        ] {
            assert!(
                source.contains(declaration),
                "ssao.slang does not declare `{declaration}`"
            );
        }
        let inv_proj = source.find("float4x4 inv_proj;").expect("just checked");
        let proj = source.find("float4x4 proj;").expect("just checked");
        let inv_view = source.find("float4x4 inv_view;").expect("just checked");
        let params = source.find("float4 params;").expect("just checked");
        assert!(
            inv_proj < proj && proj < inv_view && inv_view < params,
            "ssao.slang declares the block in a different order than `to_bytes` writes it"
        );
    }

    /// [`MAX_ACOS_ERROR`] is the bound it says it is, swept over the domain.
    ///
    /// Two million steps across `-1..=1`, against `f64::acos` — the reference
    /// here is the *wider* type deliberately, so the sweep measures the
    /// polynomial rather than `f32::acos`'s own last bit.
    ///
    /// **The bound is asserted from both sides.** A ceiling alone passes on a
    /// function that got better, which sounds harmless and is how a bound stops
    /// describing anything: nothing would notice if the polynomial were replaced
    /// by the intrinsic, which is the one substitution [`acos_approx`] exists to
    /// refuse. So the sweep also asserts the worst case is at least half the
    /// bound, and the endpoints exactly.
    #[test]
    fn the_arc_cosine_polynomial_is_as_accurate_as_it_claims() {
        const STEPS: u32 = 2_000_000;
        let mut worst = 0.0f64;
        let mut worst_at = 0.0f64;
        for step in 0..=STEPS {
            let x = f64::from(step) / f64::from(STEPS) * 2.0 - 1.0;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the argument is the thing under test and it is an f32 in the shader"
            )]
            let error = (f64::from(acos_approx(x as f32)) - x.acos()).abs();
            if error > worst {
                worst = error;
                worst_at = x;
            }
        }
        assert!(
            worst <= MAX_ACOS_ERROR,
            "acos_approx is off by {worst:e} rad at x = {worst_at}, over MAX_ACOS_ERROR \
             ({MAX_ACOS_ERROR:e})"
        );
        assert!(
            worst >= MAX_ACOS_ERROR / 2.0,
            "acos_approx is off by only {worst:e} rad — MAX_ACOS_ERROR ({MAX_ACOS_ERROR:e}) no \
             longer describes this function, and a bound nothing approaches would pass on the \
             intrinsic this polynomial exists to refuse"
        );
        assert_eq!(
            acos_approx(1.0),
            0.0,
            "a grazing horizon must be exactly zero"
        );
        assert_eq!(
            acos_approx(-1.0),
            std::f32::consts::PI,
            "the reflected endpoint must be exactly pi"
        );
    }

    /// `ssao.slang` evaluates [`ACOS_KERNEL`], coefficient for coefficient.
    ///
    /// There is no `#include` in these shaders, so the polynomial exists twice
    /// and nothing but this holds the copies together — `crate::fog`'s
    /// `the_shader_spells_the_same_constants` for the same reason. A slip in one
    /// digit compiles, renders a frame nobody would question, and moves every
    /// horizon in it by an amount no golden blessed after the slip can see.
    ///
    /// Compared as **values** and not as text: `1.570_728_8` in Rust is
    /// `1.5707288` in Slang, and the same number spelled two ways.
    #[test]
    fn the_shader_evaluates_this_modules_polynomial() {
        let source = include_str!("../shaders/ssao.slang");
        let declaration = "static const float ACOS_KERNEL[";
        let at = source
            .find(declaration)
            .expect("ssao.slang declares no ACOS_KERNEL");
        let rest = &source[at..];
        let open = rest.find('{').expect("an initialiser opens with a brace");
        let close = rest.find('}').expect("an initialiser closes with a brace");
        let shader: Vec<f32> = rest[open + 1..close]
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                value
                    .parse()
                    .unwrap_or_else(|error| panic!("ACOS_KERNEL holds a non-float: {error}"))
            })
            .collect();
        assert_eq!(
            shader.as_slice(),
            ACOS_KERNEL.as_slice(),
            "ssao.slang's ACOS_KERNEL has drifted from this module's"
        );
    }

    /// The tilt inside a slice is signed against the **tangent**, not against
    /// the screen direction lifted into view space.
    ///
    /// The two agree only at the exact centre of the frame, where `view` is the
    /// view axis. Everywhere else `in_plane` has a component along `view`, so
    /// signing with it leans `gamma` the wrong way and puts both horizon clamps
    /// on the wrong sides of the surface — which draws a smooth wash over every
    /// flat surface, growing towards the frame's edges, that reads as a vignette
    /// and not as a bug. It survived a whole `render_e2e` run before
    /// `the_probes_scene_lights_its_room_and_matches_its_golden`'s flatness
    /// assertion caught it, and it is the shape a golden blessed on top of it
    /// would have hidden forever.
    ///
    /// A source check, because the value is a geometric relationship no unit
    /// test on this side of the crate boundary can evaluate: the crate does not
    /// carry a depth buffer to run the pass over.
    #[test]
    fn the_slice_tilt_is_signed_against_the_view_orthogonal_tangent() {
        let source = include_str!("../shaders/ssao.slang");
        for expression in [
            "float3 tangent = cross(view, axis);",
            "float sign_gamma = dot(tangent, projected) < 0.0 ? -1.0 : 1.0;",
        ] {
            assert!(
                source.contains(expression),
                "ssao.slang no longer spells `{expression}`; the slice tilt is signed by \
                 something else and every off-centre pixel is occluded by its own flat surface"
            );
        }
        assert!(
            !source.contains("dot(in_plane, projected)"),
            "ssao.slang signs the slice tilt against `in_plane`, which is not perpendicular to \
             `view` away from the frame's centre"
        );
    }

    /// The layout claim, checked rather than asserted in prose.
    #[test]
    fn the_block_is_three_matrices_and_a_row() {
        let mut inv_proj = [0.0f32; 16];
        inv_proj[0] = 1.0;
        let mut proj = [0.0f32; 16];
        proj[15] = 2.0;
        let mut inv_view = [0.0f32; 16];
        inv_view[0] = 3.0;
        inv_view[15] = 4.0;
        let params = SsaoParams {
            inv_proj,
            proj,
            inv_view,
            radius: 0.5,
            slices: SLICE_COUNT_MAX,
            intensity: INTENSITY_MAX,
            bent_normals: true,
        };
        let bytes = params.to_bytes();

        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[124..128], &2.0f32.to_le_bytes());
        // The third matrix, which is where the bent direction's rotation rides
        // — swapping it with either of the two above draws a picture rather
        // than failing, so both of its ends are read.
        assert_eq!(&bytes[128..132], &3.0f32.to_le_bytes());
        assert_eq!(&bytes[188..192], &4.0f32.to_le_bytes());
        assert_eq!(&bytes[192..196], &0.5f32.to_le_bytes());
        // The count rides in `params.y` as a float, and **the whole point is
        // that the trip is exact**: the shader casts it straight back to a
        // `uint`, so a value that arrived a hair under would floor to one less
        // and sweep a plane fewer than the host asked for.
        assert_eq!(
            &bytes[196..200],
            &f32::from(SLICE_COUNT_MAX).to_le_bytes(),
            "the slice count did not survive the block as the number it went in as"
        );
        // The intensity rides in `params.z`, which the row used to pad with.
        // The shader compares it against its own `INTENSITY_DEFAULT` to decide
        // whether to touch the frame at all, so a value that did not arrive
        // exactly is a golden that moved for a knob nobody set.
        assert_eq!(
            &bytes[200..204],
            &INTENSITY_MAX.to_le_bytes(),
            "the AO intensity did not survive the block as the number it went in as"
        );
        // And `params.w`, which was the row's last padding word until the bent
        // direction took it. The shader tests it against zero, so what matters
        // is that an asked-for direction does not arrive as one.
        assert_eq!(
            &bytes[204..208],
            &1.0f32.to_le_bytes(),
            "the bent-direction switch did not survive the block"
        );
        let off = SsaoParams {
            bent_normals: false,
            ..params
        }
        .to_bytes();
        assert_eq!(
            &off[204..208],
            &0.0f32.to_le_bytes(),
            "a frame that asked for no bent direction has to reach the shader as the zero the \
             lane held when it was padding"
        );
    }

    /// The bent-direction constants and the shaders must name the same numbers.
    ///
    /// `the_far_plane_matches_the_constant_ssao_slang_declares`'s check, for its
    /// reason, and here the failure is quieter than most: a threshold that
    /// drifted in one of the four declarations is a filter calling a cancelled
    /// average a direction — or calling a real one the sentinel — which draws a
    /// frame lit slightly wrongly and reports nothing.
    ///
    /// **All four**, because the mean is thresholded in the gather and in both
    /// filters and the decoded length is thresholded in `mesh.slang`; a shader
    /// left off this list is a copy nothing holds. `mesh.slang` is not in
    /// [`OCCLUSION_SOURCES`] — it is the consumer rather than a pass of the
    /// chain — so it is named here.
    #[test]
    fn the_bent_normal_length_matches_the_constant_the_shaders_declare() {
        let declaration =
            format!("static const float BENT_NORMAL_MIN_LENGTH = {BENT_NORMAL_MIN_LENGTH};");
        for (name, source) in OCCLUSION_SOURCES
            .into_iter()
            .chain([("mesh.slang", include_str!("../shaders/mesh.slang"))])
        {
            assert!(
                source.contains(&declaration),
                "{name} does not declare `{declaration}`; the bent-direction threshold has \
                 drifted from the shaders"
            );
        }
    }

    /// The encoded sentinel is the byte [`BENT_NORMAL_NONE`] names, and it
    /// decodes to a length the threshold rejects.
    ///
    /// **This is the whole of why the placeholder and a gathered pixel can be
    /// the same case.** The shader writes `direction * 0.5 + 0.5` and the
    /// target quantises; if that byte were not the placeholder's, a frame with
    /// no occlusion pass would decode to some direction and the ambient term
    /// would be steered by a rounding step. Both halves are arithmetic this
    /// crate can do, so neither is left to a comment.
    #[test]
    fn the_zero_direction_encodes_to_the_placeholders_byte() {
        let encoded = 0.0f32.mul_add(0.5, 0.5);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the product is a channel level in 0..=255 by construction"
        )]
        let byte = (encoded * 255.0 + 0.5) as u8;
        assert_eq!(
            byte, BENT_NORMAL_NONE,
            "the zero direction does not quantise to the byte the renderer's placeholder holds"
        );
        let decoded = f32::from(BENT_NORMAL_NONE) / 255.0 * 2.0 - 1.0;
        let length = (3.0f32 * decoded * decoded).sqrt();
        assert!(
            length < BENT_NORMAL_MIN_LENGTH,
            "the sentinel decodes to a length of {length}, which BENT_NORMAL_MIN_LENGTH \
             ({BENT_NORMAL_MIN_LENGTH}) does not reject"
        );
        // And from the other side, so the threshold is not a bound nothing
        // approaches: a unit direction survives the same quantisation, and the
        // worst it can come back as is a channel level short on every axis.
        let worst = 1.0f32 / 3.0f32.sqrt() - 1.0 / 255.0;
        let unit = (3.0f32 * worst * worst).sqrt();
        assert!(
            unit > BENT_NORMAL_MIN_LENGTH,
            "a quantised unit direction comes back at {unit}, which BENT_NORMAL_MIN_LENGTH \
             ({BENT_NORMAL_MIN_LENGTH}) rejects as a sentinel"
        );
    }

    /// The gather must accumulate a bent direction and rotate it into world
    /// space, and the two filters must renormalise what they average.
    ///
    /// **No golden notices if either stops**, which is
    /// `the_reconstruction_weighs_a_tap_by_its_depth`'s situation exactly: a
    /// constant direction and a correctly-varying one both light a 256×192
    /// fixture within its tolerance, and a direction that is merely the wrong
    /// *length* changes nothing at all until something normalises it. The
    /// behavioural checks are `crcbl`'s
    /// `forward_e2e::occlusion::the_bent_direction_leans_out_of_the_occluded_corner`
    /// and `the_filters_leave_the_bent_direction_a_unit_vector`, and both are
    /// `#[ignore]`d and need a GPU — so on a machine or a CI job with no device
    /// these source checks are the only thing left standing.
    #[test]
    fn the_bent_direction_is_gathered_filtered_and_renormalised() {
        assert!(
            include_str!("../shaders/ssao.slang")
                .contains("normalize(mul(camera.inv_view, float4(bent, 0.0)).xyz)"),
            "ssao.slang no longer rotates the accumulated bent direction into world space, so \
             `mesh.slang` would steer its ambient term by a view-space vector and the lighting \
             would swing with the camera"
        );
        for (name, source) in [
            (
                "ssao_blur.slang",
                include_str!("../shaders/ssao_blur.slang"),
            ),
            (
                "ssao_upsample.slang",
                include_str!("../shaders/ssao_upsample.slang"),
            ),
        ] {
            assert!(
                source.contains("float3 mean = weight > 0.0 ? summed / weight"),
                "{name}'s `encode_bent` no longer takes the mean of its taps"
            );
            assert!(
                source.contains(": normalize(mean);"),
                "{name}'s `encode_bent` no longer renormalises, so the direction it writes \
                 carries the filter's own disagreement as a length"
            );
        }
    }

    /// The three intensities and the shader must name the same numbers.
    ///
    /// `the_far_plane_matches_the_constant_ssao_slang_declares`'s check, for its
    /// reason: the shader compiles with any of them, and a ceiling that drifted
    /// above the console's range is a value the host can ask for and the frame
    /// silently clamps, while a default that drifted off one is every golden in
    /// the tree moving for a knob nobody set.
    #[test]
    fn the_intensity_bounds_match_the_constants_ssao_upsample_slang_declares() {
        let source = include_str!("../shaders/ssao_upsample.slang");
        for declaration in [
            format!("static const float INTENSITY_DEFAULT = {INTENSITY_DEFAULT:.1};"),
            format!("static const float INTENSITY_MIN = {INTENSITY_MIN};"),
            format!("static const float INTENSITY_MAX = {INTENSITY_MAX:.1};"),
        ] {
            assert!(
                source.contains(&declaration),
                "ssao_upsample.slang does not declare `{declaration}`; the intensity bounds \
                 have drifted from the shader"
            );
        }
    }

    /// An unwritten `params.z` must be the default, and the default must be
    /// applied by **not applying anything**.
    ///
    /// Two failures, both silent and both pictures. A zero clamped into the
    /// range rather than answered with [`INTENSITY_DEFAULT`] is a producer that
    /// wrote nothing getting a frame with no occlusion at all — every
    /// visibility raised to zero is one — which is the failure
    /// `the_slice_counts_match_the_constants_ssao_slang_declares` guards the
    /// other field against. And a shader that reached `pow` at the default
    /// would move every golden in this workspace by however much a target's
    /// logarithm and exponential disagree with the identity, for a knob nobody
    /// touched.
    ///
    /// A source check for `the_slice_tilt_is_signed_against_the_view_orthogonal_tangent`'s
    /// reason: this crate carries no depth buffer to run the pass over. The
    /// behavioural half is `crcbl`'s
    /// `forward_e2e::occlusion::the_ao_intensity_scales_the_reconstructed_occlusion`,
    /// which reads both cases back off a device.
    #[test]
    fn the_reconstruction_answers_an_unwritten_intensity_with_the_default() {
        let source = include_str!("../shaders/ssao_upsample.slang");
        for expression in [
            "asked == 0.0 ? INTENSITY_DEFAULT : clamp(asked, INTENSITY_MIN, INTENSITY_MAX)",
            "intensity == INTENSITY_DEFAULT ? visibility : pow(visibility, intensity)",
        ] {
            assert!(
                source.contains(expression),
                "ssao_upsample.slang no longer spells `{expression}`; either a block whose \
                 `params.z` was never written switches the occlusion off entirely, or the \
                 default no longer returns the visibility the horizons measured"
            );
        }
    }
}
