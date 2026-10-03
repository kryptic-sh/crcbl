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
- Win/lose, restart, lobby-lite. Late join is allowed: a player admitted mid-run
  plays from there (decided 2026-10-01 — see the backlog).
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
   stage 8 dogfood. **The map is scene data since 2026-09-30**: the path and the
   plots are `apps/towers/assets/scenes/field.scn/`, which the game reads and
   the editor opens, **and since 2026-10-01 the editor plays it** (F5) — and
   since 2026-10-03 builds towers on it in play, through the game's own
   commands; authoring the real map in the editor is what is left.
3. Co-op over real transport + browser client (stage 10 exit demo: wasm client
   into native dedicated server). **Its LAN half is built (2026-10-01)**:
   `--host`, `--serve`, `--join` and `--browse` over `crcbl::lan` — see "Where
   this stands" for what that holds and what is unverified.
4. Polish pass: world-space health bars, minimap, game-feel cheap wins.

## Where this stands

**Milestone 1's first three slices are built.** `apps/towers` is the solo loop
on one map, with the combat content this milestone's scope line asks for: creeps
of three kinds walk a path as kinematic bodies, **three** kinds of tower answer
them — a single-target bolt, a splash tower whose shot bursts where it lands,
and a slow tower that holds what is inside its reach — each with one upgrade
tier, a kill pays gold, a scripted table of **ten** waves runs out, and the run
is won at the end of the table or lost at zero lives and then plays itself
again. `PlaceTower`, `UpgradeTower` and `StartWave` are commands the client
seals into four bytes and the server validates over `InMemoryTransport`, so solo
is already the same game the co-op build will be.

**The map is scene data.** The path's corners and the build plots are two chunk
files in `apps/towers/assets/scenes/field.scn/` — `sys/waypoints.ron`, one
`Waypoint(order, position)` per corner, and `sys/plots.ron`, one
`Plot(label, position)` per plot — read through `crcbl_scene::scn`, compiled in
by default and replaced at run time by `--scene <DIR>`. `crate::map`'s
`Map::new` holds a layout to the rules the rest of the sample assumes and
refuses one that breaks any by name: at least two corners and at most
`MAX_WAYPOINTS`, every leg along `X` or `Z` and longer than the lane is wide, at
least one plot and at most `MAX_PLOTS`, and every plot on the field, clear of
the lane by an upgraded tower's radius and within the shortest-reaching kind's
range of it. The exit is still derived from the last corner. The committed files
are what the milestone 1 table was, byte for byte as `Scene::save` writes it,
and `apps/editor`'s shipped vocabulary registers both components and opens the
field.

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

| Slice | What it is                                                                                                                                                                                                                             | Status               |
| ----- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| 1     | Solo loop on the hardcoded map: the polyline path, one creep archetype, one single-target tower, three scripted waves, shared gold and lives, the three commands over the loopback, a fixed overhead camera, the debug panel           | **Built 2026-09-07** |
| 2     | The browser demo: `src/web.rs`, a `towers` directory under `web/demos/`, the `DEMOS` row, the Pages steps, the feature card and the browser gate                                                                                       | **Built 2026-09-07** |
| 3a    | The combat content: the splash and slow towers, one upgrade tier each behind an `UpgradeTower` command, the tanky and swarm creeps, all ten waves, a material per kind and a burst instance at a splash impact                         | **Built 2026-09-10** |
| 3b    | The presentation content milestone 1 still owes: `.crpix` art and the build menu it makes possible, spatial audio, world-space health bars                                                                                             | Owed                 |
| 4     | The dev fly/walk camera — `CharacterController` on this map, which is the controller's first real terrain                                                                                                                              | Owed                 |
| 5     | Save and resume between waves (topic 14): `S` and an autosave at each wave's end, refused mid-wave; the lobby's _Continue_ and `--resume` (solo, host, dedicated server); `save`/`load` at a server's console; the browser's OPFS save | **Built 2026-10-03** |

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
code. The debug panel has a "lan" section during a LAN session — the port, the
players and the largest snapshot on a host, the session on a joiner — but the
netgraph the network module was specified for (RTT, jitter, loss, tick-lead) is
not built anywhere yet.

**What it is waiting on, and it is not one thing.**

- **Milestone 1 is under way rather than waiting.** Slices 1 and 2 are built;
  the table above is what the rest of it costs. The four ingredients this
  section used to list as present are present and three of them are now
  exercised by code: the per-collider trigger flag, `sweep_sphere` and
  `overlap_sphere` are what the sample runs on, and `CharacterController` is the
  one still untouched here — it is slice 4's, and `apps/puppet`, `apps/breach`
  and `apps/shard` drive it from three different cameras in the meantime.
- **Milestone 2's map is in the editor's vocabulary; authoring it there is what
  is left.** Since 2026-09-30 the path and the plots are
  `apps/towers/assets/scenes/field.scn/`, `crcbl_towers::register_components`
  names their two components, and `apps/editor/src/scene.rs::vocabulary`
  registers them beside its own block, breakout's and puppet's — so the editor
  opens the field, and a directory it saves is one `--scene` plays. The
  committed field is still the milestone 1 table written out by the writer
  rather than a map authored in the editor, so the exit criterion "map authored
  100% in the editor" is not met yet: that is the dogfood pass
  (`docs/plan/08-editor.md`). The simulation itself still lives in a `Stage`
  with no ECS system, which is fine for a map — the scene is read into a `Map`
  and the stage plays on that — and would not be for anything the editor should
  place that moves.
- **The editor plays the field (2026-10-01).** `register_components` also
  registers towers' play module (`crate::game`'s `play`, under `waypoints`): F5
  in the editor builds a `Stage` from the scene through `Map::load` and ticks it
  through `TowersModule`'s solo half, as one local player asking for nothing, so
  the build phase runs down, the waves come and the creeps walk the lane and
  leak. Each creep is mirrored into the editor's world as a `Walker`, a runtime
  component the editor draws as a greybox box and never lists or saves, and a
  field `Map::load` refuses does not play, naming the rule.
- **The editor takes part in a played field (2026-10-03).** Towers registers
  play controls beside its module (`crate::game`'s `play::controls`): _Place
  tower_ on a picked plot with a kind, _Start wave_, _Upgrade_ on a picked tower
  and _Restart_. The editor lists them in a strip under its toolbar while the
  field plays, and a click is encoded into the four bytes solo's client sends
  for the same command — through the one `Controls`-to-frame conversion
  `Game::set_controls` makes — and handed to the module's next tick as its
  client inputs, so the stage validates it as it validates any player's. A
  refusal reaches the editor's status line as the label a player is shown, and
  the strip shows Lives, Gold, Wave and Outcome, read off a readout system the
  module registers in the world it plays in. A tool that ticks the module
  without taking the refusals finds no more than the newest `UNTOLD_KEPT` left
  on the stage, and the next take opens with a line counting the older ones
  dropped; solo, a host and a dedicated server take them every tick, uncapped,
  and the queue is in no state hash. Towers, bolts and bursts are mirrored
  beside the creeps (`Turret`, `Shot` and `Blast`, runtime components drawn as
  greybox boxes), so a placed tower stands on its plot, at its tier's size. Not
  looked at on a device: every check is headless.
- **A run saves and resumes between waves (slice 5, 2026-10-03).** A save is
  taken in the build phase of a run still being played — `S`, or the autosave at
  each wave's end, the first tick its last creep is out — and refused, on the
  page, while a wave is coming in or once the run is over. The table measures
  the build phase from a wave's last release, so on this field the last wave's
  creeps are usually still walking: a save carries them, the bolts in the air
  and the bursts still drawn beside the counters, the clock and the towers, and
  a resumed stage hashes as the one that saved and plays on alike tick for tick
  — which took the stage reading its physics queries in the field's order, since
  the resumed physics world is a fresh one. The bytes are a versioned `TWRS`
  payload in `crcbl-store`'s save container, naming the map by
  `Map::fingerprint`; a save of another map or version, a corrupt or truncated
  file, or a value no run between waves holds is refused by name
  (`crate::save`). Solo and the host keep one slot; the lobby offers it as
  _Continue_ and `--resume` opens on it, refusing to start without it; a
  dedicated server keeps its own and takes `save` and `load` at its console, and
  a run loaded on an empty server waits for its players. Joiners need nothing:
  their run is the host's, and a snapshot is the whole field. The browser saves
  to OPFS and opens on its save, having no lobby. **Not looked at:** a save
  loaded under real players on a LAN, and the browser's save in a browser —
  every check is headless and native.
- **The editor picks a built tower, and every mirrored thing keeps its entity
  (2026-10-03).** _Upgrade_ takes `ParamKind::PickedRuntime("turrets")`: the
  editor gives each mirrored tower a picking collider, a click on one is the
  play's runtime pick, and the encoder reads the plot off the picked `Turret`'s
  row — the client's `UpgradeTower` names a plot. A bolt carries an id
  (`Bolt::id`, the run's shot count when it was fired) and a burst its bolt's,
  so the mirror keys every creep, tower, bolt and burst by what it is (body,
  plot, id) beside the run, not by its place in the stage's swap-removed lists:
  an entity stands for one thing from the tick it arrives to the tick it goes.
  The ids are not in the state hash — the simulation never reads them, and the
  shot count they come from already is — so the hash, recorded replays and their
  re-simulation are unchanged.
- **Milestone 3's co-op over real transport is built; its exit criterion is not
  met.** Since 2026-10-01 `towers --host [PORT]` runs the stage on a
  `crcbl_server::Host` behind `crcbl::lan`'s UDP listener and announcer, and the
  host plays in it as one of its own clients, over an in-memory pair.
  `--join <IP:PORT>` and `--browse` join one: a joiner has no stage and draws
  what the host's snapshots carry — the server's world replicates the frame's
  view of the stage as `crate::replica`'s quantized entities, solo's included.
  Every player's commands are validated in admission order against one purse and
  one pool of lives, up to four players. **`towers --serve [PORT]` is the
  dedicated server**: the same host with no window, no renderer and no player of
  its own, announced on the LAN and ticking on the wall clock until `quit` is
  typed at its stdin console (`status` prints the status line), so all four
  places are joiners'; it prints a status line (players, wave, lives, gold,
  outcome) on every change and on an interval. **With nobody in the session the
  run holds still** — `run_team_tick` sees a tick with no command frame and
  steps nothing, so no build phase runs out and no wave is sent at an empty
  field until the first player joins; a player whose link dropped keeps the run
  going through their grace period. A run whose last player left for good (every
  grace period over) is reset once, so the next group starts on a fresh field
  (decided 2026-10-01). `lan::tests` has four joiners of a dedicated server on
  loopback splitting the plots between them and winning all ten waves, a browser
  finding an empty server that has sent no wave, and a player leaving mid-run
  while the other plays on. **A native `towers` opens on a lobby**
  (`crcbl_towers::lobby`) when its command line chose nothing — no session flag,
  no `--scene`, no `--headless`, `--frames` or `--screenshot`: solo, host, a row
  per LAN host this build can join (name and players), the others dimmed under
  the title with the reason (another version, build or game, or full), and a
  connect row that joins an `IP:PORT` typed into it; the arrows, Enter and a
  pad's d-pad and South drive it, as every menu. The web build has none and
  boots straight into solo. `--browse` still joins the first host of this build
  it hears. Four players winning the whole table fit every snapshot in one
  datagram with nothing held back (the largest is 767 of 1158 bytes).
  **Unverified:** two machines on a real LAN, the broadcast query reaching a
  host at all, whether a Windows firewall prompt blocks the first run, and
  `--serve`'s own wall-clock loop — every test is one process on loopback,
  driving the server a frame at a time. **The host's map is sent at join**
  (since 2026-10-01): the moment a joiner is admitted the host sends its map
  sealed on the reliable channel (`Map::to_wire`, through
  `crcbl_server::Host::send_event`), and the joiner builds its game — and its
  GPU field — only once `Map::from_wire` has read it back and held it to every
  rule a scene file is; so any joiner plays any host whatever its own `--scene`,
  the map is out of the handshake's schema (protocol version 4), a joiner the
  host accepts again on its link (`PeerEvent::Reaccepted`) is sent it again, and
  a map the joiner refuses ends the join by name. **A refused command is told to
  whoever sent it:** the host sends that player an event naming the rule
  (`game::Refusal`), solo and the host's own player get the same, and the page
  shows it as a `REFUSED: …` line for a few seconds; the map and the refusals
  share one versioned, tagged envelope (`lan::event`), and an event a player
  cannot read is counted and passed over. **A join from the lobby that fails
  comes back to it:** the lobby stays up saying `JOINING` until the map is in,
  and a refusal, a dead link, a bad map or no map within `lan::JOIN_TIMEOUT`
  leaves the player there with the reason; `--join` and `--browse` wait under a
  `JOINING` panel that shows and logs it. **So does a session from the lobby
  that ends after the map came** — the host left or shut down, a kick, or a dead
  link: the player is back in the lobby, over the idle solo run that was under
  it and its own field, with a `SESSION ENDED` warning naming how, and the
  lobby's browser listening again; `--join` and `--browse` show the end on their
  panel instead. **Not built:** the wasm client, which the LAN rule rules out.
  `docs/backlog.md`'s _What towers' LAN co-op shipped without_ has each with
  what it would take.

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
