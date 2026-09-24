//! Box against box: contact-solver rung 2 (`docs/notes/simulation.md`),
//! decision 2's separating axis test with clipping, reduced to four points,
//! with flip-invariant feature ids.
//!
//! ```text
//!   cached axis ── still separates? ──▶ no contact
//!        │ no
//!   15 axes: 3 faces of A, 3 of B, 9 edge pairs ── any separates? ──▶ none
//!        │ none                                    (and cache it)
//!   pick: an edge pair only when clearly better than both faces,
//!         B's face only when clearly better than A's
//!        │
//!   face: the incident face of the other box, clipped to the reference
//!         face's four sides (Sutherland–Hodgman), kept within the
//!         speculative distance, reduced to four
//!   edge: the nearest points of the two edges, one point
//! ```
//!
//! # Sources
//!
//! - **The axes and their separations** are the fifteen of Ericson,
//!   _Real-Time Collision Detection_ §4.4.1, "OBB–OBB intersection", each
//!   projected with both boxes' full support radii, so every axis is a true
//!   lower bound on the gap and no Minkowski-face pruning is needed. An edge
//!   pair's axis is normalised and skipped when the edges are within
//!   [`EDGE_SINE`] of parallel, where Ericson instead adds an epsilon to the
//!   rotation's absolute values; a near-parallel pair's contact is a face's.
//! - **The choice between a face and an edge contact** is Gregorius, "The
//!   Separating Axis Test between Convex Polyhedra", GDC 2013: an edge pair
//!   wins only by a relative margin plus half a linear slop, and B's face beats
//!   A's only by a smaller one, so a resting pair does not flip between
//!   features from one tick to the next. His margins are written for negative
//!   separations (`0.90 · s` and `0.98 · s`); here they are taken against the
//!   separation's magnitude, `s + 0.10 · |s|`, which is the same for an
//!   overlap and still favours a face across a speculative gap.
//! - **The cached axis** is Gregorius's "SAT with temporal coherence" from the
//!   same talk: the last test's best axis is tried first, and if it still
//!   separates the pair by more than the speculative distance there is no
//!   contact. That answer is exactly the full test's — any axis's separation
//!   bounds the gap from below — so the cache changes cost and never results.
//! - **The clipping** is Sutherland–Hodgman, Ericson §8.3.4, against the
//!   reference face's four side planes, each pushed out by [`CLIP_TOLERANCE`].
//! - **The reduction** to four points is Gregorius, "Robust Contact Creation
//!   for Physics Simulations", GDC 2015, the scheme Box2D v3 and Box3D reduce
//!   with: the deepest point, the point furthest from it, the point making the
//!   largest triangle with those two, and the point adding the most area
//!   outside that triangle.
//!
//! # Feature ids
//!
//! A point is named by the pair of features it is made of — one feature of
//! box `A`, one of box `B` — whichever box's face was the reference:
//!
//! | point                                   | `A`'s feature | `B`'s feature |
//! | --------------------------------------- | ------------- | ------------- |
//! | an incident corner inside the reference | face / corner | corner / face |
//! | an incident edge through a side plane   | face / edge   | edge / face   |
//! | a reference corner over the incident    | corner / face | face / corner |
//! | edge against edge                       | edge          | edge          |
//!
//! Each feature is a code — a corner `1 + c`, an edge `9 + e`, a face `21 + f`
//! — and the id is `A`'s code in the low byte and `B`'s in the next. Naming
//! both sides the same way whichever is the reference is what makes the ids
//! flip-invariant, as decision 2 asks.
//!
//! **Why the side planes are pushed out.** Two equal boxes stacked square have
//! their incident corners exactly on the reference face's sides. Without a
//! tolerance, rounding decides each tick whether a corner is inside — kept as a
//! corner — or just outside — replaced by where its edges cross the side — and
//! the ids flicker between the two, which throws the warm start away. Half a
//! millimetre outward keeps the corners.

use glam::{DMat3, DQuat, DVec3};

use super::{LINEAR_SLOP, MAX_POINTS, Manifold, closest_between_segments};

/// Gregorius's absolute tolerance: half a linear slop.
pub(super) const ABSOLUTE_TOLERANCE: f64 = 0.5 * LINEAR_SLOP;

/// The share of its separation's magnitude by which an edge pair must beat
/// both faces: Gregorius's `1 − 0.90`.
pub(super) const EDGE_RELATIVE_TOLERANCE: f64 = 0.10;

/// The share by which box `B`'s face must beat box `A`'s: Gregorius's
/// `1 − 0.98`.
pub(super) const FACE_RELATIVE_TOLERANCE: f64 = 0.02;

/// How far the reference face's side planes are pushed out before clipping, in
/// metres; see the module docs.
pub(super) const CLIP_TOLERANCE: f64 = 0.1 * LINEAR_SLOP;

/// The sine of the angle below which two edges count as parallel and their
/// cross product is not tested as an axis.
const EDGE_SINE: f64 = 1.0e-3;

/// The most points clipping a quadrilateral against four planes can leave: one
/// more for each plane.
pub(super) const MAX_CLIPPED: usize = 8;

/// One of the fifteen axes of a box pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    /// The normal of box `A`'s faces on this axis.
    FaceA(usize),
    /// The normal of box `B`'s faces on this axis.
    FaceB(usize),
    /// The cross product of `A`'s edge direction and `B`'s.
    Edge(usize, usize),
}

/// The separating axis a box pair's last full test found, kept on the contact
/// from one tick to the next.
///
/// Holding it never changes a manifold: it only lets a pair that is still
/// apart along it skip the other fourteen axes. See the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SatCache {
    axis: Option<Axis>,
}

impl SatCache {
    /// Whether an axis is cached.
    #[must_use]
    pub const fn is_set(&self) -> bool {
        self.axis.is_some()
    }
}

/// A box, with its axes as world vectors.
#[derive(Clone, Copy, Debug)]
struct Obb {
    centre: DVec3,
    axes: [DVec3; 3],
    half: [f64; 3],
}

impl Obb {
    fn new(centre: DVec3, rotation: DQuat, half: DVec3) -> Self {
        let m = DMat3::from_quat(rotation);
        Self {
            centre,
            axes: [m.x_axis, m.y_axis, m.z_axis],
            half: half.to_array(),
        }
    }

    /// How far the box reaches from its centre along the unit vector `n`.
    fn radius(&self, n: DVec3) -> f64 {
        self.half[0] * self.axes[0].dot(n).abs()
            + self.half[1] * self.axes[1].dot(n).abs()
            + self.half[2] * self.axes[2].dot(n).abs()
    }

    /// Corner `c`: bit `k` set is the positive end of axis `k`.
    fn corner(&self, c: usize) -> DVec3 {
        let mut p = self.centre;
        for k in 0..3 {
            p += self.axes[k] * (sign_of_bit(c, k) * self.half[k]);
        }
        p
    }
}

pub(super) fn sign_of_bit(c: usize, k: usize) -> f64 {
    if c & (1 << k) == 0 { -1.0 } else { 1.0 }
}

/// The two axes other than `k`, in ascending order.
pub(super) const fn others(k: usize) -> (usize, usize) {
    match k {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// The edge along `axis` whose other two coordinates are those of corner `c`.
pub(super) fn edge_index(axis: usize, c: usize) -> usize {
    let (lo, hi) = others(axis);
    axis * 4 + ((c >> lo) & 1) + 2 * ((c >> hi) & 1)
}

/// The face on `axis` at its positive end if `positive`.
pub(super) fn face_index(axis: usize, positive: bool) -> usize {
    axis * 2 + usize::from(positive)
}

pub(super) const fn corner_code(c: usize) -> u32 {
    1 + c as u32
}

pub(super) const fn edge_code(e: usize) -> u32 {
    9 + e as u32
}

pub(super) const fn face_code(f: usize) -> u32 {
    21 + f as u32
}

/// A feature id from box `A`'s feature code and box `B`'s.
pub(super) const fn pack(a: u32, b: u32) -> u32 {
    a | (b << 8)
}

/// The code of a box's face on `axis`, at its positive end if `positive`.
pub(super) fn face_feature(axis: usize, positive: bool) -> u32 {
    face_code(face_index(axis, positive))
}

/// The code of the feature of a box of half-extents `half` that the point
/// `local`, in the box's frame, is nearest: a corner where it lies outside on
/// all three axes, an edge on two, a face on one, and from inside the face it
/// is least deep under.
///
/// A sphere's and a capsule's manifolds against a box name their points by it,
/// so a box turning a different corner onto them does not hand the new corner
/// the old one's impulse.
pub(super) fn feature_near(local: DVec3, half: DVec3) -> u32 {
    let outside: [bool; 3] = core::array::from_fn(|k| local[k].abs() > half[k]);
    let positive = |k: usize| local[k] > 0.0;
    let bits = |axes: &[usize]| {
        axes.iter()
            .fold(0, |c, &k| c | (usize::from(positive(k)) << k))
    };
    match outside.iter().filter(|&&o| o).count() {
        3 => corner_code(bits(&[0, 1, 2])),
        2 => {
            let axis = (0..3).find(|&k| !outside[k]).expect("one axis inside");
            let (lo, hi) = others(axis);
            edge_code(edge_index(axis, bits(&[lo, hi])))
        }
        1 => {
            let k = (0..3).find(|&k| outside[k]).expect("one axis outside");
            face_code(face_index(k, positive(k)))
        }
        _ => {
            let depth = half - local.abs();
            let k = if depth.x <= depth.y && depth.x <= depth.z {
                0
            } else if depth.y <= depth.z {
                1
            } else {
                2
            };
            face_code(face_index(k, positive(k)))
        }
    }
}

/// The separation of the boxes along the unit vector `n`, and `n` turned to
/// point from `A` towards `B`.
fn separation(a: &Obb, b: &Obb, n: DVec3) -> (f64, DVec3) {
    let d = (b.centre - a.centre).dot(n);
    let oriented = if d < 0.0 { -n } else { n };
    (d.abs() - a.radius(n) - b.radius(n), oriented)
}

/// The separation along `axis`, or `None` for an edge pair too near parallel
/// to have an axis.
fn axis_separation(a: &Obb, b: &Obb, axis: Axis) -> Option<(f64, DVec3)> {
    match axis {
        Axis::FaceA(i) => Some(separation(a, b, a.axes[i])),
        Axis::FaceB(j) => Some(separation(a, b, b.axes[j])),
        Axis::Edge(i, j) => {
            let cross = a.axes[i].cross(b.axes[j]);
            let length = cross.length();
            (length >= EDGE_SINE).then(|| separation(a, b, cross / length))
        }
    }
}

/// The best axis of one kind: its separation, oriented normal and name.
#[derive(Clone, Copy, Debug)]
struct Best {
    separation: f64,
    normal: DVec3,
    axis: Axis,
}

/// The axis of `candidates` with the greatest separation, the first on a tie.
fn best_of(a: &Obb, b: &Obb, candidates: impl Iterator<Item = Axis>) -> Option<Best> {
    let mut best: Option<Best> = None;
    for axis in candidates {
        if let Some((separation, normal)) = axis_separation(a, b, axis)
            && best.is_none_or(|best| separation > best.separation)
        {
            best = Some(Best {
                separation,
                normal,
                axis,
            });
        }
    }
    best
}

/// A vertex of the polygon being clipped.
#[derive(Clone, Copy, Debug)]
struct ClipVertex {
    point: DVec3,
    /// The incident box's feature it lies on.
    incident: u32,
    /// The reference box's.
    reference: u32,
    /// What the polygon's edge from here to the next vertex lies along.
    out: Along,
}

/// What a clipped polygon's edge lies along.
#[derive(Clone, Copy, Debug)]
enum Along {
    /// An edge of the incident box, by edge index.
    Incident(usize),
    /// A side face of the reference box, by face index.
    Reference(usize),
}

/// A polygon of at most [`MAX_CLIPPED`] vertices.
#[derive(Clone, Copy, Debug)]
struct Polygon {
    vertices: [ClipVertex; MAX_CLIPPED],
    count: usize,
}

impl Polygon {
    fn push(&mut self, vertex: ClipVertex) {
        debug_assert!(self.count < MAX_CLIPPED, "one more vertex per plane");
        self.vertices[self.count] = vertex;
        self.count += 1;
    }
}

/// The two faces a face contact is between, by face index.
#[derive(Clone, Copy, Debug)]
struct Faces {
    reference: usize,
    incident: usize,
}

/// The part of `input` on the inner side of the reference face `face`'s plane
/// `normal · x ≤ offset` — one step of Sutherland–Hodgman, Ericson §8.3.4,
/// naming each new vertex by the features it is made of.
fn clip(input: &Polygon, normal: DVec3, offset: f64, face: usize, faces: Faces) -> Polygon {
    let mut output = Polygon { count: 0, ..*input };
    for k in 0..input.count {
        let v0 = input.vertices[k];
        let v1 = input.vertices[(k + 1) % input.count];
        let d0 = normal.dot(v0.point) - offset;
        let d1 = normal.dot(v1.point) - offset;
        if d0 <= 0.0 {
            let mut kept = v0;
            if d0 == 0.0 && d1 > 0.0 {
                // On the plane and leaving: what follows runs along it.
                kept.out = Along::Reference(face);
            }
            output.push(kept);
        }
        if (d0 < 0.0 && d1 > 0.0) || (d0 > 0.0 && d1 < 0.0) {
            let t = d0 / (d0 - d1);
            let (incident, reference) = match v0.out {
                Along::Incident(edge) => (edge_code(edge), face_code(face)),
                Along::Reference(side) => (
                    face_code(faces.incident),
                    reference_corner_code(side, face, faces.reference),
                ),
            };
            output.push(ClipVertex {
                point: v0.point + (v1.point - v0.point) * t,
                incident,
                reference,
                // Leaving, the polygon follows this plane to where it comes
                // back in; entering, it carries on along the same edge.
                out: if d0 < 0.0 {
                    Along::Reference(face)
                } else {
                    v0.out
                },
            });
        }
    }
    output
}

/// The code of the reference box's corner where side faces `f1` and `f2` meet
/// its reference face `face`: where two side planes cross over the incident
/// face, the point stands under that corner.
///
/// Naming it by the corner rather than by the edge the two sides share is what
/// keeps it the same point whichever box is the reference: a small box's
/// corner on a big box's face is its corner against that face either way.
pub(super) fn reference_corner_code(f1: usize, f2: usize, face: usize) -> u32 {
    let (a1, a2) = (f1 / 2, f2 / 2);
    if a1 == a2 {
        // Parallel sides never meet; a segment on one cannot cross the other
        // unless the box is flat, and then its face names the point.
        return face_code(f2);
    }
    let corner = ((f1 & 1) << a1) | ((f2 & 1) << a2) | ((face & 1) << (face / 2));
    corner_code(corner)
}

/// How far apart two boxes are, as the greatest separation over all fifteen
/// axes, and that axis's normal from `A` towards `B`.
///
/// Apart, that is a lower bound on the distance between them, since every
/// axis's separation is; in overlap it is minus the depth along the axis of
/// least overlap, which for two boxes is the depth itself short of the
/// near-parallel edge pairs [`EDGE_SINE`] skips. The sweeps step by it.
pub(super) fn gap(
    ca: DVec3,
    ra: DQuat,
    ha: DVec3,
    cb: DVec3,
    rb: DQuat,
    hb: DVec3,
) -> (f64, DVec3) {
    let a = Obb::new(ca, ra, ha);
    let b = Obb::new(cb, rb, hb);
    let axes = (0..3)
        .map(Axis::FaceA)
        .chain((0..3).map(Axis::FaceB))
        .chain((0..9).map(|k| Axis::Edge(k / 3, k % 3)));
    let best = best_of(&a, &b, axes).expect("a face axis always exists");
    (best.separation, best.normal)
}

/// A point of the manifold before reduction.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Candidate {
    pub(super) point: DVec3,
    pub(super) separation: f64,
    pub(super) id: u32,
}

/// The manifold between two boxes, trying and refreshing `cache`.
#[allow(clippy::too_many_arguments)]
pub(super) fn boxes(
    ca: DVec3,
    ra: DQuat,
    ha: DVec3,
    cb: DVec3,
    rb: DQuat,
    hb: DVec3,
    speculative: f64,
    cache: &mut SatCache,
) -> Manifold {
    let a = Obb::new(ca, ra, ha);
    let b = Obb::new(cb, rb, hb);

    if let Some(axis) = cache.axis
        && let Some((separation, _)) = axis_separation(&a, &b, axis)
        && separation > speculative
    {
        return Manifold::EMPTY;
    }

    let face_a = best_of(&a, &b, (0..3).map(Axis::FaceA)).expect("a face axis always exists");
    if face_a.separation > speculative {
        cache.axis = Some(face_a.axis);
        return Manifold::EMPTY;
    }
    let face_b = best_of(&a, &b, (0..3).map(Axis::FaceB)).expect("a face axis always exists");
    if face_b.separation > speculative {
        cache.axis = Some(face_b.axis);
        return Manifold::EMPTY;
    }
    let edge = best_of(&a, &b, (0..9).map(|k| Axis::Edge(k / 3, k % 3)));
    if let Some(edge) = edge
        && edge.separation > speculative
    {
        cache.axis = Some(edge.axis);
        return Manifold::EMPTY;
    }

    let face_best = face_a.separation.max(face_b.separation);
    let edge_wins = edge.is_some_and(|edge| {
        edge.separation > face_best + EDGE_RELATIVE_TOLERANCE * face_best.abs() + ABSOLUTE_TOLERANCE
    });
    let b_wins = face_b.separation
        > face_a.separation
            + FACE_RELATIVE_TOLERANCE * face_a.separation.abs()
            + ABSOLUTE_TOLERANCE;
    let chosen = match edge {
        Some(edge) if edge_wins => edge,
        _ if b_wins => face_b,
        _ => face_a,
    };
    cache.axis = Some(
        [Some(face_a), Some(face_b), edge]
            .into_iter()
            .flatten()
            .fold(face_a, |best, next| {
                if next.separation > best.separation {
                    next
                } else {
                    best
                }
            })
            .axis,
    );

    match chosen.axis {
        Axis::Edge(i, j) => edge_contact(&a, &b, i, j, chosen.normal, speculative),
        Axis::FaceA(i) => face_contact(&a, &b, i, chosen.normal, true, speculative),
        Axis::FaceB(j) => face_contact(&b, &a, j, -chosen.normal, false, speculative),
    }
}

/// One point where edge `i` of `A` crosses edge `j` of `B`, `normal` pointing
/// from `A` to `B`.
fn edge_contact(a: &Obb, b: &Obb, i: usize, j: usize, normal: DVec3, speculative: f64) -> Manifold {
    // Each box's edge along its axis that reaches furthest towards the other.
    let support = |obb: &Obb, axis: usize, towards: DVec3| {
        let mut centre = obb.centre;
        let mut corner = 0;
        for k in (0..3).filter(|&k| k != axis) {
            let positive = obb.axes[k].dot(towards) >= 0.0;
            corner |= usize::from(positive) << k;
            centre += obb.axes[k] * (if positive { 1.0 } else { -1.0 } * obb.half[k]);
        }
        let reach = obb.axes[axis] * obb.half[axis];
        (centre - reach, centre + reach, edge_index(axis, corner))
    };
    let (a0, a1, edge_a) = support(a, i, normal);
    let (b0, b1, edge_b) = support(b, j, -normal);
    let (on_a, on_b) = closest_between_segments(a0, a1, b0, b1);
    let separation = (on_b - on_a).dot(normal);
    let mut manifold = Manifold::with_normal(normal);
    if separation <= speculative {
        manifold.push(
            (on_a + on_b) * 0.5,
            separation,
            pack(edge_code(edge_a), edge_code(edge_b)),
        );
    }
    manifold
}

/// The manifold of `incident` against face `axis` of `reference`, whose outward
/// normal there is `outward`; `reference_is_a` says which of the pair's boxes
/// the reference is, which decides the manifold's normal and the ids' order.
fn face_contact(
    reference: &Obb,
    incident: &Obb,
    axis: usize,
    outward: DVec3,
    reference_is_a: bool,
    speculative: f64,
) -> Manifold {
    let reference_face = face_index(axis, reference.axes[axis].dot(outward) > 0.0);

    // The incident face: the one whose normal is most against the reference's.
    let mut k = 0;
    let mut most = f64::NEG_INFINITY;
    for candidate in 0..3 {
        let along = incident.axes[candidate].dot(outward).abs();
        if along > most {
            most = along;
            k = candidate;
        }
    }
    let incident_positive = incident.axes[k].dot(outward) < 0.0;
    let incident_face = face_index(k, incident_positive);
    let (u, v) = others(k);
    let base = usize::from(incident_positive) << k;
    let corners = [
        base,
        base | (1 << u),
        base | (1 << u) | (1 << v),
        base | (1 << v),
    ];
    let mut polygon = Polygon {
        vertices: [ClipVertex {
            point: DVec3::ZERO,
            incident: 0,
            reference: 0,
            out: Along::Incident(0),
        }; MAX_CLIPPED],
        count: 0,
    };
    for n in 0..4 {
        let (c0, c1) = (corners[n], corners[(n + 1) % 4]);
        polygon.push(ClipVertex {
            point: incident.corner(c0),
            incident: corner_code(c0),
            reference: face_code(reference_face),
            out: Along::Incident(edge_index((c0 ^ c1).trailing_zeros() as usize, c0)),
        });
    }

    let (su, sv) = others(axis);
    for side in [su, sv] {
        for positive in [true, false] {
            let normal = if positive {
                reference.axes[side]
            } else {
                -reference.axes[side]
            };
            let offset = normal.dot(reference.centre) + reference.half[side] + CLIP_TOLERANCE;
            polygon = clip(
                &polygon,
                normal,
                offset,
                face_index(side, positive),
                Faces {
                    reference: reference_face,
                    incident: incident_face,
                },
            );
        }
    }

    let face_offset = outward.dot(reference.centre) + reference.half[axis];
    let mut candidates = [Candidate::default(); MAX_CLIPPED];
    let mut count = 0;
    for vertex in &polygon.vertices[..polygon.count] {
        let separation = outward.dot(vertex.point) - face_offset;
        if separation > speculative {
            continue;
        }
        let id = if reference_is_a {
            pack(vertex.reference, vertex.incident)
        } else {
            pack(vertex.incident, vertex.reference)
        };
        candidates[count] = Candidate {
            point: vertex.point - outward * (0.5 * separation),
            separation,
            id,
        };
        count += 1;
    }

    let normal = if reference_is_a { outward } else { -outward };
    let mut manifold = Manifold::with_normal(normal);
    let (kept, kept_count) = reduce(&candidates[..count], normal);
    for &index in &kept[..kept_count] {
        let c = candidates[index];
        manifold.push(c.point, c.separation, c.id);
    }
    manifold
}

/// The indices of at most four of `points` that keep the manifold's area —
/// Gregorius, GDC 2015: the deepest, the furthest from it, the largest
/// triangle with those two, and the most area added outside that triangle.
/// Ties go to the earlier point, so the choice is the same on every run.
pub(super) fn reduce(points: &[Candidate], normal: DVec3) -> ([usize; MAX_POINTS], usize) {
    let mut kept = [0; MAX_POINTS];
    if points.len() <= MAX_POINTS {
        for (k, slot) in kept.iter_mut().enumerate().take(points.len()) {
            *slot = k;
        }
        return (kept, points.len());
    }
    let area = |a: usize, b: usize, p: usize| {
        (points[b].point - points[a].point)
            .cross(points[p].point - points[a].point)
            .dot(normal)
    };
    let argmax = |score: &dyn Fn(usize) -> f64| {
        let mut best = (f64::NEG_INFINITY, 0);
        for p in 0..points.len() {
            let s = score(p);
            if s > best.0 {
                best = (s, p);
            }
        }
        best
    };

    let (_, first) = argmax(&|p| -points[p].separation);
    let (_, second) = argmax(&|p| (points[p].point - points[first].point).length_squared());
    let (_, third) = argmax(&|p| area(first, second, p).abs());
    // Wound so the triangle's area is positive: a point outside an edge then
    // makes a negative area with it.
    let (a, b, c) = if area(first, second, third) >= 0.0 {
        (first, second, third)
    } else {
        (first, third, second)
    };
    let (outside, fourth) = argmax(&|p| (-area(a, b, p)).max(-area(b, c, p)).max(-area(c, a, p)));
    kept[..3].copy_from_slice(&[first, second, third]);
    if outside > 0.0 {
        kept[3] = fourth;
        (kept, 4)
    } else {
        (kept, 3)
    }
}

#[cfg(test)]
mod tests {
    use super::super::collide;
    use super::*;
    use crate::contact::shape::ContactShape;

    const SPEC: f64 = 0.02;

    fn cube(centre: DVec3, rotation: DQuat, half: f64) -> ContactShape {
        ContactShape::Box {
            centre,
            rotation,
            half: DVec3::splat(half),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// The shortest-arc rotation taking unit vector `from` onto unit vector
    /// `to`, built without a sine.
    fn arc(from: DVec3, to: DVec3) -> DQuat {
        let axis = from.cross(to);
        DQuat::from_xyzw(axis.x, axis.y, axis.z, 1.0 + from.dot(to)).normalize()
    }

    /// The points' X and Z, sorted, against `expected` to within rounding.
    fn assert_xz(manifold: &Manifold, expected: &[(f64, f64)]) {
        let got = sorted_xz(manifold);
        assert_eq!(got.len(), expected.len(), "{manifold:?}");
        for (g, w) in got.iter().zip(expected) {
            assert!(
                close(g.0, w.0) && close(g.1, w.1),
                "{got:?} against {expected:?}"
            );
        }
    }

    fn sorted_xz(manifold: &Manifold) -> Vec<(f64, f64)> {
        let mut out: Vec<(f64, f64)> = manifold
            .points()
            .iter()
            .map(|p| (p.point.x, p.point.z))
            .collect();
        out.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.total_cmp(&q.1)));
        out
    }

    /// **Face on face.** A unit cube a centimetre into another straight below
    /// it touches at its four lower corners, each a centimetre deep, midway
    /// between the two faces — 4.95 dm up — with the normal straight up and
    /// four different ids.
    #[test]
    fn a_cube_on_a_cube_touches_at_four_corners() {
        let lower = cube(DVec3::ZERO, DQuat::IDENTITY, 0.5);
        let upper = cube(DVec3::new(0.0, 0.99, 0.0), DQuat::IDENTITY, 0.5);
        let m = collide(&lower, &upper, SPEC);
        assert_eq!(m.points().len(), 4, "{m:?}");
        assert_eq!(m.normal, DVec3::Y);
        for p in m.points() {
            assert!(close(p.separation, -0.01), "{m:?}");
            assert!(close(p.point.y, 0.495), "{m:?}");
        }
        assert_xz(&m, &[(-0.5, -0.5), (-0.5, 0.5), (0.5, -0.5), (0.5, 0.5)]);
        let mut ids: Vec<u32> = m.points().iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 4, "{m:?}");

        // And the other way up the normal still runs from A into B.
        let m = collide(&upper, &lower, SPEC);
        assert_eq!(m.normal, -DVec3::Y, "{m:?}");
        assert_eq!(m.points().len(), 4, "{m:?}");
    }

    /// **A small box on a big one** rests on its own four corners, wherever the
    /// big box's face is the reference, and the ids are the small box's
    /// corners against the big box's top face.
    #[test]
    fn a_small_box_on_a_big_one_rests_on_its_own_corners() {
        let big = cube(DVec3::ZERO, DQuat::IDENTITY, 0.5);
        let small = cube(DVec3::new(0.1, 0.74, -0.2), DQuat::IDENTITY, 0.25);
        let m = collide(&big, &small, SPEC);
        assert_xz(
            &m,
            &[(-0.15, -0.45), (-0.15, 0.05), (0.35, -0.45), (0.35, 0.05)],
        );
        let top = face_code(face_index(1, true));
        for p in m.points() {
            assert!(close(p.separation, -0.01), "{m:?}");
            assert_eq!(p.id & 0xff, top, "A's feature is its top face: {m:?}");
            let corner = (p.id >> 8) - 1;
            assert!(
                corner < 8 && corner & 0b010 == 0,
                "B's lower corners: {m:?}"
            );
        }
    }

    /// **Half off the edge.** A cube shifted half its width along X over
    /// another touches over the half that overlaps: two of its own corners and
    /// two points where its lower edges cross the lower cube's side, named by
    /// that edge and that side.
    #[test]
    fn a_cube_half_over_an_edge_touches_over_the_overlap() {
        let lower = cube(DVec3::ZERO, DQuat::IDENTITY, 0.5);
        let upper = cube(DVec3::new(0.5, 0.99, 0.0), DQuat::IDENTITY, 0.5);
        let m = collide(&lower, &upper, SPEC);
        assert_eq!(m.points().len(), 4, "{m:?}");
        let edge = 0.5 + CLIP_TOLERANCE;
        assert_xz(&m, &[(0.0, -0.5), (0.0, 0.5), (edge, -0.5), (edge, 0.5)]);
        let side = face_code(face_index(0, true));
        let clipped: Vec<_> = m.points().iter().filter(|p| p.point.x > 0.25).collect();
        assert_eq!(clipped.len(), 2, "{m:?}");
        for p in clipped {
            assert_eq!(p.id & 0xff, side, "cut by the lower cube's +X side: {m:?}");
            let edge = (p.id >> 8) - 9;
            assert!(
                edge < 12 && edge / 4 == 0,
                "along an X edge of the upper: {m:?}"
            );
        }
    }

    /// **Turned about the normal.** A cube turned 45° on top of another is an
    /// octagon of eight points, reduced to four: the deepest, the furthest from
    /// it and the two that keep the most area — a square of 1.1716 across the
    /// diagonal, 0.5858 in area, the largest four of the octagon hold.
    #[test]
    fn a_turned_cube_on_a_cube_is_reduced_to_the_four_that_keep_the_most_area() {
        let lower = cube(DVec3::ZERO, DQuat::IDENTITY, 0.5);
        let turn = crate::rotation_from_scaled_axis(DVec3::Y * core::f64::consts::FRAC_PI_4);
        let upper = cube(DVec3::new(0.0, 0.99, 0.0), turn, 0.5);
        let m = collide(&lower, &upper, SPEC);
        assert_eq!(m.points().len(), 4, "{m:?}");
        let p: Vec<DVec3> = m.points().iter().map(|p| p.point).collect();
        // The points come in the order they were chosen: the first two are
        // the furthest apart and the last two lie either side of the line
        // between them, so those are the quadrilateral's diagonals, and its
        // area is half their cross product.
        let area = 0.5 * (p[1] - p[0]).cross(p[3] - p[2]).y.abs();
        let corner = 0.5 * core::f64::consts::SQRT_2 - 0.5;
        // The square's diagonal runs from (0.5, c) to (−0.5, −c).
        let expected = 0.5 * (1.0 + 4.0 * corner * corner);
        assert!(
            (area - expected).abs() < 1e-3,
            "kept {area} of a best {expected}: {m:?}"
        );
        for point in m.points() {
            assert!(close(point.separation, -0.01), "{m:?}");
        }
    }

    /// **Edge on edge.** A cube turned 45° about Z has a ridge along Z on top;
    /// one turned 45° about X, above it, a ridge along X underneath. Crossed a
    /// centimetre into each other they touch at one point, the crossing, a
    /// centimetre deep, with the normal straight up and the two edges' ids.
    #[test]
    fn crossed_ridges_touch_at_one_point() {
        let quarter = core::f64::consts::FRAC_PI_4;
        let ridge = 0.5 * core::f64::consts::SQRT_2;
        let lower = cube(
            DVec3::ZERO,
            crate::rotation_from_scaled_axis(DVec3::Z * quarter),
            0.5,
        );
        let upper = cube(
            DVec3::new(0.0, 2.0 * ridge - 0.01, 0.0),
            crate::rotation_from_scaled_axis(DVec3::X * quarter),
            0.5,
        );
        let m = collide(&lower, &upper, SPEC);
        assert_eq!(m.points().len(), 1, "{m:?}");
        assert!((m.normal - DVec3::Y).length() < 1e-12, "{m:?}");
        let p = m.points()[0];
        assert!(close(p.separation, -0.01), "{m:?}");
        assert!(
            (p.point - DVec3::new(0.0, ridge - 0.005, 0.0)).length() < 1e-9,
            "{m:?}"
        );
        let (a, b) = (p.id & 0xff, p.id >> 8);
        assert!((9..21).contains(&a) && (9..21).contains(&b), "{m:?}");
        assert_eq!((a - 9) / 4, 2, "the lower cube's ridge runs along its Z");
        assert_eq!((b - 9) / 4, 0, "the upper cube's along its X");
    }

    /// **Corner on face.** A cube stood on a corner, its long diagonal upright,
    /// a centimetre into a cube below touches at that corner alone.
    #[test]
    fn a_cube_on_its_corner_touches_at_one_point() {
        let diagonal = DVec3::ONE.normalize();
        let tip = 0.5 * 3.0f64.sqrt();
        let lower = cube(DVec3::ZERO, DQuat::IDENTITY, 0.5);
        let upper = cube(
            DVec3::new(0.0, 0.5 + tip - 0.01, 0.0),
            arc(diagonal, -DVec3::Y),
            0.5,
        );
        let m = collide(&lower, &upper, SPEC);
        assert_eq!(m.points().len(), 1, "{m:?}");
        assert!((m.normal - DVec3::Y).length() < 1e-12, "{m:?}");
        let p = m.points()[0];
        assert!(close(p.separation, -0.01), "{m:?}");
        assert!(
            (p.point - DVec3::new(0.0, 0.495, 0.0)).length() < 1e-9,
            "{m:?}"
        );
        assert_eq!(p.id & 0xff, face_code(face_index(1, true)), "{m:?}");
    }

    /// **A turned box leaning on a turned box** is found along the turned
    /// faces, not the world's axes: two cubes both turned 0.4 rad about Z, one
    /// on the other along their shared up axis, touch face to face.
    #[test]
    fn turned_boxes_touch_along_their_turned_faces() {
        let turn = crate::rotation_from_scaled_axis(DVec3::Z * 0.4);
        let up = turn * DVec3::Y;
        let lower = cube(DVec3::new(1.0, 2.0, 3.0), turn, 0.5);
        let upper = cube(DVec3::new(1.0, 2.0, 3.0) + up * 0.98, turn, 0.5);
        let m = collide(&lower, &upper, SPEC);
        assert_eq!(m.points().len(), 4, "{m:?}");
        assert!((m.normal - up).length() < 1e-12, "{m:?}");
        for p in m.points() {
            assert!((p.separation + 0.02).abs() < 1e-9, "{m:?}");
        }
    }

    /// **Apart is apart, and the cache only saves work.** Beyond the
    /// speculative distance there are no points and the separating axis is
    /// cached; brought back into contact with that stale axis cached, the
    /// pair gets the full test's manifold, bit for bit.
    #[test]
    fn a_cached_axis_never_changes_the_manifold() {
        let lower = cube(DVec3::ZERO, DQuat::IDENTITY, 0.5);
        let turn = crate::rotation_from_scaled_axis(DVec3::new(0.1, 0.3, -0.2));
        let mut cache = SatCache::default();
        let apart = cube(DVec3::new(0.2, 1.2, 0.1), turn, 0.5);
        let m = super::super::collide_cached(&lower, &apart, SPEC, &mut cache);
        assert!(m.points().is_empty(), "{m:?}");
        assert!(cache.is_set());
        assert_eq!(cache.axis, Some(Axis::FaceA(1)), "{cache:?}");

        for y in [1.2, 1.1, 1.0, 0.95, 0.9] {
            let upper = cube(DVec3::new(0.2, y, 0.1), turn, 0.5);
            let cached = super::super::collide_cached(&lower, &upper, SPEC, &mut cache);
            assert_eq!(cached, collide(&lower, &upper, SPEC), "at y = {y}");
        }
        let touching = super::super::collide_cached(
            &lower,
            &cube(DVec3::new(0.2, 0.9, 0.1), turn, 0.5),
            SPEC,
            &mut cache,
        );
        assert!(!touching.points().is_empty(), "{touching:?}");
    }

    /// **The ids are the same whichever box's face is the reference.** A cube
    /// resting across the top of a bigger one has the bigger one's top face as
    /// its reference; resting under it, the smaller one's. Swapping which box
    /// is `A` swaps the halves of every id and nothing else.
    #[test]
    fn swapping_the_boxes_swaps_the_halves_of_each_id() {
        let big = cube(DVec3::ZERO, DQuat::IDENTITY, 0.5);
        let small = cube(DVec3::new(0.1, 0.74, 0.0), DQuat::IDENTITY, 0.25);
        let one = collide(&big, &small, SPEC);
        let other = collide(&small, &big, SPEC);
        let swap = |id: u32| (id >> 8) | ((id & 0xff) << 8);
        let mut a: Vec<u32> = one.points().iter().map(|p| p.id).collect();
        let mut b: Vec<u32> = other.points().iter().map(|p| swap(p.id)).collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a.len(), 4, "{one:?}");
        assert_eq!(a, b);
    }
}
