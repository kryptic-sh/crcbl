# Topic 35 — Ragdolls

Articulated rigid-body death and impact physics: skeleton bones become
constrained bodies, the killing impulse throws them, and the result feeds back
into the render pose. First-class for modern shooters — a corpse that falls
wrong reads as broken, and in a looting game the corpse is also a container that
must land in the _same place_ for everyone.

**Depends on** the contact solver (36, L2) for bodies-vs-world and L3 joints for
the articulation — ragdolls are the flagship consumer of both.

**Built in `crcbl-ragdoll` (`crcbl::ragdoll`), with the editor preview in
`apps/editor`.** The authored rig (`RagdollDesc`) and its fitted fallback
(`RagdollRig::generate`), the death handoff from the last animated pose
(`Ragdoll::spawn`, `strike_with`), skinning from the bodies (`pose_into`), the
settled pose's wire form and the client's soft pull (`SettledPose`, `settle_to`,
`pull_towards`, and inside the solver `tether`), budgets and forced settling
(`SettleBudget`, `force_settle`), substep LOD (`set_substeps`) and the rise
point (`rise_point`) exist. What is left, and why, is in `docs/backlog.md` under
"HIGH PRIORITY: EW engine requirements"; the sections below are the design those
pieces follow.

## The split: server settles, client performs

The naive choices are both wrong for this engine. Client-only ragdolls desync
corpse positions (fatal when corpses are lootable and block sightlines). Full
server-authoritative detail simulation is expensive and mostly invisible. So:

| Layer      | Simulates                                                                                               | Purpose                                                                               |
| ---------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| **Server** | **Simplified ragdoll** — few bodies (torso/hips + limbs as coarse capsules), same solver, deterministic | Gameplay truth: where the corpse settles, what it blocks, what you can loot and shoot |
| **Client** | **Full-detail ragdoll** — all bone bodies, self-collision, seeded from the identical death impulse      | Presentation: the visible flail and fall                                              |

- On settle, the server broadcasts a compact **final pose snapshot** (bone
  transforms, quantized); clients converge their detail ragdoll onto it over a
  short blend. Everyone sees the same corpse in the same place in the same pose
  — the thing that matters — without replicating a full skeleton every tick.
- In-flight divergence is bounded, not eliminated: the client ragdoll is softly
  constrained toward the server's simplified body positions (a gentle positional
  pull), so the two never disagree by more than a body width even before
  settling.
- Determinism: the _server_ ragdoll is sim state and rides the tick hash (fixed
  iteration/ordering from 36). The client's detail pass is presentation and free
  to vary — same rule as pose evaluation (17).

## Death handoff (anim → ragdoll)

- Bodies initialize at the **last animated pose** with velocities derived from
  the pose delta (a running character's corpse keeps running for a step), plus
  the **killing impulse straight from `KineticContact`** (28) — direction,
  magnitude, and _hit location_ come from the damage event, so a headshot snaps
  the head and a leg hit buckles the leg. No separate "death force" system.
- Hit reactions while alive are **additive animation**, not ragdoll (17) —
  cheaper, more art-directable, and doesn't fight the character controller.
  Powered/partial ragdoll (blend a live character's upper body into physics) is
  a later slice, explicitly not MVP-of-feature.
- Ragdoll → get-up (stumble and recover) needs pose matching back into the state
  machine; deferred, with the seam noted so it isn't precluded.

## Setup: authored first, auto fallback

Same philosophy as LOD (25):

1. **Authored ragdoll asset** per skeleton: bone→body mapping, capsule/box
   shapes, joint types and limits, collision groups, mass distribution.
   Editor-editable with live preview (drop the ragdoll, watch it fall).
2. **Auto-generated fallback**: capsules fitted from bone lengths/radii, joint
   types inferred by bone naming/hierarchy, sane default limits — good enough to
   test with immediately, hand-tuned when it matters.

Joint types (L3): **hinge** (knee, elbow), **swing-twist cone** (shoulder, hip,
neck), **fixed** (welded segments). Limits are per-joint data.

**Self-collision**: adjacent bodies exempt (they always overlap at joints),
non-adjacent enabled so limbs don't pass through the torso. A quality knob —
disabling it is the first thing to drop on low settings.

## Anatomical joint limits: hard stop, soft zone, neutral pose (2026-11-01)

Asked by the owner so ragdolls move like bodies: a knee cannot bend past its
fold or backwards, and every joint meets resistance before its stop and drifts
towards a neutral pose. Where it stands today: `crcbl-phys`'s `RevoluteJoint`
has hard limits and a spring to `target_angle`; `SphericalJoint` has a
**circular** cone, a twist range and a friction motor, and no spring;
`RagdollRig::generate` makes every joint the same ball (its docs: "an elbow is a
ball here, and a hinge only in an authored rig"), so a generated knee bends
sideways and backwards as far as forwards.

Three layers, built in this order:

1. **Physics (`crcbl-phys`), joint capability.**
   - **Hard limits**, rigid as now: a hinge's `[lower, upper]`; a ball's swing
     as an **ellipse** (two half-angles about the frame's x and y, so a shoulder
     can swing further forward than sideways), the frame itself carrying any
     asymmetry; twist `[lower, upper]`.
   - **The owner's resistance rules** (2026-11-01), which model muscle: the
     joint cannot pass its hard limit; past its soft limit it meets resistance,
     and the closer it gets to the hard limit the more it meets; at or near its
     neutral position it meets none. So per axis a joint has a **neutral**
     position, a free range around it bounded by **soft** limits, and **hard**
     limits beyond those; the bounds are asymmetric offsets from neutral (a
     knee's neutral is slightly bent). Between soft and hard the restoring
     torque rises progressively with depth (stiffness ramping from zero at the
     soft bound), in the solver's soft-step form so it is stable and gains no
     energy. Off by default; zero cost when off.
   - **A neutral spring** pulling everywhere is not the default — it would
     resist near neutral. It stays optional (a hinge already has
     `target_angle`), alongside the existing friction motor.
   - The industry forms to follow: Jolt's `SwingTwistConstraint` (elliptical
     swing, twist, motors) and PhysX's D6 soft limits (a limit with stiffness
     and damping); Box2D v3's soft-step springs, which the solver is already
     built on. No energy gain, determinism and the cross-target hash pins apply
     as everywhere in the solver.
2. **Ragdoll description (`crcbl-ragdoll`).** `JointKindDesc::Hinge` and `Ball`
   gain the soft bounds, spring settings and a neutral angle or orientation;
   `Ball` gains the two swing half-angles in place of one cone (the old `cone`
   reads as both). Authored rigs set everything per joint.
3. **Anatomical defaults in `RagdollRig::generate`.** An opt-in profile on
   `GenerateSettings` that infers the joint type and ranges from humanoid bone
   names (Mixamo and Unreal naming, plus EW's skeleton) and the hierarchy: knees
   and elbows hinges about the bone's bend axis, hips and shoulders elliptical
   balls, spine, neck and wrists small balls, fingers hinges. Bones it cannot
   name keep today's ball.

**Neutral pose for humanoids: NASA's neutral body posture** (owner's call,
2026-11-01). The posture a relaxed body takes when no muscle works against
gravity is exactly "the position where no resistance is met", so it is the
humanoid profile's neutral. Source: NASA/SP-2010-3407 Rev 1, _Human Integration
Design Handbook_, figure 4.3-2 "Neutral Body Posture in 0g" (page 59), the
current form of NASA-STD-3000's figure built from Skylab measurements of 12
crew. Read off the figure (mean ± spread):

| Joint                               | Figure value | As joint flexion for the profile     |
| ----------------------------------- | ------------ | ------------------------------------ |
| Head/neck, tilt from body vertical  | 24° (±5°)    | 24° forward                          |
| Shoulder, upper arm from torso line | 36° (±19°)   | 36° flexion                          |
| Elbow, included angle               | 122° (±24°)  | 58° flexion                          |
| Hip, trunk-to-thigh included angle  | 128° (±7°)   | 52° flexion                          |
| Knee, thigh-to-calf included angle  | 133° (±8°)   | 47° flexion                          |
| Ankle, foot to the torso line       | 111° (±6°)   | about 21° plantar flexion (see note) |
| Shoulder, front view                | 49° (±20°)   | read as abduction; to confirm        |
| Arm, front view (second arc)        | 39° (±11°)   | not yet attributed; to confirm       |
| Hip, front view                     | 12° (±6°)    | 12° abduction                        |
| Knee/leg, front view                | 200° (±10°)  | not yet attributed; to confirm       |

Notes, so the numbers are used honestly: the foot angle is to "a line through
the torso" (the handbook's own words), so its ankle value depends on the hip and
knee angles and is converted per rig, not copied; the front-view arcs marked "to
confirm" need the original NASA-STD-3000 section 3.3.4.3 text (not retrieved:
its MSIS site no longer resolves) or a careful re-read of the figure before they
are used. The figure is a mean and the handbook says individual variation is to
be expected; NASA TM-2003-104805 (Mount, Whitmore, Stealey 2003, STS-57) found
larger variation than Skylab suggested and no crew member in exactly the
standard posture. So the profile treats the posture as the centre of each
joint's free range, not a point: the ± spread (at least) sits inside the
no-resistance zone, and resistance begins only at the soft limit beyond it. Soft
and hard limits come from anatomical range-of-motion data (a separate source,
owed: e.g. the AAOS normal ranges), not from this figure. NBP is a 0g posture; a
ragdoll under gravity still falls and folds — the neutral only says where its
joints stop being pushed.

Coordination: EW is the consumer and already carries its own anatomy rig on a
branch; its per-joint ranges are the data the defaults are checked against, and
it adopts the API once layer 2 ships. EW's answer (2026-11-01): its rig is
`RagdollRig::generate` with `GenerateSettings::DEFAULT` (every joint a 0.7 rad
ball, twist ±0.5), knees and elbows fold backwards, fingers and toes tremble;
its 55 bone names mix case (`UpperArm_L`, `lowerarm_l`, `Thigh_L`, `calf_l`), so
names match case-insensitively. It asks for the profile to mark joints **"no
body, rigid with parent"** (`ik_*` helpers, `eyes`/`eyebrows`, optionally
fingers and toes) in place of its skeleton-copy trick. Limit-hit events with an
impulse (for impact sounds) and a drive towards an animation pose are wanted
later, not now; breakable limits are not wanted. Done when a generated
humanoid's knees and elbows never bend backwards or sideways past tolerance
under scripted falls, a joint inside its soft range meets no resistance, and the
ragdoll still sleeps (the creep fix's tests).

## Corpses are gameplay objects

- **Shootable**: once ragdolled, hitboxes follow the ragdoll bodies. The
  server's simplified bodies are what the hitbox history ring (26) records, so
  **lag-compensated hits on corpses work** with no special path.
- **Lootable**: the corpse entity carries the dead player's containers (34); its
  settled position is server truth, so the loot is where everyone sees it.
- **Blocks and occludes**: corpses are ordinary colliders — they stop bullets by
  their material (28), get carved by decals (33), and count for audio occlusion.
  Whether they block movement is a game knob.
- **Persistence**: corpses sleep (36's island sleeping) so a match with 40
  bodies on the ground costs nothing; lifetime/despawn is game policy.

## Budgets

- Cap on concurrently _active_ (unsettled) ragdolls; excess deaths settle
  instantly to a canned pose rather than dropping the feature.
- Distance LOD: far ragdolls simulate client-side at reduced substeps/body count
  or skip straight to the server settle pose.
- Settle timeout: a ragdoll that hasn't slept by N seconds is force-settled (the
  anti-jitter-forever guard).

## Debug + testing

- Debug draw: bodies, joints with limit cones, contacts, sleep state,
  server-simplified vs client-detail overlay (the divergence view).
- Properties: **no energy gain** (total kinetic energy never increases without
  an applied impulse — the anti-explosion invariant); joint limits never
  exceeded beyond tolerance; settle time bounded; server ragdoll determinism
  hash stable across thread counts (21's killer test).
- Golden frames for settled poses from scripted deaths (fixed impulses).
- e2e: scripted kill → settled corpse position identical on two clients and in
  the replay.

## Delivery (wave 2 tail / FPS-era, breach drives it)

1. Ragdoll asset format + auto-generation + editor preview.
2. Server simplified ragdoll + settle-pose replication + hitbox handoff.
3. Client detail ragdoll, seeded impulse, soft convergence, blend on settle.
4. Self-collision, budgets, distance LOD, force-settle guard.
5. Corpse-as-container/collider integration (34/28/33).
6. Powered ragdoll + get-up (later, if a sample demands it).

## Risks

- **The classic ragdoll explosion**: mitigated by the solver's design (36 — warm
  starting, substepping, velocity clamps) and caught by the no-energy-gain
  property rather than by watching bodies fly.
- **Two ragdolls, one truth**: divergence between server-simplified and
  client-detail is the design's main tension; bounded by soft convergence and
  made visible by the overlay view — measured, not hoped.
- **Uncanny deaths**: physics-only deaths look floppy. Mitigation is art
  direction (short death-anim into ragdoll, tuned joint stiffness), not more
  simulation.
- **Scope**: powered ragdolls and get-up are their own project; the line is
  drawn at "die convincingly, land consistently".
