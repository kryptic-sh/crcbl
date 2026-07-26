# Topic 17 — Skeletal Animation (`crcbl-anim`)

Skeletal animation as an engine system: skeletons and clips come from glTF
(already the locked source format — skins/animations are parsed today and
unused), cooked to engine curves, played back through blend trees and a
data-driven state machine, skinned **on the GPU**. Scheduled post-MVP wave 1;
the [puppet sample](sample/09-puppet.md) is its forcing function and acceptance
test.

## Data pipeline

- **Source**: glTF skins (joint hierarchy, inverse bind matrices) + animation
  channels (TRS curves, sampled). Import extends the stage 6 pipeline — no new
  source format.
- **Cooked**: per-clip compressed curve tracks (fixed-rate resample +
  quantization; curve-fitting later if size demands), skeleton = flat joint
  array (parent indices, bind pose). `crcbl import` grows `--skeletons/--clips`;
  cooked output versioned like all bakes.
- Retargeting: out of scope initially (clips bind to their skeleton); the cooked
  format keeps joint names so retargeting can land later.

## Runtime model (server/client split, consistent with everything else)

- **Server**: animation _logic_ only — state machine ticks, transition
  decisions, normalized clip time, root-motion extraction (velocity applied to
  the physics body), animation events (footstep at t=0.3 → gameplay/audio
  event). No pose math on the server; state is small POD (replicates + saves
  like any component).
- **Client**: full pose evaluation from the replicated anim state — sample
  curves → blend → joint palette. Interpolates between server states exactly
  like transforms.
- Determinism: server anim state is part of the tick hash; pose math is
  client-side presentation and free to vary.

## Evaluation stack (client)

1. **Clip sampling**: time → local joint TRS per track.
2. **Blending**: linear blend nodes (1D — e.g. idle↔walk↔run by speed; 2D
   directional later), additive layers (aim offsets), per-bone masks (upper-body
   shoot while lower-body runs).
3. **State machine**: data-driven (RON asset, hot-reloadable): states = blend
   trees, transitions = condition exprs over action/params + exit time,
   crossfade durations. Authored by hand in MVP-of-the-feature; editor graph UI
   later.
4. **Post ops**: root-motion strip, sockets/attachments (weapon to hand joint),
   two-bone IK + look-at (first IK; full-body IK not planned).
5. Output: joint palette per instance.

## GPU skinning (round-trip principle applied)

- Compute prepass: joint palettes (SSBO, all animated instances) + bind-pose
  vertex pool → skinned vertices written to a transient region of the geometry
  pool. Renderer consumes them like any static mesh — **vertex pulling and the
  GPU-driven pipeline don't know skinning exists**.
- Culling uses conservative animated AABBs (bind pose inflated by clip bounds,
  computed at cook time).
- Tier B (WebGPU): same compute path — skinning is just a compute shader, no
  bindless needed. Wasm gets animated characters day one.
- Skinned shadow casters: the skinned output region feeds the topic 18 shadow
  passes for free (same pool).

## Debug tools

- Skeleton overlay (joints/bones via debug draw), clip scrubber panel,
  state-machine live view (current state, transition progress, params),
  blend-weight inspector. `crcbl anim dump <clip>` CLI.

## Testing (topic 12)

- Golden poses: sample clip at fixed times → joint palette hash vs blessed
  values (per glTF sample-model suite: Fox, CesiumMan, RiggedFigure).
- Blend math unit tests vs hand-computed two-joint cases.
- State machine property test: scripted param sequences → deterministic
  state/time hash (server side — rides the determinism harness).
- Event timing test: footstep events fire at exact tick regardless of framerate
  (server-side timing proof).

## Delivery (post-MVP wave 1 — see ROADMAP post section)

1. Import/cook (skeletons, clips) + golden-pose tests.
2. Server anim state + state machine + events + root motion.
3. Client sampling/blending + GPU skinning compute pass.
4. Sockets, masks, additive layers, two-bone IK/look-at.
5. **puppet sample** proves the stack (+ shadows + action input together).
6. Editor: state-machine panel (view first, graph editing later).

## Risks

- **Blend-tree scope creep** (the Unity Mecanim tarpit). MVP-of-feature = 1D
  blends, masks, additive, crossfades — what puppet needs. 2D blends when a
  sample demands locomotion strafing; no visual graph editor until hand-authored
  RON hurts.
- **Root motion vs server physics** — root motion drives the char controller
  (topic 5 L0), not transform directly; decided now to avoid the classic desync.
- **Curve compression rabbit hole** — fixed-rate quantized MVP; measure before
  fitting curves.

## Corrections (design review, 2026-07-27)

- **"No pose math on the server" was overstated**: server-side root-motion
  extraction and animation-event timing both require sampling cooked curves.
  Corrected: the cook emits a **server strip** per clip — root track + event
  track + duration only — which the server loads; full curve sets stay
  client-only. The claim becomes "the server samples no _pose_ curves", which is
  true and implementable.
- **TAA motion vectors for skinned meshes need previous-frame skinned
  positions**, not just a previous transform (18's "prev-transform slot makes
  TAA additive" is false for deforming geometry). Corrected: the
  **skinned-output pool region is double-buffered (prev/current ping-pong) from
  day one** — a pool-layout decision that is nearly free now and a
  skinning-pipeline rewrite later.
