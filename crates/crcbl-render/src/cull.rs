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
//! Nothing in the renderer consumes the visible list yet — [`crate::forward`]
//! records the same draws it always has. Indirect draw generation is the next
//! slice; this one buys the cull math and its proof.
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
}
