//! The instance array: one storage-buffer element per drawable object, written
//! by **delta upload** — the changed elements only, coalesced into runs.
//!
//! ```text
//!  set(handle, instance) ──▶ mirror[index] rewritten
//!                            └─▶ index marked dirty in every frame's range set
//!
//!  begin_frame(device) ──▶ rotate to the next frame's buffer
//!                          └─▶ one write_buffer per dirty run, then clear
//!                              └─▶ nothing dirty ⇒ nothing written at all
//! ```
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.2: "`GpuInstance` array (SSBO):
//! transform, mesh id, material id, flags. Written by delta upload each frame
//! (**changed instances only — dirty ranges, not full re-upload**)." The
//! emphasis is the whole reason this type exists rather than a `Vec` re-uploaded
//! every frame, which is what [`crate::sprite_pass`] does and what §3.2 is
//! written against.
//!
//! The record itself — and which of its fields are reserved rather than working
//! — is [`crcbl_shaders::mesh::GpuInstance`], because the layout is a contract
//! with `mesh.slang` and belongs in the crate that owns that source.
//!
//! # Dirty runs, and the case they exist for
//!
//! The pool holds a sorted list of half-open runs that never touch and
//! never overlap. Marking an index that **abuts** a run extends it; marking one
//! that closes the gap between two runs merges them; marking one that is already
//! inside a run does nothing. So writing instances 3, 4 and 5 produces one
//! upload of three elements and writing 3 and 900 produces two uploads of one —
//! whatever order the marks arrive in.
//!
//! Nothing merges across a gap, however small: two runs one element apart stay
//! two. A gap-tolerant merge trades bytes for calls, and choosing where that
//! trade sits needs a real scene to measure — §3.3's, which is also the first
//! thing that will move more than one instance per frame.
//!
//! # A ring, because the previous frame is still reading
//!
//! There is one buffer **per frame in flight**, for the reason
//! [`crate::forward`]'s uniform ring gives: an instance the CPU rewrites while
//! the GPU is still drawing last frame is a read-after-write hazard across
//! submissions, which is `docs/plan/02-vulkan-backend.md`'s headline risk for
//! this stage.
//!
//! The consequence is that a write is dirty in **every** slot's range set, not
//! one: each slot is a separate copy of the same logical array and each has to
//! be caught up before it is drawn from. So one changed instance costs one small
//! upload per frame for the next `frames_in_flight` frames, and then nothing.
//! That is still delta upload — the cost is the size of what changed, not the
//! size of the array — and [`InstancePool::begin_frame`] writes nothing at all
//! once every slot has caught up.
//!
//! # Where each instance was last frame
//!
//! [`GpuInstance::previous_transform`] is the pool's field, not the caller's,
//! the way the liveness bit is. [`InstancePool::set`] fills it from what the
//! record already holds, [`InstancePool::insert`] fills it from the new
//! transform — a spawn did not travel from anywhere — and
//! [`InstancePool::rotate`] puts back at rest whatever moved a frame ago and
//! has not moved again. [`InstancePool::set_bases`] is the one write that is
//! **not** a move: it rewrites a record's two base vertices and leaves this
//! field and the carry-forward enrolment exactly as they stand, because the
//! per-frame re-point a skinned object goes through is not a caller saying the
//! object travelled.
//!
//! That last step is what makes the field mean *last frame* rather than *the
//! last time anyone wrote this*. An instance that moves for a while and then
//! stands still is written on the frames it moves and not after; without the
//! carry-forward its record would go on naming the transform it left, and every
//! temporal consumer would go on smearing it along a journey that finished.
//!
//! It is the frame *before* the one being assembled that settles, because a
//! caller writes a frame's instances and then calls
//! [`InstancePool::begin_frame`] — so the moves of the frame about to be drawn
//! have already happened by the time the rotate runs, and settling those would
//! erase each one before it was ever drawn.
//!
//! The cost is one comparison per element per frame after the last write to it,
//! and one extra upload of that element on the first frame it stands still.
//! Nothing reads the field yet — `docs/plan/43-render-standards.md` §9 is why
//! it is here before its readers are.
//!
//! # The pool never grows
//!
//! Capacity is fixed by [`InstancePoolDesc`] and [`InstancePool::insert`] fails
//! by name when it runs out. Same reason [`crate::mesh_pool`] gives: growing a
//! device buffer means allocating a larger one, copying, and re-pointing every
//! bind group that names the old one, in the middle of a frame.
//!
//! Unlike the geometry pools there is no fragmentation to report, because every
//! instance is the same size: a slot that comes back fits whatever asks for it
//! next. [`InstancePoolError::PoolFull`] therefore names one number where
//! [`crate::mesh_pool::MeshPoolError::PoolExhausted`] names three.
//!
//! # What a removed instance leaves behind, and the one bit that says so
//!
//! [`InstancePool::remove`] frees the slot and retires the handle, and it
//! rewrites **one field** of the element: it clears
//! [`GpuInstance::LIVE`] from the record's flags and marks
//! the slot dirty, so the next [`InstancePool::begin_frame`] carries the
//! removal to the GPU. Everything else the removed instance held — its
//! transform, its mesh id — stays exactly as it was.
//!
//! That is the whole contract, and it is what §3.3's cull pass needs: the pass
//! walks the array from element zero, so a freed slot has to be able to say it
//! is one. `cull.slang` asks the bit before it reads anything else in the
//! record, and [`crate::cull::visible_instances`] — the CPU oracle — does the
//! same.
//!
//! **The bit is the pool's, not the caller's.** [`InstancePool::insert`] and
//! [`InstancePool::set`] set it whatever the caller passed, so an instance
//! cannot arrive dead by omission, and a caller cannot clear it without going
//! through `remove`. The other bits of the word are the caller's and are left
//! alone.
//!
//! ## Why the record is not zeroed
//!
//! [`crate::mesh_pool::MeshPool::free`] clears its table entry to all zeroes,
//! and the analogous move here would be to zero the whole instance. It is the
//! wrong one. What the mesh table is doing is *writing the empty value of the
//! field that answers the question* — `index_count` — and the fields it clears
//! with it are ones a stale entry would otherwise be misread through; the
//! parallel here is clearing the liveness bit, which is that same write.
//!
//! Zeroing the rest would make it worse rather than tidier. A zeroed transform
//! is a degenerate box at the origin and a zeroed mesh id names table entry 0,
//! which is an ordinary resident mesh — so a consumer that skipped the liveness
//! check would see a removed instance as *visible whenever the origin is*,
//! where leaving the record alone at least leaves it wherever it was. The
//! failure a shortcut produces should not be the one that looks plausible.

use core::ops::Range;

use crcbl_core::{Handle, Pool};
use crcbl_hal::{BufferDesc, BufferHandle, BufferUsage, Device, HalError, MemoryLocation};
use crcbl_shaders::mesh::{GpuInstance, INSTANCE_STRIDE};

/// Marker type for [`InstanceHandle`]. Uninhabited.
#[derive(Debug)]
pub enum Instance {}

/// An instance in an [`InstancePool`]. Generational, so a removed instance's
/// handle stops resolving rather than naming whatever took its slot.
pub type InstanceHandle = Handle<Instance>;

/// What an [`InstancePool`] refuses to do.
#[derive(Debug, thiserror::Error)]
pub enum InstancePoolError {
    /// Every slot is in use.
    ///
    /// `in_use` is normally the number of live instances. It can exceed that
    /// only through generation exhaustion — see [`Pool`]'s docs on retired
    /// slots, which needs about 4.3 billion removals of one slot — and the two
    /// are reported together so that case is legible rather than baffling.
    #[error("the instance pool holds {capacity} instances and {in_use} of them are in use")]
    PoolFull {
        /// The pool's fixed capacity.
        capacity: u32,
        /// Slots that cannot be handed out.
        in_use: u32,
    },
    /// Anything the seam refused.
    #[error(transparent)]
    Hal(#[from] HalError),
}

/// Flattens a pool error into the seam's, for callers whose constructors are
/// already `Result<_, HalError>`.
///
/// Same shape and same reason as [`crate::mesh_pool::MeshPoolError`]'s: the seam
/// has no vocabulary for "this pool is full", so everything that is not already
/// a [`HalError`] arrives as [`Backend`](HalError::Backend), which renders
/// verbatim and keeps the numbers.
impl From<InstancePoolError> for HalError {
    fn from(error: InstancePoolError) -> Self {
        match error {
            InstancePoolError::Hal(hal) => hal,
            other => Self::Backend(other.to_string()),
        }
    }
}

/// How large an [`InstancePool`] is, and what to call it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstancePoolDesc<'a> {
    /// Debug name; each frame's buffer is named after it.
    pub label: Option<&'a str>,
    /// Instances the pool can hold. Fixed — see the [module docs](self).
    pub capacity: u32,
    /// How many buffers to keep, one per frame in flight. Must be at least one.
    pub frames_in_flight: usize,
}

/// One `GpuInstance` array, mirrored on the host and uploaded by delta.
///
/// See the [module docs](self) for the coalescing rule, the ring, and what a
/// removal leaves behind — all three are decisions rather than accidents.
#[derive(Debug)]
pub struct InstancePool {
    /// One per frame in flight; `frame` says which is current.
    buffers: Vec<BufferHandle>,
    /// What has changed and not yet reached each buffer. Same length as
    /// `buffers`, and indexed the same way.
    dirty: Vec<DirtyRanges>,
    /// The slot `begin_frame` last rotated to.
    frame: usize,

    /// The array exactly as a buffer holds it, so a dirty run is one contiguous
    /// slice and there is no second copy of an instance to drift from this one.
    mirror: Vec<u8>,
    /// Generational slot allocation, and nothing else: the values live in
    /// `mirror`. Reusing the crate's pool rather than writing a second
    /// allocator that gets the generation-retirement rule subtly different.
    slots: Pool<()>,
    /// Elements a caller has written since the last [`InstancePool::rotate`],
    /// which is what the frame being assembled now moves.
    written_this_frame: Vec<u32>,
    /// `written_this_frame` as it stood at the last [`InstancePool::rotate`]:
    /// what the frame already drawn moved, and so what the next rotate has to
    /// put back at rest. See the [module docs](self) on the previous transform.
    written_last_frame: Vec<u32>,
    /// Whether each element is already in `written_this_frame`, so a caller
    /// that writes one instance repeatedly in a frame enrols it once. Indexed
    /// by element, `capacity` long.
    moved_mark: Vec<bool>,
    capacity: u32,
    /// One past the highest element [`InstancePool::insert`] has ever handed
    /// out — what [`InstancePool::slot_count`] reports.
    high_water: u32,
    /// How many times the mirror has been written — [`InstancePool::revision`].
    revision: u64,
}

impl InstancePool {
    /// Creates the ring, with every buffer **cleared**.
    ///
    /// Cleared rather than left as the driver handed it over, and for
    /// [`crate::mesh_pool::MeshPool`]'s reason rather than for tidiness: a slot
    /// nothing has written is read by anything that walks the array, and "a
    /// zeroed element is a dead instance" is only a contract if it holds for
    /// the slots no one has touched as well as for the ones
    /// [`InstancePool::remove`] clears. A driver's leftovers are not zeroes,
    /// and a leftover with [`GpuInstance::LIVE`] set is an instance the cull
    /// pass keeps, at a transform nobody chose.
    ///
    /// This is why it was not always so: until the flags word had a defined
    /// bit, zeroing bought nothing an element written before it is addressed
    /// did not already have.
    ///
    /// The device buffers are created **before** the host mirror, so a capacity
    /// no device could hold arrives as a [`HalError`] naming the buffer rather
    /// than as an aborting host allocation of the same size.
    ///
    /// # Errors
    ///
    /// [`InstancePoolError::Hal`] from any seam call. A failure part-way through
    /// releases whatever was already created.
    ///
    /// # Panics
    ///
    /// If `frames_in_flight` is zero: a ring with no buffers has nothing to
    /// bind, and every later index into it would be the real failure.
    pub fn new(
        device: &dyn Device,
        desc: &InstancePoolDesc<'_>,
    ) -> Result<Self, InstancePoolError> {
        assert!(
            desc.frames_in_flight > 0,
            "an instance pool needs at least one buffer to bind"
        );
        let stem = desc.label.unwrap_or("instance pool");
        let size = u64::from(desc.capacity) * INSTANCE_STRIDE as u64;
        let cleared = vec![0u8; desc.capacity as usize * INSTANCE_STRIDE];
        let mut buffers = Vec::with_capacity(desc.frames_in_flight);
        for frame in 0..desc.frames_in_flight {
            let created = device
                .create_buffer(&BufferDesc {
                    label: Some(&format!("{stem} instances {frame}")),
                    size,
                    // Host-visible, so a delta is a `write_buffer` rather than a
                    // staging copy needing a barrier inside a frame.
                    // `crate::graph` is the only thing allowed to record a
                    // barrier during a frame, and a per-frame transfer submit is
                    // what §3.1's start-up upload path deliberately is not.
                    usage: BufferUsage::STORAGE,
                    memory: MemoryLocation::HostUpload,
                })
                // A buffer created and not cleared is one the failure path below
                // has to release, so the clear happens here rather than in a
                // second loop over the finished ring.
                .and_then(|buffer| match device.write_buffer(buffer, 0, &cleared) {
                    Ok(()) => Ok(buffer),
                    Err(error) => {
                        device.destroy_buffer(buffer);
                        Err(error)
                    }
                });
            match created {
                Ok(buffer) => buffers.push(buffer),
                Err(error) => {
                    for buffer in buffers {
                        device.destroy_buffer(buffer);
                    }
                    return Err(error.into());
                }
            }
        }
        Ok(Self {
            buffers,
            dirty: vec![DirtyRanges::default(); desc.frames_in_flight],
            frame: 0,
            mirror: vec![0; desc.capacity as usize * INSTANCE_STRIDE],
            slots: Pool::new(),
            written_this_frame: Vec::new(),
            written_last_frame: Vec::new(),
            moved_mark: vec![false; desc.capacity as usize],
            capacity: desc.capacity,
            high_water: 0,
            revision: 0,
        })
    }

    /// The buffers, one per frame in flight, in slot order.
    ///
    /// A caller builds one bind group per slot naming the buffer at that slot,
    /// and draws frame `n` with the group whose slot [`InstancePool::begin_frame`]
    /// returned.
    #[must_use]
    pub fn buffers(&self) -> &[BufferHandle] {
        &self.buffers
    }

    /// How many instances the pool can hold.
    #[must_use]
    pub const fn capacity(&self) -> u32 {
        self.capacity
    }

    /// How many instances are live.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether no instance is live.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Array elements anything walking the pool has to cover: one past the
    /// highest slot ever handed out.
    ///
    /// **Not [`InstancePool::len`]**, which counts live instances and so shrinks
    /// when one is removed while the slots above it stay occupied. A cull
    /// dispatch sized by `len` would silently stop testing the instances past
    /// the gap, which reads as "those were culled".
    ///
    /// It never shrinks, because a slot handed out once may be handed out again
    /// and nothing compacts the array. Every element below it is either a live
    /// instance or a dead record — [`InstancePool::remove`] leaves one, and
    /// [`InstancePool::new`] clears the buffers so an untouched slot is one too
    /// — which is what makes walking the whole range safe.
    /// How many times any element of the array has been written, over the
    /// pool's whole life.
    ///
    /// **A number to compare with an older reading of itself, not to interpret**
    /// — two readings that differ say the instances a pass draws are not the
    /// ones it drew last time it looked, and two that agree say they are. What
    /// wants that is a consumer whose output is a function of the array and can
    /// be kept instead of recomputed: `ForwardRenderer`'s shadow atlas is the
    /// one there is, and `docs/plan/45-shadows.md`'s static-caching rung is why.
    ///
    /// It moves on **every** write, including ones no draw could see: a rewrite
    /// with identical bytes (which [`set`](Self::set) deliberately does not
    /// filter), a re-point of a skinned object's bases, and the settling of a
    /// `previous_transform` the frame after a move stopped. Reading it as "the
    /// scene changed" is therefore conservative in the safe direction — it says
    /// *changed* more often than a byte comparison would, and never says
    /// *unchanged* when a byte moved.
    ///
    /// It counts writes rather than hashing the array because the array is the
    /// pool's own capacity in bytes and this is read once a frame.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn slot_count(&self) -> u32 {
        self.high_water
    }

    /// Takes a slot, writes `instance` into it **live**, and marks it dirty
    /// everywhere.
    ///
    /// [`GpuInstance::LIVE`] is set whatever `instance.flags` carried — see the
    /// [module docs](self) on why the bit is the pool's rather than the
    /// caller's.
    ///
    /// # Errors
    ///
    /// [`InstancePoolError::PoolFull`] when every slot is in use. The pool never
    /// grows — see the [module docs](self).
    pub fn insert(&mut self, instance: &GpuInstance) -> Result<InstanceHandle, InstancePoolError> {
        // `Pool` hands out a fresh index only when it has no vacant slot, so
        // "live plus permanently retired" is exactly the highest index it could
        // be about to create. Checking before inserting rather than after keeps
        // a refused insert from burning a generation.
        let in_use = self.in_use();
        if in_use >= self.capacity {
            return Err(InstancePoolError::PoolFull {
                capacity: self.capacity,
                in_use,
            });
        }
        let handle: InstanceHandle = self.slots.insert(()).cast();
        self.high_water = self.high_water.max(handle.index() + 1);
        // **A spawn did not travel from anywhere**, so the record is created
        // already at rest: whatever the slot held before belonged to another
        // instance, and carrying it forward would give the new one a motion
        // vector across the gap between them.
        self.write_live(handle.index(), instance, instance.transform);
        Ok(handle)
    }

    /// Rewrites `handle`'s instance and marks it dirty everywhere.
    ///
    /// Returns whether the handle named a live instance; a stale one writes
    /// nothing rather than overwriting whatever took its slot.
    ///
    /// [`GpuInstance::LIVE`] is set, exactly as [`InstancePool::insert`] sets
    /// it: a live instance cannot be made dead by rewriting it, only by
    /// [`InstancePool::remove`].
    ///
    /// A rewrite with the same bytes still marks the slot dirty. Filtering that
    /// would trade a comparison per write for an upload that mostly does not
    /// happen, and it would make "what got uploaded" depend on the old value —
    /// which is exactly the kind of thing a caller cannot reason about. Callers
    /// that know a value did not change do not call this.
    pub fn set(&mut self, handle: InstanceHandle, instance: &GpuInstance) -> bool {
        if !self.slots.contains(handle.cast()) {
            return false;
        }
        // Where it was is what the record already holds, which is why the
        // caller does not pass it — see [`GpuInstance::previous_transform`].
        let previous = self.read(handle.index()).transform;
        self.write_live(handle.index(), instance, previous);
        true
    }

    /// Rewrites `handle`'s two base vertices and **nothing else** — the
    /// previous transform the record already holds included.
    ///
    /// Returns whether the handle named a live instance; a stale one writes
    /// nothing, on [`InstancePool::set`]'s terms. The record keeps
    /// [`GpuInstance::LIVE`], because every path that puts one in the mirror is
    /// a path that set the bit.
    ///
    /// **It exists because a re-point is not a move.**
    /// [`ForwardRenderer::point_skinned_instances`](crate::forward::ForwardRenderer)
    /// rewrites every skinned object's pair of bases once a frame, between the
    /// [`rotate`](Self::rotate) and the upload, and doing that through
    /// [`set`](Self::set) made it a *transform* write: `set` takes the previous
    /// transform from the record it overwrites, and by then that record already
    /// carries this frame's transform, so `previous_transform` came out equal to
    /// `transform` for every skinned instance on every frame. A character that
    /// walked contributed nothing to the motion target from the walk.
    ///
    /// It also leaves the element out of the next [`rotate`](Self::rotate)'s
    /// carry-forward, which is the same reason from the other side: a call that
    /// runs for every skinned object every frame would keep each of them
    /// permanently written-this-frame, nothing would ever be put back at rest,
    /// and one move would be reported for ever. What enrols an element is a
    /// caller's own write — [`insert`](Self::insert) and [`set`](Self::set).
    pub fn set_bases(
        &mut self,
        handle: InstanceHandle,
        base_vertex: u32,
        previous_base_vertex: u32,
    ) -> bool {
        if !self.slots.contains(handle.cast()) {
            return false;
        }
        let index = handle.index();
        let mut record = self.read(index);
        record.base_vertex = base_vertex;
        record.previous_base_vertex = previous_base_vertex;
        self.write(index, &record);
        true
    }

    /// What `handle`'s slot currently holds, decoded from the host mirror, or
    /// `None` if the handle is stale.
    ///
    /// The record carries [`GpuInstance::LIVE`], because the pool put it there:
    /// this is what the buffer holds, not what the caller passed.
    #[must_use]
    pub fn get(&self, handle: InstanceHandle) -> Option<GpuInstance> {
        self.slots
            .contains(handle.cast())
            .then(|| self.read(handle.index()))
    }

    /// Every live element of the array, in slot order.
    ///
    /// **The records the device would draw**, decoded from the same mirror the
    /// buffers are filled from — including
    /// [`GpuInstance::LIVE`](crcbl_shaders::mesh::GpuInstance::LIVE), which the
    /// pool sets, so a caller filters on the bit rather than on a second list
    /// it kept. Slots nothing has written are cleared, and a cleared record has
    /// the bit off.
    ///
    /// Added for `crate::probe_visibility`, which needs the scene as it stands
    /// rather than as it was described: a probe's visibility map is about where
    /// the walls *are*, and the walls arrive through
    /// [`InstancePool::insert`] rather than through the description.
    #[must_use]
    pub fn live(&self) -> Vec<GpuInstance> {
        (0..self.high_water)
            .map(|index| self.read(index))
            .filter(|record| record.flags & GpuInstance::LIVE != 0)
            .collect()
    }

    /// Which element of the array `handle` is, or `None` if the handle is stale.
    ///
    /// This is the number a shader indexes with, which is why it is public: a
    /// draw has to be able to say which instance it draws.
    #[must_use]
    pub fn index(&self, handle: InstanceHandle) -> Option<u32> {
        self.slots.contains(handle.cast()).then(|| handle.index())
    }

    /// Frees `handle`'s slot for reuse, retires the handle, and clears the
    /// element's [`GpuInstance::LIVE`] bit.
    ///
    /// Returns whether the handle named a live instance. The rest of the
    /// element's bytes are left as they are — see the [module docs](self) on
    /// why clearing the bit is the whole of what a removal owes the GPU, and why
    /// zeroing the record would be worse than leaving it.
    ///
    /// The clear reaches the device the way every other write does, through
    /// [`InstancePool::begin_frame`]: the slot is marked dirty in every buffer's
    /// range set, so a removal costs the same one small upload per frame in
    /// flight that a rewrite does.
    pub fn remove(&mut self, handle: InstanceHandle) -> bool {
        if self.slots.remove(handle.cast()).is_none() {
            return false;
        }
        let index = handle.index();
        let mut removed = self.read(index);
        removed.flags &= !GpuInstance::LIVE;
        self.write(index, &removed);
        true
    }

    /// Rotates to the next frame's buffer and writes that buffer's outstanding
    /// changes into it, returning the slot it rotated to.
    ///
    /// **A frame in which nothing was written performs no seam call at all.**
    /// That is §3.2's claim about this design, and it is the thing to check when
    /// wondering whether delta upload is really happening.
    ///
    /// # Errors
    ///
    /// [`HalError`] if a write failed. The runs it had already written stay
    /// written and the rest stay dirty, so a later frame retries them — a
    /// half-uploaded array that forgot the rest would be the worse failure.
    pub fn begin_frame(&mut self, device: &dyn Device) -> Result<usize, HalError> {
        let slot = self.rotate();
        self.flush(device)?;
        Ok(slot)
    }

    /// Rotates to the next frame's buffer **without uploading anything**, and
    /// returns the slot it rotated to.
    ///
    /// The half of [`begin_frame`](Self::begin_frame) that decides which buffer
    /// this frame writes, split out for the one caller that has an instance to
    /// write which *depends* on the slot:
    /// [`ForwardRenderer::begin_skinned_frame`](crate::forward::ForwardRenderer::begin_skinned_frame)
    /// points every skinned instance at the half of its region this frame's
    /// dispatch will fill, and the parity it reads comes from a
    /// [`Skinning`](crate::skinning::Skinning) that has to be handed this slot
    /// first. Rotating and uploading in one call would leave that write for the
    /// *next* frame's upload, which is a character posed one frame late.
    ///
    /// [`flush`](Self::flush) must follow it, or this frame draws whatever the
    /// frame before last left in this buffer.
    ///
    /// **This is also the frame boundary the previous transform is measured
    /// against**: an element the frame before last wrote and this one did not
    /// is put back at rest here, so an instance that stopped moving stops
    /// reporting a move. See [`GpuInstance::previous_transform`] and the
    /// [module docs](self).
    pub fn rotate(&mut self) -> usize {
        self.carry_forward();
        self.frame = (self.frame + 1) % self.buffers.len();
        self.frame
    }

    /// Writes the current slot's outstanding changes into its buffer.
    ///
    /// The other half of [`begin_frame`](Self::begin_frame). Idempotent: a
    /// second call with nothing newly written performs no seam call at all.
    ///
    /// # Errors
    ///
    /// [`HalError`] if a write failed, on [`begin_frame`](Self::begin_frame)'s
    /// terms — the runs already written stay written and the rest stay dirty.
    pub fn flush(&mut self, device: &dyn Device) -> Result<(), HalError> {
        let slot = self.frame;
        let buffer = self.buffers[slot];
        // Detached so the mirror can be borrowed while the runs are walked.
        let runs = core::mem::take(&mut self.dirty[slot].runs);
        for (done, run) in runs.iter().enumerate() {
            let at = run.start as usize * INSTANCE_STRIDE;
            let end = run.end as usize * INSTANCE_STRIDE;
            if let Err(error) = device.write_buffer(buffer, at as u64, &self.mirror[at..end]) {
                self.dirty[slot].runs = runs[done..].to_vec();
                return Err(error);
            }
        }
        Ok(())
    }

    /// Releases every buffer. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        for buffer in self.buffers {
            device.destroy_buffer(buffer);
        }
    }

    /// Slots that cannot be handed out: live ones, plus any permanently
    /// withdrawn by generation exhaustion.
    fn in_use(&self) -> u32 {
        let used = self.slots.len() + self.slots.retired_slots();
        u32::try_from(used).unwrap_or(u32::MAX)
    }

    /// What the mirror holds at `index`, live or not.
    ///
    /// Decoded rather than kept beside the bytes, for the reason the mirror is
    /// bytes at all: a second copy of an instance is a second thing to drift
    /// from the one the buffer gets.
    fn read(&self, index: u32) -> GpuInstance {
        let at = index as usize * INSTANCE_STRIDE;
        let bytes: &[u8; INSTANCE_STRIDE] = self.mirror[at..at + INSTANCE_STRIDE]
            .try_into()
            .unwrap_or_else(|_| unreachable!("the mirror is capacity × INSTANCE_STRIDE"));
        GpuInstance::from_bytes(bytes)
    }

    /// [`InstancePool::write`] with [`GpuInstance::LIVE`] forced on and
    /// `previous_transform` set to `previous`, which is what every
    /// caller-supplied record goes through.
    ///
    /// Both fields are the pool's rather than the caller's, for the same
    /// reason: the pool is what knows the answer — that the record is live, and
    /// what it held a moment ago — and a caller asked for either would be
    /// keeping a second copy of what this one already has.
    ///
    /// It also enrols the element in the carry-forward the next
    /// [`InstancePool::rotate`] performs, which is what stops a moving instance
    /// that stopped from reporting the move it made forever.
    fn write_live(&mut self, index: u32, instance: &GpuInstance, previous: [f32; 16]) {
        self.write(
            index,
            &GpuInstance {
                flags: instance.flags | GpuInstance::LIVE,
                previous_transform: previous,
                ..*instance
            },
        );
        if !self.moved_mark[index as usize] {
            self.moved_mark[index as usize] = true;
            self.written_this_frame.push(index);
        }
    }

    /// Puts every element a caller wrote last frame at rest: its
    /// `previous_transform` becomes its `transform`, so a frame in which it is
    /// not written again reports no motion.
    ///
    /// **This is the half that makes the field mean "last frame" rather than
    /// "the last time anyone touched it".** Without it, an instance that moved
    /// once and then stood still would go on carrying the transform it moved
    /// from, and every temporal consumer would go on smearing it along a
    /// journey that finished.
    ///
    /// **It carries forward the frame *before* the one being assembled**, and
    /// that is the whole subtlety. A caller writes the instances for frame `n`
    /// and *then* calls [`InstancePool::begin_frame`], so by the time this runs
    /// the moves of frame `n` have already happened and each already carries
    /// where it came from — putting those at rest would erase the move before
    /// it was ever drawn. What is left to settle is frame `n - 1`'s writes that
    /// frame `n` did not repeat, which is exactly "it moved, and then it
    /// stopped".
    ///
    /// A record already at rest is skipped, so the cost of standing still is
    /// one comparison in the frame after the last move and nothing after that.
    /// The write goes through [`InstancePool::write`] rather than
    /// [`InstancePool::write_live`]: it must not enrol the element again, or a
    /// pool that had ever moved anything would re-examine it every frame
    /// forever.
    fn carry_forward(&mut self) {
        for index in core::mem::take(&mut self.written_last_frame) {
            if self.moved_mark[index as usize] {
                // Written again for the frame about to be drawn, so it is
                // moving rather than stopping and its own write said where
                // from.
                continue;
            }
            let mut record = self.read(index);
            if record.previous_transform == record.transform {
                continue;
            }
            record.previous_transform = record.transform;
            self.write(index, &record);
        }
        self.written_last_frame = core::mem::take(&mut self.written_this_frame);
        for index in &self.written_last_frame {
            self.moved_mark[*index as usize] = false;
        }
    }

    /// Puts `instance` into the mirror at `index` and marks it dirty in every
    /// slot's range set.
    ///
    /// **The only thing that ever changes the mirror**, which is what makes
    /// [`revision`](Self::revision) a total answer rather than a count of the
    /// paths somebody remembered to instrument.
    fn write(&mut self, index: u32, instance: &GpuInstance) {
        let at = index as usize * INSTANCE_STRIDE;
        self.mirror[at..at + INSTANCE_STRIDE].copy_from_slice(&instance.to_bytes());
        self.revision = self.revision.wrapping_add(1);
        for dirty in &mut self.dirty {
            dirty.mark(index);
        }
    }
}

/// A sorted set of half-open element runs that never overlap and never touch.
///
/// "Never touch" is the load-bearing half: it is what makes marking 3, 4 and 5
/// one run rather than three, and it is maintained by [`DirtyRanges::mark`]
/// extending and merging rather than appending. A list that only appended would
/// produce one `write_buffer` per changed element, which is the cost §3.2's
/// delta upload exists to avoid — it would still be correct, and it would still
/// pass any test that only checked the bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DirtyRanges {
    runs: Vec<Range<u32>>,
}

impl DirtyRanges {
    /// Marks one element dirty, extending or merging the runs around it.
    ///
    /// Idempotent: marking an element already inside a run changes nothing, so
    /// a value written twice between uploads is uploaded once.
    ///
    /// `index` must be below `u32::MAX`, which an [`InstancePool`] index is:
    /// it is below a capacity that is itself a `u32`.
    fn mark(&mut self, index: u32) {
        // The first run that could contain or abut `index` from the left.
        // Everything before it ends strictly below `index`, so nothing there
        // can touch what this leaves behind.
        let at = self.runs.partition_point(|run| run.end < index);
        // Inside the run, or exactly one past its end.
        if self.runs.get(at).is_some_and(|run| run.start <= index) {
            if self.runs[at].end == index {
                self.runs[at].end = index + 1;
                // Extending may have closed the gap to the next run, which is
                // the case that turns "3 and 5, then 4" into one run.
                if self
                    .runs
                    .get(at + 1)
                    .is_some_and(|next| next.start == index + 1)
                {
                    let end = self.runs.remove(at + 1).end;
                    self.runs[at].end = end;
                }
            }
            return;
        }
        // Exactly one before the run. It cannot now touch the run before it:
        // that one ends strictly below `index`.
        if self.runs.get(at).is_some_and(|run| run.start == index + 1) {
            self.runs[at].start = index;
            return;
        }
        // A gap on both sides, or past the last run.
        self.runs.insert(at, index..index + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::{Event, NullInstance, Recorder};
    use crcbl_hal::{DeviceDesc, Instance};

    const FRAMES: usize = 2;

    fn open() -> (Recorder, Box<dyn Device>) {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        (recorder, device)
    }

    fn pool(device: &dyn Device, capacity: u32) -> InstancePool {
        InstancePool::new(
            device,
            &InstancePoolDesc {
                label: Some("test"),
                capacity,
                frames_in_flight: FRAMES,
            },
        )
        .expect("the null backend accepts every descriptor")
    }

    /// An instance whose transform is distinguishable from every other `n`, as
    /// a **caller** hands it over.
    ///
    /// Its flags do not carry [`GpuInstance::LIVE`] for the `n` the tests below
    /// use, which is what makes [`stored`] a different value and the pool's
    /// forcing of the bit observable.
    fn instance(n: u32) -> GpuInstance {
        GpuInstance {
            transform: [n as f32; 16],
            // The pool overwrites this, and a value here that the pool would
            // also have chosen could not tell that apart from it being passed
            // through. Every `n` the tests use is non-zero, so this is a
            // transform no caller record carries.
            previous_transform: [0.0; 16],
            mesh: n,
            material: n + 1,
            sector: n + 2,
            flags: n + 3,
            base_vertex: n + 4,
            previous_base_vertex: n + 5,
        }
    }

    /// [`instance`] as the pool stores it when it is **inserted**: the
    /// caller's record, live, and at rest.
    ///
    /// Refuses an `n` whose flags happen to carry the bit already, because a
    /// comparison against such a record would pass whether or not the pool set
    /// it — which is the one way an assertion about the bit can be vacuous. The
    /// same refusal covers the previous transform: `n` is non-zero for every
    /// caller this module builds, so `instance(n)`'s zeroed one is a value the
    /// pool has to have replaced.
    fn stored(n: u32) -> GpuInstance {
        let caller = instance(n);
        assert_eq!(
            caller.flags & GpuInstance::LIVE,
            0,
            "instance({n}) already carries the live bit; pick another n"
        );
        assert_ne!(
            n, 0,
            "instance(0)'s transform is its own zeroed previous one"
        );
        GpuInstance {
            flags: caller.flags | GpuInstance::LIVE,
            previous_transform: caller.transform,
            ..caller
        }
    }

    /// [`stored`] as the pool holds it after a **rewrite** from `was`: live,
    /// and carrying where it came from.
    fn stored_after(n: u32, was: u32) -> GpuInstance {
        GpuInstance {
            previous_transform: instance(was).transform,
            ..stored(n)
        }
    }

    /// Element `index` of the buffer at `slot`, decoded from the bytes that
    /// actually reached the device.
    ///
    /// Read back rather than taken from the pool's mirror, so an assertion is
    /// evidence about what a shader would read and not about the pool's
    /// bookkeeping agreeing with itself.
    fn on_device(recorder: &Recorder, pool: &InstancePool, slot: usize, index: u32) -> GpuInstance {
        let bytes = recorder
            .buffer_bytes(pool.buffers()[slot])
            .expect("the buffer is live");
        let at = index as usize * INSTANCE_STRIDE;
        GpuInstance::from_bytes(
            bytes[at..at + INSTANCE_STRIDE]
                .try_into()
                .expect("one element"),
        )
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

    /// Drives `frames` uploads, discarding what they wrote — used to reach the
    /// steady state where every slot has caught up.
    fn settle(pool: &mut InstancePool, device: &dyn Device) {
        for _ in 0..FRAMES {
            pool.begin_frame(device).expect("the null device writes");
        }
    }

    /// A dirty set's runs as `(start, end)` pairs, which is what the assertions
    /// below compare — a bare `Range` literal in an array reads as a range the
    /// author meant to iterate, and `clippy` says so.
    fn runs(dirty: &DirtyRanges) -> Vec<(u32, u32)> {
        dirty.runs.iter().map(|run| (run.start, run.end)).collect()
    }

    // --- the coalescing, which is pure logic ---

    /// **The headline claim.** Adjacent marks are one run; separated marks are
    /// two. A list that appended would report three runs for the first case,
    /// which is one `write_buffer` per instance and the whole thing this design
    /// exists to avoid.
    #[test]
    fn adjacent_marks_coalesce_and_separated_ones_do_not() {
        let mut dirty = DirtyRanges::default();
        for index in [3u32, 4, 5] {
            dirty.mark(index);
        }
        assert_eq!(runs(&dirty), [(3, 6)], "3, 4 and 5 are one run of three");

        let mut dirty = DirtyRanges::default();
        dirty.mark(3);
        dirty.mark(900);
        assert_eq!(runs(&dirty), [(3, 4), (900, 901)], "a gap stays a gap");
    }

    /// The exact boundary, in both directions: two marks that **touch** are one
    /// run, two marks one element apart are two. Off by one either way and the
    /// first case fragments or the second merges across a gap it must not.
    #[test]
    fn the_boundary_between_touching_and_separated_is_one_element() {
        // `4` touches: 3..4 and 4..5 share an endpoint. `5` leaves one clear
        // element between them.
        for (second, expected) in [(4u32, &[(3, 5)][..]), (5, &[(3, 4), (5, 6)][..])] {
            let mut dirty = DirtyRanges::default();
            dirty.mark(3);
            dirty.mark(second);
            assert_eq!(runs(&dirty), expected, "marking 3 then {second}");
        }
        // And from the other side, because extending a run backwards is a
        // different branch of `mark` than extending it forwards.
        for (first, expected) in [(4u32, &[(3, 5)][..]), (5, &[(3, 4), (5, 6)][..])] {
            let mut dirty = DirtyRanges::default();
            dirty.mark(first);
            dirty.mark(3);
            assert_eq!(runs(&dirty), expected, "marking {first} then 3");
        }
    }

    /// Marking the element between two runs merges all three. Without the merge
    /// after an extend, this stays two runs that touch — which breaks the
    /// invariant everything else assumes and costs an upload forever after.
    #[test]
    fn closing_the_gap_between_two_runs_merges_them() {
        let mut dirty = DirtyRanges::default();
        dirty.mark(3);
        dirty.mark(5);
        assert_eq!(runs(&dirty), [(3, 4), (5, 6)]);
        dirty.mark(4);
        assert_eq!(runs(&dirty), [(3, 6)], "one run of three, not two touching");
    }

    /// Order does not matter: every permutation of the same marks produces the
    /// same runs. Coalescing that only worked left-to-right would pass the
    /// tests above and fail the moment a caller wrote its instances in any
    /// other order.
    #[test]
    fn coalescing_does_not_depend_on_the_order_marks_arrive_in() {
        let marks = [7u32, 3, 5, 4, 900, 6];
        let mut permuted = marks;
        // Every rotation is enough to move each element through every position.
        for shift in 0..marks.len() {
            permuted.rotate_left(shift.min(1));
            let mut dirty = DirtyRanges::default();
            for index in permuted {
                dirty.mark(index);
            }
            assert_eq!(
                runs(&dirty),
                [(3, 8), (900, 901)],
                "the order {permuted:?} produced different runs"
            );
        }
    }

    /// Marking the same element twice is one run of one, so a value rewritten
    /// between uploads is uploaded once.
    #[test]
    fn marking_the_same_element_twice_changes_nothing() {
        let mut dirty = DirtyRanges::default();
        dirty.mark(3);
        dirty.mark(3);
        assert_eq!(runs(&dirty), [(3, 4)]);
        // And re-marking inside an existing run leaves it alone.
        dirty.mark(4);
        dirty.mark(5);
        dirty.mark(4);
        assert_eq!(runs(&dirty), [(3, 6)]);
    }

    // --- the pool ---

    /// **The other headline claim: an unchanged frame uploads nothing.**
    ///
    /// Asserted on the recorded seam calls rather than on a "dirty" flag,
    /// because a flag is a claim about the pool's bookkeeping and this is a
    /// claim about what reached the device.
    #[test]
    fn a_frame_with_no_writes_uploads_nothing() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 16);
        pool.insert(&instance(1)).expect("room");
        settle(&mut pool, device.as_ref());

        let before = writes(&recorder).len();
        assert!(before > 0, "the insert must have reached the device at all");
        for _ in 0..FRAMES * 3 {
            pool.begin_frame(device.as_ref()).expect("writes");
        }
        assert_eq!(
            writes(&recorder).len(),
            before,
            "six frames with nothing written must perform no seam call"
        );

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **Every path that changes an instance moves the revision, and a frame
    /// that changed nothing leaves it alone.**
    ///
    /// [`InstancePool::revision`]'s whole contract, and the thing a consumer
    /// keeping something derived from the array — `ForwardRenderer`'s shadow
    /// atlas — bets a frame's correctness on. The list is deliberately every
    /// mutator this type has rather than a sample: a path that changed the
    /// mirror without moving the number would be a caster that moved under a
    /// shadow map nothing redrew.
    #[test]
    fn every_write_moves_the_revision_and_a_still_frame_does_not() {
        let (recorder, device) = open();
        let device = device.as_ref();
        let mut pool = pool(device, 16);
        let start = pool.revision();

        let first = pool.insert(&instance(1)).expect("room");
        let inserted = pool.revision();
        assert!(inserted > start, "an insert did not move the revision");

        assert!(pool.set(first, &instance(2)), "the handle is live");
        let written = pool.revision();
        assert!(written > inserted, "a rewrite did not move the revision");

        assert!(
            pool.set(first, &instance(2)),
            "a rewrite with the same bytes is still a write — see `InstancePool::set`"
        );
        let repeated = pool.revision();
        assert!(
            repeated > written,
            "a rewrite the pool does not filter must not be filtered here either"
        );

        assert!(pool.set_bases(first, 4, 8), "the handle is live");
        let repointed = pool.revision();
        assert!(repointed > repeated, "a re-point did not move the revision");

        // A move to somewhere the record was not, so the settle below has a
        // `previous_transform` to put back: rewriting an instance with the
        // bytes it already holds leaves the two equal, and the carry-forward
        // skips a record already at rest.
        assert!(pool.set(first, &instance(3)), "the handle is live");
        let moved = pool.revision();
        assert!(moved > repointed, "a move did not move the revision");

        // The frame after a move settles the record's `previous_transform`,
        // which is a write and so a change this number reports. Conservative in
        // the safe direction, and the reason the doc says so.
        settle(&mut pool, device);
        let settled = pool.revision();
        assert!(
            settled > moved,
            "the carry-forward wrote the mirror without saying so"
        );

        // And now the pool is quiet: nothing was written, so nothing moves,
        // however many frames go by.
        for _ in 0..FRAMES * 3 {
            pool.begin_frame(device).expect("the null device writes");
        }
        assert_eq!(
            pool.revision(),
            settled,
            "a run of frames nobody wrote in moved the revision, so nothing derived from this \
             array could ever be kept"
        );

        assert!(pool.remove(first), "the handle is live");
        assert!(
            pool.revision() > settled,
            "a removal did not move the revision, so an instance that left the scene would go \
             on casting a shadow"
        );

        pool.destroy(device);
        recorder.assert_valid();
    }

    /// Three adjacent instances written in one frame reach the device as **one**
    /// write of three elements, and two separated ones as two writes of one.
    /// This is the coalescing test above, seen from the device's side.
    #[test]
    fn adjacent_writes_reach_the_device_as_one_call() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 1024);
        let handles: Vec<_> = (0..6)
            .map(|n| pool.insert(&instance(n)).expect("room"))
            .collect();
        settle(&mut pool, device.as_ref());

        let before = writes(&recorder).len();
        for handle in &handles[3..6] {
            pool.set(*handle, &instance(99));
        }
        pool.begin_frame(device.as_ref()).expect("writes");
        assert_eq!(
            writes(&recorder)[before..],
            [(3 * INSTANCE_STRIDE as u64, 3 * INSTANCE_STRIDE)],
            "instances 3, 4 and 5 must be one write of three elements"
        );

        // Drained, so the next measurement is one slot's own runs rather than
        // that slot's leftovers from the write above — each slot lags the
        // others by a frame, which is the ring doing its job.
        settle(&mut pool, device.as_ref());
        let before = writes(&recorder).len();
        pool.set(handles[0], &instance(98));
        pool.set(handles[5], &instance(98));
        pool.begin_frame(device.as_ref()).expect("writes");
        assert_eq!(
            writes(&recorder)[before..],
            [
                (0, INSTANCE_STRIDE),
                (5 * INSTANCE_STRIDE as u64, INSTANCE_STRIDE)
            ],
            "instances 0 and 5 are not adjacent and must be two writes"
        );

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A write reaches **every** buffer in the ring, not just the one that
    /// happened to be current: each slot is a separate copy, and a slot that
    /// missed an update would draw a stale instance every other frame.
    #[test]
    fn a_write_reaches_every_buffer_in_the_ring() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 16);
        let handle = pool.insert(&instance(1)).expect("room");
        settle(&mut pool, device.as_ref());

        pool.set(handle, &instance(2));
        let mut written_to = Vec::new();
        for _ in 0..FRAMES {
            let slot = pool.begin_frame(device.as_ref()).expect("writes");
            written_to.push(pool.buffers()[slot]);
        }
        let targets: Vec<BufferHandle> = recorder
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::BufferWritten { buffer, .. } => Some(buffer),
                _ => None,
            })
            .collect();
        for buffer in pool.buffers() {
            assert!(
                targets.contains(buffer),
                "buffer {buffer:?} never received the instance"
            );
        }
        assert_eq!(written_to.len(), FRAMES);

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// A full pool refuses by name, and takes nothing: the refused insert must
    /// not consume a slot, or a caller that retries would lose capacity each
    /// time it was told there was none.
    #[test]
    fn a_full_pool_fails_by_name_and_takes_nothing() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 2);
        let first = pool.insert(&instance(0)).expect("room");
        pool.insert(&instance(1)).expect("room");

        let error = pool
            .insert(&instance(2))
            .expect_err("two instances fill a pool of two");
        assert!(
            matches!(
                error,
                InstancePoolError::PoolFull {
                    capacity: 2,
                    in_use: 2
                }
            ),
            "got: {error:?}"
        );
        assert_eq!(pool.len(), 2, "a refused insert must not consume a slot");

        // And a freed slot is handed out again, so the refusal above is the
        // capacity rather than the pool having given up.
        assert!(pool.remove(first));
        let reused = pool.insert(&instance(3)).expect("the freed slot");
        assert_eq!(pool.index(reused), Some(0), "the low slot comes back");
        assert_eq!(pool.index(first), None, "and the old handle is retired");

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// The number a cull dispatch is sized by covers every occupied slot, and
    /// **it does not shrink when one is freed**.
    ///
    /// That is the whole difference from [`InstancePool::len`]: remove the
    /// middle instance of three and `len` says two, while the third is still at
    /// element 2 and a walk of `0..2` would never test it. A dispatch sized that
    /// way drops an instance from the picture and reports it as culled.
    #[test]
    fn the_slot_count_covers_every_occupied_element_and_never_shrinks() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 8);
        assert_eq!(pool.slot_count(), 0, "an empty pool has nothing to walk");

        let handles: Vec<InstanceHandle> = (0..3)
            .map(|index| pool.insert(&instance(index)).expect("room"))
            .collect();
        assert_eq!(pool.slot_count(), 3);

        assert!(pool.remove(handles[1]));
        assert_eq!(pool.len(), 2, "two are live");
        assert_eq!(
            pool.slot_count(),
            3,
            "and the highest is still at element 2, so a walk must still reach it"
        );

        // A reused slot does not raise it either: the element was already
        // covered.
        pool.insert(&instance(9)).expect("the freed slot");
        assert_eq!(pool.slot_count(), 3);

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// One buffer per frame in flight, each sized for the whole capacity — the
    /// ring is what the module docs claim it is, and a pool that quietly kept
    /// one buffer would pass every other test here.
    #[test]
    fn the_ring_is_one_buffer_per_frame_in_flight() {
        let (recorder, device) = open();
        let pool = pool(device.as_ref(), 16);
        assert_eq!(pool.buffers().len(), FRAMES);
        let mut distinct = pool.buffers().to_vec();
        distinct.sort_unstable_by_key(|buffer| buffer.to_bits());
        distinct.dedup();
        assert_eq!(distinct.len(), FRAMES, "the slots must not share a buffer");
        assert_eq!(pool.capacity(), 16);
        assert!(pool.is_empty());

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **Every buffer is cleared when the pool is created**, which is the other
    /// half of the removal contract: the cull pass walks from element zero, so
    /// the slots above the ones in use have to answer too, and a zeroed element
    /// is a dead instance.
    ///
    /// Asserted on the seam calls rather than on the bytes, and that is the
    /// only assertion available here: this recorder zero-fills a mappable
    /// buffer at creation, so a byte comparison would pass whether or not the
    /// pool ever wrote one. What a real driver leaves in a fresh host-visible
    /// allocation is its own business, which is why `crcbl-vk`'s `cull` e2e
    /// dispatches over a pool's whole capacity with only three slots inserted.
    #[test]
    fn every_buffer_is_cleared_when_the_pool_is_created() {
        let (recorder, device) = open();
        let pool = pool(device.as_ref(), 16);
        assert_eq!(
            writes(&recorder),
            vec![(0, 16 * INSTANCE_STRIDE); FRAMES],
            "each buffer in the ring must be cleared, whole, before anything reads it"
        );

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **Where an instance was last frame, across the three cases that decide
    /// it**: a spawn is at rest, a move carries where it moved from, and a
    /// frame with no move puts it back at rest.
    ///
    /// The third is the one the carry-forward exists for and the one a pool
    /// without it would fail: `set` alone leaves the record naming the
    /// transform it left, so an instance that moved once and then stood still
    /// would report that move on every frame afterwards, forever.
    ///
    /// The order is a frame's real one — write the instances, *then*
    /// `begin_frame` — because that is what decides which frame a move belongs
    /// to.
    ///
    /// Read back off the device rather than out of the mirror: this is what a
    /// shader would fetch.
    #[test]
    fn an_instance_carries_where_it_was_last_frame() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 16);
        let handle = pool.insert(&instance(7)).expect("room");
        settle(&mut pool, device.as_ref());
        let index = pool.index(handle).expect("live");
        let slot = pool.frame;
        assert_eq!(
            on_device(&recorder, &pool, slot, index).previous_transform,
            instance(7).transform,
            "a spawn did not travel from anywhere"
        );

        // The frame it moves: the record names both ends of the move.
        assert!(pool.set(handle, &instance(11)));
        let slot = pool
            .begin_frame(device.as_ref())
            .expect("the null device writes");
        let moved = on_device(&recorder, &pool, slot, index);
        assert_eq!(moved.transform, instance(11).transform, "it arrived");
        assert_eq!(
            moved.previous_transform,
            instance(7).transform,
            "and it says where from"
        );

        // The next frame, with nothing written: back at rest.
        let slot = pool
            .begin_frame(device.as_ref())
            .expect("the null device writes");
        let stopped = on_device(&recorder, &pool, slot, index);
        assert_eq!(
            stopped.transform,
            instance(11).transform,
            "it has not moved"
        );
        assert_eq!(
            stopped.previous_transform, stopped.transform,
            "so it must report no motion"
        );

        // And it stays at rest without being written again, in every buffer of
        // the ring rather than only the one the last frame rotated to.
        settle(&mut pool, device.as_ref());
        for slot in 0..FRAMES {
            let resting = on_device(&recorder, &pool, slot, index);
            assert_eq!(
                resting.previous_transform, resting.transform,
                "buffer {slot} still has it moving"
            );
        }

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **An instance that keeps moving is never put at rest.** The
    /// carry-forward runs a frame behind, so the case it must not touch is the
    /// one where the caller wrote the instance again — settling that would
    /// erase a move before the frame holding it was ever drawn.
    #[test]
    fn an_instance_still_moving_is_not_put_at_rest() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 16);
        let handle = pool.insert(&instance(1)).expect("room");
        settle(&mut pool, device.as_ref());
        let index = pool.index(handle).expect("live");

        for step in 2..6u32 {
            assert!(pool.set(handle, &instance(step)));
            let slot = pool
                .begin_frame(device.as_ref())
                .expect("the null device writes");
            let moving = on_device(&recorder, &pool, slot, index);
            assert_eq!(
                (moving.previous_transform, moving.transform),
                (instance(step - 1).transform, instance(step).transform),
                "the frame that moved it to {step} lost one end of the move"
            );
        }

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **A re-point rewrites the two bases and leaves the move alone**, which
    /// is the whole of why [`InstancePool::set_bases`] exists.
    ///
    /// The record is moved first, so it is carrying a previous transform that
    /// differs from its current one when the bases are rewritten — a call that
    /// went through [`InstancePool::set`] would take the previous transform
    /// from the record it overwrites, which by then holds the move's *end*, and
    /// the move would be gone. That is what
    /// [`crate::forward::ForwardRenderer`]'s per-frame re-point did to every
    /// skinned instance.
    ///
    /// The frame checked on the device is the one the move is drawn in. The
    /// frame after it puts the record back at rest, which is
    /// [`carry_forward`](InstancePool::carry_forward)'s job rather than this
    /// call's — [`an_instance_carries_where_it_was_last_frame`] is where that
    /// half is asserted, and the renderer-level
    /// `a_skinned_instance_that_moves_carries_the_transform_it_moved_from` is
    /// where the two meet.
    ///
    /// The stale half is [`InstancePool::set`]'s contract: `false`, and nothing
    /// written — asserted on the bytes that reached the device and on the
    /// uploads a later frame performs, because a mirror written without being
    /// marked dirty would satisfy only one of the two.
    #[test]
    fn setting_the_bases_keeps_the_transform_the_record_moved_from() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 16);
        let handle = pool.insert(&instance(7)).expect("room");
        assert!(pool.set(handle, &instance(11)), "the handle is live");

        assert!(pool.set_bases(handle, 40, 41), "the handle is still live");
        let repointed = GpuInstance {
            base_vertex: 40,
            previous_base_vertex: 41,
            ..stored_after(11, 7)
        };
        assert_eq!(
            pool.get(handle),
            Some(repointed),
            "a re-point owes the two bases it was given and every other field the record \
             already held — the previous transform above all"
        );

        let index = pool.index(handle).expect("live");
        let slot = pool
            .begin_frame(device.as_ref())
            .expect("the null device writes");
        assert_eq!(
            on_device(&recorder, &pool, slot, index),
            repointed,
            "the buffer this frame draws from did not take the re-point, so a shader reads \
             the bases of the frame before"
        );

        assert!(pool.remove(handle));
        for _ in 0..FRAMES * 2 {
            pool.begin_frame(device.as_ref())
                .expect("the null device writes");
        }
        recorder.clear();
        assert!(
            !pool.set_bases(handle, 60, 61),
            "a retired handle names no instance"
        );
        for _ in 0..FRAMES * 2 {
            pool.begin_frame(device.as_ref())
                .expect("the null device writes");
        }
        assert_eq!(
            writes(&recorder),
            [],
            "a stale re-point marked a slot dirty, so it wrote into whatever took it"
        );
        for slot in 0..FRAMES {
            let dead = on_device(&recorder, &pool, slot, index);
            assert_eq!(
                (dead.base_vertex, dead.previous_base_vertex),
                (40, 41),
                "buffer {slot} took a stale handle's bases"
            );
        }

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **Standing still costs one upload per buffer and then nothing.** That is
    /// the claim delta upload is here to make, and an unconditional
    /// carry-forward would break it — every instance would be dirty in every
    /// frame forever.
    ///
    /// The first assertion is what keeps the second from being vacuous: a
    /// carry-forward that never reached the device would also produce no
    /// uploads at rest.
    #[test]
    fn an_instance_at_rest_is_uploaded_once_and_then_never() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 16);
        let handle = pool.insert(&instance(7)).expect("room");
        settle(&mut pool, device.as_ref());

        assert!(pool.set(handle, &instance(11)));
        settle(&mut pool, device.as_ref());

        // The move has reached every buffer and the last of those frames
        // carried it forward, which marked the element dirty everywhere again.
        recorder.clear();
        pool.begin_frame(device.as_ref())
            .expect("the null device writes");
        let carried = writes(&recorder).len();
        assert_eq!(
            carried, 1,
            "the buffer that had not seen the carry-forward yet should have taken it, \
             and {carried} write(s) says the carry-forward never reached the device"
        );

        recorder.clear();
        for _ in 0..FRAMES * 2 {
            pool.begin_frame(device.as_ref())
                .expect("the null device writes");
        }
        assert_eq!(
            writes(&recorder),
            [],
            "an instance nobody has written costs no upload"
        );

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// What goes in comes out, field for field, and a removed handle stops
    /// resolving. The four ids are the same width and would permute silently,
    /// so the round trip is checked rather than assumed.
    #[test]
    fn an_instance_round_trips_through_the_pool() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 16);
        let handle = pool.insert(&instance(7)).expect("room");
        assert_eq!(pool.get(handle), Some(stored(7)));

        assert!(pool.set(handle, &instance(11)));
        assert_eq!(
            pool.get(handle),
            Some(stored_after(11, 7)),
            "a rewrite keeps where the instance was"
        );

        assert!(pool.remove(handle));
        assert_eq!(
            pool.get(handle),
            None,
            "a removed handle resolves to nothing"
        );
        assert!(!pool.set(handle, &instance(12)), "and cannot be written");
        assert!(!pool.remove(handle), "and cannot be removed twice");

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **Removing an instance clears its live bit on the device, and changes
    /// nothing else about the record.**
    ///
    /// The whole slice: a slot the cull pass walks past has to be able to say
    /// it is free, and the bit is the only thing a removal is allowed to
    /// rewrite. Asserted on the bytes in the buffer rather than on the mirror,
    /// because a removal that never reached a buffer is a removal the GPU does
    /// not know about.
    #[test]
    fn removing_an_instance_clears_its_live_bit_on_the_device() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 16);
        let first = pool.insert(&instance(1)).expect("room");
        let second = pool.insert(&instance(3)).expect("room");
        settle(&mut pool, device.as_ref());
        for slot in 0..FRAMES {
            assert_eq!(
                on_device(&recorder, &pool, slot, 0),
                stored(1),
                "buffer {slot} did not receive the live instance"
            );
        }

        assert!(pool.remove(first));
        settle(&mut pool, device.as_ref());
        for slot in 0..FRAMES {
            let removed = on_device(&recorder, &pool, slot, 0);
            assert_eq!(
                removed.flags & GpuInstance::LIVE,
                0,
                "buffer {slot} still holds a live instance at the removed slot: {removed:?}"
            );
            // And the rest of the record is exactly what it was. The pool
            // deliberately does not zero it — see the module docs — so a test
            // that only checked the bit would pass on an implementation that
            // wrote a degenerate instance at the origin instead.
            assert_eq!(
                removed,
                GpuInstance {
                    flags: stored(1).flags & !GpuInstance::LIVE,
                    ..stored(1)
                },
                "buffer {slot} rewrote more than the flags word"
            );
            assert_eq!(
                on_device(&recorder, &pool, slot, 1),
                stored(3),
                "buffer {slot} disturbed the instance beside the removed one"
            );
        }

        assert_eq!(
            pool.get(second),
            Some(stored(3)),
            "and the other handle still resolves"
        );

        // A second removal of the retired handle writes nothing at all, so a
        // caller cannot clear the live bit of whatever has since taken the slot.
        let before = writes(&recorder).len();
        assert!(!pool.remove(first));
        settle(&mut pool, device.as_ref());
        assert_eq!(
            writes(&recorder).len(),
            before,
            "removing a retired handle must upload nothing"
        );

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// **A slot reused after a removal is live again, holding its new
    /// contents.** The other direction of the same defect: a liveness bit that
    /// stuck would make the pool leak capacity that looks allocated and draws
    /// nothing.
    #[test]
    fn a_reused_slot_is_live_again_with_its_new_contents() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 16);
        let first = pool.insert(&instance(1)).expect("room");
        assert!(pool.remove(first));
        settle(&mut pool, device.as_ref());

        let reused = pool.insert(&instance(5)).expect("the freed slot");
        assert_eq!(pool.index(reused), Some(0), "the low slot comes back");
        settle(&mut pool, device.as_ref());
        for slot in 0..FRAMES {
            assert_eq!(
                on_device(&recorder, &pool, slot, 0),
                stored(5),
                "buffer {slot} holds neither the new instance nor the live bit"
            );
        }
        assert_eq!(pool.get(reused), Some(stored(5)));

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// The bytes that reach the device are the instance's, at the offset its
    /// index names — checked by writing a high slot and reading back the offset
    /// the seam was given. An off-by-one in the element/byte conversion would
    /// put every instance one slot out and still upload the right number of
    /// bytes.
    #[test]
    fn an_instances_offset_is_its_index_times_the_stride() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 16);
        for n in 0..5 {
            pool.insert(&instance(n)).expect("room");
        }
        settle(&mut pool, device.as_ref());
        let handle = pool.insert(&instance(5)).expect("room");
        assert_eq!(pool.index(handle), Some(5));

        let before = writes(&recorder).len();
        pool.begin_frame(device.as_ref()).expect("writes");
        assert_eq!(
            writes(&recorder)[before..],
            [(5 * INSTANCE_STRIDE as u64, INSTANCE_STRIDE)]
        );

        pool.destroy(device.as_ref());
        recorder.assert_valid();
    }

    /// Everything created is destroyed.
    #[test]
    fn a_pool_leaks_nothing() {
        let (recorder, device) = open();
        let before = recorder.total_live_objects();
        let mut pool = pool(device.as_ref(), 16);
        pool.insert(&instance(0)).expect("room");
        settle(&mut pool, device.as_ref());
        assert!(recorder.total_live_objects() > before);

        pool.destroy(device.as_ref());
        assert_eq!(
            recorder.total_live_objects(),
            before,
            "every buffer created must be destroyed"
        );
        recorder.assert_valid();
    }

    /// A pool error that has to travel as a [`HalError`] keeps its message,
    /// which is the whole reason the exhaustion error names its numbers.
    #[test]
    fn a_pool_error_flattens_into_the_seams_without_losing_its_message() {
        let full = InstancePoolError::PoolFull {
            capacity: 4,
            in_use: 4,
        };
        let message = full.to_string();
        assert_eq!(HalError::from(full).to_string(), message);

        let hal = InstancePoolError::Hal(HalError::OutOfDeviceMemory);
        assert!(
            matches!(HalError::from(hal), HalError::OutOfDeviceMemory),
            "a HalError must not be stringified on the way through"
        );
    }
}
