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
so it opens the shelf's Suzanne over the page's own `fetch()` source (the
document the module generates is the fallback for a site with no shelf) — and it
takes one the visitor chooses: a `.glb` or `.gltf` dropped on the canvas is
opened over a `MemorySource`, the same call the built-in document takes. A file
that will not parse keeps the frame that is on screen and puts the loader's own
sentence on the status bar, because a page has no exit code to fail with.

## Milestone 4 — the PBR showcase (decided 2026-08-30)

**The user's ask, 2026-08-30:** the viewer is the demo that showcases the PBR
material set — not a new sample. It gains three things:

1. **Every way of opening a model.** The command line on run, a drop on the
   window and a drop on the browser's canvas are the three doors, and all three
   exist; a window drop opens through `model::load` — a `DirSource` rooted at
   the file's own directory, so a `.gltf` with its buffers beside it works —
   rather than the browser's one-file `load_bytes`. What this item still lacks
   is X11, where `crcbl-shell` raises no drop event because XDND is
   unimplemented; `docs/backlog.md` carries that.
2. **The full metallic-roughness set rendered.** Base colour is drawn today;
   normal, metallic-roughness-occlusion and emissive pages arrive with
   foundation (a) and the normal-map rung with (d) in
   `docs/plan/43-render-standards.md`'s lighting order, and the viewer on
   Suzanne is that rung's golden. Sun and sky, LTC area lights, the atlas and
   the probe volume all show on this shelf as they land.

**The shelf, as built (2026-08-30):** the `ESC` panel's `SHELF` row lists the
nine models below and Suzanne opens when nothing is asked for, on both hosts.
Only Suzanne is committed — the whole shelf is about 138 MB and the repository
uses no LFS — and `tools/fetch-shelf.sh` fetches the rest at a pinned upstream
commit with a sha256 per file (`apps/viewer/assets/shelf.sha256` is the one file
list; `apps/viewer/src/shelf.rs` is the table). **The browser carries three**:
Suzanne pre-loaded, Avocado and WaterBottle fetched when picked — 18.9 MB
against a 25 MB budget for the demo's assets; the next-smallest model would take
it to 28 MB, so the other six are native-only.

### The licence rule for shipped assets (decided 2026-08-30)

The repository is MIT and its demos are published, so every asset committed to
it is redistributed under terms a downstream MIT user inherits. The rule:

- **CC0 first.** An asset with no obligations is the default; nothing to track,
  nothing a fork can get wrong.
- **CC-BY 4.0 is allowed with attribution** in an `ATTRIBUTION.md` beside the
  asset naming the author, the source URL and the licence, and the same line in
  the demo's page. Not the default, because a fork that drops the file is in
  breach and nothing in the tree would notice.
- **No NC, no SA, no research-only.** A non-commercial clause is incompatible
  with a permissive engine that ships a product; share-alike would relicense the
  demo.
- **Provenance is verified at the source, not remembered.** The licences below
  were read on 2026-08-30 from the Khronos `glTF-Sample-Assets` model table and
  each model's own `README.md`, from `polyhaven.com/license`, and from the
  Stanford scanning repository's terms page, and again on 2026-08-30 from each
  shelf model's `README.md` at the commit `tools/fetch-shelf.sh` pins. Re-read
  before committing a file; an asset's licence is the one on its page that day.

**Decided 2026-08-30, the user:** the model demos use **the CC0 models from
Khronos' `glTF-Sample-Assets` and nothing else.** Not Poly Haven models, not
CC-BY sets with an attribution file, not a modelled-here rabbit — one source,
one licence, nothing to track. Poly Haven stays named below only as the CC0
source for a PBR _texture_ or an HDRI if a rung ever needs one that the Khronos
shelf lacks.

### The models, checked

| Model                                                                                     | Source                          | Licence                                                                 | Verdict                                                                                                                                                                                                               |
| ----------------------------------------------------------------------------------------- | ------------------------------- | ----------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Suzanne** (the monkey)                                                                  | Khronos glTF-Sample-Assets      | CC0-1.0 (UX3D 2017)                                                     | **In.** The one-mesh material fixture; ~8k triangles subdivided, a normal-mapped variant is ours                                                                                                                      |
| Stanford bunny (the rabbit)                                                               | Stanford 3D Scanning Repository | research-only, no commercial use without permission                     | **Out.** "Not to be used for commercial purposes, nor appear in a product for sale" — a redistributed MIT demo is exactly that. Any rabbit here is a different rabbit                                                 |
| Avocado, BoomBox, Corset, Lantern, WaterBottle, BarramundiFish, FlightHelmet, SciFiHelmet | Khronos glTF-Sample-Assets      | CC0-1.0                                                                 | **In**, as the gallery's shelf: full metallic-roughness sets with normal, occlusion and emissive maps, sized for a browser tier                                                                                       |
| AntiqueCamera                                                                             | Khronos glTF-Sample-Assets      | CC0-1.0 plus `LicenseRef-LegalMark-UX3D` on a logo baked into a texture | **Out.** The mark's own text says UX3D "reserves the right to remove the Mark or unilaterally change the terms of use" — the obligation-to-track the rule above exists to avoid. Read 2026-08-30 at the pinned commit |
| DamagedHelmet                                                                             | Khronos glTF-Sample-Assets      | CC-BY 4.0 / CC-BY-NC 4.0 dual                                           | **Out.** The dual licence is a trap for a fork; SciFiHelmet is the same kind of model under CC0                                                                                                                       |
| MetalRoughSpheres                                                                         | Khronos glTF-Sample-Assets      | CC-BY 4.0                                                               | Allowed with attribution; the BRDF ladder's calibration chart, worth the attribution line                                                                                                                             |
| Poly Haven models and textures                                                            | polyhaven.com                   | CC0                                                                     | **In.** The source for any prop or PBR texture set the gallery wants beyond the Khronos shelf, and for an HDRI once the sky can take one                                                                              |
| Duck, BrainStem, CesiumMan                                                                | Khronos glTF-Sample-Assets      | SCEA / Poser EULA / CC-BY                                               | Out, or not worth their terms                                                                                                                                                                                         |

There is no rabbit on the Khronos CC0 shelf, and the Stanford scan is the one
the user meant and the one that cannot ship — so the demos have no rabbit. Named
here so nobody re-derives it.

## Exit criteria

- Loads ≥90% of the Khronos glTF-Sample-Models suite without crash; failures log
  actionable messages (file, feature, skip reason).
- Blender → re-export → live update loop works.
- A non-developer can use it (open file, look at model) with zero instructions.
