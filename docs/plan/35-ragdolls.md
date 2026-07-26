# Topic 35 — Ragdolls

Articulated rigid-body death and impact physics: skeleton bones become
constrained bodies, the killing impulse throws them, and the result feeds back
into the render pose. First-class for modern shooters — a corpse that falls
wrong reads as broken, and in a looting game the corpse is also a container that
must land in the _same place_ for everyone.

**Depends on** the contact solver (36, L2) for bodies-vs-world and L3 joints for
the articulation — ragdolls are the flagship consumer of both.

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
