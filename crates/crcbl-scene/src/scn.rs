//! The `.scn/` scene directory: a header, an environment, and one RON file per
//! system.
//!
//! Stage 6's scene rule (`docs/notes/tooling.md`: _A scene is a directory of
//! chunk files_) is the shape, and it is a directory rather than a document for
//! two reasons that fight inside any single file: a save rewrites one system's
//! chunk rather than the whole scene, and two people editing different systems
//! merge with no conflict.
//!
//! ```text
//! scenes/board.scn/
//!   scene.ron        # format version, name, the system manifest
//!   env.ron          # camera defaults and ambience
//!   sys/
//!     bricks.ron     # that system's entity array
//! ```
//!
//! [`Scene::load`] reads those keys through a [`crcbl_assets::AssetSource`], so
//! the same loader serves a directory ([`crcbl_assets::DirSource`]) and a
//! browser build with no filesystem ([`crcbl_assets::MemorySource`] seeded from
//! `include_str!`). [`Scene::save`] hands back the files it would write; nothing
//! here touches `std::fs`, which is stage 6's "no synchronous IO anywhere in
//! engine crates" exit criterion and is also what makes the writer testable
//! without a directory.
//!
//! # The file's id is not the runtime handle
//!
//! [`crcbl_core::Pool`] has no insert-at-index, so a [`crcbl_ecs::Entity`]'s
//! bits are a function of spawn history and not of the file. Persisting them
//! would either regenerate ids on save — banned by name in the plan's
//! deterministic-writer section — or break on a world that already holds
//! entities. So the file owns a [`SceneEntityId`], and [`IdMap`] is the
//! correspondence between the two for as long as the scene is loaded.
//!
//! # Registration, and why `crcbl-ecs` gains nothing
//!
//! A chunk file holds one system's component array, so reading it needs that
//! component's `serde` implementation — and putting that bound on
//! [`crcbl_ecs::SystemTrait`] would reach every system in the workspace and put
//! `serde` in the entity crate, whose dependency list is `crcbl-core` and
//! nothing else. The bound sits on [`chunk_of`] instead: the caller names the
//! systems it wants persisted, passes the resulting [`SystemChunk`] codecs to
//! [`Scene::load`], and a manifest entry with no codec is an error rather than a
//! skip.
//!
//! Reaching a system by *name* rather than by type is deliberate.
//! [`crcbl_ecs::World::system_mut`] finds the first system of a type, so two
//! `System<Prop>`s under different names collide; the manifest names files, so
//! the walk over [`crcbl_ecs::Schedule`] is the query that matches the format.
//!
//! # What is not here
//!
//! Each is owed in `docs/backlog.md`, under _What the deleted 06-assets-scenes
//! plan left unbuilt_ and the entries it names.
//!
//! - **Dirty-chunk tracking.** It exists to make an editor's save cheap, and
//!   [`Scene::save`] rewrites every chunk the manifest names.
//! - **Hot reload and the watcher** — stage 6's task 5.
//! - **`crcbl bake` and `PackSource`**, the single-blob shipping form — task 6.
//! - **Sidecar `.meta.ron` GUIDs.** Assets stay referenced by canonical path;
//!   a GUID arrives through `crcbl_assets::AssetId::from_bits`.
//! - **The editor's command journal and sector sharding** — P12, and stage 6's
//!   "Scaling" subsection.

use std::any::type_name;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::marker::PhantomData;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crcbl_assets::{AssetSource, StorageError};
use crcbl_ecs::{ComponentHash, Entity, System, World};

// ---------------------------------------------------------------------------
// Ids
// ---------------------------------------------------------------------------

/// An entity's identity **in the file**, dense and never reused.
///
/// Written as a bare number — the type is `#[serde(transparent)]`, so a chunk
/// row reads `(3, Brick(…))` rather than repeating the type name on every line.
///
/// It is not a [`crcbl_ecs::Entity`] and does not survive outside the scene it
/// was read from: see the [module docs](self).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SceneEntityId(pub u32);

impl fmt::Display for SceneEntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Which live entity each [`SceneEntityId`] became, and back again.
///
/// Produced by [`Scene::load`] and consumed by [`Scene::save`], which is why it
/// holds both directions: the loader spawns in file order and the writer has to
/// put an entity back under the id it arrived with.
#[derive(Debug, Default)]
pub struct IdMap {
    /// Sorted, because it is what the writer iterates: the plan's "entities
    /// sorted by stable ID" is this map's ordering and not a sort at save time.
    to_entity: BTreeMap<SceneEntityId, Entity>,
    to_id: HashMap<Entity, SceneEntityId>,
    /// One past the highest id ever handed out, so [`IdMap::assign`] never
    /// reuses one a chunk file already spells.
    next: u32,
}

impl IdMap {
    /// An empty map, holding no scene.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The entity `id` was spawned as, if this scene named it.
    #[must_use]
    pub fn entity(&self, id: SceneEntityId) -> Option<Entity> {
        self.to_entity.get(&id).copied()
    }

    /// The id `entity` is written back under, if it came from the file or was
    /// [`assign`](Self::assign)ed one.
    #[must_use]
    pub fn id(&self, entity: Entity) -> Option<SceneEntityId> {
        self.to_id.get(&entity).copied()
    }

    /// Gives `entity` an id so a later [`Scene::save`] can write it, and returns
    /// the one it already has if it has one.
    ///
    /// This is how an entity spawned after load enters the scene. Ids are handed
    /// out in call order rather than derived from the entity, so a caller that
    /// wants a stable file assigns them in a deterministic order — which is the
    /// same discipline the file itself encodes.
    ///
    /// # Panics
    ///
    /// If the scene has handed out every one of the 2^32 ids, which no caller
    /// reaches: an id is four bytes and a scene of that many entities does not
    /// fit in memory to be saved.
    pub fn assign(&mut self, entity: Entity) -> SceneEntityId {
        if let Some(id) = self.id(entity) {
            return id;
        }
        let id = SceneEntityId(self.next);
        self.next = self
            .next
            .checked_add(1)
            .expect("a scene holds fewer than 2^32 entities");
        self.to_entity.insert(id, entity);
        self.to_id.insert(entity, id);
        id
    }

    /// How many entities the scene named.
    #[must_use]
    pub fn len(&self) -> usize {
        self.to_entity.len()
    }

    /// Whether the scene named none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.to_entity.is_empty()
    }

    /// Records the file's own id for a freshly spawned entity.
    ///
    /// Refuses a second use of one id: two chunk files that both claim `7` would
    /// otherwise silently lose an entity out of the map, and it would come back
    /// as a save that dropped a row.
    fn bind(&mut self, key: &str, id: SceneEntityId, entity: Entity) -> Result<(), ScnError> {
        if self.to_entity.contains_key(&id) {
            return Err(ScnError::DuplicateId {
                key: key.to_string(),
                id,
            });
        }
        self.next = self.next.max(id.0.saturating_add(1));
        self.to_entity.insert(id, entity);
        self.to_id.insert(entity, id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The files
// ---------------------------------------------------------------------------

/// `scene.ron`: what the loader has to agree with before it reads anything else.
///
/// Private, and separate from [`Scene`], for [`crcbl_inventory`]'s reason: the
/// file shape is one type and the runtime type another. Here the difference is
/// `format`, which is checked and then has nothing left to say, and `env`, which
/// lives in its own file.
///
/// [`crcbl_inventory`]: https://docs.rs/crcbl-inventory
#[derive(Serialize, Deserialize)]
#[serde(rename = "Scene", deny_unknown_fields)]
struct SceneFile {
    format: u32,
    name: String,
    systems: Vec<String>,
}

/// `env.ron`: the camera the scene opens on and the light it sits in.
///
/// Deliberately small. Stage 6's scene layout called this file "camera
/// defaults, lighting, ambience", and a field nothing reads is a type nothing
/// fills — the renderer's own descriptions are `crcbl_render::scene`, and a
/// second copy of them here would be a second thing to keep in step.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename = "Env", deny_unknown_fields)]
pub struct Env {
    /// Where the scene is viewed from when it opens.
    pub camera: EnvCamera,
    /// Linear RGB the scene is lit by in the absence of any other light.
    pub ambient: [f32; 3],
}

/// The camera half of [`Env`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename = "Camera", deny_unknown_fields)]
pub struct EnvCamera {
    /// World-space eye position.
    pub position: [f32; 3],
    /// The world-space point the eye is aimed at.
    pub look_at: [f32; 3],
}

/// `sys/<name>.ron`: one system's entity array.
///
/// Generic over the component so that [`chunk_of`] is the only place a `serde`
/// bound appears. `system` is written as well as implied by the file name, and
/// the loader checks the two agree — a chunk moved or copied under another name
/// is a mistake worth a message rather than a silently mis-attached array.
#[derive(Serialize, Deserialize)]
#[serde(rename = "Chunk", deny_unknown_fields)]
struct ChunkFile<T> {
    system: String,
    entities: Vec<(SceneEntityId, T)>,
}

// ---------------------------------------------------------------------------
// Codecs
// ---------------------------------------------------------------------------

/// Reads and writes one system's chunk file.
///
/// Object-safe on purpose: a scene's codecs are a heterogeneous list — one
/// component type per entry — and [`chunk_of`] is how a caller builds one
/// without naming the implementing type.
pub trait SystemChunk: fmt::Debug {
    /// The system's name, which is the manifest entry and the file stem.
    fn name(&self) -> &str;

    /// Spawns `text`'s entities into `world` and records their ids in `ids`.
    ///
    /// `key` is the asset key the text came from, so a refusal can say which
    /// file it is about.
    ///
    /// # Errors
    ///
    /// [`ScnError`] if the text is not this system's chunk, if the world holds
    /// no system of the right name and component type, or if an id is claimed
    /// twice.
    fn read(
        &self,
        world: &mut World,
        ids: &mut IdMap,
        key: &str,
        text: &str,
    ) -> Result<(), ScnError>;

    /// The text of this system's chunk file, as [`Scene::save`] would write it.
    ///
    /// Takes `&mut World` although it changes nothing, and that is a fact about
    /// `crcbl-ecs` rather than about this trait: [`crcbl_ecs::SystemTrait`]
    /// exposes `as_any_mut` and no shared `as_any`, so the only way to reach a
    /// `System<T>` by name is [`crcbl_ecs::Schedule::iter_mut`].
    ///
    /// # Errors
    ///
    /// [`ScnError`] if the world holds no such system, if an entity in it was
    /// never given a [`SceneEntityId`], or if the component's own `Serialize`
    /// fails.
    fn write(&self, world: &mut World, ids: &IdMap) -> Result<String, ScnError>;
}

/// The codec for a `System<T>` registered under `name`.
///
/// `T` is the component the chunk file holds rows of; the three bounds are what
/// each half needs — `Serialize`/`DeserializeOwned` for the file,
/// [`ComponentHash`] and `'static` because that is what
/// `impl SystemTrait for System<T>` already requires and so what the schedule
/// can hold.
///
/// ```
/// use crcbl_ecs::{ComponentHash, System, World};
/// use crcbl_scene::scn::chunk_of;
/// use serde::{Deserialize, Serialize};
/// use std::hash::Hasher;
///
/// #[derive(Serialize, Deserialize)]
/// struct Prop {
///     position: [f32; 3],
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
/// let mut world = World::new();
/// world.register_system(Box::new(System::<Prop>::new("props")));
/// let chunks = vec![chunk_of::<Prop>("props")];
/// assert_eq!(chunks[0].name(), "props");
/// ```
#[must_use]
pub fn chunk_of<T>(name: impl Into<String>) -> Box<dyn SystemChunk>
where
    T: Serialize + DeserializeOwned + ComponentHash + 'static,
{
    Box::new(ChunkOf::<T> {
        name: name.into(),
        component: PhantomData,
    })
}

struct ChunkOf<T> {
    name: String,
    component: PhantomData<fn() -> T>,
}

impl<T> fmt::Debug for ChunkOf<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemChunk")
            .field("name", &self.name)
            .field("component", &type_name::<T>())
            .finish()
    }
}

impl<T> SystemChunk for ChunkOf<T>
where
    T: Serialize + DeserializeOwned + ComponentHash + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn read(
        &self,
        world: &mut World,
        ids: &mut IdMap,
        key: &str,
        text: &str,
    ) -> Result<(), ScnError> {
        let file: ChunkFile<T> =
            ron::from_str(text).map_err(|error| ScnError::parse(key, &error))?;
        if file.system != self.name {
            return Err(ScnError::Chunk {
                key: key.to_string(),
                declared: file.system,
                expected: self.name.clone(),
            });
        }

        // Spawn first, attach second: `World::spawn` and the borrow of the
        // system out of the schedule are both `&mut world`, so they cannot be
        // held at once.
        let mut spawned = Vec::with_capacity(file.entities.len());
        for (id, data) in file.entities {
            let entity = world.spawn();
            ids.bind(key, id, entity)?;
            spawned.push((entity, data));
        }
        let system = system_named::<T>(world, &self.name)?;
        for (entity, data) in spawned {
            system.attach(entity, data);
        }
        Ok(())
    }

    fn write(&self, world: &mut World, ids: &IdMap) -> Result<String, ScnError> {
        let name = self.name.clone();
        let system = system_named::<T>(world, &name)?;

        // A `BTreeMap` and not a sort: the file's order is the id order, and
        // `System::iter_entities` yields storage order, which swap-remove makes
        // a function of attach/detach history.
        let mut rows: BTreeMap<SceneEntityId, &T> = BTreeMap::new();
        for (entity, data) in system.iter_entities() {
            let id = ids.id(entity).ok_or_else(|| ScnError::NoSceneId {
                system: name.clone(),
                entity: format!("{entity:?}"),
            })?;
            rows.insert(id, data);
        }

        let file = ChunkFile {
            system: name.clone(),
            entities: rows.into_iter().collect(),
        };
        ron::ser::to_string_pretty(&file, pretty()).map_err(|error| ScnError::Write {
            system: name,
            message: error.to_string(),
        })
    }
}

/// The `System<T>` in `world`'s schedule that is called `name`.
///
/// The name is the query, not the type: [`crcbl_ecs::World::system_mut`] finds
/// the first system of a type, and a scene whose manifest holds two systems of
/// one component would get the same array twice.
fn system_named<'w, T>(world: &'w mut World, name: &str) -> Result<&'w mut System<T>, ScnError>
where
    T: ComponentHash + 'static,
{
    world
        .schedule_mut()
        .iter_mut()
        .find(|system| system.name() == name)
        .and_then(|system| system.as_any_mut().downcast_mut::<System<T>>())
        .ok_or_else(|| ScnError::NoSystem {
            system: name.to_string(),
            component: type_name::<T>(),
        })
}

// ---------------------------------------------------------------------------
// The scene
// ---------------------------------------------------------------------------

/// A loaded scene: its name, the systems its directory holds, and its [`Env`].
///
/// The entities are not in here — they are in the [`World`] the loader spawned
/// them into, which is the whole point of a scene file. [`IdMap`] is what joins
/// the two.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    name: String,
    systems: Vec<String>,
    env: Env,
}

impl Scene {
    /// The only format version this build reads or writes.
    ///
    /// **0, and it stays 0 until 1.0** — the rule
    /// `docs/notes/tooling.md` records for every format the engine owns.
    /// The header exists so a stale file fails loudly, not so it can be carried
    /// forward: a `.scn/` that no longer loads is re-authored, and there is no
    /// migration machinery before 1.0.
    pub const FORMAT: u32 = 0;

    /// A scene with nothing loaded into a world yet, for a caller that is
    /// building one to [`save`](Self::save) rather than reading one.
    #[must_use]
    pub fn new(name: impl Into<String>, systems: Vec<String>, env: Env) -> Self {
        Self {
            name: name.into(),
            systems,
            env,
        }
    }

    /// Reads `dir` through `source` and spawns its entities into `world`.
    ///
    /// Reads exactly `dir/scene.ron`, `dir/env.ron`, and `dir/sys/<name>.ron`
    /// for each name the header's manifest lists, **in manifest order** — so
    /// the entity ids a `World` hands out are a function of the file rather
    /// than of the order `chunks` happens to be in.
    ///
    /// `dir` may be `.` (or empty), which is what a [`crcbl_assets::DirSource`]
    /// rooted at the scene directory itself wants.
    ///
    /// # Errors
    ///
    /// [`ScnError`], every variant of which names the key it is about:
    /// a key that will not read, text that is not RON or is RON that is not
    /// this format (an unknown field included — every file type sets
    /// `deny_unknown_fields`), a `format` this build does not know, a manifest
    /// entry with no codec in `chunks`, or a chunk whose declared system is not
    /// the one its file is named for.
    pub fn load(
        source: &dyn AssetSource,
        dir: &Path,
        chunks: &[Box<dyn SystemChunk>],
        world: &mut World,
    ) -> Result<(Self, IdMap), ScnError> {
        let prefix = dir.to_str().ok_or_else(|| ScnError::Read {
            key: dir.display().to_string(),
            source: StorageError::InvalidPath(dir.to_path_buf()),
        })?;

        let header_key = join_key(prefix, "scene.ron");
        let text = read_text(source, &header_key)?;
        let header: SceneFile =
            ron::from_str(&text).map_err(|error| ScnError::parse(&header_key, &error))?;
        if header.format != Self::FORMAT {
            return Err(ScnError::Format {
                key: header_key,
                found: header.format,
                expected: Self::FORMAT,
            });
        }

        let env_key = join_key(prefix, "env.ron");
        let text = read_text(source, &env_key)?;
        let env: Env = ron::from_str(&text).map_err(|error| ScnError::parse(&env_key, &error))?;

        let mut ids = IdMap::new();
        for name in &header.systems {
            let codec = codec_named(chunks, name)?;
            let key = join_key(prefix, &format!("sys/{name}.ron"));
            let text = read_text(source, &key)?;
            codec.read(world, &mut ids, &key, &text)?;
        }

        Ok((
            Self {
                name: header.name,
                systems: header.systems,
                env,
            },
            ids,
        ))
    }

    /// The files this scene is, keyed **relative to the scene directory** —
    /// `scene.ron`, `env.ron`, `sys/<name>.ron`.
    ///
    /// Returns the text rather than writing it: no engine crate performs
    /// synchronous IO, and a caller that wants a directory joins each key onto
    /// its root. Keys are asset keys and so are always `/`-separated, for the
    /// reason [`crcbl_store::web::canonical_key`] gives at length — `Path`'s
    /// separator is the host's, and a key written on Windows would not be one
    /// anywhere else.
    ///
    /// **Byte-identical for equal scenes, on every platform.** Fields print in
    /// struct order because that is the order serde's derive visits them,
    /// entities print in [`SceneEntityId`] order because [`IdMap`] is sorted,
    /// floats print through Rust's own shortest-round-trip `Display` (which is
    /// `core`'s `flt2dec` and not a libm that varies by platform), and the
    /// newline is pinned to `\n` rather than left to
    /// [`ron::ser::PrettyConfig`]'s default of `\r\n` on Windows. Nothing here
    /// carries a timestamp or any editor-session state.
    ///
    /// [`crcbl_store::web::canonical_key`]: https://docs.rs/crcbl-store
    ///
    /// # Errors
    ///
    /// [`ScnError`] if the manifest names a system with no codec in `chunks` or
    /// none in `world`, or if an entity attached to one of them was never given
    /// a [`SceneEntityId`] — a save that silently dropped such a row is the
    /// failure this refusal exists to prevent.
    pub fn save(
        &self,
        world: &mut World,
        ids: &IdMap,
        chunks: &[Box<dyn SystemChunk>],
    ) -> Result<BTreeMap<String, String>, ScnError> {
        let header = SceneFile {
            format: Self::FORMAT,
            name: self.name.clone(),
            systems: self.systems.clone(),
        };
        let mut files = BTreeMap::new();
        files.insert("scene.ron".to_string(), to_ron(&header));
        files.insert("env.ron".to_string(), to_ron(&self.env));
        for name in &self.systems {
            let codec = codec_named(chunks, name)?;
            files.insert(format!("sys/{name}.ron"), codec.write(world, ids)?);
        }
        Ok(files)
    }

    /// The scene's name, as its header spells it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The manifest: the systems whose chunk files this scene is made of, in
    /// the order they are read and written.
    #[must_use]
    pub fn systems(&self) -> &[String] {
        &self.systems
    }

    /// The camera and ambience `env.ron` carries.
    #[must_use]
    pub fn env(&self) -> &Env {
        &self.env
    }

    /// The [`Env`], to change before a [`save`](Self::save).
    pub fn env_mut(&mut self) -> &mut Env {
        &mut self.env
    }
}

/// The codec registered for `name`, or the refusal that names it.
fn codec_named<'c>(
    chunks: &'c [Box<dyn SystemChunk>],
    name: &str,
) -> Result<&'c dyn SystemChunk, ScnError> {
    chunks
        .iter()
        .find(|codec| codec.name() == name)
        .map(AsRef::as_ref)
        .ok_or_else(|| ScnError::NoCodec {
            system: name.to_string(),
        })
}

/// `prefix/name`, or `name` when the prefix is the scene directory itself.
///
/// String-joined rather than [`Path::join`]ed because the result is an asset
/// key, which is a URL path on every host — `Path`'s separator is the host's,
/// and a key joined on Windows would not be a key anywhere else.
///
/// An empty prefix needs the first arm: `/scene.ron` has an empty leading
/// component and every source refuses it. A prefix of `.` does not, and
/// deliberately has no arm of its own — `crcbl_store::web::canonical_key` drops
/// a `.` component, so `./scene.ron` and `scene.ron` are one key at both ends.
fn join_key(prefix: &str, name: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

/// The bytes at `key`, as text.
fn read_text(source: &dyn AssetSource, key: &str) -> Result<String, ScnError> {
    let bytes = source
        .read(Path::new(key))
        .map_err(|source| ScnError::Read {
            key: key.to_string(),
            source,
        })?;
    String::from_utf8(bytes).map_err(|error| ScnError::Read {
        key: key.to_string(),
        source: StorageError::Other(format!("not UTF-8: {error}")),
    })
}

/// Writes one of this module's own types.
///
/// `expect` for the reason `crcbl_render::stack::CameraStack::to_ron` gives:
/// [`SceneFile`] and [`Env`] are strings, integers, floats and sequences, and
/// ron's serializer has no failing path over those. A *component* is a caller's
/// type and can fail, which is why [`SystemChunk::write`] returns a `Result`
/// and this does not.
fn to_ron<T: Serialize>(value: &T) -> String {
    ron::ser::to_string_pretty(value, pretty())
        .expect("a scene header and its env have no serializer path that can fail")
}

/// The one writer configuration, so nothing that later writes a chunk can
/// disagree with [`Scene::save`] about what a file looks like.
///
/// A third copy of the three lines `crcbl_render::stack::CameraStack::pretty`
/// and `crcbl_inventory::catalog::Catalog::pretty` already are, rather than a
/// shared helper: the three crates share no dependency that could hold one, and
/// what they actually share is the reason — [`ron::ser::PrettyConfig`]'s default
/// newline is `\r\n` on Windows.
fn pretty() -> ron::ser::PrettyConfig {
    ron::ser::PrettyConfig::new()
        .new_line("\n")
        .indentor("    ")
        .struct_names(true)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a directory is not a scene, or a world is not one that can be written.
///
/// Every variant names the key or the system it is about, because the caller
/// showing this to a person is showing them which file to open.
#[derive(Debug, thiserror::Error)]
pub enum ScnError {
    /// A key would not read, or its bytes are not text.
    #[error("reading `{key}`: {source}")]
    Read {
        /// The asset key that failed.
        key: String,
        /// What the source said.
        source: StorageError,
    },

    /// The text is not RON, or is RON that is not this file's type — an unknown
    /// field included, since every file type sets `deny_unknown_fields`.
    ///
    /// The position is ron's own, and the message is ron's verbatim, which is
    /// what carries the offending field's name.
    #[error("`{key}` line {line}, column {column}: {message}")]
    Parse {
        /// The asset key the text came from.
        key: String,
        /// The line ron stopped at, 1-based.
        line: usize,
        /// The column of that line, 1-based as ron counts it.
        column: usize,
        /// What ron said, without the position it said it at.
        message: String,
    },

    /// The header declares a format this build does not read.
    ///
    /// See [`Scene::FORMAT`] for why there is no migration path behind this.
    #[error("`{key}` is scene format {found}; this build reads format {expected}")]
    Format {
        /// The header's key.
        key: String,
        /// The version the file declares.
        found: u32,
        /// [`Scene::FORMAT`].
        expected: u32,
    },

    /// The manifest names a system the caller registered no [`SystemChunk`]
    /// for.
    ///
    /// An error rather than a skip: a scene that loaded with a third of its
    /// entities missing is worse than one that did not load.
    #[error("the scene's manifest names the system `{system}`, which has no registered codec")]
    NoCodec {
        /// The manifest entry.
        system: String,
    },

    /// A chunk file declares a system that is not the one it was read as.
    #[error("`{key}` is the chunk of system `{declared}`, read as `{expected}`")]
    Chunk {
        /// The chunk's key.
        key: String,
        /// What its `system` field says.
        declared: String,
        /// The manifest entry it was read for.
        expected: String,
    },

    /// The world's schedule holds no `System<T>` under that name.
    #[error("no system named `{system}` holding `{component}` is registered in the world")]
    NoSystem {
        /// The manifest entry.
        system: String,
        /// The component type the codec was built for.
        component: &'static str,
    },

    /// Two rows claim one [`SceneEntityId`].
    #[error("`{key}` reuses the scene entity id {id}")]
    DuplicateId {
        /// The chunk's key.
        key: String,
        /// The id claimed twice.
        id: SceneEntityId,
    },

    /// An entity attached to a manifest system was never given an id, so the
    /// writer has nothing to file it under.
    ///
    /// [`IdMap::assign`] is what an entity spawned after load needs.
    #[error("system `{system}` holds {entity}, which has no scene entity id")]
    NoSceneId {
        /// The system being written.
        system: String,
        /// The entity's `Debug` form, which carries its index and generation.
        entity: String,
    },

    /// A component's own `Serialize` refused.
    #[error("writing system `{system}`: {message}")]
    Write {
        /// The system being written.
        system: String,
        /// What ron said.
        message: String,
    },
}

impl ScnError {
    /// Reduce ron's [`SpannedError`](ron::error::SpannedError) to the key, the
    /// position and the message, so a caller reporting it need not depend on
    /// ron's types. The same shape `crcbl_render::stack::StackError` has.
    fn parse(key: &str, error: &ron::error::SpannedError) -> Self {
        Self::Parse {
            key: key.to_string(),
            line: error.span.start.line,
            column: error.span.start.col,
            message: error.code.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::hash::Hasher;

    use crcbl_assets::MemorySource;
    use crcbl_ecs::System;

    use super::*;

    /// One system's component, in the shape a real one has: a couple of floats
    /// and a name.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Mark {
        position: [f32; 3],
        label: String,
    }

    impl ComponentHash for Mark {
        fn hash_component(&self, hasher: &mut dyn Hasher) {
            for value in self.position {
                hasher.write(&value.to_bits().to_le_bytes());
            }
            hasher.write(self.label.as_bytes());
        }
    }

    const HEADER: &str =
        "Scene(\n    format: 0,\n    name: \"one\",\n    systems: [\n        \"marks\",\n    ],\n)";
    const ENV: &str = "Env(\n    camera: Camera(\n        position: (0.0, 2.0, 8.0),\n        look_at: (0.0, 0.0, 0.0),\n    ),\n    ambient: (0.05, 0.05, 0.08),\n)";
    const MARKS: &str = "Chunk(\n    system: \"marks\",\n    entities: [\n        (0, Mark(\n            position: (1.0, 0.0, -1.0),\n            label: \"first\",\n        )),\n    ],\n)";

    fn source(header: &str, env: &str, marks: &str) -> MemorySource {
        let mut source = MemorySource::new();
        for (key, text) in [
            ("one.scn/scene.ron", header),
            ("one.scn/env.ron", env),
            ("one.scn/sys/marks.ron", marks),
        ] {
            source
                .insert(Path::new(key), text.as_bytes().to_vec())
                .expect("a nested scene key is a legal asset key");
        }
        source
    }

    fn world_with_marks() -> World {
        let mut world = World::new();
        world.register_system(Box::new(System::<Mark>::new("marks")));
        world
    }

    fn codecs() -> Vec<Box<dyn SystemChunk>> {
        vec![chunk_of::<Mark>("marks")]
    }

    fn load(header: &str, env: &str, marks: &str) -> Result<(Scene, IdMap, World), ScnError> {
        let source = source(header, env, marks);
        let mut world = world_with_marks();
        let (scene, ids) = Scene::load(&source, Path::new("one.scn"), &codecs(), &mut world)?;
        Ok((scene, ids, world))
    }

    /// **The canonical trio is exactly what the writer writes**, which is the
    /// discipline that keeps a committed scene honest: the fixtures above are
    /// generated by this writer, so a change to the configuration moves them
    /// and has to be blessed rather than absorbed.
    #[test]
    fn the_canonical_scene_is_what_the_writer_writes() {
        let (scene, ids, mut world) = load(HEADER, ENV, MARKS).expect("the canonical scene loads");
        let files = scene
            .save(&mut world, &ids, &codecs())
            .expect("a scene that loaded can be written");

        assert_eq!(files["scene.ron"], HEADER);
        assert_eq!(files["env.ron"], ENV);
        assert_eq!(files["sys/marks.ron"], MARKS);
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            ["env.ron", "scene.ron", "sys/marks.ron"],
            "the manifest's files and nothing else"
        );

        for (key, text) in &files {
            assert!(!text.contains('\r'), "the newline is pinned in {key}");
        }

        assert_eq!(scene.name(), "one");
        assert_eq!(scene.systems(), ["marks"]);
        assert_eq!(scene.env().camera.position, [0.0, 2.0, 8.0]);
        assert_eq!(scene.env().ambient, [0.05, 0.05, 0.08]);
        assert_eq!(ids.len(), 1);
        assert!(!ids.is_empty());
    }

    /// **A version this build does not know is refused, not read as the current
    /// one.** [`Scene::FORMAT`]'s doc says why that matters more than a
    /// migration would.
    #[test]
    fn a_scene_of_another_format_is_refused_by_number() {
        let error = load(&HEADER.replace("format: 0", "format: 1"), ENV, MARKS)
            .expect_err("format 1 is not this build's");
        assert!(
            matches!(&error, ScnError::Format { key, found, expected }
                if key == "one.scn/scene.ron" && *found == 1 && *expected == Scene::FORMAT),
            "{error}"
        );
        assert!(error.to_string().contains("one.scn/scene.ron"), "{error}");
    }

    /// **A field nothing reads is refused where it is written.** Every file type
    /// sets `deny_unknown_fields`, so a typo is a position rather than a value
    /// silently defaulted.
    #[test]
    fn an_unknown_field_is_refused_with_the_position_ron_gives() {
        let error = load(HEADER, &ENV.replace("ambient", "ambent"), MARKS)
            .expect_err("an unknown field is not an env");
        let ScnError::Parse {
            key,
            line,
            column,
            message,
        } = &error
        else {
            panic!("{error}");
        };
        assert_eq!(key, "one.scn/env.ron");
        assert!(*line > 1, "the position is ron's, not zero");
        assert!(*column >= 1);
        assert!(message.contains("ambent"), "ron names the field: {message}");

        let error = load(HEADER, ENV, &MARKS.replace("label", "labl"))
            .expect_err("an unknown field is not a chunk either");
        assert!(
            matches!(&error, ScnError::Parse { key, .. } if key == "one.scn/sys/marks.ron"),
            "{error}"
        );
    }

    /// **A chunk file that is not this system's is refused rather than
    /// attached.** The file name and the `system` field are two statements of
    /// the same thing, and a copied file is where they disagree.
    #[test]
    fn a_chunk_declaring_another_system_is_refused() {
        let error = load(HEADER, ENV, &MARKS.replace("\"marks\"", "\"props\""))
            .expect_err("that chunk belongs to another system");
        assert!(
            matches!(&error, ScnError::Chunk { key, declared, expected }
                if key == "one.scn/sys/marks.ron" && declared == "props" && expected == "marks"),
            "{error}"
        );
    }

    /// **A manifest entry with no codec stops the load.** Skipping it would
    /// produce a world missing a third of its entities and no way to tell.
    #[test]
    fn a_manifest_system_with_no_codec_is_refused_by_name() {
        let source = source(HEADER, ENV, MARKS);
        let mut world = world_with_marks();
        let error = Scene::load(&source, Path::new("one.scn"), &[], &mut world)
            .expect_err("no codec was registered for `marks`");
        assert!(
            matches!(&error, ScnError::NoCodec { system } if system == "marks"),
            "{error}"
        );
    }

    /// **A codec with no system in the world is refused too.** The mirror of
    /// the case above, and the one a caller hits by forgetting
    /// `register_system`.
    #[test]
    fn a_codec_with_no_system_in_the_world_is_refused() {
        let source = source(HEADER, ENV, MARKS);
        let mut world = World::new();
        let error = Scene::load(&source, Path::new("one.scn"), &codecs(), &mut world)
            .expect_err("the world holds no `marks`");
        assert!(
            matches!(&error, ScnError::NoSystem { system, .. } if system == "marks"),
            "{error}"
        );
    }

    /// **One id, one entity.** Two rows claiming the same id would drop one out
    /// of the map and come back as a save that lost a row.
    #[test]
    fn a_repeated_scene_entity_id_is_refused() {
        let doubled = MARKS.replace(
            "    ],\n)",
            "        (0, Mark(\n            position: (2.0, 0.0, 0.0),\n            label: \"second\",\n        )),\n    ],\n)",
        );
        let error = load(HEADER, ENV, &doubled).expect_err("id 0 is claimed twice");
        assert!(
            matches!(&error, ScnError::DuplicateId { key, id }
                if key == "one.scn/sys/marks.ron" && *id == SceneEntityId(0)),
            "{error}"
        );
    }

    /// **An entity with no id is refused rather than dropped.** The writer has
    /// nowhere to file it, and a save that quietly shrank the scene is the
    /// failure worth being loud about; [`IdMap::assign`] is the fix.
    #[test]
    fn saving_an_entity_that_never_got_an_id_is_refused() {
        let (scene, mut ids, mut world) = load(HEADER, ENV, MARKS).expect("the scene loads");
        let stray = world.spawn();
        let marks = system_named::<Mark>(&mut world, "marks").expect("the world holds it");
        marks.attach(
            stray,
            Mark {
                position: [0.0; 3],
                label: "stray".to_string(),
            },
        );

        let error = scene
            .save(&mut world, &ids, &codecs())
            .expect_err("the stray has no scene id");
        assert!(
            matches!(&error, ScnError::NoSceneId { system, .. } if system == "marks"),
            "{error}"
        );

        // Give it one and the same save succeeds, with the id after the file's.
        assert_eq!(ids.assign(stray), SceneEntityId(1));
        assert_eq!(ids.assign(stray), SceneEntityId(1), "assign is idempotent");
        let files = scene
            .save(&mut world, &ids, &codecs())
            .expect("every entity has an id now");
        assert!(files["sys/marks.ron"].contains("stray"));
        assert_eq!(ids.entity(SceneEntityId(1)), Some(stray));
        assert_eq!(ids.id(stray), Some(SceneEntityId(1)));
    }

    /// **A key that is not there says so, and says which key.**
    #[test]
    fn a_directory_with_no_header_names_the_key_it_wanted() {
        let source = MemorySource::new();
        let mut world = world_with_marks();
        let error = Scene::load(&source, Path::new("one.scn"), &codecs(), &mut world)
            .expect_err("there is no scene there");
        assert!(
            matches!(&error, ScnError::Read { key, source: StorageError::NotFound(_) }
                if key == "one.scn/scene.ron"),
            "{error}"
        );
    }

    /// **A source rooted at the scene directory itself loads.** `.` and the
    /// empty path both mean "here", which is what a `DirSource::at(scene_dir)`
    /// hands over. The empty one is the case with a branch behind it: joining
    /// it would produce `/scene.ron`, whose leading empty component every
    /// source refuses. `.` is carried by `canonical_key` dropping the
    /// component, which is why `join_key` has no arm for it.
    #[test]
    fn a_scene_directory_that_is_the_source_root_loads() {
        let mut source = MemorySource::new();
        for (key, text) in [
            ("scene.ron", HEADER),
            ("env.ron", ENV),
            ("sys/marks.ron", MARKS),
        ] {
            source
                .insert(Path::new(key), text.as_bytes().to_vec())
                .expect("a legal asset key");
        }
        for dir in [".", ""] {
            let mut world = world_with_marks();
            let (scene, ids) = Scene::load(&source, Path::new(dir), &codecs(), &mut world)
                .unwrap_or_else(|error| panic!("`{dir}` is the source root: {error}"));
            assert_eq!(scene.name(), "one");
            assert_eq!(ids.len(), 1);
        }
    }

    /// **A scene built in code writes the same file a loaded one does**, which
    /// is what lets a sample's committed chunk be generated rather than typed.
    #[test]
    fn a_scene_built_in_code_writes_the_canonical_files() {
        let mut world = world_with_marks();
        let mut ids = IdMap::new();
        let entity = world.spawn();
        assert_eq!(ids.assign(entity), SceneEntityId(0));
        system_named::<Mark>(&mut world, "marks")
            .expect("the world holds it")
            .attach(
                entity,
                Mark {
                    position: [1.0, 0.0, -1.0],
                    label: "first".to_string(),
                },
            );

        let mut scene = Scene::new(
            "one",
            vec!["marks".to_string()],
            Env {
                camera: EnvCamera {
                    position: [0.0, 2.0, 8.0],
                    look_at: [0.0; 3],
                },
                ambient: [0.05, 0.05, 0.08],
            },
        );
        let files = scene
            .save(&mut world, &ids, &codecs())
            .expect("the world matches the manifest");
        assert_eq!(files["scene.ron"], HEADER);
        assert_eq!(files["env.ron"], ENV);
        assert_eq!(files["sys/marks.ron"], MARKS);

        scene.env_mut().ambient = [0.0; 3];
        assert_eq!(scene.env().ambient, [0.0; 3]);
        assert_ne!(
            scene
                .save(&mut world, &ids, &codecs())
                .expect("still writable")["env.ron"],
            ENV
        );
    }

    /// The debug form names the system and the component, so a codec list is
    /// readable in a panic message.
    #[test]
    fn a_codecs_debug_output_names_its_system_and_component() {
        let text = format!("{:?}", chunk_of::<Mark>("marks"));
        assert!(text.contains("marks"), "{text}");
        assert!(text.contains("Mark"), "{text}");
    }
}
