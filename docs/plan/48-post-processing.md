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

**`[upscale]` has no implementation on either side of the seam, verified
2026-08-27.** [15-windowing.md](15-windowing.md) defines borderless as an
internal render target upscale-blitted to the native surface, and
`ShellCaps::HW_UPSCALE` reports what a window system will do for free — but
`crcbl-render` has no upscale pass, no render-scale knob and no internal target
whose extent differs from the swapchain's. So every stage of this chain runs at
native resolution today, and the ordering is a contract for a pass that does not
exist rather than a description of a frame. Whoever builds it inherits the two
interactions below unchanged.

- **HDR (MVP, lands with P7)**: scene renders to RGBA16F; lighting in linear HDR
  from the start (retrofitting HDR is repainting every material — do it the
  moment real lighting exists). Fixed exposure MVP; auto-exposure (histogram,
  GPU reduce) later.
- **Tonemap (MVP)**: filmic/ACES-fitted curve + sRGB encode. One combined
  fullscreen pass with exposure. **Built 2026-08-27, and the clamp stayed the
  default.** `tonemap.slang` carries two operators behind a `uint curve` lane of
  its block — exposure-and-clamp, and Stephen Hill's fit of the ACES RRT and ODT
  — and `crcbl_render::ForwardRenderer::set_tonemap_curve` is what a view asks
  with. Fixed exposure is a runtime uniform; auto-exposure (histogram, GPU
  reduce) is still later.

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

- **AA (MVP)**: **FXAA**, then SMAA 1x, with TAA post-MVP and MSAA priced rather
  than rejected — the whole ladder, what each rung costs in this tree and what
  is refused are [49-antialiasing.md](49-antialiasing.md). **Built 2026-08-27**:
  `fxaa.slang` and `crcbl_render::fxaa` are one fullscreen resolve after the
  tonemap, `RenderEffects::ANTIALIASING` is the bit, and it is in
  `DEFAULT_STACK` — so every frame this engine draws is resolved. SMAA 1x is the
  next rung and is not built.

  **The prev-transform slot is not reserved**, whatever this row claimed until
  2026-08-27. `crcbl_shaders::mesh::GpuInstance` carries `transform`, `mesh`,
  `material`, `sector`, `flags` and `base_vertex` and nothing else, so the
  instance format is a widening TAA still has to pay for. The cheap insurance
  was never taken; the reason to take it — that `INSTANCE_STRIDE` is cheap to
  extend now and expensive once §3.3's shaders index past it — is the one
  `GpuInstance::sector` is already in the record on.

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
