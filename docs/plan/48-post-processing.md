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
scene (HDR RGBA16F) → bloom (down/upsample chain) → exposure + tonemap + grade
  → antialiasing resolve (FXAA | CMAA2) → [upscale] → UI
```

**`[upscale]` was built on 2026-08-27**, and the order above stopped being a
contract for a pass that does not exist. `ForwardRenderer::set_render_scale`
sizes an internal target at a fraction of the caller's extent — the cluster
grid, the level-of-detail pixel budget, the Hi-Z pyramid, bloom and FXAA all
follow it there — and `shaders/upscale.slang` reconstructs that target into the
caller's own as the last pass of the _render_ chain; the UI composites after it,
which is the order the pipeline further down states. Every stage of this chain
now genuinely costs what the internal extent says, which is the whole reason the
order is written this way.

**At full scale there is no pass and no second image**: the stage before it
writes the caller's target directly, the same additive-zero shape the FXAA rung
landed in, so a frame that asked for no scaling is what it was before the pass
existed. The filter is Catmull-Rom, sixteen taps, priced against bilinear in
[43-render-standards.md](43-render-standards.md)'s §7.

**The seam above the renderer arrived 2026-08-28**:
`[engine.video] render_scale` is read by `crcbl::settings::video`,
`GpuContext::render_scale` carries it, and `apps/viewer` hands it to the
renderer. What no `Shell` carries yet is the free half —
[15-windowing.md](15-windowing.md) defines borderless as an internal target
upscale-blitted to the native surface and `ShellCaps::HW_UPSCALE` reports what a
window system will do for free, and that is still a definition without a
request.

- **HDR (MVP, lands with P7)**: scene renders to RGBA16F; lighting in linear HDR
  from the start (retrofitting HDR is repainting every material — do it the
  moment real lighting exists). Fixed exposure MVP; auto-exposure (histogram,
  GPU reduce) **built 2026-08-29** — see the rung below.
- **Tonemap (MVP)**: filmic/ACES-fitted curve + sRGB encode. One combined
  fullscreen pass with exposure. **Built 2026-08-27, and the fit is the
  default.** `tonemap.slang` carries two operators behind a `uint curve` lane of
  its block — exposure-and-clamp, and Stephen Hill's fit of the ACES RRT and ODT
  — and `crcbl_render::ForwardRenderer::set_tonemap_curve` is what a view asks
  with. Fixed exposure is still a runtime uniform, and auto-exposure is the lane
  beside it rather than a replacement for it — the rung below says why.

  **The operator is per view, not per engine.** `ForwardRenderer` starts on the
  fit because it shades in linear HDR, and what a caller asks _for_ is usually
  the clamp: it is the identity on `[0, 1]`, so display-referred content reaches
  the swapchain exactly, and a fixture predicting a code value from a host model
  of the shading needs the frame to stay scene-referred. Every 2D sample in this
  tree draws through the sprite and UI passes and never reaches this one at all.
  A debug view resolves to the clamp whatever a caller set —
  `ForwardRenderer::resolved_tonemap_curve` — because a readout's pixels are
  data and a curve is a monotone remapping of every one of them.

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

- **AA (MVP)**: **FXAA**, then CMAA2, with TAA post-MVP and MSAA priced rather
  than rejected — the whole ladder, what each rung costs in this tree and what
  is refused are [49-antialiasing.md](49-antialiasing.md). **Built 2026-08-27**:
  `fxaa.slang` and `crcbl_render::fxaa` are one fullscreen resolve after the
  tonemap, `RenderEffects::ANTIALIASING` is the bit, and it is in
  `DEFAULT_STACK` — so every frame this engine draws is resolved.

- **Bloom (P10) — built.** Physically-plausible threshold-free downsample chain
  (Karis average), tent upsample, additive with scalar: `crcbl_render::bloom`,
  `bloom_down.slang` / `bloom_up.slang` / `bloom_composite.slang`,
  `crcbl_shaders::bloom::BloomParams`, `RenderEffects::BLOOM` and the
  `Scene::Bloom` fixture. `docs/backlog.md` carries what the slice left.
- **Stack is data-driven per camera (RON: which passes) — built 2026-09-06.**
  `crcbl_render::stack::CameraStack` is the file: one optional pass per
  `RenderEffects` bit, `compile` is a bit per `Some`, and
  `ForwardRenderer::set_camera_stack` writes the camera layer with the other
  three left alone. `apps/lantern/assets/camera.ron` is a demo tuning itself
  without an engine edit, `--stack` points the same demo at another file, and
  `crcbl::screenshot`'s bloom fixture composes one from a string literal. **The
  parameters half is not built**: every pass type but the antialiasing slot is
  field-less, because the renderer's per-pass parameters are setters (`set_fog`,
  `set_exposure_adaptation`, `set_tonemap_curve`) whose types have no serialized
  form yet — `docs/backlog.md` lists which. The settings UI (topic 14 P10)
  exposing quality toggles is the `[engine.video]` layer's, and it is wired.

### Colour grading: a post-tonemap 3D LUT, decided 2026-09-06

[43-render-standards.md](43-render-standards.md) §6 marks colour grading, depth
of field and lens artefacts **missing**, and grading is the one of the three
that the pass order is a contract for, so it is specified here and the other two
follow it.

**The form is a post-tonemap 3D lookup table**, which is what Unreal, Unity and
Godot 4 all ship. It is applied to the display-referred colour the tonemap
produced, so a grade is a function of what the viewer sees rather than of the
scene's radiance, and an artist's grade survives a change of tonemap operator
rather than being invalidated by one.

- **32³ texels in `Rgba8Unorm`**, an `ImageType::D3` image sampled trilinearly.
  32 is the size every tool writes and every engine reads, and it is small
  enough — 128 KB — that it costs nothing to keep resident per camera. Not an
  sRGB view: the texels _are_ the graded output and decoding them once more
  would apply the transfer function twice.

  It is the **engine's first 3D image**. `ImageType::D3` and `ImageViewType::D3`
  are already on the seam and answered by every backend; what has no 3D form is
  `crcbl_render::transient`'s pool, whose `TransientImageDesc` has no depth
  field ([51-volumetrics.md](51-volumetrics.md) records that). A LUT is uploaded
  once and never transient, so it is created directly and does not wait on that.

- **Authored as `.cube`, cooked at load.** Adobe's `.cube` is what Resolve,
  Photoshop, Lightroom and every grading tool export, so an artist's file drops
  in with no conversion — the same argument
  [06-assets-scenes.md](06-assets-scenes.md) makes for glTF. The text form is
  parsed into the image at load rather than committed as a cooked artifact,
  because the parse is a float triple per line and 32,768 lines is not a build
  step worth having.

- **Identity when absent, exactly — by skipping the lookup, not by neutralising
  it.** A camera with no grade sets a flag in the tonemap's block and the sample
  is not taken. Neither alternative is actually the identity: a 1³ table returns
  its one texel for every input, and a 32³ ramp still costs a trilinear fetch
  and re-quantises the result to eight bits, so a frame that asked for no grade
  would not be the frame drawn before grading existed. That is the same
  additive-zero discipline the upscale and the adaptation blend already keep,
  and it is what says no golden in this tree moves when the field lands.

- **It is a `CameraStack` field**, not a `RenderEffects` bit: a grade is a
  parameter with a value, and the stack rung above records exactly that gap —
  every pass type but the antialiasing slot is field-less because no per-pass
  parameter has a serialized form. Grading is the first pass to need one, so it
  is where that shape gets decided: a path to a `.cube`, resolved against the
  stack file's own directory.

- **Where it sits: after the tonemap, before the antialiasing resolve.** After
  the tonemap because that is what "post-tonemap LUT" means; before the resolve
  because the resolve's whole job is to smooth the picture that will be
  displayed, and grading afterwards would move colours across edges the resolve
  had already reconciled. The pass-fusion decision below says why it rides
  **inside** the tonemap pass rather than adding a fullscreen round trip: the
  tonemap writes the resolve's luma into its own alpha channel, so the grade has
  to be applied before that write or the resolve reads the luma of an ungraded
  frame.

**Depth of field and lens artefacts stay missing, and they follow this rung.**
Both are display-referred effects with parameters, so each needs the same
serialized-parameter shape the LUT field settles, and neither has a reason to be
built before there is a curve and a grade to defocus and to flare.

### Pass fusion, taken 2026-08-30: two round trips the stack does not need

Each pass in the stack is a fullscreen write and a fullscreen read, and on the
software and browser tiers that traffic is the pass. Two of them fold:

- **Tonemap and the AA edge detect are one pass.** FXAA reads tonemapped luma
  and CMAA2's edge pass reads the same, so the tonemap writes its luma into the
  alpha channel it already owns and the resolve reads it back without a second
  image — the resolve's first stage is the tonemap's last. One fullscreen round
  trip gone from every frame the engine draws.
- **Auto-exposure's histogram reads the bloom chain's first downsample**, not
  the full scene target: a quarter-area image with the same distribution to
  within the bins' width, and a chain that already exists on every frame bloom
  is on for. On a frame without bloom the histogram builds its own quarter
  level, which is still a quarter of the reads it does today.

Both are checked by the goldens they must not move — the exposure e2e's
percentile and the AA observer's counts — and priced on
[40-profiling.md](40-profiling.md)'s baseline.

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

**All four layers have a source in the tree** — the camera one since 2026-09-06,
and it is this document's own rung above. `crcbl_render::effects` is the
resolution point: `RenderEffects` is the effect set, `EffectRequest` carries the
three requested layers, and `EffectRequest::resolve` applies the whole order in
one place. `ForwardRenderer::begin_frame` resolves once per frame and freezes
the answer, so the half of a frame that parametrises the shadow culls and the
half that dispatches them cannot disagree.

- **Programmatic** is wired: `ForwardRenderer::set_effect_request`, and
  `apps/lantern`'s `--no-shadows` / `--no-ao` / `--no-reflections` drive it.
  There is no `--no-bloom`, and there is nothing for one to turn off: bloom is
  **one of the five effects** `RenderEffects::DEFAULT_STACK` subtracts, so a
  view that has declared no render stack — which is every view in this workspace
  but the `Scene::Bloom` fixture — is not drawing it to begin with. The reason
  is on that constant: the other three approximate light transport present in
  the scene, and a camera given no stack has been given no lens.
- **Device removes nothing**, and it is not wired to anything: `device_effects`
  is initialised to `RenderEffects::all()` and never assigned, so no
  `DeviceCaps` value reaches it. That is a fact about these three effects rather
  than an unfinished clamp — the AO rules in
  [rendering notes](../notes/rendering.md) say it of the occlusion pair in as
  many words, the reflection pair's module says it of itself, and a device too
  small for the shadow atlas fails to build the renderer rather than degrading
  past it. The first real rule arrives with the ray-traced variants, which
  `LightingPath` selects.
- **Camera stack** is wired, and it is a file: `crcbl_render::stack` is the
  reader and the deterministic writer, `CameraStack::compile` is what makes
  `RenderEffects` the compiled form of that file, and
  `ForwardRenderer::set_camera_stack` overlays it under the `[engine.video]`
  layer in the order `39-capabilities.md` fixes. It was foundations block (b)
  (2026-08-30), landed 2026-09-06, and the `ron` crate arrived with it — this
  workspace's first RON reader of any kind. A sample's tuning and an A/B
  measurement on `40-profiling.md`'s baseline are each a file now:
  `apps/lantern` reads `assets/camera.ron` at open and `--stack` points it at
  another. **What a stack cannot yet say is a pass's parameters**, which is the
  half of topic 18's sentence still outstanding — see the rung above, and
  `docs/backlog.md` for which passes are waiting on a serialized form. A quality
  tier is still the `[engine.video]` layer's rather than this one's.
- **`[engine.video]`** is wired: `GpuContext` reads the player's settings file
  while it opens — `SettingsSource::Platform` by default, so every sample and
  the `crcbl new` scaffold get it without asking — and
  `GpuContext::effect_request` hands the layer to a renderer built on that
  context. `crcbl::settings`' `VIDEO_KEYS` is the one place an **effect** key is
  spelled, and a key that is absent clamps nothing, because this layer may only
  remove. The `[engine.video]` keys that are not effect bits — `render_scale`,
  `frame_limit`, `antialiasing` — are spelled beside it, each with its own
  constant and its own reason.
