//! What the importer refuses, and why it has to be this crate that refuses it.
//!
//! # `gltf`'s own validation cannot be used here
//!
//! [`gltf::Gltf::from_slice`] validates the document and returns
//! [`gltf::Error::Validation`] for a bad one, which sounds like exactly the
//! gate this module duplicates. It is not usable, because **it panics on some
//! of the very inputs it exists to reject**. In `gltf` 1.4.1:
//!
//! - `gltf_json::mesh::primitive_validate_hook` indexes `root.accessors` with
//!   the primitive's `POSITION` index *directly*. The derive runs the field
//!   validations first, so an out-of-range index is duly reported — and then
//!   the hook indexes with it anyway. Handing the fixture from
//!   `an_out_of_range_position_accessor_is_refused` to `Gltf::from_slice`
//!   aborts in `gltf-json-1.4.1/src/mesh.rs:151` with
//!   `index out of bounds: the len is 4 but the index is 9`.
//! - `gltf::binary::Glb::from_slice` computes `header.length as usize - 12`
//!   before checking anything, so a `.glb` whose header declares a length below
//!   its own header size subtracts with overflow —
//!   `gltf-1.4.1/src/binary.rs:252`, `attempt to subtract with overflow`, from
//!   the sixteen bytes
//!   `a_glb_that_declares_a_length_below_its_own_header_is_refused` uses.
//!
//! Both were reproduced against the resolved version rather than reasoned
//! about; both are debug-build panics and release-build silent wrongness.
//!
//! So the importer parses with [`gltf::Gltf::from_slice_without_validation`]
//! and does its own checking, here, over the JSON — and the checks are written
//! against *what the importer reads*, which is the only defensible boundary:
//! every accessor, view and index [`crate::gltf_import`] touches is checked
//! before the typed API is allowed near it, because that API is a field of
//! `.unwrap()`, `unreachable!()` and `debug_assert!` reachable from file
//! contents.
//!
//! # What is checked, and what is deliberately not
//!
//! Checked: everything whose absence or wrongness would make `gltf` panic,
//! read out of bounds, or hand back geometry that indexes past its own vertex
//! array. `check_document` runs them in the order its own docs list.
//!
//! Not checked: glTF rules the importer never depends on — `POSITION`'s
//! required `min`/`max` (nothing here reads the bounding box), accessor
//! `byteOffset` alignment, `normalized` on a float accessor. Refusing a file
//! over a rule this code does not rely on would reject working assets for a
//! purity nobody benefits from; the risk section of
//! `docs/plan/06-assets-scenes.md` asks for the opposite bias.

use std::fmt;
use std::path::Path;

use crcbl_assets::StorageError;
use gltf::json::accessor::{ComponentType, Type};
use gltf::json::animation::Property;
use gltf::json::buffer::{MAX_BYTE_STRIDE, MIN_BYTE_STRIDE};
use gltf::json::validation::Checked;
use gltf::json::{Root, mesh::Semantic};

/// The fixed part of a `.glb` file: magic, version, total length.
///
/// `gltf::binary::Header::size_of` is the same number and is private, which is
/// why it is spelled again here rather than reused.
pub(crate) const GLB_HEADER_LEN: usize = 12;

/// A malformed-input failure, named by the asset it came from.
///
/// [`StorageError::Other`] rather than a variant of its own: nothing in the
/// engine acts differently on "this file is corrupt" than on any other
/// unrecoverable read, and a second error enum beside `StorageError` would make
/// every caller of [`crate::import_gltf`] match twice. See
/// `docs/backlog.md` for the smallest addition that would change that, and for
/// why nothing needs it yet.
pub(crate) fn malformed(key: &Path, reason: impl fmt::Display) -> StorageError {
    StorageError::Other(format!("{}: {reason}", key.display()))
}

/// Refuse a `.glb` whose 12-byte header cannot be trusted to the `gltf` crate.
///
/// Only the one field that crate reads without checking: a declared total
/// length below the header's own size makes `Glb::from_slice` subtract with
/// overflow. Everything else about the container — a chunk length that overruns
/// the file, a missing `JSON` chunk, a version other than 2 — `gltf` reports as
/// an error, and those arrive here as [`malformed`] through the parse.
pub(crate) fn check_glb_header(bytes: &[u8], key: &Path) -> Result<(), StorageError> {
    if bytes.len() < GLB_HEADER_LEN {
        return Err(malformed(
            key,
            format!(
                "a .glb header is {GLB_HEADER_LEN} bytes and this file is {}",
                bytes.len()
            ),
        ));
    }
    let declared = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    if (declared as usize) < GLB_HEADER_LEN {
        return Err(malformed(
            key,
            format!(
                "the .glb header declares a total length of {declared}, \
                 which is less than the {GLB_HEADER_LEN}-byte header itself"
            ),
        ));
    }
    Ok(())
}

/// Refuse a document the importer cannot read without trusting it.
///
/// `buffers` is the bytes actually resolved for each of `root.buffers`, in
/// order — not the lengths the file declares. Buffer views are checked against
/// what is really there, so a manifest that lies about a buffer's size cannot
/// widen a view past the end of it.
///
/// The order below is the order the reads happen in, so a failure names the
/// first thing that was wrong rather than the last:
///
/// 1. every buffer view lies inside its buffer;
/// 2. every accessor has a usable type, a non-zero count, and a span that fits
///    in its view;
/// 3. every mesh primitive names accessors that exist and carry the types the
///    reader will cast them to;
/// 4. every node, scene and default-scene index exists;
/// 5. every skin names joints, a skeleton and bind matrices that exist;
/// 6. every animation channel names a sampler, a node and accessors that exist,
///    and a `path` and `interpolation` glTF defines;
/// 7. every image says where its bytes are;
/// 8. every texture names an image and a sampler that exist;
/// 9. every material's texture references name a texture that exists.
pub(crate) fn check_document(
    root: &Root,
    buffers: &[Vec<u8>],
    key: &Path,
) -> Result<(), StorageError> {
    check_views(root, buffers, key)?;
    check_accessors(root, key)?;
    check_meshes(root, key)?;
    check_nodes(root, key)?;
    check_skins(root, key)?;
    check_animations(root, key)?;
    check_images(root, key)?;
    check_textures(root, key)?;
    check_materials(root, key)
}

/// A `u64` from the file as a `usize`, or a refusal.
///
/// glTF sizes are 64-bit in the JSON and `usize` everywhere they are used. On a
/// 64-bit host the conversion cannot fail; on wasm32 it can, and a length that
/// does not fit the address space is a file this machine cannot load however it
/// is spelled.
fn fits(value: u64, what: &str, key: &Path) -> Result<usize, StorageError> {
    usize::try_from(value).map_err(|_| {
        malformed(
            key,
            format!("{what} is {value}, which does not fit in memory"),
        )
    })
}

fn check_views(root: &Root, buffers: &[Vec<u8>], key: &Path) -> Result<(), StorageError> {
    for (index, view) in root.buffer_views.iter().enumerate() {
        let buffer = buffers.get(view.buffer.value()).ok_or_else(|| {
            malformed(
                key,
                format!(
                    "bufferView {index} names buffer {}, and there are {}",
                    view.buffer.value(),
                    buffers.len()
                ),
            )
        })?;
        let offset = fits(
            view.byte_offset.unwrap_or_default().0,
            &format!("bufferView {index}'s byteOffset"),
            key,
        )?;
        let length = fits(
            view.byte_length.0,
            &format!("bufferView {index}'s byteLength"),
            key,
        )?;
        let end = offset.checked_add(length).ok_or_else(|| {
            malformed(
                key,
                format!("bufferView {index}'s byteOffset + byteLength overflows"),
            )
        })?;
        if end > buffer.len() {
            return Err(malformed(
                key,
                format!(
                    "bufferView {index} covers bytes {offset}..{end} of a buffer that is {} long",
                    buffer.len()
                ),
            ));
        }
        if let Some(stride) = view.byte_stride {
            // Zero means "tightly packed" to `gltf::buffer::View::stride`, so it
            // is not a stride at all and is not checked as one.
            if stride.0 != 0 && !(MIN_BYTE_STRIDE..=MAX_BYTE_STRIDE).contains(&stride.0) {
                return Err(malformed(
                    key,
                    format!(
                        "bufferView {index}'s byteStride is {}, outside \
                         {MIN_BYTE_STRIDE}..={MAX_BYTE_STRIDE}",
                        stride.0
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn check_accessors(root: &Root, key: &Path) -> Result<(), StorageError> {
    for (index, accessor) in root.accessors.iter().enumerate() {
        if accessor.sparse.is_some() {
            return Err(StorageError::Unsupported(
                "glTF sparse accessors: no importer reads one, and gltf's sparse \
                 iterator underflows on a zero sparse count",
            ));
        }
        let Checked::Valid(component) = accessor.component_type else {
            return Err(malformed(
                key,
                format!("accessor {index} has a componentType glTF does not define"),
            ));
        };
        let Checked::Valid(dimensions) = accessor.type_ else {
            return Err(malformed(
                key,
                format!("accessor {index} has a type glTF does not define"),
            ));
        };
        let count = fits(accessor.count.0, &format!("accessor {index}'s count"), key)?;
        if count == 0 {
            // `gltf::accessor::Iter::new` computes `stride * (count - 1)`.
            return Err(malformed(
                key,
                format!("accessor {index} has a count of zero"),
            ));
        }
        let Some(view_index) = accessor.buffer_view else {
            return Err(malformed(
                key,
                format!("accessor {index} has no bufferView and is not sparse"),
            ));
        };
        let view = root.buffer_views.get(view_index.value()).ok_or_else(|| {
            malformed(
                key,
                format!(
                    "accessor {index} names bufferView {}, and there are {}",
                    view_index.value(),
                    root.buffer_views.len()
                ),
            )
        })?;

        let element = component.0.size() * dimensions.multiplicity();
        let stride = match view.byte_stride {
            Some(stride) if stride.0 != 0 => stride.0,
            _ => element,
        };
        if stride < element {
            // `Iter::new` asserts this in debug builds and reads overlapping
            // elements in release ones.
            return Err(malformed(
                key,
                format!(
                    "accessor {index} reads {element}-byte elements through a \
                     bufferView with a byteStride of {stride}"
                ),
            ));
        }
        let offset = fits(
            accessor.byte_offset.unwrap_or_default().0,
            &format!("accessor {index}'s byteOffset"),
            key,
        )?;
        let span = stride
            .checked_mul(count - 1)
            .and_then(|span| span.checked_add(element))
            .and_then(|span| span.checked_add(offset))
            .ok_or_else(|| {
                malformed(
                    key,
                    format!("accessor {index}'s count of {count} overflows its own byte span"),
                )
            })?;
        let view_length = fits(
            view.byte_length.0,
            &format!("bufferView {}'s byteLength", view_index.value()),
            key,
        )?;
        if span > view_length {
            return Err(malformed(
                key,
                format!(
                    "accessor {index} reads {span} bytes from a bufferView that is {view_length} long"
                ),
            ));
        }
    }
    Ok(())
}

/// The types [`crate::gltf_import`] casts each attribute to.
///
/// A mismatch is a `debug_assert` or an `unreachable!()` inside `gltf`'s
/// reader, so it is checked here instead. `TEXCOORD_0` has three legal spellings
/// because `ReadTexCoords::into_f32` un-normalises the two integer ones.
fn check_attribute(
    root: &Root,
    key: &Path,
    what: &str,
    accessor: usize,
    dimensions: Type,
    components: &[ComponentType],
) -> Result<(), StorageError> {
    let accessor = root.accessors.get(accessor).ok_or_else(|| {
        malformed(
            key,
            format!(
                "{what} names accessor {accessor}, and there are {}",
                root.accessors.len()
            ),
        )
    })?;
    // Both are `Checked::Valid` here: `check_accessors` refuses the document
    // otherwise, and it runs first.
    let ok = matches!(accessor.type_, Checked::Valid(found) if found == dimensions)
        && matches!(accessor.component_type, Checked::Valid(found)
            if components.contains(&found.0));
    if ok {
        return Ok(());
    }
    Err(malformed(
        key,
        format!(
            "{what} is {:?}/{:?}, and the importer reads {dimensions:?} of one of {components:?}",
            accessor.type_, accessor.component_type
        ),
    ))
}

fn check_meshes(root: &Root, key: &Path) -> Result<(), StorageError> {
    for (mesh_index, mesh) in root.meshes.iter().enumerate() {
        for (index, primitive) in mesh.primitives.iter().enumerate() {
            let at = format!("mesh {mesh_index} primitive {index}");
            if matches!(primitive.mode, Checked::Invalid) {
                // `gltf::Primitive::mode` unwraps this.
                return Err(malformed(
                    key,
                    format!("{at} has a mode glTF does not define"),
                ));
            }
            for (semantic, accessor) in &primitive.attributes {
                let named = |what: &str| format!("{at}'s {what}");
                match semantic {
                    Checked::Valid(Semantic::Positions) => check_attribute(
                        root,
                        key,
                        &named("POSITION"),
                        accessor.value(),
                        Type::Vec3,
                        &[ComponentType::F32],
                    )?,
                    Checked::Valid(Semantic::Normals) => check_attribute(
                        root,
                        key,
                        &named("NORMAL"),
                        accessor.value(),
                        Type::Vec3,
                        &[ComponentType::F32],
                    )?,
                    Checked::Valid(Semantic::TexCoords(0)) => check_attribute(
                        root,
                        key,
                        &named("TEXCOORD_0"),
                        accessor.value(),
                        Type::Vec2,
                        &[ComponentType::F32, ComponentType::U8, ComponentType::U16],
                    )?,
                    // The two component types glTF allows a joint index, and
                    // the only two `ReadJoints` has variants for — anything
                    // else is `unreachable!()` inside `Reader::read_joints`.
                    Checked::Valid(Semantic::Joints(0)) => check_attribute(
                        root,
                        key,
                        &named("JOINTS_0"),
                        accessor.value(),
                        Type::Vec4,
                        &[ComponentType::U8, ComponentType::U16],
                    )?,
                    // `ReadWeights::into_f32` un-normalises the two integer
                    // spellings, the same way `TEXCOORD_0`'s does.
                    Checked::Valid(Semantic::Weights(0)) => check_attribute(
                        root,
                        key,
                        &named("WEIGHTS_0"),
                        accessor.value(),
                        Type::Vec4,
                        &[ComponentType::F32, ComponentType::U8, ComponentType::U16],
                    )?,
                    // An attribute this importer does not read. Its accessor is
                    // still required to exist, because nothing stops a later
                    // slice reading it and an index that names nothing is a
                    // defect in the file either way.
                    _ if accessor.value() >= root.accessors.len() => {
                        return Err(malformed(
                            key,
                            format!(
                                "{at} has an attribute naming accessor {}, and there are {}",
                                accessor.value(),
                                root.accessors.len()
                            ),
                        ));
                    }
                    _ => {}
                }
            }
            if !primitive
                .attributes
                .contains_key(&Checked::Valid(Semantic::Positions))
            {
                return Err(malformed(key, format!("{at} has no POSITION attribute")));
            }
            if let Some(indices) = primitive.indices {
                check_attribute(
                    root,
                    key,
                    &format!("{at}'s indices"),
                    indices.value(),
                    Type::Scalar,
                    &[ComponentType::U8, ComponentType::U16, ComponentType::U32],
                )?;
            }
            if let Some(material) = primitive.material
                && material.value() >= root.materials.len()
            {
                return Err(malformed(
                    key,
                    format!(
                        "{at} names material {}, and there are {}",
                        material.value(),
                        root.materials.len()
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn check_nodes(root: &Root, key: &Path) -> Result<(), StorageError> {
    for (index, node) in root.nodes.iter().enumerate() {
        if let Some(mesh) = node.mesh
            && mesh.value() >= root.meshes.len()
        {
            return Err(malformed(
                key,
                format!(
                    "node {index} names mesh {}, and there are {}",
                    mesh.value(),
                    root.meshes.len()
                ),
            ));
        }
        if let Some(skin) = node.skin
            && skin.value() >= root.skins.len()
        {
            return Err(malformed(
                key,
                format!(
                    "node {index} wears skin {}, and there are {}",
                    skin.value(),
                    root.skins.len()
                ),
            ));
        }
        for child in node.children.iter().flatten() {
            if child.value() >= root.nodes.len() {
                return Err(malformed(
                    key,
                    format!(
                        "node {index} has child {}, and there are {} nodes",
                        child.value(),
                        root.nodes.len()
                    ),
                ));
            }
        }
    }
    for (index, scene) in root.scenes.iter().enumerate() {
        for node in &scene.nodes {
            if node.value() >= root.nodes.len() {
                return Err(malformed(
                    key,
                    format!(
                        "scene {index} names node {}, and there are {}",
                        node.value(),
                        root.nodes.len()
                    ),
                ));
            }
        }
    }
    if let Some(scene) = root.scene
        && scene.value() >= root.scenes.len()
    {
        return Err(malformed(
            key,
            format!(
                "the default scene is {}, and there are {}",
                scene.value(),
                root.scenes.len()
            ),
        ));
    }
    Ok(())
}

/// Every skin's joints, skeleton and `inverseBindMatrices` exist.
///
/// `gltf::Skin` resolves all three by `nth(index).unwrap()` — `joints()`,
/// `skeleton()` and `inverse_bind_matrices()` each — so a skin naming a node or
/// an accessor that is not there aborts the process. The matrix accessor is
/// checked for its type as well as its existence, because
/// `accessor::Iter::<[[f32; 4]; 4]>::new` asserts the element size it was
/// handed.
fn check_skins(root: &Root, key: &Path) -> Result<(), StorageError> {
    for (index, skin) in root.skins.iter().enumerate() {
        for joint in &skin.joints {
            if joint.value() >= root.nodes.len() {
                return Err(malformed(
                    key,
                    format!(
                        "skin {index} names joint node {}, and there are {} nodes",
                        joint.value(),
                        root.nodes.len()
                    ),
                ));
            }
        }
        if let Some(skeleton) = skin.skeleton
            && skeleton.value() >= root.nodes.len()
        {
            return Err(malformed(
                key,
                format!(
                    "skin {index} names skeleton node {}, and there are {} nodes",
                    skeleton.value(),
                    root.nodes.len()
                ),
            ));
        }
        if let Some(matrices) = skin.inverse_bind_matrices {
            check_attribute(
                root,
                key,
                &format!("skin {index}'s inverseBindMatrices"),
                matrices.value(),
                Type::Mat4,
                &[ComponentType::F32],
            )?;
        }
    }
    Ok(())
}

/// Every animation channel can be read without `gltf` unwrapping something the
/// file did not supply.
///
/// Five `unwrap`s over file contents live behind one channel:
/// `Channel::sampler` is `samplers().nth(index).unwrap()`, `Target::node` is
/// the same over the node array, `Target::property` and
/// `Sampler::interpolation` unwrap a `Checked`, and `Sampler::input`/`output`
/// index the accessor array. The output accessor's type depends on what the
/// channel drives, so it is checked against the target's own path — a rotation
/// channel whose output is not `VEC4` reaches an `unreachable!()` in
/// `Reader::read_outputs`.
fn check_animations(root: &Root, key: &Path) -> Result<(), StorageError> {
    for (index, animation) in root.animations.iter().enumerate() {
        for (channel_index, channel) in animation.channels.iter().enumerate() {
            let at = format!("animation {index} channel {channel_index}");
            if channel.target.node.value() >= root.nodes.len() {
                return Err(malformed(
                    key,
                    format!(
                        "{at} targets node {}, and there are {} nodes",
                        channel.target.node.value(),
                        root.nodes.len()
                    ),
                ));
            }
            let Checked::Valid(property) = channel.target.path else {
                return Err(malformed(
                    key,
                    format!("{at} targets a path glTF does not define"),
                ));
            };
            let sampler = animation
                .samplers
                .get(channel.sampler.value())
                .ok_or_else(|| {
                    malformed(
                        key,
                        format!(
                            "{at} names sampler {}, and animation {index} has {}",
                            channel.sampler.value(),
                            animation.samplers.len()
                        ),
                    )
                })?;
            if matches!(sampler.interpolation, Checked::Invalid) {
                return Err(malformed(
                    key,
                    format!("{at}'s sampler has an interpolation glTF does not define"),
                ));
            }
            check_attribute(
                root,
                key,
                &format!("{at}'s input"),
                sampler.input.value(),
                Type::Scalar,
                &[ComponentType::F32],
            )?;
            // The variants `ReadOutputs` has for each path: a TRS curve is
            // float triples, and a rotation or a morph weight may also arrive
            // as a normalized integer, which `into_f32` un-normalises.
            let (dimensions, components) = match property {
                Property::Translation | Property::Scale => (Type::Vec3, &[ComponentType::F32][..]),
                Property::Rotation => (Type::Vec4, NORMALIZABLE),
                Property::MorphTargetWeights => (Type::Scalar, NORMALIZABLE),
            };
            check_attribute(
                root,
                key,
                &format!("{at}'s output"),
                sampler.output.value(),
                dimensions,
                components,
            )?;
        }
    }
    Ok(())
}

/// The component types a rotation or a morph weight may be stored as.
///
/// The float spelling plus the four normalized-integer ones the specification
/// allows, which are exactly the variants `gltf`'s `Rotations` and
/// `MorphTargetWeights` enums have — anything else is an `unreachable!()` in
/// `Reader::read_outputs`.
const NORMALIZABLE: &[ComponentType] = &[
    ComponentType::F32,
    ComponentType::I8,
    ComponentType::U8,
    ComponentType::I16,
    ComponentType::U16,
];

/// Every image says where its bytes are, in a form
/// [`gltf::image::Image::source`] can answer without panicking.
///
/// That method is three `unwrap`s over file contents: a `bufferView` is
/// resolved with `views().nth(index).unwrap()` and its `mimeType` is
/// `unwrap()`ed as *required*, and an image with no `bufferView` at all
/// `unwrap()`s `uri`. So an image with neither, one whose view does not exist,
/// or one that puts its bytes in a view without saying what they are, aborts
/// the process rather than returning — which is this module's whole reason to
/// exist.
fn check_images(root: &Root, key: &Path) -> Result<(), StorageError> {
    for (index, image) in root.images.iter().enumerate() {
        match (&image.buffer_view, &image.uri) {
            (Some(view), _) => {
                if view.value() >= root.buffer_views.len() {
                    return Err(malformed(
                        key,
                        format!(
                            "image {index} names bufferView {}, and there are {}",
                            view.value(),
                            root.buffer_views.len()
                        ),
                    ));
                }
                if image.mime_type.is_none() {
                    return Err(malformed(
                        key,
                        format!(
                            "image {index} puts its bytes in a bufferView and declares no \
                             mimeType, which the specification requires there"
                        ),
                    ));
                }
            }
            (None, Some(_)) => {}
            (None, None) => {
                return Err(malformed(
                    key,
                    format!("image {index} has neither a uri nor a bufferView"),
                ));
            }
        }
    }
    Ok(())
}

/// The `source` a texture that omits one arrives with.
///
/// **`source` is not an `Option` in `gltf-json`'s model**: it carries a `serde`
/// default of `u32::MAX`, so a texture supplying its image through an extension
/// — `KHR_texture_basisu`, `EXT_texture_webp` — is indistinguishable from one
/// naming image four billion. Named here because two modules have to agree on
/// it: this one lets it through, and `gltf_import::build` must not call
/// [`gltf::Texture::source`] on it.
pub(crate) const TEXTURE_SOURCE_ABSENT: usize = u32::MAX as usize;

/// Every texture names an image and a sampler that exist, or names no image at
/// all.
///
/// [`gltf::Texture::source`] is `images().nth(json.source.value()).unwrap()` and
/// [`gltf::Texture::sampler`] is the same shape over `samplers`, so an index
/// past the end aborts the process rather than being refused. That is what this
/// checks.
///
/// # A texture with no image is skipped, not refused
///
/// It was refused until 2026-08-19, on the argument that a texture with no
/// readable image is a material pointing at nothing. Two things made that
/// wrong. The message said `texture 0 names image 4294967295`, reporting a
/// sentinel this crate invented as though the document had written it. And it
/// is inconsistent with everything around it: a document requiring an extension
/// this importer lacks now loads and says so — see
/// `gltf_import::warn_unsupported_extensions` — and an image whose bytes cannot
/// be decoded is already skipped with the material falling back to its base
/// colour. `SheenWoodLeatherSofa` from the Khronos suite is the case: it
/// requires `EXT_texture_webp`, which supplies the image, and it was the one
/// document in that suite refused for this.
///
/// So an **absent** source passes here and `gltf_import::build` drops the
/// texture; an **out-of-range** source is still refused, because that is a
/// document naming an image it does not have.
fn check_textures(root: &Root, key: &Path) -> Result<(), StorageError> {
    for (index, texture) in root.textures.iter().enumerate() {
        if texture.source.value() != TEXTURE_SOURCE_ABSENT
            && texture.source.value() >= root.images.len()
        {
            return Err(malformed(
                key,
                format!(
                    "texture {index} names image {}, and there are {}",
                    texture.source.value(),
                    root.images.len()
                ),
            ));
        }
        if let Some(sampler) = &texture.sampler
            && sampler.value() >= root.samplers.len()
        {
            return Err(malformed(
                key,
                format!(
                    "texture {index} names sampler {}, and there are {}",
                    sampler.value(),
                    root.samplers.len()
                ),
            ));
        }
    }
    Ok(())
}

/// Every texture reference a material carries names a texture that exists.
///
/// [`gltf::material::PbrMetallicRoughness::base_color_texture`] resolves the
/// index with `textures().nth(index).unwrap()`, and each of the other four
/// slots is the same call in a different accessor. All five are checked rather
/// than only the one [`crate::gltf_import`] reads today: they are one loop, and
/// the alternative is a panic waiting for whichever slot is read next.
fn check_materials(root: &Root, key: &Path) -> Result<(), StorageError> {
    for (index, material) in root.materials.iter().enumerate() {
        let pbr = &material.pbr_metallic_roughness;
        let slots = [
            (
                "baseColorTexture",
                pbr.base_color_texture.as_ref().map(|info| info.index),
            ),
            (
                "metallicRoughnessTexture",
                pbr.metallic_roughness_texture
                    .as_ref()
                    .map(|info| info.index),
            ),
            (
                "normalTexture",
                material
                    .normal_texture
                    .as_ref()
                    .map(|texture| texture.index),
            ),
            (
                "occlusionTexture",
                material
                    .occlusion_texture
                    .as_ref()
                    .map(|texture| texture.index),
            ),
            (
                "emissiveTexture",
                material.emissive_texture.as_ref().map(|info| info.index),
            ),
        ];
        for (slot, texture) in slots {
            if let Some(texture) = texture
                && texture.value() >= root.textures.len()
            {
                return Err(malformed(
                    key,
                    format!(
                        "material {index}'s {slot} names texture {}, and there are {}",
                        texture.value(),
                        root.textures.len()
                    ),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::GLB_HEADER_LEN;
    use crate::gltf_fixture::{
        Assets, BIN_CHUNK_BUFFER, EXTERNAL_BUFFER, glb, import_glb, import_glb_bytes,
        import_gltf_text, import_rigged_glb, replacing, rigged_json, textured_parts, triangle_bin,
        triangle_json,
    };
    use crcbl_assets::StorageError;

    /// The rigged fixture is real glTF, and its geometry survives the rig.
    ///
    /// This asserts nothing about what the importer makes of the rig —
    /// `gltf_import`'s own tests do that. What it pins is that the document
    /// parses, which is the part a hand-written buffer layout gets wrong: an accessor whose offset is not a
    /// multiple of its component size, a `bufferView` that runs past the
    /// buffer, or a `JOINTS_0` declared as float would all fail here rather
    /// than much later, in whatever first tries to read them.
    #[test]
    fn the_rigged_fixture_is_a_document_this_importer_accepts() {
        let scene = import_rigged_glb(&rigged_json(BIN_CHUNK_BUFFER))
            .expect("the rigged fixture should be valid glTF");
        assert_eq!(scene.meshes().len(), 1, "{:?}", scene.meshes().len());
        assert_eq!(scene.materials().len(), 1);
    }

    /// The rig in the fixture says what this file thinks it says.
    ///
    /// The test above only asks that the document parses, and a hand-written
    /// buffer layout can be well-formed and still wrong: shifting the payload
    /// while leaving every `byteOffset` alone leaves each accessor in range and
    /// reading the wrong bytes. The values are read straight out of `gltf` here
    /// and compared against the constants the fixture was built from, so the
    /// rig the importer's own tests assert on is one that has been confirmed
    /// independently of the importer.
    #[test]
    fn the_rigged_fixture_holds_the_rig_it_declares() {
        use crate::gltf_fixture::{CLIP_ROTATIONS, CLIP_TIMES, INVERSE_BIND, rigged_bin};

        let bytes = glb(&rigged_json(BIN_CHUNK_BUFFER), Some(&rigged_bin()));
        let gltf = gltf::Gltf::from_slice(&bytes).expect("the fixture should parse");
        let blob = gltf.blob.as_deref().expect("a GLB carries its buffer");
        let buffer = |_: gltf::Buffer<'_>| Some(blob);

        let skin = gltf.skins().next().expect("one skin");
        assert_eq!(skin.name(), Some("rig"));
        let joints: Vec<usize> = skin.joints().map(|node| node.index()).collect();
        assert_eq!(joints, vec![1, 2], "the joints are the two joint nodes");
        assert_eq!(skin.skeleton().map(|node| node.index()), Some(1));

        let matrices: Vec<[[f32; 4]; 4]> = skin
            .reader(buffer)
            .read_inverse_bind_matrices()
            .expect("the skin declares inverse bind matrices")
            .collect();
        assert_eq!(matrices.len(), INVERSE_BIND.len());
        for (read, declared) in matrices.iter().zip(INVERSE_BIND) {
            // `read` arrives as four columns; the constant is the same numbers
            // flat, in the column-major order the format stores them.
            let flat: Vec<f32> = read.iter().flatten().copied().collect();
            assert_eq!(flat, declared.to_vec(), "an inverse bind matrix differs");
        }

        let animation = gltf.animations().next().expect("one animation");
        assert_eq!(animation.name(), Some("wave"));
        let channel = animation.channels().next().expect("one channel");
        assert_eq!(channel.target().node().index(), 2);
        assert!(
            matches!(
                channel.target().property(),
                gltf::animation::Property::Rotation
            ),
            "the channel should drive rotation"
        );

        let reader = channel.reader(buffer);
        let times: Vec<f32> = reader.read_inputs().expect("keyframe times").collect();
        assert_eq!(times, CLIP_TIMES.to_vec());
        let gltf::animation::util::ReadOutputs::Rotations(rotations) =
            reader.read_outputs().expect("keyframe outputs")
        else {
            panic!("the channel's outputs should be rotations");
        };
        let read: Vec<[f32; 4]> = rotations.into_f32().collect();
        assert_eq!(read, CLIP_ROTATIONS.to_vec());
    }

    /// The rig's constants mean what their names say.
    ///
    /// The test above compares the buffer against the constants it was written
    /// from, which catches a layout that shifted and cannot catch a constant
    /// that is simply wrong — change one and both sides move together. These
    /// two facts are derivable instead, so they are derived: an inverse bind
    /// matrix composed with the bind transform it inverts is the identity, and
    /// a quaternion for a quarter turn about Z carries *half* that angle and so
    /// takes x to y.
    #[test]
    fn the_rigs_constants_are_the_transforms_they_claim_to_be() {
        use crate::gltf_fixture::{CLIP_ROTATIONS, INVERSE_BIND};
        use glam::{Mat4, Quat, Vec3};

        // The tip joint binds at `"translation": [0, 1, 0]` in the document, and
        // the root at the origin.
        for (bind, inverse) in [
            (Vec3::ZERO, INVERSE_BIND[0]),
            (Vec3::new(0.0, 1.0, 0.0), INVERSE_BIND[1]),
        ] {
            let composed = Mat4::from_cols_array(&inverse) * Mat4::from_translation(bind);
            let drift = (composed - Mat4::IDENTITY)
                .to_cols_array()
                .iter()
                .fold(0.0f32, |worst, cell| worst.max(cell.abs()));
            assert!(
                drift < 1.0e-6,
                "the inverse bind matrix for a joint bound at {bind} is not its inverse: {composed}"
            );
        }

        let identity = Quat::from_array(CLIP_ROTATIONS[0]);
        assert!(
            (identity * Vec3::X).distance(Vec3::X) < 1.0e-6,
            "the first keyframe should be the identity rotation"
        );
        let quarter = Quat::from_array(CLIP_ROTATIONS[1]);
        assert!(
            quarter.is_normalized(),
            "a rotation quaternion has to be a unit one"
        );
        assert!(
            (quarter * Vec3::X).distance(Vec3::Y) < 1.0e-6,
            "a quarter turn about Z should take x to y, and takes it to {}",
            quarter * Vec3::X
        );
    }

    /// The fixture with one thing wrong, imported. Panics rather than returns
    /// if the import *succeeded*, so a mutation that stopped being malformed
    /// cannot pass as a refusal.
    #[track_caller]
    fn refused(json: &str) -> String {
        match import_glb(json) {
            Err(error) => error.to_string(),
            Ok(scene) => {
                panic!("this document should have been refused, and it imported: {scene:?}")
            }
        }
    }

    /// Every mutation that must be refused, with the substring of the reason
    /// that says the importer refused it for the reason meant rather than by
    /// tripping over something else on the way.
    #[test]
    fn every_malformed_document_is_refused_with_the_reason_it_was_refused_for() {
        let base = triangle_json(BIN_CHUNK_BUFFER);
        for (from, to, because) in [
            // Indices naming things that are not there. The POSITION one is the
            // case `gltf`'s own validation panics on; see the module docs.
            (
                "\"POSITION\": 0",
                "\"POSITION\": 9",
                "names accessor 9, and there are 4",
            ),
            (
                "\"NORMAL\": 1",
                "\"NORMAL\": 9",
                "names accessor 9, and there are 4",
            ),
            (
                "\"TEXCOORD_0\": 2",
                "\"TEXCOORD_0\": 9",
                "names accessor 9, and there are 4",
            ),
            (
                "\"indices\": 3",
                "\"indices\": 9",
                "names accessor 9, and there are 4",
            ),
            (
                "\"material\": 0",
                "\"material\": 7",
                "names material 7, and there are 1",
            ),
            (
                "\"mesh\": 0",
                "\"mesh\": 7",
                "names mesh 7, and there are 1",
            ),
            (
                "\"children\": [1]",
                "\"children\": [7]",
                "has child 7, and there are 2 nodes",
            ),
            (
                "\"scenes\": [{ \"nodes\": [0] }]",
                "\"scenes\": [{ \"nodes\": [7] }]",
                "names node 7, and there are 2",
            ),
            (
                "\"scene\": 0",
                "\"scene\": 7",
                "the default scene is 7, and there are 1",
            ),
            (
                "{ \"bufferView\": 0,",
                "{ \"bufferView\": 9,",
                "names bufferView 9, and there are 4",
            ),
            (
                "{ \"buffer\": 0, \"byteOffset\": 0",
                "{ \"buffer\": 9, \"byteOffset\": 0",
                "names buffer 9, and there are 1",
            ),
            // Buffer views and accessors reaching past what is really there.
            (
                "\"byteOffset\": 0, \"byteLength\": 36",
                "\"byteOffset\": 0, \"byteLength\": 999",
                "covers bytes 0..999 of a buffer that is 104 long",
            ),
            (
                "\"byteOffset\": 96, \"byteLength\": 6",
                "\"byteOffset\": 100, \"byteLength\": 6",
                "covers bytes 100..106 of a buffer that is 104 long",
            ),
            (
                "{ \"bufferView\": 0, \"componentType\": 5126, \"count\": 3",
                "{ \"bufferView\": 0, \"componentType\": 5126, \"count\": 300",
                "reads 3600 bytes from a bufferView that is 36 long",
            ),
            // The count arithmetic `gltf` does unchecked.
            (
                "{ \"bufferView\": 0, \"componentType\": 5126, \"count\": 3",
                "{ \"bufferView\": 0, \"componentType\": 5126, \"count\": 18446744073709551615",
                "accessor 0",
            ),
            (
                "{ \"bufferView\": 0, \"componentType\": 5126, \"count\": 3",
                "{ \"bufferView\": 0, \"componentType\": 5126, \"count\": 0",
                "accessor 0 has a count of zero",
            ),
            // Types the reader would miscast, each an `unreachable!()` or a
            // `debug_assert` inside `gltf`.
            (
                "{ \"bufferView\": 0, \"componentType\": 5126, \"count\": 3, \"type\": \"VEC3\" }",
                "{ \"bufferView\": 0, \"componentType\": 5126, \"count\": 3, \"type\": \"VEC2\" }",
                "POSITION is Valid(Vec2)",
            ),
            (
                "{ \"bufferView\": 0, \"componentType\": 5126, \"count\": 3, \"type\": \"VEC3\" }",
                "{ \"bufferView\": 0, \"componentType\": 5123, \"count\": 3, \"type\": \"VEC3\" }",
                "POSITION is Valid(Vec3)/Valid(GenericComponentType(U16))",
            ),
            (
                "{ \"bufferView\": 3, \"componentType\": 5123, \"count\": 3, \"type\": \"SCALAR\" }",
                "{ \"bufferView\": 3, \"componentType\": 5126, \"count\": 1, \"type\": \"SCALAR\" }",
                "indices is Valid(Scalar)/Valid(GenericComponentType(F32))",
            ),
            (
                "{ \"bufferView\": 2, \"componentType\": 5126, \"count\": 3, \"type\": \"VEC2\" }",
                "{ \"bufferView\": 2, \"componentType\": 5122, \"count\": 3, \"type\": \"VEC2\" }",
                "TEXCOORD_0 is Valid(Vec2)/Valid(GenericComponentType(I16))",
            ),
            (
                "\"componentType\": 5126, \"count\": 3, \"type\": \"VEC3\" },\n    { \"bufferView\": 2",
                "\"componentType\": 9999, \"count\": 3, \"type\": \"VEC3\" },\n    { \"bufferView\": 2",
                "accessor 1 has a componentType glTF does not define",
            ),
            (
                "\"componentType\": 5126, \"count\": 3, \"type\": \"VEC3\" },\n    { \"bufferView\": 2",
                "\"componentType\": 5126, \"count\": 3, \"type\": \"VEC9\" },\n    { \"bufferView\": 2",
                "accessor 1 has a type glTF does not define",
            ),
            // A stride smaller than the element it strides.
            (
                "{ \"buffer\": 0, \"byteOffset\": 0, \"byteLength\": 36 }",
                "{ \"buffer\": 0, \"byteOffset\": 0, \"byteLength\": 36, \"byteStride\": 4 }",
                "reads 12-byte elements through a bufferView with a byteStride of 4",
            ),
            (
                "{ \"buffer\": 0, \"byteOffset\": 0, \"byteLength\": 36 }",
                "{ \"buffer\": 0, \"byteOffset\": 0, \"byteLength\": 36, \"byteStride\": 1 }",
                "byteStride is 1, outside 4..=252",
            ),
            // Things the file says that it must not.
            (
                "\"mode\": 0,",
                "\"mode\": 42,",
                "has a mode glTF does not define",
            ),
            (
                "\"attributes\": { \"POSITION\": 0, ",
                "\"attributes\": { ",
                "has no POSITION attribute",
            ),
            (
                "\"children\": [1]",
                "\"children\": [0]",
                "the node hierarchy must be a forest",
            ),
            (
                "{ \"bufferView\": 1, \"componentType\": 5126, \"count\": 3",
                "{ \"bufferView\": 1, \"componentType\": 5126, \"count\": 2",
                "has 3 positions and 2 NORMAL values",
            ),
        ] {
            // `mode` is not in the base fixture; the two entries that touch it
            // start from a document that has one.
            let start = if from.contains("\"mode\"") || to.contains("\"mode\"") {
                replacing(&base, "\"indices\": 3,", "\"indices\": 3, \"mode\": 0,")
            } else {
                base.clone()
            };
            let reason = refused(&replacing(&start, from, to));
            assert!(
                reason.contains(because),
                "{from:?} -> {to:?} was refused, but for {reason:?} rather than {because:?}"
            );
        }
    }

    /// The texture side, table-driven on its own base document — the triangle
    /// fixture has no images, textures or samplers to put a foot wrong in.
    ///
    /// **Every one of these is a `.unwrap()` in `gltf` 1.4.1**, not a tidiness
    /// rule: `Image::source` resolves its `bufferView` with
    /// `views().nth(index).unwrap()` and `unwrap()`s the `mimeType` beside it,
    /// `Texture::source` and `Texture::sampler` are the same shape over `images`
    /// and `samplers`, and `PbrMetallicRoughness::base_color_texture` is
    /// `textures().nth(index).unwrap()`. A document that reaches one of those
    /// aborts the process instead of being refused.
    ///
    /// A texture that omits `source` entirely is **not** on this list, and has
    /// its own test below: it is skipped rather than refused, because the
    /// image is supplied by an extension — see [`check_textures`].
    #[test]
    fn every_malformed_texture_reference_is_refused_with_the_reason_it_was_refused_for() {
        let (base, bin) = textured_parts(b"not really a png", "image/png", 0);
        for (from, to, because) in [
            (
                "\"textures\": [{ \"source\": 0 }]",
                "\"textures\": [{ \"source\": 9 }]",
                "texture 0 names image 9, and there are 1",
            ),
            (
                "\"textures\": [{ \"source\": 0 }]",
                "\"textures\": [{ \"source\": 0, \"sampler\": 3 }]",
                "texture 0 names sampler 3, and there are 0",
            ),
            (
                "\"baseColorTexture\": { \"index\": 0",
                "\"baseColorTexture\": { \"index\": 9",
                "material 0's baseColorTexture names texture 9, and there are 1",
            ),
            (
                "\"bufferView\": 4, \"mimeType\": \"image/png\"",
                "\"bufferView\": 9, \"mimeType\": \"image/png\"",
                "image 0 names bufferView 9, and there are 5",
            ),
            (
                "\"bufferView\": 4, \"mimeType\": \"image/png\"",
                "\"bufferView\": 4",
                "image 0 puts its bytes in a bufferView and declares no mimeType",
            ),
            (
                "\"name\": \"paint\", \"bufferView\": 4, \"mimeType\": \"image/png\"",
                "\"name\": \"paint\"",
                "image 0 has neither a uri nor a bufferView",
            ),
        ] {
            let json = replacing(&base, from, to);
            let bytes = glb(&json, Some(&bin));
            let reason = match import_glb_bytes(&bytes) {
                Err(error) => error.to_string(),
                Ok(scene) => {
                    panic!(
                        "{from:?} -> {to:?} should have been refused, and it imported: {scene:?}"
                    )
                }
            };
            assert!(
                reason.contains(because),
                "{from:?} -> {to:?} was refused, but for {reason:?} rather than {because:?}"
            );
        }
    }

    /// The other four material texture slots reach the same `unwrap` as
    /// `baseColorTexture` and are checked with it, even though nothing reads
    /// them yet.
    #[test]
    fn a_material_texture_slot_the_importer_does_not_read_is_still_checked() {
        let (base, bin) = textured_parts(b"not really a png", "image/png", 0);
        for slot in [
            "\"normalTexture\": { \"index\": 9 }",
            "\"occlusionTexture\": { \"index\": 9 }",
            "\"emissiveTexture\": { \"index\": 9 }",
            "\"metallicRoughnessTexture\": { \"index\": 9 }",
        ] {
            // The metallic-roughness one lives inside the PBR block and the
            // other three beside it, which is where each is spliced in.
            let json = if slot.starts_with("\"metallicRoughness") {
                replacing(
                    &base,
                    "\"baseColorFactor\"",
                    &format!("{slot}, \"baseColorFactor\""),
                )
            } else {
                replacing(
                    &base,
                    "\"name\": \"painted\"",
                    &format!("\"name\": \"painted\", {slot}"),
                )
            };
            let reason = match import_glb_bytes(&glb(&json, Some(&bin))) {
                Err(error) => error.to_string(),
                Ok(scene) => panic!("{slot} should have been refused, and it imported: {scene:?}"),
            };
            assert!(
                reason.contains("names texture 9, and there are 1"),
                "{slot} was refused for {reason:?}"
            );
        }
    }

    /// [`refused`] for the rigged fixture, whose buffer is a different length.
    #[track_caller]
    fn refused_rig(json: &str) -> String {
        match import_rigged_glb(json) {
            Err(error) => error.to_string(),
            Ok(scene) => {
                panic!("this document should have been refused, and it imported: {scene:?}")
            }
        }
    }

    /// Every way a rig can name something that is not there, refused.
    ///
    /// Each of these is an `unwrap` inside `gltf` over a number the file
    /// supplied — `Skin::joints`, `Skin::skeleton`, `Channel::sampler`,
    /// `Target::node` — or an accessor whose element size `accessor::Iter`
    /// asserts. Without the checks they would abort the process rather than
    /// return, which is this module's whole reason to exist, so each is
    /// exercised rather than assumed.
    #[test]
    fn every_malformed_rig_is_refused_with_the_reason_it_was_refused_for() {
        let base = rigged_json(BIN_CHUNK_BUFFER);
        for (from, to, because) in [
            (
                r#""joints": [1, 2]"#,
                r#""joints": [1, 9]"#,
                "names joint node 9, and there are 3 nodes",
            ),
            (
                r#""skin": 0"#,
                r#""skin": 7"#,
                "wears skin 7, and there are 1",
            ),
            (
                r#""skeleton": 1"#,
                r#""skeleton": 9"#,
                "names skeleton node 9, and there are 3 nodes",
            ),
            (
                r#""inverseBindMatrices": 6"#,
                r#""inverseBindMatrices": 9"#,
                "inverseBindMatrices names accessor 9",
            ),
            (
                r#""target": { "node": 2, "path": "rotation" }"#,
                r#""target": { "node": 9, "path": "rotation" }"#,
                "targets node 9, and there are 3 nodes",
            ),
            (
                r#""channels": [{ "sampler": 0,"#,
                r#""channels": [{ "sampler": 9,"#,
                "names sampler 9, and animation 0 has 1",
            ),
            (
                r#""samplers": [{ "input": 7, "output": 8"#,
                r#""samplers": [{ "input": 9, "output": 8"#,
                "input names accessor 9",
            ),
            (
                r#""samplers": [{ "input": 7, "output": 8"#,
                r#""samplers": [{ "input": 7, "output": 9"#,
                "output names accessor 9",
            ),
        ] {
            let reason = refused_rig(&replacing(&base, from, to));
            assert!(
                reason.contains(because),
                "{to:?} was refused for {reason:?}, and the reason should say {because:?}"
            );
        }
    }

    /// A rotation channel whose output is not a quaternion is refused rather
    /// than reaching `ReadOutputs`.
    ///
    /// Separate from the table because the mutation is a *type*, not an index:
    /// accessor 8 stays in range and in bounds, so nothing before
    /// `check_animations` has any reason to object — and
    /// `Reader::read_outputs` matches a rotation's data type with an
    /// `unreachable!()` arm.
    #[test]
    fn a_rotation_channel_whose_output_is_not_a_quaternion_is_refused() {
        let json = replacing(
            &rigged_json(BIN_CHUNK_BUFFER),
            r#"{ "bufferView": 8, "componentType": 5126, "count": 2, "type": "VEC4" }"#,
            r#"{ "bufferView": 8, "componentType": 5126, "count": 2, "type": "VEC3" }"#,
        );
        let reason = refused_rig(&json);
        assert!(
            reason.contains("output is Valid(Vec3)"),
            "unexpected reason: {reason}"
        );
    }

    /// `JOINTS_0` as a float is refused: `Reader::read_joints` has variants for
    /// the two unsigned integer spellings the specification allows and an
    /// `unreachable!()` for everything else.
    #[test]
    fn a_float_joints_attribute_is_refused() {
        let json = replacing(
            &rigged_json(BIN_CHUNK_BUFFER),
            r#""JOINTS_0": 4"#,
            r#""JOINTS_0": 5"#,
        );
        let reason = refused_rig(&json);
        assert!(
            reason.contains("JOINTS_0 is") && reason.contains("F32"),
            "unexpected reason: {reason}"
        );
    }

    /// Named separately from the table because the claim is about the mesh
    /// primitive's `POSITION` index specifically: that is the one
    /// `gltf::Gltf::from_slice` aborts the process on rather than reporting.
    #[test]
    fn an_out_of_range_position_accessor_is_refused() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            "\"POSITION\": 0",
            "\"POSITION\": 9",
        );
        assert!(refused(&json).contains("POSITION names accessor 9"));
    }

    #[test]
    fn a_glb_that_declares_a_length_below_its_own_header_is_refused() {
        let mut bytes = glb(&triangle_json(BIN_CHUNK_BUFFER), Some(&triangle_bin()));
        bytes[8..12].copy_from_slice(&4u32.to_le_bytes());
        let error = import_glb_bytes(&bytes).unwrap_err().to_string();
        assert!(
            error.contains("declares a total length of 4"),
            "unexpected reason: {error}"
        );
    }

    #[test]
    fn a_glb_shorter_than_a_header_is_refused() {
        for length in 0..GLB_HEADER_LEN {
            let bytes = &b"glTF\x02\x00\x00\x00\x64\x00\x00\x00"[..length];
            let error = import_glb_bytes(bytes).unwrap_err();
            assert!(
                matches!(&error, StorageError::Other(_)),
                "{length} bytes gave {error:?}"
            );
        }
    }

    #[test]
    fn a_truncated_glb_is_refused() {
        let whole = glb(&triangle_json(BIN_CHUNK_BUFFER), Some(&triangle_bin()));
        for cut in [GLB_HEADER_LEN + 1, whole.len() / 2, whole.len() - 1] {
            let error = import_glb_bytes(&whole[..cut]).unwrap_err();
            assert!(
                matches!(&error, StorageError::Other(_)),
                "a {cut}-byte prefix of a {}-byte file gave {error:?}",
                whole.len()
            );
        }
    }

    #[test]
    fn a_glb_chunk_length_that_overruns_the_file_is_refused() {
        let mut bytes = glb(&triangle_json(BIN_CHUNK_BUFFER), Some(&triangle_bin()));
        // The JSON chunk's declared length, immediately after the 12-byte
        // header, made larger than everything that follows it.
        let overrun = u32::try_from(bytes.len()).unwrap() * 2;
        bytes[12..16].copy_from_slice(&overrun.to_le_bytes());
        let error = import_glb_bytes(&bytes).unwrap_err().to_string();
        assert!(
            error.contains("JSON chunk length exceeds that of slice"),
            "unexpected reason: {error}"
        );
    }

    #[test]
    fn a_glb_with_no_bin_chunk_cannot_hold_a_buffer_that_omits_its_uri() {
        let bytes = glb(&triangle_json(BIN_CHUNK_BUFFER), None);
        let error = import_glb_bytes(&bytes).unwrap_err().to_string();
        assert!(
            error.contains("buffer 0 has no uri"),
            "unexpected reason: {error}"
        );
    }

    #[test]
    fn a_second_buffer_may_not_claim_the_bin_chunk_the_first_one_took() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            "\"buffers\": [{ \"byteLength\": 102 }]",
            "\"buffers\": [{ \"byteLength\": 102 }, { \"byteLength\": 102 }]",
        );
        assert!(refused(&json).contains("buffer 1 has no uri"));
    }

    #[test]
    fn a_buffer_that_declares_more_bytes_than_arrived_is_refused() {
        let json = replacing(
            &triangle_json(EXTERNAL_BUFFER),
            "\"byteLength\": 102,",
            "\"byteLength\": 400,",
        );
        let error = import_gltf_text(&json, &triangle_bin())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("buffer 0 declares 400 bytes and 102 arrived"),
            "unexpected reason: {error}"
        );
    }

    /// Two shapes of buffer URI that are legal glTF and are not legal asset
    /// keys. Both are refused by the source, before anything reads a file: the
    /// escaping one has a real file on the other end of it, so a traversal that
    /// got through would return those bytes rather than an error.
    #[test]
    fn a_buffer_uri_that_is_not_a_legal_asset_key_never_reaches_the_filesystem() {
        for uri in ["../secrets.bin", "/etc/passwd", "sub%20dir/triangle.bin"] {
            let assets = Assets::new();
            std::fs::write(assets.outside().join("secrets.bin"), triangle_bin()).unwrap();
            let json = replacing(
                &triangle_json(EXTERNAL_BUFFER),
                "\"uri\": \"triangle.bin\"",
                &format!("\"uri\": {uri:?}"),
            );
            assets.write("meshes/model.gltf", json.as_bytes());
            let error = assets.import("meshes/model.gltf").unwrap_err();
            assert!(
                matches!(error, StorageError::InvalidPath(_)),
                "{uri:?} gave {error:?}"
            );
            assert!(
                std::fs::read(assets.outside().join("secrets.bin")).is_ok(),
                "the file an escape would have reached is still there"
            );
        }
    }

    #[test]
    fn a_data_uri_buffer_is_refused_rather_than_decoded() {
        let json = replacing(
            &triangle_json(EXTERNAL_BUFFER),
            "\"uri\": \"triangle.bin\"",
            "\"uri\": \"data:application/octet-stream;base64,AAAAAA==\"",
        );
        let error = import_gltf_text(&json, &triangle_bin()).unwrap_err();
        assert!(
            matches!(error, StorageError::Unsupported(reason) if reason.contains("data: URI")),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn a_sparse_accessor_is_refused_rather_than_read_as_a_dense_one() {
        let json = replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            "{ \"bufferView\": 3, \"componentType\": 5123, \"count\": 3, \"type\": \"SCALAR\" }",
            "{ \"bufferView\": 3, \"componentType\": 5123, \"count\": 3, \"type\": \"SCALAR\", \
             \"sparse\": { \"count\": 1, \
             \"indices\": { \"bufferView\": 3, \"componentType\": 5123 }, \
             \"values\": { \"bufferView\": 3 } } }",
        );
        assert!(matches!(
            import_glb(&json),
            Err(StorageError::Unsupported(reason)) if reason.contains("sparse")
        ));
    }

    #[test]
    fn an_index_past_the_end_of_its_own_vertex_array_is_refused() {
        let mut bin = triangle_bin();
        // The third index, at byte 100 of the layout `triangle_bin` documents.
        bin[100..102].copy_from_slice(&9u16.to_le_bytes());
        let error = import_gltf_text(&triangle_json(EXTERNAL_BUFFER), &bin)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("has index 9 and 3 vertices"),
            "unexpected reason: {error}"
        );
    }

    #[test]
    fn a_document_that_is_not_gltf_at_all_is_refused() {
        for bytes in [&b""[..], b"not json", b"{}", b"{\"asset\":{}}", b"glTF"] {
            let error = import_glb_bytes(bytes).unwrap_err();
            assert!(
                matches!(&error, StorageError::Other(_)),
                "{bytes:?} gave {error:?}"
            );
        }
    }
}
