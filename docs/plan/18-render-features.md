# Topic 18 — Render Features: Shadows + Post-Processing

The visual-credibility layer on top of the stage 2/3 renderer: shadow maps and
the post-processing stack (HDR, tonemapping, AA, bloom). Shadows and the
HDR/tonemap/FXAA core are **MVP** (they land inside the renderer phases); bloom
follows at P10; TAA is post-MVP.

## Shadows (MVP — lands with P7)

- **Sun: cascaded shadow maps** (CSM), 2–3 cascades, stable (texel-snapped)
  projections, PCF filtering (3×3 MVP). One directional light with shadows is
  the MVP contract — it's what makes 3D scenes read as 3D.
- **GPU-driven all the way**: shadow pass reuses the stage 3 compute culling
  (one cull dispatch per cascade against the same instance/geometry pools,
  indirect draws into depth-only pipelines). No CPU re-traversal per cascade —
  the shadow cost scales like the main pass, by design.
- Render graph: cascades = depth targets owned by the graph; barriers/layout
  automatic like every pass. Debug: cascade-split visualization overlay +
  shadow-map inspector panel (topic 7 debug tools).
- Skinned casters (topic 17) come free via the skinned-output pool region.
- **Spot-light shadows** (single map) when towers wants them (tower projectiles
  at night — optional polish); **point-light** (cube maps) post-MVP;
  static-geometry caching (cached cascades / shadow atlases) post-MVP when a
  sample's perf numbers demand it.
- Tier B note: identical approach — depth pass + compacted draws; nothing
  bindless-dependent in the shadow path.

## Post-processing stack

Pipeline order (all at internal render resolution, before the topic 15
render-scale upscale; UI composites after, at native resolution):

```
scene (HDR RGBA16F) → bloom (down/upsample chain) → exposure + tonemap → FXAA → [upscale] → UI
```

- **HDR (MVP, lands with P7)**: scene renders to RGBA16F; lighting in linear HDR
  from the start (retrofitting HDR is repainting every material — do it the
  moment real lighting exists). Fixed exposure MVP; auto-exposure (histogram,
  GPU reduce) later.
- **Tonemap (MVP)**: filmic/ACES-fitted curve + sRGB encode. One combined
  fullscreen pass with exposure.
- **AA (MVP)**: **FXAA** — cheap, single pass, no history. **TAA post-MVP**
  (needs motion vectors in the G-pass + history management + the ghosting fight;
  motion vectors slot into the instance path when TAA lands — the instance
  format reserves the prev-transform slot **now** so TAA is additive later).
  MSAA rejected (fights deferred-ish/HDR pipelines and Tier B; FXAA→TAA is the
  path).
- **Bloom (P10)**: physically-plausible threshold-free downsample chain (Karis
  average), 5–6 mips, tent upsample, additive with scalar. Cheap, huge
  perceived-quality win — timed with the UI/debug polish phase so the profiler
  HUD can show its cost honestly.
- Stack is data-driven per camera (RON: which passes, parameters) —
  games/samples tune without engine edits; settings UI (topic 14 P10) exposes
  quality toggles.

## Interactions (kept honest)

- Render-scale upscale (topic 15) happens **after** tonemap+AA: post chain costs
  scale with internal res (the whole point of render scale).
- UI renders after upscale at native res (crisp text regardless of 3D scale) —
  this ordering is the reason the UI pass was kept separate in stage 7.
- Debug overlays (debug draw, gizmos) render pre-tonemap in HDR (they're in the
  world) except UI-space panels.
- Golden-image tests (topic 12): shadows and each post pass get dedicated golden
  frames; tonemap changes are the classic "everything shifted" diff — the
  `--bless` flow exists for exactly this.

## Delivery

| Slice                                                       | Phase     |
| ----------------------------------------------------------- | --------- |
| HDR target + exposure/tonemap pass + FXAA                   | P7        |
| Sun CSM (culling-integrated, PCF), cascade debug overlay    | P7        |
| Bloom chain                                                 | P10       |
| Spot shadows (if towers polish wants)                       | S6 window |
| Auto-exposure, TAA (motion vectors), point shadows, atlases | post-MVP  |

Sample impact: horde (S3) onward renders shadowed + tonemapped; orbit's planet
terminator and towers' map lighting are the showcase beneficiaries; exit
criteria of those samples inherit "shadows on, stack on" implicitly via the
phase gates.

## Risks

- **CSM artifact whack-a-mole** (peter-panning, acne, cascade seams): budget it;
  stable snapping + slope-scaled bias + debug overlay from day one; artifacts
  are visible in golden frames.
- **Post-stack perf on Tier B/wasm**: each pass is simple, but measure — the
  horde web demo budget (S3) includes the stack.
- **TAA later ≠ never**: prev-transform slot reserved now is the cheap
  insurance; everything else about TAA stays post-MVP.
