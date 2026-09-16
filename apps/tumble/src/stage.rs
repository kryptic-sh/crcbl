//! The stage: a floor, the two scenes' bodies as boxes, a fixed camera and a
//! sun.
//!
//! Every surface is a `crcbl::greybox` primitive. The bodies are one-metre
//! cubes scaled to their half-extents, so an instance's transform is the body's
//! pose with nothing in between — the picture is the physics state, converted
//! to render space's `f32` at the last moment.

use std::borrow::Cow;

use crcbl::greybox::{GREYBOX_TILE_M, cube, grid_material, grid_page, platform};
use crcbl::math::{DQuat, DVec3, Mat4, Quat, Vec3};
use crcbl::render::scene::{Capacities, Geometry, InstanceDesc, MeshDesc, ProbeGrid, SceneDesc};
use crcbl::render::{
    Camera, DirectionalLight, ForwardRenderer, InstanceHandle, InstancePoolError, Projection,
};
use crcbl::shaders::mesh::GpuMaterial;

use crate::scene::{BOX_HALF, Scenes};

/// How wide the floor is, in metres.
const FLOOR_M: f32 = 12.0;

/// How thick it is.
const FLOOR_THICKNESS_M: f32 = 0.3;

/// The floor slab.
const FLOOR_MESH: usize = 0;
/// A one-metre cube, scaled per body.
const CUBE_MESH: usize = 1;

/// The floor's material row.
const FLOOR_MATERIAL: usize = 0;
/// The T-handle's.
const HANDLE_MATERIAL: usize = 1;
/// The box's.
const BOX_MATERIAL: usize = 2;
/// How many rows the description declares.
const MATERIALS: usize = 3;

/// The floor, the handle's two parts and the box.
const INSTANCES: u32 = 4;

/// What this stage reserves: what it places and no more.
const CAPACITIES: Capacities = Capacities {
    vertices: 1024,
    indices: 2048,
    meshes: 2,
    instances: INSTANCES,
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

/// Everything this stage makes resident.
#[must_use]
pub fn scene() -> SceneDesc<'static> {
    let mesh = |label: &'static str, geometry: Geometry<'static>| MeshDesc {
        label: Cow::Borrowed(label),
        geometry,
    };
    SceneDesc {
        meshes: vec![
            mesh("floor", platform(FLOOR_M, FLOOR_M, FLOOR_THICKNESS_M)),
            mesh("cube", cube(1.0)),
        ],
        materials: vec![
            painted([0.26, 0.27, 0.30]),
            painted([0.85, 0.45, 0.16]),
            painted([0.22, 0.46, 0.80]),
        ],
        page: grid_page(),
        probes: ProbeGrid::default(),
        capacities: CAPACITIES,
    }
}

/// The instances the bodies are drawn with.
#[derive(Debug)]
pub struct Drawn {
    bar: InstanceHandle,
    stem: InstanceHandle,
    crate_: InstanceHandle,
}

/// A body's box as an instance transform: scaled to its full extents, turned
/// and placed.
#[allow(clippy::cast_possible_truncation)]
fn transform(centre: DVec3, rotation: DQuat, half: DVec3) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        (half * 2.0).as_vec3(),
        Quat::from_xyzw(
            rotation.x as f32,
            rotation.y as f32,
            rotation.z as f32,
            rotation.w as f32,
        )
        .normalize(),
        centre.as_vec3(),
    )
}

/// Makes the floor resident and the three body instances, posed where
/// `scenes` has them.
///
/// # Errors
///
/// [`InstancePoolError`] if this file's capacities do not cover what it places.
pub fn place(renderer: &mut ForwardRenderer, scenes: &Scenes) -> Result<Drawn, InstancePoolError> {
    renderer.add_instance(&InstanceDesc {
        mesh: FLOOR_MESH,
        material: FLOOR_MATERIAL,
        transform: Mat4::from_translation(Vec3::new(0.0, -FLOOR_THICKNESS_M, 0.0)),
    })?;
    let [bar, stem] = scenes.handle_parts();
    let (centre, rotation) = scenes.box_pose();
    let mut add = |(centre, rotation, half): (DVec3, DQuat, DVec3), material| {
        renderer.add_instance(&InstanceDesc {
            mesh: CUBE_MESH,
            material,
            transform: transform(centre, rotation, half),
        })
    };
    Ok(Drawn {
        bar: add(bar, HANDLE_MATERIAL)?,
        stem: add(stem, HANDLE_MATERIAL)?,
        crate_: add((centre, rotation, DVec3::splat(BOX_HALF)), BOX_MATERIAL)?,
    })
}

impl Drawn {
    /// Poses the three body instances where `scenes` has them now.
    pub fn update(&self, renderer: &mut ForwardRenderer, scenes: &Scenes) {
        let [bar, stem] = scenes.handle_parts();
        let (centre, rotation) = scenes.box_pose();
        for (handle, (centre, rotation, half), material) in [
            (self.bar, bar, HANDLE_MATERIAL),
            (self.stem, stem, HANDLE_MATERIAL),
            (
                self.crate_,
                (centre, rotation, DVec3::splat(BOX_HALF)),
                BOX_MATERIAL,
            ),
        ] {
            renderer.set_instance(
                handle,
                &InstanceDesc {
                    mesh: CUBE_MESH,
                    material,
                    transform: transform(centre, rotation, half),
                },
            );
        }
    }
}

/// Where the frame is seen from: fixed, square on to both scenes, a little
/// above them.
#[must_use]
pub fn camera() -> Camera {
    Camera {
        eye: Vec3::new(0.0, 3.2, 8.5),
        target: Vec3::new(0.0, 1.6, 0.0),
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

    /// The capacities are exactly what the description declares and what
    /// [`place`] adds.
    #[test]
    fn the_description_reserves_what_this_file_places() {
        let scene = scene();
        assert_eq!(scene.materials.len(), MATERIALS);
        assert_eq!(scene.meshes.len(), CAPACITIES.meshes as usize);
        assert_eq!(scene.capacities.instances, INSTANCES);
    }

    /// An instance is the body: a unit cube scaled to twice the half-extents,
    /// its centre where the body's is.
    #[test]
    fn a_body_transform_is_its_pose_and_its_extents() {
        let m = transform(
            DVec3::new(1.0, 2.0, 3.0),
            DQuat::IDENTITY,
            DVec3::new(0.5, 0.25, 1.0),
        );
        let corner = m.transform_point3(Vec3::splat(0.5));
        assert_eq!(corner, Vec3::new(1.5, 2.25, 4.0));
    }
}
