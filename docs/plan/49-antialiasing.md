# Topic 49 — Antialiasing: FXAA, SMAA, CMAA2, TAA and the MSAA question

Split out of [18-render-features.md](18-render-features.md) on 2026-08-27,
verbatim. That topic had grown past a hundred kilobytes and a reader after one
technique had to carry six others to reach it; topic 18 is now the index that
orders these and holds what is genuinely cross-cutting — the interactions, the
delivery table and the risks.

## Antialiasing

The stack's AA slot, and the ladder that runs through it. The first rung of it
is in the tree; the rest of this section is what the rungs above it are.

### FXAA 3.11 first — landed 2026-08-27

`crates/crcbl-shaders/shaders/fxaa.slang` and `crates/crcbl-render/src/fxaa.rs`:
one fullscreen pass over the tonemapped image — a luma edge detect, a subpixel
blend along the edge it found, no history, no new attachment and no change to
any pass in front of it. The cheapest thing that removes the staircase, and the
tier that stays after the rung above it lands.

**It is `RenderEffects::ANTIALIASING`, and it is in `DEFAULT_STACK`** — flipped
in a second change, whose whole content is the re-bless the last item of the
cost list below describes. Every frame the engine draws is resolved; the lens is
now the only effect a view has to ask for by name.

**A debug view takes the resolve off again**, and
`ForwardRenderer::resolved_effects` is where that happens rather than in any
caller. `DebugView::Heatmap`, `LodTint` and `Normals` are readouts: a pixel's
colour _is_ a cluster's projected error or its DAG level, read against a legend,
and a filter that blends two clusters' shades invents a ramp position no cluster
occupies. `apps/quarry`'s heatmap and LOD tests count a frame's distinct colours
and are what found this — under the flip they went from 2 colours to 64.

**Three tests measured a pixel the resolve had moved**, and each is a different
answer to the same question. `crcbl`'s
`the_resolve_is_what_puts_the_soft_pixels_there` compares the same scene with
and against the bit, so it names the bit itself as its control. The HDR fixture
in `crates/crcbl/tests/mesh_e2e/hdr.rs` refuses the bit, because it reads back
the single swapchain texel under the HDR peak and asks whether the _tonemap_
clamped it — a filtered texel would make that assertion about the filter. And
`crcbl-vk`'s per-pass timer report and `draw_gen_e2e`'s `FULLSCREEN_INSTANCES`
both simply grew by one, which is the shape of the frame changing and not a
measurement moving.

**Switching it on changes the shape of the frame rather than adding a pass to
it**, which is the one structural thing here worth knowing. Every other
fullscreen pass reads a transient and writes a transient; this one reads what
the tonemap wrote and writes what the UI is composited onto. So with the bit off
the tonemap writes the caller's target directly, and with it on the tonemap
writes a `display-color` transient at the target's own format and the resolve
writes the target. The ground grid moves with the tonemap and not with the
resolve — it is a field of thin high-contrast lines, which is what an edge
filter exists for — and the UI stays behind the resolve, so glyphs are never
filtered.

**Two things the native gates could not see, and one of them was a defect.**
`fxaa.slang` samples with `SampleLevel` and not `Sample` everywhere, because
WGSL refuses an implicit-LOD sample reached from non-uniform control flow and
every tap in the filter is reached from some — the early-out returns before them
and the edge search runs its steps under a flag. All four targets compiled the
implicit form without complaint; what caught it was
`web/run-render-harness-e2e.sh`, where a WGSL module that will not parse is a
device that refuses the pipeline and a scene that draws nothing. The other is
the linear luma correction the source's header describes: the tonemap writes
linear values into an sRGB-format target and lets the hardware encode, so a pass
sampling that target sees linear, and FXAA's thresholds were fitted to gamma
space.

Its fixture is `Scene::Aa` — one slab turned about the view axis, so its
silhouette runs diagonally between two flat levels — and the claim its golden
cannot make is in `the_resolve_is_what_puts_the_soft_pixels_there`, which draws
that same scene twice through `crcbl::screenshot::aa_forward` and compares. What
the fixture pins is a band rather than a reading: at least `AA_MIN_SOFT_PIXELS`
soft pixels with the resolve, at least four times fewer without it, and a mean
level moved by no more than `AA_MEAN_TOLERANCE`. A run's own numbers were
written here once and are not what goes red when the filter changes.

**Its template is `crates/crcbl-shaders/shaders/bloom_composite.slang` and not
`crates/crcbl-shaders/shaders/tonemap.slang`**, which is worth saying because
the obvious answer is the wrong one. The tonemap is a 1:1 blit — it samples at
the pixel's own centre through a nearest sampler, so it reads one texel and no
neighbour, which is the whole of its determinism argument. (It is `Sample`
through a declared `SamplerState` rather than a `Load`; the sampler the renderer
creates for it is what makes the two the same read.) The bloom composite already
carries both halves FXAA needs: the same fullscreen triangle out of
`SV_VertexID`, and a neighbourhood gathered around a UV through an `inv_source`
texel-size uniform its Rust mirror writes once per frame. An `fxaa.slang` is
that file with the tent replaced by the edge detect.

What it cost, item by item, because none of it was hypothetical:

- One `.slang` source and **four committed artifact sets** — SPIR-V, WGSL, MSL
  and DXIL — each hashed into the manifest `crates/crcbl-shaders/tools/` writes
  and `--check` gates.
- A params mirror under `crates/crcbl-shaders/src/`, on
  `crcbl_shaders::bloom::BloomParams`'s terms: one block, declared once,
  agreeing with the source member for member.
- **A fifth `RenderEffects` bit, which is not free.**
  `crates/crcbl-render/src/effects.rs`'s `NAMES` table is as long as the type
  has flags, so an unnamed fifth effect does not compile, and
  `every_effect_is_named_exactly_once_and_the_row_prints_them` pins the exact
  string `DEFAULT_STACK.row()` produces — the row every sample's summary line
  and debug panel print. Putting the bit in the default stack changes that
  string, so the assertion moves deliberately rather than by surprise.
- A pass in `crates/crcbl-render/src/forward.rs` shaped like the tonemap block
  it follows: a pipeline, a layout, a params buffer per frame in flight and a
  bind-group ring keyed on the views it reads. `RENDER_PASSES` grows a term and
  `fullscreen_passes` grows a branch, which is what keeps the frame's timer
  count matching the frame.
- **A re-bless of every golden the bit is on for.** FXAA moves every edge in
  every frame it runs on, so there is no additive-zero property to land it
  behind — the probe and bloom slices had one and this does not. The switch
  therefore decides how much of the suite moves, and the honest default is the
  one that moves it exactly once. That is what the flip spent: seventeen images
  under `crates/crcbl/tests/golden/`, six under `apps/quarry/tests/golden/` and
  two under `apps/lantern/tests/golden/`. The `crcbl` and `lantern` sets are
  blessed on the software path (`CRCBL_ADAPTER=cpu`) and quarry's on the
  discrete adapter, which is where each was blessed before; every one of them
  was then verified against **both** adapters.

### SMAA 1x second, and it is the real industry standard step

**SMAA 1x is what the engine reaches for where FXAA over-blurs text and thin
geometry** — not TAA, and not a wider FXAA preset. It is **built**:
`crcbl_render::Smaa`, its three shaders, the cooked tables,
`RenderEffects::SMAA` and `Antialiasing::Smaa`, with
`smaa_changes_a_band_along_the_edges_and_nothing_else` as its observer. What
follows is the design it was built to, kept because the refusals in it still
bind. Three passes: an edge detection, a blend-weight calculation that looks the
detected pattern up in a precomputed **area** table and a **search** table, and
a neighbourhood blend that applies the weights. Each is the fullscreen shape the
tier below establishes, so the pass machinery is the same machinery a third
time.

Two things about it are specific to this tree:

- **The lookup tables are a data cost, not a computation.** They are on the
  order of 160 KB and have to arrive as **committed bytes** with a generator and
  a `--check` mode behind them, on `cook-clusters`' precedent and hashed the way
  `spirv/manifest.txt` hashes an artifact. Deriving them at start-up instead
  would put a table four rasterisers computed independently underneath every
  golden in the suite, which is the read this file's determinism arguments spend
  their whole length avoiding.
- **It is historyless, so it is deterministic by construction**, and that is
  what makes it golden-safe where TAA is not. Its inputs are one frame's pixels
  and two constant tables; no frame it draws is a function of how many frames
  preceded it.

FXAA does not leave when SMAA arrives. **It stays as the cheap tier**, on the
terms `RenderEffects` already gives the other pairs: a tier that is off is a
frame with fewer passes, not a shader branch.

**What the rung still owes is cross-backend evidence** — Metal, DX12 and WebGPU
compile the artifacts, and only CI has run them — which `docs/backlog.md`
carries.

**Superseded 2026-08-30**: the eighth decision below puts CMAA2 in this tier's
place, and the three shaders, the two tables, `cook-smaa` and
`crcbl_render::smaa` leave the tree with the slice that lands it. What stays is
the observer's shape — `crates/crcbl/tests/mesh_e2e/smaa.rs`'s three
measurements are what CMAA2 is held to, on the same scene.

### TAA is specified and still post-MVP

TAA needs two things this tree does not have:

- **A per-frame subpixel jitter on the projection**, which changes the camera
  matrix every golden in the suite is drawn through.
- **A history target with neighbourhood clamping**, which makes a frame a
  function of how many frames were drawn before it. That is the property
  [47-reflections.md](47-reflections.md) already refuses in writing for SSR
  history, and [50-irradiance-probes.md](50-irradiance-probes.md) again for
  DDGI.

The motion vectors it reads are in the frame. **The convention is
texture-coordinate space, current minus previous, `+y` down**, so this rung's
history buffer is sampled at `uv - motion` — written on the format constant and
on `TransientImageDesc::motion`, and observed by `DebugView::Motion`,
`crates/crcbl/tests/mesh_e2e/motion.rs` and — for a deformed surface, through
`GpuInstance::previous_base_vertex` —
`crates/crcbl/tests/mesh_e2e/skinned_motion.rs`. **What the target does not yet
carry is the camera's motion where the sky shows through**, which is
`docs/backlog.md`'s and this rung's to finish.

`crcbl_render::skinning`'s `SkinnedRegion::previous_base` was the half of the
reservation taken first: topic 17's 2026-07-27 correction double-buffers the
skinned-output pool region from day one and a frame alternates which run it
writes. The instance side followed on 2026-08-27.

### A seventh, taken 2026-08-27: MSAA is reopened, priced, and still not the default

The AA row rejected MSAA for fighting "deferred-ish/HDR pipelines", and **that
is deferred-renderer reasoning applied to a renderer that is not deferred**.
This engine is clustered forward, and [44-lighting.md](44-lighting.md)'s
"Clustered forward" section rejected deferred partly _because_ deferred fights
MSAA. A rejection cannot be inherited from the argument it was the counterweight
to.

The seam already carries it. `crates/crcbl-hal/src/pipeline.rs`'s
`MultisampleState` has `samples` and `alpha_to_coverage`, every pipeline in the
tree takes one, and its `Default` says in as many words that MSAA is available
and never the default. So the honest position is not "rejected" but **viable and
priced**, and the price is specific:

- **The depth prepass has to be multisampled too.** The forward pass attaches
  the depth the prepass wrote, and a single-sample depth image cannot be
  attached beside a multisampled colour target.
- **Both screen-space passes read that depth, and each wants one sample of it.**
  `ssao.slang` reconstructs a normal from four neighbouring depths and
  `ssr.slang` marches it tap by tap. So MSAA buys either a depth **resolve**
  before those passes — a pass and an image the frame does not have — or
  per-sample versions of both, which is the occlusion pair and the reflection
  pair rewritten.

That is why it is not the default, and it is a reason rather than a refusal.
**MSAA is the right answer for a forward renderer doing little screen-space
work**; FXAA and then SMAA are the right answer for this one for exactly as long
as SSAO and SSR are in the stack. A view that drops both — which the per-camera
effect layer above already allows — is a view where the arithmetic flips, and
the reader holding that view is the one who should make the call.

### An eighth, taken 2026-08-30: one AA row, and MSAA as its top rungs

Counter-Strike 2's video panel is the comparand, and its AA row is one ladder:
None, CMAA2, then 2×, 4× and 8× MSAA, with 4× the default and **no temporal
option at all** — Valve's forward renderer refused TAA for clarity, on the same
grounds this section's determinism arguments refuse it for goldens. Its
filtering row is bilinear, trilinear and anisotropic 2× to 16×, which
`apps/options`' `ANISOTROPIES` row already matches rung for rung.

What this tree had instead, until the cycler below landed, was **two independent
bits** — `RenderEffects::ANTIALIASING` and `RenderEffects::SMAA` — each its own
settings row, so the panel could switch both on and the resolve slot picked
between them out of sight. Neither is a `VIDEO_KEYS` row today; that table's own
comment says why. The next two rungs of this ladder are the CS2 shape:

1. **One `antialiasing` cycler row — built 2026-08-30.**
   `crcbl_render::Antialiasing` is the ladder (`None`, `Fxaa`, `Smaa`; CMAA2 and
   the MSAA rungs arrive with the slices below). The
   `[engine.video] antialiasing` key holds `Antialiasing::name`'s word, the
   `smaa` key is gone, and `VIDEO_KEYS` is a six-row table of pure booleans
   again. `EffectRequest` grew `antialiasing: Option<Antialiasing>` and
   `resolve` applies it as a **replacement** inside the AA slot — after the
   video clamp, before the programmatic override, before the device — because a
   clamp cannot choose SMAA where the camera asked for FXAA.
   `GpuContext::antialiasing` reads it while the context opens and
   `effect_request` carries it. `apps/options`' `ANTIALIASING` row sits beside
   `ANISOTROPY`, is born on whatever `RenderEffects::DEFAULT_STACK` carries —
   which is what an absent key means — and is corrected to the file's rung on
   the first frame. `RenderEffects::DEFAULT_STACK` did **not** change: which
   tier the default carries is still the answer this row's default _is_, and
   flipping it is still the user's call and a re-bless — **and the call is
   taken, 2026-08-30: it is not flipped to SMAA.** CMAA2 becomes the default
   tier when its slice lands, so the goldens re-bless once for the filter that
   stays rather than twice. `web/tools/browser-e2e.mjs`'s `toFader` moved with
   the row and the options browser gate was run locally.

   A file still holding the boolean reads as the meaning it had:
   `antialiasing = true` was "the player has not asked for less", which is an
   unpicked tier; `antialiasing = false` was "no resolve", which is
   `Antialiasing::None`. Neither warns. No other migration — everything here is
   v0.

2. **CMAA2 in SMAA's place** — the user's call, 2026-08-30, taken with CS2's row
   in front of them, and the trade is written out under the refusal below.
   Intel's Conservative Morphological AA 2 (Strugar, 2018) is the same tier as
   SMAA 1x — one frame's pixels, no history — in a different shape: an edge
   detect writes a candidate list and only the candidates are classified and
   blended, so the cost is proportional to the frame's edges rather than to its
   pixels, which is the tier the software and browser paths most want. It
   carries no lookup table, so nothing is cooked; its shape rules are analytic
   and it is deliberately conservative — text and thin lines stay crisper than
   SMAA leaves them, gentle slopes keep a little more of their step. Three
   things about it are specific to this tree:
   - **It is compute, not a fullscreen triangle**: an edge pass, a candidate
     pass and a deferred colour-apply pass, with the candidate list and the
     per-pixel blend items in storage buffers a work-group appends to. Those are
     the AA stage's own buffers and stay under
     `crcbl_hal::PORTABLE_STORAGE_BUFFERS_PER_STAGE`, which the forward pass
     already sits against.
   - **The blend items reach a pixel in atomic order**, and the reference sums
     them in that order. A float sum in an order the hardware chooses is a frame
     that is not a function of its inputs — this file's determinism argument —
     so the apply pass accumulates in fixed point, or sorts a pixel's items
     before it sums them. Which one is measured, on both rasterisers, before a
     golden is blessed on it.
   - **It is held to SMAA's observer**: the resolve moves a share of the frame
     near a luma discontinuity and halves the count of pixels stepping by 96 of
     255, on `mesh_e2e`'s scene, on radv and lavapipe. The SMAA tier leaves in
     the same slice, so there is one morphological tier in the tree at any time
     and the resolve slot still records one filter or the other.
3. **MSAA 2×, 4× and 8×** as the rungs above CMAA2, on the price the seventh
   decision above put on it: the depth prepass goes multisampled, and one
   **depth resolve** pass writes the single-sample image `ssao.slang`,
   `ssr.slang` and the Hi-Z pyramid read today, so neither screen-space pass is
   rewritten. The froxel and light-cluster passes do not care. MSAA is
   **opt-in**, never the default: the software and browser tiers pay for every
   sample, and 4× on lavapipe is the wrong default for a suite that runs there.

### What is refused

- **Keeping SMAA 1x beside CMAA2.** Both are one tier — one frame's pixels, no
  history, an edge classification and a blend — and a menu with both is a row
  nobody can choose between. This section first declined CMAA2 on that ground,
  with SMAA already built and measured; the user reversed it the same day
  (2026-08-30) for CMAA2's cost model, which scales with edges rather than
  pixels and so favours the lavapipe and browser tiers every golden runs on, and
  for the crisper text its conservatism leaves. So SMAA is the one that goes,
  and its retirement is part of the CMAA2 slice rather than a follow-up.

- **DLSS.** Single-vendor and closed: it runs on one hardware line behind an
  SDK, where every other path in this engine is held to being the same code on
  all four backends. A quality tier that exists on one adapter is a second
  renderer wearing a capability flag.
- **FSR 2 and FSR 3.** Temporal, so they inherit **everything** TAA still owes
  above — the jitter and the history target — and add a history of their own on
  top. Being vendor-neutral answers the objection to DLSS and touches none of
  the reasons TAA is post-MVP.
- **Any AA that resolves after the UI pass.** The UI composites at native
  resolution after the upscale seam, deliberately, so its text is rasterised
  sharp. Running an edge filter over it afterwards blurs glyphs that were never
  aliased, which is a regression with a quality setting's name on it.
