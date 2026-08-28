//! The material table: one storage-buffer row per material, and the id an
//! instance carries to select one.
//!
//! ```text
//!  insert(device, material) ──▶ takes a slot ──▶ one write_buffer of one row
//!                               └─▶ index(handle) is what a GpuInstance carries
//!
//!  remove(device, handle) ──▶ frees the slot ──▶ the row is cleared to black
//! ```
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.2: "Material table (SSBO) …
//! Material id indexes the table". [`crcbl_shaders::mesh::GpuInstance::material`]
//! is that id and [`crcbl_shaders::mesh::GpuMaterial`] is that row — the layout
//! lives in the crate that owns `mesh.slang`, because it is a contract with the
//! shader.
//!
//! # The factors and one texture, which is an [`ArrayPages`] layer
//!
//! §3.2's sentence is "the table holds texture indices + factors", and a row
//! now holds both: [`GpuMaterial::base_color`] and
//! [`GpuMaterial::base_color_texture`]. The second is a **layer** of the one
//! `Texture2DArray` `mesh.slang` binds — an [`ArrayPages`] page — and not a
//! [`BindingModel::Bindless`] descriptor slot, which is the binding-model
//! decision this table used to be waiting on. A layer index needs nothing of a
//! device; a descriptor array needs `DESCRIPTOR_INDEXING`, which `crcbl-mtl`
//! withdraws. So one column serves every backend, and this table does not have
//! to know which model the device it is bound on selected.
//!
//! **This type does not own the page.** It stores an index and nothing else:
//! which image the layers live in, how many there are and what layer 0 holds
//! are [`crate::forward`]'s, exactly as which *mesh* a `GpuInstance::mesh`
//! names is [`crate::mesh_pool`]'s. So a row naming a layer the page does not
//! have samples out of range and nothing *here* can tell — which is why the
//! type that does own both refuses it: [`ForwardRenderer::with_scene`] checks
//! every [`SceneDesc::materials`] row against the page's layer count before it
//! creates anything, and names the row and the layer when it declines. A row
//! reaching [`MaterialTable::insert`] has already been through that.
//!
//! Still absent, and still deliberately: every other texture slot a material
//! could have — normal, metallic-roughness, emissive. `docs/plan/37-materials.md`
//! owns the shape a real material takes. The page's mip chain is [`crate::mip`]'s
//! and goes up with the page, and the sampler reads it trilinear.
//!
//! # One buffer, and no ring — unlike [`crate::instance_pool`]
//!
//! This is the decision worth reading before changing anything here, because
//! the two tables look alike and are not.
//!
//! [`InstancePool`](crate::instance_pool::InstancePool) keeps one buffer per
//! frame in flight and a set of dirty runs per buffer, because **the instance
//! array is rewritten every frame while the previous frame is still drawing
//! from it**: a moving object writes its transform each frame, so the coalesced
//! delta upload is the whole reason that type exists rather than a `Vec` the
//! renderer re-uploads.
//!
//! A material is written when it is *created*. That is [`crate::mesh_pool`]'s
//! mesh table exactly — "one host-visible buffer rather than a ring … nothing
//! rewrites an entry between frames, so there is no frame still reading the
//! value a write would replace" — and this follows it: one buffer, shared by
//! every frame's bind group, written in place. Copying the ring and the dirty
//! runs across would be a delta upload whose deltas are always one row, plus
//! `frames_in_flight` copies of a table nothing varies per frame; and factoring
//! the run-coalescing out to share it would be an abstraction with one real
//! user and one that only wants `write_buffer`.
//!
//! The cost is stated rather than hidden: **rewriting a material while a frame
//! is in flight is a read-after-write hazard across submissions**, the one
//! `docs/plan/02-vulkan-backend.md` calls this stage's headline risk. So
//! [`MaterialTable::set`] is a start-up call, like [`crate::mesh_pool::MeshPool::upload`]
//! and [`crate::texture::upload_texture`] beside it. An animated material is
//! what makes this a ring, and on that day the coalescing in
//! [`crate::instance_pool`] is worth sharing because there would be two callers
//! that need it.
//!
//! # The table never grows
//!
//! Capacity is fixed by [`MaterialTableDesc`] and [`MaterialTable::insert`]
//! fails by name when it runs out — [`crate::instance_pool`]'s reason, since
//! every row is the same size: growing a device buffer means allocating a
//! larger one, copying, and re-pointing every bind group that names the old
//! one.
//!
//! # A cleared row is black, which is the point
//!
//! [`MaterialTable::new`] clears the buffer and [`MaterialTable::remove`]
//! clears the row it frees, so a row nobody has written and a row whose
//! material was removed read the same: a base colour of zero, which shades
//! black.
//!
//! There is no "no material" value to write instead. A mesh table has one —
//! `index_count == 0` is an entry naming no mesh — because a mesh id resolves
//! to a *range*, and the empty range draws nothing. Every RGBA factor is a
//! material somebody could want, so the empty value and a legitimate one are
//! the same bytes. Black is therefore chosen for how it fails: an instance
//! whose material was never created is a black object, which is seen, where a
//! white one is an object that looks correct.
//!
//! [`BindingModel::Bindless`]: crcbl_hal::BindingModel::Bindless
//! [`ArrayPages`]: crcbl_hal::BindingModel::ArrayPages
//! [`GpuMaterial::base_color`]: crcbl_shaders::mesh::GpuMaterial::base_color
//! [`GpuMaterial::base_color_texture`]: crcbl_shaders::mesh::GpuMaterial::base_color_texture
//! [`ForwardRenderer::with_scene`]: crate::forward::ForwardRenderer::with_scene
//! [`SceneDesc::materials`]: crate::scene::SceneDesc::materials

use crcbl_core::{Handle, Pool};
use crcbl_hal::{BufferDesc, BufferHandle, BufferUsage, Device, HalError, MemoryLocation};
use crcbl_shaders::mesh::{GpuMaterial, MATERIAL_STRIDE};

/// Marker type for [`MaterialHandle`]. Uninhabited.
#[derive(Debug)]
pub enum Material {}

/// A material in a [`MaterialTable`]. Generational, so a removed material's
/// handle stops resolving rather than naming whatever took its row.
pub type MaterialHandle = Handle<Material>;

/// What a [`MaterialTable`] refuses to do.
#[derive(Debug, thiserror::Error)]
pub enum MaterialTableError {
    /// Every row is in use.
    ///
    /// `in_use` is normally the number of live materials. It can exceed that
    /// only through generation exhaustion — see [`Pool`]'s docs on retired
    /// slots — and the two are reported together for
    /// [`InstancePoolError::PoolFull`](crate::instance_pool::InstancePoolError::PoolFull)'s
    /// reason: so that case is legible rather than baffling.
    #[error("the material table holds {capacity} materials and {in_use} of them are in use")]
    TableFull {
        /// The table's fixed capacity.
        capacity: u32,
        /// Rows that cannot be handed out.
        in_use: u32,
    },
    /// Anything the seam refused.
    #[error(transparent)]
    Hal(#[from] HalError),
}

/// Flattens a table error into the seam's, for callers whose constructors are
/// already `Result<_, HalError>`.
///
/// Same shape and same reason as
/// [`InstancePoolError`](crate::instance_pool::InstancePoolError)'s: the seam
/// has no vocabulary for "this table is full", so everything that is not
/// already a [`HalError`] arrives as [`Backend`](HalError::Backend), which
/// renders verbatim and keeps the numbers.
impl From<MaterialTableError> for HalError {
    fn from(error: MaterialTableError) -> Self {
        match error {
            MaterialTableError::Hal(hal) => hal,
            other => Self::Backend(other.to_string()),
        }
    }
}

/// How large a [`MaterialTable`] is, and what to call it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialTableDesc<'a> {
    /// Debug name; the buffer is named after it.
    pub label: Option<&'a str>,
    /// Materials the table can hold. Fixed — see the [module docs](self).
    pub capacity: u32,
}

/// One `GpuMaterial` array, written in place.
///
/// See the [module docs](self) for why there is no ring and why a cleared row
/// is black — both are decisions rather than accidents.
#[derive(Debug)]
pub struct MaterialTable {
    buffer: BufferHandle,
    /// Generational row allocation, and nothing else: the values live in the
    /// buffer. Reusing the crate's pool rather than writing a second allocator
    /// that gets the generation-retirement rule subtly different, exactly as
    /// [`crate::instance_pool`] does.
    slots: Pool<()>,
    capacity: u32,
}

impl MaterialTable {
    /// Creates the table, **cleared**.
    ///
    /// Cleared rather than left as the driver handed it over, for the reason
    /// [`crate::mesh_pool::MeshPool`] clears its own table: a row nothing has
    /// written is read by any instance naming it, and "a zeroed row is black"
    /// is only a contract if it holds for the rows nobody has touched as well
    /// as for the ones [`MaterialTable::remove`] clears. A driver's leftovers
    /// are not zeroes, and they are not a colour anybody chose either.
    ///
    /// # Errors
    ///
    /// [`MaterialTableError::Hal`] from any seam call. A failure part-way
    /// through releases the buffer it had already created.
    pub fn new(
        device: &dyn Device,
        desc: &MaterialTableDesc<'_>,
    ) -> Result<Self, MaterialTableError> {
        let stem = desc.label.unwrap_or("material table");
        let size = u64::from(desc.capacity) * MATERIAL_STRIDE as u64;
        let buffer = device.create_buffer(&BufferDesc {
            label: Some(&format!("{stem} materials")),
            size,
            // Host-visible, so a write is a `write_buffer` rather than a
            // staging copy needing a barrier. Same choice, and the same
            // reasoning, as the mesh table's.
            usage: BufferUsage::STORAGE,
            memory: MemoryLocation::HostUpload,
        })?;
        let cleared = vec![0u8; usize::try_from(size).unwrap_or(usize::MAX)];
        if let Err(error) = device.write_buffer(buffer, 0, &cleared) {
            device.destroy_buffer(buffer);
            return Err(error.into());
        }
        Ok(Self {
            buffer,
            slots: Pool::new(),
            capacity: desc.capacity,
        })
    }

    /// The table. Bound as a read-only storage buffer; the vertex stage indexes
    /// it with an instance's material id.
    ///
    /// One buffer for every frame's bind group, unlike
    /// [`InstancePool::buffers`](crate::instance_pool::InstancePool::buffers) —
    /// see the [module docs](self).
    #[must_use]
    pub const fn buffer(&self) -> BufferHandle {
        self.buffer
    }

    /// How many materials the table can hold, which is the bound every material
    /// id is below.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.capacity
    }

    /// How many materials are live.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether no material is live.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Takes a row, writes `material` into it, and returns its handle.
    ///
    /// # Errors
    ///
    /// [`MaterialTableError::TableFull`] when every row is in use — the table
    /// never grows, see the [module docs](self) — and
    /// [`MaterialTableError::Hal`] if the row could not be written. A refused
    /// insert takes no row.
    pub fn insert(
        &mut self,
        device: &dyn Device,
        material: &GpuMaterial,
    ) -> Result<MaterialHandle, MaterialTableError> {
        // `Pool` hands out a fresh index only when it has no vacant slot, so
        // "live plus permanently retired" is exactly the highest index it could
        // be about to create. Checked before inserting rather than after, so a
        // refused insert does not burn a generation.
        let in_use = self.in_use();
        if in_use >= self.capacity {
            return Err(MaterialTableError::TableFull {
                capacity: self.capacity,
                in_use,
            });
        }
        let handle: MaterialHandle = self.slots.insert(()).cast();
        match self.write(device, handle.index(), material) {
            Ok(()) => Ok(handle),
            // The row is whatever the clear or the previous occupant left, and
            // the slot goes back rather than staying allocated to a material
            // the GPU never received.
            Err(error) => {
                self.slots.remove(handle.cast());
                Err(error.into())
            }
        }
    }

    /// Rewrites `handle`'s row.
    ///
    /// Returns whether the handle named a live material; a stale one writes
    /// nothing rather than recolouring whatever took its row.
    ///
    /// **A start-up call**, like [`crate::mesh_pool::MeshPool::upload`]: the
    /// table has no ring, so a row rewritten while a frame is in flight is a
    /// read-after-write hazard across submissions. The [module docs](self) say
    /// what would have to change first.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the row could not be written.
    pub fn set(
        &mut self,
        device: &dyn Device,
        handle: MaterialHandle,
        material: &GpuMaterial,
    ) -> Result<bool, HalError> {
        if !self.slots.contains(handle.cast()) {
            return Ok(false);
        }
        self.write(device, handle.index(), material)?;
        Ok(true)
    }

    /// Which row `handle` is, or `None` if the handle is stale.
    ///
    /// This is the number a [`GpuInstance::material`](crcbl_shaders::mesh::GpuInstance::material)
    /// carries, which is why it is public: an instance has to be able to say
    /// which material it is shaded with.
    #[must_use]
    pub fn index(&self, handle: MaterialHandle) -> Option<u32> {
        self.slots.contains(handle.cast()).then(|| handle.index())
    }

    /// Frees `handle`'s row for reuse, retires the handle, and **clears the row
    /// to black**.
    ///
    /// Returns whether the handle named a live material. The clear is what
    /// makes an instance still naming the row shade black rather than take the
    /// colour of whatever material lands there next — the same defect a stale
    /// mesh-table entry produces, and the same answer
    /// [`crate::mesh_pool::MeshPool::free`] gives it. What neither can defend
    /// against is a row *reused*: a material id is a bare `u32` with no
    /// generation in it, and only [`MaterialHandle`] can tell those apart.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the row could not be cleared. The slot is freed either
    /// way: a handle that failed to clear is still retired, and the row holds
    /// its old colour until something writes it.
    pub fn remove(
        &mut self,
        device: &dyn Device,
        handle: MaterialHandle,
    ) -> Result<bool, HalError> {
        if self.slots.remove(handle.cast()).is_none() {
            return Ok(false);
        }
        self.write(device, handle.index(), &GpuMaterial::default())?;
        Ok(true)
    }

    /// Releases the buffer. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        device.destroy_buffer(self.buffer);
    }

    /// Rows that cannot be handed out: live ones, plus any permanently
    /// withdrawn by generation exhaustion.
    fn in_use(&self) -> u32 {
        let used = self.slots.len() + self.slots.retired_slots();
        u32::try_from(used).unwrap_or(u32::MAX)
    }

    /// Writes one row, at the offset `index` names.
    ///
    /// The one place a [`GpuMaterial`] becomes bytes, so the element/byte
    /// conversion has a single point of contact rather than one per call site.
    fn write(
        &self,
        device: &dyn Device,
        index: u32,
        material: &GpuMaterial,
    ) -> Result<(), HalError> {
        let at = u64::from(index) * MATERIAL_STRIDE as u64;
        device.write_buffer(self.buffer, at, &material.to_bytes())
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

    fn table(device: &dyn Device, capacity: u32) -> MaterialTable {
        MaterialTable::new(
            device,
            &MaterialTableDesc {
                label: Some("test"),
                capacity,
            },
        )
        .expect("the null backend accepts every descriptor")
    }

    /// A material distinguishable from every other `n`, with no two floats
    /// equal — so a row read at the wrong offset, or a word written in the
    /// wrong order, is a different value rather than the same one.
    ///
    /// The texture layer is `n + 7` rather than `n`, so it is different from the
    /// row index, from the material number and from every factor in the row: an
    /// index read out of the wrong word cannot come out right by coincidence.
    ///
    /// The shading factors, the tiling pair and the emissive triple are on the
    /// same scheme, and they are the ones the scheme most has to cover: they sit
    /// past the texture index, so a writer that still stops there leaves them
    /// zero and this is what says so. Every fraction is distinct and inside
    /// `0..1`, so no value of one row can coincide with a value of another.
    fn material(n: u32) -> GpuMaterial {
        let base = n as f32;
        GpuMaterial {
            base_color: [base, base + 0.125, base + 0.25, base + 0.375],
            base_color_texture: n + 7,
            metallic: base + 0.5,
            roughness: base + 0.625,
            tiling: n % 2,
            tile_metres: base + 0.75,
            // The last three words of the row, and the ones a writer that
            // stopped at the old padding leaves zero.
            emissive: [base + 0.8125, base + 0.875, base + 0.9375],
        }
    }

    /// Row `index` of the table, decoded from the bytes that actually reached
    /// the device.
    ///
    /// Read back rather than taken from any host-side copy — there is none, and
    /// that is the point: an assertion here is evidence about what a shader
    /// would read.
    fn on_device(recorder: &Recorder, table: &MaterialTable, index: u32) -> GpuMaterial {
        let bytes = recorder
            .buffer_bytes(table.buffer())
            .expect("the buffer is live");
        let at = index as usize * MATERIAL_STRIDE;
        GpuMaterial::from_bytes(bytes[at..at + MATERIAL_STRIDE].try_into().expect("one row"))
    }

    /// Every `BufferWritten` this recorder saw, as `(offset, len)`.
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

    /// **The headline claim: a material id is a row, and the row it is is
    /// `index * MATERIAL_STRIDE` bytes in.**
    ///
    /// Checked by filling several rows and reading each back at the offset its
    /// index names. An off-by-one or a stride the two sides disagree about
    /// would put every material one row out and still write the right number of
    /// bytes — which is the failure a second material exists to expose and a
    /// single-row table cannot.
    #[test]
    fn a_materials_row_is_its_index_times_the_stride() {
        let (recorder, device) = open();
        let mut table = table(device.as_ref(), 8);
        let handles: Vec<MaterialHandle> = (0..4)
            .map(|n| {
                table
                    .insert(device.as_ref(), &material(n))
                    .expect("room for four")
            })
            .collect();

        for (n, handle) in handles.iter().enumerate() {
            let index = u32::try_from(n).expect("four rows");
            assert_eq!(
                table.index(*handle),
                Some(index),
                "the {n}th insert must take the {n}th row"
            );
            assert_eq!(
                on_device(&recorder, &table, index),
                material(index),
                "row {index} does not hold the material inserted into it"
            );
        }

        // And the seam was given exactly one row's worth at that offset, so the
        // agreement above is not a table that writes itself whole every time.
        let before = writes(&recorder).len();
        table
            .insert(device.as_ref(), &material(9))
            .expect("room for five");
        assert_eq!(
            writes(&recorder)[before..],
            [(4 * MATERIAL_STRIDE as u64, MATERIAL_STRIDE)]
        );

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **A row's texture layer reaches the device, and two rows can differ in
    /// nothing else.**
    ///
    /// §3.2's "texture indices + factors", from the table's side: the number a
    /// shader reads to pick a page layer is the number a caller inserted, at
    /// the offset the row's layout puts it. Two materials with an identical
    /// factor and different layers are what the rendered observable rests on,
    /// so they are checked here to be two different rows rather than one value
    /// written twice.
    #[test]
    fn a_rows_texture_layer_is_written_beside_its_factor() {
        let (recorder, device) = open();
        let mut table = table(device.as_ref(), 4);
        let untextured = GpuMaterial::UNTINTED;
        let textured = GpuMaterial {
            base_color_texture: 2,
            ..untextured
        };
        assert_eq!(
            untextured.base_color, textured.base_color,
            "the pair must differ in the texture layer and nothing else"
        );

        let first = table.insert(device.as_ref(), &untextured).expect("room");
        let second = table.insert(device.as_ref(), &textured).expect("room");

        assert_eq!(on_device(&recorder, &table, 0), untextured);
        assert_eq!(on_device(&recorder, &table, 1), textured);
        assert_eq!(
            on_device(&recorder, &table, 1).base_color_texture,
            2,
            "the layer a caller asked for must be the layer the buffer holds"
        );
        assert_ne!(
            on_device(&recorder, &table, 0),
            on_device(&recorder, &table, 1),
            "two rows differing only in their layer must be two different rows"
        );
        assert_eq!(table.index(first), Some(0));
        assert_eq!(table.index(second), Some(1));

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **The table is cleared when it is created**, whole — the other half of
    /// the black-row contract: an instance may name a row nobody has written,
    /// and what it reads has to be a colour this crate chose.
    #[test]
    fn the_table_is_cleared_when_it_is_created() {
        let (recorder, device) = open();
        let table = table(device.as_ref(), 8);
        assert_eq!(
            writes(&recorder),
            [(0, 8 * MATERIAL_STRIDE)],
            "the whole table must be cleared before anything reads it"
        );
        assert_eq!(table.capacity(), 8);
        assert!(table.is_empty());

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A rewritten material reaches the device, and a stale handle rewrites
    /// nothing.
    #[test]
    fn a_material_can_be_rewritten_and_a_stale_handle_cannot() {
        let (recorder, device) = open();
        let mut table = table(device.as_ref(), 4);
        let handle = table.insert(device.as_ref(), &material(1)).expect("room");
        assert_eq!(on_device(&recorder, &table, 0), material(1));

        assert!(
            table
                .set(device.as_ref(), handle, &material(2))
                .expect("the null device writes")
        );
        assert_eq!(on_device(&recorder, &table, 0), material(2));

        assert!(table.remove(device.as_ref(), handle).expect("writes"));
        let before = writes(&recorder).len();
        assert!(
            !table
                .set(device.as_ref(), handle, &material(3))
                .expect("a stale handle is not an error"),
            "a retired handle must not resolve"
        );
        assert_eq!(
            writes(&recorder).len(),
            before,
            "and must not reach the device at all"
        );
        assert_eq!(table.index(handle), None);

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **Removing a material clears its row to black on the device, and
    /// disturbs no other row.**
    ///
    /// The whole of what a removal owes the GPU: an instance still naming the
    /// row shades black rather than taking the next material's colour. Asserted
    /// on the bytes in the buffer, because a removal that never reached it is a
    /// removal the GPU does not know about.
    #[test]
    fn removing_a_material_clears_its_row_and_leaves_its_neighbours() {
        let (recorder, device) = open();
        let mut table = table(device.as_ref(), 4);
        let first = table.insert(device.as_ref(), &material(1)).expect("room");
        let second = table.insert(device.as_ref(), &material(2)).expect("room");

        assert!(table.remove(device.as_ref(), first).expect("writes"));
        assert_eq!(
            on_device(&recorder, &table, 0),
            GpuMaterial::default(),
            "the freed row must read black"
        );
        assert_ne!(
            GpuMaterial::default(),
            material(1),
            "the material removed was not already black, so the clear is observable"
        );
        assert_eq!(
            on_device(&recorder, &table, 1),
            material(2),
            "the row beside the removed one was disturbed"
        );
        assert_eq!(table.index(second), Some(1));

        // A second removal of the retired handle writes nothing, so a caller
        // cannot black out whatever has since taken the row.
        let before = writes(&recorder).len();
        assert!(!table.remove(device.as_ref(), first).expect("writes"));
        assert_eq!(writes(&recorder).len(), before);

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A full table refuses by name and takes nothing, and a freed row is
    /// handed out again — so the refusal is the capacity rather than the table
    /// having given up.
    #[test]
    fn a_full_table_fails_by_name_and_takes_nothing() {
        let (recorder, device) = open();
        let mut table = table(device.as_ref(), 2);
        let first = table.insert(device.as_ref(), &material(0)).expect("room");
        table.insert(device.as_ref(), &material(1)).expect("room");

        let error = table
            .insert(device.as_ref(), &material(2))
            .expect_err("two materials fill a table of two");
        assert!(
            matches!(
                error,
                MaterialTableError::TableFull {
                    capacity: 2,
                    in_use: 2
                }
            ),
            "got: {error:?}"
        );
        assert_eq!(table.len(), 2, "a refused insert must not consume a row");

        assert!(table.remove(device.as_ref(), first).expect("writes"));
        let reused = table
            .insert(device.as_ref(), &material(3))
            .expect("the freed row");
        assert_eq!(table.index(reused), Some(0), "the low row comes back");
        assert_eq!(table.index(first), None, "and the old handle is retired");
        assert_eq!(on_device(&recorder, &table, 0), material(3));

        table.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// Everything created is destroyed.
    #[test]
    fn a_table_leaks_nothing() {
        let (recorder, device) = open();
        let before = recorder.total_live_objects();
        let mut table = table(device.as_ref(), 4);
        table.insert(device.as_ref(), &material(0)).expect("room");
        assert!(recorder.total_live_objects() > before);

        table.destroy(device.as_ref());
        assert_eq!(
            recorder.total_live_objects(),
            before,
            "every buffer created must be destroyed"
        );
        recorder.assert_valid();
    }

    /// A table error that has to travel as a [`HalError`] keeps its message,
    /// which is the whole reason the exhaustion error names its numbers.
    #[test]
    fn a_table_error_flattens_into_the_seams_without_losing_its_message() {
        let full = MaterialTableError::TableFull {
            capacity: 4,
            in_use: 4,
        };
        let message = full.to_string();
        assert_eq!(HalError::from(full).to_string(), message);

        let hal = MaterialTableError::Hal(HalError::OutOfDeviceMemory);
        assert!(
            matches!(HalError::from(hal), HalError::OutOfDeviceMemory),
            "a HalError must not be stringified on the way through"
        );
    }
}
