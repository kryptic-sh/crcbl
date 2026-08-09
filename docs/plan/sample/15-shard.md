# Sample 15 — shard (MMO-style flagship, web slice then native world)

Persistent-world action-RPG: a shared world of streamed sectors, many
simultaneous clients, characters that persist between sessions. Built in two
milestones, **web first**.

## Two milestones, and why that order

**Milestone 1 — the web slice.** A single-player, single-zone cut of the game
that builds for `wasm32` and ships on the Pages site. Rasterised lighting,
`IndirectPerBatch` geometry, `ArrayPages` materials — every fallback path,
because a browser has no ray tracing, no mesh shaders and no bindless
([39-capabilities.md](../39-capabilities.md)).

**Milestone 2 — the native world.** The same game with the persistent shared
world underneath it: sector streaming, interest-managed replication, a dedicated
headless server, accounts, and the native rendering path on top — ray-traced
lighting, meshlet geometry, per-cluster LOD.

**The constrained target comes first deliberately.** The fallback paths are what
every browser visitor and every Apple machine runs, and a fallback proven after
the fact is a fallback nobody proved. Building the web slice first means the
raster twin and both geometry fallbacks work on real content before a single
ray-traced or meshlet feature is layered over them — and the layering is then a
capability upgrade rather than a rewrite, which is the whole claim
[39-capabilities.md](../39-capabilities.md) makes.

This also gives the Pages site a 3D flagship. Every browser figure recorded so
far comes from a 2D sample.

## Milestone 1 proves — the web slice

- **The rasterised lighting twin under real load.** A torch-lit interior zone:
  point and spot shadows, screen-space AO and reflections, irradiance probes —
  every raster effect topic 18 owes, in lighting conditions that make errors
  obvious. A dark interior is the honest test; daylight hides exactly the
  mistakes this path is prone to.
- **Both geometry fallbacks on content built to be played**, not content built
  to be tested. lumen and quarry are the acceptance fixtures; this is the load.
- **A large material set against `ArrayPages`.** Armour, weapons, environment
  kits and effects are the worst case for the paged binding model.
- **Server-authoritative gameplay in a browser** — client and server over
  `InMemoryTransport` in one wasm module, per sample rule 2, on the platform
  where "just simulate on the client" is most tempting.
- **Persistence that is a structure rather than a score**: character, inventory
  and stash through topic 14, natively in the platform data dir and in the
  browser through OPFS.
- **The grid-inventory kit gets a second consumer.** Topic 34 is written for
  breach; a kit with one consumer is that consumer's shape wearing a kit's name.
- **A real browser budget for real 3D content**, including how close the build
  comes to the wasm32 address-space ceiling — the first sample whose content
  could plausibly approach it.

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

## Exit criteria

**Milestone 1**

- A complete play session — explore, fight, loot, level, save, resume — in a
  browser, from the same build that runs natively.
- Golden frames per `GeometryPath` from a fixed camera set, plus the
  human-reviewed comparison recorded here.
- Recorded browser budget for real 3D content, and the peak wasm memory figure.
- The inventory kit used without a single engine change made on its behalf;
  anything it needed filed as a topic 34 finding instead.

**Milestone 2**

- Many concurrent clients on a dedicated server for a recorded soak duration,
  with per-client bandwidth inside the stated budget.
- A player walks across a sector boundary with no visible seam and no
  replication gap, and the state hash is unaffected by which sector they started
  in.
- World state survives a server restart mid-session; characters resume.
- Recorded three-way budget: native ray-traced, native rasterised, browser
  rasterised — the number this sample exists to produce.
