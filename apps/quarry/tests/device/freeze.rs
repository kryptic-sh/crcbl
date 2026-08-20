//! Freezing the eye the cut is selected from —
//! `docs/plan/sample/14-quarry.md`'s "freeze-selection-from-here camera", the
//! third of the three that document's Proves section asks for.
//!
//! # Why a cut has to be looked at from somewhere else
//!
//! A screen-space error budget says a cut is acceptable *from the eye it was
//! chosen for*. That is exactly what makes an unfrozen frame useless as
//! evidence: from the selecting eye, a cut that is far too coarse and a cut that
//! is exactly right have the same silhouette, because the budget is the promise
//! that they do. Every frame a reviewer has ever looked at is drawn from the
//! point that chose it, so it is the one viewpoint where a wrong cut looks
//! right.
//!
//! `ForwardRenderer::set_frozen_selection_eye` pins the selection at one point
//! and leaves everything else on the live camera, so a reviewer flies away from
//! that point and looks at the cut it chose. These are the three claims that
//! makes: the cut stops moving, an unpinned one does not, and the culls carry on
//! following the camera — because a frame culled for a viewpoint nobody is
//! standing at would draw nothing to look at.
//!
//! Every assertion here needs the per-cluster cut, which needs an amplification
//! stage. On a device without one the module says so and passes, as the rest of
//! this suite does.

use crcbl::render::Camera;

use crate::harness::{DOLLY_END, DOLLY_START, Levels, MIXING_BUDGET, Quarry, dolly};

/// How many stops the camera is flown through while the selection is pinned.
///
/// The dolly's own run, sampled as `dolly.rs` samples it, so the camera really
/// does travel the whole face rather than nudging. It is the same journey
/// `detail_arrives_as_the_dolly_closes_on_the_face` measures the mean level
/// falling most of a level across, which is what makes "the cut did not move"
/// worth asserting over it.
const STOPS: usize = 5;

/// Frames rendered at one pose before the cut is read as settled.
///
/// `docs/plan/25-lod.md`'s hysteresis is a per-frame fixed point: the first
/// frame of all judges every group against the *expand* budget with no history,
/// and the ones after it judge an expanded group against the lower hold budget.
/// So a cut is a fact about a pose only once it has stopped moving, and freezing
/// before then would pin a selection still converging — which would drift on its
/// own and pass the "it did not follow the camera" assertion for the wrong
/// reason. Three is measured: at this budget the second frame already agrees
/// with the third, and the first assertion of
/// [`the_frozen_cut_does_not_follow_the_camera`] is what holds it to that — it
/// compares the last two cuts before anything is pinned.
const SETTLE: usize = 3;

/// Renders `SETTLE` frames at `at` and answers with the last two cuts.
fn settled(quarry: &mut Quarry, at: f32) -> Option<(Vec<usize>, Vec<usize>)> {
    let mut last = None;
    let mut previous = None;
    for _ in 0..SETTLE {
        previous = last;
        last = quarry.frame(at).cut;
    }
    Some((previous?, last?))
}

/// Printed and passed where the device cannot record a per-cluster cut.
fn no_cut() {
    eprintln!(
        "quarry freeze: this device records no per-cluster cut, so there is nothing to pin — see \
         harness::read_the_cut"
    );
}

/// **The cut stops following the camera, and an unpinned one does not.**
///
/// Both halves, in one run on one renderer, because either alone is worthless.
/// A renderer that ignored the camera entirely would satisfy the first; a test
/// that only moved the camera a little would satisfy it too, since a cut is
/// stable across small moves by design. So the camera is flown the dolly's whole
/// run — the move `detail_arrives_as_the_dolly_closes_on_the_face` measures a
/// mean level falling most of a level over — and the pin is then released at the
/// far end, where the cut has to move.
#[test]
fn the_frozen_cut_does_not_follow_the_camera() {
    let mut quarry = Quarry::open(Levels::Dag, MIXING_BUDGET);
    let Some((settling, pinned)) = settled(&mut quarry, DOLLY_START) else {
        no_cut();
        quarry.finish();
        return;
    };
    assert_eq!(
        settling, pinned,
        "the cut was still moving at a standing camera after {SETTLE} frames, so pinning it here \
         would pin a selection that goes on changing by itself"
    );

    let from = dolly(DOLLY_START).eye;
    quarry.renderer.set_frozen_selection_eye(Some(from));
    eprintln!("quarry freeze: pinned at {from:?}, cut {pinned:?}");

    for stop in 1..=STOPS {
        let at = DOLLY_START + (DOLLY_END - DOLLY_START) * stop as f32 / STOPS as f32;
        let frame = quarry.frame(at);
        let cut = frame.cut.expect("the cut came back a moment ago");
        eprintln!(
            "quarry freeze: pinned, camera at {at:.3} — cut {cut:?}, {} of {} pixels",
            frame.covered, frame.pixels,
        );
        assert_eq!(
            cut, pinned,
            "the camera reached {at:.3} of the dolly and the pinned cut moved with it — the \
             selection is still projecting from the camera",
        );
    }

    // And the same pose, unpinned, must *not* answer with the pinned cut —
    // otherwise every assertion above is about a renderer that ignores the
    // camera rather than about a pin that holds.
    quarry.renderer.set_frozen_selection_eye(None);
    let released = quarry
        .frame(DOLLY_END)
        .cut
        .expect("the cut came back a moment ago");
    eprintln!("quarry freeze: released at the far end — cut {released:?}");
    quarry.finish();
    assert_ne!(
        released, pinned,
        "releasing the pin at the far end of the dolly left the cut exactly where the near end \
         put it, so this renderer selects the same cut wherever the camera is and the assertions \
         above show nothing",
    );
}

/// **No pin is the same thing as a pin at the camera's own eye.**
///
/// The claim that every golden, every suite and every frame this engine has
/// already drawn is untouched: `None` is not a value the selection reads, it is
/// the camera's eye handed over exactly as it was before the field existed. Two
/// fixtures, the same pose, the same number of frames — one unpinned and one
/// pinned at the very point it is standing at — have to reach the same cut.
///
/// It is the cheapest thing here to get wrong: a `None` that fell through to a
/// zero, or to the last pin, would leave the whole engine selecting from the
/// world origin and still draw a plausible picture.
#[test]
fn no_pin_selects_exactly_as_a_pin_at_the_camera_does() {
    let mut live = Quarry::open(Levels::Dag, MIXING_BUDGET);
    assert_eq!(
        live.renderer.frozen_selection_eye(),
        None,
        "a renderer nobody pinned must follow the camera",
    );
    let Some((_, unpinned)) = settled(&mut live, DOLLY_START) else {
        no_cut();
        live.finish();
        return;
    };
    live.finish();

    let mut held = Quarry::open(Levels::Dag, MIXING_BUDGET);
    held.renderer
        .set_frozen_selection_eye(Some(dolly(DOLLY_START).eye));
    let (_, at_the_camera) = settled(&mut held, DOLLY_START).expect("the first fixture had a cut");
    held.finish();

    eprintln!("quarry freeze: unpinned {unpinned:?} against pinned-here {at_the_camera:?}");
    assert_eq!(
        unpinned, at_the_camera,
        "pinning the selection at the exact point the camera is standing changed the cut, so \
         `None` is not handing the descent the camera's eye",
    );
}

/// **The culls go on following the camera while the cut is pinned.**
///
/// The design decision this feature turns on, asserted rather than argued in a
/// comment. Only the *selection* freezes; everything that decides what is on
/// screen keeps asking about the eye the picture is drawn from. Freeze the
/// viewpoint wholesale — the obvious reading of "freeze the camera" — and the
/// feature destroys itself: a reviewer flies away from the pinned point and the
/// frame goes on drawing what the pinned point could see, which is a photograph
/// rather than a cut they can walk around.
///
/// The observable is the pair of numbers in one run. Pinned at the dolly's eye
/// and pointed at the face, the amplification stage keeps clusters and the frame
/// covers most of its pixels. Turn the camera around **without moving the pin**
/// and both fall to zero, because the frustum planes come from the frame's own
/// view-projection and not from the pin. A viewpoint that had frozen with the
/// selection would keep drawing the face into the back of the reviewer's head.
///
/// # The normal cone is live too, and this face cannot show it
///
/// `mesh_cluster.slang`'s `cluster_survives` also asks which way a cluster faces
/// **the viewer**, out of `FrameUniforms::camera_position` — which
/// `begin_frame` writes from `camera.eye` and never from the pin. That is
/// asserted nowhere here, and it is not for want of trying. Measured on
/// 2026-08-20: pinning the eye *underneath* the face and feeding that pin to the
/// cone as well — every cluster of the surface then facing away from its
/// viewer — changed the clusters kept from 44 to 42 and the covered pixels not
/// at all. So on this face the cone rejects almost nothing whichever side it is
/// asked from, and there is no assertion to build out of it. Why it rejects so
/// little is untested; the likely reason is that a rough surface gives clusters
/// cones wider than a hemisphere, which
/// `crcbl_shaders::meshlet::ClusterBounds::cone_cutoff` records as a cutoff at
/// or below zero and `cluster_survives` then skips outright.
/// `docs/backlog.md` carries it.
#[test]
fn the_culls_follow_the_live_camera_while_the_cut_is_pinned() {
    let mut quarry = Quarry::open(Levels::Dag, MIXING_BUDGET);
    if settled(&mut quarry, DOLLY_START).is_none() {
        no_cut();
        quarry.finish();
        return;
    }
    let facing = dolly(DOLLY_START);
    quarry.renderer.set_frozen_selection_eye(Some(facing.eye));

    let kept = |frame: &crate::harness::Frame| frame.culled.and_then(|stats| stats.clusters);
    let mut seen = quarry.frame(DOLLY_START);
    let held = kept(&seen).expect("the ring has come round by now, and this path counts clusters");
    let covered_facing = seen.covered;
    assert!(
        held > 0 && covered_facing * 5 > seen.pixels,
        "the pinned cut kept {held} cluster(s) and covered {covered_facing} of {} pixels \
         from the very pose it was pinned at, so there is nothing for the turn below to be \
         measured against",
        seen.pixels,
    );

    // Away: the same eye, the target reflected through it. The pin has not
    // moved, so anything that survives here survived a *live* frustum.
    let away = Camera {
        target: facing.eye * 2.0 - facing.target,
        ..facing
    };
    for _ in 0..SETTLE {
        seen = quarry.frame_from(&away);
    }
    let looking_away = kept(&seen).expect("the ring reports every frame");
    eprintln!(
        "quarry freeze: pinned at {:?}, {} pixels a frame — facing the face, {held} cluster(s) \
         and {covered_facing} covered; turned away, {looking_away} cluster(s) and {} covered",
        facing.eye, seen.pixels, seen.covered,
    );
    quarry.finish();
    assert_eq!(
        looking_away, 0,
        "the camera turned its back on the face and {looking_away} of {held} clusters still \
         survived — the culls are being run against the pinned viewpoint rather than against \
         the one the reviewer is looking through",
    );
    assert_eq!(
        seen.covered, 0,
        "the camera turned its back on the face and {} of {} pixels still drew",
        seen.covered, seen.pixels,
    );
}
