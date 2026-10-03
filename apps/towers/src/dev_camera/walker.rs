//! The walk half of the dev camera: a capsule [`CharacterController`] drives
//! over a physics world of its own, built from the map.
//!
//! # Its own world, never the stage's
//!
//! The stage's [`PhysicsWorld`] is the simulation's: every creep is a sphere
//! in it, every bolt sweeps it, and a resumed stage hashes as the one that
//! saved only because its queries are read in the field's order. A walker
//! that queried it would be no reader at all — every query takes
//! `&mut PhysicsWorld`, because the broadphase is rebuilt lazily on the first
//! query after a change — and on a joiner there is no stage to query. So
//! [`Walker`] builds a second world from the same [`Map`] numbers
//! ([`Map::lane_collider`], [`pad_collider`], [`tower_collider`],
//! [`ground_collider`]) and nothing it does reaches the first.
//!
//! # What is solid to it
//!
//! * **The ground, the lane and the pads**, so the lane is a kerb a walker
//!   steps up onto and a pad another.
//! * **Every tower standing on the field**, at its tier's size, synced from
//!   what the frame draws — [`Walker::sync_towers`] — so a tower built while
//!   walking stands in the way from the next tick, solo or joined.
//! * **A wall round the field's edge** that nothing draws, so a walker cannot
//!   walk off the slab and fall for ever. The field has no rim of its own;
//!   this one is the walker's alone.
//! * **The exit volume, as the trigger it is.** It is in this world flagged as
//!   in the stage's, so the controller's sweeps pass through it exactly as a
//!   bolt does.
//!
//! **The creeps are not here at all.** They move every tick and the controller
//! reads no collider's velocity — a creep walking into a walker would resolve
//! as a penetration pushed out on the next move rather than as a push — and a
//! dev camera standing in the lane is for watching them go by.

use crcbl::math::DVec3;
use crcbl::phys::{
    BoxCollider, CharacterConfig, CharacterController, ColliderId, MoveOutcome, PhysicsWorld,
    SlideContact,
};

use crate::map::{
    HALF_DEPTH, HALF_WIDTH, MAX_PLOTS, Map, SLAB_THICKNESS, ground_collider, pad_collider,
    tower_collider,
};
use crate::tower::Tier;

/// How fast the walker walks, in metres a second: a brisk walk, the speed
/// `apps/puppet` walks its lane at.
pub const WALK_SPEED: f64 = 3.2;

/// Gravity, in metres per second squared, integrated into a fall speed as
/// `apps/puppet`'s is.
pub const GRAVITY: f64 = -9.81;

/// How far above the walker's **feet** the eye sits, in metres — where a head
/// is on [`CharacterConfig::default`]'s capsule, as `apps/breach` puts it.
pub const EYE_HEIGHT: f64 = 1.65;

/// How tall the wall round the field's edge stands, in metres: well over the
/// capsule's height, so it is a wall and never a step.
pub const EDGE_HEIGHT: f64 = 3.0;

/// How thick that wall is, in metres, standing outside the slab's footprint.
const EDGE_THICKNESS: f64 = 1.0;

/// How far inside the field's edge a walker dropped from the fly camera lands
/// at the nearest, in metres — clear of the edge wall by more than its radius.
pub const SPAWN_INSET: f64 = 1.0;

/// What a collider in the walker's world is, for the debug panel's contact
/// rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Ground,
    Lane,
    Pad,
    Exit,
    Edge,
    /// The tower on this plot.
    Tower(usize),
}

impl Part {
    /// How the debug panel names it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ground => "ground",
            Self::Lane => "lane",
            Self::Pad => "pad",
            Self::Exit => "exit",
            Self::Edge => "edge",
            Self::Tower(_) => "tower",
        }
    }
}

/// A capsule walking the field — see the module docs.
#[derive(Debug)]
pub struct Walker {
    world: PhysicsWorld,
    /// Every collider [`Walker::new`] put in the world, and what it is.
    field: Vec<(ColliderId, Part)>,
    /// Where each plot's tower stands, in the map's plot order.
    plots: Vec<DVec3>,
    /// The collider standing on each plot, and the tier it was built for, or
    /// `None` for an empty plot.
    towers: [Option<(ColliderId, Tier)>; MAX_PLOTS],
    controller: CharacterController,
    /// Metres a second, negative down — see [`GRAVITY`].
    fall_speed: f64,
    /// What the last move's sweeps met, in order.
    contacts: Vec<SlideContact>,
    /// What the last move did.
    last: MoveOutcome,
}

impl Walker {
    /// A walker over `map`'s field, standing on the ground at the path's start
    /// until it is dropped somewhere else.
    #[must_use]
    pub fn new(map: &Map) -> Self {
        let mut world = PhysicsWorld::new();
        let mut field = vec![(world.add_box(ground_collider()), Part::Ground)];
        for leg in 0..map.path().legs() {
            field.push((world.add_box(map.lane_collider(leg)), Part::Lane));
        }
        let plots: Vec<DVec3> = map.plots().iter().map(crate::scene::Plot::at).collect();
        for &feet in &plots {
            field.push((world.add_box(pad_collider(feet)), Part::Pad));
        }
        for edge in edge_colliders() {
            field.push((world.add_box(edge), Part::Edge));
        }
        let exit = world.add_box(map.exit_collider());
        world.set_trigger(exit, true);
        field.push((exit, Part::Exit));

        let config = CharacterConfig::default();
        let start = map.path().waypoints()[0];
        let mut walker = Self {
            world,
            field,
            plots,
            towers: [None; MAX_PLOTS],
            controller: CharacterController::new(config, start),
            fall_speed: 0.0,
            contacts: Vec::new(),
            last: MoveOutcome::default(),
        };
        walker.place(start + DVec3::Y * standing_centre(&config));
        walker
    }

    /// Puts the capsule's centre at `centre`, falling from rest and standing on
    /// nothing until its next move finds the ground.
    pub fn place(&mut self, centre: DVec3) {
        self.controller.set_position(centre);
        self.fall_speed = 0.0;
        self.contacts.clear();
        self.last = MoveOutcome::default();
    }

    /// Drops the walker from under an eye at `eye` — where the fly camera is
    /// when walking is switched on — kept [`SPAWN_INSET`] inside the field's
    /// edge, and never lower than standing on the ground, since the fly camera
    /// passes through the slab.
    pub fn drop_from(&mut self, eye: DVec3) {
        let config = *self.controller.config();
        let reach_x = HALF_WIDTH - SPAWN_INSET;
        let reach_z = HALF_DEPTH - SPAWN_INSET;
        let standing = standing_centre(&config);
        self.place(DVec3::new(
            eye.x.clamp(-reach_x, reach_x),
            (eye.y - EYE_HEIGHT + standing).max(standing),
            eye.z.clamp(-reach_z, reach_z),
        ));
    }

    /// Makes the towers in this world the ones `towers` — a frame's
    /// [`crate::game::RenderState::towers`] — says stand on the field: one on
    /// each plot it names, at the tier it names, and none on the
    /// others. A plot whose tower is unchanged keeps its collider.
    pub fn sync_towers(&mut self, towers: &[Option<crate::tower::TowerView>; MAX_PLOTS]) {
        for (plot, &feet) in self.plots.iter().enumerate() {
            let want = towers[plot].map(|view| view.tier);
            if self.towers[plot].map(|(_, tier)| tier) == want {
                continue;
            }
            if let Some((id, _)) = self.towers[plot].take() {
                self.world.remove(id);
            }
            if let Some(tier) = want {
                let id = self.world.add_capsule(tower_collider(feet, tier));
                self.towers[plot] = Some((id, tier));
            }
        }
    }

    /// Walks `direction` — a unit vector on the ground plane, or zero to stand
    /// — for `dt` seconds at [`WALK_SPEED`], falling under [`GRAVITY`], and
    /// says what the move did. The sweeps it made are
    /// [`Walker::contacts`] until the next.
    pub fn step(&mut self, direction: DVec3, dt: f64) -> MoveOutcome {
        let horizontal = direction * WALK_SPEED * dt;
        // Integrated while off the ground and reset on it, as `apps/puppet`
        // does: a grounded move takes its rise from the ground and discards
        // the vertical it is asked for, so this is the next tick's fall.
        self.fall_speed += GRAVITY * dt;
        let motion = horizontal + DVec3::Y * self.fall_speed * dt;
        let outcome =
            self.controller
                .move_and_slide_into(&mut self.world, motion, &mut self.contacts);
        if outcome.grounded {
            self.fall_speed = 0.0;
        } else if outcome.hit_ceiling {
            self.fall_speed = self.fall_speed.min(0.0);
        }
        self.last = outcome;
        outcome
    }

    /// Where the capsule's feet are.
    #[must_use]
    pub fn feet(&self) -> DVec3 {
        let config = self.controller.config();
        self.controller.position() - DVec3::Y * (config.radius + config.half_height)
    }

    /// Where the walker's eye is: [`EYE_HEIGHT`] over its feet.
    #[must_use]
    pub fn eye(&self) -> DVec3 {
        self.feet() + DVec3::Y * EYE_HEIGHT
    }

    /// Whether the walker stands on walkable ground.
    #[must_use]
    pub fn is_grounded(&self) -> bool {
        self.controller.is_grounded()
    }

    /// What the walker stands on, if anything.
    #[must_use]
    pub fn ground(&self) -> Option<Part> {
        self.controller
            .ground()
            .and_then(|ground| self.part_of(ground.collider))
    }

    /// What the last move's sweeps met, in the order the slide met them —
    /// [`CharacterController::move_and_slide_into`]'s record.
    #[must_use]
    pub fn contacts(&self) -> &[SlideContact] {
        &self.contacts
    }

    /// What the last move did.
    #[must_use]
    pub const fn last(&self) -> &MoveOutcome {
        &self.last
    }

    /// The limits the capsule moves under.
    #[must_use]
    pub fn config(&self) -> &CharacterConfig {
        self.controller.config()
    }

    /// What `id` is in this world, or `None` for an id it does not hold.
    #[must_use]
    pub fn part_of(&self, id: ColliderId) -> Option<Part> {
        self.field
            .iter()
            .find(|(field, _)| *field == id)
            .map(|(_, part)| *part)
            .or_else(|| {
                self.towers
                    .iter()
                    .position(|tower| tower.is_some_and(|(tower, _)| tower == id))
                    .map(Part::Tower)
            })
    }
}

/// Where a capsule standing on `y = 0` has its centre: its radius and half
/// its cylinder up, and the skin a settled move keeps under it.
fn standing_centre(config: &CharacterConfig) -> f64 {
    config.radius + config.half_height + config.skin_width
}

/// The four walls round the field's edge, standing outside the slab from its
/// underside to [`EDGE_HEIGHT`]: their inner faces are the slab's edges, so a
/// walker is stopped where the ground ends.
fn edge_colliders() -> [BoxCollider; 4] {
    let half_y = 0.5 * (EDGE_HEIGHT + SLAB_THICKNESS);
    let centre_y = 0.5 * (EDGE_HEIGHT - SLAB_THICKNESS);
    let half_t = 0.5 * EDGE_THICKNESS;
    let along_z = DVec3::new(half_t, half_y, HALF_DEPTH + EDGE_THICKNESS);
    let along_x = DVec3::new(HALF_WIDTH + EDGE_THICKNESS, half_y, half_t);
    [
        BoxCollider::new(DVec3::new(HALF_WIDTH + half_t, centre_y, 0.0), along_z),
        BoxCollider::new(DVec3::new(-HALF_WIDTH - half_t, centre_y, 0.0), along_z),
        BoxCollider::new(DVec3::new(0.0, centre_y, HALF_DEPTH + half_t), along_x),
        BoxCollider::new(DVec3::new(0.0, centre_y, -HALF_DEPTH - half_t), along_x),
    ]
}

#[cfg(test)]
mod tests;
