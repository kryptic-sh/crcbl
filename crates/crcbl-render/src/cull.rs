//! Frustum culling: an axis-aligned box, six half-spaces, and the reference
//! implementation `cull.slang` is checked against.
//!
//! ```text
//!  camera.view_projection(aspect) ──▶ Frustum::from_view_projection
//!                                            │ six planes, unnormalized
//!                                            ▼
//!  GpuMesh::bounds_*  ──▶ Aabb ──transformed by GpuInstance::transform──▶ Aabb
//!                                            │
//!                                     Frustum::intersects
//!                                            ▼
//!                                     visible_instances
//! ```
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.3: "Compute pass: frustum cull
//! against instance AABBs → compacted visible instance list". The compute pass
//! is `crcbl-shaders`' `cull.slang`; **everything in this module is ordinary
//! Rust**, and it exists for two reasons that are not the same reason:
//!
//! * [`Frustum::from_view_projection`] is what fills the shader's uniform block.
//!   Nothing on the GPU can extract planes from a matrix it is never handed.
//! * [`visible_instances`] is the **oracle**. A dispatch that runs and writes a
//!   buffer nobody reads is indistinguishable from one that does nothing, so
//!   `crcbl-vk`'s `cull` e2e reads the list back and compares it against this,
//!   over instances placed inside, outside each plane, and straddling.
//!
//! [`crate::draw_gen`] is what consumes the visible list: it turns the survivors
//! into per-bucket indirect draw arguments, which [`crate::forward`] draws from.
//! This module stays the *proof* rather than the path — nothing in a frame calls
//! [`visible_instances`], and that is the point of an oracle.
//!
//! # And the same again, one layer down
//!
//! [`cluster_survives_cull`] is §3.5's per-cluster rejection: the frustum
//! against a cluster's bounding **sphere**, and its normal cone against the
//! camera. It is the second oracle here for the first one's reason — the rule
//! it mirrors runs in `mesh_cluster.slang`'s amplification stage, where the
//! only observable is a counter, and a rule stated in Rust is a rule a test can
//! state the answer to by hand.
//!
//! The instance cull above it is unchanged and still runs first. Cluster
//! rejection happens *inside* a surviving instance; neither replaces the other.
//!
//! # A rotated box is not a box
//!
//! [`Aabb::transformed`] uses the standard conservative method — the
//! **absolute-value matrix** — and its docs carry the derivation. The result
//! contains the rotated box and is usually larger, which is the direction a cull
//! must err in: a false survivor costs a draw, a false rejection is a hole in
//! the picture.
//!
//! # The planes are not normalized
//!
//! [`Frustum::intersects`] scales linearly with each plane's normal, so
//! normalizing changes no answer — and under the engine's reversed-Z
//! **infinite** perspective one of the six planes comes out with a zero normal,
//! because the far plane is at infinity and rejects nothing. Normalizing would
//! divide that one by zero and make every comparison against it `NaN`, which is
//! false, which reads as "outside" — every instance culled, an empty screen, and
//! no error anywhere. [`Frustum::from_view_projection`] therefore hands the
//! planes over exactly as extracted, and this module's
//! `the_infinite_far_plane_is_degenerate_and_rejects_nothing` is what says so.

use crcbl_shaders::mesh::{GpuInstance, GpuMesh};
use crcbl_shaders::meshlet::ClusterBounds;
use glam::{Mat3, Mat4, Vec3, Vec4};

/// An axis-aligned bounding box.
///
/// Used for a mesh's **local-space** bounds — the box around its vertices as
/// they sit in the pool, before any instance transform — and for the world-space
/// box [`Aabb::transformed`] produces from one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aabb {
    /// Lowest corner.
    pub min: Vec3,
    /// Highest corner.
    pub max: Vec3,
}

impl Aabb {
    /// The box containing every point, or `None` for an empty iterator.
    ///
    /// `None` rather than an empty-box sentinel: there is no box containing
    /// nothing, and the usual encoding for one — `min` at `+INFINITY`, `max` at
    /// `-INFINITY` — is a pair of values that survive into a buffer and turn
    /// every later comparison into a `NaN`.
    #[must_use]
    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Option<Self> {
        let mut points = points.into_iter();
        let first = points.next()?;
        let mut bounds = Self {
            min: first,
            max: first,
        };
        for point in points {
            bounds.min = bounds.min.min(point);
            bounds.max = bounds.max.max(point);
        }
        Some(bounds)
    }

    /// The box's centre.
    #[must_use]
    pub fn center(&self) -> Vec3 {
        (self.max + self.min) * 0.5
    }

    /// Half the box's size along each axis.
    #[must_use]
    pub fn half_extent(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    /// The smallest *axis-aligned* box this module will claim contains
    /// `transform` applied to this one.
    ///
    /// **A rotated box is not a box**, so this is the standard conservative
    /// construction (Ericson, *Real-Time Collision Detection* §4.2.6): the new
    /// centre is the transform applied to the old centre, and the new
    /// half-extent is the *element-wise absolute value* of the transform's 3×3
    /// part applied to the old half-extent. Along world axis `i` the box reaches
    /// `Σⱼ |m[i][j]| · e[j]`, which is the furthest any corner can get once every
    /// combination of `±e` is allowed — so the result contains the rotated box,
    /// and for a rotation that is not a multiple of a quarter turn it is
    /// strictly larger.
    ///
    /// `cull.slang` performs exactly this arithmetic, which is why a rotated
    /// instance is one of the cases the e2e places deliberately.
    #[must_use]
    pub fn transformed(&self, transform: Mat4) -> Self {
        let center = transform.transform_point3(self.center());
        let basis = Mat3::from_mat4(transform);
        // `abs` element-wise, then the ordinary matrix-vector product. Built
        // column by column because that is how `glam` stores a `Mat3`; the
        // product is the same either way, since every entry is non-negative.
        let absolute = Mat3::from_cols(basis.x_axis.abs(), basis.y_axis.abs(), basis.z_axis.abs());
        let extent = absolute * self.half_extent();
        Self {
            min: center - extent,
            max: center + extent,
        }
    }
}

/// Planes in a [`Frustum`].
pub const PLANE_COUNT: usize = crcbl_shaders::cull::PLANE_COUNT;

/// A view frustum, as six half-spaces.
///
/// Each plane is `[nx, ny, nz, d]` and a point `p` is inside it when
/// `n · p + d >= 0`. The normals point *inward*, and none of them is unit
/// length — see the [module docs](self) for why normalizing them would be a
/// bug rather than a tidy-up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frustum {
    /// The six half-spaces, in the order
    /// [`from_view_projection`](Frustum::from_view_projection) extracts them.
    pub planes: [Vec4; PLANE_COUNT],
}

impl Frustum {
    /// The frustum of a world → clip matrix.
    ///
    /// **Gribb-Hartmann**: each clip-space inequality is a linear function of
    /// the world position, so the plane is a sum or difference of two *rows* of
    /// the matrix. For the `0..w` depth range every backend here uses, the six
    /// are `x ≥ -w`, `x ≤ w`, `y ≥ -w`, `y ≤ w`, `z ≥ 0`, `z ≤ w`, giving
    /// `row3 ± row0`, `row3 ± row1`, `row2`, and `row3 - row2`.
    ///
    /// The last two are named by their clip-space inequality rather than as
    /// "near" and "far", because under this engine's reversed-Z they are the
    /// other way round from the usual reading: `z ≥ 0` is the *far* plane and
    /// `z ≤ w` is the near one. Under the infinite perspective projection the
    /// far plane is degenerate — a zero normal and a positive offset, so every
    /// point passes it — which is correct and is also why nothing here divides
    /// by a normal's length.
    #[must_use]
    pub fn from_view_projection(view_projection: Mat4) -> Self {
        let row = |index: usize| view_projection.row(index);
        let (x, y, z, w) = (row(0), row(1), row(2), row(3));
        Self {
            planes: [w + x, w - x, w + y, w - y, z, w - z],
        }
    }

    /// Whether `bounds` is on the inside of every plane, and so must be drawn.
    ///
    /// Conservative in the safe direction: a box outside the frustum but inside
    /// each of its planes taken alone — the corner case every plane-by-plane
    /// test has — answers `true`. The alternative rejects geometry that is on
    /// screen.
    #[must_use]
    pub fn intersects(&self, bounds: &Aabb) -> bool {
        let center = bounds.center();
        let extent = bounds.half_extent();
        self.planes.iter().all(|plane| {
            let normal = plane.truncate();
            // How far the box reaches from its centre towards this plane, along
            // the plane's own normal. `abs` on the normal rather than a corner
            // selection: same number, no branch, and it is what the shader does.
            let radius = normal.abs().dot(extent);
            normal.dot(center) + plane.w >= -radius
        })
    }

    /// Whether the sphere of `radius` about `center` reaches inside every
    /// plane. Conservative on [`Frustum::intersects`]' terms exactly.
    ///
    /// The **radius** is scaled by each plane normal's length rather than the
    /// plane divided by it, which is the same inequality and the only form that
    /// survives an unnormalized plane: the infinite reversed-Z projection's far
    /// plane has a zero normal, and dividing by it would make every comparison
    /// against it `NaN` — which is false, which reads as "outside". See the
    /// module docs, and `cluster_survives`' loop in `mesh_cluster.slang`, which
    /// is this arithmetic.
    ///
    /// A sphere rather than the box [`Frustum::intersects`] takes, because a
    /// cluster's bound *is* a sphere: [`ClusterBounds::radius`] is the distance
    /// to its furthest vertex, and boxing it up again would widen the bound by
    /// up to a factor of √3 for nothing.
    #[must_use]
    pub fn intersects_sphere(&self, center: Vec3, radius: f32) -> bool {
        self.planes.iter().all(|plane| {
            let normal = plane.truncate();
            normal.dot(center) + plane.w >= -radius * normal.length()
        })
    }
}

/// Whether a cluster of an instance may contribute a pixel — **the reference
/// implementation of §3.5's per-cluster cull**, and the rule
/// `mesh_cluster.slang`'s amplification stage runs.
///
/// `transform` is the instance's model → world matrix and `camera` its
/// world-space eye position, so this is the cull in **world space**: the same
/// space `frustum` is already in, because it comes from the same
/// [`Frustum::from_view_projection`] the instance cull is handed. `bounds` is
/// in the mesh's own space, which is what makes the transform necessary at all.
///
/// **The sphere is moved by the whole transform, scale included.** The centre
/// goes through the matrix, the radius is multiplied by the largest factor the
/// 3×3 can stretch a length by, and the cone axis is the 3×3 applied to it and
/// then made a unit vector again — which is the world-space form of the rule for
/// any transform that preserves angles, and conservative for one that does not.
/// This used to take the radius across unchanged and lean on
/// [`GpuInstance::transform`] being rigid; the engine's own scenes are not —
/// `crcbl::screenshot`'s trough is the open box scaled by `6 × 2 × 1.6` — and an
/// instance-sized sphere standing in for a scene-sized one rejects clusters that
/// fill half the frame.
///
/// # The two halves
///
/// Frustum first, against the bounding sphere, then the normal cone. The cone
/// rule and the derivation that makes it safe are
/// [`ClusterBounds::cone_cutoff`]'s docs; the short version is that the
/// [`radius`](ClusterBounds::radius) term is what stops it rejecting a cluster
/// that has a front-facing triangle.
///
/// **This is the oracle, not the path.** Nothing in a frame calls it — the
/// amplification stage is what culls — and it exists so a test can state the
/// answer for a case a picture cannot show, exactly as [`visible_instances`]
/// does for the instance cull one layer up.
///
/// [`GpuInstance::transform`]: crcbl_shaders::mesh::GpuInstance::transform
#[must_use]
pub fn cluster_survives_cull(
    frustum: &Frustum,
    camera: Vec3,
    transform: Mat4,
    bounds: &ClusterBounds,
) -> bool {
    let basis = Mat3::from_mat4(transform);
    let center = transform.transform_point3(Vec3::from_array(bounds.center));
    let radius = bounds.radius * max_stretch(basis);
    if !frustum.intersects_sphere(center, radius) {
        return false;
    }
    // A unit axis, so the two sides of the cone inequality below are in the same
    // units: the left is a length along the axis and the right is a length in
    // world space. Left unnormalised, a transform that scales by `s` multiplies
    // one side and not the other, which is a cone test that answers differently
    // for the same shape at two sizes.
    let axis = (basis * Vec3::from_array(bounds.cone_axis)).normalize_or_zero();
    let to_center = center - camera;
    let sine = (1.0 - bounds.cone_cutoff * bounds.cone_cutoff)
        .max(0.0)
        .sqrt();
    let faces_away =
        bounds.cone_cutoff > 0.0 && axis.dot(to_center) > sine.mul_add(to_center.length(), radius);
    !faces_away
}

/// The largest factor by which `basis` can grow a vector's length, bounded from
/// above — what a bounding *sphere*'s radius has to be multiplied by to still
/// bound the transformed cluster.
///
/// That factor is `basis`' largest singular value, and this bounds it rather
/// than computing it: `σ_max² = λ_max(BᵀB)`, and a symmetric matrix's spectral
/// radius is at most any induced norm of it, so the largest absolute row sum of
/// `BᵀB` is an upper bound on `σ_max²`. Bounding is the safe direction for a
/// cull, and the bound is **exact** whenever `basis`' columns are orthogonal —
/// every rotation-then-scale, which is every transform this engine builds —
/// because `BᵀB` is then diagonal and its rows are the columns' squared lengths.
///
/// The alternative, `max` over the columns' lengths, is exact on the same
/// matrices and *under*-estimates on a sheared one, which is the direction that
/// drops geometry. This form needs no contract about what a caller may pass.
///
/// `1.0` for a rotation or a translation, so nothing that was already correct
/// moves.
fn max_stretch(basis: Mat3) -> f32 {
    let gram = basis.transpose() * basis;
    let row_sum = |row: usize| {
        (0..3)
            .map(|column| gram.col(column)[row].abs())
            .sum::<f32>()
    };
    (0..3).map(row_sum).fold(0.0f32, f32::max).sqrt()
}

/// The instances `frustum` keeps, as indices into `instances`.
///
/// **The reference implementation, and the oracle `cull.slang` is checked
/// against.** It resolves each instance's mesh exactly as the shader does —
/// `GpuInstance::mesh` indexes `meshes`, the entry carries the local-space
/// bounds, the instance transform makes them world-space — and applies the same
/// rejections in the same order.
///
/// An instance without
/// [`GpuInstance::LIVE`](crcbl_shaders::mesh::GpuInstance::LIVE) in its
/// [`flags`](GpuInstance::flags) is **not** visible, and that is asked first:
/// the array is a pool, so an element between two live instances may be a slot
/// [`InstancePool::remove`](crate::instance_pool::InstancePool::remove) freed,
/// still holding the transform and mesh id it had when it was live. Nothing
/// else in the record is looked at until that bit says it means anything.
///
/// An instance whose mesh entry has a zero
/// [`index_count`](GpuMesh::index_count) is **not** visible: that is the
/// all-zero record [`MeshPool::free`](crate::mesh_pool::MeshPool::free) leaves
/// behind, whose bounds are a degenerate point box at the origin, and treating
/// it as geometry would put a mesh that does not exist on screen whenever the
/// origin is. An instance naming an entry past the end of `meshes` is not
/// visible either — the shader has no such guard, because a bind group's buffer
/// bound whole is exactly the table and reading past it is a validation error
/// the caller must not create.
///
/// The order is ascending. **The GPU's is not**, because slots come from an
/// atomic, so a comparison between the two sorts first.
#[must_use]
pub fn visible_instances(
    frustum: &Frustum,
    instances: &[GpuInstance],
    meshes: &[GpuMesh],
) -> Vec<u32> {
    let mut visible = Vec::new();
    for (index, instance) in instances.iter().enumerate() {
        if instance.flags & GpuInstance::LIVE == 0 {
            continue;
        }
        let Some(mesh) = meshes.get(instance.mesh as usize) else {
            continue;
        };
        if mesh.index_count == 0 {
            continue;
        }
        let bounds = Aabb {
            min: Vec3::from_array(mesh.bounds_min),
            max: Vec3::from_array(mesh.bounds_max),
        };
        let world = bounds.transformed(Mat4::from_cols_array(&instance.transform));
        if frustum.intersects(&world) {
            visible.push(u32::try_from(index).unwrap_or_else(|_| {
                unreachable!("an instance array is indexed by u32 everywhere else")
            }));
        }
    }
    visible
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Camera, Projection};
    use core::f32::consts::FRAC_PI_2;

    /// The camera every test below culls against: two metres back along +Z,
    /// looking at the origin, default 45° perspective.
    fn frustum() -> Frustum {
        Frustum::from_view_projection(Camera::default().view_projection(1.0))
    }

    /// A unit cube centred on the origin, as a mesh-table entry.
    fn unit_cube() -> GpuMesh {
        GpuMesh {
            base_vertex: 0,
            base_index: 0,
            index_count: 36,
            bounds_min: [-0.5, -0.5, -0.5],
            bounds_max: [0.5, 0.5, 0.5],
        }
    }

    /// A **live** instance of mesh 0 at `translation`.
    ///
    /// Live rather than default, because a default record is dead — see
    /// [`GpuInstance::LIVE`] — and a test scene built from dead instances would
    /// cull everything and assert nothing.
    fn at(translation: Vec3) -> GpuInstance {
        GpuInstance {
            transform: Mat4::from_translation(translation).to_cols_array(),
            flags: GpuInstance::LIVE,
            ..GpuInstance::default()
        }
    }

    #[test]
    fn a_box_at_the_origin_is_inside_the_default_camera() {
        let bounds = Aabb {
            min: Vec3::splat(-0.5),
            max: Vec3::splat(0.5),
        };
        assert!(frustum().intersects(&bounds));
    }

    /// One case per plane, each moved just past it, so a frustum missing a
    /// plane — or carrying the same plane twice — fails on the direction it
    /// stopped covering rather than passing everything.
    #[test]
    fn a_box_pushed_past_any_one_plane_is_outside() {
        let frustum = frustum();
        let unit = Aabb {
            min: Vec3::splat(-0.5),
            max: Vec3::splat(0.5),
        };
        // Right/left/top/bottom are far enough sideways to leave a 45° cone;
        // `behind` is past the eye at z = 2, which is the near plane's side.
        for (what, translation) in [
            ("right", Vec3::new(20.0, 0.0, 0.0)),
            ("left", Vec3::new(-20.0, 0.0, 0.0)),
            ("top", Vec3::new(0.0, 20.0, 0.0)),
            ("bottom", Vec3::new(0.0, -20.0, 0.0)),
            ("behind", Vec3::new(0.0, 0.0, 20.0)),
        ] {
            let moved = unit.transformed(Mat4::from_translation(translation));
            assert!(
                !frustum.intersects(&moved),
                "a box {what} of the frustum was not culled: {moved:?}"
            );
        }
    }

    /// The reversed-Z infinite projection has no far plane, and this is where
    /// that stops being prose. A normalization pass over these planes would
    /// divide this one by zero.
    #[test]
    fn the_infinite_far_plane_is_degenerate_and_rejects_nothing() {
        let frustum = frustum();
        // `z >= 0` — the far plane under reversed-Z; see
        // `Frustum::from_view_projection`.
        let far = frustum.planes[4];
        assert_eq!(far.truncate(), Vec3::ZERO, "{far:?}");
        assert!(far.w > 0.0, "so every point is on its inside: {far:?}");

        // Ten kilometres away along the view direction, and still visible.
        let distant = Aabb {
            min: Vec3::new(-0.5, -0.5, -10_000.5),
            max: Vec3::new(0.5, 0.5, -9_999.5),
        };
        assert!(frustum.intersects(&distant));

        // An orthographic camera has a finite far plane, so the same slot is a
        // real one there — which is what says the degeneracy is the projection's
        // and not this extraction's.
        let ortho = Frustum::from_view_projection(
            Camera::default()
                .with_projection(Projection::Orthographic {
                    half_height: 1.0,
                    near: 0.1,
                    far: 10.0,
                })
                .view_projection(1.0),
        );
        assert_ne!(ortho.planes[4].truncate(), Vec3::ZERO);
        assert!(!ortho.intersects(&distant));
    }

    /// The conservative transform, on the case that distinguishes it from
    /// transforming the two corners: a 45° rotation about Z grows a unit square
    /// by √2, and rotating `min` and `max` alone would leave it the same size.
    #[test]
    fn a_rotated_box_grows() {
        let unit = Aabb {
            min: Vec3::new(-0.5, -0.5, -0.5),
            max: Vec3::new(0.5, 0.5, 0.5),
        };
        let rotated = unit.transformed(Mat4::from_rotation_z(core::f32::consts::FRAC_PI_4));
        let half = rotated.half_extent();
        let expected = core::f32::consts::SQRT_2 / 2.0;
        assert!(
            (half.x - expected).abs() < 1e-5 && (half.y - expected).abs() < 1e-5,
            "a 45° rotation of a unit box has half-extent {expected} in x and y, got {half:?}"
        );
        assert!(
            (half.z - 0.5).abs() < 1e-6,
            "the rotation axis is untouched: {half:?}"
        );

        // A quarter turn is the case where the bound is exact rather than
        // conservative, so it must *not* grow — an implementation that added a
        // slack term would pass the assertion above and fail this one.
        let quarter = unit.transformed(Mat4::from_rotation_z(FRAC_PI_2));
        assert!(
            (quarter.half_extent() - Vec3::splat(0.5))
                .abs()
                .max_element()
                < 1e-6,
            "{:?}",
            quarter.half_extent()
        );
    }

    /// The rotation the conservative bound exists for, seen through the cull:
    /// a tall thin bar off to the right of the frustum, then laid on its side so
    /// it reaches back in. Same centre, same mesh, opposite answers — which is
    /// what an implementation that transformed only `min` and `max` would get
    /// wrong.
    #[test]
    fn a_rotation_can_bring_a_box_back_into_the_frustum() {
        let frustum = frustum();
        let bar = Aabb {
            min: Vec3::new(-0.02, -1.0, -0.02),
            max: Vec3::new(0.02, 1.0, 0.02),
        };
        // The default camera sees roughly |x| < 0.83 at z = 0, so a 4 cm wide
        // bar centred at x = 1.5 is outside; a metre of it either side is not.
        let offset = Vec3::new(1.5, 0.0, 0.0);
        let upright = bar.transformed(Mat4::from_translation(offset));
        assert!(!frustum.intersects(&upright), "{upright:?}");

        let laid_down =
            bar.transformed(Mat4::from_translation(offset) * Mat4::from_rotation_z(FRAC_PI_2));
        assert!(frustum.intersects(&laid_down), "{laid_down:?}");
    }

    /// **A removed instance is not visible**, whatever the frustum says about
    /// where it last was.
    ///
    /// The instance is at the origin, which the camera is pointed at, and it is
    /// the same record as the live one beside it in every respect but the bit —
    /// so the only thing that can be deciding this is the bit.
    #[test]
    fn an_instance_without_the_live_bit_is_not_visible() {
        let frustum = frustum();
        let meshes = [unit_cube()];
        let live = at(Vec3::ZERO);
        let removed = GpuInstance {
            flags: live.flags & !GpuInstance::LIVE,
            ..live
        };
        assert_eq!(
            visible_instances(&frustum, &[live, removed], &meshes),
            vec![0],
            "the live instance survives and its dead twin does not"
        );
        // And the other order, so this is the flag and not the index.
        assert_eq!(
            visible_instances(&frustum, &[removed, live], &meshes),
            vec![1]
        );
        // A record that is all zeroes is dead too — that is the slot a pool
        // hands out before anything writes it.
        assert!(
            visible_instances(&frustum, &[GpuInstance::default()], &meshes).is_empty(),
            "a default instance is not live"
        );
    }

    /// The reference's other two rejections, neither of which is the frustum
    /// test.
    #[test]
    fn an_instance_whose_mesh_entry_is_empty_is_not_visible() {
        let frustum = frustum();
        let meshes = [unit_cube(), GpuMesh::default()];
        let instances = [
            at(Vec3::ZERO),
            GpuInstance {
                mesh: 1,
                ..at(Vec3::ZERO)
            },
            GpuInstance {
                mesh: 7,
                ..at(Vec3::ZERO)
            },
        ];
        assert_eq!(visible_instances(&frustum, &instances, &meshes), vec![0]);
    }

    /// The list is the instances the frustum keeps, in ascending order, and it
    /// changes when the camera does — which is the assertion a cull test that
    /// never rejects anything would be missing.
    #[test]
    fn the_visible_set_follows_the_camera() {
        let meshes = [unit_cube()];
        let instances = [at(Vec3::ZERO), at(Vec3::new(6.0, 0.0, 0.0))];

        let ahead = Frustum::from_view_projection(Camera::default().view_projection(1.0));
        assert_eq!(visible_instances(&ahead, &instances, &meshes), vec![0]);

        // The same scene from a camera looking at the *other* cube: now it is
        // the one that survives and the origin is behind the eye.
        let turned = Camera {
            eye: Vec3::new(6.0, 0.0, 2.0),
            target: Vec3::new(6.0, 0.0, 0.0),
            ..Camera::default()
        };
        assert_eq!(
            visible_instances(
                &Frustum::from_view_projection(turned.view_projection(1.0)),
                &instances,
                &meshes
            ),
            vec![1]
        );
    }

    // -----------------------------------------------------------------------
    // §3.5's per-cluster cull
    // -----------------------------------------------------------------------

    /// A cluster ten units down `-Z` — straight ahead of [`frustum`]'s camera,
    /// which sits at `+Z` looking at the origin — with the cone `axis` and
    /// half-angle cosine `cutoff` the case wants.
    ///
    /// Ten units away and not one, so the sphere is small against the distance
    /// and every answer below is the *cone*'s rather than the frustum's.
    fn ahead(axis: Vec3, cutoff: f32, radius: f32) -> ClusterBounds {
        ClusterBounds {
            center: [0.0, 0.0, -10.0],
            radius,
            cone_axis: axis.normalize().to_array(),
            cone_cutoff: cutoff,
        }
    }

    /// The cull as [`ahead`]'s clusters see it: identity transform, and the
    /// camera [`frustum`] was built from.
    fn survives(bounds: &ClusterBounds) -> bool {
        cluster_survives_cull(&frustum(), Camera::default().eye, Mat4::IDENTITY, bounds)
    }

    /// A flat cluster whose normals point at the camera is drawn, and the same
    /// cluster turned around is not.
    ///
    /// The two answers are stated before the arithmetic: the camera is at `+Z`,
    /// so a `+Z` axis faces it and a `-Z` axis faces away, and a cluster of one
    /// flat face has `cone_cutoff == 1.0` and therefore no half-angle to soften
    /// either answer.
    #[test]
    fn a_cluster_is_kept_facing_the_camera_and_rejected_facing_away() {
        assert!(
            survives(&ahead(Vec3::Z, 1.0, 0.5)),
            "a face turned towards the camera is what the cull must never reject"
        );
        assert!(
            !survives(&ahead(Vec3::NEG_Z, 1.0, 0.5)),
            "a face turned away is the whole point of the cone test"
        );
    }

    /// **A cluster at [`ClusterBounds::OMNIDIRECTIONAL_CUTOFF`] is never
    /// rejected by the cone**, whatever its axis and whichever way it points.
    ///
    /// That value is what a closed shape's cancelled normals produce — the cube
    /// and the pyramid both carry it — so a cull that honoured the axis anyway
    /// would drop whole meshes. `OMNIDIRECTIONAL_AXIS` is a unit vector rather
    /// than a zero one precisely so that the arithmetic here still runs, which
    /// is why the axis being *meaningless* has to be caught by the cutoff test
    /// and by nothing else.
    #[test]
    fn an_omnidirectional_cluster_is_never_cone_culled() {
        for axis in [Vec3::Z, Vec3::NEG_Z, Vec3::X, Vec3::new(1.0, 2.0, -3.0)] {
            assert!(
                survives(&ahead(axis, ClusterBounds::OMNIDIRECTIONAL_CUTOFF, 0.5)),
                "a cone spanning every direction holds a front-facing normal for \
                 every view, and {axis} is one of them"
            );
        }
        // And a cone spanning exactly a hemisphere is the boundary of that, so
        // it is rejected by nothing either.
        assert!(survives(&ahead(Vec3::NEG_Z, 0.0, 0.5)));
    }

    /// **The radius term, which is the correction this rule needed.**
    ///
    /// Every number here is chosen so the answer can be stated before the code
    /// runs. The camera is at the origin's `+Z` side and the cluster's centre is
    /// ten units away, so `length(d)` is a little over ten; `cone_cutoff` is
    /// `0.6`, so `sqrt(1 - cutoff²)` is `0.8` and the radius-free rule rejects
    /// as soon as `dot(axis, d)` exceeds `8`. The axis is tilted to put that dot
    /// product at about `8.5` — past the radius-free threshold — and the radius
    /// is `2.0`, well past the `0.5` the correction needs to keep it.
    ///
    /// It **must** be kept: at a half-angle of `acos(0.6)` and a centre-to-axis
    /// angle of `acos(0.85)` the cone's leading edge is still about five degrees
    /// off the perpendicular, so the cluster's near side carries triangles that
    /// face the camera. The radius-free form draws a hole there.
    #[test]
    fn a_cluster_the_point_form_would_reject_survives_the_conservative_one() {
        // dot(axis, center) = -10 * axis.z, so axis.z = -0.85 puts
        // dot(axis, d) at 8.5 once the camera's own +Z offset is folded in.
        let axis = Vec3::new(0.526_782_7, 0.0, -0.85);
        let cutoff = 0.6f32;
        let bounds = ahead(axis, cutoff, 2.0);

        // The radius-free rule, spelled out here rather than referenced, so
        // this test states what it is contrasting against.
        let camera = Camera::default().eye;
        let to_center = Vec3::from_array(bounds.center) - camera;
        let sine = (1.0 - cutoff * cutoff).sqrt();
        let projected = Vec3::from_array(bounds.cone_axis).dot(to_center);
        assert!(
            projected > sine * to_center.length(),
            "the point-form rule must reject this cluster, or the contrast below \
             is vacuous: {projected} against {}",
            sine * to_center.length()
        );

        assert!(
            survives(&bounds),
            "the conservative rule must keep it — the radius term is the whole \
             difference, and without it this cluster's front-facing triangles are \
             a hole in the picture"
        );

        // And the term is not simply "keep everything": the same cone at a
        // radius small enough to matter is rejected, which is what says the
        // radius is doing the work rather than the cutoff.
        assert!(!survives(&ahead(axis, cutoff, 0.1)));
    }

    /// The frustum half, against the sphere rather than a box.
    ///
    /// Behind the eye and far off to the side are the two rejections a
    /// per-cluster frustum test has to make; the same sphere in front of the
    /// camera is the anti-vacuity half.
    #[test]
    fn a_cluster_outside_the_frustum_is_rejected_whichever_way_it_faces() {
        let frustum = frustum();
        let camera = Camera::default().eye;
        // Facing the camera throughout, so nothing below can pass by the cone.
        let at = |center: [f32; 3]| ClusterBounds {
            center,
            radius: 0.5,
            cone_axis: Vec3::Z.to_array(),
            cone_cutoff: 1.0,
        };
        let survives = |bounds: &ClusterBounds| {
            cluster_survives_cull(&frustum, camera, Mat4::IDENTITY, bounds)
        };
        assert!(survives(&at([0.0, 0.0, -10.0])), "straight ahead");
        assert!(
            !survives(&at([0.0, 0.0, 20.0])),
            "twenty units behind the eye is past the near plane"
        );
        assert!(
            !survives(&at([40.0, 0.0, -10.0])),
            "forty units to the right of a 45° frustum ten units deep is outside it"
        );
    }

    /// The cull is in **world space**, so the instance transform is what puts a
    /// cluster there — and a rotation turns a cone round with it.
    ///
    /// Without the transform on the axis, a mesh authored facing `-Z` would be
    /// rejected however its instance is turned, which is a whole object missing
    /// from the frame for as long as it faces the camera.
    #[test]
    fn the_instance_transform_moves_the_sphere_and_turns_the_cone() {
        let frustum = frustum();
        let camera = Camera::default().eye;
        // In the mesh's own space this cluster sits at the origin facing -Z,
        // which from a +Z camera is away.
        let bounds = ClusterBounds {
            center: [0.0, 0.0, 0.0],
            radius: 0.5,
            cone_axis: [0.0, 0.0, -1.0],
            cone_cutoff: 1.0,
        };
        let back = Mat4::from_translation(Vec3::new(0.0, 0.0, -10.0));
        assert!(
            !cluster_survives_cull(&frustum, camera, back, &bounds),
            "translation alone leaves it facing away"
        );

        // Turned half a turn about Y, the same cluster faces the camera.
        let turned = back * Mat4::from_rotation_y(core::f32::consts::PI);
        assert!(
            cluster_survives_cull(&frustum, camera, turned, &bounds),
            "the cone axis must be transformed with the instance"
        );

        // And moved out of the frustum it is rejected wherever it points, which
        // is the half the translation is responsible for.
        let aside = Mat4::from_translation(Vec3::new(40.0, 0.0, -10.0))
            * Mat4::from_rotation_y(core::f32::consts::PI);
        assert!(!cluster_survives_cull(&frustum, camera, aside, &bounds));
    }

    /// **Which of the open box's five clusters survive from each of the three
    /// cameras `crcbl-vk`'s cluster-count end-to-end renders from**, worked out
    /// here where the answer is arithmetic rather than a number read out of a
    /// buffer.
    ///
    /// The GPU can only report a *total*, so a count on its own says nothing
    /// about which clusters went. This says which, on the same geometry, the
    /// same instance transform and the same aspect ratio — so the numbers that
    /// test asserts have a statement behind them and a change to either side
    /// shows up without a GPU.
    ///
    /// The three cases are the three the slice owes:
    ///
    /// * **Nothing rejected.** The box's faces point *inward*, so a camera
    ///   above it and between its walls sees every one of them front-on. This
    ///   is what an over-eager cone test fails, and the case a picture cannot
    ///   show — a cluster wrongly rejected here draws nothing, and the wall it
    ///   would have drawn is behind the camera's own viewpoint anyway.
    /// * **The cone rejects two.** From the golden's camera, out beyond `+X`
    ///   and `+Z`, the two walls carrying those inward normals are seen from
    ///   behind.
    /// * **The frustum rejects two.** From inside the box looking level at the
    ///   `-Z` wall, the floor's bounding sphere is entirely below the bottom
    ///   plane and the `+Z` wall's is entirely behind the eye — and every
    ///   cluster still faces the camera, so nothing here is the cone's doing.
    #[test]
    fn the_open_box_loses_the_clusters_each_of_the_three_test_cameras_hides() {
        let clusters = crcbl_shaders::meshlet::open_box_clusters();
        assert_eq!(
            clusters.clusters.len(),
            crcbl_shaders::mesh::OPEN_BOX_FACES.len(),
            "one cluster per face is what makes the counts below mean anything"
        );
        // Where `crcbl-vk`'s mesh suite puts the box, and the aspect its 256×192
        // frame has. Both matter: the frustum answers below are this shape's.
        let at = Vec3::new(-2.0, 0.0, 0.0);
        let transform = Mat4::from_translation(at);
        let aspect = 256.0 / 192.0;

        let kept = |eye: Vec3, target: Vec3| -> Vec<&'static str> {
            let look = Camera {
                eye,
                target,
                ..Camera::default()
            };
            let frustum = Frustum::from_view_projection(look.view_projection(aspect));
            clusters
                .clusters
                .iter()
                .zip(crcbl_shaders::mesh::OPEN_BOX_FACES)
                .filter(|(cluster, _)| {
                    cluster_survives_cull(&frustum, eye, transform, &cluster.bounds)
                })
                .map(|(_, face)| face.name)
                .collect()
        };

        assert_eq!(
            kept(at + Vec3::new(0.2, 3.2, 0.3), at).len(),
            crcbl_shaders::mesh::OPEN_BOX_FACES.len(),
            "from above and between the walls every face is front-facing, so a \
             correct cull rejects none of them"
        );
        assert_eq!(
            kept(Vec3::new(1.6, 1.2, 2.2) * 1.7, Vec3::ZERO),
            vec!["floor", "-X wall", "-Z wall"],
            "the two walls whose inward normals point away from the golden's \
             camera are the ones the cone must reject, and no others"
        );
        assert_eq!(
            kept(
                at + Vec3::new(0.0, 0.4, -0.3),
                at + Vec3::new(0.0, 0.4, -1.0)
            ),
            vec!["-X wall", "+X wall", "-Z wall"],
            "from inside, looking level, the floor is under the frame and the +Z \
             wall is behind the eye"
        );
    }

    /// **The radius term, on the real mesh and from the camera the end-to-end
    /// renders it from.**
    ///
    /// [`a_cluster_the_point_form_would_reject_survives_the_conservative_one`]
    /// is the correctness case — a partial cone whose near side genuinely faces
    /// the camera — and it is a constructed one, because **no mesh this engine
    /// ships has a `cone_cutoff` strictly between zero and one**: a flat face
    /// gets `1.0` and a closed shape gets
    /// [`ClusterBounds::OMNIDIRECTIONAL_CUTOFF`]. At a cutoff of exactly `1.0`
    /// the two forms differ only where the camera is already past the face's own
    /// plane, so what the term buys on *this* geometry is slack rather than a
    /// pixel.
    ///
    /// It is still worth pinning, and this is why: this is the only place the
    /// radius term reaches a number the GPU produces, so without it a shader
    /// that dropped `+ radius` would pass every count in `crcbl-vk`'s suite. The
    /// camera is above the box and just outside two of its walls, so those two
    /// are the pair the forms disagree about — five kept against three.
    #[test]
    fn the_radius_term_keeps_two_clusters_the_point_form_drops() {
        let clusters = crcbl_shaders::meshlet::open_box_clusters();
        let at = Vec3::new(-2.0, 0.0, 0.0);
        let transform = Mat4::from_translation(at);
        let eye = at + Vec3::new(0.8, 3.0, 0.8);
        let look = Camera {
            eye,
            target: at,
            ..Camera::default()
        };
        let frustum = Frustum::from_view_projection(look.view_projection(256.0 / 192.0));

        // The rule as documented, which is what `cluster_survives_cull` runs.
        let conservative = clusters
            .clusters
            .iter()
            .filter(|cluster| cluster_survives_cull(&frustum, eye, transform, &cluster.bounds))
            .count();
        assert_eq!(
            conservative,
            crcbl_shaders::mesh::OPEN_BOX_FACES.len(),
            "the documented rule keeps every cluster from here"
        );

        // And the radius-free form, written out so this states what it is
        // contrasting against rather than referring to it.
        let point_form = clusters
            .clusters
            .iter()
            .filter(|cluster| {
                let bounds = &cluster.bounds;
                let center = transform.transform_point3(Vec3::from_array(bounds.center));
                if !frustum.intersects_sphere(center, bounds.radius) {
                    return false;
                }
                let axis = Mat3::from_mat4(transform) * Vec3::from_array(bounds.cone_axis);
                let to_center = center - eye;
                let sine = (1.0 - bounds.cone_cutoff * bounds.cone_cutoff)
                    .max(0.0)
                    .sqrt();
                !(bounds.cone_cutoff > 0.0 && axis.dot(to_center) > sine * to_center.length())
            })
            .count();
        assert_eq!(
            point_form, 3,
            "the radius-free form drops the two walls whose planes this camera has \
             crossed, which is the difference the term exists to make"
        );
    }

    // -----------------------------------------------------------------------
    // A cluster's sphere under a scaled instance
    // -----------------------------------------------------------------------

    /// `crcbl::screenshot`'s trough: the open box scaled into a long narrow room
    /// and lifted so its floor is the plane `y = 0`.
    ///
    /// The engine's own non-rigid instance, copied here rather than referred to
    /// because `crcbl-render` cannot depend on `crcbl` — and the numbers are what
    /// the two tests below are about, so a trough of some other size would not be
    /// evidence about the scene that found this.
    fn trough(offset: Vec3) -> Mat4 {
        Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0) + offset)
            * Mat4::from_scale(Vec3::new(6.0, 2.0, 1.6))
    }

    /// The camera that scene is drawn with: straight down into the trough from
    /// just above its walls, `+Z` up because the view direction is `Y`.
    fn trough_camera() -> Camera {
        Camera {
            eye: Vec3::new(0.0, 2.2, 0.0),
            target: Vec3::ZERO,
            up: Vec3::Z,
            projection: Projection::Perspective {
                fov_y: core::f32::consts::FRAC_PI_3,
                near: 0.01,
            },
        }
    }

    /// **A cluster's bounding sphere has to bound the cluster after the instance
    /// transform, and a scaled instance is where that stops being free.**
    ///
    /// [`ClusterBounds::radius`] is a distance in the *mesh's* own space. The
    /// cull needs one in world space, and the two are the same number only for a
    /// transform that preserves length. This walks the open box's real vertices
    /// through the trough's transform and measures how far each cluster's
    /// furthest one actually gets from its transformed centre.
    ///
    /// Both halves are asserted, and the second is what makes the first mean
    /// something:
    ///
    /// * the scaled radius **contains** every vertex, which is the property a
    ///   bounding sphere is; and
    /// * the unscaled radius **does not** — by more than a factor of four on the
    ///   floor — so a test that passed with the scaling removed would be
    ///   asserting nothing.
    ///
    /// A wrongly small sphere is not a slightly worse cull. It is a cluster that
    /// covers half the frame reported as entirely outside a plane it straddles,
    /// which is the frame `docs/backlog.md` recorded as coming back clear.
    #[test]
    fn a_scaled_instances_cluster_sphere_still_contains_its_geometry() {
        let clusters = crcbl_shaders::meshlet::open_box_clusters();
        let vertices = crcbl_shaders::mesh::open_box_vertices();
        let transform = trough(Vec3::ZERO);
        let stretch = max_stretch(Mat3::from_mat4(transform));
        assert_eq!(
            stretch, 6.0,
            "the trough's longest axis is its run, and the bound is exact for an \
             axis-aligned scale"
        );

        let mut escaped_the_local_radius = 0usize;
        for cluster in &clusters.clusters {
            let bounds = &cluster.bounds;
            let center = transform.transform_point3(Vec3::from_array(bounds.center));
            let run = cluster.vertex_offset as usize;
            let furthest = clusters.vertices[run..run + cluster.vertex_count as usize]
                .iter()
                .map(|&vertex| {
                    let position = vertices[vertex as usize].position;
                    let world = transform.transform_point3(Vec3::new(
                        position[0],
                        position[1],
                        position[2],
                    ));
                    (world - center).length()
                })
                .fold(0.0f32, f32::max);
            assert!(
                furthest <= bounds.radius * stretch,
                "the scaled radius must bound the transformed cluster: {furthest} \
                 reached against {}",
                bounds.radius * stretch
            );
            if furthest > bounds.radius {
                escaped_the_local_radius += 1;
            }
        }
        assert_eq!(
            escaped_the_local_radius,
            clusters.clusters.len(),
            "every cluster of a trough this size outgrows its mesh-space radius, so \
             a cull carrying that radius across is testing a sphere that contains \
             none of them"
        );
    }

    /// **The trough slid along its own run keeps the floor that is still on
    /// screen** — the defect `docs/backlog.md` recorded as "a large open box
    /// offset laterally from the camera stops drawing entirely".
    ///
    /// Three units along `+X`, the trough spans `x = 0..6` and the camera looks
    /// straight down at the origin, so the near half of its floor fills a good
    /// part of the frame — which this states by projecting the floor cluster's
    /// own vertices rather than by asserting it. The cull must therefore keep
    /// that cluster, and with a mesh-space radius it does not: `0.71` against the
    /// `3.10` the floor actually reaches, which puts its whole sphere outside the
    /// left plane.
    ///
    /// The instance cull one layer up keeps this instance from every offset here
    /// — [`visible_instances`] works on the mesh's *box*, which the transform
    /// scales correctly — so nothing before the amplification stage notices, and
    /// the frame comes back as clear colour with the counters reporting a draw.
    #[test]
    fn a_scaled_trough_slid_across_the_camera_keeps_the_floor_it_still_shows() {
        let clusters = crcbl_shaders::meshlet::open_box_clusters();
        let vertices = crcbl_shaders::mesh::open_box_vertices();
        let camera = trough_camera();
        let view_projection = camera.view_projection(256.0 / 192.0);
        let frustum = Frustum::from_view_projection(view_projection);
        let floor = &clusters.clusters[0];
        assert_eq!(
            crcbl_shaders::mesh::OPEN_BOX_FACES[0].name,
            "floor",
            "cluster zero is the face this test is about"
        );

        for offset in [2.6f32, 3.0, 4.0] {
            let transform = trough(Vec3::X * offset);
            // The floor is on screen, and this is the measurement that says so:
            // at least one of its vertices lands inside the unit clip box in
            // front of the eye.
            let run = floor.vertex_offset as usize;
            let on_screen = clusters.vertices[run..run + floor.vertex_count as usize]
                .iter()
                .filter(|&&vertex| {
                    let position = vertices[vertex as usize].position;
                    let clip = view_projection
                        * (transform
                            * Vec3::new(position[0], position[1], position[2]).extend(1.0));
                    clip.w > 0.0
                        && (clip.x / clip.w).abs() <= 1.0
                        && (clip.y / clip.w).abs() <= 1.0
                        && clip.z / clip.w >= 0.0
                })
                .count();
            assert!(
                on_screen > 0,
                "the trough {offset} units along its run must still show floor, or \
                 there is nothing here for a cull to wrongly reject"
            );
            assert!(
                cluster_survives_cull(&frustum, camera.eye, transform, &floor.bounds),
                "the floor puts {on_screen} of its vertices in the frame from {offset} \
                 units out and the cull rejected it — that is the box that stops \
                 drawing"
            );
        }

        // And the cull still rejects: the same trough far enough out is gone, so
        // the assertions above are not satisfied by a rule that keeps everything.
        let away = trough(Vec3::X * 40.0);
        assert!(
            !clusters
                .clusters
                .iter()
                .any(|cluster| cluster_survives_cull(&frustum, camera.eye, away, &cluster.bounds)),
            "forty units out the whole trough is outside the frustum, radius and all"
        );
    }
}
