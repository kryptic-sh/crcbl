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
//! # Why meshlet clustering and mesh simplification are here too
//!
//! [`meshlet`] and [`mod@simplify`] belong beside the other two for the same
//! reason: `docs/plan/03-gpu-driven-rendering.md` §3.5's cluster build and
//! `docs/plan/25-lod.md`'s QEM decimation are both bake steps over exactly the
//! host arrays [`import_gltf`] produces — positions and indices — so they land
//! in the crate that owns them rather than becoming further names for the same
//! responsibility. [`mod@lod`] is the two of them composed into
//! `docs/plan/25-lod.md`'s chain of levels, and [`mod@cluster_dag`] is the same
//! two composed into its cluster DAG — the structure per-cluster selection
//! needs, which a chain cannot provide. All of them are host-side builders
//! alone; nothing consumes them yet, and each module's docs say what is still
//! missing.
//!
//! # No GPU work here
//!
//! [`import_gltf`], [`build_meshlets`], [`simplify`](simplify()),
//! [`build_lod_chain`] and [`build_cluster_dag`] all end at host memory: vertex
//! arrays, index arrays, [`crcbl_shaders::mesh::GpuMaterial`] rows, cluster
//! records and simplified levels. Pool upload, textures and mip generation are
//! the second half of step 3 and belong to the crate that owns the pools.

pub mod cluster_dag;
pub mod gltf_check;
#[cfg(test)]
mod gltf_fixture;
pub mod gltf_import;
pub mod lod;
pub mod meshlet;
pub mod simplify;

pub use cluster_dag::{
    ClusterDag, ClusterDagError, ClusterGroup, DagLevel, GroupBounds, build_cluster_dag,
};
pub use gltf_import::{GltfInstance, GltfMesh, GltfPrimitive, GltfScene, import_gltf};
pub use lod::{DEFAULT_LOD_RATIOS, LodError, LodLevel, build_lod_chain};
pub use meshlet::{ClusterBounds, Meshlet, MeshletBuild, MeshletError, build_meshlets};
pub use simplify::{Simplified, SimplifyError, simplify, simplify_with_locked_edges};
