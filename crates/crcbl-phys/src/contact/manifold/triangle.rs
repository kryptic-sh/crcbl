//! A sphere, a capsule or a box against one triangle of a mesh — rung 5's
//! static-mesh contacts — with active edges.
//!
//! ```text
//!   the shape's centre behind the triangle's plane? ──▶ no contact
//!        │ no
//!   sphere:  the triangle's nearest point to the centre
//!   capsule: the nearest points of the core and the triangle; two points
//!            where it lies along the face or along an active edge
//!   box:     13 axes — the normal, 3 box faces, 9 edge crosses — the
//!            triangle's face preferred, as the box pair prefers A's
//!        │
//!   the feature touched is an inactive edge, or a vertex of two? ──▶ the
//!   triangle's own normal instead of the one found (Jolt's FixNormal)
//! ```
//!
//! The triangle is always shape `B`: it ranks above every shape that can
//! meet it, so the normal points from the shape into the triangle.
//!
//! # Sources
//!
//! - **One-sided**: Box2D v3, `b2CollideChainSegmentAndPolygon`, which
//!   returns no manifold for a polygon whose centroid is behind the segment.
//! - **Active edges**: Jolt Physics, `ActiveEdges::FixNormal`. Jolt keeps the
//!   contact's depth and replaces its normal; so does this. For a sphere or a
//!   capsule past an inactive edge the point's separation stays the true
//!   distance to the triangle, and only the direction it is solved along
//!   becomes the triangle's normal; for a box, the face contact the fix
//!   switches to clips the box to the triangle, and a point over the
//!   triangle is exactly as far as its height.
//! - **The box's axes** are Akenine-Möller's triangle–box separating axis
//!   test (`crate::mesh::geometry::box_triangle_axes`), the choice among them
//!   Gregorius's tolerances as the box pair has them, the clipping
//!   Sutherland–Hodgman against the triangle's three sides or the box face's
//!   four, and the reduction to four points the box pair's.
//!
//! # Feature ids
//!
//! A point is `pack(a, b)`: `a` the shape's feature — a capsule's end, `0` or
//! `1`, or `2` for its one nearest point; a box's corner, edge or face code
//! as the box pair has them — and `b` the triangle's, a vertex `1 + i`, an
//! edge `4 + i` or the face `7`. A point pushed along the face normal by the
//! active-edge fix is named against the face, since the face is what it
//! behaves as.

use glam::{DMat3, DQuat, DVec3};

use super::box_box::{
    ABSOLUTE_TOLERANCE, CLIP_TOLERANCE, Candidate, EDGE_RELATIVE_TOLERANCE,
    FACE_RELATIVE_TOLERANCE, MAX_CLIPPED, corner_code, edge_code, edge_index, face_code,
    face_index, others, pack, reduce, reference_corner_code, sign_of_bit,
};
use super::{Manifold, PARALLEL_SINE, closest_between_segments, closest_on_segment};
use crate::mesh::geometry::{
    Cuboid, Feature, SatAxis, Separation, box_triangle_axes, box_triangle_gap, closest_on_triangle,
    segment_triangle,
};

/// The face's code, which every point the active-edge fix turns is named by.
const FACE: u32 = Feature::Face.code();

/// Whether a shape whose centre is `centre` is behind the triangle, and so
/// not collided with it.
fn behind(centre: DVec3, corners: &[DVec3; 3], normal: DVec3) -> bool {
    normal.dot(centre - corners[0]) < 0.0
}

/// A sphere against a triangle: one point, at the triangle's nearest point.
pub(super) fn sphere_triangle(
    centre: DVec3,
    radius: f64,
    corners: &[DVec3; 3],
    normal: DVec3,
    active: u8,
    speculative: f64,
) -> Manifold {
    if behind(centre, corners, normal) {
        return Manifold::EMPTY;
    }
    let closest = closest_on_triangle(centre, corners);
    let away = centre - closest.point;
    let reach = radius + speculative;
    let distance_squared = away.length_squared();
    if distance_squared > reach * reach {
        return Manifold::EMPTY;
    }
    let distance = distance_squared.sqrt();
    let feature = closest.feature;
    let (outward, code) = if feature != Feature::Face && feature.is_active(active) && distance > 0.0
    {
        (away / distance, feature.code())
    } else {
        (normal, FACE)
    };
    let mut manifold = Manifold::with_normal(-outward);
    manifold.push(
        centre - outward * (0.5 * (radius + distance)),
        distance - radius,
        pack(0, code),
    );
    manifold
}

/// A capsule of core `p0`–`p1` against a triangle: two points where it lies
/// along the face or an active edge, one elsewhere.
#[allow(clippy::too_many_arguments)]
pub(super) fn capsule_triangle(
    p0: DVec3,
    p1: DVec3,
    radius: f64,
    corners: &[DVec3; 3],
    normal: DVec3,
    active: u8,
    speculative: f64,
) -> Manifold {
    if behind((p0 + p1) * 0.5, corners, normal) {
        return Manifold::EMPTY;
    }
    let closest = segment_triangle(p0, p1, corners, normal);
    let away = closest.on_segment - closest.on_triangle.point;
    let reach = radius + speculative;
    let distance_squared = away.length_squared();
    if distance_squared > reach * reach {
        return Manifold::EMPTY;
    }
    let distance = distance_squared.sqrt();
    let along = p1 - p0;
    let feature = closest.on_triangle.feature;
    let turned = feature != Feature::Face && !feature.is_active(active);
    let flat = {
        let rise = along.dot(normal);
        rise * rise <= PARALLEL_SINE * PARALLEL_SINE * along.length_squared()
    };

    if feature == Feature::Face || turned || flat {
        // Against the face: both ends of the stretch over the triangle.
        let mut manifold = Manifold::with_normal(-normal);
        if let Some((lo, hi)) = clip_to_prism(p0, along, corners, normal) {
            let params: &[(u32, f64)] = if hi > lo {
                &[(0, lo), (1, hi)]
            } else {
                &[(0, lo)]
            };
            for &(id, t) in params {
                let point = p0 + along * t;
                let height = normal.dot(point - corners[0]);
                let separation = height - radius;
                if separation <= speculative {
                    manifold.push(
                        point - normal * (0.5 * (radius + height)),
                        separation,
                        pack(id, FACE),
                    );
                }
            }
        }
        if manifold.count > 0 {
            return manifold;
        }
        if feature == Feature::Face || turned {
            // Over a neighbour, past an inactive edge: the nearest point, at
            // its true distance, pushed along the face's normal.
            manifold.push(
                closest.on_segment - normal * (0.5 * (radius + distance)),
                distance - radius,
                pack(2, FACE),
            );
            return manifold;
        }
    }

    // An active edge or vertex.
    let outward = if distance > 0.0 {
        away / distance
    } else {
        normal
    };
    let mut manifold = Manifold::with_normal(-outward);
    // Lying along an active edge, the nearest point could be anywhere on the
    // stretch they share: rest on both ends of it, as the box pair does.
    let edges: &[usize] = match feature {
        Feature::Edge(i) => &[i],
        Feature::Vertex(0) => &[0, 2],
        Feature::Vertex(1) => &[1, 0],
        Feature::Vertex(_) => &[2, 1],
        Feature::Face => &[],
    };
    for &e in edges {
        let (a, b) = (corners[e], corners[(e + 1) % 3]);
        let edge = b - a;
        let parallel = along.cross(edge).length_squared()
            <= PARALLEL_SINE * PARALLEL_SINE * along.length_squared() * edge.length_squared();
        if active & (1 << e) == 0 || !parallel {
            continue;
        }
        let Some((lo, hi)) = clip_to_edge(p0, along, a, edge) else {
            continue;
        };
        if hi <= lo {
            continue;
        }
        for (id, t) in [(0u32, lo), (1, hi)] {
            let point = p0 + along * t;
            let (_, on_edge) = closest_on_segment(point, a, b);
            let gap = (point - on_edge).dot(outward);
            let separation = gap - radius;
            if separation <= speculative {
                manifold.push(
                    point - outward * (0.5 * (radius + gap)),
                    separation,
                    pack(id, Feature::Edge(e).code()),
                );
            }
        }
        if manifold.count > 0 {
            return manifold;
        }
    }
    manifold.push(
        closest.on_segment - outward * (0.5 * (radius + distance)),
        distance - radius,
        pack(2, feature.code()),
    );
    manifold
}

/// The stretch of `p0 + along · t`, `t ∈ [0, 1]`, over the triangle: inside
/// its three side planes, each pushed out by [`CLIP_TOLERANCE`].
fn clip_to_prism(
    p0: DVec3,
    along: DVec3,
    corners: &[DVec3; 3],
    normal: DVec3,
) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for e in 0..3 {
        let side = (corners[(e + 1) % 3] - corners[e])
            .cross(normal)
            .normalize();
        let offset = side.dot(corners[e]) + CLIP_TOLERANCE;
        let start = side.dot(p0) - offset;
        let rate = side.dot(along);
        if rate == 0.0 {
            if start > 0.0 {
                return None;
            }
            continue;
        }
        let t = -start / rate;
        if rate > 0.0 {
            hi = hi.min(t);
        } else {
            lo = lo.max(t);
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// The stretch of `p0 + along · t`, `t ∈ [0, 1]`, alongside the edge from `a`
/// along `edge`: where its projection onto the edge falls within it.
fn clip_to_edge(p0: DVec3, along: DVec3, a: DVec3, edge: DVec3) -> Option<(f64, f64)> {
    let length_squared = edge.length_squared();
    let start = (p0 - a).dot(edge) / length_squared;
    let rate = along.dot(edge) / length_squared;
    if rate == 0.0 {
        return (0.0..=1.0).contains(&start).then_some((0.0, 1.0));
    }
    let t0 = -start / rate;
    let t1 = (1.0 - start) / rate;
    let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
    (lo <= hi).then_some((lo, hi))
}

/// An oriented box as a [`Cuboid`].
fn cuboid(centre: DVec3, rotation: DQuat, half: DVec3) -> Cuboid {
    let m = DMat3::from_quat(rotation);
    Cuboid {
        centre,
        axes: [m.x_axis, m.y_axis, m.z_axis],
        half,
    }
}

/// The box's corner `c`: bit `k` set is the positive end of axis `k`.
fn corner(cuboid: &Cuboid, c: usize) -> DVec3 {
    (0..3).fold(cuboid.centre, |p, k| {
        p + cuboid.axes[k] * (sign_of_bit(c, k) * cuboid.half[k])
    })
}

/// A box against a triangle: see the module docs.
#[allow(clippy::too_many_arguments)]
pub(super) fn box_triangle(
    centre: DVec3,
    rotation: DQuat,
    half: DVec3,
    corners: &[DVec3; 3],
    normal: DVec3,
    active: u8,
    speculative: f64,
) -> Manifold {
    if behind(centre, corners, normal) {
        return Manifold::EMPTY;
    }
    let cuboid = cuboid(centre, rotation, half);
    let axes = box_triangle_axes(&cuboid, corners, normal);
    if axes
        .iter()
        .flatten()
        .any(|axis| axis.separation > speculative)
    {
        return Manifold::EMPTY;
    }
    let best = |range: core::ops::Range<usize>| {
        axes[range]
            .iter()
            .flatten()
            .fold(None::<Separation>, |best, &next| match best {
                Some(best) if best.separation >= next.separation => Some(best),
                _ => Some(next),
            })
    };
    let face = axes[0].expect("the face axis always exists");
    let box_face = best(1..4).expect("a box face axis always exists");
    let edge = best(4..13);

    let face_best = face.separation.max(box_face.separation);
    let edge_wins = edge.is_some_and(|edge| {
        edge.separation > face_best + EDGE_RELATIVE_TOLERANCE * face_best.abs() + ABSOLUTE_TOLERANCE
    });
    let box_wins = box_face.separation
        > face.separation + FACE_RELATIVE_TOLERANCE * face.separation.abs() + ABSOLUTE_TOLERANCE;
    let mut chosen = match edge {
        Some(edge) if edge_wins => edge,
        _ if box_wins => box_face,
        _ => face,
    };
    // Active edges: what the chosen axis touches on the triangle.
    let touched = match chosen.axis {
        SatAxis::Face => Feature::Face,
        SatAxis::Edge(_, e) => Feature::Edge(e),
        SatAxis::BoxFace(_) => support_feature(corners, chosen.normal),
    };
    if !touched.is_active(active) {
        chosen = face;
    }
    match chosen.axis {
        SatAxis::Face => face_contact(&cuboid, corners, normal, speculative),
        SatAxis::BoxFace(k) => box_face_contact(&cuboid, k, chosen.normal, corners, speculative),
        SatAxis::Edge(k, e) => edge_contact(&cuboid, k, e, chosen.normal, corners, speculative),
    }
}

/// The triangle's feature nearest a box along `towards`, the axis from the
/// box to it: the corners least far along it, within [`CLIP_TOLERANCE`].
fn support_feature(corners: &[DVec3; 3], towards: DVec3) -> Feature {
    let depth = corners.map(|c| towards.dot(c));
    let least = depth[0].min(depth[1]).min(depth[2]);
    let near: [bool; 3] = core::array::from_fn(|i| depth[i] <= least + CLIP_TOLERANCE);
    match near {
        [true, true, true] => Feature::Face,
        [true, true, false] => Feature::Edge(0),
        [false, true, true] => Feature::Edge(1),
        [true, false, true] => Feature::Edge(2),
        [true, false, false] => Feature::Vertex(0),
        [false, true, false] => Feature::Vertex(1),
        _ => Feature::Vertex(2),
    }
}

/// What a clipped polygon's edge from one vertex to the next lies along.
#[derive(Clone, Copy, Debug)]
enum Along {
    /// An edge of the box, by edge index.
    BoxEdge(usize),
    /// A side face of the box's reference face, by face index.
    BoxSide(usize),
    /// An edge of the triangle.
    TriangleEdge(usize),
    /// A side plane of the triangle, through its edge.
    TriangleSide(usize),
}

/// A vertex of a polygon being clipped, named by the box's feature it lies
/// on and the triangle's.
#[derive(Clone, Copy, Debug)]
struct ClipVertex {
    point: DVec3,
    on_box: u32,
    on_triangle: u32,
    out: Along,
}

/// A polygon of at most [`MAX_CLIPPED`] vertices.
#[derive(Clone, Copy, Debug)]
struct Polygon {
    vertices: [ClipVertex; MAX_CLIPPED],
    count: usize,
}

impl Polygon {
    const EMPTY: Self = Self {
        vertices: [ClipVertex {
            point: DVec3::ZERO,
            on_box: 0,
            on_triangle: 0,
            out: Along::BoxEdge(0),
        }; MAX_CLIPPED],
        count: 0,
    };

    fn push(&mut self, vertex: ClipVertex) {
        debug_assert!(self.count < MAX_CLIPPED, "one more vertex per plane");
        self.vertices[self.count] = vertex;
        self.count += 1;
    }
}

/// The part of `input` on the inner side of `normal · x ≤ offset`, the plane
/// `side`: Sutherland–Hodgman, as the box pair clips, with each new vertex
/// named by `crossing` from what the edge it cut lies along.
fn clip(
    input: &Polygon,
    normal: DVec3,
    offset: f64,
    side: Along,
    crossing: impl Fn(Along) -> (u32, u32),
) -> Polygon {
    let mut output = Polygon::EMPTY;
    for k in 0..input.count {
        let v0 = input.vertices[k];
        let v1 = input.vertices[(k + 1) % input.count];
        let d0 = normal.dot(v0.point) - offset;
        let d1 = normal.dot(v1.point) - offset;
        if d0 <= 0.0 {
            let mut kept = v0;
            if d0 == 0.0 && d1 > 0.0 {
                kept.out = side;
            }
            output.push(kept);
        }
        if (d0 < 0.0 && d1 > 0.0) || (d0 > 0.0 && d1 < 0.0) {
            let t = d0 / (d0 - d1);
            let (on_box, on_triangle) = crossing(v0.out);
            output.push(ClipVertex {
                point: v0.point + (v1.point - v0.point) * t,
                on_box,
                on_triangle,
                out: if d0 < 0.0 { side } else { v0.out },
            });
        }
    }
    output
}

/// The triangle's vertex where its edges `e1` and `e2` meet.
fn shared_vertex(e1: usize, e2: usize) -> usize {
    if e2 == (e1 + 1) % 3 { e2 } else { e1 }
}

/// Points within `speculative` of a reference face of outward normal
/// `outward` at height `face_offset`, reduced to four, as a manifold of
/// `normal`.
fn finish(
    polygon: &Polygon,
    outward: DVec3,
    face_offset: f64,
    normal: DVec3,
    speculative: f64,
) -> Manifold {
    let mut candidates = [Candidate::default(); MAX_CLIPPED];
    let mut count = 0;
    for vertex in &polygon.vertices[..polygon.count] {
        let separation = outward.dot(vertex.point) - face_offset;
        if separation > speculative {
            continue;
        }
        candidates[count] = Candidate {
            point: vertex.point - outward * (0.5 * separation),
            separation,
            id: pack(vertex.on_box, vertex.on_triangle),
        };
        count += 1;
    }
    let mut manifold = Manifold::with_normal(normal);
    let (kept, kept_count) = reduce(&candidates[..count], normal);
    for &index in &kept[..kept_count] {
        let c = candidates[index];
        manifold.push(c.point, c.separation, c.id);
    }
    manifold
}

/// The box's face turned most against the triangle's normal, clipped to the
/// triangle's three sides.
fn face_contact(
    cuboid: &Cuboid,
    corners: &[DVec3; 3],
    normal: DVec3,
    speculative: f64,
) -> Manifold {
    let mut k = 0;
    let mut most = f64::NEG_INFINITY;
    for candidate in 0..3 {
        let along = cuboid.axes[candidate].dot(normal).abs();
        if along > most {
            most = along;
            k = candidate;
        }
    }
    let positive = cuboid.axes[k].dot(normal) < 0.0;
    let incident_face = face_index(k, positive);
    let (u, v) = others(k);
    let base = usize::from(positive) << k;
    let quad = [
        base,
        base | (1 << u),
        base | (1 << u) | (1 << v),
        base | (1 << v),
    ];
    let mut polygon = Polygon::EMPTY;
    for n in 0..4 {
        let (c0, c1) = (quad[n], quad[(n + 1) % 4]);
        polygon.push(ClipVertex {
            point: corner(cuboid, c0),
            on_box: corner_code(c0),
            on_triangle: FACE,
            out: Along::BoxEdge(edge_index((c0 ^ c1).trailing_zeros() as usize, c0)),
        });
    }
    for e in 0..3 {
        let side = (corners[(e + 1) % 3] - corners[e])
            .cross(normal)
            .normalize();
        polygon = clip(
            &polygon,
            side,
            side.dot(corners[e]) + CLIP_TOLERANCE,
            Along::TriangleSide(e),
            |out| match out {
                Along::BoxEdge(edge) => (edge_code(edge), Feature::Edge(e).code()),
                Along::TriangleSide(other) => (
                    face_code(incident_face),
                    Feature::Vertex(shared_vertex(other, e)).code(),
                ),
                Along::BoxSide(_) | Along::TriangleEdge(_) => {
                    unreachable!("a box face is clipped by the triangle's sides only")
                }
            },
        );
    }
    finish(
        &polygon,
        normal,
        normal.dot(corners[0]),
        -normal,
        speculative,
    )
}

/// The triangle clipped to the box's face on axis `k` that faces it along
/// `towards`.
fn box_face_contact(
    cuboid: &Cuboid,
    k: usize,
    towards: DVec3,
    corners: &[DVec3; 3],
    speculative: f64,
) -> Manifold {
    let reference_face = face_index(k, cuboid.axes[k].dot(towards) > 0.0);
    let mut polygon = Polygon::EMPTY;
    for (i, &point) in corners.iter().enumerate() {
        polygon.push(ClipVertex {
            point,
            on_box: face_code(reference_face),
            on_triangle: Feature::Vertex(i).code(),
            out: Along::TriangleEdge(i),
        });
    }
    let (su, sv) = others(k);
    for side in [su, sv] {
        for positive in [true, false] {
            let plane = if positive {
                cuboid.axes[side]
            } else {
                -cuboid.axes[side]
            };
            let face = face_index(side, positive);
            polygon = clip(
                &polygon,
                plane,
                plane.dot(cuboid.centre) + cuboid.half[side] + CLIP_TOLERANCE,
                Along::BoxSide(face),
                |out| match out {
                    Along::TriangleEdge(e) => (face_code(face), Feature::Edge(e).code()),
                    Along::BoxSide(other) => {
                        (reference_corner_code(other, face, reference_face), FACE)
                    }
                    Along::BoxEdge(_) | Along::TriangleSide(_) => {
                        unreachable!("a triangle is clipped by the box's sides only")
                    }
                },
            );
        }
    }
    finish(
        &polygon,
        towards,
        towards.dot(cuboid.centre) + cuboid.half[k],
        towards,
        speculative,
    )
}

/// One point where the box's edge along axis `k` nearest the triangle meets
/// the triangle's edge `e`, `towards` the axis from the box to the triangle.
fn edge_contact(
    cuboid: &Cuboid,
    k: usize,
    e: usize,
    towards: DVec3,
    corners: &[DVec3; 3],
    speculative: f64,
) -> Manifold {
    let mut middle = cuboid.centre;
    let mut c = 0;
    for j in (0..3).filter(|&j| j != k) {
        let positive = cuboid.axes[j].dot(towards) >= 0.0;
        c |= usize::from(positive) << j;
        middle += cuboid.axes[j] * (if positive { 1.0 } else { -1.0 } * cuboid.half[j]);
    }
    let reach = cuboid.axes[k] * cuboid.half[k];
    let (on_box, on_triangle) = closest_between_segments(
        middle - reach,
        middle + reach,
        corners[e],
        corners[(e + 1) % 3],
    );
    let separation = (on_triangle - on_box).dot(towards);
    let mut manifold = Manifold::with_normal(towards);
    if separation <= speculative {
        manifold.push(
            (on_box + on_triangle) * 0.5,
            separation,
            pack(edge_code(edge_index(k, c)), Feature::Edge(e).code()),
        );
    }
    manifold
}

/// How far a sphere is from a triangle, and the direction from it to the
/// triangle: exact, and infinite from behind.
pub(super) fn sphere_gap(
    centre: DVec3,
    radius: f64,
    corners: &[DVec3; 3],
    normal: DVec3,
) -> (f64, DVec3) {
    if behind(centre, corners, normal) {
        return (f64::INFINITY, -normal);
    }
    let towards = closest_on_triangle(centre, corners).point - centre;
    let distance = towards.length();
    if distance > 0.0 {
        (distance - radius, towards / distance)
    } else {
        (-radius, -normal)
    }
}

/// How far a capsule is from a triangle, and the direction from it to the
/// triangle: exact apart, infinite from behind, and for a core through the
/// face, minus how far its lower end would have to rise to clear it.
pub(super) fn capsule_gap(
    p0: DVec3,
    p1: DVec3,
    radius: f64,
    corners: &[DVec3; 3],
    normal: DVec3,
) -> (f64, DVec3) {
    if behind((p0 + p1) * 0.5, corners, normal) {
        return (f64::INFINITY, -normal);
    }
    let closest = segment_triangle(p0, p1, corners, normal);
    let towards = closest.on_triangle.point - closest.on_segment;
    let distance = towards.length();
    if distance > 0.0 {
        (distance - radius, towards / distance)
    } else {
        let lowest = normal.dot(p0 - corners[0]).min(normal.dot(p1 - corners[0]));
        (lowest - radius, -normal)
    }
}

/// How far a box is from a triangle, and the direction from it to the
/// triangle: the best of the thirteen axes, a lower bound apart, and infinite
/// from behind.
pub(super) fn box_gap(
    centre: DVec3,
    rotation: DQuat,
    half: DVec3,
    corners: &[DVec3; 3],
    normal: DVec3,
) -> (f64, DVec3) {
    if behind(centre, corners, normal) {
        return (f64::INFINITY, -normal);
    }
    let best = box_triangle_gap(&cuboid(centre, rotation, half), corners, normal);
    (best.separation, best.normal)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: f64 = 0.02;

    /// The floor triangle `(0,0,0)`, `(0,0,2)`, `(2,0,0)`, normal up.
    const FLOOR: [DVec3; 3] = [
        DVec3::ZERO,
        DVec3::new(0.0, 0.0, 2.0),
        DVec3::new(2.0, 0.0, 0.0),
    ];

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    fn near(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-12
    }

    /// **A sphere on the face** is one point a centimetre deep, midway, the
    /// normal straight down into the triangle, named by the face. **Beside a
    /// boundary edge**, active, it pushes out from the edge; the same edge
    /// inactive pushes along the face's normal at the same true depth.
    /// **Behind the triangle** it is nothing.
    #[test]
    fn a_sphere_against_a_triangle_by_hand() {
        let m = sphere_triangle(
            DVec3::new(0.5, 0.49, 0.5),
            0.5,
            &FLOOR,
            DVec3::Y,
            0b111,
            SPEC,
        );
        assert_eq!(m.points().len(), 1);
        assert_eq!(m.normal, -DVec3::Y);
        let p = m.points()[0];
        assert!(close(p.separation, -0.01), "{m:?}");
        assert!(near(p.point, DVec3::new(0.5, -0.005, 0.5)), "{m:?}");
        assert_eq!(p.id, pack(0, FACE));

        // 0.3 beside edge 0 (x = 0) and 0.4 up: 0.5 from (0, 0, 0.5), radius
        // 0.6, so 0.1 deep along (-0.6, 0.8, 0).
        let beside = DVec3::new(-0.3, 0.4, 0.5);
        let m = sphere_triangle(beside, 0.6, &FLOOR, DVec3::Y, 0b111, SPEC);
        assert!(near(m.normal, DVec3::new(0.6, -0.8, 0.0)), "{m:?}");
        assert!(close(m.points()[0].separation, -0.1), "{m:?}");
        assert_eq!(m.points()[0].id, pack(0, Feature::Edge(0).code()));
        let m = sphere_triangle(beside, 0.6, &FLOOR, DVec3::Y, 0b110, SPEC);
        assert_eq!(m.normal, -DVec3::Y, "{m:?}");
        assert!(close(m.points()[0].separation, -0.1), "{m:?}");
        assert_eq!(m.points()[0].id, pack(0, FACE));

        let under = sphere_triangle(
            DVec3::new(0.5, -0.1, 0.5),
            0.5,
            &FLOOR,
            DVec3::Y,
            0b111,
            SPEC,
        );
        assert!(under.points().is_empty(), "{under:?}");
    }

    /// **A capsule lying on the face** rests on both ends of its core; one
    /// lying across the hypotenuse rests on the stretch over the triangle,
    /// cut where it crosses the edge's side plane.
    #[test]
    fn a_capsule_against_a_triangle_by_hand() {
        let m = capsule_triangle(
            DVec3::new(0.2, 0.19, 0.5),
            DVec3::new(1.2, 0.19, 0.5),
            0.2,
            &FLOOR,
            DVec3::Y,
            0b111,
            SPEC,
        );
        assert_eq!(m.points().len(), 2, "{m:?}");
        assert_eq!(m.normal, -DVec3::Y);
        for p in m.points() {
            assert!(close(p.separation, -0.01), "{m:?}");
            assert!(close(p.point.y, -0.005), "{m:?}");
        }
        let mut xs: Vec<f64> = m.points().iter().map(|p| p.point.x).collect();
        xs.sort_by(f64::total_cmp);
        assert!(close(xs[0], 0.2) && close(xs[1], 1.2), "{m:?}");

        // Along z = 1.5 from x = 0 to 2: over the triangle up to x = 0.5, and
        // the side plane is pushed out by the clip tolerance, so the cut is
        // CLIP_TOLERANCE √2 further along.
        let m = capsule_triangle(
            DVec3::new(0.0, 0.2, 1.5),
            DVec3::new(2.0, 0.2, 1.5),
            0.2,
            &FLOOR,
            DVec3::Y,
            0b111,
            SPEC,
        );
        assert_eq!(m.points().len(), 2, "{m:?}");
        let mut xs: Vec<f64> = m.points().iter().map(|p| p.point.x).collect();
        xs.sort_by(f64::total_cmp);
        let cut = 0.5 + CLIP_TOLERANCE * core::f64::consts::SQRT_2;
        assert!(close(xs[0], 0.0) && close(xs[1], cut), "{xs:?}");
    }

    /// **A box resting on the face** stands on its four lower corners, a
    /// centimetre deep, named by its corners against the face.
    #[test]
    fn a_box_on_a_triangle_stands_on_its_corners() {
        let m = box_triangle(
            DVec3::new(0.5, 0.24, 0.5),
            DQuat::IDENTITY,
            DVec3::splat(0.25),
            &FLOOR,
            DVec3::Y,
            0b111,
            SPEC,
        );
        assert_eq!(m.points().len(), 4, "{m:?}");
        assert_eq!(m.normal, -DVec3::Y);
        let mut xz: Vec<(f64, f64)> = m.points().iter().map(|p| (p.point.x, p.point.z)).collect();
        xz.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        assert_eq!(xz, [(0.25, 0.25), (0.25, 0.75), (0.75, 0.25), (0.75, 0.75)]);
        for p in m.points() {
            assert!(close(p.separation, -0.01), "{m:?}");
            assert!(close(p.point.y, -0.005), "{m:?}");
            assert_eq!(p.id >> 8, FACE, "{m:?}");
            let corner = (p.id & 0xff) - 1;
            assert!(corner < 8 && corner & 0b010 == 0, "a lower corner: {m:?}");
        }
    }

    /// **A box over the hypotenuse** is clipped to the triangle: its bottom
    /// face `[0.85, 1.35]²` keeps the corner `(0.85, 0.85)` and where its
    /// two edges through that corner cross `x + z = 2`, pushed out by the
    /// clip tolerance.
    #[test]
    fn a_box_over_the_hypotenuse_is_clipped_to_it() {
        let m = box_triangle(
            DVec3::new(1.1, 0.24, 1.1),
            DQuat::IDENTITY,
            DVec3::splat(0.25),
            &FLOOR,
            DVec3::Y,
            0b111,
            SPEC,
        );
        assert_eq!(m.normal, -DVec3::Y);
        let reach = 2.0 + CLIP_TOLERANCE * core::f64::consts::SQRT_2;
        let mut xz: Vec<(f64, f64)> = m.points().iter().map(|p| (p.point.x, p.point.z)).collect();
        xz.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        assert_eq!(xz.len(), 3, "{m:?}");
        assert!(close(xz[0].0, 0.85) && close(xz[0].1, 0.85), "{xz:?}");
        assert!(
            close(xz[1].0, 0.85) && close(xz[1].1, reach - 0.85),
            "{xz:?}"
        );
        assert!(
            close(xz[2].0, reach - 0.85) && close(xz[2].1, 0.85),
            "{xz:?}"
        );
        for p in m.points() {
            assert!(close(p.separation, -0.01), "{m:?}");
        }
    }

    /// **A box sliding up to a seam.** Its leading face a millimetre short of
    /// the triangle's edge 0 and its bottom two millimetres into the plane:
    /// with the edge active the triangle's edge is in its way, a speculative
    /// contact 1 mm ahead pushing back along its face — the ghost collision;
    /// with the edge inactive it is not over the triangle yet, and the face
    /// contact it is given instead has no points.
    #[test]
    fn an_inactive_seam_is_not_in_a_sliding_boxs_way() {
        let centre = DVec3::new(-0.251, 0.248, 1.0);
        let active = box_triangle(
            centre,
            DQuat::IDENTITY,
            DVec3::splat(0.25),
            &FLOOR,
            DVec3::Y,
            0b111,
            SPEC,
        );
        assert_eq!(active.normal, DVec3::X, "the box's own face: {active:?}");
        assert_eq!(active.points().len(), 2, "{active:?}");
        for p in active.points() {
            assert!(close(p.separation, 0.001), "{active:?}");
        }
        let inactive = box_triangle(
            centre,
            DQuat::IDENTITY,
            DVec3::splat(0.25),
            &FLOOR,
            DVec3::Y,
            0b110,
            SPEC,
        );
        assert!(inactive.points().is_empty(), "{inactive:?}");
    }

    /// The gaps are exact for round shapes and a lower bound for a box, and
    /// every shape behind is infinitely far.
    #[test]
    fn the_gaps_measure_from_the_front() {
        let (d, n) = sphere_gap(DVec3::new(0.5, 2.0, 0.5), 0.5, &FLOOR, DVec3::Y);
        assert!(close(d, 1.5) && n == -DVec3::Y, "{d} {n:?}");
        let (d, _) = capsule_gap(
            DVec3::new(0.5, 2.0, 0.5),
            DVec3::new(0.5, 3.0, 0.5),
            0.5,
            &FLOOR,
            DVec3::Y,
        );
        assert!(close(d, 1.5), "{d}");
        let (d, n) = box_gap(
            DVec3::new(0.5, 1.0, 0.5),
            DQuat::IDENTITY,
            DVec3::splat(0.25),
            &FLOOR,
            DVec3::Y,
        );
        assert!(close(d, 0.75) && n == -DVec3::Y, "{d} {n:?}");
        assert_eq!(
            sphere_gap(DVec3::new(0.5, -0.1, 0.5), 0.5, &FLOOR, DVec3::Y).0,
            f64::INFINITY
        );
    }
}
