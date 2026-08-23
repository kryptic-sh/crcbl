//! The wasm half of the worker-backend gate — see `web/tools/worker-gate.mjs`.
//!
//! # Why this is an example and not a test
//!
//! What has to be proved is that a **Web Worker** brought up through
//! `crcbl_jobs::workers`'s ABI runs Rust on a stack of its own. Nothing native
//! can observe that, and `cargo test` cannot run a `wasm32` target at all, so
//! the gate is a JS harness driving a real artifact. This is the artifact: a
//! `cdylib` example of this crate, built by `web/build.sh --threads`.
//!
//! An example rather than a cargo feature on the library, because the exports
//! below exist only to be observed. A feature would put them in the library's
//! surface where a demo build could pick them up; an example cannot reach a
//! site artifact by any route. They are also deliberately **not** `__crcbl_*`
//! prefixed: that prefix is the engine's browser ABI, and
//! `web/tools/check-exports.mjs` requires every symbol wearing it to be
//! exported by every demo artifact. These are not that.
//!
//! # What each export is for
//!
//! | Export | Signature | What it answers |
//! | --- | --- | --- |
//! | `gate_threaded` | `() -> u32` | `Spawn::threaded` on `default_spawner()`. `0` before the host announces itself, which is the claim that `threaded()` does not lie. |
//! | `gate_parallelism` | `() -> u32` | `Spawn::parallelism` on the same backend. |
//! | `gate_expected` | `() -> u32` | The checksum a correct run produces. **Read it before any worker starts**: it is computed by running the same chunk work on the calling thread. |
//! | `gate_pool` | `(u32) -> u32` | Build a pool of that many workers through the seam, and answer how many it actually got. |
//! | `gate_run` | `() -> u32` | One `par_for` over the buffer, and its checksum. |
//! | `gate_threads` | `() -> u32` | How many distinct threads have run a chunk. |
//! | `gate_clobbered` | `() -> u32` | `1` if any chunk found its own stack array changed underneath it. |
//! | `gate_tls_shared` | `() -> u32` | `1` if a thread found another thread's value in its own thread-local. |
//!
//! # The two failures this is shaped around
//!
//! **A worker that never has `__stack_pointer` written keeps the main thread's
//! stack, and that failure is silent**: a chunk that merely allocates returns
//! the right answer every time. So the chunk work writes a large array on its
//! own stack, holds it across a window wide enough for another thread to be
//! inside its own copy of the same frame, and then reads every word back and
//! compares. `gate_clobbered` is that comparison, and the checksum `gate_run`
//! answers moves as well.
//!
//! **A worker that never calls `__wasm_init_tls` does not necessarily trap.**
//! `docs/plan/21-jobs.md` records one that did, against a different crate; in
//! this build `__tls_base` is simply left at zero, every worker's thread-locals
//! alias the same address near the start of linear memory, and a
//! `thread_local!` with a `const` initialiser reads and writes it without
//! complaint. Measured, and it defeated this gate's first shape — a
//! "count each thread once" flag was satisfied by one worker setting the shared
//! flag for all of them.
//!
//! So the thread-local holds something only its own thread could have put
//! there: **the address of the caller's stack frame**. A thread that finds a
//! value belonging to a different stack is reading someone else's TLS, which is
//! exactly the failure, and it needs no trap to be visible. That also makes the
//! thread count independent of TLS being correct, since it is the stack address
//! that identifies a thread here.

#[cfg(target_arch = "wasm32")]
mod gate {
    use core::sync::atomic::{AtomicU32, Ordering};
    use std::cell::{Cell, RefCell};

    use crcbl_jobs::{Pool, default_spawner};

    /// Items in the buffer one `gate_run` covers.
    const ITEMS: usize = 64;

    /// Items per chunk. One, so that every worker the pool has can be inside a
    /// chunk at the same time and the stack windows overlap.
    const CHUNK: usize = 1;

    /// Words each chunk writes on its own stack.
    ///
    /// Large enough that two threads sharing one stack region cannot miss each
    /// other, and small enough that several frames fit in the stack
    /// `crcbl_jobs::workers` hands a worker.
    const STACK_PROBE_WORDS: usize = 4096;

    /// How long a chunk holds its stack array before reading it back.
    ///
    /// Not a timing assumption: a window that is too short costs the gate
    /// sensitivity on one chunk, and there are `ITEMS` of them per run and
    /// several runs per gate.
    const HOLD: u32 = 20_000;

    /// How far two frame addresses may be apart and still be the same stack.
    ///
    /// A thread's probe frame sits at the same call depth every time, so within
    /// one thread the spread is a handful of bytes; two threads are on separate
    /// megabyte allocations. Anything between the two separates them, and this
    /// is deliberately nearer the small end.
    const SAME_STACK: u32 = 64 * 1024;

    /// Distinct threads that have run a chunk, identified by their stacks
    /// rather than by anything thread-local — see the module docs.
    static THREADS: AtomicU32 = AtomicU32::new(0);

    /// Set by any chunk that read back a word it did not write.
    static CLOBBERED: AtomicU32 = AtomicU32::new(0);

    /// Set by any chunk that found a frame address it could not have written in
    /// its own thread-local.
    static TLS_SHARED: AtomicU32 = AtomicU32::new(0);

    thread_local! {
        /// A frame address from the first chunk this thread ran, and zero
        /// before that. Only this thread's own stack can produce it, which is
        /// what makes reading someone else's TLS observable.
        static MY_FRAME: Cell<u32> = const { Cell::new(0) };
        /// The pool under test. It belongs to the thread that calls
        /// `gate_pool` and `gate_run`, which is the host's drain thread.
        static POOL: RefCell<Option<Pool>> = const { RefCell::new(None) };
        /// What that pool runs over.
        static BUFFER: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    }

    /// The word a probe writes at `index` for `seed`. Any function of both
    /// would do; what matters is that a chunk can recompute it.
    fn word(seed: u32, index: usize) -> u32 {
        seed.wrapping_mul(0x9e37_79b9)
            .wrapping_add(index as u32)
            .rotate_left(index as u32 % 32)
    }

    /// Records this thread once, and checks that its thread-local is its own.
    ///
    /// `frame` is an address inside the caller's stack frame. The first call
    /// from a thread stores it and counts the thread; every later call from the
    /// same thread must find an address on the same stack, because a
    /// thread-local is per-thread. Finding one from somewhere else means every
    /// worker is reading one shared TLS block, which is what a missing
    /// `__wasm_init_tls` leaves behind.
    fn register_thread(frame: u32) {
        MY_FRAME.with(|mine| {
            let seen = mine.get();
            if seen == 0 {
                mine.set(frame);
                THREADS.fetch_add(1, Ordering::Relaxed);
            } else if seen.abs_diff(frame) > SAME_STACK {
                TLS_SHARED.store(1, Ordering::Relaxed);
            }
        });
    }

    /// Writes a large array on **this thread's stack**, holds it, and reads it
    /// back.
    ///
    /// The `black_box` is what puts the array in memory and keeps the reads
    /// from being folded into the writes; without it the whole function is a
    /// constant and the gate observes nothing.
    fn stack_probe(seed: u32) -> u32 {
        let mut scratch = [0u32; STACK_PROBE_WORDS];
        for (index, slot) in scratch.iter_mut().enumerate() {
            *slot = word(seed, index);
        }
        core::hint::black_box(&scratch);
        // The array is on this thread's stack, so its address names the stack.
        register_thread(scratch.as_ptr() as usize as u32);

        let mut noise = 0u32;
        for step in 0..HOLD {
            noise = core::hint::black_box(noise.wrapping_add(step));
        }
        core::hint::black_box(noise);

        let mut sum = 0u32;
        for (index, &slot) in scratch.iter().enumerate() {
            if slot != word(seed, index) {
                CLOBBERED.store(1, Ordering::Relaxed);
            }
            sum = sum.wrapping_add(slot);
        }
        sum
    }

    /// One chunk of the gate's `par_for`.
    fn run_chunk(start: usize, items: &mut [u32]) {
        for (offset, slot) in items.iter_mut().enumerate() {
            *slot = stack_probe((start + offset) as u32);
        }
    }

    fn checksum(items: &[u32]) -> u32 {
        items
            .iter()
            .fold(0u32, |acc, &item| acc.rotate_left(7) ^ item)
    }

    /// `Spawn::threaded` on the backend this target actually gets.
    #[unsafe(no_mangle)]
    pub extern "C" fn gate_threaded() -> u32 {
        u32::from(default_spawner().threaded())
    }

    /// `Spawn::parallelism` on the same backend.
    #[unsafe(no_mangle)]
    pub extern "C" fn gate_parallelism() -> u32 {
        u32::try_from(default_spawner().parallelism().get()).unwrap_or(u32::MAX)
    }

    /// The checksum a correct run produces, computed on the calling thread.
    ///
    /// Call it before any worker exists: it runs the same stack probes, so a
    /// worker already running on this thread's stack would corrupt the answer
    /// the gate compares against.
    #[unsafe(no_mangle)]
    pub extern "C" fn gate_expected() -> u32 {
        let mut items = vec![0u32; ITEMS];
        run_chunk(0, &mut items);
        checksum(&items)
    }

    /// Build a pool of `workers` workers through the seam.
    ///
    /// Answers how many it got — zero on a backend with no threads, which is
    /// the degradation path `Pool::with_workers` already had, and `u32::MAX` if
    /// a spawner that promised threads then refused one. Any previous pool is
    /// dropped, which shuts its workers down.
    #[unsafe(no_mangle)]
    pub extern "C" fn gate_pool(workers: u32) -> u32 {
        let spawner = default_spawner();
        let Ok(pool) = Pool::with_workers(spawner.as_ref(), workers as usize) else {
            return u32::MAX;
        };
        let got = u32::try_from(pool.workers()).unwrap_or(u32::MAX);
        POOL.with(|slot| *slot.borrow_mut() = Some(pool));
        got
    }

    /// One `par_for` over the buffer, and its checksum.
    ///
    /// `0` when no pool has been built.
    #[unsafe(no_mangle)]
    pub extern "C" fn gate_run() -> u32 {
        POOL.with(|slot| {
            let mut slot = slot.borrow_mut();
            let Some(pool) = slot.as_mut() else {
                return 0;
            };
            BUFFER.with(|buffer| {
                let mut buffer = buffer.borrow_mut();
                buffer.clear();
                buffer.resize(ITEMS, 0);
                pool.par_for(&mut buffer, CHUNK, run_chunk);
                checksum(&buffer)
            })
        })
    }

    /// How many distinct threads have run a chunk.
    #[unsafe(no_mangle)]
    pub extern "C" fn gate_threads() -> u32 {
        THREADS.load(Ordering::Relaxed)
    }

    /// `1` if any chunk read back a stack word it had not written.
    #[unsafe(no_mangle)]
    pub extern "C" fn gate_clobbered() -> u32 {
        CLOBBERED.load(Ordering::Relaxed)
    }

    /// `1` if any thread found another thread's frame address in its own
    /// thread-local.
    #[unsafe(no_mangle)]
    pub extern "C" fn gate_tls_shared() -> u32 {
        TLS_SHARED.load(Ordering::Relaxed)
    }
}
