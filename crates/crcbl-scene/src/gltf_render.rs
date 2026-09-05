//! From an imported [`GltfScene`] to the [`SceneDesc`] a
//! [`ForwardRenderer`](crcbl_render::forward::ForwardRenderer) makes resident,
//! and the [`InstanceDesc`]s that place it.
//!
//! [`crate::gltf_import`] ends at host arrays: positions, indices, material
//! factors, encoded images and a flattened node hierarchy. The renderer starts
//! at a different vocabulary: vertex *bytes* in `crcbl_shaders::mesh`'s layout,
//! meshlet clusters, one square texture page of equal-sized layers, and a
//! material table whose rows index that page. Nothing joined the two, so no
//! `.gltf` or `.glb` in this engine had ever reached a pixel. This module is
//! that join.
//!
//! ```no_run
//! # use crcbl_assets::{DirSource, StorageError};
//! # use std::path::Path;
//! # fn load(source: &DirSource) -> Result<(), StorageError> {
//! let key = Path::new("meshes/helmet.glb");
//! let imported = crcbl_scene::import_gltf(source, key)?;
//! let converted = crcbl_scene::gltf_render::build_render_scene(&imported, key);
//!
//! // `converted.scene` goes to `ForwardRenderer::with_scene`, and each of
//! // `converted.instances` to `add_instance`. Anything the file asked for and
//! // did not get is in `converted.skipped`, already logged.
//! for skip in &converted.skipped {
//!     println!("{skip}");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Why the bridge is on this side of the seam
//!
//! `crcbl-render` must not depend on `crcbl-scene`:
//! [`crcbl_render::scene::Geometry`] says so in its own docs — the meshlet
//! build is a *bake* step precisely so the renderer does not link a glTF
//! parser. The dependency can therefore only run this way, and it is the shape
//! `crcbl-greybox` already has: host data that describes a scene, naming the
//! crate whose vocabulary a scene is described in. It is behind this crate's
//! `render` feature so the rest of the crate — which is all bake work ending at
//! host memory — stays usable without linking a renderer; see that manifest.
//!
//! # A primitive is a mesh here, because a material is per primitive
//!
//! glTF gives a *mesh* several primitives and a material to each;
//! [`InstanceDesc`] carries one mesh and one material. So the flattening is one
//! [`MeshDesc`] per glTF primitive and one instance per (node, primitive) pair —
//! a two-primitive mesh drawn by three nodes becomes two resident meshes and six
//! instances. [`RenderScene::instances`] is already in that expanded form.
//!
//! # The rig rides along with the vertices, because it cannot be re-derived
//!
//! A skinned primitive carries [`GltfPrimitive::joints`] and
//! [`GltfPrimitive::weights`], one of each per position. The vertices this
//! module emits are **not** always one per position: a primitive with no
//! `NORMAL` is de-indexed so that every triangle gets its own three vertices,
//! and nothing in the emitted run says which of the two shapes it got. So a
//! consumer holding only [`MeshDesc`]s cannot pair a vertex with the binding it
//! was imported with, and the pairing has to be made here, by the code that
//! does the expansion.
//!
//! [`RenderScene::origins`] is where it lands: one [`MeshOrigin`] per
//! [`SceneDesc::meshes`] row, in that order, holding the row's bindings in
//! **emitted-vertex order** and exactly as long as its vertices — empty for an
//! unskinned primitive. Each is a
//! [`crcbl_shaders::skinning::SkinBinding`], which is the record
//! [`SkinRange::bindings`](crcbl_render::skinning::SkinRange::bindings) takes,
//! so a caller passes the slice rather than converting it first. That is
//! [`mesh::GpuMaterial`]'s seam again — this crate already produces the
//! shader's own records instead of a parallel copy of them — and it adds no
//! dependency: the record lives in `crcbl-shaders`, which the bake side of this
//! crate links anyway.
//!
//! A primitive the conversion **skips** — one with no triangles, or one whose
//! meshlet build failed — produces no [`MeshDesc`], so it has no [`MeshOrigin`]
//! either and its bindings go with the geometry they indexed. There is no
//! second [`Skip`] for the rig: a binding array is indexed *by* the vertices
//! that are gone, so a message about it would report one loss twice, and the
//! [`Skip`] already pushed for the primitive is the record.
//!
//! A document mixing skinned and unskinned primitives therefore comes back as
//! one entry per primitive that was converted, each carrying a full run or an
//! empty one. [`MeshOrigin::mesh`] and [`MeshOrigin::primitive`] name the glTF
//! primitive the row came from, which is also the mapping from a converted mesh
//! back to its primitive that nothing exposed before — the hole a skip leaves
//! is exactly why reading it off the row index does not work.
//!
//! # What the bindings still do not come with
//!
//! A binding's joint indices are relative to the *skin the drawing node wears*,
//! and a skin is a property of a node rather than of a primitive. Neither
//! [`InstanceDesc`] nor [`MeshOrigin`] names the node it came from, so the
//! palette a [`SkinRange`](crcbl_render::skinning::SkinRange) needs beside its
//! bindings cannot yet be built from a [`RenderScene`] alone; it takes the
//! [`GltfScene`] the conversion read, whose [`GltfScene::instances`] carry the
//! node. Closing that is a change to what an instance is, not to what a mesh
//! is, and this module does not make it.
//!
//! # Material row 0 is the glTF default material
//!
//! [`SceneDesc::materials`]' first row is what an instance written without a
//! material id shades through, and glTF has its own name for that case: a
//! primitive with no `material` uses the specification's default, which is an
//! untinted, *fully rough conductor* — not
//! [`GpuMaterial::UNTINTED`](mesh::GpuMaterial::UNTINTED), which is a dielectric
//! at half roughness. So row 0 is the spec's default and the document's own
//! materials follow it: **material `n` of the file is row `n + 1`**.
//!
//! # What this does with textures, and what a page cannot hold
//!
//! A [`PageDesc`] is one image: every layer shares one square extent and one
//! format — `docs/backlog.md`'s `ArrayPages` entry says what still binds. Real
//! glTF textures are neither square nor equally sized, so they are
//! **resampled** onto a common extent — the largest side of the largest decoded
//! image, clamped to [`MAX_PAGE_EXTENT`] — by [`crcbl_render::mip::resample`],
//! the alpha-weighted box filter that averages in *linear* light and re-encodes
//! to sRGB, which is what the page's `Rgba8UnormSrgb` format means. The same
//! filter builds each layer's mip chain when the page is uploaded. The normal
//! page has a filter of its own; see below.
//!
//! The page costs `layers × extent² × 4` bytes on the device, and a third again
//! for the chain below level 0, so a document with many large textures is
//! expensive by construction. That is the single-page shape's limit rather than
//! this module's choice; the fix is the bindless form the backlog entry
//! describes.
//!
//! Only images a slot the shading actually reads can use are decoded:
//! `baseColorTexture` and `normalTexture`, which is what [`mesh::GpuMaterial`]'s
//! two live page columns are. A material with no texture in a slot, or one this
//! module could not decode, keeps
//! [`GpuMaterial::NO_PAGE`](mesh::GpuMaterial::NO_PAGE) in that column — the
//! out-of-band value the fragment stage reads as "no page", so the surface
//! shades by its factors alone rather than black and no page has to carry a
//! neutral layer for it to point at.
//!
//! **A normal map goes through its own resampler**, [`crcbl_render::mip::normal_resample`]:
//! no transfer curve, no alpha weighting, and a renormalise after the average.
//! The colour filter's sRGB decode is wrong by a gamma for the linear
//! tangent-space vectors a normal map holds — exact for an image the page does
//! not resize, and skewed for every averaged texel of one it does. An image both
//! slots name is therefore resampled twice, once through each filter, because
//! the two pages hold different kinds of value.
//!
//! # Everything unsupported is skipped loudly
//!
//! `docs/plan/sample/05-viewer.md`'s exit criterion is that a file from a tool
//! nobody curated either loads or says why not, naming the file, the feature and
//! the reason. That is a property of this layer, not of an application: by the
//! time a viewer holds a [`SceneDesc`] the evidence is gone. So every conversion
//! that could not be made appends a [`Skip`] **and** logs it at warning level,
//! and the conversion itself is infallible — a document nothing could be made of
//! yields an empty scene and a full list of why, not an error that hides the
//! nine things that did work behind the one that did not.

use std::borrow::Cow;
use std::fmt;
use std::path::Path;

use crcbl_render::scene::{
    Capacities, Geometry, InstanceDesc, MeshDesc, PAGE_EXTENT, PageDesc, ProbeGrid, SceneDesc,
};
use crcbl_shaders::mesh::{self, MeshVertex, VERTEX_STRIDE};
use crcbl_shaders::skinning::SkinBinding;
use crcbl_shaders::vertex::{TangentFrame, UvRange};
use glam::{Mat4, Vec3};

use crate::gltf_import::{GltfPrimitive, GltfScene, GltfTexture};
use crate::meshlet::build_meshlets;

/// The largest square page this module will build, in texels a side.
///
/// A page is one device image of `layers × extent² × 4` bytes, so the extent is
/// multiplied by every texture in the document: at this cap, sixteen textures is
/// 256 MiB. Chosen as the size most glTF base-colour maps are authored at, so
/// the common case resamples nothing; a document whose textures are larger is
/// downsampled to it rather than refused.
pub const MAX_PAGE_EXTENT: u32 = 2048;

/// The [`SceneDesc::materials`] row a primitive naming no material shades
/// through — the glTF specification's default material.
///
/// Zero because that is the row [`mesh::GpuInstance::default`] names; see the
/// [module docs](self).
pub const GLTF_DEFAULT_MATERIAL: usize = 0;

/// One glTF feature the conversion could not honour, and what was lost by it.
///
/// Every one of these was logged when it happened. It is collected as well so a
/// tool can list them beside the model rather than asking a user to read a log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skip {
    /// The glTF feature, spelled the way the specification spells it —
    /// `baseColorTexture`, `texCoord`, `mode`.
    pub feature: &'static str,
    /// Where in the document, in terms the file's author would recognise:
    /// `material 3 "paint"`, `mesh 0 primitive 2 "hull"`.
    pub at: String,
    /// What happened instead, and why. Written to be actionable on its own — the
    /// thing a viewer shows and a user acts on.
    pub why: String,
}

impl fmt::Display for Skip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: skipping {}: {}", self.at, self.feature, self.why)
    }
}

/// Everything [`build_render_scene`] produced: what to make resident, where to
/// put it, and what the file asked for and did not get.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderScene {
    /// Hand to
    /// [`ForwardRenderer::with_scene`](crcbl_render::forward::ForwardRenderer::with_scene).
    pub scene: SceneDesc<'static>,
    /// Hand each to
    /// [`ForwardRenderer::add_instance`](crcbl_render::forward::ForwardRenderer::add_instance).
    ///
    /// Already expanded per primitive, and already sized for by
    /// [`SceneDesc::capacities`] — see the [module docs](self).
    pub instances: Vec<InstanceDesc>,
    /// One entry per [`SceneDesc::meshes`] row, in the same order: the glTF
    /// primitive that row was made from, and the skin bindings its vertices
    /// carry.
    ///
    /// `origins[i]` describes `scene.meshes[i]`. See the [module docs](self)
    /// for why the bindings are produced here and what a skipped primitive
    /// leaves behind.
    pub origins: Vec<MeshOrigin>,
    /// Every [`Skip`], in the order they were found.
    ///
    /// Empty is the whole file arriving intact.
    pub skipped: Vec<Skip>,
}

/// Which glTF primitive one [`SceneDesc::meshes`] row was made from, and the
/// skin bindings its vertices carry.
///
/// A primitive the conversion skipped has no row and so no entry here, which is
/// why [`mesh`](Self::mesh) and [`primitive`](Self::primitive) are carried
/// rather than inferred from the position in [`RenderScene::origins`].
#[derive(Clone, Debug, PartialEq)]
pub struct MeshOrigin {
    /// Which of [`GltfScene::meshes`] the row's primitive belongs to.
    pub mesh: usize,
    /// Which of that mesh's [`GltfPrimitive`]s it is.
    pub primitive: usize,
    /// One [`SkinBinding`] per emitted vertex of the row, in the same order —
    /// the slice
    /// [`SkinRange::bindings`](crcbl_render::skinning::SkinRange::bindings)
    /// takes, and **empty** for a primitive the file gave no `JOINTS_0`.
    pub bindings: Vec<SkinBinding>,
}

/// Convert an imported document into what the forward renderer draws.
///
/// `key` is the document's own asset key. It is not read — it is what every
/// warning names, so a run over a directory of models says which file the
/// message is about.
///
/// Infallible by design: see the [module docs](self). A document with no usable
/// geometry yields a scene with no meshes, no instances, and a [`Skip`] per
/// thing that went wrong.
#[must_use]
pub fn build_render_scene(scene: &GltfScene, key: &Path) -> RenderScene {
    let mut skips = Skips {
        key,
        list: Vec::new(),
    };

    let (page, base_layers, normal_layers) = pack_page(scene, &mut skips);
    let materials = material_rows(scene, &base_layers, &normal_layers, &mut skips);
    let (meshes, origins, slots) = resident_meshes(scene, &mut skips);
    let instances = place_instances(scene, &slots, &mut skips);

    let vertices: usize = meshes.iter().map(vertex_count).sum();
    let indices: usize = meshes.iter().map(index_count).sum();

    RenderScene {
        scene: SceneDesc {
            capacities: capacities_for(
                vertices,
                indices,
                meshes.len(),
                materials.len(),
                &instances,
            ),
            meshes,
            materials,
            page,
            probes: ProbeGrid::default(),
        },
        instances,
        origins,
        skipped: skips.list,
    }
}

/// Collects [`Skip`]s and logs each as it arrives.
struct Skips<'a> {
    key: &'a Path,
    list: Vec<Skip>,
}

impl Skips<'_> {
    fn push(&mut self, feature: &'static str, at: String, why: String) {
        let skip = Skip { feature, at, why };
        crcbl_core::log::warn!("{}: {skip}", self.key.display());
        self.list.push(skip);
    }
}

/// How the document names a material, for a message.
///
/// The index alone: a [`GltfScene`] keeps no material names, because a row is a
/// [`mesh::GpuMaterial`] and the shader's record has no name field. The index is
/// still the thing a reader matches against the file's `materials` array.
fn material_label(material: usize) -> String {
    format!("material {material}")
}

/// How the document names a primitive, for a message.
fn primitive_label(scene: &GltfScene, mesh: usize, primitive: usize) -> String {
    match scene.meshes()[mesh].name() {
        Some(name) => format!("mesh {mesh} {name:?} primitive {primitive}"),
        None => format!("mesh {mesh} primitive {primitive}"),
    }
}

/// How the document names an image, for a message.
fn image_label(scene: &GltfScene, image: usize) -> String {
    match scene.images()[image].name() {
        Some(name) => format!("image {image} {name:?}"),
        None => format!("image {image}"),
    }
}

// ---------------------------------------------------------------------------
// The texture page
// ---------------------------------------------------------------------------

/// Decode every image the document actually uses, resample them onto one square
/// extent, and return the page with a map from image index to layer for each of
/// its two pages: base colour, then normal.
///
/// A map is `None` for an image that is not in that page — never wanted, never
/// decoded, or decoded and refused — and a material naming one keeps
/// [`GpuMaterial::NO_PAGE`](mesh::GpuMaterial::NO_PAGE) in that column.
///
/// **One extent covers both pages**, because a [`PageDesc`] has one; an image
/// larger than it is resampled down whichever slot named it.
fn pack_page(
    scene: &GltfScene,
    skips: &mut Skips<'_>,
) -> (PageDesc<'static>, Vec<Option<u32>>, Vec<Option<u32>>) {
    let base_wanted = wanted_images(
        scene.base_color_textures(),
        "baseColorTexture",
        "the surface shades with its base-colour factor alone",
        skips,
    );
    let normal_wanted = wanted_images(
        scene.normal_textures(),
        "normalTexture",
        "the surface shades with its interpolated normal and no normal map",
        skips,
    );

    // Decoded once per image rather than once per slot: a document naming one
    // image from both slots is legal, and decoding it twice would push the same
    // failure into `skips` twice and report one loss as two.
    let mut wanted = base_wanted.clone();
    for image in &normal_wanted {
        if !wanted.contains(image) {
            wanted.push(*image);
        }
    }
    let mut decoded: Vec<(usize, crcbl_sprite::load::Rgba8)> = Vec::new();
    for image in wanted {
        match decode_image(scene, image, skips) {
            Some(rgba) => decoded.push((image, rgba)),
            None => continue,
        }
    }

    // One extent for every layer, and it has to hold the largest thing in the
    // document or that texture loses detail no later pass can put back. The
    // floor is the extent the engine's own demo page uses, so a document with no
    // textures at all still gets a page every backend has drawn through.
    let extent = decoded
        .iter()
        .map(|(_, rgba)| rgba.width.max(rgba.height))
        .max()
        .unwrap_or(PAGE_EXTENT)
        .clamp(PAGE_EXTENT, MAX_PAGE_EXTENT);

    let mut page = PageDesc::opaque_white(extent);
    let mut base_layers = vec![None; scene.images().len()];
    let mut normal_layers = vec![None; scene.images().len()];
    for (image, rgba) in decoded {
        // The two pages are two device images, so one texel run cannot be
        // shared between them; an image both slots name is resampled per page.
        if base_wanted.contains(&image) {
            base_layers[image] = Some(page.push_layer(crcbl_render::mip::resample(
                &rgba.pixels,
                rgba.width,
                rgba.height,
                extent,
            )));
        }
        if normal_wanted.contains(&image) {
            // **`normal_resample`, not `resample`** — the filter that averages
            // the decoded vectors plainly and renormalises, where the one above
            // decodes an sRGB curve and weights by alpha. The same image in both
            // slots is therefore resampled twice with two filters, which is the
            // right answer rather than an inefficiency: the two pages hold
            // different kinds of value and neither filter is correct for the
            // other's.
            normal_layers[image] = Some(page.push_normal_layer(
                crcbl_render::mip::normal_resample(&rgba.pixels, rgba.width, rgba.height, extent),
            ));
        }
    }
    (page, base_layers, normal_layers)
}

/// Which of [`GltfScene::images`] one material slot's textures name, in the
/// order they are first asked for, with a [`Skip`] for each that samples a UV
/// set this importer does not read.
///
/// `feature` is the slot the way the specification spells it, and `instead` is
/// what the surface does without it — the two halves of the message that differ
/// between the base-colour and normal slots.
///
/// An image no live slot names is deliberately left out: a layer per unused
/// image is device memory for nothing.
fn wanted_images(
    textures: &[Option<GltfTexture>],
    feature: &'static str,
    instead: &str,
    skips: &mut Skips<'_>,
) -> Vec<usize> {
    let mut wanted: Vec<usize> = Vec::new();
    for (material, texture) in textures.iter().enumerate() {
        let Some(texture) = texture else { continue };
        if texture.tex_coord() != 0 {
            skips.push(
                "texCoord",
                material_label(material),
                format!(
                    "its {feature} samples TEXCOORD_{}, and this importer reads \
                     TEXCOORD_0 only; {instead}",
                    texture.tex_coord()
                ),
            );
            continue;
        }
        if !wanted.contains(&texture.image()) {
            wanted.push(texture.image());
        }
    }
    wanted
}

/// The PNG magic number: eight bytes that no other format starts with.
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The first three bytes of every JPEG: `SOI` then the first marker.
const JPEG_MAGIC: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// Decode one image to RGBA8, or record why it could not be.
///
/// The **bytes** decide the format, not the document's `mimeType`: a declared
/// type is a claim by whoever wrote the file, and a `.png` that is really a JPEG
/// is a thing exporters produce. The declaration is used only to make the
/// message say what the file claimed.
fn decode_image(
    scene: &GltfScene,
    image: usize,
    skips: &mut Skips<'_>,
) -> Option<crcbl_sprite::load::Rgba8> {
    let at = image_label(scene, image);
    let entry = &scene.images()[image];
    let bytes = match entry.bytes() {
        Ok(bytes) => bytes,
        Err(why) => {
            // Already warned about by the importer, which is where the read
            // failed; recorded here so one list holds everything a viewer shows.
            skips.push("image", at, why.to_owned());
            return None;
        }
    };

    if !bytes.starts_with(&PNG_MAGIC) {
        let found = if bytes.starts_with(&JPEG_MAGIC) {
            "JPEG".to_owned()
        } else {
            format!(
                "neither PNG nor JPEG (mimeType {})",
                entry.mime().unwrap_or("undeclared")
            )
        };
        skips.push(
            "image",
            at,
            format!(
                "its bytes are {found}, and this build decodes PNG only; every material \
                 naming it shades with its base-colour factor alone"
            ),
        );
        return None;
    }

    match crcbl_sprite::load::decode_png(bytes) {
        Ok(rgba) if rgba.width > 0 && rgba.height > 0 => Some(rgba),
        Ok(rgba) => {
            skips.push(
                "image",
                at,
                format!(
                    "it decodes to {}×{}, which has no texels",
                    rgba.width, rgba.height
                ),
            );
            None
        }
        Err(error) => {
            skips.push("image", at, format!("the PNG decoder refused it: {error}"));
            None
        }
    }
}

// ---------------------------------------------------------------------------
// The material table
// ---------------------------------------------------------------------------

/// The glTF specification's default material: untinted, and a fully rough
/// conductor.
///
/// Written out rather than spread from
/// [`GpuMaterial::UNTINTED`](mesh::GpuMaterial::UNTINTED), which is a dielectric
/// at half roughness: the two disagree, and the document's own default is what a
/// primitive naming no material means. The same constant
/// [`crate::gltf_import`]'s tests assert an unstyled material imports as.
const GLTF_DEFAULT_ROW: mesh::GpuMaterial = mesh::GpuMaterial {
    base_color: [1.0; 4],
    base_color_texture: mesh::GpuMaterial::NO_PAGE,
    metallic: 1.0,
    roughness: 1.0,
    tiling: mesh::GpuMaterial::TILING_AUTHORED,
    tile_metres: 1.0,
    emissive: [0.0; 3],
    // The default material names no texture of any kind, so every page column
    // carries the out-of-band value that means "none" and no page is read.
    normal_texture: mesh::GpuMaterial::NO_PAGE,
    // glTF's own default `normalTexture.scale`.
    normal_scale: 1.0,
    metallic_roughness_occlusion_texture: mesh::GpuMaterial::NO_PAGE,
    emissive_texture: mesh::GpuMaterial::NO_PAGE,
    // glTF's own default `alphaCutoff`, and `alphaMode` of `OPAQUE`, which is
    // the absence of every flag.
    alpha_cutoff: 0.5,
    flags: 0,
};

/// Which layer of one page a material's texture reference resolves to, or
/// `none` — that page's own "no layer" value — when it resolves to nothing.
///
/// **The UV set is re-checked here, not only in `pack_page`.** Two materials can
/// name one image with different `texCoord`s, and then the image *is* on the
/// page — put there for the one that asked for set 0. Reading the layer map
/// alone would hand it to the other one too, which would sample it with
/// coordinates from a set the file did not mean, silently, having already logged
/// that it would not.
///
/// An image that never reached the page falls back the same way, and `pack_page`
/// has already said why; this only makes the row honest about it.
fn layer_at(texture: Option<GltfTexture>, layers: &[Option<u32>], none: u32) -> u32 {
    match texture {
        Some(texture) if texture.tex_coord() == 0 => layers
            .get(texture.image())
            .copied()
            .flatten()
            .unwrap_or(none),
        Some(_) | None => none,
    }
}

/// The material table: the glTF default first, then the document's own rows with
/// their texture columns pointed at the page.
fn material_rows(
    scene: &GltfScene,
    base_layers: &[Option<u32>],
    normal_layers: &[Option<u32>],
    skips: &mut Skips<'_>,
) -> Vec<mesh::GpuMaterial> {
    let mut rows = Vec::with_capacity(scene.materials().len() + 1);
    rows.push(GLTF_DEFAULT_ROW);
    for (index, row) in scene.materials().iter().enumerate() {
        let layer = layer_at(
            scene.base_color_textures()[index],
            base_layers,
            mesh::GpuMaterial::NO_PAGE,
        );
        let normal_layer = layer_at(
            scene.normal_textures()[index],
            normal_layers,
            mesh::GpuMaterial::NO_PAGE,
        );
        if row.base_color[3] < 1.0 {
            skips.push(
                "alphaMode",
                material_label(index),
                format!(
                    "its base colour is {:.3} opaque and the forward pass draws every \
                     surface opaque; it will render solid",
                    row.base_color[3]
                ),
            );
        }
        rows.push(mesh::GpuMaterial {
            base_color_texture: layer,
            // `normal_scale` is already the document's: it is a material factor
            // and the importer put it on the row. Only the layer is this
            // module's to fill.
            normal_texture: normal_layer,
            ..*row
        });
    }
    rows
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// Where each glTF primitive ended up in [`SceneDesc::meshes`].
///
/// `slots[mesh][primitive]` is `None` for a primitive that was skipped, so an
/// instance of it places nothing rather than placing the primitive beside it.
type Slots = Vec<Vec<Option<usize>>>;

/// One [`MeshDesc`] per usable glTF primitive, in document order, and one
/// [`MeshOrigin`] beside each.
fn resident_meshes(
    scene: &GltfScene,
    skips: &mut Skips<'_>,
) -> (Vec<MeshDesc<'static>>, Vec<MeshOrigin>, Slots) {
    let mut meshes = Vec::new();
    let mut origins = Vec::new();
    let mut slots: Slots = Vec::with_capacity(scene.meshes().len());
    for (mesh_index, mesh) in scene.meshes().iter().enumerate() {
        let mut mesh_slots = Vec::with_capacity(mesh.primitives().len());
        for (primitive_index, primitive) in mesh.primitives().iter().enumerate() {
            let at = || primitive_label(scene, mesh_index, primitive_index);
            if primitive.indices().is_empty() {
                skips.push("primitive", at(), "it has no triangles to draw".to_owned());
                mesh_slots.push(None);
                continue;
            }
            // A textured material on geometry with no UVs. The primitive draws
            // perfectly well — every vertex gets `(0, 0)` — but the whole
            // surface then samples one texel of the page, which reads as a flat
            // tint nobody authored and is exactly the sort of thing a viewer's
            // user needs told rather than left to wonder at.
            if primitive.tex_coords().is_empty()
                && primitive
                    .material()
                    .and_then(|material| scene.base_color_textures()[material])
                    .is_some()
            {
                skips.push(
                    "TEXCOORD_0",
                    at(),
                    "its material carries a baseColorTexture and the primitive has no \
                     texture coordinates, so the whole surface samples one texel"
                        .to_owned(),
                );
            }

            let expanded = expand(primitive);
            let clusters = match build_meshlets(&expanded.positions, &expanded.indices) {
                Ok(build) => build.into_clusters(),
                Err(error) => {
                    skips.push(
                        "primitive",
                        at(),
                        format!("it could not be partitioned into meshlets: {error}"),
                    );
                    mesh_slots.push(None);
                    continue;
                }
            };
            mesh_slots.push(Some(meshes.len()));
            origins.push(MeshOrigin {
                mesh: mesh_index,
                primitive: primitive_index,
                bindings: expanded.bindings,
            });
            meshes.push(MeshDesc {
                label: Cow::Owned(at()),
                geometry: Geometry::Flat {
                    vertices: Cow::Owned(mesh::vertex_bytes(&expanded.vertices)),
                    uv_range: expanded.uv_range,
                    indices: Cow::Owned(expanded.indices),
                    clusters,
                    flags: if expanded.authored_tangents {
                        mesh::GpuMesh::MESH_AUTHORED_TANGENTS
                    } else {
                        0
                    },
                },
            });
        }
        slots.push(mesh_slots);
    }
    (meshes, origins, slots)
}

/// What [`expand`] made of one primitive.
struct Expanded {
    /// The emitted vertices' positions, which is what the meshlet build reads.
    positions: Vec<[f32; 3]>,
    /// The emitted vertices in the renderer's own record, one per entry of
    /// [`Expanded::positions`].
    vertices: Vec<MeshVertex>,
    /// The range every one of their UV lanes was quantised against: the bounds
    /// of the coordinates this primitive actually emitted.
    ///
    /// The *emitted* ones rather than `TEXCOORD_0` whole, because a primitive
    /// with no normals is de-indexed and a primitive with fewer coordinates
    /// than positions gets `[0, 0]` for the rest — so the accessor's own bounds
    /// are neither an upper nor a lower bound on what the vertices carry, and a
    /// range that did not contain a coordinate would clamp it to an edge.
    uv_range: UvRange,
    /// The triangle list over them.
    indices: Vec<u32>,
    /// One binding per emitted vertex, in the same order, or empty for an
    /// unskinned primitive — see the [module docs](self).
    bindings: Vec<SkinBinding>,
    /// Whether every emitted vertex's frame came from the file's own `TANGENT`
    /// — what [`mesh::GpuMesh::MESH_AUTHORED_TANGENTS`] claims.
    authored_tangents: bool,
}

/// A primitive's vertices in the renderer's own record, with the positions and
/// indices the meshlet build reads and the skin bindings that pair with them.
///
/// **A primitive with no `NORMAL` is de-indexed and given flat face normals**,
/// which is what the glTF specification requires of a client: "When normals are
/// not specified, client implementations MUST calculate flat normals". Flat
/// normals cannot be shared between faces, so the triangle list is expanded to
/// three vertices per triangle first — the same thing
/// `crcbl_shaders::mesh::cube_vertices` does by hand for the engine's own cube.
/// Doing anything else here is what makes an exported mesh light as a
/// featureless blob.
///
/// **The de-indexed path drops the file's `TANGENT` too**, and
/// [`Expanded::authored_tangents`] is `false` there for that reason. It is
/// taken only by a primitive with no `NORMAL`, whose normals are the face
/// normals computed just above; a tangent authored against the normals the file
/// carried is not the frame of one this module invented, and a mesh marked
/// [`mesh::GpuMesh::MESH_AUTHORED_TANGENTS`] on that basis would have the
/// fragment stage trust eight bytes that describe a different surface.
///
/// That expansion is why the bindings are produced here: on the indexed path an
/// emitted vertex is input vertex `n`, on the de-indexed one it is input vertex
/// `indices[n]`, and the emitted run carries no trace of which happened. So the
/// input vertex each emitted vertex came from is recorded as the expansion runs
/// and the bindings are read through it, rather than being re-derived by a
/// consumer that cannot tell the two shapes apart.
fn expand(primitive: &GltfPrimitive) -> Expanded {
    let uv_at = |index: usize| {
        primitive
            .tex_coords()
            .get(index)
            .copied()
            .unwrap_or([0.0, 0.0])
    };

    if !primitive.normals().is_empty() {
        let positions = primitive.positions().to_vec();
        let uvs: Vec<[f32; 2]> = (0..positions.len()).map(uv_at).collect();
        let uv_range = UvRange::from_uvs(&uvs);
        let vertices = positions
            .iter()
            .enumerate()
            .map(|(index, position)| {
                vertex(
                    *position,
                    primitive.normals()[index],
                    // `get` rather than an index: the importer refuses a
                    // `TANGENT` that is neither absent nor as long as
                    // `POSITION`, so this cannot be short — and a frame silently
                    // invented from a panic is not what a belt-and-braces read
                    // should cost.
                    primitive.tangents().get(index).copied(),
                    uvs[index],
                    &uv_range,
                )
            })
            .collect();
        let bindings = bindings_at(primitive, 0..positions.len());
        return Expanded {
            positions,
            vertices,
            uv_range,
            indices: primitive.indices().to_vec(),
            bindings,
            authored_tangents: !primitive.tangents().is_empty(),
        };
    }

    let mut positions = Vec::with_capacity(primitive.indices().len());
    // The flat-normal path emits its coordinates before it can quantise them,
    // for `Expanded::uv_range`'s reason: de-indexing changes which coordinates
    // are emitted and how often, and the range is the bounds of exactly those.
    let mut authored: Vec<([f32; 3], [f32; 2])> = Vec::with_capacity(primitive.indices().len());
    let mut sources = Vec::with_capacity(primitive.indices().len());
    for corners in primitive.indices().chunks_exact(3) {
        let corner = |at: usize| primitive.positions()[corners[at] as usize];
        let (a, b, c) = (
            Vec3::from(corner(0)),
            Vec3::from(corner(1)),
            Vec3::from(corner(2)),
        );
        // A degenerate triangle has no plane, so its cross product is zero and
        // `normalize` would be NaN. `+Y` is arbitrary and the triangle covers no
        // pixels either way; what matters is that no NaN reaches a vertex
        // buffer, where it would poison the mesh's cluster bounds too.
        let face = (b - a).cross(c - a).normalize_or(Vec3::Y);
        for (at, &index) in corners.iter().enumerate() {
            positions.push(corner(at));
            authored.push((face.to_array(), uv_at(index as usize)));
            // Pushed here rather than re-read off `indices` afterwards so that
            // the two cannot disagree: whatever this loop chooses to emit — a
            // trailing partial triangle is dropped by `chunks_exact`, for one —
            // the binding order is chosen by the same statement as the vertex
            // order.
            sources.push(index as usize);
        }
    }
    let bindings = bindings_at(primitive, sources.into_iter());
    let indices = (0..u32::try_from(positions.len()).unwrap_or(u32::MAX)).collect();
    let uvs: Vec<[f32; 2]> = authored.iter().map(|(_, uv)| *uv).collect();
    let uv_range = UvRange::from_uvs(&uvs);
    let vertices = positions
        .iter()
        .zip(&authored)
        // No tangent on this path even when the file wrote one. The normals
        // here are face normals this module just computed, because the file had
        // no `NORMAL` at all — and a tangent authored against the normals the
        // file *did* have describes the frame of a surface that is not this one.
        .map(|(position, (normal, uv))| vertex(*position, *normal, None, *uv, &uv_range))
        .collect();
    Expanded {
        positions,
        vertices,
        uv_range,
        indices,
        bindings,
        authored_tangents: false,
    }
}

/// One [`SkinBinding`] per entry of `order`, or no bindings at all for a
/// primitive the file gave no `JOINTS_0`.
///
/// `order` is the input vertex each emitted vertex came from — the identity on
/// the indexed path, `indices[n]` on the de-indexed one. The import refuses a
/// primitive carrying one of `JOINTS_0`/`WEIGHTS_0` without the other and one
/// whose attribute is not as long as `POSITION`, so an index good for a
/// position is good for a binding, and the empty case is all-or-nothing rather
/// than per-vertex.
///
/// The `u16` joint indices widen to the `u32` the GPU record holds;
/// [`SkinBinding::joints`] carries the argument for widening over packing.
fn bindings_at(primitive: &GltfPrimitive, order: impl Iterator<Item = usize>) -> Vec<SkinBinding> {
    if primitive.joints().is_empty() {
        return Vec::new();
    }
    order
        .map(|index| SkinBinding {
            joints: primitive.joints()[index].map(u32::from),
            weights: primitive.weights()[index],
        })
        .collect()
}

/// One vertex in `crcbl_shaders::mesh`'s record.
///
/// `tangent` is the primitive's `TANGENT` entry for this vertex, or `None` for
/// a primitive that carries none — which is what decides whether the frame is
/// the file's own or [`orthonormal_basis`](crcbl_shaders::vertex::orthonormal_basis)'
/// stand-in, and therefore whether the mesh may claim
/// [`MESH_AUTHORED_TANGENTS`](mesh::GpuMesh::MESH_AUTHORED_TANGENTS).
///
/// The albedo is white because glTF puts a primitive's colour in its *material*,
/// and the fragment stage multiplies the row's factor and the page texel into
/// this — so anything but `1.0` here would tint every imported surface by a
/// number the file never wrote. `COLOR_0` is not read by the importer, so there
/// is nothing else this could be.
fn vertex(
    position: [f32; 3],
    normal: [f32; 3],
    tangent: Option<[f32; 4]>,
    uv: [f32; 2],
    range: &UvRange,
) -> MeshVertex {
    let Some(tangent) = tangent else {
        // `from_normal` fills the frame with
        // `crcbl_shaders::vertex::orthonormal_basis`' stand-in, which agrees
        // with no UV parameterisation — so the mesh must not claim
        // `MESH_AUTHORED_TANGENTS`, and `docs/plan/43-render-standards.md` §2
        // says the fragment stage takes its screen-space derivative frame
        // instead until the MikkTSpace call `docs/backlog.md` carries lands.
        return MeshVertex::from_normal(position, normal, [1.0; 4], uv, range);
    };
    // glTF's `TANGENT` is `xyz` and a handedness in `w`, and the bitangent is
    // `w × cross(normal, tangent)` — the same order `QTangent::decode` and
    // `skinning.slang` reconstruct it in, so a frame that goes through the
    // encoder comes back out of either of them unchanged.
    let normal_vec = Vec3::from(normal);
    let tangent_vec = Vec3::new(tangent[0], tangent[1], tangent[2]);
    MeshVertex::from_frame(
        position,
        TangentFrame {
            tangent: tangent_vec.to_array(),
            bitangent: (normal_vec.cross(tangent_vec) * tangent[3]).to_array(),
            normal,
        },
        [1.0; 4],
        uv,
        range,
    )
}

/// How many vertices a description mesh holds, for the pool it has to fit in.
fn vertex_count(mesh: &MeshDesc<'_>) -> usize {
    match &mesh.geometry {
        Geometry::Flat { vertices, .. } => vertices.len() / VERTEX_STRIDE,
        Geometry::Dag { levels, .. } => {
            levels.iter().map(|level| level.len()).sum::<usize>() / VERTEX_STRIDE
        }
    }
}

/// How many indices a description mesh holds, on [`vertex_count`]'s terms.
fn index_count(mesh: &MeshDesc<'_>) -> usize {
    match &mesh.geometry {
        Geometry::Flat { indices, .. } => indices.len(),
        // Never produced here — this module builds no DAGs — and a level's
        // indices live in its clusters rather than an index array.
        Geometry::Dag { .. } => 0,
    }
}

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

/// How far two axis scales may differ before the transform counts as
/// non-uniform, as a ratio of the longest to the shortest.
///
/// Above one because an exporter writes `0.999999` for a scale of one often
/// enough that an exact test reports every file; well under any scale a person
/// authored deliberately.
const UNIFORM_SCALE_TOLERANCE: f32 = 1.001;

/// One [`InstanceDesc`] per (drawn node, usable primitive) pair.
fn place_instances(scene: &GltfScene, slots: &Slots, skips: &mut Skips<'_>) -> Vec<InstanceDesc> {
    let mut instances = Vec::new();
    for placement in scene.instances() {
        let transform = Mat4::from_cols_array(&placement.transform());
        let node = placement.node();
        let at = || match scene.nodes()[node].name() {
            Some(name) => format!("node {node} {name:?}"),
            None => format!("node {node}"),
        };
        if let Some(ratio) = non_uniform_scale(&transform) {
            // Not refused, and nothing about it is now wrong: both halves of
            // this warning have been closed in the shaders. `mesh.slang` and
            // `mesh_cluster.slang` take a normal through `normal_basis`, the
            // cofactor matrix, which is exact under any affine transform; and
            // `cluster_survives` skips the normal-cone test outright unless the
            // transform preserves angles, so no cluster facing the camera can
            // be rejected. What is left is throughput — such an instance is
            // culled by its bounding sphere alone. Reported because a file that
            // draws more slowly than its geometry says it should is worth
            // knowing about, and `docs/backlog.md` carries the bound that would
            // let the cone test run on it.
            skips.push(
                "scale",
                at(),
                format!(
                    "its world transform scales axes unequally (longest ÷ shortest = \
                     {ratio:.3}); it draws and lights correctly, and the mesh path \
                     culls it by its bounding sphere alone because a scaled cone is \
                     no longer a cone, so it costs more to draw than it should"
                ),
            );
        }

        let mesh_slots = &slots[placement.mesh()];
        let mut placed = 0usize;
        for (primitive_index, slot) in mesh_slots.iter().enumerate() {
            let Some(slot) = *slot else { continue };
            let material = scene.meshes()[placement.mesh()].primitives()[primitive_index]
                .material()
                .map_or(GLTF_DEFAULT_MATERIAL, |material| material + 1);
            instances.push(InstanceDesc {
                mesh: slot,
                material,
                transform,
            });
            placed += 1;
        }
        if placed == 0 {
            skips.push(
                "node",
                at(),
                format!(
                    "the mesh it draws ({}) has no primitive this renderer can make \
                     resident, so the node places nothing",
                    placement.mesh()
                ),
            );
        }
    }
    instances
}

/// How unequally a transform scales its axes, or `None` when it does not.
///
/// The scales are the lengths of the basis columns. Rotation does not change
/// them and translation is not in them, so this reads scale and nothing else.
fn non_uniform_scale(transform: &Mat4) -> Option<f32> {
    let lengths = [
        transform.x_axis.truncate().length(),
        transform.y_axis.truncate().length(),
        transform.z_axis.truncate().length(),
    ];
    let shortest = lengths.iter().copied().fold(f32::INFINITY, f32::min);
    let longest = lengths.iter().copied().fold(0.0f32, f32::max);
    // A zero-scale axis collapses the object to a plane; the ratio is infinite
    // and the normals are gone with it, which is worth the same warning.
    if shortest <= 0.0 {
        return Some(f32::INFINITY);
    }
    (longest / shortest > UNIFORM_SCALE_TOLERANCE).then_some(longest / shortest)
}

// ---------------------------------------------------------------------------
// Capacities
// ---------------------------------------------------------------------------

/// The pools this description needs, measured off the description itself.
///
/// Every one is device memory taken once and never grown, so the numbers are the
/// scene's own rather than [`Capacities::default`]'s — a model with a hundred
/// thousand vertices does not fit the default pool, and a two-triangle one
/// should not reserve for it. Each is floored at one: a pool of no bytes is not
/// a pool, and an empty document still has to produce a description a device
/// will accept.
///
/// [`Capacities::lights`] is the default's, because lights are not in a glTF
/// this importer reads and the caller adds its own.
fn capacities_for(
    vertices: usize,
    indices: usize,
    meshes: usize,
    materials: usize,
    instances: &[InstanceDesc],
) -> Capacities {
    let fit = |count: usize| u32::try_from(count).unwrap_or(u32::MAX).max(1);
    Capacities {
        vertices: fit(vertices),
        indices: fit(indices),
        meshes: fit(meshes),
        instances: fit(instances.len()),
        materials: fit(materials),
        probes: 0,
        ..Capacities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gltf_fixture::{
        BASE_COLOR, BIN_CHUNK_BUFFER, IMAGE_TEXELS, JOINTS, QUAD_INDICES, QUAD_JOINTS,
        QUAD_POSITIONS, QUAD_WEIGHTS, WEIGHTS, glb, import_glb, import_glb_bytes,
        import_rigged_glb, import_skinned_pair_glb, png_bytes, replacing, rigged_json,
        skinned_pair_bin, skinned_pair_json, textured_glb, textured_parts, triangle_json,
    };
    use crate::gltf_import::tests::{
        NORMAL_SCALE, TANGENTS, normal_mapped_glb, tangent_bin, tangent_glb, tangent_json,
    };
    use crcbl_shaders::vertex::QTangent;

    /// The key every conversion under test is named by, so a message that leaks
    /// into an assertion is recognisable.
    const KEY: &str = "meshes/model.glb";

    /// The image [`textured_glb`] carries in the cases that mean to succeed.
    fn image_png() -> Vec<u8> {
        png_bytes(2, 2, &IMAGE_TEXELS)
    }

    fn convert(bytes: &[u8]) -> RenderScene {
        let scene = import_glb_bytes(bytes).expect("the fixture imports");
        build_render_scene(&scene, Path::new(KEY))
    }

    fn convert_json(json: &str) -> RenderScene {
        let scene = import_glb(json).expect("the fixture imports");
        build_render_scene(&scene, Path::new(KEY))
    }

    /// One mesh's vertices, decoded back out of the bytes the pool would take.
    ///
    /// Read from the description rather than from a host-side copy, for
    /// `GpuMaterial::from_bytes`'s reason: the bytes are what the device sees,
    /// and a packer that wrote the right numbers in the wrong order is exactly
    /// what an assertion on the source values could not catch.
    fn vertices_of(mesh: &MeshDesc<'_>) -> Vec<MeshVertex> {
        let Geometry::Flat { vertices, .. } = &mesh.geometry else {
            panic!("this module builds no DAGs");
        };
        vertices
            .chunks_exact(VERTEX_STRIDE)
            .map(|bytes| MeshVertex::from_bytes(bytes.try_into().expect("one record")))
            .collect()
    }

    /// The range that mesh's UV lanes decode through, which the description
    /// carries beside them.
    fn uv_range_of(mesh: &MeshDesc<'_>) -> UvRange {
        let Geometry::Flat { uv_range, .. } = &mesh.geometry else {
            panic!("this module builds no DAGs");
        };
        *uv_range
    }

    /// The normal a vertex's frame decodes to.
    fn normal_of(vertex: &MeshVertex) -> [f32; 3] {
        vertex.qtangent.decode().normal
    }

    fn indices_of(mesh: &MeshDesc<'_>) -> Vec<u32> {
        let Geometry::Flat { indices, .. } = &mesh.geometry else {
            panic!("this module builds no DAGs");
        };
        indices.to_vec()
    }

    /// The per-mesh bits the description carries into
    /// [`mesh::GpuMesh::flags`].
    fn flags_of(mesh: &MeshDesc<'_>) -> u32 {
        let Geometry::Flat { flags, .. } = &mesh.geometry else {
            panic!("this module builds no DAGs");
        };
        *flags
    }

    /// The tangent and handedness a vertex's frame decodes to.
    fn tangent_of(vertex: &MeshVertex) -> ([f32; 3], f32) {
        (
            vertex.qtangent.decode().tangent,
            vertex.qtangent.handedness(),
        )
    }

    #[test]
    fn a_primitive_with_tangents_carries_the_authored_frame_and_says_so() {
        let converted = convert(&tangent_glb(TANGENTS.len()));
        assert_eq!(converted.skipped, []);

        let mesh = &converted.scene.meshes[0];
        assert_eq!(
            flags_of(mesh),
            mesh::GpuMesh::MESH_AUTHORED_TANGENTS,
            "these vertices came from the file's own TANGENT, so the fragment stage \
             may perturb in the interpolated frame rather than a derivative one",
        );

        for (vertex, authored) in vertices_of(mesh).iter().zip(TANGENTS) {
            let (tangent, handedness) = tangent_of(vertex);
            for (axis, want) in tangent.iter().zip(authored) {
                assert!(
                    (axis - want).abs() <= QTangent::MAX_COMPONENT_ERROR,
                    "the frame decodes to {tangent:?}, not the authored {authored:?}",
                );
            }
            assert_eq!(
                handedness, authored[3],
                "glTF's `w` is the handedness, and it is the half of the attribute \
                 that no amount of geometry can re-derive",
            );
        }
    }

    #[test]
    fn a_primitive_without_tangents_is_not_marked_as_carrying_them() {
        let converted = convert_json(&triangle_json(BIN_CHUNK_BUFFER));

        assert_eq!(
            flags_of(&converted.scene.meshes[0]),
            0,
            "every frame here is `orthonormal_basis`' stand-in, which agrees with no \
             UV parameterisation",
        );
    }

    /// **A de-indexed primitive is unmarked even when the file carried
    /// tangents**, and its frames are the stand-in.
    ///
    /// That path is taken only by a primitive with no `NORMAL`, whose normals
    /// are the face normals `expand` just computed — so a tangent authored
    /// against the normals the file *did* have is not a frame around these.
    #[test]
    fn a_de_indexed_primitive_is_unmarked_even_though_the_file_carried_tangents() {
        let json = replacing(
            &tangent_json(TANGENTS.len()),
            r#""POSITION": 0, "NORMAL": 1, "TANGENT": 4"#,
            r#""POSITION": 0, "TANGENT": 4"#,
        );
        let imported =
            import_glb_bytes(&glb(&json, Some(&tangent_bin()))).expect("the fixture imports");
        // Both halves of the fixture, asserted rather than assumed: no `NORMAL`
        // is what sends it down the de-indexing path, and a `TANGENT` is what
        // makes dropping one a thing that happened.
        let primitive = &imported.meshes()[0].primitives()[0];
        assert!(
            primitive.normals().is_empty(),
            "the fixture kept its NORMAL"
        );
        assert!(
            !primitive.tangents().is_empty(),
            "the fixture lost its TANGENT, so there is nothing here to drop"
        );

        let converted = build_render_scene(&imported, Path::new(KEY));
        assert_eq!(converted.skipped, []);

        let mesh = &converted.scene.meshes[0];
        assert_eq!(flags_of(mesh), 0);
        for vertex in &vertices_of(mesh) {
            let (tangent, handedness) = tangent_of(vertex);
            assert_eq!(
                handedness, 1.0,
                "`orthonormal_basis` is right-handed, where the fixture's own TANGENT \
                 is left-handed on two of its three vertices: {tangent:?}",
            );
        }
    }

    #[test]
    fn a_normal_map_lands_on_its_own_page_layer_and_the_row_names_it() {
        let converted = convert(&normal_mapped_glb());
        assert_eq!(converted.skipped, []);

        let page = &converted.scene.page;
        assert_eq!(page.extent(), 2, "the page is sized by the one image in it");
        assert_eq!(
            page.layers().len(),
            1,
            "the base-colour page keeps its white layer alone: this material names no \
             baseColorTexture, so nothing but the normal slot wanted the image",
        );
        assert_eq!(
            page.normal_layers().len(),
            2,
            "the neutral layer the type owns, then the image"
        );
        assert!(
            page.normal_layers()[0]
                .chunks_exact(4)
                .all(|texel| texel == PageDesc::NEUTRAL_NORMAL),
            "layer 0 is still the neutral texel the type burns",
        );
        assert_eq!(
            &page.normal_layers()[1][..],
            &IMAGE_TEXELS[..],
            "the image arrives texel for texel, in row-major order, unresampled"
        );

        assert_eq!(
            converted.scene.materials[1],
            mesh::GpuMaterial {
                base_color: BASE_COLOR,
                normal_texture: 1,
                normal_scale: NORMAL_SCALE,
                ..GLTF_DEFAULT_ROW
            },
            "the row keeps the importer's scale and gains the page layer",
        );
    }

    #[test]
    fn a_textured_material_lands_on_its_own_page_layer_and_the_row_names_it() {
        let converted = convert(&textured_glb(&image_png(), "image/png", 0));

        assert_eq!(
            converted.skipped,
            [],
            "nothing about this document is unsupported"
        );

        let page = &converted.scene.page;
        assert_eq!(page.extent(), 2, "the page is sized by the one image in it");
        assert_eq!(page.layers().len(), 2, "the white layer, then the image");
        assert!(
            page.layers()[0]
                .iter()
                .all(|&texel| texel == PageDesc::WHITE),
            "layer 0 is still the white layer `opaque_white` burns"
        );
        assert_eq!(
            &page.layers()[1][..],
            &IMAGE_TEXELS[..],
            "the image arrives texel for texel, in row-major order, unresampled"
        );

        // Row 0 is the glTF default and row 1 is the document's own, textured.
        assert_eq!(converted.scene.materials.len(), 2);
        assert_eq!(
            converted.scene.materials[GLTF_DEFAULT_MATERIAL],
            GLTF_DEFAULT_ROW
        );
        assert_eq!(
            converted.scene.materials[1],
            mesh::GpuMaterial {
                base_color: BASE_COLOR,
                base_color_texture: 1,
                ..GLTF_DEFAULT_ROW
            },
            "the document's material keeps its factors and gains the page layer"
        );
        assert_eq!(
            converted
                .instances
                .iter()
                .map(|it| it.material)
                .collect::<Vec<_>>(),
            [1],
            "the primitive names material 0 of the file, which is row 1 here"
        );
    }

    #[test]
    fn a_material_with_no_texture_names_no_page() {
        let converted = convert_json(&triangle_json(BIN_CHUNK_BUFFER));

        assert_eq!(converted.skipped, []);
        assert_eq!(
            converted.scene.page.layers().len(),
            1,
            "a document with no textures gets the white layer and nothing else"
        );
        assert_eq!(
            converted.scene.materials[1].base_color_texture,
            mesh::GpuMaterial::NO_PAGE
        );
    }

    #[test]
    fn a_jpeg_image_is_skipped_by_name_and_its_material_falls_back_to_white() {
        // Real JPEG magic in a document that claims PNG: the bytes decide, and
        // the message has to name what was actually there.
        let converted = convert(&textured_glb(
            &[0xFF, 0xD8, 0xFF, 0xE0, 0x00],
            "image/png",
            0,
        ));

        assert_eq!(converted.skipped.len(), 1, "{:?}", converted.skipped);
        let skip = &converted.skipped[0];
        assert_eq!(skip.feature, "image");
        assert_eq!(
            skip.at, "image 0 \"paint\"",
            "the message names the image the file named"
        );
        assert!(
            skip.why.contains("JPEG") && skip.why.contains("PNG only"),
            "the reason has to say what the bytes were and what this build takes: {}",
            skip.why
        );

        assert_eq!(converted.scene.page.layers().len(), 1);
        assert_eq!(
            converted.scene.materials[1].base_color_texture,
            mesh::GpuMaterial::NO_PAGE,
            "a row whose image never reached the page must name no page at all, not a \
             layer that is not there"
        );
        assert_eq!(
            converted.scene.materials[1].base_color, BASE_COLOR,
            "losing the texture does not lose the factor"
        );
    }

    #[test]
    fn a_second_uv_set_is_skipped_because_only_texcoord_0_is_imported() {
        let converted = convert(&textured_glb(&image_png(), "image/png", 1));

        assert_eq!(converted.skipped.len(), 1, "{:?}", converted.skipped);
        assert_eq!(converted.skipped[0].feature, "texCoord");
        assert_eq!(converted.skipped[0].at, "material 0");
        assert!(
            converted.skipped[0].why.contains("TEXCOORD_1"),
            "the reason names the set the file asked for: {}",
            converted.skipped[0].why
        );
        assert_eq!(
            converted.scene.page.layers().len(),
            1,
            "an image nothing can sample must not take a layer"
        );
        assert_eq!(
            converted.scene.materials[1].base_color_texture,
            mesh::GpuMaterial::NO_PAGE
        );
    }

    #[test]
    fn the_node_hierarchy_arrives_composed_into_one_instance_transform() {
        let converted = convert_json(&triangle_json(BIN_CHUNK_BUFFER));

        assert_eq!(converted.instances.len(), 1, "one node draws the one mesh");
        let instance = converted.instances[0];
        assert_eq!(instance.mesh, 0);
        assert_eq!(
            instance.transform,
            Mat4::from_translation(Vec3::new(10.0, 5.0, 0.0)),
            "the parent's +10 X and the child's +5 Y compose; either alone is a \
             different matrix"
        );
    }

    /// The skip is about the cull, not the shading: a normal goes through the
    /// cofactor matrix now, and a cone axis does not.
    #[test]
    fn a_non_uniform_scale_is_reported_and_the_object_is_still_placed() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            r#""translation": [10.0, 0.0, 0.0]"#,
            r#""scale": [1.0, 2.0, 4.0]"#,
        );
        let converted = convert_json(&json);

        assert_eq!(converted.skipped.len(), 1, "{:?}", converted.skipped);
        assert_eq!(converted.skipped[0].feature, "scale");
        assert_eq!(
            converted.skipped[0].at, "node 1 \"leaf\"",
            "the node named is the one that *draws*, because the transform reported is its \
             world transform — the scale was written on its parent and it inherited it"
        );
        assert!(
            converted.skipped[0].why.contains("4.000"),
            "the reason quotes how unequal the axes are: {}",
            converted.skipped[0].why
        );
        assert_eq!(
            converted.instances.len(),
            1,
            "a scaled node draws and shades correctly and may lose clusters to the cone \
             cull; dropping the node would lose all of it"
        );
    }

    #[test]
    fn a_uniform_scale_is_not_reported_because_it_moves_no_direction() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            r#""translation": [10.0, 0.0, 0.0]"#,
            r#""scale": [3.0, 3.0, 3.0]"#,
        );
        assert_eq!(convert_json(&json).skipped, []);
    }

    #[test]
    fn a_primitive_without_normals_is_de_indexed_and_given_flat_face_normals() {
        // The specification requires exactly this of a client: "When normals are
        // not specified, client implementations MUST calculate flat normals."
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            r#""POSITION": 0, "NORMAL": 1, "#,
            r#""POSITION": 0, "#,
        );
        let converted = convert_json(&json);
        assert_eq!(converted.skipped, []);

        let vertices = vertices_of(&converted.scene.meshes[0]);
        assert_eq!(
            vertices.len(),
            3,
            "a flat normal cannot be shared, so the triangle list is expanded"
        );
        assert_eq!(indices_of(&converted.scene.meshes[0]), [0, 1, 2]);
        // The fixture's triangle is (0,0,0), (1,0,0), (0,1,0) — counter-clockwise
        // seen from +Z, so its face normal is +Z and nothing else.
        for vertex in &vertices {
            let normal = normal_of(vertex);
            for (axis, want) in normal.iter().zip([0.0, 0.0, 1.0]) {
                assert!(
                    (axis - want).abs() <= QTangent::MAX_COMPONENT_ERROR,
                    "{vertex:?} decodes to {normal:?}"
                );
            }
        }
    }

    #[test]
    fn a_primitive_with_normals_keeps_its_own_vertices_and_uvs() {
        let converted = convert_json(&triangle_json(BIN_CHUNK_BUFFER));
        let vertices = vertices_of(&converted.scene.meshes[0]);

        assert_eq!(vertices.len(), 3, "an indexed mesh is not expanded");
        assert_eq!(vertices[1].position, [1.0, 0.0, 0.0]);
        let normal = normal_of(&vertices[1]);
        for (axis, want) in normal.iter().zip([0.0, 0.0, 1.0]) {
            assert!(
                (axis - want).abs() <= QTangent::MAX_COMPONENT_ERROR,
                "the frame decodes to {normal:?}"
            );
        }
        // Decoded through the description's own range, which is what a shader
        // does: a lane on its own is not a coordinate.
        let uv = uv_range_of(&converted.scene.meshes[0]).decode(vertices[1].uv0);
        for (axis, want) in uv.iter().zip([1.0, 0.0]) {
            assert!(
                (axis - want).abs() <= UvRange::MAX_RELATIVE_ERROR,
                "TEXCOORD_0 reaches the vertex: {uv:?}"
            );
        }
        assert_eq!(
            vertices[1].color, [255; 4],
            "the albedo is white, or every imported surface would be tinted by a number \
             the file never wrote"
        );
    }

    #[test]
    fn a_primitive_naming_no_material_shades_through_the_gltf_default_row() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            "\"indices\": 3,\n      \"material\": 0",
            "\"indices\": 3",
        );
        let converted = convert_json(&json);

        assert_eq!(converted.instances[0].material, GLTF_DEFAULT_MATERIAL);
        assert_eq!(
            converted.scene.materials[GLTF_DEFAULT_MATERIAL], GLTF_DEFAULT_ROW,
            "row 0 is glTF's own default — a fully rough conductor, not the engine's \
             neutral dielectric"
        );
        assert_ne!(
            GLTF_DEFAULT_ROW,
            mesh::GpuMaterial {
                base_color_texture: mesh::GpuMaterial::NO_PAGE,
                ..mesh::GpuMaterial::UNTINTED
            },
            "the two defaults disagree, which is the whole reason row 0 is spelled out"
        );
    }

    #[test]
    fn the_capacities_cover_the_description_they_were_measured_from() {
        let converted = convert(&textured_glb(&image_png(), "image/png", 0));
        let capacities = converted.scene.capacities;

        let vertices: usize = converted.scene.meshes.iter().map(vertex_count).sum();
        let indices: usize = converted.scene.meshes.iter().map(index_count).sum();
        assert_eq!(capacities.vertices as usize, vertices);
        assert_eq!(capacities.indices as usize, indices);
        assert_eq!(capacities.meshes as usize, converted.scene.meshes.len());
        assert_eq!(
            capacities.materials as usize,
            converted.scene.materials.len()
        );
        assert_eq!(capacities.instances as usize, converted.instances.len());
        assert_eq!(
            capacities.probes, 0,
            "a glTF this importer reads has no probe grid"
        );
    }

    #[test]
    fn an_empty_document_yields_a_description_a_device_would_still_accept() {
        let json = r#"{ "asset": { "version": "2.0" } }"#;
        let scene = import_glb(json).expect("a document of nothing is legal glTF");
        let converted = build_render_scene(&scene, Path::new(KEY));

        assert_eq!(converted.scene.meshes, []);
        assert_eq!(converted.instances, []);
        assert_eq!(converted.scene.page.extent(), PAGE_EXTENT);
        for capacity in [
            converted.scene.capacities.vertices,
            converted.scene.capacities.indices,
            converted.scene.capacities.meshes,
            converted.scene.capacities.instances,
            converted.scene.capacities.materials,
        ] {
            assert!(capacity >= 1, "a pool of no bytes is not a pool");
        }
    }

    #[test]
    fn a_second_uv_set_does_not_ride_in_on_a_layer_another_material_put_there() {
        // Two materials, one image: the first samples it with `TEXCOORD_0` and
        // earns a layer, the second asks for `TEXCOORD_1`. Reading the layer map
        // alone would hand the layer to the second one as well, and it would
        // sample it with coordinates the file did not mean — silently, having
        // already logged that it would not.
        let (json, bin) = textured_parts(&image_png(), "image/png", 0);
        let json = replacing(
            &json,
            r#""baseColorTexture": { "index": 0, "texCoord": 0 }
    }
  }],"#,
            r#""baseColorTexture": { "index": 0, "texCoord": 0 }
    }
  }, {
    "name": "second",
    "pbrMetallicRoughness": {
      "baseColorTexture": { "index": 0, "texCoord": 1 }
    }
  }],"#,
        );
        let converted = convert(&glb(&json, Some(&bin)));

        assert_eq!(
            converted.scene.page.layers().len(),
            2,
            "the image is on the page, put there by the material that can sample it"
        );
        assert_eq!(
            converted.scene.materials[1].base_color_texture, 1,
            "the TEXCOORD_0 material samples it"
        );
        assert_eq!(
            converted.scene.materials[2].base_color_texture,
            mesh::GpuMaterial::NO_PAGE,
            "the TEXCOORD_1 material must not, even though the layer exists"
        );
        assert_eq!(
            converted
                .skipped
                .iter()
                .map(|skip| skip.feature)
                .collect::<Vec<_>>(),
            ["texCoord"],
        );
    }

    #[test]
    fn a_textured_material_on_geometry_with_no_uvs_is_reported() {
        let (json, bin) = textured_parts(&image_png(), "image/png", 0);
        let json = replacing(&json, r#", "TEXCOORD_0": 2"#, "");
        let converted = convert(&glb(&json, Some(&bin)));

        assert_eq!(converted.skipped.len(), 1, "{:?}", converted.skipped);
        assert_eq!(converted.skipped[0].feature, "TEXCOORD_0");
        assert_eq!(converted.skipped[0].at, "mesh 0 \"triangle\" primitive 0");
        assert!(
            converted.skipped[0].why.contains("samples one texel"),
            "the reason says what the surface will actually look like: {}",
            converted.skipped[0].why
        );

        // Reported, not refused: the geometry is fine and the texture is on the
        // page, so the surface draws — flatly.
        assert_eq!(converted.scene.meshes.len(), 1);
        assert_eq!(converted.scene.materials[1].base_color_texture, 1);
        let range = uv_range_of(&converted.scene.meshes[0]);
        for vertex in vertices_of(&converted.scene.meshes[0]) {
            assert_eq!(range.decode(vertex.uv0), [0.0; 2]);
        }
    }

    // -- skin bindings -----------------------------------------------------

    /// The skinned pair, converted, with nothing about it unsupported.
    fn convert_pair(json: &str) -> RenderScene {
        let scene = import_skinned_pair_glb(json).expect("the fixture imports");
        build_render_scene(&scene, Path::new(KEY))
    }

    /// What the file bound vertex `index` of the quad with, in the record the
    /// skinning pass reads.
    fn quad_binding(index: usize) -> SkinBinding {
        SkinBinding {
            joints: QUAD_JOINTS[index].map(u32::from),
            weights: QUAD_WEIGHTS[index],
        }
    }

    /// The fixture is held to the format, not merely to what this crate
    /// accepts: the importer parses without validating, so a bad offset or a
    /// `POSITION` with no `min`/`max` would sail through it and leave every
    /// assertion below testing a document no other tool would load.
    #[test]
    fn the_skinned_pair_fixture_is_a_document_the_validator_accepts() {
        let bytes = glb(&skinned_pair_json(), Some(&skinned_pair_bin()));
        let gltf = gltf::Gltf::from_slice(&bytes).expect("the fixture should validate");
        assert_eq!(gltf.document.skins().count(), 1, "the quad has a skin");
        assert_eq!(
            gltf.document.meshes().count(),
            2,
            "a skinned one and a plain one"
        );
    }

    #[test]
    fn a_skinned_primitive_keeps_its_bindings_where_the_vertices_pass_straight_through() {
        let scene = import_rigged_glb(&rigged_json(BIN_CHUNK_BUFFER)).expect("it imports");
        let converted = build_render_scene(&scene, Path::new(KEY));

        assert_eq!(converted.skipped, [], "nothing here is unsupported");
        let vertices = vertices_of(&converted.scene.meshes[0]);
        assert_eq!(
            vertices.len(),
            JOINTS.len(),
            "the rigged fixture declares NORMAL, so its vertices are its positions"
        );

        assert_eq!(converted.origins.len(), converted.scene.meshes.len());
        let origin = &converted.origins[0];
        assert_eq!((origin.mesh, origin.primitive), (0, 0));
        assert_eq!(
            origin.bindings.len(),
            vertices.len(),
            "a run has to be exactly as long as the vertices it describes, or the \
             dispatch over it skins the wrong ones"
        );
        let expected: Vec<SkinBinding> = JOINTS
            .iter()
            .zip(WEIGHTS)
            .map(|(joints, weights)| SkinBinding {
                joints: joints.map(u32::from),
                weights,
            })
            .collect();
        assert_eq!(
            origin.bindings, expected,
            "the joints widen to u32 and the weights arrive unrenormalised, in the \
             document's own order"
        );
    }

    /// The case a consumer could not reconstruct: with no `NORMAL` the triangle
    /// list is de-indexed, so emitted vertex `n` is input vertex `indices[n]`
    /// and its binding has to follow it there.
    #[test]
    fn a_de_indexed_primitive_carries_the_binding_of_the_vertex_each_corner_names() {
        let converted = convert_pair(&skinned_pair_json());
        assert_eq!(converted.skipped, [], "{:?}", converted.skipped);

        let quad = &converted.origins[1];
        assert_eq!((quad.mesh, quad.primitive), (1, 0));
        let vertices = vertices_of(&converted.scene.meshes[1]);
        assert_eq!(
            vertices.len(),
            QUAD_INDICES.len(),
            "no NORMAL, so the list is expanded to a vertex per corner of every triangle"
        );
        assert_eq!(quad.bindings.len(), vertices.len());
        assert_ne!(
            quad.bindings.len(),
            QUAD_POSITIONS.len(),
            "a run left in position order would be shorter than the vertices, which is \
             the failure this fixture is shaped to produce"
        );

        for (emitted, &source) in QUAD_INDICES.iter().enumerate() {
            let source = source as usize;
            let position = QUAD_POSITIONS[source];
            assert_eq!(
                vertices[emitted].position, position,
                "emitted vertex {emitted} is corner {source}"
            );
            assert_eq!(
                quad.bindings[emitted],
                quad_binding(source),
                "so its binding is corner {source}'s, not vertex {emitted}'s"
            );
        }
    }

    #[test]
    fn an_unskinned_primitive_emits_no_bindings_rather_than_default_ones() {
        let converted = convert_json(&triangle_json(BIN_CHUNK_BUFFER));

        assert_eq!(converted.origins.len(), 1);
        assert_eq!(
            converted.origins[0].bindings,
            [],
            "a primitive with no JOINTS_0 gets an empty run; a run of zeroed bindings \
             would bind every vertex to joint 0 at no weight and collapse the mesh"
        );
    }

    #[test]
    fn a_document_mixing_skinned_and_unskinned_primitives_keeps_the_two_apart() {
        let converted = convert_pair(&skinned_pair_json());
        assert_eq!(converted.skipped, [], "{:?}", converted.skipped);

        assert_eq!(converted.scene.meshes.len(), 2);
        assert_eq!(converted.origins.len(), 2);
        assert_eq!(
            converted
                .origins
                .iter()
                .map(|origin| (origin.mesh, origin.primitive))
                .collect::<Vec<_>>(),
            [(0, 0), (1, 0)],
            "one entry per converted primitive, in document order"
        );
        for (index, origin) in converted.origins.iter().enumerate() {
            let vertices = vertices_of(&converted.scene.meshes[index]).len();
            assert!(
                origin.bindings.is_empty() || origin.bindings.len() == vertices,
                "run {index} is {} long against {vertices} vertices",
                origin.bindings.len()
            );
        }
        assert_eq!(
            converted.origins[0].bindings,
            [],
            "the plain triangle's run stays empty even though the document has a skin"
        );
        assert_eq!(converted.origins[1].bindings.len(), QUAD_INDICES.len());
    }

    /// A skipped primitive takes its rig with it and leaves no entry, which is
    /// why an entry names the primitive it came from instead of being found by
    /// its position in the document.
    #[test]
    fn a_skipped_primitive_leaves_no_run_and_the_entry_after_it_still_names_itself() {
        // Two indices is not a whole triangle, which the importer accepts and
        // the meshlet build refuses — so mesh 0 is dropped by the conversion
        // rather than a step earlier.
        let json = replacing(
            &skinned_pair_json(),
            r#"{ "bufferView": 6, "componentType": 5123, "count": 3, "type": "SCALAR" }"#,
            r#"{ "bufferView": 6, "componentType": 5123, "count": 2, "type": "SCALAR" }"#,
        );
        let converted = convert_pair(&json);

        assert_eq!(
            converted
                .skipped
                .iter()
                .map(|skip| (skip.feature, skip.at.as_str()))
                .collect::<Vec<_>>(),
            [
                ("primitive", "mesh 0 \"plain-triangle\" primitive 0"),
                ("node", "node 0 \"plain-triangle\""),
            ],
            "the primitive is reported where it failed and the node that drew it is \
             reported for placing nothing; neither message is about a rig, because the \
             one that was lost indexed vertices that are not there"
        );

        assert_eq!(converted.scene.meshes.len(), 1, "only the quad converted");
        assert_eq!(converted.origins.len(), 1);
        assert_eq!(
            (converted.origins[0].mesh, converted.origins[0].primitive),
            (1, 0),
            "row 0 came from mesh 1, so a caller reading a document index off a row \
             index would name the primitive that was skipped"
        );
        assert_eq!(
            converted.origins[0].bindings.len(),
            QUAD_INDICES.len(),
            "the surviving run is still the quad's, whole"
        );
        for (emitted, &source) in QUAD_INDICES.iter().enumerate() {
            assert_eq!(
                converted.origins[0].bindings[emitted],
                quad_binding(source as usize)
            );
        }
    }
}
