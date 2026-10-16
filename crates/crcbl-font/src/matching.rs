//! Choosing a family's face for a weight and style: CSS Fonts Level 4's font
//! matching algorithm, §5.2, step 4.
//!
//! The faces are narrowed by width, then by style, then by weight, each step
//! keeping only the faces that share the value it chose:
//!
//! * **Width** is always asked for as normal, since a [`FontQuery`] names no
//!   width. A normal face wins; else the narrower faces, nearest first; else
//!   the wider, nearest first.
//! * **Style**: italic asks for italic, then oblique, then normal; oblique
//!   for oblique, then italic, then normal; normal for normal, then oblique,
//!   then italic.
//! * **Weight**, `desired` the query's: from 400 to 500 inclusive, the weights
//!   from `desired` up to 500 ascending, then those below `desired`
//!   descending, then those above 500 ascending; below 400, the weights at or
//!   below `desired` descending, then those above ascending; above 500, the
//!   weights at or above `desired` ascending, then those below descending.
//!
//! The weight rule is the one `crcbl-ui`'s `font::face` applies to the faces
//! an application registered. That one has no width step, and tells italic
//! from oblique no more than its stylesheets do.
//!
//! [`FontQuery`]: crate::FontQuery

use core::cmp::Ordering;

use crate::FontStyle;

/// A face's width as a fraction of normal, as `skrifa` reads it from the
/// `OS/2` table's `usWidthClass`.
pub(crate) const NORMAL_STRETCH: f32 = 1.0;

/// What matching reads of a face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Attributes {
    /// Its weight class, 1 to 1000.
    pub weight: u16,
    pub style: FontStyle,
    /// Its width as a fraction of [`NORMAL_STRETCH`].
    pub stretch: f32,
}

/// The index in `faces` of the face CSS font matching picks for `weight` and
/// `style`, or `None` for no faces. Of faces that tie on all three, the first.
pub(crate) fn best(faces: &[Attributes], weight: u16, style: FontStyle) -> Option<usize> {
    let stretch = faces
        .iter()
        .map(|face| face.stretch)
        .min_by(|a, b| stretch_order(*a, *b))?;
    let style = style_order(style).into_iter().find(|&style| {
        faces
            .iter()
            .any(|face| face.stretch == stretch && face.style == style)
    })?;
    let candidates = || {
        faces
            .iter()
            .enumerate()
            .filter(move |(_, face)| face.stretch == stretch && face.style == style)
    };
    let chosen = candidates()
        .map(|(_, face)| face.weight)
        .min_by_key(|&candidate| weight_rank(weight, candidate))?;
    candidates()
        .find(|(_, face)| face.weight == chosen)
        .map(|(at, _)| at)
}

/// Which of two widths CSS prefers when normal is asked for: `Less` for `a`.
fn stretch_order(a: f32, b: f32) -> Ordering {
    let rank = |stretch: f32| {
        if stretch == NORMAL_STRETCH {
            (0, 0.0)
        } else if stretch < NORMAL_STRETCH {
            (1, NORMAL_STRETCH - stretch)
        } else {
            (2, stretch - NORMAL_STRETCH)
        }
    };
    let (a, b) = (rank(a), rank(b));
    a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))
}

/// The styles tried for `desired`, in order.
fn style_order(desired: FontStyle) -> [FontStyle; 3] {
    use FontStyle::{Italic, Normal, Oblique};
    match desired {
        Normal => [Normal, Oblique, Italic],
        Italic => [Italic, Oblique, Normal],
        Oblique => [Oblique, Italic, Normal],
    }
}

/// Where `candidate` stands when `desired` is asked for: lower is preferred.
/// The group says which of the rule's runs it falls in, the distance its
/// place in that run.
fn weight_rank(desired: u16, candidate: u16) -> (u8, u16) {
    /// Where CSS's middle band, the weights that prefer to round up, starts.
    const BAND_START: u16 = 400;
    /// Where it ends, inclusive.
    const BAND_END: u16 = 500;
    let distance = desired.abs_diff(candidate);
    if (BAND_START..=BAND_END).contains(&desired) {
        if (desired..=BAND_END).contains(&candidate) {
            (0, distance)
        } else if candidate < desired {
            (1, distance)
        } else {
            (2, distance)
        }
    } else if desired < BAND_START {
        (u8::from(candidate > desired), distance)
    } else {
        (u8::from(candidate < desired), distance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FontStyle::{Italic, Normal, Oblique};

    fn face(weight: u16, style: FontStyle) -> Attributes {
        Attributes {
            weight,
            style,
            stretch: NORMAL_STRETCH,
        }
    }

    /// The weight `best` picks from `weights`, all normal and upright.
    fn pick(weights: &[u16], desired: u16) -> u16 {
        let faces: Vec<_> = weights.iter().map(|&w| face(w, Normal)).collect();
        faces[best(&faces, desired, Normal).expect("a face")].weight
    }

    #[test]
    fn an_exact_weight_wins() {
        assert_eq!(pick(&[100, 400, 700, 900], 700), 700);
        assert_eq!(pick(&[100, 400, 700, 900], 400), 400);
    }

    #[test]
    fn the_middle_band_rounds_up_to_500_then_down_then_up() {
        // 400 asks for 500 first, then below.
        assert_eq!(pick(&[300, 500, 600], 400), 500);
        assert_eq!(pick(&[300, 600], 400), 300);
        assert_eq!(pick(&[200, 300, 600], 400), 300);
        // Nothing at or below: the lightest above 500.
        assert_eq!(pick(&[600, 900], 400), 600);
        // 450 takes 500 over 400, though 400 is as near.
        assert_eq!(pick(&[400, 500], 450), 500);
        // 500 itself, then below.
        assert_eq!(pick(&[400, 700], 500), 400);
        // Band members above `desired` but within the band win over below.
        assert_eq!(pick(&[300, 480], 420), 480);
    }

    #[test]
    fn light_weights_round_down_first() {
        assert_eq!(pick(&[100, 200, 400], 300), 200);
        assert_eq!(pick(&[400, 700], 300), 400);
        assert_eq!(pick(&[100, 700], 350), 100);
        // The weight itself before anything lighter.
        assert_eq!(pick(&[200, 300], 300), 300);
    }

    #[test]
    fn heavy_weights_round_up_first() {
        assert_eq!(pick(&[400, 800, 900], 700), 800);
        assert_eq!(pick(&[400, 500, 600], 700), 600);
        assert_eq!(pick(&[300, 900], 600), 900);
    }

    #[test]
    fn style_falls_back_in_css_order() {
        let faces = [face(400, Normal), face(400, Oblique), face(400, Italic)];
        let style = |desired, faces: &[Attributes]| faces[best(faces, 400, desired).unwrap()].style;
        assert_eq!(style(Italic, &faces), Italic);
        assert_eq!(style(Oblique, &faces), Oblique);
        assert_eq!(style(Normal, &faces), Normal);
        // Italic without italic: oblique before normal.
        assert_eq!(style(Italic, &faces[..2]), Oblique);
        // Oblique without oblique: italic before normal.
        let no_oblique = [face(400, Normal), face(400, Italic)];
        assert_eq!(style(Oblique, &no_oblique), Italic);
        // Normal without normal: oblique before italic.
        assert_eq!(style(Normal, &faces[1..]), Oblique);
        // Only an upright face: every style takes it.
        assert_eq!(style(Italic, &faces[..1]), Normal);
    }

    #[test]
    fn style_is_chosen_before_weight() {
        // An italic query takes the light italic over the exact-weight roman.
        let faces = [face(700, Normal), face(300, Italic)];
        assert_eq!(best(&faces, 700, Italic), Some(1));
    }

    #[test]
    fn width_is_chosen_before_style_and_normal_wins() {
        let condensed = Attributes {
            stretch: 0.75,
            ..face(400, Italic)
        };
        let faces = [condensed, face(400, Normal)];
        // The italic is condensed, so the normal-width roman wins.
        assert_eq!(best(&faces, 400, Italic), Some(1));
    }

    #[test]
    fn without_a_normal_width_narrower_comes_before_wider() {
        let at = |stretch| Attributes {
            stretch,
            ..face(400, Normal)
        };
        assert_eq!(best(&[at(1.125), at(0.875)], 400, Normal), Some(1));
        assert_eq!(best(&[at(0.5), at(0.875)], 400, Normal), Some(1));
        assert_eq!(best(&[at(1.5), at(1.125)], 400, Normal), Some(1));
    }

    #[test]
    fn ties_go_to_the_first_face_and_nothing_is_none() {
        let faces = [face(400, Normal), face(400, Normal)];
        assert_eq!(best(&faces, 400, Normal), Some(0));
        assert_eq!(best(&[], 400, Normal), None);
    }
}
