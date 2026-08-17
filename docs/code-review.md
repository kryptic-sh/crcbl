# crcbl code review

Full-workspace review, 2026-08-01. Every `.rs` file under `crates/` and `apps/`
(~103k lines, excluding `target/` and `crcbl-net/fuzz/target/`) was read line by
line, plus the workspace manifests, `deny.toml`, `rust-toolchain.toml`,
`.config/`, and `.github/workflows/`.

Findings are grouped by crate. Each carries `path:line`, a severity, a category,
and the concrete failure mode. Findings marked **[probe]** were reproduced by
running the code (probe programs written outside the repo); the rest were
verified by reading. Anything that could not be confirmed is marked _unverified_
together with what would confirm it.

**This is a record of that day, not a description of the tree.** It has not been
amended since it was written and it will not be: `docs/plan/ROADMAP.md` says its
findings were fixed across eight commits, and later phases have moved code out
from under others — the breakout `paddle_model` finding, for instance, describes
a forward pass breakout no longer has at all, having been redrawn as sprites at
P4B. Line numbers and `path:line` citations are as of 2026-08-01. Read it for
what was found and how, and read `git log` for what happened next. Patching the
findings that happen to get noticed would leave a document that is neither a
record nor current; the honest way to refresh it is to run the review again.

## Contents

- [Executive summary](#executive-summary)
- [Cross-cutting themes](#cross-cutting-themes)
- [Top findings by severity](#top-findings-by-severity)
- [crcbl-shell — Wayland backend](#crcbl-shell--wayland-backend)
- [crcbl-shell — X11 backend](#crcbl-shell--x11-backend)
- [crcbl-shell — core, headless, web](#crcbl-shell--core-headless-web)
- [crcbl-vk — Vulkan backend](#crcbl-vk--vulkan-backend)
- [crcbl-hal — GPU seam + null backend](#crcbl-hal--gpu-seam--null-backend)
- [crcbl-render + crcbl-wgpu](#crcbl-render--crcbl-wgpu)
- [crcbl-net + crcbl-server + crcbl-client](#crcbl-net--crcbl-server--crcbl-client)
- [crcbl-phys + crcbl-ecs](#crcbl-phys--crcbl-ecs)
- [crcbl-core + crcbl-store + crcbl-audio](#crcbl-core--crcbl-store--crcbl-audio)
- [crcbl-ui + crcbl-input + crcbl-shaders](#crcbl-ui--crcbl-input--crcbl-shaders)
- [crcbl-golden + crcbl-wl-scanner + crcbl-cli + crcbl facade](#crcbl-golden--crcbl-wl-scanner--crcbl-cli--crcbl-facade)
- [apps + workspace config + CI](#apps--workspace-config--ci)

## Executive summary

The engine's foundational layers are strong. `crcbl-core` (handles, arena,
fixed-timestep clock, sector world) produced no finding above style: generation
exhaustion retires slots instead of wrapping, the arena's `mut_from_ref`
argument holds, and the clock's spiral-of-death cap preserves the sub-tick
remainder. The Wayland and X11 FFI is unusually careful for hand-written
`dlopen` bindings — generated argument arrays, accurate SAFETY comments, correct
reply ownership, no leaks or double-frees found in the X11 reply paths. The
`crcbl-net` decoders are genuinely hardened: every wire-driven
`Vec::with_capacity` is bounded by a prior count check, all multiplications are
`checked_*`, and no reachable panic or unbounded allocation was found on any of
the seven decode entry points. The hand-rolled SHA-256 in `crcbl-shaders` is
correct against all five NIST vectors, including the 1,000,000-`'a'` case.

The problems cluster in three places, and they share one root cause: **layers
whose tests only exercise the happy path, or exercise it through a model that
does not match the real thing.**

1. **Untrusted-input parsers outside `crcbl-net`.** `crcbl-store` and
   `crcbl-audio` read attacker- or corruption-controlled lengths straight into
   `Vec::with_capacity`. A 62-byte save file aborts the process; a 30-byte
   replay file panics; an 8-byte QOA header reserves 17 GB. `NativeStorage`
   allows path traversal and absolute paths out of the storage root, and
   `delete` calls `remove_dir_all` on the result. None of this is protected by
   the discipline `crcbl-net` applies one crate over.

2. **Backends that have never been run.** `crcbl-wgpu` cannot complete a single
   frame: `acquire_next_frame` builds handles with a zero generation so
   `from_bits` returns `None` and the `.expect` panics on the first call; all
   five copy commands are silent no-ops; the depth usage maps to empty
   `TextureUsages`. The Web shell's JS entry points push into a queue nothing
   drains, so no canvas window is ever configured. Both compile clean and both
   have unit tests — the tests bypass the entry points that are broken.

3. **The gameplay stack is not simulated the way it claims.** Physics advances
   at a hardcoded 1/120 s regardless of tick rate, so simulated speed is a
   function of the configured schedule; breakout steps its game logic once per
   _frame_ rather than once per tick, so paddle speed scales with fps; the ball
   sweep runs once per frame while the server may run several physics ticks, so
   the ball tunnels below 60 fps; and the BVH refit uses the pre-sort element
   index, so moving one collider makes an untouched one vanish from every query
   **[probe]**. The three "determinism" tests in breakout never launch the ball,
   so all six invocations assert `0 == 0`.

None of the above requires an architectural change. The seams are right; the
gaps are in validation, in a handful of index/sign errors, and in tests that
assert the wrong thing.

## Cross-cutting themes

**Unvalidated declared lengths.** `crcbl-net` gets this right everywhere and is
the model to copy. `crcbl-store/save.rs:237`, `crcbl-store/replay.rs:185`,
`crcbl-audio/qoa.rs:164` and `crcbl-golden/image.rs:181` each allocate from a
header field before validating it against the remaining bytes. Three of the four
are reachable from a file a user would routinely open (autosave, shared replay,
downloaded asset).

**32-bit / wasm32 truncation.** `save.rs:255`, `replay.rs:203` and `wav.rs:95`
all bounds-check with `cursor + len` where `len` is `u32`-derived; on wasm32 — a
target the workspace deliberately supports as of `afd63bf` — that wraps in
release and the following slice index panics. `component_hash.rs:42` hashes
`usize` at platform width, so the same component state hashes differently on
wasm32 and x86-64, defeating the schema-compat check the hash exists for.

**The null backend is not a faithful reference.** `crcbl-hal`'s `null` backend
is the validator the graph-compile suite runs against, but its validation is a
strict subset of `crcbl-vk`'s in at least six places (unclosed pass, bind-group
layout checks, push-constant overflow, `query_results` range, duplicate
bindings, bindless ceilings). Streams that green-light on null fail on Vulkan.
Symmetrically, `crcbl-vk`'s `destroy_*` functions remove the pool entry _before_
checking the owner, which the null backend gets right and asserts — the same
assertion run against `crcbl-vk` would fail.

**Documentation that contradicts the code.** Recurring, and each instance is a
latent bug because the next reader trusts the prose:
`crcbl-shell/wayland/fd.rs:78` (idle vs total timeout),
`crcbl-hal/command.rs:540` (`Send`-not-`Sync` vs the `HalThreadSafe` bound),
`crcbl-store/save.rs:148` ("SHA-256" that is `DefaultHasher`),
`crcbl-store/settings.rs:265` (comment states the reverse of the merge order),
`crcbl-ui/hud.rs:96` (documented return value the signature does not have),
`crcbl-audio/lib.rs:48` (zeroed-buffer promise the stereo path does not keep),
`apps/breakout/Cargo.toml:10` ("one engine dependency" above eleven).

**Trusted/untrusted splits an attacker can flip.** `crcbl-client` raises its
decode limits on receipt of an unauthenticated `Accept`; nothing in the protocol
authenticates any message, so acks, rejects and the "trusted" flag are all
forgeable. This is the single highest-value security gap in the tree.

**Duplication that has already diverged.** Three copies of the X11
wire-reinterpret helper; four copies of the zero-direction-safe slab test; three
copies of the quadratic root solver in `crcbl-phys` — and the three copies
disagree on root selection, which is the direct cause of three separate sweep
bugs. `apps/breakout/src/gpu.rs` is a comment-stripped ~90%-similar copy of
`apps/sandbox/src/gpu.rs`, minus the 90 lines of design rationale.

## Top findings by severity

The full detail for each is in the crate section below.

### Critical

| Finding                                                                        | Location                        |
| ------------------------------------------------------------------------------ | ------------------------------- |
| BVH refit uses pre-sort index; moving one collider hides another from queries  | `crcbl-phys/src/world.rs:322`   |
| `unsafe impl Send/Sync for Mixer` is unsound; safe `fill(&self)` races         | `crcbl-audio/src/mixer.rs:138`  |
| 62-byte save file aborts the process via `handle_alloc_error`                  | `crcbl-store/src/save.rs:237`   |
| Web shell's JS entry points push to a queue nothing drains                     | `crcbl-shell/src/web/mod.rs:58` |
| `acquire_next_frame` builds zero-generation handles; first acquire panics      | `crcbl-wgpu/src/device.rs:751`  |
| All five wgpu copy/fill commands are silent no-ops returning `Ok`              | `crcbl-wgpu/src/command.rs:94`  |
| Depth usage maps to empty `TextureUsages`; every forward pass fails            | `crcbl-wgpu/src/conv.rs:87`     |
| Breakout restart never rebuilds the brick grid                                 | `apps/breakout/src/game.rs:629` |
| Game stepped once per frame, not per tick; paddle speed scales with fps        | `apps/breakout/src/app.rs:281`  |
| Ball sweep runs once per frame while physics runs N times; tunnels below 60fps | `apps/breakout/src/game.rs:517` |

### High (selection)

| Finding                                                                | Location                             |
| ---------------------------------------------------------------------- | ------------------------------------ |
| Unauthenticated acks permanently desync a victim client                | `crcbl-server/src/lib.rs:219`        |
| One forged `Reject` permanently wedges a client                        | `crcbl-client/src/lib.rs:391`        |
| Path traversal + absolute paths escape the storage root                | `crcbl-store/src/lib.rs:151`         |
| QOA LMS weight update is inverted vs. spec; all non-silent audio wrong | `crcbl-audio/src/qoa.rs:317`         |
| 30-byte replay file panics `FileTransport::open`                       | `crcbl-store/src/replay.rs:185`      |
| Hostile `settings.toml` panics the next `set()`                        | `crcbl-store/src/settings.rs:320`    |
| Physics hardcodes `step(1/120)` regardless of tick rate                | `crcbl-phys/src/system.rs:402`       |
| Sweep returns `None` when start and end both overlap                   | `crcbl-phys/src/query.rs:411`        |
| Read-only depth attachment declared in the wrong image layout          | `crcbl-vk/src/command.rs:548`        |
| Submission counter incremented before the submit can fail              | `crcbl-vk/src/device.rs:1718`        |
| Swapchain retired without waiting on armed acquire fences              | `crcbl-vk/src/device.rs:691`         |
| `TimeBase::rebase` underflows on a compositor-supplied timestamp       | `crcbl-shell/src/wayland/mod.rs:303` |
| Unbounded property read; clipboard `MAX_BYTES` does not cover it       | `crcbl-shell/src/x11/mod.rs:439`     |
| Range-scoped barrier emitted with a whole-image `from` state           | `crcbl-render/src/graph.rs:1590`     |
| Disabled input actions re-fire one tick later                          | `crcbl-input/src/lib.rs:321`         |
| Text layout scales the absolute cursor, so advance scales twice        | `crcbl-ui/src/text.rs:154`           |
| CLI screenshot writes red/blue-swapped PNGs on BGRA surfaces           | `crcbl-cli/src/screenshot.rs:22`     |
| XML attribute values injected verbatim into generated Rust             | `crcbl-wl-scanner/src/emit.rs:337`   |
| No CI job builds wasm32 despite deliberate wasm32 support              | `.github/workflows/ci.yml`           |
| Breakout draws only the paddle; ball and all 40 bricks are invisible   | `apps/breakout/src/gpu.rs:370`       |
| All three breakout determinism tests are vacuous                       | `apps/breakout/src/game.rs:691`      |

## crcbl-shell — Wayland backend

### High

- **`crates/crcbl-shell/src/wayland/e2e.rs:185` — `map_window` installs a
  libwayland dispatcher pointing at a stack local, then returns**
  (unsafe-soundness). `binding` is a `ShmBinding` on `map_window`'s stack;
  `ptr::from_mut(&mut binding).cast()` is handed to `wl_proxy_add_dispatcher` on
  `binding.registry`, and that registry proxy is never destroyed. It stays live
  on the shell's connection for the rest of the process, so any later
  `wl_registry.global`/`global_remove` — an output hotplug, a compositor plugin
  loading — re-enters `bind_shm` with a dangling `*mut ShmBinding` and does
  `&mut *user_data` on freed stack memory. The sibling helpers get this right
  (`VirtualInput::attach` and `DragSource::attach` box their state and destroy
  the registry in `Drop`); `map_window` is the outlier. Feature-gated to
  `wayland-e2e`, so test-only — but the e2e suite is exactly where output
  hotplug and `swaymsg output … scale` happen.

### Medium

- **`crates/crcbl-shell/src/wayland/mod.rs:303` — `TimeBase::rebase` underflows
  on a compositor-supplied timestamp** (correctness). `full -= WRAP` runs
  whenever `full > now_millis + WRAP/2`, but on a machine with uptime under 24.8
  days `now_millis`'s high 32 bits are zero, so `full == wayland_millis < WRAP`
  and the subtraction underflows. Verified by extracting the function:
  `rebase(0, 60_000_000_000, u32::MAX)` panics with _attempt to subtract with
  overflow_; in release it wraps to ~1.8e19 ms and every subsequent `EventTime`
  is garbage. Reachable from any `wl_pointer.motion`/`button`/`axis` or
  `wl_keyboard.key` whose `time` argument a buggy or hostile compositor sets
  above `now + 2^31` ms — a raw `u32` off the wire with no validation.
- **`crates/crcbl-shell/src/wayland/mod.rs:681` (SAFETY note at `:704`) — the
  dispatcher's `user_data` is a reborrow, not the `Box::into_raw` root**
  (unsafe-soundness). `Sink::watch` passes `ptr::from_mut(self)`, where `self`
  is the transient `&mut Sink` produced by `Conn::sink()`'s `&mut *self.sink`.
  Every subsequent `Conn::sink()` re-derives a fresh `&mut` from the raw root,
  invalidating the earlier reborrow and every raw pointer descended from it — so
  from the second `watch` onward libwayland holds pointers with dead tags and
  `dispatch`'s `&mut *user_data` is UB under Stacked/Tree Borrows. The SAFETY
  comment asserts the pointer "comes from `Box::into_raw`", which is not what
  the code does. One-line fix: pass `self.sink`. Miri under
  `-Zmiri-stacked-borrows` on a two-`watch` sequence would confirm. Same pattern
  at `e2e.rs:185/416/891/909/982`.
- **`crates/crcbl-shell/src/wayland/mod.rs:4483` — `set_constraints` silently
  makes a non-resizable window resizable** (correctness). `create_window` calls
  `apply_constraints(toplevel, desc.constraints, desc.resizable, desc.size)`,
  but `WlWindow` stores no `resizable` field, so `set_constraints` hardcodes
  `true` and re-sends `set_min_size`/`set_max_size` from the constraint set
  instead of the `min == max == requested` pinning that `resizable: false`
  means. The X11 backend does keep this (`x11/mod.rs:549`, `x11/shell.rs:264`),
  so the two backends now disagree about the same seam call.
- **`crates/crcbl-shell/src/wayland/fd.rs:168` (also `:219`, `:251`) — the
  transfer deadline is total elapsed time, not idle time, contrary to its own
  doc** (correctness). `TIMEOUT`'s doc at `fd.rs:78` says "how long a transfer
  may make no progress before it is abandoned", but `deadline_nanos` is computed
  once in `Reading::new`/`Writing::new` and never advanced when `moved` is true.
  A paste streaming steadily but taking over 2 s in total — a large image, well
  within the 64 MiB `MAX_BYTES` cap — is reported `State::Failed` and surfaces
  as `ClipboardContent::Unavailable` despite having delivered most of its bytes.

### Low

- **`crates/crcbl-shell/src/wayland/mod.rs:2240` (also `:2525`, `:4187`) —
  `Conn::destroy` used on interfaces that have protocol destructors**, which
  `Conn::destroy`'s own doc at `:1204` forbids. `zxdg_output_v1` has `destroy`
  at opcode 0, `wl_output` has `release` since v3 (bound at 4), `wl_seat` has
  `release` since v5 (bound at 8). On an output or seat hotplug the server-side
  object is never told, so the compositor keeps it until disconnect.
- **`crates/crcbl-shell/src/wayland/mod.rs:2235` — `global_remove` is only
  handled for `wl_output` and `wl_seat`.** Every other bound global
  (`xdg_wm_base`, `wl_compositor`, `wp_viewporter`, `wl_data_device_manager`)
  keeps a non-null, now-dead proxy in `Conn`, and `bind_global`'s `is_null()`
  guard means a re-advertised global is never re-bound. The next `create_window`
  then marshals `xdg_wm_base.get_xdg_surface` on a destroyed object — a protocol
  error that disconnects the client.
- **`crates/crcbl-shell/src/wayland/mod.rs:4778` — `clipboard_offer`'s
  `create_data_source` failure path orphans the previous source.** `previous` is
  `take()`n out of the device at `:4752`; the `source.is_null()` branch returns
  `Err` without destroying it and without putting it back. The proxy leaks, the
  compositor still believes we own the selection, and the next
  `wl_data_source.send` finds no seat so the peer gets an empty paste.
- **`crates/crcbl-shell/src/wayland/mod.rs:3592` — unbounded allocation from
  compositor-supplied mime lists.** `RawEvent::OfferMime` pushes with no cap and
  no dedup; `wl_data_offer.offer` may be sent arbitrarily many times.
  `Device::incoming` likewise grows one `Offer` per `data_offer` event, and
  every watched offer adds an entry to `Sink::objects`, which `kind_of`
  linear-scans for every event on the connection (`mod.rs:639`).
- **`crates/crcbl-shell/src/wayland/mod.rs:3690` — one full copy of the
  clipboard payload per `wl_data_source.send`, with no limit on concurrent
  writes.** A peer can call `wl_data_offer.receive` repeatedly; each `send`
  allocates `bytes_for(...).to_vec()` and holds it until the transfer completes
  or its 2 s deadline expires — a peer-controlled memory multiplier bounded only
  by the peer's fd limit.
- **`crates/crcbl-shell/src/wayland/mod.rs:3744` — `drag_entered` null-guards
  the version query but not the requests that follow.** It computes `version`
  behind `if offer.proxy.is_null()`, then calls `accept` and `set_actions` on
  the same pointer unguarded; the generated wrappers call
  `wl_proxy_get_version(proxy)` first, so a null proxy is a null deref.
  Unreachable today. Separately, when `claim` returns `None` the function sends
  no `accept` at all, leaving the drag source with no answer.
- **`crates/crcbl-shell/src/wayland/mod.rs:4123` — `constraint_destructor()`'s
  contract is not what the call sites claim** (YAGNI). It takes no arguments and
  unconditionally returns `zwp_locked_pointer_v1::destroy`;
  `let _ = zwp_confined_pointer_v1::REQ_DESTROY;` is a dead statement that only
  makes the claim look substantiated. It works because both interfaces put
  `destroy` at opcode 0.
- **`crates/crcbl-shell/src/wayland/mod.rs:2018` — `bind_global` re-watches an
  already-bound `xdg_wm_base`.** The `bind!` macro guards on `is_null()` but the
  `watch` call runs unconditionally, so a second `xdg_wm_base` global pushes a
  duplicate entry into `Sink::objects` and makes `wl_proxy_add_dispatcher`
  return -1 with a libwayland error log.
- **`crates/crcbl-shell/src/wayland/e2e.rs:177,200` — `map_window` leaks its
  private `wl_registry` and `wl_shm` proxies on every call.** (The
  `wl_shm_pool`/`wl_buffer` leaks _are_ documented at `:218`; these two are
  not.) `create_mapped` calls it once per window per test, across ~20 tests.
- **YAGNI — write-only state** (verified by grep): `Seat::press_serial`
  (`:1488`), `PointerFrame::axis_source` with `RawEvent::PointerAxisSource` and
  its decoder arm (`:1538`, `:1026`, `:2663`), `WlWindow::title` (`:1689`),
  `Output::logical_height` (`:1371`), and `RawEvent::DragMotion::time`, decoded
  then discarded at `:3632`.
- **DRY** — `self.seats[index].data.as_ref()/as_mut().and_then(...)` appears ~10
  times across `process_data`, `drag_entered`, `drag_dropped`, `clear_source`,
  `clipboard_offer`; one `data_device_mut(index)` helper collapses all of them.
  `PhysicalPoint::new(keymap::fixed_to_f64(x) * scale, …)` is written out four
  times (`:2550`, `:3005`, `:3648`, `:3773`). `destroy_offer` takes
  `&data::Offer` but reads only `.proxy`, so three call sites build a throwaway
  `Offer` to pass a pointer.
- **Adjacent, reached from `mod.rs:2826`**:
  `crates/crcbl-shell/src/linux/xkb.rs:585` mmaps the compositor-declared `size`
  then reads `*mapped.add(size - 1)`. `mmap` succeeds past the end of the file,
  so a compositor declaring a `wl_keyboard.keymap` size larger than the memfd it
  sends causes **SIGBUS** rather than an error. Fix (fstat and clamp) is in that
  file.

**Notes.** Unusually careful FFI: argument arrays are generated rather than
transcribed, SAFETY notes are almost always accurate, CString temporaries are
correctly bound at all six marshalling sites, fd ownership funnels through a
single `adopt`, and the destroy-order and version-gating rules are right. The
defects cluster in two places — the `Sink` pointer's provenance, and arithmetic
that trusts compositor-supplied values. The `e2e.rs` scaffolding is measurably
less careful than the shipping code.

## crcbl-shell — X11 backend

### High

- **`crates/crcbl-shell/src/x11/mod.rs:439` — `Conn::get_property` accumulates a
  peer-controlled property with no cap; the clipboard's `MAX_BYTES` guard does
  not cover the direct path** (security). The loop reads `CHUNK_WORDS` (4 MiB)
  at a time and follows `bytes_after` until exhausted, so the returned `Vec`'s
  size is chosen entirely by whoever wrote the property. `selection::MAX_BYTES`
  is only consulted in `Read::on_chunk` (`selection.rs:244`), i.e. after the
  bytes are already resident. On the non-`INCR` path (`xselection.rs:221` →
  `selection.rs:230`) there is **no** size check at all: a clipboard owner
  answering `ConvertSelection` with one multi-gigabyte property makes this
  process allocate all of it, then `value.to_vec()` copies it again.
  `detect_window_manager` and `read_xft_scale` read root-window properties
  through the same helper.

### Medium

- **`crates/crcbl-shell/src/x11/shell.rs:460` — `wait_events` can block
  indefinitely while decoded events are already pending** (correctness). The
  comment at `:471` asserts everything queued was drained by the last `pump`,
  but `collect_events` stops at `MAX_EVENTS_PER_PUMP = 4096` (`input.rs:113`)
  and `pump` delivers only `queue.len()` per call. With a full batch pending and
  no selection transfer outstanding, `wait_events(None)` polls only the socket
  fd and sleeps until _new_ traffic arrives, stalling a frame loop that has 4096
  undelivered events in memory.
- **`crates/crcbl-shell/src/x11/shell.rs:322` / `input.rs:452` — window
  destruction handles outstanding clipboard reads two different, both-wrong
  ways** (correctness). `destroy_window` does
  `self.reads.retain(|read| read.window != window)` and never emits an answer,
  so an accepted `ClipboardRequestId` is silently dropped — the opposite of the
  "answered exactly once" property `selection.rs` claims is structural.
  `handle_destroy` (a server-driven `DestroyNotify`, e.g. `xkill`) removes the
  window from the pool but does **not** touch `self.reads`, so the read
  survives, `settle`'s `AcknowledgeChunk` arm silently no-ops on `xid == 0`, and
  2 s later `service_transfers` emits `ClipboardData` naming a `WindowId`
  already reported as `WindowDestroyed`. Neither path prunes `self.writes`.
- **`crates/crcbl-shell/src/x11/shell.rs:538` — `set_pointer_mode` releases the
  grab unconditionally and leaves `pointer_mode` inconsistent** (correctness).
  Line 551 issues `xcb_ungrab_pointer` before knowing whether the new grab will
  succeed. If `GrabPointer` then returns a non-`Success` status the function
  returns `Err` while the window's recorded `pointer_mode` still says
  `Locked`/`Confined` and no grab is held. The same call also silently breaks a
  grab held for a _different_ window of the same shell.
- **`crates/crcbl-shell/src/x11/shell.rs:703` — `clipboard_offer` claims
  `CLIPBOARD` even when every offer was filtered out** (correctness). The
  `offers.is_empty()` release check tests the _caller's_ slice; the `retain` at
  `:731` then drops every mime with no interned atom (`MimeType::Other`). An
  offer list of only `Other(...)` leaves `self.offers` empty, yet `:733` still
  takes ownership. `answer_selection_request` then returns `0` for everything
  including `TARGETS`, so this process owns the desktop clipboard and refuses
  every conversion until it releases it.
- **`crates/crcbl-shell/src/x11/monitors.rs:241` — `refresh_of` re-issues a full
  `GetScreenResourcesCurrent` per CRTC** (perf). `enumerate_monitors` calls it
  once per CRTC, each doing a fresh synchronous round trip for the entire
  screen-resources reply purely to look up one mode id; `read_output_name` adds
  another per output. A six-monitor desktop pays twelve extra round trips plus
  six large reply allocations on connect and again on every RandR change — in
  the same file whose sibling `read_crtcs` is written two-phase specifically to
  avoid this.

### Low

- **`crates/crcbl-shell/src/x11/input.rs:661`, `xselection.rs:626`, `e2e.rs:836`
  — `assume_init()` on a possibly-partially-initialized `T`**
  (unsafe-soundness). All three copy `size_of::<T>().min(raw.len())` bytes then
  call `assume_init()`. If `raw` is shorter than `T`, the tail is uninitialized
  and reading it is UB regardless of "every bit pattern is valid" — uninit is
  not a bit pattern. Latent today; the `.min()` is what makes a short slice
  silently unsound rather than an error, and `input.rs`'s `debug_assert!`
  disappears in release.
- **Same three sites — three byte-identical copies of the same helper**
  (`read_event`/`read_wire`/`wire`) (DRY). One `pub(super)` helper in `ffi`
  would carry one audited proof instead of three.
- **`crates/crcbl-shell/src/x11/mod.rs:520` — `Conn::send_event<T>` is generic
  and unconstrained, but `xcb_send_event` unconditionally reads 32 bytes**
  (unsafe-soundness). The SAFETY comment discharges the obligation by appealing
  to an invariant the signature does not express, in a module that also defines
  `SelectionRequestEvent` (28 bytes) and `FocusEvent` (12 bytes). A
  `const { assert!(size_of::<T>() == 32) }` would make it structural.
- **`crates/crcbl-shell/src/x11/xselection.rs:197` — `advertised_targets` uses
  `dedup()`, which only removes _consecutive_ duplicates** (correctness). Two
  `MimeType::TextUtf8` offers (`clipboard_offer` does not dedup) append
  `text_targets()` twice, producing four non-adjacent repeats that survive into
  the `TARGETS` property.
- **`crates/crcbl-shell/src/x11/xselection.rs:497` — `refresh_server_time`
  re-enters `drain()` from inside the synchronous `clipboard_offer`**
  (correctness). `clipboard_offer` captures `xid` before the probe; the probe's
  `drain()` can process a `DestroyNotify` for that very window, after which
  `shell.rs:733` claims the selection on a destroyed XID and records it as
  `owner_window`.
- **`crates/crcbl-shell/src/x11/connect.rs:310` — `read_xft_scale` requires the
  whole `RESOURCE_MANAGER` property to be valid UTF-8** (correctness). X
  resource databases are Latin-1 by protocol; one non-ASCII byte anywhere in
  `.Xresources` makes `from_utf8` fail and silently resets the desktop scale to
  `DEFAULT_SCALE`, ignoring a pure-ASCII `Xft.dpi` line.
- **`crates/crcbl-shell/src/x11/keys.rs:122` vs `linux/keymap.rs:181` — the two
  Linux backends disagree about `PointerButton::Other(n)`** (correctness). X11
  stores the raw X11 button number (`Other(10)`); Wayland stores the raw evdev
  code (`Other(0x115)`). `keymap.rs`'s own doc says "inventing an index would
  make two backends disagree about what `Other(2)` means" — which is exactly
  what happens, so an input profile saved under one backend binds a different
  physical button under the other.
- **`crates/crcbl-shell/src/x11/xselection.rs:114` — no ICCCM `TIMESTAMP` or
  `MULTIPLE` target** (correctness). `mime_for_target` returns `None` for both,
  so the owner answers `property = 0`. `TIMESTAMP` is required of every
  selection owner and is what a peer uses to detect that the selection changed
  under it.
- **`crates/crcbl-shell/src/x11/input.rs:702` — `fp3232` divides the fraction by
  `u32::MAX` instead of `2^32`** (correctness). ~2.3e-10 relative error,
  unobservable in practice, but the function's whole reason for existing is
  being exact about this format.
- **`crates/crcbl-shell/src/x11/mod.rs:413` — the `16 => data.len() / 2` arm of
  `set_property` has no caller** (YAGNI). Every call site passes format 8 or 32.
- **`crates/crcbl-shell/src/x11/selection.rs:265`** — the doc block for
  `max_property_bytes` is attached to `choose_target` (missing blank line), so
  `max_property_bytes` ships undocumented (style).
- **`crates/crcbl-shell/src/x11/xselection.rs:344`** — `settle`'s
  `AcknowledgeChunk` and `Convert` arms duplicate the same six-line
  `(xid, property)` lookup; `window.rs:356`/`:361` are the same function
  differing only in element type (DRY).

**Notes.** High quality and unusually self-aware: FFI structs are size-asserted
against the C layout, reply ownership is paired one-for-one with `free_reply` at
every site checked, null replies are handled everywhere, the `Ext`/`OnceLock`
design avoids any `unsafe impl Sync`, and the genuinely hard parts (the
`GeGeneric` `full_sequence` insertion at offset 32, `FP3232` sign handling, the
`SelectionNotifyEvent` 24→32-byte padding, the detectable-auto-repeat argument,
the `SetSelectionOwner` timestamp race) are all correct and documented. On the
DRY question: **`selection.rs` and `xselection.rs` are not redundant** — the
former is a pure, server-free state machine and the latter the request/event
half that drives it; the split is what makes the `INCR` logic unit-testable.

## crcbl-shell — core, headless, web

### Critical

- **`crates/crcbl-shell/src/web/mod.rs:58` — the JS→wasm entry points push into
  a queue nothing ever drains.** `push_event` appends to the `WEB_EVENTS`
  thread-local; `WebShell::pump` (`:453`) drains `self.events`, a completely
  separate `Rc<RefCell<VecDeque>>`. `WEB_EVENTS` has exactly two references in
  the crate (the `thread_local!` and the write) and is never read; the `QUEUE`
  bridge at `:571` is written once in `WebShell::new` and never read either.
  Every `__crcbl_web_resize`/`_key`/`_pointer` call accumulates in an unbounded
  queue: the canvas window is never configured, `window_state().size()` stays
  `None` forever, no swapchain is created, and the process leaks one
  `ShellEvent` per browser input event for the page's lifetime. (correctness +
  unbounded allocation; verified by exhaustive grep of both statics.)

### High

- **`crates/crcbl-shell/src/headless.rs:1321` — `set_constraints` silently
  discards a pending mode change** (correctness). The `Some(config)` branch
  builds the replacement `PendingConfigure` from `config.mode`, the currently
  effective mode, rather than `state.settled_mode()`. `set_mode(Borderless)`
  followed by `set_constraints(...)` before the configure lands overwrites the
  borderless configure with a windowed one; `requested_mode` stays `Borderless`,
  `effective_mode()` stays `Windowed` and `mode_request_honoured()` is `false`
  forever. **[probe]**
  `requested=Borderless effective=Some(Windowed) honoured=false`.
- **`crates/crcbl-shell/src/headless.rs:1203` — a borderless window on a
  non-primary monitor gets the primary monitor's scale factor** (correctness).
  `let scale_factor = self.monitors[0].scale_factor;` is hoisted above the
  `match desc.mode`, and the `Borderless { monitor: Some(id) }` arm takes
  `monitor.size()` but never `monitor.scale_factor` — unlike `set_mode`
  (`:1297`), which gets it right. **[probe]** borderless on a 2560×1440 @2.0
  monitor reports `size=2560×1440, scale=1.0`.
- **`crates/crcbl-shell/src/web/mod.rs:389` — reported caps contradict what the
  backend does** (correctness). `caps()` keeps `RAW_POINTER_MOTION`, but
  `__crcbl_web_pointer` hard-codes `raw_delta: None` (`:171`) — the exact case
  the cap's doc says the bit must be _clear_ for. A first-person camera written
  correctly against caps gets zero aim input and no error. Same for `TEXT_IME`,
  `WINDOW_POSITION`, `SERVER_DECORATIONS`.
- **`crates/crcbl-shell/src/web/mod.rs:402` — `destroy_window` does not
  invalidate the handle** (correctness). It queues `WindowDestroyed` and returns
  `Ok`, but `self.window` is unchanged, so `window_state`, `surface_target`,
  `set_cursor` and `set_pointer_mode` all keep succeeding — breaking `Shell`
  obligation 1 ("stale handles fail cleanly") and handing out a
  `SurfaceTarget::Web` for a canvas the consumer was told is gone.

### Medium

- **`crates/crcbl-shell/src/geom.rs:251` — `PhysicalRect::contains` is inclusive
  on both far edges** (correctness). `(x - self.x) as i64 <= self.width as i64`
  should be `<`. **[probe]** for `PhysicalRect::new(0,0,1920,1080)` and
  `new(1920,0,1920,1080)`, `contains(1920, 0)` is `true` for both. The live
  caller is `x11/shell.rs:114`'s monitor lookup, so a pointer on the first
  column of the second monitor is attributed to the first. The existing test
  only exercises a point well inside.
- **`crates/crcbl-shell/src/web/mod.rs:62,115,160` — every web input event is
  stamped with the frame timestamp, on the wrong epoch** (correctness).
  `now_ms()` returns whatever the last `__crcbl_web_frame(now_ms)` stored, so
  all events between two rAF callbacks share one timestamp — precisely the frame
  quantization `event.rs:8` says makes a fast double-tap indistinguishable from
  a slow one. The shim should pass the DOM event's own `event.timeStamp`.
  `WebShell` also does not override `align_event_clock`, so timestamps stay on
  the page-load epoch (trait obligation 2 unmet), and `offset_ms as u64`
  discards sub-millisecond precision.
- **`crates/crcbl-shell/src/web/mod.rs:398` — `create_window` ignores its
  descriptor and never refuses a second window** (correctness). It returns
  `self.window` unconditionally: `title`, `size`, `constraints`, `mode`,
  `visible`, `accept_drops` are dropped, and a second call succeeds even though
  `caps()` clears `MULTI_WINDOW` and the trait docs promise
  `ShellError::Unsupported`.
- **`crates/crcbl-shell/src/web/mod.rs:427` — four methods never validate the
  window handle** (correctness). `set_title`, `set_visible`, `set_mode`,
  `set_constraints` take `_window` and return `Ok(())`; every other method in
  the impl does the check, so this is an omission rather than a policy.
- **`crates/crcbl-shell/src/web/mod.rs:521` — `reply_close_request` can never
  succeed, and `CloseReply::Close` does nothing** (correctness/YAGNI).
  `close_pending` is initialised `false` and never set `true`; even if
  reachable, `Close` and `Keep` have identical bodies. `focused` is set once and
  never updated, so `WindowState::focused` is permanently `true`.
- **`crates/crcbl-shell/src/backend.rs:226` — `web_open` always succeeds,
  including on desktop** (correctness). `CRCBL_SHELL=web` on Linux opens a
  `WebShell` that (given the queue bug) never produces an event, so the app
  hangs on the configure loop with no diagnostic — the registry's own docs make
  exactly this argument for why headless is `auto: false`. _Unverified half:_
  `CRCBL_CANVAS_ID` is read via `std::env::var`, which on
  `wasm32-unknown-unknown` always returns `NotPresent`, so `canvas_id` is always
  the `unwrap_or(0)` fallback; confirm by building for that target.

### Low

- **`crates/crcbl-shell/src/geom.rs:254`** — `x - self.x` is an `i32`
  subtraction before the `as i64` widening, so
  `PhysicalRect::new(-1, 0, w, h).contains(i32::MAX, 0)` panics in debug and
  wraps in release. Public, undocumented panic (correctness).
- **`crates/crcbl-shell/src/headless.rs:1366`** — `HeadlessShell` reports
  `EVENT_WAIT` (via `ShellCaps::DESKTOP`) but `wait_events` only increments a
  counter; the fidelity reference gives the opposite of what the bit promises.
- **`crates/crcbl-shell/src/headless.rs:549` vs `:1100`** — one state
  transition, two event shapes: `change_scale_factor` queues only
  `ScaleFactorChanged`, while `deliver_due_configures` queues that _plus_ a
  redundant `Resized` with the identical size. A consumer's swapchain-recreation
  count depends on which path produced the change.
- **`crates/crcbl-shell/src/headless.rs:367`** — `with_monitors` computes
  `monitor.id.0 + 1`, which overflows for `MonitorId(u32::MAX)`, and does not
  reject duplicate ids even though `monitor.rs` makes id uniqueness a backend
  obligation.
- **`crates/crcbl-shell/src/headless.rs:1191`** — `create_window` accepts
  `accept_drops: true` without `ShellCaps::DRAG_DROP`, deferring the refusal to
  `drop_file`. `tests/seam_from_outside.rs:50` builds exactly such a window
  without asserting the inconsistency.
- **`crates/crcbl-shell/src/web/mod.rs:99`** — `scale.max(1.0)` clamps a
  legitimate `devicePixelRatio` below 1 (browser zoom-out), producing a backing
  store that does not match the reported scale.
- **`crates/crcbl-shell/src/web/mod.rs:136`** — the deprecated JS `keyCode` is
  used as `Scancode` and `keysym` is always `Keysym::NONE`; `Scancode` is
  documented as the _physical_ code, so rebind menus get nothing usable.
  `KeyboardEvent.code` is the correct source.
- **`crates/crcbl-shell/src/web/mod.rs:179`** — `PointerButton::Other(n as u16)`
  truncates a `u32` from JS with no range check.
- **`crates/crcbl-shell/src/web/mod.rs:83`** — the
  `#[unsafe(no_mangle)] pub extern "C"` symbols are emitted on every target
  (`lib.rs:262` compiles the module unconditionally), so every native binary
  exports `__crcbl_web_*` — a link-time collision hazard for no benefit. The
  functions are also marked `unsafe` while containing no unsafe operation
  (YAGNI/style).
- **`crates/crcbl-shell/src/clipboard.rs:346`** — `ClipboardContent::into_bytes`
  has no caller in the workspace (YAGNI).
- **`crates/crcbl-shell/Cargo.toml:3`** — description still says "own Wayland
  and X11 backends" after the Web backend landed (style).
- **DRY** — the `PointerMode::Free => unreachable!(...)` arm plus the
  confine/lock message table is duplicated verbatim at `headless.rs:1392` and
  `web/mod.rs:484`; a `PointerMode::unsupported_what()` next to `required_cap()`
  in `cursor.rs` removes the copy. `WebShell`'s six-times-repeated window check
  wants one helper — the four methods that forgot it are the direct cost of not
  having one.

**Notes.** The platform-neutral core (`geom`, `event`, `clipboard`, `caps`,
`cursor`, `monitor`, `error`, `window`) is genuinely strong: the
physical/logical split, requested-vs-effective modelling, `ReceivedMime`'s
asymmetry, the `text/uri-list` parser and the three-outcome `ClipboardContent`
are well-argued and well-tested, with no `HashMap`, no `unsafe`, and no
iteration-order nondeterminism anywhere in scope. `HeadlessShell` is a serious
model rather than a stub. The Web backend is the outlier and reads as
unfinished; a single test that calls `__crcbl_web_resize` then `pump` would have
caught the critical finding.

## crcbl-vk — Vulkan backend

### High

- **`crates/crcbl-vk/src/command.rs:548` — a read-only depth attachment is
  declared in the wrong image layout** (correctness). `begin_render_pass`
  hardcodes `DEPTH_STENCIL_ATTACHMENT_OPTIMAL` for the depth (and stencil,
  `:572`) attachment, but `conv::state_masks(ResourceState::DepthStencilRead)`
  (`conv.rs:640`) transitions the image to `DEPTH_STENCIL_READ_ONLY_OPTIMAL`,
  and the seam explicitly permits that state (`crcbl-hal/src/command.rs:186`);
  `crcbl-render`'s `PassBuilder::depth_read` (`graph.rs:586`) emits it. Any
  depth-prepass-style pass begins rendering with an attachment layout that does
  not match the image's current layout — a hard VU violation that breaks the P1
  "zero validation errors" gate and yields undefined attachment contents.
- **`crates/crcbl-vk/src/device.rs:1718` — the submission counter is incremented
  before the submit can fail** (correctness). `submissions.fetch_add` runs
  before the `submit.signals` lookups (`:1721`, which `?`-return) and before
  `queue_submit2` (`:1752`). If any fails, no submission ever signals `value` on
  the retire timeline, so `poll_retire` is permanently stuck: the deletion queue
  never drains again (unbounded leak until `wait_idle`/`Drop`) and every
  `request_readback` without an explicit wait returns `Pending` forever.
- **`crates/crcbl-vk/src/device.rs:691` — a swapchain is retired without waiting
  on its armed acquire fences** (unsafe-soundness). `retire_swapchain` calls
  `vkDeviceWaitIdle` then `destroy_trash` destroys the acquire semaphores and
  fences (`:831`). `vkDeviceWaitIdle` does not complete a pending
  `vkAcquireNextImageKHR`, so the common sequence _acquire → resize →
  reconfigure/destroy before presenting_ destroys a fence still in use and a
  semaphore with a pending signal. The `acquire_armed` bookkeeping needed for
  this already exists in `FrameSync` and is simply not consulted.

### Medium

- **`crates/crcbl-vk/src/device.rs:1198` — `create_image` bypasses
  `conv::sample_count`** (correctness/DRY). It calls
  `vk::SampleCountFlags::from_raw(desc.samples.max(1))` directly.
  `conv::sample_count` exists precisely to reject non-powers-of-two (its doc
  explains `3` decodes as `TYPE_1 | TYPE_2`), and
  `create_graphics_pipeline_impl:609` uses it. `ImageDesc { samples: 3 }`
  silently reaches the driver as a two-bit mask instead of an
  `InvalidDescriptor`.
- **`crates/crcbl-vk/src/device.rs:872` — cross-device handle detection is
  structurally dead** (correctness). Every pool is per-device and every insert
  stamps `owner: self.inner.id` (all 20 insertion sites verified), so
  `entry.owner() != owner` is unreachable and `HalError::ForeignObject` can
  never be produced. Obligation 3 is satisfied only by accident: device B's
  handle usually fails to resolve in device A's pool. Once both devices have
  allocated the same slot index and generation, device A accepts device B's
  `BufferHandle` and writes/destroys its own unrelated object.
- **`crates/crcbl-vk/src/device.rs:1006` — `destroy_*` removes the pool row
  before checking the owner** (correctness). `destroy_buffer`, `destroy_image`
  (`:1234`), `destroy_semaphore` (`:1580`), `destroy_query_set`,
  `destroy_sampler`, `destroy_pipeline_handle` (`:333`), the four
  pipeline/bind-group destroyers and `destroy_swapchain` (`:1845`) all
  `remove(...)` then `if entry.owner != id { return; }`. The row is already gone
  and the entry dropped, so the driver object leaks; combined with the finding
  above, a colliding handle from another device destroys this device's object
  outright. `Instance::destroy_surface` (`instance.rs:695`) gets this right and
  is the model. The null backend asserts the correct behaviour
  (`crcbl-hal/src/null/tests.rs:1278`) — that assertion run against `crcbl-vk`
  would fail.
- **`crates/crcbl-vk/src/command.rs:247` — a requested queue-ownership transfer
  is silently dropped** (correctness). Both the buffer (`:247`) and image
  (`:288`) paths use
  `if let Some(transfer) = … && let (Ok, Ok) = (queue_family(from), queue_family(to))`;
  when either handle does not resolve (e.g. `QueueKind::Transfer` on a device
  that did not enable `TRANSFER_QUEUE`), the barrier is emitted with
  `QUEUE_FAMILY_IGNORED` on both sides and the resource is used on the second
  family with no transfer — defined result: undefined contents. Every other
  resolution failure in this file calls `self.fail(...)`.
- **`crates/crcbl-vk/src/pipeline.rs:1043` — descriptor buffer offsets are never
  alignment-checked** (correctness). `BindingResource::Buffer { offset }` is
  copied straight into `VkDescriptorBufferInfo` and `command.rs:763` passes
  `dynamic_offsets` straight to `vkCmdBindDescriptorSets`.
  `Limits::min_uniform_buffer_offset_alignment` is populated by `adapter.rs:286`
  and read by nothing, so a misaligned offset becomes a driver-side VU violation
  rather than an `InvalidDescriptor`.
- **`crates/crcbl-vk/src/pipeline.rs:380` — `BindGroupDesc::variable_count` is
  used unvalidated** (correctness). It drives both the pool size and
  `VkDescriptorSetVariableDescriptorCountAllocateInfo` with no check against
  `layout_binding_count`. A count above the layout's declared length fails
  `vkAllocateDescriptorSets` with a VUID instead of the named error the rest of
  the module is careful to produce.
- **`crates/crcbl-vk/src/device.rs:711` — no fallback when the preferred memory
  type is exhausted** (correctness). `allocate` calls `find_memory_type` once
  and maps any failure to `OutOfDeviceMemory`.
  `MemoryRequest::for_location(HostUpload)` prefers
  `DEVICE_LOCAL | HOST_VISIBLE`, which on a non-resizable-BAR discrete GPU is a
  256 MB heap; once full, uploads fail hard even though a plain host-visible
  type with gigabytes free satisfies `required`.
- **`crates/crcbl-vk/src/command.rs:622` — `end_render_pass` pops a debug label
  it may not have pushed** (correctness). `begin_render_pass` pushes only when
  `desc.label.is_some()` (`:603`), but `end_render_pass` pops whenever
  `label_depth > 0`; `end_compute_pass` (`:925`) is identical. An unlabelled
  pass opened inside a caller's `begin_debug_label` scope closes the caller's
  label early, corrupting the capture tree.

### Low

- **`crates/crcbl-vk/src/command.rs:561`** — the stencil-attachment decision
  keys off the ops, not the format. A `D32Float` attachment with a non-default
  stencil op gets a `pStencilAttachment` for a depth-only view (a VU violation);
  the encoder does not have the view's format to decide correctly.
- **`crates/crcbl-vk/src/conv.rs:637`** — `DepthStencilRead` names
  `FRAGMENT_SHADER` with no shader access bit. Harmless over-sync today, but it
  reads as an intent to cover sampling the depth buffer, which would need
  `SHADER_SAMPLED_READ` and `SHADER_READ_ONLY_OPTIMAL` — it will bite when P7's
  depth pyramid samples depth.
- **`crates/crcbl-vk/src/device.rs:1163`** — image creation validates only 2D
  extent: `mip_levels` is not checked against `log2(max extent)`, array layers
  not against `max_image_array_layers`, 3D depth not against `max_image_3d`, and
  an empty `usage` mask is not rejected.
- **`crates/crcbl-vk/src/debug.rs:283`** — the messenger callback can abort the
  process from inside the driver (unsafe-soundness). It allocates and dispatches
  into an arbitrary `log` backend with no `catch_unwind`; a panicking logger
  unwinds out of an `extern "system"` fn — abort on current Rust, still a
  process kill from a driver callback. The pointer handling itself (null checks,
  `Arc::into_raw`/`from_raw` pairing, `VK_FALSE` return) is correct.
- **`crates/crcbl-vk/src/instance.rs:842`** — the first `mapped.dedup()` is a
  no-op; the list is unsorted at that point.
- **`crates/crcbl-vk/src/adapter.rs:345`** — `VkPhysicalDeviceVulkan13Features`
  is chained onto every device regardless of reported API version, and
  `enumerate` keeps adapters failing the 1.3 floor — sorted by device type, so
  an unopenable discrete GPU still lands at `adapters()[0]`, which the crate
  docs say `apps/sandbox` takes blind.
- **`crates/crcbl-vk/src/device.rs:588`** — `queue_handle` synthesises from the
  kind index alone, so another device's `QueueHandle` resolves here and
  `submit`/`present` run on the wrong device's queue with no error.
- **`crates/crcbl-vk/src/command.rs:462`** — `fill_buffer` does not check that
  `offset`/`size` are multiples of 4, which `vkCmdFillBuffer` requires.
  Relatedly `conv::buffer_usage` (`conv.rs:204`) silently ignores
  `BufferUsage::QUERY_RESOLVE`; it happens to be covered by the unconditional
  `TRANSFER_DST`, but nothing says so.
- **`crates/crcbl-vk/src/device.rs:646`** — `poll_retire` swallows a failed
  `vkGetSemaphoreCounterValue`; on `ERROR_DEVICE_LOST` it looks identical to
  "nothing has retired yet", so a lost device shows up as a slow leak rather
  than a reported error.

### DRY / YAGNI

- **`command.rs:76,729,1152`, `pipeline.rs:115`** — the bound-pipeline tracking
  is entirely dead: `BoundPipeline::layout` carries `#[allow(dead_code)]`,
  `VkCommandEncoder::{graphics, compute}` are written and never read,
  `current_bind_point`'s second element is bound to `_bound`, and
  `PipelineEntry::layout` exists only to feed it. All vestigial since
  `bind_group`/`push_constants` gained explicit layout parameters.
- **`device.rs:196`** — `Trash::Swapchain` is never pushed into the
  `RetireQueue`; `retire_swapchain` hands it straight to `destroy_trash`, so the
  `Box` and its "by far the largest thing that can be parked" rationale describe
  a path that does not exist.
- **Three hand-maintained teardown sweeps** — `destroy_trash` (`device.rs:806`),
  `Drop for DeviceInner` (`:2546`), and `build_swapchain`'s `unwind` closure
  (`:2104`), each with its own ordering rules; `Drop` additionally re-implements
  per-object destruction for all thirteen pools. Any new resource kind must be
  added in three places to avoid a leak.
- **Eleven near-identical `destroy_*` bodies** differ only in pool and `Trash`
  variant; a `retire_from(pool, kind, to_trash)` helper would also be the single
  place to fix the remove-before-owner-check bug.
- **`debug.rs:197,314`** — the `Info`/`Verbose` arms are unreachable:
  `messenger_create_info` subscribes only to `ERROR | WARNING`.
- **`pipeline.rs:634`** — each shader stage looks its module up twice.
- **`mem.rs:49`** — `DeviceLocal`'s `preferred` equals its `required`, so the
  two-pass search degenerates for that location.

**Notes.** High quality and well-argued: the deletion queue keyed on a real
timeline value, the acquire-fence-per-slot ring, the `swapchain_owned` guards,
the `unwind` closures, the SPIR-V parser (bounds-checked, endianness-aware,
zero-word-count guarded, well tested), and the validation sink that makes "zero
validation errors" a failing assertion are all done properly, and the `unsafe`
blocks carry accurate safety comments. Host-visible memory correctly requires
`HOST_COHERENT`, designing the `nonCoherentAtomSize` hazard out. The defects
cluster where the code must agree with something outside the file. The e2e suite
covers clear/triangle/mesh goldens, resize storms, mid-flight destruction,
sync-validation provocation, tier reporting and timers — but not a read-only
depth pass, a failed submit, or acquire-then-reconfigure, which is exactly where
the three High findings sit.

## crcbl-hal — GPU seam + null backend

No unsoundness and no `unsafe` misuse; the crate's only `unsafe fn` is
`Instance::create_surface`, which the null backend implements without
dereferencing anything.

### High

- **`crates/crcbl-hal/src/null/mod.rs:1648` — `finish()` returns `Ok` on an
  unclosed pass; `crcbl-vk` returns `Err`** (correctness).
  `CommandEncoder::finish` is documented at `command.rs:721` as returning
  `HalError` for an unclosed pass. `crcbl-vk/src/command.rs:1054` does that;
  `crcbl-wgpu/src/command.rs:323` silently drops the open pass; null records a
  `ValidationError` and returns `Ok`. A suite that asserts `finish().is_ok()` on
  null (as `null/tests.rs:667` does) passes in CI and fails on Vulkan.
- **`crates/crcbl-hal/src/null/mod.rs:713,725` —
  `create_bind_group`/`update_bind_group` perform no layout validation**
  (correctness). Null only checks that handles resolve.
  `crcbl-vk/src/pipeline.rs:1026` rejects an entry naming an undeclared binding
  and a resource whose kind mismatches the slot, and `:473` rejects
  `update_bind_group` on a layout without `UPDATE_AFTER_BIND` — the error
  `device.rs:514` explicitly promises. `BindGroupDesc::variable_count` is never
  read by null.
- **`crates/crcbl-hal/src/null/mod.rs:766` — `range.offset + range.size`
  overflows `u32`** (correctness).
  `PushConstantRange { offset: u32::MAX, size: 1 }` panics in debug and wraps to
  `0` in release, where it compares `0 > max_push_constant_size` and **passes**
  validation. `crcbl-vk/src/pipeline.rs:530` uses `saturating_add`. The same
  expression is repeated in the error message at `:769`.
- **`crates/crcbl-hal/src/null/mod.rs:879` — `query_results` ignores
  `first_query` and the output length** (correctness). The parameter is
  `_first_query` and the set's `count` is not even stored, so the range check
  `device.rs:592` documents cannot be performed. `crcbl-vk/src/device.rs:1502`
  returns `InvalidDescriptor` when the range overruns. Reading 64 timestamps out
  of a 4-query set gets zeros on null and an error on Vulkan.
- **`crates/crcbl-hal/src/null/mod.rs:85,944,1135` — `QueueHandle` sits outside
  the ownership mechanism** (api-contract). Queue handles are synthesised from
  `(1<<32)|index` rather than pooled, so the side-table owner check that
  `device.rs:112` obliges every backend to implement cannot apply. Null's
  `submit`, `present` and `create_command_encoder` ignore the queue argument
  entirely — a queue handle from another device, or a fabricated one, is
  accepted silently. Rule 3 has a hole the seam does not acknowledge.

### Medium

- **`crates/crcbl-hal/src/command.rs:540` — the documented `Send`-but-not-`Sync`
  encoder contract is contradicted by the actual bound** (api-contract). The doc
  says two threads may not record into one encoder, but
  `CommandEncoder: HalThreadSafe` and `HalThreadSafe: Send + Sync`
  (`threading.rs:15`) require every implementor to be `Sync`.
  `crcbl-wgpu/src/cell.rs` exists partly to satisfy that. Either the doc or the
  bound is wrong.
- **`crates/crcbl-hal/src/null/mod.rs:543` — `poll_readback` consumes latency
  before validating `out.len()`** (correctness). `polls_remaining` is
  decremented at `:557` and the length check happens at `:563`, so a poll that
  returns `InvalidDescriptor` still advances the simulated readback — a caller
  that gets the slice length wrong once observes a shorter latency than
  `set_readback_latency` configured.
- **`crates/crcbl-hal/src/null/mod.rs:495,546,577` — check-then-relock with
  `.expect("checked above")`** (correctness). `check()` takes the recorder mutex
  and drops it; the caller re-locks and unwraps. `Device` is
  `&self + Send + Sync`, so a concurrent `destroy_buffer` between the two locks
  turns "checked above" into a panic instead of `InvalidHandle`.
  `write_buffer:450` handles the `None` case correctly; `request_readback` and
  `poll_readback` do not.
- **`crates/crcbl-hal/src/null/mod.rs:598` — image validation uses the 2D limit
  for every image type and never checks samples** (correctness).
  `longest = max(width, height)` is compared against `max_image_2d` regardless
  of type; `max_image_3d` is never read and a `D3` image's `depth_or_layers` is
  checked against nothing. `samples` is never validated — and `Limits` has no
  sample-count field, so the `create_image` error documented at `device.rs:431`
  is unrepresentable in the seam.
- **`crates/crcbl-hal/src/resource.rs:189` — `full_mip_levels` ignores
  `depth_or_layers`** (correctness). The doc says
  `floor(log2(max_dimension)) + 1` but only width and height participate; a
  `4×4×64` `D3` volume reports 3 levels where the real chain is 7, so a caller
  building a full mip chain under-allocates.
- **`crates/crcbl-hal/src/null/mod.rs:973,1165` — swapchain format is
  unvalidated and image count is clamped to a hardcoded range** (correctness).
  `SwapchainDesc::format` is documented (`swapchain.rs:241`) as "must be one of
  `SurfaceCaps::formats`"; null accepts any `Format`, including `D32Float`.
  `build_ring` clamps `image_count` to a literal `2..=3` rather than to the
  `min_image_count`/`max_image_count` the same backend reports, so the clamp and
  the advertised caps can drift.
- **`crates/crcbl-hal/src/format.rs:99` — `block_size` is meaningless for
  depth/stencil formats and there is no per-aspect accessor** (correctness).
  `D32FloatS8Uint => 8` and `D24UnormS8Uint => 4` describe neither the
  depth-aspect footprint nor the stencil-aspect one that a `BufferImageCopy`
  needs. Any consumer computing a staging size as `w * h * block_size()` for a
  depth copy gets the wrong answer, with no `texel_size(aspect)` offered.
- **`crates/crcbl-hal/src/null/mod.rs:684` — bind-group-layout validation omits
  two checks `crcbl-vk` performs** (correctness). No check of `entry.count`
  against `max_bindless_descriptors` (`crcbl-vk/src/pipeline.rs:183`) and no
  rejection of duplicate `binding` numbers.

### Low

- **`crates/crcbl-hal/src/pipeline.rs:141`** — "Binding slots, in any order"
  contradicts the enforced rule: both null (`null/mod.rs:693`) and `crcbl-vk`
  (`pipeline.rs:170`) require `VARIABLE_COUNT` on the **last slice element**,
  while Vulkan's rule is "highest binding _number_". With entries genuinely in
  any order those differ, and the doc invites the reordering that breaks it.
- **`crates/crcbl-hal/src/null/mod.rs:1118`** — `unwrap_or(1)` cannot prevent
  the modulo-by-zero it looks like it prevents: `u32::try_from(0usize)` is
  `Ok(0)`. Unreachable today because `build_ring` clamps to `2..=3`, but the
  guard is decorative.
- **`crates/crcbl-hal/src/null/mod.rs:297`** — `.union(desc.required_features)`
  is dead code; `create_device` already returned `Err(UnsupportedFeatures)`
  unless `required_features ⊆ caps.features`.
- **`crates/crcbl-hal/src/null/mod.rs:187,302`** — `implicit_acquire` is derived
  from _adapter_ caps, not device caps, so a device opened with
  `required_features: Features::COMPUTE` reports no `TIMELINE_SEMAPHORE` yet
  still receives acquire/present semaphores, making the backend's own stated
  rule false for that configuration.
- **`crates/crcbl-hal/src/caps.rs:205`** — most `Limits` fields are written by
  backends and read by nobody: `max_image_3d`, `max_uniform_buffer_range`,
  `max_storage_buffer_range`, `optimal_buffer_copy_offset_alignment`, both
  min-offset-alignments, `max_compute_workgroup_size`,
  `max_compute_invocations_per_workgroup`. None is enforced by any backend's
  descriptor validation (YAGNI).
- **`crates/crcbl-hal/src/caps.rs:183`** — `SHADER_DEBUG_PRINTF` is never set or
  queried (YAGNI).
- **`crates/crcbl-hal/src/format.rs:129`, `pipeline.rs:594`** —
  `Format::block_extent` and `BlendState::additive` are used only by their own
  unit tests. `block_extent` in particular is what a correct compressed-texture
  copy would need, and its absence from every call site suggests copy footprints
  are computed without it (YAGNI).
- **`crates/crcbl-hal/src/resource.rs:25`, `pipeline.rs:31`, +5 modules** — 16
  hand-written `pub enum Marker {}` + `pub type XHandle = Handle<Marker>` pairs,
  identical modulo the name. `null/record.rs:38/76/97` compounds it: the
  `ObjectKind` enum, its `ALL` array (with a test asserting the two stay in
  declaration order) and its `name()` match are three parallel lists, and
  `Command::name()` at `:300` is a 32-arm match restating every variant name.
  One declarative macro collapses all of it (DRY).
- **`crates/crcbl-hal/src/null/mod.rs:85`** — synthesised queue handles collide
  in `to_bits` with the first pooled object of every kind, making
  `Event::Submitted { queue }` indistinguishable from the first buffer in the
  recorded stream this backend exists to make readable (style).
- **`crates/crcbl-hal/src/caps.rs:404`** — the test named
  `tier_a_requires_the_documented_four_capabilities` asserts four of the six
  flags in `TIER_A`; dropping `MULTI_DRAW_INDIRECT` or `TIMELINE_SEMAPHORE`
  would not fail it (style).

**Notes.** The handle scheme is sound — 32-bit index + `NonZeroU32` generation,
generation exhaustion retires the slot rather than wrapping, no `as`-truncation
on the handle path, and `Handle`'s manual trait impls correctly avoid inheriting
`T`'s auto traits. The five High findings share one root cause: the null backend
is documented as the reference validator but its validation is a strict subset
of `crcbl-vk`'s, so the graph-compile suite it was built to host green-lights
streams Vulkan rejects.

## crcbl-render + crcbl-wgpu

### Critical

All six are in `crcbl-wgpu` and compound: the first `acquire_next_frame` panics
before anything else is reached, so this backend cannot have been run against a
real surface.

- **`crates/crcbl-wgpu/src/device.rs:751` — `acquire_next_frame` builds handles
  from a slot index, so `from_bits` always returns `None` and the `.expect`
  panics.** `images.insert(texture).index() as u64` puts the slot index in the
  low half and **zero** in the generation half; `Handle::from_bits`
  (`crcbl-core/src/handle.rs:89`) returns `None` when `(bits >> 32) == 0`, so
  `:764` `.expect("valid handle")` panics on the first acquire. Same defect at
  `:783`/`:795`. Should be `.to_bits()`.
- **`crates/crcbl-wgpu/src/conv.rs:87` — `map_image_usage` drops
  `DEPTH_STENCIL_ATTACHMENT`, so the depth buffer maps to empty
  `TextureUsages`.** `TransientImageDesc::scene_depth` (`graph.rs:1924`) sets
  exactly that usage and nothing else; the mapper has no arm for it, so
  `create_image` asks wgpu for a texture with `TextureUsages::empty()`. wgpu
  rejects it, and even if it did not the texture could never be a depth
  attachment. Every forward pass fails on this backend.
- **`crates/crcbl-wgpu/src/command.rs:94` — all five copy/fill commands are
  silent no-ops that report success.** `copy_buffer_to_buffer`,
  `copy_buffer_to_image`, `copy_image_to_buffer`, `copy_image_to_image`,
  `fill_buffer` have empty bodies. `ForwardRenderer::upload` (`forward.rs:599`)
  and `UiRenderer::upload_texture_r8` (`ui_pass.rs:485`) both return `Ok`, so
  the cube geometry and the glyph atlas are never uploaded and the renderer
  draws from uninitialised device-local memory with no error anywhere.
- **`crates/crcbl-wgpu/src/device.rs:96` — `write_buffer` targets buffers
  created without `COPY_DST`.** `map_buffer_usage` only sets `COPY_DST` for
  `BufferUsage::TRANSFER_DST`. `forward.rs:168` creates the frame-uniform buffer
  with `UNIFORM` and `ui_pass.rs:137` with `STORAGE`/`INDEX`;
  `Queue::write_buffer` on those is a wgpu validation error, so every per-frame
  uniform write fails.
- **`crates/crcbl-wgpu/src/device.rs:89` — `mapped_at_creation` is set for every
  `HostUpload` buffer and `unmap()` is never called.** The buffer stays
  CPU-mapped forever, so the first write/bind/draw against it errors or panics
  inside wgpu. It also silently requires the size to be a multiple of 4, which
  nothing enforces.
- **`crates/crcbl-wgpu/src/device.rs:158` — `create_image_view` passes the
  `u32::MAX` "all remaining" sentinel straight to wgpu.**
  `ImageSubresourceRange::ALL` is `u32::MAX` (`hal/resource.rs:294`) and
  `TransientPool::image` builds every transient view with `all(format)`
  (`transient.rs:188`). `mip_level_count: Some(u32::MAX)` is out of range in
  wgpu; the sentinel must become `None`. Every graph-owned view creation fails.

### High

- **`crates/crcbl-wgpu/src/device.rs:36` — `WgpuDevice::new` ignores
  `DeviceDesc`, so `required_features` is never honoured** (correctness).
  `_desc` is unused, `required_features: wgpu::Features::empty()` is requested,
  and `caps.features` is hardcoded empty. `DeviceDesc::for_adapter` demands
  `Features::TIER_A` and the hal contract (`hal/device.rs:184`) says creation
  must fail with `UnsupportedFeatures` naming the gap — instead it succeeds and
  reports a device with no features.
- **`crates/crcbl-wgpu/src/device.rs:57` / `instance.rs:33` — fabricated
  capabilities** (correctness). `Limits::desktop()` and `Features::empty()` for
  every adapter; `adapter.features()`, `adapter.limits()` and `info.device_type`
  are all discarded (`DeviceType::Other` for everything). The reported limits
  are internally inconsistent with the reported features
  (`max_push_constant_size: 128` while `PUSH_CONSTANTS` is absent), and a caller
  sizing off `max_storage_buffer_range: u32::MAX` will exceed what wgpu granted.
- **`crates/crcbl-render/src/ui_pass.rs:202` — the UI pipeline hardcodes
  `Format::Rgba8UnormSrgb` and the "reconfigured at draw time" comment is
  false** (correctness). Nothing rebuilds this pipeline.
  `apps/breakout/src/gpu.rs:210` picks the swapchain format from
  `caps.preferred_format()`, normally `Bgra8UnormSrgb`; under dynamic rendering
  the pipeline's colour-target format is checked against the attachment at
  pass-begin, so the UI pass errors on any other swapchain.
  `ForwardRenderer::new` takes `target_format` for exactly this reason;
  `UiRenderer::new` does not.
- **`crates/crcbl-render/src/ui_pass.rs:374` — `push_constants` is recorded
  unconditionally against a layout that may have no push-constant range**
  (correctness). `:179` builds the layout with `push_constants: None` when
  `Features::PUSH_CONSTANTS` is absent, but the pass body pushes regardless. On
  such a device the push is illegal; on `crcbl-wgpu` it compounds with
  `immediate_size: 0` and the empty `push_constants` body, so the shader's
  `viewport` block is never written and the UI is transformed by zeros.
- **`crates/crcbl-wgpu/src/command.rs:141` — `stencil_ops: Some(..)` is emitted
  for depth-only attachments** (correctness). The graph always fills
  `stencil_load`/`stencil_store` (`graph.rs:1351`) and this backend always wraps
  them in `Some`. The engine's depth attachment is `D32Float`, which has no
  stencil aspect; wgpu rejects a render pass supplying stencil ops for such a
  format, so the forward pass cannot begin.
- **`crates/crcbl-render/src/graph.rs:1590` — a range-scoped barrier is emitted
  with a `from` state taken from whole-image tracking** (correctness).
  `use_subresource` records a per-range barrier but `transition` updates one
  whole-image tracker. In `tests/graph_compile.rs:1455` the second pass emits
  `from: ShaderReadWrite` for **mip 1**, which never left `Undefined` — naming
  an `oldLayout` a subresource is not in is a validation error and makes
  contents undefined. The doc at `graph.rs:626` calls this "conservative"; it is
  under-specification, and the test asserts the buggy shape.
- **`crates/crcbl-wgpu/src/device.rs:749` — the acquired swapchain texture and
  view are inserted into the pools every frame and never removed**
  (correctness). `present` (`:811`) only drops `slot.acquired`; two `Pool` slots
  per frame, each holding an `Arc`, leak for the process lifetime.
- **`crates/crcbl-wgpu/src/command.rs:268,310`, `device.rs:328,534` — indirect
  draws, indirect dispatch and `update_bind_group` are silent no-ops or blanket
  errors** (correctness). The seam's docs (`hal/command.rs:35`) call indirect
  "the steady state"; a P7 GPU-driven frame renders nothing on this backend and
  reports no error.

### Medium

- **`crates/crcbl-render/src/ui_pass.rs:264,279`** — element counts compared
  against byte counts: `vb_needed` is bytes but `last_vertex_count[idx]` stores
  element count. Because the stride exceeds 1 the comparison is almost always
  true, so both geometry buffers and the frame bind group are destroyed and
  recreated **every frame** even in steady state.
- **`crates/crcbl-render/src/ui_pass.rs:267,281,304`** — destroy-then-create
  leaves a dangling handle on the error path: `destroy_buffer` runs before
  `create_buffer(...)?`, so a failure leaves the field holding a destroyed
  handle and `UiRenderer::destroy` double-destroys it. `TransientPool::image`
  (`transient.rs:190`) shows the correct shape.
- **`crates/crcbl-render/src/forward.rs:572`** — `upload` leaks every resource
  created so far on any error path (`write_buffer?`, `create_buffer?`,
  `finish?`, `submit?`, `wait_idle?` all return without destroying
  `staging`/`target`/the command buffer). `ForwardRenderer::new` has the same
  shape; same defect in `ui_pass.rs:419`. The leak test only exercises the
  success path.
- **`crates/crcbl-render/src/forward.rs:350`** — the aspect guard covers a zero
  height but not a zero width; `extent.0 == 0` yields `aspect == 0.0`, tripping
  `Projection::matrix`'s `assert!(aspect > 0.0)` (`camera.rs:110`). A window
  minimised to zero width crashes the frame loop.
- **`crates/crcbl-render/src/graph.rs:1718`** — a queue transfer emits only the
  acquire half, never the release. `QueueTransfer`'s contract
  (`hal/command.rs:320`) is two barriers with identical fields.
  `tests/graph_compile.rs:962` asserts only the acquire, so the model claimed to
  make a transfer queue "additive later" needs a rewrite, not an addition.
- **`crates/crcbl-wgpu/src/device.rs:605`** — `submit` discards waits and
  signals, so a timeline semaphore never advances and `semaphore_value` returns
  its initial value forever; the `filter_map` at `:610` also swallows any handle
  whose slot is missing or whose `buffer` is `None` and still returns `Ok(())`.
  With `wait_semaphores` returning `Ok(true)` unconditionally, frames-in-flight
  pacing is a no-op.
- **`crates/crcbl-wgpu/src/device.rs:279,288,295,281`** — panics on caller input
  inside `create_bind_group` (`.expect("stale buffer")`,
  `NonZero::new(size).unwrap()`) while the layout lookup two lines above returns
  a `HalError`. Same class at `command.rs:123,334`.
- **`crates/crcbl-wgpu/src/command.rs:127,139,143`** — `StoreOp` is ignored;
  every attachment is stored. `StoreOp::Discard` is what
  `PassBuilder::clear_depth` asks for precisely so a tiler does not write depth
  back — which matters most on the mobile/browser targets this backend exists
  for (perf).
- **`crates/crcbl-wgpu/src/conv.rs:20,25`** — silent format substitutions:
  `R11g11b10Float` maps to `Rgba16Float` (wgpu has `Rg11b10Ufloat`, and
  `unmap_format:253` maps back from it, so the pair is not an inverse), and the
  `_ =>` arm turns every unhandled format — including all eight BC formats and
  `Rgba32Float`/`R32Float`/`Rgb10a2Unorm` — into `Rgba8Unorm` behind a
  `log::warn`.
- **`crates/crcbl-wgpu/src/conv.rs:227`** — binding-type conversion hardcodes
  properties it was given: `SampledImage` always becomes
  `Float { filterable: true }, D2, multisampled: false`; `StorageImage` always
  `ReadWrite, Rgba8Unorm, D2`, discarding the `read_only` flag.
- **`crates/crcbl-wgpu/src/device.rs:422,434`** — `ColorTargetState::write_mask`
  is ignored (`ColorWrites::ALL` overrides it) and `ds.bias.constant as i32`
  narrows an `f32`, so any sub-unit constant bias becomes `0`.
- **`crates/crcbl-wgpu/src/device.rs:41` + `Cargo.toml`** — wasm32 gating is
  incomplete: `cell.rs` carefully swaps `Arc/Mutex` for `Rc/RefCell` on wasm32,
  but both constructors call `pollster::block_on` (which cannot block the
  browser main thread), `lib.rs:22` gates `create_native` off wasm32 with no
  replacement, and `SurfaceTarget::Web` returns `Unsupported("Web (P5.3)")`. The
  crate cannot be constructed at all on wasm32. `Cargo.toml` also has no wasm32
  target section, and `bytemuck`/`thiserror` are declared but unused.
- **`crates/crcbl-render/src/camera.rs:232`** — `depth_of`'s doc contradicts its
  guard: it claims `None` "for a point on or behind the eye plane", but the
  guard is `clip.w.abs() > f32::MIN_POSITIVE`, so a point behind the eye returns
  `Some` with a negative depth outside `0..1`.
- **`crates/crcbl-render/src/camera.rs:198`** — `view()` propagates NaN for a
  degenerate `up` (`Vec3::ZERO`, or parallel to `target - eye`). `Camera::up`'s
  doc states the requirement but nothing checks it, while `Projection::matrix`
  asserts on every other degenerate input — so this is the one caller bug that
  reaches the GPU as a silently blank screen.
- **`crates/crcbl-render/src/graph.rs:1835`** — duplicate declarations of one
  resource in the same state are accepted: `.color(img, ..).color(img, ..)`
  produces two colour attachments pointing at one view;
  `.read_image(x).read_image(x)` produces a duplicate access that no-ops.
- **`crates/crcbl-render/src/transient.rs:67`** — `TransientImageDesc` has no
  `mip_levels`, so `TransientPool::image` hardcodes `mip_levels: 1` and any
  `use_subresource(base_mip: 1, ..)` on a transient — the case
  `graph_compile.rs:1420` exercises — barriers a mip level the physical image
  does not have.

### Low

- **`crates/crcbl-render/src/ui_pass.rs:364`** — the UI pass re-sets viewport
  and scissor from its own `extent`, duplicating what `CompiledGraph::execute`
  already set from `pass.render_area` (`graph.rs:1093`); the two can disagree
  and the graph's is authoritative (DRY).
- **`crates/crcbl-render/src/ui_pass.rs:419` vs `forward.rs:572`** — two
  near-identical staging-upload helpers with hand-written barriers; `lib.rs:25`
  claims "the one barrier outside [the graph] is `forward`'s startup upload",
  which `ui_pass` falsifies (DRY).
- **`crates/crcbl-wgpu/src/command.rs:180`** — `rect.x as u32` wraps a negative
  `Rect2d` offset into a huge unsigned scissor (latent: the graph only passes
  0).
- **`crates/crcbl-wgpu/src/command.rs:58,71`** — `LoadOp::DontCare` maps to
  `Clear(0)`, adding a clear the caller asked to avoid; for depth it coincides
  with the reversed-Z far plane, which makes the coincidence load-bearing
  (perf).
- **`crates/crcbl-wgpu/src/command.rs:84`** —
  `begin_debug_label`/`end_debug_label`/`insert_debug_marker` call
  `encoder_mut()` unconditionally; invoked while a pass is open they record on
  the encoder behind an active `RenderPass<'static>`, which wgpu rejects.
- **`crates/crcbl-wgpu/src/device.rs:602`** — `destroy_command_buffer` is empty,
  so a command buffer created but never submitted leaks its slot and its
  `wgpu::CommandBuffer`.
- **`crates/crcbl-render/src/timing.rs:237`** — `0..self.capacity * 2` overflows
  `u32` for caller-supplied `max_passes > u32::MAX/2`.
- **`crates/crcbl-render/src/forward.rs:406,194`** — `let _ = vertices;` inside
  the forward pass closure and `let _ = index;` in the ring loop are vestigial
  (style).
- **`crates/crcbl-render/src/transient.rs:131`** — `TransientPool::frames` is
  incremented and never read (YAGNI).
- **`crates/crcbl-wgpu/src/conv.rs:109,186`** —
  `map_load_op`/`map_store_op`/`map_stencil_op` are `#[allow(dead_code)]` and
  unreachable; `command.rs` has its own private copies (YAGNI/DRY).
- **`crates/crcbl-render/src/graph.rs:500,606,629,376,400,427`,
  `timing.rs:197`** — `PassBuilder::on_queue`, `read_subresource`,
  `use_subresource`, `RenderGraph::create_buffer`, `import_buffer`,
  `pass_count`, `physical_buffer_count`, `images_alias`, `barrier_batches`,
  `PassTimers::capacity` have no callers outside the module and its tests. The
  entire buffer half of the graph — `BufferId`, `BufferNode`, `BufferSource`,
  `buffer_slots`, `transient_buffers`, `GraphBufferBarrier`,
  `TransientPool::buffers` — is exercised only by tests (YAGNI).
- **`crates/crcbl-wgpu/src/resources.rs:26,47,69`** —
  `SwapchainSlot::surface_handle_id`, `CommandBufferSlot::label`,
  `Pools::query_sets`, `SurfaceSlot::platform` are all `#[allow(dead_code)]`
  (YAGNI).

**Notes.** The graph's core is sound where it matters most: the interval-packing
in `assign` (`graph.rs:1765`) is correct — `free_from` is monotonically
non-decreasing per slot because assignment proceeds in first-use order, so two
aliased transients can never be simultaneously live; `order.sort_by_key` is
stable and `colors.sort_by_key` is on the caller's explicit index, so no
`HashMap` iteration order reaches recorded command order; declaration order is
execution order by design; cross-frame and cross-alias `from`-state handling is
right and well tested. No cycle detection, topological sort, or pass culling
exists — both documented. No frustum-plane extraction exists in `camera.rs`.

## crcbl-net + crcbl-server + crcbl-client

### High

- **`crates/crcbl-server/src/lib.rs:219` — unauthenticated acks let an off-path
  attacker permanently stall replication** (security). `process_inbound_message`
  feeds any decodable ack straight into `SessionManager::handle_ack`
  (`session.rs:150`) with no session-state check and no authentication; the only
  gate is "tick is present in my baseline ring". Tick numbers are in every
  snapshot in cleartext, so a spoofer floods `encode_ack(ZERO, newest_tick)`;
  the server then delta-encodes against a baseline the real client never
  applied, the client rejects every delta (`client/src/lib.rs:422`), and because
  fresh forged acks keep arriving the baseline never falls out of the 64-entry
  ring, so the keyframe fallback at `server/src/lib.rs:416` never triggers. One
  spoofed packet self-heals after ~64 ticks; a spoofed _stream_ is a permanent,
  silent desync.
- **`crates/crcbl-client/src/lib.rs:422` — client has no recovery when the
  server's baseline is _ahead_ of it** (correctness). The re-ack repair path
  only fires when `delta.baseline_tick < baseline.tick`. When the server deltas
  against a tick the client never reached, the client silently `continue`s
  forever and never re-announces its true baseline or requests a keyframe. There
  is no "N ticks without progress → force keyframe" on either side. This is what
  turns the finding above from a hiccup into a hang.
- **`crates/crcbl-client/src/lib.rs:391` — one forged `Reject` permanently
  wedges a client** (security). `handshake_rejected = true` is terminal:
  `update()` never sends another Hello, for the lifetime of the object, for
  _any_ reject code including transient ones (`server_full`, entropy failure
  0x06). An attacker who observes one Hello (generation is plaintext) replies
  with a forged Reject carrying that generation. The resume token is also
  transmitted in the clear by `encode_handshake_result` (`codec.rs:360`), so an
  on-path observer can hijack the session.

### Medium

- **`crates/crcbl-client/src/lib.rs:399,438` — "trusted" decode limits are
  unlocked by an unauthenticated message** (security). `handshake_complete` —
  set purely from an unauthenticated `Accept` — switches decoding from
  `decode_delta`/`apply` to `decode_trusted_delta`/`apply_trusted`, raising the
  system cap from 256 to the packet maximum. The trusted/untrusted split buys
  nothing an attacker cannot flip.
- **`crates/crcbl-client/src/lib.rs:369` — client has no inbound rate limit at
  all** (security/DoS). `recv_snapshots` drains
  `while let Some(msg) = self.transport.recv()?` with no message or byte budget,
  unlike the server's `InboundRateLimiter`. Each accepted delta costs a full
  baseline clone plus a second full clone inside `apply_inner` (`delta.rs:505`)
  plus a full re-serialisation (`delta.rs:562`) — up to
  `MAX_BASELINE_ENCODED_BYTES` (256 KiB) copied ~3× per packet, driven entirely
  by the peer's send rate.
- **`crates/crcbl-server/src/lib.rs:174,189` — a single rate-limited message
  aborts the whole drain loop** (correctness/DoS). `process_inbound_message`
  returns `false` on rate-limit and both loops `break`, so the _reliable_
  control queue stops being drained as soon as the shared budget trips on an
  unreliable packet — head-of-line blocking on handshakes and acks, which the
  reliable/unreliable split exists to prevent.
- **`crates/crcbl-client/src/lib.rs:167,199` — interpolation alpha is not
  derived from snapshot ticks** (correctness). `interpolate` lerps
  `prev → current` using the _local_ `FrameClock` alpha, which resets to 0 every
  local tick. With packet loss or a server tick rate ≠ client tick rate, the
  same pair is re-lerped each local tick, so the entity slides prev→current,
  snaps back, and repeats. Nothing uses the tick delta between the two buffered
  snapshots, and `server_tick` from `Accept` is discarded (`:384`).
- **`crates/crcbl-server/src/lib.rs:388` — `system_id` is the schedule index,
  not a stable id** (correctness). `writer.write_system(idx as u32, data)`
  derives the replicated id from iteration position; registering or removing a
  system at runtime silently remaps ids and the client applies system A's blobs
  into system B's baseline map with no error, because the protocol treats
  `system_id` as opaque.
- **`crates/crcbl-net/src/delta.rs:505,562` +
  `crates/crcbl-server/src/lib.rs:402` — delta path is O(full state) per tick**
  (perf). `apply_inner` clones the entire `systems` map, then
  `baseline_to_snapshots` re-serialises the whole baseline. Per tick the server
  does `systems.to_vec()`, `from_trusted_snapshot` (parses every blob),
  `previous.cloned()`, then `encode_with_sector` (parses every blob again) —
  roughly five full-state copies/parses per tick per client. The delta encoding
  saves bandwidth but not CPU or allocation.
- **`crates/crcbl-net/src/delta.rs:384` — change detection compares 64-bit
  `DefaultHasher` digests instead of bytes** (correctness/perf). With both
  buffers in hand, a collision silently drops a real update (client keeps stale
  component data with no error path), and hashing both sides is slower than
  `data != baseline_data` for small blobs.
- **`crates/crcbl-net/src/condition.rs:187 — `reorder_window`is honoured as a
  boolean** (correctness/YAGNI). Documented as "shuffle up to this many
  consecutive ready messages", but the code tests`window > 1`and shuffles the
  entire`ready` vector, so reorder depth is not configurable and scripted tests
  do not reproduce the window they claim.
- **`crates/crcbl-server/src/lib.rs:283` — production `assert!` on a duplicated
  invariant** (correctness). `assert!(self.session.try_reconnect(...))` is
  guarded by a preceding `can_reconnect`; the two functions re-implement the
  same three checks independently. They agree today; any divergence turns a
  reconnect race into a server panic.

### Low

- `crates/crcbl-net/src/condition.rs:284` — `deterministic_shuffle` takes
  `(*seed as usize) % (i+1)` from an LCG's _low_ bits, whose period is 2^(k+1);
  for small `i` the shuffle degenerates into a near-fixed pattern, biasing
  reorder tests.
- `crates/crcbl-net/src/condition.rs:152` — `Duration::from_secs_f64(total)`
  panics on overflow/NaN; reachable with a very large `latency + jitter` config.
- `crates/crcbl-net/src/condition.rs:209` — `send_reliable` and
  `send_unreliable` bodies are byte-identical except the final call (DRY).
- `crates/crcbl-net/src/session.rs:177` — `now + config.reconnect_grace_period`
  can overflow-panic on a `Duration` near `u64::MAX`; server-supplied clock, so
  not attacker-reachable today.
- `crates/crcbl-net/src/delta.rs:268,273` — `BaselineStore::get` is a linear
  scan, and `is_too_old` assumes the ring is tick-sorted while `insert` never
  enforces that ordering.
- `crates/crcbl-net/src/transport.rs:186` — `recv()` drains the _unreliable_
  queue first, so only the explicit `recv_reliable` prioritises control traffic;
  the trait's default impl (`:66`) silently returns `Ok(None)`, so a backend
  author who forgets to override it loses all control traffic with no error.
- `crates/crcbl-net/src/codec.rs:41`, `delta.rs:766` — `payload.len() as u32`
  truncates in error values on >4 GiB slices (cosmetic; both gated by a length
  check).
- `crates/crcbl-server/src/lib.rs:67,100`, `crcbl-client/src/lib.rs:109,123` —
  `if !cfg!(test)` around `assert_explicit()` puts a test-only carve-out in
  shipped library code; a downstream crate's tests get the weakened check too.
- `crates/crcbl-server/tests/integration.rs:15` — tests pin
  `protocol_version: 3` while `ProtocolCompatibility::DEFAULT` is 4, so the
  shipped default version is never exercised end-to-end.
- `crates/crcbl-net/src/condition.rs` tests — `thread::sleep`-based timing
  assertions (`:465`, `:530`, `:569`, `:692`, `:739`, `:768`) are wall-clock
  dependent and will flake on loaded CI.

### DRY

- **`crates/crcbl-net/src/delta.rs:771` (high-value cleanup)** —
  `decode_delta_inner` hand-rolls bounds-checked LE reads ~20 times
  (`if cursor + N > payload.len()` / `try_into().unwrap()` / `cursor += N`)
  while `codec.rs:55` already has a tested `ByteReader` doing exactly that.
  Every hand-rolled site is an independent chance to get a bound wrong.
- The `(entity_bits: u64, len: u32, bytes)` framing is parsed by four separate
  implementations (`delta.rs:163`, `delta.rs:884`, `client/src/lib.rs:44`,
  `client/src/lib.rs:545`) and written by three more.
- Three trusted/untrusted twin pairs with near-identical bodies:
  `Baseline::from_snapshot`/`from_trusted_snapshot`,
  `DeltaCodec::apply`/`apply_trusted`, `decode_delta`/`decode_trusted_delta` —
  all differing only in one system-count constant.
- `crates/crcbl-server/src/lib.rs:216` — inbound dispatch is trial-decode
  (`decode_hello`, else `decode_ack`, else `decode_client_to_server`) rather
  than a switch on the tag byte the wire format already carries.

### YAGNI

- `crates/crcbl-server/src/sim_hash.rs:18` — `SimHash` is never constructed or
  referenced anywhere in the workspace.
- `crates/crcbl-net/src/delta.rs:27` — `MAX_TRUSTED_DELTA_SYSTEMS` (4096) can
  never bind: the payload-derived check at `:825` caps system count at 4093
  first.
- `crates/crcbl-net/src/delta.rs:82` — `Baseline::from_snapshot` (the
  limit-enforcing constructor) has exactly one caller, a test; server and client
  both use `from_trusted_snapshot`.
- `crates/crcbl-net/src/codec.rs:46` — `validate_field_len(len, _remaining)`
  never reads its second parameter.
- `crates/crcbl-net/src/messages.rs` — the whole
  `ServerToClient`/`ClientToServer` snapshot codec (tags 0x00/0x01/0x10/0x11)
  plus `SnapshotReader` are exercised only by unit tests and the fuzz target;
  the live wire path is delta + ack + handshake. `Event` and `Command` are never
  sent, and the server decodes `Input` only to discard it.
- `crates/crcbl-net/src/transport.rs:10` — `Message.kind` duplicates the
  `send_reliable`/`send_unreliable` choice and is never validated against it,
  while `ConditionSimulator` re-routes on `kind`, so a mismatched field silently
  switches channels.
- `crates/crcbl-net/src/session.rs:205` — `try_reconnect`'s `_config` parameter
  is unused; `delta.rs:119,126` — `#[allow(dead_code)]` on
  `system_count`/`entity_count` is stale.

**Notes.** Decoders are genuinely hardened: every
`Vec::with_capacity`/`HashSet::with_capacity` in `codec.rs` and `delta.rs` is
bounded by a prior `count ≤ remaining/min_item_size` check, all multiplications
use `checked_*`, all slice reads are length-gated, and no reachable panic or
unbounded allocation was found on any of the seven decode entry points. Resume
tokens are 32 bytes from `getrandom::fill` with a non-short-circuiting
comparison and redacted `Debug`, rotated on every successful reconnect. The gap
is one layer up: nothing authenticates a message, so acks, rejects and the
"trusted" flag are all forgeable, and there is no keyframe-recovery watchdog on
either side.

## crcbl-phys + crcbl-ecs

Findings 1, 3–8 and 17 were reproduced with probe binaries linked against the
built crates.

### Critical

- **`crates/crcbl-phys/src/world.rs:322` (with `broadphase.rs:392`) — BVH refit
  uses the pre-sort element index; colliders silently disappear from all
  queries** (correctness) **[probe]**. `rebuild()` fills `bvh_slot_to_elem` with
  the position in the _unsorted_ `elements` vec, but `Bvh::build_rec` sorts each
  range by centroid before pushing leaves, so `element_node`/`element_indices`
  are in _post-sort_ leaf order. `set_sphere/set_box/set_capsule` therefore
  refit the wrong leaf whenever ≥2 colliders are not already centroid-ordered.
  Two spheres at x=20 (slot 0) and x=5 (slot 1); `cast_ray(+X)` hits slot 1 at
  t=4.0; after `set_sphere(slot0, x=-50)` the same ray returns `None` — the
  untouched sphere became invisible. `PhysicsSystem::step` calls this path for
  every body every substep.

### High

- **`crates/crcbl-phys/src/system.rs:402` — `SystemTrait::tick` hardcodes
  `step(1.0/120.0)` regardless of the schedule's tick rate** (correctness).
  `Server::tick → World::tick → Schedule::run → PhysicsSystem::tick`; breakout
  constructs the server with `tick_hz: 60` (and `30` elsewhere) and never calls
  `step` itself, so physics advances 0.5 s per wall second at 60 Hz and 0.25 s
  at 30 Hz. Simulated gameplay speed is a function of the configured tick rate.
- **`crates/crcbl-phys/src/query.rs:411` — `swept_sphere_vs_sphere` returns
  `None` when the sweep both starts and ends overlapping** (correctness)
  **[probe]**. The root selection accepts only `t1` or `t2` inside `[0,1]`; a
  deep overlap gives `t1<0` and `t2>1`, so both are rejected. Target sphere r=5
  at origin, sweep from `(0,0,0)` to `(0.1,0,0)` with r=0.5 → `None`. A body
  resting inside another reports no contact at all.
- **`crates/crcbl-phys/src/query.rs:411` — when a sweep starts overlapping, the
  reported TOI is the _exit_ time, not 0** (correctness) **[probe]**. Start
  `(0.5,0,0)`, target r=1 at origin, swept r=0.5 → `t=0.222` (the exit at x=1.5)
  with `started_inside=false`. Depenetration is applied after the sphere has
  already passed through. `swept_sphere_vs_aabb` clamps with `tmin.max(0.0)`
  instead — the two shape paths disagree.
- **`crates/crcbl-phys/src/query.rs:323` — `ray_vs_capsule` returns the far-side
  cylinder exit for rays starting radially inside the infinite cylinder but
  outside the capsule** (correctness) **[probe]**. `ray_vs_capsule_cylinder`
  falls back to `t1.max(t2)` when the near root is negative and, if that exit
  lies in the Y band, `ray_vs_capsule` returns it before testing the caps.
  Capsule r=1 hh=1, ray from `(0.9, 5, 0)` aimed almost straight down returns
  `t=5.001` at `(1.0, ~0, 0)` — a backface on the side wall — when the true
  first hit is the top hemisphere at t≈4.

### Medium

- **`crates/crcbl-phys/src/query.rs:539,563` — `swept_sphere_vs_aabb` returns a
  fabricated `+Z` normal and `started_inside=false` when the sweep starts inside
  the inflated box** (correctness) **[probe]**. With `t` clamped to 0 the hit
  point is the segment start, matching no inflated face within the 1e-9 eps, so
  the `else` arm yields `DVec3::Z`. Push-out goes along an arbitrary axis.
- **`crates/crcbl-phys/src/query.rs:352` — `started_inside` is hardcoded `false`
  on every capsule cylinder hit** (correctness) **[probe]**.
  `ray_vs_sphere`/`ray_vs_aabb` both report `true` for the same situation, so
  callers using the flag to reject self-hits get the wrong answer for capsules
  only.
- **`crates/crcbl-phys/src/query.rs:589` — stationary `swept_sphere_vs_capsule`
  derives the normal from `capsule.centre` rather than the closest point on the
  capsule segment** (correctness) **[probe]**. Capsule hh=5 at origin, sphere at
  `(1.2, 5, 0)` → normal `(0.233, 0.972, 0)` where the correct lateral normal is
  `(1,0,0)`. A character overlapping a tall capsule is pushed along its axis
  instead of sideways.
- **`crates/crcbl-phys/src/world.rs:126` — the `is_trigger` flag is stored and
  settable but no query reads it** (correctness/YAGNI). `cast_ray`,
  `sweep_sphere`, `overlap_sphere` and `overlap_aabb` all treat triggers as
  solid, so a "non-solid, overlap-only" volume (documented at
  `components.rs:240`) blocks bullets and stops sweeps.
- **`crates/crcbl-phys/src/system.rs:171` — `step` drives force application and
  integration in `HashMap<Entity, RigidBody>` key order** (determinism).
  `RandomState` is seeded per process, so the order differs every run.
  Numerically harmless today only because every force provider is stateless and
  per-body; any pairwise or accumulating provider breaks the crate's determinism
  claim silently.
- **`crates/crcbl-ecs/src/system.rs:257` — `System<T>::hash_state` iterates
  `data` in swap-remove storage order rather than a canonical entity order**
  (determinism). Two worlds with identical logical state hash differently if
  their attach/detach histories differed. `PhysicsSystem::replicate` sorts by
  `entity.to_bits()` for exactly this reason; the hash path does not.
- **`crates/crcbl-phys/src/system.rs:394` — `PhysicsSystem` never overrides
  `hash_state`/`contributes_to_hash`, so the determinism harness excludes all
  physics state** (determinism). `crcbl-server/src/sim_hash.rs:35` calls
  `world.hash_state`, which for this system is the default no-op, while
  `apps/breakout/src/game.rs:260` registers `PhysicsSystem`. Every position,
  velocity and collider in the flagship sample is outside the "provably
  deterministic" hash.
- **`crates/crcbl-ecs/src/component_hash.rs:42` — `usize`/`isize` hash their
  platform-width `to_le_bytes()`** (determinism). 8 bytes on x86-64, 4 on
  wasm32, so the same component state hashes differently across the two bindings
  the `GameModule` docs claim to compare.
- **`crates/crcbl-phys/src/world.rs:147,292` — `ColliderId` is a bare slot index
  with no generation, and `remove` recycles slots** (correctness). A
  `ColliderId` retained across a remove+add silently addresses the new occupant,
  so `set_sphere`/`set_trigger`/`remove` mutate an unrelated collider. `Entity`
  gets this right through `crcbl_core::Handle`; `ColliderId` (exposed via
  `world()`/`world_mut()`) does not.
- **`crates/crcbl-phys/src/world.rs:323` — the `bool` from `bvh.update_aabb` is
  discarded** (correctness). When the refit fails the BVH is left stale instead
  of being marked dirty (`self.bvh = None`), so subsequent queries run against
  wrong bounds rather than rebuilding.
- **`crates/crcbl-phys/src/components.rs:188` — `Transform::decode` validates
  only the byte length** (security). Wire data decodes straight into
  `position`/`rotation` with no finiteness or unit-quaternion check, so a peer
  can inject NaN/inf positions or a degenerate quaternion that propagates
  through `forward()/right()/up()`, AABBs and the BVH.
- **`crates/crcbl-phys/src/query.rs:101,189` — `ray_vs_sphere`/`ray_vs_aabb`
  reject the whole shape when the near root falls before `t_min`, even if the
  far root lies inside `[t_min, t_max]`** (correctness) **[probe]**. Sphere r=1
  at origin, ray from x=-5 with `with_bounds(4.5, 100.0)` → `None`; same for the
  AABB. Any caller that advances `t_min` to skip a start offset loses hits on
  shapes it is already inside.

### Low

- **`crates/crcbl-ecs/src/schedule.rs:82`** — `hasher.write(name.as_bytes())`
  has no length delimiter, so the system-name/component-data boundary is
  ambiguous; system `"ab"` with no data hashes identically to system `"a"` whose
  first component byte is `b'b'` (determinism).
- **`crates/crcbl-ecs/src/schedule.rs:80`** — `sort_by_key` on non-unique system
  names; duplicates fall back to insertion order, so registration order leaks
  into the "order-independent" hash (determinism).
- **`crates/crcbl-ecs/src/system.rs:152`** — `attach` accepts an entity that has
  already been swept; the data is never removed, permanently inflating
  `entity_count` and `hash_state`.
- **`crates/crcbl-phys/src/query.rs:246`** — the ray/segment parallelism test
  compares `denom.abs()` against the absolute `f64::EPSILON`; the threshold is
  scale-dependent.
- **`crates/crcbl-phys/src/query.rs:196,538`** — face classification uses an
  absolute `1e-9` eps against `point`. Probes at 1e6…1e11 with axis-aligned rays
  still classified correctly, so _unverified_; a fuzz over oblique rays at large
  world coordinates would confirm or refute.
- **`crates/crcbl-phys/src/collider.rs:106`** — `is_empty` doc says "zero or
  negative extent" but the code tests `min > max` only; a zero-thickness AABB is
  non-empty yet has `surface_area() == 0` (style).
- **`crates/crcbl-phys/src/broadphase.rs:112`** — doc claims "post-order
  layout"; `build_rec` reserves the parent slot before recursing, i.e. pre-order
  (style).
- **`crates/crcbl-phys/src/components.rs:164`** — `encoded_len(&self)` ignores
  `self` and returns a constant (style).
- **`crates/crcbl-phys/src/lib.rs:50`** — `pub fn test_entity` with an `expect`
  ships in the production API surface (YAGNI).

### DRY

- The zero-direction-safe slab test is written four times: `collider.rs:181`,
  `broadphase.rs:425`, `query.rs:147`, `query.rs:496` — each with its own copy
  of the `is_finite` guard.
- Point→face-normal classification duplicated at `query.rs:197` and
  `query.rs:539`.
- The quadratic `b²-4ac` solver appears three times (`query.rs:85`, `:313`,
  `:400`) — **and the three copies disagree on root selection, which is the
  direct cause of the three sweep/capsule findings above.**
- `world.rs:340` `closest_hit` and `world.rs:362` `closest_swept` are the same
  loop with a different three-arm match.
- `system.rs:285` (`add_collider_to_world`) and `system.rs:343`
  (`sync_collider_from_cached`) repeat the same three-arm destructure; the
  slot→entity lookup is copy-pasted at `:207,220,234,256`.
- `test_entity` exists three times: `crcbl-ecs/src/lib.rs:66`,
  `crcbl-phys/src/lib.rs:50`, `crcbl-phys/src/system.rs:448`.

### YAGNI / perf

- `Aabb::surface_area`, `from_points`, `contains`, `half_extents`
  (`collider.rs:52,80,94,163`) have no non-test callers — SAH splitting was
  never implemented (the BVH uses a count median).
- `PhysicsWorld::generation()` (`world.rs:281`) is read only by its own unit
  test.
- `Integrator` (`integrator.rs:20`) is a single-impl boxed trait and
  `set_integrator` (`system.rs:121`) is never called — one `dyn` dispatch per
  body per substep for no variation.
- The debug-draw path is inert: `DebugCtx` is an empty struct,
  `Schedule::debug_draw` is never called by `World::tick` or any app,
  `System::set_debug_draw`/`DebugDrawFn` have no non-test users.
- `System::iter_entities_mut` (`ecs/system.rs:214`) is unused.
- `PhysicsSystem::overlap_sphere` (`system.rs:229`) fabricates a content-free
  `ShapeHit` for every result.
- `Node::leaf_count` is always 1 and `child_right` is unused in leaves, so every
  leaf loop iterates a one-element range.
- `Bvh::traverse_ray` (`broadphase.rs:314`) collects _all_ intersected leaves
  with no `t`-based pruning and allocates a `Vec` per query; `BvhHit.point` is
  computed for every leaf and ignored by both callers.
  `PhysicsWorld::len()`/`is_empty()` are O(n) scans and `is_empty` calls `len`.

**Notes.** No `unsafe` blocks exist in either crate, and entity ABA is correctly
prevented by `crcbl_core::Handle`'s generation counter — the analogous
`ColliderId` is not.

## crcbl-core + crcbl-store + crcbl-audio

Every finding above Low was reproduced with a probe binary linked against the
real crates.

### Critical

- **`crates/crcbl-audio/src/mixer.rs:138` — `unsafe impl Send/Sync for Mixer` is
  unsound; safe code causes a data race** (unsafe-soundness) **[probe]**. The
  safety comment asserts the `UnsafeCell` is only accessed from a single audio
  thread, but `fill(&self)` is a safe public method on a `Sync` type.
  `Arc<Mixer>` shared across two threads calling `fill` gives two live
  `&mut Vec<Voice>` from `&*self.voices.get()`, and `retain_mut`
  reallocates/drops `Vec<f32>` buffers under the other thread.
  `voice_count(&self)` races the same way. `assert_send_sync::<Mixer>()`
  compiles — nothing in the API enforces the claimed contract.
- **`crates/crcbl-store/src/save.rs:237` — a 62-byte save file aborts the
  process** (security) **[probe]**. `sector_count` is a `u32` read straight from
  the file (`:234`) and fed to `Vec::with_capacity` before any per-entry length
  is validated. A 62-byte file with `sector_count = 0xFFFFFFFF` produced
  `memory allocation of 206158430160 bytes failed` → `handle_alloc_error` →
  abort + core dump. Not a catchable panic; a corrupt or hostile autosave takes
  the process down with no recovery path.

### High

- **`crates/crcbl-store/src/replay.rs:185` — a 30-byte replay file panics
  `FileTransport::open`** (security) **[probe]**. `tick_count` is a `u64` cast
  to `usize` and passed to `Vec::with_capacity`; `tick_count = u64::MAX` on a
  30-byte file → `panicked at raw_vec/mod.rs: capacity overflow`. `.crpl` files
  are exactly the artifact a user shares for bug reports.
- **`crates/crcbl-audio/src/qoa.rs:164` — an 8-byte QOA file reserves 17 GB**
  (security) **[probe]**. `total_samples` (u32, `:153`) drives
  `Vec::with_capacity(total)` before a single frame header is read. Survived
  only because Linux overcommit made it a lazy virtual reservation; under a
  cgroup limit, `vm.overcommit_memory=2`, or wasm32 it aborts.
- **`crates/crcbl-audio/src/qoa.rs:317` — the LMS weight update is inverted vs.
  the QOA spec; all non-silent audio decodes wrong** (correctness) **[probe]**.
  Reference `qoa.h` does `weights[i] += (history[i] < 0 ? -delta : delta)` —
  sign from history, magnitude from delta. This code does the opposite. Against
  a reference implementation, one mono slice with non-zero LMS state:

  ```text
  crcbl : [-695,  70, -890, 445, -873, 652, -149, -163, 136,  79]
  qoa.h : [-695,  81, -910, 449, -878, 453,  178, -158, -144, 140]
  ```

  Sample 0 matches (weights unused yet); everything after diverges. The non-spec
  weight clamp at `:322` exists only to stop the wrong formula from exploding.
  Every unit test uses all-zero silent files, where both formulas produce 0.

- **`crates/crcbl-store/src/lib.rs:151` — `NativeStorage::resolve` allows
  traversal out of the storage root** (security) **[probe]**.
  `self.root.join(path)` neither rejects `..` nor absolute paths (Rust's `join`
  discards the root entirely for an absolute argument).
  `store.write("../crcbl_probe_ESCAPED.txt", …)` → `Ok`, file created outside
  the root; `store.write("/tmp/crcbl_probe_ABSOLUTE.txt", …)` → `Ok`. `delete`
  (`:170`) is worse: it calls `remove_dir_all` on whatever the traversal
  resolves to. Any save-slot or profile name reaching this from user input is an
  arbitrary write/delete.
- **`crates/crcbl-audio/src/mixer.rs:104` — looping voice over an empty buffer
  divides by zero** (correctness) **[probe]**.
  `pos = ((pos as usize) % data_len)` with `data_len == 0`:
  `Voice::new(Vec::new()).with_looping()` → `panicked at mixer.rs:104`. This
  runs inside the audio callback, so it kills the audio thread.
  `SoundBank::create_voice` will hand out a voice over an empty entry.
- **`crates/crcbl-store/src/settings.rs:320` — hostile `settings.toml` panics on
  the next `set()`** (security) **[probe]**. `set_dotted` does
  `.as_table_mut().expect("entry must be a table after insert")`; if a key on
  the dotted path already exists as a non-table in the user layer,
  `or_insert_with` returns the existing scalar and the `expect` fires.
  `settings.toml` containing `engine = "not a table"`, then
  `stack.set("engine.video.vsync", &true)` → panic. A user hand-editing their
  settings file crashes the game on the next options-menu write.
- **`crates/crcbl-audio/src/spatial.rs:179` — pan/ILD are step functions at dead
  centre, not continuous cues** (correctness) **[probe]**. The
  `pan.abs() < f32::EPSILON` fast path returns `(1.0, 1.0)` while constant-power
  panning yields `(0.707, 0.707)` at centre, and the ILD at `:190` applies the
  full 6 dB to the far ear the instant `pan` crosses zero:

  ```text
  x=0        L=1.00000 R=1.00000
  x=0.000001 L=1.00000 R=1.00000
  x=0.0001   L=0.35439 R=0.70711
  ```

  A 0.1 mm lateral move drops the left channel by 9 dB and the right by 3 dB —
  an audible click. The module docs promise "Continuous: cues interpolate
  smoothly across the sphere."

- **`crates/crcbl-audio/src/mixer.rs:109` — pitch shifting destroys L/R
  interleaving** (correctness) **[probe]**. `pos` advances by `pitch` _samples_
  through interleaved stereo data while the gain is chosen from the _output_
  index `i % CHANNELS`; the two parities decouple for any `pitch != 1.0`. With
  data `L=+1.0, R=-1.0` and `pitch = 2.0` the output was `[1.0 × 8]` — the right
  channel was never read. Correct varispeed must step by `pitch` _frames_ and
  read both channels.
- **`crates/crcbl-store/src/settings.rs:267` — `dump()` reports the opposite
  priority to `get()`** (correctness) **[probe]**. `merge_all` walks layers
  lowest-priority-first while `deep_merge` is first-write-wins, so engine
  defaults win and user/CLI layers are skipped. Engine default `volume = 100`,
  CLI override `volume = 50` → `get() = Some(50)`, `dump() = "volume = 100"`.
  The comment at `:265` states the reverse of what the code does, and
  `crcbl settings list` shows values the engine is not using.
- **`crates/crcbl-audio/src/qoa.rs:167` — multi-frame multi-channel QOA is
  silently truncated** (correctness). The header's `total_samples` is
  per-channel, but the loop compares it against `samples.len()`, the
  _interleaved_ count, and `channel_samples` at `:209` mixes the same two units.
  For a stereo file of 10 000 per-channel samples the decoder stops after the
  first 5 120-sample frame and returns half the audio.

### Medium

- **`crates/crcbl-store/src/crash_ring.rs:77` — the `unsafe` block's contract is
  unenforced, and the type cannot do what its docs claim** (unsafe-soundness).
  `push(&self)` mints `&mut *self.slots.get()` under a "single-producer
  contract" no API enforces, and `snapshot()` (`:113`) clones entries while the
  producer may be overwriting them — a documented, deliberate read/write race.
  It is unreachable only because `UnsafeCell` makes `CrashRing` `!Sync`, which
  means the module doc's "lock-free… safe to push from any thread and read from
  a panic handler" is impossible and the whole `UnsafeCell` + `AtomicUsize`
  apparatus is dead weight over a `Mutex`. Adding the `unsafe impl Sync` the
  docs imply makes it instantly unsound.
- **`crates/crcbl-audio/src/wav.rs:92` — RIFF word-alignment padding is not
  skipped; valid WAVs are rejected** (correctness) **[probe]**. Chunks with an
  odd `size` are followed by a pad byte per the RIFF spec; the loop advances
  `pos += size` only, desynchronising every subsequent chunk. A WAV with an
  odd-length `LIST` chunk (very common — INFO/artist strings) →
  `Err(Truncated)`.
- **`crates/crcbl-audio/src/lib.rs:161` — the additive-mixing contract is
  violated on the primary output path** (correctness). `AudioSource` docs
  (`:48`) promise the buffer is "already zeroed by the stream before `fill` is
  called" and `Mixer::fill` accumulates with `+=`, but only the `channels > 2`
  branch calls `data.fill(0.0)`. cpal does not guarantee zeroed buffers, so the
  mixer adds into stale data on the common stereo path.
- **`crates/crcbl-audio/src/wav.rs:146` + `mixer.rs:51` — NaN/Inf from a float
  WAV reach the output, and `f32::clamp` does not stop them** (correctness)
  **[probe]**. A `format = 3` WAV carrying `NaN`/`+inf` decoded to
  `Ok([NaN, inf])`. `with_volume/with_gains/with_pitch` use `f32::clamp`, which
  returns NaN for NaN. A voice with NaN volume and pitch produced `buf[0] = NaN`
  and, because `pos += NaN` never reaches `data_len`, `mix_block` returns `true`
  forever — `voice_count` stayed at 1 after six fills. One malformed asset
  poisons the mix and leaks an immortal voice.
- **`crates/crcbl-audio/src/mixer.rs:115` — no clipping or limiting anywhere in
  the chain** (correctness). N simultaneous voices sum past ±1.0 and go to the
  device unclamped; nothing between `Mixer::fill` and cpal bounds the signal.
- **`crates/crcbl-store/src/save.rs:148` — the "SHA-256" checksum is
  `DefaultHasher`, whose output is not stable across Rust releases**
  (correctness). The format doc (`:14`) and the function doc both say SHA-256;
  the body uses `std::collections::hash_map::DefaultHasher`, which std documents
  as unspecified. Every existing save file fails its checksum after a toolchain
  upgrade, and the 32-byte field carries only 64 bits of digest.
- **`crates/crcbl-store/src/save.rs:277` — `checksum_valid` is advisory;
  `open()` returns `Ok` on a corrupt file** (correctness). The module doc claims
  "a corrupted or truncated file is detected on open", but a failing checksum
  only sets a bool and parsing continues over the corrupt body — including the
  `sector_count` allocation above.
- **`crates/crcbl-store/src/lib.rs:184` — `list()` returns different things
  depending on the backend** (correctness) **[probe]**.
  `MemoryStorage::list("dir") = ["dir/a.txt"]`,
  `NativeStorage::list("dir") = ["a.txt"]`, against a trait doc promising "their
  full (relative) paths within the storage root". Code that round-trips a
  listing entry back into `read()` works against one impl and silently fails
  against the other.
- **`crates/crcbl-store/src/lib.rs:225` — `write_atomic` follows symlinks at a
  guessable temp path and leaks temp files on failure** (security).
  `File::create(&tmp_name)` follows an existing symlink, and `rand_suffix`
  (`:247`) is a Lehmer step over `time_nanos + pid` — predictable enough for a
  pre-planted symlink in a shared storage root (dedicated-server deployments use
  `NativeStorage::at`). Any failure between `:225` and the `rename` leaves
  `.stem.hex.ext.tmp` behind forever.
- **`crates/crcbl-store/src/lib.rs:57` — `io::Error` messages are stuffed into a
  `PathBuf`** (correctness).
  `StorageError::NotFound(PathBuf::from(e.to_string()))` produces a "path"
  reading `No such file or directory (os error 2)`; every
  `NotFound`/`PermissionDenied` loses the real path and gains a fake one.
- **`crates/crcbl-store/src/save.rs:255`, `replay.rs:203`, `wav.rs:95` —
  `cursor + len` overflows `usize` on 32-bit** (security). All three bounds
  checks add an attacker-supplied `u32`-derived length to a cursor; on wasm32
  that wraps in release, the check passes, and the following slice index panics.
- **`crates/crcbl-audio/src/lib.rs:125` — `mem::forget(stream)` leaks the cpal
  stream and the source on every `open()`** (correctness). Dropping
  `AudioStream` only makes the callback skip `fill`; the OS stream stays open
  and the leaked `Stream` + `Arc<Source>` are never freed. Once the callback
  stops writing it also stops zeroing, so cpal outputs whatever was last in the
  buffer.
- **`crates/crcbl-store/src/save.rs:349` — `AutosaveRing::list` does not return
  oldest-to-newest** (correctness). It iterates slot indices `0..capacity`;
  after any wrap the oldest surviving file is at index `self.slot`. "Load most
  recent autosave" built on the doc contract picks the wrong file; the test only
  asserts `len() == 3`.

### Low / DRY / YAGNI

- **`crates/crcbl-store/src/crash_ring.rs:140`** (DRY) — `dump` re-hardcodes
  `b"CRBLREPL"`, `1u16` and the full entry layout instead of using
  `replay::REPLAY_MAGIC`/`REPLAY_FORMAT_VERSION` or `ReplayWriter`, so a format
  bump leaves crash dumps silently emitting version 1. The module doc also
  advertises `install_panic_hook`, which does not exist.
- **`crates/crcbl-store/src/settings.rs:192,208`** (DRY) — `get` and
  `get_section` are byte-identical. Both also use `let table = layer.table()?`
  _inside_ the loop, so a layer variant returning `None` would abort the whole
  search instead of skipping that layer.
- **`crates/crcbl-store/src/settings.rs:285,308`** (YAGNI) —
  `if parts.is_empty()` is unreachable; `"".split('.')` yields one element.
- **`crates/crcbl-audio/src/wav.rs:139`** (YAGNI) —
  `total_samples * bytes_per_sample > data.len()` can never be true after the
  preceding division, so `DataTruncated` is unreachable; the `total` parameter
  threaded into the four decoders is redundant with `chunks_exact`.
- **`crates/crcbl-audio/src/spatial.rs:77`** (YAGNI) — `SpatialCue::itd_samples`
  is computed by the headline cue rule and consumed by nothing: `Voice` has no
  delay line. Same for `AudioEvent::range` (`event.rs:27`).
- **`crates/crcbl-store/src/replay.rs:182`** (YAGNI) — `_start_tick` is parsed
  and discarded; recorded `tick_id`s are also discarded by `recv()` (`:271`), so
  playback ignores both the tick timeline and `tick_rate`. The format has no
  checksum, unlike saves.
- **`crates/crcbl-audio/src/mixer.rs:83`** (YAGNI) — `Voice::is_finished` is
  never called. **`qoa.rs:41`** — `SampleCountOverflow` is unreachable.
- **`crates/crcbl-store/src/save.rs:132,140`** — `self.sectors.len() as u32` and
  `snapshot_data.len() as u32` truncate silently; a >4 GiB sector snapshot
  writes a corrupt length. **`:332`** — `AutosaveRing` never checks that
  `template` contains `{}`, so a template without it makes every slot write to
  the same path.
- **`crates/crcbl-core/src/time.rs:331`** (style) — `FrameClock::new(tick_hz)`
  documents only `tick_hz == 0` as a panic, but `tick_hz > 1_000_000_000` makes
  the period 0 and panics in `with_period` with the wrong message.
- **`crates/crcbl-core/src/log.rs:219`** (style) — `is_installed()` returns
  `true` whenever `LOGGER` was initialised, including when `set_logger` lost the
  race to a host application.
- **`crates/crcbl-store/src/settings.rs:106`** (style) —
  `String::from_utf8_lossy` silently mangles a non-UTF-8 settings file into
  replacement chars instead of reporting it.
- **`crates/crcbl-audio/src/lib.rs:170`** (style) — for `channels > 2`, only
  front L/R are written and channels 2..n are left silent with no diagnostic.
- **`crates/crcbl-audio/tests/orbit.rs:191`** (style) —
  `orbit_integration_deterministic` XORs per-block hashes, which is
  order-insensitive; `orbit_cue_changes_over_time` only asserts _some_ hash
  differs. **Fixed since.** The test feeds one hasher in block order and asserts
  that the reversed event order hashes differently, with the XOR fold named in a
  comment as what it replaced. It now lives in
  `crates/crcbl-audio/tests/spatial_chain.rs` as
  `an_orbit_hashes_the_same_twice_and_differently_in_reverse`.

### Clean — crcbl-core

`crcbl-core` produced no finding above style:

- **`handle.rs`** — generation exhaustion retires the slot rather than wrapping
  (no ABA revival); `remove` moves through `Slot::Retired` so a panic mid-swap
  cannot leave a resurrectable slot; `clear` rebuilds the free list from scratch
  and bumps only occupied generations; index truncation is guarded by
  `u32::try_from` at 2^32 slots. No `unsafe`.
- **`alloc.rs`** — the `mut_from_ref` argument holds: `base` is taken exactly
  once, `_storage` is never reborrowed, `reset`/`reset_peak` take `&mut self`,
  `reserve` uses `checked_mul`/`checked_add` and validates alignment ≤
  `BLOCK_ALIGN` before any pointer arithmetic, and ZST/empty allocations return
  a dangling-but-aligned pointer. Correctly `!Send`/`!Sync`.
- **`time.rs`** — the spiral-of-death cap drops whole ticks only and preserves
  the sub-tick remainder; `pending_ticks`' `as u32` is bounded by
  `max_catch_up_ticks`; backwards timestamps rebase rather than wrap;
  `TickId::next` saturates.
- **`world.rs`** — `normalize_axis` carries through `i128` and saturates at the
  `i64` edge; `f64 as i128` saturation and NaN→0 behaviour is relied on
  deliberately and tested; `delta_axis` takes the sector difference in `i128`.
- **`input.rs`, `surface.rs`** — vocabulary only. Note that the edge-tracking
  input state machine lives in `crcbl-input`, not here; `ButtonState` is a
  two-variant enum with no edge tracking.

**Notes.** The severity gradient is stark: `crcbl-core` is carefully reasoned
and property-tested, while `crcbl-store` and `crcbl-audio` parse untrusted bytes
with unvalidated declared lengths (three separate allocation bombs, one of which
aborts from 62 bytes), hand out unenforced `unsafe` contracts, and carry
happy-path-only test suites — which is exactly what hides the QOA LMS and
pitch-interleaving bugs.

## crcbl-ui + crcbl-input + crcbl-shaders

Findings marked **[probe]** were reproduced in a scratch crate.

### High

- **`crates/crcbl-input/src/lib.rs:321` — `begin_tick` re-resolves _disabled_
  actions, so a disabled action fires again one tick later** (correctness)
  **[probe]**. `begin_tick` loops every slot and calls `resolve_one(i)` with no
  `enabled` check, and `resolve_one` does not check either. Disabling "jump"
  while Space is held clears it correctly, then the next `begin_tick` re-reads
  `held_keys` and emits `state: Pressed, just_pressed: true` — a menu opened
  with gameplay disabled still jumps. Confirmed for both Button and Axis2
  (`disabled_wasd_returns_zero` misses it only because it never ticks).
- **`crates/crcbl-ui/src/text.rs:154` — `layout_line` scales the absolute
  cursor, so advance and line height scale _twice_ and the text anchor moves
  with font size** (correctness) **[probe]**. `cursor` already contains `pos`
  and the per-glyph `advance * scale`; multiplying the resulting `min`/`max` by
  `scale` again yields `x = pos.x*scale + i*advance*scale²`.
  `layout_line("AB", (100,200), 2.0)` → first glyph at `(202, 380)` with a 40px
  advance instead of 20. A 28px button label at `(50,50)` spans x `50..330`
  against a declared rect of `(50,50)..(166,90)`.
- **`crates/crcbl-ui/src/text.rs:118,184,196` — `c as u8` truncates the
  codepoint, silently aliasing non-ASCII characters onto the wrong ASCII glyph**
  (correctness) **[probe]**. `(c as u8).wrapping_sub(FIRST_CHAR)` keeps only the
  low byte: `'Ł'` (U+0141) renders the `'A'` glyph, `'ġ'` (U+0121) renders
  `'!'`. Others (`'é'`, `'€'`) land out of range and return `u_min > 1.0` from
  the public `glyph_u_min`/`glyph_u_max`. No missing-glyph (`.notdef`) path
  exists anywhere.
- **`crates/crcbl-ui/src/draw_list.rs:267` + `:162` vs `shaders/ui.slang:62` —
  the atlas V coordinate is assigned against the opposite Y convention from the
  shader that consumes it** (correctness). `to_triangles` documents "Y-up (the
  UI convention)" and gives `v_min = 0.0` (atlas _top_ row) to the vertex at
  `max.y`, while `ui.slang`/`ui.wgsl` compute
  `ndc.y = 1.0 - (y/viewport.y)*2.0`, i.e. screen y=0 is the top. So the atlas's
  top row maps to the quad's visually-lower edge and glyphs render vertically
  mirrored. The mismatch between the two files is confirmed by reading both; the
  mirrored pixels are _unverified_ (no golden image covers the UI pass — a
  rendered `UiPass` capture would confirm).

### Medium

- **`crates/crcbl-ui/src/widget.rs:87,101,157,199` vs `draw_list.rs:263` —
  measurement divides by 14, layout divides by `GLYPH_HEIGHT` (13), so every
  button bound and hit rect is ~7% too small** (correctness) **[probe]**.
  `Button::new("LongButtonLabel")` reports `max.x = 166.0` while its glyph quads
  reach `x = 180.7`. Text overflows the drawn background and the clickable area
  disagrees with the visible one.
- **`crates/crcbl-ui/src/draw_list.rs:216` — `RectOutline` miters the top edge
  but not the bottom, leaving a triangular notch at both top corners**
  (correctness). The top quad tapers to `inner_min.x..inner_max.x` at
  `inner_max.y` while the left/right quads stop at `inner_max.y`; the `t×t`
  corner square is only half-covered. The bottom edge is emitted full-width
  instead, so the two ends of the same border are built differently.
- **`crates/crcbl-ui/src/draw_list.rs:210` — `thickness` is never clamped to
  half the extent; a thick border on a small rect produces self-intersecting
  bowtie quads** (correctness) **[probe]**. `rect_outline((0,0),(10,10), 8.0)`
  emits a top quad whose bottom edge runs right-to-left, plus three more
  inverted quads that paint over the whole box.
- **`crates/crcbl-ui/src/widget.rs:203` — a click is credited to whatever widget
  the cursor is over on release, regardless of where the press started**
  (correctness). `was_released && state == ButtonState::Hovered` has no notion
  of an active/captured widget (there is no widget-id or retained-state layer
  anywhere in the crate), so pressing A, dragging to B and releasing fires B.
  Standard IMGUI practice is an `active_id` latched on press.
- **`crates/crcbl-ui/src/hud.rs:96,106,138,191` — HUD buttons are unreachable**
  (correctness/YAGNI). The doc says "returns the index of any button that was
  clicked", but `render` returns `()`;
  `_button_clicked: impl Fn(usize, ButtonState) -> bool` is never called;
  `btn.render(...)`'s `#[must_use]` bool is dropped with `let _ =`; and
  `Hud::render` passes `Vec2::ZERO` / `false` / `|_,_| false`. No caller can
  observe a HUD click.
- **`crates/crcbl-ui/src/hud.rs:129` — the button state machine only ever
  produces `Hovered` on the frame the mouse is released, and never `Pressed`**
  (correctness). `if hit_test { if mouse_released { Hovered } else { Idle } }`
  means a hovered-but-not-releasing button draws with the idle background, so
  `Style::bg_hover`/`bg_active` are effectively dead.
- **`crates/crcbl-ui/src/hud.rs:148` — right/bottom anchors do not subtract the
  content size, and `Center` is not centered** (correctness). `TopRight` returns
  `screen.x - offset.x` as the panel's _left_ edge and content grows rightward,
  so a right-anchored panel runs off-screen (the test asserts `790.0` and calls
  it correct). `Center => (screen - offset) * 0.5` also halves the offset.
- **`crates/crcbl-ui/src/widget.rs:90` + `text.rs:32` — `Label`'s background
  rect does not line up with its text** (correctness) **[probe]**.
  `DrawCommand::Text::pos` is documented as the "top-left anchor"
  (`draw_list.rs:60`), but `GlyphMetrics::rect` treats it as a baseline.
  `Label::new("Hi").with_bg(true)` at `(10,100)` draws a bg spanning y
  `100..116` while its glyphs span y `96.9..110.9`.
- **`crates/crcbl-input/src/lib.rs:103,541` — the WASD composite is documented
  as a "unit vector" but is never normalized** (correctness) **[probe]**. W+D
  yields `(1,1)`, magnitude `1.414` — 41% faster diagonal movement. There is no
  dead-zone, sensitivity, or normalization knob anywhere in the crate.

### Low

- **`crates/crcbl-input/src/lib.rs:294,521`** — NaN/inf deltas propagate
  straight into action values **[probe]**: `mouse_motion(NAN, 1.0)` →
  `Axis2 { x: NaN, y: 1.0 }`; `f32::clamp` also returns NaN for a NaN scroll
  delta. No validation at the ingest boundary.
- **`crates/crcbl-input/src/lib.rs:245`** — `set_enabled(name, true)` for an
  unknown name inserts into `enabled` forever: unbounded `HashSet<String>`
  growth from a typo'd or stale binding-profile name, never pruned against
  `name_to_idx`.
- **`crates/crcbl-input/src/lib.rs:229`** — `declare` panics on a duplicate
  name, so bindings loaded from a user config file turn a duplicate entry into a
  process abort rather than an error.
- **`crates/crcbl-input/src/lib.rs:190`** — there is no rebinding API at all:
  `ActionMap` exposes no way to mutate `ActionDecl::bindings` after `declare`,
  and `declare` rejects the same name twice, so a rebind means rebuilding the
  whole map (losing `enabled` and hold state). `crcbl-core/src/input.rs:9`
  explicitly anticipates "user rebinds are serialized" (YAGNI/gap).
- **`crates/crcbl-input/src/lib.rs:506`** — `Axis1` keys can only add `+1.0`, so
  a two-key analog axis cannot go negative; `clamp(-1,1)` also discards a fast
  scroll's magnitude.
- **`crates/crcbl-input/src/lib.rs:206,314`** — `elapsed: f32` accumulates
  monotonically; after ~4.5 hours of uptime f32 ULP exceeds 1 ms so
  `Held { duration }` quantizes, and after ~19 days a 16 ms `dt` stops advancing
  the clock entirely.
- **`crates/crcbl-input/src/lib.rs:359`** — four near-identical
  `resolve_affected_by_*` bodies, each allocating a `Vec<usize>` and hashing a
  `String` per slot per event; one helper taking a closure removes ~90 lines,
  and swapping `enabled: HashSet<String>` for a `bool` on `ActionSlot` removes
  the per-event string hashing (DRY/perf).
- **`crates/crcbl-ui/src/draw_list.rs:173,271`** — quad emission is written out
  three times when `push_quad` (`:309`) already does it; the `Rect` arm is
  byte-for-byte `push_quad` and the `Text` arm differs only in per-corner UVs
  (DRY).
- **`crates/crcbl-ui/src/draw_list.rs:172` vs `:453`, and `:162`** — the
  implementation comment says "clockwise winding", the test comment says "CCW
  from top-left", and the doc says "Y-up" while the shader is Y-down. Winding is
  currently harmless (`ui_pass.rs:217` uses `PrimitiveState::default()` with no
  culling), but all three statements cannot be right (style).
- **`crates/crcbl-ui/src/text.rs:127`** — `text_width` `continue`s past `'\n'`
  instead of resetting, so it disagrees with `layout_line`; multi-line text
  measures as the sum of every line's advance, which is what
  `Label::width`/`Button::rect` use for layout and hit-testing.
- **`crates/crcbl-ui/src/text.rs:119`** — `unwrap_or(&self.metrics[0])` panics
  if `metrics` is empty; unreachable today, but the fallback is written as if
  the vector might be.
- **`crates/crcbl-ui/src/hud.rs:84`** — `h += (max.y - 0.0) + 2.0`:
  `max.y - min.y` spelled with a hardcoded zero that only works because the
  caller passes `Vec2::ZERO` on the line above (style).
- **`crates/crcbl-shaders/build.rs:113,124`** — artifact paths are interpolated
  raw into generated string literals while `name`/`source` use `{:?}`: a
  manifest path containing `"` or `\` escapes the literal and injects into
  `$OUT_DIR/shaders.rs`. Requires repo write access, but the two neighbouring
  interpolations already do it correctly (security).
- **`crates/crcbl-shaders/build.rs:89`** — the section name becomes a Rust
  identifier with no validation: `2d blur` yields an invalid ident and two
  identically-named sections yield duplicate `pub static`s, surfacing as a
  compile error inside generated code rather than a manifest error. `:255`
  splices the same unvalidated name into an `$OUT_DIR` path.
- **`crates/crcbl-shaders/build.rs:235`** — the pinned-version check takes the
  last non-empty line of stderr+stdout, so any trailing output from `slangc -v`
  silently fails the equality test and downgrades the byte-for-byte check to
  hash-only with only a `cargo::warning`.
- **`crates/crcbl-shaders/src/manifest.rs:158`** — `finish` never checks that
  `wgsl` and `wgsl-sha256` are both present or both absent: `wgsl` with no hash
  reaches `build.rs:79` and fails against `""` with a message naming the wrong
  cause.
- **`crates/crcbl-shaders/src/manifest.rs:66,71,76`** — `line.split('#').next()`
  strips `#` anywhere including inside a value; duplicate `[section]` names
  produce two records and duplicate keys silently last-wins; and
  `finish(record, line_number)` is handed the _next_ section header's line, so
  an incomplete section's error points past the offending block.
- **`crates/crcbl-shaders/src/lib.rs:200,212`** — `chunks_exact(4)` silently
  drops a trailing partial word from a truncated `.spv`, and `wgsl()` maps
  invalid UTF-8 to `""`, turning a corrupt artifact into an empty shader source
  instead of an error.

### Verified clean

- **`sha256.rs` is correct.** Every `K`/`H0` constant, both `σ`/`Σ` rotate sets,
  the schedule, the padding (`0x80`, pad to `len%64==56`, big-endian bit length)
  and the `Vec::with_capacity(128)` worst case (exactly 128 for
  `remainder == 63`) were checked; all five NIST vectors including the
  1,000,000-`'a'` case pass. `(len as u64).wrapping_mul(8)` only wraps past 2
  EiB.
- **`Vertex2d`'s `unsafe impl Pod`/`Zeroable` is sound** **[probe]** —
  `size_of == 32`, `align == 4`, and `bytes_of` round-trips as exactly eight
  f32s in declaration order; glam's `Vec2` is `repr(C)` with no padding.
- **`crcbl-input` replay determinism is fine** — `name_to_idx` and `enabled` are
  never iterated; all ordering comes from `slots: Vec`, and
  `InputTickState::capture` preserves registration order.
- **`build.rs` rerun coverage is complete** for `manifest.txt`,
  `compile-shaders.sh`, `CRCBL_SLANGC` and every source/spirv/wgsl path; the
  `#[path]`-included `src/manifest.rs` and `src/sha256.rs` are picked up through
  the build script's own dep-info.
- **`mesh.rs`/`triangle.rs`** — layouts, winding (cross-product tested against
  the declared normal), index range and std140 offsets are all asserted rather
  than claimed. `vertex_bytes` and `cube_vertex_bytes` are the same loop twice
  (trivial DRY).

**Notes.** Three scope items do not exist yet: there is no
widget-id/retained-state scheme (widgets are fully stateless, so no id
collisions and no unbounded state — but also no focus, keyboard nav, or overlap
arbitration), no clipping/scissor rect anywhere in `DrawList`, and no z-order
beyond command insertion order. Index overflow past u16 is not reachable:
`to_triangles` emits `u32` and `ui_pass.rs:375` binds `IndexFormat::Uint32`.

## crcbl-golden + crcbl-wl-scanner + crcbl-cli + crcbl facade

### High

- **`crates/crcbl-cli/src/screenshot.rs:22` — `crcbl screenshot` writes a
  red/blue-swapped PNG on every BGRA surface** (correctness).
  `crcbl/src/screenshot.rs:105` picks `caps.preferred_format()`, asserted to be
  `Format::Bgra8UnormSrgb` by both `crcbl-vk/src/instance.rs:965` and
  `crcbl-hal/src/swapchain.rs:397`; `draw_and_readback` copies the raw swapchain
  bytes and the CLI feeds them to `Image::from_rgba8` unconditionally.
  `crcbl-golden` exists to fix exactly this (`ChannelOrder::Bgra`) and the
  Vulkan e2e suite does it correctly (`vk_e2e.rs:1428`), but
  `OffscreenSetup.format` is a private field with no accessor, so the CLI
  _cannot_ correct for it.
- **`crates/crcbl-wl-scanner/src/emit.rs:337` — XML attribute values are
  injected verbatim into generated Rust, with no identifier validation
  anywhere** (security). `message.name` and `interface.name` go straight into
  `c"{}"` literals (`:279`, `:337`), into `pub mod {name}` (`:274`), into
  `pub unsafe fn {}` via `escape_ident` (`:438` — which only checks a keyword
  list, never the character set), and into `///` doc comments.
  `xml.rs::decode_entities` turns `&quot;` into `"` and `&#10;` into a newline,
  so a crafted `<request name="…">` emits arbitrary Rust that runs at build time
  via `crcbl-shell/build.rs`. The XML is vendored in-repo, so the trust boundary
  is "whoever lands an upstream-protocol sync PR" — but the module's whole
  stated thesis is refuse-rather-than-guess, and there is no
  `[A-Za-z_][A-Za-z0-9_]*` check on any name.

### Medium

- **`crates/crcbl/src/screenshot.rs:274` — readback timeout is a `panic!` inside
  a function returning `Result`** (correctness). `OffscreenError` has six
  variants and none is used for the 10 s deadline; a slow lavapipe run makes
  `crcbl screenshot` abort with exit 101 instead of the documented exit 1,
  bypassing `report::emit` and `--json`.
- **`crates/crcbl/src/screenshot.rs:289` — `finish()` does
  `self.device.wait_idle().expect("idle")`** (correctness). A device-lost on
  teardown panics the CLI; `finish()` returns `()` and has no way to report, so
  the error is structurally unreportable.
- **`crates/crcbl-golden/src/image.rs:181` —
  `vec![0u8; reader.output_buffer_size().unwrap_or(0)]` allocates from the PNG
  header alone** (security). png 0.18's `Limits::default()` is a 64 MiB budget
  for the decoder's _internal_ buffers only; `output_buffer_size` is derived
  purely from IHDR width×height and capped only at `isize::MAX`. A ~100-byte PNG
  declaring 50000×50000 RGBA attempts a 10 GB allocation before a single IDAT
  byte is inflated. `decoder.set_limits` is never called and there is no
  dimension sanity check.
- **`crates/crcbl-cli/src/main.rs:43` — `std::env::args()` panics on a non-UTF-8
  argument** (correctness). Every path-taking flag (`--path`, `--engine`, `-o`,
  the `replay` file) is unusable with a non-UTF-8 path on Linux, and the failure
  is a panic plus exit 101 rather than the contracted exit 2. `args_os()` +
  `OsString` for the path-shaped values is the fix.
- **`crates/crcbl-wl-scanner/src/model.rs:294` — self-closing `<interface/>` and
  `<enum/>` are silently dropped** (correctness). `Node::Start.empty` is
  consulted only for `"request" | "event"` (`:314`); for
  `interface`/`enum`/`protocol` the value is assigned to the state slot and,
  since the reader emits no `End` for a self-closed tag, is never pushed into
  its parent. `xml.rs`'s own doc says anything it cannot understand is an
  `XmlError` "rather than a silent skip".
- **`crates/crcbl-wl-scanner/src/model.rs:389` — parser state is three
  `Option`s, not a stack, so malformed nesting reattaches messages to the wrong
  interface** (correctness). `finish_message` returns `Ok(())` when `message` is
  `None`, and `</interface>` does not clear a still-open `message`. Given
  `<interface A><request name="r">…</interface><interface B>…</request>`,
  request `r` is pushed into **B**, silently shifting B's opcode numbering — the
  exact class of failure this crate's docs say compiles cleanly and mis-encodes
  the wire.
- **`crates/crcbl-wl-scanner/src/xml.rs:136` — `return self.next_node()` is
  unbounded recursion** (security). Whitespace-only text recurses instead of
  looping, so a document of `" <!--x-->"` repeated grows the stack one frame per
  repetition; ~10⁵ comments overflow the build script's stack.
- **`crates/crcbl-cli/src/screenshot.rs:33` — `screenshot --json` emits
  `{"ok":true,"command":"screenshot"}` and nothing else** (correctness).
  `json: vec![]` while `human` carries the output path and dimensions, violating
  the "`--json` mirror of every human message" rule restated in `report.rs` — on
  the one subcommand a CI job would want to read a path out of. `tests/cli.rs`
  covers `new` and `build` but never `screenshot`.
- **`crates/crcbl-wl-scanner/src/emit.rs:563` — generated `decode_event<'a>` has
  a lifetime constrained by nothing** (unsafe-soundness).
  `pub unsafe fn decode_event<'a>(opcode: u32, args: *const WlArgument) -> Option<Event<'a>>`
  (pinned verbatim by `tests/golden.rs:399`). `args` is a raw pointer, so `'a`
  is caller-chosen and can be `'static`; the `&CStr`/`&[u8]` fields point into
  libwayland's closure storage and dangle the moment the dispatcher returns. It
  is an `unsafe fn` with a Safety section, so not unsound per se — but tying
  `'a` to a `&'a` witness parameter would move the guarantee into the type
  system for zero cost.
- **`crates/crcbl/src/screenshot.rs:181` —
  `byte_count = u64::from(w) * u64::from(h) * 4` is unchecked and `--size` has
  no upper bound** (correctness). `--size 4000000000x4000000000` overflows the
  `u64` multiply (panic in debug, wrap in release, then a wrong-sized
  `create_buffer` and `vec![0u8; byte_count as usize]`); more modestly,
  `--size 100000x100000` is a 40 GB allocation.

### Low

- **`crates/crcbl-golden/src/image.rs:79,149`** —
  `width as usize * height as usize * 4` overflows `usize` for `w*h > 2^62`;
  `from_rgba8` would then accept a short buffer, and `Image::filled` bypasses
  the length check entirely and loops `w*h` times pushing into a
  wrapped-capacity `Vec`.
- **`crates/crcbl-golden/src/compare.rs:146`** —
  `f64::from(self.width.max(1) * self.height.max(1))` multiplies two `u32`s;
  `pixel_count` is already computed correctly as `u64` at `:187` but is not
  stored on `Comparison` (correctness/DRY).
- **`crates/crcbl/Cargo.toml:24`** — `crcbl-golden` is a regular (non-dev)
  dependency of the umbrella but is referenced only from doc comments; this also
  falsifies `crcbl-golden/Cargo.toml:17` ("It reaches no shipped binary") —
  `png` is in the shipped `crcbl` binary's graph via both the umbrella and
  `crcbl-cli` (YAGNI).
- **`crates/crcbl-cli/src/cargo.rs:130`** — `execute` can only ever return
  `Ok(0)`, so the `cargo_exit_code` JSON field on success is a hard-coded `0`
  and the `i32` return is dead generality (YAGNI).
- **`crates/crcbl/src/screenshot.rs:159`** — `advance()` and the `elapsed` field
  have no caller anywhere in the workspace; every screenshot renders `spin(0.0)`
  (YAGNI).
- **`crates/crcbl-cli/src/new.rs:138`** — `render` substitutes `{{engine}}` into
  `path = "{{engine}}"` without escaping; `--engine '/tmp/a"b'` produces an
  unparseable manifest. `{{name}}` is safe because `check_name` restricts it to
  `[A-Za-z0-9_-]`.
- **`crates/crcbl-wl-scanner/src/xml.rs:105`** — the `<!` branch scans to the
  first `>`, so a DOCTYPE with an internal subset leaves `]>` to be re-parsed as
  a text node and the document silently continues, contrary to the comment.
- **`crates/crcbl-wl-scanner/src/emit.rs:585`** — event field expressions use
  `args.iter().enumerate()` as the wire slot index while `Message::type_slots`
  correctly expands an untyped `new_id` to three slots. No vendored protocol has
  an untyped `new_id` in an event (all eight XMLs scanned), so this is latent
  only.
- **`crates/crcbl-wl-scanner/src/emit.rs:274,396`** — interface module names go
  through no `escape_ident` while enum module names at `:610` do; and the
  untyped-`new_id` path synthesises parameters literally named `interface` and
  `version`, which would collide with a protocol argument of either name (fails
  loudly at compile time).
- **`crates/crcbl-cli/src/replay_cmd.rs:25,41,42`** — `tick_at(i).get() as i64`,
  `tick_rate() as i64`, `len() as i64`; the tick raw value wraps to negative
  above `i64::MAX`, and `Json::Number` is `i64`-only.
- **`crates/crcbl/src/backend.rs:207`** — the `UnknownBackend` arm of
  `open_backend` is unreachable; `REGISTRY` covers all three `GpuBackend`
  variants (YAGNI).
- **`crates/crcbl/src/lib.rs:9`** — the re-export table omits `crcbl::ui`, which
  is re-exported at `:84` (style).
- **`crates/crcbl-cli/Cargo.toml:33`** — three deps use
  `version = "0.1.0", path = "…"` while every other crate uses
  `workspace = true` (DRY).
- **`crates/crcbl-scene/src/lib.rs`** — a 4-line doc-only placeholder crate that
  nothing depends on (YAGNI).

**Notes.** Determinism of the generator is clean — `InterfaceIndex` is a `Vec`
with linear lookup and every collection preserves document order, so there is no
`HashMap`-iteration instability anywhere in `crcbl-wl-scanner`.
Signature/type-table/opcode/destructor-flag correctness is well pinned by
`tests/golden.rs` against real `wayland-scanner` output, including the
untyped-`new_id` three-slot expansion. `json.rs` escaping is correct per
RFC 8259. `crcbl-golden`'s comparison math (per-pixel + block SSIM, alpha in the
per-pixel bound and out of luma) is sound and its tests cover the over-tolerance
traps. `args.rs`'s `--` handling, numeric parsing and exit-code contract hold
up. No `unsafe` in any file in scope outside the emitted-code templates.

## apps + workspace config + CI

### Critical

- **`apps/breakout/src/game.rs:629` — restart never rebuilds the brick grid;
  winning makes the game unplayable** (correctness). `restart_game()` only calls
  `reset_ball()`. The bricks were `swap_remove`d from `ctx.bricks` (`:569`) and
  `despawn`ed (`:623`), and nothing re-spawns them. After a `Won` → Space
  restart, `bricks.is_empty()` is still true, so the first `Playing` frame
  immediately re-enters `Won` at score 0 (`:612`); after a `Lost` restart the
  player resumes on a partially-cleared grid. Score and lives _are_ reset
  (`:366`), so the reset is half-done in a way no test catches.
- **`apps/breakout/src/app.rs:281` + `game.rs:373` — the game is simulated once
  per _frame_, not once per _tick_; paddle speed scales with frame rate**
  (correctness). `Loop::frame` runs the fixed-timestep loop into `tick()`, which
  is a no-op (`:367`), then calls `self.game.step(now)` exactly once per
  rendered frame. `Game::step` integrates the paddle with a hardcoded
  `PADDLE_SPEED * (1.0 / TICK_HZ as f64)` per call, so real velocity is
  `PADDLE_SPEED × (fps / 60)`. With `WINDOWED_IDLE = 4ms` and no vsync
  guarantee, a windowed run at ~250 fps moves the paddle roughly 4× too fast.
  Headless pins the clock to exactly 1/60 s per frame, which is why every test
  passes.
- **`apps/breakout/src/game.rs:517` — collision sweep runs once per frame while
  physics runs N times per frame; the ball tunnels below 60 fps** (correctness).
  `Server::update` (`crcbl-server/src/lib.rs:131`) drains its own accumulator
  and may run 0, 1 or several physics ticks per `Game::step` call.
  `run_game_logic` is invoked once afterwards and sweeps a single segment of
  length `vel * (1.0/TICK_HZ)` (`:539`). At sub-60 fps the server integrates 2+
  ticks but only the last tick's path is tested, so bricks and walls are passed
  straight through; above 60 fps most frames re-sweep an unchanged position.
  Both regimes are unreachable in the headless tests.

### High

- **`apps/breakout/src/gpu.rs:370` — only the paddle is ever drawn; the ball and
  all 40 bricks are invisible** (correctness). `ForwardRenderer::begin_frame`
  takes a single `model: Mat4` (`crcbl-render/src/forward.rs:340`) and breakout
  passes `paddle_model(self.paddle_x)`. Nothing submits geometry for
  `ball_entity` or the brick entities, and `Gpu` has no field holding their
  transforms. The "first playable sample" renders one cube and a text HUD; the
  ball's position is only observable through the `[HUD] Ball x:` log line.
- **`apps/breakout/src/game.rs:691` — all three "determinism" tests are vacuous:
  no script ever launches the ball** (correctness). `scripted_run` never sends
  `KeyCode::Space`, so `state` stays `WaitingForLaunch`, `run_game_logic` (gated
  on `Playing` at `:426`) never runs, and `game.score` is `0` in all six
  invocations. `scripted_game_is_deterministic` asserts `0 == 0`, and
  `brick_count_is_stable_across_different_frame_budgets` presses only
  `ArrowRight`, asserts a score it can never change, and does not observe brick
  count at all. Every gameplay bug above is invisible to this suite.
- **`apps/sandbox/src/gpu.rs` ↔ `apps/breakout/src/gpu.rs` — breakout's GPU
  module is a comment-stripped copy of the sandbox's** (DRY). Measured
  structural similarity of the normalized (comment-free) source: `open()` 87.3%,
  and the
  `frame`/`record_and_submit`/`retire_to`/`resize`/`reconfigure`/`destroy` block
  90.5% across ~170 lines each. `SwapchainConfig`, `FrameOutcome`, `GpuError`
  and all four `From` impls are duplicated verbatim; `app.rs` is 68.9% similar
  including an identical `Clock`, `Pending`, `wait_for_configure`,
  `CONFIGURE_TIMEOUT`, `WINDOWED_IDLE`, `HEADLESS_FRAME_STEP`, `ExitReason`,
  `Summary`, `Flow` and `frame_budget()`. The sandbox copy carries 90 lines of
  design rationale (seam obligations, teardown order, timeline-vs-`wait_idle`);
  the breakout copy dropped it, so the two will drift silently. This is the
  engine-setup helper the sandbox's own doc comment (`gpu.rs:86`) says belongs
  in the `crcbl` umbrella.
- **`.github/workflows/ci.yml` — no job builds for `wasm32`, though wasm32 code
  exists and was added deliberately** (ci). `cfg(target_arch = "wasm32")` code
  lives in `crcbl-shell/src/lib.rs`, `crcbl-wgpu/src/lib.rs`,
  `crcbl-wgpu/src/cell.rs` and `crcbl-hal/src/threading.rs` (commits `afd63bf`,
  `84e531b`). Every CI job runs on host targets, so none of that code is ever
  compiled, let alone linted or tested. The same reasoning the file applies to
  macOS/Windows at `:130` ("validated exclusively here, so these jobs are
  load-bearing from day one") is not applied to wasm.
- **`apps/sim/src/main.rs:62` — `--tick-rate 0` panics with a divide-by-zero**
  (correctness). `"0".parse::<u32>()` succeeds, so
  `1_000_000_000u64 / tick_rate as u64` aborts the process instead of exiting 2.
  Both other apps reject `--tick-hz 0` explicitly; sim validates nothing and has
  no tests at all.
- **`apps/breakout` — the flagship sample has no binary-level test and is never
  run by any CI harness** (ci). `apps/sandbox/tests/headless.rs` drives the
  compiled binary and asserts exit codes; breakout has no `tests/` directory.
  `--package sandbox` appears in all five harness invocations (wayland-e2e,
  x11-e2e, vk-e2e) and `breakout` in none. The only breakout coverage in CI is
  the two `#[cfg(test)] mod tests` in `app.rs` and the three vacuous ones in
  `game.rs`.

### Medium

- **`apps/breakout/src/args.rs:16` + `game.rs:26` — `--tick-hz` is a documented
  flag that changes nothing** (correctness). USAGE advertises "Simulation rate
  in Hz (default 60)". The value reaches `FrameClock::new(options.tick_hz)`
  (`app.rs:228`), which only drives the no-op `tick()` and the `ticks` counter
  printed in the summary; all actual simulation uses the module constant
  `TICK_HZ: u32 = 60`.
- **`apps/breakout/src/game.rs:393` — the client/server split is decorative;
  input is never replicated** (correctness). Every tick encodes
  `ClientToServer::Input { tick: Default::default(), data: vec![] }` and the
  paddle is instead written directly into the server's world by
  `set_paddle_position` (`:385`) from client-side code. The module doc's
  "server-authoritative physics over in-memory transport" is not what the code
  does, and the per-frame `Vec` allocation for the empty payload is pure
  overhead.
- **`.github/workflows/ci.yml:477` — the coverage job can never fail** (ci). It
  generates lcov, prints a summary and uploads an artifact with no
  `--fail-under-lines`; the TODO says the gate is off until P1 "because every
  crate is empty in P0". The repo is well past P0, so a job that measures 0%
  coverage and reports success is a green check that asserts nothing.
- **`.github/workflows/**`and`cron.yml:21`— no miri and no sanitizer job, with real`unsafe`in the tree** (ci).`cron.yml`'s comment schedules miri for "when nontrivial unsafe arrives (crcbl-vk at P1)". `crcbl-vk`is present and full of`unsafe`, and neither workflow has a miri, ASan or TSan job. The workspace denies `unsafe_op_in_unsafe_fn`
  — a lint, not a UB check.
- **`rust-toolchain.toml:7` — `channel = "stable"` is not a pin, contradicting
  the file's own claim** (ci). The header says "CI installs the same toolchain
  from this file, so local and CI never drift", but `stable` resolves to
  whatever rustc was released most recently; since every clippy job runs
  `-D warnings`, a new stable release turns CI red on an untouched repository.
  The `decoder-fuzz` job shows the project knows how to pin
  (`nightly-2026-07-02`).
- **`deny.toml:7,30` — the two checks that fire most often are set to `warn`**
  (security). `yanked = "warn"` and `multiple-versions = "warn"` both produce
  output and exit 0. With 207 third-party packages in `Cargo.lock`,
  `multiple-versions` is likely already firing, so `cargo deny check` genuinely
  gates only on advisories, licenses, wildcards and sources.
- **`deny.toml:24` — the comment justifying `unused-allowed-license = "allow"`
  is stale by 207 crates** (security). "most entries are unmatched while the
  workspace has no third-party dependencies" was true once; the allow-list is
  now load-bearing, and `unused-allowed-license = "allow"` hides which of the
  eleven allowances are exercised — including `MPL-2.0` and `CC0-1.0`.
- **`apps/breakout/src/audio.rs:78` — every sound plays at half speed**
  (correctness). `data` is interleaved stereo so a frame is two `f32`s, but
  `pos += step` with `step = cue.pitch_ratio` advances one index per output
  frame and `idx = pos as usize & !1` masks each pair to be emitted twice. The
  440 Hz bounce beep comes out at 220 Hz and lasts twice as long. The step
  should be `2.0 * pitch_ratio`.
- **`apps/breakout/src/app.rs:296` / `apps/sandbox/src/app.rs:492` — teardown is
  skipped entirely when any frame returns an error** (correctness). `run()`
  propagates `engine.frame()?`, so `finish()` — and therefore `gpu.destroy()`
  and `shell.destroy_window()` — never run on the error path, and neither `Loop`
  nor `Gpu` has a `Drop` impl. Within `finish()`, an error from
  `self.gpu.destroy()?` likewise skips `destroy_window`. The `crcbl-vk` `Drop`
  impls do reclaim the objects, but while logging "N object(s) still alive at
  device teardown".
- **`apps/breakout/src/game.rs:327` — a key pressed and released inside one
  frame is silently dropped** (correctness). `key_event` queues into
  `pending_keys`; `step` flushes the whole queue after `begin_tick` and then
  reads `just_pressed` once, so a press+release in the same pump batch resolves
  to "not pressed" and the launch/restart input is lost. The comment documents
  this as a workaround for an `ActionMap` double-resolve bug — trading one
  edge-detection bug for another.

### Low

- **`apps/breakout/src/gpu.rs:513` / `game.rs:28`** — paddle geometry constants
  defined twice: `game.rs` exports `pub const PADDLE_HALF_WIDTH`/`PADDLE_Y` and
  `gpu.rs` declares private copies with identical values; the `pub` ones are
  never referenced outside `game.rs`. Changing the collider width desyncs the
  rendered paddle from the colliding one (DRY).
- **`apps/breakout/src/args.rs:87`** — `--backend vk` is rejected in breakout
  but accepted in sandbox (which delegates to `GpuBackend::from_name`). Every CI
  harness script invokes `--backend vk`, so those scripts cannot be pointed at
  breakout without editing them (DRY).
- **`apps/breakout/Cargo.toml:10`** — the comment says "one engine dependency…
  the game names `crcbl` and nothing else" directly above nine direct engine
  crates plus `glam` and `log`. Sandbox's manifest makes the same claim and
  honors it (style).
- **`apps/breakout/src/app.rs:285`** — a fresh `DrawList` and three `format!`
  allocations per frame at ~250 fps, all discarded after `set_draw_list`,
  abandoning the "steady-state frame allocates nothing" property the sandbox is
  explicit about (perf).
- **`apps/breakout/src/app.rs:290` / `apps/sandbox/src/app.rs:481`** — `frames`
  only increments on `FrameOutcome::Presented`, so a permanently-suboptimal
  swapchain never advances toward `budget` and `--frames N` never terminates; in
  breakout each non-presenting turn still calls `game.step`, so the simulation
  advances during a resize storm while nothing is drawn.
- **`apps/breakout/src/game.rs:121,370,616`** — dead scaffolding:
  `BreakoutModule::register` is empty while the state-machine comment claims
  `Playing` is "handled by BreakoutModule"; `run_game_logic` takes
  `paddle_entity` and discards it; `step` computes `alpha`, uses it at `:403`,
  then re-discards it at `:466`. The paddle bounce works only because the paddle
  falls through the generic non-brick branch (YAGNI).
- **`apps/breakout/src/audio.rs:130,169`** — two latent panics: `play_panned`
  computes `id as usize - 1` (underflows for `id == 0`), and `fade_env` computes
  `(total - i)` after a `saturating_sub`, which underflows when `total < fade`
  (any sound shorter than 1.25 ms at 48 kHz).
- **`apps/breakout/src/gpu.rs:336`** — when the compositor hands back a
  different size than requested only `configured_extent` is corrected, so a
  later `resize()` comparing against the stale `config.extent` triggers a
  needless reconfigure. Same code in the sandbox copy (`:487`).
- **`apps/sim/src/main.rs:82`** — `final_tick` can exceed the ticks actually
  simulated: the inner loop breaks at `ran >= ticks` leaving whole ticks
  unconsumed, but `clock.tick()` is read afterwards and printed alongside `ran`.
  For a harness whose entire output contract is `hash:… ticks:… final_tick:…`,
  two fields that can disagree is a confusing signal.
- **`.github/workflows/ci.yml:87,98,117`** — `cargo-machete`, `cargo-deny` and
  `nextest` install as floating latest while `cargo-fuzz@0.13.1` and
  `SLANG_VERSION: "2026.14"` are pinned with reasoning. A cargo-deny release
  that adds a check flips a green repo red with no commit (ci).
- **`.github/workflows/ci.yml`** — actions are referenced by mutable major tags,
  not commit SHAs (`actions/checkout@v7`, `Swatinem/rust-cache@v2`,
  `taiki-e/install-action@v2`, …). Blast radius is small
  (`permissions: contents: read` at both workflow roots, no `secrets.*`, no
  `pull_request_target`, and the one interpolation is a numeric PR number), but
  `taiki-e/install-action` downloads and executes third-party binaries into the
  build (security).
- **`.github/workflows/ci.yml:118`** — no `--no-default-features` leg; every
  build/lint/test uses `--all-features`, so nothing verifies that any crate
  still compiles with default features disabled (ci).
- **`.gitignore:7`** — the "ships binaries (`crcbl`, `sandbox`)" list omits
  `breakout` and `crcbl-sim`; the line exists to justify committing `Cargo.lock`
  and now under-states the case (style).

**Notes.** Two things expected and not found: `caps.max_image_count` is
correctly normalized away from Vulkan's `0 == unlimited`
(`crcbl-vk/src/instance.rs:880`), so the `.min(max_image_count)` in both apps is
safe; and the e2e harnesses' Vulkan-loader probes hard-fail under `CI` rather
than skipping (`run-wayland-e2e.sh:207`, `run-x11-e2e.sh:259`), plus both parse
the nextest run-count through a colour-stripping filter and fail on zero — a
genuinely well-built anti-silent-skip guard. CI security posture is clean: no
`pull_request_target`, no `secrets.*`, no `continue-on-error`, no `|| true`,
`contents: read` at both workflow roots.

## Suggested order of work

1. **Stop the process-killers.** The four allocation bombs (`save.rs:237`,
   `replay.rs:185`, `qoa.rs:164`, `golden/image.rs:181`) and the storage path
   traversal (`store/lib.rs:151`) are small, local fixes with a clear pattern to
   copy from `crcbl-net`: validate the declared count against the bytes actually
   remaining before reserving.
2. **Fix the two unsound `unsafe impl`s** (`mixer.rs:138`, and either enforce or
   remove `crash_ring.rs`'s single-producer contract), plus the `Sink` pointer
   provenance in `wayland/mod.rs:681`.
3. **Make the gameplay stack simulate what it claims.** The BVH refit index, the
   hardcoded physics dt, breakout's per-frame stepping, and the three quadratic
   solvers that disagree. Then make the breakout determinism tests actually
   launch the ball — they are the harness that should have caught all four.
4. **Decide what `crcbl-wgpu` and the Web shell are for.** Both currently
   compile and neither can complete a frame. Either wire them up behind a test
   that drives a real surface/canvas, or mark them explicitly unfinished so
   nothing routes to them by default.
5. **Bring the null backend up to `crcbl-vk`'s validation**, and fix
   `crcbl-vk`'s remove-before-owner-check. That closes the gap where CI
   green-lights streams Vulkan rejects.
6. **Netcode authentication.** The ack/reject/trusted-flag forgeries are all one
   missing layer; a per-session MAC over the wire payload closes all four at
   once, and the keyframe watchdog closes the recovery hole.
7. **CI gaps**: a wasm32 build leg, a miri job over
   `crcbl-vk`/`crcbl-audio`/`crcbl-store`, a coverage floor, and
   `yanked`/`multiple-versions` promoted to `deny`.
