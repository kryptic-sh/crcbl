# Sample 07 — towers (flagship)

Co-op 3D tower defense, 1–4 players. The flagship sample: every engine pillar in
one shippable game, and the long-lived dogfood project that keeps evolving with
the engine. Genre chosen because command-latency-tolerant gameplay makes
interpolation-only MVP netcode _fully sufficient_ — multiplayer first-class
without needing prediction.

## Proves

- **Everything at once**: the integration test the small samples can't be.
- **Multiplayer as first-class citizen**: same build runs solo (in-memory
  transport) and co-op (network transport when it lands; browser client via
  stage 10 WebTransport). `PlaceTower`/`UpgradeTower`/`StartWave` are commands —
  server validates, replicates; latency is invisible by genre design.
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

## Non-goals (until engine post-MVP)

Maze-building/dynamic pathing, PvP, campaign/meta-progression, difficulty modes,
cosmetics, matchmaking (direct connect only), audio (engine gap).

## Milestones

1. Solo loop on hardcoded map: creeps walk spline, towers shoot, gold, waves
   (buildable from stage 7; genuinely fun checkpoint).
2. Map from editor: author the real map in stage 8 editor — this milestone _is_
   stage 8 dogfood.
3. Co-op over real transport + browser client (stage 10 exit demo: wasm client
   into native dedicated server).
4. Polish pass: world-space health bars, minimap, game-feel cheap wins.

## Exit criteria

- 4-player co-op session (2 native, 2 browser) completes 10 waves on a dedicated
  headless server — the engine's marquee demo, recorded.
- Map authored 100% in the editor, zero hand-edited scene text.
- New tower type addable in one sitting by one dev following the sample's own
  docs — extensibility proof.
- It's actually fun for a session with friends. Flagship carries the bar the
  benchmarks don't.
