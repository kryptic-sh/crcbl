# Topic 47 — Screen-space reflections: the march, roughness and determinism

Split out of [18-render-features.md](18-render-features.md) on 2026-08-27,
verbatim. That topic had grown past a hundred kilobytes and a reader after one
technique had to carry six others to reach it; topic 18 is now the index that
orders these and holds what is genuinely cross-cutting — the interactions, the
delivery table and the risks.

## Screen-space reflections (decided 2026-08-14)

The third P7B row. SSR runs after the forward pass, so the only per-pixel data
are the depth buffer, the `Rgba16Float` scene colour and the reflectivity
attachment — and a reflection needs to know its coloured `F0` and whether its
lobe is narrow enough for a screen-space ray.

### The AO section's refusal of an attachment does not transfer

[46-ambient-occlusion.md](46-ambient-occlusion.md) refuses a normal attachment
because "the prepass has no colour target at all, so it would mean a third
geometry pipeline per `GeometryPath`, a new fragment entry point compiled to
four targets, and a new `VertexOutput` consumer". **Every clause of that is a
fact about the depth prepass**, which is built from the shadow pipeline with no
fragment stage and no colour targets. On the **forward** pass none of it holds:
both forward pipelines already take one `ColorTargetState` array and both name
the same fragment entry, so a second target is one array element and no new
pipeline, no new entry point and no new interpolant. Recorded here so the
refusal is not applied by analogy to a pass it was never about.

### The decision

- **One new colour attachment on the forward pass: `Rgba8Unorm`, `rgb = F0`,
  `a = sharpness`.** Sharpness is the clamped screen-march ramp
  `1 - roughness / ROUGHNESS_CUTOFF`: zero means the surface keeps its probe
  environment but cannot honestly launch one screen-space ray. Encoding the
  endpoint rather than reconstructing it from quantised roughness is
  load-bearing — `0.5` may round to either neighbouring byte, while zero
  survives every `Rgba8Unorm` backend exactly. `max_color_attachments` is 4 on
  the minimum capability profile.
- **It carries the two values the downstream reflection pair consumes.** `F0`
  colours Fresnel for every surface. Sharpness gates the march and controls how
  strongly the blur moves from the direct centre fallback to filtered SSR. The
  original roughness remains in the material row for the forward GGX lobe; the
  attachment does not pretend a single screen-space ray can evaluate a broad
  lobe.
- **The normal stays reconstructed from depth**, sharing the AO pass's four-tap
  function — but see the escalation clause below, because the cost of a wrong
  normal is not the same for the two features.
- **The pass is the composite.** It reads scene colour, depth and reflectivity
  and writes their sum into a second `Rgba16Float` transient, which `add_passes`
  returns in place of the scene colour. One pass, no blend state, no feedback
  loop. A frame that does not add it returns the old id and the picture is
  bit-identical — the same data-not-a-branch off-switch AO has, needing no
  placeholder because nothing in `mesh.slang` reads the result.

### What is refused

- **Runtime reflection captures** (parallax-corrected cubemaps re-rendered on
  demand) — **declined 2026-08-30**. Six views per capture every time a light
  moves, a proxy volume in the scene format for the parallax correction, and a
  second environment path beside the one that exists. What they would buy — a
  reflected _room_ rather than a reflected sky on a glossy interior surface —
  the rebuilt probe volume now provides at low frequency on every tier
  ([50-irradiance-probes.md](50-irradiance-probes.md)'s per-level directional
  environment is the SSR miss fallback), and the ray-traced tier provides
  exactly. Revisit only if a demo shows a glossy interior where neither is
  enough.
- **Packing into the scene target's alpha.** Nothing reads it today — the
  tonemap samples `rgb` and writes a literal 1.0 — but one channel cannot carry
  a coloured `F0` and a roughness, so the packing is a scalar-reflectance design
  wearing a bandwidth argument. It also takes the name away from a channel that
  already has one, and that transparency will want.
- **A material-id channel with the pass reading the table itself.** Exactly
  right for untextured materials and exactly wrong for textured ones: the
  fragment stage multiplies the row by the vertex colour and the page texel, and
  a metal's base colour **is** its `F0`.
- **A G-buffer.** This attachment moves no shading: the forward pass still
  evaluates the whole BRDF, still reads the froxel list, still writes to
  target 0. The line to hold is that the attachment gains a field when a pass
  reads it, never because a G-buffer "should have" one.
- **Reading last frame's colour with reprojection.** Motion vectors are
  post-MVP, and a history buffer makes a golden a function of how many frames
  were drawn before it.
- **A planar reflection pass.** It would give a perfect mirror with no march,
  and it is per-plane, a second geometry pass per mirror, and useless on
  anything curved. It is the right answer for the render-to-texture camera this
  document already names, and belongs in that section.

### The escalation clause, written before it is needed

Reconstructed normals are exact on a plane and wrong on a one-pixel rim at every
silhouette, where the four-tap reconstruction keeps whichever neighbour is
nearer and at an edge that neighbour is on the other surface. **For AO a wrong
normal costs a pixel an eighth of its occlusion; for SSR a wrong normal is a
wrong ray, and a wrong ray fetches an arbitrary colour.** So: if a fringe of
unrelated colour one pixel deep appears at silhouettes, the fix is a second
attachment carrying the view-space normal, **not** a tuning of the march. That
escalation is contained to the fragment stage's return struct, one target state,
one transient and the SSR shader's first ten lines, and it moves no golden
because only the SSR pass reads it.

### The march

Screen-space DDA over the projected segment, no jitter, no refinement pass.

**Built 2026-08-27: the stride is hierarchical, not fixed.** The march climbs a
Hi-Z pyramid of the depth prepass — `crates/crcbl-shaders/shaders/hiz.slang` and
`crates/crcbl-render/src/hiz.rs`, one `hiz-N` pass per level — and crosses a
whole cell of whatever level it is on in one iteration, dropping a level only
where the cell in front of it is not empty. Everything else in this section is
unchanged and still describes the march: the reach, the clip, the border ramp,
the thickness bound and the probe fallback are all what they were. A frame too
small to halve once has no pyramid and the walk stays on level 0, which is the
fixed stride the paragraphs below describe, so that path is still live rather
than replaced.

- **Screen space, not view space.** A world-unit step is tens of pixels near the
  eye and a fraction of one far away, so the same constants would be a different
  tracer in a room and on a planet. A pixel step is a pixel step everywhere, and
  it makes the loop bound a property of the screen rather than of the scene's
  scale — which matters because CI's rasterisers are software and the loop bound
  is the whole cost.
- **Amended when it was built (2026-08-14): the _reach_ is a share of the frame,
  not a fixed pixel count.** The paragraph above is right about the step and
  about the loop bound, and a first cut took it literally — sixty-four taps two
  pixels apart, so a reflection could reach 128 pixels whatever the resolution.
  That has the mirror image of the defect the paragraph refuses, one level up:
  the same scene at five times the resolution grows five times as many pixels
  between a surface and what it reflects, so the reflection got _shorter_ as the
  window got bigger. `lantern`'s panel is where it showed — the reflection its
  golden asserts at 256×192 was simply absent from the 1280×960 review frame of
  the same room. `ssr.slang` therefore derives its stride from
  `REACH_FRACTION * min(width, height) / MAX_STEPS`: the stride is still a fixed
  number of pixels along one ray, the loop bound is still a constant, the cost
  is still the same at every resolution, and a reflection is now the same share
  of the frame at every resolution rather than the same number of pixels.
- **The segment is clipped to the viewport before the walk**, so every tap is
  in-bounds by construction and a ray leaving the screen stops being a branch.
  It ends at the clipped endpoint with a **border ramp** on its weight; a hard
  stop draws a visible line where reflections end.
- **A ray that hits nothing returns the probe environment.** The same L1 table
  used for diffuse irradiance is decoded back to approximate directional
  radiance, multiplied by Fresnel, and blended against a hit by confidence. A
  zero probe volume returns exact zero and preserves the old hit multiplication
  order. This is why the table's Reflections cell says "screen-space
  reflections, probe fallback" rather than claiming screen space is complete.
- **Behind an object, the depth buffer has no information, and this is where the
  plausible wrong answer lives.** A tap says the ray is behind the _front_
  surface, not how thick that surface is. A tap counts as a hit only within a
  thickness bound; past it the tap is **no evidence and the march continues**.
  Treating any "behind" as a hit is the classic SSR smear — it reflects the
  nearest foreground object into every distant reflection and reads as a comet
  tail off every silhouette. **The thickness is derived from the ray's own depth
  advance per step**, floored by a constant, rather than being a fourth number
  the Rust mirror has to agree about.
- **No binary-search refinement.** The crossing is interpolated linearly between
  the last two taps' depth deltas. A bisection is a cascade of binary
  comparisons; an interpolation is arithmetic on two values already fetched.
- Three more, each hiding a wrong answer: **start the ray off the surface**
  along the normal or the first tap self-intersects; **clip against the near
  plane** or a ray pointing towards the camera crosses `w <= 0` and every
  projected coordinate after it is nonsense; and **fade rays pointing back at
  the viewer**, which have almost no on-screen evidence to find.

### Determinism: the goldens cannot carry this one

**The AO argument does not transfer, and the difference is quantitative.** That
pass can say a flipped sample costs an eighth and the blur then divides it by
sixteen. A march has no such denominator: the first tap whose comparison flips
**is** the answer. Two drivers disagreeing in the last bit can tap a
neighbouring pixel at the crossing, or miss the crossing entirely at the last
step — the second costs the whole reflection at that pixel.

What is still worth doing, and is not decoration:

- **No jitter of any kind**, for the rotation table's reason applied to a case
  where it matters more. Stepping artefacts get lived with or blurred; they do
  not get dithered away.
- **Every weight is continuous and goes to zero exactly where the decision is
  fragile.** A hit at the last step is at maximum distance and its fade is near
  zero; a hit near the border is on the border ramp; a tap that barely satisfies
  the thickness bound is on that ramp's low end. **The pixels where two drivers
  can disagree are, by construction, the pixels whose reflection is multiplied
  by almost nothing.** That is inspectable rather than measured.
- **The roughness gate makes the screen march identically absent on most
  surfaces.** With the cutoff at 0.5, `GpuMaterial::UNTINTED`'s 0.5 encodes
  sharpness as exact zero in `Rgba8Unorm` on every target. Such a pixel still
  receives probe environment specular, but it returns before any projected-ray
  setup or depth tap. `Scene::Probes` explicitly disables reflections because
  its absolute Rust mirror predicts diffuse irradiance alone.

  **Resolved when probe specular landed (2026-08-14):** the cutoff stays at 0.5
  and gates only the screen march. A rough conductor therefore receives the
  broad, low-frequency probe environment without pretending one projected ray
  represents its lobe, while `UNTINTED` retains an exact-zero march endpoint.
  Raising the cutoff is unnecessary unless a later fixture specifically needs
  sharper SSR on rougher surfaces.

**And the honest part**: those reduce the exposure, they do not bound it. There
is no argument that puts SSR under `Tolerance::RASTERISER` in general. So a
golden stays a review aid, every real check is a **structural ratio between two
blocks of one frame** (which one-driver drift moves together), and a fixture's
reflections must come from **large, low-frequency reflected content** — if the
reflected surface is a flat lit floor, picking the neighbouring tap changes
nothing. **If a golden flaps between CI's legs, the resolution is not to widen
the tolerance and not to re-bless per driver**: it is to make that fixture's
reflected content flatter, or to drop that golden and keep the ratio. Written
down before the first flap, because widening a tolerance will look like a
one-line fix at the time.

### Roughness

The screen-space half does **sharp mirror reflections only**. A single ray
cannot represent a wide lobe, and the failure mode of pretending otherwise is a
sharp reflection on a rough surface, which reads as a bug on sight. The
sharpness ramp is therefore a statement that the march is valid only where the
lobe is narrow; it does not gate probe environment specular, whose low-frequency
L1 result is more honest for a broad lobe than one ray.

The blur that follows is the AO blur's kernel, not a mip chain: proven in this
tree against real silhouettes, and gaining one factor — taps are weighted by how
close their roughness is to the centre's, so a mirror beside a rough metal does
not average the two. **Cone tracing over a colour mip chain is refused for this
row**: it needs mip generation on an `Rgba16Float` target and a `SampleLevel` at
a computed LOD, which is a filtered read whose level four rasterisers select
arithmetic for — the thing the AO pair spent its design avoiding. It is the
better technique and upgrading is contained to the blur pass, which the code
should say.

**Built 2026-08-14, and four things about it were not in the paragraph above.**

- **The blur had to become the composite, and the march had to stop being it.**
  A pass that adds the reflection to the scene colour leaves nothing to filter
  but the whole frame. So `ssr.slang` writes the reflection alone into an
  `Rgba16Float` transient of its own and `ssr_blur.slang` writes the sum — which
  also means the off-switch is now the pair rather than the one pass.
- **The second weight is the sharpness ramp carried through the reflection.**
  `mesh.slang` stores the lobe's roughness in the `Rgba8Unorm` attachment,
  quantised to the target's levels in the shader because a raw store of the
  cutoff is a rounding tie the output merger resolves per backend; `ssr.slang`'s
  `sharpness_of` derives `saturate(1 - roughness/cutoff)` from the reload, and
  the march copies that value into the reflection alpha. (Until 2026-08-29 the
  attachment held the ramp itself, which left the pass blind to roughness past
  the cutoff — exactly where the prefiltered environment of `44-lighting.md`'s
  rung 3 needs it. Where nothing drew, the attachment holds `NO_REFLECTION`: no
  `F0` and fully rough, since a zero alpha now reads as a mirror.) Zero
  sharpness returns the probe fallback before march setup and the blur
  composites that centre value directly. Positive sharpness uses
  `lerp(centre, filtered, sqrt(sharpness))`, so approaching the cutoff is
  continuous while a half-sharp reflection retains enough filtering to remove
  the march's stepping. The linear share was measured at 8.46–8.48 levels of
  mean row bend across lavapipe, WARP and Metal against the fixture's limit of
  8; the square-root share measures 4.82 on local lavapipe.
- **The depth tolerance is the march's `THICKNESS_FLOOR` times a small
  multiplier, and the multiplier is not decoration.** `DEPTH_TOLERANCE_RADII`'s
  shape, but a floor-thickness is a much shorter length than the AO radius: at
  one of them the filter switches itself off on a floor seen at a shallow angle,
  which is exactly where a reflection lives, and the stepping survives it. Eight
  keeps the kernel at full strength across such a surface and still falls to
  nothing across a silhouette.
- **The cutoff did not move with it.** The paragraph above pairs the blur with a
  cutoff a rough conductor clears, and those are two changes with very different
  blast radii: the filter moves the frames that already reflect, and the cutoff
  puts `GpuMaterial::UNTINTED` — nearly every surface in the engine — into the
  march. They were split, and the second is its own slice with its own decision
  to record. See `docs/backlog.md`.

**What the blur is measurably worth, and what it is not.** The stepping the
march leaves is gone: `render_e2e`'s `Scene::Ssr` band bends 17.7 levels per row
with a kernel that keeps only its centre tap and 2.8 with the real one, and that
is asserted. Cross-driver divergence fell where it mattered — on the 192 pixels
of `lantern`'s room the blur changed, llvmpipe and radv disagree by at most 8,
where the unfiltered march's worst in the panel's band was 66. **The roughness
weight, though, is not separable by any assertion this tree's fixtures
support**: no fixture puts a mirror-sharp surface beside a rough one at the same
depth, which is the case it exists for. It is kept on the construction argument,
and `docs/backlog.md` carries that as a coverage gap rather than as a claim.

### What is left to later rows

No temporal anything. No back-face or thickness buffer. No half-resolution SSR.
The reason recorded here — that half-resolution AO was owed and unmeasured, and
a second unmeasured quality-for-speed trade should not land before the timers
had been pointed at the first — **no longer applies**: half-resolution AO landed
2026-09-02 and was swept on both drivers. This row now wants a reason of its own
or none. No `LightingPath` gate, which still has no consumer. **No specular
occlusion**: AO scales the ambient term alone, a highlight is an image of a
light, and a reflection is an image of the room in one direction — none of the
three take the same occlusion factor, and if one is wanted it is its own term
and its own decision. And **SSR on transparency is out, with the interaction
recorded now**: a transparent surface writing the reflectivity attachment would
overwrite the opaque `F0` behind it while the scene colour at that pixel is a
blend. Every SSR has this; writing it down is what stops the transparency row
rediscovering it as a bug.

### Taken 2026-08-27: Hi-Z marching, and a colour pyramid that may already exist

The row above names "No Hi-Z traversal", and the roughness section concedes that
cone tracing is the better technique. This is the quality-and-performance pass
that collects both, and it is one slice because they share the march.

- **Hi-Z marching replaces the fixed-stride DDA. Built 2026-08-27**, and this
  bullet is kept for the two things it predicted wrongly. A ray climbs to
  whichever level its current cell is empty at and steps across the whole of it,
  so a march crosses a frame in `O(log n)` steps where a fixed stride spent a
  constant number of taps at a constant spacing.

  **It is a `max` reduction, not min-max.** One value per texel — the nearest
  surface anywhere in the block below it — is the whole of what the skip test
  needs, and a second channel would have been a second image, a second binding
  and a second attachment per level for a bound nothing reads.

  **The reach did not stop being a share of the frame.** `REACH_FRACTION` is
  still what bounds a ray, and the pyramid buys the cost end of the trade alone:
  the same reach for far fewer taps, and a crossing resolved to a texel instead
  of to a 1.5-pixel stride. Lengthening the reach is a separate change with its
  own goldens, and nothing here forced it.

  **The determinism cost was real and it landed as predicted**: a pyramid is a
  reduction, so its levels are float arithmetic four rasterisers perform
  independently. The mitigation held — no jitter, a constant loop bound, and
  structural ratios rather than tolerances — and the one number that moved was
  `apps/lantern`'s `SSR_HIT_TOLERANCE`, because a texel-exact crossing weighs
  the same hit differently from a strided one. The two adapters agree on the new
  figure to a tenth of a percent, which is the claim that matters.

- **Cone tracing over a colour mip chain, for roughness.** `ssr.slang`'s header
  refuses it on two costs: building a colour pyramid, and a `SampleLevel` at a
  computed LOD. Half of that has changed under it — **the bloom downsample chain
  is already a colour pyramid of the scene**, built every frame a view asks for
  bloom, so the pass that would have had to build one may be able to borrow it.
  That is flagged as a thing to **verify**, not as a saving already banked: the
  chain's format, its extents, the mip it stops at and its lifetime inside the
  graph all have to agree with what a cone trace wants, and it is drawn only
  when the bloom bit is on — which no view in this workspace but the
  `Scene::Bloom` fixture sets. The computed-LOD read is untouched by any of that
  and remains the harder half of the refusal.
- **Temporal accumulation is blocked on its own work only.** The motion-vector
  pass it waited on is in the frame and is the same pass TAA will read —
  [43-render-standards.md](43-render-standards.md)'s §9 carries the convention a
  consumer must not guess at.
- **Planar reflections stay refused for this row**, and stay the right answer
  for the render-to-texture mirror this document already names. Nothing above
  weakens that paragraph: a planar pass is per-plane, is a second geometry pass
  per mirror, and is useless on anything curved.
- **Ray-traced reflections stay at P7C**, unchanged. This slice improves the
  raster twin; it does not move the row the twin exists beside.
