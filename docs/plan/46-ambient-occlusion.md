# Topic 46 — Ambient occlusion: SSAO, its blur, and the road to GTAO

Split out of [18-render-features.md](18-render-features.md) on 2026-08-27,
verbatim. That topic had grown past a hundred kilobytes and a reader after one
technique had to carry six others to reach it; topic 18 is now the index that
orders these and holds what is genuinely cross-cutting — the interactions, the
delivery table and the risks.

## Screen-space AO: what the one-line row was missing (decided 2026-08-13)

> **Built 2026-08-14; what follows is the survey that preceded it.** Two of the
> three blockers below have since gone and the section says so further down —
> `forward.rs` has a depth prepass (`prepass_groups`), and the ambient term is
> separable. Kept because the survey is why the row was not implementable
> earlier, not because any of it still blocks.

The table above says "screen-space AO" and nothing else, and — exactly as the
shadow rows were not implementable before the light list existed — that row sat
on three things this engine did not have:

- **There is no depth prepass, and the depth buffer is built never to be read.**
  `TransientImageDesc::scene_depth` carries `DEPTH_STENCIL_ATTACHMENT` and
  nothing else, and its doc says "never sampled"; the forward pass attaches it
  with `clear_depth`, which discards on store.
- **The ambient term is unseparable after the forward pass.** `mesh.slang`
  computes `albedo * (ambient + direct) + gloss` in one line to one target, so
  anything downstream can only scale all three — and AO must darken ambient
  alone.
- **`LightingPath` has no consumer.** It is read by `Debug` impls, one log line
  and adapter tests. Only `GeometryPath` branches anything, so "the rasterised
  twin's AO" cannot be gated on the selector this file names.

So **the AO row is a depth-prepass row**, and the prepass is the structural
part. SSR will want the same depth, so it is a P7B cost that AO happens to pay
first.

### The decisions

- **Add a depth prepass, and it is unusually cheap here.** `shadow_pipeline` is
  already the depth-only twin of the colour pipeline, built from the same
  modules and the same layout; driven with the camera's bind group and the
  camera's already-generated draws it _is_ a scene depth prepass — no new
  pipeline, no new shader, no new bind group. `PassBuilder::depth_read` and
  `DepthStencilState::equal_depth_read_only` both already exist with no
  production caller, and `graph_compile.rs` already asserts the
  `DepthStencilWrite → DepthStencilRead` barrier for exactly this pair.
- **Reconstruct normals from depth; do not add a normal attachment.** Under this
  ordering an attachment is not merely MRT bandwidth — the prepass has no colour
  target at all, so it would mean a third geometry pipeline per `GeometryPath`,
  a new fragment entry point compiled to four targets, and a new `VertexOutput`
  consumer, for a buffer one pass reads. Use the four-tap closest-neighbour
  reconstruction rather than the two-tap `ddx`/`ddy` one: the naive version
  straddles the depth discontinuity at every silhouette and draws a dark rim
  around every object. Escalating later is contained to the prepass pipeline and
  the AO shader's first ten lines, and the code should say so.
- **AO is produced before the forward pass and consumed inside it**, as an
  integer texel fetch by `SV_Position.xy` multiplying `frame.ambient.rgb` alone.
  The shader already indexes a screen-space structure that way — `froxel_of`
  takes `SV_Position.xy` — and a `Load` needs no sampler, no UV and no
  filtering, which is one less thing for four backends to disagree about.
  **Multiplying the tonemap's input is refused**, and refused in writing because
  the one-line row invites it: it darkens direct light and highlights.
- **Classic normal-oriented hemisphere SSAO, eight samples, a sixteen-entry
  constant rotation table indexed by `pixel.xy & 3`, and a 4×4 blur over the
  result** — a box in the first slice, depth-weighted in the second (below). Not
  GTAO yet — its horizon integral is several times the work for quality nobody
  can resolve at the goldens' 256×192, and CI's rasterisers are software.
  Upgrading is a change to one function in one shader, the same shape
  `tonemap.slang` already documents for its curve; the pass, the resource, the
  binding and the test are unchanged.
- **The rotation comes from an integer-indexed constant table, never a float
  hash**, and **the blur is not optional**. This is the determinism rule and it
  is why the design looks conservative. Each AO sample is a binary depth
  comparison, so one sample landing on the threshold resolves differently on two
  drivers and swings that pixel by an eighth — far past
  `Tolerance::RASTERISER`'s delta of 2. Interleaved-gradient noise and
  `frac(sin(dot(…)))` hashes amplify float differences _by construction_, which
  is the opposite of what a golden needs; an integer index into a constant array
  is bit-identical by inspection. The blur's footprint is exactly the noise
  tile, so it removes the banding, and where all sixteen of its taps count it
  divides an isolated flipped sample by sixteen — which the depth-weighted
  kernel below is precise about, because it is no longer sixteen everywhere.
- **The golden is not the instrument.** An AO pass writing a constant 1.0 draws
  a perfectly plausible frame. The check is a **structural ratio**, in the shape
  `SPOT_SHADOW_RATIO` already uses: a band inside a concave corner must be
  measurably darker than a band on the same surface, at the same camera
  distance, with the same normal and the same distance from every light, outside
  the corner. That survives one-level driver drift and fails a no-op pass, an
  inverted normal, and a result that never reaches the shading line.
- **AO is always on, and the off-switch is data rather than a branch.** There is
  no device fact to gate on — every backend has a fullscreen draw, a sampled
  `D32Float` and an `R8Unorm` target — and inventing a capability that is really
  a performance opinion is what topic 39 exists to prevent. A renderer-owned 1×1
  `R8Unorm` cleared to 1.0, bound when the AO passes are not added, is the
  `shadow_placeholder` pattern already in the tree, and it makes a later quality
  knob a two-line change rather than a shader permutation.

  **Qualified when the switch was built (2026-08-14): the 1×1 form is not free,
  and what it costs is a line in the shader rather than a pass.** `mesh.slang`
  fetches this channel with a `Load` at the fragment's own pixel, chosen in that
  file for having no sampler, no UV and no texel-centre arithmetic for four
  backends to disagree about. A `Load` outside a texture's extent yields
  **zero**, not its one texel, so an unclamped fetch reads a 1×1 image as total
  occlusion everywhere but the origin: the first AO-off frame drawn that way was
  black wherever ambient was the whole of the light, on real hardware, with
  nothing reporting an error.

  So the fetch is clamped — `min` against `ambient_occlusion.GetDimensions()` —
  and with that, the paragraph above ships as written. A frame with AO off
  records **no occlusion pass at all** and takes no frame-sized image out of the
  transient pool; every property the placeholder was chosen for survives, and
  the clamp costs nothing on a frame-sized channel, where every fragment is
  inside the image already. `crcbl`'s `forward_e2e::depth_probe` is what asks
  whether the clamp is there, on every backend: it binds the same one-texel
  image to a frame of `MESH_EXTENT`, darkens the light list until ambient is the
  whole of the colour, and asserts the ambient term arrived.

  A frame-sized transient cleared to 1.0 by an `ssao-none` pass was what shipped
  first, on 2026-08-14, before the shader could be edited. It was correct and
  strictly more expensive, and it is gone.

### Risks this carries

- **The forward pass keeps clearing and writing depth**, and the first slice
  took that deliberately. `LoadOp::Load` with `Greater` — which an earlier draft
  of this section implied — **cannot work**: the prepass has already written the
  identical depth and `Greater` rejects equality, so every fragment dies and the
  frame is black. `GreaterOrEqual` works and is the version that buys the
  overdraw win, but it reintroduces the invariance risk below. Clearing is the
  only zero-risk form, and it is why `spot.png` moved by exactly zero pixels
  when AO landed.
- **Depth invariance**, which is what the overdraw win costs. `GreaterOrEqual`
  in the forward pass needs its `SV_Position.z` bit-identical to the prepass's.
  Same module, same entry point, same matrix, but two pipelines can be compiled
  differently and a marginally farther fragment is rejected, which looks like
  holes. Nothing in the shaders carries an invariance decoration. The four CI
  rasterisers are the measurement; the zero-risk fallback is to keep the forward
  pass writing depth and forgo the overdraw win, and **that fallback is taken by
  saying so in the code, never by re-blessing a golden around it**.
- **A `Load` on a depth texture with no sampler** is the corner this engine has
  already been bitten in once, over `DepthTexture2D` versus `Texture2D<float>`.
- **The box blur bleeds AO across silhouettes** as a halo. A bilateral blur is
  the fix and was deliberately deferred to the slice after the first frame
  exists — the section below is that slice.

### The depth-weighted blur (decided 2026-08-13)

The first slice shipped the box, and the risk above is what it cost: a box
kernel averages a foreground pixel's occlusion with a background that is not the
same surface, and the far plane is written "fully unoccluded", so every
silhouette in the frame carries a bright fringe exactly one kernel deep.
Replacing the kernel is a change to `ssao_blur.slang` and its bind group, which
is what the first slice said it would be.

- **Weight on view-space Z, never on the raw reversed-Z delta.** A depth
  difference is not a distance: the same one-metre gap is an enormous reversed-Z
  delta in front of the eye and almost none near the far plane, so a tolerance
  on the stored value would be a different filter in every part of the frame.
  The blur unprojects, exactly as `ssao.slang` does.
- **So the blur binds the same `SsaoParams` block**, rather than growing one of
  its own: `inv_proj` and the radius are already written there once per frame.
  The consequence is that the blur's bind group names a per-frame buffer, so its
  cache became a ring for the reason the occlusion pass's already was — a single
  cache keyed on the views hands the even frames' block to the odd ones. The
  helper both passes share now keys on every view it was given rather than on
  one, because the blur's group names two transients and is stale when either
  moves.
- **The weight is a ramp and never a cut.** `if (abs(dz) < threshold)` would put
  a _binary_ decision on the output pixel, which is precisely what the rotation
  table spends its whole argument keeping off the input samples: two drivers
  resolve the borderline case differently and the entire pixel jumps.
- **The tolerance is derived from the AO radius, not a new uniform field.** The
  radius is the only length these two passes have — `ssao.slang` gathers within
  it and its falloff is at full strength inside it — so it is already this
  pair's answer to "are these two pixels near enough to be occluding each
  other". A knob nobody adjusts would be a fourth thing the Rust mirror has to
  agree about for a number that is not free to move.
- **The far plane is the halo's mechanism, and its test is the one comparison
  that stays.** A far tap gets no weight and a far centre returns 1.0 unchanged,
  as `ssao.slang` already does at the same pixel. That test compares against an
  exact constant rather than between two computed depths, so two drivers either
  both take it or neither does.
- **What the division by sixteen is worth now, in writing.** It is the full
  sixteen wherever every tap counts, which is any surface facing the camera and
  therefore most of a frame — and it falls towards one exactly where taps are
  rejected, at a silhouette and at the far plane. The trade is deliberate: the
  taps a box spent there were a halo in every frame, and what is given up is
  margin against a driver disagreement that may be in none of them.
- **The observable is in `Scene::Cube`, and it is not the scene named after
  AO.** `Scene::Ao` looks into a closed trough, so every pixel of that frame is
  geometry: it has no far plane to bleed and no silhouette to bleed across, and
  the kernel change moves it by one channel level in a couple of hundred pixels.
  The cube frame has the plain pyramid's underside — one flat normal pointing
  down, one flat albedo, and no direct light on it, so its pixels are the
  ambient term times the occlusion and nothing else. The band along its
  silhouette measures about a thirteenth over the band two rows in with a box
  kernel and about a fortieth with this one, on both of the rasterisers it was
  run on.

### GTAO, taken 2026-08-27: the ground the refusal stood on has moved

The decision above says "Not GTAO yet" and
`crates/crcbl-shaders/shaders/ssao.slang`'s header says the same in its own
words — the horizon integral is several times the work for quality nobody can
resolve at the goldens' 256×192, and CI's rasterisers are software. **That is a
cost argument, and it never weighed the thing this section spends its longest
paragraph on.**

What ships sums **binary** depth comparisons. This section's own determinism
rule is that one such comparison landing on the threshold resolves differently
on two drivers and swings that pixel by an eighth, which is why the rotation is
a table and why the blur is not optional. GTAO's horizon-visibility integral is
**continuous**: a driver disagreeing in the last bit moves a horizon angle by a
hair and the occlusion with it, where a binary sum cliffs. **So GTAO degrades
gracefully exactly where the shipped pass cliffs, which makes it better for the
goldens rather than worse** — and that, rather than a quality opinion, is what
reopens it.

**The upgrade is contained where the shader's header already says it is.**
`occlusion_at` becomes the integral, and `ROTATIONS` becomes a slice-offset
table indexed the same way, because the rule that a rotation is an
integer-indexed constant and never a float hash survives the technique change
untouched. The pass, the `R8Unorm` resource, the binding, `ssao_blur.slang` and
the structural-ratio test do not move.

**Bent normals are the second half, and they are what make this worth more than
a quality bump.** A scalar occlusion can only _scale_ the ambient term; a bent
normal is a direction the ambient term can be sampled _along_, which is exactly
the hook the irradiance-probe section left open — `probe_irradiance` already
takes a normal and would take that one. It is also the honest route to specular
occlusion, which the SSR section refuses outright and refuses **correctly**: a
scalar AO is the wrong term for a reflection, and a bent normal with a cone
angle is the right one. That refusal stands until this pair exists.

**SSAO stays as the cheap tier** rather than being deleted, on the antialiasing
ladder's own FXAA-under-SMAA pattern: eight taps and a comparison is a real
budget on a software rasteriser and on a small device, and the two techniques
share the pass, the resource, the blur and the test.

Refused, with the reasons:

- **HBAO and HBAO+.** They read the same depth and are superseded by GTAO on it,
  so building one is a step onto a rung that is already obsolete. Nothing is
  learned on the way up that the destination does not already contain.
- **Any AO that needs a normal attachment before the SSR section's escalation
  clause actually fires.** That clause names its own trigger — a fringe of
  unrelated colour one pixel deep at silhouettes — and the attachment is its
  remedy, not AO's wish. For AO a wrong reconstructed normal costs a pixel an
  eighth of its occlusion, which is the budget this section already declined to
  spend a colour target on.
