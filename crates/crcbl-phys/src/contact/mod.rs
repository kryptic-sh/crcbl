//! Contacts: rungs 1 and 2 of `docs/plan/36-contact-solver.md`.
//!
//! ```text
//!   PhysicsSystem::step(dt), in a system built with contacts
//!
//!   forces ─▶ broadphase ─▶ narrow phase ─▶ solver ×substeps ─▶ restitution
//!             split trees    one manifold    integrate velocities
//!             move buffer    per pair,       warm start
//!             pair set       feature ids     solve (soft, biased)
//!                            matched to      integrate positions
//!                            last tick's     relax (rigid)
//!                            impulses
//! ```
//!
//! Collision runs **once a tick** and the solver runs
//! [`ContactSettings::substeps`] substeps, each with one biased and one relaxed
//! pass, warm-started from the tick before: the Soft Step of decision 1.
//! Restitution is its own pass after the substeps. See `broadphase.rs` in this
//! directory for the pairs, [`manifold`] for the shapes and `solver.rs` for the
//! arithmetic.
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
//! # What is not done yet
//!
//! General convex hulls, and GJK for spheres and capsules against them: the
//! collider set has no hull, and against a box the analytic pairs are exact.
//! Nothing sleeps (rung 3). Nothing sweeps (rung 4): speculative contacts are
//! what stop a fast body, and the speculative distance grows with the pair's
//! speed so they can. The solver is scalar `f64` (rung 6 makes it wide). A
//! tall stack needs [`ContactSettings::TALL_STACK`] for its whole system,
//! since substeps are not yet per group.

pub(crate) mod broadphase;
pub mod manifold;
pub mod shape;
pub(crate) mod solver;

use std::hash::Hasher;

use crcbl_core::Pool;
use crcbl_ecs::Entity;
use glam::DVec3;

use self::broadphase::{Broadphase, PlaneBounds, ProxyId, ProxyKind};
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
}

impl ContactSettings {
    /// Four substeps, 30 Hz contacts at damping ratio 10 pushing out at up to
    /// 3 m/s, a 2 cm speculative distance, bounces above 1 m/s,
    /// 400 m/s and a quarter turn a substep, warm starting on, and any impulse
    /// of a newton-second raising an event.
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
    /// Every substep, the restitution pass and storing the impulses.
    pub solver: f64,
}

/// What the last step of a system with contacts did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactCounters {
    /// Bodies that step.
    pub bodies: usize,
    /// Pairs in the pair set: contacts that exist, touching or not.
    pub pairs: usize,
    /// Contacts whose manifold has a point, speculative ones included.
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
    /// The manifold the last step built.
    pub manifold: Manifold,
    /// Each point's normal impulse as the last step left it, in manifold order.
    pub normal_impulses: [f64; MAX_POINTS],
}

/// What owns a proxy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Body(BodyId),
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
            solver: solver::Scratch::default(),
        }
    }

    // ── Proxies ──────────────────────────────────────────────────────────

    /// Gives body `id` a proxy for its collider, or nothing if the collider is
    /// a trigger.
    pub(crate) fn create_body_proxy(&mut self, id: BodyId, bodies: Bodies<'_>) -> Option<ProxyId> {
        let record = bodies.records.get(id)?;
        let (_, component) = record.collider.as_ref()?;
        let transform = transform_of(record, bodies);
        let shape = ContactShape::placed(component, transform)?;
        let kind = match record.set {
            BodySet::Static => ProxyKind::Static,
            BodySet::Awake => ProxyKind::Moving,
        };
        let proxy = self
            .broadphase
            .create(kind, shape.aabb().expect("a body's shape is bounded"));
        self.set_owner(proxy, Owner::Body(id));
        Some(proxy)
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
        self.broadphase.set_kind(
            proxy,
            match set {
                BodySet::Static => ProxyKind::Static,
                BodySet::Awake => ProxyKind::Moving,
            },
        );
    }

    /// Tells the broadphase a body was placed by hand.
    pub(crate) fn body_placed(&mut self, proxy: ProxyId, id: BodyId, bodies: Bodies<'_>) {
        let Some(record) = bodies.records.get(id) else {
            return;
        };
        let Some((_, component)) = record.collider.as_ref() else {
            return;
        };
        if let Some(bounds) = ContactShape::placed(component, transform_of(record, bodies))
            .and_then(|shape| shape.aabb())
        {
            self.broadphase.update(proxy, bounds);
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
    pub(crate) fn update_pairs(&mut self, bodies: Bodies<'_>, dt: f64) {
        let awake = bodies.awake;
        for index in 0..awake.ids.len() {
            let Some(record) = bodies.records.get(awake.ids[index]) else {
                continue;
            };
            let (Some(proxy), Some((_, component))) = (record.proxy, record.collider.as_ref())
            else {
                continue;
            };
            let transform = &awake.transforms[index];
            let body = &awake.bodies[index];
            let Some(shape) = ContactShape::placed(component, transform) else {
                continue;
            };
            let Some(tight) = shape.aabb() else {
                continue;
            };
            // Where it could be by the end of the tick, and a speculative
            // distance past that, turning included.
            let travel = body.velocity * dt;
            let swept = tight.union(Aabb::new(tight.min + travel, tight.max + travel));
            let turn = body.angular_velocity.length() * dt * shape.reach_from(transform.position);
            self.broadphase.update(
                proxy,
                swept.inflated(self.settings.speculative_distance + turn),
            );
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
            let sides = (self.side(contact.a, bodies), self.side(contact.b, bodies));
            let (Some(a), Some(b)) = sides else {
                self.counters.ended += u64::from(self.destroy_contact(slot));
                continue;
            };
            if !self.broadphase.overlaps(contact.a, contact.b) {
                self.counters.ended += u64::from(self.destroy_contact(slot));
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
            } else if !touching && contact.touching {
                self.counters.ended += 1;
            }
            if touching {
                self.counters.touching += 1;
                self.counters.points += manifold.points().len();
            }
            self.contacts[slot] = Some(Contact {
                manifold,
                impulses,
                touching,
                ..contact
            });
        }
        self.counters.pairs = self.broadphase.pair_count();
        self.counters.bodies = bodies.awake.ids.len();
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
        /// A contact side as the sort sees it: entities first, then planes.
        type Key = (u8, u64);
        let key = |proxy: ProxyId| -> Key {
            match self.contact_body(proxy, records) {
                Some(ContactBody::Entity(entity)) => (0, entity.to_bits()),
                Some(ContactBody::Plane(PlaneId(index))) => (1, u64::from(index)),
                None => (2, 0),
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
            Owner::Body(id) => Some(ContactBody::Entity(records.get(id)?.entity)),
            Owner::Plane(index) => Some(ContactBody::Plane(PlaneId(index))),
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
        Owner::Body(id) => {
            let record = bodies.records.get(id)?;
            let (_, component) = record.collider.as_ref()?;
            let transform = transform_of(record, bodies);
            let shape = ContactShape::placed(component, transform)?;
            let (awake, velocity, angular_velocity, inverse_mass, mass) = match record.set {
                BodySet::Static => (None, DVec3::ZERO, DVec3::ZERO, 0.0, f64::INFINITY),
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
    }
}
