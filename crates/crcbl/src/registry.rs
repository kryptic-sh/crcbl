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
//!
//!     a game ──▶ Registry::module("bricks", start)
//!                     │
//!                     └── modules()         a fresh GameModule per play, built
//!                                           from the scene's files, for a tool
//!                                           that runs the scene
//!
//!     a game ──▶ Registry::register_runtime::<Ball>("balls")
//!                     │
//!                     ├── placement()       where a thing its module spawned
//!                     └── runtime_entities()  stands, for a tool to draw it
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
//! * **Not a loop.** [`Registry::module`] records how to build a game's
//!   [`GameModule`] and [`Registry::modules`] builds them; registering their
//!   systems and ticking them is the caller's — the editor's play mode is one,
//!   and it decides the rate and the inputs.
//!
//! # A runtime component is placed and never saved
//!
//! What a module spawns while a scene plays — a creep walking the lane — is
//! not part of the scene, and a tool still wants to draw it.
//! [`Registry::register_runtime`] records **only** the placement: no codec, so
//! a manifest naming the system is refused by
//! [`Scene::load`](crcbl_scene::scn::Scene::load) and
//! [`Scene::save`](crcbl_scene::scn::Scene::save) has nothing to write it
//! with; no `&mut dyn Reflect`, so nothing edits it; and no system
//! registration, because the module that spawns into it registers its own.
//! The guarantee is the missing codec rather than a flag a save has to
//! remember to read, so a runtime system cannot leak into a scene's files.

use std::any::type_name;
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use glam::DVec3;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crcbl_assets::AssetSource;
use crcbl_ecs::{ComponentHash, Entity, GameModule, System, World};
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
    /// The games' rules over whole scenes, each with the system whose presence
    /// in a manifest says the rule applies — see [`Registry::check`].
    checks: Vec<(String, SceneCheck)>,
    /// The games' behaviour, each with the system whose presence in a manifest
    /// says the scene is that game's — see [`Registry::module`].
    modules: Vec<(String, ModuleFactory)>,
    /// The components a game's module spawns while a scene plays, keyed and
    /// sorted by system name as `entries` is — see
    /// [`Registry::register_runtime`].
    runtime: BTreeMap<String, RuntimeEntry>,
}

/// A game's rule over a whole scene: the scene's files, read through `source`
/// under the directory `dir`, and the reason the game would refuse them — or
/// `Ok` for a scene it would play.
///
/// What the format cannot say and a game can: a path whose legs must be
/// axis-aligned, a plot that must stand clear of the lane. A tool that saves a
/// scene runs these so that a layout the game will refuse is reported where it
/// was made rather than where it is next loaded.
pub type SceneCheck = fn(&dyn AssetSource, &Path) -> Result<(), String>;

/// Builds a fresh instance of a game's [`GameModule`] for the scene whose files
/// `source` holds under `dir` — or the reason the game refuses to play that
/// scene. What a tool that plays a scene calls each time it starts playing.
///
/// A constructor rather than an instance, because a module carries state from
/// tick to tick — a score, a wave counter — and a second play has to start from
/// none of it. A function pointer for [`SceneCheck`]'s reason: a capture-free
/// closure coerces to one, and the registry stays a table of plain pointers.
///
/// **Handed the scene's files, and allowed to refuse them**, because a game
/// reads its rules off the scene before the first tick — a path whose legs must
/// be axis-aligned is a path a module cannot walk otherwise — and a refusal
/// there is a reason a person can act on, where a module built regardless would
/// have to panic or sit inert. The files rather than the world, for
/// [`SceneCheck`]'s reason: they are what the game's own loader reads, so the
/// module plays the scene the way the game would load it.
pub type ModuleFactory = fn(&dyn AssetSource, &Path) -> Result<Box<dyn GameModule>, String>;

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

/// One runtime component, reduced to the two calls a tool drawing it makes —
/// see [`Registry::register_runtime`].
#[derive(Clone, Copy)]
struct RuntimeEntry {
    /// The component's Rust path, for a message.
    component: &'static str,
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
        self.refuse_taken(&system, entry.component);
        self.entries.insert(system, entry);
    }

    /// Registers `T` as a **runtime** component: one a game's module spawns
    /// into the system called `system` while a scene plays, which a tool draws
    /// from its [`Placement`] and never lists, edits or saves.
    ///
    /// Only the placement and the entity list are recorded — see the
    /// [module docs](self) for why the missing codec is the guarantee. So the
    /// name is not one of [`systems`](Self::systems),
    /// [`contains`](Self::contains) says a scene naming it cannot be opened,
    /// and [`register_systems`](Self::register_systems) leaves it out: the
    /// module registers the system in its own
    /// [`register`](crcbl_ecs::GameModule::register), so it exists only in a
    /// world that is playing.
    ///
    /// # Panics
    ///
    /// If `system` is already registered, as a scene system or a runtime one,
    /// for [`register`](Self::register)'s reason: [`placement`](Self::placement)
    /// finds a system by name, and two under one name would answer for each
    /// other.
    pub fn register_runtime<T>(&mut self, system: impl Into<String>)
    where
        T: ComponentHash + Placement + 'static,
    {
        let system = system.into();
        let entry = RuntimeEntry {
            component: type_name::<T>(),
            placement: placement_of::<T>,
            entities: entities_of::<T>,
        };
        self.refuse_taken(&system, entry.component);
        self.runtime.insert(system, entry);
    }

    /// Panics, naming both types, if `system` is already registered — the one
    /// refusal [`register`](Self::register) and
    /// [`register_runtime`](Self::register_runtime) share.
    fn refuse_taken(&self, system: &str, component: &str) {
        let held = self
            .entries
            .get(system)
            .map(|entry| entry.component)
            .or_else(|| self.runtime.get(system).map(|entry| entry.component));
        if let Some(held) = held {
            panic!(
                "the scene system `{system}` is already registered, holding `{held}`; \
                 registering `{component}` under it would hide one of them",
            );
        }
    }

    /// Adds `check`, run on every scene whose manifest lists `system` — the
    /// game that owns that system holding the scene to its own rules.
    ///
    /// Keyed by a system rather than run on every scene, because a registry
    /// holds several games' vocabularies at once and one game's rules are
    /// nothing to another's scene.
    pub fn check(&mut self, system: impl Into<String>, check: SceneCheck) {
        self.checks.push((system.into(), check));
    }

    /// What every check whose system `systems` lists refuses in the scene
    /// `source` holds under `dir`, in the order the checks were added — empty
    /// for a scene every applicable game would play.
    #[must_use]
    pub fn problems(
        &self,
        systems: &[String],
        source: &dyn AssetSource,
        dir: &Path,
    ) -> Vec<String> {
        self.checks
            .iter()
            .filter(|(system, _)| systems.contains(system))
            .filter_map(|(_, check)| check(source, dir).err())
            .collect()
    }

    /// Adds `factory`, whose module plays any scene whose manifest lists
    /// `system` — the game that owns that system bringing its behaviour beside
    /// its components. The factory is handed the scene's files, and may refuse
    /// them — see [`ModuleFactory`].
    ///
    /// Keyed by a system for [`check`](Self::check)'s reason: a registry holds
    /// several games' vocabularies at once, and one game's rules ticking on
    /// another's scene would move things that game never meant to move. So a
    /// game registers its module **once**, under the system whose presence says
    /// the scene is that game's; registered under two of its systems, it would
    /// be built and ticked twice.
    ///
    /// ```
    /// use std::path::Path;
    ///
    /// use crcbl::assets::{AssetSource, MemorySource};
    /// use crcbl::ecs::{ClientInputs, GameModule, World};
    /// use crcbl::registry::Registry;
    ///
    /// struct Rules;
    ///
    /// impl GameModule for Rules {
    ///     fn name(&self) -> &str {
    ///         "rules"
    ///     }
    ///     fn register(&self, _world: &mut World) {}
    ///     fn tick(&mut self, _world: &mut World, _inputs: ClientInputs<'_>) {}
    /// }
    ///
    /// fn start(_: &dyn AssetSource, _: &Path) -> Result<Box<dyn GameModule>, String> {
    ///     Ok(Box::new(Rules))
    /// }
    ///
    /// let mut registry = Registry::new();
    /// registry.module("bricks", start);
    ///
    /// let scene = MemorySource::new();
    /// let built = registry
    ///     .modules(&["bricks".to_owned()], &scene, Path::new(""))
    ///     .expect("the rules play any scene");
    /// assert_eq!(built.len(), 1);
    /// assert_eq!(built[0].name(), "rules");
    /// let none = registry
    ///     .modules(&["props".to_owned()], &scene, Path::new(""))
    ///     .expect("no game to refuse it");
    /// assert!(none.is_empty());
    /// ```
    pub fn module(&mut self, system: impl Into<String>, factory: ModuleFactory) {
        self.modules.push((system.into(), factory));
    }

    /// A fresh instance of every module whose system `systems` lists, built
    /// for the scene `source` holds under `dir`, in the order the modules were
    /// added — empty for a scene no registered game plays.
    ///
    /// Registration order rather than name order, because it is the order the
    /// modules tick in, and a game that registers two wants them to run in the
    /// order it wrote.
    ///
    /// # Errors
    ///
    /// The first refusal a factory gave, in that order: a game that will not
    /// play the scene, saying why. No module is handed back then, so a caller
    /// has registered nothing it would have to take out of its world again.
    pub fn modules(
        &self,
        systems: &[String],
        source: &dyn AssetSource,
        dir: &Path,
    ) -> Result<Vec<Box<dyn GameModule>>, String> {
        self.modules
            .iter()
            .filter(|(system, _)| systems.contains(system))
            .map(|(_, factory)| factory(source, dir))
            .collect()
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

    /// The codec of the one system called `system`, or [`None`] for a name this
    /// registry does not know.
    ///
    /// What an edit that removes, copies or rebuilds one entity reads and writes
    /// its row through — [`SystemChunk::row`] and [`SystemChunk::attach_row`].
    #[must_use]
    pub fn codec(&self, system: &str) -> Option<Box<dyn SystemChunk>> {
        self.entries.get(system).map(|entry| (entry.codec)(system))
    }

    /// The name of the registered system holding `entity`, or [`None`] for an
    /// entity none of them holds — the first in name order, for
    /// [`component`](Self::component)'s reason.
    #[must_use]
    pub fn system_of(&self, world: &mut World, entity: Entity) -> Option<String> {
        self.holder(world, entity).map(|(system, _)| system)
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
    /// whichever registered component it has — a scene component's, or a
    /// [runtime](Self::register_runtime) one's.
    ///
    /// [`None`] both for an entity no registered system holds and for one whose
    /// component is not a thing in space — see [`Placement`].
    #[must_use]
    pub fn placement(&self, world: &mut World, entity: Entity) -> Option<(DVec3, DVec3)> {
        let scene = self
            .entries
            .iter()
            .map(|(system, entry)| (system, entry.placement));
        let runtime = self
            .runtime
            .iter()
            .map(|(system, entry)| (system, entry.placement));
        scene
            .chain(runtime)
            .find_map(|(system, placement)| placement(world, system, entity))
    }

    /// Every entity a [runtime](Self::register_runtime) system holds in
    /// `world` — what a module spawned while the scene plays — by system name
    /// and then in storage order.
    ///
    /// Empty in a world no module has registered a runtime system in, which is
    /// every world that is not playing. Storage order rather than an id's,
    /// because these entities have none: nothing saves them, and the order is
    /// only the order a tool draws them in.
    #[must_use]
    pub fn runtime_entities(&self, world: &mut World) -> Vec<Entity> {
        self.runtime
            .iter()
            .flat_map(|(system, entry)| (entry.entities)(world, system))
            .collect()
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
mod tests;
