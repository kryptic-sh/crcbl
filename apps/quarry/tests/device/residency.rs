//! Residency, the frame, and the cut — milestone 1 and the first half of 2.

use crate::harness::{
    COARSE_BUDGET, DEFAULT_BUDGET, DOMINATED, Levels, MIXING_BUDGET, Quarry, draw_and_measure,
};

/// **The quarry face is a scene a `ForwardRenderer` accepts.**
///
/// The premise the frame below and every later milestone stand on. `with_scene`
/// is where a pool too small, a cluster reading outside its own arrays or a
/// vertex stride disagreeing with the shader is refused — so this failing means
/// the *content* is wrong rather than the drawing, which is why it is worth
/// asserting apart from a picture.
#[test]
fn the_face_is_a_scene_the_renderer_makes_resident() {
    let quarry = Quarry::open(Levels::Flat, DEFAULT_BUDGET);
    assert!(
        quarry.triangles > 0,
        "a face with no triangles would make every assertion here vacuous"
    );
    quarry.finish();
}

/// **A frame of the quarry face records, and on a device it draws.**
///
/// The milestone-1 proof. The frame goes through the real
/// [`ForwardRenderer::add_passes`] and the real graph, so what is exercised is
/// the path the sample will ship on rather than a rehearsal of it.
#[test]
fn the_face_draws() {
    draw_and_measure(Levels::Flat, DEFAULT_BUDGET);
}

/// **The levelled face draws, and draws the same face.**
///
/// Milestone 2's first half. `Geometry::Dag` is a different residency path —
/// one mesh-table row per level, a vertex array per level, and cluster runs the
/// selection pass reads — so a hierarchy that made a picture on the flat path
/// says nothing about this one. Held to the same coverage as the flat mesh
/// because it is the same wall: a hierarchy that drew a *different* amount of
/// the frame would be selecting wrongly rather than selecting.
#[test]
fn the_levelled_face_draws() {
    draw_and_measure(Levels::Dag, DEFAULT_BUDGET);
}
/// **The face is drawn from more than one level at once.**
///
/// Milestone 2's point, and the thing a golden cannot show: a frame whose every
/// cluster came from one level is a plausible picture. The face recedes 180
/// metres, so its near clusters and its far clusters sit at screen-space errors
/// an order of magnitude apart and no single level serves both.
///
/// The two extremes are asserted beside it, because a mixing assertion that
/// held at every budget would be measuring nothing: at one pixel the base
/// dominates, and at [`COARSE_BUDGET`] nothing of the base is drawn at all.
#[test]
fn the_cut_mixes_levels_across_the_receding_face() {
    let Some(mixed) = draw_and_measure(Levels::Dag, MIXING_BUDGET) else {
        eprintln!(
            "quarry: this device records no per-cluster cut, so there is nothing to mix — see \
             read_the_cut"
        );
        return;
    };
    // **A share, not a count of non-empty levels.** At one pixel the cut is
    // `[100, 2, …]`, which has two levels in it and is a uniform cut with a
    // rounding error on the end — so "more than one level drew" is a bar the
    // thing this test exists to distinguish already clears. What separates them
    // is whether any single level *dominates*.
    let share = |cut: &[usize]| {
        let total: usize = cut.iter().sum();
        cut.iter().copied().max().unwrap_or(0) as f32 / total as f32
    };
    assert!(
        share(&mixed) < DOMINATED,
        "at a {MIXING_BUDGET}px budget one level holds {:.0}% of the {} cluster(s) drawn \
         ({mixed:?}), which is a uniform cut wearing per-cluster selection's shape",
        share(&mixed) * 100.0,
        mixed.iter().sum::<usize>(),
    );

    let fine = draw_and_measure(Levels::Dag, DEFAULT_BUDGET).expect("the same device");
    assert!(
        share(&fine) >= DOMINATED,
        "a {DEFAULT_BUDGET}px budget spread the cut across levels too ({fine:?}), so the budget \
         is not what decides the descent and the assertion above is measuring something else",
    );

    let coarse = draw_and_measure(Levels::Dag, COARSE_BUDGET).expect("the same device");
    assert_eq!(
        coarse[0], 0,
        "a {COARSE_BUDGET}px budget still drew {} cluster(s) of the base ({coarse:?}), so the \
         descent is not stopping",
        coarse[0],
    );
}
