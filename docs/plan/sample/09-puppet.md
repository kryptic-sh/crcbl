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

**Exempt from sample rule 11** (`.crpix` art through the sprite pass): the
subject is a 3D character on a 3D map, and its overlay is the readout a reviewer
checks the picture against — pixel art in front of it would be showing the wrong
system. `apps/puppet` already takes this exemption and says so in its module
header; recording it here is rule 11's own requirement that a sample claiming it
says why in its own doc. Rules 4 and 12 apply as everywhere else.

## Milestones

1. Static character + controller + camera on the map (pre-anim: proves map
   - controller path with shadows already on).
2. Clips playing, state machine + blends, GPU skinning.
3. Root motion + events (footstep audio) + socket prop.
4. Device-swap showcase + rebind UI + glyph hints.
5. Pages demo + golden frames (posed characters per theme of topic 18 passes).

## Where this stands

**Milestone 1 and the first half of milestone 2 are built.** A character walks a
blockout under `crcbl::phys::CharacterController`, and a rig is posed by a
locomotion blend the controller's own **measured** speed drives — blended
between clips through `crcbl::anim::blend` rather than switched between them,
and drawn through the engine's skinning dispatch, one range per limb. The sun
turns while it happens, which is the only thing on the map that moves without a
key held: a shadow that never moves is indistinguishable from a dark patch
painted on the ground, and "does it read as grounded" is the eyeball test this
milestone exists for. The walk is a `GameModule` the authoritative server owns
over an `InMemoryTransport` — rule 2, no exemption — with the camera the one
thing deliberately off that side, because it is presentation; the single number
that crosses the wire from it is the yaw the player was looking along.

**Two things differ from the Scope above, and both are decisions rather than
gaps.** The map is authored in `apps/puppet/src/map.rs` rather than as a `.scn/`
dir, because there is no `.scn/` anywhere in this tree and no `apps/editor`. And
the character is a **greybox humanoid authored in code** —
`apps/puppet/src/rig.rs` authors the whole skeleton and its boxes with an idle
stance and a walk cycle, no asset on disk and no glTF parse — rather than the
stock rig this doc names. That keeps the exit criterion "character + clips
imported from a stock glTF with zero manual fixup" **entirely unmet**: it is the
asset-pipeline honesty check, and a rig the sample authored cannot answer it.

**One engine limit is visible in the picture rather than merely absent from the
feature list**: the slopes are **rounded**, because `crcbl-phys` has no oriented
box to make a wedge out of.

**And the controller's camera-agnosticism is now evidence rather than a
comment.** `crcbl::phys::CharacterController` takes a world-space displacement
and stores no orientation at all, so this sample turns a stick into a direction
and turns the body toward where it went. `apps/breach` drives the same
controller from inside the character's head and `apps/shard` from a fixed
isometric rig; `crcbl-phys` gained nothing for any of the three.

Not built — the rest of milestone 2, all of milestones 3 and 4, and half of 5:
the state machine, jump, run, root motion, animation events and the footstep
cues, the socket prop, and the whole device-swap showcase with its rebind UI and
glyph hints. `docs/backlog.md` carries the list. **Milestone 5's other half is
done**: `apps/puppet/src/web.rs` is the `wasm32` front end, `web/demos/puppet/`
is its page and `puppet` is a row in `web/build.sh`'s `DEMOS`. What that
milestone still owes is the golden frames — `apps/puppet` has no `tests/`
directory, so nothing pins a pose.

## Exit criteria

- Locomotion feels correct (blend weights vs speed continuous, no pops at
  transitions — golden-pose tests + eyeball).
- Footstep events land on the tick the foot lands (server-timed, framerate-
  independent — the topic 17 event test, live).
- Full play-through possible on each device class alone (kb/m, pad, touch),
  swapping any time.
- Runs shadowed + tonemapped in a browser at its recorded budget.
- Character + clips imported from a stock glTF with zero manual fixup — the
  asset-pipeline honesty check.
