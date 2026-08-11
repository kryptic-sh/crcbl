//! Scene format and glTF import.
//!
//! `docs/plan/06-assets-scenes.md` splits content into *source formats, which
//! are grounded open standards* and *cooked formats, which are ours*. This
//! crate holds the two halves that meet: reading glTF 2.0, the source format
//! for meshes, skins and animations ([`import_gltf`]); and — when step 4 lands
//! — the `.scn/` directory of RON chunk files, which is the one format the
//! engine owns because it owns the semantics.
//!
//! # Why the importer is here and not in `crcbl-assets`
//!
//! `crcbl-assets` is the IO seam: [`crcbl_assets::AssetSource`] answers *the
//! bytes of this key* and its own module docs say decoding is somebody else's
//! job. The other formats already follow that — PNG is decoded in
//! `crcbl-sprite`, WAV in `crcbl-audio` — because each landed in the crate that
//! owned the thing being decoded. glTF's owner is this crate: its package
//! description has said "scene format and glTF import" since the workspace
//! skeleton, `docs/plan/12-testing.md`'s anchor list assigns the glTF corpus to
//! it, and the dependency direction the plan states — `crcbl-scene` →
//! `crcbl-assets` — is the one an importer that reads through the asset seam
//! actually needs.
//!
//! A crate of its own was the alternative and would have been a third name for
//! the same responsibility, next to a crate whose stated purpose it took.
//!
//! # No GPU work here
//!
//! [`import_gltf`] ends at host memory: vertex arrays, index arrays, and
//! [`crcbl_shaders::mesh::GpuMaterial`] rows. Pool upload, textures and mip
//! generation are the second half of step 3 and belong to the crate that owns
//! the pools.

pub mod gltf_check;
#[cfg(test)]
mod gltf_fixture;
pub mod gltf_import;

pub use gltf_import::{GltfInstance, GltfMesh, GltfPrimitive, GltfScene, import_gltf};
