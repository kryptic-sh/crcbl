//! Physics on scene components: a [`Body`] a scene file carries, simulated
//! while a tool plays the scene.
//!
//! ```text
//!     a vocabulary ──▶ scene_physics::register(&mut registry)
//!                         │
//!                         ├── Body under `bodies`   a row of sys/bodies.ron
//!                         ├── a check               a body the files cannot
//!                         │                         reload, reported on save
//!                         └── the module            built per play
//!
//!     play ──▶ the factory loads the scene's files ──▶ every body placed? or
//!              a refusal naming the entity
//!          ──▶ register: one simulated body per Body row, a box at its
//!              entity's placement, in a Simulation system of its own
//!     each tick ──▶ the world's schedule steps the Simulation
//!               ──▶ the module writes each moving body's centre into its
//!                   entity's placing component, at `position`
//! ```
//!
//! # The shape is the placement
//!
//! A body has no shape of its own. Its collider is the box its entity is
//! placed by — the [`Placement`] of the entity's
//! [placing system](Registry::placing_system), which is what a tool draws and
//! picks it by — so what collides is what is drawn, and a body beside a greybox
//! block is that block falling. An entity with a body and nothing placing it
//! has no box to collide with, and play refuses it by name rather than
//! simulating a point.
//!
//! # Which pose wins: the simulation's, written into the placing component
//!
//! The simulation owns the pose while the scene plays, and each tick writes it
//! into the placing component's [`POSITION`] leaves through the registry's
//! reflected path — the same leaves and the same generic route an editor's
//! translate handle uses, so no game's type is named here and a tool draws the
//! motion through the path it already draws edits through. Stop throws the
//! played world away, so the written poses go with it.
//!
//! The write keeps the component's own offset between its `position` and its
//! placement centre: a component whose centre stands half a height above its
//! `position` (a platform standing on its origin) is moved by the distance its
//! body moved, not put with its origin where the centre should be. For a
//! component whose `position` **is** its centre, the written value is the
//! simulated centre exactly.
//!
//! # A body starts at its rotation, and keeps it
//!
//! A placement may be turned ([`OrientedBox::rotation`], from a placing
//! component's `rotation` field), and the body's box is created turned the
//! same way, so a slab tilted in the editor is a ramp that things slide down.
//! **The rotation is then locked**: a dynamic body is built with no rotational
//! inertia, which `crcbl_phys` reads as "torque does nothing and the spin
//! never changes", so it keeps the orientation it was placed at — and the box
//! that collides is still the box that is drawn, because only the centre
//! moves and only the centre is written back. Unlocking it needs the module to
//! write the orientation into the placing component's `rotation` leaves beside
//! [`POSITION`], and an inertia from `crcbl_phys::MassProperties` for the box;
//! `docs/backlog.md` has the entry. Simulating rotation without writing it
//! back was declined: a box resting on its corner while the picture shows it
//! flat is a simulation the picture lies about.
//!
//! # The simulated bodies are not the picking ones
//!
//! A tool that picks through its own `PhysicsSystem` keeps one kinematic box
//! per placed entity, rebuilt from the placements after each tick. The bodies
//! simulated here live in a [`Simulation`] — a system of its own type, holding
//! a `PhysicsSystem` with contacts — so a tool's
//! `World::system_mut::<PhysicsSystem>()` still finds its picking boxes, and
//! rebuilding them cannot overwrite a simulated body. The picking boxes follow
//! the simulation through the placements this module writes.
//!
//! # Determinism
//!
//! Bodies are created in the storage order of the `bodies` system, and
//! `crcbl_phys` steps them in the order they were created. A world loaded from
//! a scene's files holds them in the chunk file's order, so two plays that each
//! start from a load of the same files step the same bodies in the same order
//! — which is why a tool plays the world its snapshot loads into.

use std::fmt;
use std::path::Path;

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crcbl_assets::AssetSource;
use crcbl_ecs::{
    ClientInputs, ComponentHash, DebugCtx, Entity, GameModule, System, SystemTrait, World,
};
use crcbl_phys::{
    ColliderComponent, ContactSettings, GravityForce, PhysicsSystem, RigidBody, SurfaceMaterial,
    Transform,
};
use crcbl_reflect::{Reflect, Value, get_path, set_path};
use crcbl_scene::scn::{Scene, SceneEntityId};

use crate::registry::{OrientedBox, POSITION, Placement, Registry, check_chunk};

/// The scene system every [`Body`] is a row of: the manifest entry, the chunk
/// file's stem, and the system the module is registered under.
pub const BODIES: &str = "bodies";

/// The name of the [`Simulation`] system the module registers on a playing
/// world.
pub const SIMULATION: &str = "body simulation";

/// The acceleration every dynamic body falls at, in m/s²: the Earth's surface,
/// down `-Y`, which is the up a greybox scene is built with.
pub const GRAVITY: GravityForce = GravityForce::EARTH;

/// How a body moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl_reflect")]
pub enum BodyKind {
    /// Falls under [`GRAVITY`], collides, and is pushed by what it hits.
    #[default]
    Dynamic,
    /// Collides and never moves: a floor, a wall.
    Static,
    /// Collides as an immovable mass and is stepped with the dynamic bodies.
    /// Its velocity is zero — no field sets one yet — so it stands where it
    /// was placed, as a static body does; `docs/backlog.md` says what a moving
    /// one needs.
    Kinematic,
}

/// A body a scene carries: how it moves, how heavy it is, and how its surface
/// meets another's. Its shape is its entity's placement — see the
/// [module docs](self).
///
/// The values are refused on load when no simulation could take them
/// ([`Body::check`]), naming the field: a scene with a body of negative mass
/// does not open, rather than opening and failing when it plays.
#[derive(Clone, Copy, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl_reflect")]
#[serde(try_from = "BodyRow")]
pub struct Body {
    /// How it moves. A panel shows it and cannot switch it:
    /// `crcbl_reflect` describes an enum's active variant and has no way to
    /// change which one is active.
    #[reflect(name = "Kind")]
    pub kind: BodyKind,
    /// Its mass in kilograms: finite and above zero, for every kind, so that
    /// a row switched to dynamic is still one a simulation takes. Only a
    /// dynamic body's is read.
    #[reflect(name = "Mass", min = 0.01, max = 1000.0, step = 0.1)]
    pub mass: f64,
    /// Its coefficient of friction: finite and zero or more.
    #[reflect(name = "Friction", min = 0.0, max = 2.0, step = 0.01)]
    pub friction: f64,
    /// Its coefficient of restitution, from zero (no bounce) to one (a
    /// perfectly elastic one).
    #[reflect(name = "Restitution", min = 0.0, max = 1.0, step = 0.01)]
    pub restitution: f64,
}

/// A dynamic body of one kilogram, with `crcbl_phys`'s default surface — what
/// attaching a body to an entity starts it as, which is why
/// [`Registry::register`] asks for one.
impl Default for Body {
    fn default() -> Self {
        Self {
            kind: BodyKind::Dynamic,
            mass: DEFAULT_MASS,
            friction: SurfaceMaterial::DEFAULT.friction,
            restitution: SurfaceMaterial::DEFAULT.restitution,
        }
    }
}

/// The mass a new body starts with, in kilograms.
const DEFAULT_MASS: f64 = 1.0;

impl Body {
    /// `Ok` for values a simulation takes, or the first field that it would
    /// not, with the value.
    ///
    /// # Errors
    ///
    /// [`BodyError`] naming the field: a mass that is not finite and above
    /// zero, a friction that is not finite and zero or more, or a restitution
    /// outside `0..=1`.
    pub fn check(&self) -> Result<(), BodyError> {
        if !(self.mass.is_finite() && self.mass > 0.0) {
            return Err(BodyError::Mass(self.mass));
        }
        if !(self.friction.is_finite() && self.friction >= 0.0) {
            return Err(BodyError::Friction(self.friction));
        }
        if !(0.0..=1.0).contains(&self.restitution) {
            return Err(BodyError::Restitution(self.restitution));
        }
        Ok(())
    }

    /// Its surface, as the contact solver combines it.
    #[must_use]
    pub const fn material(&self) -> SurfaceMaterial {
        SurfaceMaterial::new(self.friction, self.restitution)
    }
}

/// A [`Body`] value no simulation takes, naming the field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodyError {
    /// The mass is not finite and above zero.
    Mass(f64),
    /// The friction is not finite and zero or more.
    Friction(f64),
    /// The restitution is outside `0..=1`.
    Restitution(f64),
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mass(mass) => write!(
                f,
                "a body's `mass` must be a finite number above zero, not {mass}"
            ),
            Self::Friction(friction) => write!(
                f,
                "a body's `friction` must be a finite number of zero or more, not {friction}"
            ),
            Self::Restitution(restitution) => write!(
                f,
                "a body's `restitution` must be from 0 to 1, not {restitution}"
            ),
        }
    }
}

impl std::error::Error for BodyError {}

/// [`Body`] as a file spells it, before [`Body::check`]: what serde reads a
/// row into, so a value no simulation takes is refused by the loader with the
/// file's line and column.
#[derive(Deserialize)]
#[serde(rename = "Body")]
struct BodyRow {
    kind: BodyKind,
    mass: f64,
    friction: f64,
    restitution: f64,
}

impl TryFrom<BodyRow> for Body {
    type Error = BodyError;

    fn try_from(row: BodyRow) -> Result<Self, BodyError> {
        let body = Self {
            kind: row.kind,
            mass: row.mass,
            friction: row.friction,
            restitution: row.restitution,
        };
        body.check()?;
        Ok(body)
    }
}

impl ComponentHash for Body {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        hasher.write_u8(match self.kind {
            BodyKind::Dynamic => 0,
            BodyKind::Static => 1,
            BodyKind::Kinematic => 2,
        });
        for value in [self.mass, self.friction, self.restitution] {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

/// **A body is not a thing in space on its own**: it takes its box from the
/// component that places its entity, so the placement question passes it by
/// and goes on to that one — see [`Registry::placing_system`].
impl Placement for Body {
    fn placement(&self) -> Option<OrientedBox> {
        None
    }
}

/// Registers [`Body`] under [`BODIES`], the check a tool saving a scene runs
/// over its bodies, and the module that simulates them while the scene plays.
///
/// One call, so a game's vocabulary — or a tool's — takes physics on scene
/// components whole.
pub fn register(registry: &mut Registry) {
    registry.register::<Body>(BODIES);
    registry.check(BODIES, check_bodies);
    registry.module(BODIES, start);
}

/// Whether the bodies chunk under `dir` would load: a value a panel set that
/// no simulation takes is reported where it was made, rather than as a scene
/// that will not open next time.
///
/// The chunk alone, read through its own codec: a check is handed no
/// vocabulary, and the rest of the scene is other games' business.
fn check_bodies(source: &dyn AssetSource, dir: &Path) -> Result<(), String> {
    check_chunk::<Body>(source, dir, BODIES)
}

/// The module that simulates the bodies of the scene whose files `source`
/// holds under `dir` — or the first body it cannot simulate, naming its
/// entity.
///
/// The files are loaded through `registry` the way a tool loads them, so the
/// refusal is about the scene as written: a body whose entity has nothing
/// placing it, a placement that is not a box a collider can be, or a placing
/// component with no [`POSITION`] to write the simulated pose into.
fn start(
    registry: &Registry,
    source: &dyn AssetSource,
    dir: &Path,
) -> Result<Box<dyn GameModule>, String> {
    let mut world = World::new();
    registry.register_systems(&mut world);
    let (scene, ids) = Scene::load(source, dir, &registry.codecs(), &mut world)
        .map_err(|error| error.to_string())?;
    for entity in registry.entities(&mut world, &ids, BODIES) {
        if let Err(error) = shape_of(registry, &mut world, entity) {
            let id = ids
                .id(entity)
                .expect("`Registry::entities` answers only entities the map holds");
            return Err(format!("{} {error}", label(&scene, id)));
        }
    }
    Ok(Box::new(BodyPlay {
        registry: registry.clone(),
    }))
}

/// How a refusal names an entity: its name and its id when it has a name, as
/// an outliner row reads, and its id alone when it has none.
fn label(scene: &Scene, id: SceneEntityId) -> String {
    match scene.entity_name(id) {
        Some(name) => format!("entity `{}` #{id}", name.as_str()),
        None => format!("entity #{id}"),
    }
}

/// Why an entity's body cannot be simulated, said of the entity.
#[derive(Debug)]
enum ShapeError {
    /// No scene component of the entity's answers a placement.
    Unplaced,
    /// The placement is not a box a collider can be.
    Degenerate { system: String, shape: OrientedBox },
    /// The placing component has no float leaves at `position.N`.
    NoPosition { system: String, why: String },
}

impl fmt::Display for ShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unplaced => f.write_str(
                "has a body and nothing that places it: a body collides as the box its entity \
                 is placed by",
            ),
            Self::Degenerate { system, shape } => write!(
                f,
                "is placed by `{system}` as a box of half extents {} at {}, which a collider \
                 cannot be: every value must be finite and every half extent above zero",
                shape.half_extents, shape.centre,
            ),
            Self::NoPosition { system, why } => write!(
                f,
                "is placed by `{system}`, whose component has no `{POSITION}` to write a \
                 simulated pose into: {why}"
            ),
        }
    }
}

/// The box `entity` collides as: its placement, checked to be a box a
/// collider can be and placed by a component whose [`POSITION`] the module
/// can write.
fn shape_of(
    registry: &Registry,
    world: &mut World,
    entity: Entity,
) -> Result<OrientedBox, ShapeError> {
    let system = registry
        .placing_system(world, entity)
        .ok_or(ShapeError::Unplaced)?;
    let shape = registry
        .placement(world, entity)
        .ok_or(ShapeError::Unplaced)?;
    let finite = shape.centre.is_finite() && shape.half_extents.is_finite();
    if !(finite && shape.half_extents.min_element() > 0.0) {
        return Err(ShapeError::Degenerate { system, shape });
    }
    let Some(component) = registry.component(world, &system, entity) else {
        return Err(ShapeError::Unplaced);
    };
    for axis in 0..3 {
        let path = format!("{POSITION}.{axis}");
        match get_path(component, &path) {
            Ok(Value::Float(_)) => {}
            Ok(other) => {
                return Err(ShapeError::NoPosition {
                    system,
                    why: format!("`{path}` is {}, not a number", other.kind()),
                });
            }
            Err(error) => {
                return Err(ShapeError::NoPosition {
                    system,
                    why: error.to_string(),
                });
            }
        }
    }
    Ok(shape)
}

/// Every [`Body`] in `world`'s [`BODIES`] system, in storage order — the order
/// the bodies are created in, which the [module docs](self) say why.
fn bodies(world: &mut World) -> Vec<(Entity, Body)> {
    world
        .schedule_mut()
        .iter_mut()
        .find(|system| system.name() == BODIES)
        .and_then(|system| system.as_any_mut().downcast_mut::<System<Body>>())
        .map(|system| {
            system
                .iter_entities()
                .map(|(entity, body)| (entity, *body))
                .collect()
        })
        .unwrap_or_default()
}

/// The simulated bodies of a playing scene: a `PhysicsSystem` with contacts
/// and [`GRAVITY`], under a type of its own so a tool's picking
/// `PhysicsSystem` is still the one `World::system_mut` finds — see the
/// [module docs](self).
///
/// Registered on the playing world by the module, and stepped by the world's
/// schedule at its tick period like any other system; a sweep takes a
/// despawned entity's body out.
pub struct Simulation {
    physics: PhysicsSystem,
    /// The dynamic bodies, in the order they were created: the ones whose pose
    /// is written back. One a sweep took out stays listed and has no pose.
    moving: Vec<Entity>,
}

impl Simulation {
    /// The physics world the bodies are simulated in.
    #[must_use]
    pub const fn physics(&self) -> &PhysicsSystem {
        &self.physics
    }

    /// Each dynamic body's centre as the last step left it, in the order the
    /// bodies were created.
    #[must_use]
    pub fn poses(&self) -> Vec<(Entity, DVec3)> {
        self.moving
            .iter()
            .filter_map(|&entity| {
                self.physics
                    .transform(entity)
                    .map(|transform| (entity, transform.position))
            })
            .collect()
    }
}

impl fmt::Debug for Simulation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Simulation")
            .field("physics", &self.physics)
            .field("moving", &self.moving)
            .finish()
    }
}

impl SystemTrait for Simulation {
    fn name(&self) -> &str {
        SIMULATION
    }

    fn tick(&mut self, dt: f64) {
        SystemTrait::tick(&mut self.physics, dt);
    }

    fn entity_count(&self) -> usize {
        SystemTrait::entity_count(&self.physics)
    }

    /// The dead entities' bodies leave the physics world; [`poses`](Self::poses)
    /// answers only for bodies still in it.
    fn sweep(&mut self, dead: &[Entity]) {
        SystemTrait::sweep(&mut self.physics, dead);
    }

    fn debug_draw(&mut self, ctx: &DebugCtx) {
        SystemTrait::debug_draw(&mut self.physics, ctx);
    }

    fn hash_state(&self, hasher: &mut dyn std::hash::Hasher) {
        SystemTrait::hash_state(&self.physics, hasher);
    }

    fn contributes_to_hash(&self) -> bool {
        SystemTrait::contributes_to_hash(&self.physics)
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The module: builds the [`Simulation`] on register and writes the moving
/// bodies' poses back after each tick — see the [module docs](self).
struct BodyPlay {
    /// The vocabulary the scene was loaded with, which every placement is read
    /// and every pose written through.
    registry: Registry,
}

impl GameModule for BodyPlay {
    fn name(&self) -> &str {
        BODIES
    }

    /// One simulated body per [`Body`] row, at its entity's placement.
    ///
    /// A row the factory would have refused — which a world loaded from the
    /// files it checked cannot hold — is logged and left out rather than
    /// panicking a tool: `register` has no way to refuse.
    fn register(&self, world: &mut World) {
        let mut physics = PhysicsSystem::with_contacts(ContactSettings::DEFAULT);
        physics.add_force_provider(Box::new(GRAVITY));
        let mut moving = Vec::new();
        for (entity, body) in bodies(world) {
            if let Err(error) = body.check() {
                log::warn!("{entity:?} is left out of the simulation: {error}");
                continue;
            }
            let shape = match shape_of(&self.registry, world, entity) {
                Ok(shape) => shape,
                Err(error) => {
                    log::warn!("{entity:?} is left out of the simulation: it {error}");
                    continue;
                }
            };
            let transform = Transform::new(shape.centre, shape.rotation);
            physics.set_transform(entity, transform);
            physics.set_collider(
                entity,
                &ColliderComponent::Box {
                    offset: DVec3::ZERO,
                    half_extents: shape.half_extents,
                    is_trigger: false,
                },
                &transform,
            );
            match body.kind {
                // No inertia tensor: the body keeps the rotation it was
                // placed at — see the module docs.
                BodyKind::Dynamic => {
                    physics.set_body(entity, RigidBody::new_dynamic(body.mass));
                    moving.push(entity);
                }
                BodyKind::Kinematic => physics.set_body(entity, RigidBody::new_kinematic()),
                // A transform and a collider with no body is what
                // `PhysicsSystem` holds as static.
                BodyKind::Static => {}
            }
            physics.set_material(entity, body.material());
        }
        world.register_system(Box::new(Simulation { physics, moving }));
    }

    fn tick(&mut self, world: &mut World, _inputs: ClientInputs<'_>) {
        let poses = world
            .system_mut::<Simulation>()
            .map(|simulation| simulation.poses())
            .unwrap_or_default();
        for (entity, centre) in poses {
            if let Err(why) = follow(&self.registry, world, entity, centre) {
                log::warn!("{entity:?}'s simulated pose was not written back: {why}");
            }
        }
    }
}

/// Writes `centre` into `entity`'s placing component, keeping the component's
/// own offset between its [`POSITION`] and its placement centre — see the
/// [module docs](self). Writes nothing where the component already stands
/// there.
fn follow(
    registry: &Registry,
    world: &mut World,
    entity: Entity,
    centre: DVec3,
) -> Result<(), String> {
    let system = registry
        .placing_system(world, entity)
        .ok_or("nothing places it any more")?;
    let placed = registry
        .placement(world, entity)
        .ok_or("nothing places it any more")?
        .centre;
    let component = registry
        .component(world, &system, entity)
        .ok_or("its placing component is gone")?;
    for (axis, (centre, placed)) in centre
        .to_array()
        .into_iter()
        .zip(placed.to_array())
        .enumerate()
    {
        let path = format!("{POSITION}.{axis}");
        let Value::Float(at) = get_path(component, &path).map_err(|error| error.to_string())?
        else {
            return Err(format!("`{path}` is not a number"));
        };
        let to = centre - (placed - at);
        if to != at {
            set_path(component, &path, &Value::Float(to)).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
