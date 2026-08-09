//! Global geometry pools: one vertex buffer, one index buffer, and a mesh that
//! is three integers.
//!
//! ```text
//! upload(vertices, indices) ──▶ suballocate both pools ──▶ staging ▸ copy ▸ submit
//!                                                                       │ signals
//!                                                                       ▼
//!                                                              timeline value N
//!                                                                       │
//!  mesh(handle) ─── None until the timeline has passed N ────────────────┘
//!                └── Some({base_vertex, base_index, index_count}) after
//! ```
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.1: "One large vertex pool + one
//! index pool (device-local, suballocated…). Meshes are
//! `{base_vertex, base_index, count}` ranges — a mesh handle is three integers."
//! Everything above this — instance data, GPU culling, indirect draws, meshlets
//! — assumes a mesh is a range rather than a buffer, and none of it is here.
//!
//! # One pool means one vertex format
//!
//! The vertex pool is suballocated in **vertices**, not bytes, so
//! [`MeshRange::base_vertex`] is a vertex index the shader can add and no caller
//! divides anything. That only works because every mesh in the pool has the same
//! stride, which is §3.1's "vertex pulling everywhere" stated as a data layout:
//! [`crcbl_shaders::mesh::VERTEX_STRIDE`] is *the* vertex format, and a second
//! one means a second pool rather than a stride field on this one.
//!
//! Indices are `Uint32` for the same reason, and are stored **mesh-relative**,
//! so the bytes uploaded for a mesh do not depend on where in the pool it
//! landed and a mesh can be freed and re-uploaded at a different base without
//! being rewritten.
//!
//! **The base is not `draw_indexed`'s own `base_vertex` argument.** That is the
//! obvious place for it and it is where this pool's first consumer put it, until
//! the second resident showed what it does: the four shading languages disagree
//! about whether a draw's base vertex is folded into `SV_VertexID`, and Slang's
//! SPIR-V subtracts it back out. So [`crate::forward`] passes zero there and
//! hands this number to the shader as data instead — see that module and
//! `mesh.slang`'s header, which measure all four targets.
//!
//! # Fragmentation and exhaustion, which are not the same failure
//!
//! The suballocator is a first-fit free list that **coalesces on every free**,
//! so two neighbouring frees become one block. What it never does is move a
//! resident mesh: §3.1's risk section is explicit — "MVP: free-list + offline
//! compaction on load only. **No live defrag.**"
//!
//! The consequence is the case a naive free list gets wrong, and it is
//! documented behaviour rather than a bug: **a request no single free block can
//! satisfy fails even when the total free space would have been enough.**
//! [`MeshPoolError::PoolExhausted`] names both numbers so a caller can tell the
//! two apart — a largest block far below the total is fragmentation, and a total
//! below the request is a pool that is simply full. "Offline compaction" here
//! means freeing everything and uploading again, which is what a load-time asset
//! system does anyway.
//!
//! # The pools never grow
//!
//! Capacity is fixed by [`MeshPoolDesc`] and exhaustion is an error naming it.
//! Growing a device-local pool means allocating a larger buffer, copying, and
//! re-pointing every bind group that names the old one — the live-defrag cost
//! §3.1 rules out, paid under a different name and in the middle of a frame.
//! [`crcbl_core::FrameArena`] fixes its capacity for the same reason its docs
//! give: a bound that grows on demand is indistinguishable from a leak.
//!
//! # Residency is the point of the timeline
//!
//! §3.1: "renderer consumes meshes only after the timeline value passes". Each
//! upload's transfer submit signals a value on the pool's own timeline
//! semaphore, and [`MeshPool::mesh`] answers `None` until that value has been
//! observed to pass — so a caller cannot obtain a range for a mesh whose bytes
//! may not have landed. [`MeshPool::flush`] is what makes it pass, and it is
//! also what retires the staging buffers, because a staging buffer freed before
//! its copy has run is the same bug from the other end.
//!
//! The semaphore is this crate's, not [`crcbl_hal`]'s to invent: timeline
//! semaphores are the seam's one synchronisation primitive (see
//! [`crcbl_hal::SemaphoreKind`], whose docs name this staging path), and the
//! `wait_semaphores`-or-`wait_idle` fallback for a device without them is the
//! one `crcbl::GpuContext::retire_to` already uses to retire command buffers.
//!
//! # This is a start-up path
//!
//! Like [`crate::texture`] and for the same reason, [`MeshPool::upload`] records
//! its own barriers and [`MeshPool::flush`] blocks. Both are legal because there
//! is no graph in the room: uploads happen before the frame loop, and the crate
//! docs' "no manual barriers outside the graph" names exactly these. Streaming a
//! mesh in *during* a frame is P9's, and it is the reason residency is a query
//! rather than an assertion — the shape survives when `flush` becomes a poll.

use core::ops::Range;
use core::time::Duration;

use crcbl_core::{Handle, Pool};
use crcbl_hal::{
    Barriers, BufferBarrier, BufferCopy, BufferDesc, BufferHandle, BufferUsage,
    CommandBufferHandle, CommandEncoderDesc, Device, Features, HalError, MemoryLocation,
    QueueHandle, ResourceState, SemaphoreDesc, SemaphoreHandle, SemaphoreKind, SemaphoreSignal,
    SemaphoreWait, SubmitInfo,
};
use crcbl_shaders::mesh::VERTEX_STRIDE;

/// Bytes per index. `Uint32` everywhere, so the index pool suballocates in
/// indices and [`MeshRange::base_index`] is `draw_indexed`'s own first-index.
const INDEX_STRIDE: u64 = 4;

/// How long [`MeshPool::flush`] waits for its transfers before calling the
/// device hung.
///
/// Not `u64::MAX`: a wait that cannot time out turns a lost device into a
/// process that never returns, and this one runs at start-up where there is
/// still a user watching. Long enough that a loaded CI machine running a
/// software rasteriser is not mistaken for one.
pub const UPLOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// Marker type for [`MeshHandle`]. Uninhabited.
#[derive(Debug)]
pub enum Mesh {}

/// A mesh in a [`MeshPool`]. Generational, so a freed mesh's handle stops
/// resolving rather than naming whatever took its place.
pub type MeshHandle = Handle<Mesh>;

/// Where a mesh lives in the pools — §3.1's three integers, and the only thing
/// a draw needs.
///
/// Two of the three are `draw_indexed`'s own arguments — the draw is
/// `draw_indexed(base_index..base_index + index_count, 0, …)` — and
/// [`MeshRange::base_vertex`] is deliberately not the third. The
/// [module docs](self) say why it reaches the shader as data instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeshRange {
    /// First vertex of this mesh in the vertex pool. Added to every index the
    /// shader reads, which is why the indices themselves are mesh-relative.
    ///
    /// Never above `i32::MAX`: [`MeshPool::new`] refuses a vertex capacity that
    /// a draw's *signed* base vertex could not address, which is the width
    /// `VkDrawIndexedIndirectCommand::vertexOffset` and its D3D12 and WebGPU
    /// equivalents have — so a pool this rule allows is one §3.3's indirect
    /// draws can still address.
    pub base_vertex: u32,
    /// First index of this mesh in the index pool.
    pub base_index: u32,
    /// How many indices to draw.
    pub index_count: u32,
}

/// What a [`MeshPool`] refuses to do.
#[derive(Debug, thiserror::Error)]
pub enum MeshPoolError {
    /// No single free block is large enough.
    ///
    /// `largest_free` well below `total_free` is fragmentation — see the
    /// [module docs](self) on why the pool does not compact to fix it. A
    /// `total_free` below `requested` is a pool that is simply too small.
    #[error(
        "the {pool} pool cannot fit {requested} more: the largest free block holds \
         {largest_free} and {total_free} are free in total, out of a capacity of {capacity}"
    )]
    PoolExhausted {
        /// `"vertex"` or `"index"`.
        pool: &'static str,
        /// What the allocation asked for, in elements.
        requested: u32,
        /// The largest single run the free list could offer.
        largest_free: u32,
        /// Everything free, summed across blocks.
        total_free: u32,
        /// The pool's fixed capacity, in elements.
        capacity: u32,
    },
    /// A vertex capacity no draw could address.
    ///
    /// `draw_indexed` takes its base vertex **signed** — Vulkan, D3D12 and
    /// WebGPU all do — so a pool larger than that could hand out a base the
    /// draw cannot express. Refused at creation, which is what lets
    /// [`MeshRange::base_vertex`] be an ordinary count everywhere else.
    #[error("a vertex capacity of {capacity} is past what a draw's signed base vertex can address")]
    VertexCapacityTooLarge {
        /// What was asked for.
        capacity: u32,
    },
    /// A mesh with no vertices or no indices.
    #[error("a mesh needs both vertices and indices; got {vertices} and {indices}")]
    EmptyMesh {
        /// Vertices offered.
        vertices: u32,
        /// Indices offered.
        indices: u32,
    },
    /// The vertex bytes are not a whole number of vertices.
    #[error(
        "{bytes} bytes of vertex data is not a whole number of {VERTEX_STRIDE}-byte vertices; \
         the pool holds one vertex format and this is not it"
    )]
    VertexStrideMismatch {
        /// What the caller offered.
        bytes: usize,
    },
    /// A mesh has no range to draw: unknown handle, freed handle, or an upload
    /// that has not completed.
    #[error(
        "mesh {handle:?} is not resident: it was never uploaded to this pool, it has been freed, \
         or its upload has not been flushed"
    )]
    NotResident {
        /// The handle that resolved to nothing.
        handle: MeshHandle,
    },
    /// The transfers did not complete within [`UPLOAD_TIMEOUT`].
    #[error("the mesh pool's uploads did not complete within {}s", timeout.as_secs())]
    UploadTimedOut {
        /// How long the wait was given.
        timeout: Duration,
    },
    /// Anything the seam refused.
    #[error(transparent)]
    Hal(#[from] HalError),
}

/// Flattens a pool error into the seam's, for callers whose constructors are
/// already `Result<_, HalError>`.
///
/// The seam has no vocabulary for "this pool is fragmented", so everything that
/// is not already a [`HalError`] arrives as
/// [`Backend`](HalError::Backend) — which renders verbatim, so the message
/// naming the numbers survives. Callers that want to *match* on exhaustion take
/// [`MeshPoolError`] instead.
impl From<MeshPoolError> for HalError {
    fn from(error: MeshPoolError) -> Self {
        match error {
            MeshPoolError::Hal(hal) => hal,
            other => Self::Backend(other.to_string()),
        }
    }
}

/// How large a [`MeshPool`]'s two buffers are, and what to call them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshPoolDesc<'a> {
    /// Debug name; the two buffers and the semaphore are named after it.
    pub label: Option<&'a str>,
    /// Vertices the pool can hold. Fixed — see the [module docs](self).
    pub vertex_capacity: u32,
    /// Indices the pool can hold.
    pub index_capacity: u32,
}

/// One suballocated mesh, and the timeline value its bytes arrive under.
#[derive(Clone, Copy, Debug)]
struct Resident {
    range: MeshRange,
    /// Kept so [`MeshPool::free`] can return the vertex range; the range itself
    /// only carries the *index* count, because that is all a draw needs.
    vertex_count: u32,
    /// The timeline value the upload's submit signals. The mesh is resident
    /// once [`MeshPool::completed`] has reached it.
    ready_at: u64,
}

/// A staging buffer and command buffer that cannot be released until the copy
/// using them has run.
#[derive(Clone, Copy, Debug)]
struct InFlight {
    staging: BufferHandle,
    commands: CommandBufferHandle,
}

/// One vertex pool and one index pool, with a free-list suballocator over each.
///
/// See the [module docs](self) for the fragmentation, growth and residency
/// policies, all three of which are decisions rather than accidents.
#[derive(Debug)]
pub struct MeshPool {
    vertices: BufferHandle,
    indices: BufferHandle,
    vertex_alloc: FreeList,
    index_alloc: FreeList,

    meshes: Pool<Resident>,

    /// `None` on a device without [`Features::TIMELINE_SEMAPHORE`], where
    /// [`MeshPool::flush`] falls back to `wait_idle` — the same fallback
    /// `crcbl::GpuContext::retire_to` takes.
    timeline: Option<SemaphoreHandle>,
    /// Highest value any upload has been submitted with.
    submitted: u64,
    /// Highest value observed to have passed. Meshes at or below it are
    /// resident.
    completed: u64,
    in_flight: Vec<InFlight>,
}

impl MeshPool {
    /// Creates both pools, empty.
    ///
    /// # Errors
    ///
    /// [`MeshPoolError::Hal`] from any seam call. A failure part-way through
    /// releases whatever was already created.
    pub fn new(device: &dyn Device, desc: &MeshPoolDesc<'_>) -> Result<Self, MeshPoolError> {
        if i32::try_from(desc.vertex_capacity).is_err() {
            return Err(MeshPoolError::VertexCapacityTooLarge {
                capacity: desc.vertex_capacity,
            });
        }
        let stem = desc.label.unwrap_or("mesh pool");
        let vertices = device.create_buffer(&BufferDesc {
            label: Some(&format!("{stem} vertices")),
            size: u64::from(desc.vertex_capacity) * VERTEX_STRIDE as u64,
            // STORAGE, not a vertex buffer: §3.1's vertex pulling means the
            // vertex stage indexes this with `SV_VertexID` and there is no
            // vertex-input state anywhere in the engine.
            usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        })?;
        let indices = match device.create_buffer(&BufferDesc {
            label: Some(&format!("{stem} indices")),
            size: u64::from(desc.index_capacity) * INDEX_STRIDE,
            usage: BufferUsage::INDEX | BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::DeviceLocal,
        }) {
            Ok(indices) => indices,
            Err(error) => {
                device.destroy_buffer(vertices);
                return Err(error.into());
            }
        };

        // The capability check rather than the error, so a device that simply
        // has no timeline semaphores takes the documented fallback and a device
        // that has them but failed to make one still reports why.
        let timeline = if device
            .caps()
            .features
            .contains(Features::TIMELINE_SEMAPHORE)
        {
            match device.create_semaphore(&SemaphoreDesc {
                label: Some(&format!("{stem} uploads")),
                kind: SemaphoreKind::Timeline { initial_value: 0 },
            }) {
                Ok(semaphore) => Some(semaphore),
                Err(error) => {
                    device.destroy_buffer(indices);
                    device.destroy_buffer(vertices);
                    return Err(error.into());
                }
            }
        } else {
            log::debug!("mesh pool: no timeline semaphores; draining uploads with wait_idle");
            None
        };

        Ok(Self {
            vertices,
            indices,
            vertex_alloc: FreeList::new(desc.vertex_capacity),
            index_alloc: FreeList::new(desc.index_capacity),
            meshes: Pool::new(),
            timeline,
            submitted: 0,
            completed: 0,
            in_flight: Vec::new(),
        })
    }

    /// The vertex pool. Bound as a storage buffer; the vertex stage indexes it.
    #[must_use]
    pub const fn vertex_buffer(&self) -> BufferHandle {
        self.vertices
    }

    /// The index pool. Bound once, at offset zero, for every mesh in it.
    #[must_use]
    pub const fn index_buffer(&self) -> BufferHandle {
        self.indices
    }

    /// Suballocates room for a mesh and records a staging copy into it.
    ///
    /// `vertices` is `VERTEX_STRIDE`-strided vertex data; `indices` are
    /// **mesh-relative** — the base vertex reaches the GPU beside them rather
    /// than inside them, so a mesh's bytes never depend on where it landed. The
    /// [module docs](self) say by which route. The returned mesh is not resident
    /// until [`MeshPool::flush`] has run, and [`MeshPool::mesh`] says so.
    ///
    /// # Errors
    ///
    /// [`MeshPoolError::EmptyMesh`] or [`MeshPoolError::VertexStrideMismatch`]
    /// for data this pool cannot describe, [`MeshPoolError::PoolExhausted`] when
    /// either pool has no block large enough, and [`MeshPoolError::Hal`] from any
    /// seam call. Nothing is left allocated on any failing path: a vertex range
    /// taken before the index pool refused is given back.
    pub fn upload(
        &mut self,
        device: &dyn Device,
        queue: QueueHandle,
        label: &str,
        vertices: &[u8],
        indices: &[u32],
    ) -> Result<MeshHandle, MeshPoolError> {
        if !vertices.len().is_multiple_of(VERTEX_STRIDE) {
            return Err(MeshPoolError::VertexStrideMismatch {
                bytes: vertices.len(),
            });
        }
        let vertex_count = u32::try_from(vertices.len() / VERTEX_STRIDE).unwrap_or(u32::MAX);
        let index_count = u32::try_from(indices.len()).unwrap_or(u32::MAX);
        if vertex_count == 0 || index_count == 0 {
            return Err(MeshPoolError::EmptyMesh {
                vertices: vertex_count,
                indices: index_count,
            });
        }

        let base_vertex = self
            .vertex_alloc
            .alloc(vertex_count)
            .ok_or_else(|| self.vertex_alloc.exhausted("vertex", vertex_count))?;
        let base_index = match self.index_alloc.alloc(index_count) {
            Some(base) => base,
            None => {
                // The vertex range is given back before the error leaves: a
                // failed upload that kept half its allocation would leak pool
                // space that nothing holds a handle to.
                self.vertex_alloc.free(base_vertex, vertex_count);
                return Err(self.index_alloc.exhausted("index", index_count));
            }
        };

        let range = MeshRange {
            base_vertex,
            base_index,
            index_count,
        };
        match self.record_upload(device, queue, label, vertices, indices, range) {
            Ok(ready_at) => Ok(self
                .meshes
                .insert(Resident {
                    range,
                    vertex_count,
                    ready_at,
                })
                .cast()),
            Err(error) => {
                self.vertex_alloc.free(base_vertex, vertex_count);
                self.index_alloc.free(base_index, index_count);
                Err(error)
            }
        }
    }

    /// Stages both halves, records the copies and submits them, returning the
    /// timeline value the submit signals.
    ///
    /// The staging buffer and the command buffer are handed to
    /// [`MeshPool::in_flight`](Self::in_flight) rather than destroyed: the copy
    /// has been *submitted*, not run.
    fn record_upload(
        &mut self,
        device: &dyn Device,
        queue: QueueHandle,
        label: &str,
        vertices: &[u8],
        indices: &[u32],
        range: MeshRange,
    ) -> Result<u64, MeshPoolError> {
        // One staging buffer for both halves, vertices first: two copies out of
        // one allocation rather than two allocations, since they are submitted
        // together anyway.
        let index_bytes: &[u8] = bytemuck::cast_slice(indices);
        let staging = device.create_buffer(&BufferDesc {
            label: Some(&format!("{label} staging")),
            size: (vertices.len() + index_bytes.len()) as u64,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })?;
        let submitted = match self.write_and_submit(
            device,
            queue,
            label,
            Staged {
                staging,
                vertex_bytes: vertices,
                index_bytes,
                range,
            },
        ) {
            Ok(submitted) => submitted,
            Err(error) => {
                device.destroy_buffer(staging);
                return Err(error);
            }
        };
        Ok(submitted)
    }

    /// The half of [`MeshPool::record_upload`] the staging buffer is live
    /// across, so the caller can release it on every failing path.
    fn write_and_submit(
        &mut self,
        device: &dyn Device,
        queue: QueueHandle,
        label: &str,
        staged: Staged<'_>,
    ) -> Result<u64, MeshPoolError> {
        let vertex_bytes = staged.vertex_bytes.len() as u64;
        device.write_buffer(staged.staging, 0, staged.vertex_bytes)?;
        device.write_buffer(staged.staging, vertex_bytes, staged.index_bytes)?;

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some(&format!("{label} upload")),
            queue,
        });
        // Back to TransferDst first. The pools are long-lived and already being
        // read by the time a second mesh arrives, so unlike a freshly created
        // buffer they have a state to come *from* — leaving this out is the
        // hazard that only appears once the pool has more than one resident.
        encoder.pipeline_barrier(&Barriers {
            buffers: &[
                BufferBarrier::new(
                    self.vertices,
                    ResourceState::ShaderRead,
                    ResourceState::TransferDst,
                ),
                BufferBarrier::new(
                    self.indices,
                    ResourceState::IndexBuffer,
                    ResourceState::TransferDst,
                ),
            ],
            ..Barriers::default()
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: staged.staging,
            src_offset: 0,
            dst: self.vertices,
            dst_offset: u64::from(staged.range.base_vertex) * VERTEX_STRIDE as u64,
            size: vertex_bytes,
        });
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: staged.staging,
            src_offset: vertex_bytes,
            dst: self.indices,
            dst_offset: u64::from(staged.range.base_index) * INDEX_STRIDE,
            size: staged.index_bytes.len() as u64,
        });
        encoder.pipeline_barrier(&Barriers {
            buffers: &[
                BufferBarrier::new(
                    self.vertices,
                    ResourceState::TransferDst,
                    ResourceState::ShaderRead,
                ),
                BufferBarrier::new(
                    self.indices,
                    ResourceState::TransferDst,
                    ResourceState::IndexBuffer,
                ),
            ],
            ..Barriers::default()
        });
        let commands = encoder.finish()?;

        let value = self.submitted + 1;
        let signals = self
            .timeline
            .map(|semaphore| SemaphoreSignal { semaphore, value });
        let submit = SubmitInfo {
            command_buffers: &[commands],
            waits: &[],
            signals: signals.as_slice(),
        };
        if let Err(error) = device.submit(queue, &submit) {
            device.destroy_command_buffer(commands);
            return Err(error.into());
        }
        self.submitted = value;
        self.in_flight.push(InFlight {
            staging: staged.staging,
            commands,
        });
        Ok(value)
    }

    /// Waits for every submitted upload, making its meshes resident, and
    /// releases the staging buffers they used.
    ///
    /// # Errors
    ///
    /// [`MeshPoolError::UploadTimedOut`] if the transfers have not completed
    /// within [`UPLOAD_TIMEOUT`], in which case nothing becomes resident and
    /// nothing is released — the staging buffers may still be being read.
    /// [`MeshPoolError::Hal`] if the wait itself failed.
    pub fn flush(&mut self, device: &dyn Device) -> Result<(), MeshPoolError> {
        if self.completed >= self.submitted {
            return Ok(());
        }
        match self.timeline {
            Some(semaphore) => {
                let wait = SemaphoreWait {
                    semaphore,
                    value: self.submitted,
                };
                let timeout = u64::try_from(UPLOAD_TIMEOUT.as_nanos()).unwrap_or(u64::MAX);
                if !device.wait_semaphores(&[wait], timeout)? {
                    return Err(MeshPoolError::UploadTimedOut {
                        timeout: UPLOAD_TIMEOUT,
                    });
                }
            }
            // The device-wide hammer, which is what a device with no timeline
            // has. Correct, and coarser than it needs to be.
            None => device.wait_idle()?,
        }
        self.completed = self.submitted;
        for entry in self.in_flight.drain(..) {
            device.destroy_command_buffer(entry.commands);
            device.destroy_buffer(entry.staging);
        }
        Ok(())
    }

    /// Where `handle`'s mesh is, or `None` if there is nothing safe to draw.
    ///
    /// `None` covers both halves of that: a handle this pool never issued or has
    /// since freed, and a mesh whose upload has not completed. A caller cannot
    /// obtain a range for a mesh the GPU may not have received yet, which is
    /// §3.1's "renderer consumes meshes only after the timeline value passes"
    /// expressed as a type rather than as a rule to remember.
    #[must_use]
    pub fn mesh(&self, handle: MeshHandle) -> Option<MeshRange> {
        self.meshes
            .get(handle.cast())
            .filter(|resident| resident.ready_at <= self.completed)
            .map(|resident| resident.range)
    }

    /// Returns a mesh's space to both free lists.
    ///
    /// The space is reusable immediately, which is only true because uploads are
    /// drained by [`MeshPool::flush`] before anything draws — freeing a mesh a
    /// submitted-but-unfinished copy still writes into would be a hazard, and
    /// P9's streaming path is where that stops being free.
    ///
    /// Returns whether the handle named a live mesh; a second free of the same
    /// handle answers `false` rather than corrupting the free list, because the
    /// handle's generation has moved on.
    pub fn free(&mut self, handle: MeshHandle) -> bool {
        let Some(resident) = self.meshes.remove(handle.cast()) else {
            return false;
        };
        self.vertex_alloc
            .free(resident.range.base_vertex, resident.vertex_count);
        self.index_alloc
            .free(resident.range.base_index, resident.range.index_count);
        true
    }

    /// Vertices free, and the largest single run of them: the pair that tells
    /// fragmentation from a full pool. Indices in
    /// [`MeshPool::index_space`](Self::index_space).
    #[must_use]
    pub fn vertex_space(&self) -> (u32, u32) {
        (
            self.vertex_alloc.total_free(),
            self.vertex_alloc.largest_free(),
        )
    }

    /// Indices free, and the largest single run of them.
    #[must_use]
    pub fn index_space(&self) -> (u32, u32) {
        (
            self.index_alloc.total_free(),
            self.index_alloc.largest_free(),
        )
    }

    /// Releases both pools and everything still staged. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        for entry in self.in_flight {
            device.destroy_command_buffer(entry.commands);
            device.destroy_buffer(entry.staging);
        }
        if let Some(semaphore) = self.timeline {
            device.destroy_semaphore(semaphore);
        }
        device.destroy_buffer(self.indices);
        device.destroy_buffer(self.vertices);
    }
}

/// What [`MeshPool::write_and_submit`] needs that is not the device or the
/// queue — one struct rather than six positional arguments, two of which are
/// byte slices that would otherwise be swappable at the call site.
struct Staged<'a> {
    staging: BufferHandle,
    vertex_bytes: &'a [u8],
    index_bytes: &'a [u8],
    range: MeshRange,
}

/// A first-fit free-list suballocator over `0..capacity` **elements**.
///
/// Elements rather than bytes: the vertex pool counts vertices and the index
/// pool counts indices, so both hand back a number a draw call takes directly.
/// One type used twice rather than two nearly-identical ones, because the two
/// pools would always need the same fix.
///
/// The invariant every method preserves: `free` is sorted by `start`, holds no
/// empty block, and holds no two blocks that touch. Coalescing on [`FreeList::free`]
/// is what maintains the last of those, and it is what makes an alloc/free/alloc
/// cycle reuse the space instead of shredding it.
#[derive(Clone, Debug)]
struct FreeList {
    capacity: u32,
    free: Vec<Range<u32>>,
}

impl FreeList {
    /// A pool with everything free.
    fn new(capacity: u32) -> Self {
        let mut free = Vec::new();
        // One block covering everything — and *no* block at all when there is
        // nothing to cover, because "sorted, non-empty, non-touching" is the
        // invariant every other method relies on.
        if capacity > 0 {
            free.push(0..capacity);
        }
        Self { capacity, free }
    }

    /// Takes `len` elements from the first block that can hold them, returning
    /// where they start.
    ///
    /// `None` when no *single* block is large enough, whatever the total —
    /// which is the [module docs](self)' fragmentation case, not an oversight.
    /// A zero-length request is `None` too: it has no address to hand back.
    fn alloc(&mut self, len: u32) -> Option<u32> {
        if len == 0 {
            return None;
        }
        let index = self
            .free
            .iter()
            .position(|block| block.end - block.start >= len)?;
        let start = self.free[index].start;
        self.free[index].start += len;
        if self.free[index].is_empty() {
            self.free.remove(index);
        }
        Some(start)
    }

    /// Gives `start..start + len` back, merging it with either neighbour it
    /// touches.
    fn free(&mut self, start: u32, len: u32) {
        if len == 0 {
            return;
        }
        let mut block = start..start + len;
        let at = self.free.partition_point(|other| other.start < start);
        debug_assert!(
            at == 0 || self.free[at - 1].end <= block.start,
            "freeing {block:?} overlaps the free block before it"
        );
        debug_assert!(
            self.free.get(at).is_none_or(|next| block.end <= next.start),
            "freeing {block:?} overlaps the free block after it"
        );
        // Absorb the following block first, so the merge into the preceding one
        // below carries it too and three neighbours collapse into one.
        if self
            .free
            .get(at)
            .is_some_and(|next| next.start == block.end)
        {
            block.end = self.free.remove(at).end;
        }
        if at > 0 && self.free[at - 1].end == block.start {
            self.free[at - 1].end = block.end;
        } else {
            self.free.insert(at, block);
        }
    }

    /// Elements free across every block.
    fn total_free(&self) -> u32 {
        self.free.iter().map(|block| block.end - block.start).sum()
    }

    /// The largest single run, which is what an allocation can actually get.
    fn largest_free(&self) -> u32 {
        self.free
            .iter()
            .map(|block| block.end - block.start)
            .max()
            .unwrap_or(0)
    }

    /// The error for a request this list could not satisfy, with both numbers
    /// that distinguish fragmentation from a full pool.
    fn exhausted(&self, pool: &'static str, requested: u32) -> MeshPoolError {
        MeshPoolError::PoolExhausted {
            pool,
            requested,
            largest_free: self.largest_free(),
            total_free: self.total_free(),
            capacity: self.capacity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::{Command, Event, NullInstance, Recorder};
    use crcbl_hal::{DeviceDesc, Instance, QueueKind};
    use crcbl_shaders::mesh;

    fn open() -> (Recorder, Box<dyn Device>, QueueHandle) {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        (recorder, device, queue)
    }

    fn pool(device: &dyn Device, vertices: u32, indices: u32) -> MeshPool {
        MeshPool::new(
            device,
            &MeshPoolDesc {
                label: Some("test"),
                vertex_capacity: vertices,
                index_capacity: indices,
            },
        )
        .expect("the null backend accepts every descriptor")
    }

    /// `n` vertices' worth of bytes, distinct per vertex so a copy landing at
    /// the wrong offset could not compare equal by accident.
    fn vertex_bytes(n: u32) -> Vec<u8> {
        (0..n as usize * VERTEX_STRIDE)
            .map(|byte| (byte % 251) as u8)
            .collect()
    }

    // --- the allocator, which is pure logic ---

    /// Every allocation is inside the pool and disjoint from every other one.
    /// A first-fit list that forgot to advance a block's start would hand the
    /// same address out twice, and every mesh after the first would be drawn
    /// with another's vertices.
    #[test]
    fn allocations_never_overlap() {
        let mut list = FreeList::new(64);
        let mut taken: Vec<Range<u32>> = Vec::new();
        for len in [8u32, 1, 16, 3, 8, 20] {
            let start = list.alloc(len).expect("64 elements is enough for these");
            let block = start..start + len;
            assert!(block.end <= 64, "{block:?} runs past the pool");
            for other in &taken {
                assert!(
                    block.end <= other.start || other.end <= block.start,
                    "{block:?} overlaps {other:?}"
                );
            }
            taken.push(block);
        }
        let handed_out: u32 = taken.iter().map(|block| block.end - block.start).sum();
        assert_eq!(
            list.total_free(),
            64 - handed_out,
            "what is free and what is handed out must add up to the capacity"
        );
    }

    /// Free, and the space comes back — at the same address, because the list
    /// is first-fit and the hole is the first thing that fits.
    #[test]
    fn a_freed_block_is_reused() {
        let mut list = FreeList::new(32);
        let first = list.alloc(8).expect("empty pool");
        let middle = list.alloc(8).expect("still room");
        let last = list.alloc(8).expect("still room");
        assert_eq!((first, middle, last), (0, 8, 16));

        list.free(middle, 8);
        assert_eq!(list.total_free(), 16, "the hole plus the tail");
        assert_eq!(
            list.alloc(8),
            Some(middle),
            "the hole is reused, not the tail"
        );
        assert_eq!(list.total_free(), 8);
    }

    /// The case a naive free list gets wrong: enough space in total, no single
    /// block large enough, and **no compaction to fix it** — see the module
    /// docs. The error names both numbers so the two can be told apart.
    #[test]
    fn a_request_larger_than_every_block_fails_even_with_the_space_free() {
        let mut list = FreeList::new(10);
        let first = list.alloc(4).expect("empty");
        let fence = list.alloc(2).expect("room");
        let third = list.alloc(4).expect("room");
        assert_eq!((first, fence, third), (0, 4, 6));

        // Two holes of four, kept apart by a live allocation of two.
        list.free(first, 4);
        list.free(third, 4);
        assert_eq!(list.total_free(), 8);
        assert_eq!(list.largest_free(), 4);

        assert_eq!(list.alloc(5), None, "8 free, and not 5 of them in a row");
        let error = list.exhausted("vertex", 5);
        let message = error.to_string();
        assert!(
            message.contains("largest free block holds 4") && message.contains("8 are free"),
            "the error must name both numbers, got: {message}"
        );
        // And what does fit is still handed out, so the failure above is the
        // block size and not the list having given up.
        assert_eq!(list.alloc(4), Some(0));
    }

    /// The other half of the same case: freeing the allocation between two
    /// holes merges all three, and the request that just failed now fits. A
    /// list that appended freed blocks without coalescing would still report
    /// three blocks of four, two and four, and still refuse.
    #[test]
    fn freeing_between_two_holes_coalesces_all_three() {
        let mut list = FreeList::new(10);
        let first = list.alloc(4).expect("empty");
        let fence = list.alloc(2).expect("room");
        let third = list.alloc(4).expect("room");
        list.free(first, 4);
        list.free(third, 4);
        assert_eq!(list.largest_free(), 4);

        list.free(fence, 2);
        assert_eq!(list.total_free(), 10);
        assert_eq!(
            list.largest_free(),
            10,
            "three touching holes are one block, not three"
        );
        assert_eq!(list.alloc(10), Some(0), "and the whole pool is allocatable");
    }

    /// Coalescing works from either side, not just when the freed block happens
    /// to sit after its neighbour.
    #[test]
    fn coalescing_merges_the_neighbour_on_either_side() {
        for order in [[0u32, 4], [4, 0]] {
            let mut list = FreeList::new(8);
            assert_eq!(list.alloc(4), Some(0));
            assert_eq!(list.alloc(4), Some(4));
            for start in order {
                list.free(start, 4);
            }
            assert_eq!(
                list.largest_free(),
                8,
                "freeing in the order {order:?} left the list fragmented"
            );
        }
    }

    // --- the pool ---

    /// A vertex capacity a draw's signed base vertex could not address is
    /// refused at creation, before any device object exists.
    ///
    /// This is the guard the draw call's conversion is allowed to call
    /// unreachable on, so it has to be shown to actually reject something —
    /// a capacity check that never fires would make that `unreachable!` a
    /// claim about nothing.
    #[test]
    fn a_vertex_capacity_a_draw_could_not_address_is_refused() {
        let (recorder, device, _) = open();
        let error = MeshPool::new(
            device.as_ref(),
            &MeshPoolDesc {
                label: Some("far too large"),
                vertex_capacity: u32::MAX,
                index_capacity: 8,
            },
        )
        .expect_err("a draw cannot address that many vertices");
        assert!(
            matches!(error, MeshPoolError::VertexCapacityTooLarge { .. }),
            "got: {error:?}"
        );
        assert_eq!(
            recorder.total_live_objects(),
            0,
            "a refused pool creates nothing"
        );
        // And the largest capacity a draw *can* address is accepted, so the
        // check above is a boundary rather than a blanket refusal. The buffer is
        // never allocated by the null backend, which is what makes asking for
        // one this size a test rather than an out-of-memory.
        let pool = MeshPool::new(
            device.as_ref(),
            &MeshPoolDesc {
                label: Some("at the boundary"),
                vertex_capacity: i32::MAX as u32,
                index_capacity: 8,
            },
        )
        .expect("the largest addressable capacity is not too large");
        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A device without timeline semaphores drains with `wait_idle` and its
    /// meshes still become resident — the same fallback
    /// `crcbl::GpuContext::retire_to` takes, and a whole branch that no other
    /// test enters.
    #[test]
    fn a_device_without_timeline_semaphores_drains_with_wait_idle() {
        let recorder = Recorder::new();
        let instance = NullInstance::portable().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        // Not `for_adapter`: its required features include the timeline
        // semaphore this test exists to do without.
        let device = instance
            .create_device(&DeviceDesc {
                label: None,
                adapter: adapter.id,
                required_features: Features::COMPUTE,
                optional_features: Features::empty(),
                compatible_surface: None,
            })
            .expect("the portable null adapter opens");
        assert!(
            !device
                .caps()
                .features
                .contains(Features::TIMELINE_SEMAPHORE),
            "this test is only meaningful on a device that has none"
        );
        let queue = device.queue(QueueKind::Graphics).expect("always present");

        let mut pool = pool(device.as_ref(), 64, 64);
        let handle = pool
            .upload(device.as_ref(), queue, "cube", &vertex_bytes(4), &[0, 1, 2])
            .expect("it fits");
        let signals: Vec<_> = recorder
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::Submitted { signals, .. } => Some(signals),
                _ => None,
            })
            .collect();
        assert_eq!(
            signals,
            vec![Vec::new()],
            "there is no timeline to signal, so the submit names none"
        );
        assert_eq!(pool.mesh(handle), None, "and it is still not resident");

        pool.flush(device.as_ref()).expect("wait_idle always works");
        assert!(
            recorder
                .events()
                .iter()
                .any(|event| matches!(event, Event::WaitedIdle)),
            "the fallback is `wait_idle`; nothing else can drain this device"
        );
        assert!(
            pool.mesh(handle).is_some(),
            "and the mesh is resident afterwards, or this device could never draw"
        );

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A pool that cannot fit a mesh says so by name, and takes nothing: the
    /// vertex range the failed upload had already reserved is given back, or a
    /// pool would leak space every time an index pool filled first.
    #[test]
    fn an_upload_that_does_not_fit_fails_by_name_and_reserves_nothing() {
        let (recorder, device, queue) = open();
        // Room for the vertices, not for the indices.
        let mut pool = pool(device.as_ref(), 64, 8);
        let before = pool.vertex_space();

        let error = pool
            .upload(
                device.as_ref(),
                queue,
                "too many indices",
                &vertex_bytes(4),
                &[0; 16],
            )
            .expect_err("16 indices do not fit in 8");
        assert!(
            matches!(
                error,
                MeshPoolError::PoolExhausted {
                    pool: "index",
                    requested: 16,
                    capacity: 8,
                    ..
                }
            ),
            "got: {error:?}"
        );
        assert_eq!(
            pool.vertex_space(),
            before,
            "the vertex range reserved before the index pool refused must come back"
        );
        assert!(
            !recorder
                .events()
                .iter()
                .any(|event| matches!(event, Event::Submitted { .. })),
            "a refused upload submits nothing"
        );
        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **The bug this design exists to prevent.** The mesh has been uploaded —
    /// the copy is recorded and submitted — and it is still not something a
    /// caller can draw, because the timeline value it was uploaded under has
    /// not been observed to pass. Only [`MeshPool::flush`] makes it resident.
    ///
    /// What this cannot show is a *real* GPU still executing the copy: the null
    /// backend records commands rather than running them, and satisfies every
    /// wait immediately. So this pins the gate's existence, not its timing —
    /// the timing is `crcbl-vk`'s e2e suite, where the cube either has vertices
    /// or does not.
    #[test]
    fn a_mesh_is_not_resident_until_its_upload_is_flushed() {
        let (recorder, device, queue) = open();
        let mut pool = pool(device.as_ref(), 64, 64);
        let handle = pool
            .upload(
                device.as_ref(),
                queue,
                "cube",
                &vertex_bytes(4),
                &[0, 1, 2, 0, 2, 3],
            )
            .expect("it fits");

        assert!(
            recorder
                .events()
                .iter()
                .any(|event| matches!(event, Event::Submitted { .. })),
            "the copy really was submitted; residency is not just 'nothing happened yet'"
        );
        assert_eq!(
            pool.mesh(handle),
            None,
            "an unflushed mesh must not hand out a range"
        );

        pool.flush(device.as_ref()).expect("the null device drains");
        assert_eq!(
            pool.mesh(handle),
            Some(MeshRange {
                base_vertex: 0,
                base_index: 0,
                index_count: 6,
            }),
            "and once the timeline has passed, it must"
        );

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// The upload signals the pool's timeline, which is what residency is
    /// waiting on. A submit with no signal would make `flush` wait for a value
    /// nothing ever reaches.
    #[test]
    fn an_upload_signals_the_pools_timeline() {
        let (recorder, device, queue) = open();
        let mut pool = pool(device.as_ref(), 64, 64);
        pool.upload(device.as_ref(), queue, "cube", &vertex_bytes(4), &[0, 1, 2])
            .expect("it fits");

        let signals: Vec<_> = recorder
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::Submitted { signals, .. } => Some(signals),
                _ => None,
            })
            .collect();
        assert_eq!(signals.len(), 1, "one upload, one submit");
        assert_eq!(
            signals[0].len(),
            1,
            "the submit must signal something, or nothing can wait for it"
        );
        assert_eq!(
            signals[0][0].value, 1,
            "the first upload is timeline value 1"
        );
        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// Two meshes land at different bases, and the second's copies are offset
    /// by the first's size in **bytes** — the one place the element/byte
    /// conversion could go wrong without any test noticing.
    #[test]
    fn a_second_mesh_lands_after_the_first_in_both_pools() {
        let (recorder, device, queue) = open();
        let mut pool = pool(device.as_ref(), 64, 64);
        for (label, vertices, indices) in [
            ("first", 4u32, vec![0u32, 1, 2, 0, 2, 3]),
            ("second", 3, vec![0u32, 1, 2]),
        ] {
            pool.upload(
                device.as_ref(),
                queue,
                label,
                &vertex_bytes(vertices),
                &indices,
            )
            .expect("both fit");
        }
        pool.flush(device.as_ref()).expect("drains");

        let copies: Vec<BufferCopy> = recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::CopyBufferToBuffer(copy) => Some(copy),
                _ => None,
            })
            .collect();
        assert_eq!(copies.len(), 4, "two meshes, vertices and indices each");
        // First mesh: both halves at the start of their pool.
        assert_eq!(copies[0].dst_offset, 0);
        assert_eq!(copies[1].dst_offset, 0);
        // Second mesh: four vertices and six indices further in.
        assert_eq!(copies[2].dst_offset, 4 * VERTEX_STRIDE as u64);
        assert_eq!(copies[3].dst_offset, 6 * INDEX_STRIDE);
        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// Freeing a mesh returns its space and stops its handle resolving, so a
    /// stale handle cannot draw whatever moved in.
    #[test]
    fn freeing_a_mesh_returns_its_space_and_retires_its_handle() {
        let (recorder, device, queue) = open();
        let mut pool = pool(device.as_ref(), 64, 64);
        let handle = pool
            .upload(device.as_ref(), queue, "cube", &vertex_bytes(8), &[0; 12])
            .expect("it fits");
        pool.flush(device.as_ref()).expect("drains");
        assert_eq!(pool.vertex_space(), (64 - 8, 64 - 8));

        assert!(pool.free(handle), "the handle names a live mesh");
        assert_eq!(pool.vertex_space(), (64, 64), "and its space comes back");
        assert_eq!(pool.index_space(), (64, 64));
        assert_eq!(pool.mesh(handle), None, "the handle stops resolving");
        assert!(!pool.free(handle), "and a second free is refused");
        assert_eq!(
            pool.vertex_space(),
            (64, 64),
            "a refused free must not double-count the space"
        );

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A cube's worth of real geometry goes in and comes back out as the three
    /// integers a draw takes.
    #[test]
    fn the_cube_becomes_three_integers() {
        let (recorder, device, queue) = open();
        let mut pool = pool(device.as_ref(), 1024, 1024);
        let handle = pool
            .upload(
                device.as_ref(),
                queue,
                "cube",
                &mesh::cube_vertex_bytes(),
                &mesh::cube_indices(),
            )
            .expect("a cube is small");
        pool.flush(device.as_ref()).expect("drains");
        let range = pool.mesh(handle).expect("flushed");
        assert_eq!(range.base_vertex, 0);
        assert_eq!(range.base_index, 0);
        assert_eq!(range.index_count as usize, mesh::CUBE_INDEX_COUNT);
        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// Vertex data that is not a whole number of vertices is refused before
    /// anything is allocated: the pool holds one vertex format, and bytes that
    /// are not it would silently shift every following mesh.
    #[test]
    fn vertex_data_of_the_wrong_stride_is_refused() {
        let (recorder, device, queue) = open();
        let mut pool = pool(device.as_ref(), 64, 64);
        let error = pool
            .upload(
                device.as_ref(),
                queue,
                "ragged",
                &[0u8; VERTEX_STRIDE + 1],
                &[0, 1, 2],
            )
            .expect_err("49 bytes is not a whole number of vertices");
        assert!(
            matches!(error, MeshPoolError::VertexStrideMismatch { .. }),
            "got: {error:?}"
        );
        assert_eq!(pool.vertex_space(), (64, 64), "and nothing was reserved");

        let error = pool
            .upload(device.as_ref(), queue, "empty", &[], &[])
            .expect_err("a mesh needs both halves");
        assert!(
            matches!(error, MeshPoolError::EmptyMesh { .. }),
            "got: {error:?}"
        );

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// Everything created is destroyed, staging buffers included — and a pool
    /// destroyed with an upload still in flight releases that too.
    #[test]
    fn a_pool_leaks_nothing() {
        let (recorder, device, queue) = open();
        let before = recorder.total_live_objects();
        let mut pool = pool(device.as_ref(), 64, 64);
        pool.upload(
            device.as_ref(),
            queue,
            "flushed",
            &vertex_bytes(4),
            &[0, 1, 2],
        )
        .expect("fits");
        pool.flush(device.as_ref()).expect("drains");
        pool.upload(
            device.as_ref(),
            queue,
            "still staged",
            &vertex_bytes(4),
            &[0, 1, 2],
        )
        .expect("fits");
        assert!(recorder.total_live_objects() > before);

        pool.destroy(device.as_ref());
        assert_eq!(
            recorder.total_live_objects(),
            before,
            "every object created must be destroyed, staged or not"
        );
        recorder.assert_valid();
    }

    /// A pool error that has to travel as a [`HalError`] keeps its message
    /// rather than becoming a generic failure — which is the whole reason the
    /// exhaustion error names its numbers.
    #[test]
    fn a_pool_error_flattens_into_the_seams_without_losing_its_message() {
        let exhausted = MeshPoolError::PoolExhausted {
            pool: "vertex",
            requested: 5,
            largest_free: 4,
            total_free: 8,
            capacity: 10,
        };
        let message = exhausted.to_string();
        let flattened = HalError::from(exhausted);
        assert_eq!(flattened.to_string(), message);

        // And a seam error stays itself rather than being wrapped in a string.
        let hal = MeshPoolError::Hal(HalError::OutOfDeviceMemory);
        assert!(
            matches!(HalError::from(hal), HalError::OutOfDeviceMemory),
            "a HalError must not be stringified on the way through"
        );
    }
}
