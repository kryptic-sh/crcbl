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
   (buildable from stage 7; genuinely fun checkpoint). **Three of its four
   slices are built — the first two 2026-09-07 and the combat content
   2026-09-10** — see "Where this stands" for what they hold and what they owe.
2. Map from editor: author the real map in stage 8 editor — this milestone _is_
   stage 8 dogfood.
3. Co-op over real transport + browser client (stage 10 exit demo: wasm client
   into native dedicated server).
4. Polish pass: world-space health bars, minimap, game-feel cheap wins.

## Where this stands

**Milestone 1's first three slices are built.** `apps/towers` is the solo loop
on a hardcoded map, with the combat content this milestone's scope line asks
for: creeps of three kinds walk a path as kinematic bodies, **three** kinds of
tower answer them — a single-target bolt, a splash tower whose shot bursts where
it lands, and a slow tower that holds what is inside its reach — each with one
upgrade tier, a kill pays gold, a scripted table of **ten** waves runs out, and
the run is won at the end of the table or lost at zero lives and then plays
itself again. `PlaceTower`, `UpgradeTower` and `StartWave` are commands the
client seals into four bytes and the server validates over `InMemoryTransport`,
so solo is already the same game the co-op build will be.

**The three kinds are three kinds, and that is asserted rather than claimed.**
The tenth row needs a splash tower **and** a slow tower to hold: five bolt
towers, a splash tower with four bolts, and a slow tower with four bolts are
each overrun by it, and a splash tower at the entry plot with a slow tower at
the gate holds the whole table — `crate::game`'s
`neither_a_splash_nor_a_slow_tower_alone_can_hold_the_last_row` plays all three
losing plans out. The economy is load-bearing with it: that plan costs far more
than an opening purse, so the bounties the early rows pay are the only way it
gets built, which `crate::wave`'s
`the_bounties_pay_for_the_plan_the_last_rows_need` is the arithmetic for. **What
those numbers are not is designed** — `docs/backlog.md` records that they were
reached by a sweep and what it would take to price the three kinds from what
each is for.

**It is on the demo site**, at `/demos/towers/`, from the same build that runs
natively — rule 7 is met and the exit criterion below that asks for a web build
that "ships and is single player" is the one thing on that list this sample can
already claim. `web/tools/browser-e2e.mjs`'s `towers` row is what holds it: that
gate builds a tower in a real browser and reads the purse pay for it, asks for
the same plot again and reads the server refuse it, sends a wave with the key
and measures it against the tick the table had it due on, watches a kill pay its
bounty back, and then does the same pair twice more for slice 3a's two new
commands — an upgrade on a plot with nothing on it refused, a kind key followed
by a build putting **that kind** up, and stepping it up taken with the purse
paying the upgrade's own price. It then **clicks a button** in the row the page
puts under the field and reads the game take that too, which is the only thing
that tells a working touch control from a rendered one. Every price and label it
writes out is held against this crate's own tables by `apps/towers`'
`the_browser_gates_game_constants_are_the_ones_this_crate_declares`.

| Slice | What it is                                                                                                                                                                                                                   | Status               |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| 1     | Solo loop on the hardcoded map: the polyline path, one creep archetype, one single-target tower, three scripted waves, shared gold and lives, the three commands over the loopback, a fixed overhead camera, the debug panel | **Built 2026-09-07** |
| 2     | The browser demo: `src/web.rs`, a `towers` directory under `web/demos/`, the `DEMOS` row, the Pages steps, the feature card and the browser gate                                                                             | **Built 2026-09-07** |
| 3a    | The combat content: the splash and slow towers, one upgrade tier each behind an `UpgradeTower` command, the tanky and swarm creeps, all ten waves, a material per kind and a burst instance at a splash impact               | **Built 2026-09-10** |
| 3b    | The presentation content milestone 1 still owes: `.crpix` art and the build menu it makes possible, spatial audio, world-space health bars                                                                                   | Owed                 |
| 4     | The dev fly/walk camera — `CharacterController` on this map, which is the controller's first real terrain                                                                                                                    | Owed                 |
| 5     | Save and resume between waves (topic 14)                                                                                                                                                                                     | Owed                 |

**What these slices prove is narrower than the "Proves" list above**, and it is
three `crcbl-phys` L0 queries this document named towers as the forcing function
for, now asked five ways. Not one line of the engine changed on the sample's
behalf — slice 3a's splash burst and slow field are both the **same**
`overlap_sphere`, which is the strongest thing that can be said about a content
slice.

- **A trigger volume is how a creep reaches the exit.**
  `PhysicsWorld::set_trigger` makes the volume non-solid, so every sweep and
  every ray passes through it and only `overlap_sphere` reports it — which is
  exactly the pair a "did anything get in here?" volume wants. `map::world`
  registers it, `creep::has_reached_the_exit` asks it, and
  `map::tests::the_exit_is_a_volume_a_bolt_flies_through_and_an_overlap_reports`
  holds both halves against the map's own geometry.
- **Acquisition is a sphere overlap, and the filter is the interesting half.**
  The same query hands a tower the ground slab and the exit volume, so
  `tower::acquire` answers with a creep or with nothing — and its test asserts
  the query really does return the other things, without which a build with no
  filter at all would pass. **Slice 3a asks it two more ways**: a splash bolt's
  `tower::burst_into` at the point it lands, which is this document's "overlap
  burst at impact point", and a slow tower's `tower::hold` once a tick, which is
  the first **continuous** reading of the query in the workspace where the other
  two are instants.
- **CCD against a target that is itself moving.** A bolt covers more ground in
  one tick than a creep is wide, and the creeps have already walked by the time
  the bolts sweep, so `sweep_sphere` over the segment is the only thing that
  sees the hit.
  `tower::tests::a_bolt_hits_a_creep_that_a_test_at_either_end_of_the_tick_would_miss`
  takes both static readings beside it: an overlap at the start of the tick and
  an overlap at the end, and neither finds the creep the sweep hit.

**The engine gap the slice found is a spline type.** Nothing in `crcbl-phys` or
`crcbl-scene` offers one, so `path` measures straight legs between `map::PATH`'s
waypoints and a creep turns a corner in a single tick. That is sample code over
kinematic bodies, exactly as the bullet below predicted; `docs/backlog.md`
carries it rather than the sample working around it.

**What the slices do not have, stated as gaps rather than as decisions.** Rule
11 is **owed, not exempted**: there is no `.crpix` art anywhere, so the tower
and creep icons, the wave banner and the two build lists are untextured
rectangles and the built-in font. Rule 8 is **owed, not exempted**: the sample
ships silent, and the "audio grammar in anger" bullet above has nothing behind
it yet. Rule 12 is half met — the three selectors are on the debug panel, the
`[HUD]` line and the summary, but there is no flag to hold a path below what the
device offers. **There is no pointer or touch input inside the canvas**, in the
window or on the page: what a tap wants to land on is the build menu slice 3b
brings with the `.crpix` art, so a hit test against the untextured lists `page`
draws today would be written to be thrown away. What a phone has instead is a
row of buttons **outside** the canvas — plot, kind, build and upgrade, in
`web/demos/towers/main.js` — which synthesise the very `keydown`/`keyup` pair
the canvas already listens for, so a finger and a keyboard reach the game down
one path and the row is thrown away with the hint text rather than with engine
code. And the debug panel's network module has nothing to report on, which is
milestone 3's problem rather than these slices'.

**What it is waiting on, and it is not one thing.**

- **Milestone 1 is under way rather than waiting.** Slices 1 and 2 are built;
  the table above is what the rest of it costs. The four ingredients this
  section used to list as present are present and three of them are now
  exercised by code: the per-collider trigger flag, `sweep_sphere` and
  `overlap_sphere` are what the sample runs on, and `CharacterController` is the
  one still untouched here — it is slice 4's, and `apps/puppet`, `apps/breach`
  and `apps/shard` drive it from three different cameras in the meantime.
- **Milestone 2 waits on the editor, which does not exist. The scene directory
  no longer holds it up.** `crcbl_scene::scn`
  ([06-assets-scenes.md](../06-assets-scenes.md)'s task 4) landed 2026-09-07 and
  `apps/breakout` reads its board out of a `.scn/` directory, so a wave that
  saves and reloads has a format to save into. What is missing is `apps/editor`:
  the workspace `Cargo.toml` records its absence as deliberate — it is a later
  phase — and `docs/plan/08-editor.md` is its design. Every "editor-built" and
  "authored in the editor" line in this doc still inherits P12, and slice 1's
  map is a table in `apps/towers/src/map.rs` for exactly that reason.
- **Milestone 3 waits on a wire.** `crcbl-net` ships `InMemoryTransport` and
  nothing else: no UDP transport, no LAN host discovery, no lobby browser. So
  "co-op over real transport" and the 4-player LAN exit criterion have no
  implementation to sit on, and the netgraph's network module has no connection
  to report on in this sample any more than it does in breakout's. The commands
  are already shaped for it, which is the one thing slice 1 could do about it.

## Exit criteria

- 4-player **LAN** co-op session completes 10 waves on a dedicated headless
  server found through the lobby browser — the engine's marquee demo, recorded.
  All clients native: a browser cannot host, cannot discover LAN hosts, and
  cannot reach a LAN server from an HTTPS page (the LAN rule in
  `docs/notes/simulation.md`).
- The **web build ships and is single player**, like every other sample's — same
  game over `InMemoryTransport`, so the wasm target cannot rot.
- Map authored 100% in the editor, zero hand-edited scene text.
- New tower type addable in one sitting by one dev following the sample's own
  docs — extensibility proof.
- It's actually fun for a session with friends. Flagship carries the bar the
  benchmarks don't.
