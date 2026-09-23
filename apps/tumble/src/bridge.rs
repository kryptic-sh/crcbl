//! The Bridge room, rung 5's proving scenes: a gapped Newton's cradle, a
//! plank bridge with crates dropped on it, and capsule ragdolls down stairs.
//!
//! ```text
//!    ═══════ bar             post ▐                          ▌ post
//!    ╲╱ ╲╱ ╲╱ ╲╱ ╲╱               ▐▬╮                      ╭▬▌
//!    ○  ○  ○  ○  ○  cradle          ╰▬╮  crates dropped  ╭▬╯
//!    ← drawn back, let go             ╰▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬▬╯  21 hinged planks
//!                                          ▣                 (their group at
//!                        ragdolls ☺☺ ▁▁                       eight substeps)
//!                                      ▔▔▁▁
//!                                          ▔▔▁▁  stairs: a triangle mesh
//! ```
//!
//! **The cradle** is five steel balls, 2.5 cm apart, each hung on two rigid
//! distance joints from a bar; the first is drawn back and let go every
//! [`CRADLE_EVERY_TICKS`]. The gap is what lets this solver pass the momentum:
//! see `crates/crcbl-phys/tests/joints.rs`. Its counters are the momentum the
//! first ball arrives with and the momentum the last one leaves with.
//!
//! **The bridge** is twenty-one planks hinged end to end between two posts,
//! laid in the hanging chain's equilibrium and asking for
//! [`BRIDGE_SUBSTEPS`] substeps for their group — decision 1's "a long chain
//! or bridge gets more substeps for its group". A crate is dropped on its
//! left half every [`CRATE_EVERY_TICKS`] and slides down to the middle; late
//! in each cycle an anvil is dropped on it, heavier than its weak link's
//! [`HINGE_BREAKS_AT`], which snaps — the broken joints are rung 5's counter
//! — and every [`BRIDGE_EVERY_TICKS`] it is built again. Its counters are the
//! sag of its middle plank against the unloaded chain's, and the joints it
//! has lost.
//!
//! **The ragdolls** are ten capsules and a ball each — ball joints with cone
//! and twist limits at the neck, waist, shoulders and hips, hinges with
//! limits at the elbows and knees — pushed off the top of a flight of stairs
//! that is one static [`TriangleMesh`], and stood back up every
//! [`RAGDOLL_EVERY_TICKS`].
//!
//! Every joint in the room is counted in the joint error on the panel: the
//! worst any joint drifted from holding on the last tick.

use std::collections::VecDeque;

use crcbl::ecs::{Entity, SystemTrait as _};
use crcbl::math::{DQuat, DVec3};
use crcbl::phys::{
    ColliderComponent, ContactSettings, DistanceJoint, GravityForce, Joint, JointId, JointKind,
    MassProperties, PhysicsSystem, RevoluteJoint, RigidBody, SphericalJoint, SurfaceMaterial,
    Transform, TriangleMesh, rotation_from_scaled_axis,
};

use crate::scene::{GRAVITY, Room, Shape, Tally, Tint, entity};

/// Where the room stands: the middle of its floor.
pub const BRIDGE_AT: DVec3 = DVec3::new(92.0, 0.0, 0.0);

/// Box2D's default friction, and no bounce.
const SURFACE: SurfaceMaterial = SurfaceMaterial::new(0.6, 0.0);
/// The cradle's steel: no friction to spin the balls, and a perfect bounce.
const STEEL: SurfaceMaterial = SurfaceMaterial::new(0.0, 1.0);
/// A crate's: against a plank's 0.6 the pair averages 0.375, slippery enough to
/// slide down the bridge's sag to its middle,
/// where 0.6 held one on the slope it was set down on and the next was set
/// down inside it.
const CRATE_SURFACE: SurfaceMaterial = SurfaceMaterial::new(0.15, 0.0);

// ---- the cradle ---------------------------------------------------------------

/// The cradle's balls.
pub const CRADLE_BALLS: u32 = 5;
/// A ball's radius, in metres.
pub const CRADLE_RADIUS: f64 = 0.1;
/// The gap between two balls: wider than the contact pipeline's speculative
/// distance, so each impact is a collision of its own.
const CRADLE_GAP: f64 = 0.025;
/// How far each ball hangs under its bar, in metres.
const CRADLE_DROP: f64 = 1.0;
/// How far out along the bar each ball's two rods are hung, either side.
const CRADLE_SPREAD: f64 = 0.3;
/// The bar's height.
const CRADLE_TOP: f64 = 2.2;
/// The first ball's leftmost place, from the room's middle.
const CRADLE_AT: DVec3 = DVec3::new(-10.5, 0.0, 0.0);
/// How high the first ball is drawn back, in metres.
pub const CRADLE_LIFT: f64 = 0.2;
/// How often the first ball is drawn back again, in ticks.
pub const CRADLE_EVERY_TICKS: u64 = 480;

// ---- the bridge ---------------------------------------------------------------

/// The bridge's planks.
pub const PLANKS: u32 = 21;
/// A plank's half-extents: half a metre along the bridge, a metre across.
pub const PLANK_HALF: DVec3 = DVec3::new(0.25, 0.05, 0.5);
/// A plank's mass, in kilograms.
const PLANK_MASS: f64 = 5.0;
/// Between the posts, in metres: a metre short of the planks laid end to
/// end, so the bridge hangs.
pub const SPAN: f64 = 9.5;
/// The height the bridge hangs from.
const BRIDGE_TOP: f64 = 4.0;
/// The substeps the planks ask for.
pub const BRIDGE_SUBSTEPS: u32 = 8;
/// The force at which the bridge's weak link breaks, in newtons: six times
/// what the crates sliding across put on any hinge, and under what the anvil
/// does. Measured on 2026-09-23, the crates put 2.5 kN on a hinge at most and
/// the anvil 20 kN on every one — a chain carries its tension end to end, so
/// a threshold on every hinge snapped all twenty-two in two ticks.
pub const HINGE_BREAKS_AT: f64 = 15_000.0;
/// The hinge that breaks: the one at the middle plank's far end. The rest
/// never do.
pub const WEAK_HINGE: usize = PLANKS as usize / 2 + 1;
/// How far each post stands clear of the point the bridge hangs from, so a
/// broken half hanging straight down misses it.
const POST_GAP: f64 = 0.1;
/// A post's half-extents.
const POST_HALF: DVec3 = DVec3::new(0.15, 0.5 * BRIDGE_TOP, 0.6);
/// A crate's half-extent and mass.
const CRATE_HALF: f64 = 0.2;
const CRATE_MASS: f64 = 20.0;
/// The anvil's half-extent and mass.
const ANVIL_HALF: f64 = 0.3;
const ANVIL_MASS: f64 = 800.0;
/// How far above the planks a crate and the anvil are let go, in metres.
const CRATE_DROP: f64 = 0.02;
const ANVIL_DROP: f64 = 2.0;
/// The plank a crate is set down on, near the bridge's left end, to slide
/// down to its middle.
const CRATE_PLANK: usize = 1;
/// How often a crate is dropped on the bridge, in ticks.
pub const CRATE_EVERY_TICKS: u64 = 150;
/// The most crates on the bridge at once; the oldest goes when another comes.
pub const MAX_CRATES: usize = 5;
/// How often the bridge is built again, in ticks.
pub const BRIDGE_EVERY_TICKS: u64 = 960;
/// The tick of each cycle the anvil drops on.
pub const ANVIL_AT_TICK: u64 = 720;

// ---- the ragdolls -------------------------------------------------------------

/// The stairs' steps.
const STEPS: u32 = 6;
/// A tread's depth and a riser's height, in metres.
const TREAD: f64 = 0.5;
const RISE: f64 = 0.25;
/// Where the stairs' top landing ends and the first step down begins, from
/// the room's middle.
const STAIRS_AT: DVec3 = DVec3::new(3.0, 0.0, 4.5);
/// The stairs' lane, across `z`, from its middle.
const STAIRS_HALF_WIDTH: f64 = 1.5;
/// The landing's depth behind the first step, along `-x`.
const LANDING: f64 = -1.5;
/// The ragdolls.
pub const RAGDOLLS: u32 = 2;
/// The substeps the ragdolls' parts ask for.
pub const RAGDOLL_SUBSTEPS: u32 = 8;
/// How fast a ragdoll is pushed off the landing, in m/s.
const RAGDOLL_PUSH: f64 = 3.0;
/// How often the ragdolls are stood back up, in ticks.
pub const RAGDOLL_EVERY_TICKS: u64 = 360;

/// Entity indices: the cradle's anchors, its balls, the posts, the stairs,
/// the planks, the ragdolls' parts, and the crates cycling through theirs.
const FIRST_CRADLE_ANCHOR: u32 = 0;
const FIRST_BALL: u32 = 20;
const POSTS: [u32; 2] = [30, 31];
const STAIRS: u32 = 40;
const FIRST_PLANK: u32 = 100;
const FIRST_RAGDOLL: u32 = 200;
const FIRST_CRATE: u32 = 10_000;
const CRATE_IDS: u64 = 1_000_000;

/// A ragdoll's parts: the name, the capsule's radius and core half-height
/// (none for the head, a ball), its middle standing, and its mass.
struct Part {
    radius: f64,
    half_height: Option<f64>,
    at: DVec3,
    mass: f64,
}

/// A standing ragdoll's parts, from its feet, facing `+x`: pelvis, chest,
/// head, the arms and the legs.
const PARTS: [Part; 11] = [
    Part {
        radius: 0.12,
        half_height: Some(0.05),
        at: DVec3::new(0.0, 1.00, 0.0),
        mass: 12.0,
    },
    Part {
        radius: 0.15,
        half_height: Some(0.14),
        at: DVec3::new(0.0, 1.38, 0.0),
        mass: 20.0,
    },
    Part {
        radius: 0.11,
        half_height: None,
        at: DVec3::new(0.0, 1.79, 0.0),
        mass: 5.0,
    },
    Part {
        radius: 0.05,
        half_height: Some(0.12),
        at: DVec3::new(0.0, 1.38, -0.24),
        mass: 2.5,
    },
    Part {
        radius: 0.045,
        half_height: Some(0.11),
        at: DVec3::new(0.0, 1.06, -0.24),
        mass: 1.8,
    },
    Part {
        radius: 0.05,
        half_height: Some(0.12),
        at: DVec3::new(0.0, 1.38, 0.24),
        mass: 2.5,
    },
    Part {
        radius: 0.045,
        half_height: Some(0.11),
        at: DVec3::new(0.0, 1.06, 0.24),
        mass: 1.8,
    },
    Part {
        radius: 0.07,
        half_height: Some(0.15),
        at: DVec3::new(0.0, 0.70, -0.1),
        mass: 8.0,
    },
    Part {
        radius: 0.055,
        half_height: Some(0.14),
        at: DVec3::new(0.0, 0.27, -0.1),
        mass: 4.0,
    },
    Part {
        radius: 0.07,
        half_height: Some(0.15),
        at: DVec3::new(0.0, 0.70, 0.1),
        mass: 8.0,
    },
    Part {
        radius: 0.055,
        half_height: Some(0.14),
        at: DVec3::new(0.0, 0.27, 0.1),
        mass: 4.0,
    },
];

/// What a ragdoll's joint is: which parts, where it is standing, which way
/// the child hangs from it, and what it lets through.
struct Link {
    parent: usize,
    child: usize,
    at: DVec3,
    kind: LinkKind,
}

enum LinkKind {
    /// A ball joint whose cone is about the child's direction, `up` or not.
    Ball { up: bool, cone: f64, twist: f64 },
    /// A hinge about `z`, across the body, bending between the two angles.
    Hinge { lower: f64, upper: f64 },
}

const LINKS: [Link; 10] = [
    // Waist, neck.
    Link {
        parent: 0,
        child: 1,
        at: DVec3::new(0.0, 1.17, 0.0),
        kind: LinkKind::Ball {
            up: true,
            cone: 0.5,
            twist: 0.4,
        },
    },
    Link {
        parent: 1,
        child: 2,
        at: DVec3::new(0.0, 1.66, 0.0),
        kind: LinkKind::Ball {
            up: true,
            cone: 0.6,
            twist: 0.8,
        },
    },
    // Shoulders, elbows.
    Link {
        parent: 1,
        child: 3,
        at: DVec3::new(0.0, 1.55, -0.24),
        kind: LinkKind::Ball {
            up: false,
            cone: 1.4,
            twist: 0.6,
        },
    },
    Link {
        parent: 3,
        child: 4,
        at: DVec3::new(0.0, 1.22, -0.24),
        kind: LinkKind::Hinge {
            lower: 0.0,
            upper: 2.3,
        },
    },
    Link {
        parent: 1,
        child: 5,
        at: DVec3::new(0.0, 1.55, 0.24),
        kind: LinkKind::Ball {
            up: false,
            cone: 1.4,
            twist: 0.6,
        },
    },
    Link {
        parent: 5,
        child: 6,
        at: DVec3::new(0.0, 1.22, 0.24),
        kind: LinkKind::Hinge {
            lower: 0.0,
            upper: 2.3,
        },
    },
    // Hips, knees.
    Link {
        parent: 0,
        child: 7,
        at: DVec3::new(0.0, 0.92, -0.1),
        kind: LinkKind::Ball {
            up: false,
            cone: 0.9,
            twist: 0.3,
        },
    },
    Link {
        parent: 7,
        child: 8,
        at: DVec3::new(0.0, 0.48, -0.1),
        kind: LinkKind::Hinge {
            lower: -2.3,
            upper: 0.0,
        },
    },
    Link {
        parent: 0,
        child: 9,
        at: DVec3::new(0.0, 0.92, 0.1),
        kind: LinkKind::Ball {
            up: false,
            cone: 0.9,
            twist: 0.3,
        },
    },
    Link {
        parent: 9,
        child: 10,
        at: DVec3::new(0.0, 0.48, 0.1),
        kind: LinkKind::Hinge {
            lower: -2.3,
            upper: 0.0,
        },
    },
];

/// What the Bridge room's counters read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BridgeReading {
    /// The momentum the cradle's first ball arrived with, this swing, in
    /// kg·m/s.
    pub cradle_in: f64,
    /// The momentum its last ball left with, this swing.
    pub cradle_out: f64,
    /// Swings so far, the current one included.
    pub cradle_swings: u64,
    /// How far the bridge's middle plank hangs below its posts, in metres.
    pub sag: f64,
    /// How far the unloaded chain's middle hangs, which it was laid at.
    pub chain_sag: f64,
    /// Crates on it.
    pub crates: usize,
    /// Its hinges still whole.
    pub hinges: usize,
    /// Times it has been built, the current one included.
    pub builds: u64,
    /// Joints broken over the run — rung 5's "broken joints".
    pub broken: u64,
    /// Times the ragdolls have been pushed off, the current one included.
    pub ragdoll_drops: u64,
    /// The room's contact and joint counters.
    pub contacts: Tally,
}

/// The Bridge room.
#[derive(Debug)]
pub struct Bridge {
    phys: PhysicsSystem,
    balls: Vec<Entity>,
    /// Each rod: its anchor and its ball.
    rods: Vec<(Entity, Entity)>,
    planks: Vec<Entity>,
    /// Where each plank was laid.
    plank_homes: Vec<Transform>,
    /// Every hinge of the bridge, as laid, and its id while it is whole.
    hinges: Vec<(Joint, Option<JointId>)>,
    chain_sag: f64,
    crates: VecDeque<(Entity, f64)>,
    dropped: u64,
    ragdolls: Vec<Vec<Entity>>,
    cradle_in: f64,
    cradle_out: f64,
    cradle_swings: u64,
    builds: u64,
    broken: u64,
    ragdoll_drops: u64,
    tick: u64,
    tally: Tally,
}

impl Default for Bridge {
    fn default() -> Self {
        Self::new()
    }
}

/// The rotation about z whose cosine and sine these are.
fn about_z(cos: f64, sin: f64) -> DQuat {
    let half_cos = (0.5 * (1.0 + cos)).sqrt();
    let half_sin = (0.5 * (1.0 - cos)).sqrt().copysign(sin);
    DQuat::from_xyzw(0.0, 0.0, half_sin, half_cos)
}

/// The slopes, as `tan θ`, of a chain of rigid links of `length` hinged end
/// to end between two anchors `span` apart at one height, each weighing
/// `weights[i]` at its middle, in static equilibrium.
///
/// Every link carries the one horizontal tension `H`, and its moment about
/// its left hinge sets `tan θᵢ = (Vᵢ + wᵢ / 2) / H`, `Vᵢ` the vertical force
/// at that hinge, which starts at minus half the whole weight and gains each
/// link's weight in turn; `H` is found by bisection so the links span the
/// gap. `crates/crcbl-phys/tests/joints.rs` holds the simulated bridge to
/// the same algebra.
fn chain_slopes(weights: &[f64], length: f64, span: f64) -> Vec<f64> {
    let total: f64 = weights.iter().sum();
    let slopes = |tension: f64| -> Vec<f64> {
        let mut vertical = -0.5 * total;
        weights
            .iter()
            .map(|&w| {
                let slope = (vertical + 0.5 * w) / tension;
                vertical += w;
                slope
            })
            .collect()
    };
    let reach = |tension: f64| -> f64 {
        slopes(tension)
            .iter()
            .map(|t| length / (1.0 + t * t).sqrt())
            .sum()
    };
    let (mut low, mut high) = (1.0e-6 * total, 1.0e6 * total);
    for _ in 0..200 {
        let mid = 0.5 * (low + high);
        if reach(mid) < span {
            low = mid;
        } else {
            high = mid;
        }
    }
    slopes(0.5 * (low + high))
}

fn place_body(
    phys: &mut PhysicsSystem,
    e: Entity,
    at: Transform,
    collider: &ColliderComponent,
    mass: f64,
    material: SurfaceMaterial,
) {
    let inertia = MassProperties::of_collider(collider, mass).inertia;
    phys.set_body(e, RigidBody::new_dynamic(mass).with_inertia(inertia));
    phys.set_transform(e, at);
    phys.set_collider(e, collider, &at);
    phys.set_material(e, material);
}

fn box_collider(half: DVec3) -> ColliderComponent {
    ColliderComponent::Box {
        offset: DVec3::ZERO,
        half_extents: half,
        is_trigger: false,
    }
}

/// The stairs: a landing, [`STEPS`] steps down `+x`, their last tread the
/// floor itself, as one mesh in the world's frame.
fn stairs_mesh() -> TriangleMesh {
    let (z0, z1) = (-STAIRS_HALF_WIDTH, STAIRS_HALF_WIDTH);
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    // A quad counter-clockwise seen from the side it faces, as two triangles.
    let mut quad = |corners: [DVec3; 4]| {
        let base = u32::try_from(vertices.len()).expect("a few quads");
        vertices.extend(corners.map(|p| p + BRIDGE_AT + STAIRS_AT));
        triangles.extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
    };
    // A tread at height `y` over `x0..x1`, facing up.
    let tread = |y: f64, x0: f64, x1: f64| {
        [
            DVec3::new(x0, y, z0),
            DVec3::new(x0, y, z1),
            DVec3::new(x1, y, z1),
            DVec3::new(x1, y, z0),
        ]
    };
    let top = f64::from(STEPS) * RISE;
    quad(tread(top, LANDING, 0.0));
    for i in 0..STEPS {
        let x = f64::from(i) * TREAD;
        let high = top - f64::from(i) * RISE;
        let low = high - RISE;
        // The riser, facing down the stairs.
        quad([
            DVec3::new(x, high, z0),
            DVec3::new(x, high, z1),
            DVec3::new(x, low, z1),
            DVec3::new(x, low, z0),
        ]);
        if low > 0.0 {
            quad(tread(low, x, x + TREAD));
        }
    }
    TriangleMesh::new(&vertices, &triangles).expect("the stairs are a valid mesh")
}

impl Bridge {
    /// Everything at its start: the cradle drawn back, the bridge laid, the
    /// ragdolls standing on the landing.
    #[must_use]
    pub fn new() -> Self {
        let mut phys = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        phys.add_plane(DVec3::Y, 0.0, SURFACE);

        // The cradle.
        let pitch = 2.0 * CRADLE_RADIUS + CRADLE_GAP;
        let mut balls = Vec::new();
        let mut rods = Vec::new();
        let ball_collider = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: CRADLE_RADIUS,
            is_trigger: false,
        };
        for k in 0..CRADLE_BALLS {
            let x = pitch * f64::from(k);
            let rest = BRIDGE_AT + CRADLE_AT + DVec3::new(x, CRADLE_TOP - CRADLE_DROP, 0.0);
            let ball = entity(FIRST_BALL + k);
            place_body(
                &mut phys,
                ball,
                Transform::from_position(rest),
                &ball_collider,
                1.0,
                STEEL,
            );
            for (side, z) in [(0, -CRADLE_SPREAD), (1, CRADLE_SPREAD)] {
                let hook = entity(FIRST_CRADLE_ANCHOR + 2 * k + side);
                let at = BRIDGE_AT + CRADLE_AT + DVec3::new(x, CRADLE_TOP, z);
                phys.set_transform(hook, Transform::from_position(at));
                phys.add_joint(Joint::new(
                    hook,
                    ball,
                    Transform::IDENTITY,
                    Transform::IDENTITY,
                    JointKind::Distance(DistanceJoint::rigid((at - rest).length())),
                ))
                .expect("a valid rod");
                rods.push((hook, ball));
            }
            balls.push(ball);
        }

        // The posts, which the bridge hangs from.
        let posts: Vec<Entity> = [-1.0, 1.0]
            .iter()
            .zip(POSTS)
            .map(|(&side, index)| {
                let e = entity(index);
                let at = Transform::from_position(
                    BRIDGE_AT
                        + DVec3::new(
                            side * (0.5 * SPAN + POST_GAP + POST_HALF.x),
                            POST_HALF.y,
                            0.0,
                        ),
                );
                phys.set_transform(e, at);
                phys.set_collider(e, &box_collider(POST_HALF), &at);
                phys.set_material(e, SURFACE);
                e
            })
            .collect();

        // The bridge, laid in the unloaded chain's equilibrium.
        let slopes = chain_slopes(
            &vec![PLANK_MASS * GRAVITY; PLANKS as usize],
            2.0 * PLANK_HALF.x,
            SPAN,
        );
        let left = BRIDGE_AT + DVec3::new(-0.5 * SPAN, BRIDGE_TOP, 0.0);
        let mut hinge = left;
        let mut planks = Vec::new();
        let mut plank_homes = Vec::new();
        let mut hinge_points = vec![left];
        for (k, slope) in (0u32..).zip(&slopes) {
            let cos = 1.0 / (1.0 + slope * slope).sqrt();
            let direction = DVec3::new(cos, slope * cos, 0.0);
            let home = Transform::new(hinge + direction * PLANK_HALF.x, about_z(cos, slope * cos));
            let e = entity(FIRST_PLANK + k);
            place_body(
                &mut phys,
                e,
                home,
                &box_collider(PLANK_HALF),
                PLANK_MASS,
                SURFACE,
            );
            phys.set_substeps(e, BRIDGE_SUBSTEPS);
            planks.push(e);
            plank_homes.push(home);
            hinge += direction * 2.0 * PLANK_HALF.x;
            hinge_points.push(hinge);
        }
        let middle = &plank_homes[PLANKS as usize / 2];
        let chain_sag = BRIDGE_TOP - (middle.position.y - BRIDGE_AT.y);
        let mut chain = vec![posts[0]];
        chain.extend(&planks);
        chain.push(posts[1]);
        let mut hinges = Vec::new();
        for (k, pair) in chain.windows(2).enumerate() {
            let at = |e: Entity| *phys.transform(e).expect("placed");
            let joint = Joint::at(
                pair[0],
                &at(pair[0]),
                pair[1],
                &at(pair[1]),
                Transform::from_position(hinge_points[k]),
                JointKind::Revolute(RevoluteJoint::hinge()),
            )
            .breaking_at(
                if k == WEAK_HINGE {
                    HINGE_BREAKS_AT
                } else {
                    f64::INFINITY
                },
                f64::INFINITY,
            );
            let id = phys.add_joint(joint).expect("a valid hinge");
            hinges.push((joint, Some(id)));
        }

        // The stairs.
        let stairs = entity(STAIRS);
        phys.set_transform(stairs, Transform::IDENTITY);
        phys.set_collider(
            stairs,
            &ColliderComponent::Mesh {
                mesh: stairs_mesh(),
                is_trigger: false,
            },
            &Transform::IDENTITY,
        );
        phys.set_material(stairs, SURFACE);

        let mut room = Self {
            phys,
            balls,
            rods,
            planks,
            plank_homes,
            hinges,
            chain_sag,
            crates: VecDeque::new(),
            dropped: 0,
            ragdolls: Vec::new(),
            cradle_in: 0.0,
            cradle_out: 0.0,
            cradle_swings: 0,
            builds: 1,
            broken: 0,
            ragdoll_drops: 0,
            tick: 0,
            tally: Tally::default(),
        };
        room.build_ragdolls();
        room.draw_back();
        room.push_ragdolls();
        room
    }

    /// Where ragdoll `r` stands, its feet on the landing.
    fn ragdoll_origin(r: u32) -> DVec3 {
        let across = (f64::from(r) - 0.5 * f64::from(RAGDOLLS - 1)) * 1.4;
        BRIDGE_AT + STAIRS_AT + DVec3::new(-0.6, f64::from(STEPS) * RISE, across)
    }

    /// Each part's collider.
    fn part_collider(part: &Part) -> ColliderComponent {
        match part.half_height {
            Some(half_height) => ColliderComponent::Capsule {
                offset: DVec3::ZERO,
                radius: part.radius,
                half_height,
                is_trigger: false,
            },
            None => ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: part.radius,
                is_trigger: false,
            },
        }
    }

    /// The ragdolls' parts and joints.
    fn build_ragdolls(&mut self) {
        // A frame whose z-axis points up, or down: a ball joint's cone is
        // about the frames' z-axes.
        let up = rotation_from_scaled_axis(DVec3::X * -core::f64::consts::FRAC_PI_2);
        let down = rotation_from_scaled_axis(DVec3::X * core::f64::consts::FRAC_PI_2);
        // A hinge's axis is z: across the body, along `z` here, so the
        // limbs bend in the plane they are pushed in.
        let across = DQuat::IDENTITY;
        for r in 0..RAGDOLLS {
            let origin = Self::ragdoll_origin(r);
            let parts: Vec<Entity> = (0u32..)
                .zip(&PARTS)
                .map(|(k, part)| {
                    let e = entity(FIRST_RAGDOLL + 20 * r + k);
                    place_body(
                        &mut self.phys,
                        e,
                        Transform::from_position(origin + part.at),
                        &Self::part_collider(part),
                        part.mass,
                        SURFACE,
                    );
                    self.phys.set_substeps(e, RAGDOLL_SUBSTEPS);
                    e
                })
                .collect();
            for link in &LINKS {
                let (parent, child) = (parts[link.parent], parts[link.child]);
                let (kind, rotation) = match link.kind {
                    LinkKind::Ball {
                        up: upward,
                        cone,
                        twist,
                    } => (
                        JointKind::Spherical(
                            SphericalJoint::ball()
                                .with_cone(cone)
                                .with_twist(-twist, twist),
                        ),
                        if upward { up } else { down },
                    ),
                    LinkKind::Hinge { lower, upper } => (
                        JointKind::Revolute(RevoluteJoint::hinge().with_limits(lower, upper)),
                        across,
                    ),
                };
                let at = |e: Entity| *self.phys.transform(e).expect("placed");
                self.phys
                    .add_joint(Joint::at(
                        parent,
                        &at(parent),
                        child,
                        &at(child),
                        Transform::new(origin + link.at, rotation),
                        kind,
                    ))
                    .expect("a valid ragdoll joint");
            }
            self.ragdolls.push(parts);
        }
    }

    /// Stands every ragdoll back up on the landing and pushes it off.
    fn push_ragdolls(&mut self) {
        for (r, parts) in (0u32..).zip(&self.ragdolls) {
            let origin = Self::ragdoll_origin(r);
            for (&e, part) in parts.iter().zip(&PARTS) {
                self.phys
                    .set_transform(e, Transform::from_position(origin + part.at));
                if let Some(body) = self.phys.body_mut(e) {
                    // The top of it pushed harder than the feet, so it pitches
                    // forward down the stairs.
                    body.velocity = DVec3::X * RAGDOLL_PUSH * (part.at.y / 1.8);
                    body.angular_velocity = DVec3::ZERO;
                }
            }
        }
        self.ragdoll_drops += 1;
    }

    /// Draws the cradle's first ball back and stills the rest.
    fn draw_back(&mut self) {
        let pitch = 2.0 * CRADLE_RADIUS + CRADLE_GAP;
        let cos = 1.0 - CRADLE_LIFT / CRADLE_DROP;
        let sin = (1.0 - cos * cos).sqrt();
        for (k, &ball) in (0u32..).zip(&self.balls) {
            let rest = BRIDGE_AT
                + CRADLE_AT
                + DVec3::new(pitch * f64::from(k), CRADLE_TOP - CRADLE_DROP, 0.0);
            let at = if k == 0 {
                rest + DVec3::new(-CRADLE_DROP * sin, CRADLE_DROP * (1.0 - cos), 0.0)
            } else {
                rest
            };
            self.phys.set_transform(ball, Transform::from_position(at));
            if let Some(body) = self.phys.body_mut(ball) {
                body.velocity = DVec3::ZERO;
                body.angular_velocity = DVec3::ZERO;
            }
        }
        self.cradle_in = 0.0;
        self.cradle_out = 0.0;
        self.cradle_swings += 1;
    }

    /// Lays the bridge again, whole, and clears its crates.
    fn rebuild(&mut self) {
        while let Some((e, _)) = self.crates.pop_front() {
            self.phys.remove_entity(e);
        }
        for (&e, home) in self.planks.iter().zip(&self.plank_homes) {
            self.phys.set_transform(e, *home);
            if let Some(body) = self.phys.body_mut(e) {
                body.velocity = DVec3::ZERO;
                body.angular_velocity = DVec3::ZERO;
            }
        }
        for (joint, id) in &mut self.hinges {
            if id.is_none() {
                *id = Some(self.phys.add_joint(*joint).expect("a valid hinge"));
            }
        }
        self.builds += 1;
    }

    /// Lets a box of half-extent `half` and `mass` go `height` above plank
    /// `over`, square on to it, the oldest crate going if there are too many.
    fn drop_crate(&mut self, over: usize, height: f64, half: f64, mass: f64) {
        if self.crates.len() >= MAX_CRATES
            && let Some((oldest, _)) = self.crates.pop_front()
        {
            self.phys.remove_entity(oldest);
        }
        let index = FIRST_CRATE + u32::try_from(self.dropped % CRATE_IDS).expect("under a million");
        self.dropped += 1;
        let e = entity(index);
        // Square on to the plank it lands on, `height` above it.
        let plank = *self.phys.transform(self.planks[over]).expect("a plank");
        let up = plank.rotation * DVec3::Y;
        let at = Transform::new(
            plank.position + up * (PLANK_HALF.y + half + height),
            plank.rotation,
        );
        place_body(
            &mut self.phys,
            e,
            at,
            &box_collider(DVec3::splat(half)),
            mass,
            CRATE_SURFACE,
        );
        self.crates.push_back((e, half));
    }

    fn position(&self, e: Entity) -> DVec3 {
        self.phys.transform(e).expect("a bridge body").position
    }

    /// Every counter, at this instant.
    #[must_use]
    pub fn reading(&self) -> BridgeReading {
        let middle = self.planks[self.planks.len() / 2];
        BridgeReading {
            cradle_in: self.cradle_in,
            cradle_out: self.cradle_out,
            cradle_swings: self.cradle_swings,
            sag: BRIDGE_TOP + BRIDGE_AT.y - self.position(middle).y,
            chain_sag: self.chain_sag,
            crates: self.crates.len(),
            hinges: self.hinges.iter().filter(|(_, id)| id.is_some()).count(),
            builds: self.builds,
            broken: self.broken,
            ragdoll_drops: self.ragdoll_drops,
            contacts: self.tally,
        }
    }

    /// The system, for a test to read joints off.
    #[cfg(test)]
    pub(crate) const fn physics(&self) -> &PhysicsSystem {
        &self.phys
    }
}

impl Room for Bridge {
    fn step(&mut self, dt: f64, clock: Option<&mut dyn FnMut() -> f64>) {
        match clock {
            Some(clock) => self.phys.step_timed(dt, clock),
            None => self.phys.step(dt),
        }
        self.tick += 1;
        self.tally.add(&self.phys.contact_counters());

        for joint_break in self.phys.broken_joints() {
            if let Some(slot) = self
                .hinges
                .iter_mut()
                .find(|(_, id)| *id == Some(joint_break.joint))
            {
                slot.1 = None;
            }
            self.broken += 1;
        }

        let first = self.phys.body(self.balls[0]).expect("a ball").velocity.x;
        let last = self
            .phys
            .body(self.balls[self.balls.len() - 1])
            .expect("a ball")
            .velocity
            .x;
        self.cradle_in = self.cradle_in.max(first);
        self.cradle_out = self.cradle_out.max(last);

        let cycle = self.tick % BRIDGE_EVERY_TICKS;
        if cycle == 0 {
            self.rebuild();
        } else if cycle == ANVIL_AT_TICK {
            self.drop_crate(PLANKS as usize / 2, ANVIL_DROP, ANVIL_HALF, ANVIL_MASS);
        } else if cycle.is_multiple_of(CRATE_EVERY_TICKS) && cycle < ANVIL_AT_TICK {
            self.drop_crate(CRATE_PLANK, CRATE_DROP, CRATE_HALF, CRATE_MASS);
        }
        if self.tick.is_multiple_of(CRADLE_EVERY_TICKS) {
            self.draw_back();
        }
        if self.tick.is_multiple_of(RAGDOLL_EVERY_TICKS) {
            self.push_ragdolls();
        }
    }

    fn hash(&self, hasher: &mut dyn std::hash::Hasher) {
        self.phys.hash_state(hasher);
    }

    fn bodies(&self, out: &mut Vec<Shape>) {
        // The rods are drawn as thin pills from anchor to ball, keyed apart
        // from the bodies by their top bit.
        const ROD_KEY: u64 = 1 << 63;
        for (k, &(hook, ball)) in (0u64..).zip(&self.rods) {
            out.push(Shape::Capsule {
                key: ROD_KEY | k,
                a: self.position(hook),
                b: self.position(ball),
                radius: 0.006,
                tint: Tint::Peg,
            });
        }
        for &ball in &self.balls {
            out.push(Shape::Sphere {
                key: ball.to_bits(),
                centre: self.position(ball),
                radius: CRADLE_RADIUS,
                tint: Tint::Ball,
            });
        }
        let mut boxed = |e: Entity, half: DVec3, tint: Tint| {
            if let Some(t) = self.phys.transform(e) {
                out.push(Shape::Box {
                    key: e.to_bits(),
                    centre: t.position,
                    rotation: t.rotation,
                    half,
                    tint,
                });
            }
        };
        for &plank in &self.planks {
            boxed(plank, PLANK_HALF, Tint::Board);
        }
        for &(e, half) in &self.crates {
            boxed(e, DVec3::splat(half), Tint::Box);
        }
        for parts in &self.ragdolls {
            for (&e, part) in parts.iter().zip(&PARTS) {
                let Some(t) = self.phys.transform(e) else {
                    continue;
                };
                out.push(match part.half_height {
                    Some(half_height) => {
                        let axis = t.rotation * DVec3::new(0.0, half_height, 0.0);
                        Shape::Capsule {
                            key: e.to_bits(),
                            a: t.position - axis,
                            b: t.position + axis,
                            radius: part.radius,
                            tint: Tint::Pill,
                        }
                    }
                    None => Shape::Sphere {
                        key: e.to_bits(),
                        centre: t.position,
                        radius: part.radius,
                        tint: Tint::Handle,
                    },
                });
            }
        }
    }

    fn fixtures(&self, out: &mut Vec<Shape>) {
        // The cradle's bar.
        let pitch = 2.0 * CRADLE_RADIUS + CRADLE_GAP;
        let length = pitch * f64::from(CRADLE_BALLS - 1) + 0.4;
        out.push(Shape::Box {
            key: 0,
            centre: BRIDGE_AT
                + CRADLE_AT
                + DVec3::new(
                    0.5 * pitch * f64::from(CRADLE_BALLS - 1),
                    CRADLE_TOP + 0.03,
                    0.0,
                ),
            rotation: DQuat::IDENTITY,
            half: DVec3::new(0.5 * length, 0.03, CRADLE_SPREAD + 0.05),
            tint: Tint::Peg,
        });
        for (k, side) in (1u64..).zip([-1.0, 1.0]) {
            out.push(Shape::Box {
                key: k,
                centre: BRIDGE_AT
                    + DVec3::new(
                        side * (0.5 * SPAN + POST_GAP + POST_HALF.x),
                        POST_HALF.y,
                        0.0,
                    ),
                rotation: DQuat::IDENTITY,
                half: POST_HALF,
                tint: Tint::Board,
            });
        }
        // The stairs, drawn as a block under the landing and under each
        // tread above the floor.
        let top = f64::from(STEPS) * RISE;
        let blocks = std::iter::once((LANDING, 0.0, top)).chain((0..STEPS).map(|i| {
            let x = f64::from(i) * TREAD;
            (x, x + TREAD, top - f64::from(i + 1) * RISE)
        }));
        for (k, (x0, x1, height)) in (10u64..).zip(blocks) {
            if height <= 0.0 {
                continue;
            }
            out.push(Shape::Box {
                key: k,
                centre: BRIDGE_AT + STAIRS_AT + DVec3::new(0.5 * (x0 + x1), 0.5 * height, 0.0),
                rotation: DQuat::IDENTITY,
                half: DVec3::new(0.5 * (x1 - x0), 0.5 * height, STAIRS_HALF_WIDTH),
                tint: Tint::Board,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::tests::tick_dt;

    /// The worst drift of any joint whose first body is at or past entity
    /// `from` and before `to`, now.
    fn worst_drift(room: &Bridge, from: u32, to: u32) -> (f64, f64) {
        room.physics()
            .joints()
            .filter(|(_, joint)| (from..to).contains(&joint.body_a.index()))
            .filter_map(|(id, _)| room.physics().joint_drift(id))
            .fold((0.0, 0.0), |(linear, angular), d| {
                (f64::max(linear, d.linear), f64::max(angular, d.angular))
            })
    }

    /// **The cradle passes its momentum and the bridge carries its crates
    /// whole**, up to the anvil: the last ball leaves with all but what the
    /// first arrived with, no hinge breaks, and the bridge's hinges hold to
    /// millimetres with crates sliding across it.
    ///
    /// Measured on 2026-09-23 over the 719 ticks before the anvil: 1.981
    /// kg·m/s in, 1.973 out; the bridge's hinges drifted 3.8 mm at worst, and
    /// nothing sank more than 9.8 mm into anything.
    #[test]
    fn the_cradle_passes_its_momentum_and_the_bridge_carries_its_crates() {
        let mut room = Bridge::new();
        let mut worst: f64 = 0.0;
        for _ in 1..ANVIL_AT_TICK {
            room.step(tick_dt(), None);
            worst = worst.max(worst_drift(&room, FIRST_PLANK - 100, FIRST_RAGDOLL).0);
        }
        let r = room.reading();
        assert!(r.cradle_out > 0.99 * r.cradle_in, "{r:?}");
        assert!(
            r.cradle_in > 0.99 * (2.0 * GRAVITY * CRADLE_LIFT).sqrt(),
            "{r:?}"
        );
        assert_eq!((r.broken, r.hinges), (0, PLANKS as usize + 1), "{r:?}");
        assert_eq!(r.crates, 4, "{r:?}");
        assert!(worst < 1.0e-2, "a hinge drifted {worst} m");
        assert!(r.contacts.peak_penetration < 0.02, "{r:?}");
    }

    /// **The anvil snaps the weak link, and only it, and the bridge is built
    /// again**: one joint broken and reported, twenty-one hinges left, and at
    /// the next cycle all twenty-two again.
    ///
    /// Measured on 2026-09-23: the weak link broke at tick 758, when the anvil
    /// landed on the bridge, carrying 15 kN.
    #[test]
    fn the_anvil_snaps_the_weak_link_and_the_bridge_is_built_again() {
        let mut room = Bridge::new();
        let mut broke_at = None;
        for tick in 1..=BRIDGE_EVERY_TICKS {
            room.step(tick_dt(), None);
            if broke_at.is_none() && !room.physics().broken_joints().is_empty() {
                let breaks = room.physics().broken_joints();
                assert_eq!(breaks.len(), 1, "{breaks:?}");
                assert!(breaks[0].force >= HINGE_BREAKS_AT, "{breaks:?}");
                broke_at = Some(tick);
            }
            if tick == BRIDGE_EVERY_TICKS - 1 {
                let r = room.reading();
                assert_eq!((r.broken, r.hinges), (1, PLANKS as usize), "{r:?}");
            }
        }
        let broke_at = broke_at.expect("the anvil did not break the bridge");
        assert!(
            broke_at > ANVIL_AT_TICK,
            "it broke at {broke_at}, before the anvil"
        );
        let r = room.reading();
        assert_eq!(
            (r.hinges, r.builds, r.crates),
            (PLANKS as usize + 1, 2, 0),
            "{r:?}"
        );
        assert!((r.sag - r.chain_sag).abs() < 1.0e-9, "{r:?}");
    }

    /// **The ragdolls keep their limbs on the way down the stairs**: over a
    /// drop, no joint of theirs drifts past the bound, no limit is passed by
    /// much, and each has tumbled at least two steps down.
    ///
    /// Measured on 2026-09-23 over the first drop: the worst joint drifted
    /// 5.3 mm and 69 mrad — a limit overshot on impact — and the pelvises
    /// came to rest 1.62 and 1.50 m past the first step, on the fourth tread.
    #[test]
    fn the_ragdolls_keep_their_limbs_down_the_stairs() {
        let mut room = Bridge::new();
        let (mut linear, mut angular): (f64, f64) = (0.0, 0.0);
        for _ in 0..RAGDOLL_EVERY_TICKS - 1 {
            room.step(tick_dt(), None);
            let (l, a) = worst_drift(&room, FIRST_RAGDOLL, FIRST_CRATE);
            linear = linear.max(l);
            angular = angular.max(a);
        }
        let pelvises: Vec<f64> = room
            .ragdolls
            .iter()
            .map(|parts| room.position(parts[0]).x - (BRIDGE_AT + STAIRS_AT).x)
            .collect();
        assert!(linear < 1.0e-2, "a ragdoll joint drifted {linear} m");
        assert!(
            angular < 0.15,
            "a ragdoll limit was passed by {angular} rad"
        );
        for x in pelvises {
            assert!(x > 2.0 * TREAD, "a ragdoll stopped at {x} m");
        }
    }
}
