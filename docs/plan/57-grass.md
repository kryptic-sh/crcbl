# Topic 57 — Grass and vegetation wind: cards, blades, shells, and trees that sway

Written 2026-09-15, from a survey of the tree and a research brief on how
shipped games and Acerola's grass and fur series draw vegetation. **Rungs G1
(2026-09-16), G2 and G3 (2026-09-17) are built; nothing else in this document
is.** Its place in the set is [18-render-features.md](18-render-features.md)'s
index; the wind every rung reads is [56-wind.md](56-wind.md)'s; the shell
technique it shares with fur is [58-hair.md](58-hair.md)'s; the fixture that
proves it is [sample/22-meadow.md](sample/22-meadow.md).

**One grass system with three looks, not three grass systems.** Card grass, mesh
blades and stylised shells differ in the geometry a blade is drawn with; they
share placement, streaming, culling, lighting, wind and interaction. A field
that switches from cards to blades should change one description and move
nothing else, and that is the test of whether the system is first class.

## Where this is

Absent, and the tree has a few pieces to build on and a few constraints that
decide the shape:

- **Alpha-mask materials exist** (`GpuMaterial::ALPHA_MODE_MASK`,
  `crates/crcbl-shaders/src/mesh.rs`), landed for foliage in
  [53-transparency.md](53-transparency.md)'s first step. Blended grass does not
  exist and is not wanted.
- **MSAA is in the seam and off by default.** `MultisampleState` in
  `crates/crcbl-hal/src/pipeline.rs` carries `samples` and `alpha_to_coverage`;
  the antialiasing ladder ([rendering notes](../notes/rendering.md)) prices MSAA
  and keeps it off because SSAO and SSR read single-sample depth. So
  alpha-to-coverage — the antialiased cutout every card-grass reference relies
  on — is available only on a view that pays for MSAA.
- **The instance pool is not a grass pool.** `ForwardRenderer`'s instances are
  full transforms culled and bucketed by `cull.slang` and `draw_gen.slang`, and
  `Capacities` (`crates/crcbl-render/src/scene.rs`) sizes them once. A field of
  a million blades does not belong in it.
- **The forward vertex stage already binds every storage buffer a browser
  guarantees.** `MeshVertex`'s documentation says so: the raster path binds
  `PORTABLE_STORAGE_BUFFERS_PER_STAGE` storage buffers in its vertex stage, "so
  a ninth is a renderer that cannot be built in a browser". A tree's wind data
  therefore cannot arrive as a new vertex stream; it has to ride in lanes that
  exist or in a sampled texture.
- **`MeshVertex` has two unused lanes and a colour.** `uv1` is in the layout and
  "nothing fills them and nothing reads them yet"; `color` is `rgba8`. Crysis
  baked its vegetation wind into exactly a vertex colour.
- **Deformed geometry and culling have met before.** Commit e05cb9f kept skinned
  instances from being rejected by source bounds and normal cones that cannot
  see vertices a palette moved; wind-moved vertices raise the same question.
- **There is no terrain system.** Grass needs a ground height and normal; a
  field takes them from a heightfield the caller supplies until one exists.
- **Lighting is `mesh.slang`'s, and there is no `#include`.** A grass shader
  that is lit by the clustered lights and the cascades copies that walk, and
  `volumetric.slang` already has such a copy guarded letter for letter against
  its source (the rule is in the [rendering notes](../notes/rendering.md));
  grass takes the same guard.

## The decisions

### 1. Grass is its own pass over compute-generated instances

Ghost of Tsushima rejected cards partly because the wind moved the whole card at
once, drew blades from an instanced indexed draw with no vertex streams, and
generated each render tile's instances in one compute dispatch that culls and
appends — "just over 1 million blades" considered, about 83,000 drawn, "each
individually animating … about two and a half milliseconds end to end"
(Wohllaib, GDC 2021). Acerola's fields are the same shape. So:

- **`crcbl-render` gains a `grass` module**: its own compute generation, its own
  instance buffers, its own pass recorded beside the opaque forward pass so it
  shares depth, fog, shadows and the light grid. Grass is opaque with a cutout;
  it is in the depth prepass (Horizon Zero Dawn's depth-only prepass then an
  equal-depth pass: "zero percent overdraw").
- **Fixed indirect slots, one per tile and LOD**, with compute writing each
  slot's instance count and zero for an empty one —
  [53-transparency.md](53-transparency.md)'s decision 2 shape, and the one that
  works without an indirect-count draw. WebGPU requires `firstInstance` to be
  zero unless `indirect-first-instance` is enabled, so each slot binds its own
  buffer offset, and its 128 MiB default storage binding size means one instance
  buffer per ring rather than one for the field (Acerola's 7.3 million blades
  came to 204 MB per copy).
- **Appends are lock-free** with WGSL's `atomicAdd`, which returns the original
  value — Ghost of Tsushima's per-lane append.
- **No motion vectors.** Ghost of Tsushima wrote none for grass, and said grass
  "makes it a poor target for temporally accumulated effects"; this engine
  refuses temporal accumulation anyway.

### 2. Placement is a deterministic function of the tile

- **A field** is a description: bounds, a ground heightfield (texture plus CPU
  copy), a density map and a type map, and a table of blade types.
- **Lanes**: each compute thread's id becomes a grid position plus a hash
  jitter, then distance, frustum and occlusion culls, then the type and height
  samples (Ghost of Tsushima). The type map is sampled with a gather and a
  position-weighted pick among the four texels, which dithers a transition.
- **Clumps** are procedural Voronoi cells over the nine nearest hash-jittered
  points, driving height, a shared facing and colour.
- **Horizon Zero Dawn's ordered-dither placement** ("deterministic, locally
  stable") is the alternative for authored layers that must not collide.
- **Gameplay never reads animated grass.** Ghost of Tsushima's stealth grass
  returns "a constant height per type … for consistency". The CPU answers "is
  this position in grass, and how tall" from the static maps; everything that
  moves is visual only.

### 3. Three looks, one blade description

| Look   | Geometry                                                                                                                            | Precedent                                                                         | Where it wins                                                              |
| ------ | ----------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Cards  | cross-quads or camera-facing billboards per clump, alpha cutout                                                                     | Acerola's billboard field; Horizon's grass assets                                 | cheapest; distant fill; the software and browser tiers                     |
| Blades | a cubic Bézier per blade built in the vertex stage from `vertex_index`, 15 vertices near and 7 far                                  | Ghost of Tsushima; Acerola's "Modern Foliage Rendering"; Outerra's 7-vertex strip | the realistic near field                                                   |
| Shells | instanced layers over a surface, each discarding outside a hashed per-cell strand that tapers with height, plus fins at silhouettes | Acerola's shell texturing, first as a grass field and then as fur                 | stylised lawns, moss and short grass; shared with [58-hair.md](58-hair.md) |

**Cards**: Castaño's coverage-preserving alpha mipmaps are cooked at build time
so a distant card keeps its density: the reference alpha of each mip is found by
bisection so the fraction of texels passing the cutout matches the top level.
Golus's in-shader mip compensation uses `log2` and is replaced by the cooked
mips. Alpha-to-coverage with Golus's sharpening is used where the view runs
MSAA, and WebGPU's multisample count is one or four.

**Blades**: tilt and facing set the tip, bend sets the midpoint, the normal is
the curve's derivative crossed with the side vector; normals tilt outward so a
flat blade reads as rounded; a blade seen edge-on is widened in view space so a
field stays full and triangles stay above a pixel; far blades blend toward the
clump normal and lose gloss. The low LOD's tile is twice the size with the same
blade count, the high LOD drops three blades in four first, and the high LOD
morphs toward the low LOD's shape near the switch.

**Shells**: the shell index is the instance index, so a field of shells is one
instanced draw rather than one per layer as Acerola's repository draws them. His
per-shell `pow(i/N, e)` height curve and ambient-occlusion term are per-shell
constants computed on the CPU into a table, which removes `pow` from the shader;
the per-cell hash is an integer hash. Shells show their gaps at grazing angles
(his examples: Dark Souls' Sif, Genshin's moss); fins are the classic fix —
NVIDIA's 2007 shells-and-fins extrudes them in a geometry shader, which WebGPU
does not have, so each edge carries a pre-built fin quad with both face normals
and the vertex stage collapses every fin not near a silhouette.

### 4. Stylised is a shading choice, not a geometry

The levers every stylised reference pulls are colour and normal: a base-to-tip
gradient along the blade, an ambient-occlusion colour at the root, an additive
tip colour (Acerola), clump-coloured patches (Ghost of Tsushima), and normals
taken from the ground or straight up so a field shades as one surface. Each is a
field of the blade type, so any of the three looks can be stylised. Primary
sources for Breath of the Wild, Genshin and Ghibli-style grass were not found;
the levers above are the ones the sources that exist agree on.

### 5. Wind bends grass without trigonometry

Every blade reads [56-wind.md](56-wind.md)'s field at its root. Compute bends
the facing by the wind (Ghost of Tsushima); the vertex stage adds a bob whose
phase comes from the blade hash and its position along the blade — which Ghost
of Tsushima wrote as "a simple sine wave" and which is written here as one,
through `crcbl_shaders::trig`. Cards lean at the top vertices by the sample;
shells displace along the wind in proportion to the shell's height constant.

### 6. Interaction is simulation state on the tick

- **A trail texture** around the camera, God of War's dual heightfield: collider
  undersides (spheres, per-joint capsules) render into two channels, one held
  and one fading upward at a constant rate; the difference over the fade rate is
  the time since contact, and the held channel's gradient is the lean. The fade
  rate is zero at the player's feet, so an idle character does not pop the grass
  up. God of War's settle pose is "a cosine times a falloff", written with
  `crcbl_shaders::trig`. The texture is stepped on the tick, re-centred by whole
  texels: state, not history.
- **Per-blade simulation** is Jahrmann and Wimmer's (i3D 2017), the rung after:
  a quadratic Bézier whose tip alone moves under recovery toward rest, gravity,
  wind and sphere collision, validated each step to stay above ground and keep
  its length, with a crush term that decays. Their wind is "multiple sine and
  cosine functions" and is replaced by the field. It runs as compute on the tick
  and is visual only: GPU arithmetic differs by vendor. Their measured cost was
  0.547 ms of physics for 397,881 blades with 183 spheres on a GTX 780M.

### 7. Streaming, shadows and the far field

Tiles are rings doubling in size around the camera; the farthest ring is a
texture on the ground rather than blades (Ghost of Tsushima); distance culling
drops blades by index (Jahrmann and Wimmer). Grass does not draw into the shadow
atlas blade by blade: Ghost of Tsushima raised the ground's vertices to grass
height with a dithered depth offset in the shadow map, which is a shadow-caster
impostor with no blade in it, plus screen-space shadows for contact.

### 8. Trees sway from baked vertex data, and hero trees from bones

Trees are a wind consumer on the forward pass rather than a pass of their own.
The research's shipped answers form a ladder, and the vertex-stage budget above
decides where each rung's data lives.

- **T1 — Crysis bending**: main bending by height with a length-preserving
  renormalise, detail bending by per-leaf phase and stiffness in the vertex
  colour (red edge stiffness, green phase, blue overall stiffness, alpha ambient
  occlusion), oscillators from smoothed triangle waves. **Permitted as
  published**, and it fits in `color` with no new stream. The vertex colour's
  current meaning as albedo moves into the material for a mesh flagged as wind
  geometry.
- **T2 — hierarchy**: two or three pivot levels per vertex (trunk, branch,
  sub-branch), as Ghost of Tsushima's trees and SpeedTree's wind carry. Pivots
  live in a per-mesh texture indexed through the `uv1` lanes — Pivot Painter 2's
  arrangement — so the data is a sampled texture and not a ninth storage buffer.
  Rotation about a pivot is God of War's translate-and-restore-length, which
  "avoids slow trigonometric functions".
- **T3 — bones**: a hero tree's branches as a small skeleton on the existing
  compute skinning path (`ForwardRenderer::reserve_skinned`,
  `add_skinned_instance`), each branch sampling wind on its own and able to
  collide. Unreal's Nanite Foliage does this and reports a hundred thousand
  bones in about 0.1 ms on the GPU; it also states its limits — "only has
  support for global wind direction", "no collision with players or objects" —
  which this field does not share.
- **Per-instance sway** is a damped spring on the CPU tick per tree, written to
  a sampled per-instance texture: 56's decision 7.
- **Import**: glTF allows custom attributes whose semantic starts with an
  underscore and whose component type is not unsigned; `crcbl-scene`'s importer
  reads `_WIND_*` attributes into the lanes above, falling back to a vertex
  colour laid out as Crysis's.
- **Culling**: a wind-flagged instance's bounds are inflated by its maximum
  sway, and its clusters' normal cones are not used to reject it — the same
  reasoning e05cb9f applied to skinned geometry. Both the vertex stage and the
  mesh-shader stage carry the motion, and the shadow pass inherits it because it
  runs the same stages.

## The rungs

Each rung is priced on the desktop adapter, lavapipe and the browser before it
counts as built, per [43-render-standards.md](43-render-standards.md), and runs
on the WebGPU backend in the fixture's browser demo.

| Rung | What it buys                                                                                                                                                                                                                                                                                                                                                                                                            | What it costs                                     | Needs                          |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- | ------------------------------ |
| G1   | The field description, tiles, deterministic placement, compute generation into fixed slots, the grass pass lit through a guarded copy of the light walk; card grass with cooked coverage mips; wind lean — **built 2026-09-16**, with the chain's level chosen per instance rather than by the hardware, because a cutout turns an implementation's freedom about a grazing quad's LOD into a binary per-pixel decision | one compute dispatch per tile; one draw per slot  | W1 of [56-wind.md](56-wind.md) |
| G2   | Mesh blades: Bézier from `vertex_index`, two LODs with morph, clumps, rounding, view-space widening — **built 2026-09-17**: the far level keeps one cell of every two-by-two block of the same tiles until G6's rings arrive, the morph's visible half is the normal's blend toward the clump, and blades are floored at a pixel for the reason cards pick a level per instance                                         | 15 or 7 vertices per blade                        | G1                             |
| G3   | Shells with fins, instanced; stylised shading levers for every look — **built 2026-09-17**: one instanced draw per tile over a `cells` buffer the generation pass writes at each cell's own index, fins pre-built and folded away in the vertex stage, levers on `BladeStyle`, strands floored at one pixel for the same reason G1's cards pick a level per instance                                                    | overdraw proportional to shell count              | G1                             |
| G4   | The trail texture: interaction on the tick                                                                                                                                                                                                                                                                                                                                                                              | one small render target and a step per tick       | G1                             |
| G5   | Per-blade simulation with collision and crush                                                                                                                                                                                                                                                                                                                                                                           | a compute step per tick over the streaming window | G2, G4                         |
| G6   | Rings, far-field texture, the shadow-caster impostor                                                                                                                                                                                                                                                                                                                                                                    | as its parts                                      | G2                             |
| T1   | Crysis bending on wind-flagged meshes, inflated bounds                                                                                                                                                                                                                                                                                                                                                                  | vertex-stage arithmetic                           | W1                             |
| T2   | Hierarchical pivots through a pivot texture; `_WIND_*` import                                                                                                                                                                                                                                                                                                                                                           | a texture per tree mesh                           | T1                             |
| T3   | Bone wind for hero trees on compute skinning                                                                                                                                                                                                                                                                                                                                                                            | a palette per tree                                | T2                             |

**Refused, with the reason**: blended grass (sorting a field is the wrong
problem; cutout plus coverage is what shipped games use); stochastic dithered
alpha (needs a temporal resolve this engine refuses); per-object gust scroll
offsets (56's decision 3); geometry and tessellation shaders (absent on WebGPU —
fins and LODs are built on the CPU or from `vertex_index` instead).

## What each rung is checked by

- **Placement is deterministic**: the same tile generates the same blades, in
  the same slots, on every backend — compared as instance data read back.
- **Coverage survives distance**: a card field's measured coverage at its
  farthest mip stays within a stated band of its nearest, against a sabotage
  that uses plain box-filtered mips.
- **Calm means still**: at zero wind intensity no blade, card, shell or tree
  moves between two ticks.
- **Interaction is local**: a collider bends blades inside its footprint and
  none outside it, and the trail recovers over the fade time.
- **Goldens** per look and per rung, with claims as relations — a blade field is
  denser in coverage than a card field at the same density; a gust band is
  brighter where blades turn their backs to the sun.

## Sources

Research brief, 2026-09-15.

- Wohllaib — _Procedural Grass in Ghost of Tsushima_, GDC 2021, transcript.
- Jahrmann, Wimmer — _Responsive Real-Time Grass Rendering for General 3D
  Scenes_, i3D 2017.
- Garrett Gunnell (Acerola) — _How Do Games Render So Much Grass?_, _Modern
  Foliage Rendering_, _What I Did To Optimize My Game's Grass_, _How Do Games
  Render Fur?_; the `Grass` and `Shell-Texturing` repositories.
- Sanders — the vegetation of Horizon Zero Dawn, GDC 2018; van Muijden —
  _GPU-Based Procedural Placement in Horizon Zero Dawn_, GDC 2017.
- Feeley — _Interactive Wind and Vegetation in God of War_, Advances 2019.
- Sousa — _Vegetation Procedural Animation and Shading in Crysis_, GPU Gems 3
  chapter 16.
- Castaño — _Computing Alpha Mipmaps_; Golus — _Anti-aliased Alpha Test: The
  Esoteric Alpha To Coverage_.
- Lengyel et al. — _Real-Time Fur over Arbitrary Surfaces_, 2001; NVIDIA — _Fur
  (with Shells and Fins)_, 2007.
- Outerra — _Procedural grass rendering_, 2012.
- Unreal Pivot Painter 2 and Nanite Foliage documentation; SpeedTree wind and
  vertex-property documentation; the glTF 2.0 specification.
- W3C WebGPU and WGSL specifications.

**Not found**: primary sources for Frostbite and Battlefield grass, Breath of
the Wild, Genshin Impact, Sable and Ghibli-style grass.
