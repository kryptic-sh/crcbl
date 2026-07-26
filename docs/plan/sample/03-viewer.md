# Sample 03 — viewer

glTF model viewer. Open a file, orbit it, inspect it. Not a game — a genuinely
usable tool, and the asset pipeline's acceptance test.

## Proves

- Stage 5 asset pipeline against the real world: arbitrary glTF files from DCC
  tools, Sketchfab, the glTF sample-model repo — not just the blessed Sponza.
  Unsupported-feature handling (log-and-skip loudly) gets exercised by files we
  didn't curate.
- Hot reload as a user feature: re-export from Blender, viewer picks it up. This
  is the artist-loop demo.
- Camera controls polish: orbit/pan/zoom/frame-selected — written once here,
  properly, then reused by the stage 7 editor (viewer is the editor viewport's
  warm-up act).
- UI as a tool-building kit (stage 6): file dialog (or drop target), material
  list, node/mesh tree, stats panel — the first UI-heavy app that isn't the
  debug overlay itself.

## Scope

- Load glTF/glb via `AssetSource`; drag-drop or path argument.
- Orbit camera, frame-on-load, grid floor, single directional light + exposure
  slider.
- Panels: mesh/material/texture listing with sizes, triangle counts, GPU pool
  occupancy (reuses debug stats); wireframe and normals-debug view toggles.
- Server-authoritative rule applies loosely here: viewer is client-only by
  charter exception — it simulates nothing. Documented as the one sanctioned
  exception (a tool, not a game; rule 2 exists for games).

## Non-goals (hard cap)

Animation playback (post-MVP engine feature), material _editing_, export, scene
composition (that's the editor), PBR environment lighting/IBL beyond the single
light + exposure.

## Milestones

1. Load + orbit + grid (stage 5 exit demo).
2. Panels + debug views (stage 6 exit ladder).
3. Hot-reload-on-reexport demo recorded (doubles as engine marketing).

## Exit criteria

- Loads ≥90% of the Khronos glTF-Sample-Models suite without crash; failures log
  actionable messages (file, feature, skip reason).
- Blender → re-export → live update loop works.
- A non-developer can use it (open file, look at model) with zero instructions.
