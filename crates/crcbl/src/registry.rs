//! The component registry: what a tool needs to know to open a scene it did not
//! write.
//!
//! A `.scn/` chunk file holds one system's component array, and reading it needs
//! the **type** its rows are of —
//! [`chunk_of::<T>`](crcbl_scene::scn::chunk_of) is bounded on it. So a tool
//! that opens a scene has to be told, per manifest entry, what that entry is
//! made of. Slice 1 of `docs/plan/08-editor.md` told `apps/editor` by hand, in a
//! module with one entry and a dependency on the game that owned it; this is
//! that module generalised, and the arrow it replaces is gone.
//!
//! ```text
//!     a game ──▶ Registry::register::<Brick>("bricks")
//!                     │
//!                     ├── codecs()          the chunk file, read and written
//!                     ├── register_systems  the System<Brick> a load spawns into
//!                     ├── component()       &mut dyn Reflect, for an edit
//!                     └── placement()       a centre and half extents, for a
//!                                           collider and a bounds box
//! ```
//!
//! # One call registers all four, which is why they cannot drift
//!
//! [`Registry::register`] takes **one** type parameter and produces every half
//! at once. There is no way to register a codec and forget the system, or to
//! register a system the editor cannot reach a component of: the four are
//! monomorphised from one `T` at one call site, and a type that cannot answer
//! all four does not compile. That is the failure this module exists to remove —
//! "not registered" arriving as "nothing to edit" — closed at compile time
//! rather than detected at run time.
//!
//! What is left for run time is the other direction: a scene whose manifest
//! names a system **nobody registered**. [`Scene::load`](crcbl_scene::scn::Scene::load)
//! already refuses that by name
//! ([`ScnError::NoCodec`](crcbl_scene::scn::ScnError::NoCodec)), and because
//! [`Registry::codecs`] is the whole registry there is no way to hand it a
//! shorter list than the one the systems were registered from.
//!
//! # Where this lives, and why it is here rather than lower down
//!
//! It needs three crates at once: [`crcbl_ecs`] for the `World` and the
//! `System<T>`, [`crcbl_scene::scn`] for the codec, and [`crcbl_reflect`] for
//! the accessor. Every candidate below the umbrella would have to gain an arrow
//! it should not have:
//!
//! * **`crcbl-scene`** would gain `crcbl-reflect`. Both crates' docs say that
//!   arrow is deliberately absent — `crates/crcbl-reflect/src/lib.rs`'s "the
//!   arrow between them is deliberately absent: nothing here depends on
//!   `crcbl-scene`, and `crcbl-scene` does not depend on this" — and it would
//!   put a panel's display names and slider ranges in the crate a shipped game
//!   links to read its level.
//! * **`crcbl-ecs`** would gain `crcbl-scene`, which is a cycle: that crate
//!   already depends on this one. `crates/crcbl-scene/src/scn.rs` makes the
//!   same argument from the other side about `serde` — "the entity crate, whose
//!   dependency list is `crcbl-core` and nothing else".
//! * **A new crate** would be correct and would cost the workspace members, the
//!   dependency and licence gates, the docs jobs and the wasm legs — for a type
//!   whose every consumer already takes this umbrella.
//!
//! So it is a module of the facade, behind the same feature that re-exports the
//! scene format — `scn` or `scene`, either of which brings [`crcbl_scene`] in,
//! and without which there is no [`chunk_of`] to build an entry from. A game
//! registers its components with the crate it already depends on, and so does
//! the tool.
//!
//! # What this is not
//!
//! * **Not a type registry.** Nothing here constructs a component, names one by
//!   string, or reflects over a type that was never registered.
//!   `crates/crcbl-reflect/src/lib.rs` says why that is a different job.
//! * **Not a scene-format change.** An entity's placement comes from the
//!   [`Placement`] impl of the component it already has, not from a new chunk
//!   row: `crcbl::phys::Transform` on the entity is the general answer and it is
//!   a format change nobody has decided yet — `docs/plan/08-editor.md`'s missing
//!   piece 6.
//! * **Not a schedule.** [`Registry::register_systems`] adds the systems a
//!   scene's chunks load into and nothing else; a tool that also wants physics,
//!   or a game that wants its own, registers it beside them.

use std::any::type_name;
use std::collections::BTreeMap;
use std::fmt;

use glam::DVec3;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crcbl_ecs::{ComponentHash, Entity, System, World};
use crcbl_reflect::Reflect;
use crcbl_scene::scn::{IdMap, SystemChunk, chunk_of};

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

/// Where a component's entity stands in the world, and how far it reaches.
///
/// **The smallest honest answer to "which of a component's fields is a
/// placement".** `#[derive(Reflect)]` says which fields a panel may edit and
/// deliberately says nothing about which of them is a position; a collider and a
/// selection box both need one, and until a scene can carry a
/// `crcbl::phys::Transform` of its own — the format change
/// `docs/plan/08-editor.md`'s missing piece 6 describes, and an open decision —
/// the component is the only thing that knows.
///
/// A trait rather than a derive attribute or an accessor stored in the registry,
/// for two reasons:
///
/// * it is a **bound on [`Registry::register`]**, so a component registered for
///   a tool that cannot say where it is fails to compile rather than loading as
///   a row nothing can pick;
/// * it lives beside the fields it reads, so renaming one breaks the impl in the
///   same file rather than in a tool's hand-written vocabulary.
///
/// # A row that is not a thing in space
///
/// [`None`], and that is a real answer rather than a missing one: `apps/puppet`'s
/// `Sun` is a direction and an intensity, and a scene made of it has a row an
/// outliner lists and a ray cannot hit.
pub trait Placement {
    /// This component's centre and its **half** extents, in simulation space —
    /// which is what both a box collider and a debug-draw box want — or [`None`]
    /// for a row that is not a thing in space.
    fn placement(&self) -> Option<(DVec3, DVec3)>;
}

// ---------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------

/// What a tool needs, per scene system name: the codec, the system, the
/// `&mut dyn Reflect` and the [`Placement`].
///
/// Built once at start-up, from one `register` call per component. A tool holds
/// one and asks it; a game hands one out, and uses the same one to load its own
/// scene, so the vocabulary a game ships and the vocabulary a tool sees are the
/// same list rather than two that agree today.
///
/// ```
/// use crcbl::ecs::{ComponentHash, World};
/// use crcbl::math::DVec3;
/// use crcbl::reflect::Reflect;
/// use crcbl::registry::{Placement, Registry};
/// use crcbl::serde::{Deserialize, Serialize};
/// use std::hash::Hasher;
///
/// #[derive(Reflect, Serialize, Deserialize)]
/// #[reflect(crate = "crcbl::reflect")]
/// #[serde(crate = "crcbl::serde")]
/// struct Prop {
///     position: [f64; 3],
/// }
///
/// impl ComponentHash for Prop {
///     fn hash_component(&self, hasher: &mut dyn Hasher) {
///         for value in self.position {
///             hasher.write(&value.to_bits().to_le_bytes());
///         }
///     }
/// }
///
/// impl Placement for Prop {
///     fn placement(&self) -> Option<(DVec3, DVec3)> {
///         Some((DVec3::from_array(self.position), DVec3::splat(0.5)))
///     }
/// }
///
/// let mut registry = Registry::new();
/// registry.register::<Prop>("props");
///
/// assert_eq!(registry.systems().collect::<Vec<_>>(), ["props"]);
/// assert_eq!(registry.codecs().len(), 1);
///
/// // The codec and the system are one registration, so a world built from the
/// // registry is one every codec can read into.
/// let mut world = World::new();
/// registry.register_systems(&mut world);
/// assert_eq!(world.schedule().len(), 1);
/// ```
#[derive(Default)]
pub struct Registry {
    /// Keyed by system name, which is what a manifest entry, a chunk file's stem
    /// and [`Scene::load`](crcbl_scene::scn::Scene::load) all join on. Sorted, so
    /// [`Registry::systems`] has an order that is a function of the names rather
    /// than of the order a game happened to register them in.
    entries: BTreeMap<String, Entry>,
}

/// One registered component, reduced to the calls a tool makes.
///
/// Function pointers rather than boxed closures: each is a generic function
/// monomorphised for the registered `T`, so an entry is `Copy`-sized and there is
/// no second place a type parameter could be spelled differently from the first.
#[derive(Clone, Copy)]
struct Entry {
    /// The component's Rust path, for a message and for a test that wants to say
    /// *which* type a name resolved to.
    component: &'static str,
    codec: fn(&str) -> Box<dyn SystemChunk>,
    register: fn(&mut World, &str),
    component_mut: for<'w> fn(&'w mut World, &str, Entity) -> Option<&'w mut dyn Reflect>,
    placement: fn(&mut World, &str, Entity) -> Option<(DVec3, DVec3)>,
    entities: fn(&mut World, &str) -> Vec<Entity>,
}

impl Registry {
    /// A registry holding nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `T` as the component of the scene system called `system`.
    ///
    /// The one call that produces the codec, the system registration, the
    /// `&mut dyn Reflect` accessor and the [`Placement`] — see the
    /// [module docs](self) for why they are one call and not four.
    ///
    /// # Panics
    ///
    /// If `system` is already registered, naming it and both component types. A
    /// registry is built once from a literal list, so two entries under one name
    /// is a mistake in that list: the manifest joins on the name, and whichever
    /// of the two lost would be a component silently never reached.
    pub fn register<T>(&mut self, system: impl Into<String>)
    where
        T: Serialize + DeserializeOwned + ComponentHash + Reflect + Placement + 'static,
    {
        let system = system.into();
        let entry = Entry {
            component: type_name::<T>(),
            codec: codec_of::<T>,
            register: register_of::<T>,
            component_mut: component_of::<T>,
            placement: placement_of::<T>,
            entities: entities_of::<T>,
        };
        if let Some(held) = self.entries.get(&system) {
            panic!(
                "the scene system `{system}` is already registered, holding `{}`; \
                 registering `{}` under it would hide one of them",
                held.component, entry.component,
            );
        }
        self.entries.insert(system, entry);
    }

    /// The system names this registry knows, in name order.
    pub fn systems(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Whether `system` is registered — which is whether a scene naming it can
    /// be opened at all.
    #[must_use]
    pub fn contains(&self, system: &str) -> bool {
        self.entries.contains_key(system)
    }

    /// The Rust path of the component `system`'s rows are of, or [`None`] for a
    /// name this registry does not know.
    ///
    /// What makes "the registry resolved this chunk to a type" a claim a test can
    /// read rather than an inference from a load that happened to work.
    #[must_use]
    pub fn component_type(&self, system: &str) -> Option<&'static str> {
        self.entries.get(system).map(|entry| entry.component)
    }

    /// How many components are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The codecs [`Scene::load`](crcbl_scene::scn::Scene::load) and
    /// [`Scene::save`](crcbl_scene::scn::Scene::save) read and write this
    /// vocabulary's chunk files with.
    ///
    /// Freshly built rather than held: [`SystemChunk`] is a trait object and a
    /// scene wants a slice of them, so the registry stores the constructor and
    /// the list is the caller's to own. A load or a save is one call and the
    /// allocation is a handful of pointers.
    #[must_use]
    pub fn codecs(&self) -> Vec<Box<dyn SystemChunk>> {
        self.entries
            .iter()
            .map(|(system, entry)| (entry.codec)(system))
            .collect()
    }

    /// Registers one [`System<T>`](crcbl_ecs::System) per registered component,
    /// under the name its chunk file is spelled with.
    ///
    /// The other half of [`codecs`](Self::codecs), and from the same entries — a
    /// world built here is one every one of those codecs can read into.
    pub fn register_systems(&self, world: &mut World) {
        for (system, entry) in &self.entries {
            (entry.register)(world, system);
        }
    }

    /// `entity`'s component, as the rows an inspector draws and an edit is
    /// applied to.
    ///
    /// [`None`] for an entity no registered system holds — an id from another
    /// scene, or one this vocabulary has nothing for.
    ///
    /// `&mut World` although a read would do:
    /// [`crcbl_ecs::SystemTrait`] exposes `as_any_mut` and no shared `as_any`,
    /// so the only way to reach a `System<T>` by name is through a unique borrow.
    /// [`SystemChunk::write`] carries the same note for the same reason.
    ///
    /// # The first system that holds it
    ///
    /// Systems are tried in name order and the first hit wins. A scene cannot put
    /// one entity in two chunks — each row spawns its own entity, so the same id
    /// in two files is a duplicate-id error — so "the first" and "the only" are
    /// the same entity today, and the day the format changes
    /// (`docs/plan/08-editor.md`'s missing piece 6) this becomes a choice that has
    /// to be made rather than one that is made here.
    pub fn component<'w>(
        &self,
        world: &'w mut World,
        entity: Entity,
    ) -> Option<&'w mut dyn Reflect> {
        // Resolved to one entry first and borrowed once, rather than looped over
        // with the borrow live: a `&'w mut World` handed to a call inside the
        // loop cannot be handed to the next iteration.
        let (system, entry) = self.holder(world, entity)?;
        (entry.component_mut)(world, &system, entity)
    }

    /// Where `entity` stands and how far it reaches, from the [`Placement`] of
    /// whichever registered component it has.
    ///
    /// [`None`] both for an entity no registered system holds and for one whose
    /// component is not a thing in space — see [`Placement`].
    #[must_use]
    pub fn placement(&self, world: &mut World, entity: Entity) -> Option<(DVec3, DVec3)> {
        for (system, entry) in &self.entries {
            if let Some(placement) = (entry.placement)(world, system, entity) {
                return Some(placement);
            }
        }
        None
    }

    /// The entities the system called `system` holds, in the order the chunk file
    /// spells them.
    ///
    /// Empty for a name this registry does not know — which a loaded scene cannot
    /// contain, because [`Scene::load`](crcbl_scene::scn::Scene::load) refuses a
    /// manifest entry with no codec, and which is still the honest answer rather
    /// than a panic.
    ///
    /// The sort is what makes the order file order:
    /// [`IdMap`] is keyed by
    /// [`SceneEntityId`](crcbl_scene::scn::SceneEntityId), while
    /// `System::iter_entities` yields storage order, which swap-remove makes a
    /// function of attach history.
    #[must_use]
    pub fn entities(&self, world: &mut World, ids: &IdMap, system: &str) -> Vec<Entity> {
        let Some(entry) = self.entries.get(system) else {
            return Vec::new();
        };
        let mut rows: Vec<_> = (entry.entities)(world, system)
            .into_iter()
            .filter_map(|entity| ids.id(entity).map(|id| (id, entity)))
            .collect();
        rows.sort_by_key(|(id, _)| *id);
        rows.into_iter().map(|(_, entity)| entity).collect()
    }

    /// The name and entry of the first registered system holding `entity`.
    ///
    /// The name is cloned because the entries are borrowed from `self` while the
    /// caller still needs `&mut World`, and an entry is a handful of function
    /// pointers.
    fn holder(&self, world: &mut World, entity: Entity) -> Option<(String, Entry)> {
        self.entries.iter().find_map(|(system, entry)| {
            (entry.component_mut)(world, system, entity).map(|_| (system.clone(), *entry))
        })
    }
}

impl fmt::Debug for Registry {
    /// The vocabulary, as a name-to-type map — which is the whole of what a
    /// registry is, and what a log line reporting "this tool can open these"
    /// wants to print.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(
                self.entries
                    .iter()
                    .map(|(system, entry)| (system, entry.component)),
            )
            .finish()
    }
}

// ---------------------------------------------------------------------------
// The monomorphised halves
// ---------------------------------------------------------------------------

/// The `System<T>` in `world`'s schedule that is called `name`.
///
/// The name is the query and not the type, for
/// [`crcbl_scene::scn`]'s reason: [`World::system_mut`](crcbl_ecs::World::system_mut)
/// finds the first system of a type, so two systems of one component would
/// answer with the same array twice.
fn system_named<'w, T>(world: &'w mut World, name: &str) -> Option<&'w mut System<T>>
where
    T: ComponentHash + 'static,
{
    world
        .schedule_mut()
        .iter_mut()
        .find(|system| system.name() == name)
        .and_then(|system| system.as_any_mut().downcast_mut::<System<T>>())
}

/// [`Entry::codec`] for `T`.
fn codec_of<T>(name: &str) -> Box<dyn SystemChunk>
where
    T: Serialize + DeserializeOwned + ComponentHash + 'static,
{
    chunk_of::<T>(name)
}

/// [`Entry::register`] for `T`.
fn register_of<T>(world: &mut World, name: &str)
where
    T: ComponentHash + 'static,
{
    world.register_system(Box::new(System::<T>::new(name)));
}

/// [`Entry::component_mut`] for `T`.
fn component_of<'w, T>(
    world: &'w mut World,
    name: &str,
    entity: Entity,
) -> Option<&'w mut dyn Reflect>
where
    T: ComponentHash + Reflect + 'static,
{
    let component = system_named::<T>(world, name)?.get_mut(entity)?;
    Some(component)
}

/// [`Entry::placement`] for `T`.
fn placement_of<T>(world: &mut World, name: &str, entity: Entity) -> Option<(DVec3, DVec3)>
where
    T: ComponentHash + Placement + 'static,
{
    system_named::<T>(world, name)?.get(entity)?.placement()
}

/// [`Entry::entities`] for `T`, in storage order — [`Registry::entities`] is what
/// puts them in file order.
fn entities_of<T>(world: &mut World, name: &str) -> Vec<Entity>
where
    T: ComponentHash + 'static,
{
    system_named::<T>(world, name)
        .map(|system| system.iter_entities().map(|(entity, _)| entity).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::hash::Hasher;

    use crcbl_assets::MemorySource;
    use crcbl_reflect::{Value, get_path};
    use crcbl_scene::scn::{Scene, ScnError};
    use serde::Deserialize;

    use super::*;

    /// A component in the shape a real one has: a position, an extent, and a
    /// placement built from both.
    #[derive(Clone, Copy, Debug, PartialEq, Reflect, Serialize, Deserialize)]
    #[reflect(crate = "crcbl_reflect")]
    struct Block {
        position: [f64; 3],
        half_extents: [f64; 3],
    }

    impl ComponentHash for Block {
        fn hash_component(&self, hasher: &mut dyn Hasher) {
            for value in self.position.iter().chain(&self.half_extents) {
                hasher.write(&value.to_bits().to_le_bytes());
            }
        }
    }

    impl Placement for Block {
        fn placement(&self) -> Option<(DVec3, DVec3)> {
            Some((
                DVec3::from_array(self.position),
                DVec3::from_array(self.half_extents),
            ))
        }
    }

    /// A second vocabulary, and one that is **not** a thing in space — the
    /// `apps/puppet` `Sun` shape, so the tests below are about two component
    /// types rather than one registered twice.
    #[derive(Clone, Copy, Debug, PartialEq, Reflect, Serialize, Deserialize)]
    #[reflect(crate = "crcbl_reflect")]
    struct Beacon {
        intensity: f32,
    }

    impl ComponentHash for Beacon {
        fn hash_component(&self, hasher: &mut dyn Hasher) {
            hasher.write(&self.intensity.to_bits().to_le_bytes());
        }
    }

    impl Placement for Beacon {
        fn placement(&self) -> Option<(DVec3, DVec3)> {
            None
        }
    }

    /// Both components, under the names the scene below spells.
    fn registry() -> Registry {
        let mut registry = Registry::new();
        registry.register::<Block>("blocks");
        registry.register::<Beacon>("beacons");
        registry
    }

    /// A two-system scene, as text, keyed the way a source rooted at the scene
    /// directory reads it.
    fn scene_source() -> MemorySource {
        let mut source = MemorySource::new();
        for (key, text) in [
            (
                "scene.ron",
                "(format: 0, name: \"two\", systems: [\"blocks\", \"beacons\"])",
            ),
            (
                "env.ron",
                "(camera: (position: (0.0, 0.0, 8.0), look_at: (0.0, 0.0, 0.0)), \
                 ambient: (0.1, 0.1, 0.1))",
            ),
            (
                "sys/blocks.ron",
                "(system: \"blocks\", entities: [\
                 (0, (position: (1.0, 2.0, 3.0), half_extents: (0.5, 0.25, 0.5))), \
                 (1, (position: (4.0, 0.0, 0.0), half_extents: (1.0, 1.0, 1.0)))])",
            ),
            (
                "sys/beacons.ron",
                "(system: \"beacons\", entities: [(2, (intensity: 3.0))])",
            ),
        ] {
            source
                .insert(std::path::Path::new(key), text.as_bytes().to_vec())
                .expect("a scene key is a legal asset key");
        }
        source
    }

    /// **A chunk resolves to the type its rows are of**, which is the whole
    /// question a tool asks the registry — and it is asked by name, because the
    /// name is what a manifest carries.
    #[test]
    fn the_registry_resolves_a_system_name_to_the_component_it_holds() {
        let registry = registry();
        assert_eq!(registry.len(), 2);
        assert!(registry.contains("blocks"));
        assert!(!registry.contains("bricks"));
        assert!(
            registry
                .component_type("blocks")
                .expect("blocks is registered")
                .ends_with("Block"),
            "{:?}",
            registry.component_type("blocks"),
        );
        assert!(
            registry
                .component_type("beacons")
                .expect("beacons is registered")
                .ends_with("Beacon"),
        );
        assert_eq!(registry.component_type("bricks"), None);
    }

    /// **A scene of two systems loads through the registry alone**, and every row
    /// of both arrives — the claim a registry with one entry could not make.
    #[test]
    fn a_scene_of_two_systems_loads_through_the_registry() {
        let registry = registry();
        let mut world = World::new();
        registry.register_systems(&mut world);
        let (scene, ids) = Scene::load(
            &scene_source(),
            std::path::Path::new(""),
            &registry.codecs(),
            &mut world,
        )
        .expect("two registered systems is a scene this registry opens");

        assert_eq!(scene.systems(), ["blocks", "beacons"]);
        assert_eq!(ids.len(), 3, "two blocks and a beacon");
        assert_eq!(registry.entities(&mut world, &ids, "blocks").len(), 2);
        assert_eq!(registry.entities(&mut world, &ids, "beacons").len(), 1);
        assert!(
            registry.entities(&mut world, &ids, "bricks").is_empty(),
            "a name this registry does not know holds nothing",
        );
    }

    /// **An unregistered system fails loudly, naming itself.** The failure this
    /// module exists to remove is the other one: a load that quietly skipped the
    /// chunk and handed back a scene with a third of its entities missing.
    #[test]
    fn a_scene_naming_an_unregistered_system_is_refused_by_name() {
        let mut registry = Registry::new();
        registry.register::<Block>("blocks");
        let mut world = World::new();
        registry.register_systems(&mut world);

        let error = Scene::load(
            &scene_source(),
            std::path::Path::new(""),
            &registry.codecs(),
            &mut world,
        )
        .expect_err("`beacons` is not registered");
        assert!(
            matches!(&error, ScnError::NoCodec { system } if system == "beacons"),
            "{error}",
        );
        assert!(
            error.to_string().contains("beacons"),
            "the refusal does not name the system: {error}",
        );
    }

    /// **The codec list and the registered systems are the same list**, which is
    /// what makes "registered for the scene but not for the tool" impossible
    /// rather than merely unlikely.
    #[test]
    fn every_codec_has_a_system_of_the_same_name_and_the_reverse() {
        let registry = registry();
        let mut world = World::new();
        registry.register_systems(&mut world);

        let codecs: Vec<String> = registry
            .codecs()
            .iter()
            .map(|codec| codec.name().to_owned())
            .collect();
        let systems: Vec<String> = world
            .schedule_mut()
            .iter_mut()
            .map(|system| system.name().to_owned())
            .collect();
        assert_eq!(codecs, vec!["beacons".to_owned(), "blocks".to_owned()]);
        assert_eq!(systems, codecs);
    }

    /// The `&mut dyn Reflect` a registry hands back is **that entity's**
    /// component, reachable by the same path an edit command carries — and
    /// writing through it changes that row and no other.
    #[test]
    fn the_component_accessor_reads_and_writes_the_entity_it_names() {
        let registry = registry();
        let mut world = World::new();
        registry.register_systems(&mut world);
        let (_, ids) = Scene::load(
            &scene_source(),
            std::path::Path::new(""),
            &registry.codecs(),
            &mut world,
        )
        .expect("the scene loads");

        let first = ids
            .entity(crcbl_scene::scn::SceneEntityId(0))
            .expect("id 0");
        let second = ids
            .entity(crcbl_scene::scn::SceneEntityId(1))
            .expect("id 1");

        let component = registry
            .component(&mut world, first)
            .expect("a block is a registered component");
        assert_eq!(get_path(component, "position.1"), Ok(Value::Float(2.0)));
        crcbl_reflect::set_path(component, "position.1", &Value::Float(9.0))
            .expect("a block has a y");

        assert_eq!(
            registry
                .placement(&mut world, first)
                .expect("a block is a thing in space")
                .0,
            DVec3::new(1.0, 9.0, 3.0),
            "the write did not reach the component the placement reads",
        );
        assert_eq!(
            registry
                .placement(&mut world, second)
                .expect("so is its neighbour")
                .0,
            DVec3::new(4.0, 0.0, 0.0),
            "editing one row moved another",
        );
    }

    /// A component that is not a thing in space has **no** placement, and that is
    /// different from being unregistered: the accessor still reaches it.
    #[test]
    fn a_component_that_is_not_in_space_has_a_reflect_row_and_no_placement() {
        let registry = registry();
        let mut world = World::new();
        registry.register_systems(&mut world);
        let (_, ids) = Scene::load(
            &scene_source(),
            std::path::Path::new(""),
            &registry.codecs(),
            &mut world,
        )
        .expect("the scene loads");
        let beacon = ids
            .entity(crcbl_scene::scn::SceneEntityId(2))
            .expect("id 2");

        assert!(
            registry.component(&mut world, beacon).is_some(),
            "a beacon is editable",
        );
        assert_eq!(registry.placement(&mut world, beacon), None);
    }

    /// An entity no registered system holds answers [`None`] to both halves,
    /// rather than panicking or reaching some other entity's row.
    #[test]
    fn an_entity_with_no_registered_component_has_neither_half() {
        let registry = registry();
        let mut world = World::new();
        registry.register_systems(&mut world);
        let stranger = world.spawn();

        assert!(registry.component(&mut world, stranger).is_none());
        assert_eq!(registry.placement(&mut world, stranger), None);
    }

    /// An empty registry opens nothing and says so, rather than opening a scene
    /// with no entities in it.
    #[test]
    fn an_empty_registry_holds_nothing_and_refuses_every_scene() {
        let registry = Registry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.codecs().is_empty());

        let mut world = World::new();
        registry.register_systems(&mut world);
        assert_eq!(world.schedule().len(), 0);
        assert!(
            Scene::load(
                &scene_source(),
                std::path::Path::new(""),
                &registry.codecs(),
                &mut world,
            )
            .is_err(),
        );
    }

    /// Registering two components under one name is a panic that names the
    /// system and both types, rather than a silent replacement of one by the
    /// other.
    #[test]
    #[should_panic(expected = "is already registered")]
    fn two_components_under_one_system_name_is_refused() {
        let mut registry = Registry::new();
        registry.register::<Block>("blocks");
        registry.register::<Beacon>("blocks");
    }

    /// The `Debug` form is the vocabulary: every name with the type it resolves
    /// to, which is what a tool logs when it says what it can open.
    #[test]
    fn the_debug_form_names_every_system_and_its_component() {
        let text = format!("{:?}", registry());
        assert!(text.contains("blocks"), "{text}");
        assert!(text.contains("beacons"), "{text}");
        assert!(text.contains("Block"), "{text}");
        assert!(text.contains("Beacon"), "{text}");
    }
}
