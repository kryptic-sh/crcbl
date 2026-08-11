//! The cluster buffers `mesh_cluster.slang` reads: §3.5's meshlets, resident on
//! the GPU.
//!
//! Three storage buffers and a range per mesh. The **clusters** are
//! [`crcbl_shaders::meshlet::Meshlet`] records back to back in mesh order; the
//! **vertex** array is the original vertex indices, one run per cluster; and the
//! **corner** array is `u8` corner indices packed four to a word. Every one of
//! those shapes is decided by the record, not here — see that module, which is
//! where both sides of the packing are written.
//!
//! # It receives clusters; it does not build them
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.5 makes the meshlet build a **bake
//! step**, and the layering makes it one whether or not it wants to be:
//! `crcbl_scene::meshlet::build_meshlets` is the builder, `crcbl-scene` pulls in
//! `gltf`, and a renderer that depended on the glTF importer to describe its own
//! geometry would have the direction backwards. So a caller hands this module
//! cooked clusters. [`crcbl_shaders::meshlet::cube_clusters`] and its pyramid
//! siblings are the cooked form of the meshes the forward pass has resident,
//! and `crcbl-scene`'s own test is what checks them against the real builder.
//!
//! # It is only built on the mesh-shader path
//!
//! A device on either indirect tail draws the same geometry out of the index
//! pool, so nothing reads these buffers and [`crate::forward`] does not create
//! them. That is why this is a pool of its own rather than three more fields on
//! [`MeshPool`](crate::mesh_pool::MeshPool): the geometry pools are every
//! device's, and these are one path's.

use crcbl_hal::{BufferDesc, BufferHandle, BufferUsage, Device, HalError, MemoryLocation};
use crcbl_shaders::meshlet::{self, MeshClusters};

/// Where one mesh's clusters live in [`ClusterPool::clusters`].
///
/// The two numbers a bucket's [`ClusterDrawConstants`] block carries, and the
/// x extent of the dispatch that draws it.
///
/// [`ClusterDrawConstants`]: crcbl_shaders::meshlet::ClusterDrawConstants
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClusterRange {
    /// This mesh's first cluster.
    pub base: u32,
    /// How many clusters it has. Never zero for a mesh with triangles, which is
    /// what lets the mesh stage read cluster `base` unconditionally.
    pub count: u32,
}

/// Several meshes' cluster arrays laid end to end, before any of it is a
/// buffer.
///
/// Separate from [`ClusterPool::new`] so the arithmetic that shifts a mesh's
/// offsets is testable without a device — which is what a test of it has to be
/// to be a test at all: the seam takes bytes and hands back a handle, so a pool
/// that concatenated wrongly uploads exactly as many bytes as one that did not.
#[derive(Debug, Default, PartialEq)]
struct Concatenated {
    clusters: Vec<meshlet::Meshlet>,
    vertices: Vec<u32>,
    corners: Vec<u8>,
    ranges: Vec<ClusterRange>,
}

/// Lays `meshes`' three arrays end to end, moving each cluster's offsets by
/// what came before it.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] when an offset outgrows the `u32` the record
/// holds, or when `meshes` cluster into nothing.
fn concatenate(meshes: &[MeshClusters]) -> Result<Concatenated, HalError> {
    let mut out = Concatenated::default();
    for mesh in meshes {
        let base = u32::try_from(out.clusters.len()).map_err(|_| {
            HalError::InvalidDescriptor(
                "more clusters than a u32 can name; the record's offsets are 32-bit".to_string(),
            )
        })?;
        // Every offset in this mesh moves by however much came before it.
        // Rebuilt through `Meshlet::new` rather than added into the existing
        // fields, because that is the constructor that refuses an offset a
        // `u32` cannot hold — and a wrapped one would point at the previous
        // mesh's corners.
        for cluster in &mesh.clusters {
            let shifted = meshlet::Meshlet::new(
                cluster.vertex_offset as usize + out.vertices.len(),
                cluster.vertex_count as usize,
                cluster.triangle_offset as usize + out.corners.len(),
                cluster.triangle_count as usize,
                cluster.bounds,
            )
            .map_err(|error| HalError::InvalidDescriptor(error.to_string()))?;
            out.clusters.push(shifted);
        }
        out.vertices.extend_from_slice(&mesh.vertices);
        out.corners.extend_from_slice(&mesh.corners);
        let count = u32::try_from(mesh.clusters.len())
            .unwrap_or_else(|_| unreachable!("the running total above would have refused first"));
        out.ranges.push(ClusterRange { base, count });
    }
    if out.clusters.is_empty() {
        return Err(HalError::InvalidDescriptor(
            "a cluster pool with no clusters has nothing for a mesh stage to read".to_string(),
        ));
    }
    Ok(out)
}

/// The three cluster buffers, and where each mesh's clusters are in them.
#[derive(Debug)]
pub struct ClusterPool {
    clusters: BufferHandle,
    vertices: BufferHandle,
    corners: BufferHandle,
    ranges: Vec<ClusterRange>,
}

impl ClusterPool {
    /// Uploads `meshes`' clusters, concatenated in the order given, and returns
    /// the pool plus a range per mesh.
    ///
    /// The three buffers are `HostUpload` rather than device-local with a
    /// staging copy, on [`InstancePool`](crate::instance_pool::InstancePool)'s
    /// terms: they are written once at start-up and read by every frame, and a
    /// staging copy would buy a transfer submit and a barrier for bytes that
    /// never change. A bake cache that streams clusters is what would make them
    /// device-local, and that is §3.5's own later slice.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call, and
    /// [`HalError::InvalidDescriptor`] if `meshes` cluster into nothing — a
    /// pipeline whose cluster buffer is empty has a mesh stage that cannot
    /// safely read element zero, which is the read its out-of-range path makes.
    pub fn new(
        device: &dyn Device,
        label: &str,
        meshes: &[MeshClusters],
    ) -> Result<Self, HalError> {
        let Concatenated {
            clusters,
            vertices,
            corners,
            ranges,
        } = concatenate(meshes)?;

        let mut created: Vec<BufferHandle> = Vec::with_capacity(3);
        let mut upload = |name: &str, bytes: &[u8]| -> Result<BufferHandle, HalError> {
            let buffer = device.create_buffer(&BufferDesc {
                label: Some(&format!("{label} {name}")),
                size: bytes.len() as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })?;
            created.push(buffer);
            device.write_buffer(buffer, 0, bytes)?;
            Ok(buffer)
        };
        // Each `?` can leave the ones before it created, so the whole sequence
        // is rolled back on the way out rather than leaked — the same shape
        // every other pool in this crate uses.
        let built = (|| {
            let cluster_buffer = upload("clusters", &meshlet::cluster_bytes(&clusters))?;
            let vertex_bytes: Vec<u8> = vertices.iter().flat_map(|v| v.to_le_bytes()).collect();
            let vertex_buffer = upload("cluster vertices", &vertex_bytes)?;
            let corner_buffer = upload("cluster corners", &meshlet::corner_bytes(&corners))?;
            Ok::<_, HalError>((cluster_buffer, vertex_buffer, corner_buffer))
        })();
        match built {
            Ok((clusters, vertices, corners)) => Ok(Self {
                clusters,
                vertices,
                corners,
                ranges,
            }),
            Err(error) => {
                for buffer in created {
                    device.destroy_buffer(buffer);
                }
                Err(error)
            }
        }
    }

    /// The cluster records, as `mesh_cluster.slang`'s binding 9.
    #[must_use]
    pub const fn clusters(&self) -> BufferHandle {
        self.clusters
    }

    /// The per-cluster vertex runs, as binding 10.
    #[must_use]
    pub const fn vertices(&self) -> BufferHandle {
        self.vertices
    }

    /// The packed corner words, as binding 11.
    #[must_use]
    pub const fn corners(&self) -> BufferHandle {
        self.corners
    }

    /// Where mesh `index`'s clusters are, in the order [`ClusterPool::new`]
    /// received them.
    #[must_use]
    pub fn range(&self, index: usize) -> Option<ClusterRange> {
        self.ranges.get(index).copied()
    }

    /// Releases the three buffers. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        for buffer in [self.clusters, self.vertices, self.corners] {
            device.destroy_buffer(buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::{NullInstance, Recorder};
    use crcbl_hal::{DeviceDesc, Instance, QueueKind};

    fn open() -> (Recorder, Box<dyn Device>) {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let _ = device.queue(QueueKind::Graphics);
        (recorder, device)
    }

    /// **A second mesh's clusters address their own runs.** That is the whole
    /// job of concatenating three arrays: every offset in the second mesh moves
    /// by the length of the first's runs, and one that did not would draw the
    /// first mesh's corners under the second mesh's name — same triangle count,
    /// same buffer sizes, wrong geometry.
    ///
    /// Against [`concatenate`] rather than through a device, because a device
    /// cannot see it: the seam takes bytes and hands back a handle, so a pool
    /// that shifted nothing uploads exactly as many bytes as one that did.
    #[test]
    fn the_second_mesh_s_clusters_point_past_the_first_s_runs() {
        let cube = crcbl_shaders::meshlet::cube_clusters();
        let pyramid = crcbl_shaders::meshlet::pyramid_clusters();
        let laid_out = concatenate(&[cube.clone(), pyramid.clone()]).expect("both meshes fit");

        assert_eq!(
            laid_out.ranges,
            vec![
                ClusterRange {
                    base: 0,
                    count: u32::try_from(cube.clusters.len()).expect("small"),
                },
                ClusterRange {
                    base: u32::try_from(cube.clusters.len()).expect("small"),
                    count: u32::try_from(pyramid.clusters.len()).expect("small"),
                },
            ],
            "each mesh's clusters start where the last mesh's ended"
        );

        // The cooked records are mesh-relative — both meshes' first cluster
        // starts at offset zero — which is what makes the shift observable at
        // all rather than a no-op that happens to agree.
        assert_eq!(pyramid.clusters[0].vertex_offset, 0);
        assert_eq!(pyramid.clusters[0].triangle_offset, 0);
        assert!(
            !cube.vertices.is_empty() && !cube.corners.is_empty(),
            "the first mesh has to have runs for the second one's shift to be \
             anything but zero"
        );

        let shifted = laid_out.clusters[cube.clusters.len()];
        assert_eq!(
            shifted.vertex_offset,
            u32::try_from(cube.vertices.len()).expect("small"),
            "the pyramid's vertex run starts where the cube's ended"
        );
        assert_eq!(
            shifted.triangle_offset,
            u32::try_from(cube.corners.len()).expect("small"),
            "and so does its corner run"
        );
        assert_eq!(
            shifted.bounds, pyramid.clusters[0].bounds,
            "the bounds are carried through unchanged — only the offsets move"
        );
        assert_eq!(
            (laid_out.vertices.len(), laid_out.corners.len()),
            (
                cube.vertices.len() + pyramid.vertices.len(),
                cube.corners.len() + pyramid.corners.len()
            ),
            "and both runs hold every entry of both meshes"
        );
    }

    /// The pool uploads what [`concatenate`] produced, at the stride the shader
    /// reads — checked against the writes the recorder saw rather than against
    /// a host-side copy.
    #[test]
    fn the_pool_uploads_every_cluster_of_every_mesh() {
        let (recorder, device) = open();
        let cube = crcbl_shaders::meshlet::cube_clusters();
        let pyramid = crcbl_shaders::meshlet::pyramid_clusters();
        let pool = ClusterPool::new(device.as_ref(), "test", &[cube.clone(), pyramid.clone()])
            .expect("the null backend accepts every descriptor");

        assert_eq!(pool.range(2), None, "there is no third mesh");
        let written = |buffer| {
            recorder
                .events()
                .into_iter()
                .find_map(|event| match event {
                    crcbl_hal::null::Event::BufferWritten {
                        buffer: written,
                        len,
                        ..
                    } if written == buffer => Some(len),
                    _ => None,
                })
                .expect("the buffer was written")
        };
        assert_eq!(
            written(pool.clusters()),
            (cube.clusters.len() + pyramid.clusters.len()) * crcbl_shaders::meshlet::MESHLET_STRIDE
        );
        assert_eq!(
            written(pool.vertices()),
            (cube.vertices.len() + pyramid.vertices.len()) * 4
        );
        assert_eq!(
            written(pool.corners()),
            (cube.corners.len() + pyramid.corners.len()).div_ceil(4) * 4,
            "corners pack four to a word, tail included"
        );

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A pool with nothing in it is refused rather than created empty: the mesh
    /// stage reads cluster `cluster_base` on its out-of-range path, and an
    /// empty buffer makes that read out of bounds.
    #[test]
    fn a_pool_with_no_clusters_is_refused() {
        let (_, device) = open();
        let error = ClusterPool::new(device.as_ref(), "test", &[])
            .expect_err("an empty pool has nothing to draw");
        assert!(
            matches!(error, HalError::InvalidDescriptor(ref message) if message.contains("no clusters")),
            "{error}"
        );
    }

    /// Every buffer created is destroyed, which is the leak assertion the rest
    /// of this crate's pools carry.
    #[test]
    fn the_pool_leaks_nothing() {
        let (recorder, device) = open();
        let before = recorder.total_live_objects();
        let pool = ClusterPool::new(
            device.as_ref(),
            "test",
            &[crcbl_shaders::meshlet::cube_clusters()],
        )
        .expect("built");
        assert!(recorder.total_live_objects() > before);
        pool.destroy(device.as_ref());
        assert_eq!(recorder.total_live_objects(), before);
        recorder.assert_valid();
    }
}
