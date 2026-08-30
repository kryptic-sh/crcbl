# Sample 21 — gallery (S4F, gates the PBR material set)

Two demos on open models with real PBR material sets, so the rendering rungs
from foundation (a) onward have a picture that is not a greybox: **gallery** — a
turntable of textured models under the sun and sky — and **suzanne**, the
one-mesh material fixture. The user's ask, 2026-08-30: "new demos that use the
open-source rabbit and monkey models with PBR materials, or other suitable
models that work with our MIT licence".

## The licence rule for shipped assets (decided 2026-08-30)

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
  Stanford scanning repository's terms page. Re-read before committing a file;
  an asset's licence is the one on its page that day.

**Decided 2026-08-30, the user:** the model demos use **the CC0 models from
Khronos' `glTF-Sample-Assets` and nothing else.** Not Poly Haven models, not
CC-BY sets with an attribution file, not a modelled-here rabbit — one source,
one licence, nothing to track. Poly Haven stays named below only as the CC0
source for a PBR _texture_ or an HDRI if a rung ever needs one that the Khronos
shelf lacks.

## The models, checked

| Model                                                                                                    | Source                          | Licence                                             | Verdict                                                                                                                                                               |
| -------------------------------------------------------------------------------------------------------- | ------------------------------- | --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Suzanne** (the monkey)                                                                                 | Khronos glTF-Sample-Assets      | CC0-1.0 (UX3D 2017)                                 | **In.** The one-mesh material fixture; ~8k triangles subdivided, a normal-mapped variant is ours                                                                      |
| Stanford bunny (the rabbit)                                                                              | Stanford 3D Scanning Repository | research-only, no commercial use without permission | **Out.** "Not to be used for commercial purposes, nor appear in a product for sale" — a redistributed MIT demo is exactly that. Any rabbit here is a different rabbit |
| Avocado, BoomBox, Corset, Lantern, WaterBottle, BarramundiFish, AntiqueCamera, FlightHelmet, SciFiHelmet | Khronos glTF-Sample-Assets      | CC0-1.0                                             | **In**, as the gallery's shelf: full metallic-roughness sets with normal, occlusion and emissive maps, sized for a browser tier                                       |
| DamagedHelmet                                                                                            | Khronos glTF-Sample-Assets      | CC-BY 4.0 / CC-BY-NC 4.0 dual                       | **Out.** The dual licence is a trap for a fork; SciFiHelmet is the same kind of model under CC0                                                                       |
| MetalRoughSpheres                                                                                        | Khronos glTF-Sample-Assets      | CC-BY 4.0                                           | Allowed with attribution; the BRDF ladder's calibration chart, worth the attribution line                                                                             |
| Poly Haven models and textures                                                                           | polyhaven.com                   | CC0                                                 | **In.** The source for any prop or PBR texture set the gallery wants beyond the Khronos shelf, and for an HDRI once the sky can take one                              |
| Duck, BrainStem, CesiumMan                                                                               | Khronos glTF-Sample-Assets      | SCEA / Poser EULA / CC-BY                           | Out, or not worth their terms                                                                                                                                         |

There is no rabbit on the Khronos CC0 shelf, and the Stanford scan is the one
the user meant and the one that cannot ship — so the demos have no rabbit. Named
here so nobody re-derives it.

## What each demo proves

**suzanne** — the material fixture. One mesh, the full metallic-roughness set
(base colour, normal, metallic-roughness-occlusion, emissive), a sun, the sky,
and the turntable. It is the golden for the normal-map rung, the specular-IBL
rung already built, LTC area lights when they land, and the tier table's
material rows. Small enough that every backend and the browser tier draw it in
the gate at full size.

**gallery** — the shelf. Six to eight CC0 models on plinths under the sun and
sky, a fly camera and a turntable per model, the options panel reachable so the
tier table is exercised on real materials. Proves the page allocator (d) with
many texture sets resident at once, the cluster DAG on a 70k-triangle helmet,
and the shadow atlas allocator on a shelf of small occluders. Its browser
artifact is the size budget's acceptance test: the shelf is chosen so the wasm
plus assets stays inside the budget the web build already tracks.

## Order

Both wait on **foundation (a) vertex v2** (the QTangent the normal map needs)
and the **normal-map rung** with **(d) the page allocator** — the first two
items of `docs/plan/43-render-standards.md`'s lighting order. `suzanne` lands
with the normal-map rung as its fixture; `gallery` follows when the page
allocator holds more than one texture set. Both join every list a new demo joins
(`docs/backlog.md`'s "a new demo joins eight lists").

## Not in these

- Animation: `puppet` owns skinning; the gallery's models are static.
- A model picker over arbitrary files: that is `viewer`, the tool. The gallery
  ships a fixed shelf so its goldens hold.
- HDRI lighting: the sky is the gradient or the atmosphere; an image-based sky
  is a later rung in `43-render-standards.md` §8.
