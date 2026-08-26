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
- Pages web demo — the flashiest bundle on the site; browser budget recorded.

## Non-goals (hard cap)

Node-graph authoring, custom per-particle scripting, gameplay of any kind,
effects requiring engine features that don't exist (no requests smuggled in as
"the gallery needs it" — the workbench exercises what topic 20 ships).

**Exempt from sample rule 11** (`.crpix` art through the sprite pass): a
particle's texture comes from topic 20's own authoring path, and routing it
through the sprite pass instead would be demonstrating the wrong system.
`.crpix` is still the right source for a _particle_ sheet where one is
hand-drawn — the format is a PNG baker and does not care who samples the result
— but that is an authoring convenience here, not the rule. Rule 4's debug panel
applies, and a particle workbench without live frame timing is not a workbench.

**Exempt from sample rule 2** (client/server authority), on topic 20's own
grounds rather than on this sample's convenience. `docs/plan/20-particles.md`
opens by saying visual-only VFX are "client + GPU … zero gameplay reads, zero
readbacks", and that "gameplay-relevant particles are not particles — they're
entities". So `apps/sparks` opens no `World` and stands up no
`InMemoryTransport`: an effect here is fire-and-forget presentation, and putting
one behind a wire would be demonstrating the wrong thing.
`apps/sparks/src/show.rs` makes the same argument where the schedule is written.

## Milestones

1. Gallery with billboards + curves (topic 20 slice 1 proof).
2. All render modes as slices land; golden frames per effect.
3. Workbench param UI + curve/gradient widgets (feeds back into topic 7 widget
   set).
4. Stock library polish + Pages demo.

## Where this stands

**Milestone 1 is built, and the two claims it exists for are met.** A simulated
particle becomes an instance and rides the stage 3 GPU-driven pipeline that was
already there — there is no particle shader in this sample and no pass of its
own, which is `docs/plan/20-particles.md`'s "inject transforms into the stage 3
instance path" taken literally. And the **hostile effect is held at its share**:
`apps/sparks/src/effects.rs`'s `spam` is on the page with its refusal counter
beside it, never a frame-rate cliff. The show runs itself — nothing reads a key,
which the browser gate depends on, because a count it watches rise and fall has
to do so without anything reaching the page or the check would be testing the
input path instead of the simulation.

**Colour is quantised, and that is a finding rather than a shortcut.** The
instance path carries a mesh, a material row and a transform, and no
per-instance tint — so colour over lifetime reaches the screen as
`effects::PALETTE_STEPS` baked material rows per effect, each particle drawn
through the row nearest the colour the simulation gave it.
`apps/sparks/src/effects.rs` argues it and `docs/backlog.md` carries what
changing it would cost. It is deliberately not fixed here, because the hard cap
above is that the gallery exercises what topic 20 ships rather than smuggling
engine features in behind it.

**The gallery is two stock effects, not the nine the Scope names** — impact
sparks and a smoke puff, plus the hostile one. Every render mode past plain mesh
particles is unbuilt: no billboards, flipbooks, soft particles, ribbons or
trails, no depth collision, no sorting. So is the whole authoring loop — no RON
effect assets and therefore no hot reload, no workbench, no sliders and no curve
or gradient widgets. And the step **runs on the CPU**, which is the staging
`docs/plan/20-particles.md` asks for and not its destination. `docs/backlog.md`
carries each with what it would take.

**One exit criterion names a tool that does not exist.** There is no `vfx`
subcommand on the `crcbl` CLI — `crates/crcbl-cli/src/args.rs` dispatches `new`,
`run`, `build`, `screenshot`, `replay`, `crpix`, `lod`, `import`, `bench`, `sim`
and `settings`, and nothing else. `apps/sparks` also has no `tests/` directory,
so there are no golden frames per effect either. Both are owed together: the
golden frames are what the subcommand would exist to produce.

## Exit criteria

- Every topic 20 feature has a gallery entry + fixed-seed golden frame in CI
  (the VFX regression surface, same pattern as hud's widget gallery).
- A new effect is authorable start-to-finish in the workbench without touching a
  text editor (and the saved RON diffs cleanly — deterministic writer proof for
  a new asset type).
- Hostile-effect budget test green; overdraw heatmap functional.
- 1k concurrent effect instances at 60 fps native / recorded browser budget in
  browser; CPU cost flat vs effect count (profiler capture archived).
