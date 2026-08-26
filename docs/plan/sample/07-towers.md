# Sample 07 — towers (flagship)

Co-op 3D tower defense, 1–4 players. The flagship sample: every engine pillar in
one shippable game, and the long-lived dogfood project that keeps evolving with
the engine. Genre chosen because command-latency-tolerant gameplay makes
interpolation-only MVP netcode _fully sufficient_ — multiplayer first-class
without needing prediction.

## Proves

- **Everything at once**: the integration test the small samples can't be.
- **Multiplayer as first-class citizen**: same build runs solo (in-memory
  transport) and **LAN co-op** (UDP when it lands at P13, host found by direct
  address or local-network discovery). `PlaceTower`/`UpgradeTower`/`StartWave`
  are commands — server validates, replicates; latency is invisible by genre
  design.
- **Editor as content pipeline**: maps (path splines, build plots, spawn points,
  props) are authored in the stage 8 editor and shipped as `.scn/` scene dirs.
  The editor's real-world usability is measured by building towers maps in it.
- **GPU-driven at gameplay scale**: creep waves (hundreds–thousands),
  projectiles, tower instances — horde's lessons applied in a real game.
- **Full UI surface**: build menu, tower select/upgrade panel, wave timer,
  economy readout, world-space health bars (3D-space UI), minimap via ortho
  second view (the 2D story again, as a feature).
- **System-owned-array ECS as textbook**: `CreepSystem`, `TowerSystem`,
  `ProjectileSystem`, `WaveSystem`, `EconomySystem`, `PathSystem` — the sample's
  code is the ECS documentation.
- **Physics slice it drives** (interleaved build): CCD vs moving targets (TOI
  where both bodies move), kinematic spline-followers in the broadphase, trigger
  volumes (creep-reaches-exit), character controller (dev fly/walk camera on the
  map — the controller's first real terrain).
- **Audio grammar in anger** (topic 13): off-screen creep waves locatable by ear
  (direction + behind-cue), tower fire pans with the camera, occlusion muffles
  lanes behind terrain — the esports-legibility claim demonstrated in a real
  game, native + browser.

## Scope (MVP of the sample)

- 1 map (editor-built), fixed creep path (spline-follow kinematic bodies; no
  dynamic pathfinding/maze-building — that's the RTS trap).
- 3 tower types (single-target, splash, slow) + 1 upgrade tier each. All combat
  through `crcbl-phys`: tower acquisition = sphere overlap (range),
  single-target = swept-projectile CCD vs _moving_ creeps, splash = overlap
  burst at impact point.
- 3 creep types (fast, tanky, swarm), 10 scripted waves, shared team lives +
  shared gold.
- 1–4 players co-op; solo = same game over in-memory transport.
- Win/lose, restart, lobby-lite (join before wave 1; late join post-MVP).
- Save/resume (topic 14): manual + between-wave autosave; solo and
  dedicated-server co-op (world save server-side, clients rejoin into it — save
  = same snapshot machinery as join-in-progress).
- **`.crpix` art throughout the 2D layer** (sample rule 11): tower and creep
  icons, the wave banner, the build menu, the range indicators. As the flagship
  this is also where skinned buttons (`Button::with_skin`, nine-slice) stop
  being a `crcbl-vk` golden and become a real UI — a build menu is the first
  place a button's corners surviving a resize is something a player sees.
- **Debug panel on, with its network module** (sample rule 4). Towers is the
  first sample where that module is not decoration: 1–4 players over a real
  transport is what the netgraph — RTT, jitter, loss, snapshot size, tick-lead —
  was specified for, and this is the sample that finds out whether it reads.

## Non-goals (until engine post-MVP)

Maze-building/dynamic pathing, PvP, campaign/meta-progression, difficulty modes,
cosmetics, matchmaking (direct connect only), ~~audio (engine gap)~~.

**The audio non-goal is withdrawn: there is no engine gap.** `crcbl-audio` ships
— a device seam with a real-time streaming thread natively and an `AudioWorklet`
in the browser, a mixer, a spatial module and a synth — and every 2D sample on
the ladder already emits spatial cues through it. Sample rule 8 therefore
applies to towers with no exemption, and the "audio grammar in anger" bullet
above is a requirement rather than an aspiration.

## Milestones

1. Solo loop on hardcoded map: creeps walk spline, towers shoot, gold, waves
   (buildable from stage 7; genuinely fun checkpoint).
2. Map from editor: author the real map in stage 8 editor — this milestone _is_
   stage 8 dogfood.
3. Co-op over real transport + browser client (stage 10 exit demo: wasm client
   into native dedicated server).
4. Polish pass: world-space health bars, minimap, game-feel cheap wins.

## Where this stands

**There is no towers crate under `apps/`**, and towers is not a row in
`web/build.sh`'s `DEMOS` array — so nothing here has been built, and no claim in
this document has been tested against code.

**What it is waiting on, and it is not one thing.**

- **Milestone 2 waits on the editor, which does not exist.** There is no
  `apps/editor` and there is no `.scn/` directory anywhere in this tree. The
  workspace `Cargo.toml` records the absence as deliberate — the editor is a
  later phase — and `docs/plan/08-editor.md` is its design. Every "editor-built"
  and "authored in the editor" line in this doc inherits that.
- **Milestone 3 waits on a wire.** `crcbl-net` ships `InMemoryTransport` and
  nothing else: no UDP transport, no LAN host discovery, no lobby browser. So
  "co-op over real transport" and the 4-player LAN exit criterion have no
  implementation to sit on, and the netgraph's network module has no connection
  to report on in this sample any more than it does in breakout's.
- **Milestone 1 waits on neither, and that is worth stating.** A solo loop on a
  hardcoded map needs a spline follower, tower acquisition by sphere overlap, a
  swept projectile against moving creeps, and a trigger volume for
  creep-reaches-exit. `crcbl-phys` has all four ingredients today —
  `PhysicsWorld` carries a per-collider trigger flag whose colliders are
  non-solid and skipped by the sweeps, `sweep_sphere` and `overlap_sphere` are
  what breakout, asteroids and horde already run on, and `CharacterController`
  is what `apps/puppet`, `apps/breach` and `apps/shard` drive from three
  different cameras. What is genuinely absent is a **spline type**: nothing in
  `crcbl-phys` or `crcbl-scene` offers one (the only splines in the workspace
  are `crcbl-anim`'s clip interpolation and the glTF importer's), so a path
  follower would be sample code over kinematic bodies.

**So this doc's flagship status is aspirational in full.** Nothing in it has
been contradicted by a build, because there has been no build.

## Exit criteria

- 4-player **LAN** co-op session completes 10 waves on a dedicated headless
  server found through the lobby browser — the engine's marquee demo, recorded.
  All clients native: a browser cannot host, cannot discover LAN hosts, and
  cannot reach a LAN server from an HTTPS page (topic 23's LAN correction).
- The **web build ships and is single player**, like every other sample's — same
  game over `InMemoryTransport`, so the wasm target cannot rot.
- Map authored 100% in the editor, zero hand-edited scene text.
- New tower type addable in one sitting by one dev following the sample's own
  docs — extensibility proof.
- It's actually fun for a session with friends. Flagship carries the bar the
  benchmarks don't.
