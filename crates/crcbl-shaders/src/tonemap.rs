//! The uniform block `tonemap.slang` reads, in the layout that shader declares.
//!
//! Same reason as [`crate::ssao`]: the shader fixes a byte layout, every
//! producer of those bytes has to agree with it exactly, and keeping the mirror
//! in the crate that owns the source means there is one place to change rather
//! than one per consumer.

/// Bytes of the uniform block: a `float` and a `uint`, in a row of their own.
///
/// `std140` rounds a block up to a multiple of sixteen, so the two values are
/// followed by two padding words rather than sitting in an eight-byte buffer.
/// See [`TonemapParams::to_bytes`].
pub const PARAMS_SIZE: usize = 16;

/// The exposure a renderer nobody has configured applies.
///
/// **`1.0`, which is what `static const float EXPOSURE` held while the value was
/// a compile-time constant** — so a caller that never touches it draws the frame
/// this engine drew before the block existed, and every golden image is
/// unchanged. It is also the value at which the operator is the identity on
/// `[0, 1]`, which is the argument `tonemap.slang`'s header makes for
/// exposure-and-clamp over a curve.
pub const DEFAULT_EXPOSURE: f32 = 1.0;

/// Which operator `tonemap.slang` runs, mirroring its `CURVE_*` constants.
///
/// **The clamp is the default and the identity on `0..=1`.** Every 2D sample in
/// the tree is display-referred already, so a curve applied to it would move
/// colours an artist chose; a curve is something a view asks for, the way an
/// effect bit is — see `crcbl_render::ForwardRenderer::set_tonemap_curve`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u32)]
pub enum TonemapCurve {
    /// Multiply by the exposure and clamp. What this pass shipped with.
    #[default]
    Clamp = 0,
    /// The ACES filmic curve, as Stephen Hill's fit of the RRT and the ODT.
    ///
    /// Two changes of primaries around a rational polynomial, and **no
    /// transcendental function anywhere in it** — which is what makes it
    /// blessable across four backends, and why it is here rather than AgX.
    /// [`TonemapCurve::apply`] is the arithmetic, spelled the way the shader
    /// spells it.
    Aces = 1,
}

/// Into ACES-relative primaries, the fit's first step. Rows, as the shader
/// declares them.
const ACES_INPUT: [[f32; 3]; 3] = [
    [0.59719, 0.35458, 0.04823],
    [0.07600, 0.90834, 0.01566],
    [0.02840, 0.13383, 0.83777],
];

/// And back to display primaries, the fit's last step.
const ACES_OUTPUT: [[f32; 3]; 3] = [
    [1.60475, -0.53108, -0.07367],
    [-0.10208, 1.10813, -0.00605],
    [-0.00327, -0.07276, 1.07602],
];

/// `matrix * color`, rows dotted against the column — which is what `mul` does
/// in Slang for a `float3x3` on the left.
fn transform(matrix: &[[f32; 3]; 3], color: [f32; 3]) -> [f32; 3] {
    let mut out = [0.0f32; 3];
    for (row, value) in matrix.iter().zip(out.iter_mut()) {
        *value = row[0] * color[0] + row[1] * color[1] + row[2] * color[2];
    }
    out
}

impl TonemapCurve {
    /// The selector value the shader compares against.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self as u32
    }

    /// The operator on the CPU, for anything that has to predict what the pass
    /// will write.
    ///
    /// **A second copy of arithmetic the shader also carries, and deliberately
    /// so.** A fit is a transcription of published constants, and a slip in one
    /// of twenty-three digits compiles, renders a plausible picture and passes
    /// every golden that was blessed after the slip. This copy is what the
    /// specification's own anchors are asserted against — a neutral stays
    /// neutral, and mid-grey lands at a tenth — and
    /// `the_shader_spells_the_same_constants` is what stops the two drifting.
    #[must_use]
    pub fn apply(self, color: [f32; 3], exposure: f32) -> [f32; 3] {
        let mut exposed = color;
        for channel in &mut exposed {
            *channel *= exposure;
        }
        match self {
            Self::Clamp => saturate(exposed),
            Self::Aces => {
                let mut fitted = transform(&ACES_INPUT, exposed);
                for channel in &mut fitted {
                    let numerator = *channel * (*channel + 0.0245786) - 0.000090537;
                    let denominator = *channel * (0.983729 * *channel + 0.432951) + 0.238081;
                    *channel = numerator / denominator;
                }
                saturate(transform(&ACES_OUTPUT, fitted))
            }
        }
    }
}

/// Each channel clamped to `0..=1`, which is what `saturate` does in a shader.
fn saturate(color: [f32; 3]) -> [f32; 3] {
    [
        color[0].clamp(0.0, 1.0),
        color[1].clamp(0.0, 1.0),
        color[2].clamp(0.0, 1.0),
    ]
}

/// The uniform block, matching `struct TonemapParams` in `shaders/tonemap.slang`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TonemapParams {
    /// The multiplier applied to the scene colour before the operator.
    pub exposure: f32,
    /// Which operator runs on the exposed colour.
    pub curve: TonemapCurve,
}

impl Default for TonemapParams {
    /// [`DEFAULT_EXPOSURE`], so a block built without an opinion is the one the
    /// compile-time constant used to produce.
    fn default() -> Self {
        Self {
            exposure: DEFAULT_EXPOSURE,
            curve: TonemapCurve::Clamp,
        }
    }
}

impl TonemapParams {
    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian, and the two padding words after [`curve`] are written
    /// rather than left alone for [`crate::ssao::SsaoParams::to_bytes`]'s reason:
    /// the buffer is [`PARAMS_SIZE`] wide and a partial write leaves the tail
    /// undefined.
    ///
    /// [`curve`]: Self::curve
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        bytes[..4].copy_from_slice(&self.exposure.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.curve.as_u32().to_le_bytes());
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The block the shader declares, member for member.
    ///
    /// Nothing else can catch a rename or a reorder: the shader compiles either
    /// way and the buffer is bound either way, and a block whose one member moved
    /// would read a padding word as the exposure and draw a black frame. Reading
    /// the source is the check, and the source is hash-pinned by the manifest, so
    /// it is the same file the committed artifact was built from.
    #[test]
    fn the_uniform_block_matches_the_struct_tonemap_slang_declares() {
        let source = include_str!("../shaders/tonemap.slang");
        assert!(
            source.contains("float exposure;"),
            "tonemap.slang does not declare `float exposure;`"
        );
        assert!(
            source.contains("uint curve;"),
            "tonemap.slang does not declare `uint curve;`"
        );
        assert!(
            source.contains("ConstantBuffer<TonemapParams> params;"),
            "tonemap.slang does not bind the block `to_bytes` writes"
        );
    }

    /// The selector values the two sides compare against are the same values.
    #[test]
    fn the_curve_selectors_match_the_constants_the_shader_declares() {
        let source = include_str!("../shaders/tonemap.slang");
        for (curve, spelling) in [
            (TonemapCurve::Clamp, "static const uint CURVE_CLAMP = 0u;"),
            (TonemapCurve::Aces, "static const uint CURVE_ACES = 1u;"),
        ] {
            assert!(
                source.contains(spelling),
                "tonemap.slang does not declare `{spelling}`, so {curve:?} selects nothing"
            );
        }
        assert_eq!(TonemapCurve::Clamp.as_u32(), 0);
        assert_eq!(TonemapCurve::Aces.as_u32(), 1);
    }

    /// **Every constant of the fit appears in the shader, spelled the same
    /// way.**
    ///
    /// [`TonemapCurve::apply`] is a transcription of published constants and so
    /// is the shader's copy, and the tests below pin only this one. Nothing else
    /// in the tree can see a digit that differs between them: both compile, both
    /// render a plausible frame, and a golden blessed after a slip enshrines it.
    /// A grep over the source is crude and it is decisive — the file is
    /// hash-pinned by the manifest, so it is the same source the committed
    /// artifacts were built from.
    #[test]
    fn the_shader_spells_the_same_constants() {
        let source = include_str!("../shaders/tonemap.slang");
        let matrices = ACES_INPUT.iter().chain(ACES_OUTPUT.iter()).flatten();
        for value in matrices {
            // The shader writes them without a trailing zero and with the sign
            // attached, which is how the published fit writes them.
            let spelling = format!("{value}");
            assert!(
                source.contains(&spelling),
                "tonemap.slang does not contain the matrix constant `{spelling}`"
            );
        }
        for spelling in [
            "v * (v + 0.0245786) - 0.000090537",
            "v * (0.983729 * v + 0.432951) + 0.238081",
        ] {
            assert!(
                source.contains(spelling),
                "tonemap.slang does not contain the polynomial `{spelling}`"
            );
        }
    }

    /// **A neutral goes in and a neutral comes out**, which is the fit's own
    /// structural invariant and the cheapest typo catcher there is.
    ///
    /// Both matrices have rows summing to one, so a grey maps to a grey however
    /// the polynomial between them behaves. One wrong digit anywhere in the
    /// eighteen breaks that, and breaks it visibly: the frame takes a colour
    /// cast no golden was blessed with.
    #[test]
    fn a_neutral_stays_neutral_through_the_aces_fit() {
        for matrix in [&ACES_INPUT, &ACES_OUTPUT] {
            for row in matrix {
                let sum = row[0] + row[1] + row[2];
                assert!(
                    (sum - 1.0).abs() < 1e-4,
                    "a row of the fit sums to {sum}, not one: {row:?}"
                );
            }
        }
        for input in [0.0f32, 0.18, 0.5, 1.0, 4.0] {
            let [r, g, b] = TonemapCurve::Aces.apply([input, input, input], 1.0);
            assert!(
                (r - g).abs() < 1e-3 && (g - b).abs() < 1e-3,
                "a grey of {input} came out as {r}, {g}, {b}"
            );
        }
    }

    /// **Mid-grey lands at a tenth**, which is the ACES output transform's
    /// published anchor and not a number this file computed for itself.
    ///
    /// Scene-referred 0.18 is the reference grey card, and the ODT this fit
    /// stands in for puts it at roughly 10% of display range. Getting that
    /// within a percentage point is what says the two matrices and the
    /// polynomial are the *ACES* fit rather than some other rational curve.
    #[test]
    fn mid_grey_lands_where_the_aces_output_transform_puts_it() {
        let [grey, _, _] = TonemapCurve::Aces.apply([0.18, 0.18, 0.18], 1.0);
        assert!(
            (grey - 0.10).abs() < 0.01,
            "scene-referred mid-grey came out at {grey}, and the ACES ODT puts it \
             near a tenth of display range"
        );
    }

    /// **The curve distinguishes highlights the clamp cannot.**
    ///
    /// This is the whole point of having one: under exposure-and-clamp every
    /// linear value at or above 1.0 is the same white, so a specular highlight
    /// and a light source read identically and the picture goes chalky. Under
    /// the fit they stay ordered and stay below one.
    #[test]
    fn the_curve_rolls_off_where_the_clamp_cannot() {
        let hot = [1.0f32, 2.0, 4.0, 8.0, 16.0];
        let clamped: Vec<f32> = hot
            .iter()
            .map(|v| TonemapCurve::Clamp.apply([*v; 3], 1.0)[0])
            .collect();
        assert!(
            clamped.iter().all(|v| (*v - 1.0).abs() < f32::EPSILON),
            "the clamp must map every one of {hot:?} to white; got {clamped:?}"
        );
        let curved: Vec<f32> = hot
            .iter()
            .map(|v| TonemapCurve::Aces.apply([*v; 3], 1.0)[0])
            .collect();
        for pair in curved.windows(2) {
            assert!(
                pair[1] > pair[0],
                "the fit must keep highlights ordered; got {curved:?}"
            );
        }
        assert!(
            curved.iter().all(|v| *v < 1.0),
            "the fit must leave headroom above every finite input; got {curved:?}"
        );
    }

    /// **The clamp is untouched by the selector existing**, which is what says
    /// no golden in the tree could have moved.
    #[test]
    fn the_clamp_is_still_the_identity_on_display_referred_colour() {
        for value in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let [out, _, _] = TonemapCurve::Clamp.apply([value; 3], DEFAULT_EXPOSURE);
            assert!(
                (out - value).abs() < f32::EPSILON,
                "the default operator moved {value} to {out}"
            );
        }
    }

    /// The exposure is the first word, the curve the second, and the rest of the
    /// row is zeroed.
    #[test]
    fn the_block_is_the_two_values_and_a_padded_row() {
        let bytes = TonemapParams {
            exposure: 2.5,
            curve: TonemapCurve::Aces,
        }
        .to_bytes();
        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &2.5f32.to_le_bytes());
        assert_eq!(&bytes[4..8], &1u32.to_le_bytes());
        assert!(bytes[8..].iter().all(|byte| *byte == 0), "{bytes:?}");
    }

    /// The default is the value the constant held, which is what says the
    /// goldens cannot have moved.
    #[test]
    fn the_default_is_the_constant_the_shader_used_to_carry() {
        assert!((TonemapParams::default().exposure - 1.0).abs() < f32::EPSILON);
        assert_eq!(TonemapParams::default().curve, TonemapCurve::Clamp);
        assert_eq!(
            TonemapParams::default().to_bytes(),
            TonemapParams {
                exposure: DEFAULT_EXPOSURE,
                curve: TonemapCurve::Clamp,
            }
            .to_bytes(),
        );
    }
}
