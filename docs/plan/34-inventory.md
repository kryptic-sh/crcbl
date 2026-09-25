# Topic 34 — Grid Inventory + Drag-Drop

Two things in one topic because they're useless apart: a **general drag-drop
capability** in the UI system (engine, everyone gets it) and an **optional
grid-container kit** (`crcbl-inventory`) implementing Tarkov-style spatial
inventories — looting corpses and containers, moving guns/ammo/gear between
grids, equipment slots, weapon attachments. Kit rules follow the player kit
(30): first-class, optional, zero engine privileges. Drag-drop lands wave 1 (the
editor asset browser wants it); the kit is FPS-era with breach.

> **Status, 2026-09-07, re-checked 2026-09-25.** Part 2 is built and has two
> consumers; part 1's pointer mechanism is built, and the rest of part 1 is
> still the engine's to build. By the Delivery table's weight that is well under
> half the document, which is why it stays rather than folding into the notes.
>
> **Part 1, drag-drop: the pointer half is in `crcbl-ui`.** It shipped
> 2026-09-23 as `crcbl_ui::grid_drag` — `CellGrid`, `GridDrag<P>`, a typed
> payload, `can_accept`, drop feedback as widget state (`DropFeedback`),
> cross-grid drags and the grab offset — built on `widget.rs`'s press capture,
> and `apps/shard` and `apps/breach` use it with their own copies deleted. It is
> grids only: there is no general drag source or drop target outside a
> `CellGrid`, no ghost drawn under the pointer, and no pad, keyboard or touch
> path. A panel's `can_accept` asks `Grid::can_move_within` (2026-09-25), which
> runs `move_within`'s check against the grid as it is, the item's own cells
> counting as free, without cloning the grid or moving anything.
>
> **The styling half is not how the feedback arrives.** This section hangs
> feedback on `:drop-ok` / `:drop-bad` pseudo-classes "like everything else
> (topic 7)". `crcbl-ui` does now have a stylesheet system — `crcbl_ui::style`
> parses CSS with selectors and pseudo-classes — but it has no `:drop-ok` or
> `:drop-bad`, and `docs/backlog.md`'s 2026-09-06 decision stands: the
> capability is built against widget state, egui's and imgui's shape, and the
> panel decides what an accepting or refusing cell looks like.
>
> Also unbuilt: the first consumer named below. `apps/editor` exists, but it has
> no asset browser.
>
> **Part 2, the grid kit: built, and consumed twice.** `crates/crcbl-inventory`
> is the model half — one `Grid` with an occupancy map and an optional tag
> filter, footprints as `8×8` bitmasks in a `u64`, four rotations, deterministic
> first-fit placement, atomic moves, stacking with split and merge, and a RON
> catalogue. `apps/shard` is its first consumer and the sample that forced it:
> `src/loot.rs` (the item table, the carried grid, the drop roll),
> `src/panel.rs` (the grid drawn and dragged), a pickup intent bit through the
> wire, and the grid inside its save payload. **Not one line of the engine
> changed on that sample's behalf**, which is `sample/15-shard.md`'s own exit
> criterion; what shard wanted and did not get is in `docs/backlog.md` as topic
> 34 findings. `apps/breach` is the second consumer as of 2026-09-07 — the rig a
> first-person player carries, with the trigger gated on it holding a weapon —
> and it took no engine change either; see "Decided" below for what the second
> consumer measured.
>
> What of this document is still unbuilt is the Delivery table below: the rest
> of part 1 (above), nesting and the rollup through it, mounts and coverage,
> items as entities, the command protocol and access grants, the stash, and
> client optimism.
>
> **Icon bake:** `crcbl icon bake` is not a verb. `crcbl-cli`'s parser accepts
> `new`, `run`, `build`, `screenshot`, `replay`, `crpix`, `lod`, `import`,
> `bench`, `sim` and `settings`; neither `icon` nor `bake` parses, so the "part
> of `crcbl bake`" mitigation under Risks has nothing to be part of yet.

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

**Everything is a grid (LOCKED).** There is exactly one container primitive — a
`W×H` cell grid with an occupancy bitmap and an optional accept-filter. No
second "slot" concept exists:

| Container      | Shape                  | Filter           | Holds                              |
| -------------- | ---------------------- | ---------------- | ---------------------------------- |
| Pocket         | `1×2`                  | none             | one 1×2 item, or two 1×1 items     |
| Chest rig cell | `1×2` / `2×2` per cell | none / mags-only | whatever fits the area             |
| Backpack       | `5×10`                 | none             | any arrangement of items that fits |
| Helmet slot    | `1×1`                  | tag `helmet`     | one helmet                         |
| Optic mount    | `1×1`                  | tag `optic`      | one optic                          |

An "equipment slot" is just a 1×1 grid with a tag filter; a weapon's attachment
points are 1×1 filtered grids on the item itself. One primitive, one placement
algorithm, one persistence format — filters are data.

- **Items** carry a `w×h` footprint. **Rotation is 90° and free**: a 1×2 item
  rotates to 2×1 and fits anywhere that shape fits. Rotation is part of
  placement state (stored, persisted, replicated).
- **Fit is purely geometric**: an item goes anywhere its (possibly rotated)
  footprint has free cells — no layout rules, no reserved regions. A 5×10
  backpack accepts any arrangement that packs.
- **Nesting**: grids inside grids (rig in backpack, mags in rig), with a hard
  depth cap and cycle rejection — the classic bag-inside-itself /
  infinite-volume exploit is refused at the model level, not patched later.
- **Stacking**: stackable items carry count + max; split/merge are ordinary
  moves.
- **Aggregates propagate**: weight and volume roll up through nesting so a
  loaded backpack weighs what it contains (feeding the player kit's encumbrance
  preset knobs, 30).
- Auto-placement (`take all`, quick-move) is deterministic first-fit **trying
  both rotations**; explicit drags carry an exact cell + rotation.

### Equipment: mounts, coverage, and composition

Worn gear needs two orthogonal ideas — **where it attaches** and **what it
physically occupies**. Conflating them is what forces engines into hand-written
exclusion tables; keeping them apart makes layering rules fall out of data.

- **Mount** = which 1×1 filtered grid on the player it goes into (`head`,
  `ears`, `torso`, `armor`, `back`, `legs`…). Already the uniform primitive.
- **Coverage** = the set of body regions the item physically occupies (`skull`,
  `ears`, `face`, `torso_outer`, `torso_armor`, …). **Two worn items conflict
  iff their coverage sets intersect** — that single rule replaces every bespoke
  "can't wear X with Y" list.

| Item          | Mount   | Coverage                     |
| ------------- | ------- | ---------------------------- |
| Helmet        | `head`  | `{skull}`                    |
| Hat           | `head`  | `{skull}`                    |
| Headset       | `ears`  | `{ears}`                     |
| Chest rig     | `torso` | `{torso_outer}`              |
| Plate carrier | `torso` | `{torso_outer, torso_armor}` |
| Body armor    | `armor` | `{torso_armor}`              |

The requested rules derive with no special cases:

- helmet vs hat → same mount **and** `{skull}` ∩ `{skull}` ≠ ∅ → exclusive.
- headset → different mount, disjoint coverage → wearable with either.
- chest rig vs plate carrier → same `torso` mount → exclusive.
- **armor + chest rig** → `{torso_armor}` ∩ `{torso_outer}` = ∅ → **allowed**.
- **armor + plate carrier** → `{torso_armor}` ∩ `{torso_outer, torso_armor}` ≠ ∅
  → **refused**, because the carrier already occupies the armor layer.

Coverage vocabulary is game data — a game with exosuits or three armor layers
writes its own regions without touching the kit.

**Composition: gear is just grids it provides.** An equipment item declares the
child grids it exposes, which is exactly what distinguishes these three:

```ron
PlateCarrier( mount: "torso", coverage: ["torso_outer", "torso_armor"],
  provides: [ Grid(1,1, filter: "plate_front"), Grid(1,1, filter: "plate_back"),
              Grid(2,3), Grid(2,3) ] )            // plates AND storage

ChestRig(     mount: "torso", coverage: ["torso_outer"],
  provides: [ Grid(2,3), Grid(1,2), Grid(1,2) ] ) // storage, no plate grids

BodyArmor(    mount: "armor", coverage: ["torso_armor"],
  provides: [ Grid(1,1, filter: "plate_front"), Grid(1,1, filter: "plate_back") ] )
                                                   // plates, no storage
```

A chest rig **is** a plate carrier without plate grids; body armor **is** a
plate carrier without storage grids — stated as data, not as three
implementations. Removing a worn container takes its contents with it (nesting
already covers that).

**Two bridges out of coverage** (one data model, three consumers):

- **Protection** (28): coverage regions map to hitbox groups, so the damage
  model knows which plate the chain hit and what it protects — no parallel
  "armor zones" table.
- **Visuals** (29): coverage drives render layering and body-part hiding (helmet
  hides hair, carrier renders over rig) inside the cosmetic loadout component —
  again, no second data model.

**Conflict UX**: an equip that conflicts is refused with the offending item
named ("remove plate carrier"); games may enable auto-unequip-and-place as a kit
knob rather than reimplementing it.

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

### Persistence (three lifetimes, one format)

Because a container is always "grid + occupancy + placements", inventory
serializes as plain component data — but _where_ it persists differs by kind,
and the kit is explicit about all three:

| Kind                        | Lives in                                                       | Survives                                          |
| --------------------------- | -------------------------------------------------------------- | ------------------------------------------------- |
| **World containers**        | scene chunks (6) for authored ones; world snapshot for spawned | scene save / world save (14)                      |
| **Carried inventory**       | player entity's components                                     | world save; wiped or kept per game rules on death |
| **Persistent player stash** | **server-side store keyed by PlayerId** (27)                   | across matches and sessions                       |

- The stash is deliberately **server-side, not in the client profile** (14's
  profiles are local preferences — binds, high scores). A client-stored stash
  would be a client-authoritative item source, i.e. free duplication. Same async
  `StorageSource` seam, server data dir.
- **Stable item ids across save/load**: item entity ids are persisted, never
  regenerated — the same discipline the scene writer uses (6). Regenerating ids
  on load would break attachment references, stack identity, and any audit
  trail.
- **Versioned item schemas**: rides the per-system version + migration seam from
  14 — adding a field to an item type doesn't invalidate a stash.
- **Dropped world items**: persistence is game policy (despawn timer vs
  permanent); the kit exposes the lifetime knob and the events.
- **Atomicity across the boundary too**: moving an item from stash into a match
  load-out is a transaction over two stores — either both sides commit or
  neither (same no-dupe rule as in-match moves, and the property test covers
  store-crossing moves).

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

**Built, in `crates/crcbl-inventory` and `apps/shard`:**

- Placement: occupancy never overlaps, and a refused move leaves the grid
  untouched down to the slot id (`a_refused_move_leaves_the_grid_as_it_was`).
  The one written as a **property**, over proptest-generated shapes and grids,
  is first-fit: it finds a placement whenever a brute-force sweep can, and the
  same one. **Rotation is four-fold rather than an involution** — that is this
  crate's one departure from the sketch above and the crate docs argue it: a
  bitmask footprint admits an L, whose quarter turn is not its three-quarter
  turn, so `an_l_turned_four_times_is_the_shape_it_started_as` is written on an
  L and not on the `1×2` pocket item, and it is what catches a `turned_once`
  written as a bare transpose.
- **Conservation, from a consumer**: `apps/shard`'s floor plus its carried grid
  is the number of felled foes, before and after every pickup — a refused insert
  leaves the stack on the floor rather than losing it.
- **Persistence roundtrip**: `apps/shard` writes placements, cells, rotations,
  counts and `StackId`s into its save and reads them back; the ids are
  identical, and a payload claiming a stack no foe could have left, one stack
  twice, or two items in one cell reads as no save.
- **Scripted pointer drag through `HeadlessShell`**: press over one cell,
  release over another, and the placement is where the pointer let go.

**Not built, and each waits on the slice it belongs to:**

- **No-dupe property (the headline)**: fuzzed concurrent move/split/merge
  streams from N clients against shared containers. Needs the command protocol —
  there is no server-side inventory transaction to fuzz.
- Nesting depth and cycle rejection; weight/volume rollup under deep nesting.
- **Coverage-conflict property** against a hand-written truth table for the
  shipped vocabulary: needs mounts and coverage.
- Stash survives a server restart; store-crossing moves are atomic under
  injected failure.
- Access property: container contents never appear in any message before a grant
  or after a revoke.
- The other three devices: pad, keyboard and touch drags completing the same
  moves. The pointer one is scripted; the four-device claim is part 1's.
- Golden frames for grid rendering and drop-state styling.

## Delivery

| Slice                                                                                                                                                                                                                  | Phase                                               |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| UI drag-drop capability (sources/targets/ghost/`:drop-ok`), pointer + pad/keyboard/touch paths                                                                                                                         | wave 1 (editor asset browser is the first consumer) |
| ✅ Uniform grid model (+ filters) + placement/rotation + stacking — `crcbl-inventory`, consumed by `apps/shard` and `apps/breach`                                                                                      | shipped 2026-09-07                                  |
| Nesting: grids inside grids, depth cap and cycle rejection                                                                                                                                                             | FPS-era                                             |
| Mounts and coverage; gear as the grids it provides                                                                                                                                                                     | FPS-era                                             |
| Persistence: **server-side PlayerId stash store** and store-crossing transactions. A carried grid in a game's own save is shipped — `apps/shard/src/save.rs` writes placements, ids and rotations at payload version 2 | FPS-era                                             |
| Items as entities: stable identity as replicated components                                                                                                                                                            | FPS-era                                             |
| Command protocol + server validation + atomic moves + access grants                                                                                                                                                    | FPS-era                                             |
| Client optimism + pending/rollback UX                                                                                                                                                                                  | FPS-era                                             |
| Icon bake (`crcbl icon bake`) + 3D inspect view. A grid UI exists as a _game's_, in `apps/shard/src/panel.rs`; the engine's is part 1                                                                                  | FPS-era                                             |
| Weight/volume rollup → player-kit encumbrance. `Grid::weight_g` is the flat sum over one grid; the rollup needs nesting                                                                                                | FPS-era                                             |
| Contested-loot policy hooks, container types (mag-only, quick-slots)                                                                                                                                                   | breach-driven                                       |

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

## Decided: shard forced the kit (2026-09-06, built 2026-09-07)

Sample rule 13 — a topic that can name no adopting sample is not ready to be
built — was the question this document waited on, and the answer is in
`docs/backlog.md` under the same heading: **option 1, the kit is built from
`apps/shard` and `apps/breach` adopts it later as the second consumer.** Growing
a kit from the consumer that needs it first and then proving it with a second is
how every engine's UI kit arrives; the risk both plans name — "a kit with one
consumer is that consumer's shape wearing a kit's name" — is real and is what
breach's adoption is for.

What that risk looks like in the built crate, so the second consumer knows where
to push: the shipped model is everything shard's loot loop needed and nothing it
did not. Filters exist and shard sets none, because it equips nothing. `split`
and `merge` exist and shard calls neither, because nothing it carries is worth
splitting. Nesting is absent because a `4×4` pocket has nowhere to nest. The
parts most likely to be shard-shaped are therefore the ones with **no** consumer
yet, and breach is what will find them.

**Breach adopted it on 2026-09-07**, as `apps/breach/src/loadout.rs` and
`src/panel.rs`, with no engine change either. What the second consumer used that
the first did not is the tag vocabulary — the trigger asks whether the rig holds
anything tagged `weapon`. What neither has touched, after two consumers, is
`Grid::filter`, `split`/`merge`, any rotation but `Deg0`, and nesting; those
stay the parts with no consumer. The findings breach filed are in
`docs/backlog.md` beside shard's: `ItemDef` has no room for a game's own
numbers, and a tag question costs a slot scan.

## Correction (design review, 2026-07-27)

**Stash scope boundary, stated before someone hits it in breach.** The
server-side PlayerId store works for a single long-lived community server. A
_fleet_ of match servers (tier-3 ranked, 27) sharing one stash needs a shared
backend datastore — which 23/27 deliberately keep outside engine scope. The
line: **engine stash = per-server-instance store**; cross-fleet stash =
backend-project territory, reached through the same `StorageSource` seam so the
engine side never changes.
