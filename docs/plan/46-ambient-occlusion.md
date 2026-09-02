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
  tile, so it removes the _radial_ banding, and where all sixteen of its taps
  count it divides an isolated flipped sample by sixteen — which the
  depth-weighted kernel below is precise about, because it is no longer sixteen
  everywhere. **It does not remove the tangential banding**, and measurement
  2026-08-31 is why that sentence now says "radial": the tile carries only eight
  plane orientations at the shipping slice count, and averaging a footprint over
  a field that still carries the same eight spreads the step rather than
  removing it — a second blur pass on its own measurably makes it worse. What
  buys orientations is `crcbl_render::ssao`'s `r_ssao_slices`; `docs/backlog.md`
  carries the numbers on both local tiers and the two defaults still to decide.
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

### The horizon integral: its arc cosine, its trap, and what it deleted

**The arc cosine is this crate's, not the target's.** Every angle in the
integral comes through one, no target specifies its accuracy, and two
rasterisers disagreeing about `acos` is precisely the driver divergence the
section above argued GTAO would _avoid_. `crcbl_shaders::ssao::acos_approx` is
Abramowitz and Stegun 4.4.45 — a degree-three minimax fit and a square root,
both exactly specified — swept against `f64::acos` to `MAX_ACOS_ERROR`, and
`ssao.slang` carries the same four coefficients under a test that compares them
as values. The bound is asserted from _below_ as well: a ceiling nothing
approaches would pass on the intrinsic the polynomial exists to refuse.

**The trap, written down because it draws a picture rather than an error.** The
tilt of the surface inside a slice is signed by which side of the view direction
the projected normal leans towards, and the direction it is signed against must
be perpendicular to `view`. A screen-space direction lifted into view space with
a zero `z` is perpendicular to the view _axis_, and those two coincide only at
the exact centre of the frame. Sign with it and every off-centre pixel gets a
tilt leaning the wrong way, which puts both horizon clamps on the wrong sides: a
flat floor stops being unoccluded and picks up a smooth wash growing towards the
frame's edges — a vignette, which is a thing renderers have, and would have been
blessed. What caught it was `probes`' flatness assertion, which measures two
blocks of one flat floor a fifth of the frame apart and allows half a channel
level between them; the wash was three. The guard is now
`the_slice_tilt_is_signed_against_the_view_orthogonal_tangent`.

Two things the horizon integral made unnecessary, both deleted rather than left
as machinery:

- **The depth bias.** `SsaoParams::bias` and `forward.rs`'s `SSAO_BIAS` existed
  because half of a flat surface's own samples land marginally in front of it
  once depth is quantised, which a threshold comparison turns into grey haze. A
  horizon integral has no threshold: a sample in the surface's own plane lands
  exactly on the tangent, where the integral is stationary. Swept from zero to
  0.4 radians of angular bias against the same frames and it moved nothing that
  the sign fix above did not move further, so the uniform is gone and `params.y`
  is padding.
- **A self-occlusion fudge at all.** There is none in the shipped code, which is
  why `probes`' floor now matches its AO-off render byte for byte.

**SSAO did not stay as the cheap tier.** The decision above says it would, on
the FXAA-under-SMAA pattern, and that is still the right shape — but a tier
needs a selector to choose it and there is none, so keeping the eight-tap body
would have been a second technique nothing could reach. `docs/backlog.md`
carries it.

**Bent normals are still owed**, and they are the half that makes this more than
a quality bump. They need the `R8Unorm` target widened, so they are their own
slice.

**DECIDED 2026-08-30 — which tiers get which.** The user's call, on the question
of where the widened target is worth its bandwidth:

- **Low: scalar occlusion plus the multi-bounce tint. The tint has landed, and
  it landed on every tier.** `mesh.slang`'s `multi_bounce_occlusion` is Jimenez
  et al. 2016's polynomial of the scalar term and the surface albedo. It reads
  no second target and adds no second pass, so there was no bandwidth for a
  quality knob to buy back and it is unconditional: no `RenderEffects` bit, no
  console variable, no settings key. What remains of low's half is which scalar
  pass it runs, SSAO or GTAO, which is a measurement on that tier's hardware
  rather than a design choice, and it is why the cheap-rung paragraph above
  still stands: the eight-tap body is gone, and comes back behind a selector
  only if low measures for it.
- **Medium and high: bent normals plus specular occlusion.** The `R8Unorm`
  target widens to carry the bent direction beside the scalar, the ambient term
  is sampled along it, and the reflection term takes the cone's occlusion — the
  pair the SSR section's refusal has been waiting for. The widening is the
  bandwidth low does not pay.

  **This rung is not blocked by the normal-attachment refusal below, checked
  2026-09-02.** The refusal is real and stands, and this rung does not need what
  it refuses: GTAO's horizon search produces a bent direction as a by-product of
  the sweep it already runs, off the same depth-derived normal, so nothing new
  is attached to the forward pass. The one place the answer was not obvious is
  the reconstruction's fallback — where the scalar's identity is `1.0`, a
  direction has no such constant — and `shaders/ssao_upsample.slang` already
  binds `scene_depth` at the frame's own extent and loads it, so the fallback
  can be the depth-derived normal at that pixel. What the rung does cost is
  bandwidth and the blur: a direction does not filter like a scalar, and
  whatever the widened blur does it must renormalise.

**The published fit is not exactly one at full visibility, and the occlusion
off-switch is why that had to be fixed rather than measured.** The three
coefficients sum to `0.9996 + 0.0005 * albedo` — a hair under one for a dark
albedo, where the fit's own `max` returns the one, and a hair over it above an
albedo of 0.8, where nothing in the published form catches it.
`crcbl_render::forward` binds a 1×1 white image when it adds no occlusion pass,
so with the effect off _every_ fragment in the frame arrives at full visibility,
and a frame that asked for no occlusion has to be the frame it was before this
function existed.

So `multi_bounce_occlusion` clamps the top end too, and that `min` is the one
place it departs from the paper. It is a correction rather than a preference:
nothing occludes the fragment, so there is no bounce for the tint to add, and a
multiplier above one there is inventing light out of a least-squares residual.
It costs nothing anywhere else, because the cubic clears one at no other
visibility in the range. `crcbl_shaders::mesh`'s
`the_multi_bounce_tint_leaves_an_unoccluded_fragment_alone` holds both halves —
the clamped identity, and a swept bound on the raw fit that keeps the six
coefficients under a check the clamp would otherwise hide.

**It narrows the occlusion contrast a scene shows, and two suites' claims were
re-measured against that.** The tint lifts an occluded fragment by the colour of
what occludes it, which is the point, and on a bright surface the lift is large:
`crcbl`'s AO scene separated its wall bands from its open floor by a ratio of
`1.198` and now separates them by `1.058`, and `apps/lantern`'s contact corner
moved from a comfortable margin to `1.038`. Both match the published curve at
those albedos, so this is the fit working rather than the occlusion weakening.
`AO_RATIO` and `AO_LIFT` carry the new measurements and still land at exactly
`1.00` for a pass that never reached the shading line. What they have lost is
margin — `docs/backlog.md` carries that, and the AO intensity control that would
buy it back.

The presets of foundation (g) select between the two. **They exist now** —
`crcbl::settings::presets` landed 2026-08-31 — so this rung wires into them
rather than waiting for them, and it inherits their one open question: a preset
clears an effect by writing that effect's `VIDEO_KEYS` row, so whichever knob
selects the bent pair needs a row of its own or the presets need a way to write
a keyless one. `docs/backlog.md` carries that question, raised by the
contact-shadow rung, which hit it first. Both halves are priced on the three
tiers before the rung counts.

**What the pass costs, measured 2026-08-28.** `crcbl_render::PassTimers` times
every pass in the graph and `apps/lantern` builds one, so the report comes out
of `lantern --headless --frames 400 --size 1920x1080` under `RUST_LOG=info`. On
an RX 7900 XTX (radv, Mesa 26.2.1) that frame is 0.986 ms of GPU time across 53
passes and both of lantern's views, and **`ssao` is the most expensive pass in
it**: 0.255 ms, 25.9%, against `forward`'s 0.199 ms, `ssr`'s 0.099 ms,
`shadow`'s 0.070 ms and `ssao-blur`'s 0.032 ms. Sixteen depth taps, an
`acos_approx` and a `sqrt` per tap at 1920×1080 is what that buys.

**Re-measured over a distribution on 2026-08-28**, once
`crcbl_render::PassStats` existed to take one: the same run reports `ssao` at
**0.258 ms p50 / 0.263 ms p95** and 26.0% of a 0.990 ms p50 total, summed across
both of lantern's views rather than read off the room view's row. The
single-frame reading above stands — it was not a fluke of the frame it came
from.

**Re-measured again 2026-09-01, and neither the ranking nor the figure holds.**
The same command on the same machine now reports a 2.143 ms frame with `ssao` at
**0.488 ms p50 / 0.505 ms p95**, 22.8%, and `forward` ahead of it at 0.531 ms
and 24.8%. Every pass grew over the same period — `shadow` from 0.070 ms to
0.350 ms, `ssr` from 0.099 ms to 0.218 ms — which is the shadow atlas, the
cascades, normal maps and the LTC widening arriving, so the frame roughly
doubling is work that was added rather than a regression.

**The `ssao` pass is not where that came from, and this was checked rather than
assumed.** `docs/backlog.md`'s 2026-08-31 sweep reads 582 µs for the same
two-slice pass, against 0.258 ms here three days earlier, and the tangential
rung is the only AO change between them — so the rung was the suspect. It is
not: compiling the pre-rung `ssao.slang` against today's tree and running the
same command measures **0.518 ms**, _slower_ than the 0.488 ms the current one
takes. The rung made the pass slightly faster. A full-screen pass's cost is not
scene-independent here — `MIN_RADIUS_PIXELS` lets a distant or flat pixel leave
the march early — so a denser room is the remaining candidate, and it has not
been isolated.

**It is not a comparison.** The eight-tap hemisphere is deleted, so what GTAO
costs _against what it replaced_ is not measurable from this tree — recovering
it is the `git show` the tier note above describes, and a quality seam that
offered the cheaper rung would need exactly that number to be honest about it.

### Half resolution, and the reconstruction that carries it (built 2026-09-02)

> **Built, and this section describes the pass as it stands.** Everything above
> describes a gather at the frame's own extent, which is not what runs.

The gather and both blurs run at `crcbl_render::ssao::half_extent`, and
`ssao_upsample.slang` carries the result back to the frame. That shader is
**depth-aware rather than bilinear**, which is the whole reason it can exist: a
full-resolution pixel beside a silhouette has half-resolution neighbours on the
_other_ surface, and weighting those by distance alone averages the background's
occlusion into the foreground's rim.

Its tap loop reads the block's own sample and the next one along each axis — one
tap where the two grids coincide, two where they do not — and weights each by
distance and by how near its surface is, on the depth tolerance
`ssao_blur.slang` shares. A tap on the far plane takes no share at all. The
nearest tap keeps a floor it cannot lose, so a pixel whose every tap is rejected
— a surface thinner than the occlusion grid — still has a divisor rather than
dividing zero by zero.

**What it cost is quality on the tangential axis**, not correctness: the blur's
footprint now spans twice as much of the frame, so each tile phase pairs with a
wider spread of distances from an edge, and the shipping slice/blur pair became
markedly less smooth there. `docs/backlog.md`'s HIGH PRIORITY entry carries the
measurements on all three tiers and the decision they inform — whether the
tangential rung's defaults move — and is where those numbers belong, since they
move whenever the pass does.

`crates/crcbl/tests/forward_e2e/occlusion.rs` is the harness: the silhouette is
measured on both axes, the reconstruction is held to the nearest gathered sample
for a sliver that misses the grid entirely, and every threshold in it was swept
on radv and lavapipe.

### The occlusion view

`lantern`'s pause panel has an `AO VIEW` row, deliberately below its `AO` row
and deliberately **not** a `RenderEffects` toggle, because the two are different
questions: the row above turns the pass off, this one changes which picture is
drawn.

**Grey, not a false-colour ramp.** A ramp puts a visible edge where the hue
changes and none where the occlusion steps, so it reads as structure the pass
did not compute. Luminance has the gradient the eye is being asked about.

**A frame with the pass switched off draws white, and that is the honest
answer.** What the branch samples is whatever occupies the binding, which on
such a frame is the 1×1 white image the renderer substitutes for a computed
channel — so "no occlusion was computed" and "nothing occludes here" are the
same value by construction, exactly as they are for the ambient term that
multiplies by it. That coincidence is what the device test turns into evidence:
`the_occlusion_view_draws_the_channel_and_not_a_constant` renders one scene
twice, separated by `RenderEffects::AMBIENT_OCCLUSION` alone, and needs the
first frame uniformly white **and** the second not — a branch returning a
literal white passes the first half and fails the second, and a
`set_occlusion_view` wired to nothing fails it too.

**The cube alone was not a scene worth measuring.** With nothing under it the
only occluded texels are the rim of its own silhouette — four of them in a
256×192 frame, measured — so the probe puts a slab under the cube and reads 4755
occluded texels out of 32425 drawn. A population that thin cannot tell a working
pass from a rounding step.

This is milestone 1 of [sample/19-alcove.md](sample/19-alcove.md)'s three parts,
and the one that document says to build first. **The intensity control exists**
— `crcbl_render::ssao::r_ssao_intensity`, a console variable the pass reads
every frame — so what is left of that milestone is the scene and a live _radius_
control, which has no knob of any kind. A console variable is live but it is not
_shown_, which is half of what that sample asks a control to be.
