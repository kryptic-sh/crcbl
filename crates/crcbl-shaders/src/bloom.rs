//! The uniform block the three `bloom_*.slang` sources read, in the layout they
//! declare — and the guard that holds the filter helpers they share together.
//!
//! Same reason as [`crate::ssao`]: the shader fixes a byte layout, every
//! producer of those bytes has to agree with it exactly, and keeping the mirror
//! in the crate that owns the source means there is one place to change rather
//! than one per consumer.
//!
//! **One block for all three passes**, rather than three that agree today. The
//! chain is one algorithm walked in two directions, every step of it needs the
//! texel size of the image it is reading, and the two scalars beside that are
//! each read by one step and ignored by the others. Three blocks would be three
//! `std140` layouts to keep in step and one ring of buffers per shape;
//! `crcbl_render::bloom` writes one row per step of the chain out of this.
//!
//! The guard is [`tests::the_shared_filter_helpers_have_not_drifted`], on
//! [`crate::ssr`]'s terms: `bloom_up.slang` and `bloom_composite.slang` both
//! carry the 3×3 tent, and all three sources carry the bilinear `tap` it is
//! built out of, because this repo has no include mechanism by design.
//!
//! [`tests::the_shared_filter_helpers_have_not_drifted`]: self

/// Bytes of the uniform block: a `float2` and two `float`s, in one row.
///
/// `std140` aligns a `float2` to eight bytes, so the two scalars follow it at
/// offsets 8 and 12 and the row closes at sixteen with no tail padding. See
/// [`BloomParams::to_bytes`].
pub const PARAMS_SIZE: usize = 16;

/// The Karis switch on the chain's **first** downsample.
///
/// `bloom_down.slang` multiplies its luma term by this, so `1.0` is the partial
/// Karis average and [`KARIS_OFF`] is the plain weights — see that shader on why
/// the switch is a factor rather than a branch.
pub const KARIS_ON: f32 = 1.0;

/// The Karis switch on every downsample after the first.
///
/// **Exactly zero, and that exactness is the point**: at zero every weight in
/// `bloom_down.slang` is its own base and the five of them sum to exactly `1.0`
/// in binary floating point, so the plain thirteen-tap filter is recovered bit
/// for bit rather than approximately.
pub const KARIS_OFF: f32 = 0.0;

/// How much of the chain a renderer nobody has configured adds to the frame.
///
/// `bloom_composite.slang` computes `scene + bloom * strength`, and `bloom` there
/// is the **sum** of every level of the chain rather than one blurred copy of
/// the scene — so the scalar that looks right is far below the one a
/// single-Gaussian bloom would want. At this value a frame with no
/// above-display-range content in it moves by a fraction of a least significant
/// bit, and a bright emitter grows a halo that is plainly visible against it.
///
/// There is deliberately **no setter** for it. A knob with no caller is surface
/// this codebase rejects, and the per-camera render stack
/// `docs/plan/18-render-features.md` describes is where a real one belongs — so
/// the number stays here until something asks for it.
pub const DEFAULT_STRENGTH: f32 = 0.05;

/// The uniform block, matching `struct BloomParams` in `shaders/bloom_down.slang`
/// and the identical declarations in `bloom_up.slang` and
/// `bloom_composite.slang`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BloomParams {
    /// One over the extent of the image the step **reads**, in texels.
    ///
    /// Every tap offset in the chain is a whole number of source texels scaled
    /// by this, so a chain whose extents do not halve exactly changes the
    /// filter's footprint by the rounding fraction and nothing else.
    pub inv_source: [f32; 2],
    /// [`KARIS_ON`] on the first downsample, [`KARIS_OFF`] everywhere else.
    pub karis: f32,
    /// The composite's scalar; [`DEFAULT_STRENGTH`] unless a caller says
    /// otherwise. Ignored by every other step.
    pub strength: f32,
}

impl BloomParams {
    /// The block as the bytes a uniform buffer holds.
    ///
    /// Little-endian throughout, and the whole [`PARAMS_SIZE`] is written for
    /// [`crate::ssao::SsaoParams::to_bytes`]'s reason: the buffer is that wide
    /// and a partial write leaves the tail undefined.
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0;
        for value in [
            self.inv_source[0],
            self.inv_source[1],
            self.karis,
            self.strength,
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        debug_assert_eq!(at, PARAMS_SIZE, "the row closes with no tail padding");
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every bloom source, for the guards below.
    const SOURCES: [(&str, &str); 3] = [
        (
            "bloom_down.slang",
            include_str!("../shaders/bloom_down.slang"),
        ),
        ("bloom_up.slang", include_str!("../shaders/bloom_up.slang")),
        (
            "bloom_composite.slang",
            include_str!("../shaders/bloom_composite.slang"),
        ),
    ];

    /// The body of the function named `signature` in `source`, brace to brace.
    ///
    /// [`crate::ssr`]'s `body_of`, copied for the reason that module's guard
    /// exists at all — it is a test helper in a `cfg(test)` module and neither
    /// crate exports one.
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
    /// All three sources declare `tap` and two of them declare `tent`, because
    /// the manifest hashes one source per artifact and an `#include` would be a
    /// file whose edits nothing downstream notices. Nothing else in the tree
    /// would notice one copy being fixed and the others left: the shaders
    /// compile either way, and the failure is a chain whose upsample spreads a
    /// mip differently from the pass that finishes it — a softer or harder halo,
    /// which is a picture.
    ///
    /// The bodies compare rather than the whole declarations, because the doc
    /// comment above each is allowed to say what that file uses it for.
    #[test]
    fn the_shared_filter_helpers_have_not_drifted() {
        for signature in [
            "float3 tap(float2 uv, float2 offset)",
            "float3 tent(float2 uv)",
        ] {
            let copies: Vec<(&str, String)> = SOURCES
                .iter()
                .filter_map(|(name, source)| Some((*name, body_of(source, signature)?)))
                .collect();
            assert!(
                copies.len() > 1,
                "`{signature}` was found in {} of the bloom shaders, so this guard is holding \
                 nothing together — either the signature moved or `SOURCES` is stale",
                copies.len()
            );
            let (first_name, first) = &copies[0];
            for (name, body) in &copies[1..] {
                assert_eq!(
                    body, first,
                    "`{signature}` differs between {first_name} and {name}; the bloom filter \
                     helpers are copied verbatim and one copy has drifted"
                );
            }
        }
    }

    /// The block every bloom source declares, member for member and in this
    /// order.
    ///
    /// Nothing else can catch a rename or a reorder: the shaders compile either
    /// way and the buffer is bound either way, and a block whose members moved
    /// would read the Karis switch as half a texel size and blur the chain into
    /// a different picture. Reading the source is the check, and the source is
    /// hash-pinned by the manifest, so it is the same file the committed
    /// artifact was built from.
    #[test]
    fn the_uniform_block_matches_the_struct_every_bloom_source_declares() {
        for (name, source) in SOURCES {
            for declaration in ["float2 inv_source;", "float karis;", "float strength;"] {
                assert!(
                    source.contains(declaration),
                    "{name} does not declare `{declaration}`"
                );
            }
            let inv_source = source.find("float2 inv_source;").expect("just checked");
            let karis = source.find("float karis;").expect("just checked");
            let strength = source.find("float strength;").expect("just checked");
            assert!(
                inv_source < karis && karis < strength,
                "{name} declares the block in a different order than `to_bytes` writes it"
            );
        }
    }

    /// The `karis` factor is what the shader multiplies its luma term by, and
    /// the plain weights it recovers sum to exactly one.
    ///
    /// Both halves are checkable here and neither is checkable anywhere else: a
    /// switch spelled as an `if` in the shader would compile and draw, and a
    /// denominator that summed to `0.999…` would leave every later mip of every
    /// chain slightly brighter than the filter it is meant to be.
    #[test]
    fn the_plain_weights_the_karis_switch_falls_back_to_sum_to_exactly_one() {
        let source = include_str!("../shaders/bloom_down.slang");
        assert!(
            source.contains("params.karis * luma(g0)"),
            "bloom_down.slang no longer multiplies its luma term by the switch, so \
             `KARIS_OFF` is not the plain filter any more"
        );
        // The five weights as the shader writes them, at `karis = 0`.
        let weights = [0.125f32, 0.125, 0.125, 0.125, 0.5];
        let sum = weights.iter().fold(0.0f32, |sum, weight| sum + weight);
        assert_eq!(
            sum, 1.0,
            "the thirteen-tap filter's weights must sum to exactly one, or `KARIS_OFF` \
             rescales every mip past the first"
        );
        assert_eq!(KARIS_OFF, 0.0);
        assert_eq!(KARIS_ON, 1.0);
    }

    /// The layout claim, checked rather than asserted in prose.
    #[test]
    fn the_block_is_a_texel_size_and_two_scalars_in_one_row() {
        let bytes = BloomParams {
            inv_source: [0.25, 0.5],
            karis: KARIS_ON,
            strength: 0.125,
        }
        .to_bytes();

        assert_eq!(bytes.len(), PARAMS_SIZE);
        assert_eq!(&bytes[0..4], &0.25f32.to_le_bytes());
        assert_eq!(&bytes[4..8], &0.5f32.to_le_bytes());
        assert_eq!(&bytes[8..12], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[12..16], &0.125f32.to_le_bytes());
    }
}
