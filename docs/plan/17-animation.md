# Topic 17 — Skeletal Animation (`crcbl-anim`)

Skeletal animation as an engine system: skeletons and clips come from glTF
(already the locked source format), cooked to engine curves, played back through
blend trees and a data-driven state machine, skinned **on the GPU**. Scheduled
post-MVP wave 1; the [puppet sample](sample/09-puppet.md) is its forcing
function and acceptance test.

## Data pipeline

- **Source**: glTF skins (joint hierarchy, inverse bind matrices) + animation
  channels (TRS curves, sampled). Import extends the stage 6 pipeline — no new
  source format. **The extraction has landed**:
  `crates/crcbl-scene/src/gltf_import.rs` reads the document's `skins` and
  `animations` arrays into `GltfSkin`/`GltfClip`/`GltfChannel`, and
  `GltfPrimitive::joints`/`weights` carry the per-vertex binding. Reading is not
  playback and that file says so — nothing there poses a skeleton. **The arrow
  that is still missing is the one out of it**: nothing converts a `GltfSkin` +
  `GltfClip` into `crcbl-anim`'s `Skeleton` and `Clip`, so the only rig any
  sample plays is `apps/puppet/src/rig.rs`, authored in code with no asset on
  disk. That conversion is index bookkeeping belonging to whoever holds both
  crates, and `crcbl-anim` deliberately depends on neither `crcbl-scene` nor
  `gltf` so a browser build that only plays cooked clips never links a parser.
- **Cooked**: per-clip compressed curve tracks (fixed-rate resample +
  quantization; curve-fitting later if size demands), skeleton = flat joint
  array (parent indices, bind pose). `crcbl import` grows `--skeletons/--clips`;
  cooked output versioned like all bakes. **None of the cook exists** — there is
  no cooked clip format and `crcbl import` has neither flag; `Skeleton` and
  `Clip` are runtime types a caller constructs directly.
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

1. ~~**Clip sampling**: time → local joint TRS per track.~~ Shipped:
   `Clip::sample_into` fills a `Pose`, allocating nothing.
2. **Blending**: linear blend nodes (1D — e.g. idle↔walk↔run by speed; 2D
   directional later), additive layers (aim offsets), per-bone masks (upper-body
   shoot while lower-body runs). **The 1D half shipped and nothing else did**:
   `blend_into` and `BlendSpace1d` are in the crate and `apps/puppet` mixes
   idle↔walk↔run by measured speed through them, while additive layers, per-bone
   masks and a 2D space have no types at all.
3. **State machine**: data-driven (RON asset, hot-reloadable): states = blend
   trees, transitions = condition exprs over action/params + exit time,
   crossfade durations. Authored by hand in MVP-of-the-feature; editor graph UI
   later. Unbuilt.
4. **Post ops**: root-motion strip, sockets/attachments (weapon to hand joint),
   two-bone IK + look-at (first IK; full-body IK not planned). Unbuilt.
5. ~~Output: joint palette per instance.~~ Shipped: `Palette::compute`.

## GPU skinning (round-trip principle applied)

**Built**, as `crates/crcbl-render/src/skinning.rs` over
`crates/crcbl-shaders/shaders/skinning.slang`; `apps/puppet` is its consumer,
natively and in the browser. What the design promised and what it delivered:

- Compute prepass: joint palettes + bind-pose vertex pool → skinned vertices
  written to a transient region of the geometry pool. Renderer consumes them
  like any static mesh — **vertex pulling and the GPU-driven pipeline don't know
  skinning exists**. Held: one branch exists downstream, in the raster stages,
  on `GpuInstance::BASE_VERTEX_OVERRIDE`; cull, draw generation and the shadow
  passes gained nothing, because a skinned instance goes on naming its source
  mesh.
- Culling uses conservative animated AABBs (bind pose inflated by clip bounds,
  computed at cook time). **Not done** — there is no cook to compute them at, so
  a skinned instance is culled against its undeformed box.
- The browser gets the same compute path — skinning is just a compute shader, no
  bindless needed — so wasm gets animated characters day one. Skinning is
  independent of every path selector in
  [39-capabilities.md](39-capabilities.md). Held: `puppet` publishes as a
  browser demo.
- Skinned shadow casters: the skinned output region feeds the topic 18 shadow
  passes for free (same pool). Held.
- **One dispatch per animated range**, not one over a table of ranges. The
  GPU-driven form needs a range table the shader can index — a second layout to
  pin against `slangc` — and is deliberately deferred.

## Debug tools

- Skeleton overlay (joints/bones via debug draw), clip scrubber panel,
  state-machine live view (current state, transition progress, params),
  blend-weight inspector. `crcbl anim dump <clip>` CLI.

## Testing (topic 12)

- Golden poses: sample clip at fixed times → joint palette hash vs blessed
  values (per glTF sample-model suite: Fox, CesiumMan, RiggedFigure). Unbuilt,
  and blocked on the cook: nothing in the tree turns a `.glb` rig into a
  `Skeleton`.
- Blend math unit tests vs hand-computed two-joint cases.
- State machine property test: scripted param sequences → deterministic
  state/time hash (server side — rides the determinism harness).
- Event timing test: footstep events fire at exact tick regardless of framerate
  (server-side timing proof).

## Delivery (post-MVP wave 1 — see ROADMAP post section)

Step 3 landed, and step 5 in part — **out of order**, because step 1's cook does
not exist and `apps/puppet` therefore plays a rig authored in Rust. That is why
the sample proves the evaluation stack without proving the pipeline.

1. Import/cook (skeletons, clips) + golden-pose tests. Import only; no cook. The
   tests in `crates/crcbl-anim/tests/` check hand-composed chains and the glTF
   interpolation modes, not blessed poses from the sample-model suite.
2. Server anim state + state machine + events + root motion.
3. ~~Client sampling/blending + GPU skinning compute pass.~~ Shipped.
4. Sockets, masks, additive layers, two-bone IK/look-at.
5. **puppet sample** proves the stack (+ shadows + action input together). Built
   and published through its milestone 2 — clip, blend, palette, skinning
   dispatch and shadows. The stock-glTF-character honesty check is its milestone
   5 and needs step 1.
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
  skinning-pipeline rewrite later. **Followed**: `SkinnedRegion` reserves two
  runs and `Skinning::begin_frame` alternates. Nothing reads the other half —
  there is no TAA pass — so the prev region is memory bought now and spent
  later, deliberately.
