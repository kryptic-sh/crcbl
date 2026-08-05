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
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
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
}

/// The state a parked worker wakes on.
struct Sleep {
    /// Bumped by every submission. A worker reads it, searches once more, and
    /// only then sleeps *while it is unchanged* — so a submission that lands
    /// between the search and the sleep is either found by the search or
    /// changes the number, and a worker cannot park beside work it never saw.
    submissions: u64,
    /// Workers currently parked.
    ///
    /// Maintained under the same lock the sleep waits on, which is what makes
    /// it exact rather than a hint: a worker that has decided to park has
    /// already counted itself by the time anything else can take the lock. So a
    /// submission that finds it zero can skip the broadcast — nobody is
    /// listening — and a test can wait for the pool to be genuinely asleep
    /// before asserting anything about waking it, which is otherwise a race the
    /// test would usually win and occasionally lose.
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

        for entry in &self.chunks {
            if self.queue.push(NonNull::from(entry)).is_err() {
                // The queue is full. Running it here keeps the accounting
                // exact — every chunk runs exactly once — where dropping it
                // would hang the wait below forever.
                //
                // SAFETY: `job` outlives this call, `entry.index` is one of its
                // chunks, and this entry never reached the queue so nothing
                // else can run it.
                unsafe { run_one(job, entry.index) };
            }
        }

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
                Some(chunk) => unsafe { run_chunk(chunk) },
                // Nothing left to take, and workers still finishing what they
                // took. Yielding rather than spinning, because on a machine
                // with fewer cores than workers the thread we are waiting for
                // may be waiting for this one's timeslice.
                None => std::thread::yield_now(),
            }
        }
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
            unsafe { run_chunk(chunk) };
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
            unsafe { run_chunk(chunk) };
            continue;
        }

        let mut sleep = lock(&shared.sleep);
        sleep.parked += 1;
        while !sleep.shutdown && sleep.submissions == submissions {
            sleep = shared
                .wake
                .wait(sleep)
                .unwrap_or_else(PoisonError::into_inner);
        }
        sleep.parked -= 1;
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
                Steal::Task(chunk) => return Some(chunk),
                Steal::Empty => return None,
                Steal::Retry => core::hint::spin_loop(),
            }
        }
    }
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

    /// Blocks until every worker is parked.
    ///
    /// **The precondition for any assertion about waking them**, and it is not
    /// optional: a freshly built pool's workers spend their first moments
    /// spinning through the steal loop, where they find work — and the shutdown
    /// flag — without ever being woken. A test that submits into that window
    /// passes whether or not the pool ever signals anything, which is a check
    /// that cannot fail. Measured rather than assumed: with this wait removed,
    /// a `notify_all` deleted from either the submission or the shutdown path
    /// leaves the whole suite green.
    #[cfg(not(target_arch = "wasm32"))]
    fn wait_until_parked(pool: &Pool) {
        let deadline = Instant::now() + DEADLINE;
        while lock(&pool.shared.sleep).parked < pool.workers() {
            assert!(
                Instant::now() < deadline,
                "only {} of {} workers ever parked",
                lock(&pool.shared.sleep).parked,
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
        wait_until_parked(&pool);

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
        wait_until_parked(&pool);
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
        wait_until_parked(&pool);
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
}
