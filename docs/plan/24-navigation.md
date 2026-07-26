# Topic 24 — Navigation: Navmesh + Pathfinding (`crcbl-nav`)

From-scratch navigation: Recast-lineage navmesh generation from physics
colliders, tiled and sector-aware from day one, A\* + funnel pathfinding, and a
crowd/steering layer that fits the SoA ECS and the jobs pool. Server-side
simulation (AI movement is gameplay), deterministic like everything else.
Post-MVP wave 2; **arena's bots are the forcing function** (they're already
planned as headless load-test clients — this is how they walk).

## Pipeline: colliders → navmesh (Recast-lineage, ours)

The generation algorithm is well-published (Recast is the de-facto industry
standard — Unreal/Unity/Godot all descend from it); we implement the pipeline
ourselves against our own data:

1. **Source = physics colliders** (topic 5 L0): the static collision world _is_
   the walkable world — no separate nav geometry authored, no mesh-vs-collision
   drift. Per-collider nav flags (walkable / not / area type) live on the
   acoustic-material-style property block.
2. **Voxelize** collider geometry into a heightfield (agent-radius/height/
   climb/slope parameters), **erode** by agent radius.
3. **Region-partition → contours → convex poly mesh** + detail mesh (surface
   height accuracy).
4. Output: **poly navmesh per tile** — compact, versioned, cooked format (bake
   step via `crcbl bake`/`crcbl nav bake`; jobs-pool parallel per tile,
   deterministic output per input hash).

**Agent classes**: one baked navmesh per radius/height class (small set,
declared per scene: `humanoid`, `large`, …). Erosion per class beats runtime
radius math — Recast's own conclusion; class count is a content decision.

## Tiled + sector-aware (galaxy discipline applied)

- Navmesh is **tiled**; tiles nest inside sectors (a sector holds an integral
  grid of nav tiles). Tiles load/unload with sector streaming (stage 6) — nav
  data is simply part of a sector's cooked payload; the ownership/migration
  model (topic 23) applies unchanged to agents.
- **Cross-tile/cross-sector stitching**: boundary edges match by construction
  (tile bake clamps to tile bounds); portals between tiles are precomputed at
  bake, portals across sector seams resolved at sector-load (same handshake
  streaming already does for physics).
- Long-range paths are **hierarchical**: sector/tile-level graph search first
  (coarse), poly-level A* within the corridor, funnel for the final string-pull.
  Galaxy-scale path queries stay bounded — an agent never A*s across a planet
  poly-by-poly.
- On-rails regions have no navmesh (nothing walks on an equation); bubbles
  bake/load nav with the rest of the sector.

## Runtime: queries + dynamic changes

- **Path query API** (batch-first, like phys queries): request in, corridor out
  — `find_path(from, to, class, area_mask)`, plus `nearest_poly`, `raycast_nav`
  (walkability ray), `random_point_in(area)`. Batched per tick on the jobs pool
  (deterministic mode); exposed to modules via the ABI as a capability group
  (topic 16).
- **Corridor following**: agents hold a path corridor (poly refs + funnel
  waypoints); replan is _local by default_ — corridor repair around a changed
  tile, full replan only when repair fails (the expensive path is the
  exception).
- **Dynamic obstacles**, two tiers:
  - _Stamped obstacles_ (Detour-temp-obstacle style): cylinders/boxes stamped
    into tiles at runtime — cheap, for doors/props/tower-placement.
  - _Tile re-bake_: geometry actually changed (destruction, building) → async
    re-voxelize affected tiles on the jobs pool; agents corridor- repair when
    the swap lands. Budgeted (N tiles/tick), deterministic swap tick (the
    rebuild is async, the _apply_ is a tick event — hash safety).
- **Area costs/flags**: per-poly area id → per-agent-query cost multipliers
  - mask (avoid water, prefer roads, forbid danger) — data, not code.
- **Off-mesh links**: authored jump/drop/door connections (editor-placed, scene
  data); traversal emits an event the game/animation reacts to (topic 17 anim
  events pair naturally: link traversal → jump state).

## Steering / crowd (deliberately thin)

- Path following + arrival + **separation** (the horde flocking-lite, promoted):
  SoA crowd system, `par_for`-friendly, thousands of agents.
- **ORCA-class local avoidance is post-topic scope** — separation + corridor
  repair covers the samples; full reciprocal avoidance lands only when a sample
  visibly needs it (the classic over-engineering trap in nav).
- Steering output = desired velocity → fed to the phys character controller or
  kinematic body (topic 5) — nav never moves anything directly; physics owns
  motion (same rule as root motion, topic 17).

## Debug + tooling

- Overlay (topic 7): navmesh polys by area color, tile bounds, corridors, funnel
  waypoints, off-mesh links, per-agent state (corridor health, replan count);
  heatmap of replan hotspots.
- Editor (stage 8 growth): nav settings panel (agent classes, per-scene), bake
  button + progress, obstacle preview, off-mesh link placement via gizmos; area
  painting post-MVP.
- CLI (topic 11): `crcbl nav bake|query|dump|stats` — bake headless, query paths
  from scripts (CI + agents), dump tiles as RON.
- Testing (topic 12): golden paths on fixture maps (known lengths ±ε); property
  tests — returned paths lie on the mesh, are connected, respect masks; bake
  determinism (same input hash → identical tiles); crowd soak = determinism
  harness (`--threads 1/N` identical, as ever); replan-storm stress (tile churn
  while 1k agents walk).

## Delivery (post-MVP wave 2 — arena era)

1. Bake pipeline (voxelize→polymesh, tiles, agent classes) + golden tests.
2. Query API (A\* + funnel + hierarchy) + batch/module bindings.
3. Corridor agents + separation crowd; arena bots walk (the proof).
4. Stamped obstacles + async tile re-bake (towers maze-mode becomes _possible_ —
   still a game-design decision, not a default).
5. Editor panel + links + overlays; `crcbl nav` CLI.

## Risks

- **Recast-pipeline correctness** is genuinely fiddly (contour simplification
  edge cases, thin-slope voxel artifacts): fixture-map golden tests from step 1,
  and the published literature is thorough — this is well-mapped from-scratch
  territory, like the UDP layer.
- **Dynamic re-bake cost spikes**: hard tile budget per tick + async-build/
  tick-apply split; the heatmap makes hotspots visible before they're reports.
- **Crowd scope creep** (ORCA, formations, flow fields): thin-by-charter; flow
  fields are a _different topic_ if an RTS sample ever exists.
- **Determinism × async re-bake**: builds happen off-thread but apply on a tick
  boundary as an event — the hash test catches any leak of build timing into sim
  state.
