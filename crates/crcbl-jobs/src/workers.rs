//! The browser's spawn backend: requests a host turns into Web Workers.
//!
//! # Why this queues instead of spawning
//!
//! A wasm module cannot start its own thread. Only the host can instantiate the
//! module a second time against the shared `WebAssembly.Memory` — see the
//! [crate docs](crate) and `docs/plan/21-jobs.md`'s finding 2 — so
//! [`Spawn::spawn`] here starts nothing. It **queues** the work, and the page
//! drains that queue through the exports in [`shim`].
//!
//! That is compatible with the seam because the seam is fire-and-forget:
//! [`Spawn::spawn`] has no join, and the native backend detaches the handle for
//! the same reason. What a queued spawn cannot do is report a host-side
//! refusal — by the time a `Worker` fails to start, `spawn` has long since
//! returned `Ok`. The honest place to learn that a runtime has no threads stays
//! [`Spawn::threaded`], asked once at startup, exactly as the seam's docs say.
//!
//! # Nothing crosses into wasm
//!
//! The engine's ABI is exports-plus-polling: JS calls exports and polls, wasm
//! never calls out. A backend built on an `extern "C" { fn spawn_worker(…); }`
//! would be the first import in a browser artifact and
//! `web/tools/check-exports.mjs` fails any artifact that imports anything (in
//! threaded mode, anything but `env.memory`). So the direction is inverted:
//! [`shim::__crcbl_web_jobs_pending`] is polled, [`shim::__crcbl_web_jobs_take`]
//! hands out one request, and the worker calls back in through
//! [`shim::__crcbl_web_jobs_entry`].
//!
//! # The host is what makes this threaded
//!
//! A `+atomics` artifact loaded by a page with no worker shim has no threads,
//! and an artifact cannot tell the two pages apart. So [`Workers::threaded`]
//! answers **false** until the host has said otherwise through
//! [`shim::__crcbl_web_jobs_host_ready`], which is also where
//! `navigator.hardwareConcurrency` arrives — a number only JS knows. Until then
//! [`Spawn::parallelism`] is one, which the trait defines as "run inline", and
//! [`Pool::with_workers`](crate::pool::Pool::with_workers) degrades onto the
//! path it already had.
//!
//! # The ABI
//!
//! Every symbol is `__crcbl_web_jobs_*`, in the `(u32, …) -> u32` shape the rest
//! of the engine's browser ABI uses.
//!
//! | Export | Signature | What it does |
//! | --- | --- | --- |
//! | [`__crcbl_web_jobs_host_ready`](shim::__crcbl_web_jobs_host_ready) | `(u32) -> u32` | The host announces it can start workers, and reports `navigator.hardwareConcurrency`. Answers the worker count actually recorded. |
//! | [`__crcbl_web_jobs_pending`](shim::__crcbl_web_jobs_pending) | `() -> u32` | How many spawn requests are waiting. The poll. |
//! | [`__crcbl_web_jobs_take`](shim::__crcbl_web_jobs_take) | `() -> u32` | Take the oldest request and answer its handle, or `0` when there is none. |
//! | [`__crcbl_web_jobs_name_ptr`](shim::__crcbl_web_jobs_name_ptr) | `() -> *const u8` | Where the taken request's thread name starts. |
//! | [`__crcbl_web_jobs_name_len`](shim::__crcbl_web_jobs_name_len) | `() -> u32` | How long it is, in UTF-8 bytes. |
//! | [`__crcbl_web_jobs_stack_alloc`](shim::__crcbl_web_jobs_stack_alloc) | `() -> *mut u8` | Leak one worker stack and answer the **high** address to write into `__stack_pointer`. |
//! | [`__crcbl_web_jobs_tls_alloc`](shim::__crcbl_web_jobs_tls_alloc) | `(u32, u32) -> *mut u8` | Leak one TLS block of `__tls_size` bytes at `__tls_align`, to pass to `__wasm_init_tls`. |
//! | [`__crcbl_web_jobs_entry`](shim::__crcbl_web_jobs_entry) | `(u32) -> u32` | **On the worker**: run the work the handle names. `1` if it ran, `0` if the handle is not one this module handed out. |
//!
//! The bring-up sequence per worker, which is the part that has to be in this
//! order:
//!
//! 1. main polls `__crcbl_web_jobs_pending`, calls `__crcbl_web_jobs_take`,
//!    reads the name, and allocates a stack and a TLS block;
//! 2. the worker instantiates the same module against the same `env.memory`;
//! 3. it writes the stack top into the `__stack_pointer` global — **skipping
//!    this is silent**: the worker then shares the main thread's stack and only
//!    code that writes a large stack array can tell;
//! 4. it calls `__wasm_init_tls(tlsPtr)` — skipping this traps on the first
//!    thread-local access;
//! 5. it calls `__crcbl_web_jobs_entry(handle)`, which does not return for a
//!    pool worker.
//!
//! `web/tools/worker-gate.mjs` is that sequence, run against a real artifact,
//! with the stack step observable.

use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicU32, Ordering};
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, TryLockError};

use crate::spawn::{Spawn, SpawnError, Work};

/// Bytes of stack each worker gets.
///
/// One request per worker, leaked, so the number is a per-thread cost rather
/// than a per-spawn one. Chosen to match the stack the linker gives the main
/// thread (`wasm-ld`'s `--stack-size` default) on the reasoning that a worker
/// runs the same subsystem loops main would have. **Nothing has measured it.**
const WORKER_STACK_BYTES: usize = 1024 * 1024;

/// The alignment every allocation here comes out at, and the one the wasm ABI
/// keeps `__stack_pointer` at.
///
/// It is also the ceiling [`shim::__crcbl_web_jobs_tls_alloc`] accepts for
/// `__tls_align`: a block aligned to this satisfies any smaller request, and a
/// larger one is refused rather than quietly under-aligned.
const ALIGNMENT: usize = 16;

/// How many workers the host says the machine has, or zero before it has said
/// anything.
///
/// Zero is the whole of [`Workers::threaded`]'s answer, so a page with no
/// worker shim never reaches the spawn queue at all. `Relaxed` because the
/// value carries nothing with it: everything a spawn publishes is ordered by
/// [`QUEUE`]'s own lock.
static HOST_WORKERS: AtomicU32 = AtomicU32::new(0);

/// The spawn requests, and the ones handed out and not yet entered.
static QUEUE: Mutex<Queue> = Mutex::new(Queue::new());

/// One request that has not been turned into a worker yet.
struct Request {
    name: &'static str,
    work: Work,
    handle: u32,
}

struct Queue {
    /// Oldest first. [`Spawn::spawn`] pushes the back, the host takes the front.
    pending: VecDeque<Request>,
    /// Taken by the host and not yet entered by a worker.
    ///
    /// **This is what makes the handle safe.** JS holds an integer, never a
    /// pointer, and [`shim::__crcbl_web_jobs_entry`] can only run work that is
    /// in here — so a handle that was invented, replayed or corrupted finds
    /// nothing and is refused, rather than being read as an address.
    handed_out: Vec<(u32, Work)>,
    /// Saturating rather than wrapping, because `0` is "no request" in the ABI
    /// and a wrap would eventually mint it.
    ///
    /// So handles are unique up to `u32::MAX` and not beyond: past the ceiling
    /// every later request carries `u32::MAX`. That is survivable rather than
    /// unreachable-by-construction — [`shim::__crcbl_web_jobs_entry`] removes
    /// one matching entry per call, so each work still runs exactly once, and
    /// what a collision costs is which worker's name goes with which work.
    next_handle: u32,
    /// The name of the request the last [`shim::__crcbl_web_jobs_take`] handed
    /// out, for the label the host puts on the worker.
    taken_name: Option<&'static str>,
}

impl Queue {
    const fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            handed_out: Vec::new(),
            next_handle: 1,
            taken_name: None,
        }
    }
}

/// Takes [`QUEUE`] by spinning, because the thread that drains it may be one
/// that is forbidden to block.
///
/// **`try_lock`, never `lock`.** On `wasm32` with atomics, std's `Mutex` is the
/// futex one (`sys/sync/mutex/futex.rs`), whose `lock` falls through to
/// `futex_wait` and therefore to `memory_atomic_wait32` — the instruction the
/// browser's main thread *traps* on rather than blocking in
/// (`docs/plan/21-jobs.md` finding 3), and the main thread is exactly where the
/// drain half of this queue runs. `try_lock` in that same file is one
/// `compare_exchange` with no futex path at any contention level, so this
/// function cannot reach the instruction that throws.
///
/// Spinning terminates because no critical section here waits on anything: each
/// is a push, a pop or a scan of the in-flight list, and none of them calls
/// into the caller's code — the work itself runs after the guard is dropped.
///
/// Traced through std's sources, not observed in a browser: node lets its main
/// thread block, so no gate here can show the trap.
fn hold() -> MutexGuard<'static, Queue> {
    loop {
        match QUEUE.try_lock() {
            Ok(queue) => return queue,
            // The queue's own invariants survive a panic in this file — nothing
            // here can panic mid-update — and refusing to lock afterwards would
            // turn a bug into a hang. Same call `pool` makes, for the same
            // reason.
            Err(TryLockError::Poisoned(poisoned)) => return poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => core::hint::spin_loop(),
        }
    }
}

/// `bytes` rounded up, zeroed, [`ALIGNMENT`]-aligned, and **leaked**.
///
/// Deliberate rather than a leak to apologise for: both callers hand the region
/// to a thread that never gives it back. A pool worker's loop does not return
/// (`pool::work`), so there is no later moment at which either the stack it is
/// running on or the TLS block it is reading could be freed. Freeing them at
/// any point a `Drop` could run would be freeing memory a live thread is
/// standing on.
///
/// `u128` is the element type for its alignment, which a `Vec<u8>` does not
/// have — `align_of::<u8>()` is one. The `const` block is what makes that a
/// checked property of the target rather than a claim: a target where `u128`
/// is narrower or less aligned fails to compile here instead of handing back a
/// misaligned stack.
fn leak(bytes: usize) -> &'static mut [u128] {
    const {
        assert!(size_of::<u128>() == ALIGNMENT);
        assert!(align_of::<u128>() >= ALIGNMENT);
    }
    Vec::leak(vec![0u128; bytes.div_ceil(ALIGNMENT)])
}

/// A spawner that queues requests for a host to turn into Web Workers.
///
/// **`wasm32` only, on purpose**, which mirrors `Threads` being absent there: a
/// backend that can only work behind a page's worker shim should not be
/// nameable on a target that has real threads.
///
/// It answers [`Spawn::threaded`] with **false** until the page has announced
/// itself through [`shim::__crcbl_web_jobs_host_ready`], so an artifact loaded
/// by a page that does not implement the shim degrades exactly as
/// [`Inline`](crate::Inline) does. See the [module docs](self).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Workers;

impl Spawn for Workers {
    fn threaded(&self) -> bool {
        HOST_WORKERS.load(Ordering::Relaxed) > 0
    }

    fn parallelism(&self) -> NonZeroUsize {
        NonZeroUsize::new(HOST_WORKERS.load(Ordering::Relaxed) as usize)
            .unwrap_or(NonZeroUsize::MIN)
    }

    fn spawn(&self, name: &'static str, work: Work) -> Result<(), SpawnError> {
        if !self.threaded() {
            // Dropped rather than run, for [`Inline`](crate::Inline)'s reason:
            // `work` is a subsystem's whole loop as often as it is a finite
            // task.
            drop(work);
            return Err(SpawnError::NoThreads(name));
        }
        let mut queue = hold();
        let handle = queue.next_handle;
        queue.next_handle = queue.next_handle.saturating_add(1);
        queue.pending.push_back(Request { name, work, handle });
        // `Ok` means queued, not started. Nothing above the seam can be told
        // that a `Worker` refused to start, because `spawn` has returned by
        // then — see the module docs.
        Ok(())
    }
}

/// The exports the page's worker shim calls. See the [module docs](self).
pub mod shim {
    use core::sync::atomic::Ordering;

    use super::{ALIGNMENT, HOST_WORKERS, WORKER_STACK_BYTES, hold, leak};

    /// The host announces that it can start workers, and how many the machine
    /// has.
    ///
    /// `concurrency` is `navigator.hardwareConcurrency`, clamped to at least
    /// one — a host that calls this at all is claiming it can start a worker,
    /// and the count is what [`Spawn::parallelism`](crate::Spawn::parallelism)
    /// reports. Answers the number recorded.
    ///
    /// **Until this is called there are no threads.** A page that cannot start
    /// workers — no `crossOriginIsolated`, no shim — simply never calls it, and
    /// every consumer degrades through
    /// [`Spawn::threaded`](crate::Spawn::threaded) as it already does.
    ///
    /// Calling it again replaces the count; it is not cumulative.
    #[unsafe(no_mangle)]
    pub extern "C" fn __crcbl_web_jobs_host_ready(concurrency: u32) -> u32 {
        let workers = concurrency.max(1);
        HOST_WORKERS.store(workers, Ordering::Relaxed);
        workers
    }

    /// How many spawn requests are waiting for a worker.
    ///
    /// The poll half of the ABI: a host drains until this answers zero.
    /// Allocates nothing.
    #[unsafe(no_mangle)]
    pub extern "C" fn __crcbl_web_jobs_pending() -> u32 {
        u32::try_from(hold().pending.len()).unwrap_or(u32::MAX)
    }

    /// Take the oldest request, and answer the handle a worker will enter it
    /// with.
    ///
    /// `0` when nothing is waiting. The name that came with the request is
    /// readable from [`__crcbl_web_jobs_name_ptr`] and
    /// [`__crcbl_web_jobs_name_len`] until the next call to this.
    ///
    /// The work stays owned by this module. A handle is an integer with no
    /// meaning outside [`__crcbl_web_jobs_entry`], which is what keeps a bad
    /// one from being an address.
    #[unsafe(no_mangle)]
    pub extern "C" fn __crcbl_web_jobs_take() -> u32 {
        let mut queue = hold();
        let Some(request) = queue.pending.pop_front() else {
            queue.taken_name = None;
            return 0;
        };
        queue.taken_name = Some(request.name);
        queue.handed_out.push((request.handle, request.work));
        request.handle
    }

    /// Where the last taken request's thread name starts, or null if the last
    /// [`__crcbl_web_jobs_take`] found nothing. Allocates nothing.
    #[unsafe(no_mangle)]
    pub extern "C" fn __crcbl_web_jobs_name_ptr() -> *const u8 {
        hold().taken_name.map_or(core::ptr::null(), str::as_ptr)
    }

    /// How long that name is, in UTF-8 bytes. Allocates nothing.
    #[unsafe(no_mangle)]
    pub extern "C" fn __crcbl_web_jobs_name_len() -> u32 {
        hold()
            .taken_name
            .map_or(0, |name| u32::try_from(name.len()).unwrap_or(u32::MAX))
    }

    /// Leak one worker stack, and answer the address to write into
    /// `__stack_pointer`.
    ///
    /// **The high address**, because the wasm stack grows down: the value is one
    /// past the end of the region, so the first push lands inside it. The
    /// region carries `ALIGNMENT`, the alignment the wasm ABI keeps the stack
    /// pointer at, and its length is a whole number of those — so the answer
    /// does too.
    ///
    /// Called once per worker by the host, before that worker instantiates.
    /// Leaked deliberately — see `leak`.
    #[unsafe(no_mangle)]
    pub extern "C" fn __crcbl_web_jobs_stack_alloc() -> *mut u8 {
        leak(WORKER_STACK_BYTES).as_mut_ptr_range().end.cast()
    }

    /// Leak one TLS block, and answer the address to pass to
    /// `__wasm_init_tls`.
    ///
    /// `size` and `align` are the `__tls_size` and `__tls_align` globals the
    /// host reads off the instance. They are per-build — a trivial crate and a
    /// demo do not agree — so they are read rather than assumed here as well.
    ///
    /// `0` for a request this module cannot satisfy: a zero size, or an
    /// alignment that is not a power of two or is coarser than `ALIGNMENT`, the
    /// one every allocation here comes out at. Refused rather than
    /// under-aligned, because an under-aligned TLS block is the kind of failure
    /// that shows up as a wrong value much later.
    #[unsafe(no_mangle)]
    pub extern "C" fn __crcbl_web_jobs_tls_alloc(size: u32, align: u32) -> *mut u8 {
        let align = align as usize;
        if size == 0 || !align.is_power_of_two() || align > ALIGNMENT {
            return core::ptr::null_mut();
        }
        leak(size as usize).as_mut_ptr().cast()
    }

    /// Run the work a handle names. **Called on the worker**, and it does not
    /// return until the work does.
    ///
    /// `1` when the work ran. `0` when `handle` is not one
    /// [`__crcbl_web_jobs_take`] handed out and has not already been entered —
    /// an invented handle, a replayed one, or one from a request that was
    /// entered twice. Nothing is dereferenced to find that out.
    ///
    /// The queue's lock is released before the work starts, so work that spawns
    /// more work does not deadlock against its own entry.
    #[unsafe(no_mangle)]
    pub extern "C" fn __crcbl_web_jobs_entry(handle: u32) -> u32 {
        let work = {
            let mut queue = hold();
            let at = queue.handed_out.iter().position(|(id, _)| *id == handle);
            at.map(|at| queue.handed_out.swap_remove(at).1)
        };
        let Some(work) = work else { return 0 };
        work();
        1
    }
}
