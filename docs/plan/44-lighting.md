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

| Effect              | `RayTraced`                         | `Rasterised`                                                                  |
| ------------------- | ----------------------------------- | ----------------------------------------------------------------------------- |
| Global illumination | ray-traced GI                       | irradiance probes filled every frame by a reflective shadow map, plus ambient |
| Reflections         | ray-traced reflections              | screen-space reflections, probe fallback                                      |
| Shadows             | ray-traced shadows, all light types | CSM for sun, one atlas tile for spot, six for point                           |
| Ambient occlusion   | ray-traced AO                       | screen-space AO                                                               |

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
  scarce resource: the sun's cascades, then a stated number of spot tiles and
  six-tile point runs, chosen by a rule the frame can state. **That rule was
  taken** — `crcbl_render::shadow::coverage` ranks by screen influence, with
  `HOLD_RATIO` as the hysteresis that stops a tie swapping every frame and
  `tile_level` demoting a light that covers little to a smaller tile — and it is
  written up in [45-shadows.md](45-shadows.md) rather than here. An unshadowed
  light still lights; it just does not occlude.

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
  ([50-irradiance-probes.md](50-irradiance-probes.md)). The default is clear of
  it — `GpuMaterial::UNTINTED` is `metallic 0.0` — but **scenes are not**:
  `apps/lantern`'s mirror slab and its brass block are both fully metallic, and
  `crcbl_scene`'s two glTF paths default a row to metallic as glTF specifies. So
  the reassurance this bullet used to carry is spent, and the surfaces standing
  black without those two rows are the ones lantern ships to show them off.
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
glTF says they mean. **The BRDF is not this engine's PBR gap.** What separated
it from a current engine's look on 2026-08-27 was that the BRDF's _inputs_ were
constants, that it lost energy at high roughness, and that it had no environment
to reflect — rungs ordered by their benefit per unit of work. The energy is back
(rung 1, 2026-08-27) and the gradient sky is reflected through the split-sum
(rung 3, 2026-08-29); the rungs below keep the decisions each was built on.
Still open: rung 2's remaining pages and alpha modes, which
[43-render-standards.md](43-render-standards.md)'s §2 carries, and rung 4.

### Rung 1 — Multi-scatter energy compensation — landed 2026-08-27

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

**Where it lives.** `crcbl_shaders::dfg::energy_compensation` is the host
mirror, `tables/dfg.bin` the committed table, and `mesh.slang`'s
`specular_compensation` scales the specular sum by it once the light loop has
run — the diffuse term is left alone, since what the lobe dropped left the
surface as specular. Rung 3 reads the same table for its scale-and-bias pair.

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
  mid-range and the surface merely reads as "shinier than intended". A page
  constant per kind and a test that asserts a _number_ page is never the sRGB
  format is the whole guard. **Built 2026-08-30** with the normal page and
  **extended 2026-09-06** with the packed metallic-roughness-occlusion page and
  the emissive one: `crcbl_render::forward`'s `NORMAL_PAGE_FORMAT`,
  `MRO_PAGE_FORMAT` and `EMISSIVE_PAGE_FORMAT` are those constants and
  `the_page_formats_split_colour_from_number` is that test — it reads all four,
  checks `PageKind::format` answers with each, and checks each graph import
  declares its own, so a page that quietly took another kind's format fails
  rather than looking shinier.
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

### Rung 3 — Specular IBL by the split-sum approximation — landed 2026-08-29

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

**An atmosphere reaches this table through its own three bands (2026-09-05).**
[43-render-standards.md](43-render-standards.md) §8's sky is built, and it is a
LUT rather than a gradient — but `sky_prefilter.bin` convolves a _gradient_,
which is what makes it two channels instead of a cubemap chain. So a frame with
an atmosphere fills `SsrParams::sky` from
`crcbl_shaders::atmosphere::SkyView::gradient_fit`: the LUT's own poles and its
azimuthal mean at the horizon. Nothing in this rung changed, and what the
substitution cannot carry — the bright limb beside the sun, which a gradient has
no azimuth to hold — is §8's paragraph and `docs/backlog.md`'s entry rather than
this one's.

### Rung 4 — Specular antialiasing — landed 2026-09-05

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

**What landed.** `mesh.slang`'s `specular_aa_kernel` is Tokuyoshi and
Kaplanyan's isotropic filter transcribed from their listing: `SIGMA_PX` squared
times the summed squared screen-space derivatives of the shading normal is the
variance the pixel could not resolve, twice that is what it contributes to the
half-vector distribution, `KAPPA` clamps it, and the sum widens GGX's `alpha2`.
Both constants are the paper's — `SPECULAR_AA_SIGMA_PX` a half pixel and
`SPECULAR_AA_KAPPA` 0.18 — and `crcbl_shaders::mesh` mirrors them beside a
source-text test that holds the shader's spelling of the whole function to this
crate's copy, which is what a transcription needs when nothing else in the tree
knows the right answer.

**It widens the direct lobe alone**, and the refusals are argued at the call
site: the `dfg` split-sum pair and the `ltc` transform are bilinear reads of a
64-square table in the _perceptual_ roughness and move by a fraction of a texel
under a turning normal, and the reflectivity attachment has to describe the
material because `ssr.slang` reflects the scene rather than a light — a
per-pixel widening there would blur a mirror wherever screen-space geometry was
dense and unblur it as the camera moved. Regularising `roughness` itself, so
every consumer followed, needs two square roots to climb back out of `alpha2`
and that round trip is not the identity in floating point; every golden in the
tree would then move on a fragment whose kernel is exactly zero.

**The zero-kernel identity holds to the bit, and it was checked as bytes.** A
facet carrying one normal at every corner interpolates to that normal, both
derivatives are exactly zero and `alpha2 + 0.0` is the old `alpha2`. Against a
build with `SPECULAR_AA_KAPPA` forced to zero, the new fixture's frame differs
in rows 11 to 90 — its corrugated band — and in no other row. Across the rest of
the tree the same holds: `run-render-e2e.sh`, `run-forward-e2e.sh`,
`run-mesh-e2e.sh`, `run-draw-gen-e2e.sh` and the lantern, sundial, alcove and
quarry suites all pass unchanged on radv, quarry's curved DAG included — the
dunes patch is the single exception, and the paragraph below is its.

**The one golden that moves is the dunes patch, and it is re-blessed here.**
That surface is the only one in the tree with real curvature, and the
interpolated normal of a coarse DAG level turns several degrees a pixel, so its
kernel is not zero and its crests soften. Against the old reference
`the_dunes_scene_draws_its_cluster_dag_and_matches_its_golden` reported 4618
pixels differing (9.3953%), max channel delta 84, 595 over tolerance (1.2105%),
22 grossly wrong (0.0448%), ssim 0.999681 on radv and 29870 differing
(60.7707%), max 84, 617 over tolerance (1.2553%), ssim 0.999505 on lavapipe —
**and that scene's own reader passed both times**, its three claims about a lit
patch under a clear sky intact and 254 shades down the patch's middle on radv,
252 on lavapipe, so what failed was the picture and nothing about the frame.
Blessed on radv, the new reference is matched on lavapipe at 43 pixels over
tolerance (0.0875%), none grossly wrong, ssim 0.999777, and through the browser
backend on SwiftShader at 252 over tolerance (0.5127%), 12 grossly wrong
(0.0244%), ssim 0.999553.

**The fixture is `crcbl::screenshot::Scene::SpecularAa`.** A plate that is
geometrically one plane, drawn through a long lens from straight overhead so the
mirror direction is the same across it, carrying a conductor's material — no
diffuse lobe and no ambient, so every lit pixel is the specular term. Its `+z`
half is cut into 93 strips whose authored vertex normals zigzag 25° either side
of the mirror direction, a turn of 12.5 degrees per pixel against a lobe 1.9°
wide; its `-z` half is one quad with a constant normal on the lobe's shoulder.
Measured over a band of each, on radv and on lavapipe: the corrugated band's
maximum over its mean falls from **1.86 to 1.52** and its mean rises from **81.1
to 147.3** — the energy the undersampled lobe was losing between pixel centres —
while the flat band reads **110.6** either way, and the two frames are equal
channel for channel everywhere outside the corrugated band's own rows.
`crates/crcbl/tests/render_e2e.rs` holds all three claims, and its own doc
carries the four sabotages each bound was watched go red under.

**Every strip is exactly two pixels wide, on integer pixel columns, and that is
a portability property rather than a tidiness one.** Vulkan guarantees only four
`subPixelPrecisionBits`: a rasteriser may snap a vertex to a sixteenth of a
pixel, and radv carries eight bits where SwiftShader carries four. The first
version of this plate had 1.49-pixel strips at arbitrary sub-pixel positions,
and the two rasterisers put its edges in different places — 4788 pixels over
`Tolerance::RASTERISER` through the browser backend (9.7412%), max channel delta
24, all of it inside the corrugated band, with the flat band and the background
agreeing exactly. Sizing the plate so the projection — affine, since the plate
is one plane at a constant distance under a camera looking straight down it —
puts every edge on an integer column takes that to **1 pixel over tolerance
(0.0020%), max channel delta 3, ssim 0.999923**, and the gate passes on the
scene. The same comparison against a build with `SPECULAR_AA_KAPPA` forced to
zero now has **no pixel over tolerance at all and a max delta of 2**, which says
the residue was the vertex grid and not the kernel. `screenshot`'s
`SPECULAR_STRIP_PITCH` carries the arithmetic and `specular_plate_mesh` asserts
it vertex by vertex, so a later change to the camera or the extent fails loudly
instead of reappearing as a cross-backend diff.

**Priced** with
`apps/lantern --headless --frames 400 --size 1920x1080 --backend vk`, three runs
a configuration and the median of the p50s each run reported, against the same
tree with the kernel's one call site deleted so the derivatives are never taken.
Lantern's room is flat-normalled, so this is the unconditional cost — the kernel
is straight-line code with no branch, so a fragment whose variance is zero pays
what one whose variance is not pays. `forward` goes **0.342 → 0.341 ms** on an
RX 7900 XTX under radv (spreads 0.340–0.343 and 0.340–0.344) and **35.475 →
35.488 ms** on lavapipe (35.396–35.780 and 35.342–35.962), so on both tiers the
rung costs less than three runs can separate from noise. `shadow` and
`depth-prepass` evaluate no specular lobe and take no kernel at all, and they
are quoted as the noise floor of this measurement rather than as a result:
**0.141 → 0.140 ms** and **0.038 → 0.039 ms** on radv, **10.963 → 10.562 ms**
and **1.540 → 1.515 ms** on lavapipe. The software tier's shadow pass moved four
tenths of a millisecond between two builds that cannot have changed it, which is
the scale of drift any conclusion about its `forward` has to clear.

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
| sun alone                    | 0.087 / 0.090 ms                | 9.730 / 10.303 ms    |
| + a full froxel of point     | 0.225 / 0.231 ms                | 19.139 / 20.294 ms   |
| + a full froxel of rectangle | 0.559 / 0.578 ms                | 30.937 / 32.037 ms   |

p50 / p95, and the three rows are rendered **interleaved on one device**, a
frame each per turn, so a burst of contention lands on all three alike —
measured one set after another on lavapipe under the rest of the suite, the sun
alone came out dearer than the sixteen area lights that followed it. Over the
sun-only frame that is 8.6 µs per point light and 29.5 µs per rectangle on radv,
and 0.588 ms against 1.325 ms on lavapipe — **a rectangle costs 3.4× a point
light on the desktop tier and 2.3× on the software one**.

**What this table measures, and what it leaves out (2026-09-02).** `forward` is
the opaque geometry draw and its shading — `mesh.slang`'s per-light loop is
inside it, which is what forward+ means. Clustering is **not**: `light-cluster`
is its own compute pass, and it scales with the light count too — 0.002 ms for
the sun alone against 0.007 ms once sixteen lights arrive, the same figure for
sixteen rectangles as for sixteen point lights, stable across three runs. That
is **0.31 µs per light of clustering**, on top of the shading below, and it is
kind-independent for the reason `mesh_e2e/rect_bound.rs` measures separately:
`light_cluster.slang` bounds a rectangle by a sphere exactly as it bounds a
point light. So a point light truly costs about 8.9 µs and a rectangle about
29.8 µs, and the ratio with clustering folded in is 3.3× rather than 3.4×.

The `forward` figures also carry two full-extent attachment clears, which
**cannot** be split out: `clear_color` is a `LoadOp::Clear` fused into the pass
begin, so timing it separately would mean adding a pass and a full-target write
— a slower renderer bought for a tidier number. They are constant across all
three rows, so they cancel in every subtraction below; what they do affect is
any reading of `forward` as a _share_ of a frame. `docs/backlog.md` carries the
zero-geometry baseline row that would attribute them without changing the
renderer.

**Re-taken 2026-09-02** after `b36be08` changed how the colour pass shades and
`38b2688` changed the unprojection several of these passes use; the figures
above are the new ones, radv's from the median of three runs. What moved is the
desktop ratio, from 3.7× to 3.4×: both kinds of light got a little dearer per
light and the punctual one got dearer faster. lavapipe did not move at all —
0.588 ms against a recorded 0.581, and 1.325 against 1.326 — which is what says
the desktop shift is real rather than a re-measurement artefact. The answer the
row predicted, "on everywhere", holds: even lavapipe's full froxel of rectangles
is a third of a 1080p frame it already spends ten milliseconds on, and no scene
in this tree has sixteen area lights over every pixel.

**The browser tier is stated rather than measured**, and the reason recorded
here — that no scene with an area light reaches the browser harness — stopped
being the reason on 2026-09-02, when `Scene::AreaLight` and `Scene::FillLight`
both joined `apps/render-harness`'s list and began being compared against the
radv reference every run. What a scene buys is a **frame**, not a **price**:
nothing times a rectangle's shading against a punctual light's on that tier, so
the figure below is still an ALU count. `docs/backlog.md` carries the gap. By
count, a rectangle costs a fragment four extra `Load`s (the transform's bilinear
tap, which shares its texel coordinates with the `dfg` read) and two polygon
integrals of up to five edges each; an edge is a normalise, a dot, a 2D cross
and the published rational, with a `sqrt` and a divide on the obtuse branch.
Against a punctual light's two normalises and the GGX lobe's two `sqrt`s that is
roughly an order of magnitude more ALU, which is what the two measured tiers put
at 2.3× to 3.4×. The browser runs the same shader on the same silicon through
WebGPU, so it should sit in the same band.

**What the rung left**, all in `docs/backlog.md`: sphere, tube and disc shapes
(the table serves them unchanged — they need corners and a shape field, not a
second fit); textured area lights; area-light shadows; and `volumetric.slang`
scattering a rectangle as a point at its centre. The `crcbl::screenshot::Scene`
entry this list also asked for shipped 2026-09-02, so it is gone from here —
`git log` is where it lives now.

### What stays out, and why

- **A second BRDF lobe of any kind** — anisotropic GGX, clearcoat, sheen,
  subsurface. Each is a real term in a modern material system and each is a
  second material model, which the rule at the top of this file refuses until an
  asset in this tree needs one. They are additive later: the row has no space at
  `MATERIAL_STRIDE` today, so the first of them arrives with a stride widening
  and can bring the rest.
- **Parallax occlusion mapping.** A per-pixel march with a dependent texture
  read, for an effect normal mapping already approximates; it is a rung above
  normal maps rather than beside them.
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
