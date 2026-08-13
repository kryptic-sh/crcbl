# Sample 13 — lumen (S4B, gates P7B–P7C)

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

## Status: milestone 1a, and the blocker is gone (2026-08-14)

**`apps/lumen` exists and renders the charter's room.** The blocker this section
used to record — "the engine has no way for an app to describe a scene" — was
answered by six slices in `crcbl-render`: the resident set is a `SceneDesc` an
application writes, instances are `add_instance` / `set_instance` /
`remove_instance`, `begin_frame` places nothing of its own, and `crcbl-scene`'s
meshlet builder is reachable through `crcbl`'s non-default `scene` feature so an
app can bake a mesh. `docs/backlog.md` carries what that left owed.

### What milestone 1a delivers

- **The room the Scope section names**, described by the sample rather than by
  the engine: `crcbl_lumen::room` bakes nine meshes from literals through
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
- **A golden frame with five structural claims in front of it**
  (`apps/lumen/tests/golden.rs`): the sun reaches the floor through the opening
  and not beside it, the shaded floor is ambient rather than black, a conductor
  has no ambient term, and the coloured wall's base-colour factor reached the
  fragment stage. Each is a ratio between two blocks of pixels, and each is
  re-run at twenty-five times the pixel count so it is a claim about the room
  rather than about the sampling.

### Two surfaces look broken and are not

**The mirror-grade panel and the rough metal block render near-black.** Ambient
scales the _diffuse_ albedo and a conductor has none, so a fully metallic
surface out of every light's specular reach has nothing left to shade with —
[18-render-features.md](../18-render-features.md) is where the model is argued.
What fills it in is a reflection, and both screen-space reflections and
irradiance probes are unbuilt. Nothing here fakes it: the debug panel's
`unbuilt` section says so on the screen where the black is, and
`crcbl_lumen::room`'s module docs say it where a reader of the scene will find
it.

**The coloured wall does not bounce**, for the neighbouring reason. It is a
coloured wall taking a low sun and a warm lamp; what a bounce would do to the
room is milestone 3's picture. The fixed camera deliberately puts a floor in
full sun beside a wall in shadow, which is the configuration global illumination
would change most.

### Still owed at this milestone, and where

Recorded in `docs/backlog.md` rather than here: screen-space reflections,
irradiance probes, ray tracing and the acceleration structures, the
render-to-texture monitor camera, per-effect toggles for shadows and ambient
occlusion (so there is no shadows-off or AO-off frame to compare against yet),
the `[engine.video]` and programmatic-override layers of the toggle resolution
order, the Pages web demo, and a CI leg that runs the golden suite.

## Milestones

1. Scene + raster path complete: shadows for every light type, SSAO, SSR,
   irradiance probes (P7B proof).
2. Acceleration structures + ray-traced shadows and AO (P7C).
3. Ray-traced reflections and GI; side-by-side and A/B-flip modes.
4. Toggle matrix across all three layers; forced-path runs; Pages demo.

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
