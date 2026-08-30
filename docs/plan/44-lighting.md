# Topic 44 — Lighting: the two paths, the light list and the BRDF

Split out of [18-render-features.md](18-render-features.md) on 2026-08-27,
verbatim. That topic had grown past a hundred kilobytes and a reader after one
technique had to carry six others to reach it; topic 18 is now the index that
orders these and holds what is genuinely cross-cutting — the interactions, the
delivery table and the risks.

## Lighting: two complete paths (MVP)

**Ray-traced lighting is MVP, and so is a full rasterised twin of everything it
does.** `LightingPath` (see [39-capabilities.md](39-capabilities.md)) selects
between them per device, and the selection degrades: a device without
`RAY_QUERY` and `ACCELERATION_STRUCTURE` gets `Rasterised` and a complete
picture that merely looks worse.

This is a deliberate and expensive choice. It is made because **ray tracing is
Vulkan and D3D12 only** — WebGPU has no ray tracing at all, and Slang cannot yet
emit ray tracing for the Metal target, so macOS, iOS and every browser run the
rasterised path. Those are not edge platforms, so the raster twin is not a
degraded fallback nobody looks at; it is the path most players will see.

**Correction (2026-08-23): the arithmetic behind "expensive" has changed, and
the conclusion has not.** `crcbl-dx12` was deferred on 2026-08-21 along with
`crcbl-mtl` (see [09-backends-metal-dx12.md](09-backends-metal-dx12.md)), so of
the two backends that can ray trace, one is parked. Every backend under active
work — `crcbl-vk` and `crcbl-webgpu` — would run the rasterised path on every
device except Vulkan hardware with `RAY_QUERY`. That makes the raster twin
**more** clearly the right call, not less: it is no longer the path most players
will see, it is very nearly the only path.

What it does change is P7C's own justification, which this table does not carry
and should not be read as carrying. A ray-traced path is a whole second lighting
implementation reaching one of four backends while two of the other three are
deferred, and whether that is worth building next is a scheduling question for
the roadmap rather than a design question for this document. `docs/backlog.md`
is where that decision belongs.

| Effect              | `RayTraced`                         | `Rasterised`                                     |
| ------------------- | ----------------------------------- | ------------------------------------------------ |
| Global illumination | ray-traced GI                       | irradiance probes + baked/ambient                |
| Reflections         | ray-traced reflections              | screen-space reflections, probe fallback         |
| Shadows             | ray-traced shadows, all light types | CSM for sun, single map for spot, cube for point |
| Ambient occlusion   | ray-traced AO                       | screen-space AO                                  |

Rules that keep the two from diverging into two renderers:

- **One material model, one BRDF, one set of inputs.** Both paths shade with the
  same material table (topic 37) and the same lighting maths; what differs is
  how visibility and incoming radiance are _gathered_, never how they are
  _shaded_.
- **One tonemapped output target.** The post stack
  ([48-post-processing.md](48-post-processing.md)) runs identically after either
  path, so nothing downstream branches on `LightingPath`.
- **Golden images per path**, and a documented pair-wise comparison: the two
  paths are not expected to match pixel for pixel, but a scene that reads
  correctly on one and wrongly on the other is a defect in whichever is wrong.
  The comparison is a human-reviewed reference, not an automatic tolerance.
- **Acceleration structures are built regardless of who consumes them** where
  the device supports them — BLAS per mesh asset at bake or load, TLAS refit per
  frame from the same instance data the cull pass reads. Topic 24 and topic 13
  are the other potential consumers; neither is MVP and neither may assume the
  structure exists.

## Many lights: the list and how it is gathered (decided 2026-08-13)

The table above names shadows for spot and point lights, and when this section
was written **the engine had exactly one light** — a single `DirectionalLight`
(direction, colour) in the frame block. There was no light list, no light
culling and no count budget specified anywhere, so the shadow rows above were
not implementable as written. This section is that missing half, and it is
built: `crcbl_render::light` turns a `DirectionalLight` and each `Light::Point`
/ `Light::Spot` into `GpuLight` rows, and `crcbl_render::light_grid` runs the
compute pass that assigns them to the froxel grid `mesh.slang`'s fragment stage
indexes.

### Clustered forward

Lights live in an SSBO of rows, exactly as instances and materials already do,
and a **compute pass assigns them to a froxel grid** — screen tiles by depth
slices — which the fragment stage indexes by its own position. This is the same
discipline §3.3 already uses for instances: a compute pass produces compacted
lists, the draw reads them, the CPU uploads deltas and nothing else.

Chosen over the alternatives for reasons that are about this engine rather than
taste:

- **Tiled / Forward+** is simpler and degrades badly with depth range: a tile
  spanning a near wall and a far skybox gathers every light in between. The
  samples that motivate lighting (lantern, towers) are exactly that shape.
- **Deferred** conflicts with two rules already locked here: "one material
  model, one BRDF, one set of inputs" shared with the ray-traced twin, and the
  post stack running identically after either path. It also fights MSAA and
  transparency, and it would make the raster path structurally unlike the
  ray-traced one, which is the divergence this topic exists to prevent.
- **Clustered forward needs nothing of a device** — a compute pass and two
  storage buffers — so it is the same code on all four backends, which is the
  constraint every other path in this engine is held to.

**The line, restated as a rule on 2026-08-30** because the user asked that no
rung anywhere move this renderer towards deferred, on the grounds that forward+
is where the industry is heading and cheaper at the frame: **shading happens in
the forward pass and nowhere else.** A pass may _write a second attachment
beside the lit colour_ — the reflectivity target `47-reflections.md` argued in,
the motion target `43-render-standards.md` §9 built, an albedo or a normal that
a screen-space GI rung may one day want — because each is a by-product of the
shading the forward pass already did, read by one named consumer. What no rung
may do is move the BRDF, the froxel walk or a light's evaluation _out_ of the
forward pass into a pass that reads attachments: no G-buffer lighting, no
deferred decals, no visibility-buffer shading (`03-gpu-driven-rendering.md`'s
"visibility buffer slot" is an occlusion-cull input, not that). The test for a
proposal is one question — after it lands, does `mesh.slang` still evaluate
every light that reaches a fragment? — and `43-render-standards.md` §10 lists
deferred and visibility buffers among what is refused on this section's
authority.

**And a budget on the attachments the rule allows (2026-08-30)**: the forward
pass writes at most **16 bytes a pixel** on the software and browser tiers — and
that is where it stands today with no headroom: the lit target's eight,
`TransientImageDesc::reflectivity`'s four (`Rgba8Unorm`) and
`TransientImageDesc::motion`'s four (`Rg16Float`) — so a fourth attachment lands
only by paying for itself, with a measured frame on lavapipe beside it, per
`43-render-standards.md`'s pricing rule. Past that figure the pass has bought a
G-buffer's bandwidth without a G-buffer's savings, which on a bandwidth-bound
tier is the whole cost of the frame.

### What it costs and what is budgeted

- **A light is a row**: position, radius, colour premultiplied by intensity,
  type, and for a spot its direction and cone angles. A directional light is a
  row too, flagged as affecting every cluster, so the sun stops being a special
  case in the shader.
- **A cluster holds a bounded number of light indices.** Overflow is **counted
  and reported**, never silently dropped — topic 40's counters are where that
  number surfaces, and a scene that overflows should be visible in the debug
  panel rather than mysteriously dark.
- **Shadowed lights are a subset, and a small one.** Shadow atlas space is the
  scarce resource: the sun's cascades, then a stated number of spot maps and
  point cube maps, chosen by a rule the frame can state (nearest, brightest,
  largest screen influence — the rule is the next slice's decision and belongs
  in this file when taken). An unshadowed light still lights; it just does not
  occlude.

## The BRDF the "one material model" rule names (decided 2026-08-13)

The rule at the top of this file — "one material model, one BRDF, one set of
inputs" — had no content behind it. `mesh.slang` shaded with Lambert plus a
Blinn-Phong lobe whose exponent and strength were two `static const` floats,
`SPECULAR_POWER = 32.0` and `SPECULAR_STRENGTH = 0.35`, so there was exactly one
material in the engine however many rows the table held. That is the state the
SSR row ([47-reflections.md](47-reflections.md)) cannot be built on:
screen-space reflections have to know which pixels reflect and how sharply, and
nothing in the engine could say.

So the material row grew `metallic` and `roughness` — glTF's own two, under
their own names — and the lobe became **one Cook-Torrance GGX lobe** driven by
them: Trowbridge-Reitz `D`, Smith height-correlated visibility, Schlick's
Fresnel, Lambert diffuse. The two constants are deleted, not left beside it.

Why GGX now rather than a roughness-driven Blinn later:

- **The rule is the reason.** A roughness-driven Blinn would be a second
  material model, and P7C's ray-traced twin shades with the same one — so it
  would be written once and rewritten once, and in between the two paths would
  be comparable only by eye.
- **glTF already speaks it.** The importer reads `metallicFactor` and
  `roughnessFactor` off the accessor it was already holding for the base colour,
  so an imported material means what its author meant with no mapping in
  between.
- **It costs nothing a device has to have.** The lobe is dot products, a square
  root and two divides. No `pow` with a variable exponent and no trigonometry,
  for the reason the rest of `mesh.slang` avoids them: a platform's
  transcendental functions differ in the last place between the four targets, so
  none of them may reach a colour.

  **There are two ways out of the rule, and both are in use.** Bake the function
  into a table at cook time and sample it — rung 1's `DFG` table below — or
  build it out of the operations the rule permits, which is what
  `crcbl_shaders::fog` does for the exponential height fog
  [43-render-standards.md](43-render-standards.md) §4 wanted: range reduction, a
  Taylor kernel over the reciprocal factorials, and an exponent field written
  directly. The table costs an artifact and a binding; the construction costs
  neither, and is available whenever the function has one.

  **The rule is about shading, not about the file**, and the correction is worth
  making because the looser claim invites the wrong test. `mesh.slang` does call
  `log2` — three times, in `froxel_of`, inverting the cluster grid's exponential
  slice mapping. That is sound for the same reason the ban is real elsewhere:
  its result goes through a `floor` into an integer slice index, so a last-place
  disagreement between two platforms changes nothing unless a fragment is
  standing exactly on a slice boundary, and a fragment on the boundary was
  already free to land either side. A transcendental whose output is quantised
  is safe; a transcendental in the arithmetic that produces a texel is not.

Two consequences worth stating before somebody meets them:

- **A metal has no ambient term, and that is the model rather than a bug.**
  Ambient scales the diffuse albedo, and a conductor's diffuse albedo is zero —
  what a metal owes the room is a reflection, not a scatter. So a fully metallic
  surface out of every light's reach is **black** until it has something to
  reflect, and the two rows that give it one are exactly SSR
  ([47-reflections.md](47-reflections.md)) and irradiance probes
  ([50-irradiance-probes.md](50-irradiance-probes.md)). Nothing regresses today:
  `GpuMaterial::UNTINTED` is `metallic 0.0` and no scene in the tree sets one
  higher.
- **The engine's Lambert term carries no `1 / pi`, so neither does the specular
  one.** Trowbridge-Reitz normalises to `alpha2 / (pi * shape * shape)` and
  Lambert to `albedo / pi`; this engine's diffuse is a bare `albedo * N·L`, the
  convention where a light's intensity has absorbed the reciprocal. The `pi` is
  therefore folded out of `D` as well, so the ratio between the two lobes is the
  physical one. Writing the textbook `D` against this diffuse would put every
  highlight a factor of `pi` under the surface it sits on, which is not a look
  anyone chose — and it is what keeps a roughness of a half close to the Blinn
  exponent of 32 it replaced.

## Physically based: what holds and what is missing (2026-08-27)

The lobe above is a physically based BRDF and the rest of the frame is built to
match it — linear HDR from the first pass, a tonemap that maps scene-referred
radiance to display, and a metallic-roughness row whose two factors mean what
glTF says they mean. **The BRDF is not this engine's PBR gap.** What separates
it from a current engine's look is that the BRDF's _inputs_ are constants, that
it loses energy at high roughness, and that it has no environment to reflect —
three rungs, each independently landable, in the order their benefit per unit of
work falls.

### Rung 1 — Multi-scatter energy compensation

A single-scatter GGX lobe accounts for light that bounces off the microsurface
**once**. Everything that bounces twice or more is dropped, and the share
dropped grows with roughness: a furnace test — a surface under uniform white
light, which must return exactly white — comes back visibly grey for a rough
conductor, and darker the rougher it gets. So a rough metal in this engine is
too dark, and the error is not a constant that a factor could absorb because it
varies with both roughness and `N·V`.

The two standard answers both add the missing energy back from a table of the
lobe's directional albedo:

- **Kulla-Conty** stores `E(N·V, roughness)`, the fraction the single-scatter
  lobe returns, and adds a second lobe scaled by `1 / E - 1`.
- **Fdez-Agüera**'s closed form (what the Khronos glTF sample viewer ships)
  reads the same information out of the split-sum `DFG` pair that rung 3 needs
  anyway, so it costs no second table.

**Take Fdez-Agüera**, for that last reason: one table serves both rungs, and a
table that two features read is a table somebody will keep correct.

**Built 2026-08-27, both halves.** `crcbl_shaders::dfg` holds the committed
`tables/dfg.bin` — 64 square, two `f32` channels, `DFG_SAMPLES` samples a texel
— with `directional_albedo` and `energy_compensation` over it, and
`crates/crcbl-shaders/tools/cook-dfg.rs` regenerates or checks it the way
`cook-clusters` does the DAG. What it measures: a head-on surface hands back all
of the light at the smoothest row and **0.317 of it at the roughest**, so a
fully rough conductor in this engine is short by more than two thirds until the
factor puts it back. The table is cross-checked against an independent uniform
quadrature of the same integral, which is what says it is right rather than
merely self-consistent.

The shader half landed with it. `mesh.slang` binds the table as an `Rg8Unorm`
image at binding 25, decodes each texel's byte pair as one 16-bit fixed-point
number, filters four of them itself, and multiplies the summed specular lobe by
`1 + f0 (1 / E - 1)` once outside the light loop — the factor depends on the
material and the view and on nothing a light carries. Two of `apps/lantern`'s
goldens moved and were re-blessed.

**The filter is written out rather than asked of a sampler**, which is the same
determinism argument one line down: a hardware filter's weights are
fixed-function arithmetic four rasterisers compute independently, and these
goldens have no tolerance. It also costs no sampler and therefore moves no index
of Metal's sampler argument table.

**Fixed point rather than the `Rg16Float` rung 3 names below.** The stored value
is a share of arriving light and lives in `[0, 1]`, where a step of `1 / 65535`
is finer everywhere than half precision — which near one is `2^-11`, thirty
times coarser — and the split is an integer one this crate can perform without a
dependency. Rung 3 wants the scale and bias pair rather than their sum and will
upload its own image from the same committed bytes.

**What darkened, and why it is not this term taking light away.** Re-blessing
`room.png` moved 16113 channels up and 11470 down. Every darkened pixel sits on
a hard edge: the scene target is pointwise brighter by construction — the factor
is never below one — and FXAA's edge resolve is what turns a brighter
neighbourhood into a different blend. Measured by rendering the same frame with
the multiply neutralised and with reflections both on and off: 689 darkened
channels of 102045 changed at 960×720, present with reflections off, and each
one on a 173-against-203 silhouette.

**It is legal here, and the reason generalises.** The compensation term is
multiplies and divides over a sampled table, so no transcendental reaches a
colour. That is the general escape this workspace has from its own determinism
rule: **bake the transcendental into a texture at build time and sample it at
run time.** A table is data — it is compared as an artifact, byte for byte,
exactly as `spirv/manifest.txt` compares a compiled shader — where the same
arithmetic evaluated per fragment is four platforms' `pow` disagreeing in the
last place.

**Blocked on nothing.** The term needs `f0`, roughness and `N·V`, and
`fragmentMain` holds all three before it enters the light loop. This rung can
land before rung 3 and hand rung 3 a `DFG` table that already exists.

### Rung 2 — The inputs: a texture set, and its colour space

[43-render-standards.md](43-render-standards.md)'s §2 is this rung and carries
the dependency order — tangent frame, then pages, then alpha modes. Two things
belong here rather than there, because they are properties of the shading model
rather than of the material asset:

- **The non-colour pages are linear, and nothing about them may be sRGB.**
  `crcbl_render::forward`'s `BASE_COLOR_PAGE_FORMAT` is `Rgba8UnormSrgb` and is
  right to be: a base-colour texel is an sRGB-encoded colour, which is what glTF
  defines it as. A normal, a roughness, a metalness or an occlusion texel is
  **not a colour** — it is a number — and decoding it through an sRGB curve is
  wrong by a gamma. This is the classic PBR bug and it survives review because
  it looks plausible: roughness read through the decode is too smooth in the
  mid-range and the surface merely reads as "shinier than intended". A second
  page constant, `Rgba8Unorm`, and a test that asserts the two formats differ is
  the whole guard.
- **Two channels, not three, for a normal page.** The tangent-space `z` is
  `sqrt(1 - x² - y²)`, which is a square root and therefore allowed where a
  transcendental would not be, and reconstructing it means a BC5-class two-
  channel format holds the map at half the bytes with more precision per
  channel. The neutral texel §2 names as `(0.5, 0.5, 1.0)` becomes `(0.5, 0.5)`.
- **The pages' mips, sampler and compression are
  [43-render-standards.md](43-render-standards.md)'s filtering subsection**, and
  one line of it is the shading model's: a normal page's mips are renormalised
  after averaging, and the length a normal loses in that average is the
  roughness rung 4 puts back.

### Rung 3 — Specular IBL by the split-sum approximation

[43-render-standards.md](43-render-standards.md)'s §5 states the gap: L1
irradiance answers a diffuse question, and a rough metal needs prefiltered
radiance. The standard construction is Karis's split-sum, and both of its halves
survive this workspace's determinism rule for the same reason rung 1 does:

- **The `DFG` table** — two dimensions, `(N·V, roughness)`, two channels, a
  scale and a bias on `f0`. Baked once at build time and **committed like a
  shader artifact**, so there is no runtime `pow`, no per-platform derivation
  and nothing to re-bless: a table that four backends read the same bytes of
  gives four backends the same answer by construction. `Rg16Float` at 64×64 is
  the usual size and is a few kilobytes.
- **The prefiltered radiance chain** — the environment convolved against the GGX
  lobe at increasing roughness, one mip per step. The run-time lookup is
  `roughness * (mips - 1)` and a `SampleLevel`: one multiply, one fetch. The
  _bake_ may use whatever arithmetic it likes, because its output is an image
  compared as an artifact rather than a frame compared as a golden.

**This rung and §8's sky are one rung, not two.** A prefilter needs something to
convolve, and this engine's background is the scene target's clear colour. The
smallest thing that closes both is a gradient sky rendered into a small cube,
prefiltered on the way in — which also gives §5's SSR miss a real radiance
fallback in place of the L1 decode it approximates with today.

**The prefilter is a table, not a cube, and it is cooked (2026-08-29).** The
gradient is linear in its three colours and reads only a direction's `y`, so its
convolution against the lobe is
`far · W_far + opposite · W_opposite + horizon · (1 − W_far − W_opposite)` with
two weights over `(|R.y|, roughness)` — `crcbl_shaders::sky_prefilter`, a
64-square two-channel table committed as `tables/sky_prefilter.bin` on
`cook-dfg`'s terms (`cook-sky-prefilter`, with `--check` in CI). The sky's
colours stay a run-time parameter; only the lobe is baked. Facing the zenith,
the roughest lobe sees 0.697 of it and the horizon for the rest.
`prefiltered_radiance` is the sum on the CPU.

**The prefiltered half is in the frame (2026-08-29).** The table goes up once as
an `Rgba8Unorm` image (`sky_prefilter::texels`, two 16-bit fixed-point shares a
texel) bound to `ssr.slang`, which filters it with four `Load`s on
`specular_albedo_at`'s terms and reads the sky through it at the surface's
roughness — `sky_prefiltered`, in place of the mirror-direction `sky_radiance`
the miss fallback added before. It lives in the reflection pass and not in
`mesh.slang`'s ambient sum because that is where the gradient's rows already are
and where metals take their ambient specular; a term in both would count the sky
twice. The roughness axis shows in exactly one frame of the suite — the fully
rough floor `tests/render_e2e.rs` holds under a sky lit only below the horizon,
which a mirror's rays cannot see and the lobe can — and the goldens, all
mirrors, did not move. **And the `DFG` half is in the frame since the same
day**: `dfg::pair_texels` is the table's two channels as a second `Rgba8Unorm`
image the pass binds beside the sky's, filtered by the same four-`Load` helper
(`fixed_pair_at`), and `f0 · scale + bias` at `(N·V, roughness)` scales the
environment — sky and probes alike — in place of Schlick along the reflected
direction. The roughness-zero row is Schlick, so mirrors take what they took;
the rough floor in `tests/render_e2e.rs` takes under a quarter of what Schlick
gave it at its grazing `N·V`, and the test holds that from both sides. The rung
is built; what it leaves — no ambient specular when `REFLECTIONS` is off — is a
decision `docs/backlog.md` records.

### Rung 4 — Specular antialiasing, once normal maps exist

A high-frequency normal map under a low roughness aliases: the shading signal
moves faster than the pixel grid samples it, and the result is a field of
fireflies that **no antialiasing rung removes**, because the aliasing is in the
shading rather than in the geometry. [49-antialiasing.md](49-antialiasing.md)'s
whole ladder is silent on it by construction.

The industry answer is roughness regularisation: widen the lobe by the
screen-space variance of the normal, so a surface whose normal is changing fast
within one pixel is shaded as if it were rougher. Kaplanyan's formulation and
the Tokuyoshi-Kaplanyan improvement are both derivatives of the normal, a dot
product and a clamp.

**The determinism objection does not apply, and it is worth being exact about
why**, because §2 raises it against derivative-based tangents in stronger terms
than the tree supports. `shaders/mesh.slang`'s `geometric_normal_of` already
takes `ddx`/`ddy` of the world position in the fragment stage, its result drives
the shadow slope bias, and that reaches the frame — under cross-backend goldens
that hold today. So screen-space derivatives are not banned here and never were.
What §2's argument actually rules out is deriving a **tangent frame** from
derivatives, and the reason is mirrored UVs, where the derivative route gives a
handedness the vertex route gets right.

### What stays out, and why

- **A second BRDF lobe of any kind** — anisotropic GGX, clearcoat, sheen,
  subsurface. Each is a real term in a modern material system and each is a
  second material model, which the rule at the top of this file refuses until an
  asset in this tree needs one. They are additive later: the row has no space at
  `MATERIAL_STRIDE` today, so the first of them arrives with a stride widening
  and can bring the rest.
- **Parallax occlusion mapping.** A per-pixel march with a dependent texture
  read, for an effect normal mapping already approximates; it is a rung above
  normal maps rather than beside them, and there is no normal map yet.
- **Burley diffuse is refused — the user's call, 2026-08-30.** It was unranked
  until then: its fifth powers are Schlick's and decompose into multiplies
  exactly as `ggx_lobe`'s do, so the determinism argument that rules out AgX
  never touched it. What it buys is a retroreflective rim on rough dielectrics,
  which is small next to any rung above; Unreal ships Lambert for the same
  trade. **Lambert stays**, and what improves it is everything that already does
  or is scheduled to: the multi-scatter energy compensation on the lobe (built),
  the multi-bounce occlusion tint and bent-normal ambient
  ([46-ambient-occlusion.md](46-ambient-occlusion.md)'s decision of the same
  day), the LTC area lights and the probe volume's bounce. The diffuse lobe
  itself is not where the picture is lacking.
