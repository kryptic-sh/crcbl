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

/// Bytes of the uniform block: two `float4x4` and one `float4` row.
///
/// `std140` gives a `float4x4` four sixteen-byte columns and a `float4` one row,
/// and the total is already a multiple of sixteen, so there is no tail padding
/// to write. See [`SsaoParams::to_bytes`].
pub const PARAMS_SIZE: usize = 64 + 64 + 16;

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
    /// The sampling radius, in world units.
    ///
    /// The only scalar the block carries. A depth bias sat beside it until GTAO
    /// replaced the hemisphere of depth comparisons that needed one; `ssao.slang`
    /// says on `SsaoParams::params` why a horizon integral does not.
    pub radius: f32,
}

impl SsaoParams {
    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian throughout, and the three padding words after [`radius`]
    /// are written rather than left alone for [`crate::compute_probe::Params`]'s
    /// reason: the buffer is [`PARAMS_SIZE`] wide and a partial write leaves the
    /// tail undefined.
    ///
    /// [`radius`]: Self::radius
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0;
        for value in self.inv_proj.into_iter().chain(self.proj) {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        bytes[at..at + 4].copy_from_slice(&self.radius.to_le_bytes());
        at += 4;
        debug_assert_eq!(at + 12, PARAMS_SIZE, "three padding words close the row");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant and the shader must name the same far plane.
    ///
    /// Nothing else can catch this: the shader compiles either way, and a
    /// mismatch shows up only as an unoccluded sky or a division by zero on
    /// whatever machine happens to look. Reading the source is the check, and the
    /// source is hash-pinned by the manifest, so it is the same file the
    /// committed artifact was built from.
    #[test]
    fn the_far_plane_matches_the_constant_ssao_slang_declares() {
        let source = include_str!("../shaders/ssao.slang");
        let declaration = format!("static const float DEPTH_FAR = {DEPTH_FAR:.1};");
        assert!(
            source.contains(&declaration),
            "ssao.slang does not declare `{declaration}`; DEPTH_FAR has drifted from the shader"
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
        for declaration in ["float4x4 inv_proj;", "float4x4 proj;", "float4 params;"] {
            assert!(
                source.contains(declaration),
                "ssao.slang does not declare `{declaration}`"
            );
        }
        let inv_proj = source.find("float4x4 inv_proj;").expect("just checked");
        let proj = source.find("float4x4 proj;").expect("just checked");
        let params = source.find("float4 params;").expect("just checked");
        assert!(
            inv_proj < proj && proj < params,
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
    fn the_block_is_two_matrices_and_a_padded_row() {
        let mut inv_proj = [0.0f32; 16];
        inv_proj[0] = 1.0;
        let mut proj = [0.0f32; 16];
        proj[15] = 2.0;
        let bytes = SsaoParams {
            inv_proj,
            proj,
            radius: 0.5,
        }
        .to_bytes();

        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[124..128], &2.0f32.to_le_bytes());
        assert_eq!(&bytes[128..132], &0.5f32.to_le_bytes());
        assert!(bytes[132..].iter().all(|byte| *byte == 0), "{bytes:?}");
    }
}
