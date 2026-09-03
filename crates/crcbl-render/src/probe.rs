//! The irradiance probe table: one storage-buffer row per probe, filled once
//! from the scene description.
//!
//! ```text
//!  SceneDesc::probes ──▶ ProbeTable::new (cleared) ──▶ fill(rows) ──▶ binding 23
//!                        └─▶ ProbeGrid::volume rides in mesh::FrameUniforms
//! ```
//!
//! `docs/plan/18-render-features.md`'s irradiance probes: a static grid in a
//! storage buffer, trilinearly interpolated by `mesh.slang` and added to the
//! flat ambient term. [`crcbl_shaders::probe::GpuProbe`] is the row and
//! [`crcbl_shaders::probe::ProbeVolume`] is the header that says where the rows
//! are — both live in the crate that owns the shader, because they are a
//! contract with it.
//!
//! # Device-local, so a compute pass can write it
//!
//! The rows are [`MemoryLocation::DeviceLocal`] and the buffer carries
//! [`BufferUsage::TRANSFER_DST`] beside [`BufferUsage::STORAGE`], which is what
//! `docs/plan/50-irradiance-probes.md`'s updater needs of it: the seam refuses a
//! *writable* storage binding of a host-visible buffer — D3D12's rule, argued at
//! [`MemoryLocation`] — so a table a dispatch may one day fill cannot be
//! host-visible whatever its binding says today. Nothing writes a probe on the
//! GPU yet; what this buys is that the memory no longer forbids it.
//!
//! The cost is that filling the table is a staging copy rather than a
//! [`Device::write_buffer`], which is valid only for
//! [`MemoryLocation::HostUpload`]. [`ProbeTable::upload`] is that copy — a
//! host-visible buffer, one `write_buffer` into it, and one submission that
//! barriers every frame's rows into `TransferDst` and back to `ShaderRead`.
//! `crate::probe_capture`'s load path is the same shape and the same reason: a
//! one-off upload at build with its own encoder.
//!
//! # A ring of frames, which is what device-local forces
//!
//! [`ForwardRenderer`]'s `SharedBindings` names one probe buffer in every frame
//! in flight's bind group, so a single buffer and a pass that wrote it would be
//! frame N+1's write landing in rows frame N's forward pass is still reading —
//! the read-after-write hazard across submissions that
//! [`crate::material_table`] states and this table used to be allowed to ignore.
//! It is not ignorable once the memory permits a write, so the table is
//! `frames` buffers and [`ProbeTable::buffer`] takes the frame.
//! [`crate::light_grid`]'s per-frame `grid[frame]` is the pattern.
//!
//! **Every frame's copy is the same authored bytes**, because nothing varies a
//! probe per frame yet. The ring is the structure the updater lands into, not a
//! difference anything can currently observe.
//!
//! What is *not* copied across from [`crate::material_table`] is the
//! generational [`Pool`](crcbl_core::Pool). A material row is handed out, edited
//! and freed one at a time through a handle; the probes arrive as one array and
//! are written once, so there is nothing to allocate and no handle for anything
//! to go stale. Adding a pool here would be an allocator with no caller.
//!
//! # The table is never empty, even when the scene has no probes
//!
//! A buffer of zero bytes is not a buffer — `crcbl_hal`'s null backend refuses
//! `size == 0` outright and Vulkan's own `VkBufferCreateInfo` requires a
//! non-zero size — so the table holds at least one row whatever the capacity.
//! That row is the zeroed one, which is exactly what the degenerate volume
//! addresses: `mesh.slang` clamps every fetch into the table, a scene with no
//! probes clamps to row 0, and a row of zeroes adds zero. So the empty case
//! needs no branch in the shader and no second binding here.
//!
//! [`ForwardRenderer`]: crate::forward::ForwardRenderer

use crcbl_hal::{
    Barriers, BufferBarrier, BufferCopy, BufferDesc, BufferHandle, BufferUsage, CommandEncoderDesc,
    Device, HalError, MemoryLocation, QueueHandle, ResourceState, SubmitInfo,
};
use crcbl_shaders::probe::{GpuProbe, PROBE_STRIDE};

/// How large a [`ProbeTable`] is, and what to call it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProbeTableDesc<'a> {
    /// Debug name; the buffers are named after it.
    pub(crate) label: Option<&'a str>,
    /// Probes the table can hold. Fixed, like every other pool the scene sizes.
    pub(crate) capacity: u32,
    /// Frames in flight, which is how many copies of the rows there are — see
    /// the [module docs](self) for why there is more than one.
    pub(crate) frames: usize,
}

/// One `GpuProbe` array per frame in flight, written once.
#[derive(Debug)]
pub(crate) struct ProbeTable {
    buffers: Vec<BufferHandle>,
    capacity: u32,
}

impl ProbeTable {
    /// Creates the table, **cleared**.
    ///
    /// Cleared for [`MaterialTable::new`](crate::material_table::MaterialTable::new)'s
    /// reason, and here it is the whole of the additive-zero property rather
    /// than a tidiness: row 0 is what a scene with no probes reads, and a
    /// driver's leftovers are neither zero nor a light anybody chose. The clear
    /// is a staging copy like [`ProbeTable::fill`] is, because the rows are
    /// device-local.
    ///
    /// Each frame's buffer holds `max(capacity, 1)` rows — see the
    /// [module docs](self) for why it can never be empty.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] for a ring of no frames, or [`HalError`]
    /// from any seam call. A failure part-way through releases the buffers it
    /// had already created.
    pub(crate) fn new(
        device: &dyn Device,
        queue: QueueHandle,
        desc: &ProbeTableDesc<'_>,
    ) -> Result<Self, HalError> {
        if desc.frames == 0 {
            return Err(HalError::InvalidDescriptor(
                "a probe table needs at least one frame in flight".to_string(),
            ));
        }
        let stem = desc.label.unwrap_or("probe table");
        let size = u64::from(desc.capacity.max(1)) * PROBE_STRIDE as u64;
        let mut buffers = Vec::with_capacity(desc.frames);
        for frame in 0..desc.frames {
            match device.create_buffer(&BufferDesc {
                label: Some(&format!("{stem} probes {frame}")),
                size,
                // **Device-local, and `TRANSFER_DST` so the staging copy has
                // somewhere to land.** See the module docs: the memory is what
                // decides whether a dispatch could ever write these rows, and
                // the binding stays read-only regardless.
                usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::DeviceLocal,
            }) {
                Ok(buffer) => buffers.push(buffer),
                Err(error) => {
                    for buffer in buffers {
                        device.destroy_buffer(buffer);
                    }
                    return Err(error);
                }
            }
        }
        let table = Self {
            buffers,
            capacity: desc.capacity,
        };
        let cleared = vec![0u8; usize::try_from(size).unwrap_or(usize::MAX)];
        // **`Undefined` as the source**, which is the one place in this module
        // it is right: the buffers were created a line ago, so there is no
        // earlier access for a barrier to order against and nothing in them
        // worth preserving.
        if let Err(error) = table.upload(
            device,
            queue,
            "probe table clear",
            ResourceState::Undefined,
            &cleared,
        ) {
            table.destroy(device);
            return Err(error);
        }
        Ok(table)
    }

    /// This `frame`'s rows. Bound as a read-only storage buffer; the fragment
    /// stage indexes it with a grid cell.
    ///
    /// # Panics
    ///
    /// If `frame` is past the ring the descriptor asked for.
    pub(crate) fn buffer(&self, frame: usize) -> BufferHandle {
        self.buffers[frame]
    }

    /// Writes `probes` into the first rows of **every** frame's copy, in order.
    ///
    /// Nothing clears the rows past them: they are the zeroes
    /// [`ProbeTable::new`] wrote, and a scene fills this once.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] if there are more probes than the table
    /// holds — which
    /// [`ForwardRenderer::with_scene`](crate::forward::ForwardRenderer::with_scene)
    /// has already refused by name, so reaching it means the two counts were
    /// derived from different descriptions. [`HalError`] otherwise, if the rows
    /// could not be staged, copied or submitted.
    pub(crate) fn fill(
        &self,
        device: &dyn Device,
        queue: QueueHandle,
        probes: &[GpuProbe],
    ) -> Result<(), HalError> {
        if probes.len() > self.capacity as usize {
            return Err(HalError::InvalidDescriptor(format!(
                "the probe table holds {} and the description needs {}",
                self.capacity,
                probes.len()
            )));
        }
        // One staged block rather than one per row: the whole array is
        // contiguous and arrives at once, unlike a material's, so there is
        // nothing for a per-row loop to buy.
        let mut bytes = Vec::with_capacity(probes.len() * PROBE_STRIDE);
        for probe in probes {
            bytes.extend_from_slice(&probe.to_bytes());
        }
        if bytes.is_empty() {
            // Nothing to say to the device, and a zero-length buffer is not
            // something any backend accepts. The rows are already the zeroes
            // `new` wrote.
            return Ok(());
        }
        // `ShaderRead` as the source, unlike the clear's: `new` left every
        // buffer there, and a barrier naming `Undefined` here would carry no
        // source scope and order this copy against the clear not at all.
        self.upload(
            device,
            queue,
            "probe table fill",
            ResourceState::ShaderRead,
            &bytes,
        )
    }

    /// Copies `bytes` into the head of every frame's rows, through one staging
    /// buffer and one submission.
    ///
    /// `from` is the state the rows are already in — see the two call sites for
    /// why they disagree about it.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. The staging buffer is released on every
    /// path, failing or not.
    fn upload(
        &self,
        device: &dyn Device,
        queue: QueueHandle,
        label: &str,
        from: ResourceState,
        bytes: &[u8],
    ) -> Result<(), HalError> {
        let staging = device.create_buffer(&BufferDesc {
            label: Some(label),
            size: bytes.len() as u64,
            usage: BufferUsage::TRANSFER_SRC,
            memory: MemoryLocation::HostUpload,
        })?;
        let result = self.copy_from(device, queue, label, staging, from, bytes);
        device.destroy_buffer(staging);
        result
    }

    /// The half of [`ProbeTable::upload`] the staging buffer is live across, so
    /// the caller can release it on every failing path.
    fn copy_from(
        &self,
        device: &dyn Device,
        queue: QueueHandle,
        label: &str,
        staging: BufferHandle,
        from: ResourceState,
        bytes: &[u8],
    ) -> Result<(), HalError> {
        device.write_buffer(staging, 0, bytes)?;

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some(label),
            queue,
        });
        let into: Vec<BufferBarrier> = self
            .buffers
            .iter()
            .map(|&buffer| BufferBarrier::new(buffer, from, ResourceState::TransferDst))
            .collect();
        encoder.pipeline_barrier(&Barriers {
            buffers: &into,
            ..Barriers::default()
        });
        for &buffer in &self.buffers {
            encoder.copy_buffer_to_buffer(&BufferCopy {
                src: staging,
                src_offset: 0,
                dst: buffer,
                dst_offset: 0,
                size: bytes.len() as u64,
            });
        }
        let out: Vec<BufferBarrier> = self
            .buffers
            .iter()
            .map(|&buffer| {
                BufferBarrier::new(
                    buffer,
                    ResourceState::TransferDst,
                    ResourceState::ShaderRead,
                )
            })
            .collect();
        encoder.pipeline_barrier(&Barriers {
            buffers: &out,
            ..Barriers::default()
        });

        let commands = encoder.finish()?;
        // Waited on rather than pipelined: this runs at build, the staging
        // buffer above dies with this call, and the graph's import of these rows
        // claims `ShaderRead` on the first frame that draws them.
        let submitted = device
            .submit(queue, &SubmitInfo::new(&[commands]))
            .and_then(|()| device.wait_idle());
        device.destroy_command_buffer(commands);
        submitted
    }

    /// Releases every frame's buffer. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        for buffer in self.buffers {
            device.destroy_buffer(buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::{Command, Event, NullInstance, Recorder};
    use crcbl_hal::{DeviceDesc, Instance, QueueKind};

    /// Frames the tests build their rings over. Three rather than
    /// [`crate::forward::FRAMES_IN_FLIGHT`]'s two, so a loop that filled only
    /// the first and the last is a different count from a loop that filled all
    /// of them.
    const FRAMES: usize = 3;

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

    fn table(device: &dyn Device, queue: QueueHandle, capacity: u32) -> ProbeTable {
        ProbeTable::new(
            device,
            queue,
            &ProbeTableDesc {
                label: Some("test"),
                capacity,
                frames: FRAMES,
            },
        )
        .expect("the null backend accepts every descriptor")
    }

    /// A probe distinguishable from every other `n`, with no two coefficients
    /// equal — so a row read at the wrong offset, or a band written in the
    /// wrong order, is a different value rather than the same one.
    fn probe(n: u32) -> GpuProbe {
        let base = n as f32;
        GpuProbe {
            sh_r: [base, base + 0.125, base + 0.25, base + 0.375],
            sh_g: [base + 0.5, base + 0.625, base + 0.75, base + 0.875],
            sh_b: [base + 1.0, base + 1.125, base + 1.25, base + 1.375],
        }
    }

    fn writes(recorder: &Recorder) -> Vec<(u64, usize)> {
        recorder
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::BufferWritten { offset, len, .. } => Some((offset, len)),
                _ => None,
            })
            .collect()
    }

    fn copies(recorder: &Recorder) -> Vec<BufferCopy> {
        recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::CopyBufferToBuffer(copy) => Some(copy),
                _ => None,
            })
            .collect()
    }

    /// Every buffer transition recorded, in order, as `(from, to)` pairs.
    fn transitions(recorder: &Recorder) -> Vec<Vec<(ResourceState, ResourceState)>> {
        recorder
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                Command::Barrier { buffers, .. } if !buffers.is_empty() => Some(
                    buffers
                        .into_iter()
                        .map(|barrier| (barrier.from, barrier.to))
                        .collect(),
                ),
                _ => None,
            })
            .collect()
    }

    /// **Every frame's rows get their own copy of the same staged block**,
    /// which is the whole of what the ring is until something writes a probe
    /// on the GPU.
    ///
    /// # This once read the bytes back, and no longer can
    ///
    /// It used to decode each row out of [`Recorder::buffer_bytes`] and assert
    /// that row `n` held probe `n` — the table was host-visible then, so the
    /// host's `write_buffer` *was* the contents. The rows are device-local now
    /// and the null backend executes no copies, so the buffer this asks about
    /// holds the nothing it was created with; the staging buffer that did carry
    /// the bytes is released before `fill` returns.
    ///
    /// What moved is only where that check lives, not whether it exists:
    /// `crates/crcbl/tests/render_e2e.rs`'s `the_probes_scene_lights_its_room_and_matches_its_golden`
    /// and the mirror comparison beside it run the shader against real rows, and
    /// a block landing at the wrong offset is a different picture there. What is
    /// checkable *here* is the schedule — that the staged block is the rows'
    /// length, that there is a copy per frame, and that each lands at the head
    /// of its own buffer.
    #[test]
    fn every_frames_rows_get_the_same_staged_block() {
        let (recorder, device, queue) = open();
        let table = table(device.as_ref(), queue, 8);
        recorder.clear();

        let probes: Vec<GpuProbe> = (0..4).map(probe).collect();
        table
            .fill(device.as_ref(), queue, &probes)
            .expect("room for four");

        let staged = probes.len() * PROBE_STRIDE;
        assert_eq!(
            writes(&recorder),
            [(0, staged)],
            "the rows reach the device as one staged block, not one write per row"
        );
        let copies = copies(&recorder);
        assert_eq!(
            copies.len(),
            FRAMES,
            "a copy per frame in flight, so no frame is left holding the clear"
        );
        for (frame, copy) in copies.iter().enumerate() {
            assert_eq!(
                copy.dst,
                table.buffer(frame),
                "frame {frame}'s copy names another frame's buffer"
            );
            assert_eq!(
                copy.dst_offset, 0,
                "the rows start at the head of the table"
            );
            assert_eq!(copy.size, staged as u64);
        }
        assert!(
            copies.iter().all(|copy| copy.src == copies[0].src),
            "one staging buffer for the whole ring, not one per frame"
        );
        // And the ring is a ring: three buffers, not one handle three times.
        for frame in 1..FRAMES {
            assert_ne!(
                table.buffer(frame),
                table.buffer(frame - 1),
                "frame {frame} shares its rows with the frame before it"
            );
        }

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **Both copies are bracketed by barriers over every frame's buffer**, and
    /// they name different source states.
    ///
    /// The clear's source is [`ResourceState::Undefined`] because its buffers
    /// were created a moment before and have no earlier access; the fill's is
    /// [`ResourceState::ShaderRead`] because the clear left them there, and a
    /// barrier naming `Undefined` at that point would carry no source scope and
    /// order the fill against the clear not at all.
    #[test]
    fn a_copy_is_bracketed_by_barriers_over_the_whole_ring() {
        let (recorder, device, queue) = open();
        let table = table(device.as_ref(), queue, 4);

        let clear = transitions(&recorder);
        assert_eq!(clear.len(), 2, "the clear brackets its copies");
        assert_eq!(
            clear[0],
            vec![(ResourceState::Undefined, ResourceState::TransferDst); FRAMES]
        );
        assert_eq!(
            clear[1],
            vec![(ResourceState::TransferDst, ResourceState::ShaderRead); FRAMES]
        );

        recorder.clear();
        table
            .fill(device.as_ref(), queue, &[probe(0)])
            .expect("room for one");
        let fill = transitions(&recorder);
        assert_eq!(fill.len(), 2, "the fill brackets its copies too");
        assert_eq!(
            fill[0],
            vec![(ResourceState::ShaderRead, ResourceState::TransferDst); FRAMES],
            "the fill has to order itself against the clear, which left the rows readable"
        );
        assert_eq!(
            fill[1],
            vec![(ResourceState::TransferDst, ResourceState::ShaderRead); FRAMES],
            "and leave them where the graph's import claims they are"
        );

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **The table is cleared when it is created, and holds a row even for a
    /// scene with no probes** — the two halves of what makes an empty grid add
    /// exactly zero on the device.
    #[test]
    fn an_empty_table_is_still_one_cleared_row() {
        let (recorder, device, queue) = open();
        let table = table(device.as_ref(), queue, 0);
        assert_eq!(
            writes(&recorder),
            [(0, PROBE_STRIDE)],
            "a capacity of zero must still create and clear one row, because a \
             buffer of no bytes is not a buffer"
        );
        let cleared = copies(&recorder);
        assert_eq!(cleared.len(), FRAMES, "every frame's row is cleared");
        assert!(
            cleared
                .iter()
                .all(|copy| copy.size == PROBE_STRIDE as u64 && copy.dst_offset == 0),
            "got: {cleared:?}"
        );

        // And filling it with nothing reaches the device not at all, rather
        // than as a zero-length staging buffer nothing accepts.
        recorder.clear();
        table
            .fill(device.as_ref(), queue, &[])
            .expect("nothing to write");
        assert_eq!(writes(&recorder), []);
        assert_eq!(copies(&recorder), []);

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// More probes than the table holds is refused by name and reaches the
    /// device not at all.
    #[test]
    fn an_over_full_table_refuses_by_name() {
        let (recorder, device, queue) = open();
        let table = table(device.as_ref(), queue, 2);
        let probes: Vec<GpuProbe> = (0..3).map(probe).collect();
        recorder.clear();
        let error = table
            .fill(device.as_ref(), queue, &probes)
            .expect_err("three probes do not fit a table of two");
        assert!(
            matches!(&error, HalError::InvalidDescriptor(message) if message.contains("2")
                && message.contains("3")),
            "got: {error:?}"
        );
        assert_eq!(writes(&recorder), [], "a refused fill stages nothing");
        assert_eq!(copies(&recorder), [], "and copies nothing");

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A ring of no frames is refused rather than served as a table nothing can
    /// bind.
    #[test]
    fn a_ring_of_no_frames_is_refused() {
        let (recorder, device, queue) = open();
        let before = recorder.total_live_objects();
        let error = ProbeTable::new(
            device.as_ref(),
            queue,
            &ProbeTableDesc {
                label: Some("test"),
                capacity: 4,
                frames: 0,
            },
        )
        .expect_err("a table with no frame in flight is bound by nothing");
        assert!(
            matches!(&error, HalError::InvalidDescriptor(message)
                if message.contains("frame in flight")),
            "got: {error:?}"
        );
        assert_eq!(recorder.total_live_objects(), before);
        recorder.assert_valid();
    }

    /// Everything created is destroyed — the ring, and the staging buffer and
    /// command buffer each upload builds.
    #[test]
    fn a_table_leaks_nothing() {
        let (recorder, device, queue) = open();
        let before = recorder.total_live_objects();
        let table = table(device.as_ref(), queue, 4);
        assert!(recorder.total_live_objects() > before);
        table
            .fill(device.as_ref(), queue, &[probe(0)])
            .expect("room for one");

        table.destroy(device.as_ref());
        assert_eq!(recorder.total_live_objects(), before);
        recorder.assert_valid();
    }
}
