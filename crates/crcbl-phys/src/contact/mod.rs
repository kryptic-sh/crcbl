//! Contacts: rungs 1 to 3 of `docs/plan/36-contact-solver.md`.
//!
//! ```text
//!   PhysicsSystem::step(dt), in a system built with contacts
//!
//!   broadphase ─▶ narrow phase ─▶ islands ─▶ forces ─▶ solver ×substeps ─▶ sleep
//!   split trees   one manifold    wake what            integrate velocities  timers,
//!   move buffer   per pair,       was touched,         warm start            split one
//!   pair set      feature ids     merge what           solve (soft, biased)  island,
//!                 matched to      began touching       integrate positions   sleep the
//!                 last tick's                          relax (rigid)         still ones
//!                 impulses                             then restitution
//! ```
//!
//! Collision runs **once a tick** and the solver runs
//! [`ContactSettings::substeps`] substeps, each with one biased and one relaxed
//! pass, warm-started from the tick before: the Soft Step of decision 1.
//! Restitution is its own pass after the substeps. See `broadphase.rs` in this
//! directory for the pairs, [`manifold`] for the shapes, `solver.rs` for the
//! arithmetic and `island.rs` for islands and sleep.
//!
//! # Opt in, per system
//!
//! A [`crate::PhysicsSystem`] made with [`crate::PhysicsSystem::new`] has no
//! contacts and steps exactly as it did before rung 1: one integration of `dt`
//! a call. Contacts are
//! [`crate::PhysicsSystem::with_contacts`]'s, because every sample built before
//! them — horde's ten thousand kinematic sprites, breakout's paddle — has
//! colliders that were never meant to push each other, and paying a
//! broadphase for them would be a cost those samples never asked for.
//!
//! # What rung 2 added
//!
//! Box against box, through a separating axis test whose last axis each
//! contact caches, with clipping, four-point reduction and flip-invariant
//! feature ids — see [`manifold`]. And friction at each manifold's centroid
//! with a twist term about its normal, in place of friction at every point —
//! see `solver.rs`.
//!
//! # What rung 3 added
//!
//! Persistent islands of dynamic bodies joined by touching contacts, merged as
//! contacts begin and split lazily, and **sleep**: an island all of whose
//! bodies have moved slower than [`ContactSettings::sleep_speed`] and turned
//! slower than [`ContactSettings::sleep_angular_speed`] for
//! [`ContactSettings::time_to_sleep`] leaves the awake set, and costs the
//! step, the broadphase update and the solver nothing until it wakes. A
//! sleeping island wakes when
//!
//! - a contact with one of its bodies begins touching a body that moves: an
//!   awake dynamic body, or a kinematic one with a velocity;
//! - one of its bodies is given a force, a torque, a velocity or a whole new
//!   body — [`crate::PhysicsSystem::apply_force`],
//!   [`crate::PhysicsSystem::apply_torque`],
//!   [`crate::PhysicsSystem::body_mut`], [`crate::PhysicsSystem::set_body`];
//! - one of its bodies, or a body touching one, is teleported with
//!   [`crate::PhysicsSystem::set_transform`], given a new collider or
//!   material, or loses its collider or its entity — a support taken away;
//! - a body that does not move is placed or created overlapping one of its
//!   bodies, which a teleported or new static collider is.
//!
//! A query wakes nothing, and neither does reading a body or a transform. See
//! `island.rs`.
//!
//! # Compounds
//!
//! A [`crate::ColliderComponent::Compound`] body is several boxes. **Each part
//! is a broadphase proxy of its own**, as each shape of a Box2D v3 body is:
//! the owner of a proxy is a body and a part, two parts of one body never
//! pair, and every contact is between two parts — so a compound resting on
//! the floor on two parts has two contacts with it, each a box manifold of up
//! to four points. Everything downstream is unchanged by that: a contact's
//! cached separating axis, its feature ids and its warm-start impulses are
//! the part pair's, and persist exactly as a lone box's do, because the pair
//! is the contact; islands, sleep and the wake rules work on bodies, which
//! own every part's contacts. The alternative — one proxy per body and a
//! narrow phase that walks part pairs, with feature ids widened by the part
//! indices — would put a part-against-part cull in every tick that the
//! broadphase's fat bounds and pair set already do once, and make a contact
//! hold several manifolds with different normals. The cost of the choice is
//! a proxy per part, which [`crate::CompoundShape::MAX_PARTS`] bounds, and
//! with it the points a pair of bodies can put in the solver: four per
//! touching pair of parts. One [`KineticContact`] is raised per contact, so a
//! compound landing flat on two parts raises two.
//!
//! # What is not done yet
//!
//! General convex hulls, and GJK for spheres and capsules against them: the
//! collider set has no hull, and against a box the analytic pairs are exact.
//! Nothing sweeps (rung 4): speculative contacts are
//! what stop a fast body, and the speculative distance grows with the pair's
//! speed so they can. The solver is scalar `f64` (rung 6 makes it wide). A
//! tall stack needs [`ContactSettings::TALL_STACK`] for its whole system,
//! since substeps are not yet per group.

pub(crate) mod broadphase;
pub(crate) mod island;
pub mod manifold;
pub mod shape;
pub(crate) mod solver;

use std::hash::Hasher;

use crcbl_core::Pool;
use crcbl_ecs::Entity;
use glam::DVec3;

use self::broadphase::{Broadphase, PlaneBounds, ProxyId, ProxyKind};
use self::island::{IslandId, Islands};
use self::manifold::{MAX_POINTS, Manifold, SatCache};
use self::shape::ContactShape;
use crate::collider::Aabb;
use crate::material::SurfaceMaterial;
use crate::system::{AwakeSet, BodyId, BodyRecord, BodySet, StaticSet, canonical_bits};

/// The contact pipeline's knobs, with the values
/// `docs/plan/36-contact-solver.md` decision 1 and Box2D v3 settled on as
/// defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactSettings {
    /// Solver substeps a tick. Collision runs once whatever this is.
    pub substeps: u32,
    /// How stiff a contact is, as the frequency of the spring it behaves as,
    /// in hertz. Capped at a quarter of the substep rate, so a spring is never
    /// stepped too coarsely to be stable. A contact with a static or kinematic
    /// body is twice as stiff.
    pub contact_hertz: f64,
    /// The contact spring's damping ratio: 1 is critical, and more is the
    /// overdamped push that stops a sunk body without bouncing it back out.
    pub damping_ratio: f64,
    /// The fastest a contact pushes two overlapping bodies apart, in m/s.
    pub push_out_speed: f64,
    /// How far apart two shapes may be and still have a contact, in metres,
    /// before speed is added: four times Box2D's 5 mm linear slop.
    /// The pipeline adds how far the pair can close in one tick, so a body
    /// that would cross a thin wall between two ticks meets a contact first.
    pub speculative_distance: f64,
    /// The approach speed below which a contact does not bounce, in m/s, and
    /// below which it raises no [`KineticContact`].
    pub restitution_threshold: f64,
    /// The fastest a body may move, in m/s.
    pub max_linear_speed: f64,
    /// The most a body may turn in one substep, in radians.
    pub max_rotation: f64,
    /// Whether last tick's impulses seed this tick's.
    pub warm_starting: bool,
    /// The smallest normal impulse over a tick, in N·s, that raises a
    /// [`KineticContact`].
    pub kinetic_impulse: f64,
    /// Whether islands sleep. Turned off, every sleeping island wakes on the
    /// next step.
    pub sleep: bool,
    /// How slow a dynamic body must move to count as still, in m/s.
    pub sleep_speed: f64,
    /// How slow a dynamic body must turn to count as still, in rad/s.
    pub sleep_angular_speed: f64,
    /// How long every body of an island must stay still before the island
    /// sleeps, in seconds.
    pub time_to_sleep: f64,
}

impl ContactSettings {
    /// Four substeps, 30 Hz contacts at damping ratio 10 pushing out at up to
    /// 3 m/s, a 2 cm speculative distance, bounces above 1 m/s,
    /// 400 m/s and a quarter turn a substep, warm starting on, any impulse
    /// of a newton-second raising an event, and an island sleeping once its
    /// bodies have stayed under 5 cm/s and 0.1 rad/s for half a second.
    ///
    /// **The sleep thresholds.** Half a second under 5 cm/s is decision 4's,
    /// and Box2D v3's `B2_TIME_TO_SLEEP` and default sleep threshold. Box2D
    /// judges turning by the speed it gives the body's farthest point; this
    /// judges it by angular speed, at 0.1 rad/s — the turn that moves a point
    /// half a metre out at 5 cm/s, so for the half-metre props it is tuned for
    /// the two agree. Angular speed has the merit that a body with no
    /// collider, which has no farthest point, cannot sleep while it spins.
    pub const DEFAULT: Self = Self {
        substeps: 4,
        contact_hertz: 30.0,
        damping_ratio: 10.0,
        push_out_speed: 3.0,
        speculative_distance: 4.0 * 0.005,
        restitution_threshold: 1.0,
        max_linear_speed: 400.0,
        max_rotation: 0.25 * core::f64::consts::PI,
        warm_starting: true,
        kinetic_impulse: 1.0,
        sleep: true,
        sleep_speed: 0.05,
        sleep_angular_speed: 0.1,
        time_to_sleep: 0.5,
    };

    /// [`DEFAULT`](Self::DEFAULT) with twice the substeps and three times the
    /// stiffness — 90 Hz, under the 120 Hz cap eight substeps allow — for a
    /// tall stack, which is decision 1's "more substeps for its group".
    ///
    /// **Why a column needs it.** A soft contact is a spring of stiffness
    /// `m ω²` on the pair's effective mass, whatever it carries, so a column's
    /// joints resist rocking with a stiffness that does not grow with the
    /// weight above them. That is a heavy column on elastic joints, which
    /// buckles under its own weight past Greenhill's height `L³ = 7.837 EI / q`.
    /// For cubes of half-extent `w` — each corner's effective mass `m / 8`,
    /// four corners, `EI = 4 (m/8) ω² w² · 2w` and `q = m g / 2w` — that is
    /// `N = (1.96 ω² w / g)^⅓` cubes, whatever their mass: at the default
    /// 30 Hz, 15 one-metre cubes, and at 90 Hz, 32. Measured on 2026-09-23: at
    /// the defaults 14 one-metre cubes stood and 17 fell; with these settings
    /// 20 stood. Box2D's contacts default to the same 30 Hz, so the same
    /// arithmetic applies to it.
    pub const TALL_STACK: Self = Self {
        substeps: 8,
        contact_hertz: 90.0,
        ..Self::DEFAULT
    };
}

impl Default for ContactSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How long each stage of one [`crate::PhysicsSystem::step_timed`] took, in
/// seconds, by the clock the caller passed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StageTimes {
    /// Updating the proxies and finding new pairs.
    pub broadphase: f64,
    /// Every pair's manifold.
    pub narrow_phase: f64,
    /// The forces, every substep, the restitution pass and storing the
    /// impulses.
    pub solver: f64,
    /// Keeping the islands: giving new bodies theirs, waking and merging
    /// them, the sleep timers, a split and putting still islands to sleep.
    pub islands: f64,
}

/// What the last step of a system with contacts did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactCounters {
    /// Bodies that step: the awake ones, dynamic and kinematic.
    pub bodies: usize,
    /// Bodies whose island sleeps: nothing steps them or solves them.
    pub sleeping: usize,
    /// Islands awake.
    pub islands: usize,
    /// Islands asleep.
    pub sleeping_islands: usize,
    /// Pairs in the pair set: contacts that exist, touching or not, asleep
    /// or not.
    pub pairs: usize,
    /// Contacts the step collided whose manifold has a point, speculative
    /// ones included. A sleeping island's contacts are not collided, so they
    /// are not counted here, nor their points below.
    pub touching: usize,
    /// Manifold points over every contact.
    pub points: usize,
    /// Points whose impulse was carried over from the tick before.
    pub persisted: usize,
    /// Contacts that gained their first point this step.
    pub begun: u64,
    /// Contacts that lost their last point, or ended, this step.
    pub ended: u64,
    /// The deepest overlap any point had when the step's manifolds were built,
    /// in metres.
    pub worst_penetration: f64,
    /// Points the restitution pass bounced.
    pub bounces: u64,
    /// Over those, the sum of separating speed after over approach speed
    /// before.
    pub bounce_ratio_sum: f64,
    /// Over those, the sum of the restitution each was asked for.
    pub restitution_sum: f64,
    /// Each stage's time, if the step was timed.
    pub stages: Option<StageTimes>,
}

impl ContactCounters {
    /// The mean bounce the restitution pass produced, as separating speed over
    /// approach speed, or `None` if nothing bounced.
    #[must_use]
    pub fn bounce_ratio(&self) -> Option<f64> {
        (self.bounces > 0).then(|| self.bounce_ratio_sum / self.bounces as f64)
    }

    /// Manifold points over touching contacts — rung 2's "points per
    /// manifold" — or `None` if nothing touched.
    #[must_use]
    pub fn points_per_manifold(&self) -> Option<f64> {
        (self.touching > 0).then(|| self.points as f64 / self.touching as f64)
    }

    /// The share of this step's points whose feature id matched one of the
    /// step before's, and so were warm-started — rung 2's "persisted-id
    /// ratio" — or `None` if there were no points. A stack at rest reads one;
    /// flickering ids read less.
    #[must_use]
    pub fn persisted_ratio(&self) -> Option<f64> {
        (self.points > 0).then(|| self.persisted as f64 / self.points as f64)
    }
}

/// Where a [`KineticContact`] came from.
///
/// `docs/plan/28-ballistics.md` names two sources, a ballistic sweep's chain
/// hit and a solver contact; the ballistic sweep chain is not built, so only
/// the solver's exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum KineticSource {
    /// An impulse the contact solver applied.
    Contact,
}

/// Mass in motion meeting a body hard enough to matter —
/// `docs/plan/28-ballistics.md`'s event, raised by the contact solver.
///
/// One is raised per contact per step whose approach speed reached
/// [`ContactSettings::restitution_threshold`] and whose normal impulse over the
/// tick reached [`ContactSettings::kinetic_impulse`], so a crate resting on the
/// floor raises none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KineticContact {
    /// What produced it.
    pub source: KineticSource,
    /// The body that did the hitting: the faster into the contact of two
    /// dynamic bodies, or the static or kinematic one a dynamic body met.
    /// `None` for a plane, which is world geometry with no entity.
    pub impactor: Option<Entity>,
    /// The dynamic body that was hit.
    pub struck: Entity,
    /// Where, averaged over the contact's points.
    pub point: DVec3,
    /// The unit normal, from the impactor into the struck body.
    pub normal: DVec3,
    /// The impactor's velocity less the struck body's, at the contact, as the
    /// step began.
    pub relative_velocity: DVec3,
    /// The impactor's mass, in kilograms: infinite for a static, kinematic or
    /// plane impactor.
    pub impactor_mass: f64,
    /// The kinetic energy the normal impulse took out of the approach, in
    /// joules, summed over the contact's points with each point's own
    /// effective mass — exact for one point, an estimate for several.
    pub energy_deposited: f64,
    /// The normal impulse over the tick, in N·s.
    pub impulse: f64,
}

/// A plane added to a system with contacts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlaneId(u32);

/// One side of a [`ContactReport`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactBody {
    /// A registered entity.
    Entity(Entity),
    /// A plane.
    Plane(PlaneId),
}

/// A contact as [`crate::PhysicsSystem::contacts`] reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContactReport {
    /// The contact's slot in the pool: the same for as long as the pair lasts.
    pub slot: u32,
    /// Shape `A`, the lower-ranked; the normal points away from it.
    pub a: ContactBody,
    /// Shape `B`.
    pub b: ContactBody,
    /// Which part of `a`'s collider: an index into its
    /// [`crate::CompoundShape::parts`] for a compound, and 0 for anything else.
    pub part_a: usize,
    /// Which part of `b`'s collider, likewise.
    pub part_b: usize,
    /// The manifold the last step built.
    pub manifold: Manifold,
    /// Each point's normal impulse as the last step left it, in manifold order.
    pub normal_impulses: [f64; MAX_POINTS],
}

/// What owns a proxy: a body's collider — which part of it, for a compound,
/// and part 0 of anything else — or a plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Body(BodyId, usize),
    Plane(u32),
}

/// A plane's record.
#[derive(Clone, Copy, Debug)]
struct PlaneRecord {
    bounds: PlaneBounds,
    material: SurfaceMaterial,
}

/// The impulses a contact keeps for warm starting: one normal impulse per
/// point, matched to the next tick's points by feature id, and the friction
/// and twist the whole manifold carries at its centroid.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct WarmImpulses {
    normal: [f64; MAX_POINTS],
    /// The friction impulse as a world vector, so a normal that turns between
    /// ticks still hands its successor the right amount.
    friction: DVec3,
    /// The twist impulse about the normal, in N·m·s.
    twist: f64,
}

/// A pair in the pool.
#[derive(Clone, Copy, Debug)]
struct Contact {
    a: ProxyId,
    b: ProxyId,
    manifold: Manifold,
    impulses: WarmImpulses,
    touching: bool,
    /// A box pair's last separating axis. It only ever saves work, so it is
    /// not part of the hashed state.
    cache: SatCache,
}

/// One side of a contact, resolved against the bodies for one tick.
#[derive(Clone, Copy, Debug)]
struct Side {
    shape: ContactShape,
    /// The body's index in the awake set, if it steps.
    awake: Option<usize>,
    entity: Option<Entity>,
    material: SurfaceMaterial,
    position: DVec3,
    velocity: DVec3,
    angular_velocity: DVec3,
    inverse_mass: f64,
    mass: f64,
}

/// The bodies a pipeline reads, lent by the system for one call.
#[derive(Clone, Copy)]
pub(crate) struct Bodies<'a> {
    pub(crate) records: &'a Pool<BodyRecord>,
    pub(crate) statics: &'a StaticSet,
    pub(crate) awake: &'a AwakeSet,
    pub(crate) islands: &'a Islands,
}

/// How one side of a contact takes part in a tick: what the narrow phase and
/// the solver need to know before they resolve the side in full, and what the
/// islands need to know about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Presence {
    /// An awake dynamic body.
    Dynamic(BodyId),
    /// A kinematic body with a velocity.
    Moving,
    /// A static body, a kinematic one at rest, or a plane.
    Still,
    /// A body whose island sleeps.
    Asleep(BodyId),
}

impl Presence {
    /// Whether it moves this tick, so a contact with it must be collided.
    const fn moves(self) -> bool {
        matches!(self, Self::Dynamic(_) | Self::Moving)
    }

    /// The body, if it belongs to an island.
    const fn island_body(self) -> Option<BodyId> {
        match self {
            Self::Dynamic(id) | Self::Asleep(id) => Some(id),
            Self::Moving | Self::Still => None,
        }
    }
}

/// What a step's broadphase and narrow phase found that the islands must act
/// on, for the system to apply before it solves.
#[derive(Debug, Default)]
pub(crate) struct IslandEvents {
    /// Sleeping bodies to wake: touched by something that moves, or found
    /// overlapping something placed.
    pub(crate) wake: Vec<BodyId>,
    /// Pairs of island bodies whose contact began touching: their islands
    /// merge.
    pub(crate) links: Vec<(BodyId, BodyId)>,
    /// An island body of each contact between two island bodies that stopped
    /// touching: its island may be in pieces.
    pub(crate) unlinks: Vec<BodyId>,
    /// Awake dynamic bodies touching a moving kinematic body: they are not
    /// still, whatever their own speed.
    pub(crate) stirred: Vec<BodyId>,
}

impl IslandEvents {
    fn clear(&mut self) {
        self.wake.clear();
        self.links.clear();
        self.unlinks.clear();
        self.stirred.clear();
    }
}

/// The contact pipeline a system with contacts owns.
#[derive(Debug)]
pub(crate) struct ContactPipeline {
    pub(crate) settings: ContactSettings,
    broadphase: Broadphase,
    /// Indexed by proxy id.
    owners: Vec<Option<Owner>>,
    planes: Vec<PlaneRecord>,
    contacts: Vec<Option<Contact>>,
    free_contacts: Vec<u32>,
    new_pairs: Vec<(ProxyId, ProxyId)>,
    /// Touching contacts ended between steps — by a body or a collider taken
    /// out — for the next step's counters.
    ended_between_steps: u64,
    pub(crate) counters: ContactCounters,
    pub(crate) kinetic: Vec<KineticContact>,
    pub(crate) events: IslandEvents,
    /// The contacts inside an island being split.
    edges: Vec<(BodyId, BodyId)>,
    solver: solver::Scratch,
}

impl ContactPipeline {
    pub(crate) fn new(settings: ContactSettings) -> Self {
        Self {
            settings,
            broadphase: Broadphase::new(),
            owners: Vec::new(),
            planes: Vec::new(),
            contacts: Vec::new(),
            free_contacts: Vec::new(),
            new_pairs: Vec::new(),
            ended_between_steps: 0,
            counters: ContactCounters::default(),
            kinetic: Vec::new(),
            events: IslandEvents::default(),
            edges: Vec::new(),
            solver: solver::Scratch::default(),
        }
    }

    // ── Proxies ──────────────────────────────────────────────────────────

    /// Gives body `id` a proxy for each part of its collider, in part order,
    /// or none if the collider is a trigger.
    pub(crate) fn create_body_proxies(&mut self, id: BodyId, bodies: Bodies<'_>) -> Vec<ProxyId> {
        let Some(record) = bodies.records.get(id) else {
            return Vec::new();
        };
        let Some((_, component)) = record.collider.as_ref() else {
            return Vec::new();
        };
        let transform = transform_of(record, bodies);
        let kind = proxy_kind(record.set);
        let mut proxies = Vec::new();
        for part in 0..component.part_count() {
            let Some(shape) = ContactShape::placed_part(component, part, transform) else {
                continue;
            };
            let proxy = self
                .broadphase
                .create(kind, shape.aabb().expect("a body's shape is bounded"));
            self.set_owner(proxy, Owner::Body(id, part));
            proxies.push(proxy);
        }
        proxies
    }

    /// Adds a plane.
    pub(crate) fn add_plane(
        &mut self,
        normal: DVec3,
        offset: f64,
        material: SurfaceMaterial,
    ) -> PlaneId {
        let index = self.planes.len() as u32;
        let bounds = PlaneBounds { normal, offset };
        self.planes.push(PlaneRecord { bounds, material });
        let proxy = self.broadphase.create_plane(index, bounds);
        self.set_owner(proxy, Owner::Plane(index));
        PlaneId(index)
    }

    /// Removes a proxy and ends every contact it is part of, counting the
    /// touching ones as ended in the next step's counters.
    pub(crate) fn destroy_proxy(&mut self, proxy: ProxyId) {
        for slot in 0..self.contacts.len() {
            if let Some(contact) = self.contacts[slot]
                && (contact.a == proxy || contact.b == proxy)
                && self.destroy_contact(slot)
            {
                self.ended_between_steps += 1;
            }
        }
        self.broadphase.destroy(proxy);
        if let Some(owner) = self.owners.get_mut(proxy as usize) {
            *owner = None;
        }
    }

    /// Moves a proxy to the tree its body's set belongs in.
    pub(crate) fn body_changed_set(&mut self, proxy: ProxyId, set: BodySet) {
        self.broadphase.set_kind(proxy, proxy_kind(set));
    }

    /// Tells the broadphase a body was placed by hand.
    pub(crate) fn body_placed(&mut self, id: BodyId, bodies: Bodies<'_>) {
        let Some(record) = bodies.records.get(id) else {
            return;
        };
        let Some((_, component)) = record.collider.as_ref() else {
            return;
        };
        let transform = transform_of(record, bodies);
        for (part, &proxy) in record.proxies.iter().enumerate() {
            if let Some(bounds) =
                ContactShape::placed_part(component, part, transform).and_then(|shape| shape.aabb())
            {
                self.broadphase.update(proxy, bounds);
            }
        }
    }

    fn set_owner(&mut self, proxy: ProxyId, owner: Owner) {
        let index = proxy as usize;
        if index >= self.owners.len() {
            self.owners.resize(index + 1, None);
        }
        self.owners[index] = Some(owner);
    }

    // ── The step ─────────────────────────────────────────────────────────

    /// Brings every moving proxy's bounds up to date for a tick of `dt` and
    /// turns the pairs that are new into contacts.
    ///
    /// A sleeping body's proxy is not updated — it has not moved — and a new
    /// pair between a sleeping body and one that does not move either is a
    /// static or resting kinematic body placed or created where the sleeper
    /// is, which wakes it: the narrow phase never collides such a pair.
    pub(crate) fn update_pairs(&mut self, bodies: Bodies<'_>, dt: f64) {
        self.events.clear();
        let awake = bodies.awake;
        for index in 0..awake.ids.len() {
            let Some(record) = bodies.records.get(awake.ids[index]) else {
                continue;
            };
            let Some((_, component)) = record.collider.as_ref() else {
                continue;
            };
            let transform = &awake.transforms[index];
            let body = &awake.bodies[index];
            for (part, &proxy) in record.proxies.iter().enumerate() {
                let Some(shape) = ContactShape::placed_part(component, part, transform) else {
                    continue;
                };
                let Some(tight) = shape.aabb() else {
                    continue;
                };
                // Where it could be by the end of the tick, and a speculative
                // distance past that, turning included.
                let travel = body.velocity * dt;
                let swept = tight.union(Aabb::new(tight.min + travel, tight.max + travel));
                let turn =
                    body.angular_velocity.length() * dt * shape.reach_from(transform.position);
                self.broadphase.update(
                    proxy,
                    swept.inflated(self.settings.speculative_distance + turn),
                );
            }
        }

        let Self {
            broadphase,
            owners,
            planes,
            new_pairs,
            ..
        } = self;
        new_pairs.clear();
        broadphase.find_new_pairs(
            |a, b| {
                // Two parts of one compound are one rigid body: they never
                // collide.
                let body = |proxy: ProxyId| match owners.get(proxy as usize) {
                    Some(Some(Owner::Body(id, _))) => Some(*id),
                    _ => None,
                };
                if body(a).is_some() && body(a) == body(b) {
                    return false;
                }
                let dynamic = |proxy: ProxyId| {
                    owner_side(owners, planes, proxy, bodies).is_some_and(|s| s.inverse_mass > 0.0)
                };
                dynamic(a) || dynamic(b)
            },
            new_pairs,
        );

        for pair in 0..self.new_pairs.len() {
            let (p, q) = self.new_pairs[pair];
            let (Some(sp), Some(sq)) = (self.side(p, bodies), self.side(q, bodies)) else {
                self.broadphase.remove_pair(p, q);
                continue;
            };
            match (self.presence(p, bodies), self.presence(q, bodies)) {
                (Some(Presence::Asleep(id)), Some(Presence::Still))
                | (Some(Presence::Still), Some(Presence::Asleep(id))) => self.events.wake.push(id),
                _ => {}
            }
            let (a, b) = if sq.shape.rank() < sp.shape.rank() {
                (q, p)
            } else {
                (p, q)
            };
            let contact = Contact {
                a,
                b,
                manifold: Manifold::EMPTY,
                impulses: WarmImpulses::default(),
                touching: false,
                cache: SatCache::default(),
            };
            match self.free_contacts.pop() {
                Some(slot) => self.contacts[slot as usize] = Some(contact),
                None => self.contacts.push(Some(contact)),
            }
        }
    }

    /// Every contact's manifold for a tick of `dt`, with last tick's impulses
    /// matched onto the points by feature id, and the pairs whose fat bounds
    /// parted ended.
    ///
    /// A contact neither of whose sides moves, and one of which sleeps, is
    /// left exactly as it was: that is a sleeping island's contact, with
    /// nothing to change it. A sleeping body touched by one that moves is
    /// woken, and contacts beginning or ending between two island bodies are
    /// reported for their islands to merge or be split.
    pub(crate) fn collide(&mut self, bodies: Bodies<'_>, dt: f64) {
        let counters = &mut self.counters;
        counters.touching = 0;
        counters.points = 0;
        counters.persisted = 0;
        counters.begun = 0;
        counters.ended = std::mem::take(&mut self.ended_between_steps);
        counters.worst_penetration = 0.0;

        for slot in 0..self.contacts.len() {
            let Some(mut contact) = self.contacts[slot] else {
                continue;
            };
            let (pa, pb) = (
                self.presence(contact.a, bodies),
                self.presence(contact.b, bodies),
            );
            if let (Some(pa), Some(pb)) = (pa, pb)
                && !pa.moves()
                && !pb.moves()
                && (matches!(pa, Presence::Asleep(_)) || matches!(pb, Presence::Asleep(_)))
            {
                continue;
            }
            let sides = (self.side(contact.a, bodies), self.side(contact.b, bodies));
            let (Some(a), Some(b), Some(pa), Some(pb)) = (sides.0, sides.1, pa, pb) else {
                self.counters.ended += u64::from(self.destroy_contact(slot));
                continue;
            };
            let island_pair = pa.island_body().zip(pb.island_body());
            if !self.broadphase.overlaps(contact.a, contact.b) {
                let was_touching = self.destroy_contact(slot);
                self.counters.ended += u64::from(was_touching);
                if was_touching && let Some((a, _)) = island_pair {
                    self.events.unlinks.push(a);
                }
                continue;
            }

            let manifold = if a.inverse_mass > 0.0 || b.inverse_mass > 0.0 {
                let reach = |side: &Side| {
                    side.velocity.length()
                        + side.angular_velocity.length() * side.shape.reach_from(side.position)
                };
                let speculative = self.settings.speculative_distance + dt * (reach(&a) + reach(&b));
                manifold::collide_cached(&a.shape, &b.shape, speculative, &mut contact.cache)
            } else {
                Manifold::EMPTY
            };

            let mut impulses = WarmImpulses::default();
            let mut manifold = manifold;
            let mut persisted = false;
            for (k, point) in manifold.points_mut().iter_mut().enumerate() {
                if let Some(old) = contact
                    .manifold
                    .points()
                    .iter()
                    .position(|p| p.id == point.id)
                {
                    impulses.normal[k] = contact.impulses.normal[old];
                    persisted = true;
                    self.counters.persisted += 1;
                }
                self.counters.worst_penetration =
                    self.counters.worst_penetration.max(-point.separation);
            }
            // The manifold's friction and twist carry over while any of its
            // points does: a contact that kept none is a new contact.
            if persisted {
                impulses.friction = contact.impulses.friction;
                impulses.twist = contact.impulses.twist;
            }

            let touching = !manifold.points().is_empty();
            if touching && !contact.touching {
                self.counters.begun += 1;
                if let Some(pair) = island_pair {
                    self.events.links.push(pair);
                }
            } else if !touching && contact.touching {
                self.counters.ended += 1;
                if let Some((a, _)) = island_pair {
                    self.events.unlinks.push(a);
                }
            }
            if touching {
                self.counters.touching += 1;
                self.counters.points += manifold.points().len();
                for (this, other) in [(pa, pb), (pb, pa)] {
                    match (this, other) {
                        // Linked as well as woken even when the contact did
                        // not just begin: a body that became dynamic while
                        // touching begins nothing, and this is where its
                        // island finds the one it touches.
                        (Presence::Asleep(id), Presence::Dynamic(other)) => {
                            self.events.wake.push(id);
                            self.events.links.push((id, other));
                        }
                        (Presence::Asleep(id), other) if other.moves() => {
                            self.events.wake.push(id);
                        }
                        (Presence::Dynamic(id), Presence::Moving) => {
                            self.events.stirred.push(id);
                        }
                        _ => {}
                    }
                }
            }
            self.contacts[slot] = Some(Contact {
                manifold,
                impulses,
                touching,
                ..contact
            });
        }
        self.counters.pairs = self.broadphase.pair_count();
    }

    /// Every body touching the body whose proxies are `proxies`, into `out`.
    pub(crate) fn touching_bodies(&self, proxies: &[ProxyId], out: &mut Vec<BodyId>) {
        for contact in self.contacts.iter().flatten() {
            if !contact.touching {
                continue;
            }
            let other = if proxies.contains(&contact.a) {
                contact.b
            } else if proxies.contains(&contact.b) {
                contact.a
            } else {
                continue;
            };
            if let Some(Some(Owner::Body(id, _))) = self.owners.get(other as usize) {
                out.push(*id);
            }
        }
    }

    /// Every touching contact between two bodies of island `island`, in pool
    /// order: what [`Islands::split`] needs.
    pub(crate) fn island_edges(
        &mut self,
        records: &Pool<BodyRecord>,
        island: IslandId,
    ) -> &[(BodyId, BodyId)] {
        self.edges.clear();
        let in_island = |proxy: ProxyId| match self.owners.get(proxy as usize) {
            Some(Some(Owner::Body(id, _)))
                if records.get(*id).and_then(|record| record.island) == Some(island) =>
            {
                Some(*id)
            }
            _ => None,
        };
        for contact in self.contacts.iter().flatten() {
            if contact.touching
                && let (Some(a), Some(b)) = (in_island(contact.a), in_island(contact.b))
            {
                self.edges.push((a, b));
            }
        }
        &self.edges
    }

    /// Every live contact, in pool order.
    pub(crate) fn reports(&self, records: &Pool<BodyRecord>) -> Vec<ContactReport> {
        self.contacts
            .iter()
            .enumerate()
            .filter_map(|(slot, contact)| {
                let contact = contact.as_ref()?;
                Some(ContactReport {
                    slot: slot as u32,
                    a: self.contact_body(contact.a, records)?,
                    b: self.contact_body(contact.b, records)?,
                    part_a: self.part(contact.a),
                    part_b: self.part(contact.b),
                    manifold: contact.manifold,
                    normal_impulses: contact.impulses.normal,
                })
            })
            .collect()
    }

    /// Feeds the planes and every contact's manifold and carried impulses into
    /// a determinism hash.
    ///
    /// Contacts go in sorted by the two sides they join rather than in pool
    /// order, which depends on the history of calls that built the pool, and
    /// every float canonicalised, as [`crate::PhysicsSystem`]'s own state is.
    pub(crate) fn hash_state(&self, records: &Pool<BodyRecord>, hasher: &mut dyn Hasher) {
        let write = |hasher: &mut dyn Hasher, value: f64| {
            hasher.write(&canonical_bits(value).to_le_bytes());
        };
        for plane in &self.planes {
            for value in plane.bounds.normal.to_array() {
                write(hasher, value);
            }
            write(hasher, plane.bounds.offset);
        }
        /// A contact side as the sort sees it: an entity's first part, a
        /// plane, a side with no owner, then an entity's later parts — the
        /// tag, the entity or plane, and the part.
        ///
        /// A compound body has one contact per touching pair of parts, so the
        /// part is what keeps two contacts between the same two bodies apart
        /// and in one order. It goes in under a tag of its own, and only for a
        /// part past the first, so a state with no compound in it hashes as it
        /// did before compounds.
        type Key = (u8, u64, u64);
        let key = |proxy: ProxyId| -> Key {
            match self.contact_body(proxy, records) {
                Some(ContactBody::Entity(entity)) => match self.part(proxy) {
                    0 => (0, entity.to_bits(), 0),
                    part => (3, entity.to_bits(), part as u64),
                },
                Some(ContactBody::Plane(PlaneId(index))) => (1, u64::from(index), 0),
                None => (2, 0, 0),
            }
        };
        let mut live: Vec<(Key, Key, &Contact)> = self
            .contacts
            .iter()
            .flatten()
            .map(|contact| (key(contact.a), key(contact.b), contact))
            .collect();
        live.sort_unstable_by_key(|(a, b, _)| (*a, *b));
        for (a, b, contact) in live {
            hasher.write(&[a.0, b.0]);
            hasher.write(&a.1.to_le_bytes());
            hasher.write(&b.1.to_le_bytes());
            for side in [a, b] {
                if side.0 == 3 {
                    hasher.write(&side.2.to_le_bytes());
                }
            }
            let points = contact.manifold.points();
            hasher.write(&(points.len() as u32).to_le_bytes());
            for value in contact.manifold.normal.to_array() {
                write(hasher, value);
            }
            for (point, &impulse) in points.iter().zip(&contact.impulses.normal) {
                hasher.write(&point.id.to_le_bytes());
                for value in point.point.to_array() {
                    write(hasher, value);
                }
                write(hasher, point.separation);
                write(hasher, impulse);
            }
            for value in contact.impulses.friction.to_array() {
                write(hasher, value);
            }
            write(hasher, contact.impulses.twist);
        }
    }

    fn contact_body(&self, proxy: ProxyId, records: &Pool<BodyRecord>) -> Option<ContactBody> {
        match (*self.owners.get(proxy as usize)?)? {
            Owner::Body(id, _) => Some(ContactBody::Entity(records.get(id)?.entity)),
            Owner::Plane(index) => Some(ContactBody::Plane(PlaneId(index))),
        }
    }

    /// Which part of its collider a proxy is: 0 for anything but a
    /// compound's later parts, planes included.
    fn part(&self, proxy: ProxyId) -> usize {
        match self.owners.get(proxy as usize) {
            Some(Some(Owner::Body(_, part))) => *part,
            _ => 0,
        }
    }

    /// Ends the contact in `slot`, and says whether it was touching.
    fn destroy_contact(&mut self, slot: usize) -> bool {
        let Some(contact) = self.contacts[slot].take() else {
            return false;
        };
        self.broadphase.remove_pair(contact.a, contact.b);
        self.free_contacts.push(slot as u32);
        contact.touching
    }

    fn side(&self, proxy: ProxyId, bodies: Bodies<'_>) -> Option<Side> {
        owner_side(&self.owners, &self.planes, proxy, bodies)
    }

    /// How a proxy's owner takes part this tick, read from its record without
    /// placing its shape: cheap enough to ask of every contact, asleep ones
    /// included.
    fn presence(&self, proxy: ProxyId, bodies: Bodies<'_>) -> Option<Presence> {
        match (*self.owners.get(proxy as usize)?)? {
            Owner::Plane(_) => Some(Presence::Still),
            Owner::Body(id, _) => {
                let record = bodies.records.get(id)?;
                Some(match record.set {
                    BodySet::Static => Presence::Still,
                    BodySet::Sleeping => Presence::Asleep(id),
                    BodySet::Awake => {
                        let body = &bodies.awake.bodies[record.index];
                        if body.is_dynamic() {
                            Presence::Dynamic(id)
                        } else if body.velocity != DVec3::ZERO
                            || body.angular_velocity != DVec3::ZERO
                        {
                            Presence::Moving
                        } else {
                            Presence::Still
                        }
                    }
                })
            }
        }
    }

    /// Whether the contact between `a` and `b` has a side the solver moves:
    /// an awake dynamic body.
    fn solvable(&self, a: ProxyId, b: ProxyId, bodies: Bodies<'_>) -> bool {
        let dynamic = |proxy| matches!(self.presence(proxy, bodies), Some(Presence::Dynamic(_)));
        dynamic(a) || dynamic(b)
    }
}

/// The broadphase tree a body in `set` belongs in: a sleeping body stays in the
/// moving tree, so waking it moves nothing.
const fn proxy_kind(set: BodySet) -> ProxyKind {
    match set {
        BodySet::Static => ProxyKind::Static,
        BodySet::Awake | BodySet::Sleeping => ProxyKind::Moving,
    }
}

/// A proxy's side of a contact, resolved against `bodies`.
fn owner_side(
    owners: &[Option<Owner>],
    planes: &[PlaneRecord],
    proxy: ProxyId,
    bodies: Bodies<'_>,
) -> Option<Side> {
    match (*owners.get(proxy as usize)?)? {
        Owner::Plane(index) => {
            let plane = planes.get(index as usize)?;
            Some(Side {
                shape: ContactShape::Plane {
                    normal: plane.bounds.normal,
                    offset: plane.bounds.offset,
                },
                awake: None,
                entity: None,
                material: plane.material,
                position: DVec3::ZERO,
                velocity: DVec3::ZERO,
                angular_velocity: DVec3::ZERO,
                inverse_mass: 0.0,
                mass: f64::INFINITY,
            })
        }
        Owner::Body(id, part) => {
            let record = bodies.records.get(id)?;
            let (_, component) = record.collider.as_ref()?;
            let transform = transform_of(record, bodies);
            let shape = ContactShape::placed_part(component, part, transform)?;
            let (awake, velocity, angular_velocity, inverse_mass, mass) = match record.set {
                BodySet::Static => (None, DVec3::ZERO, DVec3::ZERO, 0.0, f64::INFINITY),
                BodySet::Sleeping => {
                    let body = bodies.islands.body(record.island, record.index);
                    (None, DVec3::ZERO, DVec3::ZERO, body.inverse_mass, body.mass)
                }
                BodySet::Awake => {
                    let body = &bodies.awake.bodies[record.index];
                    (
                        Some(record.index),
                        body.velocity,
                        body.angular_velocity,
                        body.inverse_mass,
                        body.mass,
                    )
                }
            };
            Some(Side {
                shape,
                awake,
                entity: Some(record.entity),
                material: record.material,
                position: transform.position,
                velocity,
                angular_velocity,
                inverse_mass,
                mass,
            })
        }
    }
}

/// Where the body a record names is.
fn transform_of<'a>(record: &BodyRecord, bodies: Bodies<'a>) -> &'a crate::components::Transform {
    match record.set {
        BodySet::Static => &bodies.statics.transforms[record.index],
        BodySet::Awake => &bodies.awake.transforms[record.index],
        BodySet::Sleeping => bodies.islands.transform(record.island, record.index),
    }
}
