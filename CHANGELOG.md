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

- **The base-colour page carries a mip chain.** `crcbl_render::mip` builds one
  on the host — `resample`, the alpha-weighted box filter in linear light the
  glTF importer already packed textures with, now shared, and `chain`, that
  filter run down to one texel — and `ForwardRenderer::with_scene` uploads every
  layer's chain through the new `upload_texture_mip_layers`, one copy per level.
  Images from every `upload_texture*` are now created as copy sources as well,
  so a level can be read back. The page's sampler reads the chain trilinear —
  `Linear` on all three filters, no level clamp — so a minified texture no
  longer shimmers; anisotropy stays at one until the `anisotropic_filtering` key
  of `docs/plan/43-render-standards.md`'s filtering rung lands. The lights
  fixture's sun (`dim_sun`) is halved so its pools stay the brightest thing on
  their pyramids under a bilinear page. The page costs a third again in device
  memory for the chain.

- **Auto-exposure: the frame measures its own exposure, and no readback stalls
  on it.** `RenderEffects::AUTO_EXPOSURE` — the `auto_exposure` settings key —
  adds three compute passes before the tonemap: `exposure.slang`'s `clearMain`,
  `histogramMain` and `reduceMain` bin the finished frame's luminance and write
  one float into a device-local buffer the tonemap reads in the same frame.
  `crcbl_shaders::exposure` is the same arithmetic on the CPU — `bin_of`,
  `bin_luminance`, `measure` — and `ForwardRenderer::exposure_buffers` hands the
  bins and the measured exposure to a caller that wants to check them.

  The bins are the exponent field of the luminance rather than a `log2`, so they
  are identical on all four backends; the reduce runs on a single invocation
  because float addition is not associative and a tree would sum the bins in an
  order the device schedules.

  **It is not in `DEFAULT_STACK`**, and it is the first post effect with no
  additive-zero form — a measured exposure is by definition not the one the
  caller set, so a view has to ask. A frame that does not ask draws exactly the
  picture it drew before the pass existed.

- **Auto-exposure rolls between frames instead of cutting.**
  `ForwardRenderer::set_exposure_adaptation` takes an `ExposureAdaptation` —
  `brighten` and `darken` rates in fractions of the remaining distance per
  second, and the view's own frame delta — and the reduce writes a step from
  what the frame before was exposed by rather than the measurement itself. The
  previous value is the slot behind the one this frame writes in the `measured`
  ring, and every slot is filled with `DEFAULT_EXPOSURE` before any frame
  exists, so the first frame's step starts somewhere defined.

  Two rates because a real eye adapts down to a bright scene faster than it
  adapts back up. The step is linear rather than `1 - exp(-rate * delta)`: an
  exposure multiplies every texel, and no transcendental may reach a colour
  here. `crcbl_shaders::exposure::adapt` is the same arithmetic on the CPU.

  **`None` — the default — is the whole distance in one frame**, which is the
  picture the pass drew before this landed, exactly: the shader branches on the
  endpoints rather than interpolating, because
  `previous + (target - previous) * 1` is not `target` in floating point.

- **The froxel column can be read back and checked per froxel.**
  `ForwardRenderer::froxel_buffers` hands out one frame's `FroxelBuffers` — the
  parameter block, the column and the per-froxel sun visibility — and
  `crcbl_shaders::volumetric::VolumetricParams::from_bytes` decodes that block,
  so a caller models the volume from the numbers the shaders were handed rather
  than from a second derivation of them. The three buffers all carry
  `TRANSFER_SRC` now. `crates/crcbl/tests/mesh_e2e/froxels.rs` is the first
  caller: it rebuilds every slab and scans it on the host, which is what tells a
  wrong scatter from a wrong scan where a composited frame can only say that one
  of them moved.

- **`apps/lantern` asks for the froxel volumetric path, so something dispatches
  it.** `room::View::Main`'s camera stack now carries
  `RenderEffects::VOLUMETRIC_FOG` — the only view in the workspace that asks for
  it. `crcbl_render::volumetric`'s scatter, scan and composite were built on
  every frame whatever the bit said and dispatched by nothing but one Vulkan
  test, so every other backend compiled the artifacts and never ran them; the
  scatter pass binds a **comparison sampler to a compute stage**, which a driver
  can refuse where the artifact still compiles. Every backend that draws this
  room now runs the column, the browser gate runs it on WebGPU, and the gate
  asserts the resolved row the demo prints.

  **No frame moved.** The fixture hands its renderers no `Fog`, and a view with
  `Fog::NONE` draws the same picture through either integrator to the bit, so
  both goldens are the ones already checked in.

  `apps/lantern/tests/golden.rs`'s
  `the_air_scatters_the_sun_where_the_cascades_let_it_through` is the first
  rendered claim about the froxel path anywhere: two arms in the same medium
  differing only in `Fog::sun_scattering`, asserting that a block whose column
  is sunlit air gains and a block whose column the cascades shut out does not
  move. Replacing the scatter pass's own `visibilities[froxel]` with `1.0` — a
  sabotage `docs/backlog.md` records as leaving all 28 `mesh_e2e` tests green to
  the digit — now reddens it.

- **A game can hide its cursor, or name a shape for it.** `HostedGame::cursor`
  answers `Option<CursorIcon>` — `None` hides — and the loop reconciles it
  beside `HostedGame::pointer_mode`, calling `Shell::set_cursor` only when the
  answer changes. Every backend already implemented the shell call and nothing
  outside `crcbl-shell` had ever made one, so until now the shape was
  unreachable from a game and a hidden cursor was not a thing the engine could
  do.

  It is the **second axis**, not a fourth pointer mode: `PointerMode` says where
  the pointer may go, this says what is drawn on it. That is the split GLFW
  arrived at (`GLFW_CURSOR_HIDDEN` versus `CAPTURED` and `DISABLED`) and the one
  Unity uses (`CursorLockMode` plus `Cursor.visible`). A shooter drawing its own
  reticle hides the cursor with the pointer free; a strategy game confines the
  pointer and keeps it visible. `PointerMode::Locked` still hides the cursor by
  itself on every platform that has a lock — the request is what it comes back
  to when the lock ends.

  **`breakout` confines the pointer to the board and hides the cursor while the
  paddle is being driven** — the pair GLFW calls `CAPTURED`, and the first use
  of `PointerMode::Confined` anywhere in the workspace. That game binds the
  pointer's absolute `x` straight to the paddle, so a visible arrow was a second
  pointer drawn a paddle's height above the first, and a hand that ran past the
  window's edge parked the paddle and left a dead zone to cross on the way back.
  Every menu it has — start, pause, won, lost — frees the pointer and gives the
  arrow back, because a menu is not something a paddle can point at; so does
  focus loss, which pauses the loop before the reconcile runs. A browser has no
  confine primitive, declines the request and says so in the log, and the demo
  plays exactly as it did.

  **`viewer` is the first sample to answer it**: the whole canvas is a
  turntable, so it shows the open hand every model viewer shows, closes it into
  a fist while the hand is holding, and hands back the ordinary arrow while the
  pause panel is up — a grab hand over a row of buttons says the wrong thing
  about what a click there would do.

  The browser honours it too: `Shell::set_cursor` publishes the CSS keyword
  `CursorIcon::as_css_name` already answers in — `none` for hidden — and the
  shim writes it to the canvas once a frame. `PointerMode::Confined` remains
  unsupported on web and errors rather than doing nothing, because no browser
  has a confine primitive.

- **A slider row answers the keyboard.** `ArrowLeft` and `ArrowRight` move the
  highlighted slider by `Slider::KEY_STEP` — twenty presses end to end — through
  the new `Menu::nudge_slider` and the engine's `MENU_LEFT_KEY` /
  `MENU_RIGHT_KEY`. `Menu::activate` reports nothing for a slider by design, so
  until now a player with no pointer could select a volume and had no key that
  would change it, in a widget set whose stated primary input is the keyboard.

  The two keys are claimed **only while the highlighted row is a slider**, which
  is narrower than the other three menu keys: `apps/asteroids` turns the ship
  with them, and every menu in the workspace but one is a list of buttons, so a
  panel that took the arrows outright would swallow a game's turn key every time
  it paused. `Menu::slider_highlighted` is the predicate.

- **`options`, the first settings screen in the workspace.**
  `cargo run -p options` opens a panel of faders over the six `[engine.audio]`
  buses — master, music, effects, interface, voice, ambience — writes them to
  the player's own settings file on `SAVE`, and places them from that file on
  the next start. `RESET` puts every bus back to unity. It is the first
  application to write a setting: `SettingsStack::save_platform` shipped with no
  caller a player could reach, and this is it. The video half of the catalogue
  is not on the screen yet — `docs/plan/sample/20-options.md` has it.

  **The faders are audible while they move.** `apps/options`'s `audio` module
  puts three of the six buses on content a player can tell apart by behaviour
  rather than by timbre: a looping tone on `Bus::Music` from start-up, a noise
  tick once a second on `Bus::Sfx`, and a click on `Bus::Ui` each time a fader
  passes a detent or a button is pressed. `Bus::Master` multiplies all three, so
  the routing is audible too. Moving a fader moves `Mixer::set_bus_gain` in the
  same call that writes the key, so what a player hears and what `SAVE` would
  write cannot come apart — and a run opens with the mixer already on the file's
  gains, before the first sound. `Bus::Voice` and `Bus::Ambience` carry nothing,
  and their rows now say `(silent)` beside the percentage rather than leaving a
  player to wonder whether their audio is broken.

  **`[engine.video] frame_limit` is the first video key on the screen.** A
  `FRAME CAP` row steps through a ladder of ceilings — `menu::FRAME_CAPS`, from
  30 fps up to no ceiling at all — writing the key as it goes, and `RESET` takes
  the cap off along with the faders. It is the one row here that does not apply
  as it moves: the loop takes its frame ceiling once, when `Loop::new` holds the
  game's own `--fps` under the file's, so the row reads `(next start)` until a
  run has actually started under the chosen cap. `menu::is_above` asks
  `FrameLimit::clamped_to` which of two ceilings is higher rather than comparing
  rates, because unlimited is spelled zero and is the largest ceiling there is.

  **It runs in a browser too**, at `/demos/options/`, from the same build that
  runs natively. That half is the point of the sample rather than a bonus: a tab
  has no filesystem, so the settings file goes to the origin's OPFS store, and
  the way that fails is silently. The browser gate drives the whole round trip
  on every push — a fader nudged with the arrows, `SAVE`, `settings.toml` found
  in the OPFS root, the page reloaded onto the gain it saved, and then the store
  wiped and the reload required to come up at unity.

- **Every sample now honours the player's frame-rate ceiling, and no sample had
  to be told to.** `GameGpu` gains `video`, which hands the `[engine.video]`
  section a bundle's `GpuContext` read to the engine's loop, and `Loop::new`
  holds `LoopConfig::limit` under it with `FrameLimit::clamped_to`. So `--fps`
  is what the game asks for and the player's file is the ceiling over it; a file
  that says nothing changes nothing.

  It hands over `VideoSettings` rather than the context because a bundle need
  not have one — the loop's own test fixture opens no device — and because what
  the loop wants is the settings, not the device that read them.
  `GpuContext::video` is the new accessor behind it, beside the narrow
  `video_effects`, `render_scale` and `frame_limit`. `impl_game_gpu!` forwards
  the method, and its `const _` guard makes a bundle that forgets an `E0599`
  naming `video` rather than an infinite recursion.

### Changed

- **`Loop::new` no longer takes `LoopConfig::limit` at face value.** It applies
  the player's `[engine.video] frame_limit` first. A run whose player set no
  ceiling is unaffected; one whose player capped the rate lower now starts
  there.

### Added

- **`[engine.video] frame_limit` is read, and it is the first key whose clamp
  needs the value it is clamping.** `crcbl::settings::frame_limit` answers with
  the ceiling the player's file puts on the frame rate and
  `FrameLimit::clamped_to` applies it to whatever the game asked for, because
  "less" here means less than a runtime value no reader holds.
  `GpuContext::frame_limit(asked)` is the pair a caller uses, beside
  `render_scale` and `video_effects`, and `VideoSettings` grows the field.

  **Zero is unlimited on both sides**, which is `FrameLimit::fps`'s own
  spelling, so a file saying `frame_limit = 0` and a file saying nothing give
  one answer — the ceiling that takes nothing away. That also makes unlimited
  sit at the _top_ of the order while being the smallest `u32`, which is what
  `clamped_to` exists for and what a `min` on the rate would get backwards.
  `set_frame_limit` writes it, `set_video` includes it, and an unlimited ceiling
  is written as a row rather than omitted for the same reason an effect left on
  is. The catalogue row moves from `Named` to `Read`, so `crcbl settings list`
  now marks it `read`.

  Nothing in `apps/` applies it yet: `Loop::new` is where the limit reaches the
  clock and it holds no `GpuContext`, so a game applies it itself for now.

- **The settings catalogue is a list in code, and `crcbl settings list` says
  which of a player's keys anything reads.** `crcbl::settings::catalogue`
  enumerates every key the engine defines with its domain and a `KeyStatus` —
  `Read` where a reader in that module answers it, `Named` where
  `docs/plan/15-windowing.md` fixed the name and nothing reads it yet — and
  `catalogued` is the lookup. The read rows are derived from `VIDEO_KEYS`,
  `RENDER_SCALE_KEY` and `Bus::settings_key` rather than restated, so a key
  cannot be catalogued under one spelling and read under another.

  `crcbl settings list` now carries a `status` per line — `read`, `named`,
  `unknown` or `game` — and names the `engine.` keys the engine does not define,
  in the human output and as an `unknown` array in `--json`. `set` still writes
  any dotted key, because a game's own namespace is not the command's business;
  what changed is that a mistyped `engine.video.shadow` is reported instead of
  parsing, saving and being read by nothing for ever.

- **A setting can now be written, which nothing in the workspace could do.**
  `crcbl-store` shipped the layered stack, `SettingsStack::set` and `save`, but
  `SettingsStack::platform` resolved the platform's storage, read the file and
  dropped it — so a caller holding that stack had nothing to hand `save`, and
  the only writer in the tree was `crcbl settings`. `platform` now has two
  siblings: `SettingsStack::with_platform_storage` lends that storage for the
  length of a call (a borrow, because the browser's OPFS store is an `Rc` and
  cannot be a `Box<dyn StorageSource>`), and `SettingsStack::save_platform`
  writes the user layer back to the same `SETTINGS_FILE`, erroring where the
  platform names no settings directory rather than logging.

  Above it, `crcbl::settings` grows a writer beside each reader — `set_video`,
  `set_video_effects`, `set_render_scale` and `set_audio_gain` — each clamping
  on the way in to the range its reader clamps on the way out, so a slider's
  stored value is the one the next start-up draws at. An effect the player
  leaves on is written as `true` rather than omitted, which the reader treats
  alike but a settings screen cannot: the key it would have to remove to turn an
  effect back on is the one it never wrote. A non-finite scale or gain is
  refused rather than written. `SettingsSource::stack` becomes the public
  `SettingsSource::open`, and `SettingsSource::save` writes a stack back where
  that source read it — `SettingsSource::None` saves nothing and reports so,
  because a golden run must not persist into whichever home directory it ran in.

- **`[engine.video] render_scale`: the player can ask for a smaller internal
  frame.** `ForwardRenderer::set_render_scale` and the Catmull-Rom upscale pass
  have existed since 2026-08-27 with no caller above the renderer. The video
  settings layer now reads a scale beside its four effect booleans:
  `crcbl::settings::video` returns a `VideoSettings { effects, render_scale }`,
  `GpuContext::render_scale` surfaces what the context read while it opened, and
  `apps/viewer` hands it to the renderer at build and again across a reload. An
  absent key is `1.0` — the whole extent and no upscale pass at all — so a run
  with no settings file draws exactly what it drew before. The reader clamps to
  the renderer's own `MIN_RENDER_SCALE..=1.0`, and a value that is not a usable
  number (a string, `nan`, `inf`) warns naming the key and clamps nothing.
  `crcbl::settings::video_effects` is unchanged and still the effects-only read.

- **`crcbl_render::PassStats`: what each pass normally costs, not what one frame
  cost.** The engine's exit log used to print the newest `FrameTimings` the
  timers had resolved — one arbitrary, frames-latent sample, listed once per
  occurrence, which for `lantern` is 53 rows across two views. It now prints a
  `gpu passes` line instead: a p50, a p95 and a share of the p50 total for each
  distinct pass label over the last 120 frames, with a label's occurrences
  summed within the frame and the count shown. A run of
  `lantern --headless --frames 400 --size 1920x1080` reports 18 labels for 0.990
  ms of p50 where the old line reported 53 rows for whatever frame 397 happened
  to cost. The accumulator is public and fed by `Loop::record_frame_cost`, so a
  game driving its own loop can keep one. `FrameTimings::report` is unchanged
  and still the single-frame form.

- **`crcbl_core::stats::Window<N>`: the rolling window both distributions are
  read off.** The eviction, the sort and the count below which a nearest-rank
  p95 is the maximum under another name were private to `crcbl_ui::budget`; they
  are now beside `percentile_of` and `MIN_PERCENTILE_SAMPLES`, which is where
  the rest of that arithmetic already lived. `BudgetStats` and `PassStats` are
  both callers.

- **`crcbl::settings::audio_gains`: the `[engine.audio]` bus volumes a player
  can set.** One key per bus — `master_volume`, `music_volume`, `sfx_volume`,
  `ui_volume`, `voice_volume`, `ambience_volume` — each a linear gain clamped to
  `[0, 1]`, with an absent key meaning unity, and a value that is not a usable
  number (a string, `nan`, `inf`) warning naming the key and leaving the bus
  where the game set it. `SettingsSource::audio_gains` is the seam a game reads
  them through, and it is public where the video layer's is not, because a
  `GpuContext` owns the renderer and nothing in the engine owns a mixer.
  `SettingsSource::apply_audio_gains` hands them to a mixer bus by bus, and
  `SettingsSource::for_run(headless)` picks the source — `None` headless, so a
  golden run takes nothing from whichever home directory it executes in. All
  four samples that own a mixer read them in `Audio::new`, before the first cue:
  `apps/asteroids`, `apps/breakout`, `apps/flappy` and `apps/horde`.

- **`crcbl_audio::mixer::Bus`: six fixed gain stages, so a player can turn the
  music down without turning the gunfire down.** `master`, `music`, `sfx`, `ui`,
  `voice` and `ambience`, each one linear gain reachable through
  `Mixer::set_bus_gain`. A voice is routed at spawn with `Voice::with_bus` and
  defaults to `Bus::Sfx`, so every existing caller keeps sounding as it did. The
  final gain is `sample × voice_gain × bus_gain × master_gain`, left to right —
  contract rather than detail, because a regrouping moves every sample of every
  golden buffer. Nothing reads the `[engine.audio]` keys yet: the gains are
  reachable from code, not from a settings file.

- **An ambient-occlusion debug view.**
  `crcbl_render::ForwardRenderer::set_occlusion_view` draws the blurred
  occlusion channel as grey in place of the shaded picture, on the sentinel lane
  the normals, LOD-tint and heatmap views already share — and outermost on it,
  so it wins over all three. `lantern`'s pause panel gains an `AO VIEW` row for
  it, which is **not** the `AO` row above it: that one turns the occlusion pass
  off, this one changes which picture is drawn. On a frame with the pass off the
  view is white everywhere, because that is the 1×1 image the renderer binds in
  place of a computed channel.

- **`crcbl_shaders::volumetric`: the arithmetic a froxel volumetric pass runs
  on.** `phase` is Henyey-Greenstein — the angular half, which is what makes fog
  glow around a light rather than uniformly — and `integrate_slice` is what one
  slice of a froxel column owes the composite: the radiance it adds and the
  fraction of what is behind it that gets through, in one closed form.

  Neither reaches for a transcendental, per the shading rule in
  `docs/plan/44-lighting.md`. The exponential is `crcbl_shaders::fog::exp_neg`,
  and the phase function's three-halves power is written `d * sqrt(d)`, because
  IEEE-754 requires a correctly rounded `sqrt` and specifies nothing about
  `pow`.

  The slice integral carries the self-attenuation term, so cutting a column into
  more slices does not brighten it — 1, 2, 7, 64 and 512 slices of a homogeneous
  column all composite to the same radiance. `fog::SERIES_CUTOFF` and
  `fog::one_minus_exp_over` are public now, the latter being the shape of every
  homogeneous-medium integral rather than only height fog's.

- **Volumetric fog: the froxel column, behind `RenderEffects::VOLUMETRIC_FOG`.**
  Three passes over the subdivision `light_cluster.slang` already builds —
  `volumetric.slang` writes what each slab of air scatters and transmits, scans
  each column into an exclusive prefix, and `volumetric_composite.slang`
  integrates the last partial slice along the pixel's own ray and composites
  over the frame. A storage buffer rather than a 3D texture, for the reasons
  `docs/plan/51-volumetrics.md` gives.

  **Off by default**, and it is not in `RenderEffects::DEFAULT_STACK`: the
  medium it integrates is the same exponential height fog `mesh.slang`
  composites analytically, so the frame block's fog density is zeroed on a frame
  that runs the froxel path and the air is charged once. A frame with
  `Fog::NONE` draws identically either way, byte for byte.
  `[engine.video] volumetric_fog` is the settings key.

  **The sun scatters into it**, through the Henyey-Greenstein lobe
  `crcbl_shaders::volumetric::phase` pins: `Fog` gained `sun_scattering` — what
  fraction of the sun's radiance the medium sends out per unit length — and
  `anisotropy`, how forward that scattering leans. Both default to zero, so a
  caller that sets neither gets exactly the column that shipped without them.

  **And the cascades occlude it.** The scatter pass looks each froxel's midpoint
  up in the sun's shadow atlas and scales the sun term of its source by what
  comes back, so the air behind an occluder keeps its ambient glow and loses the
  beam — a shaft rather than a dimmer. The environment term is untouched by it,
  which is what stops a shadowed column going to black.

  The lookup is `mesh.slang`'s, copied once into the scatter pass and held to
  the original by a drift guard; the atlas and its comparison sampler are bound
  to a compute stage, which is new. Neither of that file's receiver biases came
  with it and neither did its `n_dot_l` early return: all three are about a
  facet, and a froxel is a volume of air.

- **The sky is drawn behind the frame.** The gradient already lit the scene and
  was what a missed reflection fell back to, but nothing put it on screen — a
  frame lit by a bright sky still had `crcbl_render::SCENE_CLEAR` behind it.
  `sky.slang` and `crcbl_render::sky_pass` now draw it, between the forward pass
  and everything that reads the scene colour, so the reflection composite, the
  bloom chain and the tonemap all work on a frame that has a sky in it.

  It is **depth-tested rather than masked**: the full-screen triangle is emitted
  at the reversed-Z far plane and the pipeline compares `GreaterOrEqual` with
  depth writes off, so the hardware that rejected the hidden fragments is what
  selects the background. The pass binds no depth texture, no sampler, and has
  no `discard` in it.

  `crcbl_render::SCENE_CLEAR` is now re-exported from the crate root. A frame
  whose sky is `Sky::NONE` adds no pass at all, so this landed without moving a
  golden.

- **A missed reflection falls back to the sky.** A screen-space ray that leaves
  the frame or finds no geometry took the irradiance probes' environment and
  nothing else, so a scene with an empty probe volume reflected black. It now
  adds the gradient sky along the reflected ray. Three `float4` rows at the end
  of `ssr::SsrParams`, which grows from 256 to 304 bytes.

  It carries the **gradient** rather than the L1 projection
  `mesh::FrameUniforms` holds, which is the one place the two blocks
  deliberately disagree about how to hold one sky: an ambient term wants the
  environment's cosine-weighted integral and L1 is that integral, while a
  reflection wants the radiance along one direction, and rebuilding that from
  four irradiance coefficients would blur a gradient the pass can evaluate
  exactly.

  `Sky::NONE` writes three zero rows and adds nothing, so this rung landed
  switched off like the ambient one and no golden moved.

- **A gradient sky lights the scene.** `ForwardRenderer::set_sky` takes a
  `crcbl_render::Sky` — three linear-RGB radiances for zenith, horizon and
  ground — and `mesh.slang` adds the irradiance that gradient delivers to the
  ambient term, so a surface facing up is lit by the sky and one facing down by
  the ground's bounce. No new pass and no new binding: three `float4` rows at
  the end of `mesh::FrameUniforms`, which grows from 1264 to 1312 bytes. A host
  that writes that block itself has three rows to add; every existing member
  keeps its offset.

  It is **added** to the flat ambient and to the irradiance grid rather than
  chosen between them — all three are the same term, what a diffuse surface
  receives from an environment it is not looking at. An author who wants the sky
  to be the whole of it sets `DirectionalLight::ambient` to zero.

  **`Sky::NONE` is the default and is exactly off.** A black gradient projects
  to coefficients that are all zero, so the fragment stage adds `max(0, 0)` and
  every golden blessed before a sky existed still matches.

  Known gap: **nothing draws the sky.** The background is still the scene
  target's clear colour, and the environment `ssr.slang` falls back to on a
  missed ray is still the probe grid's — see `docs/plan/43-render-standards.md`
  §8.

- **A gradient sky, and the environment it lights with.**
  `crcbl_shaders::sky::SkyGradient` is three linear-RGB radiances — zenith,
  horizon, ground — blended by a smoothstep in a direction's `y`. `radiance`
  gives what a ray leaving the scene sees; `irradiance` gives the same field
  projected onto the L1 spherical-harmonic basis this engine's probes already
  speak, as a `crcbl_shaders::probe::GpuProbe`, so a sky's ambient contribution
  and a probe volume's add through one convention rather than two.

  The projection is a closed form rather than a quadrature: the gradient is a
  function of `y` alone, so the sphere integral collapses to two moments of the
  blend, and the `x` and `z` bands are zero by symmetry.

  `SkyGradient::BLACK` projects to `GpuProbe::ZERO` exactly, which is what will
  let the sky arrive switched off. Nothing renders it yet —
  `docs/plan/43-render-standards.md` §8 is what this is the first half of.

- **Exponential height fog.** `ForwardRenderer::set_fog` takes a
  `crcbl_render::Fog` — density, falloff, reference height and a scattered
  radiance — and `mesh.slang` composites it over every shaded surface, so
  distance reads as distance and a valley fills while a hilltop stays clear. No
  new pass and no new binding: two `float4` rows at the end of
  `mesh::FrameUniforms`, which grows from 1232 to 1264 bytes. A host that writes
  that block itself has two rows to add; every existing member keeps its offset.

  **`Fog::NONE` is the default and is exactly off.** The transmittance at zero
  density is exactly one and the shader composites as `lit * t + fog * (1 - t)`
  rather than as a `lerp`, so a caller who never asks for fog gets the frame
  this engine drew before it existed, bit for bit.

  Known gap: the screen-space reflections `ssr_blur.slang` adds after this pass
  arrive unfogged onto a fogged surface — `docs/backlog.md` carries it.

- **An exponential the shading rule allows.** `crcbl_shaders::fog` computes
  `e^-x`, the exponential-height-fog optical depth along a ray and the
  transmittance it implies, without calling a transcendental function —
  `exp_neg`, `optical_depth` and `transmittance`, plus the constants the Slang
  mirror will spell. This is what unblocks height fog, which
  `docs/plan/43-render-standards.md` §4 had held as a decision rather than a
  slice: the workspace's rule is that no transcendental may reach a colour, and
  the construction here is range reduction, a Taylor kernel over the reciprocal
  factorials and an exponent field written directly, all of it operations
  IEEE-754 pins down. Measured within two units in the last place of `f64::exp`
  over its whole domain, and the closed-form integral is checked against Simpson
  quadrature.

  Nothing renders fog yet — the frame uniforms, the shader mirror and the
  composite are the next slice.

- **Every instance now says where it was last frame.**
  `crcbl_shaders::mesh::GpuInstance` carries `previous_transform` beside
  `transform` and `INSTANCE_STRIDE` grows from 96 to 160 bytes — a **format
  change**: `mesh.slang`, `mesh_cluster.slang`, `cull.slang` and
  `draw_gen.slang` all declare the wider record, so a host that writes the
  instance buffer itself has to move every field past the transform.

  `crcbl_render::InstancePool` fills the field, and callers do not pass it — the
  pool owns it the way it owns the liveness bit. `set` carries the transform the
  record already held, `insert` starts at rest because a spawn did not travel
  from anywhere, and `rotate` puts back at rest whatever moved a frame ago and
  has not moved again, so an instance that stops moving stops reporting a move
  instead of repeating it forever. Standing still costs one upload per buffer of
  the ring and nothing after that.

  **Nothing reads it yet and no frame moved.** It is here because
  `docs/plan/43-render-standards.md` §9 asks for it before its readers arrive:
  temporal antialiasing, temporal reflections, temporal upscaling, per-object
  motion blur and SSGI accumulation all want a motion vector, and widening the
  record is cheap while four shader copies declare it and expensive once more
  shaders index past the stride. What those five still owe is the pass — a
  motion-vector target, the subtraction, and the previous frame's
  view-projection in the frame block.

- **The split-sum `DFG` table, and the energy a GGX lobe loses without it.**
  `crcbl_shaders::dfg` is a committed 64-square table over `(N·V, roughness)` —
  `crates/crcbl-shaders/tables/dfg.bin` — with `directional_albedo` and
  `energy_compensation` over it. A single-scatter GGX lobe drops every
  microfacet bounce after the first, and the table measures the shortfall: a
  head-on surface hands back all of the light when polished and **0.317 of it
  when fully rough**, so a rough conductor renders short by more than two thirds
  until the factor puts it back.

  **Every rough conductor now shades with it.** `mesh.slang` binds the table as
  an `Rg8Unorm` image, filters it in the shader at `(N·V, roughness)` and
  multiplies the summed specular lobe by `1 + f0 (1 / E - 1)`. Two of
  `apps/lantern`'s goldens moved and were re-blessed; the frame is net brighter
  and the metal ball is where the change is visible. The diffuse term is
  untouched, because the light the lobe dropped went on bouncing inside the
  microsurface and left it as specular.

  **A polished surface is unchanged, exactly.** `E` is one where roughness is
  small, so the factor is one and every dielectric in the tree — which is most
  of it — draws the frame it drew before. The albedo is clamped to one before it
  is inverted, so the table's Monte Carlo noise cannot turn the term into a
  dimming.

  **Filtered in the shader rather than by a sampler**: four `Load`s and the
  bilinear blend written out. A hardware filter's weights are computed
  independently by four rasterisers and these goldens are compared with no
  tolerance, where multiplies and adds agree everywhere. The image is two bytes
  of fixed point per texel rather than the `Rg16Float` a BRDF table usually
  takes — the value is a share of arriving light, so `1 / 65535` over `[0, 1]`
  is finer than half precision is near one.

  **Baked once and committed, not computed.** The integrator importance-samples
  the lobe, which takes a `sin` and a `cos` per sample, and four platforms'
  `libm` disagree in the last place — so a table each machine computed would be
  four tables under goldens compared with no tolerance. `cook-dfg` regenerates
  it and `cook-dfg --check` holds it to its integrator in CI, on the same terms
  as `cook-clusters` and the committed cluster DAG.

- **Render scale: the frame can be drawn at fewer pixels than the window.**
  `crcbl_render::ForwardRenderer::set_render_scale` sizes an internal colour
  target at a fraction of the caller's extent — down to `MIN_RENDER_SCALE`, a
  quarter in each dimension — and `crates/crcbl-shaders/shaders/upscale.slang`
  reconstructs it into the caller's own target as the last pass of the frame.
  The cluster grid, the level-of-detail pixel budget, the Hi-Z pyramid, bloom
  and FXAA all follow the internal extent, so the post chain finally costs what
  `docs/plan/48-post-processing.md` has been ordering it to cost.

  **At full scale nothing changes**: the stage before it writes the caller's
  target directly, so there is no second image and no pass, and a frame that
  asked for no scaling is bit-identical to one from before this existed. Not one
  golden moved.

  **The filter is Catmull-Rom**, sixteen taps — Mitchell-Netravali at
  `B = 0, C = 0.5`, so a surviving texel reaches the frame unchanged and the
  weights are a partition of unity by exact identity. Multiplies and adds only,
  no transcendental near a colour, which is what lets it be blessed on all four
  backends. It is spatial, not temporal: no jitter, no history, no motion
  vectors, so swapping it for an FSR-class upscaler later replaces the pass
  without moving the seam around it.

  **Nothing above the renderer can ask for it yet** — no settings key and no
  `Shell` request; see `docs/backlog.md`.

- **Materials emit light.** `crcbl_shaders::mesh::GpuMaterial` grew an
  `emissive` triple — a linear radiance added to the shaded colour and scaled by
  nothing, so a lamp reads the same in a corner as in the open and a black
  emissive surface is still black. `shaders/mesh.slang` adds it last, unclamped,
  so a value above one reaches the `Rgba16Float` scene target and the bloom
  chain turns it into a glow.

  **It cost no stride and moved no offset.** The three words it occupies were
  the row's `pad0`/`pad1`/`pad2`, so `MATERIAL_STRIDE` is still 48 and every
  earlier member is where it was — verified against the offsets `slangc`
  decorates the struct with. `[0.0; 3]` is the default, zero added to a colour
  is that colour exactly, and not one golden in the tree moved.

  **glTF import fills it.** `emissiveFactor` is capped at one per channel, so
  the `KHR_materials_emissive_strength` feature is now enabled on the `gltf`
  dependency and the importer stores the product — without it every emitter
  above white would load silently dimmed.

- **The tonemap has a filmic curve, and it is off by default.**
  `crates/crcbl-shaders/shaders/tonemap.slang` carries a second operator behind
  a `uint curve` lane of its uniform block: Stephen Hill's fit of the ACES RRT
  and ODT, two changes of primaries around a rational polynomial.
  `ForwardRenderer::set_tonemap_curve` and `ForwardRenderer::tonemap_curve` are
  how a view asks for it and reads back what it got, and
  `crcbl_shaders::tonemap::TonemapCurve` is the selector both sides share.

  **Nothing in the tree looks different yet**, deliberately. Exposure-and-clamp
  stays the default because it is the identity on `0..=1`, so every 2D sample —
  which is display-referred already — reaches the swapchain exactly as it did.
  Flipping which stacks default to the curve is a separate change whose whole
  content is the re-bless, the shape the FXAA resolve landed in.

  **ACES rather than AgX**, and the reason is this workspace's goldens: AgX
  needs a `log2` and a `pow` per channel, four platforms' transcendental
  functions differ in the last place, and a curve that cannot be blessed on all
  four backends is not one this engine can ship. The fit uses no transcendental
  at all. `TonemapCurve::apply` is the same arithmetic on the CPU, checked
  against the ODT's published anchors rather than against itself — a neutral
  stays neutral because both matrices' rows sum to one, and a scene-referred
  0.18 lands near a tenth of display range — and a source grep holds the shader
  to the same constants so the two copies cannot drift.

- **Screen-space reflections march a Hi-Z pyramid instead of a fixed stride.**
  `crates/crcbl-shaders/shaders/hiz.slang` and `crates/crcbl-render/src/hiz.rs`
  reduce the depth prepass to up to five levels, each texel the nearest surface
  of the 2×2 block below it, and `shaders/ssr.slang` climbs that structure: a
  ray over open space crosses a 32-texel cell in one iteration and walks back
  down to a single texel only where something is in its way. The reflection is
  resolved to a texel rather than to a 1.5-pixel stride, which is what removed
  the stepping the old walk left on grazing rays.

  **The frame grows a `hiz-N` pass per level**, between `forward` and `ssr`, and
  only when `RenderEffects::REFLECTIONS` is on. The count depends on the extent
  — the frame halved until a level would fall under eight texels — so
  `crcbl_render::hiz::levels_for` and `level_extent` are public for anyone
  budgeting passes or GPU timer slots. A frame too small to halve once gets no
  pyramid at all and the march walks the prepass at full resolution, so the pass
  is an optimisation the reflection can be built without.

  The pyramid is `D32Float` written through `SV_Depth`, not an `R32Float` colour
  target: it keeps every level the same texture type as the prepass, so the
  march reads all six bindings through one `DepthTexture2D` and the seam needs
  no third sample type for a format WebGPU cannot filter.

  **It also took out a NaN the old walk was hiding.** A ray clipped exactly to
  the vanishing point solves for an infinite ray parameter, and the depth of
  every point along that segment is then `inf * 0` — so those rays found no
  crossing and silently fell back to the probe environment. The fixed-stride
  walk avoided it by accident, because flooring the reach to a whole number of
  strides never landed on the point itself; a march that clips to its own cell
  boundaries does land there. `ssr.slang` now stops a pixel short of the
  vanishing point and rejects a non-finite parameter by name.

- **Antialiasing: FXAA 3.11 over the tonemapped frame.**
  `docs/plan/18-render-features.md`'s AA slot had been a contract for a pass
  that did not exist; `crates/crcbl-shaders/shaders/fxaa.slang` and
  `crates/crcbl-render/src/fxaa.rs` are that pass. One fullscreen resolve — a
  luma edge detect and a subpixel blend along the edge it finds — with no
  history, no motion vectors and no change to any pass in front of it.

  **It is `RenderEffects::ANTIALIASING`, it is in
  `RenderEffects::DEFAULT_STACK`, and every frame the engine draws is
  resolved.** That cost a re-bless of the whole suite — twenty-five golden
  images across `crates/crcbl`, `apps/quarry` and `apps/lantern` — which is what
  a resolve does: it changes every edge it runs over. Bloom is now the only
  effect a view has to ask for by name. A caller turns the resolve off through
  the camera stack, through `EffectRequest`'s override, or through the
  `[engine.video] antialiasing` key `crcbl`'s settings stack already reads.
  `crcbl screenshot --scene aa` draws the fixture.

  **A debug view takes it off by itself.** `DebugView::Heatmap`, `LodTint` and
  `Normals` are readouts rather than pictures — a pixel's colour is a cluster's
  projected error or its DAG level, read against a legend — so
  `ForwardRenderer::resolved_effects` drops the bit whenever one of them is on,
  and a blended shade cannot invent a ramp position no cluster occupies.

  **Switching it on changes the shape of the frame**, which is worth knowing if
  you read the graph: with the bit off the tonemap writes the caller's target,
  and with it on the tonemap writes a `display-color` transient at the target's
  own format and the resolve writes the target. The ground grid moves with the
  tonemap so its thin lines are filtered; the UI composites onto the resolved
  target afterwards, so glyphs are not.

  **A new golden fixture, `Scene::Aa`** — one slab turned about the view axis so
  its silhouette runs diagonally between two flat levels — and, because no
  golden can tell an antialiased edge from a clean one, a test that draws that
  scene twice and compares: 532 pixels between the two levels with the resolve,
  zero without it, and a mean level that moves by 0.24 out of 255. It is in the
  browser parity gate too, where it matches its golden within a channel delta
  of 2.

- **`apps/shard`, the first slice of the action-RPG sample, on the site.**
  `docs/plan/sample/15-shard.md`'s milestone 1 exists to put _content_ through
  the render paths a browser is stuck with, and this is that content: one
  torch-lit interior zone, assembled from `crcbl::greybox` primitives off a
  single ASCII table (`zone::LAYOUT`) that the meshes and the colliders are both
  read from, walked on the same `crcbl::phys::CharacterController` `apps/puppet`
  and `apps/breach` drive. It builds for `wasm32` and ships at `/demos/shard/`,
  which gives the Pages site a 3D sample that is a game rather than a fixture.

  **The lighting is the point.** Braziers carry point lights that flicker on the
  _simulated_ clock, the shrine carries a spot whose cone falls across the
  corridor's doorposts, there are more lights than the renderer's shadow slots
  so it ranks them and shadows the two that win, and the ambient term is a baked
  irradiance volume gathered by casting rays into the same colliders the capsule
  sweeps against — so a torch behind a pillar does not reach the probe on the
  far side of it. `L` puts the torches out. None of those features gained a line
  on this sample's behalf; the sample is the load on them.

  **A third camera rig on one controller** (`shard::camera`): a fixed isometric
  elevation at a distance far shorter than the genre's, with a yaw that moves in
  quarter turns on `Q`/`E`. puppet orbits behind the shoulder and breach sits
  inside the head; `crcbl-phys` gained nothing for any of the three.

  Rule 12 in full: the `[HUD]` heartbeat, the debug panel and the summary line
  all name the `GeometryPath`, `BindingModel` and `LightingPath` the frames were
  actually drawn through, and the browser gate asserts they are
  `IndirectPerBatch`, `ArrayPages` and `Rasterised` — the fallback arms, which a
  browser resolves to by construction.

  **Three archetypes of foe, an ability each, and a blow that answers them**
  (`shard::foe`). A **husk** closes and jabs; an **adept** holds a stand-off
  band and throws down its own sighting line, walking _backwards_ when the
  character gets inside it; a **warden** is slow, takes five blows, and stands
  still for a telegraphed wind-up before its slam lands, so stepping out of
  reach in that window is a slam that misses. Each walks through the same
  `crcbl::phys::CharacterController` the character does, and each holds an
  authored post (`foe::POSTS`) until it can actually _see_ the character — the
  sighting is one `crcbl::phys::PhysicsWorld::cast_ray` through the zone's own
  colliders, so a doorpost between the two is a foe that has not noticed you.
  Unlike `apps/breach`'s practice bots there is no patrol and no respawn: a zone
  is cleared rather than worked, and a foe that is felled stays down as a
  non-solid body a later blow passes through.

  `Space` swings the character's cleave: everything within reach that has a
  clear line takes damage, resolved through the same `cast_ray` the sighting
  uses, so stone between the two stops a blow exactly as it stops a sighting.
  **Both sides have health and both can die** — the character who runs out is
  returned to the spawn with one more down against their name. The `[HUD]`
  heartbeat, the debug panel and the overlay carry the numbers: foes standing,
  foes engaged, health, downs, blows swung against blows landed, damage dealt
  against damage taken, and what the cleave would answer.
  `web/tools/browser-e2e.mjs` reads three positive/control pairs off that line
  in a real browser — a foe that engages when the character comes at it against
  one that had engaged nothing on every beat before, a blow that fells a foe
  against one swung with nothing in reach, and an ability that costs health
  against a character who had taken nothing.

  **A character that persists between sessions** (`shard::save`). Where they are
  standing, what they have left, how many times they have been put down and how
  much health each foe has — which is what says who is felled — written through
  `crcbl-store`'s existing save container: a header, one sector at
  `SectorId::ZERO`, and a SHA-256 over everything before it. Natively that goes
  to the platform **data** directory (`~/.local/share/shard/character.crb` on
  Linux); in a browser it goes to the Origin Private File System through the
  same shim `web/engine/storage.js` already drove for high scores. Nothing in
  `crcbl-store` changed on the sample's behalf, and a `--headless` run opens no
  vault at all, so the test suite and CI leave nothing behind.

  It autosaves once per second of **simulated** time, so a machine drawing this
  zone at a fifth of real time saves exactly as often per second of play; the
  next session resumes from it, and the `[HUD]` heartbeat, the debug panel and
  the summary line all say whether this one did (`resumed:`) and how many writes
  it has made (`saves:`). A save this build will not stand behind — wrong
  length, foreign magic, an unknown payload version, a roster that is not this
  zone's, a health above an archetype's own ceiling, a position that is not a
  finite number — reads as _no save_ and the zone opens fresh rather than being
  clamped into something plausible. `web/tools/browser-e2e.mjs` reads the bytes
  back out of OPFS in a real browser, reloads the page and requires the
  character to come back where the save's own heartbeat reported them, then
  clears the store and requires the same page to come up on the spawn with the
  zone intact.

  **Four verbs of six.** Milestone 1's loop is explore, fight, loot, level,
  save, resume; exploring, fighting, saving and resuming are here. No item, no
  rarity, no experience, no inventory grid — and **no reserved field for one**
  in the save, deliberately: who forces `docs/plan/34-inventory.md`'s kit is an
  open question in `docs/backlog.md`, and a reserved field would answer it by
  accident. No sector streaming and no networking. `docs/backlog.md` carries
  what each of those needs.

- **`apps/breach`'s bot practice map, and a `--map` flag to choose it.**
  `docs/plan/sample/11-breach.md`'s milestone 0 is a firing range **and** a bot
  practice map; this is the second half. A second greybox room — a pillar tall
  enough to break a sightline, two crates and a patrol circuit around them —
  with three bots in it. Each walks an **authored waypoint route**
  (`map::practice::ROUTES`) through the same `crcbl::phys::CharacterController`
  the player walks through, notices the player through the same
  `crcbl::phys::PhysicsWorld::cast_ray` the pistol resolves with, and shoots
  back on a fixed cadence with that same pistol. The player has health and
  respawns; a bot the player shoots goes down and comes back on its route.
  **There is no navmesh and no pathfinding** — `docs/plan/24-navigation.md` is a
  later subsystem whose own forcing function is a different sample, and
  `apps/breach` deliberately does not force it.

  Cover is the whole subject: a bot sees the player only when nothing is between
  their eyes, so a bot crossing the far side of its circuit loses the player
  behind the pillar and picks them up again at either end — and the rounds it
  fires in between go into the pillar rather than into the player. The `[HUD]`
  heartbeat and the debug panel carry that as numbers: which map, how many bots
  are alive, how many have the player in sight, how many are in range and
  covered, the player's health and respawn count, and the bots' shots against
  the ones that arrived.

  `--map range|practice` chooses the map on a command line, `?map=practice`
  chooses it on the demo page (through a new `__crcbl_breach_map` wasm export),
  and `/demos/breach/` still opens the firing range. `web/tools/browser-e2e.mjs`
  drives the second map in a real browser: it navigates to `?map=practice` and
  reads three positive/control pairs off the heartbeat — a patrol that walks
  (and it is a bot's position, not the player's and not the other map's
  travelling plate), a sighting that happens (and one that cover stops), and a
  round that costs health (and one that never arrives).

- **`apps/breach`, a first-person firing range, on the demo site.**
  `docs/plan/sample/11-breach.md`'s milestone 0, first slice: a greybox indoor
  range — floor, four walls, a ceiling, a firing line and three lanes with a
  target plate each, one of which travels — walked with `W/A/S/D`, looked around
  with the mouse or the arrows, and shot with one hitscan pistol on `SPACE`. A
  trigger pull is a `crcbl::phys::PhysicsWorld::cast_ray` from the eye along the
  view; a ray that lands on a standing plate scores and knocks it down, and the
  plate stands back up after a fixed delay. The overlay carries a crosshair that
  lights up on a target, the score and each lane's state; the debug panel and
  the `[HUD]` heartbeat add the position, the angle being aimed along and — rule
  12 — the `GeometryPath`, `BindingModel` and `LightingPath` the frames were
  drawn through, which in a browser are the fallbacks by construction.

  **What a reader gets out of it is the claim `apps/puppet` could only assert.**
  `crcbl::phys::CharacterController` takes a world-space displacement and holds
  no camera; puppet drives it from a third-person orbit camera and breach drives
  the _same_ controller from a first-person one that shares none of puppet's
  code — down to measuring its yaw the other way round — and `crcbl-phys` gained
  nothing for either of them. The firing line is the same argument in miniature:
  it is a kerb over the controller's own `step_offset`, so the player is kept
  behind it by the controller refusing to climb it rather than by a rule in the
  game code.

  Nobody has to press anything to see it work: with no input the range runs
  itself, sweeping onto each lane and taking the plate down, and the first key
  or trigger pull hands the controls over — squaring the shooter up down the
  near lane and resetting the score, so a visitor's first string starts from a
  known pose rather than from whatever bearing the demonstration was swinging
  through. The far lane's plate travels its lane on a timer either way.

  There is no mouselook in the browser and that is deliberate: the web shell
  reports no `RAW_POINTER_MOTION`, so the engine declines the pointer lock
  rather than telling a first-person camera it has aim input it can trust — the
  first of the four reasons that sample's competitive milestones are native
  only. Milestone 0 has no weapon but the pistol, no armour, no ballistics and
  no networking beyond the in-memory loopback every sample has;
  `docs/backlog.md` carries the rest.

- **`crcbl-vfx`: the first slice of the particle system.** A `ParticleSystem`
  holds a structure-of-arrays `ParticlePool` and hands each effect a contiguous
  `SlotRange` out of it, so an effect's live particles are one run of records —
  the layout the compute pass this is staged towards will upload as it stands.
  Randomness is a **stateless hash of (effect seed, particle index, stream)**
  rather than a stream: `hash::pcg3d` is the three-word PCG hash from Jarzynski
  and Olano's _Hash Functions for GPU Rendering_, so particle _k_ draws the same
  lifetime, direction and size whatever else ran in the frame, and a fixed seed
  with a fixed timestep replays a run bit for bit on one machine. The modifier
  menu is a `Modifiers` struct — gravity, drag, a size `Curve` and a colour
  `Gradient` over lifetime — deliberately not a list, because
  `docs/plan/20-particles.md` names modifier creep toward a VM as the risk.
  Emitters are `Spawn::Burst` and `Spawn::Rate` over `Shape::Point` and
  `Shape::Cone`. **Budgets are enforced rather than advertised**: an effect's
  `max_particles` is the size of its range, `RangeAllocator::alloc_clamped`
  clamps a share the pool cannot fit instead of refusing it, and `EffectStats`
  and `PoolStats` count every spawn the budget turned down. Reached through the
  umbrella as `crcbl::vfx`, behind the new non-default `vfx` feature.
- **`apps/sparks`, the VFX fixture, on the demo site.** A greybox stage with
  three effects on it: impact sparks off an anvil, a smoke puff that stops and
  starts at a vent, and a deliberately hostile emitter asking for a hundred
  thousand particles a second out of a share of sixty-four. Every particle is
  drawn as an **ordinary instance** through `ForwardRenderer::set_instance`, so
  it rides the existing GPU culling and draw generation and the sample adds no
  pass and no shader of its own — `docs/plan/20-particles.md`'s mesh particles.
  The overlay carries the budget readout `docs/plan/sample/10-sparks.md` asks
  for: what each effect holds against what it was allowed, and how many spawns
  were refused. It takes no input and runs from a published seed.
- **`crcbl-anim` gained a blend layer**: `blend_into` mixes two `Pose`s by
  weight into a caller-owned third, and `BlendSpace1d` places clips along one
  axis and samples the pair around a position at a shared **phase** rather than
  a shared time, so clips of different lengths stay in step. Rotations take the
  shorter arc, and both ends of a blend are copied through exactly rather than
  interpolated toward, so a blend space sitting on a stop plays that clip
  untouched.
- **`apps/puppet` walks a skinned character.** `puppet::rig` authors a greybox
  humanoid in code — nine joints, five boxes each bound across two of them, an
  idle stance and a one-second stride — with no asset on disk and no glTF parse;
  `puppet::anim` blends the set on the character's **measured** ground speed and
  composes the palette, and `puppet::gpu` draws it through the engine's skinning
  dispatch. `game::RenderState` and `game::Stats` gained `speed`, the overlay
  gained `SPEED` and `BLEND` rows, and the demo logs a second heartbeat,
  `[POSE]`, on the client's own clock. `map::Character` is no longer `Copy` and
  `map::place` now returns `map::PlaceError`; `map::BODY_MESH` is replaced by
  `map::CHARACTER_MESH_BASE`.
- **`apps/viewer`'s panel says whether the camera has been taken hold of.** A
  `held` row and heartbeat field, reading `on` once a drag or the wheel has
  stopped the idle turntable for good. It exists to tell two failures apart that
  look identical from outside — a page whose loop died, and a camera something
  handed over — which is a distinction the browser gate could not make on the
  macOS runner.

- **`apps/breach`'s primary mouse button pulls the trigger**, on any shell that
  actually grants the pointer lock. It is gated in `pointer_event` rather than
  bound through `ActionMap`, because a map binding is not told where the pointer
  is and so cannot tell a shot at the crosshair from the click that asks for the
  lock in the first place. What says "captured" is a pointer _motion_ carrying
  no absolute position, which is a shape only a held lock produces — not
  `at.is_none()` on its own, which is also what a click from a mouse that has
  not moved reports, and which would have fired on the commonest click there is.
  The keyboard trigger and the arrow-key look stay, and are what the sample
  plays with wherever the lock is declined.

- **Mouse look in a browser.** `crcbl-shell`'s web backend reports
  `ShellCaps::POINTER_LOCK | RAW_POINTER_MOTION`, so `ShellCaps::has_mouselook`
  is true there and the engine stops downgrading a requested
  `PointerMode::Locked` to `Free` on every browser build.
  `WebShell::set_pointer_mode` publishes the request through the new
  `__crcbl_web_pointer_lock_wanted` export; `web/engine/shell.js` polls it once
  a frame and waits for a `pointerdown` from a **mouse** — a browser grants
  `requestPointerLock` only from a user gesture, wasm is never in one, and a
  finger has no cursor to pin — then reports the outcome back through the new
  `__crcbl_web_pointer_lock` entry point. While that says locked,
  `ShellEvent::PointerMotion` carries `raw_delta` and no `abs`, and `Button` and
  `Wheel` carry no position, exactly as the Wayland and X11 backends do.

  `__crcbl_web_pointer_motion` takes two more arguments, the `movementX` /
  `movementY` delta scaled to device pixels; a page with its own copy of the
  shim must pass them.

  The lock is asked for with `unadjustedMovement: true`, which is the OS
  acceleration bypass `RAW_POINTER_MOTION` names — implemented by Chrome and
  Edge from 88, Chrome for Android from 151, Firefox from 152 and Safari from
  18.4. Where a browser declines it the shim retries without it and the deltas
  are the OS-adjusted ones; the capability's docs and `docs/backlog.md` both say
  so rather than claiming more than the code delivers.

### Changed

- **The shadow filter takes its 32 taps only at a shadow edge.** `tile_pcf`, in
  both `mesh.slang` and `volumetric.slang`, now probes five of the disc's taps
  first — one near the centre and four about a quarter turn apart near the rim —
  and returns a flat lit or shadowed the moment those five agree. A fragment
  away from any shadow edge costs 5 taps rather than 32, a sun-lit one 21 rather
  than 48, and a froxel 5 rather than 32; a fragment the probe cannot decide
  costs 37 and is shaded exactly as it was before. Every golden in the tree
  still matches, on both a discrete GPU and a software rasteriser — the probe
  and the disc agree everywhere the shipped scenes look.

- **Ambient occlusion is GTAO, a horizon integral, not a hemisphere of depth
  comparisons.** `ssao.slang`'s `occlusion_at` takes two slices through the eye
  per pixel — the second the first turned an exact quarter turn — marches four
  pixel-steps along each side of each to find the highest horizon, and evaluates
  the closed-form slice integral over the two angles. Sixteen depth taps where
  there were eight. The pass, its `R8Unorm` target, the binding,
  `ssao_blur.slang` and the structural-ratio test are unchanged.

  Every angle comes through `crcbl_shaders::ssao::acos_approx`, which is
  Abramowitz and Stegun 4.4.45 and a `sqrt` rather than the target's `acos`: no
  graphics API specifies an accuracy for that intrinsic, and two rasterisers
  disagreeing about it is the driver divergence this technique was chosen to
  avoid. It is swept against `f64::acos` to `MAX_ACOS_ERROR`, and `ssao.slang`'s
  copy of the coefficients is compared as values.

  What it buys, in the goldens: the hemisphere had been laying a soft wash over
  flat surfaces that face the camera, and every one of the five frames that
  moved moved by losing one. `probes`' floor is a single flat quad with its
  walls more than a sampling radius away, and it now measures identical to the
  same frame rendered with occlusion switched off; `lights`' cube had a diagonal
  gradient across a flat front face. The contact darkening they surround is
  still there, and tighter — in `Scene::Ao`'s trough the wall foot goes from
  53.1 to 51.7 luma and the gradient stops terracing, 19 distinct levels in 19
  steps against 13 in 16.

- **`SsaoParams::bias` and `forward.rs`'s `SSAO_BIAS` are gone**, and
  `SsaoParams::params.y` is padding. The bias existed because a threshold
  comparison turns a flat surface's own quantised samples into grey haze; a
  horizon integral has no threshold, since a sample in the surface's own plane
  lands exactly on the tangent where the integral is stationary. Swept from zero
  to 0.4 radians of angular bias and it moved nothing, so it was deleted rather
  than left as a uniform nothing reads.

- **The sun's shadow filter is as wide as the penumbra the scene has, not a
  constant.** `mesh.slang`'s `tile_pcf` takes its radius as a parameter, and
  `sun_penumbra_texels` measures one per fragment: sixteen depths `Load`ed from
  the atlas over eight tile texels, the ones nearer the light averaged into a
  blocker height, and a similar triangle through `SHADOW_SUN_TAN_RADIUS` turning
  that height into a width. PCSS, in other words, and the width is clamped
  between the old fixed reach and the search's own — under it a contact would
  lose the dither the rotated disc was tuned around, over it the filter would
  spread taps across texels nothing measured.

  What it buys: the far boundary of `apps/lantern`'s block shadow, thrown from
  the block's top edge onto a cascade whose texels are 62.5 mm, arrived
  quantised to them as a sawtooth. Its RMS departure from the straight line it
  should be fell **1.58 px to 0.58 px**. The block's foot line did not move,
  which is the lower clamp doing its job.

  **The physical sun buys nothing here and that was measured.** At `tan` of the
  real 0.27° angular radius, `apps/lantern` differed from a fixed filter in 36
  bytes of 4,915,200 and the dunes terrain not at all — a cascade texel is
  larger than the penumbra the geometry implies. So the shipped constant is a
  softness knob at four times the sun's size, and it says so.

  The sun only: `punctual_visibility` and `volumetric.slang` pass the fixed
  reach. `crcbl_shaders::mesh::SHADOW_CASTER_REACH` is new and
  `crcbl_render::shadow::CASTER_REACH` now takes its value from it, because the
  blocker search inverts the box `cascade_matrix` builds.

- **The sun's and every punctual light's shadow bias is a normal offset, not a
  slope-scaled depth bias.** `mesh.slang`'s `shadow_slope` is gone;
  `shadow_normal_offset` moves the receiver **along its own geometric normal**
  by `sin(acos(Ng·L))` texels before the lookup, so a grazing surface reads a
  texel it owns instead of comparing a depth that was raised until the acne went
  away. Only the constant term still moves towards the light.

  `SHADOW_SLOPE_BIAS_CLAMP` went with it and is not replaced: `tan` runs to
  infinity as a surface turns edge-on and needed a ceiling, and `sin` is bounded
  by one.

  `crcbl_render::shadow::SLOPE_BIAS_TEXELS` is now `NORMAL_OFFSET_TEXELS` and
  `FrameUniforms::shadow_params.w` carries the new meaning. `DEPTH_BIAS_TEXELS`
  fell from three texels to one, which is what the offset earned back. Measured
  on radv through `apps/lantern`'s review frame: the lit strip at the `-x`
  wall's foot went from 0.391 m wide, peaking 89 luma above the shadow it sat
  in, to nothing at all — the profile never leaves the shadow's own value — and
  the cornice lift over the shadowed back wall went 78.3 luma to 11.7. The dunes
  patch's self-shadowing dots went 60 to 24.

  It costs a scalloped fringe a couple of pixels deep at a silhouette's foot,
  where the offset walks a receiver across the edge of its own caster. That is
  the standard cost of this direction and `docs/plan/45-shadows.md` records it
  beside the numbers above.

  Goldens re-blessed: `cube`, `cube_97x61`, `dunes`, `spot_shadow`,
  `point_shadow`, and `apps/lantern`'s `room.png` and `live.png`. Nothing
  outside a shadow moved.

- **The sun's cascades cross-fade instead of switching.** `mesh.slang`'s cascade
  lookup is now `cascade_visibility`, and `sun_visibility` calls it for both
  cascades over a band at the outer edge of the one it selected — a tenth of
  that cascade's reach — mixing the two by distance. Where the switch used to
  fall, a shadow edge jumped sideways and a surface biased out of its own shadow
  on one side was back in it on the other, because a near cascade's texel is a
  sixth of the outer one's here and both biases are counted in texels.

  Measured on radv along the circle where the two cascades meet on
  `apps/lantern`'s floor, 4.088 m from the eye: the luma step across the
  boundary, on the samples the switch owns rather than a shadow edge crossing
  it, fell from 33.7 to 5.8 on average and from 63.6 to 17.7 at worst, while
  control circles at 3.6 m and 4.5 m came back byte-identical.

  It costs a second PCF for fragments inside the band and nothing for the ones
  outside it; the outermost cascade has nothing to fade into and never pays.
  Goldens re-blessed: `apps/lantern`'s `room.png` and `live.png`. Every golden
  in `crates/crcbl/tests/golden/` still matches.

- **The shadow filter is a rotated 32-tap disc, not a 3×3 box.** `tile_pcf` in
  both `mesh.slang` and `volumetric.slang` takes its taps on a Vogel spiral of
  radius two tile texels and turns that spiral by one of sixteen tabulated
  rotations, picked off the fragment's pixel coordinate — the froxel's column
  index, in the volumetric pass — through an ordered-dither matrix. Twice the
  penumbra of the box, which could not reach further without its tap count
  growing as the square of the radius.

  **A Vogel spiral rather than a Poisson set**, deliberately: the spiral has a
  closed form, so `crcbl_shaders::mesh`'s
  `the_shadow_disc_is_the_vogel_spiral_it_claims_to_be` re-derives every literal
  in the table instead of pinning a copy of itself. **The rotation is a constant
  table indexed by an integer** rather than a float hash and a `sincos`, so
  every target picks the same disc.

  `crcbl_render::shadow::DEPTH_BIAS_TEXELS` rose from 1.0 to 1.5, which is what
  a tap two texels out costs: measured on radv, the dunes patch's self-shadowing
  dots went 24 → 47 when the reach doubled at the old bias and back to 25 at the
  new one, with no lit strip at the wall's foot returning. The tap count is what
  keeps the dither quiet — the grain over a smooth shadowed patch of that scene
  reads 1.459 at 16 taps, 1.099 at 24 and 0.918 at 32, against the box's own
  0.827.

  **Nothing in the tree times it.** Thirty-two taps against nine, in two passes,
  chosen on the picture alone; `SHADOW_TAPS` is the first constant a
  graphics-quality setting should expose. Goldens re-blessed: `dunes`, and
  `apps/lantern`'s `room.png` and `live.png`.

### Breaking

- **`GpuInstance` gained `base_vertex` and `INSTANCE_STRIDE` is 96 bytes**,
  was 80. The shader declares its three `uint` of tail padding **explicitly**,
  because DXIL's structured-buffer stride does not round up where `std430`, WGSL
  and MSL do: letting the tail be implicit gave one buffer two element sizes, 96
  on three backends and 84 on the fourth, with nothing to report it.
- **`MeshPool::alias` is removed**, along with the two-mesh-table-entry design
  it existed for. `SkinnedMesh::reserve` and `release` no longer take a `Device`
  and cannot fail on one; `SkinnedMesh::mesh_id` takes no parity and answers the
  source mesh's id; `ForwardRenderer::reserve_skinned` no longer takes a
  `Device`, and `release_skinned` takes none and returns nothing. A skinned
  primitive costs two vertex runs and **no** mesh-table entries, so
  `Capacities::meshes` no longer has to be sized for one.

### Fixed

- **The DX12 backend can bind a read-only depth attachment.** `depth_read` on a
  render graph pass — what the sky pass and the ground grid both declare — was
  refused outright with `invalid descriptor`, because an image view owned
  exactly one depth descriptor and it was the writable one. A DSV's read-only
  flags are part of the descriptor rather than something chosen at bind time, so
  `create_image_view` now builds both and the render pass picks by what the
  caller declared; the stencil flag rides along only for a format that has a
  stencil plane. Clearing a read-only attachment is now refused at the seam,
  which D3D12 itself only reports through the debug layer.

- **Only one point light could ever cast a shadow, in any scene, on any
  backend.** `mesh.slang` admits a point light's cube only where its whole run
  of six faces is inside the atlas's light region, and that region was seven
  tiles — so the second point light of any pair was refused a run, lit without
  occluding, and _re-lit_ whatever its twin was shadowing. A rig with a light
  either side of a walkway, which is an ordinary rig, therefore had no working
  point-light shadow at all: `apps/shard`'s flanking braziers measured the
  player capsule's shadow at 2 of 255 levels. The atlas is now four tiles across
  by four down instead of three by three, so `SHADOW_LIGHT_TILES` is 14 — two
  point cubes and two spots — and `crcbl_render::shadow::LIGHT_SLOTS` is 4 to
  match. **The image did not grow**: `SHADOW_TILE` went from 1024 to 768 with
  the grid, and four times 768 is three times 1024, so the atlas is the same
  3072² `D32Float` allocation to the texel and the whole cost is per-tile shadow
  resolution. What is not free is the slots: a slot is a `DrawGen`, so the two
  the budget added are device-local memory the atlas did not ask for. Every
  golden of a shadowed scene moved with the tile size and was re-blessed —
  `crates/crcbl/tests/golden/dunes.png` and `apps/lantern`'s `room.png` and
  `live.png`.

- **`apps/viewer`'s idle turntable stopped for a click that only meant focus.**
  A press with no movement is a click, and the commonest click on that canvas is
  the one that hands it the keyboard — so a visitor who clicked to type lost the
  camera's idle turn for the rest of the session. It now hands over on a drag
  that has actually moved. This was intermittent rather than constant for a
  reason worth knowing: `PointerUpdate`'s `pressed` and `released` are per-frame
  edges, so a click landing inside one frame arrives as both at once and cancels
  itself, while the same click split across two frames arrives as a press alone.
  The browser gate's own focus click split about one run in three on the macOS
  runner and reddened the viewer's motion check there, which is how it was
  found.

- **A skinned mesh drew its bind pose, on every backend.** The vertex stage
  resolved its base vertex through the _draw's bucket_, and no skinned region
  has a bucket — buckets are built once, from the description's meshes — so the
  skinning dispatch's output was written correctly every frame and never read. A
  skinned instance now names its **source** mesh and carries the base of the
  region it was deformed into, read by `mesh.slang` and `mesh_cluster.slang`
  when `GpuInstance::flags` carries `GpuInstance::BASE_VERTEX_OVERRIDE`. The
  bucket stays authoritative without the bit, which a `Geometry::Dag` needs: its
  level is chosen per instance on the GPU and its base belongs to the selected
  level.

  Both raster and mesh-shader paths were wrong, and both are now covered by a
  test that renders a skinned cube and reads the pixels back — the kind of test
  whose absence let a subsystem pass every gate in the workspace while drawing
  one fixed pose.

- **A `GpuInstance` naming a mesh-table slot the description never filled no
  longer reads past the level-selection tables.** `draw_gen.slang`'s
  `mesh_levels_of` indexes them with `GpuInstance::mesh` and no bound, and its
  own doc comment states the invariant it rests on — "every entry an instance
  can name is filled" — but `ForwardRenderer::with_scene` sized them to the
  description's meshes, while `MeshPool::alias` hands out slots past every one
  of them. What that read found decided the frame: zeros resolved to mesh 0 and
  drew another mesh's geometry, and a live `group_count` sent `select_level`'s
  loop over a count no allocation backs, which on radv is a hard GPU recovery
  rather than a wrong picture. The tables are now sized by the mesh table's
  capacity, so the invariant the shader states is one the renderer keeps.

### Added

- **`apps/puppet`, a third-person character sample, and it runs in a browser.**
  A greybox map of steps and mounds, `crcbl_phys::CharacterController` walking a
  capsule over it with shadows on, and an orbit-follow camera. `W/A/S/D` walk,
  `Q/E` turn the view and `R/F` tilt it; before any key is pressed a scripted
  circuit paces the spawn pad so the page is not a still frame. It is in
  `web/build.sh`'s demo list, on the demo index, and covered by the browser e2e
  gate, which drives the walk key through CDP and requires the character to
  advance while it is held, stop when it is released, climb the 0.3 m step and
  be refused by the 0.9 m one.

  The controller stays camera-agnostic: `puppet::camera::walk_direction` turns
  the camera's yaw and the two input axes into a world direction and
  `puppet::game::run_tick` hands `move_and_slide` a displacement, so the
  conversion lives in the sample and nothing in `crcbl-phys` knows a camera
  exists. There is no animation — that is the sample's milestone 2.

- **`crcbl`'s `greybox` feature**, which re-exports `crcbl-greybox` as
  `crcbl::greybox`, the way `scene` does for `crcbl-scene`. A sample that wants
  metric blockout primitives can now name only the engine.

- **A swept-capsule query, `PhysicsWorld::sweep_capsule` and
  `sweep_capsule_excluding`,** alongside the sphere sweeps and with the same
  semantics: exact shape-level TOI into the same `ShapeHit`, triggers skipped,
  the broadphase asked for the swept volume rather than the centre line, and the
  `_excluding` form for a body that is registered in the world and would
  otherwise be the closest hit on its own segment. `crcbl_phys::query` gained
  `swept_capsule_vs_sphere`, `swept_capsule_vs_aabb` and
  `swept_capsule_vs_capsule` under it, and `OverlapQueries` the shared-borrow
  twins.

  A character controller cannot be built on a sphere sweep. Given a character's
  width the sphere is too short and rides over a step its chest should have
  struck; given its height it is too wide and cannot fit through its own
  doorway. The two errors are the same sphere, so neither is tunable away.

  Every capsule is Y-aligned, which makes the narrow phase exact rather than
  approximate: growing a target along Y by the capsule's half-height is closed
  over the shapes this crate has — a sphere becomes a capsule, a capsule a
  taller capsule, an AABB a taller AABB — so each test is the _sphere_ sweep
  against a grown target, with the contact point mapped back onto the real one.

- **A penetration query, `PhysicsWorld::capsule_penetrations_into`,** with
  `crcbl_phys::Penetration` and the `capsule_penetration_vs_*` trio behind it.
  It answers what a sweep structurally cannot: a sweep that starts already
  overlapping reports the contact at `t = 0` because there is no earlier time,
  and the _depth_ is a different measurement. PhysX and Unity split the same
  pair the same way. This is what a character that spawned inside the level gets
  out with.

- **`crcbl_phys::CharacterController`, the L0 capsule character controller**
  `docs/plan/05-physics.md` specifies: sweep-based movement with ground
  detection, a slope limit, step offset up and down, sliding along surfaces
  rather than stopping dead, and depenetration. It runs at whatever fixed
  timestep the caller steps it at, in `f64` throughout.

  **It knows nothing about any camera**, which is what lets one controller serve
  a first-person and a third-person game. `move_and_slide` takes a world-space
  **displacement** — turning a stick vector into a world direction is the
  caller's job, because that is the only step the two styles genuinely differ in
  — and there is no orientation on the type at all, so a first-person rig can
  pin facing to its view while a third-person one turns the body toward
  `MoveOutcome::motion` over time. No camera collision, no follow logic, no
  spring arm.

  The move is Quake's `SV_FlyMove` collect-and-slide loop over a plane set,
  including its crease fallback and its stop-dead rule, with Unreal's `StepUp`
  and `ComputeGroundMovementDelta` for the two pieces Quake has no equivalent
  of, and Godot's walkable-floor angle and floor snap. `CharacterConfig` carries
  the tunables; the slope limit is stored as the **cosine** of the steepest
  walkable angle, not the angle, so no platform transcendental runs anywhere in
  the deterministic path. The module's own docs name the failure modes it
  handles and the ones it does not.

- **`crcbl_scene::GltfNode::skin`, which node a document's skin is worn by.**
  The importer read the `skins` array but never `node.skin`, so nothing
  downstream could tell a skinned instance from scenery — and a mesh's
  `JOINTS_0` cannot decide it, because the same rigged mesh may legally be drawn
  again under a node with no skin. This is the fact a caller pairs a
  `MeshOrigin`'s bindings with a palette through. `gltf_check` now bounds-checks
  it as well: a node naming a skin that is not there used to reach `gltf`'s own
  `unwrap` and abort the process, which is exactly what that module exists to
  prevent.

- **The GPU skinning compute shader, `crcbl-shaders`' `skinning.slang`, and
  `crcbl_shaders::skinning`, which pins its layout.**
  `docs/plan/17-animation.md`'s skinning prepass: a joint palette and a run of
  bind-pose vertices in, the same vertices blended onto that palette written
  into a **transient region of the same vertex pool**. What it writes is
  `crcbl_shaders::mesh::MeshVertex` byte for byte, so a skinned mesh is drawn,
  culled and shadowed by passes that were never told skinning exists. SPIR-V,
  WGSL, MSL and DXIL artifacts are committed like every other shader's, so the
  browser gets the same path as every native backend.

  `crcbl_shaders::skinning` carries the numbers a consumer has to agree with
  exactly: `WORKGROUP_SIZE`, the `Params` uniform block and its `PARAMS_SIZE`,
  the `SkinBinding` per-vertex record and its `SKIN_BINDING_STRIDE`,
  `JOINT_STRIDE` for the palette buffer, and the bind-group table the pass
  needs. `Params::to_bytes` **refuses** a block whose bind-pose and skinned
  ranges overlap, or whose palette is empty — both are dispatches with no
  correct outcome and nothing on the GPU could report either.

  Three decisions are written down where they are made rather than left to be
  rediscovered. A normal goes through the blended matrix's **cofactor**, the
  same `normal_basis` the two mesh shaders use, so a joint carrying a
  non-uniform scale still shades correctly; what that does not model is the
  gradient of the weights across the surface, which is zero inside a rigidly
  bound region and largest in the band around a bending joint. The four weights
  are used **exactly as stored** and are not renormalised, matching
  `crcbl-scene`'s import — a set that does not sum deflates its vertex visibly
  rather than being silently repaired. And the `u16` joint indices glTF stores
  are **widened** to `u32` rather than packed, because `std430` padding makes
  the packed form exactly as wide and WGSL has no 16-bit integer to unpack into.

  The host side — the render-graph pass, the transient pool region and the
  buffers it reads — is `crcbl_render::skinning`, below.

- **The skinning compute pass, `crcbl_render::skinning`.** What dispatches
  `skinning.slang`. `Skinning` owns one frame's joint-palette and skin-binding
  buffers and a uniform block and bind group per skinned range, and
  `Skinning::add_pass` records one dispatch per range into the render graph,
  each sized from its own vertex count and the kernel's workgroup size. The
  vertex pool is imported into the graph as one resource read and written
  through one binding, so the barrier that orders the pass against the mesh
  passes that draw its output is the graph's.

  `SkinnedRegion` is the transient region of `crcbl_render::mesh_pool`'s vertex
  pool the pass writes into, and it is **double-buffered from the first
  version**, per `docs/plan/17-animation.md`'s 2026-07-27 correction: two runs
  of equal length that alternate with `Skinning::parity`, because TAA on
  deforming geometry needs the previous frame's skinned positions and
  retrofitting that is a skinning-pipeline rewrite. **Nothing reads the previous
  half yet** — there is no TAA pass — so the memory it costs is bought now to
  keep the pool layout from having to change later. `MeshPool::reserve_vertices`
  and `MeshPool::release_vertices` are the pool API it draws from: a run no
  upload fills, disjoint from every other live allocation, which is what makes
  the kernel's non-overlapping-ranges precondition true rather than merely
  asserted.

  **`Skinning::begin_frame` refuses a skin binding naming a joint its range's
  palette has not got**, and names the range, the vertex, the joint slot and the
  index. The shader can only clamp such an index, and `crcbl-scene` cannot check
  it at import because a glTF primitive does not know which skin will be applied
  to it — so this call, the one place the palette and the bindings are in hand
  together, is where a malformed rig is caught. Empty palettes, binding runs
  that are not one per vertex, and frames past any capacity are refused by name
  for the same reason: a character silently missing from a frame is what these
  bounds exist to prevent.

  **Nothing in the workspace executes the kernel yet.** `crcbl-render` depends
  on no backend, so its own tests check the recorded command stream and the
  bytes that reached each buffer; `crcbl_render::skinning::skin_vertex` is the
  CPU oracle a readback will be compared against, on the terms
  `crcbl_render::cull::visible_instances` already sets.

- **The seam an application draws a skinned mesh through,
  `crcbl_render::forward`.** The skinning kernel and the pass that dispatches it
  existed with no way for a caller to point a draw at what they wrote; this is
  that way. `ForwardRenderer::reserve_skinned` takes a description mesh index
  and hands back a `SkinnedMesh` — a `SkinnedRegion` plus the mesh-table entries
  a draw of it resolves through — `ForwardRenderer::vertex_buffer` is what
  `SkinningDesc::vertices` takes, `SkinnedMesh::skin_range` builds the frame's
  `SkinRange` from a palette and a set of bindings, and
  `ForwardRenderer::add_skinned_instance` / `set_skinned_instance` place the
  object with a `SkinnedInstanceDesc`. `ForwardRenderer::release_skinned` gives
  all of it back. A caller never spells a base vertex, a parity or a table id.

  **A skinned primitive gets two mesh-table entries, one per parity, and neither
  is ever rewritten.** `MeshPool::alias` is the new primitive that creates one:
  a second entry naming a mesh's indices and box at a different base vertex,
  owning no pool space, written once and cleared once by `MeshPool::free`. That
  preserves the reason the mesh table is a single host-visible buffer rather
  than a ring — nothing rewrites an entry between frames — which re-pointing one
  entry at the half a frame skins would have broken silently, on exactly the
  frames that overlap. What alternates instead is which entry an _instance_
  names, and instances already go through a ring. The bind pose keeps its own
  entry, so one mesh can be drawn skinned and unskinned in the same frame. A
  skinned mesh therefore costs two `Capacities::meshes` entries and two vertex
  regions on top of its own.

  **The two new frame calls are what make the missing barrier unrepresentable.**
  `ForwardRenderer::begin_skinned_frame` rotates the instance ring, hands the
  slot to `Skinning::begin_frame`, re-points every skinned object at the half
  that frame's dispatch fills, and only then uploads the instance delta — the
  one order that does not draw the previous frame's pose.
  `ForwardRenderer::add_skinned_passes` takes the `&Skinning` rather than a
  `BufferId`, adds the dispatch itself, and declares the read on the shadow
  pass, the depth prepass and the forward pass, so there is no ordering left for
  a caller to get wrong. `add_passes` and `begin_frame` are unchanged and build
  the frame they always did. `MAX_TIMED_PASSES` now includes
  `Skinning::MAX_PASSES`.

  Two limits are documented rather than hidden. A `Geometry::Dag` **cannot** be
  skinned — its coarser levels are separate vertex runs no dispatch writes — and
  `reserve_skinned` refuses one by name. And an alias entry carries the bind
  pose's bounding box, so a mesh deformed outside it is culled while on screen;
  the box the dispatch will actually fill is a property of a palette the
  reservation has never seen.

- **`InstancePool::rotate` and `InstancePool::flush`**, the two halves
  `InstancePool::begin_frame` now delegates to. Split so a caller with a write
  that depends on the frame slot — the skinned-instance re-pointing above — can
  make it between the rotation and the upload instead of a frame late.
  `begin_frame` is unchanged for everyone else.

- **`crcbl_scene::gltf_render` carries a skinned primitive's per-vertex bindings
  through to the vertices it emits.** `RenderScene::origins` is a new
  `Vec<MeshOrigin>`, one entry per `SceneDesc::meshes` row and in the same
  order. Each names the glTF mesh and primitive the row was made from and holds
  `MeshOrigin::bindings`: one `crcbl_shaders::skinning::SkinBinding` per emitted
  vertex, in emitted-vertex order, empty for a primitive with no `JOINTS_0`.
  That is the slice `crcbl_render::skinning::SkinRange::bindings` takes, so a
  caller passes it straight to the skinning pass instead of converting it; the
  conversion still names no renderer type, the record coming from
  `crcbl-shaders` the way `GpuMaterial` already does.

  The bindings could not be re-derived downstream, which is why they are
  produced here. A primitive that declares `NORMAL` emits one vertex per
  position; one that does not is **de-indexed** so the flat normals the
  specification requires can be per face, and emitted vertex `n` is then input
  vertex `indices[n]`. Nothing in the emitted run says which of the two shapes
  it got, so a consumer holding only the vertices could not pair them with the
  file's `JOINTS_0`/`WEIGHTS_0` at all.

  `MeshOrigin` is also the first mapping from a converted mesh back to the
  primitive it came from — `apps/viewer`'s `world_bounds` documents the absence
  of one. It is needed rather than decorative because a primitive the conversion
  skips produces no row: its bindings go with the geometry they indexed, no
  separate skip is logged for the rig, and every row after the hole sits at an
  index its document position no longer predicts.

  What a binding still does not arrive with is its palette. Joint indices are
  relative to the skin the _drawing node_ wears, and neither `InstanceDesc` nor
  `MeshOrigin` names a node, so building a `SkinRange` still takes the
  `GltfScene` the conversion read.

- **`apps/viewer` draws its document's skinned mesh, deformed by the clip it
  plays, in a browser.** `crate::model::skinned_of` decides which instances a
  skin deforms — from `GltfNode::skin`, because the same rigged mesh may legally
  be drawn again under a node with no skin and skinning that copy would put it
  wherever the joints are — and `Gpu::build_skinning` reserves a region for
  each, with the frame going through `ForwardRenderer::begin_skinned_frame` and
  `add_skinned_passes`. This is the first time the skinning kernel has executed
  on `crcbl-webgpu`: its WGSL was validated by naga and dispatched by nothing.
  The browser gate holds it — the turntable is dragged to a stop and the canvas
  must keep changing across a clip cycle, with the frozen camera and the moving
  pose both asserted so a pass cannot be vacuous.

- **A document a visitor drops on the browser canvas starts the turntable
  again.** It already re-framed the camera, on the argument that nobody has
  aimed at a file they have only just chosen; the turntable was the same
  decision and was left latched off, so a dropped document sat still at an angle
  chosen for the previous one. A re-export of the same document still changes
  neither, because that is the same document an artist has aimed at.

- **`apps/viewer` plays the clip in the document it opened, and `B` draws the
  posed skeleton.** The rig is converted into `crcbl::anim`'s `Skeleton` and
  `Clip`, sampled every frame off the render clock, and composed into a joint
  palette; the `[HUD]` line and the `F3` panel carry `pose`, which is how far
  the clip has bent the skeleton away from its rest pose in metres. `B` overlays
  the bones and each joint's axes — off by default, because a viewer's job is to
  show the asset unadorned and bones nobody asked for are decoration. Both the
  compiled-in demo document and one a visitor drops go through the same
  conversion.

  What the conversion cannot bring in becomes a `Skip`, alongside the ones the
  mesh conversion already reports: a skin whose joints are not in
  parent-before-child order is **refused rather than sorted**, because the
  mesh's `JOINTS_0` and the skin's `inverseBindMatrices` already agree on the
  existing numbering and renumbering would draw a skeleton matching no vertex in
  the file. A channel naming a node the skin has not got is ordinary and dropped
  quietly.

- **`crcbl-anim`, a new crate: skeletal animation's runtime half.** A `Skeleton`
  is joints in palette order, each with a parent index, an inverse bind matrix
  and a rest pose; a `Clip` is `Channel`s of keyframes over it;
  `Clip::sample_into` turns a clip and a time in seconds into one local `Trs`
  per joint, and `Palette::compute` composes those down the hierarchy and folds
  in the inverse binds to give the matrices a skinning shader consumes. All
  three glTF interpolation modes are implemented against the specification's
  Appendix C and quoted where each is: `STEP`, `LINEAR` — which is _spherical_
  for a rotation and linear for anything else — and `CUBICSPLINE`, whose
  tangents are scaled by the segment duration and whose rotations are
  normalized. Reachable as `crcbl::anim`.

  Deliberately only that much of `docs/plan/17-animation.md`: no blending, no
  state machine, no root motion and no GPU skinning, each of which is a later
  slice with its own consumer. It also does **not** depend on `crcbl-scene` — it
  takes the joint hierarchy as parent indices and the curves as plain arrays, so
  playing a cooked clip links no glTF parser.

  Three shapes are ordinary rather than errors, and each has a defined answer: a
  channel naming a joint this skeleton has not got is skipped (a clip drives a
  document's nodes, not only one skin's joints), a joint no channel drives keeps
  its **rest** pose rather than the identity (identity would collapse the bone
  onto its parent), and a time outside the clip holds the nearest keyframe
  rather than wrapping (looping is the caller's modulo, and a clip that must
  hold its last pose could not say so otherwise). Sampling allocates nothing per
  frame and finds its keyframe segment by binary search.

- **The browser viewer takes a document the visitor drops on it.** Drag a `.glb`
  or `.gltf` onto the canvas and it opens through the same loader a path does:
  the camera re-frames on the new bounds, the listing panel rebuilds, and a file
  that will not parse leaves the frame that is on screen and says why under the
  canvas. That completes `docs/plan/sample/05-viewer.md`'s V-F5, whose native
  half — `viewer <MODEL>` — has always been there. `viewer::load_bytes` is the
  new entry point: a document opened out of memory, which is what a dropped file
  is and what the demo's own compiled-in document now goes through too.

- **`apps/viewer` reports the rig of the document it opened.** The listing panel
  behind `I` names the file's animation clips and counts the joints its skins
  declare, or says `rig: none` for a document that brought neither — most of
  them. An animation the file left unnamed, which glTF allows, is drawn as
  `(unnamed clip N)` with its place in the file, rather than as a blank row. The
  `[HUD]` heartbeat and the `F3` panel carry the joint count as well, and only
  the count: a clip name is arbitrary text out of someone else's document, and
  that line is parsed as `name: value` pairs. `Model::rig` is the new
  `viewer::Rig` summary — nothing here poses a skeleton yet, so a rig is
  something the viewer reports rather than plays.

- **The browser demo's document has a rig to report.** The `.glb`
  `apps/viewer/src/demo_model.rs` generates now carries a skin over two joint
  nodes with real inverse bind matrices, `JOINTS_0`/`WEIGHTS_0` binding the
  crate's lower and upper halves to a joint each, and one clip that turns the
  upper one. A joint node draws nothing, so the demo still places three
  instances and still frames on the same box. The lower joint carries the
  crate's own placement, because glTF says a skinned mesh node's transform must
  be ignored — so a renderer that skins and this one, which does not, draw the
  crate in the same place. The browser gate requires the joint count off the
  heartbeat, which is what makes the import's new skin and clip reading
  something a check can fail on.

- **A `GltfNode` carries its own transform and the nodes under it.**
  `GltfNode::local_transform` is the node's placement as the document gives it —
  glTF's `matrix`, or its `translation`/`rotation`/`scale` composed as
  `T * R * S`, and `Mat4::IDENTITY` for a node that declares none — and
  `GltfNode::children` is the document's `children` array in file order. Both
  are on the node table rather than on `GltfScene::instances`, because a joint
  node draws no mesh and so never becomes an instance: until now a skeleton's
  bones were the one part of an imported document nothing could read. A matrix
  rather than three components because the conversion back is the lossy
  direction, and a `children` list rather than a `parent` because that one is
  well defined for a node no scene reaches; both accessors say so. The importer
  composes instance transforms out of the same expression the field stores, so
  the local and the world answer cannot drift apart.

- **The glTF importer reads skins and animations.** `GltfScene::skins` is the
  document's `skins` array — joint node indices, inverse bind matrices as
  `glam::Mat4`, and the skeleton root — and `GltfScene::clips` is its
  `animations` array, each clip a list of `GltfChannel`s carrying a target node,
  keyframe times, an interpolation and the sampled translations, rotations,
  scales or morph weights. `GltfPrimitive::joints` and `GltfPrimitive::weights`
  are the per-vertex binding, empty for an unskinned primitive and otherwise as
  long as `positions`. Component types are normalised the way the specification
  defines them, so a `JOINTS_0` stored as bytes and a quaternion stored as
  normalized shorts arrive as `u16` and `f32` rather than as raw numbers. This
  is the source stage of `docs/plan/17-animation.md` and nothing plays it yet: a
  rigged character still draws in its bind pose, and the two warnings that used
  to say skins and animations were being skipped are gone because they no longer
  are. Morph _targets_ are still dropped, and still warned about.

- **`apps/bracket` — matchmaking, rating and a ladder, with no game attached.**
  Players queue, get paired by rating with a tolerance that widens as they wait,
  a stub resolves the match from true skill, and Elo moves.
  `bracket sim --seed N --players M --ticks K` runs a synthetic population
  headless and deterministically, reporting what the matchmaker traded: how far
  the ratings ended from the true skills, how evenly it paired, and how long
  people waited. It runs in a browser as the tenth demo on the site, taking no
  input at all: what a visitor sees is a ladder sorting itself out and a
  CONVERGENCE curve falling, drawn with the new `DrawList::polyline`. Each
  ladder bar carries a mark at that player's true skill — the number the
  matchmaker never sees — so the claim is on screen rather than asserted. The
  curve climbs again if left running, which is real and explained on the page:
  pairing people against their nearest equal is in tension with rating them
  correctly.

- **`DrawList::line` and `DrawList::polyline`** — stroked segments and connected
  runs, the UI layer's first primitive that is not axis-aligned. Both take a
  thickness in pixels and stroke centred on the path; a polyline can close, and
  its corners are bevelled. A point that is not finite **breaks** the run rather
  than being dropped, so a caller whose data diverged draws the gap instead of a
  chord it never asked for. `apps/orbit` draws its trajectory and engine plume
  with them; the map used to be a row of squares because nothing in the engine
  could draw a curve.

- **`apps/orbit` runs in a browser.** The ninth demo on the site, and the
  physics pillar's acceptance test: a rocket off a pad, through an atmosphere
  that fights it, into an orbit it holds. Burning or in the air, the flight is
  integrated — inverse-square gravity, quadratic drag, thrust against a mass
  that falls as the tank empties, four substeps a tick. Coasting in vacuum it is
  propagated analytically instead, so timewarp to x1000 costs no more than x1
  and drifts by nothing; opening the throttle or touching the top of the
  atmosphere drops it back to x1 on the same tick, because the analytic solution
  stops being true there. It flies itself until the first key is pressed, so a
  page that has just loaded shows a flight rather than a rocket on a pad, and
  the orbit drawn on the map is the propagator asked where the ship will be at
  ninety-six times across one period rather than an ellipse fitted to it.

- **`crcbl_phys::PointGravity`** — Newtonian gravity toward a body, falling off
  as the inverse square, alongside the uniform `GravityForce` that stays. A
  platformer wants the field; anything that goes high enough to notice gravity
  points at something wants the point, and an orbit is only possible under the
  second.

- **`crcbl_phys::propagate` and `Orbit`** — analytic Kepler propagation, so a
  coasting body's position at time `t` costs the same whether `t` is a second or
  a century away and drifts by nothing in between. It is the universal-variable
  formulation, which is one code path for every conic: a circular parking orbit,
  a transfer ellipse and an escape hyperbola all go through the same solve, and
  none of the angular elements that are undefined for a circular or equatorial
  orbit is ever formed. `Orbit` reports the quantities a flight UI can always
  show — semi-major axis, eccentricity, periapsis, apoapsis, period, specific
  energy — and returns `None` for the period and apoapsis of an orbit that does
  not come back rather than a zero that reads like one. Ten thousand chained
  propagations of a geostationary orbit leave its energy and semi-major axis
  within a billionth of where they started.

- **`crcbl_phys::Frames`** — a hierarchy of reference frames (star → planet →
  moon → vehicle) with explicit sphere-of-influence crossings. `Frames::convert`
  reads a `State` given in one frame as it stands in another by walking to their
  common ancestor, and `Frames::transition` answers which frame a body should be
  simulated in next. Positions are `WorldPos`, so a conversion is integer sector
  arithmetic and round-trips a position bit for bit: a millimetre held 10^15 m
  from the root survives a crossing to within a nanometre, where a plain `f64`
  metre coordinate cannot represent it at all. `sphere_of_influence` is the
  Laplace radius of the patched-conic approximation, checked in tests against
  the published radii for Earth and the Moon.

- **`crcbl_phys::Atmosphere` and `AtmosphericDrag`** — an exponential
  density-vs-altitude profile around a spherical body, and the quadratic drag
  `F = ½ρv²·Cd·A` a body moving through it feels. Terminal velocity is not
  written anywhere: it emerges where the drag balances what is pulling the body
  down, and the tests measure it against `√(2mg / ρ·Cd·A)` rather than against a
  number the module chose. `Atmosphere::EARTH` is the ISO 2533 sea-level density
  with a troposphere-fit scale height and the Kármán line as its ceiling; above
  that ceiling there is no air at all, which is the boundary an on-rails orbit
  propagator will hand back to live integration at. This is separate from
  `DragForce`, which is linear damping and stays — the two have different
  terminal velocities and neither is a better version of the other.

- **`apps/viewer` runs in a browser.** It is the eighth demo on the site, built
  by `web/build.sh` and driven by the browser e2e gate like the rest. A tab has
  no path to type and no directory to root an asset source at, so it opens a
  glTF document generated by `apps/viewer/src/demo_model.rs` and compiled into
  the wasm module — two meshes at three nodes with two materials, enough that
  the listing panel behind `I` reports something. Everything past the loading is
  the native code: the orbit camera, the frame-on-load, the grid scaled to the
  document, the wireframe and normals views, the exposure. A visitor could not
  open a file of their own; the drop target above is what changed that, in this
  same unreleased version.

- **`crcbl_assets::MemorySource`** — an `AssetSource` over bytes already in
  memory, for content that never lived in a directory. It is what the viewer's
  browser demo reads its document through, and what a dropped file would arrive
  as.

- **`viewer` turns the document by itself until you take hold of it.** An idle
  turntable, stopped for good by the first drag, pan or zoom. A tool whose
  subject does not move otherwise draws the same frame for ever, which is
  indistinguishable from a stopped loop — the browser gate reads the angle to
  know a frame is advancing.

### Changed

- **`crcbl::engine::PolledGpu` gained a `Context` associated type**, passed
  through `PolledBoot::request`. It is `()` for every bundle built from a window
  and its options, which is all of them but one; `crcbl::impl_polled_gpu!`
  declares that, so a sample using the macro needs no change. `apps/viewer`'s
  bundle is built out of the glTF document it exists to show, and could not be
  constructed from the window alone — before this the only route was opening
  with nothing resident and swapping the real scene in afterwards, which builds
  a renderer twice at every start-up to work around a signature.

### Fixed

- **`crcbl_phys::Orbit::apoapsis` no longer returns `Some(NaN)` for a radial
  trajectory.** It came from the semi-latus rectum over `1 - e`, which for a
  body with no angular momentum is `0 / 0` — and that is not a corner case: a
  rocket going straight up off a planet that does not rotate is exactly radial,
  and so is anything dropped from rest. It is now `a(1 + e)`, which is the same
  quantity for every other orbit, better conditioned near `e = 1`, and finite
  here: the height the body stops at before falling back. `periapsis` keeps the
  semi-latus form, which is the one that survives the other end.

- **`crcbl-vk` answered the wrong error variant for anisotropy it cannot do.**
  `create_sampler` checked the ceiling before the feature, so on a device
  without `SAMPLER_ANISOTROPY` — whose honest ceiling is 1.0, the value that
  turns anisotropy off — every anisotropic request came back
  `HalError::InvalidDescriptor`, "your descriptor is wrong". What was wrong was
  that the device cannot do it at all, which the seam spells
  `HalError::Unsupported`; a caller matching on that variant to pick a fallback
  path missed the refusal entirely. The feature check now runs first, so the
  ceiling check only ever sees a device that has the feature.

- **`crcbl-vk` reported an adapter's limits on a device that was granted less.**
  A device's `caps()` narrowed `features` to what `vkCreateDevice` actually
  granted but carried the adapter's `limits` verbatim, and four of those are
  feature-gated. A device opened without `SAMPLER_ANISOTROPY` reported
  `max_sampler_anisotropy: 16` and then answered `Unsupported` to every sampler
  above 1.0; `max_push_constant_size`, `max_draw_indirect_count` and
  `max_bindless_descriptors` overstated the same way. A caller reading a limit
  to decide what to ask for — which is what the seam says the field is for — was
  refused by the device that reported it. The gating is now one function the
  adapter applies to what it supports and the device applies again to what it
  was granted.

- **`Instance::destroy_surface` documented a teardown order that does not
  exist.** Its one-line doc said every swapchain on a surface must already be
  destroyed. No backend requires that and four go out of their way not to:
  `crcbl-vk` keeps a per-surface swapchain count and a zombie list so
  `vkDestroySurfaceKHR` can be deferred, `crcbl-mtl` lets the last swapchain's
  clone of the layer release it, `crcbl-dx12` has nothing to defer because a
  swapchain holds its own `HWND`, and the WebGPU replayer drops the context
  reference without unconfiguring the canvas. The doc now says what they all do:
  the handle goes stale immediately, the swapchain on it keeps working, and the
  caller owes no order between the two calls. A caller who believed the old
  sentence was writing teardown sequencing it never needed.

- **A mesh dispatch's indirect arguments are checked like any other draw's.**
  `draw_mesh_tasks_indirect` recorded whatever offset, stride and count it was
  handed on `crcbl-vk` and the null backend, so a misaligned offset reached the
  driver and was read from the wrong bytes. `plan_mesh_indirect` had been on the
  seam since the argument rules moved there and had no caller; both backends now
  call it. The structure is three words rather than a draw's four or five and
  every other rule is the same one, because they are all rules about stepping an
  array of structures in a buffer. `crcbl-webgpu` still answers `Unsupported`:
  WebGPU has no mesh stage.

- **A sampler is held to an anisotropy floor, and a NaN no longer reaches a
  driver.** `SamplerDesc::anisotropy` documented its ceiling and never its
  floor, though `1.0` is the value that disables anisotropy and nothing below it
  is a request. The null backend and `crcbl-webgpu` served anything; the floor
  is now `SamplerDesc::check_anisotropy_floor` and every backend calls it.

  It also refuses a NaN, which the old spelling did not: `anisotropy < 1.0` is
  **false** for a NaN, and so is `anisotropy > max`, so a NaN passed both checks
  on every backend and reached the driver unexamined. The floor is written to
  catch it and says in its own comment why it is not spelled the shorter way.

  The ceiling stayed separate from the floor rather than joining it in one
  check, because `crcbl-webgpu` must not enforce it: that backend reports
  `max_sampler_anisotropy: 1` to mean "no ceiling this backend can guarantee",
  and refusing above it would make anisotropic filtering unreachable there. A
  test now stands over that: wiring the ceiling into `crcbl-webgpu` fails.

- **The null backend and `crcbl-webgpu` hold a buffer binding to its slot's
  offset alignment.** `Limits::min_uniform_buffer_offset_alignment` and
  `min_storage_buffer_offset_alignment` are documented as alignments a binding
  offset must be a multiple of, and neither backend mentioned either limit — so
  a misaligned offset became `VUID-VkWriteDescriptorSet-descriptorType-00327` in
  a driver rather than an `InvalidDescriptor` naming the binding, and the
  reference backend could not state the rule at all. The null backend also now
  checks a bind's **dynamic** offsets against the layout that declared them: one
  per dynamic-offset binding, each aligned.

  Both rules moved out of `crcbl-vk` to the seam as
  `crcbl_hal::check_binding_offset_alignment` and
  `crcbl_hal::check_dynamic_offsets`, so there is one copy. `crcbl-webgpu`
  enforces the first and not the second, and its `bind_group` says so: that
  method is on the encoder, which can reach neither the device's limits nor its
  layout table.

- **`crcbl-vk` and the null backend check a count-buffer draw's offsets, not
  just its handles.** `draw_indirect_count` and `draw_indexed_indirect_count`
  took `args_offset`, `count_offset`, `max_draw_count` and `stride` on trust,
  which is the same undefined read the plain indirect form had: a driver given a
  misaligned offset returns no error and reads argument structures from the
  wrong bytes. The seam gained `plan_count_structures`, which adds the two rules
  particular to this form — both offsets four-byte aligned, and the count word
  fitting inside the count buffer — and delegates the argument array to
  `plan_structures`, so the stride and span rules have one copy rather than two.
  `crcbl-webgpu` still answers `Unsupported`, because WebGPU has no count-buffer
  draw.

  `crcbl-vk`'s resolve failure also named the argument buffer whichever of the
  two was actually missing, so a bad count buffer reported the wrong handle.

- **`crcbl-vk`, the null backend and `crcbl-webgpu` refuse an image view of mips
  or layers its image does not have.** All three passed the subresource straight
  through; on Vulkan that is
  `VUID-VkImageViewCreateInfo-subresourceRange-01478`, where drivers return
  `VK_SUCCESS` and the view addresses mips that do not exist. None of the three
  could state the rule, because none of them recorded the shape it is about: the
  null backend filed `Detail::None` for every image, `crcbl-vk`'s `ImageEntry`
  kept the format and not the counts, and `crcbl-webgpu` tracked no images at
  all. Each now records the mip and layer counts and calls the seam's new
  `ImageViewDesc::check`.

  **An explicit count that runs past the end is refused, not clamped.**
  `ImageSubresourceRange::ALL` is how a caller asks for "the rest", so a literal
  count larger than the image is a caller's arithmetic being wrong, and Vulkan
  and WebGPU both treat it as invalid. `crcbl-mtl` and `crcbl-dx12` clamp it
  instead, which is now a divergence from the seam recorded in `docs/backlog.md`
  rather than a difference anyone should rely on.

- **`crcbl-vk`, the null backend and `crcbl-webgpu` refuse an image whose shape
  no API can build.** `ImageDesc::check` now states two rules the seam owed: an
  `ImageType::D1` image has a height of 1 — which `Extent3d::height` had
  documented all along without anything enforcing it — and a multisampled image
  is two-dimensional with exactly one mip level. Both are the graphics APIs'
  rules rather than this seam's (`VUID-VkImageCreateInfo-imageType-00956` and
  `VUID-VkImageCreateInfo-samples-02257`), and `crcbl-mtl` and `crcbl-dx12` were
  already refusing the first and the mip half from their own copies. So the
  answer stopped depending on which backend the frame ran on.

- **`crcbl-vk`, the null backend and `crcbl-webgpu` refuse a malformed indirect
  draw instead of handing it to a driver.** All three passed `DrawIndirect`'s
  `offset`, `stride` and `draw_count` straight through. On Vulkan a misaligned
  offset is `VUID-vkCmdDrawIndirect-offset-02710`: no error code is returned and
  the driver reads argument structures from the wrong bytes, so the symptom is a
  wrong picture or a fault with a clean log. The rules were not new — they were
  `crcbl-mtl`'s, whose own doc said they are pure arithmetic testable without a
  Mac — so they moved to the seam as `crcbl_hal::indirect` and the three
  backends now call them. `crcbl-dx12` and `crcbl-mtl` are unchanged; they
  already enforced the same rules.

  **`crcbl-webgpu` enforces the offset and stride rules and not the bound**, and
  says so where it is written: its encoder holds a channel and cannot reach a
  buffer's length, and the bound is the one rule of the three that the browser
  itself validates and reports. The other two it does not report, which is why
  they are refused before anything is encoded.

- **`crcbl-dx12` clears the outstanding acquire when a swapchain is
  reconfigured.** `reconfigure_swapchain` destroys the old back buffers and
  their views but edited the swapchain entry in place, so the acquired ring
  index survived and named an image that no longer existed — a present across a
  reconfigure presented a dead back buffer. `crcbl-vk` and the null backend
  avoid this by replacing the whole entry; this one now says it explicitly.
  Found by the agnostic `a_present_without_an_acquire_is_refused` the moment it
  first ran on WARP.

- **The null backend refuses a present with no matching acquire**, the last of
  the seam's presentation rules it could not state. It kept a ring cursor but no
  record of the outstanding acquire, so it had nothing to refuse against, while
  `crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` had each been answering "present
  without a matching `acquire_next_frame`" all along — the one backend whose
  purpose is to model the seam with no driver in the room was the one that could
  not. `Detail::Swapchain` now carries the acquired ring index, taken by the
  present and cleared by `reconfigure_swapchain`, which reissues the ring the
  index pointed into. With every backend answering, one agnostic test
  (`a_present_without_an_acquire_is_refused`) now holds them all to the same
  sentence.

- **`crcbl-webgpu` refuses a present with no frame to present.** `crcbl-vk`,
  `crcbl-mtl` and `crcbl-dx12` all answer "present without a matching
  `acquire_next_frame`"; this backend answered `Ok` and encoded the command, so
  the replayer went on to present a canvas whose texture the swapchain had never
  handed out. The arm that matters is a present after `reconfigure_swapchain`,
  which clears the acquired pair _and destroys the image behind it_ — a
  use-after-free that used to reach the browser as an ordinary command. A
  present before any acquire and the same frame presented twice are refused too,
  so the rule reads the same here as everywhere else. Getting the last of those
  took a second piece of state rather than the acquired pair alone: the pair has
  a second job — the next acquire retires it, and taking it at the present would
  put the replayer's image and image-view tables back to growing by one entry
  per frame — so `SwapchainState` records separately whether it has been
  presented.

- **`crcbl-vk` refuses an image with zero mip levels**, which it silently
  accepted and then clamped to one on the way to `vkCreateImage`. The seam and
  the null backend have always called a zero a caller bug; this backend kept its
  own copy of the image rules and that copy had drifted, so a zero reached the
  driver as `VUID-VkImageCreateInfo-mipLevels-00947` — reported by the
  validation layer and not necessarily by a release driver. `create_image` now
  calls `ImageDesc::check`, the shared version of the rules, instead of
  restating them; the format/type/usage support query stays, being the one
  Vulkan has to be asked. Callers passing at least one mip level see no change.

- **`crcbl-webgpu` holds a buffer binding to its slot's range ceiling and to the
  memory a shader may write.** Both rules turn on the slot's `BindingKind`,
  which lives in the bind group layout, so the device now records the buffer
  bindings of each live layout alongside the buffer sizes and locations it
  already kept. A binding over `Limits::max_uniform_buffer_range` or
  `max_storage_buffer_range` — including through
  `BindingResource::WHOLE_BUFFER`, which is resolved against the recorded size —
  and a `HostUpload` or `HostReadback` buffer in a
  `StorageBuffer { read_only: false }` slot are now
  `HalError::InvalidDescriptor` from `create_bind_group`. The first used to
  arrive a frame later as WebGPU's own `maxUniformBufferBindingSize` error
  through `Device::take_error`; the second arrived nowhere at all, because
  `COPY_DST | STORAGE` is an ordinary legal WebGPU buffer and only D3D12 refuses
  it. A bind group naming a binding its layout does not declare, or a layout or
  buffer handle this device did not issue, is refused too.

- **`crcbl-webgpu` refuses a write or a readback past the end of its buffer.**
  `Device::write_buffer` checked only that its end address fits a `u64`, and
  `Device::request_readback` checked nothing, so a range past the end of the
  buffer was an `InvalidDescriptor` on the other four backends and an encoded
  command here. The device now records each live buffer's size — the same table
  it already keeps for query-set counts, for the same reason: the browser is not
  needed to know how big a buffer this device asked for. A stale or unissued
  buffer handle is now `HalError::InvalidHandle` from these two calls rather
  than a command naming an id the replayer would fail to resolve a frame later.
  Callers already writing in range see no change.

- **`crcbl-webgpu` checks image and swapchain descriptors instead of encoding
  them.** `create_image` ran no validation at all: a zero extent, an extent past
  `max_image_2d`/`max_image_3d`, more array layers than the device allows, zero
  mip levels, more mips than the extent can hold, a sample count that is not a
  power of two, and an empty usage were each refused on the other backends and
  encoded here. `create_swapchain` and `reconfigure_swapchain` took a zero
  extent the same way, where the seam says an unconfigured or minimized window
  means "do not create one yet" and every other backend answers in that
  sentence. Both now refuse before anything reaches the wire, so the failure
  names the descriptor rather than arriving a frame later as the browser's
  wording about a texture or a canvas through `Device::take_error`.

- **`crcbl-webgpu` runs three seam checks it used to run nowhere.**
  `BindGroupLayoutDesc::check_entries` and
  `ComputePipelineDesc::check_workgroup_size` are documented "every backend
  calls this" and every other backend does; this one called neither, so a zero
  binding count, a binding number declared twice, a `VARIABLE_COUNT` entry out
  of place, a visibility naming a stage the device lacks, and a workgroup size
  that is zero or past `Limits::max_compute_workgroup_size` were all refusals on
  four backends and encoded commands here. Neither is catchable downstream:
  WebGPU has no rule about most of them, and `workgroupSize` is dropped on the
  wire because `GPUComputePipelineDescriptor` has no member for it —
  `Command::CreateComputePipeline`'s docs claimed the replayer checked it, which
  it never could. Third, `create_shader_module` now answers
  `ShaderModuleDesc::unusable(ShaderSources::WGSL)` for a descriptor carrying no
  WGSL, where it used to encode a `createShaderModule` with empty `code` and let
  the first pipeline built on it fail with a browser message naming neither the
  module nor the format the caller shipped.

- **`crcbl-vk` refuses a swapchain format the surface does not offer.**
  `SwapchainDesc::format` is documented "must be one of `SurfaceCaps::formats`",
  and the null, Metal and D3D12 backends each answered
  `HalError::InvalidDescriptor` for one that is not. Vulkan passed whatever it
  was given straight to `vkCreateSwapchainKHR`, which is
  `VUID-VkSwapchainCreateInfoKHR-imageFormat-01273` — reported by the validation
  layer and not necessarily by a release driver, so the same descriptor was a
  caller bug on three backends and an unusable-or-working swapchain on the
  fourth depending on the driver. Both paths now check: the WSI one against
  `vkGetPhysicalDeviceSurfaceFormatsKHR` filtered exactly as `surface_caps`
  filters it, and the offscreen ring against its own list. Callers who already
  read `SurfaceCaps::formats` see no change.

- **A page that opens a second GPU device no longer wedges on the first one's
  last answer.** The engine's pump held any reply `putReplyStream` would not
  take and offered it again next frame, which is right for the three reasons a
  live channel refuses one and wrong for the fourth: when there is no channel
  left, the run those replies answer is over, and the next `StreamChannel` the
  page installs starts its sequence numbers at `0` again. The held reply then
  named a command that channel never sent, `ReplyInbox::drain` refused the whole
  buffer it arrived in — the real answers in it included — and whatever was
  waiting on one of those waited for ever. `web/engine/gpu-transport.js` now
  exports `replyChannelInstalled`, which is how `web/engine/demo.js`,
  `web/probe/main.js` and `web/harness/main.js` tell "never" from "not now" and
  drop replies nothing can receive. Reachable since the offscreen teardown began
  asking `Device::take_error` on its way out, which puts a reply-bearing command
  in the last frame before the channel goes: it timed out every second scene of
  `web/run-render-harness-e2e.sh`, six of twelve, alternating by index.

- **`crcbl-vk` refuses a writable storage binding of a host-visible buffer.**
  `BufferDesc::memory` has always said that a buffer a shader writes must be
  `MemoryLocation::DeviceLocal` — D3D12's upload and readback heaps refuse
  `ALLOW_UNORDERED_ACCESS` at creation — but only the null and D3D12 backends
  enforced it. Vulkan accepted a `HostUpload` or `HostReadback` buffer in a
  `BindingKind::StorageBuffer { read_only: false }` slot: the driver allows it
  and the validation layer reports nothing, so the same descriptor was a caller
  bug on two backends and a working binding on Vulkan that would fail on D3D12.
  `create_bind_group` now answers `HalError::InvalidDescriptor` naming the
  binding and the location it was given. Read-only storage bindings of a
  host-visible buffer are unaffected, and callers who already respected the
  documented rule see no change. `crcbl-mtl` and `crcbl-webgpu` still do not
  enforce it; `docs/backlog.md` carries both.

- **`crcbl-webgpu` refuses a zero-size buffer, which the seam has always said it
  must.** `BufferDesc::size` is documented "must be non-zero", and the null,
  Vulkan, Metal and D3D12 backends each answered `HalError::InvalidDescriptor`
  for a zero. The WebGPU backend allocated a handle, encoded a `CreateBuffer`
  and answered `Ok` — so the same descriptor was a caller bug on four backends
  and a zero-size `GPUBuffer` on the fifth, and nothing downstream would have
  caught it because the stream refuses malformed streams, not invalid
  descriptors. Callers who already respected the documented rule see no change.
  Two tests now hold every backend to it:
  `a_zero_size_buffer_is_refused_instead_of_served` in the agnostic seam suite
  for the four native backends, and
  `a_zero_size_buffer_is_refused_without_encoding_anything` in `crcbl-webgpu`'s
  own `hal::tests` for the browser one, which that suite cannot reach.

### Changed

- **A depth-test-only pass no longer stores its depth attachment on Vulkan.**
  `crcbl-vk` answers `VK_ATTACHMENT_STORE_OP_NONE` — Vulkan 1.3 core, so no
  extension and no feature — for any attachment whose
  `DepthStencilAttachment::read_only` is set, where it previously passed the
  caller's `StoreOp` straight through and wrote back a buffer nothing had
  written. The picture is unchanged, because nothing wrote: a pass that only
  tests depth has nothing to store. `crcbl-render` already set the flag from the
  pass's own `write` flag, so no caller changes and no other backend is
  affected. `ResourceState::DepthStencilRead` still declares an attachment write
  in the barrier masks — conservatism for the backends that have no such store
  op, and `docs/backlog.md` carries the measurement and the open question.

### Added

- **`crcbl_hal::indirect`**, the seam's indirect-draw argument rules:
  `plan_structures` and `check_layout`, the argument widths the three graphics
  APIs agree on, and `IndirectPlan`. A backend that steps an array of argument
  structures now has one place to ask whether it may.

- **A binary run can prove the validation layer is _checking_, not merely
  loaded.** `CRCBL_VK_VALIDATION_PROVOKE=1` asks a **debug** build of `crcbl-vk`
  to record one deliberate specification violation — a `vkCmdCopyBuffer` region
  larger than either of the two 64-byte buffers it is given — into a command
  buffer that is ended and destroyed **without ever being submitted**, at the
  first successful `Device::present`. Only a _core check_ produces that message,
  which is what the self-test above cannot ask: a submitted message is delivered
  whatever the layer's checks are set to, so a layer running with
  `VK_KHRONOS_VALIDATION_VALIDATE_CORE=false` loads, prints
  `crcbl-vk: validation enabled (…)`, delivers the self-test and reports nothing
  at all — measured, and now the difference a run can see. After a present
  rather than at `VkInstance::open` for a second reason: a whole frame has by
  then been recorded, submitted and presented, so a messenger that went quiet
  after start-up fails it too. Recorded and dropped, so no driver ever executes
  the undefined behaviour; harmless with no layer loaded, and with
  `CRCBL_VK_VALIDATION_FATAL=1` the resulting error rides the ordinary
  `Device::take_error` path and ends the run. **Off by default on every
  profile**, and `#[cfg(debug_assertions)]` besides — a release build logs that
  it heard the request and cannot honour it. Every gate that runs a binary under
  the layer sets it: `tools/run-samples-windowed.sh`, `run-x11-e2e.sh` and
  `run-wayland-e2e.sh` grade it beside their existing self-test pass, at no
  extra run, and `crcbl-cli`'s scaffold suite gets its own run because the
  self-test's fatal error would otherwise end that one before the first present.
  `run-vk-e2e.sh` asks the same question through the `crcbl-vk` suite it already
  runs.

- **A binary run can prove its own validation check is able to fail.**
  `CRCBL_VK_VALIDATION_SELF_TEST=1` asks a **debug** build of `crcbl-vk` to put
  one synthetic message (`CRCBL-VALIDATION-SELF-TEST`, `ERROR`, `VALIDATION`)
  through `vkSubmitDebugUtilsMessageEXT` as `VkInstance::open` creates the debug
  messenger, so it travels the messenger, the callback, `ValidationSink` and the
  log exactly as a layer error does. The three shell harnesses that run a binary
  under the layer — `tools/run-samples-windowed.sh` and the two `crcbl-shell`
  e2e scripts — each gained a pass that sets it and requires their own
  validation check to come back red. Until now that check had never been shown
  able to match anything: demoting the callback's record from `error!` to
  `info!` leaves every ordinary pass green and only this one goes red. **Off by
  default on every profile**, and `#[cfg(debug_assertions)]` besides, so a
  release build cannot be made to write validation errors into its own log — it
  logs that it heard the request and cannot honour it. It does **not** prove the
  layer is checking anything: a submitted message is delivered whatever the
  layer's checks are set to, and `docs/backlog.md` says what would.

- **Every sample reports the effects its frames were actually drawn through.**
  `RenderEffects::row` is the one spelling — `shadows ao ssr`, `none` for an
  empty set — and `viewer`, `quarry` and `sandbox` now carry it on their summary
  line beside `lantern`, which already did. `Summary::effects` on viewer and
  sandbox, `Paths::effects` and a new `effects` debug-panel row on quarry. It is
  read back off the `ForwardRenderer`, so a request the device clamped does not
  report as granted. Each of the three has a unit test that fails if the line
  handing the player's `[engine.video]` settings to the renderer is deleted —
  which nothing checked before.

- **A Vulkan validation error can fail a run, not just log one.**
  `CRCBL_VK_VALIDATION_FATAL=1` makes `crcbl-vk` answer the seam's existing
  out-of-band channel — `Device::take_error`, which the engine already drains at
  the top of every frame — so a specification violation becomes
  `GpuError::Hal(HalError::Backend(…))` and stops the run. A headless sample
  that drew through one used to exit 0 with the message in a log nobody read.
  All seven CI steps that set `CRCBL_VK_VALIDATION` now set this too; none of
  them could fail on validation before. **Errors only**: a performance warning
  describes a correct frame, so it still only logs, and the shell harnesses stay
  the stricter gate. **Off by default**, including in a debug build where
  validation itself is on, so a developer's `cargo run` still reaches the frame
  that caused the error. `ValidationSink::take_error` is the new drain;
  `ValidationSink::report` is unaffected and still shows every message, taken or
  not. **A run that asks for the gate and cannot have it is refused**:
  `VkInstance::open` returns `OpenError::FatalValidationUnavailable` when the
  variable is set and the layer is not installed, rather than starting with the
  gate silently absent. Every other caller still gets the existing warning and a
  working engine.
- **`crcbl-vk` says out loud when validation is on, and the harnesses that run a
  binary now fail on what it reports.** A run with `CRCBL_VK_VALIDATION=1` logs
  `crcbl-vk: validation enabled (VK_LAYER_KHRONOS_validation), …` at info from
  `crcbl_vk::debug`, emitted only once the debug messenger really exists. That
  line is the half of `ValidationReport::assert_clean` a shell script can read:
  `tools/run-samples-windowed.sh` and both shell e2e harnesses now refuse a
  clean log that does not carry it, and fail on any error or warning the
  messenger produced. Before this, eight sites set the variable and no run could
  fail because of it.
- **`crcbl::session::Loopback::impaired`** builds the single-player pair with
  both directions behind `crcbl_net`'s `ConditionSimulator`, so a game can be
  played over a link with loss, latency, jitter or reordering on it and the run
  reproduces from one seed. The two ends are seeded differently — one seed for
  both would sample one impairment pattern twice rather than two — and the
  caller's seed still reproduces the whole run. `Loopback` is now generic over
  its transport with `InMemoryTransport` as the default, so every existing
  `Loopback::new` call site is unchanged.
- **`crcbl_net::Clock`, with `SystemClock` and `ManualClock`.**
  `ConditionSimulator` schedules delayed messages against a clock rather than
  against `Instant::now`, and `ConditionSimulator::with_clock` /
  `Loopback::impaired_on_a_manual_clock` hand a test one it drives by hand.
  Cloned `ManualClock` handles share one time, so both ends of a link move
  together. `ConditionSimulator::new` is unchanged and still reads the wall
  clock. What this buys: a latency run that spends no wall time and reproduces
  exactly from its seed — breakout's impairment sweep went from about three
  minutes and a different answer every run to 2.2 seconds and the same one.

- **Bloom.** `docs/plan/18-render-features.md`'s P10 chain: a threshold-free
  partial-Karis downsample pyramid, a 3×3 tent upsample added back down it, and
  an additive composite, recorded between `ssr-blur` and `tonemap`.
  `RenderEffects::BLOOM` is the toggle and `crcbl::settings`' `bloom` key is the
  player's. **It is off unless a view asks for it**:
  `RenderEffects::DEFAULT_STACK` — what `EffectRequest::default()`'s `camera`
  now holds — is every effect but this one, because bloom is a property of a
  lens rather than of the scene's light transport, and a camera given no render
  stack has been given no lens. No frame the engine already drew has changed.
  The chain's length comes from the target's extent — six levels at 1080p, none
  at all below sixteen pixels on an axis — so `ForwardRenderer::MAX_PASSES`
  carries a ceiling for it rather than a count. New shaders `bloom_down`,
  `bloom_up` and `bloom_composite`; new fixture `Scene::Bloom` and the `bloom`
  golden, whose check compares the floor beside the emitter against its mirror
  on the other side of the frame, so a chain that returned its input or a
  uniform blur cannot pass it. The browser gate draws it too — `bloom` is in
  `apps/render-harness`'s scene list, so `crcbl-webgpu` renders the chain in a
  real browser and is held against the same golden Vulkan is.

- **The server consumes client input.** `Server` holds the input frames that
  arrived since the last tick — each with the `TickId` its client stamped it
  with — and hands them to `GameModule::tick` as a `ClientInputs` view, in
  arrival order. The queue is emptied at the start of every tick, so a frame is
  offered to exactly one module call and holding it for a later one is the
  jitter buffer this deliberately is not. It is bounded by
  `crcbl_server::MAX_CLIENT_INPUTS_PER_TICK` — twice
  `DEFAULT_MAX_CATCH_UP_TICKS`, because how many arrive in one tick is the
  peer's choice — and frames past the cap are refused newest-first, so the
  module reads the order the client sent rather than a reordered prefix.
  `ClientInputs::dropped` tells the module and `Server::dropped_input_count`
  tells an operator. Aligning an input to the tick it names is still absent, and
  the code says so where a reader will look.

- **`apps/asteroids`, `apps/flappy` and `apps/horde` play over the wire too.**
  Each one's `Intent` gained `from_wire` and `from_inputs`, so the bytes the
  facade hands `Client::set_input` are the bytes its module decodes out of
  `ClientInputs` inside the server's tick, and the `Arc<Mutex<GameLogic>>` each
  shares with its module is output-only. The decode validates rather than
  trusts: a payload of the wrong length, or a bit outside the flag mask that
  build defines, is refused — and horde's level-up choice field is exactly as
  wide as its offer, held by a compile-time assertion, so no byte a peer can
  send names an upgrade that does not exist. Frames that pile up in one tick
  fold per field: a held direction takes the latest frame's word, an edge (fire,
  flap, restart) survives from whichever frame raised it, and horde's choice
  takes the first frame that names one. Each game sends before it simulates and
  spends one tick on the handshake as it is built, so the first thing a player
  presses is the first thing the simulation sees. `apps/asteroids` deals its
  board after that tick: unlike breakout's, its field moves on every tick.

- **`apps/breakout` plays over the wire.** Its `Intent` gained `from_wire` — the
  first decode half any sample has — and the paddle, the launch and the restart
  are driven by bytes that were sealed, sent over `InMemoryTransport`, opened
  and decoded inside the server's tick. The `Arc<Mutex<GameLogic>>` it shares
  with its module is output-only: nothing reaches the simulation except over the
  transport.

- **`crcbl-webgpu` reports what the browser never let go of at teardown.** The
  other three backends warn from their device's destructor —
  `N object(s) still alive at device teardown (…)` — and their e2e runners fail
  on that line; a command stream has no destructor to hang it on, so the report
  is made where the objects actually live. A new wasm export,
  `__crcbl_web_gpu_stream_ended`, answers `1` once the release that drained the
  run's last frame has let the retained channel go — which a stream length of
  zero cannot say, since that is also what a page sees before the engine boots.
  `takeCommandStream` reads it and marks the frame, and the replayer writes the
  same line, in the same words, **after** that frame's own destroys have run.
  `Replayer#teardownReport` is the receipt: `null` until the stream ends, then
  the list it reported on, so a clean run can be told from a reporter that never
  fired. `web/run-browser-e2e.sh` fails on the line, as `run-vk-e2e.sh` does.

- **A player's `[engine.video]` settings reach every sample that has effects to
  clamp.** `lantern` already did; `viewer`, `quarry` and `sandbox` now hand
  `GpuContext::effect_request` to their renderer as it opens, and `viewer`
  carries the request across a document reload beside the exposure and the
  wireframe. Nothing else in `apps/` builds a `ForwardRenderer`, so nothing else
  has an effect a settings file could turn off. The layer only clamps downward:
  a run with no settings file draws what it drew before.

- **`crcbl-scene` has a `gltf-fixture` feature.** It compiles
  `crcbl_scene::gltf_fixture`'s triangle document and `.glb` container outside
  `cfg(test)` so another crate can build one — `BIN_CHUNK_BUFFER`,
  `triangle_bin`, `triangle_json` and `glb`, which together make one whole
  document and nothing more. It adds no dependency and changes nothing a default
  build compiles. **Not engine API**: it exists so a harness that has to hand a
  real `.glb` to a tool has one without a binary blob being committed, and
  `tools/run-samples-windowed.sh` now runs `viewer` windowed against what the
  `write-triangle-glb` example writes.

- **`crcbl sim` runs the determinism harness.**
  `crcbl sim [--ticks N] [--tick-rate HZ] [--seed SEED] [--json]` simulates a
  headless server over a seed-generated world and prints
  `hash:<hex> ticks:<n> final_tick:<n>` — the same input gives the same hash,
  and a hash that moves between two runs of one build is the harness reporting
  what it exists to catch. This replaces the `apps/sim` binary, which is
  **deleted**: its tests are ported against the `crcbl` binary.
  `docs/plan/11-cli-headless.md` sketches a scene argument and
  `--input script.ron`, and **neither is built** — this tree has no scene format
  and no RON reader — so both are refused by name rather than ignored.

- **`crcbl settings get|set|list` reaches a player's `settings.toml`.** A
  setting is scriptable now, not only reachable from a settings screen. The file
  is `<config dir>/<app>/settings.toml`; `--app` names the game and defaults to
  the project in the current directory, and `--config-dir` stands in for the
  platform's config directory, which is what a test or a CI job that must not
  touch a real `~/.config` needs, since `dirs` is not redirectable through the
  environment everywhere. A value is typed by TOML's own grammar rather than by
  a flag, and `--` ends the options so a value can start with `-`. Unlike a
  game's start-up, which turns an unreadable settings file into an empty layer
  and a log line, this verb **fails** on a file that will not parse: a `set` on
  top of an empty layer would serialise the player's file away.

- **`crcbl import <gltf> [--json]` runs the glTF importer standalone.** It
  reports what came out of one document — meshes, the primitives across them,
  materials, images, every entry of the `nodes` array, and the instances, one
  per node that draws a mesh — from a single list, so the human line and the
  `--json` object cannot carry different numbers. What the importer _skipped_ is
  not a second report: the verb installs the engine's stderr logger, so the
  warnings `import_gltf` already emits for an unsupported extension, an image
  whose URI will not resolve, or a primitive that is not a triangle list land
  beside the counts. A skip is not a failure — the run exits 0 and the skipped
  item is still counted. **`--out <dir>` is not built and is refused by name**,
  with the reason: the importer produces an in-memory `GltfScene` and this tree
  has no on-disk scene format to write one to.

- **A player's `[engine.video]` settings now reach the frame.** `GpuContext`
  reads the player's `settings.toml` while it opens — `SettingsSource::Platform`
  by default, from the directory `GpuContextDesc::label` names, so a game gets
  this without asking — and `GpuContext::effect_request` hands the layer to a
  renderer built on that context. `crcbl::settings::VIDEO_KEYS` is the one place
  a key is spelled (`shadows`, `ambient_occlusion`, `reflections` under
  `[engine.video]`), and the layer only ever clamps downward: a key that is
  absent, `true`, or holding something that is not a boolean all leave the
  effect standing, so a settings file cannot switch on what the view never asked
  for. `SettingsSource::None` is what a run with no player — a golden
  comparison, a benchmark, a determinism harness — asks for instead.
  `crcbl-store` gained `SettingsStack::platform`, `SettingsStack::from_storage`
  and `NativeStorage::config_root`, which resolves the platform config directory
  without creating it.

- **X11 answers ICCCM's `MULTIPLE` selection target.** A peer that wants several
  formats can write one `ATOM_PAIR` list and ask once, instead of a
  `ConvertSelection` per format; each pair is converted into the property that
  pair names, and a pair the owner cannot serve comes back with its property
  atom replaced by `None`. `MULTIPLE` is advertised in `TARGETS`, which is how a
  peer knows to use it.

- **A device with more than two channels says so.** `AudioStream::open` logs the
  channel count it found when it exceeds the mixer's two, naming how many stay
  silent. The mixer feeds the first two channels and leaves the rest alone — on
  5.1 that is centre, LFE and both surrounds — because `cpal` reports a channel
  count and not a layout, so nothing can know which index is the centre.
  `fill_audio`'s docs now say that, and a test pins it.

- **Every read-only physics query has a shared, `Sync` form.** `OverlapQueries`
  (colliders) and `EntityOverlapQueries` (entities) already carried
  `overlap_sphere_into`; they now also carry `overlap_aabb_into`, `cast_ray` and
  `sweep_sphere`, with `sweep_sphere_excluding` at the collider layer. Each
  takes `&self` and a caller-owned `QueryScratch`, so a data-parallel pass hands
  one view to every chunk and gives each chunk its own buffers.

  The exclusive forms are unchanged in behaviour and now delegate to the same
  `*_core` the shared forms use, so there is no second copy of a traversal for
  the two to disagree in. A side effect worth knowing: `PhysicsWorld::cast_ray`,
  `overlap_aabb` and `sweep_sphere_excluding` no longer allocate per call —
  `cast_ray` used to build a traversal stack and a hit vector on every cast.

- **A physics query result becomes an array subscript.** `ColliderId::index` is
  public, so turning a query result back into "which of my bodies is this" costs
  an index rather than a `HashMap` lookup. Its doc states what the slot promises
  and what it does not: stable while the collider lives, bounded by slots ever
  allocated rather than by `PhysicsWorld::len`, not dense once anything has been
  removed, and not an identity — a recycled slot names a different collider and
  only the generation parts them.

- **`BroadphaseStats` says whether a phase refit or rebuilt.** Three lifetime
  totals that never reset — `refits`, `updates_without_refit` and `rebuilds` —
  so two readings either side of a phase subtract to that phase's own numbers.
  Worth knowing, and now pinned by a test: `Bvh::update_aabb` never refuses a
  new box for being far from the old one, so a body that teleports across the
  world still refits and pays for the trip in a looser tree, not a rebuild.

- **`crcbl bench --scenario phys`** times `crcbl-phys`'s broadphase on one
  thread, as three separately reported phases: building a tree over `--bodies`
  spheres, refitting it after every one of them moves a tick's worth, and then
  one overlap query per body. `--extent` is the density control — the same crowd
  in a smaller square arena — and the run reports the neighbours per query it
  actually found, so the query timing can be read against the answer size rather
  than against the body count. A flag belonging to the other scenario is refused
  by name rather than ignored.

  Every pass, warm-up included, is held against an `O(bodies²)` scan with no
  tree in it: a pass that answered nothing, answered short, or answered the
  right total across the wrong queries fails the run instead of reporting a fast
  number.

  Every timed pass now folds _which_ bodies answered, not just how many — the
  case a total and a per-query shape cannot see. Measured cost of putting that
  in the timed loop: about two percent on the query phase, paid identically by
  every run — a debug-build figure, and the one number in this entry that has
  not been re-taken on a release build. The output also reports one iteration's
  refits, updates left for a rebuild, and tree builds, so a run says whether its
  refit phase refit.

  `--ticks` ages the tree before the query phase: N drift-and-refit steps, the
  last one timed, defaulting to 1 so an existing invocation reports what it
  always did. It answers a question nobody had measured — a BVH that only ever
  refits never re-picks a leaf. Measured on a release build at `--bodies 2000`,
  `--iterations 20`, two runs each: the tree is structurally identical at 1 tick
  and at 100000 (3999 nodes, depth 12, one build, both runs), while the query
  phase's p50 rises from 0.65 ms to 5.27 ms — a little over eight-fold — and it
  does that while answering **less**, 3.977 neighbours per query against 5.962.
  A crowd that has walked away from its tree costs more per answer and gives
  fewer. The refit phase does not follow: its p50 falls from 0.125 ms to 0.099
  ms over the same span. So a game would see this in its broadphase queries and
  never in its physics tick.

- **`crcbl bench --scenario jobs`** times `crcbl_jobs::Pool::par_for` over a
  fixed synthetic workload and reports it as a distribution: p50, p95, p99 and
  max, never a mean, with the pool's own counters beside them and the
  environment the numbers came from. `--workers 0` is the serial baseline the
  parallel figures only mean something against; `--chunk`, `--items`,
  `--iterations` and `--warmup` are the knobs. Warm-up iterations run before the
  counters are reset and are excluded from the statistics, and the run fails
  rather than reports if the warm-up ran no chunks. Below the sample count at
  which a nearest-rank p95 is simply the maximum, it prints the maximum and says
  why there is no percentile. Human output by default and `--json` on request,
  like every other subcommand.

  The workload's result is used — each item is written back, folded into a
  checksum the output carries, and compared against a serial pass over the same
  seeds — so an elided loop, a chunk that ran twice and a chunk that never ran
  are each a failure rather than a fast number.

- **`Pool::stats` reports what the job pool actually did**, so a phase that
  adopts `crcbl-jobs` can show the adoption helped rather than assert it.
  `PoolStats` carries the chunks the driver ran against the chunks the workers
  ran (which sum, between submissions, to every chunk a completed `par_for`
  split into), successful steals, searches that found the deque empty, searches
  that lost the exchange for an item that was really there, worker parks, the
  largest burst any one submission queued, and the number of submissions.
  `Pool::reset_stats` zeroes them so a reading covers one phase.

  Every counter is a `Relaxed` atomic nothing inside the pool reads back, so
  none of them joins the happens-before `par_for` rests on, and they are counted
  per chunk — a `par_for` over ten thousand items costs the driver two atomic
  writes for the whole call. A reading taken mid-call is torn across its fields
  by construction, which `PoolStats` documents rather than preventing with a
  lock: instrumentation that changes the schedule ends up measuring itself.

  Steal retries are counted apart from steal failures on purpose. Folding them
  together would report a deque busy enough that thieves collide over it as an
  idle one — the opposite reading.

- **The Web Worker spawn backend now runs in a real browser, and
  `web/run-jobs-e2e.sh` is the gate that says so.** `crcbl_jobs::workers` has
  had the queue-and-drain ABI and a `node:worker_threads` gate over it since it
  landed; what did not exist was a page. `web/jobs/` is that page — `host.js`
  announces the host through `__crcbl_web_jobs_host_ready`, drains the spawn
  queue, and starts one `Worker` per request; `worker.js` is the five-step
  bring-up (instantiate against the shared memory, write `__stack_pointer`, call
  `__wasm_init_tls`, report, enter) `crates/crcbl-jobs/src/workers.rs`
  specifies. The gate asserts a chunk ran on a thread that is not the driver, on
  a stack of its own and with thread-locals of its own, in headless Chromium.

  **Four claims are only answerable in a browser**, and this is now the only
  place any of them is asked: that a `Worker` takes a structured-cloned
  `WebAssembly.Module` and a shared `WebAssembly.Memory`; that the memory can be
  constructed at all, which is a property of the document rather than of the
  build; that a page's **main thread** survives driving a pool whose workers
  park on `memory.atomic.wait32` — measured at 3000 `par_for` calls with eight
  workers up, with no trap, where `docs/backlog.md` had predicted one; and that
  a **non-threaded** artifact, the shape every published one has, is refused
  workers rather than announced. Four red switches in the page's query string
  break one step each, and the script insists the right assertion went red and
  the others did not.

  `web/build.sh --threads --gate-only` builds just the artifact the browser run
  drives, so it costs one `-Z build-std` example rather than seven demo builds.
  Nothing threaded is published: `web/jobs` is pruned from the site copy,
  because a page loading an artifact that imports a shared `env.memory` could
  only fail on an origin that sends no COOP/COEP pair, which is every GitHub
  Pages origin. The host half lives at `web/engine/jobs.js` and the bring-up at
  `web/engine/jobs-worker.js`, beside the rest of the shim, because the entry
  below needs the same code from a demo's loader.

- **A sample's simulation runs off the main thread in a browser, and
  `web/run-horde-threads-e2e.sh` is the gate that says so.** This is the claim
  the entry above cannot make: that page has no engine on it. This one drives
  `demos/horde/` — the page a visitor loads and the shim a visitor runs — on a
  threaded site, and asserts that horde's steering pass ran chunks on a Web
  Worker.

  **`apps/horde` grew the three exports that make it observable.**
  `__crcbl_horde_sim_threads` counts the distinct threads that have run a
  `steer_enemies` chunk and `__crcbl_horde_sim_workers` reports the pool's
  worker count — the same evidence
  `steering_is_bit_identical_however_many_workers_run_it` takes from its probe
  pass. They exist because nothing else in the sample's ABI can distinguish the
  two runs: `steer_enemies` is bit-identical at any worker count **by
  construction**, so a threaded run and an inline one draw the same frames and
  every other check passes either way. `__crcbl_horde_prefill` is the third, and
  it is `--prefill` reachable from a page (`?prefill=N`), through `assemble`'s
  own `stage_field` call: `Pool::par_for` runs a single chunk inline whatever
  the pool holds, and `STEER_CHUNK` is 64 enemies, so a demo with a small field
  never leaves the main thread whatever the workers are doing.

  **`web/build.sh --threads` now assembles a whole site** into
  `target/site-threaded/` — the same pages, the same `web/engine/`, the same
  `web/demos/<name>/main.js`, with the worker-capable artifact beside each demo
  and the new `web/tools/wasm-loader-threads.js` as its `<lib>.js`, which
  constructs the shared memory, announces through `WorkerHost` and drains the
  spawn queue. `--threads --serve` serves it cross-origin isolated. It is never
  `target/site/`, the directory the Pages workflow uploads.

  **Three red checks, and the third is also the compatibility proof.**
  `?no-host-ready` leaves the announcement out, so the demo plays and steers
  entirely on its own thread; `--prefill 0` leaves horde at its title screen, so
  nothing is steered at all; and the third runs the same gate against **the
  published site**, which must fail the thread assertion and pass everything
  else. Measured: on the threaded site horde steers on four threads with a pool
  of three workers, and on the published site on one with a pool of none.

- **`crcbl_lantern::room::spot` puts a spot light in lantern's room**, and
  `room::lights` is the one list both of the sample's views and its golden suite
  feed to `set_lights` — the sample used to spell `[room::lamp(t)]` at each of
  those three call sites. It is a cool downlight over the room's back-left
  corner, which is the one part of the room neither the sun (blocked by the wall
  below the window's sill) nor the orbiting lamp (out of `LAMP_REACH` from every
  point on its orbit) reaches, so its cone and its penumbra are legible against
  what was there rather than added to it. `apps/lantern/tests/golden/room.png`
  and `live.png` are re-blessed for it.

  **Both of the room's punctual lights are shadowed**, on every frame of the
  lamp's orbit and from a run started at any phase of it — the light region grew
  a seventh tile for exactly this (see `Changed` below), so the lamp's cube and
  the downlight's map are held side by side.
  `room::the_shadow_atlas_holds_both_punctual_lights_on_every_frame_of_the_orbit`
  is what asserts it, and it fails to compile if the region is ever shortened
  back. Adding the light moved no read point in the room: every measured value
  in the golden suite was identical to the run before it.

- **A corner post stands in that downlight's cone, so the third shadow is in the
  picture.** A tile in the atlas is not a shadow on screen — the cone fell where
  nothing stood, so its map was rendered on every frame and occluded nothing a
  camera could see, and a frame drawn with the spot holding a tile was
  byte-identical to one drawn with it holding none. `POST_MESH`'s slender post
  is what stands in it now, and two new public read points on the `-x` wall,
  `room::SPOT_LIT` and `room::SPOT_SHADOWED`, are the pair its shadow divides —
  one material row, one normal, one of them with the post between it and the
  fitting.

  That wall face is the one surface in the room the downlight lights alone (the
  sun is back-facing to it, the lamp at `t = 0` is past its own reach), so
  `apps/lantern/tests/golden.rs`'s shadow toggle measures the spot's own map and
  nothing else: the shadowed block reads 50.9 with the atlas and 105.2 without
  it on radv, 51.0 → 105.2 on lavapipe, while the lit block in the same pool
  does not move at all. With the post unplaced the same block reads 112.5 →
  112.5, which is what the claim exists to catch.
  `room::the_downlight_casts_a_shadow_of_the_corner_post` holds the geometry
  behind it with no GPU. `apps/lantern/tests/golden/room.png` and `live.png` are
  re-blessed for it, and the frame costs 0.008 ms more on a 0.51 ms frame with
  no new pass.

- **`crcbl_hal::null::NullInstance::with_adapters` and
  `Recorder::refuse_surface_on`** let a null instance list more than one adapter
  and make a named one answer `HalError::Unsupported` for a surface — the
  variant `crcbl-vk` returns for the same condition. Together they reach the
  engine's surface-aware adapter walk, which until now no test could drive: the
  case it exists for is a discrete GPU that enumerates first and cannot present
  to the window while a software rasteriser behind it can.

- **`crcbl_hal::null::Recorder::report_present_wait_timeouts` and
  `NullInstance::with_present_feedback`** make a null device claim
  `Features::PRESENT_FEEDBACK` and answer a present wait with
  `SurfaceError::Timeout`. They land together because the seam requires an
  immediate `Ok(())` from a device without that feature, so a null timeout was
  illegal until a null device could claim it — and together they reach the
  engine's render-the-frame-anyway answer to a display that fell behind, which
  no test could get to before.

- **`crcbl_hal::null::Recorder::report_suboptimal_acquires`** makes a null
  device hand back an `AcquiredFrame` with `suboptimal` set, which it could not
  do before — the field was a hardcoded `false`, so the engine's
  reconfigure-after-present policy was reachable from no test. It is _counted_
  rather than latched, unlike `report_swapchain_out_of_date` beside it: a
  suboptimal swapchain still presents and the engine answers it by
  reconfiguring, so a latched one would rebuild every frame for as long as a
  loop ran.

- **`crcbl_audio::wav::encode`** writes a `WavFile` back out as an IEEE-float
  WAV (`format = 3`) — the inverse of `wav::decode`, which until now was the
  only direction the module went. It writes f32 and nothing else: every integer
  depth is a quantisation, and what this exists to write are reference waveforms
  that get decoded and compared against, where rounding on the way out puts the
  error in the file instead of in the thing under test. It refuses the two files
  `decode` would refuse to read back — `WavError::MissingFmt` for no channels,
  `WavError::MissingData` for no samples — and `WavError` gains
  `Unrepresentable`, for a buffer larger than RIFF's fixed-width size fields can
  describe. An exhaustive `match` on `WavError` therefore needs one more arm. A
  non-finite sample is written through as it stands and still comes back as
  silence, because `decode` is where NaN and the infinities are stopped.

- **`crcbl_webgpu::probe` gained the texture-sampling fixture and its shims**,
  and with them the first probe in this crate whose shader binds anything at
  all. `PROBE_TEXTURE_SAMPLE_*` uploads a two-by-two `rgba8unorm` source with
  four different texels, binds it with a nearest sampler through a
  `BindingKind::SampledImage`/`BindingKind::Sampler` pair, samples it across a
  fullscreen quad and reads the target back; the browser gate's group **AJ**
  holds each quadrant against the source texel from the same corner. Until now
  the seam could describe a sampled binding and nothing had ever shown one
  reaching a fragment shader in a real browser — `SampledImage` appeared in this
  crate only inside a bind-group _layout_, an object WebGPU reports nothing
  about but its label. The four colours are chosen so that no channel
  permutation maps any of them onto any other and no alpha is `0` or `255`,
  which is what makes a swapped channel or a dropped alpha fail rather than
  pass.

- **`crcbl_webgpu::probe` gained the BC1 fixture and its shims**, and group
  **AK** with them — the contrast group for `texture-compression-bc`, the last
  mapped WebGPU feature whose bit was reported to callers on the strength of a
  node stub. It uploads an 8×8 `bc1-rgba-unorm` source as four blocks, one per
  quadrant, and holds each decoded quadrant against its block's endpoint byte
  for byte. The endpoints are cube corners because that is the only thing the
  specifications make exact: D3D 11.3 §19.5.2 permits a decode tolerance across
  every channel of every texel and requires bit accuracy only of BC6H and BC7,
  its one exactness clause being for values decoding to `0.0` or `1.0` — which
  are also the only values where bit replication and Khronos Data Format 1.3
  §18.1's rational agree. A mid-tone endpoint would have been a gate that can
  fail a conformant GPU. `probe_device_desc` now asks for
  `Features::TEXTURE_COMPRESSION_BC` optionally, so a device that has it opens
  with it.

- **`crcbl_webgpu::probe` gained the pass-span fixture and group AL**, the
  contrast group for `timestamp-query` and the last mapped WebGPU feature
  without one. Two submissions each hold four empty compute passes and four that
  dispatch a dependent-arithmetic loop, every pass timed through its own
  descriptor, and the group asserts a **separation** rather than a duration:
  every busy pass outspans every empty pass in the same frame, and the whole
  boundary array is non-decreasing across passes and across the submission
  boundary. **No nanosecond constant enters either claim**, which is what lets
  it survive a browser that quantises — rounding moves both sides. The workgroup
  count was chosen from a measured sweep rather than picked. `probe_device_desc`
  asks for `Features::TIMESTAMP_QUERY` optionally, and the group's fourth check
  holds the browser's own refusal of a `'timestamp'` query set against the
  probe's answer, so a device without the feature cannot report as covered.

### Changed

- **A rustc warning now fails CI on every target, the way clippy and rustfmt
  already did.** Clippy's `-D warnings` only covered what clippy was pointed at;
  `cargo build`, `cargo check`, `cargo test`, `nextest`, the e2e harnesses and
  the wasm builds all compiled with warnings merely printed. That mattered most
  where it was least visible: a backend's platform half is compiled by exactly
  one job, so `crcbl-mtl`'s warnings existed only on macOS, `crcbl-dx12`'s only
  on Windows and `crcbl-webgpu`'s only on wasm32 — a line in a log nobody opens.
  `RUSTFLAGS: -D warnings` is set for the whole workflow, which reaches this
  workspace's crates and not its dependencies, since cargo caps lints on
  registry and git dependencies. `web/build.sh` now keeps an inherited
  `RUSTFLAGS` in front of the flags it adds instead of replacing it, which had
  taken the gate off the threaded wasm build — the one build that compiles the
  atomics path.

### Fixed

- **A frame's closing barrier before a present now names a pipeline stage.**
  `ResourceState::Present` expanded to `VK_PIPELINE_STAGE_2_NONE` on Vulkan, and
  that state is the destination scope of the transition into
  `VK_IMAGE_LAYOUT_PRESENT_SRC_KHR` — so synchronisation validation reported
  `SYNC-HAZARD-PRESENT-AFTER-WRITE` with `write_barriers: 0` against every
  windowed frame, 480 of them in a 120-frame sandbox run. It names
  `ALL_COMMANDS` now, which is correct in both scopes and costs nothing on a
  barrier with no work after it. Only ever visible with sync validation on,
  which no windowed CI step sets — `docs/backlog.md` records what is left there.
- **The three offscreen GPU suites now fail when the device reports a failure
  its return values did not carry.** `render_e2e`, `tiling_e2e` and `gltf_e2e`
  were the lavapipe CI steps a Vulkan validation error could not fail: a frame
  the layer had already refused was compared against its golden, matched, and
  exited 0. `OffscreenSetup::finish` drains `Device::take_error` after the
  teardown and reports it as the new `OffscreenError::DeviceReported`, and the
  suites assert on it through a shared `tests/offscreen/verdict.rs` fixture
  whose `Drop` also prints the device's verdict when a test panics before
  reaching `finish`. `apps/lantern`'s golden test and the screenshot CLI get the
  same check, since both go through `finish`. Verified with
  `CRCBL_VK_VALIDATION_SELF_TEST=1` against CI's own layer: all three exited 0
  before and fail now, and all three stay green with nothing injected.
- **`ConditionSimulator`'s reliable channel is now actually reliable.** It
  implements `Transport`, whose `send_reliable` promises ordered, lossless
  delivery, and it was applying its loss, duplication and reorder draws to both
  channels — so a handshake could be dropped, duplicated or delivered out of
  order by the thing standing in for a transport that does none of those. The
  layering is now a real stack's: a lossy wire underneath, and the reliability
  the transport provides on top. On `send_reliable` a loss draw costs a
  retransmission instead of the message (500 ms, doubling per attempt, after
  ENet's `ENET_PEER_DEFAULT_ROUND_TRIP_TIME` and its `roundTripTimeout *= 2`),
  the duplicate draw is not taken at all, and a message is never released before
  the one in front of it — head-of-line blocking, which is what stops jitter
  from permuting the channel. Past five retransmissions the send returns
  `TransportError::Disconnected`: a link that cannot deliver is a dead link, and
  `loss_rate` may be 1.0. `send_unreliable` is unchanged, and `SimConditions`'
  field docs now say which channel each knob is about.
- **Every lavapipe CI step now fails on a validation error, not just the seven
  that ran a sample binary.** The other seventeen — the agnostic e2e harnesses
  and every sample golden — inherited validation from the debug build profile
  alone: on, reported to a log, and fatal to nothing, which is a layer message
  in every CI log and in no CI result. They all set `CRCBL_VK_VALIDATION`,
  `CRCBL_VK_SYNC_VALIDATION` and `CRCBL_VK_VALIDATION_FATAL` now, and all
  fifteen harnesses were run against CI's own layer 1.3.275 and Mesa 25.2.8 with
  the full gate on before the change landed. The Windows leg and the coverage
  job are excluded, each with the reason beside it in `ci.yml`.
- **A GPU error raised on the way out no longer exits 0.** `Device::take_error`
  was drained only at the top of a frame, so everything after the last one — the
  final submit completing, the final `wait_idle`, the swapchain and surface
  teardown — had no reader, and a run that violated the specification while
  shutting down reported success. `GpuContext::destroy` now drains it once more
  after the destroys and returns `GpuError::Hal` if anything was there.
- **A read-only depth attachment no longer loses the barrier that orders its
  store.** `ResourceState::DepthStencilRead` expanded to a read-only Vulkan
  access mask, but an attachment in that state still performs
  `VK_ATTACHMENT_STORE_OP_STORE` when the pass ends — so when a depth-test-only
  pass was the last to touch an image, the next frame's layout transition was a
  write whose source scope was a pure read and could not order against it.
  Validation reported it as a cross-submission `SYNC-HAZARD-WRITE-AFTER-WRITE`
  with `write_barriers: 0`, and it reached every frame the viewer drew.
  `DepthStencilRead` now carries `DEPTH_STENCIL_ATTACHMENT_WRITE`, and
  `ResourceState::is_write` returns `true` for it, so the graph no longer elides
  the barrier between two such passes either. No golden moved. The viewer's CI
  step gets `CRCBL_VK_VALIDATION_FATAL` back, so all seven now have it.
- **`ConditionSimulator` no longer loses a message the inner transport
  refused.** Latency turns a steady stream into a burst on release, so the
  simulator is what fills a bounded channel — and both send paths threw the
  result of that forwarding away, so the loss arrived looking like the
  configured loss rate had eaten it. A refused message now stays queued and goes
  out on the next drain, and the refusal reaches the caller at the next hand-off
  as `TransportError::Backpressure`, where it is unambiguous: that message was
  not taken, so retrying it cannot duplicate it. `send_reliable`,
  `send_unreliable`, `recv` and `recv_reliable` can therefore all return an
  error the simulator previously hid.
- **`lantern`'s effect rows named every effect but bloom.** The row came from a
  hand-written table of three in `apps/lantern`, so a frame drawn with the bloom
  chain reported a row with no bloom in it. It now goes through
  `RenderEffects::row`, whose name table is as long as the type has bits — an
  effect added and left unnamed is a compile error rather than a row that
  quietly stops mentioning it.
- **breakout's session handshake waits for the client, not only the server.**
  The server considers itself connected the moment it reads the hello; the
  client only when the result reaches it back, and until then it holds no
  session key and discards every input frame it is asked to send. Over a
  loopback link the two land in the same tick, so no shipped run changes; over a
  link with a round trip on it the opening inputs were being posted into a
  keyless client and thrown away.
- **`crcbl lod` reports what the importer skipped.** The engine's stderr logger
  is now installed by `crcbl`'s `main` rather than by `crcbl import`, so every
  verb that drives the glTF importer surfaces its warnings — an unresolvable
  image URI, a required extension this importer lacks, a primitive that is not a
  triangle list. `crcbl lod` previously ran the same importer in silence, and a
  document whose textures were never going to load looked identical to one whose
  did.
- **A WebGPU frame gives back the handles it acquires.** `acquire_next_frame`
  minted a fresh image and image-view handle every frame and never retired the
  pair from the frame before, so a browser-side handle table grew for as long as
  the application ran. The device now tracks the outstanding pair per swapchain
  and retires it as the next acquire is encoded; reconfiguring or destroying the
  swapchain retires it too.

- **A WebGPU teardown reaches the page that has to replay it.** The command
  stream is drained by JavaScript, so a device dropped on the wasm side freed
  its channel before the page could read the destroy commands sitting in it —
  the objects those commands named stayed alive in the browser. Dropping a
  `WebGpuDevice` now retains the channel for one more drain, and
  `__crcbl_web_gpu_stream_release` is what finally releases it.

- **A single X11 clipboard request can no longer make this client hold an
  unbounded number of payload copies.** An oversized conversion starts an `INCR`
  transfer that keeps a full copy of the offered bytes until the requestor pulls
  the last chunk or the two-second timeout abandons it, and nothing bounded how
  many could run: one `MULTIPLE` request whose `ATOM_PAIR` list repeats an
  oversized target started one transfer per pair, and that list is bounded only
  by the 64 MiB property cap — so one foreign client could ask for millions of
  copies in one request. `selection::MAX_PENDING_WRITES` caps them at eight, the
  same number the Wayland backend already used, and the refusal is an answer in
  the place ICCCM puts one: a `SelectionNotify` naming no property on the
  ordinary path, a `None` property atom on the `MULTIPLE` path. It is checked
  before the `INCR` header is written, so a refused requestor is never left
  waiting on chunks that will not come.

- **A scaled instance now picks the level of detail for the size it is drawn
  at.** A cluster group's radius and its simplification error are lengths in the
  mesh's own space, and `draw_gen.slang`'s `select_level` put only the group's
  centre through the instance transform — so an instance drawn four times the
  size it was authored at kept a level whose error moves the surface four times
  as far on screen as the metric was told. Both lengths now go through
  `max_stretch` of the instance's 3×3, the same bound the cluster cull already
  used for its bounding sphere, and `mesh_cluster.slang`'s screen-error heatmap
  scales them too so it still describes the cut it is drawn over. Seam-visible
  as a signature change: `MeshLevels::select` and `uniform_level` take a
  `stretch`, and `GroupCost::scaled` is the new host spelling. A rotation or a
  translation gives exactly `1.0`, so nothing already correct moves — every
  render golden is unchanged.

- **The `bare` sample opens at the size its help says.** It hand-rolled the
  `--size` fallback with 640x480 instead of calling
  `crcbl::engine::requested_window_size`, while pasting in the shared `OPTIONS:`
  block whose `--size` line says the default is 960x720 — so the one sample
  written to show the engine driven as a library was the one binary whose window
  disagreed with its own help. It calls the engine's helper now, like every
  other sample, and comes up at 960x720 windowed and headless alike.

- **A non-uniformly scaled instance no longer loses clusters that face the
  camera.** The mesh path's per-cluster back-face cull carried a cluster's cone
  axis through the instance's bare 3×3 — the transform a tangent takes — so a
  rotation composed with a non-uniform scale could put the axis it read more
  than 50° from where the surface actually faces, and the cluster was rejected
  while facing the camera. The cone test is now skipped for any instance whose
  transform does not preserve angles, in `mesh_cluster.slang` and in
  `crcbl_render::cull`'s oracle alike; the bounding sphere still culls there. No
  behaviour changes for a rotation, a uniform scale, or the two composed.

- **A window survives a compositor withdrawing a global.** Wayland's
  `global_remove` was handled for `wl_output` and `wl_seat` only, so every
  singleton — `wl_compositor`, `xdg_wm_base`, the viewporter, the decoration and
  pointer managers, `wl_data_device_manager` — kept a non-null but inert proxy.
  A compositor re-advertising the interface could not rebind it, and the next
  `create_window` marshalled on a destroyed object and disconnected the client.
  Each is now let go the way its own protocol allows, and rebinds when it comes
  back; `create_window` and `clipboard_offer` fail by name while it is gone.

- **X11 `wait_events` no longer sleeps with events already decoded.** A burst
  past the per-pump cap leaves the remainder inside libxcb's own queue, and the
  connection's descriptor has nothing readable for them — so a caller that waits
  for input slept out its whole timeout with events in hand. It now returns at
  once when the last drain stopped at the cap.

- **A peer cannot make the clipboard hold one copy of the selection per
  request.** Each `wl_data_source.send` cost a full copy held until the transfer
  finished or idled out, uncapped, with the number of requests chosen by the
  peer. Eight at a time now, refusing the newest — which closes its descriptor,
  so the peer reads an empty payload instead of blocking on a pipe.

- **Normals survive a non-uniform scale.** The mesh shaders took a normal
  through `GpuInstance::transform`'s bare upper-left 3×3, which is the transform
  a _tangent_ takes, and the field's doc required callers to be rigid to make
  that true. Nothing enforced it and the engine's own scenes broke it. Both
  raster paths now build the cofactor matrix — `normal_basis`, three cross
  products, no extra bytes in the instance buffer — so `InstanceDesc::transform`
  and `GpuInstance::transform` accept any affine matrix. No golden image moved:
  the identity, a uniform scale and an axis-aligned scale on an axis-aligned
  normal are each their own cofactor matrix once the fragment stage normalises,
  and those are the shapes the committed frames are drawn with.

- **A settings file that is not UTF-8 is reported instead of repaired.**
  `StorageSettingsFile::load` decoded with `String::from_utf8_lossy` before
  parsing, so a partial write or a foreign encoding reached the TOML parser as
  replacement characters — often still a valid document, whose truncated values
  the next `save` wrote back. It now returns `StorageError::Other` naming the
  file and the byte offset. TOML is UTF-8 by definition, so this rejects nothing
  a settings file was allowed to contain.

- **`FrameClock::new` names a tick rate it cannot run.** A rate over 1 GHz
  truncates to a zero-nanosecond period, and the panic came from `with_period`
  blaming a `Duration` the caller never wrote. It is now rejected in `new`, by
  the rate, and the `# Panics` section says so.

- **A bind group covering more of a buffer than the slot allows is refused.**
  `Limits::max_uniform_buffer_range` and `Limits::max_storage_buffer_range` were
  reported by every backend and checked by none, so an over-long binding was
  `VUID-VkWriteDescriptorSet-descriptorType-00332`/`-00333` — undefined
  behaviour a validation layer catches and a release driver does not.
  `create_bind_group` and `update_bind_group` now return `InvalidDescriptor`
  naming the binding, the length bound and the limit.
  `BindingResource::WHOLE_BUFFER` is resolved against the buffer's size less the
  offset first, so an over-large buffer bound whole is refused and the same
  buffer bound in pieces is not. `Limits`' doc no longer claims every field is a
  hard ceiling: the two `min_*_buffer_offset_alignment` fields are alignments,
  and `optimal_buffer_copy_offset_alignment` is a preference a copy may ignore.

- **`crcbl_core::log::is_installed` no longer claims an install that was
  rejected.** It answered "did this module build a logger", and
  `try_init_logging` has to build one before offering it to `log::set_logger` —
  which takes a `&'static dyn Log`, so there is no other order. On a process
  where a host application, a test harness or anything else owned the slot
  first, it reported `true` for a logger `log` had refused.

- **`GpuContext::retire_to` no longer destroys a command buffer whose wait was
  not satisfied.** `Device::wait_semaphores` answers `Ok(false)` for a wait it
  did not satisfy — an outcome, not an error — and the result was discarded, so
  the buffer was freed while the device might still have been reading it. It now
  stays queued and the call reports `GpuError::Unusable`. `u64::MAX` did not
  make this unreachable: the seam takes a timeout as a number, and the null
  device answers from its recorded timeline without consulting it.

### Changed

- `quarry`'s `gpu::Paths::of` takes the resolved effect set as a third argument
  and `Paths` carries it as a public field; `sandbox`'s `app::Sandbox::new`
  takes it as a fourth.
- **`percentile_of` and `MIN_PERCENTILE_SAMPLES` moved to `crcbl_core::stats`.**
  They were private to `crcbl_ui::budget`, and `crcbl bench` asks the same
  question of different numbers. `crcbl_ui::budget::MIN_PERCENTILE_SAMPLES` is
  now a re-export, so no caller changes.

- **A simplified face can no longer turn past a right angle from the facing it
  started with.** Flip rejection was per collapse, so a face could rotate all
  the way round across a run of individually-accepted ones — measured at 16 such
  faces on a spiked height field at half the triangles, and none after. The
  check costs about 3% and is not gated on `debug_assertions`. It makes the
  coarsest levels stall slightly sooner, which moved `clusters/dunes.dag` and
  two pinned histograms; the quarry goldens render that DAG and are unchanged.

- **The demo shim copies three buffers it used to hand a browser API directly,
  because a shared `WebAssembly.Memory` is refused where a plain one is not.**
  `readUtf8` in `web/engine/wasm.js`, the entropy seed in `web/engine/demo.js`
  and the command stream's label decode in `web/engine/gpu-stream.js` each built
  a view over `memory.buffer` and passed it to `TextDecoder.decode`,
  `crypto.getRandomValues` or `TextDecoder.decode` again — every one of which
  throws `The provided ArrayBufferView value must not be shared`. Measured
  against a threaded horde: the demo died on its first log line, then on its
  entropy seed, then reported its first `DeviceDesc::label` as invalid UTF-8,
  because that decode's `catch` reported the `TypeError` as the only failure it
  used to have. The published build is unaffected in behaviour and pays three
  small copies.

- **The shadow atlas's light region holds seven tiles rather than six, so a
  scene can shadow a point light and a spot at the same time.**
  `crcbl_shaders::mesh::SHADOW_LIGHT_TILES` goes 6 → 7 against an unchanged
  `SHADOW_POINT_FACES` of 6, which is what leaves a tile over: until now the two
  were the same number, the region was exactly one point light's cube, and
  `crcbl_render::shadow::Selection` had no base left for a spot. That was not a
  lantern problem — every scene with one light of each kind hit it, which is an
  ordinary lighting rig.

  The grid goes from `SHADOW_ATLAS_COLUMNS` × `SHADOW_ATLAS_ROWS` = 4 × 2 to 3 ×
  3 and the atlas from 4096 × 2048 to 3072 × 3072 of `D32Float` — **32 MiB to 36
  MiB**, the cost of the change. The cascades stay tiles `0..CASCADES` in the
  top row at the origins they were blessed at, so no cascade's rasterised tile
  moves. `crcbl_render::shadow::LIGHT_SLOTS` is unchanged at 2: this is not a
  change to how many lights a frame can shadow, only to how many tiles the
  region holds, so no new `DrawGen` is allocated. A _second_ point light still
  does not fit, and still lights without occluding.

  **`FrameUniforms::light_view_proj` gains one `float4x4`, which moves every
  member after it** — the block goes 720 to 784 bytes, and the offsets `slangc`
  emits for the members past it move with it. Both `.slang` copies of the
  constants moved in the same change; `crcbl_shaders::mesh`'s
  `the_cascade_count_matches_the_one_the_shaders_declare` and
  `the_uniform_block_matches_the_offsets_slangc_emits` are what hold the Rust
  and shader sides together, and both were confirmed to fail on a one-sided
  edit. Every committed SPIR-V, WGSL, MSL and DXIL artifact for `mesh.slang` and
  `mesh_cluster.slang` is regenerated with the pinned `slangc` 2026.14 and `dxc`
  1.9.0.1.

- **`crcbl_webgpu::probe::probe_device_desc` now asks for `DEPTH_CLAMP` as well
  as `TIMESTAMP_QUERY`**, both optional, so a browser without
  `depth-clip-control` still opens a device and the probe reports the reason. It
  comes with `probe_clamp_clamped_pipeline_desc` and
  `probe_clamp_clipped_pipeline_desc`, a pair differing only in `depth_clamp`.

  The reason it matters to a caller is what it now proves.
  `Features::DEPTH_CLAMP` has been reported off the browser's
  `depth-clip-control` all along, but every pipeline the probe built set
  `depth_clamp: false`, so no command originating in Rust had ever carried the
  flag to a real browser — the only place the `true` path ran was a Node stub
  that recorded the descriptor it was handed. The browser gate now draws a
  triangle past the far plane through both pipelines and reads back that the
  clamped one kept its fragments and the control one did not, on SwiftShader and
  on hardware. The capability was always answered `Yes`; it is now witnessed.

- **`crcbl-webgpu` answers `Support::Yes` for
  `Capability::IndirectArgumentPaddedStride`**, where it answered `No`. The old
  reason — "WebGPU's drawIndirect reads one tightly packed argument structure
  and has no stride parameter to honour" — is true of the WebGPU call and false
  of this backend: the stride crosses the stream whole and the replayer unrolls
  the draw into one `drawIndirect` per structure at `offset + i * stride`. A
  caller that was avoiding a padded stride on WebGPU, or branching on this
  capability, can stop. The stride must still be a multiple of 4 and at least
  the argument structure's width, which is every backend's rule rather than this
  one's. The matching `DIVERGENCES` row is gone, so `parity_blockers()` is one
  shorter.

- **`crcbl_hal::null` answers a timeline honestly instead of reporting every
  wait satisfied.** `submit` now applies a submission's `SemaphoreSignal`s to
  the timeline as it accepts them — a device that runs no work has already
  finished it — and refuses a signal that does not move a timeline forwards,
  with the same `HalError::InvalidDescriptor` `signal_semaphore` uses; a refused
  submit records no event. `wait_semaphores` compares against the value the
  device tracks and returns `Ok(false)` for one nothing has signalled, where it
  used to return `Ok(true)` for every wait. A test that waited on a value
  nothing reaches was passing on the strength of that answer and now fails,
  which is the point: the null device is what a caller checks its own sequencing
  against. Binary semaphores in a signal list are recorded and skipped, not
  refused — the engine's frame loop signals its present semaphore that way every
  frame.

### Breaking

- **`GameModule::tick` takes the tick's client input.** The signature is
  `fn tick(&mut self, world: &mut World, inputs: ClientInputs<'_>)`. Every
  implementation adds the parameter; one that ignores input names it `_inputs`.
  `ClientInputs` is exported from `crcbl-ecs`, so `crcbl::ecs::ClientInputs`.

- **`crcbl_hal::null::Event::PresentWaited` carries a `timed_out` flag.**
  `Event` is not `#[non_exhaustive]`, so a `match` that destructures that
  variant by field needs the new one. It exists because the event stream could
  not otherwise tell a wait that lapsed from one that did not: an engine that
  renders the frame anyway — which is the documented policy — leaves the same
  acquire and the same present either way.

- **`Device::wait_semaphores` refuses a binary semaphore**, and the null backend
  now does so too. A host wait has no value to compare against on a semaphore
  that carries none, which is why `signal_semaphore` already refused the mirror
  case; `crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` all answered
  `HalError::Unsupported` for it already, and `crcbl-hal`'s `NullDevice`
  answered `Ok(true)` — telling a caller that a wait no real device accepts had
  been satisfied. The seam's `# Errors` now names it. A caller that waited on a
  binary semaphore against the null device and read the `bool` gets an `Err`
  instead; one that was waiting on a real device was already getting it.

- **`PhysicsSystem::sweep_body` no longer reports the swept entity's own
  collider.** A body sweeping forward hit itself at `t: 0.0` — the collider
  sitting on the segment's origin is the nearest thing on it — so a caller that
  wanted the wall ahead had to remove the sweeper's collider before the query
  and put it back after, which flappy and breakout both did. The exclusion could
  not live above the query: `PhysicsWorld` keeps one winner, so filtering a
  self-hit out of the answer discards the sweep instead of falling through to
  the shape behind it. A caller that was compensating for the self-hit — by
  discarding a zero-`t` result, or by unhooking its own collider — should stop.

- **`PhysicsSystem::overlap_sphere` and `overlap_sphere_into` answer `Entity`
  rather than `(Entity, ShapeHit)`.** The hit was fabricated: every result
  carried `t: 0.0`, `point: centre`, `normal: DVec3::Y` and
  `started_inside: true`, the same answer whatever the geometry, so a caller
  that read the normal got "up" for every body it found. None did — all six call
  sites in this workspace named it `_hit` — and the fields are not a gap to fill
  in later, because an overlap has no impact time, no impact point and no single
  normal. `cast_ray` and `sweep_sphere` are the queries that genuinely have a
  hit, and they still return one. A caller that was destructuring the pair drops
  the second half.

- **`RenderGraph::import_image` and `import_buffer` now deduplicate on the
  handle, and a repeat import that contradicts the first is a compile error.**
  Each call used to push a node, so one `ImageHandle` imported twice in a frame
  became two `ImageId`s with **independent state trackers** — a write through
  one and a read through the other were two accesses to one image with no
  barrier between them, because the reader's tracker was already sitting at its
  own declared `initial` and the graph computed no transition.
  `validate_imports` could not see it: it compares each declaration against the
  pool's ledger of what the _previous_ executed graph left, so two declarations
  that agree with last frame agree with each other. The repeat now returns the
  id already issued and the node keeps the first label; a declaration differing
  in any of `view`, `format`, `extent`, `initial`, `claim` or `final_state`
  comes back from `compile` as `GraphError::ImportDeclarationConflict` (or
  `BufferImportDeclarationConflict`), naming the field through the new public
  `ImportField`. An identical re-import is legal and is the point — two
  subsystems importing the same target is the ordinary case. Two new
  `GraphError` variants, so an exhaustive `match` on it must add arms.

### Added

- **`flappy`'s run summary reports the simulation's own tick count**, as
  `60 frames, 59 ticks (59 simulated)`, from the newly public `Game::ticks_run`.
  The existing `ticks` counts the times the loop called `Game::tick` and rises
  whether or not the call did anything; a caller that wants to know the
  simulation ran wants the new one. `Summary` gains a `sim_ticks` field, so an
  exhaustive construction of it must add one.

- **`crcbl-audio`'s generators are held to their waveform.** `synth`'s tests now
  check `sine` and `looped_sine` against `x[n+1] = 2cos(w)x[n] - x[n-1]`, which
  every sinusoid and no other shape satisfies, and hold `noise_burst` to probe
  samples and total energy within a tolerance. No behaviour changed. One
  documented claim did: `noise_burst`'s docs said its output is the same on
  every build, and it is not — the decay goes through libm's `exp`, which glibc,
  Apple's and the MSVC runtime round differently, so a byte-exact golden of it
  is not possible anywhere but the platform that recorded it.

- **A `windowed-e2e` feature on `crcbl`**, and `tests/run-windowed-e2e.sh`
  behind it: the first suite in this workspace to create a surface that is not
  `SurfaceTarget::Offscreen`. On `crcbl-vk` a null `VkSurfaceKHR` is the
  discriminator the backend branches on, so the offscreen arms of
  `acquire_next_frame` and `present` return before `vkAcquireNextImageKHR` and
  `vkQueuePresentKHR` are reached — the acquire semaphores, the per-slot acquire
  fence, the `oldSwapchain` handoff and the extent clamp against a real
  `VkSurfaceCapabilitiesKHR` were checked by nothing. Consumers gain a feature
  they can turn on; the suite itself is `#[ignore]`d and needs an X server.

- **`PhysicsWorld::sweep_sphere_excluding(&Segment, radius, Option<ColliderId>)`**
  is the sweep with one collider held out of the candidate set, and
  `sweep_sphere` is now its `None` case. The exclusion is by `ColliderId`, so it
  is generation-checked: an id whose slot has since been reused excludes
  nothing, and the sweep answers the collider now occupying that slot rather
  than silently hiding it.

- **`ForwardRenderer` declares the base-colour page it samples**, through the
  new `ForwardRenderer::base_color_page_import()` and the public
  `BASE_COLOR_PAGE_LABEL`. The page was bound through the material descriptor
  and never imported, so a graph that also _wrote_ it had no declared ordering
  between the write and the draws that read it — a render-to-texture view
  copying into a page layer got a correct barrier only if the caller imported
  the page itself and worked the states out by hand. The read is declared on all
  three passes whose bind groups name it (`shadow`, `depth-prepass`, `forward`),
  because a bound descriptor in the wrong layout is a validation error whether
  or not a fragment stage samples it. A caller that never writes the page pays
  one import node and three declarations that produce no barriers at all, since
  `ShaderRead → ShaderRead` needs none.

- **`ForwardRenderer::base_color_page()` hands out the base-colour page** as the
  `UploadedTexture` the renderer uploaded — the image and the view every
  material row samples through `GpuMaterial::base_color_texture`. It is what a
  render-to-texture view needs in order to land somewhere visible: nothing above
  `crcbl-render` could name that image, so a caller had no way to import the
  page into its own graph and declare a copy into one layer's subresource. The
  page is created with `ImageUsage::TRANSFER_DST` already, because the upload
  that filled it is a copy, so a per-frame copy into it needs no new usage flag.

- **`apps/lantern` has an in-scene monitor**, drawn from a second camera into a
  page layer the screen in the room samples. `room::View` is the two views,
  `room::MONITOR_STACK` is what the monitor's view asks for — every effect
  except the reflections — and `room::monitor_camera()` stands on the middle of
  the screen's own face looking out along its normal, so the monitor cannot
  appear in its own picture. The screen's picture is one frame behind: the copy
  is added at the tail of the graph, after the passes that sample the page.

- **`apps/lantern` answers `--screenshot <PATH>`**, writing the run's last
  presented frame to a PNG and forcing `--headless`, as the other samples do.
  lantern needs it for a reason of its own: the monitor is fed at the tail of
  the frame's graph, so the only picture with a live screen in it is one this
  binary presented — the golden suite's live-monitor arm runs the binary and
  reads the file it leaves behind.

### Changed

- **`EffectRequest::camera` has a source in the tree.** The camera layer of the
  four-layer toggle resolution order was a field nothing but a test wrote;
  lantern's monitor is now its consumer, through a `ForwardRenderer` per view
  each holding the request its own view asked for. This is not the render-stack
  RON topic 18 describes — nothing in this workspace reads or writes RON — and
  `crcbl_render::effects`' module table says so rather than claiming the row is
  closed. `EffectRequest::video` remains unwired.

- **`crcbl_lantern::room::place` takes a `room::View`** and places only the
  objects that stand in that view, so the monitor's bezel and screen are absent
  from the monitor's own renderer rather than merely outside its frustum.

### Breaking

- **`apps/lumen` is renamed to `apps/lantern`.** Lumen is Unreal Engine 5's
  global-illumination system, and this sample is a _lighting_ acceptance
  fixture, so the collision sat squarely where it would mislead. The crate, the
  binary, the `Lantern` type, `docs/plan/sample/13-lantern.md`,
  `apps/lantern/tests/run-lantern-golden.sh` and the published demo all move
  with it: **`/demos/lumen/` becomes `/demos/lantern/` and the old path is not
  kept.** No release has been tagged and nothing durable pointed at the old URL.
  The room, the goldens and every measurement are unchanged — the sole
  behavioural difference is the name.

- **`ForwardRenderer::add_passes` now takes a `&TransientPool` as its second
  argument**, and the shadow atlas's and shadow placeholder's
  `ImportedImage::initial` are read from the pool's ledger
  (`TransientPool::imported_image_use`, `None` meaning
  `ResourceState::Undefined`) instead of from a renderer field. The field
  advanced at graph-build time, so a frame whose `compile` failed left it
  claiming `ShaderRead` for an image no barrier had touched — and with the
  ledger untouched, `InitialClaim::Tracked` had nothing to contradict and passed
  the wrong declaration through. The placeholder fared worse: it was declared in
  the state it was wanted in, so the graph emitted no barrier for it at all and
  bound a descriptor to an image with no layout. Every caller already holds the
  pool it passes to `graph.compile`.

- **`ImportedImage` gained a required `claim: InitialClaim` field, and a
  contradicting `initial` is now a compile error rather than a silent hazard.**
  `initial` was a declaration the graph could not check, and a declaration that
  lies produces a barrier with no source scope — `Undefined` maps to
  `(stage NONE, access NONE, layout UNDEFINED)`, so nothing waits for the
  previous frame's reads. `TransientPool` now records what each executed graph
  left every tracked import in, and `RenderGraph::compile` answers
  `GraphError::ImportStateMismatch` when the next frame's `initial` disagrees.
  `InitialClaim::Acquired` is the exemption a swapchain image needs: its acquire
  semaphore already orders the frame, so there is no ordering left for `initial`
  to carry. Every struct literal must add the field, and choosing `Acquired`
  where an image is genuinely owned opts that import out of the check.

- **`crcbl-wgpu` is deleted.** The crate, its `wgpu-e2e` suite, both of its CI
  jobs, the native vk↔wgpu image compare, the `GpuBackend::Wgpu` and
  `BackendKind::Wgpu` variants and `CRCBL_GPU=wgpu` are all gone. It was the
  bridge that got this engine into a browser before `crcbl-webgpu` existed, and
  every sample now builds on vk and WebGPU without it.

  `CRCBL_GPU=wgpu` is **rejected** rather than aliased onto another backend —
  the same rule that stopped `webgpu` silently opening wgpu, applied in the
  other direction, so a stale environment variable says so instead of quietly
  running something else.

  **51 of 254 resolved packages leave with it**, including the whole `wgpu`
  family, `glow`, `khronos-egl`, `gl_generator`, `gpu-allocator` and
  `parking_lot`, and five `cargo deny` duplicate-skips became unnecessary.
  `naga` stays: it is a `crcbl-shaders` dev-dependency that validates the WGSL
  `crcbl-webgpu` ships to a browser, and it simply becomes its own pin.

  `BackendKind::is_parity_target` went too. Its only `false` arm besides `Null`
  was `Wgpu` — the backend whose divergences were never going to be worked — so
  with that gone it was `is_gpu` under another name, and `parity_blockers`
  filters on `is_gpu` now.

- **`CullStats::clusters` is `Option<ClusterCull>`, not `Option<u64>`.** The
  amplification stage counted survivors and nothing else, so a panel could say
  "30 of 58 clusters survived" and could not say which test rejected the other
  28 — a number equally consistent with the normal cone doing all the work and
  with it doing none. `crcbl::render::ClusterCull` carries `survivors`,
  `frustum_rejects` and `cone_rejects` plus a `tested()` sum. Still one
  `Option`, and `None` on exactly the conditions the old field had: the two
  indirect geometry paths, and a device with `Features::MESH_SHADER` and no
  `Features::TASK_SHADER`. A caller wanting the old number reads
  `.map(|cull| cull.survivors)`.

  `crcbl_shaders::cull::STATS_WORDS` is 5, with `CLUSTER_FRUSTUM_REJECT_WORD`
  and `CLUSTER_CONE_REJECT_WORD` at 3 and 4. `mesh_cluster.slang`'s
  amplification stage still does **one** atomic per invocation: the verdict
  picks the word, and a cluster the DAG descent never selected lands in no
  bucket at all — which is what makes the three sum to the size of the cut.
  `apps/quarry`'s device suite asserts that identity on hardware.

- **`MeshPipelineDesc` gains `mesh_workgroup_size` and `task_workgroup_size`.**
  Metal takes both threadgroup sizes at the **draw** —
  `drawMeshThreadgroups:threadsPerObjectThreadgroup:threadsPerMeshThreadgroup:`
  — and Slang's Metal target drops `[numthreads(…)]` entirely, so a backend
  holding a module and an entry point has no number to pass and cannot draw at
  all. The same argument `ComputePipelineDesc::workgroup_size` already carries,
  one stage along. `crcbl-vk` verifies both against the SPIR-V, so a value that
  disagrees with the shader is caught there rather than launching the wrong
  thread count on the one backend that reads it from the descriptor.

- **`crcbl::engine::FrameInfo` gained a public field, `render_dt: Duration`** —
  the frame's wall-clock delta, straight from
  `crcbl_core::FrameClock::render_dt`. It advances on **every** frame, including
  a paused one, which is what separates it from `tick_dt` and `ticks`: a timer
  stepped on the simulation stops dead while a pause panel is up. The struct is
  not `#[non_exhaustive]`, so any out-of-tree code constructing one with a
  struct literal must add the field — nothing in this workspace does, since the
  loop is the only constructor.

- **`CommandEncoder::fill_buffer` is `clear_buffer`, and it takes no value.**
  The seam promised "a repeating 32-bit value" that three of five backends could
  never keep: Metal's `fillBuffer:range:value:` repeats a _byte_, so only a word
  with four equal bytes has an encoding, and WebGPU and wgpu offer a zero clear
  and nothing else. `Capability::BufferFillRepeatedByte` and `BufferFillWord`
  existed only to describe how badly each one could keep the promise, and both
  are gone with the parameter. `BufferFillZero` stays; every backend answers
  `Yes`.

  **Parity blockers go from eight to six** — by deleting the divergence rather
  than implementing it. `crcbl-dx12` now refuses **no command at all**: the
  non-zero fill was the last refusal it chose, so the test that held them has no
  subject and is gone too.

  Nothing called it. Every caller of `fill_buffer` in the workspace was a
  backend or a test, and the use it was written for — zeroing an indirect count
  at the top of a frame — belongs to `clear_counters.slang`, which does it from
  _inside_ the render graph where a clear may not go, and which is what let
  those counters become device-local.

  **The WebGPU command stream drops the trailing `u32`, so `STREAM_VERSION` goes
  4 → 5**, and `gpu-stream.js`, `gpu-replay.js` and the corpus move with it. The
  replayer's non-zero refusal is gone: there is no such value to refuse.

- **`DivergenceKind::Unclassified` is gone.** It meant "which of the other three
  this is cannot be settled from here", and held exactly two rows — Metal's
  `TimestampQuery` and `PipelineStatisticsQuery`. `crcbl_mtl::adapter`'s
  counter-sampling probe has now taken the measurement they were waiting on, so
  both are classified `Unwritten` and the variant held nothing;
  `every_kind_describes_at_least_one_real_row` is the test that says a kind with
  no row is vocabulary rather than classification. `docs/backlog.md` keeps the
  reasoning, because the concept may be wanted again.

  **The parity blocker count did not move** — it is still eight. An unanswered
  question and unwritten work both block parity, and reclassifying one as the
  other is honesty about what is owed, not progress against it.

- **Timestamp query results are nanoseconds, and `Limits::timestamp_period_ns`
  is gone.** The field asked for nanoseconds per GPU tick, which is Vulkan's
  model: D3D12 has the reciprocal, WebGPU returns nanoseconds already and so
  reported a `1.0` sentinel, and **Metal has no fixed tick period at all** — it
  correlates the GPU clock to the host at sample time. Two of four backends
  could not answer it truthfully.

  Each backend now converts where the factor is known. Vulkan scales through a
  fixed-point multiplier rather than `f64`, which stops distinguishing single
  nanoseconds past 2^53; D3D12 divides by its integer frequency exactly, with no
  float at all; WebGPU passes through.

  **`resolve_query_set` deliberately does not convert** — it is a GPU-side copy
  with nothing to multiply by, so its destination holds the device's own units.
  That asymmetry is now stated on both calls.

  The reply format drops the field, so `REPLY_VERSION` goes 2 → 3. The command
  stream is untouched; the two are versioned separately.

- **`MultisampleState::mask` is gone.** Vulkan, D3D12 and WebGPU all carry a
  per-pipeline sample mask; **Metal has none at all**, so `crcbl-mtl` refused
  any non-full value outright — and no `Capability` or `DIVERGENCES` row
  declared that, so the parity mechanism did not know the field existed. Nothing
  in the workspace ever set a partial mask. Every backend now passes "all
  samples" explicitly, and `alpha_to_coverage` remains for shader-driven
  coverage.

  Vulkan's is now an empty slice — a null `pSampleMask`, which the specification
  defines as all bits set. The old code passed a single word, which would have
  been **short at 64 samples**, where the array must be `ceil(samples / 32)`
  long.

  The WebGPU command stream drops the word, so `STREAM_VERSION` goes 3 → 4: an
  older decoder would read the `alpha_to_coverage` byte plus three bytes of the
  colour-target count as a mask.

- **`StencilState::reference` is gone; `set_stencil_reference` is the only
  channel.** It was honoured by two backends and dropped by two, so the same
  recorded command stream drew differently depending on the backend: `crcbl-vk`
  declares the state dynamic unconditionally and `crcbl-webgpu` drops the field,
  so an earlier `set_stencil_reference` survived a pipeline bind — while
  `crcbl-dx12` and `crcbl-mtl` re-applied the pipeline's value at every bind and
  overwrote it. `Capability::StencilReference` reported supported on all four,
  so the parity report was green and blind to it.

  The seam now **states the rule** rather than leaving it to each backend: a
  pass opens holding `crcbl_hal::stencil::INITIAL_REFERENCE`, and binding a
  pipeline does not disturb the current reference. Every backend sets that value
  as a pass opens, since Vulkan's is otherwise undefined and D3D12's outlives a
  pass.

  The WebGPU command stream drops the word, so `STREAM_VERSION` goes 2 → 3 — an
  older decoder would have read four bytes of depth bias as a reference.

- **`CommandEncoder::write_timestamp` is gone; a pass takes its timestamps in
  its descriptor.** `RenderPassDesc` and `ComputePassDesc` gained
  `timestamp_writes: Option<PassTimestampWrites>`, naming a query set and the
  two distinct indices written at the beginning and end of the pass. The old
  verb named an arbitrary point in the command stream, which only Vulkan and
  D3D12 can express: WebGPU takes timestamps solely through a pass descriptor's
  `timestampWrites`, and Metal samples only at stage boundaries. A seam verb two
  of four backends cannot honour is the defect, so the seam now describes the
  intersection — and describes it as a _shape_, since "only call this at a pass
  boundary" is a rule a caller can break and a descriptor field is not.

  With it, `crcbl-webgpu` serves timestamp query sets and **the browser backend
  reaches zero divergences** — `REVIEWED_BLOCKERS` no longer names it at all.
  The stream format carries the new field, so
  `crcbl_webgpu::tag::STREAM_VERSION` is now 2.

  Two consequences for callers of `crcbl-render`'s `PassTimers`, both real: a
  pass's reported cost is now the pass alone rather than the pass plus the
  barriers the graph inserted ahead of it, and a `PassKind::Copy` gets no row at
  all rather than one reading 0.000 ms — a copy cannot sit inside a pass, so
  there is nothing to bracket.

- **`CommandEncoder::write_timestamp` is gone; a pass names its two timestamps
  in its own descriptor.** `RenderPassDesc` and `ComputePassDesc` gained a
  `timestamp_writes: Option<PassTimestampWrites>`, holding the query set and the
  two query indices written when the pass opens and closes.

  The old verb named an arbitrary point in the command stream, which only half
  the backends can express: Vulkan's `vkCmdWriteTimestamp2` and D3D12's
  `EndQuery` take one anywhere, Metal samples only where an encoder opens and
  closes, and WebGPU takes one **only** through
  `GPURenderPassDescriptor.timestampWrites`. The pass boundary is the portable
  intersection, and putting it in the descriptor makes it a shape a caller
  cannot misuse rather than a rule to remember — there is no call left to record
  in the wrong place. The two indices must be distinct queries of the set, which
  WebGPU requires outright and every backend now refuses by name.

  **This closes `Capability::TimestampQuery` for `crcbl-webgpu`, and with it the
  last WebGPU divergence.** `create_query_set` now serves `QueryKind::Timestamp`
  on a device that opened with the browser's `timestamp-query`, the two pass
  commands carry the field over the wire, and the replayer builds
  `timestampWrites` from it. `crcbl-webgpu` diverges from nothing: every
  remaining refusal is WebGPU itself refusing.

  Callers: `crcbl-render`'s `PassTimers` names its queries in the descriptor the
  render graph builds, so a pass's reported cost is now the pass alone rather
  than the pass plus the barriers the graph inserted for it — and a
  `PassKind::Copy`, which opens no scope, gets **no row in the report** rather
  than a row reading 0.000 ms. `crcbl-mtl` and `crcbl-wgpu` refuse a pass that
  asks for timestamps by name, neither backend having a set one could be written
  into.

  The WebGPU command stream's `tag::STREAM_VERSION` is `2`: a trailing field on
  an existing command is a changed record, which an older `gpu-stream.js` would
  decode as something else.

- **`crcbl_hal::Device` gained a required `signal_semaphore` method: the CPU can
  now advance a timeline.** The seam could read a timeline (`semaphore_value`)
  and block on one (`wait_semaphores`) and had no way to move one, so a wait for
  a value no earlier submission would signal could only ever be satisfied by
  later work — which on a one-queue backend is work the queue cannot reach.
  `signal_semaphore(semaphore, value)` maps onto `vkSignalSemaphore`,
  `ID3D12Fence::Signal` and `MTLSharedEvent`'s `setSignaledValue:`.

  **`value` must be strictly greater than the value the semaphore already
  holds**, and a backend that tracks what submissions have been given to signal
  holds it above those too. Vulkan refuses a lower value itself; D3D12 and Metal
  set a fence or an event backwards with no diagnostic at all, and every waiter
  past the higher value then stops waking on a queue that looks healthy. The
  refusal is `HalError::InvalidDescriptor` — a number the caller can correct —
  and a **binary** semaphore is `HalError::Unsupported`, following
  `semaphore_value`. There is no default implementation: a backend that has not
  answered fails to compile.

  `crcbl-webgpu` and `crcbl-wgpu` refuse: WebGPU has no semaphore object at all,
  and a wgpu timeline is per-submission completion rather than something to
  signal. `crcbl_hal::Capability::CpuTimelineSignal` is the new claim, with a
  `DIVERGENCES` row for each.

- **`crcbl-mtl` performs `Capability::TimelineWaitBeforeSignal`.** `submit` used
  to refuse a wait for a timeline value nothing had encoded a signal past,
  because with one queue and no host signal nothing could ever satisfy it. The
  host signal is what was missing; the refusal is gone, and Metal's reviewed
  divergence row with it. **The parity blocker set is now twelve rows — dx12 5,
  Metal 6, WebGPU 1.** A caller that submits such a wait and never signals the
  value stops the Metal queue, exactly as the same mistake stops a Vulkan queue.

- **`crcbl_hal::BindingKind::StorageImage` gained `view_type` and `format`, and
  WebGPU can now build a storage-texture layout.** The variant used to carry
  `read_only` alone, which is everything Vulkan, Metal and D3D12 need — each
  takes the dimension and the texel format off the bound `ImageViewDesc` — and
  nothing WebGPU needs: `GPUStorageTextureBindingLayout.format` is a required
  member with no default, so `crcbl-webgpu` and `crcbl-wgpu` both answered
  `Support::No` to `Capability::StorageImageBinding`. It now mirrors
  `BindingKind::SampledImage`, whose `view_type`/`sample_type` exist for the
  same reason, and each of the other backends' conversions says where it drops
  the two new fields. **Every construction of `BindingKind::StorageImage` must
  name them.**

  `crcbl-webgpu` answers `Support::Yes`. The two fields cross the WebGPU command
  stream after `read_only` — an `ImageViewType` code then a `Format` code,
  making it the longest `BindingKind` body on the wire — and
  `web/engine/gpu-replay.js` turns them into
  `storageTexture: { access, format, viewDimension }`. A format WebGPU does not
  allow as a storage texture (every sRGB, depth and block-compressed one, and
  `bgra8unorm`, whose `bgra8unorm-storage` feature the seam has no bit to ask
  for) and the two cube view dimensions it forbids are each refused **by name at
  layout creation**, rather than becoming a browser validation error against a
  handle that already exists.

  `read_only` maps to `'read-only'`/`'write-only'` and never to WebGPU's
  `'read-write'`, which is legal on a much shorter format list; the narrowing is
  documented at `STORAGE_TEXTURE_ACCESS` and fails loudly at pipeline creation
  rather than silently.

  `crcbl-wgpu` still refuses, now honestly: wgpu's `BindingType::StorageTexture`
  takes exactly what the seam carries, and that backend — scheduled for deletion
  — never grew the arm. Its `DIVERGENCES` row stays, classified `Unwritten`.

- **`crcbl_hal::Device` gained a required `supports` method, and every backend
  must answer for every seam behaviour.** The new `crcbl_hal::Capability` enum
  names one seam behaviour per variant — the three different `fill_buffer`
  promises, the GPU-read draw count, a padded indirect stride, mesh and task
  stages, update-after-bind, push constants, bindless arrays, storage-image
  bindings, a buffer copy of a depth-format image, the three query kinds, the
  two semaphore kinds and the two kinds of timeline wait — and
  `Device::supports(capability)` answers `Support::Yes`, `Support::No(reason)`
  or `Support::NotOnThisDevice(reason)` through an exhaustive `match` with no
  wildcard arm. The third arm says the refusal was the _device's_ rather than
  the backend's, and `Support::granted` — which every device-gated arm in every
  backend already goes through — is the only thing that produces it.

  `Capability::DepthImageCopy` is now answered `Support::Yes` by every backend,
  `crcbl-dx12` included — see below for what that took. The one pair still
  refused there is `D24UnormS8Uint`'s depth plane, and by name: no fully typed
  single-plane DXGI format has 24-bit unorm elements, which is the same absence
  WebGPU has and which `wgpu-hal` encodes as `None` for the same pair.

  On WebGPU — and on `wgpu`, which enforces the same table — the capability is
  narrowed by the API rather than by the backend, and the refusal is per format
  and per direction: the depth plane of `D32Float`, `D32FloatS8Uint` and
  `D16Unorm` copies out to a buffer, only `D16Unorm`'s copies back in, and
  `D24UnormS8Uint`'s copies neither way because `depth24plus` has no defined
  memory layout. A shadow atlas readback is inside all of those.

  This is deliberately **not** a `Features` bit. `Features` stays what it is:
  optional capabilities a _caller requests_ at device creation, negotiated per
  device. A bitflag cannot make a _backend_ answer — a backend that never sets a
  new bit compiles and silently reports the capability absent — so adding a
  behaviour to one backend used to cost nothing anywhere else. Adding a
  `Capability` variant is now a compile error in every backend until each one
  says what it does about it.

  Anyone implementing `crcbl_hal::Device` outside this workspace must add a
  `supports` implementation. There is deliberately no default: a default would
  restore exactly the silence the enum removes.

- **A `wasm32` build links `crcbl-webgpu` and nothing else, and the browser no
  longer needs `wasm-bindgen`.** `crcbl-wgpu` is now a native-only dependency of
  the umbrella, so `crcbl`'s `webgpu` feature and `web/build.sh`'s
  `CRCBL_WEB_BACKEND` are **removed** — with one browser backend there is
  nothing left to select. `crcbl-wgpu` is unchanged natively: it is still
  `CRCBL_GPU=wgpu` on every desktop platform and still has its own suites.

  Because the browser backend speaks to JavaScript through a command stream
  rather than through generated bindings, the wasm now imports **nothing**, and
  `web/build.sh` copies a small loader beside the artifact instead of running
  `wasm-bindgen`. Deploying the site needs `cargo`, `python3` and `node`, with
  nothing to `cargo install` first. The demo artifacts roughly **halved** —
  breakout 3,182,323 → 1,609,109 bytes, lantern 3,565,467 → 1,995,548 — and
  their generated glue went from 72 KB to 3 KB.

  **What this costs:** a browser without WebGPU has no fallback any more. The
  WebGL2 path came from `wgpu`, which is no longer linkable there.

- **`GpuMaterial` gained two fields and its GPU row grew from 32 to 48 bytes.**
  `tiling` (`TILING_AUTHORED`, the `0` default, or `TILING_PHYSICAL`) and
  `tile_metres` select and size physical tiling. Code that spreads
  `..GpuMaterial::UNTINTED` — which is nearly all of it — is unaffected and
  renders identically, because `TILING_AUTHORED` is zero and samples the vertex
  UV exactly as before. A construction site that spells out **every** field
  without the spread no longer compiles until it names the two new ones or
  adopts the spread. `MATERIAL_STRIDE` moved with it, so anything writing the
  material table by hand must use the constant rather than a literal 32.

- **The base-colour sampler wraps instead of clamping.** Physical tiling needs a
  repeating address mode to tile past `0..1`; authored UVs stay inside `0..1`,
  so a wrapped and a clamped read return the same texel for them and no existing
  frame changes.

- **`"webgpu"` no longer means `wgpu`.** `GpuBackend::from_name("webgpu")`
  returned `GpuBackend::Wgpu` — an alias — and now returns the new
  `GpuBackend::WebGpu`. Anything using `CRCBL_GPU=webgpu` or `--backend webgpu`
  to reach wgpu must spell it `wgpu`.

  The alias had to go for the two backends to be told apart during the
  transition, and it was worse than a naming wart: left in place, every
  `CRCBL_GPU=webgpu` run would have opened wgpu and reported success, so the
  first "the new backend works" would have been evidence about the old one.

  `crcbl_hal::BackendKind` and `crcbl::backend::GpuBackend` each gained a
  variant, so a downstream exhaustive `match` on either needs a new arm.

- **`crcbl::log` is the engine's own logging module now, not the `log` crate
  re-exported.** `crcbl::log::info!(…)` and its four siblings are `crcbl_core`'s
  macros, reachable both at the crate root (`crcbl_core::info!`) and beside the
  sink (`crcbl_core::log::info!`). Call sites did not change — the path is the
  same either way — but a crate that reached through `crcbl::log` for something
  only the `log` crate has, such as `log::Log` or `log::set_logger`, now needs
  to depend on `log` itself. `Level` and `LevelFilter` are still there,
  re-exported.

  **The `log` crate is still underneath and is not going away**: `wgpu`, `naga`
  and `gpu-allocator` report through that facade, the sink still implements
  `log::Log`, and the macros dispatch through `log::logger()` so whichever sink
  a target installed receives them. That last part is why `wasm32` still logs at
  all — the browser installs `crcbl::web`'s queue, not the stderr sink.

  Seven crates dropped their direct `log` dependency as a result: `crcbl-dx12`,
  `crcbl-mtl`, `crcbl-render`, `crcbl-shell`, `crcbl-store`, `crcbl-vk` and
  `crcbl-wgpu`.

- **`SceneDesc` and `Capacities` each gained a field**, so a struct literal
  spelling every one of them needs the new one: `probes: ProbeGrid::default()`
  and `probes: 0` are the values that change nothing. `..Default::default()`
  callers are unaffected. See the irradiance-probe entry under _Added_ for what
  the fields are for.

- **`mesh.slang`'s `fragmentMain` writes two colour targets, so a pipeline built
  from `crcbl_shaders::MESH` needs two `ColorTargetState`s.** Target 0 is the
  `Rgba16Float` scene colour it always wrote; target 1 is `Rgba8Unorm` carrying
  `rgb = F0` and `a = sharpness`, the screen-march ramp that reaches exact zero
  at `ROUGHNESS_CUTOFF`. A pipeline left with one target has a fragment stage
  writing location 1 into an attachment that is not there, which WebGPU refuses
  outright and Vulkan reports as a warning at best.

  It is still **one** fragment entry point and one shader module: both
  `GeometryPath` pipelines gained one array element, and no golden moved on any
  of the four rasterisers. `crcbl_render::ForwardRenderer` needs nothing from a
  caller — it attaches its own transient, cleared to zero — and **nothing reads
  the attachment yet**, so the picture is unchanged. See
  `docs/plan/18-render-features.md`'s screen-space reflections section for what
  will.

- **The five demo setters are gone: `ForwardRenderer::set_pyramid`,
  `set_tinted_pyramid`, `set_textured_pyramid`, `set_open_box` and
  `set_dunes`.** Every one of them named a `scene::demo()` mesh and material row
  by _position_, which is why `with_scene` refused any description shorter than
  the demo's four meshes and three rows. Place the object instead —
  `add_instance(&InstanceDesc { mesh: scene::DEMO_PYRAMID, material: scene::DEMO_TINTED, transform })`
  — and hold the handle if you mean to move or remove it later: the setters kept
  one internally, and a caller that inserts twice gets two live objects rather
  than one that moved.

  **Instance insertion order is entirely the caller's now**, and it is the LOD
  hysteresis key: place objects in the order the setters did — the cube first
  wherever there is one — or a `Geometry::Dag` object inherits another's
  expanded-group state for a frame.

  With them go the description floor (a **one-mesh, one-row** `SceneDesc` is a
  scene) and `ForwardRenderer::place`'s swallowed `InstancePoolError::PoolFull`,
  which logged and dropped an object because none of those five signatures could
  report it.

- **`ForwardRenderer::dunes_clusters()` and `dunes_level_buckets()` are
  `cluster_range(mesh)` and `level_buckets(mesh)`.** Both used to publish the
  demo scene's fourth mesh by index, and `build` computed them by indexing the
  description at `DEMO_DUNES` — so a description with fewer meshes did not
  merely get a wrong answer, it panicked. They take a `SceneDesc::meshes` index
  now and answer for any resident mesh. `cluster_range` is still `None` off the
  mesh path and `level_buckets` still empty on it.

- **`set_dunes`' `bool` is `ForwardRenderer::selects_levels()`.** It answers
  whether this renderer can choose _which level_ of a `Geometry::Dag` mesh an
  object is drawn at — `false` only on a device with a mesh stage and no
  amplification stage, which would emit every level of the DAG at once. A caller
  placing a DAG asks it first; `add_instance` cannot refuse for that reason,
  because its only error is a full pool.

- **`crcbl_shaders::mesh::GpuMaterial` gained `metallic: f32` and
  `roughness: f32`, and `mesh.slang` shades with one GGX lobe driven by them.**
  Anything building a `GpuMaterial` literally has to name the two fields
  (`..GpuMaterial::UNTINTED` supplies them). `MATERIAL_STRIDE` is **unchanged at
  32** — both went into padding the row already had — so nothing that writes the
  table at a stride changes.

  `GpuMaterial::UNTINTED` is no longer "every factor `1.0`": it is
  `metallic 0.0, roughness 0.5`, an ordinary painted surface, because a lobe is
  evaluated rather than multiplied by and there is no neutral pair. Half is
  roughly where a Blinn exponent of 32 sat, so the shading a scene already had
  is the shading it keeps.

  `mesh.slang`'s `SPECULAR_POWER` and `SPECULAR_STRENGTH` are **deleted**. The
  lobe is Cook-Torrance — Trowbridge-Reitz `D`, Smith height-correlated
  visibility, Schlick Fresnel, Lambert diffuse — with
  `F0 = lerp(0.04, base_color, metallic)` and a diffuse albedo of
  `base_color * (1 - metallic)`. **A fully metallic surface therefore has no
  ambient term and is black until it has something to reflect**; screen-space
  reflections and irradiance probes are the two rows that give it one, and
  `docs/plan/18-render-features.md` is where that is argued. Every 3D golden
  moved; `sprite` and `ui` are byte-identical.

- **`crcbl_scene::gltf_import` fills both new factors, so an imported default
  material is no longer `GpuMaterial::UNTINTED`.** `metallicFactor` and
  `roughnessFactor` come off the same `pbrMetallicRoughness` accessor the base
  colour already did. glTF defaults a material to `metallic 1.0, roughness 1.0`
  — a fully rough conductor — where the engine's neutral row is a dielectric at
  half roughness, and the importer reports the document rather than the engine's
  preference. Callers that relied on the old equality have to name the
  specification's defaults instead.

- **`FrameUniforms` lost `light_direction`, `light_color` and `lod_params`.**
  The sun is a row in the light list now rather than a field, and `lod_params`
  was already dead — documented in-tree as "read by no shader since hysteresis
  landed, and written all the same". It gained `cluster_grid` and then
  `light_view_proj`, and is 656 bytes. Anything constructing `FrameUniforms` or
  reading those fields has to change.

- **`GpuLight::pad0` became `shadow_tile`, and `Light::row` takes it.** It names
  the first atlas tile the light occludes through — one tile for a spot, the
  first of six for a point. A row's default is no longer all-zero:
  `NO_SHADOW_TILE` is `u32::MAX`, because zero is a real tile and a row that
  forgot to say would occlude through whichever light holds it.

- **`BindingKind::SampledImage` gained `sample_type` and `BindingKind::Sampler`
  became a struct variant with `comparison`.** Every construction has to name
  them — `SampleType::Float` and `comparison: false` reproduce the old behaviour
  — and every `match` on `Sampler` becomes `Sampler { .. }`.

  A shadow map needs both and neither was expressible: WebGPU takes the sample
  type and the sampler's comparison mode in the **layout**, so a `D32Float` view
  bound as `Float { filterable: true }` is refused at pipeline creation whatever
  the sampler does. This is the gap `docs/backlog.md` predicted when `view_type`
  closed the dimension half. `crcbl-wgpu` consumes both; Vulkan, Metal and D3D12
  read them off the sampler and the view and each says so where it drops them.
  **The wgpu suite is the only local gate on this** — the other three would not
  have noticed a mistake.

- **`crcbl_hal::CommandEncoder` gained `draw_mesh_tasks_indirect`**, so anything
  implementing that trait outside this workspace has a new method to write. It
  takes the existing `DrawIndirect`, and the argument buffer holds **three
  consecutive `u32`s** — group counts x, y, z — 4-aligned, tight stride 12,
  `draw_count > 1` gated on `MULTI_DRAW_INDIRECT`. `DrawIndirect`'s own docs now
  say which structure each of the three indirect calls reads.

  Vulkan maps it to `cmd_draw_mesh_tasks_indirect`; the null backend records it;
  `crcbl-wgpu` returns `Unsupported` naming that WebGPU has no mesh stage; Metal
  and D3D12 refuse it exactly as they already refuse `draw_mesh_tasks`. **No new
  `Features` flag** — `VK_EXT_mesh_shader` defines both entry points together,
  D3D12 mesh tier 1 admits the `DISPATCH_MESH` signature, and a Metal device
  with mesh functions has the indirect draw, so no API offers one without the
  other.

- **The mixer remembers a listener, so a cue no longer carries one.**
  `crcbl_audio::spatial::Listener` is new — `#[non_exhaustive]`, built through
  `Listener::new(position)` — and `Mixer` gained `set_listener`, `listener` and
  `cue(emitter, grammar)`. `compute_cue` is unchanged and still takes an
  explicit listener: it is a pure function, and `Mixer::cue` is what supplies
  the remembered one.

  It exists because the engine had no listener at all, which left every game
  inventing where the ear was: the four samples spelled the same call three
  different ways — `play_panned(id, emitter_x)`,
  `play_at(id, listener_x, x, y)`, `play_at(id, x, y)` and
  `play_at(id, listener, at)`. All four are now `play_at(id, world_position)`,
  and each sample's listener convention is one `set_listener` line at the right
  point in its frame instead of a parameter on every cue: breakout and asteroids
  place theirs once in `Audio::new` because their camera never moves, flappy
  pushes one axis per tick, and horde reads the player's position under the same
  lock as the cue queue, so a cue raised on a tick is heard from where the
  player was on that tick.

  `Listener` is a type rather than three floats for a specific reason:
  `compute_cue` derives azimuth as the angle from +Z, which assumes the
  listener's orientation is fixed. A listener that can turn needs a forward
  vector, and that is the field this type exists to gain without breaking
  callers.

  With no listener set, cues are heard from `Listener::ORIGIN` — a real place,
  readable back through `Mixer::listener()`, not a sentinel. Refusing to cue
  until one arrives would have fired on the two samples that are _right_ to set
  theirs once and never touch it again.

- **`crcbl_render::Sprite` is `#[non_exhaustive]` and is built through
  `Sprite::new(sheet, rect, uv)`.** A struct literal from outside `crcbl-render`
  no longer compiles, and neither does `..base` functional update; `rotation`
  and `tint` are `with_rotation` and `with_tint`, both `const` and both
  returning the sprite. The fields stay `pub` and are still readable — `Sprite`
  has no invariant to protect, so this is about construction only.

  It exists because every new field was a breaking change to every caller:
  adding `rotation` broke nine literals that had nothing to do with turning, and
  the sample count is going up. The next field is now a non-event for anything
  outside the crate. The measurement behind the split, over all 34 construction
  sites: `sheet`, `rect` and `uv` are set by every one of them, while `rotation`
  is non-zero at five and `tint` non-white at five.

  `new` takes two adjacent `[f32; 4]`s, which is the argument-swap hazard
  `SheetDesc`'s own documentation names and the compiler cannot see. What
  catches it is the instance-layout test asserting `rect` at byte 0 and `uv` at
  byte 16 from distinct values, and the sprite golden frames — a swap inside
  `new` reds seven unit tests and
  `the_sprite_scene_draws_through_the_sprite_renderer_and_matches_its_golden`.
  Call sites remain on their own, and `new`'s docs say so.

- **`crcbl_hal::BindingKind::SampledImage` is now a struct variant carrying the
  view dimension**: `SampledImage { view_type: ImageViewType }`. Every
  construction has to name it (`ImageViewType::D2` reproduces the old
  behaviour), and every `match` arm has to become `SampledImage { .. }`.

  It exists because WebGPU takes the dimension in the **layout** rather than off
  the bound view: `wgpu::BindingType::Texture` has a `view_dimension`, and a
  layout that says `D2` while the view is `D2Array` is refused at pipeline
  creation with "expects dimension = D2, but given a view with dimension =
  D2Array". `crcbl-wgpu` used to hardcode `D2`, which was invisible while every
  sampled binding in the engine was a `Texture2D` and became a build failure the
  moment `mesh.slang` declared a `Texture2DArray`. Vulkan, Metal and D3D12 all
  read the dimension from the view and ignore the field; each backend's
  conversion says so where it drops it.

- **A material now carries a base-colour texture, and the vertex and material
  layouts both grew.** `crcbl_shaders::mesh::GpuMaterial` gained
  `base_color_texture: u32`, so `MATERIAL_STRIDE` is 32 rather than 16 and
  anything building a `GpuMaterial` literally has to name the field
  (`..GpuMaterial::UNTINTED` supplies it). `MeshVertex` gained `uv: [f32; 4]`,
  so `VERTEX_STRIDE` is 64 rather than 48 — every consumer in this workspace
  uses the constant, but a producer of vertex bytes that did not would now write
  short rows.

  `mesh.slang` gained binding 7 (`Texture2DArray`) and binding 8
  (`SamplerState`), both visible to the vertex and fragment stages for the
  reason binding 6 already was. **Any caller that builds its own bind-group
  layout for that module must add both**, because a pipeline layout that does
  not cover a binding the module declares is refused outright.

- **`crcbl_golden::Tolerance` gained `gross_channel_delta` and
  `max_gross_ratio`, and `Comparison` gained `gross_pixels` and `gross_ratio`.**
  Anything constructing a `Tolerance` literally has to name the two new fields;
  `Tolerance::EXACT` and `Tolerance::RASTERISER` are unchanged as names and
  every consumer in this workspace uses those. `Failure` gained a
  `TooManyGrossPixels` variant, so a `match` over it that was exhaustive is not
  any more, and `Comparison::summary()`'s line gained an
  `N grossly wrong (X.XXXX%)` field between "over tolerance" and "mean abs
  error" — a script parsing that line by position has to move.

  The comparator now scores **two** questions instead of trading one ratio
  against both. `max_failing_ratio` bounds how much of the frame may drift past
  `max_channel_delta`, and `max_gross_ratio` bounds how much may be past
  `gross_channel_delta`, out where drift does not reach. A driver that disagrees
  about many pixels slightly and a bug that gets a few pixels badly wrong are no
  longer measured against each other.

  This is what `Tolerance::RASTERISER` is now made of: `max_channel_delta: 2`
  and `max_failing_ratio: 0.01` for drift, `gross_channel_delta: 24` and
  `max_gross_ratio: 0.001` for defects, `min_ssim: 0.99` for structure. Every
  one is measured. A plainly visible sprite recolour — 361 pixels of a 256×192
  frame at delta 40, 0.7345% — used to pass a comparator whose only count-based
  bound was 2% of the frame; a single ratio tightened to refuse it had to sit
  between that recolour and WARP's legitimate sprite edges (76 pixels at delta
  13, 0.1546%), leaving 3.2× of room on one side and 1.47× on the other. Split
  in two, the same three frames have **6.5×** (WARP, on the drift budget),
  **7.3×** (the recolour, on the gross budget) and **24×** (metal's cube, 2
  pixels at delta 207, on the gross budget). The one band that loosens is
  0.5%–1% of a frame off by 3 to 24 levels, which nothing measured on any
  backend has ever occupied.

- **`mesh.slang` gained a seventh binding: the material table.** Binding 6 is a
  read-only storage buffer of `crcbl_shaders::mesh::GpuMaterial`, indexed by
  `GpuInstance::material` in the **fragment** stage, which the vertex stage
  reaches by handing it a `nointerpolation uint material : TEXCOORD0` varying.
  Anything building its own bind group or bind-group layout for that shader has
  to name it **and make it visible to the fragment stage** — a pipeline layout
  that does not cover a binding the shader declares, or covers it for the wrong
  stage, is refused at pipeline creation — and anything asserting the shader's
  declared registers gains one `Srv`. `GpuInstance::material` therefore stops
  being a reserved field: an instance now has to carry the id of a material that
  exists, because an unwritten table row is a base colour of zero and shades
  black.

- **`ShaderModuleDesc::dxil` is a list of `(entry point, container)` pairs**,
  `&[(&str, &[u8])]`, where it was `Option<&[u8]>`. A DXIL container holds one
  entry point, so a module drawing with a vertex and a fragment stage now offers
  a container for each and stays **one** module on every backend — where the
  alternative was one descriptor per stage, which would have made the three
  backends that carry every entry point in one artifact compile it twice.
  Absence is the empty slice, so `dxil: None` becomes `dxil: &[]` and
  `dxil: Some(bytes)` becomes `dxil: &[(entry_point, bytes)]`. `crcbl-dx12`
  picks the container named by the stage's `ShaderEntry::entry_point` and
  refuses by name when the module was given none for it.

- **`mesh.slang` gained a sixth binding and `DrawConstants` changed meaning.**
  Binding 5 is the per-bucket run of surviving instance indices the vertex stage
  now reads its instance out of, and `DrawConstants::base_instance` is
  `DrawConstants::base`: where this draw's run starts, not which instance it is.
  A caller building its own bind group for that shader — `crcbl-vk`'s depth
  probe is the only one — has to bind a run and pass a base. The byte layout is
  unchanged.

- **Taking an object out of the scene removes its instance** rather than
  skipping a draw. An instance in the pool is an object in the scene now that
  culling decides what draws, so hiding an object and culling it off screen take
  the same path out of the frame.

### Changed

- **A browser device error now names the commands it came from.** The replayer
  wraps each flush that carries commands in one `pushErrorScope` per
  `GPUErrorFilter`, so a validation, out-of-memory or internal error the browser
  raises reaches `Device::take_error` as
  `the device reported … during commands 60–61` instead of arriving as the
  device's and unattributed. Errors that fire with no flush open still arrive
  through `uncapturederror`, unattributed, as they did. The attribution is
  **not** synchronous with the failing call — `popErrorScope` answers a round
  trip later, and no granularity changes that.

- **`crcbl-mtl` implements `QueryKind::Timestamp` and
  `QueryKind::PipelineStatistics`.** Both refused with "this device advertises
  no counter-sampled set" before. `create_query_set` builds an
  `MTLCounterSampleBuffer` over `MTLCommonCounterSetTimestamp` or
  `MTLCommonCounterSetStatistic`, a timed pass carries it in the render or
  compute descriptor's `sampleBufferAttachments` at the two indices
  `PassTimestampWrites` names, `resolve_query_set` reaches it through the blit
  encoder's counter resolve, and `query_results` converts Metal's GPU ticks to
  the nanoseconds the seam owes.

  `Features::TIMESTAMP_QUERY` is reported only when the device carries the
  counter set **and** answers `supportsCounterSampling:` at the stage boundary
  the attachments sample at — the question the code depends on, not a
  `supportsFamily:` proxy. A device with neither reports neither flag and takes
  the documented degrade path.

  **Written against Apple's documentation and the installed bindings; no Metal
  device has run any of it.** The two parity rows stay on the reviewed list for
  that reason, with their text changed from "unwritten" to naming the calls that
  now exist. What holds the ABI honest meanwhile is a `const` block that checks
  this crate's own widths and sentinel against `MTLCounterResultTimestamp`,
  `MTLCounterResultStatistic` and `MTLCounterErrorValue` — it fails to compile
  on a macOS target if any of them is wrong.

- **The browser gate reads a demo's clear colour while the demo is playing.**
  Group G sampled whatever frame group F left on screen, so flappy's death
  screen — which dims the whole sky — was read as the sRGB encode having failed,
  and a Pages run went red on a docs-only commit. It now presses the demo's own
  start key until the demo's own started line appears, and checks that it got
  there, so the sample is never taken in an unknown state. A broken encode still
  fails: a live frame without one shows the linear colour.

- **The browser gate proves its own reporting channels are open.** Three of its
  checks assert a silence — no uncaught page exception, no missing asset, no
  WebGPU device error — and each passed just as happily against a listener that
  was never attached, a filter that swallowed everything, or a server that
  stopped recording its 404s. A new group H breaks all three on purpose and
  asserts the break was seen; `web/run-browser-e2e.sh` refuses a run in which it
  did not appear, so the group cannot go missing quietly. Same shape as
  `crcbl-vk`'s `validation_gate`, which exists for the same reason.

- **`crcbl-mtl` reports objects a caller never destroyed.** `DeviceInner` had no
  `Drop` at all, so Metal was the one backend where a handle nobody destroyed
  was invisible — `crcbl-vk` has warned since it was written and `crcbl-dx12`
  since 2026-08. Same message and same kinds as the other two:
  `N object(s) still alive at device teardown (7 image, 7 image view)`. ARC
  frees the objects either way, so this is a diagnostic rather than a repair,
  and it warns rather than failing anything.

- **`crcbl-vk`'s teardown leak warning names what leaked.** It said
  `N object(s) still alive at device teardown`, which tells a reader that
  something leaked and nothing about where to look — the suites that trip it
  have hundreds of creations between them. It now lists the kinds:
  `14 object(s) still alive at device teardown (7 image, 7 image view)`. The
  comment above it already called this "a leak worth naming"; it just never
  named it.

  Two leaks were found and fixed with it the same afternoon, both in
  `hal_seam_e2e`: a command buffer the render-pass-clear test never destroyed,
  and a pipeline layout `exercise_update_bind_group` created and handed to a
  callee that only borrows it.

- **The demo site needs no Python.** `web/build-pages.py` — the static page
  renderer that fills `web/templates/layout.html` from `web/pages/` — is now
  `web/tools/build-pages.mjs`, so `cargo` and `node` are the whole tool list for
  `./web/build.sh`. Node was already required by the export-contract check, the
  boot smoke test, the static server and the browser e2e, and no workflow ever
  installed Python: the site build had an unpinned dependency on whatever
  `python3` the runner image happened to ship.

  A straight port, checked as one — both renderers were run side by side and
  every one of the seven pages came out byte-identical, with identical stdout
  and identical exit codes, and eight error paths were compared message by
  message.

  Two deliberate differences. The cycle guard's message said `<!--include-->`
  could nest 8 deep while the loop actually rejected a legitimate chain of
  exactly 8; it now allows what it claims. And the unsubstituted-slot message
  lists names comma-separated rather than in Python's list `repr`, matching the
  other messages in the same file.

- **`crcbl-wgpu` now refuses a `BindingFlags::VARIABLE_COUNT` layout at
  `create_bind_group_layout`**, with `HalError::Unsupported`, and refuses
  `BindGroupDesc::variable_count` at `create_bind_group` with the same variant —
  the shape `crcbl-mtl` already used for this pair. It used to accept both: the
  layout became an ordinary fixed-size wgpu binding array and the field was
  merely checked against the entry list, which is the silent downgrade to a
  fixed array that `BindingFlags` requires a backend to refuse rather than
  perform. A wgpu binding array's length is the layout's count and the length of
  the slice a group is created with, and this backend has no
  `update_bind_group`, so nothing here chooses a length at group creation or
  fills a slot afterwards. The `Support::No` this backend already declared for
  `Capability::BindlessDescriptorArray` — and its `DIVERGENCES` row — now say
  that instead of claiming wgpu offers no partial binding, which it does, behind
  `PARTIALLY_BOUND_BINDING_ARRAY`. Callers wanting a fixed array are unaffected:
  a layout `count` above one still arrives as wgpu's array spelling.

- **`crcbl-webgpu`'s byte primitives moved to a shared `bytes` module** so both
  directions of the stream read and write through one implementation rather than
  two that can drift. `DecodeError` is still re-exported from the crate root, so
  callers are unaffected; two of its messages no longer say "command", because
  they now describe replies too. The command fixture is byte-identical across
  the move, which is what says the refactor changed no bytes.

- **`wait_for_configure` now logs `shell: first configure at WxH` itself**, so
  the line has one source instead of eight. It existed in `PolledBoot` and in
  every sample; the browser gate asserts it by exact text and reaches only the
  `PolledBoot` copy, so the seven it never runs could have drifted from the one
  it does. A caller that logged it after the call should drop that line — it is
  emitted from `crcbl::engine` on both paths now, which is the module the gate
  already expected.

- **`ImageDesc::memory` is gone.** Images are device-local, and the type says so
  by not having the field rather than by refusing the other values at run time —
  `CLAUDE.md`'s own rule, that a contract is enforced rather than documented,
  and a field every caller must fill and can still fill wrongly is the weaker
  form of it. 36 construction sites lost a line; the only one that was not
  `DeviceLocal` was the D3D12 test asserting the refusal, which went with it.

  The refusal added a commit earlier is deleted along with its test, and so is
  `crcbl-dx12`'s internal check — a guard against a state that can no longer be
  constructed is noise. `crcbl-vk`'s `create_owned_image` lost its location
  parameter entirely; Metal and D3D12 now name the one location at the call
  rather than forwarding a field, keeping the mapping shared with buffers; wgpu
  needed no edit because it never read it. `BufferDesc::memory` is untouched and
  still uses all three locations.

- **An image is always `DeviceLocal`, and the seam says so now.** `crcbl-dx12`
  refused any other setting at `create_image` and the seam's doc said only
  "almost always", so a caller could write code that worked on three backends
  and removed the device on the fourth. The null backend refuses it now, with
  the mechanism documented: D3D12's `UPLOAD`/`READBACK` heaps admit
  `D3D12_RESOURCE_DIMENSION_BUFFER` only, so a host-visible texture is not slow
  — it is uncreatable.

  **This is stronger than the buffer rule, not the same shape.** That one
  forbids a combination and leaves host-visible buffers legal elsewhere; this
  forbids the _value_, leaving `ImageDesc::memory` one legal setting. What
  decides it is that the seam has no way to touch an image's bytes from the CPU
  at all — there is no `write_image`, no mapping, no subresource layout — so the
  field buys a caller nothing observable on any backend while reliably removing
  a D3D12 device.

  Measured rather than assumed, on real hardware: Vulkan _accepts_ a
  host-visible image on radv and lavapipe, but `crcbl-vk` hardcodes optimal
  tiling, so what you get is an optimal-tiled image in host-visible memory —
  allocated, legal and useless, since `vkGetImageSubresourceLayout` is defined
  only for linear tiling. Metal honours the ask and is equally unreachable
  through this seam. **wgpu does not read the field at all** —
  `wgpu::TextureDescriptor` has no member for it — so it is the one backend that
  would silently mis-honour rather than refuse.

  The "almost always" hedge was covering nothing: of 58 `ImageDesc`
  constructions in the tree, the only non-`DeviceLocal` one is the D3D12 test
  asserting the refusal.

- **A buffer a shader writes must be `DeviceLocal`, and the seam says so now.**
  D3D12's upload and readback heaps refuse `ALLOW_UNORDERED_ACCESS` at creation
  and pin the resource to a state a shader cannot write from, so there is no
  unordered access view of one — and that rule lived only in `crcbl-dx12`, where
  a caller reading the seam could not find it. It has cost a D3D12 device twice.
  `MemoryLocation` documents it with the mechanism, `BufferUsage::STORAGE` and
  `BindingKind::StorageBuffer::read_only` point at it, and the **null backend
  refuses it** at `create_bind_group` and `update_bind_group`.

  **This is deliberately stricter than Vulkan and Metal**, which both permit it
  and where it can be a real optimisation on unified memory. The seam exists so
  that code working on one backend works on all four, and this particular
  divergence does not degrade — it removes the device. If host-visible shader
  writes are ever wanted they are a `Features` flag with a documented fallback,
  not a silent per-backend difference.

  **Read-only storage bindings of host-visible buffers are untouched**, which is
  how every uniform and read-only table in the engine works — dropping that
  exemption fails 28 tests across the sample crates, which is what says the
  carve-out is load-bearing rather than decorative. Nothing in the tree violated
  the new rule: the two devices it cost were already fixed.

- **Per-cluster culling now skips work, not just output.** The mesh dispatch was
  CPU-bounded at `(cluster_count, slot_count, 1)`, so a rejected cluster still
  had its workgroup launched and returned early. `draw_gen.slang` writes a
  per-bucket `MeshTasksArgs` — x from a new host-uploaded cluster table, y
  accumulated by the same atomic that fills `instance_count`, z one — and the
  forward pass records `draw_mesh_tasks_indirect` against it, so the extents
  come from GPU memory the cull pass wrote.

  The extents could not ride the existing draw-argument structure: `crcbl-wgpu`
  refuses a padded stride for `draw_indexed_indirect_count`, and the mesh path
  reads those arguments as a shader read in the same pass, where a resource has
  exactly one `ResourceState`. A second buffer was the way through.

  Proven by readback rather than by picture, since a golden cannot see a
  workgroup that was not launched: the box's bucket goes **1 → 0** when it
  leaves the scene while the cube's stays 1 — which a pool-sized extent cannot
  do, because `slot_count` never shrinks. Pinning the extent to the pool
  capacity instead gives `[16385, 16384, 16385]` and reds the test.

- **Bind-group-layout validation is one function on the seam, not five different
  ones.** `BindGroupLayoutDesc::check_entries(caps, backend)` and
  `BindGroupLayoutEntry::resolved_count(limits)` are new on `crcbl-hal`, beside
  the rules they enforce, and every backend calls them: `crcbl_vk::pipeline`'s
  `validate_bind_group_layout` and `layout_binding_count` are gone, as is the
  null backend's inline copy and `crcbl-wgpu`'s, and `crcbl-dx12`'s
  `check_entry` keeps only its root-descriptor rules. The rule was stated once
  and enforced four times with the wording, the coverage and the error types all
  drifting between them — a duplicated-binding refusal that named the binding in
  three backends and not in the fourth, a descriptor-indexing check two of five
  did not make, and two backends that silently ignored the count ceiling.

  Callers see the same message for the same mistake on every backend now.
  Notably the `VARIABLE_COUNT` rule reports **which** half failed — "not the
  last entry" and "not the highest-numbered binding" are separate messages, as
  D3D12 already did — where three backends emitted one sentence covering both
  and left the reader to work out which to fix.

- **Every backend now holds Vulkan's line on validation, and the tests assert
  it.** `crcbl-dx12`'s device tests assert a clean D3D12 debug-layer report at
  teardown, with warnings counting as failures and an **absent** layer failing
  rather than passing — `CRCBL_DX12_VALIDATION=0` is the opt-out for a machine
  without Windows' Graphics Tools. `debug::diagnosis` no longer clears the info
  queue, so an error quoted inside a `HalError` is the same one that fails
  teardown instead of consuming it.

  `crcbl-mtl` gained a validation report asserted at every device test's
  teardown, and it is **weaker than the other two by nature, not by omission**:
  Metal has no queryable validation channel, so it asserts that the debug layer
  interposed on the device and that no command buffer ended in
  `MTLCommandBufferStatus::Error`. An API misuse aborts the process rather than
  being reportable. `CRCBL_MTL_VALIDATION` is its requirement flag.

  Also fixed underneath it: a failed `MTLCommandBuffer` reported through nothing
  but its own `status`, so a submission nobody waited on failed in total
  silence. Failures are now tracked per submission and logged as errors.

### Fixed

- **A block-compressed buffer↔texture copy is no longer refused by the WebGPU
  backend.** The replayer derived a copy's `bytesPerRow` from a bytes-per-texel
  table, which no BC format has an entry in, so every `copy_buffer_to_image` or
  `copy_image_to_buffer` naming one failed with "has no single bytes-per-texel"
  — despite `crcbl_hal::Format` spelling the BC formats and the replayer already
  mapping each to its WebGPU name behind `texture-compression-bc`. Both pitches
  now convert through the format's block footprint, which is what WebGPU's
  buffer layout measures and what `Format::block_extent` and
  `Format::block_size` state on the seam. An uncompressed format's block is one
  texel, so its layout is unchanged. A copy of a mip level a block does not
  divide rounds up to the whole block it occupies.

- **`render-harness-e2e.mjs` left a Chromium and its profile behind on every
  error it diagnosed.** It stopped the browser from a `finally` in `main`, and
  its own `fail` calls `process.exit`, which does not unwind one — so a harness
  that never finished, a harness that could not run, and every Ctrl-C leaked the
  whole process tree. Reproduced by interrupting the driver mid-run: twelve
  chromium processes and the profile directory survived, and none do now. The
  browser registry and the exit and signal hooks the two other browser gates
  each had their own copy of are now in `browser-launch.mjs`, which registers
  every browser `launch` starts, so no gate can be written without them.

- **`sandbox --backend` offered four backends of six, and both rejection
  messages are now built from the enum.** `apps/sandbox` named `vk`, `mtl`,
  `dx12` and `null`, having never been updated when `wgpu` and `webgpu` were
  added, so the flag refused a name its own `USAGE` block listed two lines
  above. `crcbl::backend::GpuBackend::name_list` is the single source both it
  and `crcbl::args` now format, and `GpuBackend::ALL` — held complete by an
  exhaustive `match` — is what it is built from, so a seventh backend cannot
  leave either message behind.

- **The mesh bind-group layout declares bindings 13, 14, 18 and 19 on every mesh
  path**, not only where there is an amplification stage. Slang's Metal target
  ignores `[[vk::binding]]` and hands each resource the next index in its
  stage's flat table in declaration order, and `crcbl-mtl` derives that index by
  counting the layout's entries — so a layout that skipped 13 and 14 put binding
  17 at `buffer(11)` where `msl/mesh_cluster.metal` reads it at `buffer(13)`,
  and everything above it was off by two. A wrong picture with a clean log, on
  the one backend nobody here can debug on. Unreachable until now only because
  Metal refused mesh pipelines outright.

- **The server no longer replicates entities it has already destroyed.**
  `crcbl_server::Server::tick` ran `World::tick` (which sweeps), then
  `GameModule::tick`, then serialised the snapshot — so anything a game module
  despawned was still in the pool and in every system's storage when the
  snapshot was built, and the client was told about entities the server no
  longer had. On the delta path they were tombstoned a tick late; on a keyframe,
  which has no baseline to diff against, they simply appeared and then vanished
  with nothing to explain it. The tick now sweeps between the module and the
  snapshot, so a destruction is replicated on the tick it happens. A module can
  still read a despawned entity back inside its own `tick` call; it cannot after
  that call returns.

- **`apps/viewer` hot reload no longer stops while the `ESC` pause panel is
  up.** `crate::watch` was polled from `Viewer::tick`, and a paused frame runs
  no ticks, so a re-export from Blender went unnoticed until the panel was
  closed — which is the artist loop the sample exists to demonstrate. It is
  polled from `Viewer::draw` on the new `FrameInfo::render_dt` now. The poll
  interval (`watch::POLL_SECONDS`) and the unpaused behaviour are unchanged.

- **`crcbl-wgpu::create_image` refuses one too**, and a new agnostic seam test
  is what found it. `create_texture` was not wrapped in `checked()`, unlike
  `create_graphics_pipeline` beside it, so an unservable format reached the
  caller as success and surfaced through the uncaptured-error handler —
  `D32FloatS8Uint` on a device without `depth32float-stencil8`, which is the
  mirror of radv's case.

- **`a_created_image_is_one_the_device_can_serve` holds every backend to the
  contract**: a successful `create_image` must yield a usable image, or the call
  must refuse. It reads _both_ error channels, because `crcbl-mtl` refuses
  through the return value and `crcbl-wgpu` through `Device::take_error`, and it
  asserts at least one format was served so a run where everything was refused
  cannot pass as coverage of the accepting path. Measured: radv serves
  `D32FloatS8Uint` and refuses `D24UnormS8Uint`, wgpu the exact reverse, and
  lavapipe serves both.

  On CI it settled a question reading could not: **`crcbl-mtl` already
  refuses**, and helpfully — "Apple silicon reports no; use Format::D32Float,
  which the seam already prefers". **`crcbl-dx12` on WARP serves both**, so its
  refusal path did not run and dx12 is not proven clean; a branch the
  environment cannot reach is not covered however green the run.

- **`crcbl-vk::create_image` refuses a format the device cannot serve.** It did
  not ask: `vkCreateImage` is not required to fail for an unsupported
  format/usage pair, and radv returns success for `D24UnormS8Uint` as a
  depth-stencil attachment while the validation layer reports
  `VK_ERROR_FORMAT_NOT_SUPPORTED` from
  `vkGetPhysicalDeviceImageFormatProperties2` and two more VUIDs at view and
  pipeline creation. So a caller got a live-looking handle and found out much
  later — the seam suite's own raster fixture passed on undefined behaviour once
  before the layer output was read.

  It now asks that query before creating, and refuses with
  `HalError::Unsupported` rather than `InvalidDescriptor`: the descriptor is
  well formed and another device would serve it, which is the distinction the
  seam draws between the two.

  Measured both ways by forcing the fixture to try `D24UnormS8Uint` first —
  without the change `create_image` returns `Ok` and the fixture builds a pass
  on it; with it the call is refused and the negotiation falls through to
  `D32FloatS8Uint`. It refuses nothing in use: 130-odd tests across the vk,
  seam, render, forward, mesh, sprite, draw-gen, tiling and quarry suites all
  pass on radv.

- **The three browser drivers share one launch budget.** They run the same
  launch-and-poll loop in three copies, and raising the timeout in two of them
  left the third — `browser-e2e.mjs`, which had it as an unnamed `30_000`
  literal — disagreeing with the other two. `LAUNCH_TIMEOUT_MS` now lives in
  `browser-launch.mjs`, which all three already import from, beside the
  `readDevToolsPort` it bounds.

- **CI type-checks `crcbl-mtl` and `crcbl-dx12` as a consumer gets them.** Both
  compile only on their own platform jobs, and every one of those passes
  `--all-features` — so the default configuration was compiled nowhere, and a
  `use` that belonged behind a feature gate broke silently once already. The
  Linux clippy job now runs `--no-default-features` for each against
  `aarch64-apple-darwin` and `x86_64-pc-windows-msvc`.

  **It rides the Linux job rather than a macOS matrix entry**, which is what
  made this look expensive enough to be an open question:
  `cargo clippy --target` type-checks a platform backend without that platform.
  Two target installs and two crate checks. Red-checked by deleting a
  `#[cfg(feature = "mtl-e2e")]` from an import block, which reproduces the
  original break exactly.

- **CI shellchecks the browser harnesses too.** The lint step's glob was
  `tools/*.sh crates/*/tests/*.sh`, which reads like "every harness" and left
  `web/`'s five scripts — both browser gates among them — and the samples' own
  harnesses unchecked. It now covers `apps/*/tests/*.sh` and `web/*.sh`, and
  widening it cost two real fixes: both browser runners indented a multi-line
  list through `sed 's|^|  |'`, and use a read loop now.

- **The browser gates wait two minutes for Chrome to start, not thirty
  seconds.** Three of the last eight non-cancelled Pages runs on `main` died
  with "the browser never wrote DevToolsActivePort", and the failing run's own
  stderr says why: it reached dbus initialisation 22 seconds after launch and
  had not written the port file when the deadline fired at 30. The driver
  already distinguishes a browser that _exited_, so that branch is only reached
  by one still starting — the gate was giving up on a live browser. A healthy
  runner drives the whole phase, launch and eleven scenes and their readbacks,
  in 14 to 19 seconds, so the new budget is headroom for a bad minute and is
  spent only on the path that would otherwise fail.

### Added

- **`RenderGraph::compile` now checks an imported buffer's declared state, as it
  already does an imported image's.** `ImportedBuffer::initial` was a
  declaration nothing verified, and one that lies produces a barrier with the
  wrong source scope — the same hazard class that has been wrong twice on the
  image side. `TransientPool` records what each executed graph left every
  imported buffer in (`TransientPool::imported_buffer_use` reads it), and
  `GraphError::BufferImportStateMismatch` is what a disagreeing declaration
  gets. **Unlike an image, a buffer import has no `InitialClaim` and no
  exemption**: nothing hands this engine a buffer from behind an acquire
  semaphore, so a claim field would have exactly one legal value on the very
  struct whose check exists because escape hatches are how a guard gets lost.
  `ImportedBuffer` gained no field, so every construction site compiles
  unchanged; what changes is that a graph importing a buffer in a state the
  previous frame did not leave it in now fails to compile rather than emitting
  the wrong barrier.

- **`--screenshot <PATH>` writes the frame a sample presented, as a PNG.** A
  field on `crcbl::args::Common` and an arm in `Common::consume`, so it is the
  engine's flag rather than one game's: the run's _last_ presented frame is
  copied straight off the swapchain image the game drew into — menu, HUD and
  every pass included — and written where the flag says.

  **It turns `--headless` on rather than trusting the caller to pass it.**
  Reading a presented image back off a real window surface is not something
  every backend and window system will do; an offscreen ring is, so that is the
  only surface the flag ever runs against, and there is no invocation in which
  it produces nothing.

  `docs/plan/12-testing.md` asks every sample for a determinism check **and** a
  golden frame, and only the determinism half existed. Nothing the samples
  compared — simulation tuples, `horde`'s `state_hash`,
  `crcbl_server::sim_hash::hash_world` — contains a pixel, which is how every
  browser demo shipped a transfer function that was too dark for several commits
  with every gate green.

  A sample declares it can serve the flag with `Common::with_screenshot`, and
  `apps/breakout` is the first that does. **A sample that has not wired the
  arming refuses `--screenshot` with exit 2** rather than accepting it and
  writing nothing, and its `--help` does not list it — the flag's help text is
  `crcbl::args::SCREENSHOT_HELP`, spliced in by the samples that have it.

  `crcbl-golden` is therefore a normal dependency of `crcbl` on native targets,
  because something in the binary has to encode the PNG. It stays out of
  `wasm32` builds, where there is no argv to carry the flag and no file to
  write, so no browser demo links a PNG encoder.

- **`apps/breakout` has a golden frame**, `apps/breakout/tests/golden.rs` and
  `apps/breakout/tests/golden/board.png`, driven by
  `apps/breakout/tests/run-breakout-golden.sh` and run on lavapipe in CI. It
  runs the **compiled binary** with `--screenshot` and compares the file that
  binary left behind — the first golden in the tree that does, and the reason
  the capture is an engine flag instead of per-sample test code.

  Three claims stand in front of the comparison, in
  `crates/crcbl/tests/render_e2e.rs`'s shape: a distinct-colour floor, the menu
  panel against the field behind it, and the top-left brick's red against its
  blue — the last being the one a channel-order mistake fails and a luma-based
  structural comparison does not. A fourth,
  `a_uniformly_darkened_frame_is_refused_by_the_tolerance_the_golden_uses`,
  needs no GPU and pins that `Tolerance::RASTERISER` refuses the uniform
  multiply the reported defect amounted to.

- **`apps/flappy`, `apps/hud`, `apps/asteroids` and `apps/horde` have golden
  frames too**, so every sample in the tree now has the determinism check and
  the golden frame `docs/plan/12-testing.md` asks for. Each is a
  `tests/golden.rs` and a `tests/golden/*.png` driven by its own
  `tests/run-<sample>-golden.sh`, each gets a step beside breakout's in
  `.github/workflows/ci.yml`, and each runs the compiled binary with
  `--screenshot`. All four declare `Common::with_screenshot`, so `--screenshot`
  is a flag those binaries now have and list in `--help` instead of refusing
  with exit 2.

  **The claims in front of each comparison are the sample's own**, because a
  golden passes on two blank images: flappy's blue sky against its green ground
  band, hud's red health fill against its blue mana fill, asteroids' rock
  against the space beside it, and — the one that is not a block ratio — the
  fraction of horde's frame that is enemy-red, because a block on one enemy
  cannot tell four enemies from four hundred.

  **horde's golden is of a `--prefill` run.** A headless horde never leaves its
  title screen, and a golden of an empty arena would go on passing after every
  enemy sprite stopped drawing. The suite refuses the frame unless the summary
  reports the run as `Playing`.

  No sample repeats breakout's darkening test: that pins `Tolerance::RASTERISER`
  itself, which all five compare under unchanged.

- **`crcbl-mtl` builds mesh pipelines and records the mesh draws.** A real
  `MTLMeshRenderPipelineDescriptor` — object, mesh and fragment functions,
  colour attachments, depth and stencil, sample count — sharing the raster
  path's target checks and state rather than copying them, plus
  `drawMeshThreadgroups:` and its indirect form. An OS or family that cannot
  host it is refused by name (macOS 13 and `MTLGPUFamilyMetal3`) rather than
  crashing.

  **It is not reported as `Features::MESH_SHADER`, and both parity rows stay.**
  No Metal 3 device has executed a line of it, and `Support::Yes` means the
  backend performs the capability as the seam documents it. `crcbl-dx12` is the
  precedent: its mesh path is written and its adapter withholds the flag for the
  same reason. The rows move after a real device runs it, not before.

- **`ForwardRenderer::set_frozen_selection_eye`** — topic 25's third debug
  overlay, and the last one `docs/plan/sample/14-quarry.md` owed. It pins the
  eye the cluster cut is selected from and changes nothing else: the frustum,
  the normal cone and the frame are all still the live camera's, so a reviewer
  flies away from the pinned point and looks at the cut that point chose. That
  is the only vantage a cut can be judged from — from the eye that selected it,
  a cut far too coarse and a cut exactly right have the same silhouette, which
  is what a screen-space error budget promises. `None` hands the descent the
  camera's own eye, exactly as before the field existed. `quarry` gains the key,
  a pause row and a panel row naming where it is pinned.

- **`crcbl_hal::null::Recorder::bind_group_layouts_created`**, for asserting on
  the layouts a renderer declares rather than only the modules it compiles.

- **`ForwardRenderer::set_heatmap`/`heatmap()`, and `DebugView`.** Topic 25's
  second debug overlay: each cluster shaded by the projected screen-space error
  the LOD selection judged it on — the number `draw_gen.slang`'s
  `group_is_expanded` compares against the budget — on a ramp that climbs in
  Rec. 709 luminance from a cold floor to white, with a hue break at the hold
  budget and another at the expand budget. Each budget therefore draws itself as
  a contour across the surface instead of hiding in a gradient, which is the
  whole question a viewer has. A rainbow was rejected: it has no readable
  ordering, and a luminance-monotone ramp survives a greyscale screenshot.

  Mesh path only, exactly as the LOD tint is — a per-cluster error exists only
  where selection is per cluster, and both indirect paths draw the same flat
  grey. `ForwardRenderer::debug_view()` is the one place the three overlays'
  precedence is decided (heatmap over LOD tint over normals), so a caller
  setting two switches gets a defined answer rather than whichever shader branch
  ran first.

- **`quarry` gains `--heatmap`**, a `HEATMAP` pause-menu row, and a `view` row
  on the debug panel naming the overlay in force. The two overlay rows are
  mutually exclusive, and `--heatmap` wins over `--lod-view` in either order.

- **`quarry` has a browser demo**, at `/demos/quarry/`. The geometry acceptance
  fixture compiled to `wasm32`, drawing its cluster DAG through WebGPU. A
  browser exposes neither a mesh stage nor a GPU-side draw count, so the page
  resolves to `GeometryPath::IndirectPerBatch` and `BindingModel::ArrayPages`
  **by construction** — the level is chosen once per instance and the whole face
  draws at one level at a time, which is the honest picture of what a browser
  visitor gets. Measured 34/34 in the browser gate on an RX 7900 XTX and on
  Chromium's SwiftShader.

- **`quarry --camera dolly`**, a third camera that runs the goldens' own dolly
  down the face and back on the simulation clock — 90 m in 30 s, slow enough
  that a level boundary arriving is watchable, which is what "no boundary
  popping" has to be for anyone to check it. The pause menu's `CAMERA` row is
  now a three-way cycle (`FIXED` → `DOLLY` → `FREE`), and the browser page opens
  on the dolly. It turns round at the far end rather than looping: restarting at
  the near end would put 90 m of translation into one frame, which is the
  artefact this sample exists to disprove.

- **`apps/quarry` has a windowed front end**, so the geometry acceptance fixture
  is something a reviewer can look at rather than only measure. It draws the
  face's cluster DAG through `ForwardRenderer` with the pause menu and the debug
  panel, and adds `--camera fixed|free`, `--force-geometry`, `--force-binding`,
  `--lod-budget <PX>` (zero, negative and NaN are refused — each expands every
  group to the bottom of the DAG), `--lod-view` and `--report`. The debug panel
  names the geometry, binding and lighting paths, whether the run forced any of
  them, the budget the frame was actually selected under, and what the frame's
  culling kept — instances and clusters apart, naming the frame the readback
  came from, because that ring runs a few frames behind. `--headless --frames N`
  prints the same paths beside the triangle count, the budget and the cut.

  The device-free counts the binary used to print unconditionally are unchanged
  and now live behind `--report`, which opens no shell and no adapter.

- **`crcbl_quarry::camera`** owns `DOLLY_START`, `DOLLY_END` and `dolly`, moved
  out of the device suite so **the window and the committed goldens fly one
  path** — a windowed pose that is not the pose `apps/quarry/tests/golden/` was
  blessed from is a picture nobody can hold against the reference. Beside them
  `FLY_SPEED`, the free camera's speed, derived from the face's depth rather
  than typed in: the engine's room-sized default would take 75 seconds to cross
  it.

- **`crcbl::render::Flyer` is the engine's free-fly camera**, moved out of
  `apps/lantern` so a second sample can fly the same one. It is the same
  controller — WASD and Space/Shift on a fixed timestep, arrow keys and
  `PointerUpdate::motion` for the turn, a pitch clamped short of vertical — and
  it now sits beside `OrbitCamera` where it needs no device and every claim it
  makes is a unit test. `SPEED`, `TURN` and `LOOK` come with it.

  **The walk speed is per-camera.** `SPEED` is now a _default_ sized for a room,
  and `Flyer::with_speed` replaces it for a scene that is not room-sized;
  `Flyer::speed` reads it back. `Flyer::at` still starts at `SPEED`, so nothing
  that already flew one moves differently. The turn rates stay constants: an
  angle is an angle at every scale.

  `crcbl_lantern`'s public surface is unchanged — it re-exports `Flyer`, `SPEED`
  and `TURN` from the engine rather than defining them.

### Changed

- **`crcbl_shaders::mesh::FrameUniforms` gained a trailing
  `lod_params: [f32; 4]`** — the frame's pixels-per-unit and both LOD budgets,
  carried into the geometry stage so the heatmap shades by the metric the cut
  was actually chosen with rather than a second derivation of it. Appended, so
  no existing member's offset moves and every golden blessed before it still
  matches. Out-of-tree code building the struct with a literal must add the
  field.

- **`crcbl_shaders::meshlet::ClusterDrawConstants` gained `level_groups_at`**,
  where the frame's `LevelGroup` records start. The block is still 32 bytes.

- **quarry's `[HUD]` heartbeat gained `eye z:`**, the camera's position down the
  face. Anything parsing that line needs updating. It is what the browser gate
  reads to prove the page is simulating rather than merely presenting — a frame
  counter or a wall clock cannot move it.

- **`crcbl-webgpu`'s refusals are checked too, without a browser.** It is the
  one backend the native seam suite cannot open — `crcbl::backend::open` answers
  "it reaches a device only on wasm32" — so the driver that holds every other
  backend to "declared unsupported must refuse" had never run against it.
  `WebGpuDevice` records commands to a stream rather than executing them, so a
  refusal is a decision the crate makes in Rust and an ordinary unit test can
  see it. Covers the rows whose refusal is a single device call — the four
  timeline rows and `PipelineStatisticsQuery` — demands `HalError::Unsupported`
  specifically, and checks the accepting side so it cannot pass by refusing
  everything.

  A sibling test covers the encoder's three: `draw_indirect_count`,
  `draw_indexed_indirect_count` and `draw_mesh_tasks` return nothing, so they
  record the refusal and surface it at `finish`, and reaching them needs no
  pipeline at all. Nine of this backend's twelve unsupported rows are now
  checked natively — nine of thirteen, which is `DIVERGENCES`' own row count for
  this backend. The line falls where the refusal is _decided_: those nine are
  refused by the crate in Rust, while `PushConstants`,
  `BindlessDescriptorArray`, `PolygonModeLine` and
  `IndirectArgumentPaddedStride` are refused by `gpu-replay.js` in the browser,
  because the writer "carries what the caller gives" and validates nothing. No
  native test can ever cover those four, so a probe group is the only route
  rather than the convenient one.

- **The seam suite can now exercise the half of the parity contract a capable
  device hides — and it found three mis-declarations in `crcbl-vk`.** With every
  optional feature asked for, that backend declares **all 24** capabilities
  supported, so "declared supported must work" ran 24 times and "declared
  unsupported must refuse" ran **never**. Thirteen are gated on a device
  feature, so `CRCBL_SEAM_WITHHOLD=all` opens the device with none of them and
  puts those thirteen on the refusal side.

  What that found, none of it visible on any device CI opens:
  - **`create_semaphore` built a timeline on a device opened without
    `Features::TIMELINE_SEMAPHORE`**, while `supports` declared it unsupported —
    the backend performing what it said it would refuse. It now refuses, which
    is what makes the declaration true.
  - **`TimelineWaitBeforeSignal` answered `Support::Yes` unconditionally** for
    something meaningless without a timeline. Gated with the rest.
  - **`UpdateBindGroup` answered `Support::Yes` unconditionally**, while this
    backend's `update_bind_group` refuses a layout without `UPDATE_AFTER_BIND` —
    and the `descriptor_binding_*_update_after_bind` bits all come from
    `Features::DESCRIPTOR_INDEXING`. Gated on it.

  **`crcbl-dx12` and `crcbl-mtl` carried the same `TimelineWaitBeforeSignal`
  inconsistency** — `ID3D12Fence` and `MTLSharedEvent` are both core, so each
  built a timeline on a device that had just declared it unsupported. Found by
  reading, since nothing here runs either backend; each got the same two changes
  and each job got the narrow step, landed type-checked against
  `x86_64-pc-windows-msvc` and `aarch64-apple-darwin`.

  A CI step on the lavapipe job runs the narrow pass, scoped to the one test:
  the reviewed-divergence snapshot and the pipeline-layout ceiling describe the
  capable configuration and legitimately do not hold on a device opened with
  nothing.

- **`Capability::SamplerAnisotropy` is driven by the agnostic seam suite**, so
  its capability coverage goes 20 of 24 to **21 of 24**. It had been unexercised
  on the reasoning that "the observable is a filtered texel", needing a shader
  that samples a minified texture at a grazing angle — an argument against a
  capability the enum does not define. This one is "a `SamplerDesc` with
  anisotropy above `1.0`", and `exercise_sampler_anisotropy` drives that: the
  descriptor is created at the device's own `Limits::max_sampler_anisotropy`,
  and a backend declaring support while capping that limit at `1.0` — the value
  that _disables_ anisotropy — is reported as silently ignoring the call and
  fails. Same correction `StorageImageBinding` came out of.

  The filtering test stays declined, and for its own good reason: a conformant
  implementation may legally take fewer samples than asked for, so "the
  anisotropic image differs from the isotropic one" is not guaranteed even on an
  honest device.

- **The browser's pixels are held against a native backend's, directly.** New
  `web/run-cross-backend-e2e.sh`, wired into `pages.yml`'s `render-harness` job
  after the golden comparison: it renders each of the eleven golden scenes
  through `CRCBL_GPU=vk` and compares the readbacks that step already produced
  against them, so the wasm build and the browser run happen once.

  **This is what `crcbl-wgpu` was still being kept for.**
  `crates/crcbl/tests/run-cross-backend-e2e.sh` compares vk against wgpu, which
  on Linux is a second abstraction over the same Vulkan driver; this compares vk
  against a genuinely separate implementation, over eleven scenes rather than
  three. Measured, radv against Chromium-on-SwiftShader: nine of eleven match,
  `sprite` is byte-identical, and the two that do not are the same `ssr` and
  `ui` the golden gate already excuses. It is _tighter_ than the golden
  comparison, not looser — `ssr` differs in 1355 pixels here against 25,611
  there. The same 9/11 verdict comes out with lavapipe as the reference, which
  is what says the gate measures the browser rather than the reference's
  rasteriser.

  Nothing is deleted yet: the vk↔wgpu job stays until `crcbl-wgpu` goes.

- **quarry's device suite runs in CI, on lavapipe.** It ran only on a
  developer's GPU before: under a bare `cargo test` it opens the `Null` backend,
  where every assertion about a picture reports itself skipped — honest, and
  useless as a gate. `apps/quarry/tests/run-quarry-e2e.sh` is the harness, and
  it refuses two ways a green run can prove nothing: no `CRCBL_GPU` named, and a
  run in which no frame reported a per-cluster cut. `CRCBL_GPU=null` passes all
  nine tests and still exits non-zero.

  **The numbers turned out not to be a driver's**, which is what made the CI
  step worth adding: measured, the per-cluster cuts and the drawn levels are
  _identical_ on radv and on lavapipe, and only the rasterised pixel counts
  differ — by five of 49,152 at the widest.

  Also widened: CI's shellcheck glob was `crates/*/tests/*.sh`, which reads like
  "every harness" and left the samples' own harnesses unchecked. It now covers
  `apps/*/tests/*.sh` too.

- **`GpuContext::adapter` names the GPU a context opened on.** The only way an
  application can say which device it is running on — every other place that
  names one is inside a backend, writing to the log.

- **quarry attributes its reduction between the two culls.** The sample's exit
  criteria ask for it by name — "a single total hides which one is working" —
  and `ForwardRenderer::cull_stats` carries both numbers out of the frame that
  made them. Down the dolly the camera's instance cull keeps 1 of 1 every frame
  while the amplification stage keeps 27 to 47 clusters, so **all** of the
  reduction is cluster culling.

  That is a true answer and a degenerate one: quarry places one instance of one
  mesh, so "the instance cull did nothing" and "the instance cull is broken"
  draw the same frame. It is asserted rather than assumed for exactly that
  reason, and whether the sample should depict several faces is recorded in
  `docs/backlog.md` as a design question.

- **quarry asserts one thing that is not a count: that the face is shaded.**
  Coverage, the per-cluster cut, the uniform cut's walk and the triangle counts
  would every one be unchanged by a face lit from the wrong side, which is the
  gap a golden closes — but shading is a mechanism, so it can be observed by
  moving its input. Crossing the sun from one shoulder to the other changes
  58.3% of the frame, and switching it off changes 37.2%; a light of **no colour
  has no direction**, byte for byte, which is where a shadow cascade that
  survived its own light would show up.

  **What was tried and is not true: "a sun below the horizon is no sun."** It
  looked like a leak — a sun 53° under moves 12 pixels the unlit frame does not,
  and one 24° under moves 12,227 — and it is correct rendering. The face carries
  34 metres of relief over 120 of width, so it has slopes steep enough to catch
  a low sun. Recorded in the test, because the measurement reads like a bug.

- **The dolly runs on every path, and quarry records its triangle counts.** The
  sample's exit criteria ask for no LOD popping on _any_ path and a triangle
  count per path, and the two indirect paths needed their own observable: a
  uniform cut picks one level for the whole mesh, so the bucket whose
  `instance_count` came out non-zero **is** the level and its `index_count` is
  what the draw asked the device for. Level 0 draws 8192 triangles, halving per
  rung.

  Down the dolly at a 256-pixel budget the cut walks 2 → 1 → 0, a stop on each
  rung, and both indirect paths agree exactly. **At 1024 pixels it skips 2 → 0,
  and that is the camera rather than a defect** — a stop is about 17 metres and
  a uniform cut moves the whole mesh at once, so a coarse enough budget makes
  one step of the dolly worth more than one rung. Recorded because it bounds
  what the assertion means.

- **quarry draws the same face on all three geometry paths.**
  `docs/plan/sample/14-quarry.md`'s milestone 3, reached by subtracting features
  from one capable adapter rather than by needing three machines: withholding
  `MESH_SHADER` selects `IndirectCount` and withholding `DRAW_INDIRECT_COUNT` as
  well selects `IndirectPerBatch`. The fixture asserts the path that opened is
  the path asked for, because a device that never had the feature lands a rung
  lower and would make a green comparison between one path and itself.

  Read at two budgets, since they say different things. At one pixel the mesh
  path's cut is the base mesh, so all three draw the same triangles and cover an
  identical 28,650 pixels of 49,152. At sixteen the mesh path is drawing levels
  1 and 2 per cluster while the other two select per instance — **different
  geometry** — and they still land within four pixels of each other, which is
  "no LOD popping" seen from another angle.

- **quarry has its fixed dolly, and detail arrives as the camera closes.** One
  straight run down the face's own axis, nine stops, all on **one renderer** —
  which is what makes it a different measurement from the same positions
  rendered by fresh contexts, since `docs/plan/25-lod.md`'s hysteresis is
  device-local state a shader writes once a frame. Level 2's contribution falls
  18 → 0 while level 0's rises 0 → 38, and the cut's mean level goes 1.375 →
  0.513.

  **Smoothness is asserted separately, and it is the "no LOD popping"
  criterion.** A cut that reached the same end by jumping there would satisfy
  the fall and would pop on screen, so each step is held to a tenth of a level
  of rise; the measured worst is 0.013, once, at the last stop. Run backwards
  the whole thing mirrors — the mean level climbs 0.558 → 1.220 as the camera
  retreats — which is how the assertion was shown able to fail.

- **quarry selects per cluster, and it is measured rather than looked at.** The
  amplification stage's cut comes out of `ForwardRenderer::cluster_selection` —
  one `u32` per resident cluster, which nothing in the frame reads — and
  `tests/residency.rs` splits it back into levels by the pool's own layout. From
  the sample's camera on an RX 7900 XTX, swept over the pixel budget:

  | budget | clusters drawn | per level, finest first  |
  | ------ | -------------- | ------------------------ |
  | 1      | 102            | `[100, 2, 0, …]`         |
  | 4      | 71             | `[28, 43, 0, …]`         |
  | 16     | 48             | `[0, 30, 18, 0, …]`      |
  | 64     | 32             | `[0, 5, 22, 5, 0, …]`    |
  | 4096   | 18             | `[0, 0, 0, 13, 5, 0, …]` |

  At 4096 pixels the face is 18 coarse clusters instead of 102 fine ones and
  still covers 57.0% of the frame against 57.6% — the wall is there, drawn from
  a fifth of the geometry.

  **The assertion is a share, not a count of non-empty levels.** `[100, 2, …]`
  is a uniform cut with a rounding error on the end and would clear "more than
  one level drew", which is exactly the case the test exists to distinguish; it
  requires instead that no level holds four fifths of the cut. Both extremes are
  asserted beside it — the base dominating at one pixel, and nothing of the base
  surviving at 4096 — so the mixing assertion is shown to be able to fail.

- **`apps/quarry` has its tiling case, and it found that border locking is not
  needed for it.** `docs/plan/sample/14-quarry.md`'s scope names "a tiling
  modular wall piece for border locking", and `crcbl_quarry::tile` is it: tiles
  sample the same height field at **world** coordinates, so tile 0's `+X` column
  and tile 1's `-X` column are the same places and come out bit-identical, with
  no stitching pass to keep in sync. Decimated independently, they still meet.

  **The explicit locking turned out to be ceremony**, and the red-check is what
  said so: written first with every border edge passed to
  `simplify_with_locked_edges`, replacing that list with `&[]` **still passed**.
  `crcbl_scene::simplify`'s own docs have the reason — "an edge used by any
  number of faces other than two is a border… an open mesh keeps its boundary
  loop exactly" — so a tile's outer border is held whether or not a caller asks.
  That function is for boundaries **interior** to the mesh, which is the cluster
  group's edge `crcbl_scene::cluster_dag` passes it. The module calls plain
  `simplify` and says so.

- **The `quarry` binary reports the hierarchy, not just the flat mesh.** At its
  256-cell default the face coarsens into **12 levels**, 1640 clusters and
  131,072 triangles down to 30 and 1,540, and the levelled scene reserves
  180,714 vertices across 12 mesh-table entries against the flat one's 82,562.
  Per level rather than as a total, because "how many levels and how fast do
  they shrink" is the question a reader has about a DAG and one number answers
  neither. It still opens no device; what needs one is asserted in
  `tests/device/`.

- **`apps/quarry`'s face is a cluster hierarchy too.** The new
  `crcbl_quarry::dag` builds the QEM DAG over the same content through
  `crcbl_scene::build_cluster_dag` and describes it as a `Geometry::Dag` scene —
  `docs/plan/sample/14-quarry.md`'s milestone 2, first half. On an RX 7900 XTX
  the levelled face draws on the `MeshShader` path and covers 58.6% of a 256×192
  frame against the flat mesh's 57.6%, which is the same wall.

  **Each level's normals are recomputed from its own triangles**, because
  `crcbl_scene::simplify` is position-only and a coarse vertex belongs to no
  vertex below it. The dunes patch's alternative — evaluate the analytic height
  field and take its gradient — was not taken: that is the _fine_ surface's
  normal, every ripple the decimator just removed shaded back on, which would
  make a level look unlike its own silhouette.

  The pools reserve every level summed, not the base mesh's counts, since a DAG
  is resident all at once and selection only decides what is drawn.

- **`GpuContext::open_offscreen` renders with no window behind it.** Same
  context, same `acquire` / `submit_and_present` frame loop, built on
  `SurfaceTarget::Offscreen` — a target every backend implements and none of
  them dereferences, so it needs no shell, no display server and no `unsafe`
  from the caller. What it is for is rendering with nothing to render _into_: a
  golden frame in a test, a thumbnail from a headless job, an application
  asserting its own scene draws before it has a shell. It is blocking, like
  `open`, and has no non-blocking twin because there is no offscreen browser
  context to open.

- **`ForwardRenderer::with_scene` refuses a cluster array that would read
  outside itself**, through a new `MeshClusters::check` in `crcbl-shaders`. A
  cluster's two runs must lie inside the arrays they index, its vertex run must
  name only vertices the mesh actually has, its corners must name only entries
  of its own run, and it must be inside `MAX_CLUSTER_VERTICES` and
  `MAX_CLUSTER_TRIANGLES`. The new `ClustersInvalid` names the first cluster
  that is wrong and, through `ClusterFault`, which read it is and the numbers
  that make it one.

  **This was found by a red-check that came back green.** `apps/quarry`'s
  residency test claimed to catch "a cluster naming a vertex past the end";
  setting one to `u32::MAX` and re-running produced a pass. Nothing below the
  seam reports it either — the mesh stage indexes these arrays unchecked,
  because it cannot check them — so the failure mode was a wrong frame or a lost
  device with nothing anywhere saying why. The runs are widened to `u64` before
  they are added, so a run that wraps `uint` is refused rather than passing as a
  short one.

- **`apps/quarry` exists, and generates its scene.**
  `docs/plan/sample/14-quarry.md`'s S4C sample — the geometry acceptance fixture
  — begins with its content: `face::quarry_face` builds a dense heightfield
  quarry face, 131,072 triangles at the binary's 256 cells, which
  `crcbl_scene::build_meshlets` turns into 1,640 clusters.

  **It recedes rather than standing up**, and that is the specification talking:
  a flat wall is one distance from the camera, so every cluster in it wants the
  same LOD and per-cluster selection has nothing to prove over per-instance
  selection. The face spans 180 m of depth so the near and far clusters of one
  mesh sit at screen-space errors an order of magnitude apart.

  Deterministic by construction: each height is `crcbl::core::rand::hash_u64` of
  its lattice coordinate, so a vertex depends on where it is and nothing else —
  not on iteration order, which a seeded sequential generator would quietly
  depend on the first time the loop was parallelised.

  `scene::quarry_scene` turns it into the `SceneDesc` the renderer makes
  resident — packed vertices, the meshlet clusters, one rock material as row 0
  and a one-texel page. **It computes its own pool sizes**, because
  `Capacities::default` reserves 65,536 vertices and 262,144 indices while this
  face needs 66,049 and 393,216: taking the defaults would be refused at
  `with_scene`. A test asserts the defaults still would not fit, so the
  arithmetic cannot quietly become ceremony.

  Nothing renders yet. The binary opens no device and reports the counts and the
  reservation, because those halves of the sample's "counts per path" exit
  criterion need no GPU and the draw counts arrive with the renderer.

  **But the face draws, and that is asserted rather than looked at.**
  `tests/residency.rs` opens an offscreen context, makes the scene resident and
  records a frame through the real `ForwardRenderer` and the real render graph.
  On `Null` it asserts the frame recorded — the forward pass is in the compiled
  graph — and says so; on whatever `CRCBL_GPU` names it reads the frame back and
  asserts what covers it. On an RX 7900 XTX the face selects
  `GeometryPath::MeshShader` and covers **57.6%** of a 256×192 frame in 947
  distinct colours, against 0% with the instance removed. That is
  `docs/plan/sample/14-quarry.md`'s milestone 1.

- **`apps/viewer` reloads the document when it is written again.**
  `docs/plan/sample/05-viewer.md` milestone 3's artist loop: re-export from
  Blender and the frame becomes the new file, with no window reopened. The new
  `crate::watch` module polls the path four times a second and needs a
  modification time and length to stand still across two looks before it offers
  a reload — an exporter writes a `.glb` progressively, so a load in the middle
  of one is a parse error about a file that is about to be fine.

  `Gpu::reload` builds the whole new renderer, grid and instance list before it
  releases the live one, so **every failure keeps the frame already on screen**:
  a document too large for its pools, one caught mid-write, one that converts to
  nothing. The exposure and the wireframe are carried across, since they are the
  renderer's state and the renderer is what gets replaced. The camera is
  deliberately not re-framed — an artist who has just placed the view does not
  want it moved because they saved; `F` re-frames when they do.

  `--tick-hz` now paces something: the tick was empty, and the poll is what it
  does. The look itself is on a fixed interval, so the rate a save is noticed at
  does not move with the flag. The debug panel gained a `reloads` row, which is
  the only way to tell a reload that ran from one that was never offered when a
  re-export changes nothing visible.

- **`crcbl-ui` menus carry sliders, and `apps/viewer`'s panel has one for the
  exposure.** `MenuItem` gained a `kind` — `MenuItemKind::Action` for the button
  every row was until now, or `MenuItemKind::Slider`, a value in `0.0..=1.0` the
  pointer drags along a groove drawn between the row's label and its value
  column. `Menu::slider`/`set_slider` are the two ends of it, `MenuSet::get_mut`
  reaches a menu by name for a game refreshing a live value, and `MenuStyle`
  gained the groove's metrics and the three colours it is painted with.

  **A slider reports no `WidgetId`, from the commit key or from a click.** There
  is no action for a value to fire, and an id escaping one would arrive at a
  game's action table looking exactly like a button press — which is why the
  viewer's `MenuAction` stays uninhabited while its panel now carries a row
  numbered in the game's range.

  **The drag wins over the caller's write.** `set_slider` is refused while the
  handle is held, so a game mirroring the value it reads back into the widget
  every frame — which is the shape the viewer uses — cannot pin the handle under
  the cursor.

  In the viewer the handle is **even in stops**, not in multipliers: the range
  is five stops either side of one, and laid out linearly every value below one
  would live in the first one-and-a-half percent of the groove. `-` and `=`
  still step the exposure and now move the handle with it.

  The groove is drawn with the UI pass's own rectangles rather than nine-sliced
  art, so it needs no addition to the shipped sheet.

- **`apps/viewer` draws a normals view.** `N` toggles it, off by default. Each
  surface is coloured by its **world-space** normal as `n * 0.5 + 0.5` — +X red,
  +Y green, +Z blue — so an inverted face reads as the complement of the colour
  it should have, and a missing or badly interpolated normal shows as itself
  rather than as the plausible shading a light would have given it.
  `ForwardRenderer` gained `set_normals_view`/`normals_view`, and the debug
  panel's new `normals` row says `world` or `off`.

  **World space rather than view space**, so a face keeps its colour while the
  camera orbits and "is this face inverted" is something a modeller sees rather
  than infers from a picture that re-colours whenever they move. View-space
  normals answer the other question — is this normal smooth — and would need a
  view matrix the frame's uniform block does not carry.

  It builds no pipeline and adds no pass: the switch is one previously unused
  lane of the frame block (`ambient.w`), read by a branch in the fragment stage,
  so unlike the wireframe there is no device that can refuse it. Off writes the
  value that lane always held, so no golden moved.

- **Exposure is a runtime value, and `apps/viewer` can change it.** The
  tonemap's multiplier was a compile-time constant; it now arrives in a uniform
  block, with `ForwardRenderer::set_exposure`/`exposure` and a range of five
  stops either side of the default. `-` and `=` step it in the viewer by a third
  of a stop — the camera exposure-compensation increment, so three presses is a
  doubling — and holding a key sweeps, which is what a continuous range wants.
  The listing panel shows the current value.

  A **uniform block rather than a push constant**, because WebGPU has no push
  constants: a range would split the pass into two layouts and two copies of the
  shader for the sake of four bytes a frame.

  The default is exactly the constant it replaced, so no golden moved.

- **`Capability::DrawIndirectCount` works on the Metal backend**, taking the
  parity blocker list from nine rows to eight — and it also unblocks
  `Capability::IndirectArgumentPaddedStride`, which the seam had been declining
  on Metal only because the draw-count ceiling was one. Metal's exercised
  capability coverage goes from 20 of 26 to 22.

  Metal has no GPU-side draw count and **no indirect command buffer is used**. A
  compute kernel packs the argument structures into a backend buffer and zeroes
  the instance count of every structure at or beyond the count the GPU wrote;
  the pass then issues that many ordinary indirect draws. A draw of zero
  instances emits no vertices by definition, so the surplus draws are no-ops.

  It packs rather than editing the caller's arguments: zeroing in place would
  leave the zeroes behind, and a later frame with a **larger** count would then
  draw nothing for every structure an earlier smaller count had reached.

  `max_draw_indirect_count` is 8. It is a deliberate bound rather than a device
  answer, because this design issues that many draws whatever the GPU count says
  — so raising it far is what would make it wrong.

- **`apps/viewer` draws a wireframe.** `W` toggles it, off by default.
  `ForwardRenderer` gained `set_wireframe`, `wireframe` and the associated
  `supports_wireframe`, so an application can ask before it offers the view.

  **Only the colour pass changes fill mode.** The depth prepass stays filled, so
  SSAO and SSR keep reading solid depth — depth drawn as lines is depth with
  holes in it. One consequence worth knowing: the lines are then shaded with
  occlusion from surfaces that are not drawn.

  A device without `Features::POLYGON_MODE_LINE` gets a refusal from
  `set_wireframe` rather than a silently filled frame, and the viewer reports
  the state **actually in force** in its debug panel rather than what was asked
  for. WebGPU has no line fill mode, so this is the ordinary case on the web
  rather than an error path.

- **`apps/viewer` lists what it loaded.** `I` toggles a panel naming the
  document, its mesh, vertex, index, material and texture counts, the page
  extent its textures were resampled to, the instances placed, the bounds the
  camera framed on, and **every feature the conversion skipped, by name**. Those
  skips were already produced and logged; a viewer whose job is opening files
  nobody curated should put "here is what did not come in" in front of the
  person who opened it, not only in a terminal they may not be reading.

  Off by default. The box is measured from the text and clamped to the
  framebuffer, so a narrow window cuts lines with an ellipsis and a short one
  drops the tail and says how much it dropped — the counts survive, the skip
  list is what shortens, and stderr still has all of it.

- **`apps/lantern` has mouse look.** Moving the mouse turns the view while
  flying; the pause panel releases the pointer. The keyboard turn is unchanged.

- **`HostedGame::pointer_mode` lets a hosted game ask for the pointer.** A
  defaulted, polled hook returning `PointerMode`, so every existing game is
  untouched: the loop reads it once a frame and calls `Shell::set_pointer_mode`
  only when the answer changes, so a game that always wants the same thing costs
  one virtual call and no shell traffic. The game states what it wants and the
  loop reconciles, rather than issuing commands it cannot see the result of.

  **A shell that cannot deliver relative motion is not asked to lock.**
  `ShellCaps::POINTER_LOCK` alone would accept the request and then hand back a
  hidden cursor and no motion, so the loop gates a lock on `has_mouselook()` —
  both `POINTER_LOCK` and `RAW_POINTER_MOTION` — and leaves the pointer free
  otherwise, logging once rather than every frame.

- **`apps/viewer`** — `viewer <MODEL>` opens a `.gltf` or `.glb`, frames the
  camera on it, turns it under the mouse and draws it under a single directional
  light. `docs/plan/sample/05-viewer.md`'s milestone 1 without the grid floor;
  `docs/backlog.md` lists what the sample still owes.

  The file's own directory becomes the asset root and its file name the key, so
  a `.gltf` resolves the `.bin` and the textures beside it. **Nothing it can be
  pointed at panics**: a missing file, a truncated container, a name the asset
  seam refuses, a directory, a document with no geometry and a document whose
  bounds are not finite are each a sentence on stderr naming the file and what
  to do, and exit code 1. Everything `build_render_scene` could not honour is
  **printed** as well as logged, because the conversion's warnings go through
  `CRCBL_LOG`, which under its default filter a user never sees.

  Controls are `three.js`-shaped, which is what a user arriving from a browser
  model viewer already has in their hands: left drag orbits with the model
  following the pointer, middle or right drag pans, the wheel zooms, `F` frames
  the model again. Drag deltas are `PointerUpdate::motion` — the unaccelerated
  `raw_delta` where the backend has one and differenced `abs` positions where it
  does not — so the browser backend drags too.

  It is **hosted by `crcbl::engine::Loop`** like every other sample, so `ESC`
  opens a panel, `F11` goes fullscreen and `F3` shows the debug overlay with the
  document's instance and skip counts on it. That took widening the loop's input
  — see the entry below.

- **`crcbl::engine::Loop` delivers the whole pointer, so a tool application can
  be hosted.** It used to fold a pump down to a position and the primary
  button's two edges: `Pending::observe` matched `PointerButton::Left` alone and
  `ShellEvent::Wheel` fell into its catch-all, so a hosted game could not be
  given a wheel or a second drag button at all. Three additions close that:
  - **`HostedGame::wheel_event(ScrollDelta)`** — one call per scroll the shell
    reported, never summed. `ScrollDelta` keeps detents and pixels apart on
    purpose and leaves the conversion to the application, so an engine that
    added a batch up would be choosing that policy for every caller.
  - **`HostedGame::button_event(PointerButton, bool)`** — every button that is
    not the primary one, once per edge, shaped like `key_event` because for a
    hosted game that is what a right-click is. The primary button is still
    `PointerUpdate`'s two edges, because it is the one a menu arbitrates. A
    button held when the window loses focus is **released** here, the way a held
    key is.
  - **`PointerUpdate::motion`** — how far the pointer travelled this frame, in
    framebuffer pixels: the unaccelerated `raw_delta` where the backend reports
    one and the difference of successive positions where it does not. It is not
    the difference of `PointerUpdate::at`, which is clamped at the edge of the
    display, carries pointer acceleration, and does not exist at all under
    `PointerMode::Locked` — a frame under a lock now reports `at: None` with a
    `motion` and is delivered.

  Both new methods default to doing nothing, like `pointer_event` and
  `touch_event`: nothing reads a value back out of them, so an unoverridden one
  is a game with no wheel binding rather than a check that passed by doing
  nothing. No existing sample changed. `apps/viewer` is the caller they were
  added for, and its migration onto the loop is what says they are enough.

- **`crcbl::assets`** re-exports `crcbl-assets`, so `AssetSource`, `DirSource`
  and the registry are reachable from a crate that names only `crcbl`. It was a
  dev-dependency of the umbrella while `crcbl::scene::import_gltf` took a
  `&dyn AssetSource` from it, which made that entry point one a caller could
  name and not call.

- **`crcbl_render::OrbitCamera`** — the orbit, pan, zoom and frame-selected
  controls the model viewer and the stage-8 editor viewport both need, in
  `crcbl-render` rather than in an app. It takes **deltas, never keys**: nothing
  in `crcbl_render::orbit` names a key, a button or a modifier, so an editor
  whose input model is not a sample's can reuse the arithmetic. `orbit` turns
  the eye about a fixed pivot — a positive yaw swings it toward the camera's own
  right, the same rotation sense as `apps/lantern`'s `Flyer` — and clamps the
  elevation to `orbit::PITCH_LIMIT` short of vertical, where `Camera::view`
  panics on a degenerate basis. `pan` slides the pivot across the view plane in
  **fractions of the viewport height**, so a drag tracks the pointer at any zoom
  and on any window size. `zoom` is multiplicative and changes distance, never
  the field of view, clamped to `orbit::MIN_DISTANCE..=orbit::MAX_DISTANCE` so
  no amount of it reaches the pivot or runs off to infinity. `frame` fits an
  `Aabb`'s **bounding sphere against both screen axes** — the horizontal
  half-angle is `atan(tan(fov_y / 2) · aspect)`, which is the smaller one in a
  window narrower than it is tall, so fitting by height alone leaves a wide
  object hanging off both sides — and holds the result at least `near + radius`
  away, so a point-sized selection is framed in front of the near plane rather
  than inside it.

  It produces a `Camera`, needs no device, and is perspective-only: `new`
  refuses an orthographic projection outright, because zoom under one is a
  change of `half_height` and a controller that accepted it would have a zoom
  that moved the eye and changed nothing on screen.

- **`crcbl_shaders::bindless_probe`** — the first committed shader in this
  workspace that declares an **array of descriptors**. Until it landed, every
  `Texture2DArray` here was one layered image, no committed SPIR-V declared
  `RuntimeDescriptorArray` and no WGSL contained `binding_array`, so
  `BindingFlags::VARIABLE_COUNT`, `BindGroupDesc::variable_count` and
  `BindGroupEntry::array_index` had nothing to be proved against on any backend.
  It is a compute shader over an unbounded `StructuredBuffer<uint> sources[]` at
  the last and highest-numbered binding, with a scalar destination in front of
  it, and it copies each descriptor's words into that descriptor's own block of
  the output — so a readback says which element of the array was read from which
  buffer. Emitted for SPIR-V and DXIL only: `crcbl-mtl` binds Metal's flat
  argument tables and `crcbl-webgpu` has no binding arrays at all, so both
  refuse the layout it needs and an MSL or WGSL artifact would be bytes nothing
  loads.

  With it, `Capability::BindlessDescriptorArray` is driven by the agnostic seam
  suite instead of being reported as a coverage gap — 22 of 26 capabilities on
  both `CRCBL_GPU=vk` and `CRCBL_GPU=wgpu`, up from 21.

- **`crcbl_shaders::push_constant_raster`** — the first committed shader in this
  workspace that reads a push-constant block from the **graphics** stages, a
  vertex/fragment pair emitted for SPIR-V, MSL and DXIL and, like
  `push_constant_probe` beside it, deliberately not for WGSL. The vertex stage
  takes the clip-space rectangle it draws from the block and the fragment stage
  takes the colour it writes, and the pipeline pulls no vertices from anywhere —
  so the layout it needs carries no bind group at all and the picture it
  produces has no source but the block. The module publishes the block's byte
  layout and the vertex count, because a backend with only a module and an entry
  point can guess neither.

  It exists because `Capability::PushConstants` is stage-agnostic while
  everything driving it was a compute dispatch, so every backend counted as
  covered while the plumbing that actually differs by stage — D3D12's per-stage
  root-parameter visibility, Metal's per-stage argument tables and
  `setVertexBytes:`/`setFragmentBytes:`, Vulkan's stage mask — had nothing
  asserting it. The seam suite now draws twice in one render pass with different
  blocks and requires each draw to have seen its own, which one draw cannot
  distinguish from two draws sharing one.

- **A removed D3D12 device now names the operation it died on.** `crcbl-dx12`
  turns DRED — Device Removed Extended Data — on before the first
  `D3D12CreateDevice`, forcing `SetAutoBreadcrumbsEnablement` and
  `SetPageFaultEnablement` to `D3D12_DRED_ENABLEMENT_FORCED_ON`, and reads it
  back off the dead device inside the message `crcbl_dx12::debug`'s `diagnosis`
  already builds beside `GetDeviceRemovedReason`. Where that message used to end
  at `DXGI_ERROR_DEVICE_REMOVED (0x887A0005)`, it now also carries, per command
  list, the debug names of the list and its queue, how many recorded operations
  the GPU finished, and a window of the command history around the boundary with
  the operation the GPU had not finished marked `IN FLIGHT` — plus, for a page
  fault, the faulting GPU virtual address with the live and recently freed
  allocations DRED names around it.

  It is **always on** rather than gated by `CRCBL_DX12_VALIDATION`: that
  variable means "this machine has Windows' Graphics Tools feature", DRED needs
  no such component, and a device removal is not a failure anybody gets to
  reproduce with a flag set the second time. The new `crcbl_dx12::dred` module
  argues it in full.

- **`crcbl-dx12` builds mesh pipelines.** `Device::create_mesh_pipeline` packs a
  `D3D12_PIPELINE_STATE_STREAM_DESC` for `ID3D12Device2::CreatePipelineState`,
  and `CommandEncoder::draw_mesh_tasks` / `draw_mesh_tasks_indirect` record
  `DispatchMesh` and an `ExecuteIndirect` of `DISPATCH_MESH`. The stream carries
  the amplification stage, so `MeshPipelineDesc::task` reaches D3D12's `AS`; it
  omits input layout, strip cut, stream output, vertex shader and primitive
  topology, the last because the seam documents `PrimitiveState::topology` as
  ignored for a mesh pipeline — so a mesh pipeline records no
  `IASetPrimitiveTopology` at all rather than setting `UNDEFINED`, which is a
  debug-layer error.

  No `Device` entry point on this backend answers `Unsupported` any more.

  **The backend still reports no `Features::MESH_SHADER`**, so `GeometryPath` is
  unchanged, every golden keeps its key, and a bind group reaching these stages
  declares `ShaderStages::ALL` — `check_supported` refuses the mesh bit on an
  adapter that reports no mesh support, and `ALL` maps to
  `D3D12_SHADER_VISIBILITY_ALL`, which reaches both stages. Reporting the flag
  is a separate change with a golden re-bless in it, and the `MeshShading` and
  `TaskShaderStage` divergences stay until then — with reasons that now say the
  calls exist and the flag does not, rather than claiming no stream is built.

- **`crcbl-dx12` gained five capabilities it used to refuse**, taking it from
  sixteen open parity divergences to six. It copies image to image (depth and
  stencil planes included — an image-to-image copy needs no placed footprint, so
  the two obstacles that block the buffer path vanish), copies a depth or
  stencil plane to and from a buffer, hands out timeline and binary semaphores
  and waits on either from the CPU or the queue — including a queue wait on a
  value nothing has signalled yet, which D3D12 permits and Metal cannot —
  creates all three kinds of query heap, and resolves a multisampled attachment
  after its pass.

  Two of those carry a caveat worth knowing. `Device::query_results` on a
  pipeline-statistics set is refused: the seam gives one `u64` per query and
  D3D12 resolves a fixed 88-byte struct, which is a seam defect rather than a
  backend one — Vulkan's statistics pools are 24 bytes for the same reason. And
  a copy between a colour format and its sRGB partner is refused although D3D12
  would take it, because the seam's `Format` has no typeless-family relation and
  a silent reinterpretation is worse than a loud refusal.

- **Push constants work on every backend that has them.** `crcbl-dx12` builds a
  `D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS` at the `b` register the committed
  DXIL puts the block at, and `crcbl-mtl` binds it with `setBytes:` at the
  argument-table index the committed MSL leaves free. Both report
  `Features::PUSH_CONSTANTS` and a real `max_push_constant_size` — 256 bytes on
  D3D12, which is the whole root signature and therefore a ceiling rather than a
  promise, and 4096 on Metal, which is Apple's published `setBytes` limit and is
  not shared with anything.

  Neither slot is a number somebody chose. HLSL and MSL have no push constants,
  so Slang emits the block as an ordinary constant buffer and the compiler
  numbers it in declaration order — and `crcbl-shaders`' declaration-order lint
  requires a push constant to be declared **last**. So it always lands after
  every bound resource, on both targets. WGSL has none at all, which is why the
  probe shader ships for three targets and not four.

- **`crcbl-webgpu` streams `set_stencil_reference`.** It used to record the call
  as unsupported, which made `finish()` refuse the whole command buffer rather
  than the frame merely coming out wrong.

- **`crcbl_shaders::push_constant_probe`** — the first committed shader in this
  workspace that declares a push-constant block, emitted for SPIR-V, MSL and
  DXIL. There is no WGSL target because WGSL has no push constants, which is
  also why `ui.slang`'s block became a bound uniform buffer. The module
  publishes the workgroup size, the word count and the block size, because a
  backend with only a module and an entry point can guess none of them.

- **A reviewed parity contract between the backends: `crcbl_hal::DIVERGENCES`.**
  Every capability a backend knowingly lacks on every device it can open is a
  listed `(capability, backend, why)` row — Metal's absent GPU-side draw count,
  D3D12's missing buffer fill, `crcbl-wgpu`'s zero-only fill, WebGPU's immutable
  bind groups, and thirty-odd more. `crcbl_hal::parity_verdict` is the rule that
  reads it, answering a `crcbl_hal::ParityVerdict`: a refusal is accounted for
  when the pair is on the list with a reason, or when the backend answered
  `Support::NotOnThisDevice` because the device itself withheld the gating
  `Features` flag.

  **Whose refusal it is decides which.** A backend's own refusal needs a listed
  row on _every_ device, including one reporting the gating flag clear — so a
  row cannot be retired by a device that could not have proved anything either
  way. That distinction is `Support::NotOnThisDevice`, and it is checked against
  the device rather than believed: a backend claiming it for a capability with
  no gating feature, or on a device that reports the flag, is
  `ParityVerdict::FalseDeviceGate` and fails. The verdict a device cannot settle
  is `ParityVerdict::UnprovableHere`, which the seam suite names and counts in
  its report instead of skipping silently.

  `crates/crcbl/tests/hal_seam_e2e.rs` enforces both directions against a real
  device, so divergence stays possible and stops being accidental — a backend
  that starts refusing something new fails until somebody writes down why, and a
  backend that starts _supporting_ something listed fails until the row is
  deleted. The same file drives the capability declarations against the
  hardware: a backend that claims a behaviour and refuses it fails, one that
  declares a behaviour absent and performs it fails, and one that accepts a call
  and silently does nothing fails hardest — every return value says success,
  which is how the `fill_buffer` divergence reached CI in the first place.

  `crcbl_hal::BackendKind::is_gpu` is new alongside it: `Null` records rather
  than executes, so it is outside the parity model.

- **A glTF file now reaches pixels.**
  `crcbl_scene::gltf_render::build_render_scene` turns an imported `GltfScene`
  into the `SceneDesc` a `ForwardRenderer` makes resident and the
  `InstanceDesc`s that place it — the piece that was missing, since
  `import_gltf` had no consumer in the workspace but the LOD command and
  `crcbl-render` did not know it existed. Behind `crcbl-scene`'s new `render`
  feature, which `crcbl`'s `scene` feature turns on, so `crcbl-cli` still links
  no renderer.

  One `MeshDesc` per glTF primitive and one instance per node/primitive pair,
  because a material is per primitive and an `InstanceDesc` carries one of each;
  node transforms arrive composed from the root, rotation and scale included.
  Material row 0 is the glTF specification's own default material — an untinted
  fully rough conductor — so the document's material `n` is row `n + 1`. A
  primitive with no `NORMAL` is de-indexed and given flat face normals, as the
  specification requires of a client. Pool capacities are measured off the
  description rather than taken from `Capacities::default`.

  **Textures work.** `import_gltf` now carries each image's encoded bytes
  (`GltfScene::images`) and which one a material's `baseColorTexture` names
  (`GltfScene::base_color_textures`), and the bridge decodes the PNGs, resamples
  them onto one square page extent — the largest side of the largest image,
  capped at `MAX_PAGE_EXTENT` — and points each material row at its layer. The
  resampler averages in linear light and weights by alpha, so a downscale is not
  darkened by averaging sRGB bytes.

  **Everything unsupported is skipped loudly**, which is
  `docs/plan/sample/05-viewer.md`'s exit criterion: `RenderScene::skipped` lists
  every one as a `Skip` naming the feature, where in the document it was, and
  what happened instead, and each is logged with the file's key as it is found.
  The conversion itself cannot fail — a document nothing could be made of gives
  an empty scene and a full list of why. Skipped today: images that are not PNG
  (JPEG in particular — there is no JPEG decoder in the workspace), images whose
  URI will not resolve or is a `data:` URI, `texCoord` sets other than 0,
  non-uniform node scale (the object still draws; its normals light wrongly),
  and any primitive the meshlet builder refuses. `import_gltf` additionally
  warns when a document carries skins, animations or morph targets, which it
  does not read.

  An image that will not resolve is now a skipped image rather than a failed
  import — a buffer in the same state still fails, because a file without its
  geometry has nothing to draw where one without its texture draws white.

  `crcbl_scene::gltf_check` grew the refusals the texture side needs, on its
  existing terms — every index the `gltf` crate resolves with an `unwrap` is
  checked before that crate is allowed near it. A malformed image, texture,
  sampler or material texture reference is now a `StorageError::Other` naming
  the file and the defect, where it would otherwise abort the process. That
  includes a `textures` entry with no `source`, which `gltf-json` defaults to
  `u32::MAX` rather than modelling as absent, and which is what a document
  supplying its image through `KHR_texture_basisu` looks like.

- **Physical-size texture tiling, so an asset's size reads at a glance.** A
  material whose `tiling` is `GpuMaterial::TILING_PHYSICAL` derives its sampling
  UV from the surface's world-space extent instead of the authored UV, repeating
  the texture once per `tile_metres` (default `1.0`). A 1×1×1 m cube then shows
  one texture cell per side and a 2×2×2 m cube shows 2×2 of them, where an
  authored UV stretches the single cell across a face of any size. The mesh
  shader projects onto the dominant normal axis, exact for the axis-aligned
  faces a blockout is built from. `crcbl-greybox` ships seven 1024×1024 gridded
  one-metre tiles using it — grey, red, green, blue, orange, brown and black —
  through `GreyboxColor`, `greybox_page()` and `greybox_color_material()`, so a
  greybox surface reads as a metric ruler at any scale.

- **`crcbl-greybox`, a prototyping asset pack sized in reality.** A crate of
  ready-to-drop blockout assets so a game can stand up a level before it has
  art. The 3D half is procedural geometry sized in metres — cube, wall, doorway,
  ramp, stairs, column, cylinder, sphere, a 1.8m human-scale capsule, platform —
  with a `scene3d()` `SceneDesc` and index constants. The 2D half, behind the
  default `bake` feature, is a set of committed `.crpix` sprites baked into the
  crate and read back as `Loaded` sheets: grid tile, solid, circle, checker,
  thin and one-way platforms, 45° and 2:1 slopes, ladder, spike, a 1×2-tile
  player and an enemy placeholder in a magenta actor tint, pickup, and an arrow.
  They are authored at a 32-texel base tile, so a game that sets its own
  `TEXELS_PER_UNIT = 32` gets one tile per world unit; `GreyboxSprite::texels`
  gives each sprite's size and `GreyboxSprite::ALL` the whole set.
  `--no-default-features` drops the 2D half for a geometry-only pack.

- **`crcbl-webgpu`, a new crate holding the command-stream encoding** the coming
  browser backend will speak: wasm serialises HAL calls into a buffer it owns
  and JS decodes and replays them. This first piece is the encoding alone — a
  writer, an explicit tag table, and a bounds-checked reader that exists for
  testing and for dumping a stream, since the production decoder is JS.

  It covers one command per encoding shape rather than the whole HAL surface, so
  the remaining commands have a worked example to follow. The crate deliberately
  depends on nothing browser-shaped: no `wasm-bindgen`, `web-sys` or `js-sys`, a
  graphics import here being the thing the whole design exists to avoid.

  `docs/plan/41-webgpu-stream.md` is the specification.

- **`web/engine/gpu-stream.js`, the decoding half of that stream**, and a
  committed fixture that keeps the two halves honest. The format has two
  hand-written implementations in two languages and no compiler reads both, so
  the Rust side commits a canonical stream, a Rust test asserts it still encodes
  byte-for-byte to that fixture, and a node check decodes the same bytes with
  the JavaScript decoder and asserts every field. Either half drifting fails one
  of the two. The node check runs in the Pages build, which covers pull
  requests.

  Handles arrive as `{ index, generation }` rather than one number, and 64-bit
  scalars as `BigInt`: `Handle::to_bits` puts the generation in the high half,
  so a handle read as a JS number is silently wrong once the generation passes
  2^21, and `WHOLE_BUFFER` read as a number becomes a different, still-enormous
  size that nothing downstream would question.

- **`crcbl::webgpu`, and the browser transport under it.** `crcbl-webgpu` is now
  a `wasm32`-only dependency of the umbrella, so every demo's artifact exports
  `__crcbl_web_gpu_stream_len`, `_ptr` and `_release`, and
  `web/engine/gpu-transport.js` drains them from the shim's frame loop beside
  the fetch drain.

  **Nothing encodes into the stream yet**, so those exports answer `0` every
  frame and the drain returns nothing. The transport is wired and gated rather
  than carrying commands, and both halves say so where a reader will see it. It
  is linked now so the export contract is exercised by the real browser gate
  rather than by a crate no page builds in; it costs about 1.5 KB in each demo's
  shipped wasm, measured.

- **`BackendKind::WebGpu` and `GpuBackend::WebGpu`**, printing and parsing as
  `"webgpu"`, selectable with `CRCBL_GPU=webgpu` or `--backend webgpu`.

  Asking for it today **fails loudly**: `GpuError::Backend` wrapping
  `HalError::Unsupported`, whose message says the backend is not implemented yet
  and what is missing behind it. It is never selected automatically and never
  falls back to another backend, because a fallback here would turn "not built"
  into a passing run on a different backend.

- **A browser opens a device through the stream.** `Command::RequestDevice`,
  `Reply::Device` and `Reply::DeviceFailed`, and a `crcbl_webgpu::device` module
  whose `DeviceProbe` is the polled state between asking and opening.

  The device's capabilities are the **device's**, not a copy of the adapter's —
  WebGPU grants what was asked for, not everything the adapter has — and the
  browser gate proves they differ by opening a device for itself with the same
  descriptor and comparing.

  A required feature with no WebGPU name **fails the request before the browser
  is asked**, carrying the exact bits that could not be satisfied, rather than
  being dropped so the engine receives a device lacking something it declared it
  needed. Optional features are filtered to what the adapter has, because
  `requestDevice` fails the whole request over one it lacks — which would turn
  "nice to have" into fatal.

  Neither wire version moved: this adds tags rather than changing a record, and
  an unknown tag already fails loudly naming the byte, where a version bump
  would refuse buffers the older half still decodes correctly.

- **The adapter reply now carries the whole of `crcbl_hal::AdapterInfo`**,
  `DeviceCaps` included, so what a browser reports reaches the capability seam
  instead of an id and a name. `Reply::Adapter` holds an `AdapterInfo`,
  `ReplyWriter::adapter` takes `&AdapterInfo`, `AdapterProbe::adapters` returns
  `Vec<AdapterInfo>`, and neither is `Eq` any more because `Limits` has `f32`
  fields. The reply wire format is version **2**, and `DecodeError::InvalidEnum`
  carries a `u64` so a 64-bit feature word fits.

  **The fields WebGPU cannot supply are marked unknown, not filled in.**
  `vendor_id` and `device_id` are `0`, `device_type` is `Other` — "declined to
  say" — and `driver` is empty, because `GPUAdapterInfo` gives strings and no
  numeric ids, and WebGPU never says discrete or integrated. A plausible-looking
  fabricated id would be indistinguishable downstream from a real one.

  The feature mapping is explicit and lossy in both directions, and both lists
  are written down where the mapping lives: four HAL flags are granted by core
  WebGPU, four come from named features, nineteen can never be set, and every
  WebGPU feature with no HAL bit is dropped — including ones a browser invents,
  since the code iterates its own table rather than the spec's.

  The browser gate corroborates it: the feature word wasm received is compared
  against one the page computes from the live `adapter.features`, using bit
  numbers spelled out in the driver rather than imported from the code under
  test.

- **The first round trip: wasm asks the browser to enumerate adapters and reads
  the answer back.** `Command::EnumerateAdapters` and `Reply::NoAdapter`, a
  `FAMILY_INSTANCE` tag range, `StreamChannel::encode_awaited`, and
  `AdapterProbe` in `crcbl_webgpu::instance`. `web/engine/gpu-replay.js` is the
  first real replayer — it executes decoded commands against WebGPU — and
  `web/engine/gpu-probe.js` drives a round trip from a page.

  WebGPU's adapter request is asynchronous and the stream is replayed
  synchronously once a frame, so the reply lands on a **later** frame, named by
  the sequence of the command that asked. That is what the sequence numbers were
  for, and it is now exercised rather than argued.

  The browser gate carries the evidence: it compares the adapter name wasm
  received against what `navigator.gpu` reports to the same page at the same
  moment, so a replayer answering with a constant fails it.

  **This is not the backend.** There is no `impl Instance` — `AdapterInfo` has
  eight fields and this channel carries two — and `crcbl::backend` still refuses
  `webgpu`. The probe entry points exist to observe the round trip and are
  deleted when a real backend installs its own channel.

- **The reply channel, JS to wasm** — `Reply`, `ReplyWriter`, `ReplyReader` and
  `decode_replies`,
  `StreamChannel::{expect_reply, waiting_replies, drain_replies}`, four
  `__crcbl_web_gpu_reply_*` exports, and `web/engine/gpu-reply.js` as the
  browser-side encoder.

  Replies are a stream of the same shape as the commands, with their own magic
  so one direction's buffer cannot be read as the other's. Each reply names the
  command it answers by sequence number, explicitly rather than positionally,
  because replies need not arrive in order or at all — and **a reply for a
  sequence nothing is waiting on refuses the whole buffer** rather than being
  dropped, since a replayer answering the wrong command is precisely the bug
  this channel would otherwise hide.

  `__crcbl_web_gpu_reply_buffer` is the one export that can grow wasm memory, so
  it is the one a JS view must be built after and never across.

- **Five logging macros in the engine — `error!`, `warn!`, `info!`, `debug!`,
  `trace!`** — plus `log_at!`, which takes the level and which the other five
  forward to so the target, the level check and the route to the sink are
  written once. They take `format!` arguments and tag each record with the
  calling module, which is what `CRCBL_LOG`'s per-module directives match.

  A filtered-out call does not evaluate its argument expressions. That is
  narrower than it sounds and the docs say so: `format_args!` already defers the
  _formatting_, so what the level check saves is evaluating the arguments and
  calling into the sink at all.

- **A wall-clock start banner**, written once by `init_logging`:
  `run started 2026-08-15 05:20:07 UTC`. Every other line still carries
  seconds-since-start, which is the question a frame loop asks; the banner is
  what lets those seconds be lined up against something outside the process
  without paying a date conversion per line. The date arithmetic is Howard
  Hinnant's `civil_from_days`, transcribed and checked against known timestamps
  and a full four-century round trip.

- **The browser log queue carries the same prefix as the native sink.** It had
  none: `Instant::now` panics on `wasm32` and the module imports no
  `console.log`. It reads the `performance.now()` the shim already hands to
  `App::frame`, so the two formats are now one. No wall-clock banner there —
  `SystemTime::now` panics on that target too, and the console stamps each line
  as the shim prints it.

- **`crcbl::engine::open_shell(headless)`**, which opens the headless backend by
  name or the platform's, and which all six samples now call instead of writing
  the pair out.

  The copies were a place to be quietly wrong. Both arms take the _same_
  `ShellError` and differ only in which `LoopError` variant wraps it, and only
  `NoWindowSystem` carries the hint telling a user that `--headless` runs
  everywhere — so mapping a failed `open()` to `Shell` would have lost the hint
  and changed nothing else. It is generic over the game's error alone because
  `LoopError` already is.

- **`crcbl::engine::DEFAULT_WINDOW_SIZE` and
  `crcbl::engine::requested_window_size`.** `LogicalSize::new(960.0, 720.0)` was
  a bare literal in all six samples, and the rule beside it — `--size` names
  pixels, a window request is logical, so convert at scale 1 — was written out
  next to each one. The `crcbl new` scaffold had already given the number a
  name; now the engine gives it one, and the conversion with it.

  The scale-1 part is the half worth a name. Converting at the display's real
  factor instead would open a 960×540 window for `--size 1920x1080` on a 2×
  display, and the headless offscreen ring renders at exactly the extent that
  was asked for — so the windowed run and the headless one would silently frame
  different scenes. A test pins it.

- **`Backing::platform(app_name)` and `crcbl_store::web::opfs::installed()`.**
  `platform` is "where this machine keeps a small persistent value" in one place
  — the config directory natively, the installed OPFS store in a browser,
  `Backing::None` when neither can be had. Four games had written that rule out
  themselves as a `#[cfg]`-split `platform_backing`, which is a fact about the
  platform and not about any game.

  `installed()` is what makes the browser arm reachable from the engine: the
  slot already held a `Weak` for the `__crcbl_web_opfs_*` exports, and it now
  has a getter, so the record and the entry points read the _same_ store instead
  of two paths agreeing by convention. A game no longer needs its own
  `opfs_store` accessor over `web_exports!`'s `STORAGE` cell, and the four that
  had one have dropped it.

- **`crcbl::impl_polled_bundle!`**, which declares a bundle's `Pending` type,
  its `poll`, and the `open`/`request_open` pair — the blocking and non-blocking
  halves of start-up, both routed through the _same_ descriptor.
  `impl_polled_bundle!(gpu: Gpu, pending: PendingGpu, desc: desc)` replaces
  about thirty lines that five samples carried identically.

  **The descriptor function is named, not generated**, and that is the design
  rather than an omission. Generating it would leave the label as the only knob
  and would make each sample's "the features I ask for are the engine's own"
  test vacuous — a check that cannot fail is not a check, so those tests would
  have had to go, and what replaced them would have been harder to write than
  what it replaced. `apps/lantern` could not use a generated one at all:
  overriding `optional_features` is how it forces a lesser path.

- **`crcbl::impl_game_gpu!` and `crcbl::impl_polled_gpu!`**, which write the
  `GameGpu`/`GpuSurface` and `PolledGpu` impls for a bundle that already has the
  methods as inherent ones. Every sample had these blocks written out byte for
  byte; `impl_game_gpu!(Gpu)` and
  `impl_polled_gpu!(gpu: Gpu, pending: PendingGpu)` replace them.

  They are two macros rather than one with a flag because `PolledGpu` is the
  half a bundle can outgrow — `apps/lantern` threads its own defaults into
  `request_open` and writes that impl by hand.

  **`impl_game_gpu!` opens with a `const _` block coercing each inherent method
  to a function pointer, and that block is load-bearing.** Each forward is
  `Self::method(self)`, which resolves to the _trait_ method when the bundle has
  no inherent one — infinite recursion rather than a compile error. Hand-written
  that is caught by `unconditional_recursion`, but rustc suppresses its lints
  inside an external macro's expansion, so the guard restores what the move
  would otherwise have taken away: a missing method is now `E0599` naming it.

- **`SpriteRenderer::register_baked` and `crcbl_sprite::load::load_baked`**, the
  two halves of "turn a baked sheet into a registered one" that every caller of
  `crcbl_sprite::load` had written out for itself. `register_baked` takes a
  `&Loaded` and does the `SheetDesc` mapping — size off the image, sampler mode
  off the sheet; `load_baked` is `load` for bytes that came from
  `include_bytes!` on a `build.rs` product, panicking with the sheet's name
  because a failure there is a broken build rather than input a caller could
  handle. It is `#[track_caller]`, so the panic names the line that asked for
  the sheet.

  A game that already has an `ART_TICK_HZ` still needs a one-line local wrapper
  to supply it — that constant is generated per crate, so the rate is the
  caller's and only the failure policy is shared.

- **`apps/lantern` runs in a browser.** The sixth demo on the Pages site, and
  the first written against `crcbl::web_exports!` rather than migrated onto it:
  `apps/lantern/src/web.rs` is the macro invocation naming the ten
  `__crcbl_lantern_*` symbols plus a `crcbl::web::WebPending` impl, and nothing
  else. The package is a `cdylib` now, `apps/lantern/src/gpu.rs` implements
  `crcbl::engine::PolledGpu` — the browser's non-blocking device request — and
  `crcbl_lantern::PendingLoop` is its polled start-up.

  It is worth publishing for one reason: WebGPU exposes no ray query, so the
  page draws the room through `LightingPath::Rasterised` **by construction**,
  and it is the one place that path can be looked at without building anything.

  **It needs a real GPU**, which no other demo here does. The draw-argument pass
  binds fourteen storage buffers in one compute stage and Chrome's SwiftShader
  adapter caps a stage at ten, so on a software adapter the pipeline is never
  created and the canvas stays black. The page says so, and `docs/backlog.md`
  carries the measurement — including why `.github/workflows/pages.yml` has no
  browser-gate step for this one demo.

  `Lantern` also logs a `[HUD]` heartbeat from inside the tick, the same shape
  and cadence every other sample uses: the lighting path the frame took, and the
  orbiting lamp's position. Those are the two things
  `web/tools/browser-e2e.mjs`'s new `lantern` row asserts.

- **`crcbl::web_exports!`: a sample's browser entry point, written once.** The
  ten `#[unsafe(no_mangle)] extern "C"` symbols a demo's JS shim calls —
  `prepare`, `log_level`, `boot`, `frame`, `status`, `shutdown`, `error_ptr`,
  `error_len`, `log_take`, `log_ptr` — plus the page state they run on, emitted
  from one place instead of copied per sample. `apps/asteroids`,
  `apps/breakout`, `apps/flappy`, `apps/horde` and `apps/hud` each dropped their
  copy; the wasm export set of all five is byte-identical before and after,
  which is the thing that matters, because the shim resolves these names at run
  time and a renamed export produces a page that loads and stays blank rather
  than a build error.

  The macro takes each symbol name as a **named** argument rather than a prefix:
  `concat_idents!` is not stable, two demos can be open in one browser so the
  names must not collide, and `web/tools/check-exports.mjs` learns the contract
  by scanning for literal `__crcbl_…` names on the JS side. What stays in a
  sample is its `crcbl::web::WebPending` impl — the options the game boots with
  and the error it fails with are the game's own — and, for the four samples
  with a save file, the `opfs_store`/`asset_source` accessors over the `STORAGE`
  cell the macro emits.

- **`SceneDesc::probes` and `Capacities::probes`: an irradiance-probe volume a
  scene can author.** `docs/plan/18-render-features.md`'s diffuse global
  illumination as a static grid of L1 spherical-harmonic probes —
  `crcbl_render::ProbeGrid` is the volume (an origin, a reciprocal spacing and a
  count per axis) and `crcbl_shaders::probe::GpuProbe` is one probe's row.
  `mesh.slang` interpolates the grid trilinearly and **adds** the result to
  `frame.ambient.rgb`, so an author who wants the probes to be the whole
  environment sets `DirectionalLight::ambient` to zero. It adds no render pass
  and no device requirement.

  Fill a row with `GpuProbe::accumulate(direction, radiance, solid_angle)`,
  summed over a partition of the sphere: it is the projection with the
  clamped-cosine transfer already folded in, and it is the only correct way to
  fill one — the band scales come from Ramamoorthi & Hanrahan 2001 and are
  checked against that paper's values. There is no bake tool, on a hard
  prerequisite rather than on taste: a gather bake needs a ray-triangle
  intersector and a BVH, and `crcbl-phys` has neither.

  `apps/lantern` now authors a deterministic volume baked from the room's own
  dimensions: cube-face quadrature gathers the sun's first bounce from the
  axis-aligned shell, including visibility through the window reveal. The
  coloured wall measurably tints neighbouring plaster; interior props, lamp
  bounce and later bounces remain intentionally outside this analytic model.
  Other scenes retain the empty default, which evaluates to exactly zero and
  leaves their goldens byte-identical. A description whose probe count disagrees
  with its volume's counts, or which needs more rows than `Capacities::probes`
  reserves, is refused by name at `ForwardRenderer::with_scene` like every other
  capacity.

  `crcbl screenshot --scene probes` is the dedicated device-path fixture. Its
  two-probe grid clamps most of the floor to broad endpoint regions and confines
  interpolation to a narrow central band, avoiding the cross-rasteriser drift of
  the reverted full-frame gradient. The render e2e gate compares each endpoint
  and the centre against `probe::irradiance_at`, asserts the endpoint regions
  remain flat, matches a golden, and renders through both geometry paths.

  SSR misses now evaluate the same probe table as approximate L1 radiance and
  blend it with screen-space hits by the march's confidence. The pass removes
  the diffuse clamped-cosine transfer from each stored SH band before specular
  evaluation; a zero volume preserves the previous hit arithmetic exactly.
  lantern's golden gate renders authored and zeroed rows separately, proving the
  off-screen part of its mirror is lit by probe data while its screen-space hit
  remains. Rough surfaces receive the same environment specular even though
  `ROUGHNESS_CUTOFF` prevents them from screen-space marching: attachment alpha
  stores that march's sharpness ramp, so cutoff alpha survives `Rgba8Unorm` as
  exact zero. The blur composites that zero-sharpness probe fallback directly;
  positive sharpness blends continuously from the centre fallback into filtered
  SSR through a square-root filter share, retaining enough smoothing at the
  middle of the ramp to remove fixed-stride march stepping. The dedicated
  `Scene::Probes` fixture disables reflections so its absolute Rust comparison
  remains a diffuse-irradiance contract.

- **`apps/lantern`'s pause menu switches the effects, mid-run.** Three rows
  below `CAMERA` — `SHADOWS`, `AO` and `REFLECTIONS`, the words `--no-shadows`,
  `--no-ao` and `--no-reflections` already use — each labelled with what the
  frame is drawing and each swapping it on ENTER. Comparing a shadowed room with
  an unshadowed one is now a keypress instead of a restart with a different
  flag, and the flags still set the state the panel opens in.

  A row shows the **resolved** answer rather than the requested one, so an
  effect a device cannot draw reads `UNAVAILABLE` rather than `OFF` and the
  panel never offers a switch that does nothing. Pressing such a row changes
  nothing, since the device clamp is the last layer and no override can escape
  it. Under the row, `crcbl_lantern::toggled_effect` is read-modify-write on the
  **programmatic** layer of the resolution order alone: the camera stack and
  `[engine.video]` fields come back as they went in.

- **`crcbl::screenshot::OffscreenSetup::open_forward_with`** — `open_forward`
  taking the optional feature set to open the device with, instead of always
  asking for `OffscreenSetup::OPTIONAL_FEATURES`. `open_with` is the same knob
  for the `Scene` variants this module owns; this is it for a `ForwardScene` the
  caller built. An adapter reports what it reports, so withholding a feature it
  has is the only way one machine draws an application's scene on more than one
  `GeometryPath` — without it every such frame comes off the best tail available
  and the lesser paths, which is what browsers and Apple devices run, are never
  executed. `open_forward` is unchanged for existing callers: it delegates here
  with `OPTIONAL_FEATURES` and still draws on the best path the adapter offers.

- **Per-effect toggles, resolved through topic 39's four layers.**
  `crcbl_render::effects` is the resolution point: `RenderEffects` is the effect
  set (`SHADOWS`, `AMBIENT_OCCLUSION`, `REFLECTIONS`), `EffectRequest` carries
  the three layers a caller supplies, and `EffectRequest::resolve` applies
  camera stack → `[engine.video]` → programmatic override → device clamp in one
  place. `ForwardRenderer::set_effect_request` is the setter,
  `ForwardRenderer::effects` is what the frame in flight actually draws, and
  `begin_frame` resolves once and freezes the answer for that frame.

  **Nothing changes for a caller that does not ask.** The default request is
  every effect, so every existing frame is the one it always was and no golden
  moved on any rasteriser.

  A switched-off effect is fewer recorded passes and one different bound
  descriptor, never a shader permutation: shadows off records no cull and no
  draw into the atlas, which keeps its reversed-Z clear and reads as fully lit;
  reflections off tonemaps the forward pass's own scene colour, bit for bit;
  ambient occlusion off records neither occlusion pass and binds a
  renderer-owned 1×1 white image in place of the channel, which `mesh.slang`
  reads as "nothing occludes" because it clamps that fetch to the image it is
  reading. `apps/lantern` reaches all three from the command line with
  `--no-shadows`, `--no-ao` and `--no-reflections`, and its debug panel and
  headless summary both name the **resolved** set.

  Two of the four layers are present and have no source in the tree — there is
  no render-stack RON, and nothing builds a settings stack at startup — and the
  device clamp currently removes nothing, which is a fact about these three
  effects rather than an unfinished clamp. `docs/backlog.md` carries all three
  statements and what each would take.

- **Screen-space reflections.** `shaders/ssr.slang` marches the depth prepass in
  screen space, reads `F0` and screen-march sharpness out of the reflectivity
  attachment, and writes the reflection it finds; `shaders/ssr_blur.slang`
  filters that with `ssao_blur.slang`'s 4×4 kernel and adds it to the scene
  colour — two full-screen passes between the forward pass and the tonemap,
  added to every frame `ForwardRenderer::add_passes` builds. A ray that finds
  nothing adds the probe environment; a zero probe volume still adds exact zero.

  The kernel is weighted on view-space depth **and** on how sharp each tap's own
  surface is, so a reflecting surface does not average with the matt floor it
  stands on and a mirror does not average with a rough metal beside it. The
  second weight is the march's own roughness ramp, which it writes into the
  alpha of the image the blur reads. What the filter is worth is a number rather
  than a picture: the reflection in `Scene::Ssr` alternates by 17.7 levels from
  one row of the floor to the next without it and 2.8 with it.

  Mirror-sharp only for now: the weight ramps linearly to nothing at
  `crcbl_shaders::ssr::ROUGHNESS_CUTOFF` (0.5), which is
  `GpuMaterial::UNTINTED`'s own roughness, so every surface nobody gave a
  material to weighs exactly zero, the blur pass hands such a pixel's scene
  colour straight through, and every golden in the tree but three is
  bit-identical.

  `ForwardRenderer::add_passes` now returns **the composited frame** rather than
  the target the forward pass wrote. Both are `Rgba16Float` transients of the
  same description, so a caller reading the return value back gets the scene
  _with_ its reflections; a caller that had stored the old id is reading the
  frame the tonemap did not resolve.

  Two things this needed and did not have: `TransientImageDesc::reflectivity`
  gained `SAMPLED`, and the forward pass now **stores** its depth instead of
  discarding it. The second was a real bug rather than a tidy-up — a discarded
  attachment is undefined, not "what was written", so the same build drew
  reflections on Vulkan and none at all on wgpu with nothing reporting an error.

- **`crcbl::screenshot::Scene::Ssr`** — a smooth floor with the plain pyramid
  standing on it, seen from just above the floor, and
  `crcbl screenshot --scene ssr` draws it. The one frame in the tree whose
  subject is the march.

- **`crcbl_shaders::ssr`** — `SsrParams` (the two projection matrices
  `ssr.slang` reads) and `ROUGHNESS_CUTOFF`, so an application can say which of
  its own materials the pass can see.

- **`crcbl_render::TransientImageDesc::reflectivity`** — the `Rgba8Unorm`
  transient the forward pass writes `F0` and roughness into, beside
  `scene_color`, `scene_depth` and `ambient_occlusion`. `COLOR_ATTACHMENT`,
  `TRANSFER_SRC` and — since the reflection pass — `SAMPLED`. The forward pass
  clears it to zero, so a pixel no geometry covered says "nothing reflects here"
  rather than holding whatever was in the memory.

- **`apps/lantern` — the lighting acceptance fixture, at milestone 1a.** One
  indoor room, described by the sample rather than by the engine: nine meshes
  baked from literals through `crcbl::scene::build_meshlets`, five material
  rows, a two-layer base-colour page and its own `Capacities`. A window the sun
  comes through, a mirror-grade panel, a rough metal block, a coloured wall and
  a moving point light — plus a fixed camera the goldens are taken from, a
  keyboard free-fly camera, and `--force-geometry` / `--force-binding` for
  running the frame on a path below the one this device selects.

  **The metal is lit by reflection alone.** Ambient scales the diffuse albedo
  and a conductor has none, so a fully metallic surface out of every light's
  specular reach has nothing to shade with but a reflection. Both halves of that
  are built — see the screen-space-reflection and irradiance-probe entries above
  — so the debug panel's `unbuilt` section no longer reports the metal as black;
  its `metal` row now names what a reflection actually falls back to, which is a
  baked probe volume rather than a trace of the room.
  `docs/plan/sample/13-lantern.md` says what is still owed — ray tracing, the
  monitor camera and the web demo.

  `apps/lantern/tests/run-lantern-golden.sh` renders the fixed camera on a named
  backend and checks five claims about **where** the frame is bright and dark
  before comparing it against a golden. It is the first application scene in the
  tree; every other frame draws `crcbl_render::scene::demo`.

- **`crcbl::screenshot::OffscreenSetup::open_forward` and `ForwardScene`, so an
  application can render its own scene offscreen.** `OffscreenSetup::open` takes
  one of the engine's own `Scene` variants; this takes a closure handed the
  device, the queue and the surface format, and returns the `ForwardRenderer`
  the caller built along with its camera and sun. Everything below that — the
  offscreen surface, the adapter pin, the swapchain ring, the barriers around
  the readback and the row unpadding — is unchanged and shared, which is what
  keeps a sample from rebuilding it.

  Fixed alongside it: a scene that refused during `open` — `Scene::Dunes` on a
  device with a mesh stage and no amplification stage — left its swapchain and
  its surface behind.

- **`crcbl::scene` — `crcbl-scene` behind the non-default `scene` feature, so an
  application can reach the bake its own meshes need.** A
  `render::scene::Geometry::Flat` carries a `MeshClusters` and a `Geometry::Dag`
  a cooked `ClusterDag`, and `crcbl-render` can build neither: §3.5 makes the
  cluster build a bake step precisely so the renderer never depends on `gltf`.
  With the feature on, an application calls `build_meshlets` and the new
  `MeshletBuild::into_clusters()`, or `build_cluster_dag` and
  `ClusterDag::cook`, and hands the result to `ForwardRenderer::with_scene`.

  **Off by default**, on `crcbl-sprite`'s `load`/`bake` terms: a game shipping
  cooked meshes links no glTF parser, and neither does a browser build —
  `cargo tree -p crcbl -e normal` finds no `gltf` until `--features scene`.

- **`MeshletBuild::into_clusters()` → `crcbl_shaders::meshlet::MeshClusters`.**
  A rename of the three arrays the builder already produces, and the only way an
  application had of turning `build_meshlets` output into what a resident mesh
  takes. `ClusterDag::cook` goes through it too, so there is one spelling of the
  mapping, and `crcbl-shaders`' `cook-clusters` example calls `ClusterDag::cook`
  rather than keeping a second copy of it.

- **`ForwardRenderer::add_instance` / `set_instance` / `remove_instance`:
  instances are a runtime API, so an application can put its own objects in the
  scene.** `scene::InstanceDesc` is `{ mesh, material, transform }`, and both
  indices are positions in the `SceneDesc` the renderer was built from — not
  mesh table ids, which a DAG occupies one of per level and which a caller has
  no way to know. `add_instance` returns an `InstanceHandle` or
  `InstancePoolError::PoolFull`; a stale handle rewrites and removes nothing.

  The renderer resolves any description's mesh and material indices now rather
  than holding seven ids read out of `scene::demo`'s positions. The five demo
  setters survived one slice as wrappers over these calls and are gone — see
  Breaking, above.

  Two things a caller placing objects needs from the docs, both on
  `add_instance`: an instance's **array index is the LOD hysteresis key**
  (`draw_gen.slang` reads `instance_index * group_stride`), so a slot freed by
  `remove_instance` hands the next object the previous occupant's expanded-group
  history for one frame; and a `Geometry::Dag` mesh is not drawable on a device
  with a mesh stage and no amplification stage, which is what
  `ForwardRenderer::selects_levels` answers.

- **`crcbl_render::scene` and `ForwardRenderer::with_scene`: the resident set is
  a description now, not something the renderer uploads to itself.** `SceneDesc`
  — `meshes` + `materials` + `page` + `capacities` — is host-side data with no
  device in it, so it can be built and compared with no GPU in the room.
  `MeshDesc::geometry` is `Geometry::Flat` (vertex bytes, indices, cooked
  clusters) or `Geometry::Dag` (a `ClusterDag` plus a vertex array per level);
  `PageDesc` owns layer 0 so the white texel a material naming no texture
  samples cannot be got wrong; `Capacities` is what the `POOL_*` constants were,
  with `Default` at the numbers the engine shipped.

  `ForwardRenderer::new` keeps its exact signature and is
  `with_scene(&scene::demo())`, so **every existing caller is untouched and no
  golden moved** — the demo scene is a caller of the API rather than a special
  case inside the renderer. `scene::demo` is the cube, the pyramid, the open box
  and the dunes DAG with the three material rows and two page layers the golden
  suite reads.

  A description's **order is load-bearing**, and the module docs say so at the
  four places it decides a frame: material row 0 is what an instance written
  without a material id shades through, mesh table ids come from upload order,
  page layer 0 has to decode to `1.0`, and the bucket table is one bucket per
  description mesh — built by walking that list, so `draw_gen.slang`'s
  first-match scatter cannot be given two buckets naming one mesh. A description
  that cannot be made resident is refused as `HalError::InvalidDescriptor`
  **before the first device object exists**, so a rejection leaks nothing —
  including one that outgrows the pools it sized: a description needing more
  vertices, indices, mesh table entries or material rows than `Capacities`
  reserves is refused up front, naming the pool, the capacity and the need,
  rather than part way through filling one. `Capacities::instances` is the one
  exception, because objects are placed while the renderer runs: filling it is
  `InstancePoolError::PoolFull` from `add_instance`.

  `Geometry::Dag`'s documented limitation — `crcbl_scene::simplify` is
  position-only, so a coarse level's attributes are the caller's to supply — is
  in `docs/backlog.md`.

- **`crcbl screenshot --scene` reaches every scene the engine draws.**
  `crcbl::screenshot::Scene` has had nine variants for a while and the CLI
  parsed three of them, so `dunes`, `lights`, `spot`, `spot_shadow`,
  `point_shadow` and `ao` — every 3D lighting scene the render e2e blesses a
  golden for — could not be rendered by hand at all. Each name is now the
  **golden's file stem**, so a frame taken at any size and the 256×192 one CI
  compares are reachable by the same word. An unknown name is still exit 2
  rather than a silent fall back to the cube, and `--help` now lists them; a
  test asserts the help text names every scene the parser accepts, and
  `scene_name`'s match is exhaustive so a new variant stops the crate compiling
  until it is named.

- **Screen-space ambient occlusion**, the rasterised twin's AO row. A depth
  prepass — driven by the existing depth-only pipeline with the camera's own
  bind group and draws, so no new pipeline or shader — feeds an `ssao` pass that
  reconstructs normals from depth and takes eight hemisphere samples, a `4x4`
  blur weighted by view-space depth, and a texel fetch in the forward shader
  that multiplies `frame.ambient.rgb` **alone**. Darkening the tonemap's input
  instead would have darkened direct light and highlights, which is what the
  plan's one-line row invited and what it now refuses in writing.

  **The rotation comes from a sixteen-entry constant table indexed by
  `pixel.xy & 3`, never a float hash, and the blur is not optional.** Each AO
  sample is a binary depth comparison, so one landing on the threshold resolves
  differently on two drivers and swings that pixel by an eighth — far past the
  golden tolerance. Noise functions amplify float differences by construction;
  an integer index into a constant array is bit-identical by inspection, and the
  blur's footprint is exactly the noise tile.

  **The blur weights each tap by how far its view-space depth is from the
  centre's**, because a box kernel averages a foreground pixel's occlusion with
  a background that is not the same surface — and the far plane is written
  "fully unoccluded", so every silhouette carried a bright fringe one kernel
  deep. It unprojects through the same `SsaoParams` buffer the occlusion pass
  writes, so there is no second uniform block and no new knob: the tolerance is
  derived from the AO radius, half weight at one radius and none at two. The
  weight is a linear ramp rather than a threshold, since a binary test on the
  output pixel is the same driver-disagreement hazard the constant rotation
  table exists to avoid. The consequence to know about is that the sixteen-tap
  divisor is now sixteen only where every tap counts — full strength on a flat
  surface, weaker exactly at a silhouette.

  **The check is a structural ratio, not the golden**: a band inside a concave
  corner against a band on the same surface at the same camera distance, because
  an AO pass writing a constant 1.0 draws a perfectly plausible frame. The blur
  has one of its own — the plain pyramid's underside in the cube frame, whose
  pixels are the ambient term alone, must not brighten along the edge the clear
  stands behind. AO is always on, and its off-switch is a 1×1 white texture
  rather than a shader permutation. `ao`, `cube`, `lights`, `dunes`,
  `spot_shadow` and `point_shadow` were re-blessed; `spot`, `sprite` and `ui`
  are unchanged to the pixel, and `spot` staying so is what says the term is
  contact occlusion rather than a global scale.

- **Flappy and breakout can be paused with a finger, and a second finger can
  work a menu while the first holds a control.** Pause is the loop's rather than
  a game action and its menu is the only tappable route to fullscreen and the
  debug panel, so a phone could previously start a run and never stop it.
  `crcbl::engine::PauseControl` is shared across the three demos with touch —
  size, corner, palette, appear-condition and hit-test are one piece of
  knowledge, and it owns the extent so a sample needs no pixel conversion.

  The lockout fix landed in the menu's hit-testing: contacts are a second device
  driving the same widgets, the way the activate key already is. The contact
  carrying the emulated pointer is skipped, without which a one-finger tap fires
  twice. Contacts are now delivered before the pointer, because a sample cannot
  say "that pointer press belonged to my control" until it has heard about the
  finger — and the finger pressing pause _is_ the emulated pointer, so without
  that it flapped in flappy and served in breakout on the way to pausing.

  A control the panel took away also re-grabs on its next move now, instead of
  needing the thumb lifted and landed again.

- **Horde plays on a phone: a floating stick and a PAUSE button.**
  `crcbl_ui::touch`'s `TouchStick` and `TouchButton` are widgets acting as a
  virtual device, and `Binding::Virtual` is how `ActionMap` binds them — the
  same table row a key sits in, so horde's `move` action is one `Axis2` bound to
  WASD, the arrows and `Virtual("stick_move")` together. A stick's deflection
  lands in the same accumulator the `Wasd` composite uses, so a key and a thumb
  sum inside the unit disc rather than to twice the speed.

  The stick appears where the thumb lands rather than at a fixed corner, because
  every fixed position is wrong for some grip and a floating origin reads
  exactly zero on the frame the finger arrives. A second finger on a held
  control is refused and offered to the next one. Controls appear once a contact
  has arrived — not on `ShellCaps::TOUCH`, which a desktop touchscreen also sets
  — so a desktop player sees nothing change.

  Pause came with it because pause is the loop's rather than a game action, so a
  phone could otherwise start a run and never stop it, and the pause menu is the
  only tappable route to fullscreen and the debug panel. `HostedGame` gained
  `take_pending_pause`.

- **The seam carries multiple contacts.**
  `ShellEvent::Touch { contact, phase, position }` with `ContactId` and
  `TouchPhase`, routed to a new `HostedGame::touch_event`. The web shell stops
  throwing away every contact but the first — there is somewhere for them to go
  now.

  **A touchscreen produces both streams**: every contact as `Touch`, and the
  primary contact additionally as the emulated pointer, which is the browser's
  own compatibility rule and is now an obligation on any backend setting
  `ShellCaps::TOUCH`. A game bound only to `Binding::MouseButton` therefore sees
  exactly what it saw before. A contact id is unique among contacts that are
  down together and reused after one ends, so state keyed on one must be dropped
  when it ends. `Cancelled` is not `Ended`: the system took the gesture, so the
  position is the last one the platform knew rather than a place anyone chose,
  and a consumer undoes rather than commits.

  No desktop backend implements touch and none claims to — `caps.rs` names the
  path each would have to write and says the bit is clear because the code is
  not written. `Pending` is no longer `Copy`.

- **Flappy and breakout play on a touchscreen.** Flappy taps to flap; breakout's
  paddle follows a finger and a tap serves. `Binding::PointerPosition { axis }`
  is new and feeds an `Axis1` normalised to the surface at −1…+1 — not an
  `Axis2`, which would put a _place_ in the same value shape as
  `Binding::Wasd`'s _direction_. Within one action an absolute binding replaces
  the relative ones rather than summing, because a place plus a rate is neither.

  **`HostedGame::pointer_event` is new, and it is why none of this worked
  before.** Touch reached the shell, but the loop swallowed every pointer event
  — `Pending::observe` returned `Handled::Loop` for `ShellEvent::Button` and a
  game could only be handed keys, so no pointer binding could ever have fired.

  The pointer wins on the tick it moves and the keyboard owns every other tick,
  so arrow keys still work with a cursor over the field; a pointer that leaves
  keeps its last position, which is what stops the paddle walking to the middle
  on every tap. The canvas takes `touch-action: none`, without which the browser
  claims the gesture mid-drag.

  **Horde stays keyboard-only** — a movement stick needs on-screen controls and
  real multi-touch — and asteroids is excluded on purpose: three concurrent
  controls have no phone layout better than the keyboard one.

- **The culling stats come back off the GPU, so the culling win is visible.** A
  ring of `HostReadback` buffers, one per frame in flight plus one, fed by a
  copy the render graph schedules and resolved only when a slot comes back round
  — the shape `PassTimers` already uses, and for the same reason: the latency
  _is_ the synchronisation, so there is no fence, no `wait_idle` and no poll
  loop. `instances drawn` and `clusters drawn` are numbers now instead of
  `indirect`, and a new `cull frame` row says which frame they came from.

  **`RenderGraph::add_copy_pass` and `PassKind::Copy` are new**, and were
  unavoidable: the seam allows a copy only outside a pass scope, and every
  existing pass kind opens one. So a copy could not be a convention about what a
  compute body may do — it had to be a kind whose body runs with no scope open.
  `GraphError::AttachmentInComputePass` became
  `AttachmentOutsideRenderPass { kind, .. }` to cover both.

  Only the camera's cull is read. A cascade's survivors answer a different
  question about a different frustum, and summing them would produce a number
  larger than the instance count. A device that refuses the readback reports
  nothing rather than zero, and the cluster word — written by the amplification
  stage, so absent on three of the four ways the engine draws — reads `unknown`
  rather than `0` where nothing counted it.

- **One place a frame's draws and instances are counted.**
  `crcbl_render::FrameCounters`: each renderer answers with its own record and a
  caller sums them, the same shape the timed-pass bound already uses. The debug
  panel gains a `counters` section, and the numbers are sampled onto the trace
  as `crcbl_core::trace` counters — which nothing had done until now.

  **Two of the plan's counters are deliberately absent rather than
  approximated.** Instances drawn and triangles read `indirect` wherever a
  `ForwardRenderer` is in the frame, because the culling survivor count lives in
  a device-local buffer that nothing copies back: the readback the plan listed
  as already existing does not exist. A triangle count derived from a cluster
  count and a nominal triangles-per-cluster would look authoritative and be
  wrong. Clusters drawn and the level histogram are absent for the same reason,
  plus one more — that word is written by the amplification stage, so it is
  blank on three of the four ways the engine draws.

  `GameGpu::counters` has no default implementation: a bundle that forgot it
  would otherwise put `draws: 0` on the panel, which is "not counted" arriving
  as "nothing drawn".

- **The frame loop is instrumented, and the debug panel answers "am I
  GPU-bound?"** Six spans across `Loop::frame` — `frame` around `input`, `pace`,
  `tick`, `draw` and `present`, with `present-wait` nested inside the last — and
  a `budget` section showing CPU and GPU frame time as p50/p95 over a rolling
  120-frame window with which of the two is the budget. `CRCBL_TRACE` turns the
  profiler on the way `CRCBL_LOG` turns on logging, so it needs no rebuild.

  **CPU frame time is the frame span less the spans the loop spent blocked** —
  `pace` and `present-wait`. Including them would make the row read as the
  display's period on every machine under vsync, exceed the GPU total whatever
  the GPU was doing, and answer "CPU-bound" to a question it never looked at.

  The two halves are distributions over their own windows, not a pair: the GPU
  report is frames latent by design and nothing here stalls to "fix" that, so
  the row carries the frame number its newest GPU sample came from. Percentiles
  are refused below 20 samples, because nearest-rank p95 is just the maximum
  under that, and the section is absent until it has one — a run with the
  profiler off gets no row of dashes.

- **`crcbl_core::trace`: CPU spans and counters**, topic 40's span API. A scoped
  span with a static name, opened and closed by RAII and nesting freely; a
  counter is its sibling, a named `u64` sampled at the depth it was taken from.
  `drain()` is the frame boundary and hands back a snapshot per thread.

  **Always compiled and gated at runtime**, because a profiler you must rebuild
  to use is one nobody turns on mid-investigation. Disabled it costs one relaxed
  load, a test and a tail jump — read out of the release assembly rather than
  asserted — and the gate starts off.

  Records are a flat begin/end stream per thread rather than a tree, which is
  what Chrome Trace, a p50/p95 scan and per-thread tracks all want; each record
  carries the depth it sat at, so nesting is read rather than walked. A thread's
  buffer is fixed and **refuses rather than grows or evicts** — evicting the
  oldest record would take out the frame's own begin — and every refusal is
  counted and reported. Threads get a small numbered track with their name
  attached, since a Chrome Trace `tid` and a panel row both need a number and
  `ThreadId` has none.

  Nothing is instrumented yet: this slice is the mechanism, and the frame loop,
  the debug row and the trace export are the ones after it.

- **Point lights cast shadows too, through six atlas tiles rather than a cube
  map.** The grid is 4×2 now: two cascades and a six-tile light region. Faces
  are the cube-map order — `+X -X +Y -Y +Z -Z` — built by `shadow::face_axis` on
  the host and picked by `mesh.slang`'s `point_face` from the largest component
  of the offset to the light.

  **One cull per point light, not one per face**, which is the decision recorded
  in `docs/plan/18-render-features.md`: the six faces' union is the light's
  sphere and that is what the cull tests anyway, so one visible set feeds all
  six draws and a face discards what is behind it. The alternative would have
  been thirty megabytes of `DrawGen` for one light.

  That splits one number into three. `SHADOW_LIGHT_TILES` is atlas space,
  `shadow::LIGHT_SLOTS` is cull space, and the view count is the product — so
  the reachable states are **one point light or two spots**, and a light that
  fits neither budget still lights and simply does not occlude. A point light
  that cannot fit six consecutive tiles is skipped without taking the budget
  down with it, so a smaller light ranked behind it is still shadowed.

  `Scene::PointShadow` is the new golden: two casters standing on opposite sides
  of the light, so a frame that shadows one direction and not the other — the
  shape a face-indexing bug takes — fails rather than looking plausible.

- **Spot lights cast shadows.** The shadow atlas became a fixed grid of
  1024-texel tiles: the sun's cascades keep the first ones and the rest are
  handed out one per shadowed spot, which is `docs/plan/18-render-features.md`'s
  recorded decision. `shadow::Selection` ranks eligible lights by projected
  screen influence — radius over distance, the metric family LOD already uses —
  breaks ties by index, and holds an incumbent's tile until a challenger beats
  it by a quarter, so a shadow does not blink in and out as the camera drifts.

  **A light that gets no tile still lights and simply does not occlude.** A cone
  at or past 80° has no projection to build, so it is refused a tile by name and
  keeps lighting, as does every spot past the budget. `GpuLight` gained
  `shadow_tile`, spent out of the first padding word, so the row costs no more
  bytes than before; `NO_SHADOW_TILE` is `u32::MAX` rather than zero, because
  zero is a real tile, and `GpuLight::default()` is hand-written for that
  reason.

  A spot's map is a **perspective** projection down the cone, reversed-Z like
  everything else here, its field of view twice the outer half-angle so the map
  covers the cone exactly. It gets no texel snap and needs none: the cascades
  snap because their box follows the camera, and a spot's matrix is a pure
  function of the light. It biases in world units before projecting, in tile
  texels at the receiver, because a perspective map's depth precision is
  distributed nothing like a cascade's and the sun's constants do not transfer.

  `FrameUniforms` gained `light_view_proj`, appended after `cluster_grid` so no
  existing member moved and every cascade golden stayed byte-identical.
  `Light::row` now takes the slot. `Scene::SpotShadow` is the new golden: a
  pyramid between the light and the floor, asserting the floor is dark behind
  the caster and lit across the pool from it, and that removing the caster
  lights what it darkened.

- **Spot lights are drawn and their cone is asserted by pixels.** `Scene::Spot`
  is a floor lit from directly overhead, so cone axis, surface normal and view
  direction are all one axis and brightness is a function of distance from the
  frame's centre alone. Four luminance profiles out from the centre assert a lit
  floor, a core at least three times brighter, the axis as the maximum, and **at
  least twelve samples strictly inside the penumbra band** — the check that
  separates a ramp from a boolean.

  That last one earns its place: swapping the inner and outer angles produces a
  frame with the **same 697 at the axis and the same 106 at the edge** as a
  correct one, and every other assertion passes on it. Only the penumbra count
  moves, to zero.

  The froxel bound for a spot is a cone as well as a sphere now, each rejection
  slackened by the froxel's own bounding radius so it can only ever add froxels.
  One narrow spot goes from **144 froxels to 91**, a 37 % drop, with every
  golden bit-identical on radv, lavapipe and wgpu. Dropping the slack makes it
  too tight, and the spot scene catches that as a tile-shaped bite out of the
  pool — which is exactly the seam a too-tight cull produces and the reason the
  scene exists.

- **Many lights, gathered by a clustered-forward pass.** `crcbl_render::Light`
  with `PointLight` and `SpotLight`, an SSBO of rows the way instances and
  materials already are, and `light_cluster.slang` assigning them to a froxel
  grid — screen tiles by depth slices — that the fragment stage indexes by its
  own position. `Scene::Lights` is the new golden.

  **The sun is a row too**, flagged as reaching every froxel, so it stops being
  a special case in the shader — and the proof that the conversion is faithful
  is that **every existing golden is bit-identical**, measured byte-for-byte
  before and after rather than trusted to the comparator, which is
  tolerance-and-SSIM based and would have absorbed real drift.

  Depth slices are **exponential** (Olsson–Assarsson) because a uniform split
  over 0.1–1000 m would give a first slice 41 m deep holding every light. The
  slice index comes from **linear view depth**, not `SV_Position.z`: under this
  engine's reversed-Z that value runs backwards _and_ hyperbolically, so a
  uniform step in it would put one slice covering 2.4 m to infinity.
  `1/SV_Position.w` was avoided too — a reciprocal on some targets and not
  others, which is the class of cross-target disagreement `mesh.slang`'s header
  records being burned by twice. An orthographic camera has no view depth at all
  and runs on one slice through its own branch.

  Assignment is conservative by construction: a froxel is the convex hull of its
  eight corners so their AABB contains it exactly, the falloff window is exactly
  zero at the light's radius so cull and shading are the same statement, and a
  spot is bounded by its sphere rather than its cone — loose in the safe
  direction.

  **Cluster overflow is counted rather than dropped silently**, riding the
  existing delayed-readback counter: 21 lights over 288 froxels against a budget
  of 16 refuses 1440 assignments, and the zero case is asserted first so the
  counter is not wired to a constant.

- **`crcbl lod stats` and `crcbl lod gen`** — topic 25's tooling row, host-only.
  `stats` resolves every mesh the file draws and reports, per level, **where the
  geometry came from** (the file's own node and which convention declared it, or
  the DAG depth that generated it) with triangle and cluster counts and the
  group error range, then the shape of each DAG behind it. `gen` writes the
  cooked `.dag` artifact and decodes it back before reporting success.

  **Stalls are named rather than averaged away.** A level that kept more than
  three quarters of the level below it is reported `— STALLED`, and the real
  dunes patch trips it: levels 4 through 6 go 568 → 412 → 324 triangles with the
  error unchanged throughout. A report that smoothed that over would be hiding
  the one thing worth looking for.

  A hand-authored level below LOD0 reports no cluster count and no error, on
  purpose — it was never clustered or decimated here, so there is no engine
  number and printing one would invent it. LOD0 is the exception, being both the
  file's own geometry and DAG level 0.

  A refusal is an error rather than a row: an unimportable file, a level two
  nodes claim, an `MSFT_lod` id that draws nothing, or a gap the generator
  cannot reach all exit non-zero with no table. `--json` carries the same facts
  for the benchmark and editor consumers topic 40 anticipates.

  **`preview` is recognised and refused as unimplemented**, not absent — and the
  reason is bigger than it looks: `crcbl::screenshot::Scene` is a closed enum of
  three built-in scenes, so nothing anywhere can render arbitrary imported
  geometry offscreen. That scene has to exist before a preview can.

- **Shadow LOD bias: the shadow pass selects coarser casters than the camera.**
  `SHADOW_LOD_BIAS` multiplies both selection budgets for the whole pass. On the
  dunes patch at the shipped camera that is 57 clusters at `[13, 26, 18, …]` for
  the camera against 48 at `[5, 17, 24, 2, …]` for the shadow, with 7 of the 30
  groups the camera expanded staying collapsed.

  **The cascades were selecting from the wrong eye, and that is fixed.** They
  used `camera.eye + light_direction * cascade_far` — which is not the light's
  position, since a directional sun has none, but the camera's own eye pushed
  along the sun's direction, and it stepped per cascade so two cascades asked
  two different detail questions about one caster. They now select from the
  camera's eye at the camera's pixels-per-unit, because what a coarser caster
  costs is a shadow edge displaced by the group's error, and that displacement
  is seen by the camera at the camera's distance. The light remains the eye for
  the amplification stage's normal-cone test, where a shadow map's viewer
  genuinely is the light — two consumers that had been sharing one value now
  each get the one they need.

  A budget multiplier rather than "+N levels", because the descent has no level
  parameter and level-to-level error ratios are a property of the mesh: on this
  DAG level 0→1 steps about 2.4x, level 2→3 about 8.8x, and the top three levels
  share one error. Monotonicity survives because it is one positive constant
  over the whole pass, and a subset property falls out — the shadow cut is never
  finer than the camera's anywhere, which is what the test asserts.

  Per-cascade selection rings are new, because the colour pass is recorded last
  and overwrote the single selection buffer, so the shadow pass's descent had no
  observable at all and the bias would have been unmeasurable.

- **Hand-authored LOD levels are imported and win over generated ones.**
  `crcbl_scene::resolve_lod(scene, node)` resolves a mesh's chain and reports,
  per level, **where it came from** — `LodOrigin::Hand { node, mesh, via }`
  naming the glTF node and whether node naming, `MSFT_lod`, or both declared it,
  or `Generated { dag_level }` naming the DAG depth. LOD0 is always the file's
  own geometry. Gaps are filled by the generator and nothing else is; a fully
  hand-authored chain never runs it at all, observable as an empty `dags()`.

  **No silent substitution**, as the plan requires: a level two nodes claim, an
  `MSFT_lod` id that draws nothing, a node named like a level that draws
  nothing, and a gap the generator cannot reach are each a named error rather
  than a quiet stand-in.

  **Hand levels never enter the DAG**, structurally rather than by convention: a
  hand level is a mesh index into the file and a generated one is a depth into
  `dags()`, so there is no array where the distinction could be lost. A mesh
  with both is therefore selected **per instance** — an artist supplies
  whole-mesh geometry, not a crack-free cluster hierarchy, and a per-cluster cut
  across the two would crack.

  `MSFT_lod` needed `gltf`'s `extensions` feature, which costs nothing: both
  that crate's and `gltf-json`'s feature lists are empty, the `serde_json`
  behind the raw extension map is already non-optional in each, and `Cargo.lock`
  is unchanged. `MSFT_lod` on _materials_ is deliberately not read.

- **LOD hysteresis, so a camera drifting across a threshold stops flickering.**
  A group starts expanding above the budget and keeps expanding until its
  projected error falls to `LOD_HOLD_RATIO` of it. Measured on a
  boundary-straddling drift: **39 level changes over 40 host frames with one
  threshold, 0 with two**; on a real GPU, `[0, 1, 0, 1, …]` becomes `[0, 0, …]`.
  A decisive move still switches.

  **Per-group history, and that is a soundness requirement rather than a
  saving.** A cut is a cover only while expansion is monotone up the DAG, and a
  remembered answer can otherwise leave a child collapsed under an expanded
  parent — a hole. The two-threshold rule is monotone whenever the plain rule
  is, because a parent's error is at least its children's and its sphere
  contains theirs, so starting from all-zero every later frame is monotone by
  induction. Per-cluster history would have been 16.6 MB _and_ wrong; per group
  is 3.87 MB at the pool's instance capacity.

  The state is **one buffer, deliberately not a ring**: an instance the frustum
  rejected writes nothing, so its slot in a fresh ring holds a value from frames
  ago that is not its own history and need not be monotone. Ordering comes from
  the graph — the draw-argument pass declares it `ShaderReadWrite` and every
  mesh pass `ShaderRead`, so each frame's first barrier carries a real source
  scope over the previous frame's writes and reads.

  It also shrank `ClusterSelect` from 48 bytes to 16: a record now names two
  group _indices_ rather than carrying two copies of a group's error and sphere,
  so every cluster of a group reads the same word instead of bit-identical
  copies. Shadow cascades keep their own state, since sharing the camera's would
  have two eyes undoing each other's band every frame.

### Added

- **`Capability::BindlessDescriptorArray` works on the Metal backend**, taking
  the parity blocker list from ten rows to nine. `crcbl_mtl::binding` binds a
  descriptor array as a Metal argument buffer — a table of
  `MTLBuffer::gpuAddress` values written directly, with `useResource` residency
  — instead of refusing `BindingFlags` outright. It is reported only where the
  device answers argument-buffer `Tier2` and `Metal3`; a lesser Mac now answers
  "not on this device" rather than the backend answering "no".

  `bindless_probe.slang` changed shape to make this possible: its array is a
  **bounded** `ParameterBlock`, which is the only form Slang lowers to something
  Metal accepts, and it now ships an MSL artifact. That moves the array to its
  own descriptor set, so a bind group for it is two groups rather than one.

- **`crcbl_render::grid` draws an infinite reference grid as a screen-space
  pass.** `grid.slang` reconstructs the ground plane per fragment — view ray
  against `y = 0` — and derives line coverage from screen-space derivatives, so
  a line is a constant width in pixels at any zoom and needs no density LOD. Two
  scales, a distance fade, premultiplied output, and `SV_Depth` written from the
  hit so scene geometry occludes it. This is the technique Blender's overlay
  grid, Godot 4's and Unity's all use; a grid drawn as geometry cannot hold a
  constant screen width.

  It is wired into `ForwardRenderer` behind `set_ground_grid`, **off by
  default**, and drawn **after the tonemap** rather than with the scene: a
  reference grid has to look the same at any exposure, so it must not be
  tonemapped like scene content. It still depth-tests against the scene depth,
  so geometry occludes it. `apps/viewer` turns it on, which completes
  `docs/plan/sample/05-viewer.md` milestone 1.

  The style is fixed at 1 m cells fading at 100 m, so a document authored at a
  very different scale gets a grid that is one line or entirely faded; deriving
  it from the model's bounds is recorded in `docs/backlog.md`.

- **`CommandEncoder::fill_buffer` works on the D3D12 backend, for a value of
  zero.** `Capability::BufferFillZero` now reports supported on dx12, which
  takes the parity blocker list from eleven rows to ten. It is implemented the
  way `wgpu-hal`'s dx12 backend does it — one small zeroed device-local resource
  created with the device, then `CopyBufferRegion` over the destination range —
  rather than through `ClearUnorderedAccessViewUint`, which would need a UAV of
  every fillable buffer and so either `ALLOW_UNORDERED_ACCESS` on every
  allocation or a fill that works only on `STORAGE` buffers.

  **A non-zero fill is still refused**, now with a message naming the capability
  and why, rather than as an unimplemented slice.
  `Capability::BufferFillRepeatedByte` and `Capability::BufferFillWord` remain
  declined on dx12; their recorded reason was wrong — the crate does build
  shader-visible descriptor heaps — and now states the real obstacle.

### Fixed

- **A texture whose image comes from an extension is skipped, not refused.**
  `source` is not an `Option` in `gltf-json`'s model — it carries a `serde`
  default of `u32::MAX` — so a texture supplying its image through
  `EXT_texture_webp` or `KHR_texture_basisu` was refused with
  `texture 0 names image 4294967295`, a sentinel this crate invented reported as
  though the document had written it. The material now loses its texture and
  keeps its base colour, which is where an undecodable image already landed, and
  a warning names the count. **A source that is genuinely out of range is still
  refused** — that is a document naming an image it does not have.

- **A glTF that needs an extension this importer lacks now says so.** Every
  entry in `extensionsRequired` and `extensionsUsed` that `crcbl-scene` does not
  implement is named in a warning against the document's key — required loudly
  ("it is drawn without them, so what is on screen is not what the file
  describes") and optional quietly. `MSFT_lod`, the one extension the importer
  does implement, is excluded.

  **Eighteen of the 116 Khronos sample models that load declare a required
  extension**, and every one of them was previously drawn without it in silence
  — a `KHR_materials_sheen` sofa rendered with no sheen and the only clue was
  that the picture looked wrong. This is `docs/plan/sample/05-viewer.md`'s
  second exit criterion: file, feature, reason.

  **Reported rather than refused**, which is a deliberate reading of a SHOULD
  NOT in the specification: a viewer exists to open the file somebody is
  holding, and refusing outright tells them less about their asset than drawing
  it and naming what is missing. `docs/backlog.md` carries the trade, because
  the honest answer may yet be a flag.

- **A glTF is no longer refused over an animation the importer already
  discards.** `gltf_json::animation::Target` makes `node` a required field and
  `KHR_animation_pointer` replaces it with a pointer, so `serde` failed on the
  whole `Root` and the document was rejected outright — while
  `gltf_import::report_unsupported` skips every animation in every document and
  logs a line saying so. A file was being refused over a feature the importer
  had decided to ignore.

  `parse` now retries once with the `animations` array removed, and reports the
  **first** error if that fails too, so a document broken for another reason
  still fails on its own terms rather than on an altered copy. Only `animations`
  is dropped, and only because nothing reads it; no other array has that
  property, so no other array is touched.

  Measured on the Khronos `glTF-Sample-Assets` suite: of the 118 models that
  ship a `glTF-Binary` variant, **111 loaded before and 116 do now** — 94.1% to
  98.3%. `AnimatedColorsCube` is the one that makes it a defect rather than a
  limitation: it lists nothing in `extensionsRequired`, so the specification
  says it has to load.

- **`crcbl-wgpu` accepted a pipeline layout with too many bind groups and handed
  back a poisoned one.** It checked the push-constant range but had no
  `max_bind_groups` guard, so an over-count reached `wgpu`, which files a
  validation error and **still returns an object** — the caller got `Ok` and a
  layout that could not be used. It now refuses with
  `HalError::InvalidDescriptor` like every other backend.

- **A `NaN` vertex moved a meshlet cluster's bounding sphere off its geometry.**
  `crcbl_scene::meshlet::cluster_bounds` folded the cluster's centre with glam's
  `Vec3::min`/`Vec3::max`, which discard the accumulator on a `NaN` — while the
  radius beside it folds with `f32::max`, which skips one honestly. The result
  was a plausible radius around a displaced centre, and that sphere is what
  `crcbl_render::cull`'s cluster test uses, so a cluster could be culled while
  on screen. `crcbl_scene::cluster_dag::enclosing` folded group spheres the same
  way and lost the order-independence its docs claim. Both now go through a new
  crate-private `bounds` module.

  Nothing validates a `POSITION` accessor for finiteness — `gltf_check` never
  reads a float's value and `build_meshlets` lists only a partial triangle and
  an out-of-range index — so an imported document reaches this directly.

- **`Aabb::from_points` could return a finite box that did not contain its own
  points.** The fold used `Vec3::min`/`Vec3::max`, which glam writes as a bare
  `if self.x < rhs.x { self.x } else { rhs.x }`. Every comparison against `NaN`
  is false, so those return the _incoming_ point and throw the accumulator away:
  one `NaN` vertex followed by one finite vertex left a box that looked healthy
  and no longer enclosed the geometry, and a box that does not contain its
  points culls what is on screen — the direction `crcbl_render::cull` documents
  a cull must never err in. It now folds through `f32::min`/`f32::max` per lane,
  which skip the unusable operand, so an odd `NaN` coordinate is ignored and
  every finite point stays enclosed. A lane with no finite value at all is still
  `NaN`, so a wholly degenerate mesh remains visible as one, and infinities are
  unaffected.

  **`apps/viewer` gained a vertex scan because of it.** Its `NonFiniteGeometry`
  check tested the resulting box, which can no longer observe a `NaN` at all;
  positions are now tested before the fold. The prior behaviour was recorded —
  in a doc comment, a fixture and the backlog — as `f32::min`'s
  absorb-everywhere rule, the opposite of what the dependency does, and the
  fixture had been built around the wrong half deliberately.

- **lantern's free-fly camera turned the wrong way.** The left arrow turned the
  view right and the right arrow turned it left. Yaw is measured from `-Z` and
  rises toward `+X`, which is the right-hand side — so
  `axis(turn_left, turn_right)`, whose positive argument is the _left_ key,
  added to yaw for the left arrow. The arguments are now the other way round,
  matching the strafe two lines below, which had the same shape and the correct
  order. The turn axis now has a sign test of its own; the pitch axis beside it
  always had one, which is how yaw came to be the untested half.

- **A lost GPU device was reported as whatever call happened to notice it.** A
  device dying mid-frame surfaced as
  `readback N could not be mapped: AbortError: … is lost` — a sentence naming a
  readback that was fine, with the actual cause buried in the browser's tail
  text — and every later call added its own downstream error. `gpu-replay.js`
  now watches `GPUDevice.lost`, records the reason and message the promise
  carries, and reports `the device was lost: <reason>: <message>` **once**, as
  the first thing a reader sees. Every command afterwards is answered with that
  same sentence rather than a symptom of its own, and readbacks in flight — plus
  any a rejection has already claimed — are failed with it, so no raw
  `AbortError` reaches the caller.

  A device destroyed on purpose is not reported as a failure: `'destroyed'` is
  reachable only through `GPUDevice.destroy()`, which this engine never calls,
  so it is recorded as terminal without putting an error on the queue and an
  ordinary page close stays silent.

- **`crcbl-mtl` refused timestamp and pipeline-statistics query sets with a
  reason that was untrue on any Mac but the CI one.** The message said an
  `MTLCounterSampleBuffer`'s descriptor must name one of
  `MTLDevice::counterSets` "and the device this backend's CI runs on advertises
  none, so neither pool can be built there" — a device-specific claim on a
  refusal that never asks the device anything: `create_query_set` matches on the
  query kind alone, so an Apple-silicon Mac advertising counter sets is refused
  identically and told about a machine it is not. The operative reason is the
  one `adapter::features_of` already gives for withholding
  `Features::TIMESTAMP_QUERY` and `Features::PIPELINE_STATISTICS_QUERY`:
  reporting either obliges a `Limits::timestamp_period_ns`, and Metal correlates
  the GPU clock to the host's at sample time rather than ticking at a fixed
  period, so there is no honest period to report. The message now leads with
  that, keeps the CI device's zero counter sets as the second obstacle it is,
  and the constant's doc says which of the two each half describes.

- **Every frame the browser backend presented was a whole transfer function too
  dark.** `crcbl-webgpu` answered a canvas surface with the two linear formats a
  `GPUCanvasContext` can be configured with and nothing sRGB, so
  `SurfaceCaps::preferred_format` fell through to a linear one and the swapchain
  was never encoded — every pass above the seam writes display-referred values
  and leaves the encode to the hardware. The old `crcbl-wgpu` browser path did
  this internally; nothing carried it across.

  `surfaceCapsFor` in `web/engine/gpu-replay.js` now leads its format list with
  the sRGB counterpart of `getPreferredCanvasFormat()`, and `CreateSwapchain`
  configures the canvas with the **base** format while naming that counterpart
  in `GPUCanvasConfiguration.viewFormats`; `AcquireNextFrame` creates the
  frame's view in the format the caller asked for rather than defaulting it. The
  base format stays the browser's own preference, so there is still no
  full-canvas copy per present. The offscreen ring is unchanged — it already
  answered sRGB directly — and the eleven-scene golden parity is unaffected.

  Nothing above the HAL seam changed: no engine `ImageViewDesc` reinterprets an
  image's format, so `ImageDesc` needed no `view_formats` field.

- **`apps/hud` opened its device without present feedback or display timing.**
  Its `desc()` spelled out `TIMESTAMP_QUERY | DEBUG_MARKERS` by hand, dropping
  `PRESENT_FEEDBACK` and `PRESENT_TIMING` along with the `GPU_DRIVEN` it meant
  to omit. `GpuContextDesc::default` records what that costs: `acquire` calls
  `wait_until_presented` every frame and a device never asked for the capability
  answers immediately forever, so the closed pacing loop is unreachable, and
  `display_timing` answers `Unknown` forever. hud now takes the engine's bundle
  whole, and carries the same
  `the_features_this_sample_asks_for_are_the_engine_s_own` guard the other
  samples have had since one of them shipped a subset by hand.

- **A warp that moved no pointer reported success on Windows.** `crcbl_shell`'s
  Win32 backend discarded three failures in `warp_to_client` — a stale window, a
  refused `ClientToScreen`, and `SetCursorPos`'s `BOOL` — so
  `Shell::warp_pointer` returned `Ok(())` having done nothing. Windows refuses a
  cursor move from a process that is not in the foreground, which is the common
  case, and the only symptom was a pointer that stayed where it was.
  `warp_pointer` now returns `ShellError::Backend` naming that requirement. The
  two internal courtesy warps — the locked-pointer recentre and the initial
  centring when a lock is taken — log instead, because there the mode is already
  in force and only the move failed.

- **A third off what is left of that strip, by biasing the shadow against the
  triangle the rasteriser drew rather than the normal interpolated across it.**
  `mesh.slang`'s slope term read `tan(acos(N·L))` off the shading normal, so a
  surface shaded with normals its triangles do not have — an analytic height
  field on one-metre quads is the extreme case — asked for less bias than its
  facets needed and self-shadowed in a cross-hatch. The constant term was
  covering that for every scene in the tree. `geometric_normal_of` takes the
  facet from the screen-space derivatives of the world position, `shadow_slope`
  reads the slope off it, and `crcbl_render::shadow::DEPTH_BIAS_TEXELS` comes
  down from 6.0 to 3.0.

  In `apps/lantern`'s room the strip at a wall's foot goes 0.382 m → 0.256, the
  band down the back wall's left edge 0.373 → 0.244, and the cornice under the
  ceiling from 61 luma over the shadowed wall to 21. Both light types read the
  same normal, so a spot's and a point light's maps are biased against their
  receivers' facets too; no punctual golden moved, because the scenes that
  exercise them receive on flat floors. The two goldens that moved are
  `apps/lantern/tests/golden/room.png` and
  `crates/crcbl/tests/golden/dunes.png`.

- **The sun lit a strip along the foot of every wall, a band down the side of
  anything standing against one, and a bright cornice under a ceiling.** In
  `apps/lantern`'s room those measured 0.60 m, 0.58 m and a band three times
  brighter than the surface it sat on — enough to read as a pillar or a window
  reveal that is not in the scene. The sun's shadow bias was denominated in a
  cascade's clip depth, so its world meaning was that number times the cascade's
  whole depth range, `2 · radius` plus `crcbl_render::shadow`'s 40 m caster
  reach: 0.83 m of slack against walls 0.15 m thick, and it grew with any scene
  that needed more caster reach.

  It is now denominated in **texels of the cascade the fragment landed in** and
  applied to the world position before projecting — the same shape and the same
  unit `mesh.slang`'s punctual lights already used. The first two artefacts
  measure 0.375 m and 0.368 m; the cornice thins, by an amount the entry above
  quantifies against a metric that reproduces. A near cascade is now biased
  proportionally less than a far one, where the old denomination had that
  backwards.

  Two goldens moved with it: `apps/lantern/tests/golden/room.png` and
  `crates/crcbl/tests/golden/dunes.png`. Nothing new self-shadows —
  `docs/plan/18-render-features.md` carries the measurements either side, and
  what stops the strip shrinking further.

- **`crcbl-vk` freed a destroyed resource while a command buffer that was
  recorded and not yet submitted still referenced it — a use-after-free the
  driver reads through.** The seam permits record → destroy → submit, and the
  deletion queue kept a destroyed object parked until every submission _naming_
  it completed. A command buffer recorded against the same object and not yet
  submitted was invisible to that: no submission had extended its objects'
  retirement, so an earlier submission completing freed them under it. The
  validation layer reports it at the next submit as
  `VUID-vkQueueSubmit2-commandBuffer-03874` ("recorded but now has become
  invalid"), and lavapipe then reads the freed allocation and segfaults.

  `poll_retire` now refuses to free anything a recorded-but-unsubmitted command
  buffer names, and `submit` marks its command buffers submitted once the driver
  has accepted them. Nothing above the seam changes: an object still frees as
  soon as every recording that names it has been submitted or destroyed and the
  timeline has passed it. `crcbl-dx12` and `crcbl-mtl` never had this — their
  recordings take a COM/ARC reference to what they name.

- **A scaled instance's clusters were culled as if it were unscaled, so geometry
  silently vanished.** `cluster_survives` carried a cluster's mesh-space
  bounding radius into a world-space frustum test, documented as safe because
  `GpuInstance::transform` "is rigid" — a claim already false in two shipped
  scenes, where the true world radius is four to five times the local one. A
  large scaled object offset from the camera therefore lost every cluster and
  drew nothing, on devices with an amplification stage; the instance-level cull
  kept it correctly, which is why nothing upstream noticed.

  The radius is scaled by the square root of the largest absolute row sum of
  `BᵀB` — an upper bound on the basis's largest singular value, exact for any
  rotation-then-scale, and needing no contract about what callers may pass,
  which is what the previous code needed and did not have. It is `1.0` for a
  rigid transform, so nothing previously correct moved and no golden changed.
  The transformed cone axis is normalised for the same reason: unnormalised, the
  same shape at two sizes got two answers.

- **A press made before a panel opened fired that panel's buttons.** `UiState`
  latches while the pointer is down, so a pointer already held when a menu
  appears latched whatever button appeared beneath it and fired it on release —
  rare with a mouse, and the ordinary case on a phone, where the thumb on the
  movement stick _is_ the emulated pointer. Horde asked for fullscreen when that
  thumb came off a pause menu it never touched. A press now belongs to whoever
  was on screen when it landed, and a panel switch drops it.

- **A tap on a menu button did nothing, so no demo could actually be started on
  a phone.** For a touch pointer the browser fires `pointerleave` in the same
  pump as `pointerup`; the web shim reported that as a focus loss, so the
  position the release is hit-tested against was already gone by the time the
  engine looked. The identical click with a mouse worked, which is why it
  shipped. A finger between contacts is not hovering anywhere, so touch no
  longer reports enter and leave at all.

  Two more found with it: `pointercancel` handling was **inert**, because the
  spec gives it `button: -1` and that became a `PointerButton::Other` the engine
  ignores — the release was dropped and the game stayed holding the button,
  exactly the failure the handler existed to prevent. And the coarse-pointer
  copy swap did nothing, because `.key-row { display: flex }` ties on
  specificity with `.touch-only`/`.pointer-only` and won on source order, so
  every desktop saw the keyboard row and every phone saw `Esc`, `F11` and `F3`.

  All three were shipped green and all three were found by teaching the browser
  gate to dispatch touch. It drove `Input.dispatchMouseEvent` only, so the mouse
  path of shared plumbing was covered and nothing touch-specific ever ran.

- **wgpu reported a bindless ceiling no layout could be built at.**
  `max_bindless_descriptors` came straight from wgpu's
  `max_binding_array_elements_per_shader_stage`, which is the count
  `create_bind_group_layout` will not _reject_ — not one it will _accept_. wgpu
  eagerly creates a descriptor pool for 64 sets when a layout is registered, so
  radv's 8,388,606 asked the driver for roughly 537 million descriptors in one
  call and got `OUT_OF_HOST_MEMORY`, out of the very call the `u32::MAX` count
  sentinel resolves through. It is capped at the 500,000 wgpu commits to in
  writing for any device with binding arrays, the same reasoning `crcbl-dx12`
  gives for reporting the tier 2 heap constant on a tier 3 device: `Limits` is
  documented as what the backend _guarantees_.

  The portable bindless declaration therefore failed on every adapter generous
  enough to report a large ceiling, and worked on the software one CI pins —
  which is why the wgpu suite was green in CI and red on real hardware.

- **The samples' profiler HUD was timing the first eight passes of a fourteen-
  pass frame.** Every sample picked its own `MAX_TIMED_PASSES` — a literal that
  has to track how many passes the renderer records, and that nothing made track
  it. `crcbl_render::MAX_TIMED_PASSES` is that number now, summed from a
  `MAX_PASSES` each renderer states about itself, so a pass added anywhere moves
  it instead of seven copies drifting. Sandbox goes from 8 timed rows to all 14.

  The warning `PassTimers` logs when its capacity is short now fires once rather
  than every frame; a caller that sizes its own timers deliberately still gets
  it.

- **The LOD hysteresis state was host-visible and shader-written, which removes
  a D3D12 device.** Upload and readback heaps refuse `ALLOW_UNORDERED_ACCESS` at
  creation, so there is no unordered access view of one, and `crcbl-dx12`
  refused the binding by name. It is `DeviceLocal` now, zeroed by a start-up
  staging copy rather than a host write — **once**, before frame zero, because
  unlike the draw-generation counters this is history and zeroing it per frame
  would delete the hysteresis silently.

  `crcbl-render`'s
  `nothing_the_draw_generation_lets_a_shader_write_is_host_visible` is the
  guard: it builds a real `DrawGen` on the null backend and checks every buffer
  a shader writes. It needs no ICD, so it covers the WARP leg from a Linux box —
  which is where this class has now cost a device twice.

- **The mesh path's cut collapsed to the top level, from a bind range that had
  not grown with its struct.** `ClusterDrawConstants` went 16 to 32 bytes while
  the bind group still named `DRAW_CONSTANTS_SIZE` for that dynamic uniform —
  and a uniform read past a bound range is **not a fault, it is a zero**, so the
  group stride read as 0 and every instance descended against instance zero's
  state. Both the bind range and the dynamic stride now use one constant sized
  for the larger of the two blocks.

- **The uniform cut, so every geometry path draws a DAG mesh.** `draw_gen.slang`
  picks one level per instance for `IndirectCount` and `IndirectPerBatch`, and
  `Scene::Dunes` renders on all three paths. Until now per-cluster selection
  existed only where there is an amplification stage, which excludes every
  browser, WARP and the macOS runner.

  **The level chosen is the finest at which any group is expanded**, each group
  measured against its **own** sphere. That is provably the per-cluster cut's
  own floor rather than an approximation of it: nothing below it is drawn per
  cluster, and something at it is. Measuring against the root group or a
  whole-mesh sphere instead over-selects without bound — a sphere containing
  every group's is never further from the eye, so it reports a larger error, and
  on a patch seen from its own edge it saturates at level 0 from everywhere.

  The two paths are compared three ways, not by "both drew something": the host
  rule equals `cut(...).map(level).min()` over a sweep; two real devices — one
  opened with the mesh-stage features and one without — agree camera for camera;
  and at a budget where both resolve to level 0 the frames are
  **byte-identical**. Selected level goes 0 → 1 → 2 at 2, 200 and 1000 units
  back.

  `mesh::DrawConstants` gained `mesh`, because a DAG level is its own vertex
  range and a draw of level 2's indices needs level 2's base vertex while the
  instance still names level 0.

- **`OffscreenSetup` now asks for `TASK_SHADER`, which it never had.** Every
  `render_e2e` run on a mesh-shader adapter had been going through the
  un-amplified `meshMain` — the golden frames were real, but not of the path the
  device advertised. The suite's "lesser path" arm now subtracts **both**
  mesh-stage flags, because Vulkan enables `meshShader` when `taskShader` is
  requested, and without that both arms selected the same path and the
  self-comparison guard fired.

- **Per-cluster LOD selection on the GPU — topic 25's runtime half.** The
  amplification stage descends the cluster DAG against projected screen-space
  error, so one draw of one mesh renders at several detail levels across its own
  surface. On a real GPU, the near third of the dunes patch draws
  `{level 0: 13 clusters, level 1: 12}` while the far third draws
  `{level 2: 14}` — identical on radv and lavapipe.

  **The GPU's chosen cut is asserted equal to the host rule's**, cluster for
  cluster across all 254, using the very `pixels_per_unit` and budget the
  renderer wrote into the frame block. So the shader's implementation of
  `projected_error` is held to the same metric as the two Rust ones rather than
  trusted to agree.

  **Both halves of the decision index a group, never a cluster.** Each
  `ClusterSelect` record carries the producing and containing groups'
  `(error, centre, radius)` copied into every cluster that group touches, so a
  group's clusters evaluate bit-identical inputs and a cut cannot split one
  across a boundary it never locked. There is no cluster centre in the descent
  at all, and a DAG whose grouping misses a cluster is refused rather than
  defaulted.

  A parallel per-cluster buffer rather than a wider `Meshlet`: that record is
  the wire format of the committed `dunes.dag`, its 48-byte stride is pinned
  against the offsets slangc emits, and the fields are meaningless for the cube,
  pyramid and open box.

  `ClusterDag::check_cover` promotes the crack-free edge-cover check out of the
  tests, so the host sweep and the GPU test run one implementation, and the
  read-back cut is asserted crack-free **at the shipped configuration** rather
  than by inference from a sweep that did not include it. Tearing that real cut
  proves it bites: dropping one cluster reports a 45-edge hole, its own
  boundary; drawing every cluster twice reports 5446 crowded edges.

  `set_dunes` refuses without `Features::TASK_SHADER` — with no amplification
  stage there is no descent, and a DAG mesh would draw all seven levels at once.

- **A cooked cluster DAG reaches the renderer's crate, and a model built to
  exercise it.** `crcbl_shaders::dunes` is a 64x64 height-field patch — 4225
  vertices, 8192 triangles, 64 units across against a 4-unit amplitude — and
  `crates/crcbl-shaders/clusters/dunes.dag` is its cluster DAG cooked to a
  committed binary artifact: 7 levels, 103 leaf clusters down to 6.

  The seam mirrors the shader arrangement. `tools/cook-clusters.rs` generates
  the artifact from `crcbl_scene::cluster_dag::build_cluster_dag`, `--check`
  regenerates and compares, and CI runs it. **`crcbl-shaders` stays
  dependency-free**: `crcbl-scene` already depends on it, so cargo refuses a
  normal dependency back and a `[[bin]]` cannot see dev-dependencies — but a
  dev-dependency cycle is allowed and an _example_ can see one, so the generator
  is an example and `cargo build -p crcbl-shaders` builds that crate alone.

  Every DAG invariant is re-asserted **over the committed bytes** rather than an
  in-memory DAG: coverage, crack-free cuts by the position-bit edge count,
  monotone error, group spheres containing every sphere below. Nothing was lost
  in cooking.

  The height function moved into `crcbl-shaders` and `crcbl-scene`'s test
  fixture delegates to it, so the surface the decimator is tested against and
  the one the engine draws cannot drift — the 93 existing `crcbl-scene` tests
  passing unchanged, including ones pinning exact triangle counts, is the
  evidence the arithmetic is bit-identical. Vertex normals come from the
  **analytic gradient** of the height, so a decimated level is shaded against
  the real surface rather than against faces the simplifier moved.

  From an eye at the near edge, the near third of the patch draws levels 0 and 1
  while the far third draws level 2 — a two-level gap across one draw of one
  mesh, driven by distance.

- **The cluster DAG carries what a GPU descent needs, and states the selection
  rule.** `ClusterGroup` gained `error()`, `bounds()` and
  `projected_error(eye, pixels_per_unit)`; `GroupBounds` is the group's sphere;
  `DagLevel::bounds()` reports the producing group's sphere per cluster.

  **Monotone stored error does not survive division by a distance** — a closer
  group projects larger from a smaller number — so a group's sphere is built to
  **enclose** the spheres of every group below it, in the same fold that raises
  its error to dominate theirs. A containing sphere is never further from any
  eye than one inside it, so `error / distance` rises up the DAG for every
  camera rather than for the ones that happened to get tested. The radius is
  taken in `f64` and rounded up one `next_up`, because narrowing to `f32` can
  leave a part a rounding step outside the sphere meant to contain it.

  Both halves of the descent index a **group**, never a cluster, so every
  cluster a group produced evaluates a bit-identical predicate and a cut cannot
  split a group across a boundary it never locked. Scaling by each cluster's own
  sphere instead makes the mesh crack, and there is a test that says so.

- **`build_meshlets` grows clusters across shared edges instead of walking the
  index buffer.** A cluster seeds on a triangle and repeatedly takes the
  edge-adjacent triangle with the most vertex reuse, then nearest the seed's
  centroid, then lowest index. On a 32x32 dune field the mean cluster bounding
  sphere goes from **16.04 to 6.90** on a mesh 32 units across, with 21 of 23
  clusters under radius 8 where **0 of 34** were before.

  Adjacency rather than a space-filling curve, for two reasons: a curve sorts
  space, so two surfaces a hair apart interleave into one cluster whose sphere
  spans the gap; and the vertex bound — which closes most clusters — is about
  vertex _sharing_, which adjacency measures directly and proximity only
  predicts. Distance is measured from the **seed**, not the cluster's moving
  centre, because a moving centre finds both ends of a strip equidistant and
  grows into a strip as long as the mesh.

  A cluster jumps to a disconnected component only if it can take the whole
  thing. That keeps a seam-split mesh — a heap of two-triangle components —
  clustering sensibly instead of one cluster per two triangles, and it is what
  leaves the cooked cube, pyramid and open-box constants bit-identical, so no
  golden moved.

  It also removed a stall in the cluster DAG: levels went
  `2048 → 1024 → 512 → 272 → 206 → 128` to a clean
  `2048 → 1024 → 512 → 256 → 128`.

- **`crcbl_scene::cluster_dag` — the crack-free cluster hierarchy topic 25
  specifies.** `build_cluster_dag` clusters the base mesh, groups neighbouring
  clusters by partitioning the **shared-edge** adjacency graph, locks each
  group's outer boundary while simplifying its interior, re-splits, and repeats
  with different groupings — so an edge locked at one level becomes interior at
  the next. Every cut through the result is crack-free by construction, which is
  what a chain of independently-clustered levels cannot give.

  `simplify_with_locked_edges` is the prerequisite: the simplifier infers
  topological borders on its own, but a group's outer boundary is **interior**
  to the mesh and can only come from the caller. `simplify` is now a one-line
  delegation with an empty set, so every pre-existing test exercises the new
  path and proves the old behaviour is unchanged.

  **One `simplify` call per level, not one per group** — deliberately. Handing
  each group over as its own mesh would put its boundary on a topological border
  and lock it for free, leaving the new parameter decoration, and would split
  the level's vertices per group so the next level's adjacency could not see
  through them.

  **Error is carried per group, not per cluster.** A group simplifies as a unit,
  so its parents stand or fall together; a cut drawing one while descending into
  another would tear along a boundary the group never locked.

  The crack test keys every drawn edge by the **bit patterns** of its endpoint
  positions and requires each to appear exactly twice except on the base border.
  Two levels number their vertices independently, so a leaf's interface edge and
  a parent's can only collide if the coarser level kept the finer one's vertices
  bit-exactly. It sweeps every threshold at which the cut changes, and asserts
  that several of those cuts genuinely mix levels — a uniform cut is the chain,
  which was never the problem.

  Its fixture is a 32x32 dune field, 2048 triangles, 34 leaf clusters and 6 DAG
  levels. Its height function is quartic rather than trigonometric because a
  fixture pinned by equality that uses `sinf` differs in the last place between
  glibc, Apple libm and MSVC, and fails only on a CI runner.

- **The sun casts shadows — topic 18's cascaded shadow maps, at two cascades.**
  `crcbl_render::shadow` computes practical-split distances, a stable
  sphere-around-the-eye fit and texel-snapped reversed-Z orthographic
  projections; `ForwardRenderer` renders a depth-only pass into a cascades-wide
  `D32Float` atlas, one `DrawGen` cull dispatch per cascade as the plan asks,
  and `mesh.slang`'s `sun_visibility` selects a cascade by eye distance and
  filters it with 3x3 hardware PCF. It runs on every `GeometryPath` —
  `mesh_cluster.slang` shares the fragment stage — and `SHADOW_CASCADES` is a
  constant checked against both shader sources, so three is a number rather than
  a rewrite.

  The shadow pass reuses the colour pipeline's own vertex and mesh stages
  unmodified, by binding a second copy of the frame block whose `view_proj` is
  the cascade matrix. There is no second transform path to drift.

  Shadowing multiplies the sun's diffuse and specular only, so a shadowed
  surface keeps its ambient and reads as dark rather than black.

  `crates/crcbl/tests/golden/cube.png` was re-blessed on lavapipe because its
  three co-located pyramids now shadow one another. **Vulkan and wgpu render the
  new reference identically, at zero differing pixels each** — two independent
  backends agreeing is what says the picture is right rather than one backend's
  bug blessed into a file. `mesh.png`, `mesh_second.png` and `mesh_ortho.png`
  are unchanged at zero differing pixels, which is the evidence there is no
  acne: a lone cube is pixel-identical to before.

- **`crcbl_scene::lod` — the LOD chain topic 25 specifies.**
  `build_lod_chain(positions, indices, ratios)` composes the simplifier and the
  meshlet builder into levels, each carrying its geometry, its clusters and its
  error, with `DEFAULT_LOD_RATIOS` the plan's 50/25/12.5/6.25 %. LOD0 is the
  base verbatim at error zero, so the chain is one longer than the ratio list.

  **Every level is decimated from the base mesh, not from the level above**, and
  that is the whole design decision. A quadric run accumulates the planes of the
  mesh _it started from_, so a cascaded level's error is measured against its
  predecessor rather than against LOD0 — measured on a torus, cascading reports
  `0.4917831` where decimating from the base reports `0.6015088` for the same
  level, an 18 % understatement that compounds downward. Runtime selection asks
  "may this stand in for the full-quality mesh", so every level has to be on one
  scale or the numbers cannot be compared. Cascading is cheaper and is exactly
  the option that cannot fill the error column honestly.

  Error is non-decreasing up the chain, asserted per adjacent pair — though note
  that invariant holds for **both** designs and so does not distinguish them; a
  separate test re-derives each level from the base to pin the provenance.

  **This chain supports per-instance selection only.** Each level is clustered
  independently, so two levels' cluster boundaries have no relationship and
  drawing one level's cluster beside another's cracks along the shared edge.
  That is the `IndirectCount`/`IndirectPerBatch` granularity;
  `docs/plan/03-gpu-driven-rendering.md` §3.5's per-cluster selection needs the
  grouped, boundary-locked, re-split DAG instead, which is a different builder
  and an open decision.

- **`crcbl_scene::simplify` — QEM mesh simplification, topic 25's auto-LOD
  generator.** `simplify(positions, indices, target_triangles)` returns a
  `Simplified` carrying the decimated mesh and its `max_error`. Iterative edge
  collapse ordered by Garland–Heckbert quadric error, cited to the 1997 paper in
  the module docs so the arithmetic can be checked against it, with unweighted
  quadrics exactly as the paper defines them.

  The collapsed vertex goes to the quadric-optimal position; a singular **or
  near-singular** matrix falls back to the best of the two endpoints and the
  midpoint. The near-singular half matters: a finite-check alone is not enough,
  because a nearly-singular quadric inverts to a finite but absurd answer — the
  test derives a case whose escaped vertex lands about 10⁶ units from a mesh
  whose planes all pass within one unit of the origin.

  Deterministic as topic 25 requires, and deliberately so: no hash-map iteration
  anywhere, a strict total order on candidates keyed by cost then endpoints then
  versions, survivors renumbered in ascending original index, faces emitted in
  original order, `f64` internally.

  Guardrails: border and non-manifold vertices are locked, faces that would
  invert or become slivers are refused, and the **link condition** — the two
  endpoints sharing exactly two neighbours — is enforced. That last one is not
  in the plan's list and the closed-mesh requirement silently depends on it:
  without it a torus at 25 % gains an edge with four faces and stops being
  closed.

  `max_error` is the largest `sqrt((Q_a + Q_b)(v̄))` over the collapses
  performed, in model units — the square root of the summed squared distances to
  the planes folded into those quadrics. **It is not a certified Hausdorff
  bound**, and the docs say so.

  **Not attribute-aware.** UV and normal seams, material boundaries and skin
  weights are all constraints on data this function is never handed; a seam that
  shares positions is invisible to it and will drift. That is the plan's own
  named auto-LOD risk and it is recorded rather than implied away. No cluster
  hierarchy, no runtime selection, no consumer yet.

- **Per-cluster culling in the amplification stage — §3.5's second bullet.**
  `mesh_cluster.slang` gained a task stage that rejects a cluster on the frustum
  and on its normal cone, `ForwardRenderer::culls_clusters()` reports whether it
  is running, and the surviving-cluster count rides `draw_gen`'s existing
  delayed readback as a second word beside the instance count — §3.6 promises
  one readback in the frame loop and this keeps it at one. The instance cull is
  unchanged and still runs first.

  `Features::TASK_SHADER` is separate from `MESH_SHADER`: a device with mesh
  shaders and no task shaders builds no task stage, culls nothing, and draws
  exactly as before.

  **The documented cull rule was wrong and is fixed in the same change.**
  `ClusterBounds::cone_cutoff` stated the point-sized form, which treats every
  triangle as sharing the centre's view direction — so a cluster with a real
  radius close to the camera could hold a front-facing triangle and still be
  rejected. The conservative form adds the radius:
  `dot(axis, center - camera) > sqrt(1 - cutoff²) · |center - camera| + radius`.
  A randomised check over 400 000 samples dropped a front-facing cluster **0**
  times with the corrected form and **11 225** times with the old one. The
  `cone_cutoff > 0` guard is still needed beside it: `sqrt(1 - cutoff²)` is even
  in `cutoff`, so it cannot tell a narrow cone from one wider than a hemisphere.

  Culling that rejects nothing passes every golden, so the tests count instead
  of looking. Four cameras, each measured with the box in the scene and out of
  it so the numbers are attributable: all five clusters survive from two cameras
  that see every face, the cone rejects two from the golden's camera, and the
  frustum rejects two from inside the box. The fourth camera exists only to pin
  the radius term — dropping `+ radius` leaves the other three counts untouched.

- **A mesh that clusters into more than one meshlet, and renders.** Both
  resident meshes were a single cluster each, so a cluster with a non-zero
  `vertex_offset` _within_ a mesh was covered by unit tests and by no rendered
  frame — and per-cluster culling would have had nothing to reject.
  `crcbl_shaders::mesh` gains an open box: a unit cube missing its `+Y` face,
  each remaining face divided into 4×4 quads with unshared vertices, which
  clusters into **five**, one per face. `ForwardRenderer` grew a third bucket to
  draw it, resident but not instanced by default, so no existing golden moved.

  Every coordinate is a multiple of a quarter, which is deliberate: the test
  that pins the cooked clusters against the real builder compares bounds for
  equality, and a trig-derived mesh would differ in the last place between
  glibc, macOS and MSVC — a failure only a CI runner could show you. The one
  irrational value is a radius of `sqrt(0.5)`, a single correctly-rounded
  operation.

  The box is open and inward-facing so that a camera exists from which every one
  of its clusters is front-facing. A closed shape has none, and that camera is
  what the culling work needs to assert nothing is rejected that should not be.

- **Every sample that builds a renderer now asks for the mesh path too.**
  `horde`, `breakout`, `flappy` and `asteroids` each spell their own
  `optional_features` and were the four that stayed on `IndirectCount` after the
  flip below. What this buys is sample rule 12 — "every sample runs on every
  path the device offers, and says which it took" — and a downgrade line that
  now names `MESH_SHADER` where a device lacks it. **It is not a performance
  change**: `apps/sandbox` is the only sample that constructs a
  `ForwardRenderer`, and `EmitTail::from_caps` is the only reader of
  `geometry_path()`, so the four draw every sprite through the same unbranched
  `encoder.draw` as before. Measured on horde at 10 000 instances, three repeats
  per arm: the between-arm difference is smaller than the within-arm spread, and
  the GPU timings are identical.

  `apps/hud` is deliberately left out. Its `desc()` omits `GPU_DRIVEN` entirely
  with a stated reason — nothing in it issues an indirect draw — and it builds
  neither renderer, so a mesh stage there would be a flag with no consumer.

- **A mesh-capable device now actually draws through the mesh path.**
  `MESH_SHADER` is requested as an optional feature at both sites that open a
  device with `GPU_DRIVEN` — `crcbl::GpuContextDesc::default` and the new
  `OffscreenSetup::OPTIONAL_FEATURES` — so `apps/sandbox`, `apps/bare` and the
  golden-frame harness select `GeometryPath::MeshShader` where the adapter
  offers it. `OffscreenSetup::open_with` is new public API for naming a
  different set.

  **It is named beside `Features::GPU_DRIVEN`, deliberately not added to it.**
  That bundle is used as `required_features` in four places against a null
  backend that reports no mesh shaders, so folding `MESH_SHADER` in would refuse
  those devices outright — and it would make every `gpu_driven()` test select
  the mesh path, deleting the only coverage the other two `GeometryPath` arms
  have. The bundle is the data-layout axis; geometry is a separate selector.

  The golden tests now assert the device took the best path its adapter offers,
  because a golden passing is equally consistent with nothing having changed.
  Every scene `render_e2e` covers — `ui`, `sprite`, `cube` — is compared byte
  for byte between the two paths in one process, and all three are identical on
  an RX 7900 XTX and on lavapipe. No golden moved.

- **The forward pass draws through a mesh pipeline, and it is the same
  picture.** `docs/plan/03-gpu-driven-rendering.md` §3.5's geometry path exists:
  `EmitTail::Mesh` is selected from `GeometryPath::MeshShader`,
  `crcbl_render::cluster_pool` uploads a mesh's clusters, and
  `mesh_cluster.slang`'s mesh stage emits them.
  `ForwardRenderer::geometry_path()` reports which path a renderer resolved.

  The GPU-facing record is new — `crcbl_shaders::meshlet::Meshlet` with
  `MESHLET_STRIDE` and `ClusterBounds`, beside `GpuMaterial` and `MeshVertex`,
  whose offsets are pinned against what `spirv-dis` reports the shader expects.
  `crcbl_scene::meshlet::build_meshlets` is still the builder and re-exports it;
  the record lives in `crcbl-shaders` because `crcbl-render` must not depend on
  `crcbl-scene`, which would pull `gltf` into the renderer. The builder's
  `usize` offsets narrow through `Meshlet::new`, the only constructor, which
  refuses an offset a `u32` cannot hold rather than truncating it.

  **The mesh path matches the indirect paths' own golden, not one of its own** —
  `tests/golden/mesh.png` at zero differing pixels on an RX 7900 XTX, and
  `every_geometry_path_draws_the_same_frame` compares all three paths byte for
  byte in one process. A new golden for a new path would have passed whatever
  that path happened to draw.

  **No app selects it yet.** `Features::GPU_DRIVEN` does not include
  `MESH_SHADER`, and `crcbl::GpuContextDesc::default` asks for `GPU_DRIVEN`, so
  the samples and `crates/crcbl/tests/golden/cube.png` still run `IndirectCount`
  on hardware that could do better. Only the vk device tests request the flag.

  Not built, deliberately: per-cluster culling in an amplification stage (there
  is no amplification stage at all — `ClusterBounds` is uploaded and read by
  nothing), cluster LOD, and any bake cache.

- **`apps/hud` runs in a browser, so every sample now has a demo on the Pages
  site.** It gained a `web.rs`, a `cdylib` library named `crcbl_hud`, the polled
  `PolledGpu`/`PendingLoop` bring-up the other samples use, and an entry at each
  registration site — `web/build.sh`, `web/build-pages.py`,
  `web/tools/browser-e2e.mjs`, `web/pages/`, `web/demos/` and a step in the
  Pages workflow. The bin target is unchanged; the library rename means the
  binary now says `use crcbl_hud::…`.

  It is the smallest wasm artifact of the five at 2 720 934 bytes, against
  horde's 3 028 644. `Game::log_heartbeat` is new and is the one behavioural
  addition: hud logged nothing from inside its tick, and the browser gate reads
  both "a paused demo runs no ticks" and hud's own advancing state off that
  line.

  hud takes no input, so its gate row asserts no key press and the shared
  `run-browser-e2e.sh` no longer claims every demo "took a real key event" — the
  per-check lines still name the key where there was one.

- **`crcbl_scene::meshlet` clusters a triangle list into meshlets.**
  `build_meshlets(positions, indices)` returns the meshoptimizer/NVIDIA
  three-array layout — the original vertex indices run per cluster, three `u8`
  corners per triangle indexing into that cluster's own run, and a `Meshlet`
  record naming both runs plus a `ClusterBounds` — under `MAX_CLUSTER_VERTICES`
  and `MAX_CLUSTER_TRIANGLES`. It is `docs/plan/03-gpu-driven-rendering.md`
  §3.5's bake step and it is deterministic: the same two arrays give
  byte-identical output, which is what §3.5 asks for and what a bake cache will
  need.

  `ClusterBounds` carries a bounding sphere (AABB midpoint and furthest vertex —
  valid, deliberately not minimal) and a normal cone whose axis is the
  area-weighted sum of the triangle normals and whose `cone_cutoff` is the
  smallest dot product any of them makes with it. A cluster whose normals cancel
  — a closed shape, a fan of opposing faces, nothing but zero-area triangles —
  gets `OMNIDIRECTIONAL_CUTOFF` and a unit `OMNIDIRECTIONAL_AXIS` rather than a
  NaN, because a NaN reaching a backface cull silently drops geometry. The cull
  rule the cone exists for is written out on `cone_cutoff` for the consumer.

  **This is the builder and nothing else** — no GPU upload, no amplification or
  mesh shader, no bake cache, no `GeometryPath::MeshShader` emit tail, and no
  caller. Each is a later slice, and the module says so.

- **Materials have a base-colour texture, through one `ArrayPages` page.**
  `docs/plan/03-gpu-driven-rendering.md` §3.2's "texture indices + factors" now
  has both halves: `crcbl_render::forward` uploads a `D2Array` image whose
  layers material rows index, `mesh.slang` samples it in the fragment stage and
  multiplies the texel into the factor and the vertex albedo.

  **One binding model, and it is the one every device can run.** The page is a
  single image with `count: 1` and no `BindingFlags`, so the layout is legal on
  vk, wgpu, Metal and D3D12 alike — `BindingModel::Bindless` needs
  `Features::DESCRIPTOR_INDEXING`, which `crcbl-mtl` withdraws, so a descriptor
  array would have left Metal with no texture path. Nothing is refused anywhere;
  a bindless device runs the same declaration and will gain capacity rather than
  a second code path.

  Colour space is the trap and it is handled by the format: the page is created
  as `Rgba8UnormSrgb`, which is what glTF defines a base-colour texture to be,
  so the sampler decodes to linear and the shader multiplies two linear values.

  **`CRCBL_GPU=wgpu` cannot draw the cube scene until `crcbl-hal` can say a
  sampled image is an array.** `BindingKind::SampledImage` carries no view
  dimension, so `crcbl-wgpu` declares every one as `D2` and refuses the page's
  `D2Array` view at `create_bind_group`. Vulkan, Metal and D3D12 take the
  dimension from the view and are unaffected; the sprite and UI scenes are
  unaffected on wgpu too. It fails at build with a named error rather than
  drawing untextured — see `docs/backlog.md`.

- **`crcbl_render::upload_texture_layers`** uploads several equally sized layers
  into one `D2Array` image, beside `upload_texture`'s single-layer `D2`. It
  records one copy per layer, because a copy region's extent is 2D on every
  backend the engine has.

- **`ForwardRenderer::set_textured_pyramid`** puts a third instance of the
  pyramid mesh in the frame, shaded through a material that differs from
  `set_pyramid`'s in its page layer and in nothing else — the texture column's
  observable, beside `set_tinted_pyramid`'s for the factor column. The `cube`
  golden holds all three, and `tests/golden/cube.png` was re-blessed for it.

- **The null backend can now be resized and killed on demand.** Two injection
  hooks join the four `crcbl_hal::null::Recorder` already had.
  `report_swapchain_out_of_date()` latches the swapchain out of date, so
  `acquire_next_frame`, `present` and `wait_until_presented` all report
  `SurfaceError::OutOfDate` until a successful `reconfigure_swapchain` clears it
  — the variant the seam calls expected traffic and that this backend could not
  produce at all, since every `SurfaceError` it built was `SurfaceError::Hal`.
  `lose_device(message)` loses the device permanently: every later call that
  resolves a handle, plus `Device::wait_idle`, fails with
  `HalError::DeviceLost(message)` and nothing clears it. It is the deliberate
  opposite of `report_device_error`, which stays recoverable and one-shot.
  Between them the engine's three out-of-date arms and its device-loss policy —
  loss surfaces, the loop stops, nothing is rebuilt — are testable on a machine
  with no GPU, where before they ran only on a real driver mid-resize.

- **`crcbl-scene` — glTF 2.0 import, through the asset seam.**
  `crcbl_scene::import_gltf(source, key)` reads a `.gltf` or `.glb` and every
  external `.bin` it names through `crcbl_assets::AssetSource`, and returns a
  `GltfScene`: meshes as triangle lists (positions, normals, `TEXCOORD_0`,
  indices), materials as `crcbl_shaders::mesh::GpuMaterial` rows, and the node
  hierarchy flattened into instances carrying a composed column-major model →
  world matrix. glTF's `baseColorFactor` is linear RGBA and so is
  `GpuMaterial::base_color`, so the material mapping is an assignment with no
  colour conversion. Nothing touches `std::fs`: a source that answers
  `StorageError::Pending` makes the import `Pending` too, which is what lets it
  work in a browser. No GPU upload, no textures and no scene format yet — those
  are the rest of `docs/plan/06-assets-scenes.md`'s step 3 and its step 4.

  **The crate does its own validation rather than using `gltf`'s**, because
  `gltf` 1.4.1's validation panics on inputs it exists to reject: an
  out-of-range `POSITION` accessor index aborts in
  `gltf_json::mesh::primitive_validate_hook`, and a `.glb` header declaring a
  total length below its own 12 bytes subtracts with overflow in
  `Glb::from_slice`. Every accessor, buffer view and index the importer reads is
  bounds-checked first, so no file contents can panic it: a truncated `.glb`, a
  chunk length that overruns, a buffer view past the end of its buffer, an
  accessor count that overflows its own byte span, an index past its vertex
  array, a node hierarchy with a cycle and a buffer URI that escapes the asset
  root are all errors. `data:` URI buffers and sparse accessors are refused as
  `StorageError::Unsupported`; primitives that are not triangle lists are logged
  and skipped.

- **`crcbl_assets::StorageError`** is re-exported, so a crate that implements or
  calls `AssetSource` can name the error it returns without depending on
  `crcbl-store`.

- **`crcbl-assets` — asset ids, load states, and the IO seam under them.** A new
  workspace member carrying `AssetId` (128-bit, printed as 32 hex digits,
  derived from the canonical asset key), `AssetRegistry` with
  `crcbl_core::Handle`-based handles and a `Loading | Ready | Failed` state
  machine, the `AssetSource` trait — one `read` that is defined never to block
  and answers `StorageError::Pending` while IO is outstanding — and `DirSource`,
  the native implementation over a directory. Keys are validated with
  `crcbl_store::web::canonical_key`, the same rule the browser fetch backend
  applies, so a name that loads from a directory is a name that can be served
  over HTTP; anything that would escape the asset root, or that HTTP would read
  as a query, fragment, scheme or another origin, is refused. Nothing decodes an
  asset yet: this layer hands out bytes, and the glTF/PNG/WAV importers are the
  next slice.

- **`apps/hud` — sample 04 at its first milestone.** A HUD page built from the
  UI system's slice-1 primitives: health and mana bars, a four-slot ability row
  whose slots read `READY` or sweep down a cooldown, a wave banner and a damage
  ticker, all driven by a server-owned ticker over `InMemoryTransport` and laid
  out against the acquired extent rather than a fixed size. It contributes two
  debug-overlay modules, and the `page` one tallies its rows off the draw list,
  so the panel reports what the UI pass actually uploaded rather than what the
  sample believes it drew.

  **Milestone 1 only**, which is what `docs/plan/sample/04-hud.md` scopes to P4.
  The stylesheet subset, the two themes, the widget gallery, the UI inspector
  and the hot-reload demo are P10 and wait on a styling system that does not
  exist; the minimap frame is left out because the hard cap forbids the scene it
  would frame.

- **`crcbl_render::MaterialTable`, and a material id that indexes something.**
  `docs/plan/03-gpu-driven-rendering.md` §3.2's material table: a storage buffer
  of `GpuMaterial` rows, one `base_color` factor each, which `mesh.slang`'s
  vertex stage multiplies into the vertex albedo. Two instances of one mesh
  differing in nothing but `GpuInstance::material` are two colours in one draw,
  which the cube golden now shows — `ForwardRenderer::set_tinted_pyramid` is the
  second pyramid there for exactly that.

  **The factors half only.** §3.2 pairs the table with a bindless texture array
  or texture array pages, and there is no texture column: which of the two an
  index would mean is a decision the engine has not taken, and a column carried
  ahead of it is a field nothing reads. One buffer and no ring, unlike the
  instance array — a material is written when it is created, so
  `MaterialTable::set` is a start-up call on the terms `MeshPool::upload` is.

- **`CRCBL_DX12_VALIDATION` turns the D3D12 debug layer on or off**, and it is
  on by default in a debug build and off in a release one — the shape
  `CRCBL_VK_VALIDATION` already has on Vulkan. It needs Windows' _Graphics
  Tools_ optional feature; without it `crcbl-dx12` warns and carries on, because
  a missing optional component must not stop the engine running. Because the
  layer writes to a debugger and CI has none attached, its messages are also
  pulled out of the `ID3D12InfoQueue` and put into the error a caller actually
  sees.

- **Every device-removed failure `crcbl-dx12` reports now names its reason.**
  `DXGI_ERROR_DEVICE_REMOVED` is reported at the _next_ call rather than the one
  that caused it, so the code alone is a symptom; `GetDeviceRemovedReason`'s
  answer — spelled out, not left as an `HRESULT` — and whatever the debug layer
  stored are appended to the message by `HalError::DeviceLost` and
  `HalError::Backend` alike, on resource creation, swapchain creation, resize,
  present, `GetBuffer` and every fence wait.

- **`CRCBL_ADAPTER` picks which adapter a screenshot opens a device on**, for
  every backend. It names a device _class_ — `cpu`, `integrated`, `discrete` or
  `virtual` — rather than an index, because an index is a position in one
  machine's enumeration and moves when a GPU is added or removed. Unset keeps
  the previous behaviour, whatever the backend enumerated first. A pin that
  matches no adapter, matches more than one, or is not a class at all is a hard
  failure naming what _was_ enumerated — never a fallback, for the reason
  `CRCBL_VK_ICD` exists: a harness that asked for the software rasteriser and
  silently got a discrete GPU produces a green run about a device nobody chose.
  The resolver is `crcbl::adapter` (`select`, `pin`, `device_type_from_name`)
  and `crates/crcbl/tests/run-render-e2e.sh` passes it through.

  The measurement behind it: `crcbl::screenshot` took `adapters().first()`, and
  on `windows-latest` that adapter is not a usable device — a D3D12 frame died
  on its first buffer with `DXGI_ERROR_DEVICE_REMOVED` in a job whose D3D12 HAL
  suite had just passed 155/155 on WARP. `crcbl-dx12`'s own `CRCBL_DX12_ADAPTER`
  is unchanged and still serves that crate's suite; it is `#[cfg(test)]` and
  could not reach a harness in another crate.

- **`OffscreenSetup::adapter`** returns the `AdapterInfo` the frame's device was
  created on, beside the existing `backend()` and `caps()`. A screenshot could
  not say which of a machine's adapters drew it, so a pin that never reached the
  process and one that was honoured looked identical from outside.

- **`crcbl-dx12` honours dynamic offsets.** A
  `BindingKind::UniformBuffer { dynamic: true }` or its storage-buffer twin was
  refused at `create_bind_group_layout`, and `bind_group` refused a non-empty
  `dynamic_offsets` again; both now work. Such a binding leaves the set's
  descriptor table and becomes a **root descriptor** — a root CBV, SRV or UAV,
  which takes a GPU virtual address rather than a descriptor handle, so the
  offset is one addition on the way to
  `SetGraphicsRootConstantBufferView`/`SetComputeRootConstantBufferView` and
  their SRV/UAV siblings. It costs no descriptor in the group's block, and it
  still takes its HLSL register in declaration order beside the table's.
  `ForwardRenderer`'s mesh set — whose binding 3 is dynamic — is a layout this
  backend can now build, and the forward pass's
  `bind_group(0, group, &[constant_offset], layout)` records.

  Three things are refused by name rather than discovered later: a dynamic
  binding with `count` other than 1 or with any `BindingFlags`, because a root
  descriptor is one address and is not in a descriptor heap; a `bind_group`
  whose offset count, alignment or bounds do not fit the set, checked against
  the device's own `min_uniform_buffer_offset_alignment` (256 on D3D12) and
  `min_storage_buffer_offset_alignment` (16); and a **pipeline layout that
  exceeds D3D12's 64-DWORD root signature budget**, at `create_pipeline_layout`
  rather than at the draw — a descriptor table costs one DWORD and a root
  descriptor two, so 32 dynamic bindings across a layout's sets are the ceiling.

- **`crcbl-dx12` records every draw the seam has.** `bind_index_buffer` sets a
  `D3D12_INDEX_BUFFER_VIEW`, `draw_indexed` is `DrawIndexedInstanced`, and
  `draw_indirect`, `draw_indexed_indirect` and both `_count` siblings are
  `ExecuteIndirect` through command signatures the device caches per
  `(argument layout, stride)` — D3D12 puts `ByteStride` on the signature rather
  than on the call, so two callers striding differently need two objects. A draw
  with no pipeline bound, an indexed one with no index buffer bound, an argument
  span that runs past its buffer, an unaligned argument or count offset, and a
  multi-command stride below one argument structure are each refused by name at
  record time, because `ExecuteIndirect` reports none of them.

- **`crcbl-dx12` reports `DRAW_INDIRECT_COUNT`, `MULTI_DRAW_INDIRECT` and
  `INDIRECT_FIRST_INSTANCE`**, and `Limits::max_draw_indirect_count` moves off
  the floor to `u32::MAX` — `ExecuteIndirect`'s own `MaxCommandCount` is a
  `UINT` and D3D12 states no lower ceiling. All three are parameters and fields
  of that one call rather than capability bits, and each is reported now that
  the call behind it is made. **`DRAW_INDIRECT_COUNT` moves a selector**: every
  D3D12 adapter now derives `GeometryPath::IndirectCount` where it derived
  `IndirectPerBatch`, so a renderer on this backend takes the arm that reads its
  draw count out of GPU memory. Metal cannot follow, because it has no
  count-from-memory execution at all.

- **`crcbl_shaders::Shader::dxil_containers` hands over every DXIL container a
  shader holds**, each paired with its entry-point name, in the shape
  `ShaderModuleDesc::dxil` takes. `Shader::dxil(entry_point)` still answers for
  one entry point; a call site filling in a descriptor wants the new accessor,
  and every one of the engine's passes now does — so the graphics passes offer
  DXIL where they previously offered none.

- **`CRCBL_GPU=dx12` selects the Direct3D 12 backend on Windows.**
  `crcbl::backend::GpuBackend` gains a `Dx12` variant, spelled `dx12` or `d3d12`
  wherever a backend is named — the environment variable, `--backend`, and
  `GpuBackend::from_name`. The registry entry exists on Windows alone, exactly
  as the Metal entry exists on macOS alone, and it is **never auto-selected**:
  Windows already reaches a GPU through `crcbl-vk`, and D3D12 is the same engine
  through a different loader rather than a replacement for it, so an
  unconfigured run there picks Vulkan as before. Off Windows the name still
  parses and resolving it reports `GpuError::UnknownBackend` naming the backends
  that build does have.

  The rest of the seam is not there yet: a `CRCBL_GPU=dx12` run of anything that
  builds a `ForwardRenderer` now builds every pipeline the renderer needs and
  gets as far as the forward pass's `bind_index_buffer`, which refuses with
  "indexed draws (the DX12 pipeline slice)". Adapter enumeration, buffers,
  images, bind groups, graphics and compute pipelines, a clear, a triangle, a
  dispatch and a swapchain all work.

- **`crcbl-dx12` runs compute.** `Device::create_compute_pipeline` builds a
  `D3D12_COMPUTE_PIPELINE_STATE_DESC` from the same root signature and the same
  validated DXIL container the graphics path uses, and
  `CommandEncoder::bind_compute_pipeline`, `dispatch` and `dispatch_indirect`
  record against it — the last through an `ExecuteIndirect` command signature
  the device creates once. A bind group issued inside a compute pass now lands
  on the compute bind point rather than the graphics one, which is the only
  signal the seam carries. `Features::COMPUTE` is reported as of this change and
  not before it, and the test behind the flag dispatches `compute_probe.slang`
  and reads back what it wrote.

  The seam's `ComputePipelineDesc::workgroup_size` is checked against the
  artifact, not just against the device's limits: `[numthreads(x, y, z)]` is in
  every signed container's `PSV0` part, so a descriptor that disagrees with the
  shader is refused by name here exactly as `crcbl-vk` refuses it from SPIR-V.

- **`crcbl-dx12` accepts `SurfaceTarget::Offscreen`**, so a D3D12 device can
  render into a texture and read it back with no window — what
  `crcbl screenshot` and every headless harness need. It used to refuse with
  "offscreen surfaces (a later DX12 slice)". The "swapchain" on such a surface
  is a ring of plain `ID3D12Resource` textures with no `IDXGISwapChain3` behind
  it, driven through the same `acquire_next_frame`/`present` pair a window uses:
  acquire reads a ring cursor instead of `GetCurrentBackBufferIndex`, present
  bumps it instead of calling `Present`, and `reconfigure_swapchain` recreates
  the images instead of calling `ResizeBuffers`.

  `Instance::surface_caps` answers for an offscreen surface from the ring's own
  capabilities rather than a window's, and they genuinely differ: flip-discard's
  format list and its two-image floor do not apply, so a ring may be one image
  deep, offers the same formats in the same order as `crcbl-vk`'s offscreen ring
  — `Rgba8UnormSrgb` first — and reports no `current_extent`. Presents on a ring
  are unnumbered, so `wait_until_presented` answers immediately rather than
  blocking on a waitable object that does not exist.

- **A screenshot now says which backend drew it and what that device selected.**
  `crcbl::screenshot::OffscreenSetup::backend` returns the `BackendKind` the
  registry opened and `OffscreenSetup::caps` returns its `DeviceCaps`, so a
  caller can read the `GeometryPath`, `BindingModel` and `LightingPath` the
  frame was actually rendered through. Without them the frame is the only
  output, and every backend draws this scene identically by construction — so a
  run pinned with `CRCBL_GPU` that silently fell back to another backend
  produced a passing frame and proved nothing about the one that was asked for.

- **The forward pass draws from GPU-generated indirect arguments.** `cull.slang`
  and a new `draw_gen.slang` run as two compute passes in front of the forward
  pass, and the pass records **one indirect call per bucket whatever the scene
  holds** rather than one draw per object — topic 03 §3.3, both halves. Adding
  or removing an object is an instance in `InstancePool` and changes no recorded
  command; how many instances a bucket draws, and which, is written by the GPU
  into buffers the draw reads. The barriers between the three passes, including
  the transition into `ResourceState::IndirectArgument`, are the render graph's.

  New: `crcbl_render::draw_gen::{DrawGen, DrawGenDesc, GeneratedDraws}` owns the
  two dispatches and their buffers, `crcbl_shaders::draw_gen` owns the workgroup
  size, uniform block and `DrawIndexedArgs` layout
  (`VkDrawIndexedIndirectCommand`, which D3D12 and `wgpu` spell the same way),
  and `ForwardRenderer::{draws, frame}` expose the generated buffers so a caller
  can read the culling statistics back.

  Which call the pass records comes from `GeometryPath`: `IndirectCount` issues
  `draw_indexed_indirect_count` per bucket with a GPU-written count, and
  `IndirectPerBatch` — Metal, whose API has multi-draw-indirect and no GPU-side
  count — issues `draw_indexed_indirect` with a count of one and leans on the
  bucket's instance count being zero. Both draw the same frame byte for byte.
  `GeometryPath::MeshShader` has no tail here yet and degrades to an indirect
  one, with a log line saying so.

- **`InstancePool::slot_count`**, the array elements a walk of the pool has to
  cover: one past the highest slot ever handed out, which — unlike `len` — does
  not shrink when an instance is removed from the middle. The cull dispatch is
  sized by it.

- **A removed instance stops being drawn.** `GpuInstance::flags` gains its first
  defined bit, `GpuInstance::LIVE` (bit 0): set, the element is a live instance;
  clear, it is a slot whose instance was removed and is still holding the
  transform and mesh id it had. `cull.slang` asks that bit before it reads
  anything else in the record, and `crcbl_render::cull::visible_instances` — the
  CPU oracle — does the same, so a freed slot is no longer culled (and possibly
  kept) on stale data. The layout is unchanged: `flags` was already there and
  already 4 bytes at offset 76.

  `InstancePool` owns the bit rather than its callers. `insert` and `set` set it
  whatever the caller passed, `remove` clears it and marks the slot dirty so the
  next `begin_frame` carries the removal to the device, and **nothing else about
  a removed record is rewritten** — a zeroed instance would be a live-looking
  cube at the origin for any consumer that skipped the check.
  `InstancePool::new` now also clears its buffers, so a slot nothing has written
  reads as dead rather than as whatever the driver left there; a pass that walks
  the array from element zero is what makes that difference visible.

  The consequence for a caller is that the cull pass may be dispatched over the
  pool's whole capacity: correctness no longer rests on an `instance_count` that
  happens to stop before the first freed slot.

- **GPU frustum culling, checked against a CPU reference.** `crcbl-shaders`' new
  `cull.slang` is `docs/plan/03-gpu-driven-rendering.md` §3.3's compute pass:
  one thread per instance, the mesh's local-space AABB transformed by the
  instance transform, tested against six camera half-spaces, and the survivors
  appended to a compacted list of instance indices with an atomic counter.
  `crcbl_shaders::cull` carries its `Params` block (six `float4` planes, an
  instance count and a list capacity) and `WORKGROUP_SIZE`. The counter is the
  **true** survivor count and can exceed the list's capacity — an overflow is a
  number a caller can see rather than a list that quietly stops growing.

  `crcbl_render::cull` is the same cull in ordinary Rust: `Aabb`, `Frustum`
  (Gribb-Hartmann plane extraction from a view-projection matrix, deliberately
  **not** normalized — under the engine's reversed-Z infinite projection the far
  plane has a zero normal, and normalizing it produces `NaN`s that cull
  everything), and `visible_instances`. `Aabb::transformed` is the standard
  conservative absolute-value-matrix bound, because a rotated box is not a box.

  **Nothing consumes the visible list yet**: `ForwardRenderer` records the same
  draws it always has, and no pass in `crcbl-render` dispatches the shader.
  Indirect draw generation is the next slice. What exists now is the cull math
  and its proof — `crcbl-vk`'s `cull` e2e reads the list and the counter back
  and compares them against the Rust reference over instances placed inside,
  outside each of the six planes, straddling one, rotated back in, and naming a
  freed mesh.

- **A GPU-side mesh table, and `GpuInstance::mesh` now means something.**
  `MeshPool` maintains a third buffer beside the vertex and index pools: one
  `crcbl_shaders::mesh::GpuMesh { base_vertex, base_index, index_count, bounds_min, bounds_max }`
  (36 bytes, `MESH_ENTRY_STRIDE` — nine scalars, no padding) per mesh it can
  hold, at `mesh.slang`'s new binding 4. The bounds are the mesh's local-space
  box, computed by `MeshPool::upload` from the vertex positions it is handed and
  carried on `MeshRange::bounds`; they live in the range's own record because
  they share its lifetime exactly, and the cull pass above is what reads them.
  `MeshPool::table_index` is the id an instance carries and
  `MeshPool::table_buffer` is what a bind group names; `MeshPoolDesc` grew
  `mesh_capacity` and `MeshPoolError::MeshTableFull` reports a table with no
  entry left. **The vertex stage now resolves its own base vertex** — through
  the drawn instance's `mesh` id — instead of being handed one per draw. That is
  what `docs/plan/03-gpu-driven-rendering.md` §3.3's cull pass needs (it emits
  draws the CPU never looked at, so the geometry has to be resolvable from
  instance data alone), and it already buys something before that pass exists: a
  base vertex resolved per _instance_ lets one draw cover instances of different
  meshes, where a per-draw constant made every instance in a draw share a mesh.
  The rule that produced the block is untouched — every draw still passes zero
  for both of its own bases.

  A freed mesh's entry is **cleared**, so an instance still naming it resolves
  to the empty range (`index_count == 0`) rather than to whatever mesh next
  lands in that space; `MeshPool::free` therefore takes a `&dyn Device` and
  returns `Result<bool, MeshPoolError>`, and frees nothing if the clear fails.
  What clearing cannot cover is a table _slot_ reused by a later upload: a mesh
  id is a bare `u32` with no generation in it, so a stale id names the mesh that
  took the slot. `MeshHandle` is the generational one, and only it can tell
  those apart.

  Upgrading: `MeshPoolDesc` needs `mesh_capacity`; `MeshPool::free` needs the
  device; anything building `mesh.slang`'s descriptor set by hand must add
  binding 4 — a read-only storage buffer of `GpuMesh` — and every instance it
  draws must carry a mesh id that indexes it, because id 0 is a real entry and
  an instance that forgot its id draws whatever mesh sits there.

- **A second mesh in the geometry pool, and `ForwardRenderer::set_pyramid` to
  draw it.** `crcbl_shaders::mesh::pyramid_vertices` / `pyramid_indices` /
  `pyramid_vertex_bytes` are a square pyramid in five colours no cube face has,
  uploaded after the cube so it is the pool's first resident at a **non-zero**
  base vertex. Off by default, so the frame every sample draws is unchanged;
  `crcbl screenshot --scene cube` and a new `crcbl-vk` golden
  (`tests/golden/mesh_second.png`) turn it on. It exists to make a base vertex
  observable — see the fix below, which no picture could show while the pool
  held one mesh.

- **The instance array: `crcbl_render::instance_pool`, and the cube is now an
  instance.** `InstancePool` owns one `crcbl_shaders::mesh::GpuInstance` storage
  buffer per frame in flight and uploads **deltas** —
  `docs/plan/03-gpu-driven-rendering.md` §3.2's "changed instances only, dirty
  ranges, not full re-upload". `insert`/`set`/`remove` take generational
  `InstanceHandle`s; `index` gives the element number a shader addresses;
  `begin_frame` rotates to the next buffer, writes that buffer's outstanding
  changes, and returns the slot the caller's bind group and uniform ring should
  use. Adjacent writes coalesce into one `write_buffer` — instances 3, 4 and 5
  are one upload of three, 3 and 900 are two of one, in whatever order the
  writes arrive — and a frame in which nothing changed performs **no seam call
  at all**. The pool never grows: `InstancePoolError::PoolFull` names the
  capacity and what is in use.
- **`crcbl_shaders::mesh::GpuInstance` (80 bytes) and `INSTANCE_STRIDE`**, plus
  `mesh.slang`'s matching `struct GpuInstance` at binding 2. `transform` is a
  rigid model-to-sector `float4x4`; `mesh` indexes the mesh table (see the entry
  above, which is what made it mean something); `material`, `sector` and `flags`
  are **reserved and read by nothing**. In particular `sector` is _not_ working
  camera-relative rendering: §3.2's 2026-07-27 correction also calls for a
  per-frame f64 sector→camera offset table and a shader-side addition, and
  neither exists, so every instance is in sector 0 and `transform` is a plain
  model→world matrix. The field is in the format now because extending it after
  §3.3's shaders index it is the expensive path.
- **Global geometry pools: `crcbl_render::mesh_pool`, and the cube now lives in
  one.** One device-local vertex buffer and one index buffer, suballocated by a
  first-fit free list, so a mesh is
  `MeshRange { base_vertex, base_index, index_count }` — the three integers
  `docs/plan/03-gpu-driven-rendering.md` §3.1 asks for and everything above it
  (instance data, GPU culling, indirect draws, meshlets) assumes.
  `MeshPool::upload` suballocates both pools, stages the bytes and submits the
  copy against the pool's own timeline semaphore; `MeshPool::flush` waits for
  that value and retires the staging buffers; `MeshPool::mesh` hands out a range
  **only** for a mesh whose upload has completed, so the renderer cannot consume
  geometry the GPU has not received. `MeshPool::free` returns a mesh's space and
  retires its handle. `ForwardRenderer` no longer owns two buffers of its own:
  the cube is the pool's first resident, drawn as a range with `draw_indexed`'s
  own base vertex, and the `mesh` and `ortho mesh` goldens are unchanged by the
  move.
- **The pools never grow and never defragment, and say so by name.** Capacity is
  fixed at `MeshPool::new`; a request no single free block can satisfy fails
  with `MeshPoolError::PoolExhausted`, which names the largest free block _and_
  the total free so a caller can tell fragmentation from a full pool. This is
  §3.1's stated MVP — "free-list + offline compaction on load only, no live
  defrag" — rather than an omission; the free list does coalesce neighbouring
  frees, so an alloc/free/alloc cycle reuses its space.
- **Mesh shaders are usable end to end: `Device::create_mesh_pipeline`,
  `CommandEncoder::draw_mesh_tasks`, and a golden image of the result.**
  `Features::MESH_SHADER` was reported and nothing could ask for it. The seam
  now takes a `MeshPipelineDesc` — a task stage (optional), a mesh stage, a
  fragment stage, and **no vertex input at all** — and returns an ordinary
  `GraphicsPipelineHandle`, so a mesh pipeline is bound with
  `bind_graphics_pipeline` and destroyed with `destroy_graphics_pipeline` like
  any other. `draw_mesh_tasks(x, y, z)` is the draw, taking workgroup counts of
  whichever stage the pipeline starts with. `ShaderStages` grows `MESH` and
  `TASK`, deliberately **outside** `GRAPHICS` and `ALL`, because a stage flag
  naming a stage the device lacks is refused rather than ignored. `crcbl-vk`
  implements both through `VK_EXT_mesh_shader`; `crcbl-wgpu`, `crcbl-mtl` and
  `crcbl-dx12` refuse them with `HalError::Unsupported` and report no
  `MESH_SHADER`. A device that does not report the capability refuses pipeline
  creation by name rather than failing later, and `TASK_SHADER` is refused on
  its own flag.
- **`crcbl_shaders::MESH_SHADER`, from `shaders/mesh_shader.slang` — the first
  shader that is not all four targets.** One triangle emitted by a mesh stage,
  plus an amplification stage whose payload tints it, plus the fragment stage
  both share. It declares `spirv, msl, dxil` and **not** `wgsl`, because Slang
  refuses a mesh entry point for that target outright; this is the first real
  use of the per-shader target declaration, which exists precisely so the
  refusal is a build failure rather than a broken committed artifact. The
  compile script and `build.rs` learned the `meshext` and `taskext` execution
  models, and `crcbl_shaders::Stage` grew `Mesh` and `Task`.
  `crcbl_shaders::mesh_shader` carries the triangle's positions, its colours and
  the amplification tint for the tests that sample them, plus
  `vertex_bytes`/`VERTEX_STRIDE` for the storage buffer the mesh stage pulls
  them from.
- **A bind-group layout and a push-constant range may now name
  `ShaderStages::MESH` and `ShaderStages::TASK`, so a mesh shader can read a
  buffer.** Until this, nothing accepted either flag and `mesh_shader.slang`
  hardcoded its three vertices; it now pulls them from a `StructuredBuffer` at
  set 0 binding 0, the way `triangle.slang` does. A layout entry's `visibility`
  or a `PushConstantRange::stages` naming a stage the device does not report is
  refused up front with `HalError::Unsupported` — by
  `ShaderStages::check_supported`, which every backend calls, rather than by a
  driver VUID that names neither the binding nor the capability. The two stages
  are still outside `GRAPHICS` and `ALL`, so nothing that already worked
  changes. The amplification stage's payload became a local rather than a
  module-scope `groupshared`, which is what keeps the emitted MSL legal: Slang
  2026.14 hands every entry point of a module with any global shader parameter a
  copy of every global, so the `groupshared` one landed in the fragment function
  as a `threadgroup` declaration that `xcrun metal` refuses.
- **`crcbl-vk`'s e2e suite gained a `GeometryPath::MeshShader` golden**,
  `tests/golden/mesh_shader_triangle.png` — apex-down, so it cannot be satisfied
  by a copy of the raster triangle's — alongside tests that the mesh stage's
  three vertices reach memory, that the amplification stage's payload actually
  arrives (the tinted frame differs from the untinted one in a way only the task
  stage can produce), and that a mesh pipeline naming a fragment entry point as
  its mesh stage is refused by name.
- **`crcbl_core::log::capture`, so a log line can be asserted on.** Returns a
  `Capture` guard that collects every record the **calling thread** logs, as
  `CapturedRecord { level, target, message }` read back through
  `Capture::records`. Capture is thread-scoped so concurrent tests in one binary
  cannot interleave their records, and it sees every level regardless of
  `CRCBL_LOG`, so an assertion does not turn on the environment. It is additive:
  stderr still gets exactly what the filter admitted, and a process that never
  calls it behaves as before. `capture` panics rather than capturing nothing if
  this thread is already capturing or if another logger owns the process slot.
  It is now what holds four `crcbl` log lines to their wording, each of them the
  only evidence its decision was taken and none of them read by anything before:
  the capability downgrade line from `docs/plan/39-capabilities.md`, the
  present-feedback line whose existence is why `wait_until_presented` was left
  returning `()`, the pacing resolution's
  `hal: display timing …; asked for …, pacing …`, and
  `engine: the frame limit is …`. The last two were previously checked only by
  `crates/crcbl-shell/tests/run-wayland-e2e.sh`, so on a machine without a
  Wayland compositor they could be deleted with the suite staying green.
- **`crcbl screenshot --scene`, and a cross-backend comparison that runs every
  scene.** The subcommand takes `cube` (the default, unchanged: the lit cube
  through `ForwardRenderer`), `sprite` — four sprites over three
  `SpriteRenderer` batches in `A A B A` submission order, two sheets, one of
  them tinted — and `ui`, a panel, a translucent bar over its edge, an outline
  and two lines of glyph-atlas text through `UiRenderer`. The library side is
  `crcbl::screenshot::Scene`, taken by `OffscreenSetup::open`, which is a
  breaking change to that signature.
  `crates/crcbl/tests/run-cross-backend-e2e.sh` now renders **every** scene
  through both backends at every size and compares each; its anti-vacuity colour
  floor is per scene (`CRCBL_CROSS_MIN_COLORS_CUBE`, `_SPRITE`, `_UI`, replacing
  the single `CRCBL_CROSS_MIN_COLORS`) because a UI frame has 7 distinct colours
  where the lit cube has 36–41, and its "zero comparisons ran" guard is now
  checked per scene as well as overall. This is
  `docs/plan/02-vulkan-backend.md`'s shader-portability rule 5: semantic
  divergence between the four targets is caught by rendering, not by reading,
  and the gate previously drew one scene — so `sprite.slang` and `ui.slang`, the
  two shaders with an actual history of divergence, were not covered at all.

- **Every shader declares the targets it must compile to, and the compile script
  emits exactly those.** Each `crates/crcbl-shaders/shaders/*.slang` opens with
  a `// crcbl-targets: spirv, wgsl, msl, dxil` line; `tools/compile-shaders.sh`
  refuses a source with no declaration, an unknown target name, or a declaration
  without `spirv` (the entry points every other target is driven from are read
  out of the SPIR-V), and refuses an artifact left in the tree for a target its
  shader no longer declares. The declaration is recorded as a `targets` key in
  `spirv/manifest.txt` and reaches
  `crcbl_shaders::manifest::ShaderRecord::targets`, where a record whose
  declaration and artifact columns disagree is rejected — so the check also runs
  in `build.rs`, on machines with no shader compiler. Every shader shipped today
  declares all four; the mechanism exists for the first mesh-shader or
  ray-tracing source, which will have no WGSL form at all.

- **A per-target preprocessor define, so a shader can differ by target without
  being forked.** `tools/compile-shaders.sh` and `build.rs` pass exactly one of
  `CRCBL_TARGET_SPIRV`, `CRCBL_TARGET_WGSL`, `CRCBL_TARGET_MSL` and
  `CRCBL_TARGET_HLSL` (the DXIL leg, named for the language Slang emits on the
  way). Slang defines no target macro of its own, so until now the only way to
  differ per target was a second copy of the file. No committed artifact
  changed: the defines are inert in every shader that ignores them.

- **A lint that refuses a shader declaring its resources out of binding order.**
  Slang's Metal target ignores `[[vk::binding]]` and assigns argument-table
  indices in declaration order, while `crcbl-mtl` binds by ascending
  `(set, binding)`; when `ui.slang` disagreed with itself, its MSL put the push
  constants where the vertex buffer should have been and the UI pass drew
  nothing on macOS. `crcbl-shaders` now parses every `.slang` and asserts
  ascending `(set, binding)` with push constants last, which is where Slang's
  Metal target puts them and where `crcbl-mtl` leaves room. A comment was
  previously the only thing preventing a recurrence.

- **The WGSL and MSL artifacts are validated, where before only the SPIR-V
  was.** `crcbl-shaders` gained `tests/wgsl_validation.rs`, which parses and
  validates every committed `wgsl/*.wgsl` with naga — the same front end `wgpu`
  compiles WGSL through, so a module it rejects is a pipeline that fails to
  create — and cross-checks the set it swept against the manifest records
  declaring `wgsl`. `wgsl/ui.wgsl` shipped for months with an undecorated
  `var<uniform>` that naga refuses outright, which `crcbl-wgpu` could never have
  loaded; that artifact is checked in as a fixture so the failure path stays
  exercised. naga is a dev-dependency pinned to the version `wgpu` already
  resolves — the library itself still has no dependencies. The MSL is compiled
  with `xcrun metal` on the macOS CI job, which is the only place it can be
  checked at all, and that step fails if it compiled zero files. **naga
  accepting a module is not Dawn accepting it**: Dawn enforces WGSL's uniformity
  rule where naga does not, which is how the UI shader's non-uniform
  `textureSample` drew a black canvas in the browser. This narrows the gap; it
  does not close it.

- **The engine names every capability it asked for and did not get, once, at
  device creation.** `crcbl_hal::downgrades(requested, granted)` returns a
  `Downgrades` describing each absent optional feature and the path selector its
  absence moved (`Downgrade::feature`, `::name`, `::selected`, the last a
  `SelectedPath::{Geometry, Binding, Lighting}`); `GpuContext`'s open logs it
  when it is not empty, as
  `hal: this device does not have DESCRIPTOR_INDEXING -> binding ArrayPages, …`.
  A device that got everything logs **nothing**, which is what makes the silence
  readable: `IndirectPerBatch` in the path line is now distinguishable from a
  descriptor that never asked for the count. `GeometryPath::INPUTS`,
  `BindingModel::INPUTS` and `LightingPath::INPUTS` are new, and state which
  features each selector is derived from.

- **Five `crcbl_hal::Features` flags for mesh shading and ray tracing.**
  `MESH_SHADER`, `TASK_SHADER`, `RAY_QUERY`, `RAY_TRACING_PIPELINE` and
  `ACCELERATION_STRUCTURE`. `MESH_SHADER` is the best `GeometryPath` and
  `RAY_QUERY` plus `ACCELERATION_STRUCTURE` together select
  `LightingPath::RayTraced`. Vulkan reports them (below); wgpu, Metal, D3D12 and
  the null presets report every one of them clear, so a device on those backends
  still selects the same path it did before.

- **`crcbl-vk` reports mesh shading and ray tracing, and enables them when
  asked.** An adapter's `DeviceCaps` now carries `MESH_SHADER` / `TASK_SHADER`
  from `VK_EXT_mesh_shader`, and `ACCELERATION_STRUCTURE` / `RAY_QUERY` /
  `RAY_TRACING_PIPELINE` from `VK_KHR_acceleration_structure` (with its
  `VK_KHR_deferred_host_operations` dependency), `VK_KHR_ray_query` and
  `VK_KHR_ray_tracing_pipeline` — each only when the extension is listed **and**
  its feature bit came back true, and only when everything it depends on is
  there too: neither ray capability is reported without the acceleration
  structure it traverses, and the task stage is not reported without the mesh
  stage it feeds. `GeometryPath::MeshShader` and `LightingPath::RayTraced` are
  therefore reachable for the first time. The extensions are enabled only when a
  caller names the capability in `required_features` or `optional_features`, so
  device creation is unchanged for everyone else. **This is reporting only** —
  no mesh-shader pipelines, no acceleration structures, no ray-tracing commands
  yet.

- **The X11 F11 pass now asserts the summary-line extent.** `run-x11-e2e.sh`'s
  toggle pass used to press F11 at a running sandbox, check the engine's own log
  line about the mode, and SIGTERM the sandbox — so the _extent_ after F11 was
  never checked. The key sender (`crcbl-e2e-x11-key`) now walks the X11 tree
  from the root (`Peer::find_window`, a new QueryTree + `WM_CLASS` binding
  behind the `x11-e2e` feature), finds the sandbox's window, and asks it to
  close with `WM_DELETE_WINDOW`; the sandbox tears down cleanly and prints its
  end-of-run summary, and the script asserts it reads `at 1920x1080, borderless`
  under a window manager and `at 1280x720, windowed` without one. A new suite
  test pins the window-finding walk against a unique `WM_CLASS` both with and
  without `openbox`.

- **A shot that kills a rock now raises a flash where it died.** The rock used
  to vanish and split with only the explosion cue to mark the hit;
  `apps/asteroids` now draws a two-frame burst — a white-hot core for the first
  half of the flash's 0.15 s life, a wider, dimmer fade for the second — scaled
  to cover the rock that died. The flash lives in the seeded simulation beside
  the cue, so a recorded script replays the picture as well as the score;
  particles remain a hard non-goal and this is a sprite, not one.

- **The sandbox's pause menu can change pacing and the frame cap mid-run.** Two
  new rows — `PACING: AUTO` and `FPS: 1000`, each labelled with the value it is
  set to — cycle on Enter: pacing through `Auto` → `Vsync` → `Adaptive` → `Off`,
  the cap up 30 → 60 → 120 → 240 → 1000 → unlimited. The pacing change lands on
  the GPU on the first tick after resume, through the sample's own `Gpu` and
  `GpuContext::set_pacing`; the cap change is handed to the loop through the new
  `HostedGame::take_pending_frame_limit`, which applies it to its clock with
  `Clock::set_limit` and takes it so it is not re-applied every frame. This is
  the first code in the workspace to exercise either mid-run route from a
  running game; the games without a settings screen use the method's default
  `None` and are untouched.

- **`NineSliceSource` carries its own texels-per-unit scale.**
  `with_texels_per_unit` (default 1) makes the fixed bands of `expand` and
  `minimum_size` come back in the caller's units, so a game whose world is not
  one unit per texel no longer has to scale its sprite plane and camera to
  compensate. The flappy and breakout samples were migrated to world-unit sprite
  planes; the menu's camera workaround is still owed (backlog).

- **`--pacing` and `--fps`, so a run can pick its display sync and its frame cap
  from the command line.** `--pacing <auto|vsync|adaptive|off>` sets
  `GpuContextDesc::pacing` and `--fps <N>` sets the loop's `FrameLimit`; both
  are on `crcbl::args::Common`, so every sample that takes the shared flag set
  gets them, and `apps/sandbox` — which keeps its own parser — takes them too.
  The defaults are unchanged and are what they always were: `auto`, which is
  adaptive sync where the display is running it and vsync where it is not, and
  1000 fps, which is a runaway guard rather than a cap. `--fps 0` is unlimited,
  the spelling `FrameLimit::fps` already documented. An unknown pacing is
  refused by name and lists the four — `--pacing vrr` is told the word here is
  `adaptive` — and `--fps` refuses a value that is not a number or does not fit
  a `u32` rather than truncating it.

  **A run now says what it got.** `Clock::set_limit` logs one `info` line,
  `engine: the frame limit is 30 fps` (or `unlimited`), on the real clock only —
  a headless run has no frame limit to report. The pacing already appeared on
  the `hal: display timing …; asked for …, pacing …` line.

  **A game can still pick both, and change them while it runs.** The flags are
  the command line's route to values a game may set for itself: `Common` is an
  ordinary struct with public fields, `crcbl::engine::Loop::clock_source_mut` is
  new and is the frame limit's counterpart to `GpuContext::set_pacing`, and the
  `crcbl new` template documents both routes where a scaffolded game would look
  for them.

- **`--size <WxH>`, so a run picks the extent its window opens at.** The value
  is on `crcbl::args::Common` as `size: Option<PhysicalSize>`, so the four
  samples that take the shared flag set — and the `crcbl new` template — open
  their window at the size named instead of their hardcoded default; a `WxH`
  that is not two positive numbers is refused by name. It exists for the
  headless measurement the samples were otherwise stuck at one extent for: the
  offscreen ring takes its extent from the window, and `--size 1920x1080`
  renders at exactly 1920 × 1080 (the window request is logical at scale 1).
  `apps/sandbox`, which keeps its own parser, does not take it.

- **The HAL can be asked what the display is doing with presented frames, not
  just what was requested.** `crcbl_hal::DisplayTiming` is a new four-state
  answer — `Unknown`, `Fixed { cycle }`, `Variable { shortest }` and
  `Stepped { cycle, step }` — returned by the new
  `Device::display_timing(swapchain)`, and gated by the new
  `Features::PRESENT_TIMING` (outside `TIER_A`, like `PRESENT_FEEDBACK`). A
  `PresentMode` is a request; this is the observation, and it is the only thing
  in the seam that distinguishes a fixed 60 Hz panel from an adaptive one
  currently sitting at 60 Hz. **It is a live query — the answer changes when a
  laptop enters power-saving mode or a window moves to another monitor — so
  callers must not cache it.** A device without the capability answers
  `Ok(DisplayTiming::Unknown)` rather than erroring, exactly as
  `wait_until_presented` answers `Ok(())`; a foreign or destroyed swapchain is
  still `ForeignObject`/`InvalidHandle` on every backend. The free function
  `display_timing_from_refresh_nanos` is the conversion from a presentation
  engine's two nanosecond figures, exposed and unit-tested on its own because
  every subtle mistake in this feature lives there. `crcbl-vk` implements it
  against `VK_EXT_present_timing` through hand-written FFI (`ash` has no
  bindings for it); `crcbl-wgpu`, `crcbl-mtl` and `crcbl-dx12` answer `Unknown`
  and document what their platform would need to do better.

  **The engine reads it once, at start-up, and paces on the answer.**
  `GpuContextDesc::default()` asks for `Features::PRESENT_TIMING` beside
  `PRESENT_FEEDBACK`, so the extension chain is negotiated on a device that has
  it, and `GpuContext::submit_and_present` queries after its **first** present —
  after, because the platform may report nothing until an image has been
  presented; once, because a driver that only ever answers `Unknown` would
  otherwise be asked again every frame for the life of the process. The outcome
  is one `info` line beginning `hal: display timing `, naming all three of what
  was asked, what the display reported and what is in force
  (`hal: display timing Unknown; asked for Auto, pacing Vsync`), so "asked for
  `Auto` and the display said `Variable`" is distinguishable in a log from
  "asked for `Adaptive`". A failed query degrades to `Unknown` and a `debug`
  line; it never fails a frame that has already been presented. Resizes,
  display-mode changes and out-of-date presents do **not** re-run it — a window
  dragged onto a VRR monitor keeps the pacing it started with until the game
  asks for another.

- **`Pacing::Auto`, and games can switch pacing at runtime.**
  `GpuContext::set_pacing(Pacing)` changes how frames are paced mid-run,
  rebuilding the swapchain **only** when the present mode it resolves to differs
  from the one presenting — so a settings screen that re-applies every value on
  every apply costs nothing. `GpuContext::pacing()` reports what was asked for
  and `GpuContext::effective_pacing()` what is actually in force (never `Auto`),
  because a caller that asked for `Auto` and got vsync needs to tell that from
  having asked for vsync. A failed switch rolls both of them and the swapchain's
  mode back together, leaving the context usable on the pacing it already had.

- **The demo site is served cross-origin isolated, and the browser gate asserts
  it.** `web/tools/serve.mjs` is a new static server that sends
  `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp` — the pair a browser requires
  before it will hand out `SharedArrayBuffer`, and therefore before any wasm
  build with `+atomics` can run. `web/build.sh --serve` runs it in place of
  `python3 -m http.server`, and `web/tools/browser-e2e.mjs` imports it instead
  of keeping a second server of its own, so the origin the gate checks is the
  origin a human loads. Group A now asserts `crossOriginIsolated === true` and
  that `new WebAssembly.Memory({ shared: true })` actually succeeds, and
  `run-browser-e2e.sh` fails a run whose output does not contain that check by
  name — the headers are otherwise something nothing in the repository would
  notice going missing.

  `--serve` binds loopback only now, where `python3 -m http.server` bound every
  interface: `http://<lan-ip>:8000` is not a secure context, so it would have
  served a page that looked right and was not isolated.

  This is the local half of the question only. GitHub Pages cannot set either
  header, so the published demos are still not isolated; see `docs/backlog.md`.

- **`apps/horde` steers its crowd on the job pool, and `--workers` is the switch
  that proves it deterministic.** The separation pass — one broadphase
  neighbourhood query per enemy per tick, which is the workload the sample
  exists to produce — now decides every velocity through
  `crcbl_jobs::Pool::par_for` in chunks of 64, and writes them back serially.
  `--workers <N>` sizes the pool; `--workers 0` gives the pool a spawner with no
  threads, which is the shape the browser gets, so the design's `--threads 1`
  versus `--threads N` comparison can be run on one machine. Nothing about the
  game changes: the results are bit-identical at every worker count, because the
  chunk boundaries never depend on it and each enemy's arithmetic — including
  the neighbour sum, whose floating-point order is the BVH's — is untouched by
  which thread ran it.

  Measured on a 32-core machine, 600 headless frames with `--prefill 6000` and
  the null backend: **8.38 s at `--workers 0`, 1.48 s at the default**. There is
  no `cfg(target_arch)` in the sample; `crcbl_jobs::default_spawner` answers the
  browser question, and on `wasm32` the pool has no workers and runs every chunk
  on the calling thread.

  The other four samples were left alone. Breakout's forty bricks, flappy's
  handful of pipes and asteroids' forty-four rocks are smaller than one chunk,
  and sandbox and bare have no per-frame collection at all — a `par_for` over
  any of them would be slower than the loop it replaced.

- **`crcbl-phys` can answer overlap queries under a shared borrow, so several
  threads can ask at once.** `PhysicsWorld::overlap_queries` and
  `PhysicsSystem::overlap_queries` take `&mut self` once, build the broadphase,
  and hand back an `OverlapQueries` / `EntityOverlapQueries` — `Copy`, `Sync`,
  and valid only while the world cannot be mutated, which is the type system
  enforcing what a comment would otherwise have to ask for. Their
  `overlap_sphere_into` takes a caller-owned `QueryScratch` in place of the
  world's own buffers, so a data-parallel pass gives one to each thread and
  still allocates nothing in the steady state. The `&mut self` forms are
  unchanged for callers and now delegate to the same traversal, so there are not
  two implementations to drift apart.

- **The umbrella re-exports the job system as `crcbl::jobs`**, so a game reaches
  `Pool`, `par_for` and `default_spawner` without naming a second workspace path
  — the same arrangement the other nine simulation crates already had.

- **`crcbl-jobs` has a work-stealing pool, and `par_for` works with or without
  threads.** `Pool::new(spawner)` sizes itself to `Spawn::parallelism` minus the
  thread that drives it, `Pool::with_workers(spawner, n)` names a count for a
  caller that knows what else is running, and `Pool::par_for(items, chunk, f)`
  calls `f(start, chunk)` once per fixed-size chunk of a `&mut [T]`. The pool is
  built through the `Spawn` seam rather than `std::thread`, which is what gives
  it a browser story: on a spawner with no threads it has no workers and runs
  every chunk on the calling thread, and **the chunk boundaries come from the
  caller's chunk length and the slice, never from the worker count**, so the
  same call reaches the same closure calls and the same bytes in both modes.

  `par_for` takes `&mut self`, so one thread drives a pool at a time; a
  subsystem that wants its own parallelism builds its own pool. The driving
  thread is a participant rather than a waiter — it runs chunks off its own end
  of the deque while the workers steal from the other — so a call completes even
  if no worker ever wakes, and waking one is throughput rather than correctness.

  **A panicking chunk does not poison the pool**: it is caught where it runs,
  the other chunks still run, and the panic is re-raised on the calling thread
  afterwards. Where several panic, the lowest-numbered chunk's is the one
  re-raised, so the failure reported is the same with and without threads.
  Dropping the pool wakes every parked worker and returns without waiting for
  them, because the seam detaches its threads by design.

  The deque behind it is a **bounded Chase-Lev**, written here rather than
  taken: `crossbeam-deque` is the ecosystem's answer and is not in this
  workspace's lockfile, and adding a dependency is not this crate's call.
  Bounded because growing is what forces epoch-based reclamation; a push the
  queue will not take is run on the spot. Its slots hold pointers in atomics so
  that the speculative read a thief does before it knows the item is its cannot
  be a data race.

- **The seam can now be asked when a frame actually reached the display, and the
  engine asks.** `Device::wait_until_presented(swapchain, present_id, timeout)`
  blocks until a numbered present has completed, `PresentInfo::present_id`
  numbers it, and `Features::PRESENT_FEEDBACK` says whether a device can answer.
  The names are the capability's rather than any one platform's, because the
  three that have it disagree on the shape — one numbers a present and blocks on
  the number, one hands out a waitable object with no number, one only calls
  back once a drawable has been shown — so the id is the caller's currency and
  each backend maps it onto whatever it has.

  **A device without the capability returns `Ok(())` at once rather than
  refusing**, which is what keeps the wait out of every caller's per-frame
  branching: a condition that cannot change after device creation should not be
  re-tested every frame, and a caller that skipped the test would turn a missing
  capability into a failed frame. So does a `present_id` the backend has no
  record of — never presented, or from before the last `reconfigure_swapchain`,
  which restarts the numbering.

  `crcbl::engine`'s `GpuContext::acquire` waits for the present
  `FRAMES_IN_FLIGHT` behind the frame it is about to start, before it takes an
  image and before any work is recorded. Not the frame just submitted: that
  drains the pipeline to a single frame and costs more than not waiting at all.
  `Pacing::Off` waits for nothing, since being paced by the display is the one
  thing that mode exists to avoid, and `PRESENT_WAIT_TIMEOUT` bounds the wait so
  a compositor that stopped answering cannot hang the loop. The frame limiter is
  unchanged and still needed — it answers "am I running faster than the cap",
  which is a different question.

  **`crcbl-vk`, `crcbl-mtl` and `crcbl-dx12` implement it**; `crcbl-wgpu` and
  the null backend still answer immediately and advertise nothing.

- **`crcbl-dx12` presents to a window, and paces on the display while doing
  it.** `Instance::create_surface` finally reads `SurfaceTarget::Win32`'s
  `hwnd`, `Instance::surface_caps` answers from DXGI and the window, and
  `Device::create_swapchain` / `reconfigure_swapchain` / `acquire_next_frame` /
  `present` / `destroy_swapchain` build and drive a
  `DXGI_SWAP_EFFECT_FLIP_DISCARD` swapchain on it. Every other `SurfaceTarget`
  variant is refused by name, and the two kinds of refusal stay apart:
  `Offscreen` names an unwritten slice, while a Wayland, XCB, AppKit or canvas
  target names the backend that owns it, because D3D12's only presentation
  target is an `HWND`.

  **`surface_caps` offers only what `CreateSwapChainForHwnd` will accept.**
  Flip-model takes four back-buffer layouts and rejects everything else, so the
  format list is those four plus the two sRGB spellings — presented the way
  D3D12 requires, as a linear back buffer under an sRGB render target view,
  which is the one differing-format cast this backend permits. Present modes are
  `Fifo` always and `Immediate` **only where the factory reports
  `DXGI_FEATURE_PRESENT_ALLOW_TEARING`**, since a flip-model present with a zero
  sync interval and no tearing flag does not tear and offering it would be a
  mode that does not do what its name says. `Mailbox` and `FifoRelaxed` are
  absent: DXGI has neither. `current_extent` comes from `GetClientRect`, which
  is the only thing on Windows that knows.

  Acquire is the **implicit** shape the seam already documents for `crcbl-wgpu`
  and `crcbl-mtl` — the index comes from `GetCurrentBackBufferIndex`, so both
  semaphores are `None` — and `suboptimal` is always `false` and
  `SurfaceError::OutOfDate` never produced, because DXGI has no such condition
  and inventing one would put a frame loop into an unending reconfigure.

  Present feedback ships in the same change rather than after it, because
  `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT` is a **creation** flag:
  designing the swapchain without it would have meant replacing it immediately.
  `Features::PRESENT_FEEDBACK` is reported for every adapter — `IDXGISwapChain2`
  predates D3D12, so there is no machine where it would have been probed and
  come back no — and `Device::wait_until_presented` blocks on
  `GetFrameLatencyWaitableObject`'s handle. That handle carries **no id**, so
  the backend keeps its own record of the ids it was given and answers the
  seam's immediate cases from it: zero numbers nothing, and an id above the
  highest this swapchain object presented names a frame it was never asked for —
  a present that failed after the caller spent the id, or one from before a
  `reconfigure_swapchain`, where `ResizeBuffers` restarts the numbering.

- **A Vulkan device paces on the display where the driver can say when a frame
  landed.** `crcbl-vk` requests `VK_KHR_present_id` and `VK_KHR_present_wait`,
  chains `VkPresentIdKHR` onto each present and answers
  `Device::wait_until_presented` with `vkWaitForPresentKHR`. The pair is
  optional and asked for only after `vkEnumerateDeviceExtensionProperties` lists
  both and `vkGetPhysicalDeviceFeatures2` returns both feature bits — requesting
  an absent device extension fails `vkCreateDevice` outright — so
  `Features::PRESENT_FEEDBACK` on an `AdapterInfo` or a `DeviceCaps` means the
  device really can answer. It is driver-dependent in practice: radv has the
  pair, lavapipe does not.

  `GpuContextDesc::default()` now asks for `PRESENT_FEEDBACK` among its optional
  features, so a game built on the engine gets the closed loop without naming
  it. A device that does not have it keeps the open-loop frame limiter, exactly
  as before.

  Three cases still answer at once rather than blocking, because
  `vkWaitForPresentKHR` would otherwise sit out the whole timeout for a frame
  that will never arrive: an offscreen image ring, which has no `VkSwapchainKHR`
  at all; an id whose present failed with `OutOfDate` after the caller had
  already spent it; and an id from before a `reconfigure_swapchain`, which
  builds a new swapchain object that never saw it.

- **A Metal device paces on the display too, and every Metal device can.**
  `crcbl-mtl` reports `Features::PRESENT_FEEDBACK` unconditionally and answers
  `Device::wait_until_presented` from `MTLDrawable::addPresentedHandler:` —
  Metal numbers no present and offers nothing to block on, so `present` attaches
  a handler carrying the caller's own `PresentInfo::present_id` and the wait
  sleeps on a condition variable until that number is reported back. The flag is
  unconditional because the handler is a plain drawable method with no query
  behind it; there is no Metal device that cannot answer.

  The flag is on the **device** while the drawable is a property of a
  **swapchain**, so a device driving the offscreen ring advertises it and its
  ring still answers every wait at once, through the seam's own "nothing to wait
  for" case. Withholding the flag instead would make every macOS window
  unpaceable, since the seam then requires an immediate `Ok(())` forever.

  An id the swapchain was never given also answers at once, and a reconfigure
  restarts the numbering: the ledger belongs to the swapchain and a rebuilt one
  starts empty. An id that does not strictly increase is refused and its present
  goes out unnumbered, with a warning, rather than renumbering the swapchain
  backwards.

  Adds a direct dependency on `block2`, the Objective-C block ABI —
  `addPresentedHandler:` takes a block and there is no other way to reach it. It
  is the same binding family as `objc2` and was already in `Cargo.lock`.

- **The Metal backend's hardware suite now runs in CI, so `crcbl-mtl`'s draws
  are verified by a machine rather than by nobody.** A `mtl e2e (macos-latest)`
  job runs `crates/crcbl-mtl/tests/run-mtl-e2e.sh`, which turns on the `mtl-e2e`
  feature and the crate's `#[ignore]`d tests — the triangle draw, the engine's
  own `triangle.slang` draw through a bind group, the indexed draw and the
  multi-draw-indirect. Those four had never been executed anywhere: they were
  gated on the belief that a CI runner's `Apple Paravirtual device` cannot run a
  shader, which was measured on macos-14 and is not true of the image
  `macos-latest` resolves to today. The tests stay `#[ignore]`d, so a plain
  `--all-features` run on a machine without a usable GPU is still green, and the
  script still fails when the suite reports zero tests run.

  One test is held out of the CI job: the layer swapchain's drawable
  acquisition, which depends on a headless container vending a `CAMetalLayer`
  drawable rather than on shader execution. Running the script on a real Mac
  covers it, and covers a non-virtual GPU besides.

- **`crcbl-shaders` ships DXIL, and `crcbl-dx12` draws a triangle with it.** The
  artifact pipeline grew a fourth target: `dxil/<shader>.<entry>.dxil`, compiled
  in two steps — `slangc -target hlsl` then a **pinned** `dxc` at Shader Model
  6.6 — because Slang's own `-target dxil` shells out to whichever `dxc` it
  finds. `CRCBL_DXC` is required with **no `PATH` fallback**: distributions ship
  Shader Model 6.10 preview builds that abort on a trivial shader, and a
  fallback would find one silently. The script verifies the container signature
  of every artifact it generates, because an unsigned container hashes and
  commits like any other and is then refused by every real D3D12 driver.

  DXIL is the one target with an artifact **per entry point** — `dxc` compiles a
  single `-E`, and a D3D12 pipeline takes one blob per stage — so
  `crcbl_shaders::EntryPoint::dxil` and `Shader::dxil(entry_point)` are
  per-entry-point where `Shader::wgsl` and `Shader::msl` are per-shader, and
  `spirv/manifest.txt` records one `dxil` line per entry point beside a
  `dxc-version` and `dxil-model` pin.

  `crcbl-dx12` consumes it: shader modules over validated DXIL containers, root
  signatures from pipeline layouts, bind group layouts and bind groups over a
  shader-visible descriptor heap, `D3D12_GRAPHICS_PIPELINE_STATE_DESC` built
  from the seam's descriptors, and `bind_graphics_pipeline` / `bind_group` /
  `draw` on the encoder. The measurement is a triangle drawn through the real
  seam and read back with its texels asserted, not a call that returned `Ok`.

  Still refused by name: compute pipelines, indexed and indirect draws, index
  buffers, dispatches, query sets, semaphores, swapchains, dynamic offsets and
  push constants — the last two as `InvalidDescriptor` rather than
  `Unsupported`, because a descriptor table has no offset to apply and this
  device reports no `PUSH_CONSTANTS`.

- **`crcbl-shaders`: the UI shaders declare their resources in binding order,
  which is what makes text appear on Metal.** `ui.slang` and `ui_tier_b.slang`
  declared `constants` first while numbering it last, and Slang's Metal target
  assigns argument-table indices in _declaration_ order — so their MSL bound
  `constants` at `buffer(0)` and `vertices` at `buffer(1)`, while `crcbl-mtl`
  flattens `(set, binding)` by ascending binding number and bound them the other
  way round. The UI vertex stage read the viewport constants as its vertex
  array: every quad went nowhere, silently, and macOS ran flappy with no HUD, no
  score and no menu labels. Reordering the two declarations fixes it; SPIR-V and
  WGSL are byte-identical afterwards, because `[[vk::binding]]` already pinned
  those. `crcbl_mtl::binding` carries the rule and the obligation it puts on new
  shaders.

- **The Metal backend is selectable, and on macOS it is what `open()` picks.**
  `crcbl`'s GPU registry grew a `GpuBackend::Metal` entry behind
  `cfg(target_os = "macos")`, so `crcbl-mtl` is finally reachable from a game:
  `--backend mtl` (or `metal`, or `CRCBL_GPU=mtl`) names it, and an ordinary run
  with no flag gets it. This is the wire-up every Metal slice since MTL1
  deferred — a registry entry for a backend that could not yet hand back a
  device would have been a path that exists only to fail — and MTL2 through MTL6
  landed the device, the swapchain, pipelines, bind groups and draws it was
  waiting on.

  **Vulkan is still registered on macOS but is no longer selected automatically
  there.** Apple platforms are Metal only per
  `docs/plan/09-backends-metal-dx12.md`'s 2026-08-05 correction, and a Mac
  without MoltenVK has no `libvulkan.dylib` for `ash` to `dlopen` at all — so
  what the old order produced was not a fallback but the only outcome: every
  sample on macOS exited with "no GPU backend available (tried: vk)" and a hint
  to run the null backend. `CRCBL_GPU=vk` still reaches Vulkan by name for
  whoever installed a loader and means it. Selection elsewhere is unchanged:
  Vulkan on the rest of native, wgpu in a browser, and null never automatic
  anywhere.

- **`crcbl-shaders`**: `COMPUTE_PROBE`, the crate's first **compute** shader —
  every other source it ships is a drawing shader. `shaders/compute_probe.slang`
  squares a `StructuredBuffer<uint>` element-wise into an `RWStructuredBuffer`,
  bounded by a `count` in a uniform buffer, with SPIR-V, WGSL and MSL artifacts
  and a manifest entry like every other shader. The companion
  `crcbl_shaders::compute_probe` module carries `WORKGROUP_SIZE` and the
  `Params` uniform layout, so a caller computing its dispatch size reads the
  number the shader declares rather than one it remembers; a unit test reads the
  `.slang` source and fails if the two drift.

  It exists to make the compute half of `crcbl-hal` testable against a real
  driver — a dispatch that silently does nothing returns `Ok` too — and the MSL
  and WGSL artifacts are emitted even though no Metal or wgpu code path
  dispatches compute yet, because the compile script drives all three targets
  and the manifest hashes all three.

- **`crcbl-mtl`**: a new crate, opening P14 with **the only path to a GPU on
  macOS and iOS** — Apple platforms are Metal only, per the 2026-08-05 platform
  decision, so nothing else reaches a device there. Its first slice is **adapter
  enumeration and nothing else**: `MetalInstance::open` calls
  `MTLCopyAllDevices` and turns every device into an `AdapterInfo` whose
  `DeviceCaps` come from real queries — `argumentBuffersSupport` for
  `DESCRIPTOR_INDEXING`, the `MTLGPUFamily::Metal3` query for
  `BUFFER_DEVICE_ADDRESS`, `supportsBCTextureCompression`, and `maxBufferLength`
  / `maxThreadsPerThreadgroup` / a `supportsTextureSampleCount:` probe for the
  limits Metal will answer before a device exists.

  **Every other entry point refuses by name.** `create_surface`, `surface_caps`
  and `request_device` return `HalError::Unsupported` whose `what` says which
  slice the answer arrives in, so a caller reads "not yet" rather than "broken";
  an out-of-range adapter still gets `NoSuchAdapter`, because that is a caller
  bug this slice can genuinely diagnose and hiding it behind a refusal would
  lose it.

  **It advertises Tier B today, and that is not a claim about Metal.**
  `DeviceCaps::tier` is derived from `Features` precisely so a backend cannot
  assert a tier it has not earned, and `DRAW_INDIRECT_COUNT` /
  `MULTI_DRAW_INDIRECT` wait on the indirect-command-buffer decision the command
  slice makes. The hardware is Tier A; this backend is not yet.

  Off macOS the crate is documentation with no public items, so `objc2-metal` is
  never fetched or built there. Nothing instantiated it at this slice — no app,
  and no entry in the engine's backend selection; the registry entry above is
  what closed that, once there was a device to hand back.

  **Its second slice opens a real device.** `request_device` now checks adapter,
  then required features, then `compatible_surface`, and hands back a
  `PendingDevice` that completes on its first poll. `MetalDevice` implements the
  resource half of the seam — buffers, images, image views and samplers in
  `crcbl-core` `Pool`s, plus `write_buffer`, `queue` and a `wait_idle` that
  really commits a command buffer and waits on it. The instance now keeps its
  `MTLDevice` objects behind an `Arc` shared with every device it opens, which
  is how the seam's "a `Device` outlives its `Instance`" obligation is
  discharged.

  `MemoryLocation` maps to `Private` for `DeviceLocal` and `Shared` for both
  host locations. **`Managed` is deliberately never produced**: it is the
  two-copy mode and both directions need a call this slice does not have, so
  choosing it for readback would return stale bytes on an Intel Mac and correct
  ones on Apple silicon — right on one class of Mac only. `write_buffer` refuses
  `DeviceLocal` by name rather than silently writing nothing, matching what
  `crcbl-vk` answers for the same call.

  All 29 seam formats have an exact `MTLPixelFormat` counterpart, and the
  mapping is tested for **injectivity** — two formats sharing one Metal format
  is invisible at run time (the image is created, the sample succeeds, the
  colour is wrong), and it is the same class of defect as the missing sRGB
  encode that made the browser build render too dark.

  **Its third slice records and submits GPU work, and produces the first
  pixel.** `MetalCommandEncoder` owns one open Metal encoder at a time and
  closes it before opening another, because a second concurrent encoder raises.
  `begin_render_pass` builds a real `MTLRenderPassDescriptor` — colour slots,
  MSAA resolve folded into the store action, the reversed-Z depth clear passed
  through untouched, stencil attached only when the view's format has a stencil
  plane. Copies go through `MTLBlitCommandEncoder`, `submit` takes waits and
  signals, timeline semaphores are `MTLSharedEvent`, and readback is request /
  poll / destroy that genuinely observes command-buffer completion rather than
  assuming it. Draws and dispatches fail the encoder rather than being dropped,
  so `finish` returns the refusal instead of a command buffer that submits and
  draws nothing.

  **`pipeline_barrier` ends the open blit encoder and records nothing else — the
  encoder boundary is the barrier.** Metal tracks hazards automatically between
  encoders for resources whose `hazardTrackingMode` is `Tracked`, which is the
  default for everything allocated straight from an `MTLDevice`, and this
  backend allocates nothing else. What would break that — heaps, parallel render
  encoders, a barrier inside a pass — is written down where the decision is, and
  a test asserts the premise on real objects instead of trusting it.

  A submission may no longer wait on a timeline value that only a _later_
  submission signals. With one queue and no CPU-side signal in the seam that can
  never be satisfied, and its failure mode is a queue that stops with the
  process alive and nothing in any log — so it is refused up front, by name.

  **Its fourth slice compiles shaders and draws.** `create_shader_module` goes
  through `newLibraryWithSource:options:error:` and carries Metal's own
  `NSError` text into `HalError::ShaderCompilation`, because that message is the
  only debugging aid a shader author gets. Graphics and compute pipelines build,
  and a draw paints pixels a test asserts exactly.

  **That draw test does not run in CI, and the reason is the runner rather than
  the code.** GitHub's `macos-latest` exposes an `Apple Paravirtual device` that
  hangs the command buffer on any shader execution — measured, with both
  encoders reporting `completed` rather than faulted. The test is therefore
  feature-gated behind `mtl-e2e` and `#[ignore]`d, run by
  `crates/crcbl-mtl/tests/run-mtl-e2e.sh` on a real Mac. Metal has no software
  rasteriser to substitute the way lavapipe does for Vulkan, so this is a
  coverage gap rather than a workaround; `docs/backlog.md` states it as one.
  Everything short of shader execution — clears, blit copies, semaphores,
  readback, MSL compilation, pipeline-state creation — does run there, and does
  pass.

  **An `MTLRenderPipelineState` is only half of `GraphicsPipelineDesc`.** Cull
  mode, winding, fill mode, depth clip, depth bias, the depth/stencil state and
  the primitive topology are all encoder or draw-call state in Metal rather than
  pipeline state, so they are stored beside the pipeline object and replayed
  when it is bound — otherwise half the descriptor would silently not apply.

  The engine's own `triangle.slang` **compiles into a real pipeline** but is not
  yet drawn: it pulls vertices from a `StructuredBuffer`, which needs bind
  groups, and those are still refused. The pixel test draws a resource-free
  `[[vertex_id]]` triangle instead, generated from the same constant the
  assertion uses so the two cannot drift.

- **`crcbl-mtl`** presents. Its fifth slice adds surfaces over `CAMetalLayer`
  for `SurfaceTarget::AppKit`, an offscreen image ring for
  `SurfaceTarget::Offscreen`, and the whole swapchain half of the seam —
  `surface_caps`, create / reconfigure / destroy, `acquire_next_frame` and
  `present`. **macOS now has a native GPU path from window to pixel**, which
  since the 2026-08-05 platform decision it otherwise did not have at all.

  **The offscreen ring is the half CI can actually run**, and it does: acquire →
  render-pass clear → barrier → blit → submit → present → readback, with the
  exact texels asserted, on the runner's real (if paravirtual) device. Acquiring
  a `CAMetalLayer` drawable needs a display, so that one test is gated behind
  `mtl-e2e` like the triangle.

  **No semaphore is created for WSI, and that is Metal's shape rather than a
  shortcut.** `nextDrawable` blocks the CPU and returns a ready texture, so
  there is no presentation-engine signal to reconcile: `acquire_semaphore` and
  `present_semaphore` are both `None`, the implicit-acquire form the seam
  already documents for `crcbl-wgpu`. Presenting goes through
  `MTLCommandBuffer::presentDrawable:` rather than `MTLDrawable::present`, which
  would hand the drawable over while the GPU may still be writing it.

  A layer is offered `Bgra8UnormSrgb` first and **never the RGBA8 pair** —
  `CAMetalLayer::pixelFormat` raises on RGBA8 — so `preferred_format` lands on
  an sRGB format the layer will actually accept. A test reads the format back
  off the layer and pins it against the conversion table, because the value that
  would make it false is `BGRA8Unorm`: exactly the missing-encode bug that made
  the browser build render too dark.

- **`crcbl-mtl`** binds resources and draws indexed and indirect. Bind group
  layouts, bind groups, `update_bind_group`, pipeline layouts naming them, index
  buffers, `draw_indexed`, `draw_indirect` and `draw_indexed_indirect` are all
  real calls now. **The engine's own `triangle.slang` can finally be drawn** —
  it compiles, builds a pipeline over a layout naming its `StructuredBuffer`,
  binds a real vertex buffer and draws.

  **Bind groups map to flat per-stage argument tables, not argument buffers**,
  and the artifacts decided it: every MSL file `crcbl-shaders` commits declares
  plain arguments (`device Vertex* [[buffer(0)]]`), because Slang's Metal target
  emits no argument-buffer struct. Binding a descriptor block where a shader
  declared a vertex pointer does not fail — the shader reads descriptor words as
  vertex data. So argument buffers were the option that silently draws garbage
  with the shaders that exist. A consequence worth having: directly bound
  resources are made resident and hazard-tracked by Metal itself, so there is no
  `useResource` residency management and MTL3's barrier-is-an-encoder-boundary
  argument stays intact.

  **The backend still reports Tier B, and `DESCRIPTOR_INDEXING` was withdrawn.**
  It had been reported from `argumentBuffersSupport == Tier2` — true of the
  hardware — but flat tables have no runtime-sized array, so the backend refuses
  every `BindingFlags`, and the seam says a backend that refuses them must not
  claim the feature. `MULTI_DRAW_INDIRECT` and `INDIRECT_FIRST_INSTANCE` were
  earned. `DRAW_INDIRECT_COUNT` remains unreachable while the backend encodes
  straight into the command buffer, because Metal's only GPU-count execution
  needs an indirect command buffer populated by a compute pass that would have
  to run before the render encoder exists. Nothing above the seam is affected —
  every layout in `crcbl-render` already uses `BindingFlags::empty()`.

- **`crcbl-dx12`**: a new crate, opening the DX12 half of P14. This first slice
  is **adapter enumeration and nothing else** — surfaces, surface caps and
  device creation refuse by name, while an out-of-range adapter still gets
  `NoSuchAdapter`.

  **D3D12 has no adapter-level capability query.** `CheckFeatureSupport` lives
  on `ID3D12Device` and there is no physical-device object, so enumeration opens
  a device per adapter at feature level 11.0, asks, and drops it. An adapter
  DXGI lists but D3D12 refuses is dropped — and the id counter advances only on
  a kept adapter, or every id past the gap would name the wrong GPU.

  **It exists to settle whether WARP clears Tier A.** WARP is D3D12's software
  rasteriser and ships in Windows; `windows-latest` currently has no GPU at all,
  so Windows has never had golden images or render coverage. Each adapter prints
  its `ResourceBindingTier`, `HighestShaderModel` and SM6.6 dynamic-resource
  answer, and a CI step publishes the line — nextest hides a passing test's
  stdout, which is exactly the run where a measurement wants reading.

  `DESCRIPTOR_INDEXING` is reported from tier 3 **and** shader model 6.6, both
  required and neither implying the other. The indirect features are withheld
  despite `ExecuteIndirect` being a direct fit, because no call in the crate
  makes them true yet — the precedent `crcbl-mtl` set by withdrawing a flag it
  could not honour.

  **Its second slice opens a real device.** `request_device` now checks adapter,
  then required features, then `compatible_surface`, and hands back a
  `PendingDevice` that completes on its first poll; behind it are a real
  `ID3D12Device` and a `D3D12_COMMAND_LIST_TYPE_DIRECT` queue. `Dx12Device`
  implements the resource half of the seam — buffers, images, image views and
  samplers in `crcbl-core` `Pool`s, plus `write_buffer`, `queue` and a
  `wait_idle` that signals an `ID3D12Fence` and blocks on it. The instance now
  keeps its DXGI factory and adapters behind an `Arc` shared with every device
  it opens, which is how the seam's "a `Device` outlives its `Instance`"
  obligation is discharged.

  **An image view is a descriptor, not an object**, so the crate gained a small
  allocator over CPU-visible descriptor heaps — one per D3D12 heap type, grown a
  chunk at a time. One seam view becomes up to four descriptors, because a
  texture that is sampled and rendered to needs an SRV _and_ an RTV; the image's
  `ImageUsage` decides which. Every combination D3D12 has no member for — a
  depth stencil view of a volume, a multisampled UAV, a cube whose layers are
  not whole cubes, a view whose dimensionality is not its image's — is refused
  with `InvalidDescriptor` naming it, because `CreateShaderResourceView` and its
  three siblings return `void` and cannot report anything.

  **`ImageViewDesc::format` must equal its image's format on this backend**,
  which is a documented divergence from `crcbl-mtl`: D3D12 permits the sRGB
  reinterpretation the seam describes only from a typeless resource, or where an
  optional casting capability is reported, and neither is worth making the
  seam's promise machine-dependent or every render target uncompressed. A
  **sampled depth** image is not that case and does work: it is stored typeless
  (`R32_TYPELESS` and friends) with the depth-stencil view and the shader view
  each naming their own concrete format.

  `MemoryLocation` maps to D3D12's three standard heaps, and each gets the only
  initial resource state D3D12 accepts for it. An image on a host-visible heap
  is refused — those heaps hold buffers only — as is `write_buffer` on a
  `DeviceLocal` buffer, which D3D12 reaches through a copy rather than a map.

  Every seam format has an exact `DXGI_FORMAT` and the mapping is tested for
  injectivity, as is the separate typeless/depth-read table: two seam formats
  collapsing onto one API format is invisible at run time — the image is
  created, the sample succeeds, the colour is wrong.

  `TEXTURE_COMPRESSION_BC` and `SAMPLER_ANISOTROPY` are now reported, each
  because the call behind it landed: BC support is measured with a real
  `CheckFeatureSupport(D3D12_FEATURE_FORMAT_SUPPORT)` per BC format, and
  `max_sampler_anisotropy` moves to `D3D12_REQ_MAXANISOTROPY`. The tier is still
  B on every adapter.

  **Its third slice records, submits and clears.** `Dx12CommandEncoder` is a
  real `ID3D12GraphicsCommandList` over its own `ID3D12CommandAllocator`, taken
  when the encoder is created so a queue handle from another device is a
  `ForeignObject` the encoder carries to `finish`. `begin_render_pass` binds
  attachments with `OMSetRenderTargets` and honours `LoadOp::Clear` through
  `ClearRenderTargetView`/`ClearDepthStencilView`, with viewport and scissor set
  from the pass's render area; `Device::submit` runs the lists on the queue and
  signals an `ID3D12Fence`; `request_readback`/`poll_readback` observe that
  fence and map the buffer. So a cleared pixel is now written, copied and **read
  back and asserted** rather than assumed — which is the measurement
  `docs/backlog.md` asked for about whether WARP can execute anything at all, as
  opposed to merely reporting `ResourceBindingTier=3`.

  **A clear honours `RenderPassDesc::render_area`, unlike `crcbl-mtl`.** D3D12's
  clears take a rectangle list, so the area is passed through — Vulkan's
  semantic; a Metal `loadAction` clears the whole attachment whatever the pass
  said. `StoreOp::Discard` is honoured as `Store`: `OMSetRenderTargets` has no
  store op, and storing when the caller did not need it is slower and never
  wrong.

  **`destroy_*` freeing a resource with work in flight is no longer a
  use-after-free.** A D3D12 command list retains nothing it references, so the
  encoder now takes its own reference to every resource it records against and a
  submission parks that set on a fence-keyed retire queue — along with the
  command list and allocator, which `ExecuteCommandLists` does not retain
  either. `destroy_buffer` and its siblings are unchanged and still free on the
  spot, because the reference keeping the resource alive is the submission's.
  That is a smaller mechanism than `crcbl-vk`'s deletion queue needs, and the
  reason is that COM refcounts the bookkeeping Vulkan handles cannot.

  `pipeline_barrier` becomes `ResourceBarrier` transitions, per subresource or
  whole-resource; a barrier on a host-visible buffer is dropped rather than
  recorded, because D3D12 pins upload and readback resources to one state for
  their lifetime. Buffer↔buffer and buffer↔image copies are recorded, with
  D3D12's 256-byte row pitch and 512-byte placement alignments refused by name —
  neither is expressible in `BufferImageCopy`, and `CopyTextureRegion` returns
  `void`, so an unaligned footprint would arrive as a readback of the wrong
  bytes. Draws, dispatches, bind groups, push constants, index buffers, buffer
  fills, image-to-image copies, MSAA resolves and read-only depth attachments
  all **fail the encoder**, so `finish` returns the refusal rather than a
  command buffer that submits and does nothing. Semaphore waits and signals on a
  submission, and `ReadbackDesc::after`, are `InvalidHandle` — no semaphore
  exists to have issued one.

  Everything past that — pipelines, bind groups, shader modules, swapchains and
  queries — still refuses by name.

- **`crcbl-shaders`** now emits **MSL** beside the SPIR-V and WGSL. Slang's
  `-target metal` output is committed as `msl/*.metal`, hashed into
  `spirv/manifest.txt` exactly like the other two, verified by `build.rs` on
  every machine and byte-recompiled by the `shaders` CI job. `Shader::msl()`
  joins `.spirv()` and `.wgsl()`. Regenerating left every existing `.spv` and
  `.wgsl` byte-identical, which is independent evidence the pinned `slangc` is
  the one the artifacts were built with.

- **`crcbl-hal`**: `ShaderModuleDesc` gained `msl`, and `ShaderSources` gained
  `MSL`. A backend that can only compile one language now reports the gap by
  name, so an MSL-only descriptor handed to `crcbl-vk` says so rather than
  failing obscurely. Every call site in `crcbl-render`, `crcbl-vk`, the null
  backend and the seam suites was updated in the same change.

- **`crcbl-jobs`**: a new crate, opening P5B with **the seam every engine thread
  will start through**. `Spawn` has three methods — `threaded`, `parallelism`
  and `spawn` — with two backends behind it: `Threads` over `std::thread`, and
  `Inline`, which has none and refuses every spawn by name. `default_spawner`
  picks between them and is the only place in the threading model that spells
  `cfg(target_arch)`.

  **It exists because `std::thread::spawn` compiles on `wasm32-unknown-unknown`
  and fails at run time.** `std`'s wasm-with-atomics arm takes only `sleep` from
  `thread/wasm.rs`; `Thread::new` comes from `thread/unsupported.rs` and returns
  `UNSUPPORTED_PLATFORM`, because a wasm module cannot instantiate its own
  worker — only the host can, against the shared `WebAssembly.Memory`. A pool
  written on `std::thread` would therefore compile for the browser and have no
  browser story, so the seam lands before anything is built on it. `Threads` is
  not nameable on `wasm32` at all, which makes reaching for it there a compile
  error rather than a run-time `Err`.

  **Degrading is a decision, not an error**: `Spawn::threaded` is asked once
  while a subsystem is being built, and a caller picks a long-lived thread or a
  tick-driven loop from the answer. A `spawn` that fails afterwards is a real
  error, and the closure is gone by then either way.

  Not here yet: the SPSC rings, the work-stealing pool and `par_for` — the
  slices above this one — and the browser's worker backend.

- **`crcbl-jobs`**: `mailbox` — the latest-wins triple buffer a _state_ crosses
  a thread boundary through. One producer publishes complete states at its own
  cadence, one consumer takes the newest, and neither ever waits: a slow
  producer publishes less often and a slow consumer skips the states in between.
  `Publisher::publish` swaps an index rather than copying the payload, and
  `Subscriber::read` always returns a whole state — never an `Option`, because a
  frame drawn from a state one tick old is the outcome this design prefers to a
  frame that waited. `Subscriber::has_new` is the staleness the profiler will
  report.

  Three slots, because that is the count at which neither side ever waits: one
  the producer owns, one the consumer owns, one in the handoff. The `unsafe`
  rests on `{producer, handoff, consumer}` staying a permutation of `{0, 1, 2}`
  — both sides only ever exchange their own index with the handoff's — and the
  tests assert that permutation directly after every operation rather than
  arguing for it. `crcbl-jobs` joins the weekly Miri job, which runs the
  two-thread stress test for real: a torn read is reported there as a data race,
  and nothing else in this workspace can detect one.

  **Deliberately not for streams.** Input edges, audio commands and net packets
  must not be droppable, and this drops by construction — a 3 ms tap between two
  reads would simply not be there. Those want the ring below.

- **`crcbl-jobs`**: `ring` — the bounded SPSC queue a _stream_ crosses a thread
  boundary through, and the opposite discipline to the mailbox. Every item is
  delivered, in order; a producer that outruns its consumer is refused rather
  than allowed to overwrite. `Producer::push` hands the item **back** in a
  `Full<T>` rather than dropping it, so shedding load is always the caller's
  decision and never a silent one, and `Consumer::overflows` counts the refusals
  for the profiler. Capacity rounds up to a power of two so the index wrap is a
  mask.

  **Drop-oldest is not implemented**, though the design lists it as a policy: it
  cannot be done from the producer, because the read cursor belongs to the
  consumer and advancing it would make the producer a second writer to it —
  which is what makes an SPSC ring cheap in the first place. Documented at the
  module and recorded in `docs/backlog.md` rather than left to be discovered.

  Both primitives run under the weekly Miri job. **The memory orderings are
  checked by Miri and by nothing else**, and that is the hardware's doing rather
  than the suite's: on x86-64 a `Release` store and a `Relaxed` one compile to
  the same instruction. Measured — weakening the ring's push to `Relaxed` left
  the whole suite green while Miri reported the data race in `pop`.

- **horde**: **health potions, dropped by brutes and drunk by walking over
  them.** A `potion` frame in `apps/horde/assets/actors.crpix` — a stoppered
  flask in a crimson that appears nowhere else in the sheet, drawn to the same
  14-texel collider the gem is — and a `game::PickupKind` that says what walking
  over a thing pays out. A potion is a **variant of the existing pickup**, not a
  second population: the same `Vec`, the same entity index, the same
  `MAX_PICKUPS` ceiling, the same trigger collider and the same collection
  query, so the soak test's two exact leak equalities and its entity growth
  bound are unchanged rather than each gaining a term.

  **Brutes only, and one brute in twenty.** The brute does more contact damage
  than the other two kinds together and is the one slow enough to walk away
  from, so the heal is paid out by the fight that cost the hit points — the same
  argument `EnemyKind::xp` already makes for experience. The rate came off a
  measurement rather than a feel: at one brute in three the kiting soak
  (`a_long_run_leaks_nothing`) stopped reaching a death at all, which is contact
  damage ceasing to be the pressure the genre is made of. Over the first hundred
  seconds of the default seed it is now **2 potions from 219 kills**, and
  `potions_drop_from_brutes_at_the_rate_the_constant_says` is where that is
  measured.

  **The roll is simulation state.** `game::drops_potion` hashes the run's kill
  counter under a `LOOT_HAND` salt on the run seed — the same construction the
  prop scatter uses, for the same reason — so a drop is identical in a replay,
  on a server and on a client. `the_same_script_replays_bit_identically` now
  compares the loot on the ground and the potion count as well.

  Healing clamps to `Stats::max_hp` and **never to `PLAYER_MAX_HP`**, through a
  new `heal_player` that `Upgrade::Vitality` now shares: the ceiling moves when
  a run takes that upgrade, and a heal clamped to the constant would stop paying
  out with nothing on screen to show it. A potion is worth a quarter of the
  starting bar (`POTION_HEAL`), which is a couple of seconds inside the mass and
  under one inside a brute.

  A **sixth spatial cue**, `audio::SOUND_HEAL`, rather than a second use of the
  gem's: a gem sounds for very nearly every kill and a potion for about one kill
  in a hundred, and the rarest event in the game played through the most common
  sound is the same as not playing it.

  `game::XP_RADIUS` is renamed **`game::LOOT_RADIUS`**, since it is now the
  collider of both pickups; `RenderState`'s `PickupView` carries a `kind`. The
  batch count is unchanged — the potion is another frame of the one actors
  sheet.

- **horde**: **trees and bushes scattered over the arena, and the player cannot
  walk through them.** A new `apps/horde/assets/props.crpix` — one 36-texel
  frame size, a 0.9-unit tree and a 0.5-unit bush, each drawn to its own
  collider to the texel — on a sheet and a layer of its own between the grass
  and everything that moves. `game::scatter_props` deals them from a jittered
  lattice as a pure function of the game's seed, so two games built from one
  `Setup` stand in the same arena on every machine and in a replay.

  **The collision is the player's alone**: enemies walk through props and bolts
  fly through them, which is `docs/plan/sample/03-horde.md`'s hard cap on
  pathfinding doing its job — a prop the horde had to route around would be an
  obstacle query per enemy per tick on the one loop this sample exists to keep
  flat. So a prop is a `game::PropView` in a plain `Vec` with no entity and no
  collider, and the soak test's two exact leak equalities are unchanged.
  `game::push_out_of_props` moves the player to the nearest point on the prop's
  surface, so walking into a trunk off-centre slides round it rather than
  sticking, and it runs beside the arena clamp — props first, then the wall,
  which is the order that terminates. The lattice keeps every prop far enough
  from a wall that the clamp can never hand the player back to one, and far
  enough from the spawn that a run never starts inside a tree; both are `const`
  assertions as well as tests.

  `RenderState` carries `props`, `SceneStats` gained a `props` count beside
  `ground`, and a populated frame is **four** batches instead of three. The
  claim that number exists to make visible is unchanged and is the one it always
  was: the batch count is flat in the size of the horde, not any particular
  value.

- **horde**: the three enemy kinds are **Diablo II monsters**. The frames in
  `apps/horde/assets/actors.crpix` are renamed and redrawn — `grunt` → `fallen`
  (a horned, hunched imp with a crude bone blade), `runner` → `quill-rat` (a low
  wide body under a fan of spines, on four thin legs) and `brute` → `overlord`
  (a head sunk between two shoulder masses, lit brow, tusks) — and
  `art::enemy_frame` is where a kind is mapped to one. **`EnemyKind`'s variants
  are unchanged**: `Grunt`, `Runner` and `Brute` name the roles the spawn table
  and `EnemyKind::from_roll` reason about, and nothing in `game.rs` moved. Each
  silhouette is still drawn to its own collider to the texel, which is why the
  runner became a quadruped: thirteen texels does not carry a humanoid.

  The palette that came with them is muted earth and blood — three shades of
  blood, three of hide, three of dead skin and one bone — and two new tests say
  where it has to sit.
  `art::tests::the_monsters_sit_between_the_grass_and_the_player_in_luma` puts
  every kind's average texel above the brightest texel of `assets/terrain.crpix`
  and every monster texel below the player's average, and
  `art::tests::the_monsters_have_a_dark_rim_and_the_player_a_bright_one` finds
  the boundary of each silhouette and asserts the dark-rim/bright-rim asymmetry
  in both directions rather than leaving it a sentence in the sheet.

- **horde**: the player is a **wizard**, and it moves like one. Five new frames
  in `apps/horde/assets/actors.crpix` — a standing pose and a four-frame walk
  cycle held four ticks a frame, played from `RenderState::elapsed` so a replay
  animates the way the run it replays did. The wizard **faces the way the input
  last pointed**, not the way the gun is aiming and not the way the arena clamp
  left it moving: `RenderState` carries `player_facing` and `player_walking`,
  and a released key leaves the facing where it was rather than snapping back.
  Facing left is the same art with its `u` range reversed, so there is no second
  column of frames.

  **Bolts leave the head of the staff.** `game::MUZZLE_OFFSET` — a distance
  along the aim — is replaced by `game::STAFF_MUZZLE` and `game::staff_muzzle`,
  a point that mirrors with the figure;
  `art::tests::the_staff_head_is_where_the_muzzle_says_it_is` measures the baked
  art against it, so the orb and the shot cannot drift apart. The gun still
  _chooses_ its target from the player's centre and now _aims_ from the staff,
  which is what stops a shot fired from an offset muzzle travelling parallel to
  the line that would have hit. When the wizard is facing away from its target
  the bolt still starts at the drawn staff and crosses the body; that choice is
  written down on `staff_muzzle`.

  The batch count is unchanged — every frame of the wizard is a frame of the one
  actors sheet — and the hold guard in `apps/horde/build.rs` and `art.rs` is
  real for the first time, because until now no horde clip held a frame for
  longer than the one tick that survives almost any wrong tick↔millisecond
  arithmetic.

- **horde**: a **tiled grass ground** under the field, from a new
  `apps/horde/assets/terrain.crpix` — four 2-unit variants chosen per tile by
  `crcbl::core::rand`'s index hash, so a tile draws the same grass whichever way
  it is walked into and the ground has no visible lattice. Laid over the view
  rather than the arena, so its cost is bounded by the window. The sample's art
  is three sheets now instead of two; `SceneStats` gained a `ground` count and
  reports three batches on a populated frame instead of two. The claim that
  number exists to make visible is unchanged and is stated as what it always
  was: the batch count is **flat in the size of the horde**, not any particular
  value.

- **crcbl-shell**: an **AppKit end-to-end pass**, so macOS is held to the
  standard the other three backends already were. It extends
  `crates/crcbl-shell/tests/appkit_session.rs` — the `harness = false` target
  that exists because `libtest` runs every body on a thread it spawns and AppKit
  raises off the main thread — rather than adding a second one, because two
  processes each bootstrapping an `NSApplication` would fight over which is
  frontmost and injected input follows whichever wins.

  **Input the window system generated**, through `CGEventPost`: a key press and
  its release, an arrow key, pointer motion, a click and a wheel notch. That is
  what reaches `interpretKeyEvents:`, which nothing had ever reached — so
  `ShellEvent::TextCommit` on macOS was in exactly the state the Win32 backend's
  was in before its own e2e suite found `TranslateMessage` missing from the
  pump. It also observes the asymmetry `appkit::pointer` exists to describe: a
  cursor moved down the screen comes back with a _larger_ window Y and a
  _positive_ raw delta, because `locationInWindow` is Y-up and Quartz's delta is
  not.

  **A pasteboard round trip against `pbcopy` and `pbpaste`**, in both directions
  — Apple's own processes, with no `crcbl-shell` in them, which is what
  separates "the pasteboard server has the bytes" from "the shell answered its
  own read out of a cache". A helper binary of ours was considered and declined;
  `docs/backlog.md` records that this covers text only, since `pbpaste` cannot
  be asked for the engine's own format.

  **AppKit as the judge** rather than the backend's own bookkeeping, through
  three new `crcbl_shell::session_support` entry points — `window_facts`,
  `key_window` and `resize_window` — and `activation`, which now takes the title
  of the window to describe. Three of the five switches `appkit::view` lists as
  "structural rather than verified" are now read back off the live window —
  `acceptsMouseMovedEvents`, the first responder being `CrcblView` rather than
  the window, and the registered dragged types — and a resize AppKit performed,
  a borderless flip that covers the `NSScreen` it names exactly, and the
  restored title bar are all checked against `NSWindow` and `NSScreen`.

  **None of that readback goes through `-[NSApp keyWindow]` any more**, which is
  the correction the first macOS run forced. That run reported
  `app_active: false` with `can_become_key: true`: a GitHub runner gives an
  unbundled binary a window server and a window but not activation, so the key
  window was nil and every assertion behind it was being discarded over a
  precondition it did not have. `window_facts` finds this process's own window
  by title among `-[NSApp windows]`, and reports `app_active` and `is_key` as
  fields rather than requiring them; `key_window` remains for the one caller
  that genuinely needs the keyboard, which is `CGEventPost`. The harness then
  asks the session for activation itself —
  `-[NSRunningApplication activateWithOptions:]`, which reaches a lever the
  backend is right not to have, since a game does not get to steal the focus —
  and **the runner grants it**, so the window becomes key and the injected input
  runs. If it is ever refused, the injected-input assertions and the warp
  readback are skipped with a printed account of what did not run and why,
  rather than failing the session or going quietly green.

  **A warp is not an event**, which the same run found:
  `CGWarpMouseCursorPosition` moves the cursor and posts nothing, so reading a
  warp back needs a real `kCGEventMouseMoved` posted at the point the cursor was
  moved to. That makes the check stronger than it was — the seam's conversion
  into Quartz's global space and the backend's conversion out of
  `locationInWindow` are now judged against each other, rather than one of them
  against a tracking-area crossing that a boundary-crossing warp happened to
  produce.

  **And a synthesized mouse event carries no delta unless the poster sets one.**
  `CGEventCreateMouseEvent` leaves `kCGMouseEventDeltaX`/`Y` at zero and
  `-[NSEvent deltaX]` reads exactly those, so `raw_delta` came back `(0.0, 0.0)`
  — correctly. The harness now writes a known delta onto the event, so the seam
  is held to reporting _that_ pair rather than merely something non-zero, and
  the asymmetry `appkit::pointer` exists to describe is observed for the first
  time: a move right and **up** comes back with a larger window X, a smaller
  window Y, and a delta whose Y is still negative, because `locationInWindow` is
  flipped into the seam's space and Quartz's delta is already in it.

  **`ShellEvent::TextCommit` from a real keystroke now has executable coverage
  on macOS.** The injected `kVK_ANSI_A` reaches `interpretKeyEvents:` through
  `sendEvent:` and the first responder, and commits `"a"` — the chain that was
  written blind and is the macOS counterpart of the `TranslateMessage` gap the
  Win32 backend shipped with. That also settles the risk the slice was written
  around: **TCC does not gate `CGEventPost` for events delivered back to the
  posting process.**

  **And the scroll notch reaches the event it is posted on.**
  `CGEventCreateScrollWheelEvent`'s `wheel1` is a _named_ parameter — only
  `wheel2` and `wheel3` are variadic — and the harness had declared the `...`
  one parameter early, so on Apple silicon the amount went to the stack while
  the callee read a register and the event scrolled by zero. The same class of
  defect `appkit::ffi` guards against for `objc_msgSend`, arriving through a
  hand-written C variadic instead.

  **The sample-level pass has no macOS equivalent**, on the same terms as
  Windows: it needs a renderer and macOS has no Vulkan until MoltenVK clears its
  P14 gate. `docs/plan/ROADMAP.md`'s 2026-08-04 correction says so, and
  `docs/backlog.md` carries it as a gap rather than approximating it.

- **crcbl-shell**: **the clipboard and file drops on the AppKit backend**, so
  `ShellCaps::CLIPBOARD` and `ShellCaps::DRAG_DROP` are set there and
  `clipboard_offer`/`clipboard_request` answer instead of returning
  `Unsupported`. macOS is now the fourth backend to implement the whole seam.

  A copy publishes every offered format at once under its own `NSPasteboard`
  type: text under `public.utf8-plain-text`, which is what TextEdit and every
  other application reads, and the engine's own `application/x-crcbl+ron` under
  that mime string verbatim — the same spelling the other three backends use, so
  an engine-to-engine copy is lossless and byte-identical across platforms. An
  empty offer slice **clears** the pasteboard, because macOS has no owner to
  release: a pasteboard is content the server holds. Reads answer the three
  `ClipboardContent` outcomes distinctly, and the answer names the format that
  was _asked_ for — a pasteboard type is a UTI rather than a mime, so there is
  no peer spelling to report.

  Nothing is provided lazily and nothing is held after a write:
  `setData:forType:` copies the bytes to the pasteboard server, so this backend
  carries no deadline, no retry budget and no state between pumps — the only one
  of the four whose clipboard needs none of them.
  `pasteboard:provideDataForType:` is refused for the same structural reason the
  Win32 backend refuses `WM_RENDERFORMAT`, and `docs/backlog.md` says not to
  revisit it without a seam change.

  File drops arrive through `registerForDraggedTypes:` and the
  `NSDraggingDestination` methods on the content view, honouring
  `WindowDesc::accept_drops` — and there the gate is the **system's**: AppKit
  sends no dragging message at all to a view that has not registered, which is
  the same strength as Win32's `WS_EX_ACCEPTFILES` and stronger than Wayland's.
  Each `public.file-url` goes through the shared `parse_uri_list`, so a
  percent-encoded name, a `file://localhost/…` authority and a filename that is
  not valid UTF-8 all behave exactly as they do on the other backends, and a
  dragged _URL_ is not turned into a path that looks plausible and does not
  exist. Promised files (`com.apple.pasteboard.promised-file-url`) are not
  accepted; the seam has no way to name a destination for one.

- **crcbl-shell**: **input on the AppKit backend** — keyboard, text, pointer,
  scroll, relative motion, pointer lock, cursors and warping, so a game is
  playable on macOS rather than merely windowed. `POINTER_LOCK`, `POINTER_WARP`,
  `RAW_POINTER_MOTION` and `TEXT_IME` join the capability set, and
  `ShellCaps::has_mouselook()` is true there.

  Keys carry Apple's `kVK_*` codes mapped to `KeyCode` (a third numbering, which
  coincides with neither evdev nor PS/2 set 1 at any point), an X11 keysym, the
  auto-repeat flag and the modifiers of that event. **Four keys the seam names
  are unreachable on macOS** — `PrintScreen`, `ScrollLock`, `Pause` and
  `ContextMenu` have no `kVK_*` code, and those positions on a Mac keyboard are
  `F13`–`F15`, which are their own keys. **Num Lock is not a modifier there**:
  macOS has no such latch, and `NSEventModifierFlagNumericPad` means "this key
  is on the keypad", so `Modifiers::NUM_LOCK` is never set. **Option is reported
  as `ALT` and never `ALT_GR`**, because the same key is macOS's Alt and its
  level-3 shift and no third key distinguishes them — the opposite conclusion
  the Win32 backend reaches, from the same starting point.

  Text goes through a real `NSTextInputClient` and `interpretKeyEvents:`, so
  commits arrive from the **input method** and dead keys compose — reading
  `-[NSEvent characters]` instead would leave every accented character
  unreachable. Pre-edit is tracked and never surfaced (the seam has no event for
  one), so an input method's candidate window appears at the window's origin
  rather than under a caret.

  The pointer reports both scroll units — a trackpad's `ScrollDelta::Pixels` and
  a wheel's `Lines`, the first backend where both arms are reachable — buttons
  past the fifth through `otherMouseDown:`, and enter/leave from an
  `NSTrackingArea`. `PointerMode::Locked` freezes the cursor with
  `CGAssociateMouseAndMouseCursorPosition(false)` and needs none of the
  clip-and-recentre machinery Win32 and X11 carry.

  Two things a consumer must know. **`PointerMode::Confined` is refused,
  permanently**: macOS has no confine API, only warping the cursor back after it
  has already left, so `POINTER_CONFINE` stays clear — the only desktop backend
  where the two capture modes come apart. And **`RAW_POINTER_MOTION` here is
  unclamped but _accelerated_**: `NSEvent`'s deltas are separate from the
  absolute position and keep flowing at the screen edge, which is what makes a
  camera work, but macOS publishes no way to remove the system's pointer
  acceleration from them.

- **crcbl-shell**: an **AppKit backend**, registered and selected automatically
  on macOS — so `crcbl_shell::open()` now returns a real window there instead of
  `NoBackend`. The window lifecycle: `NSApplication` bootstrap, create, show,
  hide, destroy, title, close-request interception (`windowShouldClose:` answers
  `NO`, and the seam asks), windowed ↔ borderless on a **named** display with
  the windowed style mask and frame restored exactly, size constraints through
  `setContentMinSize:`/`setContentMaxSize:`/`setContentAspectRatio:`, `NSScreen`
  enumeration with visible frame, backing scale, refresh rate and hotplug, an
  event pump and a blocking `wait_events`, and `SurfaceTarget::AppKit` for the
  HAL. Built on hand-written Objective-C runtime FFI — `objc_getClass`,
  `sel_registerName`, `objc_msgSend` and runtime-built classes — with no `objc2`
  and no `cocoa`.

  The shell creates and owns the `CAMetalLayer` and hosts it on its `NSView`, so
  `SurfaceTarget::AppKit` carries the layer and **no HAL backend ever touches
  AppKit**. Borderless is a frameless window at the display's size, not
  `toggleFullScreen:`: the desktop's mode is untouched and there is no Spaces
  transition. `ASPECT_HINT_HONORED`, `WINDOW_POSITION`, `SERVER_DECORATIONS`,
  `MULTI_WINDOW` and `EVENT_WAIT` are set; the pasteboard and drag-and-drop are
  the slice after this one and every bit they would set stays clear.

  Four macOS facts a consumer may need. **`AppKitShell::open` requires the
  process's main thread** and returns `ShellError::Backend` naming that rule
  anywhere else — AppKit raises an Objective-C exception otherwise, which
  unwinding into Rust is undefined behaviour. **`FRACTIONAL_SCALE` is clear**,
  because `backingScaleFactor` is 1.0 or 2.0 and a "scaled" HiDPI mode changes
  the point resolution rather than the factor. **`MonitorInfo::bounds` does not
  tile** across displays of different scales, because AppKit's global coordinate
  space is points rather than pixels — the caveat that field already documents
  for Wayland, now true on a second platform; window placement is unaffected,
  because it is expressed in points. And `MonitorInfo::refresh_millihertz` can
  finally be non-integral: `CGDisplayModeGetRefreshRate` reports 59.94 as 59.94,
  which no other backend's API is able to.

- **crcbl-shell**: a **Win32 backend**, registered and selected automatically on
  Windows — so `crcbl_shell::open()` now returns a real window there instead of
  `NoBackend`. The window lifecycle: create, show, hide, destroy, title,
  close-request interception, windowed ↔ borderless on a named monitor with the
  windowed placement restored exactly, size constraints (`WM_GETMINMAXINFO`
  limits and a live `WM_SIZING` aspect lock), monitor enumeration with work
  area, refresh rate and per-monitor DPI, per-monitor-v2 DPI awareness with
  `WM_DPICHANGED` handled mid-session, a message pump, a blocking `wait_events`,
  and `SurfaceTarget::Win32` for the HAL. Built on hand-written
  `extern "system"` declarations for `user32`, `gdi32`, `shcore` and `kernel32`
  — there is no `windows-rs` and no `winapi`.

- **crcbl-shell** (Win32): **input**. Keyboard events carry a PS/2 set-1 scan
  code with its `E0` prefix folded in, the `KeyCode` for that physical position,
  the layout's `Keysym`, the modifiers and the auto-repeat flag; `WM_CHAR`
  becomes `TextCommit`, with surrogate pairs reassembled so an astral codepoint
  arrives whole and control characters dropped. The pump calls
  `TranslateMessage`, which is what makes a `WM_CHAR` exist at all — dead keys,
  AltGr and an input method's commit all arrive through it, and without it
  typing into a Crucible window produced no text whatever. Pointer motion, all
  five buttons including the two thumb buttons, derived enter and real leave,
  mouse capture so a button released outside the window is still reported, and
  both wheel axes with high-resolution fractions of a detent preserved.
  `WM_INPUT` raw relative motion, with an absolute-reporting device — a
  remote-desktop session, a tablet — differenced into a delta instead of being
  read as one. `PointerMode::Confined` and `PointerMode::Locked` through
  `ClipCursor`, and `warp_pointer` through `SetCursorPos`. Cursor shapes are the
  stock `IDC_*` set applied from `WM_SETCURSOR`, and hiding goes through a
  balanced `ShowCursor` count.

  A confined pointer's clip is the client rectangle **intersected with the
  virtual screen**: `ClipCursor` clamps, so a window larger than the desktop is
  confined to the part of itself that is on screen.

  `POINTER_LOCK`, `POINTER_CONFINE`, `POINTER_WARP` and `RAW_POINTER_MOTION` are
  now set on this backend — the last of them latched on whether
  `RegisterRawInputDevices` was accepted — and `set_cursor` applies rather than
  records. **`TEXT_IME` stays clear**: nothing here touches `WM_IME_*`, so there
  is no composition string and no candidate-window placement, and typing working
  through `WM_CHAR` is not the same claim.

  Three Windows facts worth knowing before building on it: a window frozen
  during a user drag-resize is the system's modal message loop and not a hang; a
  monitor's refresh rate is a whole hertz here, so 59.94 Hz reports as 60; and a
  `DeviceId` names a device _kind_ rather than a device, so two mice cannot be
  told apart yet.

- **crcbl-shell** (Win32): **the clipboard and file drops**, so `CLIPBOARD` and
  `DRAG_DROP` are now set and `clipboard_offer`/`clipboard_request` work instead
  of returning `Unsupported`.

  A write publishes each offered format at once — `CF_UNICODETEXT` for
  `text/plain;charset=utf-8`, and a `RegisterClipboardFormatW` format named
  after the mime for everything else — so one copy reaches Notepad as text _and_
  round-trips through another Crucible as `application/x-crcbl+ron` without
  loss. The reader picks. Windows synthesizes `CF_TEXT` and `CF_OEMTEXT` from
  the Unicode text in both directions, so there is no `TARGETS`-style format
  negotiation to do. An empty `offers` slice empties the clipboard: Win32 has no
  selection _owner_ to relinquish, so that is what "release" can mean here.

  Reads are answered inside `clipboard_request` and delivered on the next
  `pump`, exactly once. `Win32` has neither Wayland's focus gate nor its serial
  requirement — any window may open the clipboard at any time — so a read is
  never _held_ and `clipboard_offer` never returns `NeedsUserInteraction`. The
  one real wait is `OpenClipboard` being refused while another process has the
  clipboard open, which is routine; it is retried for a bounded 70 ms and then
  reported `Unavailable` rather than failing a paste over a refusal that was
  over before the user noticed.

  Files dropped on a window created with `WindowDesc::accept_drops` arrive as
  one `ShellEvent::DroppedFile` per file, with the drop point in client pixels,
  through `DragAcceptFiles` and `WM_DROPFILES`. The gate is enforced by the
  system as well as by this backend: without `WS_EX_ACCEPTFILES` no drop message
  is ever sent. **There is no drag feedback** — no drop cursor and no hover
  highlight while a file is still in the air — because that is `IDropTarget`,
  which is COM; the drop itself works.

- **crcbl-shell** (Win32): `wait_events` now drains the message queue before it
  sleeps and no longer passes `MWMO_INPUTAVAILABLE`. A message _sent_ to a
  window (rather than posted) leaves `QS_SENDMESSAGE` set after `PeekMessage`
  has dispatched it, and that flag asks to be woken by exactly that bit — so the
  wait returned immediately, forever, and an application idling at zero frames
  per second span a core instead. Draining first is the stronger form of what
  the flag was there for. That removed `QS_SENDMESSAGE` from the picture and did
  not make the wait sleep on a CI runner, where a _posted_ message still wakes
  it; `docs/backlog.md` carries what is known and what is not.

- **crcbl-shell**: `DisplayMode::satisfied_by`, the request-versus-answer
  comparison `WindowState::mode_request_honoured` now uses.

- **crcbl-shell**: a **Win32 end-to-end suite** behind the new `win32-e2e`
  feature (off by default), run by `crates/crcbl-shell/tests/run-win32-e2e.ps1`
  and by a CI job of its own against a real Windows desktop — the treatment
  Wayland and X11 got at P0.5/P0.6. It drives the backend through `open_backend`
  and `dyn Shell` only, and covers what no in-process test can reach:
  keystrokes, clicks and wheel notches **injected from another process** with
  `SendInput`, so they arrive as posted, queued, translated and dispatched
  messages; mode flips and resize storms judged by `GetWindowRect` rather than
  by the backend's own bookkeeping; monitors, DPI and focus against the desktop
  the machine actually has; and a clipboard round trip with a second process, in
  both directions, with this shell's message loop stopped.

  Two helper binaries come with it, `crcbl-e2e-win32-input` and
  `crcbl-e2e-win32-clip`, on the same terms as the two Linux key senders:
  `required-features`, and a `main` that fails loudly on any other platform.

  **The harness defeats Windows' foreground lock, and the backend does not learn
  how.** `SetForegroundWindow` is granted only to a process that already owns
  the foreground or received the last input event, and under `nextest` every
  test is a fresh process with neither — so three tests spent twenty seconds
  each being refused by the job's own console window. The suite now lowers
  `SPI_SETFOREGROUNDLOCKTIMEOUT` for the session (restoring it on the way out,
  for a desktop that is not a CI runner) and attaches its input queue to the
  foreground thread's around the request, which is what an automated harness
  does to arrange a precondition a human would have arranged by clicking. None
  of it is in `src/win32/`: a game does not get to steal focus, and a backend
  that knew how could do it to a user.

  **The sample-level pass has no Windows equivalent yet.** The Linux suites
  press F11 at a running game, which needs a renderer, and no runner on this
  platform has a Vulkan device — `docs/plan/ROADMAP.md` schedules it for P14.

- **`apps/horde` takes `--choose <N>`**, so a headless run can reach past the
  level-up screen. The screen has no way out but a digit key, which parked
  `horde --headless --frames 600 --prefill 200` at its first level-up at three
  seconds — no headless invocation could reach a potion, so every measurement of
  the drop rate came from `game::tests`. The flag presses the digit for the
  player once per distinct offer, tracked by the same level-and-offer identity
  the panel rebuilds on. The digit is validated `1..=UPGRADE_CHOICES` at parse
  time, because a choice out of range is silently ignored by `apply_choice`.

- **asteroids interpolates positions between ticks, snapping across the wrap.**
  Every angle was lerped across the frame's alpha and every position was the
  last tick's, so a rock at 60 Hz on a 144 Hz display moved in sixtieths. Each
  body now publishes `(previous position, current position, teleported)`: the
  wrap sets the flag on the tick it moves a body, a respawn and every spawn
  reset the pair, and the renderer lerps between the pair or snaps on a flagged
  tick — the naive "lerp the positions too" would fly a wrapped body back across
  the whole field.

### Fixed

- **Four samples had silently lost present-based pacing.** `horde`, `breakout`,
  `flappy` and `asteroids` each hand-wrote an `optional_features` set that was
  `crcbl::GpuContextDesc::default`'s **minus `PRESENT_FEEDBACK` and
  `PRESENT_TIMING`** — stale copies of a default from before those were added. A
  device opened without `PRESENT_FEEDBACK` cannot observe its own presents, so
  `GpuContext::acquire`'s closed loop was unreachable in all four: dead code,
  and nothing said so. `apps/sandbox` logged
  `hal: pacing on presents, 2 frames deep` and the four games logged nothing.

  All four now inherit the engine's set rather than restating it, and each has a
  test asserting its `optional_features` equals `GpuContextDesc::default`'s —
  the copies were the mechanism, so the fix removes the copies rather than
  adding two flags to four files. Verified past the log line: run windowed
  against a real Wayland swapchain, each of the four now reaches
  `crcbl-vk: vkWaitForPresentKHR on present 1; the loop is closed`.

  **No frame budget moved.** Horde at 10 000 instances under its own documented
  conditions is 0.130 ms CPU before and after, GPU total 0.045–0.046 ms either
  way, and a windowed 120-frame run is 1.96 s in both. Expected: FIFO already
  paced the loop through `vkQueuePresentKHR`, so closing the loop changes where
  the CPU waits, not how long. Browsers and wgpu grant neither flag, so those
  paths are unchanged and keep the open-loop limiter.

- **A browser with `navigator.gpu` and no adapter killed demo boot with an
  uncaught `TypeError`.** Reported against the live site on Chromium 151 under
  Wayland with `--render-node-override` on a hybrid Intel/NVIDIA laptop, whose
  `chrome://gpu` reads `Vulkan: Disabled` — and Chrome runs WebGPU on Vulkan
  there, so every adapter request is refused.

  `GPU.requestAdapter()` resolves to **`null`** in that case, and wgpu 30 loses
  it: the vendored binding types the nullable WebIDL return as
  `js_sys::JsOption<GpuAdapter>`, whose `into_option` counts only `undefined` as
  absent, so JS `null` arrives as `Some(null)`. `enumerate_adapters` then yields
  a one-element list holding it and `Adapter::get_info()` reads `.info` off
  `null`. Nothing generated for a structural getter has a `try`, so the
  `TypeError` unwound through wasm uncatchably instead of reaching the "no
  usable adapter" arm `WgpuInstance::new_async` already had.

  It now asks the browser for an adapter before enumerating anything — wgpu's
  own `is_browser_webgpu_supported`, which tests the result for null before
  reading a property off it — and returns `None` with a named reason in the log.
  No adapter metadata is invented. `web/engine/demo.js` also asks, before
  downloading the engine, and says "This browser has WebGPU, but no GPU to run
  it on" with a pointer to the browser's own GPU report, warning that its WebGPU
  line can read "Hardware accelerated" while every adapter is still refused.

- **The portable bindless declaration failed on wgpu and overflowed on D3D12.**
  `BindGroupLayoutEntry::count` of `u32::MAX` is the seam's "as many as you
  can": `crcbl-vk` clamped it to `Limits::max_bindless_descriptors` and the null
  backend mirrored that, while `crcbl-wgpu` handed it to wgpu verbatim and got a
  hard rejection — so the one spelling meant to be portable built a layout on
  Vulkan and errored on the web backend. Worse on `crcbl-dx12`: a `u32::MAX`
  binding **without** `BindingFlags::VARIABLE_COUNT` planned a descriptor range
  of `u32::MAX`, and the running offset then overflowed for every range after
  it. Both now resolve the sentinel through the seam's `resolved_count`.

  `crcbl-mtl` deliberately still **refuses** it rather than clamping. It reports
  `max_bindless_descriptors: 0` because flat argument tables have no
  runtime-sized array, so clamping would hand back a one-element array on a
  backend that cannot do bindless at all — the quiet downgrade the seam exists
  to forbid. A named refusal is the honest answer there.

  The field's own documentation did not state the sentinel before this — only
  the module header mentioned it in passing — which is how two backends came to
  ignore it. It says so now.

- **`crcbl-wgpu` silently dropped three things the seam says it must refuse.**
  `create_bind_group_layout` read `visibility`, `kind` and `count` and nothing
  else, so a layout setting any `BindingFlags` on a device without
  `Features::DESCRIPTOR_INDEXING` was built as an ordinary fixed array wearing a
  bindless declaration, and a `VARIABLE_COUNT` entry that broke the ordering
  rule — it must be both the last entry of the slice and the highest binding
  number — was accepted. `create_bind_group` dropped
  `BindGroupDesc::variable_count` without a word. Each is now refused by name,
  in the wording `crcbl-vk` and `crcbl-mtl` already use, so all four backends
  answer the same descriptor the same way.

  `variable_count` is **validated rather than honoured**, and the reason is in
  the code: on Vulkan the number sizes an allocation that `update_bind_group`
  fills in later, and wgpu has neither half — a binding array's length _is_ the
  length of the slice handed to `create_bind_group`, and this backend's
  `update_bind_group` is `Unsupported` because WebGPU bind groups are immutable.
  So the number says nothing the entry list has not, and it is checked against
  the entries and the layout's declared ceiling instead.

  Two smaller ones alongside. `count: 0` is refused rather than mapped to a
  scalar binding, which vk, D3D12, Metal and the null backend all already did.
  And `create_bind_group_layout` is now error-scoped like the pipelines and bind
  groups: wgpu reports a rejected layout to the error handler and **still
  returns an object**, so a poisoned layout used to arrive as `Ok` and surface
  as a validation failure in whichever pipeline later named it.

- **`crcbl-wgpu` could not fill an array binding, so every descriptor-indexing
  bind group it built was broken.** `Device::create_bind_group` resolved each
  `crcbl_hal::BindGroupEntry` to a scalar `wgpu::BindingResource` keyed on
  `binding` alone — `BindGroupEntry::array_index` appeared nowhere in the crate
  — so two entries naming elements 0 and 1 of one binding arrived as two
  `wgpu::BindGroupEntry`s with the same binding number. The layout half already
  mapped the seam's `count` onto wgpu's `Some(NonZero)`, so the layout was
  expressible while the group was not, and the backend reports
  `Features::DESCRIPTOR_INDEXING`. Entries are now bucketed by binding, sorted
  by `array_index`, and emitted as `TextureViewArray` / `SamplerArray` /
  `BufferArray` when the **layout** declares a count — wgpu picks the spelling
  off the layout, not off how many entries a group happens to supply.

  Two things a caller can now see. Fills wgpu has no spelling for are refused as
  `HalError::InvalidDescriptor` naming the binding and the index, rather than
  packed: a hole (wgpu's arrays are dense, so element _i_ of the slice **is**
  array element _i_, and closing a gap would silently shift every later element
  down one), an index written twice, an index past the declared count, one
  binding filled with more than one kind of resource, and an entry naming a
  binding the layout never declared. A trailing shortfall — elements `0..n` with
  `n` below the count — is the one partial fill wgpu accepts and still builds.
  And `create_bind_group` is now error-scoped like the pipelines already were:
  wgpu reports a rejected bind group to the error handler and **still returns an
  object**, so a bad group used to arrive as `Ok` and surface as a validation
  failure in whichever pass later bound it. It is now
  `HalError::Backend("wgpu create_bind_group: …")` at the call that made it.

- **`crcbl::screenshot`'s readback barriers lied about the swapchain image's
  state, and never put it back.** `OffscreenSetup::draw_and_readback` declared
  its pre-copy transition as coming from `ResourceState::ColorAttachment` — the
  state the frame's last pass leaves the target in, not the state the graph
  hands it back in, which is `ForwardRenderer::present_target`'s
  `final_state: Present` — and then presented the image still in
  `ResourceState::TransferSrc`. Vulkan reported the first as
  `VUID-VkImageMemoryBarrier2-oldLayout-01197` on every screenshot ever taken;
  the second is a D3D12 debug-layer error on the second trip round the ring,
  where the declared before-state `COMMON` meets an image left in `COPY_SOURCE`.
  The copy is now bracketed by `Present` → `TransferSrc` and `TransferSrc` →
  `Present`. Pixels are unchanged — the golden cube still matches to zero
  differing pixels.

- **The three GPU draw-generation counters are device-local, zeroed by a
  dispatch inside the frame.** `crcbl_render::draw_gen` put its survivor count,
  indirect arguments and draw counts on `MemoryLocation::HostUpload` and bound
  them writable, so that `DrawGen::begin_frame` could zero them from the CPU —
  the seam allows a buffer fill only outside a pass, and a render-graph frame is
  passes end to end. D3D12 has no unordered access view of an upload-heap
  resource at all, so that arrangement is what removed its device. A new
  `clear_counters.slang` pass, scheduled by `DrawGen::add_passes` ahead of the
  cull dispatch and barriered into it by the graph, writes the zeroes instead;
  all five of the stage's buffers are now `MemoryLocation::DeviceLocal`, and the
  three the pass owns also carry `BufferUsage::TRANSFER_DST` so a test can
  poison them. `DrawGen::begin_frame` still writes the cull parameters and no
  longer touches the counters. A frame now records three compute dispatches
  ahead of the draws rather than two, and the per-pass GPU timer report names
  `clear-counters` first. Nothing rendered changes.

- **A uniform buffer smaller than 256 bytes removed the D3D12 device.** A
  constant buffer view's `SizeInBytes` must be a multiple of 256 and a view may
  not run past the end of its resource, so `crcbl-dx12` rounding the view up
  over a 16-byte buffer was `DXGI_ERROR_INVALID_CALL` and a removed device —
  reported at whatever call came next, which is why it looked like an offscreen
  swapchain failure. `create_buffer` now pads the **allocation** of any buffer
  carrying `BufferUsage::UNIFORM` up to the same 256-byte block, and every
  constant buffer view is checked against that allocation instead of assuming
  it. Nothing above the seam can see the padding: the size a caller asked for is
  still the size `write_buffer`, `WHOLE_BUFFER` and every bounds check use, and
  `Limits::max_uniform_buffer_range` is a limit on a bindable range rather than
  on an allocation.

- **`crcbl-dx12` refuses a host-visible buffer bound for writing instead of
  taking the device down.** D3D12 has no unordered access view of an upload- or
  readback-heap resource — the flag is rejected at creation and the heap pins
  the resource to a state a shader cannot write from — and the seam permits the
  combination because Vulkan does. Binding one to a
  `BindingKind::StorageBuffer { read_only: false }` slot is now
  `HalError::InvalidDescriptor` naming the binding, the heap and the fix, where
  it used to be a `CreateUnorderedAccessView` that wrote nothing and a device
  removed at the next call. Read-only storage bindings of a host-visible buffer
  are unaffected, and remain how the engine's instance and table buffers are
  read. **A shader that writes a buffer needs `MemoryLocation::DeviceLocal` on
  this backend**; `crcbl-render`'s GPU draw generation still asks for the other
  thing, so the D3D12 frame does not yet run.

- **A `crcbl-dx12` buffer view is bounded and aligned rather than truncated.** A
  storage binding's raw view is refused when its offset is not a multiple of
  D3D12's 16-byte raw-view alignment, or when its range is shorter than one
  four-byte element, or when the element count would not fit `NumElements` —
  which was previously clamped to `u32::MAX`, i.e. to a view running past the
  end of the buffer. A constant buffer binding is likewise refused when its
  offset is not a multiple of `Limits::min_uniform_buffer_offset_alignment`.
  Every one of those was a `Create*View` call that returns `void` and diagnoses
  nothing.

- **Every `crcbl-mtl` draw hung on Apple's paravirtual GPU, and the call was
  `setDepthStencilState:nil`.** `bind_graphics_pipeline` passed nil for any
  pipeline whose descriptor carried no `depth_stencil` — which is every pipeline
  drawing into a colour-only pass — and that argument hangs the virtualised
  device GitHub's macOS runners expose, faulting the command buffer with
  `kIOGPUCommandBufferCallbackErrorHang` while render-pass clears on the same
  device succeeded. A ten-probe bisect isolated it: a hand-encoded pass plus
  `setDepthStencilState:nil` hung, the same pass plus a real
  `MTLDepthStencilState` passed, and each of the five other rasteriser calls
  passed alone.

  Metal documents nil as "restore the default state", so the driver is at fault,
  but the fix costs one object per device and removes the nil path entirely: a
  `MetalDevice` now builds one always-pass, never-write `MTLDepthStencilState`
  when it opens, and a pipeline that declares no depth/stencil state carries
  that instead of `None`. The substituted state compares `Always` with depth
  writes off and keeps on every stencil outcome, so it tests nothing and writes
  nothing — it cannot change an image.

- **`crcbl-dx12` built root signatures naming registers its shaders do not
  read.** `BaseShaderRegister` was the seam's binding number and `RegisterSpace`
  was the set index, on the theory that `[[vk::binding(binding, set)]]` reaches
  HLSL unchanged. It does not: the attribute is Vulkan-only, and `dxc` numbers
  each register class from zero in declaration order across the whole source, in
  space 0 — so a set holding a `ConstantBuffer`, a `StructuredBuffer` and an
  `RWStructuredBuffer` at bindings 0, 1 and 2 is `b0`/`t0`/`u0` in the container
  and was being described as `b0`/`t1`/`u2`. Pipeline creation rejects that, so
  every shader in this workspace whose set mixes resource classes — `mesh`,
  `cull`, `draw_gen`, `compute_probe`, `sprite`, `ui` — could not have been used
  from this backend. Only `triangle.slang`, whose set is one storage buffer,
  happened to work.

  Registers are now assigned per class in ascending `(set, binding)` order,
  threaded across a whole pipeline layout, and the rule is checked against the
  resource table in every committed DXIL container by a test that needs no
  Windows.

### Changed

- **Breaking: `ComputePipelineDesc` carries a `workgroup_size`, and Metal can
  dispatch.** `crcbl-mtl` refused `bind_compute_pipeline`, `dispatch` and
  `dispatch_indirect` outright, because
  `dispatchThreadgroups:threadsPerThreadgroup:` takes the
  threads-per-threadgroup at the _call_ while SPIR-V, DXIL and WGSL bake it into
  the module — so MSL had nowhere to declare it and the seam had no field
  carrying it. `crcbl_hal::ComputePipelineDesc` now has
  `workgroup_size: [u32; 3]`, which every caller must add; take it from the
  `WORKGROUP_SIZE` constant `crcbl-shaders` publishes beside each compute shader
  (`[crcbl_shaders::cull::WORKGROUP_SIZE, 1, 1]`) rather than writing a literal,
  since that constant is pinned to the shader's own `[numthreads(…)]`.

  Two guards keep the new field from becoming a second, independent number.
  `ComputePipelineDesc::check_workgroup_size` refuses a zero, an over-limit
  dimension or too many invocations per workgroup, and every backend calls it;
  and `crcbl-vk` additionally reads the `LocalSize` out of the SPIR-V it is
  compiling and fails with `HalError::ShaderCompilation` naming both sizes when
  the descriptor disagrees with the shader. Metal cannot perform the second
  check — MSL declares no thread count — which is exactly why it is done where
  it can be.

  `crcbl-mtl`'s compute pass now opens a real `MTLComputeCommandEncoder` whose
  lifetime is the pass's, and `bind_group` reaches its argument tables. A copy
  inside a compute pass is now refused rather than silently ending the pass's
  encoder and taking its pipeline state with it, and a barrier inside one is
  ignored exactly as it already was inside a render pass. `Features::COMPUTE` on
  Metal now means the whole path rather than "compute pipelines exist".

- **Breaking: `crcbl_shaders::mesh::FrameUniforms` no longer has a `model`
  field, and the block is 128 bytes rather than 192.** The per-object transform
  moved into the instance array, so `mesh.slang`'s uniform block holds only what
  is genuinely per frame and its vertex stage reads
  `instances[SV_InstanceID].transform`. `ForwardRenderer::begin_frame` keeps its
  signature — the `model: Mat4` it takes is now written into the instance pool —
  so a caller that only drives the renderer is unaffected; a caller that builds
  a `FrameUniforms` itself must drop the field and bind a `GpuInstance` storage
  buffer at `(set 0, binding 2)`. Every `mesh.slang` artifact is regenerated and
  the `mesh` and `ortho mesh` goldens are unchanged by the move.
- **Breaking: the UI pass has one constant path, and `ConstantDelivery` is
  gone.** `crcbl_render::ConstantDelivery`, `UiRenderer::constant_delivery` and
  the `ui_tier_b` shader (`shaders/ui_tier_b.slang` and its `spirv/`, `wgsl/`,
  `msl/` and `dxil/` artifacts) are removed. `ui.slang` takes its viewport from
  a uniform buffer at `(set 0, binding 3)` on every target instead of a
  `[[vk::push_constant]]` block, so one artifact set serves every backend and
  `UiRenderer` builds the same pipeline layout, bind-group layout, buffers and
  command stream whatever the device reports for `Features::PUSH_CONSTANTS`. The
  cost is one indirection per vertex where a push constant would have served;
  the saving is a permutation axis and a second `.slang` that had to be kept in
  step by hand. The sample binaries no longer ask for `PUSH_CONSTANTS` at all —
  nothing in the engine reads one now.

- **`wgsl/ui.wgsl` is a loadable artifact for the first time.** It declares
  `@binding(3) @group(0) var<uniform> constants_0`, where the push-constant form
  lowered to a module-scope `var<uniform>` with no `@group`/`@binding` that naga
  rejects outright — so `crcbl-wgpu`, the only backend that ingests WGSL, could
  not create the UI module from it and resolved `ui_tier_b` instead. Verified by
  parsing and validating every `wgsl/*.slang` output with naga 30: all six pass,
  and the previous `ui.wgsl` fails with "Binding decoration is missing or not
  applicable". The regenerated `spirv/ui.spv`, `wgsl/ui.wgsl` and `msl/ui.metal`
  are byte-identical to the deleted `ui_tier_b` ones, and every Vulkan golden
  image — `button_skin_widths` and `menu_frame_two_sizes` among them — is
  unchanged at zero differing pixels.

- **Breaking: the two-valued renderer tier is gone, replaced by device
  capabilities and three derived path selectors.** `crcbl_hal::RendererTier` and
  `DeviceCaps::tier` are removed; `Features::TIER_A` is renamed
  `Features::GPU_DRIVEN` and documented as a named bundle to pass as
  `optional_features`, never as a requirement. In their place
  `DeviceCaps::geometry_path`, `DeviceCaps::binding_model` and
  `DeviceCaps::lighting_path` answer with
  `GeometryPath::{MeshShader, IndirectCount, IndirectPerBatch}`,
  `BindingModel::{Bindless, ArrayPages}` and
  `LightingPath::{RayTraced, Rasterised}` — each ordered best-first, each
  degrading monotonically, and each also constructible from a bare `Features`
  through `from_features`. Log lines and `Debug` impls that printed a tier now
  print the three selected paths. A tier could not express three independent
  axes, and forcing a device into the wrong bucket is a lie the renderer then
  acts on. The null backend's two presets are renamed with it:
  `NullInstance::tier_a` is now `NullInstance::gpu_driven` and
  `NullInstance::tier_b` is now `NullInstance::portable`.

- **Breaking: `DeviceDesc::for_adapter` requires only what nothing can work
  without.** `required_features` is now
  `Features::COMPUTE | Features::TIMELINE_SEMAPHORE`, with
  `Features::GPU_DRIVEN` moved to `optional_features`. It used to demand the
  whole GPU-driven bundle, so a device was refused over one absent flag while
  having the rest — the reason `crcbl-mtl` was refused outright over
  `DRAW_INDIRECT_COUNT`, which is absent from Metal's API rather than
  unimplemented. That backend now opens on the seam's own constructor and
  degrades. A caller that genuinely cannot render without a feature still names
  it in `required_features` and still gets a named `UnsupportedFeatures`
  failure.

- **`ModeRequest::mode` answers `None` when there is no window to read, instead
  of an invented `Windowed`.** The `DisplayMode` it returned for a dead window
  read exactly like a genuinely windowed run — the defect the `mode_at_exit`
  fallback exists to paper over for summaries. Callers with a live window
  (`Loop::display_mode`, `ModeRequest::toggle`) unwrap it; a run that ended
  still reports through `mode_at_exit`, which keeps the last mode the window was
  seen in rather than inventing one.

- **Breaking: `FrameLimit` stores the rate it was asked for and derives the
  period.** `FrameLimit::fps` is now `const` and `FrameLimit::period` is not;
  `rate()` is new, and `Display` prints `1000 fps` or `unlimited`. Nothing about
  the pacing changes — this is what lets a log report the number that was typed
  instead of a 33.333333 ms period, or a rate recovered from one by a division
  that rounds.

- **Breaking: `LoopConfig` gained `limit`, and `PolledGpu::request` /
  `PolledBoot::request` take a `GpuOptions` in place of an
  `Option<GpuBackend>`.** `GpuOptions` is the half of `GpuContextDesc` that
  comes from the command line rather than from the game — the backend and the
  pacing — so a game's own `desc` ends `..GpuContextDesc::from(gpu)` and the
  next run-level knob is a field there rather than another parameter threaded
  through five bring-up paths. `Common::gpu()` and `Common::loop_config()` are
  the two calls a sample makes; the four games' identical six-line `LoopConfig`
  literals are now one call each.

- **Breaking: `Pacing` has a fourth variant and a new default.** `Pacing::Auto`
  is now `Pacing::default()`; `Pacing::Vsync` is not. Any `match` on `Pacing`
  must gain an arm, and — the quieter half — **every caller that took
  `GpuContextDesc::default()` has changed behaviour without changing a line**:
  such a context now opens on vsync, asks the display once after its first
  present, and rebuilds itself onto the adaptive present mode if the display
  reports `DisplayTiming::Variable` or `Stepped`. `Fixed`, `Unknown` and a
  failed query all stay on vsync, which is what every machine this repo can test
  on reports.

  **A caller that wants the old behaviour writes `pacing: Pacing::Vsync`** in
  its `GpuContextDesc` (or calls `set_pacing(Pacing::Vsync)`): a concrete
  `Vsync`, `Adaptive` or `Off` is never overridden by the observation, which
  refines `Auto` and nothing else. `Pacing::Auto.preferences()` is the vsync
  list — the swapchain genuinely opens on `Fifo`, because the present mode is
  chosen before any present exists and `VK_EXT_present_timing` is specified to
  report nothing until one has — so `Auto` and `Vsync` differ in what happens
  after the first present, not before it.

- **`crcbl_hal::ShaderModuleDesc` gained a `dxil` field**, and
  `crcbl_hal::ShaderSources` a matching `DXIL` bit. It is `Option<&'a [u8]>` — a
  DXIL container is a signed binary blob, so it is closer to `spirv: &[u32]`
  than to the `Option<&str>` source text of `wgsl` and `msl`, and it is an
  `Option` because a zero-byte container is a _truncated_ file rather than an
  absent one. Every construction site must name the field; a module carrying two
  entry points passes `None`, which is the truthful answer rather than an
  omission.

- **`crcbl-shaders`: `Shader::new` and `EntryPoint::new` changed shape.**
  `EntryPoint::new` takes the entry point's DXIL container as a third argument
  and `Shader::new` is unchanged in arity — the DXIL hangs off the entry point,
  because that is what a container holds. Only the generated table calls either.

- **`crcbl-shaders`: the SPIR-V, WGSL and MSL artifacts are byte-identical.**
  Nothing about the existing three targets changed; a moved hash there would be
  a bug, not a re-bless.

- **`crcbl-shaders`**: `tools/compile-shaders.sh` now passes
  `-fvk-use-entrypoint-name` to the SPIR-V target, so a module's entry point
  keeps its source name in `OpEntryPoint`. Without it Slang renames a module's
  _only_ entry point to `main` while the WGSL and MSL targets keep the real
  name, which would have made a single-entry-point module addressable as `main`
  on Vulkan and as its own name everywhere else. Every existing artifact is
  byte-identical with and without the flag — each has two entry points, which is
  the case Slang does not rename — so no committed `.spv` moved and no golden
  image needed re-blessing.

- **crcbl-mtl**: **a GPU fault now names the encoder that caused it.** Every
  `MTLCommandBuffer` this backend creates is built from an
  `MTLCommandBufferDescriptor` carrying
  `MTLCommandBufferErrorOptionEncoderExecutionStatus`, and the `HalError`
  reported by `poll_readback` and `wait_idle` unpacks
  `MTLCommandBufferEncoderInfoErrorKey` out of the failure's `userInfo`: each
  encoder in recording order, with its label, its debug signposts and whether it
  faulted, was merely affected, or never started. The message also carries the
  `NSError` domain and code and the `MTLDevice`'s own name. Where a fault
  previously read `Caused GPU Hang Error (00000003:…)` and stopped, it now says
  which of a command buffer's encoders died — the difference between a broken
  render pass and a copy that never ran. Every encoder is labelled to make that
  legible: the copy encoder is `crcbl copies`, and a render pass with no
  `RenderPassDesc::label` is `crcbl render pass` rather than nameless.

- **crcbl-shell** (Wayland): the effective mode of a fullscreen window now names
  the monitor it is on, taken from `wl_surface.enter`. Asking for a monitor is
  only a hint on this platform, but which one the compositor used is observable,
  and without it `mode_request_honoured` answered "no" to a request the
  compositor had honoured exactly. A summary line that read `borderless` may now
  read `borderless on monitor 2`. `None` still means the backend cannot say —
  the surface is on no output or on two.

- **asteroids**: the ship draws a flame under its nozzle while thrusting. The
  sheet gains a second frame (`assets/ship.crpix`), `RenderState` carries the
  thrust intent from the tick, and `art::Scene::build` picks the frame — the
  ship is no longer one picture whether or not the engine is on.

### Fixed

- **A mesh anywhere but the start of the geometry pool drew another mesh's
  vertices, on Vulkan and Direct3D but not on WebGPU or Metal.** `mesh.slang`
  pulled its vertex with `vertices[SV_VertexID]` while
  `crcbl_render::ForwardRenderer` passed the mesh's `MeshRange::base_vertex`
  through `draw_indexed`'s own base-vertex argument — and Slang lowers
  `SV_VertexID` to `gl_VertexIndex - BaseVertex` on SPIR-V, which subtracted
  that base straight back out. The same disagreement covers `SV_InstanceID`.
  Invisible while the cube was the pool's only resident, because its base is 0.

  Fixed the way `sprite.slang` resolved its half: **every draw the forward pass
  records now passes zero for both of its bases**, which is the one value all
  four targets agree on, and the real ones arrive as data. The instance index is
  a `crcbl_shaders::mesh::DrawConstants` block (binding 3, one 16-byte block per
  draw, reached through a dynamic offset); the base vertex is the mesh table's,
  reached through the drawn instance — see the mesh-table entry above, which
  moved it there. Nothing in the picture now depends on how a target lowers a
  builtin. The mesh pool still stores indices mesh-relative, so a mesh's bytes
  still do not depend on where it landed.

  Upgrading: anything that builds `mesh.slang`'s descriptor set by hand must add
  binding 3 — a uniform buffer holding `DrawConstants` — or the pipeline draws
  nothing.

- **Asteroids: a rock straddling a field edge was drawn once, so half of it
  vanished for the whole of a crossing.** The field wraps, and the half past the
  seam belongs at the opposite edge — now it is drawn there: every rock is
  emitted at its own position plus a ghost per wrapped offset (`wrapped_offsets`
  in `apps/asteroids/src/art.rs`), with the corner case (a rock crossing a
  corner needs the diagonal copy too) covered. A wave spawns its rocks **on**
  the border, so this was visible at every wave start rather than only during
  mid-flight crossings. The ship and the shots straddle the same seams and are
  left single — their crossings are shorter and the missing half less
  conspicuous.

- **`crcbl-shaders`**: `build.rs`'s byte-for-byte recompile check invoked
  `slangc` with an **absolute** source path while `tools/compile-shaders.sh`
  uses one relative to the crate root. Slang copies the path it was given into
  the `#line` directives of its Metal output, so the check compared an artifact
  against a differently-pathed rebuild of itself and failed the build outright —
  on every machine with the pinned compiler installed, which is to say on every
  machine belonging to someone editing a shader. The recompile now runs from the
  crate root with the manifest's own relative path.

- **`crcbl-vk`**: **a surface handle crossed instances silently, and freeing it
  destroyed the wrong object.** Each `VkInstance` owns its own surface pool, so
  two instances issue byte-identical handles; the ownership check compared the
  entry's owner against the looking-up instance, which is trivially true for
  whatever _that_ instance holds at the same index. So instance A answered
  `surface_caps` for instance B's handle with A's own surface, accepted it as
  `compatible_surface` in `request_device`, and — the one that corrupts state —
  `A.destroy_surface(b)` freed **A's** surface while B went on using a handle it
  still believed live. `crcbl-hal`'s obligation 3 requires
  `HalError::ForeignObject` here; the arm existed and was unreachable.

  Surface handles now carry their issuing instance, reusing the tagging scheme
  the device-scoped handles already used, so the check is against the handle's
  own tag rather than against the pool it was looked up in. A handle no instance
  issued is still `InvalidHandle`, and one belonging to another instance is now
  `ForeignObject`. Found by writing the cross-instance test the reference
  backend did not have — `crcbl-hal`'s null backend had covered this case all
  along.

- **crcbl-dx12**: **a software rasteriser was enumerated as an integrated GPU.**
  `is_software` consulted only `DXGI_ADAPTER_FLAG_SOFTWARE`, and DXGI lists
  "Microsoft Basic Render Driver" — which is WARP — with that flag _clear_, so
  `Instance::adapters` reported it as `DeviceType::Integrated`. A caller ranking
  `Discrete > Integrated > Cpu` to prefer real hardware picked the software
  rasteriser and believed it had a GPU. Measured on `windows-latest`, where
  neither listed adapter is hardware; a machine carrying the Basic Render Driver
  beside a real GPU is where it would have cost a frame rate.

  The test is now that flag **or** Microsoft's own vendor and device ids, named
  as constants in `crcbl_dx12::adapter` and read off the runner's own
  enumeration line. Both halves of the pair are required, and the flag is still
  consulted first, so an adapter that sets it is caught whatever its ids say.
  One consequence reaches `Instance::adapters`: such an entry is now skipped by
  the hardware pass and appended once by `EnumWarpAdapter`, so a machine that
  listed it twice lists it once, as `Cpu`.

  The LUID de-duplication is unchanged, and was never what fixed this: those two
  entries carry different LUIDs, so DXGI considers them two adapters and there
  was nothing for it to collapse.

- **crcbl-wgpu**: **the browser build presented every frame far too dark.**
  WebGPU's supported context formats are all linear — `getPreferredCanvasFormat`
  returns `rgba8unorm` or `bgra8unorm`, and `configure` refuses an `-srgb` one —
  so `SurfaceCaps::preferred_format` fell through to its "first format offered"
  fallback and picked a linear target. Every pass above the seam writes
  display-referred values and leaves the sRGB encode to the hardware, so on a
  linear target the encode simply never happened: the horde's grass, authored at
  `#19211a`, reached the canvas as roughly `#020302` while the same bytes were
  right on Vulkan.

  A WebGPU surface now advertises the sRGB counterpart of each 8-bit format it
  reports, and a swapchain asked for one is configured with the linear
  counterpart plus that format in `viewFormats` — the encode comes from the view
  `acquire_next_frame` builds. Nothing changes for a native surface, which
  offers its sRGB formats outright: the counterparts are appended only where the
  surface did not already list them, and only for a canvas.

- **crcbl** (engine): **a key held when a menu opened stayed held forever.**
  `MenuPump` claims Up, Down and Enter while a menu is showing, and it was
  claiming the _release_ as well as the press — so a movement key pressed before
  the menu opened and let go under it was never reported up to the game. In the
  horde: hold Down, level up, pick an upgrade, and the wizard walked south with
  nothing pressed until the key was tapped again.

  The held-key list is now what its documentation always said it was — the keys
  the game has been told are down — and a claimed release is forwarded when the
  key is on it. A claimed _press_ still does not reach the game and no longer
  joins the list, so the game only ever sees matched pairs. The list already
  cleared correctly here; clearing it was never the fix, because nothing but
  focus loss reads it.

- **crcbl-shell** (AppKit): `CrcblWindow` overrides
  `constrainFrameRect:toScreen:` to answer the proposed rectangle unchanged, so
  AppKit can no longer silently rewrite a frame this backend sets. The default
  keeps a title bar clear of the menu bar, which is right for a window a person
  dragged and wrong for every frame here — all of them are computed from an
  `NSScreen` rectangle and are on that screen by construction. `setFrame:` also
  now reads the frame back and logs when a window did not go where it was put,
  which nothing above this layer could otherwise notice: `WindowState` carries
  an extent and no position.

  That override was necessary and not sufficient; the defect that prompted it is
  fixed in the entry below.

- **crcbl-shell** (AppKit): **a mode change put the window back where it was
  created.** `DisplayMode::Borderless` produced a window of exactly the right
  size at the wrong origin — hanging off two edges of the display — and the way
  back was worse, restoring the creation frame's origin _and size_ rather than
  the placement the window had before the flip. Neither was visible through the
  seam, which carries an extent and no position.

  The cause is a fact about AppKit worth stating on its own:
  **`-[NSApplication setPresentationOptions:]` returns every window of the
  application to its creation frame.** Not the window it is called about — the
  property is on `NSApplication` — and not "constrains it to the screen". The
  backend applied the borderless presentation options _after_ placing the
  window, so every frame it set was immediately thrown away, on both legs of the
  round trip.

  `apply_mode` now applies the style mask, then the presentation options, then
  the frame, making the frame the last geometry it sets. The middle position
  matters as much as the last: applying the options before the style mask
  changes makes AppKit raise `NSInvalidArgumentException`, and an Objective-C
  exception unwinding through Rust aborts the process. `appkit::window`'s module
  docs carry the measurement and all three positions, since anyone reordering
  those statements would otherwise reintroduce one defect or the other.

- **crcbl-shell** (AppKit): **a mode change took the keyboard away from the
  view.** `-[NSWindow setStyleMask:]` rebuilds a window's frame view and the
  content view stops being the first responder — so after a flip to
  `DisplayMode::Borderless`, or back, `sendEvent:` delivered every key event to
  the window and `CrcblView` received none. A game that pressed F11 went
  permanently deaf, silently, with no error anywhere. `apply_mode` now re-claims
  the first responder after each style-mask change, sharing `focus_content_view`
  with window creation so the two cannot drift, and the session asserts the view
  still has the keyboard **after the borderless leg** as well as after a full
  round trip — a game stays borderless, so a responder restored only on the way
  out would be a game that is deaf for as long as it is being played.

- **crcbl-shell** (AppKit): windows no longer take part in **macOS state
  restoration**. `isRestorable` defaults to `YES`, which enrols a window in a
  feature this backend cannot honour and should not want: restoration re-creates
  windows at launch through a `restorationClass` or an application-delegate
  callback, neither of which exists here — the backend deliberately never takes
  the delegate slot — and it makes the operating system a second, invisible
  source of truth for a placement the seam hands to `WindowDesc` and a game
  hands to its settings screen. It also writes saved state to disk keyed by an
  application identity an unbundled binary does not stably have.
  `setRestorable:` is now `NO` at creation. Argued on its own merits; whether it
  also accounts for the borderless-origin defect above is a separate question.

- **crcbl-shell** (X11): hiding a window with `set_visible(false)` unmapped it
  without telling the window manager. ICCCM 4.1.4 requires a synthetic
  `UnmapNotify` to the root alongside the unmap, because a reparenting manager
  watches the frame it created rather than the client window inside it and may
  never see the real event. Under `openbox` the window was unmapped and remapped
  before the application could observe it hidden.

- **crcbl-shell**: `WindowState::mode_request_honoured` compared the requested
  and effective modes with `==`, which is wrong whenever the backend can name
  the monitor. `Borderless { monitor: None }` means "wherever the window already
  is" as a _request_, so an answer of `Borderless { monitor: Some(..) }`
  satisfies it — but the two are not equal, so every granted fullscreen on X11
  read as refused and a UI toggle over a fullscreen window would have shown
  "off". The comparison is now `DisplayMode::satisfied_by`, which keeps the
  asymmetry: a request naming a monitor is still not answered by one that cannot
  say which.

- **crcbl-shell** (X11): the backend never wrote `WM_HINTS`, so it never told a
  window manager that its window wants the keyboard. ICCCM 4.1.7 lets a window
  manager assume convenient values when the property is absent, and "this window
  takes no input" is one of them — a game whose window is never focused receives
  no key for its whole run. It now writes `input = True` with `NormalState`,
  which is ICCCM's passive focus model and what every toolkit does. **Changed
  nothing measurable under openbox**, which defaults the other way; this is
  conformance rather than an observed repair.

- **crcbl-shell** (X11): a `set_mode` issued after a window was configured but
  before its `MapNotify` arrived was silently dropped. `apply_fullscreen` chose
  between writing `_NET_WM_STATE` and sending a `ClientMessage` on whether the
  window was mapped — which follows `MapNotify` — but a window manager begins
  managing a window at the map _request_, and on X11 the first configure also
  arrives before `MapNotify`. A game that opened a window, waited for its size
  and asked for fullscreen landed in that gap every time: it wrote a property
  the window manager then overwrote with its own view. It now branches on
  `XWindow::map_requested`, and the whole X11 suite runs under `openbox` in CI
  as well as under bare Xvfb.

- **crcbl** (`engine`): a run that ended because the player closed the window
  reported `DisplayMode::Windowed` whatever mode it had been in. Accepting a
  close request destroys the window, and the summary is built afterwards, so
  `ModeRequest::mode` had nothing left to read and fell back to its default — in
  the same words a genuinely windowed run uses, so nothing downstream could tell
  the two apart. `ModeRequest` now records the mode it last saw and the new
  `ModeRequest::mode_at_exit` prefers the live answer, falling back to that.
  `Loop::finish` and `apps/bare` both use it.

- **crcbl-shell** (X11): a window created with
  `WindowDesc { mode: Borderless, .. }` reported its own request back as the
  effective mode when no window manager was running. EWMH has the _client_ write
  `_NET_WM_STATE` to request an initial state — before a window is mapped there
  is no window manager conversation to have — and a window manager then takes
  ownership of the property. The backend worked out the effective mode by
  reading that property back, so with nobody to take ownership it read its own
  write: `effective_mode()` said borderless and `mode_request_honoured()` said
  true, for a window still at its windowed size that nothing had touched. It now
  trusts `_NET_WM_STATE` only when `_NET_SUPPORTING_WM_CHECK` says something is
  there to have written it.

  `set_mode` after mapping was never affected — that path sends a client message
  to the root window and never writes the property — so the bug was reachable
  only through the creation path, which is exactly the path the new
  `--fullscreen` flag takes. Every WM-less X session, kiosk and CI runner would
  have had a summary line claiming a fullscreen it did not have.

- **horde**: `--wall-clock` stopped reaching the clock. Hosting the game in the
  engine's loop changed the wiring from `Clock::new(!real_clock())` to
  `Clock::new(headless)`, so a headless run with the flag read the fake
  fixed-step clock and the debug panel's frame timing reported the step rather
  than the frame — every headless scale measurement since was measuring nothing.
  The wiring is restored, and a regression test pins a headless `--wall-clock`
  run on the real clock while a headless run without it keeps the fixed step.

### Added

- **crcbl** (`args::Common`): `--fullscreen`, and `Common::display_mode()` that
  turns it into a `DisplayMode` for `WindowDesc::mode`. Asked for at window
  creation rather than switched to afterwards, so a fullscreen game does not
  show a decorated window for the frames a `set_mode` would take to land. `F11`
  still toggles from either starting point. Every sample honours it —
  `apps/sandbox` through its own parser, which predates the shared one.

- **samples**: the summary line each binary prints now names the display mode
  the window system actually settled on, beside the extent. `RunSummary::mode`
  already carried it and nothing reported it, which left a refused fullscreen
  indistinguishable from an honoured one from outside the process. `apps/bare`
  gained a `Summary::mode` field to do the same from a hand-written loop, via
  the public `engine::ModeRequest::mode`.

- **crcbl-sprite** (`bake::bake_dir`): the generated table now declares
  `ART_TICK_HZ`, the rate the holds were baked at. A `.crpix` counts holds in
  simulation ticks and an Aseprite sidecar counts milliseconds, so the
  conversion runs once at bake time and once at load time and the two must agree
  — and a build script cannot `use` the crate it builds, so every consumer
  declared the number a second time beside its loader. Five copies (`apps/*` and
  `crcbl-render`) are deleted; the `build.rs` value is the only source.

- **crcbl-phys**: `DampingForce::world_force(velocity, mass, dt)` and
  `DragForce::world_force(velocity)`, beside the `ThrustForce::world_force` that
  already existed. A force provider applies to **every** dynamic body, so a game
  damping one entity among a field of others could not use the pipeline;
  `apps/asteroids` wrote `-k·v` and the `mass/dt` cap out by hand instead, and
  that copy is now deleted. The cap travels with the route — it is what stops a
  coarse tick rate from over-damping past zero and flying the body backwards.

- **crcbl-phys**: `overlap_sphere_into` on both `PhysicsSystem` and
  `PhysicsWorld`, and `Bvh::traverse_aabb_into`, so a game that queries once per
  body per tick can hoist one buffer out of its loop. The owned forms cost three
  `Vec`s per call — the result, the collider ids, and the BVH's candidate list —
  and the descent stack a fourth; the `_into` path clears and refills the
  caller's buffer and keeps the rest as fields, so a crowd steers without
  allocating. The owned forms remain, unchanged for every existing caller, and
  now delegate.

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

- **crcbl-cli** (`crcbl new`): the scaffold now hands you the engine-owned loop.
  `src/main.rs` was 276 lines that opened the shell, called
  `unsafe { instance.create_surface(&target) }` itself, configured its own
  swapchain and ran its own `loop {}` — while every sample had stopped doing any
  of that and no crate under `apps/` contains an `unsafe` block at all. Its doc
  comment argued the loop was "deliberately yours rather than the engine's"
  because "an engine that owned it could not run in a browser", which
  `crcbl::web` had already disproved in four published demos.

  A generated project is now a `HostedGame` and a `GameGpu` over
  `crcbl::engine::GpuContext`, and arrives with a pause menu on `ESC`,
  fullscreen on `F11`, the debug panel with per-pass GPU timings on `F3`, mouse
  and keyboard menu navigation, and resize handling — none of which the old
  template had. It parses its flags with `crcbl::args::Common` and builds its
  help text from the engine's own two blocks, so `--tick-hz`, `--backend` and
  the debug-overlay pair work and cannot drift. `log = "0.4"` is gone from the
  generated manifest: `crcbl::log` covers it, so a new project starts with the
  same single dependency the samples have. The template ships three unit tests
  and `crcbl-cli`'s scaffold e2e now runs them.

  One consequence to know about: a generated project goes through
  `crcbl::backend`'s real registry, where the old template hardcoded
  `NullInstance`. That registry never falls back to null on its own, so the
  generated `.github/workflows/ci.yml` names `--backend null` — a stock CI
  runner has no driver, and without it the first push fails with
  `ERROR_INCOMPATIBLE_DRIVER`. Drop the flag once that job installs one.

  The library-style loop is still supported and is still `apps/bare`, guarded
  from outside the crate by `crates/crcbl/tests/library_seam.rs`. What changed
  is which of the two a new project starts from.

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

- **asteroids**: rocks kept shattering and scoring behind the game-over panel —
  leftover bullets swept unconditionally, and the score the best never saw could
  exceed the recorded one. The bullet sweep and `shatter`'s score line are now
  gated on the playing state. The tick also allocates nothing now: the sweep,
  wrap and view-refresh paths borrow or hoist their per-tick buffers.

- **breakout**: the Start menu popped up between lives when the first life was
  lost at score 0 with the grid still full — that state is indistinguishable
  from a fresh game by score and grid alone. `MenuKind::of` now also requires
  full lives, so "never started" and "one life down" are told apart.

- **horde**: a bolt still in flight when the player died kept killing enemies
  behind the death panel — the kill counter, kill sound and gem drops continued
  for up to `BOLT_LIFE`, contradicting the documented "the kill count is
  frozen". The bolt sweep is now gated on the playing state.

- **crcbl-ui**: a drag from one menu item onto a neighbour drew the neighbour
  `Pressed` — the drawn state came from a menu-global "something is down" flag
  plus whatever was hovered, not from `UiState`'s capture. The item the press
  belongs to is now tracked, so a drag-off leaves both items `Idle`.

- **crcbl-store**: `ReplayWriter::encode` wrote a `>4 GiB` entry's length as a
  truncated `u32` — a corrupt file with no error. It now refuses with the
  format's u32 length named, exactly as `save.rs` does.

- **crcbl-sprite**: crpix bake-time pixel math overflowed `u32` on a
  large-but-parseable file — a 32768×32768 frame's `width × height × 4` wrapped
  to zero, and the strip's `sheet_w × fh × 4` wrapped too, producing a truncated
  sheet or an OOB index panic. Frame and strip sizes are now checked in `u64` at
  parse time and refused with a named `TooLarge` error.

- **crcbl-wl-scanner**: an attribute value ending in `/` was mistaken for the
  self-closing marker — `<arg summary="foo/">` had its slash stripped from the
  value and the tag flagged empty. The trailing-slash test is now quote-aware.

- **crcbl** (`engine`): a menu key pressed before a menu opened stayed in
  `held_keys` forever when released while the menu was showing — the menu-key
  arms dropped the release before the held-key bookkeeping ran. The bookkeeping
  now runs for every key, matching its own documentation.

- **crcbl-phys**: a sweep shorter than the quadratic solver's EPSILON floor
  (below ~1.5e-8 m) was reported as a miss even when it started inside the
  target — `solve_quadratic` rejects `a <= EPSILON` outright, so the swept
  queries now treat anything at or below that floor as stationary and report the
  resting contact. Overlap queries against an inverted box (`Aabb::EMPTY`) also
  panicked on the clamp; they now answer "no overlap" instead.

- **crcbl-phys**: `RigidBody::new_dynamic(0.0)` was a silent NaN cascade in
  release builds — the only guard was a `debug_assert`, so `inverse_mass = +inf`
  poisoned every query in the world. The contract now panics in every build.

- **crcbl-audio**: the synth generators overflowed on hostile parameters —
  `(sample_rate × seconds) as usize` saturates to `usize::MAX` and
  `frames × CHANNELS` then wraps or aborts, and `looped_sine(0.0, …)` divided by
  zero. Frame counts are now computed in f64, capped at a minute, and a zero
  frequency returns an empty buffer.

- **crcbl-input**: `begin_tick` accepted a negative `dt`, moving the clock
  backwards so a held button reported a negative `Held` duration. Only forward
  time is accepted now.

- **crcbl-render**: `upload_texture` sized a compressed format's row as
  `width × block_size` — for BC formats the block covers 4×4 texels, so a BC1
  row is `ceil(width/4) × 8` bytes, and a compressed upload was silently wrong
  by a factor of four. Compressed formats are now refused by name before any
  device call; no caller uploads one.

- **crcbl-render**: `UiRenderer::begin_frame` committed the new element counts
  before the buffer uploads that make them true — a failed `write_buffer` left
  the new counts over stale indices and the next draw read out of bounds. The
  counts are now committed only after the uploads succeed.

- **crcbl-wgpu**: `DeviceDesc::compatible_surface` was never validated — a
  destroyed or foreign surface handle was accepted where the null backend
  returns `InvalidHandle`. `request()` now checks the handle against the
  instance's surface pool. `write_buffer` also accepted `HostReadback` buffers
  (mappable, but not a valid target); it now requires `HostUpload` exactly,
  matching the null backend.

- **crcbl-wgpu**: a padded indirect-draw `stride` was silently ignored — wgpu
  reads tightly packed argument structs while crcbl-vk honours a stride, so a
  padded one rendered garbage. All four indirect draw methods now refuse a
  non-tight stride loudly.

- **crcbl-shell** (Wayland): a `wl_data_offer` the compositor announced and
  never claimed leaked when refused — a drag `enter` for a vanished seat or
  naming an unannounced id sent `accept(null)` but never destroyed the proxy,
  and a second `enter` without `leave` overwrote the first drag without
  destroying its offer. Refused offers and the previous drag's offer are now
  destroyed.

- **crcbl-shell** (X11): an INCR chunk that could not be read was mistaken for
  the transfer terminator — a null reply or an over-cap property mapped to the
  empty slice that means "paste complete", so a hostile or broken owner's
  truncated paste was reported as a successful transfer. A type-less property
  (the ICCCM terminator) is now returned distinctly from a read failure, and a
  failed chunk read leaves the transfer to time out as `Unavailable` instead.

- **crcbl-shell** (X11): `refresh_server_time` burned its full 250 ms deadline
  when the probe's notify carried the same server millisecond as the previous
  event — at ≥1 kHz event rates the `last_server_time != before` wait could
  never be satisfied. The loop now waits for the probe's own notify to arrive,
  regardless of its stamp.

- **crcbl-shell** (Win32): a window hidden while borderless was re-shown by a
  second borderless request, and by the windowed restore. Both read the
  `WS_VISIBLE` bit from the style snapshot captured at the first borderless
  entry. They now use the live style, and the restore's `SetWindowPlacement`
  gets `SW_HIDE` for a hidden window instead of the saved `showCmd`.

- **crcbl-shell** (AppKit): a borderless window dragged to another display
  published nothing and kept naming the old monitor — `effective_mode.monitor`
  is written only by `apply_mode`, so a move that left size and scale unchanged
  also left the configuration unchanged. `refresh_configuration` now re-derives
  the borderless monitor from the screen the window is actually on.

- **crcbl-net**, **crcbl-server**: a delta in the last 25 bytes before the
  transport's 64 KiB cap encoded and sealed fine but was dropped by
  `send_unreliable` every tick — and the server had already retained it as the
  next delta's baseline, evicting the client's real one and leaving it desynced
  until the world shrank. The encode cap now leaves room for the seal by
  construction, and a snapshot is retained as a baseline only after the
  transport accepted it.

- **crcbl-cli**: `crcbl crpix` frame names that are clip keywords silently
  corrupted the written `.crpix` — a frame named `loop` wrote
  `clip flap: loop loop`, which parses as zero frames plus the loop flag, with
  exit 0. The shared name guard now refuses exact `loop`, `reverse`, `pingpong`
  and `@`, which the format reads as flags rather than frame names.

- **crcbl**, **sandbox**: `--tick-hz` values above `1_000_000_000` parsed
  cleanly and then panicked the engine — `1e9 / hz` truncates to a zero
  nanosecond period, which `FrameClock` asserts against after the GPU is already
  open (exit 101 instead of the documented exit 2). Both parsers now refuse
  rates past `MAX_TICK_RATE`, the same bound `sim` already carried.

- **crcbl-sprite**: `decode_png` sized its output buffer from the PNG's IHDR
  width and height alone — `output_buffer_size` trusts the file's claim and is
  capped only at `isize::MAX`, so a ~100-byte hostile PNG declaring 65536×65536
  forced a multi-gigabyte allocation (2²⁰×2²⁰ aborts the process). The declared
  pixel count is now bounded against `1 << 28` before any allocation, the same
  guard `crcbl-golden`'s `load_png` carries.

- **crcbl-audio**: native audio played 48 kHz-authored voices at the device rate
  — on a 44.1 kHz device everything ran ~9% slow, ~147 cents flat, the exact
  failure the browser path resamples to avoid. The mixer now steps each voice's
  playhead at the internal rate per output frame, so pitch and duration hold on
  any hardware (and are bit-identical when the rates match).

- **crcbl-audio**: the mono and multichannel output paths allocated a scratch
  `Vec` on the OS audio thread every block. The scratch is now owned by the
  stream's callback and reused — one allocation, then a resize and zero per
  block, with no malloc on the realtime path after the first block.

- **crcbl-wgpu**: an MSAA pass silently dropped its resolve target — every
  `wgpu::RenderPassColorAttachment` hardcoded `resolve_target: None`, so a 4x
  pass rendered into the MSAA image and nothing was ever resolved. The resolve
  views are now resolved from the pool and wired into the pass, and a stale
  resolve handle fails loudly instead of dropping the resolve unnoticed.

- **crcbl-wgpu**: push-constant range addition overflowed — `offset + size` in
  plain u32 arithmetic panicked in debug and wrapped to 0 in release. The end is
  now computed with `saturating_add` and ranges past the device's maximum are
  refused with `InvalidDescriptor`, matching the null backend.

- **crcbl-shell** (Wayland): announced-but-unclaimed `wl_data_offer`s grew
  without bound — a hostile compositor that announces offers and never claims
  them accumulated proxies, sink entries and per-offer mime strings for the
  whole session. A `Device` now holds at most 8 pending offers, evicting (and
  destroying) the oldest past the cap, and each offer's format list is capped at
  32 — the same bound every transfer already carried.

- **crcbl-shell** (X11): an `INCR` clipboard transfer **to one of our own
  windows** — a self-paste, or one of our windows pasting our own offer, of a
  payload over the server's request limit — replaced that window's event mask
  with `{PropertyChange}` and stripped every input event off it permanently.
  `ChangeWindowAttributes(EVENT_MASK, …)` is a replace, not an OR, and our own
  windows already select `PropertyChange` through `WINDOW_EVENT_MASK`; the call
  is now skipped for them, and kept only for foreign requestors, whose mask we
  cannot know.

- **crcbl-shell** (Win32): minimizing a captured window re-applied the pointer
  clip from the iconic window's 0×0 client area — both corners mapped to the
  same point, pinning the cursor for the whole minimized period. The `WM_SIZE`
  `SIZE_MINIMIZED` arm now releases the clip (keeping the recorded target, so
  restore re-clips from the real rectangle) instead of falling through to
  `reclip`.

- **crcbl-shell** (AppKit): a hidden window — created `visible: false` or hidden
  with `set_visible(false)` — popped on screen and took key focus when
  `set_mode(Borderless)` ran: the borderless arm ordered the window front with
  no visibility check, and `window_state().visible` reported true for a window
  nobody showed. `apply_mode` now guards `makeKeyAndOrderFront:` behind AppKit's
  `isVisible`, matching the `WS_VISIBLE` the Win32 sibling carries across its
  style change.

- **crcbl-vk**: the deletion queue freed a destroyed object one submission after
  it was parked — right for one future submission, a GPU-side use-after-free for
  two: an object recorded into two command buffers was freed when the first
  completed while the second was still queued or running. Command buffers now
  record the raw objects their commands use, and a submission extends the
  retirement of every referenced parked object to its own completion, so a
  destroyed object stays alive until the **last** submission referencing it
  finishes. The retire scan frees every entry whose own key is reached, so an
  extended key cannot hold up a ready successor.

- **crcbl-vk**: a readback whose explicit wait semaphore was destroyed between
  `request_readback` and `poll_readback` was undefined behaviour — the
  completion point was stored as the raw `VkSemaphore` and dereferenced at poll
  time with no liveness check. It is now stored as a generational handle and
  re-resolved through the device pool, exactly like the readback buffer, so a
  destroyed semaphore reports `InvalidHandle` instead.

- **crcbl-vk**: query commands with caller-supplied ranges no longer hand
  out-of-range values to the driver. `reset_query_set`, `write_timestamp` and
  `resolve_query_set` now bounds-check against the pool's query count at record
  time and fail with `InvalidDescriptor`, matching `Device::query_results` and
  the null backend — an over-large range used to be recorded and reached
  `vkCmdCopyQueryPoolResults`/`vkCmdResetQueryPool` as a validation violation.

- **crcbl-server**: a reconnect hello that arrived **after** the grace deadline
  expired the session without marking it terminated, so the next fresh join
  silently re-issued the dead session's token and id — and the departed client
  could still reconnect against the "new" session with its old credential. The
  expiry inside `handle_hello` now sets `session_terminated`, so the fresh join
  rotates to a new session and token.

- **crcbl-client**: a client holding a resume token a restarted server no longer
  recognised retried the stale token forever at capped backoff, wedged at
  "connecting" with no fresh join ever sent. Two consecutive
  `INVALID_SESSION_TOKEN` rejections now drop the token and session id and fall
  back to a fresh token-less join (two rather than one, so a single forged
  reject cannot throw away a still-valid credential).

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
