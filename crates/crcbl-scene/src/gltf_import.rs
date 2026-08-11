//! glTF 2.0 import: bytes through [`AssetSource`], geometry and material
//! factors out.
//!
//! This is the first half of step 3 of `docs/plan/06-assets-scenes.md`. It ends
//! at host memory: there is no GPU pool upload, no texture decode and no mip
//! generation here, and the types below are what the upload step consumes.
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
//! into albedo before any tonemap or sRGB encode. Both defaults agree too —
//! glTF's missing-factor default is `[1.0; 4]` and so is
//! [`GpuMaterial::UNTINTED`].
//!
//! The row also has a `base_color_texture` column, and **this importer leaves
//! it at the untextured layer**. glTF's `baseColorTexture` is an image, and
//! nothing here decodes one, uploads one or owns a layer of the renderer's
//! page — so a material arrives with its factor and the page's white layer,
//! which is exactly what an imported material shaded before the column existed.
//!
//! # What is parsed and dropped
//!
//! Skins and animations are in the format and are not read: the plan has them
//! "parsed but unused until the animation feature lands", and a type nothing
//! fills is worse than no type. Textures are step 3's second half. Vertex
//! colours, tangents, `TEXCOORD_1` and morph targets are read by nothing here
//! and so are not extracted.
//!
//! [`AssetSource`]: crcbl_assets::AssetSource
//! [`AssetSource::read`]: crcbl_assets::AssetSource::read
//! [`GpuMaterial::base_color`]: crcbl_shaders::mesh::GpuMaterial::base_color
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
    /// `None` means the glTF default material, which is
    /// [`GpuMaterial::UNTINTED`] plus factors nothing here reads. It is not
    /// substituted for a real index, because a table row nothing wrote is black
    /// and the caller has to decide what to put there.
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
    mesh: usize,
    transform: [f32; 16],
}

impl GltfInstance {
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
    /// **Not necessarily rigid.** That field additionally requires rotation and
    /// translation only, because the mesh shader transforms normals with the
    /// 3×3 part and no inverse-transpose; glTF nodes carry scale and this
    /// preserves it. Deciding what to do with a scaled node — bake it into the
    /// vertices, or grow the shader an inverse-transpose — belongs to the
    /// upload step, and losing the scale here would take the choice away.
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
    build(&document.document, &buffers, key)
}

/// Deserialize without `gltf`'s validation, which cannot be used — see
/// [`crate::gltf_check`].
fn parse(bytes: &[u8], key: &Path) -> Result<gltf::Gltf, StorageError> {
    // The same test `from_slice_without_validation` makes to decide whether
    // this is a container or bare JSON.
    if bytes.starts_with(b"glTF") {
        check_glb_header(bytes, key)?;
    }
    gltf::Gltf::from_slice_without_validation(bytes).map_err(|error| malformed(key, error))
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
    document: &gltf::Document,
    buffers: &[Vec<u8>],
    key: &Path,
) -> Result<GltfScene, StorageError> {
    let materials = document
        .materials()
        .map(|material| GpuMaterial {
            base_color: material.pbr_metallic_roughness().base_color_factor(),
            // **The factor only; `base_color_texture` is left untextured.**
            // glTF's `baseColorTexture` is an image this crate does not decode,
            // upload or own a page layer in — see `docs/backlog.md`'s "The
            // material table has both halves". Naming layer 0 is the honest
            // value for that: it is the page's white layer, so an imported
            // material shades with its factor and nothing else.
            ..GpuMaterial::UNTINTED
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
                log::warn!(
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
        instances: flatten(document, key)?,
        meshes,
        materials,
    })
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
        BASE_COLOR, BIN_CHUNK_BUFFER, EXTERNAL_BUFFER, INDICES, NORMALS, POSITIONS, TEX_COORDS,
        import_glb, import_gltf_text, replacing, triangle_bin, triangle_json,
    };

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

        assert_eq!(
            scene.materials(),
            [GpuMaterial {
                base_color: BASE_COLOR,
                ..GpuMaterial::UNTINTED
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

    #[test]
    fn a_material_with_no_factors_is_untinted_and_a_primitive_may_name_none() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            "\"pbrMetallicRoughness\": { \"baseColorFactor\": [0.25, 0.5, 0.75, 1.0] }",
            "\"doubleSided\": true",
        );
        let json = replacing(&json, ",\n      \"material\": 0", "");
        let scene = import_glb(&json).unwrap();

        assert_eq!(scene.materials(), [GpuMaterial::UNTINTED]);
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
}
