//! Crucible's job system — starting with the seam every engine thread goes
//! through.
//!
//! # Why a seam and not `std::thread`
//!
//! `std::thread::spawn` **compiles on `wasm32-unknown-unknown` and fails at run
//! time.** In `library/std/src/sys/thread/mod.rs` the
//! `all(target_family = "wasm", target_feature = "atomics")` arm takes only
//! `sleep` from `thread/wasm.rs`; `Thread`, `available_parallelism`,
//! `yield_now` and `set_name` all come from `thread/unsupported.rs`, where
//! `Thread::new` returns `io::Error::UNSUPPORTED_PLATFORM`. Enabling atomics
//! buys shared memory and working atomic primitives — and no way to start a
//! thread, because a wasm module cannot instantiate its own worker. Only the
//! host can, against the shared `WebAssembly.Memory`.
//!
//! So a pool built on `std::thread` is a pool that compiles for the browser and
//! has no browser story, and the failure is a run-time `Err` in a place nothing
//! checks. `docs/plan/21-jobs.md`'s 2026-08-03 correction records the
//! measurement; this crate is the consequence of it. Every subsystem thread in
//! the topology starts through [`Spawn`] instead, and the platform's answer to
//! "can I have a thread" is asked once, at startup, through
//! [`Spawn::threaded`].
//!
//! # What is here, and what is not
//!
//! The seam and two backends: `Threads` over `std::thread`, and [`Inline`],
//! which has none and says so. [`default_spawner`] picks between them, which is
//! the only place a consumer would otherwise write `cfg(target_arch)`.
//!
//! `Threads` is unlinked here deliberately: it does not exist on `wasm32` — see
//! its own docs for why — so a link to it is unresolvable in exactly the build
//! this crate was written for, and the wasm rustdoc gate says so.
//!
//! Above the seam are the design's two communication primitives — [`mailbox`],
//! a latest-wins triple buffer for *states*, where neither side ever waits and
//! the newest is the only one anybody wants, and [`ring`], a bounded SPSC for
//! *streams*, where every item is delivered because an input edge must not be
//! droppable the way a state is.
//!
//! Beside them is [`pool`]: a work-stealing pool and
//! [`par_for`](pool::Pool::par_for), for the data-parallel bursts *inside* a
//! stage. It is built on [`Spawn`] rather than on `std::thread` — the whole
//! reason the seam landed first — so it works on a runtime with no threads at
//! all, where it runs every chunk on the calling thread and reaches the same
//! answer.
//!
//! Beside them on `wasm32` is the `workers` module and its `Workers`, the
//! browser's backend. It cannot start a thread itself — only the host can
//! instantiate the module again against the shared memory — so it **queues**
//! each spawn for a page to drain and turn into a `Worker`. It is what
//! [`default_spawner`] picks there, and it answers [`Spawn::threaded`] with
//! `false` until a page has said it can start workers, so an artifact loaded by
//! a page without the shim degrades onto [`Inline`]'s behaviour rather than
//! promising threads it has no way to get. Unlinked here for `Threads`'
//! reason turned around: neither the module nor the type exists off `wasm32`,
//! so a link to either is unresolvable in the native rustdoc gate.
//!
//! # The unsafe, and what checks it
//!
//! [`mailbox`], [`ring`] and [`pool`] are the modules that reach past the
//! language's checks, for the reason the design names: the primitives are the
//! sole novel concurrency surface, so they are kept tiny and checked hard. Each
//! invariant is stated where its `unsafe` is and asserted directly by the tests
//! rather than argued for. The pool's work-stealing deque is the exception that
//! proves it — its slots hold pointers in atomics precisely so that the
//! speculative read a thief does, before it knows whether the item is its own,
//! needs no `unsafe` at all.
//!
//! **The memory orderings are checked by Miri and by nothing else, and that is
//! a property of the hardware rather than a gap in the suite.** x86-64 is
//! total-store-order: a `Release` store and a `Relaxed` store compile to the
//! same instruction, so weakening one changes no observable behaviour on this
//! machine however long the stress tests run. Measured, not assumed —
//! `Ordering::Release` was weakened to `Relaxed` in [`ring`]'s push and the
//! whole suite stayed green, while Miri reported the data race in `pop` with a
//! backtrace. The same was done again for [`pool`]: weakening the release on
//! the outstanding-chunk count, or the acquire the deque's thieves take on
//! `bottom`, leaves every test passing here and is a data race under Miri.
//! Miri is therefore the only thing standing between a wrong ordering and an
//! aarch64 or wasm user, and **a change to any of these atomics has to be run
//! under it locally before it is believed** — `cargo miri test -p crcbl-jobs`
//! interprets this crate in about twenty seconds. The CI job is weekly
//! (`.github/workflows/cron.yml`), so waiting for it is waiting up to a week to
//! find out.
//!
//! # Degrading is a decision, not an error
//!
//! A runtime with no threads is not a failure — it is the browser today and a
//! headless test tomorrow, and the design's rule is that pipeline stages become
//! sequential calls and `par_for` runs inline, with **no `cfg` in the systems
//! themselves**. That is why the question is [`Spawn::threaded`], asked once
//! while a subsystem is being built, rather than a [`SpawnError`] handled per
//! spawn: a caller picks its shape up front, and a later
//! [`spawn`](Spawn::spawn) failure is a real error on a runtime that promised
//! otherwise.

mod deque;
pub mod mailbox;
pub mod pool;
pub mod ring;
mod spawn;
#[cfg(target_arch = "wasm32")]
pub mod workers;

pub use pool::Pool;
#[cfg(not(target_arch = "wasm32"))]
pub use spawn::Threads;
pub use spawn::{Inline, Spawn, SpawnError, Work, default_spawner};
#[cfg(target_arch = "wasm32")]
pub use workers::Workers;
