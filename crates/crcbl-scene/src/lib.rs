//! Scene format and glTF import.
//!
//! `docs/plan/06-assets-scenes.md` splits content into *source formats, which
//! are grounded open standards* and *cooked formats, which are ours*. This
//! crate holds the two halves that meet: reading glTF 2.0, the source format
//! for meshes, skins and animations (`import_gltf`, behind the default `gltf`
//! feature); and [`scn`], the `.scn/` directory of RON chunk files, which is
//! the one format the engine owns because it owns the semantics.
//!
//! # The two halves are separable, and the feature is where
//!
//! `gltf` is on by default and gates everything spelled in terms of a glTF
//! document — `gltf_import`, `gltf_check`, `gltf_render`, `gltf_fixture` and
//! `lod_resolve`. Off, what is left is [`scn`] and the host builders over plain
//! vertex and index arrays ([`meshlet`], [`mod@simplify`], [`mod@lod`],
//! [`mod@cluster_dag`]), which is the build a browser wants: a game whose level
//! is a `.scn/` reads no source format at run time, and `crcbl`'s `scn` feature
//! is how it says so. See this crate's `Cargo.toml` and the root workspace
//! entry, which is where `default-features = false` lives.
//!
//! # Why the importer is here and not in `crcbl-assets`
//!
//! `crcbl-assets` is the IO seam: [`crcbl_assets::AssetSource`] answers *the
//! bytes of this key* and its own module docs say decoding is somebody else's
//! job. The other formats already follow that — PNG is decoded in
//! `crcbl-sprite`, WAV in `crcbl-audio` — because each landed in the crate that
//! owned the thing being decoded. glTF's owner is this crate: its package
//! description has said "scene format and glTF import" since the workspace
//! skeleton, `docs/notes/process.md`'s anchor list assigns the glTF corpus to
//! it, and the dependency direction the plan states — `crcbl-scene` →
//! `crcbl-assets` — is the one an importer that reads through the asset seam
//! actually needs.
//!
//! A crate of its own was the alternative and would have been a third name for
//! the same responsibility, next to a crate whose stated purpose it took.
//!
//! # Why meshlet clustering and mesh simplification are here too
//!
//! [`meshlet`] and [`mod@simplify`] belong beside the other two for the same
//! reason: topic 03 §3.5's cluster build and
//! topic 25's QEM decimation are both bake steps over exactly the
//! host arrays `import_gltf` produces — positions and indices — so they land
//! in the crate that owns them rather than becoming further names for the same
//! responsibility. [`mod@lod`] is the two of them composed into
//! topic 25's chain of levels, and [`mod@cluster_dag`] is the same
//! two composed into its cluster DAG — the structure per-cluster selection
//! needs, which a chain cannot provide. `lod_resolve` is where that meets
//! the importer: the plan's hand-authored precedence, deciding per level
//! whether the file supplied it or the generator has to. All of them are
//! host-side builders alone; nothing consumes them yet, and each module's docs
//! say what is still missing.
//!
//! # No GPU work here
//!
//! `import_gltf`, [`build_meshlets`], [`simplify`](simplify()),
//! [`build_lod_chain`] and [`build_cluster_dag`] all end at host memory: vertex
//! arrays, index arrays, [`crcbl_shaders::mesh::GpuMaterial`] rows, cluster
//! records and simplified levels. Pool upload, textures and mip generation are
//! the second half of step 3 and belong to the crate that owns the pools.

pub mod cluster_dag;
#[cfg(feature = "gltf")]
pub mod gltf_check;
// Fixtures, not engine surface: `pub` only so a gate in another crate can
// generate the `.glb` it needs, and only when it asks for the feature. The
// module's own header says what that exposes and what it leaves `pub(crate)`.
#[cfg(all(feature = "gltf", any(test, feature = "gltf-fixture")))]
pub mod gltf_fixture;
#[cfg(feature = "gltf")]
pub mod gltf_import;
#[cfg(feature = "render")]
pub mod gltf_render;
pub mod lod;
// Behind `gltf` with the importer rather than beside `lod`: `resolve_lod` takes
// a `&GltfScene`, so the hand-authored half of topic 25's
// precedence is a reading of the source format and does not exist without it.
#[cfg(feature = "gltf")]
pub mod lod_resolve;
pub mod meshlet;
pub mod scn;
pub mod simplify;

pub use cluster_dag::{
    ClusterDag, ClusterDagError, ClusterGroup, DagLevel, GroupBounds, build_cluster_dag,
};
#[cfg(feature = "gltf")]
pub use gltf_import::{
    GltfChannel, GltfClip, GltfImage, GltfInstance, GltfInterpolation, GltfMesh, GltfNode,
    GltfPrimitive, GltfSamples, GltfScene, GltfSkin, GltfTexture, import_gltf,
};
#[cfg(feature = "render")]
pub use gltf_render::{MeshOrigin, RenderScene, Skip, build_render_scene};
pub use lod::{DEFAULT_LOD_RATIOS, LodError, LodLevel, build_lod_chain};
#[cfg(feature = "gltf")]
pub use lod_resolve::{HandLodLink, LodOrigin, LodResolveError, MeshLod, resolve_lod};
pub use meshlet::{ClusterBounds, Meshlet, MeshletBuild, MeshletError, build_meshlets};
pub use scn::{Env, EnvCamera, IdMap, Scene, SceneEntityId, ScnError, SystemChunk, chunk_of};
pub use simplify::{Simplified, SimplifyError, simplify, simplify_with_locked_edges};
