# Sample 05 — viewer

glTF model viewer. Open a file, orbit it, inspect it. Not a game — a genuinely
usable tool, and the asset pipeline's acceptance test.

## Proves

- Stage 6 asset pipeline against the real world: arbitrary glTF files from DCC
  tools, Sketchfab, the glTF sample-model repo — not just the blessed Sponza.
  Unsupported-feature handling (log-and-skip loudly) gets exercised by files we
  didn't curate.
- Hot reload as a user feature: re-export from Blender, viewer picks it up. This
  is the artist-loop demo.
- Camera controls polish: orbit/pan/zoom/frame-selected — written once here,
  properly, then reused by the stage 8 editor (viewer is the editor viewport's
  warm-up act).
- UI as a tool-building kit (stage 7): file dialog (or drop target), material
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

~~Animation playback (post-MVP engine feature)~~ — **withdrawn**: that engine
feature landed, so the cap was protecting nothing. `crcbl-anim` and
`crcbl_render::skinning` ship, and `apps/viewer/src/anim.rs` converts a
document's first skin and first clip into what `crcbl::anim` poses and samples
it every frame, looping; `B` draws the posed skeleton over the model. The
conversion is the **application's**, deliberately, because `crcbl-anim` does not
depend on the glTF importer. What the cap was actually protecting — that the
viewer does not become an animation _tool_ — still holds: there is no timeline,
no clip selection and no retargeting.

Still capped: material _editing_, export, scene composition (that's the editor),
PBR environment lighting/IBL beyond the single light + exposure.

**Exempt from sample rule 11** (`.crpix` art through the sprite pass): the whole
point is that the viewer shows _the user's_ asset, unadorned. Authored art in
the viewport would be exactly the thing it must not do. Rule 4's debug panel
applies as it does everywhere — this sample is already the tool that dogfoods
the inspector, and the panel is the same surface.

## Milestones

1. Load + orbit + grid (stage 6 exit demo).
2. Panels + debug views (stage 7 exit ladder).
3. Hot-reload-on-reexport demo recorded (doubles as engine marketing).

### Where this stands

**Milestones 1 and 2 are built**, and milestone 3's mechanism is built while its
recording is not. `apps/viewer` takes a path, reads it through the asset seam,
converts it, frames the camera on it, turns it under the mouse and draws it
under a single directional light over a grid floor; `I` shows what the document
holds and what the conversion could not bring in, `W` draws it in wireframe, `N`
in world-space normals, and `-`/`=` and the `ESC` panel's slider step the
exposure. `apps/viewer/src/watch.rs` is the re-export loop: a `stat` four times
a second rather than a filesystem-notification dependency, with a settle delay,
because an exporter writes a `.glb` progressively and every platform API reports
a re-export as a burst that has to be debounced back into one anyway. What
milestone 3 still owes is the **recorded** demo.

**And it runs in a browser**, which the ladder's rule 7 filed as a stretch.
`apps/viewer/src/web.rs` is the `wasm32` front end and `web/demos/viewer/` is
its page. A tab has no path to type and no directory to root an asset source at,
so it opens a document the module generates and compiles into itself — and it
takes one the visitor chooses: a `.glb` or `.gltf` dropped on the canvas is
opened over a `MemorySource`, the same call the built-in document takes. A file
that will not parse keeps the frame that is on screen and puts the loader's own
sentence on the status bar, because a page has no exit code to fail with.

## Exit criteria

- Loads ≥90% of the Khronos glTF-Sample-Models suite without crash; failures log
  actionable messages (file, feature, skip reason).
- Blender → re-export → live update loop works.
- A non-developer can use it (open file, look at model) with zero instructions.
