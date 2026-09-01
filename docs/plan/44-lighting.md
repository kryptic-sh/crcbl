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
not implementable as written. This section is that missing half.

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

**The table's filter is written out rather than asked of a sampler**, which is
the same determinism argument one line down: a hardware filter's weights are
fixed-function arithmetic four rasterisers compute independently, and these
goldens have no tolerance. It also costs no sampler and therefore moves no index
of Metal's sampler argument table.

**Fixed point rather than the `Rg16Float` rung 3 names below.** The stored value
is a share of arriving light and lives in `[0, 1]`, where a step of `1 / 65535`
is finer everywhere than half precision — which near one is `2^-11`, thirty
times coarser — and the split is an integer one this crate can perform without a
dependency. Rung 3 wants the scale and bias pair rather than their sum and will
upload its own image from the same committed bytes.

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
  the whole guard. **Built 2026-08-30** with the normal page:
  `crcbl_render::forward`'s `NORMAL_PAGE_FORMAT` is that constant and
  `the_two_page_formats_differ` is that test — it reads both constants, checks
  they differ, and checks each graph import declares its own, so a page that
  quietly took the other one fails rather than looking shinier.
- **Two channels, not three, for a normal page.** The tangent-space `z` is
  `sqrt(1 - x² - y²)`, which is a square root and therefore allowed where a
  transcendental would not be, and reconstructing it means a BC5-class two-
  channel format holds the map at half the bytes with more precision per
  channel. The neutral texel §2 names as `(0.5, 0.5, 1.0)` becomes `(0.5, 0.5)`.
- **The pages' mips, sampler and compression are
  [43-render-standards.md](43-render-standards.md)'s filtering subsection**, and
  one line of it is the shading model's: a normal page's mips are renormalised
  after averaging, and the length a normal loses in that average is the
  roughness rung 4 puts back. **Half built 2026-08-30**:
  `crcbl_render::mip::normal_resample` is that second filter — no transfer
  curve, no alpha weight, a plain average of the decoded vectors and a
  renormalise, with a cell covering exactly one source texel copied byte for
  byte rather than round-tripped. What it does _not_ do is keep the length it
  divided out, so rung 4 has nothing to read yet; that is `docs/backlog.md`'s
  and it is the same change as the MRO page's roughness channel.

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

**The prefilter is a table, not a cube.** The gradient is linear in its three
colours and reads only a direction's `y`, so its convolution against the lobe is
`far · W_far + opposite · W_opposite + horizon · (1 − W_far − W_opposite)` with
two weights over `(|R.y|, roughness)` — `crcbl_shaders::sky_prefilter`, a
64-square two-channel table committed as `tables/sky_prefilter.bin` on
`cook-dfg`'s terms (`cook-sky-prefilter`, with `--check` in CI). The sky's
colours stay a run-time parameter; only the lobe is baked. Facing the zenith,
the roughest lobe sees 0.697 of it and the horizon for the rest.
`prefiltered_radiance` is the sum on the CPU.

**What the rung leaves** — no ambient specular when `REFLECTIONS` is off — is a
decision `docs/backlog.md` records: the prefiltered sky and the `DFG` pair are
read in the reflection pass and not in `mesh.slang`'s ambient sum, because that
is where the gradient's rows already are and where metals take their ambient
specular, and a term in both would count the sky twice.

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

### Rung 5 — LTC area lights, and the fill flag — rectangles landed 2026-08-31

**Decided by the user's rule "best-looking for the performance", 2026-08-30.**
Point lights read as pinpricks; a fixture reads as a fixture only when its
specular highlight has the fixture's shape. **Linearly transformed cosines**
(Heitz, Dupuy, Hill and Neubelt, 2016) give sphere, tube and rectangle lights a
physically plausible specular lobe from a small cooked table indexed by
roughness and view angle — the same shape as `crcbl_shaders::dfg`'s table, and
the same determinism argument: the transcendentals are in the cook, not the
shader. **All three shapes**, because a rect is what a window and a panel are,
and sphere and tube are nearly free once the table exists. **After clustered
forward**, so the light record changes once: `GpuLight` gains the shape, its
extents and its orientation in the same widening that gives it a cluster index,
and every constructor is touched once.

The same widening carries the **fill flag**: a light marked _fill_ casts no
shadow and contributes no specular. It is how a no-bake stack lights the far end
of a room — a practice every classic engine relies on and clustered forward
makes affordable by the hundred — and a flag on the record rather than a light
type, because everything else about it (cluster, falloff, colour) is the
ordinary light's.

**What landed.** `crcbl_shaders::ltc` holds the fit and the polygon integral;
`tables/ltc.bin` is the cooked 64-square table of the inverse transform's four
free entries, and `cook-ltc --check` holds it to its own integrator in CI the
way `cook-dfg` does. `GpuLight` grew to `LIGHT_STRIDE` 80 to carry a `tangent`
and a `flags` word, `KIND_RECT` and `FLAG_FILL` are the first values in each,
and `crcbl_render::RectLight` is the constructor. `mesh.slang` shades a
rectangle with the clamped-cosine integral outright for diffuse and the same
integral through the fitted transform for specular, scaled by the `dfg` pair —
which is why the paper's second table is not cooked at all: its magnitude and
Fresnel are Karis's scale and bias rearranged,
`f0 · magnitude + (1 − f0) · fresnel` and `f0 · scale + bias` being the same
number. Binding 25 therefore carries both channels now rather than one, and
binding 27 is the transform.

**Priced on radv and on lavapipe, 2026-08-31**, off `crcbl_render::PassStats`
through `mesh_e2e`'s `the_price_of_a_froxel_full_of_area_lights`, at 1920×1080
over 400 frames with a froxel full of lights — `CLUSTER_LIGHT_CAPACITY` of them,
so every fragment walks a full list, which is the worst case the grid allows:

| forward pass, 1920×1080      | radv (RX 7900 XTX, Mesa 26.2.1) | lavapipe (same Mesa) |
| ---------------------------- | ------------------------------- | -------------------- |
| sun alone                    | 0.082 / 0.092 ms                | 10.242 / 10.656 ms   |
| + a full froxel of point     | 0.203 / 0.231 ms                | 19.540 / 20.164 ms   |
| + a full froxel of rectangle | 0.532 / 0.615 ms                | 31.455 / 32.138 ms   |

p50 / p95, and the three rows are rendered **interleaved on one device**, a
frame each per turn, so a burst of contention lands on all three alike —
measured one set after another on lavapipe under the rest of the suite, the sun
alone came out dearer than the sixteen area lights that followed it. Over the
sun-only frame that is 7.6 µs per point light and 28.1 µs per rectangle on radv,
and 0.581 ms against 1.326 ms on lavapipe — **a rectangle costs 3.7× a point
light on the desktop tier and 2.3× on the software one**. The answer the row
predicted, "on everywhere", holds: even lavapipe's full froxel of rectangles is
a third of a 1080p frame it already spends ten milliseconds on, and no scene in
this tree has sixteen area lights over every pixel.

**The browser tier is stated rather than measured**, because no scene with an
area light reaches the browser harness yet — `docs/backlog.md` carries that gap.
By count, a rectangle costs a fragment four extra `Load`s (the transform's
bilinear tap, which shares its texel coordinates with the `dfg` read) and two
polygon integrals of up to five edges each; an edge is a normalise, a dot, a 2D
cross and the published rational, with a `sqrt` and a divide on the obtuse
branch. Against a punctual light's two normalises and the GGX lobe's two `sqrt`s
that is roughly an order of magnitude more ALU, which is what the two measured
tiers put at 2.3× to 3.7×. The browser runs the same shader on the same silicon
through WebGPU, so it should sit in the same band.

**What the rung left**, all in `docs/backlog.md`: sphere, tube and disc shapes
(the table serves them unchanged — they need corners and a shape field, not a
second fit); textured area lights; a rectangle in the `crcbl::screenshot::Scene`
list so the frame reaches `render_e2e` and the browser; area-light shadows; and
`volumetric.slang` scattering a rectangle as a point at its centre.

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
