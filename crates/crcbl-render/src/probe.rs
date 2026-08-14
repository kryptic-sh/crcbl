//! The irradiance probe table: one storage-buffer row per probe, filled once
//! from the scene description.
//!
//! ```text
//!  SceneDesc::probes ──▶ ProbeTable::new (cleared) ──▶ fill(rows) ──▶ binding 23
//!                        └─▶ ProbeGrid::volume rides in mesh::FrameUniforms
//! ```
//!
//! `docs/plan/18-render-features.md`'s irradiance probes: a static grid in a
//! read-only storage buffer, trilinearly interpolated by `mesh.slang` and added
//! to the flat ambient term. [`crcbl_shaders::probe::GpuProbe`] is the row and
//! [`crcbl_shaders::probe::ProbeVolume`] is the header that says where the rows
//! are — both live in the crate that owns the shader, because they are a
//! contract with it.
//!
//! # [`crate::material_table`]'s shape, minus everything a probe does not need
//!
//! One host-visible storage buffer, cleared at creation, written in place: the
//! same arrangement, for the same reason — a probe is written when the scene is
//! made resident and never rewritten, so a ring would be `frames_in_flight`
//! copies of a table nothing varies per frame.
//!
//! What is *not* copied across is the generational [`Pool`](crcbl_core::Pool).
//! A material row is handed out, edited and freed one at a time through a
//! handle; the probes arrive as one array and are written once, so there is
//! nothing to allocate and no handle for anything to go stale. Adding a pool
//! here would be an allocator with no caller.
//!
//! The consequence is stated rather than hidden, and it is
//! [`crate::material_table`]'s: **rewriting a probe while a frame is in flight
//! is a read-after-write hazard across submissions**. A moving light is what
//! makes this a ring, and there is no path to one today —
//! [`ForwardRenderer::with_scene`] is the only caller of [`ProbeTable::fill`].
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
//! [`ForwardRenderer::with_scene`]: crate::forward::ForwardRenderer::with_scene

use crcbl_hal::{BufferDesc, BufferHandle, BufferUsage, Device, HalError, MemoryLocation};
use crcbl_shaders::probe::{GpuProbe, PROBE_STRIDE};

/// How large a [`ProbeTable`] is, and what to call it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProbeTableDesc<'a> {
    /// Debug name; the buffer is named after it.
    pub(crate) label: Option<&'a str>,
    /// Probes the table can hold. Fixed, like every other pool the scene sizes.
    pub(crate) capacity: u32,
}

/// One `GpuProbe` array, written once.
#[derive(Debug)]
pub(crate) struct ProbeTable {
    buffer: BufferHandle,
    capacity: u32,
}

impl ProbeTable {
    /// Creates the table, **cleared**.
    ///
    /// Cleared for [`MaterialTable::new`](crate::material_table::MaterialTable::new)'s
    /// reason, and here it is the whole of the additive-zero property rather
    /// than a tidiness: row 0 is what a scene with no probes reads, and a
    /// driver's leftovers are neither zero nor a light anybody chose.
    ///
    /// The buffer holds `max(capacity, 1)` rows — see the [module docs](self)
    /// for why it can never be empty.
    ///
    /// # Errors
    ///
    /// [`HalError`] from any seam call. A failure part-way through releases the
    /// buffer it had already created.
    pub(crate) fn new(device: &dyn Device, desc: &ProbeTableDesc<'_>) -> Result<Self, HalError> {
        let stem = desc.label.unwrap_or("probe table");
        let size = u64::from(desc.capacity.max(1)) * PROBE_STRIDE as u64;
        let buffer = device.create_buffer(&BufferDesc {
            label: Some(&format!("{stem} probes")),
            size,
            // Host-visible, so filling it is a `write_buffer` rather than a
            // staging copy needing a barrier — the material table's choice, for
            // the material table's reason. The binding is read-only, which is
            // what the seam requires of a host-visible storage buffer.
            usage: BufferUsage::STORAGE,
            memory: MemoryLocation::HostUpload,
        })?;
        let cleared = vec![0u8; usize::try_from(size).unwrap_or(usize::MAX)];
        if let Err(error) = device.write_buffer(buffer, 0, &cleared) {
            device.destroy_buffer(buffer);
            return Err(error);
        }
        Ok(Self {
            buffer,
            capacity: desc.capacity,
        })
    }

    /// The table. Bound as a read-only storage buffer; the fragment stage
    /// indexes it with a grid cell.
    pub(crate) const fn buffer(&self) -> BufferHandle {
        self.buffer
    }

    /// Writes `probes` into the first rows of the table, in order.
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
    /// could not be written.
    pub(crate) fn fill(&self, device: &dyn Device, probes: &[GpuProbe]) -> Result<(), HalError> {
        if probes.len() > self.capacity as usize {
            return Err(HalError::InvalidDescriptor(format!(
                "the probe table holds {} and the description needs {}",
                self.capacity,
                probes.len()
            )));
        }
        // One write rather than one per row: the whole array is contiguous and
        // arrives at once, unlike a material's, so there is nothing for a
        // per-row loop to buy.
        let mut bytes = Vec::with_capacity(probes.len() * PROBE_STRIDE);
        for probe in probes {
            bytes.extend_from_slice(&probe.to_bytes());
        }
        if bytes.is_empty() {
            // Nothing to say to the device, and a zero-length write is not
            // something every backend accepts. The rows are already the zeroes
            // `new` wrote.
            return Ok(());
        }
        device.write_buffer(self.buffer, 0, &bytes)
    }

    /// Releases the buffer. The device must be idle.
    pub(crate) fn destroy(self, device: &dyn Device) {
        device.destroy_buffer(self.buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::{Event, NullInstance, Recorder};
    use crcbl_hal::{DeviceDesc, Instance};

    fn open() -> (Recorder, Box<dyn Device>) {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        (recorder, device)
    }

    fn table(device: &dyn Device, capacity: u32) -> ProbeTable {
        ProbeTable::new(
            device,
            &ProbeTableDesc {
                label: Some("test"),
                capacity,
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

    fn on_device(recorder: &Recorder, table: &ProbeTable, index: usize) -> GpuProbe {
        let bytes = recorder
            .buffer_bytes(table.buffer())
            .expect("the buffer is live");
        let at = index * PROBE_STRIDE;
        GpuProbe::from_bytes(bytes[at..at + PROBE_STRIDE].try_into().expect("one row"))
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

    /// **A probe's row is its index times the stride**, checked on the bytes
    /// that actually reached the device.
    ///
    /// An off-by-one or a stride the two sides disagree about would put every
    /// probe one row out and still write the right number of bytes — which is
    /// the failure a second probe exposes and a single-row table cannot.
    #[test]
    fn a_probes_row_is_its_index_times_the_stride() {
        let (recorder, device) = open();
        let table = table(device.as_ref(), 8);
        let probes: Vec<GpuProbe> = (0..4).map(probe).collect();
        table.fill(device.as_ref(), &probes).expect("room for four");

        for (index, wrote) in probes.iter().enumerate() {
            assert_eq!(
                &on_device(&recorder, &table, index),
                wrote,
                "row {index} does not hold the probe written into it"
            );
        }
        // The rows past the description keep the clear, which is what makes an
        // over-sized capacity add nothing rather than whatever the driver left.
        assert_eq!(on_device(&recorder, &table, 7), GpuProbe::ZERO);

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **The table is cleared when it is created, and holds a row even for a
    /// scene with no probes** — the two halves of what makes an empty grid add
    /// exactly zero on the device.
    #[test]
    fn an_empty_table_is_still_one_cleared_row() {
        let (recorder, device) = open();
        let table = table(device.as_ref(), 0);
        assert_eq!(
            writes(&recorder),
            [(0, PROBE_STRIDE)],
            "a capacity of zero must still create and clear one row, because a \
             buffer of no bytes is not a buffer"
        );
        assert_eq!(on_device(&recorder, &table, 0), GpuProbe::ZERO);

        // And filling it with nothing reaches the device not at all, rather
        // than as a zero-length write.
        let before = writes(&recorder).len();
        table.fill(device.as_ref(), &[]).expect("nothing to write");
        assert_eq!(writes(&recorder).len(), before);

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// More probes than the table holds is refused by name and reaches the
    /// device not at all.
    #[test]
    fn an_over_full_table_refuses_by_name() {
        let (recorder, device) = open();
        let table = table(device.as_ref(), 2);
        let probes: Vec<GpuProbe> = (0..3).map(probe).collect();
        let before = writes(&recorder).len();
        let error = table
            .fill(device.as_ref(), &probes)
            .expect_err("three probes do not fit a table of two");
        assert!(
            matches!(&error, HalError::InvalidDescriptor(message) if message.contains("2")
                && message.contains("3")),
            "got: {error:?}"
        );
        assert_eq!(
            writes(&recorder).len(),
            before,
            "a refused fill must not write a partial table"
        );

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// Everything created is destroyed.
    #[test]
    fn a_table_leaks_nothing() {
        let (recorder, device) = open();
        let before = recorder.total_live_objects();
        let table = table(device.as_ref(), 4);
        assert!(recorder.total_live_objects() > before);

        table.destroy(device.as_ref());
        assert_eq!(recorder.total_live_objects(), before);
        recorder.assert_valid();
    }
}
