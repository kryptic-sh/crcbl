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

## Status: not started, and it cannot start yet (2026-08-13)

**There is no `apps/lumen`, and building one today would not be this sample.**
The blocker is not SSR or irradiance probes — those are merely unbuilt P7B rows.
It is that **the engine has no way for an app to describe a scene**, so the room
the Scope section asks for has no representation.

What `crcbl::render::ForwardRenderer` offers an app is a **fixed resident set**,
not a scene: `begin_frame` takes the cube's transform as an argument, and
`set_pyramid`, `set_tinted_pyramid`, `set_textured_pyramid`, `set_open_box` and
`set_dunes` place four more instances of meshes the renderer uploaded to itself
in `new`. Each names one of three material rows the renderer also owns. There is
no mesh-upload call, no instance call, and no material call above the pools —
`MeshPool`, `InstancePool` and `MaterialTable` are public, but composing them
means an app writing a second forward renderer, which sample rule 1 exists to
forbid. `crcbl-scene`'s glTF importer is not reachable either: it is not a
dependency of `crcbl`.

So, against the Scope section: the window, the mirror-grade surface, the rough
metal surface and the coloured bounce wall each need geometry or a material this
sample cannot author, and the per-effect toggles need switches `add_passes` does
not have — it records the shadow, depth-prepass, light-grid, SSAO and SSAO-blur
passes unconditionally. Only the moving light is buildable today (`set_lights`
takes an arbitrary `&[Light]`), along with the free-fly camera, the debug panel
and rule 12's path reporting.

**The roadmap already says this and the deliverable column contradicts it.**
`docs/plan/ROADMAP.md`'s phase table orders S4B _after_ **P9** — "Assets +
scenes: `AssetSource`, glTF import, RON scene format, hot reload; material
templates + instances" — which is the phase that builds exactly what is missing.
P7B's deliverable column nonetheless reads "lumen (13) renders the scene
complete on `LightingPath::Rasterised`". Both cannot hold. The phase ordering is
the one supported by the code: **lumen is a P9-dependent sample**, and P7B needs
a different exit gate or a stated dependency on P9.

`docs/backlog.md` carries the engine work item, the option sizing, and the
deferred web demo.

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
