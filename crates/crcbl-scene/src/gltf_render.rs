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
//! A [`PageDesc`] is one image: every layer shares one square extent, one
//! format and no mips (`docs/backlog.md`, "The material table has both halves").
//! Real glTF textures are neither square nor equally sized, so they are
//! **resampled** onto a common extent — the largest side of the largest decoded
//! image, clamped to [`MAX_PAGE_EXTENT`] — by an alpha-weighted box filter that
//! averages in *linear* light and re-encodes to sRGB, which is what the page's
//! `Rgba8UnormSrgb` format means. Averaging the stored bytes instead would
//! darken every downscale.
//!
//! The page costs `layers × extent² × 4` bytes on the device, so a document with
//! many large textures is expensive by construction. That is the single-page
//! shape's limit rather than this module's choice; the fix is the bindless form
//! the backlog entry describes.
//!
//! Only images an untextured-capable slot can actually use are decoded:
//! `baseColorTexture` and nothing else, because [`mesh::GpuMaterial`] has one
//! texture column. A material with no texture, or one this module could not
//! decode, keeps [`PageDesc::UNTEXTURED_LAYER`] — the page's white layer, so the
//! surface shades by its factors alone rather than black.
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
use glam::{Mat4, Vec3};

use crate::gltf_import::{GltfPrimitive, GltfScene};
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
    /// Every [`Skip`], in the order they were found.
    ///
    /// Empty is the whole file arriving intact.
    pub skipped: Vec<Skip>,
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

    let (page, layer_of_image) = pack_page(scene, &mut skips);
    let materials = material_rows(scene, &layer_of_image, &mut skips);
    let (meshes, slots) = resident_meshes(scene, &mut skips);
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

/// Decode every base-colour image the document actually uses, resample them onto
/// one square extent, and return the page with a map from image index to layer.
///
/// The map is `None` for an image that is not in the page — never decoded, or
/// decoded and refused — and a material naming one keeps
/// [`PageDesc::UNTEXTURED_LAYER`].
fn pack_page(scene: &GltfScene, skips: &mut Skips<'_>) -> (PageDesc<'static>, Vec<Option<u32>>) {
    // Only images a material's `baseColorTexture` names: an image reached only
    // through a normal or emissive slot has nowhere to go in a one-column
    // material row, and a layer per unused image is device memory for nothing.
    let mut wanted: Vec<usize> = Vec::new();
    for (material, texture) in scene.base_color_textures().iter().enumerate() {
        let Some(texture) = texture else { continue };
        if texture.tex_coord() != 0 {
            skips.push(
                "texCoord",
                material_label(material),
                format!(
                    "its baseColorTexture samples TEXCOORD_{}, and this importer reads \
                     TEXCOORD_0 only; the surface shades with its base-colour factor alone",
                    texture.tex_coord()
                ),
            );
            continue;
        }
        if !wanted.contains(&texture.image()) {
            wanted.push(texture.image());
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
    let mut layer_of_image = vec![None; scene.images().len()];
    for (image, rgba) in decoded {
        layer_of_image[image] = Some(page.push_layer(resample(&rgba, extent)));
    }
    (page, layer_of_image)
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

/// Resample `rgba` onto a square `extent`, alpha-weighted and in linear light.
///
/// A box filter: each destination texel averages the source texels its own cell
/// covers. Where the source is *smaller* than the destination each cell covers
/// exactly one texel, so an upscale is nearest-neighbour — which is what the
/// page's sampler filters with anyway.
///
/// Two things it does that a naive average does not, and both are visible when
/// they are missing. The stored bytes are sRGB-encoded, so they are decoded to
/// linear before averaging and re-encoded after; averaging the encodings instead
/// darkens every downscale. And the colours are weighted by alpha, so a
/// transparent texel does not drag the colour of its neighbours towards whatever
/// happens to be stored under it.
fn resample(rgba: &crcbl_sprite::load::Rgba8, extent: u32) -> Vec<u8> {
    let (width, height) = (rgba.width, rgba.height);
    let mut out = vec![0u8; extent as usize * extent as usize * 4];
    for y in 0..extent {
        let (y0, y1) = source_span(y, extent, height);
        for x in 0..extent {
            let (x0, x1) = source_span(x, extent, width);
            let mut weighted = [0.0f32; 3];
            let mut plain = [0.0f32; 3];
            let mut alpha = 0.0f32;
            let mut texels = 0.0f32;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let at = (sy as usize * width as usize + sx as usize) * 4;
                    let a = f32::from(rgba.pixels[at + 3]) / 255.0;
                    for channel in 0..3 {
                        let linear = srgb_to_linear(rgba.pixels[at + channel]);
                        weighted[channel] += linear * a;
                        plain[channel] += linear;
                    }
                    alpha += a;
                    texels += 1.0;
                }
            }
            let at = (y as usize * extent as usize + x as usize) * 4;
            for channel in 0..3 {
                // A cell that is wholly transparent has no alpha to weight by,
                // and its colour is still the best guess for what is under it.
                let linear = if alpha > 0.0 {
                    weighted[channel] / alpha
                } else {
                    plain[channel] / texels
                };
                out[at + channel] = linear_to_srgb(linear);
            }
            out[at + 3] = quantise(alpha / texels);
        }
    }
    out
}

/// The half-open run of source texels destination texel `at` covers, on one
/// axis.
///
/// Never empty: when the destination is the larger of the two, the run is the
/// single texel the destination centre falls in.
fn source_span(at: u32, extent: u32, source: u32) -> (u32, u32) {
    let scale = |step: u32| (u64::from(step) * u64::from(source) / u64::from(extent)) as u32;
    let start = scale(at).min(source.saturating_sub(1));
    let end = scale(at + 1).clamp(start + 1, source);
    (start, end)
}

/// One sRGB-encoded byte as a linear value in `0..=1`.
///
/// The IEC 61966-2-1 transfer function, which is what `Rgba8UnormSrgb` decodes
/// with.
fn srgb_to_linear(value: u8) -> f32 {
    let encoded = f32::from(value) / 255.0;
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// The inverse of [`srgb_to_linear`], rounded to a byte.
fn linear_to_srgb(value: f32) -> u8 {
    let linear = value.clamp(0.0, 1.0);
    let encoded = if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    quantise(encoded)
}

/// A `0..=1` fraction as the nearest byte.
fn quantise(value: f32) -> u8 {
    // `clamp` first so the cast cannot saturate on a NaN-free out-of-range
    // input, and `+ 0.5` so it rounds rather than truncates.
    (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
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
    base_color_texture: PageDesc::UNTEXTURED_LAYER,
    metallic: 1.0,
    roughness: 1.0,
    tiling: mesh::GpuMaterial::TILING_AUTHORED,
    tile_metres: 1.0,
};

/// The material table: the glTF default first, then the document's own rows with
/// their texture column pointed at the page.
fn material_rows(
    scene: &GltfScene,
    layer_of_image: &[Option<u32>],
    skips: &mut Skips<'_>,
) -> Vec<mesh::GpuMaterial> {
    let mut rows = Vec::with_capacity(scene.materials().len() + 1);
    rows.push(GLTF_DEFAULT_ROW);
    for (index, row) in scene.materials().iter().enumerate() {
        let layer = match scene.base_color_textures()[index] {
            // **The UV set is re-checked here, not only in `pack_page`.** Two
            // materials can name one image with different `texCoord`s, and then
            // the image *is* on the page — put there for the one that asked for
            // set 0. Reading `layer_of_image` alone would hand it to the other
            // one too, which would sample it with coordinates from a set the
            // file did not mean, silently, having already logged that it would
            // not.
            Some(texture) if texture.tex_coord() == 0 => layer_of_image
                .get(texture.image())
                .copied()
                .flatten()
                // A texture that never reached the page: `pack_page` has
                // already said why, so this only makes the row honest about it.
                .unwrap_or(PageDesc::UNTEXTURED_LAYER),
            Some(_) | None => PageDesc::UNTEXTURED_LAYER,
        };
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

/// One [`MeshDesc`] per usable glTF primitive, in document order.
fn resident_meshes(scene: &GltfScene, skips: &mut Skips<'_>) -> (Vec<MeshDesc<'static>>, Slots) {
    let mut meshes = Vec::new();
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

            let (positions, vertices, indices) = expand(primitive);
            let clusters = match build_meshlets(&positions, &indices) {
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
            meshes.push(MeshDesc {
                label: Cow::Owned(at()),
                geometry: Geometry::Flat {
                    vertices: Cow::Owned(vertex_bytes(&vertices)),
                    indices: Cow::Owned(indices),
                    clusters,
                },
            });
        }
        slots.push(mesh_slots);
    }
    (meshes, slots)
}

/// A primitive's vertices in the renderer's own record, with the positions and
/// indices the meshlet build reads.
///
/// **A primitive with no `NORMAL` is de-indexed and given flat face normals**,
/// which is what the glTF specification requires of a client: "When normals are
/// not specified, client implementations MUST calculate flat normals". Flat
/// normals cannot be shared between faces, so the triangle list is expanded to
/// three vertices per triangle first — the same thing
/// `crcbl_shaders::mesh::cube_vertices` does by hand for the engine's own cube.
/// Doing anything else here is what makes an exported mesh light as a
/// featureless blob.
fn expand(primitive: &GltfPrimitive) -> (Vec<[f32; 3]>, Vec<MeshVertex>, Vec<u32>) {
    let uv_at = |index: usize| {
        primitive
            .tex_coords()
            .get(index)
            .copied()
            .unwrap_or([0.0, 0.0])
    };

    if !primitive.normals().is_empty() {
        let positions = primitive.positions().to_vec();
        let vertices = positions
            .iter()
            .enumerate()
            .map(|(index, position)| vertex(*position, primitive.normals()[index], uv_at(index)))
            .collect();
        return (positions, vertices, primitive.indices().to_vec());
    }

    let mut positions = Vec::with_capacity(primitive.indices().len());
    let mut vertices = Vec::with_capacity(primitive.indices().len());
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
            vertices.push(vertex(corner(at), face.to_array(), uv_at(index as usize)));
        }
    }
    let indices = (0..u32::try_from(positions.len()).unwrap_or(u32::MAX)).collect();
    (positions, vertices, indices)
}

/// One vertex in `crcbl_shaders::mesh`'s record.
///
/// The albedo is white because glTF puts a primitive's colour in its *material*,
/// and the fragment stage multiplies the row's factor and the page texel into
/// this — so anything but `1.0` here would tint every imported surface by a
/// number the file never wrote. `COLOR_0` is not read by the importer, so there
/// is nothing else this could be.
fn vertex(position: [f32; 3], normal: [f32; 3], uv: [f32; 2]) -> MeshVertex {
    MeshVertex {
        position: [position[0], position[1], position[2], 1.0],
        normal: [normal[0], normal[1], normal[2], 0.0],
        color: [1.0; 4],
        uv: [uv[0], uv[1], 0.0, 0.0],
    }
}

/// The vertices as the little-endian `f32` bytes a geometry pool holds —
/// position, normal, colour, uv per vertex, [`VERTEX_STRIDE`] bytes each, which
/// is exactly what `crcbl_shaders::mesh::cube_vertex_bytes` produces.
fn vertex_bytes(vertices: &[MeshVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_STRIDE);
    for vertex in vertices {
        for value in vertex
            .position
            .iter()
            .chain(&vertex.normal)
            .chain(&vertex.color)
            .chain(&vertex.uv)
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
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
            // Not refused: the geometry is right and only the shading is off,
            // which is a far better answer than an object missing from the
            // frame. `mesh.slang` normalises the interpolated normal, so a
            // *uniform* scale is exact and only this case is wrong.
            skips.push(
                "scale",
                at(),
                format!(
                    "its world transform scales axes unequally (longest ÷ shortest = \
                     {ratio:.3}), and the mesh shader transforms normals with the 3×3 part \
                     and no inverse-transpose; the object draws in the right place and \
                     lights wrongly"
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
/// The scales are the lengths of the basis columns, which is what the shader's
/// 3×3 multiply does to a normal. Rotation does not change them and translation
/// is not in them, so this reads scale and nothing else.
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
        BASE_COLOR, BIN_CHUNK_BUFFER, IMAGE_TEXELS, glb, import_glb, import_glb_bytes, png_bytes,
        replacing, textured_glb, textured_parts, triangle_json,
    };
    use crcbl_sprite::load::Rgba8;

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
            .map(|bytes| {
                let at = |offset: usize| {
                    f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
                };
                MeshVertex {
                    position: [at(0), at(4), at(8), at(12)],
                    normal: [at(16), at(20), at(24), at(28)],
                    color: [at(32), at(36), at(40), at(44)],
                    uv: [at(48), at(52), at(56), at(60)],
                }
            })
            .collect()
    }

    fn indices_of(mesh: &MeshDesc<'_>) -> Vec<u32> {
        let Geometry::Flat { indices, .. } = &mesh.geometry else {
            panic!("this module builds no DAGs");
        };
        indices.to_vec()
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
            page.layers()[PageDesc::UNTEXTURED_LAYER as usize]
                .iter()
                .all(|&texel| texel == PageDesc::WHITE),
            "layer 0 is the untextured white every unnamed material multiplies by"
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
    fn a_material_with_no_texture_stays_on_the_untextured_layer() {
        let converted = convert_json(&triangle_json(BIN_CHUNK_BUFFER));

        assert_eq!(converted.skipped, []);
        assert_eq!(
            converted.scene.page.layers().len(),
            1,
            "a document with no textures gets the white layer and nothing else"
        );
        assert_eq!(
            converted.scene.materials[1].base_color_texture,
            PageDesc::UNTEXTURED_LAYER
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
            PageDesc::UNTEXTURED_LAYER,
            "a row whose image never reached the page must name the white layer, not a \
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
            PageDesc::UNTEXTURED_LAYER
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
            "a scaled node lights wrongly and still draws; dropping it would lose the object"
        );
    }

    #[test]
    fn a_uniform_scale_is_not_reported_because_the_shader_normalises_the_normal() {
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
            assert_eq!(vertex.normal, [0.0, 0.0, 1.0, 0.0], "{vertex:?}");
        }
    }

    #[test]
    fn a_primitive_with_normals_keeps_its_own_vertices_and_uvs() {
        let converted = convert_json(&triangle_json(BIN_CHUNK_BUFFER));
        let vertices = vertices_of(&converted.scene.meshes[0]);

        assert_eq!(vertices.len(), 3, "an indexed mesh is not expanded");
        assert_eq!(vertices[1].position, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(vertices[1].normal, [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(
            vertices[1].uv,
            [1.0, 0.0, 0.0, 0.0],
            "TEXCOORD_0 reaches the vertex"
        );
        assert_eq!(
            vertices[1].color, [1.0; 4],
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
                base_color_texture: PageDesc::UNTEXTURED_LAYER,
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

    // -- the resampler -----------------------------------------------------

    fn rgba(width: u32, height: u32, pixels: &[u8]) -> Rgba8 {
        Rgba8 {
            width,
            height,
            pixels: pixels.to_vec(),
        }
    }

    #[test]
    fn resampling_onto_the_extent_an_image_already_has_changes_no_texel() {
        let source = rgba(2, 2, &IMAGE_TEXELS);
        assert_eq!(resample(&source, 2), IMAGE_TEXELS);
    }

    #[test]
    fn a_downscale_averages_in_linear_light_rather_than_in_stored_bytes() {
        // Two black and two white texels. Their average is a half in *linear*
        // light, and sRGB encodes a half at 188 — the mid grey a person would
        // pick. Averaging the stored bytes instead lands on 128, which is a
        // linear 0.216: the same picture, visibly darker, on every downscale.
        let checker = [
            0x00, 0x00, 0x00, 0xFF, //
            0xFF, 0xFF, 0xFF, 0xFF, //
            0xFF, 0xFF, 0xFF, 0xFF, //
            0x00, 0x00, 0x00, 0xFF,
        ];
        let one = resample(&rgba(2, 2, &checker), 1);

        assert_eq!(one.len(), 4);
        assert_eq!(one[3], 0xFF, "opaque in, opaque out");
        for channel in &one[..3] {
            assert!(
                (i32::from(*channel) - 188).abs() <= 1,
                "a half in linear light encodes to 188 and this is {channel}"
            );
            assert!(
                (i32::from(*channel) - 128).abs() > 8,
                "{channel} is the byte average, which is the bug this asserts against"
            );
        }
    }

    #[test]
    fn an_upscale_repeats_texels_rather_than_inventing_them() {
        // The page's sampler filters nearest and has no mips, so an upscale that
        // blended would only blur what the sampler is about to point-sample.
        let four = resample(&rgba(2, 2, &IMAGE_TEXELS), 4);
        let texel = |x: usize, y: usize| &four[(y * 4 + x) * 4..][..4];

        assert_eq!(
            texel(0, 0),
            &IMAGE_TEXELS[0..4],
            "red fills the top-left quarter"
        );
        assert_eq!(texel(1, 1), &IMAGE_TEXELS[0..4]);
        assert_eq!(texel(2, 0), &IMAGE_TEXELS[4..8], "green the top-right");
        assert_eq!(texel(0, 2), &IMAGE_TEXELS[8..12], "blue the bottom-left");
        assert_eq!(
            texel(3, 3),
            &IMAGE_TEXELS[12..16],
            "yellow the bottom-right"
        );
    }

    #[test]
    fn a_wholly_transparent_cell_keeps_its_colour_instead_of_dividing_by_zero() {
        let clear = [
            0xFF, 0x00, 0x00, 0x00, //
            0xFF, 0x00, 0x00, 0x00, //
            0xFF, 0x00, 0x00, 0x00, //
            0xFF, 0x00, 0x00, 0x00,
        ];
        assert_eq!(resample(&rgba(2, 2, &clear), 1), [0xFF, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn alpha_weighting_keeps_a_transparent_neighbour_from_tinting_an_opaque_one() {
        // One opaque red texel beside three fully transparent black ones. The
        // colour under a transparent texel is not colour anybody authored, so
        // the average must be red — an unweighted mean would drag it to a
        // quarter-strength red that no texel of the source holds.
        let fringe = [
            0xFF, 0x00, 0x00, 0xFF, //
            0x00, 0x00, 0x00, 0x00, //
            0x00, 0x00, 0x00, 0x00, //
            0x00, 0x00, 0x00, 0x00,
        ];
        let one = resample(&rgba(2, 2, &fringe), 1);
        assert_eq!(
            &one[..3],
            &[0xFF, 0x00, 0x00],
            "the colour is the opaque texel's"
        );
        assert_eq!(
            one[3], 64,
            "the coverage is a quarter, which is what alpha carries"
        );
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
            PageDesc::UNTEXTURED_LAYER,
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
        for vertex in vertices_of(&converted.scene.meshes[0]) {
            assert_eq!(vertex.uv, [0.0; 4]);
        }
    }
}
