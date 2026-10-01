//! A mesh a scene places: a [`Mesh`] row naming a glTF asset, standing where
//! its `position` says, and boxed by the bounds the asset was measured at.
//!
//! ```text
//!     a vocabulary ──▶ scene_mesh::register(&mut registry)
//!                         │
//!                         ├── Mesh under `meshes`   a row of sys/meshes.ron:
//!                         │                         an asset key and a position
//!                         └── a check               an asset key the files
//!                                                   cannot reload, on save
//!
//!     a tool ──▶ MeshLibrary::resolve(world, assets)
//!                  └── each Mesh's asset imported once, its box measured,
//!                      and written into the row — or the row left on the
//!                      placeholder box, with a problem naming the asset
//! ```
//!
//! # The asset is a key, checked where it enters
//!
//! [`Mesh::asset`] is a key into whatever [`AssetSource`] a tool reads its
//! assets through — relative, `/`-separated, and ending in an extension the
//! glTF importer reads ([`MESH_EXTENSIONS`]). [`check_asset`] is the rule, by
//! name: an absolute path, a `..`, another extension, a byte no asset key may
//! hold and a key spelled other than its canonical form are each refused with
//! the reason. A scene row is read through it ([`ScnError::Parse`] naming the
//! file, line and field, as `Body`'s values are), and the [`register`] check
//! reports a key a panel typed, so a scene is never saved holding a path that
//! would walk out of the asset root. The empty key is the one exception: it is
//! a mesh with no asset chosen yet — what attaching one starts as — and is
//! drawn as the placeholder.
//!
//! [`ScnError::Parse`]: crcbl_scene::scn::ScnError::Parse
//!
//! # The box comes from the asset, and is never written
//!
//! A [`Placement`] answers from the component alone, so the bounds have to be
//! **in** the row by the time anything asks — and they are a fact about the
//! asset, not the scene, so the file does not carry them: a re-exported model
//! would otherwise be boxed by the size it used to be. The row holds them
//! beside the key they were measured for ([`Mesh::set_local_bounds`]), filled
//! by [`MeshLibrary::resolve`] after a load and after every edit, and skipped
//! by serde and by the inspector alike. A row whose bounds were measured for
//! another key — a panel retyped the asset — answers the placeholder until it
//! is resolved again, rather than the old asset's box.
//!
//! **An asset that is missing, malformed or not yet chosen is a placeholder,
//! never a panic**: a cube of [`PLACEHOLDER_HALF_EXTENT`] centred on the
//! mesh's origin, which a tool draws, picks and moves like any other box, and
//! a [`MeshProblem`] naming the entity's asset and why, which a tool reports.
//!
//! # Where the box stands
//!
//! `position` is where the asset's own origin is put, which is what a person
//! types and what moving the mesh writes. The placement is the measured box,
//! offset by it: a model authored standing on its origin has its centre half
//! its height above `position`, and the translate handle and a simulated body
//! move it by the distance they move its centre, as they move any component
//! whose centre is not its position ([`crate::scene_physics`]'s module docs).
//!
//! A flat model — one quad — measures zero on one axis; its box is given
//! [`MIN_HALF_EXTENT`] there, so a ray can strike it and a body can collide as
//! it, rather than a box no collider can be.

use std::fmt;
use std::path::Path;

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crcbl_assets::AssetSource;
use crcbl_ecs::{ComponentHash, Entity, System, World};
use crcbl_reflect::Reflect;
use crcbl_store::web::canonical_key;

use crate::registry::{Placement, Registry, check_chunk};

/// The scene system every [`Mesh`] is a row of: the manifest entry and the
/// chunk file's stem.
pub const MESHES: &str = "meshes";

/// The extensions [`Mesh::asset`] may end in: the two forms the glTF importer
/// reads, `.glb` and `.gltf`, compared without regard to ASCII case.
pub const MESH_EXTENSIONS: [&str; 2] = ["glb", "gltf"];

/// Half the side of the cube a mesh is boxed as while its asset has not been
/// measured — missing, malformed, not chosen, or not yet resolved — in metres.
///
/// The size a new greybox block is, so a placeholder reads as "a thing here"
/// at the scale of the scenes it appears in.
pub const PLACEHOLDER_HALF_EXTENT: f64 = 0.5;

/// The least half extent a measured box has on any axis, in metres: what a
/// flat model is given across its plane, so a ray can strike it and a
/// collider can be made of it. Half a centimetre — thinner than any step a
/// panel drags in.
pub const MIN_HALF_EXTENT: f64 = 0.005;

/// A mesh a scene places: which asset, and where its origin stands. Its box is
/// the asset's, measured — see the [module docs](self).
#[derive(Clone, Debug, Default, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl_reflect")]
#[serde(try_from = "MeshRow")]
pub struct Mesh {
    /// The asset's key, as [`check_asset`] admits it — or empty for a mesh
    /// with no asset chosen yet, which is drawn as the placeholder.
    #[reflect(name = "Asset")]
    pub asset: String,
    /// Where the asset's own origin stands.
    #[reflect(name = "Position", step = 0.01)]
    pub position: [f64; 3],
    /// The asset's box in its own frame, with the key it was measured for;
    /// never written to a file nor shown in a panel.
    #[reflect(skip)]
    #[serde(skip)]
    measured: Option<Measured>,
}

/// A box measured for one asset key: see [`Mesh::set_local_bounds`].
#[derive(Clone, Debug, PartialEq)]
struct Measured {
    asset: String,
    min: DVec3,
    max: DVec3,
}

impl Mesh {
    /// A mesh of `asset` with its origin at `position`, not yet measured.
    #[must_use]
    pub fn new(asset: impl Into<String>, position: [f64; 3]) -> Self {
        Self {
            asset: asset.into(),
            position,
            measured: None,
        }
    }

    /// A mesh of `asset` standing on `point`: the bottom centre of its box —
    /// `local`, the asset's own box, or the placeholder's where it has none —
    /// put at `point`. What a tool dropping an asset onto a surface spawns.
    #[must_use]
    pub fn standing_on(
        asset: impl Into<String>,
        point: DVec3,
        local: Option<(DVec3, DVec3)>,
    ) -> Self {
        let (min, max) = local.unwrap_or_else(placeholder);
        let centre = (min + max) * 0.5;
        let foot = DVec3::new(centre.x, min.y, centre.z);
        Self::new(asset, (point - foot).to_array())
    }

    /// The asset's box in its own frame, as it was measured for the key this
    /// row holds now — [`None`] for a row not measured, or measured for a key
    /// it no longer holds.
    #[must_use]
    pub fn local_bounds(&self) -> Option<(DVec3, DVec3)> {
        self.measured
            .as_ref()
            .filter(|measured| measured.asset == self.asset)
            .map(|measured| (measured.min, measured.max))
    }

    /// Records `min` and `max` as the box of the asset this row holds now.
    pub fn set_local_bounds(&mut self, min: DVec3, max: DVec3) {
        self.measured = Some(Measured {
            asset: self.asset.clone(),
            min,
            max,
        });
    }

    /// Forgets the measured box, so the row answers the placeholder.
    pub fn clear_local_bounds(&mut self) {
        self.measured = None;
    }
}

/// The placeholder's box in a mesh's own frame: a cube of
/// [`PLACEHOLDER_HALF_EXTENT`] centred on its origin.
fn placeholder() -> (DVec3, DVec3) {
    (
        DVec3::splat(-PLACEHOLDER_HALF_EXTENT),
        DVec3::splat(PLACEHOLDER_HALF_EXTENT),
    )
}

/// The measured box offset by `position`, or the placeholder's — never
/// [`None`]: a mesh is a thing in space whether or not its asset loaded.
impl Placement for Mesh {
    fn placement(&self) -> Option<(DVec3, DVec3)> {
        let (min, max) = self.local_bounds().unwrap_or_else(placeholder);
        let centre = DVec3::from_array(self.position) + (min + max) * 0.5;
        let half = ((max - min) * 0.5).max(DVec3::splat(MIN_HALF_EXTENT));
        Some((centre, half))
    }
}

impl ComponentHash for Mesh {
    /// The key and the position: the measured box is a fact about the asset,
    /// recomputed from the key, so it adds nothing a hash could disagree on.
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        hasher.write(self.asset.as_bytes());
        // A terminator, so a key and a position cannot run into one another.
        hasher.write_u8(0);
        for value in self.position {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

/// [`Mesh`] as a file spells it, before [`check_asset`]: what serde reads a
/// row into, so a key that would walk out of the asset root is refused by the
/// loader with the file's line and column.
#[derive(Deserialize)]
#[serde(rename = "Mesh")]
struct MeshRow {
    asset: String,
    position: [f64; 3],
}

impl TryFrom<MeshRow> for Mesh {
    type Error = MeshPathError;

    fn try_from(row: MeshRow) -> Result<Self, MeshPathError> {
        check_asset(&row.asset)?;
        Ok(Self::new(row.asset, row.position))
    }
}

/// Why a string is not a [`Mesh::asset`] key, naming it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshPathError {
    /// An absolute path — a leading `/` or a drive — where a key relative to
    /// the asset root belongs.
    Absolute(String),
    /// A `..` segment, which would walk out of the asset root.
    Parent(String),
    /// An extension the glTF importer does not read, or none.
    Extension(String),
    /// A byte, an empty segment or a length no asset key may have.
    NotAKey(String),
    /// A legal key spelled other than its canonical form — a `./` prefix or a
    /// trailing `/` — which would load and then save as a different string.
    NotCanonical {
        /// As written.
        asset: String,
        /// As a key is spelled.
        canonical: String,
    },
}

impl fmt::Display for MeshPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absolute(asset) => write!(
                f,
                "a mesh's `asset` must be relative to the asset root, not the absolute `{asset}`"
            ),
            Self::Parent(asset) => write!(
                f,
                "a mesh's `asset` may not climb out of the asset root with `..`: `{asset}`"
            ),
            Self::Extension(asset) => write!(
                f,
                "a mesh's `asset` must be a `.glb` or `.gltf` file, not `{asset}`"
            ),
            Self::NotAKey(asset) => write!(
                f,
                "a mesh's `asset` must be an asset key — letters, digits, `.`, `_` and `-` \
                 between single `/`s — not `{asset}`"
            ),
            Self::NotCanonical { asset, canonical } => write!(
                f,
                "a mesh's `asset` must be spelled `{canonical}`, not `{asset}`"
            ),
        }
    }
}

impl std::error::Error for MeshPathError {}

/// `Ok` for a key a [`Mesh`] may name — or the empty key, a mesh with no asset
/// chosen — or the first rule it breaks. See the [module docs](self).
///
/// # Errors
///
/// [`MeshPathError`] naming the rule and the key.
pub fn check_asset(asset: &str) -> Result<(), MeshPathError> {
    if asset.is_empty() {
        return Ok(());
    }
    let owned = || asset.to_owned();
    let bytes = asset.as_bytes();
    if asset.starts_with('/') || (bytes.len() > 1 && bytes[1] == b':') {
        return Err(MeshPathError::Absolute(owned()));
    }
    if asset.split(['/', '\\']).any(|part| part == "..") {
        return Err(MeshPathError::Parent(owned()));
    }
    let extension = asset.rsplit_once('.').map(|(_, extension)| extension);
    if !extension.is_some_and(|extension| {
        MESH_EXTENSIONS
            .iter()
            .any(|known| known.eq_ignore_ascii_case(extension))
    }) {
        return Err(MeshPathError::Extension(owned()));
    }
    let canonical = canonical_key(Path::new(asset)).map_err(|_| MeshPathError::NotAKey(owned()))?;
    if canonical != asset {
        return Err(MeshPathError::NotCanonical {
            asset: owned(),
            canonical,
        });
    }
    Ok(())
}

/// Whether `key` names an asset a [`Mesh`] can be made of: what an asset
/// browser lists. The empty key is a mesh with no asset, not an asset.
#[must_use]
pub fn is_mesh_asset(key: &str) -> bool {
    !key.is_empty() && check_asset(key).is_ok()
}

/// Registers [`Mesh`] under [`MESHES`], and the check a tool saving a scene
/// runs over its meshes' keys.
///
/// One call, so a game's vocabulary — or a tool's — takes meshes whole.
pub fn register(registry: &mut Registry) {
    registry.register::<Mesh>(MESHES);
    registry.check(MESHES, check_meshes);
}

/// Whether the meshes chunk under `dir` would load: a key a panel typed that
/// the loader refuses is reported where it was typed.
fn check_meshes(source: &dyn AssetSource, dir: &Path) -> Result<(), String> {
    check_chunk::<Mesh>(source, dir, MESHES)
}

/// The `System<Mesh>` in `world` called [`MESHES`], found by name for
/// [`crate::registry`]'s reason.
fn meshes(world: &mut World) -> Option<&mut System<Mesh>> {
    world
        .schedule_mut()
        .iter_mut()
        .find(|system| system.name() == MESHES)
        .and_then(|system| system.as_any_mut().downcast_mut::<System<Mesh>>())
}

/// `entity`'s mesh, or [`None`] for an entity the [`MESHES`] system does not
/// hold — or a world with no such system.
#[must_use]
pub fn mesh_of(world: &mut World, entity: Entity) -> Option<&Mesh> {
    meshes(world)?.get(entity)
}

/// Why a mesh's asset could not be measured, naming the asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshError {
    /// No asset is chosen: the key is empty.
    Unassigned,
    /// The key is not one a mesh may name.
    Path(MeshPathError),
    /// The source has been asked for the asset and does not have it yet. Not
    /// remembered, so the next resolve asks again.
    Pending(String),
    /// The asset would not import: missing, or not a glTF the importer reads.
    Read {
        /// Which asset.
        asset: String,
        /// What the source or the importer said.
        message: String,
    },
    /// The asset imported and places no vertex in its scene.
    Empty(String),
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unassigned => f.write_str("has no asset chosen, so it is drawn as a placeholder"),
            Self::Path(error) => write!(f, "{error}"),
            Self::Pending(asset) => write!(f, "`{asset}` is still being fetched"),
            Self::Read { asset, message } => write!(f, "`{asset}` would not load: {message}"),
            Self::Empty(asset) => write!(f, "`{asset}` holds no vertex to box"),
        }
    }
}

impl std::error::Error for MeshError {}

/// A mesh whose asset could not be measured: which entity, and why. It is
/// boxed as the placeholder until the asset can be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshProblem {
    /// Whose mesh.
    pub entity: Entity,
    /// The key it names, as it named it.
    pub asset: String,
    /// Why it has no box of its own.
    pub error: MeshError,
}

impl fmt::Display for MeshProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a mesh {}", self.error)
    }
}

/// What [`MeshLibrary::resolve`] did to a world.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolution {
    /// The entities whose placement moved, or whose box was measured or
    /// forgotten — whose collider, bounds and picture a tool brings up to
    /// date. The second half counts a row retyped to another asset of the
    /// same size, which is placed as before and drawn as another thing.
    pub moved: Vec<Entity>,
    /// Every mesh left on the placeholder, and why, in storage order.
    pub problems: Vec<MeshProblem>,
}

/// Every asset a tool has measured, each imported once: the boxes
/// [`resolve`](Self::resolve) writes into the rows.
///
/// Behind the umbrella's `scene` feature, because measuring is importing and
/// importing is the glTF parser; a build with only `scn` reads and writes
/// [`Mesh`] rows and boxes them as placeholders.
///
/// **A result is remembered for the library's life**, success or failure —
/// [`MeshError::Pending`] excepted — so a frame that resolves a scene of a
/// thousand meshes imports each asset once. A tool that wants an asset read
/// again after it changed on disk makes a new library.
#[cfg(feature = "scene")]
#[derive(Debug, Default)]
pub struct MeshLibrary {
    measured: std::collections::BTreeMap<String, Result<(DVec3, DVec3), MeshError>>,
}

#[cfg(feature = "scene")]
impl MeshLibrary {
    /// A library that has measured nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The box of the asset `asset` names in its own frame, read through
    /// `source` the first time it is asked for.
    ///
    /// # Errors
    ///
    /// [`MeshError`] naming the asset: none chosen, a key a mesh may not name,
    /// one still being fetched, one that will not import, or one that places
    /// no vertex.
    pub fn measure(
        &mut self,
        source: &dyn AssetSource,
        asset: &str,
    ) -> Result<(DVec3, DVec3), MeshError> {
        if asset.is_empty() {
            return Err(MeshError::Unassigned);
        }
        check_asset(asset).map_err(MeshError::Path)?;
        if let Some(measured) = self.measured.get(asset) {
            return measured.clone();
        }
        let measured = measure(source, asset);
        if !matches!(measured, Err(MeshError::Pending(_))) {
            self.measured.insert(asset.to_owned(), measured.clone());
        }
        measured
    }

    /// Writes each [`Mesh`] row of `world` the box of its asset, read through
    /// `source`, or leaves it on the placeholder with a problem saying why.
    pub fn resolve(&mut self, world: &mut World, source: &dyn AssetSource) -> Resolution {
        let mut resolution = Resolution::default();
        let Some(system) = meshes(world) else {
            return resolution;
        };
        let entities: Vec<Entity> = system.iter_entities().map(|(entity, _)| entity).collect();
        for entity in entities {
            let Some(mesh) = system.get_mut(entity) else {
                continue;
            };
            let before = (mesh.placement(), mesh.local_bounds());
            match self.measure(source, &mesh.asset) {
                Ok((min, max)) => mesh.set_local_bounds(min, max),
                Err(error) => {
                    mesh.clear_local_bounds();
                    resolution.problems.push(MeshProblem {
                        entity,
                        asset: mesh.asset.clone(),
                        error,
                    });
                }
            }
            if (mesh.placement(), mesh.local_bounds()) != before {
                resolution.moved.push(entity);
            }
        }
        resolution
    }
}

/// [`MeshLibrary::measure`]'s import: the box around every vertex of every
/// node of the asset's scene, each through its node's transform — the
/// transforms its picture is drawn with.
#[cfg(feature = "scene")]
fn measure(source: &dyn AssetSource, asset: &str) -> Result<(DVec3, DVec3), MeshError> {
    let scene =
        crcbl_scene::import_gltf(source, Path::new(asset)).map_err(|error| match error {
            crcbl_store::StorageError::Pending(_) => MeshError::Pending(asset.to_owned()),
            other => MeshError::Read {
                asset: asset.to_owned(),
                message: other.to_string(),
            },
        })?;
    let mut bounds: Option<(DVec3, DVec3)> = None;
    for instance in scene.instances() {
        let transform = glam::Mat4::from_cols_array(&instance.transform());
        let Some(mesh) = scene.meshes().get(instance.mesh()) else {
            continue;
        };
        for primitive in mesh.primitives() {
            for &position in primitive.positions() {
                let at = transform
                    .transform_point3(glam::Vec3::from_array(position))
                    .as_dvec3();
                if !at.is_finite() {
                    return Err(MeshError::Read {
                        asset: asset.to_owned(),
                        message: "a vertex lands at a position that is not finite".to_owned(),
                    });
                }
                bounds = Some(match bounds {
                    Some((min, max)) => (min.min(at), max.max(at)),
                    None => (at, at),
                });
            }
        }
    }
    bounds.ok_or_else(|| MeshError::Empty(asset.to_owned()))
}

#[cfg(test)]
mod tests;
