//! Triangle geometry the mesh's queries and the contact pipeline share: the
//! nearest point of a triangle, of a segment and a triangle, a ray through a
//! triangle, and a point swept into a triangle fattened by a radius.
//!
//! A triangle is `[a, b, c]`, wound so `(b − a) × (c − a)` is its normal, and
//! its features are numbered as [`Feature`] says. Every function is `+ − × ÷`
//! and `sqrt`, so every target reaches the same bits.

use glam::DVec3;

/// A feature of a triangle `[v0, v1, v2]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Feature {
    /// Vertex `i`.
    Vertex(usize),
    /// Edge `i`, from vertex `i` to vertex `i + 1` (mod 3).
    Edge(usize),
    /// The face.
    Face,
}

impl Feature {
    /// Whether contact with this feature may push along its own direction
    /// rather than the face normal, under a triangle's active-edge bits (bit
    /// `i` set: edge `i` is active).
    ///
    /// The face always may. An edge may when it is active; a vertex when
    /// either edge meeting there is — Jolt Physics' `ActiveEdges::FixNormal`,
    /// which asks "edge 0 or 2 needs to be active" of vertex 0.
    pub(crate) const fn is_active(self, active: u8) -> bool {
        match self {
            Self::Face => true,
            Self::Edge(i) => active & (1 << i) != 0,
            Self::Vertex(i) => active & ((1 << i) | (1 << ((i + 2) % 3))) != 0,
        }
    }

    /// The feature's code in a contact's feature id: a vertex `1 + i`, an
    /// edge `4 + i`, the face `7`.
    pub(crate) const fn code(self) -> u32 {
        match self {
            Self::Vertex(i) => 1 + i as u32,
            Self::Edge(i) => 4 + i as u32,
            Self::Face => 7,
        }
    }
}

/// A point of a triangle, with its barycentric weights and the feature it
/// lies on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Closest {
    pub(crate) point: DVec3,
    /// The weights of `v0`, `v1` and `v2` that make [`point`](Self::point).
    pub(crate) barycentric: DVec3,
    pub(crate) feature: Feature,
}

/// The point of triangle `tri` nearest `p` — Ericson, _Real-Time Collision
/// Detection_, §5.1.5, `ClosestPtPointTriangle`, which finds the Voronoi
/// region of `p` and so names the feature the point lies on.
pub(crate) fn closest_on_triangle(p: DVec3, tri: &[DVec3; 3]) -> Closest {
    let [a, b, c] = *tri;
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return vertex(tri, 0);
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return vertex(tri, 1);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return Closest {
            point: a + ab * v,
            barycentric: DVec3::new(1.0 - v, v, 0.0),
            feature: Feature::Edge(0),
        };
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return vertex(tri, 2);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return Closest {
            point: a + ac * w,
            barycentric: DVec3::new(1.0 - w, 0.0, w),
            feature: Feature::Edge(2),
        };
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return Closest {
            point: b + (c - b) * w,
            barycentric: DVec3::new(0.0, 1.0 - w, w),
            feature: Feature::Edge(1),
        };
    }
    let denominator = 1.0 / (va + vb + vc);
    let v = vb * denominator;
    let w = vc * denominator;
    Closest {
        point: a + ab * v + ac * w,
        barycentric: DVec3::new(1.0 - v - w, v, w),
        feature: Feature::Face,
    }
}

fn vertex(tri: &[DVec3; 3], i: usize) -> Closest {
    let mut barycentric = DVec3::ZERO;
    barycentric[i] = 1.0;
    Closest {
        point: tri[i],
        barycentric,
        feature: Feature::Vertex(i),
    }
}

/// The barycentric weights of `p`, a point in the triangle's plane — Ericson
/// §3.4, `Barycentric`, by the ratios of the sub-triangles' areas.
pub(crate) fn barycentric(p: DVec3, tri: &[DVec3; 3]) -> DVec3 {
    let [a, b, c] = *tri;
    let v0 = b - a;
    let v1 = c - a;
    let v2 = p - a;
    let d00 = v0.dot(v0);
    let d01 = v0.dot(v1);
    let d11 = v1.dot(v1);
    let d20 = v2.dot(v0);
    let d21 = v2.dot(v1);
    let denominator = d00 * d11 - d01 * d01;
    let v = (d11 * d20 - d01 * d21) / denominator;
    let w = (d00 * d21 - d01 * d20) / denominator;
    DVec3::new(1.0 - v - w, v, w)
}

/// The parameters `(s, t)` of the nearest points of the segments `p1 + s d1`
/// and `p2 + t d2`, each in `[0, 1]` — Ericson §5.1.9,
/// `ClosestPtSegmentSegment`.
pub(crate) fn segment_parameters(p1: DVec3, q1: DVec3, p2: DVec3, q2: DVec3) -> (f64, f64) {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.length_squared();
    let e = d2.length_squared();
    let f = d2.dot(r);
    if a == 0.0 && e == 0.0 {
        (0.0, 0.0)
    } else if a == 0.0 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e == 0.0 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denominator = a * e - b * b;
            let s = if denominator > 0.0 {
                ((b * f - c * e) / denominator).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let t = (b * s + f) / e;
            if t < 0.0 {
                ((-c / a).clamp(0.0, 1.0), 0.0)
            } else if t > 1.0 {
                (((b - c) / a).clamp(0.0, 1.0), 1.0)
            } else {
                (s, t)
            }
        }
    }
}

/// The nearest points of the segment `p0`–`p1` and a triangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SegmentClosest {
    /// The parameter along the segment, in `[0, 1]`.
    pub(crate) t: f64,
    /// The segment's nearest point.
    pub(crate) on_segment: DVec3,
    /// The triangle's.
    pub(crate) on_triangle: Closest,
}

impl SegmentClosest {
    /// The distance between the two.
    pub(crate) fn distance(&self) -> f64 {
        (self.on_segment - self.on_triangle.point).length()
    }
}

/// The nearest points of the segment `p0`–`p1` and triangle `tri`, whose unit
/// normal is `normal`.
///
/// A segment through the face meets it at distance zero, where it crosses.
/// Otherwise the nearest points are an end of the segment and its nearest
/// point of the triangle, or the segment against one of the triangle's
/// edges — Ericson §5.1.10's case split — and the first of the five
/// candidates at the least distance is taken, so the answer is the same on
/// every run.
pub(crate) fn segment_triangle(
    p0: DVec3,
    p1: DVec3,
    tri: &[DVec3; 3],
    normal: DVec3,
) -> SegmentClosest {
    let d0 = normal.dot(p0 - tri[0]);
    let d1 = normal.dot(p1 - tri[0]);
    if ((d0 <= 0.0 && d1 >= 0.0) || (d0 >= 0.0 && d1 <= 0.0)) && d0 != d1 {
        let t = d0 / (d0 - d1);
        let crossing = p0 + (p1 - p0) * t;
        let weights = barycentric(crossing, tri);
        if weights.min_element() >= 0.0 {
            return SegmentClosest {
                t,
                on_segment: crossing,
                on_triangle: Closest {
                    point: crossing,
                    barycentric: weights,
                    feature: Feature::Face,
                },
            };
        }
    }

    let end = |t: f64, p: DVec3| SegmentClosest {
        t,
        on_segment: p,
        on_triangle: closest_on_triangle(p, tri),
    };
    let mut best = end(0.0, p0);
    let mut best_distance = (best.on_segment - best.on_triangle.point).length_squared();
    let mut consider = |candidate: SegmentClosest| {
        let distance = (candidate.on_segment - candidate.on_triangle.point).length_squared();
        if distance < best_distance {
            best = candidate;
            best_distance = distance;
        }
    };
    consider(end(1.0, p1));
    for i in 0..3 {
        let (a, b) = (tri[i], tri[(i + 1) % 3]);
        let (s, u) = segment_parameters(p0, p1, a, b);
        let feature = if u <= 0.0 {
            Feature::Vertex(i)
        } else if u >= 1.0 {
            Feature::Vertex((i + 1) % 3)
        } else {
            Feature::Edge(i)
        };
        let mut weights = DVec3::ZERO;
        weights[i] = 1.0 - u;
        weights[(i + 1) % 3] = u;
        consider(SegmentClosest {
            t: s,
            on_segment: p0 + (p1 - p0) * s,
            on_triangle: Closest {
                point: a + (b - a) * u,
                barycentric: weights,
                feature,
            },
        });
    }
    best
}

/// Where the ray `origin + t · dir` crosses triangle `tri`, from either side:
/// `t`, unbounded, and the barycentric weights of the crossing. `None` for a
/// ray that misses, or runs parallel to the triangle's plane — a triangle has
/// no thickness to strike edge on.
///
/// Möller and Trumbore, "Fast, Minimum Storage Ray/Triangle Intersection",
/// _Journal of Graphics Tools_ 2(1), 1997. The parallel test is relative to
/// the three lengths the determinant is a product of, so it means the same
/// at every scale.
pub(crate) fn ray_triangle(origin: DVec3, dir: DVec3, tri: &[DVec3; 3]) -> Option<(f64, DVec3)> {
    /// The sine of the angle between the ray and the plane below which the
    /// two count as parallel.
    const PARALLEL: f64 = 1.0e-12;
    let [a, b, c] = *tri;
    let e1 = b - a;
    let e2 = c - a;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    let scale = e1.length() * e2.length() * dir.length();
    if det.abs() <= PARALLEL * scale || scale == 0.0 {
        return None;
    }
    let inverse = 1.0 / det;
    let s = origin - a;
    let u = s.dot(p) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some((e2.dot(q) * inverse, DVec3::new(1.0 - u - v, u, v)))
}

/// The roots of `a t² + 2 h t + c = 0`, smaller first, or `None` for none or
/// a degenerate `a`.
fn roots(a: f64, h: f64, c: f64) -> Option<(f64, f64)> {
    if a <= 0.0 {
        return None;
    }
    let discriminant = h * h - a * c;
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    Some(((-h - root) / a, (-h + root) / a))
}

/// When the point `origin + t · dir`, which starts outside it, first enters
/// the capsule of radius `r` about the segment `a`–`b`, if it does at some
/// `t ≥ 0`.
///
/// The capsule is a cylinder and two balls, and the point enters the capsule
/// where it first enters any of the three, since it starts outside all of
/// them: the cylinder counts only where it enters between the ends. A
/// segment of no length is a ball.
fn enter_capsule(origin: DVec3, dir: DVec3, a: DVec3, b: DVec3, r: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    let mut keep = |t: f64| {
        if t >= 0.0 && best.is_none_or(|b| t < b) {
            best = Some(t);
        }
    };
    let axis = b - a;
    let length_squared = axis.length_squared();
    if length_squared > 0.0 {
        // The components across the axis: |w + t v| = r.
        let o = origin - a;
        let w = o - axis * (o.dot(axis) / length_squared);
        let v = dir - axis * (dir.dot(axis) / length_squared);
        if let Some((t, _)) = roots(v.length_squared(), w.dot(v), w.length_squared() - r * r) {
            let along = (o + dir * t).dot(axis);
            if (0.0..=length_squared).contains(&along) {
                keep(t);
            }
        }
    }
    for centre in [a, b] {
        let o = origin - centre;
        if let Some((t, _)) = roots(dir.length_squared(), o.dot(dir), o.length_squared() - r * r) {
            keep(t);
        }
    }
    best
}

/// When the point `origin + t · dir`, which starts outside it, first crosses
/// one of the two faces of the convex polygon `poly` pushed out `r` along its
/// normal, landing over the polygon, if it does at some `t ≥ 0`. A polygon of
/// no area has no faces and answers `None`.
fn enter_faces(origin: DVec3, dir: DVec3, poly: &[DVec3], r: f64) -> Option<f64> {
    let normal = (poly[1] - poly[0]).cross(poly[2] - poly[0]);
    let length = normal.length();
    let approach = normal.dot(dir);
    if length == 0.0 || approach == 0.0 {
        return None;
    }
    let normal = normal / length;
    let approach = approach / length;
    let base = normal.dot(poly[0]);
    let mut best: Option<f64> = None;
    for side in [1.0, -1.0] {
        let t = (base + side * r - normal.dot(origin)) / approach;
        if t < 0.0 || best.is_some_and(|b| t >= b) {
            continue;
        }
        let on_plane = origin + dir * t - normal * (side * r);
        let inside = (0..poly.len()).all(|i| {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            (q - p).cross(on_plane - p).dot(normal) >= 0.0
        });
        if inside {
            best = Some(t);
        }
    }
    best
}

/// When a ball of radius `r` whose centre moves along `origin + t · dir`
/// first touches the shape swept by triangle `tri` along the segment from
/// `-half` to `+half` — a ball against a triangle when `half` is zero, and a
/// capsule of half-axis `half` against one otherwise — at some `t ∈ [0, 1]`.
/// The ball must start clear of it.
///
/// The swept triangle is a convex polyhedron — the triangle's two ends and a
/// parallelogram per edge — and the ball meets it where its centre enters the
/// polyhedron fattened by `r`, which is the union of each face fattened by
/// `r`: the face's two faces pushed out along its normal, and a capsule on
/// each of its edges. Its centre enters the union where it first enters any
/// part of it, since it starts outside them all. A polyhedron that is flat,
/// because the triangle lies along `half`, is still covered by its faces,
/// whose normals then all lie in one plane.
pub(crate) fn sweep_into_triangle(
    origin: DVec3,
    dir: DVec3,
    tri: &[DVec3; 3],
    half: DVec3,
    r: f64,
) -> Option<f64> {
    let mut best: Option<f64> = None;
    let mut keep = |t: Option<f64>| {
        if let Some(t) = t
            && t <= 1.0
            && best.is_none_or(|b| t < b)
        {
            best = Some(t);
        }
    };
    if half == DVec3::ZERO {
        keep(enter_faces(origin, dir, tri, r));
        for i in 0..3 {
            keep(enter_capsule(origin, dir, tri[i], tri[(i + 1) % 3], r));
        }
        return best;
    }
    let low = tri.map(|v| v - half);
    let high = tri.map(|v| v + half);
    keep(enter_faces(origin, dir, &low, r));
    keep(enter_faces(origin, dir, &high, r));
    for i in 0..3 {
        let j = (i + 1) % 3;
        keep(enter_faces(
            origin,
            dir,
            &[low[i], low[j], high[j], high[i]],
            r,
        ));
        keep(enter_capsule(origin, dir, low[i], low[j], r));
        keep(enter_capsule(origin, dir, high[i], high[j], r));
        keep(enter_capsule(origin, dir, low[i], high[i], r));
    }
    best
}

/// An oriented box: its centre, its three unit axes and its half-extents
/// along them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cuboid {
    pub(crate) centre: DVec3,
    pub(crate) axes: [DVec3; 3],
    pub(crate) half: DVec3,
}

impl Cuboid {
    /// How far the box reaches from its centre along the unit vector `n`.
    pub(crate) fn radius(&self, n: DVec3) -> f64 {
        self.half.x * self.axes[0].dot(n).abs()
            + self.half.y * self.axes[1].dot(n).abs()
            + self.half.z * self.axes[2].dot(n).abs()
    }
}

/// One of the thirteen axes of a box against a triangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SatAxis {
    /// The triangle's normal.
    Face,
    /// The box's axis `k`.
    BoxFace(usize),
    /// The box's axis `k` crossed with the triangle's edge `e`.
    Edge(usize, usize),
}

/// A box and a triangle's separation along one axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Separation {
    /// Positive apart, negative in overlap.
    pub(crate) separation: f64,
    /// The unit axis, pointing from the box towards the triangle.
    pub(crate) normal: DVec3,
    pub(crate) axis: SatAxis,
}

/// The sine of the angle below which a box's axis and a triangle's edge count
/// as parallel, and their cross product is not tested — the box pair's
/// threshold.
const EDGE_SINE: f64 = 1.0e-3;

/// The box and the triangle's separation along the unit vector `n`, and `n`
/// turned to point from the box towards the triangle: the gap between their
/// two intervals on it, on whichever side the triangle lies.
fn interval_separation(cuboid: &Cuboid, tri: &[DVec3; 3], n: DVec3) -> (f64, DVec3) {
    let centre = n.dot(cuboid.centre);
    let reach = cuboid.radius(n);
    let p = tri.map(|v| n.dot(v));
    let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
    let ahead = lo - (centre + reach);
    let behind = (centre - reach) - hi;
    if ahead >= behind {
        (ahead, n)
    } else {
        (behind, -n)
    }
}

/// The box and the triangle of unit normal `normal` separated along each of
/// the thirteen axes of the separating axis theorem — Akenine-Möller, "Fast
/// 3D Triangle-Box Overlap Testing", _Journal of Graphics Tools_ 6(1), 2001:
/// the triangle's normal, the box's three axes and the nine crosses of a box
/// axis with a triangle edge, in that order. A cross within [`EDGE_SINE`] of
/// parallel is `None`.
///
/// The two overlap exactly when no axis separates them, and apart, the
/// greatest separation is a lower bound on the distance between them. For a
/// box whose centre is in front of the triangle, the normal's separation is
/// the box's lowest point's height over the plane.
pub(crate) fn box_triangle_axes(
    cuboid: &Cuboid,
    tri: &[DVec3; 3],
    normal: DVec3,
) -> [Option<Separation>; 13] {
    let mut out = [None; 13];
    let (separation, n) = interval_separation(cuboid, tri, normal);
    out[0] = Some(Separation {
        separation,
        normal: n,
        axis: SatAxis::Face,
    });
    for k in 0..3 {
        let (separation, n) = interval_separation(cuboid, tri, cuboid.axes[k]);
        out[1 + k] = Some(Separation {
            separation,
            normal: n,
            axis: SatAxis::BoxFace(k),
        });
    }
    for k in 0..3 {
        for e in 0..3 {
            let edge = tri[(e + 1) % 3] - tri[e];
            let cross = cuboid.axes[k].cross(edge);
            let length = cross.length();
            if length < EDGE_SINE * edge.length() || length == 0.0 {
                continue;
            }
            let (separation, n) = interval_separation(cuboid, tri, cross / length);
            out[4 + 3 * k + e] = Some(Separation {
                separation,
                normal: n,
                axis: SatAxis::Edge(k, e),
            });
        }
    }
    out
}

/// The greatest of [`box_triangle_axes`]' separations: a lower bound on the
/// distance between the box and the triangle when positive, and when not,
/// the two overlap.
pub(crate) fn box_triangle_gap(cuboid: &Cuboid, tri: &[DVec3; 3], normal: DVec3) -> Separation {
    box_triangle_axes(cuboid, tri, normal)
        .into_iter()
        .flatten()
        .fold(None::<Separation>, |best, next| match best {
            Some(best) if best.separation >= next.separation => Some(best),
            _ => Some(next),
        })
        .expect("the face axis always exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A triangle in the floor, normal up: `(0,0,0)`, `(0,0,2)`, `(2,0,0)`.
    const FLOOR: [DVec3; 3] = [
        DVec3::ZERO,
        DVec3::new(0.0, 0.0, 2.0),
        DVec3::new(2.0, 0.0, 0.0),
    ];

    fn close(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-12
    }

    /// Each Voronoi region answers its own feature, and the weights make the
    /// point.
    #[test]
    fn the_nearest_point_names_the_region_it_is_in() {
        let above = closest_on_triangle(DVec3::new(0.5, 3.0, 0.5), &FLOOR);
        assert_eq!(above.feature, Feature::Face);
        assert!(close(above.point, DVec3::new(0.5, 0.0, 0.5)), "{above:?}");
        assert!(close(above.barycentric, DVec3::new(0.5, 0.25, 0.25)));

        let beside = closest_on_triangle(DVec3::new(-1.0, 1.0, 0.5), &FLOOR);
        assert_eq!(beside.feature, Feature::Edge(0));
        assert!(close(beside.point, DVec3::new(0.0, 0.0, 0.5)), "{beside:?}");

        let past_hypotenuse = closest_on_triangle(DVec3::new(2.0, 0.0, 2.0), &FLOOR);
        assert_eq!(past_hypotenuse.feature, Feature::Edge(1));
        assert!(close(past_hypotenuse.point, DVec3::new(1.0, 0.0, 1.0)));

        let corner = closest_on_triangle(DVec3::new(3.0, 0.0, -1.0), &FLOOR);
        assert_eq!(corner.feature, Feature::Vertex(2));
        for sample in [above, beside, past_hypotenuse, corner] {
            let rebuilt = FLOOR[0] * sample.barycentric.x
                + FLOOR[1] * sample.barycentric.y
                + FLOOR[2] * sample.barycentric.z;
            assert!(close(rebuilt, sample.point), "{sample:?}");
        }
    }

    /// A vertex is active if either edge meeting there is.
    #[test]
    fn a_vertex_is_active_with_either_of_its_edges() {
        assert!(Feature::Vertex(0).is_active(0b100));
        assert!(Feature::Vertex(0).is_active(0b001));
        assert!(!Feature::Vertex(0).is_active(0b010));
        assert!(Feature::Vertex(2).is_active(0b010));
        assert!(!Feature::Edge(1).is_active(0b101));
        assert!(Feature::Face.is_active(0));
    }

    /// A segment through the face meets it where it crosses; one beside it
    /// meets an edge.
    #[test]
    fn a_segment_meets_a_triangle_where_it_crosses_or_nearest() {
        let through = segment_triangle(
            DVec3::new(0.5, 1.0, 0.5),
            DVec3::new(0.5, -1.0, 0.5),
            &FLOOR,
            DVec3::Y,
        );
        assert_eq!(through.distance(), 0.0);
        assert!((through.t - 0.5).abs() < 1e-15);

        let beside_edge = segment_triangle(
            DVec3::new(-1.0, 0.5, 1.0),
            DVec3::new(-0.5, 0.5, 1.0),
            &FLOOR,
            DVec3::Y,
        );
        assert_eq!(beside_edge.on_triangle.feature, Feature::Edge(0));
        assert_eq!(beside_edge.t, 1.0);
        assert!((beside_edge.distance() - 0.5f64.sqrt()).abs() < 1e-12);
    }

    /// A ray hits the face from either side at the crossing, and misses past
    /// the hypotenuse and when it runs along the plane.
    #[test]
    fn a_ray_crosses_a_triangle_from_either_side() {
        let (t, w) = ray_triangle(DVec3::new(0.5, 2.0, 0.5), -DVec3::Y, &FLOOR).unwrap();
        assert!((t - 2.0).abs() < 1e-15 && close(w, DVec3::new(0.5, 0.25, 0.25)));
        let (t, _) = ray_triangle(DVec3::new(0.5, -3.0, 0.5), DVec3::Y * 2.0, &FLOOR).unwrap();
        assert!((t - 1.5).abs() < 1e-15);
        assert!(ray_triangle(DVec3::new(1.5, 2.0, 1.5), -DVec3::Y, &FLOOR).is_none());
        assert!(ray_triangle(DVec3::new(-1.0, 0.0, 0.5), DVec3::X, &FLOOR).is_none());
    }

    /// A ball dropped on the face lands a radius above it; one dropped beside
    /// the edge meets the edge's rounding; a capsule dropped upright lands on
    /// its lower end.
    #[test]
    fn a_swept_ball_or_capsule_meets_the_fattened_triangle() {
        let t = sweep_into_triangle(
            DVec3::new(0.5, 2.0, 0.5),
            DVec3::new(0.0, -2.0, 0.0),
            &FLOOR,
            DVec3::ZERO,
            0.5,
        )
        .unwrap();
        assert!((t - 0.75).abs() < 1e-12, "{t}");

        // 0.3 beside edge 0 (x = 0), radius 0.5: touches at height 0.4.
        let t = sweep_into_triangle(
            DVec3::new(-0.3, 2.0, 1.0),
            DVec3::new(0.0, -2.0, 0.0),
            &FLOOR,
            DVec3::ZERO,
            0.5,
        )
        .unwrap();
        assert!((t - 0.8).abs() < 1e-12, "{t}");

        // Upright, half-axis 1, radius 0.25: its lower end lands when the
        // centre is 1.25 up.
        let t = sweep_into_triangle(
            DVec3::new(0.5, 3.0, 0.5),
            DVec3::new(0.0, -2.0, 0.0),
            &FLOOR,
            DVec3::Y,
            0.25,
        )
        .unwrap();
        assert!((t - 0.875).abs() < 1e-12, "{t}");

        assert!(
            sweep_into_triangle(
                DVec3::new(3.0, 2.0, 3.0),
                DVec3::new(0.0, -2.0, 0.0),
                &FLOOR,
                DVec3::Y,
                0.25
            )
            .is_none()
        );
    }
}
