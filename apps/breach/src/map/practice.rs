//! The bot practice map: cover, two sightlines and a patrol circuit.
//!
//! ```text
//!            +X
//!             │   ┌───────────────────────────────────────────┐  z = -11
//!             │   │        ◀───── bravo ─────▶  (z = -9.5)     │
//!             │   │   ┌───────────────────────┐   ▣ far crate  │
//!             │   │   │ alpha's circuit       │   (x=+7, z=-7) │
//!             │   │   │        ███ pillar     │        ▲       │
//!             │   │   │        ███ (x=0,z=-2) │      charlie   │
//!             │   │   │ ▣ near crate          │        ▼       │
//!             │   │   └───────────────────────┘   (x = +7.5)   │
//!             │   │            ▲ spawn, z = +8, facing −Z      │
//!             │   └───────────────────────────────────────────┘  z = +11
//!            −X
//! ```
//!
//! # What this map is for, and what it deliberately is not
//!
//! It is the second half of `docs/plan/sample/11-breach.md`'s milestone 0: a
//! room with bots in it that walk, notice the player and shoot back. **There is
//! no navmesh and no pathfinding anywhere in it.** `docs/plan/24-navigation.md`
//! is a post-MVP subsystem whose own text names `arena`'s bots as its forcing
//! function, not breach's — so a practice bot here walks an **authored** list of
//! waypoints ([`ROUTES`]) through the same
//! [`CharacterController`](crcbl::phys::CharacterController) the player walks
//! through, and the cover is what stops it seeing rather than what it steers
//! around.
//!
//! # One set of numbers, two consumers — again
//!
//! Every block here is a [`Cover`] row, and both the mesh in [`place`] and the
//! collider in [`world`] are built from that same row. The room's shell — the
//! floor, the four walls and the ceiling — is written from
//! [`super::SLAB_THICKNESS`], [`super::WALL_THICKNESS`] and
//! [`super::CEILING_Y`], because the two rooms are the same building.
//!
//! # The cover is what makes the sightlines mean something
//!
//! [`PILLAR`] stands three metres tall, well over [`crate::camera::EYE_HEIGHT`],
//! and sits between the spawn and the far half of the room. So a bot on the far
//! leg of [`ROUTES`]`[0]`'s circuit walks **behind** it and out of sight, and
//! comes back into sight at either end — which is the whole of what
//! `crate::bots::has_line_of_sight` is asked, and the reason the browser gate
//! can watch a sighting appear and disappear without touching a key.
//! `the_circuit_passes_behind_the_pillar_and_comes_back_out` is what holds that
//! to the geometry rather than to this paragraph.
//!
//! # There is no sun in here either
//!
//! [`lamps`] hangs point lights under the ceiling for the same reason
//! [`super::lamps`] does, and [`super::house_light`] is the ambient row both
//! rooms are drawn with.

use std::borrow::Cow;

use crcbl::math::{DVec3, Mat4, Vec3};
use crcbl::phys::{BoxCollider, CharacterConfig, PhysicsWorld};
use crcbl::render::scene::{Capacities, Geometry, InstanceDesc, MeshDesc, ProbeGrid, SceneDesc};
use crcbl::render::{ForwardRenderer, InstanceHandle, InstancePoolError, Light, PointLight};

use super::{CEILING_Y, SLAB_THICKNESS, WALL_THICKNESS, painted};

// ---------------------------------------------------------------------------
// The room
// ---------------------------------------------------------------------------

/// How far the arena reaches either side of its centre line, in metres.
pub const HALF_WIDTH: f64 = 9.0;

/// The end of the arena the bots patrol, in metres along `Z`.
pub const FAR_Z: f64 = -11.0;

/// The end the player spawns at.
pub const NEAR_Z: f64 = 11.0;

/// How deep the room is, in metres.
pub const DEPTH: f64 = NEAR_Z - FAR_Z;

/// The `Z` the room's slabs are centred on.
pub const CENTRE_Z: f64 = 0.5 * (NEAR_Z + FAR_Z);

/// Where the player's **feet** start, in metres.
///
/// Back from the cover and square on to the pillar, so a visitor who arrives and
/// does nothing is looking down the sightline the bots come out of.
pub const SPAWN: DVec3 = DVec3::new(0.0, 0.0, 8.0);

// ---------------------------------------------------------------------------
// The cover
// ---------------------------------------------------------------------------

/// One block of cover: where it stands, how big it is, and what it is called.
///
/// Axis-aligned, like everything else in this sample — `crcbl-phys` has a
/// sphere, an axis-aligned box and a Y-aligned capsule, and a blockout of boxes
/// is one where the mesh and the collider are the same cuboid to the last
/// millimetre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cover {
    /// What the panel and a failing test call it.
    pub label: &'static str,
    /// Where it stands across the arena, in metres.
    pub x: f64,
    /// …and down it, in metres along `Z`.
    pub z: f64,
    /// How wide it is across `X`, in metres.
    pub width: f64,
    /// How deep it is across `Z`, in metres.
    pub depth: f64,
    /// How tall it stands, in metres.
    pub height: f64,
}

impl Cover {
    /// The block as the physics world holds it.
    ///
    /// The same six numbers [`place`] draws it from — see the module docs.
    #[must_use]
    pub fn collider(&self) -> BoxCollider {
        BoxCollider::new(
            DVec3::new(self.x, 0.5 * self.height, self.z),
            DVec3::new(0.5 * self.width, 0.5 * self.height, 0.5 * self.depth),
        )
    }
}

/// Which block [`COVER`] holds the tall one.
///
/// Named because the sightline test and the module docs both point at it: it is
/// the block the far half of [`ROUTES`]`[0]` disappears behind.
pub const PILLAR: usize = 0;

/// The blocks in the room, tallest first.
///
/// Three, and each is doing a different job: [`PILLAR`] breaks the sightline
/// down the middle of the room, and the two crates are cover a player can put
/// between themselves and a bot on either flank.
pub const COVER: [Cover; 3] = [
    Cover {
        label: "pillar",
        x: 0.0,
        z: -2.0,
        width: 3.0,
        depth: 3.0,
        height: 3.0,
    },
    Cover {
        label: "near crate",
        x: -6.5,
        z: 2.0,
        width: 2.5,
        depth: 2.5,
        height: 2.2,
    },
    Cover {
        label: "far crate",
        x: 7.0,
        z: -7.0,
        width: 2.5,
        depth: 2.5,
        height: 2.2,
    },
];

// ---------------------------------------------------------------------------
// The patrols
// ---------------------------------------------------------------------------

/// How many bots the map has.
///
/// Three, and a fixed number rather than a flag: this is a practice map, not a
/// scale fixture, and `apps/horde` is where a crowd is measured.
pub const BOTS: usize = 3;

/// One bot's authored patrol: what it is called and where it walks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Route {
    /// What the panel, the `[HUD]` line and [`crate::game::Aim`] call this bot.
    pub label: &'static str,
    /// The **feet** positions it walks between, in order, as a cycle: the last
    /// waypoint is followed by the first, so a two-waypoint route is a
    /// back-and-forth and a four-waypoint one is a circuit.
    pub waypoints: &'static [DVec3],
}

/// The three patrols.
///
/// Authored so that each says something different about what
/// `crate::bots::has_line_of_sight` answers from the spawn:
///
/// * **alpha** walks a circuit around [`PILLAR`], so it is out of sight down the
///   middle of its far leg and back in sight at both ends of it.
/// * **bravo** paces the far wall almost entirely behind the pillar, and shows
///   itself only at the ends of its beat.
/// * **charlie** paces the right-hand flank in the open, and is the bot a
///   visitor who does nothing is shot by.
pub const ROUTES: [Route; BOTS] = [
    Route {
        label: "alpha",
        waypoints: &[
            DVec3::new(-4.5, 0.0, -8.0),
            DVec3::new(4.5, 0.0, -8.0),
            DVec3::new(4.5, 0.0, 4.5),
            DVec3::new(-4.5, 0.0, 4.5),
        ],
    },
    Route {
        label: "bravo",
        waypoints: &[DVec3::new(-4.0, 0.0, -9.5), DVec3::new(4.0, 0.0, -9.5)],
    },
    Route {
        label: "charlie",
        waypoints: &[DVec3::new(7.5, 0.0, 5.0), DVec3::new(7.5, 0.0, -3.0)],
    },
];

// ---------------------------------------------------------------------------
// The scene description
// ---------------------------------------------------------------------------

/// The floor slab — [`SceneDesc::meshes`] slot 0.
pub const FLOOR_MESH: usize = 0;
/// The ceiling slab.
pub const CEILING_MESH: usize = 1;
/// The wall across the arena, drawn at both ends.
pub const END_WALL_MESH: usize = 2;
/// The wall along it, drawn on both sides.
pub const SIDE_WALL_MESH: usize = 3;
/// [`PILLAR`], the one block tall enough to break a sightline.
pub const PILLAR_MESH: usize = 4;
/// A crate, which both of the other [`COVER`] rows share.
pub const CRATE_MESH: usize = 5;
/// A bot's body — the capsule the controller sweeps, drawn as it is swept.
pub const BOT_MESH: usize = 6;

/// The shell — floor, walls and ceiling. [`SceneDesc::materials`] slot 0.
pub const SHELL_MATERIAL: usize = 0;
/// Cover, which is a different colour from the shell so the shootable geometry
/// and the walkable geometry are told apart at a glance.
pub const COVER_MATERIAL: usize = 1;
/// A bot that has not noticed the player.
pub const BOT_MATERIAL: usize = 2;
/// …and one that has. **The picture says which**, for the reason a knocked-down
/// plate is drawn orange on the range: a state a reviewer cannot see is a state
/// they cannot check the readout against.
pub const BOT_ALERT_MATERIAL: usize = 3;
/// A bot the player has shot, lying on the floor until it comes back.
pub const BOT_DOWN_MATERIAL: usize = 4;

/// How many latitude bands per hemisphere a bot's capsule is drawn with, and how
/// many longitude columns.
///
/// Enough to read as a body at ten metres and no more: this is a greybox figure,
/// and the browser is the target the whole slice is built for.
const BOT_RINGS: u32 = 6;
const BOT_SEGMENTS: u32 = 12;

/// What this map reserves, which is a little over what it places.
///
/// Sized against the description rather than left at [`Capacities::default`],
/// for [`super`]'s reason. `the_map_fits_the_pools_it_reserves` asserts it.
const CAPACITIES: Capacities = Capacities {
    vertices: 4 * 1024,
    indices: 8 * 1024,
    meshes: 8,
    instances: 16,
    materials: 8,
    lights: 8,
    probes: 0,
};

/// The shape of a bot, which is the shape of the thing that walks.
///
/// [`CharacterConfig::default`] is what `crate::bots` builds every controller
/// with, so the capsule drawn here is the capsule swept — and a reviewer looking
/// at a bot standing against a wall is looking at the collider.
fn bot_shape() -> (f32, f32) {
    let config = CharacterConfig::default();
    (
        config.radius as f32,
        (2.0 * (config.radius + config.half_height)) as f32,
    )
}

/// Everything this map makes resident: seven meshes, five painted rows and the
/// grid page they sample.
///
/// The mesh and material order is the constants above, in value order;
/// `the_constants_name_their_own_meshes` asserts it.
#[must_use]
pub fn scene() -> SceneDesc<'static> {
    use crcbl::greybox::{capsule, grid_page, platform, wall};

    let mesh = |label: &'static str, geometry: Geometry<'static>| MeshDesc {
        label: Cow::Borrowed(label),
        geometry,
    };
    let block = |at: &Cover| platform(at.width as f32, at.depth as f32, at.height as f32);
    let (radius, height) = bot_shape();
    SceneDesc {
        meshes: vec![
            mesh(
                "floor",
                platform(2.0 * HALF_WIDTH as f32, DEPTH as f32, SLAB_THICKNESS as f32),
            ),
            mesh(
                "ceiling",
                platform(2.0 * HALF_WIDTH as f32, DEPTH as f32, SLAB_THICKNESS as f32),
            ),
            mesh(
                "end wall",
                wall(
                    2.0 * HALF_WIDTH as f32,
                    CEILING_Y as f32,
                    WALL_THICKNESS as f32,
                ),
            ),
            // Built along `X` like every other `wall`, and turned a quarter turn
            // about `+Y` where it is placed. One mesh serves both sides.
            mesh(
                "side wall",
                wall(DEPTH as f32, CEILING_Y as f32, WALL_THICKNESS as f32),
            ),
            mesh("pillar", block(&COVER[PILLAR])),
            mesh("crate", block(&COVER[1])),
            mesh("bot", capsule(radius, height, BOT_RINGS, BOT_SEGMENTS)),
        ],
        materials: vec![
            painted([0.27, 0.29, 0.33]),
            painted([0.44, 0.40, 0.33]),
            painted([0.36, 0.58, 0.72]),
            painted([0.86, 0.44, 0.30]),
            painted([0.24, 0.26, 0.30]),
        ],
        page: grid_page(),
        probes: ProbeGrid::default(),
        capacities: CAPACITIES,
    }
}

/// The bots, as the renderer holds them.
///
/// Handed back by [`place`] because they are the only instances on this map that
/// are ever rewritten: the room and its cover are placed once and drawn for the
/// rest of the run.
#[derive(Debug)]
pub struct Figures {
    bodies: [InstanceHandle; BOTS],
}

/// Where one bot is drawn, and how.
///
/// The frame's copy of what `crate::bots::Bot` is, snapshotted with the rest of
/// [`crate::game::RenderState`] so a draw never reads through the tick's lock.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BotView {
    /// Where its **feet** are, in metres.
    pub feet: DVec3,
    /// Which way it is walking, in [`crate::camera::forward`]'s measure.
    pub facing: f32,
    /// Whether it is on its feet at all.
    pub alive: bool,
    /// Whether it has the player in sight — which is the one piece of a bot's
    /// state the picture carries.
    pub alerted: bool,
}

impl Figures {
    /// Draws one bot where and how the simulation says.
    ///
    /// # Panics
    ///
    /// If `bot` is not a bot. Called only from [`crate::game`]'s own indices.
    pub fn set_bot(&self, renderer: &mut ForwardRenderer, bot: usize, view: &BotView) {
        renderer.set_instance(
            self.bodies[bot],
            &InstanceDesc {
                mesh: BOT_MESH,
                material: if !view.alive {
                    BOT_DOWN_MATERIAL
                } else if view.alerted {
                    BOT_ALERT_MATERIAL
                } else {
                    BOT_MATERIAL
                },
                transform: bot_transform(view),
            },
        );
    }
}

/// Where a bot's mesh sits, standing or knocked over.
///
/// The capsule rises from `y = 0` and is centred on its other two axes, so a
/// standing bot is a translation to its feet and a turn about `+Y` to face the
/// way it is walking. Down is a quarter turn about `+X`, which lays the body
/// along `-Z`, lifted by its own radius so it lies **on** the floor rather than
/// half inside it — the same two poses a knocked plate takes.
fn bot_transform(view: &BotView) -> Mat4 {
    let feet = Vec3::new(view.feet.x as f32, view.feet.y as f32, view.feet.z as f32);
    if view.alive {
        Mat4::from_translation(feet) * Mat4::from_rotation_y(view.facing)
    } else {
        let (radius, _) = bot_shape();
        Mat4::from_translation(feet + Vec3::Y * radius)
            * Mat4::from_rotation_x(-core::f32::consts::FRAC_PI_2)
    }
}

/// Places the room and its cover and hands back the bots.
///
/// Also sets [`lamps`], which is sticky: nothing in this sample moves them.
///
/// # Errors
///
/// [`InstancePoolError`] if `CAPACITIES`'s instance count does not cover the
/// map, which is this file's numbers being wrong rather than a condition a run
/// can be in.
pub fn place(renderer: &mut ForwardRenderer) -> Result<Figures, InstancePoolError> {
    let at =
        |x: f64, y: f64, z: f64| Mat4::from_translation(Vec3::new(x as f32, y as f32, z as f32));
    let turned =
        |x: f64| at(x, 0.0, CENTRE_Z) * Mat4::from_rotation_y(core::f32::consts::FRAC_PI_2);
    let wall_offset = HALF_WIDTH + 0.5 * WALL_THICKNESS;

    for (mesh, transform) in [
        // A `platform` rises from `y = 0`, so the floor is dropped by its own
        // thickness to put its top there.
        (FLOOR_MESH, at(0.0, -SLAB_THICKNESS, CENTRE_Z)),
        (CEILING_MESH, at(0.0, CEILING_Y, CENTRE_Z)),
        (END_WALL_MESH, at(0.0, 0.0, FAR_Z - 0.5 * WALL_THICKNESS)),
        (END_WALL_MESH, at(0.0, 0.0, NEAR_Z + 0.5 * WALL_THICKNESS)),
        (SIDE_WALL_MESH, turned(-wall_offset)),
        (SIDE_WALL_MESH, turned(wall_offset)),
    ] {
        renderer.add_instance(&InstanceDesc {
            mesh,
            material: SHELL_MATERIAL,
            transform,
        })?;
    }

    for (index, block) in COVER.iter().enumerate() {
        renderer.add_instance(&InstanceDesc {
            mesh: if index == PILLAR {
                PILLAR_MESH
            } else {
                CRATE_MESH
            },
            material: COVER_MATERIAL,
            transform: at(block.x, 0.0, block.z),
        })?;
    }

    // The bots last, on the first waypoint of their own route and alive — which
    // is the state `crate::bots` starts them in, so the first frame agrees with
    // the first tick without either having to ask the other.
    let mut bodies = Vec::with_capacity(BOTS);
    for route in ROUTES {
        let view = BotView {
            feet: route.waypoints[0],
            facing: 0.0,
            alive: true,
            alerted: false,
        };
        bodies.push(renderer.add_instance(&InstanceDesc {
            mesh: BOT_MESH,
            material: BOT_MATERIAL,
            transform: bot_transform(&view),
        })?);
    }

    renderer.set_lights(&lamps());
    Ok(Figures {
        bodies: bodies
            .try_into()
            .unwrap_or_else(|_| unreachable!("one instance per route was pushed")),
    })
}

// ---------------------------------------------------------------------------
// The collision side
// ---------------------------------------------------------------------------

/// The same room, as the colliders the capsules sweep against and every ray in
/// this map is cast into.
///
/// The **bots are not in it**: each adds its own capsule when it spawns, because
/// a body that moves belongs to the thing that moves it — see `crate::bots`.
#[must_use]
pub fn world() -> PhysicsWorld {
    let mut world = PhysicsWorld::new();
    let slab = |y: f64| {
        BoxCollider::new(
            DVec3::new(0.0, y, CENTRE_Z),
            DVec3::new(HALF_WIDTH, 0.5 * SLAB_THICKNESS, 0.5 * DEPTH),
        )
    };
    world.add_box(slab(-0.5 * SLAB_THICKNESS));
    world.add_box(slab(CEILING_Y + 0.5 * SLAB_THICKNESS));

    let end_wall = |z: f64| {
        BoxCollider::new(
            DVec3::new(0.0, 0.5 * CEILING_Y, z),
            DVec3::new(HALF_WIDTH, 0.5 * CEILING_Y, 0.5 * WALL_THICKNESS),
        )
    };
    world.add_box(end_wall(FAR_Z - 0.5 * WALL_THICKNESS));
    world.add_box(end_wall(NEAR_Z + 0.5 * WALL_THICKNESS));

    let side_wall = |x: f64| {
        BoxCollider::new(
            DVec3::new(x, 0.5 * CEILING_Y, CENTRE_Z),
            DVec3::new(0.5 * WALL_THICKNESS, 0.5 * CEILING_Y, 0.5 * DEPTH),
        )
    };
    world.add_box(side_wall(-(HALF_WIDTH + 0.5 * WALL_THICKNESS)));
    world.add_box(side_wall(HALF_WIDTH + 0.5 * WALL_THICKNESS));

    for block in COVER {
        world.add_box(block.collider());
    }
    world
}

// ---------------------------------------------------------------------------
// The light
// ---------------------------------------------------------------------------

/// How bright a ceiling lamp is, before its colour. See [`super::lamps`].
const LAMP_INTENSITY: f32 = 24.0;

/// How far a lamp reaches, in metres.
const LAMP_RADIUS: f32 = 18.0;

/// How far under the ceiling a lamp hangs, in metres.
const LAMP_DROP: f64 = 0.3;

/// Where the lamps hang, in metres along `Z` — one over each third of the room,
/// so the cover casts its shadow across the floor rather than along it.
const LAMP_Z: [f64; 3] = [7.0, -1.0, -9.0];

/// The room's fittings: a row of point lights under the ceiling.
#[must_use]
pub fn lamps() -> Vec<Light> {
    LAMP_Z
        .iter()
        .map(|&z| {
            Light::Point(PointLight {
                position: Vec3::new(0.0, (CEILING_Y - LAMP_DROP) as f32, z as f32),
                radius: LAMP_RADIUS,
                color: Vec3::new(1.0, 0.97, 0.92) * LAMP_INTENSITY,
                fill: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::EYE_HEIGHT;
    use crcbl::phys::Ray;

    /// Whether the segment from the spawn's eye to `target` reaches it.
    ///
    /// The same question `crate::bots::has_line_of_sight` asks, asked here of a
    /// world with no bots in it — so what it answers about is the **cover**.
    fn clear_from_spawn(world: &mut PhysicsWorld, target: DVec3) -> bool {
        let eye = DVec3::new(SPAWN.x, EYE_HEIGHT, SPAWN.z);
        let ray = Ray::new(eye, target - eye).with_bounds(0.0, 1.0);
        world.cast_ray(&ray).is_none()
    }

    /// **Every mesh the description makes resident is placed, and every row it
    /// declares is named**, in the order the constants say.
    #[test]
    fn the_constants_name_their_own_meshes() {
        let scene = scene();
        let labels: Vec<&str> = scene.meshes.iter().map(|m| m.label.as_ref()).collect();
        assert_eq!(
            [
                FLOOR_MESH,
                CEILING_MESH,
                END_WALL_MESH,
                SIDE_WALL_MESH,
                PILLAR_MESH,
                CRATE_MESH,
                BOT_MESH,
            ]
            .map(|mesh| labels[mesh]),
            [
                "floor",
                "ceiling",
                "end wall",
                "side wall",
                "pillar",
                "crate",
                "bot",
            ],
        );
        assert_eq!(scene.meshes.len(), BOT_MESH + 1);
        assert_eq!(
            scene.materials.len(),
            5,
            "five painted rows, one per constant",
        );
        // The two crates share one mesh, so they have to be the same size — a
        // pair that drifted would draw one of them at the other's dimensions
        // while its collider stayed its own.
        assert_eq!(COVER[1].width, COVER[2].width);
        assert_eq!(COVER[1].depth, COVER[2].depth);
        assert_eq!(COVER[1].height, COVER[2].height);
    }

    /// **The map fits the pools it reserves.** Counted off the description and
    /// the placement rather than restated.
    #[test]
    fn the_map_fits_the_pools_it_reserves() {
        let scene = scene();
        assert!(scene.meshes.len() <= CAPACITIES.meshes as usize);
        assert!(scene.materials.len() <= CAPACITIES.materials as usize);
        // Six pieces of shell, one instance per block of cover and one per bot.
        let instances = 6 + COVER.len() + BOTS;
        assert!(
            instances <= CAPACITIES.instances as usize,
            "{instances} instances in a pool of {}",
            CAPACITIES.instances,
        );
        assert!(lamps().len() < CAPACITIES.lights as usize);
    }

    /// **The spawn is on this room's floor, inside its walls, and clear of every
    /// block of cover.** A player who starts inside the pillar is a player the
    /// depenetration pass shoves somewhere unpredictable.
    #[test]
    fn the_spawn_has_the_floor_of_this_room_under_it_and_nothing_else() {
        let mut world = world();
        let config = CharacterConfig::default();
        let mut player = crcbl::phys::CharacterController::new(
            config,
            SPAWN + DVec3::Y * (config.radius + config.half_height),
        );
        player.move_and_slide(&mut world, DVec3::ZERO);
        assert!(player.is_grounded(), "the spawn has no floor under it");
        let feet = player.position().y - (config.radius + config.half_height);
        assert!(
            feet.abs() < config.skin_width * 3.0,
            "the player settled at {feet} rather than on the floor",
        );
        const { assert!(SPAWN.z < NEAR_Z && SPAWN.z > FAR_Z) };
        const { assert!(SPAWN.x < HALF_WIDTH && SPAWN.x > -HALF_WIDTH) };
        for block in COVER {
            let clear = (SPAWN.x - block.x).abs() > 0.5 * block.width + config.radius
                || (SPAWN.z - block.z).abs() > 0.5 * block.depth + config.radius;
            assert!(clear, "the spawn is inside the {}", block.label);
        }
    }

    /// **Every waypoint is somewhere a bot can stand**: inside the room, and
    /// clear of every block of cover by its own radius.
    ///
    /// A route that clipped a crate would leave a bot grinding against it for
    /// the rest of the run, which reads on the panel as a patrol that stopped.
    #[test]
    fn every_waypoint_is_clear_of_the_walls_and_the_cover() {
        let radius = CharacterConfig::default().radius;
        for route in ROUTES {
            assert!(
                route.waypoints.len() >= 2,
                "{}'s route is not a patrol",
                route.label,
            );
            for point in route.waypoints {
                assert!(
                    point.x.abs() + radius < HALF_WIDTH,
                    "{} walks into a side wall at {point}",
                    route.label,
                );
                assert!(
                    point.z + radius < NEAR_Z && point.z - radius > FAR_Z,
                    "{} walks into an end wall at {point}",
                    route.label,
                );
                assert_eq!(point.y, 0.0, "{}'s waypoints are feet", route.label);
                for block in COVER {
                    let clear = (point.x - block.x).abs() > 0.5 * block.width + radius
                        || (point.z - block.z).abs() > 0.5 * block.depth + radius;
                    assert!(
                        clear,
                        "{} walks into the {} at {point}",
                        route.label, block.label,
                    );
                }
            }
        }
    }

    /// **alpha's circuit passes behind the pillar and comes back out**, which is
    /// the whole reason this map has a pillar — and the positive and the control
    /// the browser gate reads, made here where a failure names the geometry.
    ///
    /// Swept along the far leg rather than checked at two chosen points: which
    /// end is hidden is exactly the thing a hand-picked pair gets wrong.
    #[test]
    fn the_circuit_passes_behind_the_pillar_and_comes_back_out() {
        let mut world = world();
        let alpha = ROUTES[0].waypoints;
        let (from, to) = (alpha[0], alpha[1]);
        let mut hidden = 0;
        let mut shown = 0;
        for step in 0..=100 {
            let t = f64::from(step) / 100.0;
            let feet = from + (to - from) * t;
            let head = DVec3::new(feet.x, EYE_HEIGHT, feet.z);
            if clear_from_spawn(&mut world, head) {
                shown += 1;
            } else {
                hidden += 1;
            }
        }
        assert!(
            hidden > 0,
            "nothing on alpha's far leg is behind cover, so the map has no sightline to break",
        );
        assert!(
            shown > 0,
            "the whole of alpha's far leg is behind cover, so a sighting can never happen",
        );
        // And the middle of that leg is the hidden half, which is what says the
        // pillar is what is doing it rather than some corner of the room.
        let middle = DVec3::new(0.5 * (from.x + to.x), EYE_HEIGHT, 0.5 * (from.z + to.z));
        assert!(
            !clear_from_spawn(&mut world, middle),
            "the point directly behind the pillar is in plain sight",
        );
    }

    /// **charlie is in the open along its whole beat**, which is what makes a
    /// page nobody touches a page where something is shooting at the player.
    #[test]
    fn charlies_beat_is_in_plain_sight_from_the_spawn() {
        let mut world = world();
        let beat = ROUTES[2].waypoints;
        for step in 0..=40 {
            let t = f64::from(step) / 40.0;
            let feet = beat[0] + (beat[1] - beat[0]) * t;
            let head = DVec3::new(feet.x, EYE_HEIGHT, feet.z);
            assert!(
                clear_from_spawn(&mut world, head),
                "charlie is hidden at {head}, so an untouched page may never be shot at",
            );
        }
    }

    /// **Every block of cover stands over the height a shot is taken from**,
    /// which is the difference between cover and a hurdle: a block a level ray
    /// passes over is scenery, and this map's whole subject is what a ray does
    /// not get through. [`PILLAR`] is the tallest, because it is the one that
    /// has to break the sightline down the middle of the room from any stance.
    #[test]
    fn every_block_of_cover_stands_over_the_height_a_shot_is_taken_from() {
        for block in COVER {
            assert!(
                block.height > EYE_HEIGHT,
                "the {} is {} m and the eye is at {EYE_HEIGHT} m, so a level shot goes over it",
                block.label,
                block.height,
            );
            assert!(
                block.height < CEILING_Y,
                "the {} reaches the ceiling",
                block.label,
            );
            assert!(block.height <= COVER[PILLAR].height);
        }
    }
}
