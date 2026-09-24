# Sample 15 — shard (MMO-style flagship, web slice then native world)

Persistent-world action-RPG: a shared world of streamed sectors, many
simultaneous clients, characters that persist between sessions. Built in two
milestones, **web first**.

## Two milestones, and why that order

**Milestone 1 — the web slice.** A single-player, single-zone cut of the game
that builds for `wasm32` and ships on the Pages site. Rasterised lighting,
`IndirectPerBatch` geometry, `ArrayPages` materials — every fallback path,
because a browser has no ray tracing, no mesh shaders and no bindless
([backends notes](../../notes/backends.md)).

**Milestone 2 — the native world.** The same game with the persistent shared
world underneath it: sector streaming, interest-managed replication, a dedicated
headless server, accounts, and the native rendering path on top — ray-traced
lighting, meshlet geometry, per-cluster LOD.

**The constrained target comes first deliberately.** The fallback paths are what
every browser visitor and every Apple machine runs, and a fallback proven after
the fact is a fallback nobody proved. Building the web slice first means the
raster twin and both geometry fallbacks work on real content before a single
ray-traced or meshlet feature is layered over them — and the layering is then a
capability upgrade rather than a rewrite, which is the whole claim topic 39
makes (see the [backends notes](../../notes/backends.md)).

This also gives the Pages site a 3D flagship. Every browser figure recorded so
far comes from a 2D sample.

## Milestone 1 proves — the web slice

- **The rasterised lighting twin under real load.** A torch-lit interior zone:
  point and spot shadows, screen-space AO and reflections, irradiance probes —
  every raster effect topic 18 owes, in lighting conditions that make errors
  obvious. A dark interior is the honest test; daylight hides exactly the
  mistakes this path is prone to.
- **Both geometry fallbacks on content built to be played**, not content built
  to be tested. lantern and quarry are the acceptance fixtures; this is the
  load.
- **A large material set against `ArrayPages`.** Armour, weapons, environment
  kits and effects are the worst case for the paged binding model.
- **Server-authoritative gameplay in a browser** — client and server over
  `InMemoryTransport` in one wasm module, per sample rule 2, on the platform
  where "just simulate on the client" is most tempting.
- **Persistence that is a structure rather than a score**: character, inventory
  and stash through topic 14, natively in the platform data dir and in the
  browser through OPFS.
- **The grid-inventory kit gets a consumer.** Topic 34 was written for breach,
  and this doc used to say shard should be its _second_ consumer — a kit with
  one consumer is that consumer's shape wearing a kit's name. `docs/backlog.md`
  settled it the other way on 2026-09-06, because breach's inventory sits in
  milestones that are native-only by that sample's own reasoning and nothing was
  forcing the kit at all: shard builds it, breach adopts it, and **breach's
  adoption is what the original sentence was really asking for**.
- **A real browser budget for real 3D content**, including how close the build
  comes to the wasm32 address-space ceiling — the first sample whose content
  could plausibly approach it. Measured on 2026-09-07, and it does not: the
  answer is 376 times under, because the content is in GPU objects rather than
  in the linear memory. The exit criteria below carry the reading.

## Milestone 2 proves — the native world

- **Sector-scoped interest management at scale.** Topic 23's sector-scoped
  envelopes and ack-baseline deltas exist; nothing has driven them with players
  spread across a world. towers is four players in one map, breach is ten in one
  arena. This is the sample where a client is sent a fraction of the world and
  the fraction changes as it moves.
- **Sector streaming against the galaxy-scale position model** — `WorldPos`,
  rebasing, load/unload with hysteresis, and a seam a player can walk across
  without seeing it.
- **A dedicated headless server with real uptime**, hosted on the local network:
  many concurrent clients, a long soak, world state saved server-side per topic
  14, and a bandwidth budget the priority encoder actually has to respect.
- **Characters belong to the host's world.** Identity is per server rather than
  per service (topic 27, PSK-shaped): you join a friend's shard and your
  character lives there. No accounts service, no cross-server transfer — the
  interesting problem is world persistence, not identity federation.
- **The native rendering path layered on the web slice**: ray-traced lighting,
  meshlet geometry with per-cluster LOD, and the recorded three-way budget —
  native ray-traced, native rasterised, browser rasterised.

## Scope (hard caps)

- **Milestone 1**: one zone, modular hand-authored pieces assembled per seed, a
  handful of enemy archetypes and abilities, loot with rarity, an inventory
  grid, level-ups. Camera isometric-ish but pulled closer than the genre's
  convention, so lighting and material detail stay legible.
- **Milestone 2**: a small world of a few streamed sectors — enough that
  streaming and interest management are real, not enough to be an art project.
  Concurrent-client target recorded in this doc when measured, not guessed.
- **Multiplayer is LAN**, as it is for breach: direct connect by IP or a lobby
  browser over local-network host discovery. No hosted service, no relay, no
  cross-server anything. A persistent world on a machine in the same room is
  still a persistent world, and it exercises streaming, interest management and
  server-side saves exactly the same way.
- Modular tiling pieces are deliberate: they exercise border locking in topic
  25's simplifier, where a wall segment whose LOD chain breaks its own edges is
  visible the moment the next segment no longer meets it.

## Non-goals (hard cap)

PvP of any kind, ranked or matchmaking, chat and social systems, an economy or
trading between players, open-world scale beyond a few sectors, raids or group
content. No competitive integrity claims — that is breach's problem and a reason
breach is native only. The web milestone additionally ships **no networking at
all**: it is single player, and remote sessions are milestone 2's job on native.

**Exempt from sample rule 11** (`.crpix` art through the sprite pass): the
subject is a lit 3D world. Rules 4 and 12 apply in full — path reporting matters
here more than anywhere, because this is the sample where the fallback paths
carry real content.

## Where this stands

**Milestone 1's first slice is built.** `apps/shard` is a torch-lit interior
zone walked in an isometric-ish third person, running natively and in a browser
from one build; `web/demos/shard/` is the page and `shard` is a row in
`web/build.sh`'s `DEMOS` array. It is what this doc's milestone 1 exists to be:
the **load** on the fallback paths rather than a fixture for them —
`apps/lantern` and `apps/quarry` are the acceptance fixtures, and this is a zone
of modular tiles with a torch over every brazier and a spot over the shrine,
more lights than there are shadow slots to give them, screen-space occlusion and
reflections, and an irradiance volume the engine's updater fills every frame,
all in a dark interior where a mistake in any of them shows. It also gives the
Pages site the 3D flagship this doc asks for; every browser figure recorded
before it came from a 2D sample.

**Every renderer feature it leans on already existed** — shadows, effects and
probes in `crates/crcbl-render/` — but saying none of them gained a line on
shard's behalf was wrong within the hour it was written: `e2c3584`, "let a
second point light cast", changed `shadow.rs`, `forward.rs` and `mesh.slang` and
names this sample as the cause. Leaning on a feature is how its gaps get found;
what the sample did not need was a new feature. Nor did `crcbl-store`: the save
is `crcbl::store::save::SaveWriter`'s container, the platform data directory
natively and OPFS in a browser, and what is this sample's is the payload inside
the one sector and which directory it goes in. Nor did `crcbl-phys`:
`apps/shard/src/camera.rs` is a **third** rig on the one `CharacterController`,
after `apps/puppet`'s orbit and `apps/breach`'s first person, and it is the one
whose camera the player barely controls — fixed elevation, fixed distance, a yaw
that moves in quarter turns.

**The sample has goldens now, and one measurement that is not one.**
`apps/shard/tests/run-shard-golden.sh` is the first thing in `ci.yml` that runs
`apps/shard` on a real driver — the zone from four bearings on three geometry
paths, against four committed references — and it is also where the one recorded
shortfall of this milestone's picture lives: the reference is drawn with
anisotropic filtering off, because radv and llvmpipe do not agree about it on a
floor this grazing and nothing else in the frame accounts for any of the
difference. The browser budget and the peak wasm memory figure are taken as of
2026-09-07, and the exit criteria below record them: a frame of this zone costs
0.983 ms of GPU p50 on a hardware adapter and 5.0 to 12.7 **seconds** under CI's
SwiftShader, and the linear memory peaks at 10.9 MiB — a quarter of one percent
of what a wasm32 module can address.

**The zone is one authored table and everything else is read off it**: a floor
slab per open tile, a solid block per wall tile, pillars, a dais, braziers, and
doorways with holes through them. The meshes and the colliders walk the _same_
grid, so what looks solid is solid. There is no roof, because the camera is
above one. This is the modular kit this doc asks for deliberately — the pieces
topic 25's border locking has to hold together — at its first size.

**All six of milestone 1's verbs are here: explore, fight, loot, level, save,
resume.** A felled foe leaves a stack where it falls, `F` takes it into a `4×4`
grid on the character, `I` opens the panel that draws it, a pointer drag moves
items between cells, and the save carries the grid — placements, rotations,
counts and stack ids — across a resume. That grid is
`docs/plan/34-inventory.md`'s kit, `crates/crcbl-inventory`, and **shard is its
first consumer rather than its second**: `docs/backlog.md`'s decision of
2026-09-06 is that shard forces the kit and breach adopts it later, which is the
one place this doc's original plan was overtaken. The exit criterion that
depends on it — "used without a single engine change made on its behalf" — is
**met**: nothing in `crcbl-inventory`, `crcbl-ui` or `crcbl` changed for either
slice, and what shard wanted from them is filed as topic 34 findings.

**Level and rarity arrived together**, because each is what makes the other
worth having. Felling a foe is worth `foe::Kind::experience` — a warden is five
blows and a husk is one, so the zone pays for the risk rather than for the count
— and taking what it left is worth its tier. Both go into one running total, and
`apps/shard/src/level.rs` is a **table of thresholds** rather than a curve: the
total each level begins at, written out, so the progression is something a
reviewer reads instead of something only a run knows. A level deepens the
character's health pool and does nothing else — the ceiling rises, the health
under it does not, and a return to the spawn is what pours the deeper one — and
the table's last row is set against the _least_ a full clear pays out, so the
top level is reachable on every seed rather than on the lucky ones.

**A tier is rolled the way the item and the count already were**: a hash of the
seed and the foe's index off a third salt, so it has no history and a zone
cleared in another order leaves the same haul at the same tiers. Nothing stores
it — a tier is that hash wherever it is asked for, on the floor, in the grid or
after a resume — which is why there is no side table keyed by `StackId` and no
rarity field in the payload. It is **visible** as the outline every cell of a
stack is drawn in, and it **means** something the fight verb can feel: no verb
here uses an item — nothing is equipped, eaten or swung — so what a tier scales
is what the find teaches. The overlay carries the level and how far into it the
character is, the `[HUD]` heartbeat carries the same pair, and the save is at
payload version 3, holding the experience and deriving the level from it.

What a level is **not** yet is something to spend: there is no skill, no stat
point and no equipment. There is no sector streaming and no networking of any
kind — the plan says milestone 1 ships none, and the loopback here is sample
rule 2 rather than a network. The recorded browser budget, the peak wasm memory
figure and the golden frames are all taken (see below).

**One absence is in the picture rather than in the feature list: the character
is a capsule.** It is the _same_ capsule `crcbl::phys::CharacterConfig` sweeps,
so the figure on screen is the shape the physics moved; an authored rig would be
a second character system with no animation to drive it, and `apps/puppet` is
the sample that owns that seam.

**Milestone 2 is entirely unstarted**, and its dependency is outside this
sample: `crcbl-net` ships `InMemoryTransport` and nothing else, so there is no
wire for a dedicated server, interest management or sector-scoped replication to
run over.

## Exit criteria

**Milestone 1**

- ✅ A complete play session — explore, fight, loot, level, save, resume — in a
  browser, from the same build that runs natively. Met 2026-09-07: the loot
  section at the end of `web/tools/browser-e2e.mjs`'s fight block is the last
  two verbs. It presses `F` where the fight left the character, and reads the
  kill's worth, the find's worth and the level pair off the `[HUD]` heartbeat —
  the kill pays what `foe::Kind::experience` gives the kind that actually fell,
  the find pays what its tier does, and the two together cross the first row of
  `level::THRESHOLDS`, so the level turns on the pickup.
- ✅ Golden frames per `GeometryPath` from a fixed camera set, plus the
  human-reviewed comparison recorded here. Met 2026-09-07:
  `apps/shard/tests/golden.rs` draws the zone at 256×192 from the four bearings
  `Iso` can be in — computed from the rig rather than pressed for, since a
  headless run receives no `Q`/`E` — once on each of `MeshShader`,
  `IndirectCount` and `IndirectPerBatch`, each reached by subtracting features
  from one adapter the way `apps/quarry/tests/device/harness.rs` does. Every
  path is held to the _same_ reference per bearing, on `apps/lantern`'s
  argument: a lesser tail is a constraint on submission, not a second renderer,
  and on radv all three draw the four frames bit for bit identically. Reviewed
  by opening all four: the capsule standing on the tiled floor inside its pool
  of torchlight at the spawn and half bearings, and at the two quarter turns a
  brazier blazing on the aisle behind it with a lit dais slab beside it — no
  frame black or blank. Blessed on an RX 7900 XTX (radv, Mesa 26.2.2) and
  compared on llvmpipe (LLVM 22.1.8), where the worst of the twelve puts 0.3479%
  of the frame over `Tolerance::RASTERISER`'s per-channel delta against a 1%
  budget, 0.0020% grossly wrong against 0.1%, and ssim 0.999142 against a 0.99
  floor. The suite's device withholds `SAMPLER_ANISOTROPY`, which is the whole
  of what the two rasterisers disagree about here — see that file's `BASE`.
- ✅ Recorded browser budget for real 3D content, and the peak wasm memory
  figure. Met 2026-09-07. Both come off the browser gate now rather than out of
  this doc, so a later run re-takes them: the budget is the engine's own
  `gpu passes (p50 / p95)` table, printed into the page log at exit, and the
  heap is a `[MEM]` line `web/demos/shard/main.js` prints whenever the linear
  memory grows and once on its last frame.

  **The budget, from three runs of the same page at 959×463**, all three drawing
  through `IndirectPerBatch`, `ArrayPages` and `Rasterised`:
  - **A hardware adapter** — this machine, Chrome on WebGPU, adapter reported as
    `amd rdna-3`: 26 labels, **0.983 ms of p50** for the whole frame, of which
    `forward` is 0.213 / 0.214 ms (21.7%), `ssr` 0.196 ms (19.9%) and
    `probe-gather` 0.114 ms (11.6%). Nothing here is the limit: the CPU clock
    reads mean 16.666 ms (60.0 fps), and the gate's own steady-state reading —
    82 replayed frames in 1.4 s, 16.6 ms a frame — is measuring the display's
    cadence rather than the zone. Reproducible: a second run of the same gate
    printed the same 0.983 ms of p50, with `forward` at 0.212 / 0.213 ms.
  - **CI's SwiftShader, on a fast runner**: 26 labels, **5033.952 ms of p50**,
    of which `forward` alone is 4250.733 / 4375.511 ms — **84.4%** of the frame.
  - **CI's SwiftShader, on a slow one**: 26 labels, **12681.860 ms of p50**,
    `forward` 11198.427 / 11461.437 ms — **88.3%**.

  So the same frame costs a millisecond on a GPU and five to thirteen seconds on
  a software rasteriser, and the whole of that spread is the forward pass — the
  lit, clustered, many-light one this zone exists to load. Two runners on the
  same image differ from each other by 2.5x, which is why every wait in
  `web/tools/browser-e2e.mjs` is scaled by a slowdown it measures rather than
  fixed. The CPU means those two runs print, 60.845 ms and 62.422 ms, are
  **not** a frame time: the fixed-step clock caps a frame at 64 ms, and both are
  sitting on that cap.

  **The peak wasm heap is 11 403 264 bytes — 10.9 MiB**, on the hardware run,
  and the same figure on each of the four page loads that run makes. That is
  **0.27% of the 4 GiB a wasm32 module can address** — 376 times under it — so
  the sample this doc expected to be the first to approach the ceiling is not
  near it at all: the zone's meshes and textures live in GPU objects the page
  holds, not in the linear memory, which carries game state, the CPU side of the
  zone and the two stream buffers. `WASM_HEAP_CEILING` in
  `web/tools/browser-e2e.mjs` is set at 32 MiB against that reading, and
  `web/run-browser-e2e.sh` fails a shard run whose driver never took it.

  **The heap on CI is untaken until the next Pages run.** The page change that
  prints it is newer than the evidence above, so the SwiftShader logs quoted
  here carry no `[MEM]` line; the figure will be in the next `web-e2e-shard`
  artifact's `shard-swiftshader.log`, which is that run's browser console.

- ✅ The inventory kit used without a single engine change made on its behalf;
  anything it needed filed as a topic 34 finding instead. Met 2026-09-07: the
  kit is consumed through `crcbl::inventory`, the drag is built inside the
  sample out of `UiState`'s press capture, and the findings — a typed drag-drop
  capability, a `PointerUpdate::pixels` to match `TouchUpdate`'s, a
  `Grid::relink` for a loaded grid — are in `docs/backlog.md`.

**Milestone 2**

- Many concurrent clients on a dedicated server for a recorded soak duration,
  with per-client bandwidth inside the stated budget.
- A player walks across a sector boundary with no visible seam and no
  replication gap, and the state hash is unaffected by which sector they started
  in.
- World state survives a server restart mid-session; characters resume.
- Recorded three-way budget: native ray-traced, native rasterised, browser
  rasterised — the number this sample exists to produce.
