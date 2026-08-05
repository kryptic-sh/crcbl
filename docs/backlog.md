# Backlog

What was raised and not finished. A changelog says what shipped; this says what
did not, and why. Delete an entry when it ships — `git log` is the history.

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

- **`VoiceMix::from(&SpatialCue)` drops `itd_samples` on the floor.** A `Voice`
  has no per-channel delay line, so the interaural time difference the cue
  grammar computes — rule 1's other half — reaches nothing. What survives is the
  gain difference, which is the direction without the timing. Documented on the
  `From` impl in `crates/crcbl-audio/src/mixer.rs` so it is not a silent loss.
  Fixing it means a fractional delay per channel inside `Voice::mix_block`, at a
  cost the audio thread has not been measured for.

- **`Mixer::stop` cuts a voice dead rather than fading it.** It removes the
  voice from the list immediately — which is what makes `voice_count` and
  `is_playing` answer without an audio thread to reap, and what horde's cap
  needs — but a loud voice stopped mid-cycle is a click. Asteroids' engine is
  the only caller and it is quiet enough not to matter. A short release ramp
  (mark stopped, fade over one block, then drop) is the fix and it would have to
  keep the immediate accounting: `stop` must still make room in the cap on the
  spot.

- **Nothing has listened to the migrated cues on a real device.** Every sample's
  audio was rewritten onto `crcbl_audio::mixer` and the checks are all
  structural: buffer shapes, pan ordering, voice counts, loop seams. Two
  audible-only risks are unverified. Asteroids' engine changed from a pulsed
  one-shot to a continuous loop at `ENGINE_GAIN` = 0.25 against the one-shots'
  0.5, and that ratio was chosen by reasoning rather than by hearing it. The
  loop seam is asserted to be a bare tone with no envelope, which is the right
  property, but nobody has heard whether ten joins a second is inaudible in
  practice. Both want a person with headphones.

- **Horde's render-side scale table has not been re-measured with the ground and
  the props in it.** `docs/plan/sample/03-horde.md`'s "The render side: flat,
  and not close to a budget" was taken before `assets/terrain.crpix` and
  `assets/props.crpix` existed, and the section says so. Every frame now also
  carries the ground tiles — 300 of them at 960 × 720, which
  `art::tests::the_visible_ground_is_bounded_by_the_view_and_not_the_arena` pins
  — as opaque, full-coverage quads under everything else, and the handful of
  props the view holds. Both are constant additions and neither touches the
  flat-in-the-horde claim the exit criterion is about, but the `sprites` GPU
  column and the CPU column are both now understated by whatever those quads
  cost. Re-running the same `--prefill` series would settle it; the fixture and
  the conditions are written out in that section. Nobody has, because the
  measurement needs the reference machine and a release build, and H1 and H4
  were art slices.

- **The whole horde art overhaul is verified by measurement and none of it by
  eye**, and that is one gap rather than five. The grass ground, the wizard and
  its walk cycle, the three monsters, the props and the potions all landed on
  2026-08-04 without a display in the environment, so every claim about the
  _picture_ is an assertion about baked bytes: silhouettes against colliders,
  palettes against each other's luma, the staff orb's centroid against
  `game::STAFF_MUZZLE`, drop rates off seeded runs. The per-slice entries below
  and under "Told apart at a glance" say what each one specifically has not been
  seen doing. **The grass has the least behind it of the five** — it was checked
  as an offline mosaic rendered from the same `.crpix` bytes and the same hash,
  which is not the shipped pipeline, and no test anywhere asserts that the
  tiling has no visible seam. Half an hour of `cargo run -p horde` on a machine
  with a screen closes more of this than any test that could be written for it.

- **Nobody has looked at horde's props on a screen.** H4 was developed and
  verified headless. `assets/props.crpix`'s two frames are pinned against the
  other sheets by `the_props_sit_between_the_grass_and_the_crowd_in_luma` and
  sized against their colliders by
  `every_prop_silhouette_is_the_size_of_the_collider_it_stands_for`, both in
  `art::tests` — which is the same standing the monsters' art has and no better.
  Three things are therefore unverified rather than wrong: whether a top-down
  canopy reads as a _tree_ at all rather than as a green disc; whether the
  overlap of a wizard standing on a canopy edge looks acceptable in motion (the
  reasoning for accepting it is in `assets/props.crpix`, and it is reasoning);
  and whether the density feels right in play. One run of the demo with a person
  watching settles all three. Related and already recorded below: "Told apart at
  a glance" is the same shape of gap for the crowd.

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

- **Horde's movement tests now share an arena with the scenery.** Several tests
  that predate H4 — `a_player_walking_at_a_wall_stops_at_it`,
  `the_player_moves_at_the_stated_speed_and_a_diagonal_is_no_faster` — walk the
  player across ground that `scatter_props` has since put trees on. They pass,
  and they pass deterministically, because the layout is a pure function of
  `DEFAULT_SEED`. But they pass because that particular layout leaves their
  paths clear, not because anything arranged it: a change to `PROP_DENSITY`,
  `PROP_CELL` or `DEFAULT_SEED` could drop a tree in front of one of them, and
  the failure would look like a movement bug. Two ways out, neither taken: give
  those tests a seed chosen for a clear corridor, or let `Harness::staged` take
  a prop-free arena — which would make them tests of a game that does not exist.

## The goal: a sample depends on `crcbl` and `std`, and it is met bar one line

Stated as a target for the samples on 2026-08-03, and reached on 2026-08-03.
Every one of `apps/{bare,breakout,flappy,asteroids,horde,sandbox}/Cargo.toml`
now names `crcbl` and nothing else under `[dependencies]` — the nine simulation
crates are re-exported, `glam` is `crcbl::math` and `log` is `crcbl::log`. What
is left is one build-dependency and one file.

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

- **`main.rs` is four near-identical copies.** Measured with comments and blanks
  stripped: 37 code lines in breakout and flappy, 39 in asteroids, 42 in horde,
  and `breakout` against `flappy` differs in 8 of the 37 — the crate name, the
  summary fields printed, the exit code's message. It is what the extraction of
  `crcbl::args` left behind: the parser is shared, the front end that calls it
  is not. Smallest remaining copy in the tree and nothing about it is urgent;
  the count is here so it is not measured a third time.

Everything else that was on this list shipped and is out of it. The one
non-obvious residue: each sample's `web.rs` still carries its own
`__crcbl_<sample>_` symbols, which `web/tools/check-exports.mjs` requires to be
literal — see _Considered and declined_. Adopting `crcbl_ui::hud` was declined
on its merits, also below.

## "Told apart at a glance" is the horde art's whole premise and nothing measures it

`apps/horde/assets/actors.crpix` sizes the entire sheet — `art::TEXELS_PER_UNIT`
= 20, and every argument that follows from it — on three enemy kinds being
distinguishable in a crowd. Nothing tests that, and after the Diablo II redraw
it is worth writing down that nothing can:

- `art::tests::the_three_enemy_kinds_are_three_different_pictures` measures each
  outline along eight rays from the frame's centre and asserts the largest
  difference between any pair exceeds 0.12 of the frame's half-width. It rules
  out three sizes of one drawing. It says nothing about legibility, and a pair
  of genuinely different monsters can score low on it — after the redraw
  `fallen` against `quill-rat` is **0.162**, where the shapes it replaced scored
  0.412. The shapes are not less distinct (a horned biped against a spined
  quadruped); the eight-ray metric is just insensitive to the difference,
  because both are drawn out to the edges of their own collider box and the
  metric mostly reports box size. Do not read that number as a legibility
  margin, and do not tighten the threshold expecting it to mean one.
- `the_monsters_sit_between_the_grass_and_the_player_in_luma` and
  `the_monsters_have_a_dark_rim_and_the_player_a_bright_one` pin the two
  brightness relations the sheet argues for. Neither is a legibility test
  either; they are the conditions under which legibility is _possible_.

What would actually measure it is a human looking at a full screen of the crowd,
or a perceptual difference metric over the rendered frames. Neither was
attempted. **The redraw was eyeballed by its author on a static sprite strip
against the grass base colour, not in a running window and not at a crowd
density** — the headless `--prefill 200` run exercises the code path and prints
stats, and no frame from it was looked at.

## The browser's sRGB fix has never been seen in a browser

`crcbl-wgpu` now advertises the sRGB counterparts of a canvas's linear formats
and configures the canvas linear with an sRGB `viewFormats` entry, so the
hardware encode happens on the view rather than not at all. What is verified,
and what is not:

- **Verified by falsification.** `with_srgb_views` and `swapchain_config` are
  the two decision points and both are unit tested against the format list
  WebGPU actually reports (`wgpu-30.0.0/src/backend/webgpu.rs`, whose
  `get_capabilities` returns `Rgba8Unorm`, `Bgra8Unorm` and — only where the
  canvas takes it — `Rgba16Float`). Each test was watched go red with its half
  of the fix removed.
- **Verified by the compiler.** The wasm32 clippy and rustdoc gates both pass,
  which is what CI runs; there is no wasm test harness.
- **Not verified at all: that the frame is no longer dark.** Nothing here can
  open a browser. The diagnosis is read off the shader (`sprite.wgsl`'s fragment
  entry returns `textureSample(...) * tint` with no OETF) and the sheet's format
  (`Rgba8UnormSrgb`, so sampling decodes to linear), and the arithmetic says
  `#19211a` reaches the canvas as roughly `#020302` without the encode — which
  is what "the grass is black" looked like. Confidence in the cause is high;
  confidence that _this_ is the whole of it rests on nobody having loaded the
  page.

`./web/build.sh --serve` and half a minute at `http://localhost:8000` settles
it, and would also settle the eyeball gap recorded above. Until then the entry
stays here.

Also unclosed by this fix: every other sample presents through the same path, so
breakout, flappy and asteroids were dark in a browser too and are expected to
have changed appearance. None of them was looked at either.

## Frame pacing sleeps on the monotonic clock, which is not what a display does

`crcbl::engine::Pacing` chooses a present mode (`Vsync` → `Fifo`, `Adaptive` →
`FifoRelaxed`/`Mailbox`, `Off` → `Mailbox`/`Immediate`) and `FrameLimit` paces
the loop by sleeping the difference between the last frame's length and a
period, on `std::time` — `Clock::Real` in `crates/crcbl/src/engine.rs`, where
the `wasm32` arm of `sleep` is deliberately a no-op because the browser paces
frames itself. That is the whole mechanism, and it is open loop: it never learns
when a frame was actually shown.

Two pieces are named and neither is started. No code under `crates/` requests
either extension; the only mention of one anywhere is the doc comment on
`Pacing` saying why the engine cannot answer which mode is running.

- **Pace on `VK_KHR_present_wait`.** `vkWaitForPresentKHR` blocks until a
  numbered present is on screen, which is the closed-loop version of the sleep
  above and the way to cut a frame of latency without spinning.
  `ash 0.38.0+1.3.281` — the pin — **already binds it**:
  `ash::khr::present_wait::Device::wait_for_present(swapchain, present_id, timeout)`,
  and `vk::PresentIdKHR` is in `definitions.rs`, so the swapchain can number its
  presents. No hand-written FFI needed, contrary to what this was assumed to
  cost. What it needs is the extension pair requested at device creation, an id
  per present, and a wait the loop can skip when the extension is absent. **The
  seam must be named for the capability, never the extension** — v2 of
  `present_wait` is a different contract and should drop in behind the same
  name.

- **Read the real present mode with `VK_EXT_present_timing`.** Today
  `Pacing::Adaptive` is a _request_ with no observation behind it, which the
  enum documents. `present_timing` is **not** in the pinned `ash` (checked: no
  `present_timing` anywhere in its source) and is still provisional, so this
  half is genuine hand-written FFI. It is what would let the engine say which
  mode is running and what the panel's range is; nothing depends on it yet.

## P5B — the job system, and the two decisions in front of it

`crates/crcbl-jobs` carries the spawn seam (`Spawn`, `Threads`, `Inline`,
`default_spawner`) and two of the design's three communication primitives:
`mailbox` (latest-wins triple buffer, for states) and `ring` (bounded SPSC, for
streams). `docs/plan/21-jobs.md` and the roadmap's 2026-08-03 correction carry
the design and the measurements; what belongs here is the ordering, what the
primitives do not do, and the two questions that were not ours to answer.

The order is forced: **the spawn seam and its single-threaded fallback come
first** — done — because `docs/plan/21-jobs.md` records that
`std::thread::spawn` _compiles_ on `wasm32-unknown-unknown` and returns
`UNSUPPORTED_PLATFORM` at run time, so a pool built on `std::thread` is a pool
that silently has no browser story. What is still owed, in order: the
work-stealing pool and `par_for` in both modes, adoption by the four samples
(four consumers is what proves a seam before P6–P8 build on it), then the worker
backend behind it.

**The atomics are checked by Miri and by nothing else.** x86-64 is
total-store-order, so a `Release` store and a `Relaxed` one compile to the same
instruction and weakening one is invisible to any test on this machine. That is
measured rather than assumed: `ring`'s push was weakened to `Relaxed` and the
whole suite stayed green, while `cargo miri test` reported the data race in
`pop` with a backtrace. The Miri job is therefore load-bearing rather than a
nicety: it is the only gate that would catch a wrong ordering before an aarch64
or wasm user does.

**It runs weekly, in `cron.yml`, and that is a deliberate choice rather than an
oversight.** Moving it onto the per-PR path was tried on 2026-08-05 and reverted
the same day: the full crate list is minutes of interpretation on every pull
request, and that is not a price this repository wants to pay for a check whose
per-commit value is concentrated in one small crate. The consequences to keep in
mind:

- **An ordering regression can sit on `main` for up to a week.** Nothing on the
  per-PR path can see one.
- So the obligation moves to the author: `cargo miri test -p crcbl-jobs`
  interprets that crate in about **seventeen seconds**, and any change to its
  atomics is expected to be run under it before it is pushed. That is written
  into the crate docs as well, where somebody editing the atomics will see it.
- The narrower option — a `crcbl-jobs`-only per-PR leg at that seventeen-second
  cost, leaving the broad list weekly — was **not** taken and remains available
  if the weekly cadence ever misses something real.

Also still open: **nothing runs the primitives on a weakly-ordered machine.**
Miri models the memory ordering, which is a stronger check than any test on x86,
but it is a model — an aarch64 runner exercising the same stress tests natively
would be independent evidence, and GitHub offers one. Not attempted, and the
cost is a second `test` leg rather than anything subtle.

**A weekly job is a job nobody watches, and this file has now paid for that
twice.** The Miri run went red on 2026-08-03 because `crcbl-audio` gained a
native device path and `alsa-sys`'s build script wants `libasound2-dev`, which
the cron job never installed; it stayed red until 2026-08-05, and was found only
because the job was briefly moved onto the per-PR path. The install step is
there now. The habit that would have caught it sooner: after a dependency lands,
trigger the cron manually (`gh workflow run cron.yml`) instead of waiting for
Monday.

**`ring` does not implement drop-oldest**, though `21-jobs.md` lists it beside
drop-newest as an overflow policy. It cannot be done from the producer: the read
cursor belongs to the consumer, and a producer advancing it to make room would
be a second writer to it, which is exactly what makes an SPSC ring cheap. `push`
hands the item back and counts the refusal instead, leaving the policy to the
caller. If a real consumer turns up wanting drop-oldest, the honest options are
a consumer-side drain-and-discard or an MPSC design, not a flag on this one.

**The seam has no consumers yet, which is the thing to be honest about.** Its
shape was chosen from the design doc and the topology, not from a caller pushing
back on it — and this workspace's own rule is that a seam is not frozen until
two samples have used it. Expect the adoption slice to change something here;
`Spawn::threaded` returning a `bool` rather than a richer capability is the most
likely candidate, since a caller that wants "threads, but only one" cannot say
so today.

- **Decided 2026-08-05: pin a dated nightly for the wasm worker target only.** A
  threaded wasm artifact needs
  `-C target-feature=+atomics,+bulk-memory,+mutable-globals` and `-Z build-std`,
  which is nightly-only, while `rust-toolchain.toml` pins an **exact stable**
  (`1.97.0`) on purpose — its own comment calls a floating channel a broken
  promise. The answer is the shape `decoder-fuzz` already uses: a nightly pinned
  by date, used for that one target and nothing else, so contributors on stable
  are unaffected and no CI job can go red on an untouched repository.
  `21-jobs.md` records the build clean on `nightly-2026-07-02`, which is
  installed on the development machine. **Not yet done** — it lands with the
  worker backend, which is what needs it.

- **Decided 2026-08-05: prove cross-origin isolation locally before deciding
  about Pages.** GitHub Pages cannot set COOP/COEP, so `crossOriginIsolated` is
  false there, so there is no `SharedArrayBuffer` and no shared-memory input
  ring in the published demos; `coi-serviceworker` is the only route and it is
  third-party JavaScript in a `web/` directory that deliberately has no npm
  dependencies. Rather than spend that call now, the worker backend gets proved
  against a locally served site with real COOP/COEP headers —
  `web/build.sh --serve` is where they go — and the Pages question returns when
  there is something working to publish. **Not yet done**; the headers are not
  in `build.sh` yet, and nothing asserts `crossOriginIsolated` anywhere.

  What this does not change: if the shim is eventually declined, the demos run
  single-threaded through `Inline` and native keeps the full topology, and the
  roadmap's `crossOriginIsolated` gate should then be struck rather than left
  unmeetable. That outcome is exactly what the seam-first ordering exists to
  survive.

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
system} × {honoured, refused} is now executed by a harness on Wayland
(`run-wayland-e2e.sh`); on X11 only the refusing half is.
`crates/crcbl-shell/tests/bin/send_key.rs` is what drives `F11` at a running
sample from outside its process. What is still uncovered:

- **The null GPU backend is excluded from every mode assertion, and correctly.**
  It presents by doing nothing, so no `wl_buffer` is attached, so the surface
  never maps: `swaymsg -t get_tree` lists no `app_id` for a null-backend run
  where a Vulkan one lists `sh.kryptic.crcbl.sandbox` — observed, not inferred.
  An unmapped surface gets no fullscreen configure, so any mode assertion there
  would be checking a window the compositor does not have.
- **X11 under a window manager: the pass exists, the backend does not survive
  it.** Its own section is below, and it is the largest hole on this list.
- **`F11` is only pressed at the sandbox, and only under Wayland.** The four
  games take the same engine-owned path (`crcbl::engine::FULLSCREEN_KEY`), and
  `run-x11-e2e.sh` starts no key sender: the equivalent there is XTEST through
  the suite's own peer client, which is in-process and would need the same
  out-of-process treatment.
- **macOS and Windows have shell backends now, and neither has a game-level mode
  pass.** P5C built both, and each has an end-to-end suite that opens a window,
  flips its mode and reports injected input — but that is the _shell_ being
  driven directly. Nobody presses `F11` at a running sample on either, the way
  `run-wayland-e2e.sh` does: Windows would need the key sender pointed at a game
  rather than at the suite's own window, and macOS has no renderer to run a game
  with until MoltenVK clears P14. See the platform sections below.

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
display, and both had state that survived between them.** Neither showed up
until the environment got a second inhabitant, and both produced the same
symptom: a _tail_ of tests that passed alone and failed in a full run, moving
whenever anything was reordered.

- **X11, the pointer.** `XTEST` leaves it wherever the last test put it, so a
  test that warped it to `(500, 500)` decided where the next test's window was
  placed and whether `openbox` focused it. `Session::open` parks it at the
  centre.
- **X11, the window manager's idea of what is still alive.** A test process
  exiting with a window still mapped destroys it by closing the connection, and
  `openbox` was left with `_NET_ACTIVE_WINDOW` naming an XID that was no longer
  in `_NET_CLIENT_LIST` — after which it focused nothing new for the rest of the
  run. `Session`'s `Drop` withdraws and destroys its windows and then **waits
  for `_NET_CLIENT_LIST` to drop them**, which is the manager saying it has
  finished. Graded evidence, six runs each: no `Drop` at all, 2 clean; `Drop`
  with a fixed four pumps, 5 clean; `Drop` waiting for the client list, 8 of 8.
- **Wayland, the focused workspace.** A test that fullscreens onto the second
  output leaves sway's focus there, so the next test's window opens on a
  1280x720 display and waits out its deadline for a 1920x1080 configure. A
  `FocusedWorkspace` guard puts it back.

The rule that falls out: **anything a test moves and does not move back belongs
in `Session`, not in the test.** The pointer, the input focus, the clipboard
owner, the focused workspace and the compositor's idea of which clients exist
are all this kind of thing.

Two blind alleys, recorded so they are not re-run. Neither is the fix and both
looked convincing: giving the `_NET_ACTIVE_WINDOW` message a real server
timestamp instead of `CurrentTime` (`Peer::server_time`, kept — it is correct
EWMH), and asking `openbox` less often or clicking the frame instead. The click
made it measurably _worse_: five runs, 3-5 failures each.

## What the Win32 backend has and has not been run against

P5C W1, W2 and W3 wrote the whole of `crates/crcbl-shell/src/win32/` on a Linux
machine, and W4 wrote its end-to-end suite there too. It is cross-checked with
`cargo check`/`cargo clippy --target x86_64-pc-windows-msvc`, which do not link
and do not run — **a cross-check proves the code typechecks and nothing more**.

### The runner is a real, non-idle desktop

Three CI round trips established this one assertion at a time, so it is written
down once here rather than rediscovered a fourth time. Every Windows test
written from now on has to hold under all of it:

- **The display is 1024×768.** Smaller than `WindowDesc::default`'s 1280×720, so
  anything that assumes a default-sized window fits on screen is wrong there.
  `ClipCursor` clamps to the virtual screen, which is how this was found.
- **A cursor is over the window, and it keeps moving.** Showing a window under
  it delivers a genuine `WM_MOUSEMOVE`, so the backend's derived pointer arrival
  happens before a test sends anything.
  `the_pointer_enters_moves_clicks_scrolls_and_leaves` has been rewritten for
  this **three** times, and each version assumed a slightly quieter machine than
  the last. It sends a `WM_MOUSELEAVE` to put the derived state on a known edge;
  the first rewrite sent it _before_ the pump that discards the desktop's own
  events, and that pump dispatched a real `WM_MOUSEMOVE`, derived the arrival
  from it, and threw the arrival away. The run collected `[(false, None)]`: a
  leave with nothing in front of it. Draining first and leaving second is
  genuinely part of the fix, and stays — `SendMessageW` calls the window
  procedure synchronously and nothing pumps between the leave and the first
  synthetic movement, so no queued message can be processed in that gap.
- **The foreground can be taken away mid-test, and two tests reported it as
  something else.** `refresh_cursor_visibility` and `refresh_clip` both act only
  for a window that is focused, so a stolen foreground makes each of them a
  no-op — and the assertion that then fails is about the _consequence_.
  `hiding_the_cursor_is_balanced_however_many_times_it_is_asked_for` failed as
  `left: 0, right: -1`, which reads exactly like the `ShowCursor`
  reference-count bug it exists to catch;
  `minimizing_a_captured_window_releases_the_clip` failed naming the wrong
  rectangle when in fact no clip had been applied at all. Both are now routed
  through `focus_and_confirm`, which grants the focus, reads it back and
  retries, so a runner that cannot host the test says so.

  The audit that followed has a clean answer, and it is the reason nothing else
  in that module needs the same treatment: the window procedure handles
  `WM_SETFOCUS`/`WM_KILLFOCUS` **synchronously**, gated on `shared.clipped()`
  rather than on pumped state. So a test that _delivers the focus message
  itself_ and asserts immediately cannot flake, which covers the three
  assertions in
  `confining_the_pointer_clips_it_and_losing_focus_gives_the_desktop_back`; that
  test also asserts `state.focused` after arranging it, which is the line the
  two broken ones were missing. The vulnerable shape is precisely **asking the
  system for focus and then asserting something read back from pumped state**.

  **Neither fix was reproduced.** There is no Windows machine here; both were
  diagnosed by reading the focus gates and matching them against the observed
  values, then typechecked for `x86_64-pc-windows-msvc` from Linux, which proves
  they compile and nothing more. Both went green on the next run — but this
  family failed twice in two consecutive runs on two different tests, so one
  green run is not evidence that it is stable.

  **What it did not fix is the position of our events in the sequence**, and the
  second rewrite still asserted that, requiring the first crossing to be an
  arrival. The runner answered:

  ```text
  the first crossing after a leave is the arrival:
    [(false, None), (true, Some(PhysicalPoint { x: 40.0, y: 30.0 })), (false, None)]
  ```

  Our arrival, at the injected point, with a crossing of the desktop's in front
  of it: the discarding pump had derived an arrival it threw away, so
  `send_leave` was a real transition rather than the no-op it is on a still
  desktop, and it produced a leave that nothing in the test's own sequence
  accounts for.

  **The general rule, which is the part to carry to any other suite that runs on
  a live desktop: identify your own events by their payload, never by their
  index.** A live desktop may insert crossings before, between and after yours,
  so any claim about position-in-the-sequence is a claim that it held still. The
  test now finds its arrival by the point it injected — `(40, 30)`, which no
  real movement reports — and asserts only about that pair: the arrival carries
  the position of the movement it was derived from, the crossing straight after
  it is the leave (safe, because every `send_*` between them is a synchronous
  `SendMessageW`), and **no** arrival carries the second movement's point, which
  is the one-shot `TrackMouseEvent` claim the old alternation check was there
  for. Nothing at all is asserted about the crossings the desktop contributes.

- **Messages arrive that this process did not cause**, every few milliseconds,
  and here is the proof rather than the assertion. Five consecutive timed waits
  on a window whose queue had just been drained to quiescence:

  ```text
  attempt 0: Message after  5.7433ms, queue 0x80008, message id 799
  attempt 1: Message after 16.0896ms, queue 0x400040, message None
  attempt 2: Message after 31.6668ms, queue 0x400040, message None
  attempt 3: Message after   299.7µs, queue 0x80008, message id 96
  attempt 4: Message after 10.9571ms, queue 0x20002, message id 512
  ```

  **799 is `WM_DWMNCRENDERINGCHANGED` and 512 is `WM_MOUSEMOVE`.** The desktop
  window manager is sending composition notifications and the physical cursor is
  moving over the window, continuously, with nothing running on the machine but
  this test. **An idle window with a drained queue does not exist on that
  runner**, and no amount of draining will create one. Any assertion that needs
  quiet is a flaky assertion; assert what a busy desktop cannot fake instead —
  those numbers also say the wait _worked_, since four of the five blocked for
  5.7 ms to 31.7 ms before something woke them, and only the 299 µs one returned
  to an already-full queue.

- **The foreground is somebody else's, and it is _locked_.**
  `SetForegroundWindow` is granted only to a process that already owns the
  foreground, was started by the one that does, received the last input event,
  or asks after the lock timeout has expired with no user input at all. Under
  `nextest` every test is a fresh, short-lived process with none of those, so
  the first e2e run lost three tests to twenty seconds each of being refused —
  the foreground sitting on `0x10200`, the job's own console, every time. What
  defeats it is `SPI_SETFOREGROUNDLOCKTIMEOUT` plus `AttachThreadInput` against
  the current foreground thread, and it lives in `desktop::take_foreground` in
  the **e2e suite**, never in `src/win32/`: a game does not get to steal focus.
  Unrun; see the M4 entry below for what its failure now prints if the lock
  survives both levers.

**W1's first CI run on `windows-latest` answered the open question: the runner
does give a process a usable window station.** 2248 tests passed and 1 failed;
`Win32Shell::open`, `create_window`, the message pump, the borderless round
trip, monitor enumeration and per-monitor DPI all executed against a real
desktop, and `cargo build`/`cargo clippy` linked the hand-written declarations
for real.

**The W2 run then diagnosed both of its own failures**, which is what the
`Wake`-and-`GetQueueStatus` reporting was added for. 2284 tests, 2282 passed:

- `wait_events_genuinely_blocks` reported `Wake::Message` after 14 ms with the
  queue holding `0x400040` — `QS_SENDMESSAGE` in both halves. A message _sent_
  to a window is dispatched by `PeekMessage` but not retrieved, so the bit
  survives the pump, and `MWMO_INPUTAVAILABLE` asks to be woken by exactly that
  bit. `Win32Shell::wait` now drains the queue itself and sleeps with no flags.
- `confining_the_pointer_clips_it_...` reported a clip 12 px narrower on the
  right than the client rectangle, with the right edge at exactly 1024.
  **`ClipCursor` intersects with the virtual screen**, and the runner's display
  is 1024×768 — smaller than the default window. The backend was correct and the
  assertion overstated what the API promises; it now compares against
  `client.intersect(virtual_screen())`. The clamp is real behaviour a game meets
  on a 1366×768 laptop, so the case is described rather than avoided.

**The W3 run reported 2304 tests, 2302 passed, and both failures are addressed
in this slice** — one in the backend, one in the assertion:

- `wait_events_genuinely_blocks` reported `Wake::Message` after 573.9 µs with
  the queue holding `0x80008`. The drain **did** remove `QS_SENDMESSAGE`; what
  is left is `QS_POSTMESSAGE` in both halves — a posted message present now, and
  one arrived since the last check. **This should be impossible.**
  `drain_messages` peeks with `PM_REMOVE`, a null `hwnd` and a zero filter
  range, which takes every window and thread message, and nothing in
  `src/win32/` calls `PostMessageW`, `PostThreadMessageW` or `SetTimer` — so it
  comes from outside this crate, in the sub-millisecond gap between the drain
  and the wait.

  **Not guessed at a third time.** Two rounds were spent on hypotheses and both
  were settled only by making the failure carry data, so the test now prints the
  `MSG` itself through `Win32Shell::peek_pending` (`PM_NOREMOVE`) — id, `hwnd`,
  `wParam`, `lParam` — beside the `QS_` word, and additionally asserts that the
  pre-drain loop _reached_ quiescence, so "this runner's queue never empties"
  and "the wait woke spuriously" stop sharing one failure. **The wait is not
  fixed and is not claimed to be.** The next run either names a message the
  backend should be consuming (fix the backend) or names one no application can
  drain, in which case a wait genuinely cannot sleep on that runner and the test
  should say so and assert what is true. Do not decide which in advance.

- `the_pointer_enters_moves_clicks_scrolls_and_leaves` saw two `PointerMotion`s
  and no arrival, because the desktop's own cursor had already produced one. The
  backend was right and the assertion assumed an empty desktop; see the runner
  section above for the fix and the rule it now states.

**W4 found a bug in the backend that only an out-of-process keystroke could
reach: the pump never called `TranslateMessage`.** A `WM_CHAR` exists only
because that call ran over a key message _in the queue_, so `WM_CHAR` was never
generated for a real keystroke — the `Char` branch of the window procedure, the
surrogate reassembly in `win32/keys.rs` and every `ShellEvent::TextCommit` were
unreachable from a keyboard, and typing into a Crucible window on Windows
produced no text at all. The in-crate suite could not see it: those tests send
`WM_CHAR` with `SendMessageW`, which does not pass through the queue.
`drain_messages` now calls it, and
`a_key_typed_by_another_process_carries_its_position_its_symbol_and_its_text` is
the guard — **unrun, like everything else here**.

**The W4 run reached a real desktop, and its instrumentation ended the
`wait_events` investigation.** The PowerShell harness parsed and ran, the suite
compiled and started, `SendInput` from a second process reached the suite's
window (the sender reported the foreground as ours on every command), and the
resize-coalescing test — a burst of resizes injected from another process,
collapsed into one event — passed. That was the single most uncertain assumption
in the slice and it holds.

Two things it settled, and the second is the one worth carrying:

- **`wait_events_genuinely_blocks` printed the observation that ends it:**
  `the queue holds 0x400040, and the message still in it is None`. A `QS_` bit
  is set and `PeekMessage` with `PM_NOREMOVE` has nothing to return. `0x40` is
  `QS_SENDMESSAGE`: a message _sent_ to a window of this thread is **processed**
  by `PeekMessage` and never _returned_ by it, so it can neither report it nor
  clear its bit — and MSDN says so in its own caveat, that a `QS_` flag being
  set does not guarantee a subsequent `PeekMessage` will return a message. No
  amount of draining fixes it, because there is nothing retrievable to drain.
  The third run's `0x80008` was the same phenomenon showing a different bit.

  The fix is `QS_ALLEVENTS` (`0x04BF`), which is precisely `QS_ALLINPUT` minus
  `QS_SENDMESSAGE`. **That much is settled; the assertion beside it was not, and
  went red again on the M3 run** — see the M4 entry below, which is where the
  observable finally stopped being "the machine went quiet".

- **The transferable lesson, which is not the `QS_` bit.** Four round trips, and
  every one of them was settled only by making the failure carry more evidence
  than the last: a duration, then `Wake`, then the `QS_` word, then the `MSG`
  itself. The first three were hypotheses that each looked like a fix. Applies
  to M4 and to the macOS slices as much as to this one — **when a remote failure
  has no diagnosis, spend the round trip on instrumentation rather than on a
  candidate fix.**

**Genuine OS auto-repeat is not reachable from `SendInput`, and is therefore
uncovered on this backend.**
`a_key_held_by_another_process_reports_its_second_press_as_a_repeat` asserted
press-repeat-release and got press-release: a second `SendInput` key-down for a
key already down is **not a state transition**, so no second `WM_KEYDOWN` is
generated at all. Real typematic comes from the keyboard driver's repeat timer
holding a physically-depressed key, which no injection API reproduces. The test
now asserts the coalescing (renamed
`a_second_injected_press_of_a_held_key_produces_no_second_event`), and the
_decoding_ of `lParam` bit 30 stays covered where it can be — the in-crate suite
builds that `lParam` itself and delivers it with `SendMessageW`. What is
uncovered is that the **system** sets the bit on a real held key. Closing it
needs a physical keyboard or a kernel-level virtual HID device; neither is a CI
runner.

**The e2e harness stopped at the first failure and hid thirteen tests.**
`Summary [0.694s] 2/15 tests run` — nextest's default fail-fast, on a suite
nobody can run locally and whose round trip is half an hour. `run-win32-e2e.ps1`
now passes `--no-fail-fast`. Its count gate needed a second fix to go with it:
the old regex `(\d+) tests? run` matches the digits immediately before the
words, which for `2/15 tests run` is **15** — so a run that executed two tests
reported a healthy-looking fifteen. The pattern is now
`(?:(\d+)/)?(\d+) tests? run` and a present first group is treated as a
cancelled run and fails the gate. Falsified offline against a regex engine with
the same semantics, not against `pwsh`, which is not on the development machine.

**M4 changed two in-crate assertions that the M3 run failed, and neither was a
code change.** `build + test (windows-latest)` went from 2360/2360 green to 2362
passed, 2 failed on a commit that touched only `appkit/` — so both tests were
flaky all along and the runner simply decided differently. They are the second
and third instances of the same rule, and they are why it is written at the top
of this section with its evidence attached:

- **`wait_events_genuinely_blocks` no longer asks the machine to be quiet.** It
  asserted `Wake::TimedOut` on at least one of five attempts; the run printed
  all five and every one was woken early by a real message (see the decoded
  queue evidence above). The observable is now the one that actually separates a
  blocking wait from a no-op one — an attempt that either reached its timeout or
  **slept at all** before being woken. A wait that does nothing returns in
  microseconds every single time; the measured populations are 299 µs for a
  return to a full queue against 5.7 ms for the shortest genuine block, so the
  line is drawn at 2 ms with an order of magnitude of clearance on each side.
  The pre-drain loop still runs and its outcome is still printed, but it is
  **reported rather than asserted**: whether a desktop goes quiet is the
  desktop's business.
- **`the_pointer_enters_moves_clicks_scrolls_and_leaves` stopped counting
  crossings**, and after a third failure stopped indexing them too. Covered in
  the runner section above, because the fix is a fact about the runner rather
  than about that one test.

**The Win32 e2e suite's three foreground failures are addressed and unrun.** All
three timed out identically in `Session::foreground`. Two levers are now pulled
per request — `SPI_SETFOREGROUNDLOCKTIMEOUT` lowered to zero for the session and
restored on the way out, and `AttachThreadInput` against the thread owning the
current foreground window, held across `BringWindowToTop`, `SetForegroundWindow`
and `SetFocus` and released immediately (attaching to a thread that has stopped
pumping is a documented way to hang). Two things about the shape are worth
keeping whatever the next run says:

- **The deadline is four seconds, not twenty.** Everything else in that suite
  waits on another process and deserves a generous deadline; a foreground grant
  is decided by the window station on the turn it is asked for, against rules
  that do not change while a poll spins. Twenty seconds of asking was the same
  refusal four hundred times, three times over, for a minute of CI that taught
  nothing the first second had not.
- **The failure names the lever that did not work.** It prints the foreground
  window's handle, class, title, thread and process, this process's own thread
  and process, and the lock timeout _read back_ after asking for zero. A zero
  there with the foreground still elsewhere means the lock is not what is
  refusing; anything else means `SPI_SETFOREGROUNDLOCKTIMEOUT` was itself
  refused, which it is documented to be for a process that cannot already take
  the foreground. If both levers fail, the honest outcome is that a GitHub
  runner does not let a fresh process take the foreground and the five tests
  that need it cannot run there — which is a finding to record, not a reason to
  make them pass.

**The in-crate suite has the same latent problem and was left alone.**
`confining_the_pointer_clips_it_...` and
`warping_the_pointer_moves_it_to_a_position_in_the_window` call a bare
`SetForegroundWindow` (`make_foreground`, in `src/win32/shell.rs`'s test module)
and have been passing on the runner regardless — the ordinary `build + test` job
is one process, so once it owns the foreground it keeps qualifying. If they ever
start failing, the same dance is the answer and it should move to a shared
helper rather than being written twice.

What remains unverified:

- The structure layouts in `win32/ffi.rs` are asserted by size and offset on the
  host, which catches a missing or wrong-width field but not a _reordering_ of
  two fields of the same width. `DEVMODEW` is the one with two unions in it and
  the one to re-read if a refresh rate ever looks implausible; `RAWMOUSE` is the
  one to re-read if a raw delta does. W3 found a third hole while falsifying
  that guard: **a field whose width shrinks into its own trailing padding moves
  no offset and is not caught** — narrowing `DropFiles::p_files` from `u32` to
  `u16` leaves every assertion green. Nothing depends on it today (that field is
  written by a test and read by nothing), but the assertion is weaker than it
  reads.
- The pure arithmetic (`win32/geometry.rs`, `win32/events.rs`, `win32/keys.rs`,
  `win32/pointer.rs`, `TimeBase`, `win32/proc.rs`'s shared state) _is_ covered
  on Linux and each of its guards was falsified by mutation before this was
  written.

What W2 adds to that list, in the order it would hurt:

- **No input has ever been delivered by a real device.** The Windows suite
  drives the window procedure with `SendMessageW`, which is the real procedure
  against the real cached state but not the real message stream. Nothing has
  confirmed that `WM_KEYDOWN`'s `lParam` carries what `keys::scancode` expects
  from an actual keyboard, or that `GetMessageTime` inside a procedure answers
  for the message being dispatched.
- **`WM_INPUT` is untestable from CI and is untested.** A raw report needs an
  `HRAWINPUT` only the system can produce, so `input::read_raw_mouse` and the
  `RIM_TYPE_MOUSE` check have never run. `pointer::RawMotion`'s
  absolute-versus-relative arithmetic _is_ covered on Linux; the plumbing that
  feeds it is not. The absolute path additionally needs a machine that produces
  absolute reports — a remote-desktop session or a tablet — which no runner has.
- **`ClipCursor` and `SetCursorPos` are restricted to the foreground process.**
  `confining_the_pointer_clips_it_and_losing_focus_gives_the_desktop_back` and
  `warping_the_pointer_moves_it_to_a_position_in_the_window` call
  `SetForegroundWindow` first and then assert against the system's own state
  (`GetClipCursor`, `GetCursorPos`). If a GitHub runner does not let a process
  take the foreground, both fail — which is a finding about the runner rather
  than about the code, and is the reason to read the failure before changing
  anything.
- **`MapVirtualKeyW(.., MAPVK_VK_TO_CHAR)` is assumed to answer the uppercase
  letter** for a letter key, which `input::unshifted` then lowercases so that a
  rebind menu reads the same as it does on Linux. If the call already answers
  lowercase, the lowercasing is a no-op and nothing changes;
  `a_key_press_carries_its_position_its_symbol_and_its_repeat_flag` asserts the
  lowercase result either way.
- **The `ShowCursor` balance is asserted through the count itself**
  (`cursor_display_count` reads it by moving it and putting it back). That test
  is the only thing standing between this backend and an invisible cursor for
  the rest of a session, so it is the one to keep rather than relax.

What W3 adds:

- **No file has ever been dragged onto a window.**
  `a_drop_becomes_one_event_...` builds a `DROPFILES` block by hand and sends
  `WM_DROPFILES`, which runs the real procedure, the real
  `DragQueryFileW`/`DragQueryPoint`/`DragFinish` and the real translation — but
  the block is this project's idea of what the shell sends, not the shell's. If
  shell32 rejects it the test reports zero files and reads exactly like a
  backend bug: `ffi::DropFiles`'s size assertion is the first thing to re-check,
  and `f_wide` the second.
- **`DragAcceptFiles` and the `WS_EX_ACCEPTFILES` round trip are asserted
  through the style word.**
  `the_drop_registration_survives_a_trip_through_borderless` reads the bit back
  after a borderless flip and then sends a drop. What it cannot show is that a
  _real_ drag would have been offered to the window, since that is a decision
  the shell makes in another process.
- **No other process has ever contended for the clipboard**, so `Opened::After`
  and `Opened::Refused` have never been produced. The retry loop itself is
  therefore unexercised; only the budget arithmetic is covered, on Linux. A
  clipboard-manager utility on a developer machine is the cheapest way to reach
  the retry path if it ever needs proving.
- **`GlobalSize` returning more than was requested has not been observed**, so
  the padding half of `Clipboard::put`'s zeroing is belt rather than a fix for
  something seen. It is cheap and it makes the read-back trimming exact either
  way.
- **The clipboard tests share the desktop's clipboard.** They write to the real
  window station, so two Windows suites running concurrently on one runner would
  interfere. Nothing does that today; it is worth knowing before anything is
  parallelised across processes. W4's suite is `--test-threads 1` for exactly
  this reason, and for the foreground window and the cursor beside it.

What W4 closes, and what it does not:

- **Closed, subject to a runner confirming it.** Input now arrives as a real
  message stream — posted, queued, translated, dispatched — from a separate
  process (`tests/bin/send_input_win32.rs`, `SendInput`), which is what found
  the missing `TranslateMessage`. The clipboard now round-trips with a second
  process in both directions, and `another_process_reads_what_we_copied_...`
  asserts it happens with **zero** pumps of this shell's loop, which is the
  claim that separates Win32's content-transfer clipboard from X11's and
  Wayland's ownership model. Focus arrives through a real foreground change
  rather than a hand-sent `WM_SETFOCUS`, and mode flips and resize storms are
  judged by `GetWindowRect`/`GetClientRect` rather than by the backend's own
  bookkeeping.
- **`WM_INPUT` is now attempted and may still not be reachable.**
  `injected_motion_arrives_as_raw_relative_motion_for_mouselook` assumes
  `SendInput` feeds the raw input stack on a `windows-latest` image. That is the
  single assertion in the suite most likely to be answered by the runner rather
  than by the backend; if it fails with the ordinary `PointerMotion` present and
  `raw_delta` absent, the finding is about the image and the test should say so
  rather than be deleted. The **absolute** raw path still needs a device that
  reports absolutely — a remote-desktop session or a tablet — which no runner
  has.
- **Auto-repeat is not the driver's.** Windows typematic comes from the
  keyboard, and an injected key does not repeat, so
  `a_key_held_by_another_process_reports_its_second_press_as_a_repeat` sends two
  presses and reads bit 30, which the _system_ sets. That is the same bit a real
  hold sets, so the claim is sound; what is untested is the driver's timing.
- **Drag and drop is still not end to end.** A `WM_DROPFILES` carries an `HDROP`
  the shell allocated _in the target process's_ context, so no test process can
  hand one over — the in-crate suite's synthetic block remains the only
  coverage, with the caveat recorded above.
- **No sample-level pass, and there cannot be one yet.** The Linux suites run
  the sandbox and press F11 at it; that needs a renderer, and neither
  `windows-latest` nor `macos-latest` has a Vulkan device
  (`docs/plan/ROADMAP.md`, 2026-08-04 correction — MoltenVK is gated behind
  P14). This is a coverage gap stated rather than approximated: nothing in
  `tests/win32_e2e.rs` pretends to cover the `set_mode`-from-a-key path in a
  running game.
- **`tests/run-win32-e2e.ps1` has never been parsed by a PowerShell.** It was
  written on a machine with no `pwsh`, so its syntax, its `$LASTEXITCODE`
  handling through `Tee-Object`, and its ANSI-stripping regex are all unrun. The
  CI job is the first execution. If it fails before the suite starts, the script
  is the suspect and not the backend.
- **Nothing in `tests/win32_e2e.rs` has ever executed.** It is a Windows suite
  written on Linux; the cross-target check compiles it and does not link it. Its
  failure messages are written on that assumption — they carry the queue word,
  the foreground handle, the actual rectangle and the helper process's own
  output, because the last two CI failures were solved by assertions that
  printed data and the one before that wasted a round trip by printing a
  duration.

## Owed on the Win32 backend after W3

- **There is no drag feedback, only drops.** While a file is over the window
  nothing highlights and the cursor stays the system's default: `WM_DROPFILES`
  is a notification, not a conversation. `DragEnter`/`DragOver`, a drop cursor,
  non-file formats and copy-versus-move all need `RegisterDragDrop` and an
  `IDropTarget`, which is **COM** — `OleInitialize` on the pumping thread, a
  hand-written vtable with `IUnknown` reference counting, and an apartment this
  crate does not own and cannot uninitialise without knowing what the host
  process put in it. Considered and declined for W3: it buys feedback rather
  than drops, and `ShellEvent::DroppedFile` is what the seam actually names.
  Owed before the editor's asset browser wants a drop target that looks like one
  (P12), the same milestone XDND on X11 is owed for.
- **`MimeType::UriList` is a registered format, not `CF_HDROP`.** A "copy file"
  from Explorer puts `CF_HDROP` on the clipboard, and this backend does not read
  it: a request for `text/uri-list` finds whatever was published under that
  registered name and is otherwise `Empty`. Rendering `CF_HDROP` as a
  `text/uri-list` blob means _encoding_ URIs, and the shared decoder
  `clipboard::parse_uri_list` cannot round-trip a Windows path — `file:///C:/a`
  decodes to the path `/C:/a`, which is not a file. Closing this means either a
  Windows-aware `file:` encoder plus a matching decoder, or delivering
  `CF_HDROP` as paths through a different route. Not attempted; named so the gap
  is not rediscovered as a bug.
- **A clipboard payload whose last bytes are NUL loses them.** Payloads are
  written NUL-terminated and read back with trailing NULs trimmed, which is what
  makes a `GlobalSize` larger than the request harmless and is what other
  applications do for registered text formats. Neither of the engine's two
  formats is binary; an `Other("image/png")` offer ending in NUL would be
  truncated. Recorded in `win32::clipboard`'s module docs as well.
- **`SetCurrentProcessExplicitAppUserModelID` is now one library closer.** W3
  links `shell32` for the drop calls, so the "it needs a third system library"
  half of the `app_id` entry below is no longer a cost — what is left is the
  decision about _where_ a process-wide property is set when a host application
  embedded the engine.

## Owed on the Win32 backend after W2

- **`WindowDesc::app_id` is validated and never applied.** Win32's equivalent of
  `WM_CLASS` is the Application User Model ID, set process-wide with
  `SetCurrentProcessExplicitAppUserModelID` from `shell32` — it decides taskbar
  grouping and which shortcut a window matches. W1 rejects an id containing a
  NUL and otherwise ignores it, so a Crucible window groups under whatever
  Explorer infers. Wiring it means a third system library and a decision about
  _where_ a process-wide property is set (opening a shell is the wrong place if
  a host application embedded the engine).
- **`ShellCaps::TEXT_IME` is clear although typing works.** `WM_CHAR` is
  handled, including surrogate pairs, and leaving `DefWindowProc` to run the
  default IME does deliver a committed CJK string through it — so composition
  probably works today, invisibly. The bit stays clear because it claims the
  commit path is _wired to an input method_: nothing here touches
  `WM_IME_STARTCOMPOSITION`/`WM_IME_COMPOSITION`/`WM_IME_ENDCOMPOSITION`, the
  seam cannot tell a pre-edit from a commit, and there is no way to place the
  candidate window at the caret. Wayland latches the same bit on having _bound_
  `text-input-v3`; matching that standard here means handling the `WM_IME_*`
  family and giving the seam a pre-edit event, which is its own slice. The
  argument is written out in `Win32Shell::caps`.
- **`DeviceId` names a device kind, not a device.** `KEYBOARD_DEVICE` and
  `POINTER_DEVICE` in `win32/input.rs` are constants, the same admission the X11
  backend makes. Windows is better placed to fix this than X11 is —
  `RAWINPUTHEADER::hDevice` identifies the physical device on every `WM_INPUT` —
  but turning a handle into a stable `DeviceId` needs a handle table and a
  hotplug story (`WM_INPUT_DEVICE_CHANGE`), and raw input would have to become
  the source of button and wheel events too rather than only of motion.
  Local-multiplayer device assignment is what wants it.
- **A modal drag-resize accumulates raw motion.** `WM_INPUT` keeps arriving
  while Windows runs its own message loop, and nothing drains the queue until
  the drag ends, so a three-second edge drag delivers a few thousand
  `PointerMotion` events in one `pump`. It is bounded (the drag is finite) and
  it is not a leak, but it is a burst. Not fixed because the two obvious fixes
  are both wrong: coalescing relative samples loses the per-event timing
  `docs/plan/19-input.md`'s pattern evaluator is a function of, and dropping
  them needs a "we are inside a modal loop" flag whose only consumer would be
  this. The pointer is on a window edge rather than in mouselook while it
  happens.
- **Refresh rate is a whole hertz, so 59.94 Hz reports as 60.**
  `EnumDisplaySettingsW`'s `DEVMODEW::dmDisplayFrequency` is an integer, and
  `MonitorInfo::refresh_millihertz` exists precisely because that rounding
  matters to frame pacing. The exact figure is in `QueryDisplayConfig`'s
  `DISPLAYCONFIG_RATIONAL` (60000/1001), which needs a second display API and
  its own path-and-mode array walk. Windows is the only one of the five backends
  with this gap; it is worth closing when frame pacing lands.
- **A monitor's name is its device name (`\\.\DISPLAY1`), not its marketing
  name.** `MonitorInfo::name` already documents itself as display-only and
  unstable, and the alternatives are worse (`EnumDisplayDevicesW` usually
  answers "Generic PnP Monitor") or bigger (`QueryDisplayConfig` again).
  Considered and declined for W1.
- **A window frozen during a user drag-resize is accepted, not fixed.** Windows
  runs its own modal message loop between `WM_ENTERSIZEMOVE` and
  `WM_EXITSIZEMOVE`, so `pump` does not return and no frame is rendered until
  the mouse is released. The resize storm is coalesced so the delay is not also
  an event flood, but the freeze is real. The usual fix — `SetTimer` plus a
  frame rendered from `WM_TIMER` inside the modal loop — **cannot be built in
  this crate**: rendering a frame means calling the engine, and the shell
  deliberately has no `Shell::run(closure)` to call back into (the crate docs
  explain why, and wasm is the reason). Closing it needs a second seam, "render
  one frame now", which is a decision above `crcbl-shell`. Recorded here rather
  than attempted.

## What the AppKit backend has and has not been run against

P5C M1 wrote the whole of `crates/crcbl-shell/src/appkit/` on a Linux machine.
It is cross-checked with
`cargo check`/`cargo clippy --target aarch64-apple-darwin`, which do not link
and do not run — **a cross-check proves the code typechecks and nothing more.**
The window lifecycle has since executed on a `macos-latest` runner through
`tests/appkit_session.rs`; everything below is written against what that pass
does and does not reach.

### The window session cannot be a `#[test]`, and that is measured rather than assumed

This is the finding to know before writing any macOS test, because it decides
the shape of the whole suite:

- **AppKit is main-thread-only and enforces it by raising.**
  `-[NSApplication nextEventMatchingMask:untilDate:inMode:dequeue:]` asserts on
  the thread and throws `NSInternalInconsistencyException`; an Objective-C
  exception unwinding through a Rust frame is undefined behaviour, not an error
  return. So `AppKitShell::open` refuses off the main thread
  (`appkit::app::require_main_thread`) rather than finding out.
- **Rust's `libtest` always runs a test body on a thread it spawns.** Measured
  on this workspace's toolchain, not recalled: a probe crate asserting
  `gettid() == getpid()` fails under `cargo nextest run`, and fails again when
  the test binary is invoked directly with `--exact <name> --test-threads=1`.
  The serial path in `libtest` does not put the body on the main thread.

**The rule those two findings generalise to, now with a second instance behind
it: on macOS a test's _thread_ and its _app state_ are part of its
preconditions, and `#[test]` supplies neither.** M2 shipped a green `#[test]`
asserting every `CursorIcon`'s `NSCursor` selector exists and answers a non-nil
object; the `macos-latest` runner failed it with
`+[NSCursor "arrowCursor"] answered nil`. The selector table was correct — the
environment was one in which an AppKit **object** cannot be created, because
`libtest` runs bodies on a spawned thread in a process where `NSApplication` was
never initialised.

The dividing line, and it is the question to ask before writing any macOS test:

- **The Objective-C runtime is thread-safe and needs no application.**
  `objc_getClass`, `sel_registerName`, `objc_allocateClassPair`,
  `class_addMethod`, `objc_msgSend` against Foundation. `appkit::view`'s and
  `appkit::shell`'s suites are all of this shape and stay `#[test]`s.
- **CoreGraphics is thread-safe too.** `CGMainDisplayID`,
  `CGWarpMouseCursorPosition`, `CGAssociateMouseAndMouseCursorPosition` —
  `appkit::input`'s two remaining tests.
- **An AppKit object needs both the main thread and `NSApplication`.** Cursors,
  screens, pasteboards, views, windows. These live in
  `crcbl_shell::session_support`, called from `tests/appkit_session.rs`, and the
  M2 cursor test was moved there unchanged.

`session_support`'s functions answer `Result<(), String>` rather than asserting,
so the failure names the selector or the value that disagreed — which is what
made the M2 failure diagnosable at a glance and is the same reason `Wake` is a
named value rather than a duration.

Together those mean a `#[test]` can never drive an AppKit window. **Closed in
the same commit** by `crates/crcbl-shell/tests/appkit_session.rs`, a
`harness = false` target that owns its `main` and therefore runs _as_ the
process: it opens the backend, creates a window, pumps to the first `Resized`,
reads the size back, flips to borderless and back, and destroys it. The
alternative considered and declined was re-executing the test binary as a child
process with `--test-threads=1` — it does not work, for the measured reason
above.

Two things about that target are worth knowing before touching it. It is **not**
feature-gated and it runs on every host, because cargo builds and runs a
harness-less target everywhere; off macOS it prints why it did nothing rather
than reporting a pass it did not earn. And it was falsified the way any other
guard is: a `panic!` in its body fails both `cargo test` (exit 101) and
`cargo nextest run`, so it is genuinely wired into the run and can go red.

**A `harness = false` target owes libtest's list protocol, and forgetting it
broke three CI jobs at once.** `cargo nextest` enumerates a binary by running it
with `--list --format terse` and parsing `<name>: test` lines out of stdout; the
first version of this target ignored argv, so the _listing_ step ran the whole
window session and then failed to parse its prose — `test (linux)`,
`coverage (linux)` and `build + test (macos-latest)` all died on
`creating test list failed`. `main` now answers `--list` (and the empty
`--ignored` listing) before anything else happens.

The reason it was missed is the transferable part: the target **was** falsified,
under `cargo test`, and `cargo test` does not enumerate. CI runs
`cargo nextest`. A guard checked under one runner and shipped under another is a
guard that was not checked — anything owning its own `main` has to be verified
with `cargo nextest list` as well as `cargo nextest run`.

**It has now run on a Mac, and every step passed.** On `macos-latest`: the
backend opened on the main thread, reported
`ASPECT_HINT_HONORED | MULTI_WINDOW | WINDOW_POSITION | SERVER_DECORATIONS | EVENT_WAIT`
and one monitor, configured the window at the requested 640×480, took the whole
1024×768 screen borderless, restored 640×480 exactly on the way back, and
destroyed the window. So a GitHub macOS runner **does** give a process a usable
WindowServer session, `NSApplication` bootstraps from a plain unbundled binary
with `setActivationPolicy:`, and M1's lifecycle is executed rather than only
typechecked. What is listed as uncovered below is what that pass does _not_
reach.

### What the macOS suite does cover

Not nothing, and it covers the highest-risk item in the backend:

- **Every `objc_msgSend` signature shape this backend transmutes**, dispatched
  against Foundation classes that are thread-safe and against a class built at
  runtime for the purpose (`CrcblFfiProbe`, in `appkit::shell`'s tests) — so
  both sides of the call are ours and a mismatch shows up as a wrong value
  rather than as a crash somewhere later. That includes the `NSRect` return,
  which must go through `objc_msgSend_stret` on x86_64 and must **not** on
  aarch64, an `NSRect` argument, `BOOL` in both directions, and
  `class_addMethod` with its type encodings.
- **The main-thread refusal**, exercised from a thread spawned explicitly rather
  than relying on `libtest`'s.
- **That the runner has a graphics session at all**, through `CGMainDisplayID` —
  CoreGraphics is thread-safe, so this is the one thing about the runner's
  display a spawned test body may ask.
- **The two CoreGraphics calls M2's pointer modes rest on**, for the same
  reason: `CGWarpMouseCursorPosition` really moves the cursor and
  `CGEventGetLocation` reports it back at the point named, and
  `CGAssociateMouseAndMouseCursorPosition` is accepted in both directions. Both
  tests restore what they changed before asserting, because the runner's desktop
  is real.
- **That every `NSCursor` selector `pointer::cursor_selector` names exists in
  this AppKit** and answers with a non-nil cursor — the half a host test cannot
  reach, since it has no `NSCursor` to ask. It is checked from the **session
  target**, not from a `#[test]`; see the rule below for why the `#[test]`
  version was green here and red on the runner.
- **That `CrcblView`'s `registerForDraggedTypes:` really registers, and that a
  view that never called it accepts nothing.** Read back through
  `-[NSView registeredDraggedTypes]`, which makes the `accept_drops` gate a
  mechanism rather than a promise. Also from the session target.
- **A round trip through the real system pasteboard**, in both formats, twice
  over: `session_support::pasteboard_round_trip` through the FFI, asserting that
  `-[NSPasteboard changeCount]` advanced across the write, and the session's own
  `clipboard()` through the `Shell` seam, asserting that each accepted read is
  answered **exactly once** and that the three `ClipboardContent` outcomes are
  told apart.
- **That `CrcblView` really installs every responder and `NSTextInputClient`
  method and claims the protocol.** The class is built by the same
  `view::view_class` the real path calls, and `objc_allocateClassPair` has no
  AppKit in it, so a spawned test body may build and inspect it. That catches a
  refused `class_addMethod` and a misspelled selector, which are otherwise
  silent.
- The pure modules — `appkit::geometry`, `appkit::events`, `appkit::keys`,
  `appkit::pointer` and `appkit::TimeBase` — run on **every** host including
  this Linux one, which is where the Y flip, the points-to-backing-pixels
  conversion, the `kVK_*` table, the modifier reconstruction, the scroll units
  and the timestamp rebasing are falsifiable. Every guard in them was falsified
  by mutation before this was written.

### Uncovered, and why each one is uncovered

- **`NSApplicationPresentationOptions`.** An invalid combination _raises_, and
  `geometry::presentation_options` never produces one — asserted as a pairing in
  a host test. The session pass flips to borderless and back, so the legal pair
  _is_ now accepted by a running `NSApplication`; what is unverified is that no
  path can produce the illegal one.
- **Reference counting.** `releasedWhenClosed` is turned off at creation and
  `appkit::shell::release_window` is the single matching release for the window
  and the layer. Reasoned, not observed; there is no leak check anywhere in this
  workspace and Instruments is not in CI.
- **The delegate-to-shell table.** `appkit::app::DELEGATES` is a thread-local
  `Vec<(usize, *const Shared)>` rather than an Objective-C instance variable,
  deliberately: the ivar route is `ivar_getOffset` plus pointer arithmetic into
  the middle of an object, which corrupts rather than fails when it is wrong,
  and none of it can be falsified from a Linux machine. The table is safe Rust
  with the same lifetime story. Reconsider if a profile ever shows the lookup.
- **The `NSWindow` subclass is now half observed.** `CrcblWindow` overrides
  `canBecomeKeyWindow` and `canBecomeMainWindow` so that a borderless window can
  take the keyboard. The M4 run read the live window back and reported
  `class: "NSKVONotifying_CrcblWindow"` with `can_become_key: true` — so the
  runtime-built subclass took, AppKit's KVO subclassed it in turn as it does for
  any observed object, and **the override is installed and answering**. What is
  still unseen is the override mattering: no keystroke has been delivered to a
  borderless window, because no keystroke has been delivered at all (see the M5
  entry below).

### What M4 wrote, and what the runner answered

M4 is the end-to-end pass: input the window system generated, a pasteboard round
trip against another process, and AppKit as the judge of the window. It was
written on Linux and cross-checked with
`cargo clippy --target aarch64-apple-darwin`, which does not link and does not
run.

**It has now run, and it stopped before the injection.** The session got as far
as the cursor shapes, the dragged-type registration, the general pasteboard and
a window configured at 640×480, then waited ten seconds for a key window that
never came. The diagnostic added for exactly this said which mechanism refused:

```text
-[NSApp keyWindow] is nil and the application is inactive
Activation { app_active: false, windows: 1, title: Some("crcbl appkit session"),
             class: "NSKVONotifying_CrcblWindow", visible: true,
             can_become_key: true, is_key: false }
```

**So the finding is about activation and not about TCC**, which was the risk
this section had been written around. A GitHub macOS runner gives an unbundled
binary a window server and a window and does **not** give it activation on the
backend's own polite request, even though `setActivationPolicy:Regular` was
accepted (`open()` fails loudly otherwise), `finishLaunching` ran, and
`activate` was sent in both the macOS 14 and the legacy spelling.

M5 is what follows from it, and it is three things:

- **The readback stopped going through the key window**, which is where nearly
  all the recovered coverage is. Every fact M4 asserts about the window —
  `acceptsMouseMovedEvents`, the first responder, the registered dragged types,
  the frame, the content extent, the backing scale, the screen, the style mask —
  is an ordinary `NSWindow` or `NSView` accessor that answers whether or not the
  application is active. `session_support::window_facts` finds this process's
  own window by title among `-[NSApp windows]` and reads them there;
  `app_active` and `is_key` became reported fields of `WindowFacts` rather than
  preconditions for reading anything. `key_window` stays for its one genuine
  caller, the check that this process holds the keyboard before anything is
  posted. **The rule is general: a precondition belongs on the assertions that
  need it, not on the function that gathers the evidence.**
- **The harness asks for activation itself**, in `frontmost::ask` in
  `tests/appkit_session.rs` — `-[NSRunningApplication currentApplication]` plus
  `activateWithOptions:` with `NSApplicationActivateIgnoringOtherApps`, and the
  legacy `-[NSApp activateIgnoringOtherApps:]` beside it, both guarded by
  `respondsToSelector:`. It is deliberately **not** in `src/appkit/`, on exactly
  the argument `tests/win32_e2e.rs`'s `desktop::take_foreground` gives: a
  harness may arrange a precondition a backend must never arrange for itself,
  because a game does not get to steal the focus. It is judged by
  `-[NSApp isActive]` afterwards rather than by what the method returned, and
  pulled once per turn of the poll rather than once.
- **If activation is still refused, only the injection is skipped**, loudly. See
  the entry below; that is the coverage gap this slice leaves behind.

**The runner granted it, and the harness lever is why.** The M5 run reported
`-[NSRunningApplication activateWithOptions:] answered true`, then
`application active true, window key true`, then went on to inject. So a GitHub
macOS runner **does** have a foreground session an unbundled binary can be put
into — it simply will not hand it over to the cooperative
`-[NSApplication activate]` that the backend is right to be limited to. The
whole M4 judge passes on that runner, the warp lands exactly, and **TCC did not
block activation**; whether it blocks `CGEventPost` is answered by the injection
assertions themselves, which now run.

`injection_skipped` is therefore **written and unrun**. It stays: the same
refusal is what a developer's laptop gives a background process, and the branch
that keeps a real coverage gap honest is worth more than the lines it costs.

**Why `activateWithOptions:` worked where the backend's own attempt did not.**
The backend's `activate` prefers `-[NSApplication activate]` where
`respondsToSelector:` finds it, which on macOS 14 and later is the _cooperative_
spelling: it asks, and an application that is not entitled to interrupt whatever
is frontmost does not get to. The forceful spellings are deprecated as of 14 and
not removed, and deprecation is a compiler diagnostic in Objective-C and nothing
at all across `objc_msgSend` — so the harness reaches a lever the backend never
takes, which is the whole shape of the harness/backend split here.

### TCC does not gate `CGEventPost` back to the posting process — settled

**This was the single largest open risk in the macOS half, and it is now closed
by observation rather than by argument.** The whole M4 slice was written around
the possibility that macOS 10.14+'s Accessibility gate would refuse synthesized
keyboard events on a runner with nobody to grant the right, in which case none
of the injected input could ever arrive. It does not, at least for events posted
by a process and delivered back to that same process:

- The posted `kVK_ANSI_A` came back as a `ShellEvent::Key` with the right
  scancode, `KeyCode`, keysym and `Pressed`/`Released` pair.
- **`TextCommit("a")` arrived with it.** That means `sendEvent:` routed the
  event to the first responder, `keyDown:` handed it to `interpretKeyEvents:`,
  the input method called `insertText:replacementRange:`, and `CrcblView`'s
  `inputContext` was non-nil — which is true only because the class conforms to
  `NSTextInputClient`. Every link in that chain was written on a Linux machine
  and none of them had ever run.
- The arrow key produced a `Key` and **no** `TextCommit`, so the backend is
  asking the input method rather than reading `-[NSEvent characters]`.
- The injected pointer motion arrived and its position was right.

So **macOS is no longer where Windows was before its e2e suite found
`TranslateMessage`**, and `ShellEvent::TextCommit` on this backend is executable
coverage rather than a structural claim. `AXIsProcessTrusted()` stays uncalled
and is now unlikely ever to be worth adding.

The `postEvent:atStart:` fallback is therefore **not needed and should not be
written.** It is recorded here only so nobody re-derives it: it would have been
`-[NSApplication postEvent:atStart:]` with an `NSEvent` from
`+[NSEvent keyEventWithType:…]`, needing no permission and reaching everything
but the window server's own leg — at the cost of a ten-argument `objc_msgSend`
transmute with an `NSPoint` by value, which is exactly the class of FFI this
crate says must not be written blind. The real path works; this would be
strictly less coverage for strictly more risk.

### A warp is not an event, and that is the M5 run's own finding

`CGWarpMouseCursorPosition` moves the cursor and **posts nothing**. Apple
documents it that way; M4 did not read it that way, and wrote `wait_for_pointer`
as "warp, then wait for the pointer to be reported". The run showed both halves
of what that means in one log, which is why this is worth writing down rather
than just fixing:

- `input`'s warp goes from _outside_ the window to a point inside it, and it
  **passed** —
  `warped to PhysicalPoint { x: 480.0, y: 240.0 } and landed at PhysicalPoint { x: 480.0, y: 240.0 }`.
  Not because the warp reported anything: AppKit re-evaluates its tracking areas
  against where the cursor actually is, so crossing the boundary produces a
  `mouseEntered:` and therefore a `PointerFocus` carrying a position. The warp
  was never the thing being observed.
- `injected_input`'s warp goes from one point _inside_ the window to another. It
  crosses nothing, so there is no `mouseEntered:`, and a warp generates no
  `mouseMoved:` either. It waited the full ten seconds and collected
  `["MonitorsChanged"]`.

The fix is that `wait_for_pointer` now posts a real `kCGEventMouseMoved` at a
caller-supplied point on **every turn** of its poll — every turn, because
`CGWarpMouseCursorPosition` suppresses local events briefly afterwards and a
single swallowed post would be a flake rather than a finding. The warp is left
to do only what it promises. Three consequences worth keeping:

- **The round trip is stronger than it was**, not weaker. The seam converts the
  target into Quartz's global space and warps; `quartz::cursor` reads back the
  global point it chose; the posted move goes to _that_ point; the backend
  converts what the window server delivers back into the seam's space. Both
  conversions are now checked against each other rather than one of them against
  a tracking-area crossing.
- **The warp round trip moved behind the activation gate**, into
  `warp_round_trip`, because it posts and `CGEventPost` goes to whoever is
  frontmost. `input` keeps everything that posts nothing — the capability set,
  the confine refusal, the clamped out-of-window warp, the pointer modes, the
  cursors — and stays unconditional.
- **The injected motion is identified by distance, not by recency.**
  `wait_for_pointer` posts moves at the parked point until one is reported, and
  the last of those can still be in flight when the next collection starts, so
  "the most recent `PointerMotion`" could be a stale report of `parked` compared
  against itself. The filter takes any motion at least half of `NUDGE` away in
  both axes and the assertions take the **direction**, deliberately — a backend
  that reflected the Y reports a point `NUDGE.1` _above_ `parked`, which the
  filter admits and the assertion catches. Filtering on direction would have
  hidden the exact bug the check exists for. Same rule as the Win32 crossing
  fix: on a live desktop, find your own event by its payload.

### The payload rule was written down and then not applied one caller up

The bullet above ends "on a live desktop, find your own event by its payload",
and `wait_for_pointer` — the function that bullet is _about_ — went on taking
the first position of any value. It flaked on a docs-only commit, `c6531c4`,
whose code was byte-identical to a run that had passed:

```text
warped to PhysicalPoint { x: 480.0, y: 240.0 } in a
PhysicalSize { width: 640, height: 480 } window and the window system
reported PhysicalPoint { x: 320.0, y: 240.0 }
```

`(320, 240)` is the window's exact centre, and nothing was broken: it is where
`PointerMode::Locked` puts the cursor before freezing it
(`appkit::input::centre_pointer`). `input` sets and clears that lock without
pumping afterwards, AppKit re-evaluates its tracking areas against a warp, and
so a **truthful report of where the cursor was one step earlier** was still in
flight when the next question was asked. It arrived first and was read as the
answer.

Two things are worth keeping from it:

- **Draining before the warp does not fix this class of bug**, and reaching for
  it would have looked like a fix. The window server delivers asynchronously, so
  the stale report need not have _arrived_ by the time anything drains. Only the
  payload distinguishes it. `wait_for_pointer` now takes the position it is
  waiting for and accepts a report only within `POINTER_SLACK` (3 px) of it.
- **A rule recorded next to one call site does not propagate to the others.**
  This one was learned on Win32, written into the AppKit suite, and applied to
  the nudge check twenty lines below the function that needed it most. When a
  finding is about a _class_ of check, the next step is grepping for the rest of
  that class, not documenting the instance.

**Unfalsified, stated as the gap it is:** the run after the fix shows the filter
_accepting_ the right report —
`warped to PhysicalPoint { x: 480.0, y: 240.0 } and landed at PhysicalPoint { x: 480.0, y: 240.0 }`,
CI run 30897245466, all nineteen jobs green. It does not show the filter
_rejecting_ a wrong one. The failure path is the same `DEADLINE` assert that has
been seen red in an earlier run, so the mechanism works; what nobody has watched
go red is this predicate against a genuinely wrong conversion. That would take a
deliberate mutation round on a macOS runner.

### A synthesized mouse event carries no delta unless you put one on it

The run after the warp fix got past the pointer position and failed one
assertion deeper, with `raw_delta` of `(0.0, 0.0)`. The backend was right and
the harness was wrong: **`CGEventCreateMouseEvent` places an event at an
absolute location and computes nothing else.** `kCGMouseEventDeltaX` and
`kCGMouseEventDeltaY` stay zero unless the poster sets them with
`CGEventSetIntegerValueField`, and `-[NSEvent deltaX]` reads exactly those
fields — which `appkit::view` passes straight into `raw_delta`. It reported the
zero it was handed, faithfully.

**The assertion was checking a value the harness had never supplied**, which is
the more useful half of this finding: it asserted only that the delta was
positive in both axes, so it could never have failed for the right reason and
could only have _passed_ by accident, off some unrelated movement on the
runner's desk. That is "a check that cannot fail is not a check" arriving
through an injected-input harness rather than through a stub.

`quartz::move_mouse_by` now sets both fields, and the delta is the same constant
as the travel (`NUDGE`) so the two cannot drift. Every part of the pair earns
its place: the magnitudes differ so a `deltaX`/`deltaY` swap shows up, the signs
differ so a **reflection** fails distinctly from a swap — reflecting Y makes the
second component positive, swapping makes the first negative — and a negative
component is the only way to see one survive the trip rather than being clamped
or `abs`ed. That makes the Y-up-position-against-Y-down-delta asymmetry that
`appkit::pointer` exists to describe observable for the first time; M2 listed it
as reasoned rather than observed and it has been ever since.

The delta is judged by **sign and proportion, not exact equality**, and that is
the general form rather than a loosening: `-[NSEvent deltaX]` is documented in
device-independent points while the field being set is in the event stream's own
units, and nothing reachable from a Linux machine says whether a Retina host
scales between them. A uniform factor is a fact about the display and passes;
one axis scaled differently from the other is a defect and does not.

**The two neighbouring posters were audited for the same defect and are clean**,
which is worth recording so a round trip is not spent finding out:

- `CGEventCreateScrollWheelEvent` takes its unit as an **argument**, not a field
  left unset, and the unit is the whole of what decides
  `hasPreciseScrollingDeltas` and therefore `Lines` against `Pixels`. Its
  _amount_ was wrong for a different reason — see the next entry.
- `CGEventCreateKeyboardEvent` leaves `kCGKeyboardEventAutorepeat` at zero and
  `-[NSEvent isARepeat]` reads it — but the session asserts the first press is
  **not** a repeat, so there the default _is_ the value under test rather than
  an oversight.
- `kCGMouseEventClickState` is zero on a synthesized click and this seam never
  reads it (buttons come from `buttonNumber`), so nothing asserted depends on
  it. It is set to one anyway: a press with a click state of zero is not what a
  real click looks like, and AppKit acting on that would be a harness defect
  reported as a backend one.

### Instrumentation that cannot report is the same defect as a check that cannot fail

**A `log::warn!` with no logger behind it is a discarded string**, and a whole
CI round trip was spent on one. The readback added to
`appkit::window::set_frame` — whose entire purpose was to say whether the window
landed where it was put — was structurally incapable of speaking, because
nothing in `tests/appkit_session.rs`'s process had ever called
`crcbl_core::log::try_init_logging`. It shipped, the run failed, and the
diagnostic it was written to produce was not in the log because it could never
have been.

This is the "a check that cannot fail is not a check" rule one level up, and it
is worth stating separately because the reflex that catches the first one does
not catch this: an assertion is obviously something you can try to break, while
a log line looks like it either prints or does not and the failure mode is
invisible. **Before trusting a diagnostic, make it emit once.**

The session now installs a logger before it opens the backend, with the filter
fixed in code rather than read from `CRCBL_LOG` — `Filter::from_env` would leave
the session's own diagnosis depending on whether a CI job happened to set an
environment variable, and the job does not. `crcbl_shell::appkit::window` is
turned up to `debug` and everything else stays at `info`, so the placement trail
is complete and the event pump does not bury it. `try_init_logging` rather than
`init_logging`, because the `Result` says whether a logger was already there and
silently ignoring that is how this went wrong the first time.

**Falsified offline rather than reasoned about**, in a throwaway crate against
the real `crcbl-core`: `Filter::parse("info,crcbl_shell::appkit::window=debug")`
answers `Debug` for `crcbl_shell::appkit::window`, `Info` for
`crcbl_shell::appkit::shell`, and `max_level` `Debug` so the facade's global
fast path lets the records through; a `debug!` and a `warn!` on the window
target both reach **stderr**, and a `debug!` on another module's target is
correctly dropped. Stderr is the stream nextest has been surfacing all along —
every panic message read this session arrived on it.

### Changing presentation options moves every window to its creation frame

**This is a fact about AppKit, not about this codebase, and it cost eight CI
round trips.** It is written up here and in `appkit::window`'s module docs
because it is documented nowhere Apple publishes and because the next person to
reorder those statements will otherwise undo it.

**`-[NSApplication setPresentationOptions:]` returns every window of the
application to its creation frame.** Not the window it is called about — there
is no such window, the property is on `NSApplication` — and not "constrains it
to the screen". It puts windows back where they were created: origin on the way
into borderless, origin _and size_ on the way out.

Measured on a real runner by bracketing `apply_mode` statement by statement:

```text
after the Borderless arm                 [0,0,1024,768]
after size_layer                         [0,0,1024,768]
after refresh_presentation (options 0x5) [192,160,1024,768]   <- moved

after the Windowed arm                   [192,256,512,416]
after size_layer                         [192,256,512,416]
after refresh_presentation (options 0x0) [192,160,640,512]    <- moved and resized
```

`[192,160,640,512]` is exactly the frame the window was created at, and
`centred([0, 63, 1024, 674], 640x480)` — the creation-time visible frame — is
`(192, 160)` on both axes. **One mechanism, both directions.**

**The fix is an ordering rule: mask, then options, then frame.**
`refresh_presentation` now runs between the style mask and the frame rather than
in a tail after both, so the frame is the last geometry `apply_mode` sets. (The
middle position rather than the first is its own finding, immediately below.) It
is correct by construction rather than by repair, and two properties make the
reordering free: the borderless frame is computed from `-[NSScreen frame]`
rather than `visibleFrame`, so it does not depend on what the options do; and
the effective mode, which `refresh_presentation` decides from, is now settled up
front by `borderless_target` returning the whole `Screen` instead of only its
rectangle. That removes the read-back-the-screen-afterwards step which was the
only reason the effective mode had to be assigned late in the first place.

The alternative — re-asserting the frame after `refresh_presentation` — was
considered and declined. It leaves a window that visibly moves and then moves
back (and on the windowed leg, resizes and resizes back), it needs a comment
explaining why a second call exists, and it is fragile: anything later added
after the re-assert reintroduces the bug. The ordering rule needs no second
call.

**The `apply_mode:` bracket readings stay.** They are what made this findable at
all, and they are what makes a regression obvious: anything added below the arm
that repositions the window shows up immediately as a frame that changed after
the arm had set it.

#### The options will not take before the style mask — a third position, paid for separately

The obvious repair for the above was to hoist `refresh_presentation` to the very
front of `apply_mode`. That put `setPresentationOptions:` before the style mask
had changed, and **AppKit raised**. The process aborted with `SIGTRAP` — an
Objective-C exception unwinding through a Rust frame — with the whole
injected-input suite already green behind it and nothing logged after the
statement before it.

**The value was not the problem, and that is provable rather than assumed.**
M1's docs warn that `AutoHideMenuBar` without a Dock bit is an illegal
combination AppKit raises on, which made it the natural suspect.
`geometry::presentation_options` is a `const fn` whose only input is a
two-variant enum and whose only outputs are `PRESENTATION_DEFAULT` (`0x0`) and
`PRESENTATION_BORDERLESS` (`AutoHideDock | AutoHideMenuBar`, `0x5`). It cannot
construct the illegal combination at any call site at any time, so _when_ it is
called cannot change the value's legality. What changed was the window's state
when the options were applied.

So the rule has **three** positions: `setStyleMask:`, then
`refresh_presentation`, then `setFrame:`. The frame is last because both of the
others move the window; the mask is first because the options are not accepted
while the window is still in the style it is leaving. `apply_mode` now hoists
the target mask and target frame out of what were two `match` arms so the
sequence reads as one ordered block — buried in a `match`, the rule was
expressible twice and enforceable neither time.

#### An Objective-C exception reaching Rust is undefined behaviour, and this backend has now done it

`appkit::mod`'s docs already state the hazard; this is the first time it has
actually happened, and it happened in CI. Worth recording plainly:

- **Nothing guards it.** There is no `@try`/`@catch` anywhere in this backend
  and no `extern "C-unwind"` boundary — grepped, not assumed. Every
  `objc_msgSend` through `appkit::ffi` is a potential abort if the receiver
  raises. That is a deliberate position rather than an oversight: catching
  Objective-C exceptions from Rust needs a C shim or `objc_exception_try_enter`,
  and an `NSException` that has already been thrown has left AppKit's internal
  state undefined anyway, so surviving one buys very little.
- **The failure is at least loud.** `SIGTRAP` from an aborted unwind is
  unmistakable in a CI log, and it takes the process down rather than continuing
  with corrupted state. What it is not is _diagnosable_ — nothing after the
  raising statement runs, so any diagnostic printed after the call is a
  diagnostic that never prints. `refresh_presentation` now logs the value it is
  about to send **before** sending it, for exactly that reason.
- **`refresh_presentation` is the one path known to be able to raise**, and it
  is called from `apply_mode` and from window destruction. The known trigger is
  applying borderless options before the window carries a borderless style mask.

#### What the eight rounds actually eliminated

Written down so none of them is tried again. Each was killed by evidence, not by
argument:

- **`constrainFrameRect:toScreen:`.** The override is installed and the host
  test proving it passes on the runner; the window still moved. Its default
  moves a window _down_ to clear the menu bar and would never move one right by
  192 — which was reason to doubt it before the run, and the lesson is to weigh
  the _shape_ of a symptom against the shape a mechanism produces.
- **A corrupted `NSRect` argument.** `setFrame:` logged `asked` and `landed`
  identical, so the HFA-in-`v0`-`v3` theory — the same class as the `wheel1`
  variadic that did bite — was ruled out for that call.
- **The event pump and any delegate callback.** The frame reads wrong on the
  line printed _before the session's first pump_.
- **macOS state restoration.** `setRestorable:NO` was added, the next run
  confirmed `isRestorable false`, and the origin was still wrong. The change
  stays on its own merits, argued in `window.rs`. `frameAutosaveName` was
  `Some("")` throughout, so autosave was never in it.

#### What instrumentation was kept, and what was scaffolding

The hunt left a lot of logging. Kept is whatever would make a _regression_
diagnosable in one round rather than nine; dropped is whatever only answered a
question that is now answered.

**Kept:**

- **`set_frame`'s from / asked / landed line, and its warning on mismatch.** The
  one place a frame can be silently rewritten, and the warning is the only thing
  that would ever say so — `WindowState` carries no position, so nothing above
  this layer can notice.
- **`apply_mode`'s four readings**: the placement capture (the input the restore
  assertion is checked against, so a failure explains itself), after
  `setStyleMask:` (which changes the mask, the frame _and_ the responder — each
  of the three was a defect once), after `refresh_presentation` (the mover), and
  one at the exit carrying the frame and the responder. Each step prints what it
  changed, so a disagreement at the exit with agreement above localises a
  regression to the tail immediately.
- **`refresh_presentation`'s pre-send line.** That path can raise and abort the
  process, and a diagnostic printed after a call that can abort is a diagnostic
  that never runs on the one occasion it is wanted.
- **`install_logger` in the session.** Without it none of the above exists at
  all; that lesson has its own section.

**Dropped:**

- **The `set_mode:` bracket in `shell.rs`.** It existed to prove the move
  happened inside `apply_mode` rather than after it. That is settled, and the
  line now duplicates `apply_mode`'s own exit reading one frame later.
- **The session's per-pump-turn frame and responder tracking in `flip`.** Built
  to answer "which turn does it change on", which turned out to be "none — it is
  wrong before the first pump". `flip` is back to `wait_for`, and the assertions
  on the snapshots either side of each leg cover what it was watching for.
- **The screens dump in the borderless path**
  (`computed <rect> from screens [...]`). It existed to test "is
  `borderless_frame` wrong", which it was not. `set_frame`'s `asked` still names
  the rectangle that was computed.
- **The `before setStyleMask:` baseline reading.** The capture line above it
  already names the frame, and the after-mask line names the delta.

#### The two rules that got there

- **The readback layer earned itself.** `WindowState` carries an extent and no
  position, so `set_mode` reported a perfectly correct `PhysicalSize` throughout
  and every run before this printed it happily. Only asking `NSWindow` for its
  own `frame` showed the origin at all. **This is the first defect that layer
  caught and it is the kind only it can catch** — and the restore leg's
  corruption was invisible even to it for eight rounds, because the borderless
  assertion fired first. Gathering both flips before asserting either is what
  exposed it.
- **Instrumentation that cannot report is the same defect as a check that cannot
  fail.** The `set_frame` readback shipped, a round trip was spent, and it
  produced nothing because no logger was installed in the session's process — a
  `log::warn!` with no logger behind it is a discarded string. That rule has its
  own section below.

### `setStyleMask:` takes the first responder, and every mode change had to give it back

**Fixed.** `sendEvent:` delivers key events to the first responder, which is the
_window_ until something claims it — so a window that is its own first responder
swallows every keystroke and hands the view none, with nothing anywhere
reporting it. `create_native_window` claimed it once with `makeFirstResponder:`,
and `setStyleMask:` took it straight back.

Observed rather than supposed, one statement apart, on both legs:

```text
borderless: first responder is the content view — before setStyleMask: true
borderless: style mask asked 0x0, ... first responder is the content view false
```

and the session's own trail across a round trip read
`CrcblView -> NSKVONotifying_CrcblWindow -> NSKVONotifying_CrcblWindow`. **A
game that pressed F11 went permanently deaf**, silently, which is precisely the
failure `makeFirstResponder:` is on `appkit::view`'s list of five switches to
prevent — and the second of those five to turn out to be genuinely broken rather
than merely unverified.

`focus_content_view` now does it, shared between creation and `apply_mode`
rather than copied, and it warns when AppKit refuses. Two decisions worth
keeping:

- **It goes last in the ordered sequence**, after the frame and after
  `makeKeyAndOrderFront:`, for the same reason the frame is late: it is a state
  that has to survive, and putting it after everything means nothing in the
  sequence can take it away again.
- **It is exempt from the "reads of window geometry go above the sequence"
  rule.** It reads the content view and the responder chain, and neither the
  mask, the options nor the frame invalidates those — so the rule that forced
  the placement capture upwards does not reach it. Worth stating because the two
  rules sit in the same function and look alike.

**Asserted after the borderless leg as well as after the round trip**, and the
first of those is the one with teeth: a game goes borderless and _stays_ there,
so a responder restored only on the way back out would be a game that is deaf
for exactly as long as anyone is playing it — and an end-to-end check would have
passed that happily.

### `wheel1` is a named parameter, and declaring it variadic scrolled by zero

The run after the delta fix reached the scroll and got
`a notch of one line is not zero lines`. The real C signature is:

```c
CGEventRef CGEventCreateScrollWheelEvent(CGEventSourceRef source,
                                         CGScrollEventUnit units,
                                         CGWheelCount wheelCount,
                                         int32_t wheel1, ...);
```

**`wheel1` is named; only `wheel2` and `wheel3` are variadic.** The harness
declared the `...` one parameter early, so on `aarch64-apple-darwin` — where
Apple's ABI puts variadic arguments on the **stack** while named ones go in
registers — the amount was written to the stack and the callee read `w3`. The
event carried whatever was in that register, which was zero.

Three things about this are worth keeping:

- **It is the exact failure mode this crate warns about at length, and it did
  not arrive where anyone was watching.** `appkit::ffi` devotes a section to it
  for `objc_msgSend` and M1 built the whole `msg_send` transmute generic around
  it, so every Objective-C dispatch in the backend writes its signature down and
  is checked. This came through a plain C function in the test harness, whose
  declaration was hand-written once and never re-read against the header. **A
  wrong signature compiles cleanly, links, runs, and corrupts an argument at run
  time** — the compiler cannot help, and neither can any amount of care applied
  to a different call.
- **The old declaration made the argument unchecked, which is why it survived.**
  Falsified both ways offline: with `...` starting at `wheel1`, passing an `i64`
  compiles silently, because a variadic argument is subject to default promotion
  and is type-checked against nothing. With `wheel1: i32` named, the same call
  is a hard `E0308`. The fix converts an unchecked argument into a checked one,
  which is a stronger statement than "the numbers now line up".
- **An empty variadic list still has to be declared.** One axis means `wheel1`
  and nothing after it, and `fn(..., wheel1: i32, ...)` called with four
  arguments is correct; dropping the `...` entirely would be a different
  signature again.

**Every other `extern` declaration in the harness was re-read against its header
after this, and `CGEventCreateScrollWheelEvent` is the only variadic one.**
`CGEventCreate`, `CGEventGetLocation`, `CGEventCreateKeyboardEvent`,
`CGEventCreateMouseEvent`, `CGEventPost`, `CGEventSetIntegerValueField`,
`CFRelease`, `objc_getClass`, `sel_registerName`, `objc_autoreleasePoolPush` and
`objc_autoreleasePoolPop` are all fixed-arity and match. `objc_msgSend` is
declared with no parameters and **never called through that declaration** — it
is transmuted per call site, which is the same discipline `appkit::ffi` enforces
and the reason the Objective-C side of the harness did not have this bug.

Decisions and limits worth keeping whatever the run says:

- **The injection is posted from the session process, not from a helper
  binary.** The Win32 suite needs a second process because `SendInput` from the
  thread that owns the window never touches the message queue. That argument
  does not transfer: `CGEventPost` hands the event to the **window server**,
  which decides who is frontmost and delivers it through the ordinary run loop,
  so it re-enters from outside whoever posted it. A second process would also
  make the TCC question unambiguous in the wrong direction — synthetic events
  aimed at _another_ application are gated for certain.
- **`CGEventCreateScrollWheelEvent` is _partly_ variadic**, and this bullet used
  to say its per-axis amounts were all variadic arguments. That was wrong about
  the first axis and is what produced a scroll of zero; the corrected account,
  with the ABI reasoning and what was falsified, is in the `wheel1` section
  above. Left here as a pointer rather than deleted, because the wrong version
  is the kind of thing that gets re-derived from a half-memory of the C header.
- **The scroll's _sign_ is not asserted, only its unit.** "Natural scrolling" is
  a per-user system preference that inverts it, so an assertion on the sign is
  an assertion about the runner's settings. What is pinned is `Lines` rather
  than `Pixels`, which is what `hasPreciseScrollingDeltas` decides. The
  horizontal sign stays unverified for the same reason it always did.
- **The cross-process pasteboard check is `pbcopy` and `pbpaste`, so it covers
  text only.** A helper binary of ours was considered and declined: macOS ships
  two stock clipboard clients written by Apple, and a peer of our own would be a
  second hand-written Objective-C FFI whose only advantage is that we maintain
  it — and, since this target has no `required-features`, it would be built on
  Linux, Windows and `wasm32` by every `--all-features` job to do nothing there.
  What that costs is that `application/x-crcbl+ron` is **not** round-tripped
  through a second process; `pbpaste` cannot be asked for it. The RON half stays
  covered in-process. If an engine-to-engine paste ever misbehaves on macOS,
  this is the gap it would hide in.
- **M4 extends `tests/appkit_session.rs` rather than adding a second
  harness-less target.** `nextest` runs binaries in parallel, and two processes
  each bootstrapping an `NSApplication` and taking the key window would fight
  over which is frontmost — the loser reporting that injected input never
  arrived. Serialising them means a test group in `.config/nextest.toml`. The
  cost is that the whole session is one `nextest` test, so any failure reports
  all of it as failed; that is paid for by every step printing what it reached,
  and by the CI step below.
- **CI names the target rather than relying on the sweep.**
  `build + test (macos-latest)` gained a step running
  `cargo nextest run -p crcbl-shell --test appkit_session --no-tests fail --success-output immediate`.
  `--no-tests fail` is the macOS shape of the count gate `run-win32-e2e.ps1`
  performs by parsing the summary: this target is the _only_ executable coverage
  the AppKit backend has, is `harness = false`, and is behind no feature — so
  nothing would go red if it stopped being built. `--success-output immediate`
  prints the session's own narrative on a green run, which is the run where a
  reader most wants to know which optional path actually happened, and nobody on
  this team has a Mac to ask.

**The whole session now passes on a runner, end to end** — every injected event
(key, text, arrow, pointer position, raw delta, click, scroll), both pasteboard
directions including the cross-process `pbcopy`/`pbpaste` check, the resize, and
the mode round trip: borderless covering the screen exactly, and the restore
landing on the pre-flip frame to the point. What follows is what the session
still does **not** reach.

Still uncovered after M5:

- **`Borderless { monitor: Some(..) }` lands on the named screen's origin by
  construction and not by observation.** The runner has one display, so a
  backend that ignored the named monitor entirely would pass every assertion in
  the suite. Needs a two-display machine; see the borderless-origin entry above.
- **A window created borderless is untested**, and now carries a known ordering
  hazard. The session creates its window windowed and flips, so
  `create_native_window`'s borderless arm — which places the window with
  `initWithContentRect:` rather than `setFrame:display:` — has never run. It
  shares the `constrainFrameRect:toScreen:` override, but **the presentation
  options are applied by `refresh_presentation` on the first `set_mode`, not at
  creation**, so a window born borderless has its frame set before any options
  change has happened to it. Whether that matters depends on when the first
  options change lands relative to it, which nothing has measured. The ordering
  rule in `appkit::window`'s module docs is the thing to re-read before adding a
  test here.
- **The first-responder fix is unrun.** `focus_content_view` after every
  `setStyleMask:`, and the two assertions that guard it, land in the same commit
  as this entry. Everything else in the session has now been seen green.

- **`injection_skipped` is written and unrun**, because the runner granted
  activation. It stays for the case that produced it, which a developer running
  this as a background process on their own machine will meet. If it is ever
  taken it prints the `Activation` evidence and branches on `can_become_key` to
  say whose defect it is, rather than reporting a bare timeout.
- **The sample-level F11 pass.** Needs a renderer; macOS has no Vulkan until
  MoltenVK clears its P14 gate (`docs/plan/ROADMAP.md`, 2026-08-04). Not
  approximated.
- **A real drag and drop.** Unchanged from M3: a drag needs a _source_
  application with a mouse held down over a Finder item, which `CGEventPost`
  alone does not provide. What M4 adds is that the registration is now read back
  off the real window rather than off a throwaway view.
- **`AXIsProcessTrusted()` is not called**, although it would say outright
  whether TCC is the reason for a failed injection. It lives in
  `ApplicationServices`, which this crate does not link, and adding a framework
  to the macOS build to improve one failure message is a link error's worth of
  risk on a platform nobody here can build for. If the first run's diagnosis is
  ambiguous, that is the next instrument to add.

### What M2 could not verify, and what would verify it

Input is the half of this backend with the least executable coverage, because
the thing it needs is **injected input at the session level** — the macOS
counterpart of `SendInput`, which is `CGEventCreateKeyboardEvent` plus
`CGEventPost`. **M4 wrote that, and the first macOS run never reached it** — the
application does not become active on a GitHub runner, so nothing may be posted.
Everything below is therefore still a structural claim, and the M4/M5 section
further down says what would turn each one into an observation, what recovered
without activation, and what did not.

- **The switches that make input exist at all**, written out in `appkit::view`'s
  module docs: `setAcceptsMouseMovedEvents:`, `makeFirstResponder:`, the
  `NSTrackingArea`, and `interpretKeyEvents:` — plus M3's
  `registerForDraggedTypes:`, which is the one of them with a readback
  (`-[NSView registeredDraggedTypes]`) and is therefore asserted rather than
  only written down. These are this platform's shape of the `TranslateMessage`
  gap the Windows half paid a CI round trip for — **each of them is invisible to
  a test that calls the responder method itself**, because each governs whether
  the event is generated or routed rather than what the method does with it. The
  session pass exercises two of them indirectly: it warps the pointer into the
  window and requires a `PointerFocus` (the tracking area) or a `PointerMotion`
  (the accepts-moved flag plus first responder) to come back.

  **M4 closes three of the five by readback and attacks the other two by
  injection. After M5 the readback half runs on a runner that never activates;
  the injection half may still not run at all.** `session_support::window_facts`
  reads `acceptsMouseMovedEvents`, the first responder's class and the content
  view's `registeredDraggedTypes` off the **live** window — found by title
  rather than through `-[NSApp keyWindow]`, which is what makes it independent
  of activation — and that turns those three from "we called the setter" into
  "the window is in that state". The tracking area and `interpretKeyEvents:`
  have no readback, so they are attacked with `CGEventPost`: a posted
  `kVK_ANSI_A` has to come back as a `Key` **and** a `TextCommit` of `"a"`,
  which is only possible if `sendEvent:` routed the event to the first
  responder, `keyDown:` handed it to `interpretKeyEvents:`, and the view's
  `inputContext` was non-nil — the last of which is true only because
  `CrcblView` conforms to `NSTextInputClient`. **That has now happened on a
  runner.** The posted `kVK_ANSI_A` came back as a `Key` and the `TextCommit` of
  `"a"` came with it, so `interpretKeyEvents:` is reached end to end and macOS
  is no longer where Windows was before its e2e suite found `TranslateMessage`.
  Of the original five, only the `NSTrackingArea` is still a structural claim —
  the posted pointer motion goes to the key window through `mouseMoved:` rather
  than through a tracking crossing, so nothing yet _requires_ the tracking area
  to have been registered.

- **Every type encoding on `CrcblView`'s methods.** The runtime reads them only
  when it forwards a method through an `NSInvocation`, which nothing in this
  crate does and an input method might. A wrong one is a wrong-width read in a
  path CI never enters. Checking it needs a forwarded invocation, which is its
  own contrivance; the mitigation taken instead is that the encodings are
  written from one place (`ffi::ENC_RANGE`, `ENC_RECT`, `ENC_POINT`) rather than
  spelled out per method.
- **IME composition.** `TEXT_IME` is set on the structural standard the Wayland
  backend is held to — the view conforms to `NSTextInputClient` and every key
  event goes through `interpretKeyEvents:`, which is strictly more than
  Wayland's "bound `text-input-v3`". That a Japanese input method actually
  commits through it is unverified: a GitHub runner has no IME installed, and
  adding one is a runner-image change rather than a test.
- **The horizontal scroll sign.** Vertical is settled — a wheel turned away from
  the user is positive on macOS and in the seam — and horizontal is passed
  through on the same reasoning without a trackpad to confirm it. If a
  two-finger swipe right turns out to scroll left, the fix is one negation in
  `pointer::scroll` and the test beside it.
- **That `deltaY` is Y-down while `locationInWindow` is Y-up.** This is the
  asymmetry `appkit::pointer` exists to make visible, and it rests on the deltas
  being Quartz's `kCGMouseEventDelta*` — documented, and the convention GLFW and
  SDL both rely on, but not something this workspace has watched. A wrong answer
  is an inverted first-person camera, which is unmistakable the first time
  anybody plays.

### Considered and declined in M2

- **`PointerMode::Confined` is not implemented, and `POINTER_CONFINE` is
  clear.** macOS has no confine API: no `ClipCursor`, no
  `XGrabPointer(confine_to)`, no `zwp_confined_pointer_v1`. The only technique
  available is warping the cursor back after it has already crossed the
  boundary, which runs a frame late (so the pointer visibly leaves and snaps
  back), fights the user's own motion at the edge, and manufactures motion
  events a consumer cannot tell from real ones. Approximating it would set a
  capability bit with no mechanism behind it. `set_pointer_mode` refuses the
  mode by name. **Do not revisit this without a public API to point at.**
- **`RAW_POINTER_MOTION` is set although the deltas are accelerated.**
  `NSEvent`'s `deltaX`/`deltaY` satisfy the half of that bit that decides
  whether a camera works — separate from the absolute position, and unclamped by
  the screen edge — and not the half that says "unaccelerated": macOS applies
  pointer acceleration in the HID system before the event exists and publishes
  no way to ask for the pre-acceleration value. GLFW answers
  `glfwRawMouseMotionSupported() == false` on this platform for exactly that
  reason. The alternative was considered and declined: clearing the bit obliges
  `raw_delta: None`, and with `abs` also `None` under `Locked` that makes
  mouselook **impossible** on macOS rather than merely accelerated. Closing it
  properly means IOKit — `IOHIDManager`, or `IOHIDGetAccelerationWithKey` and a
  temporary acceleration change — which is a slice of its own and would be the
  first thing in this crate to reach past AppKit.
- **`DeviceId` is a constant per device _kind_, as on X11 and Win32.** An
  `NSEvent` does carry a `deviceID`, but it identifies a tablet and is
  documented as meaningful only for the tablet event family, so it is not the
  per-mouse identity `docs/plan/19-input.md` wants. The real answer on this
  platform is IOKit, which is the same slice as the acceleration entry above.
- **The candidate window is placed at the window's origin, not at a caret.**
  `firstRectForCharacterRange:` has to answer something and the seam does not
  model a caret — nothing above `crcbl-shell` says where text is being typed. So
  a Japanese candidate list appears at the bottom-left of the window rather than
  under the text. Closing it needs a seam addition ("the caret is here"), which
  is a decision above this crate and should be taken once for every backend that
  has an IME.
- **`appkit::keys::named_keysym` and `win32::keys::named_keysym` are the same
  table, and it was not extracted.** It is a pure function of the engine's own
  `KeyCode` with no platform in it, so it is duplicated _knowledge_ rather than
  duplicated shape, and the codebase's own rule says extract it. It was not,
  because M2's brief scoped its edits to `crates/crcbl-shell/src/appkit/**` and
  the extraction moves a `win32` file. What guards it meanwhile is a test in
  `appkit::keys` asserting the two agree for every `KeyCode`, which compiles
  wherever both modules do — every host, under `cfg(test)`. The extraction
  itself is a shared module (`crate::keysym`, or a third entry beside
  `linux::keymap`) plus a re-export from `win32::keys` so callers do not churn.

### What M3 could not verify, and what would verify it

M3 is `NSPasteboard` and file drops in. The clipboard half has real coverage —
see the two round trips listed above — so what is left is the drop half and two
edges.

- **No drop has ever been delivered.** A drag comes from another application's
  mouse, so nothing inside this process can produce one: what is verified is
  that `CrcblView` implements the four `NSDraggingDestination` methods, that
  `registerForDraggedTypes:` registers exactly `public.file-url`, and that a
  view which never registered accepts nothing. What is **not** verified is that
  `performDragOperation:` is reached, that `-[NSDraggingInfo draggingLocation]`
  is in the window space this backend converts it as, or that
  `-[NSPasteboard pasteboardItems]` on a real Finder drag yields one item per
  file with a `public.file-url` on it. All three are documented behaviour and
  none has been watched. Closing it is the same lever as M2's input gaps —
  session-level injected input, `CGEventPost` — plus a source application to
  drag from, which `CGEventPost` alone does not provide: a drag needs a real
  mouse-down-and-move over a Finder item. That is a harder problem than key
  injection and may be M4's honest answer of "not coverable in CI".
- **Only `public.file-url` is read.** `NSFilenamesPboardType` (deprecated in
  10.13) and `com.apple.pasteboard.promised-file-url` are not. The promised form
  is the interesting one: the source has not written the file yet and the
  receiver has to name a destination directory for it, which the seam has no way
  to ask for — `ShellEvent::DroppedFile` carries a path that already exists.
  Closing it is a seam question ("where should a promised drop land?"), not a
  backend one, and it should be answered once for every platform that has the
  concept.
- **macOS 15's pasteboard-access prompt has not been met.** Recent macOS asks
  the user before an application reads the pasteboard outside an explicit paste
  action. It gates _reads_, not writes, and it does not turn a read into an
  error — but if a future runner image shows it, the session's `clipboard()`
  would block rather than fail, which reads as a hang. Nothing has been
  observed; this is recorded so that a mysterious ten-second timeout in
  `paste()` is diagnosed rather than rediscovered. The runner's macOS version is
  in the job log.
- **`clipboard_offer` racing another process.** `declareTypes:owner:` claims the
  pasteboard and `setData:forType:` answers `NO` if ownership changed in
  between; the backend reports that as `ShellError::Backend` naming it. Reasoned
  from the documented return, not observed — producing it needs two processes
  writing at once.

### Considered and declined in M3

- **Lazy pasteboard provision (`pasteboard:provideDataForType:`) is not used,
  and it is structurally unavailable rather than merely unimplemented.** It
  would save a copy of every payload and it cannot work here for two reasons.
  The callback arrives on our **main run loop**, driven by the pasteboard server
  on behalf of a reader in another process — and between two `Shell::pump`s an
  engine is rendering, so there is no run-loop turn to service it in; every
  callback in this backend records and returns (see `appkit::events`), and an
  owner that deferred the answer to the next pump has already handed the reader
  nothing. And a lazy owner owes the pasteboard a flush before the process exits
  and must stay messageable until it does, so a shell dropped by a host
  application that keeps running leaves the server holding an unretained pointer
  to a freed object. This is the same refusal `win32::clipboard` makes about
  `WM_RENDERFORMAT`, arriving for the same reason on a second platform. **Do not
  revisit without a seam that gives the shell a run-loop turn it owns.**
- **The engine's own format is published under its mime string, not under a
  `dyn.*` UTI.** `UTTypeCreatePreferredIdentifierForTag` can synthesize a type
  identifier from a mime type, and the result is opaque, version-dependent, and
  reaches nothing that `application/x-crcbl+ron` does not — a pasteboard type is
  an arbitrary string, so the mime is a legal one, it is unique to this engine
  by construction, and it is byte-identical to what the other three backends
  name the same format with. Only text uses a system UTI
  (`public.utf8-plain-text`), because that is the one format other applications
  have to recognise.
- **Drag and drop _out_ is not implemented on any backend.**
  `docs/plan/15-windowing.md` scopes drag-and-drop to "file paths in
  (viewer/editor import)". Named here only because `NSDraggingSource` is the
  obvious next thing a reader of `appkit::view` would look for and its absence
  is a plan decision rather than a gap in this backend.
- **`clipboard_readable` is left at the trait's provided default.** Any process
  may read the general pasteboard at any time: macOS has neither Wayland's focus
  gate nor its serial requirement, so there is nothing to override. The trait
  method's own documentation already names this shape as the one the default is
  right for, alongside X11 and Win32.

### Deliberately not in M1

- **No menu bar.** An unbundled application with the Regular activation policy
  gets the system's default menu bar, which is enough for a window to be
  focusable and is not enough to ship — no application menu, so no ⌘Q. Building
  one is `NSMenu`/`NSMenuItem` and a decision about what belongs in it, which is
  above this crate.
- **`HW_UPSCALE` is clear although macOS has it.** A `CAMetalLayer`'s
  `drawableSize` is independent of its bounds and Core Animation scales the
  difference in hardware — exactly what Wayland's `wp_viewport` buys and what
  `docs/plan/15-windowing.md`'s borderless render-scale wants. **The seam has no
  way to ask for it**: nothing in `Shell` says "present a buffer smaller than
  the window". Setting the bit would be a claim with no mechanism behind it.
  Closing it is a seam change (a render-scale request on `Shell`, honoured by
  the Wayland backend through `wp_viewport` and here through `drawableSize`),
  which is a decision above this crate and should be taken once for both.
- **`app_id` has nowhere to go.** macOS's equivalent is `CFBundleIdentifier` in
  an `Info.plist`, which is a property of the bundle and cannot be set by a
  running process. `WindowDesc::app_id` is validated for a NUL byte so that a
  descriptor rejected on the other backends is rejected here, and is otherwise
  unused. Unlike the Win32 `AppUserModelID` entry this is not a deferral — there
  is nothing to defer to.
- **A live resize drag freezes the window**, on the same terms as the Win32
  modal loop and with the same unavailable fix. See that entry above; the two
  share one problem and one answer.

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
- **The extent after an `F11`, on X11.** The X11 toggle pass asserts the
  engine's own account of the mode — honoured under `openbox`, refused without —
  but not the summary line's extent, because the sandbox is killed rather than
  asked to close: sending `WM_DELETE_WINDOW` means finding another process's
  window, which needs `QueryTree` and a `WM_CLASS` walk the key sender does not
  have. `run_sandbox vk fullscreen` covers the swapchain-follows half on the
  same platform.

## Two display-mode defects, and the shape they shared

Both are fixed and in `CHANGELOG.md`; what is worth carrying is the **shape**,
because it is the one this area keeps producing and neither was found by
reading. Each was a value that stood in for an observation and was
indistinguishable from a real one:

1. `crcbl-shell`'s X11 backend derived the effective mode by reading
   `_NET_WM_STATE` back — a property the _client_ writes to request an initial
   state — so with no window manager to take ownership it read its own request
   and called it the answer.
2. `crcbl::engine::ModeRequest::mode` answered `DisplayMode::Windowed` when the
   window could not be read, and `Loop::finish` builds the summary _after_
   accepting a close request destroys the window. Every session a player ended
   the ordinary way reported windowed.

Both were found by an end-to-end pass that ran the whole thing and read the line
at the end, and both would have gone on passing every unit test. **The open
question this leaves**: `ModeRequest::mode` still returns a `DisplayMode` rather
than an `Option`, so a caller with a dead window still gets an invented
`Windowed` — `mode_at_exit` is the fix for the one caller that had the problem,
not for the type. Changing the signature would touch `Loop::display_mode` and
`ModeRequest::toggle`, both of which genuinely have a live window and would have
to unwrap something they know cannot fail.

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

- **`NineSliceSource` has no texels → units scale, and there are now two callers
  working around it.** `expand` takes its insets as target units directly, so a
  6-texel cap is 6 units tall whatever the caller's world is. Flappy scaled its
  whole sprite plane by `art::TEXELS_PER_UNIT` = 20 so its pipe's cap would
  survive a 12-unit playable band; breakout hit the identical wall on
  `assets/field.crpix`, whose 10-texel walls would otherwise be ten world units
  thick inside a court 28 across, and reached the same convention at a scale of
  10 — chosen independently, from the ball rather than from the pipe, which is
  the only part that did not copy.

  **This was the "if a second game hits this" condition, and it has been hit.**
  It was not fixed in that slice because the slice was scoped to `apps/*` and
  the change is in `crcbl-render`. The fix: a scale on `NineSliceSource` — a
  `texels_per_unit: f32` field set at `from_sheet` time, or an
  `expand_scaled(target, scale)` beside `expand` — so `minimum_size` and the
  fixed bands come back in the caller's units and a game whose world is not one
  unit per texel does not have to scale its camera to compensate. Both
  `art::TEXELS_PER_UNIT` constants and both `gpu::projection` multiplications
  come out when it lands; the sprite rectangles stay as they are, because those
  were never the problem. Nothing else in the workspace calls `expand` —
  `crcbl-render`'s own `button_skin` does, and would take the same scale of 1.

  **A third caller has now appeared, inside `crcbl-render` itself.**
  `crcbl_render::menu_camera` exists only because of this: the menu is laid out
  in device pixels, and drawing it through a camera of one unit per pixel would
  make the window frame's four-texel corner four _pixels_ at every scale, so a
  menu drawn three times as large would keep a hairline border. The camera
  divides by the style's scale instead, exactly as the two samples scale theirs.
  It comes out with the other two when `expand` learns a scale.

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
- **The panel's own cost is measured at 960 × 720 and still not at 1080p.**
  Horde's scale runs took it: with ten thousand enemies on the field and the
  panel showing three sections, switching it on moves the `ui-composite` GPU
  pass from 0.004 ms to 0.005 ms and leaves the CPU frame time inside its own
  noise (0.107/0.109 ms off against 0.100/0.101 ms on, two runs each — the
  "with" runs came out _lower_, which is the noise floor rather than a saving).
  `07-ui-debug.md`'s criterion is **"<0.5 ms GPU at 1080p"** and that extent has
  not been run, so the criterion is not closed; it is two orders of magnitude
  the right side of it at three quarters the pixels. Conditions: release,
  `--backend vk` on radv (RX 7900 XTX), headless offscreen ring,
  `--wall-clock --tick-hz 1 --frames 900 --prefill 10000`, `PassTimers::latest`
  for the GPU number and the panel's own `FrameStats` mean for the CPU one.
- **The overlay starts hidden in a release wasm build.** The default is
  `cfg!(debug_assertions)`, which is sample rule 4's "on by default in dev
  builds" taken literally; the demos on `crcbl.kryptic.sh` are release builds,
  so a visitor has to press F3. Whether the published demos should default it on
  is a product decision nobody has made. `web.rs` builds `Options::default()`,
  so turning it on there is one field.

## Coverage gaps

- **No sample test pins the _values_ its RNG produces — only that two runs
  agree.** Found by falsifying `crcbl_core::rand::hash_u64`: dropping its final
  xor-shift left asteroids, flappy, horde and `crcbl-phys` **entirely green**,
  and only `crcbl-core`'s own tests went red. The determinism suites compare one
  run against another, and both runs move together when the hash changes, so a
  rewrite would silently redraw every course, board and horde in the project
  with nothing failing. `it_matches_the_reference_vectors` now pins the shared
  implementation against the published splitmix64 outputs, which covers all four
  games because they all call it — but each game's own mapping from a hash to a
  gap centre or a spawn ring is still unpinned, so a change to `gap_centre`'s
  arithmetic has the same free hand the hash used to. What would close it: one
  test per game asserting a couple of literal positions for a fixed seed.

- **The mixer adoption was not verified by ear, and two of its choices are
  audible-only.** See the entry under _Owed_ above. Structurally everything is
  pinned; nothing has been listened to.

- **`Mixer` is exercised single-threaded in every sample test.** The engine has
  `mixer_is_sync_and_fill_is_serialised`, which drives four threads through
  `fill` while voices loop, but nothing tests `play`/`stop`/`set_mix` racing
  against a live `fill` — which is exactly what happens in a real game, where
  the game thread calls all three while the audio callback runs. The `Mutex`
  makes it safe by construction and no test says so.

- **The `wasm32` audio path is not built by the local verification loop.**
  `AudioStream::open` on `wasm32` goes through `web::install`, and the blanket
  `impl AudioSource for Arc<T>` is what makes an `Arc<Mixer>` acceptable there
  too. The browser gate (`web/run-browser-e2e.sh --build`) covers the four demos
  end to end, which is the only place that path runs.

- **Nothing checks the demo site's HTML in CI.** The 2026-08-02 audit ran
  `npx html-validate` (recommended + document + a11y presets) and a stdlib
  Python parser over the three built pages, plus `curl` over every external link
  and a headless-Chromium screenshot at 1280 and at 390 wide. All of that was a
  human running commands; `web/build.sh` runs `check-exports.mjs` and
  `smoke.mjs` and nothing that reads the HTML it just wrote. What it would take,
  in the no-npm spirit of `web/`: fold the link-and-asset resolution check into
  `build-pages.py`, which already knows every page it wrote and every file the
  site will contain. The tag-balance half is what `html-validate` does better,
  and that one needs a dependency.
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
- **Only the sandbox asserts that its UI pass reaches the render graph.**
  `f3_toggles_the_debug_overlay_in_the_sandbox` reads the frame's graph dump and
  requires `ui-composite` to be present when the overlay is on and absent when
  it is off, which is what makes "the overlay was drawn" mean "the overlay was
  composited". Breakout and flappy have no equivalent — their tests stop at the
  draw list reaching `Gpu` — because neither `Gpu` keeps the dump. This predates
  the overlay: nothing ever asserted their HUD's pass either. Closing it is a
  `#[cfg(test)] last_dump: String` in each, the same three lines the sandbox now
  has.
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
- **The bird sprite is not checked against the bird collider.** It is drawn 0.8
  world units across against a `2 * BIRD_RADIUS` of 0.7, deliberately, and
  nothing asserts the relationship — so art that grew to twice the collider
  would look wrong and pass. Breakout closed the equivalent gap
  (`every_sprite_covers_the_collider_it_stands_for`, which holds all forty
  bricks, the paddle and the ball to their colliders exactly); flappy's is
  harder only because the bird is deliberately _not_ exact, so the assertion has
  to be a stated ratio rather than an equality.

- **Nothing checks breakout's clear colour against the court's interior.**
  `art::SURROUND` and `field.crpix`'s `f` are two hand-written sRGB values that
  have to read as different surfaces; a change to one is invisible to the tests.
  Low stakes — it is a look, not a behaviour — but it is the one number in
  breakout's art with nothing holding it.
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

- **A consumer cannot see the cost it is being asked to avoid.** `Bvh::depth` is
  public; `PhysicsWorld` exposes no `depth()`, no node count and no `&Bvh`, so a
  game has no way to measure whether its teleport rule helps. The rule above is
  therefore chosen on the argument and not on a measurement. Either expose a
  `PhysicsWorld::broadphase_stats()` or accept that the claim stays unverified.
  Ties into the missing benchmark below.

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

- **Segment CCD earns a named method, and still has none.** P6 said "decide when
  asteroids writes it". Asteroids wrote it (`sweep_bullets`), and the verdict is
  yes: `Segment { start: pos - vel * dt, end: pos }` then `sweep_sphere` is the
  same six lines in every game that fires anything, and getting `dt` or the
  order wrong is silent —
  `a_bullet_that_crosses_a_rock_within_one_tick_still_hits_it` goes red under
  exactly that mistake. Wanted: something like
  `PhysicsSystem::sweep_body(entity, dt, exclude)` that reads the body's own
  velocity and radius. Not blocking; three samples in, it is a pattern.

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
  `overlap_sphere_into` runs through all three layers now, so horde's
  `steer_enemies` hoists one `neighbours` buffer out of its loop and the pass
  allocates nothing once it has grown: the collider ids land in a scratch buffer
  on `PhysicsSystem`, and the BVH's descent stack and candidate list are
  `PhysicsWorld`'s own fields. It used to be **three** `Vec`s per enemy per tick
  — `overlap_sphere`'s own, `PhysicsWorld::overlap_sphere`'s, and
  `Bvh::traverse_aabb`'s — which at the plan's ten thousand is 1.8 million
  allocations a second, every one dropped immediately.

  **What is not known is whether it mattered.** No before/after number exists,
  for the reason under _What horde still owes_: ten thousand enemies kill the
  player in under a second, so a wall-clock run measures a simulation that has
  stopped, and this repository has no allocation counter and no benchmark
  harness. The change is justified by the count, not by a measurement, and
  anybody quoting it as a speed-up is quoting something nobody ran.

- **Steering is embarrassingly parallel and there is nothing to run it on.**
  Horde's separation pass reads positions, queries, and writes velocities —
  nothing it writes is read by the broadphase, so the pass is order-independent
  by construction and every enemy could be done on a different thread. There is
  no `crcbl-jobs` (P8) and no parallel ECS schedule, so it is a `for` loop. This
  is the entry the roadmap already predicted; it is repeated here with the
  evidence that the _shape_ is right, which the roadmap could not know — and
  with a number: **14.66 ms a tick at ten thousand spread, 84.09 ms converged**,
  against a 16.67 ms budget. Sixteen cores is the difference between the sample
  hitting its target and missing it by 5×, and there is no shared mutable state
  in the pass to stop them.

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

- **A sphere overlap of radius `R` returns everything within `R + r_b`, and
  horde depends on that exactly.** Not a complaint — it is what makes
  `separation_query_radius` `r_self + slack` rather than
  `r_self + max_enemy_radius() + slack`, which at a brute's 0.85 would nearly
  triple the area a grunt searches. It is written down because it is an
  undocumented consequence of `overlap_sphere` being shape-aware rather than an
  AABB test, and nothing in `crcbl-phys` says so.
  `the_separation_query_radius_is_exactly_the_neighbourhood` is the guard, in
  the consumer, where it does not belong.

## What horde still owes

S3 is done — the core loop, the art and progression, and now audio, the longest
run, the browser demo and the scale measurement. `docs/plan/sample/03-horde.md`
carries the numbers and their conditions; this is what was raised and not
finished. Entries the measurement closed have been deleted rather than
annotated.

- **Do not put the autostart back.** This sample shipped without a start screen
  on purpose — its board is empty at `t = 0`, so a waiting state is a blank
  arena with a prompt on it, sample rules 4 and 11 do not require one, and
  adding `WaitingToStart` churned the suite exactly as predicted. **The user
  played it and asked for the screen**, which reverses that call for good: a
  demo that starts taking hit points off a player who has not looked at the
  window yet is worse than a blank arena, and four samples that open the same
  way are worth more than one clever exception. `GameState::WaitingToStart`
  short-circuits `run_tick`; `restart` lands on the title screen rather than in
  play, so `TRY AGAIN` takes two presses, the same as asteroids and flappy. The
  argument is preserved in `GameState`'s own docs and in `crate::menu`'s header
  — it is recorded so it is not re-derived, not so it can be re-applied.

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
  steals the oldest cheap one. Nothing shows `dropped()` yet — it is on `Audio`
  and not on the debug panel.

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

- **The spawn ring is relative to the player and clamped into the arena**, so a
  player standing in a corner gets enemies materialising on the wall beside them
  instead of walking on from off screen. Rejecting and re-drawing the angle
  would fix it and would no longer be a pure function of the index, which is the
  property the determinism suite rests on; the honest fix is to pick the arc
  that is inside the arena rather than to retry.

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
  itself is fine.

  **It also caps what a headless run can reach**, which matters now that there
  is a drop worth watching for: `horde --headless --frames 600 --prefill 200`
  banks its first level at three seconds and parks, so no headless invocation
  reaches a potion however many frames it is given. Every measurement of the
  drop rate therefore comes from `game::tests`, which drives the level-up screen
  through the autopilot. A `--choose <n>` flag, or an autopilot behind a flag,
  would give the binary the same reach the tests have.

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

- **The mirrored wizard has never been rasterised, and the walk has never been
  watched.** `art::mirrored` swaps a frame's `u` ends;
  `art::tests::facing_left_reverses_the_frames_u_range` asserts the exact
  reversal, asserts every point the quad will sample stays inside the frame's
  own interval — the property that stops a mirrored actor sampling the grunt
  next to it in the strip — and reproduces `sprite.slang`'s
  `lerp(uv.x, uv.z, corner.x)` on the CPU. That last part is a **copy of the
  shader's rule, not the shader**. What was checked by reading the shader: `u`
  is an unconditional `lerp` with no clamp and no `saturate`, and the fragment's
  `sharpen` is written in terms of `fwidth`, which is symmetric — so a reversed
  range interpolates rather than degenerating. The evidence that would actually
  settle it is a golden in `crates/crcbl-vk/tests/vk_e2e.rs` that renders a
  frame and its mirror and compares the two images column-reversed; that file
  was outside the write scope of the slice that added the flip. Nobody has seen
  a picture of a wizard facing left, and nobody has seen the walk cycle play —
  the browser gate's canvas capture that a human looked at predates both.

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

- **No thrust flame still, and the audio slice was where it was going to land.**
  The previous entry here said "do it with the audio slice, where the thrust cue
  lands anyway", and the audio slice did not: `RenderState` still carries no
  thrust intent, so the ship draws one frame whatever it is doing and a player
  hears the engine without seeing it. It is two rows of `assets/ship.crpix`, a
  `bool` on `RenderState` set from `Intent::thrust`, and a frame index in
  `art::Scene::build`. The `bool` now exists on the simulation side —
  `GameLogic::thrusting`, mirrored onto `Game::thrusting` — so what is left is
  carrying it onto `RenderState` and picking the frame. The cue timer that used
  to be suggested as the flame's clock is gone; a flame that flickers wants its
  own, and the frame's alpha is the honest source.
- **No golden buffer for the cues.** The three sounds are synthesised
  deterministically — `audio::noise` runs splitmix64 from a fixed seed — so a
  golden buffer is _possible_, and there is not one. What the tests assert is
  that each cue fires, that it carries the position of the thing that raised it,
  and that the explosion decays and is not a tone. Nobody has listened to the
  result on a real device and no test can tell a good explosion from a bad one.
- **Positions are not interpolated, and the wrap is why.** Every angle asteroids
  draws is lerped across the frame's alpha; every _position_ is the last tick's,
  so a rock at 60 Hz on a 144 Hz display moves in sixtieths. The fix is not
  "lerp those too": the playfield wraps, so a body that crossed an edge has a
  previous position a whole field away and interpolating it would fly it back
  across the screen. What it needs is for `RockView` / `BulletView` / the ship
  to carry the previous position **and a flag saying it teleported**, set by
  `teleport_if_outside`, with the renderer snapping rather than lerping on that
  tick. That is a change to what the simulation publishes, not to how it is
  drawn, which is why it was not folded into the art slice.
- **No visual for a shot hitting a rock.** The explosion is audible and not
  visible: a rock disappears and two smaller ones appear, with nothing between.
  Particles are a hard non-goal in `docs/plan/sample/02-asteroids.md`, so
  whatever this is, it is not that — a two-frame flash sprite at the hit
  position is the cheapest thing that would read.
- **Rocks that straddle an edge are drawn once, not twice.** A large rock is 3.4
  units across on a 32-unit field, so for about a tenth of a second per crossing
  half of it is missing rather than appearing on the far side. Drawing a ghost
  copy at the wrapped offset is a handful of extra sprites and no simulation
  change; not done because the slice was scoped to the art and this is a
  rendering rule the other two samples have no precedent for.
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
  reads the canvas back at 26/26, so "the frame is not blank, not one flat
  colour and changes between frames" is checked — 89 distinct colours across a
  959×463 canvas on the SwiftShader adapter. What is still unchecked is whether
  it is the **right** picture, and in particular whether a rotated
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

- **Texels-per-unit: fix the engine, or let each sample pick a scale?** Taken:
  **let the convention stand through P4B, fix it before the third sample.**
  `NineSliceSource::expand` takes its insets as target units directly, so a
  6-texel cap is 6 units tall whatever the caller's world is; flappy scaled its
  sprite plane by 20 and breakout by 10, chosen independently from the pipe and
  from the ball. The reason for deferring was scope — the slice was `apps/*` and
  the fix is in `crcbl-render` — not doubt about the fix. _Changes it_: nothing;
  this one is already owed. The work item, with the shape the fix should take,
  is under **The sprite system** above. Recorded here so the deferral itself is
  on the record and not just its consequence.

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
published. **47 findings: 16 medium, 31 low, no critical or high.** All sixteen
medium findings were closed on 2026-08-04 — see the list below, which replaced
the per-finding entries; the Low section is what remains.

### Medium

Closed on 2026-08-04, one commit each (all pushed to `main`; `git log` is the
record, and each fix shipped with a test that failed on the old code):

- **1 — server rotated the dead session's token on a late reconnect**
  (`fix(net): rotate session on late reconnect; drop stale resume token`,
  68cc444): `handle_hello` now sets `session_terminated` when its own expiry
  check drops the session.
- **2 — client retried a stale resume token forever** (same commit): two
  consecutive `INVALID_SESSION_TOKEN` rejections drop the credential and fall
  back to a fresh join.
- **3 — deletion queue freed a destroyed object one submission early**
  (`fix(vk): keep destroyed objects alive for every referencing submission`,
  6b267f9): command buffers record the raw objects they use and a submission
  extends each parked one's retirement to its own completion.
- **4 — AppKit `set_mode(Borderless)` showed a hidden window**
  (`fix(shell): keep a hidden AppKit window hidden across set_mode`, 3d55423).
- **5 — Win32 minimize re-applied the pointer clip from a 0×0 client rect**
  (`fix(shell): release the pointer clip when a captured window minimizes`,
  32f90c8).
- **6 — RandR `OutputChangeNotify` never handled: FALSE POSITIVE, verified
  2026-08-04.** The xcb randr protocol defines exactly two events —
  `ScreenChangeNotify` (base+0) and `Notify` (base+1, whose `subCode` carries
  CrtcChange/OutputChange/etc.; `/usr/share/xcb/randr.xml` declares
  `<event name="ScreenChangeNotify" number="0">` and
  `<event name="Notify" number="1">` and nothing else) — so `handle_event`'s
  `base..=base+1` range already routes output changes to
  `handle_monitors_changed`. There is no base+2 event and nothing was unhandled.
- **7 — INCR mask replace stripped input from our own windows**
  (`fix(shell): never replace an own window's mask for INCR property changes`,
  9b90ba8).
- **8 — unbounded unclaimed wayland data offers**
  (`fix(shell): bound unclaimed wayland data offers`, 1c834b3): pending offers
  capped at 8 with oldest-first eviction, per-offer mimes capped at 32.
- **9 — reference frame's depth image entered the pass in `Undefined`**
  (`test(hal): transition the reference frame's depth image into the pass`,
  b53c603).
- **10 — wgpu push-constant range overflowed**
  (`fix(wgpu): wire MSAA resolve targets; saturate push-constant range end`,
  cd49486).
- **11 — wgpu dropped `ColorAttachment::resolve`** (same commit): resolve views
  resolved from the pool and wired into the pass; stale handles fail loudly.
- **12 — native audio detuned at non-48 kHz device rates**
  (`fix(audio): step voices at the internal rate; stop per-block allocation`,
  4c59171).
- **13 — per-block allocation on the audio thread** (same commit): the mono and
  multichannel scratch is owned by the stream callback and reused.
- **14 — hostile-IHDR PNG allocation bomb**
  (`fix(sprite): bound the PNG output buffer before allocating from IHDR`,
  0e6fcb7): declared pixels bounded against `1 << 28` before any allocation.
- **15 — `--tick-hz` above 1e9 panicked the engine after the GPU was open**
  (`fix(cli): refuse --tick-hz rates the frame clock cannot express`, 473702d):
  both parsers refuse past `MAX_TICK_RATE`.
- **16 — crpix frame names that are clip keywords silently corrupted the file**
  (`fix(cli): refuse crpix frame names that are clip keywords`, 0f104a7).

### Low

All thirty closed on 2026-08-04, one commit each (pushed to `main`; `git log` is
the record, and each fix shipped with its failing-first test or an honestly
stated gap):

- **17 — delta encode cap left no room for the seal overhead**
  (`fix(net): fit the sealed delta in the transport cap; retain baselines only on send`,
  62f738d).
- **18, 19 — readback wait-semaphore UB; query bounds** (`fix(vk)`, 8ec3170 —
  see the Medium list).
- **20 — stale borderless monitor after a display move** (`fix(shell)`,
  c4a8c70).
- **21 — stale `WS_VISIBLE` re-showed a hidden window** (`fix(shell)`, bb6ad48).
- **22, 23 — INCR read failure mistaken for the terminator; timestamp-probe
  stall** (`fix(shell)`, 5fd013b).
- **24 — refused and overwritten drag offers leaked** (`fix(shell)`, bc5bece).
- **25, 26, 27 — stale and contradictory seam docs** (`docs(hal)`, caae5d0).
- **28, 29, 30, 31 — wgpu stride/surface/write-buffer; ui counts**
  (`fix(wgpu,render)`, b4f22f5).
- **32, 33 — compressed upload sizing; negative `dt`** (`fix(render,input)`,
  bcd6af7).
- **34, 35, 36, 37 — phys sweep/overlap/mass edges; synth overflow**
  (`fix(phys,audio)`, 3fc07da).
- **38, 39, 40, 41, 42 — replay length; menu drag drawn-state; crpix overflow;
  quote-unaware XML empty-tag; menu key stuck in `held_keys`**
  (`fix(store,ui,sprite,xml,engine)`, f768b7d).
- **43, 44, 45, 46 — asteroids score and tick allocations; breakout start-menu
  corner; horde kill freeze** (`fix(asteroids,breakout,horde)`, d4f6974).

Coverage notes carried with the fixes: 20, 21, 22 and 23 have no runnable test
on the runners where they live (two-display drag, Windows-only, timing- or
hostile-compositor-dependent) and said so; 38 and 40 need multi-GiB fixtures to
hit the overflow path directly and tested the checked arithmetic instead; 41's
fix is a regression guard for a case the quote-balancing scan already mostly
handled, pinning the correct `empty` flag and the un-stripped attribute value.

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

`crates/crcbl-mtl` enumerates adapters and refuses everything else by name. Four
things were raised while building it and not settled.

- **Nothing instantiates it.** No app constructs `MetalInstance`, and it is not
  in the engine's backend selection or re-exported from the `crcbl` umbrella. CI
  builds and tests it on `macos-latest`; no shipping binary reaches it. The
  wire- up belongs with the slice that gives it a device to hand back — a
  registry entry for a backend whose `request_device` always refuses would be a
  path that exists only to fail.
- **It advertises Tier B, and a tier-aware caller will believe it.**
  `DeviceCaps::tier` is derived, and `DRAW_INDIRECT_COUNT` /
  `MULTI_DRAW_INDIRECT` stay off until the command slice picks Metal's indirect
  path (indirect command buffers, per `docs/plan/09-backends-metal-dx12.md`'s
  mapping table). Correct today and documented in the crate docs, but it is
  visible behaviour rather than an internal note: once anything selects on tier,
  macOS takes the Tier B branch until MTL6.
- **The engine has never stated a minimum macOS version.** `adapter.rs` sends
  `supportsBCTextureCompression` among others, which dates the floor to macOS
  11; `objc2` does not gate on availability, so an older system raises an
  unrecognised-selector exception rather than answering wrongly. Loud, but
  undecided — and the same question the AppKit shell backend has been carrying
  unstated since P5C.
- **`DeviceType::Virtual` is unreachable on Metal.** There is no virtualisation
  query, so a paravirtual GPU under Apple's Virtualization framework answers
  every question exactly as the built-in one and enumerates as `Integrated`.
  Only name-matching would separate them, which is not a capability query.
  Stated as a gap, not fixed.

### Not verified: every assertion in the crate

The seven tests in `crates/crcbl-mtl/src/instance.rs` have **never executed** —
this is a Linux tree with no Apple hardware.
`cargo clippy --target aarch64-apple-darwin` type-checks and lints the whole
backend (the Darwin std is installed and no linking is involved), and that gate
was shown able to fail, so the code is known to compile against the real
`objc2-metal` 0.3.2 API. What is unverified is every runtime value: that
`maxBufferLength` clears the seam's 128 MiB floor, that the sample-count probe
finds anything, that Apple Silicon reports argument-buffer Tier 2, and that
linking the Metal framework works at all. CI's `build + test (macos-latest)` leg
is the first execution.

## The weekly Miri job cannot finish, and the ALSA fix is what revealed it

Run 30966491592 (manual `workflow_dispatch`, 2026-08-05) installed the ALSA
headers successfully and then **hit `timeout-minutes: 60` inside "Interpret the
physics and audio libraries"** — cancelled at 60:16, not failed. So the ALSA
repair in `514b2ea` was correct and is confirmed; it just uncovered the next
problem, which the earlier `pkg_config` failure had been masking since the
dependency landed.

`cron.yml`'s own comment claims the two steps take "about seven minutes of
interpretation", and that number is now known wrong: step 1 (`crcbl-core`,
`crcbl-hal`, `crcbl-ecs`, `crcbl-store`, `crcbl-ui`, `crcbl-jobs`) measured ~200
s locally, and step 2 alone exceeds the remaining budget. The 2026-07-27 run
passed, so something between then and now made `crcbl-phys` or `crcbl-audio`
much slower under the interpreter — which of the two is unmeasured.

Not yet chosen: split step 2 into per-crate steps to find the offender, raise
the timeout, narrow the target list the way `crcbl-net` already was (with the
reason recorded), or cut `PROPTEST_CASES` further for those two crates. Measure
first — the current comment is evidence that guessing at these numbers is how
the file got here.
