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
//! # What a removed instance leaves behind
//!
//! [`InstancePool::remove`] frees the slot and retires the handle; it does
//! **not** rewrite the element on the GPU, which keeps whatever the removed
//! instance last had. That is safe today because nothing walks the array —
//! the draw addresses one instance the caller names — and it is a real gap the
//! moment §3.3's cull pass iterates it. The bit that would answer "is this slot
//! live" is [`crcbl_shaders::mesh::GpuInstance::flags`], which is reserved and
//! defines nothing, so there is no honest way to write it here yet.

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
    capacity: u32,
}

impl InstancePool {
    /// Creates the ring, with every buffer uninitialised.
    ///
    /// Uninitialised rather than zeroed: an element is written before anything
    /// addresses it, because [`InstancePool::insert`] marks it dirty in every
    /// slot and [`InstancePool::begin_frame`] flushes a slot before that slot is
    /// drawn from.
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
        let mut buffers = Vec::with_capacity(desc.frames_in_flight);
        for frame in 0..desc.frames_in_flight {
            match device.create_buffer(&BufferDesc {
                label: Some(&format!("{stem} instances {frame}")),
                size,
                // Host-visible, so a delta is a `write_buffer` rather than a
                // staging copy needing a barrier inside a frame. `crate::graph`
                // is the only thing allowed to record a barrier during a frame,
                // and a per-frame transfer submit is what §3.1's start-up
                // upload path deliberately is not.
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            }) {
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
            capacity: desc.capacity,
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

    /// Takes a slot, writes `instance` into it, and marks it dirty everywhere.
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
        self.write(handle.index(), instance);
        Ok(handle)
    }

    /// Rewrites `handle`'s instance and marks it dirty everywhere.
    ///
    /// Returns whether the handle named a live instance; a stale one writes
    /// nothing rather than overwriting whatever took its slot.
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
        self.write(handle.index(), instance);
        true
    }

    /// What `handle`'s slot currently holds, decoded from the host mirror, or
    /// `None` if the handle is stale.
    #[must_use]
    pub fn get(&self, handle: InstanceHandle) -> Option<GpuInstance> {
        if !self.slots.contains(handle.cast()) {
            return None;
        }
        let at = handle.index() as usize * INSTANCE_STRIDE;
        let bytes: &[u8; INSTANCE_STRIDE] = self.mirror[at..at + INSTANCE_STRIDE]
            .try_into()
            .unwrap_or_else(|_| unreachable!("the mirror is capacity × INSTANCE_STRIDE"));
        Some(GpuInstance::from_bytes(bytes))
    }

    /// Which element of the array `handle` is, or `None` if the handle is stale.
    ///
    /// This is the number a shader indexes with, which is why it is public: a
    /// draw has to be able to say which instance it draws.
    #[must_use]
    pub fn index(&self, handle: InstanceHandle) -> Option<u32> {
        self.slots.contains(handle.cast()).then(|| handle.index())
    }

    /// Frees `handle`'s slot for reuse and retires the handle.
    ///
    /// Returns whether the handle named a live instance. The element's bytes are
    /// left as they are on the GPU — see the [module docs](self) on what that
    /// does and does not cost.
    pub fn remove(&mut self, handle: InstanceHandle) -> bool {
        self.slots.remove(handle.cast()).is_some()
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
        self.frame = (self.frame + 1) % self.buffers.len();
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
        Ok(slot)
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

    /// Puts `instance` into the mirror at `index` and marks it dirty in every
    /// slot's range set.
    fn write(&mut self, index: u32, instance: &GpuInstance) {
        let at = index as usize * INSTANCE_STRIDE;
        self.mirror[at..at + INSTANCE_STRIDE].copy_from_slice(&instance.to_bytes());
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

    /// An instance whose transform is distinguishable from every other `n`.
    fn instance(n: u32) -> GpuInstance {
        GpuInstance {
            transform: [n as f32; 16],
            mesh: n,
            material: n + 1,
            sector: n + 2,
            flags: n + 3,
        }
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

    /// What goes in comes out, field for field, and a removed handle stops
    /// resolving. The four ids are the same width and would permute silently,
    /// so the round trip is checked rather than assumed.
    #[test]
    fn an_instance_round_trips_through_the_pool() {
        let (recorder, device) = open();
        let mut pool = pool(device.as_ref(), 16);
        let handle = pool.insert(&instance(7)).expect("room");
        assert_eq!(pool.get(handle), Some(instance(7)));

        assert!(pool.set(handle, &instance(11)));
        assert_eq!(pool.get(handle), Some(instance(11)));

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
