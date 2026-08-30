# Topic 50 — Irradiance probes: the L1 grid and the diffuse GI twin

Split out of [18-render-features.md](18-render-features.md) on 2026-08-27,
verbatim. That topic had grown past a hundred kilobytes and a reader after one
technique had to carry six others to reach it; topic 18 is now the index that
orders these and holds what is genuinely cross-cutting — the interactions, the
delivery table and the risks.

## DECIDED 2026-08-30 — one volume, two updaters, and no leaking

**The user's decision, on the question "best-looking dynamic lighting for decent
performance, and above all no light leaking", after the no-bake rule of the same
day.** The grid below stays, and everything that _fills_ it changes:

- **The static bakes go.** `apps/lantern`'s `bounce` module and `apps/shard`'s
  `light::probes` compute a lighting result once, at load, from a sun and
  torches that then move — a bake in the sense the rule forbids (the result
  outlives its inputs), whatever thread computed it. Both are replaced by the
  updater below in the slice that lands it; until then they stand, documented
  here as the thing being removed.
- **Each probe gains a visibility map**: a small octahedral depth + depth² image
  (Majercik et al. 2019's contribution, and the one thing that makes a probe
  grid stop leaking). `probe_irradiance` weights each of a fragment's eight
  probes by a Chebyshev visibility test against it, so a probe on the far side
  of a wall gets no weight. The maps are **rendered, not baked**: a depth cube
  per probe from the static geometry, drawn on load and again on demand for the
  probes a static-geometry change touches — the same shape as a reflection
  capture, a runtime capture of geometry rather than of light. Dynamic objects
  are not in them and do not occlude the bounce, which is the accepted limit
  (Enlighten's too).
- **The raster updater, every frame, on all four backends**: the sun's near
  cascade (and any lamp the application asks for) also writes flux and normal —
  a reflective shadow map — and one compute pass gathers the RSM into every
  probe, **each sample gated by that probe's visibility map**, so an RSM texel
  the probe cannot see contributes nothing. Sky irradiance goes through the same
  visibility, which is what stops a closed room receiving the sky. A fixed
  sample pattern, every probe every frame, no history —
  [43-render-standards.md](43-render-standards.md) §5's C2 holds and a golden is
  a function of its own inputs. One bounce, dynamic sun and lamps.
- **The traced updater, on `crcbl-vk`, `crcbl-dx12` and `crcbl-mtl`**: the same
  rows, filled by inline ray queries once foundation (c) exists, which buys
  multi-bounce and dynamic occluders. The volume, the visibility test and the
  shader readers are the same on both tiers; only the pass that writes the rows
  differs. That is what "the diffuse GI twin" in this file's title now means.

**Layered density, camera-centred (the user's addition, 2026-08-30).** One
uniform grid over a whole scene is either too coarse near the camera or too
large far from it. The volume is a **clipmap**: a small number of levels (three
or four), each a fixed probe count centred on the camera, level `k` spaced `2^k`
times level 0 — dense probes where the camera is, sparse ones in the distance,
and a world that can be any size because the levels are camera-relative. A level
scrolls with the camera in whole probe steps with **toroidal addressing**, so
moving one cell exposes one slab of new probes and every other probe keeps its
rows and its visibility map. A fragment reads the finest level that contains it,
blended over a band at each level's edge so a level change never pops; within a
level the read is the trilinear gather and the Chebyshev weighting above. Rows
are per level, so [43-render-standards.md](43-render-standards.md) §5's C1 (one
storage buffer) holds with an offset per level.

**Captured on load, then on scroll — never baked.** The visibility maps for the
whole initial window are rendered when a scene loads; after that only the slab a
scroll exposes is rendered, in the frame it appears, and a probe whose static
geometry changed is re-rendered on demand. The lighting rows are never stored
across a load: they are recomputed every frame by the updater, which is what
keeps the sun and every lamp dynamic. "Baked on load" in conversation means this
capture, and the word the plan uses for it is _captured_, because what is stored
is geometry.

**One base, two sample producers — nothing else is bespoke.** The pipeline is
the same on every tier, and the two tiers differ in exactly one stage:

```text
  placement (clipmap, scroll, relocation)   ── shared
        │
  sample producer: per probe, N directions → (radiance, distance, backface)
        ├── raster tier: depth cube per probe (distance, backface) captured on
        │                load/scroll + the RSM gather (radiance), gated by it
        └── RT tier:     inline ray queries, every frame — one ray gives all
                         three, dynamic objects included
        │
  integrate ── shared: samples → L1 irradiance rows + octahedral depth/depth²
        │
  shading read ── shared: level pick, trilinear, Chebyshev weight, SSR fallback
```

The producer's contract is a sample buffer of a fixed layout; the integrate
pass, the storage, the relocation rule (a probe whose samples are mostly
backfaces sits inside a wall and is moved or disabled — DDGI's rule, answered by
both producers because both report backfaces) and the shader read never know
which producer ran. What the RT producer buys over the raster one is dynamic
occluders in the visibility and, if C2's temporal question is ever answered yes,
a second and further bounce by reading the previous frame's rows; single bounce
is the rule on both until then.

**What this amends.** The GI decision of 2026-08-30 said the tier below ray
tracing has no bounce term; it now has this one, because it is leak-free and its
cost is one compute pass and two extra shadow-pass targets — the first thing on
the ladder below that the user judged worth having on every tier. The DDGI
rejection below stands on its temporal half and falls on its ray-tracing half;
the light-field-probe rejection ("no leaking defect yet to justify them") is
withdrawn — leaking is the defect the decision is about. The section "The
irradiance is authored, not baked and not computed at runtime" describes what
shipped in August and is superseded by this one.

**Pricing, before it is built.** Visibility capture: 6 × 32² depth per probe,
thousands of probes in a few hundred milliseconds on desktop, amortised over
frames on the browser tier. Per frame: the RSM's two extra targets on the near
cascade and one gather pass — the DDGI-class budget of roughly half a
millisecond to a millisecond at 1080p on desktop; the browser tier takes a
smaller grid. Each figure is measured on the three tiers before the rung counts,
per the standing rule.

**Order.** Among the raster lighting items this rebuild comes after LTC area
lights, the shadow atlas and the AO tint, and **ahead of the atmosphere** — the
user's order, 2026-08-30.

## Irradiance probes: the design (2026-08-14)

The capability table's `Rasterised` twin of ray-traced global illumination. The
paragraphs below describe what shipped rather than what was intended.

### A static grid of L1 spherical-harmonic probes, and no new pass

An irradiance volume: a uniform grid of probes in a read-only storage buffer,
trilinearly interpolated in the shader. The diffuse half is **added** to
`frame.ambient` in `mesh.slang`; the specular half is what a miss returns inside
`ssr.slang`.

**It adds no render pass** — `RENDER_PASSES`, `MAX_PASSES` and
`fullscreen_passes` are all unchanged. That is the single strongest reason to
prefer it here over anything that needs a pass of its own.

L1 is four coefficients per channel, packed so that evaluating irradiance for a
normal is **three dot products** against `float4(N, 1)`. No `pow`, no
trigonometry — the rule `ggx_lobe` is already held to, and the rule this file's
determinism argument rests on.

### Rejected, for reasons about this tree rather than about the technique

- **DDGI** (Majercik et al. 2019) needs ray tracing, which nothing here has, and
  temporal accumulation, which [47-reflections.md](47-reflections.md) already
  refuses in writing for SSR history: a golden must not be a function of how
  many frames preceded it. Either one is fatal alone.
- ~~**Light-field probes** (McGuire et al. 2017) are the correct answer to light
  leaking and cost a per-probe octahedral depth map. There is no leaking defect
  yet to justify them.~~ **Withdrawn 2026-08-30** — the per-probe depth map is
  adopted, in the decision at the top of this file.
- **SH L2** (Ramamoorthi & Hanrahan 2001) captures ~99% of diffuse irradiance
  against L1's ~87%, for 27 floats against 12 — a difference
  `Tolerance::RASTERISER` cannot resolve. Escalating later is contained to one
  constant and one function.
- **The Valve ambient cube** (McTaggart 2004) is close, and never rings
  negative. L1 is 12 floats to its 18 and needs no per-axis select; L1's ringing
  costs one `max(…, 0)` at this grid density. Recorded as the drop-in if ringing
  is ever seen.
- **A 3D texture with hardware trilinear.** `ImageType::D3` exists in the seam
  but nothing outside `null`'s tests has ever created one, `texture.rs` knows
  only the `D2Array` upload path, and hardware filter weights are vendor tables
  — the exact class of filtered read the AO and SSR designs spent their
  determinism arguments avoiding. An 8-tap manual lerp over a cache-resident
  table costs less and risks nothing.
- **A compute pass writing a probe volume.** One of the two reasons given here
  has gone: it was that `crcbl-wgpu` refused `BindingKind::StorageImage`
  outright, and that crate was deleted 2026-08-21 — `crcbl-webgpu` answers
  `Capability::StorageImageBinding` with `Support::Yes`. So the browser tier
  could run it; a storage buffer would be permitted, but the
  temporal-accumulation refusal above applies regardless.
- **Prefiltered radiance cubemaps** — the only thing that puts a recognisable
  room in a mirror — need cube arrays, mip chains and `SampleLevel` at a
  computed LOD, which `ssr.slang` refuses in writing. Deferred with a named
  trigger.

### A gather bake needs an intersector this tree does not have

Casting rays at scene triangles is what a gather bake is, and this tree has **no
ray-triangle intersector and no BVH**. `crcbl-phys`'s `query` module has
ray-vs-sphere, ray-vs-AABB and ray-vs-capsule and nothing else. Writing both,
plus an artifact format and a manifest entry, is its own topic-sized piece of
work and must not be smuggled into this row. The precedent for when it comes is
real — `cook-clusters` is a committed-artifact generator with a `--check` mode,
and `spirv/manifest.txt` is how such an artifact is hashed.

### Additive, which is what makes it safe to land empty

```
float3 irradiance = frame.ambient.rgb + probe_irradiance(world_position, normal);
```

A scene with no probes uploads a volume of zeroes, and `x + 0 == x` exactly on
every target — so the frame is **bit-identical and there is no branch
anywhere**. It is the same argument this file already makes for the sun becoming
a row: the sum starts at zero and `0 + x` is `x` exactly. An author who wants
probes to be the whole ambient sets `DirectionalLight::ambient` to zero.

AO still scales the diffuse environment alone, so the **no specular occlusion**
refusal above is untouched.

### The specular half goes where the SSR design already left room for it

`ssr.slang` fills the space a screen-space miss leaves with probe radiance:

```
hit = hit_color * fresnel * confidence;
fallback = probe_radiance(world_position, reflection_dir) * fresnel * (1 - confidence);
reflection = hit + fallback;
```

`confidence` is the existing hit weight. Written as two terms rather than the
algebraically equivalent `lerp`: with a zero probe volume, `fallback` is exactly
zero and the pre-probe `hit_color * fresnel * confidence` multiplication order
is unchanged, so existing SSR hits remain bit-identical instead of moving by a
half-float rounding level.

The table stores _irradiance_ coefficients with the diffuse clamped-cosine
transfer already folded in. Specular needs radiance, so `probe_radiance` divides
the constant band by `π` and the linear band by `2π/3` before evaluating the L1
basis. Directly dotting the stored rows would brighten a constant environment by
`π` and distort its directional term.

Three things follow: no double-counting by construction, the zero-probe case
stays bit-identical, and **a fully metallic surface stops being black**, because
a conductor's only non-direct light is a reflection and now it has one
everywhere the surface faces rather than only where the march lands. This also
holds at and above `ROUGHNESS_CUTOFF`: zero sharpness returns the probe term
before screen-march setup, and the blur composites that centre value directly.
Positive sharpness blends continuously from it into filtered SSR.

**The honest limit:** an L1 probe is a very blurry environment. On a rough metal
it is close to right; on a mirror it is a smooth gradient where a room should
be. That is "not black", which is what was wanted — it is not "a mirror".

### The data path adds no device requirement

A read-only storage-buffer fetch in a fragment stage, which is what the material
table, the light list and the froxel grid already are. The seam permits it
explicitly: only _writable_ storage bindings of host-visible buffers are
refused. The mesh binding is appended **after** `AMBIENT_OCCLUSION_BINDING`,
never inserted, because `crcbl-mtl` numbers Metal arguments by counting layout
entries while Slang numbers by declaration order and the two agree only while
both ascend. As with occlusion, the new index is past everything
`mesh_cluster.slang` declares, so that file needs no mirror.

**No new `Features` flag and no new selector** — topic 39 requires a real second
path behind a selector, not a capability that could have been a uniform.

### Determinism: the SSR argument does not transfer, and does not need to

The evaluation is three dot products, seven lerps and one `max`. There is **no
comparison between two fetched values anywhere in it** — SSR's exposure is that
the first tap whose comparison flips _is_ the answer, and a probe lookup has no
tap and no flip. The one discontinuous quantity is the grid cell index, and it
is provably not a hazard: the trilinear weight on the far corner reaches exactly
zero at the boundary where the index changes, so a driver landing in cell `i` at
`f≈1` and one landing in cell `i+1` at `f≈0` compute the same value.

So a probe golden goes under `Tolerance::RASTERISER` like every other 3D golden.

### Testing

- **CPU, no GPU**: encoding round-trip and a stride assertion; and a Rust mirror
  of the SH evaluation **checked against known values from the literature** — a
  constant environment of radiance `L` integrates to irradiance `π·L`, and the
  L1 band's transfer coefficient is `2π/3`. A transcription slip there would
  pass every other test in this tree.
- **The bit-identity gate**: slice 1 re-blesses nothing. If any golden moves,
  the additive-zero property is broken. Shown red by perturbing one coefficient.
- **`Scene::Probes`**: the open box with ambient at zero and the sun right down,
  so every pixel is the probe term and nothing else — the anti-vacuity condition
  `Scene::Ao` already relies on. Two probes with opposite-coloured L1, and the
  observable is a **ratio between two blocks of one frame**, which is the form
  this document mandates for anything a tolerance cannot bound. A flat ambient
  gives ratio 1.0 and a zero volume gives a black frame, so it fails in both
  directions rather than asserting what unfinished code already returns.
