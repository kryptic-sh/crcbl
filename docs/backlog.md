# Backlog

What was raised and not finished. A changelog says what shipped; this says what
did not, and why. Delete an entry when it ships — `git log` is the history.

### `crcbl-dx12` points at a backlog note about `crcbl::screenshot` that is not here

`an_offscreen_ring_draws_reads_back_and_comes_round_again`, in
`crates/crcbl-dx12/src/swapchain.rs`, explains its closing `TransferSrc` →
`Present` barrier with "see the note about `crcbl::screenshot` in
`docs/backlog.md`". There is no such note, and
`grep -n screenshot docs/backlog.md` finds nothing about barriers at all — it
was either never written or deleted with something else.

The defect it was about is fixed:
`crcbl::screenshot::OffscreenSetup::draw_and_readback` now brackets its copy
with `Present` → `TransferSrc` and `TransferSrc` → `Present`, and
`every_readback_barrier_declares_the_state_the_image_is_actually_in` replays the
null backend's recorded stream to hold it there. So what is left is a dangling
cross-reference in a doc comment, in a crate outside the paths that fix owned.
Either repoint it at that test or drop the clause; it is a one-line edit and
needs a Windows-crate touch, not a decision.

### The Win32 pointer-clip tests are held out of the ordinary sweep

Three flakes across the session, on **two different tests** and **three
different assertions**, every time for a commit that touched no shell code
(`d9ee566`, `28fc1b7`, `0354eec` — all renderer or CI changes).

One shape underneath all of them: the test compares a clip rectangle the system
applied against one this process computed, and both operands move. The desktop
repositions windows, the foreground is contended by whatever else the runner is
doing, and this runner changes its display set mid-run — the behaviour that made
`refresh_clip` refuse a degenerate refresh.

Two rounds of narrowing helped and did not fix it: reading through
`confined_to_client` immediately before asserting, and re-deriving the rectangle
after a restore rather than reusing one from before the minimize. The prediction
in this entry was "if it flakes a third time, stop patching and quarantine" — it
did, so both are now `#[ignore]`d.

**They are not disabled.** `run-win32-e2e.ps1` passes `--run-ignored all` and
runs on a real interactive desktop, so they still gate there — the only place
their preconditions actually hold. What they stop doing is failing the workspace
sweep, where nothing guarantees a foreground window or a stable display set.
Deleting them would have been worse: a process that keeps the cursor clipped
after losing focus has taken the desktop hostage.

Still open and unrelated: the **focus** flake (three instances), where
`focus_and_confirm` loses the foreground.

### `GpuInstance::flags` is a bare `u32`, not `bitflags`

**Recommendation taken, yours to override.** `LIVE = 1 << 0` is the first
defined bit and more are coming (§3.3's own needs, topic 18's per-instance
toggles), so `crcbl_hal::Features`-style `bitflags` is the house pattern and
would normally win.

It lost on one fact: the type has to live where the layout lives, and
**`crcbl-shaders` has no dependencies at all, deliberately** — its `Cargo.toml`
says so, because the library is what a not-yet-written backend consumes.
`bitflags` would be its first, and taking a new dependency is your call. The
alternative — a wrapper type in `crcbl-render` — would be a second
representation of the same word, which is the drift `crcbl-shaders` exists to
prevent.

So: an associated const `GpuInstance::LIVE`, documented as bit 0. Revisit when
topic 18's toggles land; the cost of the switch is one dependency on
`crcbl-shaders` and nothing else.

Calls made on judgement during the 2026-08-09 planning session, listed so they
can be confirmed or reversed without re-deriving anything. **Each says what was
decided, why, and what reversing it costs.** Delete an entry once it is
confirmed; the rest of this file assumes them.

- **`naga` added as a dev-dependency of `crcbl-shaders`, taken without asking.**
  A new dependency is normally the user's call. Taken because the alternative
  was leaving three of four committed shader artifacts validated by nothing —
  the gap that let `wgsl/ui.wgsl` ship for months with a `var<uniform>` carrying
  no binding decoration, which `crcbl-wgpu` could never have loaded. naga is the
  tool that would have caught it, is already in `Cargo.lock` through wgpu at the
  same version, and is dev-only so it does not ship. `git diff Cargo.lock` is a
  three-line dependency edge and **no new package entered the graph**;
  `cargo deny` is clean. **To override:** drop the dev-dependency and the WGSL
  artifacts go back to being unchecked, or find a validator that is not naga.

- **The shader manifest's section order was locale-dependent, and that broke
  `main` a third time.** `compile-shaders.sh` iterates `shaders/*.slang`, and a
  glob is sorted by the caller's collation: `en_US.UTF-8` ignores the
  punctuation and puts `mesh_shader.slang` before `mesh.slang`, while `C`
  compares bytes and puts `mesh.slang` first. So the committed manifest carried
  one developer's locale, CI regenerated the other order, and the byte
  comparison refused it — with every artifact identical and only the section
  order differing. `export LC_ALL=C` fixes it and the manifest was regenerated.

  Worth keeping because the class is general: **this project pins its compilers
  and now its validator, and the environment those run in is provenance too.**
  Any glob, sort, `uniq` or `tr` in a build script has the same exposure. It
  went unnoticed for months because no two shader filenames collided this way
  until `mesh_shader.slang` arrived next to `mesh.slang`.

- **CI's `spirv-val` is pinned to a fixed `.deb`, after an unpinned one broke
  `main` twice.** The shader job installed whatever `spirv-tools` the runner
  image carried. On 2026-08-09 that was **SPIRV-Tools v2025.1**, which rejects a
  valid mesh shader over `VUID-PrimitiveTriangleIndicesEXT-…-07054` — claiming
  the indices decoration is used without `OutputTrianglesEXT`, on a module that
  declares that execution mode on both entry points reading the decorated
  variable.

  **This was established rather than argued.** Both validators were fetched and
  run locally against the same artifact: v2025.1 rejects it, v2026.1 and v2026.3
  accept it, and radv and lavapipe both render it correctly. So it is an
  upstream validator bug fixed between those releases, and the artifact is
  sound. The pin is Ubuntu's own `.deb` at a fixed version, because LunarG
  publishes no repository for the SDK originally reached for — that first
  attempt 404'd and failed the job a second time, which is its own lesson about
  pinning to a URL nobody checked.

  **To override:** if you would rather not depend on a validator version, the
  alternative is to stop two mesh entry points sharing one
  `PrimitiveTriangleIndicesEXT` variable — split `amplifiedMeshMain` into its
  own `.slang` — which makes the artifact acceptable to v2025.1 too. That works
  around a fixed upstream bug in shader structure, which is why it was not
  chosen.

- **Metal's `DRAW_INDIRECT_COUNT`: the seam was _not_ reshaped.** This reverses
  an explicit instruction ("update the seam and get all features supported in
  all the native backends"), on evidence found after it was given: `wgpu-hal`
  declines the same feature on Metal — `wgpu-types` documents
  `MULTI_DRAW_INDIRECT_COUNT` as DX12 and Vulkan only, and its Metal backend
  contains no multi-draw code at all. Two independent implementations reached
  the same conclusion, so it is a Metal API fact rather than a gap. With mesh
  shaders as the primary geometry path, Metal sits on the primary path anyway
  and the count only affects the fallback. **To override:** the seam grows a
  "record indirect work before the pass opens" step and Metal builds an ICB from
  the count buffer in a compute kernel — real work, and it makes the seam less
  Vulkan-shaped, which `crcbl-hal` has resisted so far.
- **Shader pipeline: four independent Slang lowerings kept, plus guardrails.**
  Godot's SPIR-V-as-single-IR model would make the `SV_InstanceID` class of
  divergence structurally impossible, but costs two vendored C/C++ translators
  and cannot serve the WGSL leg anyway (naga's SPIR-V frontend rejects
  `DrawParameters`). Recorded as reopenable in `docs/plan/02-vulkan-backend.md`
  with a named trigger. **To override:** adopt SPIRV-Cross and spirv-to-dxil for
  the native targets.
- **`crcbl-wgpu` should report capabilities honestly rather than pinning to a
  low tier.** wgpu on native exposes bindless, multi-draw-indirect-count, ray
  query and mesh shaders; the reduced set belongs to the browser, not the crate.
  **To override:** keep it deliberately limited as a pure triage backend, and
  say so in its crate docs so the pinning is not read as a bug.
- **The editor is native-only.** `10-wasm-webgpu.md` called editor-in-browser a
  stretch that "should mostly work by construction"; the asset browser, OS
  drag-drop and the file watcher are all native-shaped and nobody examined it.
  **To override:** scope what a browser editor would actually do about those
  three.
- **`crcbl_ui::hud` gets deleted, not extended.** It has no consumer, and the
  obvious fix (a `color` on `Label`) builds on the model topic 7's CSS rewrite
  replaces. **To override:** add the field and have the samples adopt it,
  knowing it is throwaway.
- **towers co-op and arena are native LAN**, and arena's prediction work is
  therefore validated against injected latency only. towers loses its
  mixed-native/browser marquee session. **To override:** host something.
- **The `delve` sample was folded into `shard` before it was written.** It was
  proposed to fill the web-flagship vacancy; shard's web milestone fills it, and
  two samples in one genre is duplication. **To override:** split them again.
- **New phase and gate names are inventions**: P7B (raster twin), P7C
  (ray-traced path), S4B (lumen), S4C (quarry), S6B (shard web slice), S7
  (bracket). So are the sample names lumen, quarry, shard and bracket.
- **Point-light shadows moved into MVP.** They were post-MVP; the raster twin
  has to cover every light type ray-traced shadows cover, so they follow from
  the parity decision rather than being a separate call.
- **bracket keeps a single-player web demo** — client and matchmaking server
  in-process over `InMemoryTransport` — rather than shipping no web build.
  Preserves sample rule 7 and demonstrates the matchmaker and rating curve; only
  the transport is absent.

## Owed

The S1B findings in `docs/plan/ROADMAP.md` were the substantive list — six
places two unrelated games were pushed into the same workaround. **All six are
now closed**: 1 by `SpriteRenderer` (P4B), 2 by `crates/crcbl/src/web.rs`, 3
inside the phase that found it, 4 by `crcbl::store::record::Record`, 5 by
`crcbl_audio::mixer` reached through the blanket `impl AudioSource for Arc<T>`,
and 6 by the umbrella's re-exports — verified by reading each sample's manifest
and the crates named above, not by trusting the roadmap's own status column,
which still carries the pre-closure narrative for 2. What is left below has no
phase attached to it.

- **`rect` and `uv` are both `[f32; 4]` and adjacent in `Sprite::new`, and
  nothing but a picture catches a swap.** This is the hazard `SheetDesc`'s doc
  comment names — "two adjacent `u32`s that can be swapped at the call site are
  a bug the compiler cannot see" — reintroduced knowingly when `Sprite` gained a
  constructor, because the alternative (defaulting `uv` to the whole sheet)
  trades a swapped-argument wrong picture for a silently-whole-sheet one. What
  catches it today: `the_instance_layout_is_exactly_what_the_shader_reads`
  asserts the two lanes at their byte offsets from distinct values, and the
  sprite goldens catch it at any call site the golden scenes reach. **A call
  site no golden covers is not covered** — every sample's `art.rs` is in that
  set.

  The real fix is newtypes — a `WorldRect` and a `SheetUv`, so the compiler
  refuses the swap — and it ripples through every `[f32; 4]`-returning helper in
  the samples, which is why it was not attempted inside an API refactor. Worth
  doing when something else is already touching those helpers.

- **`SheetDesc` was considered for the same treatment and declined.** Its own
  doc argues it is already the safe form: five named fields is what a positional
  constructor would be a regression from, and its `label`/`pixels` borrow `'a`,
  so a `new` would carry the lifetime into a builder chain for nothing. The
  measurement agrees — every construction names all five fields, so there is no
  "the default is fine here" population for `with_*` to serve. The half worth
  having is `#[non_exhaustive]` **alone**, and that is a decision to take when a
  sixth field (a mip count, a swizzle) is actually proposed; taking it now would
  break every call site to buy nothing today.

- **`SpriteInstance` in `crcbl_render::sprite_pass` is the same public-literal
  exposure, on the GPU-side twin.** It cannot take `#[non_exhaustive]` without
  thought, because its `bytemuck::Pod` derive and the `..Default` idiom around
  it are load-bearing. Nothing constructs one outside `crcbl-render` today.
  Noted, not investigated.

- **The listener standoff moved from the emitters onto the listener, and the
  subtraction changed precision with it.** Every sample used to compute
  `compute_cue([0,0,0], [dx, dy, 1.0])` — the listener at the origin, and "one
  unit in front" added to _each emitter's_ Z with the same comment copied into
  three files. That standoff is a fact about the camera, so it now sits on the
  listener (`LISTENER_STANDOFF` in each sample, listener at `z = -1`, emitters
  at their true Z).

  `emitter − listener` is arithmetically the same, but **not bit-identical**,
  and the agent's report claiming it was is wrong. The samples that subtracted
  first — horde and flappy — did `(at.x - listener.x) as f64→f32`, one rounding;
  the new path casts each coordinate to `f32` and subtracts inside
  `compute_cue`, two roundings. The error is bounded by the coordinate
  magnitude, and horde's arena is `ARENA_HALF_WIDTH` 48 by `ARENA_HALF_HEIGHT`
  36, so it is on the order of 1e-5 on a direction that gets normalised — far
  below audibility and below every assertion in the suite, which is why nothing
  moved. Recorded because "bit identical" is what someone would otherwise assume
  when reading the diff, and it would be the wrong thing to rely on if these
  coordinates ever grow.

- **`CueGrammar` is a parameter of `Mixer::cue` that every call site passes
  `&CueGrammar::default()` to** — five of them. By the workspace's own rule that
  is a parameter nothing varies, and putting the grammar on the mixer beside the
  listener would collapse `cue(emitter, &CueGrammar::default())` to
  `cue(emitter)`. Deliberately not taken with the listener: "this mixer's
  grammar" is a bigger claim than "this mixer's listener", and it was not part
  of the decision that was delegated.

- **`Listener` has a position and no orientation**, so `compute_cue` still
  hard-codes "the listener faces +Z" and its module docs say so. That is the
  field the type was made `#[non_exhaustive]` to be able to gain; nothing needs
  it until a game turns its camera.

- **`docs/code-review.md` cites two latent panics in `play_panned` that are not
  there and now cannot be.** The entry names
  `apps/breakout/src/audio.rs:130,169` for an `id as usize - 1` underflow and a
  `fade_env` underflow. The first was already fixed before this session — the
  current code takes `bank.create_voice(id)` behind a `let Some(…)` guard and
  its comment says in so many words that the `id - 1` index it used to have is
  gone — and `play_panned` itself no longer exists, so the citation is doubly
  stale. Left alone because it is unclear whether that file is a living findings
  list to be pruned or a dated snapshot to be preserved; **that is the decision
  needed**, and it applies to the whole document rather than this one entry. The
  `fade_env` half was not re-checked.

- **Nothing has listened to the migrated cues on a real device.** Every sample's
  audio was rewritten onto `crcbl_audio::mixer` and the checks are all
  structural: buffer shapes, pan ordering, voice counts, loop seams. Two
  audible-only risks are unverified. Asteroids' engine changed from a pulsed
  one-shot to a continuous loop at `ENGINE_GAIN` = 0.25 against the one-shots'
  0.5, and that ratio was chosen by reasoning rather than by hearing it. The
  loop seam is asserted to be a bare tone with no envelope, which is the right
  property, but nobody has heard whether ten joins a second is inaudible in
  practice. Both want a person with headphones.

- **How many props a view holds is not pinned by anything.** The count over the
  whole arena is —
  `game::tests::the_scatter_is_sparse_and_never_pens_the_player_in` asserts it
  lands between 30 and 70 over 64 seeds — and so is the fact that the layer is
  culled, but the number a 960 × 720 window actually shows is only observable
  from `SceneStats::props` in a running frame. A test in the shape of
  `the_visible_ground_is_bounded_by_the_view_and_not_the_arena` would pin it and
  would make `PROP_DENSITY`'s "a handful in a view" checkable rather than
  asserted. Not written because the honest bound is wide — the scatter is random
  and the player's own glade sits in the first view — and a wide bound on one
  seed is a weak test.

## The samples depend on `crcbl` and `std` — met, with one exception

Reached on 2026-08-03: every one of
`apps/{bare,breakout,flappy,asteroids,horde,sandbox}/Cargo.toml` names `crcbl`
and nothing else under `[dependencies]` — the nine simulation crates are
re-exported, `glam` is `crcbl::math` and `log` is `crcbl::log`. What is left:

- **The `crcbl-sprite`/`bake` build-dependency is the one exception, and it was
  taken rather than decided.** The four game manifests carry
  `crcbl-sprite = { features = ["bake"] }` under `[build-dependencies]`, with
  the reason written in each: `crcbl` re-exports the `.crpix` _reader_ and not
  the PNG _encoder_, and cargo's `resolver = "3"` keeps the bake feature out of
  the shipped binary because build-dependency features resolve separately. The
  alternative — making the umbrella's heavy re-exports optional so a build
  script can take `default-features = false, features = ["bake"]` — buys a
  literal zero-exception rule at the price of a feature matrix on the umbrella's
  public surface. **Still a decision nobody has made**; the exception is
  defensible and is what ships today.

## Frame pacing sleeps on the monotonic clock, which is not what a display does

`crcbl::engine::Pacing` chooses a present mode (`Auto`/`Vsync` → `Fifo`,
`Adaptive` → `FifoRelaxed`/`Mailbox`, `Off` → `Mailbox`/`Immediate`; `Auto` may
then rebuild onto `Adaptive`'s once the display has been read) and `FrameLimit`
paces the loop by sleeping the difference between the last frame's length and a
period, on `std::time` — `Clock::Real` in `crates/crcbl/src/engine.rs`, where
the `wasm32` arm of `sleep` is deliberately a no-op because the browser paces
frames itself. That is the whole mechanism, and it is open loop: it never learns
when a frame was actually shown.

**The seam, the engine wiring and the Vulkan backend now exist.**
`Features::PRESENT_FEEDBACK`, `PresentInfo::present_id` and
`Device::wait_until_presented` are the capability-named seam the note below
asked for, and `GpuContext::acquire` calls the wait for the present
`FRAMES_IN_FLIGHT` behind the frame it is about to start
(`GpuContext::present_to_wait_for` is the arithmetic, tested without a GPU). A
device that does not advertise the flag answers `Ok(())` immediately, which is
what lets the call site have no branch on which backend is underneath.
`crcbl-vk` answers it with `vkWaitForPresentKHR` where the driver has
`VK_KHR_present_id` + `VK_KHR_present_wait`; the other four `Device` impls are
still the immediate answer, and `FrameLimit` is untouched and still the only
thing pacing a loop on a device without the capability.

What is still owed:

- **`vkWaitForPresentKHR` is verified on radv only, and CI does not run it at
  all.** `VK_KHR_present_wait` is a driver-conditional extension: this
  developer's radv exposes it, lavapipe does not (`vulkaninfo` lists it under
  both RADV devices and under neither llvmpipe entry), and lavapipe is what CI
  runs for both `vk e2e` and the wayland sandbox pass. So every CI leg exercises
  the _absent-capability_ path and proves nothing about the wait. `run_sandbox`
  in `crates/crcbl-shell/tests/run-wayland-e2e.sh` says so on stderr rather than
  passing quietly, and asserts the two halves agree when the extensions are
  there — the backend's own `vkWaitForPresentKHR on present` line has to appear,
  which is the only thing that tells a real wait from the immediate `Ok(())`.
  Closing this needs a CI leg with a driver that has the pair; nothing else will
  do it.

- **The windowed half of the `wait_until_presented` id check now has a check.**
  `run-wayland-e2e.sh` runs the sandbox with `--wait-unpresented`: on its first
  tick the sandbox calls `Device::wait_until_presented` with `u64::MAX` on its
  real swapchain and logs whether the device answered at once, and the
  present-feedback block asserts the success line. Falsified on radv: with the
  id guard removed, the wait blocks the whole 60 s timeout and the pass goes
  red; with it, it answers in microseconds. What is still not independently
  checkable is the offscreen guard alone — an offscreen entry never records an
  id, so the id guard answers for it, and the vk e2e
  (`the_offscreen_ring_answers_a_present_wait_with_no_swapchain_to_wait_on`)
  only goes red when **both** are removed (removing both segfaults radv on a
  `VK_NULL_HANDLE` swapchain).

- **Metal's `addPresentedHandler:` path is verified by nothing, on any
  machine.** This is the wider of the two coverage holes in present feedback,
  because unlike Vulkan's it is not driver-conditional — it is simply that no
  automated run anywhere has a drawable. `crcbl_mtl::swapchain`'s
  `attach_presented_handler` is the only code in the capability that is not
  plain Rust, and the only test that reaches it is
  `a_layer_swapchain_acquires_a_drawable_and_presents_it`, which numbers each
  present and waits for its number. That test is `#[ignore]`d **and** excluded
  by name from the `mtl-e2e` job's filter, because a headless runner's detached
  `CAMetalLayer` vends no drawable at all — so the job says nothing about it and
  never will.

  What would close it: **a person on a real Mac running
  `crates/crcbl-mtl/tests/run-mtl-e2e.sh`**, which needs a window server. That
  is the only thing that has ever executed a Metal present at all. Two outcomes
  are worth writing down when someone does. If it passes, the handler fires for
  a _detached_ layer and the capability is confirmed end to end. If it times out
  on the first wait, the handler does **not** fire for a layer outside a view
  hierarchy — which would be a real constraint, not a bug in the ledger, and
  would mean the test needs an `NSWindow` rather than a detached layer. Neither
  is known today; nothing has run it.

  Everything on this side of the callback _is_ covered and runs on every host:
  `crcbl_mtl::present`'s tests pin the immediate answers, the strictly
  increasing id, the out-of-order report, the reset across a reconfigure, the
  lapsed timeout, and that a blocked wait is genuinely woken rather than left to
  time out and re-check its condition. That module is compiled off macOS under
  `cfg(test)` for exactly that reason.

- **`Condvar::wait_timeout_while` reports success after a timeout it should have
  failed, and that is not a Metal fact.** Found while falsifying
  `crcbl_mtl::present`: with `record_shown`'s `notify_all` removed, every
  present wait still returned `Ok`, because `wait_timeout_while` re-tests its
  condition _after_ the deadline lapses and reports "not timed out" when it has
  since become true. A missing wake-up therefore costs a whole
  `PRESENT_WAIT_TIMEOUT` per frame and raises no error anywhere. The test now
  bounds the elapsed time from above as well as below, which is what catches it.
  Audited for the rest of the workspace on 2026-08-06: the only other condvar is
  `crcbl-jobs`' pool, and it calls the bare `Condvar::wait` with **no timeout**
  — a missed wake-up there is throughput, never a reported success — so this is
  the only `wait_timeout` pairing in the tree and no other instance needs the
  bound.

- **Read the real present mode with `VK_EXT_present_timing`.** The seam, the
  Vulkan backend and the engine's use of them now exist; **what no run has
  produced is an answer other than `Unknown`.** `DisplayTiming` and
  `display_timing_from_refresh_nanos` in `crcbl_hal::swapchain`,
  `Features::PRESENT_TIMING` and `Device::display_timing` are the capability-
  named seam; `crcbl_vk::present_timing` is the hand-written FFI (`ash` still
  has no bindings — rechecked against the pinned 0.38.0+1.3.281);
  `GpuContext::settle_pacing` in `crates/crcbl/src/engine.rs` is the caller,
  asking once after the first present and resolving `Pacing::Auto` against the
  answer with `Pacing::resolve`. The extension is **ratified**, not provisional
  as this entry previously said: `supported="vulkan" ratified="vulkan"` in
  `vk.xml`, revision 3, which is what `/usr/include/vulkan/vulkan_core.h`
  declares and what RADV exposes here.

  Still owed:
  - **Only the `Unknown` arm has ever executed against a driver, and that is now
    a measured result rather than an untested path.**
    `crates/crcbl-shell/tests/run-wayland-e2e.sh` drives the sandbox on RADV (RX
    7900 XTX, Mesa 26.1.6) against a nested headless sway 1.12. The chain
    negotiates —
    `crcbl-vk: present timing enabled (VK_EXT_present_timing + VK_KHR_present_id2)`
    — the query reaches the driver after the first present, and the answer is
    `hal: display timing Unknown; asked for Auto, pacing Vsync`, in both display
    modes. `Fixed`, `Variable` and `Stepped` are still covered by unit tests on
    the pure mapping and by **nothing else on any machine**. The script asserts
    only that the engine _asked_; it shouts when the arm is `Unknown` rather
    than asserting a cadence nobody has seen.
  - **The pacing resolution therefore runs on one input everywhere it runs for
    real.** `Pacing::resolve` in `crates/crcbl/src/engine.rs` maps (requested,
    observed) to the pacing in force, and
    `auto_is_the_only_pacing_the_display_can_change` walks all sixteen pairs —
    but only `(Auto, Unknown)` has ever executed against a driver, on any
    machine. Everything the `Variable`/`Stepped` branch claims (that the rebuild
    onto `FifoRelaxed`/`Mailbox` is right, that it improves anything on a real
    VRR panel, that one rebuild during start-up is not visible as a hitch) is
    argued, not measured. A VRR panel driven by a compositor that reports a
    cadence is what would settle it, and that is the same missing machine the
    entry above needs.
  - **That `Stepped` resolves to `Adaptive` is a judgement call, not a measured
    one.** The reasoning is in `Pacing::resolve`'s doc comment: a quantised
    cycle is not a fixed one, so a fixed-vblank wait is wrong there in the same
    way it is on a free-running panel. No driver here has ever emitted `Stepped`
    at all (see the mapping gap below), so nothing distinguishes this from the
    other choice — pacing a stepped panel on vsync at its current multiple —
    except the argument.
  - **Why RADV answers `Unknown` there is partly determined.** Verified: that
    sway session advertises `wp_presentation` and neither
    `wp_commit_timing_manager_v1` nor `wp_fifo_manager_v1` (`wayland-info` on
    the session the script starts), while `libvulkan_radeon.so` contains
    bindings for both of those names — so the Wayland WSI looks for protocols
    this compositor does not offer. **Not determined:** whether
    `vkGetSwapchainTimingPropertiesEXT` returned `VK_NOT_READY` or `SUCCESS`
    with zeroed figures, since `crcbl-vk` maps both to `Unknown` and logs only
    the mapped arm; and whether RADV would report a real cadence on a compositor
    that does advertise those protocols. Hyprland 0.56.2 (installed here)
    implements both, but it cannot be nested inside sway for a safe headless
    experiment — it demands `xdg_wm_base` v6 and sway 1.12 offers v5 — and
    running it on DRM would take over this machine's console. A DRM-backed
    session, or the same run under a compositor with commit-timing, is what
    would settle it.
  - **The four-state reading of `refreshDuration`/`refreshInterval` is taken
    from the proposal text, not from a driver.** In particular the `Stepped`
    case (interval non-zero, not `UINT64_MAX`, not equal to the duration) is a
    shape no driver here has been observed to emit, and the contradictory-input
    arm — an interval that does not divide the duration, mapped conservatively
    to `Unknown` — is a guess about driver bugs rather than a response to one.
  - **One present may not be enough on some platform, and the engine will never
    find out.** The query is one-shot by design — the extension may answer
    `VK_NOT_READY` until an image has been presented, and a driver that answers
    `Unknown` forever must not be re-queried forever — so a platform that needs
    _two_ presents before it will speak reads `Unknown` here and stays on
    `Pacing::Vsync`. No such platform is known; the alternative (retry until it
    answers, or until some count) was declined because the count would be a
    number nobody could justify and the failure it guards against has never been
    seen. A caller on such a platform asks for `Pacing::Adaptive` by name.
  - **The observation is not refreshed, ever.** A window dragged from a fixed
    panel to a VRR one, or a laptop entering power saving, keeps the pacing
    start-up settled on. Declined for this slice: re-reading on every
    reconfigure is the per-frame query on any driver that answers `Unknown` (it
    would re-run on every resize), and a panel that flaps between cadences would
    rebuild the swapchain each time. `GpuContext::set_pacing` is the escape
    hatch, and a game with a monitor-changed event of its own can call it.

  Note that `wait_until_presented` deliberately does **not** return a timestamp
  — a caller that needs one needs a second method, and `VK_EXT_present_timing`'s
  `vkGetPastPresentationTimingEXT` is where it would come from. Only
  `vkGetSwapchainTimingPropertiesEXT` is bound today.

Considered and declined while shaping the seam, so it is not re-argued: having
the wait return an enum distinguishing "waited" from "this device cannot observe
presents". The distinction has exactly one consumer — a log line — and
`caps().features` already answers it once at start-up, where the engine now logs
it. Also declined: refusing with `HalError::Unsupported` on a device without the
capability. It was tried as a falsification and the engine's frame loop fails
every frame under it, which is the argument against it in one line.

## `--pacing` and `--fps` reach the engine; three quarters of what they can ask for is unexercised

`crcbl::args::Common::pacing` and `::limit` carry the two values,
`Common::gpu()` hands the first to `GpuContextDesc` through `GpuOptions`, and
`Common::loop_config()` hands the second to `Loop::new`, which applies it to the
clock. `run_sandbox_paced` in `crates/crcbl-shell/tests/run-wayland-e2e.sh`
proves the whole path on a real Vulkan swapchain — the flag, the present mode,
the logged limit and the measured frame time. What that leaves:

- **The `adaptive` e2e pass runs in CI, and proves only that the present mode is
  reachable.** `run_sandbox_paced` takes the pacing and its expected present
  mode, and the wayland suite opens a second pass with `--pacing adaptive`,
  asserting `asked for Adaptive, pacing Adaptive` and a `FifoRelaxed` swapchain
  — the mode a VRR panel wants, which no run had ever opened one on (its
  coverage was unit tests and nothing else). What is still a missing machine,
  tracked in the `Pacing::resolve` entry above, is whether adaptive _improves_
  anything on a real VRR panel — this pass proves only that the present mode is
  reachable.

- **`--fps` is unobservable on every CI leg without a compositor.** The limiter
  lives on `Clock::Real` by construction, so a headless run takes the flag and
  correctly does nothing with it — which means the macOS, Windows and null-
  backend legs say nothing about it, and neither does the browser, where every
  entry point builds `Clock::manual` and `requestAnimationFrame` is the pacing.
  The wayland pass is the only place it is real.

- **`apps/sandbox` now duplicates eight shared flags rather than six.** Its
  parser is deliberately its own (`crates/crcbl/src/args.rs`'s module docs make
  the case, and it is a real one: the sandbox takes `--camera` and `--title` and
  no `--seed`). The cost is that the sandbox is also the **only** sample the
  Wayland and X11 harnesses drive, so every flag added to `Common` is either
  written twice or untestable against a window system. `--pacing` and `--fps`
  were written twice. Worth deciding once: either `apps/sandbox` consumes
  `Common::consume` for the shared set and keeps its own arms for the rest, or a
  `Common`-consuming sample joins the harness scripts. Not attempted here
  because either is a change to what the harness runs, which is not something to
  do inside a flag slice.

## The CLI scaffold gate has thin timing margin, observed once

`a_scaffolded_project_builds_lints_and_runs_headless` in
`crates/crcbl-cli/tests/cli_e2e.rs` scaffolds a project into a temporary
directory and points `CARGO_TARGET_DIR` at another one, so **every run is a cold
build of the whole engine** — deliberately, because an inherited target
directory would deadlock against the lock the test runner already holds.
`.config/nextest.toml` gives it
`slow-timeout = { period = "60s", terminate-after = 4 }`, a 240s ceiling, and it
has measured **`TIMEOUT [240.174s]`** once and `36.367s` on the rerun — a 6x
spread, cause undetermined, "seen once, unexplained". It has not failed in CI.
What is worth knowing: the margin is a wall-clock budget on a from-scratch
engine build, and it moves with every dependency the engine gains and every
runner GitHub retires. If this job ever goes red on a timeout, the fix is the
`period`/`terminate-after` pair rather than anything in the scaffold.

## P5B — the job system, and the two decisions in front of it

`crates/crcbl-jobs` carries the spawn seam (`Spawn`, `Threads`, `Inline`,
`default_spawner`), the design's two communication primitives — `mailbox`
(latest-wins triple buffer, for states) and `ring` (bounded SPSC, for streams) —
and the work-stealing `pool` with `par_for` in both modes. The order is forced:
the spawn seam and its single-threaded fallback came first (a pool on
`std::thread` would silently have no browser story — spawning _compiles_ on
wasm32 and returns `UNSUPPORTED_PLATFORM` at run time), then the pool, then
adoption. **What is still owed is the worker backend behind the seam.** The
adoption slice found **one consumer, not four**, and that is a fact about the
samples rather than a shortfall: `apps/horde`'s `steer_enemies` is on `par_for`,
and every other candidate collection is smaller than a single chunk — breakout
has forty bricks, asteroids at most forty-four rocks — so a `par_for` over them
would be the serial loop plus a pool. **The "two samples freeze a seam" rule has
therefore not been met**, and `Spawn::threaded` returning a `bool` is still the
most likely thing to give.

- **Only `overlap_sphere_into` has the shared form.** `cast_ray`, `sweep_sphere`
  and `overlap_aabb` are still `&mut self` — nothing parallel calls them yet,
  and `sweep_bolts` (the obvious candidate) reduces into a shared hit list in an
  order the scheduler would choose, so it needs a design decision before it
  needs an API.
- **`STEER_CHUNK` was chosen by argument, not by measurement.** Sixty-four
  enemies a chunk keeps the split independent of the worker count and stays
  under the pool's 1024-slot queue up to 65 536 enemies. Nothing has swept the
  value, and the right time to is when there is a benchmark that isolates the
  pass.

**The atomics are checked by Miri and by nothing else.** x86-64 is
total-store-order, so a `Release` store and a `Relaxed` one compile to the same
instruction and weakening one is invisible to any test on this machine — which
is why the Miri job is load-bearing. It runs **weekly, in `cron.yml`**, a
deliberate choice: the full crate list is minutes of interpretation per PR, and
the per-commit value is concentrated in one small crate. The consequences to
keep in mind: **an ordering regression can sit on `main` for up to a week**, so
any change to the atomics is expected to be run under
`cargo miri test -p crcbl-jobs` (~23 s for its 40 tests) before it is pushed —
written into the crate docs where somebody editing the atomics will see it — and
**after a dependency lands, trigger the cron manually**
(`gh workflow run cron.yml`) rather than waiting for Monday; the weekly job went
red on 2026-08-03 for want of a `libasound2-dev` install and was found only
because it was briefly on the per-PR path.

Also still open: **nothing runs the primitives on a weakly-ordered machine.**
Miri models the memory ordering, which is a stronger check than any test on x86,
but it is a model — an aarch64 runner exercising the same stress tests natively
would be independent evidence, and GitHub offers one. Not attempted, and the
cost is a second `test` leg rather than anything subtle.

**The pool's own gaps**, none of which is a defect:

- **The lost-wakeup window is argued, not tested.** A worker reads the
  submission count under the lock, searches once more, and only then sleeps
  while that count is unchanged; the second search is what closes the gap
  between the first search and the read. Making a test land inside that window
  needs an injection point the pool does not have. It is bounded rather than
  frightening: **waking a worker is throughput, never correctness** — the
  driving thread runs the chunks itself until they are gone, so a missed wakeup
  costs parallelism for one call and cannot hang it.
- **Nothing benchmarks the pool in isolation.** A harness that times the pass
  alone, and sweeps the chunk length, is what would let `STEER_CHUNK` be chosen
  rather than argued.
- **A mode comparison cannot catch a defect that is symmetric across modes**,
  and this was measured rather than assumed: dropping the last chunk of every
  `par_for` leaves both worker-count tests green, because a pool with no workers
  drops it too. Eight other horde tests go red on that mutation, which is what
  actually covers it — worth knowing before anyone reaches for the worker-count
  tests as a general correctness net.
- **One deque, not one per worker.** Only the driving thread pushes today, so a
  per-worker deque would be a queue nothing ever puts anything in. What needs
  them is `scope(|s| …)` fork-join (the design lists it for BVH build), where a
  running chunk spawns more work. Not written, and nothing calls it.
- **`Mutex` + `Condvar` for the sleep, in the frame path.** The design's rule is
  no mutexes in the frame path; this takes one per _submission_, not per job,
  and a worker takes it only on its way to sleep. A futex-style parking scheme
  would remove the lock, and needs the profiler to say whether it is worth the
  reasoning.
- **Considered and declined: aborting the remaining chunks when one panics.**
  Running them all instead lets the panic be reported by chunk index — the
  lowest wins — so a panicking `par_for` fails identically with and without
  threads.
- **A broken completion count hangs the suite rather than failing it.** Three
  mutations each wedge `par_for`'s wait loop instead of going red, because a
  chunk that never finishes is exactly what that loop waits for. A deadline in
  the wait loop would fix the symptom by putting a timeout in the frame path,
  which is a worse trade; the honest note is that this class of defect looks
  like a hang.
- **Considered and declined: `crossbeam-deque`.** It would be the right one, but
  neither it nor any `crossbeam-*` nor `rayon` is in `Cargo.lock`, and a new
  dependency is the user's call. Worth revisiting if one arrives for another
  reason: its growable deque would remove this one's capacity ceiling, past
  which `par_for` runs the extra chunks on the driver.

**`ring` does not implement drop-oldest**, though `21-jobs.md` lists it beside
drop-newest as an overflow policy. It cannot be done from the producer: the read
cursor belongs to the consumer, and a producer advancing it to make room would
be a second writer to it, which is exactly what makes an SPSC ring cheap. `push`
hands the item back and counts the refusal instead, leaving the policy to the
caller. If a real consumer turns up wanting drop-oldest, the honest options are
a consumer-side drain-and-discard or an MPSC design, not a flag on this one.

**The seam has one consumer, and one is not two.** `apps/horde` has used the
spawn seam without asking for anything back, which is evidence and not the two
samples this workspace's rule wants before a seam is frozen. `Spawn::threaded`
returning a `bool` rather than a richer capability is still the most likely
thing to give — horde works around it by handing `Pool::with_workers` an
`Inline` spawner when `--workers 0` is asked for, which is a caller saying
"threads, but none" through the only channel there is.

- **Cross-origin isolation is proved locally, and the worker backend is blocked
  on the pinned nightly's `rust-src`.** The isolation half is done:
  `web/tools/serve.mjs` sends `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp`, `web/build.sh --serve` runs it,
  and `run-browser-e2e.sh` refuses a run whose output does not contain the
  `crossOriginIsolated === true` check by name. Two cautions for whoever next
  touches the gate: the browser readback table in `web/run-browser-e2e.sh` was
  measured on Chromium 150 and this machine has 151, where the `Xvfb` +
  SwiftShader row no longer holds — **re-measure it before concluding the gate
  is broken**; and the Pages question is deliberately still open (GitHub Pages
  cannot set either header, `coi-serviceworker` is third-party JS) — if the shim
  is declined, the demos run single-threaded through `Inline` and the roadmap's
  `crossOriginIsolated` gate should be struck rather than left unmeetable.

- **Blocked 2026-08-06: the wasm worker backend needs `rust-src` on
  `nightly-2026-07-02`, which is not installed.** The plan stands — a nightly
  pinned by date for that one target, in the shape `decoder-fuzz` already uses,
  because `rust-toolchain.toml` pins an **exact stable** (`1.97.0`) on purpose
  and its own comment calls a floating channel a broken promise. Measured:
  `rustup component list --toolchain nightly-2026-07-02 --installed` lists
  `cargo`, `rust-std` and `rustc` and **not** `rust-src`, and the build fails
  with
  `library/Cargo.lock does not exist, unable to build with the standard library`.
  No backend was written against a toolchain that cannot build it:
  `default_spawner` still yields `Inline` on wasm, which is a whole answer
  rather than a stub.

  To unblock:
  `rustup component add rust-src --toolchain nightly-2026-07-02-x86_64-unknown-linux-gnu`,
  then re-check `21-jobs.md`'s finding 1 before building on it. Beyond the
  toolchain, three things are known to be in the way and are worth deciding
  before writing code:
  - **A wasm module cannot start its own worker** (`21-jobs.md` finding 2), so
    the backend is an `extern "C"` import the `web/` half implements — the same
    hand-written ABI shape `crcbl-audio`'s `web` module and `crcbl-store`'s OPFS
    path already use, and the reason no engine crate depends on `wasm-bindgen`.
    A `web-sys`/`js-sys` backend would be a new dependency in a wasm graph that
    currently has zero third-party crates, which is the user's call.
  - **The demos are built without atomics**, so the worker path cannot be
    exercised by the existing site at all. Proving it needs a threaded artifact
    the `DEMOS` loop in `web/build.sh` does not build, and
    `wasm-bindgen --target web` has to emit glue that accepts a shared
    `WebAssembly.Memory` — untested here against the pinned 0.2.126 CLI.
  - **The fallback has to be automatic and loud**, because every GitHub Pages
    visitor is in the non-isolated state. The observable to assert is the one
    `apps/horde`'s determinism tests already use — `pool.workers()` and two
    distinct thread ids — not "a worker backend was selected".

## What the scaffold's gate does not cover

`crcbl new`'s template now hosts `crcbl::engine::Loop`, and the scaffold e2e
compiles it, lints it, runs its three unit tests, runs it headless, and — since
the sway start-up moved into `crates/crcbl-shell/tests/sway-session.sh` — runs
it **windowed** against a private headless compositor, asserting the summary
reports the wayland shell at the size the template asks for. Against the null
backend by default and against lavapipe in CI (`CRCBL_CLI_E2E_BACKEND=vk`). What
is left:

- **Nobody has looked at the scaffold.** The windowed pass asserts the window is
  the size it asked for and that frames were presented; it does not assert what
  is _in_ them. Whether the pause menu, the fullscreen toggle and the debug
  panel look right in a generated project is still `cargo run` on a desktop,
  then `ESC`, `F11`, `F3`. Same class as every other "nothing has looked at it"
  entry in _Coverage gaps_.

- **Vulkan validation is not gated on it.** Run by hand against lavapipe with
  `CRCBL_VK_VALIDATION=1 CRCBL_VK_SYNC_VALIDATION=1`, the template's graph is
  clean — 30 frames, no layer messages. It is not in CI because a validation
  error only _logs_: `crcbl run` still exits zero, so the step would be a check
  that cannot fail. Gating it needs the scaffold e2e to read the child's stderr
  for layer messages, or `crcbl-vk` to grow a "fail on validation error" mode
  the sample harnesses could share.

Every combination of {born borderless, toggled with `F11`, imposed by the window
system} × {honoured, refused} is now executed by a harness on both Wayland
(`run-wayland-e2e.sh`) and X11 (`run-x11-e2e.sh`, with and without
`CRCBL_E2E_X11_WM`) — and the X11 F11 pass asserts the summary line's extent
after a clean `WM_DELETE_WINDOW` close as well as the engine's own mode line.
`crates/crcbl-shell/tests/bin/send_key_wayland.rs` and `send_key_x11.rs` are
what drive `F11` at a running sample from outside its process. What is still
uncovered:

- **The null GPU backend is excluded from every mode assertion, and correctly.**
  It presents by doing nothing, so no `wl_buffer` is attached, so the surface
  never maps: `swaymsg -t get_tree` lists no `app_id` for a null-backend run
  where a Vulkan one lists `sh.kryptic.crcbl.sandbox` — observed, not inferred.
  An unmapped surface gets no fullscreen configure, so any mode assertion there
  would be checking a window the compositor does not have.
- **`F11` is only pressed at the sandbox, never at one of the four games.** The
  games take the same engine-owned path (`crcbl::engine::FULLSCREEN_KEY`), but
  no harness presses the key at a running game — only at the sandbox, on both
  platforms.
- **macOS and Windows have shell backends now, and neither has a game-level mode
  pass.** P5C built both, and each has an end-to-end suite that opens a window,
  flips its mode and reports injected input — but that is the _shell_ being
  driven directly. Nobody presses `F11` at a running sample on either, the way
  `run-wayland-e2e.sh` does: Windows would need the key sender pointed at a game
  rather than at the suite's own window, and macOS has no renderer to run a game
  with until `crcbl-mtl` can present — permanently so, since the 2026-08-05
  decision makes Metal the only Apple path. See the platform sections below.

## The docs gate reads more files than any other, and it reads them on wasm32

CI runs `cargo doc --workspace --all-features` on the host **and**
`--target wasm32-unknown-unknown`, both under `RUSTDOCFLAGS: -D warnings`. Three
consequences that cost a round trip each if you do not know them:

- **An intra-doc link to an item that is `cfg`-ed out on the other target is an
  error there.** Write it as a code span instead. `#[cfg_attr]`-ing two versions
  of the sentence puts the same prose in two places and guarantees they drift.
- **`--all-features` builds Linux-only targets on every platform.** A
  feature-gated helper whose `use` resolves only on Linux compiles nowhere else.
  Give it a `#[cfg(not(target_os = "linux"))] fn main` that fails and says why,
  rather than a `cfg` that quietly compiles it to nothing —
  `crates/crcbl-shell/tests/bin/send_key_wayland.rs` is the worked example.
- **Rustdoc is the only gate that notices a public type nobody exported.** A
  `pub` field whose type is `pub` inside a private module is readable and
  unnameable: a consumer can get the value out and cannot write it down.
  `cargo clippy`, `cargo fmt` and the whole test suite pass straight through it,
  because nothing in the crate itself needs the path. Rustdoc reports it as
  `public documentation for X links to private item Y`, which reads like a
  formatting nit and is an unusable API. `RenderState::player_facing` and
  `RenderState::props` shipped that way and were caught only in CI.

None of the three is reachable from a local `cargo clippy --all-targets`, which
is what makes them worth writing down rather than rediscovering. **Run
`cargo doc` before pushing**, both targets, or CI will run it for you.

## Cross-test state, found by adding a window manager and a second monitor

**Both e2e suites run every test in its own process against one long-lived
display, and both had state that survived between them** — a _tail_ of tests
that passed alone and failed in a full run, moving whenever anything was
reordered. Three instances were found and fixed: the X11 pointer (`XTEST` leaves
it wherever the last test put it; `Session::open` parks it at the centre), the
X11 window manager's idea of what is still alive (`Session`'s `Drop` withdraws
and destroys its windows and then **waits for `_NET_CLIENT_LIST` to drop them**;
graded evidence, eight of eight runs clean against two and five for the earlier
attempts), and the Wayland focused workspace (a `FocusedWorkspace` guard puts it
back after a test fullscreens onto the second output).

**The rule that falls out: anything a test moves and does not move back belongs
in `Session`, not in the test.** The pointer, the input focus, the clipboard
owner, the focused workspace and the compositor's idea of which clients exist
are all this kind of thing.

Two blind alleys, recorded so they are not re-run. Neither is the fix and both
looked convincing: giving the `_NET_ACTIVE_WINDOW` message a real server
timestamp instead of `CurrentTime` (`Peer::server_time`, kept — it is correct
EWMH), and asking `openbox` less often or clicking the frame instead. The click
made it measurably _worse_: five runs, 3-5 failures each.

## What the Win32 backend has and has not been run against

The whole of `crates/crcbl-shell/src/win32/` and its e2e suite were written on a
Linux machine and are cross-checked with
`cargo check`/`cargo clippy --target x86_64-pc-windows-msvc`, which do not link
and do not run — **a cross-check proves the code typechecks and nothing more**.
The e2e suite has since run on `windows-latest` in CI (W1–W4 and the rounds
after), so the window lifecycle, input, clipboard and mode flips are executed;
everything below is what those runs still do not reach.

### The runner is a real, non-idle desktop

Any Windows test written from now on has to hold under all of this: the display
is **1024×768** — smaller than `WindowDesc::default`'s 1280×720; a cursor is
always over the window and keeps moving (a genuine `WM_MOUSEMOVE` arrives before
a test sends anything); the foreground is contested and `SetForegroundWindow` is
granted only under narrow rules — the e2e suite's `desktop::take_foreground`
pulls `SPI_SETFOREGROUNDLOCKTIMEOUT` plus `AttachThreadInput`; and **messages
arrive that this process did not cause**, every few milliseconds
(`WM_DWMNCRENDERINGCHANGED`, real `WM_MOUSEMOVE`), so an idle window with a
drained queue does not exist on that runner. The rule that cost three flaky
runs: **identify your own events by their payload, never by their index in the
sequence.**

### Unverified, in the order it would hurt

- **Structure layouts in `win32/ffi.rs` are asserted by size and offset, which
  catches a missing or wrong-width field but not a reordering of two same-width
  fields** — and a field whose width shrinks into its own trailing padding moves
  no offset and is not caught either (narrowing `DropFiles::p_files` from `u32`
  to `u16` leaves every assertion green). `DEVMODEW` is the one with two unions
  in it and the one to re-read if a refresh rate ever looks implausible;
  `RAWMOUSE` if a raw delta does.
- **No input has ever been delivered by a real device.** The suite drives the
  window procedure with `SendMessageW` — the real procedure against the real
  cached state, but not the real message stream. Nothing has confirmed that a
  real keyboard's `lParam` carries what `keys::scancode` expects, or that
  `GetMessageTime` answers for the message being dispatched.
- **`WM_INPUT` is untestable from CI and is untested.** A raw report needs an
  `HRAWINPUT` only the system can produce, so `input::read_raw_mouse` and the
  `RIM_TYPE_MOUSE` check have never run; the absolute path needs a machine that
  produces absolute reports (a remote-desktop session or a tablet). W4's
  `injected_motion_arrives_as_raw_relative_motion_for_mouselook` assumes
  `SendInput` feeds the raw stack on a `windows-latest` image — the single
  assertion most likely to be answered by the runner rather than by the backend;
  if it fails with the ordinary `PointerMotion` present and `raw_delta` absent,
  the finding is about the image, not the backend.
- **`ClipCursor` and `SetCursorPos` are restricted to the foreground process.**
  The two pointer tests call `SetForegroundWindow` first; if a GitHub runner
  refuses the foreground, both fail — a finding about the runner, not the
  backend.
- **`MapVirtualKeyW(.., MAPVK_VK_TO_CHAR)` is assumed to answer the uppercase
  letter**, which `input::unshifted` then lowercases so a rebind menu reads the
  same as on Linux; if the call already answers lowercase, the lowercasing is a
  no-op and nothing changes. The test asserts the lowercase result either way.
- **The `ShowCursor` balance is asserted through the count itself**
  (`cursor_display_count` reads it by moving it and putting it back). That test
  is the only thing standing between this backend and an invisible cursor for
  the rest of a session, so it is the one to keep rather than relax.
- **Auto-repeat is not the driver's.** Windows typematic comes from the
  keyboard, and an injected key does not repeat; the repeat test sends two
  presses and reads bit 30, which the _system_ sets — the same bit a real hold
  sets, so the claim is sound; what is untested is the driver's timing.
- **No file has ever been dragged onto a window.** The drop test builds a
  `DROPFILES` block by hand — this project's idea of what the shell sends, not
  the shell's, and a real drag needs a source application's mouse. If shell32
  rejects the block it reads exactly like a backend bug: `ffi::DropFiles`'s size
  assertion is the first thing to re-check, and `f_wide` the second.
  `DragAcceptFiles` and the `WS_EX_ACCEPTFILES` round trip are asserted through
  the style word; a _real_ drag being offered is a decision the shell makes in
  another process.
- **No other process has ever contended for the clipboard**, so `Opened::After`
  and `Opened::Refused` have never been produced and the retry loop itself is
  unexercised; only the budget arithmetic is covered, on Linux. The clipboard
  tests also share the desktop's clipboard — two Windows suites in parallel
  would interfere, which is why the e2e suite is `--test-threads 1`.
- **No sample-level pass, and there cannot be one yet.** The Linux suites run
  the sandbox and press F11 at it; that needs a renderer, and `windows-latest`
  has no Vulkan device, so this waits on a D3D12 path.

### Owed on the Win32 backend

- **Drag feedback: there is only a drop, never a conversation.** `WM_DROPFILES`
  is a notification; `DragEnter`/`DragOver`, a drop cursor, non-file formats and
  copy-versus-move all need `RegisterDragDrop` and an `IDropTarget` — COM, with
  `OleInitialize` on the pumping thread and an apartment this crate does not
  own. Considered and declined for W3 (it buys feedback rather than drops, and
  `ShellEvent::DroppedFile` is what the seam actually names); owed before the
  editor's asset browser wants a drop target that looks like one (P12).
- **`MimeType::UriList` is a registered format, not `CF_HDROP`.** A "copy file"
  from Explorer puts `CF_HDROP` on the clipboard, and this backend does not read
  it: a request for `text/uri-list` finds whatever was published under that
  registered name and is otherwise `Empty`. Rendering `CF_HDROP` as a
  `text/uri-list` blob means _encoding_ URIs, and the shared decoder
  `clipboard::parse_uri_list` cannot round-trip a Windows path (`file:///C:/a`
  decodes to `/C:/a`, which is not a file). Closing this means either a
  Windows-aware `file:` encoder plus a matching decoder, or delivering
  `CF_HDROP` as paths through a different route. Not attempted; named so the gap
  is not rediscovered as a bug.
- **A clipboard payload whose last bytes are NUL loses them.** Payloads are
  written NUL-terminated and read back with trailing NULs trimmed, which is what
  makes a `GlobalSize` larger than the request harmless — and what would
  truncate an `Other("image/png")` offer ending in NUL. Recorded in
  `win32::clipboard`'s module docs as well.
- **`WindowDesc::app_id` is validated and never applied.** Win32's equivalent of
  `WM_CLASS` is the Application User Model ID, set process-wide with
  `SetCurrentProcessExplicitAppUserModelID` from `shell32`; it decides taskbar
  grouping and which shortcut a window matches, and W1 rejects a NUL and
  otherwise ignores it, so a Crucible window groups under whatever Explorer
  infers. Wiring it means a third system library (`shell32` is already linked
  for the drop calls) and a decision about _where_ a process-wide property is
  set when a host application embedded the engine.
- **`ShellCaps::TEXT_IME` is clear although typing works.** `WM_CHAR` is
  handled, including surrogate pairs, and the default IME does deliver a
  committed CJK string through it — but nothing touches the `WM_IME_*` family,
  the seam cannot tell a pre-edit from a commit, and there is no way to place
  the candidate window at the caret. Matching Wayland's standard means handling
  the `WM_IME_*` family and giving the seam a pre-edit event, which is its own
  slice. The argument is written out in `Win32Shell::caps`.
- **`DeviceId` names a device kind, not a device.** Windows is better placed to
  fix this than X11 is — `RAWINPUTHEADER::hDevice` identifies the physical
  device on every `WM_INPUT` — but turning a handle into a stable `DeviceId`
  needs a handle table and a hotplug story, and raw input would have to become
  the source of button and wheel events too rather than only of motion.
- **A modal drag-resize accumulates raw motion.** `WM_INPUT` keeps arriving
  while Windows runs its own message loop, so a three-second edge drag delivers
  a few thousand `PointerMotion` events in one `pump`. Bounded, not a leak, and
  the two obvious fixes are both wrong: coalescing loses the per-event timing
  `docs/plan/19-input.md`'s pattern evaluator is a function of, and dropping
  needs a "we are inside a modal loop" flag nobody else would consume.
- **Refresh rate is a whole hertz, so 59.94 Hz reports as 60.**
  `EnumDisplaySettingsW`'s `DEVMODEW::dmDisplayFrequency` is an integer, and
  `MonitorInfo::refresh_millihertz` exists precisely because that rounding
  matters to frame pacing. The exact figure is in `QueryDisplayConfig`'s
  `DISPLAYCONFIG_RATIONAL` — worth closing now that frame pacing is real.
  **Closed 2026-08-07**: `win32::monitors` now reads the exact rate from
  `QueryDisplayConfig` (path walk → `DisplayConfigGetDeviceInfo` source name →
  the target mode's `vSyncFreq`, `vSyncFreqDivider` applied), falling back to
  the integer path when the walk cannot answer. The first CI run on
  `windows-latest` caught the virtual-display case: the runner's desktop reports
  a placeholder rational (1 mHz), which the exact path now refuses
  (`MIN_PLAUSIBLE_REFRESH_MHZ`, so the seam's documented "0 = cannot determine"
  is what such a display reports); the e2e's refresh band permits that zero.
  What is still unobserved: the exact rate of a _physical_ display — the
  `win32: exact refresh for …` info line is the only record of which path a
  machine took, since a broken walk silently falls back and every test stays
  green.
- **A window frozen during a user drag-resize is accepted, not fixed.** Windows
  runs its own modal loop between `WM_ENTERSIZEMOVE` and `WM_EXITSIZEMOVE`, so
  no frame renders until the mouse is released. The usual fix — `SetTimer` plus
  a frame rendered from `WM_TIMER` — cannot be built in this crate: the shell
  deliberately has no `Shell::run(closure)`. Closing it needs a second seam,
  "render one frame now", which is a decision above `crcbl-shell`.

## What the AppKit backend has and has not been run against

`crates/crcbl-shell/src/appkit/` was written on a Linux machine and
cross-checked with `cargo check`/`cargo clippy --target aarch64-apple-darwin`,
which do not link and do not run — **a cross-check proves the code typechecks
and nothing more**. The window lifecycle and the injected-input suite have since
run on `macos-latest` through `tests/appkit_session.rs`; everything below is
what that pass still does not reach.

### The one rule to know before writing any macOS test

**A `#[test]` can never drive an AppKit window, and that is measured rather than
assumed.** AppKit is main-thread-only and enforces it by raising
(`-[NSApplication nextEventMatchingMask:...]` throws
`NSInternalInconsistencyException`; an Objective-C exception unwinding through a
Rust frame is undefined behaviour), and Rust's `libtest` always runs a test body
on a thread it spawns — so the thread and app state a test needs are exactly
what `#[test]` does not supply (a green `#[test]` asserting every `NSCursor`
selector failed on the runner with `+[NSCursor "arrowCursor"] answered nil`).
The window suite therefore lives in
`crates/crcbl-shell/tests/appkit_session.rs`, a `harness = false` target that
owns its `main` and runs _as_ the process; it is not feature-gated (off macOS it
prints why it did nothing rather than reporting a pass it did not earn), and it
answers libtest's `--list` protocol before anything else — a `harness = false`
target has to be verified with `cargo nextest list` as well as
`cargo nextest run`, because `cargo test` does not enumerate and CI uses
nextest. A host `#[test]` is fine for the Objective-C runtime (thread-safe,
needs no application), CoreGraphics, and the pure modules; anything that creates
an AppKit object needs the session target.

What the session covers that is easy to miss: every `objc_msgSend` signature
shape this backend transmutes is dispatched against a class built at runtime
(`CrcblFfiProbe`) and against Foundation classes; the main-thread refusal is
exercised; every `NSCursor` selector `pointer::cursor_selector` names is checked
from the session; the pasteboard round-trips through a second process
(`pbcopy`/`pbpaste`, so text only — `application/x-crcbl+ron` is not
round-tripped cross-process, and if an engine-to-engine paste ever misbehaves on
macOS this is the gap it would hide in); and the pure modules (`geometry`,
`events`, `keys`, `pointer`, `TimeBase`) run on every host.

### Uncovered, and why each one is uncovered

- **`Borderless { monitor: Some(..) }` lands on the named screen's origin by
  construction and not by observation.** The runner has one display, so a
  backend that ignored the named monitor entirely would pass every assertion.
  Needs a two-display machine.
- **A window created borderless is untested.** The session creates its window
  windowed and flips; `create_native_window`'s borderless arm (placing the
  window with `initWithContentRect:` rather than `setFrame:display:`) has never
  run, and the presentation options are applied by `refresh_presentation` on the
  first `set_mode`, not at creation — whether that ordering matters for a window
  born borderless has not been measured. Re-read the ordering rule in
  `appkit::window`'s module docs before adding a test here.
- **`injection_skipped` is written and unrun**, because the runner granted
  activation. It stays for the case that produced it — a developer running this
  as a background process on their own machine — and prints the `Activation`
  evidence rather than a bare timeout if it is ever taken.
- **The harness, not the backend, asks for activation.** A GitHub macOS runner
  will not hand the foreground to the cooperative `-[NSApplication activate]`
  the backend is right to be limited to; `frontmost::ask` in the session uses
  `-[NSRunningApplication activateWithOptions:]` with
  `NSApplicationActivateIgnoringOtherApps`. That split — a harness may arrange a
  precondition a backend must never arrange for itself — is the same shape as
  Win32's `desktop::take_foreground`.
- **The sample-level F11 pass.** Needs a renderer, and macOS has no Vulkan at
  all — permanently, per the 2026-08-05 decision that Apple platforms are Metal
  only. It waits on `crcbl-mtl` reaching a swapchain, not on a gate.
- **A real drag and drop.** A drag needs a _source_ application with a mouse
  held down over a Finder item, which `CGEventPost` alone does not provide;
  `performDragOperation:` and the real `draggingLocation`/pasteboardItems
  conversion have never been watched. What M4 adds is that the registration is
  read back off the real window (`-[NSView registeredDraggedTypes]`), so the
  gate is a mechanism rather than a promise.
- **Only `public.file-url` is read.** `NSFilenamesPboardType` and
  `com.apple.pasteboard.promised-file-url` are not; the promised form needs the
  receiver to name a destination directory, which the seam has no way to ask
  for. Closing it is a seam question ("where should a promised drop land?"), to
  be answered once for every platform that has the concept.
- **macOS 15's pasteboard-access prompt has not been met.** It gates _reads_,
  not writes, and it does not turn a read into an error — but if a future runner
  image shows it, the session's `clipboard()` would block rather than fail,
  which reads as a hang. Recorded so a mysterious ten-second timeout in
  `paste()` is diagnosed rather than rediscovered.
- **IME composition is unverified.** The view conforms to `NSTextInputClient`
  and every key goes through `interpretKeyEvents:`, which is the structural
  standard Wayland is held to; that a Japanese input method actually commits
  through it is unverified, because a GitHub runner has no IME installed.
- **The horizontal scroll sign.** Vertical is settled; horizontal is passed
  through on the same reasoning without a trackpad to confirm it. If a
  two-finger swipe right turns out to scroll left, the fix is one negation in
  `pointer::scroll` and the test beside it.
- **Reference counting is reasoned, not observed.** `releasedWhenClosed` is
  turned off and `appkit::shell::release_window` is the single matching release
  for the window and the layer; there is no leak check anywhere in this
  workspace and Instruments is not in CI.
- **`AXIsProcessTrusted()` is not called**, although it would say outright
  whether TCC is the reason for a failed injection. It lives in
  `ApplicationServices`, which this crate does not link; if the first run's
  diagnosis is ever ambiguous, that is the next instrument to add. (TCC does
  **not** gate `CGEventPost` back to the posting process — settled by
  observation — so the `postEvent:atStart:` fallback should not be written.)
- **The `NSTrackingArea` is still a structural claim.** The posted pointer
  motion goes to the key window through `mouseMoved:` rather than through a
  tracking crossing, so nothing yet _requires_ the tracking area to have been
  registered.
- **Every type encoding on `CrcblView`'s methods.** The runtime reads them only
  when it forwards a method through an `NSInvocation`, which nothing in this
  crate does; a wrong one is a wrong-width read in a path CI never enters. The
  mitigation is that the encodings are written from one place (`ffi::ENC_RANGE`,
  `ENC_RECT`, `ENC_POINT`) rather than spelled out per method.

### Considered and declined

- **`PointerMode::Confined` is not implemented, and `POINTER_CONFINE` is
  clear.** macOS has no confine API; the only technique is warping back after
  the cursor has already crossed, which runs a frame late, fights the user's
  motion and manufactures events a consumer cannot tell from real ones.
  Approximating it would set a capability bit with no mechanism behind it. **Do
  not revisit without a public API to point at.**
- **`RAW_POINTER_MOTION` is set although the deltas are accelerated.** `NSEvent`
  deltas satisfy the half of that bit that decides whether a camera works and
  not the "unaccelerated" half; GLFW answers
  `glfwRawMouseMotionSupported() == false` on this platform for the same reason.
  Closing it properly means IOKit, a slice of its own.
- **`DeviceId` is a constant per device _kind_, as on X11 and Win32.** An
  `NSEvent`'s `deviceID` identifies a tablet and is meaningful only for the
  tablet family; the real answer is IOKit, the same slice as above.
- **The IME candidate window is placed at the window's origin, not at a caret.**
  The seam does not model a caret — nothing above `crcbl-shell` says where text
  is being typed. Closing it needs a seam addition ("the caret is here"), a
  decision above this crate to be taken once for every backend with an IME.
- **Lazy pasteboard provision (`pasteboard:provideDataForType:`) is not used,
  and it is structurally unavailable.** The callback arrives on the main run
  loop driven by the pasteboard server on behalf of a reader in another process
  — between two `Shell::pump`s an engine is rendering, so there is no run-loop
  turn to service it in — and a lazy owner must stay messageable until the
  flush, leaving the server holding an unretained pointer if the host process
  survives the shell. The same refusal `win32::clipboard` makes about
  `WM_RENDERFORMAT`. **Do not revisit without a seam that gives the shell a
  run-loop turn it owns.**
- **The engine's own format is published under its mime string, not a `dyn.*`
  UTI.** A pasteboard type is an arbitrary string, the mime is unique to this
  engine by construction, and it is byte-identical to what the other three
  backends name the same format with. Only text uses a system UTI.
- **Drag and drop _out_ is not implemented on any backend.** `15-windowing.md`
  scopes drag-and-drop to "file paths in"; `NSDraggingSource` is absent by plan
  decision rather than by gap.
- **No menu bar.** An unbundled Regular-policy application gets the system's
  default menu bar — enough to be focusable, not enough to ship (no ⌘Q).
  Building one is `NSMenu`/`NSMenuItem` and a decision about what belongs in it,
  which is above this crate.
- **`HW_UPSCALE` is clear although macOS has it.** A `CAMetalLayer`'s
  `drawableSize` is independent of its bounds, exactly what `wp_viewport` buys —
  but **the seam has no way to ask for it**. Setting the bit would be a claim
  with no mechanism behind it; closing it is a seam change (a render-scale
  request on `Shell`), a decision above this crate to be taken once for both
  backends.
- **`app_id` has nowhere to go.** macOS's equivalent is `CFBundleIdentifier` in
  an `Info.plist`, which cannot be set by a running process; the descriptor is
  validated for a NUL byte so a rejected descriptor is rejected here too, and is
  otherwise unused.
- **A live resize drag freezes the window**, on the same terms as the Win32
  modal loop and with the same unavailable fix.

## Not covered on either backend

- **A window manager that does not respect the requested size.** `openbox` with
  the packaged default theme decorates around the client area, so the client
  keeps the size it asked for. One that shrinks the client, or a tiling manager
  that ignores the request entirely, would break the sandbox passes' extent
  assertions — a real configuration, not exercised.
- **X11 multi-monitor.** The Wayland suite now declares two outputs and asserts
  that a fullscreen request naming the second one lands on the second one; the
  X11 suite still has a single `Xvfb` screen, so `move_to_monitor` and
  `Borderless { monitor: Some(..) }` are unit-tested only on the backend that
  can actually honour them. `Xvfb`'s RANDR exposes one CRTC and
  `xrandr --setmonitor` defines RANDR 1.5 _monitors_, which `crcbl-shell`'s
  enumeration does not read — it goes through `GetScreenResourcesCurrent` and
  `GetCrtcInfo`. Two ways forward: read `RRGetMonitors` first and fall back to
  CRTCs (what GTK and Qt do, and it makes the headless split testable), or run a
  real `Xorg` with the dummy driver configured for two heads in CI. The first is
  a backend change with its own slice; the second is a CI dependency.
- **Pixels.** Every display-mode assertion on both backends is a summary line, a
  log line, or the compositor's own tree. That a fullscreen frame is _composed_
  at the new extent, rather than merely built at it, is unchecked.

## Five sample `gpu.rs` files, two of them identical

`apps/breakout/src/gpu.rs` and `apps/flappy/src/gpu.rs` differ in **nothing**
but the game's name: rename `breakout`/`Breakout` to match and `diff` reports
zero lines. Both are 622 and 619 lines. `apps/asteroids` and `apps/horde` differ
substantially (352 and 487 lines against breakout's) and `apps/sandbox` almost
entirely, so this is a two-file duplication rather than a five-file one.

Not acted on because the seam is not obvious. The shared shape is "orthographic
camera + sprite pass + menu pass + UI pass over `GpuContext`", which is a
plausible `crcbl-render` bundle — but breakout's camera is fixed and flappy's
scrolls, and the two files agreeing today may be the coincidence of two 2D games
at the same stage rather than one piece of knowledge written twice. Revisit when
a third game wants the same bundle; a helper with two callers that then needs a
flag per caller is the failure mode.

## `run-vk-e2e.sh` pins no ICD by default, so nobody was running CI's gate

The script's header says it "is what a developer runs to see what CI sees". That
was not true unless the developer happened to set `CRCBL_VK_ICD`: the pin block
is wrapped in `if [ -n "${CRCBL_VK_ICD:-}" ]`, and with the variable unset the
script exports nothing and the Vulkan loader picks whatever is installed. On a
workstation that is the discrete GPU. Measured here: a bare
`crates/crcbl-vk/tests/run-vk-e2e.sh` reports
`adapter "AMD Radeon RX 7900 XTX (RADV NAVI31)"`, while CI's job sets
`CRCBL_VK_ICD=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json` and gets llvmpipe.
The suite printed the adapter it got all along, but an adapter line is not
something anyone reads looking for an absence.

The script now prints a warning naming the gap and the command that closes it,
and still runs — testing against real hardware deliberately is worth doing, and
this is how. **It is still not a hard failure**, which is a judgement worth
revisiting: the alternative is defaulting `CRCBL_VK_ICD` to lavapipe so the bare
invocation is CI's invocation, and requiring an explicit opt-out to use the real
GPU. That is probably the better default and was not taken here because it
changes what an existing command does.

## This machine's validation layer cannot see the two-submission hazard

`crcbl-vk`'s offscreen-ring write-after-read (fixed — see `CHANGELOG.md`) is
reported by CI's layer and by nothing here, and the difference is the **layer**,
not the ICD. Measured under CI's own ICD, so the ICD is ruled out:
`CRCBL_VK_ICD=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json` (which the script
resolves to Arch's `lvp_icd.json`), adapter
`llvmpipe (Mesa 26.1.6-arch1.1 (LLVM 22.1.8))`, layer
`VK_LAYER_KHRONOS_validation spec 1.4.357` — and
`synchronisation_validation_catches_a_missing_barrier` still reports
`record-time=yes one-submission=yes cross-submission=no`.

`syncval_submit_time_validation` **defaults to true** in that layer build (read
out of `/usr/share/vulkan/explicit_layer.d/VkLayer_khronos_validation.json`),
and setting it explicitly through `VK_LAYER_SETTINGS_PATH` and through the
`VK_KHRONOS_VALIDATION_*` environment form both left the measured reach
unchanged. So the switch is not the explanation and there is no known way to
widen it here. What that costs: the **two-submission** instance of this bug can
only be observed in CI.

What stands in for it locally is
`reusing_an_offscreen_ring_image_is_ordered_against_the_frame_that_had_it` in
`crates/crcbl-vk/tests/vk_e2e.rs`, which provokes the same missing dependency at
record-time distance — a one-image ring, both trips recorded into one command
buffer — where every layer build sees it. It was falsified by disabling the
widening in `VkCommandEncoder::pipeline_barrier`: red, with the layer naming
`vkCmdPipelineBarrier2 performs image layout transition on the VkImage ... which was previously read by vkCmdCopyImageToBuffer`.

**A CPU wait was tried first and does not work**, which is worth keeping because
it is a plausible idea. `acquire_next_frame` blocked on the retire timeline
before handing a reused image back. Instrumentation confirmed it ran (`reuse=3`
and `reuse=4` on the third and fourth frames of the failing test), and CI
reported the identical hazard anyway: a host-side wait establishes real ordering
but is not a queue dependency, and syncval reasons about submitted commands. It
also costs exactly the frame overlap the ring exists to provide. It was removed
rather than kept alongside the barrier.

**`crcbl-wgpu`'s offscreen path does not have this gap** — checked 2026-08-04,
and the untested assumption that used to sit here is deleted. Its acquire does
report `acquire_semaphore: None`, but the hazard needs the discarding transition
and that transition cannot reach wgpu: `WgpuCommandEncoder::pipeline_barrier` is
a no-op, so the `ResourceState::Undefined` the seam records is dropped at the
backend boundary, and wgpu-core inserts its own transitions from its usage
tracker — `command/transfer.rs`'s `transition_textures(&src_barrier)` before a
texture→buffer copy, and `device/queue.rs`'s
`insert_barriers_from_device_tracker` in front of each submitted command buffer,
which is what carries a texture's state across submissions (read in wgpu-core
30.0.0, the resolved version).

`reusing_an_offscreen_ring_image_is_ordered_against_the_frame_that_had_it` in
`crates/crcbl-wgpu/tests/wgpu_e2e.rs` is the check: a one-image ring, trip one
clears and copies out, trip two clears the same image to the reversed colour,
and the staging buffer must still hold trip one's. Green on radv, on lavapipe
and on the GL backend. Falsified both ways — writing trip two's colour in trip
one, and deleting the copy — each red, and each for its own reason.

**And the layer agrees, with a control that proves the layer was listening.**
Sync validation is not something wgpu-hal requests, so it was forced at layer
level: a settings file with `khronos_validation.validate_sync = true` reached
through `VK_LAYER_SETTINGS_PATH`, which makes the layer print
`Current Validation Enabled: … Synchronization` at `vkCreateInstance`. Under it
the wgpu test reports no hazard. The control is the same file and the same ICD
against `crcbl-vk` with the widening in `pipeline_barrier` disabled: red, with
`SYNC-HAZARD-WRITE-AFTER-READ … previously read by vkCmdCopyImageToBuffer`. So
the silence on the wgpu side is a verdict rather than an absence.

**Worth knowing before the next investigation**: `CRCBL_VK_SYNC_VALIDATION` is
what turns sync validation on for `crcbl-vk`, and with it unset the vk test
above stays green with the fix removed — the suite says
`CRCBL_VK_SYNC_VALIDATION is not set; skipping the sync-hazard probe`, which is
easy to run past. `run-vk-e2e.sh` defaults it to `1`; a raw
`cargo test`/test-binary invocation does not.

## The demo site's social preview is blank

No `og:image`, so every link posted to Slack, Discord or a Mastodon timeline
renders as a bare title and description. `web/templates/layout.html` is where
the tag would go and the site already carries `og:title`, `og:description`,
`og:type`, `og:url` and `og:site_name`, so the tag itself is one line — the
missing half is the image. It cannot be the `favicon.svg`: the platforms that
matter want a raster of about 1200×630, and several ignore SVG entirely.

Deliberately not invented here, because the choice is a design one: a rendered
frame of a demo, a wordmark on the site's own background, or a per-demo card.
Whatever it is has to be baked from a committed source the way
`crates/crcbl-render/assets/menu.crpix` is, not a PNG dropped into `web/` with
no way to regenerate it.

## `/favicon.ico` is still a 404, deliberately

`web/favicon.svg` is declared by the layout, which is what stops the browsers in
the requirements list (Chrome/Edge 113+, Safari 18+, Firefox) asking for
`/favicon.ico` at all — verified: `curl` against the live site returned 404 for
that path before the change, and the built pages now carry
`<link rel="icon" href="/favicon.svg">`. A browser that ignores the declaration
still gets a 404 and no icon.

Not fixed because an `.ico` is a binary blob and this repo bakes its art from
committed text. `web/build.sh` has no image toolchain and adding one for a 16×16
icon is a worse trade than the miss. `web/tools/browser-e2e.mjs` still filters
`favicon.ico` out of its 404 assertion for the same reason.

## Always run the browser gate with `--build`

A run without `--build` uses whatever is in `target/site`, and that directory
outlives the commit it was built from — a stale site is how a green run gets
reported for code that is red. This has cost real time once already: both demos
were signed off at "25/25" against a site built before the focus, pause and menu
work landed, and the gate had in fact been red the whole time. Pass `--build`,
or delete `target/site` first.

## Should the click that refocuses a canvas reach the game at all?

**Behaviour that surprised us, deliberately left alone.** A canvas has no title
bar, so `web/engine/shell.js` gives it the keyboard from its own `pointerdown`
handler — which makes the click that "clicks back into the window" also a press
at a real position inside the game. With the pause menu on screen and `RESUME`
under the cursor, clicking back in resumes. That is each half behaving correctly
and the combination being surprising; it is what put the browser gate at 23/25
for a slice, because section E clicked the canvas's _centre_ to restore focus
and the menu is centred there.

The alternative is click-to-focus **activation blocking**: the first press after
a focus gain restores focus and is swallowed rather than delivered, which is
what several desktop toolkits do. Not done, and not obviously right — swallowing
a click is its own surprise, and for a paused game the current behaviour is
arguably the friendlier one (the player clicked on `RESUME`; they got a resume).
It needs a decision rather than a patch, and it would have to be decided for
native and web together, since `Loop` cannot tell the two apart.

What holds the line meanwhile:
`a_focusing_click_off_every_button_leaves_the_game_paused` in all four games'
`app.rs` asserts the corner is over no button and the centre is over `RESUME`,
so a menu that grew until it reached the corner fails a fast Rust test rather
than the slow browser one. Four copies of it, plus horde's
`a_focusing_click_off_every_button_leaves_the_title_screen_up`, because the menu
geometry is per-sample even though `FOCUS_CLICK_INSET` — 8 pixels, in
`web/tools/browser-e2e.mjs` — is not. The loop around them is
`crcbl::engine::Loop` now, so this is one of the few things still written out
per sample, and it is per sample for a reason rather than by omission.

## The sprite system, and what is left of the retrofit

The pipeline is joined up end to end for both games: each of `apps/flappy` and
`apps/breakout` authors `.crpix` text under `assets/`, a `build.rs` bakes it,
and an `art::Scene` draws it through `SpriteRenderer` on a layer stack. What is
left:

- **The tick rate is one constant now, and the guard around it is still weak in
  three of the five.** `bake_dir` writes `ART_TICK_HZ` into the generated table,
  so the loader reads the rate the art was actually baked at and the `build.rs`
  value is its only source — the two halves cannot disagree, and the five
  hand-written copies beside the loaders are gone. What survives is the
  _conversion_ pair: a `.crpix` counts holds in ticks, a sidecar counts
  milliseconds, and each game's `the_art_bakes_to_the_sheets_it_declares`
  asserts an authored hold makes the round trip. **Breakout's, asteroids' and
  `crcbl-render`'s are weaker than flappy's and horde's**, because nothing they
  draw is animated: they can only assert the default hold of one tick, which
  survives a fairly wide range of wrong arithmetic. Asteroids' ship and rocks
  _turn_, which is a rotation applied to a still frame and not a clip, so it
  does not help. Each gets real the moment that game has a clip.

- **Breakout's paddle is a plain frame, not a nine-slice.**
  `game::PADDLE_HALF_WIDTH` is a `const` and nothing shadows it, so the paddle
  is 10 world units across on every tick of every run and a stretch would have
  had no caller. If a widening power-up is ever added, `assets/paddle.crpix`
  wants `nine: 12 12 0 0` and `art::paddle_rect` already produces the target
  rectangle `expand` would take.

- **`Sprite::rotation` has no pivot offset, and that was a decision.** The angle
  turns the quad about the centre of its own `rect` and there is no way to name
  another point. Considered and declined: the sheet lane has exactly one
  component left, an offset needs two, and the case it would serve is
  expressible without it — a rectangle rotated about an outside point is the
  same rectangle rotated about its own centre and then translated, so a caller
  wanting an orbit computes the translated `rect` and gets it exactly. Reopen it
  if something wants a pivot that is _animated_ independently of position, which
  is the one shape the translation trick makes awkward; it would need a fifth
  `float4` on the instance, taking it from 64 bytes to 80.

- **A nine-slice cannot be rotated, and neither can a menu or a button skin.**
  `NineQuads::sprites` hard-codes `rotation: 0.0` with a comment saying why: the
  nine quads are stretched against each other, so turning each about its own
  centre opens a gap at every band boundary, and turning the frame as a whole
  needs one pivot shared by all nine — which is a different feature from
  `Sprite::rotation`, and would be `expand`'s job rather than the instance's.
  Nothing has asked for it. If something does, the shape is a rotation on
  `NineSliceSource::expand`'s target that it applies to all nine quads about the
  target's centre, which needs the per-sprite pivot above or a rect-plus-angle
  that is not the rect's own centre.

- **The angle a sample interpolates is the sample's own, and the third one will
  copy it.** Asteroids answered the open question — `game::lerp_angle`, a
  shortest-arc scalar lerp between the previous tick's angle and this one,
  driven by `FrameClock::alpha` — but it answered it _in the sample_. Nothing in
  `crcbl-render` or `crcbl-client` offers it: `Client::interpolate` lerps
  `Transform`s, whose `DQuat` no sprite path reads, and `crcbl-phys` still has
  no angular velocity, so game code owns every angle. The next sample that turns
  something writes the same twenty lines. Worth promoting only when there is a
  second caller — the shape would be a `lerp_angle` in `crcbl-core`'s math, or a
  `Sprite` that could take the pair and the alpha, and the second is a much
  bigger claim than it looks.

## The debug overlay, and what is left of it

The modular panel is built and all three samples switch it on with F3 (or
`--debug-overlay`): `crcbl_ui::debug` owns `DebugPanel`, `DebugSection`,
`DebugModule`, `FrameStats` and `DebugOverlay`; `crcbl-render` contributes the
`gpu` section by implementing `DebugModule` for `FrameTimings`. What is left:

- **There is no network module, and no sample could show one yet.** The panel is
  ready for it — a module is a `DebugModule` impl and one `add` call — but
  nothing was written, for two reasons. The first is that `23-netcode.md`'s
  netgraph list (RTT, jitter, loss, send/recv bandwidth, snapshot size, resend
  counts, tick-lead) is **not measurable today**: `InMemoryTransport` has no
  timing, no loss and no byte accounting, and `Client` exposes only
  `is_connected`, `session_id`, `last_applied_tick`, `baseline_entity_count`,
  `baseline_system_count`, `processing_error_count`, `auth_failure_count` and
  `rate_limited_message_count`/`rate_limited_byte_count`. Those are real numbers
  and a module could show them, but they are a connection-health readout, not a
  netgraph, and shipping them under that name would make the P10 work look done.
  The second is placement: the module belongs in the crate that owns the numbers
  (`crcbl-client`, following `crcbl-render`'s example), which means a
  `crcbl-client → crcbl-ui` dependency. That is not obviously wrong — `crcbl-ui`
  depends on nothing but `glam` and `bytemuck`, so there is no cycle — but it is
  the first time a simulation crate would depend on the UI, and it is a call
  worth making deliberately rather than in passing. **What it would take**: the
  transport growing byte and timing counters, then a `DebugModule` impl beside
  them, then one `add` line in each sample that has a connection.
- **The overlay starts hidden in a release wasm build.** The default is
  `cfg!(debug_assertions)`, which is sample rule 4's "on by default in dev
  builds" taken literally; the demos on `crcbl.kryptic.sh` are release builds,
  so a visitor has to press F3. Whether the published demos should default it on
  is a product decision nobody has made. `web.rs` builds `Options::default()`,
  so turning it on there is one field.

## Coverage gaps

- **The mixer adoption was not verified by ear, and two of its choices are
  audible-only.** See the entry under _Owed_ above. Structurally everything is
  pinned; nothing has been listened to.

- **The `wasm32` audio path is not built by the local verification loop.**
  `AudioStream::open` on `wasm32` goes through `web::install`, and the blanket
  `impl AudioSource for Arc<T>` is what makes an `Arc<Mixer>` acceptable there
  too. The browser gate (`web/run-browser-e2e.sh --build`) covers the four demos
  end to end, which is the only place that path runs.

- **`html-validate` reports `require-sri` on every `<link>` and `<script>`, and
  it is being ignored.** Subresource Integrity guards a resource served by
  someone else; the stylesheet and the demo shims are same-origin files this
  repo builds in the same step as the page that names them, and a hash pinned in
  the layout would have to be regenerated on every edit to `style.css`. Recorded
  so the next person to run the validator does not re-litigate it.
- **No visual regression baseline for the site.** The browser gate captures the
  _canvas_ — deliberately, since the page's chrome is not what it tests — so a
  stylesheet or template change that breaks the layout around the canvas would
  pass all 26 checks. The screenshots taken during the 2026-08-02 audit were
  looked at by a human and thrown away.
- **The menu golden cannot see an inset larger than the one authored.**
  `menu_frame_two_sizes` compares the two panels' corner blocks pixel for pixel,
  which catches a corner that _scaled_ with the target — measured: making the
  panel's insets a function of the target width failed it. It cannot catch an
  inset that grew uniformly, because `menu.crpix`'s panel is uniform fill past
  texel 3 on both axes, so a `nine` of 6 draws exactly the same picture as a
  `nine` of 4. That number is pinned instead by
  `crcbl_render::menu::the_shipped_art_has_the_insets_the_layout_assumes` and by
  the layout tests, both of which go red on it.
- **The golden's reference is weak on small-area art changes.** The image is
  416×576, so recolouring a one-texel band moves under 1% of the pixels and
  compares inside `Tolerance::RASTERISER` — measured: swapping the panel's
  shadow colour for its own channels reversed passed the reference. The pixel
  assertions carry that weight instead (`assert_menu_pixels` samples the
  highlight and the shadow on all four edges, and that _did_ catch it). Worth
  knowing before adding a claim to this golden that only the reference would
  hold.
- **Nothing has looked at a menu over a real game.** The golden renders the menu
  over a flat clear colour on an offscreen ring; no test composites one over
  breakout's brick grid or flappy's course, and no human has confirmed the scrim
  reads well over either. The browser gate reaches a paused demo but only counts
  HUD lines. The browser gate's canvas capture is the closest thing there is,
  and it happens to fire while the pause menu is up — a human has now looked at
  it for both demos and it reads fine, but nothing asserts it.
- **No test and no tool captures a native sample's pixels.** `breakout` and
  `flappy` take `--headless --frames N` and print a summary; neither has a
  screenshot path, so "the Vulkan build of the game draws the right picture" is
  reachable only by running it on a desktop. The multi-sheet bug lived in the
  shipped samples for that reason: the evidence that it is gone is
  `every_batch_draws_its_own_instances_rather_than_the_first_batchs` exercising
  the same `SpriteRenderer` on radv, plus both samples running 120 headless
  frames with `CRCBL_VK_VALIDATION` and sync validation clean — **not** a
  picture of either game. What it would take: a `--capture <path>` on the sample
  front ends, reading the swapchain image back the way `vk_e2e.rs`'s
  `render_sprites` does.
- **Flappy's swept-sphere collision is exercised, not demonstrated.**
  `game::fatal` sweeps the bird's path with `PhysicsSystem::sweep_sphere`
  because that is the correct query, but at this game's speeds a point test at
  the end of the tick catches every pipe the swept one does — measured, down to
  a tick rate of 3 Hz, where the bird still ends each tick inside the pipe it
  would have tunnelled through. Tunnelling needs a step wider than a pipe plus a
  bird (2.3 units, so under about 2.6 Hz), which is not a rate this game is
  coherent at. Closing it honestly means a faster consumer, not a contrived test
  here.
- **No golden image covers the play field's framing, in either sample.** The
  browser gate reads the canvas back and asserts it is neither blank nor still.
  Flappy's `gpu::the_playable_band_is_on_screen_at_every_aspect_ratio` now puts
  the world through the real view-projection at five aspect ratios, which is
  stronger than the hand-written mapping it replaced, and is still not a pixel
  check that would catch the framing drifting.
- **The overlay has never been looked at.** Every test over it is draw-list
  strings and rectangles; no golden image, and no human has confirmed the panel
  is legible over a lit scene or a bright sprite background. The layout maths
  (value column past the longest label, panel inside the screen at two sizes) is
  asserted; the _appearance_ is not.
- **Nothing has looked at either sample's art come out of a GPU.** Every test
  over it is `Sheet` data, sprite rectangles and layer membership; the pictures
  in both retrofit reports were composited in software from the same sprite
  lists, so they say the scenes are assembled correctly and nothing about the
  shader, the sampler or the blend. `crcbl screenshot` cannot help — it renders
  the sandbox cube through `ForwardRenderer`, which **neither** sample uses now.
  Closing this means either a golden through the sprite pass with a sample's own
  sheets, or an offscreen path the samples can drive. It is the same gap for
  breakout as for flappy and is not worth two entries.
- **Nobody has _looked_ at a fullscreen window.** The mechanism is gated end to
  end now — see _Display-mode coverage_ — but every assertion is a summary line
  or a compositor's tree, not a picture. That the frame is composed correctly at
  the new extent, rather than merely built at it, is unchecked, and is the same
  gap as every other "nothing has looked at it" entry here.
- **The browser's fullscreen path has never been exercised.**
  `web/tools/browser-e2e.mjs` covers focus and pause in a real browser — it
  blurs the canvas, checks the status becomes `STATUS_PAUSED` and that the HUD
  heartbeat stops, and that Escape brings it back — but not F11. Headless
  Chromium's fullscreen behaviour under Xvfb is its own question, and
  `Input.dispatchKeyEvent` is not a user gesture for the purposes of
  `requestFullscreen`, so the check would need `Browser.setPermission` or a
  headed run to mean anything. What is covered instead is the seam:
  `crcbl-shell`'s `fullscreen_is_the_pages_answer_not_the_engines_request` and
  `the_mode_is_right_whichever_order_the_resize_and_the_change_arrive_in` drive
  `__crcbl_web_fullscreen` directly, and `check-exports.mjs` confirms the symbol
  ships and that the shim calls it.
- **Nothing has looked at the pause menu.** It is asserted as draw-list strings
  — the `PAUSED` heading and the first hint line reaching the UI pass — and no
  human or golden image has confirmed it is legible over either game's art, or
  that the full-frame dim reads as a dim rather than as a bug. It is
  deliberately the crudest possible version, behind `draw_pause_menu`, because
  the next slice replaces it.
- **The changelog starts mid-project.** `CHANGELOG.md` covers changes from
  2026-08-01 onward; everything before it is in `git log` only. Worth doing at
  the first tagged release, or not at all — there are no releases yet for a
  reader to be missing entries from.

## What `crcbl-phys` owes, found by writing asteroids

`apps/asteroids` is the P6 physics slice's first consumer. Two of the questions
P6 left open are answered below and no longer open; the rest are what building
against the crate turned up.

**Answered — the wrap's broadphase rule.** P6 left "when is a move a teleport"
to whoever wrote the wrap. Asteroids' `teleport` (in
`apps/asteroids/src/game.rs`) chose: **a wrap is a teleport, and a teleport is a
remove-and-re-insert**, done by calling `PhysicsSystem::set_collider` again,
applied uniformly to everything in the broadphase with no distance threshold —
"did the position change discontinuously" is what a wrap knows and a threshold
would only guess at it.

**Corrected — the wrap is not a correctness bug.** The old entry here said a
teleported body "leaves its ancestors' bounds stretched across the whole field",
implying collisions break. They do not, and this was checked by falsification:
swapping `set_collider` for `set_transform` in that function leaves the whole of
`apps/asteroids`' 49-test suite green, because `Bvh::update_aabb` refits every
ancestor on the way to the root and a stretched ancestor is a conservative
_superset_ — bigger than it should be, never smaller, so it prunes nothing. What
it costs is **tree quality**, not answers.

- **The per-entity routes exist now, and `PhysicsSystem` still has no per-entity
  providers.** `DampingForce::world_force(velocity, mass, dt)` and
  `DragForce::world_force(velocity)` joined `ThrustForce::world_force`, and
  asteroids' hand-rolled `damping_force` — `-k·v` plus the `mass/dt` clamp,
  written out because a provider is global — is deleted. What was **not** taken
  is the other option that entry named: letting `PhysicsSystem` hold providers
  that apply to one entity rather than to every body. Three `world_force`
  methods is the cheaper shape and it stops being so the moment a fourth force
  wants one, or a game wants several forces on one entity and has to call each
  by hand every tick.

  A trap worth keeping, found while testing it: `apply` **delegates** to
  `world_force`, so a test asserting the two agree cannot fail — deleting the
  cap from `world_force` left it green. The test asserts the model written out
  (`-velocity * min(k, m/dt)`) and, in the capped regime, that one step lands
  the velocity exactly on zero. Measured: that version goes red.

- **There is no "what does entity E overlap" query.**
  `PhysicsSystem::overlap_sphere` takes a free centre and radius. An entity that
  is only ever the _subject_ of overlap tests therefore has no reason to be in
  the broadphase at all — asteroids' ship carries no collider, because a leaf no
  query is allowed to return would have needed filtering back out of every
  result by entity id. That is fine here and will not be for a game where two
  things test against each other. What is wanted is an entity-shaped overlap
  with an exclusion list; the same exclusion list is what `sweep_sphere` needs
  and what breakout and flappy both work around by removing the sweeper's own
  collider and putting it back.

- **`PhysicsSystem::overlap_sphere` still fabricates its `ShapeHit`.** `t: 0.0`,
  `normal: DVec3::Y`, `started_inside: true` for every result. Asteroids only
  asks _whether_ anything is there, so it discards the hit outright — which
  means the type is promising a contact that no caller in the workspace can use.
  Either compute a real deepest-point normal or change the return type to entity
  ids. `PhysicsWorld::overlap_sphere` underneath is honest.

- **Rotational dynamics are absent.** `Transform` carries a `DQuat` and
  `ThrustForce` reads it, but there is no angular velocity, no torque and no
  quaternion integration: `RigidBody` has `velocity` and `force_accum` and
  nothing angular. Asteroids' ship integrates its own heading in `turn_ship` and
  writes it through `set_transform`. That is right for this game — a turn rate
  is a constant, not a physical response — and wrong for the inertia tensor the
  design doc describes. Whoever needs real torque adds `angular_velocity`,
  `torque_accum` and an inertia term to `RigidBody` and a rotation step to
  `SemiImplicitEuler`.

- **No benchmark, and no rebuild policy.** Churn cost was measured as tree
  _depth_, not as time: the claim "insert/remove beats a rebuild" is an
  algorithmic one (one root-to-leaf path against `O(n log n)`) and was not
  timed. There is also no policy that ever rebuilds a churned tree — the AVL
  bound is what makes one unnecessary, but a bulk build still produces a tighter
  tree by surface area than incremental insertion does, and nobody has measured
  the query-cost difference between the two. The horde sample (P8, 10k bodies)
  is where that stops being academic.

## `GameModule::tick` runs after the ECS sweep, so a game's destructions lag

`crcbl_ecs::World::tick` runs the schedule and then `sweep`s the deferred
destruction queue. `crcbl_server::Server::tick` calls `world.tick()` **and
then** `module.tick(&mut world)`. So every entity a `GameModule` despawns sits
in the pool for one more tick before the pool lets go of it — and a game reading
`World::entity_count()` between ticks sees a count that is high by however many
things died last tick.

Found by asteroids, whose leak test compares `entity_count()` against
`1 + rocks + bullets` on every tick and failed immediately; it now adds
`World::dead_queue_len()` to the sum. `apps/flappy`'s equivalent test asserts a
`<=` ceiling, which tolerated this without noticing it.

Two possible fixes, and the choice is the engine's:

- **Sweep after the module**, i.e. `Server::tick` calls `world.sweep()` between
  the module and `emit_snapshot`. Arguably more correct anyway: today's snapshot
  is emitted while entities the module destroyed are still in the pool.
- **Leave it and document it**, and have `World::entity_count` grow a sibling
  that excludes the queue, so a consumer is not obliged to know.

Not worked around in asteroids beyond making the test honest. **Horde hits it
harder**: its leak invariant is checked on every tick of a soak, and the queue
is non-empty on any tick something died — which at a hundred spawns a second is
most of them.

## What `crcbl-phys` owes at scale, found by writing horde

`apps/horde` runs `N` broadphase overlap queries per tick — one per enemy, for
separation — plus one for contact damage and one for aiming, so it is the first
consumer where the _per-query overhead_ rather than the query's answer is the
cost. Provisional numbers and their conditions are in
`docs/plan/sample/03-horde.md`; both entries below sit in front of them and
neither was taken, because either is an API change to `crcbl-phys` and this
slice was the sample.

- **The allocations are gone and nobody has measured what they cost.**
  `overlap_sphere_into` runs through all three layers now, and the pass
  allocates nothing once its buffers have grown: `steer_enemies` keeps one
  `QueryScratch` and one neighbour list per thread in a `thread_local!`, which
  is what a `par_for` closure can reach, and the `&mut self` callers still use
  the `QueryScratch` on `PhysicsSystem` and `PhysicsWorld`. It used to be
  **three** `Vec`s per enemy per tick — `overlap_sphere`'s own,
  `PhysicsWorld::overlap_sphere`'s, and `Bvh::traverse_aabb`'s — which at the
  plan's ten thousand is 1.8 million allocations a second, every one dropped
  immediately.

  **What is not known is whether it mattered.** No before/after number exists,
  for the reason under _What horde still owes_: ten thousand enemies kill the
  player in under a second, so a wall-clock run measures a simulation that has
  stopped, and this repository has no allocation counter and no benchmark
  harness. The change is justified by the count, not by a measurement, and
  anybody quoting it as a speed-up is quoting something nobody ran.

- **A broadphase query costs what its _answer_ costs, so the tick's cost tracks
  local density rather than entity count.** The same ten thousand enemies cost
  14.66 ms a tick spread over the arena and 84.09 ms after eight seconds of
  converging on the player — measured, both columns, in
  `docs/plan/sample/03-horde.md`. This is not a complaint about `crcbl-phys`; it
  is the fact any budget stated in "N agents" is wrong about, and it is why
  18a's provisional 8–9k figure was both too optimistic (it never let the crowd
  converge) and taken on a fixture that at ten thousand described a field larger
  than the arena. **Anything that quotes a per-agent cost for this crate has to
  say what the neighbourhoods looked like.**

- **The neighbour sum's order is the BVH's traversal order, and horde chose to
  live with it.** Floating-point addition is not associative, so the separation
  vector depends on the order `overlap_sphere` returns neighbours in. That order
  is deterministic — the tree is a pure function of the insert/remove sequence,
  which is a pure function of the seed and the script, and
  `the_same_script_replays_bit_identically` covers it — but it is deterministic
  _because of the tree_, not independently of it. Sorting each neighbourhood by
  entity id would make it independent, at the price of a sort per enemy per
  tick. **Declined** for that reason. It becomes a real question the moment the
  tree's build order stops being reproducible — a parallel insert, or a rebuild
  policy that depends on timing — and whoever adds either should read this
  first.

## What horde still owes

S3 is done — the core loop, the art and progression, and now audio, the longest
run, the browser demo and the scale measurement. `docs/plan/sample/03-horde.md`
carries the numbers and their conditions; this is what was raised and not
finished. Entries the measurement closed have been deleted rather than
annotated.

- **`--prefill` starts its own run, and that coupling is not obvious.**
  `assemble` in `apps/horde/src/app.rs` queues a start edge when
  `options.prefill > 0`, because the scale fixture would otherwise measure a
  `run_tick` that returns on its second line. It is one call beside
  `Game::stage_field` and `a_prefilled_run_does_not_wait_at_the_title_screen`
  holds it. Anything else that stages a board before the first frame — a replay
  header, a future demo mode — has to do the same or it will measure nothing and
  say it measured everything.

- **The plan's exit criteria are internally inconsistent and need rewriting, not
  answering.** "10 000 enemies at 60 Hz tick" is true of a crowd spread over the
  arena (14.66 ms of a 16.67 ms budget) and false of the same crowd converged on
  the player (84.09 ms). The difference is a factor of 5.7 at a fixed count,
  because separation is a broadphase query whose cost is the size of its answer;
  a horde converges by construction, so the second number is the one the game
  spends its time at. Whoever owns the criterion has to say **which crowd**.

- **"Playable and mildly fun for 5 minutes" cannot be true of this arena at the
  plan's count.** Ten thousand enemies in 96 × 72 units is 0.82 units apart,
  several inside `PLAYER_RADIUS` on frame zero, and contact damage is a rate
  summed over everything touching — so `--prefill 5000` and above kills the
  player in under a second. A default run, spawner only, dies at about 24
  seconds with 46 things on the field. Two ways out and neither is obviously
  right: **a bigger arena** (the density falls as the area grows, and the follow
  camera and `clamp_to_arena` already handle any size, but a 300 × 225 arena
  changes what the game _is_ — the walls are what makes kiting finite), or
  **admit the count is a benchmark target** and let the exit criterion carry two
  numbers, one for the budget and one for the game. Nobody has decided.

- **The CPU cull is still per-sprite and still `N` per frame**, and it is now
  measured: 28 µs at ten thousand, of a 16.67 ms budget. That is the work P7's
  GPU culling exists to delete, and the measurement says deleting it is worth
  0.17 % of a frame to this sample. Keep it as the reason P7 exists for _other_
  scenes; it is not the reason it exists for this one.

- **`crcbl-audio` has no voice limit, no priority and no stealing.** Still true
  after the mixer-adoption slice, which deliberately left it there.
  `apps/horde/src/audio.rs` caps itself at `MAX_VOICES` = 16 and refuses the
  newest voice, counting refusals in `Audio::dropped()`. Refusing the newest is
  the crudest answer that is honest and it is audibly wrong in one case: a
  player's _death_ cue can be refused by sixteen kill cues raised on the same
  tick. Wanted in the crate: a voice budget with a priority, so an important cue
  steals the oldest cheap one. The refusal count is on the debug panel now (the
  `audio` section's `dropped` row), so the pressure is visible while the
  crate-level budget stays undecided.

  One detail changed with the adoption and is worth knowing before anyone moves
  the cap into the crate: horde now reads `Mixer::voice_count` and then calls
  `Mixer::play`, two lock acquisitions where the hand-rolled queue held one
  across both. Only the game thread adds and only the audio thread removes, so
  the count can be stale **low** and never stale high — the cap can refuse a cue
  that had just been made room for, and can never let the count past
  `MAX_VOICES`. A cap inside the crate would not need the two-step at all.

- **Nothing has listened to the five cues**, on any device. They are synthesised
  deterministically from a fixed seed, so a golden buffer is possible and there
  is not one. What the tests assert is that each cue fires, that it carries the
  position of the thing that raised it, that the listener is the player rather
  than the origin, and that the level cue actually sweeps in pitch. No test can
  tell a good kill sound from a bad one.

- **The HUD line can still outgrow its backdrop at extreme settings.**
  `the_hud_fits_the_panel_it_is_drawn_on` measures both lines through the real
  `FontAtlas` at a stated worst case — a five-minute run at the shipped enemy
  cap, level 18, 2 048 kills — and requires them inside `HUD_PANEL_RIGHT`. It
  does **not** bound `--max-enemies 10000` with a twenty-minute soak behind it:
  five-digit fields are wider than the panel and the text would run off the end
  of it, which is what the browser gate's canvas capture caught the last time
  the panel was too narrow. A real fix is a HUD that measures itself and sizes
  its own backdrop, which is a `crcbl-ui` widget rather than a `DrawList::rect`.

- **Nothing enforces that the arena is a plane.** Positions are `DVec3`,
  everything the game produces sits at `z = 0`, and `clamp_to_arena` passes `z`
  through untouched. A body given a non-zero `z` would separate in depth and
  never be brought back — which a test fixture using `DVec3::splat` did, and
  which is how this was noticed. Either clamp `z` too or make the fact a type.
  Not a live bug: no production path can produce one.

- **The horde does not avoid the walls, it is pushed into them.** Seek is a
  straight line to the player and separation knows nothing about the arena, so a
  crowd chasing a player along an edge piles into it and the clamp holds it
  there. It reads acceptably and it is not pathfinding — which is a hard
  non-goal — but "walk around the obstacle" is the first thing a player will
  expect if props ever land in the arena.

- **Contact damage has no invulnerability frames**, by choice: it is a damage
  _rate_ summed over whatever is touching, so a stack of enemies is worse than
  one and there is no per-enemy timer on the hot path. The consequence is now
  measured rather than predicted — see the density entry above — and it is what
  makes the plan's count unplayable in this arena.

- **Nothing pulls a gem towards the player.** `Upgrade::Magnet` widens the
  radius the collection query runs at, so a gem inside it is banked on the tick
  it comes into range and one outside it is not. The genre's version drifts the
  gem to the player, which reads far better and which would be `P` steering
  updates a tick on top of the `N` the sample measures. Left out deliberately:
  the point of this slice's pool is that each upgrade is one line.

- **A gem that is never collected is lost, silently.** `MAX_PICKUPS` is 512 and
  a kill on a full field drops nothing; `Game::pickups_dropped` counts the
  refusals and nothing shows them. It is bounded and deterministic, which is
  what it was for, and a player kiting away from a heap of loot in a long run
  will not be told why their level-ups stopped. A HUD line, or dropping the
  _oldest_ gem instead of refusing the newest, would both fix it. **A potion is
  refused by the same ceiling**, which is worse than losing a gem — the rarest
  drop in the game can be eaten by a field of litter and the player is told
  nothing. `drop_pickup` says why it is not special-cased: a kind that could
  jump the queue is an unbounded population wearing a bound. Dropping the oldest
  gem would fix this half too, and is the option to weigh first.

- **Nothing on screen tells a potion from a gem before you reach it, except the
  picture.** There is no minimap, no pickup outline and no HUD line;
  `art::tests::a_potion_is_not_a_gem` is what says the two silhouettes are
  distinguishable at all, and it measures the baked frames rather than what a
  player at a distance can resolve. Nobody has looked at a field of both on a
  real display. The claim being made is about shape, not colour, precisely
  because red-against-green is the one pair a large minority of players cannot
  use — but that reasoning has not been checked against a simulated deficiency
  either.

- **A potion always lands beside a gem, and both are taken together.** A brute
  drops both, one pickup diameter apart, so a player who walks over one almost
  always takes the other in the same tick — the collection radius is wider than
  the gap. Nothing is lost by that (both pay out) and it does mean the potion is
  never a separate decision to walk to, which is half of what a rare pickup is
  for. Placing it further out would need a bound nobody has stated against
  `clamp_to_arena` and against a brute dying in a corner.

- **`POTION_DROP_CHANCE` was tuned against the autopilot, not against a
  player.** The kiting `autopilot` in `game::tests` walks a fixed circle and
  takes steady chip damage, which is not how a run is actually lost; the rate
  was settled by finding where `a_long_run_leaks_nothing` stops reaching a death
  (one brute in ten survives the whole soak on single-figure hit points, one in
  twenty dies and restarts). That makes the number defensible and not the same
  as playtested. Nobody has played a run with potions in it.

- **The level-up screen has no way out but forwards.** There is no "skip", and a
  choice out of range is ignored, so a run that reached `LevelUp` stays there
  until one of the three digits is pressed. The loop's Escape still pauses over
  it and the death menu cannot be reached from it — nothing can kill the player
  while the field is frozen, so this is not a soft-lock, but it does mean a
  browser demo left on the level-up screen looks stopped. `browser-e2e.mjs`
  watches the once-a-second `[HUD]` heartbeat, which keeps firing, so the gate
  itself is fine. Headless runs reach past the screen with `--choose <N>`, which
  is what took the potion-drop measurements out of `game::tests`.

- **The upgrade pool is repeatable without limit.** `RapidFire` has a floor
  (`FIRE_COOLDOWN_FLOOR`) and the other five do not, so a very long run has an
  unbounded weapon range, walk speed and hit-point ceiling. It is a five-minute
  game and nobody has played it for twenty; caps are a balance decision, not a
  bug, and they are not there.

- **Enemies do not turn to face anything.** Every silhouette is deliberately
  non-directional — a lump, a four-legged X, a horned slab — so no sprite
  rotation is needed and no `atan2` runs per enemy per frame. It is the right
  trade at 10k and it does mean the crowd has no sense of heading. The _player_
  turns, and it does it by reversing the frame's `u` range (`art::mirrored`)
  rather than by rotating — which would cost nothing per enemy either, if a
  future enemy ever wants a heading.

- **The wizard's walk cycle has never been watched.** `art::mirrored`'s reversal
  is now rasterised — `crates/crcbl-vk/tests/vk_e2e/sprite/mirror.rs` renders a
  frame and its mirror and compares the two images column-reversed, bit-exact on
  radv, which is the shader-side evidence the older entry asked for. What is
  still unverified is the _animation_: nobody has seen the walk cycle play in a
  running window, and the browser gate's canvas capture that a human looked at
  predates the flip.

- **A wizard walking into a wall keeps walking on the spot.**
  `RenderState::player_walking` is the intent, not the velocity after
  `clamp_to_arena`, so a player holding a direction against the arena edge
  animates while going nowhere. Deliberate — it is what the player is doing, and
  taking it from the velocity would make the wizard freeze mid-stride against
  every wall — but it is the one place the animation and the movement disagree,
  and it is worth knowing before someone "fixes" it.

- **Not measured, not reviewed: the windowed native path.** It is compiled and
  never run — there is no display in this environment — so the follow camera,
  the sprite pass, the three menus and the HUD layout have been checked by test,
  by argument, and now by a **browser**: the gate's canvas capture at 26/26 is a
  picture of the real game, and a human has looked at it. What has still never
  been seen is the _native_ window, and the fullscreen toggle against a real
  compositor is the same gap the other three samples carry.

- **Every scale number was taken on an offscreen image ring, not a swapchain.**
  `--headless` gives `crcbl-vk` a `SurfaceTarget::Offscreen` rotation of images,
  which exists precisely so that it is the same acquire/record/submit/present
  path — but it is not a windowed present, it is not vsynced, and it is 960
  × 720. A windowed 1440p run would raise the sprite pass's fill by about four
  times, which on a 0.023 ms pass is still nothing, and nobody has taken it.

- **There is no Tier B / browser scale number.** The exit criteria ask for one
  ("Tier B/wasm gets its own smaller recorded budget") and the only browser this
  repository can drive is Chromium's SwiftShader under Xvfb, which measures a
  software rasteriser. It needs a machine with a real browser GPU and a way to
  read `PassTimers` out of a wasm build — the second of which does not exist:
  the demo has no way to report its frame timings to the page.

- **Nothing checks that a `.crpix` texel lands on a whole screen pixel.** At
  `TEXELS_PER_UNIT` = 20 and a 720-pixel-high view of 28 world units, one texel
  is 1.286 screen pixels, so `SampleMode::Pixel`'s nearest sampling drops and
  doubles rows as the camera moves. Every sample has this and none of them
  addresses it; the fix is an integer-scaled render target, which is a renderer
  feature nobody has asked for.

## What asteroids itself still owes

S2 is done — simulation, art, audio, persistence and the browser demo. What is
not:

- **No golden buffer for the cues.** The three sounds are synthesised
  deterministically — `audio::noise` runs splitmix64 from a fixed seed — so a
  golden buffer is _possible_, and there is not one. What the tests assert is
  that each cue fires, that it carries the position of the thing that raised it,
  and that the explosion decays and is not a tone. Nobody has listened to the
  result on a real device and no test can tell a good explosion from a bad one.
- **The 10-minute soak in the exit criteria was not run.** What runs in CI is
  `hundreds_of_spawns_and_deaths_leak_nothing`: 18,000 ticks (five minutes of
  simulated play), 337 rocks spawned, 1,221 bullets fired, six waves cleared,
  checking the entity and collider accounting on **every** tick. Ten minutes of
  wall-clock soak with the inspector open, and the "no stale-handle panics with
  entities selected as they die" criterion, both need the entity inspector,
  which this sample does not use yet.
- **The overlap query does not know about the seam.** Ship-versus-rock is a
  single `overlap_sphere` at the ship's position, so a ship straddling an edge
  does not see a rock straddling the opposite one until one of them has wrapped.
  A full answer queries up to four offset positions. Deliberately not done: it
  costs four broadphase queries a tick to fix a one-tick artefact at a boundary
  both bodies cross constantly, and no test could tell the difference without
  being written to.
- **No golden image covers a single asteroids pixel.** Weaker than it was: the
  browser gate now loads the demo in a real Chromium, opens a WebGPU device and
  reads the canvas back with every check green, so "the frame is not blank, not
  one flat colour and changes between frames" is checked — 89 distinct colours
  across a 959×463 canvas on the SwiftShader adapter. What is still unchecked is
  whether it is the **right** picture, and in particular whether a rotated
  `SampleMode::Pixel` sprite looks right on a real driver.
  `crates/crcbl-vk/tests/vk_e2e.rs` has sprite goldens including a rotated one,
  so the shader path is covered; the game's own frame is not. There is also no
  display in the build environment, so the _windowed_ native path is compiled
  and never run. The art was checked by eye against the baked PNGs, and that is
  the honest report of it.
- **Tuning constants are compiled in.** The plan's milestone 3 wants them from a
  data file after stage 6. Every one of them is a `pub const` in `game.rs` with
  its reasoning written beside it, which is the form that survives being moved
  into a file.

## Deferred decisions

Questions that came up mid-slice and were answered by judgement rather than by
asking. Each is the question, the answer taken, and **what would change it** —
because the point is that a later reader can reopen one cheaply instead of
rediscovering that it was ever a question. An entry here is not a complaint
about the answer; most of these are probably right.

Distinct from _Considered and declined_ below, which is for ideas rejected on
their merits and expected to stay rejected. These are answers taken under
uncertainty.

- **Should `SoundBank` hold `Arc<[AudioSample]>` rather than `Vec`, so
  `create_voice` stops copying the sound?** _Yes, and it is why horde adopted
  the bank at all._ `SoundBank::create_voice` cloned the whole sample buffer per
  voice, which at horde's cue rate — up to about forty a second, each an
  allocation the size of the sound — was the one measured reason to keep the
  hand-rolled `Arc<Sound>` bank instead. Changing `Voice::data` to
  `Arc<[AudioSample]>` and `create_voice` to `Arc::clone` deletes the reason,
  and `Voice::new(Vec)` still compiles for every existing caller.
  `a_bank_shares_one_buffer_with_every_voice_it_makes` in
  `crates/crcbl-audio/src/mixer.rs` pins it on `Arc::strong_count`. **What would
  change it:** a bank that wants to hand out _mutable_ sample data, which
  nothing does.

- **Should `AudioStream::open` have kept taking `impl AudioSource` by value, or
  changed to `Arc<dyn AudioSource>`?** _Kept — the sharing went into a blanket
  `impl<T: AudioSource + ?Sized> AudioSource for Arc<T>` instead._ Changing
  `open`'s signature would have broken every existing caller, including the
  `wasm32` `web::install` path and the crate's own tests, for a case a blanket
  impl serves without touching any of them:
  `AudioStream::open(Arc::clone(&mixer))` now type-checks and a non-shared
  source still moves in as before. The cost is one redundant `Arc` layer on the
  shared path — `open` wraps whatever it is given in an `Arc` of its own — which
  is a pointer chase per block, not per sample. **What would change it:** a
  source that needs to be reached from the stream _and_ from two other places
  with different types, where the double `Arc` stops being the only wart.

- **Should the voice cap have moved into `Mixer` while the samples were being
  migrated?** _No — horde keeps `MAX_VOICES` and its refuse-newest policy._ The
  crate has no cap, no priority and no stealing, and the honest version of that
  feature is a voice budget with priorities so a death cue can steal a kill cue,
  not a bare count. Shipping the bare count in the engine would have frozen the
  crude policy as the crate's answer and taken the evidence for the good one
  with it, since horde's `Audio::dropped()` is the only measurement of the
  problem anyone has. **What would change it:** a second sample needing a cap,
  which would make it a pattern rather than one game's answer.

- **Should the samples' spatial assertions read the mixer, or the rendered
  audio?** _The mixer, through `Mixer::voice_mixes`._ Rendering a block and
  measuring left against right is the stronger observable and it was tried: it
  races the null stream's polling thread, which is draining the same mixer every
  five milliseconds and will have eaten an unpredictable prefix of any voice by
  the time the test looks. The gain-reaches-the-output half is checked once, in
  the engine, where a test can own a `Mixer` with no stream attached —
  `set_mix_re_aims_a_voice_that_is_already_playing`. **What would change it:** a
  headless `Audio` that opens no stream at all, which would make the render
  check deterministic in every sample.

  **Same race, second test, observed once:** asteroids'
  `the_engine_is_one_looping_voice_that_outlives_its_buffer` failed on
  `macos-latest` 2026-08-07 with "the engine's release block was cut" and passed
  on the immediate rerun and on both the preceding and following CI runs — no
  macOS-relevant diff separated them. Its release-block check has the same
  window as the spatial assertions above: a headless `Audio` opens the null
  stream, whose polling thread can consume the one release fade between
  `set_thrust(false)` and the test's own `fill`, so the test sees silence and
  blames the backend. The fix is the same one — a headless `Audio` with no
  stream — and it is the same decision, per sample.

- **Where does the menu art live?** Taken: **`crates/crcbl-render/assets/`**,
  baked by that crate's own `build.rs`. `apps/*` cannot depend on each other, so
  per-sample art is the same window authored three times and three games that
  look like three engines. The rejected alternative was a shared `assets/`
  directory both build scripts reach into: it shares the `.crpix` and nothing
  else — each script still bakes, each `art.rs` still loads, each game still
  writes the layout — and it puts a `../../..` path outside a package's own
  tree, which cargo does not track for rebuilds the way it tracks a package's
  files. It also gives `crcbl-vk`'s suite nothing, because that crate cannot see
  `apps/`, so the golden would be a picture of a replica. _Changes it_: art that
  is genuinely one game's. A sample that wants its own frame should author it
  under its own `assets/` and pass its own `MenuArt`; the shape for that is a
  constructor beside `MenuArt::register` taking a `Sheet`, not a fork of this
  one.

- **What size is the menu drawn at?** Taken: **the largest whole scale in 1..=4
  whose panel fits inside 90% of the framebuffer**, a pure function of the
  extent and the menu's own contents (`Menu::layout`). Whole numbers because the
  art is pixel art and a fractional scale puts a nine-slice corner on a half
  pixel; a fit rather than a constant because a fixed size is either lost on a
  4K screen or off the bottom of a 1440×400 canvas. _Changes it_: a settings
  screen with a UI-scale slider, at which point the scale is the player's and
  `layout_with` is already the entry point that takes one.

- **Does the menu shadow a key a sample had bound?** Taken: **yes, once** —
  flappy's `ArrowUp`, which is the _second_ binding of its flap action beside
  `Space`. The three menu keys (Up, Down, Enter) are the same three in every
  sample, for the reason F3, Escape and F11 are; two of the three are free in
  every game and this one is not. Space is never shadowed, is what the HUD has
  always named, and is printed on every button that flaps. The keys are consumed
  only while a menu is on screen. _Changes it_: a sample that binds Enter or the
  vertical arrows to something a player uses _while a menu is up_ — which today
  is nothing, because a menu is only on screen when the simulation is stopped or
  waiting. **Asteroids is the second, and it shadows `ArrowUp` too** — its
  second thrust binding, beside `KeyW`, which is not shadowed. Same trade, one
  sample later, and it costs less there: a menu is up only on a frame the ship
  is not being flown.

- **Does the world keep drawing behind a menu?** Taken: **yes, and it is
  dimmed** by a scrim sprite the menu's own pass draws. A frozen screenshot
  would need a captured frame and a second code path; a menu with nothing behind
  it loses the player's place. The scrim is a _sprite_ and not a `DrawList`
  rectangle because the UI pass runs after the sprite pass, so a UI-pass scrim
  would dim the menu's own frame along with the game. _Changes it_: a menu that
  wants the game genuinely stopped in the background — a settings screen over a
  paused multiplayer session, where the world is still ticking and the motion is
  a distraction.

- **Does a looping ping-pong replay its end frames?** Taken: **no.** A looping
  ping-pong's period is `2n - 2` — four frames run `0 1 2 3 2 1` and then `0`
  again — while a one-shot is `2n - 1`, precisely because that trailing `0` is
  no longer the next cycle's first, and an out-and-back that stopped on frame 1
  would look truncated. `Clip::steps` and `Clip::step` in `crcbl-sprite`.
  _Changes it_: art that wants a beat held at an end — a wing pausing at the top
  of its stroke. Today that is spelled by giving the end frame a longer `hold`,
  which works and is per-frame; if it turns out to be the common case rather
  than the exception, the answer is a hold on the return leg, not a global flag.

- **Does `reverse` reverse a clip's holds too?** Taken: **no** — a hold belongs
  to the frame it holds, not to the position in the list, so a reversed clip's
  tick pattern is the forward one read backwards. Documented on `Clip::step`.
  _Changes it_: a consumer wanting the reversed clip to have the same
  tick-by-tick timing _profile_ as the forward one (slow-then-fast staying
  slow-then-fast). Nothing has asked, and the current rule is the one that makes
  a frame's timing a property of the frame, which is easier to author against.

- **What does a nine-slice do when the target is smaller than its corners?**
  Taken: **the fixed bands shrink in proportion and the stretched band
  vanishes.** `NineSliceSource::expand`. The two alternatives were both worse:
  _refusing_ — emitting nothing, or clamping the target up to the minimum —
  makes a pipe squeezed below its caps either disappear at one size and not
  another or spill outside the rectangle it was handed; _letting the corners
  overlap_ inverts the middle band, which with no backface culling rasterises a
  mirrored quad rather than nothing, and double-blends the overlap. Shrinking
  keeps the three properties that matter more than corners staying literally
  fixed at a size where they arithmetically cannot: the quads still tile the
  target exactly, nothing is drawn outside it, and the picture is continuous —
  at exactly the minimum size this path and the ordinary one agree. _Changes
  it_: a caller that would genuinely rather draw nothing than draw squashed
  corners. That is `NineSlice::fits_in` at the call site, not a change here.

- **Should nine-slice edges tile instead of stretching?** Taken: **no tiling
  mode at all.** Two concrete costs: a tiled band is `ceil(extent / inset)`
  quads rather than one, so the instance count stops being bounded by nine and
  starts depending on how big the thing was drawn — a pipe stretched to a tall
  gap would quietly become hundreds of instances — and doing it in UV space by
  letting `u1` run past 1 needs a repeating sampler, while `SpriteRenderer` has
  exactly one sampler, `ClampToEdge`, shared by every sheet. _Changes it_: art
  whose edge is a repeating motif that stretching visibly smears — a chain, a
  rope, a brick course. Then it is a new mode with its own quad emitter, not a
  flag on `expand`.

- **Teach the UI pass a second texture, or draw button skins as sprites?**
  Taken: **sprites**, and an older backlog entry that called this "blocked on
  the UI pass sampling a second texture" was deleted as wrong rather than
  satisfied. The UI atlas is a single-channel `R8Unorm` glyph _coverage mask_
  sampled into alpha only — every fragment's RGB comes from the vertex colour —
  and a button skin is RGBA colour art. Routing it through would need a second
  bound image in a second format, a per-quad branch between two samplers, a
  UV-carrying draw command `DrawList` does not have, and an RGB path added by
  hand to both tier permutations of `ui.slang`. `SpriteRenderer` already is an
  instanced RGBA pass with alpha blending, and a skinned button is nine sprites.
  _The cost paid_: the caller owns the ordering. `RenderGraph` runs passes in
  declaration order with no topological sort, and both passes load rather than
  clear, so `SpriteRenderer::add_pass` must precede `UiRenderer::add_pass` or a
  skin paints over its own label — enforced by nothing but the order of two
  lines. _Changes it_: a UI element needing colour art _interleaved_ with text
  rather than behind it, which two passes cannot express at any ordering.

- **A fixed backdrop for breakout, or a parallax band?** Taken: **fixed.**
  _(Moved here from Considered and declined — it is a judgement about this
  game's camera, not an idea rejected on its merits.)_ Breakout's camera never
  moves — the field is fixed and the whole of it is on screen — and `Parallax`
  is `(1 − factor) × camera`, so with a camera at the origin every factor
  produces the same offset of zero. A "distant" layer and a world-locked one
  would be the same picture, and a band that scrolled anyway would be motion the
  player has no reason for. `art::Scene`'s two layers are both `Parallax::WORLD`
  and exist for depth ordering, which is the half of a `LayerStack` that still
  means something here. _Changes it_: breakout gaining a camera that moves — a
  screen-shake on a brick break would be the obvious one, and is currently a
  scope violation under the sample's "no juice" cap.

- **Commit the baked PNGs beside the `.crpix` text?** Taken: **no.** _(Moved
  here from Considered and declined.)_ It would make the build faster and the
  art reviewable in an ordinary diff, and it would create two sources of truth
  for one picture — the one a reviewer reads being the one that is not loaded.
  `docs/specs/crcbl/pix.md` is explicit that `.crpix` is a build input, and both
  samples' `build.rs` keep it that way. _Changes it_: a build where baking is
  slow enough to be felt, or a review workflow that genuinely cannot read
  `.crpix`. Neither is true today — the baker is a strip blit — and the honest
  fix for the second would be rendering `.crpix` in review, not committing PNGs.

- **What phase are the eleven sprite slices?** Taken: **P4B**, by analogy with
  P4A audio, which was the same shape — a subsystem that was not in the original
  phase table, delivered between numbered phases. It is written into both the
  roadmap's status table and its phase table. _Changes it_: a preference for a
  different label. Nothing depends on the string except the roadmap's own
  cross-references and this file.

- **What stays at P10 now that the frame-timing core is built?** Taken: **the
  rest of it.** The core shipped early, out of P10, because both existing
  samples wanted it and two more are planned before P10 — leaving it there would
  have guaranteed a third and fourth per-sample HUD, the shape `web.rs` already
  took twice. What P10 still owes is the rest of `07-ui-debug.md`'s suite
  (inspector, console, culling stats, debug-draw controls, UI inspector) and
  `23-netcode.md`'s netgraph, which is unbuildable before the transport can
  measure itself. _Changes it_: a sample that needs one of those sooner, which
  is the same argument that moved the frame-timing core.

- **How does a module register with the panel — retained list or per frame?**
  Taken: **per frame**, `DebugPanel::add(&dyn DebugModule)` once per system the
  frame actually has, matching the crate's immediate-mode authoring. A retained
  registry would need the panel to hold borrows or `Rc`s of every system that
  reports, which is the plugin framework `07-ui-debug.md` explicitly does not
  want, and it would make "a section appears because the system is present" into
  "a section appears because someone remembered to register and to unregister".
  _Changes it_: a module whose data is expensive enough to want gathering off
  the frame path, which would want a handle rather than a per-frame call.

- **What does the panel's FPS number mean?** Taken: **frames divided by the time
  they took** over a rolling 120-frame window, not the mean of the per-frame
  rates. The two agree only when every frame is the same length: 10 ms and 30 ms
  average to 67 FPS as reciprocals and to 50 FPS as `2 / 40 ms`, and the second
  is what the window actually ran at. 120 frames is two seconds at 60 Hz — short
  enough to react while you are looking at it, long enough to read. _Changes
  it_: wanting a 1%-low figure, which needs the sorted window this deliberately
  does not keep.

- **Which samples are exempt from the pixel-art rule?** Taken: **hud, viewer and
  sparks**, on the ground that each one's _subject_ is something other than
  pictures — a widget gallery, the user's own glTF, and a particle workbench —
  so authored sprite art in front of it would be showing the wrong system. hud
  still authors its button skins as `.crpix` because a skinned widget is a
  widget. Every other sample on the ladder is in scope. _Changes it_: a sample
  arguing itself out, which sample rule 11 requires it to do in its own doc with
  a reason.

- **Is `docs/code-review.md` a record or a description of current state?**
  Taken: **a record**, and left unedited except for a line in its header saying
  so. It is dated 2026-08-01, was added in one commit and never amended, and the
  roadmap already says its findings were fixed across eight commits. Several of
  its findings now describe code that no longer exists — the `paddle_model`
  finding is the clearest, since breakout has no forward pass at all. _Changes
  it_: a decision to keep it live, which would mean re-running the review rather
  than patching the findings that happen to have been noticed.

- **What does a paused frame do to the fixed-tick accumulator?** Taken: **update
  the clock and drain the accumulator without stepping the game.** The three
  candidates only differ after a long pause. _Not calling `update`_ freezes
  `FrameClock::last_update`, so the first update after the pause measures the
  whole of it and the `DEFAULT_MAX_CATCH_UP_TICKS` cap turns it into eight ticks
  in one frame — measured, not reasoned: falsifying the drain that way makes
  `resuming_after_a_long_pause_runs_one_tick_not_a_catch_up_burst` report "ran 8
  ticks" in all three samples. _Updating but not draining_ saturates the
  accumulator at the same cap and lurches identically, also measured. Draining
  leaves only the sub-tick remainder, so the first live frame runs the one tick
  it is owed, and it keeps `render_dt` real so the debug overlay's frame graph
  does not flatline at whatever it read when Escape was pressed. The cost is
  that `FrameClock`'s `TickId` advances during a pause; nothing in any sample
  reads it. _Changes it_: a consumer that does — a networked sample whose tick
  ids have to line up with a server's — which would want an explicit
  `FrameClock::reset` rather than a drain loop.

- **Is pause a `GameState` variant or the app loop's?** Taken: **the loop's.**
  Both samples' `GameState` lives inside `GameLogic`, which the authoritative
  server's `GameModule` mutates from inside a tick and which the client
  replicates; a `Paused` variant there would make the server's state depend on
  which window a player's compositor has focused, and would put a value in
  `Summary::state` that a headless scripted run could reach. Pause is not
  something the simulation does — it is the loop declining to advance it — so it
  is the loop's — `crcbl::engine::Loop::is_paused`, reported out through
  `RunSummary::paused` and from there into each game's own `Summary`. _Changes
  it_: a pause the _simulation_ has to know about, which in a multiplayer build
  it would: pausing a shared world is a server decision and would be a state on
  the server, not a client's window losing focus.

- **Does regaining focus resume?** Taken: **no.** A player who clicks back into
  the window would otherwise arrive mid-ball with no warning, and the pause menu
  exists to be dismissed on purpose. This also keeps the two edges asymmetric on
  purpose: focus loss is a thing the platform does _to_ the game, resuming is a
  thing the player does. _Changes it_: a sample where pausing costs the player
  something (a timed run), where the two-step would read as a penalty.

  **Read this together with "Should the click that refocuses a canvas reach the
  game at all?" above.** "Focus does not resume" is about the focus _event_. In
  a browser the gesture that delivers it is a click inside the game, so clicking
  back in onto `RESUME` does resume — one step, not two. The decision above is
  intact; the gesture is not the same gesture on every platform.

- **Which key pauses, given that a browser reserves Escape?** Taken: **Escape
  anyway.** Neither sample's action map binds it — breakout declares arrows,
  Space and R; flappy declares Space, Up and R — and it is what a player tries
  first. In a fullscreen browser demo Escape both leaves fullscreen and pauses,
  because `requestFullscreen` reserves the key and no page can decline it. That
  is one keystroke doing two reasonable things rather than a collision worth
  designing around. _Changes it_: a sample that wants Escape for something else,
  or a pause menu with a back-navigation stack where "leave fullscreen" and
  "close the menu" would want to be separate steps.

- **Who calls `requestFullscreen` in the browser — the shell or the page?**
  Taken: **the page.** A browser grants fullscreen only from inside a
  user-gesture handler; the shim's `keydown` listener is one and a
  `requestAnimationFrame` callback is not, and the engine reads a key on the
  frame _after_ the `keydown` that carried it, by which time the gesture is
  over. Calling it from Rust would also mean the wasm module's first non-`wbg`
  import, which `web/tools/check-exports.mjs` exists to prevent. So
  `web/engine/shell.js` binds F11 itself and reports the outcome through the new
  `__crcbl_web_fullscreen` entry point, exactly as a compositor answers
  `Shell::set_mode` with a configure rather than obeying it. The cost is that
  `FULLSCREEN_KEY` is spelled in four places — three `app.rs` files and
  `shell.js` — with nothing but a comment holding them together. _Changes it_: a
  second key wanting a gesture (pointer lock is the obvious one), which would be
  the point to give the shim a small table the engine can publish rather than a
  second hard-coded key.

- **Does the sandbox get a pause too?** Taken: **yes.** It has no game, and it
  does have a cube on the fixed timestep — the one thing in it a player can see
  stop — and the samples' standing rule is that a facility switched on in one is
  switched on the same way in all of them. It costs about fifteen lines.
  _Changes it_: nothing likely; if the sandbox ever became a pure benchmark
  harness, pausing it would be noise.

- **Should `MenuSet::activate` and `MenuSet::point` return the game's own
  `MenuAction` rather than a `WidgetId`?** _No — they return the id, and each
  sample maps it._ Returning the action needs a trait
  (`fn from_id(WidgetId) -> Option<Self>`) that every game with a menu must
  implement, to save one `.and_then(MenuAction::from_id)` at two call sites in
  `app.rs` and two test helpers per sample. The id is also what the layer
  beneath actually deals in: `Menu::activate` and `Menu::point` both return
  `Option<WidgetId>`, so the set passing it through adds no translation of its
  own. _Changes it_: a consumer that threads the action through several layers,
  where the `and_then` would start appearing at call sites that have no business
  knowing about ids.

- **Should the sandbox get a `MenuKind` enum for symmetry with the other four,
  instead of keying its set by `bool`?** _No._ `MenuSet<bool>` is what its one
  menu actually is, `false` is the state with no entry, and
  `apps/sandbox/src/app.rs` already called `self.menus.show(self.paused)` — the
  `bool` was always the key. An enum would be code added to make five files
  rhyme. _Changes it_: the sandbox growing a second panel, or the loop
  extraction turning out to need one `K` across all five.

- **Where does horde's "has the offer changed?" guard live now that the
  container is the engine's?** _In a `LevelUpOffer` type in
  `apps/horde/src/menu.rs`, held by the game itself — the `Horde` struct's
  `offer` field, rebuilt from `HostedGame::menu_kind`._ `MenuSet::replace`
  rebuilds unconditionally and drops the capture; deciding _when_ a panel is
  stale needs `built_from: Option<(u32, [Upgrade; 3])>`, which the engine cannot
  hold because it knows nothing about upgrades. The alternative was putting that
  field on the loop and inlining the comparison in `draw_menu`, which is the
  same state in a place where it could not be unit tested — and which is no
  longer even available, since the loop is `crcbl::engine::Loop` and a sample
  cannot add a field to it. _Changes it_: a second sample growing a rebuilt
  panel, at which point the guard is a shape and not horde's alone.

## What the horde Pages flake left behind

The flake itself is fixed and deleted from this file (2026-08-03, diagnosed from
the run's uploaded page log: the gate clicked the canvas **centre** to hand the
page its keyboard, which pressed `PLAY` — horde's centred first item — and
destroyed the run the `Space` after it was meant to start). Two things outlive
it.

- **A check that passes in the failure mode is not a control, and two of them
  agreeing is not corroboration.** Group E of `web/tools/browser-e2e.mjs` was
  read as evidence for ninety seconds of a contradiction that did not exist:
  `heartbeats()` counts any `[HUD]` line and horde logs one in every state
  including `WaitingToStart`, so every check in that group passes on a game
  sitting on its start screen. The theory that survived was the one nothing in
  the harness could refute.

- **Coverage gap: the title-screen inset test is horde's alone.**
  `a_focusing_click_off_every_button_leaves_the_game_paused` exists in all four
  games; `a_focusing_click_off_every_button_leaves_the_title_screen_up` is in
  `apps/horde/src/app.rs` only, because horde is the one game whose start-screen
  first item is destructive. The other three would need it if a start screen
  grew one.

## Considered and declined

- **Adopting `crcbl_ui::hud`'s `Hud`/`HudPanel` in the four samples.** It was on
  the audit's list as "the engine feature was already bought", and it is not:
  the type does not do what any of the four HUDs needs.

  **`Label` has no colour.** Colour lives on `Style`, one per panel, so a
  panel's labels are all one colour. Every sample draws its stat line yellow,
  its state line pale blue and — breakout — its lives line green, which is three
  colours in one panel and is not expressible. That alone ends it.

  Two smaller mismatches behind it. `HudPanel` sizes itself from its content,
  where horde's backdrop width is a **measured** constant with a test putting a
  stated worst-case run through the real `FontAtlas` and requiring it to fit;
  auto-sizing throws that guard away. And `Hud::render` routes button clicks,
  which a read-only stat panel has no use for.

  **What is actually shared between the four is not the drawing.** Each has a
  private `HudStrings` that rebuilds its strings only when the numbers behind
  them change — the caching is what keeps a steady-state frame from allocating.
  But the structs differ in their fields and their cache keys, because each game
  shows different numbers: that is duplicated _shape_, not duplicated knowledge,
  and the logic under it is three lines. Extracting it would be an abstraction
  over a coincidence.

  **The finding this leaves is about the engine, not the samples**:
  `crcbl_ui::hud` has no consumer anywhere in the workspace. It is either owed a
  `color` on `Label` and an optional explicit panel size — at which point the
  samples could adopt it — or it should be deleted. Not decided here, because
  adding a field nothing uses is the speculative-machinery mistake and deleting
  a module is not a call to make inside an adoption task.

- **Building the demos' export names in `web/engine/demo.js` from the sample's
  slug.**
  `exports[\`**crcbl\_${sample}\_frame\`]`would delete the thirty-line`bind`block from each`web/demos/<name>/main.js`and is the obvious way to write it. Declined because it defeats the gate:`web/tools/check-exports.mjs`learns which exports the JS depends on by scanning for a literal`.**crcbl\_…`and fails when one is missing from the artifact. Verified both directions — with the names spelled out, renaming`\_\_crcbl_breakout_frame`to`…\_framee`in`main.js`fails the check with that symbol named; behind a template literal the scan sees nothing and a typo becomes a`TypeError`
  in somebody's browser. The per-sample file is the price of keeping the check
  able to fail.
- **Folding the demo pages' "what is actually running" prose into a partial
  too.** Its opening paragraph differs between breakout and flappy by two words
  ("high score" / "best score") and its second paragraph differs materially —
  flappy's explains the seeded course, breakout's names swept-sphere collision.
  Templating it would mean the layout carrying three prose variables, which is a
  generator, not a partial. The shared blocks are the ones that are identical
  and structural: the window, the loop's keys, and the console note.
- **Reformatting `web/tools/browser-e2e.mjs` with prettier.** It is not
  prettier-clean at the width the rest of `web/` uses — confirmed against the
  version at `HEAD`, so it predates this work — and this slice touched only a
  three-line comment in it. Reformatting a 1400-line gate file to fix a
  whitespace complaint would bury that comment in a diff nobody can review.
  Worth doing on its own, with the gate run either side of it.
- **Fixing the multi-sheet sprite bug in the shader, by adding
  `SV_StartInstanceLocation` back on.** It works, and it is one line:
  `sprites[instance + base]` with `uint base : SV_StartInstanceLocation`
  restores the `BaseInstance` that `SV_InstanceID` subtracts, giving the
  absolute index that the old `draw(0..6, batch.instances)` needed. Measured
  with slangc 2026.14: the SPIR-V comes out with the `OpIAdd` next to the
  `OpISub` and no extra capability beyond the `DrawParameters` the file already
  declares.

  Declined for two reasons. First, `slangc` **rejects that semantic for WGSL** —
  `error[E55202]: system value semantic 'sv_startinstancelocation' is not supported for the current target`
  — so the source would have to be `#if`-split per target, and there is no
  target macro to split on (probed: `__TARGET_SPIRV__`, `SLANG_SPIRV`,
  `__SPIRV__`, `__TARGET_WGSL__` are all undefined; only `__SLANG_COMPILER__`
  is), so `tools/compile-shaders.sh` would have to start passing its own `-D`
  per target. Second and worse, the WGSL half would then be correct **because
  Slang's two lowerings disagree**: `SV_InstanceID` becomes
  `InstanceIndex - BaseInstance` on SPIR-V and a bare `@builtin(instance_index)`
  on WGSL, and only the SPIR-V one matches HLSL. A Slang release that made WGSL
  consistent with the rest would silently break the browser, with nothing in
  this repository pointing at the cause. Always drawing from instance 0 depends
  on neither lowering.

- **A dynamic offset on the instance _storage_ buffer rather than a per-batch
  constant block.** The obvious shape — bind `sprites` with `dynamic: true` and
  offset it to the batch — needs the binding's declared **size** to be fixed at
  bind-group creation while `offset + size` must stay inside the buffer, so the
  size would have to be "the largest batch", which is a per-frame quantity the
  group is not rebuilt for. Batches would also have to be padded to
  `min_storage_buffer_offset_alignment` (256 on WebGPU) rather than packed at
  `INSTANCE_STRIDE`. The constants block is 80 bytes and fixed, so the same
  mechanism costs nothing there.

- **Sharing `apps/*/src/audio.rs` and the best-score file between the two
  samples directly.** The duplication is real (findings 4 and 5) and the fix is
  in the engine, not in a crate the samples share between themselves: a
  `flappy-and-breakout-utils` would be a third place for the same code to rot,
  and it would hide the evidence that `crcbl-audio` and `crcbl-store` are
  missing a layer. **Vindicated**: both layers were built where the evidence
  said they belonged — `crcbl_audio::synth` and `crcbl::store::record::Record` —
  and the samples adopted them.
- **A `visible` check inside `DebugPanel::layout`.** It was written, and it
  could not be made to fail: `add` refuses to gather while hidden and
  `set_visible` drops what was gathered, so a hidden panel has no sections and
  the emptiness check already returns `None`. A guard that no test can reach is
  a guard that reports "passed" for reasons unrelated to what it guards, so it
  was deleted and the reasoning left in its place.
- **A `DebugSection::row` taking `String`s.** It takes `fmt::Arguments` instead,
  so a module writes `row("fps", format_args!("{fps:.1}"))` and formats straight
  into a `String` the section already owns. The ugly signature buys a
  steady-state section rebuild that allocates nothing, which matters for the one
  widget whose job is not to disturb the thing it is measuring.
- **Tinting one brick sprite four ways instead of authoring four frames.** It is
  the cheaper sheet and it is what `app.rs`'s colour table used to do. Four
  frames is what lets the rows differ in their _shading_ — a lit top edge and a
  shaded bottom in each row's own hue — which a single tinted rectangle cannot
  express, and it is what a sprite sheet is for. The cost is 96 × 8 texels
  instead of 24 × 8.
- **Re-randomising flappy's course from a clock.** A restart advances the seed
  deterministically (`course_seed(seed, runs)`) instead. A clock would make the
  course unreproducible, and the sample's exit criterion is that a recorded
  script replays to the same score.
- **Authoring flappy's background bands at one texel per sprite unit.** They are
  drawn at `art::BACKGROUND_SCALE` = 2 instead. At `TEXELS_PER_UNIT` = 20 a hill
  wide enough to read as a hill is a couple of hundred texels of hand-written
  rows for a silhouette with two bumps in it; the pipe is deliberately **not**
  scaled, because its caps are measured in texels and scaling would stretch
  them. If the bands ever gain detail that the doubling makes obvious, redraw
  them rather than adding a second scale knob.

## Full-codebase review 2026-08-04

Scope: working tree was clean (branch `crcbl-worktree` == `origin/main`, commit
050f570), so per the review/audit/perf conventions the **entire workspace** was
reviewed — `crates/*` and `apps/*`, ~216k lines of Rust. Correctness, security
and performance passes were split per crate across read-only review passes;
every finding below was re-verified against the code it cites (re-traced to the
return path, guard chain checked, string/length arithmetic applied) before being
published. **47 findings: 16 medium, 31 low, no critical or high — and all 47
were closed on 2026-08-04**, one commit each (pushed to `main`; `git log` is the
record, and each fix shipped with a test that failed on the old code or an
honestly stated gap). What survives below is the part of the review that is not
a closed finding.

### Cleared (the expensive half)

Per-crate review passes explicitly disproved these before publishing anything:

- **crcbl-net**: decoder panics on hostile bytes (every decoder length-gated
  through `ByteReader`); unbounded allocation from length fields (delta/system
  counts checked against remaining bytes before `with_capacity`); ReplayWindow
  edges; HMAC vs RFC 4231 vectors, constant-time compare; rate-limiter overflow
  (u128/saturating); reflected authenticated packets (disjoint direction tags
  fail the codec decode); repair-ack loop; `handle_ack` monotonicity.
- **crcbl-vk**: acquire-semaphore reuse (safe only because of the
  `slots = image_count + 1` throttle); surface refcount balance across every
  swapchain path; `Drop for DeviceInner` ordering; handle-tagging collisions;
  `write_buffer` bounds; submit-counter ordering; SPIR-V parser bounds.
- **crcbl-shell appkit**: pointer-capture revert on error; enqueue coalescing
  against the BackingChanged+Resized pair; retain/release balance; warp/flip
  math; CAMetalLayer Retina sizing; pool handle reuse.
- **crcbl-shell win32**: WM_CAPTURECHANGED guard; resize-coalescing order;
  WM_PAINT termination; 0×0 WM_SIZE handling; WM_DPICHANGED nesting; X_BUTTON
  decode; RAWINPUT sizes; TimeBase wrap; Drop ordering.
- **crcbl-shell x11**: GeGeneric sizing and `full_sequence` offset (verified
  against libxcb layout); xcb reply/event free-exactly-once at all ~20 sites;
  get_property chunk loop; Atoms pipelining; INCR state machines (terminator
  always emitted, ack-by-delete ordering); fp3232 fraction; blank_cursor
  lifetime; SelectionClear ordering; set_pointer_mode grab failure.
- **crcbl-shell wayland**: same-offer selection re-send (verified against
  wlroots source); fd close-exactly-once on every path; protocol decode overruns
  (libwayland signature validation); keymap size-vs-length check before mmap;
  drag drop/teardown double-destroy ordering; TimeBase rebase wrap; repeat-rate
  caps; axis gating.
- **crcbl-hal**: `Extent3d::full_mip_levels`; Format block/texel sizes for all
  29 formats; `needs_barrier` discriminant logic; readback poll contract; device
  outlives instance; create_device default loop; reversed-Z consistency;
  swapchain extent obligations.
- **crcbl-wgpu / null**: null ring rotation; poll_readback slice bounds; wgpu
  lock ordering; generational handle reuse; destroy_readback on Failed; present/
  reconfigure/destroy present the outstanding SurfaceTexture on every path;
  double-submit detection; semaphore promotion.
- **crcbl-render**: tonemap bind-group cache (destroyed-after-use is safe via
  the retire queue + generational handles); cross-frame barrier ordering;
  nine-slice geometry (traced against tests); camera math; texture row pitch;
  sprite-batching instance addressing; UI tier split; timer ring; graph state
  tracking.
- **crcbl-core / ecs / input**: arena aliasing (bumpalo-style argument),
  zero-size allocs, generation wrap (checked_add retires at u32::MAX),
  stale/foreign handles, System::detach swap-remove, input key up/down pairing,
  WASD normalization, WorldPos rebase math (Sterbenz), splitmix64 vectors,
  FrameClock accumulator. All non-test panic sites are unreachable from within
  the invariants.
- **crcbl-phys**: AVL rotation (traced all four shapes); BVH slot recycling;
  refit-only update_aabb; ray_vs_capsule piece tests; select_hit branches;
  entity churn; determinism; DampingForce cap; swept-TOI arithmetic.
- **crcbl-audio / store**: QOA bounds (verified against qoa.h byte-for-byte),
  allocation bomb rejected before reserve, WAV parser chunk arithmetic, mixer
  data races (single mutex, immutable samples, atomic ids), web resampler phase
  math, crash-ring wrap agreement, save/replay parser length gates, OPFS framing
  checksum + generation restore, URL/key allow-list containment.
- **crcbl-ui**: HUD snapshot vertex counts (136 hand-verified), double-applied
  scale (all callers pass 1.0), RectOutline geometry, menu centring math, fit
  loop, FrameStats windows, UTF-8 codepoint handling, widget_id collisions,
  click-capture correctness.
- **crcbl-sprite / wl-scanner / shaders / golden**: crpix header let-else,
  palette `#` handling, XML entity DoS (no DTD), quote-aware start-tag scan,
  emit identifier gating, SHA-256 vs FIPS 180-4 + NIST vectors, golden PNG size
  guard (the pattern load.rs should copy), JSON surrogate pairs.
- **crcbl-cli / engine**: semaphore value-0 semantics, cargo invocation (no
  shell, args via `Command::arg`), screenshot channel order, replay tick bounds,
  `new` template escaping, App::frame stage machine, readback arithmetic,
  GpuContext teardown order, FrameBudget cap.
- **apps**: breakout bounce data (real sweep, not fabricated), per-tick
  high_score.raise early-return, brick-neighbour geometry, asteroids wave/split/
  tumble index spaces, perimeter_point catch-all, save-file parsing,
  pause/focus/ dt handling in the engine loop; sandbox/sim/bare: sim tick-drift
  (ManualTime whole-tick drain), headless tick-count assertions, seed
  determinism, f32 hashing, frame-budget edges.

### Hardening (correct today, fragile — explicitly not defects)

- **net**: `baseline_tick = 0` is wire-ambiguous (delta.rs:824/866-869;
  unreachable — the server never encodes against tick 0); a forged `Accept`
  permanently wedges the client (unauthenticated handshake by design); `Reject`
  `msg_len` is u16 with a silent cast on encode (codec.rs:399); key rotation on
  reconnect trusts a cleartext token (documented); reject messages disclose
  server identifiers pre-auth.
- **vk**: acquire path waits on the armed fence with `u64::MAX` _while holding
  the device lock_ — a compositor that never returns an image hangs every device
  call; semaphore-reuse safety depends on the `slots = image_count + 1`
  throttle; `submit` never checks the CB's pool family matches the queue;
  `untag`'s `unreachable!` panics on a forged handle.
- **appkit**: the field drop order is the _opposite_ of what the comment claims
  (shell.rs:149-150 vs 1605-1607 — `shared` drops first; nothing dereferences it
  between the drops today, a future field or an AppKit call in `Drop` makes it a
  UAF); the first-responder-after-`setStyleMask:` hazard is logged, never
  re-issued (known open item); the borderless-origin defect is the tracked open
  item in this backlog.
- **win32**: `ScreenToClient` return ignored in the wheel arm (proc.rs:679);
  `GlobalLock` failure reads as `ClipboardContent::Empty` (documented);
  registered-format payloads lose a trailing NUL; 0×0 descriptor creates a
  frame-only window (doc overstates); `Limits` stale for one pump after
  `WM_DPICHANGED`.
- **x11**: `handle_selection_notify` phase routing times out pathological
  owners; a second keyboard's held key reads as repeat; `create_window` clamps
  width/height to u16::MAX; `warp_to` clamps out-of-i16 to (0,0); `modifiers()`
  allocates per key event without a keymap; consumer offers are not size-capped
  before `ChangeProperty` (trusted caller only).
- **wayland**: `PendingConfigure` never cleared (protocol-violation-only);
  `Conn::drain` treats any negative return as a permanent disconnect; e2e
  `attach_shm_buffer` stride×height truncates to i32 (test scaffolding); a 4 GB
  keymap file costs a 4 GB virtual mapping.
- **hal**: `ColorAttachment::resolve`'s required state is never documented; the
  reference frame destroys the command buffer right after present (wrong pattern
  to copy); `write_buffer`'s error doc says "not host-visible" but the
  requirement is `HostUpload` specifically; `query_results` "returns zeros
  without TIMESTAMP_QUERY" is unreachable (create_query_set errors first);
  `present`'s queue must be present-capable but the seam never says so;
  `AcquiredFrame` carries no swapchain identity.
- **wgpu**: unclosed pass / draws-outside-pass silently no-op where the null
  backend records validation errors; creation calls not routed through
  `checked()` (descriptor errors surface one `take_error` drain late); null
  `create_image_view` never validates format/subresource; `set_scissor` with
  `rect.x == i32::MIN` overflows `x - rect.x`; abandoned encoders leak one pool
  entry; `create_buffer` size within 3 bytes of u64::MAX panics on alignment;
  `copy_layout` bytes_per_row wraps on adversarial extents; `write_buffer`
  alignment differs between backends; offscreen surface formats differ
  (Rgba16Float offered by wgpu, refused by null); `SwapchainSlot::suboptimal` is
  dead state; two pending signals of the same timeline value pass the check;
  null `semaphore_value` always returns 0.
- **render**: cross-frame mixed-state transient handoff (single-mip production
  transients only); cross-frame queue-ownership release dropped (no second queue
  in use); `begin_frame`'s `atlas` argument is layout-only; pool transient view
  covers every mip; per-frame CPU allocations are small and documented;
  `upload_texture`'s expected-size math can overflow u64 (unreachable with real
  memory).
- **core/ecs/input**: wrong-kind bindings silently produce permanently idle
  actions (user-profile typo, no diagnostic); `set_enabled(true)` doesn't
  resolve immediately; `FrameClock::new(tick_hz > 1e9)` panics with a misleading
  message; `FrameArena` doc claims "neither Send nor Sync" (it is Send, only
  !Sync); `with_capacity(usize::MAX)` overflow is pre-empted by the vec capacity
  check; `Held` duration quantizes to f32 after ~19 days uptime.
- **phys**: `world_mut()` lets a caller desync `collider_to_entity`;
  `ThrustForce` fields are pub (unnormalized direction silently scales thrust);
  negative collider radii bypass the constructors; per-tick `Vec<Entity>` in
  `step` is negligible.
- **audio/store**: `opfs.rs` write-before-ready can be replaced by a later
  generation restore; `settings.rs get` falls through on a type error in a
  hand-edited file; `voice_mixes()`/`voice_count()` take the audio thread's
  mutex (HUD polling can stall audio); qoa.rs:349 saturates where the reference
  wraps (adversarial LMS weights only).
- **ui**: `FrameStats::with_window` aborts on a huge caller-supplied window;
  public float style fields are unclamped (0/negative → inverted geometry);
  `Text` top-left-anchor holds only for the built-in metrics; trailing-newline
  labels measure one line too tall; per-frame allocations are documented.
- **sprite/wl-scanner**: JSON recursion depth (~30-50k nested objects overflow
  the stack; sidecars trusted); `emit::KEYWORDS` omits
  `self`/`Self`/`super`/`union` (loud compile error, not silent mis-generation);
  `worst_pixels` collects all differing pixels then truncates (up to ~230 MB on
  an all-different 4K frame); `escape_ident`/`camel_case` collisions name the
  generated file, not the XML line.
- **cli/engine**: `channel_order`'s `_ => Rgba` arm would silently mislabel a
  future non-8-bit format (unreachable today); F11 toggle runs before the
  `destroyed` check; the pointer hit-test runs before `draw_menu`
  (one-frame-late menu clicks); failed `PendingGpuContext`/`GpuContext::finish`
  drops surfaces without `destroy_surface` (vk cleans up with a warning);
  `request_open`/ `start_device` accept a (0,0) extent (swapchain creation fails
  loudly); sandbox `--frames 0` accepted while bare rejects it; sandbox
  `--backend` usage text names only vk/null while more parse.
- **apps**: asteroids score is u32 (debug panic after ~43M small rocks); muzzle
  spawn wraps to the far side at the field edge; fire press during respawn is
  consumed (no edge buffering); breakout destroys a brick even when not
  approaching (unreachable with current geometry).

### Coverage

Scope: the whole workspace (clean tree at 050f570). Reviewed in full, per crate:
`crcbl-net` (+fuzz), `crcbl-server`, `crcbl-client`, `crcbl-shell` (appkit,
win32, x11, wayland, linux, web, shared), `crcbl-vk`, `crcbl-hal` (+null),
`crcbl-wgpu`, `crcbl-render` (+tests), `crcbl-core`, `crcbl-ecs`, `crcbl-input`,
`crcbl-phys` (+tests), `crcbl-audio` (+tests), `crcbl-store` (+web), `crcbl-ui`,
`crcbl-sprite` (+tests), `crcbl-wl-scanner` (+tests), `crcbl-shaders`,
`crcbl-golden`, `crcbl-scene` (empty), `crcbl-cli` (+tests), `crcbl` (engine;
non-test code), and apps asteroids, breakout, sandbox, sim, bare, horde.

GAPS — reported honestly:

- **apps/flappy: reviewed by a sub-agent whose report was never delivered** (the
  agent twice claimed delivery of a report that never arrived; only a summary
  fragment was received). The horde finding (45) was independently verified
  against the code; **flappy's zero-finding verdict is the agent's claim, not
  independently confirmed** — nothing in flappy was verified by me.
- **`crates/crcbl/src/engine.rs:3301-5105` (the test module)** and
  `crates/crcbl-ecs/src/{world,schedule}.rs` internals were not read by any
  review pass.
- **`crates/crcbl-net/fuzz/corpus/`** binary seeds — exercised via
  `include_bytes!`, not read as code.
- **wgpu internals** (the wgpu/wgpu-core dependency) were consulted for specific
  claims (resolve_target, tight packing, output_buffer_size) but not audited.
- No build/test run was performed during the review passes (read-only
  constraint); every finding above is static-verified. The project's CI gate
  (`cargo fmt/clippy/build/nextest`) was not run as part of this review.

## What MTL1 left open on the Metal backend

- **It advertises Tier B, and a tier-aware caller will believe it.**
  `DeviceCaps::tier` is derived, and `DRAW_INDIRECT_COUNT` /
  `MULTI_DRAW_INDIRECT` stay off until the command slice picks Metal's indirect
  path (indirect command buffers, per `docs/plan/09-backends-metal-dx12.md`'s
  mapping table). Correct today and documented in the crate docs, but it is
  visible behaviour: once anything selects on tier, macOS takes the Tier B
  branch.
- **The engine has never stated a minimum macOS version.** `adapter.rs` sends
  `supportsBCTextureCompression` among others, which dates the floor to macOS
  11; `objc2` does not gate on availability, so an older system raises an
  unrecognised-selector exception rather than answering wrongly. Loud, but
  undecided — the same question the AppKit shell backend has been carrying
  unstated since P5C.
- **`DeviceType::Virtual` is unreachable on Metal.** There is no virtualisation
  query, so a paravirtual GPU answers every question exactly as the built-in one
  and enumerates as `Integrated`. Stated as a gap, not fixed.

## Considered and declined: an OpenGL / GLES backend

**Decided 2026-08-05.** GL is a dying support surface and the engine will not
grow a `crcbl-gl`. The platform matrix is Vulkan for Windows, Linux and Android;
Metal for macOS and iOS; DX12 for Windows as the second Windows path. Nothing
else — see the Apple decision below, taken the same day, which closed the
MoltenVK option this entry originally listed alongside them.

Reasons, so this is not re-argued:

- **GL is already reachable and nobody needs a crate for it.** `crcbl-wgpu`
  enumerates `wgpu::Backends::all()` and wgpu's default feature set includes
  `gles`, so a GL device is enumerable through the existing backend today. It is
  present and unproven rather than supported — nothing in CI exercises it — but
  the cheap experiment would be pointing the existing wgpu e2e suite at a GL
  device, not writing a backend.
- **The blocker is above the seam, not at it.** `RendererTier` declares exactly
  two tiers, and Tier B is not a low bar: per-batch indirect draws, indexed SSBO
  lookups, and culling still running in compute. GLES 3.0 has no compute, no
  SSBOs and no indirect draw — those arrive in 3.1 — so the old hardware GL
  would be added _for_ cannot reach even Tier B. A Tier C is a renderer change
  with a third draw-emission path and a third set of golden images, which is far
  more expensive than the backend crate it would sit under.
- **GL fights this seam specifically.** No command buffers (the seam hands out a
  `CommandEncoder` and submits; GL executes immediately), thread-affine contexts
  against a seam that requires `Device: Send + Sync` on native, no explicit sync
  to map `pipeline_barrier` onto, and reversed-Z — locked engine-wide — needing
  `glClipControl`, which is core in GL 4.5 but only an extension on GLES.
- **It is the wrong tool for mobile anyway.** iOS is Metal-only and has
  deprecated GL ES since iOS 12; modern Android ships Vulkan. The Android gap is
  a `crcbl-shell` surface backend, not a HAL backend — `crcbl-vk` already exists
  and is the best-tested path in the workspace.

## Considered and declined: Vulkan on macOS and iOS

**Decided 2026-08-05. Apple platforms are Metal only.** `crcbl-vk` is not
expected to run there, MoltenVK is not a shipping path, and the MoltenVK spike
`docs/plan/09-backends-metal-dx12.md` scheduled as P14's first task **will not
be run** — the gate it was meant to inform is closed by this decision instead.

What that buys, and what it costs:

- **One macOS path instead of two.** The alternative was shipping on MoltenVK
  while native Metal caught up, which means two GPU paths to test on the
  platform with the least CI capacity, and bug reports that begin with "which
  one were you on".
- **iOS was never in question.** There is no Vulkan loader or ICD story on iOS
  at all; MoltenVK is linked directly into the app. Metal is the only path
  there, so choosing it for macOS as well makes the whole Apple side one
  backend.
- **The cost is that `crcbl-mtl` is now load-bearing rather than an
  optimisation.** Until it can present a frame, macOS has no native GPU path —
  `crcbl-wgpu` is the only thing that runs, at Tier B. That raises the stakes on
  MTL3 (first pixel) and MTL5 (swapchain) and is the reason they are the two
  slices worth watching.

The technical question the spike would have answered is recorded here because it
is the same question `crcbl-mtl` itself has to answer, and the answer is now
expected from the Metal side rather than the Vulkan one: `crcbl-vk` demands
`Features::TIER_A` outright rather than degrading, that set includes
`DRAW_INDIRECT_COUNT`, and `crates/crcbl-vk/src/adapter.rs` reads it straight
off `VkPhysicalDeviceVulkan12Features`. **Metal has no native indirect-count
draw**, which is exactly why `crcbl-mtl` reports Tier B today and why MTL6's
indirect-command-buffer work is what moves it. MoltenVK would have hit the same
wall from the other side.

One framing note kept because it explains why "the user installs MoltenVK" was
never the shape this would have taken: MoltenVK ships **bundled with the
application**. The Vulkan SDK's macOS installer places an ICD for development,
but a shipped app embeds `libMoltenVK.dylib`. It describes a developer's
machine, not a player's.

## Confirmed: DX12 stays, alongside Vulkan on Windows, and last

**Decided 2026-08-05**, closing a question that had been half-answered twice —
`docs/plan/09-backends-metal-dx12.md`'s original text justified DX12 as old-iGPU
coverage, its 2026-07-27 correction retracted that and substituted the Xbox door
plus Windows tooling, and neither pass weighed it against simply using
`crcbl-vk` on Windows.

**Windows keeps both backends. DX12 is never a replacement for Vulkan there.**

### The asymmetry that settles the "instead of" framing

`crcbl-vk` has to exist regardless — it is the Linux path and, per the same
day's platform decision, the Android one. Windows support falls out of it at
approximately zero marginal cost, because it is the same code reaching a
different loader. So dropping Vulkan _from Windows_ saves nothing: the crate,
its tests and its maintenance all stay. Replacing it with DX12 would pay for a
new backend to obtain a working path that already exists.

It would also cost the one thing Windows is uniquely placed to give:
**cross-backend differential debugging on identical hardware.** "Does it repro
on the other backend?" is reason #1 in `crcbl-hal`'s own argument for dynamic
dispatch and for compiling two backends into one binary, and Windows is the only
platform where both can run against the same GPU.

### Why it is still worth building

- **Xbox.** The only item here obtainable no other way.
- **A GPU device on the Windows CI runner.** Every software-rasteriser job in
  `ci.yml` is `ubuntu-latest`/lavapipe; `windows-latest` has no device at all,
  which is why Windows has no golden images and no sample-level render pass.
  WARP is D3D12's software rasteriser and ships in Windows, so this would be
  Windows' lavapipe. **See the open question below — this benefit is
  unconfirmed.**
- **Robustness against a missing or stale vendor ICD.** D3D12 is part of the OS;
  Vulkan is not.
- **Windows-on-ARM**, where D3D12 is first-class and Vulkan is patchier.
- **PIX and DRED**, and DXGI's waitable swapchain object — a mature answer to
  the closed-loop frame pacing this backlog already has open, where the Vulkan
  side needed `VK_KHR_present_wait` (bound in the pinned `ash`) and
  `VK_EXT_present_timing` (ratified, but not in `ash` at all, so genuine
  hand-written FFI) — both of which have since landed.

### Why it is last

- It maps near-1:1 onto the Vulkan-shaped seam, so **it finds no HAL leaks**.
  That is a cost saving and a value reduction at once: Metal is the backend that
  stresses the abstraction, which is why the plan orders it first.
- Its value is infrastructure and optionality, not capability. Nothing renders
  today that it would render better.
- It is a crate comparable in size to `crcbl-vk`, the largest in the workspace,
  plus a third shader artifact (DXIL) in `crcbl-shaders` and its manifest, plus
  another pinned toolchain in the `shaders` job, plus a second Windows path to
  test permanently.

Ranked below finishing Metal — which after the same day's decision is the _only_
Apple path — and below an Android surface in `crcbl-shell`, which is the largest
coverage win available and needs no new HAL backend at all.

### Open question, worth an afternoon before the phase starts

**Does WARP clear Tier A?** Specifically SM6.6 dynamic resources, which this
backend is specced around. If it does, the Windows CI argument is real and DX12
buys golden-image coverage on a second OS. If WARP is Tier B only, `crcbl-wgpu`
already covers that on Windows and the CI half of the justification collapses,
leaving Xbox and tooling. Cheap to check, and it changes how much of the above
is true — so check it before committing the phase, not during it.

## Considered and deferred: console backends

**Decided 2026-08-05. No console support now; open to it if someone asks for
it.** Nothing is being built speculatively, and nothing in the engine forecloses
it. The canonical platform matrix is in `docs/plan/01-foundations.md`.

### What each console would actually need

- **Xbox — comes free with DX12.** It is D3D12X through the GDK rather than
  desktop D3D12, so it is not literally the same backend, but `crcbl-dx12` is
  the prerequisite and the delta is small. This is already the strongest item in
  DX12's justification (see the DX12 entry above).
- **PlayStation — a private crate.** There is no Vulkan on PlayStation, ever.
  PS5 is AGC (with a GNM compatibility layer), PS4 is GNM/GNMX, and shaders are
  PSSL. **The blocker is legal rather than technical**: the SDK, its headers and
  the API's detailed shape are under NDA, and downloading any of it requires
  licensed-developer status with an approved concept. So it cannot live in this
  repository and cannot be written speculatively by anyone.
- **Switch — probably `crcbl-vk` with a shell backend.** It has a working Vulkan
  driver. NVN is the faster native path and what shipping titles use, but Vulkan
  is a genuine bring-up route, which makes Switch by far the cheapest console to
  reach and the only one needing no new HAL backend.

### Why this costs nothing to defer

**The seam is what makes a console backend possible at all.** A closed crate
implementing the public `crcbl-hal` traits drops into a private workspace as a
path dependency, with zero changes above the seam — the renderer, ECS, UI and
every game compile unchanged. That property is already load-bearing for the four
public backends; consoles just exercise it under an NDA.

AGC is also close to the shape already built: explicit command buffers, explicit
sync, bindless descriptors, GPU virtual addresses. The Vulkan-flavoured seam is
roughly right for it, for the same reason DX12 maps near-1:1.

The genuinely new axis is **shaders**. PSSL is HLSL-like and the platform
toolchain consumes HLSL-ish input, so the path is Slang → HLSL → PSSL — a fourth
artifact after SPIR-V, WGSL, MSL and DXIL, and the only one whose compiler could
never run in public CI.

### `BackendKind` would need a variant — and that is not a problem

`crcbl_hal::BackendKind` is a closed enum —
`Vulkan | Wgpu | Metal | Dx12 | Null` — so a console backend needs a new variant
(naming a console is not an NDA breach) or a `Custom(&'static str)`, because a
private crate cannot add one to a public enum it does not control.

**Add it when a console backend actually exists.** This was first written up as
something to settle before the seam freezes, on the grounds that a new variant
is a breaking change to a public API. That reasoning does not apply here: the
workspace is `0.1.0` with no tags, everything so far is unreleased, and the
project's own convention is that below 1.0 a breaking change bumps the minor.
Breaking changes are routine and expected, so there is nothing to buy by
deciding early — and adding a variant nothing implements would be the
speculative machinery this codebase deletes rather than keeps.

## What MTL2 left open on the Metal backend

- **`MTLTextureUsagePixelFormatView` is set on every colour image,
  unconditionally.** It can disable lossless bandwidth compression on some Apple
  GPUs. Narrowing it needs the seam to carry intended view formats the way
  WebGPU's `viewFormats` does, which is a HAL change and so was not made.
  Recorded in `conv::texture_usage`'s docs.
- **Metal validates descriptors by raising, not by returning nil**, and an
  Objective-C exception crossing into Rust aborts the process. `create_image`
  guards the rules confirmable from the headers and deliberately invents no rule
  that could not be confirmed — **so a caller passing `HostUpload` for a depth
  image could abort rather than receive an `Err`.** How far descriptor
  pre-validation should go on this backend is undecided, and it is a question
  the other backends do not have.
- **`conv`'s `ALL` format list is hand-maintained.** The staleness guard asserts
  the last sorted entry is `Bc7RgbaUnormSrgb`, which catches an appended format
  and not an inserted one. Small, and the compiler catches the half that
  matters.

## `render_area` does not exist in Metal, and clears diverge because of it

**The closest thing to a seam leak the Metal backend has hit.** Found in MTL3,
not fixed, and `crcbl-hal` was deliberately not changed.
`MTLRenderPassDescriptor` has no render-area rectangle, so `crcbl-mtl` turns
`render_area` into the render encoder's **scissor**, set only when it is a
genuine sub-rect. The consequence is a real behaviour difference: **a
`LoadOp::Clear` clears the whole attachment on Metal, where Vulkan clears only
the render area.** Nothing above the seam depends on it today (the render graph
always passes the full attachment), so this is latent rather than broken.
Options, none taken:

1. Document `render_area` as affecting rasterisation only, and require a caller
   wanting a partial clear to draw one. Cheapest; makes the seam honest about
   the weaker guarantee.
2. Have the Metal backend emulate a partial clear with a draw when `render_area`
   is a sub-rect and the load op is `Clear`. Costs a pipeline in the backend.
3. Drop `render_area` from the seam entirely and give the encoder a scissor
   call. Largest change, and closest to what Metal, DX12 and WebGPU all do.

Wants a decision before anything starts relying on the Vulkan behaviour. Both
backends must then be re-verified.

## What MTL3 left open

- **The GPU-side wait is not proven to gate.** The test proves a wait does not
  wedge the queue and that an unsatisfiable wait is refused up front. Proving
  the wait actually orders two submissions needs an observation _between_ them,
  which is a race rather than an assertion, so it is not attempted. Stated as a
  gap.
- **Query sets stay refused**, deliberately and with the argument in
  `create_query_set`'s docs: Metal answers `supportsCounterSampling:` per
  sampling point, which the seam's free-standing `write_timestamp` cannot reach.
  Half-building it would give real timings on some Macs and zeroes on others.
- **`device.rs` is 4057 lines and should be split** — the pools, the resource
  create/destroy pairs, submission and readback are separable responsibilities.
  Wants to be a move-only change so a reviewer can see it is only a move.
- **`DepthStencilAttachment::read_only` is read and deliberately not acted on.**
  Metal has no image layouts, so there is nothing to set.

## What MTL4 left open

Most of MTL4's blocking list is closed by later slices — bind groups exist as
flat argument tables, compute pipelines build, index buffers bind, and the
engine's own `triangle.slang` draws through a bind group (it is one of the
quarantined draw tests above). What remains:

- **The pipeline object is only half of `GraphicsPipelineDesc`.** An
  `MTLRenderPipelineState` carries the shaders, the colour attachment formats
  and blending — and **not** cull mode, winding, fill mode, depth clip, depth
  bias, the depth/stencil state, or the primitive topology. Those are encoder or
  draw-call state in Metal; `crcbl-mtl` stores them in a `RasterState` beside
  the pipeline and replays them at bind. A future slice that binds pipelines
  through a different path has to replay them too, or half the descriptor
  silently stops applying.

## The four draw tests are quarantined on a real bug, and the cause is down to one standing candidate

`crcbl-mtl`'s suite ran for the first time on a hosted runner on 2026-08-05 (run
31042925024): 102 of 106 passed, and **the four that failed are exactly the four
that make the GPU run a shader** —
`a_triangle_draw_paints_the_centre_and_leaves_the_corners_clear`,
`the_engines_own_triangle_draws_through_a_bind_group`,
`an_indexed_draw_reads_the_bound_index_range` and
`a_multi_draw_indirect_emits_every_argument_structure`. Each faults the same way
—
`DeviceLost("… Caused GPU Hang Error (00000003:kIOGPUCommandBufferCallbackErrorHang)")`,
every encoder `completed`, none faulted — and **this is a `crcbl-mtl` defect,
not a runner limitation**: a standalone Swift probe drew the same triangle on
the same image and read the correct texel, so the device rasterises; what hangs
is this backend's command stream. Two candidates were differences between the
probe's stream and the backend's; one (the render-target format, `Bgra8Unorm` vs
`Rgba8Unorm`) has been ruled out by an experiment that faulted byte-identically.
**The one still standing is the error-options command buffer**: every command
buffer here is made by `crate::fault::command_buffer` with
`commandBufferWithDescriptor:` +
`MTLCommandBufferErrorOptionEncoderExecutionStatus`, where the probe used a
plain `makeCommandBuffer()`. Next experiment: the same triangle through a
command buffer made without the descriptor, as the only difference — which needs
a test-only way to reach a plain command buffer, and is worth thinking about as
a backend-owned "error-options-off" mode rather than a test reaching around. If
that is ruled out too, the remaining differences are the blit encoder the probe
did not have, the `retainedReferences` default, and the readback's
managed-storage path — enumerate before guessing again.

Until the cause is found, `.github/workflows/ci.yml`'s `mtl-e2e` job holds the
faulting draws out **by name** so the rest of the suite stays a gate. **They
come back the moment this is understood** — the filter is a quarantine with a
reason, not a concession, and leaving it un-revisited would turn the one real
bug this job found into a permanently green-looking hole. The fifth held-out
test (`a_layer_swapchain_acquires_a_drawable_and_presents_it`) is `#[ignore]`d
for an unrelated reason — a CI container's detached layer vends no drawable —
and the job's filter names it separately so `--run-ignored all` does not sweep
it up. **Real Apple GPUs remain uncovered** — every hosted runner reports
`Apple M1 (Virtual)`, so a hosted green run is not evidence about hardware, and
`docs/plan/09-backends-metal-dx12.md`'s on-hardware smoke stays on the list, as
does `run-mtl-e2e.sh` on a real Mac.

Two lessons from the investigation, worth keeping: **a diagnostic that can fail
can destroy the evidence it was added to collect** (the fault reporter panicked
inside the `debugSignposts` binding and substituted its own failure for the GPU
fault it existed to report), and **a diagnostic tells you what it measured on
the machine it ran on** — the device name `Apple Paravirtual device` is printed
by every hosted image, including the two that execute shaders fine.

## What MTL5 left open on the swapchain

- **`nextDrawable` and `presentDrawable:` are proven by nothing automated.**
  Everything else in the layer path runs on CI, because a detached
  `CAMetalLayer` needs no window server, `NSView` or run loop; acquiring an
  actual drawable does. That one test is behind `mtl-e2e` and `#[ignore]`, so a
  person on a Mac is the only thing that has ever run it. Whether a detached
  layer vends a drawable on the current hosted image is an open question, cheap
  to settle in a throwaway workflow the way the shader question was.
- **`surface_caps` can never return the `Unsupported` branch its own contract
  requires** — on Metal every device can drive any layer, so the branch is
  unreachable and **the contract is untested here by construction**, not merely
  untested. `crcbl-vk` is where that path is exercised.
- **`CompositeAlpha` offers only `Opaque`.** `CAMetalLayer` has `opaque` and can
  composite with alpha, but nothing verified the non-opaque behaviour, so it is
  not offered rather than offered untested.

## The Win32 shell tests share the desktop, and it keeps costing red builds

`hiding_the_cursor_is_balanced_however_many_times_it_is_asked_for` failed the
`build + test (windows-latest)` leg on 2026-08-05; re-running the same job
passed clean on a macOS-only commit, so it is environmental — the shared
runner's foreground being contended — rather than a defect. The assertion is
doing its job (it fails at the point focus was lost, naming why nothing after it
can be trusted, instead of asserting against an unfocused window), but it is
still a red build on an unrelated change, which trains readers to re-run rather
than read. Options, none taken: retry the focus acquisition with a longer budget
than 8 attempts; move the focus-dependent assertions into the feature-gated e2e
suite where `desktop::take_foreground` already pulls the foreground levers; or
mark the test as allowed-to-retry if nextest's retry support is acceptable here.
**Third instance, 2026-08-09, and it is a _different test_:**
`win32::shell::tests::confining_the_pointer_clips_it_and_losing_focus_gives_the_desktop_back`
failed on `assert!(shell.window_state(window).focused)` — the assertion right
after `make_foreground` + `send_focus`, before the confine is even attempted. So
this is not one brittle test but the whole class the backlog already names:
`ClipCursor` and `SetCursorPos` are foreground-only, several tests arrange the
foreground to use them, and a shared runner does not always grant it.

It failed on a **revert commit**, whose code was green two commits earlier —
which is about as clean a demonstration as this gets that it is environmental
rather than a defect. A re-run of the same job was taken to unblock `main`, and
that is the third time a re-run has stood in for a decision.

**The decision is overdue, and the options have not changed:** retry the
foreground acquisition with a longer budget; move the focus-dependent assertions
into the feature-gated e2e suite where `desktop::take_foreground` already pulls
the levers; or allow this specific test a retry. Doing nothing means every
unrelated commit carries a chance of a red Windows leg, which trains readers to
re-run rather than read — the exact habit that makes a real failure invisible.

**Fourth instance, 2026-08-09, and it is a different _resource_:**
`win32::shell::tests::an_empty_offer_empties_the_clipboard_and_an_empty_payload_does_not`
failed on `assert!(!clipboard_is_open())`. So the class is wider than focus —
these tests use **shared desktop resources** (the foreground, the clipboard) on
a runner that contends for them.

**The clipboard half is fixed.** That assertion sat at the end of a test and
meant "our code closed the clipboard", while `clipboard_is_open()` was
`!GetOpenClipboardWindow().is_null()` — whether **any process** holds it. Its
scope was wider than its intent, so a foreign process failed it while our code
was correct. `clipboard_held_by(hwnd)` replaces it, every caller now asks the
narrower question, and that is both non-flaky and a **stronger** assertion. The
"nothing is open before we start" precondition was deleted rather than narrowed:
that test is _about_ contention — it asserts the open was not refused — so a
foreign holder is the case its retry budget exists for, not a reason to fail
before starting.

**The focus half is not fixed** and remains the open decision below.

Two failures in one session, both on commits that had nothing to do with
windowing (a `Revert` and a mesh-shader reland), both cleared by re-running the
same job unchanged.

**Wants a decision rather than another re-run.**

## What MTL6 settled, and what it leaves for a decision

Metal's last planned slice. **The backend still reports Tier B**, and the reason
moved rather than went away:

- **Needs the user: `dispatch` is blocked on the seam, not on Metal.**
  `MTLComputeCommandEncoder` is otherwise ready. Metal takes
  `threadsPerThreadgroup` at `dispatchThreadgroups:threadsPerThreadgroup:`, but
  SPIR-V, DXIL and WGSL bake the workgroup size **into the shader**, so MSL
  declares it nowhere and `ComputePipelineDesc` carries no field for it. There
  is no number the backend could pass that is not a guess about the kernel, and
  a wrong one runs the shader with the wrong thread count rather than failing.
  **Resolved**: `ComputePipelineDesc` carries `workgroup_size`, sourced from the
  `WORKGROUP_SIZE` constant `crcbl-shaders` publishes beside each compute
  shader, so no caller restates a number the `.slang` already declares.
- **Needs the user: `block2` is now a direct dependency of `crcbl-mtl`.**
  `objc2-metal` types the `addPresentedHandler:` parameter as a
  `block2::DynBlock` and re-exports nothing, so the callback the seam needs
  cannot be written without naming the type. The case is argued next to the edge
  in `crates/crcbl-mtl/Cargo.toml`. It adds no package to the graph — `block2`
  was already in `Cargo.lock` as an `objc2` sibling — but a direct dependency is
  a decision, so it is flagged here for ratification rather than kept silently.
- **`DESCRIPTOR_INDEXING` was withdrawn, deliberately.** Bind groups exist as
  flat argument tables now, so there is no runtime-sized array;
  `create_bind_group_layout` refuses every `BindingFlags`, and a backend
  refusing them must not report the feature. Nothing above the seam is blocked;
  it returns with argument buffers, which need Slang to emit
  argument-buffer-shaped MSL — if the flag is wanted back, the honest route is
  scheduling that shader work, not flipping the bit.
- **`DRAW_INDIRECT_COUNT` is unreachable in this backend's shape.** The count
  lives in GPU memory, and Metal's only execution that reads one is
  `executeCommandsInBuffer:indirectBuffer:indirectBufferOffset:` over an
  `MTLIndirectCommandBuffer` whose commands must **already exist** — from the
  CPU, which does not know GPU-side draw arguments, or from a compute kernel,
  which would have to run before the render encoder was opened. The emulation
  (issue `max_draw_count` draws) is silently wrong. Closing this needs either
  deferred command recording or a seam that hands the backend its indirect work
  before the pass opens.

  **Superseded 2026-08-09, and independently corroborated.** "Metal stays Tier
  B" no longer means anything — there are no tiers, per
  `docs/plan/39-capabilities.md`. Metal reports the flag clear and the renderer
  selects another `GeometryPath`; with mesh shaders as the primary path this
  affects only the fallback. And the finding is not a `crcbl-mtl` limitation:
  `wgpu-types` documents `MULTI_DRAW_INDIRECT_COUNT` as DX12 and Vulkan only,
  and `wgpu-hal`'s Metal backend contains no multi-draw code at all. Two
  implementations, one answer. The seam-reshape option stays on the table and is
  logged under _User decisions_ at the top of this file.

- **A partially filled bind group leaves its unfilled argument-table slots
  holding whatever the previous bind put there.** Not checked, because
  `update_bind_group` makes create-then-fill a legal pattern. Vulkan leaves the
  same hazard to its validation layer.

## WARP clears the bindless bar — measured, 2026-08-05

The question this file told the DX12 phase to settle is settled: the
`windows-latest` runner reports
`ResourceBindingTier=3  HighestShaderModel=6.8 sm66-dynamic-resources=yes` for
both the DXGI lists and `EnumWarpAdapter`, and `crcbl_dx12::device`'s
`a_pulled_triangle_is_drawn_and_read_back_texel_by_texel` has since passed there
— so WARP supports SM6.6 dynamic resources **and executes a shader**, which
closes the coverage hole that `windows-latest` has never had golden images or a
render pass: Windows can have a software rasteriser, the way Linux has lavapipe.
What that does not cover is hardware: WARP is one implementation with one set of
tolerances, and no D3D12 code in this workspace has run on a GPU.
`renderer-tier=B` in the run's lines is the backend's own gap — `COMPUTE`,
`TIMELINE_SEMAPHORE` and the two indirect features wait on calls no slice has
written.

Deferred inside DX4, each with what it would take:

- **Compute pipelines.** `create_compute_pipeline` still refuses. The compute
  half is a `D3D12_COMPUTE_PIPELINE_STATE_DESC` over the same root signature
  plus `SetComputeRootSignature`/`SetComputeRootDescriptorTable`, which are the
  compute twins of calls `bind_group` already makes.
- **Indexed and indirect draws.** `bind_index_buffer`, `draw_indexed` and the
  four indirect entry points refuse. Indexed needs `IASetIndexBuffer` and a
  `D3D12_INDEX_BUFFER_VIEW`; indirect needs an `ID3D12CommandSignature`, which
  is a per-argument-layout object with no counterpart in the seam.
- **Dynamic offsets.** `create_bind_group_layout` refuses a `dynamic` binding by
  name. A descriptor table has no offset to apply — D3D12's answer is a _root_
  CBV/SRV/UAV carrying a raw GPU address, which changes the root parameter type
  for every binding in the set rather than adding to it.
- **Push constants / root constants.** D3D12 _has_ the feature; what is missing
  is knowing which root parameter slot the committed DXIL expects one at — the
  same gap `crcbl-mtl` names for `setVertexBytes:`.
- **Register-space mapping is verified for set 0 only.** Every committed shader
  declares `[[vk::binding(N, 0)]]`. **Settled by measurement, and the
  expectation was wrong**: `[[vk::binding]]` is Vulkan-only, and `dxc` numbers
  each register class from zero in declaration order across the whole source,
  all in space 0 — `sprite`'s set-1 texture is `t1` in space 0, not `t0` in
  space 1. Read out of the `PSV0` resource table of every committed container. A
  multi-set layout is still only checked by that artifact test, never by a
  driver.
- **The shader-visible descriptor heaps do not grow.** One heap per type at a
  fixed capacity, and `HalError::OutOfDeviceMemory` past it, because a bind
  group's GPU handle is an address inside the heap it came from. A real
  suballocator is a slice of its own.

Two things DX1 decided that a later slice may have to undo:

- **`DESCRIPTOR_INDEXING` is reported ahead of a call** — the opposite of what
  `crcbl-mtl` ended up doing, deliberate because `adapters()` is where the WARP
  question is asked, so the flag has to be derivable before any device exists.
  The binding slice must withdraw it if D3D12 bind groups cannot deliver a
  runtime-sized array, exactly as Metal's did.
- **`driver` comes from `CheckInterfaceSupport(IID_IDXGIDevice)`**, documented
  as a Direct3D 10 interface check, with a fallback string when it refuses. WARP
  is the adapter most likely to refuse it; if the CI line shows the fallback on
  real hardware too, the field needs a different source.

**Do not write a LUID into code or an assertion.** DXGI's `AdapterLuid` is
per-boot — two CI runs reported different LUIDs for the same two adapters. It is
an identity _within_ one enumeration and nothing more: fit for de-duplicating a
list, unfit for a fixture, a golden value or a comparison across runs.

## `Format::ALL` cannot be made airtight on stable

The seam owns the canonical format list as `Format::ALL` in `crcbl-hal::format`,
and all three backends — `crcbl-vk`, `crcbl-dx12` and `crcbl-mtl` — now drive
their injectivity tests off it rather than a copy each kept beside the mapping.
What is left is one thing.

**The list is hand-maintained, and the gap that remains is a variant appended to
the enum and not to `ALL`.** `the_format_table_is_in_declaration_order` compares
each entry's discriminant against its index, so an entry inserted or duplicated
anywhere but the very end is caught. An appended one is not, and **nothing on
stable Rust can count an enum's variants**. Closing it properly needs either a
declarative macro generating the enum and the list from one source, or a
proc-macro dependency — and a new dependency is the user's call, so this is
recorded rather than decided. Both `ALL`'s doc comment and the test say this
outright rather than implying full coverage.

The same shape applies to `Command`'s list in `crcbl-hal/src/null/record.rs`,
except that there an exhaustive `match` in the test forces an author who adds a
variant to visit the file — which is the mitigation `Format` cannot have,
because its list lives in a different crate from its consumers.

## `crcbl-vk` does not enforce cross-kind pass scoping, where `null` does

`begin_compute_pass` checks only whether a compute pass is already open, and
`begin_render_pass` only whether a render pass is — so a compute pass opened
_inside_ a render pass, or the reverse, is accepted. `dispatch` checks no scope
at all. The null recorder rejects every one of those as `NestedPass` or
`OutsidePass`.

This may be deliberate: the seam says a backend **may assume** the scoping rules
hold, which makes the null backend the strict reference and `crcbl-vk` the
permissive one. So no behaviour was changed and no test asserts the absence of a
check — that would pin a gap in place. Recorded because it is the second place
the mock is stricter than the backend it models, after the cross-instance
surface bug, and that pattern is worth watching rather than rediscovering.

The illegal _commands_ are still caught by the validation layer at record time.
The illegal _pass bookkeeping_ is caught nowhere.

## The Tier B arms of the indirect-draw tests have never run

`draw_indirect_count` and `draw_indexed_indirect_count` have a Tier A path and a
fallback, and the tests cover both arms and assert which one they took. But
lavapipe on Mesa 26.1.6 reports **tier A**, so on this machine and in CI the
fallback is compiled and unrun. Reaching it needs a genuinely Tier B driver, and
we do not currently have one anywhere — CI's "software rasteriser" leg is no
longer the Tier B leg it was assumed to be.

One arm was salvaged: the `update_bind_group` refusal is a layout rule rather
than a tier rule, so that test runs its refusal path on Tier A devices too.

## Modules of `--test vk_e2e` need `#[path]`, which is not obvious

`crates/crcbl-vk/tests/vk_e2e.rs` declares its modules with
`#[path = "vk_e2e/<name>.rs"]`. Without it a crate root resolves `mod foo;`
beside itself — `tests/foo.rs` — and Cargo would then compile that file as its
own separate test binary. Verified: plain `mod harness;` fails with E0583 naming
`tests/harness.rs`. The alternative Cargo supports is `tests/vk_e2e/main.rs` as
the target root, which needs no `#[path]` at all; it was not taken because the
slice's brief pinned `tests/vk_e2e.rs` as the root file.

## Vulkan's cross-submission barriers are unverified on this machine

`run-vk-e2e.sh` reports its own reach, and on a local run it prints
`sync-validation reach: record-time=yes one-submission=yes cross-submission=no`.
So a green local run against radv says nothing about a missing barrier _between_
submissions, which is the class every missing cross-frame barrier falls into.
CI's layer configuration has caught one that a local run cannot see.

Neither environment subsumes the other: the local run has a real driver, a
discrete GPU and a real async-compute queue that CI has never had; CI has the
cross-submission checking. Treat "green locally" as insufficient for anything
touching barriers, and rely on `cargo nextest run -p crcbl-render` for that
class — it compiles consecutive frames against one pool and needs no layer.

### Two things DX1 decided that a later slice may have to undo

- **`DESCRIPTOR_INDEXING` is reported ahead of a call**, which is the opposite
  of what `crcbl-mtl` ended up doing. It is deliberate and the reversal
  condition is written into `adapter.rs`: the binding slice must withdraw it if
  D3D12 bind groups cannot deliver a runtime-sized array, exactly as Metal's
  did. The reason for the asymmetry is that `adapters()` is _where the WARP
  question is asked_, so the flag has to be derivable before any device exists.
- **`driver` comes from `CheckInterfaceSupport(IID_IDXGIDevice)`**, which is
  documented as a Direct3D 10 interface check, with a fallback string when it
  refuses. WARP is the adapter most likely to refuse it. If the CI line shows
  the fallback on real hardware too, the field needs a different source.

## What DX2 left open

The device-and-resources slice. Everything below is in `crates/crcbl-dx12`.

- **An image view's format must equal its image's on DX12, and the seam says it
  need not.** `create_image_view` refuses a differing `ImageViewDesc::format`
  outright, so the sRGB reinterpretation `crcbl-hal` documents — and `crcbl-mtl`
  delivers — is unavailable on this backend. D3D12 permits the cast only from a
  typeless resource, which costs compression on every render target, or where
  `CastingFullyTypedFormatSupported` is reported, which would make the seam's
  promise depend on the machine. Both were declined for those reasons and the
  argument is in the code. **This wants a decision before the bind-group
  slice**: either the seam narrows its promise, or DX12 pays one of the two
  costs. A sampled _depth_ image is not affected — it is stored typeless with
  the depth-stencil view and the shader view each naming a concrete format.

- **`create_image` does not check `mip_levels` against the extent.** More mips
  than the extent admits reaches D3D12 as `E_INVALIDARG` and surfaces as
  `HalError::Backend` rather than the `InvalidDescriptor` it is. Every other
  descriptor rule in the slice is checked up front precisely because the
  `Create*View` calls return `void`; this one was left to the runtime.

- **`max_sample_count` and `max_storage_buffer_range` are still at the seam's
  floor.** Both need per-format `CheckFeatureSupport` queries, which the format
  table DX2 added now makes possible — the reason `adapter.rs` gives for
  deferring them no longer holds.

- **`device.rs`'s implementation half is still over the size a module should
  reach.** The seam DX2 named — the descriptor validation, `check_image`,
  `check_view_type` and `build_views` — was split into `validate.rs` on
  2026-08-07 as a move, and the file is still the implementation half of the
  device. The next seam is not named; `crcbl-mtl`'s device.rs carries the same
  warning with its own candidates (the pools, the create/destroy pairs,
  submission, readback).

- **The slice runs on WARP and nowhere else.** Its whole suite passed on
  `windows-latest` on the first attempt, which is a behavioural result and not a
  compile: `D3D12CreateDevice`, the DIRECT queue, `CreateCommittedResource`, the
  four `Create*View` calls, the descriptor-heap strides, `Map`/`Unmap` and the
  fence wait all did what the code assumed. **What that does not cover is
  hardware.** WARP is one implementation with one set of tolerances, and the
  `Create*View` calls return `void` — a descriptor a real driver rejects and
  WARP accepts fails as a dead view, silently, on the machine nobody tested. No
  D3D12 code in this workspace has run on a GPU.

- **The concurrent-`wait_idle` ordering is not provable by test.**
  `concurrent_waits_each_signal_once_and_all_return` pins that every call
  signals exactly once, which is deterministic. It does not pin that the signals
  reach the queue in increasing order — that is what the lock on
  `DeviceInner::idle_value` exists for, and its failure mode is a hang caught by
  `slow-timeout`, not a red assertion. A passing run is not evidence the race
  cannot happen.

## What DX3 left open

The command-list, render-pass and clear slice. All in `crates/crcbl-dx12`.

- **`StoreOp::Discard` is recorded as a store.** `OMSetRenderTargets` has no
  store op; the API that has one is
  `ID3D12GraphicsCommandList4::BeginRenderPass`, plus `DiscardResource` with its
  own state constraints. Correct but slower than it needs to be, and
  `crcbl_dx12::command`'s module docs say what it does rather than what it
  should do.

- **Read-only depth attachments are refused.** `create_image_view` builds one
  writable DSV per view, and the seam has no field asking for
  `D3D12_DSV_FLAG_READ_ONLY_DEPTH`, so a view cannot know which pass will only
  read it. **Needs a decision**: a second descriptor per depth view, or a field
  on `ImageViewDesc`.

- **Descriptor-heap slots are not retired, and the belief behind that is
  unverified.** The encoder retains an attachment's _resource_ but not its
  RTV/DSV slot, on the reading that D3D12 consumes those descriptors at record
  time. If that is wrong, a `destroy_image_view` followed by a new view while
  work is in flight overwrites a descriptor the GPU is still reading. Nothing
  tests it and WARP without the debug layer may not object either way.

- **`pipeline_barrier` skips transitions on host-visible buffers**, because
  upload and readback heap resources are pinned to one state for their lifetime
  and recording a transition on them is illegal. The seam has no vocabulary for
  that, and neither Vulkan nor Metal needs it. There is **no test**: no
  deterministic observable for a skipped barrier was found.

- **`ReadbackState::Pending` is never exercised.** Every test waits before
  requesting, so `poll_readback` is `Ready` on its first poll. Forcing `Pending`
  needs work genuinely in flight, which is a race rather than a test.

- **The retire queue's "does not release early" half has no device-level test.**
  After `submit` the fence may already have passed, so asserting `pending() > 0`
  would be racy. The device tests pin the two deterministic halves — the bytes
  arrive despite a destroy, and the queue drains to empty when idle — and the
  _ordering_ is pinned by `retire.rs`'s pure unit tests instead.

- **A WARP that hangs while executing still fails as a timeout, not a named
  stage.** The test helper panics with `stage=finish`, `stage=submit` or
  `stage=wait_idle`, and the readback poll has its own deadline — but a hang
  inside execution blocks in `wait_idle`'s `INFINITE` fence wait, so it surfaces
  as nextest's SLOW-then-SIGKILL. The test name identifies it; the stage marker
  does not.

- **`device.rs` grew again.** The descriptor-validation split DX3 was waiting
  for landed on 2026-08-07 (see the DX2 entry above); the file is still the
  implementation half of the device.

## What the DX12 swapchain and present-feedback slice left open

Surfaces, a flip-model swapchain, acquire/present/reconfigure, and
`Device::wait_until_presented`. `crcbl_dx12::present` holds everything that is
arithmetic (host-testable, and its tests run on any `cargo test`);
`crcbl_dx12::swapchain` holds everything that needs DXGI.

- **Whether the pacing wait genuinely blocks is measured, not asserted.**
  `a_windowed_swapchain_presents_paces_and_resizes_on_a_real_hwnd` prints
  whether each `wait_until_presented` returned or lapsed, and accepts either,
  because a window nobody is looking at is a state the compositor may retire
  frames differently for and the seam calls `SurfaceError::Timeout` expected
  traffic. Any _other_ error fails. Closing this needs a run whose output is
  read: if the waits return promptly there, the tolerance can become an
  assertion.

- **Whose handle `GetFrameLatencyWaitableObject` returns is unsettled**, so it
  is never closed. `SwapchainEntry::waitable` argues the asymmetry — a double
  `CloseHandle` is a process fault, a leaked handle is bounded — and records
  that `wgpu-hal` 29's D3D12 backend does not close it either (checked in the
  vendored source, not recalled). Settling it means reading the current DXGI
  documentation for that method; if the caller owns it, `destroy_swapchain` and
  `reconfigure_swapchain` are the two places that have to close it.

- **A present id is matched against a count, and the mapping is deliberately
  coarse.** DXGI's waitable object answers "fewer than `SetMaximumFrameLatency`
  presents are outstanding" and carries no id, so `PresentLedger` records only
  the highest id this swapchain object was given and a wait for any id at or
  below it blocks **once**. The seam licenses exactly this — its guarantee is
  "the weakest of the three" and it names this shape — but the consequence is
  worth writing down: a caller asking about a frame far back blocks as long as
  one asking about the frame before last, and the first `frame_latency` waits on
  a fresh swapchain return immediately because the object starts signalled that
  many times. An exact id→completion map would need a bounded ring of in-flight
  ids and a soundness argument about how many presents DXGI can have
  outstanding; it was **considered and declined** for this slice as machinery
  ahead of a caller that needs it.

- **A command buffer the caller has not destroyed blocks a resize.** The encoder
  retains every resource it records against, so a `CommandBufferHandle` still in
  the caller's hand holds a reference to a back buffer and `ResizeBuffers`
  refuses. `reconfigure_swapchain` waits for the queue and destroys the
  swapchain's own views and images, which is everything it can reach; the rest
  is the caller obeying the seam's existing rule about destroying finished
  command buffers. Nothing tests it — the test helper destroys its command
  buffer every frame — and the failure is a DXGI refusal with no field named,
  which is the class of thing `crcbl_dx12::swapchain`'s `check` otherwise turns
  into something readable.

- **`reconfigure_swapchain` refuses a format change**, by name. `ResizeBuffers`
  does take a new format, but the entry's `format` is what the views are built
  from and what a _later_ reconfigure resizes with, so changing it means
  threading the new format through the failure path too. Destroy and recreate
  works today.

- **Offscreen surfaces are still refused.** `SurfaceTarget::Offscreen` names an
  unwritten slice rather than a permanent refusal, so `crcbl screenshot` and the
  golden-image e2e cannot reach D3D12 — which is the same gap that keeps this
  backend out of `crcbl`'s registry. A ring of plain images through the same
  acquire/present path is what `crcbl-vk` and `crcbl-mtl` both build.

- **The sRGB-through-the-view path is the one place this backend performs the
  differing-format cast `create_image_view` refuses**, and it is legal only
  because a flip-model back buffer is the case D3D12 permits it on. DX2's entry
  about that refusal is unaffected — it still applies to every image a caller
  creates — but the two now disagree in the same crate, and the "needs a
  decision before the bind-group slice" note should be read with this in mind.

- **`MakeWindowAssociation(DXGI_MWA_NO_ALT_ENTER)` is called per swapchain, on
  the caller's window.** It stops DXGI's own message hook taking Alt+Enter into
  a fullscreen transition nothing above the seam can see. It is a window-global
  side effect a HAL backend arguably should not have; nothing above the seam
  asked for it and nothing can turn it off.

- **Nothing above the seam uses any of this yet.** No crate outside `crcbl-dx12`
  names `Dx12Instance`, so the backend is not in `crcbl`'s registry and no
  `crcbl-shell` window has ever been handed to it. Wiring it up is its own slice
  and would make the win32 shell e2e a real end-to-end D3D12 path.

## Raised 2026-08-09 and not finished

New gaps and deferrals from the planning session. The plan docs carry the
decisions; these are the things nobody has done.

### Deferred: browser multiplayer over WebRTC

**The only route that survives the no-infrastructure constraint**, and it is
recorded rather than refused so the decision is reopenable.

Data channels with **manually exchanged connection codes**: peer A creates the
connection, waits for ICE gathering to complete so candidates are embedded in
the SDP, and the compressed base64 of that is a "code" pasted to peer B, who
answers with one of their own. No signalling server. It maps onto topic 23's
channel semantics **better than WebSocket would have** — DataChannel offers both
ordered-reliable and unordered-unreliable, so the unreliable channel survives.

Against it: a third transport to maintain; a JS shim owning `RTCPeerConnection`
(the same `extern "C"` shape `crcbl-audio`'s web module already uses, so no
`wasm-bindgen` in any crate); a code that is hundreds of characters rather than
a room code; two round trips of copy-paste with both players in live contact
elsewhere; two peers realistically, since full mesh is N(N−1) exchanges; STUN
needed off-LAN and TURN needed behind symmetric NAT, which is the part that
costs money. Free public STUN is plentiful and the Open Relay Project offers a
free TURN tier.

**Do not fold it into bracket** — manual pairing is the antithesis of
matchmaking. If it is ever built it wants its own small sample whose subject is
the transport seam over a third transport shape.

### Coverage gaps and unbuilt things the plan assumes

- **Persistent mapped buffers are a native-only design principle**, and the
  browser path had no stated answer until now. `00-overview.md`'s first core
  principle names them alongside bindless and multi-draw-indirect; `wgpu`
  exposes `MAPPABLE_PRIMARY_BUFFERS` on native only. Every browser upload is a
  staging copy. Nobody has measured what that costs.
- **Wasm modules lose NaN canonicalization and fuel in a browser.** The
  equivalence gate ("bit-identical native _and in-browser_") is unguarded
  against NaN payload divergence, and hostile-module containment has no browser
  equivalent. Survivable because untrusted modules run server-side on native; a
  browser-hosted single-player game with mods has no containment. See
  `16-wasm-modules.md`.
- **MSL is validated by nothing, anywhere.** `spirv-val` runs on the SPIR-V;
  WGSL, MSL and DXIL are unchecked. MSL cannot be checked off macOS at all —
  `xcrun metal` is macOS-only, `newLibraryWithSource:` needs a device, and no
  open-source tool parses MSL. `xcrun metal` on the existing macOS CI leg is the
  cheap fix and is not done.
- **`crcbl-render` has no render-scale or upscale path**, though
  `15-windowing.md` locks borderless as "internal render target upscaled to the
  native surface" and `18-render-features.md` orders the post chain around it.
  `ShellCaps::HW_UPSCALE` exists and nothing can ask for it. This is a locked
  display mode with its renderer half missing.
- **L0's character controller and static trimesh/heightfield colliders do not
  exist.** `05-physics.md` puts both in L0 (MVP); the ROADMAP marks "P3 L0" done
  against a narrower list. towers demands both.
- **`crcbl-audio` has no bus graph and no limiter**, though `13-audio.md`
  specifies `master ← sfx/music/ui/voice` with per-bus gain and a soft-knee
  limiter, and its delivery table puts buses in P4A. Mix snapshots and ducking
  at P10 depend on them.
- **No golden audio buffers exist**, though the exit criterion asks for one per
  sample that emits sound and asteroids and horde both synthesise
  deterministically from fixed seeds.
- **The transcendental policy is two conflicting policies.** `05-physics.md`
  requires the `libm` crate; `13-audio.md` requires own polynomial
  approximations plus a CI deny. Neither exists. `libm` would be a new
  dependency and therefore a user decision.
- **`DeviceId` is per-kind on every backend**, which blocks local-multiplayer
  device assignment that `19-input.md` says is supported "from day one". A test
  asserting two devices are distinguishable would pass vacuously.
- **`21-jobs.md`'s threaded-wasm finding is not reproducible today**: it needs
  `rust-src` on `nightly-2026-07-02`. Unblock with
  `rustup component add rust-src --toolchain nightly-2026-07-02-x86_64-unknown-linux-gnu`.
- **Sample `web.rs` doc comments are stale.** horde, asteroids and flappy still
  narrate "the fourth copy … four call sites to migrate" though
  `crates/crcbl/src/web.rs` closed S1B finding 2. Four passages in horde, two in
  asteroids, one in flappy. Code comments, not docs, so they are not covered by
  the plan sweep.

### Owed by the shader guardrails

- **The differential render gate is still vk↔wgpu only.** Rule 5 asks for every
  backend; the gate now covers three scenes (cube, sprite, ui) across two, which
  closes the shaders with a history of divergence and leaves **Metal and D3D12
  entirely outside it**. A `sprite.slang` or `ui.slang` that means something
  different on MSL or DXIL would not be caught by anything. Both are blocked on
  the same prerequisites as everything else on those backends: `crcbl-mtl`'s
  draw tests are quarantined on a GPU hang, and `crcbl-dx12` refuses offscreen
  surfaces so it cannot read a frame back at all.

- **The cross-backend CI job's timeout was left at 30 minutes while its work
  tripled** (4 renders to 12). The renders are seconds on lavapipe and the
  compile dominates, but that is a local timing judgement, not a runner
  measurement. If that job ever times out, this is the first thing to look at
  rather than the last.

- **The declaration-order lint is stricter than the rule it guards.** Metal
  assigns indices per argument _table_; the lint asserts one global ascending
  order across all sets. So it can ask for a move that would have been harmless
  — swapping two resources in different tables trips it without changing any
  Metal index. Deliberate and documented in the module header: the per-table
  rule needs the lint to model Slang's table assignment, which is more of
  Slang's behaviour than is worth encoding for a guard whose false positives
  cost one declaration move. Reopen if a real shader finds it costly.

### Metal compute works, confirmed on hardware

`ComputePipelineDesc` carries `workgroup_size`, `crcbl-mtl` implements
`bind_compute_pipeline`/`dispatch`/`dispatch_indirect`, and the macOS CI job ran
all three new tests on a real device:
`a_compute_dispatch_writes_the_values_it_ was_asked_for`,
`an_indirect_dispatch_reads_its_workgroup_count_from_the_buffer` and
`the_compute_pass_opens_an_encoder_and_its_calls_fail_only_as_themselves` all
PASS (112 tests run, 6 skipped). **Compute is no longer a Vulkan-and-wgpu
capability.** `indirect_count` is a separate Metal refusal and still stands.

The 6 skipped are the pre-existing draw tests that fault on that runner
(excluded by name in `.github/workflows/ci.yml`) — unrelated to compute, but
worth knowing the device is not fully healthy before reading any green macOS
run.

**A wrong workgroup size is caught on Vulkan and nowhere else.**
`crcbl_vk::spirv::require_workgroup_size` reads `OpExecutionMode … LocalSize`
and refuses a descriptor that disagrees with the shader. Metal cannot (MSL
declares no thread count, which is why the field exists) and wgpu keeps no
module source after `create_shader_module`. Safe only while every compute shader
is also run under Vulkan, which is true today and will not always be.

### Metal draws hang, and it is our bug, not the runner

**Corrected 2026-08-10.** An earlier version of this entry concluded the blocker
was the runner's virtualised GPU. That was wrong, and the evidence against it
was already in `.github/workflows/ci.yml`: **run 31037470086 ran a compute
dispatch and two triangle draws from a standalone Swift script on this same
`macos-26-arm64` image, and all three were correct.** The device draws. Ours
does not.

What `crates/crcbl/tests/render_e2e.rs` measured on 878f582, drawing one frame
through `ForwardRenderer`:

```
Caused GPU Hang Error (00000003:kIOGPUCommandBufferCallbackErrorHang)
[MTLCommandBufferErrorDomain 2] on `Apple Paravirtual device`;
encoders in recorded order: `cull` completed, `draw-args` completed,
`forward` completed, `tonemap` completed, `crcbl copies` completed
```

Same signature as the four draw tests already quarantined in the `mtl-e2e` job:
every encoder reports `completed`, none faulted, and the command buffer reports
a hang anyway. **Compute is fine** — both compute passes completed here, and the
dispatch and indirect-dispatch tests pass on this runner. So the fault is
specific to this backend's _draw_ command stream.

Two candidates are now dead, both killed by tests that were already green:

- **Render-target format** — run 31080128007 ran the RGBA twin unfiltered and it
  faulted byte-identically.
- **The error-options command-buffer descriptor** —
  `a_render_pass_clear_reads_back_the_exact_texels` is unfiltered, passes, and
  goes through the same `fault::command_buffer` path with
  `MTLCommandBufferErrorOptionEncoderExecutionStatus`, a render encoder, a blit
  encoder after it on the same buffer, the same `submit` and the same
  `HostReadback` poll.

What is left is the encoder calls that exist only between `begin_render_pass`
and `end_render_pass` on a draw. **Every candidate named so far is now dead,
including the leading one.**

- **Render-target format** — the RGBA twin faulted byte-identically (run
  31080128007).
- **The error-options command-buffer descriptor** —
  `a_render_pass_clear_reads_back_the_exact_texels` is unfiltered, passes, and
  goes through the same `fault::command_buffer` path, render encoder, blit
  encoder after it, `submit` and `HostReadback` poll.
- **The long draw forms** — killed by experiment on `326c751`. `crcbl-mtl`
  emitted `drawPrimitives:vertexStart:vertexCount:` for a single instance from
  zero (the spelling the working Swift probe used) and four quarantined draw
  tests were released. **All four hung identically**, `canvas` and
  `crcbl copies` both `completed`. The code is reverted; do not spend another
  run on it.

**What is left.** A render-pass clear succeeds and a draw does not, on the same
device, through the same submit and readback. So the fault is in something only
a draw sets up between `begin_render_pass` and `end_render_pass`: the render
pipeline state object itself, the bind groups a draw needs, or the
viewport/scissor. Nothing has been eliminated inside that set.

The other half of the old candidate is still untested and cheap:
`crcbl_mtl::adapter::features_of` reports `MULTI_DRAW_INDIRECT` and
`INDIRECT_FIRST_INSTANCE` **unconditionally, with no `supportsFamily:` query**.
That cannot affect a plain triangle's encoding, so it is unlikely to be this bug
— but it is an unbacked capability claim regardless, and the kind this session
has already found twice elsewhere.

A useful next experiment, if someone wants one: bisect toward the Swift probe.
It set up a pipeline state and drew; ours does the same plus bind groups and a
viewport. Removing our extras one at a time, on a device that faults in under a
second, converges faster than reasoning about it does.

### Settled: `setDepthStencilState(nil)` hung every Metal draw

**Found by bisect, fixed in `8e40f55`.** For months every draw `crcbl-mtl`
recorded hung on GitHub's macOS runner with
`kIOGPUCommandBufferCallbackErrorHang` while render-pass clears succeeded, and
six tests were quarantined for it. Two hypotheses were wrong before this one.

The final round: ten probes, each the known-good hand-encoded pass plus exactly
one call, with a known-red and a known-green control. **7 passed, 3 failed**,
and the three failures are precisely the ones passing `nil` to
`setDepthStencilState:`. Its twin — same selector, real state object — passed,
as did `setCullMode`, `setFrontFacingWinding`, `setTriangleFillMode`,
`setDepthClipMode` and `setDepthBias:slopeScale:clamp:` individually.

**The fix makes `None` unrepresentable** rather than substituting at the bind
site: a pipeline without depth-stencil state holds a default object the device
builds once at open, so nothing in the crate can produce nil and the type says
so. Every descriptor field is set explicitly — `Always`, no depth write, `Keep`
on all three stencil outcomes — because `objc2-metal` is a generated binding
that documents no defaults, and guessing them would trade a hang for wrong
pictures.

Things worth carrying out of this investigation:

- **Three hypotheses, two wrong, and both wrong ones were "what's left standing"
  arguments.** The render-target format and the long draw forms were each the
  last candidate after eliminating others. What settled it was a _controlled
  comparison_ — one call reproducing the hang and its near-identical variant not
  — rather than an elimination.
- **The bug was invisible to every picture-based test by construction.** All six
  replay calls are image-neutral for a pipeline with no culling and no depth
  attachment. No golden could ever have caught it; only a device that faults.
- **Carry a known-red and a known-green control in any bisect** whose baseline
  would otherwise be a previous log. Without them "everything passed" cannot be
  distinguished from "the runner changed".
- The probes were deleted once they answered. A diagnostic that keeps running
  after it has reported is noise in the next run's signal.

**Unverified at time of writing**: the five released draw tests have not run
since the fix. If they pass, `crcbl-mtl` has working draws in CI for the first
time and the Metal arm of `render_e2e.rs` becomes worth wiring.

### Vulkan on Windows: the loader ignores its environment when elevated

**The cause, in the loader's own words** (`44bdf32`, with `VK_LOADER_DEBUG=all`
finally on the right job):

```
[Vulkan Loader] INFO: Loader is running with elevated permissions.
                      Environment variable VK_DRIVER_FILES will be ignored
                      … VK_ICD_FILENAMES will be ignored
                      … VK_ADD_DRIVER_FILES will be ignored
                      … VK_LAYER_PATH will be ignored
[Vulkan Loader] ERROR | DRIVER: vkCreateInstance: Found no drivers!
```

**GitHub's Windows runners run elevated, and the Vulkan loader discards every
environment-variable driver and layer path when the process has elevated
privileges** — deliberately, so a lower-privileged caller cannot inject a DLL
into one. `VK_DRIVER_FILES` was set correctly the entire time and the loader was
throwing it away by design. No shell and no path form was ever going to work:
neither the `cygpath -w` fix nor the move to `pwsh` could have mattered.

**The fix is registry registration** — `HKLM\SOFTWARE\Khronos\Vulkan\Drivers`
for the ICD and `…\ExplicitLayers` for the validation layer, each a `DWORD 0`
named by the manifest's full path. That is where a normally-installed driver
registers itself and what an elevated loader still reads.

**`CRCBL_VK_EXPECT_ADAPTER` is now the only thing that can prove which driver
answered**, since the pin no longer works through the environment. It was worth
building for exactly this.

Three rounds of diagnosis went to two causes that were real but not sufficient
(the `C:/…` path form; variables not crossing from Git Bash) and one that was
never measured at all — see the retraction below. **The loader could have said
this on round one.** `VK_LOADER_DEBUG` cost one line and answered immediately
once it was set on the job being debugged.

**Retracted, and the error was mine:** an earlier version of this entry
concluded "a loader ignoring its own debug switch is not reading its
environment". The switch had been inserted by matching the first step named
`Run the suite against lavapipe`, and both the Linux and Windows jobs have a
step with that name, so it landed on Linux. That conclusion was drawn from a
variable never set on the job it described.

**Still unknown after this fix:** whether the goldens hold.
`Tolerance::RASTERISER` was calibrated radv-versus-one-lavapipe and this is a
second, Windows Mesa build — unmeasured between two lavapipes.

**Owed:** `run-vk-e2e.ps1` and `run-vk-e2e.sh` are two harnesses over one suite,
and their guards are duplicated knowledge that will drift.

### Metal draw coverage in CI: what the ecosystem does

Researched 2026-08-10, because "is this just us?" was worth answering before
buying hardware. It is a real and widely-hit gap, but **it is not our failure**
— see the entry above.

- **GitHub's own position**: "Add support for Metal in macOS images" is an open
  discussion; a GitHub staff reply says _"There is no ETA for now but it's on
  our radar."_ Real GPU passthrough for hosted macOS runners is an open feature
  request.
- **Godot hit the paravirtual device too**, differently: it aborts with
  `-[AppleParavirtDevice newArgumentEncoderWithLayout:]: unrecognized selector`
  on `Apple Paravirtual device (Apple5)`. Closed unresolved; the reporter asked
  only for a graceful error. So the device is genuinely feature-poor — but ours
  fails on draws it demonstrably supports.
- **The asymmetry that matters for the plan**: Linux and Windows both have
  software rasterisers CI can install — lavapipe, which we already use, and
  **WARP** on Windows. macOS has no equivalent, which is why this gap is
  macOS-shaped rather than general.

**Actionable consequence, cheap:** `crcbl-dx12` has no e2e at all, and the
Windows runner has a real desktop session. **WARP is the D3D12 software
rasteriser that would close that gap the same way lavapipe closes Vulkan's** —
no hardware purchase, no self-hosted runner. That is the better next investment
than a Mac mini, and it is not blocked on anything.

### The render layer runs on Vulkan, wgpu and Metal

**Closed on `eab7b5d`.** `crates/crcbl/tests/render_e2e.rs` draws a frame
through `ForwardRenderer` on whichever backend `CRCBL_GPU` names, and the Metal
arm now runs in CI:

```
metal selected IndirectPerBatch / ArrayPages / Rasterised
device on adapter 0 "Apple Paravirtual device" type=Integrated
golden cube on metal — 256x192: 37860 pixel(s) differ at all (77.0264%),
max channel delta 207, 1 over tolerance (0.0020%), ssim 0.999811
```

Three results in one run:

- **`GeometryPath::IndirectPerBatch` is proven on the backend that selects it.**
  Until now that arm had only run on Vulkan behind a deliberately weakened
  device request, which is a forced selector rather than a degradation.
- **A golden blessed on lavapipe matches Metal inside `Tolerance::RASTERISER`.**
  That tolerance was calibrated for radv-versus-lavapipe; it holds across a
  third, very different rasteriser. One pixel over, out of 49152.
- The Metal arm was blocked only by the nil depth-stencil hang. Fixing that
  unblocked it with no further work.

**What remains uncovered: D3D12.** Its HAL suite passes 155/155 on WARP, but a
frame still dies in `OffscreenSetup::open` — see the offscreen-ring entry. So
the renderer runs on three backends of four.

### Settled: the render layer runs on all four backends

**D3D12 drew the cube frame on `4907b7e`**, and with the tightest golden match
of any backend:

```
dx12 selected IndirectCount / Bindless / Rasterised
device on adapter 0 "Microsoft Basic Render Driver" type=Cpu (CRCBL_ADAPTER=cpu)
golden cube on dx12 — 256x192: max channel delta 1, 0 over tolerance (0.0000%),
ssim 0.999879
```

So `render_e2e.rs` now passes on **Vulkan, native wgpu, Metal and D3D12**, and
the step is a real gate — the `continue-on-error` is gone. One golden, blessed
on lavapipe, matched by four independent implementations.

The two causes, both found by asking the device rather than reasoning about it:

1. **A constant buffer view outran its buffer.** D3D12 requires a CBV's
   `SizeInBytes` be a multiple of 256; `crcbl-dx12` rounded the _view_ up while
   the allocation stayed 16 bytes. The allocation is padded now, only for
   `UNIFORM` usage.
2. **Three draw-generation buffers were on an upload heap and bound writable.**
   D3D12 refuses `ALLOW_UNORDERED_ACCESS` on that heap at creation and pins the
   resource to a state no shader can write from. They are `DeviceLocal` now, and
   the frame zeroes them with a clear dispatch.

**The second was not a D3D12 bug at all** — it was a compromise this file had
already recorded under GPU-driven draw generation, kept because `fill_buffer` is
legal only outside a pass and the graph had no fill step. Vulkan tolerated it
for months. Worth remembering: **a portability compromise that one backend
accepts is not a compromise, it is a latent failure with a delay on it.**

Also settled by that work: a graph-level fill was the obvious fix and the wrong
one. `fill_buffer` is four separate backend promises — Metal repeats a byte,
wgpu clears only to zero, `crcbl-dx12` refuses it entirely — so it would have
moved the blocker one call later. A dispatch's portability is held by
construction.

**Two follow-ups this leaves:**

- ~~The `dx12 e2e (WARP)` job is misnamed~~ — renamed to
  `dx12 e2e (software adapter)`. `CRCBL_ADAPTER=cpu` selects the single
  `DeviceType::Cpu` adapter, and on that runner it is **Microsoft Basic Render
  Driver** rather than WARP. Naming the job after a specific implementation
  claimed something the pin never asked for.
- ~~`crcbl-dx12::fill_buffer` wants recording as a deliberate non-fix~~ — done,
  at the refusal itself. D3D12's fill needs a shader-visible descriptor heap
  this backend does not create, and nothing in the workspace needs it now that
  the counters are cleared by dispatch. A caller who wants it should say why a
  dispatch will not do.

### What WARP has actually proven

Worth separating from what is merely implemented, because this backend is
written blind and only CI ever executes it.

Proven on hardware:

- Compute dispatch, indirect dispatch, and a workgroup size refused for
  disagreeing with the container's `[numthreads]`.
- **Indexed draws, indirect draws, and indirect-count draws reading a GPU-side
  count** — all four passed on `c4e8655`.
- The root-signature register fix, implicitly: `compute_probe`'s pipeline could
  not have been created at all under the old `[[vk::binding]]`-derived rule.

Not proven on any device: dynamic offsets, offscreen surfaces, and a recorded
frame.

**A rot to expect.** `c4e8655` reddened WARP on
`the_slices_that_have_not_arrived_still_refuse_and_name_themselves`, which
asserts the unimplemented calls still answer `Unsupported`. Three of them had
just started working. That test's own comment calls it "the half that rots" —
every DX12 slice from here has to prune it, and the failure is legible when it
happens.

Also: `crcbl-dx12`'s crate docs still say bind groups and pipelines refuse,
which the code contradicts.

### Owed by GPU-driven draw generation

§3.3 is wired end to end — `cull` → `draw-args` → `forward` — and every golden
is bit-identical through it. What is not settled:

- **Metal and D3D12 have never run `draw_gen.slang`.** The MSL compiles, its
  bindings land at `buffer(0..8)` in declaration order, and it has no
  module-scope `threadgroup`. Nothing has executed it. Note the arm Metal needs,
  `IndirectPerBatch`, is proven **on Vulkan hardware, not on Metal** — a forced
  selector on one backend is not the same evidence as the backend that actually
  degrades to it.
- **radv appears to ignore `drawCount == 0`.** Seen while falsifying:
  `vkCmdDrawIndexedIndirect` with `draw_count: 0` still drew the geometry, and
  only removing the call blanked the frame. Nothing depends on it — the
  per-batch arm always passes 1 and relies on `instance_count == 0` — but a
  future empty-draw optimisation must not assume the count is honoured. Not
  investigated further; unknown whether it is radv, the loader, or our own
  recording.
- **`GeometryPath::MeshShader` has no tail** (§3.5). It degrades to an indirect
  one and logs, exercised on the null backend only.
- **Per-bucket capacity is the whole instance capacity**, so _N_ buckets cost
  _N_ × 16K × 4 bytes per frame slot. §3.3's own correction wants scene-stat
  sizing plus an overflow counter before the bucket table grows.
- **The mesh→bucket lookup is a linear scan** in `draw_gen.slang` — correct at
  any size, O(buckets) per instance. A mesh→bucket map is what a large table
  wants.
- **The three counters are host-visible only because the seam allows a fill
  outside a pass and the graph has no fill step.** A graph-level fill, or a tiny
  clear dispatch, would let them be device-local.
- **The browser will take `IndirectPerBatch`** — WebGPU has neither indirect
  feature — and that has not been tested there. Native wgpu selects
  `IndirectCount`, so the browser's arm is not the one CI exercises.
- **A golden is not sufficient for this stage.** Breaking `first_index` to zero
  left the cube golden **bit-identical** and was caught only by the argument
  readback. Worth remembering before treating an unchanged picture as proof that
  a draw-generation change was correct.
- Incidental: `mesh.png` has only **4 distinct colours** (flat-shaded faces),
  which is why the cross-path anti-vacuity floor is `> 4` and not the
  cross-backend script's 16.

### Owed by the GPU cull pass

- **The visible list has no consumer**, deliberately: indirect draw generation
  is its own slice. The pass is built in the e2e rather than in
  `ForwardRenderer` precisely because Metal would refuse it in a live frame.
- **Dead instance slots.** `InstancePool::remove` does not rewrite the element
  and the pass iterates `0..instance_count`, so a removed slot is culled on
  stale contents. The liveness bit wants to be `GpuInstance::flags`, which is
  still reserved and defines nothing. Today the count is the caller's problem.
- **No WGSL execution of `cull.slang` anywhere.** It compiles for wgsl and is
  run only on radv and lavapipe; the cross-backend script compares rendered
  scenes and this pass renders nothing. A compute-only differential harness is
  the missing piece, and it will be needed again for every later compute pass.
- `cull.slang` **re-declares** `GpuInstance` and `GpuMesh` because the compile
  script hashes one source per artifact and there is no shared header. A drift
  test compares the field lines of both files; a shared-include mechanism would
  remove the need for it.

### Owed by the GPU mesh table

The table (`MeshPool::table_buffer`) is what §3.3's cull pass will build
indirect draws from, so these two are due before a compute pass walks it, not
after.

- **A mesh id is a bare `u32` with no generation.** Freeing clears the entry to
  the empty range, so a _freed_ id resolves to `index_count == 0` and draws
  nothing. A **reused** slot is the gap: a stale id then names whichever mesh
  took the space, silently and plausibly. `MeshHandle` is the generational type;
  the id that reaches the GPU is not. Fix by putting a generation in the id or
  an epoch word in the entry — the choice interacts with how wide the cull pass
  wants its instance record, so decide them together.
- **The table has no resident bit.** Residency is a CPU gate today
  (`MeshPool::mesh`, `MeshPool::table_index`), which suffices only because the
  CPU records every draw. Once a compute pass reads the table itself, an entry
  written at upload but not yet flushed is reachable, and `index_count == 0` is
  the only signal it has.
- `crcbl-vk`'s `depth_probe` hand-builds a one-entry mesh table, so any further
  binding added to `mesh.slang` has to be mirrored there. A second hand-built
  copy of a layout is the kind of thing that drifts; worth folding into a shared
  test helper the next time either moves.

### Settled: base vertex and base instance never reach a shader

Recorded because it is a rule for every future shader here, and because it is
the first time the differential render gate caught a real divergence rather than
a hypothetical one.

`SV_VertexID` and `SV_InstanceID` mean **different things per target**, measured
rather than assumed: SPIR-V subtracts `BaseVertex`/`BaseInstance` (HLSL's
meaning), DXIL passes them through with D3D12 excluding both bases, and WGSL and
MSL index raw builtins that _include_ them. A pooled mesh at a non-zero base
vertex therefore rendered a correct pyramid through wgpu and a corrupted slab
through Vulkan — one source, two pictures — and `run-cross-backend-e2e.sh`
failed on it at 10.09% of pixels with a structural mismatch.

**The rule: every draw passes zero for both bases, and the real values arrive in
a per-draw constants block.** Zero is the one value all four lowerings agree on,
so nothing in the picture depends on how a target lowers a builtin.
`sprite.slang` reached the same conclusion independently for its own case; this
makes it the pattern rather than one shader's workaround.

The gate only caught it because `Scene::Cube` was changed to draw a second mesh
at a non-zero base. **A path nothing exercises is a path the gate cannot see** —
which is the general form of this and worth remembering before trusting any
green run over content that does not use the feature.

### Owed by the mesh-shader path

- **Slang's Metal backend materialises every global shader parameter in every
  entry point, and that once broke `main`.** Worth keeping because it constrains
  how any future shader here is written, and because the first diagnosis was
  wrong.

  Slang 2026.14 builds a `KernelContext` struct holding a pointer to every
  global shader parameter, and materialises it — with **all** of its globals —
  in every entry point, used or not. `mesh_shader.slang` had no global shader
  parameter until a `StructuredBuffer<Vertex>` was added for the vertex pull;
  that switched the machinery on, which dragged the module-scope
  `groupshared Amplification` into all four entry points including the fragment
  one, and `xcrun metal` refuses a threadgroup declaration in a fragment
  function. Fixed by making the payload a **local** in `taskMain` so no
  `groupshared` global exists for the context to carry. Slang lowers that to the
  same `TaskPayloadWorkgroupEXT` storage class in SPIR-V and to a stack payload
  `dxc` accepts.

  **The hypothesis recorded here first — that the fragment entry point sharing
  the vertex struct was the cause — was wrong**, and was falsified by trying it:
  a separate fragment input struct leaves the declaration exactly where it was.
  So were reordering the globals and function-local `groupshared` (which Slang
  rejects outright, `E31201`). Recorded so the wrong lead is not followed twice.

  **The rule that falls out:** a module-scope `groupshared` in a file that also
  has any global shader parameter is invalid on Metal. Nothing checks this — the
  `xcrun metal` CI step catches it after the fact, which is how it was found.

- **Not reproduced: a lavapipe SIGSEGV in CI that does not occur locally.**
  `retire::two_submissions_referencing_one_destroyed_buffer_keep_it_alive`
  segfaulted on CI's lavapipe during the same run, in a test unrelated to mesh
  shaders. The same commit runs 62/62 on this machine's lavapipe (Mesa 26.1.6,
  LLVM 22.1.8), so CI's Mesa build differs. Unexplained, seen once. If it recurs
  it is a real bug and the driver version is the first thing to compare — **CI's
  lavapipe is as unpinned as its `spirv-val` was.**

- **Nothing can bind a descriptor to the mesh stage yet.** `ShaderStages::MESH`
  and `TASK` exist and map correctly, but no bind-group layout or push-constant
  range names them, and no backend polices a layout naming a mesh stage on a
  device without the capability. That is why the first mesh shader hardcodes its
  three vertices instead of pulling them from a storage buffer the way
  `triangle.slang` does — pulling needs mesh-stage visibility, which obliges
  every backend to police the flag. **This is the next slice**, and it is the
  prerequisite for a mesh shader that reads real geometry.

  Note the flags are deliberately outside `ShaderStages::GRAPHICS` and `ALL`:
  Vulkan refuses `MESH_BIT_EXT` in a layout on a device without `meshShader`, so
  a composite carrying them would break every existing layout on most devices.

- **Meshlets need a mesh asset system that does not exist.** §3.5 wants clusters
  with bounds and normal cones baked from a mesh; `crcbl-scene` is a stub and
  the only mesh in the tree is a hardcoded cube. The builder, the cluster
  hierarchy and amplification-stage culling are all blocked behind topic 6's
  asset work — building any of them now would be building ahead of a consumer.

- **`crcbl-vk`'s absent-capability refusal is unexercised.** Both drivers here
  report `MESH_SHADER`, so only the null backend takes that arm. The e2e
  falsifies what any device can refuse instead — a mesh pipeline naming a
  fragment entry point as its mesh stage.

- **Metal and D3D12 have the stages and the committed artifacts, and neither
  loads them.** `msl/mesh_shader.metal` and the `ms_6_6`/`as_6_6` DXIL are built
  and validated; what is missing is `MTLMeshRenderPipelineDescriptor` and the
  D3D12 pipeline-state stream. Both refuse the entry points by name today.

### Owed by the capability work (P7)

- **The null backend cannot express several device states, so the engine's
  handling of them is untestable without a GPU.** Found while giving the log
  lines tests. Each is a `crcbl-hal` null limitation rather than an engine one,
  and closing any of them is a small change to `crates/crcbl-hal/src/null/`:
  `SurfaceCaps::current_extent` is hardcoded `None`, so the "surface reports X
  but the shell configured Y" path is unreachable; `NullInstance::adapters`
  returns exactly one adapter, so "no adapter can serve this surface" is
  unreachable; `AcquiredFrame::suboptimal` is hardcoded `false`, so the
  reconfigure-after-present path is unreachable; `wait_until_presented` always
  returns `Ok`, so the lapsed-timeout path is unreachable; and **neither preset
  advertises `PRESENT_FEEDBACK`**, so a device that claims it has to be
  hand-built in the test.

- **The observed half of the pacing line is pinned to `Unknown` in every test.**
  `NullDevice::display_timing` returns `Unknown` unconditionally, and no driver
  in this project has ever answered anything else — so `Fixed`, `Variable` and
  `Stepped` reach `settle_pacing` nowhere. The tests distinguish outcomes by the
  _requested_ and _resulting_ halves instead, which proves the line is not a
  constant but leaves three of four observed arms unexercised end to end. Same
  missing machine as the `VK_EXT_present_timing` entry elsewhere in this file.

- **No local driver reports the mesh/ray capabilities absent, so the degradation
  path is unexercised here.** `crcbl-vk` now reports `MESH_SHADER`,
  `TASK_SHADER`, `RAY_QUERY`, `RAY_TRACING_PIPELINE` and
  `ACCELERATION_STRUCTURE` from the real device — and **lavapipe reports all
  five too** (Mesa 26.1 implements `VK_EXT_mesh_shader` and the whole ray
  tracing set), which was not expected: the software rasteriser was assumed to
  be the negative case and is not. So on this machine and in CI's `vk e2e`,
  `GeometryPath::MeshShader` and `LightingPath::RayTraced` are what gets
  selected, and the fallbacks are compiled and unrun on the Vulkan backend. The
  unit tests cover the mapping; nothing covers a real device that lacks the
  extensions. Same shape as the Tier B indirect-draw arms recorded elsewhere in
  this file — an assumption about which driver is the weak one, that turned out
  to be wrong.

- **`accelerationStructure` is enabled without forcing `bufferDeviceAddress`.**
  Checked against the installed `validusage.json`: no VUID requires them
  co-enabled, and it is validation-clean on radv. Recorded because an
  acceleration-structure _build_ slice will need `BUFFER_DEVICE_ADDRESS`
  regardless — build infos take device addresses — so the pairing question
  returns the moment anything uses the capability rather than reporting it.

- **Only the downgrade line is asserted; the engine's other decision lines are
  not.** `crcbl-core`'s logger can now be captured in a test
  (`crcbl_core::log::capture`), and `crcbl`'s device-open path asserts both that
  it names a downgrade and that it stays silent when nothing was lost. The
  mechanism exists for the rest and nothing uses it yet: the pacing resolution
  (`asked for Auto, pacing Vsync`), the present-feedback capability line, and
  Win32's `exact refresh for …` are each the **only** record that a decision was
  taken, and each could be deleted with every test staying green.

  Two limits of the capture, both documented in the code: it is thread-local, so
  a test cannot see what a worker thread logged; and a `capture()` racing an
  `init_logging()` from outside the API can still lose its probe. Neither occurs
  in the workspace today.

- **"Tier" vocabulary survives in inline comments across the backends.** The
  type is gone and the doc comments are cleaned, but narrative comments,
  `.expect()` strings and test names still say Tier A/B in `crcbl-render`,
  `crcbl-wgpu`, `crcbl-mtl`, `crcbl-vk` and `crcbl-dx12`. Two caveats before
  anyone sweeps it: `crcbl-render`'s references were about the
  `ui.slang`/`ui_tier_b.slang` **shader permutation**, which no longer exists —
  that fork is deleted, so those are now simply dead words; and much of
  `crcbl-dx12`'s is real D3D12 `ResourceBindingTier` vocabulary that must stay.

- **Every path selector value must be executed by something.** A `GeometryPath`,
  `BindingModel` or `LightingPath` value no device in CI selects is compiled and
  unrun. The existing instance is the Tier B arm of the indirect-draw tests:
  lavapipe reports the higher capability, so the fallback has never run
  anywhere. This is the risk most likely to be realised.
- **The downgrade log line must be asserted**, not admired — an e2e that forces
  a feature off has to see the engine say so.
- **`required` must be shown to fail.** A device request naming a feature the
  null backend does not report must produce the named error; a `required` that
  cannot fail is not a gate.

### The two PowerShell harnesses keep their own copy of the nextest summary guard

Every bash e2e harness now sources `tools/nextest-summary.sh` for the one thing
they all have to do — strip the colour, find nextest's summary line, tell a
complete run from the `<ran>/<total>` shape nextest prints for one it cancelled,
fail on zero. That is eight copies collapsed to one, after five of them had
drifted into reading `2/15 tests run` as a healthy fifteen.

`crates/crcbl-vk/tests/run-vk-e2e.ps1` and
`crates/crcbl-shell/tests/run-win32-e2e.ps1` cannot source a bash file, so each
keeps a PowerShell transcription of the same logic. Both are **correct today** —
their `(?:(\d+)/)?(\d+) tests? run` is where the bash fix came from — and both
are now the only place the guard can drift, since nothing compares them against
the shared one and no fix to it reaches them. `run-vk-e2e.ps1` and
`run-vk-e2e.sh` are two harnesses over the same `crcbl-vk` `vk_e2e` suite, which
is the sharpest version of the problem: one suite, two guards, one of them
shared and one of them a copy.

**The option, stated and not taken: make the Windows harnesses bash and delete
the `.ps1` copies.** It is demonstrably possible for at least one of them —
`crates/crcbl-dx12/tests/run-dx12-e2e.sh` runs on `windows-latest` today, and
its "Why bash, when `run-win32-e2e.ps1` argued for PowerShell" section argues
that the Git Bash that image ships has `mktemp`, `tee`, `sed` and `grep`, that
GitHub Actions' `shell: bash` selects it, and that what bash buys is guards
`shellcheck` and a Linux developer can exercise, which matters because nobody on
this team has a Windows machine.

Against it, from those files' own headers:

- `run-win32-e2e.ps1` chose `pwsh` because it starts nothing and needs nothing a
  Windows shell lacks: `windows-latest` boots into a session with a window
  station and a desktop, so unlike the Wayland and X11 harnesses there is no
  compositor to launch, and `mkfifo` and `trap EXIT` — the two things those
  harnesses need bash for — mean nothing on Windows. Porting it would buy the
  shared guard and nothing else.
- `run-vk-e2e.ps1`'s reason is a measurement rather than a preference, and it is
  the strong one. `run-vk-e2e.sh` **was** the Windows harness, for three CI
  runs, and the Vulkan loader never saw its environment. Two real causes were
  found and fixed on the way (the manifest reaching the loader in Git Bash's
  `C:/…` form, and exported variables not reaching a native child), and the
  loader still reported
  `windows_read_data_files_in_registry: Registry lookup failed`. Its conclusion
  is that a native process launching a native process is the only shape with no
  environment translation in it.

**Correction to the premise this entry was raised under:** `run-vk-e2e.ps1` does
_not_ register an ICD in `HKLM`. What it does is resolve `CRCBL_VK_ICD` to a
native path, fill in `VK_DRIVER_FILES`/`VK_ICD_FILENAMES` when nobody else set
them, walk `PATH` for `vulkan-1.dll`, dump those variables one process from the
loader, and run `vulkaninfo`. The `HKLM:\SOFTWARE\Khronos\Vulkan\Drivers` and
`…\ExplicitLayers` writes are in `.github/workflows/ci.yml`, in the job step
that extracts lavapipe — and stay PowerShell whichever shell the harness is
written in.

That correction cuts both ways, which is why the call needs a measurement rather
than a re-read. The workflow's own comment says GitHub's Windows runners are
elevated and that the loader **discards every environment path when the process
is elevated**, deliberately, which is why HKLM is what actually selects lavapipe
there and `CRCBL_VK_EXPECT_ADAPTER` is the only thing that proves which driver
answered. If that holds, the environment-translation argument for `pwsh` is no
longer load-bearing on the runner it was written for: nothing the harness
exports selects the driver either way. Verifying that is a CI run, not a
reading.

What was verified here: the two `.ps1` guards' regexes and their cancelled and
zero branches; that `run-dx12-e2e.sh` is bash on `windows-latest` in `ci.yml`;
that the HKLM writes are in the workflow and not in either harness. What was
not: whether `run-vk-e2e.sh` under Git Bash would pass on that runner today,
which only a CI run can answer.

Related, and also not acted on: `tools/nextest-summary-test.sh` exercises the
shared guard against every summary shape and **nothing runs it**. `ci.yml` has
no shell-lint or script job to hang it on —
`grep -n shellcheck .github/workflows/` matches nothing — and adding one was
outside the paths that slice owned. Until it is wired in, the guard's own test
is a file somebody has to remember to run.

## Test-file names: what the rename slice left, and one rename declined

The naming slice took `docs/plan/12-testing.md`'s "filenames name the subject,
never the taxonomy tier" and applied it to nine files. What it could not close,
and one thing it deliberately did not do:

**`crates/crcbl-shell/tests/appkit_session.rs` is not renamed to `appkit_e2e.rs`
— considered and declined.** By subject it is the macOS member of the family
`wayland_e2e.rs` / `x11_e2e.rs` / `win32_e2e.rs` belong to: a real WindowServer,
a real window, injected input. But in this workspace the `_e2e` suffix carries a
second meaning beyond the subject — every other file wearing it opens with a
crate-level `#![cfg(all(target_os = …, feature = "…-e2e"))]`, carries
`#[ignore]`, and is driven by its own harness script. `appkit_session.rs` has
none of that, and cannot: `.github/workflows/ci.yml`'s AppKit step records that
this target is the AppKit backend's _only_ executable coverage, so putting it
behind a gate would leave the backend with none by default. A name promising a
switch nobody has to throw is the more expensive error — the reader goes looking
for the feature that enables it and concludes it is off. The file's `//!` header
now argues this under "Why it is not called `appkit_e2e`", so the question does
not get re-opened from the filename alone. If the suffix ever stops implying a
gate, the rename becomes correct and the header is where to look.

**Stale path references left behind, all in files that slice did not own.** Each
is prose in a code span, not an intra-doc link, so nothing fails to build and
`cargo doc` stays green — they are simply wrong and will send a reader to a file
that is not there:

- `crates/crcbl-phys/src/broadphase.rs` names `tests/churn.rs` twice (module
  header and the depth-bound doc comment); it is now
  `tests/broadphase_churn.rs`.
- `crates/crcbl-phys/src/forces.rs` names `tests/property.rs`; it is now
  `tests/dynamics.rs`.
- `crates/crcbl/src/engine.rs` names `tests/library_seam.rs` in the doc comment
  about the hand-driven loop; it is now `tests/seam_from_outside.rs`.
- `docs/plan/12-testing.md` names `tests/churn.rs` and `tests/property.rs` in
  its seeded-generator paragraph, and names
  `crates/crcbl-server/tests/integration.rs` as the one file carrying a taxonomy
  tier for a name. That example is now spent — the file is
  `client_server_session.rs` — so the paragraph needs rewriting rather than a
  path substitution, and there is no remaining file in the workspace to point at
  as the counter-example.
- `docs/code-review.md` cites `crates/crcbl-server/tests/integration.rs:15` and
  `crates/crcbl-audio/tests/orbit.rs:191`. That file is a dated record of past
  reviews, so leaving the paths as they were written may be right; the decision
  was not made either way.

**Test _names_ inside these files were not touched.** `docs/plan/12-testing.md`
records six crates as drifted below the prose-sentence-name convention —
`crcbl-ecs`, `crcbl-net`, `crcbl-input`, `crcbl-phys`, `crcbl-audio` and
`crcbl-store`. Two of the renamed files sit in that set and show it:
`crates/crcbl-audio/tests/spatial_chain.rs` still has
`centre_position_is_symmetric` and `right_position_pans_to_right`, and
`orbit_cue_changes_over_time` now names a fixture the file is no longer named
after. Renaming the functions is a separate task and stays unclaimed.

### Stale test-name references left behind by the backend-qualifier rename

`docs/plan/12-testing.md`'s "a test that exists on more than one backend names
the backend or its API" rule has now been applied to `crcbl-vk`, `crcbl-mtl`,
`crcbl-dx12` and `crcbl-wgpu`: 26 verbatim-identical names across two or three
of those crates are gone, along with 12 near-identical pairs whose only
divergence was cosmetic. The rename touched test function names and their doc
comments only — no production code, no signature, no behaviour — plus the three
places a test name is a string outside a test body (`.github/workflows/ci.yml`'s
Metal job comments and `crates/crcbl-mtl/tests/run-mtl-e2e.sh`'s usage example).
`crates/crcbl-dx12/tests/run-dx12-e2e.sh` needed no change:
`the_pinned_adapter_opens_a_device_and_names_itself` and
`a_fresh_device_says_whether_it_is_validated_and_is_not_already_removed` were
never duplicates, and `.github/workflows/ci.yml`'s
`not test(a_layer_swapchain_acquires_a_drawable_and_presents_it)` filter names a
test that is unique to `crcbl-mtl` and was likewise untouched.

Prose in two files still names the old identifiers, and neither was in the
rename's paths:

- `docs/plan/12-testing.md`'s naming section argues the rule by citing
  `a_device_outlives_the_instance_that_made_it` and
  `a_compute_dispatch_writes_the_values_it_was_asked_for` as names existing "in
  three each", and gives a count of twenty-six. Both citations and the count are
  now historical. The paragraph wants rewriting to describe the convention as
  held rather than as owed — the three-way examples are now
  `a_vulkan_/a_metal_/a_d3d12_device_outlives_the_instance_that_made_it` and the
  matching compute-dispatch trio.
- `docs/backlog.md` itself names old identifiers in entries that predate the
  rename: `a_render_pass_clear_reads_back_the_exact_texels` (now
  `a_metal_`/`a_d3d12_`-prefixed, and the entry means the Metal one),
  `the_slices_that_have_not_arrived_still_refuse_and_name_themselves`,
  `an_indirect_dispatch_reads_its_workgroup_count_from_the_buffer`,
  `an_indexed_draw_reads_the_bound_index_range`,
  `a_triangle_draw_paints_the_centre_and_leaves_the_corners_clear`,
  `a_pulled_triangle_is_drawn_and_read_back_texel_by_texel` and
  `reusing_an_offscreen_ring_image_is_ordered_against_the_frame_that_had_it`.
  Each now carries a backend word.

### Two backend test-name pairs deliberately left diverging

Both are cases where renaming would break something outside the rename's paths,
not cases where the convention was judged not to apply:

- `an_indirect_calls_stride_is_only_checked_when_it_is_used` (`crcbl-dx12`,
  `src/draw.rs`) against
  `an_indirect_draws_stride_is_only_checked_when_it_is_used` (`crcbl-mtl`,
  `src/draw.rs`). They differ by one word — `calls` against `draws` — so a grep
  for either misses the other. The Metal name is quoted in
  `docs/plan/12-testing.md` as an exemplar of the prose-sentence rule, so
  renaming it strands that citation.
- `reported_limits_come_from_d3d12_and_agree_with_the_features` against
  `reported_limits_come_from_the_device_and_agree_with_the_features`
  (`crcbl-mtl`). The Metal side never says "Metal". `docs/plan/12-testing.md`
  presents this exact pair as an example of the convention being followed, so it
  is left as the doc describes it; the honest reading is that the D3D12 side
  names its API and the Metal side does not.

### Exact test-name collisions still open between non-backend crates

Measured over every crate under `crates/` with the same detector used for the
backend rename — a name defined under `#[test]` in more than one crate. Eight
remain, all outside the four backend crates and so outside that task's paths:

- `debug_format` in `crcbl-client`, `crcbl-ecs`, `crcbl-phys`, `crcbl-server`
  and `crcbl-ui` — five copies, and the only name in the workspace that is not a
  prose sentence at all.
- `automatic_selection_reports_what_it_tried` (`crcbl`, `crcbl-shell`),
  `messages_name_the_specific_problem` (`crcbl-hal`, `crcbl-shell`),
  `the_seam_is_also_usable_generically` (`crcbl-hal`, `crcbl-shell`),
  `placeholder_compatibility_is_refused` (`crcbl-client`, `crcbl-server`),
  `sweep_removes_dead_entities` (`crcbl-ecs`, `crcbl-phys`),
  `the_entry_points_answer_zero_until_a_source_is_installed` (`crcbl-audio`,
  `crcbl-store`), `the_frames_corners_do_not_grow_with_the_menu`
  (`crcbl-render`, `crcbl-ui`).

`a_device_outlives_the_instance_that_made_it` in
`crates/crcbl-hal/tests/seam_from_outside.rs` is deliberately **not** in that
list any more and should stay unqualified: it is the seam's own obligation
checked on `NullBackend` from outside the crate, which `docs/plan/12-testing.md`
calls the backend-agnostic shape. It was a fifth copy of that name until the
three backend copies took their prefixes; the bare name now belongs to the one
test that is genuinely about no backend.

### The first-triangle milestone is four different claims, not one written four ways

Recorded because the opposite is the obvious guess and unifying the four names
would flatten a real difference. All four were read end to end:

- `crcbl-mtl`'s
  `a_metal_triangle_draw_paints_the_centre_and_leaves_the_corners_clear` draws a
  hand-written MSL triangle with **no bindings at all** — geometry from
  `[[vertex_id]]`, a fragment shader returning the `INK` literal — and
  `assert_ink_triangle` checks that the centre texel is exactly the ink colour,
  all four corners exactly the clear, and every other texel is one of those two.
  `ink_msl`'s own doc says why it is not the engine's shader: pulling vertices
  needs bind groups.
- `crcbl-dx12`'s
  `a_pulled_triangle_is_drawn_by_d3d12_and_read_back_texel_by_texel` runs the
  engine's `crcbl_shaders::triangle` through an SRV over a storage buffer, and
  `assert_triangle_drawn` asserts three fixed probes are red-, blue- and
  green-dominant and that each probe's channels sum to full scale — the
  barycentric property that catches a wrong element stride.
- `crcbl-vk`'s `a_triangle_pulled_from_a_vulkan_storage_buffer_reaches_memory`
  makes the same pulled-vertex claim but derives its probes from the geometry
  (75% of the way from centroid to each vertex) rather than fixing pixel
  coordinates, and adds a centre-blend assertion for interpolation.
- `crcbl-vk`'s `the_vulkan_triangle_matches_its_golden_image` is the P1
  golden-image gate against `tests/golden/triangle.png` at
  `Tolerance::RASTERISER`.

So the flat-colour coverage check, the two dominance checks and the golden
compare are four distinct assertions; only the backend qualifier was missing and
only that was added. What is genuinely absent is a golden-image gate on Metal
and D3D12 — `crates/crcbl/tests/render_e2e.rs` is the backend-agnostic golden
and covers the cube scene, not the triangle.

## What the mtl/dx12 `#[ignore]` placement slice left open (2026-08-10)

The slice marked every `crcbl-mtl` and `crcbl-dx12` test that opens a real
device, instance or adapter with `#[ignore]`, and narrowed both harnesses from
running the whole crate to `--run-ignored only`, so the count each guards on is
the number of device tests. `docs/plan/12-testing.md`'s placement section
records what landed. What it did not settle:

### The device-test counts are a source reading, not a measured run

`run-mtl-e2e.sh` should select 71 tests and `run-dx12-e2e.sh` 73 (the mtl CI job
filters `a_layer_swapchain_acquires_a_drawable_and_presents_it` out, so it
should report 70). **Those numbers come from classifying the test bodies, not
from watching nextest select them** — nothing on this team's machines executes
either crate, and `cargo nextest list --run-ignored only` for both is empty on
Linux because every device test lives in a `#[cfg(target_os = …)]` module Linux
does not compile. The first `mtl e2e` and `dx12 e2e` runs after this are the
first observation of the real counts; a number well below these means an
`#[ignore]` did not land where it was thought to, and a number above means the
classification missed a device path.

The classification traced `instance::tests::open`, `device::tests::open_device`
and `instance::tests::pinned_adapter` transitively through each module's local
helpers, then every test the trace called pure was read. **A test that reaches a
device by some route none of those three names would have been missed**, and
nothing in the tree would report it: it would simply keep running in the
`--workspace --all-features` sweep on the macOS/Windows runners and fail there
rather than in the harness.

`crates/crcbl-dx12/tests/run-dx12-e2e.sh`'s CI job header still records "the HAL
suite above passed **155/155 on WARP**" from runs on `dc846ff` and `0354eec`.
That is a dated account and correct for those runs; a reader comparing it
against the ~73 the harness will now print should read the drop as the selection
narrowing, not as tests disappearing.

### The workspace sweep deliberately did not gain `--run-ignored all`

The slice brief asked for `--run-ignored all` on both
`cargo nextest run --workspace --all-features --locked --profile ci` lines in
`ci.yml`, so the ordinary sweeps would keep running the newly-ignored tests.
**That was not done, on `docs/plan/12-testing.md`'s authority**, which reserves
that run as the one that deliberately does not execute the ignored set so it
stays green on a machine with no compositor and no GPU. Measured rather than
argued: `cargo nextest list --workspace --all-features --run-ignored only` on
Linux selects 159 tests — `crcbl-vk::vk_e2e` 73, `crcbl-shell::wayland_e2e` 37,
`crcbl-shell::x11_e2e` 32, `crcbl-wgpu::wgpu_e2e` 15, and one each from
`crcbl::render_e2e` and `crcbl-cli::cli_e2e`. `--all-features` compiles the
Vulkan, wgpu and render suites on the macOS and Windows runners too, so the flag
would have those jobs open a Vulkan device on a runner with no loader.

What that leaves is a **pairing nothing enforces**: `crcbl-mtl`'s device
coverage now exists only in the `mtl e2e` job and `crcbl-dx12`'s only in the
`dx12 e2e` job plus `test-cross-platform`'s "DX12 adapter report" step. Delete
or disable any of those and the crate's device tests stop running everywhere,
with every remaining job still green — the harnesses' zero-count guards cannot
fire for a harness nobody invoked. The obvious fix is a required-job list the
workflow checks against itself; it was out of scope here.

### The pure/device split is not checked by anything

Nothing fails when a newly written test opens a device and forgets `#[ignore]`,
or when an existing one stops needing a device and keeps it. The first shape is
caught late (the test runs in the sweep on a runner without the hardware and
fails); the second is never caught at all — the test simply stops being run by
anything except the harness. A lint would have to know which helpers open a
device, which is the same trace this slice did by hand; recorded as a gap rather
than attempted.

### What the non-backend test-name rename left behind

The prose-sentence rule in `docs/plan/12-testing.md` was applied to every test
name of three words or fewer outside the backend crates. Measured with a
`#[test]`/`#[tokio::test]` extractor over the whole tree: 138 such names before,
1 after. The `debug_format` bullet and the `ray_misses_aabb`, `decode_empty`,
`decode_truncated`, `debug_output` and `debug_formatting` copies named in "Exact
test-name collisions still open between non-backend crates" above are resolved;
the rest of that entry's list still stands.

- **`orbit_integration_deterministic` (`crcbl-audio`, `tests/spatial_chain.rs`)
  was deliberately not renamed.** It is cited by name in `docs/code-review.md`,
  which was outside this slice's paths, so renaming it would strand that
  citation. The citation is already stale in two other ways and is worth fixing
  together with the rename: it gives the path as
  `crates/crcbl-audio/tests/orbit.rs:191` (the file is now
  `tests/spatial_chain.rs`), and its finding — that the test XORs per-block
  hashes, which is order-insensitive — no longer holds, because the test feeds
  one hasher in block order and asserts that the reversed event order hashes
  differently.
- **`crcbl-wgpu/src/conv.rs`'s `format_mapping_round_trips` remains**, a
  three-word name naming the function under test rather than a claim. `crcbl-vk`
  and `crcbl-mtl` state the same contract as
  `no_two_formats_share_a_metal_format` and its siblings, so the wgpu name
  should follow that shape. It was outside this slice's paths.
- **The four `the_workgroup_size_matches_the_numthreads_the_shader_declares`
  copies and the three `the_params_block_matches_the_offsets_slangc_emits`
  copies in `crcbl-shaders` now name their shader** — read end to end first, and
  they are one contract instantiated per shader, not one claim written several
  ways: each reads its own `.slang` source, or asserts its own `PARAMS_SIZE` and
  field offsets. That is the same situation `docs/plan/12-testing.md` describes
  for the backend crates, where the fix is to differ by the one word that names
  what is under test. Renaming them meant editing the `PARAMS_SIZE` doc comments
  in `cull.rs`, `clear_counters.rs` and `draw_gen.rs`, which cite the test by
  name; those three doc-comment lines are the only non-test text the rename
  touched.
- **Same-crate duplicate names in `crcbl-render` were left alone**:
  `a_pool_leaks_nothing` and
  `a_pool_error_flattens_into_the_seams_without_losing_its_message` each exist
  in both `instance_pool.rs` and `mesh_pool.rs`. Both are prose sentences
  already, so they were outside the rename's criterion, but a grep for either
  finds two tests over two different pools and nothing in the name says which.
  Naming the pool in each would close it.
- **Not re-examined:** the 4-to-6-word names that already read as claims but are
  thin — `sweep_removes_dead_entities`, `t_values_are_correct`,
  `element_ids_are_preserved` and their neighbours in `crcbl-phys` and
  `crcbl-net`. The rename's cut was at three words, so these were never read;
  whether they state what the body asserts is unmeasured, not judged fine.

### Fixed sleeps left in tests the assert-nothing slice did not own

The slice that gave the assert-nothing tests real assertions removed two fixed
sleeps as part of the work — `jitter_does_not_panic`'s 50 ms in
`crcbl-net/src/condition.rs` and `null_stream_runs_without_error`'s 20 ms in
`crcbl-audio/src/lib.rs`, both now poll-with-deadline. Its brief named those two
tests, so the neighbours that sleep the same way were left alone and are
recorded here rather than left to be re-derived:

- **`source_fill_receives_stereo_buffer`** (`crcbl-audio/src/lib.rs`) sleeps 30
  ms and drops the stream. Its `CheckSource::fill` asserts the rate and the
  buffer shape, so the assertions are real — but they run on the stream's
  polling thread, and a run where the thread never got scheduled inside 30 ms
  executes none of them and still passes. It is the same shape as the loop with
  no count that `the_value_column_starts_past_the_longest_label` had. The fix is
  the one `the_null_stream_fills_its_source_until_it_is_dropped` now uses next
  to it: count the fills in the source, poll for a non-zero count against a
  deadline, and assert the count before dropping the stream.
- **`the_latency_only_constructor_delays_delivery_and_still_delivers`** and
  **`a_message_under_latency_does_not_arrive_until_the_delay_has_passed`**
  (`crcbl-net/src/condition.rs`) sleep past a configured latency and then assert
  delivery. These are the honest use of a sleep — the thing being tested is a
  wall-clock delay — but they still cost their sleep on every run and would fail
  on a machine that stalls past the margin. `ConditionSimulator` schedules
  against `Instant::now()` with no injectable clock, so making them poll would
  mean either a clock seam in the simulator (a production change) or a poll loop
  that spins until the message arrives. Neither was in scope.

Also not touched: `docs/plan/12-testing.md`'s frame-poll rule is prose, and
nothing enforces it. A grep for `thread::sleep` under `crates/*/src` and
`crates/*/tests` is the whole of the available check.

## What the coverage audit found and this session did not fix

Audited 2026-08-10 across five angles: tests that cannot fail, the seam's
documented obligations, backend parity, backend-agnostic coverage, and crates
with thin coverage. The "tests that cannot fail" findings all shipped — see
`git log` for the thirteen. What is below is what did not, each with the
evidence that produced it, so the next session does not re-derive it.

Two results are deliberately recorded as **non**-gaps, because both look like
gaps and re-auditing them costs a day: `crcbl-scene` has zero tests and is
correct — `src/lib.rs` is thirteen lines of doc and no items, and it says the
`Scene` type arrives with its phase. And the "ECS replication roundtrip"
`docs/plan/12-testing.md` asks for exists already, as
`a_lossless_run_leaves_the_clients_state_hash_equal_to_the_servers` in
`crates/crcbl-net/tests/replication.rs`, with loss and reorder variants beside
it.

### Obligations tested on exactly one backend

- **Deferred driver-object destruction** (`crcbl-hal/src/device.rs`, the
  obligation that `destroy_surface` invalidates the handle at once while the
  driver object lives until the last swapchain dies) is properly tested only on
  Vulkan, by `a_surface_with_a_live_swapchain_defers_its_driver_object` and the
  two negatives beside it. `crcbl-dx12` has the survival half
  (`a_dxgi_swapchain_keeps_working_after_its_surface_handle_is_destroyed`) and
  not the deferral half. `crcbl-mtl` has neither:
  `crates/crcbl-mtl/src/swapchain.rs` argues the obligation is discharged more
  simply because Metal has no separate surface object, which is plausible and is
  a claim rather than a test. `crcbl-wgpu` has neither.

- **Clamp-and-report** (a swapchain clamps the shell's requested extent into the
  platform range and reports the result on `AcquiredFrame::extent`) is tested on
  Vulkan and Metal and on neither D3D12 nor wgpu. On D3D12 the platform does pin
  the range on a real `HWND`, so this is a real gap; the fixture
  `a_windowed_swapchain_presents_paces_and_resizes_on_a_real_hwnd` already
  builds the window a test would need.

- **A caller renders at `AcquiredFrame::extent`** is asserted by nothing at all.
  `crates/crcbl-hal/src/swapchain.rs` states it as a caller obligation and says
  using the requested size instead is the bug the field exists to prevent, and
  that it only appears while a window is being dragged. The engine does adopt
  it, but no test drives an acquire whose returned extent differs from the
  requested one and then checks the recorded render area. On the null backend
  the two are always equal, so the bug is structurally invisible there.

### The null backend can be resized and killed, but not clamped

**Both halves of this closed.** `crcbl_hal::null::Recorder` gained
`report_swapchain_out_of_date` and `lose_device` beside the four injectors it
already had, and `crates/crcbl/src/engine.rs` now tests all three of its
out-of-date arms — the acquire, the present and the pacing wait's deliberate
no-op — plus the device-loss policy end to end through `drive`. A
nineteen-strong mutation sweep over the hooks and those arms left no survivor.
So `crcbl-vk` no longer carries the only test of a resize, and "this device is
gone and stays gone" is no longer a state nothing can express.

What is left is the third thing the old entry wanted from one hook and did not
get: **an injector that makes `acquire_next_frame` hand back an extent other
than the one configured.** The seam's obligation 3 says a caller must use the
answer rather than the request, and `NullDevice::acquire_next_frame` says in a
comment that it has no window system to clamp against, so it always answers with
the configured extent. That leaves `GpuContext::acquire`'s
`acquired.extent != self.configured_extent` branch — the one that writes the
compositor's chosen size back into `config` so a later `resize` does not see a
change that is not one — reachable only on a compositor that actually clamps.
This was deliberately not built with the other two: nothing about a clamped
extent is a _failure_, so it does not belong in the fault-injection shape those
two took, and it wants its own decision about whether the recorder holds a clamp
rule or a one-shot override.

### Neither Metal nor D3D12 proves its validation layer caught anything

`crcbl-vk` sets the standard: every test ends in `finish`, which calls
`validation_report().assert_clean()`, and that method fails when the layer was
**not** enabled — because a test that passes for want of a layer proves nothing.
On top of that, `vk_e2e/validation_gate.rs` commits a deliberate violation and
asserts the layer caught it.

- **Metal has no validation at all.** No `MTL_DEBUG_LAYER` or
  `MTL_SHADER_VALIDATION` anywhere in `crates/crcbl-mtl` or the workflows, and
  no `debug.rs` in that crate. Seventy-one Metal device tests currently say
  nothing about API misuse. Metal reports through `MTLCommandBuffer.error`
  rather than a callback, so capturing it needs a small design decision;
  `crates/crcbl-mtl/src/fault.rs` already builds synthetic `NSError`s and is the
  natural home.
- **D3D12 has the machinery and does not assert on it.**
  `crates/crcbl-dx12/src/debug.rs` enables the layer and drains its info queue,
  and one test reads the flag — but no dx12 test asserts a clean report at
  teardown, and there is no deliberate-violation twin. The layer can be on, its
  messages drained, and every one of the seventy-three device tests still green
  with a validation error raised.

### The dedicated cross-backend job still compares two backends of two

**Half of this closed on `41b6e61`**, differently from how it was framed. The
entry used to say MSL and DXIL were compared against nothing, because
`run-cross-backend-e2e.sh` renders every scene on Vulkan and wgpu and on nothing
else. That is no longer the gap: `crates/crcbl/tests/render_e2e.rs` now draws
`Cube`, `Sprite` and `Ui` on whichever backend `CRCBL_GPU` names, so Metal and
D3D12 compare all three against the same lavapipe-blessed references the other
two do. Both matched on their first run, at max channel delta 1.

What is left is narrower and worth stating precisely. The vk-versus-wgpu job
compares two backends' output **to each other**, which catches a divergence
neither has a golden for; the golden path compares each backend to a
_reference_, which catches a backend drifting from what was blessed. Those are
different checks, and only the first is still two-backends-wide. Extending it
would mean running two backends in one process on one machine — which is what
that job does — and no runner has both Metal and D3D12.

So the remaining gap is structural rather than owed: a Metal-versus-D3D12
comparison has nowhere to run. The golden path is the substitute and is already
in place.

### Coverage the testing plan asks for and nothing provides

- **No sample owns a golden frame** — declined rather than owed, and the
  reasoning is under "Declined for now: a golden frame per sample" below rather
  than repeated here.
- **The ECS churn soak with a leak assert does not exist.** The plan asks for it
  by name. Nothing in `crcbl-ecs` spawns and despawns over many ticks and then
  asserts nothing leaked. One seeded loop, no GPU.
- **`crcbl-ui` owes a hit-test grid and has two points.**
  `button_hit_test_inside` and `button_hit_test_outside` exist; the sweep the
  plan names does not.

## Decisions taken 2026-08-10, so they are not re-argued

Each of these was a question the coverage audit raised and left open. They are
answered here rather than carried, with the reasoning, so a later session can
disagree with the argument rather than rediscover the question.

### Decided: `crcbl-wgpu` gets owner tagging, and the wasm build pays for it

The question was whether the browser target should carry the side table the
seam's third obligation requires. **It does.** The cost is one `u64` compare per
handle resolve, against a hash lookup that already happens; that is not a cost
the wasm build needs protecting from, and a seam obligation honoured by three of
four backends is not honoured. Cross-device handle misuse being undefined on
exactly the backend a browser runs is the worst place to have it, not the most
defensible.

Follow whichever existing backend's shape is closest rather than inventing a
fourth spelling.

**Noted while deciding, not acted on:** owner tagging will then exist in four
crates as four hand-written copies of one idea. That is duplicated knowledge and
it will drift. Extracting it into `crcbl-hal` is the obvious move and is
deliberately _not_ being done now — three of the copies work, and rewriting
working backends to host a fourth is scope the task does not need. Revisit if a
fifth backend ever appears, which is the point at which the duplication stops
being tolerable.

### Decided: device loss surfaces, it does not self-heal

The engine will not recreate the device. `HalError::DeviceLost` propagates and
the loop stops with an error naming it.

Recreation means rebuilding every resource the frame graph, the pools and the
renderers hold, on a code path that by construction almost never runs — the
classic shape of a recovery path that is broken when it is finally needed.
Surfacing it is honest, testable in one assertion, and leaves the harder policy
available later for whoever has a real reason to want it. A game that wants to
survive a lost device can restart the engine; nothing in the samples does.

**Implemented and pinned.** `Recorder::lose_device` reports a device as gone and
keeps it gone, and `a_lost_device_stops_the_driven_loop_with_an_error_naming_it`
in `crates/crcbl/src/engine.rs` drives `drive` over a real `GpuContext` on it:
the run ends on the frame that hit the loss, with the driver's own message, with
its frame budget unspent and with no rebuild attempted. The last of those is
asserted off the `hal: reconfiguring the swapchain to ` log line rather than off
the recorder, because a rebuild that failed records no event — so an engine that
never tried and one that tried and was refused look identical in the stream, and
those are exactly the two policies this entry chose between.

### Decided: the four-backend compare is more scenes in `render_e2e`, not a new job

The audit's framing was a cross-platform image compare — bless a shared
reference and have the macOS and Windows jobs compare against it. **That already
exists**: `crates/crcbl/tests/render_e2e.rs` compares against a checked-in
golden blessed on lavapipe, and CI runs it on all four backends. What it does
not do is cover more than one scene.

So the gap is one line of scope, not a new job: `Scene` has `Cube`, `Sprite` and
`Ui`, `render_e2e` draws only `Cube`, and `sprite.slang` and `ui.slang` are the
two shaders that have _actually_ diverged per target in this repo's history. The
fix is to draw all three scenes there and bless two more goldens, which gives
Metal and D3D12 the coverage `run-cross-backend-e2e.sh` gives Vulkan and wgpu.

Cheaper than a new job, reuses the anti-vacuity colour floor and the tolerance
that are already calibrated, and it puts the coverage in the file whose whole
purpose is being backend-agnostic.

### Declined: minimum-count floors on the e2e harnesses

Both backend harnesses now select `--run-ignored only`, so the number they guard
on is the device-test count — 70 on the Metal runner and 73 on the D3D12 one,
measured. The zero-count guard still passes a selection that collapsed from 73
to 3.

A floor would catch that, and it is **not** being added: any threshold below the
real number is arbitrary, and a threshold equal to it fails CI every time a
device test is added, which trains people to bump it without reading. The counts
are printed by both harnesses and visible in the run log, and the classification
that produces them is now documented in `docs/plan/12-testing.md`. Revisit if a
collapse ever actually happens — at that point the floor has evidence behind it
instead of a guess.

### Declined for now: a golden frame per sample

`docs/plan/12-testing.md` asks every sample for a determinism check _and_ a
golden frame. The determinism half is met everywhere. The golden half needs each
sample to expose a screenshot path it does not have, which is a feature in every
sample rather than a test — and the samples are already pinned by replay hashes,
which catch the same regressions a golden would catch for gameplay and catch
them deterministically.

Left undone deliberately. If it is taken up, the cheap version is one shared
`--screenshot` flag in the sample scaffold rather than five implementations.

### The seam does not describe what `wait_semaphores` does with an impossible wait

Found while writing `crcbl-wgpu`'s timeline-semaphore test, 2026-08-10, and
verified against all three implementations.

`crates/crcbl-hal/src/device.rs` documents `wait_semaphores` as returning
`Ok(true)` when the waits are satisfied and `Ok(false)` on timeout — "a timeout
is a normal outcome for a frame-pacing poll, not an error" — with
`InvalidHandle` and `DeviceLost` as the only errors. It says nothing about a
wait on a value **nothing submitted will ever signal**, which is neither
satisfied nor a timeout.

The backends disagree, and both answers are defensible:

- `crates/crcbl-wgpu/src/device.rs` returns `HalError::Unsupported` naming the
  cause, because wgpu has no standalone semaphore that could signal the value
  later, so waiting would hang until the deadline and then lie by calling it a
  timeout. That error is **not in the seam's documented set**.
- `crcbl-mtl` treats the same case as an ordinary unsatisfied wait.

**Decided: the seam grows the third outcome rather than wgpu losing it.**
Failing fast with a reason beats blocking for the full timeout to report a
timeout that was never going to be anything else — a frame-pacing poll wants to
know it asked for something impossible, not to pay the deadline first. So
`wait_semaphores`' docs should name `Unsupported` for an unsatisfiable wait, and
the other backends should adopt it when next touched.

Not done in the slice that found it: changing a seam contract means changing
every implementation of it plus the tests that assert the current behaviour, and
that slice was adding coverage rather than moving a contract. The tests as
written assert each backend's _current_ answer, so whoever takes this will see
exactly which ones move.

### `crcbl-dx12` has no timeline-semaphore test because it has no timeline semaphore

Recorded so it is not mistaken for a coverage gap. `crcbl-vk`, `crcbl-mtl` and
now `crcbl-wgpu` each have
`a_<backend>_timeline_semaphore_signals_from_a_submission_and_the_cpu_sees_it`;
D3D12 has none because the feature is unimplemented there, not because the test
was forgotten.

## `crcbl-wgpu` owner tagging: what the tests do not reach

Obligation 3 is now implemented in `crcbl-wgpu` —
`crates/crcbl-wgpu/src/handle.rs` holds the tag/id pair, and every pool entry is
an `Owned<T>` — so the "Decided: `crcbl-wgpu` gets owner tagging" entry above is
answered. Two things about that work are worth carrying rather than
rediscovering.

**The slot's `u64` cannot separate owners whose tags collide.** Not a defect in
this backend, and true of `crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` for the same
reason: every pool holds exactly one owner's rows, so a foreign handle that gets
past the tag lands on a row the _looking-up_ owner filled, the id agrees, and
the lookup succeeds. The id half is what catches a shared pool and what stops
`handle::remove` from taking a row this owner does not own — that much is
asserted by
`a_wgpu_slot_belongs_to_the_owner_that_filled_it_even_when_two_tags_collide` —
but it is not a second line of defence against a colliding tag, and this session
briefly wrote a test claiming it was before the test failed and said otherwise.
The hole opens only after `OWNER_TAG_COUNT` owners in one process.

**The windowed swapchain path's owner checks are untested.**
`WgpuDevice::swapchain` and `WgpuDevice::surface` funnel through
`dead_or_foreign`, which keeps a dead handle reporting `SurfaceError::Lost`
(what callers retry on) and reports a foreign one as
`SurfaceError::Hal(ForeignObject)`. **Nothing asserts that split.**
`crates/crcbl-wgpu/tests/wgpu_e2e.rs` is offscreen-only by construction and the
harness's Xvfb half runs `apps/sandbox` and `apps/breakout` rather than checking
errors, so no test hands `acquire_next_frame`, `present` or
`reconfigure_swapchain` a swapchain from another device or a surface from
another instance. The three new e2e tests cover buffers, the queue handle and
surfaces at `Instance` level only. Closing it needs either a windowed test
target or an offscreen swapchain crossed between two devices — the latter is
cheap and was simply not in this task's scope.

## What the three-scene `render_e2e` does and does not prove

`docs/backlog.md`'s "Decided: the four-backend compare is more scenes in
`render_e2e`, not a new job" is implemented: `crates/crcbl/tests/render_e2e.rs`
now has one `#[test]` per `Scene` — cube, sprite and UI — each with its own
golden under `crates/crcbl/tests/golden/`. What follows is what that run did not
settle.

**Metal and D3D12 remain unverified.** Both goldens were blessed on lavapipe and
both hold bit-identically on `CRCBL_GPU=vk` and `CRCBL_GPU=wgpu` against the
same ICD, which is the only cross-target evidence obtainable on a Linux machine.
The `mtl-e2e` and `dx12-e2e` jobs are the first time `sprite.slang` and
`ui.slang` will be compared against anything on MSL or DXIL, and nobody has seen
those frames. A large pixel delta there is a tolerance question; a structural
mismatch or a failed slot assertion is a finding about the backend. The
comparison prints enough to tell them apart, and
`every_sprite_slot_is_painted_and_the_gaps_are_not` is the assertion that names
the `SV_InstanceID` failure mode directly rather than as a summary number.

**`Tolerance::RASTERISER::max_failing_ratio` has been tightened from 2% to 0.1%,
and CI is the only thing that can confirm it for Metal and D3D12.** The old
figure was sized against "how many pixels differ at all" while gating "pixels
differing by more than `max_channel_delta`", and the gap was wide enough to pass
a plainly visible recolour of a quarter of one sprite — measured, at 0.7345%.
The new value is derived from the quantity actually gated: every `crcbl-vk`
golden and every `render_e2e` scene reports **0 over tolerance** on vk and wgpu
(re-run locally against lavapipe after the change, 74/74 and 3/3 twice), dx12 on
WARP and metal on a paravirtual device report 0 on all but one, and the
exception is **metal's cube at 2 pixels — 0.0041%**, which the new bound clears
by about 24x. The derivation now lives in `Tolerance::RASTERISER`'s doc comment
and in `crcbl-golden`'s crate docs, and both ends are pinned by
`a_localised_recolour_that_the_old_two_percent_ratio_passed_now_fails` and
`the_worst_measured_cross_backend_frame_still_passes_with_room_to_spare` in
`crates/crcbl-golden/src/compare.rs`.

What is **not** verified: nothing on Metal or D3D12 was re-run, because neither
can run here. Metal's cube is the closest measurement to the new limit of
anything ever taken, and its 2 pixels are wrong by a channel delta of 207, so
`max_failing_ratio` is the only knob that can absorb them — no
`max_channel_delta` worth having would. If that scene ever drifts to 50 failing
pixels at 256x192 it goes red where it used to pass, and that is the intended
behaviour rather than a regression. The `mtl-e2e` and `dx12-e2e` jobs are the
verdict.

**`max_channel_delta` was left at 2, and the local run says it cannot go
lower.** `crcbl-vk`'s `sprite_rotation` golden reports
`125 pixel(s) differ at all (0.2543%), max channel delta 2, 0 over tolerance`
against lavapipe. Dropping the delta to 1 turns those 125 drifting pixels into
125 failing ones — 0.2543%, two and a half times the new ratio — so the two
knobs cannot both be tightened. Considered and declined for that reason, with
the measurement rather than a guess behind it.

**The per-scene anti-vacuity checks are the part with real headroom, and the
colour floors are not.** The three floors are the cross-backend harness's own
`CRCBL_CROSS_MIN_COLORS_*` numbers; measured on the blessed frames at 256x192,
the sprite scene has 17 distinct colours against a floor of 16 and the UI scene
has 7 against 6. One colour of headroom each is deliberate — that is how those
floors were calibrated — but it means a scene edit that removes a colour trips
the floor before it reaches the golden, which is what happened while proving the
checks can fail.

**The four CI steps are still named "Draw a frame through ForwardRenderer".**
Three frames now, through three renderers, and only the cube's goes through
`ForwardRenderer`. The step comments were corrected; the names were left alone
because they are the labels a run's history is read by and nothing greps them.
Rename them if the churn is ever worth it.

## The material table has both halves; what is still missing from a material

`crcbl_render::MaterialTable` is `docs/plan/03-gpu-driven-rendering.md` §3.2's
material table SSBO, `crcbl_shaders::mesh::GpuMaterial` is a row, `mesh.slang`
binding 6 is where the fragment stage reads one, and binding 7 is the
`Texture2DArray` page a row's `base_color_texture` selects a layer of. The
texture-indices half of this entry is done and has been deleted. What is still
deliberately not there, and what it would take:

**One base-colour texture, and no other slot.** A row has a factor and one page
layer. Normal, metallic-roughness and emissive maps are each another `u32` in
`GpuMaterial`, another sample in `fragmentMain` and — for the first one that is
not colour data — another _page_, because an `ArrayPages` page is one image and
one format, and a normal map is linear where a base colour is sRGB. That is the
first thing that makes the single-page shape insufficient, and it should be
where the second page is introduced rather than a generic page manager arriving
ahead of a second caller.

**A page is one image, which is the limit `Bindless` exists to lift.** Every
layer shares an extent, a format and a mip count, so two textures of different
sizes cannot share a page and `crcbl_render::forward` uploads its layers at
build with the extent compiled in. Real content does not look like that. See the
wgpu entry below for what stands between here and the bindless form.

**No mip chain, and the sampler is nearest because of it.** §3.2 makes mip
generation a compute pass of its own and it is not written, so
`upload_texture_layers` uploads `mip_levels: 1` and `forward`'s base-colour
sampler filters nearest — a filtered read of a page with no mips buys a shimmer
rather than a smoother picture. The first minified material texture is what
makes the compute pass worth writing.

**Nothing imports a texture.** `crcbl-scene` parses glTF materials and this
slice did not wire it: the page's layers are two constants in
`crcbl_render::forward` (`UNTEXTURED_TEXELS`, `CHECKER_TEXELS`). Wiring it needs
a decoder, a page allocator and a lifetime story for a layer, which is P9's.

**A material is a start-up write.** `MaterialTable` is one host-visible buffer
with no ring — the mesh table's shape, not `InstancePool`'s — because nothing
rewrites a row between frames. `MaterialTable::set` therefore carries the same
caveat `MeshPool::upload` does, stated in its docs: called while a frame is in
flight it is a read-after-write hazard across submissions. **The first animated
material is what makes this a ring**, and it is the moment
`instance_pool::DirtyRanges` becomes worth sharing, because there would then be
two callers that coalesce runs rather than one that writes single rows.

**Specular stayed a constant.** `mesh.slang`'s `SPECULAR_POWER` and
`SPECULAR_STRENGTH` are documented as belonging in a material and were left
where they are. The reason they were left has now half expired — the fragment
stage does reach the table, so the varying that used to be in the way is paid
for — and what remains is only that nothing varies them. Moving them is two
floats in `GpuMaterial` and a re-bless. `docs/plan/37-materials.md` owns the
shading model they are part of.

## The debug-overlay retrofit: what was rejected, and one engine doc that is now stale

Breakout, flappy and asteroids now contribute `DebugModule` sections
(`BoardStats`, `CourseStats`, `FieldStats`, plus `DebugModule for Audio` on
flappy and asteroids), wired through `HostedGame::debug_sections` the way
horde's `SceneStats` already was. What was considered and left out:

- **Breakout has no audio section.** `breakout::audio::Audio` keeps no counter
  at all — no `plays`, no `dropped` — so a row would have meant adding state to
  the game for the panel's benefit. Rejected on those grounds. If breakout ever
  grows a `plays` vector the way flappy's did, the section is three lines.
- **Ball speed was the only invisible breakout number.** `GameLogic::ball_speed`
  is the difficulty ramp and nothing displayed it; everything else breakout
  knows (score, lives, state, high score) is already in `HudStrings`. A
  `paddle`/`ball x,y` row was considered and dropped as a number the player can
  see.
- **Asteroids does not repeat the wave.** `HudStrings::refresh` already draws
  `Wave: {wave + 1}`, and two numbers on screen under the same word that differ
  by one is worse than one.
- **No entity count for breakout.** It would have meant a new
  `Game::entity_count` accessor, and breakout does not churn: it spawns the grid
  once and despawns bricks until a restart respawns them. Flappy and asteroids
  both already had the accessor because both are churn samples.
- **Four audio modules, four different facts — deliberately not shared.** Horde
  reports `dropped` (it is the only sample with a `MAX_VOICES` cap), flappy two
  cue counts plus live voices, asteroids three cue counts plus whether the held
  engine loop is sounding, breakout nothing. The `label: value` shape is common;
  the knowledge is not, and the samples are separate binaries, so extracting one
  would mean a new crate or a change to `crcbl-ui`.

Two findings that are **not** fixed:

- **`crcbl::engine::HostedGame::debug_sections`'s doc comment is now wrong.** It
  says the empty default "is exactly what four of the five samples want" and
  that "`apps/horde` is the one that does not". Four of the five override it now
  (breakout, flappy, asteroids, horde); sandbox is the only one that does not.
  Left alone because the retrofit was scoped to `apps/**` and the comment is in
  `crates/crcbl/src/engine.rs`.
- **Debug row labels share one namespace across modules, and one collided.**
  `crcbl-render`'s `FrameTimings` renders `timings: pending` while a timestamp
  report is in flight, and asteroids' first draft used `pending` as the label
  for its deferred-despawn count. Nothing detects that — the panel is a flat
  list of `label: value` rows and a reader tells two `pending`s apart by which
  section they are under. The asteroids row was renamed to `despawns`; the
  general problem stands, and the place it bites first is a test that searches
  the draw list by label text (`row_value` in each sample's `app.rs`), which
  silently reads the wrong row rather than failing.

## `apps/hud` milestone 1: what was deliberately left out

`docs/plan/sample/04-hud.md` names milestone 1 as "P4 skeleton: HUD page with
the slice-1 primitives", and that is all that was built. Everything below was in
scope for the sample overall and is not in the tree.

**Waiting on the styling system, not on this sample.** The CSS subset and its
`.css` files, the ≥2 themes and their runtime switcher, the widget gallery page,
the UI inspector, the live-restyle hot-reload showcase, and the per-theme golden
frames in CI. All of them are P10 in the sample's own doc and all of them rest
on a layout/styling engine that does not exist: `crcbl-ui` today has `DrawList`,
`FontAtlas`, `Label`, `Button`, `Style`, `Hud`/`HudPanel`/`Anchor`, `Menu` and
the `debug` panel, and no stylesheet, cascade, selector or box model anywhere.
Building any of it now would be machinery with a single speculative consumer.
The one thing worth recording for whoever starts P10: `page::draw` is one
function that positions everything from named constants at the top of
`apps/hud/src/page.rs`, so the styling work replaces that function's body rather
than restructuring the sample.

**Sample rule 7 is met now** — hud has a `web.rs`, a `cdylib` lib named
`crcbl_hud`, polled `PolledGpu`/`PendingLoop` bring-up, and an entry at every
registration site. It is the smallest wasm artifact of any sample at **2 720 934
bytes**, against breakout 2 947 252, flappy 2 937 308, asteroids 2 970 845 and
horde 3 028 644, which is the measurement this entry said was worth having.

Four things that slice found in the shared web tooling and did not fix, none of
them hud's:

- **Settled: Chrome 151 broke the browser gate, and it was a device mismatch,
  not a readback quirk.** This entry predicted the failure and it arrived
  exactly as described — GitHub's runner moved from Chrome 150.0.7871.128 to
  151.0.7922.108 between `4eb0d65` (Pages green) and `77fa401` (Pages red),
  group A went red for all five demos at once, and the deploy was **skipped**
  rather than failed, which is the shape that hides a broken publish.

  The cause: a WebGPU canvas is handed between two devices — Dawn renders into
  it and Chromium's compositor reads it back for `toDataURL` — and those must be
  the same Vulkan implementation. `--use-webgpu-adapter=swiftshader` moves
  **only Dawn**; the shared-image device stayed on whatever the machine had, and
  on 151 the hand-off fails. The snapshot was therefore **uninitialised memory
  rather than black** — decoding the raw PNG outside the browser gave 2427
  distinct colours, almost all at alpha 0, which is why it decoded as
  `rgb(0,0,0)`. Chromium said so in its own stderr:
  `ReadPixels: Source shared image is not accessible` and
  `CopyTextureForBrowser from [Invalid Texture]`.

  The fix is `--enable-features=Vulkan --use-vulkan=swiftshader` in
  `browserFlags`, pointing the shared-image device at SwiftShader too. Neither
  flag works alone, and `--use-angle=swiftshader` does **not** substitute — it
  is specifically Chromium's shared-image Vulkan device that has to match
  Dawn's. Nothing about how pixels are read changed: `toDataURL` was never the
  problem, and the control and every render check still read through the same
  path.

  **It was never confined to the control.** With group A bypassed, the real
  breakout demo failed identically at its own canvas size with 16 device errors.
  The control was faithfully representing group D, which is its whole purpose.

  **No browser pin.** The gate passes on 151, and the control is what turned a
  silent regression into a loud one — a pin would have hidden this rather than
  fixed it.

- **An unexplained workaround, found and deliberately not used.** Creating a 2D
  canvas in the page _before_ the WebGPU context and reading it back with
  `toDataURL` also makes the SwiftShader readback work, with the old flags.
  Priming it after the fact does not work, the mechanism is unexplained, and it
  would have to be injected into every demo page. Recorded only in case the flag
  fix stops working.

- **Xvfb + `--hardware` reads transparent black on this machine** and loses the
  WebGPU device mid-run, on Chromium 151 with an RX 7900 XTX under RADV. It is
  harmless because `auto` falls through to SwiftShader — which is also what CI
  does, since the runner has no GPU at all — but a developer who passes
  `--hardware` under Xvfb gets a confusing failure. Not investigated.

- **A browser that hands out no adapter is guarded twice, and there is a race
  between the guards.** `WgpuInstance::new_async` probes with wgpu's
  `is_browser_webgpu_supported` before enumerating, and `web/engine/demo.js`
  probes before downloading the wasm. If `requestAdapter()` succeeds for a probe
  and returns null for wgpu's own enumeration a moment later, `new_async` still
  traps — proven reachable with a call-counting double that lets the first _n_
  requests through. Closing it properly needs an upstream fix: `wgpu::Adapter`
  exposes no accessor for its inner `GPUAdapter` (`api/adapter.rs` has only
  `as_custom`), and every other reader — `features()`, `limits()` — is the same
  structural getter with the same uncatchable failure.

- **Worth filing upstream against wgpu**: the vendored
  `Gpu::request_adapter{,_with_options}` bindings type a nullable WebIDL return
  (`Promise<GPUAdapter?>`) as `js_sys::JsOption`, which is undefined-only by
  documented design — `wasm-bindgen`'s own `sys.rs` says "JavaScript `null` is a
  distinct present value". Either the binding needs a null-aware wrapper or
  `future_request_adapter` should test `is_null_or_undefined`.

- **No standing regression guard for the null adapter.** The double that
  reproduces it lives in a scratchpad, not the repo. Making it permanent means a
  group-A sub-check that patches `requestAdapter` and asserts the named message,
  which is a real change to a harness with a documented one-demo-per-run shape —
  left out rather than decided unilaterally.

- **Neither WebGPU-refusal branch in `web/engine/demo.js` calls `settle()`**, so
  the Stop button stays enabled and does nothing. Pre-existing on the sibling
  branch; the new branch matched it rather than fixing half.

- **`web/engine/demo.js` has no way for a demo to say it saves nothing.** On
  `STOPPED` it prints `` `${savedLabel} saved.` `` unconditionally, so hud
  passes `savedLabel: 'Nothing'` and its status bar reads "Nothing saved." —
  true, and a workaround. A falsy branch in `demo.js` is the honest fix.
- **`web/templates/demo-window.html` is one copy for every demo**, so hud's
  canvas is labelled `aria-label="HUD game"` and the page carries a note about
  browsers not starting audio until you interact. hud is neither a game nor
  audible.
- **CI's shellcheck step covers `tools/*.sh` and `crates/*/tests/*.sh`, not
  `web/*.sh`.** Both web scripts touched here were checked by hand and are
  clean, but nothing in CI would have caught it.

**Sample rule 8 (spatial audio through `crcbl-audio`) is not met, and this may
be an honest exemption rather than a gap.** The rule is about _positional game
events_, and hud has none — no world, no listener, no position. UI click sounds
would be an audio system this sample invented for the rule's sake rather than
because anything needs them, and the sample's own scope ("fake data only — no
server simulation beyond a trivial ticker") does not reach for them. Recorded as
a decision to confirm rather than one taken: if the answer is that hud should be
silent, rule 8 should say so the way rule 11 already names hud's exemption.

**No game input, considered and declined.** Binding number keys to fire the
ability slots early was designed and dropped: the ticker already drives every
slot through ready and cooling states, the doc's scope says the page is driven
by "a scripted loop", and a second way to fire an ability would be a second
thing for the determinism script to have to cover. The consequence is that
`HostedGame::key_event` is empty for this sample. If a later milestone wants a
pointer-driven gallery, that is where input arrives.

**The samples' `build.rs` bake half is still copied per sample**, as recorded
above — hud does not add a fourth copy only because it is rule-11 exempt and has
no `build.rs` at all. Nothing changed about that finding.

**Not verified:** the sample has never been run in a real window. Every check
was headless — the null backend, and lavapipe through `--backend vk` — so the
page's colours, spacing and legibility at a real size are unreviewed, and
nothing in CI would catch a page that is correct in the draw list and ugly on
screen. The per-theme golden frames the exit criteria call for are what would
close that, and they are P10.

### `crcbl-assets` after stage 6 task 2

`AssetId`, `AssetHandle`, the `Loading | Ready | Failed` state machine,
`AssetSource` and `DirSource` landed in `crates/crcbl-assets`. What did not, and
what was decided along the way.

**`AssetId` is still hash-of-path, which the plan's own correction says is
wrong.** `AssetId::from_path` derives the id from the canonical key, so renaming
`props/crate.glb` gives it a different id and orphans every reference. The
corrected model in `docs/plan/06-assets-scenes.md` is a sidecar
`crate.glb.meta.ron` carrying a random 128-bit GUID, created on first import.
Nothing can create one here — first import is the importer, which is task 3 — so
the type was made 128 bits wide and given `AssetId::from_bits` so a sidecar GUID
drops in without the type changing. What is missing is the sidecar reader, a
writer that mints a GUID, and a registry path that prefers the sidecar's id over
the path's. Do it in the same slice as the importer, before any content exists
to be renamed.

**No `FetchSource` asset source, deliberately.** Stage 10 owns the browser asset
path. `crates/crcbl-assets/src/source.rs` shows the whole implementation in its
module docs — a newtype over `crcbl_store::web::FetchSource` delegating `read` —
because that type already canonicalises the key, already enqueues on a miss and
already answers `StorageError::Pending`. It is not written because a wrapper
with no consumer is a wrapper nobody has exercised. The claim that it needs no
caller changes is a design argument, **not** something a test proves: no
`AssetSource` other than `DirSource` and the crate's own scripted test source
exists.

**A blanket `impl<S: StorageSource> AssetSource for S` was considered and
declined.** It would have made every storage backend an asset source for free,
and it is recorded here because it will look obvious to the next reader. Two
reasons against: an asset source must not be writable, and coherence — a blanket
impl claims the trait for every present and future `StorageSource`, so
`PackSource` (a baked blob, not directory-shaped storage) could not implement
`AssetSource` on its own terms.

**Asset keys are restricted to `[A-Za-z0-9._-]` and `/`, on native too.**
`DirSource` runs `crcbl_store::web::canonical_key` before touching the
filesystem, so `my asset.png` and `café.png` are refused even though the
filesystem would serve them. Deliberate: those load natively and 404 over HTTP,
and the failure would surface at the point it is hardest to fix. The cost is
that an artist cannot name a file with a space. If that becomes a real
complaint, the fix is percent-encoding in the fetch backend, not a second key
rule here — two rules is how the two backends drift apart.

**No `Unloaded` state and no GPU retire.** The plan lists
`Unloaded → Loading → Ready | Failed`; only the last three are built, because
nothing can produce `Unloaded` — an unrequested asset has no entry and a
released one is removed. It comes back with hot reload (task 5), which turns a
`Ready` entry back into one with no bytes, and with the refcounted release's
other half: the retire calls into the stage 2 deletion queue, which need a
GPU-resident asset to retire and therefore the importer.

**`Ready` and `Failed` are terminal, with no retry.** A failed asset stays
failed until a caller releases and re-requests it. No backoff, no retry budget,
no distinction between a 404 and a transient network error — the last of those
would matter for a browser source and does not exist yet.

**Nothing depends on `crcbl-assets`.** Like `crcbl-scene`, it is a workspace
member every `cargo build --workspace` compiles for nothing until task 3 gives
it a consumer. Same trade-off, same argument as that crate's header.

**Not reviewed or built:** the exit criterion "no synchronous IO anywhere in
engine crates (CI: deny `std::fs` outside `DirSource` + tooling)". There is no
such CI gate, and `crcbl-assets` did not add one — `DirSource` reaches the
filesystem through `crcbl_store::NativeStorage` rather than calling `std::fs`
itself, so a lint written literally against `std::fs` would not name the crate
the criterion is about. Whoever writes that gate has to decide what it actually
forbids.

**Not reviewed:** thread-safety. `AssetSource` requires only `Debug`, where
`crcbl_store::StorageSource` requires `Send`. Nothing loads assets off the frame
thread today and `crcbl-jobs` has not been pointed at this seam, so the bound
was left off rather than guessed at. Adding it later is a breaking change for
any implementor that is not `Send`.

**Not reviewed:** budgets. The registry has no size cap, no eviction and no
limit on how many loads can be outstanding; `poll` walks every `Loading` entry
every call, which is fine for tens and unmeasured for thousands.

### Accepted: CI will not have a real Metal GPU, and that is not a task

Recorded as a decision so it stops reading as work somebody could pick up.

GitHub's hosted macOS images expose an `Apple Paravirtual device`. Real GPU
passthrough is an open feature request on their side with no date, so no amount
of work in this repository changes it. The options that would are a self-hosted
runner or a Mac in somebody's office, and both are a standing cost for a gap
that is narrower than it first looks.

**What the paravirtual device does cover**, and this was itself a correction —
it was long assumed to run no shaders at all, generalised from macos-14, the one
image whose `MTLCreateSystemDefaultDevice()` returns nil. macos-15 and macos-26
run compute dispatches and triangle draws correctly, `macos-latest` resolves to
macos-26, and the Metal suite's device tests pass there. The render e2e draws
every scene on it and matches goldens blessed on lavapipe.

**What it does not cover**, stated so nothing implies otherwise: a discrete or
unvirtualised Apple GPU, and anything a real driver does that a paravirtual one
does not. `crates/crcbl-mtl/tests/run-mtl-e2e.sh`'s header already says this and
should keep saying it. Metal has no software rasteriser, so unlike Vulkan
(lavapipe) and D3D12 (WARP) there is no second implementation to cross-check
against — the cross-backend comparison is the substitute, and it is weaker
because it compares Metal against a _different API_ rather than against a second
Metal.

The mitigation is the one already in place: a person on a real Mac can run
`run-mtl-e2e.sh` unchanged, and that remains the only thing that covers a
non-virtual GPU. Nothing else is owed here.

### The split comparator: what CI has to confirm, and what was declined

The scoring split landed — `Tolerance` carries `gross_channel_delta: 24` and
`max_gross_ratio: 0.001` beside a `max_failing_ratio` relaxed to 0.01, and
`compare` counts each pixel against both thresholds on its one existing visit.
What is left is verification nobody here can run, plus the alternatives that
were tried on paper and rejected, so they are not re-proposed.

**Not verified locally, and CI is the only verdict: the two frames the bound is
sized against.** Neither backend runs on this machine.

- **D3D12 / WARP's sprite scene** — 76 pixels of 49 152 over the delta at up
  to 13. It now clears the drift budget by 6.5× where it used to clear one ratio
  by 3.2×, and its 13 is under `gross_channel_delta`, so it scores nothing at
  all on the gross budget. The exposure is the second of those: if a future
  sprite scene puts WARP's edge disagreements past delta 24 on more than 0.1% of
  the frame, D3D12 goes red where the old ratio passed it. Delta 13 on an edge
  texel is a function of the contrast across that edge, not of driver quality,
  so a higher-contrast sprite could plausibly reach it. Nothing measured has.
- **Metal's cube on a paravirtual device** — 2 pixels at delta 207, 0.0041%.
  This is the one legitimate frame that scores on the gross budget at all, and
  it sits 24× under it. At 97×61, the smallest size the gate runs at, that
  budget is five pixels.

Both are pinned by fixtures in `compare.rs`'s tests
(`warps_sprite_edges_pass_and_are_what_the_ratio_is_sized_against`,
`the_worst_measured_cross_backend_frame_still_passes_with_room_to_spare`) that
reproduce the reported per-pixel numbers, so a future tightening argues with a
test. A fixture is not the frame, though: it reproduces the counts and deltas,
not the pixels.

**The one place this is looser than what it replaced.** A frame with between
0.5% and 1% of its pixels off by 3 to 24 levels was refused by the 0.005 ratio
and passes now. That band is empty in every measurement across vk, wgpu, dx12
and metal, and the alternative is leaving WARP 3.2× from a false alarm on the
backend nobody here can debug. Recorded because it is a real trade, not a free
win: the criterion the split had to meet was more room on **both** sides, and
more room on the legitimate side necessarily means a looser drift budget.

**Declined: a budget on `mean_abs_error`**, which is the shape this entry used
to propose and which the data refutes. P1.3's HDR frame is legitimate at 0.2284
mean abs error — 91% of the frame off by one level, a quantisation boundary the
whole background lands on — and the sprite recolour that must fail is 0.0734.
Any total-error budget loose enough for the first passes the second by a factor
of three, whichever way it is normalised, because total error cannot tell a
level spread over the frame from a patch that is badly wrong. Separation has to
be on **per-pixel magnitude**, which is what a second delta threshold does.
Restricting the sum to pixels already over `max_channel_delta` does work — it
separates WARP from the recolour by 16× — but only 4× on each side, which is
under the bar the split had to clear, and it costs a metric nothing else reads.

**Declined: scoring how _localised_ the differing pixels are.** It is the real
physical difference — WARP's 76 are scattered along quad edges, the recolour's
361 are a 19×19 block — and `differing_bounds` already computes a box. It was
not built because metal's legitimate 2 pixels are adjacent, so density does not
separate that pair; because a real bug need not be contiguous; and because it
needs a second traversal or a running per-region accumulator where the two delta
thresholds cost one comparison inside the existing loop. If the gross budget
ever proves too blunt, this is the next idea, not a new ratio.

**Worth keeping from the entry this replaces:** the original derivation was
built from a table of per-backend figures that did not include D3D12's sprite
scene, because that number had never appeared in a log anybody had read. It was
not wrong about the data it had. That is the ordinary shape of a bound
calibrated on the backends that are easy to measure, and the reason the Metal
and D3D12 jobs upload their diffs.

### Re-affirmed: no Vulkan on macOS, and two facts the original decision lacked

`docs/plan/09-backends-metal-dx12.md`'s 2026-08-05 correction made Apple
platforms Metal-only and cancelled the MoltenVK spike. It was reconsidered on
2026-08-11 and **kept**. The plan doc still carries the reasoning; this records
the reconsideration so the question is not opened a third time, and two things
found while costing it that the original argument did not use.

**`crcbl-vk` cannot enumerate a portability driver at all.** There is no
`VK_KHR_portability_enumeration`, no `ENUMERATE_PORTABILITY_BIT_KHR` on the
instance create flags, and no `VK_KHR_portability_subset` handling anywhere in
`crates/crcbl-vk/src/`. Without those, `vkEnumeratePhysicalDevices` returns zero
devices on macOS whether or not MoltenVK is installed. So "install MoltenVK and
it works" was never true — it is a code change first, and a small one, but it
means no macOS Vulkan support exists to accidentally regress.

**MoltenVK runs on Metal, so a macOS Vulkan CI job adds no GPU coverage.** It
would exercise `crcbl-vk`'s portability against the same paravirtual device
`crcbl-mtl` already uses, not a second driver. That is worth something — it
would have tested the capability model's degradation, since MoltenVK has neither
`DRAW_INDIRECT_COUNT` nor `VK_EXT_mesh_shader` — but it is not the independent
coverage a second backend usually buys, and the original decision's cost (two
GPU paths on the platform with the least CI capacity) stands unchanged against
it.

**If it is ever revisited, the tooling question has a trap in it.** `ash-molten`
statically links MoltenVK and would make a bare `cargo build` sufficient, but it
bypasses the Vulkan loader, and with no loader there are no validation layers —
which `crcbl-vk`'s harness asserts the presence of by design, because a suite
that passes for want of a layer proves nothing. The configuration that keeps
that guarantee is the LunarG SDK, which ships MoltenVK, the loader and the
layers together. Downloading any of it from a `build.rs` was considered and is
the wrong mechanism regardless: it breaks `--offline` and sandboxed builds, runs
in every job including the ones that need nothing, and is invisible to the
`cargo deny` gate this workspace already has.

### glTF import: what the first half left, and what it found upstream

`crcbl_scene::import_gltf` parses; nothing uploads. Written down here because
each item below is a decision or a gap rather than a line of work someone can
pick up from the code alone.

**`gltf` 1.4.1 panics on malformed input, in two places, and both were
reproduced rather than inferred.** `gltf_json::mesh::primitive_validate_hook`
indexes `root.accessors` with the primitive's `POSITION` index directly, after
the derive has already _reported_ that index as out of bounds — so
`gltf::Gltf::from_slice` aborts on a file it was called to reject
(`index out of bounds`). `gltf::binary::Glb::from_slice` computes
`header.length as usize - 12` before checking anything, so a `.glb` declaring a
total length below its own header subtracts with overflow. Both are debug-build
panics and release-build silent wrongness. The consequence is structural:
`crcbl_scene::gltf_check` exists because upstream validation cannot be trusted
to return, and `Gltf::from_slice_without_validation` is what the importer calls.
If either is fixed upstream, the argument in that module's header is what to
re-read before deleting anything — and the checks would still be needed, because
several of them (buffer views inside their buffers, accessor spans inside their
views, indices inside their own vertex array) are things `gltf` never checked at
all. Not reported upstream yet; that is the open action.

**A scaled glTF node produces a non-rigid `GltfInstance::transform`, and
`GpuInstance::transform` requires rigid.** The shader transforms normals with
the matrix's 3×3 part and no inverse-transpose, so a node with non-uniform scale
would light wrongly once uploaded. The importer preserves the scale deliberately
— dropping it here would take the choice away — and the upload step has to pick
one of: bake the scale into the vertices at import (loses instancing of a shared
mesh at different scales), carry a separate normal matrix per instance (a wider
`GpuInstance`), or refuse scaled nodes. Nothing has picked yet, and nothing
renders a glTF, so nothing is wrong today.

**Malformed files are `StorageError::Other(String)`.** That is deliberate reuse
— a second error enum beside `StorageError` would make every caller of the
importer match twice — but it means a caller cannot tell "this file is corrupt"
from "the disk is on fire" except by reading the message. The smallest addition
that would change it is one variant, `StorageError::Malformed { key, reason }`,
in `crcbl-store`; it is not there because no caller branches on the difference.
Revisit when one does (a hot-reload path that wants to keep the last good
version is the likely first).

**`data:` URI buffers and sparse accessors are refused, not implemented.** The
first needs a base64 decoder; the `gltf` crate's is only reachable through its
`import` feature, which is the feature that also does blocking file IO and pulls
in `image`, so enabling it is not an option and the decode would be ours.
Blender exports "glTF Embedded" this way, so a real asset will eventually hit
it. Sparse accessors are refused partly on YAGNI and partly because `gltf`'s
sparse iterator has the same `count - 1` underflow the dense one has, in three
more places.

**Not covered by anything yet:** a real-world glTF. Every fixture is
hand-assembled in `crates/crcbl-scene/src/gltf_fixture.rs` — one triangle, one
material, two nodes — which is what makes the malformed cases readable in a diff
but means no Khronos sample, no exporter output and no large file has ever been
through this code. [12-testing.md](12-testing.md)'s anchor list wants a vendored
Khronos subset at P9; that is where the "does it load Sponza" question gets
answered, and until then "it parses glTF" means "it parses the subset the
fixtures cover".

**`deny.toml`'s `multiple-versions` skip list has three stale entries.**
`cargo deny check` and `cargo deny --all-features check` both warn
`unmatched-skip` for `toml_edit@0.22.27`, `toml_datetime@0.6.11` and
`winnow@0.7.15` — the "toml 0.8 via crcbl-store" stack, which no longer exists
now that the workspace pins `toml = "1.1"`. The gate still passes (the check is
`deny` and the unmatched skips are warnings), so this is tidying, not a failure.
Noticed while adding `gltf`; not fixed because `deny.toml` was outside that
task's paths.

### What the backend validation gates do not cover

`crcbl-dx12` and `crcbl-mtl` now assert a clean validation report at every
device test's teardown, the line `crcbl-vk` has always held. What that does and
does not buy, per backend, because the three are not equivalent and a reader
should not assume they are.

**Metal's is genuinely weaker, and not parity.** Metal has no queryable
validation channel. `MTL_DEBUG_LAYER` is read when the framework loads, before
any of this code runs, so nothing in `crcbl-mtl` can turn it on for itself; an
API misuse is printed to stderr and then handled per
`MTL_DEBUG_LAYER_ERROR_MODE`, with no message list, no callback and no count. So
`assert_clean` there asserts two things only: that Metal interposed the layer on
this device, and that no command buffer it submitted ended in
`MTLCommandBufferStatus::Error`. **An API misuse never reaches the second**, and
the first is read from a private detail — the layer replaces the device object's
class with `MTLDebugDevice`, and `layer_wrapped_device` reads that name. If a
macOS release renames the wrapper the assertion fails naming the class it saw,
which is diagnosable, but it is the one fragile assumption in that crate.

**There is no Metal deliberate-violation gate**, unlike the other two, and that
is a finding rather than an omission: a violation aborts the process, so there
is nothing to assert against. The fault half of the teardown guard is therefore
exercised by nothing today — only a real GPU fault would prove it fires.

**`MTL_SHADER_VALIDATION` is asked for and reported, never asserted.** Whether
it took is not knowable in-process. `MTLShaderValidation::Enabled` exists
per-pipeline-descriptor in `objc2-metal` 0.3.2 and is the programmatic
alternative, but it still cannot be read back.

**Metal's extra validations are off**: `MTL_DEBUG_LAYER_VALIDATE_LOAD_ACTIONS`
and `_STORE_ACTIONS` catch reading an attachment nothing wrote and are the
reasonable next step once the base layer is known green. `_UNRETAINED_RESOURCES`
is irrelevant here — the command buffers are retained.

**D3D12's info queue has a message-count limit (1024 by default) and nothing
clears it any more**, now that `diagnosis` reads rather than drains — which is
what stops a validation error quoted in a `HalError` from consuming the one that
should fail teardown. A healthy run stores zero messages, so this is
theoretical; a device producing more than the limit would start dropping _new_
ones. `attach` could raise it with `SetMessageCountLimit`. Left alone as
premature.

**Never executed anywhere:** whether the 73 D3D12 and 71 Metal device tests are
actually clean under their layers, whether the D3D12 gate's message really names
`CreateCommittedResource`, whether the Metal suite survives `abort` on warnings,
and whether the paravirtual device supports shader validation at all. The layer
itself is confirmed present — a `main` run reports `debug layer=true` on
`windows-latest` — but none of these crates executes on this machine.

### Metal's debug layer is on `nslog`, and `assert` is the follow-up

The first run of the Metal suite under `MTL_DEBUG_LAYER` set both mode variables
to `abort`, and all 71 tests died with

```
Assertion failed: (0), function MTLGetEnvCase, file MTLUtils_Internal.h, line 100.
```

**That was not the layer finding 71 problems.** `abort` is not a value Metal
accepts, and Metal does not ignore a value it does not recognise —
`MTLGetEnvCase` asserts, so every device creation aborted before any test ran.
The accepted set is `ignore`, `assert`, `nslog`.

Both are `nslog` now, which reports each finding to stderr and lets the process
continue. That is what a first run needs — the suite has never executed under
this layer and the job is to read what it says. **It also means an API misuse
does not fail anything today**: it is a line in a log nobody's assertion reads.
`assert` is where this should end up once the log is clean, and moving it is the
follow-up. Until then the enforced half on Metal is what the backend can observe
in-process — that the layer interposed, and that no command buffer ended in
error — which is already recorded as weaker than Vulkan's and D3D12's.

### `a_copy_d3d12_cannot_place_is_refused_by_name` provokes a real layer error

Recorded because it is the one D3D12 test whose validation report is dirty on
purpose, and a future reader will otherwise try to "fix" it.

The refusal it asserts is **D3D12's own**: the seam does not reject a 252-byte
row pitch before the call, so the copy reaches the driver and the debug layer
says so. It calls `defuse()`, exactly as `crcbl-vk`'s gate tests decline to call
`Headless::finish`. The first run with teardown assertions enabled found this
and nothing else across all 75 tests — one deliberate provocation, correctly
flagged.

### Metal GPU validation changes the UI frame, which points at an out-of-bounds read

Found on the first run of `MTL_SHADER_VALIDATION=1` against the render e2e, and
it is the most interesting thing the validation work turned up.

With shader validation on — Metal logs it as `Metal GPU Validation Enabled` —
the `Ui` scene came back with **five distinct colours against a floor of six**,
so something that draws stopped drawing. The floor is not arbitrary: it is the
measured count `run-cross-backend-e2e.sh` records for that scene, and the same
frame passes on vk, wgpu and D3D12.

**Why this reads as a real defect rather than a quirk of the switch.** GPU
validation traps an out-of-bounds shader access instead of letting it return
whatever happened to be in memory. A frame that changes when the trap is turned
on is a frame that was relying on the untrapped read. The obvious shapes are an
index past a bound array or a sampler reading outside its texture in the UI
path.

**What narrows it usefully:** the Metal HAL suite runs _with_ shader validation
and is clean at 71 tests and zero failed submissions. So whatever this is lives
in the renderer's path — `ui_pass`, `ui.slang`, or what the pass binds — and not
in `crcbl-mtl` itself. The cube and sprite scenes fail the same step, but their
failure may be a consequence of the UI test's rather than independent; nobody
has separated them.

**Not diagnosed here**, because it needs a Mac: nothing in this repository can
run Metal, and the CI job's output is a colour count rather than a message —
Metal names no offending access in `nslog` mode for this. The step therefore
runs with API validation and **without** shader validation, so it keeps gating
the picture; turning `MTL_SHADER_VALIDATION` back on in
`.github/workflows/ci.yml` is how the investigation starts, and the first thing
worth trying is `MTL_DEBUG_LAYER_ERROR_MODE=assert` to see whether Metal will
name it.

## The material lookup moved to the fragment stage, and what that probe learned

`mesh.slang` reads binding 6 in `fragmentMain` now. `VertexOutput` carries
`nointerpolation uint material : TEXCOORD0` and the vertex stage writes
`vertex.color` untinted. The move was made on its own, with no texture beside
it, because a flat integer varying is the third integer this file hands across
the stage boundary and the other two — `SV_InstanceID`, `SV_VertexID` — both
lowered differently per target and were both caught by rendering rather than by
reading the emitted code.

**One edit this needs is outside the slice's paths and was not made.** The
material table's `BindGroupLayoutEntry` is `ShaderStages::VERTEX`, which is now
the one stage that does not read it:

- `ForwardRenderer::mesh_layout`'s binding 6 in
  `crates/crcbl-render/src/forward.rs`.
- The same binding in the layout `crcbl-vk`'s `vk_e2e/depth_probe.rs` builds for
  itself.

Both must become `ShaderStages::VERTEX.union(ShaderStages::FRAGMENT)`. **The
union, not `FRAGMENT` alone**, and that is a Metal constraint rather than
symmetry: Slang's Metal backend materialises every global in every entry point
(see "Slang's Metal backend materialises every global shader parameter…"), so
`vertexMain` in `msl/mesh.metal` still takes `materials [[buffer(6)]]` whether
it reads it or not. Verified with the change applied in a scratch worktree —
`vk` and `wgpu` are green and bit-identical with the union.

**Until it lands, `wgpu` cannot draw the cube at all.** Not a validation
warning: `Device::create_render_pipeline, label = 'forward mesh'` fails with
"Shader global ResourceBinding { group: 0, binding: 6 } is not available in the
pipeline layout / Visibility flags don't include the shader stage", and
`crates/crcbl/tests/run-render-e2e.sh` on `CRCBL_GPU=wgpu` reports
`3 tests run: 2 passed, 1 failed`.

**Vulkan is looser, and where it is loose depends on who is listening.** The
pipeline is created and draws the correct frame either way, but the layer emits
`VUID-VkGraphicsPipelineCreateInfo-layout-07988` — which `run-render-e2e.sh`
only logs, and which `crcbl_vk::debug` escalates to a panic. So
`crates/crcbl-vk/tests/run-vk-e2e.sh` reports
`12/74 tests run: 11 passed, 1 failed`, failing
`depth_probe::reversed_z_puts_the_nearer_surface_in_front_and_standard_z_would_not`
and cancelling the rest — the depth probe's layout, not the renderer's.

**A workspace `cargo nextest run` catches neither** — it was
`2980 tests run: 2980 passed, 168 skipped` with the shader changed and the
layouts not. Nothing below the GPU seam checks a bind-group layout's visibility
against the module bound to it.

### What the probe found, which is not what it was pointed at

**Every one of the four targets emits the flat qualifier**, read out of this
crate's own regenerated artifacts with slangc 2026.14: SPIR-V decorates both
sides `Flat`, WGSL writes `@interpolate(flat) @location(3)`, MSL puts `[[flat]]`
on the fragment's `[[stage_in]]` struct — which is where Metal reads it, not the
vertex output struct — and DXIL's input signature lists `TEXCOORD 0` as
`nointerpolation`. No divergence to report.

**Dropping `nointerpolation` does not make a golden go red, and cannot.** Tried
it, on both backends that run here:

- **SPIR-V repairs it.** Slang drops `Flat` from the vertex _output_ but keeps
  it on the fragment _input_, which is the decoration that decides
  interpolation, so `vk` draws a bit-identical frame:
  `golden cube on vulkan — 256x192: 0 pixel(s) differ at all (0.0000%)`.
- **WGSL refuses it**, and does so before any frame — naga rejects the module
  with "`@interpolate(flat)` must be explicitly specified for integer I/O". That
  is caught by `crcbl-shaders`' own
  `wgsl_validation::every_committed_wgsl_artifact_validates` on a machine with
  no GPU, which is a better gate than a golden anyway.

**And the cube scene could not detect a wrong interpolation _mode_ even if one
existed**, which is worth knowing before trusting it for the next varying. The
material id is constant across every primitive — all three vertices of a
triangle belong to one instance — so flat and linear interpolation of it agree
by construction, and there is no "fragment between two vertices" that could
resolve a third row. What `nointerpolation` actually buys here is what
`sprite.slang`'s `sheet.z` note says it buys: an exact integer instead of one
that arrived through a float unit and truncates a row early.

**What the golden does detect is a fragment resolving the wrong row**, which is
the failure a texture fetch would produce and the reason the scene's two
pyramids are in unlike colour families. Pinned by making the fragment stage read
a fixed `materials[0]` and rendering:
`256x192: 4105 pixel(s) differ at all (8.3516%), max channel delta 105, 4105 over tolerance (8.3516%), mean abs error 2.0736, rmse 11.1112, ssim 0.991305 — failed: TooManyDifferingPixels`,
the same line on `vk` and on `wgpu`.

**`msl` and `dxil` were not rendered.** Nothing here runs Metal or D3D12, and
they are the two whose lowering this probe least exercises. Their artifacts were
read and carry the right qualifier; CI is the only thing that can say the frame
does too.

### Two older entries this closes

- **"`mesh.slang`'s seventh binding has two callers outside `crcbl-render`" is
  spent and should be deleted.** Both bullets shipped: `crcbl-dx12`'s
  `dxil::tests::registers_are_assigned_per_class_in_declaration_order` reads
  `&[Cbv, Srv, Srv, Cbv, Srv, Srv, Srv]` and is green in a workspace run, and
  `vk_e2e/depth_probe.rs` has its seventh entry and is green in
  `run-vk-e2e.sh`'s `74 tests run: 74 passed`. Left in place only because this
  slice was told to append to this file rather than restructure it.
- **"Specular stayed a constant" is now a smaller question than it was.** It
  says moving `SPECULAR_POWER` and `SPECULAR_STRENGTH` into a material "needs
  the fragment stage to reach the table, which means either a `nointerpolation`
  integer varying … or two more floats through `VertexOutput`". The varying
  exists and the fragment stage reaches the table, so what is left is two `f32`
  columns in `GpuMaterial` and the shading-model decision
  `docs/plan/37-materials.md` owns — which is the part that was never
  mechanical.

### D3D12 allow-list: two entries, and what retires each

`crcbl_dx12::debug::ALLOWED` is a table of message ids the validation gate
passes over, each with the argument for it, consulted only for
`Severity::Warning` — the same id arriving as an error or corruption fails as
before, and allowed messages are counted and named so "silent" and "answered
for" stay distinct.

**Id 820, `CLEARRENDERTARGETVIEW_MISMATCHINGCLEARVALUE`.** The layer files two
different things under this number: "you passed no optimized clear value"
(advisory) and "the value you promised is not the one you cleared to" (a real
defect). This backend passes `None` for `pOptimizedClearValue`, so today only
the first can occur. **The entry is safe only while that stays true** — the
moment `Device::create_image` passes a value, this allowance hides the defect.
Removing it needs a clear-value field on `crcbl_hal::ImageDesc` and a decision
about what a pass that clears to a different colour should do, since the promise
is per-resource and the colour arrives per-pass at `begin_render_pass`.

**Id 1361, `CREATE_SAMPLER_COMPARISON_FUNC_IGNORED`.** `create_sampler` writes
`D3D12_COMPARISON_FUNC_ALWAYS` as filler when `SamplerDesc::compare` is `None`.
The right value is `D3D12_COMPARISON_FUNC_NONE` — zero, the enumerant that says
exactly what the seam means, and the old comment claiming zero is "a sampler
feedback value" was wrong and is corrected. **Not switched**, because this
backend asks for `D3D_FEATURE_LEVEL_11_0` and it is not established which
runtimes accept a zero in a classic `D3D12_SAMPLER_DESC` rather than filing
`CREATE_SAMPLER_INVALID`; that would trade an advisory for an error on machines
nothing here can test, and the CI runner being Windows Server 2025 would prove
nothing about older ones. Settle that and this entry goes.

### Verified, not a problem: the D3D12 info queue does not leak across tests

Recorded so it is not re-investigated. `debug::read_queue` reads from index 0
and never clears, which looks like it would let one test's messages fail
another's teardown. It does not: every device test opens its own `ID3D12Device`
through `device::tests::open_device`, and `debug::attach` clears _that device's_
queue at creation, so a report means "since this device was created" by
construction. Evidence: in run 31454155654, message 597 — the gate's own
deliberate violation — appears exactly once, inside the expected panic of the
test that raises it, and that test passed.

## The base-colour page is still `ArrayPages`; the wgpu blocker under it is gone

`docs/plan/03-gpu-driven-rendering.md` §3.2's texture half is implemented as one
`Texture2DArray` page — `crcbl_render::forward`'s `base_color_page`, bound at
`mesh.slang` binding 7, with `GpuMaterial::base_color_texture` selecting a
layer. `BindingModel::Bindless` — one runtime-sized array _of descriptors_,
indexed per fragment — is still not implemented, but the reason has changed and
the old reason is worth not re-deriving.

**What was blocking it is fixed.** `crcbl-wgpu` could not fill an array binding
at all: `create_bind_group` keyed every entry on `binding` alone and
`BindGroupEntry::array_index` appeared nowhere in the crate, so a bindless slice
would have selected the bindless path on wgpu (it reports `DESCRIPTOR_INDEXING`)
and then failed to build the group. `crates/crcbl-wgpu/src/binding.rs` now does
the bucketing, and
`a_wgpu_shader_reads_the_array_element_the_bind_group_put_in_each_slot` reads
both elements out of a two-texture array on lavapipe. All four backends honour
`array_index`.

**What is left is above the seam, and it is a real slice.** Nothing selects
`BindingModel::Bindless` — `crcbl_render::forward` builds one page
unconditionally. Going bindless means a descriptor array whose length is a
runtime bound, a per-material index into it that is a descriptor slot rather
than a layer, `BindingFlags::VARIABLE_COUNT` and `BindGroupDesc::variable_count`
actually being used (see the wgpu entry below — that backend ignores the second
one), and a `mesh.slang` that declares the array. The two paths then have to
render the same frame, which is the observable.

**What bindless buys**, so the case stays on the record: a page is one image, so
its layers share an extent, a format and a mip count. A descriptor array lifts
all three, which is what real imported content needs. Until then the engine has
one page of one size, which is enough for the observable and not enough for a
game.

## What `crcbl-wgpu`'s binding work still leaves, after the refusals landed

The three silent drops found while writing `crates/crcbl-wgpu/src/binding.rs`
are fixed — `BindingFlags` and the `VARIABLE_COUNT` ordering rule are checked at
layout creation, `variable_count` is checked against the layout's variable
binding and the entries supplied, `count: 0` is refused, and
`create_bind_group_layout` is error-scoped. What is left:

- **`update_bind_group` is still `Unsupported` on wgpu** — WebGPU bind groups
  are immutable and there is no update-after-bind path, so the seam's streaming
  bindless write is create-only here. It is the other half of what `array_index`
  exists for: a page of descriptors that grows as content loads has to be
  rebuilt rather than written into.

**Not verified: the browser.** Binding arrays are a native-only wgpu feature and
`DESCRIPTOR_INDEXING` will be absent under WebGPU, so the array-shaped tests
take their skip branch there. No browser run was made, and the skip branch means
a wasm regression in this code would not be observed by anything. The refusals
that do not need an array layout — the flags gate, `count: 0`, the in-band
layout error — run on every adapter, so that half is not skip-shaped.

## What `crcbl_scene::simplify` owes, and one workspace-wide trap it found

Topic 25's QEM simplifier exists host-side with no consumer. What is left:

- **`glam`'s `DMat3: Default` is the identity, not zero**, so a derived
  `Default` on a quadric seeds every vertex with the three coordinate planes. It
  was caught only by the hand-derived quadric tests — the structural ones
  (closed mesh, border kept, deterministic) all passed with the bug in place.
  `Quadric::ZERO` is spelled out with that reason. **This applies anywhere in
  the workspace that derives `Default` through a glam matrix**, which is why it
  is here rather than only in that file.
- **Flip rejection is per-collapse, not a global invariant.** A face can rotate
  a little under each of several individually-accepted collapses until it has
  come all the way round; demonstrated by popping in descending cost order,
  where a height field ends up with a face pointing at `-Z` and every single
  collapse having passed the check. The cheapest-first order is what keeps the
  local test a workable stand-in. A global orientation check is the fix.
- **A rejected candidate is dropped, not deferred** — an edge refused now is
  only reconsidered if an endpoint is later merged into. Cheap, terminates,
  leaves some collapses unmade.
- **`max_error` is not a certified Hausdorff bound**, so the plan's "reported
  error ≥ sampled Hausdorff" property test does not exist. Runtime selection
  will lean on this number, so that test is owed before it does.
- **Never measured on a real asset.** Every fixture is synthetic — torus, height
  field, tetrahedron — there is no glTF corpus case and no benchmark, and the
  cost is O(E) in candidates with re-pushes per collapse.
- **Position welding is absent**: vertices compare by index, so duplicated
  coincident vertices read as disjoint surfaces whose every shared edge is a
  border and therefore locked.
- **One mutation is provably unobservable rather than untested.** Making
  `max_error` record the last collapse instead of the largest stays green,
  because the heap pops cheapest-first and a collapse only ever adds a positive
  semi-definite quadric, so popped costs are non-decreasing and max equals last.
  That ordering is asserted directly by
  `the_costs_of_the_collapses_performed_never_decrease` rather than left as
  prose, and `.max()` is kept because it is what the metric is defined as.

## The shared layout validator: two Metal decisions, and what only CI can prove

`BindGroupLayoutDesc::check_entries` and `BindGroupLayoutEntry::resolved_count`
replaced four drifted copies. Decisions taken while doing it, so they are not
re-argued:

- **`crcbl-mtl` refuses the `u32::MAX` count sentinel where the other four clamp
  it.** Metal reports `max_bindless_descriptors: 0` — flat argument tables have
  no runtime-sized array — so clamping would hand back a **one**-element array
  on a backend that cannot do bindless at all, which is exactly the quiet
  downgrade the seam exists to forbid. `plan_set`'s table-capacity `checked_add`
  refuses it by name instead. Reversible in about a line plus a `limits`
  parameter on `plan_set` if this ever looks wrong.
- **`crcbl-mtl`'s own flags refusal is now unreachable through
  `create_bind_group_layout`.** The seam's check fires first, so a caller asking
  for bindless on Metal gets the generic "descriptor-indexing flags on a device
  without DESCRIPTOR_INDEXING" rather than `plan_set`'s Metal-specific "flat
  argument tables have no runtime-sized array". Kept that way because one
  message per mistake across all backends is the point of the extraction, and
  the generic one names the actionable fact. `plan_set` keeps the refusal and
  its own tests exercise it directly; flipping the order is the fix if the
  specific wording turns out to matter more.
- **`crcbl-vk`'s
  `a_bindless_capable_layout_is_accepted_or_refused_according_to_the_tier` only
  `eprintln!`s the misplaced-`VARIABLE_COUNT` error** rather than asserting on
  it. It does fail if the refusal stops happening — that is how the vk call site
  was proven — but it would not notice the message becoming useless.

**Coverage, stated as a gap rather than implied.** The validator is proven
_called_ on three backends by a real run: the seam and null backend on the host,
`crcbl-vk` on an RX 7900 XTX, `crcbl-wgpu` on lavapipe — neutering
`check_entries` to return `Ok(())` reddens ten seam tests plus one test in each
of those two device suites. **`crcbl-dx12` and `crcbl-mtl` are type-checked only
here.** `crcbl-dx12` is entirely `#[cfg(target_os = "windows")]`, so a Linux
`cargo test -p crcbl-dx12` never compiles `binding::tests` at all, and
`--target x86_64-pc-windows-msvc` has no linker on this box; the `--target`
clippy runs are a type-check and nothing more. Those two backends' new tests
first _execute_ on CI's Windows and macOS runners, which is where their evidence
comes from.

## `BindingKind::StorageImage` still cannot name its format or its dimension

`crcbl_wgpu::conv::map_binding_kind` returns `HalError::Unsupported` for every
storage-image binding, because `wgpu::BindingType::StorageTexture` needs the
texel format _and_ the view dimension at bind-group-layout creation and
`BindingKind::StorageImage { read_only }` carries neither.

The sampled half of that hole was closed in 2026-08 — `SampledImage` now carries
a `view_type` — and the storage half was **considered and deliberately left
open**: nothing in the engine declares a storage-image binding, so a `format`
and a `view_type` field there would be two fields nothing reads and 0 call sites
to prove them against. `SampledImage`'s field earned itself because
`crcbl_render::forward` binds a `D2Array` through it today.

Close it the same way when a compute pass first wants one — a mip-generation
pass is the likely first, `docs/plan/03-gpu-driven-rendering.md` §3.2 — and note
that the format half has no `ImageViewType`-shaped answer already sitting in the
seam: it needs `Format`, and the arm must reject a `Format` wgpu cannot express
as a storage format rather than substituting one.

## What a sampled binding still cannot say

`map_binding_kind` assumes every sampled image is float-filterable and
single-sampled: `wgpu::TextureSampleType::Float { filterable: true }` and
`multisampled: false`, both constants. That is what every sampled binding in the
engine is. A shadow-comparison sampler (`TextureSampleType::Depth`), an integer
texture (`Uint`/`Sint`) or an MSAA source would each need another field on
`BindingKind::SampledImage`, and each would fail on wgpu the way the array did —
at pipeline creation, loudly. The other three backends would not notice, so
**the wgpu suite is the only local gate on it**:
`CRCBL_GPU=wgpu crates/crcbl/tests/run-render-e2e.sh`.

## Settled: the `D2Array` page samples on Metal and D3D12

Was an open coverage gap — `SampledImage { view_type }` is dropped by
`crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` (each takes the dimension off the bound
view, and each says so at the arm that drops it), and neither Metal nor D3D12
runs a draw on this machine, so both were type-checked only.

CI confirmed it on `7c4042b`: `golden cube on metal` and `golden cube on dx12`
each came back **max channel delta 1, 0 over tolerance, 0 grossly wrong**
against the lavapipe-blessed golden. Metal is the one that mattered — it is the
only `ArrayPages` device, because it withdraws `Features::DESCRIPTOR_INDEXING` —
and its cube previously carried `max channel delta 207` with 2 pixels grossly
wrong, so agreement went strictly up. Kept only as the record that this was
checked and how; there is nothing owed.

## `crcbl_scene::meshlet`: decisions taken, and what it does not do

The §3.5 bake step exists as `build_meshlets` and has no producer and no
consumer. Decisions, so they are not re-argued:

- **It lives in `crcbl-scene`, not a crate of its own.** That crate's `lib.rs`
  already says its job ends at host memory — vertex arrays, index arrays — and a
  cluster builder is host-side geometry over exactly those. `GltfPrimitive` is
  its first producer. A crate would have been a fourth name for one
  responsibility.
- **It takes `&[[f32; 3]]` and `&[u32]`, not a `GltfPrimitive`.** Keeps it
  testable from literals and keeps the importer's private struct out of it.
  Deliberately **no** `GltfPrimitive::meshlets()` — that is a second caller that
  does not exist yet.
- **A dedicated `MeshletError`, against the crate's stated
  `StorageError`-for-everything convention** (argued in
  `crates/crcbl-scene/Cargo.toml`). The convention is about the IO seam; the
  builder reads no bytes, so every `StorageError` variant but `Other(String)` is
  unreachable and `Other` would erase which of the two caller bugs was hit.
  Reason is recorded in the manifest beside the dependency. Revisit only if a
  third error enum shows up in this crate.
- **Greedy sequential clustering**, no dependency. `meshoptimizer` would be a
  new dependency and that is the user's call; the simple form is deterministic
  by construction, which is what §3.5 actually requires.
- **Offsets are `usize`.** Narrowing them for the GPU is the later slice's call,
  and `u32` here would have needed a third error variant for overflow.

What it does not do, in the order it would be wanted:

- **No spatial pre-sort.** The walk follows index-buffer order and nothing else,
  so an incoherent mesh gets loose spheres and wide cones that cull almost
  nothing. This is the single biggest quality gap and it is a pass ahead of
  `build_meshlets`, not a change to it.
- **No per-cluster padding of `MeshletBuild::triangles`.**
  `MAX_CLUSTER_TRIANGLES` keeps a _full_ cluster's corner run a whole number of
  four-byte words, but a cluster the vertex bound closed early ends anywhere, so
  the next run is not aligned. A GPU slice that wants to read corners as `u32`
  has to add the padding; it was left out as machinery for a consumer that does
  not exist.
- **Determinism is verified same-process only.**
  `the_same_mesh_built_twice_gives_identical_clusters` cannot catch
  cross-machine float drift. A golden artifact is what would, and that belongs
  to the bake-cache slice.
- **The bounds are compile-time constants with no runtime configuration.** A
  device reporting a lower `maxMeshOutputPrimitives` than the ecosystem figure
  cannot be honoured; that is the capability slice's decision to make.
- **The bounding sphere is the AABB midpoint and the furthest vertex**, which is
  valid and not minimal. Ritter's or Welzl's would be tighter and neither was
  worth transcribing for a first cut.

### What the mesh-shader path owes, now that it draws

The layering question is settled the way it was recorded: the `Meshlet` record
lives in `crcbl_shaders::meshlet`, the builder stayed in `crcbl-scene` and
re-exports it, and `crcbl-render` gained no dependency on `crcbl-scene`. What is
left:

- **Only `apps/sandbox` constructs a `ForwardRenderer`, so only sandbox has a
  mesh to draw.** Worth knowing before reading too much into which samples
  select which `GeometryPath`: `EmitTail::from_caps` is the sole reader of
  `geometry_path()` in `crcbl-render`, the sprite pass records a plain
  `encoder.draw` and the UI pass a plain `encoder.draw_indexed`, and neither
  branches on a selector. The other samples ask for `MESH_SHADER` to satisfy
  sample rule 12 and to make the downgrade line name it — not for speed.
  Measured on horde at 10 000 instances: no difference, with the between-arm gap
  smaller than the within-arm spread. §3.5's exit criterion is about meshlets
  and cluster LOD, which none of these samples have.
- **`apps/hud` reports `IndirectPerBatch` / `ArrayPages` on an RX 7900 XTX**,
  which sample rule 12 arguably forbids. Its `desc()` omits `GPU_DRIVEN`
  deliberately — nothing in it issues an indirect draw, and it builds neither
  renderer — so the flag would have no consumer. Whether rule 12 outranks that
  reasoning is a decision, not an oversight.
- **`PRESENT_TIMING` is granted and still reports nothing.** radv grants both
  present flags — there is no downgrade line for them on vk at all — and every
  run still logs `hal: display timing Unknown; asked for Auto, pacing Vsync`. So
  the timing half of present support is negotiated and inert on this machine:
  the extension is there and the query answers `Unknown`. The feedback half is
  live and proven (`vkWaitForPresentKHR on present 1; the loop is closed`). Not
  investigated; it is `crcbl-vk`/engine territory.
- **Nothing automated asserts that a _game_ closes the present loop.**
  `crates/crcbl-shell/tests/run-wayland-e2e.sh` makes that assertion for
  `apps/sandbox` only. The four samples' new tests are **drift guards** — they
  assert `optional_features` equals the engine's — which is not the same as
  asserting pacing happened; that was verified by hand against a private
  headless sway session. Extending the wayland harness to cover a game is the
  fix.
- **`--headless --hardware` is the browser-gate flag pair that works here, and
  it is the silent-pass pair on a machine without a GPU.**
  `web/run-browser-e2e.sh`'s own header argues for Xvfb over `--headless` for
  exactly that reason: headless plus SwiftShader returns transparent black from
  the canvas readback rather than failing. It is safe on this box because there
  is a real GPU (`"hardware" adapter — amd rdna-3`), and it is being used
  because Chromium 151 broke the Xvfb path. Worth knowing before that flag pair
  is copied anywhere it would run without a GPU.

- **Settled: `Features::GPU_DRIVEN`'s doc was wrong, not its callers.** The doc
  said "never as a requirement" while nine call sites across five files pass it
  as `required_features` — `crcbl-render/src/{ui_pass,sprite_pass,texture}.rs`,
  `tests/graph_compile.rs` and `tests/ui_pass_stream.rs`. Every one is test code
  opening `NullInstance::gpu_driven()`, a preset that holds the bundle by
  construction, so there is no hardware to refuse and the requirement is a
  precondition assert — one with teeth, shown by degrading a preset and watching
  ten tests fail on `UnsupportedFeatures`. It is also load-bearing: the null
  backend grants `adapter.features ∩ (required ∪ optional)`, so naming a subset
  would change the device's selected path and quietly retarget those tests. The
  rule the doc defends binds a caller that must run on whatever device it finds,
  and no shipping caller violates it; the doc now says that, and the callers are
  unchanged.
- **`crates/crcbl-vk/tests/run-cross-backend-e2e.sh` does not echo its ICD pin**
  the way `run-render-e2e.sh` does, so which adapter it used is not observable
  from its output. It passed 6/6, but with `CRCBL_VK_ICD` set it still drew vk
  on the discrete card here.
- **Lavapipe reports `VK_EXT_mesh_shader`** (Mesa 23.2 and later), so CI's Linux
  and Windows vk jobs take the mesh path too — verified locally with
  `VK_DRIVER_FILES=…/lvp_icd.json vulkaninfo` and by a full local run on
  lavapipe at zero differing pixels. wgpu, WARP and Metal do not: each reports
  no `MESH_SHADER`, so those jobs keep drawing through an indirect tail and are
  the coverage that the fallback still works.
- **Settled: the cone cull rule needed a radius term, and it now has one.** The
  documented form was the point-sized one — it treats every triangle as sharing
  the centre's view direction, so a cluster with a real radius close to the
  camera could hold a front-facing triangle and be rejected anyway. The
  conservative form adds `+ radius`, and the derivation on
  `ClusterBounds::cone_cutoff` now carries it. Measured over 400 000 random
  samples: the corrected form dropped a front-facing cluster **0** times, the
  old one **11 225**. The `cone_cutoff > 0` guard is **not** subsumed by it —
  `sqrt(1 - cutoff²)` is even in `cutoff` and cannot tell a narrow cone from one
  wider than a hemisphere.

  **On shipped geometry the term only ever adds slack**, which is why it needs a
  constructed test rather than a picture: no mesh in the engine has a
  `cone_cutoff` strictly between 0 and 1 — a flat face gets `1.0`, a closed
  shape gets `OMNIDIRECTIONAL_CUTOFF` — and at `cutoff == 1` the two forms
  differ only where the camera has crossed the face's plane, i.e. for a
  genuinely back-facing cluster.
  `a_cluster_the_point_form_would_reject_survives_the_conservative_one` is the
  host-side case that actually pins the correction.

- **`CRCBL_BLESS` is suite-wide and there is no way to scope it to one golden.**
  Setting it re-blesses every golden the run reaches, so it cannot be used to
  regenerate a single image — and a suite-wide bless run also fails fast on the
  first test that objects, which is what stopped one from doing damage here. The
  safe way to regenerate one golden is to delete that file and run **only** its
  test (`run-vk-e2e.sh -E 'test(name)'`), because a missing reference is created
  by `Golden::check` and reported as `Blessed { created: true }`, which the
  harness turns into a failure saying the run proved nothing. Worth knowing
  before someone reaches for `CRCBL_BLESS=1` to fix one image.
- **The open box's golden is blessed on lavapipe**, like every other vk golden,
  so CI compares it at zero differing pixels and a local radv run drifts instead
  — 94.55 % of pixels differ at `max channel delta 1`, `0 over tolerance`. That
  is the split comparator working as designed, but it means this golden sits on
  the drift budget rather than near it: if `Tolerance::RASTERISER`'s
  `max_channel_delta` is ever tightened to 1, this is the first golden that
  fails, and it will fail only on the discrete card.
- **`group_is_live` in `mesh_cluster.slang` can no longer return 0** for any
  group the driver launches, now that the extents are the culled count. It is
  kept as a range check on buffer-sourced input, and its docs say so rather than
  claiming to be the filter. Removing it would also drop binding 12
  (`draw_args`) from that shader, renumber 13 and 14, shrink the forward pass's
  bind-group layout, and let the mesh path stop declaring `read_buffer(args_id)`
  — a binding renumber under Metal's declaration-order rule, on a path only
  Vulkan runs. Deliberately out of scope.
- **`crates/crcbl-vk/tests/vk_e2e/draw_gen.rs` poisons three of the clearing
  pass's buffers with a sentinel, and `mesh_args` is not among them.** The
  extent test's second lap covers the zeroing indirectly — an accumulating
  extent would double — but the sentinel path does not reach it.
- **`mesh_args` is imported with `final_state: IndirectArgument` on every
  geometry path**, so the two indirect tails pay one end-of-frame transition for
  a buffer they never read. That matches what `counts` already does on the mesh
  path; not changed.
- **Frustum rejection of these clusters is inherently marginal**, and the counts
  should be read with that in mind. A flat 1x1 face's AABB-midpoint bounding
  sphere has radius 0.707 — comparable to the whole mesh — so cluster spheres
  always straddle a side plane, and the _instance_ AABB cull is tighter than the
  cluster sphere cull. The only decisive rejections available are below or
  behind a camera inside the box, which is what the third test camera uses
  (margins of roughly 0.12–0.24 world units, about five orders of magnitude
  above f32 noise). A tighter bounding sphere — Ritter's or Welzl's, already
  flagged as not done on `ClusterBounds::center` — is what would change this.
- **The cluster count is per-frame-total, not per-bucket.** A per-bucket
  breakdown needs a wider stats buffer; the tests work around it by measuring
  each camera twice, with the open box in the scene and out of it, and
  attributing the difference.
- **`cargo clippy --all-targets` does not compile `crcbl-vk`'s e2e target** — it
  needs `--features vk-e2e`, so the bare command `CLAUDE.md` documents will not
  see a borrow error in `crates/crcbl-vk/tests/vk_e2e/**`. One got through both
  documented clippy gates in this session and surfaced only inside
  `run-vk-e2e.sh`. `cargo clippy --all-targets --all-features` **does** cover
  it, since `vk-e2e` is a real feature in that crate's manifest; the habit worth
  keeping is the `--all-features` form.
- **The cluster buffers are `HostUpload`, written once at build.** Device-local
  storage is what a bake cache that streams clusters would want.
- **No bake cache, no input hashing, no cluster LOD/QEM.**
- **Still undecided, and inherited from the builder:** whether meshlets are a
  bake artifact the renderer receives prebuilt (what §3.5 describes, and what
  keeps the dependency direction clean) or something built at `MeshPool::upload`
  time. Today neither — `crcbl-render`'s clusters come from constants in
  `crcbl_shaders::meshlet` (`cube_clusters`, `pyramid_clusters`), pinned against
  the real builder by `crcbl-scene`'s
  `the_hardcoded_meshes_cluster_the_way_the_shaders_crate_says`. That pinning is
  what stops the two drifting; it is not a substitute for deciding.
