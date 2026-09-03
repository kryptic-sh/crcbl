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

- **No static bakes.** `apps/lantern`'s `bounce` module and `apps/shard`'s
  `light::probes` computed a lighting result once, at load, from a sun and
  torches that then moved — a bake in the sense the rule forbids, whatever
  thread computed it. Both went with the updater that replaced them.
- **Each probe carries a visibility map**: an octahedral depth + depth² map per
  probe, Majercik et al. 2019's contribution and the one thing that makes a
  probe grid stop leaking. Every reader weighs each of a fragment's eight probes
  by a Chebyshev test against it, so a probe on the far side of a wall gets no
  weight on any path. `crcbl_shaders::probe_visibility` owns the layout, the
  mapping and the bound and is the Rust mirror the render tests compare the
  shader against; `crcbl_render::probe_capture` fills it on the device.
- **The raster updater, every frame, on all four backends**: the sun's near
  cascade is drawn a second time as a **reflective shadow map**, and one compute
  pass gathers the RSM into every probe, **each sample gated by that probe's
  visibility map**, so an RSM texel the probe cannot see contributes nothing. A
  fixed sample pattern, every probe every frame, no history —
  `docs/backlog.md`'s survey constraint C2 holds and a golden is a function of
  its own inputs. One bounce, dynamic sun and lamps.

  **The sky through the same visibility is not free, and is undecided.**
  `mesh.slang`'s `sky_irradiance` is three dot products against `frame.sky_sh_*`
  with no direction set to gate, so there is nothing at the fragment to weigh.
  The implementable form is that the gather folds the sky into the rows along
  the directions a probe's own map reports as open, and the host zeroes
  `sky_sh_*` for a volume the updater owns. That is a real change to how a scene
  is lit rather than a binding, and it is its own decision.

- **The traced updater, on `crcbl-vk`, `crcbl-dx12` and `crcbl-mtl`**: the same
  rows, filled by inline ray queries once foundation (c) exists, which buys
  multi-bounce and dynamic occluders. The volume, the visibility test and the
  shader readers are the same on both tiers; only the pass that writes the rows
  differs. That is what "the diffuse GI twin" in this file's title now means.

**Layered density, camera-centred (the user's addition, 2026-08-30) — the levels
have landed; the scrolling has not.** One uniform grid over a whole scene is
either too coarse near the camera or too large far from it. The volume is a
**clipmap**: a small number of levels (three or four), each a fixed probe count
centred on one point, level `k` spaced `2^k` times level 0 — dense probes in the
middle, sparse ones out at the edge, and a world that can be any size because
the levels are relative to that point. A fragment reads the finest level that
contains it, blended over a band at each level's edge; within a level the read
is the trilinear gather and the Chebyshev weighting above. Rows are per level,
so `docs/backlog.md`'s survey constraint C1 (no ninth storage buffer) holds with
an offset per level.

What the levels do **not** include, and what each would take:

- **Scrolling.** There is no toroidal addressing, no camera-follow and no
  per-frame re-centring: the volume is centred once, where the scene places it,
  and `ProbeVolume::origin` is authored rather than tracked. Adding it means a
  per-level whole-probe-step offset in the header, a modulo in `probe_row`, and
  a rule for which probes a step invalidates.
- **Recapture.** `capture_probe_visibility` is still the load-time call; the
  slab a scroll would expose is not captured, and a probe whose static geometry
  changed is not re-captured on demand. Neither can exist before scrolling does.
- **A punctual producer.** The updater that landed 2026-09-04 gathers the sun's
  near cascade and nothing else, so a lamp's bounce is not in the rows — which
  is what `apps/lantern`'s coloured wall and `apps/shard`'s torches lost when
  their bakes went. `docs/backlog.md` carries what that cost, measured.

**Captured on load, then on scroll — never baked.** The visibility maps for the
whole volume are captured when a scene loads — that much runs, and since the
levels landed it covers every level of the clipmap in the one call; after that
only the slab a scroll exposes should be captured, in the frame it appears, and
a probe whose static geometry changed re-captured on demand. Neither of the last
two exists yet, and neither can until the volume scrolls. The lighting rows are
never stored across a load: they are recomputed every frame by the updater,
which is what keeps the sun and every lamp dynamic. "Baked on load" in
conversation means this capture, and the word the plan uses for it is
_captured_, because what is stored is geometry.

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
  shading read ── shared: level pick, trilinear, SSR fallback
                  both paths: Chebyshev weight
```

**The diagram is the design, not the tree.** Of the placement box only the
clipmap exists. The raster producer is whole as of 2026-09-04 — the capture's
distance half, and the RSM gather that fills the radiance half beside it,
`crcbl_render::rsm` and `crcbl_render::probe_gather` — with the standing limit
that it gathers the sun's near cascade and no punctual light. The RT producer is
unbuilt and waits on foundation (c). One more box is unbuilt:

- **Relocation does not exist**, and nothing it needs does either. No code
  counts backfaces per probe and none moves or disables one, because the capture
  writes distance and distance² and no backface channel, so the "both producers
  report backfaces" premise has nothing behind it on the raster tier. It lands
  with the producer that first reports one.

The producer's contract is a sample buffer of a fixed layout; the integrate
pass, the storage, the relocation rule (a probe whose samples are mostly
backfaces sits inside a wall and is moved or disabled — DDGI's rule, to be
answered by both producers once both report backfaces) and the shader read never
know which producer ran. What the RT producer buys over the raster one is
dynamic occluders in the visibility and, if C2's temporal question is ever
answered yes, a second and further bounce by reading the previous frame's rows;
single bounce is the rule on both until then.

**What this amends.** The GI decision of 2026-08-30 said the tier below ray
tracing has no bounce term; it now has this one, because it is leak-free and its
cost is one compute pass and one extra render pass of three small targets — the
first thing on the ladder below that the user judged worth having on every tier.
The DDGI rejection below stands on its temporal half and falls on its
ray-tracing half; the light-field-probe rejection ("no leaking defect yet to
justify them") is withdrawn — leaking is the defect the decision is about. This
section supersedes the August account of the authored irradiance, which was
removed from this file on 2026-08-30 rather than left standing beside it.

**What the capture costs, and it is what the clipmap is priced against.**
`apps/lantern --headless --frames 400 --size 1920x1080` on radv (RX 7900 XTX,
Mesa 26.2.1), median of three runs, reported by the app's own
`lantern: probe visibility captured …` line. The capture is a one-off at load
and costs **0.93 ms for 60 probes against 12 occluder meshes** (the room's
second view, 10 occluders, takes 0.84 ms), end to end — the pipelines, the
atlas, the six views a probe, the resolve, the copy and the `wait_idle`. That is
**16 µs a probe against the 0.28 ms the host ray cast took**, a factor of
eighteen. The per-frame price of the weighting does not resolve on the diffuse
path: `forward` reports **0.293 ms p50 with `r_probe_visibility` on against
0.302 ms off**, inside the 0.02 ms spread of the same three runs. On the
specular path it does resolve, and the tiers disagree about it sharply — the
software rasteriser pays 118% of the `ssr` pass where both GPU tiers pay 47%.
`docs/backlog.md` carries that measurement, the browser reading and the
candidate that would pay for it.

**Pricing for the halves not built.** A punctual producer is a second RSM and a
second gather, so it is priced against the sun's own, which `docs/backlog.md`
records. A clipmap of a few thousand probes is a capture of tens of milliseconds
rather than seconds, which is a load path rather than a redesign; what it still
needs is the slab recapture, so that a scroll pays for the probes it exposed and
not for the level. Each figure is measured on the three tiers before the rung
counts, per the standing rule.

**Order.** Among the raster lighting items this rebuild comes after LTC area
lights, the shadow atlas and the AO tint, and **ahead of the atmosphere** — the
user's order, 2026-08-30.

## The punctual producer — designed 2026-09-04, not built

The rung the sun-only updater owes, and the one that gives `apps/lantern`'s
coloured wall its tint and `apps/shard`'s braziers their warm bounce back. What
follows is derived against the passes as they stand rather than sketched; what
is not settled is the extent, and that is a sweep rather than a decision.

**It reuses the punctual shadow views whole, exactly as the sun's half reuses
cascade 0's.** `ForwardRenderer::add_shadow_pass` already builds one row per
face — `(shadow_view(slot, face), atlas_rect(light_tile(base + face)), cull)` —
for every occupied light slot, and every row of it names a bind group whose
uniform block holds that face's own `view_proj` and a `GeneratedDraws` its
slot's cull already produced. A spot is one face and a point is six, which is
`shadow::tile_span`, the same function `Selection` allocated the run with. So a
punctual producer costs **no second cull and no second matrix**, on the same
terms the sun's `cascade_zero` hook is written on. `rsmFragmentMain` needs no
change at all: it reads the material and the interpolated world position and
normal, and takes its transform from the bound block, so a perspective view is
already a view it can be drawn through.

**What the gather must do differently, and it is two slots.** The sun's
contribution is a radiance and a solid-angle weight:

```text
  radiance = albedo · sun_color · 1/π
  weight   = texel_area · facing / r²          (r = texel to probe)
```

For a punctual light both slots change and nothing else does. Take a texel's
distance to the light `d`, its surface normal `n`, the direction to the light
`l`, and the texel's own solid angle from the light `ω`. The engine's direct
term is `color · punctual_falloff(d, radius) · (n·l)`, so the patch's area is
`A = ω · d² / (n·l)` and the flux it reflects is

```text
  Φ = albedo · color · spot_cone · punctual_falloff(d, radius) · (n·l) · A
    = albedo · color · spot_cone · punctual_falloff(d, radius) · ω · d²
```

— **`n·l` cancels**, which is the same property that lets the sun's half omit
the sun's own cosine, and for the same reason: a tilted surface is lit less per
unit area and presents more area to the same beam. So

```text
  radiance = albedo · light_color · spot_cone · 1/π
  weight   = ω · d² · punctual_falloff(d, radius) · facing / r²
```

and `weight`'s new factor is exactly the sun's `texel_area` slot with a
per-texel value in place of a constant. Two consequences worth writing down:

- **The bounce agrees with the direct term by construction.** It multiplies the
  same `range_window` and the same `1/(d² + 1)` softening `mesh.slang` shades
  with, rather than a physical inverse square — so a light whose direct
  contribution the engine has already bent keeps that bend in its bounce, and no
  scene needs a second intensity to make the two agree.
- **`ω` is analytic, not a uniform.** A shadow face is a 90° perspective, so a
  texel at `(u, v)` in `-1..1` subtends `(2/side)² / (u² + v² + 1)^{3/2}`. That
  is a closed form of the fragment's own coordinates and wants no host number
  beside it, unlike the orthographic cascade's constant footprint.

**The gather stores; it must accumulate.** `probe_gather.slang` ends
`probes[probe * 3 + n] = tile[0].…`, a plain store, so a second dispatch over
the same rows would erase the sun's rather than add to it. Either the store
becomes a `+=` with the first producer of a frame zeroing, or one dispatch walks
every producer and the store stays a store. The second is preferable on this
tree's own terms — one workgroup per probe reducing once means the `groupshared`
tree is paid once rather than per producer, and a row is still a function of one
dispatch.

**The open question is extent, and it is a cost question.** The sun's map is
`RSM_SIDE = 64`, whose gather is 0.047 ms on radv over 4096 texels; a point
light is **six** faces of that, so a naive reuse of the same extent for
`apps/lantern`'s lamp alone would be seven times the texels the gather walks
today. A punctual face wants an extent of its own, swept the way `RSM_SIDE` was
— 16, 24 and 32 against the frame and against the tint the fixture measures —
and the light budget wants a rule: every occupied tile, or the highest-ranked
light only. Neither is decided, and neither should be guessed. Price it on radv,
on lavapipe and in the browser before the rung counts, per the standing rule.

**The fixture is already half-written.** `apps/lantern`'s frame claim 6 —
`room::TINTED_PLASTER` against `UNTINTED_PLASTER`, a red-to-blue ratio the
coloured wall drives — was deleted when the tint went, and the room-side proof
that nothing but the bounce separates those two points survives. Restoring the
claim is how this rung is verified, with the deleted claim's own measured 1.17
as the number to beat. `docs/backlog.md` carries the false-negative warning that
cost the sun's half most of its debugging time: a fixture must measure a surface
facing the way the flux travels, never a floor under a probe.

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
  and `crcbl-webgpu`'s HAL tests create one, but no production path does:
  `texture.rs` knows only the `D2Array` upload path, and hardware filter weights
  are vendor tables — the exact class of filtered read the AO and SSR designs
  spent their determinism arguments avoiding. An 8-tap manual lerp over a
  cache-resident table costs less and risks nothing.
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
ray-triangle intersector**. `crcbl-phys`'s `query` module has ray-vs-sphere,
ray-vs-AABB and ray-vs-capsule and nothing else. It does have a BVH —
`crcbl_phys::Bvh`, which predates this section — so the acceleration structure
is not the missing half. Writing both, plus an artifact format and a manifest
entry, is its own topic-sized piece of work and must not be smuggled into this
row. The precedent for when it comes is real — `cook-clusters` is a
committed-artifact generator with a `--check` mode, and `spirv/manifest.txt` is
how such an artifact is hashed.

### Additive, which is what makes it safe to land empty

```
float3 irradiance = frame.ambient.rgb + sky_irradiance(normal)
                  + probe_irradiance(input.world_position, normal);
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

The evaluation is a chain of lerps, three dot products, a divide and one `max`.
There is **no comparison whose branches disagree at the point it flips** — SSR's
exposure is that the first tap whose comparison flips _is_ the answer, and a
probe lookup has no tap. Two flips exist and neither is a hazard. The grid cell
index: the trilinear weight on the far corner reaches exactly zero at the
boundary where the index changes, so a driver landing in cell `i` at `f≈1` and
one landing in cell `i+1` at `f≈0` compute the same value. And `probe_weight`'s
`to_surface <= moments.x`: at the point it flips, `behind` is zero, so the
Chebyshev bound is `variance / variance` — one, which is what the other branch
returns. Both sides of both flips meet.

So a probe golden goes under `Tolerance::RASTERISER` like every other 3D golden.

### Testing

- **CPU, no GPU**: encoding round-trip and a stride assertion; and a Rust mirror
  of the SH evaluation **checked against known values from the literature** — a
  constant environment of radiance `L` integrates to irradiance `π·L`, and the
  L1 band's transfer coefficient is `2π/3`. A transcription slip there would
  pass every other test in this tree.
- **The bit-identity gate**: slice 1 re-blessed nothing, and the property it
  guards is now the narrower one that a scene with **no probes** is unchanged —
  the visibility slice moved every golden that has them, which is what a grid
  that stops leaking is supposed to do. The unchanged half was shown by
  disabling the capture and re-comparing the probes golden: zero differing
  pixels.
- **`Scene::Probes`**: the open box with ambient at zero and the sun right down,
  so every pixel is the probe term and nothing else — the anti-vacuity condition
  `Scene::Ao` already relies on. Two probes with opposite-coloured L1, and the
  observable is a **ratio between two blocks of one frame**, which is the form
  this document mandates for anything a tolerance cannot bound. A flat ambient
  gives ratio 1.0 and a zero volume gives a black frame, so it fails in both
  directions rather than asserting what unfinished code already returns.
- **The leak test**, `a_probe_behind_a_wall_lights_nothing_through_it`: one
  fixture drawn twice, with and without a wall between a lit probe and the band
  it would otherwise light. The walled −X band must drop by at least
  `LEAK_RATIO`, and the +X band must _gain_ by `LEAK_MIN_GAIN` levels because
  the same wall hides the black probe that was a quarter of its blend — so a run
  that simply darkened the room fails it too. Shown red by forcing the Chebyshev
  weight to a constant `1.0`, which reports both bands identical across the
  pair.
- **The specular leak test**,
  `a_probe_behind_a_wall_reflects_nothing_through_it`: the same room and the
  same wall through a mirror —
  `crcbl::screenshot::probe_leak_reflection_forward` shades it as a metal at
  roughness zero, so every band pixel's ray leaves the floor, misses in screen
  space and falls back to the probe grid. Each arm is drawn with the reflection
  pair and again without it and the difference taken, so what is compared is the
  SSR pass's own output and the divider's shadow, occlusion and diffuse term all
  cancel. Same two-directions claim, same shape of thresholds. Shown red by
  forcing `probe_weight` in `ssr.slang` alone to `1.0`, which leaves the diffuse
  test green and this one failing on both bands.
