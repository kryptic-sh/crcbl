# Sample 10 — sparks (post-MVP wave 1)

VFX gallery + live authoring workbench for the particle system (topic 20). Like
hud is to the UI system: the living fixture, the parameter-surface driver, and
the demo. Not a game — the effects are the content.

## Proves

- **GPU-resident pipeline at a glance**: thousands of simultaneous effects, CPU
  flat (profiler row on screen — the pitch, visible).
- **Authoring loop**: select effect → tweak params with live sliders / curve +
  gradient widgets → hot-reload from disk when edited as text → save back
  through the deterministic writer. RON is the source of truth; the workbench is
  sugar.
- **Every render mode**: billboard, velocity-stretched, flipbook, soft,
  ribbon/trail, mesh particles (debris riding the instance path), additive vs
  alpha-sorted, HDR emissive → bloom interplay (topic 18).
- **Budgets behave**: a deliberately hostile effect (max spam) clamps to its
  pool share and the panel shows it — never a frame-rate cliff.
- **Seeded determinism**: freeze/step controls + fixed-seed replay; golden
  frames per stock effect in CI (`crcbl vfx render`).

## Scope

- Gallery page: stock effect library (impact sparks, smoke puff, fire + embers,
  muzzle flash, rain, snow, magic swirl, explosion w/ debris mesh particles,
  rocket trail) — each doubles as a shipped preset games start from.
- Workbench page: param inspector (reusing the effect-editor UI that later
  embeds in the stage 8 editor), spawn-on-click in a small 3D scene with
  shadowed props (depth-collision demo), overdraw heatmap toggle, freeze/step
  time.
- Pages web demo — the flashiest bundle on the site; Tier B budget recorded.

## Non-goals (hard cap)

Node-graph authoring, custom per-particle scripting, gameplay of any kind,
effects requiring engine features that don't exist (no requests smuggled in as
"the gallery needs it" — the workbench exercises what topic 20 ships).

## Milestones

1. Gallery with billboards + curves (topic 20 slice 1 proof).
2. All render modes as slices land; golden frames per effect.
3. Workbench param UI + curve/gradient widgets (feeds back into topic 7 widget
   set).
4. Stock library polish + Pages demo.

## Exit criteria

- Every topic 20 feature has a gallery entry + fixed-seed golden frame in CI
  (the VFX regression surface, same pattern as hud's widget gallery).
- A new effect is authorable start-to-finish in the workbench without touching a
  text editor (and the saved RON diffs cleanly — deterministic writer proof for
  a new asset type).
- Hostile-effect budget test green; overdraw heatmap functional.
- 1k concurrent effect instances at 60 fps native / recorded Tier B budget in
  browser; CPU cost flat vs effect count (profiler capture archived).
