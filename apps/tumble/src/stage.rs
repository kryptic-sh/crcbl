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
use crcbl::phys::PhysicsDebugLine;
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
const FLOORS: [([f32; 3], f32, f32); 6] = [
    ([0.0, 0.0, 0.0], 12.0, 12.0),
    ([12.0, 0.0, 0.0], 6.0, 3.0),
    ([26.0, 0.0, 0.0], 7.0, 7.0),
    ([44.0, 0.0, 0.0], 20.0, 9.0),
    ([66.0, 0.0, 0.0], 10.0, 11.0),
    ([92.0, 0.0, 1.5], 26.0, 14.0),
];

/// What this stage reserves. The pit's thousand balls, the Tower room's boxes
/// and the Bullets room's bricks and shots are one instance each, the wall's
/// pills and pegs three, and the rest is headroom for the wall's bodies and
/// the shots turning over within a frame; the Bridge room's rods and
/// ragdolls' limbs are three each, a pill's.
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
        View::Tower => (Vec3::new(44.0, 5.5, 21.0), Vec3::new(44.0, 4.5, 0.0)),
        View::Bullets => (Vec3::new(62.5, 3.2, 6.5), Vec3::new(66.0, 0.5, 0.0)),
        View::Bridge => (Vec3::new(91.0, 5.0, 19.0), Vec3::new(91.0, 2.0, 1.5)),
    };
    Camera {
        eye,
        target,
        up: Vec3::Y,
        projection: Projection::default(),
    }
}

/// The colours an awake body's box cycles through by its island, so the
/// bodies the solver steps together share one.
const ISLAND_COLORS: [[f32; 4]; 6] = [
    [0.35, 0.75, 1.0, 1.0],
    [0.45, 0.95, 0.55, 1.0],
    [1.0, 0.72, 0.2, 1.0],
    [0.95, 0.35, 0.85, 1.0],
    [0.6, 0.45, 0.95, 1.0],
    [0.3, 0.9, 0.85, 1.0],
];

/// A box in no island: a fixture, or a body not yet stepped.
const UNISLANDED_COLOR: [f32; 4] = [0.85, 0.85, 0.85, 1.0];

/// A sleeping body's box: dimmed, so what still moves stands out.
const SLEEPING_COLOR: [f32; 4] = [0.4, 0.4, 0.46, 1.0];

/// A contact normal where the surfaces touch.
const TOUCHING_COLOR: [f32; 4] = [1.0, 0.3, 0.2, 1.0];

/// A contact normal the solver keeps before the surfaces meet.
const SPECULATIVE_COLOR: [f32; 4] = [1.0, 0.9, 0.3, 1.0];

/// The room on screen's physics debug view, each segment in render space and
/// coloured, while [`Scenes::debug_view`] is on, and nothing while it is off:
/// an awake body's box by its island, a sleeping one's dimmed, a box in no
/// island apart, and each contact normal by whether its surfaces touch.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn debug_segments(scenes: &Scenes) -> Vec<(Vec3, Vec3, [f32; 4])> {
    let mut segments = Vec::new();
    if !scenes.debug_view() {
        return segments;
    }
    scenes.debug_lines(&mut |from, to, kind| {
        let color = match kind {
            PhysicsDebugLine::Bounds { sleeping: true, .. } => SLEEPING_COLOR,
            PhysicsDebugLine::Bounds {
                island: Some(island),
                ..
            } => ISLAND_COLORS[island as usize % ISLAND_COLORS.len()],
            PhysicsDebugLine::Bounds { island: None, .. } => UNISLANDED_COLOR,
            PhysicsDebugLine::Contact { touching: true, .. } => TOUCHING_COLOR,
            PhysicsDebugLine::Contact {
                touching: false, ..
            } => SPECULATIVE_COLOR,
        };
        segments.push((from.as_vec3(), to.as_vec3(), color));
    });
    segments
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
    /// floors, every fixture, the pit's thousand balls, the Tower room's
    /// boxes, the wall's cap of bodies all as pills, with the wall's cap
    /// again for a frame in which every body turned over, and the Bullets
    /// room's bricks, plank and cap of shots, twice over likewise, and the
    /// Bridge room's rods, balls, planks, ragdolls and cap of crates, twice.
    #[test]
    fn the_rooms_at_their_fullest_fit_the_instance_pool() {
        let scenes = Scenes::new();
        let mut fixtures = Vec::new();
        for room in scenes.rooms() {
            room.fixtures(&mut fixtures);
        }
        let fixture_instances: usize = fixtures.iter().map(|s| instances(s).1).sum();
        // The Tower room's bodies never come or go: one box instance each.
        let mut tower = Vec::new();
        scenes.rooms()[3].bodies(&mut tower);
        let tower_bodies = tower.len();
        // The Bridge room's bodies are fixed but for its crates: its rods,
        // balls, planks and ragdolls as they start, its cap of crates, and all
        // of it again for a frame in which every body turned over.
        let mut bridge = Vec::new();
        scenes.rooms()[5].bodies(&mut bridge);
        let bridge_instances: usize =
            bridge.iter().map(|s| instances(s).1).sum::<usize>() + crate::bridge::MAX_CRATES;
        let most = 2 * bridge_instances
            + FLOORS.len()
            + fixture_instances
            + 3
            + crate::pit::BALLS as usize
            + 2 * 3 * crate::wall::MAX_LIVE
            + tower_bodies
            + (crate::bullets::BRICKS_ACROSS * crate::bullets::BRICK_ROWS) as usize
            + 1
            + 2 * crate::bullets::MAX_SHOTS;
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

    /// How many ticks the debug view's test runs before it looks: enough for
    /// the Tower room's pyramid to sleep with its contacts still drawn.
    const DEBUG_TICKS: u64 = 100;

    /// **The debug view draws nothing until `D` asks for it, then the room on
    /// screen and only that room**: sleeping boxes dimmed, awake ones by
    /// island, touching and speculative contacts apart, and the wall's
    /// fixtures, in no island, in a colour of their own.
    #[test]
    fn the_debug_view_draws_the_room_on_screen_once_asked() {
        let mut scenes = Scenes::new();
        for _ in 0..DEBUG_TICKS {
            scenes.step(crate::scene::tests::tick_dt());
        }
        scenes.set_view(View::Tower);
        assert!(debug_segments(&scenes).is_empty(), "drawn before asked");
        scenes.toggle_debug_view();
        // Each room's own stretch of the x axis, halfway to its neighbours.
        let within = |segments: &[(Vec3, Vec3, [f32; 4])], (west, east): (f32, f32)| {
            segments
                .iter()
                .all(|(from, to, _)| [from.x, to.x].iter().all(|x| (west..east).contains(x)))
        };
        let tower = debug_segments(&scenes);
        assert!(
            within(&tower, (35.0, 55.0)),
            "a segment outside the Tower room"
        );
        let has = |segments: &[(Vec3, Vec3, [f32; 4])], color| {
            segments.iter().any(|segment| segment.2 == color)
        };
        assert!(has(&tower, SLEEPING_COLOR), "no sleeping box");
        assert!(has(&tower, TOUCHING_COLOR), "no touching contact");
        assert!(has(&tower, SPECULATIVE_COLOR), "no speculative contact");
        assert!(
            ISLAND_COLORS.iter().any(|&color| has(&tower, color)),
            "no awake box coloured by its island"
        );

        scenes.set_view(View::Wall);
        let wall = debug_segments(&scenes);
        assert!(
            within(&wall, (6.0, 19.0)),
            "a segment outside the wall's room"
        );
        assert!(has(&wall, UNISLANDED_COLOR), "no fixture drawn");

        scenes.toggle_debug_view();
        assert!(debug_segments(&scenes).is_empty(), "drawn once hidden");
    }
}
