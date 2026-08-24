//! From a path on the command line to something the renderer can draw.
//!
//! ```text
//! /home/me/models/helmet.glb
//!   └─ split ──▶ DirSource::at("/home/me/models") + key "helmet.glb"
//!        └─ import_gltf ──▶ GltfScene
//!             ├─ world_bounds ──▶ Aabb   (what OrbitCamera::frame fits)
//!             └─ build_render_scene ──▶ SceneDesc + InstanceDescs + Skips
//! ```
//!
//! # Nothing here panics on a file, whatever is in it
//!
//! This is the one application in the tree a non-developer is meant to point at
//! an arbitrary file, so every way a document can be wrong has to come back as
//! a [`LoadError`] a person can act on. That is more than not writing `unwrap`:
//! [`crcbl::scene::import_gltf`] is already documented never to panic, and
//! [`crcbl::render::OrbitCamera::frame`] *does* panic on bounds that are not
//! finite — and nothing between a file and that call checks. A `POSITION`
//! accessor holding an infinity is legal glTF as far as `gltf_check` is
//! concerned, which never looks at a float's value, so the
//! [`NonFiniteGeometry`](LoadError::NonFiniteGeometry) check in [`load`] is
//! what stands between one and the frame.
//!
//! # The path is split, and the directory becomes the asset root
//!
//! [`AssetSource`](crcbl::assets::AssetSource) reads *keys*, not paths: a key is
//! relative to a root and has to satisfy `crcbl_store::web::canonical_key`.
//! Making the file's own directory the root and its file name the key is what
//! lets a `.gltf` resolve the `.bin` and the `.png` sitting beside it, which is
//! the arrangement every DCC tool exports — a root further up would work for the
//! document and refuse its siblings' `../` uris, and a root further down has no
//! file in it.
//!
//! The key rule is stricter than a filesystem's: `[A-Za-z0-9._-]` and nothing
//! else, so `My Model.glb` is refused. That is not this module's rule to relax —
//! it is what makes an asset tree that loads natively load over HTTP too — so
//! the refusal is reported with the rule in it rather than silently rewritten.

use std::fmt;
use std::path::{Path, PathBuf};

use crcbl::assets::{DirSource, MemorySource};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::cull::Aabb;
use crcbl::scene::GltfScene;
use crcbl::scene::gltf_render::{RenderScene, Skip};
use crcbl::shaders::skinning::SkinBinding;
use crcbl::store::StorageError;

/// A document that loaded: what to draw, where it is, and what was lost.
#[derive(Debug)]
pub struct Model {
    /// The asset key the document was read under — its file name. Every message
    /// about it names this rather than the whole path, so a log line is short
    /// and still identifies the file within its directory.
    pub key: PathBuf,
    /// What [`build_render_scene`](crcbl::scene::gltf_render::build_render_scene)
    /// produced: the description, the instances, and the skips.
    pub render: RenderScene,
    /// The box around every vertex the *document* declares, in world space.
    ///
    /// See [`world_bounds`] for what "the document declares" includes and what
    /// it therefore is not.
    pub bounds: Aabb,
    /// What the document's skins and animations amount to — see [`Rig`].
    pub rig: Rig,
    /// The skeleton and the clip the viewer poses and plays, or `None` for a
    /// document with no skin — see [`crate::anim`].
    ///
    /// Converted here, at load, rather than kept as the imported scene: this is
    /// the only part of a document's rig anything downstream can consume, and
    /// carrying the keyframes twice would be keeping the rest for nobody.
    /// Everything the conversion could not bring in is already on
    /// [`Model::skipped`] beside the renderer's own losses.
    pub playable: Option<crate::anim::Playable>,
    /// Which of [`RenderScene::instances`] the document's skin deforms, and
    /// what the frame needs to deform them — see [`Skinned`].
    ///
    /// Empty for the great majority of documents, which wear no skin. Built
    /// here, at load, for [`playable`](Self::playable)'s reason: it is the last
    /// point the imported document exists, and the node-to-skin mapping this
    /// needs lives on the [`GltfScene`] and nowhere downstream of it.
    pub skinned: Skinned,
}

/// What [`crate::gpu`] needs to draw this document's skinned geometry: the
/// instances a skin deforms, and how large a palette poses them.
///
/// **`instances` being empty is the ordinary case** and is what a document with
/// no skin, and a document whose skinned nodes were all refused, both produce —
/// the frame then draws exactly what it drew before skinning existed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skinned {
    /// Matrices one instance's palette holds: the joint count of the skin
    /// [`Model::playable`] poses, and zero when there is none.
    ///
    /// The skinning pass is sized from this times [`instances`](Self::instances)'
    /// length, because every one of them is posed from the same skeleton.
    pub joints: u32,
    /// One entry per instance a skin deforms, in [`RenderScene::instances`]
    /// order.
    pub instances: Vec<SkinnedInstance>,
}

/// One instance of the document that a skin deforms.
///
/// It **replaces** a row of [`RenderScene::instances`] rather than joining it:
/// the row is drawn through the skinning seam instead of through
/// [`ForwardRenderer::add_instance`](crcbl::render::ForwardRenderer::add_instance),
/// and a caller that placed both would draw the mesh twice — once deformed and
/// once in its bind pose, in the same frame, at two different places.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinnedInstance {
    /// Which of [`RenderScene::instances`] this replaces.
    ///
    /// Its mesh and its material are read from that row rather than copied
    /// here, so there is no second answer to drift from it. Its **transform**
    /// is not — see [`transform`](Self::transform), which is the one field a
    /// skinned draw cannot take from the row it replaces.
    pub instance: usize,
    /// **Skeleton space to world**, which is where the drawing node's own
    /// transform is deliberately absent.
    ///
    /// glTF is explicit that "the transform of the skinned mesh node MUST be
    /// ignored": a skinned vertex is placed by its joints, which carry their own
    /// placement. What a palette does *not* carry is where the skeleton itself
    /// hangs — `crcbl::anim::Palette` composes a root joint's global from its
    /// own local transform and stops there — so this is the world transform of
    /// the node those roots hang from, and it is the identity for a rig whose
    /// root joint is a root of the scene.
    pub transform: crcbl::math::Mat4,
    /// One [`SkinBinding`] per emitted vertex of the row's mesh, in the same
    /// order:
    /// [`MeshOrigin::bindings`](crcbl::scene::gltf_render::MeshOrigin::bindings).
    ///
    /// Owned rather than borrowed from the [`RenderScene`], because the frame
    /// uploads them every frame and the document they came from is dropped once
    /// the renderer is built.
    pub bindings: Vec<SkinBinding>,
}

/// What a document's rig amounts to, as much of it as this application reports.
///
/// **A summary, not the rig.** [`crcbl::scene::GltfScene`] holds the joints, the
/// bind matrices and every keyframe, and the part of that this application can
/// use is converted into [`Model::playable`] and the rest of the imported scene
/// dropped — see [`crate::anim`]. What is left for a person looking at an
/// unfamiliar file is the two facts that say whether the rig survived the
/// import at all: how big the skeleton is, and what the file's animations are
/// called. Both cover *every* skin and *every* animation, where the player
/// takes the first of each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rig {
    /// How many joints the document's skins declare, summed over every skin.
    ///
    /// A sum rather than a per-skin list because the count is a scale — "this
    /// file has a skeleton in it, and it is about this big" — and the panel
    /// that draws it has no room for a row per skin. Zero for the great
    /// majority of documents, which declare no skin at all.
    ///
    /// Skins can share joint nodes, so this can exceed the number of *nodes*
    /// that are joints. It is what the file declares, which is the number that
    /// moves when an import stops reading one of them.
    pub joints: usize,
    /// The document's animation clips, named, in file order.
    ///
    /// glTF does not require an animation to carry a name, and a clip that has
    /// none arrives here as `(unnamed clip N)` with `N` its index in that same
    /// file order. A stand-in rather than a blank, because a panel drawing an
    /// empty row says nothing about whether the clip is there; and one carrying
    /// the index rather than a bare `(unnamed)`, because a document with
    /// several of them would otherwise draw one name several times.
    pub clips: Vec<String>,
}

impl Rig {
    /// Whether the document declared no rig at all: no joint and no clip.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.joints == 0 && self.clips.is_empty()
    }
}

impl Model {
    /// What the conversion could not bring in.
    ///
    /// Empty is the whole file arriving intact.
    #[must_use]
    pub fn skipped(&self) -> &[Skip] {
        &self.render.skipped
    }
}

/// Why a file could not be opened.
///
/// Every variant's [`Display`](fmt::Display) names the file, what was wrong and
/// what to do about it — `docs/plan/sample/05-viewer.md`'s exit criterion is
/// that a file from a tool nobody curated either loads or says why not.
#[derive(Debug)]
pub enum LoadError {
    /// The path does not name a file — it ends in `..`, or it is a directory.
    NotAFile(PathBuf),
    /// The file name is not a legal asset key, or the source refused the read.
    Storage {
        /// The path as it was given on the command line.
        path: PathBuf,
        /// What the seam said.
        why: StorageError,
    },
    /// The document imported and has no vertex in it: no scene, or every node
    /// in it draws nothing.
    NoGeometry(PathBuf),
    /// The box around the document's geometry is not finite.
    ///
    /// Refused rather than drawn, because the camera cannot be framed on it.
    /// Raised for an infinity by the box itself and for a `NaN` by a separate
    /// scan over the positions, for the reason [`world_bounds`] gives.
    NonFiniteGeometry(PathBuf),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAFile(path) => write!(
                f,
                "{}: not a file — give the path of a .gltf or .glb document",
                path.display()
            ),
            Self::Storage { path, why } => match why {
                // The one storage failure whose cause is the *name* rather than
                // the file, and the only one whose message would otherwise
                // leave a user with no idea what to change.
                StorageError::InvalidPath(_) => write!(
                    f,
                    "{}: the file name is not a legal asset key — a key may hold \
                     only letters, digits, '.', '_' and '-', so a name with a space \
                     or an accent in it has to be renamed",
                    path.display()
                ),
                // These already spell the resolved path out, so prefixing them
                // prints it twice. `StorageError` is `#[non_exhaustive]` and a
                // later path-carrying variant will land in the arm below — a
                // message that says the path twice, which is untidy rather than
                // wrong, so this list is worth keeping short by hand.
                StorageError::NotFound(_)
                | StorageError::PermissionDenied(_)
                | StorageError::WrongType(_)
                | StorageError::Pending(_) => write!(f, "{why}"),
                // Everything else names the *key* at most, so the path a user
                // typed has to come from here.
                other => write!(f, "{}: {other}", path.display()),
            },
            Self::NoGeometry(path) => write!(
                f,
                "{}: the document has no geometry — it declares no scene, or every \
                 node in its scene draws no mesh",
                path.display()
            ),
            Self::NonFiniteGeometry(path) => write!(
                f,
                "{}: the document has an infinite or NaN vertex position, so there \
                 is no box to frame the camera on — re-export it from the tool that \
                 produced it",
                path.display()
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// Reads the document at `path`, converts it, and works out where it is.
///
/// The whole of this application's file handling. Every [`Skip`] the conversion
/// produced is on [`Model::skipped`] and has already been logged at warning
/// level by the conversion itself; it is carried so the caller can put it in
/// front of a user who is not reading a log.
///
/// # Errors
///
/// [`LoadError`], for a path that names no file, a name the asset seam refuses,
/// a document the importer could not read, and the two shapes of geometry the
/// camera cannot be framed on. It does not panic on any file.
pub fn load(path: &Path) -> Result<Model, LoadError> {
    let key = PathBuf::from(path.file_name().ok_or_else(|| {
        // `file_name` is `None` for `..` and for a path ending in one. A
        // directory has a file name and fails later, at the read.
        LoadError::NotAFile(path.to_path_buf())
    })?);
    // A bare `model.glb` has an empty parent, which as a root means "the
    // process's working directory" — the same thing the shell meant by it.
    let root = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };

    load_from(&DirSource::at(root), &key, path)
}

/// Reads the document `key` names **out of `source`**, converts it, and works
/// out where it is.
///
/// [`load`] is this over a [`DirSource`] rooted at the file's own directory.
/// The split exists because the browser has no directory to point one at: the
/// web demo compiles its `.glb` into the module and opens it through a
/// [`MemorySource`], and a file dropped on the canvas arrives the same way.
/// [`load_bytes`] is that second shape, written once for both. Every rule below
/// — the non-finite scan, the empty document, the framing box — is the file
/// handling itself and belongs to all of them.
///
/// `reported_as` is what the errors name, and is not always `key`. Natively it
/// is the whole path the user typed, because this is the one application a
/// non-developer points at an arbitrary file and "`model.glb` could not be
/// read" does not say *which* `model.glb`. Where the bytes never had a path,
/// the key is the honest answer and the caller passes it for both.
///
/// # Errors
///
/// [`LoadError`], for a document the importer could not read and the two shapes
/// of geometry the camera cannot be framed on. It does not panic on any file.
pub fn load_from(
    source: &dyn crcbl::assets::AssetSource,
    key: &Path,
    reported_as: &Path,
) -> Result<Model, LoadError> {
    let key = PathBuf::from(key);
    let path = reported_as;
    let imported = crcbl::scene::import_gltf(source, &key).map_err(|why| LoadError::Storage {
        path: path.to_path_buf(),
        why,
    })?;

    // Before the conversion, not after: the conversion is what builds meshlets
    // out of these positions, and a non-finite one reaching that is a worse
    // failure than a message.
    if has_non_finite_position(&imported) {
        return Err(LoadError::NonFiniteGeometry(path.to_path_buf()));
    }
    let bounds =
        world_bounds(&imported).ok_or_else(|| LoadError::NoGeometry(path.to_path_buf()))?;
    if !bounds.min.is_finite() || !bounds.max.is_finite() {
        return Err(LoadError::NonFiniteGeometry(path.to_path_buf()));
    }

    let mut render = crcbl::scene::gltf_render::build_render_scene(&imported, &key);
    // Summarised and converted here, and the imported scene dropped: see `Rig`
    // and `crate::anim`. The rig's own skips join the conversion's, because
    // they are the same kind of thing — a feature the file asked for and did
    // not get — and this application has exactly one place it reports those.
    let rig = rig_of(&imported);
    let (playable, skipped) = crate::anim::playable_of(&imported);
    render.skipped.extend(skipped);
    // After `playable_of`, and reading the same skin it does: the two are one
    // decision — what this application animates — and a document where they
    // disagreed would upload a palette for a skeleton nothing else poses.
    let mut skinned_skips = Vec::new();
    let skinned = skinned_of(&imported, &render, &mut skinned_skips);
    render.skipped.extend(skinned_skips);
    // Before the renderer is built from it, because a pool is sized once. The
    // conversion measures the description exactly and leaves no headroom, and
    // every skinned instance needs room the description does not describe.
    reserve_for_skinning(&mut render.scene.capacities, &skinned);
    Ok(Model {
        key,
        render,
        bounds,
        rig,
        playable,
        skinned,
    })
}

/// Adds what the skinning seam reserves on top of the description's own pools.
///
/// **Two mesh-table entries and two vertex regions per skinned instance**, which
/// is what `ForwardRenderer::reserve_skinned` documents it takes and what
/// `crcbl::render::SkinnedRegion` needs to hold a frame's pose beside the one
/// before it. `crcbl_scene::gltf_render` sizes both pools to exactly what the
/// description holds, so without this every document with a rig fails to
/// reserve at load — on the *second* instance of the pair, with the pool
/// reporting a number the caller has no way to connect to a skin.
///
/// Saturating rather than wrapping: a document that overflows a `u32` of
/// vertices has already been refused by the conversion, and a capacity that
/// wrapped to a small number would be a pool failure blamed on the wrong thing.
fn reserve_for_skinning(capacities: &mut crcbl::render::Capacities, skinned: &Skinned) {
    for instance in &skinned.instances {
        capacities.meshes = capacities.meshes.saturating_add(2);
        let vertices = u32::try_from(instance.bindings.len()).unwrap_or(u32::MAX);
        capacities.vertices = capacities
            .vertices
            .saturating_add(vertices.saturating_mul(2));
    }
}

/// Which of `render`'s instances the document's **first** skin deforms.
///
/// The first, because that is the skin [`crate::anim::playable_of`] poses and
/// the palette is the one thing a skinned draw cannot be given twice; a node
/// wearing any other skin is reported and drawn in its bind pose. Everything
/// else this refuses is reported the same way, because a document whose rigged
/// mesh silently draws undeformed is the failure a viewer exists to explain.
///
/// # How an instance is paired with the node that placed it
///
/// Neither [`InstanceDesc`](crcbl::render::scene::InstanceDesc) nor
/// [`MeshOrigin`](crcbl::scene::gltf_render::MeshOrigin) names a node, so the
/// walk `crcbl_scene::gltf_render::place_instances` did is repeated here:
/// placements in document order, and within each, that mesh's converted
/// primitives in primitive order. [`RenderScene::origins`] is in
/// `(mesh, primitive)` order, so the rows one placement emits are the run of
/// origins naming its mesh.
///
/// **The reconstruction checks itself rather than being trusted.** Every
/// instance carries the row it draws, so the row this walk predicts is compared
/// against the row the instance actually names; a mismatch means the
/// conversion's expansion is no longer the one described above, and this skins
/// nothing at all and says so. That is what keeps it from being a silent
/// assumption about another crate's loop.
fn skinned_of(scene: &GltfScene, render: &RenderScene, skips: &mut Vec<Skip>) -> Skinned {
    let Some(skin) = scene.skins().first() else {
        return Skinned::default();
    };
    let parents = crate::anim::parents_of(scene.nodes());
    let Some(to_world) = skeleton_to_world(scene, &parents, skin.joints(), skips) else {
        return Skinned::default();
    };

    // The converted rows each glTF mesh produced, in primitive order — the
    // half of `place_instances`' walk that lives in `origins`.
    let mut rows_of: Vec<Vec<usize>> = vec![Vec::new(); scene.meshes().len()];
    for (row, origin) in render.origins.iter().enumerate() {
        rows_of[origin.mesh].push(row);
    }

    let mut instances = Vec::new();
    let mut cursor = 0usize;
    for placement in scene.instances() {
        let node = placement.node();
        let at = || match scene.nodes()[node].name() {
            Some(name) => format!("node {node} {name:?}"),
            None => format!("node {node}"),
        };
        for &row in &rows_of[placement.mesh()] {
            let Some(desc) = render.instances.get(cursor) else {
                skips.push(Skip {
                    feature: "skin",
                    at: at(),
                    why: "this viewer ran out of instances while pairing them with the nodes \
                          that placed them, so it cannot tell which of them a skin deforms \
                          and skins none of them"
                        .to_owned(),
                });
                return Skinned::default();
            };
            if desc.mesh != row {
                skips.push(Skip {
                    feature: "skin",
                    at: at(),
                    why: format!(
                        "this viewer expected instance {cursor} to draw mesh row {row} and it \
                         draws row {}, so the walk that pairs an instance with the node that \
                         placed it no longer matches the conversion; nothing in this document \
                         is skinned",
                        desc.mesh,
                    ),
                });
                return Skinned::default();
            }
            cursor += 1;

            let Some(worn) = scene.nodes()[node].skin() else {
                continue;
            };
            if worn != 0 {
                skips.push(Skip {
                    feature: "skin",
                    at: at(),
                    why: format!(
                        "it wears skin {worn} and this viewer poses the first skin only, so it \
                         is drawn in its bind pose"
                    ),
                });
                continue;
            }
            let bindings = &render.origins[row].bindings;
            if bindings.is_empty() {
                skips.push(Skip {
                    feature: "JOINTS_0",
                    at: at(),
                    why: format!(
                        "it wears a skin and the primitive it draws (mesh {} primitive {}) \
                         carries no joint bindings, so there is nothing to deform and it is \
                         drawn in its bind pose",
                        render.origins[row].mesh, render.origins[row].primitive,
                    ),
                });
                continue;
            }
            instances.push(SkinnedInstance {
                // `cursor` was advanced past this instance above, so the row
                // this describes is the one before it.
                instance: cursor - 1,
                transform: to_world,
                bindings: bindings.clone(),
            });
        }
    }

    Skinned {
        joints: u32::try_from(skin.joints().len()).unwrap_or(u32::MAX),
        instances,
    }
}

/// Where the skeleton hangs, in world space:
/// [`SkinnedInstance::transform`].
///
/// `crcbl::anim::Palette` composes a root joint's global transform from that
/// joint's own local transform and no further, so its matrices are in the space
/// of the node the root joints are children of. This is that node's world
/// transform, which is the same matrix for every joint of the skeleton and so
/// belongs on the instance rather than folded into the palette every frame.
///
/// `None` refuses the whole document's skinning, and is a hierarchy this cannot
/// answer for: a `children` cycle, which nothing between a file and here rules
/// out — `crcbl_scene::gltf_check`'s `check_nodes` checks that a child index
/// exists, not that the graph is a forest.
///
/// A skin whose root joints hang from **different** nodes gets the first one's
/// answer and a report, because one instance carries one transform. Rigs like
/// that are rare and the alternative is refusing to draw a document over a
/// second root nobody notices.
fn skeleton_to_world(
    scene: &GltfScene,
    parents: &[Option<usize>],
    joints: &[usize],
    skips: &mut Vec<Skip>,
) -> Option<Mat4> {
    // A root of the skeleton is a joint whose parent node is not itself a joint
    // of this skin, which is exactly what `crate::anim::skeleton_of` calls a
    // root when it builds the hierarchy the palette is composed down.
    let roots = joints
        .iter()
        .filter(|&&joint| !parents[joint].is_some_and(|parent| joints.contains(&parent)));
    let mut hangs_from = None;
    for &root in roots {
        let parent = parents[root];
        match hangs_from {
            None => hangs_from = Some(parent),
            Some(first) if first != parent => {
                skips.push(Skip {
                    feature: "skin",
                    at: format!("skin 0 root joint node {root}"),
                    why: "this skin's root joints hang from different nodes and a skinned \
                          instance carries one transform, so every one of them is placed by \
                          the first root's"
                        .to_owned(),
                });
            }
            Some(_) => {}
        }
    }
    let Some(Some(parent)) = hangs_from else {
        // Either the skin has no joints at all, or its roots are children of
        // nothing — the scene's own space, which is the identity.
        return Some(Mat4::IDENTITY);
    };
    match world_transform(scene.nodes(), parents, parent) {
        Some(transform) => Some(transform),
        None => {
            skips.push(Skip {
                feature: "skin",
                at: format!("node {parent}"),
                why: "the skeleton hangs from it and its ancestors form a cycle, so there is \
                      no world transform to place the skeleton with and this document is drawn \
                      in its bind pose"
                    .to_owned(),
            });
            None
        }
    }
}

/// `node`'s world transform: its own local transform with every ancestor's
/// applied outside it.
///
/// `None` for a `children` graph that is not a forest — see
/// [`skeleton_to_world`]. The climb is bounded by the node count rather than
/// trusted to terminate, because an acyclic chain visits each node at most once
/// and this is the one application a person points at a file nobody curated.
fn world_transform(
    nodes: &[crcbl::scene::GltfNode],
    parents: &[Option<usize>],
    node: usize,
) -> Option<Mat4> {
    let mut transform = Mat4::IDENTITY;
    let mut at = Some(node);
    for _ in 0..=nodes.len() {
        let Some(index) = at else {
            return Some(transform);
        };
        transform = nodes[index].local_transform() * transform;
        at = parents[index];
    }
    None
}

/// Reads the document held in `bytes`, filed under `name`.
///
/// [`load_from`] over a one-entry [`MemorySource`], which is the shape a
/// document that never had a directory takes: the `.glb`
/// [`crate::demo_model`] generates into the browser build, and a file a
/// visitor drops on the canvas.
///
/// `name` is both the key the bytes are filed under and what an error names,
/// because bytes that never had a path have no better answer. It has to be a
/// legal asset key — the rule this module's header sets out — and a name that
/// is not one is refused here, with that rule in the message, rather than
/// rewritten into something that would load.
///
/// # Errors
///
/// [`LoadError::Storage`] for a name the asset seam refuses, and everything
/// [`load_from`] reports about the document itself. It does not panic on any
/// input, which is what lets a page hand it whatever was dropped on it.
pub fn load_bytes(name: &Path, bytes: Vec<u8>) -> Result<Model, LoadError> {
    let mut source = MemorySource::new();
    source
        .insert(name, bytes)
        .map_err(|why| LoadError::Storage {
            path: name.to_path_buf(),
            why,
        })?;
    // The name twice, for `load_from`'s reason: it is what the source is asked
    // for and what an error names.
    load_from(&source, name, name)
}

/// What [`Model::rig`] reports about `scene`.
///
/// Reads the skins and the clips and nothing else of them — the bind matrices
/// and the keyframes stay in the [`GltfScene`], which this application drops
/// once the conversion has run.
fn rig_of(scene: &GltfScene) -> Rig {
    Rig {
        joints: scene.skins().iter().map(|skin| skin.joints().len()).sum(),
        clips: scene
            .clips()
            .iter()
            .enumerate()
            .map(|(index, clip)| match clip.name() {
                Some(name) => name.to_string(),
                None => format!("(unnamed clip {index})"),
            })
            .collect(),
    }
}

/// The box around every vertex the document places, in world space, or `None`
/// for a document that places none.
///
/// Composed the only way it can be: [`GltfScene`] hands out per-primitive
/// positions in *local* space and a flattened instance list carrying each
/// drawing node's composed transform, so this is one local box per primitive,
/// each pushed through its instance's transform and unioned. The union is
/// `Aabb::from_points` over the transformed corners rather than a running
/// min/max written here, because that function is the one place in the engine
/// that answers "no points" with `None` instead of an infinite sentinel.
///
/// # It is the document's geometry, not the drawn geometry
///
/// A primitive the conversion later skips — one whose meshlet build failed — is
/// still in this box, because the mapping from a converted mesh back to the
/// primitive it came from is not something
/// [`RenderScene`] exposes. The error is
/// therefore always in the safe direction: the camera can be framed wider than
/// the drawn model, never tighter, and the skip that caused it was logged.
///
/// # The box cannot report a `NaN`, so [`has_non_finite_position`] does
///
/// `Aabb::from_points` skips a `NaN` lane rather than propagating it, so that a
/// cull box always contains its finite points instead of rejecting geometry
/// that is on screen. The box it returns is therefore finite whatever the
/// positions were, and testing it — which is what this note used to describe as
/// sufficient — cannot see a `NaN` at all. An infinity still propagates, having
/// a defined ordering, and [`load`] keeps that check for it.
///
/// So the positions are read once more before the fold. That earlier note
/// dismissed the scan as too costly for "a case that draws as a degenerate
/// triangle"; the case it was actually weighing was a box that came back finite
/// while no longer containing the model, which frames the camera on geometry
/// that is not there.
/// Whether any vertex position in the document is `NaN` or infinite.
///
/// Separate from [`world_bounds`], and reading the positions a second time,
/// because the box cannot answer this. `Aabb::from_points` skips a `NaN` lane
/// rather than propagating it — deliberately, so that a cull box always
/// contains its finite points instead of rejecting geometry that is on screen —
/// and by the time the fold has run the offending coordinate is gone. Only an
/// infinity, which orders normally, still shows up in the result.
///
/// The second pass is a load-time linear scan that stops at the first bad
/// value, against a document already fully in memory. It buys the distinction
/// between [`LoadError::NoGeometry`] and [`LoadError::NonFiniteGeometry`],
/// which folding the test into `world_bounds`' `Option` would lose.
#[must_use]
fn has_non_finite_position(scene: &GltfScene) -> bool {
    scene.meshes().iter().any(|mesh| {
        mesh.primitives().iter().any(|primitive| {
            primitive
                .positions()
                .iter()
                .flatten()
                .any(|value| !value.is_finite())
        })
    })
}

#[must_use]
pub fn world_bounds(scene: &GltfScene) -> Option<Aabb> {
    let mut corners = Vec::new();
    for instance in scene.instances() {
        let transform = Mat4::from_cols_array(&instance.transform());
        for primitive in scene.meshes()[instance.mesh()].primitives() {
            let local = Aabb::from_points(
                primitive
                    .positions()
                    .iter()
                    .map(|&[x, y, z]| Vec3::new(x, y, z)),
            );
            if let Some(local) = local {
                let placed = local.transformed(transform);
                corners.push(placed.min);
                corners.push(placed.max);
            }
        }
    }
    Aabb::from_points(corners)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;

    /// A temporary directory with `document` written into it under `name`.
    fn written(name: &str, document: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join(name);
        std::fs::write(&path, document).expect("the document");
        (dir, path)
    }

    /// **The whole path works on a real file through the real seam**, and the
    /// bounds that come out are the ones the document's own hierarchy puts the
    /// geometry at.
    ///
    /// The fixture's quad is one metre across at the origin and its node
    /// translates it, so a bounds computation that dropped the transform, or
    /// that read the positions without it, lands on a box centred somewhere
    /// else — which is the failure a test on the *size* alone would miss.
    #[test]
    fn a_glb_on_disk_loads_and_its_bounds_are_where_its_hierarchy_puts_it() {
        let (_dir, path) = written("panel.glb", &fixture::quad_glb(fixture::QUAD_CENTRE));
        let model = load(&path).expect("the fixture loads");

        assert_eq!(model.key, Path::new("panel.glb"));
        assert_eq!(model.skipped(), [], "nothing about the fixture is skipped");
        assert_eq!(model.render.instances.len(), 1, "one node, one primitive");

        let centre = model.bounds.center();
        assert!(
            (centre - fixture::QUAD_CENTRE).length() < 1e-5,
            "the quad's centre is at {centre:?}, not at {:?}",
            fixture::QUAD_CENTRE,
        );
        // A one-metre quad in the XY plane: half a metre either way, and flat.
        let half = model.bounds.half_extent();
        assert!(
            (half.x - 0.5).abs() < 1e-5 && (half.y - 0.5).abs() < 1e-5,
            "{half:?}"
        );
        assert!(half.z.abs() < 1e-5, "the quad is flat in Z: {half:?}");
    }

    /// A path that names no file at all, before anything touches the disk.
    #[test]
    fn a_path_with_no_file_name_is_refused_rather_than_read() {
        let error = load(Path::new("..")).expect_err("`..` is not a document");
        assert!(matches!(error, LoadError::NotAFile(_)), "{error}");
        assert!(error.to_string().contains(".glb"), "{error}");
    }

    /// **Every way a file can be wrong is a message, and none of them is a
    /// panic.** One case per shape, because they fail at four different points
    /// of `load` and a single bad file would only ever reach the first.
    #[test]
    fn a_missing_a_misnamed_and_a_corrupt_document_are_all_refused() {
        let (dir, absent) = written("present.glb", b"unused");
        let missing = dir.path().join("absent.glb");
        let error = load(&missing).expect_err("a missing file is not a document");
        assert!(
            matches!(&error, LoadError::Storage { why, .. } if matches!(why, StorageError::NotFound(_))),
            "{error}",
        );
        drop(absent);

        // A name a filesystem is happy with and the asset seam is not. The
        // message has to say what is wrong with the *name*, or a user renames
        // nothing and tries again.
        let (_dir, spaced) = written("my model.glb", &fixture::quad_glb(Vec3::ZERO));
        let error = load(&spaced).expect_err("a space is not a key byte");
        assert!(
            matches!(&error, LoadError::Storage { why, .. } if matches!(why, StorageError::InvalidPath(_))),
            "{error}",
        );
        assert!(error.to_string().contains("legal asset key"), "{error}");

        // A `.glb` whose container is truncated: the importer's own refusal,
        // carried through rather than unwrapped.
        let mut truncated = fixture::quad_glb(Vec3::ZERO);
        truncated.truncate(8);
        let (_dir, path) = written("truncated.glb", &truncated);
        let error = load(&path).expect_err("eight bytes is not a document");
        assert!(matches!(error, LoadError::Storage { .. }), "{error}");
    }

    /// A document with a scene that draws nothing has no box to frame, and
    /// says so instead of handing the camera an empty one.
    #[test]
    fn a_document_with_no_geometry_is_refused_by_name() {
        let (_dir, path) = written("empty.glb", &fixture::empty_glb());
        let error = load(&path).expect_err("an empty scene cannot be framed");
        assert!(matches!(error, LoadError::NoGeometry(_)), "{error}");
    }

    /// **A non-finite position is refused before it reaches the camera.**
    ///
    /// The case nothing else in the workspace guards: `gltf_check` validates
    /// every index and every span and never looks at a float's value, so this
    /// document imports cleanly and `OrbitCamera::frame` asserts on the bounds
    /// it produces. Reached through `load`, not through `world_bounds` alone,
    /// because the claim is that the check is *in the path a file takes*.
    #[test]
    fn a_non_finite_vertex_position_is_refused_before_the_camera_sees_it() {
        let (_dir, path) = written("infinite.glb", &fixture::non_finite_glb());
        let error = load(&path).expect_err("an infinite corner has no bounding box");
        assert!(matches!(error, LoadError::NonFiniteGeometry(_)), "{error}");
        assert!(error.to_string().contains("NaN"), "{error}");
    }

    /// **A `NaN` corner is refused, and the box is not what notices it.**
    ///
    /// Its neighbour above uses an infinity, which survives the fold on
    /// ordering and so reaches `load`'s finite check. A `NaN` never does:
    /// `Aabb::from_points` skips that lane so the cull box keeps containing its
    /// finite points, and what comes out is finite no matter what went in. This
    /// is the test that the extra position scan exists and runs — remove
    /// `has_non_finite_position` from `load` and only this goes red.
    #[test]
    fn a_nan_vertex_position_is_refused_though_the_box_comes_out_finite() {
        let (_dir, path) = written("nan.glb", &fixture::nan_glb());
        let error = load(&path).expect_err("a NaN corner has no bounding box");
        assert!(matches!(error, LoadError::NonFiniteGeometry(_)), "{error}");
    }

    /// **A document with no rig reports none, and says so as an absence rather
    /// than as a zero.**
    ///
    /// The overwhelming majority of files are this one, so it is the case
    /// `crate::listing` draws most often — and `Rig::is_empty` is what it asks.
    /// A summary that came back with a phantom joint or a phantom clip would
    /// put a rig on the panel of every file anyone opens.
    #[test]
    fn a_document_with_no_skin_and_no_animation_reports_no_rig() {
        let (_dir, path) = written("plain.glb", &fixture::quad_glb(Vec3::ZERO));
        let model = load(&path).expect("the fixture loads");

        assert_eq!(model.rig, Rig::default());
        assert!(model.rig.is_empty());
        assert!(
            model.playable.is_none(),
            "there is no skeleton here to pose",
        );
    }

    /// **A rigged document arrives with its rig converted**, which is the link
    /// between this module and the player in [`crate::app`]: `crate::anim` says
    /// the conversion is right, and this says the loader ran it and kept the
    /// result.
    #[test]
    fn a_rigged_document_arrives_with_a_skeleton_and_a_clip_to_play() {
        let (_dir, path) = written("demo.glb", &crate::demo_model::demo_glb());
        let model = load(&path).expect("the demo document loads");

        let playable = model
            .playable
            .as_ref()
            .expect("the demo document is rigged");
        assert_eq!(
            playable.skeleton().len(),
            model.rig.joints,
            "the skeleton and the panel's count are the same skin",
        );
        assert!(
            playable.clip().duration() > 0.0,
            "the document's clip arrived with no keyframes in it",
        );
        assert_eq!(model.skipped(), [], "and nothing was lost converting it");
    }

    /// **A rig this application cannot pose is reported the way a lost texture
    /// is** — on the skip list `load_and_report` prints and the listing panel
    /// draws — and the document still loads and still draws.
    ///
    /// The fixture's joints are listed child-first, which `Skeleton::new`
    /// refuses; see [`crate::anim`] for why that is refused rather than sorted
    /// out. Without this, the refusal would be a `None` nobody is told about.
    #[test]
    fn a_rig_that_cannot_be_posed_is_on_the_skip_list_and_the_document_loads() {
        let (_dir, path) = written("reversed.glb", &fixture::skinned_glb(false));
        let model = load(&path).expect("a document whose rig will not pose still draws");

        assert!(model.playable.is_none());
        assert!(
            model.skipped().iter().any(|skip| skip.feature == "joints"),
            "the refusal never reached the user: {:?}",
            model.skipped(),
        );
        assert_eq!(
            model.rig.joints, 2,
            "the panel still reports the rig the file declares",
        );
    }

    /// **An animation with no name is still named on the panel**, by the
    /// stand-in [`Rig::clips`] documents.
    ///
    /// glTF does not require an `animations` entry to carry a name, so this is
    /// an ordinary document rather than a broken one, and the alternative — an
    /// empty string in the list — draws a blank row that reads as a panel bug.
    /// The index is in the stand-in so that a file with several unnamed clips
    /// does not draw one name several times.
    ///
    /// This document has a clip and no skin, so it is also the case that says
    /// `Rig::is_empty` asks about both: a rig summary that only looked at the
    /// joints would call this one empty and lose the clip.
    #[test]
    fn an_animation_with_no_name_is_reported_by_its_place_in_the_file() {
        let (_dir, path) = written("clip.glb", &fixture::unnamed_clip_glb());
        let model = load(&path).expect("the fixture loads");

        assert_eq!(model.rig.joints, 0, "this document rigs nothing");
        assert_eq!(model.rig.clips, ["(unnamed clip 0)"]);
        assert!(
            !model.rig.is_empty(),
            "a document with a clip and no skin still has something to report",
        );
    }

    /// **Bytes with no directory behind them load exactly as a file does.**
    /// This is the call the browser's generated document takes and the one a
    /// file dropped on the canvas takes, and neither of those can be run on the
    /// machine that builds them — so what holds it is this, against a fixture
    /// whose numbers `a_glb_on_disk_loads_and_its_bounds_are_where_its_hierarchy_puts_it`
    /// asserts through the other door.
    #[test]
    fn a_document_that_never_had_a_directory_loads_from_its_bytes() {
        let model = load_bytes(
            Path::new("panel.glb"),
            fixture::quad_glb(fixture::QUAD_CENTRE),
        )
        .expect("the fixture loads out of memory as it does off a disk");

        assert_eq!(model.key, Path::new("panel.glb"));
        assert_eq!(model.render.instances.len(), 1, "one node, one primitive");
        let centre = model.bounds.center();
        assert!(
            (centre - fixture::QUAD_CENTRE).length() < 1e-5,
            "the quad's centre is at {centre:?}, not at {:?}",
            fixture::QUAD_CENTRE,
        );
    }

    /// **A name a person's filesystem allows and this seam does not is refused
    /// with the rule in it**, rather than rewritten into something that would
    /// load. It is the message a visitor who drops `My Model.glb` reads, and
    /// the one thing about that drop the browser cannot be asked about here.
    #[test]
    fn a_name_that_is_not_a_legal_asset_key_is_refused_by_its_name() {
        let error = load_bytes(
            Path::new("My Model.glb"),
            fixture::quad_glb(fixture::QUAD_CENTRE),
        )
        .expect_err("a space is not a legal asset key");

        assert!(
            matches!(
                &error,
                LoadError::Storage {
                    why: StorageError::InvalidPath(_),
                    ..
                }
            ),
            "{error:?}",
        );
        let said = error.to_string();
        assert!(
            said.contains("My Model.glb") && said.contains("legal asset key"),
            "the message names the file and the rule: {said}",
        );
    }

    /// **Bytes that are not a document come back as a sentence, not a panic.**
    /// The whole claim the drop target makes to a visitor: whatever lands on
    /// the canvas, the page keeps drawing and says what was wrong with it.
    #[test]
    fn bytes_that_are_not_a_document_are_reported_and_not_a_panic() {
        let error = load_bytes(Path::new("dropped.glb"), b"not a glTF file".to_vec())
            .expect_err("fifteen bytes of prose are not a document");

        let said = error.to_string();
        assert!(
            said.contains("dropped.glb"),
            "the message names what was dropped: {said}",
        );
    }
}
