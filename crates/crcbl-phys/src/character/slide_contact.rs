//! What each blocked sweep of a slide met, in the order it met it:
//! [`SlideContact`], recorded by
//! [`CharacterController::move_and_slide_into`](super::CharacterController::move_and_slide_into)
//! and
//! [`CharacterController::move_lying_into`](super::CharacterController::move_lying_into).

use glam::DVec3;

use crate::world::ColliderId;

/// One sweep of a [`move_and_slide_into`] or [`move_lying_into`] that met
/// something, and what the slide did about it.
///
/// A move is a loop of straight sweeps. Each one that hits a collider is one
/// contact, in the order the loop met them, so the list holds exactly
/// [`MoveOutcome::slides`] entries, or [`LyingMoveOutcome::slides`] for a
/// lying move. **Every hit is recorded, not only the ones that stopped the
/// character**: a floor a falling character lands on, a ceiling a jump
/// grazes, a wall it was already touching and moving along or away from —
/// each of those still costs a sweep and redirects what is left, and each can
/// be followed by a later wall within the same move.
///
/// # Fractions are distance along one straight sweep, never time
///
/// [`fraction`](Self::fraction) is the share of [`requested`](Self::requested)
/// covered before the capsule touched the collider, in `[0, 1]`. For a
/// [`clearance_only`](Self::clearance_only) contact, it is where the skin
/// envelope touched instead. It measures
/// distance along that one straight segment, from where the capsule stood when
/// the sweep began.
///
/// It is **not** a share of the tick, and not of the move either:
///
/// * Each sweep after the first starts from where the last one left the
///   capsule and asks for what the last contact left over
///   ([`remaining`](Self::remaining)), so its fraction is a share of that
///   remainder alone.
/// * The controller is kinematic and the move is a straight displacement the
///   caller has already integrated. A caller that integrates an acceleration —
///   gravity, an air-control curve — and hands over the chord of that arc gets
///   fractions along the chord. Where the arc itself would have touched the
///   collider, and when, is a different point at a different time, and nothing
///   here can recover it.
/// * Summing the displacement the contacts removed and dividing by the
///   requested length does not give elapsed time either: the slide redirects
///   the motion at each contact, so distance lost to one plane is not time
///   spent.
///
/// A caller that needs times along a curved path has to sweep the path in
/// pieces short enough that a chord stands in for the arc, and own that
/// approximation.
///
/// # How the fields fit together
///
/// For a physical contact, the capsule keeps a [`skin_width`] off the surface it met, measured along
/// [`normal`](Self::normal). With `direction = requested.normalize()`,
/// `along = fraction * |requested|` and `closing = -direction · normal`, a
/// sweep that did not step up and touched from further than that
/// (`along * closing > skin_width`) stops a skin width short across the
/// normal: `applied = direction * (along - skin_width / closing)`. One that
/// touched from no further — [`started_inside`](Self::started_inside), or
/// already that near — backs off along the normal instead:
/// `applied = normal * (skin_width - max(along * closing, 0))`, or not at all
/// (`applied` zero) when that back-off is too short to count as a move, so a
/// capsule already a skin width off the surface, to within rounding, stays
/// exactly where it is. A grounded
/// lying body measures both against a wall's normal made level, the plane its
/// slide clips against, rather than the leaning one recorded here. The next
/// contact's `requested` is this one's `remaining`, exactly.
///
/// A clearance-only contact already includes the skin in the swept radius.
/// It advances by `requested * fraction`, without subtracting another skin,
/// or backs out of the envelope's initial overlap. Only incoming clearance
/// contacts are recorded; the envelope does not block motion along or away
/// from a surface.
///
/// [`move_and_slide_into`]: super::CharacterController::move_and_slide_into
/// [`move_lying_into`]: super::CharacterController::move_lying_into
/// [`MoveOutcome::slides`]: super::MoveOutcome::slides
/// [`LyingMoveOutcome::slides`]: super::LyingMoveOutcome::slides
/// [`skin_width`]: super::CharacterConfig::skin_width
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlideContact {
    /// The collider met, as the world's other queries name it. Never the
    /// controller's own [self collider](super::CharacterController::with_self_collider)
    /// and never one off its [query mask](super::CharacterController::with_query_mask):
    /// the slide's sweeps do not see those.
    pub collider: ColliderId,
    /// Unit normal of the surface met, pointing away from it toward the
    /// capsule. Whether that is ground, wall or ceiling is
    /// [`is_walkable`](super::CharacterController::is_walkable) and
    /// [`is_ceiling`](super::CharacterController::is_ceiling) on it.
    pub normal: DVec3,
    /// The straight displacement this sweep asked for, from where the capsule
    /// stood when it began. The first contact's is the move's displacement
    /// after the grounded ramp adjustment
    /// ([`move_and_slide`](super::CharacterController::move_and_slide)
    /// describes it); each later one is the previous contact's
    /// [`remaining`](Self::remaining).
    pub requested: DVec3,
    /// How much of [`requested`](Self::requested) the capsule covered before
    /// touching, in `[0, 1]`, or before its skin envelope touched when
    /// [`clearance_only`](Self::clearance_only) is set. A distance share along that straight sweep, not
    /// a time: see [the type's notes](Self#fractions-are-distance-along-one-straight-sweep-never-time).
    pub fraction: f64,
    /// Whether the capsule (its skin envelope for a clearance-only contact)
    /// was already touching or inside the collider when
    /// the sweep began. The fraction is then zero whichever way the capsule
    /// was moving, and the slide backed it off the surface by a skin width
    /// instead of advancing it. A clearance-only contact backs out by the
    /// envelope's overlap depth instead.
    pub started_inside: bool,
    /// Whether only the skin envelope met this collider. The physical body
    /// sweep missed; `fraction` and `started_inside` describe the capsule
    /// enlarged by the configured skin width instead. Parallel and departing
    /// clearance contacts are ignored.
    pub clearance_only: bool,
    /// How far this sweep moved the capsule: the advance to a skin width short
    /// of the contact, or the back-off to a skin width of one that touched
    /// nearer than that, and the whole step when
    /// [`stepped_up`](Self::stepped_up) is set.
    pub applied: DVec3,
    /// What the slide carried past this contact: the part of
    /// [`requested`](Self::requested) the advance did not cover, clipped
    /// against every plane the move has collected — or less the step's
    /// horizontal advance instead, when it stepped up. A back-off off a
    /// surface it touched nearer than a skin width is not taken out of it.
    ///
    /// Zero when this contact stopped the move dead: a corner with no crease
    /// to run along, a clip that turned the motion against the direction the
    /// move started in, or more planes than the slide keeps. It is not zero,
    /// and is still not applied, when this was the last sweep
    /// [`max_slides`](super::CharacterConfig::max_slides) allowed, or when it is
    /// too short for the slide to sweep at all.
    pub remaining: DVec3,
    /// Whether the slide stepped up over this collider rather than sliding
    /// along it.
    pub stepped_up: bool,
}
