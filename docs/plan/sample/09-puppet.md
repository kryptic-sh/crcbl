# Sample 09 — puppet (post-MVP wave 1)

Third-person character playground: one animated character on a small shadowed
map — walk/run/jump with blended locomotion, orbit-follow camera, and live
input-device swapping. The forcing function and acceptance test for skeletal
animation (topic 17), and the joint showcase for shadows/post (topic 18) and the
action input system (topic 19).

## Proves

- **Skeletal animation end-to-end** (topic 17): glTF character imported +
  cooked; server-side state machine (idle/walk/run/jump/fall) driven by action
  params; client blend tree (1D locomotion by speed, crossfades); GPU-skinned;
  animation events (footsteps → spatial audio through the cue grammar — topics
  13+17 composing).
- **Root motion → character controller**: animation-driven movement feeding the
  phys L0 capsule controller (the topic 17 desync-avoidance design,
  demonstrated).
- **Shadows + post stack** (topic 18): sun CSM over the character and props —
  the "does it read as grounded" eyeball test; tonemap/FXAA/bloom on. Cascade
  debug overlay featured in the demo's help.
- **Input agnosticism** (topic 19): keyboard/mouse ↔ gamepad ↔ on-screen touch
  pad swapped live mid-play; UI glyph hints follow last-active device; rebind
  screen included. One character, three device worlds, zero game-code branches.
- Sockets: a held prop attached to the hand joint (attachment proof).

## Scope

- One `.scn/` map (editor-authored): ground, ramps, steps, a few shadow- casting
  props. One character (Khronos Fox/CesiumMan class, or a CC0 rig).
- Locomotion set: idle, walk, run, jump, fall, land. 1D blend + state machine.
  Look-at head IK (stretch within the sample).
- Orbit-follow camera (collision-aware via phys sweep).
- Server-authoritative as always; browser build on the Pages site (GPU skinning
  is Tier-B-clean by design — animated character in wasm is part of the point).

## Non-goals (hard cap)

Combat, NPCs, terrain system, cloth/ragdoll, full-body IK, retargeting, 2D blend
spaces (unless strafing gets added, which it shouldn't — see cap).

## Milestones

1. Static character + controller + camera on the map (pre-anim: proves map
   - controller path with shadows already on).
2. Clips playing, state machine + blends, GPU skinning.
3. Root motion + events (footstep audio) + socket prop.
4. Device-swap showcase + rebind UI + glyph hints.
5. Pages demo + golden frames (posed characters per theme of topic 18 passes).

## Exit criteria

- Locomotion feels correct (blend weights vs speed continuous, no pops at
  transitions — golden-pose tests + eyeball).
- Footstep events land on the tick the foot lands (server-timed, framerate-
  independent — the topic 17 event test, live).
- Full play-through possible on each device class alone (kb/m, pad, touch),
  swapping any time.
- Runs shadowed + tonemapped in browser at the Tier B budget.
- Character + clips imported from a stock glTF with zero manual fixup — the
  asset-pipeline honesty check.
