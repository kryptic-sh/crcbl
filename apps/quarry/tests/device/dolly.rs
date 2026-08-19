//! The fixed dolly — `docs/plan/sample/14-quarry.md`'s exit criterion.
//!
//! One straight run down the face's own axis, measured frame by frame on **one
//! renderer**, which is what makes it a different measurement from the same
//! positions rendered by fresh contexts: `docs/plan/25-lod.md`'s hysteresis is
//! device-local state a shader writes once a frame, so a cut here depends on
//! every frame before it.

use crcbl::hal::GeometryPath;

use crate::harness::{DOLLY_END, DOLLY_START, Levels, MIXING_BUDGET, Quarry, backend};

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

/// The budget the uniform walk is read at — see
/// [`the_uniform_cut_walks_down_without_skipping`] for why it is not
/// [`MIXING_BUDGET`].
const WALKING_BUDGET: f32 = 256.0;

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

/// **The uniform cut walks down the levels too, one rung at a time.**
///
/// `docs/plan/sample/14-quarry.md`'s "no LOD popping on **any** path". The two
/// indirect paths pick one level for the whole mesh rather than one per cluster,
/// so their observable is the bucket that drew rather than a distribution — and
/// popping there is a visible thing: the level jumping by more than one rung
/// between neighbouring frames, or going backwards while the camera closes.
///
/// It also records what the sample's exit criteria ask for: the triangle count
/// per path, taken from the draw that actually ran rather than from the mesh —
/// 8192 at level 0, halving per rung.
///
/// # The budget is chosen so the walk has rungs in it
///
/// [`WALKING_BUDGET`] rather than [`MIXING_BUDGET`], and it is not cosmetic. At
/// sixteen pixels the cut goes level 1 → 0 at the first stop and stays, so "one
/// rung at a time" is an assertion over a single transition and cannot fail. At
/// 256 it walks 2 → 1 → 0 with a stop on each.
///
/// **At 1024 it skips, 2 → 0, and that is the camera rather than a defect.** A
/// stop is an eighth of the run, about 17 metres, and a uniform cut moves the
/// whole mesh at once — so a coarse enough budget makes one step of the dolly
/// worth more than one rung of the hierarchy. Recorded because it bounds what
/// this assertion means: it holds for a camera that moves slowly against the
/// budget, not for every camera.
#[test]
fn the_uniform_cut_walks_down_without_skipping() {
    if backend() == crcbl::backend::GpuBackend::Null {
        eprintln!(
            "quarry dolly: the Null backend selects no level, so there is no cut to walk — run \
             with CRCBL_GPU=vk"
        );
        return;
    }
    for path in [GeometryPath::IndirectCount, GeometryPath::IndirectPerBatch] {
        let mut quarry = Quarry::open_on(Levels::Dag, WALKING_BUDGET, path);
        let mut levels = Vec::with_capacity(STOPS);
        for stop in 0..STOPS {
            let at = DOLLY_START + (DOLLY_END - DOLLY_START) * stop as f32 / (STOPS - 1) as f32;
            let frame = quarry.frame(at);
            let uniform = frame
                .uniform
                .expect("an indirect path routes its instance through a level bucket");
            eprintln!(
                "quarry dolly: {path:?} {at:.3} — level {}, {} triangle(s), {} of {} pixels",
                uniform.level, uniform.triangles, frame.covered, frame.pixels,
            );
            levels.push(uniform.level);
        }
        quarry.finish();

        assert!(
            levels[0] > levels[STOPS - 1],
            "{path:?} drew level {} at both ends of the dolly ({levels:?}), so closing on the \
             face did not bring detail down the hierarchy",
            levels[0],
        );
        for (stop, pair) in levels.windows(2).enumerate() {
            assert!(
                pair[1] <= pair[0],
                "{path:?} went from level {} to {} between stop {stop} and {} while the camera \
                 closed ({levels:?}) — the descent ran backwards",
                pair[0],
                pair[1],
                stop + 1,
            );
            assert!(
                pair[0] - pair[1] <= 1,
                "{path:?} skipped from level {} to {} between stop {stop} and {} ({levels:?}), \
                 which is a rung of the hierarchy arriving all at once",
                pair[0],
                pair[1],
                stop + 1,
            );
        }
    }
}
