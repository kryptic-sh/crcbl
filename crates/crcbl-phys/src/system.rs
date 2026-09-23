//! ECS physics system: bridges `PhysicsWorld` with entity-component data.
//!
//! [`PhysicsSystem`] implements [`crcbl_ecs::SystemTrait`] so it can be
//! registered in a schedule. It owns a [`crate::PhysicsWorld`], stores
//! per-entity rigid body and transform data, and drives the integration
//! loop with configurable force providers.
//!
//! # Storage: dense sets behind generational ids
//!
//! `docs/plan/36-contact-solver.md` decision 8 sets the layout, after Box3D's:
//! a body is named by a **generational id** — a [`Handle`] from a [`Pool`] of
//! cold records — and the record says which **set** its state lives in and at
//! which **index**. A set is struct-of-arrays: the ids, the transforms and, for
//! the set that moves, the bodies, each column packed with no holes, so
//! [`PhysicsSystem::step`] walks contiguous arrays in their own order and never
//! touches a hash map or sorts anything. The entity-to-id map is consulted only
//! where an entity crosses in: the methods that take an [`Entity`].
//!
//! There are three sets. **Static** holds an entity that has a transform and
//! no [`RigidBody`] — a wall's collider, a replicated prop — and nothing steps
//! it. **Awake** holds every entity with a body, dynamic or kinematic, that
//! steps. **Sleeping** is a set per sleeping island, held by the island, in a
//! system with contacts: see [`crate::contact`]. Whatever changes a sleeping
//! body — [`PhysicsSystem::body_mut`], [`PhysicsSystem::set_transform`] and
//! the rest — wakes its island first, so only the awake and static sets are
//! ever written to from outside a step.
//!
//! **Removal swaps.** Taking a body out of a set moves the set's last body into
//! the hole and rewrites that one record's index, so a set stays dense and its
//! order is a function of the calls that built it. Two runs of one script
//! build the same order; two different call histories reaching the same bodies
//! may not, which is why [`SystemTrait::hash_state`] and
//! [`SystemTrait::replicate`] still visit entities in ascending order, as
//! `crcbl_ecs::System` does — they describe state, not the order it was built
//! in.
//!
//! # Where f32 goes later
//!
//! Decision 7 makes the solver's interior `f32` over `f64` positions once the
//! solver is wide, at rung 6. The awake set is where that lands: velocities,
//! deltas and impulses become a hot column of their own beside `transforms`,
//! indexed exactly like it, while positions stay in `transforms` as `f64`.
//! Adding a column is adding a `Vec` to the awake set and a line to its push
//! and swap-remove; the id, the record and the (set, index) addressing do not
//! change shape.

use std::collections::HashMap;

use crcbl_core::{Handle, Pool};
use crcbl_ecs::{DebugCtx, Entity, SystemTrait};
use glam::DVec3;

use crate::collider::{Aabb, BoxCollider, Capsule, Sphere};
use crate::components::{ColliderComponent, RigidBody, Transform};
use crate::compound_shape::CompoundShape;
use crate::contact::broadphase::ProxyId;
use crate::contact::island::{self, IslandId, Islands};
use crate::contact::{
    Bodies, ContactCounters, ContactPipeline, ContactReport, ContactSettings, KineticContact,
    PlaneId, StageTimes,
};
use crate::forces::ForceProvider;
use crate::integrator::{Integrator as _, SemiImplicitEuler};
use crate::material::SurfaceMaterial;
use crate::query::ShapeHit;
use crate::world::{ColliderId, OverlapQueries, PhysicsWorld, QueryScratch};
use crate::{Ray, Segment};

// ---------------------------------------------------------------------------
// Body storage
// ---------------------------------------------------------------------------

/// A body's generational id: the slot of its [`BodyRecord`].
pub(crate) type BodyId = Handle<BodyRecord>;

/// Which set a body's state lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodySet {
    /// A transform and no body: never stepped.
    Static,
    /// A body, dynamic or kinematic: stepped every [`PhysicsSystem::step`].
    Awake,
    /// A dynamic body whose island sleeps: held by the island, and never
    /// stepped.
    Sleeping,
}

/// The cold half of a body: what names it and what the step never reads.
#[derive(Debug)]
pub(crate) struct BodyRecord {
    /// The entity this body belongs to, for mapping hits and hashing.
    pub(crate) entity: Entity,
    /// The set its state lives in.
    pub(crate) set: BodySet,
    /// Its index in that set's columns.
    pub(crate) index: usize,
    /// Its collider in the world, and the component it was built from.
    pub(crate) collider: Option<(ColliderId, ColliderComponent)>,
    /// Its surface's friction and restitution.
    pub(crate) material: SurfaceMaterial,
    /// Its collider's proxies in the contact broadphase, one per part in part
    /// order — one for anything but a compound — in a system with contacts
    /// and for a collider that is not a trigger. Empty otherwise.
    pub(crate) proxies: Vec<ProxyId>,
    /// Its island, for a dynamic body in a system with contacts that has
    /// stepped since it became one.
    pub(crate) island: Option<IslandId>,
}

/// Transforms with no body: struct-of-arrays, dense, indexed alike.
#[derive(Debug, Default)]
pub(crate) struct StaticSet {
    pub(crate) ids: Vec<BodyId>,
    pub(crate) transforms: Vec<Transform>,
}

/// Bodies that step: struct-of-arrays, dense, indexed alike.
#[derive(Debug, Default)]
pub(crate) struct AwakeSet {
    pub(crate) ids: Vec<BodyId>,
    pub(crate) transforms: Vec<Transform>,
    pub(crate) bodies: Vec<RigidBody>,
    /// How long each body has been slow enough to sleep, in seconds.
    pub(crate) sleep_times: Vec<f64>,
}

impl StaticSet {
    fn push(&mut self, id: BodyId, transform: Transform) -> usize {
        self.ids.push(id);
        self.transforms.push(transform);
        self.ids.len() - 1
    }

    /// Removes `index`, returning its transform and the id now at `index`, if
    /// one moved there.
    fn swap_remove(&mut self, index: usize) -> (Transform, Option<BodyId>) {
        self.ids.swap_remove(index);
        let transform = self.transforms.swap_remove(index);
        (transform, self.ids.get(index).copied())
    }
}

impl AwakeSet {
    /// Adds a body, its sleep timer at zero, and returns its index.
    pub(crate) fn push(&mut self, id: BodyId, transform: Transform, body: RigidBody) -> usize {
        self.ids.push(id);
        self.transforms.push(transform);
        self.bodies.push(body);
        self.sleep_times.push(0.0);
        self.ids.len() - 1
    }

    /// Removes `index`, returning its transform and body and the id now at
    /// `index`, if one moved there.
    pub(crate) fn swap_remove(&mut self, index: usize) -> (Transform, RigidBody, Option<BodyId>) {
        self.ids.swap_remove(index);
        let transform = self.transforms.swap_remove(index);
        let body = self.bodies.swap_remove(index);
        self.sleep_times.swap_remove(index);
        (transform, body, self.ids.get(index).copied())
    }
}

// ---------------------------------------------------------------------------
// PhysicsSystem
// ---------------------------------------------------------------------------

/// ECS system that owns a [`PhysicsWorld`], stores rigid body dynamics data,
/// and runs the integration loop with force providers.
///
/// See the [module docs](self) for how bodies are stored.
///
/// # Integration loop
///
/// Each `tick(dt)` calls `step(dt)` once with the schedule's real tick period.
/// Without contacts the caller is responsible for substepping: call
/// `step(substep_dt)` multiple times per tick to advance physics at a higher
/// rate than the game tick. A system made
/// [`with_contacts`](Self::with_contacts) substeps inside `step` instead,
/// because collision runs once a tick and the solver several times; see
/// [`step`](Self::step).
pub struct PhysicsSystem {
    world: PhysicsWorld,

    /// Every registered entity's cold record, issuing its generational id.
    records: Pool<BodyRecord>,
    /// Entity → body id: the ECS boundary, and read nowhere else.
    entity_to_body: HashMap<Entity, BodyId>,
    /// Transforms with no body.
    statics: StaticSet,
    /// Bodies that step.
    awake: AwakeSet,
    /// The islands, and the sleeping ones' bodies. Empty without contacts.
    islands: Islands,
    /// How many records hold a collider.
    collider_count: usize,
    /// ColliderId → Entity reverse mapping.
    collider_to_entity: Vec<Option<Entity>>,

    /// Force providers applied in order each substep before integration.
    force_providers: Vec<Box<dyn ForceProvider>>,

    /// The buffers an overlap query works in, kept between calls so
    /// [`PhysicsSystem::overlap_sphere_into`] allocates nothing. A caller
    /// querying from several threads brings its own — see
    /// [`EntityOverlapQueries`].
    scratch: QueryScratch,

    /// The contact pipeline, for a system made with
    /// [`with_contacts`](Self::with_contacts).
    contacts: Option<Box<ContactPipeline>>,
}

/// Read-only overlap queries by entity, against a broadphase already built.
///
/// [`PhysicsSystem::overlap_queries`] is the only thing that makes one, and it
/// takes `&mut PhysicsSystem` to build the tree before handing back this shared
/// borrow — so the tree is current for as long as this exists, and nothing can
/// move a body while it does. [`crate::world::OverlapQueries`] carries the full
/// argument; the difference here is only that the hits come back as entities.
///
/// **This is what a `par_for` over a crowd captures.** It is `Copy` and `Sync`,
/// every chunk queries through the same one, and each chunk brings a
/// [`QueryScratch`] of its own.
#[derive(Clone, Copy, Debug)]
pub struct EntityOverlapQueries<'a> {
    queries: OverlapQueries<'a>,
    collider_to_entity: &'a [Option<Entity>],
}

impl EntityOverlapQueries<'_> {
    /// [`PhysicsSystem::overlap_sphere_into`] under a shared borrow, working in
    /// `scratch` instead of the system's own buffers.
    ///
    /// `out` is cleared and then filled, with the same entities in the same
    /// order the `&mut self` form produces — which is the property a sample
    /// swapping one for the other depends on, and which holds because the two
    /// share one traversal rather than because they were compared.
    /// `crcbl_phys::system::tests::the_view_names_the_right_entities_from_every_thread_at_once`
    /// checks the entities against the positions they were placed at.
    pub fn overlap_sphere_into(
        &self,
        centre: DVec3,
        radius: f64,
        scratch: &mut QueryScratch,
        out: &mut Vec<Entity>,
    ) {
        out.clear();
        let mut ids = std::mem::take(&mut scratch.ids);
        self.queries
            .overlap_sphere_into(centre, radius, scratch, &mut ids);
        for id in ids.iter() {
            let Some(entity) = self.entity_for(*id) else {
                continue;
            };
            out.push(entity);
        }
        // Back where it came from, keeping the capacity for the next call.
        scratch.ids = ids;
    }

    /// [`PhysicsSystem::overlap_aabb`] under a shared borrow, writing into a
    /// buffer the caller owns.
    ///
    /// `out` is cleared and then filled with the same entities in the same
    /// order the `&mut self` form produces, because that form is this one.
    /// Broadphase-only, exactly like [`PhysicsSystem::overlap_aabb`]: an entity
    /// whose *AABB* meets `aabb` is named, whatever its shape does.
    pub fn overlap_aabb_into(
        &self,
        aabb: &Aabb,
        scratch: &mut QueryScratch,
        out: &mut Vec<Entity>,
    ) {
        out.clear();
        let mut ids = std::mem::take(&mut scratch.ids);
        self.queries.overlap_aabb_into(aabb, scratch, &mut ids);
        for id in ids.iter() {
            let Some(entity) = self.entity_for(*id) else {
                continue;
            };
            out.push(entity);
        }
        scratch.ids = ids;
    }

    /// [`PhysicsSystem::cast_ray`] under a shared borrow, working in `scratch`
    /// instead of the system's own buffers.
    ///
    /// Triggers are non-solid and are skipped, and a hit on a collider no
    /// entity is mapped to answers `None` — both exactly as in the `&mut self`
    /// form, which calls this one.
    #[must_use]
    pub fn cast_ray(&self, ray: &Ray, scratch: &mut QueryScratch) -> Option<(Entity, ShapeHit)> {
        let (id, hit) = self.queries.cast_ray(ray, scratch)?;
        Some((self.entity_for(id)?, hit))
    }

    /// [`PhysicsSystem::sweep_sphere`] under a shared borrow, working in
    /// `scratch` instead of the system's own buffers.
    ///
    /// Every collider in the world is a candidate, including the sweeper's own
    /// if it has one — there is no entity-level exclusion here, because
    /// [`PhysicsSystem::sweep_body`] needs the body's velocity and transform to
    /// build its segment and this view does not carry them.
    #[must_use]
    pub fn sweep_sphere(
        &self,
        segment: &Segment,
        radius: f64,
        scratch: &mut QueryScratch,
    ) -> Option<(Entity, ShapeHit)> {
        let (id, hit) = self.queries.sweep_sphere(segment, radius, scratch)?;
        Some((self.entity_for(id)?, hit))
    }

    /// The entity a collider id belongs to, or `None` if nothing is mapped to
    /// its slot. The view's copy of [`PhysicsSystem`]'s own reverse-map
    /// lookup — both call one free function, so the two borrow shapes cannot
    /// disagree about which entity a slot is.
    fn entity_for(&self, id: ColliderId) -> Option<Entity> {
        entity_for_in(self.collider_to_entity, id)
    }
}

/// The entity mapped to a collider id's slot, or `None` if there is none.
///
/// Both [`PhysicsSystem`] and [`EntityOverlapQueries`] resolve hits through
/// this, one holding the reverse map and the other borrowing it.
fn entity_for_in(collider_to_entity: &[Option<Entity>], id: ColliderId) -> Option<Entity> {
    collider_to_entity.get(id.index() as usize).and_then(|e| *e)
}

impl PhysicsSystem {
    /// Create an empty physics system.
    ///
    /// Integration is [`SemiImplicitEuler`]; see [`PhysicsSystem::step`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            world: PhysicsWorld::new(),
            records: Pool::new(),
            entity_to_body: HashMap::new(),
            statics: StaticSet::default(),
            awake: AwakeSet::default(),
            islands: Islands::default(),
            collider_count: 0,
            collider_to_entity: Vec::new(),
            force_providers: Vec::new(),
            scratch: QueryScratch::new(),
            contacts: None,
        }
    }

    /// Create an empty physics system whose bodies collide: see
    /// [`crate::contact`].
    ///
    /// Every collider that is not a trigger takes part, and
    /// [`step`](Self::step) becomes a whole tick of
    /// [`ContactSettings::substeps`] solver substeps rather than one
    /// integration.
    #[must_use]
    pub fn with_contacts(settings: ContactSettings) -> Self {
        Self {
            contacts: Some(Box::new(ContactPipeline::new(settings))),
            ..Self::new()
        }
    }

    /// The contact settings, or `None` in a system without contacts.
    #[must_use]
    pub fn contact_settings(&self) -> Option<&ContactSettings> {
        self.contacts.as_ref().map(|pipeline| &pipeline.settings)
    }

    /// The contact settings to change, or `None` in a system without contacts.
    /// A change takes effect on the next [`step`](Self::step).
    pub fn contact_settings_mut(&mut self) -> Option<&mut ContactSettings> {
        self.contacts
            .as_mut()
            .map(|pipeline| &mut pipeline.settings)
    }

    /// Add a static plane: the half-space `normal · x ≤ offset` is solid to
    /// every body with contacts.
    ///
    /// A plane is solver geometry only. It has no entity and no collider in
    /// [`world`](Self::world), so a ray, a sweep or an overlap query passes
    /// straight through it.
    ///
    /// # Panics
    ///
    /// Panics if this system has no contacts, or if `normal` is not of unit
    /// length.
    pub fn add_plane(&mut self, normal: DVec3, offset: f64, material: SurfaceMaterial) -> PlaneId {
        assert!(
            (normal.length_squared() - 1.0).abs() < 1e-9,
            "a plane's normal has unit length: {normal:?}"
        );
        self.contacts
            .as_mut()
            .expect("planes are contact geometry: build the system with_contacts")
            .add_plane(normal, offset, material)
    }

    /// What the last [`step`](Self::step) of the contact pipeline did, or
    /// every counter zero in a system without contacts.
    #[must_use]
    pub fn contact_counters(&self) -> ContactCounters {
        self.contacts
            .as_ref()
            .map_or_else(ContactCounters::default, |pipeline| pipeline.counters)
    }

    /// The [`KineticContact`]s the last [`step`](Self::step) raised, in the
    /// order its contacts were solved.
    #[must_use]
    pub fn kinetic_contacts(&self) -> &[KineticContact] {
        self.contacts
            .as_ref()
            .map_or(&[], |pipeline| pipeline.kinetic.as_slice())
    }

    /// Every contact, touching or not, as the last [`step`](Self::step) left
    /// it, in the pool's order.
    #[must_use]
    pub fn contacts(&self) -> Vec<ContactReport> {
        self.contacts
            .as_ref()
            .map_or_else(Vec::new, |pipeline| pipeline.reports(&self.records))
    }

    /// Number of entities with colliders registered.
    #[must_use]
    pub fn collider_count(&self) -> usize {
        self.collider_count
    }

    /// Number of entities with rigid bodies registered, sleeping ones
    /// included.
    #[must_use]
    pub fn body_count(&self) -> usize {
        self.awake.bodies.len() + self.islands.sleeping_bodies()
    }

    /// Whether `entity`'s body sleeps: its island has been still long enough
    /// that nothing steps it until something wakes it — see
    /// [`crate::contact`]. `false` for an entity with no body, and always in a
    /// system without contacts.
    #[must_use]
    pub fn is_sleeping(&self, entity: Entity) -> bool {
        self.record(entity)
            .is_some_and(|record| record.set == BodySet::Sleeping)
    }

    // ── Dynamics setup ───────────────────────────────────────────────────

    /// Register a rigid body for `entity`.
    ///
    /// Replaces any existing body. The entity will participate in the
    /// integration loop, from the transform it already has or from
    /// [`Transform::IDENTITY`] if it has none. A sleeping body's island wakes.
    pub fn set_body(&mut self, entity: Entity, body: RigidBody) {
        let id = self.record_for(entity, Transform::IDENTITY);
        self.wake(id);
        let record = self.records.get(id).expect("a live record");
        let index = record.index;
        match record.set {
            BodySet::Awake => self.awake.bodies[index] = body,
            BodySet::Sleeping => unreachable!("woken above"),
            BodySet::Static => {
                let (transform, moved) = self.statics.swap_remove(index);
                self.reindex_to(moved, index);
                let awake_index = self.awake.push(id, transform, body);
                let record = self.records.get_mut(id).expect("a live record");
                record.set = BodySet::Awake;
                record.index = awake_index;
                if let Some(pipeline) = self.contacts.as_mut() {
                    for &proxy in &record.proxies {
                        pipeline.body_changed_set(proxy, BodySet::Awake);
                    }
                }
            }
        }
    }

    /// Set the world-space transform for `entity`.
    ///
    /// If the entity has a collider, it is repositioned immediately. A
    /// teleport wakes the entity's island and every sleeping island touching
    /// it where it was; one that lands a static body on a sleeping one wakes
    /// that too, on the next step.
    pub fn set_transform(&mut self, entity: Entity, transform: Transform) {
        let id = self.record_for(entity, transform);
        self.disturb(id);
        *self.transform_slot(id) = transform;
        self.sync_collider(id);
        if let Some(pipeline) = self.contacts.as_mut() {
            pipeline.body_placed(
                id,
                Bodies {
                    records: &self.records,
                    statics: &self.statics,
                    awake: &self.awake,
                    islands: &self.islands,
                },
            );
        }
    }

    /// Get a reference to an entity's rigid body. Reading a sleeping body
    /// does not wake it.
    #[must_use]
    pub fn body(&self, entity: Entity) -> Option<&RigidBody> {
        let record = self.record(entity)?;
        match record.set {
            BodySet::Awake => Some(&self.awake.bodies[record.index]),
            BodySet::Sleeping => Some(self.islands.body(record.island, record.index)),
            BodySet::Static => None,
        }
    }

    /// Get a mutable reference to an entity's rigid body, for a game that
    /// **chooses** a velocity rather than having one integrated onto it.
    ///
    /// [`set_body`](Self::set_body) is the wrong tool for that: it replaces
    /// the whole body to change one `DVec3`, and a crowd sample does it once
    /// per agent per tick. [`apply_force`](Self::apply_force) is not the tool
    /// either — a kinematic body has zero inverse mass, so a force on it is a
    /// no-op by construction, and kinematic is exactly what a steered agent is.
    ///
    /// It hands back the body and nothing else, so it cannot move a collider:
    /// position lives in the transform, and changing that still goes through
    /// [`set_transform`](Self::set_transform), which repositions the broadphase.
    ///
    /// A sleeping body's island wakes: whatever is written — a velocity, a
    /// force — must be stepped. Read with [`body`](Self::body) to leave it
    /// asleep.
    #[must_use]
    pub fn body_mut(&mut self, entity: Entity) -> Option<&mut RigidBody> {
        let &id = self.entity_to_body.get(&entity)?;
        self.wake(id);
        let record = self.records.get(id)?;
        match record.set {
            BodySet::Awake => Some(&mut self.awake.bodies[record.index]),
            BodySet::Static | BodySet::Sleeping => None,
        }
    }

    /// Get a reference to an entity's transform.
    #[must_use]
    pub fn transform(&self, entity: Entity) -> Option<&Transform> {
        let record = self.record(entity)?;
        Some(match record.set {
            BodySet::Awake => &self.awake.transforms[record.index],
            BodySet::Static => &self.statics.transforms[record.index],
            BodySet::Sleeping => self.islands.transform(record.island, record.index),
        })
    }

    /// Set the surface material of `entity`'s body or collider. Returns `false`
    /// if the entity is not registered.
    ///
    /// Every registered entity starts with [`SurfaceMaterial::DEFAULT`]. In a
    /// system with contacts, each contact combines its two surfaces' materials
    /// every tick, so a change takes effect on the next
    /// [`step`](Self::step); see [`crate::material`]. It wakes the entity's
    /// island and every sleeping island touching it, whose friction may no
    /// longer hold them.
    pub fn set_material(&mut self, entity: Entity, material: SurfaceMaterial) -> bool {
        let Some(&id) = self.entity_to_body.get(&entity) else {
            return false;
        };
        self.disturb(id);
        match self.records.get_mut(id) {
            Some(record) => {
                record.material = material;
                true
            }
            None => false,
        }
    }

    /// The surface material of `entity`, if it is registered.
    #[must_use]
    pub fn material(&self, entity: Entity) -> Option<SurfaceMaterial> {
        self.record(entity).map(|record| record.material)
    }

    /// Add a force provider. Providers are applied in order before
    /// integration.
    ///
    /// Providers are **global**: every dynamic body gets every provider. That
    /// is right for a field force — gravity, drag — and wrong for one that
    /// belongs to a single entity, such as the thrust of the one ship among a
    /// screenful of rocks. For that, use [`PhysicsSystem::apply_force`].
    pub fn add_force_provider(&mut self, provider: Box<dyn ForceProvider>) {
        self.force_providers.push(provider);
    }

    /// Add a world-space force to one entity's accumulator for the next
    /// [`PhysicsSystem::step`]. Returns `false` if the entity has no body.
    ///
    /// This is how a game applies a force only some entities feel — the
    /// player's thrust, a shove from a pickup — without a provider that would
    /// apply it to everything. Like a provider's contribution it lasts one
    /// substep: the integrator clears the accumulator, so a force held down
    /// over time is re-applied each tick.
    ///
    /// [`crate::ThrustForce::world_force`] computes the thrust vector to pass
    /// here from a body's orientation, so a per-entity thrust and a pipeline
    /// one are the same model either way.
    ///
    /// A sleeping body's island wakes, as it does for
    /// [`apply_torque`](Self::apply_torque): a force applied every tick keeps
    /// a body awake.
    pub fn apply_force(&mut self, entity: Entity, force: DVec3) -> bool {
        match self.body_mut(entity) {
            Some(body) => {
                body.apply_force(force);
                true
            }
            None => false,
        }
    }

    /// Add a world-space torque to one entity's accumulator for the next
    /// [`PhysicsSystem::step`], on [`apply_force`](Self::apply_force)'s terms.
    /// Returns `false` if the entity has no body.
    ///
    /// A body with no rotational inertia accepts the torque and ignores it;
    /// see [`RigidBody`].
    pub fn apply_torque(&mut self, entity: Entity, torque: DVec3) -> bool {
        match self.body_mut(entity) {
            Some(body) => {
                body.apply_torque(torque);
                true
            }
            None => false,
        }
    }

    // ── Collider management ────────────────────────────────────────────

    /// Add or replace a collider for `entity`.
    ///
    /// The world-space position is `transform.position + component.offset`.
    /// The component is cached so [`PhysicsSystem::step`] can reposition the
    /// collider after integration.
    ///
    /// Replacing a collider wakes on [`remove_collider`](Self::remove_collider)'s
    /// terms.
    pub fn set_collider(
        &mut self,
        entity: Entity,
        component: &ColliderComponent,
        transform: &Transform,
    ) {
        let id = self.record_for(entity, *transform);
        self.wake(id);
        *self.transform_slot(id) = *transform;
        self.remove_collider(entity);

        let world_centre = transform.position;
        let collider = match component {
            ColliderComponent::Sphere {
                offset,
                radius,
                is_trigger,
            } => {
                let centre = world_centre + *offset;
                let collider = self.world.add_sphere(Sphere::new(centre, *radius));
                self.world.set_trigger(collider, *is_trigger);
                collider
            }
            ColliderComponent::Box {
                offset,
                half_extents,
                is_trigger,
            } => {
                let centre = world_centre + *offset;
                let collider = self.world.add_box(BoxCollider::new(centre, *half_extents));
                self.world.set_trigger(collider, *is_trigger);
                collider
            }
            ColliderComponent::Capsule {
                offset,
                radius,
                half_height,
                is_trigger,
            } => {
                let centre = world_centre + *offset;
                let collider = self
                    .world
                    .add_capsule(Capsule::new(centre, *radius, *half_height));
                self.world.set_trigger(collider, *is_trigger);
                collider
            }
            ColliderComponent::Compound {
                offset,
                shape,
                is_trigger,
            } => {
                let collider = self
                    .world
                    .add_box(compound_query_box(shape, *offset, transform));
                self.world.set_trigger(collider, *is_trigger);
                collider
            }
        };

        self.records.get_mut(id).expect("a live record").collider =
            Some((collider, component.clone()));
        if let Some(pipeline) = self.contacts.as_mut() {
            let proxies = pipeline.create_body_proxies(
                id,
                Bodies {
                    records: &self.records,
                    statics: &self.statics,
                    awake: &self.awake,
                    islands: &self.islands,
                },
            );
            self.records.get_mut(id).expect("a live record").proxies = proxies;
        }
        self.collider_count += 1;
        let slot = collider.index() as usize;
        if slot >= self.collider_to_entity.len() {
            self.collider_to_entity.resize(slot + 1, None);
        }
        self.collider_to_entity[slot] = Some(entity);
    }

    /// Remove the collider and dynamics data for `entity`.
    ///
    /// Its island wakes, and so does every sleeping island that touched it:
    /// see [`remove_collider`](Self::remove_collider).
    pub fn remove_entity(&mut self, entity: Entity) {
        self.remove_collider(entity);
        let Some(id) = self.entity_to_body.remove(&entity) else {
            return;
        };
        let Some(record) = self.records.remove(id) else {
            return;
        };
        if let Some(island) = record.island {
            self.islands.remove_member(island, id);
        }
        match record.set {
            BodySet::Static => {
                let (_, moved) = self.statics.swap_remove(record.index);
                self.reindex_to(moved, record.index);
            }
            BodySet::Awake => {
                let (_, _, moved) = self.awake.swap_remove(record.index);
                self.reindex_to(moved, record.index);
            }
            BodySet::Sleeping => unreachable!("remove_collider woke it"),
        }
    }

    /// Remove only the collider (no-op if none).
    ///
    /// A body losing its collider is a support taken away: its island wakes,
    /// and so does every sleeping island touching it — a crate pulled out
    /// from under a sleeping stack drops the stack.
    pub fn remove_collider(&mut self, entity: Entity) {
        let Some(&id) = self.entity_to_body.get(&entity) else {
            return;
        };
        self.disturb(id);
        let Some(record) = self.records.get_mut(id) else {
            return;
        };
        let proxies = std::mem::take(&mut record.proxies);
        if let Some(pipeline) = self.contacts.as_mut()
            && !proxies.is_empty()
        {
            for proxy in proxies {
                pipeline.destroy_proxy(proxy);
            }
            // Its contacts with the rest of its island are gone, so the
            // island may be in pieces.
            if let Some(island) = record.island {
                self.islands.mark_removed(island);
            }
        }
        if let Some((collider, _)) = record.collider.take() {
            self.world.remove(collider);
            self.collider_count -= 1;
            let slot = collider.index() as usize;
            if slot < self.collider_to_entity.len() {
                self.collider_to_entity[slot] = None;
            }
        }
    }

    // ── Integration ────────────────────────────────────────────────────

    /// Advance dynamics by `dt` seconds.
    ///
    /// **Without contacts** this is one substep: all force providers, then
    /// every body integrated — position, velocity, orientation and angular
    /// velocity — with [`SemiImplicitEuler`]. A caller wanting substeps calls
    /// this `n` times with `dt / n`.
    ///
    /// **With contacts** ([`with_contacts`](Self::with_contacts)) this is one
    /// tick: the force providers, then the contact broadphase and every
    /// manifold once, then [`ContactSettings::substeps`] solver substeps of
    /// `dt / substeps`, each integrating the bodies with the same
    /// [`SemiImplicitEuler`] split around the contact impulses. A force applied
    /// before the call is held for the whole tick. A body that touches nothing
    /// integrates exactly as the same number of contact-free substeps would,
    /// short of the speed and rotation caps and of the sweep that follows,
    /// which stops a fast body or a bullet where its path met something. An
    /// island that has been still
    /// long enough sleeps, and a sleeping body is not stepped at all. See
    /// [`crate::contact`].
    ///
    /// Either way, collider positions are synced to the new transforms, and
    /// bodies are visited in the awake set's own order, which is the order of
    /// the calls that registered them; see the [module docs](self).
    pub fn step(&mut self, dt: f64) {
        self.step_with_clock(dt, None);
    }

    /// [`step`](Self::step), reading `clock` — seconds from any fixed origin —
    /// between the stages, so [`ContactCounters::stages`] reports what each
    /// took.
    ///
    /// The clock is the caller's because this crate reads none: a step's
    /// results must not depend on time, and `std::time::Instant` does not
    /// exist in a browser. A system without contacts has no stages and never
    /// reads it.
    pub fn step_timed(&mut self, dt: f64, clock: &mut dyn FnMut() -> f64) {
        self.step_with_clock(dt, Some(clock));
    }

    fn step_with_clock(&mut self, dt: f64, clock: Option<&mut dyn FnMut() -> f64>) {
        if self.contacts.is_some() {
            self.step_contacts(dt, clock);
        } else {
            apply_forces(&mut self.awake, &self.force_providers, dt);
            let AwakeSet {
                transforms, bodies, ..
            } = &mut self.awake;
            for (body, transform) in bodies.iter_mut().zip(transforms.iter_mut()) {
                SemiImplicitEuler.step(body, transform, dt);
            }
            sync_colliders(&mut self.world, &self.records, &self.awake);
        }
    }

    /// One tick of a system with contacts: see [`crate::contact`].
    ///
    /// The forces go on after the narrow phase rather than before it, so an
    /// island woken by a contact this tick feels them this tick; nothing
    /// before the solver reads a force.
    fn step_contacts(&mut self, dt: f64, mut clock: Option<&mut dyn FnMut() -> f64>) {
        let Some(pipeline) = self.contacts.as_mut() else {
            return;
        };
        let settings = pipeline.settings;
        let mut read = || clock.as_mut().map(|clock| clock());
        let start = read();
        if !settings.sleep && self.islands.sleeping_bodies() > 0 {
            self.islands.wake_all(&mut self.records, &mut self.awake);
        }
        self.islands.reconcile(&mut self.records, &self.awake);
        let reconciled = read();

        let lent = Bodies {
            records: &self.records,
            statics: &self.statics,
            awake: &self.awake,
            islands: &self.islands,
        };
        pipeline.update_pairs(lent, dt);
        let paired = read();
        pipeline.collide(lent, dt);
        let collided = read();

        // Wake before linking, so every island a link merges is awake.
        let events = std::mem::take(&mut pipeline.events);
        for &id in &events.wake {
            self.islands
                .wake_body(&mut self.records, &mut self.awake, id);
        }
        for &(a, b) in &events.links {
            self.islands.link(&mut self.records, &mut self.awake, a, b);
        }
        for &id in &events.unlinks {
            if let Some(island) = self.records.get(id).and_then(|record| record.island) {
                self.islands.mark_removed(island);
            }
        }
        let linked = read();

        apply_forces(&mut self.awake, &self.force_providers, dt);
        pipeline.remember_starts(&self.awake);
        pipeline.solve(
            &self.records,
            &self.statics,
            &mut self.awake,
            &self.islands,
            dt,
        );
        let solved = read();
        pipeline.sweep(
            &self.records,
            &self.statics,
            &mut self.awake,
            &self.islands,
            dt,
        );
        let swept = read();

        // Colliders follow before anything sleeps, so a body put to sleep
        // this tick leaves its collider where it stopped.
        sync_colliders(&mut self.world, &self.records, &self.awake);
        let synced = read();

        if settings.sleep {
            island::update_timers(
                &mut self.awake,
                settings.sleep_speed,
                settings.sleep_angular_speed,
                dt,
            );
            for &id in &events.stirred {
                if let Some(record) = self.records.get(id)
                    && record.set == BodySet::Awake
                {
                    self.awake.sleep_times[record.index] = 0.0;
                }
            }
            if let Some(island) =
                self.islands
                    .split_candidate(&self.records, &self.awake, settings.time_to_sleep)
            {
                let edges = pipeline.island_edges(&self.records, island);
                self.islands.split(&mut self.records, island, edges);
            }
            self.islands
                .sleep_ready(&mut self.records, &mut self.awake, settings.time_to_sleep);
        }
        let settled = read();
        pipeline.events = events;

        let counters = &mut pipeline.counters;
        counters.bodies = self.awake.ids.len();
        counters.sleeping = self.islands.sleeping_bodies();
        counters.islands = self.islands.awake_islands();
        counters.sleeping_islands = self.islands.sleeping_islands();
        counters.stages = match (
            start, reconciled, paired, collided, linked, solved, swept, synced, settled,
        ) {
            (
                Some(start),
                Some(reconciled),
                Some(paired),
                Some(collided),
                Some(linked),
                Some(solved),
                Some(swept),
                Some(synced),
                Some(settled),
            ) => Some(StageTimes {
                broadphase: paired - reconciled,
                narrow_phase: collided - paired,
                solver: solved - linked,
                islands: (reconciled - start) + (linked - collided) + (settled - synced),
                continuous: swept - solved,
            }),
            _ => None,
        };
    }

    // ── Queries ────────────────────────────────────────────────────────

    /// Cast a ray, returning the closest hit entity and details.
    #[must_use]
    pub fn cast_ray(&mut self, ray: &Ray) -> Option<(Entity, ShapeHit)> {
        // Lent to the view and put straight back, as
        // [`PhysicsSystem::overlap_sphere_into`] does and for the same reason.
        let mut scratch = std::mem::take(&mut self.scratch);
        let hit = self.overlap_queries().cast_ray(ray, &mut scratch);
        self.scratch = scratch;
        hit
    }

    /// Sweep a sphere, returning the closest hit entity and details.
    ///
    /// Every collider in the world is a candidate. A caller sweeping a body
    /// along its own path wants [`sweep_body`](Self::sweep_body) instead, which
    /// builds the same segment and leaves that body out of the answer.
    #[must_use]
    pub fn sweep_sphere(&mut self, segment: &Segment, radius: f64) -> Option<(Entity, ShapeHit)> {
        let mut scratch = std::mem::take(&mut self.scratch);
        let hit = self
            .overlap_queries()
            .sweep_sphere(segment, radius, &mut scratch);
        self.scratch = scratch;
        hit
    }

    /// Sweep the body at `entity` along the segment it covers in `dt`: from where
    /// it was (`position − velocity·dt`) to where it is (`position`), with the
    /// given `radius`. Returns the closest hit.
    ///
    /// This is the "never miss at any speed" half of CCD — a body moving faster
    /// than its own radius in one tick would tunnel through anything it crossed
    /// between steps, and the swept volume is what catches it.
    ///
    /// # The swept entity is left out of its own answer
    ///
    /// The segment ends where the body is, so the body's own collider sits on
    /// the end of it: swept against itself it is a hit at `t = 0`, closer than
    /// anything it was actually heading for. This excludes it — see
    /// [`PhysicsWorld::sweep_sphere_excluding`], which drops it in the narrow
    /// phase so the next-closest hit survives. An entity with a collider is
    /// therefore swept exactly like one without, and neither the caller nor
    /// this method has to lift a collider out of the world to ask.
    #[must_use]
    pub fn sweep_body(
        &mut self,
        entity: Entity,
        dt: f64,
        radius: f64,
    ) -> Option<(Entity, ShapeHit)> {
        let body = self.body(entity).copied()?;
        let transform = self.transform(entity).copied()?;
        let segment = Segment {
            start: transform.position - body.velocity * dt,
            end: transform.position,
        };
        let own = self
            .record(entity)
            .and_then(|record| record.collider.as_ref())
            .map(|(collider, _)| *collider);
        let (id, hit) = self.world.sweep_sphere_excluding(&segment, radius, own)?;
        Some((self.entity_for(id)?, hit))
    }

    /// Overlap query: return all entities whose collider overlaps the sphere.
    ///
    /// # An overlap has no hit, so none is returned
    ///
    /// This used to answer `(Entity, ShapeHit)` and fill the hit in with
    /// `t: 0.0`, `normal: DVec3::Y` and `started_inside: true` for every result
    /// — an answer that was the same whatever the geometry, and wrong for any
    /// caller that read it. Nothing did: every call site in this workspace
    /// named it `_hit`.
    ///
    /// It is not a gap to fill in later either. An overlap asks *which shapes
    /// are inside this volume*, and that question has no impact time, no impact
    /// point and no single surface normal — two shapes overlapping along a face
    /// have a whole contact patch. This matches what the field does: PhysX's
    /// overlap results carry an actor and a shape and nothing else, and a caller
    /// wanting depth and a normal is expected to ask a *collide* query instead.
    /// [`cast_ray`](Self::cast_ray) and [`sweep_sphere`](Self::sweep_sphere)
    /// are the queries here that genuinely have a hit, and they compute one.
    ///
    /// # The query is shape-aware, and `radius` is expanded by each collider
    ///
    /// The query sphere is tested against every collider's *shape*, so a sphere
    /// collider of radius `r_b` is returned iff its centre is within
    /// `radius + r_b` of `centre` (and a box or capsule by the extent of its own
    /// shape). A caller that wants "everything whose centre is within `radius`"
    /// has to subtract the largest collider radius itself. This is not a corner
    /// of the API — `apps/horde`'s separation query is only correct because it
    /// omits the neighbour's radius on purpose and lets this expansion supply
    /// it; the boundary is pinned by `world::tests`'
    /// `a_sphere_overlap_is_expanded_by_the_colliders_own_radius`.
    #[must_use]
    pub fn overlap_sphere(&mut self, centre: DVec3, radius: f64) -> Vec<Entity> {
        let mut out = Vec::new();
        self.overlap_sphere_into(centre, radius, &mut out);
        out
    }

    /// [`overlap_sphere`](Self::overlap_sphere) writing into a buffer the
    /// caller owns, for a game that queries once per body per tick.
    ///
    /// Same shape-aware semantics as [`overlap_sphere`](Self::overlap_sphere):
    /// a sphere collider of radius `r_b` is returned iff its centre is within
    /// `radius + r_b` of `centre`.
    ///
    /// `out` is cleared and then filled, so the buffer is hoisted out of the
    /// loop and reused. Nothing below this allocates either: the collider ids
    /// land in a scratch buffer of this system's, and the BVH's descent stack
    /// and candidate list are the world's own. A crowd of ten thousand
    /// therefore steers without a single allocation, where the owned form is
    /// three per agent per tick.
    pub fn overlap_sphere_into(&mut self, centre: DVec3, radius: f64, out: &mut Vec<Entity>) {
        // Lent to the view and put straight back — see
        // [`PhysicsWorld::overlap_sphere_into`], which does the same thing for
        // the same reason.
        let mut scratch = std::mem::take(&mut self.scratch);
        self.overlap_queries()
            .overlap_sphere_into(centre, radius, &mut scratch, out);
        self.scratch = scratch;
    }

    /// Builds the broadphase and hands back a shared view that can query it by
    /// entity.
    ///
    /// The system-level twin of [`PhysicsWorld::overlap_queries`], and the
    /// entry point a data-parallel pass over a crowd wants: it is what lets
    /// every chunk of a `crcbl_jobs` `par_for` run its own neighbourhood
    /// queries at the same time. See [`EntityOverlapQueries`].
    pub fn overlap_queries(&mut self) -> EntityOverlapQueries<'_> {
        // Split borrows: the view needs the reverse map shared and the world
        // exclusively, and they are different fields of this struct.
        let Self {
            world,
            collider_to_entity,
            ..
        } = self;
        EntityOverlapQueries {
            queries: world.overlap_queries(),
            collider_to_entity,
        }
    }

    /// Overlap query: return all entities whose AABB intersects `aabb`.
    #[must_use]
    pub fn overlap_aabb(&mut self, aabb: &Aabb) -> Vec<Entity> {
        let mut scratch = std::mem::take(&mut self.scratch);
        let mut out = Vec::new();
        self.overlap_queries()
            .overlap_aabb_into(aabb, &mut scratch, &mut out);
        self.scratch = scratch;
        out
    }

    /// Direct access to the underlying [`PhysicsWorld`] for advanced use.
    #[must_use]
    pub fn world(&self) -> &PhysicsWorld {
        &self.world
    }

    /// Mutable access to the underlying [`PhysicsWorld`] for advanced use.
    pub fn world_mut(&mut self) -> &mut PhysicsWorld {
        &mut self.world
    }

    // ── Internals ──────────────────────────────────────────────────────

    /// The entity a collider id belongs to, or `None` if nothing is mapped to
    /// its slot.
    fn entity_for(&self, id: ColliderId) -> Option<Entity> {
        entity_for_in(&self.collider_to_entity, id)
    }

    /// The record of `entity`, if it is registered.
    fn record(&self, entity: Entity) -> Option<&BodyRecord> {
        self.records.get(*self.entity_to_body.get(&entity)?)
    }

    /// The id of `entity`'s record, registering it in the static set at
    /// `transform` first if it has none.
    fn record_for(&mut self, entity: Entity, transform: Transform) -> BodyId {
        if let Some(&id) = self.entity_to_body.get(&entity) {
            return id;
        }
        let id = self.records.insert(BodyRecord {
            entity,
            set: BodySet::Static,
            index: 0,
            collider: None,
            material: SurfaceMaterial::DEFAULT,
            proxies: Vec::new(),
            island: None,
        });
        let index = self.statics.push(id, transform);
        self.records.get_mut(id).expect("just inserted").index = index;
        self.entity_to_body.insert(entity, id);
        id
    }

    /// The transform of the body `id` names, waking it first if it sleeps.
    fn transform_slot(&mut self, id: BodyId) -> &mut Transform {
        self.wake(id);
        let record = self.records.get(id).expect("a live record");
        match record.set {
            BodySet::Awake => &mut self.awake.transforms[record.index],
            BodySet::Static => &mut self.statics.transforms[record.index],
            BodySet::Sleeping => unreachable!("woken above"),
        }
    }

    /// Wakes the island of the body `id` names, if it sleeps.
    fn wake(&mut self, id: BodyId) {
        self.islands
            .wake_body(&mut self.records, &mut self.awake, id);
    }

    /// Wakes the island of the body `id` names and, while anything sleeps,
    /// every island touching it: what a change to a body that others may rest
    /// on — a teleport, a new material, a collider taken away — must do.
    ///
    /// Finding what touches a body is a walk over every contact, so it is
    /// skipped when nothing sleeps and there is nothing to wake.
    fn disturb(&mut self, id: BodyId) {
        self.wake(id);
        if self.islands.sleeping_bodies() == 0 {
            return;
        }
        let (Some(pipeline), Some(record)) = (self.contacts.as_ref(), self.records.get(id)) else {
            return;
        };
        if record.proxies.is_empty() {
            return;
        }
        let mut touching = Vec::new();
        pipeline.touching_bodies(&record.proxies, &mut touching);
        for other in touching {
            self.wake(other);
        }
    }

    /// Point the record of a body a swap-remove `moved` into `index` at its
    /// new place. `None` is the removal that emptied the end of the set.
    fn reindex_to(&mut self, moved: Option<BodyId>, index: usize) {
        if let Some(record) = moved.and_then(|id| self.records.get_mut(id)) {
            record.index = index;
        }
    }

    /// Reposition the collider of the body `id` names from its cached component
    /// and current transform. No-op if it has no collider.
    fn sync_collider(&mut self, id: BodyId) {
        let Some(record) = self.records.get(id) else {
            return;
        };
        let Some((collider, component)) = &record.collider else {
            return;
        };
        let transform = match record.set {
            BodySet::Awake => &self.awake.transforms[record.index],
            BodySet::Static => &self.statics.transforms[record.index],
            BodySet::Sleeping => self.islands.transform(record.island, record.index),
        };
        place_collider(&mut self.world, *collider, component, transform);
    }
}

/// Every force provider onto every awake body.
fn apply_forces(awake: &mut AwakeSet, providers: &[Box<dyn ForceProvider>], dt: f64) {
    let AwakeSet {
        transforms, bodies, ..
    } = awake;
    for (body, transform) in bodies.iter_mut().zip(transforms.iter()) {
        for provider in providers {
            provider.apply(body, transform, dt);
        }
    }
}

/// Every awake body's collider moved to where its body is.
fn sync_colliders(world: &mut PhysicsWorld, records: &Pool<BodyRecord>, awake: &AwakeSet) {
    for (id, transform) in awake.ids.iter().zip(awake.transforms.iter()) {
        if let Some((collider, component)) = records.get(*id).and_then(|r| r.collider.as_ref()) {
            place_collider(world, *collider, component, transform);
        }
    }
}

/// Move `collider` to where `component` sits on a body at `transform`.
fn place_collider(
    world: &mut PhysicsWorld,
    collider: ColliderId,
    component: &ColliderComponent,
    transform: &Transform,
) {
    let centre = transform.position;
    match component {
        ColliderComponent::Sphere { offset, radius, .. } => {
            world.set_sphere(collider, Sphere::new(centre + *offset, *radius));
        }
        ColliderComponent::Box {
            offset,
            half_extents,
            ..
        } => {
            world.set_box(collider, BoxCollider::new(centre + *offset, *half_extents));
        }
        ColliderComponent::Capsule {
            offset,
            radius,
            half_height,
            ..
        } => {
            world.set_capsule(
                collider,
                Capsule::new(centre + *offset, *radius, *half_height),
            );
        }
        ColliderComponent::Compound { offset, shape, .. } => {
            world.set_box(collider, compound_query_box(shape, *offset, transform));
        }
    }
}

/// What the query world holds for a compound: one box around every part as
/// the body has them turned — see [`ColliderComponent::Compound`].
fn compound_query_box(shape: &CompoundShape, offset: DVec3, transform: &Transform) -> BoxCollider {
    let bounds = shape.world_bounds(offset, transform);
    BoxCollider::new(bounds.centre(), bounds.extents() * 0.5)
}

/// `value`'s bits with every zero and every `NaN` made one: `-0.0` hashes as
/// `+0.0` and any `NaN` as the canonical one, so two states that compare equal
/// hash equal — `docs/plan/36-contact-solver.md` decision 8.
pub(crate) fn canonical_bits(value: f64) -> u64 {
    if value == 0.0 {
        0
    } else if value.is_nan() {
        f64::NAN.to_bits()
    } else {
        value.to_bits()
    }
}

impl Default for PhysicsSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PhysicsSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsSystem")
            .field("collider_count", &self.collider_count)
            .field("body_count", &self.body_count())
            .field("force_provider_count", &self.force_providers.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// SystemTrait impl
// ---------------------------------------------------------------------------

impl SystemTrait for PhysicsSystem {
    fn name(&self) -> &str {
        "physics"
    }

    fn tick(&mut self, dt: f64) {
        // One substep at the schedule's real tick period.  Assuming a rate
        // here instead would make simulated speed a function of the host's
        // tick rate.  Callers wanting sub-stepping call `step(dt / n)` n times
        // directly instead of relying on the schedule.
        self.step(dt);
    }

    fn entity_count(&self) -> usize {
        self.collider_count
    }

    fn sweep(&mut self, dead: &[Entity]) {
        for &entity in dead {
            self.remove_entity(entity);
        }
    }

    fn debug_draw(&mut self, _ctx: &DebugCtx) {
        // Future: draw collider AABBs, contacts, swept paths.
    }

    /// Feed transform and rigid-body state into the determinism hash.
    ///
    /// Entities are visited in ascending `to_bits()` order rather than in the
    /// sets' order, which depends on the history of calls that built them, and
    /// every float goes in canonicalised — `-0.0` as `+0.0`, every `NaN` as one — so the hash
    /// is a function of the simulation state and nothing else.
    ///
    /// In a system with contacts each body's sleep goes in too — whether it
    /// sleeps, and if not how long it has been still — since two states that
    /// differ only there step differently.
    fn hash_state(&self, hasher: &mut dyn std::hash::Hasher) {
        let mut entities: Vec<(u64, &BodyRecord)> = self
            .records
            .iter()
            .map(|(_, record)| (record.entity.to_bits(), record))
            .collect();
        entities.sort_unstable_by_key(|(bits, _)| *bits);

        for (bits, record) in entities {
            hasher.write(&bits.to_le_bytes());
            let (transform, body) = match record.set {
                BodySet::Awake => (
                    &self.awake.transforms[record.index],
                    Some(&self.awake.bodies[record.index]),
                ),
                BodySet::Sleeping => (
                    self.islands.transform(record.island, record.index),
                    Some(self.islands.body(record.island, record.index)),
                ),
                BodySet::Static => (&self.statics.transforms[record.index], None),
            };

            hasher.write(&[1]);
            for value in [
                transform.position.x,
                transform.position.y,
                transform.position.z,
                transform.rotation.x,
                transform.rotation.y,
                transform.rotation.z,
                transform.rotation.w,
            ] {
                hasher.write(&canonical_bits(value).to_le_bytes());
            }

            match body {
                Some(body) => {
                    hasher.write(&[1]);
                    for value in [
                        body.mass,
                        body.inverse_mass,
                        body.velocity.x,
                        body.velocity.y,
                        body.velocity.z,
                        body.force_accum.x,
                        body.force_accum.y,
                        body.force_accum.z,
                        body.angular_velocity.x,
                        body.angular_velocity.y,
                        body.angular_velocity.z,
                        body.torque_accum.x,
                        body.torque_accum.y,
                        body.torque_accum.z,
                    ]
                    .into_iter()
                    .chain(body.local_inertia.to_cols_array())
                    {
                        hasher.write(&canonical_bits(value).to_le_bytes());
                    }
                    // Only a bullet writes its flag, so a state with none
                    // hashes as it did before the flag existed.
                    if body.bullet {
                        hasher.write(&[3]);
                    }
                    if self.contacts.is_some() {
                        match record.set {
                            BodySet::Awake => {
                                hasher.write(&[0]);
                                let still = self.awake.sleep_times[record.index];
                                hasher.write(&canonical_bits(still).to_le_bytes());
                            }
                            BodySet::Sleeping | BodySet::Static => hasher.write(&[1]),
                        }
                    }
                }
                None => hasher.write(&[0]),
            }
        }

        if let Some(pipeline) = &self.contacts {
            hasher.write(&[2]);
            pipeline.hash_state(&self.records, hasher);
        }
    }

    fn contributes_to_hash(&self) -> bool {
        true
    }

    fn replicate(&self, out: &mut Vec<u8>) -> bool {
        // Per-entity Transform: position (3 × f64 LE) then rotation
        // quaternion (4 × f64 LE, x/y/z/w). Entities are sorted by bits so
        // the wire encoding is deterministic across runs and platforms.
        let mut entries: Vec<(u64, &Transform)> = self
            .records
            .iter()
            .map(|(_, record)| {
                let transform = match record.set {
                    BodySet::Awake => &self.awake.transforms[record.index],
                    BodySet::Static => &self.statics.transforms[record.index],
                    BodySet::Sleeping => self.islands.transform(record.island, record.index),
                };
                (record.entity.to_bits(), transform)
            })
            .collect();
        entries.sort_unstable_by_key(|(bits, _)| *bits);
        for (bits, transform) in &entries {
            out.extend_from_slice(&bits.to_le_bytes());
            out.extend_from_slice(&(transform.encoded_len() as u32).to_le_bytes());
            transform.encode(out);
        }
        !entries.is_empty()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forces::GravityForce;
    use glam::DVec3;

    fn test_entity(idx: u32) -> Entity {
        Entity::from_bits(((1u64) << 32) | idx as u64).expect("test entity")
    }

    // ── Existing collider tests (preserved) ────────────────────────────

    #[test]
    fn empty_system_has_no_colliders() {
        let phys = PhysicsSystem::new();
        assert_eq!(phys.collider_count(), 0);
    }

    /// **The view names the entities that are actually within reach, and says
    /// the same thing to every thread asking at once.**
    ///
    /// Two claims, and the fixture serves both. The oracle is the entity
    /// positions themselves, not [`PhysicsSystem::overlap_sphere_into`] — that
    /// is the same code under a different borrow, so comparing them proves
    /// nothing about either. What the system layer adds over
    /// [`crate::world::OverlapQueries`] is the collider-slot-to-entity step, and
    /// a slot mapped to the wrong entity is what this catches.
    ///
    /// The concurrent half is the reason the API exists: several threads share
    /// one view and each brings its own [`crate::world::QueryScratch`], and
    /// every one of them must reach the answers the calling thread reached
    /// alone. It is a guard against a future regression rather than against
    /// today's code — the view holds only shared references, so there is
    /// nothing here for threads to race over, and putting the scratch back
    /// inside it is exactly the change that would make that false.
    #[test]
    fn the_view_names_the_right_entities_from_every_thread_at_once() {
        const RADIUS: f64 = 0.45;
        const REACH: f64 = 1.5;

        let mut phys = PhysicsSystem::new();
        let mut placed: Vec<(Entity, DVec3)> = Vec::new();
        for idx in 0..40u32 {
            let entity = test_entity(idx);
            let position = DVec3::new(f64::from(idx % 7), f64::from(idx / 7), 0.0);
            let transform = Transform::from_position(position);
            phys.set_collider(
                entity,
                &ColliderComponent::Sphere {
                    offset: DVec3::ZERO,
                    radius: RADIUS,
                    is_trigger: false,
                },
                &transform,
            );
            phys.set_transform(entity, transform);
            placed.push((entity, position));
        }

        let centres: Vec<DVec3> = placed.iter().map(|(_, p)| *p).collect();
        // Two spheres overlap when their centres are within the sum of their
        // radii, which is the whole of the test the query is meant to be doing.
        let oracle = |centre: DVec3| {
            let mut hits: Vec<Entity> = placed
                .iter()
                .filter(|(_, p)| p.distance(centre) <= REACH + RADIUS)
                .map(|(e, _)| *e)
                .collect();
            hits.sort_unstable_by_key(|e| e.to_bits());
            hits
        };
        let expected: Vec<Vec<Entity>> = centres.iter().map(|c| oracle(*c)).collect();
        let biggest = expected.iter().map(Vec::len).max().unwrap_or(0);
        assert!(
            biggest > 4,
            "the widest query in the fixture reaches {biggest} entities, which \
             is not a neighbourhood",
        );

        let ask = |queries: &EntityOverlapQueries<'_>| {
            let mut scratch = crate::world::QueryScratch::new();
            let mut out = Vec::new();
            centres
                .iter()
                .map(|centre| {
                    queries.overlap_sphere_into(*centre, REACH, &mut scratch, &mut out);
                    let mut hits = out.clone();
                    hits.sort_unstable_by_key(|entity| entity.to_bits());
                    hits
                })
                .collect::<Vec<_>>()
        };

        let queries = phys.overlap_queries();
        assert_eq!(ask(&queries), expected, "the calling thread's own answers");

        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| ask(&queries)))
                .collect::<Vec<_>>();
            for handle in handles {
                assert_eq!(
                    handle.join().expect("a query thread panicked"),
                    expected,
                    "a thread sharing the view disagreed with the scan",
                );
            }
        });
    }

    /// Every probe's answers from one borrow shape or the other, so a whole
    /// run compares in a single `assert_eq!`.
    #[derive(Debug, PartialEq)]
    struct EntityProbeAnswers {
        rays: Vec<Option<(Entity, ShapeHit)>>,
        sweeps: Vec<Option<(Entity, ShapeHit)>>,
        aabbs: Vec<Vec<Entity>>,
    }

    /// **The view names the right entity for a cast, a sweep and a box query,
    /// and says the same thing to every thread asking at once.**
    ///
    /// The geometry is already pinned a layer down, by
    /// `crate::world::tests::the_shared_view_casts_sweeps_and_boxes_like_a_scan_would`,
    /// which holds the traversal to a brute-force scan. What this layer adds is
    /// the collider-slot-to-entity step, so the oracle here is the entity
    /// *positions*: every probe is aimed so that which entity it must name
    /// follows from where the entities were placed, with no shape arithmetic in
    /// the oracle at all. A slot mapped to the wrong entity is what that
    /// catches, and it is why the expected answers are all different entities.
    ///
    /// The concurrent half is the reason the shared form exists — see
    /// [`the_view_names_the_right_entities_from_every_thread_at_once`], which
    /// makes the same argument for the sphere overlap.
    #[test]
    fn the_view_casts_sweeps_and_boxes_for_the_right_entities_from_every_thread() {
        const RADIUS: f64 = 0.45;
        // Rows are one unit apart and the colliders are narrower than half of
        // that, so a probe aimed down the middle of a row reaches that row and
        // nothing else, and one aimed between two rows reaches neither.
        const ROWS: u32 = 5;
        const COLUMNS: u32 = 7;
        // Narrow enough that the swept volume between two rows still reaches
        // neither: `RADIUS + SWEEP_RADIUS` has to stay under half a row.
        const SWEEP_RADIUS: f64 = 0.04;

        let mut phys = PhysicsSystem::new();
        let mut placed: Vec<(Entity, DVec3)> = Vec::new();
        for idx in 0..ROWS * COLUMNS {
            let entity = test_entity(idx);
            let position = DVec3::new(f64::from(idx % COLUMNS), f64::from(idx / COLUMNS), 0.0);
            let transform = Transform::from_position(position);
            phys.set_collider(
                entity,
                &ColliderComponent::Sphere {
                    offset: DVec3::ZERO,
                    radius: RADIUS,
                    is_trigger: false,
                },
                &transform,
            );
            phys.set_transform(entity, transform);
            placed.push((entity, position));
        }

        // The entity nearest the -X end of a row, which is what a cast or a
        // sweep coming from there must name.
        let first_in_row = |row: f64| {
            placed
                .iter()
                .filter(|(_, p)| p.y == row)
                .min_by(|(_, a), (_, b)| a.x.total_cmp(&b.x))
                .map(|(entity, _)| *entity)
        };

        let rays: Vec<Ray> = (0..ROWS)
            .map(|row| Ray::new(DVec3::new(-10.0, f64::from(row), 0.0), DVec3::X))
            // Between two rows, so it reaches neither.
            .chain(std::iter::once(Ray::new(
                DVec3::new(-10.0, 1.5, 0.0),
                DVec3::X,
            )))
            .collect();
        let expected_rays: Vec<Option<Entity>> = (0..ROWS)
            .map(|row| first_in_row(f64::from(row)))
            .chain(std::iter::once(None))
            .collect();

        let sweeps: Vec<(Segment, f64)> = (0..ROWS)
            .map(|row| {
                (
                    Segment::new(
                        DVec3::new(-10.0, f64::from(row), 0.0),
                        DVec3::new(10.0, f64::from(row), 0.0),
                    ),
                    SWEEP_RADIUS,
                )
            })
            .chain(std::iter::once((
                Segment::new(DVec3::new(-10.0, 1.5, 0.0), DVec3::new(10.0, 1.5, 0.0)),
                SWEEP_RADIUS,
            )))
            .collect();
        let expected_sweeps = expected_rays.clone();

        let aabbs = [
            Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(0.1)),
            Aabb::from_centre_half(DVec3::new(3.0, 2.0, 0.0), DVec3::splat(1.0)),
            Aabb::from_centre_half(DVec3::new(50.0, 50.0, 0.0), DVec3::splat(1.0)),
        ];
        // Two AABBs meet when they overlap on every axis, and a sphere's is its
        // centre grown by its radius — the whole of the test this query does.
        let expected_aabbs: Vec<Vec<Entity>> = aabbs
            .iter()
            .map(|aabb| {
                let mut hits: Vec<Entity> = placed
                    .iter()
                    .filter(|(_, p)| {
                        Aabb::from_centre_half(*p, DVec3::splat(RADIUS)).intersects(aabb)
                    })
                    .map(|(entity, _)| *entity)
                    .collect();
                hits.sort_unstable_by_key(|entity| entity.to_bits());
                hits
            })
            .collect();

        // The oracle has to be able to tell a mix-up from a match: every row's
        // answer is a different entity, and the box probes are neither all
        // empty nor all the same.
        let mut named: Vec<Entity> = expected_rays.iter().flatten().copied().collect();
        assert_eq!(named.len(), ROWS as usize, "every row has a nearest entity");
        named.sort_unstable_by_key(|entity| entity.to_bits());
        named.dedup();
        assert_eq!(
            named.len(),
            ROWS as usize,
            "two rows expect the same entity, so a slot-to-entity mix-up could \\
             pass this",
        );
        let widest = expected_aabbs.iter().map(Vec::len).max().unwrap_or(0);
        assert!(
            widest > 4 && expected_aabbs.iter().any(Vec::is_empty),
            "the box probes reach at most {widest} entities and never nothing, \\
             so one of the two cases never runs",
        );

        let exclusive = EntityProbeAnswers {
            rays: rays.iter().map(|ray| phys.cast_ray(ray)).collect(),
            sweeps: sweeps
                .iter()
                .map(|(segment, radius)| phys.sweep_sphere(segment, *radius))
                .collect(),
            aabbs: aabbs
                .iter()
                .map(|aabb| {
                    let mut hits = phys.overlap_aabb(aabb);
                    hits.sort_unstable_by_key(|entity| entity.to_bits());
                    hits
                })
                .collect(),
        };

        assert_eq!(
            exclusive
                .rays
                .iter()
                .map(|hit| hit.map(|(entity, _)| entity))
                .collect::<Vec<_>>(),
            expected_rays,
            "the entities the casts named",
        );
        assert_eq!(
            exclusive
                .sweeps
                .iter()
                .map(|hit| hit.map(|(entity, _)| entity))
                .collect::<Vec<_>>(),
            expected_sweeps,
            "the entities the sweeps named",
        );
        assert_eq!(exclusive.aabbs, expected_aabbs, "the entities in the boxes");

        // One scratch and one output buffer for the whole run, as a `par_for`
        // chunk holds them: a query that forgot to clear its output is visible
        // here and would not be if every probe got a fresh buffer.
        let ask = |queries: &EntityOverlapQueries<'_>| {
            let mut scratch = crate::world::QueryScratch::new();
            let mut found = Vec::new();
            EntityProbeAnswers {
                rays: rays
                    .iter()
                    .map(|ray| queries.cast_ray(ray, &mut scratch))
                    .collect(),
                sweeps: sweeps
                    .iter()
                    .map(|(segment, radius)| queries.sweep_sphere(segment, *radius, &mut scratch))
                    .collect(),
                aabbs: aabbs
                    .iter()
                    .map(|aabb| {
                        queries.overlap_aabb_into(aabb, &mut scratch, &mut found);
                        let mut hits = found.clone();
                        hits.sort_unstable_by_key(|entity| entity.to_bits());
                        hits
                    })
                    .collect(),
            }
        };

        let queries = phys.overlap_queries();
        assert_eq!(ask(&queries), exclusive, "the calling thread's own answers");

        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4).map(|_| scope.spawn(|| ask(&queries))).collect();
            for handle in handles {
                assert_eq!(
                    handle.join().expect("a query thread panicked"),
                    exclusive,
                    "a thread sharing the view disagreed with the placements",
                );
            }
        });
    }

    #[test]
    fn replicate_encodes_sorted_transform_blobs() {
        let mut phys = PhysicsSystem::new();
        let e1 = test_entity(9);
        let e2 = test_entity(2);
        // Insert out of order — the encoding must still come out sorted.
        phys.set_transform(e1, Transform::from_position(DVec3::new(1.0, 2.0, 3.0)));
        phys.set_transform(
            e2,
            Transform::new(
                DVec3::new(-4.0, 0.5, 8.0),
                glam::DQuat::from_xyzw(0.0, 0.0, 0.0, 1.0),
            ),
        );

        let mut out = Vec::new();
        assert!(SystemTrait::replicate(&phys, &mut out));

        let entity_len = 8 + 4 + Transform::IDENTITY.encoded_len();
        assert_eq!(out.len(), 2 * entity_len);

        let mut cursor = 0usize;
        let mut decoded = Vec::new();
        while cursor < out.len() {
            let bits = u64::from_le_bytes(out[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            let len = u32::from_le_bytes(out[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            decoded.push((bits, Transform::decode(&out[cursor..cursor + len]).unwrap()));
            cursor += len;
        }
        assert_eq!(decoded[0].0, e2.to_bits(), "sorted by entity bits");
        assert_eq!(decoded[1].0, e1.to_bits());
        assert_eq!(decoded[0].1.position, DVec3::new(-4.0, 0.5, 8.0));
        assert_eq!(decoded[1].1.position, DVec3::new(1.0, 2.0, 3.0));

        // An empty system writes nothing and reports as non-replicated.
        let empty = PhysicsSystem::new();
        let mut out = Vec::new();
        assert!(!SystemTrait::replicate(&empty, &mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn transform_encode_decode_roundtrips_and_lerp_midpoint() {
        let t = Transform::new(
            DVec3::new(1.25, -2.5, 3.75),
            crate::rotation_from_scaled_axis(DVec3::Y),
        );
        let mut buf = Vec::new();
        t.encode(&mut buf);
        assert_eq!(buf.len(), t.encoded_len());
        let decoded = Transform::decode(&buf).unwrap();
        assert_eq!(decoded.position, t.position);
        assert!((decoded.rotation.dot(t.rotation) - 1.0).abs() < 1e-12);

        // Wrong-length payloads decode to None.
        assert!(Transform::decode(&buf[..buf.len() - 1]).is_none());
        assert!(Transform::decode(&[]).is_none());

        let a = Transform::from_position(DVec3::new(0.0, 0.0, 0.0));
        let b = Transform::from_position(DVec3::new(10.0, -4.0, 2.0));
        let mid = a.lerp(&b, 0.5);
        assert_eq!(mid.position, DVec3::new(5.0, -2.0, 1.0));
        let at_a = a.lerp(&b, 0.0);
        let at_b = a.lerp(&b, 1.0);
        assert_eq!(at_a.position, a.position);
        assert_eq!(at_b.position, b.position);
    }

    #[test]
    fn set_and_remove_collider() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::from_position(DVec3::new(1.0, 2.0, 3.0));
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &transform);
        assert_eq!(phys.collider_count(), 1);
        phys.remove_collider(e);
        assert_eq!(phys.collider_count(), 0);
    }

    #[test]
    fn ray_cast_hits_entity() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::from_position(DVec3::new(5.0, 0.0, 0.0));
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &transform);
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        let result = phys.cast_ray(&ray);
        assert!(result.is_some());
        let (hit_entity, _) = result.unwrap();
        assert_eq!(hit_entity, e);
    }

    #[test]
    fn ray_cast_misses_when_no_collider() {
        let mut phys = PhysicsSystem::new();
        let ray = Ray::new(DVec3::ZERO, DVec3::X);
        assert!(phys.cast_ray(&ray).is_none());
    }

    #[test]
    fn overlap_aabb_finds_entity() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::from_position(DVec3::new(2.0, 0.0, 0.0));
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &transform);
        let query = Aabb::from_centre_half(DVec3::ZERO, DVec3::splat(3.0));
        let hits = phys.overlap_aabb(&query);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], e);
    }

    #[test]
    fn sweep_removes_dead_entities() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::IDENTITY;
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &transform);
        assert_eq!(phys.collider_count(), 1);
        phys.sweep(&[e]);
        assert_eq!(phys.collider_count(), 0);
    }

    #[test]
    fn the_entity_count_starts_at_zero_and_follows_the_colliders_that_are_set() {
        let mut phys = PhysicsSystem::new();
        assert_eq!(phys.entity_count(), 0);
        let e = test_entity(0);
        phys.set_collider(
            e,
            &ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: 1.0,
                is_trigger: false,
            },
            &Transform::IDENTITY,
        );
        assert_eq!(phys.entity_count(), 1);
    }

    #[test]
    fn replacing_collider_keeps_count_stable() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::IDENTITY;
        let comp1 = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        let comp2 = ColliderComponent::Box {
            offset: DVec3::ZERO,
            half_extents: DVec3::splat(2.0),
            is_trigger: true,
        };
        phys.set_collider(e, &comp1, &transform);
        phys.set_collider(e, &comp2, &transform);
        assert_eq!(phys.collider_count(), 1);
    }

    #[test]
    fn sweep_sphere_hits_entity() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let transform = Transform::from_position(DVec3::new(5.0, 0.0, 0.0));
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &transform);
        let seg = Segment::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::new(5.0, 0.0, 0.0));
        let result = phys.sweep_sphere(&seg, 0.5);
        assert!(result.is_some());
        let (hit_entity, _) = result.unwrap();
        assert_eq!(hit_entity, e);
    }

    /// **`sweep_body` reads the body's own motion, so it catches a crossing a
    /// stationary overlap at the body's position would miss.**
    ///
    /// The body is at the origin moving at `10 u/s`; one second of that motion
    /// is the segment from `(-10, 0, 0)` to `(0, 0, 0)`. A rock sits in the
    /// middle of it, five units from where the body is now — an overlap query
    /// centred there cannot see it, and the sweep exists precisely to. The
    /// entity is the assertion that breaks if `dt` is misapplied (the segment
    /// points the other way and misses the rock entirely); the normal pins the
    /// segment's *direction*, which is the half a start/end swap corrupts
    /// without losing the hit — both are silent under the raw geometry, which
    /// is what this method's tests exist to catch.
    #[test]
    fn sweep_body_catches_a_crossing_a_stationary_overlap_would_miss() {
        let mut phys = PhysicsSystem::new();
        let bullet = test_entity(0);
        let rock = test_entity(1);

        // The swept entity: a kinematic body with no collider, moving along +x.
        let mut body = RigidBody::new_kinematic();
        body.velocity = DVec3::new(10.0, 0.0, 0.0);
        phys.set_body(bullet, body);
        phys.set_transform(bullet, Transform::from_position(DVec3::ZERO));

        // The rock: halfway along the segment `sweep_body` reconstructs as
        // `position - velocity * dt`, and clear of `position` itself.
        phys.set_collider(
            rock,
            &ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: 1.0,
                is_trigger: false,
            },
            &Transform::from_position(DVec3::new(-5.0, 0.0, 0.0)),
        );

        let (hit_entity, hit) = phys
            .sweep_body(bullet, 1.0, 0.5)
            .expect("the sweep crossed the rock");
        assert_eq!(hit_entity, rock, "the sweep hit the wrong entity");
        // Struck on the rock's leading side: a start/end swap turns this into
        // +x without changing which entity is hit.
        assert_eq!(hit.normal, DVec3::NEG_X, "the sweep ran the wrong way");
    }

    /// **A stationary body sweeps nothing it does not already touch.**
    ///
    /// Zero velocity makes the segment zero-length, which is the degenerate
    /// overlap branch of the swept-sphere test rather than a ray — so the rock
    /// sits *beside* the body, close enough that the broadphase offers it (its
    /// AABB reaches into the radius-inflated query bounds) but far enough that
    /// the exact shape test rejects it. `None` is then the shape test's answer,
    /// not the broadphase's.
    #[test]
    fn a_stationary_body_sweeps_nothing_it_does_not_already_touch() {
        let mut phys = PhysicsSystem::new();
        let bullet = test_entity(0);
        phys.set_body(bullet, RigidBody::new_kinematic());
        phys.set_transform(bullet, Transform::from_position(DVec3::ZERO));

        // Diagonally beside the body: inside the broadphase query (1.4 < 0.5 +
        // the rock's AABB reach) but outside the combined reach of 1.5.
        phys.set_collider(
            test_entity(1),
            &ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: 1.0,
                is_trigger: false,
            },
            &Transform::from_position(DVec3::new(1.4, 1.4, 0.0)),
        );

        assert_eq!(phys.sweep_body(bullet, 1.0, 0.5), None);
    }

    /// **The swept body stays registered exactly as it was.**
    ///
    /// The workaround this replaced lifted the collider out of the world for
    /// the duration of the query and put it back: a BVH removal and a BVH
    /// insertion every tick, a recycled storage slot, a bumped generation —
    /// and a window in which the body was not in the world at all, so an early
    /// return between the two calls lost its collider. The id surviving the
    /// sweep unchanged is what says none of that happens any more.
    #[test]
    fn sweep_body_leaves_the_swept_colliders_registration_alone() {
        let mut phys = PhysicsSystem::new();
        let mover = test_entity(0);
        let mut body = RigidBody::new_kinematic();
        body.velocity = DVec3::new(10.0, 0.0, 0.0);
        phys.set_body(mover, body);
        phys.set_collider(
            mover,
            &ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: 1.0,
                is_trigger: false,
            },
            &Transform::from_position(DVec3::ZERO),
        );

        let collider_of = |phys: &PhysicsSystem| {
            phys.record(mover)
                .and_then(|record| record.collider.as_ref())
                .map(|(collider, _)| *collider)
                .expect("the mover has a collider")
        };
        let before = collider_of(&phys);
        for _ in 0..8 {
            let _ = phys.sweep_body(mover, 1.0, 0.5);
        }
        assert_eq!(
            collider_of(&phys),
            before,
            "the sweep re-registered the collider it swept"
        );
        assert_eq!(phys.collider_count(), 1);
    }

    /// **A body with a collider never sweeps into itself.**
    ///
    /// The segment ends where the body is, so the body's own sphere is a hit
    /// near the end of it — at `t = 0.85` for the geometry below. Both halves
    /// break without the exclusion, and they break differently: alone in the
    /// world the mover reports hitting *itself*, and with `wall` just past its
    /// end position the self-hit is the nearer of the two and masks it. The
    /// second is why the exclusion has to happen inside the query rather than
    /// by discarding a self-naming result: the narrow phase keeps one winner,
    /// so throwing that one away throws the wall away with it.
    #[test]
    fn sweep_body_never_hits_the_body_it_sweeps() {
        let sphere = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        let mut phys = PhysicsSystem::new();
        let mover = test_entity(0);
        let wall = test_entity(1);

        let mut body = RigidBody::new_kinematic();
        body.velocity = DVec3::new(10.0, 0.0, 0.0);
        phys.set_body(mover, body);
        phys.set_collider(mover, &sphere, &Transform::from_position(DVec3::ZERO));

        assert_eq!(
            phys.sweep_body(mover, 1.0, 0.5),
            None,
            "the only collider in the world is the mover's own"
        );

        // Just past where the mover ended up: reached at t = 0.90, behind the
        // mover's own shape at t = 0.85.
        phys.set_collider(
            wall,
            &sphere,
            &Transform::from_position(DVec3::new(0.5, 0.0, 0.0)),
        );
        let (hit_entity, hit) = phys
            .sweep_body(mover, 1.0, 0.5)
            .expect("the wall is on the segment");
        assert_eq!(hit_entity, wall, "the mover masked the wall with itself");
        assert!((hit.t - 0.90).abs() < 1e-9, "expected ~0.90, got {}", hit.t);
    }

    /// The `_into` form answers exactly what the owned one does, and reuses
    /// the caller's buffer rather than appending to it.
    ///
    /// The capacity check is the half that matters to the caller it was written
    /// for: a crowd sample runs one query per body per tick, and a buffer that
    /// grew on every call would be the allocation the `_into` form exists to
    /// remove, moved rather than deleted. What it cannot observe from here is
    /// the *inner* buffers — the system's collider-id scratch and the world's
    /// BVH stack and candidate list — which are fields rather than locals now;
    /// that part is structural.
    #[test]
    fn overlap_sphere_into_answers_the_owned_form_and_reuses_the_buffer() {
        let mut phys = PhysicsSystem::new();
        for (index, x) in [0.0, 1.0, 2.0, 40.0].into_iter().enumerate() {
            let e = test_entity(index as u32);
            let at = Transform::from_position(DVec3::new(x, 0.0, 0.0));
            phys.set_collider(
                e,
                &ColliderComponent::Sphere {
                    offset: DVec3::ZERO,
                    radius: 0.5,
                    is_trigger: false,
                },
                &at,
            );
        }

        let owned = phys.overlap_sphere(DVec3::ZERO, 2.0);
        assert!(
            owned.len() > 1 && owned.len() < 4,
            "the fixture must include some colliders and exclude some: {}",
            owned.len(),
        );

        let mut out = Vec::new();
        phys.overlap_sphere_into(DVec3::ZERO, 2.0, &mut out);
        assert_eq!(out, owned, "the two forms disagree");

        // A buffer arriving with something in it comes back with only the
        // answer in it.
        out.push(test_entity(99));
        phys.overlap_sphere_into(DVec3::ZERO, 2.0, &mut out);
        assert_eq!(
            out, owned,
            "the buffer was appended to rather than refilled"
        );

        let capacity = out.capacity();
        for _ in 0..64 {
            phys.overlap_sphere_into(DVec3::ZERO, 2.0, &mut out);
        }
        assert_eq!(
            out.capacity(),
            capacity,
            "the buffer grew on a repeat query, so the loop still allocates",
        );
    }

    // ── Dynamics tests ──────────────────────────────────────────────────

    /// A velocity written through `body_mut` is the one the integrator moves
    /// the body with, and it does **not** move the collider by itself.
    ///
    /// Both halves matter to the caller this was added for: a crowd sample
    /// steers kinematic agents, which no force can move (`inverse_mass` is
    /// zero, so `acceleration = force * inverse_mass` is zero), and it must
    /// still be true that repositioning a body goes through `set_transform` so
    /// the broadphase hears about it.
    #[test]
    fn a_velocity_written_through_body_mut_is_the_one_that_integrates() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        phys.set_body(e, RigidBody::new_kinematic());
        phys.set_transform(e, Transform::from_position(DVec3::ZERO));
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 0.5,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &Transform::from_position(DVec3::ZERO));

        // A force cannot steer this body, which is why `body_mut` exists.
        phys.apply_force(e, DVec3::new(1000.0, 0.0, 0.0));
        phys.step(1.0 / 60.0);
        assert_eq!(
            phys.transform(e).expect("a transform").position,
            DVec3::ZERO,
            "a force moved a kinematic body",
        );

        phys.body_mut(e).expect("a body").velocity = DVec3::new(3.0, 0.0, 0.0);
        let dt = 1.0 / 60.0;
        phys.step(dt);
        assert!(
            (phys.transform(e).expect("a transform").position.x - 3.0 * dt).abs() < 1e-12,
            "the written velocity did not reach the integrator: {:?}",
            phys.transform(e).expect("a transform").position,
        );
        assert_eq!(
            phys.body(e).expect("a body").velocity,
            DVec3::new(3.0, 0.0, 0.0),
            "the write did not survive the step",
        );
    }

    /// Steering a body is not teleporting one: the write alone leaves the
    /// collider where it was, and the **step** is what moves it and tells the
    /// broadphase.
    ///
    /// Both halves are asserted against the same query, because either one
    /// alone is satisfied by a system that does nothing.
    #[test]
    fn a_steered_body_reaches_the_broadphase_on_the_step_and_not_before() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        phys.set_body(e, RigidBody::new_kinematic());
        let at = Transform::from_position(DVec3::new(10.0, 0.0, 0.0));
        phys.set_transform(e, at);
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 0.5,
            is_trigger: false,
        };
        phys.set_collider(e, &comp, &at);

        // Fast enough that one tick carries it clear of the query below.
        phys.body_mut(e).expect("a body").velocity = DVec3::new(0.0, 600.0, 0.0);
        assert_eq!(
            phys.overlap_sphere(DVec3::new(10.0, 0.0, 0.0), 1.0).len(),
            1,
            "writing a velocity moved the collider before anything stepped",
        );

        phys.step(1.0 / 60.0);
        assert!(
            phys.overlap_sphere(DVec3::new(10.0, 0.0, 0.0), 1.0)
                .is_empty(),
            "the step moved the body and the broadphase did not hear about it",
        );
        assert_eq!(
            phys.overlap_sphere(phys.transform(e).expect("a transform").position, 1.0)
                .len(),
            1,
            "the collider is not where the body now is",
        );
    }

    #[test]
    fn body_falls_under_gravity() {
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        let e = test_entity(0);
        phys.set_body(e, RigidBody::new_dynamic(1.0));
        phys.set_transform(e, Transform::from_position(DVec3::new(0.0, 100.0, 0.0)));

        // Step for ~1 second at 60 Hz.
        let dt = 1.0 / 60.0;
        for _ in 0..60 {
            phys.step(dt);
        }

        let t = phys.transform(e).unwrap();
        // After 1s: pos.y ≈ 100 - 0.5 * 9.81 * 1² = 100 - 4.905 ≈ 95.095
        assert!(
            t.position.y < 96.0 && t.position.y > 94.0,
            "expected pos.y ~95.1, got {}",
            t.position.y
        );

        let b = phys.body(e).unwrap();
        // vy ≈ -9.81
        assert!(
            b.velocity.y < -9.0 && b.velocity.y > -10.5,
            "expected vy ~-9.81, got {}",
            b.velocity.y
        );
    }

    #[test]
    fn kinematic_body_stays_put() {
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::EARTH));
        let e = test_entity(0);
        phys.set_body(e, RigidBody::new_kinematic());
        phys.set_transform(e, Transform::from_position(DVec3::new(5.0, 5.0, 5.0)));

        phys.step(1.0 / 60.0);
        let t = phys.transform(e).unwrap();
        assert_eq!(t.position, DVec3::new(5.0, 5.0, 5.0));
    }

    #[test]
    fn collider_moves_with_body_after_integration() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        let body = {
            let mut b = RigidBody::new_dynamic(1.0);
            b.velocity = DVec3::new(10.0, 0.0, 0.0);
            b
        };
        let transform = Transform::from_position(DVec3::new(0.0, 0.0, 0.0));
        let comp = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        phys.set_body(e, body);
        phys.set_collider(e, &comp, &transform);

        // Step: pos moves by vel * dt.
        phys.step(0.5);
        // pos = (5, 0, 0)
        let t = phys.transform(e).unwrap();
        assert!((t.position.x - 5.0).abs() < 1e-12);

        // Collider should have moved to the new position.
        let ray = Ray::new(DVec3::new(-5.0, 0.0, 0.0), DVec3::X);
        let result = phys.cast_ray(&ray);
        assert!(result.is_some());
        let (hit_entity, hit) = result.unwrap();
        assert_eq!(hit_entity, e);
        // Sphere at (5,0,0) radius 1: hit at t = (5 - 1) = 4 from origin at x=-5
        // Actually: ray origin is -5, sphere centre is 5, radius 1 → hit at t ≈ 9
        // Direction is (1,0,0) so t is distance from -5 to 4 = 9.
        assert!((hit.t - 9.0).abs() < 0.01, "t = {}", hit.t);
    }

    #[test]
    fn remove_entity_cleans_up_all_data() {
        let mut phys = PhysicsSystem::new();
        let e = test_entity(0);
        phys.set_body(e, RigidBody::new_dynamic(1.0));
        phys.set_collider(
            e,
            &ColliderComponent::Sphere {
                offset: DVec3::ZERO,
                radius: 1.0,
                is_trigger: false,
            },
            &Transform::IDENTITY,
        );
        assert_eq!(phys.body_count(), 1);
        assert_eq!(phys.collider_count(), 1);

        phys.remove_entity(e);
        assert_eq!(phys.body_count(), 0);
        assert_eq!(phys.collider_count(), 0);
        assert!(phys.body(e).is_none());
        assert!(phys.transform(e).is_none());
    }

    #[test]
    fn multiple_bodies_integrate_independently() {
        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(GravityForce::EARTH));

        let e1 = test_entity(0);
        let e2 = test_entity(1);

        phys.set_body(e1, RigidBody::new_dynamic(1.0));
        phys.set_transform(e1, Transform::from_position(DVec3::new(0.0, 0.0, 0.0)));

        // e2: kinematic, should not move.
        phys.set_body(e2, RigidBody::new_kinematic());
        phys.set_transform(e2, Transform::from_position(DVec3::new(10.0, 10.0, 0.0)));

        phys.step(1.0 / 60.0);

        let t1 = phys.transform(e1).unwrap();
        let t2 = phys.transform(e2).unwrap();
        assert!(t1.position.y < 0.0, "dynamic body should fall");
        assert_eq!(
            t2.position,
            DVec3::new(10.0, 10.0, 0.0),
            "kinematic should not move"
        );
    }

    /// A step over an empty world applies nothing and creates nothing.
    ///
    /// "Did not panic" was the whole of this before, and a `step` that invented
    /// a body out of an empty map, or ran its providers over a default body,
    /// would not have panicked either. The provider counts its own calls
    /// because that is the only way to see the difference: a force applied to a
    /// body nothing else can reach leaves no trace in the counts or the hash.
    #[test]
    fn a_step_with_no_bodies_applies_no_force_and_creates_nothing() {
        use std::cell::Cell;
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher as _;
        use std::rc::Rc;

        #[derive(Debug)]
        struct CountingGravity(Rc<Cell<usize>>);

        impl ForceProvider for CountingGravity {
            fn apply(&self, body: &mut RigidBody, transform: &Transform, dt: f64) {
                self.0.set(self.0.get() + 1);
                GravityForce::EARTH.apply(body, transform, dt);
            }
        }

        let applications = Rc::new(Cell::new(0));

        let mut phys = PhysicsSystem::new();
        phys.add_force_provider(Box::new(CountingGravity(Rc::clone(&applications))));

        let mut before = DefaultHasher::new();
        SystemTrait::hash_state(&phys, &mut before);

        phys.step(1.0 / 60.0);

        assert_eq!(
            applications.get(),
            0,
            "there is no body for a force provider to be applied to"
        );
        assert_eq!(phys.body_count(), 0);
        assert_eq!(phys.collider_count(), 0);

        let mut after = DefaultHasher::new();
        SystemTrait::hash_state(&phys, &mut after);
        assert_eq!(after.finish(), before.finish(), "the step wrote state");
    }

    // ── Schedule integration ────────────────────────────────────────────

    #[test]
    fn tick_uses_the_schedule_dt_rather_than_an_assumed_rate() {
        // Same simulated second, two tick rates.  A hardcoded step would make
        // the 30 Hz run cover half the ground of the 60 Hz one.
        let simulate = |hz: u32| {
            let mut phys = PhysicsSystem::new();
            let e = test_entity(0);
            let mut body = RigidBody::new_dynamic(1.0);
            body.velocity = DVec3::new(10.0, 0.0, 0.0);
            phys.set_body(e, body);
            phys.set_transform(e, Transform::IDENTITY);

            let dt = 1.0 / f64::from(hz);
            for _ in 0..hz {
                SystemTrait::tick(&mut phys, dt);
            }
            phys.transform(e).unwrap().position.x
        };

        let at_60 = simulate(60);
        let at_30 = simulate(30);
        assert!((at_60 - 10.0).abs() < 1e-9, "x = {at_60}");
        assert!((at_30 - 10.0).abs() < 1e-9, "x = {at_30}");
    }

    #[test]
    fn step_visits_bodies_in_canonical_order() {
        // Insertion order must not reach the arithmetic: the `bodies` map is
        // randomly seeded, so anything that depended on it would differ per
        // process.  Two systems built in opposite orders must agree bit for
        // bit.
        let build = |reverse: bool| {
            let mut phys = PhysicsSystem::new();
            phys.add_force_provider(Box::new(GravityForce::EARTH));
            let mut indices: Vec<u32> = (0..16).collect();
            if reverse {
                indices.reverse();
            }
            for i in indices {
                let e = test_entity(i);
                let mut body = RigidBody::new_dynamic(1.0 + f64::from(i) * 0.25);
                body.velocity = DVec3::new(f64::from(i) * 0.1, 0.0, 0.0);
                phys.set_body(e, body);
                phys.set_transform(e, Transform::from_position(DVec3::splat(f64::from(i))));
            }
            for _ in 0..120 {
                phys.step(1.0 / 120.0);
            }
            phys
        };

        let forward = build(false);
        let backward = build(true);
        for i in 0..16 {
            let e = test_entity(i);
            assert_eq!(
                forward.transform(e).unwrap().position,
                backward.transform(e).unwrap().position,
                "entity {i} diverged"
            );
        }
    }

    #[test]
    fn hash_state_covers_physics_state_in_canonical_order() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher as _;

        fn hash(phys: &PhysicsSystem) -> u64 {
            let mut h = DefaultHasher::new();
            SystemTrait::hash_state(phys, &mut h);
            h.finish()
        }

        let build = |reverse: bool, y: f64| {
            let mut phys = PhysicsSystem::new();
            let mut indices: Vec<u32> = (0..8).collect();
            if reverse {
                indices.reverse();
            }
            for i in indices {
                let e = test_entity(i);
                phys.set_body(e, RigidBody::new_dynamic(1.0 + f64::from(i)));
                phys.set_transform(
                    e,
                    Transform::from_position(DVec3::new(f64::from(i), y, 0.0)),
                );
            }
            phys
        };

        // The system must announce that it contributes, or the server's
        // determinism harness silently skips all of physics.
        assert!(SystemTrait::contributes_to_hash(&build(false, 0.0)));

        // Insertion order is not state; position is.
        assert_eq!(hash(&build(false, 0.0)), hash(&build(true, 0.0)));
        assert_ne!(hash(&build(false, 0.0)), hash(&build(false, 1.0)));

        // A velocity change alone must move the hash.
        let mut moving = build(false, 0.0);
        let e = test_entity(3);
        let mut body = *moving.body(e).unwrap();
        body.velocity = DVec3::new(0.0, -1.0, 0.0);
        moving.set_body(e, body);
        assert_ne!(hash(&build(false, 0.0)), hash(&moving));
    }

    // ── Dense storage ───────────────────────────────────────────────────

    /// **Removing bodies from the middle of a set leaves every other one's
    /// body, transform and collider where its entity finds them.**
    ///
    /// A swap-remove moves the set's last body into the hole, so a record whose
    /// index was not rewritten would name its neighbour's state. Each entity
    /// here has a mass, a position and a collider that are its own, so a stale
    /// index shows up as the wrong one of each. Both sets are exercised: the
    /// statics are colliders with no body.
    #[test]
    fn removing_from_the_middle_of_a_set_leaves_every_other_body_its_own_state() {
        let mut phys = PhysicsSystem::new();
        let at = |i: u32| DVec3::new(f64::from(i) * 10.0, 0.0, 0.0);
        let sphere = ColliderComponent::Sphere {
            offset: DVec3::ZERO,
            radius: 1.0,
            is_trigger: false,
        };
        for i in 0..12u32 {
            let e = test_entity(i);
            phys.set_collider(e, &sphere, &Transform::from_position(at(i)));
            if i % 3 != 0 {
                let mut body = RigidBody::new_dynamic(1.0 + f64::from(i));
                body.velocity = DVec3::Y;
                phys.set_body(e, body);
            }
        }
        for i in [1u32, 3, 7, 0, 11] {
            phys.remove_entity(test_entity(i));
        }
        phys.step(1.0);

        for i in 0..12u32 {
            let e = test_entity(i);
            let removed = [1u32, 3, 7, 0, 11].contains(&i);
            if removed {
                assert!(phys.transform(e).is_none(), "{i} survived its removal");
                continue;
            }
            let has_body = i % 3 != 0;
            let want = at(i) + if has_body { DVec3::Y } else { DVec3::ZERO };
            assert_eq!(
                phys.transform(e).expect("a transform").position,
                want,
                "entity {i} has another's transform"
            );
            assert_eq!(
                phys.body(e).map(|body| body.mass),
                has_body.then(|| 1.0 + f64::from(i)),
                "entity {i} has another's body"
            );
            let hit = phys
                .cast_ray(&Ray::new(want - DVec3::Z * 5.0, DVec3::Z))
                .map(|(entity, _)| entity);
            assert_eq!(hit, Some(e), "entity {i}'s collider is not where it is");
        }
        assert_eq!(phys.body_count(), 5);
        assert_eq!(phys.collider_count(), 7);
    }

    /// **Giving a static entity a body moves it into the awake set with its
    /// transform, and leaves the static it swapped with intact.**
    #[test]
    fn a_body_moves_a_static_entity_into_the_awake_set_with_its_transform() {
        let mut phys = PhysicsSystem::new();
        let (wall, prop, crate_) = (test_entity(0), test_entity(1), test_entity(2));
        phys.set_transform(wall, Transform::from_position(DVec3::X));
        phys.set_transform(crate_, Transform::from_position(DVec3::Y));
        phys.set_transform(prop, Transform::from_position(DVec3::Z));

        let mut body = RigidBody::new_kinematic();
        body.velocity = DVec3::X;
        phys.set_body(wall, body);
        phys.step(1.0);

        assert_eq!(
            phys.transform(wall).expect("a transform").position,
            DVec3::X * 2.0
        );
        assert_eq!(
            phys.transform(crate_).expect("a transform").position,
            DVec3::Y
        );
        assert_eq!(
            phys.transform(prop).expect("a transform").position,
            DVec3::Z
        );
        assert!(phys.body(crate_).is_none() && phys.body(prop).is_none());
        assert_eq!(phys.body_count(), 1);
    }

    #[test]
    fn a_material_is_carried_on_its_entity_and_defaults_until_set() {
        let mut phys = PhysicsSystem::new();
        let (ice, floor) = (test_entity(0), test_entity(1));
        let ghost = test_entity(2);
        phys.set_body(ice, RigidBody::new_dynamic(1.0));
        phys.set_transform(floor, Transform::IDENTITY);

        assert_eq!(phys.material(ice), Some(SurfaceMaterial::DEFAULT));
        let slick = SurfaceMaterial::new(0.02, 0.1);
        assert!(phys.set_material(ice, slick));
        assert!(phys.set_material(floor, SurfaceMaterial::new(0.9, 0.0)));
        assert_eq!(phys.material(ice), Some(slick));
        assert_eq!(phys.material(floor).map(|m| m.friction), Some(0.9));

        assert!(!phys.set_material(ghost, slick), "an unregistered entity");
        assert_eq!(phys.material(ghost), None);
        phys.remove_entity(ice);
        assert_eq!(phys.material(ice), None, "the material outlived its entity");
    }

    /// Decision 8's canonical hash: a state that compares equal hashes equal,
    /// so a velocity of `-0.0` is the same state as `+0.0`.
    #[test]
    fn the_hash_does_not_tell_a_negative_zero_from_a_positive_one() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher as _;

        let hash = |vx: f64| {
            let mut phys = PhysicsSystem::new();
            let mut body = RigidBody::new_dynamic(1.0);
            body.velocity = DVec3::new(vx, 1.0, 0.0);
            phys.set_body(test_entity(0), body);
            let mut h = DefaultHasher::new();
            SystemTrait::hash_state(&phys, &mut h);
            h.finish()
        };
        assert_eq!(hash(0.0), hash(-0.0));
        assert_ne!(
            hash(0.0),
            hash(f64::MIN_POSITIVE),
            "the hash cannot see velocity"
        );
    }
}
