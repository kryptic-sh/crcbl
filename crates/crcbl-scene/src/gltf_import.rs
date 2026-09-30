//! glTF 2.0 import: bytes through [`AssetSource`], geometry, material factors
//! and encoded images out.
//!
//! This is the first half of stage 6's step 3 (`docs/notes/tooling.md`). It
//! ends at host memory: there is no GPU pool upload, no image *decode* and no
//! mip generation here, and the types below are what the upload step consumes.
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
//! it at [`GpuMaterial::NO_PAGE`]**. That column is a layer of the renderer's page,
//! and which layer an image lands in is decided by whoever builds the page —
//! [`crate::gltf_render`], which owns both. What this module supplies instead is
//! the link the page builder needs: [`GltfScene::base_color_textures`] says
//! which of [`GltfScene::images`] each material's `baseColorTexture` names, and
//! [`GltfScene::images`] carries that image's **encoded** bytes.
//!
//! `normalTexture` rides that same seam: [`GltfScene::normal_textures`] says
//! which image each material's normal map is, and the row's `normal_texture`
//! column is left at [`GpuMaterial::NO_PAGE`] for the page builder to point.
//! Its `scale` is not a page layer and does go on the row — see `build`.
//!
//! `metallicRoughnessTexture`, `occlusionTexture` and `emissiveTexture` ride the
//! same seam, and they arrive on it since 2026-09-06:
//! [`GltfScene::metallic_roughness_textures`],
//! [`GltfScene::occlusion_textures`] and [`GltfScene::emissive_textures`] each
//! say which image a material's slot names, and each row's page column is left
//! at [`GpuMaterial::NO_PAGE`] for [`crate::gltf_render`] to point. The first
//! two land on one page between them — glTF packs occlusion, roughness and
//! metallic into one image's `r`, `g` and `b`, and the page builder is what
//! puts them there.
//!
//! **`occlusionTexture.strength` is not read.** glTF 2.0 §3.9.5 dials the
//! channel back towards one, [`GpuMaterial`] has no field to carry the factor
//! and `shaders/mesh.slang` shades at the specification's default of `1.0`, so
//! a document that writes a strength loses it silently — `docs/backlog.md`
//! holds what filling it would take.
//!
//! **`KHR_materials_ior` and `KHR_materials_specular` arrive as factors.** Every
//! term of the dielectric reflectance they define is a per-material constant,
//! so `dielectric_specular` multiplies them out into the row's
//! [`GpuMaterial::specular_f0`] and [`GpuMaterial::specular_f90`], and a
//! document naming neither gets glTF's default dielectric. The extension's two
//! textures are not read — they would be two more pages — and a document that
//! names one is told so, once.
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
//! A node that draws nothing never becomes an instance either, and a joint is
//! exactly that — so the node table is also where the *hierarchy* lives:
//! [`GltfNode::local_transform`] is a node's own placement and
//! [`GltfNode::children`] is what hangs off it, both straight from the
//! document. Composing them down a chain is what the instances already are, and
//! what a joint palette will be.
//!
//! # The rig is read, and nothing here plays it
//!
//! [`GltfScene::skins`] and [`GltfScene::clips`] are the document's `skins` and
//! `animations` arrays, and [`GltfPrimitive::joints`] and
//! [`GltfPrimitive::weights`] are the per-vertex binding that ties a mesh to
//! one. The joint *hierarchy* is not repeated in the skin, because a joint is a
//! node: it is [`GltfNode::children`] and [`GltfNode::local_transform`] over
//! the nodes [`GltfSkin::joints`] names. That is the whole of what
//! the animation pipeline calls its source stage: joint hierarchy, inverse
//! bind matrices, and sampled TRS curves, in host memory and in the document's
//! own units.
//!
//! **Reading is not playback.** Nothing in this crate poses a skeleton, blends
//! a clip or skins a vertex — a joint palette is `crcbl-anim`'s, and the
//! compute prepass that consumes one is the renderer's. A caller that imports a
//! rigged character and draws it gets its bind pose, exactly as before; what
//! changed is that the rig is now in the result rather than only in a warning.
//!
//! # What is parsed and dropped
//!
//! Vertex colours and `TEXCOORD_1` are read by nothing here and so are not
//! extracted, and neither is a second influence set — `JOINTS_1` and
//! `WEIGHTS_1` — so a vertex arrives bound to at most four joints.
//! [`GltfPrimitive::tangents`] *is* read; [`crate::gltf_render`] is what turns
//! it into a vertex frame.
//!
//! **Morph targets are dropped and warned about**, naming the file and the
//! count, rather than being silently absent from the result: a viewer showing a
//! mesh at its base shape has to be able to say why, and the only place that
//! knows the document had a morph target at all is here. A channel that
//! *animates* morph weights is still read — see [`GltfSamples::MorphWeights`] —
//! because a curve is a curve whether or not this importer extracted the shapes
//! it drives.
//!
//! `MSFT_lod` is read only where it sits on a **node**.
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
//! [`GpuMaterial::specular_f0`]: crcbl_shaders::mesh::GpuMaterial::specular_f0
//! [`GpuMaterial::specular_f90`]: crcbl_shaders::mesh::GpuMaterial::specular_f90

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crcbl_assets::{AssetSource, StorageError};
use crcbl_shaders::mesh::GpuMaterial;
use glam::Mat4;
use gltf::json::validation::Checked;
use gltf::material::AlphaMode;
use gltf::mesh::Mode;

use crate::gltf_check::{check_document, check_glb_header, malformed};

mod animation;
mod specular;

pub use animation::{GltfChannel, GltfClip, GltfInterpolation, GltfSamples};

use animation::read_clips;
use specular::{dielectric_specular, warn_specular_textures};

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
    normal_textures: Vec<Option<GltfTexture>>,
    metallic_roughness_textures: Vec<Option<GltfTexture>>,
    occlusion_textures: Vec<Option<GltfTexture>>,
    emissive_textures: Vec<Option<GltfTexture>>,
    images: Vec<GltfImage>,
    nodes: Vec<GltfNode>,
    instances: Vec<GltfInstance>,
    skins: Vec<GltfSkin>,
    clips: Vec<GltfClip>,
    unsupported_required: Vec<String>,
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

    /// Which image each material's `normalTexture` names, on
    /// [`base_color_textures`](Self::base_color_textures)' terms exactly:
    /// parallel to [`materials`](Self::materials), `None` where the material
    /// names no normal map, and every [`GltfTexture::image`] a valid index into
    /// [`images`](Self::images).
    ///
    /// The map's `scale` is **not** here: that is a material factor and it
    /// rides on the row, in [`GpuMaterial::normal_scale`]. What a row cannot
    /// carry is the *index*, which is a page layer — see the [module
    /// docs](self).
    #[inline]
    #[must_use]
    pub fn normal_textures(&self) -> &[Option<GltfTexture>] {
        &self.normal_textures
    }

    /// Which image each material's `pbrMetallicRoughness.metallicRoughnessTexture`
    /// names, on [`base_color_textures`](Self::base_color_textures)' terms
    /// exactly.
    ///
    /// glTF 2.0 §3.9.2 puts roughness in the image's `g` and metallic in its
    /// `b`, and leaves `r` to `occlusionTexture`. The two slots are therefore
    /// halves of one page rather than two pages, and
    /// [`crate::gltf_render`]'s `pack_page` is where they are packed together;
    /// the row's `metallic_roughness_occlusion_texture` column names the
    /// resulting layer.
    ///
    /// The factors beside it — `metallicFactor` and `roughnessFactor` — are on
    /// the row already, because a factor is a number and a layer is not; see
    /// the [module docs](self).
    #[inline]
    #[must_use]
    pub fn metallic_roughness_textures(&self) -> &[Option<GltfTexture>] {
        &self.metallic_roughness_textures
    }

    /// Which image each material's `occlusionTexture` names, on
    /// [`base_color_textures`](Self::base_color_textures)' terms exactly.
    ///
    /// The occlusion channel is the `r` of the same packed page
    /// [`metallic_roughness_textures`](Self::metallic_roughness_textures)
    /// fills the `g` and `b` of — and naming the *same image* from both slots
    /// is the convention glTF authors use, which is what makes one page enough.
    ///
    /// **`strength` is not here and is not anywhere**: see the [module
    /// docs](self).
    #[inline]
    #[must_use]
    pub fn occlusion_textures(&self) -> &[Option<GltfTexture>] {
        &self.occlusion_textures
    }

    /// Which image each material's `emissiveTexture` names, on
    /// [`base_color_textures`](Self::base_color_textures)' terms exactly.
    ///
    /// The `emissiveFactor` beside it — times
    /// `KHR_materials_emissive_strength` — is already on the row, in
    /// [`GpuMaterial::emissive`], which `emissive_radiance` resolves. This is
    /// the other half of that product, and it is a page layer.
    #[inline]
    #[must_use]
    pub fn emissive_textures(&self) -> &[Option<GltfTexture>] {
        &self.emissive_textures
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

    /// The document's `skins` array, in file order.
    ///
    /// Empty for the overwhelming majority of documents, which have no rig.
    /// Which skin a node wears is [`GltfNode::skin`] rather than anything here,
    /// and nothing in this crate poses a skeleton — see the [module
    /// docs](self).
    #[inline]
    #[must_use]
    pub fn skins(&self) -> &[GltfSkin] {
        &self.skins
    }

    /// The document's `animations` array, in file order, one [`GltfClip`] each.
    ///
    /// Empty for a document with no animations — and also for one whose
    /// `animations` array could not be deserialized at all, which is a warning
    /// rather than a refusal; see [`import_gltf`].
    #[inline]
    #[must_use]
    pub fn clips(&self) -> &[GltfClip] {
        &self.clips
    }

    /// Every name in the document's `extensionsRequired` that this importer
    /// does not implement, in file order.
    ///
    /// Empty is a document this importer can honour in full — which is not the
    /// same as one that arrived intact, since an *optional* extension it
    /// ignores leaves nothing here.
    ///
    /// **The document still loaded**: [`import_gltf`] draws it without them and
    /// warns, for the reason `warn_unsupported_extensions` states at length. So
    /// this is what a caller reads to say that what is on screen is not what
    /// the file describes — and it is the one thing in that warning a test can
    /// assert, which is what `apps/viewer`'s shelf manifest does.
    #[inline]
    #[must_use]
    pub fn unsupported_required_extensions(&self) -> &[String] {
        &self.unsupported_required
    }
}

/// One entry of the document's `skins` array: which nodes are its joints, and
/// what each joint's bind pose was.
///
/// The joint *hierarchy* is not repeated here. A joint is a node like any
/// other, so where it sits is [`GltfNode::local_transform`] and what hangs off
/// it is [`GltfNode::children`], and a second copy of the tree would be a
/// second answer to a question the node array already answers.
#[derive(Clone, Debug, PartialEq)]
pub struct GltfSkin {
    name: Option<String>,
    joints: Vec<usize>,
    inverse_binds: Vec<Mat4>,
    skeleton: Option<usize>,
}

impl GltfSkin {
    /// The name the document gave this skin, if it gave one.
    #[inline]
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The skin's joints, in the order the joint palette wants them: entry `n`
    /// is the node that joint `n` of this skin is.
    ///
    /// Every entry is a valid index into [`GltfScene::nodes`] — one that is not
    /// makes the document malformed rather than making this array shorter. The
    /// order is the document's, and it is load-bearing: [`GltfPrimitive::joints`]
    /// indexes *this* array, not the node array.
    #[inline]
    #[must_use]
    pub fn joints(&self) -> &[usize] {
        &self.joints
    }

    /// World → joint-local at bind time, one per entry of
    /// [`joints`](Self::joints) and exactly as long.
    ///
    /// **A skin that declares none gets identities**, which is what the
    /// specification defines their absence to mean: the matrices were
    /// pre-applied to the vertices. Materialising them here rather than
    /// answering `None` leaves the consumer one case instead of two, and the
    /// two cases would have had the same arithmetic.
    #[inline]
    #[must_use]
    pub fn inverse_binds(&self) -> &[Mat4] {
        &self.inverse_binds
    }

    /// The node the document names as this skeleton's common root, if it names
    /// one.
    ///
    /// A valid index into [`GltfScene::nodes`] where present. `None` is the
    /// ordinary case and the specification defines it as "resolve joint
    /// transforms against the scene root" — it is a hint about where the
    /// hierarchy is rooted, not a joint, and it need not be one.
    #[inline]
    #[must_use]
    pub const fn skeleton(&self) -> Option<usize> {
        self.skeleton
    }
}

/// One entry of the document's `nodes` array: what it is called, what it draws,
/// where it sits under its parent, which nodes hang off it, and the lower
/// detail levels it declares.
///
/// [`local_transform`](Self::local_transform) is the node's *own* placement and
/// not where it ends up: the other half is every parent above it, and that
/// composition is [`GltfScene::instances`]. Both are here because a node that
/// draws nothing never becomes an instance — a joint is exactly that — so the
/// composed array cannot answer where a skeleton's bones are.
#[derive(Clone, Debug, PartialEq)]
pub struct GltfNode {
    name: Option<String>,
    mesh: Option<usize>,
    skin: Option<usize>,
    local_transform: Mat4,
    children: Vec<usize>,
    lod_nodes: Vec<usize>,
}

impl GltfNode {
    /// The name the document gave this node, if it gave one.
    ///
    /// The `name_LOD1` half of topic 25's hand-authored precedence
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

    /// Which of [`GltfScene::skins`] deforms the mesh this node draws, if any.
    ///
    /// **The one fact that says an instance is skinned rather than scenery.**
    /// A rigged mesh is ordinary geometry until a node wears a skin over it,
    /// and the same mesh may appear under a second node with no skin at all —
    /// so a mesh's `JOINTS_0` does not decide it and cannot be made to.
    ///
    /// The joint indices in that mesh's bindings are relative to *this* skin's
    /// [`GltfSkin::joints`], which is why a palette cannot be built without it.
    ///
    /// [`Mat4::IDENTITY`] is what a consumer should use in place of
    /// [`local_transform`](Self::local_transform) when this is `Some`: the
    /// specification requires the transform of a skinned mesh's node to be
    /// ignored, the joints carrying the placement instead.
    #[inline]
    #[must_use]
    pub const fn skin(&self) -> Option<usize> {
        self.skin
    }

    /// This node's own transform, parent excluded — glTF's `matrix`, or its
    /// `translation`/`rotation`/`scale`, composed as `T * R * S`.
    ///
    /// [`Mat4::IDENTITY`] for a node that declares no transform at all, which
    /// is the specification's own default for every one of those fields and is
    /// most nodes.
    ///
    /// # Why a matrix and not the three components
    ///
    /// The document spells one transform two ways and the two conversions are
    /// not equally honest. `T`, `R` and `S` compose into a matrix exactly;
    /// a matrix decomposes back only if it *was* one — `gltf`'s own
    /// decomposition normalises the basis columns, so a document that authored
    /// a shear or a skew in `matrix` form would arrive here as three components
    /// that are not what it said. Storing the matrix reports every document
    /// this importer accepts, and the same expression is what
    /// [`GltfScene::instances`] composes with, so a node's local transform and
    /// its instance's world transform cannot come to differ.
    ///
    /// A consumer that wants the components back — an animation channel
    /// replaces one of them, leaving the other two at their rest values — asks
    /// [`Mat4::to_scale_rotation_translation`] for them. That decomposition is
    /// the exact one precisely where it is needed: the specification forbids
    /// the `matrix` spelling on any node an animation targets, so an animated
    /// node's matrix is a `T * R * S` by construction.
    #[inline]
    #[must_use]
    pub const fn local_transform(&self) -> Mat4 {
        self.local_transform
    }

    /// The nodes hanging off this one — the document's `children`, in file
    /// order.
    ///
    /// Every entry is a valid index into [`GltfScene::nodes`] — one that is not
    /// makes the document malformed rather than making this array shorter.
    /// Empty for a leaf.
    ///
    /// # Why children and not a parent
    ///
    /// `children` is what glTF stores, so this is a copy rather than an
    /// inference, and it is right for **every** node — including one no scene
    /// reaches, which glTF permits and [`GltfScene::nodes`] keeps. A `parent`
    /// field would not be: it is well defined only where the hierarchy is a
    /// forest, and that is proven by the walk behind [`GltfScene::instances`],
    /// which visits the nodes a scene reaches and no others. A node claimed as
    /// a child by two unreachable nodes has two parents, and a single-valued
    /// field would have had to pick one of them silently.
    ///
    /// Walking *up* — from a joint to the bones above it — is a consumer that
    /// wants the inverse of this, and it is one pass over
    /// [`GltfScene::nodes`] to build: each node's children name it as their
    /// parent.
    #[inline]
    #[must_use]
    pub fn children(&self) -> &[usize] {
        &self.children
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
/// [`normals`](GltfPrimitive::normals),
/// [`tangents`](GltfPrimitive::tangents),
/// [`tex_coords`](GltfPrimitive::tex_coords),
/// [`joints`](GltfPrimitive::joints) and
/// [`weights`](GltfPrimitive::weights) are empty when the file has none,
/// and otherwise have exactly as many entries as `positions`.
///
/// `joints` and `weights` are the two halves of one attribute and a file
/// carrying one without the other is malformed, so they are empty together or
/// filled together: a primitive is skinned or it is not.
#[derive(Clone, Debug, PartialEq)]
pub struct GltfPrimitive {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tangents: Vec<[f32; 4]>,
    tex_coords: Vec<[f32; 2]>,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
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

    /// `TANGENT`, or empty if the file has none.
    ///
    /// glTF stores the attribute as a `VEC3` **and a handedness**, in one
    /// `float4`: `xyz` is the unit surface tangent and `w` is `+1` or `-1`,
    /// the sign a client multiplies `cross(normal, tangent)` by to get the
    /// bitangent. Both halves are reported as the file wrote them — the sign
    /// is the half that cannot be re-derived, because a mirrored UV shell is
    /// geometrically indistinguishable from an unmirrored one.
    ///
    /// [`crcbl_shaders::vertex::TangentFrame`] is what this becomes, through
    /// [`crate::gltf_render`].
    #[inline]
    #[must_use]
    pub fn tangents(&self) -> &[[f32; 4]] {
        &self.tangents
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

    /// `JOINTS_0` — which four of a skin's joints each vertex is bound to — or
    /// empty if the file has none.
    ///
    /// These index [`GltfSkin::joints`], **not** [`GltfScene::nodes`]: they are
    /// joint numbers within whichever skin the node drawing this primitive
    /// wears. Widened to `u16` whichever of the two legal component types the
    /// file used.
    #[inline]
    #[must_use]
    pub fn joints(&self) -> &[[u16; 4]] {
        &self.joints
    }

    /// `WEIGHTS_0` — how much of each vertex each of its four joints owns —
    /// or empty if the file has none. Parallel to
    /// [`joints`](Self::joints).
    ///
    /// Normalised to `f32` whichever of the legal component types the file
    /// used: the specification permits weights stored as normalized bytes or
    /// shorts as well as floats. It also asks that a vertex's four weights sum
    /// to one, and this importer reports them as they were stored rather than
    /// renormalising — a file whose weights do not sum is one whose author
    /// should hear about it.
    #[inline]
    #[must_use]
    pub fn weights(&self) -> &[[f32; 4]] {
        &self.weights
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
/// # A document is not refused over an animation this importer cannot read
///
/// `gltf_json::animation::Target` makes `node` a required field, and
/// `KHR_animation_pointer` replaces it with a pointer into the document — so a
/// file using that extension fails to **deserialize at all**, and the failure
/// takes the whole `Root` with it. `AnimatedColorsCube` from the Khronos sample
/// suite is one, and it lists nothing in `extensionsRequired`, so the
/// specification says it has to load.
///
/// It has to load here in particular, because the extension is one
/// [`warn_unsupported_extensions`] already reports and carries on past. Losing
/// the mesh, the materials and the images of a file over an animation channel
/// aimed at something this crate has no curve type for is a trade nobody
/// benefits from — and it is the *whole* `animations` array that is lost, not
/// only the pointer channel, because `serde` refuses the array as a unit.
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
/// **Only `animations` is dropped, and only because it is the array that
/// refused to deserialize.** It is a real loss now that [`read_clips`] fills
/// [`GltfScene::clips`] — this document's clips are gone, and the warning below
/// says so — but it is the *smallest* one available: the alternative is losing
/// the document. No other array has that property, so no other array is
/// touched.
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

    // The file, the feature and the reason — the viewer's
    // exit criterion for a document that does not arrive whole. The count is
    // taken before the array is discarded, because afterwards nothing
    // downstream can say how many there were.
    crcbl_core::log::warn!(
        "{}: dropping {count} animation(s) this importer cannot deserialize — the document \
         uses an extension that changes an animation channel's shape, and serde refuses the \
         whole animations array over it, so the rest of the document is loaded without them \
         and this file's clips are empty",
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
    let unsupported_required = warn_dropped_features(document, key);
    let images = read_images(source, document, buffers, key)?;
    let base_color_textures = document
        .materials()
        .map(|material| {
            let info = material.pbr_metallic_roughness().base_color_texture();
            slot(
                document,
                info.map(|info| (info.texture(), info.tex_coord())),
            )
        })
        .collect();
    let normal_textures = document
        .materials()
        .map(|material| {
            let info = material.normal_texture();
            slot(
                document,
                info.map(|info| (info.texture(), info.tex_coord())),
            )
        })
        .collect();
    let metallic_roughness_textures = document
        .materials()
        .map(|material| {
            let info = material
                .pbr_metallic_roughness()
                .metallic_roughness_texture();
            slot(
                document,
                info.map(|info| (info.texture(), info.tex_coord())),
            )
        })
        .collect();
    let occlusion_textures = document
        .materials()
        .map(|material| {
            let info = material.occlusion_texture();
            slot(
                document,
                info.map(|info| (info.texture(), info.tex_coord())),
            )
        })
        .collect();
    let emissive_textures = document
        .materials()
        .map(|material| {
            let info = material.emissive_texture();
            slot(
                document,
                info.map(|info| (info.texture(), info.tex_coord())),
            )
        })
        .collect();
    let materials = document
        .materials()
        .map(|material| {
            // All three factors off one accessor, which is what makes them one
            // material rather than a colour and two numbers that could drift.
            let pbr = material.pbr_metallic_roughness();
            // **The alpha mode and the cutoff resolve together**, because the
            // cutoff is not a number a row carries in its own right: glTF says
            // `alphaCutoff` is to be ignored unless the mode is `MASK`, and
            // `GpuMaterial::alpha_cutoff` is read only where
            // `ALPHA_MODE_MASK` is set in `flags`. Deciding them in one place
            // is what stops a row claiming a threshold nothing will ever
            // compare against.
            //
            // **`MASK` arrives whole.** It is a cutout rather than a fade — a
            // fragment below the cutoff is simply not there — which needs no
            // sorting, no blend state and no pass of its own, so every stage
            // that draws the surface can honour it and `shaders/mesh.slang`
            // does: the shaded pass, the depth-only one behind the prepass and
            // the shadow atlas, and the reflective shadow map.
            //
            // **`BLEND` is recorded as `OPAQUE`, and the loss is chosen.**
            // Topic 43 §3 — "Transparency — absent,
            // structurally" — is that `crcbl-render`'s forward pass builds no
            // shaded pipeline with a `BlendState` at all, so there is nothing
            // a blend bit could select and no reading of it that comes out
            // right. Of the two wrong pictures available, `MASK` is the louder
            // one: a surface the author asked to fade smoothly would come back
            // with a hard-edged hole punched through wherever its alpha fell
            // under a cutoff nobody wrote, and holes read as damage where a
            // solid pane reads as a pane. Opaque loses the transparency and
            // nothing else, and `warn_dropped_features` names the count so the
            // loss is not silent.
            let (alpha_cutoff, alpha_flags) = match material.alpha_mode() {
                AlphaMode::Mask => (
                    // The document's threshold where it wrote one, and
                    // otherwise the specification's own default — which is the
                    // number `UNTINTED` already carries, so it is taken from
                    // there rather than spelled a second time.
                    material
                        .alpha_cutoff()
                        .unwrap_or(GpuMaterial::UNTINTED.alpha_cutoff),
                    GpuMaterial::ALPHA_MODE_MASK,
                ),
                AlphaMode::Opaque | AlphaMode::Blend => (GpuMaterial::UNTINTED.alpha_cutoff, 0),
            };
            // **`doubleSided` arrives whole too**, and it is the second mode
            // rather than a variation on the first: glTF 2.0 §3.9.6 asks for
            // back-face culling off *and* the back face's normals reversed, and
            // `crcbl-render` honours both — a `CullMode::None` twin of every
            // pipeline that draws the surface, and `mesh.slang`'s
            // `double_sided_normal`. A leaf card authored as a cutout is nearly
            // always double-sided, so the two bits together are what makes a
            // foliage material import as the thing it is.
            //
            // Absent means `false`, which is the specification's own default and
            // what `gltf::Material::double_sided` answers for a material that
            // wrote nothing.
            let flags = alpha_flags
                | if material.double_sided() {
                    GpuMaterial::DOUBLE_SIDED
                } else {
                    0
                };
            let (specular_f0, specular_f90) = dielectric_specular(&material);
            GpuMaterial {
                base_color: pbr.base_color_factor(),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                emissive: emissive_radiance(&material),
                // **The factors only; `base_color_texture` is left untextured.**
                // This column is a *page layer*, and which layer an image lands
                // in is known only to whoever builds the page. The document's
                // own answer — which image this material wants — is carried
                // beside the row in `base_color_textures` instead. `NO_PAGE` is
                // the honest value in the meantime: a row nobody re-pointed
                // reads no page and shades with its factors and nothing else.
                base_color_texture: GpuMaterial::UNTINTED.base_color_texture,
                // glTF texture coordinates are authored per vertex, so an
                // imported material samples the vertex UV — physical tiling is
                // the engine's own greybox mode, not something glTF describes.
                tiling: GpuMaterial::TILING_AUTHORED,
                tile_metres: GpuMaterial::UNTINTED.tile_metres,
                // Every page column on the same argument as `base_color_texture`
                // above: a layer is known only to whoever builds the page, and
                // the document's own answer is carried beside the row in
                // `normal_textures`.
                normal_texture: GpuMaterial::NO_PAGE,
                metallic_roughness_occlusion_texture: GpuMaterial::NO_PAGE,
                emissive_texture: GpuMaterial::NO_PAGE,
                // **The `normalTexture`'s scale is not on that list.** It is a
                // material factor, like the metallic and roughness ones above —
                // a number the document wrote and the shader multiplies — where
                // the *index* beside it is a page layer this module cannot know.
                // So the two halves of one glTF object split here, and this is
                // the half a row can hold.
                normal_scale: material.normal_texture().map_or(1.0, |info| info.scale()),
                // Both resolved above, where the argument for them is: the
                // document's `alphaMode` and `alphaCutoff`, with `BLEND`
                // flattened onto `OPAQUE`, and its `doubleSided`.
                alpha_cutoff,
                flags,
                // `KHR_materials_ior` and `KHR_materials_specular`, reduced to
                // the two numbers the lobe reads — see `dielectric_specular`.
                specular_f0,
                specular_f90,
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
        nodes: read_nodes(document, key)?,
        instances: flatten(document, key)?,
        meshes,
        materials,
        base_color_textures,
        normal_textures,
        metallic_roughness_textures,
        occlusion_textures,
        emissive_textures,
        images,
        skins: read_skins(document, buffers, key)?,
        clips: read_clips(document, buffers, key)?,
        unsupported_required,
    })
}

/// Warn, once per feature, about everything the document uses and this importer
/// does not read, and hand back the *required* extensions among them.
///
/// A log line rather than a field on [`GltfScene`] for most of these, because
/// there is nothing for a caller to *do* with a morph target this crate did not
/// parse — the value is that a mesh standing at its base shape has an
/// explanation somewhere. The counts are the document's own, and the key is
/// what makes a line actionable when a hundred files went past.
///
/// The one exception is the return value, which is
/// [`GltfScene::unsupported_required_extensions`]: a document drawn without an
/// extension it declared *required* is a document whose picture is wrong, and
/// that is a thing a caller reports and a test asserts rather than a thing only
/// a log knows.
///
/// Skins and animations used to be counted here and are now read; see
/// [`GltfScene::skins`] and [`GltfScene::clips`].
fn warn_dropped_features(document: &gltf::Document, key: &Path) -> Vec<String> {
    let root = document.as_json();
    let morph_targets: usize = root
        .meshes
        .iter()
        .flat_map(|mesh| &mesh.primitives)
        .map(|primitive| primitive.targets.as_ref().map_or(0, Vec::len))
        .sum();
    if morph_targets > 0 {
        crcbl_core::log::warn!(
            "{}: skipping {morph_targets} morph targets: this importer does not read them, \
             so every mesh draws at its base shape",
            key.display(),
        );
    }
    let unsupported_required = warn_unsupported_extensions(root, key);
    warn_specular_textures(document, key);

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

    // Counted off the JSON rather than through `gltf::Material::alpha_mode`,
    // which is an `unwrap` on a `Checked` and panics on a document naming a
    // mode the specification does not have — the same reason
    // `texture_has_an_image` reads the JSON. `MASK` is not counted here: it is
    // imported, not dropped.
    let blended = root
        .materials
        .iter()
        .filter(|material| material.alpha_mode == Checked::Valid(AlphaMode::Blend))
        .count();
    if blended > 0 {
        crcbl_core::log::warn!(
            "{}: drawing {blended} BLEND material(s) opaque: this renderer has no blended pass, \
             so a surface the document asked to fade through is solid",
            key.display(),
        );
    }

    unsupported_required
}

/// The linear radiance a material emits, as `GpuMaterial::emissive` wants it.
///
/// **glTF stores two things and this engine stores their product.**
/// `emissiveFactor` is a colour in `0..=1`, which cannot express a surface
/// brighter than white, and `KHR_materials_emissive_strength` is the multiplier
/// that lifts it — so the pair is one radiance split in two, and a shader wants
/// it whole. The scene target is `Rgba16Float`, so the product is representable
/// however large the strength is.
///
/// **A document with no extension multiplies by one**, which is what the
/// extension's own specification says its absence means, so an ordinary
/// emissive material imports at exactly its factor.
fn emissive_radiance(material: &gltf::Material<'_>) -> [f32; 3] {
    let strength = material.emissive_strength().unwrap_or(1.0);
    material.emissive_factor().map(|channel| channel * strength)
}

/// Every `(asset key, feature)` pair a warning has already named in this
/// process.
///
/// A scene or view that imports the same asset again would otherwise repeat
/// the same line each time — EW imports its range rifle on every range load —
/// and a log full of one line is a log nobody reads. The first import says it;
/// the rest are the same document and the same answer.
static REPORTED: Mutex<BTreeSet<(PathBuf, String)>> = Mutex::new(BTreeSet::new());

/// Whether this is the first time the process reports `feature` for `key`, and
/// record that it now has.
///
/// The poison is stepped over rather than propagated: nothing but this one
/// insert ever holds the lock, so a poisoned set is still a set of names
/// already said, and the worst it can cost is a line said twice or not at all.
fn first_report(key: &Path, feature: &str) -> bool {
    REPORTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert((key.to_path_buf(), feature.to_owned()))
}

/// One material slot's reference to an image, as [`GltfScene`] carries it.
///
/// `texture` is the slot's own `(texture, texCoord)` pair, or [`None`] where
/// the material names nothing in that slot. All five slots resolve through
/// here, so the rule below is written once rather than five times.
///
/// **`Texture::source` panics on a texture that has none**, so the filter is
/// load-bearing rather than tidy — see `gltf_check::TEXTURE_SOURCE_ABSENT`.
/// A texture whose image an extension supplies becomes a material with no
/// texture in that slot, which is the same place an undecodable image lands.
fn slot(
    document: &gltf::Document,
    texture: Option<(gltf::Texture<'_>, u32)>,
) -> Option<GltfTexture> {
    let (texture, tex_coord) = texture?;
    texture_has_an_image(document, texture.index()).then(|| GltfTexture {
        image: texture.source().index(),
        tex_coord,
    })
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

/// The glTF extensions this importer implements.
///
/// `lod_resolve` reads `MSFT_lod`, [`emissive_radiance`] the emissive strength,
/// and `dielectric_specular` the IOR and the specular factors. Everything else a document declares is ignored, so
/// this list is what [`warn_unsupported_extensions`] measures against.
const IMPLEMENTED_EXTENSIONS: &[&str] = &[
    "MSFT_lod",
    "KHR_materials_emissive_strength",
    "KHR_materials_ior",
    "KHR_materials_specular",
];

/// Name every extension the document declares and this importer does not
/// implement, `extensionsRequired` louder than `extensionsUsed`, and hand the
/// required ones back for [`GltfScene::unsupported_required_extensions`].
///
/// **The file, the feature and the reason**, which is what
/// the viewer's exit criteria ask of a document that did
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
fn warn_unsupported_extensions(root: &gltf::json::Root, key: &Path) -> Vec<String> {
    let unsupported = |names: &[String]| -> Vec<String> {
        names
            .iter()
            .filter(|name| !IMPLEMENTED_EXTENSIONS.contains(&name.as_str()))
            .cloned()
            .collect()
    };

    // **Each name is said once per asset per process** — see `REPORTED`. The
    // returned list is not deduplicated: it is this document's answer, and a
    // second import of the file is owed the same one.
    let unreported = |names: &[String]| -> Vec<String> {
        names
            .iter()
            .filter(|name| first_report(key, name))
            .cloned()
            .collect()
    };

    let required = unsupported(&root.extensions_required);
    let required_unreported = unreported(&required);
    if !required_unreported.is_empty() {
        crcbl_core::log::warn!(
            "{}: this document REQUIRES {}, which this importer does not implement — it is \
             drawn without them, so what is on screen is not what the file describes",
            key.display(),
            required_unreported.join(", "),
        );
    }

    // Everything used but not required, and not already named above: the
    // document itself says these are optional, so the line is quieter.
    let optional: Vec<String> = unsupported(&root.extensions_used)
        .into_iter()
        .filter(|name| !required.contains(name))
        .collect();
    let optional_unreported = unreported(&optional);
    if !optional_unreported.is_empty() {
        crcbl_core::log::warn!(
            "{}: ignoring {}, which this importer does not implement — the document lists them \
             as optional, so the rest of it is unaffected",
            key.display(),
            optional_unreported.join(", "),
        );
    }

    required
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

/// The document's `nodes` array: name, mesh, skin, local transform, children
/// and `MSFT_lod` each.
fn read_nodes(document: &gltf::Document, key: &Path) -> Result<Vec<GltfNode>, StorageError> {
    let nodes = document.nodes().len();
    document
        .nodes()
        .map(|node| {
            Ok(GltfNode {
                name: node.name().map(str::to_owned),
                // `check_nodes` put this node's mesh and every one of its
                // children inside the document, so the typed `mesh()` and
                // `children()` below cannot reach the `unwrap` each has behind
                // it.
                mesh: node.mesh().map(|mesh| mesh.index()),
                // `check_nodes` bounds-checks this one too, so the `unwrap`
                // behind the typed `skin()` is out of reach as well.
                skin: node.skin().map(|skin| skin.index()),
                local_transform: local_transform(&node),
                children: node.children().map(|child| child.index()).collect(),
                lod_nodes: read_msft_lod(&node, nodes, key)?,
            })
        })
        .collect()
}

/// One node's own transform, as a matrix.
///
/// The single place a node's local placement is read, so
/// [`GltfNode::local_transform`] and the world transforms [`flatten`] composes
/// are the same arithmetic rather than two spellings of it. Total on any
/// document: `gltf` answers the `matrix` form with the file's own matrix and
/// the `translation`/`rotation`/`scale` form with `T * R * S` over the
/// specification's defaults for whichever of the three the file left out.
fn local_transform(node: &gltf::Node<'_>) -> Mat4 {
    Mat4::from_cols_array_2d(&node.transform().matrix())
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
    node: &gltf::Node<'_>,
    nodes: usize,
    key: &Path,
) -> Result<Vec<usize>, StorageError> {
    let index = node.index();
    let Some(extension) = node.extension_value("MSFT_lod") else {
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
    let tangents: Vec<[f32; 4]> = reader
        .read_tangents()
        .map(|read| read.collect())
        .unwrap_or_default();
    let tex_coords: Vec<[f32; 2]> = reader
        .read_tex_coords(0)
        .map(|read| read.into_f32().collect())
        .unwrap_or_default();
    // `into_u16` and `into_f32` rather than a hand-read: the spec stores
    // `JOINTS_0` as unsigned bytes or shorts and `WEIGHTS_0` as floats or
    // *normalized* integers, and un-normalising a weight is the step a
    // hand-read gets wrong.
    let joints: Vec<[u16; 4]> = reader
        .read_joints(0)
        .map(|read| read.into_u16().collect())
        .unwrap_or_default();
    let weights: Vec<[f32; 4]> = reader
        .read_weights(0)
        .map(|read| read.into_f32().collect())
        .unwrap_or_default();
    if joints.is_empty() != weights.is_empty() {
        return Err(malformed(
            key,
            format!(
                "{at} has {} JOINTS_0 and {} WEIGHTS_0 values, and a skinned \
                 primitive needs both",
                joints.len(),
                weights.len()
            ),
        ));
    }
    for (what, found) in [
        ("NORMAL", normals.len()),
        ("TANGENT", tangents.len()),
        ("TEXCOORD_0", tex_coords.len()),
        ("JOINTS_0", joints.len()),
        ("WEIGHTS_0", weights.len()),
    ] {
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
        tangents,
        tex_coords,
        joints,
        weights,
        indices,
        material: primitive.material().index(),
    })
}

/// The document's `skins` array, joints and bind matrices each.
///
/// Every index here has already been checked against the node array by
/// [`crate::gltf_check`], which is what makes `gltf`'s own accessors — each an
/// `unwrap` on an index out of the file — safe to call.
fn read_skins(
    document: &gltf::Document,
    buffers: &[Vec<u8>],
    key: &Path,
) -> Result<Vec<GltfSkin>, StorageError> {
    document
        .skins()
        .map(|skin| {
            let reader = skin.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
            let joints: Vec<usize> = skin.joints().map(|node| node.index()).collect();
            // No `inverseBindMatrices` means "already applied to the vertices",
            // which is the identity — see `GltfSkin::inverse_binds`.
            let inverse_binds: Vec<Mat4> = match reader.read_inverse_bind_matrices() {
                Some(read) => read.map(|cols| Mat4::from_cols_array_2d(&cols)).collect(),
                None => vec![Mat4::IDENTITY; joints.len()],
            };
            if inverse_binds.len() != joints.len() {
                return Err(malformed(
                    key,
                    format!(
                        "skin {} has {} joints and {} inverse bind matrices",
                        skin.index(),
                        joints.len(),
                        inverse_binds.len()
                    ),
                ));
            }
            Ok(GltfSkin {
                name: skin.name().map(str::to_owned),
                joints,
                inverse_binds,
                skeleton: skin.skeleton().map(|node| node.index()),
            })
        })
        .collect()
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
        let world = parent * local_transform(&node);
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
pub(crate) mod tests;
