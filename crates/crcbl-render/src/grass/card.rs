//! The card every blade is drawn with, and the coverage-preserving mip chain
//! `docs/plan/57-grass.md`'s decision 3 asks for.
//!
//! > **Cards**: Castaño's coverage-preserving alpha mipmaps are cooked at build
//! > time so a distant card keeps its density: the reference alpha of each mip
//! > is found by bisection so the fraction of texels passing the cutout matches
//! > the top level. Golus's in-shader mip compensation uses `log2` and is
//! > replaced by the cooked mips.
//!
//! # What "cooked" is here
//!
//! A box-filtered chain loses coverage at every level, because averaging four
//! texels of which two pass a cutoff gives one texel that does not. Castaño's
//! fix is a per-level **reference alpha** found by bisection; this cooks the
//! equivalent, a per-level **scale on the stored alpha**, because the shader's
//! cutoff is one number for the whole chain. The two are the same statement —
//! scaling the stored alpha by `s` passes exactly the texels a reference of
//! `cutoff / s` would — and the scale is the half that survives being handed to
//! a sampler.
//!
//! Which level a card reads is `grass.slang`'s own choice rather than the
//! hardware's — see `grassCardLevel` there — and this chain is what makes that
//! choice cheap: a card popping from one level to the next changes its *shape*
//! and not its density, because every level passes the same fraction of its
//! texels.
//!
//! The chain is filtered from the **unscaled** box levels and scaled only on
//! the way into storage, so a level's error does not compound into the next.
//!
//! # The mask is authored in arithmetic
//!
//! [`blade_mask`] is a tuft of four strands, written as polynomials — no image
//! asset, no transcendental, and provenance a reader can check, which
//! `crcbl_wind`'s two committed layers are under for the same reason. A field
//! does not name a texture at this rung; `docs/backlog.md` carries the authored
//! page.
//!
//! **A tuft rather than one blade, and that matters to the check.** One fat
//! blade keeps most of its coverage under a box filter all by itself, so a chain
//! cooked for it would be indistinguishable from one that was not. Thin strands
//! with gaps between them are what a box filter dissolves, and what the
//! measurement this module's own tests make is about.

use crcbl_shaders::grass::ALPHA_CUTOFF;

/// Texels along one side of the card.
///
/// **The shader's number**, because `grass.slang` picks the chain's level from
/// it — see `grassCardLevel` there — so the two must be one constant and it
/// lives where every other number a shader fixes does.
pub use crcbl_shaders::grass::CARD_EXTENT;

/// Strands in the tuft [`blade_mask`] draws.
const STRANDS: usize = 4;

/// Each strand's centre across the card at its root, its half-width there, and
/// how far its tip leans across the card.
///
/// **Thin, and that is a choice a measurement forced** — see this module's
/// `the_cooked_chain_keeps_the_coverage_a_box_chain_loses`. A tuft of
/// three strands half as wide again keeps most of its coverage under a plain box
/// filter all by itself — measured on 2026-09-17, its level 4 still covered 71%
/// of what level 0 did — so a chain cooked for it would be nearly
/// indistinguishable from one that was not, and the check would be measuring
/// almost nothing. These four dissolve to no coverage at all by level 3, which
/// is what a field of real grass does at distance and what the cook exists to
/// put back.
const STRAND: [[f32; 3]; STRANDS] = [
    [0.22, 0.040, -0.13],
    [0.40, 0.055, -0.03],
    [0.60, 0.045, 0.10],
    [0.80, 0.035, 0.16],
];

/// How much of a strand's half-width its edge fades over, as a fraction of it.
///
/// **A measured compromise, and both ends of it bite.** A soft edge puts a wide
/// band of texels near the cutoff, and a sampler's bilinear filter is specified
/// to only eight bits of sub-texel precision — so where the value crosses the
/// cutoff moves by a large fraction of a texel between two implementations, and
/// every pixel in the band flips; that is what the golden's cross-backend
/// comparison notices. A *hard* edge leaves the cook nothing to work with
/// instead: a level box-averaged from an all-or-nothing mask holds only a
/// handful of distinct values, so its coverage is a coarse step function of the
/// scale and the bisection cannot land on the target at all. Swept over the
/// card this module authors on 2026-09-16, the cook's worst measurable level
/// ran 0.155 of the top level's coverage at 0.2, 0.119 at 0.3, **0.051 at
/// 0.4**, 0.089 at 0.55 and 0.091 at 0.8, with the partial texels rising from
/// 189 to 732 across that range. This is the bottom of that curve.
const EDGE_SOFTNESS: f32 = 0.4;

/// Levels in the chain a card of `extent` texels a side carries, down to one
/// texel.
#[must_use]
pub fn chain_levels(extent: u32) -> u32 {
    extent.max(1).ilog2() + 1
}

/// The authored card: `extent²` bytes of coverage, row 0 at the tuft's tip.
///
/// Row 0 is the tip because `grass.slang` samples at `1 - uv.y` and its `uv.y`
/// is zero at the blade's root — so the image's first row is the far end of the
/// blade, which is the convention an authored page would have to match.
///
/// # Panics
///
/// If `extent` is zero.
#[must_use]
pub fn blade_mask(extent: u32) -> Vec<u8> {
    assert!(extent > 0, "a card of {extent} texels has no texels");
    let step = 1.0 / extent as f32;
    let mut mask = vec![0u8; (extent * extent) as usize];
    for row in 0..extent {
        // `0` at the tip and `1` at the root, which is the axis every strand's
        // width and lean are written along.
        let up = (row as f32 + 0.5) * step;
        for column in 0..extent {
            let across = (column as f32 + 0.5) * step;
            let mut coverage = 0.0f32;
            for [root, half, lean] in STRAND {
                // The strand leans away from its root as the square of the
                // distance from it — `grass.slang` bends a card by the same
                // curve, so a card's own drawing and its geometry agree about
                // what a stalk does.
                let tip_share = 1.0 - up;
                let centre = root + lean * tip_share * tip_share;
                // Widest at the root and closing to a point at the tip.
                let width = half * up * (2.0 - up);
                if width <= 0.0 {
                    continue;
                }
                let reach = (across - centre).abs() / width;
                let inside = ((1.0 - reach) / EDGE_SOFTNESS).clamp(0.0, 1.0);
                coverage = coverage.max(inside * inside * (3.0 - 2.0 * inside));
            }
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "`coverage` is clamped into 0..=1, so the product is in 0..=255"
            )]
            let byte = (coverage * 255.0 + 0.5) as u8;
            mask[(row * extent + column) as usize] = byte;
        }
    }
    mask
}

/// The fraction of `level`'s texels the shader's cutout keeps.
///
/// The comparison is `grass.slang`'s own — a texel arrives from an `R8Unorm`
/// image as `n / 255` and is tested against [`ALPHA_CUTOFF`] — so this measures
/// what the shader will actually draw rather than a byte threshold that stands
/// in for it.
#[must_use]
pub fn coverage(level: &[u8]) -> f32 {
    if level.is_empty() {
        return 0.0;
    }
    let kept = level
        .iter()
        .filter(|texel| f32::from(**texel) / 255.0 >= ALPHA_CUTOFF)
        .count();
    kept as f32 / level.len() as f32
}

/// One level down: each texel the mean of the four it covers.
///
/// A plain box mean, and deliberately so — this is the filter Castaño's
/// correction is applied *to*, and `crate::mip`'s three filters are all for
/// colour pages with a transfer curve or a renormalise that a coverage mask has
/// neither of.
///
/// # Panics
///
/// If `level` is not `extent²` texels, or `extent` is below two.
#[must_use]
pub fn box_level(level: &[u8], extent: u32) -> Vec<u8> {
    assert!(extent >= 2, "a {extent}-texel level does not halve");
    assert_eq!(
        level.len(),
        (extent * extent) as usize,
        "a {extent}x{extent} level is {} texels",
        extent * extent
    );
    let half = extent / 2;
    let mut next = vec![0u8; (half * half) as usize];
    for row in 0..half {
        for column in 0..half {
            let at = |dr: u32, dc: u32| {
                u32::from(level[((row * 2 + dr) * extent + column * 2 + dc) as usize])
            };
            let total = at(0, 0) + at(0, 1) + at(1, 0) + at(1, 1);
            #[expect(
                clippy::cast_possible_truncation,
                reason = "four bytes plus two is at most 1022, and the quarter of that is under 256"
            )]
            let mean = ((total + 2) / 4) as u8;
            next[(row * half + column) as usize] = mean;
        }
    }
    next
}

/// How far the bisection searches for a level's scale.
///
/// Sixteen: a level whose box mean has dissolved the tuft to a quarter of the
/// cutoff still comes back, and nothing an eight-bit mask can hold needs more.
const MAX_SCALE: f32 = 16.0;

/// Bisection steps. Twenty-four halvings of `0..=16` leave a scale resolved to
/// a millionth, which is far finer than the step between two of the 256 values
/// a texel can take.
const BISECTION_STEPS: u32 = 24;

/// `level` with every texel scaled by `scale` and rounded back into a byte.
fn scaled(level: &[u8], scale: f32) -> Vec<u8> {
    level
        .iter()
        .map(|texel| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped into 0..=255 before the cast"
            )]
            let byte = (f32::from(*texel) * scale + 0.5).clamp(0.0, 255.0) as u8;
            byte
        })
        .collect()
}

/// The scale that brings `level`'s coverage nearest to `target`.
///
/// Castaño's bisection, over the scale rather than over the reference alpha —
/// see this module's header for why the two are one statement. Coverage is
/// non-decreasing in the scale, which is what makes a bisection the right search
/// at all: scaling every texel up can only move a texel across the cutoff in one
/// direction.
///
/// It is a **step** function of the scale, so the target is generally not
/// reachable exactly; what comes back is the nearer side of the last bracket,
/// compared by the coverage it produces rather than by the scale itself.
fn coverage_scale(level: &[u8], target: f32) -> f32 {
    // **A level that already covers what the top one does is left alone.**
    // Coverage is a step function of the scale, so the scales that reach the
    // target are an interval — and where one is on the interval, the bisection
    // below would land on its lower end, which is the scale that puts every
    // marginal texel exactly on the cutoff. One is the scale that changes
    // nothing, and a level needing no correction should get none.
    if coverage(level) == target {
        return 1.0;
    }
    let (mut low, mut high) = (0.0f32, MAX_SCALE);
    for _ in 0..BISECTION_STEPS {
        let middle = 0.5 * (low + high);
        if coverage(&scaled(level, middle)) < target {
            low = middle;
        } else {
            high = middle;
        }
    }
    let error = |scale: f32| (coverage(&scaled(level, scale)) - target).abs();
    if error(low) <= error(high) { low } else { high }
}

/// The card's coverage-preserving chain: level 0 as given, then one level per
/// halving, each scaled so the fraction of its texels the cutout keeps is the
/// fraction level 0's does.
///
/// # Panics
///
/// If `level0` is not `extent²` texels, or `extent` is zero.
#[must_use]
pub fn coverage_chain(level0: &[u8], extent: u32) -> Vec<Vec<u8>> {
    assert!(extent > 0, "a card of {extent} texels has no texels");
    assert_eq!(
        level0.len(),
        (extent * extent) as usize,
        "a {extent}x{extent} card is {} texels",
        extent * extent
    );
    let target = coverage(level0);
    let mut chain = vec![level0.to_vec()];
    // **Filtered from the unscaled level, stored scaled.** A chain that filtered
    // its own corrected output would compound each level's rounding into the
    // next, and the bottom of a seven-level chain is where that lands.
    let mut filtered = level0.to_vec();
    let mut side = extent;
    while side > 1 {
        filtered = box_level(&filtered, side);
        side /= 2;
        chain.push(scaled(&filtered, coverage_scale(&filtered, target)));
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The card is a tuft with gaps in it**, which is what the cook is about:
    /// a mask that was solid, or empty, would keep its coverage under any filter
    /// at all and the measurement below would be measuring nothing.
    #[test]
    fn the_mask_is_a_tuft_with_gaps_and_soft_edges() {
        let mask = blade_mask(CARD_EXTENT);
        assert_eq!(mask.len(), (CARD_EXTENT * CARD_EXTENT) as usize);
        let solid = mask.iter().filter(|texel| **texel == 255).count();
        let empty = mask.iter().filter(|texel| **texel == 0).count();
        let partial = mask.len() - solid - empty;
        let top = coverage(&mask);
        eprintln!(
            "grass card: {solid} solid, {empty} empty, {partial} partial texel(s); coverage {top:.4}"
        );
        assert!(solid > 0, "the tuft is nowhere opaque");
        assert!(empty > 0, "the tuft fills the card");
        assert!(partial > 64, "only {partial} texels are on an edge");
        assert!(
            (0.05..0.5).contains(&top),
            "a card that covers {top} of itself is not a tuft"
        );
        // The tip is the first row, so the mask thins towards it.
        let row_coverage = |row: u32| {
            let at = (row * CARD_EXTENT) as usize;
            coverage(&mask[at..at + CARD_EXTENT as usize])
        };
        assert!(
            row_coverage(CARD_EXTENT - 1) > row_coverage(1),
            "the tuft is not wider at its root than at its tip"
        );
    }

    /// The chain is the full pyramid, each level a quarter of the one above.
    #[test]
    fn the_chain_runs_to_a_single_texel() {
        let mask = blade_mask(CARD_EXTENT);
        let chain = coverage_chain(&mask, CARD_EXTENT);
        assert_eq!(chain.len(), chain_levels(CARD_EXTENT) as usize);
        for (level, texels) in chain.iter().enumerate() {
            #[expect(clippy::cast_possible_truncation, reason = "the chain is seven levels")]
            let side = crate::mip::level_extent(CARD_EXTENT, level as u32);
            assert_eq!(texels.len(), (side * side) as usize, "level {level}");
        }
        assert_eq!(chain[0], mask, "level 0 is the card as authored");
    }

    /// The cook is a function of its input and nothing else.
    #[test]
    fn the_cook_is_deterministic() {
        let mask = blade_mask(CARD_EXTENT);
        assert_eq!(
            coverage_chain(&mask, CARD_EXTENT),
            coverage_chain(&mask, CARD_EXTENT)
        );
    }

    /// How far a cooked level's coverage may sit from level 0's, as a fraction
    /// of level 0's.
    ///
    /// **A measurement, not a wish.** The coverage of a level of `n` texels can
    /// only take `n + 1` values, so the bottom of the chain cannot be held to a
    /// fine band by anything: a 4×4 level has seventeen possible coverages and
    /// the nearest one to this card's 0.1858 is 0.1875. Measured over the card
    /// this module authors on 2026-09-16, the cooked chain's levels 1 to 4 read
    /// 0.1953, 0.1875, 0.1875 and 0.1875 against that top level, a worst
    /// relative error of **0.0512**; this leaves a factor of three over it, and
    /// the run prints every level's figure.
    const COOKED_BAND: f32 = 0.16;

    /// How far under level 0's a plain box-filtered chain's farthest measurable
    /// level must fall before the cook counts as having done something.
    ///
    /// **The sabotage the plan names**, run as a measurement rather than as an
    /// edit: "a card field's measured coverage at its farthest mip stays within
    /// a stated band of its nearest, against a sabotage that uses plain
    /// box-filtered mips". Measured on 2026-09-16 over the card this module
    /// authors: the box chain covers 0.1602 at level 2, 0.0469 at level 3 and
    /// **nothing at all** at level 4, where the cooked chain holds 0.1875 at
    /// every one of them against a top level of 0.1858. The floor is half rather
    /// than all of it so that the claim is about the cook rather than about this
    /// card's particular strands dissolving completely.
    const BOX_LOSS: f32 = 0.5;

    /// **Coverage survives distance** — the rung's own check.
    ///
    /// Every cooked level down to the last one wide enough to measure keeps the
    /// fraction of texels the cutout draws; the plain box chain, from the same
    /// card, does not.
    #[test]
    fn the_cooked_chain_keeps_the_coverage_a_box_chain_loses() {
        let mask = blade_mask(CARD_EXTENT);
        let top = coverage(&mask);
        let cooked = coverage_chain(&mask, CARD_EXTENT);

        // The same card, filtered and stored with no correction at all.
        let mut boxed = vec![mask.clone()];
        let mut level = mask;
        let mut side = CARD_EXTENT;
        while side > 1 {
            level = box_level(&level, side);
            side /= 2;
            boxed.push(level.clone());
        }

        // The last level with enough texels for a fraction to mean anything:
        // a 2x2 level has five possible coverages and a 1x1 has two.
        let measurable = cooked.len() - 3;
        let mut worst = 0.0f32;
        for level in 1..=measurable {
            let cooked_at = coverage(&cooked[level]);
            let boxed_at = coverage(&boxed[level]);
            #[expect(clippy::cast_possible_truncation, reason = "the chain is seven levels")]
            let side = crate::mip::level_extent(CARD_EXTENT, level as u32);
            eprintln!(
                "grass card: level {level} ({side}x{side}) — cooked {cooked_at:.4}, box \
                 {boxed_at:.4}, top {top:.4}"
            );
            worst = worst.max((cooked_at - top).abs() / top);
            assert!(
                (cooked_at - top).abs() <= COOKED_BAND * top,
                "the cooked level {level} covers {cooked_at}, which is not within \
                 {COOKED_BAND} of the top level's {top}"
            );
        }
        eprintln!("grass card: the cooked chain's worst level is {worst:.4} of the top's coverage");
        let boxed_at = coverage(&boxed[measurable]);
        assert!(
            boxed_at <= (1.0 - BOX_LOSS) * top,
            "the box-filtered level {measurable} covers {boxed_at} against the top level's {top}, \
             so this card does not distinguish a cooked chain from an uncooked one and the claim \
             above is vacuous"
        );
    }

    /// **The bisection really searches**, and a level that needs no correction
    /// gets none.
    ///
    /// A solid mask's coverage is one at every level under any filter, so the
    /// scale that hits the target is any scale at or above one — and the search
    /// must not wander off to the top of its range on it.
    #[test]
    fn a_level_that_needs_no_correction_is_left_alone() {
        let solid = vec![255u8; 16 * 16];
        let chain = coverage_chain(&solid, 16);
        for (level, texels) in chain.iter().enumerate() {
            assert_eq!(
                texels,
                &vec![255u8; texels.len()],
                "level {level} of a solid card was rescaled"
            );
            assert_eq!(coverage(texels), 1.0);
        }
        let empty = vec![0u8; 16 * 16];
        let chain = coverage_chain(&empty, 16);
        for (level, texels) in chain.iter().enumerate() {
            assert!(
                texels.iter().all(|texel| *texel == 0),
                "level {level} of an empty card grew coverage out of nothing"
            );
        }
    }

    /// The box filter is a mean, rounded half up, and it halves the extent.
    #[test]
    fn the_box_level_is_the_mean_of_its_four() {
        let level = vec![9u8, 10, 200, 255, 1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0];
        let next = box_level(&level, 4);
        assert_eq!(next.len(), 4);
        assert_eq!(next[0], ((9 + 10 + 1 + 2 + 2) / 4) as u8);
        assert_eq!(next[1], ((200 + 255 + 3 + 4 + 2) / 4) as u8);
        assert_eq!(next[2], 0);
    }
}
