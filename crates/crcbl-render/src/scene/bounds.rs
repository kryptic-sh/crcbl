//! CPU-side bounds of a set of [`InstanceDesc`]s over a [`SceneDesc`]'s
//! meshes: one box around the lot under a root transform, or one box per
//! instance for a rigid compound.
//!
//! # Every vertex, not a transformed box
//!
//! Each box is folded from the mesh's vertex positions after the transform, so
//! it is the tightest axis-aligned box around the placed geometry. Transforming
//! the mesh's own box instead — [`Aabb::transformed`], the absolute-value
//! matrix method a cull uses — is cheaper and contains the geometry, but is
//! strictly larger under any rotation that is not a quarter turn: an object
//! placed on a surface by its box's lowest point would float above it.
//!
//! # A non-finite position is refused, not skipped
//!
//! [`Aabb::from_points`] skips a `NaN` lane on purpose, because a cull box must
//! keep containing the finite geometry. A placement or a collision part has the
//! opposite need: a box that silently leaves a vertex out is wrong without
//! saying so. So every placed position is checked and the first one that is
//! `NaN` or infinite fails the call with [`SceneBoundsError::NonFinitePosition`],
//! naming the instance — whether the vertex, the instance transform or the root
//! was the poisoned input, and including a finite product that overflowed.
//!
//! # `f32` here, `f64` in physics
//!
//! The boxes are this crate's `f32` [`Aabb`], in the precision the vertices and
//! [`InstanceDesc::transform`] are stored in. `crcbl_phys::AabbCompound` takes
//! `f64` boxes; the widening is the caller's, one `as_dvec3()` per corner, and
//! is exact, since every `f32` is an `f64`.

use glam::{Mat4, Vec3};

use super::{Geometry, InstanceDesc, SceneDesc};
use crate::cull::Aabb;
use crate::mesh_pool::vertex_positions;
use crcbl_shaders::mesh::VERTEX_STRIDE;

/// Why [`SceneDesc::instance_bounds`] or [`SceneDesc::instance_parts`] refused
/// a set of instances. Each variant that concerns one instance names its index
/// in the slice the call was given.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SceneBoundsError {
    /// The instance names a mesh past the end of [`SceneDesc::meshes`].
    #[error("instance {instance} names mesh {mesh}, which the scene does not have")]
    MissingMesh {
        /// The instance's index.
        instance: usize,
        /// The mesh it names.
        mesh: usize,
    },
    /// The instance's mesh is a [`Geometry::Dag`] with no levels, so it has no
    /// finest level to bound.
    #[error("instance {instance} names mesh {mesh}, a cluster DAG with no levels")]
    NoLevels {
        /// The instance's index.
        instance: usize,
        /// The mesh it names.
        mesh: usize,
    },
    /// The mesh's vertex bytes are not a whole number of vertices.
    #[error("instance {instance} names mesh {mesh}, whose vertex bytes end in a partial vertex")]
    PartialVertex {
        /// The instance's index.
        instance: usize,
        /// The mesh it names.
        mesh: usize,
    },
    /// A vertex of the instance's mesh, placed by the instance and root
    /// transforms, has a `NaN` or infinite coordinate.
    #[error("instance {instance}'s vertex {vertex} is not finite once placed")]
    NonFinitePosition {
        /// The instance's index.
        instance: usize,
        /// The vertex's index in its mesh (level 0's, for a DAG).
        vertex: usize,
    },
    /// The instance's mesh has no vertices, so there is no part to bound.
    #[error("instance {instance}'s mesh has no vertices")]
    EmptyPart {
        /// The instance's index.
        instance: usize,
    },
    /// No instance contributed a vertex — the set is empty, or every mesh in
    /// it is.
    #[error("the instances have no vertices to bound")]
    Empty,
}

impl SceneDesc<'_> {
    /// The box around every vertex of `instances`, each placed by
    /// `root * instance.transform`.
    ///
    /// Folded from the placed vertices, so it is the tightest axis-aligned box
    /// around them; [`Aabb::transformed`] of the mesh's own box would contain
    /// them too, but is larger under any rotation that is not a quarter turn.
    /// A [`Geometry::Dag`] is bounded by level 0, its finest. An instance whose
    /// mesh has no vertices contributes nothing.
    ///
    /// A placed vertex that is `NaN` or infinite fails the call — from a bad
    /// vertex, a bad instance transform or root, or a finite product that
    /// overflowed. [`Aabb::from_points`] would skip a `NaN`, which a cull box
    /// wants and a placement or collision box does not.
    ///
    /// # Errors
    ///
    /// The first [`SceneBoundsError`] an instance meets, in slice order, and
    /// [`SceneBoundsError::Empty`] when no instance has a vertex.
    pub fn instance_bounds(
        &self,
        instances: &[InstanceDesc],
        root: Mat4,
    ) -> Result<Aabb, SceneBoundsError> {
        let mut bounds: Option<Aabb> = None;
        for (index, instance) in instances.iter().enumerate() {
            if let Some(placed) = self.placed_bounds(index, instance, root)? {
                bounds = Some(match bounds {
                    Some(so_far) => Aabb {
                        min: so_far.min.min(placed.min),
                        max: so_far.max.max(placed.max),
                    },
                    None => placed,
                });
            }
        }
        bounds.ok_or(SceneBoundsError::Empty)
    }

    /// One box per instance, in slice order, each around its mesh's vertices
    /// placed by [`InstanceDesc::transform`] alone — the parts of a rigid
    /// compound in the frame the instance transforms are written in.
    ///
    /// Widened to `f64`, they are what `crcbl_phys::AabbCompound::new` takes:
    /// every box is finite and none is inverted. A flat part (an axis where
    /// `min == max`, from planar geometry or a zero scale) is returned as it
    /// is; a caller wanting a minimum thickness pads it.
    ///
    /// # Errors
    ///
    /// The first [`SceneBoundsError`] an instance meets, in slice order,
    /// including [`SceneBoundsError::EmptyPart`] for a mesh with no vertices.
    /// An empty slice is not an error: it yields no parts.
    pub fn instance_parts(
        &self,
        instances: &[InstanceDesc],
    ) -> Result<Vec<Aabb>, SceneBoundsError> {
        instances
            .iter()
            .enumerate()
            .map(|(index, instance)| {
                self.placed_bounds(index, instance, Mat4::IDENTITY)?
                    .ok_or(SceneBoundsError::EmptyPart { instance: index })
            })
            .collect()
    }

    /// The box around `instance`'s vertices placed by
    /// `root * instance.transform`, or `None` when its mesh has none. `index`
    /// is only what an error names.
    fn placed_bounds(
        &self,
        index: usize,
        instance: &InstanceDesc,
        root: Mat4,
    ) -> Result<Option<Aabb>, SceneBoundsError> {
        let mesh = instance.mesh;
        let geometry = &self
            .meshes
            .get(mesh)
            .ok_or(SceneBoundsError::MissingMesh {
                instance: index,
                mesh,
            })?
            .geometry;
        let vertices = match geometry {
            Geometry::Flat { vertices, .. } => vertices.as_ref(),
            Geometry::Dag { levels, .. } => levels
                .first()
                .ok_or(SceneBoundsError::NoLevels {
                    instance: index,
                    mesh,
                })?
                .as_ref(),
        };
        if vertices.len() % VERTEX_STRIDE != 0 {
            return Err(SceneBoundsError::PartialVertex {
                instance: index,
                mesh,
            });
        }
        let transform = root * instance.transform;
        let mut bounds: Option<Aabb> = None;
        for (vertex, position) in vertex_positions(vertices).enumerate() {
            let point: Vec3 = transform.transform_point3(position);
            if !point.is_finite() {
                return Err(SceneBoundsError::NonFinitePosition {
                    instance: index,
                    vertex,
                });
            }
            // Every point is finite, so `Vec3::min`/`max` cannot meet the `NaN`
            // that makes them unsafe in `Aabb::from_points`.
            bounds = Some(match bounds {
                Some(so_far) => Aabb {
                    min: so_far.min.min(point),
                    max: so_far.max.max(point),
                },
                None => Aabb {
                    min: point,
                    max: point,
                },
            });
        }
        Ok(bounds)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::f32::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_4};

    use crcbl_phys::{AabbCompound, Ray, Transform};
    use crcbl_shaders::mesh::{self, MeshVertex};
    use crcbl_shaders::meshlet::MeshClusters;
    use crcbl_shaders::vertex::UvRange;
    use glam::{DQuat, DVec3};

    use super::*;
    use crate::scene::{DEMO_CUBE, MeshDesc, demo};

    fn cube(transform: Mat4) -> InstanceDesc {
        InstanceDesc {
            mesh: DEMO_CUBE,
            material: 0,
            transform,
        }
    }

    /// The demo scene with one more flat mesh holding `positions`, and that
    /// mesh's index.
    fn with_mesh(positions: &[[f32; 3]]) -> (SceneDesc<'static>, usize) {
        let vertices: Vec<MeshVertex> = positions
            .iter()
            .map(|&position| {
                MeshVertex::from_normal(
                    position,
                    [0.0, 1.0, 0.0],
                    [1.0; 4],
                    [0.0; 2],
                    &UvRange::default(),
                )
            })
            .collect();
        let mut scene = demo();
        scene.meshes.push(MeshDesc {
            label: Cow::Borrowed("test mesh"),
            geometry: Geometry::Flat {
                vertices: Cow::Owned(mesh::vertex_bytes(&vertices)),
                uv_range: UvRange::default(),
                indices: Cow::Owned(Vec::new()),
                clusters: MeshClusters::default(),
                flags: 0,
            },
        });
        let index = scene.meshes.len() - 1;
        (scene, index)
    }

    fn flat_vertices<'s>(scene: &'s mut SceneDesc<'_>, mesh: usize) -> &'s mut Vec<u8> {
        let Geometry::Flat { vertices, .. } = &mut scene.meshes[mesh].geometry else {
            panic!("mesh {mesh} is flat geometry");
        };
        vertices.to_mut()
    }

    fn assert_close(actual: Vec3, expected: Vec3) {
        assert!(
            actual.abs_diff_eq(expected, 1e-6),
            "{actual} is not {expected}"
        );
    }

    /// An octahedron with its tips on the axes, turned an eighth about Z: its
    /// tips go to `(±√½, ±√½, 0)`, so the box reaches `√½` on X and Y. The
    /// absolute-value box of the unturned `[-1, 1]` box reaches `√2`, which is
    /// what the second assertion tells apart.
    #[test]
    fn a_turned_instance_is_bounded_by_its_vertices() {
        let (scene, octahedron) = with_mesh(&[
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ]);
        let place = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let instance = InstanceDesc {
            mesh: octahedron,
            material: 0,
            transform: Mat4::from_rotation_z(FRAC_PI_4),
        };
        let bounds = scene
            .instance_bounds(&[instance], place)
            .expect("finite geometry");
        assert_close(
            bounds.min,
            Vec3::new(1.0 - FRAC_1_SQRT_2, 2.0 - FRAC_1_SQRT_2, 2.0),
        );
        assert_close(
            bounds.max,
            Vec3::new(1.0 + FRAC_1_SQRT_2, 2.0 + FRAC_1_SQRT_2, 4.0),
        );

        let loose = Aabb {
            min: Vec3::splat(-1.0),
            max: Vec3::ONE,
        }
        .transformed(place * instance.transform);
        assert!(
            loose.max.x > bounds.max.x + 0.5,
            "the turned box of the mesh's box is {loose:?}, not the vertex bound"
        );
    }

    #[test]
    fn several_instances_are_bounded_together() {
        let scene = demo();
        let bounds = scene
            .instance_bounds(
                &[
                    cube(Mat4::from_translation(Vec3::new(-2.0, 0.0, 0.0))),
                    cube(
                        Mat4::from_translation(Vec3::new(3.0, 4.0, 5.0))
                            * Mat4::from_scale(Vec3::new(2.0, 1.0, 3.0)),
                    ),
                ],
                Mat4::IDENTITY,
            )
            .expect("two cubes");
        assert_eq!(bounds.min, Vec3::new(-2.5, -0.5, -0.5));
        assert_eq!(bounds.max, Vec3::new(4.0, 4.5, 6.5));
    }

    /// A `NaN` in the second instance's mesh, at a vertex after finite ones, is
    /// refused by name on both entry points — where [`Aabb::from_points`] would
    /// have skipped it and returned a finite box.
    #[test]
    fn a_nan_vertex_is_refused_and_names_its_instance() {
        let (mut scene, poisoned) = with_mesh(&[[0.0; 3], [1.0; 3], [2.0; 3]]);
        let bytes = flat_vertices(&mut scene, poisoned);
        bytes[2 * mesh::VERTEX_STRIDE..2 * mesh::VERTEX_STRIDE + 4]
            .copy_from_slice(&f32::NAN.to_le_bytes());
        let instances = [
            cube(Mat4::IDENTITY),
            InstanceDesc {
                mesh: poisoned,
                material: 0,
                transform: Mat4::IDENTITY,
            },
        ];
        let expected = SceneBoundsError::NonFinitePosition {
            instance: 1,
            vertex: 2,
        };
        assert_eq!(
            scene.instance_bounds(&instances, Mat4::IDENTITY),
            Err(expected)
        );
        assert_eq!(scene.instance_parts(&instances), Err(expected));
        assert_eq!(
            expected.to_string(),
            "instance 1's vertex 2 is not finite once placed"
        );
    }

    #[test]
    fn a_non_finite_transform_is_refused() {
        let scene = demo();
        let nan = Mat4::from_translation(Vec3::new(f32::NAN, 0.0, 0.0));
        let expected = SceneBoundsError::NonFinitePosition {
            instance: 0,
            vertex: 0,
        };
        assert_eq!(
            scene.instance_bounds(&[cube(nan)], Mat4::IDENTITY),
            Err(expected)
        );
        assert_eq!(
            scene.instance_bounds(&[cube(Mat4::IDENTITY)], nan),
            Err(expected)
        );
        assert_eq!(scene.instance_parts(&[cube(nan)]), Err(expected));
        // Finite inputs whose product overflows.
        let huge = Mat4::from_scale(Vec3::splat(f32::MAX));
        assert_eq!(
            scene.instance_bounds(&[cube(huge)], Mat4::from_scale(Vec3::splat(4.0))),
            Err(expected)
        );
    }

    #[test]
    fn an_empty_set_has_no_bounds_and_no_parts() {
        let scene = demo();
        assert_eq!(
            scene.instance_bounds(&[], Mat4::IDENTITY),
            Err(SceneBoundsError::Empty)
        );
        assert_eq!(scene.instance_parts(&[]), Ok(Vec::new()));
    }

    /// An instance of a mesh with no vertices is skipped by the set's bounds
    /// and refused as a part.
    #[test]
    fn an_empty_mesh_contributes_nothing_and_is_no_part() {
        let (scene, empty) = with_mesh(&[]);
        let hollow = InstanceDesc {
            mesh: empty,
            material: 0,
            transform: Mat4::IDENTITY,
        };
        assert_eq!(
            scene.instance_bounds(&[hollow], Mat4::IDENTITY),
            Err(SceneBoundsError::Empty)
        );
        assert_eq!(
            scene.instance_bounds(&[hollow, cube(Mat4::IDENTITY)], Mat4::IDENTITY),
            Ok(Aabb {
                min: Vec3::splat(-0.5),
                max: Vec3::splat(0.5),
            })
        );
        assert_eq!(
            scene.instance_parts(&[cube(Mat4::IDENTITY), hollow]),
            Err(SceneBoundsError::EmptyPart { instance: 1 })
        );
    }

    /// The parts, widened, are accepted by `AabbCompound::new`, and a ray cast
    /// at a turned pose strikes the part it is aimed at, at the face the
    /// instance transform put there.
    #[test]
    fn the_parts_build_a_compound_a_ray_strikes() {
        let scene = demo();
        let parts = scene
            .instance_parts(&[
                cube(Mat4::IDENTITY),
                cube(Mat4::from_translation(Vec3::new(3.0, 0.0, 0.0))),
            ])
            .expect("two cubes");
        let widened: Vec<crcbl_phys::Aabb> = parts
            .iter()
            .map(|part| crcbl_phys::Aabb::new(part.min.as_dvec3(), part.max.as_dvec3()))
            .collect();
        let compound = AabbCompound::new(&widened).expect("finite, non-inverted parts");
        // A quarter turn about Z takes the second cube's local +X offset to
        // world +Y, so its local -X face at x = 2.5 is the world plane y = 12.5.
        let pose = Transform::new(
            DVec3::new(0.0, 10.0, 0.0),
            DQuat::from_rotation_z(core::f64::consts::FRAC_PI_2),
        );
        let hit = compound
            .ray_cast(
                &pose,
                &Ray::new(DVec3::new(0.0, 11.75, 0.0), DVec3::new(0.0, 1.0, 0.0)),
            )
            .expect("the ray starts between the parts and runs into the second");
        assert_eq!(hit.part, 1);
        assert!((hit.hit.point - DVec3::new(0.0, 12.5, 0.0)).length() < 1e-9);
        assert!((hit.hit.t - 0.75).abs() < 1e-9);
    }

    // Ported from EW's `asset_placement` tests, with EW's expected values.

    #[test]
    fn missing_or_malformed_mesh_geometry_is_refused() {
        let scene = demo();
        let missing = InstanceDesc {
            mesh: scene.meshes.len(),
            ..cube(Mat4::IDENTITY)
        };
        assert_eq!(
            scene.instance_bounds(&[missing], Mat4::IDENTITY),
            Err(SceneBoundsError::MissingMesh {
                instance: 0,
                mesh: scene.meshes.len(),
            })
        );

        let mut incomplete = demo();
        flat_vertices(&mut incomplete, DEMO_CUBE).pop();
        assert_eq!(
            incomplete.instance_bounds(&[cube(Mat4::IDENTITY)], Mat4::IDENTITY),
            Err(SceneBoundsError::PartialVertex {
                instance: 0,
                mesh: DEMO_CUBE,
            })
        );

        let mut nonfinite = demo();
        flat_vertices(&mut nonfinite, DEMO_CUBE)[..4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(
            nonfinite.instance_bounds(&[cube(Mat4::IDENTITY)], Mat4::IDENTITY),
            Err(SceneBoundsError::NonFinitePosition {
                instance: 0,
                vertex: 0,
            })
        );
    }

    #[test]
    fn a_mesh_with_no_vertices_alone_has_no_bounds() {
        let mut scene = demo();
        flat_vertices(&mut scene, DEMO_CUBE).clear();
        assert_eq!(
            scene.instance_bounds(&[cube(Mat4::IDENTITY)], Mat4::IDENTITY),
            Err(SceneBoundsError::Empty)
        );
    }

    #[test]
    fn a_dag_is_bounded_by_its_finest_level_and_transforms_compose() {
        let mut dag_scene = demo();
        let dag_mesh = dag_scene
            .meshes
            .iter()
            .position(|mesh| matches!(mesh.geometry, Geometry::Dag { .. }))
            .expect("the demo includes a DAG mesh");
        let instance = InstanceDesc {
            mesh: dag_mesh,
            material: 0,
            transform: Mat4::IDENTITY,
        };
        let expected = dag_scene
            .instance_bounds(&[instance], Mat4::IDENTITY)
            .expect("the dunes have vertices");
        {
            let Geometry::Dag { levels, .. } = &mut dag_scene.meshes[dag_mesh].geometry else {
                panic!("the selected demo mesh is a DAG");
            };
            assert!(levels.len() > 1);
            for vertex in levels[1].to_mut().chunks_exact_mut(mesh::VERTEX_STRIDE) {
                vertex[..12].copy_from_slice(
                    &[10_000.0_f32, 10_000.0, 10_000.0]
                        .map(f32::to_le_bytes)
                        .concat(),
                );
            }
        }
        assert_eq!(
            dag_scene.instance_bounds(&[instance], Mat4::IDENTITY),
            Ok(expected)
        );

        let Geometry::Dag { levels, .. } = &mut dag_scene.meshes[dag_mesh].geometry else {
            panic!("the selected demo mesh is a DAG");
        };
        levels.clear();
        assert_eq!(
            dag_scene.instance_bounds(&[instance], Mat4::IDENTITY),
            Err(SceneBoundsError::NoLevels {
                instance: 0,
                mesh: dag_mesh,
            })
        );

        let scene = demo();
        let root =
            Mat4::from_translation(Vec3::new(10.0, 20.0, 30.0)) * Mat4::from_rotation_z(FRAC_PI_2);
        let bounds = scene
            .instance_bounds(&[cube(Mat4::from_translation(Vec3::X * 2.0))], root)
            .expect("a cube");
        assert_eq!(bounds.min, Vec3::new(9.5, 21.5, 29.5));
        assert_eq!(bounds.max, Vec3::new(10.5, 22.5, 30.5));
    }

    /// EW's expected values, less EW's own `MIN_HALF_EXTENT_M` padding: the
    /// last two parts are flat boxes here, which `AabbCompound::new` accepts.
    #[test]
    fn parts_follow_each_instance_transform() {
        let scene = demo();
        let large_coordinate = f32::MAX * 0.75;
        let parts = scene
            .instance_parts(&[
                cube(Mat4::from_translation(Vec3::new(-2.0, 0.0, 0.0))),
                cube(
                    Mat4::from_translation(Vec3::new(3.0, 4.0, 5.0))
                        * Mat4::from_scale(Vec3::new(2.0, 1.0, 3.0)),
                ),
                cube(Mat4::from_translation(Vec3::splat(large_coordinate))),
                cube(
                    Mat4::from_translation(Vec3::new(7.0, 8.0, 9.0)) * Mat4::from_scale(Vec3::ZERO),
                ),
            ])
            .expect("four finite cubes");
        assert_eq!(
            parts,
            vec![
                Aabb {
                    min: Vec3::new(-2.5, -0.5, -0.5),
                    max: Vec3::new(-1.5, 0.5, 0.5),
                },
                Aabb {
                    min: Vec3::new(2.0, 3.5, 3.5),
                    max: Vec3::new(4.0, 4.5, 6.5),
                },
                Aabb {
                    min: Vec3::splat(large_coordinate),
                    max: Vec3::splat(large_coordinate),
                },
                Aabb {
                    min: Vec3::new(7.0, 8.0, 9.0),
                    max: Vec3::new(7.0, 8.0, 9.0),
                },
            ]
        );
    }
}
