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
mod tests {
    use std::hash::Hasher;

    use crcbl_assets::MemorySource;
    use crcbl_reflect::{Value, get_path};
    use crcbl_scene::scn::{Scene, SceneEntityId, ScnError};
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

    /// **An entity resolves to the system holding it, and that system's codec
    /// reads its row** — what a delete records so its undo can rebuild the
    /// entity.
    #[test]
    fn an_entity_resolves_to_its_system_and_that_systems_row() {
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

        let block = ids.entity(SceneEntityId(1)).expect("the second block");
        let beacon = ids.entity(SceneEntityId(2)).expect("the beacon");
        assert_eq!(
            registry.system_of(&mut world, block).as_deref(),
            Some("blocks")
        );
        assert_eq!(
            registry.system_of(&mut world, beacon).as_deref(),
            Some("beacons")
        );
        let stranger = world.spawn();
        assert_eq!(registry.system_of(&mut world, stranger), None);

        let row = registry
            .codec("beacons")
            .expect("beacons is registered")
            .row(&mut world, beacon)
            .expect("the system is in the world")
            .expect("it holds the beacon");
        assert_eq!(row, "(intensity:3.0)");
        assert!(registry.codec("bricks").is_none());
    }

    /// **A game's check runs on a scene that lists its system and on no other**,
    /// and says what it refuses.
    #[test]
    fn a_check_runs_only_on_scenes_listing_its_system() {
        fn refuse_beacons(_: &dyn AssetSource, _: &Path) -> Result<(), String> {
            Err("a beacon needs a block to stand on".to_owned())
        }
        fn refuse_bricks(_: &dyn AssetSource, _: &Path) -> Result<(), String> {
            Err("bricks were checked".to_owned())
        }
        fn pass(_: &dyn AssetSource, _: &Path) -> Result<(), String> {
            Ok(())
        }
        let mut registry = registry();
        registry.check("beacons", refuse_beacons);
        registry.check("bricks", refuse_bricks);
        registry.check("blocks", pass);

        let source = scene_source();
        let systems = ["blocks".to_owned(), "beacons".to_owned()];
        assert_eq!(
            registry.problems(&systems, &source, std::path::Path::new("")),
            ["a beacon needs a block to stand on"],
        );
        assert!(
            registry
                .problems(&["blocks".to_owned()], &source, std::path::Path::new(""))
                .is_empty(),
            "a check ran on a scene that does not list its system",
        );
    }

    /// **A module is built for a scene that lists its system and for no
    /// other**, in the order the modules were added — and every call builds
    /// new instances, so what one play left in a module is not where the next
    /// starts.
    #[test]
    fn modules_are_built_fresh_for_scenes_listing_their_system() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// How many modules the factories below have built.
        static BUILT: AtomicUsize = AtomicUsize::new(0);

        struct Named(&'static str);

        impl GameModule for Named {
            fn name(&self) -> &str {
                self.0
            }
            fn register(&self, _world: &mut World) {}
        }

        fn names(modules: &[Box<dyn GameModule>]) -> Vec<&str> {
            modules.iter().map(|module| module.name()).collect()
        }

        let mut registry = registry();
        registry.module("beacons", |_, _| {
            BUILT.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(Named("first")))
        });
        registry.module("bricks", |_, _| {
            BUILT.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(Named("elsewhere")))
        });
        registry.module("blocks", |_, _| {
            BUILT.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(Named("second")))
        });

        let source = scene_source();
        let build = |systems: &[String]| {
            registry
                .modules(systems, &source, Path::new(""))
                .expect("no factory here refuses")
        };
        let systems = ["blocks".to_owned(), "beacons".to_owned()];
        let first = build(&systems);
        assert_eq!(names(&first), ["first", "second"]);
        assert_eq!(
            BUILT.load(Ordering::Relaxed),
            2,
            "a module nothing listed was built"
        );
        let again = build(&systems);
        assert_eq!(names(&again), ["first", "second"]);
        assert_eq!(
            BUILT.load(Ordering::Relaxed),
            4,
            "a second call handed back the modules it built before",
        );
        assert!(build(&[]).is_empty());
    }

    /// **A factory reads the scene it is handed and may refuse it**, and the
    /// refusal is what `modules` answers — no module is handed back beside it.
    #[test]
    fn a_factory_that_refuses_the_scene_refuses_the_play() {
        struct Rules;

        impl GameModule for Rules {
            fn name(&self) -> &str {
                "rules"
            }
            fn register(&self, _world: &mut World) {}
        }

        /// Plays a scene with a header, and refuses one without.
        fn start(source: &dyn AssetSource, dir: &Path) -> Result<Box<dyn GameModule>, String> {
            source
                .read(&dir.join("scene.ron"))
                .map_err(|error| format!("no header: {error}"))?;
            Ok(Box::new(Rules))
        }

        let mut registry = registry();
        registry.module("blocks", start);
        let systems = ["blocks".to_owned()];

        let played = registry
            .modules(&systems, &scene_source(), Path::new(""))
            .expect("the scene has a header");
        assert_eq!(played.len(), 1);
        let refused = registry
            .modules(&systems, &MemorySource::new(), Path::new(""))
            .err()
            .expect("an empty source has no header");
        assert!(refused.starts_with("no header"), "{refused}");
    }

    /// A runtime component: where a thing a module spawned stands.
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Ball {
        centre: [f64; 3],
    }

    impl ComponentHash for Ball {
        fn hash_component(&self, hasher: &mut dyn Hasher) {
            for value in self.centre {
                hasher.write(&value.to_bits().to_le_bytes());
            }
        }
    }

    impl Placement for Ball {
        fn placement(&self) -> Option<(DVec3, DVec3)> {
            Some((DVec3::from_array(self.centre), DVec3::splat(0.25)))
        }
    }

    /// **A runtime component is placed and listed for drawing, and it is no
    /// part of the scene vocabulary**: not a system a scene can name, no
    /// codec, no editable row, no system a load registers.
    #[test]
    fn a_runtime_component_is_placed_and_never_part_of_the_scene() {
        let mut registry = registry();
        registry.register_runtime::<Ball>("balls");

        assert_eq!(
            registry.systems().collect::<Vec<_>>(),
            ["beacons", "blocks"]
        );
        assert!(!registry.contains("balls"));
        assert!(registry.codec("balls").is_none());
        assert_eq!(registry.codecs().len(), 2);
        assert_eq!(registry.component_type("balls"), None);

        let mut world = World::new();
        registry.register_systems(&mut world);
        assert_eq!(
            world.schedule().len(),
            2,
            "a load registered the runtime system"
        );
        assert!(
            registry.runtime_entities(&mut world).is_empty(),
            "a world nothing plays has runtime entities",
        );

        // What the module that owns it does in its own `register` and `tick`.
        let mut balls = System::<Ball>::new("balls");
        let ball = world.spawn();
        balls.attach(
            ball,
            Ball {
                centre: [1.0, 2.0, 3.0],
            },
        );
        world.register_system(Box::new(balls));

        assert_eq!(registry.runtime_entities(&mut world), [ball]);
        assert_eq!(
            registry.placement(&mut world, ball),
            Some((DVec3::new(1.0, 2.0, 3.0), DVec3::splat(0.25))),
        );
        assert!(
            registry.component(&mut world, ball).is_none(),
            "a ball is editable"
        );
        assert_eq!(registry.system_of(&mut world, ball), None);
    }

    /// **A scene naming a runtime system is refused by name**, because there
    /// is no codec to read it with — the half of "never saved" a load holds.
    #[test]
    fn a_scene_naming_a_runtime_system_is_refused_by_name() {
        let mut registry = Registry::new();
        registry.register::<Block>("blocks");
        registry.register_runtime::<Beacon>("beacons");
        let mut world = World::new();
        registry.register_systems(&mut world);

        let error = Scene::load(
            &scene_source(),
            std::path::Path::new(""),
            &registry.codecs(),
            &mut world,
        )
        .expect_err("`beacons` is runtime only");
        assert!(
            matches!(&error, ScnError::NoCodec { system } if system == "beacons"),
            "{error}",
        );
    }

    /// A runtime component under a scene system's name is refused like two
    /// scene components under one.
    #[test]
    #[should_panic(expected = "is already registered")]
    fn a_runtime_component_under_a_scene_systems_name_is_refused() {
        let mut registry = registry();
        registry.register_runtime::<Ball>("blocks");
    }

    /// …and the other way round.
    #[test]
    #[should_panic(expected = "is already registered")]
    fn a_scene_component_under_a_runtime_systems_name_is_refused() {
        let mut registry = Registry::new();
        registry.register_runtime::<Ball>("blocks");
        registry.register::<Block>("blocks");
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
