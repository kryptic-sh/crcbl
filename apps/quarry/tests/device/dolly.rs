//! The fixed dolly — `docs/plan/sample/14-quarry.md`'s exit criterion.
//!
//! One straight run down the face's own axis, measured frame by frame on **one
//! renderer**, which is what makes it a different measurement from the same
//! positions rendered by fresh contexts: `docs/plan/25-lod.md`'s hysteresis is
//! device-local state a shader writes once a frame, so a cut here depends on
//! every frame before it.

use crate::harness::{DOLLY_END, DOLLY_START, Levels, MIXING_BUDGET, Quarry};

/// How many frames the run is sampled at. Enough that the cut has somewhere to
/// move between neighbours, few enough that the run stays a few seconds.
const STOPS: usize = 9;

/// How far the mean level may rise between neighbouring frames.
///
/// **The popping bound.** Detail is meant to arrive as the camera closes, so the
/// mean level falls; a rise is the descent going backwards, which is what
/// popping looks like in this number. Measured over the run it is 0.013 once, at
/// the last stop, and zero everywhere else — so a tenth of a level is margin
/// rather than a fitted constant.
const MAY_RISE: f32 = 0.1;

/// How far the mean level must fall across the whole run.
///
/// Measured: 1.375 at the start against 0.513 at the end. Half a level is the
/// bar because the claim is that proximity moves the cut *substantially*, not
/// that it moves it at all — a run that drifted by a hundredth would satisfy any
/// strict inequality and show nothing.
const MUST_FALL: f32 = 0.5;

/// The average level a cut drew from, weighted by how many clusters each level
/// contributed. Zero is the base mesh throughout.
fn mean_level(cut: &[usize]) -> f32 {
    let total: usize = cut.iter().sum();
    assert!(total > 0, "a cut that drew nothing has no mean level");
    let weighted: usize = cut
        .iter()
        .enumerate()
        .map(|(level, drawn)| level * drawn)
        .sum();
    weighted as f32 / total as f32
}

/// **Detail arrives as the camera closes, and it arrives smoothly.**
///
/// The sample's whole premise in one number. The face recedes 180 metres, so a
/// camera running down it brings clusters that were far into the near field —
/// and the cut has to follow, or per-cluster selection is not a function of
/// where the camera is. Measured over nine stops, level 2's contribution falls
/// 18 → 0 while level 0's rises 0 → 38.
///
/// **Smoothly is the other half, and it is the "no LOD popping" criterion.** A
/// cut that reached the same end by jumping there would satisfy the fall and
/// would pop on screen, so each step is held to [`MAY_RISE`] as well.
#[test]
fn detail_arrives_as_the_dolly_closes_on_the_face() {
    let mut quarry = Quarry::open(Levels::Dag, MIXING_BUDGET);
    let mut means = Vec::with_capacity(STOPS);
    for stop in 0..STOPS {
        let at = DOLLY_START + (DOLLY_END - DOLLY_START) * stop as f32 / (STOPS - 1) as f32;
        let frame = quarry.frame(at);
        let Some(cut) = frame.cut else {
            eprintln!(
                "quarry dolly: this device records no per-cluster cut, so there is nothing to \
                 follow — see harness::read_the_cut"
            );
            quarry.finish();
            return;
        };
        let mean = mean_level(&cut);
        eprintln!(
            "quarry dolly: {at:.3} — {} of {} pixels, mean level {mean:.3}, cut {cut:?}",
            frame.covered, frame.pixels,
        );
        means.push(mean);
    }
    quarry.finish();

    let (first, last) = (means[0], means[STOPS - 1]);
    assert!(
        first - last >= MUST_FALL,
        "the mean level went {first:.3} → {last:.3} across the run, which is under {MUST_FALL} — \
         so closing on the face did not bring detail down the hierarchy: {means:?}"
    );
    for (stop, pair) in means.windows(2).enumerate() {
        assert!(
            pair[1] - pair[0] <= MAY_RISE,
            "between stop {stop} and {} the mean level rose {:.3}, over {MAY_RISE} — the descent \
             ran backwards, which is what popping looks like here: {means:?}",
            stop + 1,
            pair[1] - pair[0],
        );
    }
}
