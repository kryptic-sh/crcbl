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
    let mut kept = Vec::with_capacity(STOPS);
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
            "quarry dolly: {at:.3} — {} of {} pixels, mean level {mean:.3}, cut {cut:?}, kept \
             {:?}",
            frame.covered,
            frame.pixels,
            frame.culled.map(|stats| (stats.instances, stats.clusters)),
        );
        kept.push(frame.culled);
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

/// **Where the reduction comes from, split between the two culls.**
///
/// The sample's exit criteria ask for it by name — "how much of the reduction is
/// instance culling and how much is cluster culling, because a single total
/// hides which one is working" — and `ForwardRenderer::cull_stats` carries both
/// numbers out of the frame that made them.
///
/// **The answer for quarry today is: all of it is cluster culling**, and that is
/// a property of the scene rather than of the engine. quarry places one instance
/// of one mesh, so the camera's instance cull has exactly one thing to decide
/// about and keeps it from every position on the dolly; every cluster the
/// amplification stage drops is the whole of the reduction. Asserted rather than
/// assumed, because "the instance cull did nothing" and "the instance cull is
/// broken" produce the same drawn frame here.
///
/// Making the split *interesting* means placing several faces so some fall
/// outside the frustum — a change to what the sample depicts, and a design call
/// rather than a test one. It is in `docs/backlog.md`.
#[test]
fn all_of_the_reduction_is_cluster_culling() {
    if backend() == crcbl::backend::GpuBackend::Null {
        eprintln!(
            "quarry dolly: the Null backend culls nothing, so there is no reduction to \
             attribute — run with CRCBL_GPU=vk"
        );
        return;
    }
    let mut quarry = Quarry::open(Levels::Dag, WALKING_BUDGET);
    let mut seen = Vec::new();
    for stop in 0..STOPS {
        let at = DOLLY_START + (DOLLY_END - DOLLY_START) * stop as f32 / (STOPS - 1) as f32;
        if let Some(stats) = quarry.frame(at).culled {
            seen.push(stats);
        }
    }
    quarry.finish();

    assert!(
        !seen.is_empty(),
        "the culling statistics never came round in {STOPS} frames, and the ring is only a few \
         frames deep — so nothing is being counted rather than nothing being reported yet"
    );
    for stats in &seen {
        assert_eq!(
            stats.instances, 1,
            "the camera's cull kept {} instances of a scene holding one, on frame {} — so the \
             count is not the survivors",
            stats.instances, stats.frame,
        );
        let clusters = stats
            .clusters
            .expect("the mesh path counts what its amplification stage kept");
        assert!(
            clusters.survivors > 0,
            "the amplification stage kept no cluster on frame {}, yet the frame drew",
            stats.frame,
        );
    }
    eprintln!(
        "quarry dolly: over {} reported frame(s) the instance cull kept 1 of 1 every time, and \
         the cluster cull kept {:?}",
        seen.len(),
        seen.iter()
            .map(|s| s.clusters.map(|cull| cull.survivors))
            .collect::<Vec<_>>(),
    );
}

/// Frames rendered at one standing pose before the counters are read.
///
/// **The pose does not move, and that is the whole design of the test below.**
/// `crcbl::render::CullStatsRing` answers a few frames behind, so on a moving
/// camera the counters and the cut read out of the same `Frame` are about two
/// different cameras and cannot be added up against each other. Standing still
/// makes every frame in the run the same measurement, so which one the ring got
/// round to stops mattering. Six is `SETTLE`'s three for the hysteresis to reach
/// its fixed point, doubled so the ring has answered well before the last frame.
const STANDING: usize = 6;

/// **The three per-cluster counts partition the cut: survivors + frustum
/// rejections + cone rejections is exactly the number of clusters tested.**
///
/// `docs/plan/sample/14-quarry.md` asks for the two rejection counts on the
/// panel, and this is the assertion that says they are the counts they claim to
/// be. It is the one thing that catches a word landing at the wrong index or a
/// bucket being missed altogether: each of those leaves three plausible numbers
/// in three plausible fields, and only the sum notices. A stage that counted a
/// cluster into two words, or that counted one the DAG descent never selected,
/// fails here too.
///
/// The right-hand side is the **cut**, read out of `cluster_selection` — the
/// clusters the descent chose, which is exactly the set the amplification stage
/// puts to a cull. Not the resident pool: the coarse levels' clusters are
/// resident and were never offered to either test.
///
/// # The normal cone is allowed to be zero here
///
/// Measured on 2026-08-20 the cone rejects almost nothing on this face — 44
/// clusters kept against 42 with the cone deliberately fed an inverted eye, in
/// `freeze.rs`'s header. So this asserts the identity and the frustum's share,
/// and prints the cone's rather than demanding it. The value of the split is
/// that the panel can now *say* the cone did nothing, which it could not before.
#[test]
fn the_three_cluster_counts_add_up_to_the_cut_they_were_taken_over() {
    if backend() == crcbl::backend::GpuBackend::Null {
        eprintln!(
            "quarry dolly: the Null backend runs no amplification stage, so there are no \
             per-cluster counts to add up — run with CRCBL_GPU=vk"
        );
        return;
    }
    // The far end of the dolly, where the camera has travelled *into* the face:
    // the near half of it is behind the eye, so the frustum has real work to do
    // and the identity is asserted over a frame where both sides are non-zero.
    let mut quarry = Quarry::open(Levels::Dag, WALKING_BUDGET);
    let mut settled: Option<Vec<usize>> = None;
    let mut last = None;
    for frame in 1..=STANDING {
        let seen = quarry.frame(DOLLY_END);
        let Some(cut) = seen.cut.clone() else {
            eprintln!(
                "quarry dolly: this device records no per-cluster cut, so there is no cut for \
                 the counters to be checked against — see harness::read_the_cut"
            );
            quarry.finish();
            return;
        };
        // The first frame of all judges every group with no history and may
        // reach a different fixed point from the ones after it, so the cut is a
        // fact about this pose only from the second frame on — which is also the
        // earliest frame the ring can report, its counter being one-based.
        if frame > 1 {
            match &settled {
                None => settled = Some(cut),
                Some(before) => assert_eq!(
                    *before, cut,
                    "the cut was still moving at a standing camera on frame {frame} of \
                     {STANDING}, so no single cut describes the frame the counters came from"
                ),
            }
        }
        last = Some(seen);
    }
    let seen = last.expect("STANDING is not zero");
    let cut = settled.expect("STANDING is more than one");
    quarry.finish();

    let stats = seen
        .culled
        .expect("the ring has come round in six frames, and it is only a few frames deep");
    let clusters = stats
        .clusters
        .expect("the mesh path counts what its amplification stage tested");
    let tested: usize = cut.iter().sum();
    eprintln!(
        "quarry dolly: frame {} tested {} cluster(s) — {} kept, {} rejected by the frustum, {} \
         by the normal cone; the cut it was taken over is {cut:?}, {tested} cluster(s)",
        stats.frame,
        clusters.tested(),
        clusters.survivors,
        clusters.frustum_rejects,
        clusters.cone_rejects,
    );
    assert!(
        stats.frame >= 2,
        "the counters describe frame {}, which is the first frame of all — the one whose cut is \
         not yet the settled one this is comparing against",
        stats.frame,
    );
    assert_eq!(
        stats.instances, 1,
        "the scene holds one instance and the camera is inside it, so the cluster counts are \
         over the whole cut or over nothing"
    );
    assert_eq!(
        clusters.tested(),
        tested as u64,
        "the amplification stage counted {} cluster(s) into its three words over a cut of \
         {tested} — the three do not partition what it tested, which is a word at the wrong \
         index or a cluster counted into none of them",
        clusters.tested(),
    );
    assert!(
        clusters.survivors > 0,
        "nothing survived the cull at a pose aimed down the face, so the identity above holds \
         over a frame that drew nothing"
    );
    assert!(
        clusters.frustum_rejects > 0,
        "the camera stands inside the face with half of it behind the eye and the frustum \
         rejected no cluster at all — so the identity above is satisfied by the survivor count \
         alone and says nothing about the split"
    );
}
