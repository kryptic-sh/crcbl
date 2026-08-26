//! The range: what the player walks on, what the pistol can hit, and what both
//! are drawn as — and [`MapChoice`], which says which of this sample's two maps
//! a run opened on.
//!
//! Everything at this level is the **firing range**. The bot practice map is
//! [`practice`], a module of its own, and it is written from this one's shell
//! constants — [`CEILING_Y`], [`SLAB_THICKNESS`], [`WALL_THICKNESS`] and
//! `painted` — because the two rooms are the same building.
//!
//! ```text
//!            +X
//!             │   ┌───────────────────────────────────────────┐  z = -26, the butt
//!             │   │        ◀ ▣ ▶ far  (x = +4.5 ±3, z = -18)   │
//!             │   │   ▣ mid  (x = -6.0, z = -12)               │
//!             │   │            ▣ near  (x =  0.0, z =  -8)     │
//!             │   ├───────────────────────────────────────────┤  z =   0, the firing line
//!             │   │            ▲ spawn, z = +10, facing −Z     │
//!             │   └───────────────────────────────────────────┘  z = +12
//!            −X
//! ```
//!
//! # One set of numbers, two consumers
//!
//! Every surface here is a [`crcbl::greybox`] primitive over a constant in this
//! file, and every collider in [`world`] is written from the *same* constant.
//! There is no second set of numbers for the physics, which is what makes a
//! range that looks shootable shootable — and it is the reason a hit on the
//! wall behind a lane is a miss rather than a mystery.
//!
//! # Everything is a box, and this time that is not a compromise
//!
//! `apps/puppet/src/map.rs` had to build its slopes out of spheres, because
//! `crcbl-phys`'s colliders are a sphere, an axis-aligned box and a Y-aligned
//! capsule and there is no wedge among them. An indoor range is boxes all the
//! way down — floor, four walls, a ceiling, a kerb, three posts and three
//! plates — so the mesh and the collider are the same cuboid to the last
//! millimetre and nothing here is approximated at all.
//!
//! # The firing line is the step the controller refuses
//!
//! [`KERB_HEIGHT`] is over the default
//! [`step_offset`](crcbl::phys::CharacterConfig::step_offset), so walking into
//! it is [`MoveOutcome::hit_wall`](crcbl::phys::MoveOutcome) rather than a
//! step-up: the player cannot get down-range, and nothing in [`crate::game`]
//! special-cases their position to keep them back.
//! `the_firing_line_is_over_the_offset_the_controller_climbs` holds it to the
//! controller's own number rather than to this sentence.
//!
//! # A plate stands at eye height, and that is a decision about the first second
//!
//! [`PLATE_CENTRE_Y`] is [`crate::camera::EYE_HEIGHT`]. A visitor who arrives,
//! looks down the range and pulls the trigger hits something — the near lane is
//! straight ahead of the spawn — and a demo whose first shot goes over the
//! target is a demo nobody works out. It is also what lets a downed plate be
//! genuinely out of the way: knocked over, it lies flat at [`PLATE_BOTTOM_Y`]
//! and a level shot passes above it.
//!
//! # There is no sun in a room with a ceiling
//!
//! [`house_light`] hands [`begin_frame`](crcbl::render::ForwardRenderer) a
//! [`DirectionalLight`] whose directional term is nearly black: a sun would be
//! occluded by [`CEILING_Y`]'s slab and the room would be lit by the ambient
//! alone. What lights it instead is [`lamps`] — a row of point lights under the
//! ceiling, on `docs/plan/18-render-features.md`'s many-lights path — and the
//! sun row is kept for the one thing only it carries, which is the ambient.

use std::borrow::Cow;

use crcbl::greybox::{GREYBOX_TILE_M, grid_material, grid_page, platform, wall};
use crcbl::math::{DVec3, Mat4, Vec3};
use crcbl::phys::{BoxCollider, ColliderId, PhysicsWorld};
use crcbl::render::scene::{Capacities, Geometry, InstanceDesc, MeshDesc, ProbeGrid, SceneDesc};
use crcbl::render::{
    DirectionalLight, ForwardRenderer, InstanceHandle, InstancePoolError, Light, PointLight,
};
use crcbl::shaders::mesh::GpuMaterial;

use crate::camera::EYE_HEIGHT;

pub mod practice;

// ---------------------------------------------------------------------------
// Which map
// ---------------------------------------------------------------------------

/// Which of breach's two maps a run opens on.
///
/// **The one thing about this sample a page can choose.** `--map` sets it on a
/// command line and `__crcbl_breach_map` sets it from a browser — see
/// `apps/breach/src/args.rs` for the pair, which is the shape `apps/horde`'s
/// `--prefill`
/// already has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MapChoice {
    /// The firing range: three lanes, three plates, and a line to shoot from.
    #[default]
    Range,
    /// The bot practice map: cover, sightlines and a patrol circuit.
    Practice,
}

impl MapChoice {
    /// Every map, in the order `--help` lists them.
    pub const ALL: [Self; 2] = [Self::Range, Self::Practice];

    /// What `--map` spells it, and what the `[HUD]` line and the panel call it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Range => "range",
            Self::Practice => "practice",
        }
    }

    /// The map `name` spells, or `None` for anything else.
    ///
    /// The inverse of [`name`](Self::name), and the only place a string becomes
    /// a map — `--map`, the wasm export and the page's `?map=` all come through
    /// here, so they cannot disagree about what a name means.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|map| map.name() == name)
    }
}

// ---------------------------------------------------------------------------
// The room
// ---------------------------------------------------------------------------

/// How far the range reaches either side of its centre line, in metres.
///
/// Wide enough that the two outer lanes are a real turn away from the near one
/// — which is what makes "look somewhere else and the shot misses" a thing a
/// player does rather than a thing a test arranges.
pub const HALF_WIDTH: f64 = 8.0;

/// The far end of the range — the butt the plates stand in front of, in metres
/// along `Z`.
pub const BUTT_Z: f64 = -26.0;

/// The near end, behind the shooter.
///
/// **The run-up is long on purpose.** `web/tools/browser-e2e.mjs` holds the
/// walk key, waits for the player to have advanced, and releases it again to
/// check that they stop; the heartbeat it reads is a second of simulated time
/// apart, which at [`crate::game::WALK_SPEED`] is several metres. From
/// [`SPAWN_Z`] it is three beats to the firing line, so a slow machine's
/// release still lands while the player is moving.
pub const REAR_Z: f64 = 12.0;

/// How deep the room is, in metres.
pub const DEPTH: f64 = REAR_Z - BUTT_Z;

/// The `Z` the room's slabs are centred on.
pub const CENTRE_Z: f64 = 0.5 * (REAR_Z + BUTT_Z);

/// How high the ceiling is, in metres above the floor.
pub const CEILING_Y: f64 = 4.0;

/// How thick the floor and ceiling slabs are, in metres. The floor's **top** is
/// `y = 0`, which is what every other height here is measured from.
pub const SLAB_THICKNESS: f64 = 0.5;

/// How thick a wall is, in metres.
pub const WALL_THICKNESS: f64 = 0.4;

// ---------------------------------------------------------------------------
// The firing line
// ---------------------------------------------------------------------------

/// Where the firing line is, in metres along `Z`. The player shoots from behind
/// it and cannot get past it.
pub const FIRING_LINE_Z: f64 = 0.0;

/// How deep the kerb is across `Z`, in metres.
pub const KERB_DEPTH: f64 = 0.4;

/// How high the kerb stands, in metres.
///
/// Over the default [`step_offset`](crcbl::phys::CharacterConfig::step_offset)
/// and its skin-width band, so the controller refuses it rather than climbing
/// it — see the module docs.
pub const KERB_HEIGHT: f64 = 0.7;

// ---------------------------------------------------------------------------
// The lanes
// ---------------------------------------------------------------------------

/// How many lanes the range has.
pub const LANES: usize = 3;

/// One lane: what it is called, and where its plate stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lane {
    /// What the panel and the `[HUD]` line call it.
    pub label: &'static str,
    /// Where the plate stands across the range, in metres.
    pub x: f64,
    /// How far down the range it stands, in metres along `Z`.
    pub z: f64,
}

impl Lane {
    /// How far this lane's plate is from the firing line, in metres — what the
    /// panel prints beside its state.
    #[must_use]
    pub fn distance(&self) -> f64 {
        FIRING_LINE_Z - self.z
    }
}

/// The three lanes, near to far.
///
/// **The near one is straight ahead of the spawn**, which is the whole of what
/// makes the demo legible on arrival: a visitor who does nothing but pull the
/// trigger hits it. The other two are off to either side and further away, so
/// reaching them is a turn — and a turn is what the browser gate drives to aim
/// away from every plate at all.
pub const LANE_LIST: [Lane; LANES] = [
    Lane {
        label: "near",
        x: 0.0,
        z: -8.0,
    },
    Lane {
        label: "mid",
        x: -6.0,
        z: -12.0,
    },
    Lane {
        label: "far",
        x: 4.5,
        z: -18.0,
    },
];

/// How wide a target plate is, in metres.
pub const PLATE_WIDTH: f64 = 0.5;

/// How tall a target plate is, in metres.
pub const PLATE_HEIGHT: f64 = 0.5;

/// How thick a target plate is, in metres.
pub const PLATE_THICKNESS: f64 = 0.06;

/// How high the centre of a standing plate is, in metres above the floor.
///
/// [`crate::camera::EYE_HEIGHT`], restated as a named constant because the
/// *plate* is built from it and the two agreeing is the reason a level shot
/// hits. `the_plates_stand_at_the_height_the_player_looks_from` is what holds
/// them together.
pub const PLATE_CENTRE_Y: f64 = EYE_HEIGHT;

/// How high the bottom edge of a standing plate is, in metres — the hinge it
/// falls over, and the height a downed plate lies at.
pub const PLATE_BOTTOM_Y: f64 = PLATE_CENTRE_Y - 0.5 * PLATE_HEIGHT;

/// How wide the square post under a plate is, in metres.
pub const POST_EDGE: f64 = 0.12;

/// Which lane's plate travels rather than standing still.
///
/// **A mover, which is a thing a real range has and this demo needs anyway.**
/// A range whose picture is identical from one frame to the next is
/// indistinguishable from a loop that has stopped, and the first thing
/// `web/tools/browser-e2e.mjs`'s group D asks of any demo is that the canvas
/// changes between frames. `apps/puppet` answers that with a turning sun and
/// `apps/lantern` with an orbiting lamp; a room with a ceiling has neither, and
/// a target that traverses its lane is the answer this one already wanted.
///
/// It is the **far** lane, so it is never between the shooter and either of the
/// others — see [`plate_x`], whose sweep is bounded to keep it clear of the
/// centre line the near lane stands on.
pub const MOVER_LANE: usize = 2;

/// How far the travelling plate slides either side of its lane, in metres.
///
/// Bounded by two things and by nothing else: its far edge must stay inside
/// [`HALF_WIDTH`], and its near edge must stay clear of `x = 0`, where the near
/// lane's plate and the shot down the middle of the range both are.
/// `the_travelling_plate_stays_in_its_own_half_of_the_range` asserts both.
pub const MOVER_SWEEP_M: f64 = 3.0;

/// How long the travelling plate takes to go out and back, in seconds.
///
/// Slow enough to be tracked by a shooter rather than sprayed at, and fast
/// enough that consecutive `[HUD]` heartbeats — a simulated second apart —
/// report positions that differ far above the two decimal places the line
/// prints them to.
pub const MOVER_PERIOD_S: f64 = 9.0;

/// Where a lane's plate is across the range, `seconds` into the run.
///
/// A pure function of the simulated time, so the same tick puts it in the same
/// place on every machine — the reason `apps/puppet::map::sun` is one too.
/// Every lane but [`MOVER_LANE`] answers its own fixed `x`.
#[must_use]
pub fn plate_x(lane: usize, seconds: f64) -> f64 {
    let at = LANE_LIST[lane].x;
    if lane == MOVER_LANE {
        at + MOVER_SWEEP_M * (core::f64::consts::TAU * seconds / MOVER_PERIOD_S).sin()
    } else {
        at
    }
}

// ---------------------------------------------------------------------------
// The player
// ---------------------------------------------------------------------------

/// Where the player's **feet** start, in metres.
pub const SPAWN: DVec3 = DVec3::new(0.0, 0.0, SPAWN_Z);

/// The `Z` half of [`SPAWN`], named because [`REAR_Z`] and the run-up are
/// measured against it.
pub const SPAWN_Z: f64 = 10.0;

// ---------------------------------------------------------------------------
// The scene description
// ---------------------------------------------------------------------------

/// The floor slab — [`SceneDesc::meshes`] slot 0.
pub const FLOOR_MESH: usize = 0;
/// The ceiling slab.
pub const CEILING_MESH: usize = 1;
/// The wall across the range, drawn at both ends.
pub const END_WALL_MESH: usize = 2;
/// The wall along the range, drawn on both sides.
pub const SIDE_WALL_MESH: usize = 3;
/// The kerb at the firing line.
pub const KERB_MESH: usize = 4;
/// The post one plate stands on.
pub const POST_MESH: usize = 5;
/// A target plate.
pub const PLATE_MESH: usize = 6;

/// The shell — floor, walls and ceiling. [`SceneDesc::materials`] slot 0, and
/// therefore what an instance placed without a named material would shade
/// through.
pub const SHELL_MATERIAL: usize = 0;
/// Yellow: the firing line, which is the one line on the floor that means
/// something.
pub const LINE_MATERIAL: usize = 1;
/// Dark: a plate's post.
pub const POST_MATERIAL: usize = 2;
/// Pale steel: a plate that is standing, and therefore worth a shot.
pub const PLATE_UP_MATERIAL: usize = 3;
/// Orange: a plate that has been knocked down and is waiting to come back up.
pub const PLATE_DOWN_MATERIAL: usize = 4;

/// What this map reserves, which is a little over what it places.
///
/// Sized against the description rather than left at [`Capacities::default`],
/// for `apps/puppet/src/map.rs`'s reason: that default reserves far more
/// instances than a blockout of thirteen needs, and the level-of-detail state
/// behind that number is a word per instance per draw generator. Filling any of
/// these is a mistake in this file, and the numbers being close to what it uses
/// is what makes that true — `the_map_fits_the_pools_it_reserves` asserts it.
const CAPACITIES: Capacities = Capacities {
    vertices: 4 * 1024,
    indices: 8 * 1024,
    meshes: 8,
    instances: 32,
    materials: 8,
    lights: 8,
    probes: 0,
};

/// A painted greybox material: the metric grid of [`grid_page`], tinted, and
/// tiled **physically** so one tile measures [`GREYBOX_TILE_M`] of surface
/// however large the face is.
///
/// The tint is this map's own and the grid is the engine's. Physical tiling
/// rather than the authored kind [`grid_material`] comes with, because these
/// surfaces are metres across and an authored `0..1` tile would stretch one
/// square over the whole of the floor. It spends the 32² grid page rather than
/// `crcbl_greybox::material`'s 1024² one, because a demo that runs in a browser
/// should not upload eight megatexels to show a ruler.
pub(crate) fn painted(tint: [f32; 3]) -> GpuMaterial {
    GpuMaterial {
        base_color: [tint[0], tint[1], tint[2], 1.0],
        tiling: GpuMaterial::TILING_PHYSICAL,
        tile_metres: GREYBOX_TILE_M,
        ..grid_material()
    }
}

/// Everything this map makes resident: seven meshes, five painted rows and the
/// grid page they sample.
///
/// The mesh and material order is the constants above, in value order; keep
/// them and this assembly in step, which this module's
/// `the_constants_name_their_own_meshes` test asserts.
#[must_use]
pub fn scene() -> SceneDesc<'static> {
    let mesh = |label: &'static str, geometry: Geometry<'static>| MeshDesc {
        label: Cow::Borrowed(label),
        geometry,
    };
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
            // Built along `X` like every other `wall`, and turned a quarter
            // turn about `+Y` where it is placed. One mesh serves both sides.
            mesh(
                "side wall",
                wall(DEPTH as f32, CEILING_Y as f32, WALL_THICKNESS as f32),
            ),
            mesh(
                "firing line",
                platform(
                    2.0 * HALF_WIDTH as f32,
                    KERB_DEPTH as f32,
                    KERB_HEIGHT as f32,
                ),
            ),
            mesh(
                "post",
                wall(POST_EDGE as f32, PLATE_BOTTOM_Y as f32, POST_EDGE as f32),
            ),
            mesh(
                "plate",
                wall(
                    PLATE_WIDTH as f32,
                    PLATE_HEIGHT as f32,
                    PLATE_THICKNESS as f32,
                ),
            ),
        ],
        materials: vec![
            painted([0.29, 0.30, 0.33]),
            painted([0.80, 0.66, 0.18]),
            painted([0.17, 0.18, 0.21]),
            painted([0.62, 0.68, 0.78]),
            painted([0.78, 0.36, 0.12]),
        ],
        page: grid_page(),
        probes: ProbeGrid::default(),
        capacities: CAPACITIES,
    }
}

/// The three plates, as the renderer holds them.
///
/// Handed back by [`place`] because they are the only instances on this map
/// that are ever rewritten: everything else is placed once and drawn for the
/// rest of the run.
#[derive(Debug)]
pub struct Targets {
    plates: [InstanceHandle; LANES],
}

impl Targets {
    /// Draws one plate standing or knocked flat, at `x` across the range.
    ///
    /// The transform and the **material row** both change, so the picture says
    /// which state a plate is in from any angle — a plate seen edge-on is a
    /// line either way round, and a reader should not have to work out which
    /// line it is.
    ///
    /// `x` is the caller's rather than [`plate_x`]'s, because the simulation is
    /// what knows which instant this frame is drawing — see
    /// [`crate::game::RenderState`].
    ///
    /// # Panics
    ///
    /// If `lane` is not a lane. Called only from [`crate::game`]'s own indices,
    /// so an out-of-range one is this sample's bug rather than a state a run
    /// can reach.
    pub fn set_plate(&self, renderer: &mut ForwardRenderer, lane: usize, x: f64, down: bool) {
        renderer.set_instance(
            self.plates[lane],
            &InstanceDesc {
                mesh: PLATE_MESH,
                material: if down {
                    PLATE_DOWN_MATERIAL
                } else {
                    PLATE_UP_MATERIAL
                },
                transform: plate_transform(LANE_LIST[lane], x, down),
            },
        );
    }
}

/// Where a plate's mesh sits, standing or knocked flat.
///
/// A `wall` rises from `y = 0` and is centred on its other two axes, so a
/// standing plate is a translation to its hinge. Knocking it down is a quarter
/// turn about `+X`, which carries the top edge to `-Z` — away from the shooter,
/// the way a plate falls — and lifts it by its own half thickness so it lies
/// *on* the hinge height rather than half inside it.
fn plate_transform(at: Lane, x: f64, down: bool) -> Mat4 {
    let lift = if down { 0.5 * PLATE_THICKNESS } else { 0.0 };
    let hinge = Mat4::from_translation(Vec3::new(
        x as f32,
        (PLATE_BOTTOM_Y + lift) as f32,
        at.z as f32,
    ));
    if down {
        hinge * Mat4::from_rotation_x(-core::f32::consts::FRAC_PI_2)
    } else {
        hinge
    }
}

/// Where a plate's **collider** sits, standing or knocked flat.
///
/// The same two poses `plate_transform` draws, written from the same
/// constants — see the module docs. A downed plate is a flat slab at the hinge
/// height reaching back down-range, which is what puts it under a level shot
/// rather than in front of one.
#[must_use]
pub fn plate_collider(at: Lane, x: f64, down: bool) -> BoxCollider {
    if down {
        BoxCollider::new(
            DVec3::new(
                x,
                PLATE_BOTTOM_Y + 0.5 * PLATE_THICKNESS,
                at.z - 0.5 * PLATE_HEIGHT,
            ),
            DVec3::new(0.5 * PLATE_WIDTH, 0.5 * PLATE_THICKNESS, 0.5 * PLATE_HEIGHT),
        )
    } else {
        BoxCollider::new(
            DVec3::new(x, PLATE_CENTRE_Y, at.z),
            DVec3::new(0.5 * PLATE_WIDTH, 0.5 * PLATE_HEIGHT, 0.5 * PLATE_THICKNESS),
        )
    }
}

/// Places every object on the range and hands back the plates.
///
/// Also sets [`lamps`], which is sticky: the lights are the room's fittings and
/// nothing in this sample moves them, so they are written once here rather than
/// once a frame.
///
/// # Errors
///
/// [`InstancePoolError`] if `CAPACITIES`'s instance count does not cover the
/// map, which is this file's numbers being wrong rather than a condition a run
/// can be in.
pub fn place(renderer: &mut ForwardRenderer) -> Result<Targets, InstancePoolError> {
    let at =
        |x: f64, y: f64, z: f64| Mat4::from_translation(Vec3::new(x as f32, y as f32, z as f32));
    let turned =
        |x: f64| at(x, 0.0, CENTRE_Z) * Mat4::from_rotation_y(core::f32::consts::FRAC_PI_2);
    let wall_offset = HALF_WIDTH + 0.5 * WALL_THICKNESS;

    for (mesh, material, transform) in [
        // A `platform` rises from `y = 0`, so the floor is dropped by its own
        // thickness to put its top there.
        (
            FLOOR_MESH,
            SHELL_MATERIAL,
            at(0.0, -SLAB_THICKNESS, CENTRE_Z),
        ),
        (CEILING_MESH, SHELL_MATERIAL, at(0.0, CEILING_Y, CENTRE_Z)),
        (
            END_WALL_MESH,
            SHELL_MATERIAL,
            at(0.0, 0.0, BUTT_Z - 0.5 * WALL_THICKNESS),
        ),
        (
            END_WALL_MESH,
            SHELL_MATERIAL,
            at(0.0, 0.0, REAR_Z + 0.5 * WALL_THICKNESS),
        ),
        (SIDE_WALL_MESH, SHELL_MATERIAL, turned(-wall_offset)),
        (SIDE_WALL_MESH, SHELL_MATERIAL, turned(wall_offset)),
        (KERB_MESH, LINE_MATERIAL, at(0.0, 0.0, FIRING_LINE_Z)),
    ] {
        renderer.add_instance(&InstanceDesc {
            mesh,
            material,
            transform,
        })?;
    }

    for lane in LANE_LIST {
        renderer.add_instance(&InstanceDesc {
            mesh: POST_MESH,
            material: POST_MATERIAL,
            transform: at(lane.x, 0.0, lane.z),
        })?;
    }

    // The plates last, standing — which is the state [`crate::game`] starts
    // them in, so the first frame agrees with the first tick without either
    // having to ask the other.
    let mut plates = Vec::with_capacity(LANES);
    for (lane, at) in LANE_LIST.iter().enumerate() {
        plates.push(renderer.add_instance(&InstanceDesc {
            mesh: PLATE_MESH,
            material: PLATE_UP_MATERIAL,
            transform: plate_transform(*at, plate_x(lane, 0.0), false),
        })?);
    }

    renderer.set_lights(&lamps());
    Ok(Targets {
        plates: plates
            .try_into()
            .unwrap_or_else(|_| unreachable!("one instance per lane was pushed")),
    })
}

// ---------------------------------------------------------------------------
// The collision side
// ---------------------------------------------------------------------------

/// The same range, as the colliders the capsule sweeps against and the pistol's
/// ray is cast into — with the plates' ids, which are what [`crate::game`]
/// scores a shot against.
///
/// Every one of them is written from the constants the meshes above are written
/// from, which is the whole reason those constants exist.
///
/// # Panics
///
/// Never in practice: the plate ids are collected from [`LANE_LIST`], one per
/// lane, so the array they are handed back in is always full.
#[must_use]
pub fn world() -> (PhysicsWorld, [ColliderId; LANES]) {
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
    world.add_box(end_wall(BUTT_Z - 0.5 * WALL_THICKNESS));
    world.add_box(end_wall(REAR_Z + 0.5 * WALL_THICKNESS));

    let side_wall = |x: f64| {
        BoxCollider::new(
            DVec3::new(x, 0.5 * CEILING_Y, CENTRE_Z),
            DVec3::new(0.5 * WALL_THICKNESS, 0.5 * CEILING_Y, 0.5 * DEPTH),
        )
    };
    world.add_box(side_wall(-(HALF_WIDTH + 0.5 * WALL_THICKNESS)));
    world.add_box(side_wall(HALF_WIDTH + 0.5 * WALL_THICKNESS));

    world.add_box(BoxCollider::new(
        DVec3::new(0.0, 0.5 * KERB_HEIGHT, FIRING_LINE_Z),
        DVec3::new(HALF_WIDTH, 0.5 * KERB_HEIGHT, 0.5 * KERB_DEPTH),
    ));

    for lane in LANE_LIST {
        world.add_box(BoxCollider::new(
            DVec3::new(lane.x, 0.5 * PLATE_BOTTOM_Y, lane.z),
            DVec3::new(0.5 * POST_EDGE, 0.5 * PLATE_BOTTOM_Y, 0.5 * POST_EDGE),
        ));
    }

    let mut plates = Vec::with_capacity(LANES);
    for (lane, at) in LANE_LIST.iter().enumerate() {
        plates.push(world.add_box(plate_collider(*at, plate_x(lane, 0.0), false)));
    }
    let plates = plates
        .try_into()
        .unwrap_or_else(|_| unreachable!("one collider per lane was pushed"));
    (world, plates)
}

// ---------------------------------------------------------------------------
// The light
// ---------------------------------------------------------------------------

/// How bright a ceiling lamp is, before its colour.
///
/// Well above 1.0, like every other light in this engine: the scene target is
/// `Rgba16Float` and the tonemap pass is what brings it back.
const LAMP_INTENSITY: f32 = 24.0;

/// How far a lamp reaches, in metres. Far enough that the pools of two
/// neighbours meet, so the range is lit along its whole length rather than in
/// spots.
const LAMP_RADIUS: f32 = 16.0;

/// How far under the ceiling a lamp hangs, in metres.
const LAMP_DROP: f64 = 0.3;

/// Where the lamps hang, in metres along `Z` — one behind the shooter and one
/// over each lane's plate, so every target is lit from above rather than from
/// the shooter's end.
const LAMP_Z: [f64; 4] = [SPAWN_Z, LANE_LIST[0].z, LANE_LIST[1].z, LANE_LIST[2].z];

/// The room's fittings: a row of point lights under the ceiling.
///
/// `docs/plan/18-render-features.md`'s light list, which is what an indoor
/// scene with a ceiling actually needs — see the module docs for why the sun
/// row cannot do this job.
#[must_use]
pub fn lamps() -> Vec<Light> {
    LAMP_Z
        .iter()
        .map(|&z| {
            Light::Point(PointLight {
                position: Vec3::new(0.0, (CEILING_Y - LAMP_DROP) as f32, z as f32),
                radius: LAMP_RADIUS,
                color: Vec3::new(1.0, 0.97, 0.92) * LAMP_INTENSITY,
            })
        })
        .collect()
}

/// The sun row, which in here is the ambient and almost nothing else.
///
/// [`begin_frame`](crcbl::render::ForwardRenderer::begin_frame) takes a
/// [`DirectionalLight`] whatever the scene is, and it is the light that owns
/// the ambient term. A room with a ceiling has no sun, so the directional part
/// is a token — enough to keep the shadow cascades pointing somewhere sane —
/// and the ambient is what stops a face no lamp reaches from being black.
#[must_use]
pub fn house_light() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::new(0.0, 1.0, 0.0),
        color: Vec3::splat(0.02),
        ambient: Vec3::new(0.06, 0.065, 0.08),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every mesh the description makes resident is placed, and every row it
    /// declares is named**, in the order the constants say.
    ///
    /// A mesh nothing places is memory taken for geometry no frame draws, and a
    /// row nothing names is a colour nobody can see — both of which leave a
    /// perfectly plausible picture.
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
                KERB_MESH,
                POST_MESH,
                PLATE_MESH,
            ]
            .map(|mesh| labels[mesh]),
            [
                "floor",
                "ceiling",
                "end wall",
                "side wall",
                "firing line",
                "post",
                "plate",
            ],
        );
        assert_eq!(scene.meshes.len(), PLATE_MESH + 1);
        assert_eq!(
            scene.materials.len(),
            5,
            "five painted rows, one per constant",
        );
        for row in [
            SHELL_MATERIAL,
            LINE_MATERIAL,
            POST_MATERIAL,
            PLATE_UP_MATERIAL,
            PLATE_DOWN_MATERIAL,
        ] {
            assert_eq!(
                scene.materials[row].tiling,
                GpuMaterial::TILING_PHYSICAL,
                "row {row} must measure its grid in metres, not in its own UV",
            );
        }
    }

    /// **The firing line is a step the controller refuses**, which is the whole
    /// of what keeps the player back — there is no position check anywhere in
    /// this sample.
    #[test]
    fn the_firing_line_is_over_the_offset_the_controller_climbs() {
        let config = crcbl::phys::CharacterConfig::default();
        assert!(
            KERB_HEIGHT > config.step_offset + config.skin_width,
            "a {KERB_HEIGHT} m kerb is inside the {} m the controller steps over",
            config.step_offset,
        );
    }

    /// **A plate stands where the player looks**, so a level shot down the near
    /// lane hits it. The two constants live in two modules and nothing but this
    /// holds them together.
    #[test]
    fn the_plates_stand_at_the_height_the_player_looks_from() {
        assert_eq!(PLATE_CENTRE_Y, EYE_HEIGHT);
        const { assert!(PLATE_CENTRE_Y + 0.5 * PLATE_HEIGHT < CEILING_Y) };
        // And the near lane is the one straight ahead of the spawn, which is
        // what makes the first trigger pull mean something.
        assert_eq!(LANE_LIST[0].x, SPAWN.x);
        assert!(LANE_LIST[0].z < FIRING_LINE_Z);
        // The lanes are ordered near to far, which the panel and the heartbeat
        // both rely on when they name "the nearest lane".
        assert!(LANE_LIST[0].distance() < LANE_LIST[1].distance());
        assert!(LANE_LIST[1].distance() < LANE_LIST[2].distance());
    }

    /// **A knocked-down plate is out of a level shot's way**, which is what
    /// makes "a shot at a plate that is already down is a miss" a fact about
    /// the geometry rather than a rule in [`crate::game`].
    #[test]
    fn a_downed_plate_lies_under_the_line_a_level_shot_takes() {
        for (index, lane) in LANE_LIST.iter().enumerate() {
            let lane = *lane;
            let x = plate_x(index, 0.0);
            let down = plate_collider(lane, x, true);
            let top = down.centre.y + down.half_extents.y;
            assert!(
                top < PLATE_CENTRE_Y - 0.05,
                "a downed plate in the {} lane reaches {top:.3} m, and the eye is at \
                 {PLATE_CENTRE_Y:.3} m",
                lane.label,
            );
            // It falls away from the shooter rather than toward them.
            assert!(down.centre.z < lane.z);
            // And it is the same plate: knocking one over does not change how
            // much steel there is.
            let up = plate_collider(lane, x, false);
            let volume = |b: &BoxCollider| b.half_extents.x * b.half_extents.y * b.half_extents.z;
            assert!((volume(&up) - volume(&down)).abs() < 1e-12);
        }
    }

    /// **The room is a closed box the player is inside**, checked by dropping
    /// the controller on the spawn and reading where it settled. A floor a
    /// decimetre from its mesh would look walkable and drop the player through
    /// it.
    #[test]
    fn the_spawn_has_the_floor_of_this_room_under_it() {
        let (mut world, plates) = world();
        assert_eq!(plates.len(), LANES);
        let config = crcbl::phys::CharacterConfig::default();
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
        // And the spawn is behind the firing line with the range in front of
        // it, which is what the whole sample is arranged around.
        const { assert!(SPAWN.z > FIRING_LINE_Z) };
        const { assert!(SPAWN.z < REAR_Z) };
    }

    /// **The map fits the pools it reserves**, which is what
    /// [`CAPACITIES`] being sized against the description rather than left at
    /// the default costs: a mesh or an instance added above without a number
    /// raised here is a pool refusal at start-up on every machine.
    ///
    /// Counted off the description and the placement rather than restated, so a
    /// row added to either is a row this test sees.
    #[test]
    fn the_map_fits_the_pools_it_reserves() {
        let scene = scene();
        assert!(
            scene.meshes.len() <= CAPACITIES.meshes as usize,
            "{} meshes in a pool of {}",
            scene.meshes.len(),
            CAPACITIES.meshes,
        );
        assert!(
            scene.materials.len() <= CAPACITIES.materials as usize,
            "{} material rows in a table of {}",
            scene.materials.len(),
            CAPACITIES.materials,
        );
        // Seven pieces of shell, one post per lane and one plate per lane.
        let instances = 7 + 2 * LANES;
        assert!(
            instances <= CAPACITIES.instances as usize,
            "{instances} instances in a pool of {}",
            CAPACITIES.instances,
        );
        assert!(
            lamps().len() < CAPACITIES.lights as usize,
            "{} lamps beside the sun, in a list of {}",
            lamps().len(),
            CAPACITIES.lights,
        );
    }

    /// **The travelling plate stays in its own half of the range**, which is
    /// what makes it a mover rather than a thing that walks into the wall and
    /// through the shot down the middle.
    ///
    /// Swept over a whole period rather than checked at two instants: a sine is
    /// exactly the shape whose extremes a two-point check misses.
    #[test]
    fn the_travelling_plate_stays_in_its_own_half_of_the_range() {
        let inner = HALF_WIDTH - 0.5 * PLATE_WIDTH;
        let mut nearest = f64::INFINITY;
        let mut furthest = 0.0f64;
        for step in 0..=720 {
            let seconds = MOVER_PERIOD_S * f64::from(step) / 720.0;
            let x = plate_x(MOVER_LANE, seconds);
            nearest = nearest.min(x - 0.5 * PLATE_WIDTH);
            furthest = furthest.max(x + 0.5 * PLATE_WIDTH);
            // The lanes that do not travel do not travel.
            for (lane, at) in LANE_LIST.iter().enumerate() {
                if lane != MOVER_LANE {
                    assert_eq!(plate_x(lane, seconds), at.x);
                }
            }
        }
        assert!(
            furthest <= inner,
            "the mover reaches x = {furthest:.2}, past the wall at {HALF_WIDTH}",
        );
        assert!(
            nearest > 0.5 * PLATE_WIDTH,
            "the mover reaches x = {nearest:.2}, into the shot down the middle",
        );
        // And it does travel: a mover that never moved would pass both bounds.
        assert!(
            furthest - nearest > MOVER_SWEEP_M,
            "it only covered {:.2} m",
            furthest - nearest,
        );
        // A whole period comes back to where it started, so the sweep is a
        // cycle rather than a drift.
        assert!((plate_x(MOVER_LANE, MOVER_PERIOD_S) - plate_x(MOVER_LANE, 0.0)).abs() < 1e-9);
    }

    /// **The plate ids the world hands back are the plates**, read by casting a
    /// level ray down the near lane. An array assembled in the wrong order
    /// would score every hit against the wrong target and nothing else here
    /// would see it.
    #[test]
    fn the_plate_ids_are_the_plates_a_shot_down_a_lane_finds() {
        let (mut world, plates) = world();
        for (lane, plate) in LANE_LIST.iter().zip(plates) {
            let eye = DVec3::new(lane.x, PLATE_CENTRE_Y, SPAWN_Z);
            let ray = crcbl::phys::Ray::new(eye, DVec3::new(0.0, 0.0, -1.0));
            let (hit, _) = world.cast_ray(&ray).expect("the butt is down there");
            assert_eq!(
                hit, plate,
                "a level shot down the {} lane hit something else",
                lane.label,
            );
        }
    }
}
