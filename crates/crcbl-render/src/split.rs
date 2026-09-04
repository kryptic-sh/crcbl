//! The comparison seam: one frame's data resolved two ways, either side of a
//! vertical line.
//!
//! `docs/plan/sample/17-mirrors.md`'s exit criteria ask for a "split-screen
//! comparison of any two rungs, **from one frame's data**", and
//! `docs/plan/sample/18-sundial.md` and `docs/plan/sample/19-alcove.md` each ask
//! for the same harness on that sample's pattern. This module is the half of it
//! that is the same for all three: where the seam falls, and which pixels each
//! side owns.
//!
//! # What lives here and what does not
//!
//! **The geometry is shared; the switch is not.** A seam is a rectangle pair
//! and that is worth writing once. Which effect is being compared, and what the
//! two sides differ *by*, is a question only that effect can answer — the
//! occlusion chain's two sides differ in a uniform block, and a reflection
//! ladder's would differ in a pipeline — so each effect declares its own
//! console variable and calls [`halves`] with its own target's extent.
//!
//! One variable per effect is also what a person comparing wants: two effects
//! split at once is two questions asked of one picture, and neither answered.
//!
//! # Why a scissor and not two targets
//!
//! The effect records its pass twice, each time with [`Rect2d`] restricting
//! which pixels the draw may write, and both write the **same** image. Nothing
//! is allocated, nothing is copied, and the two sides cost together what one
//! full pass costs — a half-width scissor halves the fragments, so a comparison
//! is not a frame at half the frame rate.
//!
//! The viewport stays the whole target. A full-screen pass derives its own
//! coordinates from `SV_Position`, so shrinking the viewport would squash the
//! image into the half rather than showing that half of it; the scissor is the
//! one of the two that crops.
//!
//! **What this cannot split is a pass whose output is not where it was
//! written.** A gather that reduces, a blur that reads its neighbours, a chain
//! whose later stages mix the halves — each carries the seam sideways by its own
//! footprint. That is why the occlusion chain splits its *gather* and leaves the
//! filters whole: a few texels of blend at the seam is a better picture than a
//! tear, and the filters are not what the comparison is about.

use crcbl_hal::Rect2d;

/// The two rectangles `extent`'s pixels are divided into by a seam at `at`.
///
/// `at` is a fraction of the width — 0.5 puts the seam down the middle — and
/// the left rectangle is the one from the edge to it. Returns `None` when
/// either side would be empty, which is what makes a seam at 0 or 1 the same
/// thing as no comparison at all: an effect asking for those gets one pass over
/// the whole target rather than two, one of which draws nothing.
///
/// Rounding is the left side's: it takes the floor, so the two rectangles are
/// exactly `extent.0` wide together and no column is written twice or skipped.
/// A column written twice would be the seam's own pixel showing whichever pass
/// ran last, which is a picture that depends on pass order.
pub(crate) fn halves(extent: (u32, u32), at: f32) -> Option<(Rect2d, Rect2d)> {
    let (width, height) = extent;
    if width == 0 || height == 0 || !at.is_finite() {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    // Clamped before the cast rather than after: `at` is a console value, and a
    // float outside `0..=1` cast to `u32` is a wrap rather than a bound.
    let left = (at.clamp(0.0, 1.0) * width as f32).floor() as u32;
    if left == 0 || left >= width {
        return None;
    }
    Some((
        Rect2d {
            x: 0,
            y: 0,
            width: left,
            height,
        },
        Rect2d {
            x: i32::try_from(left).unwrap_or(i32::MAX),
            y: 0,
            width: width - left,
            height,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pair covers the target exactly: every column in one rectangle, none
    /// in both.
    ///
    /// Swept over every width a small target could have and every seam a
    /// console could carry, because the failure this guards is a single column
    /// — the one at the seam — and a spot check at 0.5 on an even width is the
    /// one case where the arithmetic cannot go wrong.
    #[test]
    fn the_two_halves_tile_the_target() {
        for width in 1..=64u32 {
            for step in 0..=100u32 {
                #[allow(clippy::cast_precision_loss)]
                let at = step as f32 / 100.0;
                let Some((left, right)) = halves((width, 8), at) else {
                    continue;
                };
                assert_eq!(left.x, 0, "the left rectangle starts at the edge");
                assert_eq!(
                    left.width + right.width,
                    width,
                    "a {width}-wide target split at {at} covers {} columns",
                    left.width + right.width
                );
                assert_eq!(
                    i64::from(right.x),
                    i64::from(left.width),
                    "the right rectangle starts where the left one ends, at {at}"
                );
                assert!(
                    left.width > 0 && right.width > 0,
                    "a rectangle with no columns is a pass that draws nothing"
                );
                assert_eq!((left.height, right.height), (8, 8), "the full height");
            }
        }
    }

    /// A seam with nothing on one side of it is no seam.
    ///
    /// The values that must refuse, and each is reachable: the ends of the
    /// console's own range, a target one pixel wide (any seam leaves a side
    /// empty), an empty target, and the floats a console variable cannot hold
    /// but a caller could pass.
    #[test]
    fn a_seam_that_leaves_a_side_empty_is_refused() {
        for (extent, at, why) in [
            ((64, 8), 0.0, "the left edge"),
            ((64, 8), 1.0, "the right edge"),
            ((64, 8), -1.0, "under the range"),
            ((64, 8), 2.0, "over the range"),
            ((64, 8), f32::NAN, "not a number"),
            ((1, 8), 0.5, "a target one column wide"),
            ((0, 8), 0.5, "no columns"),
            ((64, 0), 0.5, "no rows"),
        ] {
            assert!(
                halves(extent, at).is_none(),
                "a {extent:?} target split at {at} ({why}) gave two rectangles"
            );
        }
    }

    /// The seam lands where it was asked for, to the column.
    ///
    /// The floor is what decides it, and this is the assertion that would fail
    /// if it became a round: at a seam of 0.5 on an odd width the two are one
    /// column apart, and a golden blessed under one of them moves under the
    /// other.
    #[test]
    fn the_seam_takes_the_floor_of_the_column_it_asks_for() {
        let (left, _) = halves((100, 8), 0.5).expect("a seam down the middle");
        assert_eq!(left.width, 50);
        let (left, right) = halves((101, 8), 0.5).expect("a seam on an odd width");
        assert_eq!(
            (left.width, right.width),
            (50, 51),
            "the floor gives the extra column to the right"
        );
        let (left, _) = halves((64, 8), 0.25).expect("a quarter seam");
        assert_eq!(left.width, 16);
    }
}
