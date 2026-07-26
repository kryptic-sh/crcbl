# Topic 34 — Grid Inventory + Drag-Drop

Two things in one topic because they're useless apart: a **general drag-drop
capability** in the UI system (engine, everyone gets it) and an **optional
grid-container kit** (`crcbl-inventory`) implementing Tarkov-style spatial
inventories — looting corpses and containers, moving guns/ammo/gear between
grids, equipment slots, weapon attachments. Kit rules follow the player kit
(30): first-class, optional, zero engine privileges. Drag-drop lands wave 1 (the
editor asset browser wants it); the kit is FPS-era with breach.

## Part 1 — Drag-drop as a UI capability (topic 7 extension)

- **Drag sources / drop targets** are node properties; the payload is typed data
  (an item handle, an asset id, an entity id) — the UI never knows what it
  carries, only that a target accepted the type.
- **Ghost + feedback**: the drag ghost is an ordinary UI subtree following the
  pointer/focus; targets query `can_accept(payload)` and style themselves
  through `:drop-ok` / `:drop-bad` pseudo-classes — feedback is
  stylesheet-driven like everything else (topic 7).
- **Pointer, pad, keyboard, touch — all first class**, and the pad path falls
  out of the focused-vs-engaged rule (7): **engage on a slot = pick up**,
  `ui_move` navigates while carrying, `ui_accept` drops, `ui_back` cancels back
  to origin. Touch: long-press to lift, drag, release. One interaction model,
  four devices, no special cases.
- Cross-panel and cross-window drags; auto-scroll when hovering a scrolling
  container's edge; multi-select drag (shift/ctrl or pad modifier) with the
  whole selection as one payload.
- Engine consumers besides games: editor asset browser → viewport spawn,
  outliner reparenting, VFX curve/gradient handles.

## Part 2 — The grid container kit

### Model

- **Container** = `W×H` cell grid + occupancy bitmap; **item** = `w×h` footprint
  with **90° rotation** (the Tarkov staple), or a **typed slot** entry (helmet,
  armor, primary, secondary, rig) that accepts by item tag rather than by area.
- **Nesting**: containers inside containers (rig in backpack, mags in rig), with
  a hard depth cap and cycle rejection — the classic
  bag-inside-itself/infinite-volume exploit is refused at the model level, not
  patched later.
- **Stacking**: stackable items carry count + max; split/merge are ordinary
  moves. **Attachments** (optic on rifle, mag in weapon) are typed slots on the
  item itself — the same slot machinery, no second system.
- **Aggregates propagate**: weight and volume roll up through nesting so a
  loaded backpack weighs what it contains (feeding the player kit's encumbrance
  preset knobs, 30).
- Auto-placement (`take all`, quick-move) uses deterministic first-fit with
  rotation; explicit drags carry an exact cell + rotation.

### Items are entities, and that's the anti-dupe foundation

- Every item instance is an entity with stable identity; its state (durability,
  ammo count, attached mods, container membership) is ordinary replicated
  components (4/16) — so saves (14), replays (22), and the inspector work on
  inventories for free.
- **Duplication is structurally impossible**: a move is an atomic server-side
  transaction over container membership — never "remove, then add" (the two-step
  that becomes a dupe when step two fails). Item count is an asserted invariant
  (below).

### Server authority + access grants

- **Every mutation is a command**: `Move`, `Split`, `Merge`, `Equip`, `Drop`,
  `TakeAll` — validated server-side for reach/line-of-sight, space, type
  constraints, weight caps, and ownership. Clients never assert item state (ammo
  counts, durability are server truth).
- **Contents replicate only while access is granted**: opening a corpse or crate
  = a server-granted subscription to that container's contents; revoked on
  close, out-of-range, or death. Knowing what's in an unopened box is an
  information leak — the same principle as the visibility filter (31), applied
  to loot.
- **Concurrency**: two players looting one corpse serialize on the container;
  the loser's move fails with a reason code and the client rolls back its
  optimistic view. Contested-loot policy (locks, timers) is game data, not
  engine behavior.
- Inventory commands are rate-limited and hardened like all input (23).

### Client-side optimism

Moves apply locally on drop for responsiveness and show as **pending** (dimmed)
until confirmed; denial snaps back with the reason surfaced. This is the
prediction philosophy (26) in its simplest form — the sim truth always wins, the
UI just doesn't wait to look responsive.

### Presentation

- **Item icons are baked**: item meshes rendered offscreen into an icon atlas at
  bake time via the existing screenshot machinery (`crcbl icon bake` — same
  offscreen path as topic 11). No hand-drawn icon pipeline, and icons stay in
  sync with models by construction.
- Grid + slots are ordinary CSS-styled UI (7): cells, item cards, weight bars,
  context menus. Games reskin with a stylesheet.
- **3D inspect view** (rotate a gun in a panel) reuses the render-to-texture
  camera path from PiP optics (29).

## Testing (topic 12)

- **No-dupe property (the headline)**: fuzzed concurrent move/split/merge
  streams from N clients against shared containers → total item count and
  per-item identity invariant holds; every rejected move leaves state untouched.
- Placement properties: occupancy never overlaps; rotation math correct;
  first-fit deterministic; nesting depth/cycle rejection.
- Access property: container contents never appear in any message before a grant
  or after a revoke (rides the schema position/leak tagging from 31).
- Weight/volume rollup correctness under deep nesting.
- **Drag e2e on every device**: scripted pointer _and_ pad/keyboard drags
  through HeadlessShell complete the same moves (the four-device claim, as a
  test); golden frames for grid rendering and drop-state styling.

## Delivery

| Slice                                                                                          | Phase                                               |
| ---------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| UI drag-drop capability (sources/targets/ghost/`:drop-ok`), pointer + pad/keyboard/touch paths | wave 1 (editor asset browser is the first consumer) |
| Grid/slot model + placement + nesting + stacking                                               | FPS-era                                             |
| Command protocol + server validation + atomic moves + access grants                            | FPS-era                                             |
| Client optimism + pending/rollback UX                                                          | FPS-era                                             |
| Icon bake (`crcbl icon bake`) + grid UI + 3D inspect view                                      | FPS-era                                             |
| Weight/volume rollup → player-kit encumbrance                                                  | FPS-era                                             |
| Contested-loot policy hooks, container types (mag-only, quick-slots)                           | breach-driven                                       |

## Risks

- **Scope**: a full Tarkov inventory economy (insurance, flea market, quests,
  stash tabs) is a _game_, not a kit. The kit stops at containers, grids, slots,
  stacks, attachments, and the move protocol — economy and loot tables are game
  data.
- **Pad drag UX** is genuinely hard; the engaged model gives it a coherent
  shape, but breach playtesting is what will prove it (quick-move actions —
  "send to stash", "equip" — matter more than literal dragging on a pad, and the
  kit ships both).
- **Deep nesting perf** (weight rollups, occupancy rebuilds): depth caps +
  incremental aggregate updates; property tests cover the pathological shapes.
- **Icon bake pipeline** adds a content step; mitigated by it being automatic
  (part of `crcbl bake`) rather than an artist chore.
