# Topic 48 — The post-processing stack: order, HDR, tonemap, bloom

Split out of [18-render-features.md](18-render-features.md) on 2026-08-27,
verbatim. That topic had grown past a hundred kilobytes and a reader after one
technique had to carry six others to reach it; topic 18 is now the index that
orders these and holds what is genuinely cross-cutting — the interactions, the
delivery table and the risks.

## Post-processing stack

Pipeline order (all at internal render resolution, before the topic 15
render-scale upscale; UI composites after, at native resolution):

```
scene (HDR RGBA16F) → bloom (down/upsample chain) → exposure + tonemap → FXAA → [upscale] → UI
```

**`[upscale]` was built on 2026-08-27**, and the order above stopped being a
contract for a pass that does not exist. `ForwardRenderer::set_render_scale`
sizes an internal target at a fraction of the caller's extent — the cluster
grid, the level-of-detail pixel budget, the Hi-Z pyramid, bloom and FXAA all
follow it there — and `shaders/upscale.slang` reconstructs that target into the
caller's own as the last pass of the frame. Every stage of this chain now
genuinely costs what the internal extent says, which is the whole reason the
order is written this way.

**At full scale there is no pass and no second image**: the stage before it
writes the caller's target directly, the same additive-zero shape the FXAA rung
landed in, so a frame that asked for no scaling is what it was before the pass
existed. The filter is Catmull-Rom, sixteen taps, priced against bilinear in
[43-render-standards.md](43-render-standards.md)'s §7.

**The seam above the renderer is still missing.**
[15-windowing.md](15-windowing.md) defines borderless as an internal render
target upscale-blitted to the native surface and `ShellCaps::HW_UPSCALE` reports
what a window system will do for free, but no settings key reads `render_scale`
and no `Shell` carries a request for it. The knob is a method and nothing but a
test calls it.

- **HDR (MVP, lands with P7)**: scene renders to RGBA16F; lighting in linear HDR
  from the start (retrofitting HDR is repainting every material — do it the
  moment real lighting exists). Fixed exposure MVP; auto-exposure (histogram,
  GPU reduce) **built 2026-08-29** — see the rung below.
- **Tonemap (MVP)**: filmic/ACES-fitted curve + sRGB encode. One combined
  fullscreen pass with exposure. **Built 2026-08-27, and the clamp stayed the
  default.** `tonemap.slang` carries two operators behind a `uint curve` lane of
  its block — exposure-and-clamp, and Stephen Hill's fit of the ACES RRT and ODT
  — and `crcbl_render::ForwardRenderer::set_tonemap_curve` is what a view asks
  with. Fixed exposure is still a runtime uniform, and auto-exposure is the lane
  beside it rather than a replacement for it — the rung below says why.

  **The default is the clamp for the reason P1 chose it**, and that reason
  outlived the curve arriving: exposure-and-clamp is the identity on `[0, 1]`,
  so display-referred content — every 2D sample in this tree — reaches the
  swapchain exactly. A filmic curve over a sprite an artist already graded moves
  colours somebody chose, and it would have re-blessed the whole 2D suite for a
  picture nobody asked to change. So the operator is per view, not per engine,
  and flipping which one a 3D stack defaults to is a separate change whose whole
  content is the re-bless — exactly the shape the FXAA rung landed in.

  **ACES rather than AgX**, which is otherwise the newer answer and the one
  Blender and Filament moved to. AgX takes a `log2` and a `pow` per channel, and
  this workspace's determinism rule is that a shader uses no transcendental
  function, because four platforms' implementations of them differ in the last
  place. Hill's fit is two changes of primaries around a rational polynomial —
  multiplies, adds and divides only — so it can be blessed on all four backends.
  `crcbl_shaders::tonemap::TonemapCurve::apply` is the same arithmetic on the
  CPU, pinned against the ODT's published anchors (a neutral stays neutral; a
  scene-referred 0.18 lands near a tenth of display range), and a source grep
  holds the shader to the same constants.

- **Auto-exposure**: a luminance histogram of the finished frame and a reduce
  over it, no readback. **Built 2026-08-29.** `shaders/exposure.slang` is three
  entry points in the order a frame runs them — `clearMain` zeroes the bins,
  `histogramMain` bins one texel per invocation with an atomic add, and
  `reduceMain` walks the bins on a single invocation and divides the key by the
  average luminance of the frame's middle. `crcbl_render::exposure` owns the
  three pipelines and the two buffer rings; `RenderEffects::AUTO_EXPOSURE` is
  the bit, `auto_exposure` the settings key.

  **The bins are integer arithmetic, not a `log2`.** The exponent field of an
  IEEE-754 float is the floor of its base-two logarithm, so the bin index is a
  shift and a subtract, and the bin's lower edge is that exponent written back
  into a float — the trick `mesh.slang` already uses. That is what lets the
  histogram be identical on four backends where the transcendental this rung
  otherwise wants is identical on none of them.

  **It is out of `DEFAULT_STACK`**, and it is the first post effect that has no
  additive-zero form: an exposure the frame measured is not the exposure the
  caller set, so switching it on is always a different picture and every golden
  in this tree would have to be re-blessed to make it a default. A view asks.

  **The reduce is one invocation on purpose.** Float addition is not
  associative, so a tree reduction sums the bins in an order the device
  schedules and two runs of the same frame need not agree. Ninety-six bins on
  one lane costs less than the atomic traffic the pass before it already paid.

  **Adaptation landed the same day, and it is a step rather than a jump.** The
  reduce writes what the frame before was exposed by, moved a fraction of the
  way toward what this frame's histogram asks for; the fraction is
  `rate * delta` clamped into `[0, 1]`, and the two rates differ by direction
  because a real eye adapts down to a bright scene quickly and back up slowly.
  What carries the previous value is the `measured` ring itself — the reduce
  binds the slot behind the one it writes, which is the frame before — and
  `crcbl_render::exposure` fills every slot with the default exposure before any
  frame exists, so the first frame's step starts somewhere defined rather than
  from whatever the allocation came with.

  **The step is linear, not `1 - exp(-rate * delta)`**, which is what the
  literature and every engine write. The exponential is the honest model of an
  approach and this is its first-order term; taking it costs a `log`-family
  intrinsic in the arithmetic that produces the exposure, and an exposure
  multiplies every texel of the frame, which is exactly what this workspace's
  determinism rule refuses. The visible difference is the shape of the last
  tenth of the roll, and the two rates are a stronger lever over how a cut feels
  than that shape is.

  **Both endpoints are exact.** `previous + (target - previous) * 1` is not
  `target` in floating point, so a blend of one — what a view that asked for no
  adaptation gets — takes a branch that writes the target itself, and a blend of
  zero writes the previous itself. That is what keeps a frame with no adaptation
  asked for identical to the frame drawn before adaptation existed, rather than
  merely close to it.

  What is **not** here is a settings key: `auto_exposure` switches the effect on
  and off, and the rates are an API a view calls with its own frame delta. There
  is no clock in `crcbl-render` to take the delta from, which is why.

- **AA (MVP)**: **FXAA**, then SMAA 1x, with TAA post-MVP and MSAA priced rather
  than rejected — the whole ladder, what each rung costs in this tree and what
  is refused are [49-antialiasing.md](49-antialiasing.md). **Built 2026-08-27**:
  `fxaa.slang` and `crcbl_render::fxaa` are one fullscreen resolve after the
  tonemap, `RenderEffects::ANTIALIASING` is the bit, and it is in
  `DEFAULT_STACK` — so every frame this engine draws is resolved. SMAA 1x is the
  next rung and is not built.

  **The prev-transform slot is reserved as of 2026-08-27**, which this row
  claimed for a year before it was true. `crcbl_shaders::mesh::GpuInstance`
  carries `previous_transform` beside `transform`, `INSTANCE_STRIDE` is 160, and
  `crcbl_render::InstancePool` fills it without the caller — so the instance
  format is no longer a widening TAA has to pay for. What TAA still owes is the
  pass: a motion-vector target, the subtraction that writes it, and the previous
  frame's view-projection in the frame block.

- **Bloom (P10)**: physically-plausible threshold-free downsample chain (Karis
  average), 5–6 mips, tent upsample, additive with scalar. Cheap, huge
  perceived-quality win — timed with the UI/debug polish phase so the profiler
  HUD can show its cost honestly.
- Stack is data-driven per camera (RON: which passes, parameters) —
  games/samples tune without engine edits; settings UI (topic 14 P10) exposes
  quality toggles.

### Where the toggles live

Every feature in this document is switchable at three layers, resolved in one
place, per [39-capabilities.md](39-capabilities.md):

```
camera stack (this RON) declares what the view wants
  → [engine.video] clamps it downward as a player quality setting
  → programmatic override may set it either way
  → device capability clamps it downward, last and absolutely
```

The per-camera layer is the one this topic owns, and it is genuinely per view: a
render-to-texture camera driving a security monitor, a planar reflection, or a
weapon-scope PiP (topic 29) does not want reflections or GI of its own, and that
is a property of the camera rather than of the player's hardware.

**Built 2026-08-14, and two of the four layers have no source in the tree.**
`crcbl_render::effects` is the resolution point: `RenderEffects` is the effect
set, `EffectRequest` carries the three requested layers, and
`EffectRequest::resolve` applies the whole order in one place.
`ForwardRenderer::begin_frame` resolves once per frame and freezes the answer,
so the half of a frame that parametrises the shadow culls and the half that
dispatches them cannot disagree.

- **Programmatic** is wired: `ForwardRenderer::set_effect_request`, and
  `apps/lantern`'s `--no-shadows` / `--no-ao` / `--no-reflections` drive it.
  There is no `--no-bloom`, and there is nothing for one to turn off: bloom is
  the one effect **not** in `RenderEffects::DEFAULT_STACK`, so a view that has
  declared no render stack — which is every view in this workspace but the
  `Scene::Bloom` fixture — is not drawing it to begin with. The reason is on
  that constant: the other three approximate light transport present in the
  scene, and a camera given no stack has been given no lens.
- **Device** is wired to `DeviceCaps` and **removes nothing**, which is a fact
  about these three effects rather than an unfinished clamp —
  [46-ambient-occlusion.md](46-ambient-occlusion.md) says it of the occlusion
  pair in as many words, the reflection pair's module says it of itself, and a
  device too small for the shadow atlas fails to build the renderer rather than
  degrading past it. The first real rule arrives with the ray-traced variants,
  which `LightingPath` selects.
- **Camera stack** is a field nothing writes: there is no render-stack RON, and
  nothing in the workspace reads or writes RON at all.
- **`[engine.video]`** is wired: `GpuContext` reads the player's settings file
  while it opens — `SettingsSource::Platform` by default, so every sample and
  the `crcbl new` scaffold get it without asking — and
  `GpuContext::effect_request` hands the layer to a renderer built on that
  context. `crcbl::settings`' `VIDEO_KEYS` is the one place a key is spelled,
  and a key that is absent clamps nothing, because this layer may only remove.
