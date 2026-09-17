//! The stage: a floor under each room, every room's bodies and fixtures, a
//! camera per room and a sun.
//!
//! Every surface is a `crcbl::greybox` primitive scaled to the shape it
//! stands for: a one-metre cube to a box's extents, a one-metre ball to a
//! sphere's diameter, and a capsule as a cylinder between its core's ends with
//! a ball on each. An instance's transform is the physics state with nothing
//! in between, converted to render space's `f32` at the last moment.
//!
//! # Bodies come and go
//!
//! The wall drops and takes away bodies every second and the pit pours in a
//! thousand, so the instances are keyed by room and body: a body seen for the
//! first time gets instances, one gone since the last frame gives them back.

use std::borrow::Cow;
use std::collections::HashMap;

use crcbl::greybox::{GREYBOX_TILE_M, cube, cylinder, grid_material, grid_page, platform, sphere};
use crcbl::math::{DQuat, DVec3, Mat4, Quat, Vec3};
use crcbl::render::scene::{Capacities, Geometry, InstanceDesc, MeshDesc, ProbeGrid, SceneDesc};
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, InstanceHandle, InstancePoolError, Projection,
};
use crcbl::shaders::mesh::GpuMaterial;

use crate::scene::{Scenes, Shape, Tint, View};

/// How thick the floors are.
const FLOOR_THICKNESS_M: f32 = 0.3;

/// A one-metre slab, scaled per floor.
const FLOOR_MESH: usize = 0;
/// A one-metre cube, scaled per box.
const CUBE_MESH: usize = 1;
/// A one-metre ball, scaled per sphere.
const SPHERE_MESH: usize = 2;
/// A one-metre cylinder standing on its base, scaled per capsule.
const CYLINDER_MESH: usize = 3;
/// How many meshes the description declares.
const MESHES: usize = 4;

/// The floors' material row.
const FLOOR_MATERIAL: usize = 0;
/// How many rows the description declares: the floor's, then one per [`Tint`].
const MATERIALS: usize = 7;

/// Each room's floor: its centre on `y = 0`, and its width and depth.
const FLOORS: [([f32; 3], f32, f32); 3] = [
    ([0.0, 0.0, 0.0], 12.0, 12.0),
    ([12.0, 0.0, 0.0], 6.0, 3.0),
    ([26.0, 0.0, 0.0], 7.0, 7.0),
];

/// What this stage reserves. The pit's thousand balls are one instance each,
/// the wall's pills and pegs three, and the rest is headroom for the wall's
/// bodies turning over within a frame.
const CAPACITIES: Capacities = Capacities {
    vertices: 2048,
    indices: 8192,
    meshes: MESHES as u32,
    instances: 3072,
    materials: MATERIALS as u32,
    lights: 4,
    probes: 0,
};

/// A painted greybox material, tiled physically.
fn painted(tint: [f32; 3]) -> GpuMaterial {
    GpuMaterial {
        base_color: [tint[0], tint[1], tint[2], 1.0],
        tiling: GpuMaterial::TILING_PHYSICAL,
        tile_metres: GREYBOX_TILE_M,
        ..grid_material()
    }
}

/// The material row a tint is drawn with.
const fn material(tint: Tint) -> usize {
    match tint {
        Tint::Handle => 1,
        Tint::Box => 2,
        Tint::Ball => 3,
        Tint::Pill => 4,
        Tint::Peg => 5,
        Tint::Board => 6,
    }
}

/// Everything this stage makes resident.
#[must_use]
pub fn scene() -> SceneDesc<'static> {
    let mesh = |label: &'static str, geometry: Geometry<'static>| MeshDesc {
        label: Cow::Borrowed(label),
        geometry,
    };
    SceneDesc {
        meshes: vec![
            mesh("floor", platform(1.0, 1.0, 1.0)),
            mesh("cube", cube(1.0)),
            mesh("ball", sphere(0.5, 10, 16)),
            mesh("rod", cylinder(0.5, 1.0, 16)),
        ],
        materials: vec![
            painted([0.26, 0.27, 0.30]),
            painted([0.85, 0.45, 0.16]),
            painted([0.22, 0.46, 0.80]),
            painted([0.90, 0.78, 0.20]),
            painted([0.80, 0.28, 0.45]),
            painted([0.55, 0.58, 0.62]),
            painted([0.36, 0.30, 0.26]),
        ],
        page: grid_page(),
        probes: ProbeGrid::default(),
        capacities: CAPACITIES,
    }
}

/// `f64` rotation into render space.
#[allow(clippy::cast_possible_truncation)]
fn quat(rotation: DQuat) -> Quat {
    Quat::from_xyzw(
        rotation.x as f32,
        rotation.y as f32,
        rotation.z as f32,
        rotation.w as f32,
    )
    .normalize()
}

/// A shape as the instances that draw it: mesh and transform, one to three.
fn instances(shape: &Shape) -> ([(usize, Mat4); 3], usize, Tint) {
    let none = (0, Mat4::IDENTITY);
    match *shape {
        Shape::Box {
            centre,
            rotation,
            half,
            tint,
            ..
        } => (
            [
                (
                    CUBE_MESH,
                    Mat4::from_scale_rotation_translation(
                        (half * 2.0).as_vec3(),
                        quat(rotation),
                        centre.as_vec3(),
                    ),
                ),
                none,
                none,
            ],
            1,
            tint,
        ),
        Shape::Sphere {
            centre,
            radius,
            tint,
            ..
        } => ([(SPHERE_MESH, ball(centre, radius)), none, none], 1, tint),
        Shape::Capsule {
            a, b, radius, tint, ..
        } => {
            let axis = b - a;
            let length = axis.length();
            let turn = if length > 0.0 {
                let up = axis / length;
                let arc = DVec3::Y.cross(up);
                let w = 1.0 + DVec3::Y.dot(up);
                if w > 1e-9 {
                    DQuat::from_xyzw(arc.x, arc.y, arc.z, w).normalize()
                } else {
                    // Straight down: half a turn about X.
                    DQuat::from_xyzw(1.0, 0.0, 0.0, 0.0)
                }
            } else {
                DQuat::IDENTITY
            };
            let rod = Mat4::from_scale_rotation_translation(
                DVec3::new(2.0 * radius, length, 2.0 * radius).as_vec3(),
                quat(turn),
                a.as_vec3(),
            );
            (
                [
                    (CYLINDER_MESH, rod),
                    (SPHERE_MESH, ball(a, radius)),
                    (SPHERE_MESH, ball(b, radius)),
                ],
                3,
                tint,
            )
        }
    }
}

/// A ball of `radius` at `centre`.
fn ball(centre: DVec3, radius: f64) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        DVec3::splat(2.0 * radius).as_vec3(),
        Quat::IDENTITY,
        centre.as_vec3(),
    )
}

/// The instances the rooms' bodies are drawn with, by room and body.
#[derive(Debug, Default)]
pub struct Drawn {
    bodies: HashMap<(usize, u64), Placed>,
    /// Scratch the rooms write their shapes into, kept between frames.
    shapes: Vec<Shape>,
    frame: u64,
}

/// One body's instances, and the last frame it was seen in.
#[derive(Debug)]
struct Placed {
    handles: Vec<InstanceHandle>,
    seen: u64,
}

/// Makes the floors and every room's fixtures resident, and the bodies'
/// instances where `scenes` has them.
///
/// # Errors
///
/// [`InstancePoolError`] if this file's capacities do not cover what it places.
pub fn place(renderer: &mut ForwardRenderer, scenes: &Scenes) -> Result<Drawn, InstancePoolError> {
    for (centre, width, depth) in FLOORS {
        renderer.add_instance(&InstanceDesc {
            mesh: FLOOR_MESH,
            material: FLOOR_MATERIAL,
            transform: Mat4::from_scale_rotation_translation(
                Vec3::new(width, FLOOR_THICKNESS_M, depth),
                Quat::IDENTITY,
                Vec3::from(centre) - Vec3::new(0.0, FLOOR_THICKNESS_M, 0.0),
            ),
        })?;
    }
    let mut shapes = Vec::new();
    for room in scenes.rooms() {
        room.fixtures(&mut shapes);
    }
    for shape in &shapes {
        let (parts, count, tint) = instances(shape);
        for &(mesh, transform) in &parts[..count] {
            renderer.add_instance(&InstanceDesc {
                mesh,
                material: material(tint),
                transform,
            })?;
        }
    }
    let mut drawn = Drawn::default();
    drawn.update(renderer, scenes)?;
    Ok(drawn)
}

impl Drawn {
    /// Poses every body's instances where `scenes` has them now, adding
    /// instances for bodies new since the last frame and removing those of
    /// bodies gone.
    ///
    /// # Errors
    ///
    /// [`InstancePoolError`] if the pool cannot hold a new body.
    pub fn update(
        &mut self,
        renderer: &mut ForwardRenderer,
        scenes: &Scenes,
    ) -> Result<(), InstancePoolError> {
        self.frame += 1;
        let frame = self.frame;
        for (index, room) in scenes.rooms().into_iter().enumerate() {
            self.shapes.clear();
            room.bodies(&mut self.shapes);
            for shape in &self.shapes {
                let key = match *shape {
                    Shape::Box { key, .. }
                    | Shape::Sphere { key, .. }
                    | Shape::Capsule { key, .. } => key,
                };
                let (parts, count, tint) = instances(shape);
                let fresh = !matches!(
                    self.bodies.get(&(index, key)),
                    Some(placed) if placed.handles.len() == count
                );
                if fresh {
                    if let Some(stale) = self.bodies.remove(&(index, key)) {
                        for handle in stale.handles {
                            renderer.remove_instance(handle);
                        }
                    }
                    let mut handles = Vec::with_capacity(count);
                    for &(mesh, transform) in &parts[..count] {
                        handles.push(renderer.add_instance(&InstanceDesc {
                            mesh,
                            material: material(tint),
                            transform,
                        })?);
                    }
                    self.bodies
                        .insert((index, key), Placed { handles, seen: 0 });
                }
                let placed = self
                    .bodies
                    .get_mut(&(index, key))
                    .expect("placed above if it was not already");
                placed.seen = frame;
                for (&handle, &(mesh, transform)) in placed.handles.iter().zip(&parts[..count]) {
                    renderer.set_instance(
                        handle,
                        &InstanceDesc {
                            mesh,
                            material: material(tint),
                            transform,
                        },
                    );
                }
            }
        }
        self.bodies.retain(|_, placed| {
            let live = placed.seen == frame;
            if !live {
                for &handle in &placed.handles {
                    renderer.remove_instance(handle);
                }
            }
            live
        });
        Ok(())
    }
}

/// Where each room is seen from: fixed, square on to it, a little above.
#[must_use]
pub fn camera(view: View) -> Camera {
    let (eye, target) = match view {
        View::Spin => (Vec3::new(0.0, 3.2, 8.5), Vec3::new(0.0, 1.6, 0.0)),
        View::Wall => (Vec3::new(12.0, 3.0, 7.8), Vec3::new(12.0, 2.7, 0.0)),
        View::Pit => (Vec3::new(26.0, 4.2, 5.2), Vec3::new(26.0, 0.4, 0.0)),
    };
    Camera {
        eye,
        target,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// How high the sun sits, as the `Y` of a unit direction.
const SUN_ELEVATION: f32 = 0.72;

/// What lights the stage.
#[must_use]
pub fn sun() -> DirectionalLight {
    let flat = (1.0 - SUN_ELEVATION * SUN_ELEVATION).sqrt();
    DirectionalLight {
        direction: Vec3::new(flat * 0.6, SUN_ELEVATION, flat * 0.8),
        color: Vec3::new(1.0, 0.98, 0.92) * 1.5,
        ambient: Vec3::new(0.12, 0.13, 0.17),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capacities are exactly what the description declares.
    #[test]
    fn the_description_reserves_what_this_file_declares() {
        let scene = scene();
        assert_eq!(scene.materials.len(), MATERIALS);
        assert_eq!(scene.meshes.len(), CAPACITIES.meshes as usize);
    }

    /// Everything the rooms hold at their fullest fits the instance pool: the
    /// floors, every fixture, the pit's thousand balls and the wall's cap of
    /// bodies all as pills, with the wall's cap again for a frame in which
    /// every body turned over.
    #[test]
    fn the_rooms_at_their_fullest_fit_the_instance_pool() {
        let scenes = Scenes::new();
        let mut fixtures = Vec::new();
        for room in scenes.rooms() {
            room.fixtures(&mut fixtures);
        }
        let fixture_instances: usize = fixtures.iter().map(|s| instances(s).1).sum();
        let most = FLOORS.len()
            + fixture_instances
            + 3
            + crate::pit::BALLS as usize
            + 2 * 3 * crate::wall::MAX_LIVE;
        assert!(
            most <= CAPACITIES.instances as usize,
            "{most} instances against {}",
            CAPACITIES.instances
        );
    }

    /// A box instance is the box: a unit cube scaled to twice the
    /// half-extents, its centre where the box's is; and a capsule's rod runs
    /// from one end of its core to the other.
    #[test]
    fn an_instance_is_its_shape() {
        let (parts, count, _) = instances(&Shape::Box {
            key: 0,
            centre: DVec3::new(1.0, 2.0, 3.0),
            rotation: DQuat::IDENTITY,
            half: DVec3::new(0.5, 0.25, 1.0),
            tint: Tint::Box,
        });
        assert_eq!(count, 1);
        let corner = parts[0].1.transform_point3(Vec3::splat(0.5));
        assert_eq!(corner, Vec3::new(1.5, 2.25, 4.0));

        let (parts, count, _) = instances(&Shape::Capsule {
            key: 0,
            a: DVec3::new(1.0, 1.0, 0.0),
            b: DVec3::new(3.0, 1.0, 0.0),
            radius: 0.1,
            tint: Tint::Pill,
        });
        assert_eq!(count, 3);
        let top = parts[0].1.transform_point3(Vec3::new(0.0, 1.0, 0.0));
        assert!((top - Vec3::new(3.0, 1.0, 0.0)).length() < 1e-5, "{top:?}");
    }
}
