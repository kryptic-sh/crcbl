//! glTF 2.0 import: bytes through [`AssetSource`], geometry, material factors
//! and encoded images out.
//!
//! This is the first half of step 3 of `docs/plan/06-assets-scenes.md`. It ends
//! at host memory: there is no GPU pool upload, no image *decode* and no mip
//! generation here, and the types below are what the upload step consumes.
//! [`crate::gltf_render`] is that step for the forward renderer.
//!
//! # Everything arrives through the asset seam
//!
//! [`import_gltf`] never touches `std::fs`. The document and every external
//! `.bin` it names are read with [`AssetSource::read`], which is defined not to
//! block — so a browser source that answers
//! [`StorageError::Pending`] makes the whole import answer `Pending`, and the
//! caller retries next frame. That is the plan's "no synchronous IO anywhere in
//! engine crates" exit criterion, and it is also why the `gltf` crate's own
//! `import` feature is off: it reads files itself.
//!
//! Buffer URIs resolve **relative to the document's own key**, and go through
//! the source, so the same key rule applies to them as to everything else — a
//! `.bin` outside the asset root is refused rather than read, and a `data:` URI
//! is refused rather than decoded (see [`import_gltf`]).
//!
//! # Materials are the shader's own record
//!
//! [`GltfScene::materials`] is `[GpuMaterial]` — [`crcbl_shaders::mesh`]'s
//! material-table row, not a copy of it. glTF's `pbrMetallicRoughness.
//! baseColorFactor` is defined by the spec as **linear** RGBA, and
//! [`GpuMaterial::base_color`] is documented as linear RGBA, so the mapping is
//! an assignment with no conversion: both are the factor a shader multiplies
//! into albedo before any tonemap or sRGB encode. The same accessor's
//! `metallicFactor` and `roughnessFactor` go straight into
//! [`GpuMaterial::metallic`] and [`GpuMaterial::roughness`], which are the same
//! two numbers under the same names — `mesh.slang` shades with the GGX lobe
//! glTF's model is written for.
//!
//! **An imported default material is therefore not [`GpuMaterial::UNTINTED`].**
//! It was while the row held a factor alone, because glTF's missing-factor
//! default is `[1.0; 4]` and so is that row's. The two shading factors do not
//! agree: glTF defaults a material to `metallic 1.0, roughness 1.0`, which is a
//! fully rough conductor, where [`GpuMaterial::UNTINTED`] is a dielectric with a
//! soft highlight. The importer reports what the document says rather than what
//! the engine's own neutral row happens to be — a document that means "plastic"
//! writes the factors down, and one that does not means what the specification
//! says it means.
//!
//! The row also has a `base_color_texture` column, and **this importer leaves
//! it at the untextured layer**. That column is a layer of the renderer's page,
//! and which layer an image lands in is decided by whoever builds the page —
//! [`crate::gltf_render`], which owns both. What this module supplies instead is
//! the link the page builder needs: [`GltfScene::base_color_textures`] says
//! which of [`GltfScene::images`] each material's `baseColorTexture` names, and
//! [`GltfScene::images`] carries that image's **encoded** bytes.
//!
//! `metallicRoughnessTexture`, `normalTexture`, `occlusionTexture` and
//! `emissiveTexture` are not extracted. Not an oversight and not a decode
//! question: [`GpuMaterial`] has one texture column, so there is nowhere for a
//! second map to go — see `docs/backlog.md`'s "The material table has both
//! halves". The one whose absence is *visible* is the metallic-roughness map: a
//! document that varies gloss over a surface arrives with its factor applied
//! flat across it.
//!
//! # An image that will not resolve is skipped, where a buffer that will not is
//! refused
//!
//! [`GltfImage::bytes`] is a `Result`, and a document whose image is missing,
//! outside the asset root or embedded in a `data:` URI still imports — with that
//! image carrying the reason instead of the bytes, and a warning naming the file
//! and the image. A buffer in the same state is an error that fails the whole
//! import. The asymmetry is deliberate and it is about what is lost: a buffer is
//! the geometry, so a file without it has nothing to draw, where an image is a
//! surface's colour and a file without it draws the surface white. The viewer's
//! own exit criterion — load the sample suite, log what could not be used — is
//! the second of those, not the first.
//!
//! # The node table is the whole node array, not the scene graph
//!
//! [`GltfScene::instances`] is the *drawable* half — nodes reachable from the
//! scene, with their transforms composed — and [`GltfScene::nodes`] is the
//! document's `nodes` array in file order, whether a scene reaches them or not.
//! Both are needed and neither subsumes the other: `MSFT_lod` names its lower
//! levels by node index and those nodes are deliberately kept out of every
//! scene, so a level that only the instances knew about would be a level that
//! vanished. See [`GltfNode::lod_nodes`] and [`crate::lod_resolve`].
//!
//! # What is parsed and dropped
//!
//! Skins and animations are in the format and are not read: the plan has them
//! "parsed but unused until the animation feature lands", and a type nothing
//! fills is worse than no type. Vertex colours, tangents, `TEXCOORD_1` and
//! morph targets are read by nothing here and so are not extracted.
//!
//! **Each of those is warned about when a document actually uses it**, naming
//! the file and the feature, rather than being silently absent from the result.
//! A viewer that shows a rigged character in its bind pose has to be able to say
//! so, and the only place that knows the document had a skin at all is here.
//!
//! `MSFT_lod` is the one extension read, and only where it sits on a **node**.
//! The extension is also defined on materials — a material chain for a mesh
//! that keeps its geometry — and nothing here shades at two levels of detail,
//! so a material's copy is left alone rather than parsed into a field no
//! caller could use.
//!
//! [`AssetSource`]: crcbl_assets::AssetSource
//! [`AssetSource::read`]: crcbl_assets::AssetSource::read
//! [`GpuMaterial::base_color`]: crcbl_shaders::mesh::GpuMaterial::base_color
//! [`GpuMaterial::metallic`]: crcbl_shaders::mesh::GpuMaterial::metallic
//! [`GpuMaterial::roughness`]: crcbl_shaders::mesh::GpuMaterial::roughness
//! [`GpuMaterial::UNTINTED`]: crcbl_shaders::mesh::GpuMaterial::UNTINTED

use std::path::Path;

use crcbl_assets::{AssetSource, StorageError};
use crcbl_shaders::mesh::GpuMaterial;
use glam::Mat4;
use gltf::mesh::Mode;

use crate::gltf_check::{check_document, check_glb_header, malformed};

/// One glTF document, parsed.
///
/// The three arrays index each other: [`GltfInstance::mesh`] indexes
/// [`GltfScene::meshes`] and [`GltfPrimitive::material`] indexes
/// [`GltfScene::materials`]. Both hold because [`import_gltf`] is the only way
/// to make one and it checks every index in the file before building anything.
#[derive(Clone, Debug, PartialEq)]
pub struct GltfScene {
    meshes: Vec<GltfMesh>,
    materials: Vec<GpuMaterial>,
    base_color_textures: Vec<Option<GltfTexture>>,
    images: Vec<GltfImage>,
    nodes: Vec<GltfNode>,
    instances: Vec<GltfInstance>,
}

impl GltfScene {
    /// The document's meshes, in file order.
    #[inline]
    #[must_use]
    pub fn meshes(&self) -> &[GltfMesh] {
        &self.meshes
    }

    /// The document's materials, in file order.
    ///
    /// A [`GpuMaterial`] each, which is the whole of a glTF material that
    /// anything in this engine can currently consume — see the [module
    /// docs](self) for the colour-space argument.
    #[inline]
    #[must_use]
    pub fn materials(&self) -> &[GpuMaterial] {
        &self.materials
    }

    /// Which image each material's `baseColorTexture` names, **parallel to
    /// [`materials`](Self::materials)**: entry `n` belongs to material `n`.
    ///
    /// `None` where the material names no base-colour texture, which is every
    /// material of an untextured document. Every [`GltfTexture::image`] is a
    /// valid index into [`images`](Self::images) — one that is not makes the
    /// document malformed rather than making this array shorter.
    ///
    /// A second array rather than a field on the row because the row is
    /// [`GpuMaterial`], the shader's own record, and its
    /// `base_color_texture` column is a page layer — a number this module
    /// cannot know. See the [module docs](self).
    #[inline]
    #[must_use]
    pub fn base_color_textures(&self) -> &[Option<GltfTexture>] {
        &self.base_color_textures
    }

    /// The document's `images` array, in file order, each still encoded.
    #[inline]
    #[must_use]
    pub fn images(&self) -> &[GltfImage] {
        &self.images
    }

    /// The document's `nodes` array, in file order — every node, not only the
    /// ones a scene reaches.
    ///
    /// [`GltfInstance::node`] indexes this. See the [module docs](self) for why
    /// the unreachable ones are kept.
    #[inline]
    #[must_use]
    pub fn nodes(&self) -> &[GltfNode] {
        &self.nodes
    }

    /// The node hierarchy, flattened: one entry per node that draws a mesh.
    ///
    /// Empty when the document has no scenes, which is legal glTF and means a
    /// file of meshes with no arrangement of them.
    #[inline]
    #[must_use]
    pub fn instances(&self) -> &[GltfInstance] {
        &self.instances
    }
}

/// One entry of the document's `nodes` array: what it is called, what it draws,
/// and the lower detail levels it declares.
///
/// The transform is deliberately absent. A node's own TRS is only half of where
/// it ends up — the other half is every parent above it — and that composition
/// is what [`GltfScene::instances`] is. A per-node local transform here would
/// be a second, unconstructed answer to "where is this".
#[derive(Clone, Debug, PartialEq)]
pub struct GltfNode {
    name: Option<String>,
    mesh: Option<usize>,
    lod_nodes: Vec<usize>,
}

impl GltfNode {
    /// The name the document gave this node, if it gave one.
    ///
    /// The `name_LOD1` half of `docs/plan/25-lod.md`'s hand-authored precedence
    /// reads exactly this; [`crate::lod_resolve`] is where the convention is
    /// spelled out.
    #[inline]
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Which of [`GltfScene::meshes`] this node draws, if it draws one.
    #[inline]
    #[must_use]
    pub const fn mesh(&self) -> Option<usize> {
        self.mesh
    }

    /// The `MSFT_lod` extension's `ids`: nodes carrying this node's lower
    /// detail levels, LOD1 first.
    ///
    /// Empty when the node declares no `MSFT_lod`, which is almost every node.
    /// Every entry is a valid index into [`GltfScene::nodes`] — one that is not
    /// makes the document malformed rather than making this array shorter.
    ///
    /// This is the declaration only. Whether those nodes *are* the mesh's
    /// levels, and what happens where they disagree with the naming
    /// convention, is [`crate::lod_resolve`]'s question.
    #[inline]
    #[must_use]
    pub fn lod_nodes(&self) -> &[usize] {
        &self.lod_nodes
    }
}

/// A material's reference to one of [`GltfScene::images`], and which UV set it
/// samples with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GltfTexture {
    image: usize,
    tex_coord: u32,
}

impl GltfTexture {
    /// Which of [`GltfScene::images`] carries the bytes.
    ///
    /// Several materials naming one image is ordinary and is not deduplicated
    /// here: this is the document's own index, so whoever packs a page can see
    /// that two rows want the same layer.
    #[inline]
    #[must_use]
    pub const fn image(&self) -> usize {
        self.image
    }

    /// The `TEXCOORD_n` set this texture is sampled with — the glTF `texCoord`
    /// field, which defaults to `0`.
    ///
    /// **Anything but `0` is a set this importer does not read.**
    /// [`GltfPrimitive::tex_coords`] is `TEXCOORD_0` alone, so a material asking
    /// for set 1 has no coordinates to sample with and whoever consumes this has
    /// to say so rather than sample the wrong ones. It is reported instead of
    /// refused because the rest of the material — its factors, and its geometry
    /// — is perfectly usable.
    #[inline]
    #[must_use]
    pub const fn tex_coord(&self) -> u32 {
        self.tex_coord
    }
}

/// One entry of the document's `images` array: what it is called, what it is
/// encoded as, and either the encoded bytes or why they are not here.
///
/// **Encoded, not decoded.** These are the PNG or JPEG bytes exactly as the file
/// carries them, whether they came out of a `bufferView` of a `.glb` or a file
/// beside a `.gltf`. Turning them into texels is the page builder's job — see
/// the [module docs](self) for why the decode is not here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GltfImage {
    name: Option<String>,
    mime: Option<String>,
    bytes: Result<Vec<u8>, String>,
}

impl GltfImage {
    /// The name the document gave this image, if it gave one.
    #[inline]
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The `mimeType` the document declared, if it declared one.
    ///
    /// Required by the specification for an image in a `bufferView` and optional
    /// for one in a URI, so this is `None` only for the second kind. **It is a
    /// claim, not a fact**: a decoder should recognise the bytes it was given
    /// and use this to say what the file *said* when it cannot.
    #[inline]
    #[must_use]
    pub fn mime(&self) -> Option<&str> {
        self.mime.as_deref()
    }

    /// The encoded bytes, or the reason the import could not get them.
    ///
    /// The `Err` is a sentence naming what went wrong — a `data:` URI, a key
    /// outside the asset root, a file the source does not have. It has already
    /// been logged at warning level with the document's key; it is returned as
    /// well so a tool can show it beside the image it belongs to.
    ///
    /// # Errors
    ///
    /// Never fails at call time: the `Result` is stored, not computed.
    #[inline]
    pub fn bytes(&self) -> Result<&[u8], &str> {
        match &self.bytes {
            Ok(bytes) => Ok(bytes),
            Err(why) => Err(why),
        }
    }
}

/// A glTF mesh: a name, and the primitives it draws.
#[derive(Clone, Debug, PartialEq)]
pub struct GltfMesh {
    name: Option<String>,
    primitives: Vec<GltfPrimitive>,
}

impl GltfMesh {
    /// The name the document gave this mesh, if it gave one.
    #[inline]
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The mesh's primitives.
    ///
    /// Can be empty: a mesh whose every primitive was skipped for being
    /// something other than a triangle list keeps its entry, so an instance
    /// naming it still resolves.
    #[inline]
    #[must_use]
    pub fn primitives(&self) -> &[GltfPrimitive] {
        &self.primitives
    }
}

/// One triangle list: its vertex attributes, its indices, and its material.
///
/// [`positions`](GltfPrimitive::positions) and
/// [`indices`](GltfPrimitive::indices) are always present — a primitive without
/// `POSITION` is refused, and one without an index accessor is given the
/// trivial `0..vertex_count` so that every primitive here is indexed and the
/// GPU path has one case rather than two.
/// [`normals`](GltfPrimitive::normals) and
/// [`tex_coords`](GltfPrimitive::tex_coords) are empty when the file has none,
/// and otherwise have exactly as many entries as `positions`.
#[derive(Clone, Debug, PartialEq)]
pub struct GltfPrimitive {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tex_coords: Vec<[f32; 2]>,
    indices: Vec<u32>,
    material: Option<usize>,
}

impl GltfPrimitive {
    /// Vertex positions, in the document's own coordinate system.
    #[inline]
    #[must_use]
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// Vertex normals, or empty if the file has none.
    #[inline]
    #[must_use]
    pub fn normals(&self) -> &[[f32; 3]] {
        &self.normals
    }

    /// `TEXCOORD_0`, or empty if the file has none.
    ///
    /// Normalised to `f32` whichever of the three legal component types the
    /// file used.
    #[inline]
    #[must_use]
    pub fn tex_coords(&self) -> &[[f32; 2]] {
        &self.tex_coords
    }

    /// Triangle indices. Every one is less than `positions().len()`.
    #[inline]
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Which of [`GltfScene::materials`] to shade with, if the primitive names
    /// one.
    ///
    /// `None` means the glTF default material — an untinted, fully rough
    /// conductor, which is *not* [`GpuMaterial::UNTINTED`]; see the [module
    /// docs](self). It is not substituted for a real index either way, because a
    /// table row nothing wrote is black and the caller has to decide what to put
    /// there.
    ///
    /// [`GpuMaterial::UNTINTED`]: crcbl_shaders::mesh::GpuMaterial::UNTINTED
    #[inline]
    #[must_use]
    pub const fn material(&self) -> Option<usize> {
        self.material
    }
}

/// One node of the hierarchy that draws a mesh, with its transform composed
/// from the root.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GltfInstance {
    node: usize,
    mesh: usize,
    transform: [f32; 16],
}

impl GltfInstance {
    /// Which of [`GltfScene::nodes`] this instance came from.
    ///
    /// The way back from a drawn thing to what the document called it — and so
    /// the argument [`crate::lod_resolve::resolve_lod`] takes.
    #[inline]
    #[must_use]
    pub const fn node(&self) -> usize {
        self.node
    }

    /// Which of [`GltfScene::meshes`] this node draws.
    #[inline]
    #[must_use]
    pub const fn mesh(&self) -> usize {
        self.mesh
    }

    /// Model → world, column-major, the way `glam::Mat4::to_cols_array`
    /// produces it — which is the layout
    /// [`crcbl_shaders::mesh::GpuInstance::transform`] holds.
    ///
    /// **Scale is preserved, including a non-uniform one.** That field takes
    /// any affine matrix — the mesh shaders build the normal transform out of it
    /// rather than assuming its 3×3 is orthonormal — so a scaled node needs
    /// neither baking into the vertices nor a decision at the upload step. What
    /// a scaled node still costs is in `crcbl_scene::gltf_render`, which reports
    /// it: the per-cluster back-face cull has not learned the same lesson.
    #[inline]
    #[must_use]
    pub const fn transform(&self) -> [f32; 16] {
        self.transform
    }
}

/// Import the glTF (`.gltf`) or binary glTF (`.glb`) document at `key`.
///
/// ```
/// # use crcbl_assets::{AssetSource, DirSource, StorageError};
/// # use std::path::Path;
/// # fn load(source: &DirSource) -> Result<(), StorageError> {
/// let scene = crcbl_scene::import_gltf(source, Path::new("meshes/crate.glb"))?;
/// for instance in scene.instances() {
///     let mesh = &scene.meshes()[instance.mesh()];
///     println!("{:?} at {:?}", mesh.name(), instance.transform());
/// }
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// - [`StorageError::Pending`] — the document, or one of its buffers, is not
///   resident yet. Not a failure: poll again. The import restarts from the top
///   when it is, because nothing here holds state between calls.
/// - [`StorageError::NotFound`] / [`StorageError::InvalidPath`] — from the
///   source, for the document or for a buffer URI. A URI that escapes the asset
///   root, or that is percent-encoded, or that names a Windows path, is an
///   invalid key and is refused rather than resolved.
/// - [`StorageError::Unsupported`] — a `data:` URI buffer or a sparse accessor.
///   Both are legal glTF this importer does not read; see
///   [`crate::gltf_check`] and `docs/backlog.md`.
/// - [`StorageError::Other`] — the file is malformed, with the reason and the
///   key. Every structural defect lands here: a truncated `.glb`, a chunk
///   length that overruns, a buffer view outside its buffer, an accessor whose
///   span overflows, a missing `POSITION`, an index past the end of its own
///   vertex array, a node hierarchy that is not a tree.
///
/// # Panics
///
/// It does not, on any input. That is the point of
/// [`crate::gltf_check`]: `gltf`'s typed API is full of `unwrap`s and
/// `debug_assert`s reachable from file contents, and everything they assume is
/// checked before they are called.
pub fn import_gltf(source: &dyn AssetSource, key: &Path) -> Result<GltfScene, StorageError> {
    let bytes = source.read(key)?;
    let mut document = parse(&bytes, key)?;
    let blob = document.blob.take();
    let buffers = resolve_buffers(source, key, &document, blob)?;
    check_document(document.document.as_json(), &buffers, key)?;
    build(source, &document.document, &buffers, key)
}

/// Deserialize without `gltf`'s validation, which cannot be used — see
/// [`crate::gltf_check`].
///
/// # A document is not refused over an animation this importer discards
///
/// `gltf_json::animation::Target` makes `node` a required field, and
/// `KHR_animation_pointer` replaces it with a pointer into the document — so a
/// file using that extension fails to **deserialize at all**, and the failure
/// takes the whole `Root` with it. `AnimatedColorsCube` from the Khronos sample
/// suite is one, and it lists nothing in `extensionsRequired`, so the
/// specification says it has to load.
///
/// It has to load here in particular, because [`warn_unsupported_extensions`]
/// already
/// skips every animation in every document and logs a line saying so. Refusing
/// a file over a feature this code had decided to ignore is the shape of defect
/// where a guard is present, correct, and ordered so it can never run.
///
/// So a parse failure gets one retry with the `animations` array removed — see
/// [`parse_without_animations`] — and the original error is what a caller hears
/// if that fails too, because it is the accurate one.
fn parse(bytes: &[u8], key: &Path) -> Result<gltf::Gltf, StorageError> {
    // The same test `from_slice_without_validation` makes to decide whether
    // this is a container or bare JSON.
    if bytes.starts_with(b"glTF") {
        check_glb_header(bytes, key)?;
    }
    match gltf::Gltf::from_slice_without_validation(bytes) {
        Ok(document) => Ok(document),
        Err(error) => parse_without_animations(bytes, key).ok_or_else(|| malformed(key, error)),
    }
}

/// [`parse`]'s retry: the same document with its `animations` array removed.
///
/// `None` for a document that is malformed for any other reason, so the caller
/// reports the error from the *first* attempt rather than one about a document
/// this function has already altered.
///
/// **Only `animations` is dropped, and only because nothing reads it.**
/// `warn_unsupported_extensions` skips them for every document already, so
/// removing the
/// array loses nothing a successful parse would have kept — which is what makes
/// this a repair rather than a way of forcing a file through. No other array
/// has that property, so no other array is touched.
fn parse_without_animations(bytes: &[u8], key: &Path) -> Option<gltf::Gltf> {
    // `check_glb_header` has already run in `parse`, so `Glb::from_slice`
    // cannot reach the subtraction overflow `crate::gltf_check` documents.
    let (json, blob) = if bytes.starts_with(b"glTF") {
        let gltf::binary::Glb { json, bin, .. } = gltf::binary::Glb::from_slice(bytes).ok()?;
        (json.into_owned(), bin.map(std::borrow::Cow::into_owned))
    } else {
        (bytes.to_vec(), None)
    };

    let mut value: gltf::json::Value = gltf::json::deserialize::from_slice(&json).ok()?;
    let dropped = value.as_object_mut()?.remove("animations")?;
    let count = dropped.as_array().map_or(0, Vec::len);
    // Nothing to repair: the document has no animations and failed for some
    // other reason, so the first error is the one worth reporting.
    if count == 0 {
        return None;
    }
    let root: gltf::json::Root = gltf::json::deserialize::from_value(value).ok()?;

    // The file, the feature and the reason — `docs/plan/sample/05-viewer.md`'s
    // exit criterion for a document that does not arrive whole. Louder than
    // `warn_unsupported_extensions`'s line about animations because this one has
    // also
    // lost the `animations` array itself, so nothing downstream can count them.
    crcbl_core::log::warn!(
        "{}: dropping {count} animation(s) this importer cannot deserialize — the document \
         uses an extension that changes an animation channel's shape, and every animation is \
         skipped anyway, so the rest of the document is loaded without them",
        key.display(),
    );
    Some(gltf::Gltf {
        document: gltf::Document::from_json_without_validation(root),
        blob,
    })
}

/// The bytes of every buffer the document names, in order.
///
/// A buffer with no `uri` is the `.glb` `BIN` chunk, which the spec allows only
/// for the first buffer of a binary document; anything else is refused rather
/// than aliased.
/// The directory part of an asset key, in URI space.
///
/// **Not `Path::parent`.** A key is a URI-relative reference — the same string
/// a browser would fetch — and `crcbl_store::web::canonical_key` refuses a
/// backslash precisely so that an asset tree which loads from a directory is
/// one that can be served over HTTP. `Path::parent` and `Path::join` use the
/// *platform's* separator, so on Windows they produce `meshes\\triangle.bin`
/// from a glTF that says `triangle.bin`, and the key is then refused as a path
/// escape. That is not hypothetical: it is what CI reported the first time this
/// ran on `windows-latest`.
///
/// So this splits on `/` and nothing else, which makes the result the same on
/// every platform by construction rather than by the separator happening to
/// match.
fn uri_parent(key: &Path) -> &str {
    let key = key.to_str().unwrap_or_default();
    match key.rfind('/') {
        Some(cut) => &key[..cut],
        None => "",
    }
}

/// A key naming `uri` beside `parent`, in URI space. See [`uri_parent`].
fn uri_sibling(parent: &str, uri: &str) -> String {
    if parent.is_empty() {
        uri.to_owned()
    } else {
        format!("{parent}/{uri}")
    }
}

fn resolve_buffers(
    source: &dyn AssetSource,
    key: &Path,
    document: &gltf::Gltf,
    mut blob: Option<Vec<u8>>,
) -> Result<Vec<Vec<u8>>, StorageError> {
    let root = document.document.as_json();
    let parent = uri_parent(key);
    let mut buffers = Vec::with_capacity(root.buffers.len());
    for (index, buffer) in root.buffers.iter().enumerate() {
        let bytes = match buffer.uri.as_deref() {
            None => blob.take().ok_or_else(|| {
                malformed(
                    key,
                    format!(
                        "buffer {index} has no uri, and only the first buffer of a \
                         .glb with a BIN chunk may omit one"
                    ),
                )
            })?,
            Some(uri) if uri.starts_with("data:") => {
                return Err(StorageError::Unsupported(
                    "glTF data: URI buffers — embed the bytes in a .glb, or keep \
                     them in a .bin beside the .gltf",
                ));
            }
            Some(uri) => source.read(Path::new(&uri_sibling(parent, uri)))?,
        };
        let declared = usize::try_from(buffer.byte_length.0).unwrap_or(usize::MAX);
        if bytes.len() < declared {
            return Err(malformed(
                key,
                format!(
                    "buffer {index} declares {declared} bytes and {} arrived",
                    bytes.len()
                ),
            ));
        }
        buffers.push(bytes);
    }
    Ok(buffers)
}

fn build(
    source: &dyn AssetSource,
    document: &gltf::Document,
    buffers: &[Vec<u8>],
    key: &Path,
) -> Result<GltfScene, StorageError> {
    warn_dropped_features(document, key);
    let images = read_images(source, document, buffers, key)?;
    let base_color_textures = document
        .materials()
        .map(|material| {
            material
                .pbr_metallic_roughness()
                .base_color_texture()
                // **`Texture::source` panics on a texture that has none**, so
                // this filter is load-bearing rather than tidy — see
                // `gltf_check::TEXTURE_SOURCE_ABSENT`. A texture whose image an
                // extension supplies becomes a material with no texture, which
                // is the same place an undecodable image lands.
                .filter(|info| texture_has_an_image(document, info.texture().index()))
                .map(|info| GltfTexture {
                    image: info.texture().source().index(),
                    tex_coord: info.tex_coord(),
                })
        })
        .collect();
    let materials = document
        .materials()
        .map(|material| {
            // All three factors off one accessor, which is what makes them one
            // material rather than a colour and two numbers that could drift.
            let pbr = material.pbr_metallic_roughness();
            GpuMaterial {
                base_color: pbr.base_color_factor(),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                // **The factors only; `base_color_texture` is left untextured.**
                // This column is a *page layer*, and which layer an image lands
                // in is known only to whoever builds the page. The document's
                // own answer — which image this material wants — is carried
                // beside the row in `base_color_textures` instead. Naming layer
                // 0 here is the honest value in the meantime: it is the page's
                // white layer, so a row nobody re-pointed shades with its
                // factors and nothing else.
                base_color_texture: GpuMaterial::UNTINTED.base_color_texture,
                // glTF texture coordinates are authored per vertex, so an
                // imported material samples the vertex UV — physical tiling is
                // the engine's own greybox mode, not something glTF describes.
                tiling: GpuMaterial::TILING_AUTHORED,
                tile_metres: GpuMaterial::UNTINTED.tile_metres,
            }
        })
        .collect();

    let mut meshes = Vec::with_capacity(document.meshes().len());
    for mesh in document.meshes() {
        let mut primitives = Vec::new();
        for primitive in mesh.primitives() {
            if primitive.mode() == Mode::Triangles {
                let at = format!("mesh {} primitive {}", mesh.index(), primitive.index());
                primitives.push(read_primitive(&primitive, buffers, &at, key)?);
            } else {
                crcbl_core::log::warn!(
                    "{}: skipping primitive {} of mesh {:?}: {:?} is not a triangle list",
                    key.display(),
                    primitive.index(),
                    mesh.name().unwrap_or("<unnamed>"),
                    primitive.mode(),
                );
            }
        }
        meshes.push(GltfMesh {
            name: mesh.name().map(str::to_owned),
            primitives,
        });
    }

    Ok(GltfScene {
        nodes: read_nodes(document.as_json(), key)?,
        instances: flatten(document, key)?,
        meshes,
        materials,
        base_color_textures,
        images,
    })
}

/// Warn, once per feature, about everything the document uses and this importer
/// does not read.
///
/// A log line rather than a field on [`GltfScene`], because there is nothing for
/// a caller to *do* with a skin this crate did not parse — the value is that a
/// character standing in its bind pose has an explanation somewhere. The
/// counts are the document's own, and the key is what makes a line actionable
/// when a hundred files went past.
fn warn_dropped_features(document: &gltf::Document, key: &Path) {
    let root = document.as_json();
    let morph_targets: usize = root
        .meshes
        .iter()
        .flat_map(|mesh| &mesh.primitives)
        .map(|primitive| primitive.targets.as_ref().map_or(0, Vec::len))
        .sum();
    for (count, feature, effect) in [
        (
            root.skins.len(),
            "skins",
            "every skinned mesh draws in its bind pose",
        ),
        (
            root.animations.len(),
            "animations",
            "nothing moves; playback is a post-MVP engine feature",
        ),
        (
            morph_targets,
            "morph targets",
            "every mesh draws at its base shape",
        ),
    ] {
        if count > 0 {
            crcbl_core::log::warn!(
                "{}: skipping {count} {feature}: this importer does not read them, so {effect}",
                key.display(),
            );
        }
    }
    warn_unsupported_extensions(root, key);

    // Counted off the JSON for `texture_has_an_image`'s reason. Reported
    // separately from the extension lines because a texture can lose its image
    // without the document declaring anything — and then this is the only line
    // that says why a material came out untextured.
    let imageless = root
        .textures
        .iter()
        .filter(|texture| texture.source.value() >= root.images.len())
        .count();
    if imageless > 0 {
        crcbl_core::log::warn!(
            "{}: skipping {imageless} texture(s) that name no image: it is supplied by an \
             extension this importer does not implement, so every material using them shades \
             with its base colour",
            key.display(),
        );
    }
}

/// Whether texture `index` names an image this document actually carries.
///
/// Read off the JSON rather than through [`gltf::Texture::source`], because
/// that accessor is `images().nth(source).unwrap()` and the whole point is the
/// textures for which it would panic — see
/// [`crate::gltf_check::TEXTURE_SOURCE_ABSENT`].
fn texture_has_an_image(document: &gltf::Document, index: usize) -> bool {
    let root = document.as_json();
    root.textures
        .get(index)
        .is_some_and(|texture| texture.source.value() < root.images.len())
}

/// The one glTF extension this importer implements.
///
/// `lod_resolve` reads it. Everything else a document declares is ignored, so
/// this list is what [`warn_unsupported_extensions`] measures against — and it
/// is a list rather than a constant so that adding the second one is a line
/// here instead of a rewrite.
const IMPLEMENTED_EXTENSIONS: &[&str] = &["MSFT_lod"];

/// Name every extension the document declares and this importer does not
/// implement, `extensionsRequired` louder than `extensionsUsed`.
///
/// **The file, the feature and the reason**, which is what
/// `docs/plan/sample/05-viewer.md`'s exit criteria ask of a document that did
/// not arrive whole. Before this, a `KHR_materials_sheen` sofa loaded and drew
/// with no sheen and said nothing at all, and the only clue was that the
/// picture looked wrong.
///
/// # A required extension is reported, not refused
///
/// The specification says a client SHOULD NOT load an asset whose
/// `extensionsRequired` it cannot honour. This importer loads it anyway and
/// says so, because the alternative is worse for the one application that
/// consumes it: a viewer exists to open the file somebody is holding, and
/// refusing `PotOfCoalsAnimationPointer` outright tells them less about their
/// asset than drawing it without its specular extension does.
///
/// **That trade only holds while the report is loud**, which is what this
/// function is for. `docs/backlog.md` carries the decision, because the honest
/// answer may yet be a flag.
fn warn_unsupported_extensions(root: &gltf::json::Root, key: &Path) {
    let unsupported = |names: &[String]| -> Vec<String> {
        names
            .iter()
            .filter(|name| !IMPLEMENTED_EXTENSIONS.contains(&name.as_str()))
            .cloned()
            .collect()
    };

    let required = unsupported(&root.extensions_required);
    if !required.is_empty() {
        crcbl_core::log::warn!(
            "{}: this document REQUIRES {}, which this importer does not implement — it is \
             drawn without them, so what is on screen is not what the file describes",
            key.display(),
            required.join(", "),
        );
    }

    // Everything used but not required, and not already named above: the
    // document itself says these are optional, so the line is quieter.
    let optional: Vec<String> = unsupported(&root.extensions_used)
        .into_iter()
        .filter(|name| !required.contains(name))
        .collect();
    if !optional.is_empty() {
        crcbl_core::log::warn!(
            "{}: ignoring {}, which this importer does not implement — the document lists them \
             as optional, so the rest of it is unaffected",
            key.display(),
            optional.join(", "),
        );
    }
}

/// The encoded bytes of every image the document names, in order.
///
/// Two sources, and only one of them touches the asset seam: an image in a
/// `bufferView` is a slice of a buffer already resolved, and an image with a
/// `uri` is a key beside the document read the same way an external `.bin` is.
///
/// Anything that stops a URI resolving becomes that image's stored reason and a
/// warning, rather than the import's failure — see the [module docs](self).
/// [`StorageError::Pending`] is the one exception and propagates: "not resident
/// yet" is not a defect in the file, and answering it as a skipped texture would
/// bake a missing image into a scene that would have had one next frame.
fn read_images(
    source: &dyn AssetSource,
    document: &gltf::Document,
    buffers: &[Vec<u8>],
    key: &Path,
) -> Result<Vec<GltfImage>, StorageError> {
    let parent = uri_parent(key);
    let mut images = Vec::with_capacity(document.images().len());
    for image in document.images() {
        let index = image.index();
        let name = image.name().map(str::to_owned);
        let (mime, bytes) = match image.source() {
            gltf::image::Source::View { view, mime_type } => {
                // `check_views` put every view inside its own buffer and
                // `check_images` put this view inside the document, so the
                // slice below is in range by construction.
                let buffer = &buffers[view.buffer().index()];
                let start = view.offset();
                let bytes = buffer[start..start + view.length()].to_vec();
                (Some(mime_type.to_owned()), Ok(bytes))
            }
            gltf::image::Source::Uri { uri, mime_type } => {
                let bytes = if uri.starts_with("data:") {
                    Err(
                        "its bytes are a data: URI, which needs a base64 decoder this build \
                         does not have"
                            .to_owned(),
                    )
                } else {
                    match source.read(Path::new(&uri_sibling(parent, uri))) {
                        Ok(bytes) => Ok(bytes),
                        Err(pending @ StorageError::Pending(_)) => return Err(pending),
                        Err(error) => Err(format!("{uri:?} could not be read: {error}")),
                    }
                };
                (mime_type.map(str::to_owned), bytes)
            }
        };
        if let Err(why) = &bytes {
            crcbl_core::log::warn!(
                "{}: skipping image {index} {:?}: {why}; every material naming it shades \
                 untextured",
                key.display(),
                name.as_deref().unwrap_or("<unnamed>"),
            );
        }
        images.push(GltfImage { name, mime, bytes });
    }
    Ok(images)
}

/// The document's `nodes` array, name and mesh and `MSFT_lod` each.
fn read_nodes(root: &gltf::json::Root, key: &Path) -> Result<Vec<GltfNode>, StorageError> {
    root.nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            Ok(GltfNode {
                name: node.name.clone(),
                mesh: node.mesh.map(|mesh| mesh.value()),
                lod_nodes: read_msft_lod(node, index, root.nodes.len(), key)?,
            })
        })
        .collect()
}

/// One node's `MSFT_lod` `ids`, checked against the node array.
///
/// Read out of the raw extension JSON rather than a typed field: `gltf` models
/// only the `KHR_*` extensions it has features for, and everything else arrives
/// as the `serde_json` map the `extensions` feature exposes. Nothing is
/// silently dropped — an `MSFT_lod` that is not an object, has no `ids` array,
/// or names something other than an existing node makes the document malformed,
/// because the alternative is a declared detail level that quietly is not one.
fn read_msft_lod(
    node: &gltf::json::Node,
    index: usize,
    nodes: usize,
    key: &Path,
) -> Result<Vec<usize>, StorageError> {
    let Some(extension) = node
        .extensions
        .as_ref()
        .and_then(|extensions| extensions.others.get("MSFT_lod"))
    else {
        return Ok(Vec::new());
    };
    let ids = extension
        .get("ids")
        .and_then(|ids| ids.as_array())
        .ok_or_else(|| malformed(key, format!("node {index}'s MSFT_lod has no ids array")))?;
    ids.iter()
        .map(|id| {
            id.as_u64()
                .and_then(|id| usize::try_from(id).ok())
                .filter(|&id| id < nodes)
                .ok_or_else(|| {
                    malformed(
                        key,
                        format!("node {index}'s MSFT_lod names {id}, and there are {nodes} nodes"),
                    )
                })
        })
        .collect()
}

fn read_primitive(
    primitive: &gltf::Primitive<'_>,
    buffers: &[Vec<u8>],
    at: &str,
    key: &Path,
) -> Result<GltfPrimitive, StorageError> {
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));

    let positions: Vec<[f32; 3]> = reader
        .read_positions()
        .ok_or_else(|| malformed(key, format!("{at}'s POSITION accessor reads nothing")))?
        .collect();
    let vertices = u32::try_from(positions.len())
        .map_err(|_| malformed(key, format!("{at} has {} vertices", positions.len())))?;

    let normals: Vec<[f32; 3]> = reader
        .read_normals()
        .map(|read| read.collect())
        .unwrap_or_default();
    let tex_coords: Vec<[f32; 2]> = reader
        .read_tex_coords(0)
        .map(|read| read.into_f32().collect())
        .unwrap_or_default();
    for (what, found) in [("NORMAL", normals.len()), ("TEXCOORD_0", tex_coords.len())] {
        if found != 0 && found != positions.len() {
            return Err(malformed(
                key,
                format!(
                    "{at} has {} positions and {found} {what} values",
                    positions.len()
                ),
            ));
        }
    }

    let indices: Vec<u32> = reader
        .read_indices()
        .map_or_else(|| (0..vertices).collect(), |read| read.into_u32().collect());
    if let Some(&past) = indices.iter().find(|&&index| index >= vertices) {
        return Err(malformed(
            key,
            format!("{at} has index {past} and {vertices} vertices"),
        ));
    }

    Ok(GltfPrimitive {
        positions,
        normals,
        tex_coords,
        indices,
        material: primitive.material().index(),
    })
}

/// Walk the scene's node forest, composing transforms, and emit one instance
/// per node that draws a mesh.
///
/// Iterative rather than recursive: the depth is file-controlled, and a
/// recursive walk of a thousand-deep chain overflows the stack, which is not
/// something an error can be returned from. `visited` is what makes the walk
/// terminate at all — glTF requires the node graph to be a forest and nothing
/// in the file format prevents a cycle.
fn flatten(document: &gltf::Document, key: &Path) -> Result<Vec<GltfInstance>, StorageError> {
    let Some(scene) = document
        .default_scene()
        .or_else(|| document.scenes().next())
    else {
        return Ok(Vec::new());
    };

    let mut instances = Vec::new();
    let mut visited = vec![false; document.nodes().len()];
    let roots: Vec<_> = scene.nodes().collect();
    // Reversed on the way in so popping produces document order.
    let mut stack: Vec<_> = roots
        .into_iter()
        .rev()
        .map(|node| (node, Mat4::IDENTITY))
        .collect();
    while let Some((node, parent)) = stack.pop() {
        if std::mem::replace(&mut visited[node.index()], true) {
            return Err(malformed(
                key,
                format!(
                    "node {} is reachable twice: the node hierarchy must be a forest",
                    node.index()
                ),
            ));
        }
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(mesh) = node.mesh() {
            instances.push(GltfInstance {
                node: node.index(),
                mesh: mesh.index(),
                transform: world.to_cols_array(),
            });
        }
        let children: Vec<_> = node.children().collect();
        stack.extend(children.into_iter().rev().map(|child| (child, world)));
    }
    Ok(instances)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gltf_fixture::{
        Assets, BASE_COLOR, BIN_CHUNK_BUFFER, EXTERNAL_BUFFER, IMAGE_TEXELS, INDICES, NORMALS,
        POSITIONS, TEX_COORDS, glb, import_glb, import_glb_bytes, import_gltf_text, png_bytes,
        replacing, textured_glb, textured_parts, triangle_bin, triangle_json,
    };

    /// The row a glTF material with no `pbrMetallicRoughness` block imports as:
    /// every factor the specification's own default.
    ///
    /// **Deliberately not [`GpuMaterial::UNTINTED`]**, which it was until the row
    /// grew shading factors. glTF defaults `metallicFactor` and
    /// `roughnessFactor` to `1.0` — a fully rough conductor — where that row is a
    /// dielectric at half roughness, and the importer's job is to report the
    /// document. Written out here rather than derived from `UNTINTED`, so a
    /// change to the engine's neutral row cannot silently move what a document
    /// is claimed to say.
    const GLTF_DEFAULT_MATERIAL: GpuMaterial = GpuMaterial {
        base_color: [1.0; 4],
        base_color_texture: 0,
        metallic: 1.0,
        roughness: 1.0,
        tiling: GpuMaterial::TILING_AUTHORED,
        tile_metres: 1.0,
    };

    /// The `animations` array a `KHR_animation_pointer` document carries: a
    /// channel whose `target` names a **pointer** and no `node`.
    ///
    /// `gltf_json::animation::Target` makes `node` mandatory, so this is what
    /// makes `serde` refuse the whole `Root`. Copied from the shape
    /// `AnimatedColorsCube` in the Khronos sample suite uses, which is the
    /// document that found this.
    const ANIMATION_POINTER: &str = r#""animations": [{
    "samplers": [{ "input": 0, "output": 1, "interpolation": "LINEAR" }],
    "channels": [{
      "sampler": 0,
      "target": {
        "path": "pointer",
        "extensions": {
          "KHR_animation_pointer": { "pointer": "/materials/0/pbrMetallicRoughness/baseColorFactor" }
        }
      }
    }]
  }],
  "extensionsUsed": ["KHR_animation_pointer"],
  "#;

    /// **A document is not refused over an animation this importer discards.**
    ///
    /// `report_unsupported` skips every animation in every document and logs a
    /// line saying so, but a `KHR_animation_pointer` channel cannot even be
    /// deserialized — `node` is a required field and that extension replaces it
    /// — so the failure took the whole document with it. `AnimatedColorsCube`
    /// from the Khronos suite lists **nothing** in `extensionsRequired`, so the
    /// specification says it has to load, and before `parse_without_animations`
    /// it did not.
    ///
    /// The geometry is asserted, not just the `Ok`: a repair that dropped the
    /// animation and the mesh with it would satisfy "it loads" and be useless.
    #[test]
    fn an_animation_this_importer_cannot_deserialize_does_not_refuse_the_document() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            r#""asset": {"#,
            &format!("{ANIMATION_POINTER}\"asset\": {{"),
        );
        let scene = import_glb(&json).expect("the document loads without its animation");

        assert_eq!(scene.meshes().len(), 1, "the mesh went with the animation");
        let primitive = &scene.meshes()[0].primitives()[0];
        assert_eq!(primitive.positions(), POSITIONS);
        assert_eq!(
            primitive.indices(),
            INDICES.map(u32::from),
            "the repaired parse must produce the same geometry as an untouched one",
        );
    }

    /// **The retry does not turn a malformed document into a silent success.**
    ///
    /// `parse_without_animations` reports `None` for anything that fails for a
    /// second reason, so the caller raises the error from the *first* attempt —
    /// the accurate one, about the document as written rather than as altered.
    #[test]
    fn a_document_broken_for_another_reason_is_still_refused_with_its_own_error() {
        // An animation `serde` refuses *and* a `nodes` array that is not an
        // array. Dropping the animations cannot rescue this one.
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            r#""asset": {"#,
            &format!("{ANIMATION_POINTER}\"asset\": {{"),
        );
        let json = replacing(&json, r#""scenes": [{ "nodes": [0] }],"#, r#""scenes": 7,"#);
        let error = import_glb(&json).expect_err("a malformed document is refused");
        let message = error.to_string();
        assert!(
            !message.contains("animation"),
            "the retry's own failure was reported instead of the document's: {message}",
        );
    }

    /// Every warning `warn_dropped_features` emitted while importing `json`.
    fn import_warnings(json: &str) -> Vec<String> {
        let logs = crcbl_core::log::capture();
        import_glb(json).expect("the fixture imports");
        logs.records()
            .into_iter()
            .filter(|record| record.target.contains("gltf_import"))
            .map(|record| record.message)
            .collect()
    }

    /// The fixture with `extensionsUsed`/`extensionsRequired` spliced in.
    fn with_extensions(used: &str, required: &str) -> String {
        replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            r#""asset": {"#,
            &format!(
                r#""extensionsUsed": [{used}], "extensionsRequired": [{required}], "asset": {{"#
            ),
        )
    }

    /// **A required extension this importer cannot honour is named, not
    /// swallowed.**
    ///
    /// `docs/plan/sample/05-viewer.md`'s exit criterion asks for the file, the
    /// feature and the reason. `SheenWoodLeatherSofa` and three others from the
    /// Khronos suite used to load and draw wrong in silence — the only clue was
    /// that the picture looked off.
    #[test]
    fn a_required_extension_the_importer_lacks_is_named_in_a_warning() {
        let warnings = import_warnings(&with_extensions("", r#""KHR_materials_sheen""#));
        let named = warnings
            .iter()
            .find(|line| line.contains("REQUIRES"))
            .unwrap_or_else(|| panic!("no line named the required extension: {warnings:#?}"));
        assert!(
            named.contains("KHR_materials_sheen"),
            "the line does not name the extension: {named}",
        );
    }

    /// An extension the document itself calls optional gets the quieter line,
    /// and is not reported as required — the two say different things about
    /// whether what is on screen is the file.
    #[test]
    fn an_optional_extension_is_reported_separately_from_a_required_one() {
        let warnings = import_warnings(&with_extensions(r#""KHR_animation_pointer""#, ""));
        assert!(
            warnings
                .iter()
                .any(|line| line.contains("ignoring") && line.contains("KHR_animation_pointer")),
            "the optional extension was not reported: {warnings:#?}",
        );
        assert!(
            !warnings.iter().any(|line| line.contains("REQUIRES")),
            "an optional extension was reported as required: {warnings:#?}",
        );
    }

    /// **An extension the importer *does* implement is not reported at all.**
    ///
    /// The guard that keeps this list honest: without it the warning fires for
    /// `MSFT_lod`, which `lod_resolve` reads, and every document using it would
    /// carry a line saying its levels were ignored while they were being
    /// resolved.
    #[test]
    fn an_extension_this_importer_implements_is_not_reported() {
        let warnings = import_warnings(&with_extensions(r#""MSFT_lod""#, r#""MSFT_lod""#));
        assert!(
            !warnings
                .iter()
                .any(|line| line.contains("MSFT_lod") || line.contains("REQUIRES")),
            "an implemented extension was reported as unsupported: {warnings:#?}",
        );
    }

    /// An extension in both lists is named once, as required. Listing it twice
    /// would read as two different problems with the same document.
    #[test]
    fn an_extension_in_both_lists_is_named_once() {
        let name = r#""KHR_texture_transform""#;
        let warnings = import_warnings(&with_extensions(name, name));
        let mentions = warnings
            .iter()
            .filter(|line| line.contains("KHR_texture_transform"))
            .count();
        assert_eq!(mentions, 1, "named more than once: {warnings:#?}");
    }

    /// **A texture whose image an extension supplies is skipped, not refused.**
    ///
    /// `source` has a `serde` default of `u32::MAX` — see
    /// `gltf_check::TEXTURE_SOURCE_ABSENT` — so a texture that omits it used to
    /// be refused with `texture 0 names image 4294967295`, a sentinel this crate
    /// invented reported as though the document had written it.
    /// `SheenWoodLeatherSofa` from the Khronos suite was the one model in that
    /// suite refused for it.
    ///
    /// The material must survive with **no** texture rather than with a broken
    /// one: `gltf::Texture::source` is an `unwrap` on that index and would abort
    /// the process.
    #[test]
    fn a_texture_that_names_no_image_is_skipped_and_its_material_keeps_its_colour() {
        let (base, bin) = textured_parts(&png_bytes(2, 2, &IMAGE_TEXELS), "image/png", 0);
        let json = replacing(
            &base,
            r#""textures": [{ "source": 0 }]"#,
            r#""textures": [{ }]"#,
        );
        let scene = import_glb_bytes(&glb(&json, Some(&bin)))
            .expect("a texture with no image does not refuse the document");

        assert_eq!(
            scene.materials().len(),
            1,
            "the material went with its texture",
        );
        assert!(
            scene.base_color_textures()[0].is_none(),
            "the material kept a texture whose image cannot be read",
        );
    }

    /// And it says so, naming the count — otherwise a material coming out
    /// untextured has no explanation anywhere.
    #[test]
    fn a_skipped_texture_is_reported() {
        let (base, bin) = textured_parts(&png_bytes(2, 2, &IMAGE_TEXELS), "image/png", 0);
        let json = replacing(
            &base,
            r#""textures": [{ "source": 0 }]"#,
            r#""textures": [{ }]"#,
        );
        let logs = crcbl_core::log::capture();
        import_glb_bytes(&glb(&json, Some(&bin))).expect("the document imports");
        let messages: Vec<String> = logs
            .records()
            .into_iter()
            .filter(|record| record.target.contains("gltf_import"))
            .map(|record| record.message)
            .collect();
        assert!(
            messages
                .iter()
                .any(|line| line.contains("1 texture(s) that name no image")),
            "nothing said the texture was skipped: {messages:#?}",
        );
    }

    /// **An out-of-range source is still refused**, which is the half that must
    /// not move: that is a document naming an image it does not have, and
    /// letting it through would put an `unwrap` on a real index.
    #[test]
    fn a_texture_naming_an_image_that_does_not_exist_is_still_refused() {
        let (base, bin) = textured_parts(&png_bytes(2, 2, &IMAGE_TEXELS), "image/png", 0);
        let json = replacing(
            &base,
            r#""textures": [{ "source": 0 }]"#,
            r#""textures": [{ "source": 9 }]"#,
        );
        let error = import_glb_bytes(&glb(&json, Some(&bin)))
            .expect_err("a texture naming a missing image is refused");
        assert!(
            error.to_string().contains("texture 0 names image 9"),
            "the refusal does not name the index: {error}",
        );
    }

    #[test]
    fn a_minimal_glb_yields_its_positions_normals_texcoords_indices_and_material() {
        let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();

        assert_eq!(scene.meshes().len(), 1);
        let mesh = &scene.meshes()[0];
        assert_eq!(mesh.name(), Some("triangle"));
        assert_eq!(mesh.primitives().len(), 1);

        let primitive = &mesh.primitives()[0];
        assert_eq!(primitive.positions(), POSITIONS);
        assert_eq!(primitive.normals(), NORMALS);
        assert_eq!(primitive.tex_coords(), TEX_COORDS);
        assert_eq!(
            primitive.indices(),
            INDICES.map(u32::from),
            "u16 indices arrive widened, not reinterpreted"
        );
        assert_eq!(primitive.material(), Some(0));

        // The fixture names a `baseColorFactor` and neither shading factor, so
        // the colour is the document's and the other two are the
        // specification's defaults — which is the whole of what the mapping
        // claims.
        assert_eq!(
            scene.materials(),
            [GpuMaterial {
                base_color: BASE_COLOR,
                ..GLTF_DEFAULT_MATERIAL
            }]
        );
    }

    /// The same document with its bytes in a file instead of a chunk. Asserted
    /// against the `.glb` rather than against a second copy of the expected
    /// values, so the two paths cannot drift.
    /// The sibling key is built in URI space on every platform.
    ///
    /// This is the test that would have caught the Windows failure on Linux.
    /// Asserting the *imported scene* could not: `Path::join` produces the right
    /// string here and the wrong one on Windows, so the round trip passes on the
    /// machine the code was written on and fails on the runner. Asserting the
    /// join itself, over strings, is the same claim on both.
    #[test]
    fn a_buffer_beside_a_gltf_is_named_with_a_slash_whatever_the_platform() {
        assert_eq!(uri_parent(Path::new("meshes/scene.gltf")), "meshes");
        assert_eq!(uri_parent(Path::new("a/b/scene.gltf")), "a/b");
        assert_eq!(uri_parent(Path::new("scene.gltf")), "");

        assert_eq!(uri_sibling("meshes", "triangle.bin"), "meshes/triangle.bin");
        assert_eq!(uri_sibling("a/b", "c.bin"), "a/b/c.bin");
        // A gltf at the root names its sibling with no prefix at all — not
        // "/c.bin", which `canonical_key` refuses as an absolute path.
        assert_eq!(uri_sibling("", "c.bin"), "c.bin");

        // The property the failure was about: nothing this builds contains a
        // separator `canonical_key` will not accept.
        for key in [
            uri_sibling(uri_parent(Path::new("meshes/scene.gltf")), "triangle.bin"),
            uri_sibling(uri_parent(Path::new("scene.gltf")), "triangle.bin"),
        ] {
            assert!(!key.contains('\\'), "{key} carries a platform separator");
        }
    }

    #[test]
    fn a_gltf_reads_its_buffer_from_the_bin_file_beside_it() {
        let from_file = import_gltf_text(&triangle_json(EXTERNAL_BUFFER), &triangle_bin()).unwrap();
        assert_eq!(
            from_file,
            import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap()
        );
    }

    /// The node table is the document's array, so it holds the parent that
    /// draws nothing as well as the child that draws the mesh — and an instance
    /// says which entry it came from.
    #[test]
    fn every_node_is_in_the_table_with_its_name_and_what_it_draws() {
        let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();

        assert_eq!(
            scene.nodes(),
            [
                GltfNode {
                    name: Some("root".to_owned()),
                    mesh: None,
                    lod_nodes: Vec::new(),
                },
                GltfNode {
                    name: Some("leaf".to_owned()),
                    mesh: Some(0),
                    lod_nodes: Vec::new(),
                },
            ]
        );
        assert_eq!(
            scene.instances()[0].node(),
            1,
            "the drawing node, not the root above it"
        );
    }

    /// `MSFT_lod` has no feature of its own in `gltf`, so this is the raw
    /// extension map being read — and the ids arriving as node indices is what
    /// `crate::lod_resolve` rests on.
    #[test]
    fn a_nodes_msft_lod_ids_are_read_from_the_raw_extension() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            r#"{ "name": "leaf", "mesh": 0, "translation": [0.0, 5.0, 0.0] }"#,
            r#"{ "name": "leaf", "mesh": 0, "extensions": { "MSFT_lod": { "ids": [0] } } }"#,
        );
        let scene = import_glb(&json).unwrap();

        assert!(
            scene.nodes()[0].lod_nodes().is_empty(),
            "no extension, no ids"
        );
        assert_eq!(scene.nodes()[1].lod_nodes(), [0]);
    }

    /// The fixture's mesh hangs off a child node, so the instance transform is
    /// only right if the parent's translation was composed into it.
    #[test]
    fn a_nodes_transform_composes_with_every_parent_above_it() {
        let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();

        assert_eq!(scene.instances().len(), 1, "one node draws a mesh");
        let instance = scene.instances()[0];
        assert_eq!(instance.mesh(), 0);
        assert_eq!(
            instance.transform(),
            Mat4::from_translation(glam::Vec3::new(10.0, 5.0, 0.0)).to_cols_array(),
            "the leaf's (0, 5, 0) under the root's (10, 0, 0)"
        );
    }

    #[test]
    fn a_primitive_with_no_index_accessor_is_given_the_trivial_indices() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            "\n      \"indices\": 3,",
            "",
        );
        let scene = import_glb(&json).unwrap();
        let primitive = &scene.meshes()[0].primitives()[0];
        assert_eq!(primitive.indices(), [0, 1, 2]);
        assert_eq!(primitive.positions().len(), 3);
    }

    /// A material with no `pbrMetallicRoughness` block at all imports as the
    /// specification's defaults, and **that is no longer the engine's neutral
    /// row**.
    ///
    /// The assertion moved rather than the importer: glTF's default material is
    /// a fully rough conductor, and a mapping that quietly substituted
    /// [`GpuMaterial::UNTINTED`] for it would be reporting the engine's
    /// preference as the document's content. `GLTF_DEFAULT_MATERIAL` is what it
    /// is asserted against, and the `assert_ne!` below is what says the two are
    /// genuinely different rows — without it this test would pass again the day
    /// somebody made them equal.
    #[test]
    fn a_material_with_no_factors_takes_the_gltf_defaults_and_a_primitive_may_name_none() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            "\"pbrMetallicRoughness\": { \"baseColorFactor\": [0.25, 0.5, 0.75, 1.0] }",
            "\"doubleSided\": true",
        );
        let json = replacing(&json, ",\n      \"material\": 0", "");
        let scene = import_glb(&json).unwrap();

        assert_eq!(scene.materials(), [GLTF_DEFAULT_MATERIAL]);
        assert_ne!(
            GLTF_DEFAULT_MATERIAL,
            GpuMaterial::UNTINTED,
            "an imported default material used to be the untinted row and is not one any more; \
             a tree in which they are equal again is one where this test says nothing"
        );
        assert_eq!(scene.meshes()[0].primitives()[0].material(), None);
    }

    /// `docs/plan/06-assets-scenes.md`'s risk section: unsupported features
    /// log and skip rather than failing the load.
    #[test]
    fn a_primitive_that_is_not_a_triangle_list_is_skipped_and_the_rest_still_loads() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            "\"indices\": 3,",
            "\"indices\": 3, \"mode\": 0,",
        );
        let scene = import_glb(&json).unwrap();

        assert_eq!(scene.meshes().len(), 1, "the mesh keeps its entry");
        assert!(scene.meshes()[0].primitives().is_empty());
        assert_eq!(
            scene.instances().len(),
            1,
            "and the node naming it still resolves"
        );
    }

    #[test]
    fn a_document_with_no_scene_still_yields_its_meshes_and_no_instances() {
        let json = replacing(&triangle_json(BIN_CHUNK_BUFFER), "\n  \"scene\": 0,", "");
        let json = replacing(&json, "\n  \"scenes\": [{ \"nodes\": [0] }],", "");
        let scene = import_glb(&json).unwrap();

        assert!(scene.instances().is_empty());
        assert_eq!(scene.meshes()[0].primitives()[0].positions(), POSITIONS);
    }

    /// A document with scenes but no `scene` renders the first one, which is
    /// what every other loader does with a file the spec leaves undefined.
    #[test]
    fn a_document_with_no_default_scene_falls_back_to_the_first_one() {
        let json = replacing(&triangle_json(BIN_CHUNK_BUFFER), "\n  \"scene\": 0,", "");
        let scene = import_glb(&json).unwrap();
        assert_eq!(scene.instances().len(), 1);
    }

    /// The whole reason `AssetSource::read` may not block: a source that does
    /// not have the bytes yet makes the import a state rather than a failure,
    /// for the document and for a buffer it names alike.
    #[test]
    fn an_import_is_pending_while_either_the_document_or_a_buffer_is_not_resident() {
        use std::cell::RefCell;
        use std::collections::HashMap;
        use std::path::PathBuf;

        #[derive(Debug, Default)]
        struct Pending {
            files: HashMap<String, Vec<u8>>,
            asked: RefCell<Vec<String>>,
        }

        impl AssetSource for Pending {
            fn read(&self, key: &Path) -> Result<Vec<u8>, StorageError> {
                let name = key.to_string_lossy().into_owned();
                self.asked.borrow_mut().push(name.clone());
                self.files
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| StorageError::Pending(PathBuf::from(name)))
            }
        }

        let mut source = Pending::default();
        assert!(
            matches!(
                import_gltf(&source, Path::new("model.gltf")),
                Err(StorageError::Pending(_))
            ),
            "the document itself has not arrived"
        );

        source.files.insert(
            "model.gltf".to_string(),
            triangle_json(EXTERNAL_BUFFER).into_bytes(),
        );
        assert!(
            matches!(
                import_gltf(&source, Path::new("model.gltf")),
                Err(StorageError::Pending(_))
            ),
            "the document is here and its buffer is not"
        );
        assert_eq!(
            source.asked.borrow().last().map(String::as_str),
            Some("triangle.bin"),
            "the buffer uri resolved beside the document"
        );

        source
            .files
            .insert("triangle.bin".to_string(), triangle_bin());
        let scene = import_gltf(&source, Path::new("model.gltf")).unwrap();
        assert_eq!(scene.meshes()[0].primitives()[0].positions(), POSITIONS);
    }

    #[test]
    fn a_documents_images_arrive_encoded_beside_the_material_that_names_them() {
        let png = png_bytes(2, 2, &IMAGE_TEXELS);
        let scene = import_glb_bytes(&textured_glb(&png, "image/png", 0)).unwrap();

        assert_eq!(scene.images().len(), 1);
        let image = &scene.images()[0];
        assert_eq!(image.name(), Some("paint"));
        assert_eq!(image.mime(), Some("image/png"));
        assert_eq!(
            image.bytes(),
            Ok(&png[..]),
            "the bytes are the file's own, byte for byte and still encoded"
        );

        assert_eq!(scene.base_color_textures().len(), scene.materials().len());
        let texture = scene.base_color_textures()[0].expect("the material names a texture");
        assert_eq!(texture.image(), 0);
        assert_eq!(texture.tex_coord(), 0);
        assert_eq!(
            scene.materials()[0].base_color_texture,
            GpuMaterial::UNTINTED.base_color_texture,
            "the row's texture column stays at the untextured layer: a page layer is not \
             something this module can know"
        );
    }

    #[test]
    fn a_material_naming_no_texture_has_no_entry_beside_it() {
        let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();
        assert_eq!(scene.images(), []);
        assert_eq!(scene.base_color_textures(), [None]);
    }

    #[test]
    fn the_texcoord_a_material_asks_for_is_reported_rather_than_assumed() {
        let png = png_bytes(2, 2, &IMAGE_TEXELS);
        let scene = import_glb_bytes(&textured_glb(&png, "image/png", 1)).unwrap();
        assert_eq!(
            scene.base_color_textures()[0]
                .expect("a texture")
                .tex_coord(),
            1,
            "TEXCOORD_1 is not read, and a silent 0 here would sample the wrong UVs \
             instead of saying so"
        );
    }

    /// The asymmetry the module docs argue for: a buffer that will not resolve
    /// fails the import, and an image that will not resolve is a skipped image.
    #[test]
    fn an_image_uri_the_source_cannot_read_is_skipped_and_the_document_still_imports() {
        let (json, bin) = textured_parts(b"unused", "image/png", 0);
        let json = replacing(
            &json,
            r#"{ "name": "paint", "bufferView": 4, "mimeType": "image/png" }"#,
            r#"{ "name": "paint", "uri": "paint.png" }"#,
        );
        let scene = import_glb_bytes(&glb(&json, Some(&bin)))
            .expect("a missing texture is not a missing model");

        let why = scene.images()[0]
            .bytes()
            .expect_err("the file is not beside the document");
        assert!(
            why.contains("paint.png") && why.contains("could not be read"),
            "the reason names the uri that failed: {why}"
        );
        assert_eq!(
            scene.meshes()[0].primitives()[0].positions(),
            POSITIONS,
            "the geometry is untouched by the texture that was not there"
        );
    }

    #[test]
    fn a_data_uri_image_is_skipped_where_a_data_uri_buffer_is_refused() {
        let (json, bin) = textured_parts(b"unused", "image/png", 0);
        let json = replacing(
            &json,
            r#"{ "name": "paint", "bufferView": 4, "mimeType": "image/png" }"#,
            r#"{ "name": "paint", "uri": "data:image/png;base64,iVBORw0KGgo=" }"#,
        );
        let scene = import_glb_bytes(&glb(&json, Some(&bin))).expect("still imports");
        let why = scene.images()[0].bytes().unwrap_err();
        assert!(
            why.contains("base64"),
            "the reason says what is missing: {why}"
        );
    }

    #[test]
    fn an_image_uri_resolves_beside_the_document_like_a_buffer_uri_does() {
        let png = png_bytes(2, 2, &IMAGE_TEXELS);
        let (json, bin) = textured_parts(b"unused", "image/png", 0);
        let json = replacing(
            &json,
            r#"{ "name": "paint", "bufferView": 4, "mimeType": "image/png" }"#,
            r#"{ "name": "paint", "uri": "paint.png" }"#,
        );

        let assets = Assets::new();
        assets.write("meshes/model.glb", &glb(&json, Some(&bin)));
        assets.write("meshes/paint.png", &png);
        let scene = assets.import("meshes/model.glb").unwrap();

        assert_eq!(
            scene.images()[0].bytes(),
            Ok(&png[..]),
            "`meshes/paint.png`, not `paint.png`: a uri is relative to the document's key"
        );
        assert_eq!(
            scene.images()[0].mime(),
            None,
            "a uri image may declare no mimeType, and this one does not"
        );
    }

    #[test]
    fn an_image_uri_escaping_the_asset_root_is_skipped_rather_than_read() {
        let (json, bin) = textured_parts(b"unused", "image/png", 0);
        let json = replacing(
            &json,
            r#"{ "name": "paint", "bufferView": 4, "mimeType": "image/png" }"#,
            r#"{ "name": "paint", "uri": "../../secret.png" }"#,
        );
        let assets = Assets::new();
        assets.write("meshes/model.glb", &glb(&json, Some(&bin)));
        std::fs::write(assets.outside().join("secret.png"), b"not yours").unwrap();

        let scene = assets.import("meshes/model.glb").unwrap();
        let why = scene.images()[0].bytes().unwrap_err();
        assert!(
            why.contains("secret.png"),
            "the reason names the key that was refused: {why}"
        );
    }
}
