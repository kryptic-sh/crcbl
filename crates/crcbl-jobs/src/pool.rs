//! The work-stealing pool, and `par_for` — data-parallel bursts inside one
//! pipeline stage.
//!
//! The third thing the design's job system asks for, after the two
//! communication primitives, and the one that is genuinely parallel rather than
//! merely concurrent: [`Pool::par_for`] splits a slice into fixed chunks and
//! runs them across the pool's workers and the calling thread.
//!
//! # It works with no threads at all, and that is the point
//!
//! A [`Pool`] built on [`Inline`](crate::Inline) has zero workers and runs
//! every chunk on the caller, in ascending order. Nothing else changes — same
//! call, same chunk boundaries, same results — which is the design's rule that
//! "`par_for` runs inline" on a runtime without threads, with **no `cfg` in the
//! systems themselves**.
//!
//! The two modes agree because the *split* is what decides the answer and the
//! split does not depend on the pool: chunk boundaries come from the caller's
//! chunk length and the slice's own length, never from the worker count, and
//! each chunk gets a disjoint `&mut` sub-slice. Threaded or not, the closure is
//! called once per chunk with exactly the same `(start, items)` pair; only
//! *who* calls it and *in which order* differ. Both drivers go through the same
//! `run_one`, so there is one implementation of "run chunk `i`" rather than two
//! that could drift.
//!
//! What the pool cannot promise is a closure whose effect is not confined to
//! the chunk it was handed: one that pushes into a shared `Mutex<Vec<_>>` sees
//! its own scheduling. The `Fn(usize, &mut [T]) + Sync` bound is what forces
//! any such state to be explicit — the borrow checker will not let a closure
//! mutate anything else — but "explicit" is not "order-free", and a sim system
//! that wants the determinism rule writes its results to its own slots.
//!
//! # Panics do not poison anything
//!
//! A chunk that panics is caught where it runs; the remaining chunks still run;
//! the panic is re-raised on the calling thread once they have. **The
//! outstanding-chunk count is decremented on the panicking path too**, which is
//! what keeps the caller from waiting forever on work that is never coming, and
//! catching it on the worker is what keeps a worker thread — and therefore the
//! pool — alive for the next call. Where several chunks panic, the one with the
//! **lowest chunk index** is the one re-raised, so a panicking `par_for`
//! reports the same failure with and without threads.
//!
//! # Waking a worker is a throughput mechanism, never a correctness one
//!
//! The driving thread pushes the chunks and then **runs them itself** until
//! they are gone, so a call finishes whether or not a single worker ever woke
//! up. A missed wakeup costs parallelism for that call and nothing else, which
//! is worth knowing twice over: it is why a `par_for` cannot deadlock behind
//! the sleep protocol, and it is why an ordinary test cannot tell a broken
//! wakeup from a working one — the answer comes out right either way. What
//! tells them apart is a chunk that refuses to finish until a *second* thread
//! arrives, which is what `the_work_reaches_more_than_the_calling_thread` does.
//!
//! # The counters are observation and nothing else
//!
//! [`Pool::stats`] reports what the pool did — which chunks ran where, how
//! often a worker found the deque empty, how often one slept. Every counter is
//! a `Relaxed` atomic that nothing inside the pool ever reads back, so none of
//! them joins the happens-before the code above rests on: delete every
//! increment and a `par_for` splits the same way, computes the same answer and
//! raises the same panic. That is what makes them safe to leave in a shipping
//! build rather than behind a feature.
//!
//! They are counted per *chunk*, and where the driver is doing the counting per
//! *call* — a `par_for` over ten thousand items adds to the driver's counter
//! once rather than once an item — because a counter in the frame path that
//! costs what it measures is worse than no counter at all.
//!
//! # Shutdown
//!
//! Dropping the pool sets the shutdown flag and broadcasts, so a worker parked
//! waiting for work wakes, sees it, and returns. The seam has no join — it
//! detaches by design, see [`Spawn::spawn`] — so `drop` does not wait for
//! workers to notice; what it must guarantee is that they *will*, and
//! `dropping_the_pool_stops_every_worker` is the test that they do. A worker
//! that outlives the `Pool` by a few microseconds holds only its `Arc` of the
//! pool's shared state, which is why that state is behind an `Arc` and not
//! borrowed from the `Pool`.

use core::fmt;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering};
use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use crate::deque::{self, Steal};
use crate::spawn::{Spawn, SpawnError};

/// Slots in the pool's deque, and therefore the most chunks one `par_for` can
/// queue at once.
///
/// A call that splits into more than this pushes what fits and runs the rest on
/// the calling thread as it goes, which costs parallelism rather than
/// correctness. It is sized for the shapes the design names — ten thousand
/// bodies at a chunk of sixteen is 625 chunks — and a call that overruns it is
/// a chunk length chosen too small, since the per-chunk overhead would dominate
/// long before the queue did.
const QUEUE_CAPACITY: usize = 1024;

/// The name every pool worker's thread wears.
///
/// One name for all of them: [`Spawn::spawn`] takes a `&'static str`, so a
/// per-worker name would have to be leaked, and a profiler lane reading
/// `pool` × N is what the design's timeline view wants anyway.
const WORKER_NAME: &str = "pool";

/// A pool of workers that steal data-parallel chunks, and the thread that
/// drives it.
///
/// **The driving thread is a worker too.** It pushes the chunks, then takes
/// them back off its own end while the pool's threads steal from the other, so
/// a call is never waiting on workers while it has work in hand. That is also
/// why [`Pool::new`] asks for one fewer thread than the machine's parallelism.
///
/// `par_for` takes `&mut self`, so **one thread drives a pool at a time** —
/// which is a fact about the type rather than a rule in a comment, and it is
/// what makes the single deque sound: the pool's workers only ever *steal*, and
/// stealing is what several threads may do at once. A subsystem that wants its
/// own parallelism builds its own pool, exactly as the topology gives each
/// subsystem its own thread.
pub struct Pool {
    shared: Arc<Shared>,
    /// The owning end of the deque. Held here rather than in [`Shared`]
    /// precisely because only the driver may push and pop.
    queue: deque::Worker<Chunk>,
    /// Where the chunk descriptors of the call in flight live. Owned by the
    /// pool and reused, so a steady-state `par_for` allocates nothing.
    chunks: Vec<Chunk>,
    workers: usize,
}

/// What the workers share with the driver.
struct Shared {
    thieves: deque::Stealer<Chunk>,
    /// Guards [`Sleep`]. Taken once per `par_for` submission, and by a worker
    /// only when it is about to sleep — never on the stealing path, which is
    /// the "no mutexes in the frame path" rule read for what it is aimed at.
    sleep: Mutex<Sleep>,
    wake: Condvar,
    /// Observation, and nothing the pool itself reads — see the
    /// [module docs](self) and [`Counters`].
    stats: Counters,
}

/// The pool's counters, in the form they are kept in.
///
/// `Relaxed` throughout, and loaded back only by [`Pool::stats`]: no other read
/// in this file touches one, so they carry no ordering for anything else to
/// depend on. [`PoolStats`] is the shape they are handed out in.
#[derive(Default)]
struct Counters {
    chunks_run_by_driver: AtomicU64,
    chunks_run_by_workers: AtomicU64,
    steals: AtomicU64,
    steal_failures: AtomicU64,
    steal_retries: AtomicU64,
    parks: AtomicU64,
    longest_queue: AtomicU64,
    submissions: AtomicU64,
}

/// A reading of a [`Pool`]'s counters, from [`Pool::stats`].
///
/// Every field counts up from the moment the pool was built, or from the last
/// [`Pool::reset_stats`] — except [`longest_queue`](Self::longest_queue), which
/// is a high-water mark rather than a total.
///
/// **A reading taken while a `par_for` is in flight is torn across its
/// fields.** The counters are separate relaxed atomics, loaded one after
/// another with nothing holding them still, so the fields of one `PoolStats`
/// need not describe any single instant. That is deliberate and not a gap: a
/// lock would put the observer inside the thing it is observing, and
/// instrumentation that changes the schedule ends up measuring itself. **Read
/// it between submissions**, where the chunk counters are settled — the
/// idleness ones never are, because workers go on searching and parking after
/// a call has returned, whether or not anybody asks them to.
///
/// Between submissions, [`chunks_run_by_driver`](Self::chunks_run_by_driver)
/// plus [`chunks_run_by_workers`](Self::chunks_run_by_workers) is exactly the
/// number of chunks every completed `par_for` split into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PoolStats {
    /// Chunks the calling thread ran itself: the whole of every inline call,
    /// plus the ones it took back off its own end of the deque while the
    /// workers were busy, plus the ones a full deque refused.
    pub chunks_run_by_driver: u64,
    /// Chunks a worker thread ran.
    ///
    /// Counted before the chunk runs rather than after, which is what puts it
    /// inside the release the worker does on the outstanding-chunk count: a
    /// driver that has seen its `par_for` finish has seen these too, so the sum
    /// rule above holds the moment the call returns.
    pub chunks_run_by_workers: u64,
    /// Chunks a worker took off the shared deque.
    ///
    /// A stolen chunk is run by the thief that won it and by nothing else, so
    /// this and [`chunks_run_by_workers`](Self::chunks_run_by_workers) agree
    /// today. They are still two quantities counted at two sites — a steal is
    /// an event on the deque, running a chunk is an event on a worker — and a
    /// pool that ever put a stolen chunk anywhere but straight into the thief's
    /// hands would part them.
    pub steals: u64,
    /// Searches by a worker that found the deque genuinely empty.
    ///
    /// The reading that precedes a park, and the one that says a pool is
    /// oversized for what it is being fed. Losing a race for an item is *not*
    /// counted here — see [`steal_retries`](Self::steal_retries).
    pub steal_failures: u64,
    /// Times a worker read an item and then lost the exchange for it.
    ///
    /// Split from [`steal_failures`](Self::steal_failures) because folding the
    /// two together would report a deque busy enough that thieves collide over
    /// it as an idle one — the opposite reading. A lost exchange means somebody
    /// else took an item that was really there, so the search goes round again
    /// rather than ending; this counts contention on the stealing end, not
    /// idleness.
    pub steal_retries: u64,
    /// Times a worker blocked on the pool's condvar.
    ///
    /// One per wait, so a worker that woke, saw no new submission and went
    /// straight back down counts twice, and one that found work without ever
    /// sleeping does not count at all.
    pub parks: u64,
    /// The most chunks any one submission pushed onto the deque.
    ///
    /// **Not an instantaneous queue depth**, which nothing here samples: the
    /// workers steal while the driver is still pushing, so the deque may never
    /// have held anything like this many at once — and for the same reason a
    /// submission can push *more* chunks than the deque has slots, when thieves
    /// free them faster than the driver fills them. What it measures is the
    /// largest burst a single `par_for` handed to the workers, which is an
    /// upper bound on the depth that call reached and the cheapest honest one:
    /// a true peak occupancy needs somebody watching the deque rather than
    /// somebody counting pushes.
    pub longest_queue: u64,
    /// `par_for` calls that queued chunks for the workers.
    ///
    /// A call that ran inline — a pool with no workers, or a split that came
    /// out as a single chunk — queues nothing and is not counted, so this is
    /// how often the pool was actually asked to be a pool.
    pub submissions: u64,
}

impl Counters {
    /// Loads every counter, in the order they are declared. Torn by
    /// construction; see [`PoolStats`].
    fn snapshot(&self) -> PoolStats {
        PoolStats {
            chunks_run_by_driver: self.chunks_run_by_driver.load(Ordering::Relaxed),
            chunks_run_by_workers: self.chunks_run_by_workers.load(Ordering::Relaxed),
            steals: self.steals.load(Ordering::Relaxed),
            steal_failures: self.steal_failures.load(Ordering::Relaxed),
            steal_retries: self.steal_retries.load(Ordering::Relaxed),
            parks: self.parks.load(Ordering::Relaxed),
            longest_queue: self.longest_queue.load(Ordering::Relaxed),
            submissions: self.submissions.load(Ordering::Relaxed),
        }
    }

    /// Zeroes every counter, one at a time and for the same reason.
    fn reset(&self) {
        self.chunks_run_by_driver.store(0, Ordering::Relaxed);
        self.chunks_run_by_workers.store(0, Ordering::Relaxed);
        self.steals.store(0, Ordering::Relaxed);
        self.steal_failures.store(0, Ordering::Relaxed);
        self.steal_retries.store(0, Ordering::Relaxed);
        self.parks.store(0, Ordering::Relaxed);
        self.longest_queue.store(0, Ordering::Relaxed);
        self.submissions.store(0, Ordering::Relaxed);
    }
}

/// The state a parked worker wakes on.
struct Sleep {
    /// Bumped by every submission. A worker reads it, searches once more, and
    /// only then sleeps *while it is unchanged* — so a submission that lands
    /// between the search and the sleep is either found by the search or
    /// changes the number, and a worker cannot park beside work it never saw.
    submissions: u64,
    /// Workers **blocked on the condvar**, not workers that have decided to
    /// block.
    ///
    /// The distinction is the whole value of the number. Counted around the
    /// wait rather than before the predicate, so a worker whose predicate is
    /// already false — a submission landed while it was deciding — is never
    /// counted, and one that is counted cannot do anything until it is woken.
    /// Counted before the predicate it was the weaker claim "has decided to
    /// park", and a test that waited for it to reach the worker count could
    /// still be racing a worker on its way back out to steal.
    ///
    /// Maintained under the same lock the sleep waits on, which is what makes
    /// it exact rather than a hint. So a submission that finds it zero can skip
    /// the broadcast — nobody is listening — and a test can wait for the pool
    /// to be genuinely asleep before asserting anything about it.
    parked: usize,
    shutdown: bool,
}

/// One chunk of one `par_for`: which chunk, and the job it belongs to.
///
/// Self-describing on purpose. A queue entry that only carried an index would
/// need the job in a shared slot beside the deque, and a worker would have to
/// trust that the slot still holds the job its index came from.
///
/// The job pointer is an [`AtomicPtr`] rather than a `*const Job` so that a
/// buffer of these stays `Send` without an unsafe impl to justify. Nothing
/// contends on it: the driver writes it before the entry is pushed and every
/// read is ordered by the push/steal pair, which is why both accesses are
/// `Relaxed`.
struct Chunk {
    job: AtomicPtr<Job>,
    index: usize,
}

/// A `par_for` in flight, with the caller's types erased so a worker can run a
/// chunk of it without knowing them.
///
/// Lives on the calling thread's stack for exactly as long as `remaining` is
/// non-zero. Every field is either read-only or interior-mutable, because
/// workers reach it through a shared reference.
struct Job {
    /// Runs one chunk of `payload`. See [`run_split`].
    run: unsafe fn(payload: *const (), index: usize),
    payload: *const (),
    /// Chunks not yet finished. **The last thing a worker touches**, released
    /// so that everything the chunk wrote is visible to the driver the moment
    /// it reads zero — which is what makes the borrows in `payload` safe to
    /// hand back to the caller.
    remaining: AtomicUsize,
    /// The panic to re-raise, and the chunk it came from. See the module docs
    /// for why the lowest index wins.
    panic: Mutex<Option<(usize, Box<dyn Any + Send>)>>,
}

/// The typed half of a `par_for`, pointed at by [`Job::payload`].
///
/// Raw rather than borrowed, because the whole point is that these outlive the
/// borrow checker's view of them for the duration of the call. The safety
/// argument is in [`Pool::par_for`], which is the only thing that builds one.
struct Split<T, F> {
    data: *mut T,
    len: usize,
    chunk: usize,
    f: *const F,
}

impl fmt::Debug for Pool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pool")
            .field("workers", &self.workers)
            .finish_non_exhaustive()
    }
}

/// The pool's own locks never wrap caller code, so the only way one is poisoned
/// is a panic in this file — which would be a bug here rather than something a
/// caller can do anything about, and refusing to lock afterwards would turn it
/// into a hang. The state is still consistent, so it is taken as it stands.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Pool {
    /// Builds a pool sized to the machine, through `spawner`.
    ///
    /// **One fewer worker than [`Spawn::parallelism`]**, because the thread
    /// that calls `par_for` runs chunks too: asking for as many workers as
    /// there are cores oversubscribes by exactly one, and the one it
    /// oversubscribes is the thread the caller cares about.
    ///
    /// A spawner with no threads gives a pool with no workers, which runs
    /// everything inline and is a whole answer rather than a degraded one — see
    /// the [module docs](self).
    ///
    /// # Errors
    ///
    /// [`SpawnError`] if a spawner that promised threads then refused one. Any
    /// workers already started are shut down before this returns, so a failed
    /// build leaves nothing running.
    pub fn new(spawner: &dyn Spawn) -> Result<Self, SpawnError> {
        Self::with_workers(spawner, spawner.parallelism().get() - 1)
    }

    /// Builds a pool with a chosen number of workers.
    ///
    /// For a caller that knows what else is running — the design sizes the pool
    /// at cores minus the pinned pipeline threads, and only the topology knows
    /// how many those are — and for a test that wants a worker count the
    /// machine cannot change.
    ///
    /// `workers` is a request: a spawner with no threads answers it with zero,
    /// because [`Spawn::spawn`] would refuse every one of them. Ask
    /// [`workers`](Self::workers) what the pool actually got.
    ///
    /// # Errors
    ///
    /// As [`new`](Self::new).
    pub fn with_workers(spawner: &dyn Spawn, workers: usize) -> Result<Self, SpawnError> {
        let (queue, thieves) = deque::deque(QUEUE_CAPACITY);
        let mut pool = Self {
            shared: Arc::new(Shared {
                thieves,
                sleep: Mutex::new(Sleep {
                    submissions: 0,
                    parked: 0,
                    shutdown: false,
                }),
                wake: Condvar::new(),
                stats: Counters::default(),
            }),
            queue,
            chunks: Vec::new(),
            workers: 0,
        };
        if !spawner.threaded() {
            return Ok(pool);
        }
        for _ in 0..workers {
            let shared = Arc::clone(&pool.shared);
            // The `?` drops `pool`, whose `Drop` shuts down whatever did start:
            // a half-built pool must not leave threads behind.
            spawner.spawn(WORKER_NAME, Box::new(move || work(&shared)))?;
            pool.workers += 1;
        }
        Ok(pool)
    }

    /// How many worker threads this pool actually has, not counting the thread
    /// that drives it.
    ///
    /// Zero means every `par_for` runs inline.
    #[must_use]
    pub fn workers(&self) -> usize {
        self.workers
    }

    /// What the pool has done since it was built, or since the last
    /// [`reset_stats`](Self::reset_stats).
    ///
    /// This is how a phase that adopts the pool shows the adoption helped:
    /// chunks that reached a worker rather than the driver, searches that came
    /// back empty, workers that went to sleep. Cheap enough to read every
    /// frame — one relaxed load per field, and nothing a `par_for` waits on —
    /// and worth reading **between** submissions, for the reason
    /// [`PoolStats`] gives.
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        self.shared.stats.snapshot()
    }

    /// Zeroes every counter, so the next reading covers one phase rather than
    /// the whole run.
    ///
    /// Between submissions, again — and here there is a second reason as well
    /// as [`PoolStats`]'s: a reset that lands mid-call clears the chunks that
    /// have already run while the ones still running go on to be counted, so
    /// the driver-plus-workers sum is short for that call and only that call.
    pub fn reset_stats(&self) {
        self.shared.stats.reset();
    }

    /// Runs `f` over `items` in chunks of `chunk`, in parallel where there are
    /// workers to do it and inline where there are not.
    ///
    /// `f` is called once per chunk with the index of the chunk's first item
    /// and a `&mut` slice of just that chunk — the index is what lets a chunk
    /// address the *other* SoA arrays it is reading, which is the shape every
    /// caller in the design has.
    ///
    /// **The boundaries are the caller's and nothing else's.** Chunk `i` is
    /// always `items[i * chunk ..]` capped at `chunk` items, whatever the
    /// worker count, which is what the determinism rule needs: the same input
    /// at `--threads 1` and `--threads N` reaches the same closure calls. A
    /// `chunk` of zero is read as one.
    ///
    /// `chunk` is also the serial cutoff, and deliberately not a heuristic: one
    /// chunk means one call on this thread, so a caller that does not want a
    /// two-hundred-entity system on the pool passes a chunk length that does
    /// not split it. The pool does not guess at a size only the caller's data
    /// knows.
    ///
    /// # Panics
    ///
    /// If a chunk panics, so does this — after every other chunk has run, with
    /// the panic from the lowest-numbered chunk that had one. The pool itself
    /// is unharmed and the next call works.
    pub fn par_for<T, F>(&mut self, items: &mut [T], chunk: usize, f: F)
    where
        T: Send,
        F: Fn(usize, &mut [T]) + Sync,
    {
        let chunk = chunk.max(1);
        let len = items.len();
        let chunks = len.div_ceil(chunk);
        if chunks == 0 {
            return;
        }
        let split = Split {
            // The length first: taking the pointer is the last thing done with
            // `items`, so nothing reborrows the slice while the chunks hold
            // pointers derived from it.
            len,
            data: items.as_mut_ptr(),
            chunk,
            f: &raw const f,
        };
        let job = Job {
            run: run_split::<T, F>,
            payload: (&raw const split).cast(),
            remaining: AtomicUsize::new(chunks),
            panic: Mutex::new(None),
        };

        if self.workers == 0 || chunks == 1 {
            for index in 0..chunks {
                // SAFETY: `job` is alive for this whole loop, every index is
                // one of its chunks, and each is run once because this is the
                // only thing running them — nothing was queued.
                unsafe { run_one(&job, index) };
            }
            // Once for the whole loop rather than once per turn round it:
            // nothing else can be adding to this counter, because nothing else
            // is running any of these chunks.
            self.shared
                .stats
                .chunks_run_by_driver
                .fetch_add(chunks as u64, Ordering::Relaxed);
        } else {
            self.run_in_parallel(&job, chunks);
        }

        // Every chunk has run: `remaining` reached zero, and a chunk decrements
        // it only after it has finished. So this is the only reference left,
        // and the panic — if there was one — belongs to the calling thread now.
        if let Some((_, panic)) = lock(&job.panic).take() {
            resume_unwind(panic);
        }
    }

    /// Queues `chunks` chunks of `job`, then works alongside the pool until
    /// they are all done.
    ///
    /// Split out from [`par_for`](Self::par_for) because it is the half that
    /// does not need the caller's types, so the unsafe reasoning about the
    /// erased job sits in one place rather than inside a generic function.
    fn run_in_parallel(&mut self, job: &Job, chunks: usize) {
        // Filled before anything is pushed, so the buffer cannot reallocate
        // while the queue holds pointers into it.
        self.chunks.clear();
        self.chunks.reserve(chunks);
        for index in 0..chunks {
            self.chunks.push(Chunk {
                job: AtomicPtr::new(core::ptr::from_ref(job).cast_mut()),
                index,
            });
        }

        // Kept on the stack and folded into the counters once each: the driver
        // is the only thread that can touch either number, so what a call costs
        // the counters is a handful of atomic writes rather than one per chunk.
        let mut by_driver = 0_u64;
        let mut queued = 0_u64;
        for entry in &self.chunks {
            if self.queue.push(NonNull::from(entry)).is_err() {
                by_driver += 1;
                // The queue is full. Running it here keeps the accounting
                // exact — every chunk runs exactly once — where dropping it
                // would hang the wait below forever.
                //
                // SAFETY: `job` outlives this call, `entry.index` is one of its
                // chunks, and this entry never reached the queue so nothing
                // else can run it.
                unsafe { run_one(job, entry.index) };
            } else {
                queued += 1;
            }
        }
        self.shared
            .stats
            .submissions
            .fetch_add(1, Ordering::Relaxed);
        self.shared
            .stats
            .longest_queue
            .fetch_max(queued, Ordering::Relaxed);

        let parked = {
            let mut sleep = lock(&self.shared.sleep);
            sleep.submissions += 1;
            sleep.parked
        };
        if parked > 0 {
            // Skipped when nobody is asleep, which is the steady state of a
            // pool being fed every frame. Exact rather than a guess: a worker
            // counts itself parked under this same lock, before it can miss
            // the number above.
            self.shared.wake.notify_all();
        }

        // Acquire, pairing with the release in `run_one`: reading zero here is
        // what tells the caller its `&mut [T]` is its own again, so every
        // chunk's writes have to be visible by then.
        while job.remaining.load(Ordering::Acquire) > 0 {
            match self.queue.pop() {
                // SAFETY: a chunk is in the queue exactly once, so taking it
                // out is what makes running it exclusive; `job` outlives the
                // wait by the argument above.
                Some(chunk) => {
                    by_driver += 1;
                    unsafe { run_chunk(chunk) };
                }
                // Nothing left to take, and workers still finishing what they
                // took. Yielding rather than spinning, because on a machine
                // with fewer cores than workers the thread we are waiting for
                // may be waiting for this one's timeslice.
                None => std::thread::yield_now(),
            }
        }
        self.shared
            .stats
            .chunks_run_by_driver
            .fetch_add(by_driver, Ordering::Relaxed);
        // The descriptors are dead the moment the job is: nothing may hold a
        // pointer into this buffer past here, and clearing says so.
        self.chunks.clear();
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        {
            let mut sleep = lock(&self.shared.sleep);
            sleep.shutdown = true;
        }
        // Unconditionally, unlike a submission: shutdown happens once, so
        // there is nothing to save by asking who is listening, and one fewer
        // case to be wrong about. Sent after the lock's release, so a worker
        // between "read the flag" and "wait" cannot miss both. Workers hold
        // their own `Arc` of the shared state, so it outlives this drop for as
        // long as they need it.
        self.shared.wake.notify_all();
    }
}

/// A pool worker: steal, run, and sleep when there is nothing.
fn work(shared: &Shared) {
    loop {
        while let Some(chunk) = shared.steal() {
            // SAFETY: a chunk is handed out by exactly one successful steal,
            // and its job is alive because the driver waits for every chunk to
            // finish before returning.
            unsafe { run_stolen(shared, chunk) };
        }

        let submissions = {
            let sleep = lock(&shared.sleep);
            if sleep.shutdown {
                return;
            }
            sleep.submissions
        };
        // Between the search above and reading that number, a submission may
        // have landed; between reading it and sleeping below, another may.
        // This search covers the first, and sleeping only while the number is
        // unchanged covers the second — the bump happens under the same lock
        // the sleep waits on, so there is no gap between them.
        if let Some(chunk) = shared.steal() {
            // SAFETY: as above.
            unsafe { run_stolen(shared, chunk) };
            continue;
        }

        let mut sleep = lock(&shared.sleep);
        while !sleep.shutdown && sleep.submissions == submissions {
            // Around the wait, not around the loop: a thread that sees `parked`
            // reach the worker count has to have taken this lock, and every
            // worker it counted is *inside* the wait rather than on its way to
            // deciding. Counted outside, a worker whose predicate was already
            // false was counted and then went straight back out to steal — so a
            // test that waited for the pool to be asleep could still be racing
            // one, which it usually won and occasionally lost.
            shared.stats.parks.fetch_add(1, Ordering::Relaxed);
            sleep.parked += 1;
            sleep = shared
                .wake
                .wait(sleep)
                .unwrap_or_else(PoisonError::into_inner);
            sleep.parked -= 1;
        }
        if sleep.shutdown {
            return;
        }
    }
}

impl Shared {
    /// One chunk, or `None` when there is genuinely nothing there.
    ///
    /// A lost exchange is not "nothing": it means somebody else took an item
    /// that existed, so this retries rather than reporting an empty deque and
    /// going to sleep beside a full one. It terminates because every retry is
    /// another thread having made progress.
    fn steal(&self) -> Option<NonNull<Chunk>> {
        loop {
            match self.thieves.steal() {
                Steal::Task(chunk) => {
                    self.stats.steals.fetch_add(1, Ordering::Relaxed);
                    return Some(chunk);
                }
                Steal::Empty => {
                    // The search, not the attempt: one call that spun through
                    // a dozen lost exchanges and then found the deque empty is
                    // one failure here and a dozen retries below.
                    self.stats.steal_failures.fetch_add(1, Ordering::Relaxed);
                    return None;
                }
                Steal::Retry => {
                    self.stats.steal_retries.fetch_add(1, Ordering::Relaxed);
                    core::hint::spin_loop();
                }
            }
        }
    }
}

/// Runs a chunk a worker won, and counts it as that worker's.
///
/// **The one place a worker's chunk is counted**, which is why it is a function
/// and not a line repeated at the two places a worker takes one: a count that
/// lived at both would go on reading right with either copy deleted.
///
/// Counted *before* the chunk runs, so that the increment is published by the
/// release [`run_one`] does on the outstanding-chunk count — a driver whose
/// call has returned has therefore seen every one of them, which is what makes
/// the driver-plus-workers sum hold the moment `par_for` returns.
///
/// # Safety
///
/// As [`run_chunk`], whose contract this passes straight through.
unsafe fn run_stolen(shared: &Shared, chunk: NonNull<Chunk>) {
    shared
        .stats
        .chunks_run_by_workers
        .fetch_add(1, Ordering::Relaxed);
    // SAFETY: the caller's contract.
    unsafe { run_chunk(chunk) };
}

/// Runs the chunk `chunk` describes.
///
/// # Safety
///
/// `chunk` must point at a live [`Chunk`] whose job is still in flight, and
/// this chunk must not have been run already — running one twice would hand two
/// threads the same `&mut` sub-slice.
unsafe fn run_chunk(chunk: NonNull<Chunk>) {
    // SAFETY: the caller's contract. Shared rather than exclusive: the driver
    // owns the buffer these live in and only reads it while the job is in
    // flight.
    let chunk = unsafe { chunk.as_ref() };
    // SAFETY: the driver stored this pointer before the entry was queued, and
    // the job outlives every chunk of it by the caller's contract.
    let job = unsafe { &*chunk.job.load(Ordering::Relaxed) };
    // SAFETY: as above; the index came from the same entry as the job.
    unsafe { run_one(job, chunk.index) };
}

/// Runs chunk `index` of `job`, wherever this is called from.
///
/// **The one implementation of "run a chunk"**: the inline driver and the
/// workers both come through here, so there is no second copy for the two modes
/// to disagree in.
///
/// # Safety
///
/// `job` must be in flight, `index` must be one of its chunks, and no other
/// thread may be running this same chunk.
unsafe fn run_one(job: &Job, index: usize) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the caller's contract, which is exactly `run_split`'s.
        unsafe { (job.run)(job.payload, index) };
    }));
    if let Err(panic) = outcome {
        let mut first = lock(&job.panic);
        if first.as_ref().is_none_or(|(earlier, _)| index < *earlier) {
            *first = Some((index, panic));
        }
    }
    // Release, and **the last touch of `job`** — the driver's stack frame is
    // gone the moment this reaches zero.
    job.remaining.fetch_sub(1, Ordering::Release);
}

/// Runs chunk `index` of the `par_for` described by `payload`.
///
/// This is what [`Job::run`] points at, one instantiation per `(T, F)`, which
/// is how a worker calls a closure whose type it cannot name.
///
/// # Safety
///
/// `payload` must point at a live `Split<T, F>` whose `data`, `len` and `f` are
/// still valid, `index` must be below the chunk count that `len` and `chunk`
/// imply, and no other thread may be running this same `index`.
unsafe fn run_split<T, F>(payload: *const (), index: usize)
where
    T: Send,
    F: Fn(usize, &mut [T]) + Sync,
{
    // SAFETY: the caller's contract. Shared, and `Split` is never mutated after
    // it is built, so every chunk may hold one of these at once.
    let split = unsafe { &*payload.cast::<Split<T, F>>() };
    let start = index * split.chunk;
    let len = split.chunk.min(split.len - start);
    // SAFETY: chunk boundaries partition `0 .. len` — chunk `i` owns
    // `i * chunk` up to `chunk` items, and `start < split.len` because `index`
    // is below the chunk count — so this sub-slice is disjoint from every other
    // chunk's, and the caller's contract says no other thread holds this one.
    // The original `&mut [T]` is borrowed by `par_for` for the whole call, so
    // nothing outside the job can reach these items either.
    let items = unsafe { core::slice::from_raw_parts_mut(split.data.add(start), len) };
    // SAFETY: `f` lives in `par_for`'s frame, which outlives the job, and
    // `F: Sync` is what makes a shared reference to it legal here.
    let f = unsafe { &*split.f };
    f(start, items);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spawn::Inline;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    #[cfg(not(target_arch = "wasm32"))]
    use crate::spawn::{Threads, Work};

    /// Long enough that a slow machine never sees it, short enough that a pool
    /// which stopped handing out work fails rather than hangs.
    const DEADLINE: Duration = Duration::from_secs(60);

    /// A pool with real workers, at a count the machine cannot change — a
    /// single-core runner would otherwise turn every parallel assertion into a
    /// test of the inline path.
    #[cfg(not(target_arch = "wasm32"))]
    fn threaded(workers: usize) -> Pool {
        Pool::with_workers(&Threads, workers).expect("native threads")
    }

    /// Blocks until every worker is parked, and hands back the lock they must
    /// reacquire to stop being parked.
    ///
    /// **The precondition for any assertion about waking them**, and it is not
    /// optional: a freshly built pool's workers spend their first moments
    /// spinning through the steal loop, where they find work — and the shutdown
    /// flag — without ever being woken. A test that submits into that window
    /// passes whether or not the pool ever signals anything, which is a check
    /// that cannot fail. Measured rather than assumed: with this wait removed,
    /// a `notify_all` deleted from either the submission or the shutdown path
    /// leaves the whole suite green.
    ///
    /// **Returning the guard is what makes the answer stay true.** Dropping it
    /// leaves only "every worker was inside the wait a moment ago", and a
    /// worker already notified is still counted while it queues for this very
    /// lock — so a caller reading counters after that can be racing one on its
    /// way back out to steal. Measured: with the guard dropped before the read,
    /// `resetting_the_counters_zeroes_every_one_of_them` fails about once in
    /// two thousand runs under load, always with a steal failure the reset had
    /// already cleared. Held, no worker can leave the wait at all, so a caller
    /// holding it sees a pool that genuinely cannot move.
    #[cfg(not(target_arch = "wasm32"))]
    fn wait_until_parked(pool: &Pool) -> MutexGuard<'_, Sleep> {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let sleep = lock(&pool.shared.sleep);
            if sleep.parked >= pool.workers() {
                return sleep;
            }
            let parked = sleep.parked;
            drop(sleep);
            assert!(
                Instant::now() < deadline,
                "only {} of {} workers ever parked",
                parked,
                pool.workers(),
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_pool_with_no_threads_has_no_workers_and_still_runs_everything() {
        let mut pool = Pool::new(&Inline).expect("Inline never fails to not spawn");
        assert_eq!(pool.workers(), 0);

        // Asking a spawner with no threads for four of them is answered, not
        // refused: the whole degradation rule is that a runtime without threads
        // is served rather than failed.
        assert_eq!(
            Pool::with_workers(&Inline, 4)
                .expect("asking Inline for workers is not an error")
                .workers(),
            0,
        );

        let mut items = vec![0_u32; 100];
        pool.par_for(&mut items, 7, |start, chunk| {
            for (offset, item) in chunk.iter_mut().enumerate() {
                *item = (start + offset) as u32;
            }
        });
        assert_eq!(items, (0..100).collect::<Vec<u32>>());
    }

    /// **Every item is touched exactly once.** A chunk run twice would double
    /// an increment; one never run would leave a zero. Both are invisible to a
    /// test that only checks the sum, so this checks the items.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn every_item_is_touched_exactly_once() {
        const ITEMS: usize = if cfg!(miri) { 200 } else { 10_000 };

        let mut pool = threaded(3);
        let mut items = vec![0_u32; ITEMS];
        pool.par_for(&mut items, 16, |_, chunk| {
            for item in chunk {
                *item += 1;
            }
        });

        assert!(
            items.iter().all(|&touched| touched == 1),
            "{} items were not touched exactly once",
            items.iter().filter(|&&touched| touched != 1).count(),
        );
    }

    /// **A `par_for` with more chunks than the queue holds still runs each of
    /// them exactly once.** Past `QUEUE_CAPACITY` the pushes start being
    /// refused and the driver runs those chunks as it goes — the path where a
    /// chunk is easiest to drop on the floor or to run twice, and the only one
    /// that has neither the queue nor the wait loop keeping count.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_par_for_bigger_than_the_queue_loses_no_chunk() {
        let chunks = QUEUE_CAPACITY + QUEUE_CAPACITY / 2;
        let mut pool = threaded(3);
        let mut items = vec![0_u32; chunks];

        // One item per chunk, so the chunk count is the item count and the
        // queue is overrun by half again.
        pool.par_for(&mut items, 1, |start, chunk| {
            chunk[0] = start as u32 + 1;
        });

        assert_eq!(items.len(), chunks);
        let wrong: Vec<_> = items
            .iter()
            .enumerate()
            .filter(|&(index, &written)| written != index as u32 + 1)
            .collect();
        assert!(
            wrong.is_empty(),
            "chunks that did not run exactly once: {wrong:?}"
        );
    }

    /// The chunk boundaries are the caller's: every chunk starts where the
    /// previous ended, they cover the slice exactly once, and only the last one
    /// is short. Collected rather than assumed, because a bad `start` would
    /// still leave every item touched once.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_chunks_partition_the_slice_at_the_boundaries_the_caller_asked_for() {
        let mut pool = threaded(3);
        let mut items = vec![0_usize; 50];
        let seen = Mutex::new(Vec::new());
        pool.par_for(&mut items, 8, |start, chunk| {
            lock(&seen).push((start, chunk.len()));
        });

        let mut seen = lock(&seen).clone();
        seen.sort_unstable();
        assert_eq!(
            seen,
            vec![(0, 8), (8, 8), (16, 8), (24, 8), (32, 8), (40, 8), (48, 2)],
        );
    }

    /// **The two modes agree**, which is the whole promise: the same call over
    /// the same data, once on a pool with workers and once on a pool without,
    /// has to leave the same bytes behind. A `par_for` whose split depended on
    /// the worker count would fail here and nowhere else.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn threaded_and_inline_pools_compute_the_same_answer() {
        const ITEMS: usize = if cfg!(miri) { 200 } else { 5_000 };

        // Each item is a function of its own index, so a chunk that read the
        // wrong range writes the wrong values — and the boundaries the closure
        // was handed are recorded beside the values, because a split that
        // depended on the worker count could still reach the same bytes and
        // would then be invisible to a comparison of the data alone.
        let run = |pool: &mut Pool| {
            let mut items = vec![0_u64; ITEMS];
            let boundaries = Mutex::new(Vec::new());
            pool.par_for(&mut items, 16, |start, chunk: &mut [u64]| {
                lock(&boundaries).push((start, chunk.len()));
                for (offset, item) in chunk.iter_mut().enumerate() {
                    let index = (start + offset) as u64;
                    *item = index.wrapping_mul(2_654_435_761) ^ (index << 7);
                }
            });
            let mut boundaries = boundaries.into_inner().expect("no chunk panicked");
            boundaries.sort_unstable();
            (items, boundaries)
        };

        let (threaded_items, threaded_split) = run(&mut threaded(3));
        let (inline_items, inline_split) = run(&mut Pool::new(&Inline).expect("Inline"));

        assert_eq!(
            threaded_split, inline_split,
            "the two modes split differently"
        );
        assert_eq!(threaded_items, inline_items);
        assert_ne!(threaded_items[ITEMS - 1], 0, "the closure never ran");
    }

    /// **More than one thread actually runs the work**, which nothing else
    /// here checks: a `par_for` that ran every chunk on the caller would pass
    /// every other test in this file and be a pool in name only.
    ///
    /// Each chunk waits for a second thread to arrive before it finishes, so
    /// the assertion does not depend on the pool being slow enough to observe.
    /// The wait is bounded, so a pool whose workers never arrive fails red
    /// instead of hanging.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_work_reaches_more_than_the_calling_thread() {
        let mut pool = threaded(2);
        assert_eq!(pool.workers(), 2);
        // Parked first, so the chunks below can only reach a worker that was
        // woken by the submission.
        drop(wait_until_parked(&pool));

        let arrived = AtomicUsize::new(0);
        let threads = Mutex::new(std::collections::HashSet::new());
        let mut items = vec![0_u8; 64];
        pool.par_for(&mut items, 8, |_, _| {
            lock(&threads).insert(std::thread::current().id());
            arrived.fetch_add(1, Ordering::SeqCst);
            let deadline = Instant::now() + DEADLINE;
            while arrived.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
                std::thread::yield_now();
            }
        });

        assert!(
            lock(&threads).len() >= 2,
            "every chunk ran on one thread: {:?}",
            lock(&threads),
        );
    }

    /// A panicking chunk does not stop the others, does not poison the pool,
    /// and does not hang the caller — the three ways this could go wrong, and
    /// the deadline is what turns the third into a red test rather than a
    /// wedged run.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_panicking_chunk_leaves_the_pool_fit_for_the_next_call() {
        let mut pool = threaded(3);
        drop(wait_until_parked(&pool));
        let mut items = vec![0_u32; 64];

        let ran = AtomicUsize::new(0);
        // **The panicking chunk has to run on a worker**, or this tests the
        // inline path wearing a pool's clothes — and it did, until a mutation
        // showed it: chunk zero is the oldest, so a thief takes it first, but
        // the driving thread pops from the other end and would usually reach it
        // first anyway. So every other chunk waits for chunk zero to have run,
        // which leaves the driver stuck in whichever chunk it took and makes a
        // worker the only thing that can move the call forward.
        let failing_chunk_ran = AtomicBool::new(false);
        let panicked = catch_unwind(AssertUnwindSafe(|| {
            pool.par_for(&mut items, 8, |start, chunk| {
                ran.fetch_add(1, Ordering::SeqCst);
                for item in chunk.iter_mut() {
                    *item = 1;
                }
                if start == 0 {
                    failing_chunk_ran.store(true, Ordering::SeqCst);
                    panic!("chunk zero says no");
                }
                let deadline = Instant::now() + DEADLINE;
                while !failing_chunk_ran.load(Ordering::SeqCst) && Instant::now() < deadline {
                    std::thread::yield_now();
                }
            });
        }));

        let panic = panicked.expect_err("the panic must reach the caller");
        assert_eq!(
            // A message with nothing to format is a `&'static str` payload
            // rather than a `String`, which is a `std` detail this has to know
            // to assert on the message at all.
            panic.downcast_ref::<&str>().copied(),
            Some("chunk zero says no"),
            "a different panic arrived",
        );
        assert_eq!(ran.load(Ordering::SeqCst), 8, "chunks stopped running");
        assert!(
            items.iter().all(|&written| written == 1),
            "a chunk after the panicking one never wrote its items",
        );

        // And the pool still works, which is what "does not poison" means.
        let mut more = vec![0_u32; 64];
        pool.par_for(&mut more, 8, |_, chunk| {
            for item in chunk {
                *item = 2;
            }
        });
        assert!(more.iter().all(|&written| written == 2));
    }

    /// Both modes report the **same** panic when several chunks fail: the
    /// lowest-numbered one. Without a rule, which panic surfaced would be a
    /// scheduling artefact, and a test asserting on the message would be flaky
    /// rather than wrong.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn every_mode_reports_the_panic_from_the_lowest_chunk() {
        let failing = |pool: &mut Pool| {
            let mut items = vec![0_u32; 64];
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                pool.par_for(&mut items, 8, |start, _| {
                    assert!(start % 16 != 0, "chunk at {start}");
                });
            }));
            *outcome
                .expect_err("a chunk panicked")
                .downcast::<String>()
                .expect("the assertion's message")
        };

        assert_eq!(failing(&mut threaded(3)), "chunk at 0");
        assert_eq!(
            failing(&mut Pool::new(&Inline).expect("Inline")),
            "chunk at 0",
        );
    }

    /// **Dropping the pool ends every worker**, including one parked with
    /// nothing to do — which is the state they are all in by the time a pool is
    /// dropped, and the one a shutdown that only sets a flag would leave asleep
    /// forever.
    ///
    /// The seam detaches its threads, so there is no handle to join: what is
    /// observable is the sentinel each worker holds being dropped, and that is
    /// what this waits for.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn dropping_the_pool_stops_every_worker() {
        struct Sentinel(Arc<AtomicUsize>);
        impl Drop for Sentinel {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        /// Wraps the real spawner and hangs a sentinel on each worker's
        /// closure, so a worker that never returns is a sentinel that is never
        /// dropped.
        #[derive(Debug)]
        struct Watched(Arc<AtomicUsize>);
        impl Spawn for Watched {
            fn threaded(&self) -> bool {
                true
            }
            fn parallelism(&self) -> core::num::NonZeroUsize {
                Threads.parallelism()
            }
            fn spawn(&self, name: &'static str, work: Work) -> Result<(), SpawnError> {
                let sentinel = Sentinel(Arc::clone(&self.0));
                Threads.spawn(
                    name,
                    Box::new(move || {
                        work();
                        drop(sentinel);
                    }),
                )
            }
        }

        let stopped = Arc::new(AtomicUsize::new(0));
        let mut pool = Pool::with_workers(&Watched(Arc::clone(&stopped)), 3).expect("threads");

        // Give them something to do, then let them all park again — see
        // `wait_until_parked` for why the drop below means nothing otherwise.
        let mut items = vec![0_u32; 64];
        pool.par_for(&mut items, 8, |_, chunk| {
            for item in chunk {
                *item = 1;
            }
        });
        drop(wait_until_parked(&pool));
        drop(pool);

        let deadline = Instant::now() + DEADLINE;
        while stopped.load(Ordering::SeqCst) < 3 {
            assert!(
                Instant::now() < deadline,
                "{} of 3 workers were still running a minute after the pool \
                 was dropped",
                3 - stopped.load(Ordering::SeqCst),
            );
            std::thread::yield_now();
        }
    }

    /// A pool built from a spawner that refuses partway leaves nothing running,
    /// and says why. The workers that did start are shut down by the `Drop` on
    /// the error path — which the sentinel is what proves.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_refused_worker_fails_the_build_and_leaves_nothing_behind() {
        #[derive(Debug)]
        struct RefusesTheThird {
            started: AtomicUsize,
            stopped: Arc<AtomicUsize>,
        }
        struct Sentinel(Arc<AtomicUsize>);
        impl Drop for Sentinel {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        impl Spawn for RefusesTheThird {
            fn threaded(&self) -> bool {
                true
            }
            fn parallelism(&self) -> core::num::NonZeroUsize {
                Threads.parallelism()
            }
            fn spawn(&self, name: &'static str, work: Work) -> Result<(), SpawnError> {
                if self.started.fetch_add(1, Ordering::SeqCst) == 2 {
                    return Err(SpawnError::Os {
                        name,
                        source: std::io::Error::other("out of threads"),
                    });
                }
                let sentinel = Sentinel(Arc::clone(&self.stopped));
                Threads.spawn(
                    name,
                    Box::new(move || {
                        work();
                        drop(sentinel);
                    }),
                )
            }
        }

        let stopped = Arc::new(AtomicUsize::new(0));
        let error = Pool::with_workers(
            &RefusesTheThird {
                started: AtomicUsize::new(0),
                stopped: Arc::clone(&stopped),
            },
            4,
        )
        .expect_err("the third worker was refused");
        assert!(matches!(error, SpawnError::Os { name, .. } if name == WORKER_NAME));

        let deadline = Instant::now() + DEADLINE;
        while stopped.load(Ordering::SeqCst) < 2 {
            assert!(
                Instant::now() < deadline,
                "a worker from the failed build is still running",
            );
            std::thread::yield_now();
        }
    }

    /// A pool goes to the thread that will drive it, which is never the thread
    /// that built the topology.
    #[test]
    fn a_pool_moves_to_the_thread_that_drives_it() {
        fn assert_send<T: Send>() {}
        assert_send::<Pool>();
    }

    /// An empty slice is a call that does nothing rather than one that divides
    /// by zero, and a zero chunk length is read as one rather than looping
    /// forever on an empty range.
    #[test]
    fn the_degenerate_splits_do_nothing_surprising() {
        let mut pool = Pool::new(&Inline).expect("Inline");
        let ran = AtomicBool::new(false);

        let mut empty: Vec<u32> = Vec::new();
        pool.par_for(&mut empty, 8, |_, _| ran.store(true, Ordering::SeqCst));
        assert!(!ran.load(Ordering::SeqCst), "an empty slice ran a chunk");

        let mut items = vec![0_u32; 3];
        pool.par_for(&mut items, 0, |start, chunk| {
            assert_eq!(chunk.len(), 1, "a zero chunk length must read as one");
            chunk[0] = start as u32 + 1;
        });
        assert_eq!(items, vec![1, 2, 3]);
    }

    /// **Every chunk of a completed call is counted exactly once, and to the
    /// thread that ran it.** The sum is the whole point of splitting the two:
    /// a driver counter that also caught the workers' chunks, or a worker
    /// counter that missed the ones a full deque handed back, would still look
    /// plausible on its own and would not add up here.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_driver_and_the_workers_between_them_ran_every_chunk() {
        const ITEMS: usize = if cfg!(miri) { 200 } else { 10_000 };
        const CHUNK: usize = 16;

        let mut pool = threaded(3);
        let mut items = vec![0_u32; ITEMS];
        pool.par_for(&mut items, CHUNK, |_, chunk| {
            for item in chunk {
                *item += 1;
            }
        });

        let stats = pool.stats();
        assert_eq!(
            stats.chunks_run_by_driver + stats.chunks_run_by_workers,
            ITEMS.div_ceil(CHUNK) as u64,
            "{stats:?} does not account for every chunk",
        );
        assert_eq!(stats.submissions, 1, "one call, one submission");
    }

    /// A pool with no workers puts **every** chunk on the driver, and touches
    /// nothing else: no deque, no thieves, no sleeping. Comparing the whole
    /// snapshot rather than one field is what makes the second half of that a
    /// real assertion.
    #[test]
    fn a_pool_with_no_workers_puts_every_chunk_on_the_driver() {
        const ITEMS: usize = 50;
        const CHUNK: usize = 8;

        let mut pool = Pool::new(&Inline).expect("Inline");
        let mut items = vec![0_u32; ITEMS];
        pool.par_for(&mut items, CHUNK, |_, chunk| {
            for item in chunk {
                *item += 1;
            }
        });

        assert_eq!(
            pool.stats(),
            PoolStats {
                chunks_run_by_driver: ITEMS.div_ceil(CHUNK) as u64,
                ..PoolStats::default()
            },
        );
    }

    /// **A stolen chunk is counted twice over, once as a steal and once as a
    /// worker running it** — and the two agree, because a thief runs what it
    /// wins and hands it to nobody.
    ///
    /// Each chunk waits for a second thread before finishing, exactly as
    /// `the_work_reaches_more_than_the_calling_thread` does: without that a
    /// driver quick enough to run all eight chunks itself would leave both
    /// counters at zero, and the test would be asserting on the scheduler.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_stolen_chunk_is_counted_as_a_steal_and_as_a_worker_running_it() {
        let mut pool = threaded(2);
        drop(wait_until_parked(&pool));
        pool.reset_stats();

        let arrived = AtomicUsize::new(0);
        let mut items = vec![0_u8; 64];
        pool.par_for(&mut items, 8, |_, _| {
            arrived.fetch_add(1, Ordering::SeqCst);
            let deadline = Instant::now() + DEADLINE;
            while arrived.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
                std::thread::yield_now();
            }
        });

        let stats = pool.stats();
        assert!(
            stats.chunks_run_by_workers > 0,
            "no chunk reached a worker: {stats:?}",
        );
        assert_eq!(
            stats.steals, stats.chunks_run_by_workers,
            "a stolen chunk is run by the thief that won it: {stats:?}",
        );
        assert_eq!(
            stats.chunks_run_by_driver + stats.chunks_run_by_workers,
            items.len().div_ceil(8) as u64,
            "{stats:?} does not account for every chunk",
        );
    }

    /// A worker that finds the deque empty counts the search. Every parked
    /// worker went through at least one — two, in fact — on its way down, so
    /// waiting for them to park is what makes this a count rather than a hope.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_worker_that_finds_nothing_counts_the_search_that_found_it() {
        let pool = threaded(3);
        drop(wait_until_parked(&pool));

        let stats = pool.stats();
        assert!(
            stats.steal_failures >= pool.workers() as u64,
            "{} workers parked without one empty search each: {stats:?}",
            pool.workers(),
        );
    }

    /// **A worker that goes to sleep says so.** Parking is counted under the
    /// same lock the sleep waits on and before the wait itself, which is what
    /// lets `wait_until_parked` stand in for "and now check the counter": a
    /// thread that has seen the parked count reach the worker count cannot be
    /// racing an increment that has not happened.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_worker_with_nothing_left_to_do_parks_and_the_park_is_counted() {
        let pool = threaded(2);
        drop(wait_until_parked(&pool));

        let stats = pool.stats();
        assert!(
            stats.parks >= pool.workers() as u64,
            "{} workers are asleep and {} parks were counted: {stats:?}",
            pool.workers(),
            stats.parks,
        );
    }

    /// The longest queue is the **biggest** burst, not the last one — the
    /// high-water mark a smaller call afterwards must not lower. Both calls
    /// split into fewer chunks than the deque has slots, so every push lands
    /// and the number is exact rather than a lower bound.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_longest_queue_keeps_the_biggest_burst_rather_than_the_last() {
        const BIG: usize = 300;
        const SMALL: usize = 20;
        const { assert!(BIG < QUEUE_CAPACITY, "the big burst has to fit") };

        let mut pool = threaded(2);
        // One item per chunk, so the chunk count is the item count.
        let mut items = vec![0_u32; BIG];
        pool.par_for(&mut items, 1, |_, chunk| chunk[0] += 1);
        assert_eq!(pool.stats().longest_queue, BIG as u64);

        let mut items = vec![0_u32; SMALL];
        pool.par_for(&mut items, 1, |_, chunk| chunk[0] += 1);
        let stats = pool.stats();
        assert_eq!(
            stats.longest_queue, BIG as u64,
            "a smaller burst moved the high-water mark: {stats:?}",
        );
        assert_eq!(stats.submissions, 2, "two calls, two submissions");
    }

    /// **A thief that loses the exchange counts the retry**, which is a
    /// different thing from finding the deque empty: somebody else took an item
    /// that was really there, and the search goes round rather than ending.
    ///
    /// Contention cannot be staged — it needs two thieves inside the same
    /// exchange — so this is a stress test in the shape of the deque's own
    /// `the_last_item_goes_to_exactly_one_taker`: one-item chunks and more
    /// thieves than there is work, submitted until a retry shows up. The
    /// deadline is what makes a pool that never contends a red test rather than
    /// a hang. Not run under miri, where an interpreted spin of this length
    /// would outlast the run it belongs to.
    #[test]
    #[cfg(all(not(target_arch = "wasm32"), not(miri)))]
    fn a_thief_that_loses_the_exchange_counts_the_retry() {
        const ITEMS: usize = 64;

        let mut pool = threaded(4);
        let mut items = vec![0_u32; ITEMS];
        let deadline = Instant::now() + DEADLINE;
        let mut rounds = 0_u64;
        while pool.stats().steal_retries == 0 {
            assert!(
                Instant::now() < deadline,
                "{rounds} rounds of four thieves over one-item chunks and not \
                 one lost exchange: {:?}",
                pool.stats(),
            );
            pool.par_for(&mut items, 1, |_, chunk| chunk[0] += 1);
            rounds += 1;
        }
    }

    /// **Resetting zeroes every counter, not the ones that happened to be
    /// looked at.** Each is driven off zero first — a reset that missed one
    /// would otherwise pass on a field nothing had moved — and the pool is put
    /// back to sleep before the reset, because workers go on searching and
    /// parking after a call returns and would otherwise be counting again
    /// before the snapshot below is taken.
    ///
    /// `steal_retries` is the one field this cannot drive off zero, for the
    /// reason `a_thief_that_loses_the_exchange_counts_the_retry` gives.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn resetting_the_counters_zeroes_every_one_of_them() {
        let mut pool = threaded(2);
        drop(wait_until_parked(&pool));

        // The waiting chunks again, so that a worker is guaranteed to have
        // stolen and run one of them.
        let arrived = AtomicUsize::new(0);
        let mut items = vec![0_u8; 64];
        pool.par_for(&mut items, 8, |_, _| {
            arrived.fetch_add(1, Ordering::SeqCst);
            let deadline = Instant::now() + DEADLINE;
            while arrived.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
                std::thread::yield_now();
            }
        });

        // **A call that cannot leave the calling thread**, because the driver's
        // share of the one above is whatever the workers did not get to first.
        // Two workers took all eight chunks of it on a Windows runner and left
        // `chunks_run_by_driver` at zero, which failed this test's own
        // precondition rather than the reset it is about. A single chunk takes
        // `par_for`'s inline path by construction, so this counter moves on
        // every machine.
        //
        // Asserted as a delta rather than left to the loop below, because on a
        // machine where the driver *does* get chunks off the deque that loop
        // passes either way — it did here with this increment deleted. Only the
        // driver writes this counter and only this thread drives, so `+ 1` is
        // exact rather than a lower bound.
        let before = pool.stats().chunks_run_by_driver;
        let whole = items.len();
        pool.par_for(&mut items, whole, |_, chunk| chunk[0] += 1);
        assert_eq!(
            pool.stats().chunks_run_by_driver,
            before + 1,
            "the inline path did not count its chunk to the driver",
        );
        // Held across every read below: the pool is asleep, and while this
        // guard exists no worker can leave the wait to make it otherwise.
        let asleep = wait_until_parked(&pool);

        let stats = pool.stats();
        for (name, count) in [
            ("chunks_run_by_driver", stats.chunks_run_by_driver),
            ("chunks_run_by_workers", stats.chunks_run_by_workers),
            ("steals", stats.steals),
            ("steal_failures", stats.steal_failures),
            ("parks", stats.parks),
            ("longest_queue", stats.longest_queue),
            ("submissions", stats.submissions),
        ] {
            assert!(count > 0, "{name} was already zero: {stats:?}");
        }

        pool.reset_stats();
        assert_eq!(
            pool.stats(),
            PoolStats::default(),
            "a parked pool's counters moved, or a field was not reset",
        );
        drop(asleep);
    }
}
