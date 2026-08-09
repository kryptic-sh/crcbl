# Topic 20 — Particles / VFX (`crcbl-vfx`)

GPU-resident particle system: simulated in compute, rendered indirect, never
read back — the engine's GPU-bound principle applied to eye candy at scale.
Data-driven effect assets (RON, hot-reloadable), authored through a live
workbench ([sparks sample](sample/10-sparks.md)) and later an editor panel.
Post-MVP wave 1, alongside animation; towers' polish pass is the first in-game
consumer.

## The split (settled in the physics doc, applied here)

- **Visual-only VFX = client + GPU.** Spawn → simulate → render entirely on
  device; zero gameplay reads, zero readbacks, effectively unlimited count. This
  is 99% of all effects.
- **Gameplay-relevant "particles"** (a projectile that damages) are not
  particles — they're entities in `crcbl-phys`/ECS like everything else. The VFX
  system may _decorate_ them (trail on a rocket), never _be_ them.
- Effects are triggered by replicated server events (`Explosion at pos`),
  animation events (topic 17 footstep → dust), scene-placed emitters (stage 6/8:
  an emitter is ordinary system data), or UI (screen-space effects).
  Client-side, an effect instance is fire-and-forget.

## Simulation (GPU compute, identical on every path)

- **Global particle pool** (SSBO, structure-of-arrays: position, velocity,
  age/lifetime, size, rotation, color seed, atlas frame, emitter id) +
  per-effect allocation ranges. Freelist compaction in compute; alive-count →
  indirect draw args. No CPU involvement after spawn command.
- **Spawn**: emission compute pass consumes per-emitter spawn requests
  (rate/burst accumulators computed CPU-side per instance — tiny), shapes:
  point, sphere, hemisphere, cone, box, ring, mesh-surface (sampled from the
  geometry pool at cook time).
- **Update**: age, integrate velocity, apply modifiers — the modifier set is a
  fixed menu evaluated from the effect's params (not a per-particle VM):
  gravity, drag, curl-noise turbulence (tileable 3D noise texture), vortex,
  point attractor/repulsor, velocity/size/color-over-lifetime curves, orbit.
- **Collision (cheap, visual)**: depth-buffer collision (sparks bounce off
  visible geometry) — screen-space, costless-ish, wrong off-screen and
  acceptably so. **No physics-BVH queries from the GPU** — that would be a round
  trip; effects needing real collision are entities.
- Curves uploaded as small 1D LUT textures (bake from the RON curve defs);
  randomness = per-particle hash of (seed, index) — stateless, replayable, which
  makes golden-frame testing of particles possible (fixed seed + fixed time step
  = identical frames).

## Rendering

- **Billboards** (camera-facing, velocity-stretched option), **flipbook atlas
  animation** (frame over lifetime, optional motion-vector blending later),
  **soft particles** (depth-fade near geometry), lit (cheap N·L with scene sun)
  or unlit/additive.
- **Ribbons/trails**: compute-generated strips from particle history (rocket
  trails, blade sweeps).
- **Mesh particles**: alive particles inject transforms into the stage 3
  instance path — rocks/debris ride the normal GPU-driven pipeline (culling and
  all) for free.
- Sorting: additive effects skip it; alpha-blended sorted per-system by coarse
  depth-bin key in compute (correct-enough; exact OIT is a non-goal).
- Draw: one indirect draw per blend-mode bucket, not per effect — effect count
  doesn't touch CPU cost (the stage 3 discipline).
- Post interaction: particles render pre-tonemap in HDR (emissive values >1 feed
  topic 18 bloom naturally — glowing embers for free).

## Effect assets (authoring format)

RON (engine-data rule), hot-reloadable, schema'd like everything else:

```ron
(
  name: "impact_sparks",
  emitters: [(
    spawn: Burst(count: 64),
    shape: Cone(angle: 30.0, dir: Normal),   // Normal = from event payload
    lifetime: (0.2, 0.6),                     // min..max, per-particle hash
    velocity: (4.0, 9.0),
    modifiers: [ Gravity(9.8), Drag(1.2), CollideDepth(bounce: 0.4) ],
    over_lifetime: ( size: Curve([(0.0,1.0),(1.0,0.2)]),
                     color: Gradient([(0.0,"#fff4c0"),(1.0,"#ff5a0000")]) ),
    render: Billboard(atlas: "vfx/spark", blend: Additive, soft: true),
    max_particles: 128,
  )],
)
```

- Budgets are part of the asset (`max_particles` per emitter, pool-share per
  effect) — an effect cannot detonate the frame budget; the pool allocator
  clamps and the profiler shows who asked for what.
- Distance scaling: emission rate scales down by camera distance (per-effect
  curve) — effect LOD without a second system.

## Tooling

- **sparks workbench** (sample 10): live effect gallery + param tweaking — the
  authoring loop is "edit RON / drag sliders, watch it, save". Hot reload makes
  the text file the source of truth; the workbench is sugar over it.
- **Editor** (stage 8 growth): effect assets in the asset browser,
  drag-into-scene emitter placement, an inspector panel that reuses the
  workbench's param UI. Curve/gradient editing needs two new UI widgets (curve
  editor, gradient bar) — added to topic 7's demand-driven widget list when this
  lands.
- **Debug** (topic 7): VFX panel — live effects, particle counts vs budgets,
  pool occupancy, per-bucket GPU time (graph pass timestamps already exist);
  freeze/step-time controls for inspecting a frame of simulation.
- **CLI** (topic 11): `crcbl vfx render <effect> --time T -o frame.png`
  (offscreen, fixed seed) — golden frames for effects; `crcbl vfx lint`
  (budget/asset checks).
- **Testing** (topic 12): seeded determinism makes golden frames work; curve-LUT
  bake unit tests; pool allocator property tests (churn, clamp, freelist
  integrity); budget-overflow behavior test (clamps, never corrupts).

## Delivery (post-MVP wave 1)

1. Pool + spawn/update compute + billboards + curves/gradients + RON assets
   - hot reload.
2. sparks workbench (sample 10) — drives the param surface.
3. Flipbooks, soft particles, ribbons, mesh particles, depth collision.
4. Editor placement + inspector panel + curve/gradient widgets.
5. towers/horde/orbit retrofit pass (impacts, deaths, thruster) — the "engine
   looks alive" milestone.

## Risks

- **Modifier creep toward a VM.** The fixed modifier menu is the contract; a
  per-particle scripting VM is explicitly rejected (mods wanting custom behavior
  use mesh particles + module logic, or request a modifier).
- **Sorting/overdraw perf cliffs**: additive-first culture in the default
  assets; overdraw heatmap in the debug panel from day one.
- **Workbench scope**: it's a gallery with sliders, not a node graph. Node-based
  VFX authoring is a someday-maybe, gated on real demand.

## Correction (design review, 2026-07-27)

**Ribbon/trail history storage was unspecified**: trails need per-trail
**circular vertex history** buffers (fixed capacity per trail, allocated from
the same pool budget as particles), written by the update pass and consumed by
strip generation. Capacity is part of the effect asset's budget block, so a long
trail costs a declared amount rather than an unbounded one.
