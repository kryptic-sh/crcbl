# Topic 37 — Material Authoring

Stage 3 gave materials a runtime shape (a table row + bindless texture indices)
and stage 6 fills it from glTF. What's missing is the **authoring layer**: how a
material is defined, tuned, validated, and hot-reloaded as an asset — and how a
_render_ material relates to the _surface_ properties the rest of the engine
already reads.

## Two materials, one link (the mess this prevents)

The engine has grown two independent notions with the same name:

| Kind                 | Lives on | Consumers                                                                                                            |
| -------------------- | -------- | -------------------------------------------------------------------------------------------------------------------- |
| **Render material**  | mesh     | shading, textures, transparency, decal receipt (33)                                                                  |
| **Surface material** | collider | audio occlusion + footsteps (13), nav flags (24), ballistics (28), friction/restitution (36), decal/impact sets (33) |

They're genuinely different (visual vs physical) but almost always correspond —
a concrete wall should _look_ like concrete and _sound, stop bullets, and skid_
like concrete. So:

- **Linked, not merged**: a render material asset declares an optional
  `surface: "concrete_rough"` reference; the surface material remains its own
  asset on colliders (many colliders share one, invisible geometry has one with
  no render side).
- **Validation is the payoff**: `crcbl mat check` reports render materials with
  no surface link, surface/render pairs that disagree (a "metal" render material
  pointing at the `wood` surface), and unused surfaces. The classic content bug
  — a wall that looks like brick and sounds like tin — becomes a lint error
  instead of a playtest report.

## Templates and instances (not a node graph)

The Unreal material/material-instance split, minus the graph:

- **Material template** = a Slang shader program (2) + a declared parameter
  block (scalars, colors, texture slots, feature flags). Authored by
  programmers, versioned in the repo, compiled through the existing shader
  pipeline. Examples: `standard_pbr`, `foliage_alpha`, `glass`, `emissive`,
  `terrain_blend`, `decal`.
- **Material instance** = a **RON asset**: pick a template, set parameter values
  and texture references, optionally inherit from another instance (one level of
  parenting — child overrides a subset). This is what artists and designers
  author, and it **hot-reloads** through the stage 6 watcher.
- Runtime: an instance resolves to one **material table row** (parameters
  packed) + bindless texture indices — exactly the stage 3 layout, unchanged.
  Authoring changed; the GPU path did not.
- `standard_pbr`'s first three parameters already exist in that row and are
  already shaded: `crcbl_shaders::mesh::GpuMaterial` carries `base_color`,
  `metallic` and `roughness`, and `mesh.slang` runs one GGX lobe on them — see
  `docs/plan/18-render-features.md`'s BRDF decision. This topic owns how a
  parameter block is _declared and authored_, not which parameters the row
  holds; a template that adds one is what makes the row grow.
- glTF import (6) emits instances of `standard_pbr` — imported content and
  hand-authored content are the same kind of asset from that moment on.

**No node graph.** It's the biggest single feature in a material system and the
one most easily deferred: templates cover the shader variety a game actually
ships, and a graph can compile _into_ a template later without invalidating a
single instance asset. Gated on demonstrated demand, like every other
someday-feature here.

## Permutations, honestly

Feature flags (alpha-test, two-sided, skinned — 17, carveable — 33,
vertex-color, detail-layer) multiply into shader permutations, and combinatorial
explosion is the classic way material systems become unbuildable:

- Permutations are **declared, not inferred**: a template lists which flags it
  supports; the build enumerates only reachable combinations from what instances
  actually use (asset-driven, computed at bake).
- Permutation count is a reported build statistic with a budget — growth is
  visible in CI, not discovered when a build takes an hour.
- Runtime pipeline lookup by (template, permutation) hits the pipeline cache
  from stage 2.

## Blending and layering (small, deliberate)

- **Detail layer**: a second tiled texture set blended by mask/distance — the
  cheap trick that makes surfaces hold up close, standard on shooter maps.
- **Vertex-color blend**: 2–4 way blending for terrain-ish and modular
  architecture, driven by mesh vertex colors.
- Anything beyond that (full layered-material stacks, procedural noise chains)
  waits for the node graph question to be answered by need.

## Tooling

- **Editor material panel** (stage 8 growth): template picker, parameter widgets
  generated from the declared block (sliders/colors/texture pickers — the same
  reflection the entity inspector uses), live preview sphere/plane,
  surface-material link picker, instance parenting UI.
- **Hot reload**: edit a `.mat.ron`, see it in the running app — the same loop
  the CSS stylesheets have (7), which is the standard this engine holds itself
  to.
- **CLI** (11): `crcbl mat list|check|preview` — preview renders an instance
  offscreen on standard test geometry at fixed lighting (the golden-frame source
  and the "what does this actually look like" answer for agents).

## Testing (12)

- Golden frames per template × representative instances × lighting presets.
- Parameter roundtrip: instance → packed table row → shader read, values intact
  (catches packing/alignment bugs, the silent-corruption class).
- Inheritance property: child overrides exactly the declared subset.
- Permutation build test: every reachable permutation compiles; count stays
  under budget.
- `mat check` lint has its own fixture set (missing textures, broken surface
  links, mismatched pairs).

## Delivery

| Slice                                                      | Phase                                                              |
| ---------------------------------------------------------- | ------------------------------------------------------------------ |
| Template declaration + instance RON assets + table packing | P9 (with assets/scenes — imported glTF materials become instances) |
| Surface-material link + `crcbl mat check` lint             | P9                                                                 |
| Hot reload + permutation manager/budget reporting          | P9–P10                                                             |
| Editor material panel + preview + CLI `mat preview`        | P12                                                                |
| Detail layer + vertex-color blend                          | wave 1 (map-quality era: towers/breach)                            |
| Node graph                                                 | on demonstrated demand only                                        |

## Risks

- **Node-graph pull**: the single most requested material feature and the single
  largest. The template/instance split is the hedge — a graph becomes a template
  _generator_ later, not a rewrite.
- **Permutation explosion**: declared flags + asset-driven enumeration + a
  CI-reported budget; the failure mode is caught by a number, not by a slow
  build.
- **Two-materials confusion**: mitigated by the link + lint; docs state plainly
  that render and surface are separate assets on purpose.
