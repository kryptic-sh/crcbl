# Sample 13 — lantern (S4B, gates P7B–P7C)

Lighting acceptance test and the living fixture for topic 18. One scene,
rendered under both `LightingPath` values, with every effect toggleable
independently. Not a game — the lighting is the content.

**This is the sample that makes graceful degradation checkable.** Ray tracing is
Vulkan and D3D12 only; macOS, iOS and every browser render the rasterised twin
([39-capabilities.md](../39-capabilities.md)). A raster path nobody looks at
carefully is a raster path that ships broken to most of the audience, and this
sample exists so that "the fallback also works" is something a human has seen
rather than something the plan asserts.

## Proves

- **Both lighting paths draw the same scene**, and a human has compared them.
  Side-by-side and A/B-flip modes; the paths are not expected to match pixel for
  pixel, and a scene that reads correctly on one and wrongly on the other is a
  defect in whichever is wrong.
- **Every effect toggles independently**, on both paths: GI, reflections,
  shadows (all light types), ambient occlusion. Each toggle exercises all three
  layers of the resolution order — the camera stack, `[engine.video]`, and a
  programmatic override — because a toggle that only works from one of them is a
  finding about the resolution point.
- **Degradation is observable and monotonic.** A forced-path flag runs the whole
  scene on any selector combination the device can express; the debug panel and
  the headless summary both name what was selected and why, and the downgrade
  log line is asserted by the e2e rather than admired.
- **One material model.** The same material table and BRDF feed both paths — a
  material authored once looks like itself under either, which is the property
  that keeps two lighting paths from becoming two renderers.
- **Acceleration structures behave**: BLAS built at bake, TLAS refit per frame
  from the same instance data the cull pass reads, and the panel shows build
  cost and refit cost separately.

## Scope

- One indoor scene chosen for lighting rather than for geometry: a room with a
  window, a mirror-grade surface, a rough metal surface, a coloured bounce wall,
  and a moving light. Enough for GI, reflections and shadows to each have an
  obvious right answer.
- Free-fly camera, a fixed camera set for goldens, and a second
  render-to-texture camera driving an in-scene monitor — the per-camera toggle
  layer needs a consumer, and a monitor that does not reflect itself is it.
  **Delivered**: `room::monitor_camera` stands on the screen's own face,
  `room::MONITOR_STACK` drops the reflections, and the picture is copied into
  the page layer the screen samples.
- Path/effect control UI, and the same controls reachable from `[engine.video]`
  so the settings-screen path is exercised before P10 builds a screen.
- Pages web demo. It runs `Rasterised` by construction, which is the point: the
  browser build is the raster path's largest audience.

## Non-goals (hard cap)

Gameplay of any kind, a second scene, authoring tools (the workbench pattern
belongs to sparks and hud), physically-measured validation against a reference
renderer, denoiser research. Effects beyond what topic 18 ships — no requests
smuggled in as "the lighting demo needs it".

**Exempt from sample rule 11** (`.crpix` art through the sprite pass): the
subject is 3D lighting and pixel art in front of it would be showing the wrong
system. Rule 4's debug panel and rule 12's path reporting both apply, and a
lighting fixture without them is not a fixture.

**Exempt from sample rules 2 and 10** (client/server authority, gameplay through
`GameModule`), on the same ground the viewer is: both rules exist so a _game_'s
state lives on the server and its logic lives in module code, and there is no
game state here. The room's geometry is fixed, the lights follow the clock, and
the camera is the viewer's own. So this crate opens no `World`, registers no
system and implements no `GameModule`, and their absence is the charter's answer
rather than an oversight.

## Status: milestone 1a, and the blocker is gone (2026-08-14)

**`apps/lantern` exists and renders the charter's room.** The blocker this
section used to record — "the engine has no way for an app to describe a scene"
— was answered by six slices in `crcbl-render`: the resident set is a
`SceneDesc` an application writes, instances are `add_instance` / `set_instance`
/ `remove_instance`, `begin_frame` places nothing of its own, and
`crcbl-scene`'s meshlet builder is reachable through `crcbl`'s non-default
`scene` feature so an app can bake a mesh. `docs/backlog.md` carries what that
left owed.

### What milestone 1a delivers

- **The room the Scope section names**, described by the sample rather than by
  the engine: `crcbl_lantern::room` bakes nine meshes from literals through
  `crcbl::scene::build_meshlets`, declares five material rows and a two-layer
  page, and sizes its own `Capacities`. A window the sun comes through, a
  mirror-grade panel, a rough metal block, a coloured wall, a moving point
  light.
- **Both cameras.** A fixed pose the goldens are taken from, and a keyboard
  free-fly camera that starts at it; the pause menu's `CAMERA` row swaps them
  and returns the free one to the golden pose.
- **Rule 4's debug panel**, with three sections of the sample's own: the
  selected paths, what is unbuilt, and where the camera is.
- **Rule 12's path reporting**, in the panel and in the headless summary line,
  with `--force-geometry` and `--force-binding` opening a device without the
  features that select anything better. `IndirectPerBatch / ArrayPages` — the
  browser's shape — runs on this desktop.
- **Rule 12's other half: a second path actually drawn** (2026-08-14). Saying
  which path a frame took is not the same as having taken more than one, and
  until now every frame the golden suite drew came off the best tail the adapter
  reports. `the_room_draws_the_same_on_a_path_below_the_devices_own` draws the
  room twice through `OffscreenSetup::open_forward_with` — once on the adapter's
  own selectors, once with the features that select anything above `Forced`'s
  floor withheld — and holds **both arms to the one golden**: a lesser path is a
  constraint on data layout rather than a separate renderer, so a difference
  between the arms is a bug in the better path and a per-path reference is what
  would bless it. The subtraction is done by `Forced::optional_features`, the
  same function `--force-*` goes through, so it cannot drift from the flags a
  selector reads. Measured: radv resolves the arms to `MeshShader / Bindless`
  and `IndirectPerBatch / ArrayPages`, and llvmpipe reports mesh shading and
  bindless too, so the lavapipe leg of `vk e2e` draws the same pair — the arms
  are asserted to differ exactly when the adapter offers one of the withheld
  flags, so a device already at the floor would be a checked claim rather than a
  silent skip.
- **Every effect toggles independently, and there are frames to prove it**
  (2026-08-14). `--no-shadows`, `--no-ao` and `--no-reflections` drive the
  programmatic layer of topic 39's resolution order; the panel's `paths` section
  and the headless summary both name the **resolved** set, not the requested
  one. `every_effect_toggles_and_the_frame_says_so` renders four states at
  1280×960 and makes each claim as a pair of blocks over a pair of frames — a
  block the effect works on and a control block it does not touch, so a frame
  that merely got brighter fails. Measured on radv: the shadowed floor goes 51.0
  → 141.3 with the atlas off while the sunlit floor does not move, the plinth's
  contact corner goes 51.5 → 58.9 with occlusion off while open floor does not
  move, and the mirror panel's foot goes 29.8 → 1.3 with the march off while the
  part of the same face that reflects nothing goes 20.1 → 0.0 — that part is the
  probe environment, which the reflection pass is also what supplies.
- **The effect matrix from the pause menu, and not only from the command line**
  (2026-08-14). Three rows — `SHADOWS`, `AO` and `REFLECTIONS`, the words the
  `--no-*` flags already use — each labelled with what the frame draws and each
  swapping it on ENTER, so holding a lit room against an unlit one is a keypress
  rather than a restart of the fixture. A row is read-modify-write on the
  **programmatic** layer and nothing else (`crcbl_lantern::toggled_effect`): it
  leaves the camera-stack and `[engine.video]` fields as it found them, which is
  what stops a panel silently discarding a decision it was never asked about
  once either of those gains a source, and it is the one layer that can move a
  decision upward, so a row turns shadows back on after `--no-shadows`. What a
  row _shows_ is the **resolved** answer — `EffectRequest::resolve` against what
  the device permits — and an effect the device cannot draw reads `UNAVAILABLE`
  rather than `OFF`, so the panel never offers a tick that does nothing.

  **This is milestone 4's matrix.** Every effect is reachable from the
  programmatic layer, from the menu and from the command line; the camera stack
  has a source — the in-scene monitor's view asks for `room::MONITOR_STACK`,
  which is every effect but the reflections, through a `ForwardRenderer` of its
  own; and `[engine.video]` has one too, read by `apps/lantern/src/gpu.rs`'s
  `video_effects` call and folded with the other two by `request_for`, which
  `the_players_video_clamp_reaches_both_views` holds against both views. No
  device here clamps an effect either, so the `UNAVAILABLE` arm is covered by a
  unit test that constructs the device set —
  `a_row_the_device_cannot_draw_reads_as_unavailable` — rather than by a machine
  that reports one.

- **A golden frame with six structural claims in front of it**
  (`apps/lantern/tests/golden.rs`): the sun reaches the floor through the
  opening and not beside it, the shaded floor is ambient rather than black, a
  conductor reflects the probe environment where a screen-space ray finds
  nothing, the reflection follows the geometry down to the panel's foot, the
  coloured wall's base-colour factor reached the fragment stage, and that wall
  tints the plaster beside it. Each is a ratio between two blocks of pixels, and
  each is re-run at twenty-five times the pixel count so it is a claim about the
  room rather than about the sampling.

### Two surfaces are lit by reflection alone, and that is the model

**Neither metal surface has an ambient term.** Ambient scales the _diffuse_
albedo and a conductor has none, so a fully metallic surface out of every
light's specular reach has nothing left to shade with —
[18-render-features.md](../18-render-features.md) is where the model is argued.
What fills it in is a reflection, and both surfaces now get one. Screen-space
reflections light the mirror panel's **foot**, where a reflected ray still finds
the floor on screen; everywhere else on that face the march finds nothing and
returns the irradiance-probe volume as its environment, and the rough block,
whose roughness is above `ROUGHNESS_CUTOFF`, takes that environment directly
without marching at all. Measured on radv at the golden's own 256×192,
`zero_probes_only_remove_the_ssr_and_rough_fallbacks` reads the panel's
reflecting-nothing point at 20.3 with authored probes and 0.0 with the rows
zeroed, its foot at 51.6 and 49.0, and the brass block's camera-facing face at
97.4 and 89.7 — so the panel's upper face is probe data outright, its foot is a
real screen hit, and the block is mostly the sun's own specular with the
environment on top.

What is still owed there is that the environment is a **probe grid** rather than
a trace: a blurry low-frequency field, and the only answer this path has for
anything outside the frame. Ray tracing is what replaces it. Nothing here fakes
it — the debug panel's `unbuilt` section says so on the screen, and
`crcbl_lantern::room`'s module docs say it where a reader of the scene will find
it.

**The coloured wall bounces**, and since 2026-09-04 the engine is how, not the
sample: `crcbl_lantern::bounce` only _places_ the probes from the room's own
dimension constants and ships their rows zeroed with `ProbeUpdate::EveryFrame`,
and `crcbl_render`'s reflective-shadow-map updater fills them every frame from
the sun's near cascade and the lamp's shadow faces, each sample gated by the
probe's captured visibility. The analytic one-box gather the module used to bake
at load is gone with `docs/plan/50-irradiance-probes.md`'s no-bake rule — what
the rows hold now sees the plinth, the panel, the block and the post as
occluders, which the box never could. It is still one bounce and no history, and
the fixed camera still deliberately puts a floor in full sun beside a wall in
shadow, which is the configuration a second bounce would change most.

### Still owed at this milestone, and where

Recorded in `docs/backlog.md` rather than here: ray tracing and the acceleration
structures. All three layers of the toggle resolution order have sources now —
the programmatic one drives both the `--no-*` flags and the pause menu's rows,
the camera one is the in-scene monitor's, and `[engine.video]` reaches the
request through `gpu.rs`'s `video_effects` read. The monitor itself left
findings in `crcbl-render` — duplicate imports, an undeclared page read, one
view per renderer and one view per offscreen run — all of them in the backlog.

**Two things this list used to carry are built, and saying they are not was the
worse error.** The **Pages web demo** exists: `apps/lantern/src/web.rs` is the
`wasm32` front end, `web/demos/lantern/` is its page, and `lantern` is a row in
`web/build.sh`'s `DEMOS` array — which is the charter's own reason for wanting
it, since a browser has no ray query and the page is therefore the one place
`LightingPath::Rasterised` can be looked at without building anything. And the
**CI leg runs**: `.github/workflows/ci.yml` has a
`Draw lantern's room on lavapipe` step running
`apps/lantern/tests/run-lantern-golden.sh` under the validation layers. Its own
comment records why it was added — the golden existed and nothing ran it, so it
passed on three configurations on the author's machine and on none in CI, which
is a check that cannot fail where it matters.

## Milestones

**1 and 4 are built** — the sections above are the record of each. What is left
is the ray-traced half, which waits on the acceleration structures the backlog
carries.

1. ~~Scene + raster path complete: shadows for every light type, SSAO, SSR,
   irradiance probes (P7B proof).~~
2. Acceleration structures + ray-traced shadows and AO (P7C).
3. Ray-traced reflections and GI; side-by-side and A/B-flip modes.
4. ~~Toggle matrix across all three layers; forced-path runs; Pages demo.~~

## Exit criteria

- Every topic 18 effect has a golden frame **per lighting path** in CI, plus the
  human-reviewed pair-wise comparison recorded in the sample doc.
- Forcing any selector combination the device supports renders the scene without
  a crash, a missing surface, or an unlogged downgrade.
- The web demo renders the complete raster picture — no effect silently absent,
  no black surface where a ray-traced one would be.
- A material edited once is correct under both paths, demonstrated by the
  side-by-side rather than argued.
- Recorded budget for both paths at a stated resolution, and for the browser.
