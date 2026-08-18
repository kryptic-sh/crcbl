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

use crcbl::assets::DirSource;
use crcbl::math::{Mat4, Vec3};
use crcbl::render::cull::Aabb;
use crcbl::scene::GltfScene;
use crcbl::scene::gltf_render::{RenderScene, Skip};
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
    /// Refused rather than drawn, because the camera cannot be framed on it —
    /// see the [module docs](self) and [`world_bounds`], which is also where
    /// the one non-finite position this does *not* catch is written down.
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

    let source = DirSource::at(root);
    let imported = crcbl::scene::import_gltf(&source, &key).map_err(|why| LoadError::Storage {
        path: path.to_path_buf(),
        why,
    })?;

    // Before the conversion, not after: the conversion is what builds meshlets
    // out of these positions, and a non-finite one reaching that is a worse
    // failure than a message.
    let bounds =
        world_bounds(&imported).ok_or_else(|| LoadError::NoGeometry(path.to_path_buf()))?;
    if !bounds.min.is_finite() || !bounds.max.is_finite() {
        return Err(LoadError::NonFiniteGeometry(path.to_path_buf()));
    }

    let render = crcbl::scene::gltf_render::build_render_scene(&imported, &key);
    Ok(Model {
        key,
        render,
        bounds,
    })
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
/// # A lone `NaN` corner is absorbed, and that is `Aabb`'s behaviour
///
/// `Aabb::from_points` folds points in with `f32::min` and `f32::max`, which
/// both return the *other* operand when one side is `NaN`. So a single `NaN`
/// coordinate among finite ones disappears and the box that comes out is the
/// finite one — while an infinity, or a mesh whose every position is `NaN`,
/// propagates and is what [`load`] refuses. That is the right split for this
/// application either way: a box that is finite is a box the camera can be
/// framed on, and the odd degenerate vertex behind it draws as a degenerate
/// triangle rather than taking the process down.
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
}
