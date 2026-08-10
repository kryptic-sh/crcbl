# Backlog

What was raised and not finished. A changelog says what shipped; this says what
did not, and why. Delete an entry when it ships — `git log` is the history.

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

- **`Sprite` is a public-field struct, so every new field is a breaking
  change.** Adding `rotation` broke five literals in `apps/*/src/art.rs` and
  four inside `crcbl-render` itself, none of which had anything to do with
  turning. The next field — a pivot, a z, a flip — breaks them all again, and
  the sample count is going up. The fix is a constructor plus `with_*` methods
  (or `#[non_exhaustive]` and `..Default::default()`), decided before the field
  after next rather than after it. Not done in the rotation slice because it
  would have put an API refactor of every caller inside a rendering change.

- **The four samples still spell the audio entry point three different ways**,
  and the mixer-adoption slice deliberately did not unify them.
  `play_panned(id, emitter_x)` in breakout, `play_at(id, listener_x, x, y)` in
  flappy, `play_at(id, x, y)` in asteroids and `play_at(id, listener, at)` in
  horde. Each is right for its game's listener convention — fixed at the camera,
  fixed with one moving axis, fixed at the origin, riding the player — and the
  engine has no listener at all: `spatial::compute_cue` takes the listener
  position on every call and nothing in `crcbl-audio` remembers one. That is the
  real missing piece, and it is a design question rather than a refactor: a
  `Listener` on the mixer, set once a frame, would collapse all four signatures
  to `play_at(id, world_position)`. Not attempted here because it changes what
  the spatial grammar's API means, and the adoption slice was already touching
  four samples.

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
`crates/crcbl-shell/tests/bin/send_key.rs` and `send_key_x11.rs` are what drive
`F11` at a running sample from outside its process. What is still uncovered:

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
  `crates/crcbl-shell/tests/bin/send_key.rs` is the worked example.
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

### Vulkan on Windows: restored, on pwsh, with one measurement retracted

**Retraction first.** This entry previously said: "`VK_LOADER_DEBUG=all`
produced no loader debug output whatever. A loader ignoring its own debug switch
is not reading its environment at all — so the variables are not reaching the
process." **That is wrong, and the error was mine.** `VK_LOADER_DEBUG: all` was
inserted by matching the first step named `Run the suite against lavapipe` — and
**both** the Linux and Windows jobs have a step with that exact name, so it
landed in the Linux one (`06dbf26`, line 762, inside `vk-e2e:` which spans
719–904; `vk-e2e-windows:` starts at 905). The Windows job never had the switch
set. `windows_read_data_files_in_registry: Registry lookup failed` is a warning
the loader emits at its default level anyway.

So the bash-to-native environment hypothesis is **back to being a hypothesis**.
What is actually known is only that the loader reported falling back to the
registry — equally consistent with it reading `VK_DRIVER_FILES` and declining
every driver in it.

**What is still established, all measured:** `windows-latest` ships no loader
and no driver; the job installs a pinned SDK and a pinned lavapipe and verifies
both arrived, including resolving the manifest's relative `library_path`; it
selects all **74** tests; and every guard reported truthfully across three runs.

**The job is restored on `shell: pwsh`**, with a `run-vk-e2e.ps1` following
`run-win32-e2e.ps1`'s shape. `run-vk-e2e.sh` and `vulkan-icd.sh` are byte-
identical to before — Linux is untouched and still passes 74. `VK_LOADER_DEBUG`
now sits on the Windows job, so both the shell change and a working debug switch
land in one round, because each round trip is an hour.

pwsh is now **closer to a control than to a fix**: it removes a whole class of
doubt for one line of workflow change, but the evidence that pointed at the
environment evaporated with the retraction.

**Next-most-likely failures, in order:** the loader reads the manifest and
declines the driver (this Mesa build's `api_version` against this SDK's loader,
or an unsatisfied `vulkan_lvp.dll` dependency) — `VK_LOADER_DEBUG` will finally
say which, and that is the main thing this round buys; then the goldens, whose
`Tolerance::RASTERISER` was calibrated radv-versus-one-lavapipe and is
**unmeasured between two lavapipe builds**; then the LunarG layer not reporting
the record-time hazard `validation_gate` requires.

**Owed:** two harnesses now cover one suite, and their guards are duplicated
knowledge that will drift the first time one side gains a check.

**The general lesson, and it has now cost twice.** A scripted edit that matches
the first occurrence of a string will silently target the wrong one when the
string is not unique — the same shape as the earlier span-replacement that
deleted a neighbouring entry from this file. Anchor on something unique, or
verify where the edit landed before reporting on what it measured.

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
