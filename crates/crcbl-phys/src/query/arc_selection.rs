//! Contact entry selection, including re-entry after a rejected departure.

use glam::DVec3;

use super::arc::{ARC_TIME_TOLERANCE, AcceleratedPath, ArcHit, earliest_contact};

pub(crate) fn contact_where(
    path: &AcceleratedPath,
    half: DVec3,
    radius: f64,
    support: impl Fn(DVec3) -> f64,
    separation: impl Fn(DVec3, DVec3) -> (f64, DVec3),
    nearest: impl Fn(f64, DVec3) -> DVec3,
    accept: impl Fn(&ArcHit) -> bool,
) -> Option<ArcHit> {
    let mut from = 0.0;
    loop {
        let remaining = AcceleratedPath::new(
            path.position_at(from),
            path.velocity_at(from),
            path.acceleration,
            path.duration - from,
        );
        let (elapsed, normal, inside) =
            earliest_contact(&remaining, half, radius, &support, &separation)?;
        let time = from + elapsed;
        let hit = ArcHit {
            time,
            point: nearest(time, normal),
            normal,
            started_inside: from == 0.0 && inside,
            part: 0,
        };
        if accept(&hit) {
            return Some(hit);
        }
        let after = time + ARC_TIME_TOLERANCE;
        if after <= time || after >= path.duration {
            return None;
        }
        let overlaps = |centre: DVec3| separation(centre - half, centre + half).0 <= 0.0;
        from = first_separation(path, after, path.duration, &overlaps)?;
    }
}

/// A quadratic arc is contained in the convex hull of its Bezier controls:
/// start, start + velocity * duration / 2, end. The set of capsule centres
/// overlapping a convex primitive is convex too (its Minkowski sum with the
/// reflected capsule). If all controls overlap, the entire interval overlaps.
/// Otherwise bisect chronologically, retaining departures even when both ends
/// overlap. Only intervals below the arc's time tolerance can be skipped.
fn first_separation(
    path: &AcceleratedPath,
    from: f64,
    to: f64,
    overlaps: &impl Fn(DVec3) -> bool,
) -> Option<f64> {
    let start = path.position_at(from);
    if !overlaps(start) {
        return Some(from);
    }
    let end = path.position_at(to);
    let control = start + path.velocity_at(from) * ((to - from) * 0.5);
    if overlaps(end) && overlaps(control) {
        return None;
    }
    let mid = from + (to - from) * 0.5;
    if to - from <= ARC_TIME_TOLERANCE || mid <= from || mid >= to {
        return (!overlaps(end)).then_some(to);
    }
    first_separation(path, from, mid, overlaps)
        .or_else(|| first_separation(path, mid, to, overlaps))
}
