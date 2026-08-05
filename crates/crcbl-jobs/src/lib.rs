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
//! **Not here yet**: the latest-wins mailboxes, the SPSC rings, the
//! work-stealing pool and `par_for`. They are the slices above this one and
//! they will be built on this trait rather than on `std::thread`, which is the
//! whole reason the seam lands first. The browser's worker backend is not here
//! either — it needs a pinned nightly and cross-origin isolation, both of them
//! open questions recorded in `docs/backlog.md`, and the ordering exists so
//! that neither answer blocks anything below it.
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

mod spawn;

#[cfg(not(target_arch = "wasm32"))]
pub use spawn::Threads;
pub use spawn::{Inline, Spawn, SpawnError, Work, default_spawner};
