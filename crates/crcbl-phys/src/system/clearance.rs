//! [`PhysicsSystem::farthest_clear_point`]: how far a sphere gets along a
//! path before it would touch something.

use glam::DVec3;

use super::PhysicsSystem;
use crate::{QueryFilter, Segment};

impl PhysicsSystem {
    /// The farthest point from `from` towards `to` a sphere of `radius` can
    /// stand, `skin` metres short of the first collider it would touch.
    ///
    /// This is what a camera boom, a held weapon's muzzle or a spring arm
    /// asks: sweep from a point known to be clear — the head, the pivot —
    /// towards where the eye wants to be, and stop just in front of the wall.
    /// The skin keeps the near plane off the surface it stopped at. `to` comes
    /// back when nothing is in the way and when the path has no length; `from`
    /// when the sweep starts touching something or the skin is longer than the
    /// way to the hit. The skin is measured along the path, not along the
    /// surface's normal.
    ///
    /// `filter` leaves colliders out — the character the camera follows, at
    /// least — on [`QueryFilter`]'s terms. Triggers never stop it.
    #[must_use]
    pub fn farthest_clear_point(
        &mut self,
        from: DVec3,
        to: DVec3,
        radius: f64,
        skin: f64,
        filter: QueryFilter,
    ) -> DVec3 {
        let length = from.distance(to);
        if length == 0.0 {
            return to;
        }
        let Some((_, hit)) = self.sweep_sphere_filtered(&Segment::new(from, to), radius, filter)
        else {
            return to;
        };
        let fraction = (hit.t - skin / length).clamp(0.0, 1.0);
        from.lerp(to, fraction)
    }
}
