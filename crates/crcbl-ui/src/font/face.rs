//! Faces: which of a family's fonts a text node draws in, by its
//! `font-weight` and `font-style`, and the bold and italic drawn for a family
//! that has no such face.
//!
//! # Matching
//!
//! A family is every [`Font`] registered under one name. [`select_face`] is
//! CSS Fonts 4's font-matching algorithm (§5.2) cut down to the two properties
//! the subset has:
//!
//! * **Style first.** `italic` takes the family's slanted faces — italic or
//!   oblique, which the subset does not tell apart — and only when it has none
//!   its upright ones; `normal` takes the upright faces, and only when there
//!   are none the slanted ones.
//! * **Then weight**, among the faces of that style, by the specification's
//!   rule for the weight asked for: from 400 to 500, the weights from it up to
//!   500 ascending, then those below it descending, then those above 500
//!   ascending; below 400, those at or below it descending, then those above
//!   ascending; above 500, those at or above it ascending, then those below
//!   descending.
//!
//! # Synthesis
//!
//! When the face chosen is lighter than [`SYNTHETIC_BOLD_THRESHOLD`] and the
//! weight asked for is at least that, or `italic` is asked for and the face is
//! upright, the face is drawn **synthesised**: [`Font::synthesized`] marks it,
//! and the glyph atlas distorts its outlines before it rasterises them — a
//! bold one drawn a second time [`SYNTHETIC_BOLD_EM`] of an em to the right,
//! an italic one sheared by [`SYNTHETIC_ITALIC_SLANT`]. A synthesised face is
//! a font of its own identity, so its glyphs are cached apart from the
//! upright regular ones and a span in it is measured again.
//!
//! Synthesising rather than drawing the regular face is decided for the long
//! term (2026-10-16). A family an application registers is very often one
//! file — the committed font is one — and markup that says `<b>` or `<i>`
//! means something: a match highlighted in bold that draws in regular has lost
//! what it was for. Browsers synthesise by default (`font-synthesis: weight
//! style`) for the same reason. Distorting the outline in the atlas, rather
//! than drawing each glyph quad twice or shearing the quad, keeps the draw
//! list's one quad per glyph and its one texel to one pixel: a sheared quad
//! would sample the mask between texels, and two overlapping quads blend
//! their edges twice.
//!
//! **The bitmap font has one face and is never synthesised**: it is the
//! panels' and the console's 8×13 pixel font, and a shear or a smear of a
//! pixel font is not bold or italic but a broken glyph. A span in it ignores
//! both properties.

use super::Font;

/// The weight from which a face lighter than it is drawn synthesised bold:
/// CSS's `semibold`, the first weight past `medium`.
pub const SYNTHETIC_BOLD_THRESHOLD: u16 = 600;

/// How far right a synthesised bold glyph's second drawing is, in ems — and
/// how much wider its advance is: FreeType's `FT_GlyphSlot_Embolden` strength,
/// a twenty-fourth of the pixel size.
pub const SYNTHETIC_BOLD_EM: f32 = 1.0 / 24.0;

/// How far right a synthesised italic glyph leans per unit of height: one in
/// four, about 14°, which is CSS's default `oblique` angle and the skew
/// Chrome's Skia gives a fake italic. A ratio rather than an angle, so no
/// tangent is taken where a mask's bytes must be the same on every platform.
pub const SYNTHETIC_ITALIC_SLANT: f32 = 0.25;

/// `font-weight`: how heavy a text node's face is, from 1 to 1000 — `normal`
/// is 400 and `bold` 700.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontWeight(u16);

impl FontWeight {
    /// `normal`.
    pub const NORMAL: Self = Self(400);
    /// `bold`.
    pub const BOLD: Self = Self(700);

    /// The weight `value`, if it is in CSS's range of 1 to 1000.
    #[must_use]
    pub const fn new(value: u16) -> Option<Self> {
        if value >= 1 && value <= 1000 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// The number.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// `font-style`: whether a text node draws upright or slanted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontStyle {
    /// Upright — `normal`.
    #[default]
    Normal,
    /// Slanted — `italic`: an italic or oblique face, else the upright face
    /// sheared. See the module docs.
    Italic,
}

/// What a face is drawn with that it does not have: see the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Synthesis {
    /// Drawn twice, [`SYNTHETIC_BOLD_EM`] apart, and advanced that much more.
    pub bold: bool,
    /// Sheared by [`SYNTHETIC_ITALIC_SLANT`].
    pub italic: bool,
}

impl Synthesis {
    /// Neither.
    pub const NONE: Self = Self {
        bold: false,
        italic: false,
    };

    /// Which of the four combinations this is, `0..4`.
    pub(crate) const fn index(self) -> usize {
        self.bold as usize | (self.italic as usize) << 1
    }

    /// What `face` lacks of `weight` and `style`.
    fn of(face: &Font, weight: FontWeight, style: FontStyle) -> Self {
        let threshold = f32::from(SYNTHETIC_BOLD_THRESHOLD);
        Self {
            bold: weight.value() >= SYNTHETIC_BOLD_THRESHOLD && face.weight() < threshold,
            italic: style == FontStyle::Italic && !face.is_italic(),
        }
    }
}

/// The face of `faces` a node of `weight` and `style` draws in, synthesised
/// where it falls short — see the module docs — or `None` for no faces.
#[must_use]
pub fn select_face(faces: &[Font], weight: FontWeight, style: FontStyle) -> Option<Font> {
    let want_italic = style == FontStyle::Italic;
    let preferred = faces.iter().any(|face| face.is_italic() == want_italic);
    let desired = f32::from(weight.value());
    faces
        .iter()
        .filter(|face| !preferred || face.is_italic() == want_italic)
        .min_by(|a, b| {
            let (a, b) = (
                weight_rank(desired, a.weight()),
                weight_rank(desired, b.weight()),
            );
            a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))
        })
        .map(|face| face.synthesized(Synthesis::of(face, weight, style)))
}

/// Where a face of weight `face` stands when `desired` is asked for: a lower
/// tier first, then a lower key. CSS Fonts 4's weight rule; see the module
/// docs.
fn weight_rank(desired: f32, face: f32) -> (u8, f32) {
    let normal = f32::from(FontWeight::NORMAL.value());
    let medium = 500.0;
    if (normal..=medium).contains(&desired) {
        if face >= desired && face <= medium {
            (0, face)
        } else if face < desired {
            (1, -face)
        } else {
            (2, face)
        }
    } else if desired < normal {
        if face <= desired {
            (0, -face)
        } else {
            (1, face)
        }
    } else if face >= desired {
        (0, face)
    } else {
        (1, -face)
    }
}

#[cfg(all(test, feature = "parsed-font"))]
mod tests {
    use super::*;
    use crate::font::FontMetrics;

    fn face(weight: f32, italic: bool) -> Font {
        Font::fixed_pitch_face(
            FontMetrics {
                units_per_em: 1000,
                ascent: 800.0,
                descent: -200.0,
                line_gap: 0.0,
            },
            500.0,
            weight,
            italic,
        )
        .clone()
    }

    fn weight(value: u16) -> FontWeight {
        FontWeight::new(value).expect("in range")
    }

    /// The weight of the face chosen from a family of `weights`, all upright.
    fn chosen(weights: &[f32], desired: u16) -> f32 {
        let faces: Vec<Font> = weights.iter().map(|&w| face(w, false)).collect();
        select_face(&faces, weight(desired), FontStyle::Normal)
            .expect("a face")
            .weight()
    }

    /// **The weight rule is CSS's**, in each of its three bands, checked
    /// against families built so that a nearest-weight rule would answer
    /// differently.
    #[test]
    fn the_nearest_face_by_weight_follows_the_css_rule() {
        let cases: [(&[f32], u16, f32); 9] = [
            // 400–500: up to 500 first, then down, then above 500.
            (&[300.0, 500.0, 600.0], 400, 500.0),
            (&[300.0, 600.0], 400, 300.0),
            (&[600.0, 900.0], 450, 600.0),
            (&[350.0, 400.0, 500.0], 450, 500.0),
            // Below 400: down first, then up.
            (&[100.0, 400.0], 300, 100.0),
            (&[400.0, 700.0], 300, 400.0),
            // Above 500: up first, then down.
            (&[500.0, 900.0], 700, 900.0),
            (&[400.0, 600.0], 700, 600.0),
            (&[400.0, 700.0, 800.0], 700, 700.0),
        ];
        for (weights, desired, want) in cases {
            assert_eq!(chosen(weights, desired), want, "{desired} from {weights:?}");
        }
    }

    /// **Style decides before weight**: an italic request takes a slanted
    /// face of any weight over an upright face of the exact weight, and
    /// falls back to upright faces only when the family has no slanted one.
    #[test]
    fn style_is_matched_before_weight() {
        let family = [face(700.0, false), face(400.0, true)];
        let italic = select_face(&family, FontWeight::BOLD, FontStyle::Italic).expect("a face");
        assert!(italic.is_italic());
        assert_eq!(italic.weight(), 400.0);
        assert_eq!(
            italic.synthesis(),
            Synthesis {
                bold: true,
                italic: false
            },
            "the italic regular face is emboldened, not unslanted"
        );
        let upright = select_face(&family, FontWeight::NORMAL, FontStyle::Normal).expect("a face");
        assert!(!upright.is_italic());
        assert_eq!(upright.weight(), 700.0);

        let roman = [face(400.0, false)];
        let fallback = select_face(&roman, FontWeight::NORMAL, FontStyle::Italic).expect("a face");
        assert!(!fallback.is_italic());
        assert_eq!(
            fallback.synthesis(),
            Synthesis {
                bold: false,
                italic: true
            }
        );
        assert_eq!(
            select_face(&[], FontWeight::NORMAL, FontStyle::Normal),
            None
        );
    }

    /// **Synthesis covers exactly what the face lacks**: bold from the
    /// threshold up on a face below it, never on a face at it, and never for a
    /// weight below it.
    #[test]
    fn a_face_is_synthesised_only_for_what_it_lacks() {
        let regular = [face(400.0, false)];
        let semibold = [face(600.0, false)];
        let cases = [
            (&regular, 599, false),
            (&regular, SYNTHETIC_BOLD_THRESHOLD, true),
            (&regular, 900, true),
            (&semibold, 900, false),
        ];
        for (family, desired, bold) in cases {
            let chosen =
                select_face(family.as_slice(), weight(desired), FontStyle::Normal).expect("a face");
            assert_eq!(chosen.synthesis().bold, bold, "{desired}");
            assert!(!chosen.synthesis().italic);
        }
        let chosen = select_face(&regular, FontWeight::NORMAL, FontStyle::Normal).expect("a face");
        assert_eq!(chosen, regular[0], "nothing synthesised is the face itself");
    }

    /// The weight range is CSS's.
    #[test]
    fn a_weight_is_from_1_to_1000() {
        assert_eq!(FontWeight::new(0), None);
        assert_eq!(FontWeight::new(1).map(FontWeight::value), Some(1));
        assert_eq!(FontWeight::new(1000).map(FontWeight::value), Some(1000));
        assert_eq!(FontWeight::new(1001), None);
    }
}
