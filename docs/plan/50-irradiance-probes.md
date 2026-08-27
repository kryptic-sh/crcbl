# Topic 50 — Irradiance probes: the L1 grid and the diffuse GI twin

Split out of [18-render-features.md](18-render-features.md) on 2026-08-27,
verbatim. That topic had grown past a hundred kilobytes and a reader after one
technique had to carry six others to reach it; topic 18 is now the index that
orders these and holds what is genuinely cross-cutting — the interactions, the
delivery table and the risks.

## Irradiance probes: the design (2026-08-14)

The capability table's `Rasterised` twin of ray-traced global illumination, and
what was P7B's last unbuilt row. **It is built now, as designed here** —
`crcbl_render::probe` owns the `ProbeTable` and the `ProbeGrid` that rides in
the frame uniforms, `crcbl_shaders::probe` fixes the row layout both sides
write, `mesh.slang`'s `probe_irradiance` does the interpolation and the three
dot products, `SceneDesc::probes` is where an application hands the volume over,
and `apps/lantern`'s `bounce` module bakes the sun's first bounce into one. The
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
- **Light-field probes** (McGuire et al. 2017) are the correct answer to light
  leaking and cost a per-probe octahedral depth map. There is no leaking defect
  yet to justify them.
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

### The irradiance is authored, not baked and not computed at runtime

`SceneDesc.probes`, filled by the application — for `apps/lantern`, computed
analytically from the room's own dimension constants so that moving a wall moves
the probes.

**Not a bake tool yet, for a concrete reason:** a gather bake means casting rays
at scene triangles, and this tree has **no ray-triangle intersector and no
BVH**. `crcbl-phys`'s `query` module has ray-vs-sphere, ray-vs-AABB and
ray-vs-capsule and nothing else. Writing both, plus an artifact format and a
manifest entry, is its own topic-sized piece of work and must not be smuggled
into this row. The precedent for when it comes is real — `cook-clusters` is a
committed-artifact generator with a `--check` mode, and `spirv/manifest.txt` is
how such an artifact is hashed.

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
