//! CPU spans and counters: where a frame's time went, and on which thread.
//!
//! `docs/plan/40-profiling.md`'s span API — "a scoped CPU span with a static
//! name, opened and closed by RAII, nesting freely; the frame is the outermost",
//! and counters as "spans' siblings: a named `u64` sampled per frame". The GPU
//! half already exists as `crcbl_render::timing`'s per-pass timestamps; this is
//! the CPU half it was always meant to sit beside, and it lives here for the
//! reason [`log`](crate::log) and [`time`](crate::time) do: every crate that has
//! to open a span already depends on this one.
//!
//! ```
//! use crcbl_core::trace;
//!
//! trace::set_enabled(true);
//! {
//!     let _frame = trace::span("frame");
//!     {
//!         let _tick = trace::span("tick");
//!         trace::counter("draws", 128);
//!     }
//! }
//!
//! // Two spans are four records — a begin and an end each — plus the counter.
//! let snapshot = trace::drain();
//! assert_eq!(snapshot.record_count(), 5);
//! ```
//!
//! # Always compiled, gated at runtime
//!
//! The plan's decision, and its reasoning: "a profiler you must rebuild to use
//! is one nobody turns on mid-investigation, and a build that changes what it
//! measures is the classic way to measure the wrong thing." So [`span`] and
//! [`counter`] are compiled into every build and cost **one relaxed atomic
//! load** — [`is_enabled`] — when the gate is off. Nothing else runs: no
//! timestamp is read, no lock is taken, and the [`Span`] returned remembers that
//! it opened nothing, so its drop is a branch on a bool. Verified by reading the
//! release assembly: a plain byte load of the gate, a test, and a tail call
//! straight through to whatever the span wrapped.
//!
//! The gate starts **disabled**, so a process pays that and nothing else until
//! something calls [`set_enabled`].
//!
//! There is no compile-time removal, and that is deliberate for now. The plan
//! reserves one for shipping builds; nothing ships from this repo yet, so it has
//! no caller. When it earns its place it arrives as `--cfg crcbl_trace_off` with
//! a `build.rs` declaring `cargo::rustc-check-cfg`, **not** as a Cargo feature: a
//! feature participates in unification, so `--all-features` — which is how this
//! workspace's only test jobs run — would turn recording off everywhere and
//! leave CI asserting nothing but "nothing recorded".
//!
//! # A flat buffer of begin/end records, not a tree
//!
//! Every record goes into one flat per-thread [`Vec`] in the order it happened;
//! a span is a [`SpanBegin`](RecordKind::SpanBegin) and a matching
//! [`SpanEnd`](RecordKind::SpanEnd), never a node with children.
//!
//! That is what the plan's three consumers want. Chrome Trace Event JSON — the
//! decided export format — *is* a begin/end stream per thread. A debug-panel row
//! showing p50/p95 for a name is a scan for that name. Per-thread tracks are the
//! partition the buffers already are. A tree built at record time would buy none
//! of them anything and would cost a parent pointer maintained on the hot path.
//!
//! **What it costs the consumer**: two records per span rather than one, and
//! nesting that has to be recovered rather than read. So each record carries the
//! [`depth`](Record::depth) it sat at — the consumer gets nesting without a
//! stack walk, and the recorded depth agrees with a stack walk by construction,
//! because it *is* the height of the recorded stack at that moment.
//!
//! # What bounds the buffer, and what happens at the bound
//!
//! A per-thread buffer of [`RECORDS_PER_THREAD`] records, allocated once and
//! never grown. Past it, records are **refused and counted** — never dropped
//! silently, and never the oldest: the oldest record in a frame is the frame's
//! own begin, and a ring that evicted it would corrupt the nesting of everything
//! after it while reporting nothing. [`ThreadTrace::dropped`] carries the count
//! and [`Snapshot::report`] prints it, the way `FrameTimings` reports its own
//! truncation.
//!
//! Refusal keeps the buffer *structurally* intact, not merely short. The rule is
//! `records + ends still owed <= RECORDS_PER_THREAD`, so a span whose begin was
//! admitted always has room for its end, and a span whose begin was refused
//! records no end either. A reader never sees a begin with no end because the
//! buffer filled up.
//!
//! [`drain`] is the frame boundary: it takes every track's records and resets
//! the counts. It does **not** disturb spans that are open across it, which is
//! the one seam a reader has to know about — a span open at a drain has its
//! begin in one snapshot and its end in the next. Both halves carry their depth,
//! so both are still placeable, but a consumer pairing begins to ends must
//! tolerate an unmatched end at the start of a snapshot and an unmatched begin
//! at the end of one.
//!
//! # Threads
//!
//! Each thread records into a buffer of its own, reached through a thread-local
//! and owned by a process-wide registry, and [`drain`] collects them. Buffers
//! are per-thread and not one shared log because a profiler that serialises the
//! job system's workers on a lock changes the thing it is measuring, which is
//! the first risk the plan names.
//!
//! A thread is identified by a [`ThreadTrack`], a small counter this module
//! hands out. Not [`std::thread::ThreadId`]: it is opaque, it is not
//! [`Display`](core::fmt::Display), and there is no stable way to get a number
//! out of it — while a Chrome Trace `tid` and a debug-panel row both need one.
//! The thread's name is captured alongside it, once, so a track can be labelled
//! `worker-2` rather than `thread 3`. On `wasm32` there is one thread and it
//! gets one track, with no special case anywhere in this module.
//!
//! A track outlives the thread that owned it, so a worker's spans survive its
//! exit; the next [`drain`] hands them over and reclaims the track.
//! [`MAX_TRACKS`] is the hard bound on how many exist at once, and a thread
//! refused one is counted in [`Snapshot::refused_tracks`].
//!
//! # The clock
//!
//! One process-wide [`MonotonicTime`], created on first use, read by every
//! thread. It is [`crate::time`]'s own source rather than a bare
//! [`Instant`](std::time::Instant), because the whole point of a shared epoch is
//! that two threads' records are comparable, and a per-thread `Instant` origin
//! would make them not be.
//!
//! It is a real clock read from inside the engine, which
//! `docs/plan/12-testing.md` otherwise forbids. That is sound here for one
//! reason and only that one: **nothing the trace records is ever an input to
//! simulation.** Nothing reads a span back into the world. The tests below
//! assert structure — names, kinds, depths, counts — and the one that asserts on
//! time asserts a bound a [`sleep`](std::thread::sleep) guarantees.
//!
//! The epoch is the trace's own, taken when the first span opens. It is not the
//! frame clock's epoch, so a trace timestamp is not directly comparable with an
//! [`EventTime`](crate::time::EventTime).
//!
//! **On `wasm32-unknown-unknown`, [`Instant::now`](std::time::Instant::now)
//! compiles and panics** — `std`'s unsupported-platform stub — so enabling the
//! trace in a browser build panics on the first span. Left loud rather than
//! papered over with a zero clock: a trace whose every span lasts zero
//! nanoseconds is worse than no trace, and the gate starts off, so a browser
//! build that never enables it never reads the clock at all. A browser clock
//! means `performance.now()` through a dependency this crate does not have, and
//! that is the browser slice's decision to take.
//!
//! # Reentrancy
//!
//! A span opened from inside the trace machinery would deadlock on the buffer
//! lock it already holds. Nothing can: the recording path takes one lock, and
//! under it it does integer arithmetic and [`Vec::push`] on a vector that cannot
//! reallocate — the capacity is reserved up front and the refusal rule above
//! keeps the length inside it. It calls no caller code, formats nothing, and
//! logs nothing, so there is no path from inside it back to [`span`].
//!
//! The one caller it cannot rule out is a global allocator that opens spans of
//! its own, which would re-enter through the *first* span on a thread — the one
//! that allocates the buffer. That is caught rather than argued away: the
//! thread-local is a [`RefCell`] and a re-entrant call finds it already
//! borrowed, records nothing, and returns.

use core::fmt;
use core::fmt::Write as _;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::cell::RefCell;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use crate::time::{MonotonicTime, TimeSource as _};

/// Records one thread's buffer holds before it starts refusing.
///
/// **Records, not spans** — a span is two of them. The buffer is allocated at
/// this size the first time a thread records, and never grows: a profiler that
/// reallocates on the hot path is measuring its own allocator. That is the
/// trade-off in the number, memory against how much of a frame survives a burst
/// of instrumentation, and it is sized for a frame's worth of engine-level spans
/// rather than per-entity ones.
pub const RECORDS_PER_THREAD: usize = 4096;

/// Threads that can hold a track at once.
///
/// A thread past the bound records nothing and is counted in
/// [`Snapshot::refused_tracks`]. Tracks are reclaimed by [`drain`] once the
/// thread that owned one has exited, so this bounds *concurrent* tracers rather
/// than the number of threads a process may ever create.
pub const MAX_TRACKS: usize = 64;

/// Deepest span nesting a buffer can reach.
///
/// Not a separate budget: every open span has already recorded a begin and still
/// owes an end, so [`RECORDS_PER_THREAD`] cannot hold more than this many at
/// once. Stated as a constant because the open-span stack is reserved to it and
/// therefore never reallocates either.
const MAX_DEPTH: usize = RECORDS_PER_THREAD / 2;

/// The runtime gate. Relaxed everywhere: it orders nothing, and a thread that
/// sees a stale value for a few nanoseconds after [`set_enabled`] records one
/// span more or less than it might have, which is not a defect in a profiler.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Next [`ThreadTrack`] to hand out. Monotonic, so a reclaimed track's number is
/// never reused and a snapshot's numbering cannot alias across drains.
static NEXT_TRACK: AtomicU32 = AtomicU32::new(0);

/// Threads refused a track since the last [`drain`].
static REFUSED_TRACKS: AtomicU64 = AtomicU64::new(0);

/// Every live track. Taken by [`drain`] and by registration, never by the
/// recording path, which holds only its own buffer's lock.
static TRACKS: Mutex<Vec<Arc<TrackEntry>>> = Mutex::new(Vec::new());

/// The trace's own epoch. See the module docs on the clock.
static EPOCH: OnceLock<MonotonicTime> = OnceLock::new();

/// Turns recording on or off, returning what it was.
///
/// Takes effect on other threads as soon as they next load the gate. Spans
/// already open are unaffected: one opened while enabled records its end however
/// this is set by the time it drops, so the buffer's pairing never breaks
/// mid-span.
pub fn set_enabled(enabled: bool) -> bool {
    ENABLED.swap(enabled, Ordering::Relaxed)
}

/// Whether [`span`] and [`counter`] record anything.
///
/// The disabled cost of the whole module: this one relaxed load.
#[inline]
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Nanoseconds since the trace epoch.
fn now_nanos() -> u64 {
    let epoch = EPOCH.get_or_init(MonotonicTime::new);
    // A `u64` of nanoseconds is centuries; the saturation is a formality that
    // costs nothing and avoids a panic in a profiler.
    u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Steps over a poisoned lock rather than propagating it.
///
/// A panic unwinding through an open [`Span`] drops the guard, which records an
/// end — so a poisoned buffer is the *normal* aftermath of a test that panicked
/// on purpose, and it must not disable the profiler for the rest of the process.
/// The same call [`capture`](crate::log::capture) makes, for the same reason.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// What a trace is made of
// ---------------------------------------------------------------------------

/// Which thread a run of records came from.
///
/// A number this module hands out, not [`std::thread::ThreadId`]; see the module
/// docs on threads for why.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ThreadTrack(u32);

impl ThreadTrack {
    /// The raw track number — a Chrome Trace `tid`, and the sort key for a
    /// per-thread row.
    #[inline]
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ThreadTrack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "thread {}", self.0)
    }
}

/// What one [`Record`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecordKind {
    /// A span opened.
    SpanBegin,
    /// A span closed — the innermost one still open on this track, so begins and
    /// ends nest.
    SpanEnd,
    /// A counter sampled, carrying its value.
    Counter(u64),
}

/// One thing that happened, on one thread.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Record {
    /// The span's or counter's name. `&'static str` rather than a `String`
    /// because a profiler that allocates a name per span is measuring its own
    /// allocator; an end carries the name its begin was given.
    pub name: &'static str,
    /// What happened.
    pub kind: RecordKind,
    /// How many spans were open around it — zero for the outermost, and the
    /// same on a span's begin and its end.
    ///
    /// Counted over spans this buffer actually *holds*, so it always agrees with
    /// a stack walk of [`ThreadTrace::records`] even after records were refused.
    pub depth: u16,
    /// Nanoseconds since the trace epoch. Comparable with every other record in
    /// the same [`Snapshot`], whatever thread it came from; not comparable with
    /// an [`EventTime`](crate::time::EventTime).
    pub nanos: u64,
}

/// One thread's share of a [`Snapshot`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ThreadTrace {
    /// Which track these came from.
    pub track: ThreadTrack,
    /// The thread's name when it first recorded, if it had one.
    pub name: Option<String>,
    /// Everything the thread recorded since the last [`drain`], oldest first.
    pub records: Vec<Record>,
    /// Records this track refused for want of room. Non-zero means the trace has
    /// a hole in it and the reader is being told so; see the module docs.
    pub dropped: u64,
}

/// Everything every thread recorded since the last [`drain`].
///
/// Tracks that recorded nothing and dropped nothing are absent: a per-frame
/// trace has no room for a row saying a thread was idle.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// One entry per thread that recorded, in track order.
    pub threads: Vec<ThreadTrace>,
    /// Threads refused a track because [`MAX_TRACKS`] were already taken. Their
    /// spans are not in here and are not counted in any
    /// [`ThreadTrace::dropped`], because they never reached a buffer.
    pub refused_tracks: u64,
}

impl Snapshot {
    /// Records across every track.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.threads.iter().map(|thread| thread.records.len()).sum()
    }

    /// Whether anything at all was recorded, dropped or refused.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.threads.is_empty() && self.refused_tracks == 0
    }

    /// Records refused for want of room, across every track.
    ///
    /// Non-zero is the whole trace's "there is a hole here" signal.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.threads.iter().map(|thread| thread.dropped).sum()
    }

    /// The snapshot, as text — the counterpart to `FrameTimings::report`, and
    /// what makes a dropped record *reported* rather than merely counted.
    #[must_use]
    pub fn report(&self) -> String {
        if self.is_empty() {
            return "trace: nothing recorded\n".to_string();
        }
        let mut out = String::new();
        let _ = writeln!(
            out,
            "trace: {} record(s) on {} thread(s), {} dropped",
            self.record_count(),
            self.threads.len(),
            self.dropped(),
        );
        for thread in &self.threads {
            let _ = writeln!(
                out,
                "  {:<12} {:<16} {:>6} record(s), {:>6} dropped",
                thread.track.to_string(),
                thread.name.as_deref().unwrap_or("-"),
                thread.records.len(),
                thread.dropped,
            );
        }
        if self.refused_tracks != 0 {
            let _ = writeln!(
                out,
                "  {} thread(s) refused a track: MAX_TRACKS were already taken",
                self.refused_tracks,
            );
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Recording
// ---------------------------------------------------------------------------

/// An open span. Closes when dropped.
///
/// Neither `Send` nor `Sync`, and that is enforced rather than asked for: the
/// end is pushed onto the buffer of whichever thread drops the guard, so a guard
/// that crossed a thread boundary would pair its end with another thread's begin
/// and corrupt both tracks' nesting.
///
/// An end always closes the innermost span still open on the track, which for
/// guards held in locals — the shape this is for — is the guard being dropped.
/// Guards kept in a collection and dropped out of order still produce a
/// well-formed trace, but the durations are attributed by nesting rather than by
/// which guard went first.
///
/// The `Send` bound is what refuses the first of those, and it refuses it at
/// compile time:
///
/// ```compile_fail,E0277
/// let span = crcbl_core::trace::span("frame");
/// std::thread::spawn(move || drop(span));
/// ```
#[derive(Debug)]
#[must_use = "a span runs for as long as its guard lives; `trace::span(name)` \
              on its own opens and closes it in the same instant"]
pub struct Span {
    /// Whether a begin was recorded, and so whether an end is owed. Set once, at
    /// creation — **not** re-read from the gate at drop, so a span opened while
    /// the trace was on always closes, and one opened while it was off never
    /// leaves an end with no begin.
    open: bool,
    _not_send: PhantomData<*const ()>,
}

impl Drop for Span {
    #[inline]
    fn drop(&mut self) {
        if self.open {
            record_end();
        }
    }
}

/// Opens a span named `name`, closed when the returned guard drops.
///
/// Nests freely; the frame is meant to be the outermost. Costs one relaxed
/// atomic load when the trace is disabled.
///
/// ```
/// use crcbl_core::trace;
///
/// trace::set_enabled(true);
/// let _frame = trace::span("frame");
/// ```
#[inline]
pub fn span(name: &'static str) -> Span {
    if !is_enabled() {
        return Span {
            open: false,
            _not_send: PhantomData,
        };
    }
    Span {
        open: record_begin(name),
        _not_send: PhantomData,
    }
}

/// Samples a counter — a named `u64`, on the track and at the depth it was
/// sampled from.
///
/// Spans' sibling, not a log line: it lands in the same buffer, under the same
/// gate and the same bound.
#[inline]
pub fn counter(name: &'static str, value: u64) {
    if !is_enabled() {
        return;
    }
    let nanos = now_nanos();
    with_buffer(|buffer| buffer.sample(name, value, nanos));
}

/// Takes everything recorded since the last call and resets the counts.
///
/// The frame boundary. Spans open across it are split rather than disturbed —
/// see the module docs. Also reclaims the tracks of threads that have exited.
pub fn drain() -> Snapshot {
    let mut tracks = lock(&TRACKS);
    let mut threads = Vec::new();
    tracks.retain(|entry| {
        let mut buffer = lock(&entry.buffer);
        if !buffer.records.is_empty() || buffer.dropped != 0 {
            threads.push(ThreadTrace {
                track: entry.track,
                name: entry.name.clone(),
                // Copy and clear rather than take: `Vec::clear` keeps the
                // capacity the recording path relies on never having to grow.
                records: buffer.records.clone(),
                dropped: buffer.dropped,
            });
            buffer.records.clear();
            buffer.dropped = 0;
        }
        drop(buffer);
        // The registry holds one reference and the owning thread's thread-local
        // holds the other; one alone means that thread has exited, and its
        // records have just been handed over.
        Arc::strong_count(entry) > 1
    });
    drop(tracks);
    Snapshot {
        threads,
        refused_tracks: REFUSED_TRACKS.swap(0, Ordering::Relaxed),
    }
}

/// One thread's buffer, and the bookkeeping that keeps it bounded.
#[derive(Debug)]
struct TrackBuffer {
    records: Vec<Record>,
    /// The names of the spans open on this thread whose begin this buffer holds
    /// — one entry per end still owed, so its length is both the depth the next
    /// record sits at and the number of slots that must stay free.
    open: Vec<&'static str>,
    dropped: u64,
}

impl TrackBuffer {
    fn new() -> Self {
        Self {
            records: Vec::with_capacity(RECORDS_PER_THREAD),
            open: Vec::with_capacity(MAX_DEPTH),
            dropped: 0,
        }
    }

    /// Whether `extra` more records fit with every owed end still accounted for.
    ///
    /// The invariant every path below preserves:
    /// `records.len() + open.len() <= RECORDS_PER_THREAD`.
    fn has_room_for(&self, extra: usize) -> bool {
        self.records.len() + self.open.len() + extra <= RECORDS_PER_THREAD
    }

    /// Appends a record. Never reallocates, which the caller must have
    /// established with [`TrackBuffer::has_room_for`].
    fn push(&mut self, name: &'static str, kind: RecordKind, nanos: u64) {
        debug_assert!(
            self.records.len() < self.records.capacity(),
            "the trace buffer is about to reallocate on the recording path"
        );
        // `open` is bounded by `MAX_DEPTH`, far below `u16::MAX`; the saturation
        // is unreachable and costs a compare.
        let depth = u16::try_from(self.open.len()).unwrap_or(u16::MAX);
        self.records.push(Record {
            name,
            kind,
            depth,
            nanos,
        });
    }

    /// Opens a span, reporting whether it was admitted.
    ///
    /// Two slots, not one: a begin is only admitted if its end fits too, so a
    /// full buffer never strands a begin.
    fn begin(&mut self, name: &'static str, nanos: u64) -> bool {
        if !self.has_room_for(2) {
            self.dropped += 1;
            return false;
        }
        self.push(name, RecordKind::SpanBegin, nanos);
        self.open.push(name);
        true
    }

    /// Closes the innermost open span.
    fn end(&mut self, nanos: u64) {
        let Some(name) = self.open.pop() else {
            // Only reachable if a `Span` whose begin was admitted is dropped
            // after `drain` cleared this buffer's open stack — which `drain`
            // does not do. Recording nothing is the safe answer either way.
            return;
        };
        // Guaranteed by the invariant: the slot this end occupies was reserved
        // when its begin was admitted.
        self.push(name, RecordKind::SpanEnd, nanos);
    }

    /// Samples a counter.
    fn sample(&mut self, name: &'static str, value: u64, nanos: u64) {
        if !self.has_room_for(1) {
            self.dropped += 1;
            return;
        }
        self.push(name, RecordKind::Counter(value), nanos);
    }
}

/// A track and the buffer behind it. Outlives the thread that owned it, so a
/// worker's spans survive its exit until the next [`drain`].
#[derive(Debug)]
struct TrackEntry {
    track: ThreadTrack,
    name: Option<String>,
    buffer: Mutex<TrackBuffer>,
}

/// Whether this thread has a track, asked once.
#[derive(Debug)]
enum Registration {
    /// Nothing has been recorded on this thread yet.
    Untried,
    /// [`MAX_TRACKS`] were taken when this thread first recorded. Sticky, so a
    /// refused thread does not take the registry lock on every span.
    Refused,
    /// This thread's track.
    Track(Arc<TrackEntry>),
}

thread_local! {
    /// This thread's registration.
    ///
    /// A `RefCell` behind a `const` initialiser rather than a lazily initialised
    /// thread-local holding the `Arc` directly, and both halves of that are
    /// load-bearing. The `const` init means there is no initialiser that a
    /// re-entrant call could run a second time, and the `RefCell` means such a
    /// call finds the slot borrowed and gives up instead of recursing — see the
    /// module docs on reentrancy. It is the shape
    /// [`log`](crate::log)'s capture buffer uses, for the same reason.
    static LOCAL: RefCell<Registration> = const { RefCell::new(Registration::Untried) };
}

/// Claims a track for the calling thread, or `None` if [`MAX_TRACKS`] are taken.
fn register() -> Option<Arc<TrackEntry>> {
    let mut tracks = lock(&TRACKS);
    if tracks.len() >= MAX_TRACKS {
        REFUSED_TRACKS.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    // Built under the lock so a refused thread neither allocates a buffer it
    // will never use nor burns a track number.
    let entry = Arc::new(TrackEntry {
        track: ThreadTrack(NEXT_TRACK.fetch_add(1, Ordering::Relaxed)),
        name: std::thread::current().name().map(str::to_owned),
        buffer: Mutex::new(TrackBuffer::new()),
    });
    // Pushed while the lock is held and while this function still owns a
    // reference, so `drain` can never observe the entry at a strong count of one
    // and reclaim a track that is about to be handed to a live thread.
    tracks.push(Arc::clone(&entry));
    drop(tracks);
    Some(entry)
}

/// Runs `f` against this thread's buffer, or nothing at all if this thread has
/// no track, is already inside the recording path, or is being torn down.
fn with_buffer<R>(f: impl FnOnce(&mut TrackBuffer) -> R) -> Option<R> {
    LOCAL
        .try_with(|slot| {
            // Already borrowed means this call re-entered the recording path.
            let mut slot = slot.try_borrow_mut().ok()?;
            if matches!(*slot, Registration::Untried) {
                *slot = match register() {
                    Some(entry) => Registration::Track(entry),
                    None => Registration::Refused,
                };
            }
            let Registration::Track(entry) = &*slot else {
                return None;
            };
            Some(f(&mut lock(&entry.buffer)))
        })
        // `Err` is a thread past its thread-local destructors — a span opened
        // from another `Drop` at thread exit. It records nothing.
        .ok()
        .flatten()
}

/// Records a span's begin, reporting whether it was admitted.
fn record_begin(name: &'static str) -> bool {
    let nanos = now_nanos();
    with_buffer(|buffer| buffer.begin(name, nanos)).unwrap_or(false)
}

/// Records the end of the innermost open span on this thread.
fn record_end() {
    let nanos = now_nanos();
    with_buffer(|buffer| buffer.end(nanos));
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use proptest::prelude::*;

    use super::*;

    /// Serialises the tests that touch the process-wide trace.
    ///
    /// `cargo test` runs a crate's tests as threads in one process — nextest
    /// gives each its own — so without this two of them would share the gate and
    /// the registry, and drain each other's records.
    static TRACE_TESTS: Mutex<()> = Mutex::new(());

    /// One test's exclusive hold on the trace, handed over enabled and clean.
    #[derive(Debug)]
    struct TraceTest {
        _guard: MutexGuard<'static, ()>,
    }

    impl Drop for TraceTest {
        fn drop(&mut self) {
            set_enabled(false);
            drop(drain());
        }
    }

    /// Takes the trace, clears whatever an earlier test left behind, and enables
    /// it. Dropping the returned value disables and clears it again.
    fn trace_test() -> TraceTest {
        // Stepped over rather than propagated: a test that panics on purpose
        // must not take every later one with it.
        let guard = TRACE_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
        set_enabled(false);
        drop(drain());
        set_enabled(true);
        TraceTest { _guard: guard }
    }

    /// A record reduced to what an assertion is written against.
    type Shape = (&'static str, RecordKind, u16);

    fn shape(record: &Record) -> Shape {
        (record.name, record.kind, record.depth)
    }

    /// This thread's records, drained.
    ///
    /// Asserts the single-track assumption instead of trusting it: a stray track
    /// would mean an earlier test leaked a thread and this test is reading
    /// someone else's buffer.
    fn drain_this_thread() -> Vec<Record> {
        let snapshot = drain();
        assert!(
            snapshot.threads.len() <= 1,
            "another thread is recording: {}",
            snapshot.report()
        );
        assert_eq!(snapshot.refused_tracks, 0);
        snapshot
            .threads
            .into_iter()
            .next()
            .map_or_else(Vec::new, |thread| {
                assert_eq!(thread.dropped, 0, "records were refused unexpectedly");
                thread.records
            })
    }

    fn shapes_of(records: &[Record]) -> Vec<Shape> {
        records.iter().map(shape).collect()
    }

    #[test]
    fn nesting_is_recorded_in_order_and_at_the_right_depth() {
        let _test = trace_test();
        {
            let _frame = span("frame");
            {
                let _tick = span("tick");
                let _physics = span("physics");
            }
            let _render = span("render");
        }

        assert_eq!(
            shapes_of(&drain_this_thread()),
            vec![
                ("frame", RecordKind::SpanBegin, 0),
                ("tick", RecordKind::SpanBegin, 1),
                ("physics", RecordKind::SpanBegin, 2),
                // Guards drop in reverse declaration order, innermost first.
                ("physics", RecordKind::SpanEnd, 2),
                ("tick", RecordKind::SpanEnd, 1),
                ("render", RecordKind::SpanBegin, 1),
                ("render", RecordKind::SpanEnd, 1),
                ("frame", RecordKind::SpanEnd, 0),
            ]
        );
    }

    /// The disabled arm records **nothing** — not fewer records, none.
    #[test]
    fn the_disabled_gate_records_nothing() {
        let _test = trace_test();
        set_enabled(false);
        assert!(!is_enabled());

        {
            let _frame = span("frame");
            let _tick = span("tick");
            counter("draws", 4096);
        }

        let snapshot = drain();
        assert!(
            snapshot.is_empty(),
            "the disabled trace recorded something: {}",
            snapshot.report()
        );
        assert_eq!(snapshot.record_count(), 0);
        assert_eq!(snapshot.dropped(), 0);
        assert_eq!(snapshot.report(), "trace: nothing recorded\n");
    }

    /// The gate reports its own state, and reports what it displaced.
    #[test]
    fn the_gate_reports_what_it_was_set_to() {
        let _test = trace_test();
        assert!(
            set_enabled(true),
            "`trace_test` hands the gate over enabled"
        );
        assert!(is_enabled());
        assert!(set_enabled(false), "the previous value was `true`");
        assert!(!is_enabled());
        assert!(!set_enabled(true), "the previous value was `false`");
        assert!(is_enabled());
    }

    /// The gate flipping under a live guard must not leave a begin without an
    /// end, or an end without a begin.
    #[test]
    fn a_guard_survives_the_gate_flipping_under_it() {
        let _test = trace_test();

        // Opened while enabled, dropped while disabled: the end still lands, so
        // the pair is intact.
        {
            let _outer = span("opened-enabled");
            set_enabled(false);
        }
        assert_eq!(
            shapes_of(&drain_this_thread()),
            vec![
                ("opened-enabled", RecordKind::SpanBegin, 0),
                ("opened-enabled", RecordKind::SpanEnd, 0),
            ]
        );

        // Opened while disabled, dropped while enabled: nothing at all, rather
        // than an orphan end.
        {
            let _outer = span("opened-disabled");
            set_enabled(true);
        }
        assert_eq!(shapes_of(&drain_this_thread()), Vec::new());

        // And the buffer is still usable afterwards, at depth zero.
        {
            let _after = span("after");
        }
        assert_eq!(
            shapes_of(&drain_this_thread()),
            vec![
                ("after", RecordKind::SpanBegin, 0),
                ("after", RecordKind::SpanEnd, 0),
            ]
        );
    }

    #[test]
    fn counters_carry_their_value_and_the_depth_they_were_sampled_at() {
        let _test = trace_test();
        counter("jobs", 7);
        {
            let _frame = span("frame");
            counter("draws", 128);
            counter("instances", u64::MAX);
        }

        assert_eq!(
            shapes_of(&drain_this_thread()),
            vec![
                ("jobs", RecordKind::Counter(7), 0),
                ("frame", RecordKind::SpanBegin, 0),
                ("draws", RecordKind::Counter(128), 1),
                ("instances", RecordKind::Counter(u64::MAX), 1),
                ("frame", RecordKind::SpanEnd, 0),
            ]
        );
    }

    /// A full buffer refuses new records, counts them, and still closes the
    /// spans that were already open — and never grows to do it.
    #[test]
    fn a_full_buffer_refuses_and_counts_instead_of_growing() {
        let _test = trace_test();
        // The frame's own begin means the counters run out of room before they
        // run out of count; the surplus only guarantees they overshoot.
        let attempted = u64::try_from(RECORDS_PER_THREAD).unwrap() + 16;
        {
            let _frame = span("frame");
            for value in 0..attempted {
                counter("filler", value);
            }
            // The buffer is full, so this span is refused outright rather than
            // recording a begin it could not close.
            let _refused = span("refused");
        }
        let capacity_before = buffer_capacity();

        let snapshot = drain();
        let thread = &snapshot.threads[0];
        assert_eq!(
            thread.records.len(),
            RECORDS_PER_THREAD,
            "the buffer should be exactly full"
        );
        let admitted = thread
            .records
            .iter()
            .filter(|record| matches!(record.kind, RecordKind::Counter(_)))
            .count();
        assert!(admitted > 0 && u64::try_from(admitted).unwrap() < attempted);
        // Every counter that did not fit, plus the one span that was refused.
        assert_eq!(
            thread.dropped,
            attempted - u64::try_from(admitted).unwrap() + 1
        );
        assert_eq!(snapshot.dropped(), thread.dropped);
        // The outermost span still closed: its end slot was reserved when its
        // begin was admitted.
        assert_eq!(
            shape(&thread.records[0]),
            ("frame", RecordKind::SpanBegin, 0)
        );
        let last = thread.records.last().expect("the buffer is not empty");
        assert_eq!(shape(last), ("frame", RecordKind::SpanEnd, 0));
        // Nothing named `refused` reached the buffer at all.
        assert!(thread.records.iter().all(|record| record.name != "refused"));
        assert!(
            snapshot.report().contains("dropped"),
            "the report hides the hole: {}",
            snapshot.report()
        );
        // The whole point of refusing: the buffer never grew to take the
        // overflow, either while recording or while draining.
        assert_eq!(capacity_before, RECORDS_PER_THREAD);
        assert_eq!(
            buffer_capacity(),
            RECORDS_PER_THREAD,
            "draining reallocated"
        );
    }

    /// This thread's record-buffer capacity, read straight out of its track.
    fn buffer_capacity() -> usize {
        LOCAL.with(|slot| {
            let slot = slot.borrow();
            let Registration::Track(entry) = &*slot else {
                panic!("this thread has no track");
            };
            lock(&entry.buffer).records.capacity()
        })
    }

    /// A worker's spans land on their own track, under the worker's own name,
    /// and survive the worker exiting.
    #[test]
    fn a_worker_thread_gets_a_track_of_its_own() {
        let _test = trace_test();
        {
            let _frame = span("frame");
            std::thread::Builder::new()
                .name("worker-0".to_owned())
                .spawn(|| {
                    let _job = span("job");
                    counter("steals", 3);
                })
                .expect("spawn a worker")
                .join()
                .expect("the worker did not panic");
        }

        let snapshot = drain();
        assert_eq!(snapshot.threads.len(), 2, "{}", snapshot.report());
        let worker = snapshot
            .threads
            .iter()
            .find(|thread| thread.name.as_deref() == Some("worker-0"))
            .unwrap_or_else(|| panic!("no worker track: {}", snapshot.report()));
        assert_eq!(
            shapes_of(&worker.records),
            vec![
                ("job", RecordKind::SpanBegin, 0),
                ("steals", RecordKind::Counter(3), 1),
                ("job", RecordKind::SpanEnd, 0),
            ]
        );

        let main = snapshot
            .threads
            .iter()
            .find(|thread| thread.track != worker.track)
            .expect("the main thread's track");
        assert_ne!(main.track, worker.track);
        assert_eq!(
            shapes_of(&main.records),
            vec![
                ("frame", RecordKind::SpanBegin, 0),
                ("frame", RecordKind::SpanEnd, 0),
            ]
        );

        // The exited worker's track was reclaimed by the drain above, so the
        // next one starts clean.
        assert_eq!(drain_this_thread(), Vec::new());
    }

    /// Past [`MAX_TRACKS`] a thread records nothing and is counted saying so.
    #[test]
    fn threads_past_the_track_bound_are_refused_and_counted() {
        let _test = trace_test();
        // More than the bound, whatever tracks this process already holds.
        let surplus = 4;
        for index in 0..MAX_TRACKS + surplus {
            std::thread::Builder::new()
                .name(format!("crowd-{index}"))
                .spawn(|| {
                    let _job = span("job");
                })
                .expect("spawn")
                .join()
                .expect("the thread did not panic");
        }

        let snapshot = drain();
        assert!(
            snapshot.threads.len() <= MAX_TRACKS,
            "{} tracks past a bound of {MAX_TRACKS}",
            snapshot.threads.len()
        );
        assert!(
            snapshot.refused_tracks >= u64::try_from(surplus).unwrap(),
            "{} refusals: {}",
            snapshot.refused_tracks,
            snapshot.report()
        );
        assert!(snapshot.report().contains("refused a track"));

        // Every one of those threads has exited, so the drain above reclaimed
        // their tracks and the registry is back to this thread's alone.
        drop(span("after"));
        assert_eq!(drain().threads.len(), 1);
        assert_eq!(drain().refused_tracks, 0);
    }

    /// The one assertion here about *time*, and what keeps it from being the
    /// classic vacuous "a duration is greater than zero": the inner span sleeps,
    /// so its recorded length has a floor the clock cannot be under, and the
    /// outer span contains the inner one, so the two bounds also order the four
    /// timestamps against each other.
    #[test]
    fn a_spans_timestamps_bracket_what_ran_inside_it() {
        let _test = trace_test();
        let nap = Duration::from_millis(5);
        {
            let _outer = span("outer");
            let _inner = span("inner");
            std::thread::sleep(nap);
        }

        let records = drain_this_thread();
        let [outer_begin, inner_begin, inner_end, outer_end] = records.as_slice() else {
            panic!("expected two nested spans, got {records:?}");
        };
        let floor = u64::try_from(nap.as_nanos()).unwrap();
        assert!(
            inner_end.nanos - inner_begin.nanos >= floor,
            "the inner span slept {nap:?} and reports {} ns",
            inner_end.nanos - inner_begin.nanos
        );
        assert!(
            outer_end.nanos - outer_begin.nanos >= inner_end.nanos - inner_begin.nanos,
            "the outer span is shorter than the inner one it contains"
        );
        assert!(outer_begin.nanos <= inner_begin.nanos);
        assert!(inner_end.nanos <= outer_end.nanos);
    }

    /// A span open across a drain is split, not lost: the begin is in one
    /// snapshot and the end in the next, both carrying their depth.
    #[test]
    fn a_drain_splits_a_span_that_is_open_across_it() {
        let _test = trace_test();
        let frame = span("frame");
        assert_eq!(
            shapes_of(&drain_this_thread()),
            vec![("frame", RecordKind::SpanBegin, 0)]
        );
        drop(frame);
        assert_eq!(
            shapes_of(&drain_this_thread()),
            vec![("frame", RecordKind::SpanEnd, 0)]
        );
    }

    #[test]
    fn tracks_are_numbered_and_displayed() {
        assert_eq!(ThreadTrack(7).get(), 7);
        assert_eq!(ThreadTrack(7).to_string(), "thread 7");
        assert!(ThreadTrack(1) < ThreadTrack(2));
        assert!(Snapshot::default().is_empty());
        assert_eq!(Snapshot::default().record_count(), 0);
    }

    /// One step of the script the property below drives.
    #[derive(Clone, Copy, Debug)]
    enum Step {
        Open(&'static str),
        Close,
        Sample(u64),
    }

    /// A pool of static names, because that is what a span takes and a property
    /// test cannot leak a fresh `&'static str` per case.
    const NAMES: [&str; 4] = ["a", "b", "c", "d"];

    fn any_step() -> impl Strategy<Value = Step> {
        prop_oneof![
            5 => (0..NAMES.len()).prop_map(|index| Step::Open(NAMES[index])),
            4 => Just(Step::Close),
            2 => any::<u64>().prop_map(Step::Sample),
        ]
    }

    proptest! {
        /// The buffer's structural invariant, under an arbitrary script of
        /// opens, closes and samples: a stack walk of the records reproduces
        /// exactly the depth each record was recorded with, every begin is
        /// matched by an end carrying the same name and depth, and the buffer
        /// never exceeds its bound. This is the property a consumer relies on to
        /// rebuild nesting from a flat buffer.
        #[test]
        fn prop_recorded_depth_matches_a_stack_walk(steps in proptest::collection::vec(any_step(), 1..200)) {
            let _test = trace_test();
            let mut open: Vec<Span> = Vec::new();
            for step in &steps {
                match *step {
                    Step::Open(name) => open.push(span(name)),
                    Step::Close => drop(open.pop()),
                    Step::Sample(value) => counter("sample", value),
                }
            }
            // Innermost first, the order locals drop in.
            while open.pop().is_some() {}

            let records = drain_this_thread();
            prop_assert!(records.len() <= RECORDS_PER_THREAD);
            let mut stack: Vec<&'static str> = Vec::new();
            for record in &records {
                match record.kind {
                    RecordKind::SpanBegin => {
                        prop_assert_eq!(usize::from(record.depth), stack.len());
                        stack.push(record.name);
                    }
                    RecordKind::SpanEnd => {
                        let opened = stack.pop();
                        prop_assert_eq!(opened, Some(record.name));
                        prop_assert_eq!(usize::from(record.depth), stack.len());
                    }
                    RecordKind::Counter(_) => {
                        prop_assert_eq!(usize::from(record.depth), stack.len());
                    }
                }
            }
            // Every guard was dropped above, so nothing is left open.
            prop_assert!(stack.is_empty(), "unclosed spans: {:?}", stack);
            // Keeps the walk above from passing vacuously on a script that
            // recorded nothing: a `Close` with nothing open is a no-op, but an
            // `Open` or a `Sample` always lands.
            let recording = steps.iter().any(|step| !matches!(step, Step::Close));
            prop_assert_eq!(records.is_empty(), !recording);
        }
    }
}
