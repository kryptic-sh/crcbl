//! Rays, sweeps and overlaps against a [`TriangleMesh`], exact to the
//! triangle: its own tree finds the triangles a query's bounds reach, and each
//! is tested exactly.
//!
//! A mesh answers in its own frame, and names the triangle and where on it;
//! [`PlacedMesh`] is a mesh at a [`Transform`], answering in the world as the
//! other shapes of [`crate::PhysicsWorld`] do. To a query a triangle is a
//! surface with two sides: a hit's normal faces the side the query came from.

use glam::{DMat3, DQuat, DVec3};

use super::TriangleMesh;
use super::geometry::{
    Cuboid, box_triangle_axes, closest_on_triangle, ray_triangle, segment_triangle,
    sweep_into_triangle,
};
use crate::broadphase::{BvhHit, Ray, Segment};
use crate::collider::{Aabb, Capsule, Sphere};
use crate::components::Transform;
use crate::query::{Penetration, ShapeHit};

/// A query's nearest hit on a [`TriangleMesh`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshHit {
    /// The triangle struck: an index into [`TriangleMesh::triangles`].
    pub triangle: u32,
    /// The weights of the triangle's three corners that make the point struck
    /// on it — [`hit`](Self::hit)'s `point` in the mesh's frame.
    pub barycentric: DVec3,
    /// The hit, as the query's shape-level form reports one: for a sweep,
    /// `point` is on the triangle and `normal` points from it to the swept
    /// shape; for a ray, `normal` faces the ray.
    pub hit: ShapeHit,
}

/// The buffers a mesh query descends its tree in, kept by whoever runs the
/// query.
#[derive(Debug, Default)]
pub(crate) struct MeshScratch {
    stack: Vec<u32>,
    found: Vec<u32>,
    hits: Vec<BvhHit>,
}

impl TriangleMesh {
    /// The nearest triangle a ray in the mesh's frame crosses within its
    /// bounds, from either side: [`ShapeHit::started_inside`] is never set,
    /// since a surface has no inside. A ray running along a triangle's plane
    /// does not strike it.
    #[must_use]
    pub fn cast_ray(&self, ray: &Ray) -> Option<MeshHit> {
        self.cast_ray_with(ray, &mut MeshScratch::default())
    }

    pub(crate) fn cast_ray_with(&self, ray: &Ray, scratch: &mut MeshScratch) -> Option<MeshHit> {
        self.bvh()
            .traverse_ray_into(ray, &mut scratch.stack, &mut scratch.hits);
        let mut best: Option<(f64, u32, DVec3)> = None;
        for leaf in &scratch.hits {
            if best.is_some_and(|(t, _, _)| leaf.t > t) {
                continue;
            }
            let index = leaf.element_id;
            let Some((t, weights)) =
                ray_triangle(ray.origin, ray.dir, &self.corners(index as usize))
            else {
                continue;
            };
            if t < ray.t_min || t > ray.t_max {
                continue;
            }
            if best.is_none_or(|(bt, bi, _)| t < bt || (t == bt && index < bi)) {
                best = Some((t, index, weights));
            }
        }
        let (t, triangle, barycentric) = best?;
        let n = self.normal(triangle as usize);
        Some(MeshHit {
            triangle,
            barycentric,
            hit: ShapeHit {
                t,
                point: ray.origin + ray.dir * t,
                normal: if ray.dir.dot(n) <= 0.0 { n } else { -n },
                started_inside: false,
            },
        })
    }

    /// The first triangle a sphere of `radius` meets moving along `segment`,
    /// in the mesh's frame, as [`crate::swept_sphere_vs_sphere`] reports one:
    /// `t` in `[0, 1]`, a sweep that starts touching a triangle is a contact at
    /// `t = 0` with [`ShapeHit::started_inside`].
    #[must_use]
    pub fn sweep_sphere(&self, segment: &Segment, radius: f64) -> Option<MeshHit> {
        self.sweep_with(segment, radius, DVec3::ZERO, &mut MeshScratch::default())
    }

    /// [`sweep_sphere`](Self::sweep_sphere) for a capsule standing up the
    /// mesh frame's `y` axis, `segment` the path of its centre, as
    /// [`crate::swept_capsule_vs_aabb`] takes one.
    #[must_use]
    pub fn sweep_capsule(
        &self,
        segment: &Segment,
        radius: f64,
        half_height: f64,
    ) -> Option<MeshHit> {
        self.sweep_with(
            segment,
            radius,
            DVec3::Y * half_height,
            &mut MeshScratch::default(),
        )
    }

    /// The first triangle a capsule of `radius` about the segment from
    /// `-half` to `+half` meets moving along `segment` — a sphere when `half`
    /// is zero.
    pub(crate) fn sweep_with(
        &self,
        segment: &Segment,
        radius: f64,
        half: DVec3,
        scratch: &mut MeshScratch,
    ) -> Option<MeshHit> {
        let (start, dir) = (segment.start, segment.end - segment.start);
        let reach = DVec3::splat(radius) + half.abs();
        let bounds = Aabb::new(
            start.min(segment.end) - reach,
            start.max(segment.end) + reach,
        );
        self.bvh()
            .traverse_aabb_into(&bounds, &mut scratch.stack, &mut scratch.found);
        // (t, triangle, started inside)
        let mut best: Option<(f64, u32, bool)> = None;
        for &index in &scratch.found {
            let corners = self.corners(index as usize);
            let normal = self.normal(index as usize);
            let inside =
                segment_triangle(start - half, start + half, &corners, normal).distance() <= radius;
            let t = if inside {
                Some(0.0)
            } else {
                sweep_into_triangle(start, dir, &corners, half, radius)
            };
            if let Some(t) = t
                && best.is_none_or(|(bt, bi, _)| t < bt || (t == bt && index < bi))
            {
                best = Some((t, index, inside));
            }
        }
        let (t, triangle, started_inside) = best?;
        let corners = self.corners(triangle as usize);
        let normal = self.normal(triangle as usize);
        let centre = start + dir * t;
        let closest = segment_triangle(centre - half, centre + half, &corners, normal);
        let away = closest.on_segment - closest.on_triangle.point;
        let distance = away.length();
        Some(MeshHit {
            triangle,
            barycentric: closest.on_triangle.barycentric,
            hit: ShapeHit {
                t,
                point: closest.on_triangle.point,
                normal: facing(away, distance, normal, centre - corners[0]),
                started_inside,
            },
        })
    }

    /// Whether a sphere in the mesh's frame touches any triangle.
    #[must_use]
    pub fn overlaps_sphere(&self, sphere: &Sphere) -> bool {
        self.overlaps_sphere_with(sphere, &mut MeshScratch::default())
    }

    pub(crate) fn overlaps_sphere_with(&self, sphere: &Sphere, scratch: &mut MeshScratch) -> bool {
        self.bvh()
            .traverse_aabb_into(&sphere.aabb(), &mut scratch.stack, &mut scratch.found);
        scratch.found.iter().any(|&index| {
            let corners = self.corners(index as usize);
            (closest_on_triangle(sphere.centre, &corners).point - sphere.centre).length_squared()
                <= sphere.radius * sphere.radius
        })
    }

    /// Whether a box in the mesh's frame touches any triangle, exactly: by
    /// the separating axis test of
    /// Akenine-Möller's triangle–box overlap, not by bounds.
    #[must_use]
    pub fn overlaps_aabb(&self, aabb: &Aabb) -> bool {
        self.overlaps_box_with(
            aabb.centre(),
            DQuat::IDENTITY,
            aabb.extents() * 0.5,
            &mut MeshScratch::default(),
        )
    }

    pub(crate) fn overlaps_box_with(
        &self,
        centre: DVec3,
        rotation: DQuat,
        half: DVec3,
        scratch: &mut MeshScratch,
    ) -> bool {
        let m = DMat3::from_quat(rotation);
        let cuboid = Cuboid {
            centre,
            axes: [m.x_axis, m.y_axis, m.z_axis],
            half,
        };
        let reach = DVec3::new(
            cuboid.radius(DVec3::X),
            cuboid.radius(DVec3::Y),
            cuboid.radius(DVec3::Z),
        );
        self.bvh().traverse_aabb_into(
            &Aabb::from_centre_half(centre, reach),
            &mut scratch.stack,
            &mut scratch.found,
        );
        scratch.found.iter().any(|&index| {
            box_triangle_axes(
                &cuboid,
                &self.corners(index as usize),
                self.normal(index as usize),
            )
            .into_iter()
            .flatten()
            .all(|axis| axis.separation <= 0.0)
        })
    }

    /// How far a capsule standing up the mesh frame's `y` axis must move, and
    /// which way, to leave the triangle it is deepest in — the push-out
    /// [`crate::capsule_penetration_vs_aabb`] gives for a box — or `None` if it
    /// is inside none.
    ///
    /// A capsule whose core crosses a triangle is pushed out along the
    /// triangle's normal on the side its centre is, far enough for the end
    /// behind it to clear the plane.
    #[must_use]
    pub fn capsule_penetration(&self, capsule: &Capsule) -> Option<Penetration> {
        self.capsule_penetration_with(
            capsule.centre,
            DVec3::Y * capsule.half_height,
            capsule.radius,
            &mut MeshScratch::default(),
        )
    }

    pub(crate) fn capsule_penetration_with(
        &self,
        centre: DVec3,
        half: DVec3,
        radius: f64,
        scratch: &mut MeshScratch,
    ) -> Option<Penetration> {
        let reach = DVec3::splat(radius) + half.abs();
        self.bvh().traverse_aabb_into(
            &Aabb::new(centre - reach, centre + reach),
            &mut scratch.stack,
            &mut scratch.found,
        );
        let (p0, p1) = (centre - half, centre + half);
        let mut deepest: Option<(f64, u32, DVec3)> = None;
        for &index in &scratch.found {
            let corners = self.corners(index as usize);
            let normal = self.normal(index as usize);
            let closest = segment_triangle(p0, p1, &corners, normal);
            let away = closest.on_segment - closest.on_triangle.point;
            let distance = away.length();
            if distance >= radius {
                continue;
            }
            let out = facing(away, distance, normal, centre - corners[0]);
            let depth = if distance > 0.0 {
                radius - distance
            } else {
                let behind = (-out.dot(p0 - corners[0]))
                    .max(-out.dot(p1 - corners[0]))
                    .max(0.0);
                radius + behind
            };
            if deepest.is_none_or(|(d, i, _)| depth > d || (depth == d && index < i)) {
                deepest = Some((depth, index, out));
            }
        }
        deepest.map(|(depth, _, normal)| Penetration { normal, depth })
    }
}

/// The unit direction from a triangle to a shape `distance` away along
/// `away`, or, where they touch, its normal on the side `offset` — the shape's
/// centre less a corner — lies.
fn facing(away: DVec3, distance: f64, normal: DVec3, offset: DVec3) -> DVec3 {
    if distance > 0.0 {
        away / distance
    } else if normal.dot(offset) >= 0.0 {
        normal
    } else {
        -normal
    }
}

/// A mesh at a transform: what [`crate::PhysicsWorld`] keeps for one, and
/// answers world queries through.
#[derive(Debug, Clone)]
pub(crate) struct PlacedMesh {
    pub(crate) mesh: TriangleMesh,
    pub(crate) transform: Transform,
    /// Every triangle's world bounds, together.
    pub(crate) bounds: Aabb,
}

impl PlacedMesh {
    pub(crate) fn new(mesh: TriangleMesh, transform: Transform) -> Self {
        let local = mesh.bounds();
        let m = DMat3::from_quat(transform.rotation);
        let half = local.extents() * 0.5;
        let reach = DVec3::new(
            m.x_axis.x.abs() * half.x + m.y_axis.x.abs() * half.y + m.z_axis.x.abs() * half.z,
            m.x_axis.y.abs() * half.x + m.y_axis.y.abs() * half.y + m.z_axis.y.abs() * half.z,
            m.x_axis.z.abs() * half.x + m.y_axis.z.abs() * half.y + m.z_axis.z.abs() * half.z,
        );
        let bounds = Aabb::from_centre_half(to_world(&transform, local.centre()), reach);
        Self {
            mesh,
            transform,
            bounds,
        }
    }

    fn to_local(&self, point: DVec3) -> DVec3 {
        self.transform.rotation.inverse() * (point - self.transform.position)
    }

    fn hit_to_world(&self, hit: ShapeHit) -> ShapeHit {
        ShapeHit {
            point: to_world(&self.transform, hit.point),
            normal: self.transform.rotation * hit.normal,
            ..hit
        }
    }

    pub(crate) fn cast_ray(&self, ray: &Ray, scratch: &mut MeshScratch) -> Option<ShapeHit> {
        let local = Ray {
            origin: self.to_local(ray.origin),
            dir: self.transform.rotation.inverse() * ray.dir,
            ..*ray
        };
        let hit = self.mesh.cast_ray_with(&local, scratch)?.hit;
        // The world point from the world ray, so it lies on it to the bit.
        Some(ShapeHit {
            point: ray.origin + ray.dir * hit.t,
            normal: self.transform.rotation * hit.normal,
            ..hit
        })
    }

    /// A capsule of `radius` about `-half ..= +half`, turned with the world,
    /// swept along `segment`: a sphere when `half` is zero.
    pub(crate) fn sweep(
        &self,
        segment: &Segment,
        radius: f64,
        half: DVec3,
        scratch: &mut MeshScratch,
    ) -> Option<ShapeHit> {
        let local = Segment::new(self.to_local(segment.start), self.to_local(segment.end));
        let half = self.transform.rotation.inverse() * half;
        let hit = self.mesh.sweep_with(&local, radius, half, scratch)?;
        Some(self.hit_to_world(hit.hit))
    }

    pub(crate) fn overlaps_sphere(&self, sphere: &Sphere, scratch: &mut MeshScratch) -> bool {
        self.mesh.overlaps_sphere_with(
            &Sphere::new(self.to_local(sphere.centre), sphere.radius),
            scratch,
        )
    }

    pub(crate) fn overlaps_aabb(&self, aabb: &Aabb, scratch: &mut MeshScratch) -> bool {
        self.mesh.overlaps_box_with(
            self.to_local(aabb.centre()),
            self.transform.rotation.inverse(),
            aabb.extents() * 0.5,
            scratch,
        )
    }

    pub(crate) fn capsule_penetration(
        &self,
        capsule: &Capsule,
        scratch: &mut MeshScratch,
    ) -> Option<Penetration> {
        let penetration = self.mesh.capsule_penetration_with(
            self.to_local(capsule.centre),
            self.transform.rotation.inverse() * (DVec3::Y * capsule.half_height),
            capsule.radius,
            scratch,
        )?;
        Some(Penetration {
            normal: self.transform.rotation * penetration.normal,
            ..penetration
        })
    }
}

fn to_world(transform: &Transform, local: DVec3) -> DVec3 {
    transform.position + transform.rotation * local
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two triangles side by side in the floor, each a unit right triangle:
    /// triangle 0 over `x ∈ [0, 1]` and triangle 1 over `x ∈ [2, 3]`.
    fn two() -> TriangleMesh {
        TriangleMesh::new(
            &[
                DVec3::new(0.0, 0.0, 0.0),
                DVec3::new(0.0, 0.0, 1.0),
                DVec3::new(1.0, 0.0, 0.0),
                DVec3::new(2.0, 0.0, 0.0),
                DVec3::new(2.0, 0.0, 1.0),
                DVec3::new(3.0, 0.0, 0.0),
            ],
            &[[0, 1, 2], [3, 4, 5]],
        )
        .unwrap()
    }

    fn close(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-12
    }

    /// **A ray names the triangle it struck and where**: straight down at
    /// `(2.25, ·, 0.25)` it crosses triangle 1 at weights `(½, ¼, ¼)` of its
    /// corners `(2,0,0)`, `(2,0,1)`, `(3,0,0)`; from below the normal faces
    /// down.
    #[test]
    fn a_ray_names_the_triangle_and_the_point() {
        let mesh = two();
        let hit = mesh
            .cast_ray(&Ray::new(DVec3::new(2.25, 3.0, 0.25), -DVec3::Y))
            .unwrap();
        assert_eq!(hit.triangle, 1);
        assert!(
            close(hit.barycentric, DVec3::new(0.5, 0.25, 0.25)),
            "{hit:?}"
        );
        assert!((hit.hit.t - 3.0).abs() < 1e-15);
        assert_eq!(hit.hit.normal, DVec3::Y);
        let below = mesh
            .cast_ray(&Ray::new(DVec3::new(0.25, -1.0, 0.25), DVec3::Y))
            .unwrap();
        assert_eq!(below.triangle, 0);
        assert_eq!(below.hit.normal, -DVec3::Y);
        assert!(
            mesh.cast_ray(&Ray::new(DVec3::new(1.5, 3.0, 0.25), -DVec3::Y))
                .is_none(),
            "between the two"
        );
    }

    /// **A sphere dropped on a triangle's face lands a radius above it**, and
    /// the contact is under its centre; one dropped past the hypotenuse of
    /// triangle 0 lands on that edge.
    #[test]
    fn a_swept_sphere_lands_on_the_face_or_the_edge() {
        let mesh = two();
        let hit = mesh
            .sweep_sphere(
                &Segment::new(DVec3::new(2.25, 2.0, 0.25), DVec3::new(2.25, 0.0, 0.25)),
                0.5,
            )
            .unwrap();
        assert_eq!(hit.triangle, 1);
        assert!((hit.hit.t - 0.75).abs() < 1e-12, "{hit:?}");
        assert!(close(hit.hit.point, DVec3::new(2.25, 0.0, 0.25)), "{hit:?}");
        assert!(close(hit.barycentric, DVec3::new(0.5, 0.25, 0.25)));
        assert!(close(hit.hit.normal, DVec3::Y));
        assert!(!hit.hit.started_inside);

        // Over (0.75, ·, 0.75), 0.25√2 past the hypotenuse x + z = 1, whose
        // nearest point is (0.5, 0, 0.5): radius 0.5 touches it at height
        // √(0.25 − 0.125).
        let hit = mesh
            .sweep_sphere(
                &Segment::new(DVec3::new(0.75, 2.0, 0.75), DVec3::new(0.75, 0.0, 0.75)),
                0.5,
            )
            .unwrap();
        assert_eq!(hit.triangle, 0);
        let height = 0.125f64.sqrt();
        assert!((hit.hit.t - (2.0 - height) / 2.0).abs() < 1e-12, "{hit:?}");
        assert!(close(hit.hit.point, DVec3::new(0.5, 0.0, 0.5)), "{hit:?}");
        assert!(close(hit.barycentric, DVec3::new(0.0, 0.5, 0.5)), "{hit:?}");
    }

    /// **A capsule swept sideways into a wall of triangles** meets it with
    /// its flank, and one that starts touching is a contact at zero.
    #[test]
    fn a_swept_capsule_meets_with_its_flank_or_starts_touching() {
        // A wall in x = 0, facing -x: y, z ∈ [0, 2].
        let wall = TriangleMesh::new(
            &[
                DVec3::new(0.0, 0.0, 0.0),
                DVec3::new(0.0, 2.0, 0.0),
                DVec3::new(0.0, 2.0, 2.0),
                DVec3::new(0.0, 0.0, 2.0),
            ],
            &[[0, 2, 1], [0, 3, 2]],
        )
        .unwrap();
        assert_eq!(wall.normal(0), -DVec3::X);
        let hit = wall
            .sweep_capsule(
                &Segment::new(DVec3::new(-3.0, 1.0, 1.0), DVec3::new(1.0, 1.0, 1.0)),
                0.5,
                0.5,
            )
            .unwrap();
        // The flank, 0.5 out, touches with the centre at x = -0.5.
        assert!((hit.hit.t - 0.625).abs() < 1e-12, "{hit:?}");
        assert!(close(hit.hit.normal, -DVec3::X), "{hit:?}");
        let touching = wall
            .sweep_capsule(
                &Segment::new(DVec3::new(-0.4, 1.0, 1.0), DVec3::new(-1.0, 1.0, 1.0)),
                0.5,
                0.5,
            )
            .unwrap();
        assert_eq!(touching.hit.t, 0.0);
        assert!(touching.hit.started_inside);
    }

    /// Overlaps are exact: a box under a triangle's hypotenuse overlaps its
    /// bounds and not the triangle, and a sphere overlaps by distance.
    #[test]
    fn overlaps_are_decided_by_the_triangles_not_their_bounds() {
        let mesh = two();
        let past_hypotenuse = Aabb::from_centre_half(DVec3::new(0.9, 0.0, 0.9), DVec3::splat(0.1));
        assert!(!mesh.overlaps_aabb(&past_hypotenuse));
        assert!(mesh.overlaps_aabb(&Aabb::from_centre_half(
            DVec3::new(0.3, 0.05, 0.3),
            DVec3::splat(0.1)
        )));
        assert!(!mesh.overlaps_sphere(&Sphere::new(DVec3::new(0.9, 0.0, 0.9), 0.1)));
        assert!(mesh.overlaps_sphere(&Sphere::new(DVec3::new(0.3, 0.1, 0.3), 0.1)));
    }

    /// A capsule sunk into the floor is pushed up by its depth; a placed mesh
    /// answers in the world.
    #[test]
    fn a_sunk_capsule_is_pushed_out_along_the_normal() {
        let mesh = two();
        let capsule = Capsule::new(DVec3::new(0.3, 0.9, 0.3), 0.25, 0.75);
        let penetration = mesh.capsule_penetration(&capsule).unwrap();
        assert!(close(penetration.normal, DVec3::Y));
        assert!((penetration.depth - 0.1).abs() < 1e-12, "{penetration:?}");

        // Turned a quarter about x, the floor is the wall z = 0 facing +z, and
        // moved 5 along x.
        let turn = crate::rotation_from_scaled_axis(DVec3::X * core::f64::consts::FRAC_PI_2);
        let placed = PlacedMesh::new(mesh, Transform::new(DVec3::new(5.0, 0.0, 0.0), turn));
        let mut scratch = MeshScratch::default();
        let hit = placed
            .cast_ray(
                &Ray::new(DVec3::new(5.25, -0.25, -3.0), DVec3::Z),
                &mut scratch,
            )
            .unwrap();
        assert!((hit.t - 3.0).abs() < 1e-12, "{hit:?}");
        assert!(close(hit.normal, -DVec3::Z), "{hit:?}");
        // The turned floor is flat to within the rotation's rounding.
        assert!(
            placed
                .bounds
                .inflated(1e-12)
                .contains(&Aabb::new(hit.point, hit.point))
        );
    }
}
