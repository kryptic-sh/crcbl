# Changelog

All notable changes to this workspace are recorded here, in
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) form. Versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html); below 1.0 a breaking
change bumps the minor.

Started partway through the project, so it covers changes from this point on
rather than the whole history — `git log` is the record before it. There are no
tags yet, so everything so far is unreleased.

Internal churn a release note would not mention — refactors with no outward
effect, test-only and docs-only changes, CI repairs — is deliberately left out.

## [Unreleased]

### Added

- **crcbl-phys**: `PhysicsSystem::body_mut(entity) -> Option<&mut RigidBody>`,
  for a game that chooses a velocity rather than having one integrated onto it.
  `set_body` was the only writer and it costs two hash operations — an insert
  into the body map and a touch of the transform map — to change one `DVec3`,
  which a crowd pays once per agent per tick; `apply_force` is not an
  alternative, because a kinematic body's zero inverse mass makes a force a
  no-op. It cannot move a collider: position lives in the transform, and
  `set_transform` is still what tells the broadphase.

- **crcbl-render** (`sprite_pass`): `batch_count(&[Sprite]) -> usize` answers
  how many draw calls a sprite list will cost, without a device. The batching
  rule — a run of consecutive sprites naming one sheet is one draw, so `A A B A`
  is three and not two — was previously readable only by writing it out again,
  which `apps/horde` did to put the number on its debug panel. It delegates to
  the batcher the pass itself uses, so it cannot drift from it.

- **crcbl**: the simulation half of the engine is re-exported, so a game names
  `crcbl` and the standard library and nothing else. `crcbl::ecs`,
  `crcbl::phys`, `crcbl::net`, `crcbl::server`, `crcbl::client`, `crcbl::input`,
  `crcbl::audio`, `crcbl::store` and `crcbl::sprite` join the graphics stack
  that was already there, and `crcbl::log` re-exports the logging facade — its
  macros resolve through `$crate`, so `crcbl::log::info!` expands exactly as
  `log::info!` does and no wrapper macro exists.

  The umbrella's headline claim has been "one dependency for a game" since it
  was written, and until now only `apps/sandbox` could keep it: the other four
  samples each named eleven workspace paths beside it. None of the nine crates
  depends on `crcbl`, so this is nine `pub use` lines rather than a
  restructuring — the arrows already pointed this way and nobody had drawn them.

  `crcbl::sprite` is the reader (`load`), never the encoder. A build script that
  bakes art still names `crcbl-sprite` itself with its `bake` feature, which is
  the one dependency a sample continues to spell out, and is what keeps a PNG
  encoder out of a shipped binary.

- **crcbl** (`crcbl::engine`): `Pending` folds the whole of a pump batch that
  belongs to the loop rather than the game — the pointer, focus loss, and the
  three reserved keys `DEBUG_OVERLAY_KEY` (F3), `PAUSE_KEY` (Escape) and
  `FULLSCREEN_KEY` (F11), which are now the engine's constants. `observe`
  returns `Handled::Loop` or `Handled::Game`, so a sample's pump closure is a
  guard and its own key handling; `Pending::carrying` starts a batch from where
  the last frame left the cursor.

  The pointer half was **byte-identical in all four samples**, and it is not
  trivial code: it carries the last position across frames because motion and
  buttons arrive as separate events and a click carries a position only on some
  backends. The reserved keys were three constants spelled out five times, and
  they are the engine's because the thing F3 opens is the engine's.

  196 code lines out of the four `app.rs` files. What is left there is the loop
  — the fixed-step accumulator, teardown, the summary — which is still four
  copies.

- **crcbl** (`crcbl::args`): the flags every sample has. `Common` holds
  `--headless`, `--frames`, `--tick-hz`, `--backend` and the debug-overlay pair,
  with `frame_budget` and `debug_overlay_visible` on it; `Common::consume`
  offers one argument to that set and answers `Yes`, `Help`, `Bad(message)` or
  `No`. `Invocation<T>` wraps a game's own options, `COMMON_OPTIONS_HELP` and
  `COMMON_TAIL_HELP` are the shared `--help` blocks, and `positive`/`number`
  parse a flag's value with the rejection wording the samples already used.

  **Offered, not imposed.** A game keeps its parse loop and its `Options`
  struct, and claims what `consume` hands back — which is how `--seed`,
  `--max-enemies`, `--prefill` and `--wall-clock` stay per-game, and how
  `apps/sandbox` goes on taking `--camera` and `--title` while not being a
  consumer of this at all.

  The four game parsers were the same file: flappy's and asteroids' differed in
  **eight lines**, six of them usage prose. 894 code lines across the four
  became 599 against 270 in the engine, and the flags themselves are now tested
  once rather than four times. Each sample keeps one test that the engine's
  cannot make — that its parser actually _calls_ `consume`, since one that
  forgot would pass every test in `crcbl::args` and still reject `--headless`.

  The drift this closes was real: three of the four parsers had dropped
  breakout's assertion that the default backend stays `None`, which is what
  stranded CI on a machine with no driver. Each sample's `USAGE` now asserts it
  contains both shared help blocks byte for byte, so a reworded flag description
  reddens the build instead of shipping.

- **crcbl-store** (`crcbl::store::record`): `Record`, one `u32` kept between
  sessions. `Backing` picks where — `None` for a headless run that must leave no
  trace, `Backing::config(app)` for the platform's config directory, and
  `Backing::Browser` for a store the page's shim installed. `raise` writes only
  when the new value is larger; `set` is for the game whose better is smaller.

  The crate handed out a `StorageSource`, an atomic write and a
  platform-standard root and stopped there, so every sample that wanted a high
  score wrote the platform arms, the little-endian encode, the corrupt-file case
  and the headless rule itself. Four did, and the bodies matched line for line
  under names that agreed about nothing — `HighScore` in `high_score.bin`,
  `Best` in `best.bin`, and horde's `Best` whose number is a run length rather
  than a score. 987 lines of sample code became 389, and what is left is the
  part the engine could not have guessed: which directory, which file name, and
  which browser store.

- **crcbl** (`crcbl::session`): `Loopback`, the single-player session. Pairs an
  in-memory transport, builds the `Server` on one end and the `Client` on the
  other with the same tick rate and the same `ProtocolCompatibility`, hands the
  server its `GameModule`, and spends both clocks' first update at time zero.
  `tick_period`, `server`/`server_mut`, `client`/`client_mut` and `both_mut`
  reach the halves.

  "Single-player is a loopback server" is the engine's architectural decision —
  it is why `crcbl-server` and `crcbl-client` exist at all — and until now
  nothing in `crcbl` expressed it, so all four games implemented it from
  scratch. What stays the game's is what genuinely is: its
  `ProtocolCompatibility`, whose `schema_hash` is what stops one game's client
  hand-shaking with another's server, and its `GameModule`. Neither has a
  default, because a default for either is the wrong answer quietly.

  The baseline update at time zero is the subtle half. A `FrameClock`
  establishes itself on its first update and runs no ticks for it; doing that at
  construction is what lets a game's `tick` promise that every later call runs
  exactly one. Left to the caller, the first frame of the game silently
  simulates nothing.

- **crcbl-audio** (`crcbl::audio::synth`): waveform generators. `sine` for a
  one-shot beep, `looped_sine` for a tone that joins to itself, `noise_burst`
  for a decaying impact, and `fade_gain` for the click-free envelope under the
  first and last. Deterministic: `noise_burst` draws from a caller-supplied seed
  through `crcbl_core::rand`, so the sound a build ships is the sound every
  build ships.

  The crate had a mixer, a sound bank, an output stream and a spatial cue
  grammar, and no way to make a _sound_ — so all four samples wrote one. `sine`
  and its fade helper were byte-identical in flappy, asteroids and horde;
  breakout had the same pair under the names `gen_sine` and `fade_env`.

  Three functions, not a synthesiser: no envelope generator, no filter bank, no
  configurable oscillator type. Three is what the four samples between them
  actually use. Horde's swept `rise` has one caller and stays in horde, now
  built on `synth::fade_gain` and `synth::TONE_AMPLITUDE` so its level cannot
  drift from the engine's.

  **Nothing about the shipped audio changed** — the generators were adopted
  verbatim, and the sample buffers were compared to the engine's element by
  element before the copies were deleted.

- **crcbl** (`crcbl::engine`): frame pacing. `FrameLimit` caps how fast a
  real-time loop runs — a thousand frames a second by default, which is a
  runaway guard rather than a pacing policy, and `Clock::set_limit` changes it.
  The limiter lives on the clock rather than in the loop because every sample
  already calls `Clock::advance` once a frame, so a game gets it without asking;
  and because a manual clock has no wall clock to wait against, a headless run
  is unpaced **by construction** rather than by a check somebody has to
  remember.

  `Pacing` — `Vsync`, `Adaptive` or `Off` — replaces the hard-coded present-mode
  preference and is set through `GpuContextDesc::pacing`. One value rather than
  two flags, so "vsync on, adaptive sync on" is a state that cannot be written
  down instead of one the engine rejects at run time.

  **Nothing here turns adaptive sync on**, and that is not an omission: VRR is
  negotiated between display, driver and compositor, and an application never
  enables it. What changes is what presenting means — on a VRR panel the present
  does not wait for a fixed vblank, the panel follows the presents — so the
  engine's job is choosing a present mode and then staying inside the panel's
  range, which is what the limiter is for. Whether a panel is _actually_ running
  variable-refresh needs `VK_EXT_present_timing`, which is provisional and has
  no bindings in the pinned `ash`; until then `Adaptive` is a request rather
  than an observation.

- **crcbl** (`crcbl::engine`): `Loop`, the frame owned by the engine, and
  `HostedGame`, the seam a game reaches it through. `Loop::frame` pumps the
  shell, routes the input, runs the ticks the clock owes, draws and presents;
  `HostedGame` is the six things that genuinely differed between five samples —
  `menus`, `tick`, `key_event`, `menu_action`/`apply`, `menu_kind`, `draw` — and
  `summary`, which adds a game's own fields to the shared `RunSummary`.
  `FrameInfo` tells a `draw` what its frame did, and `LoopConfig` carries the
  three values that come from the command line rather than the game. `Loop`
  implements `GameLoop`, so `drive` and `crcbl::web::App` step it unchanged.

  `GameGpu` is the frame's half of a game's GPU bundle — `atlas`, `set_menu`,
  `take_draw_list`, `timings`, `frame`, `destroy` — and all five samples already
  had every one of them, with these signatures, as inherent methods.

  **`HostedGame` is not `crcbl::ecs::GameModule`.** That one is the simulation
  the server hosts and a wasm binding will have to reproduce bit for bit; this
  one is the presentation the loop hosts. A game implements both.

  `PolledGpu`'s `extent` and `resize` move to a new `GpuSurface` supertrait,
  which `PolledGpu` and `GameGpu` both require — the same two questions, asked
  by start-up and by the running frame, and declaring them twice on one type is
  how the two answers drift apart. The four samples with a browser build split
  their existing `impl` accordingly; nothing else changes for them.

  `apps/bare` never adopts it: it is the guard that the library path —
  assembling `GpuContext`, `Pending` and `FrameBudget` by hand — keeps working,
  and `crates/crcbl/tests/library_seam.rs` is what proves it from outside the
  crate.

  585 lines of engine and 343 of fixture and tests, against a `FakeGpu` that
  counts presents and a `FakeGame` that records what the loop asked of it —
  including an assertion that the loop never asks a game about a reserved
  `WidgetId`, which is what would silently re-point a resume button.

### Changed

- **breakout**: the first game hosted by `crcbl::engine::Loop`. `Breakout` is
  seven `HostedGame` methods and three fields — the simulation, the state it
  renders from, and its HUD — where `app.rs` used to carry the whole frame.
  `Loop<S>` is now a type alias for the engine's, so `run`, `start` and
  `with_shell` are free functions rather than inherent methods on it.

  Its menu vocabulary shrank to the part that was ever breakout's: `Launch`, on
  `LAUNCH_ID = FIRST_GAME_ID`. `MenuAction::{Resume, Fullscreen, DebugOverlay}`
  and the ids that carry them are the engine's, and `web.rs` lost its whole
  `WebLoop` impl — `crcbl::web` blanket-implements it for every engine loop,
  taking the name and the summary line from `HostedGame::NAME` and
  `HostedGame::log_summary`.

  **Nothing about the game changed**, and its own tests are the evidence: all 79
  pass unmodified except where they reached a field that is now behind an
  accessor, and the browser gate ran 27/27 checks against a real WebGPU device.
  `app.rs` lost 309 lines and `web.rs` 27, against 30 of `GameGpu` forwards in
  `gpu.rs`.

- **flappy**: hosted by `crcbl::engine::Loop` too, on the same shape as breakout
  — `Flappy` is seven `HostedGame` methods over the simulation, its render state
  and its HUD; `Flap` on `FLAP_ID = FIRST_GAME_ID` is all its menu vocabulary
  still declares; `web.rs` lost its `WebLoop` impl.

  It needed nothing the seam did not already have, which is the useful result:
  the bird's wing animation is stepped by `FrameInfo::ticks`, the field added
  for exactly this. Its own 86 tests pass and its browser gate ran 27/27.
  `app.rs` lost 288 lines and `web.rs` 28, against 30 of `GameGpu` forwards.

- **asteroids**: hosted by `crcbl::engine::Loop` as well, and it gained a fix on
  the way: **a refused fullscreen is now reported.** The sample never called
  `check_mode_request`, so a player on a tiling window manager pressed F11 and
  got no window change and no log line saying why; the engine's loop checks once
  a frame for every game it hosts.

  `Fire` on `FIRE_ID = FIRST_GAME_ID` is what its menu vocabulary still
  declares. `render_alpha` stays — this is the sample that interpolates
  rotations across a tick, and `FrameInfo::alpha` is where the number now comes
  from. `app.rs` lost 234 lines and `web.rs` 29; its 93 tests pass and its
  browser gate ran 27/27.

  The seam grew `Loop::{set_paused, gpu_mut}` for it: a test paused the loop by
  assignment, and its sprite read-back takes `&mut self`.
  - **sandbox**: the last conversion, and the one that measures the others.
    `Sandbox` is a struct with **no fields**: the sandbox has no simulation, no
    HUD and no score, and it still runs, pauses, opens a menu, goes fullscreen
    and reports a summary — all of that is the engine's now. Its `MenuAction` is
    `Infallible`, which makes `MenuAction::Game` uninhabited and is the type
    system agreeing that its three buttons are the loop's.

  It also stops declaring the six reserved keys for itself. `DEBUG_OVERLAY_KEY`
  and its five siblings were the engine's constants already, and a second
  declaration is how "the same key does the same thing in every sample" quietly
  stops being true.

  `app.rs` lost 379 lines and `menu.rs` 29; its 35 tests pass.

  `FrameInfo::tick_dt` and `HostedGame::tick` widened from `f32` to `f64`, which
  is what `FrameClock::tick_dt_secs` reports — the sandbox is the only game that
  reads it, and narrowing it was the engine deciding a precision on a game's
  behalf. `Loop::events` joins the accessors for the same reason the others did:
  a test read the field.

- **horde**: hosted by `crcbl::engine::Loop`, and the sample that stretched the
  seam. Its level-up panel is three upgrades the run's seed picked, so
  `HostedGame::menu_kind` now takes the loop's own `MenuSet` and a game may
  rebuild a panel before the kind it returns is shown. Its debug panel carries a
  section no other sample has, so `HostedGame::debug_sections` exists — empty by
  default, because "this game adds no section" is the honest answer for the
  other four. And it is the first game with **two** menu actions, `Restart` on
  `RESTART_ID` and `Choose(n)` on a reserved block above it.

  It also gains the refused-fullscreen report, for the same reason asteroids
  did. `app.rs` lost 205 lines and `web.rs` 32; its 124 tests pass and its
  browser gate ran 27/27.

  **The CPU frame report moved into the engine.** `Loop::finish` logs the clock
  it was driven from, the frame count, and mean/fps/best/worst — `apps/horde`
  wrote that itself and `--wall-clock` exists to make it mean something; every
  hosted game gets it now. The scene stats it used to carry are on horde's own
  `Summary` instead, so `main.rs` prints them natively and `log_summary` does in
  the browser.

- **crcbl** (`crcbl::engine`, `crcbl::web`): the sample loops' shared machinery
  moves into the engine, in four further slices.

  `open_window` logs the backend, aligns the shell's event clock with the
  engine's and creates the window, taking the caller's `WindowDesc` because a
  title and a size are the game's. `MAX_FRAME_STEP` joins it as an engine
  constant: the browser behaviour it guards against is the shell's.

  `PolledBoot`, with the `PolledGpu` trait, owns browser start-up — the pump,
  the configure/device state machine, the fix for a canvas resized while the
  device request is in flight, and the refusal to restart a boot that already
  finished or failed. It hands back `Booted` rather than a loop, because
  assembling one is the game's.

  `MenuPump` owns the menu's half of a pump batch: the three menu keys
  (`MENU_UP_KEY`, `MENU_DOWN_KEY` and `MENU_ACTIVATE_KEY`, now the engine's
  alongside the three reserved ones), the select/press/activate routing, and the
  held-key list. It answers with a `WidgetId`, leaving the mapping to a game's
  own action enum where it belongs.

  `crcbl::web` takes the browser entry point's shared half: the status codes — a
  wire format the JS shim switches on, so one definition is the only way they
  stay in step — the bounded log queue, and the whole `App` lifecycle behind the
  `WebLoop` and `WebPending` traits. It is deliberately not gated to `wasm32`,
  because gating it would put its tests on the one target the suite never runs.

  `run_ticks` is the fixed-step accumulator, with the rule that a **paused**
  frame still drains — the alternative banks the pause and spends it in one
  catch-up burst on the frame the player resumes. `FrameBudget` replaces the
  three fields every sample carried separately, because the reconfigure cap
  exists only so that a budget counting _presented_ frames stays reachable.
  `lose_focus` releases every held key before pausing, so a game does not resume
  believing a key is still down. `drive` is the native driver, behind a
  `GameLoop` trait that `crcbl::web::WebLoop` now requires — so the native and
  browser paths provably step the same loop.

  `PointerCapture` holds what the loop remembers about the pointer between
  frames — where it was left and whether its button is down — and resolves a
  batch into a `PointerInput`. `ModeRequest` holds the fullscreen request and
  whether the window system agreed, reporting what the window actually is rather
  than what was asked for.

  Measured: the four `app.rs` files lost 919 lines, and the four `web.rs` files
  went from 2642 to 1466. What the samples keep is what genuinely differs — each
  game's `assemble`, its `MenuAction` handler, its HUD, and the one log line
  reporting what a finished run was worth.

- **crcbl** (`crcbl::engine`): `LoopError<G>` replaces the error enum each
  sample wrote out for itself. The five loop failures — `NoWindowSystem`,
  `Shell`, `Configure`, `NeverPresented` and `Gpu` — belong to the loop however
  the game above them is spelled, and `G` names whatever the game itself
  refuses. A game with nothing of its own to refuse leaves it at the default
  `Infallible`, which makes the `Game` variant uninhabited and costs nothing.

  `BreakoutError`, `FlappyError`, `AsteroidsError`, `HordeError` and
  `SandboxError` are now aliases for it, so they keep their names and every
  `Err(FlappyError::Gpu(…))` still reads the same. `ShellError`,
  `ConfigureError` and `GpuError` still convert with `?`; a game error is
  wrapped by name, `.map_err(FlappyError::Game)`, because a blanket `From<G>`
  cannot coexist with the three concrete ones — `G` may itself be `ShellError`.

  Two messages change as a result. The sandbox's `NoWindowSystem` hint no longer
  names a roadmap phase for the missing Win32 and AppKit backends, since the
  engine has no business quoting one; it still says a platform may have no shell
  backend and still points at `--headless`. And its `NeverPresented` message
  loses a run of eighteen spaces that a missing line continuation had baked into
  the string literal.

- **samples**: `apps/{breakout,flappy,asteroids,horde}` drop eleven dependencies
  apiece and `apps/sandbox` drops its last one. `glam::` is `crcbl::math::` and
  `log::` is `crcbl::log::` at every call site — the same crates through the
  umbrella, so no version can drift and no two copies of a `Mat4` can meet.

- **crcbl** (`crcbl::engine`): the default present mode is now `Fifo` rather
  than `Mailbox`. A windowed native run vsyncs unless it asks not to, where it
  previously ran uncapped. The browser is unchanged: its swapchain already
  logged `Fifo` before this and logs it after, because the WebGPU surface does
  not offer `Mailbox` for the old preference to have found.

- **horde** (`apps/horde`): the engine's fourth game and its scale sample — the
  core loop. One arena, one player with WASD movement and an auto-aiming weapon,
  three enemy kinds that seek and push off each other, contact damage, hit
  points, death and restart. Native and headless; `--max-enemies` sets the
  ceiling on live enemies (default 1500). Drawn as untextured quads through the
  UI pass, which the art sub-slice replaces.

  Where the earlier samples ask what the engine can host, this one asks **what
  one tick costs per live body**, so the interesting part is the query pattern.
  Separation is one `PhysicsSystem::overlap_sphere` per enemy per tick, of
  radius `r_self + slack` — and the omission of the _neighbour's_ radius is
  exact rather than sloppy, because a shape-aware overlap of radius `R` returns
  everything within `R + r_b`, which is precisely the pair set separation wants.
  Contact damage is one more such query, at `PLAYER_RADIUS`, where every result
  is by construction a hit. Aiming is a third, at the weapon's range, instead of
  a scan of the enemy list. The weapon itself is segment CCD.

  Provisional numbers were taken here and **superseded by the scale sub-slice
  below**, which measures a fixture that fits inside the arena and which
  separates a spread crowd from a converged one. Both sets are in
  `docs/plan/sample/03-horde.md` with their conditions.

  Two divergences from asteroids are deliberate. **The gun fires after the bolt
  sweep**, because a projectile swept on the tick it was created is swept from a
  point one whole step behind the muzzle, through the thing that fired it —
  asteroids has the same order the other way round, and the same latent segment.
  **A wall clamp is not a teleport**: it moves a body by at most one tick of
  travel, so it is a refit rather than the remove-and-re-insert asteroids'
  screen wrap needs.

- **horde** (`apps/horde`): art and progression. `.crpix` sprites for the
  player, the three enemy kinds and the XP pickups, baked by a `build.rs` and
  drawn through `SpriteRenderer` with `SampleMode::Pixel`, replacing the
  untextured quads the core loop shipped with. XP gems drop where an enemy died
  and are collected by walking over them; banking a threshold opens a "pick 1 of
  3" level-up screen over the frozen field, from a fixed pool of six upgrades
  (`RAPID FIRE`, `HEAVY BOLTS`, `SWIFT BOOTS`, `LONG BARREL`, `VITALITY`,
  `MAGNET`). Pause, level-up and death menus over `crcbl_render`'s shared menu
  art, with the pointer, F11 and focus handling the other samples have.

  **Two sheets, and the split is a batching decision.** `SpriteRenderer` starts
  a batch whenever consecutive sprites name a different sheet, so the player,
  all three enemy kinds and the gems are one 34-texel frame size in one sheet:
  the whole field is a single batch **whatever order it is emitted in**, with no
  grouping pass over the crowd and no way for the batch count to grow with the
  horde. Asteroids has to emit its rocks largest-first to hold three batches;
  this cannot get it wrong. What it costs is the transparent margin round the
  two small kinds — a runner is 13 texels of art inside a 34-texel quad — and
  that is bounded by the screen rather than by the field.

  The scale is 20 texels a world unit, chosen from the runner: three enemy kinds
  have to be told apart at a glance in a crowd, which needs about thirteen
  texels across, and 13 / 0.64 units is 20.3. No scale makes all three enemy
  collider boxes a whole number of texels — the radii were picked for how the
  game plays, and it would take 50 texels a unit — so the shared frame is the
  largest one, which at 20 is exactly 34, and each silhouette is drawn to its
  own collider inside it.

  A level-up **freezes the field**, and the freeze is simulation state rather
  than the loop's pause: which upgrade a run took changes what the simulation
  does, so a seeded replay has to reproduce it, and the menu presses a real
  digit key into the action map rather than calling into the game. The freeze
  costs one pass on the tick it opens — a zero velocity written to the player,
  every enemy and every bolt — rather than a branch on the tick's hot path.

- **horde** (`apps/horde`): audio, the longest run, the browser demo, and the
  scale measurement the sample exists for. Five procedural spatial cues — the
  gun, an enemy coming apart, a gem banked, a level gained and the player's own
  end — with the listener **on the player**, which is the first sample whose
  listener moves. The longest run survived is kept in `~/.config/horde/best.bin`
  or the browser's Origin Private File System, in whole seconds so the record
  compares as the `m:ss` the HUD shows. The demo is live at
  `https://crcbl.kryptic.sh/demos/horde/` and the browser gate covers it at
  26/26, alongside the other three.

  **`crcbl-audio` has no voice limit, and this is the first sample that could
  not ignore it.** A kill is a cue and a gem is a cue against a fire cooldown
  whose floor is a twentieth of a second, so a late run raises about forty a
  second and each is a voice that lives until it runs out. The sample caps
  itself at sixteen, refuses the newest, and counts the refusals — and keeps
  counting the _cue_, because "did this happen" and "was there a speaker free"
  are different questions and only the first is what a test should be able to
  ask.

  Two flags carry the measurement, and both are in the shipped binary because
  the numbers have to be reproducible from a command line: **`--prefill N`**
  stages `N` enemies over the whole arena before the first frame (the spawner
  would take over ten minutes to reach the plan's target and nothing survives
  that long) and raises `--max-enemies` to fit them; **`--wall-clock`** drives a
  headless run from the real monotonic clock, so the debug panel's frame-timing
  module measures the frame instead of reporting the fixed step a headless clock
  hands it. The panel also gains this sample's own `scene` section — field,
  culled, drawn, batches — so the numbers the sample's argument rests on are
  readable in the running game.

  **The measurement, with its conditions in `docs/plan/sample/03-horde.md`.** On
  a Radeon RX 7900 XTX (radv), release, headless offscreen ring at 960 × 720,
  single-threaded:
  - **The render side is flat and the exit criterion is met.** CPU frame time
    0.096 ms on an empty field and on a field of a thousand, and 0.120 ms with
    ten thousand — nine thousand more enemies for 24 µs a frame, 0.14 % of a
    16.67 ms budget. With the driver taken out (`--backend null`) the game's own
    share is 0.005 ms to 0.033 ms. The `sprites` GPU pass goes 0.006 ms to 0.023
    ms.
  - **The batching claim holds.** Two draw calls at every count, and still two
    over ten thousand sprites with the whole field packed inside the view so
    that nothing is culled.
  - **The transparent margin is visible and does not matter.** The average enemy
    fills 31.5 % of its shared 34 × 34 quad, weighted by the mix the spawner
    deals, so about 12 µs of the sprite pass is margin at a full screen of the
    crowd — 0.07 % of the budget, against a grouping pass and an emission order
    to get wrong.
  - **The tick is what breaks, and it breaks on _density_ rather than on
    count.** Ten thousand enemies cost 14.66 ms a tick spread over the arena and
    84.09 ms once the crowd has converged on the player. Separation is one
    broadphase query per body and a query costs what its answer costs; a horde
    converges by construction. So the sample carries about ten thousand spread
    and about three thousand converged, and the plan's single figure was always
    going to be one or the other.

  **What that says about P7 and P8**, which is the reason the sample was built
  out of order in the first place: P8 (`crcbl-jobs`, the parallel schedule) is
  worth the whole of the gap — the steering pass is order-independent by
  construction and has no shared mutable state — and P7 (GPU culling, indirect
  draws, instance deltas) can return at most 0.7 % of a frame here, because the
  CPU cull it deletes costs 28 µs. The roadmap had horde waiting on P7; it was
  waiting on P8.

- **crcbl-render**: `Sprite::rotation` — sprites can turn. A per-sprite angle in
  radians, counter-clockwise, about the centre of the sprite's own `rect`. It
  rides in the fourth component of `SpriteInstance::sheet`, which was padding,
  so the instance is still 64 bytes and no buffer, stride or bind group changed.
  `Sprite` gains a field, so every struct literal that builds one needs
  `rotation: 0.0`; that is the only source-breaking part.

  Rotation interacts with `SampleMode::Pixel`, and both halves are decided
  rather than left to fall out. The **snap** stops rounding each corner once the
  quad is turned — a rotated quad has no axis-aligned rectangle to round onto,
  and rounding four corners independently shears it, changes its size and
  changes its effective angle, so a slowly turning ship would wobble — and
  instead translates the whole quad rigidly so its _centre_ lands on the pixel
  grid, which keeps the shape exact and still removes the sub-pixel crawl that
  translation causes. **Sharp bilinear needs no change at all**: `fwidth` is a
  per-fragment screen-space derivative, so it tracks the turned UV gradient by
  itself; being an L1 norm it reports up to root two times the scale on the
  diagonal, which widens the crossover band to about 1.4 fragments and never
  narrows it.

  A sprite with `rotation: 0.0` is **bit-identical** to one from before this
  change, by construction rather than by rounding luck: `sprite.slang` branches
  on the angle and the zero path is the arithmetic that was already there, down
  to the same SPIR-V `OpFMul`/`OpFAdd` pair. All eight existing golden images
  pass unchanged, at zero differing pixels.

- **crcbl-phys**: the broadphase BVH is **dynamic**. `Bvh::insert` and
  `Bvh::remove` add and drop one element along a single root-to-leaf path, and
  `PhysicsWorld::add_*` / `PhysicsWorld::remove` use them, so a world whose tree
  already exists no longer throws it away on every spawn and kill. A game that
  fires a bullet per shot and splits a rock into two used to pay a full
  `O(n log n)` rebuild for each of those events, every frame, on a tree it had
  just built. Batch population before the first query is unchanged: with no tree
  yet, adds accumulate and one bulk `Bvh::build` still runs, which produces a
  better tree than the same elements inserted one at a time.

  Insertion picks where a leaf goes by the surface area heuristic and the walk
  back to the root **rebalances** (AVL single rotation), which is what makes the
  quality claim hold rather than depend on the input. Measured over 20k
  insert/remove pairs: peak depth 13 at 1024 elements against an ideal of 11
  (`ceil(log2 n) + 1`), and 9 at 64 against 7. Without the rotation the same run
  on 1024 _coincident_ boxes — where every candidate site costs the same and the
  heuristic has nothing to choose by — reached depth 623, a tree that is very
  nearly a linked list. `Bvh::depth`, `Bvh::len` and `Bvh::is_empty` are public
  so the property is observable; `crates/crcbl-phys/tests/churn.rs` bounds depth
  by the AVL bound over thousands of operations and checks every query against a
  brute-force scan after each one.

- **crcbl-phys**: `ThrustForce` and `DampingForce`, the first two L1 force
  providers driven by a game rather than by physics for its own sake.

  `ThrustForce` is the first force that reads the body's _orientation_:
  `F = magnitude · (rotation × local_direction)`. The local axis is named rather
  than fixed at `Transform::forward` (`-Z`) because a top-down 2D game turns its
  ship about Z, where `-Z` points at the camera and thrusting along it would
  drive the ship out of the playfield plane. `ThrustForce::world_force` exposes
  the same vector to callers who are not using the provider pipeline.

  `DampingForce` is `F = -min(k, m/dt)·v`. The cap is the point: plain `-k·v`
  integrated at `k·dt/m ≥ 2` _reverses_ the velocity and then grows it, so a
  coefficient that behaves at a 240 Hz substep explodes at a 10 Hz one. With the
  cap the worst case is a velocity that reaches exactly zero. `DragForce` is
  deliberately left uncapped — it is the physical law, and a caller modelling a
  fluid wants the law.

- **crcbl-phys**: `PhysicsSystem::apply_force(entity, force)` adds a force to
  one entity for the next `step`. Force providers are global — every dynamic
  body gets every provider — which is right for gravity and wrong for the thrust
  of the one ship among a screenful of rocks.

- **crcbl-ui**, **crcbl-render**, **breakout**, **flappy**, **sandbox**: the
  samples' start, pause and end-of-game states are **menus** — a nine-sliced
  pixel-art window frame with skinned buttons inside it, centred in the
  framebuffer at every aspect ratio, replacing the flat rectangle and three
  lines of text each sample drew from its own `draw_pause_menu`.

  The art is **shared** and lives in `crates/crcbl-render/assets/menu.crpix`,
  baked by that crate's new `build.rs`: `apps/*` cannot depend on each other, so
  per-sample art would have been the same window authored three times and three
  games that looked like three engines. `crcbl_ui::menu` owns the model and the
  layout — `Menu`, `MenuItem`, `MenuStyle`, `MenuLayout`, all in screen pixels
  with no device in the room — and `crcbl_render::menu` owns the pictures:
  `MenuArt` cuts the five frames out of the sheet, `MenuRenderer` draws them
  through a `SpriteRenderer` of its own with a screen-space camera, and the
  labels stay on the UI pass. `crcbl_render::ButtonSkin` and
  `crcbl_ui::Button::with_skin`, which shipped unused, are what the buttons are
  drawn with.

  **The keyboard still works, and the mouse now does too.** Every key a sample
  bound still does exactly what it did, and each is printed on the button beside
  it; the menus add Up, Down and Enter, taken only while a menu is on screen.
  Pointer motion and clicks reach `Menu::point` through `UiState`'s press
  capture, so a press that starts on one button and is released over another
  fires neither. Both devices produce the same action.

  Behind the menu the game keeps drawing and is dimmed by a scrim sprite — drawn
  by the menu's own pass, between the game and the UI, so the panel and its
  labels are not dimmed with it. Breakout's start menu is a fresh game only:
  `WaitingForLaunch` is also where a player waits after losing a life, and a
  modal between every life would be three panels a game.

- **breakout**, **flappy**, **sandbox**: a pause state, entered and left with
  **Escape** and entered by losing window focus. A paused loop stops calling the
  game's tick, so the simulation does not advance at all; the HUD's status line
  reads `PAUSED` rather than whatever the server last thought, and a menu is
  drawn over the frame — text through the existing HUD path, behind a single
  `draw_pause_menu(&mut DrawList, extent)` per sample that the art slice
  replaces without touching the state machine. Pause is the loop's, not
  `GameState`'s: it is the loop declining to advance the simulation, and a
  `Paused` variant would put a value in the authoritative server's state that
  depends on which window a compositor has focused. `Loop::is_paused` and
  `Summary::paused` report it.
- **breakout**, **flappy**, **sandbox**: a fullscreen toggle on **F11**, which
  asks the shell for `DisplayMode::Borderless` and reads back what the window
  system actually did. There is no remembered `fullscreen` flag to disagree with
  the compositor — `Loop::display_mode` and `Summary::mode` are the _effective_
  mode, the toggle picks its target from it, and a request the window system
  refuses is logged once and reported as the mode the window really has.
- **crcbl-shell**: `__crcbl_web_fullscreen(canvas, state)`, the web backend's
  new shim entry point. A browser grants `requestFullscreen` only from inside a
  user-gesture handler and wasm is never inside one, so the page's shim makes
  the call from its own `keydown` and reports the outcome here; the backend
  moves `WindowConfiguration::mode` to match, which is what finally lets
  `WindowState::mode_request_honoured` answer `true` in a browser. An exit
  nobody asked for — Escape, which reaches no key handler — is reported the same
  way.
- **web**: `engine/shell.js` handles **F11** itself (and swallows the browser's
  own, which fullscreens the window rather than the canvas), listens for
  `fullscreenchange`, and synthesizes a focus loss on `visibilitychange` — a tab
  switch does not always blur the focused element, so `blur` alone leaves a game
  holding keys it will never see released. The demo pages gained a
  `STATUS_PAUSED` (6) status line, and `tools/browser-e2e.mjs` gained a
  focus/pause group that blurs the canvas in a real browser, checks that the HUD
  heartbeat stops, that focus coming back does not resume on its own, and that
  Escape does.

  **On a canvas, the click that restores focus is also a click in the game.**
  There is no title bar to click, so `shell.js` calls `canvas.focus()` from its
  own `pointerdown` handler — which means "clicking back in" lands a real press
  at a real position, and a press that lands on the pause menu's `RESUME` button
  resumes, exactly as it would with the game already focused. Focus itself still
  never resumes, on any platform. The two are separate and the samples' new
  `a_focusing_click_off_every_button_leaves_the_game_paused` pins them apart.

- **crcbl-ui**: `crcbl_ui::debug` — the modular debug overlay every sample now
  ships. `DebugPanel` holds `DebugSection`s and names no system; a system
  contributes by implementing `DebugModule`, whose one method fills a section it
  is handed, and the frame calls `DebugPanel::add` once per system it actually
  has. `FrameStats` is the module every frame has: a rolling window of frame
  intervals reporting FPS, average, last, best and worst. FPS is frames divided
  by the time they took, not the mean of the instantaneous rates — the two
  disagree in exactly the case a profiler exists for. `DebugOverlay` bundles the
  panel with the frame window so a sample switches the whole thing on in one
  line. `Anchor::position` is the panel's anchoring arithmetic, lifted off
  `HudPanel` so there is one copy of it.
- **crcbl-render**: `FrameTimings` implements `crcbl_ui::debug::DebugModule`, so
  the per-pass GPU timestamps that already existed appear in the overlay as a
  `gpu` section — one row per pass, plus the total and the frame number. The
  adapter lives here rather than in `crcbl-ui` because the overlay is not
  allowed to know that a render pass exists.
- **breakout**, **flappy**, **sandbox**: the debug overlay, toggled with **F3**
  and defaulting to visible in a debug build. `--debug-overlay` and
  `--no-debug-overlay` override the default. Neither game has a network module —
  both run over `InMemoryTransport` — which is what makes them the check that
  the panel composes rather than hard-codes its sections. The sandbox gained a
  UI pass to carry it; it still has no HUD and is not getting one.
- **flappy**: a second game, playable natively and at
  `https://crcbl.kryptic.sh/demos/flappy/`. One button, a bird under gravity,
  and an endless procession of pipes whose gaps are a pure function of a seed
  and the pipe's index — so the client and the server agree about the course
  without a byte of it crossing between them. It exists to find out whether the
  engine could host a game that was not breakout; what it found is written down
  in `docs/plan/ROADMAP.md`.
- **asteroids**: a third game, playable headless and natively, and the
  workspace's first sample built around **entity churn** rather than around a
  fixed world. A ship that turns, thrusts and wraps; bullets that never miss;
  rocks in three sizes that split twice; waves that grow to a ceiling; score,
  three lives, game over and restart. Every random-looking number — where a wave
  enters, which way a split throws its children — is a pure function of a seed
  and an index, so a recorded script replays bit-identically and two games on
  one seed are the same game.

  It is the first consumer of the P6 physics slice, and the seams it uses are
  the ones that slice was bought for: `ThrustForce::world_force` through
  `PhysicsSystem::apply_force` for the engine, `sweep_sphere` over a
  `prev → cur` segment for every bullet, and `overlap_sphere` against the
  broadphase for the ship. **A wrap is a teleport, and a teleport is a
  remove-and-re-insert** — the rule `docs/backlog.md` left to whoever wrote the
  wrap, chosen here and applied uniformly to everything in the broadphase.

  It is drawn as **pixel art through the sprite pass**: five `.crpix` sheets
  under `apps/asteroids/assets/` — a ship, a shot, and one per rock size — baked
  to PNG by its own `build.rs` and drawn with `SampleMode::Pixel`. Ten texels to
  the world unit, chosen by the small rock: eleven texels is the least a rock
  can be and still have a lump stick out and a bite go in, and eleven over that
  rock's 1.1-unit diameter fixes the scale. Every rock's frame is then its
  collider's bounding square to the texel — 34, 20 and 11 — and the three are
  three drawings rather than one at three magnifications, which is what makes a
  split read as a rock breaking rather than as a rock shrinking.

  **It is also the first sample where a drawn thing turns**, which the
  `Sprite::rotation` above only made possible. The ship's heading and every
  rock's tumble are integrated once per simulation tick, so drawing the newest
  value on every frame stutters at any refresh rate that is not the tick rate;
  the renderer interpolates instead, with the frame clock's alpha.
  `game::lerp_angle` takes the **short way round**, which is the whole
  difficulty: a plain lerp from 350° to 10° spins the long way, once, on the
  frame after the heading crosses zero — and `turn_ship` keeps the heading in
  `[0, τ)`, so it crosses constantly. Positions are deliberately _not_
  interpolated: this playfield wraps, and unlike an angle a wrapped position is
  a real discontinuity.

  Presentation is the shape the other two samples set: start, pause and
  game-over menus through `crcbl_render::MenuRenderer`, Escape to pause, F11 for
  fullscreen, F3 for the debug panel, and a window that loses focus pausing and
  releasing every key it was holding. That last one matters more here than in
  either earlier sample, because turning and thrusting are _held_ actions: a
  release that never arrives is a ship that spins for the rest of the session.

  **Sound**: three spatial cues through `crcbl-audio`'s grammar — the engine,
  the gun, and a rock (or the ship) coming apart. The listener is the camera at
  the middle of the field and it never moves, so unlike in either earlier sample
  the pan and the distance both swing their full range: emitters are spread over
  the whole 32 × 24 playfield and cross it constantly. The explosion is a
  decaying burst of low-passed noise from a fixed seed rather than a tone,
  because a beep reads as scoring rather than as destruction. Thrust is the
  first _sustained_ cue any sample has needed and `crcbl-audio` has no looping
  voice, so it is a one-shot re-fired every `THRUST_CUE_PERIOD` — a constant
  that lives in the simulation, because the cue is raised inside the
  deterministic tick.

  **A best score**, kept in `~/.config/asteroids/best.bin` natively, in the
  Origin Private File System in a browser, and nowhere at all under
  `--headless`. Recorded once, on the edge into game over.

  **A browser build**: `apps/asteroids` is a `cdylib` on
  `wasm32-unknown-unknown` and the demo is live at
  `https://crcbl.kryptic.sh/demos/asteroids/`. `Loop` gained
  `PendingLoop`/`set_frame_step` and `Gpu` gained `request_open`, so start-up is
  polled across `requestAnimationFrame` frames instead of blocking on a promise
  the page's own event loop has to resolve. `web/run-browser-e2e.sh` drives it
  in a real Chromium for 26/26 checks, the same as the other two.

- **crcbl-hal**: `Device::take_error`, for the failures a backend learns about
  outside the call that caused them. Defaults to `None`, so a backend that
  reports everything through its return values is unaffected.
- **breakout**: the ball's speed ramps 2% per brick broken, capped at 1.6x the
  launch speed. A lost life and a restart both put it back.
- **crcbl-render**: `texture::upload_texture` and `UploadedTexture`, a
  format-agnostic staging upload. It replaces `ui_pass`'s private R8-only
  helper, whose row pitch was computed in texels and passed to a copy that wants
  bytes — correct only because `R8Unorm` is one byte per texel. The pitch is now
  computed in bytes and converted back once, at the copy, so an RGBA8 upload
  lands where it says it does.
- **crcbl-sprite**: a `load` feature — `decode_png`, `read_aseprite_json` and
  `load`, which take a baked sheet back apart into a `Sheet` and tightly packed
  RGBA8. §7 of `docs/specs/crcbl/pix.md` specified what the sidecar contains and
  nothing read it, so a baked sidecar was write-only. `SampleMode` does not
  survive the trip — Aseprite's schema has nowhere to put it — and that is
  asserted rather than assumed.
- **crcbl-render**: `SpriteRenderer` and `sprite.slang`, an instanced
  world-space pass that draws one quad per sprite out of a registered sheet,
  alpha blended, batched by sheet in submission order. This is the instance path
  S1B finding 1 asks for: `ForwardRenderer` draws exactly one instance, which is
  why both samples push their worlds through the UI pass. Constants go through a
  uniform buffer on every tier, so unlike `ui.slang` there is no second source
  file to keep in step.
- **crcbl-render**: `SampleMode::Pixel` is sharp bilinear, not nearest. The
  linear blend is squeezed into a band one fragment wide at each texel boundary,
  so art pixels stay flat inside and cross over in one screen pixel at any
  scale, and the sprite's screen rect is snapped to whole device pixels.
  Nearest-neighbour was the placeholder: at a non-integer scale it makes some
  art pixels four screen pixels across and their neighbours five, and the
  unevenness crawls as the sprite moves. `SpriteInstance` grew a fourth `float4`
  carrying the sheet's size and the mode, so its layout changed.
- **crcbl-sprite**: `Playback`, which advances a clip over ticks — a bare `u64`
  cursor answering `frame_index` and `finished` as a closed form, so catching up
  after a stall lands exactly where tick-by-tick would. Ping-pong shows each end
  once (period `2n - 2` looping, `2n - 1` for a one-shot that has to walk home),
  and reverse carries each frame's hold with the frame rather than reversing the
  holds too. Also `Sheet::uv`, the frame rect as normalised UVs, which every
  caller was spelling out by hand.
- **crcbl-render**: `NineSliceSource::expand`, which turns stored insets into
  the quads that draw them — corners at their natural size, edges stretched on
  one axis, centre on both. Empty bands emit nothing, so a three-slice is three
  quads and a frame with no insets is one; the cut lines are computed once and
  indexed, so adjacent quads share their edges exactly and no seam opens up. A
  target below the corners' combined size shrinks them proportionally rather
  than letting them overlap and mirror.
- **crcbl-render**: `LayerStack`, `Layer` and `Parallax` — sprites grouped into
  back-to-front bands, each taking a chosen fraction of the camera's motion. A
  layer is a container rather than a field on `Sprite`, so nothing sorts and
  submission order inside a layer is still exactly what the caller gave.
- **crcbl-ui / crcbl-render**: skinned buttons. `Button::with_skin` takes the
  nine-slice insets its art was cut with, so its minimum size and its label's
  centring follow the frame rather than being guessed; `ButtonSkin` turns a
  state and a rectangle into the quads that draw it. Resizing moves the edges
  and leaves the corners alone, which is the whole point. The skin goes through
  the sprite pass rather than the UI pass — the UI atlas is a single-channel
  glyph mask, and `crcbl-render` already depends on `crcbl-ui`, so the reverse
  could never have happened.
- **crcbl-cli**: `crcbl crpix`, which turns PNG frames into one `.crpix` sheet
  in the order given, with `--nine`, `--sample`, `--clip` and `--hold`. Frames
  are named after their file stems; two inputs whose stems collide, or a stem
  the format cannot spell back, are refused rather than written out. An existing
  output is left alone without `--force`.
- **crcbl-ui**: `MenuSet<K>`, the container a game keeps its menus in. `Menu` is
  one panel; a game has several and needs to say which one a frame draws, to
  switch between them without carrying a half-finished click across, and to
  share one `UiState` so a press and its release are tested against the same
  capture. `K` is the game's own state type rather than one this crate dictates,
  and **a `K` the set holds no menu for draws nothing** — which is how "no menu
  this frame" is spelled, with no separate `Option`. `show`, `current`,
  `current_mut`, `is_showing`, `kind`, `select_next`, `select_previous`,
  `press`, `activate`, `point`, and `replace` for a panel whose buttons are
  built while the game runs. Both `show` and `replace` drop the pointer's
  capture; two entries claiming the same `K` are refused at construction,
  because the second would be unreachable.

### Changed

- **`crcbl-audio`**: the `Mixer` can now be driven by the game that owns it, and
  all four samples use it instead of a hand-rolled copy.

  `Mixer::play` took `&mut self` while `AudioStream::open` consumes its source,
  so once the stream was running nothing could reach the mixer to play through
  it — the shipped mixer was unreachable, and `apps/breakout`, `apps/flappy`,
  `apps/asteroids` and `apps/horde` had each written their own `Sound`, `Voice`,
  `VoiceQueue` and `MixerSource` around it. `play` now takes `&self` and answers
  with a `VoiceId`; `AudioSource` is implemented for `Arc<T>`, so
  `AudioStream::open(Arc::clone(&mixer))` leaves the game a handle to go on
  playing through. Existing callers keep compiling: no signature was narrowed,
  and `Mixer::play`'s new return value can be ignored.

  New alongside it: `Mixer::stop`, `Mixer::is_playing`, `Mixer::set_mix` and
  `Mixer::voice_mixes`; `VoiceId` and `VoiceMix`, with
  `VoiceMix::from(&SpatialCue)` as the "play this buffer once, panned" glue each
  sample was writing by hand (the cue's `itd_samples` is dropped — a `Voice` has
  no delay line); `Voice::with_mix`, `Voice::mix`, `Voice::is_looping` and
  `Voice::from_shared`; and `SoundBank::sound` / `SoundBank::insert_shared`.

  **`SoundBank::create_voice` no longer copies the sound.** `Voice` holds
  `Arc<[AudioSample]>`, so a voice is a playhead over the bank's buffer rather
  than a clone of it — at horde's cue rate that was an allocation the size of
  the sound per cue.

- **asteroids**: the engine is a real held sound, and an audio detail has left
  the simulation. `game::THRUST_CUE_PERIOD` and `GameLogic`'s `thrust_cue_timer`
  are **removed**: thrust used to be a one-shot re-fired on a countdown that
  lived in the deterministic tick, because the crate had no reachable looping
  voice. It is now one looping voice that `audio::Audio::set_thrust` starts on
  the first burning tick, re-aims at the ship every tick after (so the engine
  still pans across the field), and stops the tick the key comes up or the ship
  dies. What the simulation keeps is a plain `thrusting` bool, mirrored onto
  `Game::thrusting`.

  `THRUST_CUE_PERIOD` was re-exported from `apps/asteroids/src/lib.rs` and is
  gone from there too.

- **horde**: the game no longer starts itself. It opens on a `HORDE` start
  screen with a `PLAY` button — `Space`, which is the key breakout, flappy and
  asteroids print on theirs, and `R` still works — and the simulation does not
  advance until it is pressed: no spawns, no clock, no shots. The new
  `GameState::WaitingToStart` short-circuits the tick the way `LevelUp` already
  did, so a player looking at the title screen is looking at a still, empty
  arena rather than at a run that has been taking hit points off them since the
  window opened.

  **`TRY AGAIN` on the death screen now lands on that start screen**, not
  straight back into a run, which is what asteroids and flappy already do —
  restarting is two presses. `--prefill` starts its own run so the scale
  measurement still measures a running one. The sample deliberately shipped
  without a start screen; `docs/backlog.md` carries why that call was reversed.

- **flappy**: the game has art. A bird with a three-frame flap, a three-sliced
  pipe, and hills and a ground band on parallax layers, all authored as `.crpix`
  text under `apps/flappy/assets/` and baked to PNG + sidecar by a new
  `build.rs` — nothing baked is committed, so the text is the only source of
  truth and editing it rebuilds the game. The pipes were screen-space UI quads
  and the bird a lit cube through the forward pass; both are sprites in world
  coordinates now, drawn by `SpriteRenderer` between a `sky` clear and the HUD.
  Nothing about how the game _plays_ changed.
- **flappy**: `ForwardRenderer` is gone from the frame, and with it the HDR
  scene target, the depth buffer, the tonemap pass and the cube. The forward
  pass drew exactly one instance and the bird was it; a one-line `clear_color`
  pass replaces the clear it also happened to do.
- **breakout**: the board is art. Four bevelled brick frames — a brick's frame
  is read back out of its row, so a row's colour follows its position rather
  than being tracked beside it — a paddle, a ball, and a nine-sliced stone court
  whose wall faces land exactly on the colliders the ball bounces off. Authored
  as `.crpix` under `apps/breakout/assets/`, baked by a `build.rs` like
  flappy's. The forty bricks went through the UI draw list and the paddle was
  the one lit mesh; both are sprites now, and `ForwardRenderer` is gone from
  breakout too.
- **flappy**: the wing beats when the player flaps. The clip was a free-running
  loop that never looked at the bird, so the animation and the button had
  nothing to do with each other; a rising vertical velocity is exactly a flap,
  and it restarts the clip.
- **demo site**: the demo window is **one template**. The terminal frame, the
  canvas, the status bar, the focus note, the three keys the engine's loop keeps
  and the console note were the same markup written out per demo page; they are
  `web/templates/demo-*.html` now, pulled into a page with `<!--include …-->`.
  `build-pages.py` fails the build for a demo page that does not include them,
  so the next demo cannot go back to a copy.
- **demo site**: `web/engine/demo.js` is the boot sequence and the frame loop
  for every demo. `web/demos/breakout/main.js` and `web/demos/flappy/main.js`
  were 288 lines each and differed in the sample name, one status line and one
  comment — the shape that had already shipped breakout's control hint on
  flappy's page. Each is ~30 lines now: this sample's `__crcbl_<name>_*`
  symbols, written out literally so `check-exports.mjs` still sees every one,
  plus what to press and what it saves.
- **web tooling**: `check-exports.mjs` and `smoke.mjs` take `--sample <name>`,
  and `run-browser-e2e.sh` takes `CRCBL_WEB_E2E_DEMO`. Each was written when
  there was one demo and asserted against the whole workspace or against
  breakout's own strings, so the second demo broke all three. A sample's
  contract is now scoped to that sample, and the browser gate refuses a demo it
  has no expectations for rather than passing on a game that never started.

### Fixed

- **asteroids**: a bullet could hit a rock sitting **behind** the ship on the
  tick it left the gun. Segment CCD reconstructs where a projectile was as
  `position - velocity * dt`, so one created this tick was swept from a point a
  whole step behind the muzzle — through the hull and out the other side. The
  gun fires after the sweep now, as `apps/horde` already did, so a bullet's
  first sweep is its first real step. 0.4 of a unit at 60 Hz and six units at
  `--tick-hz 4`, which is where the new test looks.

- **crcbl-vk**: reusing an image from the **offscreen ring** was ordered against
  nothing, so the frame that took the image back could write it while the
  previous frame was still reading it. A headless frame ends in
  `vkCmdCopyImageToBuffer` — a read — and the next frame opens with a layout
  transition out of `ResourceState::Undefined`, which is a write that discards
  the contents. `Undefined` maps to `srcStageMask = NONE`, which is right for a
  WSI image because the acquire semaphore already carries that dependency, and
  wrong for a ring image because there is no such semaphore: the seam hands one
  back with an implicit acquire. Nothing separated the two.

  The transition out of `Undefined` on a ring image now widens its source stage
  to `ALL_COMMANDS`, whose first synchronisation scope covers everything already
  submitted to the queue — the missing dependency, and nothing more: the access
  mask stays empty, because a write-after-read needs execution ordering and no
  cache flush, and the contents are still discarded. WSI images, ordinary
  images, and the seam's public shape are all unchanged, and no caller needs a
  change.

  Affects offscreen and headless Vulkan rendering that outlives the ring:
  `crcbl screenshot`, the `crcbl-vk` e2e suite, and `--headless --backend vk`.
  Windowed rendering is untouched. Validation reports the bug as
  `SYNC-HAZARD-WRITE-AFTER-READ` with `read_barriers: VkPipelineStageFlags2(0)`
  — that empty mask being precisely the `NONE` above; without a layer it is a
  race whose outcome the GPU's speed decides.

- **crcbl-render**, **crcbl-shaders**: the sprite pass drew **every batch after
  the first from the first batch's sprites** on Vulkan. A batch is a run of
  sprites sharing a sheet, and `SpriteRenderer::add_pass` pointed each draw at
  its slice of the frame's instance buffer with `firstInstance` — but `slangc`
  lowers `SV_InstanceID` to `InstanceIndex - BaseInstance` for SPIR-V, so the
  index restarted at zero for every batch and each one redrew the first batch's
  sprites with a later sheet bound. A four-sheet frame put one rectangle on
  screen and left the rest empty. **Both samples register four sheets**, so
  `breakout` and `flappy` were affected on every native run since the pass
  shipped; the browser was not, because `slangc` lowers the same source to
  WGSL's `@builtin(instance_index)`, which WebGPU defines to include
  `firstInstance`.

  No shader source is correct on both targets while `firstInstance` is non-zero,
  so it is now always zero: every draw is `draw(0..6, 0..count)` and the batch's
  offset arrives in the new `SpriteConstants::base` field, through a
  dynamic-offset binding of set 0. **`SpriteConstants` is one block per batch
  rather than one per frame**, laid out at `SpriteRenderer::constant_stride()` —
  `CONSTANTS_SIZE` rounded up to the device's
  `min_uniform_buffer_offset_alignment` — and its `pad: [f32; 2]` has become
  `base: u32, pad: u32`. Callers of the pass are unaffected; anyone building
  `SpriteConstants` by hand is not.

  `crates/crcbl-vk/tests/vk_e2e.rs` gains a golden of three solid-colour sheets
  at four rectangles, which is red against the old pass; the batching tests in
  `crcbl-render` now pin the draw ranges at zero and the dynamic offset per
  batch.

- **breakout**, **flappy**: a window that lost focus kept playing, and kept
  saying so. The samples ignored `ShellEvent::Focus` entirely — on every
  platform, native and browser — so alt-tabbing away left the simulation running
  with the HUD reading `Playing`, and a life was lost while nobody was looking.
  Focus loss now pauses the loop and releases every key the game thinks is held,
  which is the obligation `ShellEvent::Focus`'s own documentation states: no
  platform delivers releases for keys held when focus leaves. Flappy had the
  worse half of it — its flap is an edge, and an action map that never saw Space
  come up raises no further `just_pressed`, so the bird could never flap again.
  Regaining focus deliberately does not resume.

- **crcbl-wgpu**: a shader module or pipeline that fails to build is reported.
  WebGPU hands back an object either way and delivers the reason to the device's
  error channel, so failures were invisible: the backend built a pipeline on a
  module that had not compiled and every submission after it was silently
  discarded, which presents as a black canvas over a game that reports itself as
  playing. Creation calls now return `HalError::Backend`, and the asynchronous
  half — the browser's, which no call can be blamed for — stops the frame loop
  from `GpuContext::acquire` with the driver's own message.
- **breakout**: the ball is no longer under gravity. It launches at a constant
  speed and collisions change only its direction, which is what makes a shot
  aimable.
- **breakout**: the paddle steers, by being moved. A paddle standing still
  mirrors the ball like a wall; a paddle being driven left or right decides
  which way the ball goes next, and turns a ball back the way it came rather
  than rebounding it onward.
- **breakout**: the whole play field is on screen at every aspect ratio. The
  orthographic camera derived its width from a fixed half height, so a 4:3
  surface — the size the window opens at, and the aspect the web demo's canvas
  is styled with — cropped two world units from each side and the ball
  disappeared off the edge before bouncing back.
- **crcbl-phys**: `PhysicsWorld::sweep_sphere` reports contacts it used to miss.
  The broadphase traversed the sphere's centre line, so anything the sphere
  overlapped by less than its radius was dropped before the exact test, and a
  contact landed only once the centre reached the surface.
- **crcbl-store**: `canonical_key` and the browser backends split keys on `/` on
  every platform. Parsing went through `std::path::Path`, whose separators are
  the host's, so `a\b` was refused on Linux and quietly rewritten to `a/b` on
  Windows.

[Unreleased]: https://github.com/kryptic-sh/crcbl/commits/main
